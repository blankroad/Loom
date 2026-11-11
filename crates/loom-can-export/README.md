# loom-can-export

Sealed canonical-journal records can be supplied as a one-pass iterator. This
crate emits derivative BLF/ASC files and JSONL provenance. It has no capture,
journal, RPC, gateway or CLI dependency and performs no driver calls.

```rust
use loom_can_export::{export_frames, ExportFrame, Format, Options};

// Supply frames from a sealed journal adapter; no Vec is required.
fn save(frames: impl Iterator<Item = ExportFrame>) -> Result<(), Box<dyn std::error::Error>> {
    export_frames("capture.blf", Format::Blf, Options::default(), frames)?;
    Ok(())
}
```

Use `export_file` with `Iterator<Item = Result<ExportRecord, Error>>` for input
errors, generic `ErrorFrame` observations and optional `Gap` markers. Use
`Exporter<W: Write + Seek, S: Write>` to supply streams yourself. Call `push`
one record at a time, then `finish`; an error poisons the exporter. Generic
streams are the caller's responsibility: a collecting `Cursor<Vec<u8>>` is
appropriate only for small fixtures, not large files.

## Formats and time

BLF uses exactly `autors-blf = 0.1.1`, `BlfWriter`, and nanosecond object flags
(`TIME_ONE_NANS = 2`). No whole-file `BlfFile` APIs are used. CC11/29 data, CC
RTR with requested DLC, FD64/BRS/ESI, Tx/Rx and one-based channels are mapped
from validated `loom-can-frame` values. Channel index 65535 is rejected because
its one-based value cannot fit in a BLF u16 channel. Error observations use
basic `CAN_ERROR` objects. Their driver description and direction are preserved
in the sidecar; the basic standard object has no equivalent direction field.

ASC is a bounded line writer with `date`, `base hex timestamps absolute`,
trigger block, classic `d`/`r`, `CANFD`, `ErrorFrame` and gap comment lines.
Timestamps use integer seconds and nine decimal digits. FD trailing flags
carry EDL/BRS/ESI; unavailable wire timing fields are zero placeholders. The
upstream ASC writer collects all records/output and prints six decimal digits,
so it is used only as a dev-dependency reader for small fixtures.

`Options` supplies a fixed monotonic anchor and optional UTC Unix-nanosecond wall
anchor. Records must be nondecreasing and at or after the monotonic anchor.
On-file relative nanoseconds are:

```text
ts_mono_ns - anchor_mono_ns + (anchor_wall_ns.unwrap_or(0) % 1_000_000)
```

BLF/ASC date headers have millisecond precision. Adding the submillisecond
remainder to event timestamps retains exact anchored absolute nanoseconds.
BLF `finish` patches the 16-byte SYSTEMTIME end field at header offset 56 after
the upstream writer patches counts/sizes. UTC is explicit in ASC comments;
no local timezone or current-clock inference occurs. A missing wall anchor uses
1970-01-01 as a placeholder and sidecar `wall_anchor_known=false`. Per-record
`wall_ns` and `device_ts` remain independent observations, including clock ID,
source and quality, rather than replacing host-ingest time. The caller must
select the proper anchor/segment for clock changes.

## Provenance and publication

`capture.blf.loom.jsonl` (likewise ASC) has a versioned header, one row per input
record, and a completion footer. Decimal strings preserve all u64 timestamps,
counts, producer epochs, original source sequences and object/record indices
through JSON clients. Frame rows retain origin, request ID, original
`(producer_id, producer_epoch, source_seq)`, wall/device clocks and time quality.
Zero-based `object_index` joins frame/error rows to standard objects. Gaps have
null object index and separate source/record/display scope. An unknown loss count
is null, never fabricated as zero. The first/last original coordinates appear in
the bounded summary/footer. No new journal or stream sequence is invented.

Filesystem export creates same-directory `capture.blf.partial` and
`capture.blf.loom.jsonl.partial` with `create_new`. It finishes and syncs both
files, renames the sidecar, then atomically renames the primary as the commit
marker. Read a final primary together with its complete sidecar. These are two
individual renames, not a two-file atomic transaction. A crash between renames
can leave an orphan final sidecar; without a final primary it is uncommitted.
Directory metadata is not fsynced, so no power-failure durability guarantee is
made. Callers must serialize/own the destination path; existence checks do not
provide cross-process atomic no-clobber semantics.

Handled errors return `Box<ExportFailure>` with paths and last staged source
coordinates, leave `.partial` files and attempt a bounded `failed` sidecar row.
A disk error/process crash can leave a torn row or no footer. Regenerate from
the canonical journal; an unfinished BLF header/container is not repaired into a
claimed complete capture. Existing final or partial artifacts are refused and
never resumed implicitly. Rename failure attempts to return the sidecar to its
partial name. A primary partial can remain if creation of the sidecar failed.

## Resource bounds

| Resource | Bound / overflow behavior |
|---|---|
| Verification BLF header | 64 KiB, reader rejects larger declarations |
| Verification BLF compressed/decompressed container | 8 MiB, reader rejects larger declarations/inflation |
| Verification BLF object | 1 MiB, reader rejects larger declarations |
| Generated objects | only fixed CAN/FD/error objects, at most 128 bytes |
| BLF object staging capacity | at most 256 KiB; flush threshold explicitly 128 KiB |
| Generated container raw bytes | less than 128 KiB; no input-dependent large object |
| Sidecar / ASC line staging | 4096 / 512 bytes, write fails before growth |
| File buffers | two fixed 64 KiB `BufWriter`s |
| Request / producer label | 128 UTF-8 bytes each, reject before JSON cloning |
| Gap reason / error description | 256 UTF-8 bytes each, reject before cloning |
| File path | 4096 bytes, reject larger paths |
| Stored provenance summary | first/last bounded source only, no growing history |
| Standard object count | u32 maximum, reject before writing beyond limit |

The pinned BLF writer uses a geometrically growing Vec. A 128 KiB flush threshold
keeps its capacity below 256 KiB even for mixed object sizes. This is the object
staging limit, not total process RSS: upstream container serialization makes
temporary copies (each bounded by the container size); optional zlib compression
adds fixed compressor state and bounded output scratch. File buffers and JSON
staging are additional. All are independent of total record count. The bounded
reader helper is for verification, not a general import/unknown-object adapter.

## Validation limits

Tests use the independent autors BLF/ASC readers, manually checked binary header,
container/object bytes and ASC lines, truncation/limit/error recovery fixtures,
and streamed 100,000-frame exports. ASC FD length zero is checked with independent
tokens because the upstream ASC reader incorrectly marks that case as remote.

CANoe and PCAN-View imports, Windows/Linux execution and external golden captures
remain **unverified**. Passing these tests does not pass that compatibility gate.
TRC, MF4, standard-file import, unknown-object statistics and journal/RPC export
jobs are outside this crate's implementation.

Version/source references:
[autors-blf 0.1.1](https://docs.rs/crate/autors-blf/0.1.1),
[streaming implementation](https://docs.rs/crate/autors-blf/0.1.1/source/src/stream.rs),
[autors-asc 0.1.1 implementation](https://docs.rs/crate/autors-asc/0.1.1/source/src/lib.rs).
