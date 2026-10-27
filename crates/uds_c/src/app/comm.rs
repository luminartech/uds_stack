//! `CommunicationControl` (0x28) over the first layer that serves it.

use uds_services::NegativeResponseCode as Nrc;
use uds_services::{
    Access, CommunicationControl, CommunicationControlType, CommunicationType, SubnetNumber,
};

use super::{Policy, TableApp, outside_default, settled};
use crate::op;

impl<P: Policy, const RECORD: usize, const LEVELS: usize> CommunicationControl
    for TableApp<P, RECORD, LEVELS>
{
    const MAY_RESPOND_PENDING: bool = true;

    /// `enableRxAndTx` to `disableRxAndTx`, in the extended and programming sessions;
    /// the enhanced address forms are not served.
    fn access(&self, control_type: CommunicationControlType) -> Option<Access> {
        (u8::from(control_type) <= 0x03).then(outside_default)
    }

    async fn control(
        &mut self,
        control_type: CommunicationControlType,
        communication_type: CommunicationType,
        _subnet: SubnetNumber,
        _node_id: Option<u16>,
    ) -> Result<(), Nrc> {
        let Some(control) = P::LAYERS.comm_control() else {
            return Err(Nrc::ServiceNotSupported);
        };
        let (control_type, communication_type) =
            (u8::from(control_type), u8::from(communication_type));
        settled(op::settle(|op| control.call(op, control_type, communication_type)).await)
    }
}
