//! Trait abstraction for sending UDS requests, allowing command processing
//! to be decoupled from the concrete connection type.

use async_trait::async_trait;
use simple_doip::{client::AddressType, connection::Connector};
use uds_protocol::{ProtocolRequest, ProtocolResponse, UdsSpec};

use crate::{Result, UdsClient};

/// A type-erased interface for sending UDS requests over DoIP.
///
/// This trait allows command processing code to send requests without knowing
/// the concrete connection type (`ConnectorSocket` vs `ListenerSocket`).
#[async_trait]
pub trait RequestSender: Send + Sync {
    /// Send a typed UDS request and wait for the response.
    ///
    /// `address_type` controls the DoIP target address:
    /// - `Physical` — targets a specific ECU (unicast)
    /// - `Logical` — targets the functional group address (broadcast)
    ///
    /// Returns `Ok(None)` when the request has the suppress-positive-response bit set.
    async fn send(
        &self,
        request: ProtocolRequest,
        address_type: AddressType,
    ) -> Result<Option<ProtocolResponse>>;

    /// Send a raw UDS request (as bytes) and wait for the raw response bytes.
    async fn send_raw(&self, data: Vec<u8>) -> Result<Vec<u8>>;

    /// Shut down the underlying connection.
    async fn shutdown(self: Box<Self>);
}

#[async_trait]
impl<Conn> RequestSender for UdsClient<Conn>
where
    Conn: Connector + Send + Sync + 'static,
{
    async fn send(
        &self,
        request: ProtocolRequest,
        address_type: AddressType,
    ) -> Result<Option<ProtocolResponse>> {
        self.send::<UdsSpec>(request, address_type).await
    }

    async fn send_raw(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        UdsClient::send_raw(self, data).await
    }

    async fn shutdown(self: Box<Self>) {
        UdsClient::shutdown(*self).await
    }
}
