# Changelog

本文件记录面向用户与协作者的显著变更。批次级细节见 `project_memory.md`；
架构与计划见 `docs/BLUEPRINT-1.0sno9u.vxe.md` 与 `docs/MASTER-PLAN-1.0sno9u.vxe.md`。

## [Unreleased] — Varix STAR I · AI-K1 性能域深化（F041-F057 十七域落地）

**kernel/varix/src/perfstar/**（AI-K1 泳道 B 前段，主册 B-3 深化设计报告
G-B-01~G-B-17 判据实装层；robust.rs 274 域函数指针表注册接线）：

- **F041-F044 观测面四件**：帧率账本（每帧四项打点 + 分钟桶聚合 + 24h
  降采样）、帧率归因器（四类嫌疑贡献模型回线判定）、冷启动画像（五段
  刻度 + 中位数）、预取指纹 v2（位图指纹 + 顺序区序列化）。
- **F045-F048 资源调度四件**：页缓存水位三档（`lru_bytes` 字节账/页账
  对齐）、写合并窗口三档自适应（dwell 抑制）、调度延迟预算四类分解
  （几何直方图 + aging）、CPU 频率联动（P-state + Silent 封顶唯一判据）。
- **F049-F053 底层治理五件**：空转清零、中断合并（2000ns 窗口 + 动态
  收缩 + 熔断）、4MB 大页池、堆碎片尺寸分级池（零堆 grep 自证）、启动
  四链并行。
- **F054-F057 体验四件**：图像 SIMD 解码（标量逐像素对拍）、字形光栅
  缓存（LRU + shelf packing）、合成器脏区深化（打字 P95 <5% 屏）、IO
  调度分级（三队列 EDF + fsync 硬承诺 + 反向保护时长有界 + 24h 混载
  零饥饿）。
- 自检判据逐条钉死：17 域 **156 CheckSet 检查项 + 98 宿主单测全绿**；
  全量 3897/3897 PASS（debug 通道）。施工期缺陷账本 **7 处实现侧真缺陷**
  修复（含 iotier 反向保护 resume 死锁、pagewater 字节账脱钩、cpufreq
  封顶失效三处高危）与偏差登记见 `docs/AI-K1-完成报告.md`。

## [Unreleased] — Varix STAR I · AI-K2 内核底盘域·后段（F058-F075 十八项落地）

**kernel/varix/src/star/**（AI-K2 泳道一 B 后段，域聚合器 `run_star_checks` blocks=19，
隔离舱实测 **143 通过 / 0 失败 / 0 警告**）：

- **F058-F067 底盘十件**（memcomp/netbatch/battery/reggate/selfcheck/touchpad/
  audiolow/wakegov/fsjournal/coldhot）：内存压缩前瞻评估件、网络小包批处理
  （SSH P99 ≤30ms 线）、应用能耗账本、基准回归门、性能自检、触控板手势前瞻、
  音频低延迟链（单流 ≤20ms）、唤醒源治理（合盖 2h ≤2 次）、FS 日志策略、
  启动 IO 冷热分离（P99 <2ms）。
- **F068-F070 底盘后段三件**：渲染资产按需装载（单主题 ≤80MB、切换无白屏）、
  性能模式三档（静音/均衡/性能 12 旋钮 + F197 75℃ 强制静音、原子生效）、
  性能域总判据（四层门 + 全帧集合分位 P95 ≤12.5ms/P99 ≤16.6ms + 版本漂移防护）。
- **F071-F075 桌面体验五件**：开始菜单搜索直达（三类首结果 10/10 + 空查询
  短路）、最近使用引擎（frecency 毫倍定点 + 三消费面同源）、任务栏预览缩略图
  （应用分组卡列 ≤5 + 2Hz/1Hz 刷新降档 + 一致性对账）、跳转清单（F072 同源
  最近区 + 应用域固定集 + 静态任务组 + 键盘可达）、托盘系统（三件套 + 折叠区 +
  幽灵图标 30s 清道夫 + 滑杆 80fps 帧账闭环 + 点击 <100ms 闭环对账）。
- 自检判据逐条钉死：每项 `run_*_checks() -> CheckSet`，聚合全绿；施工期缺陷
  账本 40 项修复记录与行数对账（含低于 90% 目标项如实登记）见
  `_attic/aik2-f058-f075/行数对账与缺陷账本.md`。

## [Unreleased] — VARIX 三体系统：M5 基建批收口（2026-09-22：S4.2 MSC + exFAT 受限直写 + last_boot 闭环 + AHCI + ushell 真壁纸）

**内核（kernel/varix）**
- **S4.2-A xHCI MSC 传输层**（`drivers/msc.rs` 新文件）：BOT + SCSI 最小指令集
  （TUR/INQUIRY/READ CAPACITY/READ10/WRITE10/REQUEST_SENSE），`BulkPipe` 泛型协议层
  与 xHCI bulk 端点扩展（EP1 OUT/IN、传输环、事件容错）解耦；数据段"提前到达 CSW"
  识别覆盖设备端 stall 失败语义；协议级 + 控制器级模拟器 12 项测试全绿。
- **S4.2-B exFAT 受限直写层**（`fs/exfat_rw.rs` 新文件）：**扇区粒度**（从不物化整簇，
  真实 560GiB exFAT 的 128KiB 簇安全）——同尺寸就地改写 + 同簇增长改写（STREAM
  Size 字段单扇区 RMW 作提交点）+ 根目录新建单簇文件（数据→FAT→位图→目录项顺序，
  断电=孤儿簇可回收）；断电注入"不可见或完整"二态不变式入 ktest（8 项）。
- **last_boot 写回闭环**（方案2 根解）：`bootcfg::splice_last_boot` 文本级拼接 +
  `record_last_boot`（MSC 优先 / QEMU 第二 NVMe 兜底）+ 引导流挂钩；QEMU ×5 交替
  raw 断言通过；Windows 侧仍写 SHARED 真相源（单一事实源不变）。
- **S4.6 AHCI 最小栈**（`drivers/ahci.rs` 新文件，追加件）：单端口单槽 LBA48 纯轮询；
  PCI 0x0106 扫描；目标态探针默认只读，写回环须 cmdline `ahci_selftest=1`；
  模拟器 6 项全绿。ktest 全量 **3146/0** + kcheck 0。
- **ushell 真壁纸**：`wallpaper.rs` 经 Limine internal module 装载 1280×720 RGB565
  （三世界同源静态帧），最近邻整屏 blit + 削角逐像素真色回填
  （FRAME_WALLPAPER/PX/FILL_RGB 三子命令）；缺席=回退色带，绝不阻塞引导。
  取证：1.8MiB include_bytes 进 .rodata 引发 RW 段布局敏感 fatal #PF、Limine
  内模块**子目录路径挂死**（根级正常）——资产模块通道双戒律入库。
- S4.1 xHCI HID 维持"模拟器全绿、真机 SOP 权威"结论；WHPX 宿主不可用已登记。

**桌面端（src-tauri / src）**
- 门禁修复：`we_wallpaper` 抽取核存在性检查改可注入谓词（WE 库搬家后夹具路径
  失效引发的机器相关测试回归）；vitest 2851 / tsc 0 / variable --lib 348 / ca-core 356。

**文档与档案**
- `docs/acceptance/INDEX.md` 建档（AI-6 档案库索引）+ 三份新验收记录
  （ai5 S4.2 批 / ai4 真壁纸 / ai6 门禁基线）+ ai1 真机冷启动 ×10 SOP 执行卡。

## [Unreleased] — VARIX 双域系统：批次收官（2026-09-18：27/28 实机走查全绿 + 58 拔出全链 + 批次任务 29-87 清账）

**内核（kernel/varix）**
- **嵌入层走查 20/20 PASS**（任务 27/28，`_attic/p27-28-shell-demo.py`）：ushell ring3
  用户态桌面壳承载 BootScreen 回放→桌面→开始菜单→文件管理器（SHARED exFAT 真实数据源
  `source=shared-exfat` 实测）→设置页（KV 读写 rc=0）→关于，全程 HMP 键盘驱动+逐屏截图。
  修复三根因：syscall ABI r8-r11 保存恢复（ushell 用户态 #PF 致命）、`SHIM_VFS_SOURCE`
  漏接线（页脚数据源恒 demo-tree）、`RamBlk` 由 PMM order-11（运行时分配失败+双倍超额）
  改 static .bss 4MiB（分配不可能失败，符合大缓冲戒律）。
- **拔出全链**（任务 58，`_attic/p58-pull-chain-drill.py`）：SHARED 盘运行中移除 → 内核
  「IO 失效卸载」（`mount::uninstall_io_failed`，与 `mount::healthcheck` PCI vendor 探测
  双通道）→ 文件管理器数据源**如实降级** demo-tree（零冒充）→ RAM KV 独立照常 → 断电
  重插恢复挂载+KV 干净重建（**10/10 PASS**）。连带修复 ushell 设置页光标重进不归零
  （与开始菜单/文件页「页面重进=顶部」约定对齐）。q35 热拔插需 guest ACPI 配合的语义
  差异如实登记于报告 json。任务 60 机型代理矩阵首轮 3/3 PASS（q35 基线/双核/i440fx
  诚实降级）。
- **性能口径**（任务 71，`scripts/perf-gate-usb.py`）：boot_ms=896 / first_frame_ms=896
  （ushell 首画帧内核时钟打点 `SHELL: first-frame ms=`）/ interaction P95=51.7ms，
  三指标全 PASS + 基线建档（×1.2 非劣回归带）。
- 任务 52 ramcache（LRU+水位回收对接+关机清零断言，实机探针 verdict=ok）、61 winecaps
  （能力收敛+申请式授权+审计，探针 verdict=ok）、62 PE 拒绝表（50 合法变体零误拦/20 恶意
  全拒具名理由）随批交付；ktest 3026 绿 / kcheck 0 告警。

**前端（src/）**
- 任务 50/51/53/54 VWM 引擎流窗口+冷启动阶段化叙事+画质三档+异常三场景状态机
  （engineModel/engineSessions/vwm，engine-model.test 全绿）；任务 55 IME 键表全量
  （kernelInput 0..57 与内核同序同源）。
- 任务 57 设置页四组 UI（DualBootTab 引导行为/软件通道/白名单/资源档位 + EngineTab 引擎组）。
- 任务 76/78/82 新增逻辑核：`skeleton.ts`（E1 骨架屏统一规格，300ms 快路径不闪）、
  `dataMigration.ts`（I3 搬家四步状态机断点续走+I4 退役两档三轮覆写双确认）、
  `journeys.ts`（M1-M6 六条旅程跑查器，能力注入缺失=如实 skip）。
- 门禁：tsc 0 错；vitest 2787 passed / 4 skipped（200 文件，含新增 23 例）。

**文档与工具**
- 任务 64 威胁清单终审（15 条对策+残余风险双栏+诚实声明页同源）、67 灾备 SOP、
  68 配置一致性校验器（selftest 4 场景）、60 QEMU 代理机型矩阵首轮三配置
  （i440fx 无 ECAM 诚实降级路径验证）；任务表/推进总表三处状态同步。

## [Unreleased] — 第四轮实机 QA：交互缺陷清零（2026-09-15：R4-0001~R4-2000）

- **Ctrl+W 改绑「关闭当前虚拟窗」**（`src/system/desktop/DesktopShell.tsx`）：捕获阶段拦截，
  不再落到 WebView2 默认行为导致整体退出；输入框聚焦时仅拦截默认行为，组件级 Ctrl+W 继续生效。
- **Del+Backspace 输入聚焦豁免**（`src/App.tsx`）：`sys://quit-request` 在可编辑元素聚焦时忽略，
  杜绝文件操作中 Delete+Backspace 组合误触真退出（R2-S1 遗留建议落地）。
- **开始菜单搜索词跨开关残留清零**（`src/system/startmenu/StartMenu.tsx`）：菜单关闭时清空查询态（R3-B6）。
- **curtain 恢复 DPI 修复**（`src-tauri/src/shell/kbdhook.rs`）：hide→show 后按显示器物理分辨率
  重设窗口尺寸与原点（新增 `restore_fullscreen_size`），125% 缩放下不再缩为逻辑全屏（R3-B10）。
- **收编窗回桌面通道**（`src-tauri/src/shell/embed.rs` + `src/lib/ipc.ts`）：新增 `desktop_raise`
  命令，把桌面 WebView2 提到全部收编子窗之上（首轮 B-3）；`embed_visible(false)` 后强制 WebView
  重绘修复白屏（首轮 B-4）。
- **虚拟窗控制钮对比度**（`src/styles/vwm.css`）：非聚焦红绿灯降饱和 0.35→0.6 + 双层描边环（R3-B5）。
- 门禁：tsc 0 错；vitest 2646 passed / 4 skipped；cargo test 253 passed；vite build 成功。
  实机第四轮 QA 报告（2000 项全新清单）：`docs/Variable系统实机QA检查报告第四轮-2000项-2026-09-15.md`。

## [Unreleased] — Steam 全量收进 Variable（2026-09-15：steam:// 与快捷方式不再落宿主桌面）

- **`open_path` 识别 Steam 产物**（`src-tauri/src/system.rs`）：`steam://` 链接本体、
  以及内容指向 `steam://` 的 `.url` 快捷方式（Steam 桌面快捷键格式）不再走
  「宿主 Windows 默认程序」打开，改走 Steam 通道——CEF 兼容态 + ShellExecute +
  `spawn_steam_adopt_watcher` 收编看护，Steam 主窗与游戏窗收进 Variable 桌面运行。
- **公共通道 `steam_open_url`**（`src-tauri/src/shell/ecosystem.rs`）：
  `steam_launch` 与 `open_path` 的 Steam 路由共用同一裁判（兼容态 + 收编看护），
  并对非 `steam://` 前缀做防御性校验。
- **资源管理器双击 `.url` 先探测**（`src/system/explorer/ExplorerWindow.tsx`）：
  命中 Steam 快捷方式直接进收编通道，不再弹出「用宿主 Windows 打开」选择器；
  非 Steam 的 .url 与读取失败时保持原有流程不变。
- 门禁：tsc 0 错；vitest 2646 passed / 4 skipped；`cargo test -p variable --lib`
  system 模块含新增 `steam_probe` 测试全绿；vite build 成功。

## [Unreleased] — 1.0sno9u.vxe（2026-09-14 代码大检查与优化：零告警回归 + 前端按需加载 + 仓库卫生）> 本轮为"不改变功能语义"的质量/性能治理：三条测试线由改前改后均全绿，
> 交付物是**更小的首屏、更少的编译告警、更干净的仓库**。

### 性能：桌面环境首屏体积下降约 40%
- **四个重视图改为按需加载**（`src/App.tsx`、`src/system/windows/VwmAppContent.tsx`）：
  写作（EditorView）/ 思维导图（MindmapView）/ 项目分析（ProjectAnalysisView +
  CodeXrefPanel）/ 命运推演（FateView）此前被静态 import 打进环境主 chunk，
  而桌面分支（`view === "desktop"`）根本不渲染它们。改为 `React.lazy` + `Suspense`
  后只在用户真正打开对应窗口时拉取，实测主 chunk **2010.79 kB → 1257.37 kB**
  （gzip **723 kB → 433 kB**，−40%），拆出的 EditorView(342 kB) / MindmapView(101 kB) /
  FateView(101 kB) / ProjectAnalysisView(90 kB) / XrefPanel(4 kB) 变为独立 chunk。
  已验证这 5 个模块无顶层副作用，视觉/操作/功能行为零变化（改前改后 vitest 均
  2629 通过 / 179 文件 / 0 失败）。

### 质量：内核编译告警清零（kcheck + ktest 双零告警）
- **lib 目标 28 处告警全清**：`shell/overlay.rs`、`shell/tbengine.rs` 的测试专用
  `render_to_string` 下沉进 `#[cfg(test)] mod tests` 并移除随之无用的 `String/Vec` 导入；
  删除死代码 `compatruntime::clamp_usize`、`fs23_mount::link_count_of/link_count_pub`；
  移除 `WakeGovernor` 中从未被读取的冗余 `whitelist` 字段（生效白名单本就走
  `policy.whitelist() ⊕ overrides`，该字段是死状态）；清理 12 处多余 `mut` /
  未用变量 / 死语句。
- **修正一处"末态读取"式弱断言**：`fs/fs23_journal.rs` 的 X05533「重放幂等」原写成
  `t.verify_and_replay() == 1; t.verify_and_replay() == 0`，前一个比较结果被丢弃
  （编译器 `unused_must_use`），实际只校验了后半句。改为 `&&` 连接，断言恢复完整语义。
- **测试目标 16 处告警全清，并诚实化 6 处恒真断言**：`render2d`/`audio`/`motion` 中
  `c[p] > 255`（u8）、`s <= 32767`（i16）、`v <= 255`（u8）这类比较被类型系统恒真，
  属"假绿"断言。改为有实质意义的确定性/信号非零断言，或删除并注明类型已保证边界。
  测试数与通过数不变（kernel 2713 全绿）。

### 仓库卫生：非功能性过程产物归位 `_attic`
- **46 个一次性调试脚本移入 `_attic/archive/tools-process/`**（`git mv` 保留历史）：
  `dbg_*.py`（17）、`realclick*/clickcancel*/clickicon/enumwins/checkhung/cancel*` 等
  PowerShell 点击探针（9）、`mark-*/stage-*/done-*/revert-aurora/strip-landscape/
  fix-ellipsis/probe-regex/aurora-halfres/check-aurora/ghdata_push/make_cmds_async/
  debug_ctx_freeze` 等一次性批处理（20）。判定口径：在 `tools/` 与 `_attic/` 之外
  零引用，且不在 `package.json` / CI / `scripts/` 中出现。功能性工具（bench / audit /
  keymap-audit / qmon / gen-* / hooks / portable 校验器等 38 个）原样保留。
- **清理被 git 跟踪的 Python 字节码缓存** `tools/portable/__pycache__/*.pyc`，
  `.gitignore` 追加 `__pycache__/` 与 `*.pyc`。

### 质量：桌面端后端（src-tauri）编译告警清零（65 → 0，含 --all-targets）
- **28 处忽略 must_use 返回值的 Win32 调用**统一改为 `let _ = ...`
  （`SetParent` / `SetWindowPos` / `ShowWindow` / `PostMessageW` / `EnumWindows` /
  `RmEndSession` / `CloseHandle` / `CancelIoEx` / `CertFreeCertificateContext` /
  `SystemParametersInfoW` / `ActivateKeyboardLayout` / `save_index` 等）——行为完全等价。
- **修复单实例守卫的无效泄漏**（`shell/single_instance.rs`）：`std::mem::forget(h)` 作用于
  `Copy` 类型（`HANDLE`）是空操作，编译器已警告。改为把句柄存进进程级 `AtomicPtr` 静态，
  显式表达"句柄生命周期 = 进程生命周期"，且不随 windows-rs 对 `HANDLE` Drop 语义的
  版本差异而改变行为（守卫语义不变，双开防护仍生效）。
- **清理死代码**：`bplustree` 的 `is_empty`（仅测试期使用，加注保留）、`schema::Migrator`
  类型别名、`schema` 与 `versions`/`diagnostic` 的未用导入、`search::MAX_MATCH_LINES`
  常量、`git_panel::ce` 与容器单测里未用的 `blob` 辅助函数；`uxv::apply_remove` 的
  `let mut pos = 0; … pos = 4;` 死存储改为 `let mut pos = 4;`（语义等价）。
- **6 处"保留待接线/说明性"条目改为显式 `#[allow(dead_code)]` + 中文注释**，而非粗暴删除：
  `embed::ensure_per_monitor_dpi`（无清单直跑档兜底入口）、`envs::nested_env`（M6 嵌套
  实例执行档入口）、`exec::default_kind`（serde 默认值字符串引用）、`uxv::SnapshotState
  ::data_tail`、`openhub::GatewayHandle::thread`、`workshop::RuntimeCtx::boot_done_ms`。
  逐条注明"为什么留、谁来接线"，避免下一次检查再误判。

### 构建：`build-windows.bat` 打包修复（CRLF 检出 + 脚本加固）
- **根因（行尾）**：`.gitattributes` 的 `* text=auto eol=lf` 无差别地把**批处理**也强制
  检出为 LF。`build-windows.bat` 在工作区实测 **0 处 CRLF / 123 处裸 LF**，而 `cmd.exe`
  解析 LF-only 批处理时会在 `if ( )` / `for` 多行块处断言失败。该属性随
  「仓库清理」一并生效后，**任何全新克隆拿到的都是坏掉的 .bat**——这正是"无法正常打包"
  的直接原因。修复：`.gitattributes` 增加 `*.bat text eol=crlf` 与 `*.cmd text eol=crlf`
  的显式覆盖（gitattributes 优先级高于 `core.autocrlf`），工作区 `build-windows.bat`
  已重写为 **173 CRLF / 0 裸 LF / 0 非 ASCII 字节**，`git ls-files --eol` 复核为
  `i/lf w/crlf attr/text eol=crlf`。
- **脚本加固（行为与四种模式语义不变）**：
  - 移除两处 cmd 解析高危写法：`copy ... || ( ... )` 的**嵌套括号 + `||`** 组合、
    以及 `echo ... ^(data ...^)` 的**块内转义括号**（改为纯 ASCII 无括号措辞）。
  - 未知模式不再静默回落 NSIS，改为打印用法并 `exit /b 2`。
  - 新增前置校验：`node_modules\.bin\tauri.cmd` 缺失时给出明确指引（避免 `npx` 联网兜底）。
  - 后端测试由 `cargo test` 改为 `cargo test --workspace`，与仓库门禁口径一致
    （覆盖 `container` crate）。
  - 构建后新增产物校验与**绝对路径回显**（NSIS / MSI / 便携），产物缺失即 `exit /b 1`，
    不再出现"报 BUILD OK 但什么都没产出"。
  - 便携模式清理 `dist-portable` 失败时给出明确报错（原实现会静默继续）。
- **仓库卫生**：`.gitignore` 追加 `dist-portable/`（`portable` 模式的构建产物，此前未被忽略）。
- **验证**：打包链路已原生端到端验证——`npx tauri build --bundles nsis` 成功产出
  `src-tauri\target\release\bundle\nsis\Variable_1.0.0_x64-setup.exe`，故工具链无问题，
  缺陷确在 `.bat` 包装层。本轮未改动任何 Rust/前端源码，三条测试线基线不变。
  说明：本沙箱禁止调用 `cmd.exe`，`.bat` 本身未能在此环境实际执行，需真机双击复核。

### 验证（如实）
- `npx tsc --noEmit` 0 错；`npx vitest run` 2629 passed / 4 skipped / 0 failed；
  `cargo test -p ca-core`（C 线）356 passed；`cargo ktest --lib`（K 线）2713 passed；
  `cargo kcheck` 与 `cargo ktest --lib` 均 **0 warning**；`cargo kbuild` 成功出 ELF；
  `npx vite build` 成功（7 个 MPA 入口产物齐全）。
- 未在本轮改动任何业务逻辑、UI 样式与交互；未做真机/QEMU 视觉验收（无 GUI 环境），
  按仓库纪律如实标注：视觉效果需真机复核。

## [Unreleased] — 1.0sno9u.vxe（2026-09-08 窗口路 AI-2 收口：W-1…5 / C-1…8 / B-27 / D-3）

> 窗口路全部批次代码级完成。真机验收项见 `docs/acceptance/ai2-窗口路验收.md`（逐批清单）。

### 修复
- **cargo test 编译失败**：`container.rs` 单测引用已不存在的 `super::win` 路径（改 `super::`）、
  `embed.rs` 两处 `EmbedSession` 初始化缺 `capture` 字段（补齐）。修复后 AI-2 域单测全绿
  （`sysmaint.rs` 2 个失败属 AI-5 域，未越界）。
- **L4 误嵌入 bug**（批次C-5 语义）：嵌入主路径 tier 路由 `_` 兜底臂吞掉 L4 → 反作弊/独占全屏
  应用被错误强制重父级嵌入。补显式 L4 让位臂：`attached:false` + hint 归因 reason，
  让位由运行时看护承担；`_` 兜底仅剩 L1 主路径。

### 新增
- **C-8 嵌入验证**（主计划 7.8）：`portable/AI5/Compat-Matrix.ps1` 新增 `-Action Run-Embed / Report-Embed`——
  逐软件启动 → 窗口样式探测分层（决策树镜像 `compat_probe.rs`），记录 `{tier, captureMs, crash, inputOk, dpiOk}`；
  `inputOk/dpiOk` 诚实输出 `todo` 待真机人工回填，脚本不臆造。真 PowerShell 5.1 已验证 4 条路径。
- **C-5 让位归因 `compat.hint`**（主计划 7.5）：`CompatInfo` 增 `hint: fullscreen | anticheat`
  （serde default 平滑升级旧 apps.json）；探测器区分反作弊服务特征与独占全屏引擎窗口，
  供看护语义分流（anticheat 停用 kbdhook + 横幅）。单测 `l4_hint_classification` 覆盖。
  前端消费闭环：LauncherManager L4 徽标（AC/FS，悬停完整说明）+ i18n zh/en。
- **C-4 bench 项**（主计划 7.4）：`tools/bench.cjs` 新增 `captureE2E`（预算 P95 <50ms），
  按 vwmOpen 先例如实 SKIPPED 并写明真机测量协议；`--no-gui` 实跑报告落盘 `docs/bench/2026-09-08.md`。
- **D-3 契约单测**（`shell_watch.rs`）：白名单 15 类唯一小写、策略三态、默认「询问」不自动回收。
- **证据归档**：`docs/acceptance/ai2-窗口路验收.md`（14 批索引）+ `w2-dpi-regression.md`（混合 DPI 10 款清单）+ `compat-top200.md`（C-8 口径与执行命令）。

### 说明
W-1…W-5 / C-1…C-6 / B-27 / D-3 主体代码由本路前序会话落地（`embed.rs`/`container.rs`/`capture.rs`/
`compat_probe.rs`/`winman.rs`/`ecosystem.rs`/`shell_watch.rs` + 前端 `vwm.ts`/`snapshots.ts` 接线），
本轮完成收口：过期测试修复、C-8 工具段、契约单测、证据归档；附录 A 勾选按 DoD 留待真机实测。

## [Unreleased] — 1.0sno9u.vxe（2026-09-08 体验路 AI5 收口：F-1…F-7 / A-1…A-5 / D-5 / B-30）

> 体验路全部代码级任务完成。真机验收项见 `docs/acceptance/b30-三宿主验收矩阵.md`。

### 基础功能完备化（F）
- **F-3 任务管理器**：进程/性能/启动项/服务四页（`src-tauri/src/shell/taskman.rs` + `src/system/taskman/TaskManApp.tsx`），系统关键进程保护。
- **F-4 全局文件搜索**：容器内索引（`fsindex.rs`，1.2s 增量轮询）+ 拼音兜底 + 扩展名/类型/大小过滤，结果只在 VWM 资源管理器打开。
- **F-5 通知动作 + 音量合成器 + 输入法指示 + 媒体控制**（`audioime.rs`）：通知条目 2 动作按钮；IAudioSessionManager2 会话级音量；任务栏 IME 语言/中英态（切换仅作用于 Variable 进程）；SMTC 媒体进度条（探测不到自动隐藏）。
- **F-6 系统维护**（`sysmaint.rs`）：计划备份（daily/weekly+小时点、跨会话补偿）、本地更新包 SHA-256 校验+失败回滚（下载边界如实声明）、备份可恢复性抽检自检。
- **F-7 无障碍**：高对比度主题、`:focus-visible` 全局焦点环、字号 80–150%、`audit.cjs` 升级为 CI i18n 门禁（缺键/缺后端即 fail）。

### 界面精修（A）
- **A-1 Design Tokens**：`src/design/tokens.css`（OKLCH 语义层 + 三亮度层 + 字阶/间距/圆角/阴影/动效全 token），文档 `docs/DESIGN.md`；audit 增加裸色值计数。
- **A-2 动效**：窗口 spring 入场（4% 过冲+阴影先行）、最小化 5% 压扁、贴靠统一 170ms；`data-reduce-motion` 全局 80ms 降级。
- **A-3 状态设计**：共享骨架屏组件（任务管理器/音量合成器加载态）、`tools/visual-audit.cjs` 像素审计（puppeteer 缺失时如实退出）。
- **A-4 声音**：Web Audio 合成 6 音（启动/通知/闹钟/错误/贴靠/回收站），音量/静音独立设置，勿扰自动静音。
- **A-5 首次体验**：OOBE 收敛三步（隐私契约逐条确认 → 壁纸+5 套一键换装预设 → 布局偏好），≤90s。

### 边界与验收（D/B）
- **D-5 接管边界诚实清单**：README「21.5 能力诚实声明」+ 设置→关于页内置（UAC 安全桌面/直跑档事后回收/反作弊让位/VM 系统对话白名单/LogonUI）。
- **B-30**：三宿主验收矩阵文档收口（`docs/acceptance/b30-三宿主验收矩阵.md`），真机项如实待办。

## [Unreleased] — 1.0sno9u.vxe

## [Unreleased] — 1.0sno9u.vxe（2026-09-07 便携系统 AI-5 交付核：主计划第 11+12 章）

> 多 AI 并行拆分（`docs/PORTABLE_AI_SPLIT_PLAN.md`）的 AI-5 交付核落地。
> AI-3（第 6+7 章）、AI-4（第 8+10 章）此前已合入；本次补上测试验收与交付运维。
> 实现进度：主计划 12 章中 10 章已落地（1/2/3/6/7/8/9/10/11/12），仅余第 4/5 章（AI-2 隔离核）。

### CI 修复：PowerShell 脚本编码（windows-latest 首次真机解析）

> **结果：CI 已双绿**（`backend` + `frontend`，run 34094563831），
> `Run-PortableTests.ps1` 在真 PowerShell 上 **74 项检查全过**。
> 首次真机执行共暴露 5 个真实缺陷（下表 1 个编码 + 4 个运行时），全部已修复。
> 明细见 `docs/AI5-测试交付.md` §4.1。

- **根因**：`portable/**/*.ps1` 全部是「UTF-8 无 BOM」。Windows PowerShell 5.1 对无 BOM
  文件按系统 ANSI 代码页（CP1252）解码，而汉字「应」的 UTF-8 末字节是 `0x94`，
  在 CP1252 中正是 `U+201D ”`；**PowerShell 把智能引号当作字符串定界符**，于是字符串被
  提前闭合，后面的 `)` 失去配对的 `(`，报 `Missing closing ')' in expression`。
  CI 报错的 `Run-PortableTests.ps1:160/168` 两行，其上一行恰好都含「应」字。
- **修复**：为全部 26 个 `.ps1` 加 UTF-8 BOM，使 PS 5.1 与 PS 7 都按 UTF-8 解码。
  同时把 `Run-PortableTests.ps1` 的 18 处反引号续行合并为单行，消除续行的额外脆弱性。
  注：BOM 变更也落到 `AI1/`、`AI4/` 的文件上——这是字节级编码前缀，不改动任何逻辑，
  属跨核同步必需的修复。
- **排障改进**：`portable.test.ts` 原先只保留最后一个 shell 的错误（`lastErr` 被覆盖），
  导致 `pwsh`(7) 的真实报错被 `powershell`(5.1) 的报错顶掉。现改为逐个 shell 记录并全部输出。
- **检查工具**（均已入库 `tools/portable/`）：新增 `ps_lex_check.py` 真正的 PowerShell 词法器（注释/单双引号/here-string/
  反引号续行/`$( )` 子表达式/智能引号定界）。旧的 `ps_struct_check.py` 只是括号计数器，
  曾对本缺陷给出 26/26 通过的误报；新词法器可复现该缺陷（修复前 3 处错误，修复后 0 处）。
- **运行时缺陷 2／数组 splatting 是位置绑定**：`& $p @ScriptArgs` 语法合法，但数组
  splatting 按**位置**传参，于是 `"-Action"` 这个字符串本身成了 `-Action` 的值
  （`The argument "-Action" does not belong to the set ...`）。改为**哈希表** splatting
  做命名绑定，开关参数映射 `$true`，`[int]` 参数去引号，共 16 处。
- **运行时缺陷 3／`Write-Host` 走信息流**：各脚本统一用 `Write-Ai5 -> Write-Host`，
  输出在信息流(6) 上，而 `2>&1 | Out-String` 只并入 stderr，捕获结果恒为空，
  导致所有 `-ExpectText` 断言必然失败。改用 `*>&1` 合并全部输出流。
- **运行时缺陷 4／变量名大小写不敏感**：`Accept-Gate.ps1` 的脚本级 `$Evidence`
  （证据根目录）与循环内每行的 `$evidence`（结论字段）在 PowerShell 里是**同一个变量**，
  循环开头 `$evidence = ""` 会把 `$Evidence` 一并清空，随后 `Join-Path $Evidence ...`
  收到空串。脚本级变量改名 `$EvidenceDir`。
- **运行时缺陷 5／Mandatory 数组逐元素校验**：`Save-Ai5Text` 的
  `[Parameter(Mandatory = $true)][string[]]$Lines` 会校验每个元素非空，而验收报告的
  Markdown 本就含空行（`$L += ""`）。加 `[AllowEmptyString()]`。
- **新增检查工具** `tools/portable/ps_case_collision_check.py`：扫描同一脚本内仅大小写不同的变量名
  （函数参数新建作用域，默认排除）。对修复前的 `Accept-Gate.ps1` 能报出风险并退出 1，
  对修复后的全仓 26 个脚本退出 0。

### 测试与验收（第 11 章）

- **兼容矩阵 Top200**：`portable/AI5/Compat-Matrix.ps1` + `Data/compat-matrix.json`，
  办公/设计/开发/工具/游戏 各 40 条、名称全局唯一。判定枚举仅 `pass/warn/fail/todo`，
  其中只有主计划 11.1 与 16.2 点名过的 14 条带既有结论，**其余 186 条保持 `todo` 待真机回填**，
  不因软件知名就打 ✅。`Run` 只测 `Data\Apps` 下能定位到主程序的条目，找不到记 `skip`。
- **混沌工程 12 场景**：`Chaos-Inject.ps1` + `Data/chaos-scenarios.json`（扩充 21 的 10 个必测场景
  + 主计划 11.2 的看门狗与 `0x80000003` 两项）。`0x80000003` 由 `[Diagnostics.Debugger]::Break()`
  在一次性子进程内真实触发。
- **性能基线**：`Bench-Perf.ps1`（顺序 / 4K 随机 / 冷热启动 / 内存）+ `-Action Gate` 门禁；
  报告归档 `docs/bench/2026-09-07-portable.md`（加 `-portable` 后缀，避免与 `tools/bench.cjs`
  的前端基线同名互相覆盖）。
- **验收门禁**：`Accept-Gate.ps1` 14 项（主计划 1.3 + 扩充 28 的 7 项全部纳入并扩展）。
  状态只有三种来源：自动脚本实测 / 人工实测录入 / 明确 `todo`。

### 交付与运维（第 12 章）

- **四阶段编排**：`Deploy-To-USB.ps1 -Action Preflight/Stage1..4/Verify/All`，复用 AI-1 的
  `Create-VHDX.ps1` 与 `Test-VM.ps1`，不复制造盘逻辑。
- **运维**：`Maintenance.ps1 -Action Status/Optimize/Backup/Restore/Schedule/Tune`，
  月度 `Optimize-VHD`、每日 `User.vhdx` 备份保留 3 份、一键还原、计划任务、调优清单。
- **联调**：`AI-Integration.ps1` 核验 AI1-5 交付物齐套性；AI-1/AI-2 尚未交付时如实标红，不代做。

### 安全加固（写进代码，不只是文档承诺）

- 危险场景（拔盘 / 宿主蓝屏 / 虚拟机内删 C 盘 / 驱动回退）标 `manual`，脚本**只出步骤卡不代为执行**；
  自检会把「`dangerous=true` 却是 `auto`」判为失败。
- 填盘演练默认 dry-run，真写需 `-AllowFill` 且受 512MB 上限 + `finally` 自动清理保护。
- 看门狗演练只杀脚本自己启动的一次性子进程，不枚举用户进程。
- 目标盘安全闸：拒绝宿主系统盘、拒绝固定磁盘、拒绝非法文件系统。
- 上盘用 `robocopy /E` 而非 `/MIR`，不镜像删除目标盘已有文件。
- 一键还原先把旧 `User.vhdx` 改名保留再覆盖。

### 验证接线

- 本会话的 GitHub App 令牌无 `workflows` 权限，改不了 `.github/workflows/ci.yml`
  （推送被远端拒绝）。但现有 CI 的 `frontend` 作业本来就在 `windows-latest` 上跑 `npm test`，
  于是把自检挂进 vitest：`portable/AI5/__tests__/portable.test.ts`
  —— `win32` 上真调 `pwsh`/`powershell` 执行 `Run-PortableTests.ps1`，非 Windows 明确 skip。
  **不需要新增 CI 作业即可在真 PowerShell 上验证。**
- `Run-PortableTests.ps1` 三段：官方 AST 解析全部 `.ps1`（含 AI-4 的 9 个）+ 数据不变量
  + 只读动作执行与一个负向用例（`Verify` 指向宿主系统盘必须判失败）。

### 验证结果（如实）

- 本地可跑的全部通过：21 个 `.ps1` 结构完整；21 项数据不变量；21 个脚本的函数调用 /
  `-Action` 名 / dot-source 路径交叉引用一致（含 143 处文档引用）；
  仓库自有检查 `tsc --noEmit` 通过、vitest 231 全绿、`tools/audit.cjs` 通过。
- 断言经过反向验证：向 `Data/*.json` 注入 4 个真实缺陷（删条目 / 假勾 pass / 危险场景改 auto /
  改 6s 预算）后，6 条断言如期失败；恢复后重新全绿。
- **本会话未在 PowerShell 上执行过任何脚本**：沙箱为 Linux，PowerShell 二进制下载域名被网络策略
  阻断（仅 npm/PyPI 可达）。PowerShell 语法与运行时由 PR 上的 `frontend` 作业在 windows-latest 验证。
- 真机验收 14 项全部保持 ⬜：无 Windows 宿主、无 1TB 目标盘、无 5 台测试机。

## [Unreleased] — 1.0sno9u.vxe（2026-09-07 便携系统 AI-1 存储核：主计划第 3+9 章）

> 多 AI 并行分工（`docs/PORTABLE_AI_SPLIT_PLAN.md`）的存储核批次；
> 详细实现说明见 `docs/AI1-存储.md`，压测口径见 `portable/AI1/Bench.md`。

### portable/AI1 存储脚本（5 支）

- **`Create-VHDX.ps1`**：1TB 1000MB/s 定版造盘 —— 固定 150GB VHDX + GPT + NTFS 64KB 簇 +
  4K 对齐校验 + Data 七目录 + 离线 CompactOS 与注册表调优（关 SysMain/WSearch/DiagTrack、
  开 LongPaths、关 LastAccess）；`-Sparse` 稀疏固定盘、`-Chain` 三级差分链（Base→Apps→User）；
- **`Tune-Guest.ps1`**：系统内一次性调优，逐项回读实测值 —— CompactOS always、TRIM、
  关休眠、待机/硬盘超时置 0、WinSxS `ResetBase`、RAM 盘缓存契约 `ramcache.json`；
- **`Link-DataApps.ps1`**：3.2 读写分离 —— 按 `linkmap.json` 建/校验/搬迁/卸载符号链接，
  幂等；探测 Data 卷文件系统，exFAT 自动回退 SymbolicLink（Junction 需 NTFS 目标卷）；
- **`Bench-Storage.ps1`**：9.1 选盘判定 —— 顺序写（WriteThrough 直写）/ 顺序读 /
  4K 随机（IOPS）/ 簇大小 / 4K 对齐 / TRIM / VHDX 膨胀率与碎片，一键出 Markdown 报告；
- **`Maintain-VHDX.ps1`**：每月维护 —— `Optimize-VHD -Mode Full` + 宿主卷 ReTrim +
  碎片率 <5% 体检 + 可选 `Merge-VHD` 扁平化。

### 复核发现并修正

- 分工表容量口径算错：1TB 实得 929.9GiB，「150GB + 800GB」超 20.1GiB → 定版 **Data 780GB**；
- 原型脚本 `-Filter "*.wim"` 会先命中 `sources\boot.wim`（字母序在前）→ 改 `install.wim`；
- 「回收后 <20GB 实占」对普通固定盘不成立（实占恒 = 虚拟大小）→ 新增 `-Sparse` 路径；
- 扩充 20.1 脚本 8 处修正：引号传参 / 返回码校验 / 分区复用 / `boot.wim` 误选 /
  4K 对齐 / 簇大小回读 / `Index` 参数化 / try-finally 卸载。

### 验证

- CI（windows-latest）`frontend` + `backend` 全绿；
- PowerShell 脚本经词法级结构检查（括号/引号/here-string/`$()` 配对）5/5 OK，
  基线：仓库既有 4 支 `.ps1` 同样 OK（确认无误报）；
- **如实边界**：沙箱为 Linux 且 `pwsh` 不可安装，PowerShell 解析器与 Windows cmdlet
  （`New-VHD`/`dism`/`fsutil`/`powercfg`）一次未执行，真机数据待 1TB 盘到货补入
  `Bench.md` 第 4 节（未编造任何压测数字）。

## [Unreleased] — 1.0sno9u.vxe（2026-09-06 实机反馈会话 2：原生图标 + 壁纸预览回退）

> 实机使用反馈第二轮（H1 家用机）。

### 环境内图标与 Windows 一致

- **exe 内嵌图标提取**：`icon_dataurl` 此前只认 `.ico/.png` 文件，第三方 exe
  登记项永远是通用占位图标（实机截图：Blender 显示绿块）。现链路
  SHGetFileInfoW(HICON) → GetIconInfo → GetDIBits(32bpp BGRA→RGBA，
  无 alpha 图标兜底不透明) → **手写 PNG 编码器**（CRC32 + stored deflate +
  Adler32，零新依赖）。前端 `fillNativeIcons` 流程不变，登记即自动补真图标；
- 顺手修复 launcher.rs 中 `"runas\x00"` 裸 NUL 字节导致的二进制误判（改为
  转义写法，文件恢复文本可维护）。

### scene 壁纸本地打开不再报错

- **预览图回退扫描**：scene 项目常缺 preview.jpg（实机三连报错）。现回退扫描
  项目目录取最大图片（WE scene 的 textures 贴图几乎必有图片，跳过 <20KB
  小图标，递归限 4000 项防拖慢）——绝大多数 scene 项目可直接本地静态渲染；
  真正一张图都没有的项目仍如实报错（不伪造）。

### 验证

- `cargo test` 79 全绿（65+7+7，含新增 PNG 编码器与预览回退 2 个单测）；
- `tsc --noEmit` 无错误；`tools/audit.cjs` 233/237 全对齐。

## [Unreleased] — 1.0sno9u.vxe（2026-09-06 实机反馈会话：壁纸三修复 + 小项三清）

> 实机使用反馈修复（H1 家用机），壁纸路径全部本地化收口。

### 壁纸实机修复（H1 反馈）

- **scene/应用型壁纸全部本地打开**：点击不再调 `wp_engine_open` 拉起 Wallpaper
  Engine 本体 + 隐藏窗口，改为项目 preview.jpg 本地图片壁纸渲染（无预览图如实
  报错）；三种语言词条同步（wpEngineLocal / wpEngineNoPreview）；
- **「系统桌面（Wallpaper Engine）」模式下线**：该模式渲染纯黑并让位隐藏，WE 未
  接管时整屏黑屏——右键菜单与设置页下拉均移除入口；历史设置残留 system 值由
  WallpaperLayer 兜底落入图片/媒体渲染路径，不再黑屏；
- **`wp_engine_open` 整链删除**（Rust 命令 + generate_handler + ipc.ts）：宿主侧
  不再启动 wallpaper64.exe，攻击面收窄。

### 快捷键双开防护

- 新增 `shell/single_instance.rs` 单实例守卫（命名互斥体 OpenMutexW，零新依赖）：
  双开时第二实例弹系统消息框并退出——此前双开导致全局快捷键整表注册全部失败
  （"注册失败（被系统或其他软件占用）"全量弹窗）的根因即在此。

### 小项三清

- **B-22 upstream 提示**：GitStatusView 新增 `has_upstream`，Git 面板在无
  upstream 时提示「在终端 git push -u 后显示 ↑↓」；
- **audit.cjs 双向 diff**：unusedBackend 此前已计算但从未输出——现打印
  `UNUSED BACKEND` 段（首跑即抓出 4 个历史冗余命令待人工确认）；
- **B-33 `--revoke-list` GUI 入口核实已存在**（设置页「吊销清单」按钮），清单项划掉。

### 验证

- `cargo test` 77 全绿（63+7+7）；`tsc --noEmit` 无错误；
- `tools/audit.cjs`：233 invoke 全有后端、237 命令全注册、无新增反向冗余。

## [Unreleased] — 1.0sno9u.vxe（2026-09-06 会话，M2…M9 + 辅助批次代码面全部收口）

> 状态仪表见 MASTER-PLAN 第 28 节：**33✅ / 1🟡（B-11 真机矩阵）/ 6☐（B-30 三宿主发版门禁 + 1.x 扩展生态）**。

### M2 容器与 8TB（B-12…B-17 + B-31）

- **Uxv 单文件容器**：自研格式 `[SuperBlock][追加区 chunk 记录][Footer 双副本]`——
  4MiB 定长切分 + BLAKE3 内容寻址去重 + 引用计数；B+ 树索引（oracle 差分测试）；
  read_range/stream 大文件通道；热 chunk 随机读实测远低于 20ms 口径；
- **掉电安全**：journal 事务（JNL1 魔数 + COMMIT）+ SuperBlock 指针原子发布 +
  重放撕裂截断——掉电注入 100 轮 0 数据丢失；
- **Schema 迁移协议**：惰性探测 / pre-migrate 快照 / 逐级升版 / 失败回滚 / 只升不降；
- **压缩与加密**：LZ4/Zstd-19 分级（不可压缩回 RAW）+ Argon2id/XChaCha20-Poly1305
  全容器加密（chunk/索引/journal 三层）+ 解码 LRU；
- **多卷条带与 GC**：主卷元数据 + 数据卷条带轮转（ChunkLoc.volume，schema v2 走迁移协议）；
  GC 标记/走查/最差卷单卷压实（停顿 <100ms）；
- **VHDX 快速档与迁移向导**：管理员/Mount-VHD 探测不可用如实降级 Uxv；
  数据目录→容器逐文件 BLAKE3 校验、失败源目录原样保留；
- **容器仪表与冷热分层**：水位线/写放大/规模统计 + freeze/unfreeze（冷层键空间分离）。

### M4 浏览器矩阵（B-18/B-19）

- 五款浏览器检测（App Paths + 路径兜底）+ 家族便携模板（--user-data-dir / -profile
  全指向容器）+ Profile 管理器（新建/克隆/焚毁删除）+ 设置页「浏览器」标签；
- 首次导入（书签 HTML / 密码 CSV 复制进容器，绝不读宿主浏览器运行数据）+
  任务栏按 profile 分组（pid 存活探测）。

### M5 大型编码体系（B-20…B-23）

- **VS Code Portable 一键部署**（离线 zip 降级 / data 模式全容器化 / 幂等登记）+
  嵌入启动复用整条 embed 通道；
- **工具链**：Python(embeddable)/Go(zip)/Rust(rustup 容器化 CARGO_HOME) 三通道 +
  PATH 统一注入（spawn_profiled 单点，冻结顺序）；
- **Git 只读面板**（git2：状态/分支/历史，仓库限容器内）+ **SSH 金库代理**
  （ed25519 生成封存 vault/ssh + GIT_SSH_COMMAND 注入，私钥不落宿主）；
- **并行全库搜索**（8 路线程池，跳依赖/二进制/>8MB）+ 大文件分块查看器 +
  PVCCE→VS Code `--goto` 行级跳转。

### M6 子环境系统（B-24…B-26）

- **环境档**：envs.json + envs/<id>/ 剖面 + `{envhome}` 执行档隔离（browsers/code/ai
  全部接入）——凭据与登录态按环境隔离；切换编排 4 步（快照/翻转/应用/重载）；
- **嵌套实例**（深度 ≤3）：独立数据根（VARIABLE_DATA_ROOT）+ 白名单继承接口 +
  嵌套回退独立窗口；**环境克隆**（剖面复制 + 设置快照）。

### M7 应用生态 2.0（B-27）

- 可移植性评估向导（目录可写/卸载注册表/本地配置三类启发式 → 绿黄红建议卡）+
  搬迁执行器（整拷 apps/ + portable.reg 快照 + 登记）；
- **Steam 库扫描**（libraryfolders.vdf + appmanifest）与 steam:// 协议直通 +
  商店 AUMID 启动 + 文件关联表（无关联宿主兜底打开）。

### M8 网络层（B-28）

- **环回 HTTP 代理**：CONNECT 隧道域名裁决 / 绝对式改写 / 默认拒绝（默认零出站落地）；
  受管进程经执行档自动注入 HTTP(S)_PROXY；
- 域名白名单规则库（子域通配 + 发起执行档记录）+ **kill-switch**（覆盖一切）+
  流量仪表（放行/拒绝/字节）；直连逃逸计数+标记（硬断网属防火墙，如实边界）。

### M9 安全分析工作台（B-29）

- 纯 Rust 手写 **PE 静态解析**（节表熵 >7.0 加壳标记 / 导入表 / 证书签名 /
  14 项可疑 API 命中 / 字符串提取）+ **iced-x86 入口反汇编只读查看器**；
- Windows Sandbox 探测与 .wsb 生成（样本映射只读+断网）+ 安全子环境预设
  （白名单清空 + kill-switch）+ Markdown 报告导出（不含样本字节）；
- 铁律：样本绝不在宿主执行。

### 辅助批次（B-31…B-35）

- **B-31** 容器 Schema 迁移协议（快照/升版/回滚，v1→v2 实走）；
- **B-32** OOBE 首次初始化向导（介质体检/口令建卷/三模板/60 秒导览）；
- **B-33** 应急能力包：CLI 四命令（--export-rescue / --repair / --force-raster /
  --revoke-list，无需 GUI 抢救容器）+ 紧急吊销清单导出 + 软件渲染开关（--force-raster
  → safeMode）；修复 residue_scan 前后端命令名不匹配的运行时必炸缺陷；
- **B-34** 诊断包导出（四节脱敏 Markdown，不含数据/凭据）+ 演示胶囊模板；
- **B-35** 发布合规：docs/false-positive.md（≤48h 闭环流程）+ docs/licenses.md
  许可清单归档。

### 验证基线

- cargo test --workspace **129**（container 52 含掉电注入 100 轮/代理端到端/手工 PE 构造）；
- vitest **218** / tsc 零错 / audit IPC+ i18n zh+en 全覆盖 / vite build 全绿；
- 历次自检归档：`docs/selfcheck/2026-09-06.md`（六个批次附录）。


### 已落地（2026-09-06 早段会话，M0 收口 + M1/M3 起步）


### 已落地（2026-09-06 会话，M0 收口 + M2 起步）

- **B-1 性能基准基线**：`tools/bench.cjs` 四项基准（冷启动/万文件索引/VWM 打开/
  内存水位，VWM 项如实 SKIPPED 待 GUI 插桩），首份基线归档 `docs/bench/2026-09-06.md`；
  `--check` 支持 >10% 回归门禁；
- **B-2 容器层骨架**：`src-tauri` 升级为 Cargo workspace，新建 `container` crate
  ——冻结 `StorageBackend` trait（蓝图 7.1 契约）+ `DirBackend`（原子写/路径沙箱/
  快照恢复）+ 8 项测试桩；
- 工程卫生：新增 `.gitattributes`（LF 归一化，消除 CRLF 幻影 diff）、清理未使用
  截图；本机补齐 Rust 工具链（rustup stable-msvc）。
- **M1 隔离执行档与凭据封存收口（B-3…B-6）**：受管进程一律经 `spawn_profiled`
  启动，HOME/凭据目录等环境变量强制重定向进容器（端到端测试证明）；apps.json v2
  （旧文件平滑升级）；五套重定向模板（Claude Code/Codex/ZCode/Git/Node）+ 设置页
  「执行档」标签（套用/编辑/干跑验证）；残留扫描器（会话差集，宿主零残留可验证）。
  详见 `docs/selfcheck/2026-09-06.md`。
- **M3 终端与云 AI 矩阵（B-7…B-11）**：AI Hub 面板（任务栏入口 + 三态卡片）；
  便携 Windows Terminal 经执行档+嵌入通道成为环境内终端（V1）；容器内 Node 运行时
  与 `ai install`（真实字节进度事件流，出站逐域授权）；Vault 2.0 账号身份库
  （Token 只存保险箱加密区）+ 多账号切换（配置目录 @label 隔离）；`ai verify`
  凭据零落宿主断言（真机矩阵待点验）。

### 已落地（2026-09-05 会话）

- **embed 嵌入子系统加固**：窗口捕获改为启动进程树匹配（Toolhelp32，覆盖
  Wallpaper Engine 等启动器型软件）、等待延长至 30s、超时绝不终止应用进程；
- **启动通道安全化**：`.lnk` 改 ShellExecuteExW（去 cmd 字符串拼接）、管理员运行
  改 ShellExecuteW runas（去 PowerShell 拼接）——仓库内已无 shell 字符串拼接；
- **按需求移除用户态守护子系统**（崩溃收尸/假死检测/Job Object 配额）及其前端
  事件面与文案；嵌入语义更新为"视觉与窗口层隔离，非沙箱"（如实边界）；
- **自检体系落地**：修复 audit.cjs 三处解析缺陷（子目录漏扫/属性断行/同行双键），
  补齐 25 中文 + 66 英文缺失 i18n 键，selfcheck 报告归档制度建立；
- **文档体系闭合**：BLUEPRINT（架构+契约+扩展协议+边界）、MASTER-PLAN（里程碑+
  状态仪表+闭合定义）、README（产品手册+文档索引）、CHANGELOG（本文件）；
- **CI**：GitHub Actions 五层验证栈（typecheck/vitest/cargo test/audit/build），
  push 即验证。

### 计划（按 MASTER-PLAN 里程碑）

- M1 隔离执行档与凭据封存 → M3 云端 AI 矩阵（Claude Code / Codex / ZCode 开箱）
  → M2 8TB 容器 → M4 浏览器矩阵 → M5 大型编码体系 → M6 子环境套娃
  → M7-M9 生态/网络层/安全分析工作台 → B-36..B-40 扩展生态。

## [1.0.0] — 2026-09-05

首个公开发布版本（详情见 README 各章）：

- 私人桌面环境：启动仪式（真实加载进度）、全屏壁纸层（静态/轮播/视频/网页/
  Wallpaper Engine 全类型接入）、桌面图标与文件架、Win11 风格任务栏与开始菜单、
  通知中心/快捷面板/硬件面板、回收站与文件管理器（VWM 内嵌）；
- VWM 虚拟窗口管理器：Z 序/贴靠/多开/最小化保活/几何持久化/红绿灯；
- 第三方软件：开始菜单扫描、便携化三级、原生图标提取、进程深度检测、
  环境内嵌入（进程树窗口捕获）；
- 四大空间：写作库（TipTap 富文本+xref 跨空间引用）、思维导图（八形节点/
  三线型/小地图）、项目分析 PVCCE（七级下钻/大白话词典/意图写回）、
  命运推演 FTPE（确定性种子/行为树）；
- 平台能力：全局快捷键（kbdhook 双 Esc 切环境、Del+Backspace 退出）、
  IM 未读提醒（只读窗口标题）、Wi-Fi/蓝牙控制、U 盘便携与拔出保护、
  保险箱/焚毁/备份恢复、三语（简/繁/英）+ 拼音搜索；
- 视觉：原生 WebGL 星空（Worker 管线）与 WebGL2 极光（Ray-Marching）；
- 发布物：NSIS / MSI / 便携目录；测试：Vitest 218 + cargo test 36 + 静态审计。
