# AI-3 兼容体验收口

日期：2026-09-07
范围：`PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 第 6、7 章，以及扩充章 16 的兼容兜底。
实现：`src/system/compat/ShellProxy.ts`、`src/system/compat/compatibility.ts`、`src-tauri/src/shell/compat.rs`。

## 1. 目标与原则

兼容核不重新实现 Windows，也不通过猜测扩展名、手拼路径或 `cmd /C start` 来模拟 Windows。桌面层只提出“打开这个 Shell 项”或“显示这个系统菜单”，真正的决定交给宿主 Shell。这样 `.exe`、`.lnk`、文件夹、URI、文件关联和 AppX 能继续使用宿主已有的注册表与 Shell 扩展；Variable 只负责生命周期、错误提示和必要的安全降级。

这条边界也保护了执行档隔离：没有环境变量重定向的普通登记软件走 ShellExecuteExW；需要 `{container}`、`HOME`、凭据目录等注入的受管工具仍由既有 `spawn_profiled` 使用 CreateProcess 族启动。不能为了表面上的“Shell 兼容”而把凭据重新放回宿主。`.lnk` 与 UWP 的 Shell 通道无法承诺执行档环境注入，界面和日志会如实标明这一点。

## 2. Shell 代理

`ShellProxy.ts` 是 React 侧唯一入口，提供五类操作：

1. `shellExecute` / `openWithWindows`：普通打开、协议关联、目录和快捷方式；
2. `runAsAdministrator`：显式 `runas`，UAC 同意框由 Windows 展示，用户取消会回传错误；
3. `activateApplication`：按 AUMID 调用 Store/UWP 激活通道，独立窗口是已知边界；
4. `getShellIcon`：请求 64px Explorer 风格 Shell 图标；
5. `showNativeContextMenu`：把单个文件或目录交给原生 IContextMenu，失败时返回 `shown=false`。

代理不接受一整条命令行，也不拼接引号。参数通过 Tauri 调用的独立字段传递；后端把路径、动词、参数、工作目录分别写入 `SHELLEXECUTEINFOW`。失败码 2、3、5、1155 分别转换成文件不存在、路径不存在、访问拒绝和没有关联程序的可读提示。

## 3. 原生图标与右键

图标链沿用资源管理器语义：首先提取可用的嵌入图标，随后使用 `SHCreateItemFromParsingName` 与 `IShellItemImageFactory::GetImage(SIZE{64,64})` 取得 Shell 项图像。快捷方式、文件夹或特殊 Shell 项不应因为没有普通 exe 资源就显示空白；失败时桌面仍使用通用图标，不制造一张假的品牌图。

Explorer 行的右键先尝试原生 `IContextMenu::QueryContextMenu` 和 `TrackPopupMenuEx`。因此 7-Zip、Git、Tortoise 等已经安装在宿主上的扩展可以自然出现。菜单由 Windows 阻塞式展示，选择后通过 `InvokeCommand` 执行。原生菜单当前按单选项启用；多选、非 Windows、Shell 项解析失败均返回能力失败，前端回退到原来的安全菜单。回退不是静默吞错：它只表示当前项目不能获得宿主扩展菜单。

## 4. 真 Windows 行为

Win+D、Win+方向键和 Alt+Tab 走 compat 后端的单一虚拟键路径，交给 Windows 输入栈、Explorer 和 DWM。Variable 的 VWM 只在自己的虚拟窗口集合内同步状态，不绘制一个声称等同于系统 Task View 的假界面。Alt+Tab 被 Windows 抢占时，Variable 保留 Win+Tab 切换器作为透明降级；WebView 获得焦点且系统未抢占时，既有 VWM 轮转仍然可用。

行为透传不等于强行接管宿主。公司策略、远程桌面、焦点窗口、管理员完整性级别和第三方全屏软件都可能使全局组合键不可用。此时只关闭 Variable 自己的虚拟窗口或提示用户，不向宿主注入额外 DLL，也不冒险修改 DWM 设置。

## 5. 兼容数据库与重试

`compatibility.ts` 保存可审阅的应用条目。目前覆盖 Blender、Photoshop、Wallpaper Engine 和 Steam，并允许继续追加 200 软件矩阵。条目只描述兼容建议：WIN10/WIN7/native、DPI aware/unaware、参数和备注，不偷偷改变系统设置。

重试序列由代理导出为明确的选择计划：

`普通 Shell → 用户确认的 runas → WIN7RTM → DPIUNAWARE`。

runas 永远不能自动绕过用户确认。WIN7RTM 和 DPIUNAWARE 是执行档环境变量层的选择建议，真正应用时应写入该应用自己的兼容配置并记录日志。反作弊游戏、需要驱动的软件、写死系统目录的安装器仍显示“需 B 模式/宿主安装”的真实边界。

## 6. 验收口径

- 普通 `.exe`、`.lnk`、文件夹、`steam://` 和文件关联不再通过 `cmd /C start`；
- runas 由 Windows UAC 处理，取消不会被假报为启动成功；
- UWP/AUMID 能调用系统激活管理器，不能承诺嵌入 VWM；
- 64px Shell 图标失败有通用图标回退，不出现空白 tile；
- Explorer 单文件右键可以出现宿主 Shell 扩展；能力不具备时 Variable 菜单仍可用；
- Win+D/Win+方向键不自绘 DWM，普通路径把动作交给宿主；
- `npm run typecheck`、前端单测、`cargo test --workspace` 与 `node tools/audit.cjs` 作为合入门禁。当前沙盒未安装 Node/Rust 工具链，GUI、真实 UWP、7-Zip 菜单和三宿主矩阵仍需 Windows 实机点验，不能将代码层接线描述为实机 100% 验证。

## 7. 兼容核的安全边界

Shell 扩展是宿主代码，Variable 不把它们加载进 WebView，也不将第三方 DLL 作为自己的插件加载。原生菜单只在用户明确右键并选择时调用；启动错误不会删除登记项、不会杀掉已有进程、不会修改宿主注册表。Shell 代理是一个窄的系统边界，而不是一个绕过隔离或权限模型的后门。
