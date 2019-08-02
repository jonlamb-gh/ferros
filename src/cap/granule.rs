use core::op::Sub;

use typenum::*;

use crate::arch::{G1, G2, G3, G4};
use crate::cap::{CapType, DirectRetype, LocalCap};
use crate::{IfThenElse, _IfThenElse};

/// The type returned by the architecture specific implementations of
/// `determine_best_granule_fit`.
pub(crate) struct GranuleInfo {
    /// This granule's size in bits (radix in seL4 parlance).
    size_bits: u8,
    /// How many of them do I need to do this?
    count: u16,
}

/// An abstract way of thinking about the leaves in paging structures
/// across architectures. A Granule can be a Page, a LargePage, a
/// Section, &c.
pub struct Granule<State: GranuleState> {
    /// The size of this granule in bits.
    size_bits: u8,
    /// The seL4 object id.
    type_id: usize,
    /// Is this granule mapped or unmapped and the state that goes
    /// along with that.
    state: State,
}

pub trait GranuleState:
    private::SealedGranuleState + Copy + Clone + core::fmt::Debug + Sized + PartialEq
{
}

pub mod granule_state {
    use crate::cap::asid_pool::InternalASID;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Mapped {
        pub(crate) vaddr: usize,
        pub(crate) asid: InternalASID,
    }
    impl super::GranuleState for Mapped {}

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Unmapped;
    impl super::GranuleState for Unmapped {}
}

impl<State: GranuleState> CapType for Granule<State> {}

impl DirectRetype for LocalCap<Granule<granule_state::Unmapped>> {
    // `SizeBits` is unused for a Granule; it has a custom
    // implementation of `size_bits`.
    type SizeBits = U0;

    // Same for `sel4_type_id`. It is not used in Granule's case,
    // granule implements `type_id`.
    fn sel4_type_id() -> usize {
        usize::MAX
    }

    fn type_id(&self) -> usize {
        self.type_id
    }

    fn size_bits(&self) -> usize {
        self.size_bits
    }
}

mod private {
    pub trait SealedGranuleState {}
    impl SealedGranuleState for super::granule_state::Unmapped {}
    impl SealedGranuleState for super::granule_state::Mapped {}
}

trait IToUUnsafe: Integer {
    type Uint: Unsigned;
}

impl<U: Unsigned + NonZero> IToUUnsafe for PInt<U> {
    type Uint = U;
}

impl<U: Unsigned + NonZero> IToUUnsafe for NInt<U> {
    type Uint = U0;
}

impl IToUUnsafe for Z0 {
    type Uint = U0;
}

trait _DetermineBestGranuleFit {
    type Count: Unsigned;
}

type DetermineBestGranuleFit<U> = <U as _DetermineBestGranuleFit>::Count;

impl<U: Unsigned> _DetermineBestGranuleFit for U
where
    // The incoming size must be NonZero; this is a requirement for
    // putting it into a positive int. More on that later.
    U: NonZero,

    // We need to be able to subtract G1..G4 from U, forall U; some of
    // these results may be negative. Therefore we wrap a positiive
    // signed integer around U and do the subtraction from there
    // instead.
    PInt<U>: Sub<PInt<G1>>,
    <PInt<U> as Sub<PInt<G1>>>::Output: IToUUnsafe,

    PInt<U>: Sub<PInt<G2>>,
    <PInt<U> as Sub<PInt<G2>>>::Output: IToUUnsafe,

    PInt<U>: Sub<PInt<G3>>,
    <PInt<U> as Sub<PInt<G3>>>::Output: IToUUnsafe,

    PInt<U>: Sub<PInt<G4>>,
    <PInt<U> as Sub<PInt<G4>>>::Output: IToUUnsafe,

    // U must be comparable with G1..G4 and actually greater than G4,
    // the smallest granule.
    U: IsGreaterOrEqual<G1>,
    U: IsGreaterOrEqual<G2>,
    U: IsGreaterOrEqual<G3>,
    U: IsGreaterOrEqual<G4, Output = True>,

    // Okay, now the conditionals. The next three constraints allow us
    // to write the following algorithm:
    //
    // if U >= G1 then
    //   return U - G1
    // else
    //   if U >= G2 then
    //     return U - G2
    //   else
    //     if U >= G3 then
    //       return U - G3
    //     else
    //       return U - G4
    <U as IsGreaterOrEqual<G1>>::Output: _IfThenElse<
        <<PInt<U> as Sub<PInt<G1>>>::Output as IToUUnsafe>::Uint,
        IfThenElse<
            <U as IsGreaterOrEqual<G2>>::Output,
            <<PInt<U> as Sub<PInt<G2>>>::Output as IToUUnsafe>::Uint,
            IfThenElse<
                <U as IsGreaterOrEqual<G3>>::Output,
                <<PInt<U> as Sub<PInt<G3>>>::Output as IToUUnsafe>::Uint,
                <<PInt<U> as Sub<PInt<G4>>>::Output as IToUUnsafe>::Uint,
            >,
        >,
    >,

    <U as IsGreaterOrEqual<G2>>::Output: _IfThenElse<
        <<PInt<U> as Sub<PInt<G2>>>::Output as IToUUnsafe>::Uint,
        IfThenElse<
            <U as IsGreaterOrEqual<G3>>::Output,
            <<PInt<U> as Sub<PInt<G3>>>::Output as IToUUnsafe>::Uint,
            <<PInt<U> as Sub<PInt<G4>>>::Output as IToUUnsafe>::Uint,
        >,
    >,

    <U as IsGreaterOrEqual<G3>>::Output: _IfThenElse<
        <<PInt<U> as Sub<PInt<G3>>>::Output as IToUUnsafe>::Uint,
        <<PInt<U> as Sub<PInt<G4>>>::Output as IToUUnsafe>::Uint,
    >,

    // This last one says the whole thing will ultimately give us an
    // Unsigned, which is what we need to parameterize
    // `CNodeSlots::alloc`.
    IfThenElse<
        <U as IsGreaterOrEqual<G1>>::Output,
        <<PInt<U> as Sub<PInt<G1>>>::Output as IToUUnsafe>::Uint,
        IfThenElse<
            <U as IsGreaterOrEqual<G2>>::Output,
            <<PInt<U> as Sub<PInt<G2>>>::Output as IToUUnsafe>::Uint,
            IfThenElse<
                <U as IsGreaterOrEqual<G3>>::Output,
                <<PInt<U> as Sub<PInt<G3>>>::Output as IToUUnsafe>::Uint,
                <<PInt<U> as Sub<PInt<G4>>>::Output as IToUUnsafe>::Uint,
            >,
        >,
    >: Unsigned,
{
    type Count = IfThenElse<
        <U as IsGreaterOrEqual<G1>>::Output,
        <<PInt<U> as Sub<PInt<G1>>>::Output as IToUUnsafe>::Uint,
        IfThenElse<
            <U as IsGreaterOrEqual<G2>>::Output,
            <<PInt<U> as Sub<PInt<G2>>>::Output as IToUUnsafe>::Uint,
            IfThenElse<
                <U as IsGreaterOrEqual<G3>>::Output,
                <<PInt<U> as Sub<PInt<G3>>>::Output as IToUUnsafe>::Uint,
                <<PInt<U> as Sub<PInt<G4>>>::Output as IToUUnsafe>::Uint,
            >,
        >,
    >;
}