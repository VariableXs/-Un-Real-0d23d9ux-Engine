# VX11 B05

## UNX-F9242 mmap 坏 addr 提示与坏映射探针矩阵
- stem: `unx9242_matrix_mmap_addr`
- SEG: 0x7AA00
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9242-J1 mmap addr 非法提示值注入判据过，提示值零副作用正判据格过，映射后访问坏区间 SIGSEGV 路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9243 mmap 未对齐 EINVAL 矩阵
- stem: `unx9243_matrix_mmap_einval`
- SEG: 0x7AA40
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9243-J1 非页对齐 addr/length 注入矩阵判据过，全 EINVAL 逐格检出（MAP_FIXED 时全 EINVAL），页掩码对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9244 mmap ENOMEM 矩阵（超限/无空间）
- stem: `unx9244_matrix_mmap_enomem`
- SEG: 0x7AA80
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9244-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，max_map_count 上限格过，与 RLIMIT_AS 联动格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9245 munmap/mprotect 坏区间矩阵
- stem: `unx9245_matrix_munmap_mprotect`
- SEG: 0x7AAC0
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9245-J1 未对齐区间注入判据过，munmap 全 EINVAL 格过，mprotect 坏 prot 全 EINVAL 格过，区间不存在全 ENOMEM 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9246 brk 坏值矩阵（越下界 EINVAL）
- stem: `unx9246_matrix_brk_einval`
- SEG: 0x7AB00
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9246-J1 低于初始堆底注入判据过，全 EINVAL 格过，返回新 break 正判据格过，与 B07 brk 档对账一致；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9247 mremap 错误矩阵（EINVAL/ENOMEM/EPERM 联 B17）
- stem: `unx9247_matrix_mremap_einval`
- SEG: 0x7AB40
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9247-J1 三错误码注入矩阵判据过，非对齐 old_addr 全 EINVAL 格过，与 B17 矩阵零重复分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9248 madvise 错误矩阵（EINVAL/ENOMEM 联 B17）
- stem: `unx9248_matrix_madvise_einval`
- SEG: 0x7AB80
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9248-J1 未知 advice 全 EINVAL 判据过，越界区间全 ENOMEM 格过，与 B17 矩阵零重复分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9249 mlock/mprotect 权限边界矩阵（与 B24 分界声明）
- stem: `unx9249_matrix_boundary_declare`
- SEG: 0x7ABC0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9249-J1 锁页越限全 EPERM 判据过，与 B24 权限矩阵分界声明格过，零重复判据格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
