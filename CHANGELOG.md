# Changelog

本文件记录面向用户与协作者的显著变更。批次级细节见 `project_memory.md`；
架构与计划见 `docs/BLUEPRINT-1.0sno9u.vxe.md` 与 `docs/MASTER-PLAN-1.0sno9u.vxe.md`。

## [Unreleased] — 1.0sno9u.vxe

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
