default:
    @just --list

# Format code + regenerate the public-API surface snapshot
fmt:
    cargo fmt --all
    cargo test --test public_api_doc

# Regenerate the public-API surface snapshot only
api-doc:
    cargo test --test public_api_doc

# Verify the committed snapshot is current (what CI runs)
api-doc-check:
    ZEN_API_DOC=check cargo test --test public_api_doc

# Run all tests
test:
    cargo test

# Run clippy over all targets and features
clippy:
    cargo clippy --all-features --all-targets -- -D warnings
