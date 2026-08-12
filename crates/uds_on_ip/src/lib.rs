//! # UDS on IP
//!
//! This crate provides session management for UDS (Unified Diagnostic Services) communication
//! over DoIP (Diagnostics over IP) transport. It serves as the bridge layer between the
//! protocol-agnostic [`uds_protocol`] crate and the transport-layer [`simple_doip`] crate.
//!
//! ## Purpose
//!
//! The UDS protocol (ISO 14229) defines diagnostic services but is transport-agnostic.
//! The DoIP protocol (ISO 13400) provides IP-based transport but has no knowledge of UDS semantics.
//! This crate bridges the gap by:
//!
//! - Managing UDS diagnostic sessions over DoIP connections
//! - Handling tester present keepalive to maintain sessions
//! - Routing responses to the appropriate pending requests
//! - Managing connection lifecycle (activation, deactivation, reconnection)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │         Application Code            │
//! │   (sends UDS requests/responses)    │
//! └──────────────┬──────────────────────┘
//!                │
//!                ▼
//! ┌─────────────────────────────────────┐
//! │           uds_on_ip                 │
//! │   - Session management              │
//! │   - Tester present keepalive        │
//! │   - Request/response correlation    │
//! │   - Connection lifecycle            │
//! └──────────────┬──────────────────────┘
//!                │
//!                ▼
//! ┌─────────────────────────────────────┐
//! │          simple_doip                │
//! │   - DoIP message framing            │
//! │   - TCP/UDP transport               │
//! │   - Routing activation              │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use uds_on_ip::UdsClient;
//! use uds_protocol::DiagnosticSessionControl;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to sensor
//!     let client = UdsClient::connect("192.168.1.100:13400").await?;
//!
//!     // Send UDS request - session management handled automatically
//!     let response = client.send(DiagnosticSessionControl::extended_session()).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod error;
pub mod request_sender;
pub mod session;

pub use client::{UdsClient, UdsClientOptions};
pub use error::{Error, routing_denial_cause};
pub use request_sender::RequestSender;
pub use session::SessionConfig;
pub use simple_doip::client::AddressType;
// Re-exported so a downstream crate's *production* code can name
// `Connector`/`ConnectorSocket` — e.g. to declare its own client type generic
// over the connector, defaulted to `ConnectorSocket`, the way `UdsClient<Conn
// = ConnectorSocket>` itself is — without needing `simple_doip` in its own
// `[dependencies]`. (A downstream crate's *tests* may still reach for
// `simple_doip` directly as a dev-dependency, e.g. to build a custom
// `Connector` without `ConnectorSocket`'s hardcoded port-13400 check, mirroring
// `uds_on_ip/tests/tester_present_during_pending.rs`; that's a test-only
// concern this re-export doesn't need to solve.)
pub use simple_doip::connection::{Connector, ConnectorSocket};

/// Result type for UDS on IP operations.
pub type Result<T> = std::result::Result<T, Error>;
