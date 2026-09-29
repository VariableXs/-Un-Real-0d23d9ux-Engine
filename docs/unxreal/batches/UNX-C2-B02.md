# UNX-C2-B02 · open/close/lseek/stat 族地基（F8821–F8840 · 20 条）

> AI-12 承办｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01 5,200 + 本批 5,010 = 10,210 / 240,000｜嫁接源：Linux man-pages（open/stat/lseek 章节）只跟随｜防重：与 B01（表骨架/入口/write·read/mmap 三联）分层深化，文件族对现存 `syscall::calls::FdTable`/`FdEntry`/`FdKind` 为升级接管扩容，路径解析语义归 B1 域（挂点协议不越权）｜判据与 deepen/C2-B02.md 逐条同名同判据同 ID

### UNX-F8821 · open 语义档与 O_ 标志位全集
- 域/批：C2/B02｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8821-J1 O_RDONLY/O_WRONLY/O_RDWR/O_CREAT/O_EXCL/O_TRUNC/O_APPEND/O_NONBLOCK/O_CLOEXEC 九标志组合矩阵 30 组行为与 Linux 逐组一致，标志组合非法（O_RDONLY|O_WRONLY）拒收留痕
### UNX-F8822 · openat 家族与 AT_FDCWD 语义
- 域/批：C2/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8822-J1 openat 相对 dirfd 解析正确，AT_FDCWD 特值语义与绝对路径 fallback 各 100 次断言一致，坏 dirfd 返 EBADF
### UNX-F8823 · close 语义与 fd 回收时序
- 域/批：C2/B02｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8823-J1 close 后 fd 号立即可复用判据过，双 close 第二次返 EBADF，close 返回值不释放写缓存的时序语义有账
### UNX-F8824 · fd 最小可用分配原则与 fd 表扩容
- 域/批：C2/B02｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8824-J1 分配始终取最小可用号断言 10^4 次通过，表扩容阈值与上限（RLIMIT_NOFILE 联动）触发可观测，扩容中分配无空洞
### UNX-F8825 · lseek 语义档（SEEK_SET/CUR/END 与越界）
- 域/批：C2/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8825-J1 三 SEEK 基准行为与负偏移 EINVAL 判定逐档断言，超出文件尾 seek 合法且后续读返 0，管道类 lseek 返 ESPIPE
### UNX-F8826 · stat/fstat/lstat 三联与结构布局
- 域/批：C2/B02｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F8826-J1 三号对同一文件输出字段一致（符号链接 lstat 不跟随判据），结构布局与 Linux ABI 逐字段偏移对照表零差
### UNX-F8827 · statx 扩展属性面
- 域/批：C2/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8827-J1 statx mask 请求位与返回位掩码交互语义正确（请求位/支持位/空集三档），btime 类不支持字段显式置 STATX_NOSYNC 档留痕
### UNX-F8828 · creat 兼容挂点
- 域/批：C2/B02｜纯功能行数：150｜状态：[已深化]｜判据：UNX-F8828-J1 creat 等价 open(O_WRONLY|O_CREAT|O_TRUNC) 判据 100 次一致，已存在文件截断行为可观测
### UNX-F8829 · pread64/pwrite64 定位读写
- 域/批：C2/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8829-J1 定位读写不移动文件偏移判据过（与 read/write 交叉验证），负 offset 返 EINVAL，两号短读短写计数与主档一致
### UNX-F8830 · readv/writev 散布聚布 I/O
- 域/批：C2/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8830-J1 iovec 多缓冲聚合写入顺序保持，iovcnt 上限（IOV_MAX）越界返 EINVAL，部分向量完成后返回已完成字节数
### UNX-F8831 · O_APPEND 追加原子性语义
- 域/批：C2/B02｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8831-J1 多进程并发 O_APPEND 写不交错判据（10 进程 × 1,000 次）通过，lseek 后写仍追加到底的可观测行为落账
### UNX-F8832 · O_TRUNC/O_EXCL/O_CREAT 组合矩阵
- 域/批：C2/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8832-J1 八组合行为矩阵与 Linux 一致，O_EXCL 竞态双开（两进程同刻 open）仅一方成功 10/10 次
### UNX-F8833 · O_NONBLOCK 语义基准（fd 非阻塞位）
- 域/批：C2/B02｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8833-J1 open 时设置与 F_SETFL 后设置两路等价判据过，非阻塞读空返 EAGAIN 不阻塞 100 次，阻塞 fd 行为对照零差
### UNX-F8834 · 文件族错误矩阵（EBADF/EFAULT/EINVAL/EMFILE）
- 域/批：C2/B02｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F8834-J1 四错误码 × 8 号（open/close/lseek/stat/fstat/read/write/fcntl）注入矩阵 32 格逐格断言与 Linux 逐码一致
### UNX-F8835 · fd 表 fork 继承语义（C1 联测挂点）
- 域/批：C2/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8835-J1 fork 后子进程继承 fd 副本（共享偏移）判据过，O_CLOEXEC 位继承后 exec 关闭可观测，依赖 C1 未 finalize 已记风险
### UNX-F8836 · flock 建议锁挂点（B1 联签）
- 域/批：C2/B02｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8836-J1 LOCK_SH/LOCK_EX/LOCK_UN 三操作语义挂点注册成功，与 POSIX 记录锁（fcntl 锁）互不干扰判据有账，锁本体归 B1 防重声明
### UNX-F8837 · 路径解析挂点与 B1 VFS 对接协议
- 域/批：C2/B02｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F8837-J1 路径解析协议冻结（挂点接口 + EFAULT/EACCES/ENOENT 错误转译三段），B1 未收口期走现存最小路径栈判据全过，对接缺口入 open_risks
### UNX-F8838 · 文件族短读短写可见性判据
- 域/批：C2/B02｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8838-J1 短读（文件尾）返实际字节数、短写（管道满）返已写字节数判据各 100 次准确，零长读返 0 表 EOF 与 EAGAIN 区分明确
### UNX-F8839 · open/close 高频路径延迟账（O1 对接）
- 域/批：C2/B02｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8839-J1 open/close 各 10^5 次 P50/P99 落账，与 O1 syscall 延迟五指标对接字段一致，P99 ≤ Windows 同机对照 ×1.5 阈值登记待校准
### UNX-F8840 · ktest syscall 面 B02 批断言集
- 域/批：C2/B02｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8840-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，断言账与 B01 面隔离编号互不覆盖
