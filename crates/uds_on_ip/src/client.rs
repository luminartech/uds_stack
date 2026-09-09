//! Asynchronous `UDSonIP` client.
//!
//! Drives a [`SessionLayer`] over a `simple_doip` connection, owning the clock,
//! the timers, and the sockets so the session layer does not have to.
//!
//! # Alloc-free by construction
//!
//! Responses borrow from a receive buffer the caller supplies at construction;
//! nothing here allocates, and no public type contains a `Vec` or a `String`.
//! The first driver happens to sit on `tokio` and std sockets, but the *shape*
//! of this API is the one a bare-metal driver will also present, so gaining a
//! `no_std` driver later is additive rather than a breaking redesign.
//!
//! # Why physical and functional addressing have different shapes
//!
//! ISO 14229-5:2022 Figure 9 shows a physically addressed request answered by
//! one server: `tP_Client` stops when the response arrives.
//!
//! Figure 8 shows a functionally addressed request forwarded by the gateway to
//! every server on the subnet, each of which may answer. `tP_Client` *reloads*
//! on each response, and its **expiry is the signal that no more are coming** —
//! not an error. A functional request therefore yields a sequence of responses
//! of unknown length, and cannot honestly be typed as a single `Result`.
//!
//! # Why that sequence is not a `Stream`
//!
//! Each response borrows the receive buffer, so the sequence is *lending*:
//! `next()` hands out a view valid only until the following call.
//! `futures::Stream` requires `Item` to be owned and independent of the stream,
//! so implementing it would force a copy per response — an allocation on every
//! response, exactly what this API is shaped to avoid. [`Responses`] is
//! therefore an inherent-method sequence rather than a `Stream`.

use crate::addressing::{Ai, ChannelId, LogicalAddress};
use crate::error::Result;
use crate::profile::Timing;
use crate::session::SessionLayer;

/// One response, borrowed from the client's receive buffer.
///
/// Valid until the next call that reuses the buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Response<'a> {
    /// The responding entity. For a functional request this differs between
    /// responses and is the only way to tell them apart.
    pub source: LogicalAddress,
    /// The UDS response bytes, undecoded.
    ///
    /// This crate does not decode UDS messages — that is `uds_protocol`'s job,
    /// and interpreting them is `uds_services`'. Handing back bytes is what
    /// keeps the layering honest, and what keeps this type alloc-free.
    pub data: &'a [u8],
}

/// One periodic response (`DoIP` payload `0x8004`), borrowed from the receive
/// buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeriodicResponse<'a> {
    /// The responding entity.
    pub source: LogicalAddress,
    /// The periodic data identifier.
    pub pdid: u8,
    /// The periodic data record.
    pub data: &'a [u8],
}

/// How to reach the peer, and how the session should be timed.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// The `DoIP` entity's address and connection parameters.
    pub doip: simple_doip::client::ClientOptions,
    /// Timing parameters supplied to the session layer.
    pub timing: Timing,
}

/// An asynchronous `UDSonIP` client.
///
/// Generic over the session layer so `uds_session` can replace the interim
/// implementation without a change to this type's API. `'buf` is the receive
/// buffer the caller supplies; responses borrow from it.
#[derive(Debug)]
pub struct Client<'buf, S: SessionLayer> {
    _session: S,
    _rx: &'buf mut [u8],
}

impl<'buf, S: SessionLayer> Client<'buf, S> {
    /// Connect, perform `DoIP` routing activation, and take ownership of the
    /// receive buffer responses will borrow from.
    ///
    /// Sizing `rx` is a deployment decision, exactly as it is for the session
    /// layer's channel storage, so this crate does not choose it.
    ///
    /// # Errors
    /// If the connection or routing activation fails.
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn connect(
        session: S,
        options: ClientOptions,
        rx: &'buf mut [u8],
    ) -> Result<Self, S::Error> {
        todo!("connect, activate routing, open the default channels")
    }

    /// Send a physically addressed request and await its response.
    ///
    /// # Errors
    /// [`crate::Error::ResponseTimeout`] if `tP_Client` expires,
    /// [`crate::Error::BufferTooSmall`] if the response will not fit, or a
    /// transport error. A UDS negative response is *not* an error: it is a
    /// response, and interpreting it belongs to a higher layer.
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn send(
        &mut self,
        ta: LogicalAddress,
        request: &[u8],
    ) -> Result<Response<'_>, S::Error> {
        todo!("S_Data.req, await T_Data.conf from the ACK, then T_Data.ind")
    }

    /// Send a functionally addressed request and iterate the responses.
    ///
    /// # Errors
    /// If the request cannot be submitted or transmitted.
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn send_functional(
        &mut self,
        ta: LogicalAddress,
        request: &[u8],
    ) -> Result<Responses<'_, 'buf, S>, S::Error> {
        todo!("S_Data.req on the functional channel; reload tP_Client per response")
    }

    /// Await the next periodic response (`DoIP` `0x8004`).
    ///
    /// These bypass request/response correlation and must not reset
    /// `tS3_Server` (ISO 14229-5:2022 REQ 7.16, REQ 7.20).
    ///
    /// Returns `None` once the connection closes.
    #[allow(clippy::unused_async)]
    pub async fn next_periodic(&mut self) -> Option<PeriodicResponse<'_>> {
        todo!("surface 0x8004; blocked on the simple_doip gap in crate::mapping")
    }

    /// Open a channel for an addressing triple that is not one of the defaults.
    ///
    /// # Errors
    /// If the session layer has no channel storage left.
    #[allow(unused_variables)]
    pub fn open_channel(&mut self, ai: Ai) -> Result<ChannelId, S::Error> {
        todo!("supply the tP6 pair as this channel's reload parameters")
    }

    /// Close the connection.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(self) {
        todo!("drain in-flight work, then close")
    }
}

/// The responses to one functionally addressed request.
///
/// A *lending* sequence: each [`Responses::next`] returns a view borrowed from
/// the client's receive buffer, valid only until the following call. See the
/// module documentation for why this is not a `futures::Stream`.
#[derive(Debug)]
pub struct Responses<'c, 'buf, S: SessionLayer> {
    _client: &'c mut Client<'buf, S>,
}

impl<S: SessionLayer> Responses<'_, '_, S> {
    /// Await the next response, or `None` once `tP_Client` expires.
    ///
    /// Expiry ends the sequence normally rather than producing an error: it is
    /// the defined end-of-responses signal for a functionally addressed request
    /// (ISO 14229-5:2022 Figure 8).
    #[allow(clippy::unused_async)]
    pub async fn next(&mut self) -> Option<Result<Response<'_>, S::Error>> {
        todo!("yield each T_Data.ind; end on tP_Client expiry")
    }
}
