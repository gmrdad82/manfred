use manfred_recipe::{ANCHORS, Kind, TextSpace, anchor_point, default_anchor, parse, validate};

const HEAD: &str = "manfred = 1\nname = \"words\"\n\n[[object]]\nname = \"slab\"\nkind = \"box\"\nsize = [0.4, 0.2, 0.1]\n\n";

fn problems(text_object: &str) -> Vec<(String, String)> {
    let recipe = parse(&format!("{HEAD}{text_object}")).unwrap();
    validate(&recipe)
        .into_iter()
        .map(|p| (p.path, p.text))
        .collect()
}

#[test]
fn a_text_reads_its_spans_with_their_defaults() {
    let recipe = parse(&format!(
        "{HEAD}[[object]]\nname = \"label\"\nkind = \"text\"\non = \"slab\"\nspace = \"lit\"\nwrap = 0.2\nanchor = \"top\"\nspans = [{{ text = \"a\", size = 0.02 }}, {{ text = \"b\", size = 0.03, family = \"fonts/serif.ttf\", weight = 700, italic = true, color = [0.5, 0.25, 0.0] }}]\n"
    ))
    .unwrap();
    let Kind::Text {
        spans,
        wrap,
        anchor,
        space,
        on,
    } = &recipe.objects[1].kind
    else {
        panic!("the label is text")
    };
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].color, [1.0, 1.0, 1.0]);
    assert_eq!(spans[0].family, None);
    assert_eq!(spans[0].weight, None);
    assert!(!spans[0].italic);
    assert_eq!(spans[1].family.as_deref(), Some("fonts/serif.ttf"));
    assert_eq!(spans[1].weight, Some(700));
    assert!(spans[1].italic);
    assert_eq!(*wrap, Some(0.2));
    assert_eq!(anchor.as_deref(), Some("top"));
    assert_eq!(*space, TextSpace::Lit);
    assert_eq!(on.as_deref(), Some("slab"));
    assert!(validate(&recipe).is_empty());
}

#[test]
fn the_nine_anchors_cover_the_box() {
    assert_eq!(ANCHORS.len(), 9);
    assert_eq!(anchor_point("top_left"), Some([0.0, 0.0]));
    assert_eq!(anchor_point("centre"), Some([0.5, 0.5]));
    assert_eq!(anchor_point("bottom_right"), Some([1.0, 1.0]));
    assert_eq!(anchor_point("center"), None);
    assert_eq!(default_anchor(TextSpace::Surface), "centre");
    assert_eq!(default_anchor(TextSpace::Lit), "centre");
    assert_eq!(default_anchor(TextSpace::Overlay), "top_left");
}

#[test]
fn bad_text_keys_are_problems_with_their_paths() {
    let found = problems(
        "[[object]]\nname = \"label\"\nkind = \"text\"\nwrap = 0.0\nanchor = \"middle\"\nspans = [{ text = \"a\", size = 0.0, weight = 0, color = [-1.0, 0.0, 0.0], family = \"\" }]\n",
    );
    let paths: Vec<&str> = found.iter().map(|(path, _)| path.as_str()).collect();
    for wanted in [
        "object[label].spans[0].size",
        "object[label].spans[0].weight",
        "object[label].spans[0].color",
        "object[label].spans[0].family",
        "object[label].wrap",
        "object[label].anchor",
    ] {
        assert!(paths.contains(&wanted), "{wanted} in {found:?}");
    }
    let anchor = found
        .iter()
        .find(|(path, _)| path == "object[label].anchor")
        .unwrap();
    assert!(anchor.1.contains("\"centre\""), "{}", anchor.1);
    assert!(anchor.1.ends_with("found \"middle\""), "{}", anchor.1);
}

#[test]
fn text_on_an_object_takes_no_parent() {
    let found = problems(
        "[[object]]\nname = \"label\"\nkind = \"text\"\non = \"slab\"\nparent = \"slab\"\nspans = [{ text = \"a\", size = 0.02 }]\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "object[label].parent");
    let fine = problems(
        "[[object]]\nname = \"label\"\nkind = \"text\"\nparent = \"slab\"\nspans = [{ text = \"a\", size = 0.02 }]\n",
    );
    assert!(fine.is_empty(), "{fine:?}");
}
