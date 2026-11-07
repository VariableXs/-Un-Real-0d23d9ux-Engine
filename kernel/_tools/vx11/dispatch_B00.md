# VX11 B00

## UNX-F9202 read/write 坏 fd 矩阵（全 EBADF）
- stem: `unx9202_rw_badfd_matrix_ebadf`
- SEG: 0x7A000
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9202-J1 负 fd/未开 fd/已关 fd 三类注入矩阵判据过，全返回 -EBADF 与 Linux 逐码一致，10/10 次检出复测格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9203 read/write 坏指针矩阵（全 EFAULT）
- stem: `unx9203_rw_badptr_matrix_efault`
- SEG: 0x7A040
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9203-J1 NULL/-1/未映射/只读映射写路径四类注入矩阵判据过，全 -EFAULT 格过，部分拷贝残留量声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9204 close 坏 fd 矩阵（含双关闭竞态）
- stem: `unx9204_close_badfd_matrix_race`
- SEG: 0x7A080
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9204-J1 坏 fd 全 EBADF 判据过，双 close 竞态注入第二次全 EBADF 格过，fd 槽位复用零泄漏格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9205 lseek 坏 fd/坏 whence 矩阵
- stem: `unx9205_lseek_badfd_matrix_whence`
- SEG: 0x7A0C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9205-J1 坏 fd 全 EBADF 判据过，非法 whence 全 EINVAL 格过，负偏移全 EINVAL 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9206 open 坏路径指针 EFAULT 矩阵
- stem: `unx9206_open_badptr_matrix_efault`
- SEG: 0x7A100
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9206-J1 坏路径指针注入全 EFAULT 判据过，路径截断注入零越界读格过，10/10 次检出格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9207 open 坏 flags 组合 EINVAL 矩阵
- stem: `unx9207_open_badflags_matrix_einval`
- SEG: 0x7A140
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9207-J1 O_CREAT 缺 mode/非法 O_ 位组合注入矩阵判据过，全 EINVAL 或按 Linux 语义位忽略格过，逐码对账一致；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9208 stat/fstat 坏指针/坏 fd 矩阵
- stem: `unx9208_stat_badptr_matrix_fd`
- SEG: 0x7A180
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9208-J1 坏 buf 指针全 EFAULT 判据过，坏 fd 全 EBADF 格过，路径不存在全 ENOENT 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9209 文件族负偏移与溢出边界矩阵
- stem: `unx9209_negoff_overflow_matrix_boundary`
- SEG: 0x7A1C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9209-J1 负 offset 注入全 EINVAL 判据过，off_t 溢出边界截断语义格过，与 B04 lseek 档对账一致；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
