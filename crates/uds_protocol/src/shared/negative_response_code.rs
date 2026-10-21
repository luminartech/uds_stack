/// A negative response code: ISO 14229-1:2020 Table A.1, every byte of it.
///
/// Closed, because the table is: a byte the table leaves unnamed is one of the three
/// variants that carry it. Those are built only by [`From<u8>`], so a value never holds a
/// byte a named variant has, and `==` agrees with the wire; match them as
/// `IsoSaeReserved { .. }` and read the byte with [`Self::value`]. With `serde`, a code is
/// its byte.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(from = "u8", into = "u8"))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NegativeResponseCode {
    /// `0x00`, which Table A.1 reserves for a server's internal use; it never appears in a
    /// negative response.
    PositiveResponse,
    /// A byte Table A.1 reserves for future definition: `0x01`–`0x0F`, `0x15`–`0x20`,
    /// `0x23`, `0x27`–`0x30`, `0x32`, `0x3B`–`0x4F`, `0x5E`–`0x6F`, `0x74`–`0x77`,
    /// `0x79`–`0x7D`, `0x80`, `0x8E` and `0xFF`.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    IsoSaeReserved(u8),
    /// The server rejected the request and no more specific code fits (`0x10`, Table A.1).
    /// A server answers it only where no other code applies.
    GeneralReject,
    /// The service identifier is unknown to the server or unsupported (`0x11`, Table A.1).
    /// It applies only to a service the server does not support, so no service's own list
    /// of response codes names it.
    ServiceNotSupported,
    /// The service is supported but its sub-function is unknown or unsupported (`0x12`,
    /// Table A.1).
    SubFunctionNotSupported,
    /// The request's length, or the format of its parameters, does not match what the
    /// service prescribes (`0x13`, Table A.1).
    IncorrectMessageLengthOrInvalidFormat,
    /// The response would be longer than the underlying network layer can carry (`0x14`,
    /// Table A.1).
    ResponseTooLong,
    /// The server is temporarily too busy; the client repeats the request after a delay its
    /// implementation documents set (`0x21`, Table A.1).
    BusyRepeatRequest,
    /// A precondition the server checks before the action is not met (`0x22`, Table A.1).
    ConditionsNotCorrect,
    /// The request arrived out of the order the server expects, such as a
    /// sequence-sensitive request sent too early (`0x24`, Table A.1).
    RequestSequenceError,
    /// A subnet component the action needed did not answer in time (`0x25`, Table A.1).
    NoResponseFromSubnetComponent,
    /// A fault the server holds as a DTC prevents the action (`0x26`, Table A.1).
    FailurePreventsExecutionOfRequestedAction,
    /// A parameter is outside its permitted range, or names a data or routine identifier
    /// that is unsupported, or unsupported in the active session (`0x31`, Table A.1). Every
    /// service that reads, writes or adjusts data in the server supports it.
    RequestOutOfRange,
    /// The server's security conditions are not met: its test conditions, the required
    /// message sequence (for example `DiagnosticSessionControl` then `SecurityAccess`), or
    /// an unlocked server (`0x33`, Table A.1). Beyond the services that mandate it, a
    /// server may use it wherever security is required and not yet granted.
    SecurityAccessDenied,
    /// The client's authentication state does not grant the rights the request needs
    /// (`0x34`, Table A.1).
    AuthenticationRequired,
    /// The key the client sent does not match the server's (`0x35`, Table A.1). The attempt
    /// counts: the server stays locked and increments its `securityAccessFailed` counter.
    InvalidKey,
    /// The client has failed security access more times than the server's security strategy
    /// allows (`0x36`, Table A.1).
    ExceedNumberOfAttempts,
    /// The client tried security access again before the server's required delay had
    /// elapsed (`0x37`, Table A.1).
    RequiredTimeDelayNotExpired,
    /// The request must arrive over secured transmission (`0x38`, Table A.1).
    SecureDataTransmissionRequired,
    /// The request may not arrive over secured transmission (`0x39`, Table A.1).
    SecureDataTransmissionNotAllowed,
    /// The security sub-layer rejected the message (`0x3A`, Table A.1).
    SecureDataVerificationFailed,
    /// Certificate verification failed: the server's date and time are outside the
    /// certificate's validity period (`0x50`).
    CertificateVerificationFailedInvalidTimePeriod,
    /// Certificate verification failed: its signature could not be verified (`0x51`).
    CertificateVerificationFailedInvalidSignature,
    /// Certificate verification failed: it could not be verified against the issuing
    /// authority (`0x52`).
    CertificateVerificationFailedInvalidChainOfTrust,
    /// Certificate verification failed: it does not match the requested use (`0x53`).
    CertificateVerificationFailedInvalidType,
    /// Certificate verification failed: its format is invalid (`0x54`).
    CertificateVerificationFailedInvalidFormat,
    /// Certificate verification failed: its content is invalid (`0x55`).
    CertificateVerificationFailedInvalidContent,
    /// Certificate verification failed: its scope does not match the server (`0x56`).
    CertificateVerificationFailedInvalidScope,
    /// Certificate verification failed: the certificate is revoked (`0x57`).
    CertificateVerificationFailedInvalidCertificate,
    /// The ownership verification failed (`0x58`).
    OwnershipVerificationFailed,
    /// Authentication could not compute its challenge (`0x59`, Table A.1).
    ChallengeCalculationFailed,
    /// The access rights could not be set (`0x5A`).
    SettingAccessRightsFailed,
    /// The session key could not be created or derived (`0x5B`).
    SessionKeyCreationDerivationFailed,
    /// The configuration data could not be used (`0x5C`).
    ConfigurationDataUsageFailed,
    /// De-authentication failed (`0x5D`).
    DeAuthenticationFailed,
    /// A fault prevents the upload or download to the server's memory (`0x70`, Table A.1).
    UploadDownloadNotAccepted,
    /// A fault halted the data transfer, and the active `TransferData` sequence is aborted
    /// (`0x71`, Table A.1).
    TransferDataSuspended,
    /// Erasing or programming the server's non-volatile memory, such as flash, failed
    /// (`0x72`, Table A.1).
    GeneralProgrammingFailure,
    /// The `BlockSequenceCounter` is out of sequence (`0x73`, Table A.1). A repeated
    /// `TransferDataRequest` carrying the previous request's counter is accepted, not
    /// answered with this code.
    WrongBlockSequenceCounter,
    /// The request was received correctly and its parameters are valid, but the action is
    /// unfinished and the server cannot yet take another request (`0x78`, Table A.1). The
    /// server may send it repeatedly, each one extending the client's timeout to P2\*, and
    /// once it has sent one it sends a final response, positive or negative, even for a
    /// request that suppressed its positive response. Every service supports it, so no
    /// service's `allowed_nack_codes` lists it.
    RequestCorrectlyReceivedResponsePending,
    /// The sub-function is supported, but not in the active session (`0x7E`, Table A.1);
    /// one no session supports is answered
    /// [`SubFunctionNotSupported`](Self::SubFunctionNotSupported) instead, which a server
    /// may also use in the programming session. Every service with a sub-function supports
    /// it unless a data link document says otherwise, so no service's own list of response
    /// codes names it.
    SubFunctionNotSupportedInActiveSession,
    /// The service is supported, but not in the active session (`0x7F`, Table A.1); one no
    /// session supports is answered [`ServiceNotSupported`](Self::ServiceNotSupported)
    /// instead. Every service supports it unless a data link document says otherwise, so no
    /// service's own list of response codes names it.
    ServiceNotSupportedInActiveSession,
    /// Engine speed is above the maximum the action allows (`0x81`, Table A.1).
    RpmTooHigh,
    /// Engine speed is below the minimum the action requires (`0x82`, Table A.1).
    RpmTooLow,
    /// The engine is running, and the action, such as an actuator test, needs it stopped
    /// (`0x83`, Table A.1). Distinct from [`RpmTooHigh`](Self::RpmTooHigh).
    EngineIsRunning,
    /// The engine is not running, and the action, such as an actuator test, needs it
    /// running (`0x84`, Table A.1). Distinct from [`RpmTooLow`](Self::RpmTooLow).
    EngineIsNotRunning,
    /// The engine has not run for as long as the action requires (`0x85`, Table A.1).
    EngineRunTimeTooLow,
    /// The temperature is above the maximum the action allows (`0x86`, Table A.1).
    TemperatureTooHigh,
    /// The temperature is below the minimum the action requires (`0x87`, Table A.1).
    TemperatureTooLow,
    /// Vehicle speed is above the maximum the action allows (`0x88`, Table A.1).
    VehicleSpeedTooHigh,
    /// Vehicle speed is below the minimum the action requires (`0x89`, Table A.1).
    VehicleSpeedTooLow,
    /// The throttle or accelerator pedal position is above the maximum the action allows
    /// (`0x8A`, Table A.1).
    ThrottleOrPedalTooHigh,
    /// The throttle or accelerator pedal position is below the minimum the action requires
    /// (`0x8B`, Table A.1).
    ThrottleOrPedalTooLow,
    /// The transmission is not in neutral, which the action requires (`0x8C`, Table A.1).
    TransmissionRangeNotInNeutral,
    /// The transmission is not in gear, which the action requires (`0x8D`, Table A.1).
    TransmissionRangeNotInGear,
    /// The brake switch is not closed. Some tests require it closed, for safety, from
    /// before they start until they end (`0x8F`, Table A.1).
    BrakeSwitchNotClosed,
    /// The shift lever is not in park. Some tests require it in park, for safety, from
    /// before they start until they end (`0x90`, Table A.1).
    ShifterLeverNotInPark,
    /// The torque converter clutch is locked, or its status is above the limit the action
    /// allows (`0x91`, Table A.1).
    TorqueConverterClutchLocked,
    /// The voltage at the server's primary supply pin is above the maximum the action
    /// allows (`0x92`, Table A.1).
    VoltageTooHigh,
    /// The voltage at the server's primary supply pin is below the minimum the action
    /// requires (`0x93`, Table A.1).
    VoltageTooLow,
    /// A resource the action needs is briefly unavailable; the client may retry
    /// (`0x94`, Table A.1). Any service may answer it, as any may answer response-pending.
    ResourceTemporarilyNotAvailable,
    /// A byte Table A.1 reserves for specific conditions not correct, `0x95`–`0xEF`.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    ReservedForSpecificConditionsNotCorrect(u8),
    /// A vehicle-manufacturer specific condition not correct, `0xF0`–`0xFE`.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    VehicleManufacturerSpecificConditionsNotCorrect(u8),
}

impl NegativeResponseCode {
    /// The raw NRC byte.
    ///
    /// `const`, unlike `u8::from(nrc)`, so a server dispatch table built in a `const`
    /// context can read the code back out.
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub const fn value(&self) -> u8 {
        match self {
            Self::PositiveResponse => 0x00,
            Self::IsoSaeReserved(value) => *value,
            Self::GeneralReject => 0x10,
            Self::ServiceNotSupported => 0x11,
            Self::SubFunctionNotSupported => 0x12,
            Self::IncorrectMessageLengthOrInvalidFormat => 0x13,
            Self::ResponseTooLong => 0x14,
            Self::BusyRepeatRequest => 0x21,
            Self::ConditionsNotCorrect => 0x22,
            Self::RequestSequenceError => 0x24,
            Self::NoResponseFromSubnetComponent => 0x25,
            Self::FailurePreventsExecutionOfRequestedAction => 0x26,
            Self::RequestOutOfRange => 0x31,
            Self::SecurityAccessDenied => 0x33,
            Self::AuthenticationRequired => 0x34,
            Self::InvalidKey => 0x35,
            Self::ExceedNumberOfAttempts => 0x36,
            Self::RequiredTimeDelayNotExpired => 0x37,
            Self::SecureDataTransmissionRequired => 0x38,
            Self::SecureDataTransmissionNotAllowed => 0x39,
            Self::SecureDataVerificationFailed => 0x3A,
            Self::CertificateVerificationFailedInvalidTimePeriod => 0x50,
            Self::CertificateVerificationFailedInvalidSignature => 0x51,
            Self::CertificateVerificationFailedInvalidChainOfTrust => 0x52,
            Self::CertificateVerificationFailedInvalidType => 0x53,
            Self::CertificateVerificationFailedInvalidFormat => 0x54,
            Self::CertificateVerificationFailedInvalidContent => 0x55,
            Self::CertificateVerificationFailedInvalidScope => 0x56,
            Self::CertificateVerificationFailedInvalidCertificate => 0x57,
            Self::OwnershipVerificationFailed => 0x58,
            Self::ChallengeCalculationFailed => 0x59,
            Self::SettingAccessRightsFailed => 0x5A,
            Self::SessionKeyCreationDerivationFailed => 0x5B,
            Self::ConfigurationDataUsageFailed => 0x5C,
            Self::DeAuthenticationFailed => 0x5D,
            Self::UploadDownloadNotAccepted => 0x70,
            Self::TransferDataSuspended => 0x71,
            Self::GeneralProgrammingFailure => 0x72,
            Self::WrongBlockSequenceCounter => 0x73,
            Self::RequestCorrectlyReceivedResponsePending => 0x78,
            Self::SubFunctionNotSupportedInActiveSession => 0x7E,
            Self::ServiceNotSupportedInActiveSession => 0x7F,
            Self::RpmTooHigh => 0x81,
            Self::RpmTooLow => 0x82,
            Self::EngineIsRunning => 0x83,
            Self::EngineIsNotRunning => 0x84,
            Self::EngineRunTimeTooLow => 0x85,
            Self::TemperatureTooHigh => 0x86,
            Self::TemperatureTooLow => 0x87,
            Self::VehicleSpeedTooHigh => 0x88,
            Self::VehicleSpeedTooLow => 0x89,
            Self::ThrottleOrPedalTooHigh => 0x8A,
            Self::ThrottleOrPedalTooLow => 0x8B,
            Self::TransmissionRangeNotInNeutral => 0x8C,
            Self::TransmissionRangeNotInGear => 0x8D,
            Self::BrakeSwitchNotClosed => 0x8F,
            Self::ShifterLeverNotInPark => 0x90,
            Self::TorqueConverterClutchLocked => 0x91,
            Self::VoltageTooHigh => 0x92,
            Self::VoltageTooLow => 0x93,
            Self::ResourceTemporarilyNotAvailable => 0x94,
            Self::ReservedForSpecificConditionsNotCorrect(value)
            | Self::VehicleManufacturerSpecificConditionsNotCorrect(value) => *value,
        }
    }
}

impl From<NegativeResponseCode> for u8 {
    fn from(value: NegativeResponseCode) -> Self {
        value.value()
    }
}

impl From<u8> for NegativeResponseCode {
    #[allow(clippy::match_same_arms)]
    fn from(value: u8) -> Self {
        match value {
            0x00 => Self::PositiveResponse,
            0x01..=0x0F => Self::IsoSaeReserved(value),
            0x10 => Self::GeneralReject,
            0x11 => Self::ServiceNotSupported,
            0x12 => Self::SubFunctionNotSupported,
            0x13 => Self::IncorrectMessageLengthOrInvalidFormat,
            0x14 => Self::ResponseTooLong,
            0x15..=0x20 => Self::IsoSaeReserved(value),
            0x21 => Self::BusyRepeatRequest,
            0x22 => Self::ConditionsNotCorrect,
            0x23 => Self::IsoSaeReserved(value),
            0x24 => Self::RequestSequenceError,
            0x25 => Self::NoResponseFromSubnetComponent,
            0x26 => Self::FailurePreventsExecutionOfRequestedAction,
            0x27..=0x30 => Self::IsoSaeReserved(value),
            0x31 => Self::RequestOutOfRange,
            0x32 => Self::IsoSaeReserved(value),
            0x33 => Self::SecurityAccessDenied,
            0x34 => Self::AuthenticationRequired,
            0x35 => Self::InvalidKey,
            0x36 => Self::ExceedNumberOfAttempts,
            0x37 => Self::RequiredTimeDelayNotExpired,
            0x38 => Self::SecureDataTransmissionRequired,
            0x39 => Self::SecureDataTransmissionNotAllowed,
            0x3A => Self::SecureDataVerificationFailed,
            0x3B..=0x4F => Self::IsoSaeReserved(value),
            0x50 => Self::CertificateVerificationFailedInvalidTimePeriod,
            0x51 => Self::CertificateVerificationFailedInvalidSignature,
            0x52 => Self::CertificateVerificationFailedInvalidChainOfTrust,
            0x53 => Self::CertificateVerificationFailedInvalidType,
            0x54 => Self::CertificateVerificationFailedInvalidFormat,
            0x55 => Self::CertificateVerificationFailedInvalidContent,
            0x56 => Self::CertificateVerificationFailedInvalidScope,
            0x57 => Self::CertificateVerificationFailedInvalidCertificate,
            0x58 => Self::OwnershipVerificationFailed,
            0x59 => Self::ChallengeCalculationFailed,
            0x5A => Self::SettingAccessRightsFailed,
            0x5B => Self::SessionKeyCreationDerivationFailed,
            0x5C => Self::ConfigurationDataUsageFailed,
            0x5D => Self::DeAuthenticationFailed,
            0x5E..=0x6F => Self::IsoSaeReserved(value),
            0x70 => Self::UploadDownloadNotAccepted,
            0x71 => Self::TransferDataSuspended,
            0x72 => Self::GeneralProgrammingFailure,
            0x73 => Self::WrongBlockSequenceCounter,
            0x74..=0x77 => Self::IsoSaeReserved(value),
            0x78 => Self::RequestCorrectlyReceivedResponsePending,
            0x79..=0x7D => Self::IsoSaeReserved(value),
            0x7E => Self::SubFunctionNotSupportedInActiveSession,
            0x7F => Self::ServiceNotSupportedInActiveSession,
            0x80 => Self::IsoSaeReserved(value),
            0x81 => Self::RpmTooHigh,
            0x82 => Self::RpmTooLow,
            0x83 => Self::EngineIsRunning,
            0x84 => Self::EngineIsNotRunning,
            0x85 => Self::EngineRunTimeTooLow,
            0x86 => Self::TemperatureTooHigh,
            0x87 => Self::TemperatureTooLow,
            0x88 => Self::VehicleSpeedTooHigh,
            0x89 => Self::VehicleSpeedTooLow,
            0x8A => Self::ThrottleOrPedalTooHigh,
            0x8B => Self::ThrottleOrPedalTooLow,
            0x8C => Self::TransmissionRangeNotInNeutral,
            0x8D => Self::TransmissionRangeNotInGear,
            0x8E => Self::IsoSaeReserved(value),
            0x8F => Self::BrakeSwitchNotClosed,
            0x90 => Self::ShifterLeverNotInPark,
            0x91 => Self::TorqueConverterClutchLocked,
            0x92 => Self::VoltageTooHigh,
            0x93 => Self::VoltageTooLow,
            0x94 => Self::ResourceTemporarilyNotAvailable,
            0x95..=0xEF => Self::ReservedForSpecificConditionsNotCorrect(value),
            0xF0..=0xFE => Self::VehicleManufacturerSpecificConditionsNotCorrect(value),
            0xFF => Self::IsoSaeReserved(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_negative_response_code_roundtrip(byte in any::<u8>()) {
            let code = NegativeResponseCode::from(byte);
            let back: u8 = code.into();
            prop_assert_eq!(byte, back);
        }
    }

    /// ISO 14229-1:2020 Table A.1's named codes, by byte. Spelled out from the table
    /// rather than from `From<u8>`, so the two cannot drift together.
    const NAMED: [u8; 59] = [
        0x00, 0x10, 0x11, 0x12, 0x13, 0x14, 0x21, 0x22, 0x24, 0x25, 0x26, 0x31, 0x33, 0x34,
        0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57,
        0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x70, 0x71, 0x72, 0x73, 0x78, 0x7E, 0x7F, 0x81,
        0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8F, 0x90,
        0x91, 0x92, 0x93,
    ];

    /// Every byte lands where Table A.1 puts it: a named code, an `ISOSAEReserved` byte,
    /// `0x95`–`0xEF` or `0xF0`–`0xFE`; `0x94` is named too.
    #[test]
    fn every_byte_classifies_as_table_a1_does() {
        use NegativeResponseCode as Nrc;
        for byte in 0..=u8::MAX {
            let code = Nrc::from(byte);
            let named = NAMED.contains(&byte) || byte == 0x94;
            match code {
                Nrc::IsoSaeReserved { .. } => {
                    assert!(!named && !(0x95..=0xFE).contains(&byte), "{byte:#04X}");
                }
                Nrc::ReservedForSpecificConditionsNotCorrect { .. } => {
                    assert!((0x95..=0xEF).contains(&byte), "{byte:#04X}");
                }
                Nrc::VehicleManufacturerSpecificConditionsNotCorrect { .. } => {
                    assert!((0xF0..=0xFE).contains(&byte), "{byte:#04X}");
                }
                _ => assert!(named, "{byte:#04X} is not named in Table A.1"),
            }
        }
    }
}
