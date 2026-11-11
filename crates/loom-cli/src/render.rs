//! Human output of core commands (domains render their own via `DomainCli::render_human`).

use std::io::{self, Write};

use serde_json::Value;

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

/// `HH:MM:SS.mmm` (UTC) of a wall-clock ns string.
pub fn hms(wall_ns: &Value) -> String {
    let ns: u64 = wall_ns.as_str().and_then(|x| x.parse().ok()).unwrap_or(0);
    let ms = ns / 1_000_000;
    let (h, m, sec, milli) = (
        (ms / 3_600_000) % 24,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000,
    );
    format!("{h:02}:{m:02}:{sec:02}.{milli:03}Z")
}

fn summary(payload: &Value) -> String {
    let mut t = match payload {
        Value::Object(o) if o.contains_key("text") => s(&o["text"]),
        Value::Object(o) if o.contains_key("write_state") => {
            let mut x = format!("write_state={}", s(&o["write_state"]));
            if o.get("final") == Some(&Value::Bool(true)) {
                x.push_str(" final");
            }
            x
        }
        other => other.to_string(),
    };
    if t.len() > 120 {
        let mut cut = 117;
        while !t.is_char_boundary(cut) {
            cut -= 1;
        }
        t.truncate(cut);
        t.push_str("...");
    }
    t.replace('\n', "\\n")
}

/// One event line: `#seq time domain[:channel] kind origin summary`.
pub fn event_line(e: &Value) -> String {
    let ch = e["channel"].as_str().map(|c| format!(":{c}")).unwrap_or_default();
    format!(
        "#{} {} {}{} {} {} {}",
        s(&e["seq"]),
        hms(&e["ts"]["wall_ns"]),
        s(&e["domain"]),
        ch,
        s(&e["kind"]),
        s(&e["origin"]),
        summary(&e["payload"])
    )
}

/// `loom events`.
pub fn events(data: &Value, out: &mut dyn Write) -> io::Result<()> {
    for e in data["events"].as_array().into_iter().flatten() {
        writeln!(out, "{}", event_line(e))?;
    }
    if data["truncated"] == true {
        writeln!(
            out,
            "(truncated — continue with --since {})",
            s(&data["next_cursor"])
        )?;
    }
    Ok(())
}

/// `loom status` / `loom daemon status`.
pub fn status(d: &Value, out: &mut dyn Write) -> io::Result<()> {
    writeln!(
        out,
        "loomd {} · 127.0.0.1:{} · pid {} · up {} s",
        s(&d["version"]),
        s(&d["port"]),
        s(&d["pid"]),
        d["uptime_ms"].as_u64().unwrap_or(0) / 1000
    )?;
    writeln!(
        out,
        "STOP {} · recording {} · env {} · viewers {}/{} visible · epoch {} · seq {}",
        if d["latch"] == true { "LATCHED" } else { "off" },
        s(&d["record_health"]),
        s(&d["env"]),
        s(&d["viewers"]["visible"]),
        s(&d["viewers"]["subscribed"]),
        s(&d["stream_epoch"]),
        s(&d["stream_seq_hw"])
    )?;
    for dm in d["domains"].as_array().into_iter().flatten() {
        let cmds: Vec<String> = dm["commands"].as_array().into_iter().flatten().map(s).collect();
        writeln!(out, "domain {}: {}", s(&dm["name"]), cmds.join(" "))?;
    }
    for w in d["workers"].as_array().into_iter().flatten() {
        writeln!(
            out,
            "worker {}: {} ({})",
            s(&w["kind"]),
            s(&w["state"]),
            s(&w["host"])
        )?;
    }
    Ok(())
}

/// `loom stop-all`.
pub fn latch(d: &Value, out: &mut dyn Write) -> io::Result<()> {
    let list = |v: &Value| {
        v.as_array()
            .map(|a| a.iter().map(s).collect::<Vec<_>>().join(","))
            .unwrap_or_default()
    };
    writeln!(
        out,
        "STOP {} · acked [{}] · missing [{}] · persisted {}",
        if d["on"] == true { "latched" } else { "released" },
        list(&d["acked"]),
        list(&d["missing"]),
        s(&d["persisted"])
    )
}

/// `loom help` (human) from `core.help`.
pub fn help(d: &Value, out: &mut dyn Write) -> io::Result<()> {
    for c in d["core"].as_array().into_iter().flatten() {
        writeln!(out, "{:<60} {}", s(&c["cmd"]), s(&c["explain"]))?;
    }
    for dm in d["domains"].as_array().into_iter().flatten() {
        writeln!(out, "\n[{}] {}", s(&dm["name"]), s(&dm["help"]["summary"]))?;
        for ex in dm["help"]["examples"].as_array().into_iter().flatten() {
            writeln!(out, "  {:<40} {}", s(&ex["cmd"]), s(&ex["explain"]))?;
        }
        for n in dm["help"]["notes"].as_array().into_iter().flatten() {
            writeln!(out, "  note: {}", s(n))?;
        }
    }
    Ok(())
}

/// Generic: print the data object compactly.
pub fn generic(d: &Value, out: &mut dyn Write) -> io::Result<()> {
    if !d.is_null() {
        writeln!(out, "{d}")?;
    }
    Ok(())
}
