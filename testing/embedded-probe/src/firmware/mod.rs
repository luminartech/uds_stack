//! The firmware: the server built in place as a `static`'s initializer, its discovery
//! started from reset, then run; and the client transport's size, kept in the image for
//! the size gate to read.
//!
//! With the `run-time` feature, the server is built in `main` and copied into its
//! `static`, as one over an acceptor built at run time is.

mod clock;
mod stub;

use cortex_m_rt::entry;
use simple_doip::entity::{Discovery, Entity, EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{EntityConfig, TesterAddress};
use simple_doip::tester::{DIAGNOSTIC_MESSAGE_OVERHEAD, Tester};
use simple_doip::{EntityId, GroupId, LogicalAddress, Vin};
#[cfg(not(feature = "run-time"))]
use static_cell::ConstStaticCell;
#[cfg(feature = "run-time")]
use static_cell::StaticCell;
use uds_on_ip::profile::bench_reloads;
use uds_on_ip::{DoIpClientTransport, DoIpTransport};
use uds_services::{Address, ServerParams, uds_server};

const MAX_MESSAGE: usize = 4096;
const ENTITY: LogicalAddress = LogicalAddress(0x0001);
const FUNCTIONAL: LogicalAddress = LogicalAddress(0xE400);
const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const EID: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
const VIN: [u8; 17] = *b"WVWZZZ1JZXW000001";
const GID: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

#[cfg(not(feature = "run-time"))]
type SensorDiscovery = Option<Discovery<stub::Datagrams, FixedIdentity>>;
#[cfg(feature = "run-time")]
type SensorDiscovery = Discovery<stub::Datagrams, FixedIdentity>;

type SensorEntity = Entity<'static, stub::Acceptor, 1, MAX_MESSAGE, 1, SensorDiscovery>;
type Transport = DoIpTransport<SensorEntity, 2>;

/// The sensor's tester: one message of up to `MAX_MESSAGE` bytes, header included.
type SensorTester =
    Tester<'static, stub::Connector, { MAX_MESSAGE + DIAGNOSTIC_MESSAGE_OVERHEAD }>;

/// The client role's transport over it, queueing a whole message behind the one the
/// tester carries. Nothing here runs it: its size is what the gate records, because an
/// integrator holds one by value and `new` returns it so.
type SensorClientTransport = DoIpClientTransport<SensorTester, MAX_MESSAGE>;

/// `size_of::<SensorClientTransport>()`, read out of the image by `tools/check_size.py`
/// as `client_transport`: a 32-bit target's `usize`. `main` takes its address so the
/// linker keeps it.
static CLIENT_TRANSPORT_SIZE: usize = size_of::<SensorClientTransport>();

use sensor_services::Ecu;

uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting, DataTransfer;
    transport = Transport,
    peers = 1,
    services = EcuServices,
    server = EcuServer,
}

const PARAMS: ServerParams = ServerParams::new(5_000, 50, 5_000, 0);

/// The entity's addresses, which fail the build rather than the firmware where invalid.
const ADDRESS: EntityAddress = match EntityAddress::new(ENTITY, FUNCTIONAL) {
    Ok(address) => address,
    Err(_) => panic!("the entity's addresses are invalid"),
};

/// The one tester, which fails the build rather than the firmware where invalid.
const TESTER_ADDRESS: TesterAddress = match TesterAddress::new(TESTER) {
    Ok(tester) => tester,
    Err(_) => panic!("the tester's address is invalid"),
};

static ACCEPTOR: stub::Acceptor = stub::Acceptor;

/// The server, built in place: its discovery waits for the identity `main` learns.
#[cfg(not(feature = "run-time"))]
static SERVER: ConstStaticCell<EcuServer> = ConstStaticCell::new(EcuServer::new(
    Ecu::new(),
    DoIpTransport::new(
        Entity::new_discovery_later(
            &ACCEPTOR,
            ADDRESS,
            EntityConfig::new([TESTER_ADDRESS]),
        ),
        bench_reloads(),
    ),
    Address(ENTITY.0),
    PARAMS,
));

#[cfg(feature = "run-time")]
static SERVER: StaticCell<EcuServer> = StaticCell::new();

fn halt() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    halt()
}

#[entry]
fn main() -> ! {
    let (Some(()), Ok(eid), Ok(vin), Ok(gid)) = (
        clock::start(),
        EntityId::new(EID),
        Vin::new(VIN),
        GroupId::new(GID),
    ) else {
        halt()
    };
    let identity = FixedIdentity::new(eid, DiagnosticPowerModeCode::Ready)
        .with_vin(vin)
        .with_gid(gid);
    #[cfg(not(feature = "run-time"))]
    let server = {
        let server = SERVER.take();
        let Some(Ok(())) = server.transport_mut().map(|transport| {
            transport.start_discovery(stub::Datagrams, identity, core::hint::black_box(0))
        }) else {
            halt()
        };
        server
    };
    #[cfg(feature = "run-time")]
    let server = SERVER.init_with(|| {
        let entity = Entity::new(&ACCEPTOR, ADDRESS, EntityConfig::new([TESTER_ADDRESS]))
            .with_discovery(stub::Datagrams, identity, core::hint::black_box(0));
        EcuServer::new(
            Ecu::new(),
            DoIpTransport::new(entity, bench_reloads()),
            Address(ENTITY.0),
            PARAMS,
        )
    });
    core::hint::black_box(&CLIENT_TRANSPORT_SIZE);
    let _ = embassy_futures::block_on(server.run());
    halt()
}
