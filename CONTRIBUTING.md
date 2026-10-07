# Contributing

## Build from source

You'll need macOS, stable Rust, and Apple's command-line tools. The commands
below work in zsh or Fish.

```sh
git clone https://github.com/blacktop/headroom.git
cd headroom
cargo build --release --locked
./target/release/headroom
```

To install the source build on your Cargo path:

```sh
cargo install --path . --locked
```

## Development checks

Install the development tools and run the checks used by CI:

```sh
brew install just cargo-deny
just check
just build
```

`just check` runs formatting, strict Clippy, debug and release tests,
dependency checks, and the SDK contract check. Run `just` to list the available
commands, or run the checks directly:

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --release --locked
cargo deny check
xcrun clang -fsyntax-only -x objective-c tests/sdk_contract.m
cargo build --release --locked
otool -L target/release/headroom
```

The release binary should link only to libSystem.

## Code layout

The [sampler](src/sample.rs) uses `getloadavg`, `sysctlbyname`,
`host_statistics`, `host_statistics64`, and the public thermal-pressure
notification. The
[verdict rule](src/rule.rs) takes plain samples, so its tests do not depend on
how busy the test machine is. The [SDK contract](tests/sdk_contract.m) checks
the thermal constants and CPU/VM layouts against Apple's installed headers.

## Releases

Install the release tools:

```sh
brew install cargo-edit goreleaser jq
```

`just bump` increments the patch version in `Cargo.toml` and `Cargo.lock`.
Use `just bump minor`, `just bump major`, or `just set-version 0.3.0-rc.1`
when needed.

```sh
just bump
just check
just snapshot
```

`just snapshot` builds the archives and Homebrew cask locally. Review the
version changes, commit them, and push the release commit. Then publish from a
clean worktree:

```sh
just publish
```

`just tag` creates an annotated `vVERSION` tag at `HEAD`. `just publish` creates
that tag if needed, then pushes only the tag to `origin`.

The [release workflow](.github/workflows/release.yml) requires the tag to match
`Cargo.toml`. After the checks pass, GoReleaser publishes Apple Silicon and
Intel archives with SHA-256 checksums, then updates `Casks/headroom.rb` in
`blacktop/homebrew-tap`. Prereleases leave the tap's stable cask unchanged.

The workflow needs a repository Actions secret named `HOMEBREW_TAP_TOKEN`
with Contents read/write permission for `blacktop/homebrew-tap`. GitHub's
built-in token handles this repository's release.
