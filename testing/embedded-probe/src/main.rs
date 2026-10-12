//! The sensor's server path as bare-metal firmware: `uds_server!` with every staged
//! service, over `uds_on_ip`'s `DoIpTransport`, over `simple_doip`'s `Entity` serving one
//! tester, `0x0E00`, with no allocator.
//!
//! The acceptor is a stub the optimizer cannot see through, so the image carries the
//! entity's whole frame handling but no network stack. What an integrator supplies is
//! here too: a `critical-section` implementation, an `embassy-time` driver with its timer
//! queue, and a `static` the server is built in.
//!
//! The client role is not run, but its transport's size is in the image, so that the
//! gate records what a tester holds by value.
//!
//! Built for bare metal only; on a host the binary is empty.

#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
mod firmware;

#[cfg(not(target_os = "none"))]
fn main() {}
