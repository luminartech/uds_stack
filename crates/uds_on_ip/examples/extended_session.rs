//! Example: Connect to a UDS server and request an extended diagnostic session.
//!
//! This example demonstrates the basic usage of the `uds_on_ip` crate to:
//! 1. Connect to a DoIP server
//! 2. Request an extended diagnostic session
//! 3. Handle the response
//!
//! # Usage
//!
//! ```bash
//! cargo run -p uds_on_ip --example extended_session -- <server_ip>
//! ```
//!
//! If no IP is provided, it defaults to 127.0.0.1 (localhost).

use std::net::IpAddr;
use tracing::{error, info};
use uds_on_ip::{SessionConfig, UdsClient, UdsClientOptions};
use uds_protocol::{Decode, Response};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_max_level(tracing::Level::INFO)
        .init();

    // Parse server IP from command line, default to localhost
    let server_ip: IpAddr = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("Invalid IP address"))
        .unwrap_or_else(|| [192, 168, 10, 150].into());

    info!("UDS Extended Session Example");
    info!("============================");
    info!("Target server: {}", server_ip);

    // Configure the session
    let session_config = SessionConfig::new()
        .with_tester_present_interval(std::time::Duration::from_secs(2))
        .with_response_timeout(std::time::Duration::from_secs(5));

    // Configure the client options
    let options = UdsClientOptions::new(server_ip)
        .with_server_logical_address(0xE400)
        .with_server_physical_address(0x4010)
        .with_client_logical_address(0x0E00)
        .with_routing_activation(true)
        .with_session_config(session_config);

    // Connect to the server
    info!("Connecting to server...");
    let client = match UdsClient::connect(options).await {
        Ok(client) => {
            info!("Successfully connected!");
            client
        }
        Err(e) => {
            error!("Failed to connect: {}", e);
            return Err(e.into());
        }
    };

    // Request extended diagnostic session
    info!("Requesting extended diagnostic session...");
    match client.enter_extended_session().await {
        Ok(response_bytes) => {
            info!("Successfully entered extended diagnostic session!");

            // Decode the owned response bytes caller-side.
            match Response::decode(&response_bytes) {
                Ok((Response::DiagnosticSessionControl(session_response), _)) => {
                    info!("Session Details:");
                    info!("  Session Type: {:?}", session_response.session_type);
                    info!("  P2 Server Max: {} ms", session_response.p2_server_max);
                    info!(
                        "  P2* Server Max: {} ms",
                        session_response.p2_star_server_max * 10
                    );
                }
                Ok((other, _)) => {
                    info!("Unexpected response variant: {:?}", other);
                }
                Err(e) => {
                    error!("Failed to decode session response: {}", e);
                }
            }
        }
        Err(e) => {
            error!("Failed to enter extended session: {}", e);
            client.shutdown().await;
            return Err(e.into());
        }
    }

    // Optionally send a tester present to verify the session is active
    info!("Sending tester present...");
    match client.tester_present(false).await {
        Ok(Some(_)) => info!("Tester present acknowledged"),
        Ok(None) => info!("Tester present sent (response suppressed)"),
        Err(e) => error!("Tester present failed: {}", e),
    }

    // Return to default session before disconnecting
    info!("Returning to default session...");
    match client.enter_default_session().await {
        Ok(_) => info!("Returned to default session"),
        Err(e) => error!("Failed to return to default session: {}", e),
    }

    // Clean shutdown
    info!("Disconnecting...");
    client.shutdown().await;
    info!("Done!");

    Ok(())
}
