# UDS on IP

> **Status: prototype, mid-refactor.** This README describes the superseded
> implementation retired to `legacy/`. The crate is now a transport below the
> session layer, with no driver and no runtime dependency — see the crate
> documentation (`cargo doc --open`) for what it actually provides today.

This crate provides UDS (Unified Diagnostic Services) session management over DoIP (Diagnostics over IP) transport. It serves as the bridge layer between the protocol definition crate [`uds_protocol`](https://github.com/luminartech/uds_protocol) and the transport layer crate [`simple_doip`](https://github.com/luminartech/simple_doip).

## Motivation

The automotive diagnostics stack has a natural layered architecture:

| Layer | Crate | Responsibility |
|-------|-------|----------------|
| Protocol | `uds_protocol` | UDS message encoding/decoding (ISO 14229) |
| **Session** | **`uds_on_ip`** | **Session management, keepalive, routing** |
| Transport | `simple_doip` | DoIP framing and TCP/UDP transport (ISO 13400) |

Without this intermediate layer, application code must handle:
- Connection lifecycle (routing activation/deactivation)
- Session keepalive (tester present messages)
- Response timeout handling (P2/P2* timers)
- Response pending (NRC 0x78) management
- Request/response correlation

This leads to duplicated logic across applications and tight coupling between the protocol and transport layers.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Code                         │
│              (diagnostic tools, test harnesses)             │
└──────────────────────────┬──────────────────────────────────┘
                           │ UDS Requests/Responses
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                       uds_on_ip                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Session Manager                        │    │
│  │  - Tracks diagnostic session state                  │    │
│  │  - Manages security access level                    │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Keepalive Service                      │    │
│  │  - Sends periodic TesterPresent (0x3E)              │    │
│  │  - Resets timer on any diagnostic activity          │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Response Handler                       │    │
│  │  - Correlates responses to pending requests         │    │
│  │  - Handles NRC 0x78 (response pending)              │    │
│  │  - Manages P2/P2* timeouts                          │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Connection Manager                     │    │
│  │  - DoIP routing activation                          │    │
│  │  - Reconnection on connection loss                  │    │
│  │  - Logical address management                       │    │
│  └─────────────────────────────────────────────────────┘    │
└──────────────────────────┬──────────────────────────────────┘
                           │ DoIP Diagnostic Messages
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                      simple_doip                            │
│  - DoIP message framing (ISO 13400-2)                       │
│  - TCP connection management                                │
│  - UDP vehicle discovery                                    │
└─────────────────────────────────────────────────────────────┘
```

## Key Features

### Session Management

Tracks the current diagnostic session type and handles session transitions:

- Default Session (0x01)
- Programming Session (0x02)
- Extended Diagnostic Session (0x03)
- Vendor-specific sessions

### Tester Present Keepalive

UDS sessions timeout if no diagnostic activity occurs. This crate automatically sends `TesterPresent` (SID 0x3E) messages at configurable intervals to keep sessions alive. The keepalive timer resets on any diagnostic activity.

### Response Timeout Handling

Implements the P2 and P2* timing parameters from ISO 14229:

- **P2 Server Max**: Maximum time for initial response (default 50ms, extended to 5s for robustness)
- **P2* Server Max**: Extended timeout after NRC 0x78 response pending (default 5s, can be 25s+)

When the ECU responds with NRC 0x78 (Response Pending), the crate automatically extends the timeout and waits for the final response.

### Request/Response Correlation

DoIP connections can have multiple outstanding requests. This crate tracks pending requests and routes responses to the correct caller based on service ID matching.

## Usage

```rust
use std::net::IpAddr;
use uds_on_ip::{UdsClient, UdsClientOptions, SessionConfig};
use uds_protocol::Response;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure session behavior
    let session_config = SessionConfig::new()
        .with_tester_present_interval(std::time::Duration::from_secs(2))
        .with_response_timeout(std::time::Duration::from_secs(5));

    // Configure connection options
    let options = UdsClientOptions::new([192, 168, 1, 100].into())
        .with_server_logical_address(0x0001)
        .with_routing_activation(true)
        .with_session_config(session_config);

    // Connect to sensor - handles DoIP routing activation
    let client = UdsClient::connect(options).await?;

    // Enter extended diagnostic session
    let response = client.enter_extended_session().await?;

    // Extract timing parameters from response
    if let Response::DiagnosticSessionControl(session) = response {
        println!("P2 Server Max: {} ms", session.p2_server_max);
        println!("P2* Server Max: {} ms", session.p2_star_server_max * 10);
    }

    // Send tester present to verify session
    client.tester_present(true).await?;

    // Clean shutdown
    client.shutdown().await;
    Ok(())
}
```

### Running the Example

A complete example is provided that demonstrates connecting to a server and entering an extended diagnostic session:

```bash
# Run against localhost (requires a DoIP server running)
cargo run --example extended_session

# Run against a specific IP
cargo run --example extended_session -- 192.168.1.100
```

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `tester_present_interval` | 2s | How often to send keepalive messages |
| `response_timeout` | 5s | Initial response timeout (P2 max) |
| `response_pending_timeout` | 25s | Extended timeout after NRC 0x78 (P2* max) |
| `auto_tester_present` | true | Whether to automatically send keepalives |

## Recent behavior changes

If you have downstream code built against an older `uds_on_ip`, these are
the behavior changes worth knowing about:

- **TesterPresent suppressed during NRC 0x78.** Once the ECU returns NRC
  0x78 (response pending), the in-loop tester-present is held off until
  the response cycle completes. This prevents the rare ECU
  configurations that NACK a TP mid-pending (observed during app→FBL
  transitions) from corrupting the in-flight request.
- **P2\* applied after NRC 0x78.** The session switches from the regular
  `response_timeout` (P2) to `response_pending_timeout` (P2\*) once a
  pending response arrives, instead of timing out at the original P2.
- **Initial-send reconnect returns `ReconnectedWithoutResponse`.** A
  reconnect that happens before the first response is no longer reported
  as a generic timeout — callers can distinguish "reconnected but the
  request was never answered" from "request was sent and timed out."
- **Timeout-path re-send restored after reconnect.** If a reconnect
  happens during the wait, the request is automatically re-sent on the
  new connection rather than dropped silently.
- **2-byte NACKs rejected as malformed.** Truncated NACKs no longer
  decode into bogus negative responses; they surface as a protocol error.
- **Stray cross-SID responses ignored on reconnect.** If a stale response
  for a previous SID arrives after reconnect, it's discarded and the
  current request is re-sent immediately.
- **Post-reconnect TP gated on keepalive-active.** After a reconnect, the
  tester-present pump only resumes if keepalive was active before — it
  doesn't start sending TPs on a session that wasn't using them.

## Dependencies

- [`uds_protocol`](https://github.com/luminartech/uds_protocol) - UDS message types and encoding
- [`simple_doip`](https://github.com/luminartech/simple_doip) - DoIP transport layer

## Standards References

- ISO 14229-1:2020 - Unified Diagnostic Services (UDS)
- ISO 14229-2:2021 - Session layer services
- ISO 13400-2:2019 - DoIP transport protocol
