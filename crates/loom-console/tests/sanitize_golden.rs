//! Golden corpus: `tests/golden/sanitize/*.in` → `*.out` (UART plan §5.4).

use std::fs;
use std::path::{Path, PathBuf};

use loom_console::sanitize::{SanitizeConfig, Sanitizer};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sanitize")
}

fn config_for(name: &str) -> SanitizeConfig {
    if name.starts_with("tui24_") {
        SanitizeConfig::tui(24)
    } else {
        assert!(name.starts_with("web_"), "unknown golden prefix: {name}");
        SanitizeConfig::web()
    }
}

fn run(cfg: SanitizeConfig, input: &[u8], chunk: usize) -> Vec<u8> {
    let mut s = Sanitizer::new(cfg);
    let mut out = Vec::new();
    for part in input.chunks(chunk.max(1)) {
        s.feed(part, &mut out);
    }
    out
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

#[test]
fn golden_corpus_matches_whole_and_split() {
    let mut cases = 0;
    let mut entries: Vec<_> = fs::read_dir(corpus_dir())
        .expect("golden dir exists")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "in"))
        .collect();
    entries.sort();
    for input_path in entries {
        let name = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 file name")
            .to_owned();
        let input = fs::read(&input_path).expect("read .in");
        let expected = fs::read(input_path.with_extension("out")).expect("read .out");
        let cfg = config_for(&name);
        for chunk in [usize::MAX, 1, 2, 3, 7, 4096] {
            let got = run(cfg, &input, chunk);
            assert_eq!(
                got,
                expected,
                "{name} (chunk {chunk}): got {} want {}",
                show(&got),
                show(&expected)
            );
        }
        cases += 1;
    }
    assert!(cases >= 30, "golden corpus too small: {cases}");
}
