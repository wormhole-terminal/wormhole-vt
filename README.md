# wormhole-vt

A terminal engine in Rust: the escape-sequence parser, the screen and its history. Bytes in, screen
and events out. No PTY, no UI, no `unsafe`. MIT.

It is the engine of [Wormhole](https://getwormhole.app), a terminal for macOS, and was developed
inside it until it moved here with its history.

- [`crates/wormhole-vt`](crates/wormhole-vt): the library, published as `wormhole-vt`. Its
  [README](crates/wormhole-vt/README.md) is the documentation: quickstart, guide, conformance,
  security and stability.
- [`crates/wormhole-vt-bench`](crates/wormhole-vt-bench): the conformance runner for the `.vtt` cases,
  the corpora, differential tests, `vtbench`, and the headless hosts for vttest, esctest2 and ucs-detect.
- [`tools/vt-compare`](tools/vt-compare): other engines behind the same trait, for conformance and
  speed comparisons.
- [`scripts`](scripts): the outside suites as gates, mutation testing, the public API listing,
  profiling.

**Status: beta.** Version 0.1; until 1.0 the API may change between minor versions.
