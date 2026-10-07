//! What a slot waits on from its socket: a write and a read at once, on the socket's two
//! halves, or a close. The backend makes each cancel-safe, and the progress of each is
//! applied in the poll that completes it.

use core::future::pending;

use edge_nal::{Close, Readable, TcpShutdown, TcpSplit};
use embassy_futures::select::{Either, select};
use embedded_io_async::{Read, Write};

use super::table::{Phase, Slot};
use crate::stream::rx::RxBuffer;
use crate::stream::tx::TxQueue;

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

/// Writes what is queued while reading up to the end of the frame being received, each
/// on its own half of the socket, and returns whichever finishes first; closes a
/// finalizing socket once nothing is queued. Reads nothing while finalizing. Never
/// completes for an empty slot, nor for one with nothing queued and a whole frame not
/// yet handled.
pub(super) async fn drive<S, const CAP: usize>(slot: &mut Slot<S, CAP>) -> Io
where
    S: TcpSplit + TcpShutdown,
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
    if finalizing && tx.pending().is_empty() {
        open.socket.close(Close::Both).await.ok();
        return Io::Closed;
    }
    let (mut reader, mut writer) = open.socket.split();
    let reading = !finalizing && !rx.free_to_frame_end().is_empty();
    match (tx.pending().is_empty(), reading) {
        (false, true) => {
            match select(write(&mut writer, tx), read(&mut reader, rx)).await {
                Either::First(io) | Either::Second(io) => io,
            }
        }
        (false, false) => write(&mut writer, tx).await,
        (true, true) => read(&mut reader, rx).await,
        (true, false) => pending().await,
    }
}

async fn write<W: Write, const CAP: usize>(writer: &mut W, tx: &mut TxQueue<CAP>) -> Io {
    match writer.write(tx.pending()).await {
        Ok(0) | Err(_) => Io::Lost,
        Ok(written) => {
            tx.advance(written);
            Io::Wrote
        }
    }
}

async fn read<R: Read + Readable, const CAP: usize>(
    reader: &mut R,
    rx: &mut RxBuffer<CAP>,
) -> Io {
    if reader.readable().await.is_err() {
        return Io::Lost;
    }
    match reader.read(rx.free_to_frame_end()).await {
        Ok(0) | Err(_) => Io::Lost,
        Ok(read) => {
            rx.filled(read);
            Io::Read
        }
    }
}
