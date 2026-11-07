# VX11 B15

## UNX-F9322 gettimeofday/clock_gettime 坏指针矩阵（EFAULT）
- stem: `unx9322_matrix_gettimeofday_clock_gettim`
- SEG: 0x7BE00
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9322-J1 坏指针注入判据过，全 EFAULT 格过，10/10 次检出格过，与 B06 时间族对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9323 clock_settime 非法时钟 EINVAL 矩阵
- stem: `unx9323_matrix_clock_settime_einval`
- SEG: 0x7BE40
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9323-J1 非法 clockid 注入判据过，全 EINVAL 格过，CLOCK_REALTIME 正判据格过，特权检查挂点格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9324 timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN）
- stem: `unx9324_matrix_timer_create_timer_settim`
- SEG: 0x7BE80
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9324-J1 两错误码注入矩阵判据过，非法 sigevent 全 EINVAL 格过，定时器耗尽全 EAGAIN 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9325 POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动）
- stem: `unx9325_matrix_posix_eagain`
- SEG: 0x7BEC0
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9325-J1 定时器上限注入判据过，全 EAGAIN 格过，与 B19 SIGPENDING 联动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9326 setitimer 非法间隔矩阵
- stem: `unx9326_matrix_setitimer`
- SEG: 0x7BF00
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9326-J1 负值/超上界注入判据过，全 EINVAL 格过，ITIMER_REAL 三类正判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9327 clock_nanosleep 非法标志/时钟矩阵
- stem: `unx9327_matrix_clock_nanosleep`
- SEG: 0x7BF40
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9327-J1 非法标志注入判据过，全 EINVAL/ENOTSUP 逐格检出，非法时钟全 EINVAL 格过，与 B20 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9328 getrusage 坏指针矩阵（联 B15/B19 对账）
- stem: `unx9328_matrix_getrusage_b15`
- SEG: 0x7BF80
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9328-J1 坏指针注入判据过，全 EFAULT 格过，非法 who 全 EINVAL 格过，与 B19 对账零重复格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9329 setrlimit 坏结构指针矩阵
- stem: `unx9329_matrix_setrlimit`
- SEG: 0x7BFC0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9329-J1 坏 rlim 指针注入判据过，全 EFAULT 格过，10/10 次检出格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
