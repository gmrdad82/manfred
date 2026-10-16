use std::path::PathBuf;

use manfred_recipe::{Problem, Recipe, SkyAmbient, SkyKind, load, parse, validate};

fn fixture(name: &str) -> Recipe {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sky")
        .join(name);
    load(&path).unwrap_or_else(|e| panic!("{name}: {e}")).recipe
}

fn recipe(text: &str) -> Recipe {
    parse(&format!("manfred = 1\nname = \"t\"\n{text}")).unwrap_or_else(|e| panic!("{e}"))
}

fn error(text: &str) -> String {
    match parse(&format!("manfred = 1\nname = \"t\"\n{text}")) {
        Ok(_) => panic!("should not parse:\n{text}"),
        Err(e) => e.to_string(),
    }
}

fn problems(text: &str) -> Vec<String> {
    validate(&recipe(text))
        .iter()
        .map(Problem::to_string)
        .collect()
}

const ROOM: &str = "[sky.room]\n[[sky.room.lights]]\nname = \"key\"\n";

#[test]
fn the_room_parses_with_every_lamp_key_and_the_defaults() {
    let r = fixture("room.toml");
    assert_eq!(validate(&r), Vec::new());
    let sky = r.sky.as_ref().unwrap();
    assert_eq!(sky.kind, SkyKind::Room);
    assert_eq!(sky.rotation_deg, 30.0);
    assert_eq!(sky.intensity, 1.5);
    let room = sky.room.as_ref().unwrap();
    assert_eq!(room.width, 512);
    assert_eq!(room.floor, [0.2197, 0.1352, 0.0676]);
    assert_eq!(room.wall, [0.19, 0.13, 0.07]);
    assert_eq!(room.ceiling, [0.2845, 0.2276, 0.1593]);
    assert_eq!(room.lights.len(), 2);
    let window = &room.lights[0];
    assert_eq!(window.name, "window");
    assert_eq!(
        [
            window.az,
            window.el,
            window.power,
            window.width,
            window.height
        ],
        [-64.0, 14.0, 10.585, 37.5, 29.0]
    );
    assert_eq!(window.color, [1.0, 0.86, 0.66]);
    assert_eq!([window.soft, window.slats, window.open], [0.05, 9.0, 0.62]);
    let bare = &room.lights[1];
    assert_eq!(bare.name, "");
    assert_eq!([bare.az, bare.el, bare.power], [62.0, 30.0, 1.7]);
    assert_eq!(bare.color, [1.0, 0.9, 0.78]);
    assert_eq!([bare.width, bare.height, bare.soft], [30.0, 20.0, 0.08]);
    assert_eq!([bare.slats, bare.open], [0.0, 1.0]);
    let look = r.look.as_ref().unwrap();
    assert_eq!(look.exposure, 0.93);
    assert_eq!(look.grade.haze, 0.05);
}

#[test]
fn an_empty_room_takes_the_defaults() {
    let r = recipe("[sky]\nkind = \"room\"\n[sky.room]\n");
    let room = r.sky.unwrap().room.unwrap();
    assert_eq!(room.width, 1024);
    assert_eq!(room.floor, [0.16, 0.10, 0.06]);
    assert_eq!(room.wall, [0.30, 0.24, 0.18]);
    assert_eq!(room.ceiling, [0.55, 0.47, 0.38]);
    assert!(room.lights.is_empty());
}

#[test]
fn an_unknown_lamp_or_room_key_is_refused() {
    assert_eq!(
        error(
            "[sky]\nkind = \"room\"\n[sky.room]\n[[sky.room.lights]]\nname = \"key\"\nslat = 4.0\n"
        ),
        "line 8: sky.room.lights[key].slat: unknown key `slat` in `sky.room.lights[key]`, expected one of: name, az, el, power, color, width, height, soft, slats, open"
    );
    assert_eq!(
        error("[sky]\nkind = \"room\"\n[sky.room]\nlamps = []\n"),
        "line 6: sky.room.lamps: unknown key `lamps` in `sky.room`, expected one of: width, floor, wall, ceiling, lights"
    );
}

#[test]
fn the_room_width_is_whole_texels() {
    assert_eq!(
        error("[sky]\nkind = \"room\"\n[sky.room]\nwidth = 6.0\n"),
        "line 6: sky.room.width: expected a whole number, found a float"
    );
    assert_eq!(
        problems("[sky]\nkind = \"room\"\n[sky.room]\nwidth = 1\n"),
        ["line 6: sky.room.width: expected the room texture's width in texels, from 2 to 8192"]
    );
}

#[test]
fn the_old_sky_light_points_at_the_room() {
    assert_eq!(
        error("[sky]\nkind = \"room\"\n[[sky.light]]\nname = \"key\"\n"),
        "line 5: sky.light: `[[sky.light]]` moved under `[sky.room]`, expected the lamps as `[[sky.room.lights]]`"
    );
}

#[test]
fn a_room_sky_refuses_a_path_a_preparation_and_a_balance() {
    assert_eq!(
        problems(&format!(
            "[sky]\nkind = \"room\"\npath = \"sky.hdr\"\nsource = \"test\"\n{ROOM}"
        )),
        ["line 5: sky.path: kind \"room\" draws the room, not a file, expected `path` removed"]
    );
    assert_eq!(
        problems(&format!(
            "[sky]\nkind = \"room\"\n[sky.prepare]\ncap = 60.0\nmean = 0.3\n{ROOM}"
        )),
        [
            "line 5: sky.prepare: kind \"room\" draws the room, not a file, so there is nothing to prepare, expected `[sky.prepare]` removed"
        ]
    );
    assert_eq!(
        problems(&format!(
            "[sky]\nkind = \"room\"\n[sky.prepare]\nbalance = [1.0, 0.92, 0.8]\n{ROOM}"
        )),
        [
            "line 5: sky.prepare: kind \"room\" draws the room, not a file, so there is nothing to prepare, expected `[sky.prepare]` removed"
        ]
    );
    assert_eq!(
        problems("[sky]\nkind = \"room\"\n"),
        ["line 3: sky.room: kind \"room\" draws the lamp room, expected a `[sky.room]` table"]
    );
}

#[test]
fn the_old_preparation_spellings_are_refused_with_the_new_one() {
    for kind in ["hdr", "mix"] {
        for name in ["capped", "softened"] {
            assert_eq!(
                error(&format!(
                    "[sky]\nkind = \"{kind}\"\npath = \"sky.hdr\"\nsource = \"test\"\nprepare = \"{name}\"\n{ROOM}"
                )),
                "line 7: sky.prepare: `prepare` is a table now, expected `[sky.prepare]` with `cap = 60.0` and `mean = 0.3` (and `balance = [r, g, b]` when the sky has one)",
                "{kind} {name}"
            );
        }
    }
    assert_eq!(
        error(&format!(
            "[sky]\nkind = \"hdr\"\npath = \"sky.hdr\"\nsource = \"test\"\nbalance = [1.0, 0.92, 0.8]\n{ROOM}"
        )),
        "line 7: sky.balance: `balance` moved into `[sky.prepare]`, expected `[sky.prepare]` with `balance = [r, g, b]`"
    );
}

#[test]
fn a_preparation_table_is_the_scene_files_cap_balance_and_mean() {
    let head = "[sky]\nkind = \"hdr\"\npath = \"sky.hdr\"\nsource = \"test\"\n[sky.prepare]\n";
    let capped = recipe(&format!("{head}cap = 60.0\nmean = 0.3\n{ROOM}"));
    let sky = capped.sky.unwrap();
    assert_eq!(
        (sky.prepare.as_deref(), sky.balance),
        (Some("capped"), None)
    );
    let balanced = recipe(&format!("{head}balance = [1.0, 0.92, 0.8]\n{ROOM}"));
    let sky = balanced.sky.unwrap();
    assert_eq!(
        (sky.prepare.as_deref(), sky.balance),
        (None, Some([1.0, 0.92, 0.8]))
    );
    for (table, expected) in [
        (
            "cap = 60.0\n",
            "line 7: sky.prepare: Manfred prepares an HDR at one cap and mean, expected `cap = 60.0` and `mean = 0.3` together, or neither",
        ),
        (
            "cap = 80.0\nmean = 0.3\n",
            "line 7: sky.prepare: Manfred prepares an HDR at one cap and mean, expected `cap = 60.0` and `mean = 0.3` together, or neither",
        ),
        (
            "mean = 0.5\n",
            "line 7: sky.prepare: Manfred prepares an HDR at one cap and mean, expected `cap = 60.0` and `mean = 0.3` together, or neither",
        ),
        (
            "",
            "line 7: sky.prepare: an empty table, expected one of `cap`, `balance` and `mean`",
        ),
        (
            "colour = [1.0, 1.0, 1.0]\ncap = 60.0\nmean = 0.3\n",
            "line 8: sky.prepare.colour: unknown key `colour` in `sky.prepare`, expected one of: cap, balance, mean",
        ),
    ] {
        assert_eq!(error(&format!("{head}{table}{ROOM}")), expected, "{table}");
    }
}

#[test]
fn an_hdr_sky_may_carry_the_room_for_the_sidecar() {
    let r = recipe(&format!(
        "[sky]\nkind = \"hdr\"\npath = \"sky.hdr\"\nsource = \"test\"\n[sky.prepare]\ncap = 60.0\nbalance = [1.0, 0.92, 0.8]\nmean = 0.3\n{ROOM}"
    ));
    assert_eq!(validate(&r), Vec::new());
    let sky = r.sky.unwrap();
    assert_eq!(sky.balance, Some([1.0, 0.92, 0.8]));
    assert_eq!(sky.room.unwrap().lights[0].name, "key");
}

#[test]
fn a_mix_needs_its_file_and_its_room_and_only_a_mix_takes_fill() {
    assert_eq!(
        problems("[sky]\nkind = \"mix\"\nfill = 0.25\n"),
        [
            "line 3: sky.path: kind \"mix\" needs `path`, the .hdr file",
            "line 3: sky.room: kind \"mix\" draws the lamp room, expected a `[sky.room]` table",
        ]
    );
    assert_eq!(
        problems("[sky]\nkind = \"analytic\"\nfill = 0.25\n"),
        [
            "line 5: sky.fill: `fill` scales the room's lamps over the HDR, expected it only with kind \"mix\""
        ]
    );
}

#[test]
fn the_authored_hourly_ambient_parses_for_a_room_and_is_refused_elsewhere() {
    let room = recipe(&format!(
        "[sky]\nkind = \"room\"\nambient = \"authored\"\n{ROOM}"
    ));
    assert_eq!(validate(&room), Vec::new());
    assert_eq!(room.sky.unwrap().ambient, Some(SkyAmbient::Authored));
    assert_eq!(
        recipe(&format!("[sky]\nkind = \"room\"\n{ROOM}"))
            .sky
            .unwrap()
            .ambient,
        None
    );
    assert_eq!(
        problems("[sky]\nkind = \"analytic\"\nambient = \"authored\"\n"),
        [
            "line 5: sky.ambient: `ambient = \"authored\"` scales the lamp room by the authored sun's hourly ambient, expected it only with kind \"room\", not \"analytic\""
        ]
    );
    assert_eq!(
        problems(&format!(
            "[sky]\nkind = \"mix\"\npath = \"a.hdr\"\nsource = \"s\"\nambient = \"authored\"\n{ROOM}"
        ))
        .len(),
        1
    );
    let refused = error("[sky]\nkind = \"room\"\nambient = \"other\"\n");
    assert!(refused.contains("sky.ambient"), "{refused}");
    assert!(refused.contains("\"authored\""), "{refused}");
}

#[test]
fn a_lamp_outside_the_engines_limits_is_a_problem() {
    assert_eq!(
        problems(
            "[sky]\nkind = \"room\"\n[sky.room]\n[[sky.room.lights]]\nname = \"a\"\nwidth = 180.0\npower = -1.0\n[[sky.room.lights]]\nname = \"a\"\n"
        ),
        [
            "line 8: sky.room.lights[a].width: expected an angle in degrees above 0 and below 180",
            "line 9: sky.room.lights[a].power: expected a number from 0 up",
            "line 11: sky.room.lights[a].name: duplicate name, expected a name no other lamp uses",
        ]
    );
}

#[test]
fn haze_is_an_amount_from_zero_to_one() {
    assert_eq!(
        problems("[look]\ngrade = { haze = 1.5 }\n"),
        ["line 4: look.grade.haze: expected an amount from 0 to 1"]
    );
    assert!(problems("[look]\ngrade = { haze = 1.0 }\n").is_empty());
}

#[test]
fn the_look_reads_its_view_and_defaults_to_graded() {
    use manfred_recipe::View;
    assert_eq!(recipe("[look]\n").look.unwrap().view, View::Graded);
    assert_eq!(
        recipe("[look]\nview = \"graded\"\n").look.unwrap().view,
        View::Graded
    );
    let standard = recipe("[look]\nview = \"standard\"\nexposure = 0.4\n")
        .look
        .unwrap();
    assert_eq!((standard.view, standard.exposure), (View::Standard, 0.4));
    assert!(problems("[look]\nview = \"standard\"\ngrade = {}\n").is_empty());
    assert!(problems("[look]\nview = \"standard\"\ngrade = { saturation = 1.0 }\n").is_empty());
}

#[test]
fn a_bad_view_is_refused_and_the_standard_view_takes_no_grade() {
    let error = error("[look]\nview = \"flat\"\n");
    assert!(error.contains("look.view"), "{error}");
    assert!(
        error.contains("standard") && error.contains("graded"),
        "{error}"
    );
    assert_eq!(
        problems(
            "[look]\nview = \"standard\"\ngrade = { vignette = 0.2, saturation = 1.1, warmth = 0.1, bloom = 0.05, haze = 0.3 }\n"
        ),
        [
            "line 5: look.grade.bloom: the standard view has no grade, expected 0.0 or `view = \"graded\"`",
            "line 5: look.grade.haze: the standard view has no grade, expected 0.0 or `view = \"graded\"`",
            "line 5: look.grade.saturation: the standard view has no grade, expected 1.0 or `view = \"graded\"`",
            "line 5: look.grade.vignette: the standard view has no grade, expected 0.0 or `view = \"graded\"`",
            "line 5: look.grade.warmth: the standard view has no grade, expected 0.0 or `view = \"graded\"`",
        ]
    );
    assert!(problems("[look]\nview = \"graded\"\ngrade = { vignette = 0.2 }\n").is_empty());
}

const OUTPUT: &str = "[output]\nglb = \"t.glb\"\nsidecar = \"t.json\"\nsidecar_dialect = \"facts\"\nbake = \"bake.toml\"\n[[camera]]\nname = \"c\"\nposition = [0.0, 0.0, 1.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 30.0\n";

#[test]
fn the_bake_table_reads_and_checks_its_volumes() {
    let r = recipe(&format!(
        "{OUTPUT}[bake]\nproduct = \"sample\"\nanchors = [8.0, 12.0]\nsamples = 16\n[[bake.volume]]\nname = \"desk\"\nmin = [-0.5, 0.0, -0.5]\nmax = [0.5, 0.5, 0.5]\nspacing = 0.25\n"
    ));
    assert_eq!(validate(&r), Vec::new());
    let bake = r.bake.unwrap();
    assert_eq!(bake.product.as_deref(), Some("sample"));
    assert_eq!(bake.anchors, [8.0, 12.0]);
    assert_eq!(bake.samples, 16);
    assert_eq!(bake.volumes[0].name, "desk");
    assert_eq!(bake.volumes[0].spacing, 0.25);
    assert_eq!(
        problems(&format!(
            "{OUTPUT}[bake]\nanchors = [12.0, 8.0]\nsamples = 0\nproduct = \"Blender\"\n[[bake.volume]]\nname = \"a b\"\nmin = [0.0, 0.0, 0.0]\nmax = [1.0, 1.0, 0.3]\nspacing = 0.25\n"
        )),
        [
            "line 14: bake.anchors: expected rising, distinct hours from 0 to 24",
            "line 15: bake.samples: expected a whole number above 0",
            "line 16: bake.product: \"Blender\" is not an app's name, expected lowercase letters, digits and `-`, starting with a letter or digit",
            "line 18: bake.volume[a b].name: expected a name of ASCII letters, digits, `-` and `_`",
            "line 21: bake.volume[a b].spacing: expected `max` minus `min` a whole number of spacings on every axis",
        ]
    );
}

#[test]
fn a_bake_path_needs_the_bake_table_and_the_table_needs_the_path() {
    assert_eq!(
        problems(OUTPUT),
        [
            "line 7: output.bake: a bake recipe needs its anchors, samples and volumes, expected a `[bake]` table"
        ]
    );
    assert_eq!(
        problems("[bake]\nanchors = [12.0]\nsamples = 4\n"),
        [
            "line 3: bake: `[bake]` is set but nothing writes it, expected `bake` in `[output]`",
            "line 3: bake.product: missing, expected `product`, the name of the app the bake is for",
            "line 3: bake.volume: missing, expected at least one `[[bake.volume]]`",
        ]
    );
}

#[test]
fn the_bake_file_is_named_as_pfx_reads_it() {
    let text = format!(
        "{}[bake]\nproduct = \"sample\"\nanchors = [12.0]\nsamples = 4\n[[bake.volume]]\nname = \"v\"\nmin = [0.0, 0.0, 0.0]\nmax = [1.0, 1.0, 1.0]\nspacing = 0.5\n",
        OUTPUT.replace("bake = \"bake.toml\"", "bake = \"deck.bake.toml\"")
    );
    assert_eq!(
        problems(&text),
        [
            "line 7: output.bake: `pfx bake --scene <folder>` reads the folder's `bake.toml`, expected a path ending in `bake.toml`"
        ]
    );
}

#[test]
fn an_analytic_sky_takes_turbidity_and_ground_albedo_and_other_kinds_refuse_them() {
    let sky =
        recipe("[sky]\nkind = \"analytic\"\nturbidity = 6.5\nground_albedo = [0.1, 0.2, 0.3]\n")
            .sky
            .unwrap();
    assert_eq!(sky.turbidity, Some(6.5));
    assert_eq!(sky.ground_albedo, Some([0.1, 0.2, 0.3]));
    let plain = recipe("[sky]\nkind = \"analytic\"\n").sky.unwrap();
    assert_eq!((plain.turbidity, plain.ground_albedo), (None, None));
    assert!(error("[sky]\nkind = \"analytic\"\nturbidity = 12.0\n").contains("sky.turbidity"));
    assert!(error("[sky]\nkind = \"room\"\nturbidity = 3.0\n").contains("sky.turbidity"));
}

#[test]
fn a_room_haze_needs_lo_below_hi() {
    let r = recipe(
        "[haze]\nlo = [-1.0, 0.0, -1.0]\nhi = [1.0, 2.0, 1.0]\nphase = 0.6\nambient = [0.05, 0.04, 0.03]\nseed = 4\n",
    );
    assert_eq!(r.effects.haze.unwrap().len(), 5);
    assert!(error("[haze]\nlo = [-1.0, 0.0, -1.0]\n").contains("haze.hi"));
    assert!(error("[haze]\nlo = [1.0, 0.0, -1.0]\nhi = [1.0, 2.0, 1.0]\n").contains("haze.hi"));
    assert!(error("[haze]\nheight = 0.0\nphase = 0.95\n").contains("haze.phase"));
}
