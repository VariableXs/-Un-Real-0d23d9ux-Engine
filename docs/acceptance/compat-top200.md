# C-8 兼容矩阵实测回填 — compat-top200（AI-2 窗口路）

> 批次：C-8（主计划 7.8）
> 工具：`portable/AI5/Compat-Matrix.ps1 -Action Run-Embed / Report-Embed`（批次 C-8 新增嵌入验证段）
> 矩阵：`portable/AI5/Data/compat-matrix.json`（Top200，5 类 × 40）

## 已完成（代码/工具级）

- [x] `Run-Embed`：逐软件启动 → 等主窗口 → 窗口样式探测分层（决策树镜像 `compat_probe.rs`）；
      记录 `{ tier, captureMs, crash, inputOk, dpiOk }` → `compat-embed-results.json`；
- [x] `Report-Embed`：生成嵌入验证 Markdown 报告 + 不通过项归因表；
- [x] 口径诚实：`inputOk` / `dpiOk` 一律输出 `todo`——这两项必须在 Variable 真机嵌入会话中
      人工观察后回填，脚本不臆造；
- [x] 已验证（本机 PowerShell 5.1 真跑）：语法解析 OK、空选集 exit 0、
      未安装条目 skip 路径 OK、报告生成 OK。

## 真机待测（本环境无 Variable 真机嵌入会话，全部待办）

- [ ] Top200 五类（办公/开发/图形/游戏平台/IM）逐类真机实测回填；
- [ ] 回归门槛复核：办公类 ≥ 95% L1、图形类 ≥ 80%（L1+L2）、UWP 类 ≥ 90%（L3）；
- [ ] `inputOk` / `dpiOk` 人工观察回填（含 W-2 混合 DPI 清单联动）。

## 执行命令（真机）

```powershell
portable\AI5\Compat-Matrix.ps1 -Action Run-Embed                 # 全量
portable\AI5\Compat-Matrix.ps1 -Action Run-Embed -Category office # 分类
portable\AI5\Compat-Matrix.ps1 -Action Report-Embed               # 报告
```
