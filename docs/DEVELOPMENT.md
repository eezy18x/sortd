# Development

## Prerequisites

All platforms: **Rust** (stable, 1.85+, via [rustup](https://rustup.rs)), **Node.js 22+** and **pnpm 10**
(`corepack enable` or `npm i -g pnpm`).

### Windows

- Microsoft C++ Build Tools ("Desktop development with C++") **and a Windows 10/11 SDK**
  (`winget install Microsoft.WindowsSDK.10.0.26100` if missing — without it, linking fails with `kernel32.lib` errors).
- WebView2 runtime (preinstalled on Windows 10 1803+ and Windows 11).

### macOS

- Xcode Command Line Tools: `xcode-select --install`.

### Linux (Debian/Ubuntu)

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf
```

Fedora: `sudo dnf install webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel` and the
"C Development Tools and Libraries" group. Arch: `sudo pacman -S webkit2gtk-4.1 base-devel openssl appmenu-gtk-module librsvg`.

## Commands

```bash
pnpm install              # install frontend dependencies (uses pnpm-lock.yaml)
pnpm tauri dev            # run the desktop app with hot reload
pnpm tauri build          # build installers for the current platform

pnpm lint                 # ESLint
pnpm typecheck            # TypeScript
pnpm test                 # Vitest + React Testing Library
pnpm format               # Prettier

cd src-tauri
cargo test                # Rust unit + integration tests (temporary directories only)
cargo clippy --all-targets -- -D warnings
cargo fmt
```

`pnpm dev` alone serves the UI in a browser, but without the backend every screen shows
"The SORTD backend is not available" — that is expected.

## Where data lives

The index database is `sortd.db` in the OS application-data directory for `io.github.eezy18x.sortd`
(e.g. `%APPDATA%\io.github.eezy18x.sortd` on Windows, `~/Library/Application Support/io.github.eezy18x.sortd`
on macOS, `~/.local/share/io.github.eezy18x.sortd` on Linux). Deleting it resets SORTD; files are unaffected.

## Demo data and screenshots

Debug builds honour `SORTD_DATA_DIR`, which points the app at a separate index so your real one is never touched
(release builds ignore it). README screenshots are captured from the running app against a generated sample
folder, never a personal one:

```powershell
$env:SORTD_DATA_DIR = "C:\SORTD-Demo\data"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9223"   # Windows: enables DevTools for capture
pnpm tauri dev
```

## Adding a backend command

1. Implement the logic in a core module (no Tauri types) with tests.
2. Add a `#[tauri::command]` wrapper in `src-tauri/src/commands/mod.rs`.
3. Add its name to `src-tauri/src/commands/list.rs`, register it in `lib.rs`, and add `allow-<name>` to
   `capabilities/default.json` (a test fails if these drift apart).
4. Add a typed wrapper in `src/services/api.ts`.

## Testing rules

- Tests that touch the filesystem must use `tempfile` directories. Never point tests at real user folders.
- Test builds redirect "move to trash" into an isolated temp folder, so the real system trash is never used.
- Platform-specific behaviour gets `#[cfg(...)]` tests; CI runs them on Windows, macOS and Linux.

## Schema changes

Add a new numbered file to `src-tauri/migrations/` and append it to `MIGRATIONS` in `database/mod.rs`. Never edit a
migration that has shipped.

## Performance notes

- Scanning is a single streaming traversal; throughput is bounded by filesystem metadata speed. Memory grows with
  the number of *directories*, not files.
- Hashing uses a configurable worker pool (Settings → Duplicates); fewer threads are better on spinning disks.
- No formal benchmarks have been published yet. During development a real scan indexed roughly 36,000 files
  (235 GB) before it was interrupted; systematic benchmarks are on the roadmap.
