# portable 套件自检（AI-5 交付核）

`Run-PortableTests.ps1` 是整个 `portable/` 目录（AI-1 到 AI-5 的 PowerShell 脚本 + AI-5 的数据文件）的自检入口。
它做三件事，全部作用于**仓库里的真实文件**，没有任何替身实现：

1. **语法**：用 PowerShell 官方解析器 `[System.Management.Automation.Language.Parser]::ParseFile` 解析
   `portable/` 下每个 `.ps1`，有 `ParseError` 即失败并打印行号与消息。
2. **数据不变量**：校验 `portable/AI5/Data/compat-matrix.json` 与 `chaos-scenarios.json`：
   - 矩阵 200 条、5 类各 40 条、应用名全局唯一、每条有 id、`summary.total` 与实际一致；
   - A/B 判定只允许 `pass/warn/fail/todo`；
   - 主计划 11.1 点名的 7 个条目（WPS / 微信 / Blender / PS / VS2022 / TraeCN / Steam）必须在列；
   - 混沌场景 ≥10、id 唯一、`automatable` 枚举合法、每场景有步骤与期望；
   - **`dangerous=true` 的场景必须是 `manual`**（脚本绝不代为执行危险操作）。
3. **执行只读动作**：在临时目录里真跑各脚本的只读/`-DryRun` 动作，检查退出码与关键输出，
   并跑一个负向用例（`Deploy-To-USB -Action Verify` 指向宿主系统盘必须判失败）。

## 跑法

```powershell
pwsh -NoProfile -File portable/tests/Run-PortableTests.ps1
# 只查语法与数据、不执行动作：
pwsh -NoProfile -File portable/tests/Run-PortableTests.ps1 -SkipExec
```

退出码 0 = 全通过；非 0 时末尾会列出每一条失败原因。

## 接入 CI

本会话的 GitHub App 令牌**没有 `workflows` 权限**，无法代为提交 `.github/workflows/ci.yml`
（推送时被远端拒绝：`refusing to allow a GitHub App to create or update workflow ... without workflows permission`）。
需要有权限的人把下面这段加进 `ci.yml`（与现有 `frontend` / `backend` 作业同级）：

```yaml
  portable:
    name: portable (AI4+AI5 PowerShell 套件)
    runs-on: windows-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - name: PowerShell 版本
        run: $PSVersionTable | Out-String | Write-Host
      - name: AI-5 套件自检（官方 AST 语法 + 数据不变量 + 只读动作执行）
        run: pwsh -NoProfile -File portable/tests/Run-PortableTests.ps1
      - name: AI-5 验收汇总（只读，未实测项如实标注 todo）
        if: always()
        run: pwsh -NoProfile -File portable/AI5/Accept-Gate.ps1 -Action Report -DataDrive $env:SystemDrive -OutDir $env:RUNNER_TEMP\ai5
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: ai5-acceptance
          path: ${{ runner.temp }}/ai5
          retention-days: 14
          if-no-files-found: warn
```

`windows-latest` 自带 PowerShell 7（`pwsh`），无需额外安装步骤。
`Accept-Gate` 在 CI 上会如实报告 14 项里哪些是 ⬜ 待真机 —— 这是设计行为，不是失败。
