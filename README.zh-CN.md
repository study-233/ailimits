# AI Limits 中文版

Windows 11 原生 Codex 每周余量任务栏工具，支持 Claude、OpenAI Codex、GitHub Copilot 和 Google Antigravity。

本仓库是 [napxlexn/ailimits](https://github.com/napxlexn/ailimits) 的 fork，新增 Codex 每周余量圆环、简体中文／英文切换与系统代理适配。保留原项目 GPL-3.0-or-later 许可及作者署名。

[English](README.md) · [上游乌克兰语说明](README.uk.md)

## 下载与安装

从 [本 fork 的 Releases](https://github.com/study-233/ailimits/releases/latest) 下载 Windows 11 x64 版本：

- `AiLimits-Setup-0.1.0.exe`：安装版，包含主程序、认证工具、中英文说明和许可证，支持应用内自动更新。
- `AiLimits-Portable-0.1.0.zip`：便携版，解压后运行 `ailimits.exe`，以后手动下载更新。
- `SHA256SUMS.txt`：校验文件；可用 PowerShell 的 `Get-FileHash <文件路径> -Algorithm SHA256` 比对。

本 fork 从 **v0.1.0 正式版**重新编号。运行旧 `0.6.4` 开发构建的用户需要首次手动安装；旧构建不会将较小的版本号识别为更新。安装目录、配置路径和安装标识沿用原项目，因此安装版会替换已有安装，不能与上游安装版并行安装。原有配置继续读取。

首次安装也可以使用 PowerShell：

```powershell
irm https://raw.githubusercontent.com/study-233/ailimits/master/install.ps1 | iex
```

## 快速使用

1. 先正常登录并使用相应官方 CLI，然后运行 `ailimits.exe`。小组件读取已有认证信息，不主动刷新或轮换官方 CLI 的令牌。
2. 默认在任务栏显示 Codex 每周余量圆环，右侧上方显示大号数字如 `68 %`，下方居中显示小号 `Codex`。亮色弧线代表剩余额度，100% 是完整圆环，0% 只显示底环；不使用会话额度替代每周额度。右键可设置服务商、语言和网络代理。
3. 已移除桌面悬浮窗，只保留任务栏面板和手动选择的托盘入口。右键“外观设置…”可分别调整圆环与数字。
4. “语言”提供“跟随系统／简体中文／English”。默认按 Windows 显示语言选择：中文系统使用简体中文，其他系统使用英文。菜单切换立即生效并保存。
5. “网络代理”提供“跟随系统／直连”。默认跟随 Windows 当前用户的系统代理，适配 Clash、v2rayN 等软件开启的系统代理。

中文绘制优先使用系统微软雅黑，按需加载并在设置窗口和任务栏提示间共享；无需下载或随应用分发字体。精简过的 Windows 需要保留系统中文字体。

## 余量与数据状态

鼠标停在圆环上可查看 Codex 每周余量、数据状态与本地时间的每周重置时间。额度缺失或无法认证且没有有效数据时显示 `—`；超过 5 分钟的旧数据置灰，推算数据带 `≈`。每周额度按接口的周期长度识别，不依赖它位于主窗口还是次窗口。旧版本 Codex 缓存会等待新请求纠正周期。仅凭过期数据无法保证额度已经重置，以服务商下一次成功返回为准。

面板随任务栏自动隐藏，适配系统明暗主题及缩放。按住圆环或数字可水平拖动，松手保存，重启后保持位置；右键“恢复自动位置”可重新寻找空白区。手动位置允许与应用图标重叠，界面不会强行吸附或缩小。

自动定位优先寻找空白区，空间不足或检测失败时保持上次位置。只有手动选择“托盘图标”才进入托盘；空间不足、开始菜单遮挡或检测失败不会自动切换。全屏应用运行时面板随任务栏隐藏，恢复后回到原位置。托盘和面板统一显示 Codex 每周剩余额度。

数字使用 Segoe UI Semibold，缺失时回退 Segoe UI；数字使用固定字位宽度，百分号和估算符号缩小并对齐基线。

旧配置若已指定托盘模式，可通过“任务栏指示器 → 任务栏面板”切换为圆环加数字。

## 外观与位置

百分比与 `Codex` 标签组成上下两行，整体跟随数字位置和颜色设置。标签为 Segoe UI 常规字体，默认高度 9 DIP，行间距 2 DIP。两行始终显示：数字过大时仅缩小实际显示尺寸，不修改保存的字号。无有效数据时显示 `—` 和 `Codex`，过期数据两行一起置灰。托盘仍显示圆环。

右键“外观设置…”提供圆环、数字独立的颜色、大小及水平位置和垂直偏移。圆环支持线宽；数字支持常规、半粗、粗体，以及百分号和估算符号的比例。颜色可使用原生选色器或输入 `#RRGGBB`，数字输入 `auto` 跟随任务栏主题。预览同时展示深浅主题，并即时更新任务栏；“保存”持久化，“取消”或关闭恢复原设置，“恢复默认外观”只重置外观。

大小和位置以 DIP 计量，随 Windows 缩放保持一致；垂直偏移会限制在任务栏内部。元素可以重叠；面板宽度随所选外观计算，不因任务栏拥挤缩小。

未锁定时按住圆环或数字水平拖动，松手保存；按 Esc 取消拖动。“锁定位置”固定当前面板位置并禁止拖动，重启后保持；解锁不移动面板。“恢复自动位置”解除锁定并重新寻找空白区域。空间不足、图标增减或检测失败不会自动切回托盘。

## 系统代理

- 开启代理软件的“系统代理”后，小组件下一次网络请求会自动采用新设置。关闭代理或切换端口同样无需重启；已有请求正常结束，刷新频率保持原设置。
- 兼容 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY` 及其小写形式，按 reqwest 0.12 的优先级解析。协议专用环境变量优先于对应的 Windows 系统设置；`ALL_PROXY` 是回退项。排除列表沿用该库的匹配语义。
- 环境变量中的代理地址可以使用 `http://127.0.0.1:7890`、`socks5://127.0.0.1:1080` 或 `socks5h://127.0.0.1:1080`。地址和端口以代理软件实际设置为准。
- “直连”忽略系统代理和代理环境变量；代理连接失败不会自动转为直连。TUN/VPN 在操作系统网络层生效，不受应用直连选项控制。
- 本轮支持静态系统代理，不支持 PAC 脚本或 WPAD 自动发现，也没有单独的手动代理地址编辑界面。使用代理软件时请选择写入静态系统代理的模式。
- 外部修改环境变量不会改变已运行进程的环境，因此需要重启小组件；Windows 系统代理设置变化无需重启。

四个服务商、`ailimits-auth.exe` 的令牌验证、更新检查和更新下载均使用相同代理策略。HTTPS 仍验证服务器证书；代理适配不关闭 TLS 验证。

## 配置

配置文件为 `%APPDATA%\AiLimits\config.toml`。右键菜单的修改自动保存。已有配置无需手动迁移，缺少新字段时使用默认值。

```toml
[general]
language = "auto" # auto / zh-CN / en
indicator = "panel_rows" # 每周余量圆环＋数字
panel_locked = false # 勾选“锁定位置”后禁止拖动和自动避让
# panel_position_x = 420 # 可选：距所选任务栏左侧的 DIP 坐标；删除此项恢复自动定位

[network]
proxy_mode = "system" # system / direct
```

手动编辑配置后重启应用。未知的语言或代理模式值会回退到默认值，不影响其他设置。完整配置选项参见 [英文配置说明](docs/en/CONFIG.md)。

## 认证工具

`ailimits-auth.exe` 使用同一配置文件中的语言及代理选项：

```powershell
.\ailimits-auth.exe status
.\ailimits-auth.exe set copilot
.\ailimits-auth.exe set claude
.\ailimits-auth.exe set-usage-token codex
.\ailimits-auth.exe remove copilot
.\ailimits-auth.exe remove-usage-token codex
```

密钥输入隐藏，保存在 Windows 凭据管理器中；`set-usage-token` 先验证服务商接受该令牌，再保存。品牌名称、命令、文件名和原始服务端错误保留原文。

## 构建与验证

使用 Windows、Rust MSVC 工具链及 Visual Studio C++ Build Tools（包括 Windows SDK）：

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup component add --toolchain stable-x86_64-pc-windows-msvc rustfmt clippy
cargo +stable-x86_64-pc-windows-msvc fmt --all -- --check
cargo +stable-x86_64-pc-windows-msvc clippy --all-targets -- -D warnings
cargo +stable-x86_64-pc-windows-msvc test
cargo +stable-x86_64-pc-windows-msvc build --profile release-min --bins
```

生成文件为 `target\release-min\ailimits.exe` 和 `target\release-min\ailimits-auth.exe`。运行本地编译版无需安装器。网络测试只访问本地模拟服务器，不使用真实令牌。

## 更新

应用内自动更新使用 [study-233/ailimits Releases](https://github.com/study-233/ailimits/releases)，没有 Release 时跳过，不回退到上游仓库。安装器仍需通过 GitHub 发布的 SHA-256 摘要校验。便携版／开发构建沿用原项目行为，不自动运行安装器。

本 fork 暂未发布到 winget、Scoop、Chocolatey 或 Microsoft Store；这些渠道中的原项目包不包含本 fork 的改动。许可证与原作者署名见 [LICENSE](LICENSE)，名称及标志说明见 [TRADEMARKS.md](TRADEMARKS.md)。
