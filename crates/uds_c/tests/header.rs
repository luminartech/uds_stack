//! `include/uds_c.h` against the Rust mirror of its structures: the host's C compiler
//! (`$CC`, or `cc`) builds a program that prints every structure's size and every
//! field's offset, which must equal Rust's. The header's own 32-bit layout checks run
//! in the same compile, as do strict warnings. Without a C11 compiler the test fails,
//! saying so.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test harness: a failure to build or run the probe is the test failing"
)]

use core::mem::{offset_of, size_of};
use std::env;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use uds_c::{
    UdsAccess, UdsDidEntry, UdsDtcHooks, UdsLayer, UdsRidEntry, UdsSecurityLevel,
    UdsTransferHooks, UdsTransitionHooks,
};

/// Each structure with its fields, as `(type, field, Rust offset)`, and its Rust size.
macro_rules! layouts {
    ($($ty:ident { $($field:ident),+ $(,)? })+) => {
        [$(
            (stringify!($ty), size_of::<$ty>(), &[$(
                (stringify!($field), offset_of!($ty, $field)),
            )+][..]),
        )+]
    };
}

#[test]
fn the_header_lays_out_every_structure_as_rust_does() {
    let layouts = layouts! {
        UdsAccess { sessions, security_level }
        UdsDidEntry { did, read_sessions, read, write_access, len, write }
        UdsRidEntry { rid, status_len, access, start, stop, results }
        UdsSecurityLevel { level, sessions, attempt_limit, seed_len, delay_ms, seed, key }
        UdsDtcHooks { get, snapshot_count, snapshot_item, clear, setting }
        UdsTransferHooks { access, begin, block, finish }
        UdsTransitionHooks { check, prepare, execute }
        UdsLayer {
            dids, did_count, rids, rid_count, security_levels, security_level_count, dtc,
            comm_control, transfer, session, reset,
        }
    };

    let mut program =
        String::from("#include <stdio.h>\n#include \"uds_c.h\"\nint main(void)\n{\n");
    let mut expected = String::new();
    for (ty, size, fields) in layouts {
        writeln!(program, "    printf(\"{ty} %zu\\n\", sizeof({ty}));").unwrap();
        writeln!(expected, "{ty} {size}").unwrap();
        for (field, offset) in fields {
            writeln!(
                program,
                "    printf(\"{ty}.{field} %zu\\n\", offsetof({ty}, {field}));"
            )
            .unwrap();
            writeln!(expected, "{ty}.{field} {offset}").unwrap();
        }
    }
    program.push_str("    return 0;\n}\n");

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("uds_c_header");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("layout.c");
    let binary = dir.join("layout");
    std::fs::write(&source, program).unwrap();
    let include = Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
    let compiler = env::var("CC").unwrap_or_else(|_| String::from("cc"));
    let built = Command::new(&compiler)
        .args(["-std=c11", "-Wall", "-Wextra", "-Wpedantic", "-Werror"])
        .arg("-I")
        .arg(&include)
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "this test compiles the header with a C11 compiler, `$CC` or `cc`; \
                 running `{compiler}` failed: {e}"
            )
        });
    assert!(
        built.status.success(),
        "the header does not compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&binary).output().unwrap();
    assert!(ran.status.success());
    assert_eq!(String::from_utf8(ran.stdout).unwrap(), expected);
}
