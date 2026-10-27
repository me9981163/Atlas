# Contributing to Atlas

Thank you for your interest in contributing!

## Development
1. Clone the repo
2. Run tests: `cargo test --workspace`
3. Run checks: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`

## Guidelines
- Follow the rules in [AGENTS.md](AGENTS.md)
- One change at a time, keep it simple
- Add tests for new behavior
- Comments explain *why*, not *what*
- Never guess in resolution logic

## Pull requests
- Use Conventional Commits style for commit messages
- Include tests
- Update docs if behavior changes
