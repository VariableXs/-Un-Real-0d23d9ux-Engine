# SINGULARITY-100（奇点计划）实施总步骤图

> **版本**：`1.0singularity.q` ｜ **日期**：2026-09-09
> **性质**：施工总纲 —— 回答「Q-01…Q-100 按什么顺序、经什么工序、过什么门禁落地」。
> **配套**：功能定义见 `SINGULARITY-100-功能全景图.md`；十路分工见 `SINGULARITY-100-AI分工图.md`。

---

## 0. 总步骤一图流

```
┌─────────────────────────────────────────────────────────────────────┐
│ S0 地基（先行）                                                      │
│  Q注册表 registry.ts ── i18n labels.ts ── singularity.css            │
│  Rust singularity.rs（pulse/temp/zone/journal/batch/archive）        │
│  奇点中枢 SingularityHub（ai04 挂载）+ 运行时 SingularityRuntime      │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S1–S15 十五域模块（可并行；每域 = 模块文件 + hub 注册 + 测试）          │
│  S1 启动剧场 bootTheater     S9  兼容护送 compatGuard                │
│  S2 窗口流动 windowFlow      S10 隐私补强 privacyPlus                │
│  S3 桌面艺术 desktopArt      S11 生态补强 ecoPlus                    │
│  S4 任务栏+  taskbarPlus     S12 视觉补强 visionPlus                 │
│  S5 输入流动 inputFlow       S13 声音通知 soundPlus                  │
│  S6 文件补强 filesPlus       S14 无障碍+ a11yPlus                    │
│  S7 工具套件 toolsSuite      S15 质量收官 qualityPlus                 │
│  S8 硬件环境 hwAmbient                                             │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S16 接线（一行级）                                                   │
│  DesktopShell ← import activate ｜ global.css ← @import              │
│  命令面板/快捷面板 ← 奇点中枢入口 ｜ lib.rs ← singularity 命令注册     │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S17 全量门禁自检                                                     │
│  tsc 0 err → vitest 全绿 → cargo check/test → audit 门禁             │
│  → 冒烟：hub 开合/开关持久化/降级链/reduce-motion                      │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S18 交付同步                                                         │
│  三大文档（全景/步骤/分工）入 docs/ → git commit → push GitHub main   │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 1. S0 地基（一切功能共享的底座）

| # | 工件 | 内容 | 完成口径 |
|---|---|---|---|
| 0.1 | `src/system/singularity/registry.ts` | 100 项元数据（id/域/默认态/参数 schema/边界引用），localStorage 持久化，订阅通知 | 单测：100 项唯一、域分布 7/8/7/7/7/7/7/7/6/7/6/5/5/6/8、持久化往返 |
| 0.2 | `src/system/singularity/labels.ts` | 全部 100 项 zh/en 标题与描述（含 hub 界面文案） | 词条键与 registry 一一对应 |
| 0.3 | `src/styles/singularity.css` | 全部动效/面板/叠层样式；全量引用 tokens.css 语义变量；`[data-reduce-motion]` 全降级 | 零裸色值（audit 通过） |
| 0.4 | `src-tauri/src/shell/singularity.rs` | 六组命令：`singu_pulse` / `singu_temp_scan` / `singu_zone_check` / `singu_journal_list`+`singu_journal_log` / `singu_batch_attrs` / `singu_archive_check` | `cargo check` 0 err；命令入 lib.rs handler 表 |
| 0.5 | `SingularityHub.tsx` | 十五域控制台：搜索/开关/参数/工具直开；`ai04:open-feature {feature:"singularity-hub"}` 挂载 | Esc 关闭、状态即 registry 真源 |
| 0.6 | `SingularityRuntime.tsx` | 全局运行时：订阅 registry，按域激活 15 个模块；统一处理 reduce-motion/safeMode/static 降级 | 模块激活幂等 |
| 0.7 | `activate.ts` | 副作用激活：`mountOnEvent` 注册 hub + 动态 import Runtime | DesktopShell 仅 +1 行 import |

**S0 门禁**：registry 单测绿；hub 可开可关可搜索；Rust 命令可被前端调用（IPC 冒烟）。

## 2. S1–S15 十五域模块（并行施工，互不阻塞）

每域统一工序（五步）：
1. **纯逻辑先行**（可测函数：算法/映射/计算）；
2. **行为层**（DOM 观察/CSS 叠层/事件监听，零侵入既有组件）；
3. **hub 注册**（功能卡 + 参数控件 + 状态反馈）；
4. **单测**（纯逻辑全覆盖，行为层关键分支）；
5. **降级链**（reduce-motion/safeMode/static 三态行为定义并写进 hub 描述）。

| 步 | 域 | 模块文件 | 关键纯逻辑（必测） | 关键行为 |
|---|---|---|---|---|
| S1 | 启动剧场 | `modules/bootTheater.ts` | 唤醒判定（失焦时长阈值）、问候档位（5 时段）、品牌时刻日历表 | focus/blur 监听、谢幕叠层、剧本参数注入 BootAtmosphere |
| S2 | 窗口流动 | `modules/windowFlow.ts` | 排列画廊四算法（grid/spiral/columns/quadrants）、尺寸预设表、年龄计算 | vwmStore 订阅、标题栏体征环注入、边缘偷看、动量拖拽 |
| S3 | 桌面艺术 | `modules/desktopArt.ts` | 堆叠分组规则、缩放锚点数学、涟漪波次排序 | DesktopIcons 叠层、框选橡皮筋、网格微调 |
| S4 | 任务栏+ | `modules/taskbarPlus.ts` | 温度→色映射、分页数据结构、键盘流拼音匹配 | Taskbar 叠层、时钟微卡、律动条（WebAudio） |
| S5 | 输入流动 | `modules/inputFlow.ts` | 惯性曲线（加速/回弹）、缓冲序列匹配、节拍三档、声呐音高映射 | 全局 keydown、输入框挂起仲裁 |
| S6 | 文件补强 | `modules/filesPlus.ts` | 命名规律学习（日期/序号/项目三型）、时间透镜分桶、配重四档 | Explorer 集成点（菜单/地址栏/拖拽） |
| S7 | 工具套件 | `modules/toolsSuite/*` | 番茄状态机、队列 FIFO、倒计时调度、径向轮盘几何 | 6 个 overlay 工具 + 划词工具条 |
| S8 | 硬件环境 | `modules/hwAmbient.ts` | 脉搏→极光强度/呼吸频率/色温映射、湿度分档、涨潮判定 | `singu_pulse` 轮询、外设卡片、降载联动 |
| S9 | 兼容护送 | `modules/compatGuard.ts` | 护送倒计时状态机、低电量两档协议、断连救援排布算法 | display/power/lock 事件桥 |
| S10 | 隐私补强 | `modules/privacyPlus.ts` | 密码特征启发式、TTL 判定、权限对账差异、冰山构成归集 | journal 命令对接、检疫确认卡 |
| S11 | 生态补强 | `modules/ecoPlus.ts` | 营养标签聚合、体检三轴评分、事件订阅矩阵模型 | 市场/插件面板注入、API 演练场 |
| S12 | 视觉补强 | `modules/visionPlus.ts` | 季节→色相映射表、精品店目录、材质滑块→token 映射、三拍过渡时序 | token 补丁（内存态）、鼠标视差层 |
| S13 | 声音通知 | `modules/soundPlus.ts` | 呼吸合流规则（同源 3+/s）、日报分组、电平条采样 | notifyStore 桥、AnalyserNode |
| S14 | 无障碍+ | `modules/a11yPlus.ts` | 编辑距离+拼音容错、色觉矩阵（保亮度）、单键待续环状态机 | 全局滤镜层、文字缩放变量、引导条 |
| S15 | 质量收官 | `modules/qualityPlus.ts` | 火焰图布局（分层矩形）、长任务分桶、配额总账、ECG 四导联、降级矩阵真源 | PerformanceObserver、hub 观测面板 |

**每步门禁**：模块单测绿；`tsc --noEmit` 绿；hub 中该域功能可开关且立即生效/失效。

## 3. S16 接线（对既有文件的全部改动清单）

| 文件 | 改动 | 级别 |
|---|---|---|
| `src/system/desktop/DesktopShell.tsx` | `import "../singularity/activate";`（注释一行） | 1 行 |
| `src/styles/global.css` | `@import "./singularity.css";` | 1 行 |
| `src/system/palette/CommandPalette.tsx` | 注册「奇点中枢 / Singularity Hub」命令（+若干工具命令） | ~10 行 |
| `src-tauri/src/lib.rs` | `pub mod singularity;`（shell/mod.rs）+ handler 表追加 8 条命令 | ~10 行 |
| `src-tauri/src/shell/mod.rs` | `pub mod singularity;` | 1 行 |

**红线**：不改动任何既有组件内部逻辑；不删除/重命名任何既有导出；所有新样式类名 `singu-` 前缀避免碰撞。

## 4. S17 全量门禁自检（完成口径）

1. `npx tsc --noEmit` → 0 错误。
2. `npx vitest run` → 全绿（既有 1000+ 用例 + 新增 Q 系列用例）。
3. `cargo check`（src-tauri）→ 0 错误；`cargo test` → 全绿。
4. `node scripts/audit.cjs`（视觉门禁）→ 新 CSS 零裸色值。
5. 冒烟清单（人工/浏览器核验）：
   - hub 开合（命令面板 → 奇点中枢）；
   - 任一开关切换 → 刷新后状态保留（localStorage）；
   - reduce-motion 模拟下所有 Q 动效归零；
   - 六个工具 overlay（番茄/标尺/抽屉/轮盘/倒计时/划词）开合正常；
   - `singu_pulse` 在无电池台式机上：电池相关层如实隐藏。

## 5. S18 交付同步

1. 三大文档入 `docs/`（全景/步骤/分工）。
2. `git add` 分域提交（S0 地基 → S1…S15 → S16 接线 → S17 修复 → S18 文档）。
3. push 至 GitHub `VariableXs/-Un-Real-0d23d9ux-Engine` main 分支；核对远端与本地一致。

---

## 6. 风险与回退

| 风险 | 缓解 |
|---|---|
| 100 项与存量重复 | 附录B 最近邻表逐项比对（全景文档 §17） |
| 新动效伤既有手感 | 全部效果默认关/轻量；零侵入挂载；reduce-motion 全降级 |
| Rust 命令平台差异 | sysinfo/ADS 读取失败路径全部如实降级（隐藏/提示），不假装成功 |
| 上下行事件风暴 | 新事件全部 `singu://` 前缀 + 复用 M-50 削峰纪律 |
| 样式冲突 | `singu-` 前缀 + 只读叠层 + tokens 语义变量 |
| 性能回归 | 被动 API（PerformanceObserver）优先；轮询统一 2s 且 hub 关闭时模块整体卸载 |

## 7. 施工顺序依赖图

```
S0 ──► S1  S2  S3  S4  S5  S6  S7  S8  S9  S10 S11 S12 S13 S14 S15  （并行）
 │      │   │   │   │   │   │   │   │   │    │   │   │   │   │
 │      └───┴───┴───┴───┴───┴───┴───┴───┴────┴───┴───┴───┴───┴──► S16 接线
 │                                                                  │
 └──────────────────────────（hub/registry 为一切前置）──────────────►│
                                                                     ▼
                                                                   S17 门禁
                                                                     ▼
                                                                   S18 交付
```

*SINGULARITY-100 实施总步骤图 · 完*
