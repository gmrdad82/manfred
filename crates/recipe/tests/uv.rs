use manfred_recipe::{Uv, parse, validate};

const PLATE: &str =
    "[plate]\nwidth = 200\nheight = 100\nmetres_per_pixel = 0.01\norigin = \"top_left\"\n";
const CAMERA: &str = "[[camera]]\nname = \"main\"\nposition = [0.0, 0.0, 2.0]\ntarget = [0.0, 0.0, 0.0]\nfov_y_deg = 40.0\n";

fn text(head: &str, uv: &str) -> String {
    format!(
        "manfred = 1\nname = \"t\"\n{head}[[object]]\nname = \"a\"\nkind = \"box\"\nsize = [1.0, 1.0, 1.0]\nmaterial = \"m\"\nuv = {uv}\n"
    )
}

fn uv(head: &str, value: &str) -> Uv {
    parse(&text(head, value)).unwrap().objects[0]
        .uv
        .clone()
        .unwrap()
}

fn error(head: &str, value: &str) -> String {
    parse(&text(head, value)).unwrap_err().to_string()
}

fn problems(head: &str, value: &str) -> Vec<String> {
    validate(&parse(&text(head, value)).unwrap())
        .iter()
        .map(|p| format!("{}: {}", p.path, p.text))
        .collect()
}

#[test]
fn a_rect_reads_plate_pixels_and_takes_its_defaults() {
    let got = uv(PLATE, "{ kind = \"rect\", rect_px = [150, 75, 50, 25] }");
    assert_eq!(
        got,
        Uv::Rect {
            rect: [-0.5, -0.25, 0.5, 0.25],
            atlas: [0.0, 0.0, 1.0, 1.0],
            facing: 0.5,
            clamp: false,
            away: [-1.0, -1.0],
        }
    );
    let full = uv(
        "",
        "{ kind = \"rect\", rect = [1.0, 2.0, 0.0, 0.0], atlas = [0.0, 0.1, 0.2, 0.3], facing = 0.0, clamp = true, away = [-1.0, 2.0] }",
    );
    assert_eq!(
        full,
        Uv::Rect {
            rect: [0.0, 0.0, 1.0, 2.0],
            atlas: [0.0, 0.1, 0.2, 0.3],
            facing: 0.0,
            clamp: true,
            away: [-1.0, 2.0],
        }
    );
}

#[test]
fn a_cylinder_reads_its_band_and_wall() {
    assert_eq!(
        uv(
            "",
            "{ kind = \"cylinder\", degrees = -52.0, band = [0.004, 0.066], wall = [0.002, 0.068] }"
        ),
        Uv::Cylinder {
            degrees: -52.0,
            atlas: [0.0, 0.0, 1.0, 1.0],
            band: Some([0.004, 0.066]),
            wall: Some([0.002, 0.068]),
            away: [-1.0, -1.0],
        }
    );
    assert_eq!(
        uv("", "{ kind = \"cylinder\" }"),
        Uv::Cylinder {
            degrees: 360.0,
            atlas: [0.0, 0.0, 1.0, 1.0],
            band: None,
            wall: None,
            away: [-1.0, -1.0],
        }
    );
}

#[test]
fn a_bad_uv_names_its_key() {
    let both = error(
        PLATE,
        "{ kind = \"rect\", rect = [0.0, 0.0, 1.0, 1.0], rect_px = [0, 0, 1, 1] }",
    );
    assert!(both.contains("`rect` and `rect_px` are both set"), "{both}");
    let none = error("", "{ kind = \"rect\" }");
    assert!(none.contains("uv.rect: missing"), "{none}");
    let kind = error("", "{ kind = \"box\" }");
    assert!(kind.contains("expected \"cylinder\" or \"rect\""), "{kind}");
    let extra = error(
        "",
        "{ kind = \"rect\", rect = [0.0, 0.0, 1.0, 1.0], band = [0.0, 1.0] }",
    );
    assert!(extra.contains("band"), "{extra}");
    let word = error("", "\"camera\"");
    assert!(word.contains("\"plate\", \"local\" or \"none\""), "{word}");
}

#[test]
fn validation_checks_the_plate_its_camera_and_degenerate_spans() {
    assert_eq!(
        problems(&format!("{PLATE}{CAMERA}"), "\"plate\""),
        Vec::<String>::new()
    );
    assert!(problems(CAMERA, "\"plate\"")[0].contains("needs a `[plate]`"));
    assert!(problems(PLATE, "\"plate\"")[0].contains("needs a `[[camera]]`"));
    let named = format!("{}camera = \"side\"\n{CAMERA}", PLATE);
    assert_eq!(
        problems(&named, "\"plate\""),
        ["plate.camera: no camera named `side`, expected the name of a `[[camera]]`"]
    );
    let flat = problems("", "{ kind = \"rect\", rect = [0.0, 0.0, 1.0, 0.0] }");
    assert_eq!(
        flat,
        ["object[a].uv.rect: the rect has no area, expected x1 > x0 and y1 > y0"]
    );
    let band = problems(
        "",
        "{ kind = \"cylinder\", band = [0.5, 0.5], wall = [0.0, 1.0] }",
    );
    assert_eq!(
        band,
        ["object[a].uv.band: expected [low, high] with high above low"]
    );
}
