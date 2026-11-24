# Security Policy

SORTD manages people's personal files. We take that seriously, and we do not claim SORTD is perfectly secure.

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Use GitHub's
[private vulnerability reporting](https://github.com/eezy18x/sortd/security/advisories/new) for this repository.
Include steps to reproduce, affected platform(s) and version. You should receive an acknowledgement within a week.

## Supported versions

SORTD is pre-1.0. Only the latest release (or `main`) receives fixes.

---

## Threat model

### Assets

1. The user's files (integrity first, then confidentiality).
2. The metadata index (paths and names can be sensitive).
3. The user's trust that SORTD will only do what it previewed.

### Trust boundaries

```
 ┌───────────────────────── Webview (untrusted-ish UI) ─────────────────────────┐
 │ React app. No filesystem, shell, HTTP or plugin APIs are granted.           │
 │ Can only invoke SORTD's 45 explicitly allowlisted commands (capabilities/). │
 └──────────────────────────────────┬───────────────────────────────────────────┘
                                    │ IPC (validated, typed)
 ┌──────────────────────────────────▼───────────────────────────────────────────┐
 │ Rust backend: validates every input, authorizes every path, owns all I/O.   │
 └──────────────────────────────────┬───────────────────────────────────────────┘
                                    │
            Filesystem (contents and names are untrusted input)
```

Even though the UI is bundled with the app, the backend treats IPC input as untrusted, so a UI bug (or an XSS
via a malicious file name) cannot widen what SORTD may touch.

### Threats and mitigations

| Threat | Mitigation |
|---|---|
| UI asks backend to read/modify an arbitrary path | Locations are authorized **only** via a native folder dialog opened by the backend; the webview cannot add a location by sending a string. Every path is checked against authorized locations (`security::authorize_path`). |
| Path traversal (`..`, relative paths, NUL bytes) | Lexical normalization rejects `..`, relative paths and NUL; names are validated per platform (Windows reserved names and characters included). |
| Symlink / junction escape (a link inside an authorized folder pointing elsewhere) | Scanner never follows links or junctions. Before any operation the nearest existing ancestor is canonicalized and must remain inside the authorized root. Reads use `O_NOFOLLOW` / `FILE_FLAG_OPEN_REPARSE_POINT`. Symlinks are never copied or opened. |
| TOCTOU between preview and execution | Each item is re-authorized and re-checked (size + mtime snapshot, destination still free) immediately before execution; batches are claimed atomically so they cannot run twice; plans do not survive a restart. |
| Silent overwrite | No-replace rename primitives on every platform. On Linux filesystems that don't support `RENAME_NOREPLACE` (some network/FUSE filesystems) SORTD falls back to check-then-rename, which has a small race window. |
| Partial or corrupted copies | Copy to temp file → fsync → BLAKE3 verification → no-replace rename. Cross-drive moves delete the source only after verification, and roll back the copy if the source cannot be removed. |
| Data loss from "delete" | No permanent deletion; trash only. Authorized roots themselves cannot be trashed. |
| Executing untrusted files | SORTD never executes scanned files. "Open" refuses executables, scripts, installers, shortcuts and symlinks. Type detection reads ≤ 8 KB of magic bytes. |
| Untrusted MIME/extension data | Extension-derived MIME types are labelled as hints; content signatures can override them; neither ever triggers execution. |
| Malicious file names in the UI | React escapes all text; strict CSP (`script-src 'self'`, no remote origins, `connect-src` limited to IPC). |
| Cloud placeholder abuse / unwanted downloads | OneDrive/iCloud placeholders are detected and never read. |
| SQL injection via search | All values are bound parameters; FTS queries are quoted as phrase literals. Tested with hostile inputs. |
| Sensitive data in logs | Activity entries store actions, paths and counts — never contents. The index never stores contents. |
| Network exfiltration | SORTD makes no network requests; CSP forbids remote connections. |

### Hardening trade-offs (documented honestly)

- **`freezePrototype` is disabled.** Tauri's option to freeze `Object.prototype` breaks the charting library
  (it assigns `valueOf` on its own objects). The webview has no privileged APIs beyond SORTD's validated
  commands, so the residual risk is limited to prototype-pollution bugs within the UI itself.
- **`style-src 'unsafe-inline'`** is allowed because the UI library sets inline styles (e.g. virtualized row
  positions). Scripts remain restricted to `'self'`.
- **The index is not encrypted.** It lives in the user's application-data folder with the user's permissions.
  Anyone with access to the user account can read file names and paths from it. Full-disk encryption is the
  recommended protection.
- **Hidden-file and permission semantics differ by platform**; see `docs/PLATFORM_SUPPORT.md`.

### Out of scope

- An attacker who already controls the user account or the SORTD binary.
- Malicious modifications to the SQLite index by another local process running as the same user
  (paths from the index are still re-authorized before any operation).
