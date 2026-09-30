# Variable UI 品质深化 · 完整方案与施工步骤

> 目标：把「代码分析工作台」（VS Code 式范式，基准图1）与「Variable 系统界面」（Win11 设置式范式，基准图2）统一进一套 Variable 设计语言，让 AI 设计的 UI 从"检查项通过"升级为"看得见的高品质"。
> 范围：方案 A（UI 品质深化包）+ B（视觉回归闭环）+ C（高频界面重设计）+ D（组件库先行）四线并进。
> 基准：`src/design/tokens.css`（单一事实源，OKLCH 语义层→亮度层）；两套基准图的版式解构见 §1。

---

## 0. 两张基准图的设计语言解构（先对齐再动手）

### 图1 · 代码分析工作台（VS Code 范式）——多栏工作台语言
| 版式要素 | 图1 表现 | 收编进设计语言的方式 |
|---|---|---|
| 顶部菜单栏 | 文件/编辑/选择/查看/转到/运行 + `…` 溢出 | `MenuBar` + `Menu` 组件；菜单项 h 36px（`--ctl-menu-item`） |
| 活动栏（左缘竖条） | 48px 竖排图标，选中项左侧 2px 强调条 | `ActivityBar` 组件；选中态 = `--sel-bar` 左条 + icon 变 `--text-primary` |
| 侧栏（资源管理器/搜索替换） | 320px 树 + 搜索/替换输入行 + `…` 行内溢出 | `SidePane` + `Tree` + `InlineFind`；树行 h 22px（紧凑密度） |
| 编辑器欢迎区 | 居中大 Logo + 快捷键对照列表 | `WelcomeHero` 组件；快捷键用 `Kbd` 键帽组件 |
| 状态栏 | 底部 22px：错误/警告计数、登录、Go Live、通知铃 | `StatusBar` 组件；左右分区 + `Badge` 计数 |
| 暗色底 | 近黑蓝（oklch L≈0.14）与 tokens `--bg-canvas` 一致 | 直接复用现有语义层 |

### 图2 · Variable 系统界面（Win11 设置范式）——卡片导航语言
| 版式要素 | 图2 表现 | 收编进设计语言的方式 |
|---|---|---|
| 顶部搜索 | 居中胶囊搜索框「查找设置」 | `SearchPill` 组件；h 40px、全宽 min(640px, 60%) |
| 左导航 | 300px：返回键 + 账户卡 + 图标导航项，选中项整行 `--bg-raised` + 左 3px 条 | `NavRail` 组件；导航行 h 44px、icon 20px（`--icon-size`） |
| Hero 卡 | 设备卡片（缩略图 + 名称 + 重命名链接）+ 右侧三个服务入口 | `HeroCard` 组件；高度 96px、缩略图 16:9、圆角 `--r-card` |
| Chevron 列表 | 大圆角行卡：左 icon 40px + 标题/副标题双行 + 右 `›` | `SettingsRow` 组件；行高 ≥72px、hover 抬升 `--elev-hover`、整行可点 |
| 图标 | 彩色微软风线性图标 | 统一 `AppGlyphs` 补全至 25 页面 ×1 套；线宽 1.5px |
| 群组卡片 | 设置页整页为一张大圆角容器，行与行以 1px 分隔 | `CardGroup` 组件；圆角 `--r-card`、内边距 `--sp-2` |

### 融合原则（贯穿全部步骤）
1. **一套 token、两种密度**：工作台用 compact 密度（行高 22/32px），系统界面用 comfortable 密度（行高 44/72px）——已有 `[data-density]` 档位，不新造变量。
2. **同一强调色与选中语法**：左缘竖条（工作台）与整行填充+左条（系统界面）都是 `--accent` 系，规则写死在组件里。
3. **同一圆角/阴影/动效体系**：窗口 16 / 卡片 12 / 控件 8 / 小件 4，阴影 0~6 档，动效走 `--ease-standard/--dur-*`。
4. **中文字体渲染铁律**：CJK 走 `cjk.css` 回退链，字距 `--w2-tracking` 默认 0，行高 `--w2-lh: 1.6`。

---

## 1. 总体阶段划分（D → A → B → C → 入册）

| 阶段 | 内容 | 产物 | 依赖 |
|---|---|---|---|
| P0-D | 组件库 10 → 25+ 件套（token 化） | `src/components/kit/`（tsx + kit.css + barrel） | 无 |
| P1-A | 材质系统扩容 | `material.css` 98 → 400+ 行（acrylic/mica/噪点/发光 ×5 档） | P0-D |
| P2-A | 双范式布局容器落地 | MenuBar/ActivityBar/SidePane/StatusBar + NavRail/HeroCard/SettingsRow | P0-D + P1-A |
| P3-A | UI 品质深化包入册（5 族 125 项） | 全景图第二期扩编章节 + checks 注册表 | P0~P2 |
| P4-B | 视觉回归闭环 | `tools/visual-regression/`（Playwright 截图基线 + diff + 报告） | P0-D |
| P5-C | 高频界面 2~3 套视觉稿 | `design-mockups/`（自包含 HTML，双范式×3 变体） | P1-A |
| P6 | 落地与验收 | 选稿实施 + CI 接线 + 文档同步 | 全部 |

---

## 2. P0-D · 组件库 25 件套（组件库先行）

### 2.1 件套清单（25 个，分五组，对应全景图族0351「组件目录 25 个」口径）

**基础输入组（8）**：`Button`（primary/secondary/danger/subtle ×4 变体）· `IconButton` · `Input`（含清除钮+前缀图标）· `TextArea`（自动高度）· `Select`（原生增强+键盘 roving）· `Checkbox` · `Radio` · `Switch`（对齐现有 44×20 规格）

**容器与布局组（6）**：`Card` · `CardGroup`（Win11 设置页整卡容器）· `SidePane`（图1 侧栏）· `Divider` · `Toolbar`（溢出折叠进 `…`）· `Tabs`（下划线式+胶囊式 ×2 变体）

**反馈与状态组（6）**：`Badge`（99+ 规则复用领域15 算法）· `Chip` · `Toast`（复用现有 ToastHost，抽样式进 kit.css）· `Tooltip`（吃 `--tooltip-*` 全套令牌）· `Spinner` · `Skeleton`（骨架屏，三形状：text/rect/circle）

**数据与展示组（5）**：`Progress`（条式）· `ProgressRing`（环式，dashoffset 公式复用）· `Avatar`（字/图两态+状态点）· `Breadcrumb`（中段省略复用领域15 算法）· `Kbd`（键帽，图1 欢迎区/图2 搜索框快捷键提示通用）

**导航与信息组（6，可选加做至 31）**：`MenuBar`（图1 顶栏）· `ActivityBar`（图1 活动栏）· `StatusBar`（图1 底栏）· `NavRail`（图2 左导航）· `SettingsRow`（图2 Chevron 行卡）· `HeroCard`（图2 Hero 卡）

### 2.2 施工步骤
1. 新建 `src/components/kit/`：`index.ts`（barrel）+ `kit.css`（唯一新样式文件，全部引用 token，**禁止裸值**，audit.cjs 会扫）。
2. 每组件一文件，API 遵循现有风格：函数组件 + props 内联类型；事件/i18n 逻辑薄、样式全走 kit.css 类名（`k-` 前缀防冲突）。
3. 状态三态全表：`hover`（抬升 `--elev-hover`）/ `active`（下压 `--w2-press-shift`）/ `disabled`（`--w2-disabled-alpha`）+ 键盘 `:focus-visible` 吃 `--focus-ring`。
4. ARIA 基线：可折叠必带 `aria-expanded`、对话框必带 `role/aria-modal`、roving tabindex 用于 Tabs/Menu/Select。
5. `src/components/kit/__tests__/kit.test.tsx`：每组件 ≥1 渲染断言 + 领域15 已有逻辑模型（Switch/Stepper/Tab 等）与真实组件对齐。
6. 验收：`npx tsc --noEmit` 0 错；vitest 全绿；`npm run audit`（若已接 audit.cjs）0 裸值。

---

## 3. P1-A · 材质系统（material.css 扩容）

### 3.1 材质五档（每档 ×5 级强度，共 25 档，对齐族0049/族0293 口径）
| 材质 | 实现 | 用途 |
|---|---|---|
| `m-solid` | 纯 `--bg-raised` | 对话框、菜单 |
| `m-frosted` | `backdrop-filter: blur(var(--w2-blur-2))` + `--bg-surface` | 任务栏、开始菜单 |
| `m-acrylic` | frosted + `--texture-grain` 噪点叠加 + 饱和度(1.2) | 快捷面板、通知中心 |
| `m-mica` | 壁纸取色透传（低透明 `--bg-surface`，不 blur 自身内容） | 窗口标题栏、设置页底 |
| `m-glow` | `--w2-glow` 辉光边 + 渐变描边 | 选中窗口、品牌时刻 |

HC 主题铁律：全部材质降级为 `m-solid` + 2px 对比描边（复用现有 `[data-theme="high-contrast"]` 覆写段）。reduce-motion 下辉光动画降为静态。

### 3.2 施工步骤
1. `material.css` 追加 `.m-*` 五档 + `.m-*-1..5` 强度级（透明度/模糊/噪点 opacity 三参数联动）。
2. `tokens.css` 补三变量：`--mat-blur`、`--mat-saturation`、`--mat-noise-opacity`（档位层可被 `[data-material="…"]` 运行时覆写）。
3. 逐表面替换：任务栏→m-frosted、快捷面板→m-acrylic、设置页→m-mica（每处一行 class 替换，不动布局）。
4. 验收：HC/low-perf 降级截图对比（依赖 P4 的视觉回归基线）。

---

## 4. P2-A · 双范式布局容器（把两张图"长"进组件里）

施工顺序（每件：组件 → 接入现有页面 → kit 测试）：
1. `MenuBar` + `Menu`（图1）：菜单栏溢出进 `…`；菜单项快捷键右对齐用 `Kbd`。
2. `ActivityBar`（图1）：48px 竖条 + 左 2px 选中条 + 底部设置齿轮位。
3. `SidePane` + `Tree`（图1）：资源管理器/搜索双模式；树行 22px、展开三角 chevron 旋转过渡。
4. `StatusBar`（图1）：左（问题计数/分支）右（服务状态/铃铛）两分区，间距 `--sp-2`。
5. `NavRail`（图2）：返回键 + 账户区插槽 + 导航项（icon+文字，h 44px，选中整行 `--bg-raised`+左 3px `--accent` 条）。
6. `SearchPill`（图2）：居中胶囊搜索；`Ctrl+F` 提示用 `Kbd`。
7. `SettingsRow`（图2）：icon 40px 容器 + 双行文本（title 15px/secondary 12px）+ `›`；整行按钮语义（`role=button`/真实 `<button>`）。
8. `HeroCard`（图2）：左 16:9 缩略图 + 标题/副标题/行内链接 + 右侧最多 3 个服务入口（超出折叠 `…`）。
9. `WelcomeHero`（图1）：居中 Logo + 快捷键对照列表（`Kbd` 组合）。

---

## 5. P3-A · UI 品质深化包入册（5 族 125 项，F10001~F10125）

在《AURORA-10000-功能全景图》追加「第二期扩编 · UI 品质」章节（不破坏 F00001~F10000 的 unique 校验，新章独立计数）：

| 族号 | 族名 | 区间 | 内容 |
|---|---|---|---|
| 族0401 | 组件库 25 件套落地 | F10001~F10025 | 每件套一项：API/状态三态/ARIA/token 化 |
| 族0402 | 材质系统五档 | F10026~F10050 | 五档 ×5 强度 + HC/低性能降级 |
| 族0403 | 双范式布局容器 | F10051~F10075 | 图1 工作台四件 + 图2 系统界面五件 |
| 族0404 | 数据可视化语言 | F10076~F10100 | 图表配色（OKLCH 序列）、轴/图例/网格规范、空数据态、大数值缩写、tooltip 锚定 |
| 族0405 | 微交互与插画体系 | F10101~F10125 | hover/press/release 曲线全表、骨架→内容换场、空态插画 25 套、加载分档（>300ms 才显示） |

配套：`src/features/uikit/` 增 `groupF.ts` + checks 注册表 125 项断言（复用 memoized 单次求值模式）；vitest 用例同步。

---

## 6. P4-B · 视觉回归闭环

### 6.1 流程
1. `tools/visual-regression/capture.mjs`：Playwright 启动 dev server → 按清单逐页截图（桌面/任务栏/开始菜单/设置/文件管理器/代码分析工作台 + kit 组件单页 `?story=kit`），存 `tools/visual-regression/baseline/`。
2. `tools/visual-regression/diff.mjs`：pixelmatch 对比 baseline vs current，差异 > 0.1% 列出报告 `report.html`（滑轨对比图）。
3. 基线更新规则：确认是「有意的视觉变更」才 `--update-baseline`（防基线只增不审）。
4. CI 接线：`build-windows.bat` 或 GitHub Actions 里加 `node tools/visual-regression/diff.mjs --ci`，diff 失败即红。
5. 矩阵：3 主题（dark/light/high-contrast）× 2 密度（compact/comfortable）× 关键页 ≈ 36 张基线起步。

### 6.2 验收
- 新基线一次成型；人为改坏一个组件（如去掉 hover 态）diff 能红。

---

## 7. P5-C · 高频界面视觉稿（2~3 套，自包含 HTML）

### 7.1 三套变体方向
| 变体 | 气质 | 关键决策 |
|---|---|---|
| V1「晨雾」 | 图2 原生 Win11 血统强化：m-mica 窗体、强调色跟随壁纸取色、圆角拉满 | 稳、最像"系统该有的样子" |
| V2「工作台」 | 图1 职业密度：compact 密度、菜单栏+活动栏完整、适合代码分析/工具场景 | 犀利、信息密度最高 |
| V3「极光」 | 品牌向：m-glow 辉光 + 动效曲线 emphasize，启动剧场语言延伸到日常界面 | 个性最强、演示用 |

### 7.2 覆盖界面（每套 5 屏）
桌面+图标 / 任务栏+开始菜单 / 设置（图2 范式）/ 文件管理器 / 代码分析工作台（图1 范式）。
产物：`design-mockups/v1.html / v2.html / v3.html`（零依赖单文件，内嵌 tokens 值；顶部变体切换器方便对比）。

### 7.3 步骤
1. 抽取 tokens.css 当前值内联进稿 → 保证"稿即所得"。
2. 每屏用 §4 布局容器同名 class 搭建 → 落地时组件可直接对照。
3. 你选稿（或混搭，如 V1 设置 + V2 工作台）→ 我按稿改真实组件与页面。

---

## 8. P6 · 验收口径（全阶段统一）

1. **token 铁律**：新代码零裸值（px/颜色直接量只允许出现在 tokens.css/material.css 的档位定义处）。
2. **三态 + 键盘**：每个可见交互件 hover/active/disabled/focus-visible 齐全，Tab 顺序合理。
3. **HC 不降级红线**：high-contrast 下信息仍可达（材质→纯色、辉光→描边、动效→80ms）。
4. **中文渲染**：CJK 行高 1.6、字距 0、`cjk.css` 回退链不被覆盖。
5. **回归绿**：typecheck 0 错、vitest 全绿、视觉 diff 绿、ID 校验 unique 不含冲突。
6. **文档同步**：tokens.css 头注释 ↔ docs/DESIGN.md 同一次提交内更新。

---

## 9. 一页执行清单（按序勾选）

- [ ] P0-D：`src/components/kit/` 25 件套 + kit.css + 测试（§2）
- [ ] P1-A：material.css 五档材质 + tokens 三新变量 + 逐表面替换（§3）
- [ ] P2-A：9 个布局容器组件并接入现有页面（§4）
- [ ] P3-A：全景图「第二期扩编」5 族 125 项 + uikit groupF checks（§5）
- [ ] P4-B：visual-regression 脚本 + 36 张基线 + CI 接线（§6）
- [ ] P5-C：design-mockups 三套十五屏（§7）
- [ ] P6：选稿落地 + 全部验收口径过绿（§8）
- [ ] P7：VS Code 工作台完整版式 × 现有功能落位（§10）
- [ ] P8：Windows 界面完整版式 × Variable 系统界面落位（§11）
- [ ] P9：数值级设计规范对稿（§12，施工时逐表核对）
- [ ] P10：kit 规格书逐件验收（§13，36 件含 ★ 新件）
- [ ] P11：逐屏蓝图标注核对（§14）
- [ ] P12：动效编排表落地 + 手势接入（§15）
- [ ] P13：取色流水线 + 主题六元组持久化（§16）
- [ ] P14：命令注册表 + 全局键位过冲突检测（§17）
- [ ] P15：M1~M6 里程碑推进至量化指标全达标（§18）

---

## 10. P7 · VS Code 工作台完整版式 × 现有功能落位（app-code 专属）

> 目标：把 `src/entries/app-code`（含 `src/apps/code` 五大视图 + `code-analysis` 引擎）的界面补成**完整版 VS Code 工作台**，每个版式槽位都绑定一个仓库里已存在的真实功能，不做空壳。

### 10.1 五大区总布局（grid 定义）
```
┌───────────────────────── MenuBar（22px）─────────────────────────┐
┌──┬──────────────┬──────────────────────────────┬───────────────┐
│A │  SidePane    │   EditorArea（Tab 条 35px）   │  AuxPane      │
│c │  (240~320px, │   + 面包屑 + 编辑区           │  (0~340px,    │
│t │  可折叠)      │   + Panel 区（底部，可折叠）   │  可折叠)       │
│i │              │                              │               │
├──┴──────────────┴──────────────────────────────┴───────────────┤
│                        StatusBar（22px）                        │
└─────────────────────────────────────────────────────────────────┘
```
- 分栏拖拽宽度持久化（复用族0034 窗口状态持久化的 store 模式）。
- 全部四栏可折叠；折叠态留 48px 图标条（ActivityBar 常驻）。

### 10.2 MenuBar 菜单项 × 现有功能逐一落位
| 菜单 | 菜单项 | 绑定的现有功能（真实落点） |
|---|---|---|
| 文件(F) | 打开文件夹… / 打开最近 / 另存为… | `ingest.ts`（目录摄取）/ 会话快照 `SnapshotManager.tsx` / `generate.ts`（报告导出） |
| 编辑(E) | 撤销/重做/查找/替换 | 复用领域05 撤销与历史（族0116）+ `SearchPanel.tsx` 查找替换 |
| 选择(S) | 全选/展开选择/添加下个匹配 | 编辑器标准命令，接 `writeEngine.ts` 的编辑命令层 |
| 查看(V) | 命令面板… / 资源管理器 / 搜索 / 剖析 / 大纲 / 缩放 | 全局搜索中枢（族0086）/ 各 SidePane 视图 / `ProjectAnalysisView.tsx` / `deepDescribe.ts` 大纲 |
| 转到(G) | 转到文件 Ctrl+P / 转到符号 Ctrl+Shift+O / 转到定义 F12 | `XrefPanel.tsx`（交叉引用跳转）/ `anatomy.ts` 符号表 |
| 运行(R) | 运行全部检查 / 运行当前族 / 基线对比 | `code-analysis` core 的 `run_all_checks` / 单 CheckSet / 基线比对 |
| 终端(T) | 新终端 / 运行任务… | 预留面板壳（接口冻结口径），接 build-windows.bat 任务清单 |
| 帮助(H) | 快捷键参考 / 关于 | `KeymapOverlays.tsx` / `about/` |
| `…` 溢出 | 低频项折叠 | Toolbar 溢出算法（族0351 已实现）复用 |

### 10.3 ActivityBar（48px 活动栏，7 个槽位全落现有功能）
| # | 图标 | 视图 | 落点 |
|---|---|---|---|
| 1 | 文件 | 资源管理器 | 仓库树（ingest 结果），对齐图1 |
| 2 | 放大镜 | 搜索 | `SearchPanel.tsx`（含搜索/替换双行，对齐图1） |
| 3 | 分叉 | 交叉引用 | `XrefPanel.tsx` |
| 4 | 图表 | 项目剖析 | `ProjectAnalysisView.tsx` + `ProjectVizPanels.tsx` |
| 5 | 心智图 | 知识图谱 | `src/apps/mind`（MindmapView 以视图形式嵌入编辑区） |
| 6 | 扩展 | 市场 | settings `Marketplace.tsx` 视图化 |
| 7 | 齿轮 | 设置 | `SettingsModal.tsx`（底部固定，对齐图1 布局） |

### 10.4 EditorArea（编辑区）
- **Tab 条**：打开的分析视图 tab（剖析/图谱/写作/命运），拖拽重排复用族0032 窗口分组标签算法；tab 溢出滚动。
- **欢迎页**（对齐图1）：居中 Logo（启动剧场语言延续）+ 快捷键对照（打开聊天/所有命令/开始调试 → 换为本项目三条：`Ctrl+Shift+P` 命令面板、`Ctrl+P` 转到文件、`F5` 运行检查），用 `Kbd` 组件。
- **Aurora 画布**：`AuroraCanvas.tsx` + `auroraShader.ts` 作为分析可视化主舞台，材质走 `m-mica`（§3）。
- **内嵌应用**：`write`（写作引擎）、`fate`（行为树）、`mini` 五件（计算器/倒计时/笔记/番茄钟/世界钟）以 tab 形式打开——mini 五件收进右侧 AuxPane 小窗模式。

### 10.5 Panel 区（底部）
| 面板 | 落点 |
|---|---|
| 问题 | checks 失败列表（severity 图标 + 点击跳转，对齐图1 状态栏「⚠25」口径） |
| 输出 | run_all_checks 流式日志（族0381 可观测性落点） |
| 调试控制台 | 预留壳 |
| 终端 | 预留壳（同 10.2） |

### 10.6 StatusBar（22px，左右分区）
- 左：问题计数 `✖0 ⚠N`（点击开问题面板）· 当前分析族 ID · 分支名（`GitPanel.tsx` 联动）。
- 右：登录/身份 · Go Live（dev server 状态）· 引擎标识（code-analysis core 版本）· 通知铃（`ToastHost` 汇总）。
- hover 弹 tooltip 全走 `--tooltip-*` 令牌（M-88 规范）。

### 10.7 命令面板（F1/Ctrl+Shift+P）
- 复用族0086 全局搜索中枢 + 族0087 快速启动器的既有算法；本步骤只做工作台皮：`Kbd` 提示、最近使用置顶、模糊匹配高亮。
- 命令注册表 = 10.2~10.6 全部菜单项 + 视图切换命令，一处注册、菜单/面板/键位三处消费（避免三张皮）。

### 10.8 施工步骤与验收
1. `src/apps/code/` 增 `workbench/` 子目录：`MenuBar.tsx / ActivityBar.tsx / SidePane.tsx / EditorTabs.tsx / AuxPane.tsx / PanelArea.tsx / StatusBar.tsx / CommandPalette.tsx`（全部从 kit 导基础件）。
2. `app-code` 入口改为五区 grid（§10.1），旧布局整页迁移不并存。
3. 命令注册表 `workbench/commands.ts`：`{id, title, keybinding, run}`，菜单树由它派生。
4. 视觉回归基线新增工作台 4 张（欢迎页/剖析视图/图谱视图/问题面板展开）。
5. 验收：五区均可折叠且布局持久化；命令面板可触发全部菜单命令；`Kbd` 快捷键与 KeymapOverlays 无冲突（快捷键纪律红线）。

---

## 11. P8 · Windows 界面完整版式 × Variable 系统界面落位（desktop/explorer/settings 专属）

> 目标：把图2 的 Win11 设置范式扩展成**完整的 Windows 级系统界面**——设置全页、开始菜单、任务栏、快捷面板、通知中心、文件管理器——每个槽位绑现有功能。

### 11.1 设置应用（图2 范式完整化）
**左导航 NavRail 项 × 现有 SettingsModal 27 Tab 收敛映射**（27 个平铺 Tab 收敛为 11 个导航页，每页内再分卡片组）：

| 导航项（对齐图2） | 图标 | 收编的现有 Tab |
|---|---|---|
| 主页 | 房子 | HeroCard（设备名+快捷入口）+ 4 个状态卡（更新/备份/安全/性能，取 Perf/Quality/Security 摘要） |
| 系统 | 显示器 | SystemCenterTab + SnapshotManager + StorageRecoveryTab |
| 蓝牙和其他设备 | 蓝牙 | hardware 功能域设备页（族0179 外设中心） |
| 网络和 Internet | Wi-Fi | NetworkTab + BrowsersTab |
| 个性化 | 画笔 | VisionTab + AmbienceTab + BootTheaterTab（壁纸/主题/氛围/开机剧场） |
| 应用 | 方块 | ExtensionsTab + Marketplace + WinFeelTab |
| 账户 | 人 | 会话与身份（族0239）+ 密码管理器入口（族0167） |
| 时间和语言 | 时钟 | a11y-l10n 本地化页 + 输入法（族0094） |
| 辅助功能 | 无障碍人形 | A11yTab + InputFeelTab（输入无障碍） |
| 隐私和安全性 | 盾 | SecurityTab + AuroraD4Tab |
| Windows 更新 → **系统更新** | 循环箭头 | QualityTab + PerfTab + CodeDeployCard |

**页面结构规范（逐屏统一）**：
- 页头：页名 28px（`--fs-24` 加大档）+ 页级说明 secondary。
- 内容 = 若干 `CardGroup`（大圆角整卡）× `SettingsRow`（Chevron 行卡）× `SettingsToggleRow`（行内 Switch 变体，kit 补做第 26 件）。
- 行卡规格照 §0 图2 解构：h≥72、icon 40px 容器、双行文本、右 `›`；开关行右端为 Switch。
- 顶部 `SearchPill`：搜索范围为「11 页 × 全部行卡标题/副标题」，命中高亮 + 页内跳转（复用族0083 开始菜单搜索算法）。

### 11.2 开始菜单（族0081~0085 已有逻辑，本步骤做 Win11 皮）
- 三段式：**搜索**（SearchPill 复用）→ **已固定网格**（6 列磁贴，页指示器）→ **推荐区**（最近文件，`files` 功能域联动）+ 底栏（头像=账户页入口 / 电源=关机重启仪式 族0019）。
- 材质 `m-acrylic`；打开动效：从任务栏锚点生长（`--ease-emphasized` + `--dur-5`）。
- 全部列表项用 `SettingsRow` 紧凑变体（h 48px）。

### 11.3 任务栏（族0076~0080）
- 居中图标组（Win11 布局）：开始 · 搜索 · 任务视图 · 固定应用 · 运行中应用（下划点指示，运行=短点/激活=长条）。
- 右侧托盘：快捷面板箭头 · 系统图标（网络/音量/电池，点击即快捷面板）· 时钟日期（点击=通知中心+日历，族0154/0313）· 「显示桌面」细条。
- 材质 `m-frosted`；hover 预览缩略图复用族0042 窗口嗅探。

### 11.4 快捷面板（族0090）与通知中心（族0088~0089）
- 快捷面板：Wi-Fi/蓝牙/飞行模式/省电/勿扰/投影 六大开关磁贴（磁贴点亮=`--accent`）+ 亮度/音量滑杆（kit `Slider`）+ 电池详情。落点：`sound`/`hardware`/`background` 功能域既有开关。
- 通知中心：通知卡组（按应用分组 + 「全部清除」）+ 下半月历（`mind` 的日历算法复用）。勿扰联动族0309。

### 11.5 文件管理器（`src/entries/explorer`，Win11 资源管理器范式）
- 工具栏：新建 ▾ · 剪切/复制/粘贴/重命名/删除 · 排序 ▾ · 查看 ▾（大图标/列表/详细信息）· `…`。
- 左侧 NavRail 紧凑变体：主页/图库/收藏/最近 + 磁盘树（`files` 功能域核心 族0126）。
- 地址栏=面包屑（kit `Breadcrumb`）+ 每段可下拉；右侧搜索框（族0128 文件搜索）。
- 状态栏：选中 N 项 · 容量条（族0132 磁盘与空间）。

### 11.6 桌面层（`src/entries/desktop`）
- 右键菜单 = kit `ContextMenu` 分组规范（查看/排序方式/刷新/新建/显示设置/个性化，分组间 `Divider`）。
- 桌面图标双击/框选/拖拽rubber band 全走 `interactions.css` 既有令牌；壁纸引擎（族0056）之上盖 `m-mica` 取色联动（族0057 既有算法）。

### 11.7 施工步骤与验收
1. settings 重构：`SettingsModal.tsx` 平铺 Tab → `SettingsApp`（NavRail + 页路由 + CardGroup 页骨架），27 个 Tab 按映射表归位（每 Tab 内容不改，只动容器）。
2. kit 补做第 26 件 `SettingsToggleRow` 与第 27 件 `TaskbarItem`（含运行指示态）。
3. `src/entries/desktop` 增 StartMenu/QuickPanel/NotificationCenter 三浮层（材质/动效按 §11.2~11.4）。
4. explorer 工具栏与 NavRail 替换（内容区不动）。
5. 视觉回归基线新增：设置 11 页缩略 + 开始菜单/快捷面板/通知中心/explorer 共 15 张。
6. 验收：27 Tab 全部可达（搜索能命中每一行卡）；四浮层 Esc/点击外部可关；HC 下浮层材质降级后全部文字仍达标；typecheck/vitest/视觉 diff 全绿。

---

## 12. P9 · 数值级设计规范（全部从 tokens.css 现值推导，稿即所得）

### 12.1 字阶与文本（CJK 铁律内嵌）
| 用途 | token | 值 | 行高/字重 |
|---|---|---|---|
| 设置页名 | --fs-24→加大 | 28px | 1.3 / 600 |
| 窗口标题 | --fs-13 | 13px | 1.4 / 600 |
| 行卡标题 | --fs-15 | 15px | 1.6 / 400 |
| 行卡副标题 | --fs-12 | 12px | 1.5 / 400 --text-secondary |
| 菜单项/按钮 | --fs-13 | 13px | 1 / 400 |
| 状态栏 | --fs-12 | 12px | 1 / 400 |
| 代码/ID | --w2-font-mono | Cascadia Mono | 1.5 |
- 全局行高 `--w2-lh: 1.6`；字距 `--w2-tracking: 0`；CJK 回退链恒在（cjk.css）；数字列用 `font-variant-numeric: tabular-nums`（状态栏/表格/时钟）。

### 12.2 色彩语义（dark 为基准，light/HC 走既有亮度层覆写）
| 语义 | 变量 | dark 值 | 使用纪律 |
|---|---|---|---|
| 画布 | --bg-canvas | oklch(0.14 0.02 262) | 只用于窗口底，禁直接 #hex |
| 表面 | --bg-surface | 0.18 / 0.72 透明 | 侧栏、卡片 |
| 抬升 | --bg-raised | 0.20 / 0.92 | 浮层、选中行 |
| 强调 | --accent | oklch(0.68 0.09 262) | 选中条/主按钮/焦点环，同屏 ≤3 处主强调 |
| 强调底 | --accent-soft | 0.16 透明度 | 磁贴点亮底、hover 行 |
| 成功/警告/危险 | --success/--warn/--danger | 0.78·160 / 0.82·85 / 0.68·25 | 仅状态语义，禁做装饰色 |
- 对比度红线：正文 ≥4.5:1、大字/图标 ≥3:1（AA）；强调色上一律白字或 `--bg-canvas` 字，取较优者。

### 12.3 间距与尺寸节奏
- 4px 基格：组件内 padding 用 `--sp-1/2`，组件间 `--sp-3/4`，区块间 `--sp-5/6`。
- 控件高度三密度：紧凑 28（工作台）/ 标准 32（`--ctl-btn`）/ 触达 44（`--ctl-touch` 红线）。
- 圆角语义不混用：窗口16 > 卡12 > 控8 > 片4；行卡内嵌开关仍用控 8。

### 12.4 海拔与光影
| 层 | token | 用途 |
|---|---|---|
| 0 | --elev-0 | 画布、桌面 |
| 1~2 | --elev-1/2 | 侧栏、行卡 hover |
| 3 | --elev-3 | 菜单、下拉（--w2-menu-shadow） |
| 4~5 | --elev-4/5 | 模态、开始菜单 |
| 6 | --w2-elev-6 | 拖拽中的窗口 |
- 光源恒左上 45°；拖拽=--elev-drag、hover=--elev-hover；HC 全降 2px 描边（既有规则）。

### 12.5 材质参数表（§3 的可执行数值）
| 档 | blur | saturate | 噪点 opacity | 底透明度 |
|---|---|---|---|---|
| m-frosted 1~5 | 8/12/16/20/28px | 1 | 0 | 0.55→0.75 |
| m-acrylic 1~5 | 同上 | 1.2 | 0.02→0.05 | 0.60→0.80 |
| m-mica 1~5 | 0（取色不糊内容） | 1.1 | 0.03 | 0.40→0.70 |
| m-glow | — | — | — | 描边渐变 + --w2-glow 辉光 1~5 强度 |
- 低性能（`navigator.hardwareConcurrency ≤ 4` 或省电档）自动降 acrylic→frosted→solid 两级。

---

## 13. P10 · 27 件套逐件规格书（API + 状态矩阵 + ARIA）

> 统一约定：类名前缀 `k-`；每件列出【Props / 状态矩阵 / ARIA / token 引用】；★=新增深化的第 26~27 件及工作台/系统件。

| # | 组件 | 核心 Props | 状态矩阵（全件必有） | ARIA 要点 |
|---|---|---|---|---|
| 1 | Button | variant(pri/sec/danger/subtle) size(sm/md/tg) loading | hover抬1/active压1/disabled 0.4/loading 转圈占宽 | 真按钮元素；loading 时 aria-busy |
| 2 | IconButton | label(必填，tooltip 用) | 同上+tooltip 延迟 --hover-delay | aria-label 必填 |
| 3 | Input | prefix icon clearable invalid | hover/focus 环/disabled/readonly/invalid 红描边 | aria-invalid + 描述关联 |
| 4 | TextArea | autoResize maxRows | 同 Input + 字数（可选） | — |
| 5 | Select | options roving | 展开/高亮/选中/禁用项 | role=listbox+option；roving tabindex |
| 6 | Checkbox | indeterminate | 勾/半选/禁 | 原生 input+label |
| 7 | Radio | group | 选中/禁 | radiogroup |
| 8 | Switch | checked size=44×20 | 开/关/禁；轨道 accent | role=switch aria-checked |
| 9 | Slider | min max step bubble | hover 显 bubble/拖拽/键盘 ±Home End | role=slider aria-valuenow |
| 10 | Tabs | variant(underline/pill) | 选中/禁/hover；键盘 roving | role=tablist/tab/tabpanel |
| 11 | Card | padding elevated clickable | hover 抬升（clickable 时） | clickable→role=button |
| 12 | CardGroup | title | — | — |
| 13 | SidePane | side width collapsible | 折叠/展开 200ms | aria-expanded 于折叠钮 |
| 14 | Divider | vertical inset | — | role=separator |
| 15 | Toolbar | overflow 算法 | 溢出进 `…` 菜单 | aria-haspopup |
| 16 | Badge | count max=99+ | 数字/点/胶囊 | aria-label 化数字 |
| 17 | Chip | removable selected | 选中 accent 底 | removable→按钮语义 |
| 18 | Toast | 复用 ToastHost | 队列上限 3（既有算法） | role=status |
| 19 | Tooltip | 复用 tooltip.ts | show300/hide150（既有令牌） | aria-describedby |
| 20 | Spinner | size | — | role=progressbar |
| 21 | Skeleton | shape(text/rect/circle) | 闪烁 1.4s；加载>300ms 才现 | aria-hidden |
| 22 | Progress | value | 缓动跟随 | progressbar |
| 23 | ProgressRing | value size | dashoffset 公式（既有） | 同上 |
| 24 | Avatar | src/fallback 字 status | 字/图/状态点三态 | alt 或 aria-label |
| 25 | Breadcrumb | items | 中段省略（既有算法） | nav+aria-current |
| 26 | ★Kbd | combo | 键帽底/描边/按下态 | — |
| 27 | ★SettingsRow | icon title desc toggleable | hover 抬/press 压/整行按钮 | toggleable 时内嵌 switch |
| 28 | ★MenuBar/Menu | commands 注册表派生 | 高亮/禁/分隔/子菜单 300ms 延迟 | role=menubar/menu/menuitem |
| 29 | ★ActivityBar | items badge | 选中左 2px 条 | role=tablist 纵向 |
| 30 | ★StatusBar | 左右分区 slots | — | — |
| 31 | ★NavRail | items accountSlot | 选中整行 raised+左 3px 条 | nav+aria-current |
| 32 | ★SearchPill | scope placeholder | focus 展开/ESC 收起 | role=searchbox |
| 33 | ★HeroCard | thumb title links≤3 | — | — |
| 34 | ★SettingsToggleRow | 同 SettingsRow+Switch | 行 hover 与 switch 独立 | switch 语义 |
| 35 | ★TaskbarItem | running/active badge | 下划点短/长、hover 预览 | aria-pressed |
| 36 | ★CommandPalette | commands 注册表 | 模糊高亮/最近置顶/空态 | role=combobox+listbox |

- kit.css 按 `k-<件>-<态>` 命名；态一律 token 引用；audit.cjs 扫裸值。
- 每件测试：渲染快照 1 + 状态矩阵逐态断言 + 键盘走查（可聚焦件必测 Tab/方向键）。

---

## 14. P11 · 逐屏像素级蓝图（ASCII 标注版）

### 14.1 工作台欢迎页（对齐图1 空态）
```
│ 资源管理器(320) │              中央区(bg-canvas)                 │
│  搜索[输入]      │        [Logo 96px, 呼吸动画 s=1.0]            │
│  树 22px 行     │      (Un)Real 0d23d9ux# Engine  (--fs-20/600) │
│                │   ── 快捷键对照(sp-6 顶距, 两列 220px) ──       │
│  dist ▸        │   打开命令面板      [Ctrl]+[Shift]+[P] (Kbd)   │
│  docs ▾        │   转到文件          [Ctrl]+[P]                 │
│    HEIGHTS.md  │   运行全部检查      [F5]                       │
│  src  ▸        │   最近打开(≤5, SettingsRow h40 紧凑变体)        │
├─ ⚠N · 族ID · 分支 ─────── 登录 · GoLive · 引擎vN · 🔔 ────────┤
```
标注：欢迎区元素全部垂直居中，Logo→标题 16px→列表 24px；`Kbd` 键帽 h22 圆角 4。

### 14.2 设置主页（对齐图2）
```
│ ←返回 │ [   🔍 查找设置(SearchPill h40 max640)   ]      – □ × │
│ 账户卡 │   系统(--fs-28/600, sp-6 上距)                         │
│ 主页  ◀│  ┌HeroCard h96: 缩略图16:9·设备名·重命名 | 365│云│更新┐│
│ 系统   │  └──────────────────────────────────────────────┘   │
│ 蓝牙   │  ┌CardGroup──┐ ┌CardGroup──┐ ┌CardGroup──┐          │
│ 网络   │  │🖥屏幕    ›│ │🔊声音    ›│ │🔔通知    ›│  (h72 行卡) │
│ 个性化 │  │副标题12px │ │          │ │          │           │
│ …11项  │  └──────────┘ └──────────┘ └──────────┘            │
```
标注：导航选中=整行 --bg-raised + 左 3px accent 条；行卡网格 minmax(320px,1fr) 3 列；页级滚动仅内容区。

### 14.3 开始菜单 / 快捷面板 / 通知中心（浮层三件）
```
开始菜单(640×680, m-acrylic, elev-4, 锚任务栏生长 dur-5)
  [SearchPill]  已固定 6×4 磁贴(88px, accent-soft 点亮) ·页点·
  推荐(≤6 SettingsRow h48)        │头像│⏻电源│
快捷面板(360, 同材质): 6 磁贴(2×3, 点亮=accent) + 亮度/音量 Slider + 电池行
通知中心(360): 通知卡组(应用分组, 全部清除) + 下半月历
```

### 14.4 文件管理器（explorer）
```
工具栏 h48: [+新建▾][✂📋📌][↑][🗑]│[排序▾][查看▾][…]  (m-solid)
面包屑: > 此电脑 > D > 项目 ▾每段   [🔍搜索框 240px]
NavRail 200: 主页/图库/最近/收藏 | 磁盘树     内容区(大图标/列表/详情三视图)
状态栏: 已选 N 项 · ██████░░ 128GB/512GB (Progress 条式)
```

---

## 15. P12 · 动效与手势语言（编排表）

### 15.1 时长×曲线配对表（全部已有 token，禁自造）
| 场景 | 时长 | 曲线 | 编排 |
|---|---|---|---|
| hover 抬升/色彩 | --dur-2 (120ms) | --ease-standard | 无延迟；离开 80ms |
| 菜单/下拉展开 | --dur-3 (170ms) | --ease-standard | 透明度+scale .98→1，transform-origin 顶 |
| 浮层（开始菜单/面板） | --dur-5 (240ms) | --ease-emphasized | 透明度+scale .96+8px 位移，从任务栏锚点 |
| 通知滑入 | --dur-4 (200ms) | --ease-spring | 右缘入，5s 消散（W0 口径） |
| 页面切换（设置路由） | --dur-3 | --ease-standard | 旧页 -8px 淡出，新页 +8px 淡入（方向感） |
| 骨架→内容换场 | --dur-3 | --ease-standard | 交叉溶解，禁跳变 |
| reduce-motion | --dur-1 (80ms) | 线性 | 全部位移→纯淡入淡出（既有规则兜底） |

### 15.2 手势（触屏/触控板）
- 双指滑动=内容滚动；双指捏合=缩放（图库/画布）；三指横滑=虚拟桌面切换（族0031 既有）；边缘右滑=通知中心、左滑=小组件（Win11 手势语法）。
- 触控命中 44px 红线在 compact 密度例外放宽至 40px 并记录豁免（族0099 口径）。

---

## 16. P13 · 主题与取色流水线（一壁纸一主题）

1. 壁纸入库 → 取色（族0057 既有算法输出 5 色 OKLCH 序列）→ 映射 accent/success/warn/danger 四语义（L 钳制 0.55~0.72，C 钳制 ≤0.13）→ 自动算 --accent-soft（16% 透明）→ 写入 `[data-palette="…"]` 档。
2. 对比度门禁：生成的 accent 上叠白字/黑字自动择优；不达标 → L 微调重算（≤3 轮）。
3. 主题持久化格式：`{wallpaper, palette, density, radius, motion, material}` 六元组，快照进 `SnapshotManager`（复用既有）。
4. HC 永不参与取色流水线（固定黑白黄）。

---

## 17. P14 · 键盘全景与命令注册表格式

### 17.1 注册表 schema（工作台/系统界面共用）
```ts
interface Command { id: string; title: string; category: '文件'|'视图'|…;
  keybinding?: string; when?: (ctx)=>boolean; run: ()=>void; icon?: string }
```
- 菜单树、命令面板、右键菜单三处消费同一注册表；`when` 谓词控制可见/可用。
- 快捷键纪律：注册时过 KeymapOverlays 冲突检测（既有工具），冲突即构建警告。

### 17.2 系统界面全局键位（新增，须过冲突检测）
| 键 | 动作 |
|---|---|
| Win / Ctrl+Esc | 开始菜单 |
| Win+A | 快捷面板 |
| Win+N | 通知中心 |
| Win+E | 文件管理器 |
| Win+I | 设置 |
| Win+Tab / 三指滑 | 任务视图 |
| Alt+Space | 窗口系统菜单 |
| F1（工作台内） | 命令面板 |

---

## 18. P15 · 里程碑、量化验收与风险

### 18.1 里程碑（W 波次式）
| 波次 | 内容 | 出口判据 |
|---|---|---|
| M1（P0-D+P1-A） | kit 27 件 + 材质五档 | tsc 0 错、vitest 新增 ≥40 例绿、audit 0 裸值 |
| M2（P2-A+P7/P8 骨架） | 容器接入五区工作台+设置 11 页 | 视觉基线 51 张成型（36+4+15→按 §14 实际）、0 红 |
| M3（P4-B） | 回归闭环进 CI | 人为破坏组件 diff 必红一次（演练） |
| M4（P5-C） | 三套十五屏视觉稿 | 你选稿签字 |
| M5（P3-A+P12~P14） | 入册 125 项 + 动效/键位/取色落地 | unique 校验不冲突、键位冲突 0 |
| M6 | 终验收 | §8 六条全绿 + 下表量化指标 |

### 18.2 量化指标（终验收硬门槛）
- 首帧（工作台欢迎页）≤ 300ms；浮层打开动画掉帧率 <1%（Performance 记录）。
- 同屏主强调色出现次数 ≤3；裸值审计 0；HC 模式下文字对比达标率 100%（脚本扫描）。
- 组件覆盖：settings 27 Tab、explorer、工作台五区 100% 使用 kit 件（grep 语义类名核验）。

### 18.3 风险与对策
| 风险 | 对策 |
|---|---|
| 27 Tab 迁移破坏既有深链/状态 | SettingsModal 保留路由别名映射 1 个版本；迁移前视觉基线先行 |
| 材质低性能机掉帧 | §12.5 自动降级链；省电档强制 solid |
| 快捷键冲突面扩大 | 注册表 `when` + 冲突检测进 CI，冲突即红 |
| 取色流水线产出不可控色 | L/C 钳制 + 对比度门禁 3 轮上限，兜底回默认 accent |
| 全景图 unique 校验 | 第二期扩编独立章节独立计数，不动 F00001~F10000 |
