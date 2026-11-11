use cel::{Context, Env, Program, Value as CelValue};
use serde_json::{json, Value};
use serde_json_path::JsonPath;
use std::{hint::black_box, sync::Arc, time::Instant};

type EvalResult<T> = Result<T, String>;

struct Predicates {
    env: Arc<Env>,
    exists: Program,
    hold: Program,
    exists_path: JsonPath,
    hold_path: JsonPath,
}

impl Predicates {
    fn new() -> Self {
        let env = Arc::new(Env::stdlib());
        Self {
            exists: env
                .compile(
                    "e.domain == 'uds' && e.payload.bytes[0] == 98 && e.payload.bytes[1] == 241",
                )
                .unwrap(),
            hold: env
                .compile("e.payload.value >= 10.0 && e.payload.value <= 15.0")
                .unwrap(),
            env,
            exists_path: JsonPath::parse(
                "$[? @.domain == 'uds' && @.payload.bytes[0] == 98 && @.payload.bytes[1] == 241]",
            )
            .unwrap(),
            hold_path: JsonPath::parse("$[? @.payload.value >= 10 && @.payload.value <= 15]")
                .unwrap(),
        }
    }

    fn cel(&self, program: &Program, e: &Value) -> EvalResult<bool> {
        let mut context = Context::with_env(Arc::clone(&self.env));
        context
            .add_variable("e", e.clone())
            .map_err(|e| e.to_string())?;
        match program.execute(&context).map_err(|e| e.to_string())? {
            CelValue::Bool(b) => Ok(b),
            other => Err(format!("predicate returned non-bool: {other:?}")),
        }
    }

    fn eval(&self, engine: &str, hold: bool, e: &Value) -> EvalResult<bool> {
        if engine == "cel" {
            self.cel(if hold { &self.hold } else { &self.exists }, e)
        } else {
            // Single-event wrapper: JSONPath filters enumerate array members.
            let input = Value::Array(vec![e.clone()]);
            let path = if hold {
                &self.hold_path
            } else {
                &self.exists_path
            };
            Ok(!path.query(&input).all().is_empty())
        }
    }
}

fn seq(e: &Value) -> u64 {
    e["seq"].as_u64().unwrap()
}
fn time(e: &Value) -> u64 {
    e["mono_ms"].as_u64().unwrap()
}

fn exists(p: &Predicates, engine: &str, events: &[Value], lo: u64, hi: u64) -> EvalResult<Value> {
    for e in events.iter().filter(|e| (lo..=hi).contains(&seq(e))) {
        if p.eval(engine, false, e)? {
            return Ok(json!({"verdict":"pass", "witness_seq":seq(e)}));
        }
    }
    Ok(json!({"verdict":"fail", "reason":"no_match_in_complete_fixture_range"}))
}

// Time/coverage is a Rust operator shared by both expression engines.
// Sampled hold means consecutive observed values within bounds and max_gap,
// never proof of continuous physical behavior between samples.
fn hold(
    p: &Predicates,
    engine: &str,
    events: &[Value],
    lo: u64,
    hi: u64,
    duration_ms: u64,
) -> EvalResult<Value> {
    let mut run: Option<(u64, u64, u64, u64)> = None; // start seq/time, previous time/epoch
    let mut uncertain = false;
    for e in events.iter().filter(|e| (lo..=hi).contains(&seq(e))) {
        if e["domain"] != "can" || e["source"] != "can0" {
            continue;
        }
        if e["kind"] == "source.gap" {
            run = None;
            uncertain = true;
            continue;
        }
        if e["payload"]["signal"] != "VehicleSpeed" {
            continue;
        }
        // JSONPath silently excludes missing/wrong-typed values; validate explicitly.
        if e["payload"]["value"]
            .as_f64()
            .filter(|v| v.is_finite())
            .is_none()
        {
            return Err(format!(
                "seq={} path=/payload/value expected finite number",
                seq(e)
            ));
        }
        let epoch = e["epoch"].as_u64().ok_or("missing epoch")?;
        if let Some((_, _, previous_time, previous_epoch)) = run {
            if epoch != previous_epoch || time(e) <= previous_time || time(e) - previous_time > 150
            {
                run = None;
                uncertain = true;
            }
        }
        if !p.eval(engine, true, e)? {
            run = None;
            continue;
        }
        let (start_seq, start_time, _, _) = run.unwrap_or((seq(e), time(e), time(e), epoch));
        run = Some((start_seq, start_time, time(e), epoch));
        if time(e) - start_time >= duration_ms {
            return Ok(
                json!({"verdict":"pass", "evidence_seq":[start_seq,seq(e)], "observed_hold_ms":time(e)-start_time, "max_sample_gap_ms":150}),
            );
        }
    }
    Ok(
        json!({"verdict":if uncertain {"inconclusive"} else {"fail"}, "reason":if uncertain {"gap_or_epoch_or_stale_samples"} else {"insufficient_in_range_duration"}}),
    )
}

fn main() -> EvalResult<()> {
    let started = Instant::now();
    let events: Vec<Value> = include_str!("../events.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    assert_eq!(events.len(), 50);
    assert!(events
        .windows(2)
        .all(|w| seq(&w[0]) < seq(&w[1]) && time(&w[0]) <= time(&w[1])));
    let compile_start = Instant::now();
    let predicates = Predicates::new();
    let compile_ms = compile_start.elapsed().as_secs_f64() * 1000.0;
    let mut cases = Vec::new();
    for engine in ["cel", "jsonpath"] {
        let pass = exists(&predicates, engine, &events, 10, 15)?;
        assert_eq!(pass["witness_seq"], 12);
        let outside = exists(&predicates, engine, &events, 13, 18)?;
        assert_eq!(outside["verdict"], "fail");
        let boundary = exists(&predicates, engine, &events, 12, 12)?;
        assert_eq!(boundary["witness_seq"], 12);
        let held = hold(&predicates, engine, &events, 1, 24, 300)?;
        assert_eq!(held["evidence_seq"], json!([4, 13]));
        let short = hold(&predicates, engine, &events, 1, 24, 800)?;
        assert_eq!(short["verdict"], "fail");
        let gap = hold(&predicates, engine, &events, 31, 42, 300)?;
        assert_eq!(gap["verdict"], "inconclusive");
        let type_error = hold(&predicates, engine, &events, 43, 50, 200).unwrap_err();
        assert!(type_error.contains("seq=43"));
        // Enough overall duration must not bridge an out-of-range CAN sample.
        let violation = hold(&predicates, engine, &events, 1, 30, 800)?;
        assert_eq!(violation["verdict"], "fail");
        let mut epoch_events = events.clone();
        epoch_events[9]["epoch"] = json!(2);
        let epoch = hold(&predicates, engine, &epoch_events, 1, 13, 300)?;
        assert_eq!(epoch["verdict"], "inconclusive");
        let mut sparse_events = events.clone();
        sparse_events[9]["mono_ms"] = json!(351);
        let sparse = hold(&predicates, engine, &sparse_events, 1, 13, 300)?;
        assert_eq!(sparse["verdict"], "inconclusive");
        cases.push(json!({"engine":engine,"exists":pass,"outside_seq":outside,"inclusive_boundary":boundary,"hold":held,"short_hold":short,"gap":gap,"type_error":type_error,"intermediate_violation":violation,"epoch":epoch,"stale_sample":sparse}));
    }
    let malformed_cel = predicates
        .env
        .compile("e.payload.value >=")
        .err()
        .unwrap()
        .to_string();
    let malformed_path = JsonPath::parse("$[? @.payload.value >= ]")
        .err()
        .unwrap()
        .to_string();
    let missing = json!({"payload":{}});
    let missing_cel = predicates.cel(&predicates.hold, &missing).unwrap_err();
    let missing_jsonpath = predicates.eval("jsonpath", true, &missing)?;
    assert!(!missing_jsonpath);
    let mut benchmarks = Vec::new();
    for engine in ["cel", "jsonpath"] {
        let start = Instant::now();
        for _ in 0..1000 {
            black_box(exists(&predicates, engine, black_box(&events), 1, 50)?);
            black_box(hold(&predicates, engine, black_box(&events), 1, 24, 800)?);
        }
        benchmarks.push(json!({"engine":engine,"iterations":1000,"query_pairs_total_ms":start.elapsed().as_secs_f64()*1000.0}));
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "fixture_events":events.len(), "assertions_per_engine":10,
        "compile_four_predicates_ms":compile_ms,"cases":cases,
        "diagnostics":{"cel_parse":malformed_cel,"jsonpath_parse":malformed_path,"cel_missing":missing_cel,"jsonpath_missing_is_false":!missing_jsonpath},
        "benchmarks":benchmarks,"process_internal_ms":started.elapsed().as_secs_f64()*1000.0,
        "limitations":["complete synthetic range only; production journal coverage must be verified","sampled hold; not physical continuity proof","no CPU fuel enforcement tested; expressions must be bounded"]
    })).unwrap());
    Ok(())
}
