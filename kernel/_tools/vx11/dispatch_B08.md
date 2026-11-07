# VX11 B08

## UNX-F9266 目录搜索权限 EACCES 矩阵（路径逐段）
- stem: `unx9266_matrix_perm_eacces`
- SEG: 0x7B000
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9266-J1 路径逐段缺 x 注入判据过，全 EACCES 格过，错误点（首段缺失优先）判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9267 EPERM 特权操作矩阵（root-only 全集）
- stem: `unx9267_matrix_eperm_root`
- SEG: 0x7B040
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9267-J1 root-only 操作全集注入判据过，非 root 全 EPERM 逐格检出，CAP 消费声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9268 能力位 CAP_* 与 EPERM 判定矩阵
- stem: `unx9268_matrix_cap_eperm`
- SEG: 0x7B080
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9268-J1 guard::CAP_* 能力位逐位注入判据过，缺 CAP 全 EPERM 逐格检出，能力位与 uid 判序格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9269 只读文件系统 EROFS 矩阵
- stem: `unx9269_matrix_erofs`
- SEG: 0x7B0C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9269-J1 EROFS 注入判据过，写路径全 EROFS 逐格检出，与 EACCES 判序（先挂载只读后权限）格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9270 EBUSY/EEXIST 冲突矩阵（独占创建）
- stem: `unx9270_matrix_ebusy_eexist`
- SEG: 0x7B100
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9270-J1 O_EXCL 冲突注入判据过，全 EEXIST 格过，EBUSY 独占冲突格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9271 身份翻转 EPERM 矩阵（联 B20 对账）
- stem: `unx9271_matrix_eperm_b20`
- SEG: 0x7B140
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9271-J1 非特权提限注入判据过，全 EPERM 逐格检出，与 B20 身份矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9272 ioctl ioperm/iopl 禁用格 EPERM 矩阵
- stem: `unx9272_matrix_ioctl_ioperm`
- SEG: 0x7B180
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9272-J1 ioperm/iopl 注入判据过，全 EPERM 拒绝格过，与 B16 红线注记（禁用格）对账一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9273 rlimit 硬限提升 EPERM 矩阵（联 B19 对账）
- stem: `unx9273_matrix_rlimit_eperm`
- SEG: 0x7B1C0
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9273-J1 硬限提升注入判据过，非特权全 EPERM 格过，与 B19 资源矩阵零重复分层格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
