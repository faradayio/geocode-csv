# This is a `justfile`, which is sort of like a less crufty makefile
# It's processed using <https://github.com/casey/just>, which you can
# install using `cargo install -f just`
#
# To see a list of available commands, run `just --list`

# Look up our version using cargo.
VERSION := `cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "geocode-csv") | .version'`

# Print the current version.
version:
    @echo "{{VERSION}}"

# Check to make sure that we're in releasable shape.
check:
  cargo fmt -- --check
  cargo deny check
  cargo clippy -- -D warnings
  cargo test --all

# Check to make sure our working copy is clean.
check-clean:
    git diff-index --quiet HEAD --

# Test against real network services, including Smarty.
test-full:
  cargo test --all -- --include-ignored

# Release via crates.io and GitHub.
release: check check-clean
  cargo publish
  git tag v{{VERSION}}
  git push
  git push --tags