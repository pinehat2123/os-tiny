// mod executor;
//
// mod shared;
//
// pub use executor::{ext_intr_off, ext_intr_on, run_one, run_until_idle};
// pub use shared::{kernel_should_switch, SharedPayload, TaskState};

use crate::console;

pub fn init() {
    println!("async_rt INIT");
}
