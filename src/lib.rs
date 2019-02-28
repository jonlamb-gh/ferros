#![no_std]
#![cfg_attr(feature = "alloc", feature(alloc))]
// Necessary to mark as not-Send or not-Sync
#![feature(optin_builtin_traits)]
#![feature(associated_type_defaults)]
#![recursion_limit = "128"]

#[cfg(all(feature = "alloc"))]
#[macro_use]
extern crate alloc;

extern crate arrayvec;
extern crate generic_array;
extern crate sel4_sys;
#[macro_use]
extern crate typenum;
#[macro_use]
extern crate registers;

extern crate cross_queue;

#[cfg(all(feature = "test"))]
extern crate proptest;

#[cfg(feature = "test")]
pub mod fel4_test;

#[macro_use]
mod debug;

pub mod drivers;

pub mod micro_alloc;
pub mod pow;
mod twinkle_types;
pub mod userland;

mod test_proc;

use crate::micro_alloc::Error as AllocError;
use crate::userland::{
    role, root_cnode, BootInfo, CNode, Consumer1, IPCError, IRQError, LocalCap, MultiConsumerError,
    Producer, SeL4Error, UnmappedPageTable, VSpace, VSpaceError, Waker,
};

use sel4_sys::*;
use typenum::{Diff, U1, U12, U20, U4096};
type U4095 = Diff<U4096, U1>;

fn yield_forever() {
    unsafe {
        loop {
            seL4_Yield();
        }
    }
}

pub fn run(raw_boot_info: &'static seL4_BootInfo) {
    do_run(raw_boot_info).expect("run error");
    yield_forever();
}

fn do_run(raw_boot_info: &'static seL4_BootInfo) -> Result<(), TopLevelError> {
    // wrap all untyped memory
    let mut allocator = micro_alloc::Allocator::bootstrap(&raw_boot_info)?;

    // wrap root CNode for safe usage
    let root_cnode = root_cnode(&raw_boot_info);

    // find an untyped of size 20 bits (1 meg)
    let ut20 = allocator
        .get_untyped::<U20>()
        .expect("initial alloc failure");

    let (ut18, consumer_ut18, producer_ut18, waker_ut18, root_cnode) = ut20.quarter(root_cnode)?;

    let (consumer_ut, consumer_thread_ut, root_cnode) = consumer_ut18.split(root_cnode)?;
    let (producer_ut, producer_thread_ut, root_cnode) = producer_ut18.split(root_cnode)?;
    let (waker_ut, waker_thread_ut, root_cnode) = waker_ut18.split(root_cnode)?;

    let (ut16a, ut16b, ut16c, ut16d, root_cnode) = ut18.quarter(root_cnode)?;

    let (ut14a, _, _, _, root_cnode) = ut16c.quarter(root_cnode)?;
    let (ut12, asid_pool_ut, shared_page_ut, shared_page_ut_b, root_cnode) =
        ut14a.quarter(root_cnode)?;
    let (ut10, scratch_page_table_ut, _, _, root_cnode) = ut12.quarter(root_cnode)?;
    let (ut8, _, _, _, root_cnode) = ut10.quarter(root_cnode)?;
    let (ut6, _, _, _, root_cnode) = ut8.quarter(root_cnode)?;
    let (ut5, _, root_cnode) = ut6.split(root_cnode)?;
    let (ut4a, _, root_cnode) = ut5.split(root_cnode)?; // Why two splits? To exercise split.

    // wrap the rest of the critical boot info
    let (boot_info, root_cnode) = BootInfo::wrap(raw_boot_info, asid_pool_ut, root_cnode);

    // retypes
    let (scratch_page_table, root_cnode): (LocalCap<UnmappedPageTable>, _) =
        scratch_page_table_ut.retype_local(root_cnode)?;
    let (mut scratch_page_table, boot_info) = boot_info.map_page_table(scratch_page_table)?;

    let (consumer_cnode, root_cnode): (LocalCap<CNode<U4095, role::Child>>, _) =
        ut16a.retype_cnode::<_, U12>(root_cnode)?;

    let (producer_cnode, root_cnode): (LocalCap<CNode<U4095, role::Child>>, _) =
        ut16b.retype_cnode::<_, U12>(root_cnode)?;

    let (waker_cnode, root_cnode): (LocalCap<CNode<U4095, role::Child>>, _) =
        ut16d.retype_cnode::<_, U12>(root_cnode)?;

    // vspace setup
    let (consumer_vspace, boot_info, root_cnode) = VSpace::new(boot_info, consumer_ut, root_cnode)?;
    let (producer_vspace, boot_info, root_cnode) = VSpace::new(boot_info, producer_ut, root_cnode)?;

    let (waker_vspace, mut boot_info, root_cnode) = VSpace::new(boot_info, waker_ut, root_cnode)?;

    let (
        consumer,
        consumer_token,
        producer_setup_a,
        waker_setup,
        consumer_cnode,
        consumer_vspace,
        root_cnode,
    ) = Consumer1::new(
        ut4a,
        shared_page_ut,
        consumer_cnode,
        consumer_vspace,
        &mut scratch_page_table,
        &mut boot_info.page_directory,
        root_cnode,
    )?;

    let (consumer, _, consumer_vspace, root_cnode) = consumer.add_queue(
        &consumer_token,
        shared_page_ut_b,
        consumer_vspace,
        &mut scratch_page_table,
        &mut boot_info.page_directory,
        root_cnode,
    )?;

    let consumer_params = test_proc::ConsumerParams::<role::Child> { consumer };

    let (producer_a, producer_cnode, producer_vspace, root_cnode) = Producer::new(
        &producer_setup_a,
        producer_cnode,
        producer_vspace,
        root_cnode,
    )?;
    let producer_params = test_proc::ProducerXParams::<role::Child> {
        producer: producer_a,
    };

    let (waker, waker_cnode) = Waker::new(&waker_setup, waker_cnode, &root_cnode)?;
    let waker_params = test_proc::WakerParams::<role::Child> { waker };

    let (consumer_thread, _consumer_vspace, root_cnode) = consumer_vspace.prepare_thread(
        test_proc::consumer_process,
        consumer_params,
        consumer_thread_ut,
        root_cnode,
        &mut scratch_page_table,
        &mut boot_info.page_directory,
    )?;

    consumer_thread.start(consumer_cnode, None, &boot_info.tcb, 255)?;

    let (producer_thread, _producer_vspace, root_cnode) = producer_vspace.prepare_thread(
        test_proc::producer_x_process,
        producer_params,
        producer_thread_ut,
        root_cnode,
        &mut scratch_page_table,
        &mut boot_info.page_directory,
    )?;

    producer_thread.start(producer_cnode, None, &boot_info.tcb, 255)?;

    let (waker_thread, _waker_vspace, _root_cnode) = waker_vspace.prepare_thread(
        test_proc::waker_process,
        waker_params,
        waker_thread_ut,
        root_cnode,
        &mut scratch_page_table,
        &mut boot_info.page_directory,
    )?;

    waker_thread.start(waker_cnode, None, &boot_info.tcb, 255)?;

    Ok(())
}

#[derive(Debug)]
enum TopLevelError {
    AllocError(AllocError),
    IPCError(IPCError),
    IRQError(IRQError),
    MultiConsumerError(MultiConsumerError),
    SeL4Error(SeL4Error),
    VSpaceError(VSpaceError),
}

impl From<AllocError> for TopLevelError {
    fn from(e: AllocError) -> Self {
        TopLevelError::AllocError(e)
    }
}

impl From<IPCError> for TopLevelError {
    fn from(e: IPCError) -> Self {
        TopLevelError::IPCError(e)
    }
}

impl From<MultiConsumerError> for TopLevelError {
    fn from(e: MultiConsumerError) -> Self {
        TopLevelError::MultiConsumerError(e)
    }
}

impl From<VSpaceError> for TopLevelError {
    fn from(e: VSpaceError) -> Self {
        TopLevelError::VSpaceError(e)
    }
}

impl From<SeL4Error> for TopLevelError {
    fn from(e: SeL4Error) -> Self {
        TopLevelError::SeL4Error(e)
    }
}

impl From<IRQError> for TopLevelError {
    fn from(e: IRQError) -> Self {
        TopLevelError::IRQError(e)
    }
}
