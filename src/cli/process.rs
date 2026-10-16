use std::process::{Command, ExitCode};

pub(super) const CPUS: &str = "MANFRED_CPUS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Machine<'a> {
    pub(super) queued: bool,
    pub(super) pgpu: bool,
    pub(super) systemd: bool,
    pub(super) cpus: Option<&'a str>,
}

pub(super) fn line(
    command: &str,
    exe: &str,
    args: &[String],
    machine: Machine<'_>,
) -> Option<Vec<String>> {
    if machine.queued || !machine.pgpu {
        return None;
    }
    let mut line: Vec<String> = Vec::new();
    if machine.systemd {
        line.extend(
            [
                "systemd-run",
                "--user",
                "--quiet",
                "--scope",
                "--collect",
                "--slice=build.slice",
                "-p",
                "MemoryMax=12G",
                "-p",
                "MemorySwapMax=0",
                "--",
            ]
            .map(String::from),
        );
        if let Some(cpus) = machine.cpus {
            line.extend(["taskset", "-c", cpus].map(String::from));
        }
        line.extend(["nice", "-n", "10"].map(String::from));
    }
    line.extend(["pgpu", "run", "--class", "clip", "--as", "manfred", "--"].map(String::from));
    line.push(exe.to_string());
    line.push(command.to_string());
    line.extend(args.iter().cloned());
    Some(line)
}

pub(super) fn on_path(name: &str) -> bool {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(&file).is_file()))
}

pub(super) fn real(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let real = path.canonicalize()?;
    Ok(
        match real.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
            Some(rest) if cfg!(windows) && rest.as_bytes().get(1) == Some(&b':') => rest.into(),
            _ => real,
        },
    )
}

pub(super) fn cpus(value: Option<&std::ffi::OsStr>) -> Option<String> {
    value
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn unqueued(command: &str, direct: &str, machine: Machine<'_>) -> Option<String> {
    (!machine.queued && !machine.pgpu).then(|| {
        format!(
            "manfred {command}: no GPU queue (pgpu) on the PATH, so the {command} {direct}, paced the same"
        )
    })
}

pub(super) fn requeue(command: &str, args: &[String], direct: &str) -> Option<ExitCode> {
    let cpus = on_path("taskset")
        .then(|| {
            cpus(std::env::var_os(CPUS).as_deref())
                .or_else(|| cpus(option_env!("MANFRED_CPUS").map(std::ffi::OsStr::new)))
        })
        .flatten();
    let machine = Machine {
        queued: std::env::var_os("GPU_QUEUE").is_some(),
        pgpu: on_path("pgpu"),
        systemd: on_path("systemd-run"),
        cpus: cpus.as_deref(),
    };
    if let Some(note) = unqueued(command, direct, machine) {
        eprintln!("{note}");
    }
    let exe = std::env::current_exe().ok()?;
    let line = line(command, &exe.display().to_string(), args, machine)?;
    Some(run(command, &line))
}

#[cfg(unix)]
fn run(command: &str, line: &[String]) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let error = Command::new(&line[0]).args(&line[1..]).exec();
    eprintln!("manfred {command}: cannot start {}: {error}", line[0]);
    ExitCode::FAILURE
}

#[cfg(not(unix))]
fn run(command: &str, line: &[String]) -> ExitCode {
    match Command::new(&line[0]).args(&line[1..]).status() {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("manfred {command}: cannot start {}: {error}", line[0]);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn machine(queued: bool, pgpu: bool) -> Machine<'static> {
        Machine {
            queued,
            pgpu,
            systemd: false,
            cpus: None,
        }
    }

    #[test]
    fn without_the_queue_the_render_runs_directly_with_one_plain_note() {
        let args = vec!["a.toml".to_string()];
        let absent = machine(false, false);
        assert_eq!(line("render", "manfred", &args, absent), None);
        assert_eq!(
            unqueued("render", "runs directly", absent).as_deref(),
            Some(
                "manfred render: no GPU queue (pgpu) on the PATH, so the render runs directly, paced the same"
            )
        );
        assert_eq!(
            unqueued("render", "runs directly", machine(false, true)),
            None
        );
        assert_eq!(
            unqueued("render", "runs directly", machine(true, false)),
            None
        );
    }

    #[test]
    fn the_cpu_set_comes_only_from_its_variable() {
        assert_eq!(cpus(None), None);
        assert_eq!(cpus(Some(OsStr::new(""))), None);
        assert_eq!(cpus(Some(OsStr::new("  "))), None);
        assert_eq!(cpus(Some(OsStr::new(" 2-5 "))).as_deref(), Some("2-5"));
        assert_eq!(CPUS, "MANFRED_CPUS");
    }

    #[test]
    fn a_tool_that_is_not_on_the_path_is_absent() {
        assert!(!on_path("manfred-no-such-tool-anywhere"));
    }
}
