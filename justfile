default:
    @just --list

# Format code + regenerate the public-API surface snapshot.
# The snapshot runner lives in the standalone apidoc/ package, so it is
# never built or run by plain `cargo test` or any CI job.
fmt:
    cargo fmt --all
    cargo test --manifest-path apidoc/Cargo.toml

# Regenerate the public-API surface snapshot only
api-doc:
    cargo test --manifest-path apidoc/Cargo.toml

# Verify the committed snapshot is current
api-doc-check:
    ZEN_API_DOC=check cargo test --manifest-path apidoc/Cargo.toml

# Run all tests
test:
    cargo test

# Run clippy over all targets and features
clippy:
    cargo clippy --all-features --all-targets -- -D warnings
