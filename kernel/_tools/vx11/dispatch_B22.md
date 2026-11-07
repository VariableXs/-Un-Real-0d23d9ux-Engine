# VX11 B22

## UNX-F9378 联调与 glibc 消费核联测面
- stem: `unx9378_consumer_jointest_glibc`
- SEG: 0x7CC00
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9378-J1 消费核联测判据过，glibc 消费清单与联测面对接可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9379 I 型联调判据可复测性声明
- stem: `unx9379_declare`
- SEG: 0x7CC40
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9379-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9380 ktest syscall 面 B29 批断言集
- stem: `unx9380_ktest_syscall_b29`
- SEG: 0x7CC80
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9380-J1 本批 19 条判据聚合判据过，C2×C3 联调一次跑通，域累计 162,000 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9381 I 型联调总纲二（C2×C4 联签协议与挂点面）
- stem: `unx9381_charter_cosign_hook`
- SEG: 0x7CCC0
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9381-J1 联签协议判据过，C2（阻塞挂点）×C4（futex/epoll/pty 面）双方挂点面互认格过，跨界代写禁令声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9382 futex 四操作挂点联签（WAIT/WAKE/FD/REQUEUE 协议）
- stem: `unx9382_cosign_hook_futex`
- SEG: 0x7CD00
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9382-J1 四操作挂点联签判据过，C4 futex 四操作协议逐操作对接可观测，挂点单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9383 futex EAGAIN/EINVAL 矩阵联测（C2 错误矩阵 × C4 挂点）
- stem: `unx9383_matrix_jointest_hook`
- SEG: 0x7CD40
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9383-J1 矩阵联测判据过，EAGAIN 耗尽格与 EINVAL 参数格逐格联测可观测，与 B23 判定域一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9384 epoll_ctl/epoll_wait 挂点联签（就绪队列协议）
- stem: `unx9384_cosign_hook_epoll_ctl`
- SEG: 0x7CD80
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9384-J1 两挂点联签判据过，就绪队列协议对接可观测，与 B09 四件套语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9385 epoll 错误矩阵联测（EBADF/EINVAL/EPERM）
- stem: `unx9385_matrix_jointest_epoll`
- SEG: 0x7CDC0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9385-J1 三错误码联测判据过，逐码与 C4 挂点联测一致格过，与 B22 判定域一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
