//! Address information: the parameters ISO 14229-2:2021 8.3 to 8.7 define, and the peer
//! identity ``UDSS_LLR_0044`` forms from them.

/// A target or source address.
///
/// ``UDSS_LLR_0050`` (``S_TA``) and ``UDSS_LLR_0051`` (``S_SA``).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Address(pub u16);

/// An address extension.
///
/// ``UDSS_LLR_0052`` — a 16-bit unsigned value, present only where ``S_Mtype`` carries
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AddressExtension(pub u16);

/// The message type, which also fixes whether an address extension is present.
///
/// ``UDSS_LLR_0048`` — ISO 14229-2:2021 8.3's four values. The set transcribes the
/// four-value range rather than the prose's claim of two; see the open questions page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mtype {
    /// Diagnostics, no address extension.
    Diag,
    /// Secured diagnostics, no address extension.
    SecureDiag,
    /// Remote diagnostics, carrying an address extension.
    RDiag {
        /// ``S_AE``.
        ae: AddressExtension,
    },
    /// Secured remote diagnostics, carrying an address extension.
    SecureRDiag {
        /// ``S_AE``.
        ae: AddressExtension,
    },
}

impl Mtype {
    /// The address extension this message type carries, if any.
    ///
    /// ``UDSS_LLR_0048`` with ``UDSS_LLR_0052``.
    #[must_use]
    pub const fn address_extension(self) -> Option<AddressExtension> {
        match self {
            Self::Diag | Self::SecureDiag => None,
            Self::RDiag { ae } | Self::SecureRDiag { ae } => Some(ae),
        }
    }
}

/// The communication model a message uses.
///
/// ``UDSS_LLR_0049`` — ``S_AI[TAtype]``.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaType {
    /// One client to one server.
    Physical,
    /// One client to every server the address reaches.
    Functional,
}

/// The address information a message carries.
///
/// ``UDSS_LLR_0046`` states which primitives carry which of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ai {
    /// ``S_Mtype`` — ``UDSS_LLR_0048``.
    pub mtype: Mtype,
    /// ``S_AI[SA]`` — ``UDSS_LLR_0051``.
    pub sa: Address,
    /// ``S_AI[TA]`` — ``UDSS_LLR_0050``.
    pub ta: Address,
    /// ``S_AI[TAtype]`` — ``UDSS_LLR_0049``.
    pub ta_type: TaType,
}

/// The addressing of a channel, without its ``S_AI[TAtype]``.
///
/// ``UDSS_LLR_0121`` identifies a channel by the addressing stated when opening it, and
/// ``UDSS_LLR_0049`` gives ``S_AI[TAtype]`` two values, physical and functional. A channel
/// is one or the other because the *method* that opens it says which —
/// [`crate::Client::open_physical_channel`] and
/// [`crate::Client::open_functional_channel`] — so this type carries every other part of
/// the addressing and leaves `ta_type` for [`ChannelAddressing::with_ta_type`] to supply,
/// making a channel that disagrees with its own opening method unwritable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelAddressing {
    /// ``S_Mtype`` — ``UDSS_LLR_0048``.
    pub mtype: Mtype,
    /// ``S_AI[SA]`` — ``UDSS_LLR_0051``.
    pub sa: Address,
    /// ``S_AI[TA]`` — ``UDSS_LLR_0050``.
    pub ta: Address,
}

impl ChannelAddressing {
    /// The full addressing this channel addressing forms with a given ``S_AI[TAtype]``.
    ///
    /// ``UDSS_LLR_0121`` with ``UDSS_LLR_0049``: the caller can see the correspondence
    /// between a channel's addressing and the [`Ai`] the opening method forms from it, and
    /// the crate uses this internally to form the [`Ai`] it hands on.
    #[must_use]
    pub const fn with_ta_type(self, ta_type: TaType) -> Ai {
        Ai {
            mtype: self.mtype,
            sa: self.sa,
            ta: self.ta,
            ta_type,
        }
    }
}

/// Who a peer is.
///
/// ``UDSS_LLR_0044`` — an address and, where ``S_Mtype`` carries one, an address
/// extension. The derived equality is exactly the requirement's: both carry one and both
/// parts agree, or neither carries one and the addresses agree. An identity carrying an
/// extension is never equal to one that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerIdentity {
    /// The address half.
    pub address: Address,
    /// The extension half, present exactly where ``S_Mtype`` carries one.
    pub extension: Option<AddressExtension>,
}

impl Ai {
    /// The identity of the peer this message came from.
    ///
    /// ``UDSS_LLR_0044`` — ``S_AI[SA]`` with the extension ``S_Mtype`` carries.
    #[must_use]
    pub const fn source(&self) -> PeerIdentity {
        PeerIdentity {
            address: self.sa,
            extension: self.mtype.address_extension(),
        }
    }

    /// The identity of the peer this message is addressed to.
    ///
    /// ``UDSS_LLR_0044`` — ``S_AI[TA]`` with the extension ``S_Mtype`` carries.
    #[must_use]
    pub const fn target(&self) -> PeerIdentity {
        PeerIdentity {
            address: self.ta,
            extension: self.mtype.address_extension(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, AddressExtension, Ai, Mtype, PeerIdentity, TaType};

    /// ``UDSS_LLR_0048`` — two of the four message types carry an address extension.
    #[test]
    fn only_the_remote_types_carry_an_extension() {
        assert_eq!(Mtype::Diag.address_extension(), None);
        assert_eq!(Mtype::SecureDiag.address_extension(), None);
        assert_eq!(
            Mtype::RDiag {
                ae: AddressExtension(0x1234)
            }
            .address_extension(),
            Some(AddressExtension(0x1234))
        );
        assert_eq!(
            Mtype::SecureRDiag {
                ae: AddressExtension(0x0001)
            }
            .address_extension(),
            Some(AddressExtension(0x0001))
        );
    }

    /// ``UDSS_LLR_0044`` — equal addresses, neither carrying an extension.
    #[test]
    fn identities_without_extensions_compare_on_the_address() {
        let a = PeerIdentity {
            address: Address(0x10),
            extension: None,
        };
        let b = PeerIdentity {
            address: Address(0x10),
            extension: None,
        };
        let c = PeerIdentity {
            address: Address(0x11),
            extension: None,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// ``UDSS_LLR_0044`` — "an identity that carries an extension is never equal to one
    /// that does not", even where the addresses agree.
    #[test]
    fn an_extension_never_equals_its_absence() {
        let with = PeerIdentity {
            address: Address(0x10),
            extension: Some(AddressExtension(0)),
        };
        let without = PeerIdentity {
            address: Address(0x10),
            extension: None,
        };
        assert_ne!(with, without);
    }

    /// ``UDSS_LLR_0044`` — both carry one, and both parts must agree.
    #[test]
    fn identities_with_extensions_compare_on_both_parts() {
        let a = PeerIdentity {
            address: Address(0x10),
            extension: Some(AddressExtension(7)),
        };
        let b = PeerIdentity {
            address: Address(0x10),
            extension: Some(AddressExtension(8)),
        };
        assert_ne!(a, b);
    }

    /// ``UDSS_LLR_0044`` with ``UDSS_LLR_0048`` — the identity takes its extension from
    /// ``S_Mtype``, so a `Diag` message yields identities without one.
    #[test]
    fn identities_are_read_out_of_the_addressing() {
        let ai = Ai {
            mtype: Mtype::RDiag {
                ae: AddressExtension(3),
            },
            sa: Address(0xF1),
            ta: Address(0x10),
            ta_type: TaType::Physical,
        };
        assert_eq!(
            ai.source(),
            PeerIdentity {
                address: Address(0xF1),
                extension: Some(AddressExtension(3))
            }
        );
        assert_eq!(
            ai.target(),
            PeerIdentity {
                address: Address(0x10),
                extension: Some(AddressExtension(3))
            }
        );

        let plain = Ai {
            mtype: Mtype::Diag,
            ..ai
        };
        assert_eq!(
            plain.source(),
            PeerIdentity {
                address: Address(0xF1),
                extension: None
            }
        );
    }
}
