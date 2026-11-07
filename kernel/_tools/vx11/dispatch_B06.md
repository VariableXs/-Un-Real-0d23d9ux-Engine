# VX11 B06

## UNX-F9250 地址空间越界访问 SIGSEGV 路径矩阵
- stem: `unx9250_matrix_sigsegv`
- SEG: 0x7AC00
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9250-J1 越界读写触发 #PF 判据过，SIGSEGV 投递可观测格过，si_addr 逐格正确格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9251 页对齐边界值矩阵（4K/2M 大页）
- stem: `unx9251_matrix_boundary`
- SEG: 0x7AC40
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9251-J1 4K 页界逐位注入判据过，2M 大页边界格过，跨界映射拒绝语义格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9252 零长度映射边界矩阵
- stem: `unx9252_matrix_boundary`
- SEG: 0x7AC80
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9252-J1 零长度 mmap 正判据（返回合法地址零副作用）判据过，零长度 munmap 全 EINVAL 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9253 MAP_FIXED 冲突矩阵（ENOENT/EEXIST）
- stem: `unx9253_matrix_map_fixed_enoent`
- SEG: 0x7ACC0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9253-J1 MAP_FIXED 覆盖语义判据过，与现存映射冲突覆盖可观测格过，MAP_FIXED_NOREPLACE 全 EEXIST 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9254 内存族 E 型参数探针总成
- stem: `unx9254_eprobe_suite_mem`
- SEG: 0x7AD00
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9254-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9255 内存族 E 型与 B06/B07 深化册对账格
- stem: `unx9255_etype_vs_reconcile_mem`
- SEG: 0x7AD40
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9255-J1 对账判据过，与 mmap 三联/brk 档深化册逐条判据零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9256 内存族 E 型与 Windows VirtualAlloc 错误码对照格
- stem: `unx9256_etype_vs_compare_mem`
- SEG: 0x7AD80
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9256-J1 ERROR_INVALID_ADDRESS 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9257 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9257_etype_recheck_declare`
- SEG: 0x7ADC0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9257-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
