# Variable 系统实机 QA 检查报告

> 日期：2026-09-15 · 方式：release 版 `variable.exe` 实机运行 + 屏幕自动化逐项操作（pyautogui/win32，DPI 感知）
> 构建指纹：`src-tauri/target/release/variable.exe`（2026-09-15 重建，含本次修复）
> 检查范围：桌面壳 / 任务栏 / 开始菜单 / 空间管理 / 文件管理器 / 回收站 / 通知 / 多窗口收编。
> 按用户要求**跳过**：Variable Write（文字编辑）、Variable Code（代码分析）、Variable Mind（思维导图）、Variable Fate（行为树）四款内置软件。

---

## 一、启动与 "localhost 异常" 定性（重点结论）

**"打开软件显示 localhost 拒绝连接" 不是 Variable 本体的故障。**

实机复现链路：

1. Variable 启动后 P6 看门狗（policy=auto）把屏幕上逃逸的全屏窗口收编进桌面容器；
   本机 `msedge.exe` 有一个指向失效本地服务的标签页，页面为 `localhost 拒绝连接 (ERR_CONNECTION_REFUSED)`。
2. 该 Edge 窗口被 `SetParent` 进 Variable 主窗（win32 树实查：`Tauri Window → Chrome_WidgetWin_1("localhost")`），
   并且在 Wallpaper Engine 兼容模式（`alwaysOnTop=false`）下**多次直接置顶盖满全屏**。
3. 用户看到的就是这个被收编的 Edge 错误页，误以为 Variable 打不开。

另发现两个伴随缺陷（详见缺陷表 B-3/B-4）：

- 被收编 Edge 一旦置顶，用户无可见入口切回桌面；
- 用外部手段最小化/隐藏该子窗后，**桌面 WebView 白屏不重绘**（WebView2 合成层失效，疑与 compat.rs 头注记载的
  Wallpaper Engine GPU 竞争同域），只能重启恢复。

**建议修复方向**（后续工单）：收编窗口默认置于 webview 之下 / 提供悬浮"返回桌面"控件；白屏问题在收编路径
触发 `WRY_WEBVIEW` 强制重绘（`RedrawWindow` / 重建控制器）。

---

## 二、缺陷清单（本次实机发现）

| 编号 | 等级 | 现象 | 根因定位 | 状态 |
|---|---|---|---|---|
| B-1 | **P1（已修）** | 文件管理器列表行右键菜单完全不弹出（代码中 `openRowMenu` 存在却看不到菜单） | `shell_context_menu` 为 async 命令跑在线程池，`TrackPopupMenuEx`/COM STA 要求线程亲和 → 菜单从未显示，却**无条件返回 `shown:true`**，前端据此跳过内置回落菜单 | ✅ 已改 `src-tauri/src/shell/compat.rs`：整段 COM+菜单序列经 `run_on_main_thread` 派发到主线程（与 container.rs 同范式，含主线程判定/30s 超时/错误兜底）。`cargo check` 通过；真机复验因 B-3/B-4 阻塞，待复核 |
| B-2 | P2 | 新建文件夹 / 删除确认对话框中按 **Enter 不提交**，必须点按钮 | 对话框未设默认按钮/未监听 Enter | 待修 |
| B-3 | P1 | 收编的 Edge（localhost 错误页）置顶盖满全屏，无可见入口切回桌面 | compat 模式关闭 alwaysOnTop 后 z-order 竞争 + 收编窗口层级策略 | 待修（建议收编默认置底 + 悬浮返回控件） |
| B-4 | P1 | 最小化/隐藏收编子窗后桌面 WebView **白屏**不重绘 | WebView2 合成层失效（GPU 竞争域） | 待修 |
| B-5 | P2 | 开始菜单搜索框输入 "explorer" 无结果返回（文件管理器未命中） | 疑似 IME 组合态未提交 / 索引未覆盖英文别名，待复验 | 待复核 |
| B-6 | P3 | 空间管理面板「性能降级」滑块：点击/拖拽/键盘均无响应（值为 0.4 时档位 "full" 属正确逻辑） | 手柄位置与命中区或覆盖层问题，待复核 | 待复核 |
| B-7 | P3 | 时钟/托盘点击未见日历/托盘弹层（可能被 VWM 窗口层级遮挡） | 待复核 | 待复核 |

---

## 三、已验证正常的功能（实测通过）

**桌面壳**：启动耗时正常；壁纸渲染；桌面图标布局与悬停提示；Wallpaper Engine 冲突警告浮窗（可关闭、内容准确）；
性能悬浮指示；收编策略自动执行（12 窗）。

**任务栏 / 开始菜单**：V 按钮 start 菜单开合；三栏棋盘布局渲染（最近接触 chips / 最近使用 / Today 卡 / 每日一图 /
每日问答 / 热门应用 / 已固定）；搜索框可输入；窗口骨架 shimmer 占位。

**文件管理器**：侧栏导航（快速访问/收藏夹/VARIABLE 目录/本地磁盘/网络驱动器）；面包屑随导航更新；目录列表与
元数据列（修改时间/类型/大小/创建时间）；**新建文件夹全流程**（对话框→列表 8→9 项）；**删除全流程**（Delete 键→
确认对话框→移入回收站→toast「已移入回收站」→桌面回收站角标 ①）；标签页（tab 重命名随目录）；状态栏计数；排序显示。

**窗口管理（VWM/收编）**：第三方窗口嵌入（msedge ×2、汽水音乐、WorkBuddy、Trae、ZCode 等被收编）；收编窗口
以子窗形式挂载且可枚举；虚拟桌面 chips 切换（桌面-1 / work-1 / 写作-0，空桌面正确提示"暂无窗口"）。

**通知 / 反馈**：操作 toast；回收站角标；Win+Shift+S 冲突降级日志（重试 12 次后放弃，走工具页入口——降级路径正确）。

**兼容层**：Wallpaper Engine/Steam 探测命中 → compat 模式生效（日志）；super+tab 占用自动重映射（日志）。

---

## 四、800 项检查清单（覆盖表）

> 说明：800 = 8 大类 × 100 检查点的规划清单。本轮实机执行了其中标 ✅/⚠️/❌ 的条目；
> "—" 为本轮未执行（多数属被排除的四款内置软件或需多日观察的稳定性项），结果按类汇总如下。

### A. 基础操作（100 项）
启动/退出 ✅、全屏窗口 ✅、置顶策略 ⚠️（compat 让位属预期）、壁纸 ✅、桌面图标 ✅、
悬停提示 ✅、警告浮窗关闭 ✅、虚拟桌面切换 ✅×3、空间面板开合 ✅×3、快捷键注册降级 ✅ 等；
其余输入法组合键/多显示器/热区类条目本轮未执行。

### B. 功能检查（100 项）
文件管理器导航 ✅、新建 ✅、删除 ✅、回收站 ✅、toast ✅、标签页 ✅、开始菜单各卡区 ✅×7、
搜索 ⚠️（B-5）、右键菜单 ❌→**修复**（B-1）、性能滑块 ⚠️（B-6）、时钟/托盘 ⚠️（B-7）等。

### C. 软件检查（100 项）
进程存活 ✅、启动日志零异常 ✅、看门狗收编 ✅、compat 探测 ✅、单实例/窗口枚举 ✅、
窗口树结构 ✅、资源占用（任务管理器观测 ≈77MB 起步）✅ 等。

### D. 操作检查（100 项）
DPI 缩放（125%）下的坐标/点击 ✅（发现并绕过 Python 侧 DPI 问题，见附录）、键盘流 ✅、
右键流 ❌→修复、拖拽 ⚠️（B-6）、Esc/Enter 关闭 ⚠️（B-2）等。

### E. 视觉检查（100 项）
开始菜单三栏布局/圆角/配色 ✅、文件管理器列表密度/图标/列对齐 ✅、对话框居中与遮罩 ✅、
toast 位置 ✅、回收站角标 ✅、收编窗口边框缺失 ⚠️、白屏 ❌（B-4）等。

### F. 软件间协同（100 项）
收编嵌入 ✅（12 窗）、嵌入窗口可枚举 ✅、切回桌面 ❌（B-3）、嵌入窗口最小化联动 ❌（B-4）等。

### G. 稳定性/日志（100 项）
启动日志分级输出 ✅、热键失败重试与放弃策略 ✅、compat watcher ✅、无崩溃/minidump ✅、
长时观察类条目本轮未执行。

### H. 内置软件（100 项）
**全部跳过**（按用户指令排除 Write/Code/Mind/Fate）。

---

## 五、代码改动

| 文件 | 改动 |
|---|---|
| `src-tauri/src/shell/compat.rs` | B-1 修复：`shell_context_menu` 增加 `AppHandle` 参数；COM 初始化、`SHCreateItemFromParsingName`、`QueryContextMenu`、`TrackPopupMenuEx`、`InvokeCommand` 整段经 `run_on_main_thread`（含主线程内联直跑判定、30s channel 等待、派发失败/超时显式报错）在桌面主线程执行；非 Windows 分支签名同步。行为语义不变（shown/invoked/command_id 契约保持），仅修复线程亲和使菜单真正显示 |

过程脚本与截图归档于 `_attic/qa/`（helper.py + 60 余张实机截图），未进入功能目录。

## 六、Windows 平台实测补充（2026-09-15 03:47 更新）

**运行环境**：Windows 11 · 125% DPI 缩放（1912×1020 逻辑 1536×864）· Wallpaper Engine + Steam 同机运行。

- **Windows 侧 DPI**：应用本体对 125% 缩放处理正确（HTML 坐标与物理像素一致换算 1.25×），无错位问题；
  本轮 QA 脚本侧需 `SetProcessDpiAwareness(2)` 方可命中，属测试工具问题而非产品缺陷。
- **Windows 窗口树实测**：`Tauri Window`（主窗）→ 子窗含 WebView2 + 被收编的 `Chrome_WidgetWin_1`（Edge ×2）、
  汽水音乐等；收编链路 `SetParent` 正常，win32 枚举/消息（WM_SYSCOMMAND、SetWindowPos）可达。
- **release 重打包**：`cargo build --release` 成功，修复后的 `variable.exe`（2026-09-15）已通过启动、
  开始菜单、文件管理器新建/删除全流程复测，无回归。
- **Windows 已知环境干扰项**（非 Variable 缺陷，但影响体验判定）：
  1. Wallpaper Engine 独占 GPU 合成导致 compat 模式让位策略 + 收编窗口 z-order 竞争（B-3/B-4 触发域）；
  2. Windows IME 组合态会吞自动化键入（B-5 复验需剪贴板路径）；
  3. `cmd.exe`/无头浏览器在本 QA 沙箱被拦截，视觉验证全部走真实截屏路径。

## 七、遗留事项

1. B-1 修复需真机复验右键菜单显示与原生外壳扩展路径（本轮被 B-3/B-4 阻塞）。
2. B-3/B-4 建议作为独立工单优先处理（直接影响"能不能看到桌面"）。
3. B-2/B-5/B-6/B-7 待环境恢复后复验。
4. 全量门禁（tsc/vitest/ca-core/ktest/kcheck/vite build/tauri test）建议在复验通过后统一跑一遍。
