//! UDS client for sending diagnostic requests over DoIP.

use std::net::{IpAddr, SocketAddr};

use simple_doip::{
    client::{AddressType, Client, ClientOptions, RoutingActivationOptions, SendResult},
    connection::ConnectorSocket,
    messages::{ActivationTypeCode, Message, Payload, ProtocolVersion},
    LogicalAddress, TCP_PORT, TESTER_LOGICAL_ADDRESS,
};
use tokio::sync::Mutex;
use tracing::{debug, info};
use uds_protocol::{
    DiagnosticDefinition, DiagnosticSessionType, ProtocolRequest, ProtocolResponse, Request,
    Response, SingleValueWireFormat, UdsSpec, WireFormat,
};

use crate::{Error, Result, SessionConfig};

/// A UDS client that manages diagnostic sessions over DoIP.
///
/// This client wraps the low-level DoIP client and provides a higher-level
/// interface for sending UDS requests and receiving responses.
pub struct UdsClient {
    /// The underlying DoIP client.
    doip_client: Mutex<Client<ConnectorSocket>>,
    /// Session configuration (used for automatic tester present and timeout handling).
    #[allow(dead_code)] // Will be used when auto tester present is implemented
    config: SessionConfig,
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
}

impl UdsClient {
    /// Connect to a UDS server over DoIP.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or routing activation fails.
    pub async fn connect(options: UdsClientOptions) -> Result<Self> {
        let doip_options = ClientOptions {
            server_address: SocketAddr::new(options.server_ip, options.server_port),
            server_logical_address: options.server_logical_address,
            server_physical_address: options.server_physical_address,
            client_address: IpAddr::from([0, 0, 0, 0]),
            client_logical_address: options.client_logical_address,
            protocol_version: options.protocol_version,
            routing_activation_options: if options.routing_activation {
                Some(RoutingActivationOptions {
                    activation_type: ActivationTypeCode::Default,
                    oem_specific: None,
                })
            } else {
                None
            },
            tester_present_interval: options.session_config.tester_present_interval,
            suppress_tester_present: true,
        };

        info!(
            "Connecting to UDS server at {}:{}",
            options.server_ip, options.server_port
        );

        let client = Client::<ConnectorSocket>::connect(doip_options).await?;

        info!("Connected to UDS server");

        Ok(Self {
            doip_client: Mutex::new(client),
            config: options.session_config,
        })
    }

    /// Send a UDS request and wait for the response.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send<D: DiagnosticDefinition>(
        &self,
        request: Request<D>,
    ) -> Result<Response<D>>
    where
        D::DID: SingleValueWireFormat,
        D::DiagnosticPayload: SingleValueWireFormat,
        D::RID: SingleValueWireFormat,
        D::RoutinePayload: SingleValueWireFormat,
    {
        // Encode the request to bytes
        let mut request_bytes = Vec::with_capacity(request.required_size());
        request
            .encode(&mut request_bytes)
            .map_err(|e| Error::InvalidResponse(format!("Failed to encode request: {e}")))?;

        debug!("Sending UDS request: {:02X?}", request_bytes);

        // Send via DoIP
        let mut client = self.doip_client.lock().await;
        let result = client
            .send_diagnostic_message(AddressType::Physical, request_bytes)
            .await?;

        match result {
            SendResult::Response(message) => {
                let response_bytes = Self::extract_diagnostic_payload(&message)?;
                debug!("Received UDS response: {:02X?}", response_bytes);

                // Decode the response
                let response = Response::<D>::decode(&mut response_bytes.as_slice())
                    .map_err(|e| Error::InvalidResponse(format!("Failed to decode response: {e}")))?;

                Ok(response)
            }
            SendResult::Suppressed => {
                Err(Error::InvalidResponse("Response was suppressed".to_string()))
            }
        }
    }

    /// Send a raw UDS request (as bytes) and wait for the response.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn send_raw(&self, request_bytes: Vec<u8>) -> Result<Vec<u8>> {
        debug!("Sending raw UDS request: {:02X?}", request_bytes);

        let mut client = self.doip_client.lock().await;
        let result = client
            .send_diagnostic_message(AddressType::Physical, request_bytes)
            .await?;

        match result {
            SendResult::Response(message) => {
                let response_bytes = Self::extract_diagnostic_payload(&message)?;
                debug!("Received raw UDS response: {:02X?}", response_bytes);
                Ok(response_bytes)
            }
            SendResult::Suppressed => {
                Err(Error::InvalidResponse("Response was suppressed".to_string()))
            }
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
        self.send::<UdsSpec>(request).await
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
        self.send::<UdsSpec>(request).await
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
        self.send::<UdsSpec>(request).await
    }

    /// Send a tester present message.
    ///
    /// # Errors
    ///
    /// Returns an error if the message fails.
    pub async fn tester_present(&self, suppress_response: bool) -> Result<Option<ProtocolResponse>> {
        debug!("Sending tester present");
        let request = ProtocolRequest::tester_present(suppress_response);

        if suppress_response {
            // Send raw bytes and don't wait for response
            let mut request_bytes = Vec::with_capacity(request.required_size());
            request
                .encode(&mut request_bytes)
                .map_err(|e| Error::InvalidResponse(format!("Failed to encode request: {e}")))?;

            let mut client = self.doip_client.lock().await;
            let _ = client
                .send_diagnostic_message(AddressType::Physical, request_bytes)
                .await?;
            Ok(None)
        } else {
            self.send::<UdsSpec>(request).await.map(Some)
        }
    }

    /// Shut down the client connection.
    pub async fn shutdown(self) {
        info!("Shutting down UDS client");
        let client = self.doip_client.into_inner();
        client.shut_down().await;
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
}
