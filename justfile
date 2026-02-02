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

# Run the full sensor demo (original)
demo:
    #!/usr/bin/env bash
    set -e
    trap 'kill $(jobs -p) 2>/dev/null; wait' INT TERM
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

# Run the drone demo: relay + drone, then use `just control` to connect
drone-demo:
    #!/usr/bin/env bash
    set -e
    trap 'kill $(jobs -p) 2>/dev/null; wait' INT TERM
    DRONE_ID="drone-$(uuidgen | head -c 8)"
    echo "Starting relay on :4443..."
    moq-relay dev/relay.toml &
    sleep 2
    echo "Starting drone with ID: $DRONE_ID"
    echo "  Run 'just control' in another terminal to send commands."
    DRONE_ID=$DRONE_ID cargo run --bin drone
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

# Start a drone (generates random ID if DRONE_ID not set)
drone *args:
    cargo run --bin drone {{ args }}

# Start the controller (auto-discovers all drones)
control:
    cargo run --bin controller
