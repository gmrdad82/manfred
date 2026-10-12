use egui::{
    Align, Align2, CentralPanel, Frame, Id, Layout, Pos2, Rect, Sense, Ui, UiBuilder, Vec2,
};

use crate::shell::{Shell, TABS};
use crate::theme::*;
use crate::widgets::{Dotted, Header, KeyCap, Number, Pill, Tabs, Tone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    Header,
    Tabs,
    KeyCap,
    Pill,
    Dotted,
    Number,
}

pub const SIZE: [u32; 2] = [600, 110];
pub const STATES: [&str; 3] = ["rest", "hover", "active"];
const ROW: f32 = 70.0;

impl Fixture {
    pub const ALL: [Fixture; 6] = [
        Fixture::Header,
        Fixture::Tabs,
        Fixture::KeyCap,
        Fixture::Pill,
        Fixture::Dotted,
        Fixture::Number,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Fixture::Header => "header",
            Fixture::Tabs => "tabs",
            Fixture::KeyCap => "keycap",
            Fixture::Pill => "pill",
            Fixture::Dotted => "dotted",
            Fixture::Number => "number",
        }
    }

    pub fn from_name(name: &str) -> Option<Fixture> {
        Fixture::ALL.into_iter().find(|f| f.name() == name)
    }

    pub fn size(self) -> [u32; 2] {
        SIZE
    }

    pub fn script(self) -> &'static str {
        match self {
            Fixture::Tabs => include_str!("../fixtures/tabs.txt"),
            _ => include_str!("../fixtures/cells.txt"),
        }
    }
}

fn cell(index: usize) -> Rect {
    let width = SIZE[0] as f32 / 3.0;
    Rect::from_center_size(
        Pos2::new(width * (index as f32 + 0.5), ROW),
        Vec2::new(width - 40.0, 24.0),
    )
}

pub fn ui(shell: &mut Shell, fixture: Fixture, ui: &mut Ui) {
    let ctx = &ui.ctx().clone();
    CentralPanel::default()
        .frame(Frame::NONE.fill(BG_BASE))
        .show(ui, |ui| {
            let painter = ui.painter();
            painter.text(
                Pos2::new(12.0, 12.0),
                Align2::LEFT_TOP,
                fixture.name(),
                Theme::of(ctx).font(SIZE_CODE),
                TEXT_DIM,
            );
            if fixture == Fixture::Tabs {
                painter.text(
                    Pos2::new(20.0, 34.0),
                    Align2::LEFT_TOP,
                    "Scene rest · Cameras hover · Lights active",
                    Theme::of(ctx).font(SIZE_CODE),
                    TEXT_MUTED,
                );
                let rect = Rect::from_min_size(Pos2::new(20.0, ROW - 12.0), Vec2::new(560.0, 24.0));
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                child.add(Tabs::new(&mut shell.chosen, &TABS));
                return;
            }
            for (index, state) in STATES.iter().enumerate() {
                let slot = cell(index);
                ui.painter().text(
                    Pos2::new(slot.center().x, 34.0),
                    Align2::CENTER_TOP,
                    state,
                    Theme::of(ctx).font(SIZE_CODE),
                    TEXT_MUTED,
                );
                let active = shell.chosen == Some(index);
                let response = match fixture {
                    Fixture::Header => ui.put(
                        slot,
                        Header::new("B1", "outliner").info("69").focused(active),
                    ),
                    Fixture::KeyCap => ui.put(slot, KeyCap::new("b1").button().lit(active)),
                    Fixture::Pill => {
                        ui.put(slot, Pill::new("queued", Tone::Busy).active(active));
                        ui.interact(slot, Id::new(("pill", index)), Sense::click())
                    }
                    Fixture::Dotted => {
                        ui.put(slot, Dotted::new().active(active));
                        ui.interact(slot, Id::new(("dotted", index)), Sense::click())
                    }
                    Fixture::Number => ui.put(
                        slot,
                        Number::new(&mut shell.values[index]).unit("m").width(110.0),
                    ),
                    Fixture::Tabs => return,
                };
                if response.clicked() {
                    shell.chosen = Some(index);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Script, theme};
    use pfx_editor_shell::drive;

    fn run(fixture: Fixture) -> Shell {
        let mut shell = Shell::fixture(fixture);
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, &theme::manfred());
        let script = Script::parse(fixture.script()).unwrap();
        drive(&ctx, fixture.size(), &script, |ui| shell.ui(ui));
        shell
    }

    #[test]
    fn every_fixture_has_a_name() {
        for fixture in Fixture::ALL {
            assert_eq!(Fixture::from_name(fixture.name()), Some(fixture));
        }
        assert_eq!(Fixture::from_name("nope"), None);
    }

    #[test]
    fn the_scripts_reach_the_active_cell() {
        for fixture in Fixture::ALL {
            assert_eq!(run(fixture).chosen, Some(2), "{fixture:?}");
        }
    }
}
