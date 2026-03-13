#!/usr/bin/env python3
"""Convert RTK-format TOML filter files into tokf format.

RTK stores filters under [filters.<name>] with inline [[tests.<name>]] blocks.
tokf uses a flat TOML structure with separate _test/ directories for test cases.

Usage:
    python3 scripts/convert-rtk-filters.py <rtk-filter.toml> [--out-dir <dir>]

Examples:
    # Convert a single RTK filter — outputs to cwd
    python3 scripts/convert-rtk-filters.py gcc.toml

    # Convert into the stdlib filter directory
    python3 scripts/convert-rtk-filters.py gcc.toml --out-dir crates/tokf-cli/filters/

    # Convert all RTK filters from a directory
    for f in rtk-filters/*.toml; do
        python3 scripts/convert-rtk-filters.py "$f" --out-dir crates/tokf-cli/filters/
    done
"""

from __future__ import annotations

import argparse
import os
import re
import sys

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ImportError:
        print("error: Python 3.11+ or `pip install tomli` required", file=sys.stderr)
        sys.exit(1)


def match_command_to_command(match_command: str) -> tuple[str, bool]:
    """Convert an RTK match_command regex to a tokf command prefix pattern.

    Returns (command_pattern, needs_manual_review).

    Simple cases like `^git\\s+push\\b` → "git push".
    Complex regexes are returned as-is with a review flag.
    """
    # Strip anchors
    pattern = match_command.strip()
    if pattern.startswith("^"):
        pattern = pattern[1:]

    # Strip trailing word boundary
    pattern = re.sub(r"\\b$", "", pattern)
    pattern = re.sub(r"\$$", "", pattern)

    # Replace \\s+ with single space
    pattern = re.sub(r"\\s\+", " ", pattern)

    # Check if it's a simple literal command (only word chars and spaces)
    cleaned = pattern.replace("\\", "")
    if re.match(r"^[\w\s.*/-]+$", cleaned) and not re.search(
        r"[()|\[\]{}?+^$]", pattern
    ):
        return cleaned.strip(), False

    # Complex regex — return as-is and flag for review
    return match_command, True


def convert_filter(name: str, rtk_filter: dict) -> tuple[str, bool]:
    """Convert an RTK filter dict to a tokf TOML string.

    Returns (toml_string, needs_manual_review).
    """
    lines: list[str] = []
    needs_review = False

    # command (from match_command)
    match_cmd = rtk_filter.get("match_command", "")
    command, cmd_needs_review = match_command_to_command(match_cmd)
    if cmd_needs_review:
        lines.append(f"# TODO: review — RTK uses regex: {match_cmd}")
        needs_review = True
    lines.append(f'command = "{_escape_toml(command)}"')

    # description
    if desc := rtk_filter.get("description"):
        lines.append(f'description = "{_escape_toml(desc)}"')

    # strip_ansi
    if rtk_filter.get("strip_ansi"):
        lines.append("strip_ansi = true")

    # skip (strip_lines_matching)
    if skip_patterns := rtk_filter.get("strip_lines_matching"):
        lines.append("skip = [")
        for pat in skip_patterns:
            lines.append(f'  "{_escape_toml(pat)}",')
        lines.append("]")

    # keep (keep_lines_matching)
    if keep_patterns := rtk_filter.get("keep_lines_matching"):
        lines.append("keep = [")
        for pat in keep_patterns:
            lines.append(f'  "{_escape_toml(pat)}",')
        lines.append("]")

    # head / tail / max_lines / truncate_lines_at / on_empty
    # (emit scalar fields before array-of-tables sections)
    if (head := rtk_filter.get("head_lines")) is not None:
        lines.append(f"head = {head}")
    if (tail := rtk_filter.get("tail_lines")) is not None:
        lines.append(f"tail = {tail}")
    if (max_lines := rtk_filter.get("max_lines")) is not None:
        lines.append(f"max_lines = {max_lines}")
    if (trunc := rtk_filter.get("truncate_lines_at")) is not None:
        lines.append(f"truncate_lines_at = {trunc}")
    if on_empty := rtk_filter.get("on_empty"):
        lines.append(f'on_empty = "{_escape_toml(on_empty)}"')

    # replace
    if replaces := rtk_filter.get("replace"):
        for rep in replaces:
            lines.append("")
            lines.append("[[replace]]")
            lines.append(f'pattern = "{_escape_toml(rep["pattern"])}"')
            replacement = rep.get("replacement", rep.get("output", ""))
            # Convert $N to {N} syntax
            output = _convert_dollar_syntax(replacement)
            lines.append(f'output = "{_escape_toml(output)}"')

    # match_output
    if match_rules := rtk_filter.get("match_output"):
        for rule in match_rules:
            lines.append("")
            lines.append("[[match_output]]")
            if pat := rule.get("pattern"):
                lines.append(f'pattern = "{_escape_toml(pat)}"')
            if contains := rule.get("contains"):
                lines.append(f'contains = "{_escape_toml(contains)}"')
            output = rule.get("message", rule.get("output", ""))
            lines.append(f'output = "{_escape_toml(output)}"')
            if unless := rule.get("unless"):
                lines.append(f'unless = "{_escape_toml(unless)}"')

    return "\n".join(lines) + "\n", needs_review


def convert_tests(name: str, tests: list[dict]) -> list[tuple[str, str]]:
    """Convert RTK inline tests to tokf test case TOML files.

    Returns list of (filename, content) pairs.
    """
    results = []
    for i, test in enumerate(tests):
        test_name = test.get("name", f"test_{i + 1}")
        slug = _slugify(test_name)
        filename = f"{slug}.toml"

        tc_lines: list[str] = []
        tc_lines.append(f'name = "{_escape_toml(test_name)}"')

        # exit_code (RTK doesn't have this, default to 0)
        exit_code = test.get("exit_code", 0)
        if exit_code != 0:
            tc_lines.append(f"exit_code = {exit_code}")

        # input → inline (use literal multiline strings for safety)
        if raw_input := test.get("input"):
            if "\n" in raw_input:
                tc_lines.append(f"inline = '''\n{raw_input}'''")
            else:
                tc_lines.append(f'inline = "{_escape_toml(raw_input)}"')

        # expected → [[expect]] equals
        if expected := test.get("expected"):
            tc_lines.append("")
            tc_lines.append("[[expect]]")
            if "\n" in expected:
                tc_lines.append(f"equals = '''\n{expected}'''")
            else:
                tc_lines.append(f'equals = "{_escape_toml(expected)}"')

        results.append((filename, "\n".join(tc_lines) + "\n"))

    return results


def _escape_toml(s: str) -> str:
    """Escape a string for use in a TOML basic string (double-quoted)."""
    s = s.replace("\\", "\\\\")
    s = s.replace('"', '\\"')
    s = s.replace("\t", "\\t")
    # Don't escape newlines — they should already be literal \n in patterns
    return s


def _convert_dollar_syntax(template: str) -> str:
    """Convert RTK $N capture references to tokf {N} syntax."""
    return re.sub(r"\$(\d+)", r"{\1}", template)


def _slugify(name: str) -> str:
    """Convert a test name to a safe filename slug."""
    slug = name.lower()
    slug = re.sub(r"[^a-z0-9]+", "_", slug)
    slug = slug.strip("_")
    return slug or "test"


def parse_rtk_file(path: str) -> tuple[dict[str, dict], dict[str, list[dict]]]:
    """Parse an RTK TOML file into (filters_dict, tests_dict)."""
    with open(path, "rb") as f:
        data = tomllib.load(f)

    filters = data.get("filters", {})
    tests = data.get("tests", {})

    return filters, tests


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Convert RTK TOML filters to tokf format"
    )
    parser.add_argument("input", help="RTK filter TOML file to convert")
    parser.add_argument(
        "--out-dir",
        default=".",
        help="Output directory for converted filters (default: cwd)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would be written without writing files",
    )
    args = parser.parse_args()

    filters, tests = parse_rtk_file(args.input)

    if not filters:
        print(f"warning: no [filters.*] sections found in {args.input}", file=sys.stderr)
        return

    review_needed = []

    for name, rtk_filter in filters.items():
        # Determine output path: use the filter name as filename
        # e.g. "gcc" → gcc.toml, "dotnet-build" → dotnet-build.toml
        filter_filename = f"{name}.toml"
        filter_path = os.path.join(args.out_dir, filter_filename)

        # Convert filter
        filter_toml, needs_review = convert_filter(name, rtk_filter)
        if needs_review:
            review_needed.append(name)

        if args.dry_run:
            print(f"--- {filter_path} ---")
            print(filter_toml)
        else:
            os.makedirs(os.path.dirname(filter_path) or ".", exist_ok=True)
            with open(filter_path, "w") as f:
                f.write(filter_toml)
            print(f"  wrote {filter_path}")

        # Convert tests
        filter_tests = tests.get(name, [])
        if filter_tests:
            test_dir_name = f"{name}_test"
            test_dir = os.path.join(args.out_dir, test_dir_name)

            if args.dry_run:
                for tc_filename, tc_content in convert_tests(name, filter_tests):
                    tc_path = os.path.join(test_dir, tc_filename)
                    print(f"--- {tc_path} ---")
                    print(tc_content)
            else:
                os.makedirs(test_dir, exist_ok=True)
                for tc_filename, tc_content in convert_tests(name, filter_tests):
                    tc_path = os.path.join(test_dir, tc_filename)
                    with open(tc_path, "w") as f:
                        f.write(tc_content)
                    print(f"  wrote {tc_path}")

    # Summary
    total_filters = len(filters)
    total_tests = sum(len(tests.get(name, [])) for name in filters)
    print(f"\nConverted {total_filters} filter(s), {total_tests} test(s)")

    if review_needed:
        print(f"\n⚠ Filters needing manual review (complex match_command regex):")
        for name in review_needed:
            cmd = filters[name].get("match_command", "")
            print(f"  {name}: {cmd}")


if __name__ == "__main__":
    main()
