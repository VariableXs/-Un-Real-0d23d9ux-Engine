# UNX-C4-B12 · select/poll 适配/futex 四操作与竞态再验（F10621–F10640 · 20 条）

> AI-14 承办｜域账累计：B01–B11 62,360 + 本批 5,780 = 68,140 / 240,000｜嫁接源：Linux select(2)/poll(2)/pselect(2)/futex(2)（注出处，禁凭记忆）｜防重：select/poll 是 epoll 统一 poll 面之上的"适配层"（复用 B11 就绪面与等待机制，不另建监控结构）；futex 是线程同步原语（与 A3 调度联签 F10633，与 fd 无关）——两个主题同批不混账，判据逐条可辨｜批注：lost-wakeup 防线（F10636）是 futex 竞态的总防线条（先查后挂序在 F10492/F10575/F10612 已铺）；F10640 收口含 C2 挂号（select/poll/futex 族）。

### UNX-F10621 · select() fd_set 位图主体
- 域/批：C4/B12｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10621-J1 三集（readfds/writefds/exceptfds）语义全对（就绪置位/未就绪清零 100/100），返回值==就绪 fd 总数对账 1 万轮
### UNX-F10622 · FD_ZERO/FD_SET/FD_CLR/FD_ISSET 宏面
- 域/批：C4/B12｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10622-J1 四宏行为与位图操作语义一致（100/100），越界 fd 面显性（兼容层宏行为账）
### UNX-F10623 · select 超时与 timeval 结构
- 域/批：C4/B12｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10623-J1 timeout=0 轮询态、NULL 永阻塞、有限超时返 0 三态 100/100，超时值被内核更新（剩余时间语义，注 select(2)）10/10
### UNX-F10624 · poll() pollfd 数组主体
- 域/批：C4/B12｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10624-J1 pollfd 数组（fd/events/revents）语义全对（revents 回填 100/100），events=0 仅报错误/挂起类（语义账）
### UNX-F10625 · POLLIN/POLLOUT/POLLHUP/POLLERR/POLLNVAL
- 域/批：C4/B12｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10625-J1 五事件位与统一 poll 面就绪位一致（1 万次对平），POLLNVAL（坏 fd）独立于 events 请求（10/10）
### UNX-F10626 · select/poll 到 epoll 适配层
- 域/批：C4/B12｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10626-J1 适配层复用统一 poll 面就绪接口（零独立监控结构断言），select/poll 与 epoll 同场景行为一致（交叉 1 万轮零差异）
### UNX-F10627 · pselect/ppoll 信号掩码原子切换（C6 联签）
- 域/批：C4/B12｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10627-J1 掩码原子切换（查-改-复原子序）零竞窗（1 万次），C6 联签（sigprocmask 接口）签名一致，信号打断 EINTR 面
### UNX-F10628 · nfds 上限与 FD_SETSIZE
- 域/批：C4/B12｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10628-J1 FD_SETSIZE=1024 边界（越界宏行为声明落账），poll 无硬上限（数组长度账），两面对照表
### UNX-F10629 · futex WAIT/WAKE 双操作
- 域/批：C4/B12｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10629-J1 WAIT 值不匹配即返 EAGAIN（1 万次），WAKE 唤醒数==实际等待数对账，双操作 100 轮零丢失
### UNX-F10630 · futex 键与基址判定
- 域/批：C4/B12｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10630-J1 uaddr→键映射（页基址判定 uaccess 联签）一致（同址同键 100/100），跨进程同物理页同键（共享面声明）10/10
### UNX-F10631 · FUTEX_PRIVATE_FLAG
- 域/批：C4/B12｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10631-J1 私有旗标下键空间分立（私/公同址不同键 100/100），私有面开销账（实测记录），误用跨进程面行为声明
### UNX-F10632 · FUTEX_WAIT_BITSET 预埋
- 域/批：C4/B12｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10632-J1 WAIT_BITSET/FUTEX_CLOCK_REALTIME 显性 ENOSYS 红账，WAIT/WAKE 路径零误伤（反向 100/100）
### UNX-F10633 · futex 与 A3 调度联签（唤醒路径）
- 域/批：C4/B12｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10633-J1 等待/唤醒经 A3 阻塞-唤醒接口（F10473 同族），状态字四拍账单调，唤醒延迟 ktest 粒度内 100/100
### UNX-F10634 · futex EAGAIN/EINTR/EFAULT 错误面
- 域/批：C4/B12｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10634-J1 三错误逐条可触发（EAGAIN 值变/EINTR 信号打断/EFAULT 坏指针）各 10 次，errno.rs 单点
### UNX-F10635 · futex requeue 预埋
- 域/批：C4/B12｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10635-J1 FUTEX_REQUEUE/CMP_REQUEUE 显性 ENOSYS 红账，WAIT/WAKE 零误伤（反向），条件变量实现影响声明
### UNX-F10636 · lost-wakeup 竞态防线
- 域/批：C4/B12｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10636-J1 先查后挂序单点（查值与挂队列原子序断言），竞态注入 10 万次零丢失，与 B09/B11/F10612 同序核验
### UNX-F10637 · poll 面与 pty/termios 交错
- 域/批：C4/B12｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10637-J1 pty fd poll 与模式切换/背压/终态交错 1000 轮行为一致（就绪位与状态对平），termios 变更即时反映
### UNX-F10638 · futex 数量上限与内存账
- 域/批：C4/B12｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10638-J1 键表上限触顶 EAGAIN（声明值），等待队列内存账与 B2 heap 对平，回收断言（清场归零）
### UNX-F10639 · 100 线程 futex 唤醒风暴压测
- 域/批：C4/B12｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10639-J1 100 线程 WAIT/WAKE 风暴（1 万轮）：零丢失零死锁（120 秒完成线），唤醒计数守恒，A3 联签链账全绿
### UNX-F10640 · B12 批收口与 C2 挂号联签
- 域/批：C4/B12｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10640-J1 select/pselect/poll/ppoll/futex 族挂号项（0x4000/0x4100 段）三对应核验，跨批竞态防线核验账（先查后挂序四方一致）
