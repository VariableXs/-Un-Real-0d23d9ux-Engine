# Changelog

本文件记录面向用户与协作者的显著变更。批次级细节见 `project_memory.md`；
架构与计划见 `docs/BLUEPRINT-1.0sno9u.vxe.md` 与 `docs/MASTER-PLAN-1.0sno9u.vxe.md`。

## [Unreleased] — 1.0sno9u.vxe 工作线

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
