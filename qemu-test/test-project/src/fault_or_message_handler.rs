use selfe_sys::{seL4_BootInfo, seL4_MessageInfo_new, seL4_Send};

use ferros::alloc::{micro_alloc, smart_alloc, ut_buddy};
use ferros::bootstrap::*;
use ferros::cap::*;
use ferros::test_support::*;
use ferros::userland::*;
use ferros::vspace::*;

use typenum::*;

use super::TopLevelError;

pub fn run(raw_boot_info: &'static seL4_BootInfo) -> Result<(), TopLevelError> {
    let BootInfo {
        root_page_directory,
        asid_control,
        user_image,
        root_tcb,
        ..
    } = BootInfo::wrap(&raw_boot_info);
    let mut allocator = micro_alloc::Allocator::bootstrap(&raw_boot_info)?;
    let (root_cnode, local_slots) = root_cnode(&raw_boot_info);
    let mut shared_uts = ut_buddy(
        allocator
            .get_untyped::<U18>()
            .expect("initial alloc failure"),
    );
    let mut ut27 = allocator
        .get_untyped::<U27>()
        .expect("second alloc failure");

    smart_alloc!(|slots: local_slots, ut: shared_uts| {
        let (mut local_vspace_scratch, _root_page_directory) =
            VSpaceScratchSlice::from_parts(slots, ut, root_page_directory)?;
        let (mut asid_pool, _asid_control) = asid_control.allocate_asid_pool(ut, slots)?;
    });

    let mut outer_slots = local_slots;
    let mut outer_ut = ut27;

    for c in [
        Command::ReportTrue,
        Command::ReportFalse,
        Command::ThrowFault,
        Command::ReportTrue,
        Command::ThrowFault,
        Command::ReportFalse,
    ]
    .iter()
    .cycle()
    .take(6)
    {
        with_temporary_resources(
            &mut outer_slots,
            &mut outer_ut,
            &mut asid_pool,
            |inner_slots, inner_ut, inner_asid_pool| -> Result<(), TopLevelError> {
                let uts = ut_buddy(inner_ut);
                smart_alloc!(|slots: inner_slots, ut: uts| {
                    let (child_cnode, child_slots) = retype_cnode::<U12>(ut, slots)?;
                    let (child_fault_source_slot, _child_slots) = child_slots.alloc();
                    let (source, sender, handler) = fault_or_message_channel(
                        &root_cnode,
                        ut,
                        slots,
                        child_fault_source_slot,
                        slots,
                    )?;
                    let params = ProcParams {
                        command: c.clone(),
                        sender,
                    };

                    let (child_asid, _asid_pool) = inner_asid_pool.alloc();
                    let child_vspace =
                        VSpace::new(ut, slots, child_asid, &user_image, &root_cnode)?;

                    let (child_process, _) = child_vspace.prepare_thread(
                        proc_main,
                        params,
                        ut,
                        slots,
                        &mut local_vspace_scratch,
                    )?;
                });
                child_process.start(child_cnode, Some(source), root_tcb.as_ref(), 255)?;

                match handler.await_message()? {
                    FaultOrMessage::Fault(_) => {
                        if c != &Command::ThrowFault {
                            panic!("Child process threw a fault when it should not have")
                        } else {
                            debug_println!("Successfully threw and caught a fault");
                        }
                    }
                    FaultOrMessage::Message(m) => match c {
                        Command::ThrowFault => {
                            panic!("Command expected a fault to be thrown, not a message sent")
                        }
                        Command::ReportTrue => {
                            assert_eq!(true, m, "Command expected success true to be reported")
                        }
                        Command::ReportFalse => {
                            assert_eq!(false, m, "Command expected success false to be reported")
                        }
                    },
                }
                Ok(())
            },
        )??;
    }
    debug_println!("Successfully received messages and faults");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub enum Command {
    ReportTrue,
    ReportFalse,
    ThrowFault,
}

pub struct ProcParams<Role: CNodeRole> {
    pub command: Command,
    pub sender: Sender<bool, Role>,
}

impl RetypeForSetup for ProcParams<role::Local> {
    type Output = ProcParams<role::Child>;
}

pub extern "C" fn proc_main(params: ProcParams<role::Local>) {
    debug_println!("\nThe command inside the process is {:?}\n", params.command);
    let ProcParams { command, sender } = params;
    match command {
        Command::ReportTrue => sender.blocking_send(&true).expect("Could not send true"),
        Command::ReportFalse => sender.blocking_send(&false).expect("Could not send false"),
        Command::ThrowFault => {
            unsafe {
                seL4_Send(
                    314159, // bogus cptr to nonexistent endpoint
                    seL4_MessageInfo_new(0, 0, 0, 0),
                );
            }
        }
    }
}