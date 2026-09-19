//! The outcome parameter the service primitives carry.

/// An error a layer below the session layer detected.
///
/// ``UDSS_LLR_0056`` — every ``S_Result`` value other than the ones this crate names is
/// such an error, carried without interpreting it. The session layer neither defines
/// these values nor acts on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransportError(pub u16);

/// The outcome of a service execution.
///
/// ``UDSS_LLR_0056`` — ``S_OK`` where the execution completed successfully, and every
/// other value an error a lower layer detected. That is a two-way split, which is why
/// this enumeration is closed: what ISO 14229-2:2021 8.10 leaves open is the space of
/// error *values*, and [`TransportError`] carries that.
///
/// There is no timeout here. Every timeout this set defines is its own indication, not an
/// outcome of a service execution — ``UDSS_LLR_0148`` gives the client's response-timing
/// indication to [`crate::ClientOutput::ResponseTimeout`] and ``UDSS_LLR_0100`` the
/// server's to [`crate::ServerOutput::SessionTimeout`] — and a timer this crate runs is
/// not an error a lower layer detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SResult {
    /// ``S_OK`` — the service execution completed successfully.
    ///
    /// ``UDSS_LLR_0035`` makes ``S_Data`` and ``S_Length`` meaningful only here.
    Ok,
    /// An error a lower layer detected, carried through without interpretation.
    ///
    /// ``UDSS_LLR_0056`` — 8.10 does not enumerate the error values, so the session layer
    /// carries one rather than acting on its meaning.
    Transport(TransportError),
}
