# VX11 B12

## UNX-F9298 进程族 E 型升级接管注记
- stem: `unx9298_etype_takeover_note_proc`
- SEG: 0x7B800
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9298-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9299 进程族 E 型与 glibc fork/exec 错误路径消费核
- stem: `unx9299_etype_vs_consumer_proc`
- SEG: 0x7B840
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9299-J1 fork 返回 -1 errno 透传判据过，exec 失败子进程 _exit(127) 惯例格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9300 ktest syscall 面 B25 批断言集
- stem: `unx9300_ktest_syscall_b25`
- SEG: 0x7B880
- quota: 440
- accept: ① 纯功能代码 ≥440 行；② 判据原文：UNX-F9300-J1 本批 19 条判据聚合判据过，进程族 E 型矩阵一次跑通，域累计 132,700 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9301 EINTR 语义总纲（慢系统调用中断面）
- stem: `unx9301_charter_eintr`
- SEG: 0x7B8C0
- quota: 420
- accept: ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9301-J1 慢系统调用中断面判据过，EINTR 触发三条件（信号到达/无 SA_RESTART/阻塞中）逐格落账，与 Linux 语义一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9302 SA_RESTART 重启语义矩阵（逐 syscall 重启面）
- stem: `unx9302_matrix_sa_restart_syscall`
- SEG: 0x7B900
- quota: 420
- accept: ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9302-J1 SA_RESTART 逐 syscall 重启面矩阵判据过，可重启族逐格重启可观测，不可重启族逐格 EINTR 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9303 read/write EINTR 矩阵（部分读写残留量）
- stem: `unx9303_matrix_read_write`
- SEG: 0x7B940
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9303-J1 部分读写后中断注入判据过，已传字节数返回正判据格过，零字节残留 EINTR 格过，与 B03/B04 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9304 nanosleep EINTR remaining 回写矩阵（联 B20）
- stem: `unx9304_matrix_nanosleep_eintr`
- SEG: 0x7B980
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9304-J1 中断注入 remaining 回写判据过，剩余量逐 ns 可观测格过，与 B20 休眠族对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9305 wait EINTR 矩阵（联 B18）
- stem: `unx9305_matrix_wait_eintr`
- SEG: 0x7B9C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9305-J1 阻塞 wait 中断注入判据过，全 EINTR 格过，WNOHANG 零中断正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
