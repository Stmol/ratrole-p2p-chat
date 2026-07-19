use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;

#[test]
fn version_command_starts_and_writes_to_stdout() {
    cargo_bin_cmd!("rathole")
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("rathole"));
}
