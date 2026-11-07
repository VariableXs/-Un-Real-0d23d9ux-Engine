# VX11 B18

## UNX-F9346 全域 ENOSYS 未实装号矩阵（号面全扫）
- stem: `unx9346_matrix_global_nosys`
- SEG: 0x7C400
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9346-J1 号面全扫注入判据过，未实装号全 ENOSYS 逐格检出，与 18 号注册面/COMPAT_TABLE 对账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9347 错误码优先级链总装（EFAULT>EBADF>EINVAL 判序）
- stem: `unx9347_assembly_efault_ebadf`
- SEG: 0x7C440
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9347-J1 判序链逐例总装判据过，多错误并发时取高优先逐格检出，与 Linux 判序逐例一致格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9348 errno 透传全链路总装（Errno→lxerrno→glibc）
- stem: `unx9348_assembly_errno_errno`
- SEG: 0x7C480
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9348-J1 全链路逐码总装判据过，Errno→sysret→glibc errno 逐码一致格过，与 B13 编码器零漂移格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9349 坏值注入器总装（fuzz 值域全集）
- stem: `unx9349_injector_assembly_fuzz`
- SEG: 0x7C4C0
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9349-J1 fuzz 值域全集注入判据过（零/负/上限/上限+1/全 F/全 0），检出率统计格过，零内核 panic 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9350 边界值等价类总装（零/负/上限/上限+1）
- stem: `unx9350_boundary_equiv_assembly`
- SEG: 0x7C500
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9350-J1 四等价类逐类总装判据过，逐类期望值逐格检出，等价类账落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9351 指针等价类总装（NULL/-1/非映射/只读映射）
- stem: `unx9351_equiv_assembly_null`
- SEG: 0x7C540
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9351-J1 四指针等价类总装判据过，逐类期望 EFAULT 逐格检出，只读映射写路径格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9352 fd 等价类总装（负值/未开/已关/越限）
- stem: `unx9352_equiv_assembly_fd`
- SEG: 0x7C580
- quota: 360
- accept: ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9352-J1 四 fd 等价类总装判据过，逐类期望 EBADF 逐格检出，等价类账落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9353 竞态注入总装（EINTR×五族）
- stem: `unx9353_race_assembly_eintr`
- SEG: 0x7C5C0
- quota: 380
- accept: ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9353-J1 EINTR×五族竞态总装判据过，逐族 100 次统计格过，与 B26 注入器单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
