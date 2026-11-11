use loom_test_format::*;
use serde_json::{Value, json};
#[derive(Debug)]
struct Registry;
impl RiskProvider for Registry {
    fn classify(&self, method: &str, args: &Value) -> Risk {
        match method {
            "psu.out" if args["state"] == "off" => Risk::new(RiskLevel::R1, true),
            "psu.out" | "psu.cycle" => Risk::new(RiskLevel::R3, false),
            "uds.session" if args["mode"] == "default" => Risk::new(RiskLevel::R1, true),
            "uds.security" => Risk::new(RiskLevel::R3, false),
            "uds.write-did" | "uds.dtc.clear" | "uds.reset" => Risk::new(RiskLevel::R2, false),
            "uart.send" | "dlt.level" | "uds.session" => Risk::new(RiskLevel::R1, false),
            "forbidden.write" => Risk {
                level: RiskLevel::R4,
                safe_state: false,
                default_blocked: true,
                immutable_deny: true,
            },
            _ => Risk::new(RiskLevel::R0, false),
        }
    }
}
impl CommandTable for Registry {
    fn lookup(&self, method: &str) -> Option<CommandSpec> {
        if !matches!(
            method,
            "psu.out"
                | "psu.cycle"
                | "uart.send"
                | "uart.expect"
                | "can.sig.wait-for"
                | "uds.dtc.list"
                | "uds.dtc.clear"
                | "dlt.level"
                | "la.capture"
                | "la.wait"
                | "dlt.wait-for"
                | "uds.session"
                | "uds.security"
                | "uds.read-did"
                | "uds.write-did"
                | "uds.reset"
                | "forbidden.write"
        ) {
            return None;
        }
        Some(CommandSpec {
            waiting: matches!(
                method,
                "uart.expect" | "can.sig.wait-for" | "dlt.wait-for" | "la.wait"
            ),
        })
    }
    fn validate_args(&self, _: &str, args: &Value) -> Result<(), String> {
        if !args.is_object() {
            Err("object required".into())
        } else {
            Ok(())
        }
    }
}
const TC1: &str = include_str!("fixtures/TC-1.tc.md");
const PLAN1: &str = include_str!("fixtures/TC-1.plan.yaml");
#[test]
fn three_tc_fixtures_parse_check_and_bind_original_hashes() {
    for (tc, plan, steps) in [
        (TC1, PLAN1, 5),
        (
            include_str!("fixtures/TC-2.tc.md"),
            include_str!("fixtures/TC-2.plan.yaml"),
            4,
        ),
        (
            include_str!("fixtures/TC-3.tc.md"),
            include_str!("fixtures/TC-3.plan.yaml"),
            1,
        ),
    ] {
        let doc = parse_tc(tc).expect("TC fixture");
        let p = parse_plan_with_tc(plan, &doc).expect("plan fixture");
        let r = check_plan(&p, &Registry);
        assert!(r.is_valid(), "{:?}", r.errors);
        assert_eq!(p.steps().len(), steps);
        assert!(r.tc_hash_verified);
        assert_eq!(r.tc_source_sha256, Some(sha256(tc.as_bytes())));
        assert_eq!(p.plan_sha256().len(), 64);
        for step in &doc.steps {
            assert_eq!(&tc[step.cell_spans[1].start..step.cell_spans[1].end], step.action);
            assert_eq!(
                tc[..step.span.start].bytes().filter(|b| *b == b'\n').count() + 1,
                step.source_line
            );
        }
    }
}
#[test]
fn korean_english_aliases_raw_markdown_and_body_survive() {
    for columns in [
        "# | 동작 | 기대 결과 | 합격 기준",
        "번호 | Action | Expected | Pass criteria",
        "Step | Action | Expected | Pass criteria",
    ] {
        let source = format!(
            "## 설명\n원문 **강조**.\n\n## 단계\n| {columns} |\n|---|---|---|---|\n| 1 | `a\\|b` **원문** | [결과](https://example.test) | ≤ 3 s |\n"
        );
        let d = parse_tc(&source).expect("table");
        assert_eq!(d.description, source);
        assert_eq!(d.steps.len(), 1);
        assert_eq!(d.steps[0].action, "`a\\|b` **원문**");
        assert_eq!(d.steps[0].expected, "[결과](https://example.test)");
        assert_eq!(d.steps[0].source_line, 7);
    }
}
#[test]
fn no_frontmatter_no_table_empty_and_optional_metadata_are_valid() {
    for source in [
        "",
        "자연어만으로 유효하다.\n",
        "---\n---\n## 설명\n본문\n",
        "---\ntitle: 제목\ntags: [한글]\n---\n본문",
    ] {
        let d = parse_tc(source).expect("freeform");
        assert!(d.steps.is_empty());
        assert_eq!(d.source_sha256, sha256(source.as_bytes()));
    }
    assert!(parse_tc("---\nloom_tc: 2\n---\n").is_err());
    assert!(parse_tc("---\ntags: string\n---\n").is_err());
    assert!(parse_tc("---\ntitle: open\n").is_err());
}
#[test]
fn malformed_fixtures_have_locations_and_semantic_paths() {
    let cycle = parse_plan(include_str!("fixtures/bad-cycle.plan.yaml")).expect("shape valid");
    let r = check_plan(&cycle, &Registry);
    let e = r.errors.iter().find(|e| e.code == "cycle").expect("cycle");
    assert_eq!(e.location.line, 3);
    let wait = parse_plan(include_str!("fixtures/bad-timeout.plan.yaml")).expect("shape valid");
    let r = check_plan(&wait, &Registry);
    let e = r
        .errors
        .iter()
        .find(|e| e.code == "missing_timeout")
        .expect("timeout");
    assert_eq!(e.location.line, 3);
    assert!(e.path.starts_with("/steps/0"));
    let e = parse_plan(include_str!("fixtures/bad-501.plan.yaml")).expect_err("step bound");
    assert_eq!(e.code, "step_limit");
    assert_eq!(e.location.line, 1003);
    let e = parse_tc(include_str!("fixtures/bad-columns.tc.md")).expect_err("column missing");
    assert_eq!(e.code, "table_columns");
    assert_eq!(e.location.line, 2);
}
#[test]
fn canonical_hash_order_sensitivity_claimed_summary_and_source_binding() {
    assert_eq!(
        canonical_json(&json!({"z":2,"a":{"β":1,"x":3}})).expect("canonical"),
        b"{\"a\":{\"x\":3,\"\xce\xb2\":1},\"z\":2}"
    );
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let a = parse_plan("loom_tc_plan: 1\nsteps: [{id: S1, psu.out: {state: on}}]\n").expect("a");
    let b =
        parse_plan("steps: [{psu.out: {state: on}, id: S1}]\nloom_tc_plan: 1\nrisk_summary: {risk: R0}\n")
            .expect("b");
    assert_eq!(a.plan_sha256(), b.plan_sha256());
    assert_ne!(a.source_sha256(), b.source_sha256());
    let report = check_plan(&b, &Registry);
    assert!(report.warnings.iter().any(|e| e.code == "ignored_risk_summary"));
    assert_eq!(report.maximum_risk, RiskLevel::R3);
    let c = parse_plan("loom_tc_plan: 1\nsteps: [{id: S1, psu.out: {state: off}}]").expect("changed");
    assert_ne!(a.plan_sha256(), c.plan_sha256());
    let with_hash = format!(
        "loom_tc_plan: 1\nsteps: [{{id: S1, psu.out: {{state: on}}}}]\nplan_hash: sha256:{}\n",
        a.plan_sha256()
    );
    assert_eq!(
        parse_plan(&with_hash).expect("hash verified").plan_sha256(),
        a.plan_sha256()
    );
    assert!(parse_plan(&with_hash.replace(a.plan_sha256(), &"0".repeat(64))).is_err());
    let tc = parse_tc(&TC1.replace("コールド", "changed").replace("콜드", "변경된")).expect("changed TC");
    assert_eq!(
        parse_plan_with_tc(PLAN1, &tc)
            .expect_err("stale source hash")
            .code,
        "tc_hash"
    );
}
#[test]
fn risk_counts_include_both_branches_finally_repeat_and_additional_retries() {
    let source = "loom_tc_plan: 1\nrisk_summary: [invented]\nsteps:\n  - id: A\n    psu.out: {state: on}\n    repeat: {count: 100}\n    retry: {count: 3}\n    on_fail: {goto: B}\n  - id: B\n    when: {step: A, verdict: fail}\n    uds.dtc.clear: {target: bcm}\nfinally: [{psu.out: {state: off}}]\n";
    let p = parse_plan(source).expect("plan");
    let r = check_plan(&p, &Registry);
    assert!(r.is_valid(), "{:?}", r.errors);
    assert_eq!(r.risks[0].max_uses, 400);
    assert!(r.risks.iter().any(|r| r.method == "uds.dtc.clear"));
    assert!(r.risks.iter().any(|r| r.finally && r.max_uses == 1));
    assert_eq!(r.maximum_risk, RiskLevel::R3);
    let p = parse_plan(PLAN1).expect("TC1");
    let r = check_plan(&p, &Registry);
    assert_eq!(
        r.risks
            .iter()
            .filter(|r| r.method == "psu.out" && r.args["state"] == "off")
            .map(|r| r.max_uses)
            .sum::<u64>(),
        2
    );
    let bad = source.replace(
        "finally: [{psu.out: {state: off}}]",
        "finally: [{psu.out: {state: on}}]",
    );
    assert!(
        check_plan(&parse_plan(&bad).expect("shape"), &Registry)
            .errors
            .iter()
            .any(|e| e.code == "unsafe_finally")
    );
}
#[test]
fn registry_finally_blocking_and_reference_checks() {
    for (step, code) in [
        ("{id: A, missing.rpc: {}}", "unknown_command"),
        ("{id: A, forbidden.write: {}}", "blocked_command"),
        ("{id: A, psu.out: {}, on_fail: {goto: missing}}", "goto_target"),
        (
            "{id: A, psu.out: {}, when: {step: missing, verdict: fail}}",
            "when_target",
        ),
        ("{id: A, manual: {prompt: check}}", "missing_timeout"),
    ] {
        let p = parse_plan(&format!("loom_tc_plan: 1\nsteps: [{step}]\n")).expect("shape");
        assert!(
            check_plan(&p, &Registry).errors.iter().any(|e| e.code == code),
            "{code}"
        );
    }
    let p =
        parse_plan("loom_tc_plan: 1\nsteps: [{id: A, psu.out: {}}, {id: A, psu.out: {}}]").expect("shape");
    assert!(
        check_plan(&p, &Registry)
            .errors
            .iter()
            .any(|e| e.code == "duplicate_id")
    );
    let p = parse_plan("loom_tc_plan: 1\nsteps: []\nfinally: [{uart.send: {text: x}}]").expect("shape");
    assert!(
        check_plan(&p, &Registry)
            .errors
            .iter()
            .any(|e| e.code == "unsafe_finally")
    );
    let p = parse_plan("loom_tc_plan: 1\nsteps: [{id: A, manual: {prompt: check, timeout: 10s}}]")
        .expect("manual");
    assert!(check_plan(&p, &Registry).is_valid());
}
#[test]
fn scenario_subset_inherits_target_and_preserves_legacy_expectations() {
    let p = parse_plan(include_str!("fixtures/uds-runner.scenario.yaml")).expect("UDS scenario");
    assert_eq!(p.format(), PlanFormat::Scenario);
    assert_eq!(p.steps().len(), 7);
    assert_eq!(p.steps()[0].id, "S1");
    assert_eq!(p.steps()[2].save.as_deref(), Some("before"));
    assert_eq!(p.steps()[3].actions[0].args["fields"]["region"], "${region}");
    assert_eq!(p.steps()[0].actions[0].args["target"], "bcm");
    let r = check_plan(&p, &Registry);
    assert!(r.is_valid(), "{:?}", r.errors);
    assert!(r.requires_legacy_adapter);
    assert_eq!(p.steps()[4].expect[0].pred, json!({"0x0101.region":"${region}"}));
    assert_eq!(p.steps()[6].actions[0].delay_ms, Some(3000));
}
#[test]
fn schema_types_limits_unknown_fields_and_timeouts_reject_instead_of_false() {
    for step in [
        "{id: A, psu.out: 3}",
        "{id: A, psu.out: {}, retry: {count: 4}}",
        "{id: A, psu.out: {}, repeat: {count: 101}}",
        "{id: A, psu.out: {}, retry: {count: -1}}",
        "{id: A, psu.out: {}, repeat: {count: 0}}",
        "{id: A, psu.out: {}, unknown: true}",
        "{id: A, psu.out: {}, do: [{psu.out: {}}]}",
        "{id: A, uart.expect: {timeout: 0s}}",
        "{id: A, uart.expect: {timeout_ms: 1, timeout: 2s}}",
    ] {
        let text = format!("loom_tc_plan: 1\nsteps: [{step}]\n");
        match parse_plan(&text) {
            Err(e) => assert!(e.location.line > 0),
            Ok(p) => assert!(!check_plan(&p, &Registry).is_valid(), "{step}"),
        }
    }
    for source in [
        "loom_tc_plan: 2\nsteps: []",
        "loom_tc_plan: 1\nloom_scenario: 1\nsteps: []",
        "loom_tc_plan: 1\nsteps: []\nsteps: []",
        "loom_tc_plan: 1\nsteps: []\nvalue: .nan",
        "loom_tc_plan: 1\nsteps: []\nvalue: !tag hi",
        "loom_tc_plan: 1\nsteps: []\nvalue: {3: x}",
    ] {
        assert!(parse_plan(source).is_err(), "{source}");
    }
}
#[test]
fn ast_cell_and_byte_bounds_and_yaml_syntax_marks() {
    let source = format!(
        "## 단계\n| # | 동작 | 기대 결과 | 합격 기준 |\n|---|---|---|---|\n| 1 | {} | x | y |\n",
        "x".repeat(MAX_CELL_BYTES + 1)
    );
    let e = parse_tc(&source).expect_err("cell bound");
    assert_eq!(e.code, "cell_limit");
    assert_eq!(e.location.line, 4);
    let big = "x".repeat(MAX_FILE_BYTES + 1);
    assert_eq!(parse_tc(&big).expect_err("TC bound").code, "file_limit");
    assert_eq!(parse_plan(&big).expect_err("plan bound").code, "file_limit");
    let deep = format!(
        "loom_tc_plan: 1\nsteps: []\nextra: {}0{}",
        "[".repeat(MAX_DEPTH + 1),
        "]".repeat(MAX_DEPTH + 1)
    );
    assert!(parse_plan(&deep).is_err());
    let e = parse_plan("loom_tc_plan: 1\nsteps:\n  - id: [unterminated\n").expect_err("syntax");
    assert!(e.location.line >= 3);
    assert!(parse_plan_bytes(&[255, 0]).is_err());
    assert!(parse_tc_bytes(&[255]).is_err());
}
#[test]
fn random_inputs_100000_each_parser_no_panics() {
    let mut seed = 0x8a5cd789635d2dffu64;
    let mut bytes = Vec::with_capacity(128);
    for n in 0..100_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        bytes.clear();
        let len = (seed as usize) % 128;
        for _ in 0..len {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            bytes.push((seed >> 32) as u8);
        }
        let _ = parse_tc_bytes(&bytes);
        let _ = parse_plan_bytes(&bytes);
        // Valid UTF-8 plus structured malformed forms exercise parsers, not just UTF-8 rejection.
        let text = match n % 5 {
            0 => format!("loom_tc_plan: 1\nsteps: [{}]", String::from_utf8_lossy(&bytes)),
            1 => format!(
                "## 단계\n| # | 동작 | 기대 결과 | 합격 기준 |\n|---|---|---|---|\n| 1 | {} | x | y |",
                String::from_utf8_lossy(&bytes)
            ),
            _ => String::from_utf8_lossy(&bytes).into_owned(),
        };
        let _ = parse_tc(&text);
        let _ = parse_plan(&text);
    }
}

#[test]
fn yaml_preflight_bounds_backend_events_alias_expansion_and_multiple_documents() {
    let deep = format!(
        "loom_tc_plan: 1\nsteps: []\nextra: {}0{}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let e = parse_plan(&deep).expect_err("reject before backend lexical event allocation");
    assert_eq!(e.code, "yaml_depth_limit");
    assert_eq!(e.location.line, 3);
    let many = format!(
        "loom_tc_plan: 1\nsteps: []\nextra: [{}]",
        vec!["0"; MAX_NODES].join(",")
    );
    assert!(parse_plan(&many).is_err());
    let aliases = "loom_tc_plan: 1\nsteps: []\na: &a [x,x,x,x,x,x,x,x,x,x]\nb: &b [*a,*a,*a,*a,*a,*a,*a,*a,*a,*a]\nc: &c [*b,*b,*b,*b,*b,*b,*b,*b,*b,*b]\nd: &d [*c,*c,*c,*c,*c,*c,*c,*c,*c,*c]\ne: [*d,*d,*d,*d,*d,*d,*d,*d,*d,*d]\n";
    assert!(parse_plan(aliases).is_err());
    assert!(parse_plan("loom_tc_plan: 1\nsteps: []\n---\nloom_tc_plan: 1\nsteps: []").is_err());
    assert!(parse_plan("loom_tc_plan: 1\nsteps: []\na: &a [*a]").is_err());
    let p=parse_plan("loom_tc_plan: 1\nsteps:\n - id: A\n   uart.send:\n     text: |\n       literal [ is not syntax\n       ## comment inside block\n").expect("literal scalar");
    assert!(
        p.steps()[0].actions[0].args["text"]
            .as_str()
            .expect("text")
            .contains('[')
    );
}

#[test]
fn long_alias_scalar_copies_have_an_expanded_ast_byte_ceiling() {
    let text = format!(
        "loom_tc_plan: 1\nsteps: []\nscalar: &a {}\nexpanded: [{}]\n",
        "x".repeat(16384),
        vec!["*a"; 1000].join(",")
    );
    let e =
        parse_plan(&text).expect_err("alias string expansion must be bounded before retaining all copies");
    assert!(e.message.contains("byte limit"), "{e}");
}

#[test]
fn quoted_root_step_locations_legacy_on_error_and_bounded_canonical_output() {
    let p = parse_plan("loom_tc_plan: 1\n\"steps\" :\n  -\n    id: A\n    uart.expect: {}\n")
        .expect("quoted root key");
    let report = check_plan(&p, &Registry);
    assert_eq!(
        report
            .errors
            .iter()
            .find(|e| e.code == "missing_timeout")
            .expect("timeout")
            .location
            .line,
        3
    );
    let p = parse_plan("loom_scenario: 1\non_error: continue\nsteps: [{uds.read-did: {dids: [1]}}]")
        .expect("legacy continue");
    assert_eq!(p.steps()[0].on_fail, FailureAction::Continue);
    assert!(parse_plan("loom_scenario: 1\non_error: forever\nsteps: []").is_err());
    let escaped = Value::Array(
        (0..4)
            .map(|_| Value::String("\0".repeat(MAX_SCALAR_BYTES)))
            .collect(),
    );
    let e = canonical_json(&escaped).expect_err("stop escaped output at 1 MiB");
    assert_eq!(e.code, "canonical_limit");
}
