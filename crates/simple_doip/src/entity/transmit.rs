//! When an [`Entity`](super::Entity) confirms a diagnostic message it sends: once its
//! socket has taken the message, or once the socket reports it transmitted.

/// How much of a connection's stream its socket has transmitted, read by an entity that
/// confirms on transmission, [`OnTransmit`].
pub trait TransmitProgress {
    /// The bytes of the stream transmitted, counted from the first byte written to the
    /// socket. Never decreases. A socket that cannot tell returns the bytes written so
    /// far.
    fn transmitted(&self) -> u64;
}

/// When an [`Entity`](super::Entity) confirms a diagnostic message it sends:
/// [`OnWrite`] or [`OnTransmit`], for sockets of type `S`.
///
/// Implemented by those two alone.
pub trait ConfirmOn<S>: sealed::ConfirmOn<S> {}

/// Confirms a diagnostic message once its socket has taken its last byte: the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OnWrite;

/// Confirms a diagnostic message once its socket reports its last byte transmitted,
/// through [`TransmitProgress`].
///
/// A message written is confirmed within a millisecond of its socket transmitting it:
/// meanwhile the entity reads the progress every millisecond, and other connections'
/// confirms and events do not wait on it. A connection that closes first confirms what
/// it wrote, as [`OnWrite`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OnTransmit;

pub(super) mod sealed {
    pub trait ConfirmOn<S> {
        /// Whether `socket` has transmitted its stream through position `end`.
        fn through(socket: &S, end: u64) -> bool;
    }
}

impl<S> ConfirmOn<S> for OnWrite {}

impl<S> sealed::ConfirmOn<S> for OnWrite {
    fn through(_: &S, _: u64) -> bool {
        true
    }
}

impl<S: TransmitProgress> ConfirmOn<S> for OnTransmit {}

impl<S: TransmitProgress> sealed::ConfirmOn<S> for OnTransmit {
    fn through(socket: &S, end: u64) -> bool {
        socket.transmitted() >= end
    }
}
