use manfred_recipe::{Casters, Problem, Proxy, Recipe, parse, validate};

const BASE: &str = "manfred = 1
name = \"t\"

[output]
bake = \"bake.toml\"
scene = \"t.scene.toml\"

[[object]]
name = \"top\"
kind = \"box\"
size = [1.0, 0.1, 1.0]

[[object]]
name = \"mv_lid\"
kind = \"box\"
size = [0.1, 0.1, 0.1]

[[mover]]
group = \"lid\"
pivot = [0.0, 0.0, 0.0]
axis = [0.0, 1.0, 0.0]
travel = 0.1

[[camera]]
name = \"main\"
position = [0.0, 1.0, 2.0]
target = [0.0, 0.0, 0.0]
fov_y_deg = 30.0

[bake]
product = \"sample\"
anchors = [8.0, 12.0]
samples = 4
[[bake.volume]]
name = \"room\"
min = [0.0, 0.0, 0.0]
max = [1.0, 1.0, 1.0]
spacing = 0.5

[bake.plates]
size = [1920, 1080]
dir = \"plates/main\"
";

fn edited(edits: &[(&str, &str)]) -> String {
    let mut text = BASE.to_string();
    for (from, to) in edits {
        assert!(text.contains(from), "{from}");
        text = text.replacen(from, to, 1);
    }
    text
}

fn recipe(edits: &[(&str, &str)]) -> Recipe {
    parse(&edited(edits)).unwrap_or_else(|e| panic!("{e}"))
}

fn problems(edits: &[(&str, &str)]) -> Vec<String> {
    validate(&recipe(edits))
        .iter()
        .map(Problem::to_string)
        .collect()
}

fn line_of(text: &str, needle: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .rposition(|line| line.starts_with(needle))
        .unwrap_or_else(|| panic!("{needle}"))
        + 1
}

fn refused_at(edit: (&str, &str), key: &str, needle: &str, words: &str) {
    let text = edited(&[edit]);
    let found = problems(&[edit]);
    let line = line_of(&text, needle);
    assert!(
        found
            .iter()
            .any(|p| p.starts_with(&format!("line {line}: {key}: ")) && p.contains(words)),
        "{key} at line {line} with {words}: {found:?}"
    );
}

#[test]
fn plates_read_with_pfxs_defaults() {
    let r = recipe(&[]);
    assert_eq!(validate(&r), []);
    let plates = r.bake.unwrap().plates.unwrap();
    assert_eq!(plates.size, [1920, 1080]);
    assert_eq!((plates.overscan, plates.scale), (0.1, 1.0));
    assert!(plates.all_layers());
    assert_eq!(
        (
            plates.threshold,
            plates.min_samples,
            plates.max_samples,
            plates.growth
        ),
        (0.01, 16, 1024, 2.0)
    );
    assert_eq!(
        (plates.anchors, plates.seed, plates.key),
        (None, None, None)
    );
    assert_eq!(plates.dir.as_deref(), Some("plates/main"));
    assert_eq!(plates.casters, Casters::Meshes);
    assert_eq!((plates.clamp_indirect, plates.filter_glossy), (None, None));
    assert!(plates.views.is_empty());
    let r = recipe(&[(
        "dir = \"plates/main\"\n",
        "dir = \"plates/main\"\nclamp_indirect = 0\nfilter_glossy = 0.25\n",
    )]);
    assert_eq!(validate(&r), []);
    let plates = r.bake.unwrap().plates.unwrap();
    assert_eq!(
        (plates.clamp_indirect, plates.filter_glossy),
        (Some(0.0), Some(0.25))
    );
}

#[test]
fn bad_plate_values_are_refused_at_their_line() {
    let plates = "dir = \"plates/main\"\n";
    let cases = [
        ("overscan = 2.0", "bake.plates.overscan", "from 0 to 1"),
        ("scale = 8.0", "bake.plates.scale", "0.25 to 4"),
        (
            "anchors = [12.0, 8.0]",
            "bake.plates.anchors",
            "rising, distinct hours",
        ),
        (
            "layers = [\"color\", \"depth\", \"normal\", \"transfer\"]",
            "bake.plates.layers",
            "expected `sun` and `normal` with it",
        ),
        (
            "layers = [\"color\", \"normal\"]",
            "bake.plates.layers",
            "`color` and `depth`",
        ),
        (
            "layers = [\"color\", \"depth\", \"depth\"]",
            "bake.plates.layers",
            "named twice",
        ),
        (
            "layers = [\"color\", \"depth\", \"albedo\"]",
            "bake.plates.layers",
            "unknown layer `albedo`",
        ),
        ("threshold = 0.0", "bake.plates.threshold", "above 0"),
        ("growth = 1.0", "bake.plates.growth", "above 1"),
        (
            "min_samples = 1",
            "bake.plates.min_samples",
            "2 samples or more",
        ),
        (
            "max_samples = 8",
            "bake.plates.max_samples",
            "below `min_samples`",
        ),
        (
            "max_samples = 100000",
            "bake.plates.max_samples",
            "65536 samples at most",
        ),
        (
            "clamp_indirect = 20000",
            "bake.plates.clamp_indirect",
            "outside 0 to 10000",
        ),
        (
            "filter_glossy = -0.5",
            "bake.plates.filter_glossy",
            "outside 0 to 1",
        ),
    ];
    for (line, key, words) in cases {
        let needle = line.split(" =").next().unwrap();
        refused_at((plates, &format!("{plates}{line}\n")), key, needle, words);
    }
    refused_at(
        ("size = [1920, 1080]", "size = [8, 1080]"),
        "bake.plates.size",
        "size",
        "16 to 16384",
    );
}

#[test]
fn a_view_is_a_named_perspective_camera() {
    let view = |body: &str| {
        (
            "dir = \"plates/main\"\n",
            format!("dir = \"plates/main\"\n\n[[bake.plates.view]]\n{body}"),
        )
    };
    let cases = [
        (
            "name = \"a b\"",
            "bake.plates.view[a b].name",
            "name",
            "ASCII",
        ),
        (
            "name = \"v\"\ncamera = \"nope\"",
            "bake.plates.view[v].camera",
            "camera",
            "no camera named `nope`",
        ),
        (
            "name = \"v\"\nfov = 30.0\nfocal = 35.0",
            "bake.plates.view[v].fov",
            "fov",
            "both set",
        ),
        (
            "name = \"v\"\nfov = 180.0",
            "bake.plates.view[v].fov",
            "fov",
            "below 179",
        ),
        (
            "name = \"v\"\nnear = 1.0\nfar = 0.5",
            "bake.plates.view[v].far",
            "far",
            "above `near`",
        ),
        (
            "name = \"v\"\nsensor = 24.0",
            "bake.plates.view[v].sensor",
            "sensor",
            "needs `focal`",
        ),
    ];
    for (body, key, needle, words) in cases {
        let (from, to) = view(body);
        refused_at((from, &to), key, needle, words);
    }
    let (from, to) = view("name = \"v\"\n\n[[bake.plates.view]]\nname = \"v\"");
    let text = edited(&[(from, &to)]);
    let found = problems(&[(from, &to)]);
    let second = text.rfind("name = \"v\"").unwrap();
    let line = text[..second].lines().count() + 1;
    assert!(
        found.contains(&format!(
            "line {line}: bake.plates.view[v].name: duplicate name, expected a name no other view uses"
        )),
        "{found:?}"
    );
    let ortho = "[[camera]]\nname = \"main\"\nkind = \"orthographic\"\northo_height = 1.0\n";
    let found = problems(&[("[[camera]]\nname = \"main\"\n", ortho)]);
    assert!(
        found
            .iter()
            .any(|p| p.contains("bake.plates: a plate is perspective")),
        "{found:?}"
    );
}

#[test]
fn plates_need_the_scene_and_casters_need_the_folder() {
    let text = edited(&[("scene = \"t.scene.toml\"\n", "")]);
    let found: Vec<String> = validate(&parse(&text).unwrap())
        .iter()
        .map(Problem::to_string)
        .collect();
    let line = line_of(&text, "[bake.plates]");
    assert!(
        found.contains(&format!("line {line}: bake.plates: a plate traces the static part of the scene Manfred writes, expected `scene` in `[output]`")),
        "{found:?}"
    );
    refused_at(
        ("dir = \"plates/main\"\n", "casters = \"proxies\"\n"),
        "bake.plates.casters",
        "casters",
        "expected `dir` beside `casters`",
    );
    let text = edited(&[(
        "dir = \"plates/main\"\n",
        "dir = \"plates/main\"\ncasters = \"glass\"\n",
    )]);
    let line = line_of(&text, "casters");
    assert_eq!(
        parse(&text).unwrap_err().to_string(),
        format!(
            "line {line}: bake.plates.casters: expected \"meshes\", \"proxies\" or \"none\", found \"glass\""
        )
    );
}

#[test]
fn objects_say_whether_they_move_and_how_they_stand_in() {
    let r = recipe(&[
        (
            "size = [1.0, 0.1, 1.0]\n",
            "size = [1.0, 0.1, 1.0]\nproxy = \"hull\"\n",
        ),
        (
            "[[mover]]",
            "[[object]]\nname = \"cup\"\nkind = \"box\"\nsize = [0.1, 0.1, 0.1]\ndynamic = true\n\n[[object]]\nname = \"saucer\"\nkind = \"box\"\nparent = \"cup\"\nsize = [0.1, 0.1, 0.1]\n\n[[object]]\nname = \"note\"\nkind = \"text\"\non = \"cup\"\nspans = [{ text = \"a\", size = 0.1 }]\n\n[[object]]\nname = \"card\"\nkind = \"card\"\nsize = [0.1, 0.1]\nslot = \"s\"\nface_camera = true\nkeep_transform = true\n\n[[object]]\nname = \"still\"\nkind = \"card\"\nsize = [0.1, 0.1]\nslot = \"s\"\nface_camera = true\nkeep_transform = true\ndynamic = false\n\n[[mover]]",
        ),
    ]);
    assert_eq!(validate(&r), []);
    assert_eq!(r.objects[0].proxy, Some(Proxy::Hull));
    let dynamic: Vec<(&str, bool)> = (0..r.objects.len())
        .map(|i| (r.objects[i].name.as_str(), r.dynamic(i)))
        .collect();
    assert_eq!(
        dynamic,
        [
            ("top", false),
            ("mv_lid", true),
            ("cup", true),
            ("saucer", true),
            ("note", true),
            ("card", true),
            ("still", false)
        ]
    );
    refused_at(
        (
            "size = [0.1, 0.1, 0.1]\n",
            "size = [0.1, 0.1, 0.1]\nproxy = \"box\"\n",
        ),
        "object[mv_lid].proxy",
        "proxy",
        "this object is dynamic",
    );
    let edits = [
        (
            "size = [1.0, 0.1, 1.0]\n",
            "size = [1.0, 0.1, 1.0]\nproxy = \"hull\"\n",
        ),
        ("dir = \"plates/main\"\n", ""),
    ];
    let text = edited(&edits);
    let line = line_of(&text, "proxy");
    assert_eq!(
        problems(&edits),
        [format!(
            "line {line}: object[top].proxy: no proxies file is written without a plate folder, expected `dir` in `[bake.plates]`"
        )]
    );
}
