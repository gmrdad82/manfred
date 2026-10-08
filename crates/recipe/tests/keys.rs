use manfred_recipe::{Problem, Recipe, parse, validate};

fn text(body: &str) -> String {
    format!("manfred = 1\nname = \"t\"\n{body}")
}

fn recipe(body: &str) -> Recipe {
    parse(&text(body)).unwrap_or_else(|e| panic!("{e}"))
}

fn error(body: &str) -> String {
    match parse(&text(body)) {
        Ok(_) => panic!("should not parse:\n{body}"),
        Err(e) => e.to_string(),
    }
}

fn problems(body: &str) -> Vec<String> {
    validate(&recipe(body))
        .iter()
        .map(Problem::to_string)
        .collect()
}

const DAYLIGHT: &str = "[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 52.0\nheading = 20.0\n";
const AUTHORED: &str = "[sun]\nmodel = \"authored\"\ntoward = [0.3, 0.8, 0.5]\nirradiance = 3.0\n";

#[test]
fn an_authored_sun_reads_toward_color_and_irradiance() {
    let r = recipe(&format!(
        "{AUTHORED}color = [1.0, 0.9, 0.8]\nradius_deg = 1.5\n"
    ));
    assert!(r.daylight.is_none());
    let sun = r.sun.unwrap();
    assert_eq!(sun.toward, [0.3, 0.8, 0.5]);
    assert_eq!(sun.color, [1.0, 0.9, 0.8]);
    assert_eq!(sun.irradiance, 3.0);
    assert_eq!(sun.radius_deg, 1.5);
}

#[test]
fn a_daylight_sun_reads_hour_day_latitude_heading_and_the_reference_hour() {
    let r = recipe(DAYLIGHT);
    assert!(r.sun.is_none());
    let d = r.daylight.unwrap();
    assert_eq!(
        (d.hour, d.day, d.latitude, d.heading, d.reference_hour),
        (9.0, 172, 52.0, 20.0, 16.0)
    );
    let r = recipe(&format!("{DAYLIGHT}day = 80\nreference_hour = 16.0\n"));
    assert_eq!(r.daylight.unwrap().day, 80);
    assert_eq!(validate(&recipe(DAYLIGHT)), Vec::new());
}

#[test]
fn a_sun_without_a_model_names_both() {
    assert_eq!(
        error("[sun]\ntoward = [0.3, 0.8, 0.5]\nirradiance = 3.0\n"),
        "line 3: sun.model: missing, expected \"daylight\" or \"authored\": `toward`, `color` and `irradiance` are `model = \"authored\"`, `hour`, `day`, `latitude` and `heading` are `model = \"daylight\"`"
    );
    assert_eq!(
        error("[sun]\nmodel = \"solar\"\n"),
        "line 4: sun.model: expected \"daylight\" or \"authored\", found \"solar\""
    );
}

#[test]
fn the_old_daylight_table_is_refused_with_the_new_spelling() {
    assert_eq!(
        error("[daylight]\nhour = 9.0\nlatitude = 52.0\n"),
        "line 3: daylight: `[daylight]` is part of `[sun]` now, expected `[sun]` with `model = \"daylight\"` and the same keys"
    );
}

#[test]
fn each_sun_model_takes_only_its_own_keys() {
    assert_eq!(
        error(&format!("{DAYLIGHT}irradiance = 3.0\n")),
        "line 8: sun.irradiance: unknown key `irradiance` in `sun`, expected one of: model, hour, day, latitude, heading, reference_hour"
    );
    assert_eq!(
        error(&format!("{AUTHORED}latitude = 52.0\n")),
        "line 7: sun.latitude: unknown key `latitude` in `sun`, expected one of: model, hour, toward, radius_deg, color, irradiance"
    );
    assert_eq!(
        error(&format!("{AUTHORED}hour = 9.0\n")),
        "line 7: sun.hour: an authored sun takes the hour of the shot or the window, expected `hour` removed"
    );
}

#[test]
fn the_daylight_names_its_latitude_and_heading_since_the_scene_files_defaults_differ() {
    assert_eq!(
        error("[sun]\nmodel = \"daylight\"\nhour = 9.0\nlatitude = 52.0\n"),
        "line 3: sun.heading: missing, expected a number"
    );
    assert_eq!(
        error("[sun]\nmodel = \"daylight\"\nhour = 9.0\nheading = 0.0\n"),
        "line 3: sun.latitude: missing, expected a number"
    );
}

#[test]
fn the_daylight_hour_and_reference_hour_are_checked() {
    assert_eq!(
        problems("[sun]\nmodel = \"daylight\"\nhour = 30.0\nlatitude = 52.0\nheading = 0.0\n"),
        ["line 5: sun.hour: expected an hour from 0 to 24"]
    );
    assert_eq!(
        problems(&format!("{DAYLIGHT}reference_hour = 12.0\n")),
        [
            "line 8: sun.reference_hour: Manfred lights every scene against its reference hour, expected `reference_hour = 16.0` or the key removed"
        ]
    );
}

#[test]
fn the_color_key_is_spelled_as_the_scene_file_spells_it() {
    assert_eq!(
        error(&format!("{AUTHORED}colour = [1.0, 0.9, 0.8]\n")),
        "line 7: sun.colour: unknown key `colour` in `sun`, expected one of: model, hour, toward, radius_deg, color, irradiance"
    );
    assert_eq!(
        error(
            "[[light]]\nname = \"a\"\nposition = [0.0, 1.0, 0.0]\nintensity = 1.0\ncolour = [1.0, 1.0, 1.0]\n"
        ),
        "line 7: light[a].colour: unknown key `colour` in `light[a]`, expected one of: name, position, color, intensity, radius, range, shadow"
    );
    assert_eq!(
        error(
            "[sky]\nkind = \"room\"\n[sky.room]\nfloor = [0.1, 0.1, 0.1]\n[[sky.room.lights]]\ncolour = [1.0, 1.0, 1.0]\n"
        ),
        "line 8: sky.room.lights[0].colour: unknown key `colour` in `sky.room.lights[0]`, expected one of: name, az, el, power, color, width, height, soft, slats, open"
    );
}

#[test]
fn the_scene_files_finish_table_points_at_look() {
    assert_eq!(
        error("[finish]\nstyle = \"noir\"\n"),
        "line 3: finish: `[finish]` is the scene file's table, expected the recipe's own `[look]` with `view`, `exposure` and `grade`; the scene file's `[finish]` is written from it"
    );
    let r = recipe("[look]\nview = \"standard\"\nexposure = 1.2\n");
    assert_eq!(r.look.unwrap().exposure, 1.2);
}

#[test]
fn a_sun_is_patched_and_varied_by_its_new_paths() {
    let r = recipe(&format!(
        "{DAYLIGHT}[variant.noon]\nset = {{ \"sun.hour\" = 12.0 }}\n"
    ));
    let noon = manfred_recipe::apply_variant(&r, "noon").unwrap();
    assert_eq!(noon.daylight.unwrap().hour, 12.0);
    let r = recipe(&format!(
        "{DAYLIGHT}[variant.old]\nset = {{ \"daylight.hour\" = 12.0 }}\n"
    ));
    let error = manfred_recipe::apply_variant(&r, "old").unwrap_err();
    assert!(error.to_string().contains("daylight.hour"), "{error}");
}

#[test]
fn trace_reads_transmissive_shadows_and_refuses_an_unknown_key() {
    assert!(!recipe("").trace.transmissive_shadows);
    assert!(!recipe("[trace]\n").trace.transmissive_shadows);
    assert!(
        recipe("[trace]\ntransmissive_shadows = true\n")
            .trace
            .transmissive_shadows
    );
    let e = error("[trace]\nbounces = 4\n");
    assert!(e.contains("trace") && e.contains("bounces"), "{e}");
    let e = error("[trace]\ntransmissive_shadows = 1\n");
    assert!(e.contains("transmissive_shadows"), "{e}");
}

#[test]
fn trace_reads_the_firefly_settings_off_by_default_and_refuses_them_out_of_range_at_their_line() {
    let off = recipe("").trace;
    assert_eq!((off.clamp_indirect, off.filter_glossy), (0.0, 0.0));
    let on = recipe("[trace]\nclamp_indirect = 4\nfilter_glossy = 0.25\n").trace;
    assert_eq!((on.clamp_indirect, on.filter_glossy), (4.0, 0.25));
    assert_eq!(
        problems("[trace]\nclamp_indirect = 10000\nfilter_glossy = 1.0\n"),
        Vec::<String>::new()
    );
    assert_eq!(
        problems("[trace]\nclamp_indirect = -1\nfilter_glossy = 1.5\n"),
        [
            "line 4: trace.clamp_indirect: clamp_indirect = -1 is outside 0 to 10000, expected the luminance that caps each light contribution after the first bounce, or 0 for off",
            "line 5: trace.filter_glossy: filter_glossy = 1.5 is outside 0 to 1, expected the least roughness after a diffuse bounce, or 0 for off"
        ]
    );
    let e = error("[trace]\nfilter_glossy = nan\n");
    assert!(
        e.contains("line 4: trace.filter_glossy: expected a finite number"),
        "{e}"
    );
    let e = error("[trace]\nfilter_glossy = \"soft\"\n");
    assert!(e.contains("filter_glossy"), "{e}");
}
