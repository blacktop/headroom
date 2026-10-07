set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

# List available commands.
default:
    @just --list

# Format Rust source.
fmt:
    cargo fmt

# Check Rust formatting.
fmt-check:
    cargo fmt --check

# Run Clippy with the repository's strict lints.
lint:
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Run unit tests.
test:
    cargo test --locked

# Run tests with release optimizations.
test-release:
    cargo test --release --locked

# Check dependency licenses, advisories, and sources.
deny:
    cargo deny check

# Check the FFI contract against Apple's installed SDK.
sdk-check:
    xcrun clang -fsyntax-only -x objective-c tests/sdk_contract.m

# Run the checks required by the release workflow.
check: fmt-check lint test test-release deny sdk-check

# Build the optimized binary.
build:
    cargo build --release --locked

# Run headroom with optional CLI arguments.
run *args:
    cargo run --release --locked -- "$@"

# Print the Cargo package version.
version:
    @cargo metadata --locked --no-deps --format-version 1 | jq -er '.packages[] | select(.name == "headroom") | .version'

# Bump patch (default), minor, or major; edits Cargo.toml and Cargo.lock.
bump level="patch":
    cargo set-version --bump {{ quote(level) }}

# Set an exact version, including a prerelease such as 0.2.0-rc.1.
set-version version:
    cargo set-version -- {{ quote(version) }}

# Create an annotated version tag at HEAD; the worktree must be clean.
tag: _clean
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(cargo metadata --locked --no-deps --format-version 1 | jq -er '.packages[] | select(.name == "headroom") | .version')
    tag="v$version"
    if git show-ref --verify --quiet "refs/tags/$tag"; then
        tag_commit=$(git rev-parse --verify "refs/tags/$tag^{commit}")
        head_commit=$(git rev-parse --verify HEAD)
        if [[ "$tag_commit" != "$head_commit" ]]; then
            echo "Tag $tag already points to another commit" >&2
            exit 1
        fi
        echo "Tag $tag already points to HEAD"
    else
        git tag --annotate "$tag" --message "$tag"
        echo "Created $tag"
    fi

# Push the current version tag to origin, starting the release workflow.
publish: tag
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(cargo metadata --locked --no-deps --format-version 1 | jq -er '.packages[] | select(.name == "headroom") | .version')
    git push origin "refs/tags/v$version"

# Build release archives and a Homebrew cask without publishing.
snapshot:
    goreleaser check
    goreleaser release --snapshot --clean

[private]
_clean:
    #!/usr/bin/env bash
    set -euo pipefail
    worktree_status=$(git status --porcelain)
    if [[ -n "$worktree_status" ]]; then
        echo "Commit or discard your changes before tagging a release" >&2
        exit 1
    fi
