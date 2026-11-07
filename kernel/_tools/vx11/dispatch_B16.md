# VX11 B16

## UNX-F9330 rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账）
- stem: `unx9330_matrix_rlimit_einval`
- SEG: 0x7C000
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9330-J1 软硬倒置注入判据过，全 EINVAL 格过，与 B19 矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9331 sched_setaffinity 坏掩码 EINVAL 矩阵
- stem: `unx9331_matrix_sched_setaffinity_einval`
- SEG: 0x7C040
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9331-J1 全零掩码注入判据过，全 EINVAL 格过，坏 mask 指针全 EFAULT 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9332 sched_setscheduler 非法策略 EINVAL 矩阵
- stem: `unx9332_matrix_sched_setscheduler_einval`
- SEG: 0x7C080
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9332-J1 非法策略注入判据过，全 EINVAL 格过，非法 param 全 EINVAL 格过，A1 消费声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9333 时间值规范化边界矩阵（tv_nsec 上限 1e9）
- stem: `unx9333_matrix_boundary_tv_nsec`
- SEG: 0x7C0C0
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9333-J1 tv_nsec 越界注入判据过，≥1e9 全 EINVAL 格过，规范化（tv_sec 进位）语义格过，与 B20 规范化对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9334 时间/资源族 E 型参数探针总成
- stem: `unx9334_eprobe_suite_timeres`
- SEG: 0x7C100
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9334-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9335 时间/资源族 E 型与 B06/B19 深化册对账格
- stem: `unx9335_etype_vs_reconcile_timeres`
- SEG: 0x7C140
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9335-J1 对账判据过，与时间族/资源族深化册逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9336 时间/资源族 E 型与 Windows 计时器错误对照格
- stem: `unx9336_etype_vs_compare_timeres`
- SEG: 0x7C180
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9336-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9337 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9337_etype_recheck_declare`
- SEG: 0x7C1C0
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9337-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
