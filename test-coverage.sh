rm -rf target/coverage-enabled/coverage
LLVM_PROFILE_FILE=target/coverage-enabled/coverage/%p-%m.profraw RUSTFLAGS=-Cinstrument-coverage cargo test --target-dir=./target/coverage-enabled
grcov . -s ./target/coverage-enabled/coverage --binary-path ./target/coverage-enabled/debug/deps/ -t html --branch --ignore-not-existing -o ./target/coverage-enabled/coverage/
