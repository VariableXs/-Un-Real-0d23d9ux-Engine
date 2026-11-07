# VX11 B04

## UNX-F9234 fd 族 E 型参数探针总成
- stem: `unx9234_eprobe_suite_fd`
- SEG: 0x7A800
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9234-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9235 fd 族 E 型与 B08/B15 深化册对账格
- stem: `unx9235_etype_vs_reconcile_fd`
- SEG: 0x7A840
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9235-J1 对账判据过，与 dup 族/管道深化册逐条判据零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9236 fd 族 E 型与 Windows INVALID_HANDLE_VALUE 对照格
- stem: `unx9236_etype_vs_compare_fd`
- SEG: 0x7A880
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9236-J1 INVALID_HANDLE_VALUE 映射判据过，WSA 错误差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9237 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9237_etype_recheck_declare`
- SEG: 0x7A8C0
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9237-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9238 fd 族 E 型升级接管注记
- stem: `unx9238_etype_takeover_note_fd`
- SEG: 0x7A900
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9238-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9239 fd 族 E 型与 glibc perror/strerror 消费核
- stem: `unx9239_etype_vs_consumer_fd`
- SEG: 0x7A940
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9239-J1 strerror 逐码输出判据过，与 errno 透传单源格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9240 ktest syscall 面 B22 批断言集
- stem: `unx9240_ktest_syscall_b22`
- SEG: 0x7A980
- quota: 460
- accept: ① 纯功能代码 ≥460 行；② 判据原文：UNX-F9240-J1 本批 19 条判据聚合判据过，fd 族 E 型矩阵一次跑通，域累计 112,400 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9241 内存族 E 型总纲（地址空间错误面）
- stem: `unx9241_charter_mem`
- SEG: 0x7A9C0
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9241-J1 地址空间错误面判据过，四类错误域（对齐/越界/耗尽/权限）逐格落账，与本批矩阵映射一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
