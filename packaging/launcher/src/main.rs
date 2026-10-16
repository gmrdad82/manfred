#![windows_subsystem = "windows"]

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Some(manfred) = sibling() else {
        return ExitCode::FAILURE;
    };
    let files: Vec<OsString> = env::args_os()
        .skip(1)
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect();
    let mut command = Command::new(manfred);
    command.arg("gui").args(files);
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
