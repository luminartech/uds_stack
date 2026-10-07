//! The connection table of ISO 13400-2:2019 12.6.1.2: what each slot owns, and the
//! routing activation the socket handler is arbitrating.

use embassy_time::Instant;

use crate::LogicalAddress;
use crate::messages::{ActivationTypeCode, ProtocolVersion};
use crate::stream::rx::RxBuffer;
use crate::stream::tx::TxQueue;

/// One position in the table. `CAP` sizes both buffers: a full message for a connection
/// slot, a routing activation exchange for the reserve.
pub(super) struct Slot<S, const CAP: usize> {
    pub(super) open: Option<Open<S>>,
    pub(super) rx: RxBuffer<CAP>,
    pub(super) tx: TxQueue<CAP>,
    pub(super) closed_unreported: bool,
}

/// An established socket and where it is in Figure 25.
pub(super) struct Open<S> {
    pub(super) socket: S,
    pub(super) phase: Phase,
    /// `T_TCP_Initial_Inactivity`'s expiry while [`Phase::Initialized`] until a routing
    /// activation request is received, `T_TCP_General_Inactivity`'s from then on, the
    /// close's bound while [`Phase::Finalizing`].
    pub(super) deadline: Instant,
    /// Whether a routing activation request has been received, which stops
    /// `T_TCP_Initial_Inactivity` (REQ 3.DoIP-085).
    pub(super) activation_received: bool,
    /// The version of the last frame received, which the entity's answers carry.
    pub(super) version: ProtocolVersion,
    /// Whether an event has named this connection, so its close is owed a report.
    pub(super) named: bool,
}

/// Figure 25's states. `Registered` is `Registered [Routing Active]`: neither
/// authentication nor confirmation is supported, so the two pending sub-states are
/// passed through on the spot (REQ 3.DoIP-129, 130).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Initialized,
    Registered {
        sa: LogicalAddress,
        alive_check: AliveCheck,
    },
    Finalizing {
        abort: bool,
        by_caller: bool,
    },
}

/// A registered socket's part in an [`Arbitration`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AliveCheck {
    NotAsked,
    /// To be sent an alive check request once its transmit queue has room.
    Due,
    Asked,
    Answered,
}

/// The routing activation Figure 26 is deciding, held across `next_event` calls.
#[derive(Clone, Copy)]
pub(super) struct Arbitration {
    pub(super) on: SlotRef,
    pub(super) request: Activation,
    pub(super) stage: Stage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// Figure 27 or 28, until `T_TCP_Alive_Check` expires at `deadline`.
    AliveCheck {
        scope: AliveCheckScope,
        deadline: Instant,
    },
    /// Accepted, waiting for a connection slot to take the reserve socket.
    Assign,
}

/// Figure 27 or Figure 28.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AliveCheckScope {
    SocketOfSa,
    AllRegistered,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SlotRef {
    Connection(usize),
    Reserve,
}

#[derive(Clone, Copy)]
pub(super) struct Activation {
    pub(super) sa: LogicalAddress,
    pub(super) activation_type: ActivationTypeCode,
}

impl<S, const CAP: usize> Slot<S, CAP> {
    pub(super) const fn new() -> Self {
        Self {
            open: None,
            rx: RxBuffer::new(),
            tx: TxQueue::new(),
            closed_unreported: false,
        }
    }

    /// Whether an accepted socket may be put here.
    pub(super) fn is_free(&self) -> bool {
        self.open.is_none() && !self.closed_unreported
    }

    pub(super) fn phase(&self) -> Option<Phase> {
        self.open.as_ref().map(|open| open.phase)
    }

    pub(super) fn registered_sa(&self) -> Option<LogicalAddress> {
        match self.phase()? {
            Phase::Registered { sa, .. } => Some(sa),
            _ => None,
        }
    }

    pub(super) fn is_initialized(&self) -> bool {
        self.phase() == Some(Phase::Initialized)
    }

    pub(super) fn alive_check(&self) -> Option<AliveCheck> {
        match self.phase()? {
            Phase::Registered { alive_check, .. } => Some(alive_check),
            _ => None,
        }
    }

    pub(super) fn set_alive_check(&mut self, to: AliveCheck) {
        if let Some(Open {
            phase: Phase::Registered { alive_check, .. },
            ..
        }) = self.open.as_mut()
        {
            *alive_check = to;
        }
    }

    pub(super) fn open(&mut self, socket: S, deadline: Instant) {
        self.rx.clear();
        self.tx.clear();
        self.open = Some(Open {
            socket,
            phase: Phase::Initialized,
            deadline,
            version: ProtocolVersion::V2019,
            named: false,
            activation_received: false,
        });
    }

    /// Starts closing the socket, by `abort` or after writing what is queued. A close
    /// already started keeps going, made an abort if `abort`, and owned by the caller if
    /// `by_caller`.
    pub(super) fn finalize(&mut self, abort: bool, by_caller: bool, deadline: Instant) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if let Phase::Finalizing {
            abort: was_abort,
            by_caller: was_by_caller,
        } = open.phase
        {
            open.phase = Phase::Finalizing {
                abort: abort || was_abort,
                by_caller: by_caller || was_by_caller,
            };
        } else {
            open.phase = Phase::Finalizing { abort, by_caller };
            open.deadline = deadline;
        }
    }

    /// Takes the socket out of the table, owing a report of its close if an event named
    /// it and the caller did not close it.
    pub(super) fn remove(&mut self) -> Option<S> {
        let open = self.open.take()?;
        let by_caller = matches!(
            open.phase,
            Phase::Finalizing {
                by_caller: true,
                ..
            }
        );
        self.closed_unreported = open.named && !by_caller;
        self.rx.clear();
        self.tx.clear();
        Some(open.socket)
    }
}
