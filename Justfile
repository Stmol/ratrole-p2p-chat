log_dir := "target/debug/rathole-logs"

run:
    mkdir -p "{{log_dir}}"
    RATHOLE_LOG_FILE="{{log_dir}}/rathole-$(date +%Y%m%d-%H%M%S).jsonl" cargo run

alias r := run

dev:
    mkdir -p "{{log_dir}}"
    RATHOLE_STORAGE_PROFILE=dev RATHOLE_LOG_FILE="{{log_dir}}/rathole-dev-$(date +%Y%m%d-%H%M%S).jsonl" cargo run

alias d := dev

run-relay:
    mkdir -p "{{log_dir}}"
    RATHOLE_IROH_PATH_MODE=relay-only RATHOLE_LOG_FILE="{{log_dir}}/rathole-relay-$(date +%Y%m%d-%H%M%S).jsonl" cargo run

alias rr := run-relay
