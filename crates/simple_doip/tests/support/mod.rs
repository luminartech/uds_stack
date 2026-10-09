//! Test support shared by the tester and entity tests.

pub mod mock_stack;

use simple_doip::service::{EntityConfig, TesterAddress};

/// The sensor's configuration: routing activation from [`mock_stack::TESTER`] only.
#[allow(
    dead_code,
    reason = "each test binary that includes this uses a different part"
)]
pub fn the_tester() -> EntityConfig {
    EntityConfig::new([TesterAddress::new(mock_stack::TESTER).unwrap()])
}
