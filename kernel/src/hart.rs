//! 和处理核相关的函数
use crate::{
    memory::{AddressSpaceId, MemorySet, Satp},
    task::async_task::Process,
};
use alloc::{boxed::Box, collections::LinkedList, sync::Arc};

use core::arch::asm;

/// 写一个指针到上下文指针
#[inline]
pub unsafe fn write_tp(tp: usize) {
    asm!("mv tp, {}", in(reg) tp, options(nostack));
}

/// 从tp寄存器读上下文指针
#[inline]
pub fn read_tp() -> usize {
    let tp: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) tp, options(nomem, nostack));
    }; // rust-lang/rust#82753 Thank you @Amanieu :)
    tp
}

/// 用户层将定义自己的tp寄存器意义
///
/// 在内核层中，tp指向一个结构体，说明当前的硬件线程编号，
/// 以及已经分配的地址空间和对应的用户上下文
#[repr(C)]
pub struct KernelHartInfo {
    hart_id: usize,
    current_address_space_id: AddressSpaceId, // currently unused
    current_process: Option<Arc<Process>>,    // currently unused
    hart_max_asid: AddressSpaceId,            // note: different between qemu and k210 platform
    asid_alloc: (LinkedList<usize>, usize),   // (空余的编号回收池，目前已分配最大的编号)
    user_mm_sets: (LinkedList<MemorySet>, usize), // (注册的用户地址空间映射，上一次进入的用户地址空间编号)
}

impl KernelHartInfo {
    /// 准备一个新的核，以供调度器使用
    ///
    /// 在堆上申请一片内存存放[`KernelHartInfo`]数据结构
    /// 这片内存不会马上释放，只有在调用`unload_hart`函数的时候才会释放
    pub unsafe fn load_hart(hart_id: usize) {
        let hart_info = Box::new(KernelHartInfo {
            hart_id,
            current_address_space_id: AddressSpaceId::from_raw(0),
            current_process: None,
            hart_max_asid: crate::memory::max_asid(),
            asid_alloc: (LinkedList::new(), 0), // 0留给内核，其它留给应用,
            user_mm_sets: (LinkedList::new(), 0),
        });
        let tp = Box::into_raw(hart_info) as usize; // todo: 这里有内存泄漏，要在drop里处理
        write_tp(tp)
    }

    /// 热加载/热卸载处理核，释放这个核占用的内存资源
    pub unsafe fn unload_hart() {
        let addr = read_tp();
        let bx: Box<KernelHartInfo> = Box::from_raw(addr as *mut _);
        drop(bx);
    }

    /// 得到当前硬件线程的编号，必须在load_hart之后使用
    pub fn hart_id() -> usize {
        use_tp_box(|b| b.hart_id)
    }

    pub unsafe fn load_address_space_id(asid: AddressSpaceId) {
        use_tp_box(|b| b.current_address_space_id = asid);
    }

    /// 得到当前的地址空间编号
    pub fn current_address_space_id() -> AddressSpaceId {
        use_tp_box(|b| b.current_address_space_id)
    }

    // unused
    pub unsafe fn load_process(process: Arc<Process>) {
        use_tp_box(|b| b.current_process = Some(process.clone()));
    }

    // unused
    pub fn current_process() -> Option<Arc<Process>> {
        use_tp_box(|b| b.current_process.clone())
    }

    /// 分配一个地址空间编号
    pub fn alloc_address_space_id() -> Option<AddressSpaceId> {
        use_tp_box(|b| {
            let (free, max) = &mut b.asid_alloc;
            if let Some(_) = free.front() {
                // 如果链表有内容，返回内容
                return free
                    .pop_front()
                    .map(|idx| unsafe { AddressSpaceId::from_raw(idx) });
            }
            // 如果链表是空的
            if *max < b.hart_max_asid.into_inner() {
                let ans = *max;
                *max += 1;
                Some(unsafe { AddressSpaceId::from_raw(ans) })
            } else {
                None
            }
        })
    }

    /// 释放地址空间编号
    #[allow(unused)]
    pub fn free_address_space_id(asid: AddressSpaceId) {
        use_tp_box(|b| {
            let (free, max) = &mut b.asid_alloc;
            if asid.into_inner() == *max && *max > 0 {
                *max -= 1;
                return;
            } else {
                free.push_back(asid.into_inner())
            }
        });
    }

    /// 添加用户地址空间映射
    ///
    /// 添加成功返回true，否则返回false
    pub fn load_user_mm_set(mm_set: MemorySet) -> bool {
        use_tp_box_move(|b| {
            // 检查链表当前是否有相同地址空间的[`MemorySet`]
            let (link, _prev) = &mut b.user_mm_sets;
            for set in link.iter() {
                if set.address_space_id == mm_set.address_space_id {
                    return false;
                }
            }
            link.push_back(mm_set);
            true
        })
    }

    /// 删除某个用户地址空间映射
    ///
    /// note: feature `linked_list_remove` is not stable
    #[allow(unused)]
    pub unsafe fn unload_user_mm_set(asid: usize) -> Option<MemorySet> {
        use_tp_box(|b| {
            let (link, _prev) = &mut b.user_mm_sets;
            let mut index = 0;
            for set in link.iter() {
                if set.address_space_id.into_inner() == asid {
                    break;
                }
                index += 1;
            }
            if index < link.len() {
                let mm_set = link.remove(index);
                Some(mm_set)
            } else {
                None
            }
        })
    }

    /// 根据地址空间编号找到相应的[`Satp`]结构
    ///
    /// 没有对应的地址空间编号返回[`None`]
    pub fn user_satp(asid: usize) -> Option<Satp> {
        use_tp_box(|b| {
            let (link, _prev) = &b.user_mm_sets;
            for set in link.iter() {
                if set.address_space_id.into_inner() == asid {
                    return Some(set.satp());
                }
            }
            None
        })
    }

    /// 获取上一个进入的用户的[`Satp`]结构
    pub fn prev_satp() -> Option<Satp> {
        let asid = use_tp_box(|b| b.user_mm_sets.1);
        Self::user_satp(asid)
    }

    /// 设置上一次进入的用户地址空间编号
    ///
    /// 用于即将进入用户态
    pub fn set_prev_asid(asid: usize) {
        use_tp_box(|b| b.user_mm_sets.1 = asid)
    }

    /// 获取上一次进入的用户态地址空间编号
    ///
    /// 用于用户陷入内核的时候
    pub fn get_prev_asid() -> usize {
        use_tp_box(|b| b.user_mm_sets.1)
    }
}

#[inline]
fn use_tp_box<F: Fn(&mut Box<KernelHartInfo>) -> T, T>(f: F) -> T {
    let addr = read_tp();
    let mut bx: Box<KernelHartInfo> = unsafe { Box::from_raw(addr as *mut _) };
    let ans = f(&mut bx);
    drop(Box::into_raw(bx)); // 防止Box指向的空间被释放
    ans
}

#[inline]
fn use_tp_box_move<F: FnOnce(&mut Box<KernelHartInfo>) -> T, T>(f: F) -> T {
    let addr = read_tp();
    let mut bx: Box<KernelHartInfo> = unsafe { Box::from_raw(addr as *mut _) };
    let ans = f(&mut bx);
    drop(Box::into_raw(bx)); // 防止Box指向的空间被释放
    ans
}
