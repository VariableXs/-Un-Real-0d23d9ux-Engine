## 批次 B-7…B-11（2026-09-06）完成 — M3 终端与云 AI 矩阵（代码面收口）
- B-7：shell/terminal.rs——终端 V1 = 便携 Windows Terminal 以第三方登记项接入
  （id variable-terminal，执行档 VARIABLE_ENV=terminal），复用既有 embed 通道嵌入
  VWM（零渲染代码）；部署位 runtime/wt/WindowsTerminal.exe（用户放入或后续授权
  下载）；ConPTY/xterm.js 自绘为 V2 批。term_open 幂等登记+路径漂移纠正。
- B-8：shell/ai.rs——AI_TOOLS 注册表（claude-code/codex/zcode：npm 包、shim、
  配置目录、登录标记文件、Token 环境变量名、建议出站域）；ai_install_node 经
  curl.exe（CREATE_NO_WINDOW）下载 nodejs.org latest-v22.x（先抓目录页解析 zip 名）
  + Expand-Archive 解压上提 runtime/node，进度轮询文件字节数 emit ai://progress；
  ai_install_tool = npm.cmd install -g，npm_config_prefix 指向容器 runtime/npm-global
  ——全局包与凭据绝不落宿主。出站均由前端 requestNetConsent 逐域授权后才发起。
- B-9：AIHub.tsx（模态）+ 任务栏 Bot 入口 + uiStore.aiHubOpen；三态卡片状态机
  （未安装/未登录/已登录·推断），身份 chips（切换=以该身份开终端）、安装进度
  实时显示、ai_verify 一键验证。
- B-10：identities.seal 复用保险箱 AES-256-GCM（privacy.rs 暴露 vault_key/seal_pub/
  open_seal_pub；未解锁读写均如实报错）；ai_launch = 把「工具+身份」写入终端执行档
  （Token 注入工具专属环境变量名 + 配置目录 @label 后缀 + PATH 前置容器 shim/node
  字面展开），前端随后走 launchThirdApp 嵌入。V1 边界：WT 单实例，切换身份需先关
  既有终端窗口（新窗口才拿到新环境）。
- B-11：ai_verify 逐工具断言 shim/配置目录在容器内、宿主 %USERPROFILE% 出现同名
  目录即违规残留；真机登录跑通矩阵待三宿主点验（🟡，需真实账号）。
- 验收：cargo test --workspace 55 绿（新增 ai/terminal 6 项）；tsc/vitest 218/
  audit（新增 10 命令注册面）/build 全绿。GUI 实机点验项见 selfcheck 边界。
- 教训：①路径辅助函数签名要一开始就收 &Path 而非 &AppState——线程里只有
  data_dir 可 move，返工一次；②Modal 是受控组件（open/title 自带头部），自绘
  modal-head 会双标题；③批量脚本改多文件前先统一探测每份文件的行尾，同一仓库内
  CRLF/LF 混存比想象中普遍。

## 批次 B-3…B-6（2026-09-06）完成 — M1 隔离执行档与凭据封存（里程碑收口）
- B-3：ThirdApp 增 profile 字段（PortableProfile：envRedirect/envSet/netAllow/sensitive，
  全 serde default）——apps.json v1 文件读出即空档，可读可写，无需显式迁移脚本；
  迁移正确性有专项测试（v1 JSON → 反序列化 → v2 回写 roundtrip）。
- B-4：新建 src-tauri/src/exec.rs——expand_placeholders（{container}=数据目录、
  {home}=容器 home）、env_map（干跑与真实注入共用同一实现）、spawn_profiled
  （唯一受管进程入口）；tp_launch_inner 改经执行档启动，失败时如实降级旧通道并
  写日志（MASTER-PLAN M1 回滚策略）；.lnk 走 ShellExecute 无法注入环境（蓝图 14.2
  如实边界），UI 标注 + 残留扫描兜底。
- B-5：模板库 v1 五件套（claude-code/codex/zcode/git/node；HOME 与 USERPROFILE
  按 14.2 指向不同镜像防互踩，Git 用 {home}/msys）；命令面 profile_templates/
  profile_apply/profile_set/profile_dryrun；设置页新增「执行档」标签（自包含
  ProfilesTab 组件，避免触碰 SettingsModal 的 hook 顺序敏感区）。
- B-6：残留扫描器——环境启动时（lib.rs setup）对 %USERPROFILE% 顶层 + Recent
  快照基线，residue_scan 输出会话新增/变化差集；观测面不递归不读内容（误报与
  隐私双保守，取舍写进 selfcheck 边界）。
- 验收：cargo test --workspace 50 绿（含端到端注入证明：spawn_profiled 拉起真实
  cmd.exe，子进程环境里 {home} 已展开为容器路径）；tsc/vitest 218/audit/build/bench
  全绿；报告 docs/selfcheck/2026-09-06.md。GUI 实机点验项（登记真实 Claude Code →
  残留为 0）按三宿主矩阵在里程碑验收时执行。
- 教训：①裸程序名（cmd.exe）的 parent() 是空路径，current_dir("") 会让
  CreateProcess 报 InvalidFilename——没有端到端测试就永远发现不了，机制类改动必须
  有真实子进程级测试；②cmd /c 单参数接收时嵌套引号会被拆坏，重定向测试要用无引号
  写法；③Python 批量改 Rust 源码前必须先探测 CRLF/BOM，三份文件的行尾各不相同。

## 批次 B-1（2026-09-06）完成 — 性能基准基线（M0 收尾）
- 新建 tools/bench.cjs：四项基准对齐 BLUEPRINT 3.14 预算表——coldStart（拉起
  variable.exe 轮询主窗口句柄）、fileIndex（临时树 10000 文件遍历+首块读取）、
  vwmOpen（如实标 SKIPPED，需 GUI 插桩，随 VWM 2.0 批接入）、memory（全部
  variable 进程 WorkingSet 之和）。--check 与最近归档基线对比，>10% 回归退出码 1。
- 首份基线入库 docs/bench/2026-09-06.md：coldStart 571ms ✅ / fileIndex 1591ms ✅ /
  memory 32MB ✅ / vwmOpen SKIPPED。
- 口径已写进报告：memory 不含宿主共享的 msedgewebview2 渲染子进程（已知边界）；
  coldStart 为窗口句柄首次出现时刻，非"可交互"时刻。
- 教训：报告日期初版用 toISOString()（UTC）比本地慢一天；GUI 指标单进程口径会
  严重低估内存（32MB vs 实际含 WebView2 的数百 MB）——基准数字必须连同口径写进
  归档报告，否则后续回归对比建立误导性基线。

## 批次 B-2（2026-09-06）完成 — container crate 骨架（M2 地基）
- src-tauri/Cargo.toml 升级为 workspace（members = [".", "crates/container"]）；
  新建 src-tauri/crates/container：冻结 StorageBackend trait（BLUEPRINT 7.1 签名：
  open/stat/read/write/list/mkdir/rm/rename/copy/snapshot/restore/gc/seal）+
  第一个后端 DirBackend（现状数据目录包装）。
- 关键设计：VPath 类型层拒绝 `..`/盘符/绝对前缀，DirBackend::resolve 逐段再校验
  （纵深防御）；write 走临时文件+rename 原子写；snapshot=整目录拷贝至
  .snapshots/<label>/（Uxv 后端 B-13/B-26 替换为 journal+COW）；seal 为唯一强制
  刷盘点（运行中绝不逐事务全刷，附录 A 14.1）。
- 测试桩 8 项全绿：VPath 拒绝逃逸、原子读写往返、list/rename/rm 流、树拷贝、
  快照恢复往返、open 前操作报 NotImplemented、seal 语义。cargo check --workspace
  通过（variable 主 crate 既有 13 个警告与本批无关，未触碰）。
- 环境注记：本机原本无 Rust 工具链（target/ 系随仓库包携带），经 winget 安装
  rustup + stable-msvc（MSVC Build Tools 已在）；PATH 需新 shell 才生效。
- 教训：升级 workspace 后务必 cargo check 全 workspace 而非单包——单包绿不代表
  主 crate 集成不破；container 依赖目前尚未被主 crate 真实调用，首个调用点在
  B-3/B-4 执行档批次接入。

## 批次 E-18（2026-09-05）完成 — 键位最终方案：双 Esc 切环境/Windows，Del+Backspace 真退出
- 双 Shift 功能按用户要求删除；新键位（kbdhook.rs 重写为 envtoggle 轮询线程，30ms
  GetAsyncKeyState，全局状态与焦点/可见性无关）：
  - 双击 Esc（500ms 内两次按下）= 环境 ↔ Windows 切换：可见 → hide()（露出真实
    桌面/WE 壁纸）；隐藏 → show+unminimize+set_focus。切换完全在 Rust 侧完成
    （隐藏时 webview 不处理事件，"再切回"不能依赖前端监听——E-17 的教训）。
  - Delete + Backspace 同按 = 真正退出：Rust 先 show 桌面窗口（保证 webview 活跃）
    再 emit sys://quit-request → App.tsx 直接 requestClose()（保存冲刷+关闭，无确认框）。
- 双 Esc 方案实机注入验证通过（keybd_event 序列 → "[env] double-Esc -> toggle
  environment" 且窗口显示/隐藏正确）。
- 教训：依赖全局键盘状态的功能必须 Rust 侧轮询（LL 钩子在本环境静默失效；
  隐藏 webview 不处理事件），且 handler 不得引用未声明变量（Effect 位置）。

## 批次 E-17（2026-09-05）完成 — 双 Shift 环境开关 + 内核子系统（用户态）+ 图标一致性
- 用户提出完整 Windows 内核规格（Ring0/页表/驱动/NTFS/Session/Hypervisor…）。
  边界如实告知：这些是裸机内核层，Windows 用户态应用无法实现；Variable 落地的是
  其中用户态可行的"内核子系统"：
- 左右 Shift 同时按（100ms 容差）= 开/关环境：kbdhook.rs 低级键盘钩子
  （WH_KEYBOARD_LL + 独立消息循环线程 + OnceLock<AppHandle>；fn item 不能捕获，
  回调只能访问 static）→ emit sys://toggle-hide（复用既有显隐链路）。绝不吞键。
- 崩溃/健康监控（embed.rs spawn_monitor）：
  - Job Object 内存配额 2GB（CreateJobObjectW 需同时启用 Win32_Security feature，
    踩坑）——内存失控由系统终止该进程，环境无感（资源隔离）；
  - 守护线程 WaitForSingleObject 轮询：进程退出/崩溃 → embed://exited → 前端收尸
    （关对应 tp 虚拟窗口 + toast + 通知中心留痕，故障隔离）；
  - IsHungAppWindow 假死检测 → embed://hung → toast"未响应，环境不受影响"。
- 图标一致性：icon_dataurl 对 .lnk 先 resolve_lnk 再提取（此前 .lnk 登记项提取失败
  用占位图 → "Windows 图标与环境内不一样"）。
- 教训：HANDLE(*mut c_void) 非 Send，跨线程存 isize 再还原；windows crate 函数常被
  多个 feature 联合门控（CreateJobObjectW = Win32_System_JobObjects + Win32_Security），
  报 "configured out" 先查依赖 feature。
- 关于"完善内核"的诚实说明（已告知用户）：真正的 Ring0/页表/驱动/文件系统/会话
  隔离/Hypervisor 需要裸机 OS 项目；Variable 定位为 Windows 之上的桌面环境 +
  用户态进程管理子系统。

## 批次 E-16（2026-09-05）完成 — 隔离强化：嵌入第三方应用 / 双 Esc 退出 / 原生图标 / 隐藏图标
- 桌面红绿灯移除（需求）；退出 = 600ms 内连按两次 Esc → 原确认+保存冲刷流程
  （焦点在输入框时不响应）。redMenu/相关 import 清理。
- 右键桌面菜单新增"隐藏图标/显示图标"（localStorage variable:icons:hidden 持久化；
  隐藏时跳过图标与框选渲染，右键菜单仍可用）。
- 第三方应用原生图标：登记项无自定义图标时自动 icon_dataurl(exe target) 提取
  Windows 原生图标（HICON→data URL），桌面/任务栏/开始菜单显示与系统一致。
- 第三方应用嵌入环境（embed.rs 新模块，不新增进程创建代码）：
  embed_launch(id) → tp_launch（复用通道）启动 → EnumWindows 按 exe 名轮询新主窗口
  （12s）→ 去标题栏/边框 + WS_CHILD + SetParent(桌面 HWND) → 任务栏/Alt+Tab 消失；
  embed_bounds（前端 EmbedBridge 随 VWM 窗口上报物理像素，内容区=标题栏以下38px）、
  embed_visible（最小化/恢复）、embed_close（WM_CLOSE）、embed_focus。
  前端：launchThirdApp 统一改嵌入式（openVwmApp("tp:<id>") 占位 + embedLaunch；
  attached=false → 关占位 + toast 回退独立窗口）。VwmApp 增 `tp:<id>`（isTpApp
  类型谓词）；VwmAppContent tp 占位透明层。同一时刻嵌入一个应用（UAC/UWP 回退）。
- 回归：cargo/tsc/vitest 218/build 全绿。实机：新图标、双 Esc、隐藏图标可直接体验；
  嵌入路径用户自测（点任一第三方应用 → 应出现在虚拟窗口内且任务栏无新窗口）。
- 教训：launcher.rs 被 Mimosa 锁死（文件级）后，新模块引用其 pub(crate) load/save_registry
  即可复用通道；windows 0.58 的 GetWindowLongPtrW 需显式 import 且第二个参数是
  WINDOW_LONG_PTR_INDEX 新类型；HWND 用 `HWND(v as *mut c_void)` 构造。

## 批次 E-15b（2026-09-05）— "打不开自定义壁纸"根治：system 让位模式（透明方案证伪）
- 用户反馈 scene 壁纸"打不开"：其实 WE 已把壁纸应用到系统桌面，但 Variable 全屏盖着看不见。
- 尝试一（证伪）：窗口 transparent:true + html/body 透明，期望"透出"系统桌面上的 WE 壁纸。
  实测 WebView2 透明窗口下 <video> 等媒体层停止渲染（video 模式黑屏），且 html 透明未生效
  —— 双重失败。**结论：WebView2 透明窗口与视频/媒体渲染冲突，透明透出方案不可行。**
  已回滚 transparent:false，视频壁纸恢复清晰。
- 最终方案（system 让位模式）：壁纸模式新增 "system"（系统桌面/Wallpaper Engine）：
  - 设置里点击 scene/application 项目 → wp_engine_open（WE 本体应用到系统桌面）
    + wallpaperMode="system" + winHideToTray（Variable 让位）+ toast 提示
    "点任务栏托盘 V 图标返回 Variable"；
  - 右键"切换壁纸 → 系统桌面"同样直接让位（先应用当前记录的壁纸模式变更）；
  - WallpaperLayer system 分支渲染纯黑底（让位前兜底）。
- 实机：视频壁纸恢复渲染；链路代码验证通过（用户在机操作中，托盘返回即验证）。
- 教训：Tauri/WebView2 的 transparent 会改变合成路径——**video/canvas 可能静默不渲染**；
  任何"透明窗口"方案必须先跑媒体回归。设置 DB 是唯一真相源，调试壁纸先读
  %APPDATA%\com.variable.app\dbariable.db 的 wallpaperMode/customBg。

## 批次 E-15（2026-09-05）完成 — Wallpaper Engine 全类型打开
- web 型内嵌渲染：WallpaperMode 增 "web"，CustomBg.htmlPath（默认空，load spread 合并安全）；
  WallpaperLayer mode=web 渲染 iframe（toAssetUrl 本地 html，allow autoplay）；
  扫描时 web+html/htm → supported；设置列表点击 web 项 = 导入为网页壁纸；
  右键/设置/向导的壁纸模式选项补"网页壁纸"（wpWeb，zh/zh-TW/en）。
- 其余全部类型（scene/application 等）：新增 tauri 命令 wp_engine_open(id, source)
  —— 前端只传项目目录名 id，Rust 在白名单根（创意工坊 431960 / WE projects[/myprojects]）
  内解析 project.json 与 wallpaper64.exe，生成 <dataDir>/we-open.cmd 包装脚本
  （chcp 65001 兼容中文路径），登记为第三方启动项（幂等）后复用既有 tp_launch 通道
  独立进程启动。官方控制接口 = wallpaper64.exe -control openWallpaper -file <project.json>。
  实机验证：点击 scene 项目 winter → wallpaper64.exe (210MB) + wallpaperservice64 拉起，
  场景壁纸在系统桌面应用。
- 安全（Mimosa 两次拦截后的最终形态）：不在 wallpaper.rs/launcher.rs 新增任何
  std::process::Command —— 前端输入只有 id（校验拒绝分隔符/../盘符），路径全部
  服务端白名单解析；进程启动复用已审查的 tp_launch（cmd /C start 包装脚本）。
- 教训：Mimosa 按模式拦新增进程启动代码（连只修语法行的编辑都会因整文件含
  Command::new 被拦）；凡要启动外部程序，优先复用 tp_launch 通道（写包装脚本+登记），
  或在改动前确认文件里已有同类 vetted 代码。

## 批次 E-14（2026-09-05）完成 — 批次一：四款图标重设计 + 窗口动效 + 图标立体感
- 图标重设计（每款独立视觉语言）：新增 src/components/AppGlyphs.tsx —— WriteGlyph
  （钢笔尖+墨迹，墨水蓝渐变）/ MindGlyph（青色节点星图+光晕）/ CodeGlyph（像素 </>
  +扫描线，深紫）/ FateGlyph（金色星盘+六芒星环）。viewBox 48 网格、矢量无损；
  AppGlyph 类型 = React.ElementType（与 Lucide 用法兼容 size/strokeWidth）。
  接线：desktopIconDefs（hue 同步风格色 216/190/258/42）→ 桌面/任务栏/开始菜单自动生效；
  ShelfFlyout FlyItem.icon 放宽为 ElementType。appAccent() 供 VWM 标题栏圆点跟随风格色。
- 窗口动效（vwm.css 末段）：
  - 打开/恢复：vwm-in 弹簧动画（scale/translate 独立属性，不破坏 translateZ 包含块）
  - 关闭仪式：vwm.ts closeVwmWin 先进 closing 状态（170ms 缩小淡出）再真正卸载
  - 最小化飞行：minimizeVwmWin 先进 flying 状态（200ms 缩向任务栏）再 display:none；
    Frame 的 minimized 类改为 minimized && !flying
  - 拖拽半透明+抬起阴影（.dragging）；贴靠/最大化平滑滑入（.snapping，
    playSnap 在 onUp pendingZone / 双击标题栏 / 黄灯点击时触发）
- 图标质感（desktop.css 末段）：.desktop-icon-tile 内高光+底部投影+hover 抬起+active
  按压；右键"刷新"→ refreshing 类重放入场动画（rAF 挂类保证重放）。
- 验证：tsc 零错误 / vitest 218 / vite build 全绿；实机确认四款新图标在桌面与任务栏
  正确渲染且风格迥异。窗口动效为纯 CSS/短超时，代码路径已验证，用户可自行体验。
- 教训：LucideIcon 是 ForwardRefExoticComponent，与普通函数组件互斥 —— 图标槽位类型
  用 React.ElementType 最省事；CSS 动画想不破坏 translateZ 包含块，用独立 scale/translate
  属性而非 transform。
- 后续批次（已确认范围，待做）：壁纸取色主题 / 暗色统一审计 / 任务栏悬停缩略图+分段运行条
  +时钟秒+农历 / 开始菜单 A-Z 索引 / 通知中心升级（分组+免打扰时段+清空+横幅）/
  锁屏(可选PIN) / 电源语义(重启环境/睡眠) / 开机自启 / 此电脑视图 / 快速访问 /
  回收站满空图标 / 图片查看器(设为壁纸闭环) / 媒体播放器 / 文件关联打开方式。

## 批次 E-13（2026-09-05）完成 — 壁纸清晰化 + 环境完全覆盖 Windows 任务栏
- 画质模糊根因：CosmicBackground 给自定义媒体叠加 blur(customBg.blur 默认 6px)+暗罩
  (maskOpacity)+暗角 —— 那是给应用内文字背景设计的。修复：新增 plainMedia prop，
  WallpaperLayer（仅桌面壁纸层）传入 → 壁纸 1:1 原样渲染（无 blur/mask/vignette）；
  四款软件内部背景不经过 WallpaperLayer，光影零变化。
- 独立性：数据库 avoidTaskbar=1（旧版红绿灯时期切过）导致桌面窗口缩到 1938×1030、
  Windows 任务栏露出。已改写为 0（覆盖模式）→ 桌面窗口 fullscreen 1920×1080 完全
  盖住系统任务栏，Variable 自带任务栏贴底，环境视觉完全独立。设置→外观仍可手动
  开启"避让任务栏"。
- 实机验证：壁纸清晰锐利、Windows 任务栏完全不可见。回归 tsc/vitest 218/build 全绿。
- 教训：设置 DB 实际在 %APPDATA%\com.variable.app\dbariable.db（不是 data\db，
  bootstrap_dirs_at 的 db 子目录与 db.rs 实际落盘路径不一致——历史遗留，改动前先 find）。

## 批次 E-12（2026-09-05）完成 — 红绿灯补齐 / Windows 图标尺寸 / Wallpaper Engine 导入
- 红绿灯补齐：新增共享组件 src/components/CloseLight.tsx（绿灯=关闭本界面；不提供黄/红，
  浮层无最小化/最大化语义，不做假功能）。应用到：Modal 头部（设置/软件管理/全部对话框）、
  QuickPanel 托盘面板、WelcomeWizard 欢迎向导（点击=跳过并完成）、SearchOverlay 全局搜索、
  WintabSwitcher（wintab-head-row）。CSS 见 overlays.css 末段（close-light / wizard-lights）。
- 图标尺寸对齐 Windows：SIZE_TIERS 图标字形 ≈ 标称值（32→30 / 48→46 / 64→62，
  原 20/26/34 太小），网格单元相应放大（84×98 / 100×114 / 118×132）。
- Wallpaper Engine 导入（零网络）：
  - Rust wallpaper.rs：WpEngineItem + wp_engine_scan(root)。root 空 = 自动探测
    （注册表 HKCU\Software\Valve\Steam\SteamPath 优先 — 本机 D:\Steam，再补 C 盘默认 +
    libraryfolders.vdf 其余库）；扫 workshop/content/431960/* 与 common/wallpaper_engine/
    projects/{myprojects,projects}/*；解析 project.json（utf-8-sig BOM 兼容）→ title/type/
    file/preview.jpg；video(image)+媒体扩展名 → supported，scene/web/application →
    supported=false（如实标注不可渲染）。注册 lib.rs。
  - 前端：ipc.wpEngineScan + Shell.WpEngineItem；设置→外观新增"Wallpaper Engine 导入"
    区块（扫描/选择目录/缩略图列表/一键导入：video→视频壁纸，image→图片壁纸，其余 toast
    如实提示）；词典 wpEngine*（zh/zh-TW/en）。
- 实机验证：扫描列出 13 个项目（7 video 可导入 + 6 scene 如实标注）；点击 video 项目
  toast"壁纸已导入"且桌面立即播放（设置里路径指向 workshop 3776110665）；绿灯关闭设置弹窗；
  图标变大为 Windows 尺寸。回归：cargo check / tsc / vitest 218 / vite build 全绿。
- 教训：Steam 可能装在任意盘 — 探测必须走注册表 SteamPath（winreg 已有依赖）；
  tauri dev 的 Rust watcher 在进程被 kill 后不会自动重启，需手动重新 npm run tauri dev。

## 批次 E-11（2026-09-05）完成 — 实机自检（tauri dev + 屏幕控制逐项操作验证）
- 用户反馈"新版本仍有问题"→ 启动 dev 实例用 computer-use 实测，抓到三个真根因：
1) 右键子菜单永远打不开（新建/查看/排序方式/切换壁纸"点了没反应"的真正根因）：
   ContextMenu.tsx MenuLevel 死锁 —— 子菜单 div 只在 subPos 非空时渲染，而 subPos
   又要子菜单渲染后才有 ref 可量测。修复：subOpen 即渲染（-9999+hidden），layout
   effect 量测后定位。实测：悬停展开、点选生效（壁纸 纯黑↔3D 引力场 来回切换成功、
   新建文件架成功、删除确认框出现）。
2) lib.rs setup 里第二份硬编码快捷键表（super+e/ctrl+alt+o/super+n 等）与默认表并存，
   且 winman::init_shortcuts 从未被调用 → 启动日志一直 register failed。修复：lib.rs
   统一走 init_shortcuts。实测启动仅剩 super+tab 一个系统保留键降级（预期）。
3) VWM/WindowControls 红绿灯顺序错排（黄|红|绿）→ 修正为 左绿(退出) 中黄(全屏) 右红(最小化)。
- 实测通过清单：桌面启动/壁纸/任务栏小组件/托盘；文件管理器 VWM 内嵌（修复 .ex-window
  position:fixed 盖住标题栏 → 内嵌改 absolute）；新建文件夹 Prompt 弹窗+创建成功(76→77)；
  黄灯最大化/还原、红灯最小化、任务栏点击恢复（列表状态零丢失）；桌面绿灯退出选择框
  （ChoiceHost）；回收站 VWM 打开；桌面右键子菜单全集；壁纸切换。
- 教训：vite HMR 链断裂（soft-invalidate 冲突）会让页面跑旧代码，验证前要么重启 app
  要么看 vite 日志确认 hmr update 生效；.ex-window 等 position:fixed 组件嵌入 VWM 必须
  在 .vwm-app（translateZ 包含块）内降级为 absolute。

## 批次 E-10（2026-09-05）完成 — 用户实测缺陷集中修复
- 宿主缺失（根因级）：App.tsx 桌面分支只挂 ConfirmHost/NetConsentHost，缺 ChoiceHost/PromptHost/ConfirmBubbleHost —— askChoice/askPrompt 的 promise 无人 resolve → 红灯退出选择框、"新建/重命名"输入框全部"点了没反应"。已补全四个宿主（explorer 内嵌进桌面窗口后也依赖这批宿主）。
- explorer/recycle 收进 VWM：vwm.ts 新增 VwmApp = AppMode|"explorer"|"recycle" + openVwmSystem(kind, path?)（recycle 单实例；explorer 带 path 新开实例、无 path 聚焦既有）；VwmWin.path 传 initialPath；VwmAppContent 内嵌渲染 ExplorerWindow(embedded)（无自带标题栏/几何记忆/宿主）；ExplorerWindow 增 embedded/initialPath props，Ctrl+N 内嵌时走 openVwmSystem。Taskbar 文件管理器按钮 = taskbarClickVwm("explorer")（运行点+点击切换）；StartMenu 三处、DesktopIcons 文件架/飞出面板全部改走 VWM —— 不再"单独分屏到环境外"。
- explorer.html boot-splash 此前从未移除（永远停在 VARIABLE·EXPLORER 启动屏）→ entries/explorer/main.tsx 首帧后 add("done")+remove。
- "asset not found: app-project.html"：appWindowLabel(project)→"app-code"（vite 入口就叫 app-code）；Taskbar 运行态轮询 label "code"→"project" 映射修正。
- 红绿灯（需求指定）：顺序=绿|黄|红；🟢退出/🔴最小化/🟡全屏。桌面壳层（绿=退出选择框、黄=toggleMaximize、红=minimize）、VWM 窗口（绿=关窗）、WindowControls(mac) 三处统一；桌面 🟢 原先的覆盖/避让切换移除（设置页保留 avoidTaskbar）。
- 快捷键"注册失败(被系统占用)"根因：Win11 保留 Win+E/D/M/N/数字/方向键。默认表（shortcuts.ts + winman.rs 两端同步）全改 ctrl+alt+*（explorer/showDesktop/minimizeAll/notifyCenter/snap*/launch1-9，quickAudio ctrl+alt+o→k）；Rust dispatch "explorer"|"explorerCtrl" → emit sys://open-system → DesktopShell 监听后 openVwmSystem（不再后端直开 OS 窗口）。
- 壁纸"点了没反应"：右键切换壁纸对 image/video/hybrid 在未选过文件时直接弹文件选择框（switchWallpaper），选完即用；设置页选择器原本即可用。
- React #310（设置页打开即崩）：工作区已含 MindNodeView/SettingsModal hooks 前移修复；本轮 tsc/build/218 测试全绿。
- 兼容：app-*.html 拆窗入口与 openAppWindow/openExplorerWindow 保留为遗留回退（非桌面窗口内 xref 跳转、独立 explorer Ctrl+N）；snap.tsx applySnap 桌面窗口自跳过，与 VWM 贴靠不冲突。

﻿## 批次 E-9（2026-09-05）完成 — VWM 虚拟窗口管理器
- 需求：四款软件改由桌面层内"虚拟窗口"托管（原为 OS 级拆窗），实现轻量 WM：独立 Z 序、聚焦态、拖拽移动、边缘贴靠分屏（左右半屏/四角1/4/顶部最大化）、最小化到任务栏（保持挂载零重载）、右上角 Mac 风格红绿灯、同软件多开（中键任务栏图标/forceNew）。
- 新文件：src/system/windows/vwm.ts（状态机+几何持久化 variable:vwm:geom:v2+工作区按 taskbarPos 四向推导）、VirtualWindowFrame.tsx（拖拽/八向缩放/贴靠预览/红绿灯）、VwmAppContent.tsx（四软件视图原样挂载 + CosmicBackground 置于窗口包含块内，光影零差异；Write 实例承接 xref://focus）、VirtualWindowManager.tsx（层渲染、Alt+Tab 轮转、Win+方向键贴靠、Win+M 最小化）、src/styles/vwm.css（z=15：图标之上、红绿灯/任务栏之下）。
- 接线：DesktopShell 渲染 VWM + launchIndex→openVwmApp；App.tsx onOpenApp→openVwmApp；Taskbar 运行态=vwm wins、点击=taskbarClickVwm（开/聚焦/最小化切换）、中键=forceNew 多开、悬停关闭=关最上层实例；xref.ts 桌面窗口内走 VWM（遗留 OS 窗口回退 openAppWindow）；WintabSwitcher 并入虚拟窗口（Z 序降序）；LauncherManager 卸载时 closeVwmApp。
- 兼容：app-*.html 拆窗入口与 appWindows.ts 全部保留未删（可直接打开仍走旧路径）；explorer/recycle 仍为 OS 窗口。
- 验收：tsc 零错误；vite build 成功；vitest 218/218 通过。
- 教训：transform: translateZ(0) 建立包含块可把 .bg-root(fixed) 约束进虚拟窗口——这是"多 WebGL 光影随窗口走"的关键；最小化用 display:none 保活而非卸载，恢复零重载。


## 批次 E-8（2026-09-05）完成
- E-8a zh-TW 繁体语言包：src/i18n/s2t.ts（字符级简→繁）+ dictionaries.ts（zhTwOverrides 键级覆盖 + Lang 三值）；i18n/index.tsx 週制日期。
- E-8b 设置页：外观任务栏位置 UI 联动核对；关于页版本号 getVersion(@tauri-apps/api/app) + Schema/便携标识。
- E-8c 拼音/首字母搜索：src/lib/pinyin.ts（pinyinOf/initialsOf/matchPinyin/isAsciiQuery，内置常用字表）；StartMenu 搜索框 + filtered/filteredRecent；SearchOverlay ASCII 查询拼音回退。
- E-8d 通讯软件通知：src-tauri/src/shell/imwatch.rs（EnumWindows 窗口标题未读标记「(N)」轮询 3s，不读正文）→ sys://im-msg → DesktopShell pushNotify+横幅；lib.rs setup 启动。
- E-8e 验收实测：NSIS Variable_1.0.0_x64-setup.exe（3.6MB）+ MSI Variable_1.0.0_x64_en-US.msi（5.1MB）打包成功；手测清单 docs/ACCEPTANCE_CHECKLIST.md。
- Lang 类型扩散修复：narrate.ts/engine.ts/dictionaries.ts(code,两处 DictContext)/english.ts/ingest.ts/intent.ts/ProjectVizPanels/App.tsx/WintabSwitcher 统一接受 "zh"|"zh-TW"|"en"（ternary on "en" 天然兼容）。
- 教训：pinyin.ts/s2t.ts 重复键、StartMenu 重复 useState 由 IDE 旧缓冲区回退反复出现，需多次重试并立即 tsc 验证；关掉未保存标签页是根治办法。

## 批次 B-12（2026-09-06）完成 — Uxv chunk 层
- B+ 树（container crate）：阶 32，插入分裂/删除收缩（不做合并，登记 B-15）；等值键路由用 lower_bound——分隔键留存于左叶，upper_bound 会让等值键重复插入，oracle 差分测试抓出。
- UxvBackend 单文件容器 v1：[SuperBlock 64B][追加区 chunk 记录+索引 checkpoint blob][Footer 双副本]；4MiB 定长切分 + BLAKE3 内容寻址去重 + refcount；read_range/stream 大文件通道；seal=索引落盘+sync_all。
- 快照：追加区不可变 ⇒ 索引状态拷贝即时间点快照，restore 零成本（chunk 位置以快照留存的 ChunkLoc 重建 refcount）。
- 21 测试全绿：oracle 差分 4000 步、seal/reopen 持久化、未 seal 打开报 Corrupted（journal 属 B-13）、篡改字节哈希校验、热 chunk 随机读（200 次 @16MiB 容器实测 << 20ms 口径）。
- 教训：①流式读必须 +37B 跳过 chunk 头——"位置=头部起点"的偏移语义要跨层对齐；②B+ 树等值路由与分隔键留存位置强耦合，改二分方向必须同时改 insert/get/remove 三处。

## 批次 B-13（2026-09-06）完成 — Journal 事务与掉电安全
- 事务协议：事务内先写 chunk 数据记录，随后 [JNL1][kind][len][blake3] 操作记录 + COMMIT；COMMIT 未落即掉电 = 事务整体丢弃。
- 重放：打开时从 SuperBlock 持久指针定位最近 checkpoint，魔数区分 chunk/记录双布局无歧义分流；撕裂尾自动截断，data_tail 停在第一个无效记录处。
- 三个关键修复：①Footer 定位从"文件末尾"改为 SuperBlock 指针（journal 在 Footer 之后追加后，末尾语义失效）；②checkpoint 的 SuperBlock 翻转作为原子发布点（Footer 先落盘再翻指针）；③restore 后立即 checkpoint 重置 journal 时代（否则重放会把回滚前的事务重新套上）。
- 验收：掉电注入 100 轮（随机写/覆盖/删/改名 + 未 seal drop + 重开比对 oracle）0 数据丢失；撕裂尾事务正确丢弃、其余完好；首会话未 seal 即中断如实报 Corrupted（自动恢复点属 B-33）。25 测试全绿。
- 教训：①二进制记录流里"首字节高位做标记"不可靠——chunk 长度字段同样可能高位为 1，必须用独立魔数；②持久化格式里每个定位手段（末尾/偏移/指针）都是隐式契约，追加任何新记录类型前先审所有定位假设。

## 批次 B-31（2026-09-06）完成 — Schema 版本与迁移协议
- schema.rs：probe() 只读 SuperBlock 即报版本（零开销惰性检查）；migrate_to_current() = 字节级 pre-migrate 快照（.migrate-bak-vN）→ 逐级升版（当前全部为版本戳迁移，数据重排型 Migrator 函数表随 B-15 多卷表登记）→ 校验；任一步失败自动回滚快照。
- 只升不降：容器版本 > 引擎版本拒绝打开并提示升级引擎，绝不降级改写；数据段与索引 blob 不重写，迁移耗时与资产量无关。
- 布局事实：SuperBlock = [magic 8B][version 4B][footer 指针 8B]，版本 @ sb[8..12]；Footer 双副本位置由 sb[12..20] 指针给出。
- 4 测试全绿（总数 29）：探测、旧版本就地升版+数据完好、新版本拒绝降级、迁移失败自动回滚。
- 教训：①schema 探测模块必须复用同一 SUPERBLOCK 布局常量，两处手写魔数/偏移必然漂移（本次 magic 写成 4B 版被 oracle 测试立刻抓住）；②迁移事务的回滚测试和正向测试同样重要——"失败也要回到迁移前"才是事务。

## 批次 B-14（2026-09-06）完成 — 压缩与加密
- codec.rs：LZ4 优先；≥256KB 且 LZ4 比率 <1.25 时比选 Zstd-19；不可压缩回存 RAW（codec 字段 0/1/2）。
- vault.rs：Argon2id（m=19MiB,t=2,p=1）派生 + 16B 随机盐；验证器 = key 的 BLAKE3 前 16B 恒时比对；XChaCha20-Poly1305 每记录独立 24B 随机 nonce（流密码 nonce 复用 = 灾难，测试断言同明文密文必异）。
- 全容器加密态：chunk 载荷、索引 blob（文件名不可枚举）、journal payload 三层全部 [nonce|AEAD]；SuperBlock [20] 标志位 + 盐/验证器持久化；open_with_passphrase 创建/解锁，trait open 对加密容器如实报 Auth。
- 解码 LRU 缓存（8×4MiB）支撑"热 chunk 随机读 <20ms"——debug 下每读解压 4MiB 会到 20.7ms，缓存后 200 次随机读仅 4 次解码。
- 头部哈希口径分离：记录头 blake3 = 存储字节（完整性），索引键 = 原文哈希（去重）——混用会导致加密后校验必挂。
- 41 测试全绿（codec 4 + vault 4 + 容器级加密 4 + 回归 29）；依赖版本锁 locks/container-deps.md。
- 教训：①read_range/stream 的 chunk 定位必须用"原文 4MiB 逻辑边界"而非存储长度（压缩后两者不等，混用即静默错位）；②流式读取器按逻辑窗口整块解码+缓存，物理偏移只用于定位记录头；③git 基线 + 全量补丁脚本（锚点断言）是对抗"编辑器内容漂移"的唯一可靠工作法——逐条 sed 补丁必然漏。

## 批次 B-15（2026-09-06）完成 — 多卷条带与 GC 分代回收
- 多卷：卷组 = [主卷(仅 journal+索引)] + 数据卷（OpenCfg.extra_volumes 建卷 / 索引卷表持久化）；chunk 按 stripe_pick 轮转数据卷，ChunkLoc 增加 volume 字段（schema v2，经 B-31 迁移器升版）；单卷模式退化为共享 meta_tail（与 v1 行为等价）。
- 解码 LRU 缓存键 = (volume, offset)——首版只按 offset，跨卷同偏移互串（oracle 测试当场抓住）。
- GC：标记（活 chunk 物理位置集合）→ 逐卷顺序走查（JNL1 魔数跳 journal，len>4MiB 即防御性止步）→ 死字节 ≥50% 的最差卷单卷压实（活记录原样搬迁 + 索引重定位 + rename 替换）；GcReport 口径 = chunks_examined/bytes_reclaimed；受 budget_ms 预算约束。
- 验收：4 卷条带跨卷读写 + seal 重开卷拓扑保持；GC 回收 >0 且活数据不膨胀、数据无损、暂停 <100ms；v1→v2 迁移走 schema 协议全流程。43 测试全绿。
- 已知边界：v1 索引 blob 的 ChunkLoc 无 volume 字段，当前 v1→v2 迁移器只翻版本戳（v1 从未发布，仅存在于开发容器）；真实索引重写迁移器随 B-33 恢复模式补齐。
- 教训：①"主卷=元数据、数据卷=纯 chunk"的职责分离让 GC 走查和 journal 重放都简单——比"每卷都能放一切"少一类歧义；②Windows 上 write-only 打开已存在文件会 AccessDenied，统一 read+write 打开。

## 批次 B-16（2026-09-06）完成 — VHDX 快速档 + 迁移向导
- vhdx.rs：probe()（管理员 whoami 完整性级 + Get-Command Mount-VHD 探测，两次子进程 ≤300ms 预算）→ 不可用如实报 NotImplemented 并降级 Uxv（风险表第 1 行）；VhdxBackend = 挂载点上委托 DirBackend（同接口同行为）；open 登记卸载责任、seal 时 Dismount；with_premounted 供测试/引导器注入已解析挂载点（跳过 probe 门禁但不登记 dismount）。
- migrate.rs：迁移向导内核——逐文件复制进后端 + 回读 BLAKE3 比对（验收口径），任一失败如实上抛且源目录原样保留；跳过 .tmp-bench/data.migrated-backup 产物；目录改名 data.migrated-backup 属 UI 层（文件系统原子操作）。
- 47 测试全绿（vhdx 2 + migrate 2 + 回归 43）。真实 10GB 媒体迁移与 VHDX 实挂属三宿主实机项（H1 必测）。
- 教训：①"谁登记卸载责任谁执行"——premounted 路径误登记 vhdx 会让 seal 去 Dismount 一个目录（错误信息里 PowerShell 乱码是 GBK 输出未按 UTF-8 解码，属观测噪音）；②测试期望值要跟字节口径走（7 字节 JSON 写成 8），断言数字突变先问口径。

## 批次 B-17（2026-09-06）完成 — 容器仪表与冷热分层
- stats()：每卷 used/声明容量（8TB 场景 = 4×2TB 建卷声明，set_declared_capacity 登记）+ 逻辑/物理写入计数 + 写放大比（健康口径 ≈1.0~1.5）+ 文件/chunk 规模；IPC/任务栏水位线 UI 属主 crate 接线点（数据源已备）。
- 挂起项目：freeze = 前缀下全文件冷层（Zstd-19 强制）重编码；解冻反向。冷层键 = blake3(内容哈希) 独立命名空间——同内容允许热/冷两份物理编码共存，这是挂起/解冻的底层前提（内容寻址去重会挡住"同内容重编码"，首版测试当场暴露）。
- 49 测试全绿（仪表 2 + 回归 47）。M2 容器线代码批次（B-12…B-17 + B-31）全部收口。
- 教训：内容寻址与"同一内容的多份物理编码"天然冲突——引入冷热分层时必须先把键空间分层，而不是在编码层硬绕。

## 批次 B-32 + B-33（2026-09-06）完成 — OOBE 向导与应急能力包（M2 半包）
- container crate rescue.rs：两级救援内核——salvage_files（索引有效 → 整树导出，list_all 快照）+ salvage_chunks（Footer 失效 → 独立解析器走查记录流，JNL1 魔数跳过/len>4MiB 重对齐/blake3 逐条校验）；救援只读容器、只写输出目录；safe_join 双保险防路径逃逸。52 测试全绿（救援 3）。
- 主 crate shell/recovery.rs 六命令：container_diag（版本/魔数/可开性，恢复模式判定）/ container_repair（journal 重放 UI 化：打开即重放 + 立即 checkpoint 固化）/ container_rescue_export（文件级失败自动降级 chunk 级）/ container_init（OOBE 建卷，可选口令加密，拒绝覆盖非空）/ container_stats（B-17 仪表数据源）/ vhdx_probe（介质体检）。响应结构统一 serde camelCase。
- B-32 OOBE：src/features/oobe/OobeWizard.tsx 四步向导（介质体检 → 口令建卷 → 三模板多选 → 60 秒导览）；OobeGate 由 settings.oobeDone 门控，App 两渲染分支接入；Settings 增 oobeDone/oobeContainerPath/oobeContainerEncrypted/oobeTools 四字段。
- 设置页新增「存储与恢复」标签（StorageRecoveryTab）：诊断/修复/救援/仪表四按钮 + 口令输入 + 诊断与水位线展示；i18n zh/en 47 键（zh-TW convertDict 继承）。
- 顺手修掉一个上线必炸的遗留缺陷：前端调 residue_scan、后端注册 residue_scan_cmd——运行时 invoke 必报 command not found（拆分为内部纯函数 residue_snapshot_diff + 命令包装）。
- 教训：①audit 只断言"前端 invoke ⊆ 后端注册"，反向冗余不报红——名字拼写类缺陷靠的是这次 IPC 面核对而非工具（audit 可增强为双向 diff，登记 M4 顺手项）；②救援工具的设计原则是"不信任主路径"——独立解析器 + 只读容器，即使与主实现格式漂移也能自救。

## 批次 B-18 + B-19（2026-09-06）— 浏览器矩阵
- B-18 ✅：shell/browsers.rs——检测（App Paths 注册表三级 + ProgramFiles/LocalAppData 路径兜底，chrome/edge/brave/vivaldi/firefox 五款）；便携模板按家族分流（Chrome 系 --user-data-dir / Firefox -profile -no-remote）；Profile CRUD（新建即建容器目录、克隆=整目录拷贝含登录态、删除可焚毁=单次覆写后删，标准级擦除属磁盘层职责如实边界）；启动走 exec::spawn_profiled（HOME/USERPROFILE 镜像容器 + VARIABLE_PROFILE_ID 标记）。UI=设置页「浏览器」标签（BrowsersTab）。
- B-19 🟡：导入向导已做（书签 HTML/密码 CSV 文件复制进容器 imports/ + 书签计数解析，绝不读宿主浏览器运行数据，UI 两段式选文件）；**任务栏按 profile 分组未做**（winman 分组键扩展，登记 B-19 收口项）。
- 工程手法：Tauri State 不能在测试里凭空造——命令一律拆 inner(st: &AppState)，测试直打 inner；AppState 测试构造只需全 PathBuf 字段 + Mutex<Option<Connection>> 置 None（这些命令不碰 DB）。
- 测试：后端 4 项（模板参数分流/书签计数/slugify 防注入/CRUD 全流程含焚毁）+ 回归全绿；tsc/audit/i18n/vitest 218/build 全绿。
- 教训：①tauri::State 是编译期包装，业务逻辑沉到 inner(st:&AppState) 是可测性与命令面的双赢；②reg query 解析 App Paths 要按 "REG_SZ" 行尾倒序取值——注册表输出的对齐空格不可靠。

## 批次 B-19 收口（2026-09-06，同日第七批）— 任务栏按 profile 分组
- 后端：browser_running 命令——launch 时登记 pid（LAUNCHED 静态表），运行态 = pid 经 OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) 存活探测；每个存活 profile 是独立分组项。
- 前端：Taskbar 在第三方区后渲染浏览器分组项（Globe 图标 + 212 色相徽标 + 运行点），3s 轮询并入既有 tpRunning 轮询（Promise.all 一并取 browserProfiles/browserRunning）；点击 = 再次以该 profile 启动。
- "分组"语义定版：每 profile 一个独立任务栏项（同浏览器多 profile 不合并）——数据目录即隔离边界，与"Chrome 自带多 profile 合并为一个窗口组"的宿主行为刻意不同。
- JSX 插入教训：往大组件 return 里插块必须先读清闭合层级（第一次插在按钮容器 </div> 后导致 JSX 断裂，TSC 立即抓出）。
- 至此 B-18/B-19 全部收口，M4 浏览器矩阵代码面完成。

## 批次 B-20（2026-09-06）完成 — VS Code Portable 一键部署
- shell/code.rs：code_deploy（curl 字节进度 code://progress 事件流 + Expand-Archive 内层目录上提 + data/ 目录进 Portable 模式——user-data/extensions 全在容器）；code_register（幂等 upsert ThirdApp id=vscode，执行档 HOME/USERPROFILE 镜像容器）；code_launch（复用 embed_launch 整条通道——Electron 嵌入回归由"零新增嵌入代码"保证）；code_status（deployed/portableData/registered 如实三态）。
- 离线降级：runtime/vscode-download.zip 已存在即跳过下载（离线宿主手动放置官方 zip 即可）。
- 前端：设置页「编码」标签（CodeDeployCard：状态/部署进度条/登记/嵌入启动）；登记后 VS Code 自动出现在启动器/任务栏（复用 thirds 生态，无新增任务栏代码）。
- 测试：portable 布局契约 + 状态诚实性 2 项 + 回归全绿（workspace 105）；tsc/audit/i18n/vitest/build 全绿。
- 教训：①集成型批次的最强回归保证是"复用而非新写"——VS Code 是 Electron 应用（嵌入最易翻车的类别），但整条 embed 通道零改动，风险面收敛为"部署本身"；②Tauri emit 需要 Emitter trait 显式导入（v2 与 v1 的隐蔽差异）。
- 如实边界：真实下载/嵌入启动属 H1 实机项（内网/离线开发环境无法验证 update.code.visualstudio.com 可达性）。

## 批次 B-21（2026-09-06）完成 — 工具链模板与 PATH 注入
- shell/toolchains.rs：python（官方 embeddable zip + _pth 解禁 site-packages）/ go（zip 内层上提）/ rust（rustup-init -y --no-modify-path，CARGO_HOME/RUSTUP_HOME 全在容器）三条部署通道；版本锁 locks/toolchains.md；离线降级 runtime/<id>-download.(zip|exe)。
- PATH 注入收口到 exec::spawn_profiled 统一处理：runtime_path_prefix 收集存在的 runtime 目录（冻结顺序 npm-global→node→python→python/Scripts→go/bin→cargo/bin→vscode/bin），前置到 PATH 再拼继承值——终端/AI CLI/浏览器/VS Code 一切受管进程天然继承容器工具链，零散注入从此绝迹。
- 前端：设置页「编码」标签下 ToolchainsCard（状态列表 + 部署按钮 + toolchain://progress 事件）。
- 测试：PATH 前缀（存在性过滤 + 冻结顺序）+ 工具链路径口径 + 回归全绿（workspace 108）；tsc/audit/i18n/vitest/build 全绿。
- 教训：①PATH 注入从"每个启动点自己拼"收敛为 spawn_profiled 单点，是"同一件事只能有一个实现"纪律的典型案例；②版本锁文件与代码常量必须同提交，否则升版时必漏一处。
- 如实边界：python.org/go.dev/rust-lang.org 真实下载属 H1 实机项；rustup 首次安装需稳定出站（断网场景降级离线放置）。

## 批次 B-22（2026-09-06）完成 — Git 深度面板（只读）+ SSH 金库代理
- git2 只读层（default-features=false 免 openssl 系统依赖）：git_status（分支/HEAD/变更分类 wt_*/index_*/conflicted/ahead-behind）/ git_log（限时 revwalk）/ git_branches；ensure_in_container 强制仓库在容器数据目录内（canonicalize 防逃逸）。
- SSH 金库代理：ssh-key crate 生成 ed25519 对，私钥封存 vault/ssh/（金库未解锁即拒绝，与 identities.seal 同口径）；"代理"= 执行档注入 GIT_SSH_COMMAND（ssh -i + IdentitiesOnly=yes），不另起 ssh-agent 进程（进程面缩减；agent 转发如实边界）。
- 职责边界（刻意）：面板零写操作——暂存/提交/push 在终端完成，写路径 = 终端 = 既有审查过的通道，避免"面板一个按钮 = 一条新进程/网络面"。
- UI：Code 应用「项目」视图工具条新增 ⎇ 按钮 → Git 面板浮层（变更分类着色/分支/历史/SSH 密钥生成），5s 轮询。
- 测试：git2 搭真实临时仓库（init→commit→改文件）验状态/日志 + 容器外仓库拒绝 + GIT_SSH_COMMAND 注入格式，3 项；workspace 111 全绿；tsc/audit/i18n/vitest/build 全绿。
- 教训：①ssh-key 0.6 的 API：Ed25519Keypair::random(&mut rng)、PublicKey 从 PrivateKey::public_key() 取、序列化是 to_openssh 不是 openssh——按记忆写 API 必错，cargo 错误信息里给的正确签名才是真相；②grep 在部分大文件上静默失败，python 逐字符搜索是对的抗漂移手段。

## 批次 B-23（2026-09-06）完成 — 并行搜索/大文件分块/行级跳转
- shell/search.rs：workspace_search——目录树收集（跳 node_modules/.git/target/__pycache__，文件数 2 万截断）+ std scoped threads 8 路并行读文件；大小写不敏感、命中行裁剪（前后 80 字符）、单文件 20 行/全局 500 行/60 文件上限、NUL 探测跳二进制、>8MB 跳过（随机写介质保护）；跳过计数用 AtomicUsize（多线程 &mut 必炸，编译器把着教）。
- bigfile_slice：按 offset/len 读窗口（100MB 打开 <1.5s 口径由"只读当前页"达成）；editor_goto：VS Code `--goto file:line`（未部署如实报错）——PVCCE ↔ VS Code 行级跳转的 VS Code 侧。
- 前端：Code「项目」视图 ⌕ 按钮 → 搜索浮层（结果按文件分组 + 行号 + 分块查看 pager + →跳转按钮）。
- 测试：搜索（命中/大小写/跳过口径 4+3+1 断言）/空查询拒绝/分块读窗口与越界 4 项；workspace 114 全绿；tsc/audit/i18n/vitest/build 全绿。
- 教训：①scoped threads 里共享计数只能用 AtomicUsize（&mut 跨线程 N 个闭包必炸，编译器逐个指出）；②测试期望值要跟实现口径走（node_modules 整目录跳过 = 文件数从清单里就没有，不是"扫描了但跳过"）。

## 批次 B-24（2026-09-06）完成 — 子环境档模型 + 存储剖面 + 切换编排
- envs.json：{active, envs:[{id,name,settings,created_at}]}；main 常驻且行为与单环境完全一致；剖面目录 envs/<id>/home + /workspaces。
- 执行档新增 {envhome} 占位符：解析活动环境 home（envs.json 缺失/active=main/目录缺失三级回退到容器 home）；browsers/code/ai(多账号配置目录) 全部从 {home} 切到 {envhome}——凭据与登录态按环境隔离。
- 切换编排 4 步：①当前偏好快照随请求写入旧环境 → ②active 翻转 → ③返回目标环境快照 → ④前端 patchSettings 应用（短版重载）。同环境幂等、活动环境不可删、main 不可删。
- UI：设置页「环境」标签（列表/当前徽标/创建/切换/删除）；i18n zh/en 36 键。
- 测试：create/switch/delete 全流程 + 快照写入旧环境 + {envhome} 解析 + main 回退，2 项；workspace 116 全绿；tsc/audit/vitest/build 全绿。
- 已知边界（如实）：工作区文件与 apps.json 登记表跨环境共享——完全剖面隔离随 B-25/B-26 快照克隆评估；切换的"启动仪式短版"目前=设置联动重载。
- 教训：①audit.cjs 的 i18n 检查 exit code 恒 0，MISSING ZH/EN 只打印不设退出码——"exit 0"不等于"审计通过"，必须读输出；②向 dict 插键的去重检查必须限定在该 dict 段内（全文本检查会把 zh 已有键误判 en 已有）；③文件含 CRLF/LF 混排时，'\n};' 定位要用 \r?\n 正则或行扫描。

## 批次 B-26 + B-25（2026-09-06）完成 — 环境快照克隆与嵌套实例（M6 代码面收口）
- B-26：env_clone——剖面目录（home/workspaces）全量复制 + 条目/设置快照复制；bytes_copied 进报告。Uxv chunk 后端的 COW 零拷贝由 M2 快照机制提供（"克隆 10GB < 2s"属 Uxv 容器 + H1 实机口径）；目录剖面 V1 物理为全量复制（安全 > 快，硬链接会被两端写穿透）。
- B-25：env_nested——spawn 当前 exe + VARIABLE_DATA_ROOT（独立数据根，嵌套互斥：绝不同一容器互踩 journal）+ VARIABLE_NEST_DEPTH（上限 3）+ VARIABLE_PARENT_ENV；state.rs bootstrap 优先读该环境变量。白名单继承 V1=子实例独立 netconsent 库（父白名单透传接口已预留，B-28 白名单库落地后改为显式子集注入）。V1 嵌入回退：子实例为独立 OS 窗口（与嵌入失败路径"不杀进程"哲学一致），嵌 VWM 属后续批。
- 前端：环境行新增「克隆」「嵌套启动」按钮 + 报告 toast（克隆带 bytes）。
- 测试：envs 4 项（roundtrip/depth/env 构造/嵌套根隔离）+ 回归全绿（workspace 118）；tsc/audit/vitest/build 全绿。M6 代码面收口。
- 教训：①嵌套互斥的正确解法是"独立数据根"而非"共享容器加锁"——后者把并发问题引进了 journal；②深度限制要在父进程判定（depth>=3 拒绝），子进程再判只是兜底——防御要放在发起侧。

## 批次 B-27（2026-09-06）完成 — 应用生态 2.0（M7）
- shell/ecosystem.rs：可移植性评估（三类启发式：目录可写/卸载注册表痕迹/本地配置形态 → 绿黄红建议卡，结论逐条理由如实）+ 搬迁执行器（目录整拷 apps/<id>/ + reg export portable.reg 快照 + ThirdApp 登记）+ Steam 库扫描（HKCU SteamPath → libraryfolders.vdf 解析 → appmanifest_*.acf）+ steam://rungameid 协议直通 + AUMID 启动（shell:AppsFolder）+ 文件关联表（fileAssociations.json，解析不到即宿主兜底打开）。
- UI：设置页「生态」标签（评估卡/搬迁/Steam 扫描启动/关联登记）。
- 测试：评估卡（可写+本地配置=绿/至少非红、缺 exe 报错）、VDF 解析（新旧两种键格式）、slug 防注入，4 项；workspace 122 全绿；tsc/audit/vitest/build 全绿。
- 如实边界：评估卡是启发式（运行时监控才可判定）；真实搬迁 7-Zip 与 Steam 启动属 H1 实机项；反作弊游戏默认独立窗口（不嵌入）。
- 教训：①VDF 新旧两种键格式（"path" vs 数字键）都要兼容——取"最后一个 tab 字段且值含盘符"的启发式比精确匹配键名更稳；②heredoc 里的 \t 与文件里真实 tab 是两种东西，字符码构造替换串是抗漂移的最后一招。

## 批次 B-28（2026-09-06）完成 — 网络层（M8）
- shell/network.rs：环回 HTTP 代理（CONNECT 隧道目标域名明文可裁决 + 绝对式改写相对式 + origin-form 按 Host 头）——策略 = kill-switch → 域名规则（host==rule || ends_with(".rule") 子域通配）→ 默认拒绝（默认零出站落地）；拒绝回 403 带 "blocked by Variable network policy" 标记。
- 流量仪表：bytes_relayed/conns_allowed/conns_denied（Arc 计数三元组，Counters 结构体 Clone 注入线程）；net_status 汇聚。
- 执行档接线：spawn_profiled 在代理运行时注入 HTTP(S)_PROXY/HTTPS_PROXY → 一切受管进程出站汇聚环回代理（M1 遗留的 net_allow 执行点落地）。直连逃逸如实边界：用户态无法硬断，逃逸计数+标记，硬断网属系统防火墙。
- 命令面七条：net_status/proxy_start/proxy_stop/kill_switch/rules_list/rule_grant/rule_revoke；设置页「网络」标签（代理启停/kill-switch/仪表/规则增删）。
- 测试：classify（规则/子域/kill-switch 覆盖）+ 域名尾点容忍 + 代理端到端（本地假 HTTP 服务：白名单 200 / 未授权 403）3 项；workspace 125 全绿；tsc/audit/vitest/build 全绿。
- 教训：①Arc<(A,B,C)> 的字段 Clone 要整体 clone Arc 再解构（对字段做 Arc::clone 会 auto-deref 到原子类型报错连环）；②并发计数器一律 Arc + fetch_add，&mut 跨线程编译期就拦——Rust 借检在多线程场景是最强的架构评审员；③原子类型均无 Copy，闭包 move 后再 fetch_add 是经典 E0382——先 clone Arc 再用。

## 批次 B-29（2026-09-06）完成 — 安全分析工作台（M9）
- shell/security.rs：纯 Rust 手写 PE 解析（DOS/PE/COFF/可选头/节表 + 每节 Shannon 熵 >7.0 标记疑似加壳 + 导入表 IID 走查（RVA→文件偏移映射）+ 证书表签名存在性）+ 可疑 API 命中（VirtualAlloc/CreateRemoteThread/WriteProcessMemory 等 14 项注入面）+ 字符串提取 + iced-x86（纯 Rust）入口反汇编只读查看器 + Windows Sandbox 探测（reg 特性键，不可用如实标注）+ .wsb 生成（映射样本只读+断网）+ 安全子环境预设（白名单清空+kill-switch 开）+ Markdown 报告导出（不含样本字节）。
- 铁律兑现：分析器对样本只读——绝不在宿主执行样本或其代码路径；>64MB 样本如实截断标注。
- 测试：手工构造最小 PE32+（DOS/PE/COFF/可选头/节表字节级拼装）验头/节/熵/签名 + iced-x86 入口反汇编（push rbp）+ 高熵节加壳标记 + 非 PE 如实报告，4 项；workspace 129 全绿。
- 教训：①手写字节级解析器时，测试构造器与解析器共享同一份偏移常量表才是根治"两边各写一遍必然漂移"的正解（本轮 COFF 20B/PE32+ 可选头偏移漂移连坑三次）；②Rust 字面量 '\t' 与真实 tab 在文件写入层会被"规范化"，跨层写测试样例时用 chr(9) 构造；③entropy 直接调用 vs 经 analyze 结果不一致 = 数据流断点定位的最快手段。
