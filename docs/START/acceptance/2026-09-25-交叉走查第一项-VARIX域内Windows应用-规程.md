# 交叉走查第一项 · VARIX 域内运行 Windows 应用（验收规程）

**登记日**：2026-09-25 ｜ **归属**：m3-compat 实机欠账四项之一（交叉走查） ｜ **承接**：AI01 ｜ **窗口执行人**：Variable

## 目标一句话

在 VARIX 内核域的桌面上，通过 Wine 支架把一个预置 Windows 应用（notepad-classic）真实跑起来、关掉、清算干净——产生第一份实机交叉走查证据，关掉 m3 实机欠账清单的第一格。

## 判据绑定（宿主侧已收口，本项补实机证据）

| 判据 | 内容 | 实机证据形态 |
| --- | --- | --- |
| B-1001 | wineserver 看门狗 | 启动请求后 wineserver 惰性拉起日志一行；杀 wineserver 后 ≤5s 重建日志一行 |
| B-1002 | 前缀模板可追溯 | `/home/wine/notepad-classic/` 目录存在且含模板版本号 ≥1 |
| B-1003 | 垫片零侵入 | 走查报告里 WINE_SOURCE_DIFF_LINES=0（常量面，实机复读一遍） |
| B-2103/评级 | 星卡如实 | 启动页显示 notepad-classic 评级 partial，不虚标 |
| 走查本体 | 窗口真出现 | VARIX 桌面截图（display-shot 系列 PPM）中可见 notepad 窗口 |
| 走查收尾 | 会话清算 | 关闭后无残留进程行、清算日志一行 |

## 实机六步（Variable 在窗口期照做，全程 ≤ 15 分钟）

1. **进 VARIX 域**：U 盘启动 → Limine 菜单选 VARIX（或在 Windows 域触发 OneShot 单次引导变量）。
2. **开走查采集**：usrshell 执行走查采集开档（见下方命令），确认快照卷可写。
3. **发起启动**：从应用清单选 notepad-classic（channel=wine）启动——第一请求即触发 wineserver 惰性拉起与前缀实例化。
4. **留证一**：窗口出现后截屏两张（display-shot 机制，PPM 落快照卷）。
5. **压力一步**：杀 wineserver 进程，计时观察重建（判据 ≤5 秒），截重建后窗口恢复态一张。
6. **收尾**：正常关闭 notepad → 截清算日志 → 关机回 Windows 域。

## 证据回收（Windows 域，脚本自动化）

回 Windows 后运行 `tools/vx-walkcheck-wine.py`：自动探测快照卷（标签 SNAPSHOT/VARIX_SYS）→ 收集 PPM 截图、wineserver 日志、前缀目录清单 → 逐判据核对 → 生成 `docs/acceptance/2026-09-XX-交叉走查第一项/证据包.json` 与 Markdown 报告 → 在工作包台账 m3 段登记结果（绿/红如实）。

## 纪律

- 全程零 Wine 源码触碰（B-1003 红线）；垫片失效只许如实登记，不许现场修。
- 任何一步失败：截图 + 日志照收，报告记红——**失败的走查也是走查**，静默才是事故。
- 本项只验收不施工：过程中发现缺陷登记欠账，不顺手改内核。
