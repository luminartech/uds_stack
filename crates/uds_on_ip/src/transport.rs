//! The crate's outward interface.
//!
//! [`DoIpTransport`] carries the methods `uds_services::UdsTransport`
//! requires. The `impl` block arrives once that crate publishes the trait;
//! until then these are inherent methods with the agreed signatures, so the
//! implementation is exercised rather than blocked.
//!
//! Nothing here is shaped by `uds_services`: the trait's own test is that a
//! CAN binding implements the same methods, so a `DoIP`-shaped seam would be
//! the wrong seam.

use crate::error::Error;
use crate::mapping::target_of;
use uds_session::{Ai, Reloads, SResult, Timestamp};

/// The driver's view of what arrived, or that its deadline passed first.
///
/// Mirrors `uds_services::TransportEvent`, which is the trait's type once it
/// is published.
///
/// Two inbound facts have no case here, and that is an open hole rather than a
/// decision: a driver looping on [`next_event`](DoIpTransport::next_event)
/// cannot learn that its connection went away (ISO 14229-5:2022 REQ 7.9,
/// REQ 7.11), and a periodic response (REQ 7.16) has nowhere to be delivered.
/// Both are cases on a type `uds_services` owns; raised with them 2026-09-17,
/// and held by `mapping::tests::the_two_cases_with_nowhere_to_go` rather than
/// by this paragraph.
///
/// # Why this carries no lifetime
///
/// An earlier shape was `TransportEvent<'a>` with `DataInd` holding
/// `data: &'a [u8]`, returned from `next_event(&mut self, ..)`. That ties the
/// event's lifetime to the transport's `&mut self`, so holding the request
/// bytes holds the transport mutably borrowed and
/// [`t_data_req`](DoIpTransport::t_data_req) can never be called — the server
/// could never answer the request it had just received. It is not a corner
/// case: every inbound path reaches it, because the decoded request borrows
/// the bytes and the handler writes its response while those borrows are live.
///
/// Reported by `uds_services` on 2026-09-17 and fixed here by having
/// [`next_event`](DoIpTransport::next_event) fill a buffer the caller owns.
/// The borrow ends when the call returns, so this type is `'static` and the
/// offending lifetime does not exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportEvent {
    /// A complete inbound message, written into the buffer the caller passed
    /// to [`next_event`](DoIpTransport::next_event).
    DataInd {
        /// Addressing, with the responder's `S_AI[SA]` — the only way to tell
        /// functional responses apart.
        ai: Ai,
        /// How many bytes of the caller's buffer the payload occupies. The
        /// payload is that buffer's first `len` bytes.
        len: usize,
    },
    /// A message longer than the buffer supplied, of which only the front
    /// arrived.
    ///
    /// Its first `len` bytes are in the caller's buffer and `declared` is how
    /// long the message actually was. This is **classifiable but not
    /// dispatchable**: enough to read the service identifier and answer
    /// `busyRepeatRequest` (0x21), which ISO 14229-1 8.7.6 owes a request
    /// arriving while a service is in progress, and not enough to decode.
    ///
    /// # Why a variant rather than a flag on `DataInd`
    ///
    /// `DataInd { ai, len, truncated }` was the alternative. It loses to the
    /// asymmetry of how each is ignored: `let DataInd { ai, len, .. }` discards
    /// the flag in ordinary, idiomatic destructuring, and the consequence is a
    /// fragment decoded as though it were a whole message. A separate variant
    /// can be swallowed by a wildcard arm too, but swallowing it *drops* the
    /// message, which costs a retry the client is already required to make.
    /// One shape fails safe and the other fails dangerous.
    DataTooLong {
        /// Addressing, so the caller can answer the peer it came from.
        ai: Ai,
        /// How many bytes of the caller's buffer hold the front of the
        /// message. At least the service identifier, unless the message was
        /// empty.
        len: usize,
        /// How long the message actually was.
        ///
        /// Free on `DoIP`: ISO 13400-2's generic header carries the payload
        /// length, and it is read before the payload. It is what tells an
        /// entity how large its buffer needed to be — without it, a caller
        /// learns only "too long" and can never size for the traffic it
        /// actually sees. That matters most where [`inbound_max`] is `None`,
        /// since such an entity advertises no bound and can be sent anything.
        ///
        /// [`inbound_max`]: DoIpTransport::inbound_max
        declared: usize,
    },
    /// The outcome of a requested transmission.
    ///
    /// Raised from the diagnostic message **acknowledgement**, never from the
    /// socket write returning, because the acknowledgement is what starts
    /// `tP_Client` (ISO 14229-2:2021 REQ 5.9).
    DataConf {
        /// The addressing of the transmission being confirmed.
        ai: Ai,
        /// Derived from the acknowledgement's **code**, never from its
        /// payload type.
        ///
        /// The two disagree in practice. `simple_doip`'s
        /// `Message::diagnostic_message_ack` stamps the positive payload type
        /// (`0x8002`) into the header whatever the ack code says — its own
        /// documented limitation — so reading the payload type would report a
        /// rejection as an acceptance and start `tP_Client` for a message the
        /// entity never accepted. ISO 13400-2 makes the code the authority
        /// regardless of which crate is emitting.
        ///
        /// # A rejection cannot yet say why
        ///
        /// `SResult::Transport` carries a `TransportError(u16)` precisely so a
        /// lower layer's own code reaches the caller unchanged, and there is
        /// nothing to put in it: `Payload::decode` maps a received `0x8003` to
        /// a fieldless variant, discarding the NACK code, both addresses and
        /// the echoed request bytes. So every rejection currently collapses to
        /// "the transport refused it", where ISO 13400-2 distinguishes an
        /// unknown target address from routing not activated from an
        /// out-of-memory entity — three failures a tester acts on differently.
        /// Raised with `simple_doip` 2026-09-17.
        result: SResult,
    },
    /// The deadline the driver supplied passed before anything arrived.
    Deadline,
}

/// ISO 14229-5 over `DoIP`.
///
/// `S` is the socket, so no runtime is named: this builds for a bare-metal
/// target as readily as for tokio, and an adapter for either is additive.
pub struct DoIpTransport<S> {
    #[expect(
        dead_code,
        reason = "read once t_data_req and next_event leave todo!()"
    )]
    socket: S,
    reloads: Reloads,
    inbound_max: Option<usize>,
    outbound_max: Option<usize>,
}

// Written by hand rather than derived. `#[derive(Debug)]` here would generate
// a conditional `impl<S: Debug> Debug for DoIpTransport<S>`; on this toolchain
// `missing_debug_implementations` happens to accept that as covering the
// type, but the conditional impl is still the wrong API: it leaves
// `DoIpTransport<S>` with no `Debug` at all for any socket that does not
// itself implement `Debug`, which is the common case rather than the
// exception. An unconditional impl that treats the socket as opaque covers
// every `S`.
impl<S> core::fmt::Debug for DoIpTransport<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DoIpTransport")
            .field("socket", &"..")
            .field("reloads", &self.reloads)
            .field("inbound_max", &self.inbound_max)
            .field("outbound_max", &self.outbound_max)
            .finish()
    }
}

impl<S> DoIpTransport<S> {
    /// A transport over `socket`, loading the session layer's response timer
    /// with `reloads`, and advertising no size bound in either direction until
    /// one is learned.
    ///
    /// See [`profile::bench_reloads`](crate::profile::bench_reloads) for values
    /// suitable for a bench, and for why they are not suitable for a vehicle.
    pub const fn new(socket: S, reloads: Reloads) -> Self {
        Self {
            socket,
            reloads,
            inbound_max: None,
            outbound_max: None,
        }
    }

    /// Record this entity's own *Max. data size* — what it advertises in the
    /// ISO 13400-2:2019 Table 11 entity status response.
    ///
    /// MDS is "the maximum size of one logical **request** that this `DoIP`
    /// entity can process", which is bounded by the buffer a full request is
    /// decoded from: a request larger than that cannot be processed, whatever
    /// this entity claims. The driver derives that size from the services it
    /// assembled and reports it here, so the advertised value and the buffer
    /// that must hold the request cannot disagree.
    ///
    /// Note *which* buffer, because there are two and the larger one is not the
    /// one usually in [`next_event`](Self::next_event)'s hand. A driver keeps a
    /// full-size buffer for the request it is serving, and a small one to keep
    /// receiving while a service is in progress — ISO 14229-1 8.7.6 obliges it
    /// to accept the functionally addressed `TesterPresent` and the
    /// `0x00`–`0x0F` range in that window. **MDS is the full-size one.** A
    /// request arriving against the small buffer is reported as
    /// [`TransportEvent::DataTooLong`] and answered `busyRepeatRequest` (0x21);
    /// that is occupancy, not a size this entity cannot handle, so it must not
    /// lower what is advertised.
    pub fn set_inbound_max(&mut self, max: Option<usize>) {
        self.inbound_max = max;
    }

    /// Record the peer's advertised *Max. data size*, learned from the peer's
    /// entity status response.
    ///
    /// A server typically has not requested one, which is why this stays
    /// `None` and `responseTooLong` is then unreachable rather than fabricated.
    pub fn set_outbound_max(&mut self, max: Option<usize>) {
        self.outbound_max = max;
    }

    /// The largest `A_PDU` this entity will accept, where it advertises one.
    ///
    /// ISO 13400-2:2019 Table 11 — support for *Max. data size* is
    /// **optional**, so `None` is conformant.
    #[must_use]
    pub const fn inbound_max(&self) -> Option<usize> {
        self.inbound_max
    }

    /// The largest `A_PDU` the peer will accept, where it has advertised one.
    ///
    /// This is what bounds a *response*: MDS is defined as the maximum size of
    /// one logical **request** the entity can process, so a server asking what
    /// it may send is asking about the client.
    #[must_use]
    pub const fn outbound_max(&self) -> Option<usize> {
        self.outbound_max
    }

    /// The `tP_Client` reload pair this transport dictates.
    ///
    /// [`uds_session::Reloads`], because the session layer owns the pair. What
    /// this transport dictates is *which* pair: `DoIP` has no `T_DataSOM.ind`,
    /// so ISO 14229-2:2021 REQ 5.11 gives it `tP6` rather than `tP2`. The
    /// session layer does not distinguish the two, so the choice lives in these
    /// values and nowhere else.
    #[must_use]
    pub const fn channel_timing(&self) -> Reloads {
        self.reloads
    }

    /// The current time.
    ///
    /// [`Timestamp`] rather than a bare `u32`: it is a newtype over exactly
    /// that `u32`, and it carries `interval_since`, the modulo-2³² subtraction
    /// `UDSS_LLR_0019` requires. Typing the clock this way means the value
    /// `uds_session`'s `next_deadline` returns can be handed straight back to
    /// [`next_event`](Self::next_event) with no arithmetic on either side of
    /// the seam.
    ///
    /// This is the same argument `profile` already makes for keeping
    /// milliseconds rather than a `Duration`, one step further: carrying a bare
    /// `u32` where a `Timestamp` is meant leaves the conversion implicit rather
    /// than absent.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        todo!("the clock belongs to whatever S is; see the missing-bound note on next_event")
    }

    /// `T_Data.req` — map a `T_PDU` onto a `DoIP` diagnostic message and send it.
    ///
    /// # Errors
    ///
    /// [`Error::Mapping`] if the addressing cannot be carried: the two remote
    /// message types have no `DoIP` representation.
    ///
    /// A *socket* failure has no variant yet — see
    /// [`next_event`](Self::next_event).
    #[expect(
        unused_variables,
        reason = "data is unused until t_data_req's body replaces the todo!() below"
    )]
    pub async fn t_data_req(&mut self, ai: Ai, data: &[u8]) -> Result<(), Error> {
        let _target = target_of(ai)?;
        todo!("REQ 4.3 Table 4 — send as a DoIP diagnostic message")
    }

    /// The next inbound event, or [`TransportEvent::Deadline`] when `deadline`
    /// passes first.
    ///
    /// An inbound payload is written into `buffer` and reported as
    /// [`TransportEvent::DataInd`]'s `len`; the caller reads
    /// `&buffer[..len]`. `deadline` is the session layer's `next_deadline`, so
    /// this transport never invents one.
    ///
    /// # Why the caller supplies the buffer
    ///
    /// So that the returned event borrows nothing from `self`. See
    /// [`TransportEvent`] for the defect that shape had. It also means this
    /// crate holds no inbound buffer of its own: the `DoIP` header is read into
    /// a small local array and the payload goes straight into `buffer`, so the
    /// only bytes that outlive the call are the caller's own.
    ///
    /// `buffer`'s length is what this entity can actually receive, and is
    /// therefore the value it should advertise as its ISO 13400-2:2019 Table 11
    /// *Max. data size* — see [`set_inbound_max`](Self::set_inbound_max).
    ///
    /// # A message longer than `buffer`
    ///
    /// Reported as [`TransportEvent::DataTooLong`], never as a `DataInd` whose
    /// `len` happens to equal `buffer.len()`. The caller must be able to tell a
    /// whole message from the front of a longer one: the first is dispatchable
    /// and the second is only classifiable, and delivering the second as the
    /// first would have a decoder read a fragment as a message.
    ///
    /// A caller passing a deliberately small buffer — to keep receiving while a
    /// service is in progress, as ISO 14229-1 8.7.6 requires for the
    /// functionally addressed `TesterPresent` and the `0x00`–`0x0F` range —
    /// should expect this whenever an ordinary request arrives in that window.
    /// It is the normal outcome there, not a fault: 8.7.6 owes that request
    /// `busyRepeatRequest` (0x21) regardless, and composing one needs the
    /// service identifier and the addressing, both of which this carries.
    ///
    /// Truncation is decided from the generic header, whose payload length is
    /// read before the payload itself — so the decision is made before there is
    /// a whole message to classify, and never by comparing `len` to
    /// `buffer.len()` after the fact.
    ///
    /// # Errors
    ///
    /// [`Error::Wire`] if a `DoIP` message could not be decoded.
    ///
    /// # A socket failure has nowhere to go yet
    ///
    /// `S` carries no bound, so a socket has no error type for [`Error`] to
    /// compose, and [`Error::Wire`] is explicitly not it. There is no such
    /// bound available to take: `automotive-wire-codec`'s `Sink` is
    /// synchronous, has no read side, and its `WriteError::Io` carries no
    /// detail by design, while every async surface in `simple_doip` sits
    /// behind its `codec` feature, which requires `std` and a runtime. No
    /// `no_std` async socket seam exists anywhere in this stack.
    ///
    /// The shape this will take is settled — `uds_services`' `UdsTransport`
    /// declares an associated `Error`, so the socket's failure is this crate's
    /// to name — but the bound it names is an open decision. Until it lands,
    /// the failure this method is most likely to have is unrepresentable.
    #[expect(
        unused_variables,
        reason = "buffer and deadline are unused until next_event's body replaces the todo!()"
    )]
    pub async fn next_event(
        &mut self,
        buffer: &mut [u8],
        deadline: Option<Timestamp>,
    ) -> Result<TransportEvent, Error> {
        todo!("read a DoIP message, mapping::classify it, translate the two seam cases")
    }
}

#[cfg(test)]
mod tests {
    /// ISO 13400-2:2019 Table 11 lists *Max. data size* as the fourth item of
    /// the entity status response and marks its support **optional**, so a
    /// conformant `DoIP` entity need not advertise one and `None` is a correct
    /// answer rather than a defect.
    ///
    /// A bound is therefore reported only where one was learned, and never
    /// fabricated: a response sink bounded at an invented number would reject
    /// responses the peer would have accepted.
    #[test]
    fn an_unadvertised_max_data_size_is_none_not_a_guess() {
        let t = super::DoIpTransport::new((), crate::profile::bench_reloads());
        assert_eq!(t.inbound_max(), None);
        assert_eq!(t.outbound_max(), None);
    }

    /// MDS is "the maximum size of one logical **request** that this `DoIP`
    /// entity can process", so the two directions are different questions and
    /// answering one does not answer the other.
    #[test]
    fn the_two_directions_are_independent() {
        let mut t = super::DoIpTransport::new((), crate::profile::bench_reloads());
        t.set_inbound_max(Some(4096));
        assert_eq!(t.inbound_max(), Some(4096));
        assert_eq!(
            t.outbound_max(),
            None,
            "this entity's own MDS says nothing about what the peer will accept"
        );
    }

    /// The property whose absence made the driver unwritable.
    ///
    /// `TransportEvent` previously carried a lifetime borrowed from
    /// `next_event`'s `&mut self`, so a driver holding the request bytes could
    /// not call `t_data_req` to answer them. The assertion is the bound, not
    /// the call — `assert_static` has no body worth running.
    ///
    /// Verified by watching it fail: reintroducing a borrowing variant on the
    /// enum breaks this line's build, because naming `TransportEvent` without a
    /// lifetime argument stops resolving.
    ///
    /// **If you are here because this line failed to compile, do not repair it
    /// by writing `TransportEvent<'_>`.** In this position `'_` is inferred as
    /// `'static`, so the bound would be satisfied trivially and the guarantee
    /// would be gone while the test still passed. The failure means the enum
    /// regained a lifetime, which is the defect — fix the enum.
    #[test]
    fn an_event_borrows_nothing_from_the_transport() {
        const fn assert_static<T: 'static>() {}
        assert_static::<super::TransportEvent>();
    }

    /// The seam carries four cases, and a fifth must not arrive unnoticed.
    ///
    /// This match is exhaustive with no wildcard arm, so adding a case to
    /// [`TransportEvent`] stops this test compiling. That is the intent: the
    /// two cases this seam is *missing* — a periodic response and a connection
    /// close — are tracked in `mapping::tests::the_two_cases_with_nowhere_to_go`,
    /// and when `uds_services` adds either to its type, both guards fire at
    /// once and point at each other.
    ///
    /// Verified by watching it fail: a fifth variant added to the enum breaks
    /// this match as non-exhaustive.
    #[test]
    fn the_seam_carries_four_cases() {
        fn name(event: &super::TransportEvent) -> &'static str {
            match event {
                super::TransportEvent::DataInd { .. } => "DataInd",
                super::TransportEvent::DataTooLong { .. } => "DataTooLong",
                super::TransportEvent::DataConf { .. } => "DataConf",
                super::TransportEvent::Deadline => "Deadline",
            }
        }

        assert_eq!(name(&super::TransportEvent::Deadline), "Deadline");
    }

    /// `DataInd` reports an extent into the caller's buffer rather than a
    /// borrow of the transport, so the caller reads `&buffer[..len]`.
    #[test]
    fn a_data_indication_indexes_the_callers_buffer() {
        let buffer = [0xAA_u8; 8];
        let ai = uds_session::Ai {
            mtype: uds_session::Mtype::Diag,
            sa: uds_session::Address(0x0E80),
            ta: uds_session::Address(0x0E00),
            ta_type: uds_session::TaType::Physical,
        };
        let event = super::TransportEvent::DataInd { ai, len: 3 };

        let super::TransportEvent::DataInd { len, .. } = event else {
            panic!("constructed a DataInd")
        };
        assert_eq!(&buffer[..len], &[0xAA, 0xAA, 0xAA]);
    }

    /// A truncated message is a distinct variant, so the case cannot be
    /// reached by the destructuring that reads a whole one.
    ///
    /// `DataInd`'s fields are a subset of `DataTooLong`'s, which is what makes
    /// the flag alternative dangerous: `DataInd { ai, len, .. }` would bind
    /// identically whether or not a `truncated` flag were set. Matching by
    /// variant cannot do that — an arm written for a whole message does not
    /// accept a truncated one.
    #[test]
    fn a_truncated_message_is_not_a_whole_one() {
        let ai = uds_session::Ai {
            mtype: uds_session::Mtype::Diag,
            sa: uds_session::Address(0x0E80),
            ta: uds_session::Address(0x0E00),
            ta_type: uds_session::TaType::Physical,
        };
        let whole = super::TransportEvent::DataInd { ai, len: 4 };
        let front = super::TransportEvent::DataTooLong {
            ai,
            len: 4,
            declared: 4096,
        };

        assert_ne!(
            whole, front,
            "the same four bytes mean different things depending on whether \
             more of the message exists"
        );
        assert!(
            !matches!(front, super::TransportEvent::DataInd { .. }),
            "an arm written for a dispatchable message must not accept a \
             classifiable fragment"
        );
    }

    /// `declared` is what tells an entity how large its buffer needed to be.
    ///
    /// Reporting only `len` would say "too long" without ever saying how long,
    /// so an entity could never size for the traffic it actually sees — which
    /// bites hardest where `inbound_max` is `None` and nothing is advertised.
    #[test]
    fn a_truncated_message_reports_the_size_it_needed() {
        let ai = uds_session::Ai {
            mtype: uds_session::Mtype::Diag,
            sa: uds_session::Address(0x0E80),
            ta: uds_session::Address(0x0E00),
            ta_type: uds_session::TaType::Physical,
        };
        let super::TransportEvent::DataTooLong { len, declared, .. } =
            (super::TransportEvent::DataTooLong {
                ai,
                len: 8,
                declared: 1026,
            })
        else {
            panic!("constructed a DataTooLong")
        };

        assert!(declared > len, "truncation means more existed than arrived");
        assert_eq!(
            declared, 1026,
            "the shortfall is knowable, not merely detectable"
        );
    }

    /// `Timestamp` is what makes the deadline exchangeable across the seam
    /// without arithmetic: the value `uds_session` reports as a next deadline
    /// goes straight back into `next_event`, and `interval_since` carries
    /// `UDSS_LLR_0019`'s modulo-2³² subtraction so a wrap is not a special
    /// case at either end.
    #[test]
    fn a_deadline_survives_the_wrap_it_is_typed_for() {
        let before_wrap = uds_session::Timestamp(u32::MAX - 10);
        let after_wrap = uds_session::Timestamp(5);
        assert_eq!(after_wrap.interval_since(before_wrap), 16);
    }
}
