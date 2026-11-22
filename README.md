# DiskScope

[![build](https://github.com/neonnathan/DiskScope/actions/workflows/build.yml/badge.svg)](https://github.com/neonnathan/DiskScope/actions/workflows/build.yml)

A fast, dark, native Windows disk-usage analyzer written in Rust (egui).
Find out what is actually eating your drive: folders, files and installed apps.

![DiskScope](assets/icon_preview.png)

## Install

One command, no admin rights:

```powershell
irm https://raw.githubusercontent.com/neonnathan/DiskScope/main/install.ps1 | iex
```

That fetches the latest release and runs it silently. The app lands in
`%LOCALAPPDATA%\Programs\DiskScope` with Start Menu + Desktop shortcuts and an
entry in *Settings → Apps → Installed apps*. Run the same command again to upgrade.

Prefer clicking? Download `DiskScope-Setup.exe` from
[Releases](https://github.com/neonnathan/DiskScope/releases/latest).
`DiskScope.exe` there is the portable build — double-click, no install, no admin,
no dependencies.

Uninstall from *Settings → Apps*, or:

```powershell
& "$env:LOCALAPPDATA\Programs\DiskScope\Uninstall.exe" --uninstall
```

Installer and uninstaller both accept `/S` for a fully silent run.

You can also point the app at a folder from the command line:

```
DiskScope.exe "D:\Games"
```

## Features

- **Treemap** of every folder, sized by real bytes; hover for details, double-click to drill in.
- **Sortable contents list** — size, % of folder, item count, type. Filter as you type.
- **Largest files** — the biggest individual files anywhere under the scanned root.
- **Apps** — installed programs from the Windows uninstall registry, with the real size of
  their install folder.
- **Recycle Bin delete** — safe cleanup with a confirmation; the app rescans afterwards.
- Read-only scanning: nothing is modified unless you explicitly delete something.
- Parallel walker, permission errors skipped, junctions/reparse points not followed.
- Everything local. No network, no telemetry.

## Performance

Measured on this machine — full `C:\`, 754 GB across 2.4M files: **~8 s wall,
peak 243 MB RAM** (12 walker threads). `C:\Windows` alone (178k files) takes ~6 s.

The walker is a `std::fs::read_dir` thread pool rather than a generic walk crate,
because on Windows the file size comes free with the directory enumeration while
a path-based `metadata()` call costs a handle open per file — that single call was
11 s of a 12.5 s walk over `C:\Windows`. Junctions and symlinked folders are
skipped so nothing is double-counted.

## Build

Requires Rust (MSVC toolchain).

```powershell
.\build.ps1
```

That kills any running copy (Windows locks the exe), builds the app into
`dist\DiskScope.exe`, then builds the installer around it into
`dist\DiskScope-Setup.exe`.

Manually:

```powershell
cargo build --release
Copy-Item target\release\diskscope.exe dist\DiskScope.exe -Force
cargo build --release --manifest-path installer\Cargo.toml
Copy-Item installer\target\release\diskscope-setup.exe dist\DiskScope-Setup.exe -Force
```

The installer embeds `dist\DiskScope.exe` at compile time, so the copy has to happen first.

## Testing

```powershell
.\test-install.ps1   # install -> verify shortcuts/registry/launch -> uninstall, then the silent path
.\bench.ps1 -Root "C:\"   # real scan: CPU, memory, elapsed time, screenshot
```

`test-install.ps1` drives the installer's message boxes by clicking their real
buttons (see `uiprobe.ps1`), so it runs unattended.

## Layout

| Path | What |
| --- | --- |
| `src/main.rs` | app state + UI |
| `src/scan.rs` | parallel walker (`std::fs` pool), folder arena |
| `src/treemap.rs` | squarified treemap layout |
| `src/apps.rs` | uninstall-registry app list |
| `src/theme.rs` | dark theme, fonts, formatting |
| `installer/` | self-contained Setup.exe / Uninstall.exe |
| `install.ps1` | one-line installer (latest release, silent) |
| `assets/make_icon.ps1` | regenerates `assets/icon.ico` |
| `build.ps1` | release build of app + installer |
| `test-install.ps1`, `uiprobe.ps1` | automated install/uninstall test |
| `bench.ps1`, `shot.ps1` | scan benchmark, window screenshots |

## License

MIT — see [LICENSE](LICENSE).
