#![no_std]
#![no_main]
#![allow(unreachable_code)]

#[macro_use]
extern crate user_lib;

const STATE_NUM: i32 = 68;

#[no_mangle]
pub fn main() -> i32 {
    let mut state: i32 = 0;
    let mut i: i32 = 0;
    'outer: loop {
        loop {
            match state {
                0 => {
                    state = 1;
                    continue 'outer;
                }
                _ => {
                    loop {
                        state = 1; i += 1;
                        println!("I_STATE: {}, STATE: {}", i, state);
                        if i >= STATE_NUM { break 'outer; }
                        else { continue 'outer; }
                    }
                }
            }
        }
    }
    0
}
