# VX11 B09

## UNX-F9274 权限族 E 型参数探针总成
- stem: `unx9274_eprobe_suite_perm`
- SEG: 0x7B200
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9274-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9275 权限族 E 型与三身份档/B05 深化册对账格
- stem: `unx9275_etype_vs_reconcile_perm`
- SEG: 0x7B240
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9275-J1 对账判据过，与 B05 三身份语义基准逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9276 权限族 E 型与 Windows Access Denied 对照格
- stem: `unx9276_etype_vs_compare_perm`
- SEG: 0x7B280
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9276-J1 ERROR_ACCESS_DENIED 映射判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9277 E 型判据可复测性声明（10/10 次检出）
- stem: `unx9277_etype_recheck_declare`
- SEG: 0x7B2C0
- quota: 300
- accept: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9277-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9278 权限族 E 型升级接管注记
- stem: `unx9278_etype_takeover_note_perm`
- SEG: 0x7B300
- quota: 280
- accept: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F9278-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9279 权限族 E 型与 glibc access/euid 消费核
- stem: `unx9279_etype_vs_consumer_perm`
- SEG: 0x7B340
- quota: 320
- accept: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9279-J1 access 族真实 uid 语义判据过，与 euid 差异可观测格过，消费号登记格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9280 ktest syscall 面 B24 批断言集
- stem: `unx9280_ktest_syscall_b24`
- SEG: 0x7B380
- quota: 480
- accept: ① 纯功能代码 ≥480 行；② 判据原文：UNX-F9280-J1 本批 19 条判据聚合判据过，权限族 E 型矩阵一次跑通，域累计 126,000 收口断言；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。

## UNX-F9281 进程族 E 型总纲（进程空间错误面）
- stem: `unx9281_charter_proc`
- SEG: 0x7B3C0
- quota: 400
- accept: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9281-J1 进程空间错误面判据过，四类错误域（pid 不存在/权限/资源耗尽/参数坏值）逐格落账，判序协议声明格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
