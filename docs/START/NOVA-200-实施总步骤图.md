# NOVA-200（新星计划）实施总步骤图

> **版本**：`1.0nova.w` ｜ **日期**：2026-09-09
> **性质**：施工总纲 —— 回答「W-001…W-200 按什么顺序、经什么工序、过什么门禁落地」。
> **配套**：功能定义见 `NOVA-200-功能全景图.md`；十六路分工见 `NOVA-200-AI分工图.md`（AI-01…AI-16，编号从 1 起）。

---

## 0. 总步骤一图流

```
┌─────────────────────────────────────────────────────────────────────┐
│ S0 地基（先行，与 singularity 体系平行共存、互不依赖）                  │
│  W注册表 registry.ts ── i18n labels.ts ── nova.css（nova- 前缀）      │
│  Rust nova.rs（pulse_ex / file_scan / audio_env / bluetooth_watch）  │
│  新星中枢 NovaHub（ai04 挂载）+ 运行时 NovaRuntime + activate.ts      │
│  本地语音底座 novaVoice.ts（域13/14 专用，离线模型 opt-in）            │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S1–S16 十六域模块（可并行；每域 = 模块文件 + hub 注册 + 单测）           │
│  S1  启动剧场 bootNova      S9  兼容诊疗 compatNova                   │
│  S2  窗口物理 windowNova    S10 隐私叙事 privacyNova                   │
│  S3  桌面生态 deskNova      S11 生态基因 ecoNova                       │
│  S4  任务栏+  dockNova      S12 视觉氛围 visionNova                    │
│  S5  输入节奏 inputNova     S13 声音通知 soundNova                     │
│  S6  文件叙事 filesNova     S14 无障碍+ a11yNova                       │
│  S7  效率工具 toolsNova     S15 UI 度量 designNova                     │
│  S8  硬件感知 hwNova        S16 工程收官 qualityNova                   │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S17 接线（一行级，与 singularity 接线点共址不共逻辑）                   │
│  DesktopShell ← import activate ｜ global.css ← @import               │
│  命令面板 ← 新星中枢入口 ｜ lib.rs / shell/mod.rs ← nova 命令注册      │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S18 全量门禁自检                                                      │
│  tsc 0 err → vitest 全绿 → cargo check/test → audit 门禁              │
│  → 冒烟：hub 开关持久化 / 降级链 / nova 与 singularity 共存互斥验证     │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ S19 交付同步                                                         │
│  三大文档（全景/步骤/分工）入 docs/ → git commit → push GitHub main   │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 1. S0 地基（一切 W 功能共享的底座）

| # | 工件 | 内容 | 完成口径 |
|---|---|---|---|
| 0.1 | `src/system/nova/registry.ts` | 200 项元数据（id/域/默认开关/参数 schema/changelog 字段），localStorage 键空间 `nova.registry` 独立持久化 | 单测：200 项唯一、十六域分布 12×8+13×8、持久化往返、与 singularity 键空间零冲突 |
| 0.2 | `src/system/nova/labels.ts` | 200 项 zh/en 标题与描述 + hub 文案 | 词条键与 registry 一一对应（i18n 审计绿） |
| 0.3 | `src/styles/nova.css` | 全部动效/面板/叠层样式；全量 tokens.css 语义变量；`[data-reduce-motion]` 全降级；类名 `nova-` 前缀 | 零裸色值（audit 通过）；与 `singu-` 类名零碰撞 |
| 0.4 | `src-tauri/src/shell/nova.rs` | 四组命令：`nova_pulse_ex`（分核/温度/电流/蓝牙采样）、`nova_file_scan`（文件元信息批量）、`nova_audio_env`（声级推算）、`nova_bluetooth_watch`（插拔事件） | `cargo check` 0 err；命令入 lib.rs handler 表 |
| 0.5 | `NovaHub.tsx` | 十六域控制台：搜索/开关/参数/状态/噪声计（W-147）；`ai04:open-feature { feature: "nova-hub" }` 挂载 | Esc 关闭；状态即 registry 真源；与奇点中枢（singularity-hub）并行可开互不干扰 |
| 0.6 | `NovaRuntime.tsx` | 全局运行时：订阅 registry，按域激活 16 个模块；统一 reduce-motion / safeMode / static 降级链 | 模块激活幂等；hub 全关时运行时整体卸载（零常驻开销） |
| 0.7 | `activate.ts` | 副作用激活：mountOnEvent 注册 hub + 动态 import Runtime | DesktopShell 仅 +1 行 import |
| 0.8 | `novaVoice.ts`（可选件） | 本地语音封装（离线识别 + 本地 TTS），仅域 13/14 四项消费（W-086/154/164/170/174） | 模型 opt-in 下载（显式用户动作）；未安装时四项如实隐藏不占位 |

**S0 门禁**：registry 单测绿；hub 可开可关可搜索；Rust 命令 IPC 冒烟通过；**共存验证**：singularity hub 与 nova hub 同开会话内同时挂载互不报错。

## 2. S1–S16 十六域模块（并行施工，互不阻塞）

每域统一工序（五步）：
1. **纯逻辑先行**（可测函数：算法/映射/状态机）；
2. **行为层**（DOM 观察 / CSS 叠层 / 事件监听 / Rust 命令对接，零侵入既有组件）；
3. **hub 注册**（功能卡 + 参数控件 + 状态反馈）；
4. **单测**（纯逻辑全覆盖，行为层关键分支）;
5. **降级链**（reduce-motion / safeMode / static 三态行为写进 hub 描述）。

| 步 | 域 | 模块文件 | 关键纯逻辑（必测） | 关键行为 / Rust 依赖 |
|---|---|---|---|---|
| S1 | 启动剧场 | `modules/bootNova.ts` | 温度制式判定（电源状态→制式映射）、预报模型（20 次均值+磁盘因子）、情绪测温（lastExit 状态→情绪档） | boot 阶段事件桥、星座交互层、里程碑音调度 |
| S2 | 窗口物理 | `modules/windowNova.ts` | 质量惯性曲线（面积→时延）、磁吸阻尼区几何、族谱树布局算法、九宫格吸附映射 | vwmStore 只读订阅、轨道区拖拽检测、折叠带动画 |
| S3 | 桌面生态 | `modules/deskNova.ts` | 星座曲线参数（6 座）、养成频率→存在感映射、老化梯度函数、合影差异 diff 算法 | DesktopIcons 叠层、拖拽标尺、结组手势 |
| S4 | 任务栏+ | `modules/dockNova.ts` | 磁漂弹簧（多体位移）、潮汐密度函数、模块槽注册表、昨日差集计算 | Taskbar 叠层、手柄检测（nova_bluetooth_watch）、弹道动画 |
| S5 | 输入节奏 | `modules/inputNova.ts` | 分应用节奏中位数、长按池径向环几何、速度→光环映射、标点智断状态机 | 全局 keydown 桥、沙坑反查（Z-08 注册表只读）、IMF 事件 |
| S6 | 文件叙事 | `modules/filesNova.ts` | 普查聚合算法、借阅状态机、墨阶分档、增长外推（线性+加权双模型）、代数解析器 | nova_file_scan、文件系统监听（限可见目录）、迁途记录钩子 |
| S7 | 效率工具 | `modules/toolsNova.ts` | 时间块吸附（15min 网格）、收官卡聚合、横桌四象限布局、算术表达式求值器 | 4 个 overlay 工具、跨工具拖放映射（U-17 只读对接）、语音图钉（novaVoice） |
| S8 | 硬件感知 | `modules/hwNova.ts` | 潮汐密度映射、分核聚合（超线程对）、工时累计、声级→音量补偿曲线 | nova_pulse_ex / nova_audio_env 轮询、电流读数、生日书日历 |
| S9 | 兼容诊疗 | `modules/compatNova.ts` | 观察期评级函数、DLL 导入表快扫清单、逃生三档兜底状态机、月相/字体替身相似度 | 进程/文件只读 API、反作弊特征表、minidump 本地解析、ACL 族谱读取 |
| S10 | 隐私叙事 | `modules/privacyNova.ts` | 体检三级评定、强度三轨算法、衰减公式（透明可解释）、指纹单向哈希 | 权限/事件本地账本、眼罩策略调用、焚毁仪式动画 |
| S11 | 生态基因 | `modules/ecoNova.ts` | 基因开关矩阵模型、雷达六轴聚合、依赖树冲突检测、二维码分片纠错 | 插件运行时对接（N-26 只读接口）、灯塔实测采样、声邮草稿 |
| S12 | 视觉氛围 | `modules/visionNova.ts` | 极光历色相函数、生态位三态机、水族馆群集算法（boids ≤200 粒）、长曝光色带合成、月相天文算法 | 壁纸层叠挂、画框/像素制式切换、光标光笔叠层 |
| S13 | 声音通知 | `modules/soundNova.ts` | 热图聚合、晨间电台文案编排、双耳补偿曲线、和声冲突频段表、队列聚光状态机 | notifyStore 桥、TTS（novaVoice）、AnalyserNode、声纹库 |
| S14 | 无障碍+ | `modules/a11yNova.ts` | 声令词表匹配（20 条封闭语法）、色觉配方改写矩阵、镜像键位变换表、盲文映射表 | 布局放大重排（U-12 网格对接）、听诊三通道、词典 300 条本地库 |
| S15 | UI 度量 | `modules/designNova.ts` | 黄金比/三分参考线几何、白空五线谱映射（间距→音高）、动效三元一致性、配重算法、密度三维、WCAG 对比计算 | dev 覆盖层（6 件工具）、DOM 采样巡检、报告导出 |
| S16 | 工程收官 | `modules/qualityNova.ts` | 能效三轴评级、入眠换出策略、单步重放驱动、指纹哈希树、星象 12 指标映射、战绩里程碑 | 构建产物扫描、门禁结果对接（U-24/Z-63 同源）、礼炮动画 |

**每步门禁**：模块单测绿；`tsc --noEmit` 绿；hub 中该域功能可开关且立即生效/失效；该域 Rust 命令（若有）`cargo check` 绿。

## 3. S17 接线（对既有文件的全部改动清单）

| 文件 | 改动 | 级别 |
|---|---|---|
| `src/system/desktop/DesktopShell.tsx` | `import "../nova/activate";`（紧邻既有 singularity 激活行之后） | 1 行 |
| `src/styles/global.css` | `@import "./nova.css";` | 1 行 |
| `src/system/palette/CommandPalette.tsx` | 注册「新星中枢 / Nova Hub」命令 + 域 7/15 的 overlay 工具命令 | ~12 行 |
| `src-tauri/src/lib.rs` | handler 表追加 4 条 nova 命令 | ~5 行 |
| `src-tauri/src/shell/mod.rs` | `pub mod nova;` | 1 行 |

**红线**：不改任何既有组件内部逻辑；不删/不改名任何既有导出（含 singularity 全部工件）；新样式一律 `nova-` 前缀；新事件一律 `nova://` 前缀并过 M-50 削峰纪律；localStorage 一律 `nova.*` 键空间。

## 4. S18 全量门禁自检（完成口径）

1. `npx tsc --noEmit` → 0 错误。
2. `npx vitest run` → 全绿（既有全量 + 新增 W 系列 16 套域测试）。
3. `cargo check` + `cargo test`（src-tauri）→ 0 错误全绿。
4. `node scripts/audit.cjs` → nova.css 零裸色值、`nova-`/`singu-` 类名零碰撞。
5. **共存冒烟**（重点）：singularity hub 与 nova hub 同开会话并存；两者开关状态互不串扰（键空间隔离验证）；`singu://` 与 `nova://` 事件流并行无风暴。
6. 冒烟清单（人工/浏览器核验）：
   - nova hub 开合（命令面板 → 新星中枢）；
   - 任一开关切换 → 刷新后状态保留（`nova.registry`）；
   - reduce-motion 模拟下全部 W 动效归零（含磁漂/弹道/礼炮/焚毁）；
   - 域 7 overlay（横桌/备忘板/时间梭）与域 15 dev 覆盖层开合正常；
   - `nova_pulse_ex` 无电池台式机：W-091/096 如实隐藏；
   - novaVoice 未安装：W-086/154/164 如实隐藏（不占位不假按钮）；
   - 语音/麦克风类功能与 Z-48 指示灯联动正确。

## 5. S19 交付同步

1. 三大文档入 `docs/`（全景/步骤/分工）。
2. 分域提交（S0 地基 → S1…S16 → S17 接线 → S18 修复 → S19 文档）。
3. push 至 GitHub `VariableXs/-Un-Real-0d23d9ux-Engine` main 分支；核对远端与本地一致。

---

## 6. 风险与回退

| 风险 | 缓解 |
|---|---|
| 200 项与 455 存量重复 | 附录 B 最近邻索引逐项比对（全景文档 §18） |
| 与 singularity 体系冲突 | 平行共存设计：独立键空间/类名前缀/事件前缀/挂载协议 feature 名；S18 专设共存冒烟门禁 |
| 新动效伤既有手感 | 全部默认关/轻量；零侵入挂载；reduce-motion 全降级；每项验收含性能上限（如磁漂不减帧率、弹道不阻塞窗口创建） |
| 语音功能体量与隐私 | novaVoice 独立 opt-in 件（模型显式下载）；未装即隐藏；识别全程离线 |
| Rust 命令平台差异 | 分核/电流/ACL/minidump 读取失败路径全部如实降级（N/A/隐藏），不假装成功 |
| 事件风暴 | `nova://` 前缀 + 复用 M-50 削峰 + 采样周期统一 ≥ 2s |
| 样式冲突 | `nova-` 前缀 + 只读叠层 + tokens 语义变量 + audit 类名碰撞门禁 |
| 性能回归 | 被动 API 优先；hub 全关时 Runtime 整体卸载；每项立项铁律含启动 ≤ 8ms / 内存 ≤ 6MB 约束 |

## 7. 施工顺序依赖图

```
S0 ──► S1  S2  S3  S4  S5  S6  S7  S8  S9  S10 S11 S12 S13 S14 S15 S16  （并行）
 │      │   │   │   │   │   │   │   │   │    │   │   │   │   │   │
 │      └───┴───┴───┴───┴───┴───┴───┴───┴────┴───┴───┴───┴───┴───┴──► S17 接线
 │                                                                  │
 └──────────────────（registry/hub/Runtime 为一切前置）──────────────►│
                                        novaVoice（opt-in 可后补）──►│
                                                                     ▼
                                                                   S18 门禁（含共存冒烟）
                                                                     ▼
                                                                   S19 交付
```

## 8. 与奇点计划（SINGULARITY-100）的关系声明

- **平行不合并**：nova 不复用 singularity 的 registry/hub/Runtime 工件（避免耦合导致的连锁故障），仅复用全局基建（ai04 挂载协议、tokens、M-50 削峰、M-90 午夜基座等只读公共件）。
- **互斥语义**：视觉/声音类功能与 Q 系列近似项（如 W-148 像素制式与 Q-80 材质滑块）在 hub 中互相感知：同类氛围层同时开启时取「强度较小者」并提示（防止叠加过曝）。
- **退役路径**：若某 W 项与 Q 项长期二选一，用户在 hub 可「封存」其一（封存≠删除，随时唤回）。

---

*NOVA-200 实施总步骤图 · 完*
