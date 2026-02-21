# Build the project
build:
    cargo build

# Run all tests
test:
    cargo test

# Run a single test by name
test-one name:
    cargo test {{name}}

# Run with compiler warnings denied
check:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check
