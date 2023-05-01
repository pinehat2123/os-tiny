#![no_std]
#![no_main]


#[macro_use]
extern crate user_lib;

module! {
    _HelloWorld,
    author: b"rcore",
    description: b"A simple hello world example",
    license: b"MIT"
}

struct _HelloWorld;

// impl KernelModule for HelloWorld {
//     fn init() -> Result<Self, i32> {
//         println!("Hello World");
//         Ok(HelloWorld)
//     }
// }
//
// impl Drop for HelloWorld {
//     fn drop(&mut self) {
//         println!("Bye");
//     }
// }


#[no_mangle]
fn main() {
    println!("Hello World!");
}
