# Release Skill
1. Run `make test` in cpp and ensure all pass
1. Run `cargo test` in rust and ensure all pass
2. Run `cargo clippy -- -D warnings --all-targets` in rust and fix all warnings
3. Update version in all Cargo.toml files in rust
4. Update README.md, cpp/README.md, and rust/README.md with latest details
5. Commit with message: "Release vX.Y.Z: <summary>"
EOF
