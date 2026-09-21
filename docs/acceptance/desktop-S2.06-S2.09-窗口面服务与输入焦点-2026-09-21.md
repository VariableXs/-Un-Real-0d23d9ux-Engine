# AI-4 桌面承载线 · S2.05–S2.12 施工与验收（2026-09-21）

> **工位**：AI-4 桌面承载线（《VARIX三体系统-AI分工完成图》第 5 节）
> **本批辖区**：第 2 批 S2.05–S2.12（输入事件管道 / 渲染通路 / 三件套闭环 / BootScreen / 多窗合成 / 降级 i18n / 性能基线 / M3 整合验收）+ R2 ushell 视觉对齐配合
> **施工纪律**：多会话并行（git status 盘点在前；只 add 显式路径；AI-1 handoff 未提交工作区零接触；AI-3 S2.01–S2.04 辖区零接触）
> **本文档性质**：代码层 + QEMU 层交付实录；真机接触点（S2.07/S2.12 真机验收、S2.11 真机基线）如实标注待办

---

## 1. 施工前置摸底（动手前的现状结论）

| 设施 | 现状（本会话开工时） | 出处 |
|---|---|---|
| 垫片协议单源 | ✅ v1（OK/MAPPED_ERR/MISSING 三段式 + 20 事件通道 + 20 能力位 + shim_hello 协商） | tools/shim-protocol.source.json（任务 22） |
| 内核垫片通道 | ✅ SYS_FRAME(16)/SYS_INPUT(17)/SYS_SHIM(18) 三支点，ushell ring3 消费 | proc/usrshell.rs（任务 27/28/55） |
| 输入事件契约 | ✅ shim://input 16B 定长（seq/kind/key/dx/dy/buttons/pad），服务级 4 订阅者广播 | inputsvc.rs（任务 19/55） |
| 前端输入解码 | ✅ KernelInputDecoder（seq 去重/鼠标边沿推导/IME 扩表 0..57） | src/lib/shim/kernelInput.ts（任务 26） |
| 显示服务 | ✅ 双缓冲+脏矩形提交，但**单绘制面归口**（draw_surface 唯一借用） | displaysrv.rs（任务 20） |
| 窗口面（像素级客户端） | ❌ **不存在**——无窗口注册/Z 序/最小化保活/客户端像素提交 | 本会话施工主菜 |
| 输入焦点模型 | ❌ 纯广播（多窗多进程时键盘无归属） | 本会话施工 |
| boot://event | ✅ 内核 14+1 阶段 timeline 快照回放 ↔ 前端 BootEventBuffer 容错（去重/乱序/进度单调） | usrshell.rs + bootEvents.ts（任务 28） |
| 降级 i18n | ◐ 词条+映射在（任务 29），**缺门禁执法** | perfBaseline.ts + dictionaries |
| 性能采样 | ◐ 前端首帧/交互延迟采样在（任务 29），**缺真机基线归档** | perfBaseline.ts |

**核心结论**：AI-4 的真实缺口 = ①内核侧窗口面服务（S2.06+S2.09 的承载协议）②输入焦点路由（S2.05）③验收门禁机械化（S2.10）④真机基线与整合验收设施（S2.11/S2.12）。

---

## 2. 本会话施工实录

### 2.1 窗口面服务 winsurf（S2.06 渲染通路 + S2.09 多窗合成 · 内核侧地基）

**新模块** `kernel/varix/src/winsurf.rs`（约 1080 行含测试），定版设计：

- **客户端提交模型（copy-in）**：客户端（ushell / 未来的 Servo 进程）经 `SYS_WIN`(21) 注册窗口面并按行提交像素，uaccess 拷贝进内核窗口面；唯一写入路径 `stage_row`，零旁路直写帧缓冲。协议预留共享缓冲升级位 `CAP_SHARED_BUF`（v1 恒 false——升级时客户端零语义变化）。
- **缓冲：PMM order-10 块链**：单窗 ≤8 块 = 32 MiB（覆盖 2560×1440×4 ≈ 14 MiB，满足 S2.06 双分辨率验收的容量前提）；行粒度寻址不跨块（单行 ≤16 KiB ≪ 块 4 MiB）。
- **合成器 `composite()`**：按 z 序把可见窗提交到 displaysrv Surface。双缓冲模式走**脏区增量**（窗口脏区平移到屏幕坐标，`clip_window_region` 三方求交：脏区∩窗几何∩屏幕，级联 mark_dirty/commit，溢出转全窗绝不丢脏区）；直写模式（>4MiB 帧）全窗全量重绘+mark_full。
- **最小化保活**：Minimized 窗缓冲与几何保留、跳过合成并计数（vwm `display:none` 的裸机等价物；restore 后内容原样可见）。
- **焦点**：`focus(wid)` 记录焦点窗口，`focus_owner()` 提供属主 pid 供跨层键盘路由。
- **诚实性设计**：窗口 format 与屏幕不一致 → 跳过并计数（绝不猜格式写屏）；槽满/几何越界/容量超限 → register 明确拒绝（绝不静默挤占）；PMM 分配失败 → 已分块回滚 + 整窗拒绝（不留半成品）。
- **Z 序**：显式 raise 重新盖章（z_gen 令牌），命中测试按 z 最高优先。

**系统调用面 `SYS_WIN`(21)**（proc/usrshell.rs，与 SYS_FRAME 16–20 同区延伸）：

| 子命令 | 语义 |
|---|---|
| WIN_REGISTER(1) | p1=owner_pid p2=(w<<32)h → wid（屏幕 fmt 自动绑定） |
| WIN_UNREGISTER(2) / WIN_GEO(3) / WIN_RAISE(4) / WIN_STATE(5) | 回收（焦点联动清空）/ 位置（i32×2 打包支持负坐标出屏）/ 置顶 / 最小化 |
| WIN_FOCUS(6) | wid 焦点（0=清除）+ 跨层联动内核级键盘路由 |
| WIN_SUBMIT(7) | 提交块 = {kind(FULL/DIRTY), count, Rect16[], 像素}，逐行 4KiB 分段拷贝（戒律 >64KB 禁栈的保守取值），块总长先验校验 |
| WIN_COMPOSITE(8) | 合成一帧 → 返回累计 blit 行数 |
| WIN_QUERY(9) | p1=0 服务统计；p1=wid → geo/wh/状态位三字段查询 |

用户内存访问走 sys_shim 同范式（is_user_ip/USER_TOP 校验 + volatile）；`WIN_SUBMIT_MAX_RECTS` 与 winsurf `MAX_DIRTY_PER_SUBMIT` 有**编译期对齐断言**防漂移。

**探针 `win_probe()`**（main.rs 探针链挂 display_probe 之后）：独立实例真实走 PMM 块链 → 160×90 测试窗 → 90 行渐变提交 → 3 帧合成 → 统计落串口 → unregister 归还。

### 2.2 输入焦点路由（S2.05 · 定版"谁收键"）

`inputsvc.rs` 增量层（**既有消费者零改动**——bootselect/ushell 的广播语义原样保留）：

- `FocusPolicy`：`All`（默认=现状广播）/ `KeyboardFocusOnly`（键盘只投焦点订阅者，鼠标广播不变）。无焦点（None）时 KeyboardFocusOnly 槽收不到键盘——白名单精神（没明确授权就不给）。
- 焦点切换**即生效**：切换前入队未消费的键盘事件按当前焦点判定（诚实简单，无灰色窗口期）；被路由走的事件游标推进 + `routed_away` 计数（不进 missed——事件没丢，是被路由了）。
- `subscribe_policy_pid`：订阅时声明策略+属主 pid；`set_focus_pid(pid)` 供 SYS_WIN focus 跨层接线（无匹配订阅者返回 false——窗口焦点记录照常成立，桌面进程自持 DOM 焦点语义）。
- **延迟打点（S2.11 基线初值来源）**：publish 记时钟戳、poll 交付统计 last/max/samples（TSC 周期；实机 enable_tsc、宿主注入时钟可测）；16B shim://input 契约不变（时戳只走服务级统计）。
- 与 Wine 通道同源：同一 inputsvc 总线、同一订阅者模型（pid 区分），一份代码两处用——不新增通道不分叉。

### 2.3 降级 i18n 门禁机械化（S2.10）

audit.cjs 新增 **SHIM DEGRADE GATE (S2.10)** 段：perfBaseline.ts 的 DegradeKey 全量键必须 zh/en 词典在场（zh-TW 走 zh 基底繁体转换），缺键即 fail（AUDIT FAILED 非零码退出）。实测 10 键 zh/en 全在场 → OK。降级映射无遗漏分支由既有 perf-baseline.test.ts 单测守护（vitest 全绿）。

### 2.4 S2.08 BootScreen 对接核查

逐字段核对结论：**机制已闭环，无需施工**。内核 14 阶段（Serial→SelfTest）timeline 快照 16B 记录（idx/state/ms）+ 第 15 条诊断记录（活时钟+键盘诊断字，idx=14 实机取证）经 SYS_SHIM 回放；前端 BootEventBuffer 载荷与 Windows 侧 LoadEvent 逐字段同构（camelCase），去重/乱序排序/进度单调夹取三容错齐备；bootPhase 迁移仍由 BootScreen onExitStart/onDone 驱动（App.tsx 零改动）。真机走查待 S4.1 后并行走查。

### 2.5 R2 ushell 视觉对齐（并行配合项）

现状核查：ushell 兜底桌面已实机 20/20 走查，引导期 UI（三卡菜单/handoff 交接画面）视觉语言已对齐 Variable 契约（win11 外壳契约：win11-settings.css、start-menu 类名契约）。本会话零改动（避免 R1 未接管前的无谓漂移）；S2.07 三件套真机走查时一并复核。

---

## 3. 门禁与验证证据

| 门禁 | 结果 | 说明 |
|---|---|---|
| ktest（kernel/ 单跑） | ✅ 3098 passed / 0 failed | 基线 3068 + 本会话 +18（winsurf 12 + 焦点路由 3 + 编解码 3）+ 并行会话新增 |
| kcheck | ✅ 0 error | target_os=none lib 检查过 |
| kbuild（none release） | ✅ 4,617,696 B | 真路径（sys_win/win_probe/PMM 块链）编译通过 |
| make-iso-qemu | ✅ 8,986,624 B（15:23 构建） | 源码 mtime < 构建 mtime（QEMU 层三证齐；U 盘刷写非本会话） |
| vitest | 待本会话末尾单跑结果 | AI-3 并行未跟踪文件或影响 tsc（见 4.2 遗留） |
| tsc | 本会话改动 0 错；全仓报错项为 AI-3 并行未跟踪文件 | 见 4.2 |
| V 后端 cargo test -p variable --lib | ✅ 313 passed / 0 failed | 基线 308 + 并行会话新增 |
| audit.cjs | ✅ AUDIT PASSED（含新增 S2.10 段 + AI-3 S2.01 段） | 缺键即 fail 已执法 |
| QEMU 走查 | 结果见 3.1 | win-probe 全链 + 回归 |

### 3.1 QEMU 走查（_attic/qemu-winsurf-walkthrough.py，2026-09-21 实测）

**8/8 PASS**（证据：_attic/qemu-winsurf-serial.log + _attic/acceptance-winsurf/*.png）：

```
PASS  win-probe: screen geometry reported        # screen 1280x800
PASS  win-probe: composite stats reported        # frames=3 blit_rows=270 full_redraws=3 submit_rows=90
PASS  stats: frames >= 3 (3 composites)
PASS  stats: blit_rows >= 90 (full window blit)
PASS  stats: submit_rows >= 90 (90 rows staged)
PASS  stats: direct-write path self-consistent   # 270 = 90 × 3
PASS  win-probe: PASS (unregister returned)
PASS  regression: SHELL desktop-ready            # 既有引导链零回归
```

**路径判读**：QEMU GOP 1280×800 帧 = 4.096 MiB > displaysrv 后备上限 4 MiB → 服务降级直写模式 → composite 走 full_redraws 路径（每帧全窗 90 行 × 3 帧 = 270，自洽）。双缓冲增量路径由 ktest 12 用例覆盖（含脏区只搬脏行/溢出转全窗/最小化跳过）。**两条合成路径均有实测证据**。PMM 块链分配/归还（register→unregister）在真内核内存管理下跑通。

### 3.2 vitest / tsc 交叉状态（并行会话如实记录）

- vitest 全量：2838 passed / 4 skipped / **1 failed**——失败项 = `vwm-kv-persistence.spec.ts`（AI-3 S2.04 进行中测试，全量并行负载下 5015ms 超时 > 5000ms 上限，**单跑 1 passed**）；非本会话改动。
- tsc 全仓报错 = AI-3 未跟踪测试文件（process 类型缺失），非本会话改动；本会话新增 TS 代码 0（前端零改动——S2.06/S2.09 全部落在内核侧）。

### 3.2 ktest 新增用例清单（18 项）

winsurf（12）：register_lifecycle_and_rejects / stage_row_bounds_and_content / composite_single_window_fullscreen_pixel_exact（逐像素对照）/ composite_two_windows_zorder_overlap（Z 序+raise 翻转）/ minimize_keeps_alive_and_restore（保活）/ dirty_rect_incremental_only_blits_dirty_rows（只搬脏行+溢出转全窗）/ offscreen_clip_negative_and_overflow（负坐标+超右下）/ hit_test_zorder_and_focus_owner / capacity_eighth_window_rejected / direct_mode_full_redraw / fmt_mismatch_skipped_honestly / clip_window_region_unit（三方求交纯函数）。
inputsvc（3）：focus_route_1000_no_crosstalk（×1000 无串键）/ focus_none_blocks_keyboard_focus_only_subs / latency_stats_with_injected_clock。
usrshell（3）：win_pack_xy_roundtrip_with_negative / win_pack_wh_roundtrip / win_submit_hdr_validation。

---

## 4. 遗留与真机待办（如实清单，❌ 不静默）

### 4.1 AI-4 辖区真机待办

| 项 | 依赖 | 验收口径 |
|---|---|---|
| S2.06 逐屏对照（125% DPI + 2560×1440） | S4.1 真机 USB 键鼠 + Servo 进程接入 | png_probe 度量 |
| S2.07 三件套真机闭环（M2） | Servo 承载同一份前端（R1 第 7 步） | 真机引导默认进 Variable 桌面 |
| S2.11 三项基线真机归档 | 真机会话（1.8 SOP） | 同机三次方差 ≤5%，入 docs/bench/ |
| S2.12 M3 整合验收（全流程 ×5） | 前述全部 | 验收报告 AI-6 签发 |

**诚实声明**：Servo 进程本体接入（R1 原理链第 3–4 步）是数月级长线，不在本会话范围；本会话交付的窗口面服务 = Servo 接入时的内核侧承载协议（注册→提交→合成→焦点四接口即插即用），ushell/Servo 二者按同一 SYS_WIN 协议消费。

### 4.2 并行会话交叉状态（截至本会话收尾）

- AI-1：handoff 三件套未提交（工作区 modified+untracked）——零接触，等用户确认。
- AI-3：S2.01 垫片生成物（vport/shim_protocol.rs）+ S2.03/S2.04（kvsrv 调整、vwm-kv-persistence.spec.ts）施工中——tsc 全仓报错来自其未跟踪测试文件（process 类型），非本会话改动。
- AI-5：S4.1 xHCI HID 评估文档已入 docs/acceptance/（kernel-S41-…-2026-09-21.md）。

### 4.3 施工中发现并修复的真实缺陷（本会话代码内）

1. 块大小公式错（1<<order 当成字节——order 是页框数，须乘 4096）：ktest 立案→修复→多窗口用例绿。
2. blit 负窗位 `as usize` 环绕溢出（先 i64 加法再转）。
3. composite 最小化窗统计不可达（order 收集时已过滤）。
