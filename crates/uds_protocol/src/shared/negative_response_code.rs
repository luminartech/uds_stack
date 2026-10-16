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
    /// This response code shall not be used in a negative response message.
    /// This positiveResponse parameter value is reserved for server internal implementation
    PositiveResponse,
    /// A byte Table A.1 reserves for future definition: `0x01`–`0x0F`, `0x15`–`0x20`,
    /// `0x23`, `0x27`–`0x30`, `0x32`, `0x3B`–`0x4F`, `0x5E`–`0x6F`, `0x74`–`0x77`,
    /// `0x79`–`0x7D`, `0x80`, `0x8E` and `0xFF`.
    #[cfg_attr(feature = "clap", clap(skip))]
    #[non_exhaustive]
    IsoSaeReserved(u8),
    /// This response code indicates that the requested action has been rejected by the
    /// server. The `GeneralReject` response code shall only be implemented in the server if
    /// none of the negative response codes meet the needs of the implementation. This
    /// response code shall not be used as a general replacement for other response codes
    /// defined.
    GeneralReject,
    /// This response code indicates that the requested action will not be taken because the
    /// server does not support the requested service. The server shall send this response
    /// code in the case where the client has sent a request message with a service
    /// identifier which is either unknown or not supported. This negative response code is
    /// not shown in the list of negative response codes to be supported for a diagnostic
    /// service, because this negative response code is not applicable for supported
    /// services.
    ServiceNotSupported,
    /// This response code indicates that the requested action will not be taken because the
    /// server does not support the service specific parameters of the request message. The
    /// server shall send this response code in case the client has sent a request message
    /// with a known and supported service identifier but with "sub function“ which is
    /// either unknown or not supported.
    SubFunctionNotSupported,
    /// This response code indicates that the requested action will not be taken because the
    /// length of the received request message does not match the prescribed length for the
    /// specified service, or that the format of the parameters do not match the prescribed
    /// format for the specified service.
    IncorrectMessageLengthOrInvalidFormat,
    /// This response code shall be reported by the server if the response to be generated
    /// exceeds the maximum number of bytes available by the underlying network layer.
    ResponseTooLong,
    /// This response code indicates that the server is temporarily too busy to perform the
    /// requested operation. In this circumstance the client shall perform repetition of the
    /// request message or "try again later". The repetition of the request shall be delayed
    /// by a time specified in the respective implementation documents.
    BusyRepeatRequest,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite conditions are not met.
    ConditionsNotCorrect,
    /// This response code indicates that the requested action will not be taken because the
    /// server expects a different sequence of request messages or message as sent by the
    /// client. This may occur when sequence sensitive requests are issued in the wrong
    /// order.
    RequestSequenceError,
    /// A subnet component the action needed did not answer in time (`0x25`, Table A.1).
    NoResponseFromSubnetComponent,
    /// A fault the server holds as a DTC prevents the action (`0x26`, Table A.1).
    FailurePreventsExecutionOfRequestedAction,
    /// This response code indicates that the requested action will not be taken because the
    /// server has detected that the request message contains a parameter which attempts to
    /// substitute a value beyond its range of authority (e.g. attempting to substitute a
    /// data byte of 111 when the data is only defined to 100), or which attempts to access
    /// a dataIdentifier/routineIdentifer that is not supported or not supported in active
    /// session. This response code shall be implemented for all services which allow the
    /// client to read data, write data, or adjust functions by data in the server.
    RequestOutOfRange,
    /// This response code indicates that the requested action will not be taken because the
    /// server's security strategy has not been satisfied by the client. The server shall
    /// send this response code if one of the following cases occur:
    /// - the test conditions of the server are not met,
    /// - the required message sequence e.g. `DiagnosticSessionControl`, securityAccess is
    ///   not met,
    /// - the client has sent a request message which requires an unlocked server.
    ///
    /// Beside the mandatory use of this negative response code as specified in the
    /// applicable services within this standard, this negative response code can also be
    /// used for any case where security is required and is not yet granted to perform the
    /// required service.
    SecurityAccessDenied,
    /// This response code indicates that the requested action will not be taken because the
    /// client has insufficient rights based on its Authentication state.
    AuthenticationRequired,
    /// This response code indicates that the server has not given security access because
    /// the key sent by the client did not match with the key in the server's memory. This
    /// counts as an attempt to gain security. The server shall remain locked and increment
    /// its internal securityAccessFailed counter.
    InvalidKey,
    /// This response code indicates that the requested action will not be taken because the
    /// client has unsuccessfully attempted to gain security access more times than the
    /// server's security strategy will allow.
    ExceedNumberOfAttempts,
    /// This response code indicates that the requested action will not be taken because the
    /// client's latest attempt to gain security access was initiated before the server's
    /// required timeout period had elapsed.
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
    /// This response code indicates that an attempt to upload/download to a server's memory
    /// cannot be accomplished due to some fault conditions.
    UploadDownloadNotAccepted,
    /// This response code indicates that a data transfer operation was halted due to some
    /// fault. The active transferData sequence shall be aborted.
    TransferDataSuspended,
    /// This response code indicates that the server detected an error when erasing or
    /// programming a memory location in the permanent memory device (e.g. Flash Memory).
    GeneralProgrammingFailure,
    /// This response code indicates that the server detected an error in the sequence of
    /// `BlockSequenceCounter` values. Note that the repetition of a `TransferDataRequest`
    /// message with a `BlockSequenceCounter` equal to the one included in the previous
    /// `TransferDataRequest` message shall be accepted by the server.
    WrongBlockSequenceCounter,
    /// The request was received correctly and all its parameters were valid, but the server
    /// has not finished the requested action and is not yet ready to accept another
    /// request.
    ///
    /// The server sends a final positive or negative response once the service completes,
    /// and may repeat this code until then. It affects the application-layer timing
    /// parameters (it extends P2\*), and per ISO 14229-1:2020 Annex A.1 the server "shall
    /// always send a final response (positive or negative) independent of the
    /// `suppressPosRspMsgIndicationBit` value" once it has used this code.
    ///
    /// Typically used while writing or erasing flash, when the programming routine cannot
    /// service the bus. Generally supported by every service, which is why it is not listed
    /// in the individual services' `allowed_nack_codes`.
    RequestCorrectlyReceivedResponsePending,
    /// This response code indicates that the requested action will not be taken because the
    /// server does not support the requested sub-function in the session currently active.
    /// Within the programmingSession negative response code 0x12 (subFunctionNotSupported)
    /// may optionally be reported instead of negative response code 0x7F
    /// (subFunctionNotSupportedInActiveSession). This response code shall only be used when
    /// the requested sub-function is known to be supported in another session, otherwise
    /// response code 0x12 (subFunctionNotSupported) shall be used. This response code shall
    /// be supported by each diagnostic service with a sub-function parameter, if not
    /// otherwise stated in the data link specific implementation document, therefore it is
    /// not listed in the list of applicable response codes of the diagnostic services.
    SubFunctionNotSupportedInActiveSession,
    /// This response code indicates that the requested action will not be taken because the
    /// server does not support the requested service in the session currently active. This
    /// response code shall only be used when the requested service is known to be supported
    /// in another session, otherwise response code 0x11 (serviceNotSupported) shall be
    /// used. This response code is in general supported by each diagnostic service, as not
    /// otherwise stated in the data link specific implementation document, therefore it is
    /// not listed in the list of applicable response codes of the diagnostic services.
    ServiceNotSupportedInActiveSession,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for RPM is not met (current RPM is above a
    /// pre-programmed maximum threshold).
    RpmTooHigh,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for RPM is not met (current RPM is below a
    /// pre-programmed minimum threshold).
    RpmTooLow,
    /// This is required for those actuator tests which cannot be actuated while the Engine
    /// is running. This is different from RPM too high negative response and needs to be
    /// allowed.
    EngineIsRunning,
    /// This is required for those actuator tests which cannot be actuated unless the Engine
    /// is running. This is different from RPM too low negative response, and needs to be
    /// allowed.
    EngineIsNotRunning,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for engine run time is not met (current engine run
    /// time is below a preprogrammed limit).
    EngineRunTimeTooLow,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for temperature is not met (current temperature is
    /// above a preprogrammed maximum threshold).
    TemperatureTooHigh,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for temperature is not met (current temperature is
    /// below a preprogrammed minimum threshold).
    TemperatureTooLow,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for vehicle speed is not met (current VS is above a
    /// pre-programmed maximum threshold).
    VehicleSpeedTooHigh,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for vehicle speed is not met (current VS is below a
    /// pre-programmed minimum threshold).
    VehicleSpeedTooLow,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for throttle/pedal position is not met (current TP/APP
    /// is above a preprogrammed maximum threshold).
    ThrottleOrPedalTooHigh,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for throttle/pedal position is not met (current TP/APP
    /// is below a preprogrammed minimum threshold).
    ThrottleOrPedalTooLow,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for being in neutral is not met (current transmission
    /// range is not in neutral).
    TransmissionRangeNotInNeutral,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for being in gear is not met (current transmission
    /// range is not in gear).
    TransmissionRangeNotInGear,
    /// For safety reasons, this is required for certain tests before it begins,
    /// and must be maintained for the entire duration of the test.
    BrakeSwitchNotClosed,
    /// For safety reasons, this is required for certain tests before it begins,
    /// and must be maintained for the entire duration of the test.
    ShifterLeverNotInPark,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for torque converter clutch is not met (current TCC
    /// status above a preprogrammed limit or locked).
    TorqueConverterClutchLocked,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for voltage at the primary pin of the server (ECU) is
    /// not met (current voltage is above a pre-programmed maximum threshold).
    VoltageTooHigh,
    /// This response code indicates that the requested action will not be taken because the
    /// server prerequisite condition for voltage at the primary pin of the server (ECU) is
    /// not met (current voltage is below a pre-programmed maximum threshold).
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
