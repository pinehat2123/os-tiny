//! 内核异步运行时实现
//!
//! 目前包含共享调度器实例化和内核执行器两个模块。
//!
//! Rust异步运行时是不包含在标准库里面的，交给社区贡献者实现，通常包含以下几个方面：
//!
//! * Future: 标准库提供`Future` trait，封装一个`poll`方法
//! * executor: `Future`的具体运行者
//! * reactor: `Future`的唤醒者
//!
//! 目前飓风内核里面的异步运行时主要是内核执行器，其配合共享调度器进行执行任务的工作。
//!
//! 在中断处理函数或者系统调用处理函数里面存在任务唤醒机制。
#[cfg(feature = "async_tiny")]
mod executor;
#[cfg(feature = "async_tiny")]
mod shared;

use config::SHAREDPAYLOAD_BASE;
#[cfg(feature = "async_tiny")]
pub use executor::{ext_intr_off, ext_intr_on, run_one, run_until_idle};
#[cfg(feature = "async_tiny")]
pub use shared::{kernel_should_switch, SharedPayload, TaskState};

#[cfg(feature = "async_tiny")]
pub(crate) mod syscall;
#[cfg(all(features = ["async_tiny", "async_test"]))]
pub fn init() {
     extern "C" {
        static mut _sbss: u32;
        static mut _ebss: u32;

        static mut _sdata: u32;
        static mut _edata: u32;

        static mut _stext: u32;
        static mut _srodata: u32;
        static mut _erodata: u32;
    }
    let kernel_memory = crate::memory::MemorySet::new_kernel().expect("create kernel memory set");
    kernel_memory.activate();
    let shared_payload = unsafe { SharedPayload::load(SHAREDPAYLOAD_BASE) };
    let process = crate::task::async_task::Process::new(kernel_memory).expect("create process 1");
    let hart_id = crate::hart::KernelHartInfo::hart_id();
    let address_space_id = process.address_space_id();
    let _stack_handle = process.alloc_stack().expect("alloc initial stack");
    #[allow(unused)]
    let task_1 = crate::task::async_task::new_kernel(
        task_1(),
        process.clone(),
        shared_payload.shared_scheduler,
        shared_payload.shared_set_task_state,
    );
    unsafe {
        shared_payload.add_task(hart_id, address_space_id, task_1.task_repr());
    }
    run_until_idle(
        || unsafe { shared_payload.peek_task(kernel_should_switch) },
        |task_repr| unsafe { shared_payload.delete_task(task_repr) },
        |task_repr, new_state| unsafe { shared_payload.set_task_state(task_repr, new_state) },
    );
}

async fn task_1() {
    println!("Hello World");
}
