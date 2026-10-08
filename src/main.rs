use std::process::ExitCode;

use manfred::cli::{self, Host, Pulse};
use pfx_report::{Live, Pacing, Surface};

const REPORT_TEST: &str = "--report-test";
const COMMANDS: &[&str] = &["build", "check", "diff", "shot", "render", "clip", "gui"];

struct Beat(Live);

impl Pulse for Beat {
    fn frame(&self, work: std::time::Duration) {
        self.0.frame(work);
    }

    fn idle(&self) {
        self.0.frame_idle();
    }

    fn label(&self, label: &str) {
        self.0.detail(&format!("gui {label}"));
    }
}

fn beat(recipe: Option<&str>) -> Option<Box<dyn Pulse>> {
    let live = pfx_report::live("manfred", cli::VERSION, Surface::Desk, Pacing::OnChange);
    live.detail(&format!("gui {}", recipe.unwrap_or("sample")));
    Some(Box::new(Beat(live)))
}

const HOST: Host = Host {
    beat: Some(beat),
    usage: "Crash reports:\n  manfred --report-test  Crash on purpose to send a test crash report",
    commands: &[],
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let report = pfx_report::start("manfred", cli::VERSION);
    if args.first().map(String::as_str) == Some(REPORT_TEST) {
        report.report_test();
        eprintln!("manfred --report-test: this build sends no crash reports, so nothing was sent");
        return ExitCode::FAILURE;
    }
    let command = args
        .first()
        .map(String::as_str)
        .filter(|word| COMMANDS.contains(word))
        .unwrap_or("other");
    report.breadcrumb("command", command);
    cli::main_installed(&args, &manfred::build::Registry::new(), &HOST)
}
