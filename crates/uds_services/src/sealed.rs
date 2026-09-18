//! The seal on [`ServiceSet`](crate::ServiceSet) and [`Storage`](crate::Storage).
//!
//! Both traits are documented as implemented by [`crate::uds_server`] and never by hand,
//! and before this module that was a convention a doc comment held. What it held is the
//! whole derivation argument: an application that writes its own
//! `impl ServiceSet { type Store = Store<1, 1, 1>; .. }` compiles, truncates every request
//! at one byte, and still advertises `MAX_BLOCK_LENGTH` over the wire.
//!
//! [`Storage`](crate::Storage) is sealed outright — `Store` is its only implementor and
//! nothing outside this crate can add one.
//!
//! [`ServiceSet`](crate::ServiceSet) is sealed as far as a macro-implemented trait can be.
//! The expansion happens in the application's crate, so the path it names has to be
//! reachable from there; what the seal buys is that implementing `ServiceSet` by hand now
//! requires deliberately naming a `#[doc(hidden)]` item this module documents as not
//! yours. It is a lock, not a wall.

/// Implemented by [`crate::uds_server`] and by `Store`. Not yours to implement.
pub trait Sealed {}
