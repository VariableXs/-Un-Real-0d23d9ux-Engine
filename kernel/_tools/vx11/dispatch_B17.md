# VX11 B17

## UNX-F9338 时间/资源族 E 型升级接管注记
- stem: `unx9338_etype_takeover_note_timeres`
- SEG: 0x7C200
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9338-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9339 时间/资源族 E 型与 glibc clock 家族消费核
- stem: `unx9339_etype_vs_consumer_timeres`
- SEG: 0x7C240
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9339-J1 clock_gettime errno 透传判据过，失效时钟全 EINVAL 可观测，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9340 ktest syscall 面 B27 批断言集
- stem: `unx9340_ktest_syscall_b27`
- SEG: 0x7C280
- quota: 440
- accept: ① 纯功能代码 ≥440 行；② 判据原文：UNX-F9340-J1 本批 19 条判据聚合判据过，时间资源族 E 型矩阵一次跑通，域累计 146,800 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9341 E 型总装总纲（全错误码矩阵收口协议）
- stem: `unx9341_charter_matrix_assembly`
- SEG: 0x7C2C0
- quota: 420
- accept: ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9341-J1 八批错误矩阵收口协议判据过，全错误码账逐码落账，收口判序（EFAULT>EBADF>EINVAL>EPERM）声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9342 全域 EFAULT 矩阵总装（跨五族逐格）
- stem: `unx9342_matrix_assembly_global`
- SEG: 0x7C300
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9342-J1 五族坏指针逐格总装判据过，全 EFAULT 逐格检出，拷贝方向（in/out）双向逐格格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9343 全域 EBADF 矩阵总装
- stem: `unx9343_matrix_assembly_global`
- SEG: 0x7C340
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9343-J1 五族坏 fd 逐格总装判据过，全 EBADF 逐格检出，fd 空间全域扫描格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9344 全域 EINVAL 矩阵总装
- stem: `unx9344_matrix_assembly_global`
- SEG: 0x7C380
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9344-J1 五族非法参数逐格总装判据过，全 EINVAL 逐格检出，与 Linux 逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9345 全域 EPERM/EACCES 矩阵总装
- stem: `unx9345_matrix_assembly_global`
- SEG: 0x7C3C0
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9345-J1 五族权限注入逐格总装判据过，EPERM/EACCES 分界逐格检出，与 B24 判定域对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
