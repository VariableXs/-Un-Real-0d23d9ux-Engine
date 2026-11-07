# VX11 B10

## UNX-F9282 kill 坏 pid 矩阵（ESRCH/EPERM/EINVAL）
- stem: `unx9282_matrix_kill_pid`
- SEG: 0x7B400
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9282-J1 三错误码注入矩阵判据过，pid=1 EPERM 特例格过，非法 sig 全 EINVAL 格过，不存在 pid 全 ESRCH 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9283 wait 坏 pid ECHILD 矩阵（联 B18 对账）
- stem: `unx9283_matrix_wait_pid`
- SEG: 0x7B440
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9283-J1 无子进程注入判据过，全 ECHILD 格过，与 B18 wait 矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9284 fork/clone EAGAIN 矩阵（RLIMIT_NPROC 联动）
- stem: `unx9284_matrix_fork_clone`
- SEG: 0x7B480
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9284-J1 进程数触顶注入判据过，全 EAGAIN 格过，与 B19 NPROC 联动格过，释放后恢复正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9285 fork/clone ENOMEM 矩阵（地址空间耗尽）
- stem: `unx9285_matrix_fork_clone`
- SEG: 0x7B4C0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9285-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，COW 挂点依赖声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9286 setpgid 坏 pid ESRCH 矩阵
- stem: `unx9286_matrix_setpgid_pid`
- SEG: 0x7B500
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9286-J1 坏 pid 注入判据过，全 ESRCH 格过，跨会话 EPERM 格过，与 B18 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9287 setsid EPERM 矩阵（会话首规则联 B18）
- stem: `unx9287_matrix_setsid_eperm`
- SEG: 0x7B540
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9287-J1 会话首注入判据过，全 EPERM 格过，与 B18 选举语义对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9288 execve 错误矩阵（EACCES/ENOEXEC/E2BIG/EFAULT）
- stem: `unx9288_matrix_execve_eacces`
- SEG: 0x7B580
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9288-J1 四错误码注入矩阵判据过，坏路径全 EFAULT 格过，非脚本非 ELF 全 ENOEXEC 格过，判序与 Linux 一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9289 参数长度 E2BIG 矩阵（argv/envp 上限）
- stem: `unx9289_matrix_e2big_argv`
- SEG: 0x7B5C0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9289-J1 超限 argv 注入判据过，全 E2BIG 格过，单参数上限 MAX_ARG_STRLEN 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
