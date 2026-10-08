use std::process::Command;

const ALLOWED: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "0BSD",
    "ISC",
    "Zlib",
    "CC0-1.0",
    "Unlicense",
    "Unicode-3.0",
    "OFL-1.1",
];

const EXCEPTIONS: &[(&str, &str)] = &[
    ("clipboard-win", "BSL-1.0"),
    ("error-code", "BSL-1.0"),
    ("webpki-roots", "CDLA-Permissive-2.0"),
];

const UNDECLARED: &[&str] = &["pfx"];

fn private() -> Vec<&'static str> {
    include_str!("../Cargo.toml")
        .lines()
        .filter_map(|line| line.split_once("= [\"dep:"))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(name, _)| name)
        .collect()
}

fn words(expression: &str) -> Vec<String> {
    let spaced = expression
        .replace('/', " OR ")
        .replace('(', " ( ")
        .replace(')', " ) ");
    let mut words: Vec<String> = Vec::new();
    let mut iter = spaced.split_whitespace().peekable();
    while let Some(word) = iter.next() {
        if iter.peek() == Some(&"WITH") {
            iter.next();
            let exception = iter.next().unwrap_or_default();
            words.push(format!("{word} WITH {exception}"));
        } else {
            words.push(word.to_string());
        }
    }
    words
}

fn any_of(words: &[String], at: &mut usize, ok: &dyn Fn(&str) -> bool) -> Option<bool> {
    let mut met = all_of(words, at, ok)?;
    while words.get(*at).map(String::as_str) == Some("OR") {
        *at += 1;
        met |= all_of(words, at, ok)?;
    }
    Some(met)
}

fn all_of(words: &[String], at: &mut usize, ok: &dyn Fn(&str) -> bool) -> Option<bool> {
    let mut met = one(words, at, ok)?;
    while words.get(*at).map(String::as_str) == Some("AND") {
        *at += 1;
        met &= one(words, at, ok)?;
    }
    Some(met)
}

fn one(words: &[String], at: &mut usize, ok: &dyn Fn(&str) -> bool) -> Option<bool> {
    let word = words.get(*at)?;
    *at += 1;
    match word.as_str() {
        "(" => {
            let met = any_of(words, at, ok)?;
            (words.get(*at).map(String::as_str) == Some(")")).then_some(())?;
            *at += 1;
            Some(met)
        }
        ")" | "OR" | "AND" => None,
        id => Some(ok(id)),
    }
}

fn allowed(expression: &str, ok: &dyn Fn(&str) -> bool) -> bool {
    let words = words(expression);
    let mut at = 0;
    any_of(&words, &mut at, ok) == Some(true) && at == words.len()
}

fn verdict(name: &str, licence: Option<&str>) -> Result<(), String> {
    let Some(licence) = licence else {
        return if UNDECLARED.iter().any(|own| name.starts_with(own)) || private().contains(&name) {
            Ok(())
        } else {
            Err(format!("{name}: no licence declared"))
        };
    };
    let ok = |id: &str| {
        ALLOWED.contains(&id)
            || EXCEPTIONS
                .iter()
                .any(|(crate_name, exception)| *crate_name == name && *exception == id)
    };
    if allowed(licence, &ok) {
        Ok(())
    } else {
        Err(format!("{name}: {licence} is not on the allow-list"))
    }
}

#[test]
fn spdx_expressions_are_read_with_or_and_and_and_parentheses() {
    let ok = |id: &str| ALLOWED.contains(&id);
    for good in [
        "MIT",
        "MIT OR Apache-2.0",
        "MIT/Apache-2.0",
        "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        "Apache-2.0 OR GPL-2.0-only",
        "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
    ] {
        assert!(allowed(good, &ok), "{good}");
    }
    for bad in [
        "GPL-3.0-only",
        "MIT AND GPL-2.0-only",
        "(MIT OR Apache-2.0) AND GPL-2.0-only",
        "BSL-1.0",
        "",
        "MIT OR",
        "(MIT",
        "MIT)",
    ] {
        assert!(!allowed(bad, &ok), "{bad}");
    }
    assert_eq!(verdict("error-code", Some("BSL-1.0")), Ok(()));
    assert!(verdict("other", Some("BSL-1.0")).is_err());
    assert!(verdict("stranger", None).is_err());
}

fn host() -> String {
    let output = Command::new("rustc").arg("-vV").output().unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap()
        .to_string()
}

#[test]
fn every_dependency_is_under_an_mit_compatible_licence() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut command = Command::new(cargo);
    command.args([
        "metadata",
        "--offline",
        "--locked",
        "--format-version",
        "1",
        "--filter-platform",
        &host(),
        "--manifest-path",
        concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
    ]);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    assert!(packages.len() > 100);
    let refused: Vec<String> = packages
        .iter()
        .filter_map(|package| {
            let name = package["name"].as_str().unwrap_or_default();
            verdict(name, package["license"].as_str()).err()
        })
        .collect();
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}
