//! The session-layer seam.
//!
//! `uds_on_ip` does not implement the session layer. ISO 14229-5:2022 clause 10
//! has exactly two requirements on the subject — REQ 5.1 and REQ 5.2 — and both
//! say the same thing: implement it as specified in ISO 14229-2. That is
//! `uds_session`'s job.
//!
//! This module defines the trait through which `uds_on_ip` drives a session
//! layer, so that the two crates can be developed independently and so that an
//! interim in-crate implementation can be swapped for `uds_session` without a
//! change to this crate's public API.
//!
//! # Direction of travel
//!
//! The trait is driven from *both* sides, because `uds_on_ip` sits both above
//! and below the session layer (see `ARCHITECTURE.md` §8.1):
//!
//! - Downward, from the application profile: [`SessionLayer::s_data_req`].
//! - Upward, from the transport mapping: [`SessionLayer::t_data_conf`] and
//!   [`SessionLayer::t_data_ind`].
//!
//! A sans-io session layer supports this naturally, because a single caller
//! owns both edges by construction.
//!
//! # A note on the clock
//!
//! Timestamps are `u32` milliseconds, the unit ISO 14229-2 states its timing
//! parameters in. A `u32` count of milliseconds wraps after roughly 49.7 days,
//! so every comparison against one must use wrapping-difference arithmetic
//! rather than ordering: `now.wrapping_sub(started) >= timeout`, never
//! `now >= deadline`. The width is this seam's choice rather than the
//! standard's, taken so a bare-metal target need not carry a 64-bit clock.

use crate::addressing::{Ai, ChannelId};
use crate::primitives::SResult;

/// The reload parameters a transport supplies for a channel's `tP_Client`.
///
/// ISO 14229-2:2021 clause 9.1.2 splits `tP_Client`'s reload values four ways
/// according to whether the transport offers a `T_DataSOM.ind` primitive, and
/// the session layer deliberately does not distinguish the cases: it holds a
/// default and an enhanced parameter and asks the transport binding which they
/// are.
///
/// `DoIP` has no `T_DataSOM.ind`, so for this crate they are always
/// `tP6_Client_Max` and `tP6*_Client_Max` (REQ 5.11).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChannelTiming {
    /// `tP6_Client_Max` — the wait for a complete response after `T_Data.conf`.
    pub default_reload_ms: u32,
    /// `tP6*_Client_Max` — the enhanced wait after a response-pending NRC.
    pub enhanced_reload_ms: u32,
}

/// The outcome of a transport-layer transmission, `T_Result`.
///
/// ISO 14229-2:2021 clause 7 Table 2 maps `T_Result` onto `S_Result`
/// one-for-one, so this is an alias rather than a parallel enum.
///
/// Delivered to the session layer through [`SessionLayer::t_data_conf`], which
/// is what starts `tP_Client` (ISO 14229-2:2021 REQ 5.9). On `DoIP` this is
/// derived from the diagnostic message acknowledgement, not from the act of
/// writing to the socket — see [`crate::mapping`].
pub type TResult = SResult;

/// The outcome reported to the application, `A_Result`.
///
/// Clause 7 Table 1 maps `A_Result` onto `S_Result`, so this is also an alias.
/// Three names for one enum is what the standard specifies; collapsing them
/// would hide the layer each belongs to at a call site.
pub type AResult = SResult;

/// Work the session layer wants its caller to perform.
///
/// The caller drains these after every input. This is the "retrieve outputs"
/// half of a sans-io interface: the session layer performs no I/O itself.
///
/// # A note on the borrow
///
/// Each variant that carries bytes borrows them from the session layer, which
/// means an action cannot be held across a subsequent call into the session.
/// A driver must therefore consume each action before polling again. Whether
/// this survives contact with a real async driver is the first thing this
/// prototype is meant to find out; if it does not, the alternative is for
/// actions to carry ranges into a caller-owned buffer, which suits a no-alloc
/// session layer better anyway.
#[derive(Debug)]
pub enum SessionAction<'a> {
    /// Hand these bytes to the transport for this channel.
    Transmit {
        /// The channel the bytes belong to.
        channel: ChannelId,
        /// The bytes to transmit.
        data: &'a [u8],
    },
    /// Deliver a response to the application, `A_Data.ind`.
    Indicate {
        /// The channel the response arrived on.
        channel: ChannelId,
        /// The responding server, which for a functional request differs per
        /// response.
        source: crate::addressing::Address,
        /// The response bytes.
        data: &'a [u8],
    },
    /// Complete an exchange, `A_Data.conf`.
    Confirm {
        /// The channel the exchange belonged to.
        channel: ChannelId,
        /// How it ended.
        result: AResult,
    },
    /// Arm a timer. The caller owns the clock, so it owns the timer too.
    SetTimer {
        /// The channel whose timer this is.
        channel: ChannelId,
        /// Absolute deadline on the caller's millisecond clock.
        deadline_ms: u32,
    },
    /// Disarm a channel's timer.
    ClearTimer {
        /// The channel whose timer to disarm.
        channel: ChannelId,
    },
}

/// A UDS session layer, as specified by ISO 14229-2.
///
/// Implemented by `uds_session`. `uds_on_ip` supplies every input and drains
/// every output on the application's behalf; the session layer reads no clock
/// and touches no transport.
pub trait SessionLayer {
    /// Errors this session layer can report.
    type Error: core::fmt::Debug;

    /// Open a logical communication channel and allocate its timer storage.
    ///
    /// `timing` supplies the channel's default and enhanced reload parameters;
    /// for `DoIP` these are the `tP6` pair (ISO 14229-2:2021 REQ 5.11).
    ///
    /// # Errors
    /// If no channel storage remains.
    fn open_channel(&mut self, ai: Ai, timing: ChannelTiming)
    -> Result<ChannelId, Self::Error>;

    /// Submit a request for transmission, `S_Data.req`.
    ///
    /// # Errors
    /// If the channel is unknown, or an exchange is already in progress on it.
    fn s_data_req(
        &mut self,
        now_ms: u32,
        channel: ChannelId,
        data: &[u8],
    ) -> Result<(), Self::Error>;

    /// Report that a transmission completed, `T_Data.conf`.
    ///
    /// This starts `tP_Client` (ISO 14229-2:2021 REQ 5.9). On `DoIP` it is raised
    /// when the diagnostic message acknowledgement arrives, never when the
    /// socket write returns.
    fn t_data_conf(&mut self, now_ms: u32, channel: ChannelId, result: TResult);

    /// Deliver received bytes, `T_Data.ind`. This stops `tP_Client`
    /// (ISO 14229-2:2021 REQ 5.10).
    fn t_data_ind(&mut self, now_ms: u32, channel: ChannelId, data: &[u8]);

    /// Advance time without delivering anything, so timers can expire.
    fn tick(&mut self, now_ms: u32);

    /// When this layer next needs waking, so a driver can sleep until then
    /// rather than spinning. `None` means no timer is armed.
    ///
    /// Without this a sans-io layer gives its caller no way to know when to
    /// wake, leaving busy-polling as the only correct option.
    fn next_deadline_ms(&self) -> Option<u32>;

    /// Retrieve the next action, or `None` when there is no more work.
    fn poll(&mut self, now_ms: u32) -> Option<SessionAction<'_>>;
}
