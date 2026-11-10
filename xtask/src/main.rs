//! Orquestrador de tarefas do monorepo (ADR-008): `cargo xtask <comando>`.
use std::process::{exit, Command};

fn main() {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "fmt" => run("cargo", &["fmt", "--all", "--check"]),
        "lint" => run(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        "test" => run("cargo", &["test", "--workspace"]),
        "ci" => {
            run("cargo", &["fmt", "--all", "--check"])
                && run(
                    "cargo",
                    &[
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                )
                && run("cargo", &["test", "--workspace"])
        }
        _ => {
            eprintln!("uso: cargo xtask <fmt|lint|test|ci>");
            exit(2);
        }
    };
    if !ok {
        exit(1);
    }
}

fn run(program: &str, args: &[&str]) -> bool {
    println!("> {program} {}", args.join(" "));
    Command::new(program)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
