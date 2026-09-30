# Code Analysis 独立软件 × Variable 收件箱接入设计（v2）

> 结论先行：Variable 桌面端已经内建了"软件投递 → 自动识别 → 登记可用"的完整链路
> （`src/system/launcher/thirdApps.ts` 批次 F）。本设计不再新增任何系统机制，
> 目标只有一个：**把 Code Analysis 打包成一个对收件箱流程友好的独立 exe，丢进去就能用。**

## 一、Variable 现有能力（直接复用，不重造）

| 能力 | 现状（thirdApps.ts / 后端） |
|---|---|
| 软件收件箱 | `<数据目录>\SoftwareInbox`，环境启动自动扫描登记（`tp_inbox_import`） |
| 图标 | 自动提取 exe 原生图标并持久化（128px，`tp_ensure_icons`） |
| 拖入登记 | 拖软件文件夹到桌面 → 扫描主程序候选（过滤卸载器等）→ 自动登记 |
| 显示名 | 取 exe 的 FileDescription（本地化名称） |
| 启动方式 | 虚拟窗口占位 → 原生窗口 SetParent 嵌入；失败回退独立窗口（诚实提示） |
| 快捷触达 | 桌面图标 / 开始菜单 / 任务栏固定 / Win+数字 / 文件关联打开方式 |

## 二、Code Analysis 独立软件的形态

Tauri 桌面应用（复用现有 src-tauri 工程经验），打包为绿色版目录：

```
CodeAnalysis/                 ← 整个文件夹丢进 SoftwareInbox 即可
├── code-analysis.exe         ← 主程序（嵌入窗口目标）
├── resources/                ← 前端画布资源（树根/流程图等 7 种可视化）
└── core.dll / 内置 wasm      ← 语义 IR + 七级下钻引擎
```

对收件箱友好的四个硬要求：

1. **exe 元数据齐全**：FileDescription 写"Code Analysis 代码透视"（登记显示名），
   Icon 资源内嵌（收件箱提取 128px 图标用）。
2. **标准 Win32 窗口**：保证 SetParent 嵌入成功——不要做 UWP/管理员权限自提，
   单实例（二次启动把已开窗口前置并带参）。
3. **接受路径参数**：`code-analysis.exe <项目路径>`，兼容批次 B-27 的文件关联
   （用户在 Variable 里右键项目文件夹 → 打开方式 → Code Analysis）。
4. **绿色无安装器**：不写注册表、解压即用；配置存 exe 旁或用户数据目录。

## 三、接入流程（用户视角，全程零配置）

```
1. 把 CodeAnalysis/ 文件夹拷进 SoftwareInbox（或直接拖到桌面）
2. 重启 Variable（或拖放即刻触发扫描）
3. 桌面/开始菜单出现"Code Analysis 代码透视"图标（自动提取的图标）
4. 点击启动 → 以虚拟窗口形式嵌在 Variable 环境内，随时用
```

## 四、软件内部结构（不变的原则：core 与外壳分离）

```
code-analysis/
├── core/        # 纯逻辑：解析 → 语义 IR → 七级下钻模型（Rust crate，无 UI 依赖）
├── app/         # Tauri 壳：加载 core，渲染 7 种可视化画布（HTML/wasm）
└── vxapp/       # （可选，后续）同 core 再出一版 VARIX 内核侧 viewer
```

第一版闭环：打开项目 → 树根模式七级下钻 → 点到代码行出大白话解释。

## 五、分期（Variable 路线）

| 期 | 内容 | 验收 |
|---|---|---|
| P1 | core crate：目录扫描 + 语义 IR + 七级下钻数据模型（纯逻辑 + 单测） | 单测全绿 |
| P2 | Tauri 壳：打开项目 / 树根画布下钻 / 代码行大白话面板 | 手动闭环跑通 |
| P3 | 收件箱友好化：exe 元数据 / 单实例 / 路径参数 / 绿色打包 | 丢进 SoftwareInbox 重启后自动登记 |
| P4 | 嵌入体验：嵌入窗口内正常渲染、DPI、关闭回收 | 任务栏不出现重复原生窗 |
| P5 | 增量：流程图模式 / 拖拽改码 / 更多可视化（按原 500 项规格逐批上） | — |

## 六、风险与备注

- 收件箱扫描按"主程序可能性"选 exe，目录里若有多余 exe（如 updater）会被过滤
  或误选——保证目录里**只有一个主 exe** 最稳。
- 嵌入失败（少数窗口类）会按现有逻辑回退独立窗口并 toast，属已知降级，无需处理。
- 内核侧 `.vxapp` drop-in 方案（原草案 v1，git 历史可查）保留为远期路线：VARIX
  真机应用框架最终也需要同样的投递能力；当前优先 Variable 桌面路线落地。
