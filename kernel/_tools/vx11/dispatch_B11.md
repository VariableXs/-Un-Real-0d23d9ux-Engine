# VX11 B11

## UNX-F9290 零参数 execve 边界矩阵
- stem: `unx9290_matrix_boundary_execve`
- SEG: 0x7B600
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9290-J1 空 argv 正判据（argv[0] 可空）判据过，空 envp 正判据格过，坏指针优先判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9291 僵尸收割竞态注入（wait vs exit 竞态）
- stem: `unx9291_race_wait_exit`
- SEG: 0x7B640
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9291-J1 wait-exit 竞态注入判据过，单次收割唯一性 100 次统计格过，双 wait 第二次全 ECHILD 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9292 进程退出竞态注入（SIGCHLD 投递竞态）
- stem: `unx9292_race_sigchld`
- SEG: 0x7B680
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9292-J1 SIGCHLD 投递竞态注入判据过，投递不丢失 100 次统计格过，与 B11 链对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9293 ESRCH 判定次序协议（pid 存在性先于权限）
- stem: `unx9293_order_perm_esrch`
- SEG: 0x7B6C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9293-J1 判序协议判据过，不存在 pid 全 ESRCH 逐格检出（先于 EPERM），与 Linux 逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9294 进程族 E 型参数探针总成
- stem: `unx9294_eprobe_suite_proc`
- SEG: 0x7B700
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9294-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9295 进程族 E 型与 B04/B08 深化册对账格
- stem: `unx9295_etype_vs_reconcile_proc`
- SEG: 0x7B740
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9295-J1 对账判据过，与 fork 档/进程族地基深化册逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9296 进程族 E 型与 Windows 进程句柄错误对照格
- stem: `unx9296_etype_vs_compare_proc`
- SEG: 0x7B780
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9296-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9297 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9297_etype_recheck_declare`
- SEG: 0x7B7C0
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9297-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
