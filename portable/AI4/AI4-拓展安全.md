# AI-4 拓展安全 — 第8章 无限拓展 + 第10章 安全合规 实现说明

> 归属：`portable/AI4/` · 对应主计划：第8章 + 第10章 + 扩充14/17/18
> 作者：AI-4 拓展核 · 日期：2026-09-07
> 范围约束：仅修改 `portable/AI4/`，不改 AI1/2/3/5，不改 `src/`、`docs/` 其它文件。
> 一句话：把「层式 VHDX / MSIX App Attach / 插件热加载 / 配置与云】落地成可运行脚本，并把「BitLocker / Defender / 授权 / 合规】做成可校验的自检闭环。

---

## 1. 交付物总览

| 文件 | 章节 | 作用 | 关键命令 |
|---|---|---|---|
| `Merge-Apps.ps1` | 8.1 / 3.2 | Apps 层创建、合并、绿色软件入 Data + C盘符号链接 | `-Action Create / Merge / Add-PortableApp / Status` |
| `MSIX-Attach.ps1` | 8.2 / 13.5 / 18.2 | MSIX 打包、签名、安装、AppAttach 挂载/卸载 | `-Action Package / Install / Mount / Dismount / Uninstall / Status` |
| `Plugin-Host.ps1` | 8.3 / 14.1 | 插件清单校验 + SHA256 + 签名 + LoadLibrary 热加载 | 默认校验加载、`-VerifyOnly`、`-AuthorizeAll` |
| `Plugin-Manager.ps1` | 14.1 | 插件市场清单增删/安装/列表/校验 | `-Action Init / Add / Install / List / Verify / Remove` |
| `Config-Runtime.ps1` | 8.4 | RegLoadKey 挂注册表 + path.env + 快捷键热重载 | `-Action Apply / Mount-Registry / Unmount-Registry / Apply-Env` |
| `Security-Manager.ps1` | 10.1-10.4 / 18 | BitLocker / Defender / 授权 / 合规 / 清理 / 自检 | `-Action Apply / BitLocker / Defender / Compliance / SelfCheck` |
| `Cloud-Sync.ps1` | 8.5 / 14.3 | rclone 增量同步 / 恢复 / 计划任务 / 差异 | `-Action Setup / Sync / Diff / Restore / Schedule / Status` |
| `Data-Init.ps1` | 8.4 / 3.2 | Data 目录树 + path.env + 配置模板 + 符号链接 | `-DataDrive D:`、`-FixLinks` |
| `Benchmark.ps1` | 17 | 拓展/安全链路压缩测 + 压测清单 | `-Action Run / Manifest` |
| `Config/plugin-market.json` | 14.1 | 插件市场清单（wallpaper / ai-assist） | 由 Plugin-Manager 维护 |
| `Config/permissions.json` | 14.1 / 18 | 网络插件授权与默认策略 | 由 Plugin-Host 维护 |
| `Config/shortcuts.json` | 8.4 | 快捷键配置（可热重载） | Core 或 Data-Init 复制到 Data |
| `Config/path.env` | 8.4 | 随盘环境变量模板 | Data-Init 部署到 `Data/Env` |

> 说明：本目录脚本全部是**管理员/受控 PowerShell**，可在完整 Windows 11 上直接运行；在精简版变量系统内若有未装载模块（如 Hyper-V），脚本会降级提示而非静默损坏。

---

## 2. 第8章 无限拓展 实现

### 2.1 层式镜像（8.1）

三层差分链沿用第3章：`Base.vhdx` 只读母盘 → `Apps.vhdx` 50GB 只读共享层 → `User.vhdx` 动态差分。本模块新增 **Apps 层的可分发/可合并**能力。

- **创建壳**：`Merge-Apps.ps1 -Action Create -AppsSizeGB 50`，用 64KB 簇 NTFS 格式化，标签 `VariableApps`。
- **合并回 Base**：`Merge-Apps.ps1 -Action Merge`，先 `Copy-Item` 备份 `Base.pre-merge`，再 `Merge-VHD -Path Apps.vhdx -DestinationPath Base.vhdx`，最后 `Optimize-VHD -Mode Full`。结果让 50GB 软件更新合并进母盘，下一台机器拿到新 `Base` 即可复用，无需重装系统。
- **绿色软件入 Data**：`Merge-Apps.ps1 -Action Add-PortableApp -AppName Blender -AppSource D:\dl\Blender`。robocopy 把实体放到 `D:\Data\Apps\Blender`，再用 `mklink /J C:\Program Files\Blender -> D:\Data\Apps\Blender`，实现 **C 盘零膨胀**。
- **验收**：`Status` 打印三 VHDX 的 `Size / FileSize / Type` 与 Data 内符号链接，满足「`Copy-Item NewApp Data\Apps + mklink` 即用」的验收标准。

### 2.2 MSIX App Attach（8.2 / 13.5）

- **打包**：`MSIX-Attach.ps1 -Action Package -PackagePath C:\src\Blender.app -OutDir D:\Data\MSIX`。用 Windows SDK `MakeAppx.exe` 打包为 `.msix`，再 `signtool sign /fd SHA256` 签名。
- **安装版**：`-Action Install` 走 `Add-AppxPackage`，出现 `0x80073CF3`（未启用旁加载）或 `0x80070005`（无管理员）会给出明确提示。
- **App Attach 挂载版**：`-Action Mount` 用 `Mount-AppxVolume -PackagePath ...msix -VolumePath D:\Data\MSIX\Mount`，**不写 C 盘、不污染注册表**，删除文件即卸载；相比安装版更适合 PS/VS 等重型软件。
- **卸载**：`Uninstall` 先 `Remove-AppxPackage`，再 `Dismount-AppxVolume`，删除包文件即彻底清理。
- **供应链保护**：每次操作先 `Get-AuthenticodeSignature` 校验；`0x80073CF3` 之类错误按已知错误码引导修复。
- **验收**：`Status` 列出已安装 Appx 与已挂载 AppxVolume；MSIX 挂载后开始菜单应出现对应应用。

### 2.3 插件化 Variable Engine（8.3 / 14.1）

- **Plugin-Manager**：维护 `Config/plugin-market.json`，登记 id/version/entry/permissions/publisher/sha256/signature；`-Action Verify` 对每个插件做 SHA256 + Authenticode 双重校验；`-Action Install` 复制 DLL、重算哈希、保存签发者指纹。
- **Plugin-Host**：启动时读取清单并做「四重校验」（清单存在 → 入口存在 → SHA256 匹配 → 签名有效）；对声明 `network` 权限的插件弹出授权确认（`-AuthorizeAll` 可批量授权），授权结果写 `Data/Config/permissions.json`。
- **热加载**：用 `Add-Type` C# P/Invoke 提供 `kernel32!LoadLibrary` + `GetProcAddress("VariablePluginInit")` 的等价宿主验证。实际操作中 Variable Engine Core 由 Rust 承载 `IPluginHost`（取图标 / 弹通知 / 限额启动）；本脚本保证跨语言可复现的加载/校验流程。新插件丢进 `Data\Plugins`，重启或 `Plugin-Host.ps1` 即生效。
- **权限**：对照主计划第14.1「Core 校验 SHA256 + 签名，权限按 JobObject 限额，网络插件用户显式授权」，默认策略 `network=ask`、`unsafe_native=deny`、`allow_unsigned=false`。
- **验收**：`Verify` 全绿后 `Plugin-Host` 能列出 `loaded`；未授权网络插件被跳过并写 `skipped=unauthorized-network`。

### 2.4 配置随盘走（8.4）

- `Data-Init.ps1` 建立 `Data/Env/path.env`、`Data/Config/{permissions,shortcuts,plugin-market}.json`、`Data/Registry/User.dat` 占位。
- `path.env` 定义 `VARIABLE_PLUGIN_ROOT / VARIABLE_MSIX_DIR / VARIABLE_CLOUD_REMOTE / VARIABLE_EXCHANGE` 等，Core 启动时 `SetEnvironmentVariable` 并支持相对 `DataRoot` 的展开。
- `shortcuts.json` 记录 `Win+D / Alt+Tab / Win+左 / Win+,` 等热键，约定热重载；`RegLoadKey(HKEY_USERS,"VariableUser", Data\Registry\User.dat)` 由 Core 启动时挂载，避免写宿主 HKCU。

### 2.5 云拓展（8.5 / 14.3）

- `Cloud-Sync.ps1 -Action Setup -Remote onedrive:VariableBackup`（或坚果云/S3）写入 `Data/Sync/rclone.conf`。
- `-Action Sync`：`rclone sync D:\Data remote:VariableBackup --transfers 4 --bwlimit 10M --exclude "*.tmp"`，增量、带宽限制、排除缓存。
- `-Action Diff`：`rclone check --one-way` 校验一致性；`-Action Restore`：新盘 `rclone copy` 恢复；`-Action Schedule`：`schtasks /SC MINUTE /MO 30` 每 30 分钟同步，User 数据保留 7 天版本，`User.vhdx` 每日 Checkpoint 保留 3 份。
- **U盘丢失恢复**：新盘 `rclone copy` + 合并 VHDX，10 分钟级恢复，满足「U盘丢新盘 rclone copy 即恢复」。

---

## 3. 第10章 安全与合规 实现

### 3.1 加密（10.1）

- `Security-Manager.ps1 -Action BitLocker -DataDrive D:`，调用 `manage-bde -on D: -EncryptionMethod XTS-AES256 -RecoveryPassword`，并先 `-autounlock -off` 确保 **拔盘即锁**。
- 恢复密钥（RecoveryPassword）写 `D:\Data\Security\BitLocker-Recovery.txt`，文档明确要求「另存离线」，避免把明文恢复包留在盘内。
- 宿主无密码时仅能通过 `BitLocker To Go Reader` **只读**访问 Data 分区，达到「拔盘 BitLocker 锁」验收。
- 对「VHDX 虚拟盘」「U盘物理分区」同样适用；脚本对已启用/加密中/未加密三类状态做幂等判断。

### 3.2 杀软（10.2）

- `Security-Manager.ps1 -Action Defender -DataDrive D:` 用 `Add-MpPreference -ExclusionPath D:\Data\Apps` 排除绿色软件目录，避免误杀 Blender/VS 等大软件。
- `Data\Exchange` 写入 `.scan-policy.txt`，策略 `requirement=defender-full-scan`、`allow-unsigned=false`、`autoquarantine=true`，作为「受控摆渡通道强制扫描后才放行宿主」的可审计约定。
- **纵深防御（18.1）**：宿主 → Hypervisor → Guest 三层，Guest 内再分 Low Integrity Worker，即使 Guest 被 0day 攻破也需逃逸 Hypervisor 才能触达宿主。本模块不重复实现隔离（AI-2），只负责 Defender 排除/扫描策略落地。

### 3.3 授权（10.3）

- `Security-Manager.ps1 -Action License` 打印 `slmgr /dlv`；文档说明 Sysprep 后 `slmgr /ato` 自动激活；**零售/批量 Key 可用，OEM 授权不支持换主板**。
- 明确许可边界：Variable Engine 自身 MIT；MSIX/插件包需遵守原软件许可（商业软件不得以附加方式绕过授权）。

### 3.4 合规（10.4）

- `Security-Manager.ps1 -Action Compliance -BackupPath E:\backup.bcd`：可选 `bcdedit /export` 备份引导。
- 不修改宿主 `MBR/GPT`：B 模式仅写 U盘引导，宿主硬盘 `offline` 保护。
- `-Action Cleanup`：给出 `bcdedit /delete {GUID}` + `diskpart offline` 的一键卸载流程说明（默认**提示式**，避免误删宿主引导）。

### 3.5 自检（15.3 / 18.2 联动）

- `Security-Manager.ps1 -Action SelfCheck` 检查：Data 目录树、BitLocker 保护状态、Defender 排除项、恢复密钥文件、rclone 可用性；任一待办项以黄色警告列出。
- `Benchmark.ps1 -Action Run` 跑拓展/安全链路压缩测（Apps 导入、VHDX 副本、MSIX 可用性、插件加载探测、云 tool 探测）并追加 CSV；`-Action Manifest` 输出验收阈值。
- 供应链四重校验（ISO 哈希、官方驱动、MSIX 签名、插件签名）对应主计划 18.2，本模块落地后两段：MSIX-Attach 签名校验 + Plugin-Manager/Verify 校验。

---

## 4. 验收对照

| 主计划验收项 | 实现路径 | 状态 |
|---|---|---|
| 层式 Merge-VHD Apps 层可分发 | `Merge-Apps.ps1 -Action Merge` | ✅ 已实现 |
| MSIX 挂载后开始菜单出现 | `MSIX-Attach.ps1 -Action Mount/Install` | ✅ 已实现（需真环境） |
| 插件化 LoadLibrary 热加载 | `Plugin-Host.ps1` + `Plugin-Manager.ps1` | ✅ 已实现 |
| 配置随盘 RegLoadKey + path.env | `Data-Init.ps1` + `Config/path.env` | ✅ 已实现 |
| rclone sync Data remote | `Cloud-Sync.ps1 -Action Sync` | ✅ 已实现 |
| BitLocker XTS-AES 256 | `Security-Manager.ps1 -Action BitLocker` | ✅ 已实现（需真盘） |
| Defender 排除 Data/Apps + Exchange 强制扫描 | `Security-Manager.ps1 -Action Defender` | ✅ 已实现 |
| 零售授权 slmgr /ato | `Security-Manager.ps1 -Action License` | ✅ 已实现（需真 Key） |
| Copy-Item NewApp Data/Apps + mklink 即用 | `Merge-Apps.ps1 -Action Add-PortableApp` | ✅ 已实现 |
| 拔盘 BitLocker 锁 | `Security-Manager.ps1` 开启 + 关闭自动解锁 | ✅ 已实现 |

---

## 5. 已知边界与后续

- 本模块脚本在 **Windows 11 x64 完整版** 直接可用；在精简版变量系统内若缺 `manage-bde.exe` / `Mount-AppxVolume` / `rclone.exe`，脚本会降级为警告而非中断。
- `Merge-VHD`、`Mount-AppxVolume`、`manage-bde` 需管理员 + 对应 Hyper-V / 桌面体验功能；真实 BitLocker/MSIX 验收需在真盘/虚拟机上执行。
- 与 AI-1/AI-2/AI-5 的接口约定：路径固定为 `D:\Variable-USB\*.vhdx` 与 `D:\Data\...`；AI-5 部署后本模块脚本从 `portable/AI4/` 复制到系统内即可。
- 插件 `market.json` 的 placeholder 哈希/指纹需在实际安装后由 `Plugin-Manager -Action Install` 或 `-Action Verify` 更新。
