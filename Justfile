log_dir := "target/debug/rathole-logs"
binary := "target/debug/rathole"
signing_identity := env_var_or_default("APPLE_CODESIGN_IDENTITY", "Rathole Local Development")
signing_identifier := env_var_or_default("RATHOLE_CODESIGN_IDENTIFIER", "org.rathole.rathole.dev")

run:
    mkdir -p "{{log_dir}}"
    RATHOLE_LOG_FILE="{{log_dir}}/rathole-$(date +%Y%m%d-%H%M%S).jsonl" cargo run

alias r := run

dev:
    cargo build
    codesign --force --timestamp=none --sign "{{signing_identity}}" --identifier "{{signing_identifier}}" "{{binary}}"
    mkdir -p "{{log_dir}}"
    RATHOLE_STORAGE_PROFILE=dev RATHOLE_LOG_FILE="{{log_dir}}/rathole-dev-$(date +%Y%m%d-%H%M%S).jsonl" "{{binary}}"

alias d := dev

run-relay:
    mkdir -p "{{log_dir}}"
    RATHOLE_IROH_PATH_MODE=relay-only RATHOLE_LOG_FILE="{{log_dir}}/rathole-relay-$(date +%Y%m%d-%H%M%S).jsonl" cargo run

alias rr := run-relay
