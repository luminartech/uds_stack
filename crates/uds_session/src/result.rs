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
/// ``UDSS_LLR_0056``. ``UDSS_LLR_0035`` makes ``S_Data`` and ``S_Length`` meaningful
/// only where this is [`SResult::Ok`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SResult {
    /// The service completed successfully.
    Ok,
    /// No response arrived inside the window — ISO 14229-2:2021 9.1.2's error condition,
    /// reported by ``UDSS_LLR_0148``.
    Timeout,
    /// An error a lower layer detected, carried through unchanged.
    Transport(TransportError),
}
