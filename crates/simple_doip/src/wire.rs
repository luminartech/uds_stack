//! Wire-codec types re-exported from [`automotive_wire_codec`], so that a consumer
//! needs no dependency on it of its own.
//!
//! [`MessageError`](crate::messages::MessageError) carries [`Incomplete`],
//! [`TrailingBytes`] and [`WriteError`]; every message type implements [`Decode`] and
//! [`Encode`], whose `encode` takes `&mut impl Sink`; [`SliceSink`] encodes into a
//! plain `&mut [u8]`.
//!
//! `automotive-wire-codec`'s semver is part of this crate's public API: a breaking
//! release of it is a breaking change to `simple_doip`.

pub use automotive_wire_codec::{
    Decode, Encode, Incomplete, Sink, SliceSink, TrailingBytes, WriteError,
};
