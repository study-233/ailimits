# QuotaBar configuration

Configuration remains at `%APPDATA%\AiLimits\config.toml` for upgrade compatibility. The internal executable, credentials and installation identity retain their existing names. Restart after manually editing this file.

## Display and appearance

```toml
[general]
indicator = "panel_rows" # panel_rows / tray
language = "auto" # auto / zh-CN / en
panel_locked = false
# panel_position_x = 420 # DIP from the selected taskbar's left edge

[appearance]
display_style = "rings" # rings / bars
tray_display = "auto" # auto / ring / number / ring_state
periods = "weekly" # weekly / five_hours / both
ring_color = "#FF3774" # weekly color (legacy key retained)
session_color = "#30C8F0" # 5-hour color
number_color = "auto" # auto / #RRGGBB
ring_size = 28 # 12–44 DIP
ring_thickness = 4 # 1–12; less than half the ring diameter
ring_x = 6 # 0–300 DIP
ring_y = 0 # −20–20 DIP from vertical center
number_size = 20 # 10–40 DIP; dual rows fit at up to 14 DIP
number_x = 41 # ring-mode text X, 0–300 DIP
number_y = 0 # −20–20 DIP, clamped inside each row
number_weight = "semibold" # regular / semibold / bold
symbol_percent = 60 # 40–100
bar_width = 88 # 32–200 DIP
bar_thickness = 8 # 2–16 DIP
bar_x = 112 # minimum bar X, 0–300 DIP; text reserves space before it
bar_y = 0 # −20–20 DIP from each row's center
bar_number_x = 8 # bar-mode text X, 0–300 DIP

[network]
proxy_mode = "system" # system / direct
```

All progress means **remaining** quota. Dual views have a fixed order: 5 hours then weekly; concentric rings use 5 hours outside and weekly inside. A missing selected window stays missing, even if another window is available. The duration returned by the provider determines the window; older responses without duration retain the existing parser compatibility behavior.

The tray follows the taskbar's selected shape, periods and period colors, with geometry adapted to 16/20/24/32-pixel icons. `ring` ("Follow style") draws the selected ring or bar; `auto` and `ring_state` ("Style + state") also show a corner marker for low or unavailable/stale quota. Dual views retain both periods. Explicit `number` mode uses the most constrained known selected period and shares the taskbar's numeric color and font weight; it omits the percent sign and pads values below 10 with a zero. Warning colors use the Codex provider's existing used-quota `alert_threshold` (80 means a warning at 20% remaining); stale/estimated data stays muted. Automatic number colors progress from amber to red below that threshold; explicit custom number colors remain respected.

Left-click the taskbar or tray entry for quota details. All plans use the independent five-module popup layout; missing data remains unavailable. Hover tooltips are removed. Click again, click outside or press Esc to close; dragging does not open details. Language, proxy, providers and refresh interval are in the More menu.

Settings and menus follow **AppsUseLightTheme**, while taskbar/tray ink follows **SystemUsesLightTheme**, including Windows mixed-theme configurations.

Sizes and coordinates use DIP. Dual rings constrain their rendered stroke to maintain a gap; dual text rows fit their available height without rewriting saved settings. Extreme manual ring/text offsets may intentionally overlap; bar text reserves a minimum gap before the bar.

Preferences preview a draft on the taskbar and tray. Save commits it; Cancel or closing restores the saved appearance. Reset affects appearance only. Compact/Standard/Large presets update ring diameter, text size, bar width and the ring/text spacing; Custom keeps individually edited values. Advanced controls show geometry for the active style. Small work areas use vertical scrolling, including keyboard focus scrolling.

Each new appearance field has an independent default or normalization rule. Existing ring color, offsets, text preferences and position lock survive upgrades. Unknown enum values cannot discard sibling settings or credentials. Legacy `general.indicator = "bars"` remains a tray compatibility alias; choose `[appearance].display_style = "bars"` for the new remaining-quota bar presentation.

## Language, networking and authentication

Automatic language mode selects Simplified Chinese for Chinese Windows display languages and English otherwise. Menu changes apply immediately. Chinese UI uses installed Windows fonts without downloading a font package.

System proxy mode follows Windows current-user static proxies and reqwest environment-proxy rules (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` and lowercase forms). Windows static-proxy changes affect the next request; external changes to environment variables require restarting the app. PAC/WPAD is not supported. Direct mode ignores these proxy settings, but cannot bypass operating-system TUN/VPN routing. Failed proxy requests never silently retry direct, and HTTPS certificate verification stays enabled.

Authentication uses the same configuration and proxy selection:

```powershell
.\ailimits-auth.exe status
.\ailimits-auth.exe set copilot
.\ailimits-auth.exe set claude
.\ailimits-auth.exe set-usage-token codex
.\ailimits-auth.exe remove copilot
.\ailimits-auth.exe remove-usage-token codex
```

Secret input is hidden and stored in Windows Credential Manager. Manual usage tokens are validated before saving. Base quota requests do not refresh or rotate official CLI tokens. Optional extension reads run the official CLI, which manages its own authentication.

## Legacy reference

The following inherited reference includes old desktop-widget options, which are retained for configuration compatibility but are not active QuotaBar features. Use the taskbar settings above for current presentation.

# CONFIG.md — the configuration file

Location: `%APPDATA%\AiLimits\config.toml`. Created automatically on the
first launch; every context-menu change is persisted here. Unknown fields
and providers are ignored (old configs never break parsing). On a parse
error the widget logs a warning and falls back to the defaults.

Next to it the widget may keep `provider-cache.json` (last known metrics
with a future reset — they survive restarts), `history.jsonl` (bounded usage history feeding the
Expanded sparkline) and, when `AILIMITS_LOG` or `RUST_LOG` is set,
`ailimits.log`.

## Full example

This fork also supports `[network] proxy_mode = "system"` (default) or
`"direct"`. The Network proxy menu applies changes to the next request.
System mode follows reqwest's environment/Windows static-proxy rules;
PAC/WPAD is not supported. System setting changes are detected automatically;
environment changes outside the running process require a restart.

```toml
[general]
# Language: "auto" follows Windows display language (Chinese → zh-CN,
# otherwise English); "zh-CN" / "en" explicitly select a language.
language = "auto"
# Update interval in seconds (menu: 60 / 300 / 900 / 1800; hard minimum 60).
# Polling pauses while idle/locked and backs off when a whole cycle fails;
# Claude's endpoint is also rate-limited server-side, so its refresh can
# occasionally take 2–3 minutes regardless of this setting.
update_interval_secs = 60
# Desktop widget is opt-in; tray/panel clicks do not open it while disabled.
show_widget = false
# Taskbar indicator: panel_rows (default) / panel_grid (legacy alias) display
# a single Codex WEEKLY REMAINING ring with an adjacent percentage.
# No weekly metric => a track and dash; old data is grey, estimates use ≈.
# Follows taskbar theme, DPI and auto-hide; tray mode is selected manually.
# tray uses the same Codex weekly remaining ring; bars is a legacy usage view.
# Panel placement checks actual shell controls via background UI Automation.
# Prefers free space before the tray, then before the app buttons. When space
# is insufficient or detection fails, retains its previous position and size.
# Drag the ring or digits horizontally to persist a manual position (DIP).
# Manual positions may overlap other buttons; use Restore automatic position
# to clear this optional value.
# panel_position_x = 420
# off is only allowed while the desktop widget is enabled.
indicator = "panel_rows"
# Automatic updates (menu: Automatic updates). When true, AI Limits checks
# GitHub for a newer release in the background (~30s after start, then daily),
# verifies the installer against the release's published SHA-256, installs it
# silently and restarts. Set false to disable; missing in old configs → true.
auto_update = true
# Manual nudge for the panel's position, in pixels, applied after the
# computed placement. Both default to 0, meaning no adjustment. Clamped to
# -200..200 — a typo cannot push the panel off the desktop. A repair tool
# for machines where the automatic placement misses: replaced shells such
# as StartAllBack or ExplorerPatcher, unusual taskbar layouts, touch-device
# layouts. Deliberately config-only, no menu entry.
panel_offset_x = 0
panel_offset_y = 0
# Which taskbar the mini panel attaches to (menu: Indicator -> Display;
# that submenu only appears when a secondary taskbar exists, and it's built
# at startup, so plugging in a monitor later needs a restart before the
# entry shows up). "primary" (default), or { secondary = N } where N counts
# secondary taskbars from 0, left to right by monitor position — e.g.
# { secondary = 0 } for the first (leftmost) secondary taskbar. Secondary
# taskbars only exist when Windows is set to show the taskbar on all
# displays. Falls back to "primary" if the chosen display is gone (monitor
# unplugged, or that Windows setting turned off), or if the value is
# unrecognised, rather than breaking the whole config file.
panel_display = "primary"

[window]
pos_x = 50
pos_y = 50
# Background opacity 0.10–0.85.
opacity = 0.45
# Always on top.
pinned = false
# Locked position — dragging disabled (independent of `pinned`).
locked = false

[ui]
# Palette: default / ocean / sunset / forest / neon / ice / rose / slate.
palette = "default"
# Palette saturation 0–100 (0 = greyscale, 100 = full color).
saturation = 55
# Text and bar brightness 20–100.
brightness = 100
# Monochrome mode (overrides the palette).
monochrome = false
# Layout: "vertical" or "horizontal".
layout = "vertical"
# Detail level: "compact" / "medium" / "expanded".
detail = "compact"
# Width step for the horizontal-bar (rows) layout; the vertical-bar layout
# ignores it. "full" / "three_quarters" / "half" (menu: Width).
width_scale = "full"
# Column arrangement for the vertical-bar layout; the rows layout ignores it.
# "row" (side by side, wide and short) or "column" (stacked, narrow and tall)
# (menu: Arrangement).
column_flow = "row"
# Burn-rate forecast ("~Xh to limit" when usage is climbing; menu: Forecast).
# Never replaces the reset countdown — shown only when no future reset is known.
show_forecast = false

[[providers]]
# Identifier: claude / codex / copilot / antigravity ("gemini" is a legacy
# id, renamed to "antigravity" on load).
id = "claude"
enabled = true
# Method: "subscription" (default) or "api_key" — only Claude has a choice.
auth_method = "subscription"
# Credential Manager key label; empty for the subscription method.
credential_label = ""
# Notification threshold, %.
alert_threshold = 80

[[providers]]
id = "codex"
enabled = true
auth_method = "subscription"
alert_threshold = 80

[[providers]]
id = "copilot"
enabled = true
auth_method = "subscription"
alert_threshold = 85

[[providers]]
id = "antigravity"
enabled = true
auth_method = "subscription"
alert_threshold = 80

[notifications]
enabled = true
# Per-provider toast cooldown, minutes.
cooldown_minutes = 15

[hooks]
# Optional shell commands run on usage events (empty = disabled, the
# default). Each runs detached via `cmd /C`, hidden, fire-and-forget; the
# event context arrives in environment variables: AILIMITS_EVENT
# (threshold|reset|startup), AILIMITS_PROVIDER, AILIMITS_PERCENT,
# AILIMITS_RESET_AT (RFC3339, when known).
# SECURITY: these are deliberately config-file-only — there is no menu or
# clipboard path, so nothing can auto-populate a command to run.
# Fires when a provider crosses its alert threshold (shares the toast cooldown):
on_threshold = ""
# Fires when a near-limit provider's window resets (usage drops sharply):
on_reset = ""
# Fires once after the first successful fetch:
on_startup = ""
```

## Secrets

The config never holds keys or tokens — only labels. The actual values live
in the Windows Credential Manager under service `ailimits`:

| Label | Meaning |
|---|---|
| `claude_api_key` | Claude API key (the api_key method) |
| `claude_usage_token` | manual Claude subscription usage token |
| `codex_usage_token` | manual ChatGPT/Codex usage token |
| `copilot_pat` | GitHub PAT (used instead of the gh CLI) |

The Rust structs live in `src/config/schema.rs` — that file is the single
source of truth for the schema.


## Independent popup modules

Settings has **Taskbar appearance** and **Panel content** pages. Check each module, drag its handle or use the up/down buttons; the preview follows immediately. Save commits, Cancel/close rolls back, and Restore defaults resets the current page. Hidden rows retain their slots; hiding everything leaves Settings accessible. The module array order is the display order:

```toml
[panel]
modules = [
  { id = "five_hour", visible = true },
  { id = "weekly", visible = true },
  { id = "reset_credits", visible = true },
  { id = "quota_history", visible = true },
  { id = "token_activity", visible = true },
]
```

Missing sections/modules receive defaults; unknown IDs are ignored and duplicates keep their first entry. Taskbar position and appearance remain separate. Pace compares remaining quota with `(reset-now)/window duration`, bounded to 0–100%; cached, estimated or incomplete observations have no pace verdict. The Codex alert threshold controls low-quota coloring.

Optional extensions use the local official Codex App Server's `account/rateLimits/read` and `account/usage/read`, described in the [official protocol](https://learn.chatgpt.com/docs/app-server). Opening refreshes only enabled extension modules whose cache is older than five minutes. Manual refresh includes the existing base quota scheduler. Duplicate reads coalesce. The helper honors `CODEX_HOME` (default `~/.codex`), uses file credentials, checks identity before and after reading, and never merges an unconfirmed account. No model turn, login or reset-credit consumption is requested. The official CLI manages its own authentication.

A hidden, suspended helper is attached to a Windows job before it can spawn descendants. It has a 25-second read deadline; closing the job kills the entire owned tree and the parent is reaped. Missing Codex, unsupported methods, null fields, zero available credits and count-only credits are distinct states.

`%APPDATA%/AiLimits/quota-history.json` retains 30 days, partitioned by opaque SHA-256 account keys without tokens or email addresses. Quota observations append on changes and at 15-minute anchors, including while hidden; daily compaction removes old data. Token buckets replace their date rather than increment it. Gaps over `max(2 × refresh interval, 30 minutes)` and reset boundaries break quota lines. No pre-installation quota history is fabricated. Missing Token days differ from genuine zero; totals include only known dates.

Click either chart to open a native detail window (quota: current cycle/7/30 days; Tokens: 7/30 days). Hovering samples displays dates and values in the window footer. Graph bitmaps are cached until data, size, theme, language or date changes. Closing the popup stops its UI timer.
