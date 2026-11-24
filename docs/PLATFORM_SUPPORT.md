# Platform Support

| Platform | Architectures | Packages | Verification |
|---|---|---|---|
| Windows 10/11 | x64 | MSI, NSIS | Developed, tested and run locally |
| macOS 11+ | Apple Silicon, Intel | DMG | CI build + unit/integration tests |
| Linux | x64 | AppImage, deb, rpm | CI build + unit/integration tests (Ubuntu 22.04) |

macOS and Linux have **not** been exercised interactively by the maintainers yet. CI runs the full Rust test
suite (including filesystem tests) on each platform; reports from real use are very welcome.

## Behaviour by platform

All of these are implemented in `src-tauri/src/filesystem/` behind one interface.

| Concern | Windows | macOS | Linux |
|---|---|---|---|
| Hidden files | `FILE_ATTRIBUTE_HIDDEN` or dot-prefix | `UF_HIDDEN` flag or dot-prefix | dot-prefix |
| Cloud placeholders (never read) | OneDrive: `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN`, `OFFLINE` | iCloud: `SF_DATALESS` | — |
| Links never followed | Symlinks, junctions, other non-cloud reparse points | Symlinks | Symlinks |
| No-replace rename | `MoveFileExW` (no `REPLACE_EXISTING`, no `COPY_ALLOWED`) | `renamex_np(RENAME_EXCL)` | `renameat2(RENAME_NOREPLACE)` via raw syscall; check-then-rename fallback on unsupported filesystems |
| Same-volume detection | Drive letter / UNC share of the canonical path | `st_dev` | `st_dev` |
| Path comparison | Case-insensitive | Case-insensitive (conservative for APFS) | Case-sensitive |
| Name rules | Reserved names (`CON`, `NUL`, `COM1`…), `<>:"\|?*`, no trailing dot/space | No `/` or NUL | No `/` or NUL |
| Creation time | Supported | Supported | Depends on filesystem/kernel (`statx`); shown as unavailable otherwise |
| Disk capacity | `GetDiskFreeSpaceExW` | `statvfs` | `statvfs` |
| "Unsafe to open" | `.exe .msi .bat .cmd .ps1 .lnk .url .vbs .js .hta .reg …` | Executable bit, `.app .command .pkg .sh …` | Executable bit, `.desktop .AppImage .sh …` |
| Trash | Recycle Bin | Finder Trash | freedesktop.org trash |

### Default exclusions

- **Windows:** `%SystemRoot%`, Program Files (both), ProgramData, `$Recycle.Bin`, System Volume Information,
  Recovery, PerfLogs, Config.Msi, `%USERPROFILE%\AppData`.
- **macOS:** `/System`, `/Library`, `/private`, `/dev`, `/cores`, `/bin`, `/sbin`, `/usr`, `/opt/homebrew`,
  `~/Library`, `~/.Trash`; names `.Spotlight-V100`, `.fseventsd`, `.Trashes`, …
- **Linux:** `/proc`, `/sys`, `/dev`, `/run`, `/boot`, `/usr`, `/bin`, `/sbin`, `/lib*`, `/etc`, `/var`, `/snap`,
  `/tmp`, `~/.cache`, `~/.local/share/Trash`.

If you pick one of these locations yourself, it is scanned: picking it is explicit authorization.

### macOS privacy permissions

macOS restricts Desktop, Documents, Downloads, removable and network volumes. The first time SORTD reads them,
the system asks for permission. If access is denied, the scan reports permission errors and marks folders as
incomplete. Grant access in **System Settings → Privacy & Security → Files and Folders** (or Full Disk Access).
SORTD is not sandboxed.

### Linux

Requires WebKitGTK 4.1. Mounted filesystems below a scan root are skipped unless "Cross into other drives and
mounts" is enabled. Unix permission errors are reported, never escalated.

### Known limitations

- File names that are not valid Unicode (possible on Linux) are skipped and reported, rather than stored lossily.
- Moving folders across drives and copying folders are not supported yet (files only).
- Items moved to the trash are restored through the OS trash, not from SORTD.
- Hard links are indexed as separate files and will appear as duplicates of each other.
- Cached hashes are invalidated by size and modification time (like `rsync`'s default). A tool that rewrites a
  file's content while restoring the same size *and* mtime can leave a stale cached hash; rescanning after the
  mtime changes, or resetting the index, fixes it. Deletions through SORTD still re-check size and mtime first.
