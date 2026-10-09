//! ISO 14229-2:2021 session layer services.
//!
//! This crate is built against ISO 14229-2:2021: a table, figure, clause or requirement
//! cited in it with no document named is that one's, and any other document is named
//! where it is cited.
//!
//! A transport-agnostic session layer for UDS diagnostics, implemented as a sans-io state
//! machine: no clock is read, no transport is called, and no executor is involved. The
//! caller supplies a monotonic timestamp together with inbound transport events, and
//! drains the resulting actions.
//!
//! The crate is `no_std` and performs no allocation. Storage is supplied by the caller, by
//! value: `Server<A>` owns an array of `A` associations, and `Client<K, PHYS, FUNC, R>`
//! owns `PHYS` physical channel slots and `FUNC` functional channel slots of `R`
//! responders each, with `K` the keep-alive mode of ``UDSS_LLR_0149``. Sizing every array
//! is a deployment decision, expressed as a const generic, rather than a compile-time
//! constant of this crate.
//!
//! The client's arrays split by channel kind because the two kinds hold different state:
//! ``UDSS_LLR_0139`` gives a functional channel a responder table and a physical channel
//! none, and ``UDSS_LLR_0151`` gives a physical channel a `tS3_Client` timer and a
//! functional channel none. Splitting the arrays keeps both facts true by construction and
//! stops a physical channel being charged storage for a table it must not keep. The split
//! agrees with ``UDSS_LLR_0049``'s `S_AI[TAtype]` because
//! [`Client::open_physical_channel`] and [`Client::open_functional_channel`] each supply
//! the matching `ta_type` themselves from a [`ChannelAddressing`] that cannot state the
//! other one, so a channel stored in one array can never carry the other kind's addressing.
//!
//! `unsafe` is forbidden crate-wide, and the lint configuration in `Cargo.toml` applies to
//! every target rather than to the crate root alone.
//!
//! # Status
//!
//! Both roles are implemented and tested against their requirements: the server in
//! `tests/server_behaviour.rs`, the client in `tests/client_behaviour.rs`.
//!
//! # How the surface discharges its requirements
//!
//! Four requirements are best verified by looking at these types directly, so what a
//! reviewer should look at is stated here rather than left to be inferred. Three of them
//! name inspection of the crate's types as their own verification method; the fourth,
//! ``UDSS_LLR_0081``, is a property of the order the drain yields in rather than verified
//! that way, and is included here for the same reason.
//!
//! - **``UDSS_LLR_0011``** — outputs are retrieved, not pushed. Nothing here delivers an
//!   output through a callback, handler or caller-supplied trait implementation: no public
//!   type takes a caller-supplied trait object or function, and every trait in a public
//!   bound is sealed: `Drain`, which a caller cannot name, and [`KeepAliveMode`], the only
//!   one a caller can name, which selects a [`Client`]'s mode and carries no output.
//!   Storage is supplied by value for the same reason, so no implementation of either can
//!   be the caller's. Every input returns a [`Reaction`] the caller drains, and nothing
//!   is lost to one finished early: an expiry's indication stays in the session until it
//!   is retrieved, and [`Reaction::finish`] hands back the input's own outputs not yet
//!   drained, in [`Finished::rest`].
//! - **``UDSS_LLR_0013``** — no payload is retained. No type here holds an owned buffer.
//! - **``UDSS_LLR_0014``** — an output refers to caller-owned data. [`ServerOutput`] and
//!   [`ClientOutput`] borrow `&'d [u8]` from the input that supplied it, and the
//!   [`Reaction`] carrying them cannot outlive that borrow.
//! - **``UDSS_LLR_0081``** — expiry indications precede the input's outputs and any
//!   rejection report. The drain yields every expiry indication before any output of the
//!   input, and [`Finished::rest`] keeps that order for what was not drained; the report
//!   is [`Finished::outcome`], read beside it.
//!
//! # Requirements discharged by construction
//!
//! Where the requirement set asks for an input to be rejected and these types make that
//! input unwritable, the requirement is satisfied without a check —
//! ``UDSS_LLR_0027`` states that discharge explicitly and the rest follow it:
//!
//! | Requirement | What makes it unrepresentable |
//! | --- | --- |
//! | ``UDSS_LLR_0027`` (second limb) | `t_data_ind`/`t_data_som_ind` require a channel |
//! | ``UDSS_LLR_0030`` | [`ServerTx`], [`ServerRx`], absent methods, and no channel param |
//! | ``UDSS_LLR_0031`` | [`ClientTx`], [`ClientRx`], and the absent completion report |
//! | ``UDSS_LLR_0054`` | a payload passed as a slice carries its own length |
//! | ``UDSS_LLR_0066`` | [`ExpectedResponses::Exactly`] holds a `NonZeroU16` |
//! | ``UDSS_LLR_0067`` | `ClientTx::KeepAlive` carries no session selection |
//! | ``UDSS_LLR_0068`` | `ServerRx::KeepAlive` carries no session selection |
//! | ``UDSS_LLR_0070`` | [`ClientTx`]'s expected count is a required field |
//! | ``UDSS_LLR_0071`` | `Solicitation` is a required field on a final response |
//! | ``UDSS_LLR_0072`` | the enums admit no other form; full walk in the requirement |
//! | ``UDSS_LLR_0134`` (wrong-kind limb) | the setters each require that kind's own id |
//! | ``UDSS_LLR_0151`` | no channel parameter states an `s3_client` |
//! | ``UDSS_LLR_0152`` | the reload methods exist only on the mode that has one |
//!
//! [`Reaction::finish`]: Reaction::finish

#![no_std]

mod addressing;
mod classification;
mod client;
mod keep_alive;
mod params;
mod reaction;
mod rejection;
mod result;
mod sealed;
mod server;
mod time;
mod timer;

pub use addressing::{
    Address, AddressExtension, Ai, ChannelAddressing, Mtype, PeerIdentity, TaType,
};
pub use classification::{
    ClientRx, ClientTx, ExpectedResponses, ServerRx, ServerTx, SessionSelection,
    Solicitation,
};
pub use client::{
    ChannelId, Client, ClientOutput, ClientReaction, FunctionalChannelId, FunctionalSlot,
    PhysicalChannelId, PhysicalSlot,
};
pub use keep_alive::{FunctionalKeepAlive, KeepAliveMode, PhysicalKeepAlive};
pub use params::{
    ChannelParameter, ChannelParams, ChannelReload, Reloads, ServerParameter, ServerParams,
    ServerReload,
};
pub use reaction::{Finished, Outputs, Reaction, Rest};
pub use rejection::{Cause, Causes, Content, Rejection, ReportedCause};
pub use result::{SResult, TransportError};
pub use server::{Association, Server, ServerOutput, ServerReaction};
pub use time::Timestamp;
