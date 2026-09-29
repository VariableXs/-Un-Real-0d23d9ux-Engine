# UNX-C2-B10 · 多路复用四件套之二：inotify/timerfd/eventfd/signalfd（F8981–F9000 · 20 条）

> AI-12 承办｜批次类型：M 型机制批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B09 40,800 + 本批 4,340 = 45,140 / 240,000｜嫁接源：Linux man-pages inotify(7)/timerfd_create(2)/eventfd(2)/signalfd(2)/utimensat(2) 章节、现存 `syscall::calls::EventPort` 存量档为升级接管扩容｜防重：inotify 事件源本体归 B1 VFS 写路径（联动不重铺）；信号投递本体归 B11 防重——signalfd 只立消费端号面；utimes 本体自 B06 F8918 挂点正式接管落本体档｜判据与 deepen/C2-B10.md 逐条同名同判据同 ID

### UNX-F8981 · inotify_init/inotify_init1 语义档
- 域/批：C2/B10｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8981-J1 实例 fd 分配判据过，IN_NONBLOCK/IN_CLOEXEC 旗标矩阵判据过，实例上限 EMFILE 格过
### UNX-F8982 · inotify_add_watch 掩码全集
- 域/批：C2/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8982-J1 掩码 16 位（IN_MODIFY/IN_CREATE/IN_DELETE/IN_ACCESS 等）逐位登记判据过，重复 add 更新掩码判据过，非法路径 ENOENT 格过
### UNX-F8983 · inotify_rm_watch 与事件队列
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8983-J1 rm 后队列残留事件含 IN_IGNORED 判据过，watch 描述符释放回收判据过，双重 rm EINVAL 格过
### UNX-F8984 · inotify 事件结构 ABI（struct inotify_event 冻结）
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8984-J1 事件结构布局（wd/mask/cookie/len/name）冻结判据过，name 变长对齐（4 字节）断言过，读侧整事件边界判据过
### UNX-F8985 · eventfd/eventfd2 计数语义
- 域/批：C2/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8985-J1 计数累加（上限 2^64-2 回 EAGAIN）判据过，读清零语义判据过，write 0 值返 EINVAL 格过
### UNX-F8986 · eventfd EFD_* 标志矩阵（NONBLOCK/CLOEXEC/SEMAPHORE）
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8986-J1 三旗标组合矩阵判据过，SEMAPHORE 读减一（非清零）判据过，NONBLOCK 满计数 EAGAIN 格过
### UNX-F8987 · timerfd_create/settime/gettime
- 域/批：C2/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8987-J1 新/旧 spec 设置与读回一致判据过，到期读 8 字节计数判据过，TFD_TIMER_ABSTIME/TFD_TIMER_CANCEL_ON_SET 两旗标判据过
### UNX-F8988 · utimes 本体（futimesat/utimensat 本体档）
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8988-J1 时间戳设置后 stat 读回一致判据过，UTIME_NOW/UTIME_OMIT 特殊值判据过，非特权改他人文件 EPERM 格过（自 B06 F8918 挂点接管）
### UNX-F8989 · signalfd 语义档
- 域/批：C2/B10｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8989-J1 signalfd 读出 signalfd_siginfo 布局冻结判据过，掩码内信号不再投递 handler 判据过，掩码变更重挂判据过
### UNX-F8990 · 多路复用四件套与 epoll 联动总档
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8990-J1 四件套逐一挂入 epoll 就绪回调判据过，联动行为与 F8967 协议一致回归，四路并发触发零互吞判据过
### UNX-F8991 · inotify 事件投递与 B1 VFS 写路径联动
- 域/批：C2/B10｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8991-J1 write→IN_MODIFY 投递链路判据过，create/delete 联动判据过，防重：投递点挂 B1 写路径（AI-14 联签点）不重铺
### UNX-F8992 · timerfd 到期与 A2 计时基座联动
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8992-J1 到期回调经 A2 时钟源仲裁消费协议（B06 F8911）触发判据过，时钟源切换时挂起 timerfd 不失准判据过
### UNX-F8993 · eventfd 与 W2 事件端口（EventPort）语义对齐
- 域/批：C2/B10｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8993-J1 eventfd 计数与 EventPort 端口语义对齐判据过，跨域消费协议（W2 端口挂点）落账，差异点显式登记不静默
### UNX-F8994 · signalfd 与信号族（B11）挂点协议
- 域/批：C2/B10｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8994-J1 signalfd 掩码与 rt_sigprocmask（F9002）联动判据过，防重：投递本体归 B11 不重铺，消费端协议冻结
### UNX-F8995 · 管道/FIFO 多路复用消费档
- 域/批：C2/B10｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8995-J1 管道读端数据就绪/写端腾空就绪接入多路复用判据过，FIFO 命名管道挂点判据过，对端关闭 POLLHUP 格过
### UNX-F8996 · 四件套错误矩阵（EMFILE/ENFILE/EINVAL/EBADF）
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8996-J1 四错误码 × 4 号（inotify_init/eventfd2/timerfd_create/signalfd）矩阵 16 格逐格一致
### UNX-F8997 · 异步 IO 挂点（io_setup/io_submit 登记档）
- 域/批：C2/B10｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8997-J1 aio 号面登记挂点立卡判据过，M 型先行声明落账（本体档待后续批次），未实现号返 -ENOSYS 判据过
### UNX-F8998 · 多路复用资源账本（watch 数/实例数上限落账）
- 域/批：C2/B10｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8998-J1 watch 数上限 128、实例数上限落账判据过，触达上限事件 meter 留痕不静默
### UNX-F8999 · 四件套 ktest 半自动驱动
- 域/批：C2/B10｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8999-J1 四件套联动驱动器判据过（inotify 触发→eventfd 通知→timerfd 定时→signalfd 收取单流程覆盖）
### UNX-F9000 · ktest syscall 面 B10 批断言集 + 万号整数点自检
- 域/批：C2/B10｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F9000-J1 本批 19 条判据聚合判据过，号面整数点 F9000 位置自检（区间连续性锚点）判据过
