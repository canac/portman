export LLVM_PROFILE_FILE=target-coverage/coverage/%p-%m.profraw
export RUSTFLAGS=-Cinstrument-coverage

rm -rf target-coverage/coverage
cargo test --target-dir ./target-coverage
grcov ./target-coverage/coverage/ --source-dir . --binary-path ./target-coverage/debug/ --output-type html --branch --ignore-not-existing --output-path ./target-coverage/coverage/
