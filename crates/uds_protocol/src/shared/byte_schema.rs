//! `OpenAPI` schemas for the types that serialize as a single protocol byte or 16-bit
//! identifier.
//!
//! Each such type's `serde` impl goes through the integer (`serde(try_from = "u8", into =
//! "u8")` and the like), so a derived `ToSchema`, which describes the Rust shape, would
//! disagree with what `serde` reads and writes. Each schema here is an integer carrying the
//! accepted range, so a generated client learns the bound from the document rather than
//! from a runtime rejection.

/// Implement `PartialSchema`/`ToSchema` for a type whose serialized form is one integer.
///
/// `max` is the largest byte the type's own `serde` entry point accepts, so the schema and
/// the deserializer cannot drift; `description` is the published schema description and
/// must spell out any gaps, because `minimum`/`maximum` alone cannot express a
/// non-contiguous set.
macro_rules! byte_schema {
    ($($ty:ident { max: $max:expr, description: $desc:literal }),* $(,)?) => {
        $(
            impl utoipa::PartialSchema for crate::$ty {
                fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                    utoipa::openapi::ObjectBuilder::new()
                        .schema_type(utoipa::openapi::schema::SchemaType::Type(
                            utoipa::openapi::Type::Integer,
                        ))
                        .minimum(Some(0.0))
                        .maximum(Some(f64::from($max)))
                        .description(Some($desc))
                        .into()
                }
            }

            impl utoipa::ToSchema for crate::$ty {
                fn name() -> std::borrow::Cow<'static, str> {
                    std::borrow::Cow::Borrowed(stringify!($ty))
                }
            }
        )*
    };
}

byte_schema!(
    AddressAndLengthFormatIdentifier {
        max: 0x88u8,
        description: "An addressAndLengthFormatIdentifier byte: memorySize width in the high \
                      nibble, memoryAddress width in the low nibble, each 1 to 8 bytes. So \
                      0x44 declares four bytes each. ISO 14229-1:2020 Annex H Table H.1 marks \
                      a zero nibble not applicable, and a width above 8 does not fit a 64-bit \
                      value. The accepted set is not contiguous: a byte with either nibble 0 \
                      or above 8, such as 0x40 or 0x19, is rejected."
    },
    CommunicationControlType {
        max: 0x7Fu8,
        description: "A CommunicationControl sub-function byte, 0x00 to 0x7F. Bit 7 is the \
                      suppressPosRspMsgIndicationBit and is carried separately, so a value with \
                      it set is rejected."
    },
    CommunicationType {
        max: 0x0Fu8,
        description: "The low nibble of a communicationType byte, 0x00 to 0x0F (ISO \
                      14229-1:2020 Annex B Table B.1). 0x01 is normal messages, 0x02 network \
                      management messages and 0x03 both; the other values are reserved. The \
                      high nibble is the subnetNumber, carried separately."
    },
    DataFormatIdentifier {
        max: 0xFFu8,
        description: "A dataFormatIdentifier byte: compression method in the high nibble, \
                      encryption method in the low nibble. 0x00 means neither. Every byte is a \
                      valid encoding; non-zero values are vehicle-manufacturer specific."
    },
    DiagnosticSessionType {
        max: 0x7Fu8,
        description: "A diagnosticSessionType byte, 0x00 to 0x7F (ISO 14229-1:2020 Table 24). \
                      Bit 7 is the suppressPosRspMsgIndicationBit and is carried separately, so \
                      a value with it set is rejected."
    },
    DtcExtDataRecordNumber {
        max: 0xFFu8,
        description: "A DTCExtDataRecordNumber byte (ISO 14229-1:2020 Table D.16). Every byte \
                      classifies: 0x01 to 0x8F vehicle-manufacturer specific, 0x90 to 0x9F \
                      regulated emissions OBD, 0xA0 to 0xEF regulated, 0xFE all regulated \
                      emissions OBD, 0xFF all, and the rest ISOSAEReserved."
    },
    DtcFormatIdentifier {
        max: 0xFFu8,
        description: "A DTCFormatIdentifier byte (ISO 14229-1:2020 Table 331). 0x00 to 0x04 are \
                      defined formats; every other byte classifies as ISOSAEReserved rather than \
                      being rejected."
    },
    DtcSettingType {
        max: 0x7Fu8,
        description: "A DTCSettingType byte, 0x00 to 0x7F (ISO 14229-1:2020 Table 128): 0x01 \
                      on, 0x02 off, 0x40 to 0x5F vehicle-manufacturer specific, 0x60 to 0x7E \
                      system-supplier specific, and the rest ISOSAEReserved. Bit 7 is the \
                      suppressPosRspMsgIndicationBit and is carried separately, so a value \
                      with it set is rejected."
    },
    DtcSnapshotRecordNumber {
        max: 0xFFu8,
        description: "A DTCSnapshotRecordNumber byte (ISO 14229-1:2020 Table 318). Every \
                      byte classifies: 0x00 and 0xF0 are reserved, 0xFF requests all \
                      records, and the rest name one record."
    },
    FileOperationMode {
        max: 0xFFu8,
        description: "A RequestFileTransfer modeOfOperation byte (ISO 14229-1:2020 Table 485). \
                      0x01 to 0x06 are defined; every other byte classifies as ISOSAEReserved \
                      rather than being rejected."
    },
    FunctionalGroupIdentifier {
        max: 0xFFu8,
        description: "A functionalGroupIdentifier byte (ISO 14229-1:2020 Annex D). Every byte \
                      classifies: undefined values become ISOSAEReserved rather than being \
                      rejected."
    },
    NegativeResponseCode {
        max: 0xFFu8,
        description: "A negative response code byte (ISO 14229-1:2020 Table A.1). Every byte \
                      classifies: one the table does not name is ISOSAEReserved, \
                      reservedForSpecificConditionsNotCorrect (0x95 to 0xEF) or \
                      vehicleManufacturerSpecificConditionsNotCorrect (0xF0 to 0xFE)."
    },
    ResetType {
        max: 0x7Fu8,
        description: "A resetType byte, 0x00 to 0x7F (ISO 14229-1:2020 Table 34). Bit 7 is the \
                      suppressPosRspMsgIndicationBit and is carried separately, so a value with \
                      it set is rejected."
    },
    SecurityAccessLevel {
        max: 0x41u8,
        description: "A security level, named by its requestSeed sub-function byte (ISO \
                      14229-1:2020 Table 42): an odd value from 0x01 to 0x41. Its sendKey is \
                      that value plus one. The accepted set is not contiguous: even values are \
                      rejected."
    },
    SecurityAccessType {
        max: 0x7Fu8,
        description: "A SecurityAccess sub-function byte, 0x00 to 0x7F. Odd values request a \
                      seed and even values send a key. Bit 7 is the \
                      suppressPosRspMsgIndicationBit and is carried separately, so a value with \
                      it set is rejected."
    },
    SubnetNumber {
        max: 0x0Fu8,
        description: "A subnetNumber nibble, 0x00 to 0x0F (ISO 14229-1:2020 Annex B Table B.1). \
                      0x00 addresses the receiving node including all connected networks, 0x0F \
                      the network the request arrived on, and 0x01 to 0x0E a specific subnet. It \
                      occupies the high nibble of the communicationType byte, so anything wider \
                      than a nibble is rejected."
    },
    UdsIdentifier {
        max: 0xFFFFu16,
        description: "A dataIdentifier (ISO 14229-1:2020 Annex C Table C.1). Every value \
                      classifies into a named identifier or the range it falls in."
    },
    UdsRoutineIdentifier {
        max: 0xFFFFu16,
        description: "A routineIdentifier (ISO 14229-1:2020 Annex F Table F.1). Every value \
                      classifies into a named routine or the range it falls in."
    },
);
