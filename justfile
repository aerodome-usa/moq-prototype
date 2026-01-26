default:
    @just --list

build:
    cargo build

run:
    cargo run

clean:
    cargo clean

fmt:
    cargo fmt

check:
    cargo fmt --check
    cargo clippy

# Run the full demo
demo:
    #!/usr/bin/env bash
    set -e
    echo "Starting relay on :4443..."
    moq-relay dev/relay.toml &
    sleep 2
    echo "Starting publisher..."
    cargo run --bin publish &
    sleep 2
    echo "Starting subscriber..."
    cargo run --bin subscribe &
    echo ""
    echo "Demo running. Press Ctrl+C to stop."
    wait

# Start the relay server
relay *args:
    moq-relay dev/relay.toml {{ args }}

# Publish sensor data to the relay
publish:
    cargo run --bin publish

# Subscribe to sensor data from the relay
subscribe:
    cargo run --bin subscribe
