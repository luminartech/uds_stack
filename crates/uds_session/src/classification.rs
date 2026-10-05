//! Message classification — this set's own construct, not the standard's.
//!
//! ``UDSS_LLR_0073`` forbids the session layer to inspect message data, and roughly forty
//! requirements condition on what kind of message is in hand. ``UDSS_LLR_0057`` therefore
//! has the caller state it and ``UDSS_LLR_0065`` fixes its values.
//!
//! The values are split by role. A `Server` accepts only [`ServerTx`] and [`ServerRx`],
//! a `Client` only [`ClientTx`] and [`ClientRx`], so the cross-role classifications
//! ``UDSS_LLR_0030`` and ``UDSS_LLR_0031`` require to be rejected cannot be written.

use core::num::NonZeroU16;

/// Whether the session a message selects is the default one.
///
/// ``UDSS_LLR_0065`` — the selection states this rather than naming the session, because
/// ``UDSS_LLR_0073`` bars recognising an ISO 14229-1 session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionSelection {
    /// The message selects the default session.
    Default,
    /// The message selects some non-default session.
    NonDefault,
}

/// How many responses a client expects to a request.
///
/// ``UDSS_LLR_0065``. ``UDSS_LLR_0135`` conditions on the count being other than
/// [`ExpectedResponses::None`], and ``UDSS_LLR_0138`` on the exact number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpectedResponses {
    /// No response is expected. ``UDSS_LLR_0074`` reports such a request complete.
    None,
    /// Exactly this many. ``UDSS_LLR_0066`` rejects zero, which this type cannot express.
    Exactly(NonZeroU16),
    /// Some unknown number — ISO 14229-2:2021 9.7 Table 9's unknown-count column.
    Unknown,
}

impl ExpectedResponses {
    /// The exact count, where one was stated.
    ///
    /// ``UDSS_LLR_0138`` reads it.
    #[must_use]
    pub const fn exact(self) -> Option<NonZeroU16> {
        match self {
            Self::Exactly(n) => Some(n),
            Self::None | Self::Unknown => None,
        }
    }
}

/// Whether a final response answers a request.
///
/// ``UDSS_LLR_0071`` — a final response must state one or the other. It does not apply to
/// a response-pending message, which is by construction a reply to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Solicitation {
    /// Transmitted because of a request received from a client.
    Solicited,
    /// Transmitted for any other reason — a periodic response, for instance.
    Unsolicited,
}

/// What a client asks to transmit.
///
/// ``UDSS_LLR_0065``. Kind is always `request`; ``UDSS_LLR_0031`` rejects a client
/// transmitting a response, which this type cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientTx {
    /// A `TesterPresent` sent to keep a non-default session alive.
    ///
    /// ``UDSS_LLR_0157`` and ``UDSS_LLR_0161`` condition on this. It carries no session
    /// selection, which is what ``UDSS_LLR_0067`` requires, and it is a separate variant
    /// from a repeat because ``UDSS_LLR_0065`` forbids a request to state both.
    KeepAlive {
        /// Physical keep-alive may or may not require a response, so the count is stated.
        expected: ExpectedResponses,
    },
    /// Any other request.
    Request {
        /// ``UDSS_LLR_0070`` rejects a client request stating no count; a required field
        /// makes that unrepresentable.
        expected: ExpectedResponses,
        /// Whether this repeats a request whose transmission, reception or response
        /// window failed — ISO 14229-2:2021 9.7 Table 9, via ``UDSS_LLR_0176``.
        repeat: bool,
        /// Present where the request effects a session transition — ``UDSS_LLR_0086``.
        session: Option<SessionSelection>,
    },
}

impl ClientTx {
    /// The session selection, where the message effects a transition.
    #[must_use]
    pub const fn session_selection(self) -> Option<SessionSelection> {
        match self {
            Self::KeepAlive { .. } => None,
            Self::Request { session, .. } => session,
        }
    }

    /// How many responses the message expects (``UDSS_LLR_0065``), whichever kind it is.
    #[must_use]
    pub const fn expected(self) -> ExpectedResponses {
        match self {
            Self::KeepAlive { expected } | Self::Request { expected, .. } => expected,
        }
    }
}

/// What a server asks to transmit.
///
/// ``UDSS_LLR_0065``. ``UDSS_LLR_0030`` rejects a server transmitting a request, which
/// this type cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerTx {
    /// A positive response, or a negative response whose code is not
    /// `requestCorrectlyReceived-ResponsePending`.
    FinalResponse {
        /// ``UDSS_LLR_0071`` requires one or the other.
        solicitation: Solicitation,
        /// Present where the response effects a session transition.
        ///
        /// ``UDSS_LLR_0085`` and ``UDSS_LLR_0098`` read it to decide whether the server's
        /// session timer starts or the server returns to the default session.
        session: Option<SessionSelection>,
    },
    /// A negative response whose code is `requestCorrectlyReceived-ResponsePending`.
    ///
    /// ``UDSS_LLR_0118`` rejects one while another is unconfirmed, and ``UDSS_LLR_0119``
    /// while the minimum spacing has not elapsed. The session layer never composes one:
    /// that is the application layer's, per this set's preamble.
    ResponsePending,
}

/// What a client receives.
///
/// ``UDSS_LLR_0065``. ``UDSS_LLR_0031`` rejects a client receiving a request, which this
/// type cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientRx {
    /// A final response from a server.
    FinalResponse {
        /// ``UDSS_LLR_0071`` requires one or the other.
        solicitation: Solicitation,
        /// Present where the response effects a session transition.
        ///
        /// ``UDSS_LLR_0159`` and ``UDSS_LLR_0163`` read it, together with `solicitation`,
        /// to engage or disengage physical keep-alive.
        session: Option<SessionSelection>,
    },
    /// A response-pending message — ``UDSS_LLR_0136`` and ``UDSS_LLR_0146`` act on it.
    ResponsePending,
}

/// What a server receives, and what its completion report carries.
///
/// ``UDSS_LLR_0065`` and ``UDSS_LLR_0074``. Kind is always `request`; the server states
/// no expected count and no repeat, neither having a server-side meaning.
/// ``UDSS_LLR_0030`` rejects a server receiving a response, which this type cannot
/// express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerRx {
    /// The functionally addressed `TesterPresent` whose positive response is suppressed,
    /// which ISO 14229-1:2020 8.7.6 exempts from one-request-at-a-time.
    ///
    /// It carries no session selection, which is what ``UDSS_LLR_0068`` requires.
    KeepAlive,
    /// Any other request.
    Request {
        /// Present where the request effects a session transition — ``UDSS_LLR_0098``.
        session: Option<SessionSelection>,
    },
}

impl ServerRx {
    /// The session selection, where the message effects a transition.
    #[must_use]
    pub const fn session_selection(self) -> Option<SessionSelection> {
        match self {
            Self::KeepAlive => None,
            Self::Request { session } => session,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientTx, ExpectedResponses, ServerRx, SessionSelection};
    use core::num::NonZeroU16;

    /// ``UDSS_LLR_0065`` — the count is "none, an exact number of at least one, or
    /// unknown". ``UDSS_LLR_0066`` rejects zero, which `NonZeroU16` makes
    /// unrepresentable, and ``UDSS_LLR_0070`` rejects its absence, which a required
    /// field makes unrepresentable.
    #[test]
    fn an_exact_count_is_at_least_one() {
        let one = ExpectedResponses::Exactly(NonZeroU16::MIN);
        assert_eq!(one.exact(), Some(NonZeroU16::MIN));
        assert_eq!(ExpectedResponses::None.exact(), None);
        assert_eq!(ExpectedResponses::Unknown.exact(), None);
    }

    /// ``UDSS_LLR_0065`` — both kinds of client message state their expected responses.
    #[test]
    fn either_kind_states_its_expected_responses() {
        let ka = ClientTx::KeepAlive {
            expected: ExpectedResponses::None,
        };
        assert_eq!(ka.expected(), ExpectedResponses::None);
        let request = ClientTx::Request {
            expected: ExpectedResponses::Unknown,
            repeat: true,
            session: None,
        };
        assert_eq!(request.expected(), ExpectedResponses::Unknown);
    }

    /// ``UDSS_LLR_0065`` — a request classification may state keep-alive or repeat, and
    /// "shall not state both". The two are separate variants, so it cannot.
    /// ``UDSS_LLR_0067`` rejects a keep-alive carrying a session selection; the
    /// `KeepAlive` variant carries no field for one.
    #[test]
    fn a_keep_alive_request_carries_no_session_selection() {
        let ka = ClientTx::KeepAlive {
            expected: ExpectedResponses::None,
        };
        assert_eq!(ka.session_selection(), None);

        let selecting = ClientTx::Request {
            expected: ExpectedResponses::Unknown,
            repeat: false,
            session: Some(SessionSelection::NonDefault),
        };
        assert_eq!(
            selecting.session_selection(),
            Some(SessionSelection::NonDefault)
        );
    }

    /// ``UDSS_LLR_0068`` — the same exclusion on the server's completion report, and on
    /// the request indications that share the type.
    #[test]
    fn a_server_keep_alive_carries_no_session_selection() {
        assert_eq!(ServerRx::KeepAlive.session_selection(), None);
        assert_eq!(
            ServerRx::Request {
                session: Some(SessionSelection::Default)
            }
            .session_selection(),
            Some(SessionSelection::Default)
        );
    }
}
