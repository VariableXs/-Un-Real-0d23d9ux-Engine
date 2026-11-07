# VX11 B19

## UNX-F9354 全域错误回归矩阵（与 B21–B27 判据联动）
- stem: `unx9354_matrix_regression_global`
- SEG: 0x7C600
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9354-J1 回归矩阵判据过，与七批判据逐条联动可观测，回归零回退声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9355 E 型与 B13 错误编码器总成对账格
- stem: `unx9355_etype_vs_reconcile_suite`
- SEG: 0x7C640
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9355-J1 对账判据过，与 errno.rs 编码族逐码零冲突格过，升级接管口径声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9356 E 型与 lxerrno::VarixErr 映射总装
- stem: `unx9356_etype_vs_assembly_lxerrno`
- SEG: 0x7C680
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9356-J1 映射总装判据过，VarixErr 逐码映射一致格过，单源声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9357 E 型与 Windows 错误码全域对照账
- stem: `unx9357_etype_vs_global_windows`
- SEG: 0x7C6C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9357-J1 全域对照账判据过，逐码映射表落账，差异账五条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9358 E 型判据可复测性总声明（全矩阵 10/10）
- stem: `unx9358_etype_recheck_matrix_declare`
- SEG: 0x7C700
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9358-J1 全矩阵 10 次重复注入判据过，10/10 次检出总统计格过，零 flaky 总声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9359 E 型升级接管总注记（错误面增量清单）
- stem: `unx9359_etype_takeover_list_note`
- SEG: 0x7C740
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9359-J1 接管边界总声明判据过，存量错误面零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9360 ktest syscall 面 B28 批断言集 + E 型收口断言
- stem: `unx9360_ktest_closure_syscall`
- SEG: 0x7C780
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9360-J1 本批 19 条判据聚合判据过，E 型八批（B21–B28）收口一次跑通断言过，域累计 154,100 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9361 I 型联调总纲一（C2×C3 联签协议与共享判据面）
- stem: `unx9361_charter_cosign_c2`
- SEG: 0x7C7C0
- quota: 480
- accept: ① 纯功能代码 ≥480 行；② 判据原文：UNX-F9361-J1 联签协议判据过，C2（syscall 号面）×C3（LTP 回归基建）双方判据面互认格过，跨界代写禁令声明格过（联测共享只写编号+一句话）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
