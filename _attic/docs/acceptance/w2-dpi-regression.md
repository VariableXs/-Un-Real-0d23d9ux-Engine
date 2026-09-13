# W-2 嵌入 DPI 修正 — 混合 DPI 回归清单（10 款）

> 批次：W-2（主计划 6.2，AI-2 窗口路）
> 口径：100% 主屏 + 150% 副屏（可对调），嵌入窗口跨屏拖动错位 ≤ 2px 为过；
> 不响应 `WM_DPICHANGED` 的应用登记 `dpiFix=false`（按主屏渲染，如实标注轻微模糊）。
> 状态均为 `[ ]` 真机待测——**本环境无多显示器真机，不臆造结论**。

## 实现侧（已完成，代码证据）

- `src-tauri/src/shell/embed.rs`：`ensure_per_monitor_dpi()`（PER_MONITOR_AWARE_V2 校验）、
  `window_dpi()`（MonitorFromWindow 实际 DPI）、`forward_dpichanged()`（跨屏转发 WM_DPICHANGED）；
- 会话结构含 `dpi_fix` / `last_dpi` 字段，登记表可按应用关掉 DPI 转发（例外清单）；
- 前端 bounds 上报链路：`window.devicePixelRatio` 换算物理像素（vwm.ts 上报通道）。

## 真机回归清单（10 款）

| # | 软件 | 选择理由 | dpiFix | 状态 | 备注 |
|---|---|---|---|---|---|
| 1 | 记事本 (notepad.exe) | 系统标准 Win32，Per-Monitor V2 基准 | true | [ ] | 基准项 |
| 2 | 记事本++ (Notepad++) | 常见自绘部分 UI 的编辑器 | true | [ ] | |
| 3 | Chrome | 多进程 + 自有 DPI 处理 | true | [ ] | 联动 C-1 子窗口回归 |
| 4 | Firefox | 与 Chrome 不同的 DPI 处理族 | true | [ ] | |
| 5 | VS Code | Electron（Chromium 合成层） | true | [ ] | |
| 6 | OBS Studio | Qt 自绘（L2 层级联动） | true | [ ] | 嵌入走 L2 宿主 |
| 7 | 微信 / WeChat | 常见自绘壳 + 高频用户软件 | true | [ ] | |
| 8 | PotPlayer | 旧式 Win32 自绘边框 | 预期 false 候选 | [ ] | 若不响应 WM_DPICHANGED → 登记 dpiFix=false |
| 9 | Snipaste | 轻量自绘 + 常驻托盘 | 预期 false 候选 | [ ] | 同上 |
| 10 | 计算器 (UWP) | 走 L3 画面捕获路径 | L3 不适用 | [ ] | L3 像素流无 DPI 重排问题，仅记录 |

## 测试步骤（真机）

1. 主屏 100% / 副屏 150%（或对调），Variable 运行于 Per-Monitor V2；
2. 每款软件经 VWM 嵌入后，在主副屏间往返拖动 ≥ 3 次；
3. 截图比对嵌入窗口边界与 VWM 虚拟窗口边界的偏差（> 2px 即 fail，登记归因）；
4. 不重排/模糊者：登记表 `dpiFix=false`，在备注列如实标注「按主屏渲染，轻微模糊」。

## 归档位置

- 结果回填本表（真机执行后）；
- tier/captureMs 数据另见 `compat-embed-results.json`（`Compat-Matrix.ps1 -Action Run-Embed`）。
