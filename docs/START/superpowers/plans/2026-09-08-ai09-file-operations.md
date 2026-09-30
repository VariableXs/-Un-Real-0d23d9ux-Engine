# AI-09 文件操作组（16 项）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 ENGINE Version 01XHI9DN.1.5xw 分工图中 AI-09 文件操作组的全部 16 项（Z-29…Z-35、M-19…M-27），只读优先、写操作显式确认+可撤销。

**Architecture:** 新建 Rust 模块 `src-tauri/src/shell/fileops.rs` 承载全部新后端命令（AI-09 领地「fileops 模块」），在 `lib.rs` 注册；前端在 `src/system/explorer/**` 与 `src/system/tools/` 增量扩展，4 个新工具（批量重命名/重复报告/空间分析/校验和）走 VWM 工具窗口体系；设置页新增「文件管理器」tab。共享文件（`ipc.ts`、`lib.rs`、`vwm.ts`、`StartMenu.tsx`、`dictionaries.ts`、`desktop.css`、`tools.css`、`settings.ts`）最小 diff。

**Tech Stack:** Tauri 2 + React 18 + TypeScript（前端）；Rust（后端，windows crate / blake3 / sha2 + 新增 md-5、sha1、zip）；vitest（前端测试）、cargo test（后端测试）。

**红线（承原计划，全程有效）：**
- 任何文件操作只读优先；写操作必须显式确认 + 可撤销；绝不静默删除。
- `src/apps/write|mind|code|fate/**` 零修改（隔离声明）。
- M-25 无强拆按钮；查不到占用者如实显示「系统未披露占用者」。
- M-27 SMB/网络路径入口校验拒绝。
- 默认即现状：所有新设置默认值 = 当前行为。

---

## 任务清单

### Task 1: 后端 fileops.rs — 校验和 / 重复报告 / 空间扫描 (M-21 / Z-33 / Z-34)

- [ ] `src-tauri/src/shell/fileops.rs`：
  - `checksum(path, algo)`：algo = md5|sha1|sha256|blake3；1MB 分块流式；进度事件 `checksum://progress`；可取消（`checksum_cancel(opId)`，AppState 内存取消标志表）
  - `dupe_scan(path, minSize)`：按大小分组 → 前 4KB 抽样哈希 → 全量哈希确认；事件 `dupe://progress`；返回分组（只报告不删除）
  - `space_scan(path)`：目录树递归统计（大小/文件数/目录数，上限 20 万项截断如实返回 truncated）
  - cargo test：MD5/SHA-1/SHA-256/BLAKE3 已知向量；dupe 分组逻辑
- [ ] Cargo.toml 新增 `md-5 = "0.10"`、`sha1 = "0.10"`、`zip = { version = "2", default-features = false, features = ["deflate"] }`

### Task 2: 后端 fileops.rs — 批量重命名 / 发送到 / 网络驱动器 (Z-32 / Z-35 / Z-31)

- [ ] `batch_rename_preview(paths, ops)`：规则管线（查找替换 / 序号 start+step+pad / 大小写 upper|lower / 扩展名替换），预览返回 {old,new,conflict}，冲突标红（目标已存在或同批重名）
- [ ] `batch_rename_apply(ops)`：执行前原子校验全部目标可写；写撤销日志 `data_dir/rename_undo/<ts>.json`；返回 undoId
- [ ] `batch_rename_undo(undoId)`：反向恢复
- [ ] `sendto_list()`：合并系统 SendTo 目录（%APPDATA%\Microsoft\Windows\SendTo）+ 自定义目标（`data_dir/sendto_custom.json`）+ 最近 3 目标
- [ ] `sendto_custom_add/remove(path)`；执行 = 复制到目标（复用 explorer.rs copy_recursive）
- [ ] `net_drives()`：GetLogicalDriveStringsW + GetDriveTypeW，返回每盘 {letter,path,kind,available}（网络盘离线灰化依据）
- [ ] cargo test：重命名规则管线纯函数（序号/替换/大小写/扩展名）+ 冲突检测

### Task 3: 后端 fileops.rs — 锁定侦探 / 压缩包 / 目录哨兵 (M-25 / M-23 / M-27)

- [ ] `who_locks(path)`：Windows Restart Manager API（RmStartSession/RmRegisterResources/RmGetList）；返回 [{pid,name,title}]；查不到 → 空列表（前端如实文案）；Cargo.toml windows features += Win32_System_RestartManager
- [ ] `archive_ls(path)`：zip 只读清单（zip crate）；7z/tar → 明确错误「暂不支持」；返回 {name,size,compressedSize,isDir}
- [ ] `archive_extract_one(archive, innerPath)`：解到 `data_dir/tmp/archive/`（启动时清理该区），返回解出文件路径 → 前端 openPath
- [ ] `sentinel_list/add/remove/toggle`：哨兵表持久化 `data_dir/sentinels.json`（{id,path,enabled,quietHours}，上限 5）
- [ ] ReadDirectoryChangesW 每哨兵一线程；1s 窗口节流合并；事件 `sentinel://event` {path,changes:[{kind,name}]}；网络路径（\\ 开头）入口拒绝
- [ ] cargo test：who_locks 自锁用例（本进程打开的临时文件 → 锁定者=自己）；哨兵事件合并逻辑（纯函数抽取）

### Task 4: 命令注册 + 共享文件最小 diff

- [ ] `src-tauri/src/shell/mod.rs` += `pub mod fileops;`
- [ ] `lib.rs` generate_handler 注册全部新命令
- [ ] `src/lib/ipc.ts` 增加对应包装（camelCase 参数）+ Shell 命名空间类型
- [ ] `node tools/audit.cjs` 全绿

### Task 5: 前端工具窗口 (Z-32 / Z-33 / Z-34 / M-21)

- [ ] `src/system/tools/RenameApp.tsx`：规则编辑（替换/序号/大小写/扩展名）+ 预览 diff 表（冲突红）+ 应用 + 撤销按钮（最近一次 undoId）
- [ ] `src/system/tools/DupeApp.tsx`：选目录 → 扫描（进度条）→ 分组报告（虚拟滚动 CSS overflow 即可）+ CSV/JSON 导出 + 「只报告不删除」明示
- [ ] `src/system/tools/SpaceApp.tsx`：目录树扫描 + Canvas treemap（squarified 算法）+ 排序列表双视图联动 + 快照对比（保存快照到 toolDataWrite，选两快照 diff）
- [ ] `src/system/tools/ChecksumApp.tsx`：文件路径 + 算法四选 + 进度条 + 期望值比对（绿勾/红叉）+ 复制结果
- [ ] `src/system/windows/vwm.ts`：VwmToolApp += "rename"|"dupe"|"space"|"checksum"（含默认几何 + 标题）
- [ ] `src/system/windows/VwmAppContent.tsx`：挂载四个新组件
- [ ] `src/system/startmenu/StartMenu.tsx`：TOOL_DEFS += 四项
- [ ] vitest：重命名规则管线（若放纯函数模块 `src/system/tools/renameRules.ts`）、squarified treemap 纯函数、dupe 分组展示逻辑

### Task 6: ExplorerWindow 增强 (Z-31 / Z-35 / M-24 / M-20 / M-26 / M-25)

- [ ] Z-31 侧栏：三段重构（快速访问/常用/网络）；「常用」= 访问历史 top6（settings `exRecentDirs`）；网络驱动器单独段（net_drives() 离线灰化 disabled）
- [ ] Z-35 右键「发送到」子菜单：合并系统 SendTo + 自定义 + 最近 3 目标置顶；执行 = 复制 + toast；「添加自定义目标」（目录选择复用 PromptHost 输路径）
- [ ] M-24 书签条：地址栏下方 placePins（≤12，LRU by placeStats 访问计数）；拖拽文件夹到条 = 置顶；右键（新标签/系统打开/取消置顶）；满 12 提示
- [ ] M-20 同名选择记忆：批量粘贴结束 toast 汇总（替换/跳过/保留两者计数）；「保留两者」toast 中给出新名预览；冲突对话框对剩余全部应用改为勾选式（保留原按钮语义不变）
- [ ] M-26 删除档位：settings `deleteMode: env|system|ask`（默认 env = 现状 intern_path；system = SHFileOperationW 入 Windows 回收站；ask = 每次弹选择）；后端 ex_trash 增可选 target 参数
- [ ] M-25 占用查看：ex_copy/ex_move/ex_delete 失败 toast 加「查看占用」按钮 → who_locks 浮窗（进程名/pid/标题 + 刷新 + 在任务管理器查看 + 无强拆）
- [ ] vitest：placePins LRU 淘汰、M-20 汇总计数纯函数

### Task 7: QuickPreview 增强 (Z-29 / M-22)

- [ ] Z-29 格式扩展：字体预览（ttf/otf/woff2 @font-face + A-Z 样张）；zip 清单（archive_ls 只读列表）；>100MB 非 media 拦截提示
- [ ] M-22 键导航：←/→ 相邻文件切换（由 ExplorerWindow 传入列表上下文）；↑/→ 层级导航（进入/返回目录）
- [ ] vitest：格式白名单矩阵（支持/降级两态）

### Task 8: 右键菜单注册表 + 设置页 (M-19 / Z-30 / M-27)

- [ ] `src/system/explorer/ctxMenu.ts`：行菜单项注册表（id/i18nKey/danger/group/conditionFn）
- [ ] ExplorerWindow 行菜单改读注册表 + settings `ctxMenuOrder`（显隐+顺序）覆盖
- [ ] SettingsModal 新 tab「文件管理器」：
  - Z-30 打开方式管理：扩展名 × 默认应用 × 候选（file_assoc_list/set/remove）+ 操作日志（toolDataWrite 追加）
  - M-19 右键菜单编辑器：显隐勾选 + ↑↓ 排序 + 恢复默认
  - M-26 删除档位三选 + 清空回收站前确认（显示最近 10 项可恢复清单）
  - M-27 哨兵管理：路径列表增删启停
- [ ] vitest：注册表完整性（全部动作有 i18n 与处理器）

### Task 9: i18n + CSS

- [ ] `dictionaries.ts` zh/en 全部新键（zh 双复核：audit.cjs i18n 段全绿）
- [ ] `desktop.css`：侧栏三段/书签条/发送到子菜单/占用浮窗/删除档位样式
- [ ] `tools.css`：四工具样式（复用既有 tool 卡片视觉语言）

### Task 10: 验证 + 收口

- [ ] `npm run typecheck` 全绿
- [ ] `npm test` 全绿（新增测试 + 既有回归）
- [ ] `node tools/audit.cjs` AUDIT PASSED
- [ ] `cargo check --lib` + `cargo test --lib` 全绿
- [ ] 隔离核查：`git diff --stat` 确认 `src/apps/` 零改动
- [ ] 验收文档 `docs/acceptance/ai09-验收.md` 归档（未实测不勾 ✅）
- [ ] git init + commit + push GitHub `-Un-Real-0d23d9ux-Engine`

---

## 已存在能力（软依赖，不重复建设）

- M-20 冲突对话框基础（replace/skip/keep + -all）已在 ExplorerWindow.tsx —— 增量补 toast 汇总与保留两者预览。
- M-22 空格快速预览骨架已在 QuickPreview.tsx —— 增量补键导航与白名单矩阵。
- M-26 环境回收站基座已在 recycle.rs（intern_path/rec_list/rec_restore/rec_empty）—— U-27 回收站 2.0 未交付不影响本批（软依赖：现有基座满足 M-26 需求）。
- Z-30 文件关联命令已在 ecosystem.rs（file_assoc_list/set/resolve/remove）—— 只做设置面板 UI。
