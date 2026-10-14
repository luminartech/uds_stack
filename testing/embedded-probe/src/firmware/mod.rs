//! The firmware: the server assembled in a `static`, run from reset; and the client
//! transport's size, kept in the image for the size gate to read.

mod clock;
mod stub;

use cortex_m_rt::entry;
use simple_doip::entity::{Discovery, Entity, EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{EntityConfig, TesterAddress};
use simple_doip::tester::{DIAGNOSTIC_MESSAGE_OVERHEAD, Tester};
use simple_doip::{EntityId, GroupId, LogicalAddress, Vin};
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

type SensorEntity = Entity<
    'static,
    stub::Acceptor,
    1,
    MAX_MESSAGE,
    1,
    Discovery<stub::Datagrams, FixedIdentity>,
>;
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

const PARAMS: ServerParams = ServerParams {
    s3_server: 5_000,
    p2_server_max: 50,
    p2_star_server_max: 5_000,
    response_pending_lead: 0,
};

static ACCEPTOR: stub::Acceptor = stub::Acceptor;
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
    let (Some(()), Ok(address), Ok(tester), Ok(eid), Ok(vin), Ok(gid)) = (
        clock::start(),
        EntityAddress::new(ENTITY, FUNCTIONAL),
        TesterAddress::new(TESTER),
        EntityId::new(EID),
        Vin::new(VIN),
        GroupId::new(GID),
    ) else {
        halt()
    };
    let identity = FixedIdentity::new(eid, DiagnosticPowerModeCode::Ready)
        .with_vin(vin)
        .with_gid(gid);
    let server = SERVER.init_with(|| {
        let entity = Entity::new(&ACCEPTOR, address, EntityConfig::new([tester]))
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
