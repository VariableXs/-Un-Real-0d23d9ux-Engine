# UNX-C2-B03 · 内存族三联扩容（F8841–F8860 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B02 10,210 + 本批 4,760 = 14,970 / 240,000｜嫁接源：Linux man-pages（mmap/madvise/mlock 章节）只跟随｜防重：与 B01 F8804（mmap/munmap/mprotect 三联语义档）分层——本批为内存族周边号扩容（brk/mremap/madvise/msync/mincore/mlock 族），语义档不重复；对现存 `syscall::calls::MmapRequest`/`MmapState`/`Placement` 与 A4 域（未开工）为挂点对接，底座本体归 A4 防重｜判据与 deepen/C2-B03.md 逐条同名同判据同 ID

### UNX-F8841 · brk 语义档与探顶行为（glibc 探测关键）
- 域/批：C2/B03｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8841-J1 brk(0) 返回当前断点、增减与对齐行为与 Linux 逐项一致，glibc 探顶序列（渐进探测）10/10 次正确响应
### UNX-F8842 · mremap 扩缩与搬移语义
- 域/批：C2/B03｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8842-J1 MREMAP_MAYMOVE 允许搬移判据过，不允许搬移时扩容失败返 ENOMEM，搬移后旧地址失效访问收 SIGSEGV
### UNX-F8843 · madvise 建议面（MADV_NORMAL/SEQUENTIAL/WILLNEED）
- 域/批：C2/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8843-J1 五建议档接受语义正确（建议可忽略但必须不报错），未知建议返 EINVAL，越界区间返 ENOMEM
### UNX-F8844 · msync 同步语义（MS_SYNC/MS_ASYNC）
- 域/批：C2/B03｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8844-J1 MS_SYNC 返回后数据持久判据过，MS_ASYNC 异步调度有账，非映射区间返 ENOMEM
### UNX-F8845 · mincore 驻留查询
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8845-J1 驻留位图逐页与实际映射状态一致率 100%，未对齐地址向下取页对齐行为与 Linux 一致
### UNX-F8846 · mlock/munlock 驻留锁
- 域/批：C2/B03｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8846-J1 锁定后页驻留（mincore 交叉验证）判据过，超 RLIMIT_MLOCK 返 ENOMEM，解锁后可换出可观测
### UNX-F8847 · mlockall/munlockall 与 RLIMIT_MLOCK
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8847-J1 MCL_CURRENT/MCL_FUTURE 两标志语义正确，FUTURE 对后续 mmap 自动锁定的行为留账，非特权限额触发 ENOMEM
### UNX-F8848 · MAP_FIXED_NOREPLACE 语义
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8848-J1 目标区间已被占用时返 EEXIST 且不破坏原映射判据 100 次，MAP_FIXED 覆盖语义对照（破坏性）有账
### UNX-F8849 · MAP_SHARED/MAP_PRIVATE 双模语义
- 域/批：C2/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8849-J1 SHARED 写可见（跨进程共享页判据）、PRIVATE 写时复制（父进程页不变判据）各 100 次通过，双标志同设返 EINVAL
### UNX-F8850 · PROT_NONE 与权限组合矩阵
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8850-J1 PROT_NONE 读/写/执行三向访问全收 SIGSEGV 判据过，PROT_READ|WRITE 组合矩阵 4 格逐格正确
### UNX-F8851 · 匿名映射与文件映射双路
- 域/批：C2/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8851-J1 匿名页零填充判据、文件映射内容一致性（与 read 路径对照）判据各 100 次通过，fd 偏移不因映射改变
### UNX-F8852 · mprotect 权限翻转与 #PF/SIGSEGV 联动
- 域/批：C2/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8852-J1 翻只读后写触发 #PF 且进程收 SIGSEGV（与 C4 信号域联测）10/10 次，翻转对 TLB 生效延迟有账，未映射区间返 ENOMEM
### UNX-F8853 · mmap NULL 提示与地址选择器
- 域/批：C2/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8853-J1 addr=NULL 内核自选地址单调分布判据过，glibc mmap NULL 探测序列 10/10 次正常响应，自选地址与现存 Placement 策略一致
### UNX-F8854 · 大页挂点（MAP_HUGETLB 语义预留）
- 域/批：C2/B03｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8854-J1 MAP_HUGETLB 挂点注册成功，未配置大页池时返 ENOMEM 降级路径明确，2MB 对齐断言通过
### UNX-F8855 · mmap 错误矩阵（ENOMEM/EINVAL/EFAULT/EOVERFLOW）
- 域/批：C2/B03｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8855-J1 四错误码注入矩阵（非对齐 addr、零 length、越界 length、fd 无效 + MAP_FIXED）16 格逐格与 Linux 逐码一致
### UNX-F8856 · munmap 部分区间与页对齐
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8856-J1 非页对齐地址向下取整解除判据过，部分解除后剩余区间读写正常，全解除后访问收 SIGSEGV
### UNX-F8857 · 内存族与 A4 底座对接协议（冻结接口注记）
- 域/批：C2/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8857-J1 对接协议文书（mmap 底座四原语 + 错误转译表）落盘冻结，A4 未收口期以现存 MmapRequest 最小栈运行判据全过，缺口入 open_risks
### UNX-F8858 · VM 过提交策略挂点（overcommit 模式）
- 域/批：C2/B03｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8858-J1 三模式（always/never/heuristic）挂点注册成功，never 模式下超物理内存分配返 ENOMEM 判据过
### UNX-F8859 · mmap 高频路径延迟账（缺页路径分段）
- 域/批：C2/B03｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8859-J1 mmap 调用与首次触碰（#PF）两段延迟分开落账，10^4 次 P50/P99 与 O1 对接字段一致
### UNX-F8860 · ktest syscall 面 B03 批断言集
- 域/批：C2/B03｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8860-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，内存族断言与 B01/B02 面隔离编号
