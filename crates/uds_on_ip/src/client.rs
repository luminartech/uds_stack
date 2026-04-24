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

/// Classification of an incoming UDS message relative to the primary
/// request we are currently waiting for. See
/// [`UdsClient::classify_response`] for usage.
#[derive(Debug)]
enum ResponseMatch {
    /// The message is the (positive or negative) response to the
    /// primary request. Consume it normally.
    ForRequest,
    /// The message is a response to a different SID — almost always
    /// one of our own in-flight `TesterPresent` (0x3E) messages that
    /// the ECU chose to NACK. The caller should log and discard it
    /// and keep waiting for the real response.
    ForOtherRequest {
        /// The on-wire SID of the stray response (e.g. `0x7E` for a
        /// positive TesterPresent response, or the inner SID from a
        /// `0x7F` NACK). Used only for log triage.
        response_sid: u8,
        /// `true` if this was a NACK (0x7F), `false` for a positive
        /// response. Used only for log triage.
        nack: bool,
    },
    /// The message is malformed (empty, truncated NACK, etc.). The
    /// caller should surface this as [`Error::InvalidResponse`].
    Malformed(String),
}

/// Lock a poison-tolerant `Instant` mutex.
///
/// The activity timestamp is only ever written inside critical sections that
/// cannot panic (a scalar assignment or an `elapsed()` call), so poisoning
/// would indicate a panic elsewhere while the guard happened to be held.
/// In that case recovering the inner value and continuing is strictly
/// better than cascading panics through every subsequent UDS call — the
/// timestamp's precision is not load-bearing for correctness, only for
/// keep-alive cadence.
fn lock_activity(activity: &std::sync::Mutex<Instant>) -> std::sync::MutexGuard<'_, Instant> {
    activity
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

        let elapsed = lock_activity(&last_activity).elapsed();
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
                *lock_activity(&last_activity) = Instant::now();
            }
            Err(e) => {
                warn!("Keepalive TesterPresent failed: {}", e);
                // Don't update last_activity on failure so we retry sooner
            }
        }
    }
}

/// Should the background keepalive loop be spawned for this config?
///
/// A zero interval would make the loop spin, and an explicit disable skips
/// it entirely.
fn should_run_keepalive(config: &SessionConfig) -> bool {
    config.auto_tester_present && !config.tester_present_interval.is_zero()
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
        let keepalive_handle = if should_run_keepalive(&options.session_config) {
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
        let keepalive_handle = if should_run_keepalive(&config) {
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

        *lock_activity(&self.last_activity) = Instant::now();
        let mut client = self.doip_client.lock().await;

        // When `auto_reconnect` fires during a request's lifetime, the new
        // TCP socket has no record of the request — and any stray response
        // (typically a NACK for one of our own in-flight TesterPresent
        // messages) that arrives on the fresh stream would otherwise be
        // mis-correlated as a failed response to the primary request. So
        // after a successful reconnect with no buffered message we re-send
        // the request immediately rather than waiting for P2/P2* to expire.
        // ISO 14229 services the client issues here are idempotent enough
        // that a one-shot retry is safer than leaving the ECU without a
        // request in flight. See the `classify_response` path for the
        // matching defense against stray cross-SID responses.
        let mut pending_message: Option<Message> = None;
        let reconnect_timeout = self.config.reconnect_timeout;
        let max_pending = self.config.max_response_pending_count;
        // Matches the predicate used to decide whether to spawn the background
        // keepalive task. Threaded through so that post-reconnect TPs are
        // skipped for the same configs that skip keepalive spawning.
        let keepalive_active = should_run_keepalive(&self.config);
        let mut pending_count: u32 = 0;

        // Attempt to send, with reconnection if needed
        let send_result = client
            .send_diagnostic_message(address_type, request_bytes.clone())
            .await;

        if let Err(ref e) = send_result {
            if self.config.auto_reconnect && Self::is_connection_error(e) {
                warn!("Connection error during send, attempting reconnect: {}", e);
                match Self::reconnect_and_maybe_keepalive(
                    &mut client,
                    &self.last_activity,
                    reconnect_timeout,
                    keepalive_active,
                )
                .await?
                {
                    Some(msg) => pending_message = Some(msg),
                    None => {
                        // The request never reached the ECU (send failed before
                        // transmission). Callers that need session context (e.g.
                        // a programming sequence) must retry from the top rather
                        // than blindly re-sending into a potentially wrong session.
                        return Err(Error::ReconnectedWithoutResponse);
                    }
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

        // Wait for a response. Start with P2 (response_timeout); on the first
        // NRC 0x78 the effective timeout widens to P2* (response_pending_timeout)
        // for the rest of the request, per UDS ISO 14229 semantics. The timer
        // resets after each reconnection so reconnection time doesn't eat into
        // the response wait.
        let mut response_start = Instant::now();
        let mut current_timeout = self.config.response_timeout;

        loop {
            // Check if we've exceeded the effective timeout (P2 or P2*).
            if response_start.elapsed() >= current_timeout {
                return Err(Error::Timeout(current_timeout));
            }

            // Use pending message from reconnection if available, otherwise wait for one
            let message = if let Some(msg) = pending_message.take() {
                msg
            } else {
                // Emit keepalive TesterPresent if the interval has elapsed since the last
                // outgoing message. The background keepalive task contends for the same
                // doip_client lock we hold, so during long NRC 0x78 waits it is starved;
                // we fire it ourselves here while the lock is already in hand. A
                // connection error from the TP send is a proactive signal that the
                // socket is dead even though the receive side may still yield buffered
                // data — escalate to reconnect instead of spinning on failed sends.
                if self.config.auto_tester_present {
                    match Self::maybe_send_tester_present_while_lock_held(
                        &mut client,
                        &self.last_activity,
                        self.config.tester_present_interval,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(e) if self.config.auto_reconnect && Self::is_connection_error(&e) => {
                            warn!("TP send detected dead connection, reconnecting: {}", e);
                            match Self::reconnect_and_maybe_keepalive(
                                &mut client,
                                &self.last_activity,
                                reconnect_timeout,
                                keepalive_active,
                            )
                            .await?
                            {
                                Some(msg) => {
                                    info!("Using message received during reconnection");
                                    pending_message = Some(msg);
                                }
                                None => {
                                    info!("Reconnected — re-sending request on new connection");
                                    client
                                        .send_diagnostic_message(
                                            address_type,
                                            request_bytes.clone(),
                                        )
                                        .await?;
                                    *lock_activity(&self.last_activity) = Instant::now();
                                }
                            }
                            response_start = Instant::now();
                            continue;
                        }
                        Err(_) => {
                            // Non-connection error already logged at DEBUG inside the
                            // helper; the subsequent receive will surface it if fatal.
                        }
                    }
                }

                // Cap the receive timeout so we wake at least once per keepalive
                // interval; otherwise a long interval could delay response-timeout
                // checks, and a short one could miss its TP deadline. Only apply
                // the interval cap when keepalives are on and the interval is
                // non-zero — callers with keepalives off shouldn't get extra
                // wakeups, and a zero interval would otherwise spin this loop.
                let remaining = current_timeout.saturating_sub(response_start.elapsed());
                let mut receive_timeout = remaining.min(Duration::from_secs(1));
                if self.config.auto_tester_present && !self.config.tester_present_interval.is_zero()
                {
                    receive_timeout = receive_timeout.min(self.config.tester_present_interval);
                }

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
                        match Self::reconnect_and_maybe_keepalive(
                            &mut client,
                            &self.last_activity,
                            reconnect_timeout,
                            keepalive_active,
                        )
                        .await?
                        {
                            Some(msg) => {
                                info!("Using message received during reconnection");
                                pending_message = Some(msg);
                            }
                            None => {
                                info!("Reconnected — re-sending request on new connection");
                                client
                                    .send_diagnostic_message(address_type, request_bytes.clone())
                                    .await?;
                                *lock_activity(&self.last_activity) = Instant::now();
                            }
                        }
                        response_start = Instant::now();
                        continue;
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

            // Correlate response against the in-flight request. Skip
            // stray responses to other SIDs (typically NACKs for our
            // own background TesterPresent) instead of treating them
            // as failures of this request.
            match Self::classify_response(&request_bytes, &response_bytes) {
                ResponseMatch::ForRequest => {}
                ResponseMatch::ForOtherRequest { response_sid, nack } => {
                    debug!(
                        "Ignoring stray {} for SID {:#04x} while waiting on SID {:#04x}",
                        if nack { "NACK" } else { "positive response" },
                        response_sid,
                        request_bytes[0],
                    );
                    continue;
                }
                ResponseMatch::Malformed(reason) => {
                    return Err(Error::InvalidResponse(reason));
                }
            }

            // Check for NRC 0x78 (Response Pending)
            if Self::is_response_pending(&response_bytes) {
                debug!(
                    "Received NRC 0x78 (Response Pending), switching to P2* and waiting for final response"
                );
                if let Some(max) = max_pending {
                    pending_count += 1;
                    if pending_count > max {
                        warn!(
                            "Exceeded NRC 0x78 cap ({max}); server appears stuck, aborting request"
                        );
                        return Err(Error::Nrc78PendingExceeded { max });
                    }
                }
                response_start = Instant::now();
                // Per ISO 14229, each NRC 0x78 extends the wait by P2* rather
                // than P2. Widen the effective timeout for the remainder of
                // this request (it's a no-op if already at P2*).
                current_timeout = self.config.response_pending_timeout;
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

        *lock_activity(&self.last_activity) = Instant::now();
        let mut client = self.doip_client.lock().await;

        // See send() for the full rationale on the immediate
        // re-send-after-reconnect posture and the cross-SID response
        // handling below.
        let mut pending_message: Option<Message> = None;
        let reconnect_timeout = self.config.reconnect_timeout;
        let max_pending = self.config.max_response_pending_count;
        // Matches the keepalive-spawn predicate; see send().
        let keepalive_active = should_run_keepalive(&self.config);
        let mut pending_count: u32 = 0;

        // Attempt to send, with reconnection if needed
        let send_result = client
            .send_diagnostic_message(AddressType::Physical, request_bytes.clone())
            .await;

        if let Err(ref e) = send_result {
            if self.config.auto_reconnect && Self::is_connection_error(e) {
                warn!("Connection error during send, attempting reconnect: {}", e);
                match Self::reconnect_and_maybe_keepalive(
                    &mut client,
                    &self.last_activity,
                    reconnect_timeout,
                    keepalive_active,
                )
                .await?
                {
                    Some(msg) => pending_message = Some(msg),
                    None => {
                        return Err(Error::ReconnectedWithoutResponse);
                    }
                }
            } else {
                return Err(Error::Transport(send_result.unwrap_err()));
            }
        }

        // See send() for rationale: start at P2 (response_timeout), widen to
        // P2* (response_pending_timeout) after the first NRC 0x78.
        let mut response_start = Instant::now();
        let mut current_timeout = self.config.response_timeout;

        loop {
            // Check if we've exceeded the effective timeout (P2 or P2*).
            if response_start.elapsed() >= current_timeout {
                return Err(Error::Timeout(current_timeout));
            }

            // Use pending message from reconnection if available, otherwise wait for one
            let message = if let Some(msg) = pending_message.take() {
                msg
            } else {
                // See send() for rationale: the background keepalive task is
                // blocked on the doip_client lock we hold, so we emit TesterPresent
                // ourselves when the interval has elapsed since the last outgoing
                // message. A connection-error from the TP send is a proactive dead
                // connection signal — escalate to reconnect rather than spinning.
                if self.config.auto_tester_present {
                    match Self::maybe_send_tester_present_while_lock_held(
                        &mut client,
                        &self.last_activity,
                        self.config.tester_present_interval,
                    )
                    .await
                    {
                        Ok(()) => {}
                        Err(e) if self.config.auto_reconnect && Self::is_connection_error(&e) => {
                            warn!("TP send detected dead connection, reconnecting: {}", e);
                            match Self::reconnect_and_maybe_keepalive(
                                &mut client,
                                &self.last_activity,
                                reconnect_timeout,
                                keepalive_active,
                            )
                            .await?
                            {
                                Some(msg) => {
                                    info!("Using message received during reconnection");
                                    pending_message = Some(msg);
                                }
                                None => {
                                    info!("Reconnected — re-sending request on new connection");
                                    client
                                        .send_diagnostic_message(
                                            AddressType::Physical,
                                            request_bytes.clone(),
                                        )
                                        .await?;
                                    *lock_activity(&self.last_activity) = Instant::now();
                                }
                            }
                            response_start = Instant::now();
                            continue;
                        }
                        Err(_) => {}
                    }
                }

                // Same gating as send(): only cap receive_timeout by the keepalive
                // interval when keepalives are on and the interval is non-zero.
                let remaining = current_timeout.saturating_sub(response_start.elapsed());
                let mut receive_timeout = remaining.min(Duration::from_secs(1));
                if self.config.auto_tester_present && !self.config.tester_present_interval.is_zero()
                {
                    receive_timeout = receive_timeout.min(self.config.tester_present_interval);
                }

                let receive_result = client.receive_diagnostic_response(receive_timeout).await;

                match receive_result {
                    Ok(msg) => msg,
                    Err(ref e) if self.config.auto_reconnect && Self::is_connection_error(e) => {
                        warn!(
                            "Connection error during receive, attempting reconnect: {}",
                            e,
                        );
                        match Self::reconnect_and_maybe_keepalive(
                            &mut client,
                            &self.last_activity,
                            reconnect_timeout,
                            keepalive_active,
                        )
                        .await?
                        {
                            Some(msg) => {
                                info!("Using message received during reconnection");
                                pending_message = Some(msg);
                            }
                            None => {
                                info!("Reconnected — re-sending request on new connection");
                                client
                                    .send_diagnostic_message(
                                        AddressType::Physical,
                                        request_bytes.clone(),
                                    )
                                    .await?;
                                *lock_activity(&self.last_activity) = Instant::now();
                            }
                        }
                        response_start = Instant::now();
                        continue;
                    }
                    Err(simple_doip::Error::ResponseTimeoutExceeded) => {
                        continue;
                    }
                    Err(e) => return Err(Error::Transport(e)),
                }
            };

            let response_bytes = Self::extract_diagnostic_payload(&message)?;
            debug!("Received raw UDS response: {:02X?}", response_bytes);

            // See the matching block in `send_and_receive` for the full
            // rationale: skip stray responses (typically our own
            // background-TP NACKs) rather than failing this request.
            match Self::classify_response(&request_bytes, &response_bytes) {
                ResponseMatch::ForRequest => {}
                ResponseMatch::ForOtherRequest { response_sid, nack } => {
                    debug!(
                        "Ignoring stray {} for SID {:#04x} while waiting on SID {:#04x}",
                        if nack { "NACK" } else { "positive response" },
                        response_sid,
                        request_bytes[0],
                    );
                    continue;
                }
                ResponseMatch::Malformed(reason) => {
                    return Err(Error::InvalidResponse(reason));
                }
            }

            // Check for NRC 0x78 (Response Pending)
            if Self::is_response_pending(&response_bytes) {
                debug!(
                    "Received NRC 0x78 (Response Pending), switching to P2* and waiting for final response"
                );
                if let Some(max) = max_pending {
                    pending_count += 1;
                    if pending_count > max {
                        warn!(
                            "Exceeded NRC 0x78 cap ({max}); server appears stuck, aborting request"
                        );
                        return Err(Error::Nrc78PendingExceeded { max });
                    }
                }
                response_start = Instant::now();
                // Per ISO 14229, widen the effective timeout to P2* for the
                // remainder of this request (no-op once already at P2*).
                current_timeout = self.config.response_pending_timeout;
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
        *lock_activity(&self.last_activity) = Instant::now();
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
        /// Cap each individual `client.reconnect()` call — simple_doip's
        /// reconnect() does bind_socket + routing activation (~1-2s) followed
        /// by a 5-second wait for in-flight messages, which bounds a healthy
        /// attempt at roughly this value. Any single attempt taking longer is
        /// almost certainly never returning.
        const MAX_PER_ATTEMPT: Duration = Duration::from_secs(15);

        let start = Instant::now();
        let mut attempts = 0u32;

        while start.elapsed() < reconnect_timeout {
            attempts += 1;
            info!(
                "Reconnection attempt {} (elapsed: {:?})",
                attempts,
                start.elapsed()
            );

            // Respect the outer budget: the per-attempt cap must not stretch
            // past `reconnect_timeout`, otherwise a configured 5-second
            // reconnect budget could be ignored for up to 15s.
            let remaining = reconnect_timeout.saturating_sub(start.elapsed());
            let per_attempt = remaining.min(MAX_PER_ATTEMPT);
            if per_attempt.is_zero() {
                break;
            }
            match tokio::time::timeout(per_attempt, client.reconnect()).await {
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

    /// Send a TesterPresent while the caller already holds the `doip_client`
    /// lock. Used after a successful reconnection and from the in-request
    /// keep-alive path, where the background keepalive task is blocked on the
    /// same mutex and cannot run.
    ///
    /// Returns the underlying send error on failure so the caller can decide
    /// whether to escalate (e.g. treat a connection error as a trigger for
    /// reconnection). On success (and on failure) the error/success is logged
    /// at DEBUG — callers that want a louder signal should log it themselves
    /// with the context they have.
    async fn send_tester_present_while_lock_held(
        client: &mut Client<Conn>,
        last_activity: &std::sync::Mutex<Instant>,
    ) -> std::result::Result<(), simple_doip::Error> {
        match client
            .send_diagnostic_message(AddressType::Physical, vec![0x3E, 0x80])
            .await
        {
            Ok(()) => {
                debug!("Sent TesterPresent while lock held");
                *lock_activity(last_activity) = Instant::now();
                Ok(())
            }
            Err(e) => {
                debug!("TesterPresent while lock held failed: {}", e);
                Err(e)
            }
        }
    }

    /// Emit a TesterPresent if the configured interval has elapsed since the
    /// last outgoing message. Intended for callers that already hold the
    /// `doip_client` lock across a long wait (e.g. NRC 0x78 response-pending
    /// loops), during which the background keepalive task is blocked.
    ///
    /// A zero interval is treated as "disabled" to avoid turning the receive
    /// loop into a busy-spin. On a connection error the underlying error is
    /// returned so callers can escalate to reconnect; otherwise `Ok(())`.
    async fn maybe_send_tester_present_while_lock_held(
        client: &mut Client<Conn>,
        last_activity: &std::sync::Mutex<Instant>,
        interval: Duration,
    ) -> std::result::Result<(), simple_doip::Error> {
        if interval.is_zero() {
            return Ok(());
        }
        let elapsed = lock_activity(last_activity).elapsed();
        if elapsed < interval {
            return Ok(());
        }
        Self::send_tester_present_while_lock_held(client, last_activity).await
    }

    /// Drive a reconnection and, when keepalives are enabled, immediately
    /// emit a TP on the new connection. Returns any in-flight message
    /// surfaced by the reconnect. `send_tp_after_reconnect` must match the
    /// same predicate used for spawning the background keepalive task
    /// (see `should_run_keepalive`) so `auto_tester_present = false` and
    /// `tester_present_interval = Duration::ZERO` both skip the post-
    /// reconnect TP, keeping config semantics consistent. The post-
    /// reconnect TP failure (if any) is intentionally swallowed: the next
    /// send/receive will surface a still-broken link through the normal
    /// error paths.
    async fn reconnect_and_maybe_keepalive(
        client: &mut Client<Conn>,
        last_activity: &std::sync::Mutex<Instant>,
        reconnect_timeout: Duration,
        send_tp_after_reconnect: bool,
    ) -> Result<Option<Message>> {
        let maybe_msg = Self::attempt_reconnect(client, reconnect_timeout).await?;
        if send_tp_after_reconnect {
            let _ = Self::send_tester_present_while_lock_held(client, last_activity).await;
        }
        Ok(maybe_msg)
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

    /// Classify a UDS response against the in-flight request.
    ///
    /// UDS response correlation rules:
    /// - Positive response: SID = request_SID + 0x40
    /// - Negative response: [0x7F, request_SID, NRC]
    ///
    /// Background `TesterPresent` (0x3E 0x80) traffic can arrive on the same
    /// TCP stream while we are waiting for the primary request's response
    /// — both the background keepalive task and the in-lock
    /// `maybe_send_tester_present_while_lock_held` path can emit a TP during
    /// a long wait. The ECU normally suppresses the positive TP response
    /// (sub-function 0x80) but it is still allowed to NACK the TP, and
    /// that NACK arrives on the same stream. Treating such a NACK as a
    /// failed primary response (the pre-fix behavior) produces spurious
    /// errors like "Negative response for SID 0x3e but expected 0x31"
    /// that abort the overall operation.
    ///
    /// Returns a [`ResponseMatch`] the caller can use to decide whether to
    /// consume the message, skip it, or error out.
    fn classify_response(request_bytes: &[u8], response_bytes: &[u8]) -> ResponseMatch {
        if request_bytes.is_empty() {
            return ResponseMatch::Malformed("Empty request".to_string());
        }
        if response_bytes.is_empty() {
            return ResponseMatch::Malformed("Empty response".to_string());
        }

        let request_sid = request_bytes[0];
        let response_sid = response_bytes[0];

        // Negative response (0x7F). A well-formed NACK is
        // `[0x7F, request_sid, NRC]` — three bytes minimum. A two-byte
        // `[0x7F, sid]` is missing the NRC and must be rejected as
        // malformed, not classified by inspecting `response_bytes[1]`
        // alone.
        if response_sid == 0x7F {
            if response_bytes.len() < 3 {
                return ResponseMatch::Malformed("Negative response too short".to_string());
            }
            let neg_response_for_sid = response_bytes[1];
            if neg_response_for_sid == request_sid {
                return ResponseMatch::ForRequest;
            }
            // NACK for some other SID — almost always one of our own
            // in-flight TesterPresent messages. Tell the caller to drop
            // it and keep waiting for the real response.
            return ResponseMatch::ForOtherRequest {
                response_sid: neg_response_for_sid,
                nack: true,
            };
        }

        // Positive response (request SID + 0x40).
        let expected_positive_sid = request_sid.wrapping_add(0x40);
        if response_sid == expected_positive_sid {
            return ResponseMatch::ForRequest;
        }

        // Positive response for a different SID. The only path that
        // legitimately emits such a response on our stream is an
        // in-flight TP without the suppress-positive bit set (i.e. a
        // TP response at 0x7E). Background keepalive and in-lock TPs
        // both use sub-function 0x80, so this should be vanishingly
        // rare in practice — but it is still not a reason to fail the
        // primary request.
        ResponseMatch::ForOtherRequest {
            response_sid,
            nack: false,
        }
    }
}

impl<Conn> Drop for UdsClient<Conn> {
    fn drop(&mut self) {
        if let Some(handle) = self.keepalive_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod classify_response_tests {
    use super::*;

    // Use a concrete client type just so the associated fn is reachable.
    type TestClient = UdsClient<ConnectorSocket>;

    #[test]
    fn positive_response_for_request_matches() {
        // Request: DiagnosticSessionControl (0x10), sub-function 0x03.
        // Response: 0x50 (0x10 + 0x40), sub-function 0x03.
        let m = TestClient::classify_response(&[0x10, 0x03], &[0x50, 0x03]);
        assert!(matches!(m, ResponseMatch::ForRequest), "got {m:?}");
    }

    #[test]
    fn negative_response_for_request_matches() {
        // NACK for RoutineControl (0x31), NRC 0x22 (conditionsNotCorrect).
        let m = TestClient::classify_response(&[0x31, 0x01, 0x02, 0x03], &[0x7F, 0x31, 0x22]);
        assert!(matches!(m, ResponseMatch::ForRequest), "got {m:?}");
    }

    #[test]
    fn nack_for_tester_present_while_waiting_on_routine_control_is_stray() {
        // Primary request: RoutineControl (0x31). Server NACKs a
        // background TesterPresent (0x3E) on the same stream — this is
        // the exact shape of the [INTERNAL_PROJECT_REDACTED] failure we're fixing.
        let m = TestClient::classify_response(&[0x31, 0x01, 0x02, 0x03], &[0x7F, 0x3E, 0x22]);
        match m {
            ResponseMatch::ForOtherRequest { response_sid, nack } => {
                assert_eq!(response_sid, 0x3E);
                assert!(nack, "0x7F response must be classified as NACK");
            }
            other => panic!("expected ForOtherRequest, got {other:?}"),
        }
    }

    #[test]
    fn positive_response_for_other_sid_is_stray() {
        // Primary request: RoutineControl (0x31, expected response 0x71).
        // Stream carries a positive TesterPresent response (0x7E) —
        // vanishingly rare because SPRMIB is always set, but not a
        // reason to fail the primary request.
        let m = TestClient::classify_response(&[0x31, 0x01, 0x02, 0x03], &[0x7E]);
        match m {
            ResponseMatch::ForOtherRequest { response_sid, nack } => {
                assert_eq!(response_sid, 0x7E);
                assert!(
                    !nack,
                    "0x7E positive response must not be classified as NACK"
                );
            }
            other => panic!("expected ForOtherRequest, got {other:?}"),
        }
    }

    #[test]
    fn empty_request_is_malformed() {
        let m = TestClient::classify_response(&[], &[0x50]);
        assert!(matches!(m, ResponseMatch::Malformed(_)), "got {m:?}");
    }

    #[test]
    fn empty_response_is_malformed() {
        let m = TestClient::classify_response(&[0x10, 0x03], &[]);
        assert!(matches!(m, ResponseMatch::Malformed(_)), "got {m:?}");
    }

    #[test]
    fn truncated_nack_one_byte_is_malformed() {
        // NACK must be at least [0x7F, request_sid, NRC].
        let m = TestClient::classify_response(&[0x10, 0x03], &[0x7F]);
        assert!(matches!(m, ResponseMatch::Malformed(_)), "got {m:?}");
    }

    #[test]
    fn truncated_nack_two_bytes_is_malformed() {
        // A two-byte NACK [0x7F, <sid>] has no NRC byte and must be
        // rejected — otherwise classifying by `response_bytes[1]` alone
        // would misclassify this as either `ForRequest` (when the
        // second byte happens to match the request SID) or
        // `ForOtherRequest` (when it doesn't), instead of surfacing the
        // truncation. Lock this behavior in.
        let m_matching_sid = TestClient::classify_response(&[0x10, 0x03], &[0x7F, 0x10]);
        assert!(
            matches!(m_matching_sid, ResponseMatch::Malformed(_)),
            "two-byte NACK whose inner byte matches the request SID must be Malformed, got {m_matching_sid:?}",
        );

        let m_other_sid = TestClient::classify_response(&[0x10, 0x03], &[0x7F, 0x3E]);
        assert!(
            matches!(m_other_sid, ResponseMatch::Malformed(_)),
            "two-byte NACK whose inner byte is some other SID must be Malformed, got {m_other_sid:?}",
        );
    }
}
