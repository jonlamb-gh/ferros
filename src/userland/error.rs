#[derive(Debug)]
pub enum SeL4Error {
    UntypedRetype(u32),
    TCBConfigure(u32),
    MapPageTable(u32),
    UnmapPageTable(u32),
    ASIDPoolAssign(u32),
    MapPage(u32),
    UnmapPage(u32),
    CNodeCopy(u32),
    CNodeMint(u32),
    TCBWriteRegisters(u32),
    TCBSetPriority(u32),
    TCBResume(u32),
    CNodeMutate(u32),
    CNodeMove(u32),
    CNodeDelete(u32),
}