# AI-1 · 真机冷启动 ×10 与交替引导 SOP（用户在场执行卡）

> **定位**：S1.4/S1.5 与「连续 10 次冷启动演练」的真机执行卡（2026-09-22）。
> QEMU 级演练已由 `_attic/t7-t8-qemu-campaign.py` 完成（见 ai5 验收记录附录）；
> 本文是真机（物理整机）层的执行卡——**每一步都遵守《实施总步骤图》1.8 SOP**。

---

## 0. 上机前 7 项（AI-6 执法，缺一不上机）

1. `Confirm-SecureBootUEFI` = Disabled（BIOS 已关，PTCN07WW）；
2. HiberbootEnabled=0 已回读确认（S0.4 完成 ✓）；
3. BitLocker 状态已记录；涉及反复重启已 `Suspend-BitLocker -RebootCount 0`；
4. BootOrder 已截图归档；
5. 内置盘 Windows 引导四件套 hash 基线已记录（`_attic/readonly-*` 只读体检范式）；
6. 本卡即步骤卡（动作/验证/回滚三栏齐）；
7. 用户知情确认三句话：做什么（交替引导 10 次）、最坏情况（U 盘引导失败，
   拔盘即回内置盘 Windows）、怎么回来（物理拔盘，永不失效）。

## 1. 前置部署（一次性）

- isoroot 已含新内核（last_boot 写回 + 壁纸 + AHCI）与 wallpaper.rgb565；
- 管理员运行 `_attic/esp-deploy-switch.py`（UAC 允许）→ 日志应含
  `ESP-DEPLOY-DONE` + `kern hash verified` + `wallpaper deployed hash=…`；
- 回滚锚点：`L:\limine.conf.bak` + esp-backup（F4F96E69 链）。

## 2. 连续 10 次冷启动演练（交替入口）

| 轮次 | 入口 | 动作 | 验证（每次） |
|---|---|---|---|
| 1-3 | F12 选 U 盘 → VARIX 卡 | 冷开机 → 默认倒计时进 varix | ushell 桌面点亮；**壁纸=WE 同款画面**；串口/取证可选 |
| 4-6 | 同上 → handoff | 冷开机 → 默认 → 交接画面 2.4s → 自动重启 → U 盘 Windows | Windows 自启 Variable 全屏；BootNext 清零（`bcdedit /enum firmware` 回读） |
| 7-8 | F12 → WINDOWS 卡 | 冷开机 → 选 Windows 卡 | U 盘 Windows 引导（自包含） |
| 9-10 | 拔盘 → 冷开机 ×2 | 验证内置盘 Windows 原生引导 | 每轮后插回 |

- **每轮结束跑一次日志器**（管理员不需要，W: 可写）：
  `python _attic/t8-realboot-log.py VARIX`（或 WINDOWS）——
  追加时间戳+入口+U 盘在场到 `W:\SHARED\varix-bootlog.txt`。
- **验收口径**：10/10 引导成功 + 日志连续 + 每次交接后 BootNext 清零 +
  BootOrder 与基线一致。

## 3. last_boot ×5 交替验收（真机形态）

- 交替进入 VARIX / Windows 各 ≥2 轮后（合计 ≥5 次进入），
  读 `W:\SHARED\boot-select.json` 的 `last_boot` 值：
  **每次进入后该值必须等于最近一次实际进入的系统**。
- QEMU 级 ×5 交替已在 `_attic/t7-t8-qemu-campaign.py` P2 完成（镜像 raw
  断言），真机形态复用同一断言口径（文件内容级）。

## 4. 下机后 4 项（AI-6 执法）

1. 拔盘冷启动进原生 Windows ×1；
2. 四件套 hash 与基线一致；
3. BootOrder 与基线一致（BootNext 已清零）；
4. 本卡执行记录（时间线/异常/日志器输出）追加到本目录验收档案。

## 5. 回滚

- U 盘引导失败：拔盘（物理兜底）→ 内置盘 Windows；
- U 盘内核回刷：esp-backup（F4F96E69 链）或 `L:\limine.conf.bak`；
- handoff 关闭：`W:\SHARED\boot-select.json` 的 `handoff` 改 false。
