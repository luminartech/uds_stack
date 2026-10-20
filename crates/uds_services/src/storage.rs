//! Where the bytes live.
//!
//! ``UDSSVC_ARCH_0017`` makes this crate the owner of the buffers, and
//! [`crate::uds_server`] folds each service's declared worst case into their lengths.
//!
//! A server has three: the handler holds a decoded request borrowing the in-flight
//! buffer, so the driver waits on [`crate::UdsTransport::next_event`] with the concurrent
//! buffer meanwhile (``UDSSVC_ARCH_0041``), which is how it observes the `tP2_Server`
//! deadline and clause 8.7.6's two exceptions. The in-flight buffer bounds the request a
//! handler decodes, not the request the server receives: a longer one arrives truncated,
//! as [`crate::TransportEvent::DataTooLong`], and is answered by ISO 14229-1's rules.

/// The three buffers, together.
#[derive(Debug)]
pub struct Buffers<'a> {
    /// The request being served.
    pub in_flight: &'a mut [u8],
    /// What may arrive while a service is in progress: clause 8.7.6's two exceptions
    /// only, so a handful of bytes.
    pub concurrent: &'a mut [u8],
    /// Where a handler writes.
    pub response: &'a mut [u8],
}

/// The storage one assembled application needs.
///
/// ``UDSSVC_ARCH_0013``: implemented by [`Store`], which [`crate::uds_server`] names with
/// the lengths it folded, and sealed so that nothing else can.
pub trait Storage: crate::sealed::Sealed {
    /// Zeroed storage.
    ///
    /// An associated const rather than a `const fn`, so that [`crate::Server::new`] can
    /// be `const` and build a server in a `static` initializer without a stack temporary.
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
/// ``UDSSVC_ARCH_0017`` for the client half: implemented by [`ClientStore`], which
/// [`crate::uds_client`] names with the lengths it folded from the vocabulary and its
/// [`crate::ClientLimits`], and sealed so that nothing else can. [`Debug`] is a supertrait
/// because [`crate::Responses`] holds a `&mut Client` and derives `Debug`.
///
/// The request keeps its own buffer so that a repeat after a `tP_Client` expiry re-sends
/// the bytes already encoded while the caller may still hold the response as
/// [`crate::Records`].
pub trait ClientStorage: crate::sealed::Sealed + core::fmt::Debug {
    /// Zeroed storage.
    ///
    /// An associated const for the reason [`Storage::EMPTY`] is one: [`crate::Client::new`]
    /// is `const` and builds a client in a `static` initializer.
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

    /// `EMPTY` initializes a `static`, as a server's does.
    #[test]
    fn client_storage_is_const_constructible() {
        static STORE: ClientStore<9, 77> = <ClientStore<9, 77> as ClientStorage>::EMPTY;
        assert_eq!(core::mem::size_of_val(&STORE), 86);
    }

    /// The three arrays are exactly the declared sizes.
    #[test]
    fn the_buffers_are_the_declared_lengths() {
        let mut s = <Store<7, 3, 11> as Storage>::EMPTY;
        let b = s.split();
        assert_eq!(b.in_flight.len(), 7);
        assert_eq!(b.concurrent.len(), 3);
        assert_eq!(b.response.len(), 11);
    }

    /// All three come back from one call and are usable at once: the request is read
    /// while the response is written and the concurrent slot is receiving.
    #[test]
    fn all_three_are_usable_at_once() {
        let mut s = <Store<4, 4, 4> as Storage>::EMPTY;
        let b = s.split();
        b.in_flight[0] = 0x22;
        b.concurrent[0] = 0x3E;
        b.response[0] = b.in_flight[0];
        assert_eq!((b.response[0], b.concurrent[0]), (0x22, 0x3E));
    }

    /// `EMPTY` initializes a `static`.
    #[test]
    fn storage_is_const_constructible() {
        static STORE: Store<8, 2, 8> = <Store<8, 2, 8> as Storage>::EMPTY;
        assert_eq!(core::mem::size_of_val(&STORE), 18);
    }
}
