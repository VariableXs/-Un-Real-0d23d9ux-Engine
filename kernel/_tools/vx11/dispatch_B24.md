# VX11 B24

## UNX-F9394 联调防重声明（C2×C4 号面零冲突 grep）
- stem: `unx9394_declare_dedup_c2`
- SEG: 0x7D000
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9394-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9395 联调上游依赖登记（C4 未 finalize 依赖声明）
- stem: `unx9395_declare_deps_c4`
- SEG: 0x7D040
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9395-J1 依赖声明判据过，C4 上游未 finalize 显式登记格过，fake 开发路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9396 联调与 Windows WaitableTimer/IOCP 对照账
- stem: `unx9396_windows_waitabletimer_iocp`
- SEG: 0x7D080
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9396-J1 对照账判据过，WaitableTimer/IOCP 映射表落账，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9397 联调与 glibc pthread futex 消费核
- stem: `unx9397_consumer_glibc_pthread`
- SEG: 0x7D0C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9397-J1 消费核联测判据过，pthread futex 消费链对接可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9398 I 型联调判据可复测性声明
- stem: `unx9398_declare`
- SEG: 0x7D100
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9398-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9399 I 型与 glibc 消费清单逐号核联测
- stem: `unx9399_jointest_list_glibc`
- SEG: 0x7D140
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9399-J1 逐号核联测判据过，消费清单逐号联测可观测，零号面冲突格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9400 ktest syscall 面 B30 批断言集 + B16–B30 半域收口断言
- stem: `unx9400_ktest_closure_syscall`
- SEG: 0x7D180
- quota: 460
- accept: ① 纯功能代码 ≥460 行；② 判据原文：UNX-F9400-J1 本批 19 条判据聚合判据过，B16–B30 十五批半域收口一次跑通断言过，域累计 169,000 收口断言（B31–B40 预算 71,000 待领，域账守恒 240,000）；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9401 I 型联调总纲三（C2×C3 联签协议与对账面）
- stem: `unx9401_charter_cosign_c2`
- SEG: 0x7D1C0
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9401-J1 联签协议判据过，C2（错误矩阵面）×C3（errno 账本面）双方对账面互认格过，跨界代写禁令声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
