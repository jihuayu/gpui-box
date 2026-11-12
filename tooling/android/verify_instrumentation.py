"""Verify complete AndroidJUnitRunner output, including its per-test protocol."""
import argparse
import json
from pathlib import Path
import re


def verify(text, minimum_tests):
    summaries = re.findall(r"^OK \((\d+) tests?\)\s*$", text, re.MULTILINE)
    if len(summaries) != 1 or int(summaries[0]) < minimum_tests:
        raise ValueError("Missing complete instrumentation success summary or insufficient tests")
    total = int(summaries[0])
    fields = {}
    started = []
    completed = []
    final_codes = []
    for line in text.splitlines():
        if line.startswith("INSTRUMENTATION_STATUS: "):
            key, separator, value = line.removeprefix("INSTRUMENTATION_STATUS: ").partition("=")
            if separator:
                fields[key] = value
        elif line.startswith("INSTRUMENTATION_STATUS_CODE: "):
            code = int(line.removeprefix("INSTRUMENTATION_STATUS_CODE: "))
            if code not in (0, 1):
                raise ValueError(f"Instrumentation test returned status {code}")
            if int(fields.get("numtests", "0")) != total:
                raise ValueError("Per-test count differs from the final summary")
            current = int(fields.get("current", "0"))
            identity = (current, fields.get("class"), fields.get("test"))
            if not identity[1] or not identity[2]:
                raise ValueError("Missing test class or method")
            if code == 1:
                started.append(identity)
            else:
                completed.append(identity)
            fields = {}
        elif line.startswith("INSTRUMENTATION_CODE: "):
            final_codes.append(int(line.removeprefix("INSTRUMENTATION_CODE: ")))
    if final_codes != [-1]:
        raise ValueError("Instrumentation did not return its single normal completion code")
    if len(started) != total or started != completed:
        raise ValueError("Started and completed tests differ from the final summary")
    if [item[0] for item in completed] != list(range(1, total + 1)):
        raise ValueError("Test progress contains missing, repeated or out-of-order entries")
    if len({item[1:] for item in completed}) != total:
        raise ValueError("Repeated test identities cannot satisfy full-suite acceptance")
    return {"schema_version": 1, "success": True, "tests_passed": total,
            "runtime_executed": True,
            "tests": [{"class": item[1], "method": item[2]} for item in completed]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("receipt", type=Path)
    parser.add_argument("--minimum-tests", type=int, default=23)
    args = parser.parse_args()
    if args.minimum_tests < 1:
        parser.error("minimum-tests must be positive")
    try:
        receipt = verify(args.output.read_text(encoding="utf-8"), args.minimum_tests)
    except (ValueError, OSError) as error:
        args.receipt.write_text(json.dumps({"schema_version": 1, "success": False,
            "runtime_executed": True, "error": str(error)}, indent=2) + "\n", encoding="utf-8")
        raise SystemExit(str(error))
    args.receipt.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(f"Verified {receipt['tests_passed']} complete instrumentation tests")


if __name__ == "__main__":
    main()
