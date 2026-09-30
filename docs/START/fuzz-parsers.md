# 六解析器 fuzz 常态化 · 运行与注册手册（任务70）

## 需要运行时做什么

- 全量（验收口径，各 10 万轮）：

```bash
cd kernel && cargo ktest --test fuzz_parsers -- --test-threads=6
```

- CI 快速模式：

```bash
VARIX_PARSERS_ROUNDS=1000 cargo ktest --test fuzz_parsers
```

## 覆盖清单（六解析器 → 公开入口）

| 解析器 | 入口 | 测试名 |
|---|---|---|
| boot-select | `bootcfg::parse` | `fuzz_boot_select` |
| PE | `proc::pe::parse` + `parse_imports` | `fuzz_pe_parser` |
| ELF | `proc::elf::parse` | `fuzz_elf_parser` |
| exFAT | `Bpb::parse` + `ExfatVolume::mount` + `read_dir` | `fuzz_exfat_mount` |
| 规则 | `vfsguard::RuleSet::parse` + `normalize` + `adjudicate` | `fuzz_rules_parser` |
| Uxv | `uxvingest::ingest` | `fuzz_uxv_ingest` |

## 需要注册新解析器时做什么

1. 在 `kernel/varix/tests/fuzz_parsers.rs` 写执行体 `fn exec_xxx(data: &[u8])`（只调公开纯 API；Err=如实拒绝，不算失败）。
2. 写测试 `#[test] fn fuzz_xxx() { harness("xxx", &seeds, exec_xxx); }`（结构化种子给真实样例更佳）。
3. 跑一遍确认 `fuzz xxx: N rounds, 0 crash, 0 hang`。

## 崩溃/挂死样本如何处理

- panic 或单轮超 3 秒（挂死）→ 输入自动归档 `kernel/varix/tests/fuzz-corpus/<解析器>/`，测试立即失败。
- 修复实现后，样本即回归用例：每次 fuzz 启动先全量重放 corpus（`corpus replay N`）。
- 归档文件入库（回归资产，不进 _attic）。
