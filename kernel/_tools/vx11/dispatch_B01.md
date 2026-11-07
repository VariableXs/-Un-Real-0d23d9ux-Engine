# VX11 B01

## UNX-F9210 文件族零长度读写边界矩阵
- stem: `unx9210_zerolen_matrix_boundary`
- SEG: 0x7A200
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9210-J1 零长度 read/write 正判据（零副作用返回 0）判据过，反向判序（坏 fd 优先于零长度）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9211 坏 fd 注入跨号段扫描（fd 空间全域）
- stem: `unx9211_badfd_sweep_global_fd`
- SEG: 0x7A240
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9211-J1 fd 空间全域扫描注入判据过，未开槽位全 EBADF 逐格检出，越上限与 RLIMIT_NOFILE 分界格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9212 EFAULT 拷贝半程探针（copy_to_user 半途）
- stem: `unx9212_efault_halfcopy_copy_to_user`
- SEG: 0x7A280
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9212-J1 半途拷贝注入判据过，已拷贝字节数返回语义与 Linux 一致格过，半程残留可观测格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9213 错误码编码器一致性矩阵（Errno→sysret 逐码）
- stem: `unx9213_errno_encoder_matrix_errno`
- SEG: 0x7A2C0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9213-J1 Errno 全集逐码注入判据过，sysret 编码逐码一致格过，与 B13 映射账零漂移格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9214 文件族 E 型参数探针总成
- stem: `unx9214_eprobe_suite_file`
- SEG: 0x7A300
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9214-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针总成单源；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9215 文件族 E 型与 B03–B05 深化册对账格
- stem: `unx9215_etype_vs_reconcile_file`
- SEG: 0x7A340
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9215-J1 对账判据过，与 open/close/lseek/stat 深化册逐条判据零冲突，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9216 文件族 E 型与 Windows GetLastError 对照格
- stem: `unx9216_etype_vs_compare_file`
- SEG: 0x7A380
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9216-J1 ERROR_INVALID_HANDLE/ERROR_NOACCESS 映射表判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9217 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9217_etype_recheck_declare`
- SEG: 0x7A3C0
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9217-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
