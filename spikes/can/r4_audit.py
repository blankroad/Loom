"""Read-only checks of the R3 plan against retained R1/R2 evidence."""
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
plan = ROOT / "docs/plan/can/plan.md"
raw = plan.read_bytes()
text = raw.decode()
frame = None
for block in re.findall(r"```json\n(.*?)\n```", text, re.S):
    try:
        candidate = json.loads(block)
    except json.JSONDecodeError:
        continue
    if candidate.get("event") == "can.frame":
        frame = candidate
        break
assert frame is not None
size = len(json.dumps(frame, ensure_ascii=False, separators=(",", ":")).encode()) + 1
r1 = json.loads((ROOT / "spikes/can/r1-results.json").read_text())
r2 = json.loads((ROOT / "spikes/can/r2-results.json").read_text())
result = {
    "plan_sha256": hashlib.sha256(raw).hexdigest(),
    "plan_lines": len(text.splitlines()),
    "numbered_sections": re.findall(r"^## (\d+)\.", text, re.M),
    "journal_example": {
        "compact_utf8_bytes_including_newline": size,
        "uncompressed_decimal_GB_per_hour_at_5000fps": size * 5000 * 3600 / 1e9,
        "decimal_MB_per_second_at_5000fps": size * 5000 / 1e6,
        "limitations": "Arithmetic using section 6.1 one-byte frame; not throughput, RSS, compression or worst-case measurement.",
    },
    "r1_fc_p99_idle_ms": r1["idle"]["isotp"]["ff_send_to_fc_send_ms"]["p99"],
    "r1_fc_p99_loaded_ms": r1["gil_decode_load"]["isotp"]["ff_send_to_fc_send_ms"]["p99"],
    "r2_send_error_p99_idle_ms": [x["absolute_interval_error_ms"]["p99"] for x in r2["send_idle"].values()],
    "r2_send_error_p99_loaded_ms": [x["absolute_interval_error_ms"]["p99"] for x in r2["send_gil_decode_load"].values()],
    "r2_blf_closed": r2["blf_closed"],
    "r2_blf_abrupt": r2["blf_abrupt"],
}
destination = ROOT / "spikes/can/r4-audit.json"
destination.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(result, ensure_ascii=False, indent=2))
