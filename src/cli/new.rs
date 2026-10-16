use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use manfred_generators::pyrandom::Random;
use serde_json::json;

use super::refuse;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    starter: Starter,
}

#[derive(clap::Subcommand)]
enum Starter {
    #[command(
        about = "A starter chase shot: a 9:16 timeline, a street along a route, a hero car, traffic and a chase camera"
    )]
    Street(Street),
}

#[derive(clap::Args)]
pub(super) struct Street {
    #[arg(
        long,
        value_name = "N",
        default_value_t = 1,
        help = "The seed of the route, the cars and the street"
    )]
    seed: u64,
    #[arg(
        long,
        value_name = "file",
        default_value = "street.toml",
        help = "The recipe to write; an existing file is refused"
    )]
    out: PathBuf,
    #[arg(long, help = "Print one JSON object")]
    json: bool,
}

pub(super) const POINTS: usize = 8;
const LANE_WIDTH: f64 = 3.5;
const SIDEWALK: f64 = 4.0;
const SETBACK: f64 = 12.0;
const KERB_INSET: f64 = 1.8;
const INNER: f64 = LANE_WIDTH * 0.5;
const OUTER: f64 = LANE_WIDTH * 1.5;
const SECONDS: f64 = 10.0;
const HERO_SPEED: [f64; 2] = [18.0, 32.0];
const DAY: u32 = 172;
const LATITUDE: f64 = 44.4;
const SUN_ELEVATION_DEG: f64 = 5.0;
const EYE: f64 = 0.75;
const FOV_Y_DEG: f64 = 80.0;
const ASPECT: f64 = 1080.0 / 1920.0;
const CAR_SHARE: f64 = 0.375;
const CAR_DOWN: f64 = 0.6;
const LOOK_AT_M: f64 = 8.0;
const LAG_S: f64 = 0.25;
const DRIFT: [f64; 3] = [1.2, 2.5, 26.0];
const LUT: &str = "out/lilac.cube";
const LUT_SIZE: usize = 33;
const TRAFFIC: [&str; 3] = ["sedan", "hatch", "van"];
const HERO_PAINTS: [&str; 4] = ["#5b0f1a", "#0e2a4f", "#14361f", "#2a0f3d"];

pub(super) struct Made {
    pub(super) text: String,
    pub(super) route: Vec<[f64; 3]>,
    pub(super) objects: Vec<String>,
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    let rounded = (value * scale).round() / scale;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn num(value: f64) -> String {
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') {
        text
    } else {
        format!("{text}.0")
    }
}

fn point(p: [f64; 3]) -> String {
    format!("[{}, {}, {}]", num(p[0]), num(p[1]), num(p[2]))
}

pub(super) fn route(seed: u64) -> Vec<[f64; 3]> {
    let mut random = Random::new(seed as i64);
    let length = random.uniform(560.0, 640.0);
    let amplitude = random.uniform(10.0, 22.0);
    let phase = random.uniform(-0.4, 0.4);
    let last = (POINTS - 1) as f64;
    (0..POINTS)
        .map(|i| {
            let t = i as f64 / last;
            let wobble = if i == 0 || i == POINTS - 1 {
                0.0
            } else {
                random.uniform(-2.0, 2.0)
            };
            let x = amplitude * ((std::f64::consts::TAU * t + phase).sin() - phase.sin()) + wobble;
            [round(x, 1), 0.0, round(length * t, 1)]
        })
        .collect()
}

pub(super) struct Sun {
    pub(super) hour: f64,
    pub(super) heading: f64,
    pub(super) elevation: f64,
    pub(super) azimuth: f64,
}

fn daylight(hour: f64, heading: f64) -> pfx_core::daylight::Sun {
    pfx_core::daylight::Daylight {
        hour,
        day: f64::from(DAY),
        latitude: LATITUDE,
        heading,
    }
    .sun()
}

fn travel() -> f64 {
    (HERO_SPEED[0] + HERO_SPEED[1]) * 0.5 * SECONDS
}

pub(super) fn stretch(route: &[[f64; 3]]) -> [f64; 2] {
    let line = manfred_motion::Route::new(route, false).ok();
    let (from, to) = match &line {
        Some(line) => {
            let (a, b) = (line.at(0.0).position, line.at(travel()).position);
            ([a.x, a.z], [b.x, b.z])
        }
        None => ([route[0][0], route[0][2]], [route[1][0], route[1][2]]),
    };
    let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
    let length = dx.hypot(dz).max(1e-9);
    [dx / length, dz / length]
}

pub(super) fn sun(route: &[[f64; 3]]) -> Sun {
    let (mut low, mut high) = (12.0, 22.0);
    for _ in 0..60 {
        let mid = 0.5 * (low + high);
        if daylight(mid, 180.0).elevation > SUN_ELEVATION_DEG {
            low = mid;
        } else {
            high = mid;
        }
    }
    let hour = round(0.5 * (low + high), 2);
    let ahead = stretch(route);
    let across = (-ahead[0]).atan2(ahead[1]).to_degrees();
    let heading = round(
        (daylight(hour, 180.0).azimuth - across).rem_euclid(360.0),
        1,
    );
    let placed = daylight(hour, heading);
    Sun {
        hour,
        heading,
        elevation: placed.elevation,
        azimuth: placed.azimuth,
    }
}

pub(super) struct Hero {
    half_track: f64,
    rear: f64,
    length: f64,
    width: f64,
    height: f64,
}

fn chase(hero: &Hero) -> ([f64; 3], [f64; 3]) {
    let half = (FOV_Y_DEG.to_radians() * 0.5).tan();
    let distance = hero.width / (CAR_SHARE * 2.0 * half * ASPECT);
    let back = -(hero.length * 0.5 + distance);
    let below = ((EYE - hero.height * 0.5) / distance).atan();
    let wanted = ((2.0 * CAR_DOWN - 1.0) * half).atan();
    let look = EYE + (LOOK_AT_M - back) * (wanted - below).tan();
    ([0.0, EYE, round(back, 2)], [0.0, round(look, 2), LOOK_AT_M])
}

fn grade(input: [f64; 3]) -> [f64; 3] {
    let luma = 0.2126 * input[0] + 0.7152 * input[1] + 0.0722 * input[2];
    let bright = luma.clamp(0.0, 1.0).powf(0.4);
    let curve = bright + 0.6 * bright * (1.0 - bright) * (bright - 0.5);
    let lifted = 0.09 + 0.91 * curve.clamp(0.0, 1.0);
    let high = ((lifted - 0.55) / 0.4).clamp(0.0, 1.0);
    let high = high * high * (3.0 - 2.0 * high);
    let lilac = [1.08, 0.82, 1.15];
    let warm = [1.05, 0.94, 0.98];
    [0, 1, 2].map(|k| {
        let tint = lilac[k] + (warm[k] - lilac[k]) * high;
        (lifted * tint + 0.4 * (input[k] - luma)).clamp(0.0, 1.0)
    })
}

pub(super) fn lilac() -> String {
    let mut text = String::new();
    let _ = writeln!(text, "TITLE \"lilac\"");
    let _ = writeln!(text, "LUT_3D_SIZE {LUT_SIZE}");
    let _ = writeln!(text, "DOMAIN_MIN 0.0 0.0 0.0");
    let _ = writeln!(text, "DOMAIN_MAX 1.0 1.0 1.0");
    let last = (LUT_SIZE - 1) as f64;
    for b in 0..LUT_SIZE {
        for g in 0..LUT_SIZE {
            for r in 0..LUT_SIZE {
                let out = grade([r as f64 / last, g as f64 / last, b as f64 / last]);
                let _ = writeln!(text, "{:.6} {:.6} {:.6}", out[0], out[1], out[2]);
            }
        }
    }
    text
}

fn follow_hero(w: &mut String) {
    let _ = writeln!(w, "[[follow]]");
    let _ = writeln!(w, "object = \"hero\"");
    let _ = writeln!(w, "route = \"main\"");
    let _ = writeln!(w, "lane = {}", num(INNER));
    let _ = writeln!(w, "start = 0.0");
    let _ = writeln!(
        w,
        "speed = [[0.0, {}], [{}, {}]]",
        num(HERO_SPEED[0]),
        num(SECONDS),
        num(HERO_SPEED[1])
    );
    let _ = writeln!(
        w,
        "drift = {{ amount = {}, period = {}, slip_deg = {} }}",
        num(DRIFT[0]),
        num(DRIFT[1]),
        num(DRIFT[2])
    );
    let _ = writeln!(w);
}

pub(super) fn street(seed: u64) -> Made {
    let route = route(seed);
    let mut random = Random::new(seed.wrapping_add(0x5eed) as i64);
    let paint = HERO_PAINTS[random.randbelow(HERO_PAINTS.len() as u64) as usize];
    let crossings = [90.0, 260.0, 430.0]
        .map(|at: f64| num(round(at + random.uniform(-20.0, 20.0), 1)))
        .join(", ");
    let mut t = String::new();
    let w = &mut t;
    let _ = writeln!(w, "manfred = 1");
    let _ = writeln!(w, "name = \"street\"");
    let _ = writeln!(w, "seed = {seed}");
    let _ = writeln!(w);
    let _ = writeln!(w, "[output]");
    let _ = writeln!(w, "glb = \"out/street.glb\"");
    let _ = writeln!(w, "scene = \"out/street.scene.toml\"");
    let _ = writeln!(w, "scene_camera = \"chase\"");
    let _ = writeln!(w);
    let _ = writeln!(w, "[timeline]");
    let _ = writeln!(w, "fps = 60");
    let _ = writeln!(w, "seconds = {}", num(SECONDS));
    let _ = writeln!(w, "size = [1080, 1920]");
    let _ = writeln!(w);
    let _ = writeln!(w, "[[route]]");
    let _ = writeln!(w, "name = \"main\"");
    let _ = writeln!(w, "points = [");
    for p in &route {
        let _ = writeln!(w, "  {},", point(*p));
    }
    let _ = writeln!(w, "]");
    let _ = writeln!(w);
    let mut objects = Vec::new();
    let mut object = |w: &mut String, name: &str, generator: &str, params: String| {
        objects.push(name.to_string());
        let _ = writeln!(w, "[[object]]");
        let _ = writeln!(w, "name = \"{name}\"");
        let _ = writeln!(w, "kind = \"generator\"");
        let _ = writeln!(w, "generator = \"{generator}\"");
        let _ = writeln!(w, "params = {{ {params} }}");
        let _ = writeln!(w);
    };
    object(
        w,
        "road",
        "manfred/road",
        format!(
            "route = \"main\", lanes = 2, lane_width = {}, sidewalk = {}, crosswalks = [{crossings}], seed = {seed}",
            num(LANE_WIDTH),
            num(SIDEWALK)
        ),
    );
    object(
        w,
        "buildings",
        "manfred/buildings",
        format!(
            "route = \"main\", style = \"lowrise\", setback = {}, seed = {}",
            num(SETBACK),
            seed.wrapping_add(1)
        ),
    );
    object(
        w,
        "furniture",
        "manfred/furniture",
        format!(
            "route = \"main\", offset = {}, crosswalks = [{crossings}], seed = {}",
            num(round(2.0 * LANE_WIDTH + KERB_INSET, 2)),
            seed.wrapping_add(2)
        ),
    );
    let hero = format!("style = \"sports\", paint = \"{paint}\", seed = {seed}");
    let car = hero_car(&hero);
    object(w, "hero", "manfred/car", hero);
    let mut traffic = Vec::new();
    for (index, style) in TRAFFIC.iter().enumerate() {
        let name = format!("traffic{}", index + 1);
        let car_seed = seed.wrapping_add(10 + index as u64);
        object(
            w,
            &name,
            "manfred/car",
            format!("style = \"{style}\", seed = {car_seed}"),
        );
        let start = round(35.0 + 62.0 * index as f64 + random.uniform(0.0, 12.0), 1);
        let speed = round(random.uniform(11.0, 15.0), 1);
        traffic.push((name, start, speed));
    }
    follow_hero(w);
    for (name, start, speed) in &traffic {
        let _ = writeln!(w, "[[follow]]");
        let _ = writeln!(w, "object = \"{name}\"");
        let _ = writeln!(w, "route = \"main\"");
        let _ = writeln!(w, "lane = {}", num(OUTER));
        let _ = writeln!(w, "start = {}", num(*start));
        let _ = writeln!(w, "speed = [[0.0, {}]]", num(*speed));
        let _ = writeln!(w);
    }
    let start = route[0];
    let (offset, look) = chase(&car);
    let _ = writeln!(w, "[[camera]]");
    let _ = writeln!(w, "name = \"chase\"");
    let _ = writeln!(w, "kind = \"perspective\"");
    let _ = writeln!(
        w,
        "position = {}",
        point([
            round(start[0] + INNER, 2),
            offset[1],
            round(start[2] + offset[2], 2)
        ])
    );
    let _ = writeln!(
        w,
        "target = {}",
        point([
            round(start[0] + INNER, 2),
            look[1],
            round(start[2] + look[2], 2)
        ])
    );
    let _ = writeln!(w, "fov_y_deg = {}", num(FOV_Y_DEG));
    let _ = writeln!(w, "fstop = 2.8");
    let _ = writeln!(w);
    let _ = writeln!(w, "[camera.rig]");
    let _ = writeln!(w, "follow = \"hero\"");
    let _ = writeln!(w, "offset = {}", point(offset));
    let _ = writeln!(w, "look = {}", point(look));
    let _ = writeln!(w, "lag = {}", num(LAG_S));
    let _ = writeln!(w, "look_ahead = 0.2");
    let _ = writeln!(w, "bank = 4.0");
    let _ = writeln!(w, "yaw_lag = 0.4");
    let _ = writeln!(
        w,
        "shake = {{ translation = 0.02, roll = 0.4, frequency = 7.0, seed = {seed} }}"
    );
    let _ = writeln!(w, "focus = \"follow\"");
    let _ = writeln!(w);
    let low = sun(&route);
    let _ = writeln!(w, "[sun]");
    let _ = writeln!(w, "model = \"daylight\"");
    let _ = writeln!(w, "hour = {}", num(low.hour));
    let _ = writeln!(w, "day = {DAY}");
    let _ = writeln!(w, "latitude = {}", num(LATITUDE));
    let _ = writeln!(w, "heading = {}", num(low.heading));
    let _ = writeln!(w);
    let _ = writeln!(w, "[sky]");
    let _ = writeln!(w, "kind = \"analytic\"");
    let _ = writeln!(w, "intensity = 1.5");
    let _ = writeln!(w);
    let _ = writeln!(w, "[look]");
    let _ = writeln!(w, "view = \"graded\"");
    let _ = writeln!(w, "exposure = 2.6");
    let _ = writeln!(
        w,
        "grade = {{ vignette = 0.25, saturation = 1.0, warmth = 0.05, bloom = 0.15, haze = 0.0 }}"
    );
    let _ = writeln!(w, "lut = \"{LUT}\"");
    let _ = writeln!(w, "lut_amount = 0.8");
    let _ = writeln!(w);
    for (wheel, side) in [("rl", 1.0), ("rr", -1.0)] {
        let _ = writeln!(w, "[[particles]]");
        let _ = writeln!(w, "name = \"spray_{wheel}\"");
        let _ = writeln!(w, "parent = \"hero.body\"");
        let _ = writeln!(
            w,
            "at = {}",
            point([
                round(side * car.half_track, 3),
                0.05,
                round(car.rear - 0.3, 3)
            ])
        );
        let _ = writeln!(w, "rate = 300.0");
        let _ = writeln!(w, "rate_keys = [[0.0, 300.0], [{}, 1500.0]]", num(SECONDS));
        let _ = writeln!(w, "max = 2000");
        let _ = writeln!(w, "lifetime = [0.25, 0.7]");
        let _ = writeln!(w, "speed = [2.0, 5.0]");
        let _ = writeln!(w, "direction = [0.0, 0.6, -1.0]");
        let _ = writeln!(w, "cone = 25.0");
        let _ = writeln!(w, "inherit = 0.7");
        let _ = writeln!(w, "gravity = [0.0, -9.8, 0.0]");
        let _ = writeln!(w, "drag = 3.0");
        let _ = writeln!(w, "size = [0.01, 0.06]");
        let _ = writeln!(w, "alpha = [[0.0, 0.0], [0.1, 0.25], [1.0, 0.0]]");
        let _ = writeln!(w, "color = \"#c9d3dc\"");
        let _ = writeln!(w, "stretch = 3.0");
        let _ = writeln!(w, "blend = \"alpha\"");
        let _ = writeln!(w, "lit = true");
        let _ = writeln!(w, "seed = {seed}");
        let _ = writeln!(w);
    }
    let _ = writeln!(w, "[motion_blur]");
    let _ = writeln!(w, "shutter = 0.5");
    let _ = writeln!(w, "max_px = 48.0");
    let _ = writeln!(w, "samples = 12");
    let _ = writeln!(w);
    let _ = writeln!(w, "[haze]");
    let _ = writeln!(w, "amount = 0.3");
    let _ = writeln!(w, "height = 0.0");
    let _ = writeln!(w, "falloff = 0.08");
    let _ = writeln!(w, "shafts = 0.6");
    let _ = writeln!(w, "steps = 32");
    let _ = writeln!(w, "color = \"#e9d6ee\"");
    let _ = writeln!(w);
    let _ = writeln!(w, "[flare]");
    let _ = writeln!(w, "amount = 0.4");
    let _ = writeln!(w, "ghosts = 4");
    let _ = writeln!(w, "ghost_spacing = 0.3");
    let _ = writeln!(w, "halo = 0.1");
    let _ = writeln!(w, "halo_radius = 0.3");
    let _ = writeln!(w, "starburst = 0.3");
    let _ = writeln!(w, "threshold = 8.0");
    Made {
        text: t,
        route,
        objects,
    }
}

fn hero_car(params: &str) -> Hero {
    let text = format!(
        "manfred = 1\nname = \"hero\"\n\n[[object]]\nname = \"hero\"\nkind = \"generator\"\ngenerator = \"manfred/car\"\nparams = {{ {params} }}\n"
    );
    let built = manfred_recipe::parse(&text)
        .ok()
        .and_then(|recipe| manfred_build::build(&recipe, Path::new(".")).ok());
    let car = built.as_ref().map(|built| &built.facts.extra);
    let fact = |key: &str, fallback: f64| {
        car.and_then(|car| car.get(key))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(fallback)
    };
    let rear = car
        .and_then(|car| car.get("wheels"))
        .and_then(|wheels| wheels.pointer("/rl/centre/2"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(-1.3);
    Hero {
        half_track: fact("track", 1.6) * 0.5,
        rear,
        length: fact("length", 4.4),
        width: fact("width", 1.9),
        height: fact("height", 1.2),
    }
}

fn length(route: &[[f64; 3]]) -> f64 {
    route
        .windows(2)
        .map(|pair| {
            let d = [0, 1, 2].map(|i| pair[1][i] - pair[0][i]);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .sum()
}

fn run_street(args: &Street, out: &mut dyn Write) -> Result<(), String> {
    let path: &Path = &args.out;
    if path.exists() {
        return Err(format!(
            "{} already exists; pick another --out",
            path.display()
        ));
    }
    let lut = path.parent().unwrap_or(Path::new("")).join(LUT);
    let cube = lilac();
    if lut.exists() && std::fs::read_to_string(&lut).ok().as_deref() != Some(cube.as_str()) {
        return Err(format!(
            "{} already exists and is not the starter's LUT; pick another --out folder",
            lut.display()
        ));
    }
    let starter = street(args.seed);
    for dir in [path.parent(), lut.parent()]
        .into_iter()
        .flatten()
        .filter(|dir| !dir.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&lut, &cube).map_err(|e| format!("cannot write {}: {e}", lut.display()))?;
    std::fs::write(path, &starter.text)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let low = sun(&starter.route);
    if args.json {
        let line = json!({
            "recipe": path.display().to_string(),
            "seed": args.seed,
            "route": {
                "name": "main",
                "points": starter.route.len(),
                "chord_m": round(length(&starter.route), 1),
            },
            "objects": starter.objects,
            "camera": "chase",
            "timeline": { "fps": 60, "seconds": SECONDS, "size": [1080, 1920] },
            "sun": {
                "hour": low.hour,
                "heading": low.heading,
                "elevation_deg": round(low.elevation, 2),
                "azimuth_deg": round(low.azimuth, 2),
            },
            "lut": lut.display().to_string(),
        });
        let _ = writeln!(out, "{line}");
    } else {
        let _ = writeln!(out, "{}", path.display());
    }
    Ok(())
}

pub fn run(args: &Args) -> ExitCode {
    match &args.starter {
        Starter::Street(street) => match run_street(street, &mut std::io::stdout()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => refuse("new", &message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manfred_recipe::toml_edit::{DocumentMut, Item};

    fn parsed(words: &[&str]) -> Result<Street, String> {
        match super::super::parsed(&[&["new"], words].concat())? {
            super::super::Verb::New(Args {
                starter: Starter::Street(street),
            }) => Ok(street),
            _ => Err("not new street".into()),
        }
    }

    #[test]
    fn new_street_takes_a_seed_an_out_and_json() {
        let street = parsed(&["street", "--seed", "9", "--out", "a/b.toml", "--json"]).unwrap();
        assert_eq!(
            (street.seed, street.out.as_path(), street.json),
            (9, Path::new("a/b.toml"), true)
        );
        let plain = parsed(&["street"]).unwrap();
        assert_eq!(
            (plain.seed, plain.out.as_path(), plain.json),
            (1, Path::new("street.toml"), false)
        );
        for bad in [&["street", "--seed", "x"][..], &["avenue"], &[]] {
            assert!(parsed(bad).is_err(), "{}", bad.join(" "));
        }
        let missing = parsed(&[]).err().unwrap_or_default();
        assert!(
            missing.starts_with("error: 'manfred new' requires a subcommand"),
            "{missing}"
        );
    }

    #[test]
    fn the_street_is_the_same_for_a_seed_and_another_for_another() {
        let one = street(4);
        assert_eq!(one.text, street(4).text);
        assert_ne!(one.text, street(5).text);
        assert_eq!(one.route.len(), POINTS);
        let span = one.route[POINTS - 1][2] - one.route[0][2];
        assert!((560.0..=640.0).contains(&span), "{span}");
        assert!(one.route.iter().all(|p| p[1] == 0.0));
    }

    #[test]
    fn the_street_loads_checks_clean_and_sprays_from_the_body_behind_the_rear_wheels() {
        let recipe = manfred_recipe::parse(&street(1).text).unwrap();
        assert_eq!(manfred_recipe::validate(&recipe), []);
        let car = hero_car("style = \"sports\", seed = 1");
        let rear = car.rear;
        assert!(
            car.half_track > 0.5 && rear < 0.0,
            "{} {rear}",
            car.half_track
        );
        let doc: DocumentMut = street(1).text.parse().unwrap();
        let sprays = doc["particles"].as_array_of_tables().unwrap();
        for (spray, side) in sprays.iter().zip([1.0, -1.0]) {
            assert_eq!(spray["parent"].as_str(), Some("hero.body"));
            let at: Vec<f64> = spray["at"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_float().unwrap())
                .collect();
            assert_eq!(at[0].signum(), side);
            assert!(at[2] < rear, "{at:?}");
        }
    }

    #[test]
    fn the_sun_sits_low_down_the_main_stretch() {
        for seed in [1, 2, 7] {
            let made = street(seed);
            let recipe = manfred_recipe::parse(&made.text).unwrap();
            let day = recipe.daylight.unwrap();
            let at = pfx_core::daylight::Daylight {
                hour: day.hour,
                day: f64::from(day.day),
                latitude: day.latitude,
                heading: day.heading,
            }
            .sun();
            assert!((4.0..=6.0).contains(&at.elevation), "{}", at.elevation);
            let ahead = stretch(&made.route);
            let flat = at.y_up[0].hypot(at.y_up[2]);
            let cos = (at.y_up[0] * ahead[0] + at.y_up[2] * ahead[1]) / flat;
            let off = cos.clamp(-1.0, 1.0).acos().to_degrees();
            assert!(off < 10.0, "seed {seed}: {off} degrees off the stretch");
        }
    }

    #[test]
    fn the_hero_drifts_15_to_30_degrees_off_the_road() {
        let made = street(1);
        let mut text = String::from("manfred = 1\nname = \"drift\"\nseed = 1\n\n");
        let _ = writeln!(text, "[timeline]\nfps = 60\nseconds = {}\n", num(SECONDS));
        let _ = writeln!(text, "[[route]]\nname = \"main\"\npoints = [");
        for p in &made.route {
            let _ = writeln!(text, "  {},", point(*p));
        }
        let _ = writeln!(text, "]\n");
        let _ = writeln!(
            text,
            "[[object]]\nname = \"hero\"\nkind = \"generator\"\ngenerator = \"manfred/car\"\nparams = {{ style = \"sports\", seed = 1 }}\n"
        );
        follow_hero(&mut text);
        let recipe = manfred_recipe::parse(&text).unwrap();
        let built = manfred_build::build(&recipe, Path::new(".")).unwrap();
        let body = built
            .tracks
            .iter()
            .find(|track| track.node == "hero.body" || track.node == "hero")
            .unwrap();
        let route = manfred_motion::Route::new(&made.route, false).unwrap();
        let samples = route.sample(0.25);
        let mut most: f64 = 0.0;
        for key in &body.keys {
            let [x, y, z, w] = key.rotation;
            let forward = [
                2.0 * (x * z + w * y),
                2.0 * (y * z - w * x),
                1.0 - 2.0 * (x * x + y * y),
            ];
            let [px, _, pz] = key.translation;
            let near = samples
                .iter()
                .min_by(|a, b| {
                    let da = (a.1.position.x - px).hypot(a.1.position.z - pz);
                    let db = (b.1.position.x - px).hypot(b.1.position.z - pz);
                    da.total_cmp(&db)
                })
                .unwrap();
            let tangent = near.1.tangent;
            let yaw = (forward[0] * tangent.z - forward[2] * tangent.x)
                .atan2(forward[0] * tangent.x + forward[2] * tangent.z)
                .to_degrees();
            most = most.max(yaw.abs());
        }
        assert!((15.0..=30.0).contains(&most), "{most}");
    }

    #[test]
    fn the_street_is_a_toml_recipe_with_every_table_the_chase_needs() {
        let doc: DocumentMut = street(7).text.parse().unwrap();
        assert_eq!(doc["manfred"].as_integer(), Some(1));
        assert_eq!(doc["timeline"]["fps"].as_integer(), Some(60));
        assert_eq!(doc["timeline"]["seconds"].as_float(), Some(10.0));
        let size = doc["timeline"]["size"].as_array().unwrap();
        assert_eq!(
            size.iter()
                .filter_map(|v| v.as_integer())
                .collect::<Vec<_>>(),
            [1080, 1920]
        );
        let tables = |key: &str| match &doc[key] {
            Item::ArrayOfTables(list) => list.len(),
            _ => 0,
        };
        assert_eq!(
            (
                tables("route"),
                tables("object"),
                tables("follow"),
                tables("camera"),
                tables("particles")
            ),
            (1, 7, 4, 1, 2)
        );
        let points = doc["route"][0]["points"].as_array().unwrap();
        assert_eq!(points.len(), POINTS);
        let generators: Vec<&str> = match &doc["object"] {
            Item::ArrayOfTables(list) => list
                .iter()
                .filter_map(|o| o.get("generator").and_then(Item::as_str))
                .collect(),
            _ => Vec::new(),
        };
        assert_eq!(
            generators,
            [
                "manfred/road",
                "manfred/buildings",
                "manfred/furniture",
                "manfred/car",
                "manfred/car",
                "manfred/car",
                "manfred/car"
            ]
        );
        let rig = &doc["camera"][0]["rig"];
        assert_eq!(rig["follow"].as_str(), Some("hero"));
        assert_eq!(rig["lag"].as_float(), Some(LAG_S));
        for table in [
            "sun",
            "sky",
            "look",
            "motion_blur",
            "haze",
            "flare",
            "output",
        ] {
            assert!(doc[table].is_table(), "{table}");
        }
        assert_eq!(doc["motion_blur"]["shutter"].as_float(), Some(0.5));
    }

    #[test]
    fn an_existing_file_is_refused() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/new-street");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("street.toml");
        let args = Street {
            seed: 3,
            out: file.clone(),
            json: true,
        };
        let mut out = Vec::new();
        run_street(&args, &mut out).unwrap();
        let line: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(line["seed"], 3);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), street(3).text);
        let again = run_street(&args, &mut Vec::new()).unwrap_err();
        assert!(
            again.ends_with("already exists; pick another --out"),
            "{again}"
        );
    }
}
