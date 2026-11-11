type DecimalU64 = string;
type WireEvent = { "kind": "can.frame", "payload": { seq: DecimalU64, mono_ns: DecimalU64, data_hex: string, note: string | null, } } | { "kind": "record.health", "payload": { ok: boolean, } };
