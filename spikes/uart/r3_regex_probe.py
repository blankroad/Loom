"""Planning-only regex timeout/GIL probe; no UART or product implementation.

Child watchdogs protect this experiment, not a proposed production matcher.
"""
import asyncio
import concurrent.futures
import importlib.metadata
import json
import platform
import statistics
import subprocess
import sys
import threading
import time

import regex


def summarize(values):
    return {"median_ms": round(statistics.median(values), 3),
            "min_ms": round(min(values), 3), "max_ms": round(max(values), 3),
            "samples_ms": [round(x, 3) for x in values]}


def matching_case(pattern, length, repeats=7, direct=False):
    compiled = regex.compile(pattern)
    text = "a" * length + "!"  # immutable built-in str
    elapsed, outcomes = [], []
    for _ in range(repeats):
        started = time.perf_counter()
        try:
            match = (regex.search(pattern, text, timeout=0.05) if direct else
                     compiled.search(text, timeout=0.05, concurrent=True))
            outcomes.append("match" if match else "no_match")
        except Exception as exc:
            outcomes.append(f"{type(exc).__module__}.{type(exc).__name__}: {exc}")
        elapsed.append((time.perf_counter() - started) * 1000)
    return {"pattern": pattern, "a_count": length, "input_chars": len(text),
            "timeout_ms": 50, "concurrent": "default" if direct else True,
            "api": "regex.search" if direct else "compiled.search", "outcomes": outcomes,
            "timing": summarize(elapsed)}


def clock_case():
    compiled = regex.compile(r"^(a|aa)+$")
    text = "a" * 40 + "!"
    rows = []
    for _ in range(7):
        wall, cpu = time.perf_counter(), time.process_time()
        try:
            compiled.search(text, timeout=0.05, concurrent=True)
            outcome = "returned"
        except TimeoutError:
            outcome = "builtins.TimeoutError"
        rows.append({"outcome": outcome,
                     "wall_ms": round((time.perf_counter() - wall) * 1000, 3),
                     "process_cpu_ms": round((time.process_time() - cpu) * 1000, 3)})
    return rows


def gil_case(mode, repeats=5):
    compiled = regex.compile(r"^(a|aa)+$")
    text = "a" * 40 + "!"
    runs = []
    for _ in range(repeats):
        ticks = []
        stop = threading.Event()
        ready = threading.Event()

        def counter():
            ready.set()
            while not stop.wait(0.01):
                ticks.append(time.perf_counter())

        thread = threading.Thread(target=counter)
        thread.start()
        ready.wait()
        time.sleep(0.03)
        started = time.perf_counter()
        kwargs = {"timeout": 0.25}
        if mode != "default":
            kwargs["concurrent"] = mode == "true"
        try:
            compiled.search(text, **kwargs)
            outcome = "returned"
        except Exception as exc:
            outcome = f"{type(exc).__module__}.{type(exc).__name__}"
        ended = time.perf_counter()
        time.sleep(0.03)
        stop.set()
        thread.join(timeout=1)
        assert not thread.is_alive()
        gaps = [(b - a) * 1000 for a, b in zip(ticks, ticks[1:])]
        runs.append({"outcome": outcome, "match_ms": round((ended - started) * 1000, 3),
                     "ticks_during_match": sum(started < t < ended for t in ticks),
                     "max_counter_gap_ms": round(max(gaps), 3) if gaps else None})
    return {"concurrent": mode, "period_ms": 10, "timeout_ms": 250, "runs": runs,
            "median_ticks": statistics.median(r["ticks_during_match"] for r in runs),
            "median_max_gap_ms": statistics.median(r["max_counter_gap_ms"] for r in runs)}


async def loop_case():
    compiled = regex.compile(r"^(a|aa)+$")
    text = "a" * 40 + "!"
    loop = asyncio.get_running_loop()
    ticks, durations = [], []
    stop = asyncio.Event()

    async def heartbeat():
        while not stop.is_set():
            ticks.append(time.perf_counter())
            await asyncio.sleep(0.01)

    def match():
        started = time.perf_counter()
        try:
            compiled.search(text, timeout=0.25, concurrent=True)
            outcome = "returned"
        except Exception as exc:
            outcome = f"{type(exc).__module__}.{type(exc).__name__}"
        return {"started": started, "ended": time.perf_counter(), "outcome": outcome}

    task = asyncio.create_task(heartbeat())
    await asyncio.sleep(0.03)
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        for _ in range(5):
            durations.append(await loop.run_in_executor(executor, match))
    await asyncio.sleep(0.03)
    stop.set()
    await task
    gaps = [(b - a) * 1000 for a, b in zip(ticks, ticks[1:])]
    return {"matches": [{"outcome": r["outcome"],
                          "elapsed_ms": round((r["ended"] - r["started"]) * 1000, 3),
                          "heartbeat_ticks_during": sum(r["started"] < t < r["ended"] for t in ticks)}
                         for r in durations],
            "heartbeat_gap": summarize(gaps), "period_ms": 10}


def child(kind, args):
    if kind == "pattern":
        return matching_case(args[0], int(args[1]))
    if kind == "direct":
        return matching_case(args[0], int(args[1]), direct=True)
    if kind == "clock":
        return clock_case()
    if kind == "gil":
        return gil_case(args[0])
    if kind == "loop":
        return asyncio.run(loop_case())
    raise ValueError(kind)


def main():
    result = {"os": platform.platform(), "python": platform.python_version(),
              "implementation": platform.python_implementation(),
              "gil_enabled": sys._is_gil_enabled() if hasattr(sys, "_is_gil_enabled") else None,
              "regex_version": importlib.metadata.version("regex"), "cases": []}
    cases = [("pattern", [r"(a+)+$", "32"]),
             ("pattern", [r"^(\w+\s?)*$", "24"]),
             ("pattern", [r"(a+)+$", "32768"]),
             ("pattern", [r"^(\w+\s?)*$", "32768"]),
             ("pattern", [r"^(a|aa)+$", "40"]),
             ("gil", ["true"]), ("gil", ["false"]), ("gil", ["default"]),
             ("loop", [])]
    for kind, args in cases:
        try:
            run = subprocess.run([sys.executable, __file__, "--child", kind, *args],
                                 capture_output=True, text=True, timeout=8, check=True)
            result["cases"].append({"kind": kind, "result": json.loads(run.stdout)})
        except subprocess.TimeoutExpired:
            result["cases"].append({"kind": kind, "args": args, "outer_watchdog": True})
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        print(json.dumps(child(sys.argv[2], sys.argv[3:]), ensure_ascii=False))
    else:
        main()
