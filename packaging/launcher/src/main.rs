#![windows_subsystem = "windows"]

#[cfg(target_os = "macos")]
mod macos;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os()
        .skip(1)
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect();
    let Some(manfred) = sibling() else {
        return ExitCode::FAILURE;
    };
    match args.first().and_then(|arg| arg.to_str()) {
        Some("--install-cli") => return command_line(&manfred, true),
        Some("--remove-cli") => return command_line(&manfred, false),
        Some("--first-launch") => return first_launch(&manfred),
        _ => {}
    }
    let mut command = Command::new(manfred);
    command.arg("gui").args(args);
    launch(command)
}

fn sibling() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "manfred.exe"
    } else {
        "manfred"
    };
    Some(exe.parent()?.join(name))
}

#[cfg(target_os = "macos")]
fn command_line(manfred: &std::path::Path, install: bool) -> ExitCode {
    let outcome = if install {
        macos::install(manfred)
    } else {
        macos::remove()
    };
    match outcome {
        Ok(done) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            eprintln!("manfred-desk: {refusal}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn command_line(_: &std::path::Path, _: bool) -> ExitCode {
    eprintln!("manfred-desk: --install-cli and --remove-cli are for macOS");
    ExitCode::FAILURE
}

#[cfg(target_os = "macos")]
fn first_launch(manfred: &std::path::Path) -> ExitCode {
    macos::offer(manfred);
    ExitCode::SUCCESS
}

#[cfg(not(target_os = "macos"))]
fn first_launch(_: &std::path::Path) -> ExitCode {
    eprintln!("manfred-desk: --first-launch is for macOS");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn launch(mut command: Command) -> ExitCode {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match command.creation_flags(CREATE_NO_WINDOW).spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(unix)]
fn launch(mut command: Command) -> ExitCode {
    use std::os::unix::process::CommandExt;
    if cfg!(target_os = "macos") {
        let mut path: Vec<PathBuf> = env::var_os("PATH")
            .map(|joined| env::split_paths(&joined).collect())
            .unwrap_or_default();
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !path.iter().any(|entry| entry.as_os_str() == extra) {
                path.push(PathBuf::from(extra));
            }
        }
        if let Ok(joined) = env::join_paths(path) {
            command.env("PATH", joined);
        }
    }
    let error = command.exec();
    eprintln!("manfred-desk: {error}");
    ExitCode::FAILURE
}
