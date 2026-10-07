# headroom

`headroom` tells you whether your Mac can take one more worker process. It
checks the host twice, one second apart, then prints a verdict and the numbers
behind it.

It is read-only and uses public macOS APIs. It does not write files, make
network requests, or launch other processes.

## Get started

Install the latest release with Homebrew:

```sh
brew install --cask blacktop/tap/headroom
headroom
```

To build from source, follow the steps below.

You'll need macOS, stable Rust, and Apple's command-line tools. Run these
commands in zsh or Fish:

```sh
git clone https://github.com/blacktop/headroom.git
cd headroom
cargo build --release --locked
./target/release/headroom
```

The report looks like this:

```text
ADMIT: cpu busy 0.75, load 35.7 on 18 cores, memory free 0.69, thermal nominal
cpu_busy_ratio: 0.75
load1_per_core: 1.98
load1: 35.7
load5: 9.3
load15: 9.0
ncpu: 18
mem_total_bytes: 137438953472
mem_free_ratio: 0.69
compressor_bytes: 5237522432
swapouts_delta: 0
swap_used_bytes: 788398080
pressure_level: normal
thermal_level: nominal
```

For scripts and agent orchestrators, request JSON:

```sh
./target/release/headroom --format json
```

Both formats use the same exit codes:

| Code | Verdict | What to do |
| --- | --- | --- |
| `0` | `ADMIT` | The host is within the configured limits. |
| `1` | `REFUSE` | Wait before starting another worker. |
| `2` | `UNKNOWN` | Check the report or diagnostic message. |

An orchestrator should launch a worker only after exit code `0`. To install
`headroom` on your Cargo path, run `cargo install --path . --locked`.

## Set the limits

By default, `headroom` refuses when CPU use exceeds 90%, thermal pressure
reaches `heavy`, readable memory pressure reaches `warn`, or the swapout
counter increases between samples. If CPU use cannot be measured, it falls
back to refusing when the latest one-minute load exceeds the CPU count.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--format text\|json` | `text` | Choose the report format. |
| `--max-cpu-busy RATIO` | `0.90` | Refuse when the CPU busy ratio exceeds this number. |
| `--max-load-per-core NUMBER` | `1.0` | Load-per-core limit when CPU use is unavailable. |
| `--max-thermal LEVEL` | `heavy` | Refuse at or above this thermal level. |
| `--max-pressure LEVEL` | `warn` | Refuse at or above this memory-pressure level. |
| `--allow-swapping` | Off | Permit an increase in swapouts. |

Thermal levels are `nominal`, `moderate`, `heavy`, `trapping`, and `sleeping`.
Memory-pressure levels are `normal`, `warn`, and `critical`. The CPU limit must
be between `0` and `1`. The load limit must be a finite, non-negative number.
Use `--help` for the full command help.

## Read the report

`REFUSE` lists every triggered reason, ordered by thermal pressure, memory
pressure, active swapping, then CPU use (or load when the fallback applies).
Missing or invalid required data produces
`UNKNOWN`, even when another refusal reason is known. Required data includes
load, CPU count, total RAM, VM counters, kernel page size, and thermal state.
A decreasing swap counter also produces `UNKNOWN`.

Swap usage and memory pressure are optional. If macOS denies either read, its
value is `null`; that alone does not produce `UNKNOWN`.

`cpu_busy_ratio` measures user, system, and nice CPU ticks over all ticks in the
one-second interval. Missing ticks or a zero total produce `null` and activate
the load fallback. The text verdict says when that fallback is in use.

JSON is one compact object with `schema: 2`, `verdict`, `reasons`, and the same
evidence fields as the text report. It keeps full numeric precision. Byte
counts are integers, and `swapouts_delta` is a count of pages. The free-memory
ratio adds free, inactive, speculative, and purgeable pages, then divides by
total RAM. It is capped at `1.0` because those categories can overlap.

## Development

Install the development tools, then run `just` to see the available commands:

```sh
brew install just cargo-deny cargo-edit goreleaser
just check
just build
```

The [sampler](src/sample.rs) uses `getloadavg`, `sysctlbyname`,
`host_statistics`, `host_statistics64`, and the public thermal-pressure
notification. The
[verdict rule](src/rule.rs) takes plain samples, so its tests do not depend on
how busy the test machine is. The [SDK contract](tests/sdk_contract.m) checks
the thermal constants and CPU/VM layouts against Apple's installed headers.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test
cargo test --release
cargo deny check
xcrun clang -fsyntax-only -x objective-c tests/sdk_contract.m
cargo build --release
otool -L target/release/headroom
```

## Releases

The [release workflow](.github/workflows/release.yml) runs when a `v*.*.*` tag
is pushed. The tag must match the version in `Cargo.toml`. After the checks
pass, GoReleaser publishes Apple Silicon and Intel archives with SHA-256
checksums, then updates `Casks/headroom.rb` in `blacktop/homebrew-tap`.
Prereleases leave the tap's stable cask unchanged.

The workflow needs a repository Actions secret named
`HOMEBREW_TAP_TOKEN`. It needs Contents read/write permission for
`blacktop/homebrew-tap`. The workflow uses GitHub's built-in token for this
repository's release.

For later releases, `just bump` increments the patch version in `Cargo.toml`
and `Cargo.lock`. Use `just bump minor`, `just bump major`, or
`just set-version 0.2.0-rc.1` when needed. Review and commit those changes, then
push the release commit.

To publish the current version:

```sh
just check
just tag
just publish
```

`just tag` requires a clean worktree and creates an annotated `vVERSION` tag.
`just publish` creates that tag if needed, then pushes only that tag to
`origin`.

To check the packaging locally without publishing anything:

```sh
just snapshot
```

## License

MIT. See [LICENSE](LICENSE).
