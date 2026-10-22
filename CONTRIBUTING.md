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
