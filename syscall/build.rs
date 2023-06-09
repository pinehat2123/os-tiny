fn main() {
    use std::{
        fs::{self, File},
        io::Write,
    };

    const SYSCALL_H_IN: &str = "src/syscall.h.in";
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={SYSCALL_H_IN}");

    let mut fout = File::create("src/syscalls.rs").unwrap();
    writeln!(
        fout,
        "\
//! Generated by build.rs. DO NOT EDIT.
impl crate::SyscallId {{"
    )
    .unwrap();
    fs::read_to_string(SYSCALL_H_IN)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("#define __NR_"))
        .filter_map(|line| line.split_once(' '))
        .for_each(|(name, num)| {
            writeln!(
                fout,
                "    pub const {name}: Self = Self({num});",
                name = name.to_uppercase()
            )
            .unwrap();
        });
    writeln!(fout, "}}").unwrap();
}
