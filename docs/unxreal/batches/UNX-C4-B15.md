# UNX-C4-B15 · POSIX sem/mq/eventfd/timerfd/混合压测场（F10681–F10700 · 20 条）

> AI-14 承办｜域账累计：B01–B14 79,460 + 本批 6,220 = 85,680 / 240,000｜嫁接源：Linux sem_overview(7)/sem_open(3)/mq_overview(7)/eventfd(2)/timerfd_create(2)（注出处，禁凭记忆）｜防重：POSIX 命名 sem 与 SysV sem 不同族（F10679 总表核销）；mq 与 SysV msg 不同族（fd 面 vs 键面）；eventfd/timerfd 是"fd 化的计数器/计时器"（与 pipe 防重：eventfd 计数语义非字节流）｜批注：本批为 B01–B15（F 型地基+M 型机制）总收口批——三场混合压测（F10695-F10697）+ 判据主轴总回归（F10698）+ LTP 映射收口（F10699）+ 域移交声明（F10700）；B16–B40 待后续会话承接。

### UNX-F10681 · sem_open 命名信号量与 /dev/shm 声明
- 域/批：C4/B15｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10681-J1 名字空间（/name 格式）查/建双路 100/100，挂载点声明（Varix /dev/shm 面联签 B1）落账，非法名 EINVAL 面
### UNX-F10682 · sem_wait/sem_trywait/sem_post
- 域/批：C4/B15｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10682-J1 三操作行为全对（wait 阻塞/trywait EAGAIN/post 唤醒 各 1 千次），EINTR 面（B03 联签）10/10
### UNX-F10683 · sem 链接计数与 unlink 延迟销毁
- 域/批：C4/B15｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10683-J1 unlink 后旧打开句柄可用、新 open ENOENT（10/10），引用归零即销毁（账对平）
### UNX-F10684 · sem_getvalue
- 域/批：C4/B15｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10684-J1 返回值与实际计数恒等（操作 1 万次对平），负值语义（等待者数，注 sem_getvalue(3)）账
### UNX-F10685 · mq_open 消息队列描述字
- 域/批：C4/B15｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10685-J1 mq_open 查/建双路 100/100，O_CREAT+attr 容量面，O_NONBLOCK fd 态面
### UNX-F10686 · mq_send/mq_receive 优先级
- 域/批：C4/B15｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10686-J1 优先级队列（高先出 1 万消息），同优先级 FIFO 保持，两级序账对平
### UNX-F10687 · mq_notify 预埋（显式 ENOSYS）
- 域/批：C4/B15｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10687-J1 mq_notify 显性 ENOSYS 红账，send/recv 零误伤（反向 100/100）
### UNX-F10688 · mq_setattr/mq_getattr
- 域/批：C4/B15｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10688-J1 attr 结构逐字段正确（100/100），NONBLOCK 切换即时生效（10/10），curmsgs 对平
### UNX-F10689 · eventfd 计数器（EFD_SEMAPHORE）
- 域/批：C4/B15｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10689-J1 write 累加/read 取值（普通态整取、SEMAPHORE 态减一）双面各 1 万次，计数上限 EINVAL 面
### UNX-F10690 · eventfd 非阻塞与 EAGAIN
- 域/批：C4/B15｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10690-J1 EFD_NONBLOCK 下零值读 EAGAIN（1 万次），阻塞态唤醒链（写入即醒 100/100）
### UNX-F10691 · timerfd 计时器面
- 域/批：C4/B15｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10691-J1 create/settime/time 三面全对（过期次数读出），TFD_NONBLOCK/CLOEXEC 位面，精度账（ktest 粒度）
### UNX-F10692 · timerfd 与 A3 时钟联签
- 域/批：C4/B15｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10692-J1 定时触发经 A3 时钟接口（签名核验），过期→就绪→读出链账四拍单调
### UNX-F10693 · eventfd/timerfd 与 epoll 统一面
- 域/批：C4/B15｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10693-J1 两 fd 就绪面挂接（第五/六实现方），epoll 混合监控事件正确分流（1 万次）
### UNX-F10694 · POSIX IPC 与 SysV IPC 对照账（F10679 核销）
- 域/批：C4/B15｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10694-J1 对照账逐行核销（sem 族/mq 族四行），两族判据零越界（grep 断言）
### UNX-F10695 · 混合压测场一：pty vim + unix socket + epoll 组合
- 域/批：C4/B15｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F10695-J1 全屏模拟器（pty+raw）+ socket 服务循环（epoll）同场 1 万轮：零串扰零丢失，录制器对照全绿
### UNX-F10696 · 混合压测场二：多进程管道 + futex + shm
- 域/批：C4/B15｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F10696-J1 16 进程（管道族间通信+futex 同步+shm 共享缓冲）1 万轮流水线：守恒全绿零死锁
### UNX-F10697 · 混合压测场三：信号风暴 + IPC 压测
- 域/批：C4/B15｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F10697-J1 信号风暴（实时信号 1 万发）叠加 IPC 压测：零丢失零错递，EINTR 矩阵全绿
### UNX-F10698 · 判据主轴总回归（vim 录制对照全链）
- 域/批：C4/B15｜纯功能行数：400｜状态：[骨架]｜判据：UNX-F10698-J1 主轴全链（pty vim 类全屏：raw 切换/按键流/重绘/SIGWINCH/^Z/fg）录制对照 10 场景零差异
### UNX-F10699 · LTP 全族映射账收口
- 域/批：C4/B15｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10699-J1 C4 域 LTP 映射账（pipe/signal/pty/socket/futex/ipc 族）覆盖/待补两态收口，待补项带原因码
### UNX-F10700 · B15 与域 C4 收官移交（B16–B40 待领声明）
- 域/批：C4/B15｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F10700-J1 域账收口（85,680/240,000 · 300 条 finalize 全过），B16–B40 待领交接清单（主题批注齐备），挂号面全域核验账
