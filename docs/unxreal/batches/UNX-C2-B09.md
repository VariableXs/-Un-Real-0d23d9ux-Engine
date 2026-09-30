# UNX-C2-B09 · 多路复用四件套之一：epoll 全集（F8961–F8980 · 20 条）

> AI-12 承办｜批次类型：M 型机制批启幕｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B08 36,630 + 本批 4,170 = 40,800 / 240,000｜嫁接源：Linux man-pages epoll(7)/select(2)/poll(2) 章节、现存 `syscall::calls::EventPort`/`WaitQueue` 存量档为升级接管扩容｜防重：事件端口本体与调度器唤醒本体归 A3/A2 域防重——本域为多路复用号面语义与消费协议；epoll 实例数据结构升级 EventPort 不另起炉灶｜判据与 deepen/C2-B09.md 逐条同名同判据同 ID

### UNX-F8961 · epoll_create/epoll_create1 语义档
- 域/批：C2/B09｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8961-J1 实例 fd 分配判据过，size>0 遗留参数仅校验判据过，EPOLL_CLOEXEC 旗标判据过
### UNX-F8962 · epoll_ctl ADD/MOD/DEL 三操作
- 域/批：C2/B09｜纯功能行数：250｜状态：[已深化]｜判据：UNX-F8962-J1 三操作幂等性矩阵判据过（重复 ADD 返 EEXIST、DEL 不存在返 ENOENT、MOD 不存在返 ENOENT），事件掩码登记读写回一致
### UNX-F8963 · epoll_wait 就绪队列语义
- 域/批：C2/B09｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8963-J1 就绪事件按序返回判据过，maxevents 截断语义判据过，就绪队列空转阻塞路径与超时路径双断言过
### UNX-F8964 · epoll_pwait 信号掩码挂点
- 域/批：C2/B09｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8964-J1 原子掩码切换（等待前装填/返回后还原）判据过，与 B11 信号族挂点协议一致，sigmask=NULL 退化为 epoll_wait 判据过
### UNX-F8965 · 边沿/水平触发双模（EPOLLET）
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8965-J1 LT 模式持续就绪重报判据过，ET 模式一次 armed 触发判据过，双模同 fd 混用行为差异登记不静默
### UNX-F8966 · one-shot 与唤醒联动（EPOLLONESHOT）
- 域/批：C2/B09｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8966-J1 one-shot 触发后自动 disarmed 判据过，re-arm 需显式 MOD 判据过，唤醒后事件重查协议（F8974）联动一致
### UNX-F8967 · fd 就绪回调挂接协议（fd 表/管道/socket 消费）
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8967-J1 三类 fd（普通文件恒就绪/管道读写端/socket 挂点）就绪回调注册判据过，回调与 FdTable 状态位联动判据过
### UNX-F8968 · epoll 与调度器唤醒联动（WaitQueue 消费）
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8968-J1 等待线程入队 WaitQueue 判据过，对端写唤醒→epoll_wait 返回链路判据过，虚假唤醒防护理由落账（事件重查）
### UNX-F8969 · select/pselect6 遗留兼容档
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8969-J1 select 读/写/异常三集语义判据过，pselect6 原子掩码判据过，RouteNote::ExtraArgsIgnored 差异登记
### UNX-F8970 · poll/ppoll 兼容档
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8970-J1 pollfd 数组返回 revents 位矩阵判据过（POLLIN/POLLOUT/POLLERR/POLLHUP/POLLNVAL），ppoll 掩码原子性判据过
### UNX-F8971 · fd_set 位图 ABI 冻结（FD_SETSIZE 1024）
- 域/批：C2/B09｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8971-J1 位图布局 128 字节冻结判据过，fd≥1024 返 EINVAL 判据过，冻结文档落盘与变更协议挂点
### UNX-F8972 · 超时语义档（timeout -1/0/正数三态）
- 域/批：C2/B09｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8972-J1 三态语义（无限阻塞/纯轮询/限时等待）判据各过，超时返 0 与 EINTR 打断分路判据过
### UNX-F8973 · epoll 错误矩阵（EBADF/EINVAL/EEXIST/ENOENT/ETIME）
- 域/批：C2/B09｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8973-J1 五错误码 × 3 号（epoll_ctl/epoll_wait/epoll_create1）矩阵 15 格逐格一致，EPWAKEUP 非法事件位 EINVAL 格过
### UNX-F8974 · 事件丢失防护（唤醒后事件重查协议）
- 域/批：C2/B09｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8974-J1 唤醒→重查→返回闭环判据过（10^3 次唤醒零丢事件），竞态窗口（唤醒与就绪位清零交错）防护理由落账
### UNX-F8975 · 性能账本（就绪延迟 P99 登记，Windows 对照）
- 域/批：C2/B09｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8975-J1 就绪延迟 P99 预登记（闸门补测挂点），P99 ≤ Windows WaitForMultipleObjects ×1.5 目标锚定判据落账
### UNX-F8976 · epoll 与 eventfd 联动挂点
- 域/批：C2/B09｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8976-J1 eventfd 写→epoll 就绪链路判据过，EFD_SEMAPHORE 读取减一语义与 B10 F8986 一致判据过
### UNX-F8977 · 多路复用器资源上限（maxevents/RLIMIT_NOFILE 联动）
- 域/批：C2/B09｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8977-J1 maxevents≤0 返 EINVAL 判据过，实例数受 RLIMIT_NOFILE 约束判据过，上限触达 EMFILE 事件留痕
### UNX-F8978 · fork 后 epoll 实例继承语义
- 域/批：C2/B09｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8978-J1 子进程继承实例 fd 可 wait 判据过，父子共享就绪队列语义与 Linux 对照一致，差异点登记不静默
### UNX-F8979 · epoll ktest 半自动驱动（双端探针）
- 域/批：C2/B09｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8979-J1 双端探针（写端触发/读端 wait）驱动器判据过，三种触发模式（LT/ET/ONESHOT）批内全覆盖
### UNX-F8980 · ktest syscall 面 B09 批断言集
- 域/批：C2/B09｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8980-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，多路复用断言独立编号可单独复跑
