# UNX-C2-B12 · EINTR 单点深化与 IPC 挂点族（F9021–F9040 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B11 49,460 + 本批 4,220 = 53,680 / 240,000｜嫁接源：Linux man-pages pipe(2)/socket(2)/sysvipc(7)/mq_overview(7) 章节、现存 `syscall::calls::PipeTable` 存量档为升级接管扩容｜防重：socket 协议栈本体归 B2 网络域（AI-07）防重——本域只立 socket 号面挂点与消费协议；IPC 各族本体机制归后续 M 型批次，本批立挂点登记档不重铺｜判据与 deepen/C2-B12.md 逐条同名同判据同 ID

### UNX-F9021 · EINTR/SA_RESTART 判定器单点本体（自 F8948 深化）
- 域/批：C2/B12｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F9021-J1 判定器四输入（handler 置位/信号掩码/慢调用类别/重启白名单）全组合矩阵判据过，白名单外号不重启判据过，单点唯一性（全号面仅此一处分流）判据过
### UNX-F9022 · restart_syscall 挂点
- 域/批：C2/B12｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9022-J1 重启号面挂点注册判据过，重启上下文（原号+参数快照）保存恢复判据过，无上下文调用返 EINVAL 格过
### UNX-F9023 · pipe/pipe2 本体档（PipeTable 对齐）
- 域/批：C2/B12｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F9023-J1 双 fd（读/写端）分配判据过，O_NONBLOCK/O_CLOEXEC 旗标矩阵判据过，读写端闭环 10^3 次零失真判据过
### UNX-F9024 · 管道读写阻塞语义与容量（F_SETPIPE_SZ 联动）
- 域/批：C2/B12｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9024-J1 默认 64KiB 容量判据过，写满阻塞/NONBLOCK EAGAIN 双路径判据过，原子写边界（≤PIPE_BUF 4KiB 不撕裂）判据过
### UNX-F9025 · socket 基座挂点（socket/bind/listen 登记档）
- 域/批：C2/B12｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9025-J1 三号挂点注册判据过，AF_UNIX/AF_INET 域校验判据过，防重：协议栈本体归 B2（AI-07）不重铺声明落账
### UNX-F9026 · socket connect/accept/send/recv 挂点
- 域/批：C2/B12｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9026-J1 四 IO 挂点注册判据过，慢调用 EINTR 联动（F9021 判定器消费）判据过，未连接 fd 操作返 ENOTCONN 格过
### UNX-F9027 · sendmsg/recvmsg 与 msghdr ABI 冻结
- 域/批：C2/B12｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9027-J1 msghdr/msg_iov 控制消息布局冻结判据过，多 iovec 聚合读写判据过，控制消息（ SCM_RIGHTS 挂点）登记不实现声明落账
### UNX-F9028 · UNIX 域套接字挂点（AF_UNIX）
- 域/批：C2/B12｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F9028-J1 抽象/路径名双命名空间挂点判据过，SOCK_STREAM/DGRAM 双型判据过，与 B10 管道消费档（F8995）联动判据过
### UNX-F9029 · SysV 信号量挂点（semget/semop 登记档）
- 域/批：C2/B12｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9029-J1 两号挂点注册判据过，sem_ops 结构布局冻结判据过，本体档 M 型后续批次声明落账
### UNX-F9030 · SysV 共享内存挂点（shmget/shmat 登记档）
- 域/批：C2/B12｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9030-J1 两号挂点注册判据过，与 B3 内存域（AI-09）防重对接声明落账，shmflg 权限校验挂点判据过
### UNX-F9031 · SysV 消息队列挂点（msgget/msgsnd 登记档）
- 域/批：C2/B12｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9031-J1 两号挂点注册判据过，msgbuf 布局冻结判据过，阻塞发送 EINTR 联动判据过
### UNX-F9032 · POSIX mqueue 挂点（mq_open/mq_send 登记档）
- 域/批：C2/B12｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9032-J1 两号挂点注册判据过，/dev/mqueue 路径命名约定登记判据过，未实现本体返 -ENOSYS 判据过
### UNX-F9033 · IPC 键空间与 ftok 语义
- 域/批：C2/B12｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9033-J1 ftok 派生键（st_ino/st_dev 混合）判据过，IPC_PRIVATE 特殊键判据过，键冲突查询挂点判据过
### UNX-F9034 · IPC 资源限额（RLIMIT/MSGMNB/SEMMNS）落账
- 域/批：C2/B12｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9034-J1 三类限额常量落账判据过，触达限额 ENOSPC 格过，限额可调（procfs 挂点）登记
### UNX-F9035 · IPC 错误矩阵（EIDRM/EKEYEXPIRED/ENOSPC/EINVAL）
- 域/批：C2/B12｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9035-J1 四错误码 × 3 号（semget/shmget/msgget）矩阵 12 格逐格一致，键不存在 ENOENT 格过
### UNX-F9036 · ipc 系统调用兼容门（IPC_64 旗标）
- 域/批：C2/B12｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9036-J1 遗留 ipc(2) 单号面（int 0x80 遗留）转接判据过，IPC_64 旗标分路判据过，RouteNote::Permuted 差异登记
### UNX-F9037 · 管道与多路复用联动（pipe→epoll 消费复测）
- 域/批：C2/B12｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9037-J1 管道 fd 挂入 epoll（B09 F8967 协议）复测判据过，写端触发读端唤醒闭环 10^3 次判据过
### UNX-F9038 · IPC 家族错误路径与编码器单点（F8923）一致复测
- 域/批：C2/B12｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9038-J1 IPC 族全部错误路径经 F8923 单点编码复测判据过，绕行路径零发现断言过
### UNX-F9039 · IPC 族 ktest 半自动驱动
- 域/批：C2/B12｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9039-J1 管道/socket 挂点联动驱动器判据过，SysV/mqueue 挂点 ENOSYS 路径断言覆盖
### UNX-F9040 · ktest syscall 面 B12 批断言集
- 域/批：C2/B12｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F9040-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，EINTR 单点与 IPC 挂点断言独立编号可单独复跑
