# 验收原始产物归档说明

`scripts/acceptance-gate.sh` 产生的**非功能产物**（各线日志、`vitest.json`、`vline-ids.json`、
`coverage-summary.json` 等快照）按仓库约定统一落在：

```
_attic/acceptance/acceptance-raw/
```

`docs/acceptance/` 只保留**功能性文档**（验收报告、缺口清单、评审记录等），
不再混杂可再生的日志与快照。

复验：

```bash
bash scripts/acceptance-gate.sh
cat _attic/acceptance/acceptance-raw/coverage-summary.json
```
