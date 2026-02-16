# Release Skill
1. Run `make test` in cpp and ensure all pass
2. Run `cargo test` in rust and ensure all pass
3. Run `cargo clippy --all-targets -- -D warnings` in rust and fix all warnings
4. Update version in all Cargo.toml files in rust
5. Update README.md, cpp/README.md, and rust/README.md with latest details
6. Commit with message: "Release vX.Y.Z: <summary>"
EOF
