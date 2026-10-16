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
const SUN_BEARING_DEG: f64 = 293.0;
const LANE_WIDTH: f64 = 3.2;
const INNER: f64 = LANE_WIDTH * 0.5;
const OUTER: f64 = LANE_WIDTH * 1.5;
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

fn heading(route: &[[f64; 3]]) -> f64 {
    let (first, last) = (route[0], route[route.len() - 1]);
    let across = (-(last[0] - first[0]))
        .atan2(last[2] - first[2])
        .to_degrees();
    round((SUN_BEARING_DEG - across).rem_euclid(360.0), 1)
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
    let _ = writeln!(w, "seconds = 10.0");
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
            "route = \"main\", lanes = 2, lane_width = {}, crosswalks = [{crossings}], seed = {seed}",
            num(LANE_WIDTH)
        ),
    );
    object(
        w,
        "buildings",
        "manfred/buildings",
        format!("route = \"main\", seed = {}", seed.wrapping_add(1)),
    );
    object(
        w,
        "furniture",
        "manfred/furniture",
        format!(
            "route = \"main\", crosswalks = [{crossings}], seed = {}",
            seed.wrapping_add(2)
        ),
    );
    object(
        w,
        "hero",
        "manfred/car",
        format!("style = \"sports\", paint = \"{paint}\", seed = {seed}"),
    );
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
    let _ = writeln!(w, "[[follow]]");
    let _ = writeln!(w, "object = \"hero\"");
    let _ = writeln!(w, "route = \"main\"");
    let _ = writeln!(w, "lane = {}", num(INNER));
    let _ = writeln!(w, "start = 0.0");
    let _ = writeln!(w, "speed = [[0.0, 18.0], [10.0, 32.0]]");
    let _ = writeln!(
        w,
        "drift = {{ amount = 0.35, period = 2.5, slip_deg = 4.0 }}"
    );
    let _ = writeln!(w);
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
    let _ = writeln!(w, "[[camera]]");
    let _ = writeln!(w, "name = \"chase\"");
    let _ = writeln!(w, "kind = \"perspective\"");
    let _ = writeln!(
        w,
        "position = {}",
        point([start[0] + INNER, 1.1, start[2] - 5.5])
    );
    let _ = writeln!(
        w,
        "target = {}",
        point([start[0] + INNER, 0.8, start[2] + 6.0])
    );
    let _ = writeln!(w, "fov_y_deg = 55.0");
    let _ = writeln!(w, "fstop = 2.8");
    let _ = writeln!(w);
    let _ = writeln!(w, "[camera.rig]");
    let _ = writeln!(w, "follow = \"hero\"");
    let _ = writeln!(w, "offset = [0.0, 1.1, -5.5]");
    let _ = writeln!(w, "look = [0.0, 0.8, 6.0]");
    let _ = writeln!(w, "lag = 0.25");
    let _ = writeln!(w, "look_ahead = 0.2");
    let _ = writeln!(
        w,
        "shake = {{ translation = 0.02, roll = 0.4, frequency = 7.0, seed = {seed} }}"
    );
    let _ = writeln!(w, "focus = \"follow\"");
    let _ = writeln!(w);
    let _ = writeln!(w, "[sun]");
    let _ = writeln!(w, "model = \"daylight\"");
    let _ = writeln!(w, "hour = 18.6");
    let _ = writeln!(w, "day = 172");
    let _ = writeln!(w, "latitude = 44.4");
    let _ = writeln!(w, "heading = {}", num(heading(&route)));
    let _ = writeln!(w);
    let _ = writeln!(w, "[sky]");
    let _ = writeln!(w, "kind = \"analytic\"");
    let _ = writeln!(w, "intensity = 1.0");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "# A lilac LUT belongs here once the grade takes one (the scene's [finish] lut and lut_amount)."
    );
    let _ = writeln!(w, "[look]");
    let _ = writeln!(w, "view = \"graded\"");
    let _ = writeln!(w, "exposure = 1.0");
    let _ = writeln!(
        w,
        "grade = {{ vignette = 0.25, saturation = 1.05, warmth = 0.15, bloom = 0.15, haze = 0.0 }}"
    );
    let _ = writeln!(w);
    for wheel in ["rl", "rr"] {
        let _ = writeln!(w, "[[particles]]");
        let _ = writeln!(w, "name = \"spray_{wheel}\"");
        let _ = writeln!(w, "parent = \"hero.wheel_{wheel}\"");
        let _ = writeln!(w, "at = [0.0, 0.0, 0.0]");
        let _ = writeln!(w, "rate = 0.0");
        let _ = writeln!(w, "rate_keys = [[0.0, 0.0], [4.0, 400.0]]");
        let _ = writeln!(w, "max = 2000");
        let _ = writeln!(w, "lifetime = [0.4, 0.9]");
        let _ = writeln!(w, "speed = [2.0, 5.0]");
        let _ = writeln!(w, "direction = [0.0, 0.6, -1.0]");
        let _ = writeln!(w, "cone = 25.0");
        let _ = writeln!(w, "inherit = 0.6");
        let _ = writeln!(w, "gravity = [0.0, -9.8, 0.0]");
        let _ = writeln!(w, "drag = 1.5");
        let _ = writeln!(w, "size = [0.05, 0.6]");
        let _ = writeln!(w, "alpha = [[0.0, 0.5], [1.0, 0.0]]");
        let _ = writeln!(w, "color = \"#d9dde3\"");
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
    let _ = writeln!(w, "amount = 0.25");
    let _ = writeln!(w, "height = 0.0");
    let _ = writeln!(w, "falloff = 0.08");
    let _ = writeln!(w, "shafts = 0.6");
    let _ = writeln!(w, "steps = 32");
    let _ = writeln!(w, "color = \"#ffc79a\"");
    let _ = writeln!(w);
    let _ = writeln!(w, "[flare]");
    let _ = writeln!(w, "amount = 0.4");
    let _ = writeln!(w, "ghosts = 4");
    let _ = writeln!(w, "ghost_spacing = 0.3");
    let _ = writeln!(w, "halo = 0.2");
    let _ = writeln!(w, "halo_radius = 0.45");
    let _ = writeln!(w, "starburst = 0.3");
    let _ = writeln!(w, "threshold = 1.5");
    Made {
        text: t,
        route,
        objects,
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
    let starter = street(args.seed);
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, &starter.text)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
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
            "timeline": { "fps": 60, "seconds": 10.0, "size": [1080, 1920] },
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
        assert_eq!(rig["lag"].as_float(), Some(0.25));
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
