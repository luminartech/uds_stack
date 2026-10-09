//! The seal on [`ServiceSet`](crate::ServiceSet) and [`Storage`](crate::Storage).
//!
//! Both traits are implemented by [`crate::uds_server`] and never by hand, and the seal
//! is what makes "never" a compile error rather than a request. What rests on it is the
//! whole derivation argument: an application that writes its own
//! `impl ServiceSet { type Store = Store<1, 1, 1>; .. }` compiles, truncates every request
//! at one byte, and still advertises a block length taken from `MAX_BLOCK_LENGTH`.
//!
//! [`Storage`](crate::Storage) is sealed outright — `Store` is its only implementor and
//! nothing outside this crate can add one. [`ClientStorage`](crate::ClientStorage) and
//! `ClientStore` are the client's pair, sealed for the same reason: a hand-written
//! implementor could pick buffer lengths that disagree with the maxima the assembly
//! folded.
//!
//! [`ServiceSet`](crate::ServiceSet) is sealed as far as a macro-implemented trait can be.
//! The expansion happens in the application's crate, so the path it names has to be
//! reachable from there; what the seal buys is that implementing `ServiceSet` by hand now
//! requires deliberately naming a `#[doc(hidden)]` item this module documents as not
//! yours. It is a lock, not a wall.

/// Implemented by [`crate::uds_server`], [`crate::uds_client`], `Store` and
/// `ClientStore`. Not yours to implement.
pub trait Sealed {}
