# Migrating to simple_doip 0.7

0.7.0 removes three I/O surfaces: the async client (`client::Client`, the `client`
feature), the async server (`server::Server`, the `server` feature) and
`bare_metal_entity`, which every build had, default features included. 0.6.0 is the last
release that has them.

What replaces them is the `connection` feature: `tester::Tester` and `entity::Entity`,
`no_std` and allocation-free, over an [`edge-nal`](https://docs.rs/edge-nal/0.7) backend
and an [`embassy-time`](https://docs.rs/embassy-time/0.5) clock. The crate documentation
lists what an integrator supplies for them. Above them,
[`uds_on_ip`](https://github.com/luminartech/uds_stack/tree/main/crates/uds_on_ip)
carries `uds_services` clients and servers over DoIP.

The protocol core (`messages`, `try_frame`, `wire`) and the `alloc`, `std` and `codec`
features stay, with breaking changes of their own since 0.6.0: among them, a message
encodes into a `wire::Sink`, and a negative diagnostic message acknowledgement is a
message of its own. [`CHANGELOG.md`](CHANGELOG.md) lists each.

On a host, the examples below take `edge-nal-std` as the backend and `embassy-time` with
its `std` and `generic-queue-8` features as the clock:

```toml
[dependencies]
simple_doip = { version = "0.7", features = ["connection"] }
edge-nal = "0.7"
edge-nal-std = "0.7"
embassy-time = { version = "0.5", features = ["std", "generic-queue-8"] }
```

## `client::Client` → `tester::Tester`

| 0.6                                                                | 0.7                                                                                                                                                                                                             |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Client::connect(ClientOptions)`                                   | `Tester::connect(&stack, remote, sa)`, which connects and activates routing                                                                                                                                     |
| `ClientOptions::server_address`                                    | `remote`, any `SocketAddr`, usually on `TCP_PORT`                                                                                                                                                               |
| `ClientOptions::client_logical_address`                            | `sa`, a `TesterAddress`, which refuses an address outside the tester range                                                                                                                                      |
| `ClientOptions::server_logical_address`, `server_physical_address` | the `ta` of each request                                                                                                                                                                                        |
| `ClientOptions::protocol_version`                                  | `ConnectOptions::version`, passed to `Tester::connect_with`. `Tester::connect` sends `0x03`; a caller that named `V2012` in 0.6 names `TesterVersion::V2012`. An answer in any version Table 16 defines is read |
| `ClientOptions::diagnostic_message_timeout`                        | `Tester::with_ack_timeout`; `ACK_TIMEOUT`, Table 12's 2 s, by default                                                                                                                                           |
| `RoutingActivationOptions`                                         | `ConnectOptions::activation_type` and `vm_specific`, passed to `Tester::connect_with`; `0x00` and no OEM-specific field by default                                                                              |
| `Connector`, `ConnectorSocket`                                     | the `edge-nal` stack passed to `connect`: any `TcpConnect`                                                                                                                                                      |
| `send_diagnostic_message(AddressType, bytes)`                      | `request(ta, TaType, pdu)`, then a `ConnectionEvent::Confirm` from `next_event`                                                                                                                                 |
| `receive_diagnostic_response()`                                    | `ConnectionEvent::Indication` from `next_event`                                                                                                                                                                 |
| `reconnect()`                                                      | `TesterConnection::reconnect(deadline)`, after `RECONNECT_BACKOFF` since the loss                                                                                                                               |
| `shut_down()`, `unbind_socket()`                                   | `TesterConnection::close()`                                                                                                                                                                                     |
| `Error`                                                            | `ConnectError` from `connect`, `service::ConnectionRefusal` from `request`, a `DoIpResult` in each confirm, and `ConnectionEvent::Closed` with `io_error()` for a lost connection                               |

Unlike `Client`, a `Tester` does nothing between calls: everything it reads or writes
happens inside `next_event`, which a caller keeps calling. A request's acknowledgement is
its confirm, so a response that arrives before it is still indicated.

```rust,no_run
use edge_nal_std::Stack;
use simple_doip::service::{
    ConnectionEvent, DiagnosticConnection, TesterAddress, TesterConnection,
};
use simple_doip::tester::Tester;
use simple_doip::{LogicalAddress, TCP_PORT, TaType};

const ENTITY: LogicalAddress = LogicalAddress(0x0001);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stack = Stack::new();
    let remote = ([192, 168, 0, 10], TCP_PORT).into();
    let sa = TesterAddress::new(LogicalAddress(0x0E00)).expect("a tester address");
    let mut tester = Tester::<_, 4096>::connect(&stack, remote, sa)
        .await
        .expect("connected and activated");

    tester
        .request(ENTITY, TaType::Physical, &[0x10, 0x03])
        .expect("taken");
    let mut buf = [0u8; 4096];
    loop {
        match tester.next_event(&mut buf, None).await.expect("read") {
            ConnectionEvent::Confirm { result, .. } => println!("acknowledged: {result:?}"),
            ConnectionEvent::Indication { pdu, .. } => {
                println!("answered: {pdu:02x?}");
                break;
            }
            ConnectionEvent::Closed => break,
            _ => {}
        }
    }
    tester.close().await.expect("closed");
}
```

### For UDS

A UDS client is a `uds_client!` client over `uds_on_ip::DoIpClientTransport`, which takes
the connected `Tester`. The client's services, response timing and reconnection run
above the tester, so the application calls services rather than sending bytes. As in
`testing/doip-loopback/tests/client_path.rs`:

```rust,no_run
# use edge_nal_std::Stack;
# use simple_doip::service::TesterAddress;
# use simple_doip::tester::Tester;
# use simple_doip::{LogicalAddress, TCP_PORT};
# use uds_on_ip::DoIpClientTransport;
# use uds_on_ip::profile::bench_reloads;
# use uds_services::{
#     Address, ClientLimits, ClientTiming, DataIdentifier, KeepAlive, PhysicalKeepAlive,
#     RecordError, Response, RoutineIdentifier, uds_client,
# };
#
# #[derive(Debug, Clone, Copy, PartialEq, Eq)]
# enum Did {
#     Vin,
# }
#
# impl DataIdentifier for Did {
#     const MAX_RECORD_LEN: usize = 17;
#     fn as_u16(self) -> u16 {
#         0xF190
#     }
#     fn from_u16(value: u16) -> Option<Self> {
#         (value == 0xF190).then_some(Self::Vin)
#     }
#     fn split_record(self, buf: &[u8]) -> Result<(&[u8], &[u8]), RecordError> {
#         buf.split_at_checked(17).ok_or(RecordError::Short)
#     }
# }
#
# #[derive(Debug, Clone, Copy, PartialEq, Eq)]
# enum Rid {
#     SelfTest,
# }
#
# impl RoutineIdentifier for Rid {
#     const MAX_STATUS_LEN: usize = 1;
#     fn as_u16(self) -> u16 {
#         0x0201
#     }
#     fn from_u16(value: u16) -> Option<Self> {
#         (value == 0x0201).then_some(Self::SelfTest)
#     }
# }
#
uds_client! {
    Did;
    vocabulary = DiagnoserVocabulary,
    transport = DoIpClientTransport<Tester<'static, Stack, 4096>, 4096>,
    routines = Rid,
    limits = ClientLimits::new()
        .max_dtcs(4)
        .max_dtc_record_len(16)
        .max_seed_len(2)
        .max_key_len(2)
        .max_option_len(4)
        .max_block_len(16),
    physical = 1,
    functional = 1,
    responders = 1,
    keep_alive = PhysicalKeepAlive,
    client = Diagnoser,
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() {
# let stack: &'static Stack = Box::leak(Box::new(Stack::new()));
# let remote = ([192, 168, 0, 10], TCP_PORT).into();
# let sa = TesterAddress::new(LogicalAddress(0x0E00)).expect("a tester address");
# let reloads = bench_reloads();
let tester = Tester::connect(stack, remote, sa)
    .await
    .expect("connected and activated");
let mut client = Diagnoser::new(
    DoIpClientTransport::new(tester, reloads),
    Address(0x0E00),
    KeepAlive::physical(2_000),
    ClientTiming::new(0, 0, 0),
);
if let Response::Positive(mut records) = client
    .read_data_by_identifier(Address(0x0001), &[Did::Vin])
    .await
    .expect("answered")
{
    if let Some((Did::Vin, vin)) = records.next() {
        println!("VIN {vin:02x?}");
    }
}
client.close().await.expect("closed");
# }
```

`DoIpClientTransport::with_max_data_size` takes the *Max. data size* that
`tester::discovery::entity_status` returns, so the client refuses a request longer than
the entity takes.

## `server::Server` → `entity::Entity`

| 0.6                                                                                                                                                    | 0.7                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `Server::new(handler)`, `run_server()`, `run_server_with_listener(listener)`                                                                           | `Entity::new(&acceptor, address, config)`, then `next_event` in a loop; the acceptor is the caller's, from `TcpBind::bind`     |
| `ServerConnectionHandler::get_logical_address`                                                                                                         | `EntityAddress::new(physical, functional)`                                                                                     |
| `ServerConnectionHandler::routing_activation`                                                                                                          | `service::EntityConfig::new([testers])`: the testers whose activation the entity accepts; it answers routing activation itself |
| `ServerConnectionHandler::diagnostic_message`, `ResponseWriter`                                                                                        | `EntityEvent::Indication`, answered with `request(sa, ta, TaType, pdu)`; the entity acknowledges each message itself           |
| `ServerConnectionHandler::alive_check`                                                                                                                 | none: the entity checks its testers itself (Figures 26 to 28)                                                                  |
| `run_udp_responder(socket)`, `get_vin`, `get_entity_id`, `get_group_id`, `vehicle_identification_with_eid`/`_vin`, `diagnostic_power_mode_information` | `Entity::with_discovery(udp, identity, seed)`, with a `FixedIdentity` or a `VehicleIdentity` of your own                       |
| `ServerConnectionHandler::protocol_version`                                                                                                            | none: the entity answers in each request's version                                                                             |
| one connection at a time                                                                                                                               | `MCTS` connections, plus the reserve socket                                                                                    |

The entity keeps Table 12's timers, announces itself at its first `next_event`, and
answers identification, entity status and power mode over UDP.

```rust,no_run
use edge_nal::{TcpBind, UdpBind};
use edge_nal_std::Stack;
use simple_doip::entity::{Entity, EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{DiagnosticEntity, EntityConfig, EntityEvent, TesterAddress};
use simple_doip::{EntityId, LogicalAddress, TCP_PORT, TaType, UDP_DISCOVERY_PORT, Vin};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stack = Stack::new();
    let acceptor = TcpBind::bind(&stack, ([0, 0, 0, 0], TCP_PORT).into())
        .await
        .expect("bound");
    let udp = UdpBind::bind(&stack, ([0, 0, 0, 0], UDP_DISCOVERY_PORT).into())
        .await
        .expect("bound");
    udp.get_ref()
        .set_broadcast(true)
        .expect("allowed to announce");

    let address = EntityAddress::new(LogicalAddress(0x0001), LogicalAddress(0xE400))
        .expect("entity addresses");
    let testers = EntityConfig::new([TesterAddress::new(LogicalAddress(0x0E00))
        .expect("a tester address")]);
    let identity = FixedIdentity::new(
        EntityId::new([0x02, 0x00, 0x00, 0xAB, 0xCD, 0xEF]).expect("set"),
        DiagnosticPowerModeCode::Ready,
    )
    .with_vin(Vin::new(*b"WVWZZZ1JZXW000001").expect("ASCII"));

    let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address, testers)
        .with_discovery(udp, identity, 0x1234_5678);
    let mut buf = [0u8; 4096];
    loop {
        if let EntityEvent::Indication { sa, pdu, .. } =
            entity.next_event(&mut buf, None).await.expect("serving")
        {
            entity
                .request(address.physical(), sa, TaType::Physical, pdu)
                .ok();
        }
    }
}
```

### For UDS

A UDS server is a `uds_server!` server over `uds_on_ip::DoIpTransport`, which takes the
entity. `uds_services` dispatches each request to the application's service traits and
keeps the session, its timing and the response pending. As in
`testing/doip-loopback/tests/sensor_path.rs`:

```rust,no_run
# use core::future::Future;
#
# use edge_nal::TcpBind;
# use edge_nal_std::{Stack, TcpAcceptor};
# use simple_doip::entity::{Entity, EntityAddress};
# use simple_doip::service::{EntityConfig, TesterAddress};
# use simple_doip::{LogicalAddress, TCP_PORT};
# use uds_on_ip::DoIpTransport;
# use uds_on_ip::profile::bench_reloads;
# use uds_protocol::NegativeResponseCode as Nrc;
# use uds_services::{
#     Access, Address, AfterReset, DiagnosticSessionControl, DiagnosticSessionType as S,
#     EcuReset, ResetType, ResponseSink, ServerParams, SessionTiming, SessionTransition,
#     Sessions, TesterPresent, uds_server,
# };
#
# #[derive(Debug, Default)]
# struct Ecu;
#
# impl DiagnosticSessionControl for Ecu {
#     const MAX_RESPONSE_LEN: usize = 0;
#     fn supports(&self, s: S) -> bool {
#         matches!(s, S::DefaultSession | S::ExtendedDiagnosticSession)
#     }
#     fn supported_from(&self, _s: S, _active: S) -> bool {
#         true
#     }
#     fn leaves_running_software(&self, _s: S) -> bool {
#         false
#     }
#     fn timing(&self, _s: S) -> SessionTiming {
#         SessionTiming {
#             p2_server_max_ms: 50,
#             p2_star_server_max: uds_services::P2StarServerMax::from_tens_of_millis(500),
#         }
#     }
#     fn on_transition(&mut self, _t: SessionTransition, _entered: S, _relocked: bool) {}
# }
#
# impl EcuReset for Ecu {
#     const MAY_RESPOND_PENDING: bool = false;
#     fn access(&self, _kind: ResetType) -> Option<Access> {
#         Some(Access::new(Sessions::ALL))
#     }
#     fn reset(
#         &mut self,
#         _kind: ResetType,
#     ) -> impl Future<Output = Result<AfterReset, Nrc>> {
#         core::future::ready(Ok(AfterReset::Restart))
#     }
#     fn confirmed(&mut self, _kind: ResetType) {}
# }
#
# impl TesterPresent for Ecu {
#     fn on_tester_present(&mut self) {}
# }
#
# const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 0);
#
uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent;
    transport = DoIpTransport<Entity<'static, TcpAcceptor, 1, 4096>, 2>,
    peers = 1,
    services = EcuServices,
    server = EcuServer,
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() {
# let stack = Stack::new();
# let acceptor: &'static TcpAcceptor = Box::leak(Box::new(
#     TcpBind::bind(&stack, ([0, 0, 0, 0], TCP_PORT).into())
#         .await
#         .expect("bound"),
# ));
# let address = EntityAddress::new(LogicalAddress(0x0001), LogicalAddress(0xE400))
#     .expect("entity addresses");
# let testers = EntityConfig::new([TesterAddress::new(LogicalAddress(0x0E00))
#     .expect("a tester address")]);
# let reloads = bench_reloads();
let entity = Entity::new(acceptor, address, testers);
let mut server = EcuServer::new(
    Ecu,
    DoIpTransport::new(entity, reloads),
    Address(0x0001),
    PARAMS,
);
server.run().await.expect("serving");
# }
```

## `bare_metal_entity::Entity` → `entity::Entity` on bare metal

`bare_metal_entity` was driven from a network stack's callbacks: the firmware owned the
sockets, passed each received segment or datagram to `on_tcp_rx`/`on_udp_rx`, and sent
what the entity handed to `Callbacks::send_tcp`/`send_udp`. It had no clock. The new
entity owns its sockets' reads and writes through `edge-nal`, and its timers through
`embassy-time`, so a firmware that moves needs three things it did not before:

1. **An async executor** on its target, to run `next_event`.
1. **An `embassy-time` driver and timer queue**, which give the entity Table 12's timers
   and the announcement waits.
1. **`edge-nal` sockets over its network stack**: `TcpAccept` with `TcpSplit` for
   `TCP_DATA`, and `UdpSplit` for `UDP_DISCOVERY`, whose futures the stack's callbacks
   wake. Every read, write and accept must do nothing when dropped, as the crate
   documentation sets out. The workspace's
   [`examples/embassy-net-entity`](https://github.com/luminartech/uds_stack/tree/main/examples/embassy-net-entity)
   is such an adapter for embassy-net: an `Acceptor` holding `MCTS + 1` sockets, and a
   `Udp` socket.

| `bare_metal_entity`                                                            | 0.7                                                                                                                                             |
| ------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `Entity::new()` (`const`), `init(&EntityConfig, Callbacks)`                    | `Entity::new(&acceptor, address, config).with_discovery(udp, identity, seed)`, built once the interface has its address (REQ 8.DoIP-050)        |
| `deinit()`                                                                     | dropping the entity                                                                                                                             |
| `EntityConfig::logical_address`                                                | `EntityAddress::new(physical, functional)`                                                                                                      |
| `EntityConfig::vin`, `eid`, `gid`                                              | `FixedIdentity::new(EntityId::new(eid)?, power_mode)`, `.with_vin(Vin::new(vin)?)`, and `.with_gid(GroupId::new(gid)?)` only where a GID is set |
| any tester in the tester address range                                         | only the testers named in `service::EntityConfig::new([...])`                                                                                   |
| `on_tcp_rx(&[u8]) -> TcpVerdict`, `Callbacks::send_tcp`, `on_tcp_disconnect()` | the `edge-nal` TCP sockets; the entity reads, writes and closes them itself                                                                     |
| `on_udp_rx(addr, port, &[u8])`, `Callbacks::send_udp`                          | the `edge-nal` UDP socket                                                                                                                       |
| `Callbacks::on_uds_request(req, resp) -> i32`                                  | `EntityEvent::Indication`, answered with `request`, or a `uds_services` server over `uds_on_ip::DoIpTransport`                                  |
| `UDS_RESP_CAP`, `TCP_RX_CAP`                                                   | the entity's `MAX_MESSAGE`, a type parameter                                                                                                    |

A firmware that moves sees these changes on the wire:

- The entity announces itself three times, unasked, after its first `next_event`
  (`A_DoIP_Announce_Num`), and answers an identification request after a random wait of
  up to `A_DoIP_Announce_Wait`, not at once.
- It closes a socket that does not activate routing within `T_TCP_Initial_Inactivity`,
  or that falls silent for `T_TCP_General_Inactivity`, and checks a registered tester is
  alive before letting another take its place.
- It answers each request in that request's protocol version, where
  `bare_metal_entity` always used `0x02`.
- Power mode is the identity's, where `bare_metal_entity` always answered "not
  supported".
- A diagnostic message from a source address other than the one activated on its socket
  is refused `0x02` to its sender, and the socket closed.

An entity that echoes every diagnostic message, on embassy-net, is the adapter's `serve`:

```rust,no_run
# use embassy_net::Stack;
# use embassy_net::tcp::TcpSocket;
# use embassy_net::udp::UdpSocket;
# use embassy_net_entity::{Acceptor, SocketBuffers, Udp};
# use simple_doip::entity::{Entity, EntityAddress, FixedIdentity};
# use simple_doip::service::{DiagnosticEntity, EntityConfig, EntityEvent};
# use simple_doip::{TCP_PORT, TaType};
#
# async fn serve(
#     stack: Stack<'_>,
#     address: EntityAddress,
#     config: EntityConfig,
#     udp_socket: UdpSocket<'_>,
#     identity: FixedIdentity,
#     seed: u32,
#     buffers: &mut [SocketBuffers; 2],
# ) -> Result<(), simple_doip::entity::Error<embassy_net::tcp::Error>> {
# let [first, second] = buffers;
let acceptor = Acceptor::new(
    TCP_PORT,
    [
        TcpSocket::new(stack, &mut first.rx, &mut first.tx),
        TcpSocket::new(stack, &mut second.rx, &mut second.tx),
    ],
);
let mut entity = Entity::<_, 1, 4096>::new(&acceptor, address, config)
    .with_discovery(Udp::bind(udp_socket).expect("bound"), identity, seed);
let mut buf = [0u8; 4096];
loop {
    if let EntityEvent::Indication { sa, pdu, .. } =
        entity.next_event(&mut buf, None).await?
    {
        entity
            .request(address.physical(), sa, TaType::Physical, pdu)
            .ok();
    }
}
# }
```
