# M-87 发版演练记录 — 2026-09-09

- 模式：全流程
- 结论：PASS（演练全绿）

| 步骤 | 状态 | 备注 |
| --- | --- | --- |
| 1.版本号一致性 | PASS | v1.0.0（package.json = tauri.conf.json = Cargo.toml） |
| 2a.tsc 类型检查 | PASS |  |
| 2b.vitest 单测 | PASS |  |
| 2c.静态审计 audit.cjs | PASS |  |
| 2d.键位门禁 keymap-audit | PASS |  |
| 2e.文档链接 link-check | PASS |  |
| 2f.前端构建 | PASS |  |
| 3.M-80 IPC 追踪剔除 | PASS |  |
| 4.cargo check | PASS |  |
| 5.NSIS 安装包构建 | PASS |  |
| 5b.NSIS 产物校验 | PASS | Variable_1.0.0_x64-setup.exe（6.5 MB） |
| 6.CHANGELOG 草稿生成 | PASS |  |
| 7a.ROUTES.md | PASS | 生成物与源同步 |
| 7b.SETTINGS.md | PASS | 生成物与源同步 |
