//! Rejection reports.
//!
//! ``UDSS_LLR_0015`` fixes what refusing an input means: the rejection is reported to the
//! caller, the input itself produces no output to the application and none to the
//! transport layer, and the state is left as the accompanying timestamp's expiries left
//! it. ``UDSS_LLR_0081`` orders the indications those expiries produce *before* the
//! report, which is why a report is reached only by consuming a drained reaction.
//!
//! A rejection is not an `S_Data.conf`: ``UDSS_LLR_0056`` reserves every non-`Ok`
//! `S_Result` for an error a lower layer detected, and no lower layer is involved.

/// Declares [`Cause`] and everything derived from it.
///
/// The variants, their bits, their documentation and their rendered text are one list.
/// [`Cause::ALL`] and the [`Display`](core::fmt::Display) arms are generated from it, so a
/// cause cannot come to exist that iteration fails to yield or that renders as nothing —
/// which is what a second, hand-maintained copy of the variant list would allow, silently,
/// against ``UDSS_LLR_0016``. The one hazard left is two variants sharing a bit, and
/// `every_cause_has_its_own_bit` walks `ALL` to rule that out.
macro_rules! causes {
    (
        $(
            $(#[$attr:meta])*
            $variant:ident = $bit:literal => $text:literal,
        )+
    ) => {
        /// Why an input was rejected.
        ///
        /// One variant per rejecting requirement that remains expressible. The
        /// requirements this crate's types discharge by construction —
        /// ``UDSS_LLR_0027`` (second limb), ``UDSS_LLR_0030``, ``UDSS_LLR_0031``,
        /// ``UDSS_LLR_0054``, ``UDSS_LLR_0066``, ``UDSS_LLR_0067``, ``UDSS_LLR_0068``,
        /// ``UDSS_LLR_0070``, ``UDSS_LLR_0071``, ``UDSS_LLR_0072``, ``UDSS_LLR_0152``
        /// and part of ``UDSS_LLR_0134`` — have no variant here, because an input that
        /// triggers them cannot be written.
        ///
        /// Exhaustive. The set is one variant per rejecting requirement, so it is closed
        /// by the requirement set; no requirement here states an open enumeration, and
        /// only ``UDSS_LLR_0010`` and ``UDSS_LLR_0012`` do so anywhere in the set. A
        /// cause can appear only by a requirement changing, and a caller whose match then
        /// fails to compile is being told exactly that — which a wildcard arm would
        /// swallow.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Cause {
            $( $(#[$attr])* $variant, )+
        }

        impl Cause {
            /// The bit this cause occupies in a [`Rejection`]'s set.
            const fn bit(self) -> u16 {
                match self {
                    $( Self::$variant => $bit, )+
                }
            }

            /// Every cause, in bit order, for iteration.
            const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];
        }

        impl core::fmt::Display for Cause {
            /// One short, lower-case phrase naming the condition, not the requirement
            /// number, so a caller reading it understands what it did wrong without
            /// opening the requirement set. ``UDSS_LLR_0016`` requires a report to state
            /// the cause; [`Rejection::causes`] is what states it, and this is a
            /// rendering of that state for a caller who wants text rather than a value to
            /// match on.
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(match self {
                    $( Self::$variant => $text, )+
                })
            }
        }
    };
}

causes! {
    /// ``UDSS_LLR_0027``, ``UDSS_LLR_0123``, ``UDSS_LLR_0124``, ``UDSS_LLR_0134`` and
    /// ``UDSS_LLR_0183`` — an input naming a channel the client does not have.
    NoSuchChannel = 0 => "no such channel",
    /// ``UDSS_LLR_0122`` — a channel opened with an existing channel's addressing.
    DuplicateChannelAddressing = 1 => "a channel already has this addressing",
    /// ``UDSS_LLR_0061`` — a request duplicating an outstanding association.
    AssociationOutstanding = 2 => "an association is already outstanding",
    /// ``UDSS_LLR_0062`` — no association is free.
    NoAssociationFree = 3 => "no association is free",
    /// ``UDSS_LLR_0063`` — a confirmation matching no outstanding association.
    NoMatchingAssociation = 4 => "no outstanding association matches",
    /// ``UDSS_LLR_0069`` — a classification stating no kind where one is required.
    KindRequired = 5 => "a kind is required but none was stated",
    /// ``UDSS_LLR_0118`` — a response-pending message while one is unconfirmed.
    ResponsePendingUnconfirmed = 6 =>
        "a response-pending message arrived while one is unconfirmed",
    /// ``UDSS_LLR_0119`` — a response-pending message inside the minimum spacing.
    ResponsePendingTooSoon = 7 =>
        "a response-pending message arrived inside the minimum spacing",
    /// ``UDSS_LLR_0171`` — a request on a channel whose spacing timer is running.
    /// [`Rejection::spacing_remaining`] carries the wait ``UDSS_LLR_0172`` requires.
    SpacingTimerRunning = 8 => "the channel's spacing timer is running",
    /// ``UDSS_LLR_0177`` — a third repeat. ISO 14229-2:2021 9.7 Table 9 caps them at two.
    RepeatCountSpent = 9 => "the repeat count is spent",
    /// ``UDSS_LLR_0178`` — a functional channel has not finished receiving.
    ResponseStillArriving = 10 => "a response is still arriving",
}

/// Why an input was refused, and what the refusing requirement asked the report to carry.
///
/// ``UDSS_LLR_0016`` — a set of causes rather than one, because where several
/// requirements reject the same input the one report states every cause and carries the
/// content each of them requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rejection {
    causes: u16,
    spacing_remaining: Option<u32>,
}

impl Rejection {
    /// A report stating one cause.
    #[allow(dead_code)]
    pub(crate) const fn new(cause: Cause) -> Self {
        Self {
            causes: 1u16 << cause.bit(),
            spacing_remaining: None,
        }
    }

    /// The same report, also stating `cause`.
    #[allow(dead_code)]
    pub(crate) const fn with(self, cause: Cause) -> Self {
        Self {
            causes: self.causes | (1u16 << cause.bit()),
            ..self
        }
    }

    /// The same report, carrying the wait ``UDSS_LLR_0172`` requires.
    #[allow(dead_code)]
    pub(crate) const fn with_spacing_remaining(self, remaining: u32) -> Self {
        Self {
            spacing_remaining: Some(remaining),
            ..self
        }
    }

    /// Whether this report states `cause`.
    #[must_use]
    pub const fn contains(self, cause: Cause) -> bool {
        self.causes & (1u16 << cause.bit()) != 0
    }

    /// Every cause this report states.
    ///
    /// ``UDSS_LLR_0016`` — more than one where more than one held. ``UDSS_LLR_0179``
    /// names the case: [`Cause::RepeatCountSpent`] and [`Cause::ResponseStillArriving`]
    /// call for opposite actions, so both are stated where both hold.
    #[must_use]
    pub const fn causes(self) -> Causes {
        Causes {
            report: self,
            next: 0,
        }
    }

    /// The time in milliseconds until the channel's spacing timer expires.
    ///
    /// ``UDSS_LLR_0172`` — present only on a rejection under ``UDSS_LLR_0171``, because
    /// ISO 14229-2:2021 10.3 postpones the request but gives the application no way to
    /// learn until when.
    #[must_use]
    pub const fn spacing_remaining(self) -> Option<u32> {
        self.spacing_remaining
    }
}

impl core::fmt::Display for Rejection {
    /// Every cause this report states, rendered in bit order and separated by `"; "`, with
    /// the spacing wait appended where [`Rejection::spacing_remaining`] is `Some`.
    /// ``UDSS_LLR_0016`` requires one report to state every cause that held and carry the
    /// content each rejecting requirement asked for, so rendering does not stop at the
    /// first cause, and ``UDSS_LLR_0172`` is the requirement that wait carries.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for cause in self.causes() {
            if first {
                first = false;
            } else {
                f.write_str("; ")?;
            }
            core::fmt::Display::fmt(&cause, f)?;
        }
        if let Some(remaining) = self.spacing_remaining {
            if !first {
                f.write_str("; ")?;
            }
            write!(f, "{remaining} ms remaining on the spacing timer")?;
        }
        Ok(())
    }
}

/// A rejection can be returned with `?` rather than translated at the call boundary.
///
/// No `source`: a rejection wraps nothing lower, it *is* the failure, so there is nothing
/// beneath it to report.
impl core::error::Error for Rejection {}

/// The causes a [`Rejection`] states. Created by [`Rejection::causes`].
#[derive(Debug)]
pub struct Causes {
    report: Rejection,
    next: usize,
}

impl Iterator for Causes {
    type Item = Cause;

    fn next(&mut self) -> Option<Cause> {
        while let Some(cause) = Cause::ALL.get(self.next) {
            self.next = self.next.saturating_add(1);
            if self.report.contains(*cause) {
                return Some(*cause);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Cause, Rejection};

    /// ``UDSS_LLR_0016`` — a report states every cause that held, so no two causes may
    /// share a bit: a shared bit would have one report state a cause that did not hold
    /// and hide one that did. `ALL` is generated from the same list that declares the
    /// variants, so walking it is a complete check rather than a sample.
    #[test]
    fn every_cause_has_its_own_bit() {
        let mut seen: u16 = 0;
        for cause in Cause::ALL {
            assert!(cause.bit() < 16, "{cause} does not fit the report's set");
            let mask = 1u16 << cause.bit();
            assert_eq!(seen & mask, 0, "{cause} shares a bit with an earlier cause");
            seen |= mask;
        }
    }

    /// ``UDSS_LLR_0016`` — a report states the cause of the rejection.
    #[test]
    fn a_report_states_its_cause() {
        let r = Rejection::new(Cause::NoSuchChannel);
        assert!(r.contains(Cause::NoSuchChannel));
        assert!(!r.contains(Cause::RepeatCountSpent));
        let mut listed = r.causes();
        assert_eq!(listed.next(), Some(Cause::NoSuchChannel));
        assert_eq!(listed.next(), None);
    }

    /// ``UDSS_LLR_0016`` — "where several requirements reject the same input, the one
    /// report shall state every cause". ``UDSS_LLR_0179`` is the case that needs it:
    /// ``UDSS_LLR_0177`` and ``UDSS_LLR_0178`` call for opposite actions, so a report
    /// naming only one would leave the application unable to follow Table 9.
    #[test]
    fn one_report_states_every_cause_that_held() {
        let r = Rejection::new(Cause::RepeatCountSpent).with(Cause::ResponseStillArriving);
        assert!(r.contains(Cause::RepeatCountSpent));
        assert!(r.contains(Cause::ResponseStillArriving));
        assert_eq!(r.causes().count(), 2);
    }

    /// Adding a cause twice leaves one report stating it once.
    #[test]
    fn a_cause_is_recorded_once() {
        let r = Rejection::new(Cause::NoSuchChannel).with(Cause::NoSuchChannel);
        assert_eq!(r.causes().count(), 1);
    }

    /// ``UDSS_LLR_0172`` — a rejection under ``UDSS_LLR_0171`` states the time remaining
    /// until the channel's spacing timer expires. No other rejection carries it.
    #[test]
    fn only_a_spacing_rejection_carries_a_remaining_time() {
        let spacing = Rejection::new(Cause::SpacingTimerRunning).with_spacing_remaining(17);
        assert_eq!(spacing.spacing_remaining(), Some(17));
        assert_eq!(
            Rejection::new(Cause::NoSuchChannel).spacing_remaining(),
            None
        );
    }

    /// A fixed-size `core::fmt::Write` sink. The crate has no `alloc`, so a test that
    /// renders a value needs somewhere to render it to.
    struct Buf {
        bytes: [u8; 256],
        used: usize,
    }

    impl Buf {
        const fn new() -> Self {
            Self {
                bytes: [0; 256],
                used: 0,
            }
        }

        fn as_str(&self) -> &str {
            core::str::from_utf8(self.bytes.get(..self.used).unwrap_or(&[])).unwrap_or("")
        }
    }

    impl core::fmt::Write for Buf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.used.saturating_add(s.len());
            let room = self.bytes.get_mut(self.used..end).ok_or(core::fmt::Error)?;
            room.copy_from_slice(s.as_bytes());
            self.used = end;
            Ok(())
        }
    }

    /// ``UDSS_LLR_0016`` — a report renders every cause it states, so a caller does not
    /// have to write the mapping themselves.
    #[test]
    fn a_report_renders_every_cause_it_states() {
        use core::fmt::Write as _;

        let r = Rejection::new(Cause::NoSuchChannel).with(Cause::RepeatCountSpent);
        let mut buf = Buf::new();
        write!(buf, "{r}").ok();
        let rendered = buf.as_str();
        assert!(rendered.contains("channel"), "rendered: {rendered}");
        assert!(rendered.contains("repeat"), "rendered: {rendered}");
    }

    /// ``UDSS_LLR_0172`` — a rejection carrying a spacing wait renders that wait, since
    /// it is content the rejecting requirement asked the report to carry.
    #[test]
    fn a_report_renders_its_spacing_wait() {
        use core::fmt::Write as _;

        let r = Rejection::new(Cause::SpacingTimerRunning).with_spacing_remaining(17);
        let mut buf = Buf::new();
        write!(buf, "{r}").ok();
        let rendered = buf.as_str();
        assert!(rendered.contains("17"), "rendered: {rendered}");
    }
}
