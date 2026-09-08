# SUMMIT-90 — 「极顶计划」实施步骤

> 系列：NEXT-40 → ASCENT-60 → APEX-70 → **SUMMIT-90**
> 配套文档：《SUMMIT-90-功能全景.md》（功能定义）、《SUMMIT-90-AI分工图.md》（责任归属）
> 施工总则见第 0 章；三波次交付节奏见第 1 章；逐项步骤见第 2~11 章；验收与收口见第 12 章。

---

## 0. 施工总则（每个 AI 每一项都必须遵守）

### 0.1 五条铁律的工程化落点

| 铁律 | 工程化检查点 |
|---|---|
| 零退化 | 每项功能默认关闭或默认值=现状；改动前后 `tools/visual-audit.cjs` 基线 diff ≤0.5%；新设置项默认值写在 DEFAULT_SETTINGS 并过 sanitize 测试 |
| 零键位冲突 | 新键位只允许 `ctrl+alt+` 空闲槽（用 `rg "ctrl\+alt\+" src/lib/shortcuts.ts` 先查占用）；Z-08 注册表落地后一律改走 `keymap/register()` |
| 四应用隔离 | 改动前 `git status` 确认不触碰 `src/apps/write|mind|code|fate`、`src/features/editor|mindmap|project|fate*`；PR 检查清单勾选「四应用零修改」 |
| 不弄巧成拙 | 新动效必须走既有 `--dur-*`/`--ease-*` 令牌与 reduce-motion 降级；新增 UI 文案三语齐全（audit.cjs 门禁） |
| 如实降级 | 任何系统 API 失败路径都有诚实文案与日志，禁止静默吞错 |

### 0.2 共享文件协议（沿用项目既有纪律，冲突重灾区）

- **共享文件清单**：`src/i18n/dictionaries.ts`、`src/lib/ipc.ts`、`src-tauri/src/lib.rs`、`src/system/windows/vwm.ts`、`src/system/desktop/StartMenu.tsx`、`desktop.css`/`tools.css`
- **改前必核**：`rg` 验证当前状态（他人可能已改），最小化 diff，只加不改他人段落
- **改后必验**：`npm run typecheck` + `node tools/audit.cjs` + 涉及后端时 `cargo check`
- **i18n 特别纪律**：zh 块曾被并发回滚——每次提交前 `rg "新键名" src/i18n/dictionaries.ts` 双侧（zh/en）复核
- **PS1 脚本纪律**：Edit 工具会剥 UTF-8 BOM，每次编辑 .ps1 后必须重写 BOM

### 0.3 每项功能的通用 DoD（完成定义）

1. 代码落地 + 单测（vitest / cargo test）覆盖核心路径与失败路径
2. `npm run typecheck` 0 错、`node tools/audit.cjs` AUDIT PASSED、`cargo check`（涉后端）0 错
3. 新设置项：DEFAULT_SETTINGS + sanitize + 设置页 UI + i18n 三语
4. 新键位：注册表/快捷键表登记 + 冲突检测通过 + 帮助文案
5. 默认行为与改动前一致（或默认关闭），验收清单打勾（未实测不勾，诚实口径）
6. docs/acceptance/ 下对应验收记录更新

### 0.4 测试命令速查

```powershell
npm run typecheck                # 前端类型
node tools/audit.cjs             # i18n/IPC 命令面审计
npx vitest run                   # 前端单测
cargo check                      # 后端编译
cargo test --no-run              # 测试可编译性（cargo check 绿 ≠ 测试可编译）
cargo test                       # 后端单测
node tools/visual-audit.cjs      # 视觉回归（puppeteer 可选，缺省诚实退出 2）
node tools/bench --no-gui        # 性能基准（先看 docs/bench 当日归档防覆盖）
```

---

## 1. 三波次交付节奏

| 波次 | 主题 | 项数 | 前置 |
|---|---|---|---|
| 第一波（P0） | 立刻可做、无依赖、痛点直击 | 12 项 | 无 |
| 第二波（P1） | 体验增量明显、依赖既有基础设施 | 36 项 | P0 完成 + APEX-70 基础项（Z-08 注册表建议优先） |
| 第三波（P2） | 依赖前序 N/Z/U 系列交付的生态与基建 | 42 项 | 各自前置（见全景附 A） |

> 波次内各域可并行（分属不同 AI 路）；波次间只有软依赖——P1 里不依赖 Z 系列的项可与 P0 并行。

---

## 2. 域 W 施工步骤（M-01…M-09，AI-2 窗口路）

### M-01 摇一摇最小化
1. `src/system/windows/VirtualWindowFrame.tsx` 拖拽路径：pointermove 采样横坐标，计算换向次数与速度峰值（窗口 ≥2 次换向且幅度 >60px 触发）
2. 触发调用 vwm 状态机已有的 minimizeAll 动作（复用 Ctrl+Alt+M 路径）
3. `settings.winShake`（默认 false）+ sanitize + 设置页开关 + i18n `winShakeLabel` 三语
4. 单测：合成拖拽事件序列（正常拖动 100 次 / 摇动 10 次）断言触发率
5. 验收：贴靠拖拽路径零误触（贴靠手势本身有横移，需在「已进入贴靠预览」状态禁用检测）

### M-02 窗口卷帘
1. vwm.ts 窗口状态新增 `rolledUp: boolean`；VirtualWindowFrame 高度动画到 `--titlebar-h`（复用 snapping 动效令牌）
2. 内容容器 `display:none`（与最小化同一保活策略，不卸载 React 树）
3. 标题栏右键菜单加「卷起/展开」；双击热区可选项（默认关）
4. 任务栏图标角标组件复用 IM 徽标样式；贴靠/最大化时自动展开
5. 嵌入窗口（isEmbed）菜单项置灰 + `embedNotSupported` 文案；单测卷起→展开内容状态不丢

### M-03 最小化抽屉
1. 新组件 `src/system/taskbar/MinimizedDrawer.tsx`；数据源 = vwm 状态中 `minimizedAt` 时间戳（vwm.ts 窗口对象加字段）
2. 任务栏抽屉图标（徽标 = 计数；空时 `return null` 不占位）
3. 键位 `ctrl+alt+\``（反引号，查 shortcuts.ts 确认空闲）注册进 SHORTCUT_ACTIONS + winman.rs default_binds 同步
4. 键盘导航（↑↓/Enter/Esc）复用 WintabSwitcher 交互模式；i18n `drawerEmpty/drawerTitle`
5. 单测：20 窗口最小化排序、还原后计数一致性

### M-04 窗口体检
1. `winman.rs` 新增 `health_check` 轮询（3s，仅已登记 hwnd，`IsHungAppWindow`）；结果经事件 `health://window` 推前端
2. VirtualWindowFrame 标题栏琥珀徽标（`--warn` 令牌）+ 悬停说明文案（诚实口径：应用自身问题）
3. 「结束进程」走 ConfirmHost 模态（明确数据丢失警告）→ 既有 taskkill 通道
4. 事件频率走 M-50 削峰（本项可与 M-50 并行，先用简单节流）
5. cargo test：状态机 hung→normal→hung 转移用例

### M-05 跨屏摆渡
1. 拖拽悬停边缘 600ms 计时器 → 目标屏 SnapPreviewHost 半透明预览（复用贴靠预览组件，换锚定屏）
2. 松手：目标屏坐标换算（QueryDisplayConfig 的 monitor rect + DPI），260ms 位移动画（transform 过渡，不重排）
3. 键盘版 `ctrl+alt+shift+left/right` 进快捷键表（前后端双侧）
4. 混合 DPI 单测：100%→150% 屏几何换算比例正确（纯函数抽取）
5. 验收：双屏 20 次摆渡零错位、零黑帧

### M-06 窗口挂起
1. `embed.rs` 会话扩展 `suspend()/resume()`（`NtSuspendProcess`/`NtResumeProcess`，feature gate）
2. 环境退出钩子（lib.rs on_exit 附近）：枚举挂起会话全部恢复——**带着挂起退出是红线事故**
3. 前端右键菜单 + 冰蓝徽标（`--accent` 变体）；任务栏角标
4. 只对 embed 登记进程开放；四空间/文件管理器等环境内窗口菜单项不显示
5. cargo test：挂起→恢复→挂起循环句柄不泄漏；退出恢复路径用例

### M-07 对齐参考线
1. 拖拽 move 时收集其他可见窗口 rect + 屏幕中线/三分线；距离 <8px 画 1px 参考线（绝对定位 div，`--accent` 色）
2. 吸附 = 坐标取整到参考线；Alt 按住时 `guidesEnabled=false`（本帧）
3. `settings.snapGuides`（默认 false）；i18n 开关文案
4. 单测：纯函数 `computeGuide(candidates, moving)` 距离/吸附用例
5. 验收：三窗口 1/3 排列几何误差 ≤1px；reduce-motion 不影响（效率功能）

### M-08 精炼 Alt+Tab
1. WintabSwitcher 数据层加过滤谓词（byMonitor / byApp）；过滤器状态进 settings
2. `ctrl+alt+tab` 打开带过滤的切换器；`alt+tab` 默认路径零改动
3. 空集合 → toast「当前过滤器下无其他窗口」+ 回退一次全局轮转（诚实降级）
4. i18n 三语；单测过滤谓词
5. 验收：双屏各 5 窗口集合正确

### M-09 悬停聚焦
1. VirtualWindowManager pointermove 节流（300ms 停留判定）
2. 三档设置：off（默认）/focusOnly/focusAndRaise；仅聚焦档只改焦点态不改 Z 序
3. 暂停条件：全屏应用、拖拽中、模态打开、kbdhook 环境隐藏
4. 设置项带诚实提示文案；单测暂停条件矩阵

---

## 3. 域 T 施工步骤（M-10…M-18，AI-2 / AI-5）

### M-10 跳转列表
1. 数据层 `src/system/startmenu/jumplist.ts`：recent.ts 记录 + launcher 登记表按 appKey 聚合（最近文件 top7）
2. 组件 `JumpList.tsx`：固定图标右键 → 二级浮层（最近文件 / 常用动作 / 最近实例窗口）
3. pin/unpin 持久化 settings KV（`jumpPins.{appKey}`）
4. 嵌入应用列出活会话窗口（点击 → embed focus 通道）
5. 单测聚合逻辑；i18n `jlRecent/jlPinned/jlNewWindow`

### M-11 托盘收纳抽屉
1. `tray.rs` 托盘枚举已有 → 新增溢出阈值事件；前端 `TrayDrawer.tsx`
2. 拼音搜索复用 `pinyin.ts`（`searchTray(q)`）
3. 置顶常驻 + 拖动排序持久化；新图标徽标 +1 点开清零
4. 文档明示「只读取镜像、不修改系统托盘」；i18n 三语
5. 验收：15 测试图标搜索三种输入命中

### M-12 时钟多时区
1. 设置 `clockZones: string[]`（IANA，≤3）+ sanitize
2. 时钟悬停卡增量渲染（多时区行 + ISO 周数 + 今日计数只读）
3. `Intl.DateTimeFormat` 按时区渲染（夏令时自动正确）；周数纯函数 + 单测（年初/年末/闰年边界）
4. i18n 三语；验收三城市夏令时切换日

### M-13 等待态规范
1. launcher 启动事件（pid 已有）→ 任务栏占位条目（.skeleton shimmer 既有样式）
2. 占位点击 = 聚焦/无操作（**绝不重复 ShellExecute**，800ms 防抖锁）
3. 8s 超时状态切换文案 + 点击显示详情（pid/耗时）
4. 单测防抖锁；验收 10GB 级 IDE 冷启动

### M-14 IM 未读聚合
1. imwatch.rs 事件已推 → 前端聚合 store（Σ 计数，99+ 封顶）
2. 任务栏总徽标（复用徽标组件）+ 悬停明细（逐 IM 计数，点击聚焦）
3. IM 进程退出 → 计数清零事件；i18n 三语
4. 验收：双 IM 并发未读一致

### M-15 任务栏空区菜单定制
1. 菜单项注册表 `src/system/desktop/taskbarMenu.ts`（id/i18n/action/默认显隐）
2. 菜单渲染改读注册表 + settings 覆盖（顺序+显隐）
3. 设置页「任务栏」区菜单编辑器（拖动排序复用 M-31 的托架交互或简化为 ↑↓ 按钮）
4. 「恢复默认」一键；默认状态与现状逐像素一致（视觉回归验证）

### M-16 媒体呼吸
1. MediaControl 播放态 → 时钟旁媒体图标 CSS class `breathing`
2. `@keyframes breathe`（scale 1→1.02，4s，ease-in-out infinite）；`[data-reduce-motion="true"]` 下禁用
3. `settings.mediaBreath`（默认 false）；幅度周期写死
4. 视觉回归：开启/关闭/降级三态基线

### M-17 便签速贴
1. `ctrl+alt+s` 快捷键（查空闲）→ 单行输入浮层（桌面 shell 层，PromptHost 变体）
2. 便签组件 `StickyNote.tsx`：四角堆叠、拖动、10 分钟淡隐（500ms 动画）、点击钉住
3. 数据 settings KV（`stickies: {id,text,pinned,corner}[]`，≤20 条上限）；环境重启恢复 pinned 项
4. 「一键清除全部」走确认模态；i18n 三语
5. 单测：淡隐计时、钉住持久化、上限拒绝

### M-18 音量滚轮规范
1. 前端 keydown 捕获 `VK_VOLUME_*`（不 preventDefault，纯监听）
2. 音量浮标（QuickPanel 同款样式复用）1s 淡出；与逐应用音量档位联动显示
3. DND 下照常显示（音量非打扰）；i18n
4. 验收：三种设备（键盘滚轮/多媒体键/耳机线控）走查

---

## 4. 域 F 施工步骤（M-19…M-27，AI-1 / AI-3）

### M-19 右键菜单编辑器
1. `src/components/ContextMenu.tsx` 项抽取注册表（id/i18n/danger 标记/分组）
2. 设置页「文件管理器」区编辑器（显隐勾选 + ↑↓ 排序 + 分组分隔线开关）
3. 持久化 settings `ctxMenuOrder`；「恢复默认」
4. 单测：注册表完整性（全部动作有 i18n 与处理器）

### M-20 同名选择记忆
1. explorer 批量操作的冲突对话框（已有或补齐）加「对剩余全部应用」勾选
2. 会话级传播（批量上下文对象持有选择，不进 settings）
3. 批量结束 toast 汇总（替换/跳过/保留两者计数）；保留两者重命名预览
4. 单测：47 冲突的 200 文件批量弹窗 1 次；Esc 中断不产生半拷（事务路径回归）

### M-21 校验和工具
1. 后端新 command `checksum(path, algo)`：分块流式读取（1MB 块），进度事件 `checksum://progress`，可取消（CancelToken 模式复用 bench 先例）
2. BLAKE3 复用容器依赖；MD5/SHA 由 `md-5`/`sha2` crate
3. 前端浮窗：算法四选、进度条、期望值比对（绿勾/红叉高亮）、复制按钮
4. ipc.ts 登记 + audit.cjs 通过；cargo test 对照 certutil 已知向量
5. 验收：1GB 文件内存峰值 <80MB、取消零残留

### M-22 空格快速预览
1. explorer 列表聚焦态 keydown 空格 → `QuickLook.tsx` 居中浮窗（非模态，Esc/空格关闭）
2. 渲染层复用既有预览组件；格式白名单表（支持/降级两态，降级显示图标+类型说明）
3. ←/→ 相邻文件、↑/↓ 层级；音视频显示控制条
4. 单测白名单矩阵；i18n `qlUnsupported`
5. 验收：20 格式走查 + 键盘闭环

### M-23 压缩包目录浏览
1. 后端 `archive_ls(path)` / `archive_extract_one(path, inner)`：`zip` crate 只读；7z/tar 视依赖体积决定 v1 范围（zip 优先，7z 如实「暂不支持」）
2. explorer 路径解析层：`xxx.zip` 可进入（虚拟路径前缀 `zip://`），列表/排序复用
3. 双击 → 缓存目录解压 → 系统打开 → 关闭后清理（temp 管理复用 sysmaint 临时区）
4. 包内文件名搜索（前端过滤即可）；损坏包错误走 error.rs 归一化文案
5. cargo test：zip 构造/读取/单文件解出/损坏路径

### M-24 目录置顶书签条
1. settings `placePins: string[]`（≤12）+ 访问计数 `placeStats`
2. 地址栏旁书签条组件（图标+名称，LRU 排序，满 12 淘汰提示）
3. 拖拽文件夹到书签条（drop 目标）即置顶；右键菜单（新标签/系统打开/取消）
4. 单测 LRU 淘汰；i18n 三语

### M-25 文件锁定侦探
1. 后端 `who_locks(path)`：Restart Manager API（RmStartSession/RmRegisterResources/RmGetList）
2. 删除/移动失败错误码映射 → 前端失败 toast 加「查看占用」
3. 侦探浮窗：进程名/pid/窗口标题列表 + 「在任务管理器中查看」（跳转 TaskManApp 并定位 pid）+ 刷新
4. 查不到 → 诚实文案「系统未披露占用者」；**无强拆按钮**（红线）
5. cargo test：本进程打开文件的自锁用例（锁定者=自己，可断言）

### M-26 环境回收站安全网
1. 设置 `deleteMode: system|env|ask`（默认 system = 现状）
2. env 模式：删除走环境回收站目录（便携模式跟随数据卷，usb.rs 拔出保护联动）
3. 清空前确认模态显示最近 10 项可恢复清单
4. 单测三档行为；验收便携拔盘场景

### M-27 目录监控哨兵
1. 后端 `sentinel.rs`：ReadDirectoryChangesW 每哨兵一线程（上限 5），事件节流合并 1s 窗口
2. settings `sentinels: {path,enabled,quietHours}[]`；通知中心推送（复用 notifyStore，含声音开关）
3. 环境隐藏时照常记录（通知入中心不弹横幅）
4. SMB/网络路径：入口校验拒绝并提示「暂不支持网络目录」
5. cargo test：临时目录注入增删改名事件合并用例

---

## 5. 域 K 施工步骤（M-28…M-36，AI-2）

### M-28 键位使用统计
1. winman.rs dispatch_action 处计数（内存 HashMap + 每 5 分钟落盘 settings KV `keyStats`）
2. 快捷键中心（N-17 落地后）列表加使用次数列；未落地则先出设置页只读榜
3. 注册失败（降级）单独计数成「占用榜」+ 空闲槽换绑建议（复用 M-36 的建议函数）
4. 隐私口径注释：只记 action id + 次数，无内容；单测计数与落盘

### M-29 长按加速曲线
1. VWM 键盘移动循环处：keydown 持续时间 → 重复间隔查表（500ms 前 1x，之后 1.5x/2.5x/4x 三段）
2. 只挂接「窗口移动/网格导航」两类；曲线常数 const 化
3. 单测：合成长按 2s 的移动步数 ≈ 12 次单击 ±15%

### M-30 侧键可编程
1. kbdhook 轮询路径识别 XBUTTON1/2（GetAsyncKeyState 扩展键位）
2. 映射表 settings `xBinds: {xbutton1,xbutton2}` 默认 = back/forward（零退化）
3. 全屏应用检测（复用既有避让逻辑）→ 让位回系统语义
4. 映射动作复用 SHORTCUT_ACTIONS 分发；单测默认语义

### M-31 启动槽可视化分配
1. 设置页快捷键区「启动槽托架」：9 槽组件（虚线空槽 + 图标实槽 + 键位标签）
2. 从开始菜单/桌面拖拽（HTML5 DnD，payload 携带 appKey）落槽即写 `shortcutBinds.launchN` 的应用映射（注：launch 槽的应用归属存 settings `launchApps`）
3. 应用被删 → 槽位清空 + toast；同应用多槽允许但提示
4. 单测绑定写回 effectiveBinds 一致性

### M-32 每窗口 IME 状态
1. 后端 `ime_state.rs`：ImmGetDefaultIMEWnd/ImmGetConversionStatus 查询 + WM_IME 设置（不可用 API → 事件「unsupported」，前端零动作）
2. VWM 焦点切换钩子：切换前记录旧窗口状态、切换后恢复新窗口记忆态
3. 托盘状态条小图标（中/英）；四空间窗口同样适用（只读系统状态，不改其代码）
4. 单测：状态机记录/恢复；降级路径（API 不可用）零副作用
5. 验收：终端↔聊天 50 次正确率 ≥95%

### M-33 按键回显
1. winman.rs dispatch 处事件 `keycast://show`（仅 `settings.keycast` 开启时发）
2. 前端角落浮层（修饰键组合 + i18n 功能名），2s 淡出，四角可选
3. **只回显功能组合**（dispatch 表内的），打字内容永不出现（红线注释）
4. 单测：表外按键零事件

### M-34 Esc 层级规范
1. uiStore 加「浮层栈」`overlayStack: OverlayId[]`（push/pop API）；12 类浮层（模态/右键菜单/各面板/搜索框）逐一走查入栈
2. 全局 Esc handler：栈顶消费一层；kbdhook 双击 Esc 判定保持独立（30ms 轮询，不受影响——需在 kbdhook 层加「环境内有浮层时双击 Esc 只关浮层」的白名单逻辑，谨慎回归）
3. 层级表文档进帮助中心（Z-70 落地后）；单测栈序
4. 回归重点：双击 Esc 切环境的手感不变（验收专列）

### M-35 滚轮语义规范
1. 盘点壳层 15 个滚动容器（explorer/开始菜单/通知/设置/任务栏…）逐一核对 Shift=横滚、Ctrl=缩放（支持处）
2. 任务栏滚轮音量（`settings.wheelVolume` 默认 false）+ M-18 浮标联动
3. 行为表写入 docs/INTERACTION.md（新文件，10 行级小文档）
4. 四大空间内部不在范围（隔离红线，文档注明）

### M-36 键位变更预览
1. 快捷键设置页保存前 diff：`effectiveBinds(newOverrides)` → 找被挤占 action + 系统保留键库命中标注
2. 预览卡（冲突列表 + 风险高/低）+ 「查看占用者」跳转 + 「自动挑空闲槽」（扫描 ctrl+alt+* 未占用槽建议）
3. 系统保留键库（40+，与 Z-09 清单同源共用常量）
4. 单测：diff、建议函数；i18n 三语

---

## 6. 域 C 施工步骤（M-37…M-45，AI-3 / AI-1 / AI-2）

### M-37 图标缓存自愈
1. launcher.rs 缓存条目结构加 `src_mtime + src_hash`；启动快扫失效 → 后台重建队列（并发 ≤2）
2. 重建完成事件 `icon://refresh` → 前端替换占位（占位用 .skeleton shimmer，替换走零闪策略）
3. 「全量重建」按钮进设置→维护区
4. cargo test：失效判定函数

### M-38 UWP 识别
1. launcher.rs 枚举补 GetStartApps + PackageManager（windows crate winrt feature）→ UWP 条目（图标/显示名/发布者）
2. 与 .lnk 扫描去重合并（AUMID 键）；compat 矩阵 tier=L4（让位，不可嵌入）
3. 前端嵌入按钮置灰 + 说明 tooltip
4. cargo test：合成 UWP 条目合并去重

### M-39 提权提示
1. 启动路径复用安全工作台 PE 解析（requireAdministrator manifest 标记）
2. 说明卡组件（每应用一次，settings `elevatedAck.{appKey}` 记忆）；「不再提示」勾选
3. ShellExecute runas 失败（用户拒绝 UAC）→ toast 如实回执
4. 单测 manifest 解析

### M-40 高刷自适应
1. 后端启动时 QueryDisplayConfig 主屏刷新率 → 事件推前端
2. 前端 root 元素 `--dur-scale: 0.85`（≥120Hz）写入（与 M-75 缩放乘法叠加：`calc(var(--dur-3) * var(--dur-scale) * var(--motion-scale))`）
3. `settings.hfrTiming` 默认 true；混合多屏按主屏
4. 单测：系数计算；验收 144Hz 无跳帧

### M-41 驱动共存协议
1. 内置已知驱动进程名清单（G Hub/Synapse/Armour Crate 等 10 个，const 可更新）
2. 启动进程快照匹配 → 一次性提示卡（settings `driverAck` 记忆）
3. 帮助中心收录「与驱动软件共存」条目（Z-70 落地后挂链）
4. 单测清单匹配

### M-42 实例角标
1. 任务栏聚合键 exe path；计数 → 复用徽标组件（≤3 数字 / >3 `3+`）
2. 悬停 tooltip 显示区分性窗口标题（同组窗口标题列表）
3. 单测聚合计数收敛

### M-43 便携路径自愈
1. launcher 登记结构加卷 GUID（FindFirstVolume/GetVolumeInformationByHandleW）；壁纸池/数据卷引用同步
2. 启动定位失败 → 枚举卷 GUID 匹配 → 命中批量改写登记 + toast 告知
3. 未命中走既有 `--export-rescue` 应急提示
4. cargo test：模拟盘符漂移的改写用例；usb.rs 拔出保护不误触发回归

### M-44 嵌入崩溃善后
1. embed.rs 会话状态机加 `crashed` 态（进程退出且非用户关闭）；事件 `embed://crashed`
2. 前端虚拟窗口 300ms 优雅摘除（复用关闭仪式样式，非黑框 display:none）
3. 「应用 X 已停止运行」卡片（重开按钮带原启动参数；60s 内同应用合并）
4. 取证日志（时间/pid/退出码）本地记录；cargo test 状态机转移

### M-45 热插拔稳定
1. 后端 WM_DEVICECHANGE（窗口消息钩子）→ 快捷键重注册（指数退避 ≤5 次）+ kbdhook 重挂
2. 重挂期动作队列缓存 ≤1s 后回放；toast「输入设备已重连，快捷键已恢复」（每事件一次）
3. cargo test：重注册退避；验收蓝牙键鼠休眠唤醒 30 次 100% 恢复

---

## 7. 域 S 施工步骤（M-46…M-54，AI-3 / AI-1）

### M-46 日志轮转
1. 日志 append 路径统一走 `log.rs`（若分散先收拢）：按天 + 5MB 双阈值轮转，旧份 gzip，保留 7 份/总 50MB
2. 设置→维护区显示占用 + 「立即清理」
3. cargo test：轮转触发、配额删除最旧

### M-47 迁移预检
1. settings 迁移器加 dry-run 模式（不落库返回 diff 报告：未知键/损坏值清单）
2. 自更新流程（sysmaint）升级前调用预检；不过 → 自动备份先行 + 「保守升级」建议文案
3. fixtures：5 种损坏样本单测全拦截

### M-48 DB 紧凑会话
1. 一次性 `PRAGMA auto_vacuum=incremental` 迁移（版本号升级触发）
2. sysmaint 计划任务新增 compact 任务（触发四要素：空闲 ≥15min/接电/非备份/距上次 ≥7 天）；执行前强制快照
3. incremental_vacuum 分批 + WAL checkpoint；进度走维护面板既有事件
4. cargo test：触发条件矩阵；验收 10 万条删除回收 ≥80%

### M-49 图标缓存 LRU
1. 前端图标缓存 Map → LRU 包装（last_used 更新，上限 256MB 可调 64~512）
2. 淘汰分批（rAF 每批 ≤32 张）；设置页显示占用/命中率
3. 单测：淘汰顺序、分批不阻塞主线程 >8ms

### M-50 事件削峰
1. 新 `src/lib/eventShed.ts`：三档策略（merge/latest/queue）声明式频道注册
2. 五个风暴源迁移：winman 轮询、imwatch、设置广播、硬件面板、VWM 状态
3. 单测：1000 事件/秒合成风暴，最新必达 100%、回调次数收敛
4. 迁移回归：各面板行为不变（人工走查清单）

### M-51 浸泡测试
1. bench 工具加 `--soak --hours 8` 模式：合成负载循环（开/关窗口、启停应用、设置读写、事件风暴）
2. 每小时采样内存/句柄/线程数 → 报告曲线（归档 docs/bench，先查当日归档防覆盖）
3. 泄漏阈值：8h 内存 +100MB / 句柄 +500；nightly 计划任务
4. 先观察一个月再决定是否升 CI 门禁（诚实口径写文档）

### M-52 冷启动对照
1. boot 事件流分阶段时间戳 → 快照 JSON 落盘 `docs/bench/boot-<version>.json`
2. 对照工具 `tools/boot-diff.cjs`：与上一版 diff，回归 >10% 阶段标红 + 归因建议（阶段名）
3. 发版流程（M-87）挂接；固定口径：同机空载 5 次取中位
4. 验收：注入 200ms 延迟定位到阶段

### M-53 崩溃转储
1. lib.rs 加 SetUnhandledExceptionFilter + MiniDumpWriteDump（`crashes/` 目录，保留 10 份）
2. 启动检测新 dump → 提示「可导出诊断包」（联动 Z-42 导出）
3. `tools/symbolize.ps1`（发版符号表解析主栈；注意 BOM 纪律）
4. cargo test：dump 写入路径；验收注入崩溃 5 次 100% 落盘

### M-54 资源公平调度
1. embed.rs 每应用 Job Object（JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION，三档 100/50/25%）
2. 登记信息加 `cpuQuota`；前端右键菜单 + 硬件面板标黄
3. 不做内存限额（注释红线）；单测 Job 创建/取消

---

## 8. 域 O 施工步骤（M-55…M-63，AI-4）

### M-55 路由注册公开表
1. 路由声明集中化注解（id/params schema/owner/since）；生成器 `tools/gen-routes.cjs` → `docs/ROUTES.md`
2. audit.cjs 加检查：文档路由集与代码注册集 diff 为空
3. 示例链接（variable://xxx）可复制格式

### M-56 设置自动文档
1. settings 定义处元数据集中（key/type/default/range/since/i18n 描述引用）
2. `tools/gen-settings.cjs` → `docs/SETTINGS.md`；发版流程挂接
3. audit.cjs 校验默认值与 DEFAULT_SETTINGS 一致

### M-57 本地出站桥
1. 事件白名单子集（嵌入成功/下载完成/哨兵触发等 8 个）→ 规则表 settings `webhooks`
2. 出站 POST（reqwest，超时 3s，重试 1 次）；**逐次走 netconsent 或会话白名单**
3. URL 校验：环回/内网段直接放行（仍记日志），公网段需二次确认
4. cargo test：白名单事件、URL 校验、失败重试；验收 echo server 10/10、未确认 0 出站（抓包）

### M-58 插件热重载
1. 插件运行时（N-26 落地后）加 dev 通道：目录 watch（notify crate）→ 沙箱重建
2. dev 模式插件 UI 带 DEV 角标；结构化日志到控制台
3. 坏插件隔离失败不影响环境（运行时既有隔离兜底回归）

### M-59 嵌入声明协议
1. `variable-embed.json` schema（titleMatch/minSize/multiInstance/detectWaitMs）+ 校验器
2. launcher 扫描读取，用户登记优先级高于声明（文档明示优先级规则）
3. 样例与 schema 说明进 docs/ROUTES.md 同级 `docs/EMBED-MANIFEST.md`
4. cargo test：合法/非法声明用例

### M-60 测试钩子规范
1. `docs/TEST-HOOKS.md` 命名规范（`{组件}-{语义}`）；15 条主路径组件补 data-testid
2. audit.cjs 查重复 testid；boot 完成事件作为 e2e 就绪哨兵
3. 既有 e2e 迁移到钩子选择器（迁移后零修改通过 = 验收）

### M-61 变更日志自动化
1. `tools/gen-changelog.cjs`：conventional commits 解析 → 中文模板草稿（feat/fix/docs/perf 分节）
2. 发版流程（M-87）串接；输出到 CHANGELOG.draft.md 供维护者审阅

### M-62 社区翻译格式
1. i18n 词典导出/导入 CSV（键/zh/zh-Hant/en 四列）；导入仅更新既有键 + 占位符（{n}）校验
2. CLI：`node tools/i18n-csv.cjs export|import <file>`；坏行定位行号报错
3. audit.cjs 保持一致性门禁；单测导入回环（导出→改→导入→词典断言）

### M-63 资源包安全扫描
1. `pack_scan.rs`：类型白名单（图片/CSS/JSON/字体）、zip-slip 路径穿越检查、体积阈值、可执行文件拦截
2. 导入确认模态展示扫描报告（文件数/类型分布/可疑项列表）
3. 恶意包 fixtures（exe/`../`/超大假图）cargo test 全拦截；正常包 10 个零误报

---

## 9. 域 A 施工步骤（M-64…M-72，AI-5）

### M-64 壁纸主色采样
1. 后端 `wallpaper_accent(path)`：缩至 64px → RGB→OKLCH → 聚类取前 3 主色 → 候选返回
2. 前端预览卡：三候选色对关键界面迷你预览（任务栏/按钮/开关三元素着色）
3. 应用 = 生成主题存档（走主题系统既有存档结构），绝不自动生效
4. 单测：聚类纯函数；验收 10 壁纸人工评价；HC 模式不受影响（HC 覆盖 accent）

### M-65 昼夜壁纸组
1. 壁纸池结构加「时段组」维度（morning/day/dusk/night 四组）；设置页组编辑
2. 切换调度：系统时间监听（含 M-90 日界统一事件），边界默认 6/12/18/21 可调
3. 切换动效复用 Z-68 交叉淡入技术（300ms）；视频壁纸直接替换（不强交叉，注释说明）
4. 单测边界判定；验收跨 4 边界无黑帧

### M-66 音量淡变
1. 后端音量设置路径（QuickPanel/逐应用音量动作）包 30ms 淡变（IAudioEndpointVolume SetMasterVolumeLevelScalar 分步）
2. 系统侧调节（音量键直达系统）不拦截不改写（注释红线）
3. 单测淡变步进；人工听测 20 次零爆音

### M-67 桌面纯净模式
1. `ctrl+alt+p`（查空闲）→ 三层（图标/任务栏/横幅）淡出 200ms；状态记忆进 uiStore
2. 通知中心角标移屏幕边缘小点（可感知不丢）
3. 再按恢复原状（各层独立还原）；单测状态记忆/还原
4. 验收：30 分钟视频零侵入、退出精确还原

### M-68 屏保时钟
1. 空闲检测（复用打字降级输入监听，N 分钟无输入，默认 10）
2. `ScreensaverClock.tsx`：大字号 HH:mm + 日期，暗色低亮度层；分钟级更新；任意输入 ≤100ms 退出
3. 视频壁纸播放中不触发（播放态即「活跃」）；`settings.screensaver: off|clock|black`
4. 单测空闲判定；验收 2h CPU <1%

### M-69 今日简报卡
1. 每日首启判定（与 M-90 日界事件联动，`lastBriefDate` settings）
2. 卡片：日期周数 + 昨日摘要（Z-62 统计只读，未落地则显示「本地就绪」诚实降级）+ 贴士池 30 条轮换（`briefTip{n}` i18n）
3. 8s 自动收起 + 「今日不再显示」；单测首启判定/轮换不重复

### M-70 壁纸快捷操作
1. 桌面空白右键菜单壁纸区：收藏/下一张/详细信息（分辨率/路径可复制）
2. 壁纸池 favorite 标记；壁纸工坊收藏组（N-07 落地后）或设置页分组显示
3. 菜单项走 M-19 注册表；i18n 三语

### M-71 悬停延迟面板
1. tokens.css 加 `--hover-delay-fast/std/slow`（200/400/600ms）；6 处浮层统一引用
2. 设置项 `hoverLatency: fast|std|slow` 默认 std（= 现状）
3. 视觉回归默认档与改动前一致

### M-72 氛围会话恢复
1. 退出前快照：壁纸状态/音量/DND/窗口集合（登记应用+几何；环境内窗口记路由参数）→ settings `ambientSnapshot`（可关）
2. 下次进入提示「恢复上次的样子？」（绝不自动恢复）
3. 恢复动作：逐项应用（窗口批量创建走既有 launch 通道 + 几何设置）
4. 单测快照/恢复回环；快照关闭后 `rg` 验证零残留

---

## 10. 域 U 施工步骤（M-73…M-78，AI-5）

### M-73 辅助功能桥
1. 后端读 SystemParametersInfo（粘滞/滤波键）+ 讲述人进程检测 → 启动事件/变更事件
2. 键位解析层（shortcuts.ts 消费端）支持分步修饰键序列（粘滞语义：单键序列拼装）
3. 讲述人运行 → 自动 `data-reduce-motion=true`（可覆盖）
4. 单测序列拼装；验收粘滞键逐键触发 Ctrl→Alt→E

### M-74 HC 系统跟随
1. 后端 WM_THEMECHANGED 监听 → HighContrast 状态事件
2. 前端高对比档新增「跟随系统」：系统 HC 开 → 环境 HC 主题 300ms 内切换并广播
3. 联动日志；单测联动状态机；验收三档系统 HC 来回

### M-75 动效时长缩放
1. `--motion-scale` 令牌（0.5/1）乘入全部 `--dur-*`（calc 乘法，与 M-40 hfr 系数叠加）
2. 设置项三档（0.5x/1x/交给 reduce-motion 说明）；reduce-motion 优先级最高（CSS 顺序保证）
3. 全局 20 处动效时长断言单测（编译期提取或走查清单）

### M-76 ARIA 审计
1. 15 条主路径组件补 aria-label/role/aria-keyshortcuts；状态变化 aria-live=polite
2. audit.cjs 无头检查：图标按钮（无文本子节点）必须有 aria-label（静态可查项）
3. 覆盖率报告进发版清单；NVDA 人工走查 3 条主路径
4. 注意：四大空间内部不在本轮范围（隔离），报告如实标注范围

### M-77 简繁用户词表
1. settings `s2tLexicon: Record<string,string>`（≤500 条）；s2t.ts 管线最后一级应用
2. 设置页词表编辑器（增删改查 + CSV 导入导出复用 M-62 格式）
3. 冲突词（同简异繁）保存时提示选择；单测转换增量 <5% 性能

### M-78 区域格式跟随
1. 工具函数 `src/lib/format.ts`：Intl 按系统 locale（日期/时间/数字/容量）；30 处显示点走查替换
2. 容量口径设置（二进制 MiB / 十进制 MB，默认系统口径）
3. 单测 20 边界值；验收三区域（en-US/zh-CN/de-DE）走查

---

## 11. 域 Q 施工步骤（M-79…M-87，AI-5/AI-4/AI-3/AI-1 轮值）

### M-79 错误聚合看板
1. ErrorBoundary 上报到 `errBoard.ts` 环形缓冲（500 条：组件/消息/次数/首末时间），退出落盘 JSON
2. PII 清洗（剥离用户输入片段：只保留错误消息与栈）；设置→诊断区看板 Top10 + 一键复制 markdown
3. 单测聚合；验收注入 3 类错误 100 次聚合正确

### M-80 IPC 追踪
1. ipc.ts 埋点（`import.meta.env.DEV` 编译期条件）：命令/耗时/成败 → 环形缓冲
2. `ctrl+alt+f12`（仅 dev 生效：release 下该键位不注册）打开瀑布面板（过滤/标红 >100ms）
3. release 剔除验证：构建产物 `rg "ipcTrace"` 零命中

### M-81 迁移测试基建
1. `fixtures/settings/v{n}.json` 每版一份匿名化样例；矩阵测试逐级迁移断言
2. vitest（前端 settings 迁移）+ cargo test（后端 KV 迁移）双侧
3. 流程纪律：改迁移函数必须同 PR 补「上一版→新版」用例（CONTRIBUTING 注明）

### M-82 视觉回归矩阵
1. visual-audit.cjs 基线结构升级：`{route}/{theme}/{scale}.png` 两维（dark/light/HC × 100/125/150）
2. 基线更新流程文档（PR 必须含 diff 截图）；矩阵跑批 15 路由 = 135 张
3. 阈值按主题标定（HC 对比强，阈值收紧）

### M-83 键位 CI 门禁
1. keymap-audit.cjs（Z-08 交付物）接 pre-commit hook（husky 或手动安装脚本）+ CI job
2. error 级（功能冲突）阻断、warn 级（系统键重叠）要求 PR 说明标签
3. 注入冲突提交被拦截的验收记录

### M-84 性能影响声明
1. `.github/PULL_REQUEST_TEMPLATE.md` 加必填段（三选一：无影响 / <1ms / >1ms+bench 数据）
2. bench 工具 `--snippet` 输出标准化 markdown 片段（可直贴 PR）

### M-85 依赖审计自动化
1. `tools/dep-audit.cjs`：npm audit --json + cargo audit 解析聚合 → `docs/selfcheck/dep-audit-<date>.md`
2. sysmaint 计划任务每周触发；不自动升级（红线注释：升级必须人审）
3. 注入漏洞测试依赖验证可报出（验证后移除）

### M-86 文档链接检查
1. `tools/link-check.cjs`：解析 docs/ 与 README markdown 相对链接与锚点 → 断链清单退出码 1
2. CI job；当前全库断链清零（修复属附带小改动，允许跨域提交）

### M-87 发版演练
1. `tools/release.ps1`（注意 BOM 纪律）：版本号一致性 → 全量测试 → tauri build → NSIS 产物校验 → CHANGELOG 草稿（M-61）→ ROUTES/SETTINGS 再生成（M-55/56）→ 检查清单输出
2. 空跑演练一次输出完整清单归档 docs/acceptance/
3. 版本号不一致注入被拦截的验收

---

## 12. 验收与收口

### 12.1 分波验收门

| 波次 | 验收门（全部通过才进下一波主体） |
|---|---|
| P0 完成 | 12 项全落地 + 全量测试绿 + 视觉基线零退化 + 键位零冲突（脚本证明）|
| P1 完成 | 36 项全落地 + 同上 + 三语 audit 全绿 + soak 首周报告 ≥6/7 |
| P2 完成 | 42 项全落地 + 同上 + 生态文档（ROUTES/SETTINGS/EMBED-MANIFEST）再生成一致 |

### 12.2 收口清单（全部项完成后）

1. `docs/acceptance/summit90-验收.md`：90 项逐条状态（未实测不打 ✅，沿用项目诚实口径）
2. README「二十六、已知边界与路线图」追加 SUMMIT-90 完成度总览
3. CHANGELOG 收口条目
4. 发版演练（M-87）跑通 + NSIS 产物校验
5. 键位审计、i18n 审计、视觉矩阵、soak 四份报告归档
6. 五路进度总览文档同步

### 12.3 回退预案

- 每项功能默认关闭/默认=现状 → 回退 = 关开关，不需要代码回滚
- 涉及数据结构升级（图标缓存、壁纸池、settings schema）的项：迁移函数必须带降级注释与备份先行（M-47 预检联动）
- 涉及共享文件的冲突：以 git 历史最近有效版本为准，`rg` 复核后最小化重放

### 12.4 明确不做清单（重申）

- 不修改写作/思维导图/项目分析/命运推演四应用任何代码
- 不做焚毁/覆写/诱饵类隐私功能
- 不做需要在线服务的功能（天气源、云查杀、在线字体等）
- 不做键位系统自定义外的全局键鼠监听扩张（侧键之外不碰）
- 不做「演示惊艳日常干扰」的动效（媒体呼吸是唯一例外且默认关）
