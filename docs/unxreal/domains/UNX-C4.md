# UNX-C4 · 域归集卷（AI-100 归集快照 2026-10-01）

> 由 AI-100 归集器 v2 从主汇编册块级切分生成（块定域：标题 token > 正文 token 投票 > 前块延续）；主汇编册仍为唯一权威总册，本卷为按域阅读视图，零改写零删节。

<!-- 主册行 13029 · ### UNX-C4 · IPC 与 Unix 语义 -->
### UNX-C4 · IPC 与 Unix 语义

> 域档｜承办 AI-14｜批册 40（01–40）｜条目 800｜F10401–F11200｜行数合计 240,000｜已深化 0 / 骨架 800

<!-- 主册行 13033 · #### UNX-C4-B01 · 管道地基：环形缓冲/端点分发/EOF-SIGPIPE 链（F10401–F10420 -->
#### UNX-C4-B01 · 管道地基：环形缓冲/端点分发/EOF-SIGPIPE 链（F10401–F10420 · 20 条）

> AI-14 承办｜域账累计：本批 5,840 / 240,000（B01 为域首批）｜嫁接源：POSIX.1-2017 §pipe()/Linux pipe(7) 行为面只跟随｜防重：与 proc/ipc.rs 既有 IpcBus 定长端口消息总线为不同抽象层（IpcBus 是建模期端口消息面，本批是 fd 型字节流管道面），无重复；与 A1 域 F0002–F0060 无交叠｜批注：本批为 C4 域 F 型地基批之一，B01–B08 完成后转入 M 型机制批 B09 起。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10401 | pipe2 环形缓冲区对象与回绕语义 | 340 | 骨架 | UNX-F10401-J1 环形回绕读写 100 万次零错位，写端全关后 read 返 0（EOF），读端全关后 write 触发 SIGPIPE（SIG_IGN 设置下返 EPIPE） |
| UNX-F10402 | 管道对象生命周期与读写端引用计数 | 320 | 骨架 | UNX-F10402-J1 最后一个端点关闭时对象回收，回收前读写端计数与 fd 表引用数三方一致，计数差非零即红账 |
| UNX-F10403 | pipe2 fd 端点分发与 flag 组合 | 300 | 骨架 | UNX-F10403-J1 pipe2(fds, O_NONBLOCK\|O_CLOEXEC) 后 F_GETFD 含 CLOEXEC 位、F_GETFL 含 O_NONBLOCK 位，两端标志独立可查 |
| UNX-F10404 | 管道容量与 F_SETPIPE_SZ/F_GETPIPE_SZ | 300 | 骨架 | UNX-F10404-J1 默认 64KB 档，设置 1KB–1MB 内 2 次幂容量生效，非 2 次幂向上取整落账，越界 EINVAL |
| UNX-F10405 | 写端全关 EOF 语义链 | 260 | 骨架 | UNX-F10405-J1 写端计数归零后未读完数据仍可读尽，读尽后 read 恒返 0，poll 同步报 POLLHUP |
| UNX-F10406 | 读端全关 SIGPIPE 触发链 | 300 | 骨架 | UNX-F10406-J1 读端计数归零后 write 返 EPIPE 并投递 SIGPIPE，SIG_IGN 下不投递仅返 EPIPE，死亡判据落账 |
| UNX-F10407 | fork 继承管道端点与共享对象语义 | 320 | 骨架 | UNX-F10407-J1 fork 后父子跨进程读写同一缓冲 10 万次零错序，对象不复制，引用计数 +1 留账 |
| UNX-F10408 | 管道等待队列与阻塞唤醒 | 340 | 骨架 | UNX-F10408-J1 空读阻塞者被写唤醒后取得新数据，被信号打断返 EINTR，唤醒不丢失（写入先行注入 100 次） |
| UNX-F10409 | 管道 poll/epoll 挂点三态 | 320 | 骨架 | UNX-F10409-J1 EPOLLIN/EPOLLOUT/EPOLLHUP 组合矩阵 16 态与缓冲实际状态一致率 100% |
| UNX-F10410 | 管道 F_GETFL/F_SETFL 与 O_NONBLOCK 动态切换 | 240 | 骨架 | UNX-F10410-J1 单端 O_NONBLOCK 切换后另一端不受影响，切换后首个读/写行为立即生效 |
| UNX-F10411 | 管道错误码矩阵（EBADF/EFAULT/EINVAL） | 240 | 骨架 | UNX-F10411-J1 坏 fd 返 EBADF、用户指针越界返 EFAULT 且零内核触碰、flags 非法返 EINVAL，三码各 10/10 次正确 |
| UNX-F10412 | 管道统计账本 | 260 | 骨架 | UNX-F10412-J1 每管道读写字节计数、阻塞次数、唤醒次数三账可导出，压测后总读字节 == 总写字节 |
| UNX-F10413 | ktest 管道面断言集（B01 聚合） | 380 | 骨架 | UNX-F10413-J1 本批 19 条判据聚合入 ktest，一次命令全跑，通过率 100% 才算绿，失败项带定位输出 |
| UNX-F10414 | 管道对象内存账与泄漏审计钩子 | 260 | 骨架 | UNX-F10414-J1 创建/销毁计数守恒（差值恒为零），审计钩子异步化不进读写热路径 |
| UNX-F10415 | pipe2 与 C2 挂号注册协议联签 | 280 | 骨架 | UNX-F10415-J1 pipe2 经 syscall/table.rs 号段注册分发，错误码统一走 errno.rs 编码器，表内可查号可查名可查能力位 |
| UNX-F10416 | 管道小写原子保证（<PIPE_BUF 单写者视角） | 280 | 骨架 | UNX-F10416-J1 单写者 <4KB 写入在读侧呈现为连续字节序列，阻塞写被打断后不产生半条交错 |
| UNX-F10417 | 管道多写者交错记账 | 280 | 骨架 | UNX-F10417-J1 4 写者并发大块写入允许交错但每块内部完整，块边界标记 100 万次零撕裂 |
| UNX-F10418 | 管道 EAGAIN 非阻塞空读/写满矩阵 | 240 | 骨架 | UNX-F10418-J1 非阻塞空读返 EAGAIN、写满返 EAGAIN，与 EOF（返 0）严格区分，4 象限矩阵全对 |
| UNX-F10419 | 管道 dup/dup2/dup3 端点别名语义 | 280 | 骨架 | UNX-F10419-J1 dup 后两端引用计数同步增长，dup2 覆盖目标 fd 时原对象正确释放，别名写与原写等效 |
| UNX-F10420 | 管道与 shell 重定向联测地基（C5 预埋） | 300 | 骨架 | UNX-F10420-J1 fork+dup2 构造 cmd1\|cmd2 双子进程流水线，64KB 数据经双管道往返零丢失 |

<!-- 主册行 13060 · #### UNX-C4-B02 · PIPE_BUF 原子性/FIFO 节点/splice 预留（F10421–F104 -->
#### UNX-C4-B02 · PIPE_BUF 原子性/FIFO 节点/splice 预留（F10421–F10440 · 20 条）

> AI-14 承办｜域账累计：B01 5,840 + 本批 5,460 = 11,300 / 240,000｜嫁接源：POSIX.1-2017、Linux fifo(7)/splice(2) 行为面只跟随｜防重：与 B01 管道地基为分层关系（B01 建对象与端点语义，本批立边界矩阵与命名管道/零拷贝面），判据无重复；FIFO 挂接与 B1 VFS 为联签消费关系非重复实现｜批注：E 型正反双判据素材由本批 F10421/F10422/F10427 预产，B21–B28 边界批聚合复测。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10421 | PIPE_BUF 原子写边界（4KB±1 对照） | 300 | 骨架 | UNX-F10421-J1 ≤4096 字节写 1 万块零交错，4097 字节块允许交错且短写对账，边界两侧行为分界清晰 |
| UNX-F10422 | O_NONBLOCK 写满 EAGAIN 全矩阵 | 260 | 骨架 | UNX-F10422-J1 满管非阻塞写按块长分路（≤PIPE_BUF EAGAIN；>PIPE_BUF 允许短写），短写字节数对账 100% |
| UNX-F10423 | FIFO VFS 节点与 mkfifo（B1 联签） | 320 | 骨架 | UNX-F10423-J1 mkfifo 产出 S_IFIFO 节点，stat 可见模式位，打开后进入管道对象复用 B01 语义 |
| UNX-F10424 | FIFO 打开语义（O_RDONLY 阻塞等写者/O_NONBLOCK ENXIO） | 300 | 骨架 | UNX-F10424-J1 阻塞档等对端到来 100/100 次，非阻塞读开无写者 ENXIO、非阻塞写开无读者 ENXIO |
| UNX-F10425 | FIFO EOF/SIGPIPE 与管道行为一致性 | 260 | 骨架 | UNX-F10425-J1 FIFO 的 EOF/EPIPE/SIGPIPE 三链与匿名管道逐行为对勾，差异账为零 |
| UNX-F10426 | FIFO 权限位与打开模式校验 | 240 | 骨架 | UNX-F10426-J1 只读节点上 O_WRONLY 打开返 EACCES，权限位与 B1 校验一致，umask 生效 |
| UNX-F10427 | splice 读侧页引用转移零拷贝 | 340 | 骨架 | UNX-F10427-J1 1GB 数据经 splice 迁移校验和一致，memcpy 字节数为零（页引用计数转移账证实） |
| UNX-F10428 | splice 写侧与 tee 预留挂点 | 300 | 骨架 | UNX-F10428-J1 splice 出管道到文件/另一管道各 100MB 校验一致，tee 挂点注册可查不激活 |
| UNX-F10429 | vmsplice 边界与用户页登记预留 | 260 | 骨架 | UNX-F10429-J1 vmsplice 以 SPLICE_F_GIFT 登记语义建账，未实现路径返 ENOSYS 且账面显式 |
| UNX-F10430 | 管道容量压力（1MB 档）与回绕交叉 | 280 | 骨架 | UNX-F10430-J1 1MB 档容量下回绕读写 100 万次零错位，跨回绕块完整性 100% |
| UNX-F10431 | FIFO 多读者竞争分发 | 260 | 骨架 | UNX-F10431-J1 2 读者竞争同一 FIFO，字节流不复制不丢失，各读段拼接后等于写入全量 |
| UNX-F10432 | FIFO 多写者原子交错断言 | 260 | 骨架 | UNX-F10432-J1 ≤PIPE_BUF 块 4 写者零交错，>PIPE_BUF 块块内完整，与 F10417 同判据在命名面上复测 |
| UNX-F10433 | pipe 与 dup3/O_CLOEXEC 组合 | 240 | 骨架 | UNX-F10433-J1 dup3(fd, n, O_CLOEXEC) 后 exec 该 fd 关闭，标志位三级（fd/状态字/管道）互不越界 |
| UNX-F10434 | 管道在 exec 后的存活语义（C1 联签） | 260 | 骨架 | UNX-F10434-J1 无 CLOEXEC 端点 exec 后可读写如常，带位端点 exec 后关闭并触发对端 EOF/EPIPE |
| UNX-F10435 | FIFO 与 epoll 挂点（named pipe 三态） | 260 | 骨架 | UNX-F10435-J1 FIFO 的 EPOLLIN/OUT/HUP 三态与匿名管道矩阵一致，双向对勾零差异 |
| UNX-F10436 | SIGPIPE 忽略与 MSG_NOSIGNAL 类比账 | 240 | 骨架 | UNX-F10436-J1 SIG_IGN 与 send 类 MSG_NOSIGNAL 语义映射账落表，管道侧按 SIG_IGN 分支执行 |
| UNX-F10437 | 管道 EPIPE 后写端状态机 | 220 | 骨架 | UNX-F10437-J1 首次 EPIPE 后继续写恒返 EPIPE 且不 panic、不投递重复 SIGPIPE（每进程一次投递记账） |
| UNX-F10438 | FIFO 生命周期（unlink 后已打开句柄存活） | 240 | 骨架 | UNX-F10438-J1 unlink 后新 open 失败（ENOENT），已打开句柄读写如常，全关后对象回收 |
| UNX-F10439 | ktest 管道边界面断言集（B02 聚合） | 360 | 骨架 | UNX-F10439-J1 本批 19 条判据聚合一条命令全跑，100% 绿，失败带条目级定位 |
| UNX-F10440 | 管道吞吐基准账 | 260 | 骨架 | UNX-F10440-J1 64KB 档大块吞吐与 4KB 块吞吐实测落表（MB/s），连续 3 次波动 <10%，只记实测 |

<!-- 主册行 13087 · #### UNX-C4-B03 · 信号地基：pending/sigaction/投递路径（F10441–F10460  -->
#### UNX-C4-B03 · 信号地基：pending/sigaction/投递路径（F10441–F10460 · 20 条）

> AI-14 承办｜域账累计：B01–B02 11,300 + 本批 5,680 = 16,980 / 240,000｜嫁接源：POSIX.1-2017 §信号、Linux signal(7) 行为面只跟随｜防重：信号投递机制本体归本域（C4 任务书专题二），A3 侧仅消费停止/继续调度语义（F10545 联签面），与 sched/engine.rs 既有调度器为分层关系；与 C2 错误编码器的 EINTR/SA_RESTART 单点联签按任务书 §4 冻结接口执行｜批注：本批为信号双批之首（B03 地基 + B04 递送与帧），31 标准信号编号表以 Linux signal(7) 为对标面。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10441 | 信号编号表与 31 标准信号全集 | 280 | 骨架 | UNX-F10441-J1 编号 1–31 与默认处置五类（TERM/IGN/CORE/STOP/CONT）逐信号落表，与 Linux signal(7) 对照零差异 |
| UNX-F10442 | 进程 pending 位图与标准信号去重 | 300 | 骨架 | UNX-F10442-J1 同号标准信号连投 100 次只挂起 1 位，位清除后可再挂，实时信号排队行为（B04）与本位图解耦 |
| UNX-F10443 | sigaction 结构与 handler/mask/flags 全字段 | 320 | 骨架 | UNX-F10443-J1 rt_sigaction 注册/查询往返逐字段一致，sa_mask 集合运算精确，flags 四位（SA_RESTART/SA_SIGINFO/SA_NOCLDSTOP/SA_NODEFER）独立生效 |
| UNX-F10444 | kill/tgkill/raise 投递路径 | 300 | 骨架 | UNX-F10444-J1 三入口投递目标定位正确（pid/进程组/自投），无权限返 EPERM、无进程返 ESRCH，目标死活竞态有账 |
| UNX-F10445 | 信号递送时机（syscall 返回前/中断退出前） | 320 | 骨架 | UNX-F10445-J1 pending 位在每线程返回用户态检查点必查，投递延迟上限落账（提交即查），检查点漏查为零 |
| UNX-F10446 | SA_RESTART 重启语义与 C2 错误编码器单点联签 | 300 | 骨架 | UNX-F10446-J1 慢 syscall 被 EINTR 打断后按 SA_RESTART 自动重启，重启表覆盖 12 个慢调用，与 errno.rs 单点对勾 |
| UNX-F10447 | SA_SIGINFO 三参 handler 与 siginfo_t 布局 | 300 | 骨架 | UNX-F10447-J1 siginfo_t 的 si_signo/si_errno/si_code/si_addr 四字段布局与 ABI 注出处一致，三参 handler 收值正确 |
| UNX-F10448 | SA_NOCLDSTOP/SA_NOCLDWAIT/SA_NODEFER/SA_RESETHAND 旗标矩阵 | 300 | 骨架 | UNX-F10448-J1 四旗标独立用例各 10/10 次语义正确，组合用例互不干扰，旗标间冲突有定义账 |
| UNX-F10449 | 默认处置表（TERM/IGN/CORE/STOP/CONT 五类） | 280 | 骨架 | UNX-F10449-J1 五类处置执行路径分离（终止/忽略/账面记账/停止/继续），DFL 逐信号执行与编号表一致 |
| UNX-F10450 | SIGKILL/SIGSTOP 不可捕获不可屏蔽 | 240 | 骨架 | UNX-F10450-J1 对两信号的 sigaction/拦截尝试全部 EINVALID（EINVAL），pending 期间进程仍可被杀，10/10 次必达 |
| UNX-F10451 | SIGSEGV 同步信号与 si_addr 落账 | 300 | 骨架 | UNX-F10451-J1 页错误路径产 SIGSEGV 时 si_addr 与故障地址一致（10/10 次），handler 恢复后进程可续跑 |
| UNX-F10452 | SIGILL/SIGFPE/SIGBUS 同步三件 | 280 | 骨架 | UNX-F10452-J1 三类同步异常的触发-投递-si_code 链各 10/10 次，与 F10451 同栈帧路径复用 |
| UNX-F10453 | sigprocmask 掩码操作（BLOCK/UNBLOCK/SETMASK） | 280 | 骨架 | UNX-F10453-J1 三命令语义各 10/10 次（叠加/摘除/置满），旧掩码回传正确，掩码操作原子 |
| UNX-F10454 | sigpending 查询与挂起集语义 | 240 | 骨架 | UNX-F10454-J1 挂起集返回位与实际 pending 位一致（含被掩码阻塞位），解除阻塞后立即递送 |
| UNX-F10455 | 信号掩码 fork/exec 继承语义 | 260 | 骨架 | UNX-F10455-J1 fork 原样继承掩码与 pending 清空，exec 后掩码保留、pending 清空、处置重置 DFL，三规则各 10/10 次 |
| UNX-F10456 | kill 进程组/会话广播（与 C1 联签） | 280 | 骨架 | UNX-F10456-J1 kill(-pgid) 组内全体成员收到投递（排除自身路径按 POSIX），孤儿组与组边界判定与 C1 进程组账一致 |
| UNX-F10457 | 信号队列容量与 RLIMIT_SIGPENDING 预留 | 240 | 骨架 | UNX-F10457-J1 实时信号排队上限受 rlimit 约束，超限返 EAGAIN，标准信号去重不受限（B04 域消费） |
| UNX-F10458 | ktest 信号面断言集（B03 聚合） | 360 | 骨架 | UNX-F10458-J1 本批 19 条判据聚合一条命令全跑 100% 绿，失败三段定位，跑序随机化 |
| UNX-F10459 | 信号与 C2 挂号注册协议联签 | 260 | 骨架 | UNX-F10459-J1 rt_sigaction/kill/sigprocmask 等号面注册可查（号/名/能力位），错误码经 errno.rs 单点，注册样板同 F10415 |
| UNX-F10460 | 信号递送账本（投递/丢弃/阻塞三计数） | 240 | 骨架 | UNX-F10460-J1 全系统投递/阻塞/丢弃三计数可导出，压测后账与用例预期一致，账写入热路径开销落表 |

<!-- 主册行 13114 · #### UNX-C4-B04 · 信号递送：rt_sigframe/三大高频信号/实时信号（F10461–F10480 -->
#### UNX-C4-B04 · 信号递送：rt_sigframe/三大高频信号/实时信号（F10461–F10480 · 20 条）

> AI-14 承办｜域账累计：B01–B03 16,980 + 本批 5,780 = 22,760 / 240,000｜嫁接源：Linux signal(7)/sigreturn(2)、System V AMD64 ABI 帧布局（注出处，禁凭记忆）｜防重：B03 建机制面（位图/sigaction/投递入口），本批建递送执行面（帧/三链/排队/多线程），条目边界逐条可辨；SIGCHLD 收割链与 C1 wait 语义联签（wait 本体归 C1）｜批注：本批收口后信号"投递→递送→返回"全链闭环，pty 组 SIGWINCH（F10511）与作业控制信号面（B08）直接消费。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10461 | rt_sigframe 布局与 ABI 注出处 | 320 | 骨架 | UNX-F10461-J1 帧内 pretcode/uc/siginfo 偏移与 System V ABI 注出布局逐字段一致，offsetof 断言全过 |
| UNX-F10462 | 用户 handler 栈帧构造与对齐 | 300 | 骨架 | UNX-F10462-J1 帧落用户栈按 16 字节对齐，寄存器现场快照与故障时点一致（10/10 次），栈溢出有红账 |
| UNX-F10463 | rt_sigreturn 回位与栈指针校验 | 300 | 骨架 | UNX-F10463-J1 sigreturn 恢复寄存器现场与构造时快照逐寄存器一致，伪造帧（坏魔数）拒收 SIGSEGV |
| UNX-F10464 | SA_ONSTACK 替换信号栈（sigaltstack） | 280 | 骨架 | UNX-F10464-J1 altstack 注册/查询/切换三态正确，栈内再触发同信号按 POSIX 判核终止（栈耗尽路径） |
| UNX-F10465 | SIGCHLD→waitpid 收割链（SA_NOCLDSTOP 抑制） | 320 | 骨架 | UNX-F10465-J1 子退出→SIGCHLD→waitpid 收割全链可观测，SA_NOCLDSTOP 下停止事件零投递（继续/退出照常），CLD_* 码与事件一一对应 |
| UNX-F10466 | SIGPIPE 端到端链（管道写→投递→默认终止） | 280 | 骨架 | UNX-F10466-J1 F10406 判死→B03 挂位→检查点递送→DFL 终止四段链端到端可观测，SIG_IGN 分支返 EPIPE 存活 |
| UNX-F10467 | SIGSEGV 端到端链（坏指针→崩溃账） | 280 | 骨架 | UNX-F10467-J1 F10451 挂接→递送→handler/DFL 双分支端到端 10/10 次，DFL 崩溃账含信号号/地址/寄存器三快照 |
| UNX-F10468 | 实时信号 32–64 排队（RTSIG 区间） | 300 | 骨架 | UNX-F10468-J1 32–64 号逐投逐递（排队不合并），FIFO 同号序保持，受 F10457 quota 约束超限 EAGAIN |
| UNX-F10469 | sigqueue/sigval 载荷传递 | 260 | 骨架 | UNX-F10469-J1 sigval 联合体（int/指针）往返一致 10/10 次，SI_QUEUE 码与发送者 pid/uid 如实填充 |
| UNX-F10470 | 信号嵌套与掩码自动置位（handler 执行期屏蔽本信号） | 280 | 骨架 | UNX-F10470-J1 非 NODEFER 下 handler 执行期本信号 pending 不重入，返回后掩码恢复精确（含 sa_mask 增量），嵌套其他信号按掩码判定 |
| UNX-F10471 | 竞态注入：投递与 handler 注册交错 | 300 | 骨架 | UNX-F10471-J1 注册前投递按 DFL、注册后投递按 handler，交错窗口注入 1 万次零越窗行为 |
| UNX-F10472 | 信号与 EINTR 全矩阵（慢 syscall 中断重启表） | 320 | 骨架 | UNX-F10472-J1 12 慢调用 × （SA_RESTART 有/无）= 24 格矩阵全对，部分完成调用的返回字节数如实 |
| UNX-F10473 | 停止/继续信号组（SIGSTOP/SIGTSTP/SIGCONT 与调度联签） | 300 | 骨架 | UNX-F10473-J1 STOP 家族三信号停止行为与 SIGCONT 恢复行为经 A3 联签接口落调度态，CONT 清 pending 停止类位 |
| UNX-F10474 | SIGWINCH 联动（pty 尺寸变更预埋） | 240 | 骨架 | UNX-F10474-J1 尺寸变更事件→SIGWINCH 挂位→前台组递送链可观测（事件源 B06 挂接），链路探针全绿 |
| UNX-F10475 | 信号 pending 在 exec 时清空与保留面 | 240 | 骨架 | UNX-F10475-J1 exec 后 pending 清空（F10455 规则复验）、SIG_IGN 处置保留、SA_SIGINFO/RESTART 旗标重置，三面逐项对勾 |
| UNX-F10476 | 多线程 tgkill 定向投递（线程掩码独立） | 280 | 骨架 | UNX-F10476-J1 tgkill 定向线程收递、其他线程不受扰，线程级掩码独立生效，进程级 pending 与线程级双层账一致 |
| UNX-F10477 | 信号帧溢出防护与 altstack 缺失处置 | 260 | 骨架 | UNX-F10477-J1 用户栈不足帧大小时强制 altstack，altstack 缺失按 DFL 终止并崩溃账落"栈溢出"原因，零内核踩栈 |
| UNX-F10478 | 信号账本与 LTP 信号族用例映射 | 280 | 骨架 | UNX-F10478-J1 本域信号判据与 LTP 信号组用例名映射账落表（覆盖/待补两态），待补项带原因码 |
| UNX-F10479 | ktest 信号递送面断言集（B04 聚合） | 360 | 骨架 | UNX-F10479-J1 本批 19 条判据一条命令全跑 100% 绿，等待器基建（F10458）复用，失败三段定位 |
| UNX-F10480 | 信号语义与 Linux 基准对照（录制账） | 280 | 骨架 | UNX-F10480-J1 20 场景行为序列（strace 级）与 Linux 基准逐条对照零差异，差异账恒空 |

<!-- 主册行 13141 · #### UNX-C4-B05 · pty 主从对：ptmx/pts/字节通路/录制器基建（F10481–F10500  -->
#### UNX-C4-B05 · pty 主从对：ptmx/pts/字节通路/录制器基建（F10481–F10500 · 20 条）

> AI-14 承办｜域账累计：B01–B04 22,760 + 本批 5,840 = 28,600 / 240,000｜嫁接源：Linux pty(7)/pts(4)/tty_ioctl(2)、openpty(3)（注出处，禁凭记忆）｜防重：与现存 proc/ipc.rs IpcBus（定长端口消息总线）为不同抽象层——pty 是 fd 型字节流字符设备；管道本体 B01 已收，pty 主从通路复用其环形缓冲体而判据独立逐条可辨；控制终端赋予本体在 B08（F10543），本批只立"不越期挂接"防线（F10496）｜批注：本批为判据主轴"pty 上 vim 类全屏程序录制对照"铺地基——录制器基建（F10486/F10497）是主轴前置件；F10500 联签 C2 挂号（pty ioctl 族号段注册）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10481 | ptmx 主设备节点与从端分配 | 340 | 骨架 | UNX-F10481-J1 并发 open ptmx 100 次（4 线程×25）零重号零丢号，上限路径返 EAGAIN，每对主从引用计数=2 落配对账 |
| UNX-F10482 | pts 从设备节点与 devtmpfs 挂接（B1 联签） | 300 | 骨架 | UNX-F10482-J1 open 主端后 /dev/pts/N 节点存在且 ptsname 名字与配对账逐字符一致（100/100 次），串号探针零命中 |
| UNX-F10483 | 主从字节通路（复用 B01 环形缓冲体） | 360 | 骨架 | UNX-F10483-J1 主写→从读、从写→主读双向各 10 万字节内容一致零错位，双缓冲水位独立计数 |
| UNX-F10484 | 主端 EOF/EIO 语义（从端全关终态链） | 280 | 骨架 | UNX-F10484-J1 从端全关后主 read 得 EOF（0 字节），主 write 返 EIO 不发 SIGPIPE（与 pipe 判死差异注出处），四格矩阵 10/10 次 |
| UNX-F10485 | 从端关闭回收与 pts 编号复用 | 300 | 骨架 | UNX-F10485-J1 双端引用归零即回收、编号回空闲链，污染-回收-复用循环 100 次零残留（新对缓冲全零） |
| UNX-F10486 | 字节流录制器基建（判据主轴前置件） | 380 | 骨架 | UNX-F10486-J1 1 MB 混合流（含回绕边界）录制-回放逐字节零差异，时间戳 100/100 段单调，旁路不持通路锁 |
| UNX-F10487 | TIOCGPTN/TIOCSPTLCK ioctl | 240 | 骨架 | UNX-F10487-J1 TIOCGPTN 返编号与配对账一致 100/100，TIOCSPTLCK 写读往返一致且锁位联动 unlockpt 拒绝路径 10/10 |
| UNX-F10488 | unlockpt/grantpt 语义（B1 权限联签） | 260 | 骨架 | UNX-F10488-J1 锁位下从端 open 拒（EIO）、unlock 后开 100/100，grantpt 权属（uid:tty）账可查 100/100 |
| UNX-F10489 | pty 背压与双通路水位 | 300 | 骨架 | UNX-F10489-J1 主端写满阻塞至从端读空再前进，O_NONBLOCK 返 EAGAIN，交叉写满压测 10/10 次无死锁 |
| UNX-F10490 | pty 与 SIGWINCH 联动（F10474 事件源消费） | 280 | 骨架 | UNX-F10490-J1 尺寸变更→事件源→挂位→递送四拍时序单调 10/10 次，前台组内 3 进程同收零漏 |
| UNX-F10491 | 多 pty 并发与实例上限 | 280 | 骨架 | UNX-F10491-J1 64 对并发回环内容校验和独立零错，上限触顶 EAGAIN 10/10，串扰探针零命中 |
| UNX-F10492 | pty poll 就绪面（B11 统一 poll 预埋） | 320 | 骨架 | UNX-F10492-J1 POLLIN/POLLOUT/POLLHUP 与水位三态 100 次全对，就绪翻转唤醒延迟在 ktest 时钟粒度内 100/100 |
| UNX-F10493 | 主从缓冲统计账 | 240 | 骨架 | UNX-F10493-J1 in/out/水位/终态四点计数与实际字节总和零差额（1 MB 流） |
| UNX-F10494 | pty open 旗标矩阵（含 O_NOCTTY 差异声明） | 260 | 骨架 | UNX-F10494-J1 主端 O_RDWR/RDONLY/WRONLY × O_NONBLOCK 六格矩阵全对，权限面按 fd 权限位判 |
| UNX-F10495 | pts 节点权限与 tty 组（B1 联签） | 240 | 骨架 | UNX-F10495-J1 节点权属（uid:tty · 0600）与 grantpt 账一致，非法 uid open 得 EACCES 100/100 |
| UNX-F10496 | 会话控制终端挂接声明（不越期防线，主体 B08） | 280 | 骨架 | UNX-F10496-J1 B05 阶段全部 open 路径零会话关联（探针 100/100），B08 挂接清单引用本条为前置 |
| UNX-F10497 | 录制器与判据主轴挂接（场景编号账） | 360 | 骨架 | UNX-F10497-J1 1 万行流录制-回放零差异，判据主轴场景账（vim/less/自定义全屏三类）落表 |
| UNX-F10498 | pty 设备事件账与 devtmpfs 联动声明（B1 联签） | 240 | 骨架 | UNX-F10498-J1 节点建/删事件与配对账增减对平（建 N 删 N 净差=现存对数），100 轮循环对平 |
| UNX-F10499 | pty 关闭次序与悬挂写者唤醒 | 280 | 骨架 | UNX-F10499-J1 先主/先从/同时三序矩阵下阻塞者全数唤醒得合法终态（EOF/EIO/EBADF），零悬挂（60 秒探针） |
| UNX-F10500 | B05 批压测与 C2 挂号联签 | 300 | 骨架 | UNX-F10500-J1 100 对并发双向回环 10 MB/对 零错位零死锁（120 秒内），pty ioctl 族号段注册表落 table.rs 可查 |

<!-- 主册行 13168 · #### UNX-C4-B06 · termios 与 ioctl 面：四组标志/TIOCGWINSZ/SIGWINCH -->
#### UNX-C4-B06 · termios 与 ioctl 面：四组标志/TIOCGWINSZ/SIGWINCH（F10501–F10520 · 20 条）

> AI-14 承办｜域账累计：B01–B05 28,600 + 本批 5,600 = 34,200 / 240,000｜嫁接源：Linux termios(3)/tty_ioctl(2)/ioctl_tty(2)（注出处，禁凭记忆）｜防重：termios 状态面挂 pty（B05）与行规程（B07）之间——本批立"状态本体与 ioctl 面"，输入处理行为本体归 B07，逐条判据可辨；SIGWINCH 事件源在 B04（F10474），本批 F10511 为事件源的消费反侧（winsize 账本体）｜批注：原始模式（F10514）是判据主轴"vim 类全屏程序"的直通路径——B07 全屏录制基线与本批联签；F10520 联签 C2 挂号（termios ioctl 族号段核验）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10501 | termios 结构体与四组标志 | 320 | 骨架 | UNX-F10501-J1 结构体字段/位宽/枚举值与 termios(3) 注出定义逐字段一致，offsetof 断言全过，四组标志（iflag/oflag/cflag/lflag）位表落账 |
| UNX-F10502 | tcgetattr/tcsetattr 与三动作 | 300 | 骨架 | UNX-F10502-J1 get/set 往返逐字段一致（100/100），TCSANOW/DRAIN/FLUSH 三动作时序语义正确（排空/冲刷探针 10/10 次） |
| UNX-F10503 | 波特率面（存而不用声明） | 220 | 骨架 | UNX-F10503-J1 cfgetispeed/cfsetispeed 存取往返一致 100/100，"不调制实际传输"声明落账，非法速度码 BEINVAL |
| UNX-F10504 | c_iflag 输入标志全位语义 | 340 | 骨架 | UNX-F10504-J1 IGNBRK/BRKINT/INPCK/ISTRIP/IXON/IXANY/ICRNL/INLCR/IGNCR 九位逐一行为判据过（每位正反用例各 10 次） |
| UNX-F10505 | c_oflag OPOST/ONLCR 输出处理 | 240 | 骨架 | UNX-F10505-J1 OPOST 关=字节直通（零改写），OPOST\|ONLCR 下 NL→CR-NL 逐字节可验（1 万行零错） |
| UNX-F10506 | c_cflag 控制标志（无真实 UART 声明） | 240 | 骨架 | UNX-F10506-J1 CS8/CREAD/CLOCAL/PARENB 存取与位账一致，"无 UART 调制面"声明落账，位面供未来串口域消费 |
| UNX-F10507 | c_lflag 本地标志全位语义 | 340 | 骨架 | UNX-F10507-J1 ICANON/ECHO/ISIG/TOSTOP/IEXTEN 位行为逐条判据过（正反用例各 10 次），位组合矩阵关键 8 格全对 |
| UNX-F10508 | VMIN/VTIME 四象限矩阵 | 300 | 骨架 | UNX-F10508-J1 VMIN/VTIME 四象限（阻塞/轮询/限时/字节计数）行为与 termios(3) 注出表逐格一致，限时精度账落 ktest 时钟粒度 |
| UNX-F10509 | 特殊字符集 c_cc | 300 | 骨架 | UNX-F10509-J1 VINTR/VQUIT/VERASE/VEOF/VSUSP/VSTART/VSTOP/VEOL 默认值与注出表一致，重定义往返生效（10/10），_POSIX_VDISABLE 面可用 |
| UNX-F10510 | ioctl TIOCGETA/TIOCSETA 面 | 240 | 骨架 | UNX-F10510-J1 两命令与 tcgetattr/tcsetattr 同源同判（同一实现双入口），uaccess 判界 EFAULT 面 10/10 |
| UNX-F10511 | TIOCGWINSZ/TIOCSWINSZ 与 SIGWINCH（事件源消费侧） | 280 | 骨架 | UNX-F10511-J1 winsize 写读往返一致（100/100），SET 同值零事件（防抖）、变值触发 F10474 事件源 10/10，全零 winsize 合法态 |
| UNX-F10512 | TIOCSCTTY/TIOCNOTTY（B08 联签） | 280 | 骨架 | UNX-F10512-J1 B06 阶段两命令挂分派但行为本体委托 B08（调用得 ENOTTY/预埋占位探针），B08 收口后清单引用本条 |
| UNX-F10513 | tcflush/tcdrain/tcflow | 260 | 骨架 | UNX-F10513-J1 TCIFLUSH/TCOFLUSH/TCIOFLUSH 三冲刷位行为可验（冲后读空/写位恢复 10/10），tcdrain 立即返回声明，tcflow 三动作落账 |
| UNX-F10514 | 原始模式路径与 vim 全屏前置（判据主轴） | 360 | 骨架 | UNX-F10514-J1 cfmakeraw 类原始态下字节零加工直通（控制符不产信号不回显），1 MB 二进制流往返逐字节一致，判据主轴场景账引用本条 |
| UNX-F10515 | termios 默认值与行规程默认策略 | 280 | 骨架 | UNX-F10515-J1 新开 pty 默认 termios 与注出默认表逐字段一致（含 ICRNL/ICANON/ECHO/ISIG 置位、OPOST\|ONLCR），B07 行规程按默认态起跑 |
| UNX-F10516 | exec 时 termios 保留语义 | 240 | 骨架 | UNX-F10516-J1 exec 后 termios 逐字段保留（100/100 不复位），与信号面 exec 清空（F10475）形成"状态两面"对账 |
| UNX-F10517 | echo 控制位（ECHO/ECHOE/ECHOK/ECHONL） | 280 | 骨架 | UNX-F10517-J1 四位组合下回显行为逐格可验（回显字节序与位语义一致），ECHO 关=零回显（控制符含），与 B07 回显管线联签 |
| UNX-F10518 | 非阻塞与 termios 交错 | 240 | 骨架 | UNX-F10518-J1 O_NONBLOCK 与 VMIN/VTIME 同 fd 交错时行为优先级明确（NONBLOCK 压倒限时面），交错序列 100 轮零矛盾 |
| UNX-F10519 | termios 错误码面 | 240 | 骨架 | UNX-F10519-J1 ENOTTY（非 tty fd）/EINVAL（坏旗标/坏参数）/EFAULT（坏指针）三错误逐条可触发（各 10 次），错误码走 errno.rs 单点 |
| UNX-F10520 | B06 批压测与 C2 挂号联签 | 300 | 骨架 | UNX-F10520-J1 termios 全位翻转回归（位面 × 动作 × 特殊字符组合 500 态）全过，termios ioctl 族挂号登记项落 table.rs 可查 |

<!-- 主册行 13195 · #### UNX-C4-B07 · 行规程状态机：三款全屏程序录制基线（F10521–F10540 · 20 条） -->
#### UNX-C4-B07 · 行规程状态机：三款全屏程序录制基线（F10521–F10540 · 20 条）

> AI-14 承办｜域账累计：B01–B06 34,200 + 本批 5,500 = 39,700 / 240,000｜嫁接源：Linux termios(3)/n_tty 行规程行为、signal(7)（注出处，禁凭记忆）｜防重：行规程是 termios 位面（B06）之上的"输入加工管线+回显管线"——位面判据在 B06、行为本体在本批，逐条判据可辨；信号挂位/递送本体在 B03/B04（F10526 只立"生成"侧联签）；作业控制本体在 B08（F10527 联签）｜批注：本批立"三款全屏程序录制基线"（vim/less/自定义全屏的行规程侧预期账），与 F10497 场景账、B05 录制器构成判据主轴三件套；F10540 联签 C2 挂号与主轴基线定版。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10521 | 规范模式行缓冲主循环 | 340 | 骨架 | UNX-F10521-J1 ICANON=1 下输入按行提交（遇 NL 才可读），行内容与输入逐字节一致（1 万行零错），未提交行对读者不可见探针全过 |
| UNX-F10522 | NL/CR 处理与 ICRNL/INLCR/IGNCR 矩阵 | 300 | 骨架 | UNX-F10522-J1 三旗标 8 格组合（开/关/同开）转换行为与 B06 规则表逐格一致，1 万行 × 8 格全过 |
| UNX-F10523 | VErase 退格语义（ECHOE 视觉擦除） | 280 | 骨架 | UNX-F10523-J1 VERASE 删一行内字节（行首删探针=响零操作），ECHOE 开时回显 BS-SP-BS 序列，删除后行内容可验证 |
| UNX-F10524 | VKILL 行删除（ECHOK） | 240 | 骨架 | UNX-F10524-J1 VKILL 清空当前行（缓冲与回显联动），ECHOK 开时删后回显 NL，清后行内容=空探针 10/10 |
| UNX-F10525 | VEOF 置零读（Ctrl-D EOF 链） | 280 | 骨架 | UNX-F10525-J1 行首 ^D 提交 EOF（read 得 0），非行首 ^D 提交已缓冲字节（半行读），连续两次行首 ^D 第二次仍 EOF |
| UNX-F10526 | VINTR/VQUIT 信号生成（ISIG） | 300 | 骨架 | UNX-F10526-J1 ISIG=1 下 VINTR/VQUIT 字节触发 F10474 同源事件→SIGINT/SIGQUIT 挂位，字节不进输入队列（双向断言），ISIG=0 双字节直通 |
| UNX-F10527 | VSUSP 挂起信号（B08 联签） | 240 | 骨架 | UNX-F10527-J1 VSUSP 字节触发 SIGTSTP 挂位（B03 机制），B08 作业面收口后挂接清单引用本条，B07 阶段挂位-递送链可观测 |
| UNX-F10528 | VEOL/VEOL2 边界 | 220 | 骨架 | UNX-F10528-J1 VEOL/VEOL2 定义后作为行界提交（与 NL 同效探针），默认禁用态（_POSIX_VDISABLE）下零行界效应 |
| UNX-F10529 | VREPRINT/VWERASE/VLNEXT（IEXTEN） | 260 | 骨架 | UNX-F10529-J1 IEXTEN=1 时三字符生效（重打/词删/字面化），IEXTEN=0 时三字节按普通字符处理（双向断言各 10 次） |
| UNX-F10530 | 行长上限 4096 与溢出丢弃 | 280 | 骨架 | UNX-F10530-J1 超 4096 字节行：超限字节丢弃（可配响铃声明），首 4096 按行提交，溢出计数入统计账 |
| UNX-F10531 | ECHO 回显管线 | 340 | 骨架 | UNX-F10531-J1 回显字节序与位面组合账（F10517 六格）逐格一致，控制符 ^X 形式回显（可打印化），管线与主循环解耦探针全过 |
| UNX-F10532 | 输出处理 ONLCR/OXTABS/OCRNL | 260 | 骨架 | UNX-F10532-J1 ONLCR 扩展（F10505 联签同源）、OXTABS 制表扩展、OCRNL 转换三面各 1 万字节级零错 |
| UNX-F10533 | 规范/原始双模式切换状态机 | 360 | 骨架 | UNX-F10533-J1 运行中双模式切换（shell→vim→shell 序列 1 万轮）：切换原子（无半态窗口）、未提交行切换时按声明处置（清空/保留可选，默认清空）、切换后行为立即合规 |
| UNX-F10534 | 半行读与 VMIN 协同 | 240 | 骨架 | UNX-F10534-J1 规范模式行内已满请求量即返（不必等行界），VMIN 面与 B06 四象限协同无矛盾（交错 100 轮） |
| UNX-F10535 | 行规程与 pty 通路的挂接点 | 280 | 骨架 | UNX-F10535-J1 挂接面单点：从→主方向输入管线、主→从方向输出管线各一挂接点，旁路直通模式（raw）零加工断言，挂接探针 100/100 |
| UNX-F10536 | 输入队列并发（多读者） | 240 | 骨架 | UNX-F10536-J1 多读者竞争同一 tty 时行不劈半（行原子单位），字节总量守恒（读者得数和==提交数），100 轮零劈行 |
| UNX-F10537 | ISIG 与原始模式互斥语义 | 240 | 骨架 | UNX-F10537-J1 ISIG=0（raw 家族）下三信号字节零信号产生（反向断言 1 万次），ISIG=1 恢复即生效（切换探针 10/10） |
| UNX-F10538 | 行规程统计账 | 220 | 骨架 | UNX-F10538-J1 输入加工量/回显量/丢弃量/信号生成量四计与实测流量对平（1 MB 级零差额） |
| UNX-F10539 | 畸形输入 fuzz（二进制透明） | 280 | 骨架 | UNX-F10539-J1 随机二进制 10 MB fuzz：零 panic/零断言红、非特殊字节全部直通（raw 探针），规范模式下行界外字节如实提交 |
| UNX-F10540 | B07 批压测与三款全屏录制基线定版（C2 联签） | 300 | 骨架 | UNX-F10540-J1 telnet 类行输入回归（行编辑全操作序列 1 万轮）全过；三款全屏录制基线（vim/less/自定义）账面定版；行规程挂号面 table.rs 核验过 |

<!-- 主册行 13222 · #### UNX-C4-B08 · 作业控制与会话：前台组/停止-继续（F10541–F10560 · 20 条） -->
#### UNX-C4-B08 · 作业控制与会话：前台组/停止-继续（F10541–F10560 · 20 条）

> AI-14 承办｜域账累计：B01–B07 39,700 + 本批 5,680 = 45,380 / 240,000｜嫁接源：POSIX.1-2017 job control 章节、Linux setsid(2)/setpgid(2)/credentials(7)/signal(7)（注出处，禁凭记忆）｜防重：停止/继续的调度面本体在 A3（F10548 联签 F10473），本批立会话/组结构面与前台组语义，判据逐条可辨；TIOCSCTTY/TIOCNOTTY 命令占位在 B06（F10512），本批为行为本体收口条（联签核销占位红账）｜批注：本批收口后 B05 F10496"不越期防线"解禁、预演组切真前台组；F10560 与判据主轴（vim ^Z 回 shell 场景）联签。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10541 | setsid 会话创建 | 300 | 骨架 | UNX-F10541-J1 setsid 后调用者成会话首兼组长（sid==pid==pgid），已有进程组长的调用得 EPERM，100/100 次双面 |
| UNX-F10542 | setpgid/getpgid 进程组 | 300 | 骨架 | UNX-F10542-J1 setpgid 三个合法场景（自设/父设子/同 exec 窗口）行为正确 100/100，跨会话迁移 EPERM、死进程 ESRCH 错误面 10/10 |
| UNX-F10543 | 会话首与控制终端关联（F10496 防线解禁收口） | 280 | 骨架 | UNX-F10543-J1 会话首 open tty 未经 ctty 赋予零关联（防线保留），经 F10544 赋予后关联账双向可查，B06 占位红账核销 |
| UNX-F10544 | TIOCSCTTY 竞取（无竞态会话首） | 280 | 骨架 | UNX-F10544-J1 竞取语义三态（无主得/有主拒 EPERM/强夺需成员且无主）全对，赋予后 tty 账会话字段双向一致 100/100 |
| UNX-F10545 | 前台进程组 tcgetpgrp/tcsetpgrp | 300 | 骨架 | UNX-F10545-J1 get/set 往返一致（100/100），set 拒绝面（非成员 EPERM/无控制终端 ENOTTY/坏组 ESRCH）10/10，切换后信号投递目标随迁 |
| UNX-F10546 | 后台读 SIGTTIN / 写 SIGTTOU | 320 | 骨架 | UNX-F10546-J1 后台组读 tty 触发 SIGTTIN、写触发 SIGTTOU（TOSTOP 位联动 F10517），SIG_IGN/被阻断成员例外面全对（四象限 10/10） |
| UNX-F10547 | orphaned 进程组 SIGHUP | 280 | 骨架 | UNX-F10547-J1 组成 orphaned（父不在同会话）后组内成员停止/继续事件触发 SIGHUP+SIGCONT 链，非 orphaned 组零触发（反向断言） |
| UNX-F10548 | SIGTSTP/SIGCONT 与调度联签（A3） | 300 | 骨架 | UNX-F10548-J1 停止/继续经 F10473 联签接口落 A3 调度态（可观测状态字），CONT 清 pending 停止类位复验，10/10 次全链 |
| UNX-F10549 | waitpid WUNTRACED/WCONTINUED 作业报告 | 300 | 骨架 | UNX-F10549-J1 三事件（退出/停止/继续）按旗标报告（WUNTRACED/WCONTINUED 联动 C1 收割面），事件码 WIFSTOPPED/WIFCONTINUED 一一对应 100/100 |
| UNX-F10550 | shell 管道作业组派生 | 280 | 骨架 | UNX-F10550-J1 a\|b\|c 三段派生同组（组 id==首段 pid）100/100，组内成员 exec 后组籍不变，作业表语义（B07 行规程侧）联签 |
| UNX-F10551 | TIOCNOTTY 脱离（F10512 占位核销） | 240 | 骨架 | UNX-F10551-J1 脱离后 ctty 账双向清空（会话侧/tty 侧），orphaned 判定随之翻转，B06 占位红账核销账在册 |
| UNX-F10552 | 控制终端消失 SIGHUP 广播 | 280 | 骨架 | UNX-F10552-J1 ctty 最后引用关闭时会话首得 SIGHUP，会话首退出（或忽略）后前台组得 SIGHUP 广播，链账四拍可查 |
| UNX-F10553 | 会话终止清扫与孤儿 reparent（C1 联签） | 300 | 骨架 | UNX-F10553-J1 会话首终止后组清扫账（组解散/账面归零），孤儿 reparent 到 init（C1 联签账）100/100，无悬挂会话残留 |
| UNX-F10554 | killpg 组广播 | 260 | 骨架 | UNX-F10554-J1 killpg 对组内全员挂位（成员数==挂位数对账），坏组 ESRCH、跨会话 EPERM 错误面 10/10 |
| UNX-F10555 | 作业控制与行规程联动（^Z 路径） | 300 | 骨架 | UNX-F10555-J1 ^Z（F10527 生成）→SIGTSTP→前台组停止→shell 收割（WUNTRACED）→tcsetpgrp 换组全链四拍可查，fg 后 ^Z 复测一致 |
| UNX-F10556 | fg/bg 切换原子性 | 260 | 骨架 | UNX-F10556-J1 前台组切换原子（无双前台窗口断言），切换瞬间投递目标唯一（交错 1000 轮零双投） |
| UNX-F10557 | 会话/组 ID 生命周期不变量 | 240 | 骨架 | UNX-F10557-J1 五不变量（sid 终身不变/pgid 变更边界/组长唯一/会话首唯一/ctty 单归属）断言面 1 万进程账零违例 |
| UNX-F10558 | 控制终端 fd 语义（dup2 后关系不变） | 240 | 骨架 | UNX-F10558-J1 dup2 复制 ctty fd 后新 fd 同具 ctty 关系面（getpgrp/getpgrp 行为一致 100/100），close 单 fd 不破坏关联（引用计数） |
| UNX-F10559 | 作业控制压测（shell 类循环） | 320 | 骨架 | UNX-F10559-J1 fg/bg/stop/cont 循环 1 万轮 × 10 并发作业：零悬挂、零双前台、组账守恒，SIGTTIN/TTOU 计数与预期一致 |
| UNX-F10560 | B08 批收口与判据主轴 ^Z 联签 | 300 | 骨架 | UNX-F10560-J1 vim 类全屏 ^Z 回 shell/fg 恢复全链（录制器对照）10/10 次零差异；会话面挂号（F10512/F10551 核销）与 C2 号段核验过；B05 预演组切真账落定 |

<!-- 主册行 13249 · #### UNX-C4-B09 · unix socket 流式：bind/listen/accept（F10561–F -->
#### UNX-C4-B09 · unix socket 流式：bind/listen/accept（F10561–F10580 · 20 条）

> AI-14 承办｜域账累计：B01–B08 45,380 + 本批 5,620 = 51,000 / 240,000｜嫁接源：Linux unix(7)/socket(2)/bind(2)/listen(2)/accept(2)、System V AMD64 ABI（sockaddr 布局，注出处，禁凭记忆）｜防重：与现存 proc/ipc.rs IpcBus（定长端口消息总线）为不同抽象层——unix socket 是 fd 型 socket 地址族；与 B01 pipe 不同层（socket 面向连接/命名/多客户端），流式通路复用环形缓冲体而判据独立｜批注：本批立 AF_UNIX SOCK_STREAM 全生命周期；DGRAM/SEQPACKET/抽象命名空间在 B10；F10580 联签 C2 挂号（socket 族号段）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10561 | AF_UNIX socket()/socketpair 基础 | 320 | 骨架 | UNX-F10561-J1 socket(AF_UNIX,SOCK_STREAM,0) 得 fd 且域/型/协议账可查（100/100），socketpair 得一对互联 fd（往返 10/10），错误域 EAFNOSUPPORT/EPROTOTYPE 面 |
| UNX-F10562 | sockaddr_un 路径命名（108 字节） | 260 | 骨架 | UNX-F10562-J1 sun_path 108 字节边界（超长 ENAMETOOLONG 10/10），地址解析往返（bind 后 getsockname 逐字节一致 100/100） |
| UNX-F10563 | bind 与文件系统 socket 节点（B1 联签） | 280 | 骨架 | UNX-F10563-J1 bind 后路径节点存在（socket 型 inode）且 stat 可查 100/100，已占用 EADDRINUSE、坏路径 EACCES 面 10/10 |
| UNX-F10564 | listen backlog 队列 | 280 | 骨架 | UNX-F10564-J1 backlog=16 时 16 个挂起连接全入队，第 17 个按溢出策略处理（B10/F10576 联签），listen 两次覆盖语义 10/10 |
| UNX-F10565 | accept 四参（addr/addrlen） | 260 | 骨架 | UNX-F10565-J1 accept 返回新 fd（原监听 fd 不动）100/100，addr/addrlen 填充与客户端 bind 地址一致 100/100，坏 addrlen EFAULT 面 |
| UNX-F10566 | connect 状态机（阻塞/非阻塞） | 320 | 骨架 | UNX-F10566-J1 阻塞 connect 在 backlog 有位时立即成、无位时阻塞至有位；非阻塞满时 EAGAIN 且连接请求不丢失语义落账，100/100 次 |
| UNX-F10567 | 流式字节通路 | 340 | 骨架 | UNX-F10567-J1 连接后双向字节流 1 MB 逐字节一致零错位，双端独立缓冲水位账（F10493 同款）对平 |
| UNX-F10568 | shutdown(SHUT_RD/WR/RDWR) | 280 | 骨架 | UNX-F10568-J1 三面行为：RD 后本端读 EOF/对端写 EPIPE 链、WR 后本端写 EPIPE/对端读 EOF、RDWR 双向终态，六格矩阵 10/10 |
| UNX-F10569 | EOF 语义（对端关闭 0 字节读） | 260 | 骨架 | UNX-F10569-J1 对端 close 后缓冲排空读得 EOF（0 字节），本端 write 得 EPIPE+SIGPIPE（B01 链复用），10/10 次 |
| UNX-F10570 | MSG_OOB 预埋（显式 ENOSYS） | 240 | 骨架 | UNX-F10570-J1 MSG_OOB 发送/接收路径显式 ENOSYS（红账落册），普通路径零误伤（不带旗标行为不变 100/100） |
| UNX-F10571 | MSG_PEEK 窥视 | 240 | 骨架 | UNX-F10571-J1 peek 后缓冲不变（再读得同内容 100/100），peek 与并发读竞态守恒（读总量==写入量） |
| UNX-F10572 | MSG_DONTWAIT/NONBLOCK 面 | 260 | 骨架 | UNX-F10572-J1 旗标与 fd 态两源合一（DONTWAIT 压倒 fd 态），读空/写满 EAGAIN 10/10，与 B12 poll 面协同声明 |
| UNX-F10573 | send/recv 与 write/read 等价面 | 280 | 骨架 | UNX-F10573-J1 无旗标 send==write、recv==read 行为逐字节一致（1 MB 对照 100/100），旗标面差异表落账 |
| UNX-F10574 | socketpair 双工通路 | 300 | 骨架 | UNX-F10574-J1 socketpair 双向 1 MB 回环零错位，双端 fd 权限面（读写俱备），关闭单端另一端终态链正确（10/10） |
| UNX-F10575 | unix socket 与 epoll 就绪面（B11 前置） | 300 | 骨架 | UNX-F10575-J1 监听/连接 fd 就绪位（POLLIN=挂起连接/可读、POLLOUT=可写）与状态一致 100 次，统一 poll 接口挂接（B11 消费）签名在账 |
| UNX-F10576 | 并发连接与 backlog 溢出 | 280 | 骨架 | UNX-F10576-J1 100 并发 connect 在 backlog 内全成，溢出路径按策略返 ECONNREFUSED（unix(7) 语义注出处）10/10 |
| UNX-F10577 | 节点清理（close 不 unlink）声明 | 240 | 骨架 | UNX-F10577-J1 close 全部 socket fd 后路径节点仍在（Linux 语义声明落账），复 bind 同路径 EADDRINUSE，节点删除走显式 unlink（B1） |
| UNX-F10578 | EADDRINUSE/EISCONN/ENOTCONN 错误面 | 280 | 骨架 | UNX-F10578-J1 三错误逐条可触发（重复 bind/重复 connect/未连接 recv-send），各 10 次，错误码 errno.rs 单点 |
| UNX-F10579 | 多客户端多路复用压测 | 320 | 骨架 | UNX-F10579-J1 1 服务器 + 32 客户端并发收发 1 MB/端：零错位零串扰（每连接独立校验和），退出-重连循环 100 轮零泄漏 |
| UNX-F10580 | B09 批收口与 C2 挂号联签 | 280 | 骨架 | UNX-F10580-J1 socket/socketpair/bind/listen/accept/connect/send/recv/shutdown 族挂号登记项（table.rs 0x4000/0x4100 段）逐条核验过，资源回收断言（连接账归零）过 |

<!-- 主册行 13276 · #### UNX-C4-B10 · DGRAM/SEQPACKET/抽象命名空间/三桥预埋（F10581–F10600  -->
#### UNX-C4-B10 · DGRAM/SEQPACKET/抽象命名空间/三桥预埋（F10581–F10600 · 20 条）

> AI-14 承办｜域账累计：B01–B09 51,000 + 本批 5,400 = 56,400 / 240,000｜嫁接源：Linux unix(7)/sendto(2)/recvfrom(2)/getsockopt(2)（注出处，禁凭记忆）｜防重：DGRAM 数据报与 SysV msg 队列（B14）语义不同层（地址寻址 vs 键空间队列，F10599 联签声明）；与 proc/ipc.rs 端口总线不同抽象层；SOCK_STREAM 面 B09 已收，本批立异型 socket 面｜批注：三桥预埋（F10591）是 Varix IPC 三桥（fd 型/proc 型/fs 型）架构账；F10599 与 SCM_RIGHTS（B11）联签；F10600 收口含 C2 挂号核验。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10581 | SOCK_DGRAM 数据报边界 | 320 | 骨架 | UNX-F10581-J1 报文为最小收发单元（一 send 一 recv 完整往返 1 万报零劈），创建面（AF_UNIX+DGRAM 白名单）100/100 |
| UNX-F10582 | recvfrom/sendto 地址往返 | 300 | 骨架 | UNX-F10582-J1 sendto 指定地址→recvfrom 回填发送方地址逐字节一致（100/100），addr 参数可空（已连接态）语义账 |
| UNX-F10583 | 无连接隐式绑定 | 260 | 骨架 | UNX-F10583-J1 未 bind 的 DGRAM 首次 send 即自动隐式绑定（匿名地址可被回填 100/100），connect 后 sendto 免地址语义 10/10 |
| UNX-F10584 | SOCK_SEQPACKET 保序报文 | 300 | 骨架 | UNX-F10584-J1 SEQPACKET 面向连接+报文边界双特性可验（连接面 B09 复用断言、报文序 1 万包零乱），双型差异表落账 |
| UNX-F10585 | 报文截断 MSG_TRUNC | 260 | 骨架 | UNX-F10585-J1 接收缓冲小于报文时截断+多余丢弃（MSG_TRUNC 返真实长度），100/100 次双行为 |
| UNX-F10586 | 抽象命名空间（sun_path[0]==0） | 320 | 骨架 | UNX-F10586-J1 抽象地址 bind/connect 通路全通（不出文件系统节点断言 100/100），地址含 NUL 字节有效语义落账 |
| UNX-F10587 | 抽象命名空间互斥与自动回收 | 280 | 骨架 | UNX-F10587-J1 同名抽象地址重复 bind EADDRINUSE 10/10，全部 fd 关闭即自动回收（再 bind 即成 100/100），与文件命名空间独立账 |
| UNX-F10588 | 数据报原子性与 SO_SNDBUF 上限 | 280 | 骨架 | UNX-F10588-J1 报文全有或全无（零半报 1 万次），超 SO_SNDBUF 上限报文 EMSGSIZE 10/10，上限值账可查 |
| UNX-F10589 | EAGAIN 与接收缓冲满 | 260 | 骨架 | UNX-F10589-J1 接收缓冲满时新报文丢弃（DGRAM 无背压语义注出处）+发送端非阻塞 EAGAIN 双面，10/10 次 |
| UNX-F10590 | 广播/多播不支持声明 | 220 | 骨架 | UNX-F10590-J1 sendto 空地址/组播语义路径显性拒绝（EOPNOTSUPP/ENOSYS 红账），无静默成功探针全过 |
| UNX-F10591 | unix socket 三桥预埋（fd 型/proc 型/fs 型架构账） | 320 | 骨架 | UNX-F10591-J1 三桥架构账落定（fd 型=本批 socket、proc 型=B01 管道/信号族、fs 型=B1 节点面），各桥接口签名与消费批次登记齐备 |
| UNX-F10592 | DGRAM 保序声明（本地单发者 FIFO） | 240 | 骨架 | UNX-F10592-J1 单发送者对单接收者报文 FIFO 保序（1 万包序号零乱），多发送者交错序声明（不作保序判据）落账 |
| UNX-F10593 | MSG_CTRUNC 控制消息截断（B11 前置） | 240 | 骨架 | UNX-F10593-J1 控制缓冲过小时 MSG_CTRUNC 置位（cmsg 空间不足显性化），普通报文零误伤（反向 100/100） |
| UNX-F10594 | 零长度数据报 | 240 | 骨架 | UNX-F10594-J1 零长报文作为完整报文收发（与 EOF 区分声明），往返 100/100，与流式 0 字节语义差异表落账 |
| UNX-F10595 | DGRAM 与 epoll 就绪面挂接 | 260 | 骨架 | UNX-F10595-J1 DGRAM fd 就绪位（有报文→POLLIN、缓冲满→POLLOUT 灭）与状态一致 100 次，统一接口签名（B11 消费）在账 |
| UNX-F10596 | SEQPACKET ECONNRESET 对端崩溃 | 260 | 骨架 | UNX-F10596-J1 对端异常终止（未被读的报文在途）时本端 ECONNRESET（unix(7) 语义注出处）10/10，正常关闭链不误报（反向） |
| UNX-F10597 | 抽象与文件命名空间互不可见 | 260 | 骨架 | UNX-F10597-J1 同名抽象地址与文件路径地址并存互不冲突（双向 bind 成功 100/100），跨域 connect 拒绝路径落账 |
| UNX-F10598 | SO_TYPE/SO_DOMAIN/SO_PROTOCOL getsockopt | 240 | 骨架 | UNX-F10598-J1 三选项返回与创建属性一致（100/100），SO_SNDBUF/SO_RCVBUF 查询面（值域账）10/10，坏选项 ENOPROTOOPT |
| UNX-F10599 | SCM_RIGHTS 前置与 B14 防重联签 | 260 | 骨架 | UNX-F10599-J1 SCM_RIGHTS 传递在 DGRAM/SEQPACKET 与 STREAM 三型可用性声明落账，与 SysV msg（B14）防重联签（不同层声明+判据边界） |
| UNX-F10600 | B10 批收口与 C2 挂号核验 | 280 | 骨架 | UNX-F10600-J1 DGRAM/SEQPACKET 挂号项核验（table.rs 三对应），sendto/recvfrom/getsockopt 族登记核验过，资源回收断言过 |

<!-- 主册行 13303 · #### UNX-C4-B11 · SCM_RIGHTS fd 传递/epoll 红黑树主体（F10601–F10620 -->
#### UNX-C4-B11 · SCM_RIGHTS fd 传递/epoll 红黑树主体（F10601–F10620 · 20 条）

> AI-14 承办｜域账累计：B01–B10 56,400 + 本批 5,960 = 62,360 / 240,000｜嫁接源：Linux unix(7)/cmsg(3)/epoll(7)/recvmsg(2)（注出处，禁凭记忆）｜防重：SCM_RIGHTS 是 socket 控制消息面（不是 fd 表本体——fd 表在 ring3.rs，本批联签消费）；epoll 红黑树是独立多路复用结构（与 select/poll 适配层 B12 不同层）｜批注：SCM_RIGHTS 是三桥账（F10591）fd 型桥的核心互通面；epoll 统一 poll 面的第四消费方（pty F10492/socket 流式 F10575/DGRAM F10595 已就位）；F10617 联签 C2 挂号（epoll 族）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10601 | cmsg 结构（CMSGHDR 对齐） | 320 | 骨架 | UNX-F10601-J1 cmsghdr 布局（len/level/type）与 cmsg(3) 注出对齐规则逐字段一致，offsetof/CMSG_ALIGN 断言全过，缓冲游标面正确 |
| UNX-F10602 | SCM_RIGHTS 发送侧 fd 注入 | 340 | 骨架 | UNX-F10602-J1 sendmsg 携 SCM_RIGHTS：fd 表引用原子获取+引用计数+1（快照账），报文与控制消息一体入队 100/100 |
| UNX-F10603 | 接收侧 fd 分配与安装 | 340 | 骨架 | UNX-F10603-J1 recvmsg 时逐 fd 安装进接收者 fd 表（新 fd 号可查 100/100），安装失败回滚（半装态探针零命中） |
| UNX-F10604 | fd 引用计数与安装原子性 | 300 | 骨架 | UNX-F10604-J1 传递全程 fd 引用计数账（发送者-0/接收者+1/内核暂持+1）三态对平，在途 fd 不因发送者 close 失效（10/10） |
| UNX-F10605 | MSG_CMSG_CLOEXEC | 260 | 骨架 | UNX-F10605-J1 旗标下安装的 fd 全部带 CLOEXEC 位（exec 后自动关断 100/100），无旗标位面保留（对照 10/10） |
| UNX-F10606 | SCM_CREDENTIALS 预埋（显式 ENOSYS） | 240 | 骨架 | UNX-F10606-J1 SCM_CREDENTIALS 发送/接收路径显式 ENOSYS 红账，SCM_RIGHTS 路径零误伤（反向 100/100） |
| UNX-F10607 | 半关闭时携带 fd 的 flush 语义 | 280 | 骨架 | UNX-F10607-J1 带 fd 报文在发送端 shutdown 后仍可被接收（在途 flush 100/100），连接销毁时未收 fd 报文的 fd 关闭账（泄漏防线） |
| UNX-F10608 | epoll_create1 红黑树主体 | 360 | 骨架 | UNX-F10608-J1 epoll fd 创建（EPOLL_CLOEXEC 位面），红黑树结构（插入/查找/删除 10 万次零失衡），interest list 与 fd 账一致 |
| UNX-F10609 | EPOLL_CTL_ADD/MOD/DEL | 320 | 骨架 | UNX-F10609-J1 三操作语义全对（重复 ADD EEXIST、未 ADD MOD/DEL ENOENT 各 10 次），事件集更新即时生效（100/100） |
| UNX-F10610 | 就绪链表与水平/边缘触发 | 340 | 骨架 | UNX-F10610-J1 水平触发（LT）就绪态持续上报（不消费不消失 100/100），就绪链表挂接/摘除账对平，零丢事件探针过 |
| UNX-F10611 | EPOLLET 边缘触发语义 | 320 | 骨架 | UNX-F10611-J1 ET 只在状态翻转时报一次（同态重复 wait 不重报 100/100），翻转探针（读尽再写再报）10/10 |
| UNX-F10612 | epoll_wait 三态（超时/立即/阻塞） | 300 | 骨架 | UNX-F10612-J1 timeout=0 即刻返、>0 超时返 0、-1 阻塞至事件，三态 100/100，被信号打断 EINTR 面（B03 联签）10/10 |
| UNX-F10613 | EPOLLHUP/EPOLLERR 自动上报 | 280 | 骨架 | UNX-F10613-J1 对端关闭/错误态不经 interest 掩码直接上报（掩码豁免语义 100/100），HUP 后事件不再误报（反向） |
| UNX-F10614 | EPOLLONESHOT | 260 | 骨架 | UNX-F10614-J1 一次性事件上报后自动禁用（再 wait 不报 100/100），EPOLL_CTL_MOD 重新启用即时生效（10/10） |
| UNX-F10615 | epoll 统一 poll 面（pipe/pty/socket 三源汇） | 340 | 骨架 | UNX-F10615-J1 四类 fd（pipe/pty/socket 流式/DGRAM）同树监控事件正确分流（1 万事件零串），统一接口四实现方签名一致 |
| UNX-F10616 | fd 关闭自动摘除语义 | 280 | 骨架 | UNX-F10616-J1 被监控 fd 全部引用关闭后树内条目失效（再 wait 不报 100/100），"隐形摘除"声明（Linux epoll 语义注出处）与显式 DEL 双路账 |
| UNX-F10617 | 大量 fd 压测与 C2 挂号联签 | 320 | 骨架 | UNX-F10617-J1 10k fd 全监控 wait 正确（分层扫描账），epoll 族挂号登记项（table.rs 0x4000 段）三对应核验过 |
| UNX-F10618 | EPOLLEXCLUSIVE 预埋 | 240 | 骨架 | UNX-F10618-J1 旗标解析面就位、行为显性未实现（ENOSYS 红账），单唤醒者语义声明落账（惊群防线预告） |
| UNX-F10619 | epoll fd 自身不可 poll 声明 | 220 | 骨架 | UNX-F10619-J1 epoll fd 加入另一 epoll 树被拒（EPERM，Linux 语义注出处）10/10，自监控死循环探针零命中 |
| UNX-F10620 | B11 批收口（SCM+epoll 联合场） | 300 | 骨架 | UNX-F10620-J1 "fd 传递 + epoll 服务循环"联合场（32 客户端传 fd 1000 轮）零泄漏零误装，收口账（树规模/引用账归零）过 |

<!-- 主册行 13330 · #### UNX-C4-B12 · select/poll 适配/futex 四操作与竞态再验（F10621–F1064 -->
#### UNX-C4-B12 · select/poll 适配/futex 四操作与竞态再验（F10621–F10640 · 20 条）

> AI-14 承办｜域账累计：B01–B11 62,360 + 本批 5,780 = 68,140 / 240,000｜嫁接源：Linux select(2)/poll(2)/pselect(2)/futex(2)（注出处，禁凭记忆）｜防重：select/poll 是 epoll 统一 poll 面之上的"适配层"（复用 B11 就绪面与等待机制，不另建监控结构）；futex 是线程同步原语（与 A3 调度联签 F10633，与 fd 无关）——两个主题同批不混账，判据逐条可辨｜批注：lost-wakeup 防线（F10636）是 futex 竞态的总防线条（先查后挂序在 F10492/F10575/F10612 已铺）；F10640 收口含 C2 挂号（select/poll/futex 族）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10621 | select() fd_set 位图主体 | 320 | 骨架 | UNX-F10621-J1 三集（readfds/writefds/exceptfds）语义全对（就绪置位/未就绪清零 100/100），返回值==就绪 fd 总数对账 1 万轮 |
| UNX-F10622 | FD_ZERO/FD_SET/FD_CLR/FD_ISSET 宏面 | 240 | 骨架 | UNX-F10622-J1 四宏行为与位图操作语义一致（100/100），越界 fd 面显性（兼容层宏行为账） |
| UNX-F10623 | select 超时与 timeval 结构 | 260 | 骨架 | UNX-F10623-J1 timeout=0 轮询态、NULL 永阻塞、有限超时返 0 三态 100/100，超时值被内核更新（剩余时间语义，注 select(2)）10/10 |
| UNX-F10624 | poll() pollfd 数组主体 | 300 | 骨架 | UNX-F10624-J1 pollfd 数组（fd/events/revents）语义全对（revents 回填 100/100），events=0 仅报错误/挂起类（语义账） |
| UNX-F10625 | POLLIN/POLLOUT/POLLHUP/POLLERR/POLLNVAL | 300 | 骨架 | UNX-F10625-J1 五事件位与统一 poll 面就绪位一致（1 万次对平），POLLNVAL（坏 fd）独立于 events 请求（10/10） |
| UNX-F10626 | select/poll 到 epoll 适配层 | 340 | 骨架 | UNX-F10626-J1 适配层复用统一 poll 面就绪接口（零独立监控结构断言），select/poll 与 epoll 同场景行为一致（交叉 1 万轮零差异） |
| UNX-F10627 | pselect/ppoll 信号掩码原子切换（C6 联签） | 280 | 骨架 | UNX-F10627-J1 掩码原子切换（查-改-复原子序）零竞窗（1 万次），C6 联签（sigprocmask 接口）签名一致，信号打断 EINTR 面 |
| UNX-F10628 | nfds 上限与 FD_SETSIZE | 240 | 骨架 | UNX-F10628-J1 FD_SETSIZE=1024 边界（越界宏行为声明落账），poll 无硬上限（数组长度账），两面对照表 |
| UNX-F10629 | futex WAIT/WAKE 双操作 | 340 | 骨架 | UNX-F10629-J1 WAIT 值不匹配即返 EAGAIN（1 万次），WAKE 唤醒数==实际等待数对账，双操作 100 轮零丢失 |
| UNX-F10630 | futex 键与基址判定 | 300 | 骨架 | UNX-F10630-J1 uaddr→键映射（页基址判定 uaccess 联签）一致（同址同键 100/100），跨进程同物理页同键（共享面声明）10/10 |
| UNX-F10631 | FUTEX_PRIVATE_FLAG | 260 | 骨架 | UNX-F10631-J1 私有旗标下键空间分立（私/公同址不同键 100/100），私有面开销账（实测记录），误用跨进程面行为声明 |
| UNX-F10632 | FUTEX_WAIT_BITSET 预埋 | 260 | 骨架 | UNX-F10632-J1 WAIT_BITSET/FUTEX_CLOCK_REALTIME 显性 ENOSYS 红账，WAIT/WAKE 路径零误伤（反向 100/100） |
| UNX-F10633 | futex 与 A3 调度联签（唤醒路径） | 320 | 骨架 | UNX-F10633-J1 等待/唤醒经 A3 阻塞-唤醒接口（F10473 同族），状态字四拍账单调，唤醒延迟 ktest 粒度内 100/100 |
| UNX-F10634 | futex EAGAIN/EINTR/EFAULT 错误面 | 280 | 骨架 | UNX-F10634-J1 三错误逐条可触发（EAGAIN 值变/EINTR 信号打断/EFAULT 坏指针）各 10 次，errno.rs 单点 |
| UNX-F10635 | futex requeue 预埋 | 240 | 骨架 | UNX-F10635-J1 FUTEX_REQUEUE/CMP_REQUEUE 显性 ENOSYS 红账，WAIT/WAKE 零误伤（反向），条件变量实现影响声明 |
| UNX-F10636 | lost-wakeup 竞态防线 | 340 | 骨架 | UNX-F10636-J1 先查后挂序单点（查值与挂队列原子序断言），竞态注入 10 万次零丢失，与 B09/B11/F10612 同序核验 |
| UNX-F10637 | poll 面与 pty/termios 交错 | 280 | 骨架 | UNX-F10637-J1 pty fd poll 与模式切换/背压/终态交错 1000 轮行为一致（就绪位与状态对平），termios 变更即时反映 |
| UNX-F10638 | futex 数量上限与内存账 | 260 | 骨架 | UNX-F10638-J1 键表上限触顶 EAGAIN（声明值），等待队列内存账与 B2 heap 对平，回收断言（清场归零） |
| UNX-F10639 | 100 线程 futex 唤醒风暴压测 | 320 | 骨架 | UNX-F10639-J1 100 线程 WAIT/WAKE 风暴（1 万轮）：零丢失零死锁（120 秒完成线），唤醒计数守恒，A3 联签链账全绿 |
| UNX-F10640 | B12 批收口与 C2 挂号联签 | 300 | 骨架 | UNX-F10640-J1 select/pselect/poll/ppoll/futex 族挂号项（0x4000/0x4100 段）三对应核验，跨批竞态防线核验账（先查后挂序四方一致） |

<!-- 主册行 13357 · #### UNX-C4-B13 · SysV shm：键空间/挂接/清扫/泄漏账（F10641–F10660 · 20  -->
#### UNX-C4-B13 · SysV shm：键空间/挂接/清扫/泄漏账（F10641–F10660 · 20 条）

> AI-14 承办｜域账累计：B01–B12 68,140 + 本批 5,560 = 73,700 / 240,000｜嫁接源：Linux sysvipc(7)/shmget(2)/shmat(2)/shmctl(2)（注出处，禁凭记忆）｜防重：SysV shm 与 POSIX mmap MAP_SHARED（C2 mem 面）语义不同层——shm 走键空间+显式挂接生命周期，MAP_SHARED 走地址空间面（F10654 联签声明共享底层）；与 futex 共键（F10630 共页同键判据）联签｜批注：页引用与 pmm.rs（B2）联签是本批地基；F10658 联签 C2 挂号（ipc 族）；IPC_64 版本面（F10678 预告）在 B14。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10641 | shmget 键空间与 IPC_PRIVATE | 300 | 骨架 | UNX-F10641-J1 键查/建双路（现有键返同段、新键建段）100/100，IPC_PRIVATE 恒新段（10/10），段账（id/键/尺寸）可查 |
| UNX-F10642 | shmat 映射与 SHM_RDONLY | 320 | 骨架 | UNX-F10642-J1 挂接返回地址可读写（1 MB 对平），SHM_RDONLY 下写即 SIGSEGV（pfh 联签）10/10，nattch 计数+1 |
| UNX-F10643 | shmdt 解除 | 240 | 骨架 | UNX-F10643-J1 解除后访问得 SIGSEGV（页面摘除 10/10），nattch-1，重复解除 EINVAL 面 |
| UNX-F10644 | shmctl IPC_STAT/IPC_SET/IPC_RMID | 320 | 骨架 | UNX-F10644-J1 三命令行为全对（STAT 结构逐字段/SET 权限与尺寸限缩/RMID 标记），各 10 次，错误面（EACCES/EINVAL）10/10 |
| UNX-F10645 | 标记待删与最后分离销毁 | 280 | 骨架 | UNX-F10645-J1 RMID 后旧挂接仍可用（存活性 10/10）、新挂接 ENOENT（10/10）、最后 nattch=0 即销毁（内存归零） |
| UNX-F10646 | shm 段与 pmm 页引用联签（B2） | 320 | 骨架 | UNX-F10646-J1 段页走 pmm 引用计数（分配/归还账对平），多进程挂接共享物理页（同页断言 10/10），泄漏探针零容忍 |
| UNX-F10647 | shminfo 限制（SHMMAX/SHMALL） | 260 | 骨架 | UNX-F10647-J1 超限 shmget EINVALID（SHMMAX）10/10，SHMALL 总页账触顶拒绝（10/10），限值可查可设（权限面） |
| UNX-F10648 | IPC_CREAT/IPC_EXCL 语义 | 260 | 骨架 | UNX-F10648-J1 四组合（0/CREAT/EXCL/CREAT\|EXCL）行为矩阵全对（EEXIST 面），100/100 |
| UNX-F10649 | shm 与 fork 继承 | 280 | 骨架 | UNX-F10649-J1 fork 后子进程继承挂接（同地址可访问 100/100），nattch 随继承+1，父写子读可见（共享实证） |
| UNX-F10650 | shm 与 exec 脱离 | 260 | 骨架 | UNX-F10650-J1 exec 后 SHM 挂接自动脱离（nattch-1，地址空间换血 100/100），与 termios 保留（F10516）两面差异账 |
| UNX-F10651 | EIDRM/EINVAL/EACCES 错误面 | 280 | 骨架 | UNX-F10651-J1 三错误逐条可触发（RMID 后操作 EIDRM/坏 id EINVAL/越权 EACCES）各 10 次，errno.rs 单点 |
| UNX-F10652 | shm 页故障联动 pfh.rs | 300 | 骨架 | UNX-F10652-J1 shm 页故障分类（缺页装页/越权 SIGSEGV）正确路由（100/100），RO 段写故障→SIGSEGV+si_code 面联签 B04 |
| UNX-F10653 | SHM_LOCK/SHM_UNLOCK 预埋 | 240 | 骨架 | UNX-F10653-J1 两命令显性 ENOSYS 红账，STAT/RMID 路径零误伤（反向 100/100） |
| UNX-F10654 | shm 与 mmap MAP_SHARED 等价性声明 | 280 | 骨架 | UNX-F10654-J1 底层页共享机制同源声明（联签账），shm 面与 mmap 面行为差异表（生命周期/键空间）落册 |
| UNX-F10655 | shm_nattch 计数账 | 240 | 骨架 | UNX-F10655-J1 nattch 与实际挂接数恒等（挂/离/继承/脱离全路径 1 万次对平），STAT 可见 |
| UNX-F10656 | 大段压测（64MB 段读写） | 300 | 骨架 | UNX-F10656-J1 64 MB 段 8 进程并发读写零错位（分区校验和），页账对平，120 秒完成线 |
| UNX-F10657 | shm 与能力位 guard.rs 联签 | 260 | 骨架 | UNX-F10657-J1 权限判定经 guard.rs 单点（读写/挂接面 100/100），越权账（uid/gid/mode 三要素）落册 |
| UNX-F10658 | SysV IPC key 空间与 ftok 声明（C2 联签） | 260 | 骨架 | UNX-F10658-J1 三族（shm/sem/msg）键空间分立账，ftok 声明（libc 面算法，内核只管键值），挂号项三对应 |
| UNX-F10659 | ipc_perm 权限字与属主 | 280 | 骨架 | UNX-F10659-J1 perm 结构（uid/gid/cuid/cgid/mode）逐字段正确（100/100），属主语义（创建者 vs 所有者分离）账 |
| UNX-F10660 | B13 批收口（shm 泄漏总账） | 280 | 骨架 | UNX-F10660-J1 批级泄漏总账（段账/页账/挂接账三归零），挂号核验过，B14 交接（sem/msg 同键空间族）清单 |

<!-- 主册行 13384 · #### UNX-C4-B14 · SysV sem/msg：semop-undo/类型选择接收（F10661–F106 -->
#### UNX-C4-B14 · SysV sem/msg：semop-undo/类型选择接收（F10661–F10680 · 20 条）

> AI-14 承办｜域账累计：B01–B13 73,700 + 本批 5,760 = 79,460 / 240,000｜嫁接源：Linux sysvipc(7)/semget(2)/semop(2)/semctl(2)/msgget(2)/msgsnd(2)/msgrcv(2)/msgctl(2)（注出处，禁凭记忆）｜防重：sem 是内核计数同步原语（与 futex 不同层：sem 内核态阻塞、futex 用户态值守）；msg 是键空间消息队列（与 unix DGRAM 不同层：F10599 联签，键寻址 vs 地址寻址）｜批注：semundo 回滚账（F10664）是进程退出清理联签（C1）核心；F10679 联签 SysV/POSIX IPC 防重总声明；F10680 收口含 C2 挂号核验。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10661 | semget 信号量集 | 300 | 骨架 | UNX-F10661-J1 键查/建双路（同 shm 键册分立断言）100/100，集内 nsems 上限账，IPC_CREAT/EXCL 四格矩阵复用 F10648 全对 |
| UNX-F10662 | semctl GETVAL/SETVAL/IPC_STAT | 300 | 骨架 | UNX-F10662-J1 三命令行为全对（GET/SET 往返 100/100、STAT 逐字段），union semun 参数面（uaccess 判界）正确 |
| UNX-F10663 | semop 原子操作数组 | 340 | 骨架 | UNX-F10663-J1 多操作原子提交（全成或全不落 1 万次），sembuf 数组逐项执行序正确，阻塞/非阻塞双面 |
| UNX-F10664 | SEM_UNDO 回滚账 | 300 | 骨架 | UNX-F10664-J1 UNDO 挂账（每操作登记）→进程退出回滚（C1 联签）→semval 复原（100/100），退出前显式回滚账对平 |
| UNX-F10665 | semval/semncnt/semzcnt 计数 | 280 | 骨架 | UNX-F10665-J1 三计数与实际状态恒等（等待/操作 1 万次对平），GETNCNT/GETZCNT 命令面正确 |
| UNX-F10666 | sem EAGAIN/EIDRM/EINTR 错误面 | 280 | 骨架 | UNX-F10666-J1 三错误逐条（NOWAIT 不满足 EAGAIN/RMID EIDRM/信号 EINTR）各 10 次，errno.rs 单点 |
| UNX-F10667 | 信号量集原子性与并发压测 | 320 | 骨架 | UNX-F10667-J1 64 进程争用 8 元素集 1 万轮：零超卖（计数永不负）、零死锁（120 秒线）、原子性断言全绿 |
| UNX-F10668 | msgget 消息队列 | 280 | 骨架 | UNX-F10668-J1 键查/建双路（分册断言）100/100，队列账（id/键/容量）可查，CREAT/EXCL 矩阵全对 |
| UNX-F10669 | msgsnd 阻塞/NOWAIT | 300 | 骨架 | UNX-F10669-J1 队满阻塞至有位（10/10）、NOWAIT 满时 EAGAIN（10/10），消息整体入队（原子 100/100） |
| UNX-F10670 | msgrcv 类型选择（typ>0/0/<0） | 340 | 骨架 | UNX-F10670-J1 三选择语义全对（精确类型/先进先出/类别下限）各 1 千次，选择后队列序保持 |
| UNX-F10671 | msg 消息截断 MSG_NOERROR | 260 | 骨架 | UNX-F10671-J1 无旗标大消息 E2BIG（10/10）、MSG_NOERROR 截断（10/10）、截断量账 |
| UNX-F10672 | msgctl IPC_STAT/IPC_SET/IPC_RMID | 280 | 骨架 | UNX-F10672-J1 三命令全对（STAT 逐字段/SET 权限/RMID 清队），各 10 次，错误面 10/10 |
| UNX-F10673 | EIDRM 广播唤醒 | 280 | 骨架 | UNX-F10673-J1 RMID 时队列上全部等待者得 EIDRM（挂起中 sem/msg 双面），等待数==唤醒数对账 10/10 |
| UNX-F10674 | msg 与 unix DGRAM 防重声明（F10599 核销） | 260 | 骨架 | UNX-F10674-J1 两层差异表（键/地址、队列/socket、类型选择/无）落册，DGRAM 判据零越界（grep 断言），F10599 联签核销 |
| UNX-F10675 | msg 队列上限（msgmax/msgmnb） | 260 | 骨架 | UNX-F10675-J1 单消息超 msgmax EAGAIN/EINVAL（10/10）、队列超 msgmnb 满（10/10），限值账可查 |
| UNX-F10676 | sem/msg 与能力位 guard.rs 联签 | 240 | 骨架 | UNX-F10676-J1 权限判定全走 guard.rs 单点（100/100），越权账（三要素）与 B13 同款 |
| UNX-F10677 | 混合 IPC 压测（shm+sem 生产者消费者） | 340 | 骨架 | UNX-F10677-J1 8 生产者 8 消费者 × shm 缓冲 + sem 同步 1 万轮：零丢项零重复（流水号守恒），UNDO 回滚链账全绿 |
| UNX-F10678 | IPC_64 版本面声明 | 240 | 骨架 | UNX-F10678-J1 IPC_64 旗标语义声明（Varix 原生 64 位布局、旗标透明接受），结构断言全过 |
| UNX-F10679 | SysV 与 POSIX IPC 防重总声明（B15 前置） | 280 | 骨架 | UNX-F10679-J1 两族总对照表（键/fd、ctl/attr、遗留性声明）落册，POSIX 面（B15）消费清单登记 |
| UNX-F10680 | B14 批收口与 C2 挂号核验 | 280 | 骨架 | UNX-F10680-J1 sem/msg 十二族挂号项三对应核验（0x4000/0x4100 段），批级资源清场（集账/队账归零），B15 交接清单 |

<!-- 主册行 13411 · #### UNX-C4-B15 · POSIX sem/mq/eventfd/timerfd/混合压测场（F10681– -->
#### UNX-C4-B15 · POSIX sem/mq/eventfd/timerfd/混合压测场（F10681–F10700 · 20 条）

> AI-14 承办｜域账累计：B01–B14 79,460 + 本批 6,220 = 85,680 / 240,000｜嫁接源：Linux sem_overview(7)/sem_open(3)/mq_overview(7)/eventfd(2)/timerfd_create(2)（注出处，禁凭记忆）｜防重：POSIX 命名 sem 与 SysV sem 不同族（F10679 总表核销）；mq 与 SysV msg 不同族（fd 面 vs 键面）；eventfd/timerfd 是"fd 化的计数器/计时器"（与 pipe 防重：eventfd 计数语义非字节流）｜批注：本批为 B01–B15（F 型地基+M 型机制）总收口批——三场混合压测（F10695-F10697）+ 判据主轴总回归（F10698）+ LTP 映射收口（F10699）+ 域移交声明（F10700）；B16–B40 待后续会话承接。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10681 | sem_open 命名信号量与 /dev/shm 声明 | 300 | 骨架 | UNX-F10681-J1 名字空间（/name 格式）查/建双路 100/100，挂载点声明（Varix /dev/shm 面联签 B1）落账，非法名 EINVAL 面 |
| UNX-F10682 | sem_wait/sem_trywait/sem_post | 320 | 骨架 | UNX-F10682-J1 三操作行为全对（wait 阻塞/trywait EAGAIN/post 唤醒 各 1 千次），EINTR 面（B03 联签）10/10 |
| UNX-F10683 | sem 链接计数与 unlink 延迟销毁 | 280 | 骨架 | UNX-F10683-J1 unlink 后旧打开句柄可用、新 open ENOENT（10/10），引用归零即销毁（账对平） |
| UNX-F10684 | sem_getvalue | 240 | 骨架 | UNX-F10684-J1 返回值与实际计数恒等（操作 1 万次对平），负值语义（等待者数，注 sem_getvalue(3)）账 |
| UNX-F10685 | mq_open 消息队列描述字 | 300 | 骨架 | UNX-F10685-J1 mq_open 查/建双路 100/100，O_CREAT+attr 容量面，O_NONBLOCK fd 态面 |
| UNX-F10686 | mq_send/mq_receive 优先级 | 320 | 骨架 | UNX-F10686-J1 优先级队列（高先出 1 万消息），同优先级 FIFO 保持，两级序账对平 |
| UNX-F10687 | mq_notify 预埋（显式 ENOSYS） | 240 | 骨架 | UNX-F10687-J1 mq_notify 显性 ENOSYS 红账，send/recv 零误伤（反向 100/100） |
| UNX-F10688 | mq_setattr/mq_getattr | 260 | 骨架 | UNX-F10688-J1 attr 结构逐字段正确（100/100），NONBLOCK 切换即时生效（10/10），curmsgs 对平 |
| UNX-F10689 | eventfd 计数器（EFD_SEMAPHORE） | 320 | 骨架 | UNX-F10689-J1 write 累加/read 取值（普通态整取、SEMAPHORE 态减一）双面各 1 万次，计数上限 EINVAL 面 |
| UNX-F10690 | eventfd 非阻塞与 EAGAIN | 260 | 骨架 | UNX-F10690-J1 EFD_NONBLOCK 下零值读 EAGAIN（1 万次），阻塞态唤醒链（写入即醒 100/100） |
| UNX-F10691 | timerfd 计时器面 | 300 | 骨架 | UNX-F10691-J1 create/settime/time 三面全对（过期次数读出），TFD_NONBLOCK/CLOEXEC 位面，精度账（ktest 粒度） |
| UNX-F10692 | timerfd 与 A3 时钟联签 | 260 | 骨架 | UNX-F10692-J1 定时触发经 A3 时钟接口（签名核验），过期→就绪→读出链账四拍单调 |
| UNX-F10693 | eventfd/timerfd 与 epoll 统一面 | 300 | 骨架 | UNX-F10693-J1 两 fd 就绪面挂接（第五/六实现方），epoll 混合监控事件正确分流（1 万次） |
| UNX-F10694 | POSIX IPC 与 SysV IPC 对照账（F10679 核销） | 280 | 骨架 | UNX-F10694-J1 对照账逐行核销（sem 族/mq 族四行），两族判据零越界（grep 断言） |
| UNX-F10695 | 混合压测场一：pty vim + unix socket + epoll 组合 | 380 | 骨架 | UNX-F10695-J1 全屏模拟器（pty+raw）+ socket 服务循环（epoll）同场 1 万轮：零串扰零丢失，录制器对照全绿 |
| UNX-F10696 | 混合压测场二：多进程管道 + futex + shm | 380 | 骨架 | UNX-F10696-J1 16 进程（管道族间通信+futex 同步+shm 共享缓冲）1 万轮流水线：守恒全绿零死锁 |
| UNX-F10697 | 混合压测场三：信号风暴 + IPC 压测 | 380 | 骨架 | UNX-F10697-J1 信号风暴（实时信号 1 万发）叠加 IPC 压测：零丢失零错递，EINTR 矩阵全绿 |
| UNX-F10698 | 判据主轴总回归（vim 录制对照全链） | 400 | 骨架 | UNX-F10698-J1 主轴全链（pty vim 类全屏：raw 切换/按键流/重绘/SIGWINCH/^Z/fg）录制对照 10 场景零差异 |
| UNX-F10699 | LTP 全族映射账收口 | 340 | 骨架 | UNX-F10699-J1 C4 域 LTP 映射账（pipe/signal/pty/socket/futex/ipc 族）覆盖/待补两态收口，待补项带原因码 |
| UNX-F10700 | B15 与域 C4 收官移交（B16–B40 待领声明） | 360 | 骨架 | UNX-F10700-J1 域账收口（85,680/240,000 · 300 条 finalize 全过），B16–B40 待领交接清单（主题批注齐备），挂号面全域核验账 |

<!-- 主册行 13438 · #### UNX-C4-B16 · unix socket 连接建立深水（F10701–F10720 · 20 条） -->
#### UNX-C4-B16 · unix socket 连接建立深水（F10701–F10720 · 20 条）

> AI-14 承办｜域账累计：B01–B15 85,680 + 本批 6,500 = 92,180 / 240,000｜嫁接源：Linux socket(2)/connect(2)/listen(2)/accept(2)/accept4(2)/shutdown(2)/unix(7)（注出处，禁凭记忆）｜防重：与 B09 流式册防重：B09 立 bind/listen/accept 主链正向面，本批立连接建立深水（backlog 窗口/非阻塞 connect/shutdown/close 语义），判据面不重叠；与 B10 DGRAM/SEQPACKET 防重：本批仅 stream 型连接面（seqpacket 三态 connect 对照注账不复判）｜批注：本批 B16 开批：B09–B20 M 型机制收尾段第 1/5 批——连接建立深水（backlog 窗口/非阻塞 connect/shutdown 三态/close 终止/竞争 accept）；B17–B20 随后

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10701 | listen backlog 队列深度语义 | 350 | 骨架 | UNX-F10701-J1 backlog 上限生效（somaxconn 截断 3 档 ×100/100），队列深度可观测（挂起数对账 1 千次），排满行为注账（选定拒绝面） |
| UNX-F10702 | 非阻塞 connect 与 EINPROGRESS | 350 | 骨架 | UNX-F10702-J1 O_NONBLOCK 下 connect 立即返 EINPROGRESS（1 千次），后台连接状态机三态迁移账（进行中/已立/已拒）对平 |
| UNX-F10703 | connect 完成检测（SO_ERROR/POLLOUT） | 340 | 骨架 | UNX-F10703-J1 SO_ERROR 取零=已立/取错=拒绝（双路各 500 次），取出后清零（二次读 10/10），POLLOUT 就绪与完成同步（100/100） |
| UNX-F10704 | 并发 connect 同一地址竞态 | 320 | 骨架 | UNX-F10704-J1 N 进程并发 connect 同一监听端（N=32）全部按序入队零丢失（1 千轮），fd 互不串扰（对账 grep） |
| UNX-F10705 | connect 自连（同一 socket 连自身地址）语义 | 300 | 骨架 | UNX-F10705-J1 自连行为注账（Varix 选定允许并走完整握手面 100/100），回环数据通路验证（收发对平 1 千次） |
| UNX-F10706 | accept 队列溢出行为（重传窗口） | 330 | 骨架 | UNX-F10706-J1 溢出期间 connect 拒绝计数对账（100/100），accept 排空后恢复接新（100/100），溢出事件账可查 |
| UNX-F10707 | accept4 SOCK_CLOEXEC/SOCK_NONBLOCK | 340 | 骨架 | UNX-F10707-J1 accept4 双标志位生效（CLOEXEC 经 fork+exec 验证 10/10、NONBLOCK 即时 100/100），与 fcntl 后置等价性对账 |
| UNX-F10708 | listen 未 bind 的 EINVAL 路径 | 300 | 骨架 | UNX-F10708-J1 未 bind 直接 listen→EINVAL（10/10，流式），DGRAM 型 listen 语义注账（允许/对照差异） |
| UNX-F10709 | 重复 listen 返回值 | 280 | 骨架 | UNX-F10709-J1 已 listen fd 二次 listen 成功且可更新 backlog（100/100），队列内容保持（对平） |
| UNX-F10710 | accept 对端地址长度截断 | 320 | 骨架 | UNX-F10710-J1 addrlen 足额返回完整地址（100/100），addrlen 过小截断且按传入值回写（10/10），匿名对端零长度注账 |
| UNX-F10711 | shutdown SHUT_WR 半关闭 | 340 | 骨架 | UNX-F10711-J1 SHUT_WR 后写端 EPIPE 面（100/100）、读端收 EOF、对端读净残余后 EOF（对平 1 千次），写端读不受影响（100/100） |
| UNX-F10712 | shutdown SHUT_RD 与 EOF 传播 | 300 | 骨架 | UNX-F10712-J1 SHUT_RD 后本端读立即 EOF（100/100），对端继续写不报错（接收侧丢弃注账 100/100），恢复不可行注账 |
| UNX-F10713 | shutdown 三态组合矩阵 | 320 | 骨架 | UNX-F10713-J1 SHUT_RD/WR/RDWR 三态×读写行为矩阵全对（9 格 ×100/100），二次 shutdown 幂等（10/10） |
| UNX-F10714 | 对端未 accept 的数据缓冲窗口 | 340 | 骨架 | UNX-F10714-J1 已立连接未 accept 期间对端可写（数据入连接缓冲 1 千条对平），accept 后全量可读（序账保持），窗口上限注账 |
| UNX-F10715 | close 连接终止与 SO_LINGER 占位 | 320 | 骨架 | UNX-F10715-J1 close 即终止：未读数据丢弃、对端读 EOF（100/100），SO_LINGER 显性 ENOPROTOOPT 注账（红账预埋族） |
| UNX-F10716 | dup 后监听 fd 的 accept 语义 | 300 | 骨架 | UNX-F10716-J1 dup 出的 fd 同样可 accept（100/100），新连接 fd 独立于监听 fd 副本（隔离账 1 千次），close 一份监听不失效 |
| UNX-F10717 | fork 后监听 socket 共享与竞争 accept | 300 | 骨架 | UNX-F10717-J1 N 子进程竞争 accept（N=8）每连接恰派发一个（1 千轮零重复零丢失），惊群行为注账（选定单唤醒） |
| UNX-F10718 | connect 到正在关闭的监听端竞态 | 360 | 骨架 | UNX-F10718-J1 close 监听期间 connect 注入（×100 竞态窗口）：接住=正常建立、未接=拒绝，两侧对账零悬挂（EINPROGRESS 清扫） |
| UNX-F10719 | backlog=0 边界（单挂起连接） | 310 | 骨架 | UNX-F10719-J1 backlog=0 允许恰一个挂起连接（100/100），第二个 connect 排满面拒绝（100/100），accept 后立即恢复 |
| UNX-F10720 | 连接建立段总收口（B16 断言聚合） | 380 | 骨架 | UNX-F10720-J1 F10701–F10719 十九条判据全量回归（断言聚合 380 项全绿），段内域累计账对平（85,680+6,500=92,180） |

<!-- 主册行 13465 · #### UNX-C4-B17 · unix socket 地址与关闭语义（F10721–F10740 · 20 条） -->
#### UNX-C4-B17 · unix socket 地址与关闭语义（F10721–F10740 · 20 条）

> AI-14 承办｜域账累计：B01–B15 92,180 + 本批 6,400 = 98,580 / 240,000｜嫁接源：Linux unix(7)/bind(2)/getsockname(2)/getpeername(2)/unlink(2)/stat(2)（注出处，禁凭记忆）｜防重：与 B10 防重：B10 立抽象命名空间建立与三桥预埋，本批立地址族全语义（明名/匿名/长度/权限/生命周期），判据面以「文件系统侧生命周期」为主与 B10 的「命名空间面」分工；与 B09 防重：B09 立 bind 正向主链，本批立地址边界与竞态｜批注：本批 B17：M 型机制收尾段第 2/5 批——地址族全语义（明名/抽象/匿名/长度/权限/生命周期/unlink 竞态/重绑拒绝）；B18–B20 随后

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10721 | sockaddr_un sun_path 绝对路径绑定 | 350 | 骨架 | UNX-F10721-J1 绝对路径 bind 成功（100/100），文件落盘且类型为 socket（stat 10/10），getsockname 回读一致（100/100） |
| UNX-F10722 | 相对路径与 108 字节限制（含 NUL） | 340 | 骨架 | UNX-F10722-J1 相对路径可 bind（100/100），107 字节路径+NUL 成功（100/100），108 字节无 NUL 位注账边界（判定 10/10） |
| UNX-F10723 | 地址过长 EINVAL | 300 | 骨架 | UNX-F10723-J1 超长地址（>108 含 NUL 口径）bind/connect/sendto 三口均 EINVAL（各 10/10），判定序一致（对账） |
| UNX-F10724 | 绑定文件的权限语义（umask 生效） | 330 | 骨架 | UNX-F10724-J1 bind 期间 umask 生效（mode&~umask 三组 10/10），权限面经 guard.rs 校验（拒绝/允许 ×10/10） |
| UNX-F10725 | bind 已存在路径 EADDRINUSE | 300 | 骨架 | UNX-F10725-J1 同路径二次 bind→EADDRINUSE（10/10），现存节点为非 socket 文件也拒绝（10/10），unlink 后可重绑（联 F10727） |
| UNX-F10726 | close 不自动删除地址文件 | 320 | 骨架 | UNX-F10726-J1 close 监听端后路径文件仍存在（100/100），残留文件导致重绑失败（联 F10725 10/10），生命周期差异注账 |
| UNX-F10727 | unlink 清理与重绑生命周期 | 340 | 骨架 | UNX-F10727-J1 unlink 后文件消失、重绑成功（100/100），unlink 后旧监听继续工作（句柄独立 100/100）， unlink 非 socket 文件成功注账（通用面） |
| UNX-F10728 | getsockname 抽象命名空间长度语义 | 330 | 骨架 | UNX-F10728-J1 抽象名 getsockname 回写（len 含 NUL 首字节口径 100/100），addrlen 过小截断回写真实长（10/10），B10 建立面联签核销 |
| UNX-F10729 | getpeername 未连接 EINVAL | 290 | 骨架 | UNX-F10729-J1 未连接 stream getpeername→ENOTCONN（10/10）、非 socket fd→ENOTSOCK（10/10）、无效 fd→EBADF（10/10）判定序 |
| UNX-F10730 | getpeername 流式对端地址 | 330 | 骨架 | UNX-F10730-J1 已连 stream 对端地址回写（bind 过对端 100/100、匿名对端零长 100/100），与 accept 出参一致性对账 |
| UNX-F10731 | 匿名对端（未 bind 客户端）零地址 | 300 | 骨架 | UNX-F10731-J1 匿名客户端 accept/getpeername 零长度地址（各 100/100），零长非错误（不截 EINVAL 面），语义注账 |
| UNX-F10732 | connect 期间地址文件被 unlink 竞态 | 350 | 骨架 | UNX-F10732-J1 unlink 与 connect 交替注入 ×100：接住=建立成功、未接=ECONNREFUSED，对账零第三态、旧监听存活不受影响 |
| UNX-F10733 | bind 权限不足 EACCES | 300 | 骨架 | UNX-F10733-J1 目录无写权限 bind→EACCES（10/10），有写权限成功对照（10/10），路径搜索权限面（guard.rs 联签）对账 |
| UNX-F10734 | socket 文件类型与 stat 验证 | 330 | 骨架 | UNX-F10734-J1 socket 文件 stat 类型=SSOCK（10/10）、nlink=1、对 open 读取拒绝面注账（非数据文件语义） |
| UNX-F10735 | 同一 fd 两次 bind（重绑） | 300 | 骨架 | UNX-F10735-J1 已 bind fd 二次 bind→EINVAL（10/10，选定语义注账），connect 后 bind→EINVAL（10/10） |
| UNX-F10736 | close 后 fd 重用的 connect 悬挂竞态 | 320 | 骨架 | UNX-F10736-J1 close+dup 新 fd 重用竞态 ×100：旧引用不可达（EBADF 面对账）、新 fd 全新对象（隔离账 100/100） |
| UNX-F10737 | SO_REUSEADDR 对照注账（unix 域占位） | 290 | 骨架 | UNX-F10737-J1 SO_REUSEADDR unix 域零作用验证（设置成功但行为不变 100/100），与 tcp 对照差异注账 |
| UNX-F10738 | 抽象命名空间冲突与前缀语义 | 330 | 骨架 | UNX-F10738-J1 同名抽象名二次 bind→EADDRINUSE（10/10），同前缀不同后缀共存（100/100），冲突判定按完整字节串（对账） |
| UNX-F10739 | 地址段逐字节可观测探针 | 300 | 骨架 | UNX-F10739-J1 三类地址（明名/抽象/匿名）探针账（形态表 100/100），回读逐字节一致（1 千次零漂移） |
| UNX-F10740 | 地址与关闭段总收口（B17 断言聚合） | 350 | 骨架 | UNX-F10740-J1 F10721–F10739 十九条判据聚合回归（350 项全绿），段累计对平（92,180+6,400=98,580），域总账移交清单更新 |

<!-- 主册行 13492 · #### UNX-C4-B18 · SOL_SOCKET 选项族全语义（F10741–F10760 · 20 条） -->
#### UNX-C4-B18 · SOL_SOCKET 选项族全语义（F10741–F10760 · 20 条）

> AI-14 承办｜域账累计：B01–B15 98,580 + 本批 6,300 = 104,880 / 240,000｜嫁接源：Linux socket(7)/unix(7)/getsockopt(2)/setsockopt(2)（注出处，禁凭记忆）｜防重：与 B16 防重：B16 立连接建立深水（SO_ERROR 完成面 F10705、close 终止+SO_LINGER 行为面 F10717），本批立选项读写面（SO_ERROR/SO_LINGER 选项面与 F10705/F10717 行为面分工注账）；与 B17 防重：B17 立地址族全语义，本批立 SOL_SOCKET 选项族，判据面零交集｜批注：本批 B18：M 型机制收尾段第 3/5 批——SOL_SOCKET 选项族（读写链/缓冲账/超时/凭据/继承/错误矩阵/并发竞态）；B19–B20 随后

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10741 | getsockopt/setsockopt 基本读写链（SO_TYPE） | 330 | 骨架 | UNX-F10741-J1 setsockopt/getsockopt 读写链 100/100，SO_TYPE 回读 SOCK_STREAM/SOCK_DGRAM 各 10/10，回读字面与 socket(2) 创建参数零偏离（对账表） |
| UNX-F10742 | SO_ACCEPTCONN 监听态判定 | 300 | 骨架 | UNX-F10742-J1 listen 后 SO_ACCEPTCONN 回读 1（10/10），listen 前 0（10/10），dgram socket 恒 0（10/10） |
| UNX-F10743 | SO_SNDBUF/SO_RCVBUF 读写与双界 | 320 | 骨架 | UNX-F10743-J1 两选项 set→get 回读一致（各 100/100），上限封顶 10/10，下限收敛 10/10，双界对账零偏离 |
| UNX-F10744 | SO_SNDBUF 双倍记账与下限收敛值 | 310 | 骨架 | UNX-F10744-J1 set v→get 返回记账值（双倍规则 10/10），下限收敛 2048→记账 4096（10/10），规则条文誊录核对 10/10 |
| UNX-F10745 | SO_RCVLOWAT 生效与 SO_SNDLOWAT ENOPROTOOPT | 320 | 骨架 | UNX-F10745-J1 SO_RCVLOWAT 设阈后 poll 就绪判定联动（10/10），SO_SNDLOWAT set→ENOPROTOOPT（10/10），差异注账在册 |
| UNX-F10746 | SO_RCVTIMEO/SO_SNDTIMEO 超时选项 | 320 | 骨架 | UNX-F10746-J1 设超时后阻塞 recv/send 到点 EAGAIN（各 10/10），零值还原无限阻塞（10/10），超时精度对账 10/10 |
| UNX-F10747 | SO_ERROR 选项面通用清零语义 | 320 | 骨架 | UNX-F10747-J1 注错后 get SO_ERROR 取错并清零（10/10），二读返回 0（10/10），与 F10705 完成面分工对账 10/10 |
| UNX-F10748 | SO_KEEPALIVE 域 socket 可设无效果 | 300 | 骨架 | UNX-F10748-J1 AF_UNIX 上 set→get SO_KEEPALIVE 回读一致（10/10），设后收发行为零差异（千次对账），注账在册 |
| UNX-F10749 | MSG_OOB EOPNOTSUPP 与 SO_OOBINLINE 无效果 | 310 | 骨架 | UNX-F10749-J1 AF_UNIX send MSG_OOB→EOPNOTSUPP（10/10），SO_OOBINLINE 可设回读一致（10/10），带外面零效果对账 10/10 |
| UNX-F10750 | SO_PASSCRED 每报文凭据附着 | 330 | 骨架 | UNX-F10750-J1 设 SO_PASSCRED 后每条报文附 SCM_CREDENTIALS cmsg（100/100），未设零附着（100/100），凭据与发送方一致 10/10 |
| UNX-F10751 | SO_PEERCRED 对端凭据读取 | 320 | 骨架 | UNX-F10751-J1 连接后 get SO_PEERCRED 三字段与对端一致（10/10），listen 端读到 accept 对端凭据（10/10），快照时点注账 10/10 |
| UNX-F10752 | SO_SNDBUFFORCE 特权守卫面 | 310 | 骨架 | UNX-F10752-J1 无特权 set FORCE→EPERM（10/10），特权路径经 guard.rs 放行（10/10），守卫判定序对账 10/10 |
| UNX-F10753 | 选项层级判定序（SOL_SOCKET/协议层） | 300 | 骨架 | UNX-F10753-J1 SOL_SOCKET 层命中 10/10，未知层 ENOPROTOOPT（10/10），未知选项同层 ENOPROTOOPT（10/10），判定序对账 10/10 |
| UNX-F10754 | accept 继承面（选项遗传/文件标志不遗传） | 330 | 骨架 | UNX-F10754-J1 listener 选项被 accept 新 fd 继承（10/10），O_NONBLOCK 不继承（10/10），继承清单对账 10/10 |
| UNX-F10755 | getsockopt optlen 截断与 EINVAL | 300 | 骨架 | UNX-F10755-J1 optlen 过小→EINVAL（10/10），optlen 过大按实际回写（10/10），出参回写对账 10/10 |
| UNX-F10756 | setsockopt 错误矩阵（EBADF/EINVAL/EFAULT） | 310 | 骨架 | UNX-F10756-J1 EBADF/EINVAL/EFAULT 三错误各 10/10（判定序对账），EFAULT 走 B2 copy 面联签（10/10），矩阵全绿 |
| UNX-F10757 | SO_DONTROUTE/SO_BROADCAST 域 socket 不适用注账 | 300 | 骨架 | UNX-F10757-J1 两选项 AF_UNIX 可设回读一致（10/10），设后行为零差异（千次对账），不适用注账在册 |
| UNX-F10758 | SO_LINGER 选项读写面（与 F10717 行为面分工） | 310 | 骨架 | UNX-F10758-J1 linger 结构 set→get 回读逐字段一致（10/10），l_onoff 非 0/1 外值 EINVAL（10/10），与 F10717 分工对账 10/10 |
| UNX-F10759 | 选项并发设置竞态（锁序单点） | 310 | 骨架 | UNX-F10759-J1 并发 set/get 千次无撕裂（回读全为合法值 100/100），set 与收发并发零错账（10/10），锁序对账 10/10 |
| UNX-F10760 | 选项族段总收口（B18 断言聚合） | 350 | 骨架 | UNX-F10760-J1 F10741–F10759 十九条判据聚合回归（350 项全绿），段累计对平（98,580+6,300=104,880），域总账移交清单更新 |

<!-- 主册行 13519 · #### UNX-C4-B19 · epoll 深水（F10761–F10780 · 20 条） -->
#### UNX-C4-B19 · epoll 深水（F10761–F10780 · 20 条）

> AI-14 承办｜域账累计：B01–B15 104,880 + 本批 6,400 = 111,280 / 240,000｜嫁接源：Linux epoll(7)/epoll_ctl(2)/epoll_wait(2)（注出处，禁凭记忆）｜防重：与 B18 防重：B18 立 SOL_SOCKET 选项族，本批立 epoll 复用器深水机制面；与 B26 防重：B26 立复用器 E 型错误矩阵（EINVAL/EBADF 全矩阵），本批只按条内需要最小覆盖错误；与 B16 防重：B16 立连接建立深水，本批 ready 上报只作联签｜批注：本批 B19：M 型机制收尾段第 4/5 批——epoll 深水（兴趣表/就绪链/ET-LT/ONESHOT/生命周期/嵌套）；B20 随后收口

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10761 | epoll_create1 建册与 O_CLOEXEC | 310 | 骨架 | UNX-F10761-J1 epoll fd 创建成功（10/10），size 参数校验注账（10/10），O_CLOEXEC 位生效（10/10） |
| UNX-F10762 | epoll_ctl ADD 主链（兴趣表登记） | 320 | 骨架 | UNX-F10762-J1 ADD 后兴趣表登记（10/10），事件位与 data 联合体回读一致（10/10），重复 ADD 同 fd 同表项 EEXIST（10/10） |
| UNX-F10763 | epoll_ctl MOD/DEL 幂等与语义 | 320 | 骨架 | UNX-F10763-J1 MOD 改掩码后上报随改（10/10），DEL 后零上报（10/10），MOD/DEL 各千次幂等对账 10/10 |
| UNX-F10764 | epoll_ctl EEXIST/ENOENT 错误面 | 320 | 骨架 | UNX-F10764-J1 重复 ADD EEXIST（10/10），未注册 MOD/DEL ENOENT（10/10），错误判定序对账（先 fd 后表项 10/10） |
| UNX-F10765 | LT 水平触发语义（就绪持续上报） | 330 | 骨架 | UNX-F10765-J1 LT 下未读尽持续就绪（10/10），读尽后自动静默（10/10），新数据再就绪（10/10），LT 循环对账 10/10 |
| UNX-F10766 | ET 边沿触发语义（一次性上报） | 330 | 骨架 | UNX-F10766-J1 ET 下新到数据报一次（10/10），未读尽不再报（10/10），新到沿再报（10/10），ET/LT 对照对账 10/10 |
| UNX-F10767 | EPOLLONESHOT 一次性禁用语义 | 310 | 骨架 | UNX-F10767-J1 ONESHOT 报一次后自动禁用（10/10），禁用后同 fd 其他事件零报（10/10），MOD 重新武装（10/10） |
| UNX-F10768 | EPOLLERR/EPOLLHUP 恒报面 | 320 | 骨架 | UNX-F10768-J1 未注册 ERR/HUP 掩码时对端关闭仍上报（10/10），ERR/HUP 与注册掩码无关恒报（10/10），SO_ERROR 联动 10/10 |
| UNX-F10769 | epoll_wait 三态（阻塞/超时/零轮询） | 330 | 骨架 | UNX-F10769-J1 timeout=-1 阻塞至事件（10/10），timeout=0 即返不阻塞（10/10），timeout=N 到点返 0（10/10），三态对账 10/10 |
| UNX-F10770 | epoll_wait EINTR 与 epoll_pwait | 310 | 骨架 | UNX-F10770-J1 信号中断 wait→EINTR（10/10），pwait 携 mask 屏蔽后不中断（10/10），重入语义注账 10/10 |
| UNX-F10771 | 就绪链与兴趣表联动（回检查竞态） | 330 | 骨架 | UNX-F10771-J1 注册后立即就绪可报（10/10），事件到点与 wait 并发千次零丢失（100/100），回检查竞态对账 10/10 |
| UNX-F10772 | close 自动注销（DEL 缺席等价） | 310 | 骨架 | UNX-F10772-J1 close 注册 fd 后零上报（10/10），兴趣表无残留（10/10），close 与 DEL 等价对账 10/10 |
| UNX-F10773 | dup 共享 fd 的注册与注销 | 320 | 骨架 | UNX-F10773-J1 dup 后任一副本均可注册（10/10），注册绑定底层描述（10/10），全副本 close 才注销（10/10） |
| UNX-F10774 | fork 后 epoll 实例与注册 fd 隔离 | 310 | 骨架 | UNX-F10774-J1 子进程继承 epoll fd 可 wait（10/10），子进程 DEL 影响共享实例（10/10），隔离边界注账 10/10 |
| UNX-F10775 | epoll fd 自身可 poll（嵌套 epoll） | 310 | 骨架 | UNX-F10775-J1 epoll fd 可注册进另一 epoll（10/10），下层事件经嵌套上报（10/10），三层嵌套对账 10/10 |
| UNX-F10776 | O_NONBLOCK 与 epoll 配合（非阻塞主链） | 320 | 骨架 | UNX-F10776-J1 ET+O_NONBLOCK 循环读至 EAGAIN（100/100），读尽不阻塞 wait（10/10），惯例主链对账 10/10 |
| UNX-F10777 | EPOLLRDHUP 半关闭上报面 | 310 | 骨架 | UNX-F10777-J1 对端 SHUT_WR 后 RDHUP 上报（10/10），残余数据仍可读（10/10），RDHUP 与 HUP 分工对账 10/10 |
| UNX-F10778 | 事件集合位面（EPOLLIN/OUT/PRI 全集对账） | 320 | 骨架 | UNX-F10778-J1 IN/OUT/PRI 三位独立注册上报各 10/10，位组合上报对账（10/10），未注册位零上报 10/10 |
| UNX-F10779 | epoll_wait maxevents 边界 | 320 | 骨架 | UNX-F10779-J1 maxevents=0→EINVAL（10/10），maxevents=N 只返 N 事件（10/10），事件数<就绪数截断注账 10/10 |
| UNX-F10780 | epoll 段总收口（B19 断言聚合） | 350 | 骨架 | UNX-F10780-J1 F10761–F10779 十九条判据聚合回归（350 项全绿），段累计对平（104,880+6,400=111,280），域总账移交清单更新 |

<!-- 主册行 13546 · #### UNX-C4-B20 · M 型机制总收口（F10781–F10800 · 20 条） -->
#### UNX-C4-B20 · M 型机制总收口（F10781–F10800 · 20 条）

> AI-14 承办｜域账累计：B01–B15 111,280 + 本批 6,400 = 117,680 / 240,000｜嫁接源：Linux epoll(7)/unix(7)/socket(7)（交互面出处索引）（注出处，禁凭记忆）｜防重：与 B16–B19 防重：本批零新增机制语义，全部为交互联测/聚合回归/台账核销条，判据面引用前批编号；与 B21–B28 防重：E 型错误矩阵批只引用本批移交清单不消费本批正文机制｜批注：本批 B20：M 型机制收尾段第 5/5 批总收口——交互矩阵/联测/压测/冻结件并账/防重复核/性能基线/移交核销；B21 起 E 型错误矩阵

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10781 | 机制交互总矩阵（四族两两对账） | 330 | 骨架 | UNX-F10781-J1 连接/地址/选项/复用器四族两两交互 6 组全绿（各 10/10），交互矩阵对账零空洞 10/10 |
| UNX-F10782 | 非阻塞 connect × epoll 联测主链 | 330 | 骨架 | UNX-F10782-J1 O_NONBLOCK connect→EINPROGRESS 后注册 OUT，就绪报出且 SO_ERROR=0（100/100），主链六拍对账 10/10 |
| UNX-F10783 | SO_RCVTIMEO × epoll_wait 双超时协同 | 320 | 骨架 | UNX-F10783-J1 两超时并存行为对账（wait 先到返事件/recv 先到 EAGAIN 各 10/10），双超时互不干扰 10/10 |
| UNX-F10784 | SO_PASSCRED × 就绪事件联动 | 320 | 骨架 | UNX-F10784-J1 就绪后 recvmsg 取报文附凭据（100/100），data 值与凭据同批一致（10/10），联动对账 10/10 |
| UNX-F10785 | 地址族 × 选项族联测（SO_REUSEADDR 占位 × 重绑） | 320 | 骨架 | UNX-F10785-J1 REUSEADDR 占位值对重绑判定零影响（10/10），EADDRINUSE 判定不读该选项（10/10），联测对账 10/10 |
| UNX-F10786 | close × epoll 注销 × SO_LINGER 终止三链联测 | 320 | 骨架 | UNX-F10786-J1 linger 开时 close 先滞留后注销（10/10），wait 收 HUP 时序对账（10/10），三链交叠零死锁 10/10 |
| UNX-F10787 | dup/fork × 选项继承 × epoll 生命周期联测 | 320 | 骨架 | UNX-F10787-J1 dup 继承选项且注册独立（10/10），fork 共享实例变更互见（10/10），生命周期矩阵对账 10/10 |
| UNX-F10788 | unix socket 全机制压测场（选项×收发×就绪千次） | 330 | 骨架 | UNX-F10788-J1 全机制混合压测千次零错账（100/100），资源账零泄漏（10/10），压测场冻结 10/10 |
| UNX-F10789 | M 型冻结件总清单（B16–B19 并账） | 320 | 骨架 | UNX-F10789-J1 B16–B19 冻结件全清单逐件核销（10/10），清单与域总账一致（10/10），版本指纹对账 10/10 |
| UNX-F10790 | M 型差异注账总清单（对照 Linux 差异复核） | 320 | 骨架 | UNX-F10790-J1 M 型差异注账逐项复核（10/10），差异项全数显式（10/10），无未登记差异（grep 对账 10/10） |
| UNX-F10791 | M 型防重总对账（批间判据零交集复核） | 310 | 骨架 | UNX-F10791-J1 B16–B20 五批判据面零交集复核（10/10），分工注账全数在位（10/10），防重声明对账 10/10 |
| UNX-F10792 | M 型性能基线账（千次延迟采样） | 320 | 骨架 | UNX-F10792-J1 connect/收发/就绪三面千次延迟采样入账（10/10），基线无回归（对照 B16 基线 10/10），账冻结 10/10 |
| UNX-F10793 | M 型回归套件固化（断言聚合清单） | 310 | 骨架 | UNX-F10793-J1 M 型回归清单固化可复跑（10/10），断言覆盖 B16–B19 全判据（10/10），复跑全绿 10/10 |
| UNX-F10794 | M 型异常路径总回归（错误码字面复核） | 320 | 骨架 | UNX-F10794-J1 M 型全部错误入口字面复核（10/10），errno 字面零偏离（10/10），错误矩阵对账 10/10 |
| UNX-F10795 | M 型并发总压测（多线程全机制混合） | 320 | 骨架 | UNX-F10795-J1 八线程全机制混合万次零错账（10/10），锁序对账零逆序（10/10），死锁零容忍探针全绿 10/10 |
| UNX-F10796 | B16–B19 判据全量重放（1,060 项聚合回归） | 320 | 骨架 | UNX-F10796-J1 前四批判据全量重放（1,060 项全绿），判据编号逐一在位（10/10），重放幂等（二次重放同绿 10/10） |
| UNX-F10797 | M 型文档账（条文出处总索引） | 300 | 骨架 | UNX-F10797-J1 M 型全部嫁接源出处索引在位（10/10），出处与条文页对账（10/10），索引冻结 10/10 |
| UNX-F10798 | M 型移交清单核销（指纹逐件核对） | 310 | 骨架 | UNX-F10798-J1 移交清单全件指纹核对（10/10），核销账与域总账一致（10/10），零幽灵件 10/10 |
| UNX-F10799 | M 型遗留账清点（未决项零残留核销） | 310 | 骨架 | UNX-F10799-J1 M 型注账未决项清点（10/10），遗留项全数显式核销或移交（10/10），零静默残留 10/10 |
| UNX-F10800 | M 型总收口断言（B20 聚合+段账对平） | 350 | 骨架 | UNX-F10800-J1 F10781–F10799 十九条判据聚合回归（350 项全绿），段累计对平（111,280+6,400=117,680），M 型段账封存 |

<!-- 主册行 13573 · #### UNX-C4-B21 · E 型·管道 FIFO 错误矩阵（F10801–F10820 · 20 条） -->
#### UNX-C4-B21 · E 型·管道 FIFO 错误矩阵（F10801–F10820 · 20 条）

> AI-14 承办｜域账累计：B01–B15 117,680 + 本批 6,900 = 124,580 / 240,000｜嫁接源：Linux pipe(2)/pipe(7)/fifo(7)/signal(7)（注出处，禁凭记忆）｜防重：与 M 型批防重：M 型立机制建立面，本批立 E 型错误矩阵（正反双判据：正面=错误按预期触发，反面=正确路径零误报）；与域内既有管道/FIFO 建立批（B01–B15 内）防重：既有批立建立与收发主链，本批只做错误面，判据零交集｜批注：本批 B21：E 型错误矩阵段 1/8——管道 FIFO 错误矩阵（EPIPE/SIGPIPE/EAGAIN/EFAULT/ENXIO/原子性/竞态窗），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10801 | EPIPE 正反双判据矩阵 | 370 | 骨架 | UNX-F10801-J1 正面：读端全关后 write→EPIPE（10/10）；反面：读端在场时 write 零误报（千次对账 10/10） |
| UNX-F10802 | SIGPIPE 三态矩阵（缺省/忽略/屏蔽） | 370 | 骨架 | UNX-F10802-J1 正面：缺省态 SIGPIPE 终止（10/10）；反面：忽略/屏蔽态返回 EPIPE 零信号（各 10/10） |
| UNX-F10803 | EAGAIN 正反双判据矩阵（O_NONBLOCK） | 350 | 骨架 | UNX-F10803-J1 正面：非阻塞满写/空读→EAGAIN（各 10/10）；反面：就绪时读写零误报（千次 10/10） |
| UNX-F10804 | EFAULT 正反双判据矩阵（坏指针双口） | 350 | 骨架 | UNX-F10804-J1 正面：坏指针读/写→EFAULT（各 10/10）；反面：好指针零误判（千次 10/10） |
| UNX-F10805 | 非管道 fd 误入管道路由错误面 | 340 | 骨架 | UNX-F10805-J1 正面：非管道 fd 上管道路由操作→相应错误（10/10）；反面：真管道 fd 零误判（千次 10/10） |
| UNX-F10806 | 零长度读写语义矩阵（read 0/write 0） | 340 | 骨架 | UNX-F10806-J1 正面：write 0 零副作用返 0、read 0 立即返 0（各 10/10）；反面：非零路径不受零长干扰（千次 10/10） |
| UNX-F10807 | FIFO open 错误总面（O_RDONLY/O_WRONLY 组合矩阵） | 340 | 骨架 | UNX-F10807-J1 正面：open 组合矩阵错误面全绿（10/10）；反面：合法组合零误判（千次 10/10） |
| UNX-F10808 | FIFO ENXIO 正反双判据（O_WRONLY 无读端） | 330 | 骨架 | UNX-F10808-J1 正面：O_WRONLY 无读端→ENXIO（10/10）；反面：有读端时零误报（千次 10/10） |
| UNX-F10809 | FIFO EACCES 权限矩阵 | 330 | 骨架 | UNX-F10809-J1 正面：无权限路径 open→EACCES（10/10）；反面：有权限零误报（千次 10/10） |
| UNX-F10810 | FIFO 路径解析错误面（ENOENT/ELOOP/ENAMETOOLONG） | 340 | 骨架 | UNX-F10810-J1 正面：三路径错误各 10/10；反面：正常路径零误报（千次 10/10） |
| UNX-F10811 | 管道路由非法命令错误面（fcntl/ioctl 类） | 340 | 骨架 | UNX-F10811-J1 正面：不支持命令→ENOTTY/EINVAL 按语义（10/10）；反面：支持命令零误报（千次 10/10） |
| UNX-F10812 | EMFILE/ENFILE 耗尽矩阵（进程/全局文件表） | 330 | 骨架 | UNX-F10812-J1 正面：进程表满 pipe→EMFILE、全局表满→ENFILE（各 10/10）；反面：余量内零误报（千次 10/10） |
| UNX-F10813 | EINTR 阻塞读写中断矩阵 | 350 | 骨架 | UNX-F10813-J1 正面：阻塞读写被信号中断→EINTR（各 10/10）；反面：无信号零中断（千次 10/10） |
| UNX-F10814 | 部分写语义矩阵（write 少于请求） | 340 | 骨架 | UNX-F10814-J1 正面：缓冲余量不足时部分写入返回已写字节（10/10）；反面：足量时全量写零截断（千次 10/10） |
| UNX-F10815 | PIPE_BUF 原子性矩阵（≤4096 vs >4096） | 340 | 骨架 | UNX-F10815-J1 正面：≤4096 多写者不交错（10/10）；>4096 允许交错注账（10/10）；反面：单写者零差异（千次 10/10） |
| UNX-F10816 | 读端关闭竞态窗矩阵（write 与 close 竞态） | 350 | 骨架 | UNX-F10816-J1 正面：竞态窗两结果（EPIPE 或成功）均合法且无第三结果（万次 10/10）；反面：窗外零歧义（千次 10/10） |
| UNX-F10817 | ENOMEM 内核资源耗尽面 | 340 | 骨架 | UNX-F10817-J1 正面：缓冲分配失败→ENOMEM 路径显式（10/10）；反面：资源充足零误报（千次 10/10） |
| UNX-F10818 | FIFO open 阻塞矩阵（互等行为正反） | 350 | 骨架 | UNX-F10818-J1 正面：阻塞读开等写端、阻塞写开等读端（各 10/10）；反面：对端到场即解锁（千次 10/10） |
| UNX-F10819 | O_TRUNC/O_APPEND 误用于 FIFO 显式注账面 | 350 | 骨架 | UNX-F10819-J1 正面：两标志对 FIFO 无效果且不报错（各 10/10）；反面：对普通文件正常生效（对照 10/10） |
| UNX-F10820 | 管道 FIFO 错误矩阵段总收口（B21 断言聚合） | 350 | 骨架 | UNX-F10820-J1 F10801–F10819 十九条判据聚合回归（350 项全绿），段累计对平（117,680+6,900=124,580），域总账移交清单更新 |

<!-- 主册行 13600 · #### UNX-C4-B22 · E 型·信号错误矩阵（F10821–F10840 · 20 条） -->
#### UNX-C4-B22 · E 型·信号错误矩阵（F10821–F10840 · 20 条）

> AI-14 承办｜域账累计：B01–B15 124,580 + 本批 6,800 = 131,380 / 240,000｜嫁接源：Linux signal(7)/sigaction(2)/kill(2)/sigprocmask(2)/sigsuspend(2)/alarm(2)（注出处，禁凭记忆）｜防重：与 B21 防重：B21 立管道 FIFO 错误矩阵（SIGPIPE 三态为管道侧），本批立信号本体错误矩阵（SIGPIPE 仅联动引用）；与 B21-F10813 防重：F10813 立管道读写 EINTR 面，本批 F10834 立信号处置总矩阵（SA_RESTART 全语义），判据零交集｜批注：本批 B22：E 型错误矩阵段 2/8——信号错误矩阵（硬边界/注册/kill/挂起合并/RESTART/fork-exec 边界/备栈），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10821 | SIGKILL/SIGSTOP 不可捕获矩阵 | 350 | 骨架 | UNX-F10821-J1 正面：两信号捕获/屏蔽注册均被拒（各 10/10）；反面：普通信号注册零误拒（千次 10/10） |
| UNX-F10822 | signal() 注册错误矩阵（EINVAL 非法信号号） | 340 | 骨架 | UNX-F10822-J1 正面：非法信号号→EINVAL（10/10）；反面：合法号域内全可注册（千次 10/10） |
| UNX-F10823 | sigaction 错误矩阵（EINVAL/EFAULT） | 340 | 骨架 | UNX-F10823-J1 正面：非法 act 指针→EFAULT、非法标志→EINVAL（各 10/10）；反面：合法注册零误报（千次 10/10） |
| UNX-F10824 | kill() 错误矩阵（EINVAL/EPERM/ESRCH） | 350 | 骨架 | UNX-F10824-J1 正面：三错误各 10/10（判定序对账）；反面：有权限目标零误报（千次 10/10） |
| UNX-F10825 | raise() 语义与错误面 | 330 | 骨架 | UNX-F10825-J1 正面：raise 报号等价 kill 自身（10/10）、非法号 EINVAL（10/10）；反面：合法号零误报（千次 10/10） |
| UNX-F10826 | SIGSEGV/SIGFPE/SIGILL 硬件类信号源矩阵 | 350 | 骨架 | UNX-F10826-J1 正面：三类信号源各触发（10/10）；反面：正常路径零误发（千次 10/10） |
| UNX-F10827 | sigprocmask 错误矩阵（EINVAL/NULL 面语义） | 340 | 骨架 | UNX-F10827-J1 正面：非法 how→EINVAL、非法号集内→EINVAL（各 10/10）；反面：合法掩码操作零误报（千次 10/10） |
| UNX-F10828 | sigpending 挂起集矩阵 | 340 | 骨架 | UNX-F10828-J1 正面：屏蔽期收号入挂起集、解屏后递送（各 10/10）；反面：无挂起零误报（千次 10/10） |
| UNX-F10829 | sigsuspend 原子等待矩阵 | 340 | 骨架 | UNX-F10829-J1 正面：sigsuspend 原子换屏等待返回后还原（各 10/10）；反面：非目标信号不唤醒（千次 10/10） |
| UNX-F10830 | 标准信号不排队合并矩阵 | 350 | 骨架 | UNX-F10830-J1 正面：同号挂起合并为一、解屏只递一次（各 10/10）；反面：异号不互并（千次 10/10） |
| UNX-F10831 | 信号处理函数异步安全面（safe 清单） | 340 | 骨架 | UNX-F10831-J1 正面：safe 清单内函数 handler 内可用（10/10）；反面：清单外误用可检出并登记（10/10） |
| UNX-F10832 | SIGCHLD 语义矩阵（终止通知与收尸） | 340 | 骨架 | UNX-F10832-J1 正面：子进程终止触发 SIGCHLD（10/10）、忽略态自动收尸（10/10）；反面：无子终止零误发（千次 10/10） |
| UNX-F10833 | SIGUSR1/SIGUSR2 用户信号面 | 330 | 骨架 | UNX-F10833-J1 正面：两用户信号注册/递送/挂起全链（各 10/10）；反面：未注册缺省终止显式（10/10） |
| UNX-F10834 | 信号处置总矩阵（SA_RESTART 全语义） | 340 | 骨架 | UNX-F10834-J1 正面：无 RESTART 中断返 EINTR、有 RESTART 自动重启（各 10/10）；反面：两态互斥零串扰（千次 10/10） |
| UNX-F10835 | alarm/SIGALRM 定时信号矩阵 | 340 | 骨架 | UNX-F10835-J1 正面：alarm 到点递送 SIGALRM、重置取旧值（各 10/10）；反面：alarm(0) 取消零递送（千次 10/10） |
| UNX-F10836 | fork 后信号处置与掩码继承矩阵 | 330 | 骨架 | UNX-F10836-J1 正面：子进程继承处置表与掩码（各 10/10）；反面：挂起集清空显式（10/10） |
| UNX-F10837 | exec 后信号处置复位矩阵 | 330 | 骨架 | UNX-F10837-J1 正面：捕获处置复位 DFL、忽略处置保留（各 10/10）；反面：掩码与挂起保留显式（10/10） |
| UNX-F10838 | 信号多线程路由矩阵（进程级递送） | 340 | 骨架 | UNX-F10838-J1 正面：kill 进程后任一线程可收（10/10）、掩码定路由（10/10）；反面：路由零丢失（千次 10/10） |
| UNX-F10839 | sigaltstack 备用栈面（SA_ONSTACK） | 330 | 骨架 | UNX-F10839-J1 正面：注册备栈后 ONSTACK handler 于备栈执行（10/10）；反面：未注册 ONSTACK 显式报错（10/10） |
| UNX-F10840 | 信号错误矩阵段总收口（B22 断言聚合） | 350 | 骨架 | UNX-F10840-J1 F10821–F10839 十九条判据聚合回归（350 项全绿），段累计对平（124,580+6,800=131,380），域总账移交清单更新 |

<!-- 主册行 13627 · #### UNX-C4-B23 · E 型·pty/termios 错误矩阵（F10841–F10860 · 20 条） -->
#### UNX-C4-B23 · E 型·pty/termios 错误矩阵（F10841–F10860 · 20 条）

> AI-14 承办｜域账累计：B01–B15 131,380 + 本批 6,900 = 138,280 / 240,000｜嫁接源：Linux pty(7)/termios(3)/tty_ioctl(4)/ioctl_tty(2)（注出处，禁凭记忆）｜防重：与 B22 防重：B22 立信号矩阵，本批立 pty/termios 错误矩阵；与域内既有 pty/termios 建立批（B01–B15 内）防重：既有批立建立与收发主链，本批只做错误矩阵面，判据零交集｜批注：本批 B23：E 型错误矩阵段 3/8——pty/termios 错误矩阵（EIO/ENOTTY/VMIN-VTIME 四格/窗口/会话/生命周期），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10841 | pty 打开错误矩阵（资源与参数面） | 350 | 骨架 | UNX-F10841-J1 正面：pty 耗尽 open→EAGAIN/参数非法→EINVAL（各 10/10）；反面：余量内零误报（千次 10/10） |
| UNX-F10842 | 对端关闭矩阵（master 关闭 slave 面） | 350 | 骨架 | UNX-F10842-J1 正面：master 关后 slave 读 EIO/写 EIO（各 10/10）；反面：master 在场零误报（千次 10/10） |
| UNX-F10843 | tcgetattr/tcsetattr 错误矩阵（ENOTTY/EINVAL） | 350 | 骨架 | UNX-F10843-J1 正面：非 tty fd→ENOTTY、非法可选动作→EINVAL（各 10/10）；反面：tty fd 零误报（千次 10/10） |
| UNX-F10844 | EIO 与 EOF 分账矩阵（读返 0 vs EIO） | 350 | 骨架 | UNX-F10844-J1 正面：残余读尽返 0、断链读 EIO（各 10/10）；反面：两态互斥零混报（千次 10/10） |
| UNX-F10845 | 非法 termios 参数矩阵（cflag/cc 组合） | 350 | 骨架 | UNX-F10845-J1 正面：非法 cc 下标/非法 cflag 组合→EINVAL（各 10/10）；反面：合法组合零误拒（千次 10/10） |
| UNX-F10846 | TIOCGWINSZ/TIOCSWINSZ 错误矩阵 | 340 | 骨架 | UNX-F10846-J1 正面：非 tty fd→ENOTTY（10/10）、SIGWINCH 联动（10/10）；反面：合法读写零误报（千次 10/10） |
| UNX-F10847 | TIOCGPGRP/TIOCSPGRP 错误矩阵（前景组） | 340 | 骨架 | UNX-F10847-J1 正面：非 tty→ENOTTY、非子组→EPERM/EINVAL（各 10/10）；反面：合法组零误报（千次 10/10） |
| UNX-F10848 | TIOCSCTTY/TIOCNOTTY 控制终端矩阵（EPERM） | 350 | 骨架 | UNX-F10848-J1 正面：非会话首抢占→EPERM、释放后 ioctl→ENOTTY（各 10/10）；反面：合法归属零误报（千次 10/10） |
| UNX-F10849 | tcflush/tcdrain/tcflow 错误矩阵 | 340 | 骨架 | UNX-F10849-J1 正面：queue_selector 非法→EINVAL、非 tty→ENOTTY（各 10/10）；反面：合法选择零误报（千次 10/10） |
| UNX-F10850 | 速率面矩阵（cfsetispeed/cfgetospeed） | 330 | 骨架 | UNX-F10850-J1 正面：非法速率值→EINVAL（10/10）、B0 语义（10/10）；反面：合法速率零误报（千次 10/10） |
| UNX-F10851 | ECHO/ICANON 模式矩阵（规范/非规范切换错误面） | 350 | 骨架 | UNX-F10851-J1 正面：模式切换即时生效（10/10）、非法组合 EINVAL（10/10）；反面：未切换面零扰动（千次 10/10） |
| UNX-F10852 | VMIN/VTIME 非规范读矩阵（四组合） | 350 | 骨架 | UNX-F10852-J1 正面：四组合语义各 10/10；反面：组合外零歧义（千次 10/10） |
| UNX-F10853 | pty O_NONBLOCK 读写矩阵（EAGAIN 正反） | 340 | 骨架 | UNX-F10853-J1 正面：满写/空读 EAGAIN（各 10/10）；反面：就绪零误报（千次 10/10） |
| UNX-F10854 | pty 缓冲满写矩阵（阻塞正反） | 340 | 骨架 | UNX-F10854-J1 正面：阻塞写至对端读解锁（10/10）；反面：解锁顺序 FIFO 零饥饿（千次 10/10） |
| UNX-F10855 | 无控制终端 ioctl 矩阵（ENOTTY） | 340 | 骨架 | UNX-F10855-J1 正面：无控制终端进程 ioctl→ENOTTY（10/10）；反面：有控制终端零误报（千次 10/10） |
| UNX-F10856 | setsid 释放与 vhangup 面矩阵 | 340 | 骨架 | UNX-F10856-J1 正面：setsid 释放控制终端后 ioctl→ENOTTY（10/10）、vhangup 断链（10/10）；反面：合法序列零误报（千次 10/10） |
| UNX-F10857 | pty pair 生命周期矩阵（slave 关 master 面） | 350 | 骨架 | UNX-F10857-J1 正面：slave 关后 master 读 EIO（10/10）、写 EIO（10/10）；反面：slave 在场零误报（千次 10/10） |
| UNX-F10858 | termios 并发修改竞态矩阵 | 340 | 骨架 | UNX-F10858-J1 正面：并发 tcsetattr 千次无撕裂（10/10）；反面：读写并发零错账（千次 10/10） |
| UNX-F10859 | pty 错误码字面总对账（六码矩阵） | 350 | 骨架 | UNX-F10859-J1 正面：EIO/ENXIO/ENOTTY/EAGAIN/EINVAL/EPERM 六码字面逐一对照（各 10/10）；反面：字面零偏离（千次 10/10） |
| UNX-F10860 | pty/termios 错误矩阵段总收口（B23 断言聚合） | 350 | 骨架 | UNX-F10860-J1 F10841–F10859 十九条判据聚合回归（350 项全绿），段累计对平（131,380+6,900=138,280），域总账移交清单更新 |

<!-- 主册行 13654 · #### UNX-C4-B24 · E 型·unix socket 错误矩阵一（F10861–F10880 · 20 条 -->
#### UNX-C4-B24 · E 型·unix socket 错误矩阵一（F10861–F10880 · 20 条）

> AI-14 承办｜域账累计：B01–B15 138,280 + 本批 6,900 = 145,180 / 240,000｜嫁接源：Linux unix(7)/recvmsg(2)/cmsg(3)（注出处，禁凭记忆）｜防重：与 B16–B18 防重：M 型立机制建立面（连接/地址/选项），本批立 unix socket 错误矩阵面，判据零交集；与 B21 防重：B21 立管道 EPIPE，本批 F10863 为 stream socket 侧 EPIPE（同码异域分账）｜批注：本批 B24：E 型错误矩阵段 4/8——unix socket 错误矩阵一（REFUSED/RESET/EPIPE/ENOTCONN/EISCONN/SCMR/零长/判定总序），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10861 | ECONNREFUSED 矩阵（connect 无监听） | 350 | 骨架 | UNX-F10861-J1 正面：connect 无监听 socket→ECONNREFUSED（10/10）；反面：有监听零误报（千次 10/10） |
| UNX-F10862 | ECONNRESET 矩阵（对端异常关闭） | 350 | 骨架 | UNX-F10862-J1 正面：对端 close 后本端读 ECONNRESET（10/10）；反面：正常关闭读 EOF 零混报（千次 10/10） |
| UNX-F10863 | EPIPE 矩阵（stream 对端关后写） | 350 | 骨架 | UNX-F10863-J1 正面：对端关后 write→EPIPE（10/10）；反面：对端在场零误报（千次 10/10） |
| UNX-F10864 | ENOTCONN 矩阵（未连接即收发） | 340 | 骨架 | UNX-F10864-J1 正面：未连接 stream 收发→ENOTCONN（各 10/10）；反面：已连接零误报（千次 10/10） |
| UNX-F10865 | EISCONN 矩阵（已连接 dgram 再 connect） | 340 | 骨架 | UNX-F10865-J1 正面：已 connect dgram 再 connect 他址→EISCONN（10/10）；反面：connect 同址幂等零误报（千次 10/10） |
| UNX-F10866 | EOPNOTSUPP 矩阵（listen/accept 误用） | 340 | 骨架 | UNX-F10866-J1 正面：dgram 上 listen→EOPNOTSUPP、非监听上 accept→EINVAL（各 10/10）；反面：正确用法零误报（千次 10/10） |
| UNX-F10867 | EPROTOTYPE 矩阵（类型不匹配 connect） | 340 | 骨架 | UNX-F10867-J1 正面：stream socket connect 到 seqpacket 监听端→EPROTOTYPE（10/10）；反面：同型连接零误报（千次 10/10） |
| UNX-F10868 | EALREADY 矩阵（进行中再 connect） | 340 | 骨架 | UNX-F10868-J1 正面：非阻塞 connect 进行中再 connect→EALREADY（10/10）；反面：完成后再 connect 幂等（千次 10/10） |
| UNX-F10869 | EMSGSIZE 矩阵（dgram 超限） | 340 | 骨架 | UNX-F10869-J1 正面：dgram 报文超 SO_SNDBUF 上限→EMSGSIZE（10/10）；反面：限内零误报（千次 10/10） |
| UNX-F10870 | EAGAIN dgram 满矩阵 | 340 | 骨架 | UNX-F10870-J1 正面：非阻塞 dgram 满→EAGAIN（10/10）；反面：有空间零误报（千次 10/10） |
| UNX-F10871 | EINTR 收发中断矩阵 | 340 | 骨架 | UNX-F10871-J1 正面：阻塞收发被信号中断→EINTR（各 10/10）；反面：无信号零中断（千次 10/10） |
| UNX-F10872 | msg 结构错误矩阵（msg_iov/msg_control 面EINVAL/EFAULT） | 340 | 骨架 | UNX-F10872-J1 正面：坏 msg 指针→EFAULT、非法 iovcnt→EINVAL/EMSG（各 10/10）；反面：合法 msg 零误报（千次 10/10） |
| UNX-F10873 | SCM_RIGHTS fd 传递错误矩阵（收方表满/非法 fd） | 350 | 骨架 | UNX-F10873-J1 正面：收方 fd 表满时延迟报错路径显式（10/10）、传递已关闭 fd→EBADF（10/10）；反面：正常传递零误报（千次 10/10） |
| UNX-F10874 | SCM_CREDENTIALS 伪造拒收矩阵 | 350 | 骨架 | UNX-F10874-J1 正面：用户态注入凭据 cmsg→拒收 EINVAL（10/10）；反面：内核填充凭据正常通行（千次 10/10） |
| UNX-F10875 | dgram 零长报文矩阵 | 350 | 骨架 | UNX-F10875-J1 正面：send 0 长报文合法入队、recv 读出零长（各 10/10）；反面：与 EOF 零混报（千次 10/10） |
| UNX-F10876 | 抽象命名空间错误矩阵（非法字符/占用） | 340 | 骨架 | UNX-F10876-J1 正面：抽象名含非法字符→EINVAL、占用→EADDRINUSE（各 10/10）；反面：合法名零误报（千次 10/10） |
| UNX-F10877 | accept 错误矩阵（EINVAL/EMFILE） | 350 | 骨架 | UNX-F10877-J1 正面：非监听 accept→EINVAL、fd 表满→EMFILE（各 10/10）；反面：正常 accept 零误报（千次 10/10） |
| UNX-F10878 | shutdown 错误矩阵（how 值/ENOTCONN） | 350 | 骨架 | UNX-F10878-J1 正面：非法 how→EINVAL、未连接 shutdown→ENOTCONN（各 10/10）；反面：合法三向零误报（千次 10/10） |
| UNX-F10879 | unix socket 错误判定总序表（冻结件） | 350 | 骨架 | UNX-F10879-J1 错误判定序表全枚举（10/10）、序间衔接零冲突（10/10）；反面：序表冻结后零漂移（千次 10/10） |
| UNX-F10880 | unix socket 错误矩阵一段总收口（B24 断言聚合） | 350 | 骨架 | UNX-F10880-J1 F10861–F10879 十九条判据聚合回归（350 项全绿），段累计对平（138,280+6,900=145,180），域总账移交清单更新 |

<!-- 主册行 13681 · #### UNX-C4-B25 · E 型·unix socket 错误矩阵二（F10881–F10900 · 20 条 -->
#### UNX-C4-B25 · E 型·unix socket 错误矩阵二（F10881–F10900 · 20 条）

> AI-14 承办｜域账累计：B01–B15 145,180 + 本批 7,000 = 152,180 / 240,000｜嫁接源：Linux unix(7)/recvmsg(2)/cmsg(3)/unlink(2)（注出处，禁凭记忆）｜防重：与 B24 防重：B24 立错误矩阵一（基础错误码面），本批立矩阵二（竞态窗/深水/时序/挂接面），判据零交集；与 B16–B18 防重：M 型立机制面，本批只做错误面；判定序一律挂接 F10879 冻结序表｜批注：本批 B25：E 型错误矩阵段 5/8——unix socket 错误矩阵二（竞态窗/backlog 溢出/cmsg 截断/seqpacket/peek/序表挂接），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10881 | connect 竞态窗矩阵（对端 close 并发） | 350 | 骨架 | UNX-F10881-J1 正面：竞态两结果（成功/ECONNREFUSED）合法且无第三态（万次 10/10）；反面：窗外零歧义（千次 10/10） |
| UNX-F10882 | backlog 满矩阵（连接队列溢出） | 350 | 骨架 | UNX-F10882-J1 正面：队列满后 connect 行为显式（10/10）；反面：有余量零误拒（千次 10/10） |
| UNX-F10883 | EADDRINUSE 竞态矩阵（bind 与 unlink 并发） | 350 | 骨架 | UNX-F10883-J1 正面：bind/unlink 竞态两结果合法无第三态（万次 10/10）；反面：窗外零歧义（千次 10/10） |
| UNX-F10884 | getsockname/getpeername 未绑定矩阵 | 350 | 骨架 | UNX-F10884-J1 正面：未绑 getsockname 零值域显式（10/10）、未连接 getpeername→ENOTCONN（10/10）；反面：合法状态零误报（千次 10/10） |
| UNX-F10885 | sendto 显式地址矩阵（dgram 定向发送） | 350 | 骨架 | UNX-F10885-J1 正面：sendto 显式地址直达对端（10/10）、地址不存在→ECONNREFUSED（10/10）；反面：合法目标千次零误报（10/10） |
| UNX-F10886 | recvfrom 地址快照矩阵（匿名对端零长） | 350 | 骨架 | UNX-F10886-J1 正面：recvfrom 返回发送方地址快照（10/10）、匿名对端零长地址（10/10）；反面：快照与实况一致（千次 10/10） |
| UNX-F10887 | cmsg 截断矩阵（MSG_CTRUNC） | 350 | 骨架 | UNX-F10887-J1 正面：控制缓冲不足→MSG_CTRUNC 置位（10/10）；反面：缓冲充足零置位（千次 10/10） |
| UNX-F10888 | SCM_RIGHTS 深水错误矩阵（环/重复 fd） | 350 | 骨架 | UNX-F10888-J1 正面：fd 自传递环显式处置（10/10）、重复 fd 传递计数正确（10/10）；反面：正常深水传递零泄漏（千次 10/10） |
| UNX-F10889 | seqpacket 超长截断矩阵（MSG_TRUNC） | 350 | 骨架 | UNX-F10889-J1 正面：读缓冲小于报文→截断+MSG_TRUNC 置位（10/10）；反面：缓冲充足零截断（千次 10/10） |
| UNX-F10890 | SO_RCVLOWAT 联动错误矩阵（wait 与 recv 低水位） | 350 | 骨架 | UNX-F10890-J1 正面：低于 LOWAT 时 wait 不就绪（10/10）、recv 仍可读（10/10）；反面：达阈零误判（千次 10/10） |
| UNX-F10891 | accept 与 setsockopt 竞态矩阵 | 350 | 骨架 | UNX-F10891-J1 正面：并发 accept/setsockopt 千次无撕裂（10/10）；反面：继承值完整（千次 10/10） |
| UNX-F10892 | dup 注册 epoll 后 EPIPE 联动矩阵 | 350 | 骨架 | UNX-F10892-J1 正面：经副本触发 EPIPE 全副本可感知（10/10）；反面：副本间零串扰（千次 10/10） |
| UNX-F10893 | 地址文件 unlink 竞态错误矩阵（连接中 unlink） | 350 | 骨架 | UNX-F10893-J1 正面：连接中 unlink 后新 connect→ECONNREFUSED、存量连接存活（各 10/10）；反面：存量零扰动（千次 10/10） |
| UNX-F10894 | pending 错误队列出队序矩阵 | 350 | 骨架 | UNX-F10894-J1 正面：多错误源 FIFO 出队（10/10）、单源多次合并（10/10）；反面：零丢失零重排（千次 10/10） |
| UNX-F10895 | ERR 事件与 SO_ERROR 时序矩阵 | 350 | 骨架 | UNX-F10895-J1 正面：ERR 就绪先于 SO_ERROR 消费（10/10）、消费后事件退位（10/10）；反面：时序零颠倒（千次 10/10） |
| UNX-F10896 | 抽象名进程退出自动清理矩阵 | 350 | 骨架 | UNX-F10896-J1 正面：进程退出后抽象名释放可重绑（10/10）；反面：显式绑定面零误清（千次 10/10） |
| UNX-F10897 | sendmsg iov 散布错误矩阵（部分 iov 非法） | 350 | 骨架 | UNX-F10897-J1 正面：第 N 段 iov 坏指针→EFAULT 零部分发送（10/10）；反面：全合法零误报（千次 10/10） |
| UNX-F10898 | MSG_PEEK 错误矩阵（窥视不消费） | 350 | 骨架 | UNX-F10898-J1 正面：peek 后水位零变化（千次 10/10）、空队 peek dgram→EAGAIN（10/10）；反面：消费路径零受扰（千次 10/10） |
| UNX-F10899 | 错误二段判定序挂接核销（F10879 序表） | 350 | 骨架 | UNX-F10899-J1 二段错误条挂接序表逐条核销（10/10）、序表衔接零冲突（10/10）；反面：挂接后零漂移（千次 10/10） |
| UNX-F10900 | unix socket 错误矩阵二段总收口（B25 断言聚合） | 350 | 骨架 | UNX-F10900-J1 F10881–F10899 十九条判据聚合回归（350 项全绿），段累计对平（145,180+7,000=152,180），域总账移交清单更新 |

<!-- 主册行 13708 · #### UNX-C4-B26 · E 型·复用器错误矩阵（F10901–F10920 · 20 条） -->
#### UNX-C4-B26 · E 型·复用器错误矩阵（F10901–F10920 · 20 条）

> AI-14 承办｜域账累计：B01–B15 152,180 + 本批 6,800 = 158,980 / 240,000｜嫁接源：Linux epoll(7)/poll(2)/select(2)（注出处，禁凭记忆）｜防重：与 B19 防重：B19 立 epoll 机制深水（错误按条内最小覆盖），本批立复用器 E 型全错误矩阵；与 F10779 防重：F10779 立 maxevents 边界面，本批矩阵面并账引用不重复；判定序挂接 F10879 冻结序表｜批注：本批 B26：E 型错误矩阵段 6/8——复用器错误矩阵（epoll/poll/select 三系错误/POLLNVAL/契约/三系对照），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10901 | epoll_wait 错误矩阵（EBADF/EFAULT/ENOMEM） | 340 | 骨架 | UNX-F10901-J1 正面：三错误各 10/10；反面：合法调用零误报（千次 10/10） |
| UNX-F10902 | epoll_ctl EPERM 矩阵（非 pollable fd） | 340 | 骨架 | UNX-F10902-J1 正面：普通文件 fd 注册→EPERM（10/10）；反面：可 poll 类型零误拒（千次 10/10） |
| UNX-F10903 | epoll_ctl EBADF/EINVAL 全矩阵 | 340 | 骨架 | UNX-F10903-J1 正面：EBADF/EINVAL 各 10/10（与 F10764 EEXIST/ENOENT 合并四码全绿）；反面：正确用法千次零误报（10/10） |
| UNX-F10904 | poll EFAULT/EINVAL 矩阵 | 340 | 骨架 | UNX-F10904-J1 正面：坏 fds 指针→EFAULT、nfds 负值→EINVAL（各 10/10）；反面：合法数组零误报（千次 10/10） |
| UNX-F10905 | POLLNVAL 矩阵（坏 fd 报告面） | 340 | 骨架 | UNX-F10905-J1 正面：坏 fd 条目 revents=POLLNVAL（10/10）；反面：好 fd 零 NVAL（千次 10/10） |
| UNX-F10906 | select EBADF/FD_SETSIZE 矩阵 | 340 | 骨架 | UNX-F10906-J1 正面：坏 fd→EBADF、fd≥FD_SETSIZE→EINVAL/越界（各 10/10）；反面：域内零误报（千次 10/10） |
| UNX-F10907 | select fd_set 宏越界错误面 | 320 | 骨架 | UNX-F10907-J1 正面：FD_SET 越界 fd 域内守卫检出（10/10）；反面：限内宏操作零误报（千次 10/10） |
| UNX-F10908 | 三复用器 timeout 负值对照矩阵 | 340 | 骨架 | UNX-F10908-J1 正面：三调用负 timeout 语义各显式（10/10）；反面：三调用语义零互串（千次 10/10） |
| UNX-F10909 | epoll_wait EINTR 复核矩阵 | 340 | 骨架 | UNX-F10909-J1 正面：信号中断→EINTR（10/10）、pwait 屏蔽（10/10）；反面：无信号零中断（千次 10/10） |
| UNX-F10910 | poll revents 契约矩阵 | 340 | 骨架 | UNX-F10910-J1 正面：events 用户域/revents 内核域隔离（10/10）；反面：内核零写 events 域（千次 10/10） |
| UNX-F10911 | select 返回值语义矩阵 | 330 | 骨架 | UNX-F10911-J1 正面：超时返 0/就绪返计数（各 10/10）；反面：计数与实际就绪一致（千次 10/10） |
| UNX-F10912 | poll 返回值语义矩阵 | 330 | 骨架 | UNX-F10912-J1 正面：超时返 0/就绪返条目数（各 10/10）；反面：计数与 revents 置位数一致（千次 10/10） |
| UNX-F10913 | 混合 fd 类型注册矩阵（全类型 pollable） | 350 | 骨架 | UNX-F10913-J1 正面：五类型同 epoll 实例混注册全通（10/10）；反面：类型间零串扰（千次 10/10） |
| UNX-F10914 | 注册上限/ENOMEM 矩阵 | 340 | 骨架 | UNX-F10914-J1 正面：兴趣表满→ENOMEM 显式（10/10）；反面：余量内零误报（千次 10/10） |
| UNX-F10915 | 事件消费语义矩阵（wait 后事件归属） | 340 | 骨架 | UNX-F10915-J1 正面：wait 返回事件即消费单点（10/10）、LT 未消费条件仍在重报（10/10）；反面：双消费零发生（千次 10/10） |
| UNX-F10916 | 唤醒丢失防护矩阵（事件与 wait 竞态） | 350 | 骨架 | UNX-F10916-J1 正面：三复用器并发压测万次零丢失（10/10）；反面：窗外零歧义（千次 10/10） |
| UNX-F10917 | 复用器判定序挂接核销（F10879 序表） | 340 | 骨架 | UNX-F10917-J1 复用器错误条挂接序表逐条核销（10/10）、衔接零冲突（10/10）；反面：挂接后零漂移（千次 10/10） |
| UNX-F10918 | 复用器错误码字面总对账 | 340 | 骨架 | UNX-F10918-J1 正面：EBADF/EINVAL/EFAULT/EINTR/EPERM/ENOMEM/POLLNVAL 七面字面对照（各 10/10）；反面：字面零偏离（千次 10/10） |
| UNX-F10919 | 三复用器交叉对照总矩阵 | 350 | 骨架 | UNX-F10919-J1 select/poll/epoll 同场景九组对照全绿（各 10/10）；反面：三系结果一致（千次 10/10） |
| UNX-F10920 | 复用器错误矩阵段总收口（B26 断言聚合） | 350 | 骨架 | UNX-F10920-J1 F10901–F10919 十九条判据聚合回归（350 项全绿），段累计对平（152,180+6,800=158,980），域总账移交清单更新 |

<!-- 主册行 13735 · #### UNX-C4-B27 · E 型·SysV IPC 错误矩阵（F10921–F10940 · 20 条） -->
#### UNX-C4-B27 · E 型·SysV IPC 错误矩阵（F10921–F10940 · 20 条）

> AI-14 承办｜域账累计：B01–B15 158,980 + 本批 6,900 = 165,880 / 240,000｜嫁接源：Linux svipc(7)/msgget(2)/msgsnd(2)/msgrcv(2)/semget(2)/semop(2)/shmget(2)/shmat(2)/ftok(3)（注出处，禁凭记忆）｜防重：与 B21–B26 防重：前批立管道/信号/pty/sock/复用器矩阵，本批立 SysV 三对象（消息/信号量/共享内存）错误矩阵，判据零交集；判定序挂接 F10879 冻结序表｜批注：本批 B27：E 型错误矩阵段 7/8——SysV IPC 错误矩阵（消息队列/信号量/共享内存三对象全错误面），正反双判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10921 | msgget 错误矩阵（key 四错误面） | 350 | 骨架 | UNX-F10921-J1 正面：ENOENT/EEXIST/EACCES/EINVAL 四错误各 10/10；反面：合法 key 零误报（千次 10/10） |
| UNX-F10922 | msgsnd EAGAIN/EINTR 矩阵（队列满） | 350 | 骨架 | UNX-F10922-J1 正面：满队 NOWAIT→EAGAIN（10/10）、阻塞中断 EINTR（10/10）；反面：有余量零误报（千次 10/10） |
| UNX-F10923 | msgsnd EINVAL/EACCES 矩阵（尺寸与权限） | 350 | 骨架 | UNX-F10923-J1 正面：msgsz 超 EACCES 类 EINVAL、无写权限→EACCES（各 10/10）；反面：合法发送千次零误报（10/10） |
| UNX-F10924 | msgrcv ENOMSG/E2BIG 矩阵 | 350 | 骨架 | UNX-F10924-J1 正面：无匹配 NOWAIT→ENOMSG（10/10）、超长 E2BIG（10/10）；反面：匹配消息零误报（千次 10/10） |
| UNX-F10925 | msgrcv EIDRM/EINVAL 矩阵（集合已删） | 350 | 骨架 | UNX-F10925-J1 正面：集合已删后阻塞收→EIDRM（10/10）、负 msgtyp 非法→EINVAL（10/10）；反面：存活集合零误报（千次 10/10） |
| UNX-F10926 | msgctl 错误矩阵（RMID/STAT/SET 面） | 350 | 骨架 | UNX-F10926-J1 正面：EINVAL/EPERM/EACCES 三错误各 10/10；反面：合法命令零误报（千次 10/10） |
| UNX-F10927 | msgget 表满 ENOSPC 矩阵 | 340 | 骨架 | UNX-F10927-J1 正面：系统级队列数满→ENOSPC（10/10）；反面：余量内零误报（千次 10/10） |
| UNX-F10928 | semget 错误矩阵（四错误面） | 340 | 骨架 | UNX-F10928-J1 正面：EACCES/EEXIST/ENOSPC/EINVAL 四错误各 10/10；反面：合法 key 零误报（千次 10/10） |
| UNX-F10929 | semop EAGAIN/E2BIG 矩阵 | 350 | 骨架 | UNX-F10929-J1 正面：NOWAIT 负值→EAGAIN（10/10）、sembuf 越界 EINVAL（10/10）；反面：可执行操作零误报（千次 10/10） |
| UNX-F10930 | semop EIDRM/EINVAL/EFBIG 矩阵 | 340 | 骨架 | UNX-F10930-J1 正面：已删 EIDRM（10/10）、非法 semnum EINVAL（10/10）；反面：存活集合零误报（千次 10/10） |
| UNX-F10931 | SEM_UNDO 终止回滚矩阵 | 340 | 骨架 | UNX-F10931-J1 正面：带 UNDO 操作后进程终止回滚（10/10）；反面：无 UNDO 零回滚（千次 10/10） |
| UNX-F10932 | semctl 错误矩阵（EINVAL/EPERM/ERANGE） | 340 | 骨架 | UNX-F10932-J1 正面：EINVAL/EPERM/ERANGE 三错误各 10/10；反面：合法命令零误报（千次 10/10） |
| UNX-F10933 | shmget 错误矩阵（四错误面） | 340 | 骨架 | UNX-F10933-J1 正面：EACCES/EEXIST/ENOSPC/EINVAL 四错误各 10/10；反面：合法 key 零误报（千次 10/10） |
| UNX-F10934 | shmat 错误矩阵（EMFILE/EACCES/ENOMEM/对齐） | 350 | 骨架 | UNX-F10934-J1 正面：四错误各 10/10（EMFILE/EACCES/ENOMEM/EINVAL 对齐）；反面：合法挂载零误报（千次 10/10） |
| UNX-F10935 | shmdt EINVAL 矩阵（非映射地址） | 340 | 骨架 | UNX-F10935-J1 正面：非挂载地址 shmdt→EINVAL（10/10）；反面：合法卸载千次零误报（10/10） |
| UNX-F10936 | shmctl 错误矩阵（EINVAL/EPERM/EIDRM） | 340 | 骨架 | UNX-F10936-J1 正面：EINVAL/EPERM/EIDRM 三错误各 10/10；反面：合法命令零误报（千次 10/10） |
| UNX-F10937 | SHM_RMID 与映射存活矩阵 | 350 | 骨架 | UNX-F10937-J1 正面：RMID 后已挂载映射仍可读写（10/10）、新 shmat→EINVAL（10/10）；反面：卸载后段回收（千次 10/10） |
| UNX-F10938 | ftok/key 冲突错误矩阵 | 340 | 骨架 | UNX-F10938-J1 正面：ftok 文件丢失→ENOENT（10/10）、key 冲突类型不符→EEXIST 类显式（10/10）；反面：合法 ftok 零误报（千次 10/10） |
| UNX-F10939 | SysV 判定序挂接核销（F10879 序表） | 340 | 骨架 | UNX-F10939-J1 SysV 错误条挂接序表逐条核销（10/10）、衔接零冲突（10/10）；反面：挂接后零漂移（千次 10/10） |
| UNX-F10940 | SysV 错误矩阵段总收口（B27 断言聚合） | 350 | 骨架 | UNX-F10940-J1 F10921–F10939 十九条判据聚合回归（350 项全绿），段累计对平（158,980+6,900=165,880），域总账移交清单更新 |

<!-- 主册行 13762 · #### UNX-C4-B28 · E 型·POSIX IPC 错误矩阵（F10941–F10960 · 20 条） -->
#### UNX-C4-B28 · E 型·POSIX IPC 错误矩阵（F10941–F10960 · 20 条）

> AI-14 承办｜域账累计：B01–B15 165,880 + 本批 6,800 = 172,680 / 240,000｜嫁接源：Linux mq_overview(7)/mq_send(3)/mq_receive(3)/sem_overview(7)/sem_open(3)/sem_wait(3)（注出处，禁凭记忆）｜防重：与 B27 防重：B27 立 SysV 三对象矩阵，本批立 POSIX 两域（mq/sem）矩阵，判据零交集；与 B24–B26 防重：sock/复用器面不重复；判定序挂接 F10879 冻结序表｜批注：本批 B28：E 型错误矩阵段 8/8——POSIX IPC 错误矩阵（mq/sem 两域全错误面+fd 集成+两域隔离），正反双判据制；E 型段收口

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10941 | mq_open 错误矩阵（五错误面） | 350 | 骨架 | UNX-F10941-J1 正面：ENOENT/EEXIST/EACCES/EINVAL/EMFILE 五错误各 10/10；反面：合法打开千次零误报（10/10） |
| UNX-F10942 | mq_send EAGAIN/EMSGSIZE 矩阵 | 350 | 骨架 | UNX-F10942-J1 正面：非阻塞满→EAGAIN（10/10）、msg_len 超→EMSGSIZE（10/10）；反面：限内零误报（千次 10/10） |
| UNX-F10943 | mq_send EINVAL/EBADF 矩阵 | 320 | 骨架 | UNX-F10943-J1 正面：坏描述符 EBADF（10/10）、非法优先级 EINVAL（10/10）；反面：合法范围零误报（千次 10/10） |
| UNX-F10944 | mq_receive EAGAIN/EMSGSIZE 矩阵 | 350 | 骨架 | UNX-F10944-J1 正面：非阻塞空→EAGAIN（10/10）、缓冲小于 msgsize→EMSGSIZE（10/10）；反面：合法接收千次零误报（10/10） |
| UNX-F10945 | mq_receive EBADF/EINVAL 矩阵 | 320 | 骨架 | UNX-F10945-J1 正面：坏描述符 EBADF（10/10）、msg_prio 空指针 EINVAL（10/10）；反面：合法接收千次零误报（10/10） |
| UNX-F10946 | mq_notify EBUSY/EINVAL 矩阵 | 340 | 骨架 | UNX-F10946-J1 正面：重复注册→EBUSY（10/10）、非法通知 EINVAL（10/10）；反面：合法注册千次零误报（10/10） |
| UNX-F10947 | mq_getattr/mq_setattr 错误矩阵 | 330 | 骨架 | UNX-F10947-J1 正面：坏描述符 EBADF（10/10）、attr 非法 EINVAL（10/10）；反面：合法调用千次零误报（10/10） |
| UNX-F10948 | mq_close/mq_unlink 错误矩阵 | 340 | 骨架 | UNX-F10948-J1 正面：EBADF/ENOENT/EACCES 三错误各 10/10；反面：合法关闭/删除千次零误报（10/10） |
| UNX-F10949 | mq 名字解析错误矩阵（路径/权限） | 330 | 骨架 | UNX-F10949-J1 正面：非法名 EINVAL（10/10）、名 EACCES（10/10）；反面：合法名千次零误报（10/10） |
| UNX-F10950 | sem_open 错误矩阵（四错误面） | 350 | 骨架 | UNX-F10950-J1 正面：ENOENT/EEXIST/EACCES/EINVAL 四错误各 10/10；反面：合法打开千次零误报（10/10） |
| UNX-F10951 | sem_wait EINTR/EAGAIN 矩阵 | 350 | 骨架 | UNX-F10951-J1 正面：零值 NOWAIT 类 EAGAIN（10/10）、阻塞中断 EINTR（10/10）；反面：可减值零误报（千次 10/10） |
| UNX-F10952 | sem_post EOVERFLOW 矩阵 | 340 | 骨架 | UNX-F10952-J1 正面：值达上限 post→EOVERFLOW（10/10）；反面：限内 post 千次零误报（10/10） |
| UNX-F10953 | sem_trywait EAGAIN 矩阵 | 340 | 骨架 | UNX-F10953-J1 正面：零值 trywait→EAGAIN（10/10）；反面：正值 trywait 千次零误报（10/10） |
| UNX-F10954 | sem_getvalue 错误矩阵 | 340 | 骨架 | UNX-F10954-J1 正面：EINVAL/EBADF 两错误各 10/10；反面：回读与值账一致（千次 10/10） |
| UNX-F10955 | sem_close/sem_unlink 错误矩阵 | 330 | 骨架 | UNX-F10955-J1 正面：EINVAL/ENOENT/EACCES 三错误各 10/10；反面：合法关闭/删除千次零误报（10/10） |
| UNX-F10956 | mq 描述符 fd 集成矩阵（dup/epoll 面） | 350 | 骨架 | UNX-F10956-J1 正面：mqd 可 dup/可注册 epoll（10/10）；反面：副本事件零串扰（千次 10/10） |
| UNX-F10957 | POSIX 与 SysV 并存对账矩阵（同名异域隔离） | 340 | 骨架 | UNX-F10957-J1 正面：同 key/名两域对象互不干扰（10/10）；反面：跨界引用零发生（千次 10/10） |
| UNX-F10958 | POSIX IPC 名字生命周期矩阵（unlink 后 open） | 340 | 骨架 | UNX-F10958-J1 正面：unlink 后新 open→ENOENT（10/10）、活跃描述符存活（10/10）；反面：延迟销毁语义零摇摆（千次 10/10） |
| UNX-F10959 | POSIX 判定序挂接核销（F10879 序表） | 340 | 骨架 | UNX-F10959-J1 POSIX 错误条挂接序表逐条核销（10/10）、衔接零冲突（10/10）；反面：挂接后零漂移（千次 10/10） |
| UNX-F10960 | POSIX 错误矩阵段总收口（B28 断言聚合） | 350 | 骨架 | UNX-F10960-J1 F10941–F10959 十九条判据聚合回归（350 项全绿），段累计对平（165,880+6,800=172,680），域总账移交清单更新 |

<!-- 主册行 13789 · #### UNX-C4-B29 · I 型·vim 全屏联测（F10961–F10980 · 20 条） -->
#### UNX-C4-B29 · I 型·vim 全屏联测（F10961–F10980 · 20 条）

> AI-14 承办｜域账累计：B01–B15 172,680 + 本批 7,200 = 179,880 / 240,000｜嫁接源：Linux pty(7)/termios(3)/vim(1) 终端惯例/tty_ioctl(4)（注出处，禁凭记忆）｜防重：与 B21–B28 防重：E 型矩阵批立单点错误面，本批 I 型立跨机制集成联测面（pty+termios+信号+epoll 全链），判据零交集；判定序挂接 F10879 冻结序表｜批注：本批 B29：I 型集成前段 1/2——vim 全屏联测（pty+termios+信号+epoll 全链集成，键入/重绘/resize/挂起/韧性），集成判据制

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10961 | vim 联测场搭建（pty+会话+控制终端全链） | 360 | 骨架 | UNX-F10961-J1 联测场全链搭建：openpty+setsid+TIOCSCTTY 全通（10/10），链路账对平（10/10） |
| UNX-F10962 | 原始模式切换联测（ICANON/ECHO 关闭全链） | 360 | 骨架 | UNX-F10962-J1 原始模式全位关闭即时生效（10/10），键入逐字符可见（10/10） |
| UNX-F10963 | 键入路径联测（键→pty→非规范读→应用） | 360 | 骨架 | UNX-F10963-J1 键入到应用逐拍零丢失（100/100 逐键对账），路径时序对账（10/10） |
| UNX-F10964 | epoll 驱动输入主循环联测（wait→read→处理→write） | 360 | 骨架 | UNX-F10964-J1 主循环千轮零丢失零空转（10/10），循环四拍对账（10/10） |
| UNX-F10965 | 全屏刷新输出联测（write 吞吐与缓冲协同） | 360 | 骨架 | UNX-F10965-J1 全屏刷新 100 轮零丢失（100/100 逐字节对账），缓冲协同对账（10/10） |
| UNX-F10966 | 窗口尺寸变更联测（TIOCSWINSZ→SIGWINCH→重绘） | 360 | 骨架 | UNX-F10966-J1 尺寸变更三拍全通（10/10），重绘用新尺寸（10/10） |
| UNX-F10967 | SIGTSTP/SIGCONT 挂起恢复联测（模式保存恢复） | 360 | 骨架 | UNX-F10967-J1 挂起前模式快照/恢复后逐位还原（各 10/10），千次往返零漂移（10/10） |
| UNX-F10968 | EINTR 中断重入联测（主循环信号安全） | 360 | 骨架 | UNX-F10968-J1 主循环千次中断重入零丢失（10/10），重入账对平（10/10） |
| UNX-F10969 | 转义序列解析联测（ESC [ A 序列完整性） | 360 | 骨架 | UNX-F10969-J1 方向键序列逐段到达完整（10/10）、跨读分段重组正确（10/10） |
| UNX-F10970 | 回显控制联测（密码输入关回显场景） | 360 | 骨架 | UNX-F10970-J1 关回显后键入零回显（100/100），开回显恢复逐字回显（10/10） |
| UNX-F10971 | 终端状态快照与恢复联测（对称性） | 360 | 骨架 | UNX-F10971-J1 get→set→get 往返逐位一致（10/10），千次往返零漂移（10/10） |
| UNX-F10972 | 退出清理联测（异常退出模式恢复保证） | 360 | 骨架 | UNX-F10972-J1 异常退出后终端模式恢复（10/10）、资源零泄漏（10/10） |
| UNX-F10973 | 多键连发背压联测（快速键入不丢键） | 360 | 骨架 | UNX-F10973-J1 高频连发万键零丢失（100/100 分批对账），背压协同对账（10/10） |
| UNX-F10974 | 滚动重绘一致性联测（输出与逻辑屏对账） | 360 | 骨架 | UNX-F10974-J1 滚动百轮输出序一致（10/10），重绘账对平（10/10） |
| UNX-F10975 | SIGWINCH 风暴联测（连续 resize 不崩不丢终态） | 360 | 骨架 | UNX-F10975-J1 千次连续 resize 终态正确（10/10），零崩溃零挂起（10/10） |
| UNX-F10976 | vim 场错误注入联测（EIO/EAGAIN/EINTR 混合） | 360 | 骨架 | UNX-F10976-J1 三错误混合注入千次场景存活（10/10），恢复账对平（10/10） |
| UNX-F10977 | vim 场资源账联测（fd/内存/进程零泄漏） | 360 | 骨架 | UNX-F10977-J1 场景全程资源账零泄漏（10/10），终态对平（10/10） |
| UNX-F10978 | vim 场并发会话联测（双实例隔离） | 360 | 骨架 | UNX-F10978-J1 双实例各自终端输入输出零串扰（10/10），会话隔离对账（10/10） |
| UNX-F10979 | vim 联测冻结件与回归清单 | 360 | 骨架 | UNX-F10979-J1 联测清单固化可复跑（10/10）、覆盖 F10961–F10978 全判据（10/10） |
| UNX-F10980 | I 型一段总收口（B29 断言聚合） | 360 | 骨架 | UNX-F10980-J1 F10961–F10979 十九条判据聚合回归（360 项全绿），段累计对平（172,680+7,200=179,880），域总账移交清单更新 |

<!-- 主册行 13816 · #### UNX-C4-B30 · I 型二段·多进程 IPC 压测场（F10981–F11000 · 20 条） -->
#### UNX-C4-B30 · I 型二段·多进程 IPC 压测场（F10981–F11000 · 20 条）

> AI-14 承办｜域账累计：B01–B15 179,880 + 本批 7,300 = 187,180 / 240,000｜嫁接源：Linux pipe(7)/sem_overview(7)/svipc(5)/shmget(2)/kill(2) 多进程惯例（注出处，禁凭记忆）｜防重：与 B21–B28 防重：E 型立单点错误面；与 B29 防重：I 型一段立单应用 vim 联测面，本批 I 型二段立多进程 IPC 压测面，判据零交集；判定序挂接 F10879 冻结序表｜批注：本批 B30：I 型集成后段 2/2——多进程 IPC 压测场（进程组+四机制混合+批量收割+韧性+收官），集成判据制，域 300 条总收口

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F10981 | 多进程 IPC 压测场搭建（进程组+资源池全链） | 360 | 骨架 | UNX-F10981-J1 压测场全链搭建（10/10），资源池四账对平（10/10） |
| UNX-F10982 | 多写者管道压力（PIPE_BUF 原子交错对账） | 360 | 骨架 | UNX-F10982-J1 ≤PIPE_BUF 块千次零交错（10/10），大块交错边界冻结（10/10） |
| UNX-F10983 | 多读者管道压力（竞争读不重不丢） | 360 | 骨架 | UNX-F10983-J1 多读者竞争读字节账唯一归属（千次 10/10），总量守恒（10/10） |
| UNX-F10984 | 读端 kill 写者 EPIPE/SIGPIPE 全链压力 | 360 | 骨架 | UNX-F10984-J1 kill 读端写者三态千次对账（10/10），组内隔离对平（10/10） |
| UNX-F10985 | 多进程信号量互斥压力（临界区零双入） | 360 | 骨架 | UNX-F10985-J1 N 进程临界区万次零双入（10/10），sem 账逐次对平（10/10） |
| UNX-F10986 | 信号量保护共享内存并发读写压力（账一致性） | 360 | 骨架 | UNX-F10986-J1 写独占读一致千次快照对平（10/10），shm 字节账守恒（10/10） |
| UNX-F10987 | SysV 消息队列类型路由压力（多发多收零错投） | 360 | 骨架 | UNX-F10987-J1 mtype 路由千次零错投（10/10），队列账守恒（10/10） |
| UNX-F10988 | 进程组批量 kill 压力（killpg+IPC 资源残留账） | 360 | 骨架 | UNX-F10988-J1 killpg 全组终止千次无漏（10/10），IPC 残留账逐件核销（10/10） |
| UNX-F10989 | 并发子进程退出收割压力（SIGCHLD+wait 账对平） | 360 | 骨架 | UNX-F10989-J1 M 子并发退出收割逐 pid 对平（千次 10/10），收割零遗漏（10/10） |
| UNX-F10990 | 孤儿与僵尸场景压力（收养链+收割账） | 360 | 骨架 | UNX-F10990-J1 父先退子收养千次链路对平（10/10），僵尸收割终态归零（10/10） |
| UNX-F10991 | waitpid WNOHANG 批量轮询收割压力（ECHILD 终态） | 360 | 骨架 | UNX-F10991-J1 WNOHANG 轮询收割千次零阻塞（10/10），ECHILD 终态零误报（10/10） |
| UNX-F10992 | SCM_RIGHTS 描述符传递多进程压力（fd 账联动） | 360 | 骨架 | UNX-F10992-J1 fd 传递千次接收可用（10/10），引用账两端对平（10/10） |
| UNX-F10993 | 四机制混合压力（pipe/shm/sem/msg 交叉组合） | 360 | 骨架 | UNX-F10993-J1 四机制交叉组合全账守恒（10/10），零死锁零挂起（10/10） |
| UNX-F10994 | 多进程场景错误注入韧性压力（四错混注） | 360 | 骨架 | UNX-F10994-J1 四错混注千次场景存活（10/10），恢复账对平（10/10） |
| UNX-F10995 | 四账型资源总扫（fd/内存/进程表/IPC） | 360 | 骨架 | UNX-F10995-J1 场毕四账归零（10/10），分段留痕可定位（10/10） |
| UNX-F10996 | 多进程压测性能基线冻结（吞吐/延迟在册） | 360 | 骨架 | UNX-F10996-J1 吞吐/延迟基线入册（10/10），复跑偏差容差内（10/10） |
| UNX-F10997 | 全域回归套件固化（B16–B30 判据清单） | 360 | 骨架 | UNX-F10997-J1 全域清单固化可复跑（10/10），覆盖对账零缺口（10/10） |
| UNX-F10998 | 300 条防重总对账（F10701–F11000） | 360 | 骨架 | UNX-F10998-J1 四范围 grep 零重复（10/10），判据编号全量唯一（10/10） |
| UNX-F10999 | 移交清单总封存（B16–B30 冻结件入册） | 360 | 骨架 | UNX-F10999-J1 全域冻结件逐件指纹入册（10/10），只增不删核销（10/10） |
| UNX-F11000 | C4 域总收口（B16–B30 断言聚合） | 460 | 骨架 | UNX-F11000-J1 F10981–F10999 十九条判据聚合回归（460 项全绿），段累计对平（179,880+7,300=187,180），域总账移交清单更新，B31–B40 收官接力声明在册 |

<!-- 主册行 13843 · #### UNX-C4-B31 · 挂号联签一 · futex/epoll 族挂点对账（F11001–F11020 ·  -->
#### UNX-C4-B31 · 挂号联签一 · futex/epoll 族挂点对账（F11001–F11020 · 20 条）

> AI-14 承办｜域账累计：B01–B15 187,180 + 本批 5,330 = 192,510 / 240,000｜嫁接源：C2 挂号注册协议冻结件（futex/epoll 族经分发表注册，联签条目 F10415/F10419 在册）+ futex(2)/epoll(7)/epoll_ctl(2) 惯例条文（注出处，禁凭记忆）｜防重：与 B12 防重：那批立 select/poll 适配与 futex 四操作单点原语面，本批立挂号联签对账面，判据零交集；与 B19/B26 防重：那两批立 epoll 深水主体与复用器错误矩阵，本批立挂号注册联签面；判定序挂接 F10879 冻结序表｜批注：I 型后段·挂号联签一：C2 挂号注册协议联测（futex/epoll 族挂点对账），收官条 F11020 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11001 | 挂号联签场搭建（futex/epoll 族挂点总账） | 260 | 骨架 | UNX-F11001-J1 联签场全链搭建（10/10），挂点总账逐项对平（10/10） |
| UNX-F11002 | futex 分发表注册联签（C2 协议注册项对账） | 260 | 骨架 | UNX-F11002-J1 注册项全量对账（10/10），注册序与协议一致（10/10） |
| UNX-F11003 | futex WAIT 挂点联测（入眠路径与挂号账联动） | 260 | 骨架 | UNX-F11003-J1 入眠千次账联动零偏离（10/10），挂点留痕逐次对平（10/10） |
| UNX-F11004 | futex WAKE 挂点联测（唤醒路径与唤醒计数账） | 260 | 骨架 | UNX-F11004-J1 唤醒计数逐次对平（10/10），多目标唤醒序冻结（10/10） |
| UNX-F11005 | futex 竞态挂点联签（WAIT 前唤醒窗口价值变更序） | 260 | 骨架 | UNX-F11005-J1 前唤醒窗口千次零丢失（10/10），价值变更序对账（10/10） |
| UNX-F11006 | futex 错误矩阵挂点对账（E 型 B22 面联签） | 260 | 骨架 | UNX-F11006-J1 错误码挂点逐项对账（10/10），E 型判据挂接零冲突（10/10） |
| UNX-F11007 | epoll 实例建立挂号联签（create 注册面对账） | 260 | 骨架 | UNX-F11007-J1 实例注册面逐项对平（10/10），fd 账联动一致（10/10） |
| UNX-F11008 | epoll ADD 挂号账联签（兴趣列表注册项对账） | 260 | 骨架 | UNX-F11008-J1 ADD 注册千次账对平（10/10），兴趣列表一致性（10/10） |
| UNX-F11009 | epoll MOD/DEL 挂号账联签（改注注销幂等对账） | 260 | 骨架 | UNX-F11009-J1 MOD/DEL 幂等千次对平（10/10），注销清扫零残留（10/10） |
| UNX-F11010 | epoll LT/ET 挂点行为联测（两模式事件账） | 260 | 骨架 | UNX-F11010-J1 LT 重复通知账逐次对平（10/10），ET 一次通知账（10/10） |
| UNX-F11011 | epoll ONESHOT/EXCLUSIVE 挂点联签（单发与惊群抑制挂账） | 260 | 骨架 | UNX-F11011-J1 ONESHOT 失能账逐次对平（10/10），EXCLUSIVE 抑制账（10/10） |
| UNX-F11012 | epoll 错误码挂点对账（E 型 B26 面联签） | 260 | 骨架 | UNX-F11012-J1 四错误码挂点逐项对平（10/10），错误路径挂号核销（10/10） |
| UNX-F11013 | fork/继承挂点联签（epoll fd 与 futex 跨进程共享） | 260 | 骨架 | UNX-F11013-J1 继承账逐项对平（10/10），共享挂点行为一致（10/10） |
| UNX-F11014 | close/资源回收挂点对账（注销与挂点清扫账） | 260 | 骨架 | UNX-F11014-J1 close 注销千次零残留（10/10），回收账逐件核销（10/10） |
| UNX-F11015 | futex/epoll 交叉压测挂点（同进程混用互扰账） | 260 | 骨架 | UNX-F11015-J1 混用万次互扰零命中（10/10），两族账独立对平（10/10） |
| UNX-F11016 | 多进程 epoll 惊群挂点联测（EXCLUSIVE 组行为账） | 260 | 骨架 | UNX-F11016-J1 惊群抑制千次零双醒（10/10），组账逐次对平（10/10） |
| UNX-F11017 | 超时挂点联签（futex timeout 与 epoll timeout 语义对账） | 260 | 骨架 | UNX-F11017-J1 超时路径千次对平（10/10），超时账核销零残留（10/10） |
| UNX-F11018 | 挂号压力联测（futex 万次 WAKE/WAIT 与 epoll 千次事件） | 260 | 骨架 | UNX-F11018-J1 futex 万次账守恒（10/10），epoll 千次账守恒（10/10） |
| UNX-F11019 | 挂号对账总扫（挂点全账核销与零残留断言） | 260 | 骨架 | UNX-F11019-J1 挂点全账核销对平（10/10），零残留断言成立（10/10） |
| UNX-F11020 | B31 批收口（挂号联签一总断言） | 390 | 骨架 | UNX-F11020-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 13870 · #### UNX-C4-B32 · 挂号联签二 · SysV/POSIX/pty/socket 挂号对账（F11021– -->
#### UNX-C4-B32 · 挂号联签二 · SysV/POSIX/pty/socket 挂号对账（F11021–F11040 · 20 条）

> AI-14 承办｜域账累计：B01–B15 192,510 + 本批 5,330 = 197,840 / 240,000｜嫁接源：C2 挂号注册协议冻结件（SysV/POSIX/pty/socket 族挂点在册）+ svipc(7)/sem_overview(7)/mq_overview(7)/pty(7)/unix(7) 惯例条文（注出处，禁凭记忆）｜防重：与 B13/B14 防重：那两批立 SysV shm/sem/msg 主体面，本批立挂号对账面；与 B28 防重：那批立 POSIX IPC 错误矩阵，本批挂号联签；与 B05–B07 防重：那三批立 pty/termios 主体，本批挂点账面；与 B31 防重：那批 futex/epoll 族，本批其余四族；判定序挂接 F10879 冻结序表｜批注：I 型后段·挂号联签二：C2 挂号注册协议联测（SysV/POSIX/pty/socket 族挂点对账），收官条 F11040 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11021 | SysV shm 挂号联签（shmat/shmdt 挂点注册对账） | 260 | 骨架 | UNX-F11021-J1 挂接注册账逐项对平（10/10），摘挂核销零残留（10/10） |
| UNX-F11022 | SysV sem 挂号联签（semop 挂点与挂号账联动） | 260 | 骨架 | UNX-F11022-J1 semop 挂点万次账对平（10/10），阻塞入眠账联动（10/10） |
| UNX-F11023 | SysV msg 挂号联签（msgsnd/msgrcv 挂点账对账） | 260 | 骨架 | UNX-F11023-J1 收发挂点千次账对平（10/10），队列驻留账守恒（10/10） |
| UNX-F11024 | SysV RMID 挂点联签（延迟回收挂号对账） | 260 | 骨架 | UNX-F11024-J1 RMID 挂号账逐件核销（10/10），延迟回收语义一致（10/10） |
| UNX-F11025 | POSIX sem 挂号联签（sem_wait/post 挂点账对账） | 260 | 骨架 | UNX-F11025-J1 wait/post 挂点万次对平（10/10），计数账零偏离（10/10） |
| UNX-F11026 | POSIX mq 挂号联签（mq_send/receive 挂点账） | 260 | 骨架 | UNX-F11026-J1 收发挂点千次对平（10/10），通知注册账联动（10/10） |
| UNX-F11027 | eventfd/timerfd 挂号联签（读挂点与就绪账对账） | 260 | 骨架 | UNX-F11027-J1 就绪挂点千次对平（10/10），读清账联动一致（10/10） |
| UNX-F11028 | pty 主从挂点联签（ptmx/pts 挂号对账） | 260 | 骨架 | UNX-F11028-J1 主从挂点千次对平（10/10），开闭注册账一致（10/10） |
| UNX-F11029 | termios ioctl 挂点联签（挂号与行规程账对账） | 260 | 骨架 | UNX-F11029-J1 ioctl 挂点千次对平（10/10），标志账零漂移（10/10） |
| UNX-F11030 | pty 会话挂点联签（控制终端与前台组挂号） | 260 | 骨架 | UNX-F11030-J1 会话挂号账逐项对平（10/10），前台组切换账一致（10/10） |
| UNX-F11031 | unix socket 挂号联签（bind/listen/accept 挂点账） | 260 | 骨架 | UNX-F11031-J1 三段挂点千次对平（10/10），连接注册账守恒（10/10） |
| UNX-F11032 | socket 数据面挂点联签（send/recv 挂号对账） | 260 | 骨架 | UNX-F11032-J1 收发挂点千次账对平（10/10），字节守恒零丢失（10/10） |
| UNX-F11033 | SCM_RIGHTS 挂点联签（fd 传递挂号账对账） | 260 | 骨架 | UNX-F11033-J1 传递挂点千次账对平（10/10），fd 引用账一致（10/10） |
| UNX-F11034 | 五族挂点交叉压测（SysV/POSIX/pty/socket 混用互扰账） | 260 | 骨架 | UNX-F11034-J1 混用万次互扰零命中（10/10），五族账独立对平（10/10） |
| UNX-F11035 | 五族错误路径挂点对账（错误矩阵联签） | 260 | 骨架 | UNX-F11035-J1 错误挂点逐族对平（10/10），错误路径核销零残留（10/10） |
| UNX-F11036 | 五族回收挂点总扫（close/unlink/RMID 核销对账） | 260 | 骨架 | UNX-F11036-J1 回收挂点逐族核销（10/10），三路回收零残留（10/10） |
| UNX-F11037 | 多进程五族挂号联测（fork 继承账对账） | 260 | 骨架 | UNX-F11037-J1 继承账逐族对平（10/10），跨进程行为一致（10/10） |
| UNX-F11038 | 五族挂号压力账（万次收发对平） | 260 | 骨架 | UNX-F11038-J1 五族万次账守恒（10/10），压力下零丢项（10/10） |
| UNX-F11039 | 挂号对账总扫（五族零残留断言） | 260 | 骨架 | UNX-F11039-J1 五族全账核销对平（10/10），零残留断言成立（10/10） |
| UNX-F11040 | B32 批收口（挂号联签二总断言） | 390 | 骨架 | UNX-F11040-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 13897 · #### UNX-C4-B33 · LTP ipc/pty 组对照联签 · R-C3-003 释放闸对账面（F11041 -->
#### UNX-C4-B33 · LTP ipc/pty 组对照联签 · R-C3-003 释放闸对账面（F11041–F11060 · 20 条）

> AI-14 承办｜域账累计：B01–B15 197,840 + 本批 5,330 = 203,170 / 240,000｜嫁接源：LTP ipc/pty 组公开用例清单（用例号照清单誊录，禁凭记忆，防幻觉条款：用例号一律可复查）+ R-C3-003 释放闸冻结面（C3 域向 C4 释放的联签对账面）（注出处，禁凭记忆）｜防重：与 B31/B32 防重：那两批立 C2 挂号协议联签面，本批立 LTP 外部对照面；与 B34 防重：那批立域内五族回归矩阵，本批立外部用例对照；判定序挂接 F10879 冻结序表｜批注：I 型后段·LTP 对照联签：ipc/pty 组外部用例对照+R-C3-003 释放闸承接核销，收官条 F11060 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11041 | LTP 对照场搭建（ipc/pty 组用例清单立账） | 260 | 骨架 | UNX-F11041-J1 对照场全链搭建（10/10），用例清单账逐项对平（10/10） |
| UNX-F11042 | LTP pipe/fifo 组用例对照（管道族对账） | 260 | 骨架 | UNX-F11042-J1 pipe 组用例对照逐例对平（10/10），fifo 组对照（10/10） |
| UNX-F11043 | LTP sem/shm/msg 组用例对照（SysV 族对账） | 260 | 骨架 | UNX-F11043-J1 SysV 三族用例对照对平（10/10），生命周期语义对齐（10/10） |
| UNX-F11044 | LTP pty/termios 组用例对照（终端族对账） | 260 | 骨架 | UNX-F11044-J1 pty 组用例对照对平（10/10），termios 组对照（10/10） |
| UNX-F11045 | LTP 信号/时钟组用例对照（交叉族对账） | 260 | 骨架 | UNX-F11045-J1 信号组用例对照对平（10/10），与 IPC 交叉语义对齐（10/10） |
| UNX-F11046 | LTP futex/epoll 组用例对照（复用与同步族对账） | 260 | 骨架 | UNX-F11046-J1 futex 组用例对照对平（10/10），epoll 组对照（10/10） |
| UNX-F11047 | LTP unix socket 组用例对照（套接字族对账） | 260 | 骨架 | UNX-F11047-J1 socket 组用例对照对平（10/10），SCM_RIGHTS 对照（10/10） |
| UNX-F11048 | LTP eventfd/timerfd 组用例对照（fd 族对账） | 260 | 骨架 | UNX-F11048-J1 eventfd 用例对照对平（10/10），timerfd 对照（10/10） |
| UNX-F11049 | 用例映射表对账（LTP 用例号与域判据号双账） | 260 | 骨架 | UNX-F11049-J1 映射表双账逐行对平（10/10），双向可查零断链（10/10） |
| UNX-F11050 | 失配归因账（不可对齐用例豁免登记） | 260 | 骨架 | UNX-F11050-J1 失配逐例归因留痕（10/10），豁免率在闸内（10/10） |
| UNX-F11051 | R-C3-003 释放闸对账面（C3 释放项 C4 承接核销） | 260 | 骨架 | UNX-F11051-J1 释放项逐件承接对平（10/10），承接核销账零悬空（10/10） |
| UNX-F11052 | 对照结果总账（绿/失配/豁免三态汇总） | 260 | 骨架 | UNX-F11052-J1 三态账逐组对平（10/10），总账守恒闭合（10/10） |
| UNX-F11053 | 对照回归重放（全量用例重放账） | 260 | 骨架 | UNX-F11053-J1 全量重放逐例对平（10/10），重放与首跑一致（10/10） |
| UNX-F11054 | 对照总扫与移交注账（外部对照面封存） | 260 | 骨架 | UNX-F11054-J1 对照面封存逐项核销（10/10），移交注账零悬空（10/10） |
| UNX-F11055 | LTP 对照压力组合（高并发 ipc/pty 用例组对照） | 260 | 骨架 | UNX-F11055-J1 高并发组对照对平（10/10），并发语义对齐（10/10） |
| UNX-F11056 | LTP 负向用例对照（错误注入组对账） | 260 | 骨架 | UNX-F11056-J1 负向组用例对照对平（10/10），拒止语义对齐（10/10） |
| UNX-F11057 | 对照覆盖率账（用例组覆盖全景对账） | 260 | 骨架 | UNX-F11057-J1 覆盖率逐组对平（10/10），覆盖盲区零容忍（10/10） |
| UNX-F11058 | 对照性能基线账（ipc/pty 组用例耗时基线） | 260 | 骨架 | UNX-F11058-J1 基线逐组测量留痕（10/10），回归阈值冻结（10/10） |
| UNX-F11059 | 外部对照面回归纳入协议（常态化清单） | 260 | 骨架 | UNX-F11059-J1 纳入协议逐项立账（10/10），常态化清单闭合（10/10） |
| UNX-F11060 | B33 批收口（LTP 对照联签总断言） | 390 | 骨架 | UNX-F11060-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 13924 · #### UNX-C4-B34 · 五族全量回归矩阵（F11061–F11080 · 20 条） -->
#### UNX-C4-B34 · 五族全量回归矩阵（F11061–F11080 · 20 条）

> AI-14 承办｜域账累计：B01–B15 203,170 + 本批 5,330 = 208,500 / 240,000｜嫁接源：域 B01–B33 冻结判据面（回归重放零改写，原判据原口径）+ pipe(7)/signal(7)/pty(7)/unix(7)/svipc(7) 惯例条文（注出处，禁凭记忆）｜防重：与 B31–B33 防重：那三批立 C2 联签与 LTP 外部对照面，本批立域内五族全量回归矩阵；与 B20 防重：那批 M 型总收口单段回归，本批全域 B01–B33 矩阵覆盖；判定序挂接 F10879 冻结序表｜批注：I 型后段·五族全量回归矩阵：B01–B33 判据全域重放+常态化协议，收官条 F11080 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11061 | 回归矩阵场搭建（五族矩阵账与执行序） | 260 | 骨架 | UNX-F11061-J1 矩阵场全链搭建（10/10），执行序账冻结（10/10） |
| UNX-F11062 | 管道 FIFO 回归段（B01/B02 判据重放） | 260 | 骨架 | UNX-F11062-J1 管道段判据全量重放（10/10），重放与首跑一致（10/10） |
| UNX-F11063 | 信号回归段（B03/B04/B22 判据重放） | 260 | 骨架 | UNX-F11063-J1 信号段判据全量重放（10/10），投递语义一致（10/10） |
| UNX-F11064 | pty/termios/行规程回归段（B05–B07 判据重放） | 260 | 骨架 | UNX-F11064-J1 终端段判据全量重放（10/10），录制基线一致（10/10） |
| UNX-F11065 | 作业控制回归段（B08 判据重放） | 260 | 骨架 | UNX-F11065-J1 作业段判据全量重放（10/10），会话语义一致（10/10） |
| UNX-F11066 | unix socket 流式回归段（B09/B16/B17/B18 判据重放） | 260 | 骨架 | UNX-F11066-J1 流式段判据全量重放（10/10），连接语义一致（10/10） |
| UNX-F11067 | DGRAM/SEQPACKET/SCM_RIGHTS 回归段（B10/B11 判据重放） | 260 | 骨架 | UNX-F11067-J1 报文段判据全量重放（10/10），fd 传递一致（10/10） |
| UNX-F11068 | epoll/select/poll/futex 回归段（B12/B19/B26 判据重放） | 260 | 骨架 | UNX-F11068-J1 复用段判据全量重放（10/10），模式语义一致（10/10） |
| UNX-F11069 | SysV IPC 回归段（B13/B14/B27 判据重放） | 260 | 骨架 | UNX-F11069-J1 SysV 段判据全量重放（10/10），生命周期一致（10/10） |
| UNX-F11070 | POSIX IPC 回归段（B15/B28 判据重放） | 260 | 骨架 | UNX-F11070-J1 POSIX 段判据全量重放（10/10），命名语义一致（10/10） |
| UNX-F11071 | E 型八批回归段（B21–B28 错误矩阵判据重放） | 260 | 骨架 | UNX-F11071-J1 E 型段判据全量重放（10/10），双判据面一致（10/10） |
| UNX-F11072 | I 型回归段（B29/B30/B31/B32/B33 联测判据重放） | 260 | 骨架 | UNX-F11072-J1 I 型段判据全量重放（10/10），联测语义一致（10/10） |
| UNX-F11073 | M 型回归段（B09–B20 机制主体判据重放） | 260 | 骨架 | UNX-F11073-J1 M 型段判据全量重放（10/10），机制语义一致（10/10） |
| UNX-F11074 | F 型回归段（B01–B08 地基判据重放） | 260 | 骨架 | UNX-F11074-J1 F 型段判据全量重放（10/10），地基语义一致（10/10） |
| UNX-F11075 | 回归结果总账（五族矩阵三态汇总） | 260 | 骨架 | UNX-F11075-J1 矩阵三态账逐段对平（10/10），总账守恒闭合（10/10） |
| UNX-F11076 | 失配归因与修复回执账（回归失配处置） | 260 | 骨架 | UNX-F11076-J1 失配逐例归因留痕（10/10），修复回执账闭合（10/10） |
| UNX-F11077 | 回归常态化协议与 B33 LTP 纳入挂接 | 260 | 骨架 | UNX-F11077-J1 常态化协议逐项立账（10/10），LTP 挂接对平（10/10） |
| UNX-F11078 | 回归性能回归账（基线对照重放） | 260 | 骨架 | UNX-F11078-J1 基线重放逐组对平（10/10），阈值断言零越限（10/10） |
| UNX-F11079 | 回归矩阵总扫与封存（矩阵面移交注账） | 260 | 骨架 | UNX-F11079-J1 矩阵面封存逐件核销（10/10），移交注账零悬空（10/10） |
| UNX-F11080 | B34 批收口（五族回归矩阵总断言） | 390 | 骨架 | UNX-F11080-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 13951 · #### UNX-C4-B35 · 压力长稳演习（F11081–F11100 · 20 条） -->
#### UNX-C4-B35 · 压力长稳演习（F11081–F11100 · 20 条）

> AI-14 承办｜域账累计：B01–B15 208,500 + 本批 5,330 = 213,830 / 240,000｜嫁接源：pipe(7)/svipc(7)/sem_overview(7)/mq_overview(7)/pty(7)/unix(7)/epoll(7)/signal(7) 长稳惯例条文 + F10981 压测场冻结件（注出处，禁凭记忆）｜防重：与 B30 防重：那批立多进程 IPC 压测场单轮集成段，本批立万轮长稳与全族演习面；与 B29 防重：那批立 vim 单应用联测段，本批长稳演习含录制对照万轮；判定序挂接 F10879 冻结序表｜批注：I 型后段·压力长稳演习：八族万轮长稳+三哨网+域验收两落点核销，收官条 F11100 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11081 | 演习场搭建（长稳账与哨兵网） | 260 | 骨架 | UNX-F11081-J1 演习场全链搭建（10/10），哨兵网逐哨在位（10/10） |
| UNX-F11082 | 管道 FIFO 长稳段（多写多读万轮） | 260 | 骨架 | UNX-F11082-J1 管道万轮字节守恒（10/10），原子边界万轮零漂（10/10） |
| UNX-F11083 | SysV IPC 长稳段（shm/sem/msg 万轮） | 260 | 骨架 | UNX-F11083-J1 SysV 万轮账守恒（10/10），生命周期万轮零漏（10/10） |
| UNX-F11084 | POSIX IPC 长稳段（sem/mq 万轮） | 260 | 骨架 | UNX-F11084-J1 POSIX 万轮账守恒（10/10），命名周期万轮零漏（10/10） |
| UNX-F11085 | pty vim 全屏长稳段（录制对照万轮） | 260 | 骨架 | UNX-F11085-J1 vim 万轮回放零漂（10/10），逐帧对照账对平（10/10） |
| UNX-F11086 | unix socket 长稳段（连接风暴万轮） | 260 | 骨架 | UNX-F11086-J1 连接万轮账守恒（10/10），风暴恢复零残留（10/10） |
| UNX-F11087 | epoll 长稳段（百万事件驱动） | 260 | 骨架 | UNX-F11087-J1 事件账百万守恒（10/10），模式语义百万零漂（10/10） |
| UNX-F11088 | 信号长稳段（万次风暴与合并账） | 260 | 骨架 | UNX-F11088-J1 风暴万次账守恒（10/10），合并语义万次零漂（10/10） |
| UNX-F11089 | 内存账长稳监控（泄漏探针全网） | 260 | 骨架 | UNX-F11089-J1 内存账万轮零增长（10/10），泄漏探针零命中（10/10） |
| UNX-F11090 | fd/资源账长稳监控（fd 泄漏探针） | 260 | 骨架 | UNX-F11090-J1 fd 账万轮零泄漏（10/10），资源账逐族零漏（10/10） |
| UNX-F11091 | 混合负载长稳段（五族并发混跑万轮） | 260 | 骨架 | UNX-F11091-J1 混跑万轮互扰零命中（10/10），五族账独立对平（10/10） |
| UNX-F11092 | 长稳中断注入段（kill/重启恢复账） | 260 | 骨架 | UNX-F11092-J1 注入千次恢复对平（10/10），中断后账完整（10/10） |
| UNX-F11093 | 长稳数据完整性对账（全程守恒总账） | 260 | 骨架 | UNX-F11093-J1 全程守恒账对平（10/10），分段-全程账闭合（10/10） |
| UNX-F11094 | 长稳性能曲线账（退化探针与阈值） | 260 | 骨架 | UNX-F11094-J1 曲线逐段实测留痕（10/10），退化阈值零越限（10/10） |
| UNX-F11095 | 长稳哨兵总账（异常零静默断言） | 260 | 骨架 | UNX-F11095-J1 三哨总账逐段对平（10/10），异常零静默断言（10/10） |
| UNX-F11096 | 演习结果三态总账（绿/异常/待办汇总） | 260 | 骨架 | UNX-F11096-J1 三态账逐族对平（10/10），总账守恒闭合（10/10） |
| UNX-F11097 | 域验收判据落点对账（IPC 压测/vim 全屏两落点核销） | 260 | 骨架 | UNX-F11097-J1 落点逐件核销对平（10/10），验收判据零悬空（10/10） |
| UNX-F11098 | 演习封存与复跑指引（长稳面移交注账） | 260 | 骨架 | UNX-F11098-J1 演习面封存逐件核销（10/10），复跑指引可执行（10/10） |
| UNX-F11099 | 长稳常态化协议（纳入回归清单） | 260 | 骨架 | UNX-F11099-J1 常态化协议逐项立账（10/10），清单闭合断言（10/10） |
| UNX-F11100 | B35 批收口（压力长稳演习总断言） | 390 | 骨架 | UNX-F11100-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 13978 · #### UNX-C4-B36 · 联签总对账与 I 型总收口（F11101–F11120 · 20 条） -->
#### UNX-C4-B36 · 联签总对账与 I 型总收口（F11101–F11120 · 20 条）

> AI-14 承办｜域账累计：B01–B15 213,830 + 本批 5,330 = 219,160 / 240,000｜嫁接源：域 B29–B35 I 型六批冻结判据面（总对账重放零改写）+ C2 挂号注册协议冻结件 + R-C3-003 释放闸冻结面 + 八族惯例条文（注出处，禁凭记忆）｜防重：与 B31–B35 防重：那五批立挂号联签/LTP 对照/回归矩阵/长稳演习各实体面，本批立总对账与 I 型段级收口面（逐面终核+单值断言），判据零交集；判定序挂接 F10879 冻结序表｜批注：I 型后段·联签总对账与 I 型总收口：六批面终核+700 条里程碑锚 F11120，收官条 F11120 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11101 | I 型六批总账（B29–B35 段总对账） | 260 | 骨架 | UNX-F11101-J1 六批总账逐批对平（10/10），段账守恒闭合（10/10） |
| UNX-F11102 | 联签面总对账（C2 协议全挂点核销） | 260 | 骨架 | UNX-F11102-J1 挂点全账终核对平（10/10），核销零残留断言（10/10） |
| UNX-F11103 | LTP 对照面总对账（三态总账终核） | 260 | 骨架 | UNX-F11103-J1 三态总账终核对平（10/10），豁免率终核在闸内（10/10） |
| UNX-F11104 | 回归矩阵面总对账（B34 总账终核） | 260 | 骨架 | UNX-F11104-J1 矩阵总账终核对平（10/10），常态化协议挂接终核（10/10） |
| UNX-F11105 | 演习面总对账（B35 三态终核） | 260 | 骨架 | UNX-F11105-J1 演习总账终核对平（10/10），验收落点核销复断（10/10） |
| UNX-F11106 | 跨域对账面总核（C2 挂号/C3 释放闸/R-C4-002） | 260 | 骨架 | UNX-F11106-J1 跨域账逐面终核（10/10），承接账零悬空断言（10/10） |
| UNX-F11107 | 判据库总盘点（域 700 判据全量在册核验） | 260 | 骨架 | UNX-F11107-J1 判据全量盘点对平（10/10），编号连续零断档（10/10） |
| UNX-F11108 | 冻结件清单总核（域冻结件全量在册） | 260 | 骨架 | UNX-F11108-J1 冻结件全量终核对平（10/10），版本戳零漂移（10/10） |
| UNX-F11109 | 豁免账总核（域豁免零悬空） | 260 | 骨架 | UNX-F11109-J1 豁免账逐例终核（10/10），ADR 挂账零悬空（10/10） |
| UNX-F11110 | 风险台账总核（R-C4-001/002/003 终态） | 260 | 骨架 | UNX-F11110-J1 风险项逐条终核对平（10/10），终态处置零在途（10/10） |
| UNX-F11111 | 判据重放域级抽检（跨批抽样复核） | 260 | 骨架 | UNX-F11111-J1 抽检逐条复测对平（10/10），抽检覆盖六批达标（10/10） |
| UNX-F11112 | I 型总收口断言（六批全绿单值） | 260 | 骨架 | UNX-F11112-J1 六批全绿单值断言成立（10/10），收口账版本冻结（10/10） |
| UNX-F11113 | 域 700 条里程碑账（F11120 锚定） | 260 | 骨架 | UNX-F11113-J1 里程碑账三数对平（10/10），锚定断言成立（10/10） |
| UNX-F11114 | 段级收官账（B31–B36 段 120 条断言） | 260 | 骨架 | UNX-F11114-J1 段级断言三数对平（10/10），六批账闭合（10/10） |
| UNX-F11115 | I 型面封存（场/账/协议三件） | 260 | 骨架 | UNX-F11115-J1 三件封存逐件核销（10/10），移交注账零悬空（10/10） |
| UNX-F11116 | 交接声明账（B37 清账段接力） | 260 | 骨架 | UNX-F11116-J1 接力账逐项对平（10/10），接力断言成立（10/10） |
| UNX-F11117 | 联签总对账重放（六批判据全量重放） | 260 | 骨架 | UNX-F11117-J1 全量重放逐批对平（10/10），重放与首跑一致（10/10） |
| UNX-F11118 | I 型常态化协议（联测纳入回归） | 260 | 骨架 | UNX-F11118-J1 协议三段逐项立账（10/10），清单闭合断言（10/10） |
| UNX-F11119 | 跨域移交注账（联签面移交声明） | 260 | 骨架 | UNX-F11119-J1 移交账逐件对平（10/10），移交断言零悬空（10/10） |
| UNX-F11120 | B36 批收口（联签总对账与 I 型总收口锚） | 390 | 骨架 | UNX-F11120-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 14005 · #### UNX-C4-B37 · 清账一段 · B01–B20 判据终重放（F11121–F11140 · 20 条） -->
#### UNX-C4-B37 · 清账一段 · B01–B20 判据终重放（F11121–F11140 · 20 条）

> AI-14 承办｜域账累计：B01–B15 219,160 + 本批 5,210 = 224,370 / 240,000｜嫁接源：域 B01–B20 冻结判据面（终重放零改写）+ F11061 回归矩阵场冻结件 + F11116 接力账（I 型段终点 219,160）（注出处，禁凭记忆）｜防重：与 B34 防重：那批立 I 型段内例行回归矩阵（B01–B33 覆盖），本批立 C 型域收官清账（B01–B20 段终重放+清账账闭合），两面批账分离判据零交集；判定序挂接 F10879 冻结序表｜批注：C 型收官·清账一段：B01–B20 二十批 400 条判据终重放+清账账闭合，收官条 F11140 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11121 | 清账场搭建（全域清账账与 B01–B20 执行序） | 260 | 骨架 | UNX-F11121-J1 清账场全链搭建（10/10），执行序账冻结（10/10） |
| UNX-F11122 | B01–B02 清账（管道族判据终重放） | 260 | 骨架 | UNX-F11122-J1 管道族判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11123 | B03–B04 清账（信号族判据终重放） | 260 | 骨架 | UNX-F11123-J1 信号族判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11124 | B05–B06 清账（pty/termios 判据终重放） | 260 | 骨架 | UNX-F11124-J1 终端族判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11125 | B07 清账（行规程与录制基线终重放） | 260 | 骨架 | UNX-F11125-J1 行规程判据终重放（10/10），三基线对照闭合（10/10） |
| UNX-F11126 | B08 清账（作业控制判据终重放） | 260 | 骨架 | UNX-F11126-J1 作业族判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11127 | B09–B10 清账（unix socket 主体判据终重放） | 260 | 骨架 | UNX-F11127-J1 socket 族判据终重放（10/10），三型账逐条闭合（10/10） |
| UNX-F11128 | B11 清账（SCM_RIGHTS/epoll 主体判据终重放） | 260 | 骨架 | UNX-F11128-J1 传递与复用判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11129 | B12 清账（select/poll/futex 判据终重放） | 260 | 骨架 | UNX-F11129-J1 适配与同步判据终重放（10/10），清账账逐条闭合（10/10） |
| UNX-F11130 | B13–B14 清账（SysV shm/sem/msg 判据终重放） | 260 | 骨架 | UNX-F11130-J1 SysV 族判据终重放（10/10），生命周期账闭合（10/10） |
| UNX-F11131 | B15 清账（POSIX IPC 判据终重放） | 260 | 骨架 | UNX-F11131-J1 POSIX 族判据终重放（10/10），混合场账闭合（10/10） |
| UNX-F11132 | B16–B17 清账（socket 深水判据终重放） | 260 | 骨架 | UNX-F11132-J1 深水族判据终重放（10/10），连接账逐条闭合（10/10） |
| UNX-F11133 | B18 清账（SOL_SOCKET 选项族判据终重放） | 260 | 骨架 | UNX-F11133-J1 选项族判据终重放（10/10），选项账逐条闭合（10/10） |
| UNX-F11134 | B19 清账（epoll 深水判据终重放） | 260 | 骨架 | UNX-F11134-J1 epoll 深水判据终重放（10/10），模式账逐条闭合（10/10） |
| UNX-F11135 | B20 清账（M 型总收口判据终重放） | 260 | 骨架 | UNX-F11135-J1 总收口判据终重放（10/10），交互矩阵账闭合（10/10） |
| UNX-F11136 | 清账段一总账（B01–B20 段账闭合） | 260 | 骨架 | UNX-F11136-J1 段账三数对平（10/10），清账账闭合断言（10/10） |
| UNX-F11137 | 清账段一失配处置账（终重放失配闭环） | 260 | 骨架 | UNX-F11137-J1 失配逐例归因处置（10/10），处置账闭合零在途（10/10） |
| UNX-F11138 | 清账段一封存（段一账册移交注账） | 260 | 骨架 | UNX-F11138-J1 三件封存逐件核销（10/10），移交注账零悬空（10/10） |
| UNX-F11139 | 清账段一接力账（向 B38 段二接力） | 260 | 骨架 | UNX-F11139-J1 接力账逐项对平（10/10），接力断言成立（10/10） |
| UNX-F11140 | B37 批收口（清账一段总断言） | 270 | 骨架 | UNX-F11140-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 14032 · #### UNX-C4-B38 · 清账二段 · B21–B36 判据终重放（F11141–F11160 · 20 条） -->
#### UNX-C4-B38 · 清账二段 · B21–B36 判据终重放（F11141–F11160 · 20 条）

> AI-14 承办｜域账累计：B01–B15 224,370 + 本批 5,210 = 229,580 / 240,000｜嫁接源：域 B21–B36 冻结判据面（终重放零改写）+ F11121 清账场冻结件 + F11139 接力账（段一终点）（注出处，禁凭记忆）｜防重：与 B34 防重：那批立 I 型段内例行回归矩阵，本批立 C 型域收官清账（B21–B36 段终重放+清账账闭合）；与 B71–B72 之类例行面零交集（域内无该批号）；判定序挂接 F10879 冻结序表｜批注：C 型收官·清账二段：B21–B36 十六批 320 条判据终重放+清账账闭合+域 36 批合账（范围实排勘误注记在册），收官条 F11160 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11141 | 清账场二搭建（B21–B36 执行序与范围勘误注记） | 260 | 骨架 | UNX-F11141-J1 清账场二全链搭建（10/10），范围勘误注记在册（10/10） |
| UNX-F11142 | B21 清账（管道 FIFO 错误矩阵判据终重放） | 260 | 骨架 | UNX-F11142-J1 管道错误面判据终重放（10/10），双判据账逐条闭合（10/10） |
| UNX-F11143 | B22 清账（信号错误矩阵判据终重放） | 260 | 骨架 | UNX-F11143-J1 信号错误面判据终重放（10/10），双判据账逐条闭合（10/10） |
| UNX-F11144 | B23 清账（pty/termios 错误矩阵判据终重放） | 260 | 骨架 | UNX-F11144-J1 终端错误面判据终重放（10/10），四格账逐条闭合（10/10） |
| UNX-F11145 | B24–B25 清账（unix socket 错误矩阵两批判据终重放） | 260 | 骨架 | UNX-F11145-J1 socket 错误面判据终重放（10/10），序表挂接账闭合（10/10） |
| UNX-F11146 | B26 清账（复用器错误矩阵判据终重放） | 260 | 骨架 | UNX-F11146-J1 复用错误面判据终重放（10/10），四码账逐条闭合（10/10） |
| UNX-F11147 | B27 清账（SysV IPC 错误矩阵判据终重放） | 260 | 骨架 | UNX-F11147-J1 SysV 错误面判据终重放（10/10），回滚账逐条闭合（10/10） |
| UNX-F11148 | B28 清账（POSIX IPC 错误矩阵判据终重放） | 260 | 骨架 | UNX-F11148-J1 POSIX 错误面判据终重放（10/10），能力表账闭合（10/10） |
| UNX-F11149 | B29 清账（vim 全屏联测判据终重放） | 260 | 骨架 | UNX-F11149-J1 vim 联测判据终重放（10/10），全链账逐条闭合（10/10） |
| UNX-F11150 | B30 清账（多进程 IPC 压测判据终重放） | 260 | 骨架 | UNX-F11150-J1 压测判据终重放（10/10），四机制账逐条闭合（10/10） |
| UNX-F11151 | B31–B32 清账（挂号联签两批判据终重放） | 260 | 骨架 | UNX-F11151-J1 联签判据终重放（10/10），挂号账逐条闭合（10/10） |
| UNX-F11152 | B33 清账（LTP 对照判据终重放） | 260 | 骨架 | UNX-F11152-J1 对照判据终重放（10/10），三态账逐条闭合（10/10） |
| UNX-F11153 | B34 清账（回归矩阵判据终重放） | 260 | 骨架 | UNX-F11153-J1 矩阵判据终重放（10/10），常态账逐条闭合（10/10） |
| UNX-F11154 | B35–B36 清账（演习与 I 型收口两批判据终重放） | 260 | 骨架 | UNX-F11154-J1 演习与收口判据终重放（10/10），段账逐条闭合（10/10） |
| UNX-F11155 | 清账段二总账（B21–B36 段账闭合+段一对平） | 260 | 骨架 | UNX-F11155-J1 段账三数对平（10/10），与段一总账对平（10/10） |
| UNX-F11156 | 域 36 批清账合账（B01–B36 全域清账总账） | 260 | 骨架 | UNX-F11156-J1 合账三数对平（10/10），域清账闭合断言（10/10） |
| UNX-F11157 | 清账段二失配处置账（终重放失配闭环） | 260 | 骨架 | UNX-F11157-J1 失配逐例归因处置（10/10），处置账闭合零在途（10/10） |
| UNX-F11158 | 清账段二封存（段二账册移交注账） | 260 | 骨架 | UNX-F11158-J1 三件封存逐件核销（10/10），移交注账零悬空（10/10） |
| UNX-F11159 | 清账段二接力账（向 B39 验收总核销接力） | 260 | 骨架 | UNX-F11159-J1 接力账逐项对平（10/10），接力断言成立（10/10） |
| UNX-F11160 | B38 批收口（清账二段总断言） | 270 | 骨架 | UNX-F11160-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 14059 · #### UNX-C4-B39 · 域验收总核销 · 域经快照 · 移交包三件（F11161–F11180 · 20 条 -->
#### UNX-C4-B39 · 域验收总核销 · 域经快照 · 移交包三件（F11161–F11180 · 20 条）

> AI-14 承办｜域账累计：B01–B15 229,580 + 本批 5,210 = 234,790 / 240,000｜嫁接源：域 B01–B38 全域冻结判据面（验收核销零改写）+ F11121/F11141 清账场冻结件 + C3 宪法豁免条款条文（注出处，禁凭记忆）｜防重：与 B37/B38 防重：那两批立分段清账终重放面，本批立域级验收总核销+域经沉淀+移交打包面（账面分层：批账/段账/域账三层），判据零交集；判定序挂接 F10879 冻结序表｜批注：C 型收官·域验收总核销：800 条核销对账+域经三快照+移交包三件，收官条 F11180 立批断言

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11161 | 验收总核销场搭建（域 800 条验收账） | 260 | 骨架 | UNX-F11161-J1 验收场全链搭建（10/10），域验收账冻结（10/10） |
| UNX-F11162 | 域 800 条判据全量核销对账 | 260 | 骨架 | UNX-F11162-J1 核销逐条对平（10/10），核销率单值成立（10/10） |
| UNX-F11163 | 域 40 批三态终核（满账前核验） | 260 | 骨架 | UNX-F11163-J1 三态逐批终核对平（10/10），38 批已深化断言（10/10） |
| UNX-F11164 | 域行数总账终核（满账前账面核验） | 260 | 骨架 | UNX-F11164-J1 行数账逐批对平（10/10），累计链连续断言（10/10） |
| UNX-F11165 | 判据库终盘（800 条连续零断档终验） | 260 | 骨架 | UNX-F11165-J1 全库终盘逐号对平（10/10），连续断言终验成立（10/10） |
| UNX-F11166 | 冻结件清单终版（域冻结件总登记） | 260 | 骨架 | UNX-F11166-J1 清单逐件终核对平（10/10），版本戳零漂移（10/10） |
| UNX-F11167 | 风险台账终版（R-C4-001/002/003 终态落账） | 260 | 骨架 | UNX-F11167-J1 风险逐条终态对平（10/10），零在途断言成立（10/10） |
| UNX-F11168 | 豁免账终版（域豁免清单终核） | 260 | 骨架 | UNX-F11168-J1 豁免逐例终核对平（10/10），豁免率终断在闸内（10/10） |
| UNX-F11169 | 域经快照一（判例汇编：R-C4-001 账实修正） | 260 | 骨架 | UNX-F11169-J1 判例事实链留痕（10/10），可复查断言成立（10/10） |
| UNX-F11170 | 域经快照二（检查器口径勘误判例汇编） | 260 | 骨架 | UNX-F11170-J1 勘误判例三例汇编（10/10），纪律条文化断言（10/10） |
| UNX-F11171 | 域经快照三（并发协作判例汇编） | 260 | 骨架 | UNX-F11171-J1 协作判例事实链留痕（10/10），零裹挟断言成立（10/10） |
| UNX-F11172 | 移交包一件（深化册 40 册打包清单） | 260 | 骨架 | UNX-F11172-J1 打包清单逐册对平（10/10），40 册实体核销（10/10） |
| UNX-F11173 | 移交包二件（批册 40 册打包清单） | 260 | 骨架 | UNX-F11173-J1 打包清单逐册对平（10/10），40 册实体核销（10/10） |
| UNX-F11174 | 移交包三件（场/账/协议/基线全家当清单） | 260 | 骨架 | UNX-F11174-J1 全家当清单逐件对平（10/10），实体核销零漏件（10/10） |
| UNX-F11175 | 验收总核销断言（域级单值） | 260 | 骨架 | UNX-F11175-J1 域级单值断言成立（10/10），核销账版本冻结（10/10） |
| UNX-F11176 | 移交接收账（下游消费声明） | 260 | 骨架 | UNX-F11176-J1 接收位逐件对平（10/10），零悬空断言成立（10/10） |
| UNX-F11177 | 域验收封存（验收面全家当封存） | 260 | 骨架 | UNX-F11177-J1 封存逐件核销（10/10），复跑指引可执行（10/10） |
| UNX-F11178 | 接力账（向 B40 域闭账段） | 260 | 骨架 | UNX-F11178-J1 接力账逐项对平（10/10），接力断言成立（10/10） |
| UNX-F11179 | B39 段总账（验收段账闭合） | 260 | 骨架 | UNX-F11179-J1 段账三数对平（10/10），验收账闭合（10/10） |
| UNX-F11180 | B39 批收口（域验收总核销批断言） | 270 | 骨架 | UNX-F11180-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计对平（10/10） |

<!-- 主册行 14086 · #### UNX-C4-B40 · 域闭账 · F11200 收官锚三断言+240,000 满账宣告（F11181–F1 -->
#### UNX-C4-B40 · 域闭账 · F11200 收官锚三断言+240,000 满账宣告（F11181–F11200 · 20 条）

> AI-14 承办｜域账累计：B01–B15 234,790 + 本批 5,210 = 240,000 / 240,000｜嫁接源：域 B01–B39 全域冻结判据面（闭账核验零改写）+ F11178 接力账（验收段终点 234,790）+ F11000 域 300 条总收口判例（收官锚 460 行规格）（注出处，禁凭记忆）｜防重：与 B37/B38/B39 防重：那三批立分段清账与域验收面，本批立域闭账面（三断言执行+满账宣告+域全家当封存），账面分层（批/段/域闭账四层）判据零交集；判定序挂接 F10879 冻结序表｜批注：C 型收官·域闭账：三断言执行+240,000 满账宣告+域全家当封存，F11200 域收官锚（460 行规格沿 F11000 判例）

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F11181 | 闭账场搭建（域闭账账与终账序） | 250 | 骨架 | UNX-F11181-J1 闭账场全链搭建（10/10），终账序冻结（10/10） |
| UNX-F11182 | 域 240,000 行满账终对账（四十批和终验） | 250 | 骨架 | UNX-F11182-J1 四十批行数和终对平（10/10），满账断言预备成立（10/10） |
| UNX-F11183 | 域 800 条满条终对账（四十批×20 终验） | 250 | 骨架 | UNX-F11183-J1 条数终对平（10/10），800 单值预备成立（10/10） |
| UNX-F11184 | 域字数总账终核（深化册四十册字数和） | 250 | 骨架 | UNX-F11184-J1 字数账逐册对平（10/10），总字数单值成立（10/10） |
| UNX-F11185 | 域判据终盘点（800 判据唯一连续终验） | 250 | 骨架 | UNX-F11185-J1 全库终盘逐号对平（10/10），唯一连续双断言成立（10/10） |
| UNX-F11186 | 防重四范围终扫（全域唯一+跨域零侵入终验） | 250 | 骨架 | UNX-F11186-J1 四范围终扫零命中（10/10），跨域边界断言成立（10/10） |
| UNX-F11187 | handoff C4 块终态预写核验 | 250 | 骨架 | UNX-F11187-J1 预写字段逐项核验（10/10），JSON 合法性断言（10/10） |
| UNX-F11188 | 台账三处终态预写核验（总纲/根台账/域小结） | 250 | 骨架 | UNX-F11188-J1 预写锚逐处核验（10/10），三处锚唯一性断言（10/10） |
| UNX-F11189 | 域收官标志条规格核验（F11200 三断言预检） | 250 | 骨架 | UNX-F11189-J1 规格逐项预检（10/10），三断言定义冻结（10/10） |
| UNX-F11190 | 域经汇编终版（三快照合订） | 250 | 骨架 | UNX-F11190-J1 合订逐篇对平（10/10），域经单值成立（10/10） |
| UNX-F11191 | 移交包终验（三件打包核销） | 250 | 骨架 | UNX-F11191-J1 三包终验逐件对平（10/10），打包核销零漏件（10/10） |
| UNX-F11192 | 上游消费核销账（C2 挂号/C3 释放闸终核销） | 250 | 骨架 | UNX-F11192-J1 上游账逐面终核销（10/10），双向闭合断言（10/10） |
| UNX-F11193 | 下游冻结声明账（C4 冻结接口移交声明） | 250 | 骨架 | UNX-F11193-J1 冻结件逐件声明对平（10/10），消费指南零悬空（10/10） |
| UNX-F11194 | 域闭账断言一（800/800 满条单值） | 250 | 骨架 | UNX-F11194-J1 满条断言单值成立（10/10），断言账版本冻结（10/10） |
| UNX-F11195 | 域闭账断言二（240,000/240,000 满账单值） | 250 | 骨架 | UNX-F11195-J1 满账断言单值成立（10/10），断言账版本冻结（10/10） |
| UNX-F11196 | 域闭账断言三（40 批全 [已深化] 满态单值） | 250 | 骨架 | UNX-F11196-J1 满态断言单值成立（10/10），断言账版本冻结（10/10） |
| UNX-F11197 | 域闭账单值宣告（三断言合一） | 250 | 骨架 | UNX-F11197-J1 三断言合一单值成立（10/10），宣告账版本冻结（10/10） |
| UNX-F11198 | 域闭账封存（域全家当封存） | 250 | 骨架 | UNX-F11198-J1 封存逐件核销（10/10），复跑指引可执行（10/10） |
| UNX-F11199 | 域经终接力账（向全项目账移交） | 250 | 骨架 | UNX-F11199-J1 移交账逐项对平（10/10），终接力断言成立（10/10） |
| UNX-F11200 | UNX-C4 域闭账收官锚（三断言执行+240,000 满账宣告） | 460 | 骨架 | UNX-F11200-J1 三断言执行全绿（10/10），240,000 满账宣告成立（10/10），800 条满条宣告成立（10/10） |

<a id="dom-C5"></a>

