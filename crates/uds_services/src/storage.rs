//! Where the bytes live.
//!
//! ``UDSSVC_ARCH_0017`` makes this crate the owner of the buffers. The sizes are not
//! chosen here and not chosen by the application either: [`crate::uds_server`] folds
//! each service's declared worst case into the array lengths below, at a site where
//! the types are concrete and const evaluation is ordinary stable Rust.
//!
//! **Three buffers, and the reason is the response-pending window.** A handler holds a
//! decoded request borrowing the in-flight buffer, so that buffer cannot be handed back
//! to [`crate::UdsTransport::next_event`] while the handler runs. `next_event` is the
//! only way this crate can wait (``UDSSVC_ARCH_0041``), so without a second buffer the
//! driver cannot observe the `tP2_Server` deadline and ``UDSSVC_ARCH_0031``'s 0x78 never
//! happens. Clause 8.7.6's two exceptions
//! are the second reason, not the first.
//!
//! The in-flight buffer's length is also the entity's ISO 13400-2:2019 Table 11
//! *Max. data size* — that table defines MDS as "the maximum size of one logical request
//! that this `DoIP` entity can process", which is the array a full request is decoded
//! from. Nothing carries the number across [`crate::UdsTransport`] yet; when a binding
//! needs to advertise it, the seam gains a way for this crate to state it.

/// The three buffers, together.
#[derive(Debug)]
pub struct Buffers<'a> {
    /// The request being served. Bounds MDS.
    pub in_flight: &'a mut [u8],
    /// What may arrive while a service is in progress — clause 8.7.6's two exceptions
    /// only, so a handful of bytes.
    pub concurrent: &'a mut [u8],
    /// Where a handler writes.
    pub response: &'a mut [u8],
}

/// The storage one assembled application needs.
///
/// ``UDSSVC_ARCH_0013`` — implemented by the type [`crate::uds_server`] generates,
/// never by hand. Sealed, so "never" is a compile error rather than a request: [`Store`]
/// is the only implementor, and a hand-written one could pick lengths that disagree with
/// the maxima the assembly folded.
pub trait Storage: crate::sealed::Sealed {
    /// Zeroed storage.
    ///
    /// An associated const rather than a `const fn`, because trait methods cannot be
    /// `const` on stable and [`crate::Server::new`] must work in a `static` initialiser:
    /// a multi-kilobyte server built at runtime is a stack temporary before the move,
    /// which is fatal on a small-stack target, and building it in place needs `unsafe`.
    ///
    /// This is `uds_session`'s own convention — `Association::EMPTY`,
    /// `PhysicalSlot::EMPTY`, `FunctionalSlot::EMPTY`.
    const EMPTY: Self;

    /// All three buffers at once, because all three are live at once.
    fn split(&mut self) -> Buffers<'_>;
}

/// Storage for an assembled server.
///
/// Public because the generated code names it; its fields are not.
#[derive(Debug)]
pub struct Store<const REQ: usize, const CONC: usize, const RSP: usize> {
    in_flight: [u8; REQ],
    concurrent: [u8; CONC],
    response: [u8; RSP],
}

impl<const REQ: usize, const CONC: usize, const RSP: usize> crate::sealed::Sealed
    for Store<REQ, CONC, RSP>
{
}

impl<const REQ: usize, const CONC: usize, const RSP: usize> Storage
    for Store<REQ, CONC, RSP>
{
    const EMPTY: Self = Self {
        in_flight: [0; REQ],
        concurrent: [0; CONC],
        response: [0; RSP],
    };

    fn split(&mut self) -> Buffers<'_> {
        Buffers {
            in_flight: &mut self.in_flight,
            concurrent: &mut self.concurrent,
            response: &mut self.response,
        }
    }
}

/// The two buffers a client needs, together.
#[derive(Debug)]
pub struct ClientBuffers<'a> {
    /// Where a request is encoded before it goes to the transport.
    pub request: &'a mut [u8],
    /// Where a response lands, and what [`crate::Records`] borrows.
    pub response: &'a mut [u8],
}

/// The storage one assembled client needs.
///
/// ``UDSSVC_ARCH_0017`` for the client half: the buffers are this crate's, and
/// [`crate::uds_client`] folds their lengths from the identifier enumeration's declared
/// maximum record and the most identifiers one request may name. Sealed for the reason
/// [`Storage`] is — a hand-written implementor could pick lengths that disagree with the
/// maxima the assembly folded.
///
/// [`Debug`] is a supertrait because [`crate::Responses`] holds a `&mut Client` and so
/// needs the store's `Debug` one projection deeper than a derive can infer, and this
/// crate denies `missing_debug_implementations`.
///
/// **Two buffers rather than one.** A single buffer would be the larger of the two and
/// therefore smaller than this pair, and the request would not survive the response
/// landing on top of it. Keeping them apart is what lets a retransmission after a
/// `tP_Client` expiry re-send the bytes it already encoded, rather than encoding them
/// again into storage the caller may be holding as [`crate::Records`].
pub trait ClientStorage: crate::sealed::Sealed + core::fmt::Debug {
    /// Zeroed storage.
    ///
    /// An associated const for the reason [`Storage::EMPTY`] is one: a client built at
    /// runtime is a stack temporary before the move, and
    /// [`crate::Client::new`] must work in a `static` initialiser.
    const EMPTY: Self;

    /// Both buffers at once.
    fn split(&mut self) -> ClientBuffers<'_>;
}

/// Storage for an assembled client.
///
/// Public because the generated code names it; its fields are not.
#[derive(Debug)]
pub struct ClientStore<const REQ: usize, const RSP: usize> {
    request: [u8; REQ],
    response: [u8; RSP],
}

impl<const REQ: usize, const RSP: usize> crate::sealed::Sealed for ClientStore<REQ, RSP> {}

impl<const REQ: usize, const RSP: usize> ClientStorage for ClientStore<REQ, RSP> {
    const EMPTY: Self = Self {
        request: [0; REQ],
        response: [0; RSP],
    };

    fn split(&mut self) -> ClientBuffers<'_> {
        ClientBuffers {
            request: &mut self.request,
            response: &mut self.response,
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::{ClientStorage, ClientStore, Storage, Store};

    /// The client's two arrays are the declared lengths, and both are reachable at once
    /// so a request can be encoded while the response buffer is live.
    #[test]
    fn the_client_buffers_are_the_declared_lengths() {
        let mut s = <ClientStore<9, 77> as ClientStorage>::EMPTY;
        let b = s.split();
        assert_eq!((b.request.len(), b.response.len()), (9, 77));
        b.request[0] = 0x22;
        b.response[0] = 0x62;
        assert_eq!((b.request[0], b.response[0]), (0x22, 0x62));
    }

    /// A client constructs in a `static` for the same reason a server does.
    #[test]
    fn client_storage_is_const_constructible() {
        static STORE: ClientStore<9, 77> = <ClientStore<9, 77> as ClientStorage>::EMPTY;
        assert_eq!(core::mem::size_of_val(&STORE), 86);
    }

    /// The three arrays are exactly the declared sizes. The whole derivation scheme
    /// rests on this, so it is asserted rather than assumed.
    #[test]
    fn the_buffers_are_the_declared_lengths() {
        let mut s = <Store<7, 3, 11> as Storage>::EMPTY;
        let b = s.split();
        assert_eq!(b.in_flight.len(), 7);
        assert_eq!(b.concurrent.len(), 3);
        assert_eq!(b.response.len(), 11);
    }

    /// All three come back from one call because all three are live at once: the
    /// request is read while the response is written and the concurrent slot is
    /// receiving. Three accessors would not borrow, and this test is what fails if
    /// someone later "simplifies" `split` into separate methods.
    #[test]
    fn all_three_are_usable_at_once() {
        let mut s = <Store<4, 4, 4> as Storage>::EMPTY;
        let b = s.split();
        b.in_flight[0] = 0x22;
        b.concurrent[0] = 0x3E;
        b.response[0] = b.in_flight[0];
        assert_eq!((b.response[0], b.concurrent[0]), (0x22, 0x3E));
    }

    /// `EMPTY` is an associated const, not a `const fn`, so `Server::new` can be a
    /// `const fn` in a generic context — trait methods cannot be `const` on stable.
    /// A `static` initialiser is the thing that must work.
    #[test]
    fn storage_is_const_constructible() {
        static STORE: Store<8, 2, 8> = <Store<8, 2, 8> as Storage>::EMPTY;
        assert_eq!(core::mem::size_of_val(&STORE), 18);
    }
}
