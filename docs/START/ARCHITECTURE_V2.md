# Variable V2 架构设计 — Private Desktop Environment

> 版本：v2.0.0-design · 日期：2026-09-04 · 状态：设计中
> 旧版 v1.0.0（Private Space 单窗口多空间应用）已归档，仅作代码迁移来源。

---

## 一、产品定义

**Variable 是覆盖在 Windows 之上的私人桌面环境系统**，不是一个应用。

- 全屏覆盖 Windows 桌面（无边框窗口）
- 桌面 / 任务栏 / 开始菜单 / 系统托盘 / 文件管理器 / 软件生态
- 原 4 个功能空间升级为 4 个**独立软件**，与第三方软件平级共存
- 启动过程真实透明：进度、日志、图标、字母描边 100% 由真实加载事件驱动
- U 盘迁移 100% 功能等效
- 零网络 / 零遥测 / 零账号，数据仅本机

## 二、总体分层架构

```
┌─────────────────────────────────────────────────────────────┐
│  L0 显示层    壁纸(纯黑/3D引力场/视频/图片/混合) · 桌面图标网格   │
├─────────────────────────────────────────────────────────────┤
│  L1 系统层    任务栏 · 开始菜单 · 系统托盘(蓝牙/Wi-Fi/音频/通知)  │
│               文件管理器 · 全局回收站 · 隐私提示(摄像头/麦克风)   │
├─────────────────────────────────────────────────────────────┤
│  L2 软件层    Variable Write · Mind · Code · Fate（独立软件）    │
│               + 任意第三方 Windows 软件（🟢/🟡/🔴 便携分级）      │
├─────────────────────────────────────────────────────────────┤
│  L3 内核层    Rust 后端：boot · db · workspace · library ·      │
│               mindmap · media · project_scan · backup · export  │
│               settings · shell(托盘/硬件/启动器/回收站/U盘)      │
├─────────────────────────────────────────────────────────────┤
│  横切关注点   光影系统🔒(不修改) · i18n · 隐私边界 · 事件总线     │
└─────────────────────────────────────────────────────────────┘
```

## 三、窗口模型（Tauri 2 多窗口）

| 窗口 | label | 形态 | 内容 |
|---|---|---|---|
| 桌面环境 | `desktop` | 全屏无边框，覆盖 Windows 桌面 | 启动仪式 → 壁纸 → 图标网格 → 任务栏/托盘 |
| Write | `app-write` | 独立窗口 + 红绿灯 | 四款软件功能内核 🔒（迁移自 v1） |
| Mind | `app-mind` | 同上 | 同上 |
| Code | `app-code` | 同上 | 同上 |
| Fate | `app-fate` | 同上 | 同上 |
| 第三方软件 | — | 独立 OS 进程 | Rust `Command::spawn`，与 Variable 无 WebView 关系 |

- 每个软件窗口独立红绿灯（Mac 风格右上角，唯一非 Windows 习惯点）。
- 前端采用 **vite MPA 多入口**：每个窗口一个 html 入口、独立 bundle，互不影响。
- 软件窗口由桌面环境按需创建/关闭；卸载某软件不影响其他软件。

## 四、目录结构（目标态）

### 前端 `src/`

```
src/
├─ entries/                     # vite MPA：每窗口一个入口
│  ├─ desktop/
│  │  ├─ desktop.html
│  │  └─ main.tsx               # 桌面 shell：Boot → Desktop
│  ├─ app-write/                # Variable Write 窗口入口
│  ├─ app-mind/                 # Variable Mind 窗口入口
│  ├─ app-code/                 # Variable Code 窗口入口
│  └─ app-fate/                 # Variable Fate 窗口入口
├─ system/                      # L1 桌面环境系统层（全部新增）
│  ├─ boot/                     # 启动仪式（真实事件驱动）★ 已实现
│  ├─ wallpaper/                # 5 种壁纸模式 + 3D 引力场
│  ├─ icons/                    # 桌面图标网格（Windows 拖拽逻辑）
│  ├─ taskbar/                  # Win11 风格底部居中毛玻璃任务栏
│  ├─ startmenu/                # V 按钮 + 全局搜索
│  ├─ tray/                     # 蓝牙/Wi-Fi/音频/通知中心面板
│  ├─ explorer/                 # 文件管理器（Win+E，实时同步）
│  ├─ recycle/                  # 全局回收站
│  └─ privacy/                  # 摄像头/麦克风占用提示
├─ apps/                        # L2 四款独立软件（🔒内核迁移，功能不变）
│  ├─ write/                    # ← v1 features/{folders,editor,search,workspace}
│  ├─ mind/                     # ← v1 features/mindmap
│  ├─ code/                     # ← v1 features/projectviz
│  └─ fate/                     # ← v1 features/fate
├─ packages/                    # 跨窗口共享
│  ├─ ui/                       # TrafficLights 红绿灯、窗口装饰、通用组件
│  ├─ core/                     # ipc / types / i18n / stores / settings
│  └─ light/                    # 🔒光影系统（迁移自 v1，参数零修改）
└─ styles/                      # 各层样式（boot.css 等）
```

### 后端 `src-tauri/src/`

```
src-tauri/src/
├─ main.rs / lib.rs             # 入口与 command 注册
├─ boot.rs                      # ★ 启动加载序列 + LoadEvent 事件总线（已实现）
├─ state.rs / db.rs / error.rs / models.rs        # 内核基础（保留）
├─ library.rs / mindmap.rs / media.rs /           # 四软件后端（🔒保留）
│  workspace.rs / project_scan.rs / export.rs /
│  backup.rs / settings_cmd.rs / system.rs
└─ shell/                       # 桌面环境系统命令（后续新增）
   ├─ mod.rs
   ├─ tray.rs                   # 托盘状态
   ├─ hardware.rs               # 蓝牙/Wi-Fi/音频/摄像头/麦克风（Windows API）
   ├─ launcher.rs               # 第三方软件启动器 + 便携性分级
   ├─ recycle.rs                # 全局回收站
   └─ usb.rs                    # U 盘打包/校验/路径重映射/拔出保护
```

### 数据目录（不变，U 盘相对化）

```
<dataDir>/                      # %APPDATA%/com.variable.app 或 便携 .portable 同级
├─ db/variable.db               # SQLite (WAL)
├─ media/  attachments/         # 媒体库
├─ backups/  recovery/  logs/
└─ Workspace/                   # Variable Write 工作区
```

## 五、启动事件总线协议 ★ 本次实现

### 5.1 事件通道

- 事件名：`boot://event`（Tauri Event Bus，后端 → 前端单向推送）
- 推送方：Rust `boot::spawn_boot_loader(app_handle)`，在 `AppState` 目录初始化后启动
- 消费方：`system/boot/BootScreen.tsx`

### 5.2 LoadEvent 结构

```rust
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoadEvent {
    pub progress: f32,            // 0.0–1.0 真实累计进度（按任务权重，非时间线）
    pub current_task: String,     // 正在执行的真实任务描述
    pub file_path: Option<String>,// 正在处理的文件路径
    pub file_count: Option<usize>,// 已处理文件数
    pub total_count: Option<usize>,
    pub icon: &'static str,       // 📦🔍📚🧠💻🎯🖼💽🔐✓⚠❌
    pub level: u8,                // 0=info 1=warn 2=error
    pub elapsed_ms: u64,          // 真实耗时
    pub timestamp: u64,
    pub stats: Option<BootStats>, // ready 时的真实摘要
}
```

### 5.3 真实加载序列（每一步都是真实操作）

| # | 任务 | 图标 | 权重 | 数据来源 |
|---|---|---|---|---|
| 1 | 校验数据目录结构（db/media/attachments/backups/recovery/logs/Workspace） | 📦 | 5% | 真实目录检查 |
| 2 | 打开数据库 + 应用迁移 | 📦 | 10% | rusqlite 真实打开 + `PRAGMA user_version` |
| 3 | 统计数据库各表行数（folders/documents/mindmaps/nodes/edges/media/attachments/settings/backups） | 📚🧠 | 25% | 真实 `SELECT COUNT(*)` |
| 4 | 扫描 Workspace 文件树（跳过 `.trash`，每 50 个文件推送一次中间进度） | 🔍 | 30% | 真实文件系统遍历 |
| 5 | 索引媒体库（media + attachments 文件数与字节量） | 🖼 | 20% | 真实目录遍历 |
| 6 | 清点备份 | 🔐 | 5% | 真实 backups 目录清点 |
| 7 | ready（摘要：记录数/导图数/媒体数/总耗时） | ✓ | 5% | 汇总真实数据 |

### 5.4 前端呈现规则

- 进度条 = `progress`；VARIABLE 8 字母 SVG 描边按每字母 12.5% 区间映射真实进度（rAF 平滑插值，停滞则描边停滞）
- 日志区 = 真实 `current_task` + `file_path`（保留最近 9 条）
- 统计 = 已处理文件数 / 真实耗时 / 真实速度 / 内存（`performance.memory` 可用时）
- 跳过：Esc/空格；进度 <30% 拒绝并提示；≥30% 仅跳过 UI，后台加载继续
- 完成：`progress=1` → 显示 ✓ ready + 摘要 → 停 500ms → 过渡到桌面
- 浏览器 dev 模式（无 Tauri IPC）：直接跳过启动画面，保持现有浏览器调试行为

### 5.5 验收标准（启动机制）

- [ ] 进度条 100% 反映真实加载进度，无预设时间线
- [ ] 日志 100% 是真实文件名与真实操作，无硬编码文案
- [ ] 字母描边追随真实进度（可构造慢速盘观察停滞）
- [ ] 单任务 >500ms 日志条目显示 spinner；整体 >5s 出现提示
- [ ] 加载失败不致命：warn 跳过；数据库损坏 → ❌ + 从 backups/ 恢复路径
- [ ] ready 摘要数字与数据库/磁盘真实内容一致
- [ ] 快机 3–5s / 中机 5–8s / 低配 HDD 8–15s（由真实耗时决定）

## 六、迁移路线图

| 阶段 | 内容 | 状态 |
|---|---|---|
| M1 | 启动事件机制（boot.rs + BootScreen + 事件总线） | ★ 本次 |
| M2 | 桌面环境 shell（全屏覆盖 + 壁纸 5 模式 + 图标网格 + 红绿灯） | 待做 |
| M3 | 任务栏 + 开始菜单 + 全局搜索 | 待做 |
| M4 | 四款软件拆窗（MPA 多入口 + 独立红绿灯 + 光影🔒原样迁移） | 待做 |
| M5 | 系统托盘 + 硬件面板 + 隐私提示 | 待做 |
| M6 | 文件管理器 + 全局回收站 | 待做 |
| M7 | 第三方软件启动器 + 便携性分级 | 待做 |
| M8 | U 盘完全便携（打包/校验/重映射/拔出保护） | 待做 |

## 七、锁定声明 🔒

1. 四款软件（Write/Mind/Code/Fate）**功能内核**迁移时零修改。
2. 四款软件**光影方案**保持原设计不变（本文档不描述、不评估、不修改）。
3. 核心哲学不变：零网络/零遥测/零账号/完全离线/确定性可重现（PRNG 种子）。
4. 技术栈不变：Tauri 2.x + React 18 + TypeScript 5 + Rust + SQLite (WAL) + TipTap 2 + 原生 WebGL。
5. 所有交互对齐 Windows 用户习惯，唯一例外：红绿灯位置采用 Mac 风格。
