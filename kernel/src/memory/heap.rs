//! 堆管理
//!
//! 分配一块内存空间用作堆空间使用
use super::config::KERNEL_HEAP_SIZE;
use alloc::alloc::Layout;
use mm::HEAP_ALLOCATOR as HEAP;
use buddy_system_allocator::LockedHeap;

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 全局的堆分配器
// #[global_allocator]
// static HEAP: LockedHeap<32> = LockedHeap::empty();
// static HEAP: LockedHeap = HEAP_ALLOCATOR;
// 
// #[cfg_attr(not(test), alloc_error_handler)]
// #[allow(unused)]
// fn alloc_error_handler(layout: Layout) -> ! {
//     panic!("alloc error for layout {:?}", layout)
// }
// 
/// 初始化堆
pub fn init() {
    unsafe {
        HEAP.lock()
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE)
    }
}
