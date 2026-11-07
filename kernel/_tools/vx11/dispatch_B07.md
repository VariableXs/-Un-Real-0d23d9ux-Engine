# VX11 B07

## UNX-F9258 内存族 E 型升级接管注记
- stem: `unx9258_etype_takeover_note_mem`
- SEG: 0x7AE00
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9258-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9259 内存族 E 型与 glibc malloc 失败路径消费核
- stem: `unx9259_etype_vs_consumer_mem`
- SEG: 0x7AE40
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9259-J1 malloc 返回 NULL 路径判据过，ENOMEM 透传可观测格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9260 ktest syscall 面 B23 批断言集
- stem: `unx9260_ktest_syscall_b23`
- SEG: 0x7AE80
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9260-J1 本批 19 条判据聚合判据过，内存族 E 型矩阵一次跑通，域累计 119,200 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9261 权限错误面总纲（EPERM vs EACCES 判定域）
- stem: `unx9261_charter_domain_perm`
- SEG: 0x7AEC0
- quota: 420
- accept: ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9261-J1 两错误码判定域判据过，EPERM（操作特权缺失）与 EACCES（对象权限不足）分界逐格落账，与 Linux 语义逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9262 open EACCES 矩阵（rwx 三位逐格）
- stem: `unx9262_matrix_open_eacces`
- SEG: 0x7AF00
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9262-J1 rwx 三位逐格注入判据过，缺 r 读全 EACCES 格过，缺 w 写全 EACCES 格过，缺 x 执行全 EACCES 格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9263 chmod/chown EPERM 矩阵（非属主）
- stem: `unx9263_matrix_chmod_chown`
- SEG: 0x7AF40
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9263-J1 非属主注入判据过，全 EPERM 格过，特权路径正判据格过，与 B05 三身份档对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9264 权限位模型（st_mode 九位逐格矩阵）
- stem: `unx9264_matrix_perm_st_mode`
- SEG: 0x7AF80
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9264-J1 九位逐格矩阵判据过，ugo×rwx 逐格可观测，S_IFMT 类型位掩码格过，与 stat 结构单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9265 setuid/setgid 文件位与执行路径矩阵
- stem: `unx9265_matrix_setuid_setgid`
- SEG: 0x7AFC0
- quota: 340
- accept: ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9265-J1 s 位语义挂点判据过，目录 setgid 位继承挂点格过，实装归 C1/B1 联签声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
