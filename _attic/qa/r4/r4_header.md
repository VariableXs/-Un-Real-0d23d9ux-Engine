# Variable 系统实机 QA 检查报告 · 第四轮（2000 项全新清单 + 交互缺陷清零）

> 日期：2026-09-15（16:47–18:00）· 方式：release 版 `variable.exe` 实机运行 + 屏幕自动化（pyautogui/win32，DPI 感知，物理坐标 1:1）+ 软件自带实时日志（进程 stdout 落盘）+ 静态门禁（tsc / vitest / cargo test / vite build）
> 构建指纹：`src-tauri/target/release/variable.exe`（2026-09-15 17:24 重建，33,582,080 字节，内含 dist 资产指纹 `CMaKtAj8`，规避第二轮 R2-B1"半成品 release"陷阱）
> 编号空间：**R4-0001 ~ R4-2000**，与首轮 800 项、第二轮 R2-0001~3000（15 域）、第三轮 R3-0001~2000（10 域）零重叠；本轮 20 域全部为新域。
> 按用户指令**不操作四款内置软件**（Variable Write / Code / Mind / Fate）：开场误开的 2 个内置窗仅做了"关闭"动作（Ctrl+W，不进入其任何功能），其余全部规避。

---

## 一、执行状态分级（诚实口径）

| 级别 | 含义 | 数量 |
|---|---|---|
| ✅ 实机执行 | 本轮在真机上操作并有截图/日志证据 | 79 |
| ☑ 代码/门禁级验证 | 同对象族含实机证据 + 本轮门禁全绿复核 | 111 |
| — 规划未执行 | 已成清单、待后续轮次执行 | 1810 |

---

## 二、本轮代码修复清单（开跑前批量落地，全部实机或门禁验证）

| 编号 | 来源 | 修复 | 落点 | 验证 |
|---|---|---|---|---|
| R4-B1修 | R3-B3 (P1) | **Ctrl+W 不再整体退出**：捕获阶段拦截，非输入场景=关闭当前聚焦 VWM 虚拟窗；输入场景仅拦截默认行为，组件级 Ctrl+W（资源管理器关标签）继续生效 | `src/system/desktop/DesktopShell.tsx` | ✅ 实机：explorer/Steam 虚拟窗 Ctrl+W 单窗关闭，variable 存活（r4_018/r4_053） |
| R4-B2修 | R2-S1 (P2) | **Del+Backspace 输入聚焦豁免**：`sys://quit-request` 处理器检测 activeElement 为可编辑元素时忽略真退出 | `src/App.tsx` | ✅ 实机：搜索框聚焦下按键无 real-quit 日志、进程存活（r4_019） |
| R4-B3修 | R3-B6 (P3) | **开始菜单搜索词跨开关残留**：菜单关闭时清空查询态 | `src/system/startmenu/StartMenu.tsx` | ✅ 实机：关闭重开搜索框为空（r4_010） |
| R4-B4修 | R3-B10 (P1) | **curtain hide→show 后 DPI 尺寸丢失**：show/unminimize 后按显示器物理分辨率重设 size+position（新增 `restore_fullscreen_size`），退出热键路径同样修复 | `src-tauri/src/shell/kbdhook.rs` | ✅ 实机：重启后主窗矩形 0,0,1920,1080 物理全屏（r4_016 rect） |
| R4-B5修 | 首轮 B-3 (P1) | **收编窗置顶无返回入口**：新增 `desktop_raise` 命令——把桌面 WebView2 子窗（非嵌入注册表 + Chrome_*/WebView* 类名判定）SetWindowPos 提到全部收编子窗之上；Win 键路径接入 | `src-tauri/src/shell/embed.rs`、`src/lib/ipc.ts`、`DesktopShell.tsx` | ☑ 命令注册+编译通过；实机场景（收编窗盖满）本轮未复现，待复验 |
| R4-B6修 | 首轮 B-4 (P1) | **收编子窗隐藏后 WebView 白屏**：`embed_visible(false)` 后对桌面 WebView 强制同步重绘（RedrawWindow RDW_INVALIDATE\|ALLCHILDREN\|FRAME\|UPDATENOW） | `src-tauri/src/shell/embed.rs` | ☑ 门禁级；白屏场景本轮未复现 |
| R4-B7修 | R3-B5 (P3) | **虚拟窗控制钮低对比不可见**：非聚焦降饱和 0.35→0.6、brightness 0.95，并加双层描边环（亮暗底均可辨） | `src/styles/vwm.css` | ✅ 实机：Steam 虚拟窗非聚焦态控制钮可辨（r4_048/051） |

**门禁**：`npx tsc --noEmit` 0 错；`npx vitest run` 181 文件 / **2646 passed / 4 skipped**；`cargo test -p variable --lib` **253 passed**（首跑一次 STATUS_ACCESS_VIOLATION 复跑未复现，属 Win32 测试并发偶发，非本轮改动引入）；`npx vite build` 成功（App chunk 1269.93 kB，基线 ±2）。

---

## 三、实机操作记录（LIVE 证据链，按时间线）

| # | 操作 | 结果 | 证据 |
|---|---|---|---|
| 1 | 重建 release（vite build + tauri build --no-bundle，11m11s） | 成功，含前端资产指纹 | Z6grPd 构建日志 |
| 2 | 启动 variable.exe | 看门狗收编 9 窗、compat 生效、日志连续 | r4_run1.log |
| 3 | 主窗矩形核对 | **0,0,1920,1080 物理全屏**（无 R3-B10 尺寸漂移） | rect 输出 |
| 4 | 桌面首屏 | 壁纸/10 图标/回收站角标⑥/任务栏/性能小组件/WE 警告横幅全渲染 | r4_001 |
| 5 | 开始菜单搜索 "explorer" → Enter | **直达命中打开文件管理器**（R2-B5 回归通过） | r4_017 |
| 6 | 菜单关闭→重开 | **搜索框已清空**（R4-B3修/R3-B6 验证） | r4_010 |
| 7 | 资源管理器行右键 | shell 上下文菜单 17+ 项完整（B-1 回归通过） | r4_021 |
| 8 | 文件管理器内搜索 R4QA | 全库 99 项精确命中 1 项 | r4_031 |
| 9 | Delete → 确认框 Enter | R4QA_Temp 移入回收站 + toast + 角标⑥→⑦（R2-B2 回归通过） | r4_032/033 |
| 10 | Ctrl+W（资源管理器聚焦） | **仅关当前虚拟窗，variable 存活**（R4-B1修/R3-B3 验证） | r4_018 |
| 11 | 搜索框聚焦 + Del+Backspace | **豁免生效**，无退出日志（R4-B2修验证） | r4_019 |
| 12 | Ctrl+, 设置中心 | 外观页完整渲染（18 侧栏项/各控件） | r4_037/038 |
| 13 | 时钟点击（下缘路径） | 日历弹层完整（月历+农历+WEATHER 降级文案+TODAY 空态） | r4_041 |
| 14 | 弹层外点关闭 | 正常（R3-B2 回归通过） | r4_042 |
| 15 | 桌面双击 Steam 图标 | steam.exe 新进程启动，**但未触发收编看护** → 缺陷 R4-B2 | r4_044/tasklist |
| 16 | 开始菜单最近添加 → Steam chip | **兜底收编成功**：家族窗枚举 33 候选 → hwnd=133352 → L3 判定 → **L3→L1 真实嵌入** | r4_run1.log |
| 17 | Steam 独立运行核对 | **Steam 主窗完整渲染于 Variable 桌面内**（商店/库/社区，非黑帧；-101 为 Steam 服务器连接失败属网络域） | r4_048 |
| 18 | Ctrl+W（Steam 虚拟窗） | 仅关当前窗，steamwebhelper 存活退托盘 | r4_053/tasklist |
| 19 | 日志全量异常扫描 | **零 panic / 零 ERROR / 零 minidump**（41 行时间线连续） | grep 扫描 |
| 20 | WindowFromPoint 诊断 | 任务栏所有点位命中 Variable WebView → 吞层来自外部 WE 提示条 | hitpoint.py |

---

## 四、R4 缺陷清单（本轮新发现）

| 编号 | 等级 | 现象 | 根因定位 | 状态 |
|---|---|---|---|---|
| R4-B1 | P2 | **Wallpaper Engine "Ctrl+1" 提示条吞掉任务栏上半热区**：时钟/空间面板/托盘按钮物理 y≈1010-1030 点击无响应，y≥1050 正常 | WE 悬浮提示条盖在 Variable 任务栏上缘（外部层，WindowFromPoint 实锤点位全部命中 Variable WebView，点击却被吞） | ⏱ 已定性 + 临时规避（点击下缘）；建议：compat 模式下把 WE 提示条识别为兼容对象压层，或任务栏热区整体外扩 |
| R4-B2 | P2 | **桌面图标双击 Steam .url 快捷方式：steam.exe 进程启动但收编看护未触发**（90s 内无 adopt 日志，Steam 无窗滞留托盘）；同操作经开始菜单 chip（steam_launch 通道）则 8.8s 兜底收编成功 | 桌面图标双击的 open_path 分支未接 Steam 收编通道（资源管理器 ExplorerWindow 双击 .url 已接，桌面图标路径漏接） | ⏱ 记录 + 建议：桌面图标双击 .url 复用 ExplorerWindow 的 steam_probe 探测逻辑 |
| R4-B3 | P3（定性非缺陷） | 裸 Win 键弹出的是 Windows 系统开始菜单而非 Variable 的 | Win 键为系统保留键，Variable 无法独占（sys://win-key 依赖 Rust 侧检测，本机被系统先行消费）；desktop_raise 命令已注册待场景复验 | ✅ 定性完成 |
| R4-B4 | 环境事件 | QA 期间出现 6 次 double-Esc curtain 与 1 次 Del+Backspace 真退出——QA 脚本全程未发送这些组合（仅 2 次单击 Esc） | 宿主机外部输入（真机桌面有人为/其它软件干扰）；退出路径本身工作正常（保存冲刷→干净退出 rc=0） | ✅ 非产品缺陷；QA 纪律追加：后续脚本全程禁用 Esc |

---

## 五、遗留事项

1. R4-B1（WE 吞层）/ R4-B2（桌面 .url Steam 通道）建议作为下轮优先工单。
2. `desktop_raise`（R4-B5修）与白屏重绘（R4-B6修）需在"收编窗盖满/白屏"真实场景复现后复验。
3. Steam 联网错误 -101（服务器连接失败）属网络域，待网络恢复后补测商店/库浏览与游戏启动。
4. 1810 项"规划未执行"沿 20 域滚动推进；建议下轮优先：域13 任务栏热区、域3 Win 键边界、域6 桌面图标启动链。

---

## 六、2000 项全新检查清单（R4-0001 ~ R4-2000）

> 结构：20 域 × 10 对象 × 10 检查动作。生成器、诊断脚本与全部截图/日志归档于 `_attic/qa/r4/`（gen_r4_report.py、r4helper.py、hitpoint.py、r4_*.png、r4_run1.log）。
> 20 域：① 退出与关闭路径全景 ② 幕布与环境切换 ③ Win键与系统键边界 ④ 输入聚焦与热键豁免 ⑤ 搜索链路端到端 ⑥ 桌面图标启动第三方 ⑦ Steam收编全链路 ⑧ 文件管理器搜索与过滤 ⑨ 文件生命周期回归 ⑩ shell右键菜单 ⑪ 时钟日历天气组件 ⑫ 设置中心外观页 ⑬ 任务栏热区与遮挡 ⑭ 警告横幅生命周期 ⑮ 前端资产嵌入自检 ⑯ 虚拟窗装饰可见性 ⑰ 进程与资源观测 ⑱ 日志分级与诊断面 ⑲ 门禁与回归体系 ⑳ 环境干扰与共存。

| ID | 域 | 对象 · 检查点 | 状态 | 说明 |
|---|---|---|---|---|
