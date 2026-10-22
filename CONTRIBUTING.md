# Contributing

Thanks for helping. Bug reports with the bytes that cause them are the most useful thing there is: a
hex dump or a file, what the screen shows, and what another terminal (xterm, ideally) shows instead.
Security problems go through [SECURITY.md](crates/wormhole-vt/SECURITY.md), not a public issue.

## Building and checking

The Rust version is pinned in `rust-toolchain.toml`; `rustup` installs it on the first build. A
change is ready when these pass:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs them on Linux, macOS and Windows, plus the oldest supported Rust (`rust-version`), the docs, the
package, a minute of fuzzing per target, and in `engine.yml` the outside suites (esctest2,
ucs-detect, vttest), the public API's listing and mutation testing on the lines a change touches. The
scripts in `scripts/` run each of those locally; each says how at its top.

## Answering a missed mutant

[cargo-mutants](https://mutants.rs) changes the engine one place at a time (an operator, a return
value, a branch) and runs the tests. A mutant that no test fails on is "missed": that code's behaviour
is not pinned down. `engine.yml` runs it on the lines a change touches (`scripts/mutants.sh --in-diff`)
and **fails the check** when one is missed, with an error on its line. Locally:

```sh
git diff origin/main -- crates/wormhole-vt/src > /tmp/change.diff
scripts/mutants.sh --in-diff /tmp/change.diff     # missed mutants: target/mutants/*/mutants.out/missed.txt
```

Answer each missed mutant in one of two ways:

1. **Kill it with a test.** Write the test that fails when the code is changed like that (the mutant's
   description, `replace < with <= in Grid::scroll_up`, says what to pin down), in a conformance case
   (`.vtt`) or next to the code. Run `scripts/mutants.sh --in-diff` again: it is caught now.
2. **List it as equivalent in `crates/wormhole-vt/mutants.toml`**, only when the change cannot be seen
   from outside (a capacity hint, two paths that give the same result). Add a regex to `exclude_re`
   with a comment that says why. Use the function's name when every mutant of that kind in it is
   equivalent, and the line and column (`file\\.rs:90:39: replace \\| with \\^`) when others in the
   function are not: those drift, so a line that comes back after an edit is looked at again. "Hard to
   test" is not a reason; that is a test still to write.

A full run (Actions → Engine → Run workflow, "mutants" on; about 10,700 mutants in 32 shards, hours)
measures all of the engine, adds the shards up and fails while any mutant is missed or unmeasured;
`mutants-missed` is the list.

## How the code is kept

- **Written from the specifications.** The engine's code (`crates/wormhole-vt`) is written from the
  DEC manuals, xterm's control sequence documentation, the Unicode standard and the protocols'
  own documents, never copied or ported from another terminal or engine. Other engines are welcome as
  black boxes in `tools/vt-compare`.
- **No `unsafe`** in the engine (`#![forbid(unsafe_code)]`), and no panics on any input: whatever a
  program writes is hostile input.
- **Structure.** Layers only depend downwards and no file passes 1,000 lines (aim for about 600);
  `tests/structure.rs` checks both. One module per concept.
- **Tests.** A sequence's behaviour gets a conformance case (`crates/wormhole-vt/tests/conformance/*.vtt`)
  or a test next to the code. Test names are sentences about the behaviour. Comments say why, not what.
- **The hot paths allocate nothing**: printing, scrolling and styles reuse the grid's pages. `vtbench`
  and the allocation tests show it.
- **The public API** is listed in `crates/wormhole-vt/api.txt` (`scripts/vt-api.sh --update`), so a change
  to it always shows in review.

## Commits and the changelog

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
`type(scope): subject`, lowercase, no final period, at most 100 characters (`fix(vt): …`,
`feat(vt): …`, `test(vt): …`). History is linear: rebase instead of merging. A change people will notice
adds a line under `## [Unreleased]` in `crates/wormhole-vt/CHANGELOG.md`.

## Licence

By contributing you agree that your contribution is licensed under the MIT licence of this repository.
