use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LINK: &str = "/usr/local/bin/manfred";
const OURS: &str = "Manfred.app/Contents/MacOS/manfred";
const QUESTION: &str = "Install the manfred command, so a terminal can run it?\n\nManfred links /usr/local/bin/manfred to the app and asks for an administrator's password. You can install or remove it later from Help in Manfred's window, or with manfred-desk --install-cli and --remove-cli.";

enum Link {
    Absent,
    Ours(PathBuf),
    Other,
}

fn link() -> Link {
    match fs::read_link(LINK) {
        Ok(target) if target.ends_with(OURS) => Link::Ours(target),
        Ok(_) => Link::Other,
        Err(_) if fs::symlink_metadata(LINK).is_ok() => Link::Other,
        Err(_) => Link::Absent,
    }
}

fn state() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Manfred/cli-link"))
}

fn remember(answer: &str) {
    if let Some(state) = state()
        && let Some(folder) = state.parent()
        && fs::create_dir_all(folder).is_ok()
    {
        let _ = fs::write(state, format!("{answer}\n"));
    }
}

fn in_transit(manfred: &Path) -> bool {
    let text = manfred.to_string_lossy();
    text.starts_with("/Volumes/") || text.contains("/AppTranslocation/")
}

fn shell_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn script_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn as_admin(shell: &str) -> Result<(), String> {
    let script = format!(
        "do shell script {} with administrator privileges",
        script_string(shell)
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("osascript: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if reason.contains("-128") {
        Err("cancelled at the administrator prompt".to_string())
    } else {
        Err(reason)
    }
}

fn ask() -> Option<bool> {
    let script = format!(
        "display dialog {} with title \"Manfred\" buttons {{\"Not now\", \"Install\"}} default button \"Install\" cancel button \"Not now\" with icon note",
        script_string(QUESTION)
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if output.status.success() {
        return Some(String::from_utf8_lossy(&output.stdout).contains("button returned:Install"));
    }
    String::from_utf8_lossy(&output.stderr)
        .contains("-128")
        .then_some(false)
}

pub fn offer(manfred: &Path) {
    let Some(state) = state() else {
        return;
    };
    if state.exists() || in_transit(manfred) || !matches!(link(), Link::Absent) {
        return;
    }
    match ask() {
        Some(true) => remember(if install(manfred).is_ok() {
            "installed"
        } else {
            "declined"
        }),
        Some(false) => remember("declined"),
        None => {}
    }
}

pub fn install(manfred: &Path) -> Result<String, String> {
    if in_transit(manfred) {
        return Err(format!(
            "{} runs from a disk image or a quarantined folder; move Manfred.app to Applications first",
            manfred.display()
        ));
    }
    match link() {
        Link::Ours(target) if target == manfred => {
            remember("installed");
            return Ok(format!("{LINK} already links to {}", manfred.display()));
        }
        Link::Other => {
            return Err(format!(
                "{LINK} is already there and is not Manfred.app's link; remove it first"
            ));
        }
        Link::Ours(_) | Link::Absent => {}
    }
    let target = manfred.to_string_lossy();
    as_admin(&format!(
        "mkdir -p /usr/local/bin && ln -sfn {} {LINK}",
        shell_quoted(&target)
    ))?;
    remember("installed");
    Ok(format!("linked {LINK} to {target}"))
}

pub fn remove() -> Result<String, String> {
    match link() {
        Link::Absent => {
            remember("removed");
            Ok(format!("{LINK} is not installed"))
        }
        Link::Other => Err(format!(
            "{LINK} is not Manfred.app's link; it stays as it is"
        )),
        Link::Ours(_) => {
            as_admin(&format!("rm -f {LINK}"))?;
            remember("removed");
            Ok(format!("removed {LINK}"))
        }
    }
}
