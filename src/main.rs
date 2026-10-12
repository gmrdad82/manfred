use std::process::ExitCode;

use manfred::cli::{self, Host};

const HOST: Host = Host {
    beat: None,
    usage: "",
    commands: &[],
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    cli::main_installed(&args, &manfred::build::Registry::new(), &HOST)
}
