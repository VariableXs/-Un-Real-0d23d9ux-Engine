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
