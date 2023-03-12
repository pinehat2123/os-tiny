use super::super::{
    memory::{self, Satp, AddressSpaceId, VirtualAddress, VirtualPageNumber, KERNEL_MAP_OFFSET, SWAP_FRAME_VA, swap_contex_va },
    hart::KernelHartInfo,
    trap::async_tiny::{self, SwapContext},
};


pub unsafe fn get_swap_cx<'cx>(satp: &'cx Satp, asid: usize) -> &'cx mut SwapContext {
    let swap_cx_va = VirtualAddress(memory::swap_contex_va(asid));
    let swap_cx_vpn = VirtualPageNumber::floor(swap_cx_va);
    let swap_cx_ppn = satp.translate(swap_cx_vpn).unwrap();
    // 将物理页号转换成裸指针
    (swap_cx_ppn
        .start_address()
        .0
        .wrapping_add(KERNEL_MAP_OFFSET) as *mut SwapContext)
        .as_mut()
        .unwrap()
}
