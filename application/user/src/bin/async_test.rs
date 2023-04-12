#![no_std]
#![no_main]

extern crate alloc;
#[macro_use]
extern crate user_lib;

use user_lib::{async_lib::task::wokes::Executor};
use core::{future::Future, pin::Pin, task::{Context, Poll}};

struct FibonacciFuture {
    predecessor: usize,
    successor: usize,
    index: usize,
    count: usize,
}

impl FibonacciFuture {
    fn new (count: usize) -> FibonacciFuture {
        FibonacciFuture { predecessor: 0, successor: 1, index: 0, count }
    }
}

impl Future for FibonacciFuture {
    type Output = usize;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.index == self.count {
            // println!("Fibonacci {} result: {}", self.count, self.predecessor);
            Poll::Ready(self.predecessor)
        } else {
            let tmp = self.predecessor;
            self.predecessor += self.successor;
            self.successor = tmp;
            self.index += 1;
            println!("[user space][async test]Fibonacci {}; index = {}, predecessor = {}, successor = {}", self.count, self.index, self.predecessor, self.successor);
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[no_mangle]
fn main() -> i32 {
    let executor = Executor::default();
    // for i in (1..=50).rev() {
    //     executor.spawn(async move {
    //         let ans = FibonacciFuture::new(i).await;
    //         println!("[user space] Fibonacci[{}] = {}", i, ans);
    //     });
    // }
    executor.spawn(async {
        let i = 50;
        let ans = FibonacciFuture::new(i).await;
        println!("[user space] Fibonacci[{}] = {}", i, ans);
    });
    executor.spawn(async {
        let i = 11;
        let ans = FibonacciFuture::new(i).await;
        println!("[user space] Fibonacci[{}] = {}", i, ans);
    });
    executor.spawn(async {
        let i = 1;
        let ans = FibonacciFuture::new(i).await;
        println!("[user space] Fibonacci[{}] = {}", i, ans);
    });
    executor.run_until_idle();
    0
}
