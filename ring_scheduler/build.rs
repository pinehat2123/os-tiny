use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // let platform = env::var("PLATFORM").expect("no specified platform");
    let platform = String::from("qemu");
    // Put the linker script somewhere the linker can find it
    let mut linker = fs::File::create(out_dir.join("linker.ld")).unwrap();
    match platform.as_str() {
        "qemu" => linker
            .write_all(include_bytes!("src/linker.ld"))
            .unwrap(),
        p => panic!("haven't supported platform: {}", p),
    }
    println!("cargo:rustc-link-search={}", out_dir.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/linker.ld");
}
