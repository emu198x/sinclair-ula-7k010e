# sinclair-ula-7k010e

> Read [`PRINCIPLES.md`](PRINCIPLES.md) first. [`MANIFESTO.md`](MANIFESTO.md) is why the project exists.

The Sinclair 7K010E, the ZX Spectrum 128K and +2's ULA. It moved out of
[emu198x/emu198x](https://github.com/emu198x/emu198x) with its history under that
repository's [workspace-shape decision](https://github.com/emu198x/emu198x/blob/main/knowledge/decisions/workspace-shape.md),
and is published so machines and siblings take it from crates.io.

## Binding rules

Emu198x's [RULES.md](https://github.com/emu198x/emu198x/blob/main/RULES.md)
rules 5 and 6 bind here, as they did in-tree:

- **Rule 5:** a CPU ticks at its evidence-backed native granularity, always
  cycle-accurate, never instruction-accurate.
- **Rule 6:** no Bus trait, no bus callback, no method-call memory access.
  Bus state is public pin fields, and the machine inspects them between ticks.
  See [cpu-bus-interface.md](https://github.com/emu198x/emu198x/blob/main/knowledge/decisions/cpu-bus-interface.md).

Those rules are what let this crate live apart from the machines that use it.
A change that would need a machine's help to stay correct belongs back in the
argument, not in this repository.

## Working here

- Rust pinned in `rust-toolchain.toml`. Before pushing: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Hardware facts cite the umbrella's `reference/` and `syntheses/`, never another
  emulator's code.
- Land work through pull requests, rebase-merged. Every commit subject and PR
  title is a conventional commit; `feat`, `fix`, `perf` and `revert` earn a release.
- A change to the public API is a change for every machine that uses it. Say what
  an existing caller has to do.
