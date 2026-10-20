//! The seal on [`ServiceSet`](crate::ServiceSet), [`ClientSet`](crate::ClientSet),
//! [`Storage`](crate::Storage) and [`ClientStorage`](crate::ClientStorage).
//!
//! The macros implement these traits and nothing else does: a hand-written implementor
//! could pick buffer lengths that disagree with the maxima the assembly folded.
//! [`Storage`](crate::Storage) and [`ClientStorage`](crate::ClientStorage) have their
//! implementors in this crate. The macro-implemented traits are sealed as far as a trait
//! implemented in the application's crate can be: implementing one by hand requires
//! naming a `#[doc(hidden)]` item this module documents as not yours.

/// Implemented by [`crate::uds_server`], [`crate::uds_client`], `Store` and
/// `ClientStore`. Not yours to implement.
pub trait Sealed {}
