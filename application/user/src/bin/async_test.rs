#![no_std]
#![no_main]

extern crate alloc;
#[macro_use]
extern crate user_lib;

use user_lib::async_lib::{do_yield, execute_async, read_timer, reset_timer, spawn};

async fn a(_x: usize) {}

#[no_mangle]
fn main() -> i32 {
    for i in 0..200 {
        spawn(a(i))
    }
    reset_timer();
    execute_async();
    println!("[async test] coroutines timer: {}", read_timer());
    do_yield(2);
    0
}
