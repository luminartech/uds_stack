//! UDS client for sending diagnostic requests over DoIP.

use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use simple_doip::{
    LogicalAddress, TCP_PORT, TESTER_LOGICAL_ADDRESS,
    client::{AddressType, Client, ClientOptions, RoutingActivationOptions},
    connection::{Connector, ConnectorSocket},
    messages::{ActivationTypeCode, Message, Payload, ProtocolVersion},
};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uds_protocol::{
    DiagnosticDefinition, DiagnosticSessionType, ProtocolRequest, ProtocolResponse, Request,
    Response, SingleValueWireFormat, UdsSpec, WireFormat,
};

use crate::{Error, Result, SessionConfig};

/// A UDS client that manages diagnostic sessions over DoIP.
///
/// This client wraps the low-level DoIP client and provides a higher-level
/// interface for sending UDS requests and receiving responses.
///
/// ## NRC 0x78 Handling
///
/// This client automatically handles UDS Negative Response Code 0x78 (Response Pending).
/// When the server needs more time to process a request, it sends NRC 0x78 and this client
/// will wait with an extended timeout (P2*) for the final response, without re-sending
/// the original request.
///
/// ## Generic Connection Type
///
/// The client is generic over the connection type `Conn`, allowing it to work with
/// different transport implementations (e.g., standard TCP sockets or VCC-specific listeners).
pub struct UdsClient<Conn = ConnectorSocket> {
    /// The underlying DoIP client, shared with the keepalive task.
    doip_client: Arc<Mutex<Client<Conn>>>,
    /// Session configuration (used for automatic tester present and timeout handling).
    config: SessionConfig,
    /// Tracks the last time a UDS message was sent, so the keepalive task
    /// only sends TesterPresent when the session is idle.
    last_activity: Arc<std::sync::Mutex<Instant>>,
    /// Handle for the background keepalive task, if auto_tester_present is enabled.
    keepalive_handle: Option<tokio::task::JoinHandle<()>>,
    _phantom: PhantomData<Conn>,
}

/// Options for connecting to a UDS server.
#[derive(Debug, Clone)]
pub struct UdsClientOptions {
    /// Server IP address.
    pub server_ip: IpAddr,
    /// Server port (defaults to 13400).
    pub server_port: u16,
    /// Server logical address (ECU address).
    pub server_logical_address: LogicalAddress,
    /// Server physical address.
    pub server_physical_address: LogicalAddress,
    /// Client logical address (tester address).
    pub client_logical_address: LogicalAddress,
    /// DoIP protocol version.
    pub protocol_version: ProtocolVersion,
    /// Whether to perform routing activation on connect.
    pub routing_activation: bool,
    /// Session configuration.
    pub session_config: SessionConfig,
}

impl Default for UdsClientOptions {
    fn default() -> Self {
        Self {
            server_ip: [127, 0, 0, 1].into(),
            server_port: TCP_PORT,
            server_logical_address: LogicalAddress(0x0001),
            server_physical_address: LogicalAddress(0x4010),
            client_logical_address: TESTER_LOGICAL_ADDRESS,
            protocol_version: ProtocolVersion::V2012,
            routing_activation: true,
            session_config: SessionConfig::default(),
        }
    }
}

impl UdsClientOptions {
    /// Create new options with the given server IP.
    pub fn new(server_ip: IpAddr) -> Self {
        Self {
            server_ip,
            ..Default::default()
        }
    }

    /// Set the server logical address.
    pub fn with_server_logical_address(mut self, address: u16) -> Self {
        self.server_logical_address = LogicalAddress(address);
        self
    }

    /// Set the server physical address.
    pub fn with_server_physical_address(mut self, address: u16) -> Self {
        self.server_physical_address = LogicalAddress(address);
        self
    }

    /// Set the client logical address.
    pub fn with_client_logical_address(mut self, address: u16) -> Self {
        self.client_logical_address = LogicalAddress(address);
        self
    }

    /// Set whether to perform routing activation.
    pub fn with_routing_activation(mut self, enabled: bool) -> Self {
        self.routing_activation = enabled;
        self
    }

    /// Set the session configuration.
    pub fn with_session_config(mut self, config: SessionConfig) -> Self {
        self.session_config = config;
        self
    }

    /// Convert to DoIP ClientOptions.
    pub fn to_doip_options(&self) -> ClientOptions {
        ClientOptions {
            server_address: SocketAddr::new(self.server_ip, self.server_port),
            server_logical_address: self.server_logical_address,
            server_physical_address: self.server_physical_address,
            client_address: IpAddr::from([0, 0, 0, 0]),
            client_logical_address: self.client_logical_address,
            protocol_version: self.protocol_version,
            routing_activation_options: if self.routing_activation {
                Some(RoutingActivationOptions {
                    activation_type: ActivationTypeCode::Default,
                    oem_specific: None,
                })
            } else {
                None
            },
        }
    }
}

/// Background task that sends TesterPresent (0x3E 0x80) when the session is idle.
///
/// Runs in a loop, sleeping for `interval` and then checking if enough time has
/// elapsed since the last UDS activity. If idle, sends TesterPresent with the
/// suppress-positive-response sub-function to keep the ECU's S3 timer alive.
async fn keepalive_loop<Conn>(
    doip_client: Arc<Mutex<Client<Conn>>>,
    last_activity: Arc<std::sync::Mutex<Instant>>,
    interval: Duration,
) where
    Conn: Connector + 'static + Send + Sync,
{
    loop {
        tokio::time::sleep(interval).await;

        let elapsed = last_activity.lock().unwrap().elapsed();
        if elapsed < interval {
            continue;
        }

        let mut client = doip_client.lock().await;
        match client
            .send_diagnostic_message(AddressType::Physical, vec![0x3E, 0x80])
            .await
        {
            Ok(()) => {
                debug!("Sent keepalive TesterPresent");
                *last_activity.lock().unwrap() = Instant::now();
            }
            Err(e) => {
                warn!("Keepalive TesterPresent failed: {}", e);
                // Don't update last_activity on failure so we retry sooner
            }
        }
    }
}

impl UdsClient<ConnectorSocket> {
    /// Connect to a UDS server over DoIP using the default connector.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or routing activation fails.
    pub async fn connect(options: UdsClientOptions) -> Result<Self> {
        let doip_options = options.to_doip_options();

        info!(
            "Connecting to UDS server at {}:{}",
            options.server_ip, options.server_port
        );

        let client = Client::<ConnectorSocket>::connect(doip_options).await?;

        info!("Connected to UDS server");

        let doip_client = Arc::new(Mutex::new(client));
        let last_activity = Arc::new(std::sync::Mutex::new(Instant::now()));
        let keepalive_handle = if options.session_config.auto_tester_present {
            Some(tokio::spawn(keepalive_loop(
                Arc::clone(&doip_client),
                Arc::clone(&last_activity),
                options.session_config.tester_present_interval,
            )))
        } else {
            None
        };

        Ok(Self {
            doip_client,
            config: options.session_config,
            last_activity,
            keepalive_handle,
            _phantom: PhantomData,
        })
    }
}

impl<Conn> UdsClient<Conn>
where
    Conn: Connector + 'static + Send + Sync,
{
    /// Create a UDS client from an existing DoIP client.
    ///
    /// This is useful when you have a pre-connected DoIP client (e.g., for VCC).
    pub fn from_doip_client(client: Client<Conn>, config: SessionConfig) -> Self {
        let doip_client = Arc::new(Mutex::new(client));
        let last_activity = Arc::new(std::sync::Mutex::new(Instant::now()));
        let keepalive_handle = if config.auto_tester_present {
            Some(tokio::spawn(keepalive_loop(
                Arc::clone(&doip_client),
                Arc::clone(&last_activity),
                config.tester_present_interval,
            )))
        } else {
            None
        };

        Self {
            doip_client,
            config,
            last_activity,
            keepalive_handle,
            _phantom: PhantomData,
        }
    }

    /// Get the session configuration.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Send a UDS request and wait for the response.
    ///
    /// Returns `Ok(None)` when the request has the SPRMIB (Suppress Positive Response
    /// Message Indication Bit) set, since the ECU will not send a positive response.
    ///
    /// This method automatically handles NRC 0x78 (Response Pending) by waiting with
    /// an extended timeout (P2*) for the final response without re-sending the request.
    ///
    /// If the connection is lost (e.g., sensor reboot after a session change), this
    /// method will reconnect and wait for the response on the new connection. Only if
    /// the response does not arrive within the timeout will the request be re-sent.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send<D: DiagnosticDefinition>(
        &self,
        request: Request<D>,
        address_type: AddressType,
    ) -> Result<Option<Response<D>>>
    where
        D::DID: SingleValueWireFormat,
        D::DiagnosticPayload: SingleValueWireFormat,
        D::RID: SingleValueWireFormat,
        D::RoutinePayload: SingleValueWireFormat,
    {
        let suppress_response = request.is_positive_response_suppressed();

        // Encode the request to bytes
        let mut request_bytes = Vec::with_capacity(request.required_size());
        request
            .encode(&mut request_bytes)
            .map_err(|e| Error::InvalidResponse(format!("Failed to encode request: {e}")))?;

        debug!("Sending UDS request: {:02X?}", request_bytes);

        *self.last_activity.lock().unwrap() = Instant::now();
        let mut client = self.doip_client.lock().await;

        // Track whether a reconnection happened without re-sending the request.
        // When the connection drops after sending, the request was likely already
        // received by the server. We reconnect and wait for the response on the
        // new connection first. Only if no response arrives do we re-send.
        let mut reconnected_without_resend = false;
        let mut pending_message: Option<Message> = None;

        // Attempt to send, with reconnection if needed
        let send_result = client
            .send_diagnostic_message(address_type, request_bytes.clone())
            .await;

        if let Err(ref e) = send_result {
            if self.config.auto_reconnect && Self::is_connection_error(e) {
                warn!("Connection error during send, attempting reconnect: {}", e);
                let reconnect_timeout = self.config.reconnect_timeout;
                match Self::attempt_reconnect(&mut client, reconnect_timeout).await {
                    Ok(Some(msg)) => {
                        Self::send_tester_present_locked(&mut client, &self.last_activity).await;
                        pending_message = Some(msg);
                    }
                    Ok(None) => {
                        Self::send_tester_present_locked(&mut client, &self.last_activity).await;
                        // Reconnected but no response yet — the request was likely
                        // received before the disconnect. Wait for the response on
                        // the new connection before re-sending.
                        reconnected_without_resend = true;
                    }
                    Err(e) => return Err(e),
                }
            } else {
                return Err(Error::Transport(send_result.unwrap_err()));
            }
        }

        // When SPRMIB is set, the ECU will not send a positive response
        if suppress_response {
            debug!("Positive response suppressed (SPRMIB set), not waiting for response");
            return Ok(None);
        }

        // Wait for response using the response timeout (not reconnect timeout).
        // The response timeout is reset after each reconnection, so reconnection
        // time doesn't eat into the response wait time.
        let mut response_start = Instant::now();
        let response_timeout = self.config.response_timeout;

        loop {
            // Check if we've exceeded response timeout
            if response_start.elapsed() >= response_timeout {
                if reconnected_without_resend {
                    // No response arrived on the new connection — re-send the request.
                    // This is safe because the server is already in the requested state
                    // (e.g., session control response was lost with the old connection).
                    info!("No response after reconnection, re-sending request");
                    client
                        .send_diagnostic_message(address_type, request_bytes.clone())
                        .await?;
                    reconnected_without_resend = false;
                    response_start = Instant::now();
                    continue;
                }
                return Err(Error::Timeout(response_timeout));
            }

            // Use pending message from reconnection if available, otherwise wait for one
            let message = if let Some(msg) = pending_message.take() {
                msg
            } else {
                // Use remaining response time or 1 second, whichever is smaller
                let remaining = response_timeout.saturating_sub(response_start.elapsed());
                let receive_timeout = remaining.min(Duration::from_secs(1));

                let receive_result = client.receive_diagnostic_response(receive_timeout).await;

                match receive_result {
                    Ok(msg) => msg,
                    Err(ref e) if self.config.auto_reconnect && Self::is_connection_error(e) => {
                        // Connection lost while waiting for response — reconnect and
                        // continue waiting. The request was likely already processed.
                        warn!(
                            "Connection error during receive, attempting reconnect: {}",
                            e,
                        );
                        let reconnect_timeout = self.config.reconnect_timeout;
                        match Self::attempt_reconnect(&mut client, reconnect_timeout).await {
                            Ok(Some(msg)) => {
                                Self::send_tester_present_locked(&mut client, &self.last_activity)
                                    .await;
                                info!("Using message received during reconnection");
                                pending_message = Some(msg);
                                response_start = Instant::now();
                                continue;
                            }
                            Ok(None) => {
                                Self::send_tester_present_locked(&mut client, &self.last_activity)
                                    .await;
                                // Reconnected — continue waiting for the response on
                                // the new connection. Will re-send if timeout expires.
                                info!("Reconnected — waiting for response on new connection");
                                reconnected_without_resend = true;
                                response_start = Instant::now();
                                continue;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(simple_doip::Error::ResponseTimeoutExceeded) => {
                        // Per-receive timeout - continue loop to check response timeout
                        continue;
                    }
                    Err(e) => return Err(Error::Transport(e)),
                }
            };

            let response_bytes = Self::extract_diagnostic_payload(&message)?;
            debug!("Received UDS response: {:02X?}", response_bytes);

            // Validate response corresponds to our request
            Self::validate_response_matches_request(&request_bytes, &response_bytes)?;

            // Check for NRC 0x78 (Response Pending)
            if Self::is_response_pending(&response_bytes) {
                debug!(
                    "Received NRC 0x78 (Response Pending), resetting timeout and waiting for final response"
                );
                response_start = Instant::now();
                continue; // Wait for next response WITHOUT re-sending
            }

            // Final response - decode and return
            let response = Response::<D>::decode(&mut response_bytes.as_slice())
                .map_err(|e| Error::InvalidResponse(format!("Failed to decode response: {e}")))?;
            return Ok(Some(response));
        }
    }

    /// Send a raw UDS request (as bytes) and wait for the response.
    ///
    /// This method automatically handles NRC 0x78 (Response Pending) by waiting with
    /// an extended timeout (P2*) for the final response without re-sending the request.
    ///
    /// If the connection is lost (e.g., sensor reboot after a session change), this
    /// method will reconnect and wait for the response on the new connection. Only if
    /// the response does not arrive within the timeout will the request be re-sent.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn send_raw(&self, request_bytes: Vec<u8>) -> Result<Vec<u8>> {
        debug!("Sending raw UDS request: {:02X?}", request_bytes);

        *self.last_activity.lock().unwrap() = Instant::now();
        let mut client = self.doip_client.lock().await;

        // Track whether a reconnection happened without re-sending the request.
        let mut reconnected_without_resend = false;
        let mut pending_message: Option<Message> = None;

        // Attempt to send, with reconnection if needed
        let send_result = client
            .send_diagnostic_message(AddressType::Physical, request_bytes.clone())
            .await;

        if let Err(ref e) = send_result {
            if self.config.auto_reconnect && Self::is_connection_error(e) {
                warn!("Connection error during send, attempting reconnect: {}", e);
                let reconnect_timeout = self.config.reconnect_timeout;
                match Self::attempt_reconnect(&mut client, reconnect_timeout).await {
                    Ok(Some(msg)) => {
                        Self::send_tester_present_locked(&mut client, &self.last_activity).await;
                        pending_message = Some(msg);
                    }
                    Ok(None) => {
                        Self::send_tester_present_locked(&mut client, &self.last_activity).await;
                        reconnected_without_resend = true;
                    }
                    Err(e) => return Err(e),
                }
            } else {
                return Err(Error::Transport(send_result.unwrap_err()));
            }
        }

        // Wait for response using the response timeout (not reconnect timeout).
        let mut response_start = Instant::now();
        let response_timeout = self.config.response_timeout;

        loop {
            // Check if we've exceeded response timeout
            if response_start.elapsed() >= response_timeout {
                if reconnected_without_resend {
                    info!("No response after reconnection, re-sending request");
                    client
                        .send_diagnostic_message(AddressType::Physical, request_bytes.clone())
                        .await?;
                    reconnected_without_resend = false;
                    response_start = Instant::now();
                    continue;
                }
                return Err(Error::Timeout(response_timeout));
            }

            // Use pending message from reconnection if available, otherwise wait for one
            let message = if let Some(msg) = pending_message.take() {
                msg
            } else {
                // Use remaining response time or 1 second, whichever is smaller
                let remaining = response_timeout.saturating_sub(response_start.elapsed());
                let receive_timeout = remaining.min(Duration::from_secs(1));

                let receive_result = client.receive_diagnostic_response(receive_timeout).await;

                match receive_result {
                    Ok(msg) => msg,
                    Err(ref e) if self.config.auto_reconnect && Self::is_connection_error(e) => {
                        warn!(
                            "Connection error during receive, attempting reconnect: {}",
                            e,
                        );
                        let reconnect_timeout = self.config.reconnect_timeout;
                        match Self::attempt_reconnect(&mut client, reconnect_timeout).await {
                            Ok(Some(msg)) => {
                                Self::send_tester_present_locked(&mut client, &self.last_activity)
                                    .await;
                                info!("Using message received during reconnection");
                                pending_message = Some(msg);
                                response_start = Instant::now();
                                continue;
                            }
                            Ok(None) => {
                                Self::send_tester_present_locked(&mut client, &self.last_activity)
                                    .await;
                                info!("Reconnected — waiting for response on new connection");
                                reconnected_without_resend = true;
                                response_start = Instant::now();
                                continue;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(simple_doip::Error::ResponseTimeoutExceeded) => {
                        continue;
                    }
                    Err(e) => return Err(Error::Transport(e)),
                }
            };

            let response_bytes = Self::extract_diagnostic_payload(&message)?;
            debug!("Received raw UDS response: {:02X?}", response_bytes);

            // Validate response corresponds to our request
            Self::validate_response_matches_request(&request_bytes, &response_bytes)?;

            // Check for NRC 0x78 (Response Pending)
            if Self::is_response_pending(&response_bytes) {
                debug!(
                    "Received NRC 0x78 (Response Pending), resetting timeout and waiting for final response"
                );
                response_start = Instant::now();
                continue; // Wait for next response WITHOUT re-sending
            }

            // Final response
            return Ok(response_bytes);
        }
    }

    /// Request an extended diagnostic session.
    ///
    /// This is a convenience method for the common operation of entering
    /// the extended diagnostic session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session change fails.
    pub async fn enter_extended_session(&self) -> Result<ProtocolResponse> {
        info!("Requesting extended diagnostic session");
        let request = ProtocolRequest::diagnostic_session_control(
            false,
            DiagnosticSessionType::ExtendedDiagnosticSession,
        );
        // Safe to unwrap: suppress_positive_response is false
        self.send::<UdsSpec>(request, AddressType::Physical)
            .await
            .map(|opt| opt.expect("response expected for non-suppressed request"))
    }

    /// Request the default diagnostic session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session change fails.
    pub async fn enter_default_session(&self) -> Result<ProtocolResponse> {
        info!("Requesting default diagnostic session");
        let request = ProtocolRequest::diagnostic_session_control(
            false,
            DiagnosticSessionType::DefaultSession,
        );
        self.send::<UdsSpec>(request, AddressType::Physical)
            .await
            .map(|opt| opt.expect("response expected for non-suppressed request"))
    }

    /// Request a programming session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session change fails.
    pub async fn enter_programming_session(&self) -> Result<ProtocolResponse> {
        info!("Requesting programming session");
        let request = ProtocolRequest::diagnostic_session_control(
            false,
            DiagnosticSessionType::ProgrammingSession,
        );
        self.send::<UdsSpec>(request, AddressType::Physical)
            .await
            .map(|opt| opt.expect("response expected for non-suppressed request"))
    }

    /// Send a tester present message.
    ///
    /// # Errors
    ///
    /// Returns an error if the message fails.
    pub async fn tester_present(
        &self,
        suppress_response: bool,
    ) -> Result<Option<ProtocolResponse>> {
        debug!("Sending tester present");
        *self.last_activity.lock().unwrap() = Instant::now();
        let request = ProtocolRequest::tester_present(suppress_response);

        self.send::<UdsSpec>(request, AddressType::Physical).await
    }

    /// Shut down the client connection.
    pub async fn shutdown(mut self) {
        info!("Shutting down UDS client");
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
        // Clone the Arc before dropping self so we can unwrap it afterward.
        // Drop will be a no-op since keepalive_handle was already taken.
        let doip_client = Arc::clone(&self.doip_client);
        drop(self);
        let mutex = Arc::try_unwrap(doip_client)
            .ok()
            .expect("no other references after dropping UdsClient");
        let client = mutex.into_inner();
        client.shut_down().await;
    }

    /// Check if an error is a connection error that should trigger reconnection.
    fn is_connection_error(error: &simple_doip::Error) -> bool {
        matches!(
            error,
            simple_doip::Error::ConnectionClosed
                | simple_doip::Error::SocketClosedUnexpectedly
                | simple_doip::Error::SocketNotBound
                | simple_doip::Error::NetworkError(_)
        )
    }

    /// Attempt to reconnect to the server.
    ///
    /// Retries reconnection attempts until the configured reconnect timeout is reached.
    /// Note: simple_doip's reconnect() has an internal 5-second wait for in-flight messages,
    /// so each attempt may take up to ~15 seconds (with connection setup overhead).
    ///
    /// Returns Ok(Option<Message>) if reconnection succeeds. The Option contains any
    /// in-flight message that was received during reconnection (e.g., a response that
    /// arrived while we were reconnecting).
    async fn attempt_reconnect(
        client: &mut Client<Conn>,
        reconnect_timeout: Duration,
    ) -> Result<Option<Message>> {
        let start = Instant::now();
        let mut attempts = 0u32;

        while start.elapsed() < reconnect_timeout {
            attempts += 1;
            info!(
                "Reconnection attempt {} (elapsed: {:?})",
                attempts,
                start.elapsed()
            );

            // simple_doip's reconnect() does bind_socket + routing activation (~1-2s)
            // followed by a 5-second wait for in-flight messages. Give it 15 seconds
            // total to account for slow connections.
            match tokio::time::timeout(Duration::from_secs(15), client.reconnect()).await {
                Ok(Ok(maybe_message)) => {
                    info!("Reconnected successfully after {} attempts", attempts);
                    if maybe_message.is_some() {
                        info!("Received in-flight message during reconnection");
                    }
                    return Ok(maybe_message);
                }
                Ok(Err(e)) => {
                    warn!("Reconnection attempt {} failed: {}", attempts, e);
                }
                Err(_) => {
                    warn!("Reconnection attempt {} timed out", attempts);
                }
            }

            // Small delay before next attempt to avoid tight loop
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(Error::ReconnectionFailed {
            attempts,
            elapsed: start.elapsed(),
        })
    }

    /// Send a TesterPresent message to keep the ECU's S3 timer alive.
    ///
    /// Called after a successful reconnection while the doip_client lock is
    /// already held, so the keepalive task (which also needs the lock) cannot
    /// run. Without this, the ECU may revert to the default session during the
    /// reconnection window.
    async fn send_tester_present_locked(
        client: &mut Client<Conn>,
        last_activity: &std::sync::Mutex<Instant>,
    ) {
        match client
            .send_diagnostic_message(AddressType::Physical, vec![0x3E, 0x80])
            .await
        {
            Ok(()) => {
                debug!("Sent TesterPresent after reconnection");
                *last_activity.lock().unwrap() = Instant::now();
            }
            Err(e) => {
                warn!("TesterPresent after reconnection failed: {}", e);
            }
        }
    }

    /// Extract the diagnostic payload from a DoIP message.
    fn extract_diagnostic_payload(message: &Message) -> Result<Vec<u8>> {
        match &message.payload {
            Payload::DiagnosticMessage(diag) => Ok(diag.user_data.clone()),
            other => Err(Error::InvalidResponse(format!(
                "Expected DiagnosticMessage, got {:?}",
                other
            ))),
        }
    }

    /// Check if response bytes represent NRC 0x78 (Response Pending).
    ///
    /// UDS negative response format: [0x7F, service_id, nrc]
    /// NRC 0x78 = RequestCorrectlyReceivedResponsePending
    fn is_response_pending(response_bytes: &[u8]) -> bool {
        response_bytes.len() >= 3 && response_bytes[0] == 0x7F && response_bytes[2] == 0x78
    }

    /// Validate that a UDS response corresponds to the given request.
    ///
    /// UDS response correlation rules:
    /// - Positive response: SID = request_SID + 0x40
    /// - Negative response: [0x7F, request_SID, NRC]
    ///
    /// Returns Ok(()) if the response matches, or an error describing the mismatch.
    fn validate_response_matches_request(
        request_bytes: &[u8],
        response_bytes: &[u8],
    ) -> Result<()> {
        if request_bytes.is_empty() {
            return Err(Error::InvalidResponse("Empty request".to_string()));
        }
        if response_bytes.is_empty() {
            return Err(Error::InvalidResponse("Empty response".to_string()));
        }

        let request_sid = request_bytes[0];
        let response_sid = response_bytes[0];

        // Check for negative response (0x7F)
        if response_sid == 0x7F {
            if response_bytes.len() < 2 {
                return Err(Error::InvalidResponse(
                    "Negative response too short".to_string(),
                ));
            }
            let neg_response_for_sid = response_bytes[1];
            if neg_response_for_sid != request_sid {
                return Err(Error::InvalidResponse(format!(
                    "Negative response for SID {:#04x} but expected {:#04x}",
                    neg_response_for_sid, request_sid
                )));
            }
            return Ok(());
        }

        // Check for positive response (request SID + 0x40)
        let expected_positive_sid = request_sid.wrapping_add(0x40);
        if response_sid != expected_positive_sid {
            return Err(Error::InvalidResponse(format!(
                "Response SID {:#04x} doesn't match request SID {:#04x} (expected {:#04x} or 0x7F)",
                response_sid, request_sid, expected_positive_sid
            )));
        }

        Ok(())
    }
}

impl<Conn> Drop for UdsClient<Conn> {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
        }
    }
}
