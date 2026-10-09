//! The firmware: the server assembled in a `static`, run from reset.

mod clock;
mod services;
mod stub;

use cortex_m_rt::entry;
use simple_doip::LogicalAddress;
use simple_doip::entity::{Discovery, Entity, EntityAddress, FixedIdentity};
use simple_doip::messages::DiagnosticPowerModeCode;
use simple_doip::service::{EntityConfig, TesterAddress};
use static_cell::StaticCell;
use uds_on_ip::DoIpTransport;
use uds_on_ip::profile::bench_reloads;
use uds_services::{Address, ServerParams, uds_server};

use services::Ecu;

const MAX_MESSAGE: usize = 4096;
const ENTITY: LogicalAddress = LogicalAddress(0x0001);
const FUNCTIONAL: LogicalAddress = LogicalAddress(0xE400);
const TESTER: LogicalAddress = LogicalAddress(0x0E00);
const IDENTITY: FixedIdentity = FixedIdentity::new(
    [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
    DiagnosticPowerModeCode::Ready,
)
.with_vin(*b"WVWZZZ1JZXW000001")
.with_gid([0x02, 0x00, 0x00, 0x00, 0x00, 0x00]);

type SensorEntity = Entity<
    'static,
    stub::Acceptor,
    1,
    MAX_MESSAGE,
    1,
    Discovery<stub::Datagrams, FixedIdentity>,
>;
type Transport = DoIpTransport<SensorEntity, 2>;

uds_server! {
    Ecu: DiagnosticSessionControl, EcuReset, TesterPresent, SecurityAccess,
         ReadDataByIdentifier, WriteDataByIdentifier, RoutineControl, ReadDtcInformation,
         ClearDiagnosticInformation, CommunicationControl, ControlDtcSetting;
    transport = Transport,
    peers = 1,
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
    let (Some(()), Ok(address), Ok(tester)) = (
        clock::start(),
        EntityAddress::new(ENTITY, FUNCTIONAL),
        TesterAddress::new(TESTER),
    ) else {
        halt()
    };
    let server = SERVER.init_with(|| {
        let entity = Entity::new(&ACCEPTOR, address, EntityConfig::new([tester]))
            .with_discovery(stub::Datagrams, IDENTITY, core::hint::black_box(0));
        EcuServer::new(
            Ecu::new(),
            DoIpTransport::new(entity, bench_reloads()),
            Address(ENTITY.0),
            PARAMS,
        )
    });
    let _ = embassy_futures::block_on(server.run());
    halt()
}
