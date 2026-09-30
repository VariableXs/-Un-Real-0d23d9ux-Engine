# UNX-C2-B17 · 内存族扩容：mremap/madvise/msync/mlock（F9121-F9140 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 内存族扩容）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B17.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16 6,500 + 本批 6,400 = 79,770 / 240,000｜嫁接源：mremap(2)/madvise(2)/msync(2)/mlock(2) man-pages 语义对齐；A4 mmap 底座冻结接口消费（页表挂点声明）；与 F8804 mmap 三联/F8841 brk 档单源联动｜防重声明：内存族扩容 20 号为号面新铺（B01-B15 mmap/mprotect/brk 本体已铺，本批只铺 mremap/madvise/msync/mlock/mincore 扩容号，零重复）；扩号段 F9121-F9140 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；mlock 族零 DMA 零物理地址直写；mremap 零页表直改（只消费 A4 冻结接口，页表操作归 A4 域）｜批注（AI-12）：B17 是内存族扩容轴：mremap 消费 A4 页表底座（上游未 finalize 依赖显式登记），madvise 六建议先立协议骨架（回收/预读实装随 B1 页缓存收口联签），mlock 族与 RLIMIT_MEMLOCK 联动（B19 资源族消费）｜判据与后续 deepen/C2-B17.md 逐条同名同判据同 ID

### UNX-F9121 · mremap 语义档（MREMAP_MAYMOVE/MREMAP_FIXED）
- 域/批：C2/B17｜纯功能行数：400｜状态：｜判据：UNX-F9121-J1 迁移映射判据过，MAYMOVE 位语义格过，old_size 匹配校验格过
### UNX-F9122 · mremap 与 A4 页表底座消费协议
- 域/批：C2/B17｜纯功能行数：340｜状态：｜判据：UNX-F9122-J1 页表消费协议落账判据过，PTE 迁移挂点声明格过，上游未 finalize 依赖登记格过
### UNX-F9123 · mremap 错误矩阵（EINVAL/ENOMEM/EPERM）
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9123-J1 三错误码注入矩阵判据过，逐码与 Linux 语义一致，非页对齐 addr 全 EINVAL 格过
### UNX-F9124 · madvise 语义档总纲（六建议枚举）
- 域/批：C2/B17｜纯功能行数：400｜状态：｜判据：UNX-F9124-J1 MADV_NORMAL/SEQUENTIAL/RANDOM/WILLNEED/DONTNEED/FREE 六建议枚举判据过，未知建议全 EINVAL 格过
### UNX-F9125 · madvise MADV_DONTNEED 零页回收语义
- 域/批：C2/B17｜纯功能行数：340｜状态：｜判据：UNX-F9125-J1 零页回收判据过，回收后再访问零填充可观测，与页缓存联动依赖声明格过
### UNX-F9126 · madvise MADV_WILLNEED 预读挂点
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9126-J1 预读挂点协议落账判据过，零副作用承诺格过（提示性语义）
### UNX-F9127 · madvise MADV_SEQUENTIAL/RANDOM 模式提示
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9127-J1 两模式提示格落账判据过，访问模式登记可观测，语义边界（提示非强制）声明格过
### UNX-F9128 · madvise 错误矩阵（EINVAL/ENOMEM/EACCES/EBADF）
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9128-J1 四错误码注入矩阵判据过，逐码与 Linux 语义一致，锁页越限 EPERM 归 B24 分界声明格过
### UNX-F9129 · msync 语义档（MS_SYNC/MS_ASYNC/MS_INVALIDATE）
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9129-J1 三标志语义落账判据过，MS_SYNC 同步刷回挂点声明格过，非映射区间 ENOMEM 格过
### UNX-F9130 · mlock/munlock 语义档（RLIMIT_MEMLOCK 联动）
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9130-J1 锁页判据过，超限 EPERM/ENOMEM 分界格过，与资源族限额账对账一致
### UNX-F9131 · mlockall/munlockall 语义档
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9131-J1 MCL_CURRENT/MCL_FUTURE 两标志判据过，全地址空间锁语义格过，越限矩阵引用格过
### UNX-F9132 · mincore 语义档（驻留位向量）
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9132-J1 驻留位向量回读判据过，vec 长度校验格过，坏指针全 EFAULT 格过
### UNX-F9133 · mprotect 权限翻转与 #PF 联动复核
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9133-J1 翻转后写触发 #PF 且进程收 SIGSEGV 复测判据过，与 mmap 三联语义闭环一致
### UNX-F9134 · MAP_NORESERVE/MAP_POPULATE 标志位语义
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9134-J1 两标志语义落账判据过，POPULATE 预填页可观测，NORESERVE 零预留承诺格过
### UNX-F9135 · 内存族 OOM 联动（ENOMEM 注入路径）
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9135-J1 ENOMEM 注入判据过，与 OOM 分档联动协议声明格过，注入零内核损坏格过
### UNX-F9136 · 内存族参数探针总成（addr/length 对齐探针）
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9136-J1 页对齐探针判据过，未对齐 addr 全 EINVAL 格过，length 零值格过
### UNX-F9137 · 内存族与 glibc malloc 消费核
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9137-J1 malloc 双路（brk/mmap）消费链判据过，大块分配走 mmap 阈值可观测，消费号登记格过
### UNX-F9138 · 内存族升级接管注记
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9138-J1 接管边界声明判据过，现存 mmap 底座零重写承诺格过，升级增量清单落账
### UNX-F9139 · 内存族与 Windows VirtualAlloc 对照格
- 域/批：C2/B17｜纯功能行数：300｜状态：｜判据：UNX-F9139-J1 MEM_RESERVE/COMMIT 两阶段对照判据过，PAGE_* 保护位映射表落账，差异账三条全列
### UNX-F9140 · ktest syscall 面 B17 批断言集
- 域/批：C2/B17｜纯功能行数：320｜状态：｜判据：UNX-F9140-J1 本批 19 条判据聚合判据过，mremap/madvise/msync/mlock 四族一次跑通，域累计 79,770 收口断言
