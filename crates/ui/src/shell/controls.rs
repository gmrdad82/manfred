use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Context, Id, LayerId, Pos2, Rect};

use super::*;
use crate::follow::Mode;
use crate::shots::{Fake, Image, Job, Plan, Runs};
use crate::viewer::Viewer;
use crate::{Script, scratch, theme};
use pfx_editor_shell::drive;

const SIZE: [u32; 2] = [1586, 992];

const RECIPE: &str = r#"manfred = 1
name = "controls"

[[object]]
name = "table"
kind = "box"
size = [1.0, 0.1, 1.0]
material = "oak"

[[object]]
name = "cup"
kind = "box"
size = [0.1, 0.1, 0.1]
parent = "table"
material = "enamel"

[[object]]
name = "label"
kind = "text"
on = "table"
spans = [{ text = "manfred", size = 0.02 }]

[[camera]]
name = "front"
position = [0.0, 0.5, 2.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[[camera]]
name = "side"
position = [2.0, 0.5, 0.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[sun]
model = "daylight"
hour = 9.5
latitude = 52.0
heading = 0.0

[[light]]
name = "lamp"
position = [0.2, 0.3, 0.1]
intensity = 20.0
shadow = true

[materials.oak]
family = "lacquer"
base = [0.5, 0.3, 0.1]
roughness = 0.5

[materials.enamel]
family = "lacquer"
base = [0.9, 0.9, 0.9]
roughness = 0.3
"#;

struct Bench {
    shell: Shell,
    viewer: Viewer,
    path: PathBuf,
}

fn quiet_shots(shell: &mut Shell, path: &Path) {
    let runner = Arc::new(Runs {
        processes: Mutex::new(vec![Fake {
            pid: 42,
            script: Arc::new(Mutex::new(Plan::default())),
        }]),
        statuses: Arc::new(Mutex::new(vec![
            r#"{"jobs":[{"pid":42,"state":"waiting","as":"manfred","label":"x","waits_for":null}]}"#
                .to_string(),
        ])),
        refuse: false,
    });
    let mut shots = crate::shots::Shots::with(runner, "manfred".to_string());
    shots.set_dir(path.with_file_name("renders"));
    shell.shots = shots;
}

fn own_showcase(shell: &mut Shell) {
    shell
        .picker
        .set_showcase_fallback(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../showcase"));
}

fn recipe(name: &str) -> Bench {
    let path = scratch(name);
    std::fs::write(&path, RECIPE).unwrap();
    let mut viewer = Viewer::new(&path, Mode::Fake);
    let mut shell = Shell::open(&path.display().to_string());
    viewer.sync(&mut shell);
    shell.drains = true;
    quiet_shots(&mut shell, &path);
    own_showcase(&mut shell);
    let index = |name: &str| shell.document.object(name).unwrap();
    let cup = index("cup");
    shell.select(cup);
    if let Some(eye) = shell.eye {
        shell.eye = Some(orbit::turn(&eye, [60.0, 20.0]));
    }
    Bench {
        shell,
        viewer,
        path,
    }
}

fn pass(shell: &mut Shell) -> Context {
    let ctx = Context::default();
    theme::apply_theme(&ctx, &theme::manfred());
    drive(&ctx, SIZE, &Script::default(), |ctx| shell.ui(ctx));
    ctx
}

fn edited(name: &str) -> Bench {
    let mut bench = recipe(name);
    let shell = &mut bench.shell;
    shell.change(Change::Triple(Triple::Location, 0, 0.25));
    pass(shell);
    shell.change(Change::Triple(Triple::Location, 1, 0.5));
    let ctx = pass(shell);
    shell.command(&ctx, Command::Undo);
    assert_eq!(shell.editor.as_ref().map(Editor::steps), Some((1, 1)));
    bench
}

fn lamp(name: &str) -> Bench {
    let mut bench = recipe(name);
    let lamp = bench
        .shell
        .document
        .lights
        .iter()
        .position(|(_, name, _)| name == "lamp")
        .unwrap();
    bench.shell.choose_light(lamp);
    bench
}

fn daylight(name: &str) -> Bench {
    let mut bench = recipe(name);
    bench.shell.set_hour(14.0);
    bench
}

fn text(name: &str) -> Bench {
    let mut bench = recipe(name);
    let label = bench.shell.document.object("label").unwrap();
    bench.shell.select(label);
    bench
}

fn picker(name: &str) -> Bench {
    let mut bench = recipe(name);
    bench.shell.picker.show();
    bench
}

fn confirming(name: &str) -> Bench {
    let mut bench = recipe(name);
    bench.shell.focus = Panel::B2;
    bench.shell.ask_delete();
    bench
}

fn rendering(name: &str) -> Bench {
    let mut bench = recipe(name);
    bench.shell.render();
    assert!(bench.shell.shots.busy());
    bench
}

fn picture() -> Image {
    Image {
        size: [16, 10],
        rgba: (0..160).flat_map(|n| [n as u8, 90, 140, 255]).collect(),
    }
}

fn rendered(name: &str) -> Bench {
    let mut bench = recipe(name);
    let shots = &mut bench.shell.shots;
    shots.show(Some(Job {
        camera: "front".to_string(),
        size: [16, 10],
        out: bench.path.with_file_name("front.png"),
        pid: Some(42),
        phase: Phase::Done,
        cancelling: false,
        progress: None,
    }));
    shots.put_picture(Picture {
        full: picture(),
        thumb: picture(),
    });
    bench
}

fn overlay(name: &str) -> Bench {
    let mut bench = rendered(name);
    pass(&mut bench.shell);
    bench.shell.full = true;
    bench
}

fn scene(name: &str) -> Bench {
    let path = crate::watched::fixture(name);
    let mut viewer = Viewer::scene(&path, Mode::Fake);
    let mut shell = Shell::open(&path.display().to_string());
    let ctx = Context::default();
    theme::apply_theme(&ctx, &theme::manfred());
    drive(&ctx, SIZE, &Script::default(), |ctx| {
        shell.ui(ctx);
        viewer.sync(&mut shell);
    });
    quiet_shots(&mut shell, &path);
    if let Some(first) = shell.document.objects.first().map(|_| 0) {
        shell.select(first);
    }
    if let Some(eye) = shell.eye {
        shell.eye = Some(orbit::turn(&eye, [60.0, 20.0]));
    }
    Bench {
        shell,
        viewer,
        path,
    }
}

fn empty(name: &str) -> Bench {
    let path = scratch(name);
    let mut shell = Shell::browse(&path.parent().unwrap().display().to_string());
    quiet_shots(&mut shell, &path);
    own_showcase(&mut shell);
    let viewer = Viewer::new(&path, Mode::Fake);
    Bench {
        shell,
        viewer,
        path,
    }
}

type Make = fn(&str) -> Bench;
type Ready = fn(&Context);

fn settled(_: &Context) {}

fn unfocused(ctx: &Context) {
    ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
}

const STATES: [(&str, Make, Ready); 12] = [
    ("object", recipe, settled),
    ("edited", edited, settled),
    ("lamp", lamp, settled),
    ("daylight", daylight, settled),
    ("text", text, settled),
    ("picker", picker, unfocused),
    ("confirming", confirming, settled),
    ("rendering", rendering, settled),
    ("rendered", rendered, settled),
    ("overlay", overlay, settled),
    ("scene", scene, settled),
    ("empty", empty, unfocused),
];

#[derive(Clone, Copy, Debug)]
struct Control {
    id: Id,
    rect: Rect,
}

fn controls(ctx: &Context) -> Vec<Control> {
    let mut found: Vec<Control> = ctx
        .viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, rects)| rects.iter())
                .filter(|widget| {
                    widget.enabled
                        && widget.sense.interactive()
                        && widget.sense.is_focusable()
                        && widget.interact_rect.is_positive()
                })
                .map(|widget| (widget.id, widget.layer_id, widget.interact_rect))
                .collect::<Vec<(Id, LayerId, Rect)>>()
        })
        .into_iter()
        .filter(|(_, layer, rect)| {
            ctx.layer_id_at(rect.center())
                .unwrap_or(LayerId::background())
                == *layer
        })
        .map(|(id, _, rect)| Control { id, rect })
        .collect();
    let all = found.clone();
    found.retain(|control| {
        !all.iter()
            .any(|inner| inner.id != control.id && control.rect.contains_rect(inner.rect))
    });
    found.sort_by(|a, b| {
        (a.rect.min.y, a.rect.min.x)
            .partial_cmp(&(b.rect.min.y, b.rect.min.x))
            .unwrap()
    });
    found
}

fn seen(shell: &Shell, ctx: &Context, path: &Path) -> String {
    let focused = ctx.memory(|memory| memory.focused());
    let popup = egui::Popup::is_any_open(ctx);
    let mut lines = vec![
        format!("{:?} {:?} {:?}", shell.focus, focused, popup),
        format!("{:?} {:?} {:?}", shell.search, shell.object, shell.open),
        format!("{} {} {}", shell.camera, shell.light, shell.lit_shown),
        format!("{} {} {:?}", shell.hour, shell.render_camera, shell.size),
        format!(
            "{} {} {}",
            shell.shots.samples(),
            shell.full,
            shell.picker.shown()
        ),
        format!("{:?} {:?} {:?}", shell.confirm, shell.note, shell.pick),
        format!("{:?} {:?}", shell.gizmo.mode, shell.gizmo.space),
        format!("{:?}", shell.eye),
        format!("{:?}", shell.editor.as_ref().map(Editor::steps)),
        format!("{:?}", shell.shots.job()),
        shell.document.path.clone(),
    ];
    lines.extend(std::fs::read_to_string(path).ok());
    lines.join("\n")
}

fn panel_at(shell: &Shell, at: Pos2) -> Option<Panel> {
    shell
        .panels
        .iter()
        .find(|(_, rect)| rect.contains(at))
        .map(|(panel, _)| *panel)
}

fn listed(make: Make, name: &str) -> (Vec<Control>, Vec<(Panel, Rect)>) {
    let mut bench = make(name);
    let ctx = pass(&mut bench.shell);
    (controls(&ctx), bench.shell.panels.clone())
}

fn probe((make, ready): (Make, Ready), name: &str, script: &str, focus: Option<Panel>) -> bool {
    let mut bench = make(name);
    let ctx = Context::default();
    theme::apply_theme(&ctx, &theme::manfred());
    let mut before = None;
    let mut passes = 0;
    let path = bench.path.clone();
    drive(
        &ctx,
        SIZE,
        &Script::parse(&format!("frame\n{script}\nframe\nframe")).unwrap(),
        |ctx| {
            bench.shell.ui(ctx);
            passes += 1;
            if passes == 2 {
                if let Some(focus) = focus {
                    bench.shell.focus = focus;
                }
                ready(ctx);
                before = Some(seen(&bench.shell, ctx, &path));
            }
        },
    );
    before.unwrap() != seen(&bench.shell, &ctx, &path)
}

fn other(panel: Panel) -> Panel {
    if panel == Panel::D1 {
        Panel::A
    } else {
        Panel::D1
    }
}

#[test]
fn every_control_the_shell_draws_acts() {
    let mut acted: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for (state, make, ready) in STATES {
        let name = format!("controls-walk-{state}");
        let (found, panels) = listed(make, &name);
        assert!(!found.is_empty(), "{state}: no controls drawn");
        assert!(
            !probe((make, ready), &name, "frame", None),
            "{state}: the state moves by itself, so the walk proves nothing"
        );
        for control in found {
            let at = control.rect.lerp_inside(egui::Vec2::new(0.3, 0.5));
            let panel = panels
                .iter()
                .find(|(_, rect)| rect.contains(at))
                .map(|(panel, _)| *panel);
            let header = Panel::ALL.into_iter().find(|p| header_id(*p) == control.id);
            let focus = match header {
                Some(panel) => Some(other(panel)),
                None => panel,
            };
            let did = probe(
                (make, ready),
                &name,
                &format!("click {} {}", at.x, at.y),
                focus,
            );
            let key = format!("{:?} {:?}", control.id, control.rect);
            let entry = acted
                .entry(key)
                .or_insert_with(|| (format!("{state} at {at:?} in {panel:?}"), false));
            entry.1 |= did;
        }
    }
    let dead: Vec<&String> = acted
        .values()
        .filter(|(_, did)| !did)
        .map(|(place, _)| place)
        .collect();
    assert!(dead.is_empty(), "controls that do nothing: {dead:#?}");
}

fn centre_of(ctx: &Context, id: Id) -> Pos2 {
    ctx.read_response(id)
        .unwrap_or_else(|| panic!("{id:?} was not drawn"))
        .rect
        .center()
}

fn run(shell: &mut Shell, script: &str) -> Context {
    let ctx = Context::default();
    theme::apply_theme(&ctx, &theme::manfred());
    drive(&ctx, SIZE, &Script::parse(script).unwrap(), |ctx| {
        shell.ui(ctx)
    });
    ctx
}

fn click(at: Pos2) -> String {
    format!("click {} {}\nframe", at.x, at.y)
}

#[test]
fn each_tab_jumps_to_its_panel_and_lights_up() {
    let mut bench = recipe("controls-tabs");
    let ctx = pass(&mut bench.shell);
    assert_eq!(bench.shell.focus, Panel::C1);
    assert_eq!(tab_of(bench.shell.focus), None);
    for (index, (name, panel)) in TABS.iter().zip(TAB_PANELS).enumerate() {
        let at = centre_of(&ctx, Tabs::id(name));
        run(&mut bench.shell, &click(at));
        assert_eq!(bench.shell.focus, panel, "{name}");
        assert_eq!(tab_of(bench.shell.focus), Some(index), "{name}");
    }
    run(&mut bench.shell, "key c\nkey 2\nframe");
    assert_eq!(tab_of(bench.shell.focus), Some(0));
    run(&mut bench.shell, "key c\nkey 1\nframe");
    assert_eq!(tab_of(bench.shell.focus), None);
}

#[test]
fn a_tab_gives_its_panel_the_keys_even_from_the_search_field() {
    let mut bench = recipe("controls-tab-keys");
    let ctx = pass(&mut bench.shell);
    let search = centre_of(&ctx, search_id());
    let cameras = centre_of(&ctx, Tabs::id("Cameras"));
    let lights = centre_of(&ctx, Tabs::id("Lights"));
    assert_eq!(bench.shell.camera, 0);
    run(
        &mut bench.shell,
        &format!("{}\nkey ArrowDown\nframe", click(cameras)),
    );
    assert_eq!(bench.shell.focus, Panel::B2);
    assert_eq!(bench.shell.camera, 1);
    let ctx = run(
        &mut bench.shell,
        &format!(
            "{}\ntext ta\n{}\nkey ArrowDown\nframe",
            click(search),
            click(lights)
        ),
    );
    assert_eq!(bench.shell.search, "ta");
    assert_eq!(bench.shell.focus, Panel::B3);
    assert!(ctx.memory(|memory| memory.focused()).is_none());
    assert_eq!(bench.shell.light, 1);
    assert!(bench.shell.lit_shown);
}

#[test]
fn a_header_click_focuses_its_panel_like_its_code() {
    let mut bench = recipe("controls-headers");
    let ctx = pass(&mut bench.shell);
    for panel in Panel::ALL {
        bench.shell.focus = other(panel);
        run(&mut bench.shell, &click(centre_of(&ctx, header_id(panel))));
        assert_eq!(bench.shell.focus, panel);
    }
}

fn hint_caps(ctx: &Context) -> Vec<Pos2> {
    let strip = SIZE[1] as f32 - STATUS - HINTS..SIZE[1] as f32 - STATUS;
    let mut caps: Vec<Pos2> = controls(ctx)
        .into_iter()
        .map(|control| control.rect.center())
        .filter(|at| strip.contains(&at.y))
        .collect();
    caps.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
    caps
}

#[test]
fn the_hint_bar_clicks_open_undo_and_redo_and_nothing_else() {
    let mut bench = recipe("controls-hint-caps");
    let ctx = pass(&mut bench.shell);
    let caps = hint_caps(&ctx);
    assert_eq!(caps.len(), 1, "only ctrl o acts with nothing to undo");
    run(&mut bench.shell, &click(caps[0]));
    assert!(bench.shell.picker.shown());

    let mut bench = edited("controls-hint-undo");
    let ctx = pass(&mut bench.shell);
    let caps = hint_caps(&ctx);
    assert_eq!(caps.len(), 3, "ctrl z, ctrl shift z and ctrl o");
    let (undo, redo) = (caps[0], caps[1]);
    let once = std::fs::read_to_string(&bench.path).unwrap();
    run(&mut bench.shell, &click(undo));
    assert_eq!(std::fs::read_to_string(&bench.path).unwrap(), RECIPE);
    assert_eq!(bench.shell.editor().unwrap().steps(), (0, 2));
    run(&mut bench.shell, &click(redo));
    assert_eq!(std::fs::read_to_string(&bench.path).unwrap(), once);
    assert_eq!(bench.shell.editor().unwrap().steps(), (1, 1));
}

fn on_top(ctx: &Context, at: Pos2) -> bool {
    ctx.layer_id_at(at)
        .is_some_and(|layer| layer.order == egui::Order::Foreground)
}

fn footer(bench: &mut Bench) -> Vec<Pos2> {
    let ctx = run(&mut bench.shell, "frame\nframe\nframe");
    let caps: Vec<Pos2> = controls(&ctx)
        .into_iter()
        .map(|control| control.rect.center())
        .filter(|at| on_top(&ctx, *at))
        .collect();
    let lowest = caps.iter().fold(0.0_f32, |low, at| low.max(at.y));
    let mut row: Vec<Pos2> = caps.into_iter().filter(|at| at.y == lowest).collect();
    row.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
    assert_eq!(row.len(), 2, "enter and esc: {row:?}");
    row
}

#[test]
fn the_pickers_caps_open_and_close_and_a_click_beside_it_closes_it() {
    let mut bench = picker("controls-picker");
    let row = footer(&mut bench);
    run(
        &mut bench.shell,
        &format!("frame\nframe\n{}", click(row[1])),
    );
    assert!(!bench.shell.picker.shown());

    let mut bench = picker("controls-picker-enter");
    let row = footer(&mut bench);
    let before = bench.shell.document.path.clone();
    run(
        &mut bench.shell,
        &format!("frame\nframe\n{}", click(row[0])),
    );
    assert_ne!(bench.shell.document.path, before);
    assert!(!bench.shell.picker.shown());

    let mut bench = picker("controls-picker-beside");
    let ctx = pass(&mut bench.shell);
    let viewport = bench.shell.viewport.unwrap();
    let beside = Pos2::new(viewport.left() + 12.0, viewport.top() + 12.0);
    assert!(!on_top(&ctx, beside));
    run(
        &mut bench.shell,
        &format!("frame\nframe\n{}", click(beside)),
    );
    assert!(!bench.shell.picker.shown());
}

#[test]
fn a_scene_files_c2_is_read_only_and_c3_cannot_render_it() {
    let mut bench = scene("controls-scene-read");
    let ctx = pass(&mut bench.shell);
    assert!(bench.shell.object.is_some());
    assert!(ctx.read_response(Id::new("manfred c2 name")).is_none());
    let found = controls(&ctx);
    for panel in [Panel::C2, Panel::C3] {
        let inside: Vec<&Control> = found
            .iter()
            .filter(|control| panel_at(&bench.shell, control.rect.center()) == Some(panel))
            .filter(|control| control.id != header_id(panel))
            .collect();
        assert!(inside.is_empty(), "{panel:?}: {inside:?}");
    }
}

fn sliders(ctx: &Context, shell: &Shell, panel: Panel) -> Vec<bool> {
    let rect = shell
        .panels
        .iter()
        .find(|(found, _)| *found == panel)
        .map(|(_, rect)| *rect)
        .unwrap();
    ctx.viewport(|viewport| {
        viewport
            .prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rects)| rects.iter())
            .filter(|widget| widget.sense.senses_drag() && rect.contains_rect(widget.rect))
            .map(|widget| widget.enabled)
            .collect()
    })
}

#[test]
fn the_hour_slider_is_off_in_a_scene_file_and_with_no_recipe() {
    let mut bench = recipe("controls-hour-recipe");
    let ctx = pass(&mut bench.shell);
    assert_eq!(sliders(&ctx, &bench.shell, Panel::B3), [true]);
    assert_eq!(bench.shell.hour_frozen(), None);
    let before = bench.shell.hour;
    bench.shell.focus = Panel::B3;
    run(&mut bench.shell, "key ArrowRight\nframe");
    assert_ne!(bench.shell.hour, before);

    for (name, make) in [
        ("controls-hour-scene", scene as Make),
        ("controls-hour-empty", empty as Make),
    ] {
        let mut bench = make(name);
        let ctx = pass(&mut bench.shell);
        assert_eq!(sliders(&ctx, &bench.shell, Panel::B3), [false], "{name}");
        assert!(bench.shell.hour_frozen().is_some(), "{name}");
        let before = bench.shell.hour;
        bench.shell.focus = Panel::B3;
        run(&mut bench.shell, "key ArrowRight\nframe");
        assert_eq!(bench.shell.hour, before, "{name}");
        assert!(!bench.shell.hour_set, "{name}");
    }
    let mut bench = scene("controls-hour-scene-why");
    pass(&mut bench.shell);
    assert!(bench.shell.hour_frozen().unwrap().contains("scene file"));
    let mut bench = empty("controls-hour-empty-why");
    pass(&mut bench.shell);
    assert!(bench.shell.hour_frozen().unwrap().contains("open a recipe"));
}

#[test]
fn use_hour_acts_in_the_daylight_state() {
    let name = "controls-use-hour";
    let (found, panels) = listed(daylight, name);
    let b3 = panels
        .iter()
        .find(|(panel, _)| *panel == Panel::B3)
        .map(|(_, rect)| *rect)
        .unwrap();
    let wrote = found
        .iter()
        .filter(|control| b3.contains_rect(control.rect))
        .any(|control| {
            let at = control.rect.lerp_inside(egui::Vec2::new(0.3, 0.5));
            probe(
                (daylight, settled),
                name,
                &format!("click {} {}", at.x, at.y),
                Some(Panel::B3),
            );
            std::fs::read_to_string(scratch(name)).is_ok_and(|text| text.contains("hour = 14.0"))
        });
    assert!(wrote, "no control in B3 wrote the hour");
}

#[test]
fn legends_and_pills_are_not_controls() {
    let mut bench = rendered("controls-legends");
    let ctx = pass(&mut bench.shell);
    let status = SIZE[1] as f32 - STATUS..SIZE[1] as f32;
    let in_d1: Vec<Control> = controls(&ctx)
        .into_iter()
        .filter(|control| status.contains(&control.rect.center().y))
        .collect();
    assert_eq!(in_d1.len(), 1, "only D1's header: {in_d1:?}");
    assert_eq!(in_d1[0].id, header_id(Panel::D1));
}

#[test]
#[ignore = "GPU: pgpu run --class interactive --as manfred -- cargo test -- --ignored"]
fn the_lights_tab_shot_shows_b3_focused_under_its_tab() {
    let since = std::time::Instant::now();
    let mut bench = recipe("controls-shot");
    let ctx = pass(&mut bench.shell);
    let lights = centre_of(&ctx, Tabs::id("Lights"));
    let script = Script::parse(&format!("frame\n{}\nframe", click(lights))).unwrap();
    let png = crate::shot(
        &mut crate::Manfred::lent(&mut bench.shell, &mut bench.viewer),
        SIZE,
        &script,
    )
    .unwrap();
    assert_eq!(bench.shell.focus, Panel::B3);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::write(root.join("tmp/controls-lights-tab.png"), png).unwrap();
    crate::gpu_turn(since);
}
