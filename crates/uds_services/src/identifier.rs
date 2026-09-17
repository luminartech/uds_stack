//! The identifiers an application defines.
//!
//! ``UDSSVC_ARCH_0014`` — this crate defines no data identifier, no routine identifier
//! and no DTC. Which values a server implements is the application's, and an enumeration
//! shipped here would be wrong for every application that did not happen to match it.
//!
//! ``UDSSVC_ARCH_0026`` — the application also supplies each identifier's record
//! structure, because only it knows how wide a record is. That is what makes a
//! multi-identifier response splittable without this crate knowing anything about the
//! data.

/// Why a record could not be taken from a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordError {
    /// Fewer bytes remained than this identifier's record needs.
    Short,
    /// The bytes were the right length but not a valid record.
    Malformed,
}

/// A data identifier this application supports.
///
/// ``UDSSVC_ARCH_0014``, ``UDSSVC_ARCH_0024`` — one vocabulary serves both roles, so the
/// same enumeration names a server's handlers and a client's requests.
pub trait DataIdentifier: Copy + Eq {
    /// The longest record any variant carries.
    ///
    /// Folded into the response buffer at [`crate::uds_server`]'s expansion site. It is
    /// the application's own number: nothing here can compute it.
    const MAX_RECORD_LEN: usize;

    /// The wire value.
    fn as_u16(self) -> u16;

    /// From the wire. `None` means this application does not support it, which the
    /// pipeline turns into `requestOutOfRange` (0x31).
    fn from_u16(value: u16) -> Option<Self>;

    /// Split this identifier's record off the front of `buf`, returning it and the rest.
    ///
    /// # Errors
    ///
    /// [`RecordError`] where `buf` is too short or the record is not valid.
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError>;
}

/// A routine identifier this application supports.
///
/// Separate from [`DataIdentifier`] because a routine has no record structure: its
/// option and status records are the handler's to interpret, which is
/// ``UDSSVC_ARCH_0007``'s consequence for service identifier 0x31.
pub trait RoutineIdentifier: Copy + Eq {
    /// The longest status record any variant returns.
    const MAX_STATUS_LEN: usize;

    /// The wire value.
    fn as_u16(self) -> u16;

    /// From the wire. `None` means this application does not support it.
    fn from_u16(value: u16) -> Option<Self>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests assert on the success path")]
mod tests {
    use super::{DataIdentifier, RecordError};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestDid {
        VehicleSpeed,
        VinNumber,
    }

    impl DataIdentifier for TestDid {
        const MAX_RECORD_LEN: usize = 17;
        fn as_u16(self) -> u16 {
            match self {
                Self::VehicleSpeed => 0xF4_0D,
                Self::VinNumber => 0xF1_90,
            }
        }
        fn from_u16(value: u16) -> Option<Self> {
            match value {
                0xF4_0D => Some(Self::VehicleSpeed),
                0xF1_90 => Some(Self::VinNumber),
                _ => None,
            }
        }
        fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
            let width = match self {
                Self::VehicleSpeed => 1,
                Self::VinNumber => 17,
            };
            buf.split_at_checked(width).ok_or(RecordError::Short)
        }
    }

    /// ``UDSSVC_ARCH_0014`` — an identifier the application does not define is `None`,
    /// which is how requestOutOfRange (0x31) is reached rather than a panic.
    #[test]
    fn an_unknown_identifier_is_none() {
        assert_eq!(TestDid::from_u16(0xF1_90), Some(TestDid::VinNumber));
        assert_eq!(TestDid::from_u16(0x0000), None);
    }

    /// ``UDSSVC_ARCH_0026`` — the application splits its own record, because only it
    /// knows each identifier's width. A short buffer is an error, not a truncation.
    #[test]
    fn a_record_is_split_by_the_application() {
        let buf = [0_u8; 20];
        let (record, rest) = TestDid::VinNumber.split_record(&buf).unwrap();
        assert_eq!((record.len(), rest.len()), (17, 3));
        assert_eq!(
            TestDid::VinNumber.split_record(&buf[..4]),
            Err(RecordError::Short)
        );
    }

    /// The declared maximum is what the macro folds into the response buffer, so it
    /// must be reachable as a const from a concrete type.
    #[test]
    fn the_declared_maximum_is_const() {
        const MAX: usize = <TestDid as DataIdentifier>::MAX_RECORD_LEN;
        assert_eq!(MAX, 17);
    }
}
