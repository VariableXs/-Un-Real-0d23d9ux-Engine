# Changelog

本文件记录面向用户与协作者的显著变更。批次级细节见 `project_memory.md`；
架构与计划见 `docs/BLUEPRINT-1.0sno9u.vxe.md` 与 `docs/MASTER-PLAN-1.0sno9u.vxe.md`。

## [Unreleased] — 1.0sno9u.vxe

## [Unreleased] — 1.0sno9u.vxe（2026-09-07 便携系统 AI-5 交付核：主计划第 11+12 章）

> 多 AI 并行拆分（`docs/PORTABLE_AI_SPLIT_PLAN.md`）的 AI-5 交付核落地。
> AI-3（第 6+7 章）、AI-4（第 8+10 章）此前已合入；本次补上测试验收与交付运维。
> 实现进度：主计划 12 章中 10 章已落地（1/2/3/6/7/8/9/10/11/12），仅余第 4/5 章（AI-2 隔离核）。

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
