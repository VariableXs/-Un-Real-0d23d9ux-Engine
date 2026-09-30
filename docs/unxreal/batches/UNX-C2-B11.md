# UNX-C2-B11 · 信号挂点族（F9001–F9020 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B10 45,140 + 本批 4,320 = 49,460 / 240,000｜嫁接源：Linux man-pages sigaction(2)/signal(7) 章节、x86-64 SysV ABI（信号栈帧 rt_sigframe 布局）、现存 `syscall::guard` CAP_* 存量档为升级接管扩容｜防重：信号硬件中断本体与 APIC 路由归 A2 域防重——本域为信号 syscall 号面语义与投递消费协议；signalfd 消费端已在 B10（F8989）铺面，本批只立挂点协议不重铺｜判据与 deepen/C2-B11.md 逐条同名同判据同 ID

### UNX-F9001 · rt_sigaction 语义档
- 域/批：C2/B11｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9001-J1 sigaction 结构布局（handler/mask/flags）ABI 冻结判据过，旧 action 读回判据过，SA_* 五旗标（RESTART/NOCLDSTOP/NODEFER/ONSTACK/SIGINFO）逐位判据过
### UNX-F9002 · rt_sigprocmask 掩码语义
- 域/批：C2/B11｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F9002-J1 SIG_BLOCK/UNBLOCK/SETMASK 三操作判据过，旧掩码读回判据过，SIGKILL/SIGSTOP 掩蔽请求被忽略判据过
### UNX-F9003 · kill/tgkill/tkill 投递
- 域/批：C2/B11｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9003-J1 三投递号目标解析（pid/tgid/tid）判据过，信号 0 探活语义判据过，跨进程投递权限 EPERM 格过
### UNX-F9004 · raise/pidfd_send_signal 挂点
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9004-J1 raise 自投递判据过，pidfd 号面挂点注册判据过（fd 化投递本体档待 M 型后续批次落），未实现路径返 -ENOSYS 判据过
### UNX-F9005 · sigreturn 返回路径（信号栈帧 ABI）
- 域/批：C2/B11｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9005-J1 rt_sigframe 布局（uc_flags/uc_link/uc_mcontext/uc_sigmask）对照 SysV ABI 冻结判据过，寄存器组（含 r10 槽位）恢复零差判据过，坏栈帧返 EFAULT 判据过
### UNX-F9006 · 默认处置表（SIG_DFL/SIG_IGN/SIG_ERR 全集）
- 域/批：C2/B11｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9006-J1 1–31 号默认处置（Term/Ign/Core/Stop）逐号对照 signal(7) 表判据过，处置切换生效时点判据过
### UNX-F9007 · 标准信号 1–31 与实时信号 34–64 登记簿
- 域/批：C2/B11｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9007-J1 全 64 号登记簿落账判据过，标准号不排队/实时号排队（F9008）分档判据过，32/33 保留间隙语义对照判据过
### UNX-F9008 · 信号队列与排队语义（rt 信号排队）
- 域/批：C2/B11｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9008-J1 实时信号多次投递多次送达判据过，队列上限 RLIMIT_SIGPENDING 触达 EAGAIN 格过，标准信号合并投递判据过
### UNX-F9009 · SIGKILL/SIGSTOP 不可捕获单点
- 域/批：C2/B11｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9009-J1 捕获/掩蔽/忽略三路径对 KILL/STOP 均无效判据过，KILL 组播 exit_group 联动判据过
### UNX-F9010 · sigaltstack 与 SA_ONSTACK
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9010-J1 备用栈注册与切换判据过，栈溢出防护（stack_t 尺寸校验）判据过，禁用后 ONSTACK 忽略判据过
### UNX-F9011 · sigpending/sigtimedwait/sigwaitinfo
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9011-J1 pending 集读回判据过，同步等待未决信号三号（timedwait 超时 EAGAIN 格）判据过
### UNX-F9012 · 信号与线程组路由（clone CLONE_SIGHAND 联动）
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9012-J1 共享 handler 表（CLONE_SIGHAND）判据过，tgkill 定向线程投递判据过，A3 SMP 就位后复测登记
### UNX-F9013 · 信号与 EINTR 联动协议（SA_RESTART 消费）
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9013-J1 SA_RESTART 置位→慢调用自动重启判据过，未置位→返 EINTR 判据过，单点与 F8948 锚点一致回归
### UNX-F9014 · 信号挂点与 guard.rs CAP 联动（SIGPROF/SIGALRM）
- 域/批：C2/B11｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9014-J1 计时信号触发链（itimer→SIGALRM/SIGPROF）判据过，CAP 门校验联动判据过
### UNX-F9015 · setitimer/getitimer 间隔定时器
- 域/批：C2/B11｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9015-J1 ITIMER_REAL/VIRTUAL/PROF 三档判据过，interval 周期性触发判据过，旧值读回判据过
### UNX-F9016 · alarm 兼容挂点
- 域/批：C2/B11｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9016-J1 alarm 返回剩余秒判据过，seconds=0 取消语义判据过，与 setitimer(ITIMER_REAL) 互斥语义判据过
### UNX-F9017 · 信号错误矩阵（EINVAL/ESRCH/EPERM/EBADF）
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9017-J1 四错误码 × 4 号（kill/sigaction/sigprocmask/pidfd_send_signal）矩阵 16 格逐格一致
### UNX-F9018 · 信号与多路复用竞态防护（signalfd/ppoll 联动档）
- 域/批：C2/B11｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9018-J1 掩码原子切换防丢信号协议判据过，signalfd（B10 F8989）/ppoll（B09 F8970）两消费路径联动回归
### UNX-F9019 · 信号族 ktest 半自动驱动（投递-捕获-返回三段探针）
- 域/批：C2/B11｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9019-J1 三段探针（用户态 handler 注册→内核投递→sigreturn 返回）驱动器判据过，10 号以上信号轮转覆盖
### UNX-F9020 · ktest syscall 面 B11 批断言集
- 域/批：C2/B11｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F9020-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，信号断言独立编号可单独复跑
