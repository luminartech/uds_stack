//! `DoIP` logical addressing ([`LogicalAddress`]), the identifier space used to
//! address testers, ECUs, and gateways on a `DoIP` network, per ISO 13400-2.

use core::fmt::{Debug, Display, LowerHex, UpperHex};
#[cfg(feature = "std")]
use tracing::info;

#[derive(Clone, Copy, Eq)]
/// Logical addressing is used to identify the ECU
///
/// A physical logical address uniquely represents a diagnostic application
/// layer entity within any `DoIP` entity or on any server of the in-vehicle networks
/// connected via `DoIP` gateways.
pub struct LogicalAddress(
    /// The 16-bit address value, as transmitted on the wire.
    pub u16,
);

impl LogicalAddress {
    /// Lower bound of the logical address range reserved for external test equipment
    /// (testers). Addresses below this range are reserved
    /// for other entity classes (e.g. `DoIP` gateways, ECUs).
    pub const MIN_CLIENT_ADDRESS: LogicalAddress = LogicalAddress(0x0E00);
    /// Upper bound of the logical address range reserved for external test equipment
    /// (testers).
    pub const MAX_CLIENT_ADDRESS: LogicalAddress = LogicalAddress(0x0FFF);

    /// Sub-range of client addresses reserved for internal on-board diagnostics (OBD)
    /// tooling rather than general external testers (0x0F00-0x0F7F).
    /// A client address in this range is still valid, but
    /// [`is_valid_client_address`](Self::is_valid_client_address) logs an
    /// informational (`tracing::info!`) message
    /// since this crate's use cases are external testers, not OBD tooling.
    pub const OBD_ADDRESS_RANGE: (LogicalAddress, LogicalAddress) =
        (LogicalAddress(0x0F00), LogicalAddress(0x0F7F));

    /// Whether this is a vehicle-manufacturer-defined functional group address
    /// (ISO 13400-2:2019 Table 13, `E400`–`EFFF`).
    #[must_use]
    pub const fn is_functional_group(self) -> bool {
        matches!(self.0, 0xE400..=0xEFFF)
    }

    /// The [`TaType`] a received target address is taken to have, from its
    /// ISO 13400-2:2019 Table 13 range.
    ///
    /// [`TaType::Functional`] for a [functional group](Self::is_functional_group)
    /// address and for the `E000`–`E3FF` range Table 13 leaves to use-case-specific
    /// standards, which this crate treats as functional; [`TaType::Physical`] for
    /// every other address. A receiver has nothing else to go on, since the type is
    /// not on the wire, so an address in a range Table 13 leaves to the vehicle
    /// manufacturer is always taken as physical.
    #[must_use]
    pub const fn default_ta_type(self) -> TaType {
        match self.0 {
            0xE000..=0xEFFF => TaType::Functional,
            _ => TaType::Physical,
        }
    }

    /// Verify if the logical address is within the valid range for a client address
    /// of 0x0E00 - 0x0FFF
    #[must_use]
    pub fn is_valid_client_address(&self) -> bool {
        if *self >= Self::MIN_CLIENT_ADDRESS && *self <= Self::MAX_CLIENT_ADDRESS {
            // Check if the logical address is in the OBD range
            // For now we just log info to the user since this is a valid address,
            // but it is not recommended to use this range for client addresses
            // and is not in the use case of the crate at this time
            if *self >= Self::OBD_ADDRESS_RANGE.0 && *self <= Self::OBD_ADDRESS_RANGE.1 {
                #[cfg(feature = "std")]
                info!(
                    "Logical addresses in the 0x0F00-0x0F7F range are intended for internal \
                data collection/on-board diagnostics only. Ensure that this is the intended use case."
                );
            }
            true
        } else {
            false
        }
    }
}

/// `DoIP_TAtype`: the communication model of a target address (ISO 13400-2:2019
/// 8.2.2.4).
///
/// Not carried on the wire: a diagnostic message holds only the source and target
/// addresses, so a sender supplies the type and a receiver derives it, by default
/// with [`LogicalAddress::default_ta_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaType {
    /// One-to-one communication: the target is a single diagnostic application.
    Physical,
    /// One-to-many communication: the target is a group of diagnostic applications.
    Functional,
}

impl Display for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#06X}", self.0)
    }
}
impl Debug for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#06X}", self.0)
    }
}
impl UpperHex for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        UpperHex::fmt(&self.0, f)
    }
}
impl LowerHex for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        LowerHex::fmt(&self.0, f)
    }
}
impl From<u16> for LogicalAddress {
    fn from(addr: u16) -> Self {
        LogicalAddress(addr)
    }
}
impl From<LogicalAddress> for u16 {
    fn from(addr: LogicalAddress) -> Self {
        addr.0
    }
}
impl PartialOrd<u16> for LogicalAddress {
    fn partial_cmp(&self, other: &u16) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}
impl PartialOrd<LogicalAddress> for LogicalAddress {
    fn partial_cmp(&self, other: &LogicalAddress) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl PartialEq<u16> for LogicalAddress {
    fn eq(&self, other: &u16) -> bool {
        self.0 == *other
    }
}
impl PartialEq<LogicalAddress> for LogicalAddress {
    fn eq(&self, other: &LogicalAddress) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ISO 13400-2:2019 Table 13: `E400`–`EFFF` are the functional group addresses,
    /// and nothing either side of that range is one.
    #[test]
    fn functional_group_is_table_13s_range_and_no_wider() {
        for (address, functional_group) in [
            (0xE3FF, false),
            (0xE400, true),
            (0xEFFF, true),
            (0xF000, false),
            (0x0E00, false),
            (0x0000, false),
            (0xFFFF, false),
        ] {
            assert_eq!(
                LogicalAddress(address).is_functional_group(),
                functional_group,
                "{address:#06X}"
            );
        }
    }

    /// ISO 13400-2:2019 Table 13: `E000`–`E3FF` (use-case-specific standards) and
    /// `E400`–`EFFF` (functional groups) default to functional addressing, and every
    /// other address to physical.
    #[test]
    fn default_ta_type_is_functional_for_e000_to_efff_only() {
        for (address, ta_type) in [
            (0x0000, TaType::Physical),
            (0x0E00, TaType::Physical),
            (0x7FFF, TaType::Physical),
            (0xDFFF, TaType::Physical),
            (0xE000, TaType::Functional),
            (0xE3FF, TaType::Functional),
            (0xE400, TaType::Functional),
            (0xEFFF, TaType::Functional),
            (0xF000, TaType::Physical),
            (0xFFFF, TaType::Physical),
        ] {
            assert_eq!(
                LogicalAddress(address).default_ta_type(),
                ta_type,
                "{address:#06X}"
            );
        }
    }

    #[test]
    fn test_logical_address() {
        let addr = LogicalAddress(0x0E00);
        assert!(addr.is_valid_client_address());
        assert_eq!(addr.0, 0x0E00);
        assert_eq!(addr, LogicalAddress(0x0E00));
        assert_eq!(addr, 0x0E00);
    }
}
