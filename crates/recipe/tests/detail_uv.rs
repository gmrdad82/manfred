use manfred_recipe::{DetailUv, parse, validate};

fn text(kind: &str, extra: &str) -> String {
    format!("manfred = 1\nname = \"t\"\n[[object]]\nname = \"a\"\n{kind}material = \"m\"\n{extra}")
}

const BOX: &str = "kind = \"box\"\nsize = [1.0, 1.0, 1.0]\n";

fn problems(kind: &str, extra: &str) -> Vec<String> {
    validate(&parse(&text(kind, extra)).unwrap())
        .iter()
        .map(|p| format!("{}: {}", p.path, p.text))
        .collect()
}

#[test]
fn detail_uv_reads_its_texels_per_metre_and_a_gutter_of_four() {
    let r = parse(&text(BOX, "detail_uv = { texels_per_m = 1700 }\n")).unwrap();
    assert_eq!(
        r.objects[0].detail_uv,
        Some(DetailUv {
            texels_per_m: 1700.0,
            gutter: 16,
        })
    );
    let r = parse(&text(
        BOX,
        "detail_uv = { texels_per_m = 512.5, gutter = 8 }\n",
    ))
    .unwrap();
    assert_eq!(
        r.objects[0].detail_uv,
        Some(DetailUv {
            texels_per_m: 512.5,
            gutter: 8,
        })
    );
    assert_eq!(validate(&r), []);
    let none = parse(&text(BOX, "")).unwrap();
    assert_eq!(none.objects[0].detail_uv, None);
}

#[test]
fn detail_uv_needs_its_texels_and_refuses_unknown_keys() {
    let missing = parse(&text(BOX, "detail_uv = { gutter = 2 }\n"))
        .unwrap_err()
        .to_string();
    assert!(
        missing.contains("object[a].detail_uv.texels_per_m"),
        "{missing}"
    );
    let unknown = parse(&text(
        BOX,
        "detail_uv = { texels_per_m = 10, angle = 30 }\n",
    ))
    .unwrap_err()
    .to_string();
    assert!(unknown.contains("unknown key `angle`"), "{unknown}");
}

#[test]
fn detail_uv_problems_name_the_key() {
    assert_eq!(
        problems(BOX, "detail_uv = { texels_per_m = 0 }\n"),
        ["object[a].detail_uv.texels_per_m: expected a number above 0"]
    );
    assert_eq!(
        problems(BOX, "detail_uv = { texels_per_m = 100, gutter = 65 }\n"),
        ["object[a].detail_uv.gutter: expected at most 64 texels"]
    );
    let none = problems(BOX, "uv = \"none\"\ndetail_uv = { texels_per_m = 100 }\n");
    assert_eq!(none.len(), 1);
    assert!(none[0].contains("TEXCOORD_1 beside TEXCOORD_0"), "{none:?}");
    let text_kind = problems(
        "kind = \"text\"\nspans = [{ text = \"hi\", size = 0.01 }]\n",
        "detail_uv = { texels_per_m = 100 }\n",
    );
    assert!(
        text_kind
            .iter()
            .any(|p| p.starts_with("object[a].detail_uv: text carries no mesh")),
        "{text_kind:?}"
    );
}
