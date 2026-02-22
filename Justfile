# Build the project
build:
    cargo build

# Run all tests
test:
    cargo test

# Run a single test by name
test-one name:
    cargo test {{name}}

# Run Postgres integration tests (requires Docker)
test-db:
    cargo test --test postgres_round_trip -- --ignored

# Run with compiler warnings denied
check:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Create the Postgres database and schema
db-create:
    createdb history_gen 2>/dev/null || true
    psql -d history_gen -f sql/schema.sql

# Load JSONL data into Postgres (set DATADIR to output directory)
db-load datadir="output":
    psql -d history_gen -v datadir="'{{datadir}}'" -f sql/load.sql

# Run verification queries against loaded data
db-verify:
    psql -d history_gen -f sql/verify.sql
