# AI Limits (study-233 fork)

A native Windows 11 taskbar panel for **Codex weekly remaining quota**: a ring, a percentage and a Codex label. This fork adds Simplified Chinese/English switching, live system-proxy support and appearance controls, and removes the desktop overlay.

Based on [napxlexn/ailimits](https://github.com/napxlexn/ailimits). This is an independently maintained fork, not an upstream release. Original authorship and GPL-3.0-or-later licensing are preserved.

[简体中文](README.zh-CN.md) · [Releases](https://github.com/study-233/ailimits/releases) · [Configuration](docs/en/CONFIG.md)

## Download and install

Download the Windows 11 x64 assets from [this fork's latest release](https://github.com/study-233/ailimits/releases/latest):

- **AiLimits-Setup-0.1.0.exe** — per-user installer, with the app, authentication utility, English/Chinese instructions and license texts. Installer-managed copies support automatic updates.
- **AiLimits-Portable-0.1.0.zip** — extract and run `ailimits.exe`; download future versions manually.
- **SHA256SUMS.txt** — compare the expected hash with `Get-FileHash <download> -Algorithm SHA256` in PowerShell.

Versioning restarts at **v0.1.0**, the first formal release of this fork. **Install it manually if you ran a previous 0.6.4 development build**: the updater does not treat a lower version number as an upgrade. The installer retains the existing installation directory, installation identity and configuration path, so it replaces an existing installation rather than installing alongside upstream. Existing configuration remains compatible.

For a first installation, PowerShell can download and verify the latest installer:

```powershell
irm https://raw.githubusercontent.com/study-233/ailimits/master/install.ps1 | iex
```

This fork is not published to winget, Scoop, Chocolatey or Microsoft Store. Upstream packages from those channels do not include these changes.

## Use the panel

1. Sign in through the relevant official CLI, then run `ailimits.exe`. The app reads existing credentials without refreshing or rotating official CLI tokens. Provider integrations include Claude, OpenAI Codex, GitHub Copilot and Google Antigravity; the taskbar and tray indicator focus on Codex weekly quota.
2. The bright arc shows **remaining** weekly quota: 100% is a full ring; 0% leaves only the base ring. Session quota never substitutes for missing weekly quota.
3. Hover to see quota status and the weekly reset time in local time. Missing usable data shows `—`, estimates use `≈`, and data older than five minutes is dimmed. An expired timestamp alone does not confirm a quota reset.
4. Right-click to choose providers, language, network proxy, appearance, taskbar panel or tray mode. The desktop overlay is removed. Lack of taskbar space never switches to tray automatically.

The panel follows taskbar auto-hide, full-screen visibility, Windows light/dark themes and display scaling. Automatic placement seeks free space and keeps the previous position if detection fails or space is insufficient. Drag the ring or numbers horizontally to save a manual position; press Esc to cancel a drag. Manual positions may overlap taskbar icons.

## Appearance and position

**Appearance settings…** adjusts ring and number colors, sizes, offsets, ring thickness, number weight and symbol scale. A two-theme preview and the taskbar update immediately. **Save** persists changes; **Cancel** or closing the dialog restores the previous settings. Resetting appearance affects appearance only.

The percentage and Codex label form two rows. The label follows the number color and position. When space is tight, text is scaled to fit without changing the saved font size. Sizes and positions use DIP units and follow Windows display scaling.

**Lock position** prevents dragging and automatic movement and survives restarting. Unlocking keeps the current position. **Restore automatic position** unlocks and clears the manual position.

## Language and networking

**Language → Follow system / 简体中文 / English** applies immediately and persists. Automatic mode chooses Simplified Chinese for a Chinese Windows display language and English otherwise. Chinese text uses the installed Microsoft YaHei font; no font download is needed.

**Network proxy → Follow system / Direct** applies to all providers, credential validation and update checks/downloads. System mode follows the current user's Windows static-proxy settings and reqwest's environment-proxy rules (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` and lowercase equivalents). Windows static-proxy changes affect the next request without restarting. External environment-variable changes require restarting the app.

Direct mode ignores these proxies. Failed proxy connections do not fall back to direct connections. PAC/WPAD is not supported. TUN/VPN routing remains controlled by Windows. HTTPS certificate verification remains enabled.

## Configuration and authentication

Configuration lives at `%APPDATA%\AiLimits\config.toml`. Menu changes save automatically; restart after manual file edits. Missing new fields use defaults. See the [configuration reference](docs/en/CONFIG.md); its legacy desktop-widget options are retained only for compatibility.

`ailimits-auth.exe` uses the same language and proxy configuration:

```powershell
.\ailimits-auth.exe status
.\ailimits-auth.exe set copilot
.\ailimits-auth.exe set claude
.\ailimits-auth.exe set-usage-token codex
.\ailimits-auth.exe remove copilot
.\ailimits-auth.exe remove-usage-token codex
```

Secret input is hidden and stored in Windows Credential Manager. Usage tokens are validated before saving.

## Build and validate

Use Windows, Rust's MSVC toolchain and Visual Studio C++ Build Tools with the Windows SDK:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup component add --toolchain stable-x86_64-pc-windows-msvc rustfmt clippy
cargo +stable-x86_64-pc-windows-msvc fmt --all -- --check
cargo +stable-x86_64-pc-windows-msvc clippy --locked --all-targets -- -D warnings
cargo +stable-x86_64-pc-windows-msvc test --locked
cargo +stable-x86_64-pc-windows-msvc build --locked --profile release-min --bins
```

The binaries are in `target/release-min/`. Network tests use local mock servers and no real tokens. Release tags must match Cargo and installer versions. The release workflow creates a draft with the installer, portable archive and checksums; publish it only after checking the assets.

## Updates

Automatic updates use [study-233/ailimits Releases](https://github.com/study-233/ailimits/releases) exclusively and skip when no release exists. The installer must match GitHub's SHA-256 digest. Portable and development copies do not run the installer automatically.

## License and attribution

Copyright (C) 2026 napxlexn. Fork modifications by study-233.

Licensed under the [GNU General Public License, version 3 or later](LICENSE). Corresponding source is available in this repository and at each release tag. Preserve the license and copyright notices when redistributing. See [TRADEMARKS.md](TRADEMARKS.md) for the upstream name and logo policy. Third-party components retain their respective licenses.
