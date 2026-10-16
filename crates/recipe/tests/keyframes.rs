use std::path::Path;

use manfred_recipe::{Interp, KeyEdit, Loaded, apply_all, key_patches, parse, reread, write};

const RECIPE: &str = r#"manfred = 1
name = "street"

# a short shot
[timeline]
fps = 10
seconds = 2.0

# the hero car
[[object]]
name = "hero"
kind = "box"
size = [1.0, 1.0, 1.0]
at = [0.0, 0.5, 0.0] # rest on the road

[[light]]
name = "lamp"
position = [0.0, 3.0, 0.0]
intensity = 2.0 # a dim start
"#;

fn edited(l: &mut Loaded, path: &str, edit: KeyEdit) {
    let patches = key_patches(l, path, &edit).unwrap_or_else(|e| panic!("{path} {edit:?}: {e}"));
    apply_all(l, &patches).unwrap_or_else(|e| panic!("{path} {edit:?}: {e}"));
}

#[test]
fn key_and_unkey_round_trip_through_the_file_and_keep_its_comments() {
    let file = Path::new("street.toml");
    let mut l = reread(file, RECIPE).unwrap();
    let at = "object.hero.at";
    let set = |t: f64, value: [f64; 3]| KeyEdit::Set {
        t,
        value: value.to_vec(),
    };
    edited(&mut l, at, set(0.0, [0.0, 0.5, 0.0]));
    edited(&mut l, at, set(2.0, [0.0, 0.5, 12.0]));
    edited(&mut l, at, set(1.0, [1.0, 0.5, 6.0]));
    edited(&mut l, at, set(2.0, [0.0, 0.5, 14.0]));
    edited(&mut l, at, KeyEdit::Interp(Interp::Smooth));
    edited(
        &mut l,
        "light.lamp.intensity",
        KeyEdit::Set {
            t: 0.5,
            value: vec![8.0],
        },
    );
    let channel = l.recipe.channel(at).unwrap();
    let times: Vec<f64> = channel.keys.iter().map(|k| k.0).collect();
    assert_eq!(times, [0.0, 1.0, 2.0]);
    assert_eq!(channel.keys[2].1, [0.0, 0.5, 14.0]);
    assert_eq!(channel.interp, Interp::Smooth);
    let text = write(&l);
    for comment in [
        "# a short shot",
        "# the hero car",
        "# rest on the road",
        "# a dim start",
    ] {
        assert!(text.contains(comment), "{text}");
    }
    assert!(text.contains("interp = { at = \"smooth\" }"), "{text}");
    assert_eq!(reread(file, &text).unwrap().recipe, l.recipe);

    edited(&mut l, at, KeyEdit::Remove { t: 1.0 });
    assert_eq!(l.recipe.channel(at).unwrap().keys.len(), 2);
    edited(&mut l, at, KeyEdit::RemoveAll);
    edited(&mut l, "light.lamp.intensity", KeyEdit::Remove { t: 0.5 });
    assert!(l.recipe.keyed.is_empty());
    let text = write(&l);
    assert!(!text.contains("keys") && !text.contains("interp"), "{text}");
    assert_eq!(text, RECIPE);

    let refused = [
        (
            "object.hero.at",
            KeyEdit::Set {
                t: 1.0,
                value: vec![1.0],
            },
        ),
        (
            "object.nope.at",
            KeyEdit::Set {
                t: 1.0,
                value: vec![0.0; 3],
            },
        ),
        (
            "light.lamp.cone",
            KeyEdit::Set {
                t: 1.0,
                value: vec![10.0, 20.0],
            },
        ),
        (
            "light.lamp.intensity",
            KeyEdit::Set {
                t: 1.0,
                value: vec![-1.0],
            },
        ),
        ("light.lamp.intensity", KeyEdit::Remove { t: 1.0 }),
        ("object.hero.colour", KeyEdit::RemoveAll),
    ];
    for (path, edit) in refused {
        assert!(key_patches(&l, path, &edit).is_err(), "{path} {edit:?}");
    }
}

#[test]
fn keys_blend_linear_smooth_or_step_and_hold_past_the_ends() {
    let r = parse(&format!(
        "{RECIPE}keys = {{ intensity = [[0.5, 2.0], [1.5, 6.0], [2.5, 2.0]] }}\n"
    ))
    .unwrap();
    let channel = r.channel("light.lamp.intensity").unwrap().clone();
    assert_eq!(channel.interp, Interp::Linear);
    let mut smooth = channel.clone();
    smooth.interp = Interp::Smooth;
    let mut step = channel.clone();
    step.interp = Interp::Step;
    for c in [&channel, &smooth, &step] {
        assert_eq!(c.number(0.0), 2.0);
        assert_eq!(c.number(9.0), 2.0);
        assert_eq!(c.number(1.5), 6.0);
    }
    assert_eq!(channel.number(1.0), 4.0);
    assert_eq!(step.number(1.0), 2.0);
    assert_eq!(step.number(1.49), 2.0);
    assert!(smooth.number(0.75) < channel.number(0.75));
    assert!(smooth.number(1.25) > channel.number(1.25));

    for interp in ["linear", "smooth", "step"] {
        let text = format!(
            "{RECIPE}keys = {{ intensity = [[0.0, 1.0]] }}\ninterp = {{ intensity = \"{interp}\" }}\n"
        );
        assert_eq!(
            parse(&text)
                .unwrap()
                .channel("light.lamp.intensity")
                .unwrap()
                .interp
                .name(),
            interp
        );
    }
    for (body, wanted) in [
        (
            "keys = { intensity = [[1.0, 2.0], [0.5, 3.0]] }\n",
            "rise strictly",
        ),
        ("keys = { intensity = [[0.0, [1.0, 2.0]]] }\n", "a number"),
        ("keys = { cone = [[0.0, [10.0, 20.0]]] }\n", "spot light"),
        ("keys = { intensity = [] }\n", "at least one"),
        (
            "keys = { intensity = [[0.0, 1.0]] }\ninterp = { intensity = \"ease\" }\n",
            "\"step\"",
        ),
        ("interp = { intensity = \"step\" }\n", "a `keys` table"),
    ] {
        let error = parse(&format!("{RECIPE}{body}")).unwrap_err().to_string();
        assert!(error.contains(wanted), "{body}: {error}");
    }
    let error = parse(&format!(
        "{}keys = {{ intensity = [[0.0, 1.0]] }}\n",
        RECIPE.replace("[timeline]\nfps = 10\nseconds = 2.0\n", "")
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("[timeline]"), "{error}");
}
