# AI-4 拓展核

对应 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 第 8 章（无限拓展）+ 第 10 章（安全合规），
以及扩充章 14（更多拓展）、17（性能压测）、18（安全加固）。

> 范围约束：只实现/修改 `portable/AI4/`，不改 AI1/2/3/5，不改 `src/`、`docs/` 其它文件。

## 文件

| 脚本 | 主计划章节 | 用途 |
|---|---|---|
| `Merge-Apps.ps1` | 8.1 | 层式 Apps VHDX：创建/合并/绿色软件入 Data + 符号链接 |
| `MSIX-Attach.ps1` | 8.2 | MSIX 打包/签名/安装/AppAttach 挂载/卸载 |
| `Plugin-Host.ps1` | 8.3 / 14.1 | 插件热加载宿主：SHA256+签名校验+LoadLibrary+权限 |
| `Plugin-Manager.ps1` | 14.1 | 插件市场清单增删/安装/校验 |
| `Config-Runtime.ps1` | 8.4 | RegLoadKey 挂注册表 + path.env 环境变量 + 快捷键热重载 |
| `Security-Manager.ps1` | 10 | BitLocker / Defender / 授权 / 合规 / 自检 |
| `Cloud-Sync.ps1` | 8.5 / 14.3 | rclone 增量同步/恢复/差异/计划任务 |
| `Data-Init.ps1` | 8.4 / 3.2 | Data 目录树 + path.env + 配置随盘 + 符号链接 |
| `Benchmark.ps1` | 17 | 拓展/安全链路压测 |
| `AI4-拓展安全.md` | 8+10+14/17/18 | 3000 字实现说明 |
| `Config/` | 8.4 / 14.1 | 插件市场、权限、快捷键、path.env 模板 |

## 快速上手（管理员 PowerShell）

```powershell
# 1. 建立 Data 目录与配置随盘
.\Data-Init.ps1 -DataDrive D:

# 2. 拓展：绿色软件 + MSIX + 插件 + 配置随盘
.\Merge-Apps.ps1 -Action Add-PortableApp -AppName Blender -AppSource D:\dl\Blender
.\MSIX-Attach.ps1 -Action Mount -PackagePath D:\Data\MSIX\Blender.msix
.\Plugin-Manager.ps1 -Action Verify
.\Plugin-Host.ps1
.\Config-Runtime.ps1 -Action Apply

# 3. 安全合规
.\Security-Manager.ps1 -Action Apply -DataDrive D:
.\Security-Manager.ps1 -Action SelfCheck

# 4. 云同步
.\Cloud-Sync.ps1 -Action Setup -Remote "onedrive:VariableBackup"
.\Cloud-Sync.ps1 -Action Sync

# 5. 压测
.\Benchmark.ps1 -Action Run -DataDrive D:
```

详见 `AI4-拓展安全.md` 与 `docs/PORTABLE_AI_SPLIT_PLAN.md` 的 AI-4 小节。
