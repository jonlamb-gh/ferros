use core::marker::PhantomData;

use selfe_sys::*;

use typenum::*;

use crate::arch::PageBits;
use crate::cap::{CNodeRole, Cap};
use crate::userland::CapRights;

// So, for aarch32, we have 3 levels: PageDirectory -> PageTables ->
// Pages, maybe this could work by implementing things over an
// abstract hierarchy. This is more or less how it works in existing
// Ferros.  Some Notes: In old-vspace, we only need "next" as we
// allocate in-order. There could be some needed state in new-vspace
// land so we know whats mapped and what isn't. Not so much for the
// purposes of accidentally mapping one page over another, but to
// instead be aware of what is already mapped.

// Ah, but no, not really... The kernel already knows all of these
// things. We can just try and see what works, catch errors and roll
// up the necessary mappings.

// the only state we need is the "next page addr"

// we're going to want type-level checking of what can map where,
// while this may be as simple as providing functions like
// `map_huge_page` into the right context, we may also be able to
// explicitly tie (read: index) special mappable items into their
// layers. In fact, why stop at special items? We should be able to do
// it for the branch types as well.

// the item thing is its parent. It's a doubly linked list.

/*===========================================================================*/
/* here lie some things that probably need to be moved in to
 * crate::arch once they make sense */

struct CSpace;
struct Untyped;
struct Page;
struct PageTable;
type PageTableBits = U8;
struct PageDirectory;
type Arm32Paging = PagingRec<Page, PageTable, PagingTop<PageTable, PageDirectory>>;
type Depth = U3;
/* ==========================================================================*/

struct CNode {
    slot_count: usize,
    offset: usize,
}

impl<Role: CNodeRole> Cap<CNode, Role> {
    fn new(
        ut: Cap<Untyped, Local>,
        guard: usize,
        radix: usize,
        callers_cnode: Cap<CNode, Local>,
    ) -> Self {
        unsafe {
            // Retype to fill the scratch slot with a fresh CNode
            let err = seL4_Untyped_Retype(
                ut.cptr(),                               // _service
                api_object_seL4_CapTableObject as usize, // type
                hildRadix::to_usize(),                   // size_bits
                callers_cnode.cptr(),                    // root
                0,                                       // index
                0,                                       // depth
                callers_cnode.offset,                    // offset
                1,                                       // num_objects
            );

            if err != 0 {
                return Err(SeL4Error::CNodeMutate(err));
            }

            // In order to set the guard (for the sake of our C-pointer simplification scheme),
            // mutate the CNode in the scratch slot, which copies the CNode into a second slot
            let guard_data = seL4_CNode_CapData_new(
                0,                                                      // guard
                (seL4_WordBits - ChildRadix::to_usize() as usize) as _, // guard size in bits
            )
            .words[0];

            let err = seL4_CNode_Mutate(
                dest_cptr,           // _service: seL4_CNode,
                dest_offset,         // dest_index: seL4_Word,
                seL4_WordBits as u8, // dest_depth: seL4_Uint8,
                scratch_cptr,        // src_root: seL4_CNode,
                scratch_offset,      // src_index: seL4_Word,
                seL4_WordBits as u8, // src_depth: seL4_Uint8,
                guard_data as usize, // badge or guard: seL4_Word,
            );

            // TODO - If we wanted to make more efficient use of our available slots at the cost
            // of complexity, we could swap the two created CNodes, then delete the one with
            // the incorrect guard (the one originally occupying the scratch slot).

            if err != 0 {
                return Err(SeL4Error::UntypedRetype(err));
            }
        }
    }
}

trait Arch {
    type Root: CapType;
}

trait Maps<M> {
    fn map_item(&mut self, item: M) -> Result<(), MappingError>;
}

enum MappingError {
    Overflow,

    // TODO(dan@auxon.io)
    DidntWork,
}

// TODO: a way to borrow caps but put them back if we don't use them.

/// `PagingLayer` is a type-level induction over the layers of a
/// paging structure. For ease-of-use purposes, the outer-most layer
/// is a page and the inner most is the root of the paging structure.
///
/// This allows for monadic-like exit-early semantics when mapping an
/// item:
// Actually, it's not induction, it's an indexed monad: the state is
// the layer. We only need the `next` relation, not the whole thing.
// Continuing that idea, this doesn't necessarily abstract away
// anything, each structure will still need a concrete
// implementation. It's an algebra, not parametricity.
trait PagingLayer {
    type Item;
    fn map_item(&mut self, item: Self::Item) -> Result<(), MappingError>;
}

struct PagingTop<M, L: Maps<M>> {
    layer: L,
    _item: PhantomData<M>,
}

impl<M, L: Maps<M>> PagingLayer for PagingTop<M, L> {
    type Item = M;
    fn map_item(&mut self, item: M) -> Result<(), MappingError> {
        self.layer.map_item(item)
    }
}

struct PagingRec<M, L: Maps<M>, P: PagingLayer> {
    layer: L,
    next: P,
    _item: PhantomData<M>,
}

impl<M, L: Maps<M>, P: PagingLayer> PagingLayer for PagingRec<M, L, P> {
    type Item = M;
    fn map_item(&mut self, item: M) -> Result<(), MappingError> {
        match self.layer.map_item(item) {
            Err(MappingError::Overflow) => {
                self.next.map_item(next_item)?;
                self.layer.map_item(item)
            }

            res => res,
        }
    }
}

struct VSpace<A: Arch, L: PagingLayer> {
    // TODO(dan@auxon.io): May not need this.
    _arch: PhantomData<A>,

    layers: L,
    // A page-aligned virtual address to map the next page into when
    // mapping pages without specific addresses.
    next_page: usize,
}

impl<A: Arch, L: PagingLayer> VSpace<A, L> {
    pub fn map_page(
        &mut self,
        page: Page,
        _rights: CapRights,
        _attrs: u32,
    ) -> Result<(), MappingError> {
        self.next_page = self.next_page + (1 << PageBits::USIZE);
        unimplemented!();
    }
}
