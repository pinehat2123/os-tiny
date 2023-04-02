use clap::{arg, Arg, Command};
use std::path::PathBuf;

fn cli() -> Command {
    Command::new("xtask")
        .about("xtask utils")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .allow_external_subcommands(true)
        .subcommand(
            // TODO: this is git
            Command::new("git")
                .about("A fictional versioning CLI")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .allow_external_subcommands(true)
                .subcommand(
                    Command::new("clone")
                        .about("Clones repos")
                        .arg(arg!(<REMOTE> "The remote to clone"))
                        .arg_required_else_help(true),
                )
                .subcommand(
                    Command::new("diff")
                        .about("Compare two commits")
                        .arg(arg!(base: [COMMIT]))
                        .arg(arg!(head: [COMMIT]))
                        .arg(arg!(path: [PATH]).last(true))
                        .arg(
                            arg!(--color <WHEN>)
                                .value_parser(["always", "auto", "never"])
                                .num_args(0..=1)
                                .require_equals(true)
                                .default_value("auto")
                                .default_missing_value("always"),
                        ),
                )
                .subcommand(
                    Command::new("push")
                        .about("pushes things")
                        .arg(arg!(<REMOTE> "The remote to target"))
                        .arg_required_else_help(true),
                )
                .subcommand(
                    Command::new("add")
                        .about("adds things")
                        .arg_required_else_help(true)
                        .arg(
                            arg!(<PATH> ... "Stuff to add")
                                .value_parser(clap::value_parser!(PathBuf)),
                        ),
                )
                .subcommand(
                    Command::new("stash")
                        .args_conflicts_with_subcommands(true)
                        .args(push_args())
                        .subcommand(Command::new("push").args(push_args()))
                        .subcommand(Command::new("pop").arg(arg!([STASH])))
                        .subcommand(Command::new("apply").arg(arg!([STASH]))),
                ),
        )
        // TODO: This is cargo
        .subcommand(
            Command::new("cargos")
                .about("Wrap cargo useage")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .allow_external_subcommands(true),
        )
        // TODO: This is tiny
        .subcommand(
            Command::new("tiny")
                .about("Deal With Tiny")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .allow_external_subcommands(true)
                // For kernel compile
                .subcommand(
                    // CMD::new("/usr/bin/cargo").arg("tiny").arg("kernel").arg(format!("--target={}", <TARGET>)).arg(format!("--{}", <BUILDMODE>)).status().expect("tiny kernel build error.")
                    Command::new("kernel")
                        .about("This is the kernel compile requirement")
                        .arg_required_else_help(true)
                        .arg(arg!(<TARGET> "the target of kernel"))
                        .arg(arg!(<BUILDMODE> "the build mode of kernel")),
                )
                // For ring_scheduler compile
                .subcommand(
                    Command::new("ring_scheduler")
                        .about("Ring scheduler")
                        .arg_required_else_help(true)
                        .arg(arg!(<TARGET> "the target of ring_scheduler"))
                        .arg(arg!(<BUILDMODLE> "the buiild mode of kernel")),
                )
                .subcommand(
                    Command::new("qemu")
                        .about("Qemu Run")
                        .arg_required_else_help(true)
                        .arg(Arg::new("plantform").short('p').help("plantform specify")),
                )
                // For env build
                .subcommand(Command::new("env")),
        )
}

fn push_args() -> Vec<clap::Arg> {
    vec![arg!(-m --message <MESSAGE>)]
}

use std::process::Command as CM;
fn main() {
    let matches = cli().get_matches();
    match matches.subcommand() {
        Some(("git", sub_matches)) => match sub_matches.subcommand() {
            Some(("clone", sub_matches)) => {
                println!(
                    "Clone {}",
                    sub_matches.get_one::<String>("REMOTE").expect("required")
                );
            }
            _ => {}
        },
        Some(("tiny", sub_matches)) => match sub_matches.subcommand() {
            Some(("qemu", sub_matches)) => {
                if sub_matches.contains_id("plantform") {
                    let plantform = sub_matches
                        .get_one::<String>("plantform")
                        .expect("required");
                    println!("Plantform {}", plantform);
                    match &plantform as &str {
                        "riscv64" => {
                            CM::new("/usr/bin/make")
                                .arg("run")
                                .status()
                                .expect("Run tiny error");
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}
