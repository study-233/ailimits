# QuotaBar 配置与网络

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

手动编辑配置后重启应用。未知的语言或代理模式值会回退到默认值，不影响其他设置。完整配置选项参见 [英文配置说明](../en/CONFIG.md)。

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


外观字段与范围见[完整配置参考](../en/CONFIG.md)。


## 托盘显示与主题

托盘与任务栏共用所选显示样式、周期及周期颜色。托盘显示提供“自动”“跟随显示样式”“数字”“图形 + 状态”；配置值仍为 `auto` / `ring` / `number` / `ring_state`，旧设置可继续读取。图形按托盘的 16/20/24/32 像素空间适配；选择横向进度条时托盘也显示进度条，选择双周期时保留两个周期。

“自动”和“图形 + 状态”在低额度或数据不可用、过期时增加角落状态点。“数字”使用已知周期中剩余额度较低的一个，共用任务栏数字的颜色和字重，不带百分号，小于 10 时补零。警告沿用 Codex 的 `alert_threshold`（已用百分比），缓存、估算及未知数据保持弱化显示。

两处悬浮信息共用相同的文案、行序、重置倒计时和更新时间，并遵循所选周期。Codex 套餐明确为 `pro` 或 `prolite` 时，两处提示都只显示周额度，即使选择了 5 小时；周额度缺失时显示不可用。这条规则仅影响悬浮信息，图形和原始额度数据保留。

设置窗口与菜单跟随 Windows 应用主题；任务栏和托盘跟随 Windows 系统主题。已有自定义颜色、偏移、尺寸和高级参数保留。视觉规范与验证说明见 [UI 设计记录](UI-DESIGN.md)。
