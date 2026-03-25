#!/usr/bin/env python3
"""
Compare cppguard vs lizard cyclomatic complexity on tests/example.cpp.

Exits 0 if every function's CC is within TOLERANCE of lizard's value.
Exits 1 if any function differs by more than TOLERANCE, or a tool fails.

Usage:
    python tests/compare_lizard.py [--tolerance N] [--file path/to/file.cpp]
"""
import argparse
import csv
import io
import json
import subprocess
import sys

CPPGUARD_BIN = "./target/release/cpp_guard"
# Lizard counts &&/|| as decision points (modified McCabe); cppguard uses strict
# McCabe (ISO/IEC 14764).  A function with N boolean operators will differ by ~N.
# ±10 covers typical real-world functions without making the check meaningless.
DEFAULT_TOLERANCE = 10
DEFAULT_FILE      = "tests/example.cpp"


def run_lizard(cpp_file: str) -> dict[str, int]:
    """Return {function_name: CCN} from lizard --csv output."""
    result = subprocess.run(
        ["lizard", "--csv", cpp_file],
        capture_output=True, text=True,
    )
    # lizard exits 1 when its own thresholds are exceeded — that's fine for us.
    if result.returncode not in (0, 1):
        print("lizard error:", result.stderr, file=sys.stderr)
        sys.exit(1)

    # lizard --csv has no header; columns are positional:
    # nloc, ccn, token_count, param_count, length, location,
    # filename, function_name, long_name, start_line, end_line
    CCN_IDX  = 1
    NAME_IDX = 7

    rows = list(csv.reader(io.StringIO(result.stdout)))
    # Normalize "ClassName::method" → "method" to align with cppguard's short names.
    def _short(qualified: str) -> str:
        return qualified.rsplit("::", 1)[-1].strip()

    cc: dict[str, int] = {}
    for row in rows:
        if len(row) > NAME_IDX:
            name = _short(row[NAME_IDX])
            # Keep the first occurrence when multiple overloads share a short name.
            cc.setdefault(name, int(row[CCN_IDX].strip()))
    return cc


def run_cppguard(cpp_file: str) -> dict[str, int]:
    """Return {function_name: cyclomatic_complexity} from cppguard --json output."""
    result = subprocess.run(
        [CPPGUARD_BIN, cpp_file, "--json", "-f", "-std=c++17"],
        capture_output=True, text=True,
    )
    if result.returncode not in (0, 1):
        print("cppguard error:", result.stderr, file=sys.stderr)
        sys.exit(1)

    data = json.loads(result.stdout)
    return {
        fn["name"]: fn["cyclomatic_complexity"]
        for file_report in data
        for fn in file_report["functions"]
    }


def compare(lizard_cc: dict, guard_cc: dict, tolerance: int) -> bool:
    all_names = sorted(set(lizard_cc) | set(guard_cc))
    mismatches = []

    col = 38
    print(f"\n{'Function':<{col}} {'lizard':>8}  {'cppguard':>10}  {'diff':>6}")
    print("─" * (col + 32))

    for name in all_names:
        lcc = lizard_cc.get(name)
        gcc = guard_cc.get(name)

        if lcc is None:
            diff_str, tag = "N/A", "  (only in cppguard)"
        elif gcc is None:
            diff_str, tag = "N/A", "  (only in lizard)"
        else:
            d = abs(lcc - gcc)
            diff_str = str(d)
            tag = "  *** MISMATCH ***" if d > tolerance else ""
            if d > tolerance:
                mismatches.append((name, lcc, gcc, d))

        print(f"{name:<{col}} {str(lcc):>8}  {str(gcc):>10}  {diff_str:>6}{tag}")

    print()
    if mismatches:
        print(f"FAIL — {len(mismatches)} function(s) differ by more than ±{tolerance}:")
        for name, lcc, gcc, d in mismatches:
            print(f"  {name:<{col-2}}  lizard={lcc}  cppguard={gcc}  diff={d}")
        return False

    print(f"OK — all functions are within ±{tolerance} of lizard")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tolerance", type=int, default=DEFAULT_TOLERANCE)
    parser.add_argument("--file", default=DEFAULT_FILE)
    args = parser.parse_args()

    print(f"Comparing on: {args.file}  (tolerance ±{args.tolerance})")
    lizard_cc = run_lizard(args.file)
    guard_cc  = run_cppguard(args.file)

    if not compare(lizard_cc, guard_cc, args.tolerance):
        sys.exit(1)


if __name__ == "__main__":
    main()
