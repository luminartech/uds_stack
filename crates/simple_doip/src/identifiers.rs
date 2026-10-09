//! The identifiers vehicle identification carries: [`Vin`], [`EntityId`] and
//! [`GroupId`] (ISO 13400-2:2019 Tables 1 and 5).
//!
//! Each holds a value that is set. Table 1 gives each identifier the bytes that say it is
//! not: all `0x00` or all `0xFF`. Those are refused when one is built, so that `None`
//! is the one way to say an identifier is not set.

/// All `0x00` or all `0xFF`: the bytes ISO 13400-2:2019 Table 1 gives an identifier
/// that is not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("all 0x00 or all 0xFF: ISO 13400-2 Table 1's value for an identifier not set")]
pub struct NotSet;

/// Why a [`Vin`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VinError {
    /// The bytes say no VIN is set.
    #[error(transparent)]
    NotSet(#[from] NotSet),
    /// A byte is not ASCII, as ISO 13400-2 Tables 4 and 5 give a VIN.
    #[error("the VIN is not ASCII")]
    NotAscii,
}

/// Whether `bytes` are all `0x00` or all `0xFF`.
const fn not_set(bytes: &[u8]) -> bool {
    let mut zeros = true;
    let mut ones = true;
    let mut rest = bytes;
    while let [byte, tail @ ..] = rest {
        zeros &= *byte == 0x00;
        ones &= *byte == 0xFF;
        rest = tail;
    }
    zeros || ones
}

/// A vehicle identification number (ISO 13400-2:2019 Tables 4 and 5): ISO 3779's,
/// in 17 ASCII bytes.
///
/// # Examples
///
/// ```
/// use simple_doip::{Vin, VinError};
///
/// let vin = Vin::new(*b"WVWZZZ1JZXW000001")?;
/// assert_eq!(vin.to_bytes(), *b"WVWZZZ1JZXW000001");
/// assert!(matches!(Vin::new([0x00; 17]), Err(VinError::NotSet(_))));
/// # Ok::<(), VinError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Vin([u8; 17]);

impl Vin {
    /// Takes `bytes` as a VIN.
    ///
    /// # Arguments
    ///
    /// * `bytes` - the VIN, in ASCII.
    ///
    /// # Errors
    ///
    /// - [`VinError::NotSet`] where `bytes` are [`NotSet`]'s.
    /// - [`VinError::NotAscii`] where a byte is not ASCII.
    pub const fn new(bytes: [u8; 17]) -> Result<Self, VinError> {
        if not_set(&bytes) {
            return Err(VinError::NotSet(NotSet));
        }
        if !bytes.is_ascii() {
            return Err(VinError::NotAscii);
        }
        Ok(Self(bytes))
    }

    /// The VIN's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 17] {
        self.0
    }
}

impl TryFrom<[u8; 17]> for Vin {
    type Error = VinError;

    fn try_from(bytes: [u8; 17]) -> Result<Self, VinError> {
        Self::new(bytes)
    }
}

impl From<Vin> for [u8; 17] {
    fn from(vin: Vin) -> Self {
        vin.0
    }
}

/// An entity ID, the EID (ISO 13400-2:2019 Table 5): unique to its entity, and the MAC
/// address of one of its interfaces where it has one.
///
/// # Examples
///
/// ```
/// use simple_doip::{EntityId, NotSet};
///
/// let eid = EntityId::new([0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF])?;
/// assert_eq!(eid.to_bytes(), [0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF]);
/// assert_eq!(EntityId::new([0xFF; 6]), Err(NotSet));
/// # Ok::<(), NotSet>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId([u8; 6]);

impl EntityId {
    /// Takes `bytes` as an entity ID.
    ///
    /// # Arguments
    ///
    /// * `bytes` - the entity ID.
    ///
    /// # Errors
    ///
    /// [`NotSet`] where `bytes` are its.
    pub const fn new(bytes: [u8; 6]) -> Result<Self, NotSet> {
        if not_set(&bytes) {
            Err(NotSet)
        } else {
            Ok(Self(bytes))
        }
    }

    /// The entity ID's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 6] {
        self.0
    }
}

impl TryFrom<[u8; 6]> for EntityId {
    type Error = NotSet;

    fn try_from(bytes: [u8; 6]) -> Result<Self, NotSet> {
        Self::new(bytes)
    }
}

impl From<EntityId> for [u8; 6] {
    fn from(eid: EntityId) -> Self {
        eid.0
    }
}

/// A group ID, the GID (ISO 13400-2:2019 Table 5): shared by the entities of one
/// vehicle.
///
/// # Examples
///
/// ```
/// use simple_doip::{GroupId, NotSet};
///
/// let gid = GroupId::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])?;
/// assert_eq!(gid.to_bytes(), [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
/// assert_eq!(GroupId::new([0x00; 6]), Err(NotSet));
/// # Ok::<(), NotSet>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId([u8; 6]);

impl GroupId {
    /// Takes `bytes` as a group ID.
    ///
    /// # Arguments
    ///
    /// * `bytes` - the group ID.
    ///
    /// # Errors
    ///
    /// [`NotSet`] where `bytes` are its.
    pub const fn new(bytes: [u8; 6]) -> Result<Self, NotSet> {
        if not_set(&bytes) {
            Err(NotSet)
        } else {
            Ok(Self(bytes))
        }
    }

    /// The group ID's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 6] {
        self.0
    }
}

impl TryFrom<[u8; 6]> for GroupId {
    type Error = NotSet;

    fn try_from(bytes: [u8; 6]) -> Result<Self, NotSet> {
        Self::new(bytes)
    }
}

impl From<GroupId> for [u8; 6] {
    fn from(gid: GroupId) -> Self {
        gid.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_set_is_all_0x00_or_all_0xff_and_nothing_else() {
        assert!(not_set(&[0x00; 6]));
        assert!(not_set(&[0xFF; 17]));
        assert!(!not_set(&[0x00, 0x00, 0x00, 0x00, 0x00, 0xFF]));
        assert!(!not_set(&[0xFF, 0x00, 0x00, 0x00, 0x00, 0x00]));
        assert!(!not_set(&[0x01; 6]));
    }

    #[test]
    fn a_vin_with_a_byte_outside_ascii_is_refused() {
        assert_eq!(Vin::new(*b"WVW\x80ZZ1JZXW000001"), Err(VinError::NotAscii));
    }

    #[test]
    fn an_unset_vin_is_not_set_even_though_0x00_is_ascii() {
        assert_eq!(Vin::new([0x00; 17]), Err(VinError::NotSet(NotSet)));
        assert_eq!(Vin::new([0xFF; 17]), Err(VinError::NotSet(NotSet)));
    }
}
