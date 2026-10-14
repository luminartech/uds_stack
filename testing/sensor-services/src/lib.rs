//! An application implementing every staged service: each handler does what a sensor's
//! would, so that the pipeline's every stage is reachable.

#![no_std]

mod application;

pub use application::Ecu;

use uds_services::{DataIdentifier, RecordError, RoutineIdentifier};

/// The VIN the sensor reports.
pub const VIN: [u8; 17] = *b"WP0ZZZ99ZTS392124";

/// The sensor's data identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Did {
    /// `0xF190`: the VIN, read-only outside the extended session.
    Vin,
    /// `0xF40D`: the vehicle speed, read-only.
    VehicleSpeed,
    /// `0x0100`: a two-byte configuration, written once level 3 is unlocked.
    Config,
    /// `0x0101`: a one-byte mode, `0x00` to `0x03`.
    Mode,
}

impl Did {
    const fn width(self) -> usize {
        match self {
            Self::Vin => VIN.len(),
            Self::Config => 2,
            Self::VehicleSpeed | Self::Mode => 1,
        }
    }
}

impl DataIdentifier for Did {
    const MAX_RECORD_LEN: usize = VIN.len();
    fn as_u16(self) -> u16 {
        match self {
            Self::Vin => 0xF190,
            Self::VehicleSpeed => 0xF40D,
            Self::Config => 0x0100,
            Self::Mode => 0x0101,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xF190 => Some(Self::Vin),
            0xF40D => Some(Self::VehicleSpeed),
            0x0100 => Some(Self::Config),
            0x0101 => Some(Self::Mode),
            _ => None,
        }
    }
    fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
        let width = self.width();
        let (record, rest) = buf.split_at_checked(width).ok_or(RecordError::Short)?;
        match (self, record) {
            (Self::Mode, [mode]) if *mode > 0x03 => {
                Err(RecordError::Malformed { len: width })
            }
            _ => Ok((record, rest)),
        }
    }
}

/// The sensor's routines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rid {
    /// `0xFF00`: erases memory; started only, once level 3 is unlocked.
    EraseMemory,
    /// `0x0201`: a self test; started with a one-byte option, then its results read.
    SelfTest,
}

impl RoutineIdentifier for Rid {
    const MAX_STATUS_LEN: usize = 1;
    fn as_u16(self) -> u16 {
        match self {
            Self::EraseMemory => 0xFF00,
            Self::SelfTest => 0x0201,
        }
    }
    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0xFF00 => Some(Self::EraseMemory),
            0x0201 => Some(Self::SelfTest),
            _ => None,
        }
    }
}

/// The DTCs the sensor stores, each with its status.
pub const DTCS: [([u8; 3], u8); 3] = [
    ([0x08, 0x05, 0x11], 0x24),
    ([0x0A, 0x9B, 0x17], 0x26),
    ([0x25, 0x22, 0x1F], 0x2F),
];

#[cfg(test)]
mod tests {
    use uds_services::{DiagnosticSessionControl, DiagnosticSessionType};

    use crate::Ecu;

    #[test]
    fn the_application_starts_in_the_default_session() {
        assert!(Ecu::default().supports(DiagnosticSessionType::DefaultSession));
        assert_eq!(Ecu::default().downloaded(), (0, 0));
    }
}
