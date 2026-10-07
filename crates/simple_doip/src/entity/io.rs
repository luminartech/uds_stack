//! The one socket operation a slot is waiting on, each of which the backend makes
//! cancel-safe, with its progress applied in the same poll that completes it.

use core::future::pending;

use edge_nal::{Close, Readable, TcpShutdown};
use embedded_io_async::{Read, Write};

use super::table::{Phase, Slot};

/// What a slot's socket did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Io {
    Wrote,
    Read,
    /// End of stream, a failed read or write, or a write of nothing.
    Lost,
    /// The close or abort finished; the socket can leave the table.
    Closed,
}

/// Writes what is queued, else closes a finalizing socket, else reads up to the end of
/// the frame being received. Never completes for an empty slot, nor for one holding a
/// whole frame not yet handled.
pub(super) async fn drive<S, const CAP: usize>(slot: &mut Slot<S, CAP>) -> Io
where
    S: Read + Write + Readable + TcpShutdown,
{
    let Slot { open, rx, tx, .. } = slot;
    let Some(open) = open.as_mut() else {
        return pending().await;
    };
    let finalizing = match open.phase {
        Phase::Finalizing { abort: true, .. } => {
            open.socket.abort().await.ok();
            return Io::Closed;
        }
        Phase::Finalizing { abort: false, .. } => true,
        Phase::Initialized | Phase::Registered { .. } => false,
    };
    if !tx.pending().is_empty() {
        return match open.socket.write(tx.pending()).await {
            Ok(0) | Err(_) => Io::Lost,
            Ok(written) => {
                tx.advance(written);
                Io::Wrote
            }
        };
    }
    if finalizing {
        open.socket.close(Close::Both).await.ok();
        return Io::Closed;
    }
    if rx.free_to_frame_end().is_empty() {
        return pending().await;
    }
    if open.socket.readable().await.is_err() {
        return Io::Lost;
    }
    match open.socket.read(rx.free_to_frame_end()).await {
        Ok(0) | Err(_) => Io::Lost,
        Ok(read) => {
            rx.filled(read);
            Io::Read
        }
    }
}
