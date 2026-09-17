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

左键点击任务栏或托盘打开完整配额面板，面板模块独立于任务栏配置，默认依次显示 5 小时额度、每周额度、重置机会、额度历史和 Token 活动。缺失数据保留不可用状态。任务栏和托盘不再显示悬停提示。再次点击、点击外部或 Esc 关闭详情；拖动调整位置不会打开详情。语言、代理、服务商和刷新间隔等低频操作移至“更多”菜单。

设置窗口与菜单跟随 Windows 应用主题；任务栏和托盘跟随 Windows 系统主题。已有自定义颜色、偏移、尺寸和高级参数保留。视觉规范与验证说明见 [UI 设计记录](UI-DESIGN.md)。


## 面板内容与本地历史

原生设置的“面板内容”页可独立开关五个模块，拖动左侧手柄或点击上下按钮排序。右侧实时预览布局；恢复默认重置当前页，保存提交两页修改，取消或关闭恢复原配置。隐藏项保留顺序，全部隐藏时仍可进入设置。任务栏外观及位置不受面板排序影响。

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

顺序取决于数组中的位置；标识稳定，不受语言影响。旧配置缺少本节时自动补齐默认模块；缺少新模块时追加，未知模块忽略，重复项只保留首项。

基础额度沿用现有刷新间隔，记录服务返回的周期时长和套餐。额度剩余低于时间剩余时显示“使用偏快”；时间不完整、缓存或估算数据不作节奏判断。低额度颜色沿用 Codex 的告警阈值。

扩展使用本机 `codex app-server --listen stdio://` 的 `account/rateLimits/read` 与 `account/usage/read`，协议见[官方 App Server 文档](https://learn.chatgpt.com/docs/app-server)。仅已显示的扩展模块在打开面板时刷新超过 5 分钟的缓存；手动刷新同时触发基础额度与启用的扩展读取，重复请求合并。使用 `CODEX_HOME`，默认 `%USERPROFILE%\.codex`；强制官方 CLI 使用该目录的文件认证，并在读取前后核对账号。不能确认一致时不合并数据。

辅助进程后台运行，不创建对话或消耗重置机会；25 秒总超时，结束后关闭 Windows 作业对象并等待进程退出，连同子进程清理。无 Codex、接口缺失、账号不匹配分别显示不可用。

历史保存到 `%APPDATA%\AiLimits\quota-history.json`，采用账号身份的 SHA-256 摘要作为分区标识，不保存令牌和邮箱。保留 30 天；额度变化即追加，相同额度每 15 分钟保留锚点，每日清理。Token 按日期替换，不累加重复响应。额度图超过 `max(2 × 刷新间隔, 30 分钟)` 的空白不连线，跨周期分段；不补造安装前的数据。Token 总量仅合计已返回日期，缺失日期不等同于零。

点击图表打开独立原生详情窗口。额度可选当前周期、近 7 天、近 30 天；Token 可选近 7 天、近 30 天。指向数据点在窗口底部显示日期和数值。图表缓存随数据、尺寸、主题、语言和日期变化更新；关闭面板后停止倒计时计时器。
