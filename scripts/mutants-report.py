#!/usr/bin/env python3
"""Gate on the results of mutation runs (scripts/mutants.sh).

    scripts/mutants-report.py DIR [--expect N] [--annotate] [--allow-empty] [--merged-out FILE]

Reads every `mutants.out` folder (one with a missed.txt) under DIR (the shards' artifacts, or target/mutants), adds their
caught / missed / timeout / unviable lists up, and fails (exit 1) when a mutant is missed, i.e. no
test fails when the code is changed that way, or, with --expect, when fewer mutants were measured than
`scripts/mutants.sh --list` counts (a shard that ran out of time or never started). A timeout counts as
caught, as in mutants.sh. The answer to a missed mutant is in CONTRIBUTING.md.

--allow-empty: a change with no mutants (comments only) leaves no mutants.out, and passes.
--annotate prints each miss as a GitHub error on its line (the first ten show up on the change);
--merged-out writes the merged list of misses, one "path:line:col: what" per line. The summary goes
to $GITHUB_STEP_SUMMARY when that is set. Standard library only, so a runner needs nothing installed.
"""
import argparse
import os
import sys

KINDS = ("caught", "missed", "timeout", "unviable")
HELP = (
    "A missed mutant is a change to the engine that no test notices. Kill it with a test that fails "
    "on the mutant, or, when it cannot change behaviour (an equivalent mutant), list it in "
    "crates/wormhole-vt/mutants.toml with the reason. See CONTRIBUTING.md, 'Answering a missed mutant'."
)


def out_dirs(root):
    """Every folder under root that holds a run's lists: a mutants.out, or its files alone when an
    artifact's upload dropped the folder's own name. Not inside one, nor in a scratch copy of the tree."""
    found = []
    for path, dirs, files in os.walk(root):
        if "missed.txt" in files:
            found.append(path)
            dirs[:] = []
        else:
            dirs[:] = [d for d in dirs if d != "tmp"]
    return sorted(found)


def read_list(folder, kind):
    try:
        with open(os.path.join(folder, kind + ".txt"), encoding="utf-8") as f:
            return [line.rstrip("\n") for line in f if line.strip()]
    except FileNotFoundError:
        return []


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("root")
    ap.add_argument("--expect", type=int, help="how many mutants a complete run measures")
    ap.add_argument("--annotate", action="store_true")
    ap.add_argument("--allow-empty", action="store_true", help="no mutants.out is not a failure")
    ap.add_argument("--merged-out")
    args = ap.parse_args()
    on_github = bool(os.environ.get("GITHUB_ACTIONS"))

    folders = out_dirs(args.root)
    # A mutant is in one shard only; a set also makes a folder that was copied twice harmless.
    lists = {k: set() for k in KINDS}
    for folder in folders:
        for k in KINDS:
            lists[k].update(read_list(folder, k))
    missed = sorted(lists["missed"])
    measured = sum(len(v) for v in lists.values())

    if args.merged_out:
        with open(args.merged_out, "w", encoding="utf-8") as f:
            f.writelines(line + "\n" for line in missed)

    problems = []
    if not folders and not args.allow_empty:
        problems.append(f"no mutants.out under {args.root}: nothing was measured")
    if args.expect is not None and measured != args.expect:
        problems.append(
            f"{measured} of {args.expect} mutants were measured: a shard did not finish "
            "(see its log and its artifact), so the missing ones are unknown, not caught"
        )
    if missed:
        problems.append(f"{len(missed)} mutants missed")

    counts = "  ".join(f"{k} {len(lists[k])}" for k in KINDS)
    print(f"{len(folders)} result folders, {measured} mutants measured: {counts}")
    for line in missed:
        print(f"missed  {line}")
        if args.annotate:
            # "path:line:column: replace … with …"
            parts = line.split(":", 3)
            if len(parts) == 4 and parts[1].isdigit():
                print(f"::error file={parts[0]},line={parts[1]}::No test notices this change:{parts[3]}")
    for p in problems + ([HELP] if missed else []):
        print(f"::error::{p}" if on_github else f"error: {p}")

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            of = f" of {args.expect}" if args.expect is not None else ""
            f.write(f"### Mutants: {'failed' if problems else 'all caught'}\n\n")
            f.write(f"{measured}{of} measured ({counts}).\n\n")
            f.write("".join(f"- **{p}**\n" for p in problems))
            if missed:
                f.write(f"\n{HELP}\n\n")
                f.write("".join(f"- `{line}`\n" for line in missed[:300]))
                if len(missed) > 300:
                    f.write(f"- … and {len(missed) - 300} more, in the `mutants-missed` artifact\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
