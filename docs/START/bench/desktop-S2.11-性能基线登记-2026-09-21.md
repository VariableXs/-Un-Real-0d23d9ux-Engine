# S2.11 性能基线登记 · AI-4 桌面承载（2026-09-21 建立）

> **性质**：三项基线（首帧/交互延迟/窗口开关）的**登记设施与初值来源**。
> **回归门禁规则**（S2.11 定版）：同机三次方差 ≤5% 方可入册；后续 >10% 回归即门禁红。
> **现状**：QEMU 层首帧实测值已登记（usrshell 桌面）；真机三项基线待 S4.1（USB 键鼠）后按《实施总步骤图》1.8 SOP 上机采集。

## 1. 基线项定义（口径与采集点）

| 基线项 | 口径 | 采集点 | 门禁来源 |
|---|---|---|---|
| 首帧 | 引导 → 桌面首帧可交互 | 内核串口 `SHELL: first-frame ms=`（spawn 链 tsc 实测） | docs/acceptance/ 引导走查 |
| 交互延迟 | 输入事件发布 → 前端交付 | inputsvc 延迟打点 `latency_ticks()`（TSC 周期，publish→poll；目标 <30ms） | 本文件 §3 + inputsvc.rs |
| 窗口开关 | SYS_WIN register→submit→composite→unregister 全链 | winsurf 统计 `stats()`（frames/blit_rows）+ 宿主 ktest 计时 | ktest winsurf 组 |

前端侧持续采样（Windows/内核双形态同一份代码）：`perfBaseline.ts` 首帧 + 交互延迟滚动窗口（P95 口径，localStorage `variable:perf:baseline:v1`，零采集红线：只记时长不记内容）。

## 2. QEMU 层首帧实测初值（2026-09-21，q35 / 1280x800 / 默认 TCG）

| 轮次 | 首帧 ms | 来源 |
|---|---|---|
| 本会话走查 | 1889 | `_attic/qemu-winsurf-serial.log` `SHELL: first-frame ms=1889` |

> TCG 时间膨胀下该值仅作回归相对基准（同环境同口径对比），不代表真机水平。

## 3. 交互延迟采集设施（已就位，待真机数字）

- 内核侧：`inputsvc::InputService::enable_tsc()`（实机启用）→ `latency_ticks()` 读 (last, max, samples)；打点位置 = publish（硬件泵入队）→ poll_with_seq（订阅者交付）。
- 换算：TSC 周期 ÷ `platform::info().tsc_hz` = μs（实机探针按 leaf 15/16 校准，FALLBACK_TSC_HZ 兜底）。
- 验收阈值：均值与 P95 均 < 30ms（S2.05 验收口径）。

## 4. 真机采集清单（S2.11 关闭条件）

1. 真机（Legion Y7000 IRX9）三轮首帧/交互延迟/窗口开关，三次方差 ≤5% → 入册本文件。
2. 1280×800 与 2560×1440 双分辨率各一轮（S2.06 双分辨率口径）。
3. 基线入册后交 AI-6 常态化执法（>10% 回归即红）。
