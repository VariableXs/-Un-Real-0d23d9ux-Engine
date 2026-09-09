<!-- M-84 性能影响声明门禁：本段为必填段，三选一勾选；缺选或三选多按门禁驳回 -->

## 变更摘要

<!-- 一两句话：改了什么、为什么 -->

## 影响面

<!-- 涉及的域/模块；是否触碰他人领地（见 docs/ENGINE-Version-*-AI分工图.md） -->

## 性能影响声明（必填 · 三选一）

- [ ] **无影响** — 纯文档 / 测试 / 静态文案，不触及任何运行时路径
- [ ] **<1ms 级影响** — 有理由说明即可（如：仅启动期一次性执行 / 空间换时间 / 常量级查询）
- [ ] **>1ms 影响或触及热路径** — 必须附 bench 数据，粘贴 `node tools/bench.cjs --snippet` 输出：

```
（bench --snippet 片段贴这里）
```

## 键位影响（若涉及）

<!-- 新增/修改默认键位：功能冲突（error 级）由 keymap-audit 自动阻断；
     系统保留键重叠（warn 级）须在此写明让位说明标签（Z-09 口径） -->

## 自检清单

- [ ] `npm run typecheck` 零错误
- [ ] `npm test -- --run` 全绿
- [ ] `node tools/audit.cjs` PASSED
- [ ] `node tools/keymap-audit.cjs`（涉及键位时必跑）
- [ ] `cargo check` / `cargo test`（涉及 src-tauri 时必跑）
- [ ] `node tools/link-check.cjs` 断链清零（涉及 docs/ 链接时必跑）
