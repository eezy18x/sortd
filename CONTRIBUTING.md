# Contributing to SORTD

Thanks for helping! SORTD handles people's files, so the bar is **reliability and safety first**.

## Ground rules

- Nothing may modify files outside `src-tauri/src/operations/`, and only for a user-approved, previewed plan.
- Never introduce silent overwrites, permanent deletion, or execution of scanned files.
- No telemetry, analytics or network calls.
- OS-specific code belongs in `src-tauri/src/filesystem/` behind the common interface.
- No fake data in the UI: every number must come from the index.

## Workflow

1. Open an issue for anything non-trivial before starting.
2. Branch from `main`, keep changes focused.
3. Run before pushing:
   ```bash
   pnpm format && pnpm lint && pnpm typecheck && pnpm test
   cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
   ```
4. Add tests for new behaviour — especially anything touching the filesystem (use temp dirs).
5. Update docs (`docs/`, `CHANGELOG.md`) when behaviour changes.
6. Open a pull request describing what changed, why, and how you tested it (and on which OS).

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for setup and conventions.

## Code style

- Rust: `rustfmt` + `clippy -D warnings`. Errors go through `AppError`; no `unwrap()` on user-influenced data.
- TypeScript: strict mode, ESLint, Prettier. Components never call `invoke` directly — use `src/services/api.ts`.
- UI: use the design tokens in `src/styles/globals.css` and the primitives in `src/components/ui/`.

## Reporting security issues

Please follow [SECURITY.md](SECURITY.md) — do not file public issues for vulnerabilities.
