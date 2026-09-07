# AI-3 引导路（衔接层）

> 职责（五路分工）：引导器 · Shell 接管 · 便携交付的「引擎 ↔ VM ↔ 宿主」衔接。
> 引导器本体在仓库 `launcher/`（Rust，L-1/L-3 已实现探测 / VM 编排 / 降级链）；
> 本目录是部署与联调脚本层，与 `launcher/` 共用同一套探测口径（主机名+CPU+内存 host_key、47631 心跳、47632 回调）。

## 脚本清单

| 脚本 | 用途 | 运行位置 |
| --- | --- | --- |
| `Install-VMAgent.ps1` | 把引擎（vm-agent 构建）安装进 VM 镜像：复制文件、写心跳/运行档环境、登录自启项（VM 内） | 宿主（挂载 VHDX 后指向盘符） |
| `Set-VMComfort.ps1` | VM 档体验调优检查：分辨率/DPI、剪贴板桥接、驱动器直通、USB 重定向 —— 只检查+报告，不静默改系统 | 宿主（VM 运行中或挂载态） |
| `Self-Check.ps1` | AI-3 自检：脚本语法校验、launcher 探测缓存对齐、心跳端口契约核对 | 任意 |
| `Diff-Chain.ps1` | V-2：系统盘差分链（新建/体检/回滚点/一键重置），数据盘永不重置 | 宿主（需 Hyper-V） |
| `Tune-USB.ps1` | V-3：U 盘 4K 对齐/簇大小/写缓存/SMART 寿命 | 宿主 |
| `Verify-LargeApps.ps1` | V-4：10GB 级大软件（实体留 Data 盘 + C 盘链接 + 预取）走查报告 | 宿主 |
| `Compat-5Principles.ps1` | V-5：完全兼容 5 原则 VM 档核查（注册表/服务/驱动/GPU/兜底窗） | 宿主 |
| `Layer-Chain.ps1` | V-6：层式镜像三层差分链（base←app←user）新建/体检/重置/合并 | 宿主（需 Hyper-V） |
| `Protect-VHDX.ps1` | V-7：BitLocker-to-Go 加密/解锁 + 杀软申诉包骨架（附录 E） | 宿主 |
| `Deploy-To-USB.ps1` | V-8：四阶段交付（Stage1 底座→2 引擎→3 镜像→4 收口）+ Verify | 宿主 |
| `Bench-Perf.ps1` | V-9：顺序/4K/内存基准，门禁 ≥ 本地 SSD 85% | 宿主 |
| `Accept-Gate.ps1` | V-10：14 项验收登记表 Init/Report/Check（无实测一律 todo） | 任意 |

## 契约（与 launcher/ 对齐，改前先改两处）

- 心跳：VM 内 agent 监听 `127.0.0.1:47631`，宿主发 `PING`，agent 回 `READY pid=<n>`
- 回调：agent 主动连宿主 `127.0.0.1:47632`，`READY`/`EXIT`（EXIT = 请求卸盘）
- 环境注入：`VAR_RUNTIME_MODE`（vm/vbox/light）、`VAR_HOST_ADDR`（宿主回调地址）
- 探测缓存：`probe-cache.json`（launcher 目录，字段见 `launcher/src/probe.rs` ProbeReport）

## 能力诚实声明

- 引导器全程宿主零注册表写入、零启动项；Hyper-V 启用需用户确认提权。
- 轻量直跑档隔离弱（与宿主共内核），零残留由执行档容器兜底（residue-check --expect-clean）。
- UAC/安全桌面、内核级全屏独占 + 反作弊：不接管、不回收（详见 D-5 边界清单）。
