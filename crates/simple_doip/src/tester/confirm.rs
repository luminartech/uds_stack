use crate::messages::{DiagnosticNackCode, NackCode};
use crate::service::DoIpResult;

/// ISO 13400-2:2019 Table 26 against 8.2.5.
pub(super) fn from_diagnostic_nack(code: DiagnosticNackCode) -> DoIpResult {
    match code {
        DiagnosticNackCode::InvalidSourceAddress => DoIpResult::InvalidSa,
        DiagnosticNackCode::UnknownTargetAddress => DoIpResult::UnknownTa,
        DiagnosticNackCode::DiagnosticMessageTooLarge => DoIpResult::MessageTooLarge,
        DiagnosticNackCode::OutOfMemory => DoIpResult::OutOfMemory,
        DiagnosticNackCode::TargetUnreachable => DoIpResult::TargetUnreachable,
        DiagnosticNackCode::UnknownNetwork
        | DiagnosticNackCode::TransportProtocolError
        | DiagnosticNackCode::Reserved(_) => DoIpResult::Error,
    }
}

/// ISO 13400-2:2019 Table 19 against 8.2.5.
pub(super) fn from_header_nack(code: NackCode) -> DoIpResult {
    match code {
        NackCode::IncorrectPatternFormat
        | NackCode::UnknownPayloadType
        | NackCode::InvalidPayloadLength => DoIpResult::HdrError,
        NackCode::MessageTooLarge => DoIpResult::MessageTooLarge,
        NackCode::OutOfMemory => DoIpResult::OutOfMemory,
        NackCode::Reserved(_) => DoIpResult::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_diagnostic_nack_code_maps_to_its_doip_result() {
        for (code, result) in [
            (0x00, DoIpResult::Error),
            (0x01, DoIpResult::Error),
            (0x02, DoIpResult::InvalidSa),
            (0x03, DoIpResult::UnknownTa),
            (0x04, DoIpResult::MessageTooLarge),
            (0x05, DoIpResult::OutOfMemory),
            (0x06, DoIpResult::TargetUnreachable),
            (0x07, DoIpResult::Error),
            (0x08, DoIpResult::Error),
            (0x09, DoIpResult::Error),
            (0xFF, DoIpResult::Error),
        ] {
            assert_eq!(
                from_diagnostic_nack(DiagnosticNackCode::from(code)),
                result,
                "{code:#04X}"
            );
        }
    }

    #[test]
    fn each_header_nack_code_maps_to_its_doip_result() {
        for (code, result) in [
            (0x00, DoIpResult::HdrError),
            (0x01, DoIpResult::HdrError),
            (0x02, DoIpResult::MessageTooLarge),
            (0x03, DoIpResult::OutOfMemory),
            (0x04, DoIpResult::HdrError),
            (0x05, DoIpResult::Error),
            (0xFF, DoIpResult::Error),
        ] {
            assert_eq!(
                from_header_nack(NackCode::from(code)),
                result,
                "{code:#04X}"
            );
        }
    }
}
