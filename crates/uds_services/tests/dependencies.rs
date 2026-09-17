//! ``UDSSVC_ARCH_0002`` names exactly three dependencies; ``UDSSVC_ARCH_0003`` and
//! ``UDSSVC_ARCH_0030`` say what must never appear. Asserted rather than reviewed.

/// All three resolve and are usable.
#[test]
fn the_three_dependencies_are_present() {
    fn assert_sink<S: automotive_wire_codec::Sink>() {}
    let _ = uds_session::Timestamp(0);
    let _ = uds_protocol::UdsServiceType::TesterPresent;
    assert_sink::<automotive_wire_codec::CountingSink>();
}
