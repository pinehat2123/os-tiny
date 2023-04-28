#![no_std]
#![no_main]
#![allow(unreachable_code)]

#[macro_use]
extern crate user_lib;

const STATE_NUM: i32 = 68;

#[no_mangle]
pub fn main() -> i32 {
    unsafe {
        static mut STATE: i32 = 0;
        static mut I: i32 = 0;
        'outer: loop {
            loop {
                match STATE {
                    0 => {
                        STATE = 1;
                        continue 'outer;
                    }
                    _ => {
                        loop {
                            STATE = 1; I += 1;
                            println!("I_STATE: {}, STATE: {}", I, STATE);
                            if I >= STATE_NUM { break 'outer; }
                            else { continue 'outer; }
                        }
                    }
                }
            }
        }
    }
    0
}
