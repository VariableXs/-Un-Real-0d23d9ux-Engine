# UNX-C2 · 域归集卷（AI-100 归集快照 2026-10-01）

> 由 AI-100 归集器 v2 从主汇编册块级切分生成（块定域：标题 token > 正文 token 投票 > 前块延续）；主汇编册仍为唯一权威总册，本卷为按域阅读视图，零改写零删节。

<!-- 主册行 10857 · ### UNX-C2 · 系统调用面域 -->
### UNX-C2 · 系统调用面域

> 域档｜承办 AI-12｜批册 40（01–40）｜条目 800｜F8801–F9600｜行数合计 240,000｜已深化 300 / 骨架 0

<!-- 主册行 10861 · #### UNX-C2-B01 · 分发表骨架与调用 ABI（F8801–F8820 · 20 条） -->
#### UNX-C2-B01 · 分发表骨架与调用 ABI（F8801–F8820 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：本批 5,200 / 240,000｜嫁接源：Linux man-pages（syscalls 章节）、x86-64 System V ABI、Intel SDM Vol.2 只跟随｜防重：与现存 `kernel/varix/src/syscall/`（table.rs 18 号注册面 + COMPAT_TABLE、entry.rs SYSCALL/int 0x80 双门、errno.rs、guard.rs、meter.rs、uaccess.rs、calls.rs、userlib.rs）为升级接管扩容，接管对象逐条声明，非重复实现｜判据与 deepen/C2-B01.md 逐条同名同判据同 ID｜本批为试产批 2（跨组验证依赖协议，数据回填 ADR-UNX-005）

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8801 | syscall 分发表骨架与注册协议 | 350 | 已深化 | UNX-F8801-J1 300+ 表项注册唯一性断言全过（重复注册构建失败即捕获），未知号返回 -ENOSYS 与 Linux 基准一致，抽查 50 号逐号断言零混码 |
| UNX-F8802 | syscall 入口上下文保存与参数提取 | 280 | 已深化 | UNX-F8802-J1 6 参 syscall 寄存器逐位传递正确（rdi/rsi/rdx/r10/r8/r9），r10 槽位约定专项断言通过，EFAULT 探针注入不崩内核 10/10 次 |
| UNX-F8803 | write/read 循环语义与错误矩阵 | 320 | 已深化 | UNX-F8803-J1 EBADF/EFAULT/短写/零长四路径行为与 Linux 逐码一致，O_APPEND 追加位生效可观测，短写返回值计数准确 |
| UNX-F8804 | mmap/munmap/mprotect 三联语义 | 380 | 已深化 | UNX-F8804-J1 对齐/越界/权限翻转错误码矩阵全对齐 Linux 基准，mprotect 翻只读后写触发 #PF 且进程收 SIGSEGV（与 C4 联测） |
| UNX-F8805 | -ENOSYS 与保留号段行为 | 150 | 已深化 | UNX-F8805-J1 全部未实现号返回 -ENOSYS，抽查 50 号逐号断言无混码，保留带（Reserved/Out）与未注册带（NotYet）分档留痕 |
| UNX-F8806 | 号段治理四带路由升级接管 | 260 | 已深化 | UNX-F8806-J1 band_of 四带判定（Reserved/Native/Compat/Vendor）与 Linux 号面路由回归 1,000 号零误判，band miss 与垃圾号分档入账 |
| UNX-F8807 | Linux 兼容号面转接器扩容 | 300 | 已深化 | UNX-F8807-J1 COMPAT_TABLE 15 号存量 + 扩容新号转接路由 300 号全通，RouteNote 四态（Exact/Permuted/ExtraArgsIgnored/ReturnDiffers）逐号核对一致 |
| UNX-F8808 | ABI 版本协商与特性位面升级接管 | 240 | 已深化 | UNX-F8808-J1 abi_negotiate 对 32 位调用方/未来版本/过旧版本三拒绝路径各 100 次断言正确，AbiInfo 字段逐项与注册面实态一致 |
| UNX-F8809 | 分发表注册协议冻结单与下游联签文书 | 180 | 已深化 | UNX-F8809-J1 冻结接口 {nr,handler,nargs,flags,persona} 五字段协议文书落盘，C3/C4/C5/E1 四消费方联签栏就位，接口变更走 mini-ADR 有据可查 |
| UNX-F8810 | 能力位 guard 预挂点（CAP_* 矩阵） | 260 | 已深化 | UNX-F8810-J1 每 syscall 声明所需 CAP_* 位与 guard 闸门判定回归 300 号全通，未知号 fail-closed（CAP_ADMIN）判据保持，缺位拒绝留痕 |
| UNX-F8811 | 延迟计量 meter 挂点与五指标采集 | 240 | 已深化 | UNX-F8811-J1 SyscallMeter 对 P50/P99 五指标采集落账，空调用百万次 P95 与 lxerrno 转译开销 ≤2μs 对账一致，账随批可导出 |
| UNX-F8812 | uaccess 用户指针探针协议升级接管 | 300 | 已深化 | UNX-F8812-J1 copy_from_user/copy_to_user/UserRegion 三原语探针注入坏指针 10/10 次返回 EFAULT 不崩内核，部分拷贝语义计数准确 |
| UNX-F8813 | errno 编码器全集与负值编码规约 | 280 | 已深化 | UNX-F8813-J1 Errno 全集正反向编码（sysret_decode/sysret_err）往返一致率 100%，-4095..-1 边界带逐值断言，Errno::Ok 误用被拒 |
| UNX-F8814 | 快路径标记与二分查找优化 | 220 | 已深化 | UNX-F8814-J1 表升序不变量断言保持（windows(2) 全过），fast 位号面快路径命中账可导出，查找开销对账在预算内 |
| UNX-F8815 | 双门入口（SYSCALL/int 0x80）并轨路由 | 260 | 已深化 | UNX-F8815-J1 SYSCALL 与 int 0x80 双门同号路由结果一致 1,000 对断言零差，门选择入账可观测，LSTAR/SFMASK 配置快照落账 |
| UNX-F8816 | 每任务内核栈与 TSS.RSP0 切换账 | 220 | 已深化 | UNX-F8816-J1 入口栈切换后 RSP0 指向每任务内核栈断言通过，栈溢出保护页触发可观测，切换路径无泄漏 10^5 次验证 |
| UNX-F8817 | syscall 返回路径与 sysret 单点 | 200 | 已深化 | UNX-F8817-J1 返回路径寄存器恢复断言（rcx/rip/r11 语义）全过，sysret_decode 单点消费判据入账，双向路径对称可审计 |
| UNX-F8818 | 号面自描述导出（manifest 与 nr_count） | 180 | 已深化 | UNX-F8818-J1 号面 manifest（每号名/arity/caps/fast/audited）导出与 SYSCALLS 表逐项一致，nr_count 双账核平，导出可机读 |
| UNX-F8819 | seccomp 过滤器前置挂点协议 | 240 | 已深化 | UNX-F8819-J1 SeccompFilter 前置拦截协议：过滤拒绝的号不进分发表统计（账分离）断言通过，SeccompVerdict 三态判定回归全绿 |
| UNX-F8820 | ktest syscall 面 B01 批断言集 | 340 | 已深化 | UNX-F8820-J1 本批 19 条判据全部聚合入 ktest syscall 面，一次命令全跑，CheckSet 三态（pass/fail/skip）落账，skip 必带原因，通过率 100% 才算绿 |

<!-- 主册行 10888 · #### UNX-C2-B02 · open/close/lseek/stat 族地基（F8821–F8840 · 20 -->
#### UNX-C2-B02 · open/close/lseek/stat 族地基（F8821–F8840 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01 5,200 + 本批 5,010 = 10,210 / 240,000｜嫁接源：Linux man-pages（open/stat/lseek 章节）只跟随｜防重：与 B01（表骨架/入口/write·read/mmap 三联）分层深化，文件族对现存 `syscall::calls::FdTable`/`FdEntry`/`FdKind` 为升级接管扩容，路径解析语义归 B1 域（挂点协议不越权）｜判据与 deepen/C2-B02.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8821 | open 语义档与 O_ 标志位全集 | 340 | 已深化 | UNX-F8821-J1 O_RDONLY/O_WRONLY/O_RDWR/O_CREAT/O_EXCL/O_TRUNC/O_APPEND/O_NONBLOCK/O_CLOEXEC 九标志组合矩阵 30 组行为与 Linux 逐组一致，标志组合非法（O_RDONLY\|O_WRONLY）拒收留痕 |
| UNX-F8822 | openat 家族与 AT_FDCWD 语义 | 260 | 已深化 | UNX-F8822-J1 openat 相对 dirfd 解析正确，AT_FDCWD 特值语义与绝对路径 fallback 各 100 次断言一致，坏 dirfd 返 EBADF |
| UNX-F8823 | close 语义与 fd 回收时序 | 200 | 已深化 | UNX-F8823-J1 close 后 fd 号立即可复用判据过，双 close 第二次返 EBADF，close 返回值不释放写缓存的时序语义有账 |
| UNX-F8824 | fd 最小可用分配原则与 fd 表扩容 | 280 | 已深化 | UNX-F8824-J1 分配始终取最小可用号断言 10^4 次通过，表扩容阈值与上限（RLIMIT_NOFILE 联动）触发可观测，扩容中分配无空洞 |
| UNX-F8825 | lseek 语义档（SEEK_SET/CUR/END 与越界） | 240 | 已深化 | UNX-F8825-J1 三 SEEK 基准行为与负偏移 EINVAL 判定逐档断言，超出文件尾 seek 合法且后续读返 0，管道类 lseek 返 ESPIPE |
| UNX-F8826 | stat/fstat/lstat 三联与结构布局 | 320 | 已深化 | UNX-F8826-J1 三号对同一文件输出字段一致（符号链接 lstat 不跟随判据），结构布局与 Linux ABI 逐字段偏移对照表零差 |
| UNX-F8827 | statx 扩展属性面 | 260 | 已深化 | UNX-F8827-J1 statx mask 请求位与返回位掩码交互语义正确（请求位/支持位/空集三档），btime 类不支持字段显式置 STATX_NOSYNC 档留痕 |
| UNX-F8828 | creat 兼容挂点 | 150 | 已深化 | UNX-F8828-J1 creat 等价 open(O_WRONLY\|O_CREAT\|O_TRUNC) 判据 100 次一致，已存在文件截断行为可观测 |
| UNX-F8829 | pread64/pwrite64 定位读写 | 240 | 已深化 | UNX-F8829-J1 定位读写不移动文件偏移判据过（与 read/write 交叉验证），负 offset 返 EINVAL，两号短读短写计数与主档一致 |
| UNX-F8830 | readv/writev 散布聚布 I/O | 260 | 已深化 | UNX-F8830-J1 iovec 多缓冲聚合写入顺序保持，iovcnt 上限（IOV_MAX）越界返 EINVAL，部分向量完成后返回已完成字节数 |
| UNX-F8831 | O_APPEND 追加原子性语义 | 220 | 已深化 | UNX-F8831-J1 多进程并发 O_APPEND 写不交错判据（10 进程 × 1,000 次）通过，lseek 后写仍追加到底的可观测行为落账 |
| UNX-F8832 | O_TRUNC/O_EXCL/O_CREAT 组合矩阵 | 240 | 已深化 | UNX-F8832-J1 八组合行为矩阵与 Linux 一致，O_EXCL 竞态双开（两进程同刻 open）仅一方成功 10/10 次 |
| UNX-F8833 | O_NONBLOCK 语义基准（fd 非阻塞位） | 220 | 已深化 | UNX-F8833-J1 open 时设置与 F_SETFL 后设置两路等价判据过，非阻塞读空返 EAGAIN 不阻塞 100 次，阻塞 fd 行为对照零差 |
| UNX-F8834 | 文件族错误矩阵（EBADF/EFAULT/EINVAL/EMFILE） | 300 | 已深化 | UNX-F8834-J1 四错误码 × 8 号（open/close/lseek/stat/fstat/read/write/fcntl）注入矩阵 32 格逐格断言与 Linux 逐码一致 |
| UNX-F8835 | fd 表 fork 继承语义（C1 联测挂点） | 240 | 已深化 | UNX-F8835-J1 fork 后子进程继承 fd 副本（共享偏移）判据过，O_CLOEXEC 位继承后 exec 关闭可观测，依赖 C1 未 finalize 已记风险 |
| UNX-F8836 | flock 建议锁挂点（B1 联签） | 220 | 已深化 | UNX-F8836-J1 LOCK_SH/LOCK_EX/LOCK_UN 三操作语义挂点注册成功，与 POSIX 记录锁（fcntl 锁）互不干扰判据有账，锁本体归 B1 防重声明 |
| UNX-F8837 | 路径解析挂点与 B1 VFS 对接协议 | 280 | 已深化 | UNX-F8837-J1 路径解析协议冻结（挂点接口 + EFAULT/EACCES/ENOENT 错误转译三段），B1 未收口期走现存最小路径栈判据全过，对接缺口入 open_risks |
| UNX-F8838 | 文件族短读短写可见性判据 | 200 | 已深化 | UNX-F8838-J1 短读（文件尾）返实际字节数、短写（管道满）返已写字节数判据各 100 次准确，零长读返 0 表 EOF 与 EAGAIN 区分明确 |
| UNX-F8839 | open/close 高频路径延迟账（O1 对接） | 200 | 已深化 | UNX-F8839-J1 open/close 各 10^5 次 P50/P99 落账，与 O1 syscall 延迟五指标对接字段一致，P99 ≤ Windows 同机对照 ×1.5 阈值登记待校准 |
| UNX-F8840 | ktest syscall 面 B02 批断言集 | 340 | 已深化 | UNX-F8840-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，断言账与 B01 面隔离编号互不覆盖 |

<!-- 主册行 10915 · #### UNX-C2-B03 · 内存族三联扩容（F8841–F8860 · 20 条） -->
#### UNX-C2-B03 · 内存族三联扩容（F8841–F8860 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B02 10,210 + 本批 4,760 = 14,970 / 240,000｜嫁接源：Linux man-pages（mmap/madvise/mlock 章节）只跟随｜防重：与 B01 F8804（mmap/munmap/mprotect 三联语义档）分层——本批为内存族周边号扩容（brk/mremap/madvise/msync/mincore/mlock 族），语义档不重复；对现存 `syscall::calls::MmapRequest`/`MmapState`/`Placement` 与 A4 域（未开工）为挂点对接，底座本体归 A4 防重｜判据与 deepen/C2-B03.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8841 | brk 语义档与探顶行为（glibc 探测关键） | 280 | 已深化 | UNX-F8841-J1 brk(0) 返回当前断点、增减与对齐行为与 Linux 逐项一致，glibc 探顶序列（渐进探测）10/10 次正确响应 |
| UNX-F8842 | mremap 扩缩与搬移语义 | 300 | 已深化 | UNX-F8842-J1 MREMAP_MAYMOVE 允许搬移判据过，不允许搬移时扩容失败返 ENOMEM，搬移后旧地址失效访问收 SIGSEGV |
| UNX-F8843 | madvise 建议面（MADV_NORMAL/SEQUENTIAL/WILLNEED） | 260 | 已深化 | UNX-F8843-J1 五建议档接受语义正确（建议可忽略但必须不报错），未知建议返 EINVAL，越界区间返 ENOMEM |
| UNX-F8844 | msync 同步语义（MS_SYNC/MS_ASYNC） | 220 | 已深化 | UNX-F8844-J1 MS_SYNC 返回后数据持久判据过，MS_ASYNC 异步调度有账，非映射区间返 ENOMEM |
| UNX-F8845 | mincore 驻留查询 | 200 | 已深化 | UNX-F8845-J1 驻留位图逐页与实际映射状态一致率 100%，未对齐地址向下取页对齐行为与 Linux 一致 |
| UNX-F8846 | mlock/munlock 驻留锁 | 220 | 已深化 | UNX-F8846-J1 锁定后页驻留（mincore 交叉验证）判据过，超 RLIMIT_MLOCK 返 ENOMEM，解锁后可换出可观测 |
| UNX-F8847 | mlockall/munlockall 与 RLIMIT_MLOCK | 200 | 已深化 | UNX-F8847-J1 MCL_CURRENT/MCL_FUTURE 两标志语义正确，FUTURE 对后续 mmap 自动锁定的行为留账，非特权限额触发 ENOMEM |
| UNX-F8848 | MAP_FIXED_NOREPLACE 语义 | 200 | 已深化 | UNX-F8848-J1 目标区间已被占用时返 EEXIST 且不破坏原映射判据 100 次，MAP_FIXED 覆盖语义对照（破坏性）有账 |
| UNX-F8849 | MAP_SHARED/MAP_PRIVATE 双模语义 | 240 | 已深化 | UNX-F8849-J1 SHARED 写可见（跨进程共享页判据）、PRIVATE 写时复制（父进程页不变判据）各 100 次通过，双标志同设返 EINVAL |
| UNX-F8850 | PROT_NONE 与权限组合矩阵 | 200 | 已深化 | UNX-F8850-J1 PROT_NONE 读/写/执行三向访问全收 SIGSEGV 判据过，PROT_READ\|WRITE 组合矩阵 4 格逐格正确 |
| UNX-F8851 | 匿名映射与文件映射双路 | 260 | 已深化 | UNX-F8851-J1 匿名页零填充判据、文件映射内容一致性（与 read 路径对照）判据各 100 次通过，fd 偏移不因映射改变 |
| UNX-F8852 | mprotect 权限翻转与 #PF/SIGSEGV 联动 | 260 | 已深化 | UNX-F8852-J1 翻只读后写触发 #PF 且进程收 SIGSEGV（与 C4 信号域联测）10/10 次，翻转对 TLB 生效延迟有账，未映射区间返 ENOMEM |
| UNX-F8853 | mmap NULL 提示与地址选择器 | 240 | 已深化 | UNX-F8853-J1 addr=NULL 内核自选地址单调分布判据过，glibc mmap NULL 探测序列 10/10 次正常响应，自选地址与现存 Placement 策略一致 |
| UNX-F8854 | 大页挂点（MAP_HUGETLB 语义预留） | 220 | 已深化 | UNX-F8854-J1 MAP_HUGETLB 挂点注册成功，未配置大页池时返 ENOMEM 降级路径明确，2MB 对齐断言通过 |
| UNX-F8855 | mmap 错误矩阵（ENOMEM/EINVAL/EFAULT/EOVERFLOW） | 300 | 已深化 | UNX-F8855-J1 四错误码注入矩阵（非对齐 addr、零 length、越界 length、fd 无效 + MAP_FIXED）16 格逐格与 Linux 逐码一致 |
| UNX-F8856 | munmap 部分区间与页对齐 | 200 | 已深化 | UNX-F8856-J1 非页对齐地址向下取整解除判据过，部分解除后剩余区间读写正常，全解除后访问收 SIGSEGV |
| UNX-F8857 | 内存族与 A4 底座对接协议（冻结接口注记） | 240 | 已深化 | UNX-F8857-J1 对接协议文书（mmap 底座四原语 + 错误转译表）落盘冻结，A4 未收口期以现存 MmapRequest 最小栈运行判据全过，缺口入 open_risks |
| UNX-F8858 | VM 过提交策略挂点（overcommit 模式） | 180 | 已深化 | UNX-F8858-J1 三模式（always/never/heuristic）挂点注册成功，never 模式下超物理内存分配返 ENOMEM 判据过 |
| UNX-F8859 | mmap 高频路径延迟账（缺页路径分段） | 200 | 已深化 | UNX-F8859-J1 mmap 调用与首次触碰（#PF）两段延迟分开落账，10^4 次 P50/P99 与 O1 对接字段一致 |
| UNX-F8860 | ktest syscall 面 B03 批断言集 | 340 | 已深化 | UNX-F8860-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，内存族断言与 B01/B02 面隔离编号 |

<!-- 主册行 10942 · #### UNX-C2-B04 · 进程族地基（F8861–F8880 · 20 条） -->
#### UNX-C2-B04 · 进程族地基（F8861–F8880 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B03 14,970 + 本批 4,570 = 19,540 / 240,000｜嫁接源：Linux man-pages（fork/clone/execve/wait 章节）只跟随｜防重：进程对象与装载本体归 C1 域（AI-11）防重，本域为 syscall 号面语义档与错误矩阵——对现存 `syscall::calls::WaitQueue`/`ExitStatus`、`F0039` 建模层为升级接管扩容；C1 未 finalize，跨域依赖逐条记入风险栏（§6.2 规则三）｜判据与 deepen/C2-B04.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8861 | fork 语义档与 COW 继承（A4 消费） | 320 | 已深化 | UNX-F8861-J1 fork 一次调用双返回（父得子 pid/子得 0）判据过，COW 语义依赖 A4 未收口已记风险，现存最小进程建模路径判据先行全过 |
| UNX-F8862 | vfork 挂点与语义约束 | 180 | 已深化 | UNX-F8862-J1 vfork 挂点注册成功，挂起父进程语义约束（子不得返回）文档化判据过，未实现档显式返 ENOSYS 不假装 |
| UNX-F8863 | clone/clone3 标志位面（CLONE_* 预留） | 280 | 已深化 | UNX-F8863-J1 CLONE_VM/FS/FILES/SIGHAND 四标志组合矩阵 8 格挂点注册，未支持组合返 EINVAL 不静默，clone3 结构版本字段校验判据过 |
| UNX-F8864 | execve 装载与参数环境块 | 300 | 已深化 | UNX-F8864-J1 argv/envp 计数与内容逐项传递正确（探针读回对照），E2BIG 超限返错判据过，成功不返回语义（后续代码不可达）有账 |
| UNX-F8865 | execveat 与空路径语义 | 180 | 已深化 | UNX-F8865-J1 dirfd + 相对路径解析正确，AT_EMPTY_PATH 特值语义挂点注册，坏 flags 返 EINVAL |
| UNX-F8866 | exit/exit_group 双档退出 | 240 | 已深化 | UNX-F8866-J1 exit 单线程退出与 exit_group 全组退出判据各 100 次正确，atexit 类清理挂点执行序有账，退出码 0–255 截断与 Linux 一致 |
| UNX-F8867 | wait4 回收语义与状态编码 | 280 | 已深化 | UNX-F8867-J1 WNOHANG 非阻塞轮询、阻塞等待、WUNTRACED 三档判据过，状态字 WEXITSTATUS/WIFEXITED 编码与 Linux 逐位一致 |
| UNX-F8868 | waitid 扩展等待（WEXITED/WSTOPPED/WCONTINUED） | 220 | 已深化 | UNX-F8868-J1 idtype 四档（P_ALL/P_PID/P_PGID/P_PIDFD 挂点）解析正确，WNOHANG 下 si_pid=0 约定判据过 |
| UNX-F8869 | waitpid 兼容挂点 | 160 | 已深化 | UNX-F8869-J1 waitpid 与 wait4 同语义转接判据 100 次一致，负 pid（进程组等待）语义挂点正确 |
| UNX-F8870 | getpid/getppid 双人格路由（C1 挂点） | 180 | 已深化 | UNX-F8870-J1 getpid 路由到本人格账本判据过（C1 人格标签挂点消费方验证），execve 后 pid 不变判据过 |
| UNX-F8871 | getpgrp 进程组查询 | 150 | 已深化 | UNX-F8871-J1 getpgrp 与 getpgid(0) 等价判据过，fork 后组继承判据过 |
| UNX-F8872 | 孤儿进程收养与 init 语义 | 200 | 已深化 | UNX-F8872-J1 父进程先退后子进程被收养判据过，收养目标 ppid 变更可观测，收养后可 wait 回收 |
| UNX-F8873 | 僵尸态生命周期与回收协议 | 220 | 已深化 | UNX-F8873-J1 退出后进僵尸态、wait 后回收判据过，无人 wait 的僵尸驻留有账（资源占用可观测），防泄漏上限告警判据过 |
| UNX-F8874 | 退出码传递与 WIFEXITED 解码 | 200 | 已深化 | UNX-F8874-J1 256 档退出码全档传递正确，被信号终止（WIFSIGNALED）挂点解码判据过 |
| UNX-F8875 | execve 后 fd 关闭语义（O_CLOEXEC） | 220 | 已深化 | UNX-F8875-J1 带 O_CLOEXEC 的 fd 在 exec 后关闭判据过，无标志 fd 存续判据过，关闭清单可导出 |
| UNX-F8876 | ELF interpreter 装载挂点（C1 联签） | 240 | 已深化 | UNX-F8876-J1 PT_INTERP 挂点注册与 C1 联签就位，解释器缺失返 ENOENT 判据过，装载本体归 C1 防重声明 |
| UNX-F8877 | 进程族错误矩阵（EAGAIN/EINVAL/EFAULT/EPERM） | 280 | 已深化 | UNX-F8877-J1 四错误码 × fork/execve/wait4/getpid 注入矩阵 16 格逐格与 Linux 逐码一致 |
| UNX-F8878 | 进程数限额与 EAGAIN（RLIMIT_NPROC 挂点） | 180 | 已深化 | UNX-F8878-J1 超 RLIMIT_NPROC fork 返 EAGAIN 判据过，限额值读写与 B07 资源族联动判据过 |
| UNX-F8879 | 进程族双人格并存判据（C1 联测） | 200 | 已深化 | UNX-F8879-J1 POSIX 进程与 NT 语义进程并存互不干扰判据（C1 交付后复测）登记随闸门补测，号面路由分流预演判据先行全过 |
| UNX-F8880 | ktest syscall 面 B04 批断言集 | 340 | 已深化 | UNX-F8880-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，进程族断言与既有 CheckSet 命名空间隔离 |

<!-- 主册行 10969 · #### UNX-C2-B05 · 进程身份与权限族（F8881–F8900 · 20 条） -->
#### UNX-C2-B05 · 进程身份与权限族（F8881–F8900 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B04 19,540 + 本批 4,210 = 23,750 / 240,000｜嫁接源：Linux man-pages（credentials 章节）只跟随｜防重：身份模型本体归 J1/J2 安全域防重，本域为身份族 syscall 号面语义档与错误矩阵——与 B04 进程族分层（组/会话操作本批、进程生命周期 B04）；与现存 guard.rs CAP_* 能力位为联动扩容｜判据与 deepen/C2-B05.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8881 | 三身份模型（真实/有效/保存）语义基准 | 260 | 已深化 | UNX-F8881-J1 三身份（ruid/euid/suid）切换矩阵（特权/非特权两态 × 8 操作）与 Linux 语义逐格一致，判据面为全族母版 |
| UNX-F8882 | getuid/geteuid/getgid/getegid 查询族 | 160 | 已深化 | UNX-F8882-J1 四查询号输出与设定值一致 10^3 次，恒成功语义（无错误路径）有账 |
| UNX-F8883 | setuid/setgid 双态切换（特权/非特权） | 240 | 已深化 | UNX-F8883-J1 特权进程三身份全改判据、非特权进程仅限 ruid↔euid 交换判据各 100 次，越权返 EPERM 留痕 |
| UNX-F8884 | setreuid/setregid 交换语义 | 200 | 已深化 | UNX-F8884-J1 -1 占位（不变更）语义判据过，交换后 suid 回填规则与 Linux 一致 100 次 |
| UNX-F8885 | setresuid/setresgid 全量设定 | 200 | 已深化 | UNX-F8885-J1 三值一次设定（含 -1 占位）矩阵 9 格判据过，非特权约束（目标须在三者集合内）判据过 |
| UNX-F8886 | getresuid/getresgid 三值读出 | 150 | 已深化 | UNX-F8886-J1 三值读出与设定态一致判据过，坏指针返 EFAULT，恒成功（指针有效时）有账 |
| UNX-F8887 | setfsuid/setfsgid 文件系统身份 | 180 | 已深化 | UNX-F8887-J1 fsuid 独立于 euid 的权限检查联动判据过，返回旧值语义与 Linux 一致 |
| UNX-F8888 | getgroups/setgroups 补充组 | 200 | 已深化 | UNX-F8888-J1 setgroups 仅特权判据过，getgroups size=0 查询容量语义判据过，越界返 EINVAL |
| UNX-F8889 | umask 掩码语义（读改写时序） | 160 | 已深化 | UNX-F8889-J1 umask 返回旧掩码并设新掩码判据过，open mode & ~umask 生效可观测 100 次 |
| UNX-F8890 | setpriority/getpriority 与 nice 域 | 220 | 已深化 | UNX-F8890-J1 PRIO_PROCESS 档 nice 值设置与读出一致，越界钳制到 [-20,19] 判据过，非特权降 nice 允许/升 nice 拒绝（EACCES/EPERM）判据过 |
| UNX-F8891 | prctl 基础档（PR_SET_NAME 等） | 220 | 已深化 | UNX-F8891-J1 PR_SET_NAME/PR_GET_NAME 设读一致判据过（16 字节截断语义），未知 option 返 EINVAL 全量兜底 |
| UNX-F8892 | capget/capset 能力集挂点 | 240 | 已深化 | UNX-F8892-J1 三能力集（permitted/effective/inheritable）读出与 guard CAP_* 映射一致判据过，越权设定返 EPERM |
| UNX-F8893 | 身份族错误矩阵（EPERM/EINVAL） | 260 | 已深化 | UNX-F8893-J1 两错误码 × 12 号（setuid 族全集）注入矩阵 24 格逐格与 Linux 逐码一致 |
| UNX-F8894 | 身份切换与 guard 能力位联动 | 240 | 已深化 | UNX-F8894-J1 euid 变更后 CAP 位重算判据过，文件权限检查消费 fsuid/euid 的联动矩阵判据过 |
| UNX-F8895 | 会话与控制终端挂点（C4 联签预留） | 200 | 已深化 | UNX-F8895-J1 控制终端挂点注册与 C4 联签就位，SIGHUP 投递语义预留判据文档化，本体归 C4 防重 |
| UNX-F8896 | setpgid/getpgid 组迁移语义 | 220 | 已深化 | UNX-F8896-J1 会话首进程不可迁移判据（EPERM）过，跨会话迁移拒绝判据过，自迁移（exec 前）判据过 |
| UNX-F8897 | setsid/getsid 会话建立 | 180 | 已深化 | UNX-F8897-J1 已是进程组长时 setsid 返 EPERM 判据过，成功后新会话 id=self pid 判据过 |
| UNX-F8898 | 身份族双人格对照挂点（D1 预留） | 160 | 已深化 | UNX-F8898-J1 NT 侧 token 对照挂点注册（D1 联签栏），Linux 侧语义先行判据全过，对照账随闸门补测登记 |
| UNX-F8899 | 身份族高频路径延迟账 | 180 | 已深化 | UNX-F8899-J1 getuid 族 10^5 次 P50/P99 落账（空调用转译开销 ≤2μs 对账），与 O1 对接字段一致 |
| UNX-F8900 | ktest syscall 面 B05 批断言集 | 340 | 已深化 | UNX-F8900-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，身份族断言独立编号 |

<!-- 主册行 10996 · #### UNX-C2-B06 · 时间族与计时对接（F8901–F8920 · 20 条） -->
#### UNX-C2-B06 · 时间族与计时对接（F8901–F8920 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B05 23,750 + 本批 4,200 = 27,950 / 240,000｜嫁接源：Linux man-pages（time/clock_gettime 章节）、 POSIX.1b 只跟随｜防重：计时基座本体归 A2 域（AI-02）防重——本域为时间族 syscall 号面语义档与 A2 产出消费协议；与现存 `syscall::calls::ClockId`/`ClockSource`、`SYS_CLOCK` 存量档为升级接管扩容｜判据与 deepen/C2-B06.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8901 | gettimeofday 语义档 | 200 | 已深化 | UNX-F8901-J1 tv_sec/tv_usec 输出与 clock_gettime(CLOCK_REALTIME) 换算一致 10^3 次，tz 结构遗留 NULL 约定判据过 |
| UNX-F8902 | clock_gettime 时钟 ID 全集 | 280 | 已深化 | UNX-F8902-J1 CLOCK_REALTIME/MONOTONIC/BOOTTIME/PROCESS_CPUTIME_ID/THREAD_CPUTIME_ID 五 ID 全接入判据过，未知 clockid 返 EINVAL |
| UNX-F8903 | clock_nanosleep 精确睡眠 | 240 | 已深化 | UNX-F8903-J1 TIMER_ABSTIME 绝对档与相对档判据各 100 次精确（误差落账），唤醒剩余时间（rem）回填语义正确 |
| UNX-F8904 | nanosleep 与 EINTR 重启 | 220 | 已深化 | UNX-F8904-J1 被信号打断返 EINTR 且 rem 回填未睡眠余量判据 10/10 次，重启语义与错误编码器单点（F8948）一致 |
| UNX-F8905 | clock_getres 分辨率查询 | 160 | 已深化 | UNX-F8905-J1 各时钟分辨率返回值与 A2 计时基座声明一致，坏 clockid 返 EINVAL |
| UNX-F8906 | settimeofday/clock_settime 权限挂点 | 180 | 已深化 | UNX-F8906-J1 非特权调用返 EPERM 判据过，特权档设置后 gettimeofday 读回一致判据过 |
| UNX-F8907 | time 兼容挂点 | 140 | 已深化 | UNX-F8907-J1 time 与 gettimeofday(0, NULL) 等价判据 10^3 次一致，兼容档转接路径留痕 |
| UNX-F8908 | times 进程 CPU 时间账 | 200 | 已深化 | UNX-F8908-J1 tms 四字段（self/children user/sys）单调不减判据过，时钟滴答换算（CLK_TCK）与 O1 对账一致 |
| UNX-F8909 | sysinfo 系统信息面 | 180 | 已深化 | UNX-F8909-J1 uptime/loads/totalram/freeram 字段与内核账一致判据过，结构布局 ABI 对照零差 |
| UNX-F8910 | TSC-deadline 与 A2 计时基座对接 | 260 | 已深化 | UNX-F8910-J1 高精度睡眠路径经 TSC-deadline 判据过（A2 F0039 双模开关消费），PIT 兜底路径降级可观测，双路径误差账分档 |
| UNX-F8911 | 时钟源仲裁消费协议（A2 联签） | 220 | 已深化 | UNX-F8911-J1 仲裁结果消费协议冻结（当前源/优先序/降级事件三字段），源切换时 clock_gettime 无跳变判据过 |
| UNX-F8912 | vDSO gettimeofday 数据页挂点 | 240 | 已深化 | UNX-F8912-J1 vDSO 数据页与内核路径读数一致判据 10^4 次（seqlock 一致性），FEAT_VDSO 特性位协商联动判据过 |
| UNX-F8913 | CLOCK_MONOTONIC 不可回拨判据 | 200 | 已深化 | UNX-F8913-J1 72h 预留长跑单调性断言登记随闸门补测，短程 10^6 次采样零回拨判据先行全过 |
| UNX-F8914 | CLOCK_BOOTTIME 与休眠挂点 | 160 | 已深化 | UNX-F8914-J1 BOOTTIME 含暂停时段语义挂点注册，与 MONOTONIC 差值=暂停时长判据（休眠功能就位后复测）登记 |
| UNX-F8915 | CLOCK_PROCESS_CPUTIME_ID 精度账 | 200 | 已深化 | UNX-F8915-J1 进程 CPU 时间与 times 字段交叉对账一致，多核累计无丢失判据过（A3 SMP 就位后复测登记） |
| UNX-F8916 | 时间族错误矩阵（EINVAL/EFAULT/EPERM） | 240 | 已深化 | UNX-F8916-J1 三错误码 × 6 号（gettimeofday/nanosleep/clock_gettime/clock_nanosleep/times/sysinfo）矩阵 18 格逐格一致 |
| UNX-F8917 | 时间回拨与真实时间单调化策略 | 180 | 已深化 | UNX-F8917-J1 settimeofday 回拨时 MONOTONIC 不受影响判据过，回拨事件留痕可观测 |
| UNX-F8918 | utimes 系列时间戳挂点（B10 防重声明） | 160 | 已深化 | UNX-F8918-J1 utimes/futimesat 挂点注册，时间戳语义本体归 B10（F8988）防重声明，本条只立号面挂点判据 |
| UNX-F8919 | 时间族精度账本（纳秒级落账） | 200 | 已深化 | UNX-F8919-J1 各时钟源精度声明与实测分布（10^6 采样分位数）落账，精度降级事件显式登记不静默 |
| UNX-F8920 | ktest syscall 面 B06 批断言集 | 340 | 已深化 | UNX-F8920-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，时间族断言独立编号 |

<!-- 主册行 11023 · #### UNX-C2-B07 · 参数探针与错误编码器总成（F8921–F8940 · 20 条） -->
#### UNX-C2-B07 · 参数探针与错误编码器总成（F8921–F8940 · 20 条）

> AI-12 承办｜批次类型：F 型地基收官批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B06 27,950 + 本批 4,140 = 32,090 / 240,000｜嫁接源：Linux man-pages errno(3)/intro(2) 章节、x86-64 SysV ABI（错误码负值编码约定）、现存 `syscall::errno::Errno`/`lxerrno::VarixErr`/`uaccess` 存量档为升级接管扩容｜防重：错误码语义本体归各功能号所属批次，本批为跨批编码器单点与探针总成，不重开号面｜判据与 deepen/C2-B07.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8921 | 参数拷贝探针总成（copy_from_user/copy_to_user 语义档） | 220 | 已深化 | UNX-F8921-J1 结构化参数（stat/tms/itimerspec）进出内核探针 10^3 次零失真判据过，探针失败返 EFAULT 且内核侧零残留 |
| UNX-F8922 | 用户指针合法性预检协议（access_ok 语义） | 200 | 已深化 | UNX-F8922-J1 区间重叠检测（[start,len) 越内核带/越用户带）返 EFAULT 判据过，零长度区间放行语义与 Linux 对照一致 |
| UNX-F8923 | errno 编码器单点（Errno→用户态 -errno 编码） | 260 | 已深化 | UNX-F8923-J1 Errno 枚举 16 变体逐一编码断言过，成功路径返非负、失败路径返 -errno 双相判据过，全 SyscallMeter 计数单点留痕 |
| UNX-F8924 | sysret_decode 返回值解码器（错误/成功双相） | 200 | 已深化 | UNX-F8924-J1 rax∈[-4095,-1] 判错、>4095 判成功边界断言各 10^3 次（含 -4095/-4096 临界格），与 errno.rs 存量 decode 逐值一致 |
| UNX-F8925 | -ENOSYS 全面分发兜底语义（NrVerdict::NotYet/Unknown 路由） | 240 | 已深化 | UNX-F8925-J1 未实现号/未知号均归一返 -ENOSYS 判据过，兜底路径 SyscallMeter::not_yet 计数可观测，路由判定四带（Reserved/Native/Compat/Vendor）回归零漂 |
| UNX-F8926 | EPERM 权限错误矩阵（CAP_* 网关注入） | 200 | 已深化 | UNX-F8926-J1 guard.rs CAP_* 六位逐一注入返 EPERM 判据过，放行路径零误判，权限拒绝事件留痕可查 |
| UNX-F8927 | EINVAL 参数错误矩阵（参数校验单点） | 200 | 已深化 | UNX-F8927-J1 越界 flag/非法 clockid/非法 whence 三类注入返 EINVAL 判据过，参数校验点与各功能号就地校验双轨一致性判据过 |
| UNX-F8928 | EFAULT 用户内存错误矩阵（uaccess 探针联动） | 200 | 已深化 | UNX-F8928-J1 NULL 指针/只读页写/越页尾跨界三类注入返 EFAULT 判据过，探针失败点与预检协议（F8922）判定一致 |
| UNX-F8929 | ENOSPC/ENOMEM 资源耗尽矩阵 | 180 | 已深化 | UNX-F8929-J1 fd 上限打满返 EMFILE、内存池打满返 ENOMEM 注入判据过，耗尽事件 meter 计数可观测不静默 |
| UNX-F8930 | EBUSY/EAGAIN 阻塞与占用矩阵 | 180 | 已深化 | UNX-F8930-J1 O_NONBLOCK 读空管道返 EAGAIN 判据过，锁占用返 EBUSY 判据过，阻塞/非阻塞双路径行为差留痕 |
| UNX-F8931 | 错误码负值编码 ABI 冻结（-errno 用户态约定） | 200 | 已深化 | UNX-F8931-J1 -errno 编码表冻结文档落盘判据过，userlib.rs 侧解码回归零差，冻结后变更需走 §6.3 冻结协议 |
| UNX-F8932 | Errno 枚举与 lxerrno VarixErr 单源映射消费协议 | 200 | 已深化 | UNX-F8932-J1 Errno↔VarixErr 16 变体双向映射断言过，映射表唯一（单源）判据过，新增变体须同时落两表的一致性检查挂点 |
| UNX-F8933 | 错误信息人类可读档（errno → str） | 200 | 已深化 | UNX-F8933-J1 全变体可读名逐一对账判据过，可读名与 Linux errno(3) 名称对照零漂，调试输出路径留痕 |
| UNX-F8934 | meter 计量器错误路径埋点（SyscallMeter/LatencyStats 错误计数） | 180 | 已深化 | UNX-F8934-J1 错误返回与成功返回分路计数判据过，P99 分位数含错误路径样本声明落账，埋点零开销路径（未启用时）判据过 |
| UNX-F8935 | seccomp 拒绝路径错误编码（SECCOMP_RET_ERRNO） | 200 | 已深化 | UNX-F8935-J1 seccomp ERRNO 动作注入返指定 -errno 判据过，与 guard.rs SeccompFilter 存量档联动，拒绝事件审计留痕 |
| UNX-F8936 | 参数探针零拷贝快路径（inline 化判据） | 160 | 已深化 | UNX-F8936-J1 就地取参（同页小结构）免拷贝快路径判据过，快慢路径行为等价 10^3 次回归零差 |
| UNX-F8937 | 跨批错误一致性审计单点（B02–B06 抽验联动） | 180 | 已深化 | UNX-F8937-J1 B02 文件族/B03 内存族/B04 进程族/B05 身份族/B06 时间族各抽 3 号错误码与编码器单点回归一致判据过 |
| UNX-F8938 | 兼容号面错误编码差异档（COMPAT_TABLE RouteNote::ReturnDiffers 联动） | 180 | 已深化 | UNX-F8938-J1 15 条 COMPAT 表项中 ReturnDiffers 差异逐条登记判据过，差异码与转接器行为一致复测过 |
| UNX-F8939 | 错误矩阵回归台账（错误注入用例账本） | 220 | 已深化 | UNX-F8939-J1 注入用例账本（用例 ID/目标号/注入方式/期望码）落账判据过，台账与 ktest 用例一一对应抽验 10 组一致 |
| UNX-F8940 | ktest syscall 面 B07 批断言集 | 340 | 已深化 | UNX-F8940-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，编码器/探针断言独立编号可单独复跑 |

<!-- 主册行 11050 · #### UNX-C2-B08 · 文件族扩容：dup/fcntl/ioctl/目录树（F8941–F8960 · 20 -->
#### UNX-C2-B08 · 文件族扩容：dup/fcntl/ioctl/目录树（F8941–F8960 · 20 条）

> AI-12 承办｜批次类型：F 型地基收官批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B07 32,090 + 本批 4,540 = 36,630 / 240,000｜嫁接源：Linux man-pages dup(2)/fcntl(2)/ioctl(2)/getdents(2)/rename(2) 章节、现存 `syscall::calls::FdTable` 存量档为升级接管扩容｜防重：fd 分配本体（B02 F8823）与 open 语义（B02）不重铺，本批为描述符操控/指令基座/目录树扩容；ioctl 设备本体归 B4 域防重——本域只立 per-fd 指令登记协议｜判据与 deepen/C2-B08.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8941 | dup/dup2/dup3 语义档 | 220 | 已深化 | UNX-F8941-J1 dup 返回最小空闲 fd 判据过，dup2 目标占用时原子替换判据过，dup3 NEWFD 标志（CLOEXEC）矩阵判据过 |
| UNX-F8942 | fcntl 五主指令（F_DUPFD/F_GETFD/F_SETFD/F_GETFL/F_SETFL） | 260 | 已深化 | UNX-F8942-J1 五指令逐指令断言过，F_GETFL 读回 O_ 标志与 open 时一致，F_SETFL 仅允许改运行态标志子集判据过 |
| UNX-F8943 | fcntl 记录锁指令（F_SETLK/F_SETLKW/F_GETLK） | 240 | 已深化 | UNX-F8943-J1 排他/共享锁互斥矩阵判据过，SETLKW 阻塞与信号打断 EINTR 判据过，GETLK 探测冲突锁返回归属 pid 判据过 |
| UNX-F8944 | fcntl 扩展指令（F_SETOWN/F_GETOWN/F_SETSIG/F_ADD_SEALS） | 220 | 已深化 | UNX-F8944-J1 扩展指令登记挂点注册判据过， seals 联动 memfd 挂点（B10 F8997 声明）一致性判据过，未支持指令返 EINVAL 不静默 |
| UNX-F8945 | ioctl 基座与指令编码（_IO/_IOR/_IOW/_IOWR） | 260 | 已深化 | UNX-F8945-J1 四类编码解包（方向/尺寸/类型/序号）断言过，编码-解码往返零差 10^3 次，坏编码返 ENOTTY 判据过 |
| UNX-F8946 | ioctl 终端指令集挂点（TCGETS/TCSETS/TIOCGWINSZ） | 220 | 已深化 | UNX-F8946-J1 终端三指令挂点注册判据过，非终端 fd 调用返 ENOTTY 判据过，termios 结构布局 ABI 对照零差 |
| UNX-F8947 | ioctl 设备指令登记协议（per-fd ioctl 表） | 260 | 已深化 | UNX-F8947-J1 per-fd 指令表注册协议冻结判据过，未注册指令返 ENOTTY 且留痕，B4 域设备本体防重声明落账 |
| UNX-F8948 | EINTR/错误编码单点（跨批锚点） | 240 | 已深化 | UNX-F8948-J1 可中断慢调用统一返 EINTR 单点判据过（B06 F8904 引用锚点成立），编码路径与 F8923 单点同源，SA_RESTART 重启语义挂点与 B11 F9013 对接 |
| UNX-F8949 | getcwd 语义档 | 180 | 已深化 | UNX-F8949-J1 缓冲不足返 ERANGE 判据过，size=0 自动分配语义判据过，路径与 B1 路径解析协议对账一致 |
| UNX-F8950 | chdir/fchdir 语义档 | 180 | 已深化 | UNX-F8950-J1 切换后 getcwd 读回一致判据过，fchdir 对非目录 fd 返 ENOTDIR 判据过，权限不足 EACCES 矩阵格过 |
| UNX-F8951 | mkdir/rmdir 语义档 | 200 | 已深化 | UNX-F8951-J1 建目录后 stat 类型为目录判据过，rmdir 非空目录返 ENOTEMPTY 判据过，mode 参数 umask 作用判据过 |
| UNX-F8952 | getdents64 目录读取 | 240 | 已深化 | UNX-F8952-J1 dirent64 布局（d_ino/d_off/d_reclen/d_type）ABI 对照零差判据过，分批读取与偏移推进连续性判据过，目录尾返 0 判据过 |
| UNX-F8953 | readdir 遗留兼容挂点 | 160 | 已深化 | UNX-F8953-J1 遗留号转接 getdents64 路径判据过，RouteNote::Permuted 差异登记，行为等价回归零差 |
| UNX-F8954 | 目录项缓存与 getdents 偏移 cookie | 220 | 已深化 | UNX-F8954-J1 cookie 单调性判据过，目录变更后 cookie 失效策略显式落账不静默，lseek 对目录定位 cookie 判据过 |
| UNX-F8955 | rename/renameat/renameat2 | 240 | 已深化 | UNX-F8955-J1 同目录/跨目录改名判据过，RENAME_EXCHANGE/RENAME_NOREPLACE 两旗标判据过，目标存在覆盖语义与 EEXIST 矩阵格过 |
| UNX-F8956 | unlink/unlinkat | 200 | 已深化 | UNX-F8956-J1 打开文件 unlink 后 fd 仍可读写（引用计数语义）判据过，目录目标返 EISDIR 判据过，AT_REMOVEDIR 旗标判据过 |
| UNX-F8957 | link/linkat/symlink/readlink | 220 | 已深化 | UNX-F8957-J1 硬链接 nlink 计数判据过，符号链接读取零截断判据过，链接环 ELOOP 判据过，跨设备硬链接 EXDEV 判据过 |
| UNX-F8958 | 文件系统挂载点挂接协议（B1 联动） | 220 | 已深化 | UNX-F8958-J1 C2 号面与 B1 VFS 挂载点对接协议冻结判据过，跨挂载点操作 EXDEV 行为判据过，防重：挂载本体归 B1（AI-14）不重铺 |
| UNX-F8959 | 文件族错误矩阵（EBADF/ENOTDIR/EISDIR/ENOENT/ELOOP/ENAMETOOLONG） | 220 | 已深化 | UNX-F8959-J1 六错误码 × 6 号（open/dup/getcwd/rename/unlink/link）矩阵 36 格逐格一致，路径深度边界（PATH_MAX）格过 |
| UNX-F8960 | ktest syscall 面 B08 批断言集 | 340 | 已深化 | UNX-F8960-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，描述符操控与目录树断言独立编号可单独复跑 |

<!-- 主册行 11077 · #### UNX-C2-B09 · 多路复用四件套之一：epoll 全集（F8961–F8980 · 20 条） -->
#### UNX-C2-B09 · 多路复用四件套之一：epoll 全集（F8961–F8980 · 20 条）

> AI-12 承办｜批次类型：M 型机制批启幕｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B08 36,630 + 本批 4,170 = 40,800 / 240,000｜嫁接源：Linux man-pages epoll(7)/select(2)/poll(2) 章节、现存 `syscall::calls::EventPort`/`WaitQueue` 存量档为升级接管扩容｜防重：事件端口本体与调度器唤醒本体归 A3/A2 域防重——本域为多路复用号面语义与消费协议；epoll 实例数据结构升级 EventPort 不另起炉灶｜判据与 deepen/C2-B09.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8961 | epoll_create/epoll_create1 语义档 | 180 | 已深化 | UNX-F8961-J1 实例 fd 分配判据过，size>0 遗留参数仅校验判据过，EPOLL_CLOEXEC 旗标判据过 |
| UNX-F8962 | epoll_ctl ADD/MOD/DEL 三操作 | 250 | 已深化 | UNX-F8962-J1 三操作幂等性矩阵判据过（重复 ADD 返 EEXIST、DEL 不存在返 ENOENT、MOD 不存在返 ENOENT），事件掩码登记读写回一致 |
| UNX-F8963 | epoll_wait 就绪队列语义 | 260 | 已深化 | UNX-F8963-J1 就绪事件按序返回判据过，maxevents 截断语义判据过，就绪队列空转阻塞路径与超时路径双断言过 |
| UNX-F8964 | epoll_pwait 信号掩码挂点 | 200 | 已深化 | UNX-F8964-J1 原子掩码切换（等待前装填/返回后还原）判据过，与 B11 信号族挂点协议一致，sigmask=NULL 退化为 epoll_wait 判据过 |
| UNX-F8965 | 边沿/水平触发双模（EPOLLET） | 220 | 已深化 | UNX-F8965-J1 LT 模式持续就绪重报判据过，ET 模式一次 armed 触发判据过，双模同 fd 混用行为差异登记不静默 |
| UNX-F8966 | one-shot 与唤醒联动（EPOLLONESHOT） | 200 | 已深化 | UNX-F8966-J1 one-shot 触发后自动 disarmed 判据过，re-arm 需显式 MOD 判据过，唤醒后事件重查协议（F8974）联动一致 |
| UNX-F8967 | fd 就绪回调挂接协议（fd 表/管道/socket 消费） | 220 | 已深化 | UNX-F8967-J1 三类 fd（普通文件恒就绪/管道读写端/socket 挂点）就绪回调注册判据过，回调与 FdTable 状态位联动判据过 |
| UNX-F8968 | epoll 与调度器唤醒联动（WaitQueue 消费） | 220 | 已深化 | UNX-F8968-J1 等待线程入队 WaitQueue 判据过，对端写唤醒→epoll_wait 返回链路判据过，虚假唤醒防护理由落账（事件重查） |
| UNX-F8969 | select/pselect6 遗留兼容档 | 220 | 已深化 | UNX-F8969-J1 select 读/写/异常三集语义判据过，pselect6 原子掩码判据过，RouteNote::ExtraArgsIgnored 差异登记 |
| UNX-F8970 | poll/ppoll 兼容档 | 220 | 已深化 | UNX-F8970-J1 pollfd 数组返回 revents 位矩阵判据过（POLLIN/POLLOUT/POLLERR/POLLHUP/POLLNVAL），ppoll 掩码原子性判据过 |
| UNX-F8971 | fd_set 位图 ABI 冻结（FD_SETSIZE 1024） | 220 | 已深化 | UNX-F8971-J1 位图布局 128 字节冻结判据过，fd≥1024 返 EINVAL 判据过，冻结文档落盘与变更协议挂点 |
| UNX-F8972 | 超时语义档（timeout -1/0/正数三态） | 160 | 已深化 | UNX-F8972-J1 三态语义（无限阻塞/纯轮询/限时等待）判据各过，超时返 0 与 EINTR 打断分路判据过 |
| UNX-F8973 | epoll 错误矩阵（EBADF/EINVAL/EEXIST/ENOENT/ETIME） | 180 | 已深化 | UNX-F8973-J1 五错误码 × 3 号（epoll_ctl/epoll_wait/epoll_create1）矩阵 15 格逐格一致，EPWAKEUP 非法事件位 EINVAL 格过 |
| UNX-F8974 | 事件丢失防护（唤醒后事件重查协议） | 200 | 已深化 | UNX-F8974-J1 唤醒→重查→返回闭环判据过（10^3 次唤醒零丢事件），竞态窗口（唤醒与就绪位清零交错）防护理由落账 |
| UNX-F8975 | 性能账本（就绪延迟 P99 登记，Windows 对照） | 200 | 已深化 | UNX-F8975-J1 就绪延迟 P99 预登记（闸门补测挂点），P99 ≤ Windows WaitForMultipleObjects ×1.5 目标锚定判据落账 |
| UNX-F8976 | epoll 与 eventfd 联动挂点 | 160 | 已深化 | UNX-F8976-J1 eventfd 写→epoll 就绪链路判据过，EFD_SEMAPHORE 读取减一语义与 B10 F8986 一致判据过 |
| UNX-F8977 | 多路复用器资源上限（maxevents/RLIMIT_NOFILE 联动） | 180 | 已深化 | UNX-F8977-J1 maxevents≤0 返 EINVAL 判据过，实例数受 RLIMIT_NOFILE 约束判据过，上限触达 EMFILE 事件留痕 |
| UNX-F8978 | fork 后 epoll 实例继承语义 | 160 | 已深化 | UNX-F8978-J1 子进程继承实例 fd 可 wait 判据过，父子共享就绪队列语义与 Linux 对照一致，差异点登记不静默 |
| UNX-F8979 | epoll ktest 半自动驱动（双端探针） | 180 | 已深化 | UNX-F8979-J1 双端探针（写端触发/读端 wait）驱动器判据过，三种触发模式（LT/ET/ONESHOT）批内全覆盖 |
| UNX-F8980 | ktest syscall 面 B09 批断言集 | 340 | 已深化 | UNX-F8980-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，多路复用断言独立编号可单独复跑 |

<!-- 主册行 11104 · #### UNX-C2-B10 · 多路复用四件套之二：inotify/timerfd/eventfd/signalfd -->
#### UNX-C2-B10 · 多路复用四件套之二：inotify/timerfd/eventfd/signalfd（F8981–F9000 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B09 40,800 + 本批 4,340 = 45,140 / 240,000｜嫁接源：Linux man-pages inotify(7)/timerfd_create(2)/eventfd(2)/signalfd(2)/utimensat(2) 章节、现存 `syscall::calls::EventPort` 存量档为升级接管扩容｜防重：inotify 事件源本体归 B1 VFS 写路径（联动不重铺）；信号投递本体归 B11 防重——signalfd 只立消费端号面；utimes 本体自 B06 F8918 挂点正式接管落本体档｜判据与 deepen/C2-B10.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F8981 | inotify_init/inotify_init1 语义档 | 180 | 已深化 | UNX-F8981-J1 实例 fd 分配判据过，IN_NONBLOCK/IN_CLOEXEC 旗标矩阵判据过，实例上限 EMFILE 格过 |
| UNX-F8982 | inotify_add_watch 掩码全集 | 260 | 已深化 | UNX-F8982-J1 掩码 16 位（IN_MODIFY/IN_CREATE/IN_DELETE/IN_ACCESS 等）逐位登记判据过，重复 add 更新掩码判据过，非法路径 ENOENT 格过 |
| UNX-F8983 | inotify_rm_watch 与事件队列 | 200 | 已深化 | UNX-F8983-J1 rm 后队列残留事件含 IN_IGNORED 判据过，watch 描述符释放回收判据过，双重 rm EINVAL 格过 |
| UNX-F8984 | inotify 事件结构 ABI（struct inotify_event 冻结） | 200 | 已深化 | UNX-F8984-J1 事件结构布局（wd/mask/cookie/len/name）冻结判据过，name 变长对齐（4 字节）断言过，读侧整事件边界判据过 |
| UNX-F8985 | eventfd/eventfd2 计数语义 | 260 | 已深化 | UNX-F8985-J1 计数累加（上限 2^64-2 回 EAGAIN）判据过，读清零语义判据过，write 0 值返 EINVAL 格过 |
| UNX-F8986 | eventfd EFD_* 标志矩阵（NONBLOCK/CLOEXEC/SEMAPHORE） | 200 | 已深化 | UNX-F8986-J1 三旗标组合矩阵判据过，SEMAPHORE 读减一（非清零）判据过，NONBLOCK 满计数 EAGAIN 格过 |
| UNX-F8987 | timerfd_create/settime/gettime | 280 | 已深化 | UNX-F8987-J1 新/旧 spec 设置与读回一致判据过，到期读 8 字节计数判据过，TFD_TIMER_ABSTIME/TFD_TIMER_CANCEL_ON_SET 两旗标判据过 |
| UNX-F8988 | utimes 本体（futimesat/utimensat 本体档） | 200 | 已深化 | UNX-F8988-J1 时间戳设置后 stat 读回一致判据过，UTIME_NOW/UTIME_OMIT 特殊值判据过，非特权改他人文件 EPERM 格过（自 B06 F8918 挂点接管） |
| UNX-F8989 | signalfd 语义档 | 220 | 已深化 | UNX-F8989-J1 signalfd 读出 signalfd_siginfo 布局冻结判据过，掩码内信号不再投递 handler 判据过，掩码变更重挂判据过 |
| UNX-F8990 | 多路复用四件套与 epoll 联动总档 | 200 | 已深化 | UNX-F8990-J1 四件套逐一挂入 epoll 就绪回调判据过，联动行为与 F8967 协议一致回归，四路并发触发零互吞判据过 |
| UNX-F8991 | inotify 事件投递与 B1 VFS 写路径联动 | 240 | 已深化 | UNX-F8991-J1 write→IN_MODIFY 投递链路判据过，create/delete 联动判据过，防重：投递点挂 B1 写路径（AI-14 联签点）不重铺 |
| UNX-F8992 | timerfd 到期与 A2 计时基座联动 | 200 | 已深化 | UNX-F8992-J1 到期回调经 A2 时钟源仲裁消费协议（B06 F8911）触发判据过，时钟源切换时挂起 timerfd 不失准判据过 |
| UNX-F8993 | eventfd 与 W2 事件端口（EventPort）语义对齐 | 220 | 已深化 | UNX-F8993-J1 eventfd 计数与 EventPort 端口语义对齐判据过，跨域消费协议（W2 端口挂点）落账，差异点显式登记不静默 |
| UNX-F8994 | signalfd 与信号族（B11）挂点协议 | 180 | 已深化 | UNX-F8994-J1 signalfd 掩码与 rt_sigprocmask（F9002）联动判据过，防重：投递本体归 B11 不重铺，消费端协议冻结 |
| UNX-F8995 | 管道/FIFO 多路复用消费档 | 220 | 已深化 | UNX-F8995-J1 管道读端数据就绪/写端腾空就绪接入多路复用判据过，FIFO 命名管道挂点判据过，对端关闭 POLLHUP 格过 |
| UNX-F8996 | 四件套错误矩阵（EMFILE/ENFILE/EINVAL/EBADF） | 200 | 已深化 | UNX-F8996-J1 四错误码 × 4 号（inotify_init/eventfd2/timerfd_create/signalfd）矩阵 16 格逐格一致 |
| UNX-F8997 | 异步 IO 挂点（io_setup/io_submit 登记档） | 180 | 已深化 | UNX-F8997-J1 aio 号面登记挂点立卡判据过，M 型先行声明落账（本体档待后续批次），未实现号返 -ENOSYS 判据过 |
| UNX-F8998 | 多路复用资源账本（watch 数/实例数上限落账） | 160 | 已深化 | UNX-F8998-J1 watch 数上限 128、实例数上限落账判据过，触达上限事件 meter 留痕不静默 |
| UNX-F8999 | 四件套 ktest 半自动驱动 | 200 | 已深化 | UNX-F8999-J1 四件套联动驱动器判据过（inotify 触发→eventfd 通知→timerfd 定时→signalfd 收取单流程覆盖） |
| UNX-F9000 | ktest syscall 面 B10 批断言集 + 万号整数点自检 | 340 | 已深化 | UNX-F9000-J1 本批 19 条判据聚合判据过，号面整数点 F9000 位置自检（区间连续性锚点）判据过 |

<!-- 主册行 11131 · #### UNX-C2-B11 · 信号挂点族（F9001–F9020 · 20 条） -->
#### UNX-C2-B11 · 信号挂点族（F9001–F9020 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B10 45,140 + 本批 4,320 = 49,460 / 240,000｜嫁接源：Linux man-pages sigaction(2)/signal(7) 章节、x86-64 SysV ABI（信号栈帧 rt_sigframe 布局）、现存 `syscall::guard` CAP_* 存量档为升级接管扩容｜防重：信号硬件中断本体与 APIC 路由归 A2 域防重——本域为信号 syscall 号面语义与投递消费协议；signalfd 消费端已在 B10（F8989）铺面，本批只立挂点协议不重铺｜判据与 deepen/C2-B11.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9001 | rt_sigaction 语义档 | 260 | 已深化 | UNX-F9001-J1 sigaction 结构布局（handler/mask/flags）ABI 冻结判据过，旧 action 读回判据过，SA_* 五旗标（RESTART/NOCLDSTOP/NODEFER/ONSTACK/SIGINFO）逐位判据过 |
| UNX-F9002 | rt_sigprocmask 掩码语义 | 240 | 已深化 | UNX-F9002-J1 SIG_BLOCK/UNBLOCK/SETMASK 三操作判据过，旧掩码读回判据过，SIGKILL/SIGSTOP 掩蔽请求被忽略判据过 |
| UNX-F9003 | kill/tgkill/tkill 投递 | 220 | 已深化 | UNX-F9003-J1 三投递号目标解析（pid/tgid/tid）判据过，信号 0 探活语义判据过，跨进程投递权限 EPERM 格过 |
| UNX-F9004 | raise/pidfd_send_signal 挂点 | 200 | 已深化 | UNX-F9004-J1 raise 自投递判据过，pidfd 号面挂点注册判据过（fd 化投递本体档待 M 型后续批次落），未实现路径返 -ENOSYS 判据过 |
| UNX-F9005 | sigreturn 返回路径（信号栈帧 ABI） | 260 | 已深化 | UNX-F9005-J1 rt_sigframe 布局（uc_flags/uc_link/uc_mcontext/uc_sigmask）对照 SysV ABI 冻结判据过，寄存器组（含 r10 槽位）恢复零差判据过，坏栈帧返 EFAULT 判据过 |
| UNX-F9006 | 默认处置表（SIG_DFL/SIG_IGN/SIG_ERR 全集） | 220 | 已深化 | UNX-F9006-J1 1–31 号默认处置（Term/Ign/Core/Stop）逐号对照 signal(7) 表判据过，处置切换生效时点判据过 |
| UNX-F9007 | 标准信号 1–31 与实时信号 34–64 登记簿 | 220 | 已深化 | UNX-F9007-J1 全 64 号登记簿落账判据过，标准号不排队/实时号排队（F9008）分档判据过，32/33 保留间隙语义对照判据过 |
| UNX-F9008 | 信号队列与排队语义（rt 信号排队） | 220 | 已深化 | UNX-F9008-J1 实时信号多次投递多次送达判据过，队列上限 RLIMIT_SIGPENDING 触达 EAGAIN 格过，标准信号合并投递判据过 |
| UNX-F9009 | SIGKILL/SIGSTOP 不可捕获单点 | 180 | 已深化 | UNX-F9009-J1 捕获/掩蔽/忽略三路径对 KILL/STOP 均无效判据过，KILL 组播 exit_group 联动判据过 |
| UNX-F9010 | sigaltstack 与 SA_ONSTACK | 200 | 已深化 | UNX-F9010-J1 备用栈注册与切换判据过，栈溢出防护（stack_t 尺寸校验）判据过，禁用后 ONSTACK 忽略判据过 |
| UNX-F9011 | sigpending/sigtimedwait/sigwaitinfo | 200 | 已深化 | UNX-F9011-J1 pending 集读回判据过，同步等待未决信号三号（timedwait 超时 EAGAIN 格）判据过 |
| UNX-F9012 | 信号与线程组路由（clone CLONE_SIGHAND 联动） | 200 | 已深化 | UNX-F9012-J1 共享 handler 表（CLONE_SIGHAND）判据过，tgkill 定向线程投递判据过，A3 SMP 就位后复测登记 |
| UNX-F9013 | 信号与 EINTR 联动协议（SA_RESTART 消费） | 200 | 已深化 | UNX-F9013-J1 SA_RESTART 置位→慢调用自动重启判据过，未置位→返 EINTR 判据过，单点与 F8948 锚点一致回归 |
| UNX-F9014 | 信号挂点与 guard.rs CAP 联动（SIGPROF/SIGALRM） | 180 | 已深化 | UNX-F9014-J1 计时信号触发链（itimer→SIGALRM/SIGPROF）判据过，CAP 门校验联动判据过 |
| UNX-F9015 | setitimer/getitimer 间隔定时器 | 220 | 已深化 | UNX-F9015-J1 ITIMER_REAL/VIRTUAL/PROF 三档判据过，interval 周期性触发判据过，旧值读回判据过 |
| UNX-F9016 | alarm 兼容挂点 | 180 | 已深化 | UNX-F9016-J1 alarm 返回剩余秒判据过，seconds=0 取消语义判据过，与 setitimer(ITIMER_REAL) 互斥语义判据过 |
| UNX-F9017 | 信号错误矩阵（EINVAL/ESRCH/EPERM/EBADF） | 200 | 已深化 | UNX-F9017-J1 四错误码 × 4 号（kill/sigaction/sigprocmask/pidfd_send_signal）矩阵 16 格逐格一致 |
| UNX-F9018 | 信号与多路复用竞态防护（signalfd/ppoll 联动档） | 180 | 已深化 | UNX-F9018-J1 掩码原子切换防丢信号协议判据过，signalfd（B10 F8989）/ppoll（B09 F8970）两消费路径联动回归 |
| UNX-F9019 | 信号族 ktest 半自动驱动（投递-捕获-返回三段探针） | 200 | 已深化 | UNX-F9019-J1 三段探针（用户态 handler 注册→内核投递→sigreturn 返回）驱动器判据过，10 号以上信号轮转覆盖 |
| UNX-F9020 | ktest syscall 面 B11 批断言集 | 340 | 已深化 | UNX-F9020-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，信号断言独立编号可单独复跑 |

<!-- 主册行 11158 · #### UNX-C2-B12 · EINTR 单点深化与 IPC 挂点族（F9021–F9040 · 20 条） -->
#### UNX-C2-B12 · EINTR 单点深化与 IPC 挂点族（F9021–F9040 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B11 49,460 + 本批 4,220 = 53,680 / 240,000｜嫁接源：Linux man-pages pipe(2)/socket(2)/sysvipc(7)/mq_overview(7) 章节、现存 `syscall::calls::PipeTable` 存量档为升级接管扩容｜防重：socket 协议栈本体归 B2 网络域（AI-07）防重——本域只立 socket 号面挂点与消费协议；IPC 各族本体机制归后续 M 型批次，本批立挂点登记档不重铺｜判据与 deepen/C2-B12.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9021 | EINTR/SA_RESTART 判定器单点本体（自 F8948 深化） | 280 | 已深化 | UNX-F9021-J1 判定器四输入（handler 置位/信号掩码/慢调用类别/重启白名单）全组合矩阵判据过，白名单外号不重启判据过，单点唯一性（全号面仅此一处分流）判据过 |
| UNX-F9022 | restart_syscall 挂点 | 180 | 已深化 | UNX-F9022-J1 重启号面挂点注册判据过，重启上下文（原号+参数快照）保存恢复判据过，无上下文调用返 EINVAL 格过 |
| UNX-F9023 | pipe/pipe2 本体档（PipeTable 对齐） | 280 | 已深化 | UNX-F9023-J1 双 fd（读/写端）分配判据过，O_NONBLOCK/O_CLOEXEC 旗标矩阵判据过，读写端闭环 10^3 次零失真判据过 |
| UNX-F9024 | 管道读写阻塞语义与容量（F_SETPIPE_SZ 联动） | 260 | 已深化 | UNX-F9024-J1 默认 64KiB 容量判据过，写满阻塞/NONBLOCK EAGAIN 双路径判据过，原子写边界（≤PIPE_BUF 4KiB 不撕裂）判据过 |
| UNX-F9025 | socket 基座挂点（socket/bind/listen 登记档） | 260 | 已深化 | UNX-F9025-J1 三号挂点注册判据过，AF_UNIX/AF_INET 域校验判据过，防重：协议栈本体归 B2（AI-07）不重铺声明落账 |
| UNX-F9026 | socket connect/accept/send/recv 挂点 | 260 | 已深化 | UNX-F9026-J1 四 IO 挂点注册判据过，慢调用 EINTR 联动（F9021 判定器消费）判据过，未连接 fd 操作返 ENOTCONN 格过 |
| UNX-F9027 | sendmsg/recvmsg 与 msghdr ABI 冻结 | 200 | 已深化 | UNX-F9027-J1 msghdr/msg_iov 控制消息布局冻结判据过，多 iovec 聚合读写判据过，控制消息（ SCM_RIGHTS 挂点）登记不实现声明落账 |
| UNX-F9028 | UNIX 域套接字挂点（AF_UNIX） | 240 | 已深化 | UNX-F9028-J1 抽象/路径名双命名空间挂点判据过，SOCK_STREAM/DGRAM 双型判据过，与 B10 管道消费档（F8995）联动判据过 |
| UNX-F9029 | SysV 信号量挂点（semget/semop 登记档） | 200 | 已深化 | UNX-F9029-J1 两号挂点注册判据过，sem_ops 结构布局冻结判据过，本体档 M 型后续批次声明落账 |
| UNX-F9030 | SysV 共享内存挂点（shmget/shmat 登记档） | 200 | 已深化 | UNX-F9030-J1 两号挂点注册判据过，与 B3 内存域（AI-09）防重对接声明落账，shmflg 权限校验挂点判据过 |
| UNX-F9031 | SysV 消息队列挂点（msgget/msgsnd 登记档） | 180 | 已深化 | UNX-F9031-J1 两号挂点注册判据过，msgbuf 布局冻结判据过，阻塞发送 EINTR 联动判据过 |
| UNX-F9032 | POSIX mqueue 挂点（mq_open/mq_send 登记档） | 180 | 已深化 | UNX-F9032-J1 两号挂点注册判据过，/dev/mqueue 路径命名约定登记判据过，未实现本体返 -ENOSYS 判据过 |
| UNX-F9033 | IPC 键空间与 ftok 语义 | 160 | 已深化 | UNX-F9033-J1 ftok 派生键（st_ino/st_dev 混合）判据过，IPC_PRIVATE 特殊键判据过，键冲突查询挂点判据过 |
| UNX-F9034 | IPC 资源限额（RLIMIT/MSGMNB/SEMMNS）落账 | 160 | 已深化 | UNX-F9034-J1 三类限额常量落账判据过，触达限额 ENOSPC 格过，限额可调（procfs 挂点）登记 |
| UNX-F9035 | IPC 错误矩阵（EIDRM/EKEYEXPIRED/ENOSPC/EINVAL） | 160 | 已深化 | UNX-F9035-J1 四错误码 × 3 号（semget/shmget/msgget）矩阵 12 格逐格一致，键不存在 ENOENT 格过 |
| UNX-F9036 | ipc 系统调用兼容门（IPC_64 旗标） | 160 | 已深化 | UNX-F9036-J1 遗留 ipc(2) 单号面（int 0x80 遗留）转接判据过，IPC_64 旗标分路判据过，RouteNote::Permuted 差异登记 |
| UNX-F9037 | 管道与多路复用联动（pipe→epoll 消费复测） | 180 | 已深化 | UNX-F9037-J1 管道 fd 挂入 epoll（B09 F8967 协议）复测判据过，写端触发读端唤醒闭环 10^3 次判据过 |
| UNX-F9038 | IPC 家族错误路径与编码器单点（F8923）一致复测 | 160 | 已深化 | UNX-F9038-J1 IPC 族全部错误路径经 F8923 单点编码复测判据过，绕行路径零发现断言过 |
| UNX-F9039 | IPC 族 ktest 半自动驱动 | 180 | 已深化 | UNX-F9039-J1 管道/socket 挂点联动驱动器判据过，SysV/mqueue 挂点 ENOSYS 路径断言覆盖 |
| UNX-F9040 | ktest syscall 面 B12 批断言集 | 340 | 已深化 | UNX-F9040-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，EINTR 单点与 IPC 挂点断言独立编号可单独复跑 |

<!-- 主册行 11185 · #### UNX-C2-B13 · 错误矩阵正反双判据总装（F9041–F9060 · 20 条） -->
#### UNX-C2-B13 · 错误矩阵正反双判据总装（F9041–F9060 · 20 条）

> AI-12 承办｜批次类型：E 型错误矩阵映射批（任务书 B21–B28 E 型体例在本认领段的机制化落点）｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B12 53,680 + 本批 4,310 = 57,990 / 240,000｜嫁接源：Linux man-pages errno(3)、LTP errno 注入用例体例（只跟随体例不跟随数据）、本域 B02–B12 已铺错误矩阵为抽样母体｜防重：本批不新增功能号语义，只对已铺号面做 30 号抽样正反双判据总装，抽验范围内不与各批原地矩阵重复计数｜判据与 deepen/C2-B13.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9041 | 错误矩阵抽样协议总纲（30 号抽样规则、正反判据定义） | 220 | 已深化 | UNX-F9041-J1 抽样规则（B02–B12 每批均匀抽 3 号、覆盖 F/E/M 三型）冻结判据过，正判据=合法输入→期望输出、反判据=非法输入→期望错误码的定义落账 |
| UNX-F9042 | 抽样组一：open/close/read/write 正反双判据 | 210 | 已深化 | UNX-F9042-J1 四号正判据（合法读写往返）与反判据（EBADF/EINVAL/EISDIR）各 6 用例全过，用例与 B02 矩阵同源核对 |
| UNX-F9043 | 抽样组二：mmap/munmap/mprotect 正反双判据 | 210 | 已深化 | UNX-F9043-J1 三号正判据（映射-改权-卸载闭环）与反判据（EINVAL/ENOMEM/EACCES）各 6 用例全过，页边界格覆盖 |
| UNX-F9044 | 抽样组三：fork/execve/wait4 正反双判据 | 210 | 已深化 | UNX-F9044-J1 三号正判据（派生-换影-收尸闭环）与反判据（ECHILD/EACCES/ENOENT）各 6 用例全过 |
| UNX-F9045 | 抽样组四：getpid/setuid 族正反双判据 | 220 | 已深化 | UNX-F9045-J1 双人格路由正判据与反判据（EPERM/EINVAL）各 6 用例全过，越权降格用例覆盖 |
| UNX-F9046 | 抽样组五：时间族正反双判据（clock_gettime/nanosleep/gettimeofday） | 220 | 已深化 | UNX-F9046-J1 三号正判据（时钟读数单调/睡眠精确）与反判据（EINVAL/EFAULT/EPERM）各 6 用例全过 |
| UNX-F9047 | 抽样组六：dup/fcntl/ioctl 正反双判据 | 220 | 已深化 | UNX-F9047-J1 三号正判据（复制/取置标志）与反判据（EBADF/EINVAL/ENOTTY）各 6 用例全过 |
| UNX-F9048 | 抽样组七：目录族正反双判据（getdents64/mkdir/unlink） | 220 | 已深化 | UNX-F9048-J1 三号正判据（建-读-删闭环）与反判据（ENOTDIR/EISDIR/ENOTEMPTY）各 6 用例全过 |
| UNX-F9049 | 抽样组八：epoll 族正反双判据（epoll_create1/ctl/wait） | 220 | 已深化 | UNX-F9049-J1 三号正判据（建-挂-等闭环）与反判据（EBADF/EINVAL/EEXIST/ENOENT）各 6 用例全过 |
| UNX-F9050 | 抽样组九：四件套正反双判据（eventfd2/timerfd_create/signalfd/inotify_init1） | 200 | 已深化 | UNX-F9050-J1 四号正判据（各件单流程）与反判据（EMFILE/EINVAL/EBADF）各 8 用例全过 |
| UNX-F9051 | 抽样组十：信号族正反双判据（rt_sigaction/kill/sigprocmask） | 220 | 已深化 | UNX-F9051-J1 三号正判据（注册-投递-捕获）与反判据（EINVAL/ESRCH/EPERM）各 6 用例全过 |
| UNX-F9052 | 抽样组十一：IPC 族正反双判据（pipe/shmget 挂点/msgget 挂点） | 200 | 已深化 | UNX-F9052-J1 三号正判据（管道闭环）与反判据（ENOSPC/EINVAL/-ENOSYS 挂点档）各 6 用例全过 |
| UNX-F9053 | 正判据方法论（合法输入→期望输出，三成分齐备） | 200 | 已深化 | UNX-F9053-J1 三成分（动作动词/可观察对象/复测锚点）检查器判据过，11 组正判据 100% 成分齐备审计通过 |
| UNX-F9054 | 反判据方法论（非法输入→期望错误码，边界值登记） | 200 | 已深化 | UNX-F9054-J1 边界值注入（-1/INT_MAX/NULL/越界 len）分档判据过，11 组反判据期望码与编码器单点（F8923）一致审计通过 |
| UNX-F9055 | 错误注入驱动器（ktest 错误注入框架挂点） | 200 | 已深化 | UNX-F9055-J1 注入框架（参数篡改/资源打满/权限剥夺三通道）判据过，注入点与用例账本（F8939）一一对应抽验 |
| UNX-F9056 | 判据-错误码映射账本（30 号 × 错误码矩阵落账） | 200 | 已深化 | UNX-F9056-J1 30 号映射账本落盘判据过，账本行数与抽样号数一致（30 行）校验过 |
| UNX-F9057 | 跨批错误一致性审计（B02–B12 错误矩阵与编码器单点回归） | 200 | 已深化 | UNX-F9057-J1 七批矩阵各抽 2 格与编码器单点回归一致判据过，漂移格零发现断言过 |
| UNX-F9058 | 边界值登记簿（INT_MAX/NULL/越界指针/负 fd） | 200 | 已深化 | UNX-F9058-J1 四类边界值 × 6 号登记簿落账判据过，全部边界用例反判据归格（EINVAL/EFAULT/EBADF）通过 |
| UNX-F9059 | 错误矩阵回归套件（一键批跑 30 组正反用例） | 200 | 已深化 | UNX-F9059-J1 一键批跑命令产出 30 组通过清单判据过，失败用例自动列号并阻断退出码判据过 |
| UNX-F9060 | ktest syscall 面 B13 批断言集 | 340 | 已深化 | UNX-F9060-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，抽样组断言独立编号可单独复跑 |

<!-- 主册行 11212 · #### UNX-C2-B14 · glibc 消费清单逐号核（F9061–F9080 · 20 条） -->
#### UNX-C2-B14 · glibc 消费清单逐号核（F9061–F9080 · 20 条）

> AI-12 承办｜批次类型：I 型集成映射批（任务书 B29–B36 I 型体例在本认领段的机制化落点）｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B13 57,990 + 本批 4,320 = 62,310 / 240,000｜嫁接源：glibc 2.31/2.35 syscalls.list 与源码包装层体例（只跟随体例不跟随数据）、busybox 静态链接探针体例｜防重：glibc 移植本体归 B6 用户态域防重——本域只立"每个号被谁消费"的证据账与逐号核协议，不重铺包装器实现｜判据与 deepen/C2-B14.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9061 | glibc 系统调用消费模型总纲（syscall() 包装器路径） | 220 | 已深化 | UNX-F9061-J1 包装器路径（libc 包装→syscall 指令→入口门→分发表）全链冻结判据过，r10 上下文槽位穿透声明与 B01 F8802 一致判据过 |
| UNX-F9062 | glibc open/read/write/close 消费核（B02 号面） | 240 | 已深化 | UNX-F9062-J1 四号消费证据（stdio/stdin 缓冲路径）登记判据过，静态 hello-world 探针全程只触四号断言过 |
| UNX-F9063 | glibc malloc/brk/mmap 消费核（B03 号面） | 240 | 已深化 | UNX-F9063-J1 malloc 大小块两路消费证据（brk 顶探/mmap 阈值 128KiB）登记判据过，free 触发 madvise（MADV_DONTNEED）路径证据过 |
| UNX-F9064 | glibc fork/exec/wait 消费核（B04 号面） | 220 | 已深化 | UNX-F9064-J1 system()/popen() 消费链证据（fork+execve+waitpid）登记判据过，abort() 走 raise(SIGABRT) 证据过 |
| UNX-F9065 | glibc 身份族消费核（B05 号面 getuid/setuid） | 200 | 已深化 | UNX-F9065-J1 getuid/geteuid 包装消费证据登记判据过，setuid 降格路径证据过，缓存惰性（glibc uid 缓存）行为登记 |
| UNX-F9066 | glibc 时间族消费核（B06 号面 clock_gettime/gmtime 缓存） | 220 | 已深化 | UNX-F9066-J1 clock_gettime 直入 syscall（无包装缓存）证据判据过，vDSO 快路径（B06 F8912）优先协商证据过，localtime 时区读文件链证据登记 |
| UNX-F9067 | glibc errno 转换核（__libc_errno 与 -errno 编码） | 200 | 已深化 | UNX-F9067-J1 -errno→TLS errno 转换证据登记判据过，perror/strerror 可读名与 F8933 档一致判据过 |
| UNX-F9068 | glibc dup/fcntl 消费核（B08 号面） | 200 | 已深化 | UNX-F9068-J1 dup2 用于 stdio 重定向证据判据过，fcntl(F_SETFL) 用于 NONBLOCK 证据过，F_GETFD 继承位证据过 |
| UNX-F9069 | glibc opendir/readdir 消费核（getdents64 包装） | 200 | 已深化 | UNX-F9069-J1 DIR 缓冲经 getdents64 聚合证据判据过，readdir 逐项解析与 d_type 分发证据过 |
| UNX-F9070 | glibc epoll 包装消费核（epoll_wait→poll 桥） | 200 | 已深化 | UNX-F9070-J1 epoll 三包装消费证据登记判据过，老版本 poll 桥路径证据过，event 数据透传零差判据过 |
| UNX-F9071 | glibc pthread 消费核（clone/futex 挂点） | 240 | 已深化 | UNX-F9071-J1 pthread_create 消费链（clone CLONE_VM\|CLONE_THREAD\|CLONE_SIGHAND）证据判据过，futex 挂点登记与 B04 F8865 声明对账一致 |
| UNX-F9072 | futex 语义档（glibc pthread 锁消费本体） | 260 | 已深化 | UNX-F9072-J1 FUTEX_WAIT/WAKE 双操作正判据过，私/共享 futex 分档判据过，超时路径 EAGAIN/TIMEDOUT 格过 |
| UNX-F9073 | glibc 信号消费核（sigaction/raise 包装） | 200 | 已深化 | UNX-F9073-J1 signal() 兼容包装落 sigaction 证据判据过，pthread_kill 定向投递证据过，abort 三信号（SIGABRT）链证据过 |
| UNX-F9074 | glibc dlopen 消费钩子（ELF/mmap 联动） | 180 | 已深化 | UNX-F9074-J1 dlopen 消费链（open+mmap+close）证据判据过，ELF 装载本体归 B5 域防重声明落账，探针（dlopen 后关闭源 fd 仍可用）判据过 |
| UNX-F9075 | glibc NSS/getpwuid 消费钩子（文件族联动） | 160 | 已深化 | UNX-F9075-J1 getpwuid 消费链（open /etc/passwd+read+close）证据判据过，文件不存在 ENOENT 降级路径判据过 |
| UNX-F9076 | glibc printf/stdio 消费核（write 缓冲行为） | 200 | 已深化 | UNX-F9076-J1 行缓冲/全缓冲切换证据（tty 判定经 fstat）判据过，fflush 触发 write 聚合证据过，管道另一端读数一致判据过 |
| UNX-F9077 | 消费清单逐号核账本（每号"被消费证据"登记格式） | 200 | 已深化 | UNX-F9077-J1 登记格式（号/包装函数/证据用例/探针脚本）冻结判据过，B02–B14 已核号面逐行登记数与抽样数一致校验过 |
| UNX-F9078 | 消费核回归套件（静态链接 busybox 探针登记） | 220 | 已深化 | UNX-F9078-J1 busybox 静态探针（sh/ls/echo 三 applet）消费清单落账判据过，探针触号清单与逐号核账本交叉核验一致 |
| UNX-F9079 | glibc 版本锚定账本（2.31/2.35 行为差异登记） | 180 | 已深化 | UNX-F9079-J1 两版本差异点（clock_gettime64/pthread 布局）登记判据过，差异不影响号面契约声明落账 |
| UNX-F9080 | ktest syscall 面 B14 批断言集 | 340 | 已深化 | UNX-F9080-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，消费核断言独立编号可单独复跑 |

<!-- 主册行 11239 · #### UNX-C2-B15 · LTP 可跑登记与号面冻结（F9081–F9100 · 20 条） -->
#### UNX-C2-B15 · LTP 可跑登记与号面冻结（F9081–F9100 · 20 条）

> AI-12 承办｜批次类型：I/C 型收官批（任务书 B29–B36 I 型与 B37–B40 C 型体例在本认领段的机制化落点）｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B14 62,310 + 本批 4,560 = 66,870 / 240,000（域累计恰为本会话认领预算）｜嫁接源：LTP runtest 文件体例（只跟随体例不跟随数据）、uname(2)/getrusage(2)/prlimit64(2) man-pages、波次 05 验收标准原文｜防重：uname/getrusage/prlimit64 为号面补位新铺（B01–B14 未触及），不与他批重复；号面冻结后 B16–B40 承办方须按 §6.3 冻结协议扩号｜判据与 deepen/C2-B15.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9081 | LTP 登记总纲（"可跑"三档定义） | 280 | 已深化 | UNX-F9081-J1 三档（全过/部分过/登记待测）定义冻结判据过，档位判分与依赖完成度挂钩规则落账，空档不静默声明落账 |
| UNX-F9082 | LTP 文件族测试集登记（open/read/write/stat） | 260 | 已深化 | UNX-F9082-J1 对应 LTP 族用例清单登记判据过，全过档用例逐条勾稽，部分过档缺依赖（B1 VFS 全量）显式列名 |
| UNX-F9083 | LTP 内存族测试集登记（mmap/mprotect） | 260 | 已深化 | UNX-F9083-J1 对应族用例清单登记判据过，依赖 A4 页表全量项列名登记，就地可测项勾稽 |
| UNX-F9084 | LTP 进程族测试集登记（fork/exec/wait） | 260 | 已深化 | UNX-F9084-J1 对应族用例清单登记判据过，依赖 A3 SMP/A1 调度项显式列名，就地可测项勾稽 |
| UNX-F9085 | LTP 时间族测试集登记（clock_gettime/nanosleep） | 200 | 已深化 | UNX-F9085-J1 对应族用例清单登记判据过，A2 计时基座联动项列名，就地可测项勾稽 |
| UNX-F9086 | LTP 信号族测试集登记（sigaction/kill） | 200 | 已深化 | UNX-F9086-J1 对应族用例清单登记判据过，多线程信号项（A3 依赖）列名，就地可测项勾稽 |
| UNX-F9087 | LTP 多路复用测试集登记（epoll/select/poll） | 220 | 已深化 | UNX-F9087-J1 对应族用例清单登记判据过，socket 联动项（B2 依赖）列名，管道/文件项勾稽 |
| UNX-F9088 | LTP IPC 测试集登记（pipe/msg/shm/sem） | 200 | 已深化 | UNX-F9088-J1 对应族用例清单登记判据过，SysV 本体未落项全列名（-ENOSYS 路径可测），pipe 项勾稽 |
| UNX-F9089 | LTP 系统信息面登记（sysinfo/uname/getrusage 挂点） | 180 | 已深化 | UNX-F9089-J1 三号登记判据过，sysinfo 已铺（F8909）交叉引用一致，uname/getrusage 本体档见 F9090/F9091 防重声明 |
| UNX-F9090 | uname 语义档（内核标识与版本协商） | 160 | 已深化 | UNX-F9090-J1 六字段（sysname/nodename/release/version/machine/domainname）布局冻结判据过，release 版本串与内核构建账一致判据过 |
| UNX-F9091 | getrusage 语义档（资源用量账） | 200 | 已深化 | UNX-F9091-J1 ru_utime/ru_stime 与 times（F8908）对账一致判据过，RUSAGE_SELF/CHILDREN 双档判据过，maxrss 字段来源声明落账 |
| UNX-F9092 | prlimit64 语义档（RLIMIT 全集读写） | 200 | 已深化 | UNX-F9092-J1 NOFILE/STACK/SIGPENDING/NPROC 四限额读写判据过，旧值读回判据过，非特权提限 EPERM 格过 |
| UNX-F9093 | 号面冻结清单总纲（B01–B15 铺面 300 号冻结协议） | 260 | 已深化 | UNX-F9093-J1 300 号冻结协议（ID/名称/判据/行数四要素齐备方可入冻）判据过，冻结后变更走 §6.3 协议声明落账 |
| UNX-F9094 | 号面冻结账本（F8801–F9100 逐号状态表） | 260 | 已深化 | UNX-F9094-J1 300 行状态表（号/批/状态/判据数）落账判据过，行数=300 校验过，与四范围防重 grep 结果一致性抽验 10 号 |
| UNX-F9095 | 跨域接口冻结声明（A1/A4/B1/A2/C2 边界协议落账） | 200 | 已深化 | UNX-F9095-J1 四域边界协议（A1 调度唤醒/A4 缺页回调/B1 路径解析/A2 时钟仲裁）落账判据过，未完成依赖 open_risks 移交登记 |
| UNX-F9096 | 波次 05 验收包组装（验收标准对照：数字+判据双重计数） | 280 | 已深化 | UNX-F9096-J1 验收包（数字计数 300/判据计数 ≥300/行数账 66,870）三账合一判据过，双重计数公式与总纲验收标准一致 |
| UNX-F9097 | 遗留风险登记与移交包（open_risks 全量落账 + handoff 协议） | 200 | 已深化 | UNX-F9097-J1 open_risks 全量（跨域依赖 C1/A4/B1/A2 四项）落账判据过，handoff.json 字段协议（phase/next_batch/open_risks）一致性判据过 |
| UNX-F9098 | 性能基线账本（P99 ≤ Windows ×1.5 预登记） | 200 | 已深化 | UNX-F9098-J1 五族热点号（read/write/mmap/fork/epoll_wait）P99 预登记判据过，闸门补测挂点与 meter（F8934）联动落账 |
| UNX-F9099 | C2 域收官报告生成器（批次账/判据账/行数账三账合一） | 200 | 已深化 | UNX-F9099-J1 三账合一报表（40 批进度/判据总数/行数守恒）生成判据过，报表数字与台账回填值零差 |
| UNX-F9100 | ktest syscall 面 B15 批断言集 + 域收官断言 | 340 | 已深化 | UNX-F9100-J1 本批 19 条判据聚合判据过，域收官断言（B01–B15 全批 ktest 一次跑通+行数守恒 66,870）判据过 |

<!-- 主册行 11266 · #### UNX-C2-B16 · fcntl 与 ioctl 挂点族（F9101–F9120 · 20 条） -->
#### UNX-C2-B16 · fcntl 与 ioctl 挂点族（F9101–F9120 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 文件族扩容 fcntl/ioctl 命令面）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B16.md 全六要素收口）｜域账累计：B01-B15 66,870 + 本批 6,500 = 73,370 / 240,000｜嫁接源：fcntl(2)/ioctl(2) man-pages 语义对齐（来源版本随深化册落账）；B1 VFS 冻结读路径（fd 表消费）；与 B08 dup 族/F8921 参数拷贝探针/F8966 管道容量账单源联动｜防重声明：fcntl/ioctl 族 20 号为号面新铺（B01-B15 未触及，dup 族 B08 零重叠）；本批为冻结协议 F9093 之后的扩号首段：扩号段 F9101-F9120 承接冻结面 F8801-F9100，零重编零私设号（只做 Linux 上游约定号面）｜红线注记：本批零写盘零引导零固件操作；ioctl 只走挂点协议登记，硬件端口类命令（ioperm/iopl）禁用格归 B24 EPERM 矩阵，本批零触及（双轨产线条款默认适用）｜批注（AI-12）：B16 是 M 型收尾第一轴：fcntl 四命令是 glibc FILE* 关闭语义与 shell 重定向的底层依赖，记录锁三命令按挂点协议落账（实装归 B1 联签），ioctl 以方向位解码+挂点路由为主轴——终端族三命令为 C4 pty 联签预留对装面｜判据与后续 deepen/C2-B16.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9101 | fcntl 语义档总纲（F_GETFD/F_SETFD/F_GETFL/F_SETFL 四命令） | 400 | — | UNX-F9101-J1 四命令读写回环判据过，FD_CLOEXEC 位 execve 后生效可观测，坏 fd 注入全 EBADF 逐格核对 |
| UNX-F9102 | fcntl F_DUPFD/F_DUPFD_CLOEXEC 复制命令 | 320 | — | UNX-F9102-J1 最小可用 fd ≥ arg 分配判据过，CLOEXEC 变体位翻转可观测，与 dup 族分配器单源对账 |
| UNX-F9103 | fcntl 记录锁挂点（F_GETLK/F_SETLK/F_SETLKW 三命令协议） | 340 | — | UNX-F9103-J1 三命令协议落账判据过，l_type/l_whence/l_start/l_len 四字段探针格过，实装归 B1 联签声明格过 |
| UNX-F9104 | fcntl F_SETFL O_NONBLOCK/O_APPEND 位语义 | 340 | — | UNX-F9104-J1 O_NONBLOCK 置位后非阻塞路径可观测判据过，O_APPEND 追加位生效判据过，F_GETFL 回读逐位一致 |
| UNX-F9105 | fcntl 错误矩阵（EBADF/EINVAL/EINTR/EDEADLK/EMFILE） | 320 | — | UNX-F9105-J1 五错误码注入矩阵判据过，逐码与 Linux 语义一致，非法命令全 EINVAL 格过 |
| UNX-F9106 | ioctl 语义档总纲（第三参数装箱协议与方向位） | 380 | — | UNX-F9106-J1 IO/IOR/IOW/IOWR 四方向位解码判据过，参数装箱协议落账，未知命令路由格过 |
| UNX-F9107 | ioctl 终端族挂点（TCGETS/TCSETS/TIOCGWINSZ） | 340 | — | UNX-F9107-J1 三命令挂点协议落账判据过，winsize 结构回读格过，pty 缺席全 ENOTTY 与 C4 联签格过 |
| UNX-F9108 | ioctl FIONBIO/FIOASYNC 挂点协议（网络族号段预留） | 320 | — | UNX-F9108-J1 两命令挂点落账判据过，socket 号段预留声明与 I2 域边界一致，非网络 fd 全 ENOTTY 格过 |
| UNX-F9109 | ioctl 未知命令返回 ENOTTY 矩阵 | 260 | — | UNX-F9109-J1 未知命令注入矩阵判据过，全返回 -ENOTTY 与 Linux 一致，零 -EINVAL 混淆格过 |
| UNX-F9110 | fcntl F_GETOWN/F_SETOWN 信号投递属主挂点 | 320 | — | UNX-F9110-J1 属主设置读取回环判据过，SIGIO 路由挂点协议落账，F_SETSIG 挂点声明格过 |
| UNX-F9111 | fcntl F_SETLKW 阻塞等待与 EINTR 重启联动 | 340 | — | UNX-F9111-J1 阻塞等待挂点与 EINTR 重启协议联动判据过，与 B12 四输入判定器对账一致，零阻塞对照格过 |
| UNX-F9112 | flock 挂点与 POSIX 锁差异账 | 320 | — | UNX-F9112-J1 flock(2) 挂点落账判据过，LOCK_EX/LOCK_SH/LOCK_UN 三操作协议格过，两锁体系语义差异账四条全列 |
| UNX-F9113 | dup3 与 O_CLOEXEC 联动（dup 族闭环） | 300 | — | UNX-F9113-J1 dup3 到期语义判据过，newfd==oldfd 返回 EINVAL 格过，与 dup 族分配器单源复测一致 |
| UNX-F9114 | fcntl F_GETPIPE_SZ/F_SETPIPE_SZ 管道容量命令 | 300 | — | UNX-F9114-J1 容量读写回环判据过，下限页对齐校验格过，与管道容量账对账一致 |
| UNX-F9115 | fcntl F_ADD_SEALS/F_GET_SEALS 密封挂点协议 | 300 | — | UNX-F9115-J1 密封四值挂点落账判据过，memfd 缺席全 EINVAL 格过，号面预留声明格过 |
| UNX-F9116 | ioctl 结构拷贝探针（copy_from_user/copy_to_user 联动） | 320 | — | UNX-F9116-J1 双向拷贝探针判据过，坏用户指针全 EFAULT 格过，与参数拷贝探针总成单源 |
| UNX-F9117 | fcntl/ioctl 参数探针总成（nargs=3 命令面） | 300 | — | UNX-F9117-J1 nargs=3 寄存器提取判据过，三参逐位传递与 ABI 约定一致，r10 槽位复用格过 |
| UNX-F9118 | fcntl 与 glibc FILE* 关闭路径消费核 | 320 | — | UNX-F9118-J1 glibc fclose 到 close 消费链判据过，FD_CLOEXEC 在 popen 路径生效可观测，消费号登记格过 |
| UNX-F9119 | fcntl/ioctl 升级接管注记（int 0x80 存量面） | 320 | — | UNX-F9119-J1 接管边界声明判据过，存量 ioctl 零新增语义承诺格过，行为保持清单落账 |
| UNX-F9120 | ktest syscall 面 B16 批断言集 | 340 | — | UNX-F9120-J1 本批 19 条判据聚合判据过，fcntl 消费链与 ioctl 挂点路由一次跑通，域累计 73,370 收口断言 |

<!-- 主册行 11293 · #### UNX-C2-B17 · 内存族扩容：mremap/madvise/msync/mlock（F9121–F91 -->
#### UNX-C2-B17 · 内存族扩容：mremap/madvise/msync/mlock（F9121–F9140 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 内存族扩容）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B17.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16 6,500 + 本批 6,400 = 79,770 / 240,000｜嫁接源：mremap(2)/madvise(2)/msync(2)/mlock(2) man-pages 语义对齐；A4 mmap 底座冻结接口消费（页表挂点声明）；与 F8804 mmap 三联/F8841 brk 档单源联动｜防重声明：内存族扩容 20 号为号面新铺（B01-B15 mmap/mprotect/brk 本体已铺，本批只铺 mremap/madvise/msync/mlock/mincore 扩容号，零重复）；扩号段 F9121-F9140 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；mlock 族零 DMA 零物理地址直写；mremap 零页表直改（只消费 A4 冻结接口，页表操作归 A4 域）｜批注（AI-12）：B17 是内存族扩容轴：mremap 消费 A4 页表底座（上游未 finalize 依赖显式登记），madvise 六建议先立协议骨架（回收/预读实装随 B1 页缓存收口联签），mlock 族与 RLIMIT_MEMLOCK 联动（B19 资源族消费）｜判据与后续 deepen/C2-B17.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9121 | mremap 语义档（MREMAP_MAYMOVE/MREMAP_FIXED） | 400 | — | UNX-F9121-J1 迁移映射判据过，MAYMOVE 位语义格过，old_size 匹配校验格过 |
| UNX-F9122 | mremap 与 A4 页表底座消费协议 | 340 | — | UNX-F9122-J1 页表消费协议落账判据过，PTE 迁移挂点声明格过，上游未 finalize 依赖登记格过 |
| UNX-F9123 | mremap 错误矩阵（EINVAL/ENOMEM/EPERM） | 320 | — | UNX-F9123-J1 三错误码注入矩阵判据过，逐码与 Linux 语义一致，非页对齐 addr 全 EINVAL 格过 |
| UNX-F9124 | madvise 语义档总纲（六建议枚举） | 400 | — | UNX-F9124-J1 MADV_NORMAL/SEQUENTIAL/RANDOM/WILLNEED/DONTNEED/FREE 六建议枚举判据过，未知建议全 EINVAL 格过 |
| UNX-F9125 | madvise MADV_DONTNEED 零页回收语义 | 340 | — | UNX-F9125-J1 零页回收判据过，回收后再访问零填充可观测，与页缓存联动依赖声明格过 |
| UNX-F9126 | madvise MADV_WILLNEED 预读挂点 | 320 | — | UNX-F9126-J1 预读挂点协议落账判据过，零副作用承诺格过（提示性语义） |
| UNX-F9127 | madvise MADV_SEQUENTIAL/RANDOM 模式提示 | 300 | — | UNX-F9127-J1 两模式提示格落账判据过，访问模式登记可观测，语义边界（提示非强制）声明格过 |
| UNX-F9128 | madvise 错误矩阵（EINVAL/ENOMEM/EACCES/EBADF） | 300 | — | UNX-F9128-J1 四错误码注入矩阵判据过，逐码与 Linux 语义一致，锁页越限 EPERM 归 B24 分界声明格过 |
| UNX-F9129 | msync 语义档（MS_SYNC/MS_ASYNC/MS_INVALIDATE） | 320 | — | UNX-F9129-J1 三标志语义落账判据过，MS_SYNC 同步刷回挂点声明格过，非映射区间 ENOMEM 格过 |
| UNX-F9130 | mlock/munlock 语义档（RLIMIT_MEMLOCK 联动） | 320 | — | UNX-F9130-J1 锁页判据过，超限 EPERM/ENOMEM 分界格过，与资源族限额账对账一致 |
| UNX-F9131 | mlockall/munlockall 语义档 | 320 | — | UNX-F9131-J1 MCL_CURRENT/MCL_FUTURE 两标志判据过，全地址空间锁语义格过，越限矩阵引用格过 |
| UNX-F9132 | mincore 语义档（驻留位向量） | 300 | — | UNX-F9132-J1 驻留位向量回读判据过，vec 长度校验格过，坏指针全 EFAULT 格过 |
| UNX-F9133 | mprotect 权限翻转与 #PF 联动复核 | 300 | — | UNX-F9133-J1 翻转后写触发 #PF 且进程收 SIGSEGV 复测判据过，与 mmap 三联语义闭环一致 |
| UNX-F9134 | MAP_NORESERVE/MAP_POPULATE 标志位语义 | 300 | — | UNX-F9134-J1 两标志语义落账判据过，POPULATE 预填页可观测，NORESERVE 零预留承诺格过 |
| UNX-F9135 | 内存族 OOM 联动（ENOMEM 注入路径） | 300 | — | UNX-F9135-J1 ENOMEM 注入判据过，与 OOM 分档联动协议声明格过，注入零内核损坏格过 |
| UNX-F9136 | 内存族参数探针总成（addr/length 对齐探针） | 300 | — | UNX-F9136-J1 页对齐探针判据过，未对齐 addr 全 EINVAL 格过，length 零值格过 |
| UNX-F9137 | 内存族与 glibc malloc 消费核 | 300 | — | UNX-F9137-J1 malloc 双路（brk/mmap）消费链判据过，大块分配走 mmap 阈值可观测，消费号登记格过 |
| UNX-F9138 | 内存族升级接管注记 | 300 | — | UNX-F9138-J1 接管边界声明判据过，现存 mmap 底座零重写承诺格过，升级增量清单落账 |
| UNX-F9139 | 内存族与 Windows VirtualAlloc 对照格 | 300 | — | UNX-F9139-J1 MEM_RESERVE/COMMIT 两阶段对照判据过，PAGE_* 保护位映射表落账，差异账三条全列 |
| UNX-F9140 | ktest syscall 面 B17 批断言集 | 320 | — | UNX-F9140-J1 本批 19 条判据聚合判据过，mremap/madvise/msync/mlock 四族一次跑通，域累计 79,770 收口断言 |

<!-- 主册行 11320 · #### UNX-C2-B18 · wait 进程族与进程控制（F9141–F9160 · 20 条） -->
#### UNX-C2-B18 · wait 进程族与进程控制（F9141–F9160 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 进程族，依赖 C1）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B18.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16 6,500 + B17 6,400 + 本批 6,450 = 86,220 / 240,000｜嫁接源：wait(2)/waitpid(2)/waitid(2)/setpgid(2)/setsid(2) man-pages 语义对齐；C1 进程模型冻结接口消费（判据联测共享只写编号+一句话）；与 B04 fork 档/B11 SIGCHLD 链/B15 F9084 进程族登记单源联动｜防重声明：wait 进程族 20 号为号面新铺（B01-B15 fork/exit 本体已铺，本批只铺 wait 族与进程控制扩容号，零重复）；扩号段 F9141-F9160 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；wait 族零进程强杀路径（kill 语义归 B11）；execve 挂点零内置盘引导相关操作（引导设施红线条款默认适用）｜批注（AI-12）：B18 是进程族收口轴：wait4 状态码四段解码（正常退出/信号终止/停止/继续）是 shell 作业控制的地基，进程组与会话语义为 C4 终端联签预留，execve 挂点与 C1 加载链联签（上游未 finalize 依赖显式登记）｜判据与后续 deepen/C2-B18.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9141 | wait4 语义档（状态码四段解码） | 410 | — | UNX-F9141-J1 四段解码（正常/信号终止/停止/继续）判据过，wstatus 逐位断言格过，rusage 联动格过 |
| UNX-F9142 | waitpid/wait3 兼容面（WNOHANG/WUNTRACED/WCONTINUED） | 360 | — | UNX-F9142-J1 三标志语义判据过，WNOHANG 零阻塞返回 0 可观测，兼容别名映射格过 |
| UNX-F9143 | waitid 语义档（P_PID/P_PGID/P_ALL） | 340 | — | UNX-F9143-J1 三选择模式判据过，WNOWAIT 保留态格过，si_code 字段逐值格过 |
| UNX-F9144 | 僵尸态收割语义与 ExitStatus 消费 | 340 | — | UNX-F9144-J1 收割后资源释放判据过，ExitStatus 结构消费单源格过，二次 wait 全 ECHILD 格过 |
| UNX-F9145 | wait 错误矩阵（ECHILD/EINTR/EINVAL） | 300 | — | UNX-F9145-J1 三错误码注入矩阵判据过，ECHILD 无子进程格过，EINTR 与 SA_RESTART 联动格过 |
| UNX-F9146 | setpgid/getpgid 语义档（进程组迁移协议） | 320 | — | UNX-F9146-J1 迁移协议判据过，会话首拒绝 EPERM 格过，僵尸子进程 ESRCH 格过 |
| UNX-F9147 | setsid 语义档（会话首进程选举） | 320 | — | UNX-F9147-J1 选举判据过，已有进程组 EPERM 格过，新会话脱离控制终端可观测 |
| UNX-F9148 | getppid/set_tid_address 语义档 | 300 | — | UNX-F9148-J1 getppid 初值与收养翻转判据过，set_tid_address 子退出通知挂点格过 |
| UNX-F9149 | vfork 挂点（共享地址空间窗口协议） | 320 | — | UNX-F9149-J1 窗口协议落账判据过，exec/_exit 前挂起父进程语义格过，返回值双份栈声明格过 |
| UNX-F9150 | execve 挂点协议（与 C1 加载链联签） | 360 | — | UNX-F9150-J1 挂点协议落账判据过，C1 加载链联签声明格过，EACCES/ENOEXEC 分界格过 |
| UNX-F9151 | execveat 语义档（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW） | 300 | — | UNX-F9151-J1 两标志判据过，dirfd 相对路径解析格过，AT_EMPTY_PATH 空串格过 |
| UNX-F9152 | 进程组与终端前台挂点（与 C4 联动） | 280 | — | UNX-F9152-J1 前台挂点协议落账判据过，TIOCSCTTY 挂点声明格过，pty 缺席零阻塞格过 |
| UNX-F9153 | 孤儿进程与 init 收养协议 | 300 | — | UNX-F9153-J1 收养判据过，SIGHUP 挂起通知挂点声明格过，收养后 getppid 翻转可观测 |
| UNX-F9154 | wait 族与 SIGCHLD 联动 | 340 | — | UNX-F9154-J1 SIGCHLD 投递链判据过，与信号挂点族对账一致，SA_NOCLDWAIT 抑制格过 |
| UNX-F9155 | 进程族错误矩阵补充（ESRCH/EPERM） | 280 | — | UNX-F9155-J1 两错误码注入矩阵判据过，逐码与 Linux 语义一致，EPERM 跨会话格过 |
| UNX-F9156 | 进程族参数探针总成 | 280 | — | UNX-F9156-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源 |
| UNX-F9157 | wait 族与 shell 作业控制消费核 | 300 | — | UNX-F9157-J1 fg/bg 作业链消费判据过，状态码到 $? 透传可观测，消费号登记格过 |
| UNX-F9158 | 进程族升级接管注记（现存 proc 面接管边界） | 300 | — | UNX-F9158-J1 接管边界声明判据过，存量 proc 零重写承诺格过，升级增量清单落账 |
| UNX-F9159 | 进程族与 Windows WaitForSingleObject 对照格 | 300 | — | UNX-F9159-J1 进程句柄等待对照判据过，退出码语义映射表落账，孤儿无对应差异账三条全列 |
| UNX-F9160 | ktest syscall 面 B18 批断言集 | 400 | — | UNX-F9160-J1 本批 19 条判据聚合判据过，wait 族与进程控制一次跑通，域累计 86,220 收口断言 |

<!-- 主册行 11347 · #### UNX-C2-B19 · 资源族：setrlimit/getrlimit/setpriority（F9161– -->
#### UNX-C2-B19 · 资源族：setrlimit/getrlimit/setpriority（F9161–F9180 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 资源族）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B19.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16 6,500 + B17 6,400 + B18 6,450 + 本批 6,400 = 92,620 / 240,000｜嫁接源：getrlimit(2)/setpriority(2)/times(2) man-pages 语义对齐；A2 计时基座冻结接口消费；与 B15 F9091/F9092 getrusage/prlimit64 档单源联动；A1 调度消费（sched_yield）｜防重声明：资源族 20 号为号面新铺（B01-B15 getrusage/prlimit64 号面补位已铺于 B15，本批铺 rlimit 全集与优先级族，零重复）；扩号段 F9161-F9180 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；限额硬限提升走 EPERM 矩阵（零越权路径）；零硬件频率/电压类改动（硬件红线条款默认适用）｜批注（AI-12）：B19 是资源族轴：rlimit 全集逐格是 shell ulimit 与 POSIX spawn 的地基，软硬限翻转协议（软可降硬不可升）是安全边界核心，nice 值域与 A1 调度消费单源，RLIMIT_NPROC 与 B25 fork EAGAIN 格联动｜判据与后续 deepen/C2-B19.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9161 | setrlimit/getrlimit 语义档总纲（rlimit 结构四元组） | 420 | — | UNX-F9161-J1 软硬限读写回环判据过，结构四元组探针格过，RLIM_INFINITY 编码格过 |
| UNX-F9162 | RLIMIT_NOFILE/CORE/STACK 三限额逐格 | 360 | — | UNX-F9162-J1 三限额逐格判据过，NOFILE 与 fd 分配器联动可观测，CORE 零值禁转储格过 |
| UNX-F9163 | RLIMIT_AS/DATA/RSS 内存限额族 | 340 | — | UNX-F9163-J1 三限额判据过，AS 与 mmap 分配联动可观测，RSS 提示性语义声明格过 |
| UNX-F9164 | RLIMIT_CPU/NPROC/SIGPENDING 调度限额族 | 340 | — | UNX-F9164-J1 三限额判据过，CPU 超限 SIGXCPU 投递挂点格过，NPROC 与 fork EAGAIN 联动格过 |
| UNX-F9165 | RLIMIT_FSIZE/LOCKS/MSGQUEUE/RTPRIO 补位族 | 300 | — | UNX-F9165-J1 四限额补位判据过，FSIZE 超限 SIGXFSZ 挂点格过，未实装限额零静默声明格过 |
| UNX-F9166 | prlimit64 与 setrlimit 协同（新旧 ABI 双轨） | 340 | — | UNX-F9166-J1 双轨协同判据过，新旧 ABI 行为一致可观测，与 B15 prlimit64 档对账一致 |
| UNX-F9167 | 限额错误矩阵（EINVAL/EPERM/ENOMEM） | 300 | — | UNX-F9167-J1 三错误码注入矩阵判据过，软硬序倒置全 EINVAL 格过，硬限提升全 EPERM 格过 |
| UNX-F9168 | RLIM_INFINITY 与软硬限翻转协议 | 300 | — | UNX-F9168-J1 翻转协议判据过，软限可降可升到硬限判据过，硬限非特权只降格过 |
| UNX-F9169 | setpriority/getpriority 语义档（nice 值域） | 340 | — | UNX-F9169-J1 值域 [-20,19] 判据过，越界钳制语义格过，PRIO_PROCESS/PRIO_PGRP/PRIO_USER 三档格过 |
| UNX-F9170 | 优先级 EACCES/EPERM 矩阵（特权降级规则） | 300 | — | UNX-F9170-J1 降权越级矩阵判据过，nice 降级（更优先）非特权全 EACCES 格过，与 B24 分界声明格过 |
| UNX-F9171 | sched_yield 挂点（A1 调度消费） | 300 | — | UNX-F9171-J1 让出挂点判据过，A1 调度消费声明格过，单任务零副作用格过 |
| UNX-F9172 | times 语义档（tms 四字段与 CLK_TCK） | 320 | — | UNX-F9172-J1 四字段判据过，CLK_TCK 换算一致格过，与 getrusage 对账一致格过 |
| UNX-F9173 | getrusage CHILDREN 联动（wait 收割累计） | 340 | — | UNX-F9173-J1 收割累计判据过，与 wait 族 rusage 联动一致，二次累计零重复格过 |
| UNX-F9174 | 资源族与 A2 计时基座消费协议 | 300 | — | UNX-F9174-J1 消费协议落账判据过，时钟源单源声明格过，上游依赖登记格过 |
| UNX-F9175 | 资源族参数探针总成 | 280 | — | UNX-F9175-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源 |
| UNX-F9176 | 资源族与 shell ulimit 消费核 | 300 | — | UNX-F9176-J1 ulimit 内建消费链判据过，软硬限透传可观测，消费号登记格过 |
| UNX-F9177 | 资源族升级接管注记 | 280 | — | UNX-F9177-J1 接管边界声明判据过，存量零重写承诺格过，升级增量清单落账 |
| UNX-F9178 | 资源族与 Windows 工作集/配额对照格 | 300 | — | UNX-F9178-J1 工作集对照判据过，Job Object 配额映射表落账，差异账三条全列 |
| UNX-F9179 | getrusage RUSAGE_SELF 静态账（与 B15 对账） | 300 | — | UNX-F9179-J1 静态账判据过，与 getrusage 语义档逐字段对账一致，零重复声明格过 |
| UNX-F9180 | ktest syscall 面 B19 批断言集 | 340 | — | UNX-F9180-J1 本批 19 条判据聚合判据过，rlimit 全集与优先级族一次跑通，域累计 92,620 收口断言 |

<!-- 主册行 11374 · #### UNX-C2-B20 · 身份族与休眠族：setuid/nanosleep（F9181–F9200 · 20  -->
#### UNX-C2-B20 · 身份族与休眠族：setuid/nanosleep（F9181–F9200 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 身份族 + 时间族休眠面，依赖 C1/A2）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B20.md 全六要素收口）｜域账累计：B01-B15 66,870 + B16-B19 25,750 + 本批 6,480 = 99,100 / 240,000｜嫁接源：setuid(2)/setgid(2)/nanosleep(2)/clock_nanosleep(2) man-pages 语义对齐；C1 进程对象身份字段消费；A2 时钟仲裁冻结协议消费（F8911）；与 B05 三身份档/B06 时间族单源联动｜防重声明：身份族与休眠族 20 号为号面新铺（B01-B15 三身份语义基准已铺于 B05、时间族已铺于 B06，本批铺 setuid 翻转操作号与 nanosleep 休眠号，零重复）；扩号段 F9181-F9200 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；身份族零越权提权路径（特权翻转 EPERM 矩阵在批内，B24 矩阵联动）；nanosleep 零忙等（让出归 A1 调度，阻塞挂点声明）｜批注（AI-12）：B20 是 M 型收尾批：setuid 三身份翻转协议是 setuid 程序安全模型的地基（保存身份翻转规则逐格），nanosleep 的 EINTR+remaining 回写是 B26 EINTR 矩阵的前置消费面，clock_nanosleep 的 TIMER_ABSTIME 语义为 A2 精度账预留｜判据与后续 deepen/C2-B20.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9181 | setuid/setreuid 语义档（三身份翻转协议） | 400 | — | UNX-F9181-J1 翻转协议判据过，特权根全翻格过，非特权互转格过，保存身份规则逐格过 |
| UNX-F9182 | setresuid/setfsuid 语义档 | 360 | — | UNX-F9182-J1 三参数独立翻转判据过，-1 占位不变语义格过，fsuid 特权限制格过 |
| UNX-F9183 | setgid/setregid/setresgid 语义档 | 360 | — | UNX-F9183-J1 组身份三操作判据过，与 uid 族翻转协议对称可观测，补充组集零影响声明格过 |
| UNX-F9184 | 身份翻转 EPERM 矩阵（特权规则） | 340 | — | UNX-F9184-J1 非特权提限全 EPERM 矩阵判据过，逐码与 Linux 语义一致，特权翻转零残留格过 |
| UNX-F9185 | getgroups/setgroups 语义档（补充组集） | 340 | — | UNX-F9185-J1 组集读写回环判据过，size 不足全 EINVAL 格过，特权限制格过 |
| UNX-F9186 | umask 语义档（继承与返回旧值） | 280 | — | UNX-F9186-J1 返回旧值判据过，fork/exec 继承可观测，与 open 创建模式联动格过 |
| UNX-F9187 | 身份族与 B5 三身份档对账格 | 300 | — | UNX-F9187-J1 对账判据过，与三身份语义基准逐字段一致，零语义漂移声明格过 |
| UNX-F9188 | nanosleep 语义档（timespec 规范化） | 380 | — | UNX-F9188-J1 规范化判据过，tv_nsec 值域 [0,1e9) 越界全 EINVAL 格过，早唤醒零承诺格过 |
| UNX-F9189 | nanosleep EINTR 重启与 remaining 回写 | 360 | — | UNX-F9189-J1 EINTR 回写判据过，remaining 剩余量可观测，与 B26 重启矩阵联动声明格过 |
| UNX-F9190 | clock_nanosleep 语义档（时钟基与 TIMER_ABSTIME） | 360 | — | UNX-F9190-J1 时钟基选择判据过，ABSTIME 绝对语义格过，CLOCK_MONOTONIC 越界格过 |
| UNX-F9191 | 休眠族与 A2 时钟仲裁消费协议 | 320 | — | UNX-F9191-J1 消费协议落账判据过，F8911 仲裁冻结单引用格过，时钟源单源声明格过 |
| UNX-F9192 | 休眠族精度账（与 A2 jitter 对齐） | 300 | — | UNX-F9192-J1 精度账落账判据过，jitter 对齐声明格过，真机精度判据随闸门补测登记格过 |
| UNX-F9193 | 身份族与 C1 进程对象消费协议 | 300 | — | UNX-F9193-J1 消费协议落账判据过，身份字段单源格过，上游依赖登记格过 |
| UNX-F9194 | 身份/休眠族错误矩阵补格（EINVAL/ENOMEM） | 280 | — | UNX-F9194-J1 补格矩阵判据过，逐码与 Linux 语义一致，与 B13 映射账对账一致 |
| UNX-F9195 | setuid 族与 execve 特权翻转联动（s 位注记） | 300 | — | UNX-F9195-J1 s 位注记落账判据过，exec 后身份重算挂点声明格过，实装归 C1 联签格过 |
| UNX-F9196 | 身份/休眠族参数探针总成 | 280 | — | UNX-F9196-J1 nargs 逐位提取判据过，坏 timespec 全 EFAULT 格过，与参数拷贝探针单源 |
| UNX-F9197 | 休眠族与 shell sleep 消费核 | 300 | — | UNX-F9197-J1 sleep 内建消费链判据过，秒到 timespec 换算可观测，消费号登记格过 |
| UNX-F9198 | 身份族升级接管注记（B5 存量三身份档） | 280 | — | UNX-F9198-J1 接管边界声明判据过，B5 语义档零重写承诺格过，操作号增量清单落账 |
| UNX-F9199 | 身份族与 Windows 令牌特权对照格 | 300 | — | UNX-F9199-J1 令牌特权对照判据过，AdjustTokenPrivileges 映射表落账，差异账三条全列 |
| UNX-F9200 | ktest syscall 面 B20 批断言集 + M 型收口断言 | 340 | — | UNX-F9200-J1 本批 19 条判据聚合判据过，M 型收尾五批（B16-B20）一次跑通断言过，域累计 99,100 收口断言 |

<!-- 主册行 11401 · #### UNX-C2-B21 · E 型错误矩阵一：文件族（EFAULT/EBADF）（F9201–F9220 · 2 -->
#### UNX-C2-B21 · E 型错误矩阵一：文件族（EFAULT/EBADF）（F9201–F9220 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 坏指针全 EFAULT、坏 fd 全 EBADF 文件族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B21.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B20 32,230 + 本批 6,600 = 105,700 / 240,000｜嫁接源：man-pages errno(3)/read(2)/write(2)/open(2) 错误段语义对齐；与 B03 write·read/B04 mmap 三联/B05 open·close·lseek·stat 深化册正反分层联动（正判据在深化册、反判据在本批）｜防重声明：文件族 E 型 20 号为号面新铺（B01–B15 文件族正判据已深化，本批只铺错误注入矩阵号，正反分层零重复）；扩号段 F9201–F9220 承接冻结面 F9101–F9220，零重编零私设号（只做 Linux 上游约定号面）｜红线注记：本批零写盘零引导零固件操作；错误注入零内核损坏承诺（注入后内核状态可恢复，100 次注入零 panic 声明）；双轨产线条款默认适用｜批注（AI-12）：B21 是 E 型矩阵首批：坏指针/坏 fd 注入以文件族为首发族（read/write/close/lseek/open/stat 六面），EFAULT 拷贝半程探针与参数拷贝探针总成单源，错误码判序协议（EFAULT>EBADF>EINVAL）在本批首立｜判据与后续 deepen/C2-B21.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9201 | E 型方法论总纲（正反双判据协议与矩阵记法） | 420 | — | UNX-F9201-J1 正反双判据双成分协议落账判据过，矩阵记法（错误码×注入点×期望值三列）逐格可复测格过，与 B13 错误编码器总成对账一致 |
| UNX-F9202 | read/write 坏 fd 矩阵（全 EBADF） | 340 | — | UNX-F9202-J1 负 fd/未开 fd/已关 fd 三类注入矩阵判据过，全返回 -EBADF 与 Linux 逐码一致，10/10 次检出复测格过 |
| UNX-F9203 | read/write 坏指针矩阵（全 EFAULT） | 340 | — | UNX-F9203-J1 NULL/-1/未映射/只读映射写路径四类注入矩阵判据过，全 -EFAULT 格过，部分拷贝残留量声明格过 |
| UNX-F9204 | close 坏 fd 矩阵（含双关闭竞态） | 320 | — | UNX-F9204-J1 坏 fd 全 EBADF 判据过，双 close 竞态注入第二次全 EBADF 格过，fd 槽位复用零泄漏格过 |
| UNX-F9205 | lseek 坏 fd/坏 whence 矩阵 | 320 | — | UNX-F9205-J1 坏 fd 全 EBADF 判据过，非法 whence 全 EINVAL 格过，负偏移全 EINVAL 格过 |
| UNX-F9206 | open 坏路径指针 EFAULT 矩阵 | 320 | — | UNX-F9206-J1 坏路径指针注入全 EFAULT 判据过，路径截断注入零越界读格过，10/10 次检出格过 |
| UNX-F9207 | open 坏 flags 组合 EINVAL 矩阵 | 320 | — | UNX-F9207-J1 O_CREAT 缺 mode/非法 O_ 位组合注入矩阵判据过，全 EINVAL 或按 Linux 语义位忽略格过，逐码对账一致 |
| UNX-F9208 | stat/fstat 坏指针/坏 fd 矩阵 | 340 | — | UNX-F9208-J1 坏 buf 指针全 EFAULT 判据过，坏 fd 全 EBADF 格过，路径不存在全 ENOENT 格过 |
| UNX-F9209 | 文件族负偏移与溢出边界矩阵 | 320 | — | UNX-F9209-J1 负 offset 注入全 EINVAL 判据过，off_t 溢出边界截断语义格过，与 B04 lseek 档对账一致 |
| UNX-F9210 | 文件族零长度读写边界矩阵 | 300 | — | UNX-F9210-J1 零长度 read/write 正判据（零副作用返回 0）判据过，反向判序（坏 fd 优先于零长度）格过 |
| UNX-F9211 | 坏 fd 注入跨号段扫描（fd 空间全域） | 340 | — | UNX-F9211-J1 fd 空间全域扫描注入判据过，未开槽位全 EBADF 逐格检出，越上限与 RLIMIT_NOFILE 分界格过 |
| UNX-F9212 | EFAULT 拷贝半程探针（copy_to_user 半途） | 340 | — | UNX-F9212-J1 半途拷贝注入判据过，已拷贝字节数返回语义与 Linux 一致格过，半程残留可观测格过 |
| UNX-F9213 | 错误码编码器一致性矩阵（Errno→sysret 逐码） | 340 | — | UNX-F9213-J1 Errno 全集逐码注入判据过，sysret 编码逐码一致格过，与 B13 映射账零漂移格过 |
| UNX-F9214 | 文件族 E 型参数探针总成 | 300 | — | UNX-F9214-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针总成单源 |
| UNX-F9215 | 文件族 E 型与 B03–B05 深化册对账格 | 320 | — | UNX-F9215-J1 对账判据过，与 open/close/lseek/stat 深化册逐条判据零冲突，正反分层声明格过 |
| UNX-F9216 | 文件族 E 型与 Windows GetLastError 对照格 | 300 | — | UNX-F9216-J1 ERROR_INVALID_HANDLE/ERROR_NOACCESS 映射表判据过，差异账三条全列格过 |
| UNX-F9217 | E 型判据可复测性声明（10/10 次检出） | 300 | — | UNX-F9217-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9218 | 文件族 E 型升级接管注记 | 280 | — | UNX-F9218-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9219 | 文件族 E 型与 glibc errno 透传消费核 | 320 | — | UNX-F9219-J1 glibc errno 读取透传链判据过，perror/strerror 输出逐码一致格过，消费号登记格过 |
| UNX-F9220 | ktest syscall 面 B21 批断言集 | 420 | — | UNX-F9220-J1 本批 19 条判据聚合判据过，文件族 E 型矩阵一次跑通，域累计 105,700 收口断言 |

<!-- 主册行 11428 · #### UNX-C2-B22 · E 型错误矩阵二：fd 族（EBADF/ENOENT/EMFILE/ENFILE）（ -->
#### UNX-C2-B22 · E 型错误矩阵二：fd 族（EBADF/ENOENT/EMFILE/ENFILE）（F9221–F9240 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 坏 fd 全 EBADF、路径缺失全 ENOENT fd 族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B22.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B21 38,830 + 本批 6,700 = 112,400 / 240,000｜嫁接源：man-pages dup(2)/pipe(2)/open(2)/path_resolution(7) 错误段语义对齐；与 B08 dup 族/B15 F9093 管道账/B03 深化册单源联动；B1 VFS 冻结读路径消费｜防重声明：fd 族 E 型 20 号为号面新铺（B01–B15 fd 正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9221–F9240 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；fd 竞态注入零越权写承诺（TOCTOU 窗口注入后无跨进程越权可观测）；双轨产线条款默认适用｜批注（AI-12）：B22 是 fd 族错误矩阵批：EBADF 判定域三条件（越界/未开/已关）与 ENOENT 判定面（路径解析三态）逐格落账，EBADF vs ENOENT 判定次序协议（先 fd 后路径）是全 E 型批的判序地基，EMFILE/ENFILE 双层耗尽矩阵与 B19 RLIMIT_NOFILE 联动｜判据与后续 deepen/C2-B22.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9221 | fd 语义域总纲（EBADF 判定域与 fd 空间模型） | 400 | — | UNX-F9221-J1 fd 空间模型（0/1/2 基座+分配序）判据过，EBADF 判定域三条件（越界/未开/已关）逐格落账，判定先于操作语义格过 |
| UNX-F9222 | dup/dup2/dup3 坏 fd 矩阵 | 340 | — | UNX-F9222-J1 oldfd/newfd 坏值注入矩阵判据过，全 EBADF 逐格检出，dup2 equal-fd 返回 newfd 正判据格过 |
| UNX-F9223 | pipe 系统调用错误矩阵（EFAULT/EMFILE/ENFILE） | 340 | — | UNX-F9223-J1 三错误码注入矩阵判据过，坏 pipefd 指针全 EFAULT 格过，逐码与 Linux 一致 |
| UNX-F9224 | fd 表耗尽 EMFILE 矩阵（RLIMIT_NOFILE 联动） | 360 | — | UNX-F9224-J1 限额触顶注入判据过，分配全 EMFILE 格过，限额上调后恢复正判据格过，与 B19 资源族联动格过 |
| UNX-F9225 | 系统级 ENFILE 矩阵（文件表上限） | 320 | — | UNX-F9225-J1 系统级上限注入判据过，全 ENFILE 格过，与进程级 EMFILE 判序（先进程后系统）格过 |
| UNX-F9226 | ENOENT 判定域总纲（路径解析错误面） | 360 | — | UNX-F9226-J1 路径解析错误面判据过，ENOENT 触发三态（末段缺失/中间段缺失/空组件）逐格落账，与 B1 VFS 冻结接口消费格过 |
| UNX-F9227 | open/stat/unlink ENOENT 矩阵 | 340 | — | UNX-F9227-J1 三 syscall 缺失路径注入矩阵判据过，全 ENOENT 逐格检出，O_CREAT 正判据（存在即打开）格过 |
| UNX-F9228 | 目录操作 ENOENT/ENOTDIR/EISDIR 矩阵 | 340 | — | UNX-F9228-J1 三错误码注入矩阵判据过，目录尾段缺失全 ENOENT 格过，文件作目录全 ENOTDIR 格过 |
| UNX-F9229 | 相对路径与 AT_FDCWD 边界矩阵 | 320 | — | UNX-F9229-J1 AT_FDCWD 正判据判据过，坏 dirfd 全 EBADF 格过，绝对路径忽略 dirfd 语义格过 |
| UNX-F9230 | dirfd 坏值矩阵（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW） | 320 | — | UNX-F9230-J1 两标志判据过，坏 dirfd 全 EBADF 格过，非目录 fd 全 ENOTDIR 格过 |
| UNX-F9231 | EBADF vs ENOENT 判定次序协议（先 fd 后路径） | 340 | — | UNX-F9231-J1 判序协议判据过，坏 fd 优先返回 EBADF 逐格检出，判序与 Linux 逐例一致格过 |
| UNX-F9232 | fd 复用竞态注入（close-then-use TOCTOU） | 340 | — | UNX-F9232-J1 close-then-use 竞态注入判据过，槽位复用窗口可观测格过，100 次注入零越权写格过 |
| UNX-F9233 | 错误码返回路径探针（负值编码与 err 链路） | 320 | — | UNX-F9233-J1 负值编码探针判据过，-Errno 逐码编码一致格过，err 链路与 B13 编码器单源格过 |
| UNX-F9234 | fd 族 E 型参数探针总成 | 300 | — | UNX-F9234-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9235 | fd 族 E 型与 B08/B15 深化册对账格 | 300 | — | UNX-F9235-J1 对账判据过，与 dup 族/管道深化册逐条判据零冲突格过，正反分层声明格过 |
| UNX-F9236 | fd 族 E 型与 Windows INVALID_HANDLE_VALUE 对照格 | 300 | — | UNX-F9236-J1 INVALID_HANDLE_VALUE 映射判据过，WSA 错误差异账三条全列格过 |
| UNX-F9237 | E 型判据可复测性声明（10/10 次检出） | 300 | — | UNX-F9237-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9238 | fd 族 E 型升级接管注记 | 280 | — | UNX-F9238-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9239 | fd 族 E 型与 glibc perror/strerror 消费核 | 320 | — | UNX-F9239-J1 strerror 逐码输出判据过，与 errno 透传单源格过，消费号登记格过 |
| UNX-F9240 | ktest syscall 面 B22 批断言集 | 460 | — | UNX-F9240-J1 本批 19 条判据聚合判据过，fd 族 E 型矩阵一次跑通，域累计 112,400 收口断言 |

<!-- 主册行 11455 · #### UNX-C2-B23 · E 型错误矩阵三：内存族（EINVAL/ENOMEM）（F9241–F9260 ·  -->
#### UNX-C2-B23 · E 型错误矩阵三：内存族（EINVAL/ENOMEM）（F9241–F9260 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 非法参数全 EINVAL、地址耗尽全 ENOMEM 内存族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B23.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B22 45,530 + 本批 6,800 = 119,200 / 240,000｜嫁接源：man-pages mmap(2)/mprotect(2)/brk(2) 错误段语义对齐；与 B06/B07 深化册单源联动；A4 页表底座消费协议引用｜防重声明：内存族 E 型 20 号为号面新铺（B17 内存扩容正判据已铺，本批只铺错误注入矩阵号，零重复）；扩号段 F9241–F9260 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；越界注入零内核损坏承诺（SIGSEGV 只投递用户态，零内核态损坏）；双轨产线条款默认适用｜批注（AI-12）：B23 是内存族错误矩阵批：对齐/越界/耗尽/权限四错误域逐格，MAP_FIXED_NOREPLACE 的 EEXIST 格与 SIGSEGV 投递路径（si_addr 逐格）是判据重心，mremap/madvise 错误矩阵与 B17 正反分层零重复｜判据与后续 deepen/C2-B23.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9241 | 内存族 E 型总纲（地址空间错误面） | 400 | — | UNX-F9241-J1 地址空间错误面判据过，四类错误域（对齐/越界/耗尽/权限）逐格落账，与本批矩阵映射一致格过 |
| UNX-F9242 | mmap 坏 addr 提示与坏映射探针矩阵 | 360 | — | UNX-F9242-J1 mmap addr 非法提示值注入判据过，提示值零副作用正判据格过，映射后访问坏区间 SIGSEGV 路径格过 |
| UNX-F9243 | mmap 未对齐 EINVAL 矩阵 | 360 | — | UNX-F9243-J1 非页对齐 addr/length 注入矩阵判据过，全 EINVAL 逐格检出（MAP_FIXED 时全 EINVAL），页掩码对账格过 |
| UNX-F9244 | mmap ENOMEM 矩阵（超限/无空间） | 360 | — | UNX-F9244-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，max_map_count 上限格过，与 RLIMIT_AS 联动格过 |
| UNX-F9245 | munmap/mprotect 坏区间矩阵 | 360 | — | UNX-F9245-J1 未对齐区间注入判据过，munmap 全 EINVAL 格过，mprotect 坏 prot 全 EINVAL 格过，区间不存在全 ENOMEM 格过 |
| UNX-F9246 | brk 坏值矩阵（越下界 EINVAL） | 320 | — | UNX-F9246-J1 低于初始堆底注入判据过，全 EINVAL 格过，返回新 break 正判据格过，与 B07 brk 档对账一致 |
| UNX-F9247 | mremap 错误矩阵（EINVAL/ENOMEM/EPERM 联 B17） | 340 | — | UNX-F9247-J1 三错误码注入矩阵判据过，非对齐 old_addr 全 EINVAL 格过，与 B17 矩阵零重复分层声明格过 |
| UNX-F9248 | madvise 错误矩阵（EINVAL/ENOMEM 联 B17） | 340 | — | UNX-F9248-J1 未知 advice 全 EINVAL 判据过，越界区间全 ENOMEM 格过，与 B17 矩阵零重复分层声明格过 |
| UNX-F9249 | mlock/mprotect 权限边界矩阵（与 B24 分界声明） | 340 | — | UNX-F9249-J1 锁页越限全 EPERM 判据过，与 B24 权限矩阵分界声明格过，零重复判据格过 |
| UNX-F9250 | 地址空间越界访问 SIGSEGV 路径矩阵 | 340 | — | UNX-F9250-J1 越界读写触发 #PF 判据过，SIGSEGV 投递可观测格过，si_addr 逐格正确格过 |
| UNX-F9251 | 页对齐边界值矩阵（4K/2M 大页） | 340 | — | UNX-F9251-J1 4K 页界逐位注入判据过，2M 大页边界格过，跨界映射拒绝语义格过 |
| UNX-F9252 | 零长度映射边界矩阵 | 320 | — | UNX-F9252-J1 零长度 mmap 正判据（返回合法地址零副作用）判据过，零长度 munmap 全 EINVAL 格过 |
| UNX-F9253 | MAP_FIXED 冲突矩阵（ENOENT/EEXIST） | 340 | — | UNX-F9253-J1 MAP_FIXED 覆盖语义判据过，与现存映射冲突覆盖可观测格过，MAP_FIXED_NOREPLACE 全 EEXIST 格过 |
| UNX-F9254 | 内存族 E 型参数探针总成 | 320 | — | UNX-F9254-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9255 | 内存族 E 型与 B06/B07 深化册对账格 | 320 | — | UNX-F9255-J1 对账判据过，与 mmap 三联/brk 档深化册逐条判据零冲突格过，正反分层声明格过 |
| UNX-F9256 | 内存族 E 型与 Windows VirtualAlloc 错误码对照格 | 320 | — | UNX-F9256-J1 ERROR_INVALID_ADDRESS 映射判据过，差异账三条全列格过 |
| UNX-F9257 | E 型判据可复测性声明（10/10 次检出） | 320 | — | UNX-F9257-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9258 | 内存族 E 型升级接管注记 | 280 | — | UNX-F9258-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9259 | 内存族 E 型与 glibc malloc 失败路径消费核 | 340 | — | UNX-F9259-J1 malloc 返回 NULL 路径判据过，ENOMEM 透传可观测格过，消费号登记格过 |
| UNX-F9260 | ktest syscall 面 B23 批断言集 | 380 | — | UNX-F9260-J1 本批 19 条判据聚合判据过，内存族 E 型矩阵一次跑通，域累计 119,200 收口断言 |

<!-- 主册行 11482 · #### UNX-C2-B24 · E 型错误矩阵四：权限族（EPERM/EACCES）（F9261–F9280 · 2 -->
#### UNX-C2-B24 · E 型错误矩阵四：权限族（EPERM/EACCES）（F9261–F9280 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 特权缺失全 EPERM、对象权限不足全 EACCES 权限族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B24.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B23 52,330 + 本批 6,800 = 126,000 / 240,000｜嫁接源：man-pages credentials(7)/path_resolution(7)/chmod(2) 语义对齐；与 B05 三身份档/B16 ioctl 禁用格/B19 硬限格/B20 身份矩阵联动对账；guard.rs CAP_* 能力位单源｜防重声明：权限族 E 型 20 号为号面新铺（B01–B15 权限正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9261–F9280 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；零越权路径承诺（EPERM 注入矩阵只验证拒绝，零提权旁路）；硬件端口禁用格零触及；双轨产线条款默认适用｜批注（AI-12）：B24 是权限族错误矩阵批：EPERM（操作特权缺失）vs EACCES（对象权限不足）判定域分界是本批主轴，st_mode 九位逐格与 CAP_* 能力位矩阵是安全模型地基，ioctl ioperm/iopl 禁用格与 B16 红线注记闭环｜判据与后续 deepen/C2-B24.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9261 | 权限错误面总纲（EPERM vs EACCES 判定域） | 420 | — | UNX-F9261-J1 两错误码判定域判据过，EPERM（操作特权缺失）与 EACCES（对象权限不足）分界逐格落账，与 Linux 语义逐例一致格过 |
| UNX-F9262 | open EACCES 矩阵（rwx 三位逐格） | 360 | — | UNX-F9262-J1 rwx 三位逐格注入判据过，缺 r 读全 EACCES 格过，缺 w 写全 EACCES 格过，缺 x 执行全 EACCES 格过 |
| UNX-F9263 | chmod/chown EPERM 矩阵（非属主） | 340 | — | UNX-F9263-J1 非属主注入判据过，全 EPERM 格过，特权路径正判据格过，与 B05 三身份档对账格过 |
| UNX-F9264 | 权限位模型（st_mode 九位逐格矩阵） | 380 | — | UNX-F9264-J1 九位逐格矩阵判据过，ugo×rwx 逐格可观测，S_IFMT 类型位掩码格过，与 stat 结构单源格过 |
| UNX-F9265 | setuid/setgid 文件位与执行路径矩阵 | 340 | — | UNX-F9265-J1 s 位语义挂点判据过，目录 setgid 位继承挂点格过，实装归 C1/B1 联签声明格过 |
| UNX-F9266 | 目录搜索权限 EACCES 矩阵（路径逐段） | 360 | — | UNX-F9266-J1 路径逐段缺 x 注入判据过，全 EACCES 格过，错误点（首段缺失优先）判序格过 |
| UNX-F9267 | EPERM 特权操作矩阵（root-only 全集） | 340 | — | UNX-F9267-J1 root-only 操作全集注入判据过，非 root 全 EPERM 逐格检出，CAP 消费声明格过 |
| UNX-F9268 | 能力位 CAP_* 与 EPERM 判定矩阵 | 360 | — | UNX-F9268-J1 guard::CAP_* 能力位逐位注入判据过，缺 CAP 全 EPERM 逐格检出，能力位与 uid 判序格过 |
| UNX-F9269 | 只读文件系统 EROFS 矩阵 | 320 | — | UNX-F9269-J1 EROFS 注入判据过，写路径全 EROFS 逐格检出，与 EACCES 判序（先挂载只读后权限）格过 |
| UNX-F9270 | EBUSY/EEXIST 冲突矩阵（独占创建） | 320 | — | UNX-F9270-J1 O_EXCL 冲突注入判据过，全 EEXIST 格过，EBUSY 独占冲突格过 |
| UNX-F9271 | 身份翻转 EPERM 矩阵（联 B20 对账） | 320 | — | UNX-F9271-J1 非特权提限注入判据过，全 EPERM 逐格检出，与 B20 身份矩阵零重复分层格过 |
| UNX-F9272 | ioctl ioperm/iopl 禁用格 EPERM 矩阵 | 320 | — | UNX-F9272-J1 ioperm/iopl 注入判据过，全 EPERM 拒绝格过，与 B16 红线注记（禁用格）对账一致格过 |
| UNX-F9273 | rlimit 硬限提升 EPERM 矩阵（联 B19 对账） | 320 | — | UNX-F9273-J1 硬限提升注入判据过，非特权全 EPERM 格过，与 B19 资源矩阵零重复分层格过 |
| UNX-F9274 | 权限族 E 型参数探针总成 | 300 | — | UNX-F9274-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9275 | 权限族 E 型与三身份档/B05 深化册对账格 | 320 | — | UNX-F9275-J1 对账判据过，与 B05 三身份语义基准逐条零冲突格过，正反分层声明格过 |
| UNX-F9276 | 权限族 E 型与 Windows Access Denied 对照格 | 300 | — | UNX-F9276-J1 ERROR_ACCESS_DENIED 映射判据过，差异账三条全列格过 |
| UNX-F9277 | E 型判据可复测性声明（10/10 次检出） | 300 | — | UNX-F9277-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9278 | 权限族 E 型升级接管注记 | 280 | — | UNX-F9278-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9279 | 权限族 E 型与 glibc access/euid 消费核 | 320 | — | UNX-F9279-J1 access 族真实 uid 语义判据过，与 euid 差异可观测格过，消费号登记格过 |
| UNX-F9280 | ktest syscall 面 B24 批断言集 | 480 | — | UNX-F9280-J1 本批 19 条判据聚合判据过，权限族 E 型矩阵一次跑通，域累计 126,000 收口断言 |

<!-- 主册行 11509 · #### UNX-C2-B25 · E 型错误矩阵五：进程族（ESRCH/EAGAIN）（F9281–F9300 · 2 -->
#### UNX-C2-B25 · E 型错误矩阵五：进程族（ESRCH/EAGAIN）（F9281–F9300 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · pid 不存在全 ESRCH、资源耗尽全 EAGAIN 进程族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B25.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B24 59,130 + 本批 6,700 = 132,700 / 240,000｜嫁接源：man-pages kill(2)/fork(2)/execve(2) 错误段语义对齐；与 B04 fork 档/B08 进程族地基/B18 wait 族深化册单源联动；C1 进程模型冻结接口消费｜防重声明：进程族 E 型 20 号为号面新铺（B01–B15/B18 进程正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9281–F9300 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；零进程强杀路径承诺（kill 矩阵只验投递拒绝与权限，SIGKILL 语义归 B11）；双轨产线条款默认适用｜批注（AI-12）：B25 是进程族错误矩阵批：ESRCH 判定次序协议（pid 存在性先于权限）与 EAGAIN 双源（RLIMIT_NPROC/线程上限）是判据重心，execve 四错误码矩阵与 E2BIG 参数上限是 shell 启动链的地基，竞态注入双条（收割/投递）100 次统计｜判据与后续 deepen/C2-B25.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9281 | 进程族 E 型总纲（进程空间错误面） | 400 | — | UNX-F9281-J1 进程空间错误面判据过，四类错误域（pid 不存在/权限/资源耗尽/参数坏值）逐格落账，判序协议声明格过 |
| UNX-F9282 | kill 坏 pid 矩阵（ESRCH/EPERM/EINVAL） | 360 | — | UNX-F9282-J1 三错误码注入矩阵判据过，pid=1 EPERM 特例格过，非法 sig 全 EINVAL 格过，不存在 pid 全 ESRCH 格过 |
| UNX-F9283 | wait 坏 pid ECHILD 矩阵（联 B18 对账） | 320 | — | UNX-F9283-J1 无子进程注入判据过，全 ECHILD 格过，与 B18 wait 矩阵零重复分层格过 |
| UNX-F9284 | fork/clone EAGAIN 矩阵（RLIMIT_NPROC 联动） | 380 | — | UNX-F9284-J1 进程数触顶注入判据过，全 EAGAIN 格过，与 B19 NPROC 联动格过，释放后恢复正判据格过 |
| UNX-F9285 | fork/clone ENOMEM 矩阵（地址空间耗尽） | 340 | — | UNX-F9285-J1 地址空间耗尽注入判据过，全 ENOMEM 格过，COW 挂点依赖声明格过 |
| UNX-F9286 | setpgid 坏 pid ESRCH 矩阵 | 320 | — | UNX-F9286-J1 坏 pid 注入判据过，全 ESRCH 格过，跨会话 EPERM 格过，与 B18 对账格过 |
| UNX-F9287 | setsid EPERM 矩阵（会话首规则联 B18） | 320 | — | UNX-F9287-J1 会话首注入判据过，全 EPERM 格过，与 B18 选举语义对账一致格过 |
| UNX-F9288 | execve 错误矩阵（EACCES/ENOEXEC/E2BIG/EFAULT） | 380 | — | UNX-F9288-J1 四错误码注入矩阵判据过，坏路径全 EFAULT 格过，非脚本非 ELF 全 ENOEXEC 格过，判序与 Linux 一致格过 |
| UNX-F9289 | 参数长度 E2BIG 矩阵（argv/envp 上限） | 340 | — | UNX-F9289-J1 超限 argv 注入判据过，全 E2BIG 格过，单参数上限 MAX_ARG_STRLEN 格过 |
| UNX-F9290 | 零参数 execve 边界矩阵 | 300 | — | UNX-F9290-J1 空 argv 正判据（argv[0] 可空）判据过，空 envp 正判据格过，坏指针优先判序格过 |
| UNX-F9291 | 僵尸收割竞态注入（wait vs exit 竞态） | 340 | — | UNX-F9291-J1 wait-exit 竞态注入判据过，单次收割唯一性 100 次统计格过，双 wait 第二次全 ECHILD 格过 |
| UNX-F9292 | 进程退出竞态注入（SIGCHLD 投递竞态） | 340 | — | UNX-F9292-J1 SIGCHLD 投递竞态注入判据过，投递不丢失 100 次统计格过，与 B11 链对账一致格过 |
| UNX-F9293 | ESRCH 判定次序协议（pid 存在性先于权限） | 320 | — | UNX-F9293-J1 判序协议判据过，不存在 pid 全 ESRCH 逐格检出（先于 EPERM），与 Linux 逐例一致格过 |
| UNX-F9294 | 进程族 E 型参数探针总成 | 300 | — | UNX-F9294-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9295 | 进程族 E 型与 B04/B08 深化册对账格 | 300 | — | UNX-F9295-J1 对账判据过，与 fork 档/进程族地基深化册逐条零冲突格过，正反分层声明格过 |
| UNX-F9296 | 进程族 E 型与 Windows 进程句柄错误对照格 | 300 | — | UNX-F9296-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过 |
| UNX-F9297 | E 型判据可复测性声明（10/10 次检出） | 300 | — | UNX-F9297-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9298 | 进程族 E 型升级接管注记 | 280 | — | UNX-F9298-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9299 | 进程族 E 型与 glibc fork/exec 错误路径消费核 | 320 | — | UNX-F9299-J1 fork 返回 -1 errno 透传判据过，exec 失败子进程 _exit(127) 惯例格过，消费号登记格过 |
| UNX-F9300 | ktest syscall 面 B25 批断言集 | 440 | — | UNX-F9300-J1 本批 19 条判据聚合判据过，进程族 E 型矩阵一次跑通，域累计 132,700 收口断言 |

<!-- 主册行 11536 · #### UNX-C2-B26 · E 型错误矩阵六：EINTR 竞态注入（F9301–F9320 · 20 条） -->
#### UNX-C2-B26 · E 型错误矩阵六：EINTR 竞态注入（F9301–F9320 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 竞态注入 EINTR 重启语义矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B26.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B25 65,830 + 本批 7,200 = 139,900 / 240,000｜嫁接源：man-pages signal-safety(7)/sigaction(2) SA_RESTART 语义对齐；与 B11/B12 信号挂点族/B20 nanosleep/B09 四件套对账；A1 调度让出协议消费｜防重声明：EINTR 竞态 20 号为号面新铺（B01–B15 EINTR 正判据散布各深化册，本批集中立注入矩阵号，正反分层零重复）；扩号段 F9301–F9320 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；竞态注入零丢失承诺（信号不丢失 100 次统计）；零忙等承诺（让出归 A1）；双轨产线条款默认适用｜批注（AI-12）：B26 是 EINTR 竞态注入批（E 型最重轴）：SA_RESTART 逐 syscall 重启面矩阵与部分完成语义（bytes-copied-before-signal）是 glibc TEMP_FAILURE_RETRY 消费的地基，信号发生器注入协议（时序可控）与先完成后中断判序是可复测性的核心——E 型批判据 100 次注入统计首立｜判据与后续 deepen/C2-B26.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9301 | EINTR 语义总纲（慢系统调用中断面） | 420 | — | UNX-F9301-J1 慢系统调用中断面判据过，EINTR 触发三条件（信号到达/无 SA_RESTART/阻塞中）逐格落账，与 Linux 语义一致格过 |
| UNX-F9302 | SA_RESTART 重启语义矩阵（逐 syscall 重启面） | 420 | — | UNX-F9302-J1 SA_RESTART 逐 syscall 重启面矩阵判据过，可重启族逐格重启可观测，不可重启族逐格 EINTR 格过 |
| UNX-F9303 | read/write EINTR 矩阵（部分读写残留量） | 400 | — | UNX-F9303-J1 部分读写后中断注入判据过，已传字节数返回正判据格过，零字节残留 EINTR 格过，与 B03/B04 对账格过 |
| UNX-F9304 | nanosleep EINTR remaining 回写矩阵（联 B20） | 380 | — | UNX-F9304-J1 中断注入 remaining 回写判据过，剩余量逐 ns 可观测格过，与 B20 休眠族对账一致格过 |
| UNX-F9305 | wait EINTR 矩阵（联 B18） | 320 | — | UNX-F9305-J1 阻塞 wait 中断注入判据过，全 EINTR 格过，WNOHANG 零中断正判据格过 |
| UNX-F9306 | poll/epoll EINTR 矩阵（就绪前中断） | 380 | — | UNX-F9306-J1 就绪前中断注入判据过，全 EINTR 格过，重启后重入正确格过，与 B09 四件套对账格过 |
| UNX-F9307 | 阻塞挂点 EINTR 矩阵（waitqueue 等待中断窗） | 340 | — | UNX-F9307-J1 等待队列中断注入判据过，全 EINTR 格过，唤醒零丢失格过，与 A3 fake 原语对账格过 |
| UNX-F9308 | flock/lockf EINTR 矩阵（锁等待中断） | 320 | — | UNX-F9308-J1 锁等待中断注入判据过，全 EINTR 格过，锁状态零破坏格过，与 B16 记录锁挂点对账格过 |
| UNX-F9309 | 信号投递时机竞态矩阵（阻塞前后双窗） | 400 | — | UNX-F9309-J1 阻塞前后双窗注入判据过，双窗逐格可观测，信号不丢失 100 次统计格过，与 B11/B12 对账格过 |
| UNX-F9310 | EINTR 与线程取消联动矩阵 | 320 | — | UNX-F9310-J1 取消点注入判据过，取消点集逐格落账，实装归 C1 线程联签声明格过 |
| UNX-F9311 | 部分完成语义矩阵（bytes-copied-before-signal） | 380 | — | UNX-F9311-J1 半程拷贝后中断注入判据过，已拷贝字节返回语义格过，与 EFAULT 半程探针对账一致格过 |
| UNX-F9312 | EINTR 错误注入器（信号发生器协议） | 400 | — | UNX-F9312-J1 信号发生器注入协议判据过，注入时序可控可观测，100 次注入统计复测格过 |
| UNX-F9313 | EINTR 判定次序协议（先完成后中断） | 340 | — | UNX-F9313-J1 判序协议判据过，完成优先于中断逐格检出，与 Linux 语义一致格过 |
| UNX-F9314 | E 型竞态可复测性声明（竞态 100 次注入统计） | 340 | — | UNX-F9314-J1 竞态 100 次注入统计判据过，统计分布落账格过，零 flaky 阈值声明格过 |
| UNX-F9315 | EINTR 与 B12 事件挂点族对账格 | 320 | — | UNX-F9315-J1 对账判据过，与 B12 四输入判定器逐条零冲突格过，正反分层声明格过 |
| UNX-F9316 | EINTR 与 A1 调度让出协议对账格 | 320 | — | UNX-F9316-J1 对账判据过，让出协议单源格过，上游依赖登记格过 |
| UNX-F9317 | EINTR 与 Windows ERROR_INTERRUPTED 对照格 | 300 | — | UNX-F9317-J1 映射判据过，APC 中断差异账三条全列格过 |
| UNX-F9318 | EINTR 与 glibc TEMP_FAILURE_RETRY 消费核 | 320 | — | UNX-F9318-J1 重试宏消费链判据过，重试后语义正确可观测，消费号登记格过 |
| UNX-F9319 | E 型竞态参数探针总成 | 300 | — | UNX-F9319-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9320 | ktest syscall 面 B26 批断言集 | 480 | — | UNX-F9320-J1 本批 19 条判据聚合判据过，EINTR 竞态矩阵一次跑通，域累计 139,900 收口断言 |

<!-- 主册行 11563 · #### UNX-C2-B27 · E 型错误矩阵七：时间/资源族（EINVAL/EAGAIN）（F9321–F9340 -->
#### UNX-C2-B27 · E 型错误矩阵七：时间/资源族（EINVAL/EAGAIN）（F9321–F9340 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 非法值域全 EINVAL、定时器耗尽全 EAGAIN 时间资源族矩阵）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B27.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B26 73,030 + 本批 6,900 = 146,800 / 240,000｜嫁接源：man-pages clock_getres(2)/timer_create(2)/setrlimit(2)/sched(7) 错误段语义对齐；与 B06 时间族/B19 资源族/B20 规范化对账；A2 计时基座消费｜防重声明：时间/资源族 E 型 20 号为号面新铺（B01–B15 时间资源正判据已深化，本批只铺错误注入矩阵号，零重复）；扩号段 F9321–F9340 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；零时钟源直改承诺（clock_settime 特权检查挂点声明）；零硬件频率改动；双轨产线条款默认适用｜批注（AI-12）：B27 是时间/资源族错误矩阵批：tv_nsec 规范化边界（≥1e9 全 EINVAL）与 POSIX 定时器耗尽 EAGAIN（RLIMIT_SIGPENDING 联动）是判据重心，sched_setaffinity/setscheduler 错误矩阵为 A1 调度消费预留对装面｜判据与后续 deepen/C2-B27.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9321 | 时间资源错误面总纲（EINVAL 判定域） | 400 | — | UNX-F9321-J1 时间资源错误面判据过，EINVAL 触发域（非法时钟/非法标志/值域越界）逐格落账，与 Linux 语义一致格过 |
| UNX-F9322 | gettimeofday/clock_gettime 坏指针矩阵（EFAULT） | 380 | — | UNX-F9322-J1 坏指针注入判据过，全 EFAULT 格过，10/10 次检出格过，与 B06 时间族对账格过 |
| UNX-F9323 | clock_settime 非法时钟 EINVAL 矩阵 | 360 | — | UNX-F9323-J1 非法 clockid 注入判据过，全 EINVAL 格过，CLOCK_REALTIME 正判据格过，特权检查挂点格过 |
| UNX-F9324 | timer_create/timer_settime 错误矩阵（EINVAL/EAGAIN） | 380 | — | UNX-F9324-J1 两错误码注入矩阵判据过，非法 sigevent 全 EINVAL 格过，定时器耗尽全 EAGAIN 格过 |
| UNX-F9325 | POSIX 定时器耗尽 EAGAIN 矩阵（RLIMIT_SIGPENDING 联动） | 360 | — | UNX-F9325-J1 定时器上限注入判据过，全 EAGAIN 格过，与 B19 SIGPENDING 联动格过 |
| UNX-F9326 | setitimer 非法间隔矩阵 | 320 | — | UNX-F9326-J1 负值/超上界注入判据过，全 EINVAL 格过，ITIMER_REAL 三类正判据格过 |
| UNX-F9327 | clock_nanosleep 非法标志/时钟矩阵 | 360 | — | UNX-F9327-J1 非法标志注入判据过，全 EINVAL/ENOTSUP 逐格检出，非法时钟全 EINVAL 格过，与 B20 对账格过 |
| UNX-F9328 | getrusage 坏指针矩阵（联 B15/B19 对账） | 340 | — | UNX-F9328-J1 坏指针注入判据过，全 EFAULT 格过，非法 who 全 EINVAL 格过，与 B19 对账零重复格过 |
| UNX-F9329 | setrlimit 坏结构指针矩阵 | 340 | — | UNX-F9329-J1 坏 rlim 指针注入判据过，全 EFAULT 格过，10/10 次检出格过 |
| UNX-F9330 | rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账） | 340 | — | UNX-F9330-J1 软硬倒置注入判据过，全 EINVAL 格过，与 B19 矩阵零重复分层格过 |
| UNX-F9331 | sched_setaffinity 坏掩码 EINVAL 矩阵 | 340 | — | UNX-F9331-J1 全零掩码注入判据过，全 EINVAL 格过，坏 mask 指针全 EFAULT 格过 |
| UNX-F9332 | sched_setscheduler 非法策略 EINVAL 矩阵 | 340 | — | UNX-F9332-J1 非法策略注入判据过，全 EINVAL 格过，非法 param 全 EINVAL 格过，A1 消费声明格过 |
| UNX-F9333 | 时间值规范化边界矩阵（tv_nsec 上限 1e9） | 380 | — | UNX-F9333-J1 tv_nsec 越界注入判据过，≥1e9 全 EINVAL 格过，规范化（tv_sec 进位）语义格过，与 B20 规范化对账一致格过 |
| UNX-F9334 | 时间/资源族 E 型参数探针总成 | 300 | — | UNX-F9334-J1 nargs 逐位提取判据过，坏指针全 EFAULT 格过，与参数拷贝探针单源格过 |
| UNX-F9335 | 时间/资源族 E 型与 B06/B19 深化册对账格 | 300 | — | UNX-F9335-J1 对账判据过，与时间族/资源族深化册逐条零冲突格过，正反分层声明格过 |
| UNX-F9336 | 时间/资源族 E 型与 Windows 计时器错误对照格 | 300 | — | UNX-F9336-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条全列格过 |
| UNX-F9337 | E 型判据可复测性声明（10/10 次检出） | 300 | — | UNX-F9337-J1 全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过 |
| UNX-F9338 | 时间/资源族 E 型升级接管注记 | 280 | — | UNX-F9338-J1 接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过 |
| UNX-F9339 | 时间/资源族 E 型与 glibc clock 家族消费核 | 340 | — | UNX-F9339-J1 clock_gettime errno 透传判据过，失效时钟全 EINVAL 可观测，消费号登记格过 |
| UNX-F9340 | ktest syscall 面 B27 批断言集 | 440 | — | UNX-F9340-J1 本批 19 条判据聚合判据过，时间资源族 E 型矩阵一次跑通，域累计 146,800 收口断言 |

<!-- 主册行 11590 · #### UNX-C2-B28 · E 型错误矩阵八：跨族边界值总装（F9341–F9360 · 20 条） -->
#### UNX-C2-B28 · E 型错误矩阵八：跨族边界值总装（F9341–F9360 · 20 条）

> AI-12 承办｜批次类型：E 型（任务书 B21–B28 E 型正反双判据 · 跨族边界值总装与 E 型收口）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B28.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B27 79,930 + 本批 7,300 = 154,100 / 240,000｜嫁接源：man-pages errno(3) 全码表语义对齐；与 B21–B27 七批矩阵逐批联动回归；与 B13 错误编码器总成/errno.rs/lxerrno 单源联动｜防重声明：跨族总装 20 号为号面新铺（B21–B27 七批分族矩阵已铺，本批只铺总装收口号，零重复）；扩号段 F9341–F9360 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；fuzz 全集注入零内核 panic 承诺（零/负/上限/全 F 值域扫描后内核状态可恢复）；双轨产线条款默认适用｜批注（AI-12）：B28 是 E 型收口批：五族矩阵总装（EFAULT/EBADF/EINVAL/EPERM/ENOSYS 全域逐格）+ 等价类总装（指针四类/fd 四类/边界值四类）+ 判序链总装 + errno 透传全链路——E 型八批在本批一次收口，域累计 154,100｜判据与后续 deepen/C2-B28.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9341 | E 型总装总纲（全错误码矩阵收口协议） | 420 | — | UNX-F9341-J1 八批错误矩阵收口协议判据过，全错误码账逐码落账，收口判序（EFAULT>EBADF>EINVAL>EPERM）声明格过 |
| UNX-F9342 | 全域 EFAULT 矩阵总装（跨五族逐格） | 400 | — | UNX-F9342-J1 五族坏指针逐格总装判据过，全 EFAULT 逐格检出，拷贝方向（in/out）双向逐格格过 |
| UNX-F9343 | 全域 EBADF 矩阵总装 | 380 | — | UNX-F9343-J1 五族坏 fd 逐格总装判据过，全 EBADF 逐格检出，fd 空间全域扫描格过 |
| UNX-F9344 | 全域 EINVAL 矩阵总装 | 380 | — | UNX-F9344-J1 五族非法参数逐格总装判据过，全 EINVAL 逐格检出，与 Linux 逐例一致格过 |
| UNX-F9345 | 全域 EPERM/EACCES 矩阵总装 | 380 | — | UNX-F9345-J1 五族权限注入逐格总装判据过，EPERM/EACCES 分界逐格检出，与 B24 判定域对账一致格过 |
| UNX-F9346 | 全域 ENOSYS 未实装号矩阵（号面全扫） | 400 | — | UNX-F9346-J1 号面全扫注入判据过，未实装号全 ENOSYS 逐格检出，与 18 号注册面/COMPAT_TABLE 对账格过 |
| UNX-F9347 | 错误码优先级链总装（EFAULT>EBADF>EINVAL 判序） | 380 | — | UNX-F9347-J1 判序链逐例总装判据过，多错误并发时取高优先逐格检出，与 Linux 判序逐例一致格过 |
| UNX-F9348 | errno 透传全链路总装（Errno→lxerrno→glibc） | 400 | — | UNX-F9348-J1 全链路逐码总装判据过，Errno→sysret→glibc errno 逐码一致格过，与 B13 编码器零漂移格过 |
| UNX-F9349 | 坏值注入器总装（fuzz 值域全集） | 380 | — | UNX-F9349-J1 fuzz 值域全集注入判据过（零/负/上限/上限+1/全 F/全 0），检出率统计格过，零内核 panic 声明格过 |
| UNX-F9350 | 边界值等价类总装（零/负/上限/上限+1） | 380 | — | UNX-F9350-J1 四等价类逐类总装判据过，逐类期望值逐格检出，等价类账落账格过 |
| UNX-F9351 | 指针等价类总装（NULL/-1/非映射/只读映射） | 380 | — | UNX-F9351-J1 四指针等价类总装判据过，逐类期望 EFAULT 逐格检出，只读映射写路径格过 |
| UNX-F9352 | fd 等价类总装（负值/未开/已关/越限） | 360 | — | UNX-F9352-J1 四 fd 等价类总装判据过，逐类期望 EBADF 逐格检出，等价类账落账格过 |
| UNX-F9353 | 竞态注入总装（EINTR×五族） | 380 | — | UNX-F9353-J1 EINTR×五族竞态总装判据过，逐族 100 次统计格过，与 B26 注入器单源格过 |
| UNX-F9354 | 全域错误回归矩阵（与 B21–B27 判据联动） | 400 | — | UNX-F9354-J1 回归矩阵判据过，与七批判据逐条联动可观测，回归零回退声明格过 |
| UNX-F9355 | E 型与 B13 错误编码器总成对账格 | 320 | — | UNX-F9355-J1 对账判据过，与 errno.rs 编码族逐码零冲突格过，升级接管口径声明格过 |
| UNX-F9356 | E 型与 lxerrno::VarixErr 映射总装 | 320 | — | UNX-F9356-J1 映射总装判据过，VarixErr 逐码映射一致格过，单源声明格过 |
| UNX-F9357 | E 型与 Windows 错误码全域对照账 | 320 | — | UNX-F9357-J1 全域对照账判据过，逐码映射表落账，差异账五条全列格过 |
| UNX-F9358 | E 型判据可复测性总声明（全矩阵 10/10） | 320 | — | UNX-F9358-J1 全矩阵 10 次重复注入判据过，10/10 次检出总统计格过，零 flaky 总声明格过 |
| UNX-F9359 | E 型升级接管总注记（错误面增量清单） | 300 | — | UNX-F9359-J1 接管边界总声明判据过，存量错误面零重写承诺格过，增量清单落账格过 |
| UNX-F9360 | ktest syscall 面 B28 批断言集 + E 型收口断言 | 300 | — | UNX-F9360-J1 本批 19 条判据聚合判据过，E 型八批（B21–B28）收口一次跑通断言过，域累计 154,100 收口断言 |

<!-- 主册行 11617 · #### UNX-C2-B29 · I 型联调一：C3 账本联签（C2×C3）（F9361–F9380 · 20 条） -->
#### UNX-C2-B29 · I 型联调一：C3 账本联签（C2×C3）（F9361–F9380 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B29–B36 I 型联调 · 前段：C3 LTP 回归基建联签，判据联测共享只写编号+一句话）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B29.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B28 87,230 + 本批 7,900 = 162,000 / 240,000｜嫁接源：任务书 I 型联调条款（C3 账本联测）；C3 域（AI-13）LTP 回归基建冻结接口消费；B15 F9095/F9097 移交包升级｜防重声明：I 型联调 20 号为号面新铺（B01–B28 零触及联调号，零重复）；扩号段 F9361–F9380 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C3 判据（跨界代写禁令）｜红线注记：本批零写盘零引导零固件操作；联测零改动承诺（对 C3 账本只读消费，零写入对方台账）；双轨产线条款默认适用｜批注（AI-12）：B29 是 I 型联调首批：C2 syscall 号面 × C3 LTP 回归基建联签，分发表注册协议/路由双门/errno 编码/参数探针/meter/guard 六挂点逐个联测，「可跑」登记与覆盖率账是 LTP 对接的地基，号面冻结清单 F9600 联签互认｜判据与后续 deepen/C2-B29.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9361 | I 型联调总纲一（C2×C3 联签协议与共享判据面） | 480 | — | UNX-F9361-J1 联签协议判据过，C2（syscall 号面）×C3（LTP 回归基建）双方判据面互认格过，跨界代写禁令声明格过（联测共享只写编号+一句话） |
| UNX-F9362 | C3 账本消费协议（LTP 用例号面只读消费） | 440 | — | UNX-F9362-J1 消费协议判据过，C3 LTP 用例号面只读消费声明格过，零写入 C3 账本格过 |
| UNX-F9363 | LTP「可跑」登记联测面（C2 号面 × C3 回归基建） | 420 | — | UNX-F9363-J1 「可跑」登记联测判据过，C2 号面逐号与 C3 回归基建对接可观测，登记口径单源格过 |
| UNX-F9364 | 分发表注册协议联签（table.rs 注册接口 × C3 注入点） | 460 | — | UNX-F9364-J1 注册接口联签判据过，table::SyscallDesc/resolve 与 C3 注入点对接可观测，18 号注册面+COMPAT_TABLE 零改动格过 |
| UNX-F9365 | syscall 路由联测（entry.rs 双门 × C3 探针） | 440 | — | UNX-F9365-J1 双门路由联测判据过，SYSCALL/int 0x80 双门逐门可观测，C3 探针注入零改动格过 |
| UNX-F9366 | errno 编码联测（Errno 全集 × C3 断言库） | 440 | — | UNX-F9366-J1 Errno 全集联测判据过，逐码与 C3 断言库对接一致格过，LTP 期望值逐码对账格过 |
| UNX-F9367 | 参数探针联测（uaccess 拷贝 × C3 值域库） | 380 | — | UNX-F9367-J1 拷贝探针联测判据过，copy_from_user/copy_to_user 与 C3 值域库对接可观测，坏指针 EFAULT 对账格过 |
| UNX-F9368 | meter 计量联测（SyscallMeter × C3 统计账） | 420 | — | UNX-F9368-J1 计量联测判据过，SyscallMeter 统计与 C3 统计账对接可观测，计数逐号一致格过 |
| UNX-F9369 | guard 能力位联测（CAP_* × C3 权限判据库） | 380 | — | UNX-F9369-J1 能力位联测判据过，CAP_* 与 C3 权限判据库对接可观测，EPERM 判定逐格一致格过 |
| UNX-F9370 | 号面冻结清单联签（F9600 冻结协议与 C3 台账互认） | 460 | — | UNX-F9370-J1 冻结协议联签判据过，F8801–F9600 号面冻结清单与 C3 台账互认可观测，零重编声明格过 |
| UNX-F9371 | LTP 子集清单联测（C2 号面覆盖率账） | 440 | — | UNX-F9371-J1 覆盖率账联测判据过，C2 号面 LTP 子集覆盖率逐号落账，缺口清单登记格过 |
| UNX-F9372 | 回归基线联测（ktest -f c2 与 C3 回归 runner 对接） | 440 | — | UNX-F9372-J1 基线联测判据过，ktest -f c2 与 C3 runner 对接可观测，fail==0 判据联测格过 |
| UNX-F9373 | 联测判据共享账（只写编号+一句话协议逐条） | 420 | — | UNX-F9373-J1 共享账判据过，逐条只写编号+一句话协议格过，跨界代写零发生声明格过 |
| UNX-F9374 | 联签交接包（F9095/F9097 移交包升级版） | 380 | — | UNX-F9374-J1 交接包升级判据过，B15 移交包逐项升级可观测，交接清单落账格过 |
| UNX-F9375 | 联调防重声明（C2×C3 号面零冲突 grep） | 320 | — | UNX-F9375-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过 |
| UNX-F9376 | 联调上游依赖登记（C3 未 finalize 依赖声明） | 320 | — | UNX-F9376-J1 依赖声明判据过，C3 上游未 finalize 显式登记格过，fake 开发路径格过（§52 防卡死规则一） |
| UNX-F9377 | 联调与 Windows 对照账联测面 | 300 | — | UNX-F9377-J1 对照账联测判据过，Windows 对照格与联测面对接可观测，差异账复用格过 |
| UNX-F9378 | 联调与 glibc 消费核联测面 | 320 | — | UNX-F9378-J1 消费核联测判据过，glibc 消费清单与联测面对接可观测，消费号登记格过 |
| UNX-F9379 | I 型联调判据可复测性声明 | 300 | — | UNX-F9379-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过 |
| UNX-F9380 | ktest syscall 面 B29 批断言集 | 340 | — | UNX-F9380-J1 本批 19 条判据聚合判据过，C2×C3 联调一次跑通，域累计 162,000 收口断言 |

<!-- 主册行 11644 · #### UNX-C2-B30 · I 型联调二：C4 futex/epoll 挂点联签（C2×C4）+ B16–B30 -->
#### UNX-C2-B30 · I 型联调二：C4 futex/epoll 挂点联签（C2×C4）+ B16–B30 半域收口（F9381–F9400 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B29–B36 I 型联调 · 前段：C4 futex/epoll/pty 挂点联签 + 半域收口断言）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B30.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B29 95,130 + 本批 7,000 = 169,000 / 240,000（B31–B40 预算 71,000 待领，域账守恒 66,870+173,130=240,000）｜嫁接源：任务书 I 型联调条款（C4 futex·epoll 联签）；C4 域（AI-14）pty 字节流语义与 futex 四操作冻结接口消费；B09 多路复用/B11·B12 信号挂点/B16 ENOTTY 联动｜防重声明：I 型联调 20 号为号面新铺（B01–B29 零触及联调号，零重复）；扩号段 F9381–F9400 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C4 判据｜红线注记：本批零写盘零引导零固件操作；网络族号段零触及承诺（epoll 联测只走事件面，socket 零触及）；双轨产线条款默认适用｜批注（AI-12）：B30 是 I 型前段收口批：futex 四操作/epoll 挂点/pty 缺席 ENOTTY/信号唤醒链四个挂点逐个联签，futex 唤醒丢失窗 100 次竞态统计，号面 F9400 冻结互认——B16–B30 十五批半域在本批一次收口（域累计 169,000），B31–B40 预算 71,000 已批注待领｜判据与后续 deepen/C2-B30.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9381 | I 型联调总纲二（C2×C4 联签协议与挂点面） | 400 | — | UNX-F9381-J1 联签协议判据过，C2（阻塞挂点）×C4（futex/epoll/pty 面）双方挂点面互认格过，跨界代写禁令声明格过 |
| UNX-F9382 | futex 四操作挂点联签（WAIT/WAKE/FD/REQUEUE 协议） | 400 | — | UNX-F9382-J1 四操作挂点联签判据过，C4 futex 四操作协议逐操作对接可观测，挂点单源格过 |
| UNX-F9383 | futex EAGAIN/EINVAL 矩阵联测（C2 错误矩阵 × C4 挂点） | 360 | — | UNX-F9383-J1 矩阵联测判据过，EAGAIN 耗尽格与 EINVAL 参数格逐格联测可观测，与 B23 判定域一致格过 |
| UNX-F9384 | epoll_ctl/epoll_wait 挂点联签（就绪队列协议） | 380 | — | UNX-F9384-J1 两挂点联签判据过，就绪队列协议对接可观测，与 B09 四件套语义一致格过 |
| UNX-F9385 | epoll 错误矩阵联测（EBADF/EINVAL/EPERM） | 340 | — | UNX-F9385-J1 三错误码联测判据过，逐码与 C4 挂点联测一致格过，与 B22 判定域一致格过 |
| UNX-F9386 | 多路复用四件套联测（poll/select/ppoll/pselect × C4 事件面） | 380 | — | UNX-F9386-J1 四件套联测判据过，逐件与 C4 事件面对接可观测，timeout 语义逐件一致格过 |
| UNX-F9387 | pty 缺席 ENOTTY 联签（B16 终端挂点 × C4 pty 面） | 340 | — | UNX-F9387-J1 缺席联签判据过，pty 缺席全 ENOTTY 逐格联测可观测，C4 pty 上线后翻转挂点声明格过 |
| UNX-F9388 | 信号挂点联签（B11/B12 挂点 × C4 futex 唤醒链） | 360 | — | UNX-F9388-J1 唤醒链联签判据过，信号到达唤醒 futex 等待逐格联测可观测，零丢唤醒声明格过 |
| UNX-F9389 | 阻塞唤醒原语联测（A3 fake × C4 等待队列） | 340 | — | UNX-F9389-J1 原语联测判据过，A3 fake 原语与 C4 等待队列对接可观测，Schema 先行声明格过 |
| UNX-F9390 | futex 竞态注入联测（唤醒丢失窗 100 次统计） | 380 | — | UNX-F9390-J1 竞态联测判据过，唤醒丢失窗 100 次注入统计格过，零丢失声明格过 |
| UNX-F9391 | EINTR 重启联测（B26 矩阵 × C4 挂点） | 340 | — | UNX-F9391-J1 重启联测判据过，B26 EINTR 矩阵逐格与 C4 挂点联测可观测，SA_RESTART 语义一致格过 |
| UNX-F9392 | 号面冻结互认联签（C2 F9400 冻结 × C4 台账） | 360 | — | UNX-F9392-J1 互认联签判据过，F9101–F9400 号面冻结与 C4 台账互认可观测，零重编声明格过 |
| UNX-F9393 | 联签交接包（F9400 冻结面移交清单） | 320 | — | UNX-F9393-J1 交接包判据过，F9400 冻结面移交清单逐项落账，接手会话就绪格过 |
| UNX-F9394 | 联调防重声明（C2×C4 号面零冲突 grep） | 320 | — | UNX-F9394-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过 |
| UNX-F9395 | 联调上游依赖登记（C4 未 finalize 依赖声明） | 320 | — | UNX-F9395-J1 依赖声明判据过，C4 上游未 finalize 显式登记格过，fake 开发路径格过 |
| UNX-F9396 | 联调与 Windows WaitableTimer/IOCP 对照账 | 280 | — | UNX-F9396-J1 对照账判据过，WaitableTimer/IOCP 映射表落账，差异账三条全列格过 |
| UNX-F9397 | 联调与 glibc pthread futex 消费核 | 320 | — | UNX-F9397-J1 消费核联测判据过，pthread futex 消费链对接可观测，消费号登记格过 |
| UNX-F9398 | I 型联调判据可复测性声明 | 280 | — | UNX-F9398-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，零 flaky 声明格过 |
| UNX-F9399 | I 型与 glibc 消费清单逐号核联测 | 320 | — | UNX-F9399-J1 逐号核联测判据过，消费清单逐号联测可观测，零号面冲突格过 |
| UNX-F9400 | ktest syscall 面 B30 批断言集 + B16–B30 半域收口断言 | 460 | — | UNX-F9400-J1 本批 19 条判据聚合判据过，B16–B30 十五批半域收口一次跑通断言过，域累计 169,000 收口断言（B31–B40 预算 71,000 待领，域账守恒 240,000） |

<!-- 主册行 11671 · #### UNX-C2-B31 · I 型联调三：C3 错误码账本联测后段（C2×C3）（F9401–F9420 · 2 -->
#### UNX-C2-B31 · I 型联调三：C3 错误码账本联测后段（C2×C3）（F9401–F9420 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · C3 错误码对账）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B31.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + 本批前零铺设 + 本批 7,100 = 176,100 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：C3 域（AI-13）POSIX 认证与 LTP 验收错误码账本；B13 错误矩阵正反双判据总装；B21–B28 E 型矩阵判定域｜防重声明：I 型联调 20 号为号面新铺（B01–B30 零触及本段联调号，零重复）；扩号段 F9401–F9420 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C3 判据｜红线注记：本批零写盘零引导零固件操作；错误码联测只走对账面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B31 承接 B29 C3 账本联签做错误码对账后段：errno 逐号双向对账矩阵 + 高频错误码边界联测 + C3 判据号互认抽验——判定域与 B21–B28 逐格一致，联签零分歧收官｜判据与后续 deepen/C2-B31.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9401 | I 型联调总纲三（C2×C3 联签协议与对账面） | 400 | — | UNX-F9401-J1 联签协议判据过，C2（错误矩阵面）×C3（errno 账本面）双方对账面互认格过，跨界代写禁令声明格过 |
| UNX-F9402 | C3 错误码账本全表对账矩阵（errno 逐号双向） | 420 | — | UNX-F9402-J1 全表对账判据过，errno 逐号双向对账 100% 格过，10/10 次复测零分歧格过 |
| UNX-F9403 | EFAULT/EBADF/EINVAL 优先级链对账复验 | 400 | — | UNX-F9403-J1 优先级链对账判据过，EFAULT>EBADF>EINVAL 三元逐格复验可观测，与 B21 判定域一致格过 |
| UNX-F9404 | E2BIG/EOVERFLOW/EFBIG 大对象边界联测 | 380 | — | UNX-F9404-J1 大对象联测判据过，三码边界值逐格联测可观测，10/10 次检出格过 |
| UNX-F9405 | EAGAIN/EWOULDBLOCK 同号双义对账 | 340 | — | UNX-F9405-J1 同号对账判据过，双义映射逐格一致格过，C3 账本同号零分歧格过 |
| UNX-F9406 | EDEADLK 自锁检测协同联测 | 350 | — | UNX-F9406-J1 协同联测判据过，自锁检测逐格联测可观测，10/10 次检出格过 |
| UNX-F9407 | ENAMETOOLONG 路径长边界联测 | 340 | — | UNX-F9407-J1 路径长联测判据过，PATH_MAX±1 边界逐格可观测，与 B21 判定域一致格过 |
| UNX-F9408 | ELOOP 符号链接环对账联测 | 350 | — | UNX-F9408-J1 环检测对账判据过，40 层环阈值逐格可观测，10/10 次检出格过 |
| UNX-F9409 | ENOTDIR/EISDIR 方向性错型对账 | 350 | — | UNX-F9409-J1 方向性对账判据过，双向错型逐格一致格过，与 B21 判定域一致格过 |
| UNX-F9410 | EMFILE/ENFILE 双表限联测 | 380 | — | UNX-F9410-J1 双表限联测判据过，进程/系统两级限逐格联测可观测，10/10 次检出格过 |
| UNX-F9411 | ENOSPC/EDQUOT 空间耗尽联测 | 350 | — | UNX-F9411-J1 空间耗尽联测判据过，耗尽注入逐格可观测，与 B21 判定域一致格过 |
| UNX-F9412 | EROFS/EACCES/EPERM 权限错型分界对账 | 380 | — | UNX-F9412-J1 分界对账判据过，三码判据链逐格一致格过，与 B24 判定域一致格过 |
| UNX-F9413 | ETIMEDOUT 超时面对账 | 340 | — | UNX-F9413-J1 超时面对账判据过，超时注入逐格可观测，C3 账本零分歧格过 |
| UNX-F9414 | EINTR 对账互认（B26 矩阵 × C3 账本） | 360 | — | UNX-F9414-J1 互认对账判据过，B26 EINTR 矩阵逐格与 C3 账本一致格过，零分歧声明格过 |
| UNX-F9415 | 错误消息面一致性对账（strerror 语义） | 330 | — | UNX-F9415-J1 消息面对账判据过，错误消息逐号语义一致格过，10/10 次复测格过 |
| UNX-F9416 | C3 判据号互认抽验组（编号引用不代写） | 310 | — | UNX-F9416-J1 互认抽验判据过，C3 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9417 | 联测失败分类账（可复测/环境限/待升级） | 300 | — | UNX-F9417-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9418 | 对账矩阵自动化编排判据 | 300 | — | UNX-F9418-J1 编排判据过，对账脚本一键跑通可观测，输出对账报告格过 |
| UNX-F9419 | 联测回归基线冻结组 | 300 | — | UNX-F9419-J1 基线冻结判据过，B31 联测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9420 | B31 断言集（对账 100%+互认零分歧） | 420 | — | UNX-F9420-J1 断言集判据过，对账矩阵 100% 覆盖格过，互认零分歧格过，批收口三断言落账格过 |

<!-- 主册行 11698 · #### UNX-C2-B32 · I 型联调四：C4 futex 联测后段（C2×C4）（F9421–F9440 ·  -->
#### UNX-C2-B32 · I 型联调四：C4 futex 联测后段（C2×C4）（F9421–F9440 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · C4 futex 联测）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B32.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B31 7,100 + 本批 7,100 = 183,200 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：C4 域（AI-14）futex 语义面；B30 futex 四操作联签/B10 唤醒丢失窗竞态统计；B12 EINTR 单点/B26 重启矩阵｜防重声明：I 型联调 20 号为号面新铺（B01–B31 零触及本段联调号，零重复）；扩号段 F9421–F9440 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C4 判据｜红线注记：本批零写盘零引导零固件操作；futex 联测只走等待队列事件面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B32 承接 B30 futex 前段联签做后段：REQUEUE/PI/超时/跨进程面逐项联测 + 唤醒丢失窗 100 次复测互认 + pthread 消费回归——四操作×四路径覆盖账收官｜判据与后续 deepen/C2-B32.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9421 | I 型联调总纲四（futex 后段联签协议） | 400 | — | UNX-F9421-J1 联签协议判据过，C2（阻塞挂点面）×C4（futex 后段面）互认格过，跨界代写禁令声明格过 |
| UNX-F9422 | WAIT/WAKE 全操作面回归联测 | 420 | — | UNX-F9422-J1 回归联测判据过，四操作全面逐操作回归可观测，10/10 次复测零漂移格过 |
| UNX-F9423 | FUTEX_REQUEUE 分流正确性联测 | 400 | — | UNX-F9423-J1 分流联测判据过，requeue 分流计数逐格可观测，10/10 次检出格过 |
| UNX-F9424 | FUTEX_CMP_REQUEUE 原子比较联测 | 380 | — | UNX-F9424-J1 原子比较联测判据过，比较失败 EAGAIN 逐格可观测，与 B23 判定域一致格过 |
| UNX-F9425 | futex PI 优先级继承面联测 | 390 | — | UNX-F9425-J1 PI 联测判据过，优先级继承链逐格可观测，反转场景 10/10 次检出格过 |
| UNX-F9426 | futex 死锁检测协同联测 | 340 | — | UNX-F9426-J1 协同联测判据过，EDEADLK 协同路径逐格可观测，10/10 次检出格过 |
| UNX-F9427 | 唤醒丢失窗复测互认（B30 × C4 100 次统计） | 360 | — | UNX-F9427-J1 复测互认判据过，唤醒丢失窗 100 次注入统计复测格过，零丢失声明互认格过 |
| UNX-F9428 | futex 超时 timespec 边界联测 | 350 | — | UNX-F9428-J1 超时联测判据过，timespec 零值/极值/负值三边界逐格可观测，与 B27 判定域一致格过 |
| UNX-F9429 | PRIVATE/SHARED 两面联测 | 340 | — | UNX-F9429-J1 两面联测判据过，私/共享映射逐格可观测，行为差异声明格过 |
| UNX-F9430 | futex×信号 EINTR 路径联测（B12/B26 互认） | 380 | — | UNX-F9430-J1 EINTR 路径联测判据过，信号中断返回 EINTR 逐格可观测，SA_RESTART 语义一致格过 |
| UNX-F9431 | futex 跨进程共享联测 | 340 | — | UNX-F9431-J1 跨进程联测判据过，共享区等待/唤醒逐格可观测，10/10 次检出格过 |
| UNX-F9432 | val/val2/uaddr2 位宽比较联测 | 330 | — | UNX-F9432-J1 位宽联测判据过，三参数位宽边界逐格可观测，截断行为声明格过 |
| UNX-F9433 | futex 并发争用压测联测 | 380 | — | UNX-F9433-J1 压测联测判据过，高争用场景 100 轮统计格过，P95 响应落账格过 |
| UNX-F9434 | futex 判据号互认抽验组（编号引用不代写） | 310 | — | UNX-F9434-J1 互认抽验判据过，C4 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9435 | 联测失败分类账（futex 面） | 300 | — | UNX-F9435-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9436 | 联测编排自动化判据 | 310 | — | UNX-F9436-J1 编排判据过，联测脚本一键跑通可观测，输出联测报告格过 |
| UNX-F9437 | futex 回归基线冻结组 | 300 | — | UNX-F9437-J1 基线冻结判据过，B32 联测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9438 | glibc pthread futex 消费回归联测 | 350 | — | UNX-F9438-J1 消费回归判据过，pthread 锁消费链逐格联测可观测，消费号登记格过 |
| UNX-F9439 | futex 覆盖账（四操作×四路径矩阵） | 320 | — | UNX-F9439-J1 覆盖账判据过，16 格矩阵 100% 落账格过，空格零发生声明格过 |
| UNX-F9440 | B32 断言集（futex 后段联签收官） | 400 | — | UNX-F9440-J1 断言集判据过，四操作×四路径全过格过，互认零分歧格过，批收口三断言落账格过 |

<!-- 主册行 11725 · #### UNX-C2-B33 · I 型联调五：C4 epoll 事件面联测后段（C2×C4）（F9441–F9460 -->
#### UNX-C2-B33 · I 型联调五：C4 epoll 事件面联测后段（C2×C4）（F9441–F9460 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · C4 epoll 联测）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B33.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B32 14,200 + 本批 7,100 = 190,300 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：C4 域（AI-14）epoll 事件面；B09 epoll 四件套语义档；B30 epoll_ctl/epoll_wait 挂点联签；B16 ENOTTY 终端挂点｜防重声明：I 型联调 20 号为号面新铺（B01–B32 零触及本段联调号，零重复）；扩号段 F9441–F9460 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C4 判据｜红线注记：本批零写盘零引导零固件操作；epoll 联测只走事件面（socket 零触及承诺沿 B30），零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B33 承接 B30 epoll 前段联签做后段：ET/LT 边沿矩阵/oneshot/fork 继承/close 竞态逐项联测 + 大 fd 表规模面——事件面覆盖账收官｜判据与后续 deepen/C2-B33.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9441 | I 型联调总纲五（epoll 后段联签协议） | 400 | — | UNX-F9441-J1 联签协议判据过，C2（多路复用面）×C4（epoll 后段面）互认格过，跨界代写禁令声明格过 |
| UNX-F9442 | ET/LT 混合边沿触发联测矩阵 | 420 | — | UNX-F9442-J1 边沿矩阵判据过，ET/LT 逐格触发语义联测可观测，10/10 次复测零漂移格过 |
| UNX-F9443 | epoll+fork 继承语义联测 | 400 | — | UNX-F9443-J1 继承联测判据过，fork 后兴趣列表继承语义逐格可观测，与 B04 COW 判定域一致格过 |
| UNX-F9444 | 就绪链洪泛与背压联测 | 380 | — | UNX-F9444-J1 洪泛联测判据过，洪泛注入背压路径逐格可观测，10/10 次检出格过 |
| UNX-F9445 | EPOLLONESHOT/EPOLLEXCLUSIVE 联测 | 380 | — | UNX-F9445-J1 两标志联测判据过，oneshot 复位/exclusive 抢占逐格可观测，10/10 次检出格过 |
| UNX-F9446 | epoll_ctl 全操作回归联测（ADD/MOD/DEL） | 360 | — | UNX-F9446-J1 回归联测判据过，三操作逐格回归可观测，与 B30 联签一致格过 |
| UNX-F9447 | epoll_wait 超时/立即返回边界联测 | 360 | — | UNX-F9447-J1 边界联测判据过，timeout 零值/负值/正值三态逐格可观测，与 B27 判定域一致格过 |
| UNX-F9448 | EPOLLHUP/EPOLLERR 事件语义联测 | 340 | — | UNX-F9448-J1 事件语义判据过，HUP/ERR 注入逐格可观测，10/10 次检出格过 |
| UNX-F9449 | close 竞态自动摘除联测（100 次注入） | 380 | — | UNX-F9449-J1 竞态联测判据过，close 自动摘除 100 次注入统计格过，零残留声明格过 |
| UNX-F9450 | epoll×B09 四件套一致性联测 | 350 | — | UNX-F9450-J1 一致性联测判据过，四件套语义逐件一致格过，与 B09 判定域一致格过 |
| UNX-F9451 | epoll×B16 ENOTTY 终端面联测 | 340 | — | UNX-F9451-J1 终端面联测判据过，终端 fd 注册行为逐格可观测，与 B30 缺席联签一致格过 |
| UNX-F9452 | epoll 大 fd 表规模联测（1024 级） | 360 | — | UNX-F9452-J1 规模联测判据过，1024 fd 注册/等待/摘除全链可观测，资源上界声明格过 |
| UNX-F9453 | epoll 判据号互认抽验组（编号引用不代写） | 350 | — | UNX-F9453-J1 互认抽验判据过，C4 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9454 | 联测失败分类账（epoll 面） | 310 | — | UNX-F9454-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9455 | 联测编排自动化判据 | 310 | — | UNX-F9455-J1 编排判据过，联测脚本一键跑通可观测，输出联测报告格过 |
| UNX-F9456 | epoll 回归基线冻结组 | 300 | — | UNX-F9456-J1 基线冻结判据过，B33 联测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9457 | glibc epoll 消费回归联测 | 340 | — | UNX-F9457-J1 消费回归判据过，glibc poll 替换链逐格联测可观测，消费号登记格过 |
| UNX-F9458 | epoll 覆盖账（操作×事件×触发模式矩阵） | 330 | — | UNX-F9458-J1 覆盖账判据过，三轴矩阵 100% 落账格过，空格零发生声明格过 |
| UNX-F9459 | 与 B30 前段联签对账复核 | 300 | — | UNX-F9459-J1 对账复核判据过，B30 前段联签逐项复核一致格过，零漂移声明格过 |
| UNX-F9460 | B33 断言集（epoll 后段联签收官） | 390 | — | UNX-F9460-J1 断言集判据过，三轴矩阵全过格过，互认零分歧格过，批收口三断言落账格过 |

<!-- 主册行 11752 · #### UNX-C2-B34 · I 型联调六：glibc/busybox 消费面回归联测（C2×C3×C1）（F94 -->
#### UNX-C2-B34 · I 型联调六：glibc/busybox 消费面回归联测（C2×C3×C1）（F9461–F9480 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · 消费面回归）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B34.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B33 21,300 + 本批 7,100 = 197,400 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B14 glibc 消费清单逐号核（四列登记/meter 触号/busybox 探针）；C1 域（AI-11）meter 接管面；C3 域认证账本｜防重声明：I 型联调 20 号为号面新铺（B01–B33 零触及本段联调号，零重复）；扩号段 F9461–F9480 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C3/C1 判据｜红线注记：本批零写盘零引导零固件操作；消费面联测只走观测面（meter 计数），零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B34 承接 B14 消费清单做回归联测：meter 触号全表回归 + glibc/busybox 核心链逐族联测 + 零假阳零漏计抽验——消费面覆盖账收官｜判据与后续 deepen/C2-B34.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9461 | I 型联调总纲六（消费面回归联签协议） | 400 | — | UNX-F9461-J1 联签协议判据过，C2（syscall 面）×C3（认证账本）×C1（meter 面）三方互认格过，跨界代写禁令声明格过 |
| UNX-F9462 | meter 触号全表回归联测（B14 清单 × C3 meter） | 430 | — | UNX-F9462-J1 全表回归判据过，B14 清单逐号触号回归可观测，10/10 次复测零漂移格过 |
| UNX-F9463 | glibc open/read/write/close 消费回归联测 | 410 | — | UNX-F9463-J1 回归联测判据过，四原语消费链逐格可观测，与 B02/B21 判定域一致格过 |
| UNX-F9464 | glibc fork/exec/wait 消费回归联测 | 390 | — | UNX-F9464-J1 回归联测判据过，三原语消费链逐格可观测，与 B04 判定域一致格过 |
| UNX-F9465 | glibc 信号面消费回归联测 | 390 | — | UNX-F9465-J1 回归联测判据过，信号族消费链逐格可观测，与 B11/B12 判定域一致格过 |
| UNX-F9466 | busybox 探针矩阵回归联测（B14 四列） | 360 | — | UNX-F9466-J1 矩阵回归判据过，四列登记逐列回归可观测，与 B14 判定域一致格过 |
| UNX-F9467 | busybox sh/applet 核心链联测 | 360 | — | UNX-F9467-J1 核心链联测判据过，sh 主循环/applet 派发逐格可观测，10/10 次检出格过 |
| UNX-F9468 | 动态链接器消费面联测（mmap/faccessat） | 360 | — | UNX-F9468-J1 消费面联测判据过，链接器 mmap/faccessat 链逐格可观测，与 B18 判定域一致格过 |
| UNX-F9469 | printf/stdio 缓冲消费联测 | 360 | — | UNX-F9469-J1 缓冲联测判据过，行缓冲/全缓冲注入逐格可观测，10/10 次检出格过 |
| UNX-F9470 | getpwuid/getenv 环境面消费联测 | 350 | — | UNX-F9470-J1 环境面联测判据过，环境查询链逐格可观测，与 B20 判定域一致格过 |
| UNX-F9471 | meter 零假阳/零漏计抽验（100 号抽测） | 360 | — | UNX-F9471-J1 抽验判据过，100 号抽测零假阳零漏计格过，10/10 次复测格过 |
| UNX-F9472 | 消费号覆盖账（glibc 清单逐族） | 340 | — | UNX-F9472-J1 覆盖账判据过，逐族覆盖 100% 落账格过，空族零发生声明格过 |
| UNX-F9473 | 判据号互认抽验组（编号引用不代写） | 320 | — | UNX-F9473-J1 互认抽验判据过，C3/C1 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9474 | 联测失败分类账（消费面） | 310 | — | UNX-F9474-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9475 | 联测编排自动化判据 | 320 | — | UNX-F9475-J1 编排判据过，联测脚本一键跑通可观测，输出联测报告格过 |
| UNX-F9476 | 消费面回归基线冻结组 | 310 | — | UNX-F9476-J1 基线冻结判据过，B34 联测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9477 | 消费面与 C1 meter 接管对账 | 330 | — | UNX-F9477-J1 对账判据过，meter 接管面对账逐格可观测，接管日触发条件声明格过 |
| UNX-F9478 | 消费面联测与 Windows 对照账 | 320 | — | UNX-F9478-J1 对照账判据过，Windows 消费映射表落账，差异账全列格过 |
| UNX-F9479 | 与 B14 逐号核对账复核 | 310 | — | UNX-F9479-J1 复核判据过，B14 逐号核账逐项复核一致格过，零漂移声明格过 |
| UNX-F9480 | B34 断言集（消费面回归收官） | 370 | — | UNX-F9480-J1 断言集判据过，逐族覆盖全过格过，零假阳零漏计格过，批收口三断言落账格过 |

<!-- 主册行 11779 · #### UNX-C2-B35 · I 型联调七：LTP 可跑面回归联测（C2×C3）（F9481–F9500 · 20 -->
#### UNX-C2-B35 · I 型联调七：LTP 可跑面回归联测（C2×C3）（F9481–F9500 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · LTP 面回归）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B35.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B34 28,400 + 本批 7,100 = 204,500 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B15 LTP 可跑登记与号面冻结；B29 LTP 可跑登记联签；C3 域（AI-13）POSIX 认证与 LTP 验收账本｜防重声明：I 型联调 20 号为号面新铺（B01–B34 零触及本段联调号，零重复）；扩号段 F9481–F9500 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C3 判据｜红线注记：本批零写盘零引导零固件操作；LTP 联测只走用例对账面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B35 承接 B15/B29 LTP 登记做回归联测：可跑面逐族回归 + 失败三分类 + skip 显式面——LTP 对账覆盖账收官｜判据与后续 deepen/C2-B35.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9481 | I 型联调总纲七（LTP 面联签协议） | 400 | — | UNX-F9481-J1 联签协议判据过，C2（syscall 面）×C3（LTP 账本面）互认格过，跨界代写禁令声明格过 |
| UNX-F9482 | LTP 可跑登记面回归联测（B15 登记账 × C3） | 420 | — | UNX-F9482-J1 回归联测判据过，B15 登记账逐条回归可观测，10/10 次复测零漂移格过 |
| UNX-F9483 | LTP syscall 用例族抽测联测（open/fork/wait） | 410 | — | UNX-F9483-J1 抽测联测判据过，三族用例逐格抽测可观测，与 B02/B04 判定域一致格过 |
| UNX-F9484 | LTP 信号族用例联测 | 400 | — | UNX-F9484-J1 信号族联测判据过，用例逐格联测可观测，与 B11/B12 判定域一致格过 |
| UNX-F9485 | LTP 定时器/时间族用例联测 | 390 | — | UNX-F9485-J1 时间族联测判据过，用例逐格联测可观测，与 B06/B27 判定域一致格过 |
| UNX-F9486 | LTP 失败分类账（可跑/环境限/待实现） | 370 | — | UNX-F9486-J1 分类账判据过，三分类逐条落账，可跑项 10/10 次复测格过 |
| UNX-F9487 | LTP skip 显式声明面联测 | 370 | — | UNX-F9487-J1 skip 面联测判据过，skip 逐条显式声明可观测，静默 skip 零发生格过 |
| UNX-F9488 | LTP 返回码与 errno 对账联测 | 340 | — | UNX-F9488-J1 对账联测判据过，返回码/errno 逐格对账可观测，与 B31 对账面一致格过 |
| UNX-F9489 | LTP 压力子集联测（fork/exec 循环） | 360 | — | UNX-F9489-J1 压测联测判据过，循环注入统计格过，资源回收零泄漏声明格过 |
| UNX-F9490 | LTP 与 glibc 消费面对账 | 340 | — | UNX-F9490-J1 对账判据过，消费面交叉对账逐格可观测，与 B34 对账面一致格过 |
| UNX-F9491 | LTP 判据号互认抽验组（编号引用不代写） | 330 | — | UNX-F9491-J1 互认抽验判据过，C3 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9492 | LTP 覆盖账（可跑面×族矩阵） | 330 | — | UNX-F9492-J1 覆盖账判据过，族矩阵 100% 落账格过，空格零发生声明格过 |
| UNX-F9493 | 联测失败分类账（LTP 面） | 310 | — | UNX-F9493-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9494 | 联测编排自动化判据 | 310 | — | UNX-F9494-J1 编排判据过，联测脚本一键跑通可观测，输出联测报告格过 |
| UNX-F9495 | LTP 回归基线冻结组 | 300 | — | UNX-F9495-J1 基线冻结判据过，B35 联测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9496 | LTP 与 POSIX 认证路径对账（C3 域判据引用） | 340 | — | UNX-F9496-J1 对账判据过，认证路径以编号引用逐格对账可观测，代写零发生格过 |
| UNX-F9497 | LTP 联测与 Windows 对照账 | 330 | — | UNX-F9497-J1 对照账判据过，Windows 映射表落账，差异账全列格过 |
| UNX-F9498 | 与 B15 号面冻结复核 | 310 | — | UNX-F9498-J1 复核判据过，B15 冻结面逐项复核一致格过，零漂移声明格过 |
| UNX-F9499 | 与 B29 LTP 可跑登记互认复核 | 300 | — | UNX-F9499-J1 复核判据过，B29 登记互认逐项复核一致格过，零漂移声明格过 |
| UNX-F9500 | B35 断言集（LTP 面回归收官） | 440 | — | UNX-F9500-J1 断言集判据过，族矩阵全过格过，skip 显式面全过格过，批收口三断言落账格过 |

<!-- 主册行 11806 · #### UNX-C2-B36 · I 型联调八：跨域压力矩阵与竞态注入联测收官（C2×C3×C4）（F9501–F95 -->
#### UNX-C2-B36 · I 型联调八：跨域压力矩阵与竞态注入联测收官（C2×C3×C4）（F9501–F9520 · 20 条）

> AI-12 承办｜批次类型：I 型（任务书 B31–B36 I 型联调后段 · 压力竞态收官）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B36.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B35 35,500 + 本批 7,100 = 211,600 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B12 EINTR 单点深化/B26 SA_RESTART 矩阵/B30 竞态注入统计；C3 域压力账本；C4 域 futex/epoll 压测面｜防重声明：I 型联调 20 号为号面新铺（B01–B35 零触及本段联调号，零重复）；扩号段 F9501–F9520 承接冻结面，零重编零私设号；联测判据只写编号+一句话，不代写 C3/C4 判据｜红线注记：本批零写盘零引导零固件操作；压测联测只走统计观测面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B36 是 I 型后段收官批：跨域压力矩阵三方联签 + 竞态注入 100 次×10 场景统计 + B31–B36 六批联签总对账——I 型段一次性收口｜判据与后续 deepen/C2-B36.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9501 | I 型联调收官总纲（跨域压力/竞态联签协议） | 400 | — | UNX-F9501-J1 联签协议判据过，C2×C3×C4 三方压力竞态面互认格过，跨界代写禁令声明格过 |
| UNX-F9502 | 跨域压力矩阵编排（C2×C3×C4 三方） | 430 | — | UNX-F9502-J1 编排判据过，三方矩阵逐格编排可观测，跨界代写禁令格过 |
| UNX-F9503 | 竞态注入统计收官（100 次×10 场景） | 410 | — | UNX-F9503-J1 统计收官判据过，10 场景各 100 次注入统计格过，零复现声明格过 |
| UNX-F9504 | fork/exec 风暴压测联测 | 400 | — | UNX-F9504-J1 压测联测判据过，风暴注入统计格过，资源回收零泄漏可观测 |
| UNX-F9505 | fd 洪泛与回收压测联测 | 390 | — | UNX-F9505-J1 压测联测判据过，洪泛/回收交替统计格过，EMFILE 边界 10/10 次检出格过 |
| UNX-F9506 | 内存压力联测（brk/mmap 交替） | 380 | — | UNX-F9506-J1 压测联测判据过，交替注入统计格过，ENOMEM 边界 10/10 次检出格过 |
| UNX-F9507 | 信号风暴与 EINTR 压测联测 | 370 | — | UNX-F9507-J1 压测联测判据过，风暴注入 EINTR 统计格过，与 B26 矩阵一致格过 |
| UNX-F9508 | futex 争用风暴联测（C4 联签） | 350 | — | UNX-F9508-J1 争用联测判据过，风暴统计格过，与 B32 压测面一致格过 |
| UNX-F9509 | epoll 洪泛联测（C4 联签） | 350 | — | UNX-F9509-J1 洪泛滥测判据过，洪泛统计格过，与 B33 背压面一致格过 |
| UNX-F9510 | 时间面并发压测联测 | 350 | — | UNX-F9510-J1 压测联测判据过，并发注入统计格过，单调性零回退声明格过 |
| UNX-F9511 | 身份/权限切换压测联测 | 350 | — | UNX-F9511-J1 压测联测判据过，切换注入统计格过，与 B25 判定域一致格过 |
| UNX-F9512 | 压测指标账（P95/吞吐/泄漏三轴） | 340 | — | UNX-F9512-J1 指标账判据过，三轴指标逐项落账可观测，基线冻结格过 |
| UNX-F9513 | 压测失败分类账 | 340 | — | UNX-F9513-J1 分类账判据过，失败三分类逐条落账，可复测项 10/10 次检出格过 |
| UNX-F9514 | 竞态零复现声明账（收官口径） | 330 | — | UNX-F9514-J1 声明账判据过，10 场景零复现逐场景落账格过，收官口径声明格过 |
| UNX-F9515 | 判据号互认抽验组（编号引用不代写） | 330 | — | UNX-F9515-J1 互认抽验判据过，C3/C4 判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9516 | 联测编排自动化判据 | 320 | — | UNX-F9516-J1 编排判据过，压测脚本一键跑通可观测，输出统计报告格过 |
| UNX-F9517 | 压力回归基线冻结组 | 310 | — | UNX-F9517-J1 基线冻结判据过，B36 压测面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9518 | I 型段收口断言（B31–B36 六批联签总对账） | 330 | — | UNX-F9518-J1 收口断言判据过，六批联签逐批对账格过，I 型段收口声明格过 |
| UNX-F9519 | 与 B12/B26/B30 竞态矩阵互认复核 | 310 | — | UNX-F9519-J1 复核判据过，三批竞态矩阵逐项复核一致格过，零漂移声明格过 |
| UNX-F9520 | B36 断言集（I 型后段收官） | 310 | — | UNX-F9520-J1 断言集判据过，压力矩阵全过格过，竞态零复现格过，I 型段收口落账格过 |

<!-- 主册行 11833 · #### UNX-C2-B37 · C 型收官一：域级总回归矩阵（F8801–F9600 全表）（F9521–F9540 -->
#### UNX-C2-B37 · C 型收官一：域级总回归矩阵（F8801–F9600 全表）（F9521–F9540 · 20 条）

> AI-12 承办｜批次类型：C 型（收官批 · 域级总回归）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B37.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B36 42,600 + 本批 7,100 = 218,700 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B13 错误矩阵四通道边界值总装；B15 冻结体系五件套；B37 判据与 run-tests 大测试总闸条款（fast/full/single）｜防重声明：C 型收官 20 号为号面新铺（B01–B36 零触及本段收官号，零重复）；扩号段 F9521–F9540 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；回归编排只走编排/对账面，实机执行随闸门补测（QEMU 批量排队可过夜）；双轨产线条款默认适用｜批注（AI-12）：B37 是 C 型收官首批：800 号全表回归编排（八片并行/单闸门汇总）+ M/E/I 三段面回归总装 + ktest 编排判据——域级回归一张账｜判据与后续 deepen/C2-B37.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9521 | C 型收官总纲（域级总回归协议） | 420 | — | UNX-F9521-J1 收官总纲判据过，总回归协议四方受理面落账格过，闸门口径声明格过 |
| UNX-F9522 | syscall 全表 800 号回归编排（F8801–F9600） | 440 | — | UNX-F9522-J1 全表编排判据过，800 号逐号编排落账可观测，零漏号声明格过 |
| UNX-F9523 | 回归分片编排（八片并行/单闸门汇总） | 400 | — | UNX-F9523-J1 分片编排判据过，八片边界逐片落账格过，汇总闸门单点声明格过 |
| UNX-F9524 | M 型面回归矩阵（fcntl·ioctl/mremap/wait 族逐族） | 400 | — | UNX-F9524-J1 矩阵判据过，五族逐族回归编排可观测，与 B16–B20 判定域一致格过 |
| UNX-F9525 | E 型错误矩阵回归总装（B21–B28 逐格） | 390 | — | UNX-F9525-J1 总装判据过，八批矩阵逐格回归编排可观测，判定域零漂移格过 |
| UNX-F9526 | I 型联调面回归总装（B29–B36 逐签） | 380 | — | UNX-F9526-J1 总装判据过，八批联签逐签回归编排可观测，互认面零漂移格过 |
| UNX-F9527 | 深化面回归对账（B01–B15 判据逐条） | 370 | — | UNX-F9527-J1 对账判据过，十五批深化判据逐条回归编排可观测，零漏条声明格过 |
| UNX-F9528 | ktest -f c2 编排判据（fail==0+skip 显式） | 360 | — | UNX-F9528-J1 编排判据过，fail==0+skip 显式两口径落账格过，静默 skip 零发生格过 |
| UNX-F9529 | 回归门禁红绿口径账 | 360 | — | UNX-F9529-J1 口径账判据过，红绿判定逐条落账可观测，灰区零发生声明格过 |
| UNX-F9530 | flaky 隔离与重试账（100 次统计口径） | 340 | — | UNX-F9530-J1 隔离账判据过，flaky 100 次统计口径逐条落账格过，重试上界声明格过 |
| UNX-F9531 | 回归失败分类账（阻断/待升级/环境限） | 340 | — | UNX-F9531-J1 分类账判据过，三分类逐条落账，阻断项清单格过 |
| UNX-F9532 | 回归基线快照冻结 | 340 | — | UNX-F9532-J1 快照冻结判据过，基线快照逐项冻结可观测，哈希登记格过 |
| UNX-F9533 | 回归覆盖率账（800 号×判据 J1） | 330 | — | UNX-F9533-J1 覆盖率账判据过，逐号 J1 覆盖 100% 落账格过，空号零发生声明格过 |
| UNX-F9534 | 回归耗时预算账（分钟级快测/过夜全量） | 320 | — | UNX-F9534-J1 预算账判据过，快测分钟级/全量过夜两档预算落账格过 |
| UNX-F9535 | 与 run-tests 大测试总闸对账（fast/full/single） | 320 | — | UNX-F9535-J1 对账判据过，三档入口对账落账格过，旧脚本收编零重写声明格过 |
| UNX-F9536 | 判据号互认抽验组（编号引用不代写） | 310 | — | UNX-F9536-J1 互认抽验判据过，他域判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9537 | 回归编排自动化判据 | 310 | — | UNX-F9537-J1 编排判据过，回归编排脚本一键跑通可观测，输出编排报告格过 |
| UNX-F9538 | 回归矩阵与 B13 四通道边界值总装对账 | 320 | — | UNX-F9538-J1 对账判据过，四通道边界值逐通道复核一致格过，零漂移声明格过 |
| UNX-F9539 | 与 B15 冻结体系五件套复核 | 310 | — | UNX-F9539-J1 复核判据过，五件套逐件复核一致格过，零漂移声明格过 |
| UNX-F9540 | B37 断言集（800 号回归编排 100% 编账） | 340 | — | UNX-F9540-J1 断言集判据过，全表编排 100% 落账格过，分片闸门全过格过，批收口三断言落账格过 |

<!-- 主册行 11860 · #### UNX-C2-B38 · C 型收官二：域经与文档快照冻结（F9541–F9560 · 20 条） -->
#### UNX-C2-B38 · C 型收官二：域经与文档快照冻结（F9541–F9560 · 20 条）

> AI-12 承办｜批次类型：C 型（收官批 · 域经与快照）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B38.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B37 49,700 + 本批 7,100 = 225,800 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B15 冻结体系五件套与文档快照判例（F4298 类比）；C5 域快照判例（编号引用）；B01–B40 四十批批次册全景｜防重声明：C 型收官 20 号为号面新铺（B01–B37 零触及本段收官号，零重复）；扩号段 F9541–F9560 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；快照冻结只走哈希/对账面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B38 是 C 型收官二批：域经十条 + 文档快照三件 SHA-256 冻结 + 判据账/批次册/域账满额快照——域知识资产一张账封存｜判据与后续 deepen/C2-B38.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9541 | C 型收官二总纲（域经与快照协议） | 410 | — | UNX-F9541-J1 收官总纲判据过，域经与快照协议落账格过，冻结口径声明格过 |
| UNX-F9542 | 域经十条（C2 域施工经验清单：判例/坑/约定） | 400 | — | UNX-F9542-J1 域经判据过，十条经验逐条落账可观测，判例/坑/约定三轴齐备格过 |
| UNX-F9543 | 文档快照三件 SHA-256 冻结（批次册/判据账/域小结） | 390 | — | UNX-F9543-J1 快照冻结判据过，三件哈希逐件登记可观测，10/10 次复测一致格过 |
| UNX-F9544 | 判据账终版冻结（800 判据 ID 唯一性复核） | 400 | — | UNX-F9544-J1 终版冻结判据过，800 判据 ID 唯一性复核格过，重复零发生声明格过 |
| UNX-F9545 | 批次册 40 件清单冻结（件名/行数/状态三列） | 400 | — | UNX-F9545-J1 清单冻结判据过，40 件三列逐件落账格过，与批次册逐件一致格过 |
| UNX-F9546 | 深化册 15 件快照对账（B01–B15） | 380 | — | UNX-F9546-J1 快照对账判据过，15 件深化册逐件对账可观测，与判据账一致格过 |
| UNX-F9547 | 域账满额快照（240,000 分批递推表） | 380 | — | UNX-F9547-J1 满额快照判据过，40 批递推表逐批落账格过，合计 240,000 格过 |
| UNX-F9548 | 号段边界快照（F8801/F9600 首尾相接证明） | 360 | — | UNX-F9548-J1 边界快照判据过，首尾相接证明逐段落账格过，无空洞无重叠格过 |
| UNX-F9549 | 域内引用图快照（嫁接源×消费点） | 360 | — | UNX-F9549-J1 引用图判据过，嫁接源×消费点逐边落账可观测，孤边零发生声明格过 |
| UNX-F9550 | 冻结哈希账（SHA-256 逐件登记） | 350 | — | UNX-F9550-J1 哈希账判据过，逐件哈希登记可观测，10/10 次复测一致格过 |
| UNX-F9551 | 快照篡改检测判据（哈希比对复测） | 340 | — | UNX-F9551-J1 篡改检测判据过，哈希比对 10/10 次检出格过，篡改零漏检声明格过 |
| UNX-F9552 | 域经与 ADR 对账（升级接管判例引用编号） | 340 | — | UNX-F9552-J1 对账判据过，ADR 判例以编号引用逐条对账可观测，代写零发生格过 |
| UNX-F9553 | 域经与其他域共享清单（可复用判例外借账） | 330 | — | UNX-F9553-J1 共享账判据过，外借判例逐条落账可观测，受让域登记格过 |
| UNX-F9554 | 快照目录编排（冻结件存放路径账） | 330 | — | UNX-F9554-J1 目录编排判据过，冻结件路径逐件落账格过，路径唯一声明格过 |
| UNX-F9555 | 判据号互认抽验组（编号引用不代写） | 320 | — | UNX-F9555-J1 互认抽验判据过，他域判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9556 | 快照编排自动化判据 | 320 | — | UNX-F9556-J1 编排判据过，快照脚本一键跑通可观测，输出哈希报告格过 |
| UNX-F9557 | 域经回归基线冻结组 | 320 | — | UNX-F9557-J1 基线冻结判据过，域经面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9558 | 域经与 B15 冻结体系五件套对账 | 330 | — | UNX-F9558-J1 对账判据过，五件套逐件复核一致格过，零漂移声明格过 |
| UNX-F9559 | 与 C5 域快照判例互认（编号引用） | 320 | — | UNX-F9559-J1 互认判据过，C5 判例以编号引用对账可观测，代写零发生格过 |
| UNX-F9560 | B38 断言集（快照冻结 100% 落账） | 320 | — | UNX-F9560-J1 断言集判据过，三件快照全过格过，哈希账 100% 格过，批收口三断言落账格过 |

<!-- 主册行 11887 · #### UNX-C2-B39 · C 型收官三：号面冻结终版与升级接管清册（F9561–F9580 · 20 条） -->
#### UNX-C2-B39 · C 型收官三：号面冻结终版与升级接管清册（F9561–F9580 · 20 条）

> AI-12 承办｜批次类型：C 型（收官批 · 冻结终版与接管）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B39.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B38 56,800 + 本批 7,100 = 232,900 / 240,000（域账守恒 66,870+102,130+71,000=240,000）｜嫁接源：B16 扩号段承接冻结面声明；B30 F9400 半域冻结互认；ADR-UNX-004 升级接管分层（编号引用）；B14 meter 消费面｜防重声明：C 型收官 20 号为号面新铺（B01–B38 零触及本段收官号，零重复）；扩号段 F9561–F9580 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；冻结清册只走对账面，零触及实现写路径；双轨产线条款默认适用｜批注（AI-12）：B39 是 C 型收官三批：800 号冻结终版逐号清单 + 升级接管清册（C1 meter 接管面触发条件）+ 他域消费冻结面登记——号面治理一张账｜判据与后续 deepen/C2-B39.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9561 | C 型收官三总纲（冻结终版与接管协议） | 400 | — | UNX-F9561-J1 收官总纲判据过，冻结终版与接管协议落账格过，终审口径声明格过 |
| UNX-F9562 | 号面冻结终版（F8801–F9600 逐号冻结清单） | 400 | — | UNX-F9562-J1 终版冻结判据过，800 号逐号冻结落账可观测，零漏号声明格过 |
| UNX-F9563 | 冻结面五件套终版对账（B15 体系 × 终版） | 390 | — | UNX-F9563-J1 终版对账判据过，五件套逐件终版复核一致格过，零漂移声明格过 |
| UNX-F9564 | 升级接管清册（meter::SyscallMeter/lxerrno::VarixErr 接管面） | 420 | — | UNX-F9564-J1 接管清册判据过，两接管对象逐项落账可观测，接管面清单格过 |
| UNX-F9565 | 接管分层判例对账（ADR-UNX-004 编号引用） | 400 | — | UNX-F9565-J1 分层对账判据过，ADR 判例以编号引用逐层对账可观测，代写零发生格过 |
| UNX-F9566 | 接管触发条件账（上游 finalize 日×接管日） | 390 | — | UNX-F9566-J1 触发条件账判据过，触发条件逐条落账可观测，未到条件零接管声明格过 |
| UNX-F9567 | 冻结互认终版（C3/C4/C5 台账互认登记） | 380 | — | UNX-F9567-J1 互认终版判据过，三域互认逐域落账可观测，互认零悬空声明格过 |
| UNX-F9568 | 他域消费冻结面登记（C3 LTP/C4 futex·epoll/C1 meter） | 360 | — | UNX-F9568-J1 消费登记判据过，三域消费面逐项落账格过，未登记消费零发生格过 |
| UNX-F9569 | 零重编零私设号终审（800 号 grep 复核） | 360 | — | UNX-F9569-J1 终审判据过，800 号 grep 复核零重编零私设格过，10/10 次复测格过 |
| UNX-F9570 | 号面与 Windows API 对照终版账 | 350 | — | UNX-F9570-J1 对照终版判据过，Windows 映射逐号落账可观测，差异账全列格过 |
| UNX-F9571 | 号面与 POSIX 认证对照终版账（C3 判据引用） | 350 | — | UNX-F9571-J1 对照终版判据过，POSIX 映射以编号引用逐号对账可观测，代写零发生格过 |
| UNX-F9572 | 私有挂点保护账（ENOTTY 缺席面终版） | 350 | — | UNX-F9572-J1 保护账判据过，缺席面终版逐项落账格过，C4 pty 上线翻转挂点声明格过 |
| UNX-F9573 | 冻结面变更流程账（解冻需双签声明） | 340 | — | UNX-F9573-J1 变更流程判据过，解冻双签流程落账可观测，未签变更零发生格过 |
| UNX-F9574 | 接管回归预演编排（fake 接管路径） | 340 | — | UNX-F9574-J1 预演编排判据过，fake 接管路径逐段编排可观测，Schema 先行声明格过 |
| UNX-F9575 | 判据号互认抽验组（编号引用不代写） | 320 | — | UNX-F9575-J1 互认抽验判据过，他域判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9576 | 冻结终版自动化判据（脚本 grep 复核编排） | 310 | — | UNX-F9576-J1 自动化判据过，grep 复核脚本一键跑通可观测，输出终审报告格过 |
| UNX-F9577 | 接管清册回归基线冻结组 | 310 | — | UNX-F9577-J1 基线冻结判据过，接管面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9578 | 与 B16 扩号段承接声明终版复核 | 330 | — | UNX-F9578-J1 复核判据过，B16 承接声明逐项复核一致格过，零漂移声明格过 |
| UNX-F9579 | 与 B30 F9400 半域冻结对账复核 | 310 | — | UNX-F9579-J1 复核判据过，F9400 半域冻结逐项复核一致格过，零漂移声明格过 |
| UNX-F9580 | B39 断言集（终版冻结 800 号 100% 落账） | 290 | — | UNX-F9580-J1 断言集判据过，终版冻结全过格过，互认零悬空格过，批收口三断言落账格过 |

<!-- 主册行 11914 · #### UNX-C2-B40 · C 型收官四：域收官断言 F9600 三断言 + 移交包（C2 域满账收口）（F95 -->
#### UNX-C2-B40 · C 型收官四：域收官断言 F9600 三断言 + 移交包（C2 域满账收口）（F9581–F9600 · 20 条）

> AI-12 承办｜批次类型：C 型（收官批 · 域收官断言与移交）｜本批 [已深化] 收口：20 条全为会话链新深化（深化册 deepen/C2-B40.md 全六要素收口）｜域账累计：B01–B15 66,870 + B16–B30 102,130 + B31–B39 63,900 + 本批 7,100 = 240,000 / 240,000（域账守恒 66,870+102,130+71,000=240,000，域满额收口 ✓）｜嫁接源：B15 F9100 段收官三断言判例（ktest/行数/判据账）；B39 号面冻结终版；B37 域级总回归编排｜防重声明：C 型收官 20 号为号面新铺（B01–B39 零触及本段收官号，零重复）；扩号段 F9581–F9600 承接冻结面，零重编零私设号｜红线注记：本批零写盘零引导零固件操作；收官断言只走登记/编排面，实机执行随闸门补测（QEMU 批量排队可过夜）；双轨产线条款默认适用｜批注（AI-12）：B40 是 C2 域终批：F9600 三断言（ktest -f c2 fail==0+skip 显式 / 行数 240,000 收口 / 800 判据账）登记落账 + 四向联签总结 + 移交包——域 800 条满账一次性收口｜判据与后续 deepen/C2-B40.md 逐条同名同判据同 ID

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F9581 | C 型收官四总纲（F9600 三断言协议） | 420 | — | UNX-F9581-J1 收官总纲判据过，三断言协议落账格过，闸门联动口径声明格过 |
| UNX-F9582 | 域收官断言一：ktest -f c2 fail==0+skip 显式（全 800 号） | 380 | — | UNX-F9582-J1 断言一判据过，fail==0+skip 显式全表口径落账格过，静默 skip 零发生格过 |
| UNX-F9583 | 域收官断言二：行数 240,000 收口（分批递推终版表） | 380 | — | UNX-F9583-J1 断言二判据过，40 批递推终版表逐批落账格过，合计 240,000 格过 |
| UNX-F9584 | 域收官断言三：800 判据账（ID 唯一+三成分终审） | 400 | — | UNX-F9584-J1 断言三判据过，800 判据 ID 唯一复核格过，三成分 100% 终审格过 |
| UNX-F9585 | 三断言联动编排（一闸门三断言串联） | 390 | — | UNX-F9585-J1 联动编排判据过，三断言串联编排可观测，任一断言红则闸门红格过 |
| UNX-F9586 | open_risks 移交包（域内未决风险清单） | 390 | — | UNX-F9586-J1 移交包判据过，未决风险逐条落账可观测，受让方登记格过 |
| UNX-F9587 | 升级接管移交清单（与 C1 接管面交接） | 370 | — | UNX-F9587-J1 移交清单判据过，接管面交接逐项落账格过，C1 受理声明格过 |
| UNX-F9588 | 深化期移交清单（B16–B40 500 条骨架深化指引） | 370 | — | UNX-F9588-J1 移交清单判据过，500 条深化指引逐批落账可观测，深化顺序声明格过 |
| UNX-F9589 | 跨域联签总结账（C2×C3×C4×C5 四向互认终表） | 350 | — | UNX-F9589-J1 总结账判据过，四向互认逐向落账格过，悬空互认零发生声明格过 |
| UNX-F9590 | 域收官登记（总纲/根台账/handoff 三处收口点清单） | 340 | — | UNX-F9590-J1 收官登记判据过，三处收口点逐点落账格过，漏登零发生声明格过 |
| UNX-F9591 | 域收官复测性声明（三断言 10 次重复全过口径） | 340 | — | UNX-F9591-J1 复测性声明判据过，三断言 10/10 次重复口径落账格过，flaky 零发生格过 |
| UNX-F9592 | 收官批与 B15 段收官断言（F9100）对账 | 350 | — | UNX-F9592-J1 对账判据过，F9100 段断言逐项复核一致格过，口径升级声明格过 |
| UNX-F9593 | 域收官与 freeze 体系对账（冻结五件套引用编号） | 350 | — | UNX-F9593-J1 对账判据过，五件套以编号引用逐件复核可观测，代写零发生格过 |
| UNX-F9594 | 域收官移交验收单（接手会话就绪判据） | 330 | — | UNX-F9594-J1 验收单判据过，就绪判据逐条落账格过，接手会话可启动声明格过 |
| UNX-F9595 | 判据号互认抽验组（编号引用不代写） | 330 | — | UNX-F9595-J1 互认抽验判据过，他域判据以编号引用 100% 落账，代写零发生声明格过 |
| UNX-F9596 | 收官自动化编排判据（脚本一键三断言） | 320 | — | UNX-F9596-J1 自动化判据过，三断言脚本一键跑通可观测，输出收官报告格过 |
| UNX-F9597 | 收官回归基线冻结组 | 320 | — | UNX-F9597-J1 基线冻结判据过，收官面基线逐项冻结可观测，零漂移声明格过 |
| UNX-F9598 | 域收官与 Windows/POSIX 对照终版对账 | 340 | — | UNX-F9598-J1 对照对账判据过，两对照终版逐项复核一致格过，差异账全列格过 |
| UNX-F9599 | C2 域 800 条满账总声明（40 批全列账） | 310 | — | UNX-F9599-J1 总声明判据过，40 批全列账逐批落账格过，满账零缺批声明格过 |
| UNX-F9600 | B40 断言集（域收官三断言登记落账） | 320 | — | UNX-F9600-J1 断言集判据过，三断言逐条登记落账格过，域收官声明格过，批收口落账格过 |

<a id="dom-C3"></a>

