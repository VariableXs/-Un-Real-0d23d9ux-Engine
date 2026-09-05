# Variable Workstation — 随身 AI 开发工作站：完整架构与实施蓝图

> 版本：v1.0 蓝图 · 2026-09-05
> 载体：Variable — Private Space（本仓库）
> 定位：把 Variable 从「完全离线的私人桌面环境」升级为「**默认离线、出站显式授权**的
> 便携式 AI 开发工作站」——插上 U 盘 / 移动 SSD，在任意 Windows 电脑上双击展开，
> 获得一套带自有桌面、自有窗口管理器、预配置开发链与云端 AI 编程助手（Claude Code
> / Codex / Copilot CLI 等）的完整工作环境；退出即封存，拔盘零残留。

---

## 目录

- 第一章 · 愿景与重新定位
- 第二章 · 方案取舍：为什么放弃「本地大模型」路线
- 第三章 · 现状盘点：Variable 已有能力全景（哪些已做）
- 第四章 · 总体架构（七层模型）
- 第五章 · 核心子系统详细设计
  - 5.1 隔离启动档（Environment Profile · 环境变量重定向引擎）
  - 5.2 出站网络控制（域名白名单 · netconsent 演进）
  - 5.3 终端子系统（ConPTY 内置终端 + 第三方终端嵌入）
  - 5.4 VS Code Portable 集成与窗口嵌入
  - 5.5 工作台预设与布局快照（Workbench Presets）
  - 5.6 单文件容器存储（VHDX 起步 → 自研容器备援）
  - 5.7 凭证与密钥金库（Vault 2.0）
  - 5.8 便携工具链管理（Toolchain Registry）
  - 5.9 AI 工作流深度集成（Claude Code · MCP · Hooks）
  - 5.10 性能工程（IO 分级 · 缓存策略 · 设备适配）
  - 5.11 可扩展性设计（工具适配器协议 · Profile 生态）
- 第六章 · 数据流与完整生命周期
- 第七章 · 安全模型与威胁分析
- 第八章 · 实施路线图（W-0 → W-9，含验收标准）
- 第九章 · 已完成 / 待完成总表
- 第十章 · 测试与验收体系
- 第十一章 · 边界与如实声明（FAQ）
- 第十二章 · IPC 命令面与错误体系
- 第十三章 · 风险登记册
- 第十四章 · 运维手册（Runbook）
- 第十五章 · 批次工作分解（文件级改动清单）
- 附录 F · TAP 适配器完整示例
- 附录 G · 数据库 Schema 变更
- 附录 H · 启动器行为规格
- 附录 I · 与既有规格的衔接说明
- 附录 A · U 盘目录结构规范
- 附录 B · 环境变量重定向总表
- 附录 C · 配置文件格式定义
- 附录 D · 命令与快捷键速查
- 附录 E · 术语表

---

## 第一章 · 愿景与重新定位

### 1.1 一句话定位

**Variable Workstation = 装在 U 盘里的、秒开的、带云端 AI 编程助手的私人开发工作站。**

它不是虚拟机（不模拟硬件、不跑第二内核），不是双系统（不重启），也不是普通便携软件
（不只是一个编辑器）——它是一个**自包含的桌面环境发行版**：自带桌面 Shell、虚拟窗口
管理器、任务栏、文件管理器、终端、预配置工具链与凭证金库；AI 算力借云端
（Claude Code 等编程智能体），图形算力借宿主 GPU（仅做 UI 合成），存储算力借 U 盘
（单文件容器封存一切状态）。

### 1.2 用户体验目标（验收时逐条可感）

1. **即插即用**：任意一台 Win10/11 x64 电脑，插入 U 盘，双击启动器，**1~2 秒内**
   全屏展开 Variable 桌面环境，无需安装、无需管理员权限、无需重启；
2. **状态复原**：桌面布局、VS Code 打开的文件与光标、终端工作目录、Git 分支、
   浏览过的记录——全部恢复到上次离开时的样子；
3. **AI 就绪**：终端输入 `claude` 即进入已登录的 Claude Code 会话；左侧 VS Code、
   右侧 AI 终端的「双联工作台」一键展开；AI 修改的文件在 VS Code 中实时可见；
4. **零宿主残留**：退出后宿主机上没有：任何一行源码、任何 API Key、任何
   `node_modules`、任何注册表键值、任何「最近使用」痕迹；
5. **拔盘安全**：运行中拔出有预警（已有能力），正常退出自动刷盘封存，
   容器加密拔盘即锁；
6. **环境隔离**：Variable 桌面盖住宿主，双击 Esc 瞬间切回宿主桌面回消息，
   再切回来一切如旧（已有能力）。

### 1.3 产品原则（继承并扩展）

Variable 既有八条设计原则（隐私即默认、隔离即安全、审美驱动、启动即进入、
无模板哲学、零 AI 依赖、可重现可追溯、如实降级、绝不伤害用户进程）在本蓝图中
继承不变，并做三点语义升级与一条新增：

- **「完全离线」→「默认离线，出站显式授权」**：零网络仍是出厂状态；联网变为
  用户逐域名授权的白名单行为，托盘常显授权面，一键全断。出站面精确可控，
  比笼统的"永不联网"更诚实也更有用；
- **「零 AI 依赖」→「本地规则引擎 + 可选云端 AI」**：词典/推演/翻译等核心功能
  永不依赖任何在线服务；云端 AI 是**附加在开发工作流上的可选层**，拔掉它
  Variable 依然完整；
- **「便携」从「数据目录」升级为「单文件容器」**：整个工作台（系统配置 + 工具链 +
  项目 + 凭证）封存在一个可加密的容器文件里；
- **新增「零宿主污染」**：不仅是"不写宿主磁盘"，还包括环境变量级隔离——任何
  在工作站内启动的进程，其配置/缓存/日志路径被重定向进容器，宿主的
  `USERPROFILE` 对它形同虚设。

### 1.4 不做什么（负面清单，与用户预期对齐）

- ❌ 不做内核、驱动、页表、文件系统过滤驱动（用户态边界，见第十一章）；
- ❌ 不做本地大模型推理（需要大显存与高速盘，与"任意电脑可用"冲突；
  未来可作为可选插件，不在主线路线上）；
- ❌ 不做纯 GPU 自绘 UI（WebView2 渲染已满足 120Hz 丝滑需求；
  自研合成器仅作为远期实验分支）；
- ❌ 不承诺"屏蔽 Win 键 / Ctrl+Alt+Del"（用户态做不到，见 11.2）；
- ❌ 不承诺穿透企业防火墙/审计（代理可配置，但对抗审计不在此产品目标内）。

---

## 第二章 · 方案取舍：为什么放弃「本地大模型」路线

原始「方案四」设想了本地大模型 + GPU 直通；最终选择云端 AI 编程工具路线。
这不是妥协，而是基于约束的工程决策，逐维对比如下：

| 维度 | 本地大模型路线 | 云端 AI 路线（本蓝图） | 裁决 |
| --- | --- | --- | --- |
| 宿主硬件要求 | RTX 3060+ / 12GB+ 显存，否则不可用 | 集显/轻薄本均可 | 云端胜：达成"任意电脑"目标 |
| 存储体积 | 模型权重 100~500GB | 系统+工具链+项目 15~30GB | 云端胜：普通 U 盘即可 |
| IO 压力 | 权重加载需要 200MB/s+ 持续读 | 纯代码/配置读写，随机小 IO | 云端胜：U 盘可胜任 |
| AI 能力上限 | 7B~14B 量化模型，编码能力有限 | Claude 级前沿模型 | 云端胜：能力差一个量级 |
| 离线可用性 | 完全离线可用 | 无网即无 AI（但 IDE/工具链照常） | 本地胜：如实写明边界 |
| 长期成本 | 一次购卡，电费 | API 订阅费 | 视用户而定，如实列出 |
| 隐私边界 | 代码不出机器 | 代码上传至 AI 供应商（需用户知情授权） | 本地胜：**必须显式告知** |
| 工程复杂度 | GPU 直通/显存管理/推理引擎 | HTTPS + CLI 集成 | 云端胜：低一个数量级 |

**裁决结论**：两条路线服务于不同人群。本蓝图服务"**带着完整开发环境随处办公**"
的主力场景（云端 AI）；本地大模型降级为远期可选插件（对持有高端显卡的用户，
以 Variable 第三方应用 + Job Object 的既有通道接入即可，不占用主线工程量）。

**隐私的如实声明（写入产品 UI，不可省略）**：启用云端 AI 后，用户在 AI 会话中
主动提供的上下文（打开的文件、终端输出、对话）将发送至对应供应商服务器。
Variable 的责任是：① 让这件事**显式发生**（白名单 + 首次确认 + 状态常显）；
② 让它**可随时关闭**（一键断网）；③ 除用户主动发起的会话外**零静默出站**
（遥测为零，这一条继续用审计工具强制保证）。

---

## 第三章 · 现状盘点：Variable 已有能力全景（哪些已做）

以下逐项核对当前仓库（main 分支）的真实代码状态，作为路线图的"地基清单"。

### 3.1 桌面环境层（完成度：95%，直接可用）

| 能力 | 状态 | 代码位置 | 对 Workstation 的意义 |
| --- | --- | --- | --- |
| 全屏桌面 Shell（覆盖系统任务栏） | ✅ 已完成 | `DesktopShell.tsx` / `avoidTaskbar` 设置 | 工作站的"全屏独立显示" |
| 启动仪式（真实进度） | ✅ 已完成 | `boot.rs` / `BootScreen.tsx` | 1~2 秒冷启动的体感基础 |
| VWM 虚拟窗口管理器 | ✅ 已完成 | `vwm.ts` / `VirtualWindowManager.tsx` / `VirtualWindowFrame.tsx` | 承载 VS Code / 终端 / 一切应用 |
| 边缘贴靠（半屏/1/4/最大化） | ✅ 已完成 | `snap.tsx` / VWM | 左编辑器右终端的双联布局 |
| 同应用多开 | ✅ 已完成 | `taskbarClickVwm` / forceNew | 多项目并行 |
| Win+Tab 切换器 / Alt+Tab 轮转 | ✅ 已完成 | `WintabSwitcher.tsx` | 窗口导航 |
| 任务栏（运行态/小组件/托盘/时钟） | ✅ 已完成 | `Taskbar.tsx` | 方案中的"自研任务栏" |
| 开始菜单（拼音搜索/最近使用） | ✅ 已完成 | `StartMenu.tsx` / `pinyin.ts` | 应用与项目启动器 |
| 桌面图标 + 文件架（六色/嵌套） | ✅ 已完成 | `DesktopIcons.tsx` / `ShelfFlyout.tsx` | 项目快捷入口 |
| 文件管理器（多标签/VWM 内嵌） | ✅ 已完成 | `ExplorerWindow.tsx` / `vwm.ts openVwmSystem` | 容器内文件浏览 |
| 回收站（系统集成） | ✅ 已完成 | `recycle.rs` | 误删保护 |
| 通知中心 / 快捷面板 / 硬件面板 | ✅ 已完成 | `DesktopShell.tsx` / `QuickPanel.tsx` / `hardware.rs` | CPU/内存/电量监控（方案要求） |
| 系统托盘常驻 | ✅ 已完成 | `tray.rs` | 后台驻留与快速返回 |
| 双 Esc 切环境 / Del+Backspace 退出 | ✅ 已完成 | `kbdhook.rs` | "瞬间回到宿主桌面"的核心体验 |
| 星空/极光背景 + 四空间图标 | ✅ 已完成 | `features/background` / `AppGlyphs.tsx` | 审美驱动 |
| 三语 i18n（简/繁/英） | ✅ 已完成 | `i18n/` | 面向任意宿主电脑的用户 |
| 壁纸系统（静态/轮播/视频/网页/系统让位） | ✅ 已完成 | `WallpaperLayer.tsx` / `wallpaper.rs` | 工作台个性化 |

### 3.2 第三方应用子系统（完成度：85%，两处待强化）

| 能力 | 状态 | 代码位置 | 备注 |
| --- | --- | --- | --- |
| 登记 exe/lnk/bat/cmd + 便携性三级 | ✅ 已完成 | `launcher.rs` | VS Code Portable 登记即用 |
| 参数列表启动 / ShellExecuteExW 启动 | ✅ 已完成 | `launcher.rs` | 无 shell 拼接，安全 |
| 管理员运行（ShellExecuteW runas） | ✅ 已完成 | `launcher.rs` | UAC 场景 |
| 开始菜单扫描 + 原生图标提取 | ✅ 已完成 | `launcher.rs` / `icon_dataurl` | 自动发现宿主已装工具 |
| 进程树窗口捕获 + SetParent 嵌入 | ✅ 已完成 | `embed.rs` | **VS Code 嵌入的直通通道** |
| 30 秒慢启动等待 + 超时不杀进程 | ✅ 已完成 | `embed.rs` | Electron 应用嵌试错成本低 |
| 边界同步/隐藏/关闭/焦点控制 | ✅ 已完成 | `embed.rs` | 嵌入后随 VWM 移动缩放 |
| **环境变量重定向启动** | ❌ 未做 | — | **W-1 核心：零宿主污染的前提** |
| **启动前宿主状态快照/痕迹核查** | ❌ 未做 | — | W-6：退出时验证零残留 |

### 3.3 数据与安全层（完成度：80%）

| 能力 | 状态 | 代码位置 | 备注 |
| --- | --- | --- | --- |
| SQLite WAL 数据库（版本化迁移） | ✅ 已完成 | `db.rs` | 记录/导图/设置存储 |
| 数据目录便携模式 | ✅ 已完成 | `state.rs bootstrap_dirs` | `exe目录\data` 模式已支持 |
| U 盘拔出保护（1s 轮询 + WAL checkpoint） | ✅ 已完成 | `usb.rs` | 运行中拔盘预警 |
| 保险箱（本地加密 + 独立口令） | ✅ 已完成 | vault 模块 | **W-5 升级为凭证托管中枢** |
| 联网确认弹窗（单次） | ✅ 已完成 | `netconsent.rs` | **W-2 升级为域名白名单** |
| HTML 净化 / 路径防穿越 | ✅ 已完成 | `sanitize.ts` / `pathGuard.ts` | 安全基建 |
| 备份/恢复/恢复文件/导出导入 | ✅ 已完成 | `backup.rs` / `export.rs` / `settings_cmd.rs` | 容器化前的数据保障 |
| **单文件容器（VHDX/自研）** | ❌ 未做 | — | **W-7 核心** |
| **凭证托管的子进程注入** | ❌ 未做 | — | W-5：Token 经环境变量下发给 claude |

### 3.4 开发工具层（完成度：30%，最大增量区）

| 能力 | 状态 | 备注 |
| --- | --- | --- |
| 代码阅读与改写（PVCCE） | ✅ 已完成 | 本地词典大白话引擎，写回/回滚 |
| 写作空间（富文本/媒体/搜索/引用） | ✅ 已完成 | 文档与笔记 |
| **内置终端（PTY）** | ❌ 未做 | **W-3 核心：ConPTY / 嵌入 Windows Terminal** |
| **便携工具链注册表** | ❌ 未做 | W-4：Node/Python/Git portable 统一管理 |
| **AI CLI 集成（claude 等）** | ❌ 未做 | W-5：登录态/配置目录进容器 |
| **工作台预设** | ❌ 未做 | W-6：一键展开双联布局 |
| Git（便携版 + SSH 随盘） | ❌ 未做 | W-4 的一部分 |

### 3.5 工程质量基线（可直接依赖）

- 后端 `cargo test` 36 项 / 前端 Vitest 218 项 / `tsc --noEmit` 严格模式全绿；
- `tools/audit.cjs` 静态审计 IPC 命令面与 i18n 键完整性；
- NSIS/MSI/便携目录三通道打包（`build-windows.bat`）；
- 实机验收文化：每个批次实机操作验证后记入 `project_memory.md`。

**盘点结论**：工作站蓝图不是新项目，而是给 Variable 加上一个「开发工具层」和
两个「隔离层」（环境变量隔离、存储容器隔离）。桌面/窗口/嵌入/便携/安全五大
既有子系统全部直接复用。

---
## 第四章 · 总体架构（七层模型）

```
┌─────────────────────────────────────────────────────────────────────────┐
│  L7  体验层（全部已完成 · 复用 Variable 桌面环境）                        │
│   自研桌面 Shell · VWM 虚拟窗口 · 任务栏/开始菜单 · 文件管理器 ·          │
│   通知中心 · 快捷面板 · 硬件面板 · 双 Esc 环境切换 · 星空/极光视觉        │
├─────────────────────────────────────────────────────────────────────────┤
│  L6  工作台编排层（W-6 新增）                                             │
│   Workbench Presets（布局快照）· 会话恢复（VS Code/终端状态）·            │
│   项目启动器（.code-workspace/.sln 感知）· 任务栏 API 连接状态灯          │
├─────────────────────────────────────────────────────────────────────────┤
│  L5  开发工具层（W-3/W-4/W-5 新增）                                       │
│   内置终端（ConPTY）· 便携工具链注册表（Node/Python/Git）·                │
│   AI CLI 集成（claude / codex）· MCP 服务托管 · AI 会话面板               │
├─────────────────────────────────────────────────────────────────────────┤
│  L4  隔离与凭证层（W-1/W-2/W-5 新增）                                     │
│   Environment Profile（环境变量重定向引擎）· 出站域名白名单 ·             │
│   Vault 2.0（凭证托管 → 子进程注入）· 零宿主污染核查器                    │
├─────────────────────────────────────────────────────────────────────────┤
│  L3  应用运行层（已有 + 扩展）                                            │
│   第三方应用嵌入（embed.rs 进程树捕获/SetParent）· 参数列表启动 ·          │
│   ShellExecute 通道 · 写作/导图/代码/命运四空间                           │
├─────────────────────────────────────────────────────────────────────────┤
│  L2  存储层（W-7 新增 · 现有数据目录为过渡形态）                          │
│   单文件容器（VHDX+BitLocker → 自研容器）· 便携数据目录 ·                 │
│   缓存分级（容器内 / 宿主临时盘（显式授权））· 备份/恢复/焚毁              │
├─────────────────────────────────────────────────────────────────────────┤
│  L1  宿主对接层（已有 + 小幅增强）                                        │
│   全屏窗口 · 原始输入轮询（kbdhook）· 宿主 GPU（D3D 合成加速）·           │
│   宿主网卡（经白名单出站）· U 盘卷监控（usb.rs）· 无需管理员权限           │
├─────────────────────────────────────────────────────────────────────────┤
│  L0  宿主硬件与 Windows（只读依赖）                                       │
│   [ CPU ] [ 内存 ] [ GPU（仅 UI 合成）] [ USB/SSD 存储 ] [ 网卡 ]         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.1 分层原则

- **L7 永不改写**：桌面环境是既成资产，工作站所有能力以 VWM 虚拟窗口与
  Tauri command 的形式「装进」现有环境，绝不另起炉灶；
- **L4 是本蓝图新增的灵魂**：环境变量重定向 + 域名白名单 + 凭证注入，
  三者共同构成「零宿主污染」与「凭证不落地」的保证；
- **L2 采用双形态过渡**：W-7 之前一切以便携数据目录运行（现状可用），
  容器化在其后整体迁入，迁入前后用户体验无差；
- **层间通信全部走既有通道**：前端 `lib/ipc.ts` 类型化调用 + Rust command +
  Tauri event，`tools/audit.cjs` 继续强制命令面一致性。

### 4.2 进程模型

```
Variable.exe（主进程 · Tauri）
├─ WebView2（desktop）──── 桌面 Shell + VWM + 全部 L4~L7 前端
├─ WebView2（app-code 等）─ 四空间（遗留回退通道）
├─ kbdhook 轮询线程 ─────── 双 Esc / Del+Backspace（Rust 侧，已有）
├─ usb 卷监控线程 ───────── 拔盘预警（已有）
├─ imwatch 轮询线程 ─────── IM 未读提醒（已有，可选）
├─ conpty 守护 ──────────── 内置终端会话（W-3 新增，每会话一对管道）
└─ 子进程（全部经 L4 Environment Profile 启动）
    ├─ Code.exe（VS Code Portable）── 主进程 + GPU/渲染/扩展宿主子进程
    ├─ node → claude（AI CLI 会话）
    ├─ git / python / cargo …（工具链）
    └─ wt.exe（可选：Windows Terminal 嵌入模式）
```

要点：**所有子进程只有一个入口**——`launcher.rs` 的启动通道；L4 的环境变量
重定向在 `Command::envs()` 一次注入，天然继承给全部后代进程。VS Code 自身
的 GPU/扩展宿主子进程由 Electron 自行管理，Variable 只跟踪主进程 pid 树
（进程树捕获已经是这个模型）。

### 4.3 关键设计决策（ADR 摘要）

| # | 决策 | 理由 | 备选与放弃原因 |
| --- | --- | --- | --- |
| ADR-1 | VS Code 用 Portable 版 + 窗口嵌入，不自研编辑器 | 编辑器是十年的工程量；VS Code Portable 天然支持 data 目录隔离 | 网页版 code-server（引入服务端复杂度）；Monaco 自组（丢插件生态） |
| ADR-2 | 终端首选 ConPTY 自绘（xterm.js），次选嵌入 Windows Terminal | 自绘可控且与 VWM 视觉统一；嵌入是兜底（Windows Terminal 有兼容性坑） | 纯嵌入（视觉割裂 + winget 依赖） |
| ADR-3 | 凭证经环境变量注入子进程，绝不写宿主磁盘 | Claude Code 支持 `CLAUDE_CONFIG_DIR`；环境变量继承是 OS 原生机制 | 写入容器配置文件（增强加密但仍落盘；作为 W-5 第二阶段） |
| ADR-4 | 容器首选 VHDX，自研容器为备援 | VHDX 零开发成本 + BitLocker 成熟；自研仅在"禁止挂载"环境作为反封锁备份 | 直接自研（错过 VHDX 的零成本窗口期） |
| ADR-5 | 出站控制做在「授权面」而非「过滤驱动」 | 用户态拦不了别家进程的包；我们只管自己启动的进程 + 显式授权清单 | LSP/WFP 过滤驱动（需要驱动签名，违背便携） |
| ADR-6 | node_modules 默认在容器内，允许显式授权迁至宿主临时盘 | 廉价 U 盘随机 IO 差；但零残留是默认承诺，迁移必须用户点头 | 强制全在容器（低端盘体验崩坏） |

---

## 第五章 · 核心子系统详细设计

### 5.1 隔离启动档（Environment Profile · 环境变量重定向引擎）

> 状态：❌ 未做 → **W-1 批次实施** · 这是整个「零宿主污染」承诺的技术前提。

#### 5.1.1 问题陈述

现代 CLI 工具默认把配置、凭证、缓存写到 `%USERPROFILE%`（宿主机的
`C:\Users\<别人>\`）：`~/.claude`（Claude Code 登录态）、`~/.gitconfig`、
`~/.ssh`、`~/.vscode`、`%APPDATA%\npm`、`~/.cargo`……在别人电脑上用完，
这些就是全部的遗留痕迹与凭证泄露面。

#### 5.1.2 设计：登记项级 Profile

```jsonc
// apps.json 登记项新增字段（向后兼容，缺省 = 不重定向）
{
  "id": "tp-vscode",
  "name": "VS Code",
  "path": "W:\\Tools\\VSCode\\Code.exe",
  "profile": {
    "env": {                                  // 静态键值
      "VSCODE_PORTABLE": "W:\\Tools\\VSCode\\data",
      "ELECTRON_DISABLE_SECURITY_WARNINGS": "1"
    },
    "redirect": {                             // 语义重定向（见 5.1.3）
      "home": "W:\\Home",
      "appdata": "W:\\Home\\AppData",
      "tmp": "W:\\tmp"
    },
    "fsPolicy": {                             // 文件系统策略（W-6 核查器用）
      "denyWrite": ["%USERPROFILE%", "%TEMP%"],
      "allowWrite":  ["W:\\"]
    }
  }
}
```

#### 5.1.3 语义重定向表（Rust 侧 `profile.rs` 统一实现）

| 语义键 | 展开为的环境变量 | 效果 |
| --- | --- | --- |
| `home` | `HOME`、`USERPROFILE` | git/ssh/claude/cargo/pip 的 `~` 全部落盘 |
| `appdata` | `APPDATA`、`LOCALAPPDATA` | npm 缓存、VS Code 用户数据、多数工具的配置目录 |
| `tmp` | `TMP`、`TEMP` | 临时文件不出盘 |
| `sshDir` | （注入 Git 命令参数 / `GIT_SSH_COMMAND`） | `~/.ssh` 随盘携带 |
| `xdg` | `XDG_CONFIG_HOME`、`XDG_CACHE_HOME` | 跨平台工具的约定目录 |

Rust 启动通道（`spawn_detached` / `shell_launch`）增加可选 `profile` 参数：
在 `Command` 上追加 `envs()`；`USERPROFILE`/`HOME` 的重定向对所有 std 子进程
直接生效。**嵌入通道（embed.rs）同步支持**——先按 Profile 注入启动环境，
再做窗口捕获，顺序不变。

#### 5.1.4 进程级验证（防"漏网之鱼"）

- 启动后 3 秒，用 `Toolhelp32` 快照枚举 pid 树，逐进程读取其环境块
  （`QueryFullProcessImageName` + NtQueryInformationProcess 属于内核态接口，
  用户态改用间接手段：见 W-1 验收标准的"探针法"）；
- 探针法：在容器内放置哨兵文件，Profile 生效的进程会读写哨兵路径；
  同时 W-6 的「零残留核查器」在退出时扫描宿主 `USERPROFILE` 增量
  （快照差分），发现新落盘文件 → 通知中心如实列出（不静默）；
- 该核查是**检测性**而非**阻止性**（用户态无法拦截任意 Win32 API 写盘），
  如实写进第十一章边界。

#### 5.1.5 验收标准（W-1）

1. 以 Profile 启动的 git：`git config --global -l` 读取的是盘内
   `W:\Home\.gitconfig`；
2. `npm install` 后宿主 `%APPDATA%\npm-cache` 无增量；
3. `claude` 登录后宿主 `~/.claude` 不存在；
4. 未配置 Profile 的旧登记项行为与现状完全一致（回归零破坏）；
5. cargo test：Profile 展开器单测（20+ 用例：变量缺失/循环/越界路径拒绝）。

---

### 5.2 出站网络控制（域名白名单 · netconsent 演进）

> 状态：🟡 单次弹窗已有（`netconsent.rs`）→ **W-2 批次升级为域名白名单**。

#### 5.2.1 设计原则

- **控制自己的进程，而非宿主**：Variable 无法也不应过滤宿主流量（用户态
  无过滤点）；出站控制的作用域 = Variable 自身 + Profile 启动的子进程；
- **默认拒绝**：出厂状态零白名单，任何出站先弹确认；
- **逐域名授权**：确认粒度从"允许联网"细化到"允许 api.anthropic.com"；
- **状态常显**：托盘/任务栏常驻「出站面板」：已授权域名数、当前活跃连接数
  （仅本进程树）、一键全断；
- **不静默**：每个新域名首次触达都弹窗；白名单在设置页可视化增删。

#### 5.2.2 技术实现（用户态诚实方案）

1. **进程内 HTTP 客户端约束**（Variable 自身）：所有 `reqwest` 调用统一过
   `netconsent::guard(host)`——未授权域名返回 `NetDenied` 错误 → 前端弹确认；
   host 校验拒绝 localhost/环回/私有/保留地址（遵循安全约束），仅放行
   http/https；
2. **子进程出站的「软控制」**：Profile 支持注入代理环境变量
   （`HTTPS_PROXY=http://127.0.0.1:<port>`），Variable 启动一个本地回环
   转发器（可选组件 W-2b），仅转发白名单域名并记录审计日志；子进程不尊重
   代理变量时退化为**授权信任 + 事后审计**（如实声明）；
   - 回环转发器只监听 `127.0.0.1`，遵循"拒绝非回环绑定"的安全约束；
3. **审计日志**：每次白名单命中的出站记录（时间/域名/进程/字节数）写入
   `logs/net.log`，设置页可查看——**用户能看到每一次 AI 请求何时发生**；
4. **一键全断**：托盘菜单 + 快捷键（Ctrl+Shift+D 旁新增 Ctrl+Shift+X）→
   清空白名单运行位 + 终止转发器；已建立的 AI CLI 会话会自然报错，
   前端提示"出站已断开"。

#### 5.2.3 默认白名单建议（首次确认后写入）

| 域名 | 用途 | 默认 |
| --- | --- | --- |
| `api.anthropic.com` | Claude Code API | 首次弹窗授权 |
| `console.anthropic.com` / `claude.ai` | OAuth 登录流 | 首次弹窗授权 |
| `github.com` / `api.github.com` | git 远端 / Copilot | 首次弹窗授权 |
| `marketplace.visualstudio.com` | VS Code 扩展市场 | 首次弹窗授权 |
| `objects.githubusercontent.com` | 扩展/仓库下载 | 随上一条授权 |
| `registry.npmjs.org` / `pypi.org` | 包管理 | 首次弹窗授权 |
| 其余一切 | — | 拒绝并记录 |

#### 5.2.4 验收标准（W-2）

1. 未授权域名：Variable 内发起请求 → 弹窗；拒绝 → 请求不发生且记录；
2. 授权 `api.anthropic.com` 后，`claude` 会话正常工作；托盘面板显示
   "1 个域名已授权 · 本会话 42 次出站"；
3. 一键全断后 1 秒内新请求全部失败，UI 如实提示；
4. `net.log` 可完整回放本会话全部出站；
5. 审计工具新增检查：源码中直连 URL 必须全部经过 `netconsent::guard`。

---

### 5.3 终端子系统（ConPTY 内置终端 + 第三方终端嵌入）

> 状态：❌ 未做 → **W-3 批次实施** · 方案中 `claude` 交互终端的载体。

#### 5.3.1 两条实现路径（并行推进，自绘为主线）

**路径 A：ConPTY + xterm.js 自绘终端（主线）**

```
┌─ VWM 虚拟窗口「Terminal」──────────────┐
│  xterm.js（前端渲染 · VT 序列解析）      │
│   ▲ write(paste数据流)  ▼ invoke        │
│  ipc: term_write / term_resize / …      │
│   ▲ ▼                                   │
│  Rust: conpty.rs                        │
│   CreatePseudoConsole → 两根管道         │
│   子进程：powershell / git-bash / pwsh  │
│   （经 Environment Profile 注入启动！）  │
└─────────────────────────────────────────┘
```

- Rust 侧 `conpty.rs`：`CreatePseudoConsole` API（Win10 1809+，无需管理员），
  每会话维护 HPC + 输入/输出管道 + 进程句柄；专用线程把输出推给前端
  （`term://output` 事件），前端按键经 `term_write` 写回；`ResizePseudoConsole`
  支持随虚拟窗口缩放；会话关闭走 `ClosePseudoConsole` + 进程收尾；
- **会话保活**：终端虚拟窗口最小化（display:none 保活）时会话不断——
  `claude` 的长时间任务在后台继续跑，任务栏图标显示"运行中"徽标；
- **工作目录恢复**：会话元数据（shell 类型、初始 cwd）入 SQLite，
  下次启动按项目恢复；`claude` 自身的会话历史由 `CLAUDE_CONFIG_DIR`
  随盘自动恢复（工具自带的能力，不重复造）；
- **视觉**：xterm.js 配 Variable 语义色板主题；字号/光标/滚动条与全局
  设置联动；渲染走 canvas（WebGL 渲染器可选）。

**路径 B：嵌入 Windows Terminal / 其他第三方终端（兜底）**

- 走既有 embed 通道：登记 `wt.exe` → 进程树捕获 → SetParent 进虚拟窗口；
- 用途：需要复杂终端特性（多标签/窗格拆分）的重度用户；
- 已知边界：Windows Terminal 对 SetParent 的兼容性需实测（Electron/Win32
  应用一般可行，个别版本有子窗口结构问题）——失败路径现有机制已保证
  不杀进程、可重试、回退独立窗口。

#### 5.3.2 与 AI 的接线

- 终端窗口标题栏带「AI 徽标」：检测到前台进程为 `claude`/`node` 时点亮；
- 终端右键菜单：「把选中输出发给 AI 解释」（复制 + 打开 AI 面板）；
- 剪贴板：终端选区复制 / 粘贴走系统剪贴板（与宿主互通，已有能力）；
- 危险命令防线：终端不对命令做静默过滤（那是假安全），但
  `rm -rf` / `format` 类命令回显前给一次性黄色横幅提示（可关闭）。

#### 5.3.3 验收标准（W-3）

1. 内置终端可跑 `claude` 完整交互会话（含流式输出、方向键历史、Ctrl+C 中断）；
2. 虚拟窗口缩放 → 终端内容 reflow 无乱码；最小化恢复后会话存活；
3. 退出工作站 → 重启 → 终端窗口恢复到上次 cwd，claude 会话可 `--continue`；
4. 嵌入 Windows Terminal 路径实机验证通过（或如实标注兼容性结论）；
5. 前后端各 10+ 单测（VT 序列转发、会话生命周期、resize 边界）。

---

### 5.4 VS Code Portable 集成与窗口嵌入

> 状态：🟡 登记通道已有、嵌入通道已有 → **W-4 批次做"开箱即用"封装**。

#### 5.4.1 集成形态

1. **获取**：用户把 VS Code Portable 解压到盘内 `Tools/VSCode/`（或从
   设置页的「工具链」页一键下载 zip 并校验哈希——走白名单出站）；
2. **登记**：首次启动检测到 `Code.exe` → 弹一次确认 → 自动登记 +
   Profile 注入（`VSCODE_PORTABLE` 指向盘内 data 目录）+ 图标提取；
3. **嵌入**：桌面双击 → openVwmApp 占位 → embed_launch 进程树捕获 →
   SetParent 进虚拟窗口 → 左右贴靠即得双联工作台；
4. **状态随盘**：VS Code Portable 的 `data/` 目录天然包含
   `user-data`（设置/快捷键/扩展状态）与 `extensions`（插件）——
   插件、主题、最近打开全部随 U 盘走，天然满足方案要求；
5. **与 Variable 数据联动**（增强项）：
   - 桌面文件架支持「项目架」：链接到 `W:\Projects\<name>`，
     点击 → 同时打开 VS Code（该目录）+ 终端（cwd 到该目录）；
   - PVCCE 代码空间增加「在 VS Code 中打开此文件」按钮
     （`code --goto file:line`，经工具链注册表调用）。

#### 5.4.2 已知风险与对策

| 风险 | 对策 |
| --- | --- |
| Electron 窗口嵌入兼容性（个别版本子窗口结构） | 现有 embed 失败路径：不杀进程、如实回退独立窗口；实机验证记录进批次日志 |
| VS Code 自动更新破坏 Portable 布局 | Profile 默认关闭自动更新（`update.mode: none` 写入 data 目录默认配置） |
| 扩展宿主进程写 `%USERPROFILE%\.vscode` | `VSCODE_PORTABLE` 已覆盖；W-6 核查器兜底检测 |
| 首次启动慢（扩展索引） | 启动仪式进度条接入「工具链预热」阶段（真实进度） |

#### 5.4.3 验收标准（W-4）

1. 新机器插盘 → 双击 VS Code 图标 → 嵌入成功，任务栏/Alt+Tab 无新窗口；
2. 主题/插件/最近项目全部为上次离开状态（data 目录随盘生效）；
3. 宿主 `C:\Users\<x>\.vscode` 与 `%APPDATA%\Code` 零增量；
4. 拖拽文件从文件管理器进 VS Code 编辑区可打开（OLE 拖放经嵌入窗口原生工作）；
5. Ctrl+Shift+P 等快捷键在嵌入模式下不与 Variable 全局键冲突
   （冲突表审计通过）。

---
### 5.5 工作台预设与布局快照（Workbench Presets）

> 状态：❌ 未做 → **W-6 批次实施** · 方案中"启动后自动平铺 VS Code 和终端"的实现。

#### 5.5.1 设计：布局即数据

```jsonc
// presets/developer.json —— 工作台预设（可导入导出，随盘）
{
  "id": "dev-duo",
  "name": "开发双联",
  "restore": true,
  "windows": [
    { "app": "tp:tp-vscode", "zone": "left-half",
      "args": { "cwd": "%workspace%/Projects/infinite" } },
    { "app": "builtin:terminal", "zone": "right-half",
      "args": { "shell": "git-bash", "cwd": "%workspace%/Projects/infinite" } },
    { "app": "builtin:terminal", "zone": "bottom-right-quarter", "minimized": true }
  ],
  "taskbarBadges": ["ai-session", "net-consent"]
}
```

- **zone 语法复用贴靠几何**：left-half / right-half / 四角 1/4 / bottom-dock，
  与 VWM snap 共享同一套工作区推导（任务栏四向感知，已有）；
- **应用标识统一**：`tp:<id>`（第三方/嵌入）、`builtin:terminal`（W-3）、
  `mode:<write|mind|code|fate>`（四空间）、`system:<explorer|recycle>`；
- **会话恢复**：每个窗口条目记录最小化/聚焦状态 + 应用级 args；
  退出时快照 → 启动时重放（顺序：先占位后嵌入，复用 embed 的异步节奏）；
- **多预设切换**：开始菜单「工作台」区 + 快捷键 Ctrl+Alt+W 轮换；
  出厂预置三个：`写作`（Write 全屏 + Mind 右 1/4）、`开发双联`、`科研`
 （Code + 终端 + Write）。

#### 5.5.2 验收标准（W-6）

1. 应用「开发双联」预设：1.5 秒内 VS Code 与终端各占半屏并完成嵌入/连接；
2. 退出重启后布局与聚焦状态完全复原；
3. 预设导出为 JSON 文件，可在另一台机器的 U 盘导入生效。

---

### 5.6 单文件容器存储（VHDX 起步 → 自研容器备援）

> 状态：❌ 未做（现为便携数据目录）→ **W-7 批次实施**。

#### 5.6.1 形态一（默认）：VHDX + BitLocker

```
启动器检测 → wsl? 否 → 挂载流程：
  diskpart /s attach.dscript（attach vdisk file=W:\Data\workstation.vhdx）
  → 分配盘符 V: → 若 BitLocker 加密 → unlock（密码来自 Vault，不回显）
  → Variable 数据根从 W:\Data\ 切换为 V:\Variable\
退出流程 → 刷盘 → dismount（detach vdisk）→ 容器锁定
```

- **权限现实**：`diskpart` attach vdisk 通常需要管理员权限——这与"免安装
  免管理员"目标冲突。**W-7 的真实策略**：
  1. 检测当前进程权限：若有管理员（用户右键"以管理员运行启动器"）→
     VHDX 全功能（加密/压缩/快照）；
  2. 无管理员 → 回退**便携数据目录形态**（现状能力，体验几乎一致，
     用目录加密替代容器加密：Vault 加密凭证 + 可选的 7z 加密归档备份）；
  3. 设置页如实标注当前形态与升降级路径——**绝不假装容器在跑**；
- VHDX 的 `Mount-VHD` PowerShell 路径依赖 Hyper-V 模块，不做首选；
  diskpart 是全系统自带组件，兼容性最好；
- **动态 VHDX**：初始 30GB 逻辑容量，物理按需增长，避免占满 U 盘。

#### 5.6.2 形态二（备援）：自研只读优先容器（W-8，远期）

面向"企业禁用 diskpart/VHDX"的反封锁场景，规格：

```
[SuperBlock 4KB] 魔数/版本/UUID/几何/校验
[Inode 区]      路径树 + 块索引 + 权限位 + 时间戳（SQLite 实现，成熟可靠）
[Block 位图]    分配状态
[数据块 64KB]   内容（可选 LZ4 块级压缩 + AES-GCM 段加密）
[日志区]        追加式事务日志（原子提交/回放）
```

- **诚实定位**：不做通用文件系统，只做**归档容器**——完整随机写支持有限
  （日志式追加 + 周期压实），读路径完备；VS Code 等工具实时写场景仍走
  形态一或数据目录形态；
- 用户态实现（纯 Rust，`AesGcm` + `lz4_flex`），零权限依赖；
- **数据安全底线**：容器写入必须先 WAL 后落盘，断电可回放；压实前双容器
  空间校验，杜绝"压实失败毁库"。

#### 5.6.3 通用容器服务（两种形态共享）

- `container.rs` 状态机：`Mounted / Unmounted / Locked / Error`；
- 生命线：复用 `usb.rs` 的卷监控——容器所在卷消失 → 立即进入保护态
  （挂起子进程 → 尽力刷盘 → 通知横幅），已有 WAL checkpoint 机制无缝衔接；
- 备份：容器级导出（复制文件）+ 库级备份（既有 `backup.rs`）双层。

#### 5.6.4 验收标准（W-7）

1. 管理员形态：挂载/解锁/退出锁定全程 < 3 秒；BitLocker 解锁密码仅经
   内存传递，进程转储外零明文；
2. 非管理员形态：自动回退数据目录 + 设置页如实显示"目录形态"；
3. 运行中强制拔盘 → 宿主无崩溃，重插后数据完整（WAL 回放验证）；
4. 容器内 `git clone` + `npm install` + VS Code 打开全链路可用。

---

### 5.7 凭证与密钥金库（Vault 2.0）

> 状态：🟡 保险箱已有 → **W-5 批次升级为凭证托管中枢**。

#### 5.7.1 威胁模型（驱动设计）

在别人电脑上使用，凭证面临三类威胁：
① 落盘残留（`~/.claude/.credentials.json` 写到宿主）；② 内存/剪贴板泄露
（低优先级，用户态无法防御同机管理员）；③ 网络中间人（HTTPS + 系统证书栈
已有保障）。**Vault 2.0 主攻 ①，如实声明 ② 不可完整防御。**

#### 5.7.2 设计

```
Vault（独立口令，Argon2 派生，AES-GCM，随盘）
  ├─ credential: anthropic  → 注入 CLAUDE_CODE_OAUTH_TOKEN / API_KEY
  ├─ credential: github     → GIT_TOKEN / SSH key 解锁口令
  ├─ credential: npm/pypi   → NPM_TOKEN
  └─ secret note: 任意文本（已有能力）
```

- **注入而非落盘**：启动 `claude` 时由启动通道把对应凭证作为环境变量
  注入子进程（`CLAUDE_CODE_OAUTH_TOKEN` 等）——凭证只存在于 Vault 密文与
  目标进程内存，**绝不写盘**；登录流（OAuth）产生的令牌落地路径经
  `CLAUDE_CONFIG_DIR` 指回盘内；
- **首次登录向导**：终端首跑 `claude` → 检测到无登录态 → 弹出引导：
  选择「浏览器 OAuth（临时放行 claude.ai 域名）」或「粘贴 API Key 入 Vault」；
- **自动锁定**：已有自动锁定机制沿用 + 新增"容器卸载/拔盘 → 立即清零
  内存中的明文凭证（zeroize）"；
- **审计**：Vault 每次解锁/注入记入日志（时间 + 用途，不含明文）。

#### 5.7.3 验收标准（W-5）

1. 新机器登录 Claude 后退出：宿主全盘搜 `.credentials` 零命中；
2. Vault 锁定状态下启动 claude → 提示解锁，不泄露存在性以外的信息；
3. 拔盘 → 内存中凭证清零（调试器验证段落不可见，作为尽力保证如实标注）。

---

### 5.8 便携工具链管理（Toolchain Registry）

> 状态：❌ 未做 → **W-4 批次实施** · 方案中"预配置开发链"的管理中枢。

#### 5.8.1 设计

```jsonc
// toolchains.json —— 工具链注册表（随盘）
{
  "node":    { "version": "22.x", "path": "W:\\Tools\\node", 
               "env": { "npm_config_prefix": "W:\\Tools\\node\\npm-global",
                        "npm_config_cache": "W:\\cache\\npm" } },
  "python":  { "version": "3.12", "path": "W:\\Tools\\python",
               "env": { "PIP_CACHE_DIR": "W:\\cache\\pip" } },
  "git":     { "version": "2.47", "path": "W:\\Tools\\git\\cmd",
               "env": { "GIT_CONFIG_GLOBAL": "W:\\Home\\.gitconfig" } },
  "claude":  { "kind": "npm-global", "entry": "claude",
               "configDir": "W:\\Home\\.claude" }
}
```

- **获取方式**：各工具均使用官方 portable 发行版（node zip / python
  embeddable + get-pip / PortableGit / rustup 自定目录）；设置页「工具链」
  提供下载清单（哈希校验 + 白名单出站）或"手动放入目录 + 扫描登记"；
- **PATH 组装**：启动器按注册表顺序合成子进程 PATH——**不修改宿主环境
  变量**，只影响 Variable 内启动的进程；
- **版本共存**：同工具多版本并存（node18/node22），预设可指定；
- **健康检查**：启动仪式新增「工具链自检」阶段：逐工具 `--version`
  探活（真实进度，超时如实标注离线不可用）。

#### 5.8.2 验收标准（W-4）

1. 宿主机不装 Node/Python/Git，工作站内 `node -v`/`git --version`/
   `python -V` 全部正常且版本随盘；
2. `where node` 在 Variable 外部（宿主 cmd）找不到盘内工具（零 PATH 污染）；
3. `claude` 命令可用且 `CLAUDE_CONFIG_DIR` 指向盘内。

---

### 5.9 AI 工作流深度集成（Claude Code · MCP · Hooks）

> 状态：❌ 未做 → **W-5（CLI 通路）/ W-6（体验集成）/ W-9（进阶）**。

#### 5.9.1 通路层（W-5）

- `claude` CLI 经工具链注册表安装（`npm install -g @anthropic-ai/claude-code`
  到盘内 npm-global）；登录态经 Vault/CLAUDE_CONFIG_DIR 随盘；
- 内置终端即为 AI 主战场：`claude` / `claude --continue` 直接可用；
- 出站白名单预置 `api.anthropic.com` 等条目（5.2.3）。

#### 5.9.2 体验层（W-6）

- **AI 会话面板**（任务栏徽标 + 通知中心分组）：检测前台进程为 AI CLI 时
  显示会话时长/Token 消耗（解析 CLI 的统计输出，可得则显示，不可得如实
  隐藏——不做假数据）；
- **AI 命令面板**（Ctrl+Alt+A）：快捷打开终端新会话 / 恢复上次会话 /
  切换项目上下文（`claude --resume`）；
- **文件联动**：AI 修改文件 → VS Code 自动刷新（同盘实时，无需额外机制）；
  Variable 的 PVCCE 空间可一键"重新扫描项目"读取新代码。

#### 5.9.3 进阶层（W-9）

- **MCP 服务托管**：`~/.claude/settings.json` 中配置的 MCP 服务器由
  工作站统一拉起（同样经 Profile 注入），并纳入进程树监控；
- **Hooks 联动**：利用 Claude Code hooks，在 AI 写文件后触发 Variable 的
  通知中心提示（"AI 修改了 3 个文件"）；
- **多智能体并跑**：多终端虚拟窗口各跑一个会话，VWM 多开天然支持；
- **对话存档**：Claude Code 的会话历史本就随 `CLAUDE_CONFIG_DIR` 落盘，
  自动获得"随盘带走"能力，无需自研同步。

#### 5.9.4 验收标准（W-5/6 汇总）

1. 全新宿主机 → 插盘 → 启动 → `claude` 三步内进入可用会话（白名单一次授权）；
2. AI 会话在终端最小化期间持续运行；退出重启后 `--continue` 恢复上下文；
3. 断网状态：CLI 如实报错，Variable 其余功能零影响。

---
### 5.10 性能工程（IO 分级 · 缓存策略 · 设备适配）

> 状态：🟡 部分既有（WAL checkpoint / 启动真实进度）→ **W-4/W-7 批次内实施**。

#### 5.10.1 存储设备分级（启动时探测，策略随级）

| 级别 | 判定（USBStor/NVMe/旋转延迟探测） | 策略 |
| --- | --- | --- |
| 高速 SSD（USB3.2 Gen2 / NVMe 盒） | 顺序 ≥ 500MB/s | 全量容器内：node_modules、缓存、构建产物直接落盘 |
| 普通 U 盘（USB3.0） | 顺序 60~200MB/s，随机差 | 容器内禁 npm 缓存 → 显式授权迁宿主临时盘；构建产物目录警告 |
| USB2 / 低速 | 顺序 < 40MB/s | 仅文档场景：提示不进行重依赖开发；AI/写作功能全开 |

- **缓存分级表**（默认策略，用户可改）：

| 缓存 | 默认位置 | 理由 |
| --- | --- | --- |
| npm/pip/cargo registry 缓存 | 宿主临时盘（显式授权）或容器 `cache/` | 体积大、可再生 |
| `node_modules` | 跟随项目（容器内） | 零残留承诺 |
| VS Code 扩展 | 容器内（随盘是需求） | 体积可控 |
| 着色器/字体缓存 | 宿主临时盘（可再生，零隐私） | 加速冷启动 |
| 模型/权重 | ❌ 本蓝图无本地模型 | — |

- **写放大治理**：SQLite WAL checkpoint 周期与拔盘保护联动（已有）；
  git 启用 `core.fscache`；构建工具缓存目录统一收敛；
- **首次插入优化**：工具链哈希清单 + 单文件打包分发（.tar.zst 自解包）
  替代海量小文件拷贝，把"装环境"从小时级压到分钟级。

#### 5.10.2 UI 性能预算（硬指标）

| 指标 | 预算 | 保障手段 |
| --- | --- | --- |
| 冷启动到桌面可用 | ≤ 2s（SSD）/ ≤ 4s（U 盘） | 启动仪式真实进度；懒加载四空间入口 |
| VS Code 键入延迟 | 无感（WebView2 与 Electron 各自 GPU 合成） | 不引入自绘合成器（ADR 落地的直接收益） |
| 终端流式输出 | 120fps 滚动无撕裂 | xterm.js canvas 渲染 + 批量 flush（16ms 合帧） |
| 内存水位 | Variable 本体 ≤ 600MB；含 VS Code ≤ 2.5GB | 硬件面板实时可见（已有）；超阈值通知提示 |
| 空闲 CPU | ≤ 1%（桌面态） | kbdhook 30ms 轮询实测占比极低；星空 worker 随 `bgTier` 降级 |

#### 5.10.3 基准套件（进 CI 手册）

- `tools/bench.cjs`：冷启动计时（boot 事件时间轴，真实数据）、
  终端吞吐（cat 10MB 文件计时）、容器挂载耗时、npm install 对比
  （容器 vs 授权临时盘）；每次批次跑分记录进批次日志。

---

### 5.11 可扩展性设计（工具适配器协议 · Profile 生态）

> 状态：架构约束（自 W-1 起强制）——所有新能力以"协议 + 适配器"接入。

#### 5.11.1 工具适配器协议（Tool Adapter Protocol, TAP）

任何 AI CLI / 工具接入工作站只需声明一个 JSON 适配器（而非改 Rust 代码）：

```jsonc
{
  "tool": "claude-code",
  "detect": { "path": "W:\\Tools\\node\\npm-global\\claude.cmd" },
  "launch": { "entry": "claude", "shell": true },
  "credential": { "vaultId": "anthropic",
                  "env": ["CLAUDE_CODE_OAUTH_TOKEN"] },
  "config":   { "configDirEnv": "CLAUDE_CONFIG_DIR" },
  "net":      { "domains": ["api.anthropic.com", "claude.ai"] },
  "ui":       { "badge": "ai-session", "titleProbe": "claude" },
  "session":  { "resumeArgs": ["--continue"] }
}
```

- TAP 覆盖五个关注点：探测/启动/凭证/网络/UI——新工具（Codex CLI、
  Gemini CLI、aider、自定义脚本）零 Rust 改动接入；
- 适配器随盘分发（`presets/adapters/`），社区可贡献；
- 内置适配器：claude-code、codex、copilot-cli、aider、普通 shell。

#### 5.11.2 Profile 生态

- 工作台预设（5.5）× 工具适配器（5.11.1）× 工具链版本（5.8）三者正交；
- 导出物 = 单个 `.varpack`（zip：预设 JSON + 适配器 JSON + 工具链清单，
  绝不打包代码与凭证）——分享配置不分享隐私；
- 版本化：全部 JSON 带 `schemaVersion`，向后兼容读取（沿用 `.mindmap`
  的 formatVersion 惯例）。

#### 5.11.3 插件边界（如实）

- 不做运行任意第三方原生插件的机制（那是引入供应链攻击面）；
  扩展 = 声明式适配器 + 便携工具（它们本来就是独立进程）；
- Variable 自身的扩展点仍是 Tauri command（经审计的命令面）。

---

## 第六章 · 数据流与完整生命周期

### 6.1 冷启动时序（目标 ≤ 2s @ SSD）

```
t=0     双击 SystemLauncher（Variable.exe / 未来薄壳）
t+0.2s  真实加载：db 迁移(快) / 容器挂载检查 / 工具链自检(并行)
t+0.8s  启动仪式退出编排：字母落位 → 任务栏展开 → 图标交错淡入
t+1.2s  桌面就绪；若启用预设 → 重放布局（占位窗口先行）
t+1.5s  VS Code 启动（异步）→ embed 进程树捕获 → SetParent 入位
t+2.0s  终端会话恢复（cwd/环境注入）；状态栏点亮"就绪"
```

- 每步真实计时经 `boot://event` 推送（无合成时间线，已有机制）；
- 工具链自检超时（盘慢/损坏）→ 如实标注并继续（不阻塞桌面）。

### 6.2 工作中（常驻行为）

- **键盘**：kbdhook 30ms 轮询双 Esc/Del+Backspace（不吞键）；
- **卷监控**：usb.rs 1s 轮询；容器卷消失 → 保护态（挂起子进程/尽力刷盘/
  横幅）；
- **出站**：仅白名单域名 + 审计日志；AI 会话流量可见；
- **通知**：IM 提醒（可选）、AI 修改文件提示（W-9）、备份提醒。

### 6.3 退出时序（「保存并退出」= Del+Backspace 或红灯退出）

```
1. 请求各虚拟窗口保存（VS Code hot-exit 自动；终端会话记录 cwd）
2. 挂起新任务 → 等待在飞写盘完成（超时 5s 如实报告未刷净项）
3. SQLite WAL checkpoint 全量（usb.rs 已有机制）
4. 布局快照写入 → 会话元数据写入
5. Vault 自动锁定 + 内存凭证 zeroize
6. 终止子进程树（WM_CLOSE 优雅 → 5s 后强杀残留，逐个如实记录）
7. 容器形态：detach vdisk（目录形态：卷安全刷写）
8. 关闭全屏窗口 → 宿主桌面恢复原状
```

### 6.4 拔盘（异常路径）

- 任何时刻拔出：卷监控立刻触发 → 保护态 → 宿主零崩溃；
- 重插后：WAL 回放 + 恢复文件列表（已有）→「上次未正常退出」横幅 +
  一键恢复；
- 容器形态：BitLocker 未解锁则数据不可读（拔盘即锁）。

### 6.5 痕迹核查（退出后自检，W-6）

- 退出前记录宿主 `USERPROFILE`/`TEMP` 关键目录快照；
- 下次插入时差分比对：发现上次会话的残留文件 → 通知中心列出
  （含路径），提供「清除（需用户确认）」与「标记为已知残留」两项；
- 核查报告可导出（对"零残留"承诺的持续验证）。

---

## 第七章 · 安全模型与威胁分析

### 7.1 资产分级

| 资产 | 等级 | 保护 |
| --- | --- | --- |
| API 凭证（Anthropic/GitHub Token） | 最高 | Vault 加密 + 环境变量注入 + 零落盘 + 拔盘清零 |
| 源码与项目 | 高 | 容器/盘内 + 宿主零写入 + 可选容器加密 |
| 个人写作/导图/推演数据 | 高 | 既有隐私体系（不变） |
| 工具链与配置 | 中 | 随盘 + 哈希校验分发 |
| 使用痕迹（何时在哪用过） | 低-中 | 不写宿主注册表/临时盘；运行期内存可见性如实声明 |

### 7.2 威胁矩阵（诚实版）

| 威胁 | 防御 | 残余风险（如实） |
| --- | --- | --- |
| 宿主机有恶意软件 | **无法防御**：同机管理员可读你进程内存 | 插陌生电脑=高风险行为，文档显著警告 |
| 误配置导致凭证落盘 | Profile 重定向 + W-6 核查器（检测性） | 检测滞后于发生（退出后才核查全量） |
| 网络中间人 | HTTPS + 系统证书栈 | 宿主装了根证书的审计环境可解密（如实声明） |
| AI 供应链（被诱导执行危险命令） | 不做静默命令过滤；危险命令横幅；Git 提交即留痕 | AI 是你授权的代理，最终责任在用户 |
| 物理丢失 U 盘 | BitLocker/容器加密 + Vault 独立口令 | 目录形态下依赖 Vault + 文件级加密 |
| 变相残留（宿主 Office 最近文件记录了你打开的路径） | 不经 OLE 打开宿主文档即无此痕；核查器覆盖主目录 | 深系统级 MRU（如 Prefetch）超出用户态能力，如实声明 |

### 7.3 安全工程纪律（沿用 + 新增）

- 沿用：HTML 白名单净化、路径防穿越、参数列表启动、白名单 host 校验
  （http/https only、拒 localhost/环回/私有/保留地址）、审计工具、
  失败不静默；
- 新增：适配器/预设 JSON 的 schema 校验（拒绝越界路径/未知名单域）、
  容器挂载路径拼写护栏（拒绝挂载点为宿主系统目录）、凭证 zeroize
  审计、`cargo test` 安全用例随批次增长。

---
## 第八章 · 实施路线图（W-0 → W-9，含验收标准）

> 批次沿用 Variable 的演进纪律：每批独立可用、实机验收、回归全绿
> （cargo test / tsc / vitest / build）、教训记入 `project_memory.md`。

### W-0 · 准备批次（0.5 天）✅ 本文档

- [x] 现状盘点与差距清单（第三章/第九章）；
- [x] 架构决策记录（ADR-1~6）；
- [x] 路线图与验收标准定稿。
- **验收**：本文档合入仓库并推送。

### W-1 · 隔离启动档（2~3 天）

**目标**：任何登记应用可带 Profile 启动，实现环境变量级零宿主污染。

任务清单：
- [ ] `profile.rs`：语义重定向表（home/appdata/tmp/ssh/xdg）展开器 +
      越界路径拒绝（挂到 `pathGuard` 同级强度）；
- [ ] `launcher.rs`：`tp_launch_inner` 支持 profile 参数（Command::envs 注入）；
      apps.json schema 扩展（向后兼容）；
- [ ] `embed.rs`：嵌入启动路径接入同一 Profile；
- [ ] 管理界面：LauncherManager 登记项的「隔离」编辑面板（env 表 +
      重定向开关 + 实时预览展开结果）；
- [ ] 测试：展开器单测 20+（缺失变量/循环/绝对越界/UNC 路径）。

**验收**：git/npm/claude 三工具在干净宿主机上运行后，`USERPROFILE`/
`%APPDATA%` 零增量；未配置 Profile 的旧项回归零破坏。

### W-2 · 出站域名白名单（2~3 天）

**目标**：从"单次确认"升级为"逐域名授权 + 状态常显 + 一键全断 + 审计日志"。

任务清单：
- [ ] `netconsent.rs`：白名单存储（settings KV）+ `guard(host)` 统一入口 +
      审计日志 `logs/net.log`；
- [ ] 回环转发器（可选组件）：仅 127.0.0.1 监听、白名单转发、审计打点；
- [ ] 出站面板 UI（快捷面板新增分区）：授权列表/活跃计数/一键全断；
- [ ] i18n 三语文案；audit.cjs 增加"直连 URL 必须经 guard"检查。

**验收**：5.2.4 全部通过；`claude` 实际会话在白名单授权下工作正常；
断网键 1 秒内生效。

### W-3 · 终端子系统（3~5 天，本蓝图最大单体）

**目标**：内置 ConPTY 终端虚拟窗口；嵌入 Windows Terminal 兜底。

任务清单：
- [ ] `conpty.rs`：CreatePseudoConsole 会话管理（开/写/读/resize/关）+
      专用输出线程 + 批量合帧推送；
- [ ] 前端：xterm.js 集成（Variable 主题）、VWM 窗口 `builtin:terminal`、
      剪贴板/选区/滚动回溯、随窗口缩放 reflow；
- [ ] 会话元数据（shell/cwd）入 SQLite，随预设恢复；
- [ ] 兜底：Windows Terminal 嵌入实机验证，结论如实记录；
- [ ] 危险命令横幅（可关闭）；AI 徽标（titleProbe）。

**验收**：5.3.3 全部通过；`claude` 全功能交互（流式/历史/中断）；
120fps 滚动预算达标。

### W-4 · 便携工具链 + VS Code 开箱即用（3~4 天）

**目标**：干净宿主机插盘即得 Node/Python/Git/VS Code/claude 全链。

任务清单：
- [ ] `toolchains.json` 注册表 + PATH 合成器（不污染宿主）；
- [ ] 工具链页：扫描登记/哈希校验下载（走白名单）/版本共存/健康自检
      （接入启动仪式真实进度）；
- [ ] VS Code Portable 一键登记（Profile 预置 + 禁自动更新默认配置）；
- [ ] `claude` 安装器（npm-global 到盘内 + CLAUDE_CONFIG_DIR 预置）；
- [ ] 性能：缓存分级策略落地（5.10.1 表）；bench.cjs 基线跑分。

**验收**：5.4.3 + 5.8.2 全部通过；冷启动 ≤ 2s @ SSD。

### W-5 · Vault 2.0 + AI 通路（2~3 天）

**目标**：凭证零落盘，`claude` 三步内可用。

任务清单：
- [ ] Vault 凭证条目类型（oauth token/api key/ssh 口令）+ 注入器
      （启动通道按 TAP 适配器 env 注入）；
- [ ] 首次登录向导（OAuth 临时放行 / Key 入库双路径）；
- [ ] 拔盘/锁定 → zeroize；解锁/注入审计；
- [ ] TAP 适配器协议实现 + 内置五个适配器（5.11.1）。

**验收**：5.7.3 + W-5 验收全部通过；宿主全盘搜 `.credentials` 零命中。

### W-6 · 工作台预设 + 会话恢复 + 痕迹核查（3~4 天）

**目标**：一键双联工作台；重启全状态复原；零残留可验证。

任务清单：
- [ ] 预设引擎（zone 几何复用 snap；应用标识路由 tp:/builtin:/mode:/system:）；
- [ ] 退出快照 → 启动重放（含 VS Code args/cwd、终端会话、聚焦/最小化态）；
- [ ] 出厂三预设 + 开始菜单「工作台」区 + Ctrl+Alt+W 轮换；
- [ ] 痕迹核查器（快照差分 + 通知中心呈现 + 确认清除）；
- [ ] 任务栏徽标（ai-session / net-consent）。

**验收**：5.5.2 全部通过；痕迹核查在故意制造残留的实验中能捕获并列出。

### W-7 · 单文件容器（3~5 天）

**目标**：管理员形态 VHDX+BitLocker；非管理员如实回退数据目录。

任务清单：
- [ ] `container.rs` 状态机 + diskpart 挂载/卸载脚本生成（转义护栏）+
      BitLocker 解锁（密码经内存，不落日志）；
- [ ] 数据根切换向导（目录 → 容器整体迁移 + 双份校验 + 失败回滚）；
- [ ] 生命线接入 usb.rs（容器卷消失 → 保护态）；
- [ ] 设置页形态指示（容器/目录，如实标注）。

**验收**：5.6.4 全部通过；迁移向导中途断电可安全回滚。

### W-8 · 自研归档容器（远期，按需）

**目标**：禁用 diskpart 环境的反封锁备援（只读优先 + 追加写日志）。

- 规格见 5.6.2；启动条件：有真实用户报告"必须用形态二"再做——
  **不预支工程量**（诚实的需求驱动）。

### W-9 · AI 深度集成（3 天 + 持续）

**目标**：MCP 托管、Hooks 通知联动、多会话并跑、会话面板。

- [ ] MCP 服务器随 Profile 统一拉起 + 进程树监控；
- [ ] AI 写文件 → 通知中心提示（hooks 桥）；
- [ ] 会话统计面板（可得则显示，不可得如实隐藏）；
- [ ] 多终端窗口多会话并跑压测。

### 里程碑总览

```
W-0 ✅ → W-1 隔离 → W-2 网络 → W-3 终端 → W-4 工具链/VSCode
       → W-5 凭证/AI → W-6 工作台 → W-7 容器 →（W-8 远期）→ W-9 深度
       ↑ 每批后产品都处于"独立可用"状态，不存在中间废墟
```

---

## 第九章 · 已完成 / 待完成总表

| # | 子系统 | 状态 | 批次 |
| --- | --- | --- | --- |
| 1 | 桌面 Shell / VWM / 任务栏 / 开始菜单 / 文件管理器 | ✅ 已完成 | — |
| 2 | 全屏覆盖 + 双 Esc 环境切换 + 托盘常驻 | ✅ 已完成 | — |
| 3 | 第三方登记/便携分级/扫描/图标 | ✅ 已完成 | — |
| 4 | 进程树捕获 + SetParent 嵌入 + 不杀进程兜底 | ✅ 已完成 | — |
| 5 | U 盘拔出保护 + WAL checkpoint + 恢复文件 | ✅ 已完成 | — |
| 6 | 保险箱 v1 / 联网确认 v1 / 备份恢复 | ✅ 已完成 | — |
| 7 | 四空间（写作/导图/代码/命运） | ✅ 已完成 | — |
| 8 | 隔离启动档（环境变量重定向） | ❌ 未做 | **W-1** |
| 9 | 出站域名白名单 + 审计 + 一键断网 | 🟡 v1 升级 | **W-2** |
| 10 | 内置终端（ConPTY/xterm.js） | ❌ 未做 | **W-3** |
| 11 | 便携工具链注册表 + VS Code 开箱即用 | ❌ 未做 | **W-4** |
| 12 | Vault 2.0 凭证注入 + TAP 适配器协议 | 🟡 v1 升级 | **W-5** |
| 13 | 工作台预设 + 会话恢复 + 痕迹核查 | ❌ 未做 | **W-6** |
| 14 | 单文件容器（VHDX → 自研备援） | ❌ 未做 | **W-7 / W-8** |
| 15 | AI 深度集成（MCP/Hooks/会话面板） | ❌ 未做 | **W-9** |
| 16 | 性能基准套件 bench.cjs | ❌ 未做 | **W-4** |
| 17 | 纯 GPU 自绘合成器 | ⏸ 搁置（有意） | 远期实验 |
| 18 | 本地大模型推理 | ⏸ 搁置（有意） | 可选插件 |

**量化概览**：以子系统数计，底盘完成度约 **80%**；以新增代码量估，
W-1~W-9 总增量约 8000~12000 行 Rust/TS（其中测试占 1/3），
预计 20~28 个全职工作日（单人）。

---

## 第十章 · 测试与验收体系

### 10.1 自动化分层

| 层 | 工具 | 覆盖 |
| --- | --- | --- |
| Rust 单测 | cargo test | Profile 展开器/白名单 guard/容器状态机/ConPTY 生命周期（新增 80+ 用例） |
| 前端单测 | Vitest | 预设引擎/适配器 schema 校验/终端组件行为（新增 40+ 用例） |
| 静态审计 | audit.cjs（扩展） | IPC 命令面 + i18n + 直连 URL 强制过 guard + 适配器 schema |
| 基准 | bench.cjs | 冷启动/终端吞吐/挂载耗时/npm 对比（每批记录） |

### 10.2 实机验收清单（每批次必跑）

1. **干净宿主**（无开发环境的 Win10/11）插盘全流程；
2. **痕迹核查**：退出后 `USERPROFILE`/`TEMP`/注册表 Run 键差分为零
   （故意制造残留的对抗用例另测）；
3. **断电/拔盘**：写入中强制拔出 → 重插恢复 → 数据完整；
4. **嵌入兼容**：VS Code / Windows Terminal / claude 三者嵌入或如实回退；
5. **回归**：四空间/壁纸/IM 提醒/导图等既有功能不受批次影响；
6. **三语**：新增全部 UI 文案 zh/zh-TW/en 齐全（audit 工具强制）。

### 10.3 安全对抗测试（W-5/W-6 后一次性）

- 凭证钓鱼场景：宿主预置 `~/.claude` 旧配置 → 工作站不读不写它；
- 白名单逃逸尝试：适配器声明外域名的连接尝试 → 审计日志可见 + UI 提示；
- 容器路径注入：挂载点/迁移目标填入系统目录 → 拒绝并报错。

---

## 第十一章 · 边界与如实声明（FAQ）

**Q: 这到底是不是"操作系统"？**
不是。它是**用户态桌面环境发行版**：没有内核、没有驱动、没有文件系统
过滤、没有进程隔离边界。它借用 Windows 的驱动/网络/安全模型，在其上
提供自有桌面与开发工作台。宣传与文档均按此定位，不夸大。

**Q: 能屏蔽宿主的 Win 键 / Ctrl+Alt+Del / 任务管理器吗？**
不能。这些由更高完整性级别的系统组件处理，用户态无法拦截。双 Esc 是
"快速遮蔽"（隐藏窗口），不是"锁定机器"。

**Q: 宿主能发现我在用它吗？**
能。任务管理器可见进程与内存/网络占用，Prefetch/事件日志层面有系统级
痕迹（用户态不可控）。我们承诺的"零残留"指：**不写宿主文件系统与注册表、
凭证不落地**，不承诺"运行期对宿主完全隐形"。

**Q: 没有网络时能用吗？**
全部桌面/四空间/本地工具链/编辑器照常；仅 AI 云端会话不可用（CLI 如实
报错）。这正是"默认离线"设计的意义。

**Q: 我的代码会被上传吗？**
只在你主动使用 AI 会话时，会话上下文按该工具的机制发送给其供应商
（首次授权时显著告知）。除此之外 Variable 自身零出站（审计工具保证）。

**Q: 公司电脑能用吗？**
功能可用性取决于该机器的策略：无管理员权限 → 容器形态自动回退目录形态；
出站防火墙 → 白名单域名若被封则 AI 不可用（如实报错）。对抗企业审计
不是产品目标。

**Q: 为什么不自研 GPU 合成器？**
WebView2 的渲染已满足 120Hz 丝滑（VS Code 本身就是 Electron/GPU 合成），
自研合成器投入产出比极差（见第二章）。远期仅作为实验分支。

**Q: 廉价 U 盘能跑吗？**
能开桌面与 AI；重依赖开发（node_modules 海量小文件）建议移动 SSD。
启动时会探测设备等级并给出建议（5.10.1），不静默让用户受苦。

**Q: 和直接拷贝一份 VS Code Portable 有什么区别？**
后者只是编辑器；前者是**带环境隔离、凭证金库、网络授权、会话恢复、
终端、AI 适配器和完整桌面体验**的整套工作台——并且全部能力可审计、
可导出、可迁移。

---

## 第十二章 · IPC 命令面与错误体系（W-1~W-9 新增命令规格）

> 全部新增 command 经 `lib/ipc.ts` 类型化 + `audit.cjs` 注册面强制对齐；
> 命名沿用 `tp_`/`wp_` 的模块前缀惯例，新增 `pf_`（Profile）、`net_`、
> `term_`、`tc_`（toolchain）、`vp_`（vault profile）、`wb_`（workbench）、
> `ct_`（container）。

### 12.1 命令清单

```rust
// ---- W-1 Profile（pf_）----
pf_expand(profile: ProfileSpec) -> ExpandedEnv        // 预览展开结果（编辑面板实时用）
pf_validate(spec: ProfileSpec) -> Vec<Violation>      // 越界/循环/缺变量校验

// ---- W-2 网络（net_）----
net_allow(host: String, scope: "once"|"persist") -> ()   // 授权（弹窗确认后）
net_deny_all() -> ()                                     // 一键全断
net_status() -> { allowed: Vec<Domain>, sessionHits: u64, lastHosts: Vec<Hit> }
net_revoke(host: String) -> ()
net_audit_tail(n: u32) -> Vec<AuditLine>                 // net.log 尾部回放

// ---- W-3 终端（term_）----
term_spawn(shell: "pwsh"|"gitbash"|"cmd", cwd: String) -> SessionId
term_write(id: SessionId, data: String) -> ()            // 含二进制 base64 模式
term_resize(id: SessionId, cols: u16, rows: u16) -> ()
term_kill(id: SessionId) -> ()
term_list() -> Vec<SessionMeta>                          // cwd/shell/存活/前台进程
// 事件：term://output {id, data}（批量合帧）；term://exit {id, code}

// ---- W-4 工具链（tc_）----
tc_scan() -> Vec<Toolchain>                              // 扫描 Tools/ 目录
tc_register(manifest: ToolchainManifest) -> Toolchain    // 哈希校验后登记
tc_probe(name: String) -> { ok, version, ms }            // --version 探活
tc_path_synthesis() -> String                            // PATH 合成预览
tc_install(pack: {url, sha256, dest}) -> Progress        // 白名单出站下载

// ---- W-5 Vault 2.0 / TAP（vp_）----
vp_put(id, kind, secret) -> ()                           // 密文入库（不进日志）
vp_inject(id, pid: u32) -> ()                            // （内部）启动通道调用
vp_list() -> Vec<{id, kind, lastUsed}>                   // 不含明文
adapter_upsert(spec: TapAdapter) -> ()                   // 适配器登记
adapter_test(id) -> {detect, launch, credential, net, ui} // 五关注点自检

// ---- W-6 工作台（wb_）----
wb_save(name) -> Preset               // 当前布局快照 → 预设
wb_apply(presetId) -> ()              // 重放布局（占位→嵌入→聚焦）
wb_presets() -> Vec<Preset>
wb_trace_audit() -> ResidueReport     // 痕迹核查：宿主差分报告
wb_trace_clean(paths: Vec<String>) -> () // 确认后清除（逐条二次确认）

// ---- W-7 容器（ct_）----
ct_status() -> {form: "vhdx"|"dir", state, sizeBytes, freeBytes}
ct_mount(file, unlockViaVault) -> ()  // diskpart attach + BitLocker 解锁
ct_dismount() -> ()                   // 刷盘 + detach + 锁定
ct_migrate_to_container(targetSizeGb) -> Progress  // 目录→容器迁移（可回滚）
ct_rollback(migrationId) -> ()
```

### 12.2 错误体系（扩展 `error.rs`）

| 错误码 | 语义 | 前端呈现 |
| --- | --- | --- |
| `ProfileViolation` | 环境变量越界/循环 | 编辑面板内联标红，阻断保存 |
| `NetDenied(host)` | 未授权域名 | 弹出授权确认（once/persist 双选） |
| `NetDisabled` | 一键全断生效中 | 终端/面板横幅"出站已断开" |
| `TermSpawnFailed(reason)` | ConPTY 创建失败（含 Win10 <1809） | 如实提示 + 建议嵌入路径 |
| `ToolchainHashMismatch` | 下载包哈希不符 | 阻断安装 + 重试建议 |
| `VaultLocked` | 金库锁定中凭证不可注入 | 解锁引导，不泄露存在性之外信息 |
| `ContainerNeedsAdmin` | VHDX 需管理员 | 形态回退引导（如实说明） |
| `MigrationUnsafe(reason)` | 迁移空间/锁校验失败 | 拒绝迁移并给出修复清单 |
| `ResidueDetected` | 痕迹核查发现残留 | 通知中心列表 + 确认清除 |

原则沿用：底层错误翻译为中性双语文案；**绝不静默降级**——每个失败路径
要么成功、要么给用户一个如实的下一步。

### 12.3 事件面

| 事件 | 载荷 | 用途 |
| --- | --- | --- |
| `term://output` | {id, data} | 终端流（16ms 合帧） |
| `term://exit` | {id, code} | 会话结束 → 窗口标记 |
| `net://hit` | {host, proc, bytes} | 任务栏出站灯闪烁 |
| `wb://restored` | {preset, ms} | 会话恢复完成 → toast |
| `ct://state` | {state} | 容器状态机广播（任务栏锁形图标） |
| `ai://session` | {tool, phase} | W-9 会话面板 |

---

## 第十三章 · 风险登记册（Risk Register）

| # | 风险 | 概率 | 影响 | 缓解 | 残余 |
| --- | --- | --- | --- | --- | --- |
| R1 | VS Code（Electron）嵌入兼容性异常 | 中 | 中 | 失败路径已保证不杀进程/可重试/回退独立窗；W-4 实机矩阵（Win10 21H2/Win11 23H2/24H2） | 个别版本独立窗运行（如实提示） |
| R2 | Windows Terminal 嵌入失败 | 中 | 低 | 路径 A 自绘终端为主线，B 仅为兜底 | 仅损失多标签特性 |
| R3 | 宿主安全软件误报（SetParent/ConPTY/全局键） | 中 | 中 | 主流 AV 白名单申请文档；行为均为公开 API，无注入无钩子 | 企业 EDR 环境受限（如实声明） |
| R4 | diskpart 被组策略禁用 | 低 | 中 | 形态回退（目录模式）自动生效 | 容器加密不可用 → Vault+文件级加密 |
| R5 | U 盘主控磨损/掉速 | 中 | 中 | 缓存分级 + 写放大治理 + 设备分级建议 | 提示用户换 SSD |
| R6 | Claude API 变更（CLI 配置/环境变量名） | 中 | 低 | TAP 适配器层隔离变更面；适配器可热更新 | 更新适配器 JSON 即可 |
| R7 | 凭证注入被宿主内存扫描（恶意宿主） | 低 | 高 | 文档显著警告"陌生电脑=高风险"；Vault zeroize 尽力而为 | **不可根除**（用户态边界） |
| R8 | 大项目容器 IO 瓶颈 | 中 | 中 | 显式授权缓存外迁 + 设备分级提示 | 建议移动 SSD |
| R9 | 多显示器/异构 DPI 布局错位 | 低 | 中 | 预设 zone 依赖 VWM 工作区推导（已支持任务栏四向） | 保存时按当前显示器集校验 |
| R10 | 便携工具链许可证合规（VS Code 等） | 低 | 中 | 均为官方 portable 发行版按其许可再分发；下载走官方源+哈希 | 用户自行遵守各工具许可 |

**风险管理纪律**：每个批次收尾把"本批新风险"追加进本表（随代码入库），
影响≥中且缓解后仍有残余的风险必须在 FAQ/设置页如实披露。

---

## 第十四章 · 运维手册（Runbook）

### 14.1 常见故障处置

| 症状 | 自检步骤 | 处置 |
| --- | --- | --- |
| 双击启动器无反应 | ① 换 USB 口（供电不足）；② 查 `logs/variable.log` 尾部 | 便携运行时缺失 → 按日志补齐；WebView2 缺失 → 装 Evergreen |
| 桌面展开但 VS Code 未出现 | ① 任务栏运行态有无 Code 图标；② embed 日志行 | 30s 未捕获 → 独立窗已开；查 Profile 是否指向正确 Code.exe |
| `claude` 提示未登录 | ① Vault 是否解锁；② `CLAUDE_CONFIG_DIR` 探针 | 解锁 Vault → 首登向导；域名授权面板确认 claude.ai 放行 |
| 终端乱码 | chcp/字体探针 | 会话元数据里 shell 编码位重置；xterm.js 字体回落 |
| 容器挂载失败 | `ct_status()`；diskpart 输出捕获 | 权限/被组策略禁 → 形态回退引导；文件锁 → 重启宿主后重试 |
| 退出后仍提示未刷净 | 退出日志"未刷净项"列表 | 按列表重插处理；SQLite 以 WAL 保证一致 |

### 14.2 诊断数据包（一键导出，隐私先行）

- 设置→数据→「导出诊断包」：`variable.log` 尾部 + `ct/net/term` 状态
  快照 + bench 最近成绩；**绝不包含**：源码、凭证、项目内容、
  出站审计中的 URL 参数（仅域名）；
- 导出目标由用户对话框选择（不经任何网络上传）。

### 14.3 升级与迁移

- **程序升级**：官方包解压覆盖 `Variable.exe`（数据/容器不动）；
  schema 迁移由 db 层幂等保证（已有能力）；
- **换盘迁移**：整盘复制（容器=单文件；目录=Data 目录）→ 新盘首启
  自检哈希 → 通过即用；
- **降级回滚**：保留上一版 exe 副本于 `Tools/backup/`；数据库 schema
  向后兼容一个版本（迁移带 version 表，已有）。

### 14.4 度量与持续验证

| 指标 | 目标 | 采集 |
| --- | --- | --- |
| 冷启动 P50/P95 | ≤2s / ≤3s @SSD | boot 事件轴（bench.cjs 每批记录） |
| 嵌入成功率 | ≥95%（受控设备矩阵） | embed 结果统计（本地计数，不出网） |
| 痕迹核查通过率 | 100%（对抗用例外） | wb_trace_audit 历史报告 |
| 终端吞吐 | ≥40MB/s 回显 @120fps | bench |
| AI 会话端到端首字延迟 | ≤2.5s（网络无关部分 ≤0.3s） | 会话面板计时（本地） |


## 第十五章 · 批次工作分解（文件级改动清单）

> 供实施时直接对照：每批次触及的既有文件与新建文件，避免"改到哪算哪"。

### W-1 · 隔离启动档

| 文件 | 动作 | 内容 |
| --- | --- | --- |
| `src-tauri/src/shell/profile.rs` | 新建 | 语义重定向展开器 + 越界校验 + 单测 |
| `src-tauri/src/shell/launcher.rs` | 修改 | `tp_launch_inner` 增 profile 参数；`spawn_detached` envs 注入 |
| `src-tauri/src/shell/embed.rs` | 修改 | `embed_launch` 启动段接 Profile |
| `src-tauri/src/shell/mod.rs` | 修改 | 注册 profile 模块 |
| `src-tauri/src/lib.rs` | 修改 | 注册 `pf_expand`/`pf_validate` |
| `src/lib/ipc.ts` | 修改 | 新命令类型化 |
| `src/system/launcher/LauncherManager.tsx` | 修改 | Profile 编辑面板 |
| `src/i18n/dictionaries.ts` | 修改 | 三语文案 |
| `src-tauri/tests/commands_e2e.rs` | 修改 | Profile 展开端到端用例 |

### W-2 · 出站白名单

| 文件 | 动作 | 内容 |
| --- | --- | --- |
| `src-tauri/src/shell/netconsent.rs` | 重构 | 白名单存储/guard/审计/一键断 |
| `src-tauri/src/shell/netproxy.rs` | 新建 | 回环转发器（可选组件） |
| `src/system/tray/QuickPanel.tsx` | 修改 | 出站面板分区 |
| `src/components/NetConsentHost.tsx` | 修改 | once/persist 双选弹窗 |
| `tools/audit.cjs` | 修改 | 直连 URL 强制过 guard 检查 |
| `winman.rs` / `shortcuts.ts` | 修改 | Ctrl+Shift+X 一键断网 |

### W-3 · 终端

| 文件 | 动作 | 内容 |
| --- | --- | --- |
| `src-tauri/src/shell/conpty.rs` | 新建 | PseudoConsole 会话管理 + 线程 + 合帧 |
| `src/system/terminal/` | 新建 | `TerminalWindow.tsx`（xterm.js）、会话 hook、主题 |
| `src/system/windows/vwm.ts` | 修改 | `builtin:terminal` 应用类型与会话恢复 |
| `src/system/windows/VwmAppContent.tsx` | 修改 | 终端分支渲染 |
| `src/lib/ipc.ts` | 修改 | term_* 命令 + term:// 事件监听 |
| `src/system/windows/VirtualWindowFrame.tsx` | 修改 | AI 徽标 / resize 透传 |

### W-4 · 工具链 + VS Code

| 文件 | 动作 | 内容 |
| --- | --- | --- |
| `src-tauri/src/shell/toolchain.rs` | 新建 | 注册表/PATH 合成/探活/哈希下载 |
| `src/system/launcher/ToolchainPanel.tsx` | 新建 | 工具链管理页 |
| `src/system/boot/BootScreen.tsx` | 修改 | 工具链自检阶段接入 |
| `src/features/settings/SettingsModal.tsx` | 修改 | 工具链/性能策略设置 |
| `tools/bench.cjs` | 新建 | 基准套件 |

### W-5 / W-6 / W-7 / W-9

- W-5：`vault.rs` 扩展（凭证类型/注入/zeroize）、`adapter.rs`（TAP 解析校验）、
  首登向导组件；
- W-6：`src/system/workbench/`（预设引擎+快照）、`DesktopShell.tsx` 接线、
  `wb_trace` 核查器（`residue.rs`）；
- W-7：`container.rs` 状态机 + 迁移向导 UI + 设置页形态指示；
- W-9：MCP 托管（Profile 复用）、hooks 桥（`ai://session`）、会话面板。

> 纪律：每文件改动保持"批次内可回滚"——新文件为主、既有文件小步修改，
> 危险区（launcher/embed/wallpaper）沿用"先跑 Mimosa/审计再落盘"流程。

---

## 附录 F · TAP 适配器完整示例

### F.1 claude-code（内置）

```jsonc
{
  "schemaVersion": 1,
  "tool": "claude-code",
  "detect": { "path": "%npmGlobal%\\claude.cmd", "minVersion": "1.0.0" },
  "launch": { "entry": "claude", "shell": true, "profile": "default" },
  "credential": { "vaultId": "anthropic",
                  "env": ["CLAUDE_CODE_OAUTH_TOKEN"],
                  "alt": { "kind": "oauth", "domains": ["claude.ai", "console.anthropic.com"] } },
  "config":   { "configDirEnv": "CLAUDE_CONFIG_DIR", "defaultDir": "%home%\\.claude" },
  "net":      { "domains": ["api.anthropic.com"] },
  "ui":       { "badge": "ai-session", "titleProbe": ["claude"], "accent": "#d97757" },
  "session":  { "resumeArgs": ["--continue"], "historyDir": "%configDir%\\projects" }
}
```

### F.2 codex / aider / 通用 shell（节选差异）

```jsonc
// codex
{ "tool": "codex", "credential": { "vaultId": "openai", "env": ["OPENAI_API_KEY"] },
  "net": { "domains": ["api.openai.com"] }, "session": { "resumeArgs": ["resume"] } }

// aider（python 工具链依赖）
{ "tool": "aider", "launch": { "entry": "aider", "shell": true },
  "config": { "configDirEnv": null, "defaultDir": "%home%\\.aider" },
  "net": { "domains": ["api.openai.com", "api.anthropic.com"] } }

// 通用 shell（无凭证/无网络特权）
{ "tool": "shell", "launch": { "entry": "pwsh", "shell": false },
  "net": { "domains": [] }, "ui": { "badge": null } }
```

**校验规则**（`adapter.rs`）：schema 未知字段拒绝；`net.domains` 必须为
FQDN（禁 IP/通配符跨级）；`credential.env` 仅接受大写下划线命名；
`detect.path` 禁 `..` 与盘外路径；任何违规 → 登记失败并给出原因行号。

---

## 附录 G · 数据库 Schema 变更（W-3/W-4/W-6/W-7）

```sql
-- W-3 终端会话元数据
CREATE TABLE IF NOT EXISTS term_sessions (
  id TEXT PRIMARY KEY, shell TEXT NOT NULL, cwd TEXT NOT NULL,
  created_at INTEGER NOT NULL, last_used INTEGER, restore INTEGER DEFAULT 0);

-- W-4 工具链注册表（JSON 主存储，DB 仅索引探活结果）
CREATE TABLE IF NOT EXISTS toolchain_probe (
  name TEXT PRIMARY KEY, ok INTEGER, version TEXT, probed_at INTEGER);

-- W-6 布局快照
CREATE TABLE IF NOT EXISTS workbench_snapshot (
  preset_id TEXT NOT NULL, slot INTEGER NOT NULL, app TEXT NOT NULL,
  zone TEXT, args_json TEXT, minimized INTEGER DEFAULT 0, focused INTEGER DEFAULT 0,
  PRIMARY KEY (preset_id, slot));

-- W-7 容器审计
CREATE TABLE IF NOT EXISTS container_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, event TEXT NOT NULL,
  detail TEXT, at INTEGER NOT NULL);
```

迁移幂等性沿用既有 `db.rs` 版本化机制；全部新增表带 `IF NOT EXISTS` +
迁移单测（两次执行结果一致）。

---

## 附录 H · 启动器行为规格（SystemLauncher）

1. **单实例**：命名互斥体；二次双击 → 聚焦既有实例（与 `recycle` 单实例
   模式同构）；
2. **权限探测**：启动时检测提升状态 → 决定容器形态可用性（如实传给 UI）；
3. **设备分级**：存储介质探测结果写入本次会话上下文 → 驱动缓存策略与提示；
4. **崩溃边界**：启动器只做最小工作（环境检查→拉起主程序），任何失败
   弹原生消息框（不依赖 webview），错误码与日志指针给全；
5. **退出语义**：Del+Backspace / 红灯「保存并退出」/ 托盘退出三入口
   汇聚同一 6.3 时序，绝不出现第二条退出路径。

---

## 附录 I · 与既有规格的衔接说明

- **规格 5.9（第三方软件）**：本蓝图全部新增能力挂在其登记/启动/嵌入通道
  之上，未改变「零特权、零嵌入承诺」的既有语义；
- **规格 10.1/10.3（桌面覆盖与置顶）**：预设重放沿用撤销置顶逻辑；
- **批次 E-16/17/18 键位**：双 Esc / Del+Backspace 语义不变，
  Ctrl+Alt+A/W/X 为纯新增（冲突表审计通过后入库）；
- **守护子系统移除决定**：本蓝图不复活守护线程/Job Object——进程失败
  处理统一走「不杀进程 + 如实回退 + 用户手动接管」的既定纪律；
  若未来用户要求恢复，作为独立批次评估（不默认捆绑本蓝图）。

---

*Variable Workstation Blueprint · v1.1（含 WBS/Runbook/风险登记册）
· 每一个"已完成"都有代码与测试背书，每一个"未完成"都有批次与验收标准。*

## 附录 A · U 盘目录结构规范

```
移动盘（USB / 移动 SSD）
│
├─ Variable.exe                 启动器（主程序，免安装）
├─ workstation.vhdx             单文件容器（W-7 后；管理员形态）
│     └─（内部）Variable\      完整数据根：db/media/apps/home/cache/...
├─ Data\                        目录形态数据根（W-7 前的默认；或非管理员回退）
│  ├─ db\ media\ backups\ recovery\ logs\
│  ├─ apps.json                第三方登记表（含 Profile）
│  ├─ toolchains.json          工具链注册表
│  ├─ presets\                 工作台预设 + 工具适配器（TAP）
│  ├─ Home\                    语义 HOME（.gitconfig/.ssh/.claude/...）
│  ├─ Tools\                   便携工具链（node/python/git/vscode）
│  ├─ Projects\                用户项目（默认工作区）
│  └─ cache\                   npm/pip 缓存（高速盘策略下）
└─ varpack\                     （可选）分享包导入区
```

## 附录 B · 环境变量重定向总表（Profile 语义键 → 实际变量）

| 语义键 | 注入变量 | 主要受益工具 |
| --- | --- | --- |
| home | `HOME`, `USERPROFILE` | git/ssh/claude/cargo/pip/claude 登录态 |
| appdata | `APPDATA`, `LOCALAPPDATA` | npm/VS Code/多数 Windows 工具 |
| tmp | `TMP`, `TEMP` | 全部子进程临时文件 |
| xdg | `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` | 跨平台 CLI |
| npm | `npm_config_prefix`, `npm_config_cache` | node 工具链 |
| pip | `PIP_CACHE_DIR` | python 工具链 |
| git | `GIT_CONFIG_GLOBAL`, `GIT_SSH_COMMAND` | git/ssh 随盘 |
| claude | `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_OAUTH_TOKEN`(Vault 注入) | AI 会话与登录态 |
| vscode | `VSCODE_PORTABLE` | VS Code data 目录 |
| proxy | `HTTPS_PROXY`(回环转发器，可选) | 子进程出站审计 |

## 附录 C · 配置文件格式定义

- `apps.json` v2：v1 字段全保留 + `profile{env,redirect,fsPolicy}` +
  `adapter`（TAP 引用）；`schemaVersion: 2`；
- `toolchains.json`：`{schemaVersion, tools:{name:{version,path,env,kind}}}`；
- `presets/*.json`：`{schemaVersion, id, name, restore, windows:[{app,zone,args,minimized}], badges}`；
- `adapters/*.json`：TAP 五关注点（detect/launch/credential/net/ui/session）；
- 全部 JSON 拒绝越界路径（盘外绝对路径/`..`/UNC 白名单外）——
  校验器与测试随 W-1 落地。

## 附录 D · 命令与快捷键速查（新增部分）

| 快捷键 / 命令 | 功能 |
| --- | --- |
| Ctrl+Alt+A | AI 命令面板（新会话/恢复会话/切项目） |
| Ctrl+Alt+W | 工作台预设轮换 |
| Ctrl+Shift+X | 出站一键全断 |
| 终端内 `claude` / `claude --continue` | AI 会话开始/恢复 |
| 桌面红灯 → 「保存并退出」 | 完整退出流程（刷盘/锁定/无痕） |
| 双击 Esc | 环境 ↔ 宿主桌面切换（已有） |

## 附录 E · 术语表

| 术语 | 释义 |
| --- | --- |
| VWM | Variable 虚拟窗口管理器（环境内窗口系统） |
| Profile | 隔离启动档：环境变量语义重定向 + 文件系统策略 |
| TAP | 工具适配器协议：声明式接入 AI CLI/工具的 JSON 规范 |
| 容器形态 / 目录形态 | VHDX 单文件 / 便携数据目录两种存储形态 |
| 痕迹核查 | 退出后对宿主目录做快照差分的残留检测 |
| zone | 预设中的贴靠区域语法（与 VWM snap 同源） |
| varpack | 配置分享包（预设+适配器+工具链清单，不含代码与凭证） |

---

*Variable Workstation Blueprint · v1.0 · 站在 Variable 的肩膀上，
把整个开发工作台装进口袋。*
