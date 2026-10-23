//! The calls the firmware implements, as `include/uds_c.h` declares them. A call with no
//! pointer argument has no precondition and is declared `safe`.

#![allow(
    unsafe_code,
    reason = "declares the C functions the firmware implements"
)]

/// `UDS_SOCK_WOULD_BLOCK`: nothing to do yet; the server polls again later.
pub(crate) const SOCK_WOULD_BLOCK: i32 = -1;

unsafe extern "C" {
    pub(crate) fn uds_tcp_accept(peer_addr: *mut u32, peer_port: *mut u16) -> i32;
    pub(crate) safe fn uds_tcp_readable(sock: i32) -> bool;
    pub(crate) fn uds_tcp_read(sock: i32, buf: *mut u8, cap: usize) -> i32;
    pub(crate) fn uds_tcp_write(sock: i32, buf: *const u8, len: usize) -> i32;
    pub(crate) safe fn uds_tcp_close(sock: i32, reset: bool);
    pub(crate) safe fn uds_tcp_release(sock: i32);
    pub(crate) safe fn uds_udp_readable() -> bool;
    pub(crate) fn uds_udp_recv(
        buf: *mut u8,
        cap: usize,
        src_addr: *mut u32,
        src_port: *mut u16,
    ) -> i32;
    pub(crate) fn uds_udp_send(
        dst_addr: u32,
        dst_port: u16,
        buf: *const u8,
        len: usize,
    ) -> i32;
    pub(crate) safe fn uds_now_ms() -> u32;
}
