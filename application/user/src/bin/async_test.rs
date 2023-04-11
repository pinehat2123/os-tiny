#![no_std]
#![no_main]

extern crate alloc;
#[macro_use]
extern crate user_lib;

// use user_lib::async_lib::{do_yield, execute_async, read_timer, reset_timer, job_add};
use user_lib::async_lib::task::wokes::Executor;


#[no_mangle]
fn main() -> i32 {
    // for i in 0..200 {
    //     spawn(a(i))
    // }
    // job_add(a(0));
    // reset_timer();
    // execute_async();
    // println!("[async test] coroutines timer: {}", read_timer());
    // do_yield(2);
    let executor = Executor::default();
    for _ in 1..20 {
        executor.spawn(async {
            println!("[userspace async] Hello world!")
        });
    }

    executor.run_until_idle();
    0
}
