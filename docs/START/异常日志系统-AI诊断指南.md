# 异常日志系统 · AI 诊断指南

> 目标：AI（或人）在用户报告"某功能异常"后，按本指南在 **1 分钟内**定位到：哪个功能、哪条链路、崩溃前发生过什么、下一步改哪个文件。
> 本文档描述的是**功能现状**（采集 → 存储 → 导出全链路），不是施工计划。

## 1. 全链路架构

```
┌─ 前端（每窗口，src/lib/logger.ts 统一门面）──────────────────────┐
│                                                                   │
│  console.warn/error ─┐（40+ 处既有调用零改造，[tag] 前缀自动提取） │
│  window.error        ─┤→ errBoard 环形 500 条（PII 清洗，M-79）    │
│  unhandledrejection  ─┤→ sessionNarrative 30s 心跳（U-23 崩溃叙事）│
│  logError() 显式出口 ─┤→ 统一时间线环形 600 条（logger.ts）        │
│                       └→ ipc.log("error", "[<sid>] tag: msg")     │
└──────────────────────────────┬────────────────────────────────────┘
                               │ log_frontend 命令
┌─ Rust（src-tauri）────────────▼───────────────────────────────────┐
│  applog 总线（shell/applog.rs）：tag=fe:<level> 与 launch/embed…   │
│  同一总线 → ① 环形 600 条  ② sys://applog 事件实时推送             │
│           → ③ <数据目录>/logs/applog-YYYYMMDD.log 按天落盘         │
│  variable.log（state.rs）：[<level>] msg 跨会话落盘                │
│  M-53 崩溃转储（shell/perf.rs）：crashes/crash-<ts>.dmp + .narrative│
└───────────────────────────────────────────────────────────────────┘
```

### 接线点（唯一）

- `src/entries/runtime.ts` → `setupEntryRuntime()` → `installLogCapture(entry)`。
  7 个窗口入口（desktop / app-write / app-mind / app-code / app-fate / explorer / datavault）全部经过此处，一次接线全量覆盖。
- `src/components/ErrorBoundary.tsx` → `logError("ErrorBoundary", …)`：React 渲染错误统一出口。

## 2. 存储位置（排查时按序读取）

| # | 存储 | 位置 | 内容 | 生命周期 |
|---|------|------|------|----------|
| 1 | 统一时间线 | 内存（每窗口 600 条）| debug/info/warn/error 全级别 + 模块 tag | 本会话 |
| 2 | 时间线尾部 | localStorage `variable:log:v1`（尾部 200 条）| 崩溃前现场 | 跨会话 |
| 3 | errBoard | localStorage `variable:errboard:v1`（尾部 200 条）| 近 7 天异常聚合（PII 已清洗）| 跨会话 |
| 4 | 崩溃叙事 | localStorage `vxs:sessionNarrative`（事件 20 条 + lastAlive）| 上次会话异常退出现场 | 跨会话 |
| 5 | variable.log | `<数据目录>/logs/variable.log` | 前端 error 级转发（含 `[<sid>]` 前缀）| 永久 |
| 6 | applog 按天 | `<数据目录>/logs/applog-YYYYMMDD.log` | 启动/嵌入链路 + `fe:<level>` 前端转发 | 按天 |
| 7 | 崩溃 dump | `<数据目录>/crashes/crash-<ts>.dmp`（保留 10 份）| Win32 MiniDump | 永久 |

`<数据目录>`：`%APPDATA%/com.variable.app`（安装版）或 exe 同目录（便携版，标记 `.portable`）。

## 3. 会话关联（sessionId）

- 每窗口每次加载生成 8 位十六进制 `sessionId`（`logger.sessionId()`）。
- 出现位置：诊断报告头部、`variable.log` 每条前端转发消息的 `[<sid>]` 前缀。
- 用途：把 variable.log / applog-*.log / 前端时间线对齐到同一次运行；多窗口各有一个 sid。
- 已知局限：多窗口共用 localStorage 键，`vxs:sessionNarrative` 与时间线尾部为"最后写入窗口"所有；跨窗口排障以 variable.log / applog 文件为准。

## 4. AI 标准排查流程

1. **要报告**：让用户打开 设置 → 质量与诊断 →「复制诊断报告」（按钮在有日志记录时可用），直接粘贴到对话。
   报告 = errBoard 近 7 天 Top10 聚合 + 上一会话日志尾部（崩溃前现场）+ 本会话时间线（最近 300 条，含级别/标签/时间戳）。
2. **对文件**（用户可提供或应用可自读时）：
   - 前端异常 → `variable.log` 搜 `[<sid>]`；实时状况 → 任务管理器「日志」页（`fe:` 开头 tag 即前端转发）。
   - 打不开/启动失败 → `applog-*.log` 搜 `launch` / `embed` / `adopt` / `watchdog`。
   - 闪退无日志 → `<数据目录>/crashes/` 是否有新 dump（`ipc.perfCrashDumps()` 可列）。
3. **定位代码**：时间线每条 `HH:MM:SS.mmm [LEVEL] [tag] msg`，`tag` 即 console 输出首参的 `[tag]`（对应模块）或 `logger.logError(tag,…)` 的模块名；errBoard 条目附栈顶 `src/…:行号`。
4. **修复后验证**：新日志级别事件用 `logInfo/logWarn/logError`（`src/lib/logger.ts`）打点，下一轮报告即可对比前后行为。

## 5. 各角色 API 速查（src/lib/logger.ts）

| API | 时间线 | console 镜像 | errBoard | Rust 落盘 |
|-----|:---:|:---:|:---:|:---:|
| `logDebug(tag, msg)` | ✅ | — | — | — |
| `logInfo(tag, msg)` | ✅ | — | — | — |
| `logWarn(tag, msg)` | ✅ | console.warn | — | — |
| `logError(tag, msg, stack?)` | ✅ | console.error | ✅ | ✅（`fe:error` 实时总线）|
| 裸 `console.error("[tag] …")` | ✅（自动）| 原样 | — | — |
| window 异常 / Promise 拒绝 | ✅ | `[Variable] uncaught` 原样 | ✅ | —（下次崩溃叙事可查）|

## 6. 已知缺口（后续完善方向）

- 内核（no_std）panic 只打 `file:line` 到 framebuffer，不落 RingLog/串口消息正文；用户态 panic（kernel/userspace）直接 `exit(101)` 丢弃信息。
- Rust 侧仍有部分模块直接 `eprintln!`，不经过 applog 总线（前端不可见）；逐步收敛为 `crate::shell::applog::log`。
- 三套错误体系（AppError 字符串码 / DeskError 枚举 / 内核 kerror!）尚未建立映射表。
- 时间线为每窗口独立实例，VWM 内嵌视图跟随宿主窗口。
