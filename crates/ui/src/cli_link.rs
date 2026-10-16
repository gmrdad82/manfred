use std::path::PathBuf;
use std::process::Command;

const LINK: &str = "/usr/local/bin/manfred";
const OURS: &str = "Manfred.app/Contents/MacOS/manfred";

fn desk() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let desk = exe.parent()?.join("manfred-desk");
    desk.is_file().then_some(desk)
}

fn linked() -> bool {
    std::fs::read_link(LINK).is_ok_and(|target| target.ends_with(OURS))
}

pub(crate) fn menu(ui: &mut egui::Ui) {
    let Some(desk) = desk() else {
        return;
    };
    ui.menu_button("Help", |ui| {
        let (label, flag) = if linked() {
            ("Remove the manfred command", "--remove-cli")
        } else {
            ("Install the manfred command", "--install-cli")
        };
        if ui.button(label).clicked() {
            if let Ok(mut child) = Command::new(&desk).arg(flag).spawn() {
                std::thread::spawn(move || child.wait());
            }
            ui.close();
        }
    });
}
