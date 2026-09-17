# Changelog

Notable, user-visible changes. Dates are release dates.

## 0.3.0 - 2026-09-17

### 中文

- 新增完整原生配额面板：5 小时额度、每周额度、重置机会、额度历史及近 30 天 Token 活动，保留 Windows 明暗主题和轻量绘制。
- 面板内容与任务栏外观独立设置；支持模块显隐、拖动及上下按钮排序、实时预览、恢复默认、保存和取消，兼容旧配置。
- 额度模块补齐时间剩余、使用节奏、重置倒计时和本地重置时间；缓存或不完整数据不参与节奏判断。
- 按账号保存最近 30 天真实额度观测；图表支持独立详情窗口，区分采样空白、重置周期、缺失日期和零用量。
- 按需通过本机 Codex App Server 获取重置机会和每日 Token 数据，校验账号、合并请求并缓存五分钟；读取结束或超时后清理辅助进程。
- 扩展接口不可用、字段缺失或账号无法匹配时明确显示状态；不会补造历史，也不会使用重置机会。
- 移除任务栏和托盘悬停提示；更新中英文操作说明，并将新面板截图随安装包和便携包提供。

### English

- Added a native quota panel with five configurable modules: 5-hour quota, weekly quota, reset opportunities, quota history and daily token activity.
- Panel visibility and ordering are independent of taskbar appearance, with drag/button reordering, live preview, defaults, Save/Cancel and compatible configuration migration.
- Added time remaining, usage pace and local reset times, account-isolated 30-day observations, and native chart detail windows with explicit data gaps.
- Reads optional Codex App Server data on demand with account verification, five-minute caching, request coalescing, timeouts and helper-process cleanup. Unsupported or missing data remains clearly unavailable.
- Removed taskbar/tray hover tooltips and bundled updated bilingual documentation and panel screenshots. Existing installation identity and settings are preserved during upgrades.

## 0.2.0 — QuotaBar

- 原生 Fluent 设置与菜单统一明暗主题；修复初始绘制、子菜单和滚动重绘问题。
- 托盘与任务栏共享图形、周期、颜色和悬停摘要，提供紧凑数字及状态标记。
- Codex `pro`／`prolite` 账号的悬停提示聚焦周额度，保留原始数据与图形选择。
- Unified Fluent settings and menus, repaired painting/scrolling, and synchronized tray/panel appearance and hover summaries.

- 更名为 QuotaBar，采用原创双色圆环图标；沿用安装标识、配置和凭据，支持覆盖升级。
- 任务栏与托盘支持圆环／横条，以及每周／5 小时／双周期剩余额度；双圆环外圈为 5 小时，内圈为每周。
- 重做原生外观设置：样式选择、配色、尺寸预设、深浅预览和折叠高级设置；保存生效，取消恢复。
- 保留任务栏自动隐藏、全屏隐藏、拖动锁定与 DPI 缩放，不新增网络轮询或 UI 框架。
- Renamed to QuotaBar with an original two-ring icon and compatible in-place upgrades.
- Added ring/bar presentation and weekly/5-hour/both-period selection, shared by panel, tray and preview.
- Redesigned native appearance settings with size presets, separate theme previews and collapsible advanced controls.
- Installer and portable assets now use the QuotaBar prefix; installed 0.1.0 copies can update through the same repository.

## 0.1.0 - 2026-09-17

First official release of the **study-233 fork**, based on upstream 0.6.4. Earlier entries below describe upstream releases.

### Installer revision - 2026-09-17

- 安装器仅保留简体中文和 English，补齐中文向导与安装选项。版本仍为 0.1.0；仅替换安装包及其 SHA-256，便携包和程序文件不变。旧安装包需手动重新下载，同版本修订不触发自动更新。
- The revised installer offers only Simplified Chinese and English. The version remains 0.1.0; only the installer and its checksum change. Portable and application binaries are unchanged. Download the installer again manually; this same-version revision does not trigger automatic updates.

### 中文

- 首个正式版：任务栏圆环与百分比显示 Codex 每周剩余额度，并显示 Codex 标签、数据状态及本地时间的重置提示。
- 支持简体中文／English／跟随系统，支持实时 Windows 静态系统代理和直连切换。
- 新增圆环与数字独立外观设置、深浅色实时预览、保存／取消、拖动定位、位置锁定与恢复自动位置。
- 移除桌面悬浮窗；保留任务栏面板和手动选择的托盘模式，不因空间不足自动切换。
- 提供 Windows 11 x64 安装包、便携 ZIP、中英文说明和 SHA-256 校验文件；自动更新仅使用本 fork，并校验安装器摘要。
- **首次升级需手动安装：** 本 fork 从 0.1.0 重新编号，旧 0.6.4 开发构建不会自动升级到更小的版本号。安装路径、安装标识和配置沿用原项目，安装版会替换已有安装；便携版以后继续手动更新。

### English

- Codex weekly remaining quota ring, percentage, provider label and reset tooltip on the Windows 11 taskbar.
- Live Simplified Chinese/English switching and Windows static system-proxy/direct connection modes.
- Independent ring and number styling, live light/dark previews, Save/Cancel, dragging, position locking and automatic placement reset.
- Desktop overlay removed; taskbar panel and manually selected tray mode remain.
- Windows x64 installer, portable ZIP, bilingual documentation and SHA-256 checksums. Installer updates use this fork and require a verified digest.
- **Install this first release manually:** versioning restarts at 0.1.0, so previous 0.6.4 development builds will not offer it as an update. The installer replaces an existing installation and preserves configuration compatibility. Portable copies continue to update manually.

## 0.6.4 - 2026-09-12

### Fixed

- **The compact widget's name column fits every provider name.** It was 48px
  wide and "Antigravity" at the compact size runs 52.6px, so the name reached
  into the gap before its bar. The column is 56px now, in the app and in the
  site's renderer; every width step still lands on the same tier.

## 0.6.3 - 2026-09-11

### Added

- **A Microsoft Store package.** The same `ailimits.exe` the installer ships,
  packaged as MSIX (`installer/msix/`). Inside the package the app knows it
  is packaged and leaves two things to the Store: the tray-icon registry
  write (a packaged process's HKCU writes never reach Explorer, and the
  Store policy asks that settings not change without the user's say) and
  the silent self-update (the package directory is read-only; the Store
  updates it). Start at sign-in is the package's startup task. Installer,
  Scoop and portable copies behave exactly as before.
- **A privacy page** on the site, written from the code: which logins the
  widget reads and where each lives, which four provider hosts it sends
  them to, what it stores, and what each uninstall route leaves behind.

### Changed

- **The published footprint is the August audit.** READMEs, both
  architecture documents and the Chocolatey description quoted the July
  run (0.024% of one core, 61 MB); the 2026-08-23 audit of the same window
  reads 0.005% and 37 MB.
- **Chocolatey and winget metadata** point at the site as the project's
  home, and the Chocolatey icon comes from a CDN pinned to the release tag,
  as the repository's moderator asked.

## 0.6.2 - 2026-08-21

### Added

- **Terminal installs.** A one-line PowerShell install
  (`irm .../install.ps1 | iex`) that verifies the installer against the
  release digest before running it, a Chocolatey package, and a Scoop
  manifest. A portable zip (the two exes plus the licence texts, no
  installer) is now attached to every release.

### Changed

- **A copy not managed by the installer no longer self-updates.** Running
  from a Scoop directory, an unpacked zip or a dev build, the auto-updater
  used to install a second copy into the regular install directory and leave
  the running one stale. It now recognises that the copy is not the one the
  installer put on disk, logs the available version, and leaves the update to
  whatever installed it. Installer copies, including those in a custom
  directory, update exactly as before.

## 0.6.1 - 2026-08-20

### Added

- **Widget width.** The rows layout can be shown at 100%, 75% or 50% of its
  natural width, from the context menu — useful when the widget shares a corner
  with something else.
- **Column arrangement.** With vertical progress bars the providers can sit side
  by side or stacked, flipping the widget between a wide, short shape and a
  narrow, tall one.
- **The taskbar panel can now follow a display other than the primary one.**
  On a multi-monitor setup where Windows shows the taskbar on more than one
  display, **Indicator → Display** picks which taskbar the panel attaches to.
  It falls back to the primary taskbar if the chosen display is later
  disconnected.
- **`panel_offset_x` / `panel_offset_y`** are new config-only settings that
  nudge the taskbar panel's position in pixels — a repair tool for unusual
  taskbar layouts, not exposed in the menu.

### Changed

- **The tray icon is now two rings.** The busiest provider is the outer ring,
  the runner-up the inner one, each filling clockwise from 12 o'clock. It is
  monochrome and inks itself in the system taskbar theme, so it reads on a light
  or a dark bar. A single provider shows the outer ring alone, so nothing moves
  when a second one starts reporting.
- **The panel's hover tooltip matches the shell's.** Size, padding, corner
  radius, text weight and the drop shadow were measured against a real Windows
  tooltip instead of estimated, so it now sits alongside the system's own
  tooltips rather than merely near them.

### Fixed

- **90% no longer looks like 100% in the tray.** A round line cap paints past
  the end of an arc — nearly a tenth of the circumference on the inner ring — so
  a nearly-full gauge closed completely and could not be told from a full one.
  That overhang is now subtracted, and only a true 100% closes a ring.
- **The panel comes back after the Start menu closes.** It could otherwise stay
  gone for the rest of the session, with the tray icon standing in until the app
  was restarted.
- **The panel no longer jumps to another display when Start opens**, and it
  settles into place after the taskbar's slide animation instead of mid-flight.
- **The panel survives an Explorer restart** — its taskbar watch re-points
  itself at the new bars instead of tracking windows that no longer exist.
- **A panel parked by a fullscreen app can be revived without restarting the
  app**, by switching its display.
- **The taskbar panel no longer overlaps the clock on taskbars that expose no
  notification area.** Some secondary Windows 11 taskbars have no
  `TrayNotifyWnd` to measure, so the panel now reserves a fixed width for the
  clock on those bars instead of assuming it can use the full edge.
- **A failed taskbar panel present is now logged instead of vanishing
  silently**, so a stuck or missing overlay leaves a diagnosable trace.
- **Antigravity showed a full quota while the weekly pool was spent.** The
  widget could not read the account's Code Assist project id, and Google
  answers project-less quota requests with a default view where every bucket
  reads full — so an exhausted Gemini allowance rendered as 0% used. AI Limits
  now identifies itself the way Antigravity's own client does, reads the shared
  quota pools ("Gemini Models", "Claude and GPT models") that Antigravity
  meters today, and reports an honest error instead of a quota it cannot
  verify.
- **A spent weekly limit now shows everywhere, not just on the widget.** The
  taskbar panel, the tray icon, both tooltips and the threshold notifications
  kept reporting the 5-hour session gauge — which reads low precisely when a
  spent weekly cap has already blocked new sessions. Every surface now shares
  one rule, and each limit window is classified explicitly, so Claude's Opus
  and Sonnet weekly pools count as well.
- **A failed update no longer closes the app.** If the installer handoff could
  not start, AI Limits exited anyway without updating; it now stays on the
  current version and logs the reason. The handoff also uses the absolute
  system command interpreter and relaunches the installed binary, so a copy
  running from another folder can no longer reinstall the same update forever.
- **Settings changed immediately before quitting are no longer lost** — the
  configuration is written synchronously as the window closes.

## 0.6.0 - 2026-07-25

### Added

- **Automatic updates.** AI Limits now checks GitHub for new releases in the
  background and installs them silently, then restarts itself — no manual
  download. Each installer is verified against the release's published SHA-256
  before it runs; a mismatch is refused. Toggle it any time from the context
  menu (**Automatic updates**, on by default); the choice is saved to
  `config.toml` as `auto_update`.

### Fixed

- **Claude: a maxed weekly limit now shows on the bar itself, not only on
  hover.** When the weekly allowance was exhausted the widget kept the bar on
  the 5-hour session window — which reads low (or drops out) exactly when the
  weekly cap has already blocked new sessions — so the overlay looked far from
  full until you hovered. The bar now surfaces the weekly window whenever it is
  exhausted, matching how Codex already behaved.

## 0.5.3 - 2026-07-23

### Initial public release

- Published AI Limits as free software under the GNU General Public License,
  version 3 or later (`GPL-3.0-or-later`).
- Added a separate trademark policy for the AI Limits name and logo so
  modified builds cannot be presented as official project releases.
- Included the license and trademark policy in the source archive and Windows
  installer.

### Providers and interface

- Shows live usage limits for Claude, OpenAI Codex, GitHub Copilot, and Google
  Antigravity using the authentication sources already stored by official CLI
  tools.
- Provides a configurable floating Windows 11 widget plus tray and taskbar
  indicators, stale-data explanations, reset countdowns, and optional
  burn-rate forecasts.
- Stores optional user-supplied credentials in Windows Credential Manager and
  sends no telemetry.

### Reliability and maintenance

- Bounds external `gh` and user-hook processes with timeouts.
- Serialises and coalesces configuration writes so rapid changes cannot be
  persisted out of order or grow an unbounded queue.
- Declares Rust 1.86 as the minimum supported toolchain and excludes unused
  clipboard-image support from the Windows dependency tree.
- Keeps the Windows build free of the XML dependency covered by published
  RustSec advisories and documents the remaining platform-specific audit triage.
- Ships with formatting, Clippy, tests, RustSec audit, release build, WinGet
  validation, and reproducible resource-audit tooling.
