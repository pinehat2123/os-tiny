#[allow(dead_code)]
mod syscall;
#[allow(dead_code)]
mod task;

static mut SHARED_PAYLOAD_BASE: usize = 0;
static mut ADDRESS_SPACE_ID: usize = 0;

use core::future::Future;

pub fn execute_async_main(main: impl Future<Output = i32> + Send + Sync + 'static) -> i32 {
    let hart_id = 0usize;
    let shared_payload = unsafe { task::shared::SharedPayload::new(SHARED_PAYLOAD_BASE) };
    let address_space_id = unsafe { task::shared::AddressSpaceId::from_raw(ADDRESS_SPACE_ID) };
    static mut EXIT_CODE: i32 = 0;
    let main_task = task::new_user(
        async move { unsafe { EXIT_CODE = main.await } },
        shared_payload.shared_scheduler,
        shared_payload.shared_set_task_state,
    );

    unsafe {
        shared_payload.add_task(hart_id, address_space_id, main_task.task_repr());
    }

    task::shared::run_until_ready(
        || unsafe { shared_payload.peek_task(task::shared::user_should_switch) },
        |task_reper| unsafe { shared_payload.delete_task(task_reper) },
        |task_repr, new_state| unsafe { shared_payload.set_task_state(task_repr, new_state) },
    );

    unsafe { EXIT_CODE }
}

pub fn spawn(future: impl Future<Output = ()> + Send + Sync + 'static) {
    let shared_payload = unsafe { task::shared::SharedPayload::new(SHARED_PAYLOAD_BASE) };
    let asid = unsafe { task::shared::AddressSpaceId::from_raw(ADDRESS_SPACE_ID) };
    let task = task::new_user(
        future,
        shared_payload.shared_scheduler,
        shared_payload.shared_set_task_state,
    );
    unsafe {
        shared_payload.add_task(0 /* todo */, asid, task.task_repr());
    }
}

pub fn execute_async() {
    let shared_payload = unsafe { task::shared::SharedPayload::new(SHARED_PAYLOAD_BASE) };
    task::shared::run_until_ready(
        || unsafe { shared_payload.peek_task(task::shared::user_should_switch) },
        |task_repr| unsafe { shared_payload.delete_task(task_repr) },
        |task_repr, new_state| unsafe { shared_payload.set_task_state(task_repr, new_state) },
    );
}

use syscall::sys_yield;

pub fn do_yield(next_asid: usize) -> usize {
    sys_yield(next_asid)
}
