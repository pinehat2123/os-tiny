//! 内存管理模块
mod address;
mod algorithm;
mod config;
mod frame;
mod heap;
mod mapping;

pub use self::config::*;
pub use address::{PhysicalAddress, PhysicalPageNumber, VirtualAddress, VirtualPageNumber};
pub use frame::{frame_alloc, FrameTracker};
pub use mapping::{Flags, MapType, Mapping, MemorySet, Satp, Segment};

pub fn init() {
    heap::init();
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct AddressSpaceId(u16); // in Sv39, [0, 2^16)

impl AddressSpaceId {
    pub(crate) unsafe fn from_raw(asid: usize) -> AddressSpaceId {
        AddressSpaceId(asid as u16)
    }
    pub(crate) fn into_inner(self) -> usize {
        self.0 as usize
    }
}

pub fn max_asid() -> AddressSpaceId {
    let mut val: usize = ((1 << 16) - 1) << 44;
    unsafe {
        core::arch::asm!("
        csrr    {tmp}, satp
        or      {val}, {tmp}, {val}
        csrw    satp, {val}
        csrrw   {val}, satp, {tmp}
    ", tmp = out(reg) _, val = inlateout(reg) val)
    };
    return AddressSpaceId(((val >> 44) & ((1 << 16) - 1)) as u16);
}
