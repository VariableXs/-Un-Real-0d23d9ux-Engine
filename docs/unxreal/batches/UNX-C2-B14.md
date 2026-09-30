# UNX-C2-B14 · glibc 消费清单逐号核（F9061–F9080 · 20 条）

> AI-12 承办｜批次类型：I 型集成映射批（任务书 B29–B36 I 型体例在本认领段的机制化落点）｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B13 57,990 + 本批 4,320 = 62,310 / 240,000｜嫁接源：glibc 2.31/2.35 syscalls.list 与源码包装层体例（只跟随体例不跟随数据）、busybox 静态链接探针体例｜防重：glibc 移植本体归 B6 用户态域防重——本域只立"每个号被谁消费"的证据账与逐号核协议，不重铺包装器实现｜判据与 deepen/C2-B14.md 逐条同名同判据同 ID

### UNX-F9061 · glibc 系统调用消费模型总纲（syscall() 包装器路径）
- 域/批：C2/B14｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9061-J1 包装器路径（libc 包装→syscall 指令→入口门→分发表）全链冻结判据过，r10 上下文槽位穿透声明与 B01 F8802 一致判据过
### UNX-F9062 · glibc open/read/write/close 消费核（B02 号面）
- 域/批：C2/B14｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F9062-J1 四号消费证据（stdio/stdin 缓冲路径）登记判据过，静态 hello-world 探针全程只触四号断言过
### UNX-F9063 · glibc malloc/brk/mmap 消费核（B03 号面）
- 域/批：C2/B14｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F9063-J1 malloc 大小块两路消费证据（brk 顶探/mmap 阈值 128KiB）登记判据过，free 触发 madvise（MADV_DONTNEED）路径证据过
### UNX-F9064 · glibc fork/exec/wait 消费核（B04 号面）
- 域/批：C2/B14｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9064-J1 system()/popen() 消费链证据（fork+execve+waitpid）登记判据过，abort() 走 raise(SIGABRT) 证据过
### UNX-F9065 · glibc 身份族消费核（B05 号面 getuid/setuid）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9065-J1 getuid/geteuid 包装消费证据登记判据过，setuid 降格路径证据过，缓存惰性（glibc uid 缓存）行为登记
### UNX-F9066 · glibc 时间族消费核（B06 号面 clock_gettime/gmtime 缓存）
- 域/批：C2/B14｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9066-J1 clock_gettime 直入 syscall（无包装缓存）证据判据过，vDSO 快路径（B06 F8912）优先协商证据过，localtime 时区读文件链证据登记
### UNX-F9067 · glibc errno 转换核（__libc_errno 与 -errno 编码）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9067-J1 -errno→TLS errno 转换证据登记判据过，perror/strerror 可读名与 F8933 档一致判据过
### UNX-F9068 · glibc dup/fcntl 消费核（B08 号面）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9068-J1 dup2 用于 stdio 重定向证据判据过，fcntl(F_SETFL) 用于 NONBLOCK 证据过，F_GETFD 继承位证据过
### UNX-F9069 · glibc opendir/readdir 消费核（getdents64 包装）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9069-J1 DIR 缓冲经 getdents64 聚合证据判据过，readdir 逐项解析与 d_type 分发证据过
### UNX-F9070 · glibc epoll 包装消费核（epoll_wait→poll 桥）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9070-J1 epoll 三包装消费证据登记判据过，老版本 poll 桥路径证据过，event 数据透传零差判据过
### UNX-F9071 · glibc pthread 消费核（clone/futex 挂点）
- 域/批：C2/B14｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F9071-J1 pthread_create 消费链（clone CLONE_VM|CLONE_THREAD|CLONE_SIGHAND）证据判据过，futex 挂点登记与 B04 F8865 声明对账一致
### UNX-F9072 · futex 语义档（glibc pthread 锁消费本体）
- 域/批：C2/B14｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F9072-J1 FUTEX_WAIT/WAKE 双操作正判据过，私/共享 futex 分档判据过，超时路径 EAGAIN/TIMEDOUT 格过
### UNX-F9073 · glibc 信号消费核（sigaction/raise 包装）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9073-J1 signal() 兼容包装落 sigaction 证据判据过，pthread_kill 定向投递证据过，abort 三信号（SIGABRT）链证据过
### UNX-F9074 · glibc dlopen 消费钩子（ELF/mmap 联动）
- 域/批：C2/B14｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9074-J1 dlopen 消费链（open+mmap+close）证据判据过，ELF 装载本体归 B5 域防重声明落账，探针（dlopen 后关闭源 fd 仍可用）判据过
### UNX-F9075 · glibc NSS/getpwuid 消费钩子（文件族联动）
- 域/批：C2/B14｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F9075-J1 getpwuid 消费链（open /etc/passwd+read+close）证据判据过，文件不存在 ENOENT 降级路径判据过
### UNX-F9076 · glibc printf/stdio 消费核（write 缓冲行为）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9076-J1 行缓冲/全缓冲切换证据（tty 判定经 fstat）判据过，fflush 触发 write 聚合证据过，管道另一端读数一致判据过
### UNX-F9077 · 消费清单逐号核账本（每号"被消费证据"登记格式）
- 域/批：C2/B14｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F9077-J1 登记格式（号/包装函数/证据用例/探针脚本）冻结判据过，B02–B14 已核号面逐行登记数与抽样数一致校验过
### UNX-F9078 · 消费核回归套件（静态链接 busybox 探针登记）
- 域/批：C2/B14｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F9078-J1 busybox 静态探针（sh/ls/echo 三 applet）消费清单落账判据过，探针触号清单与逐号核账本交叉核验一致
### UNX-F9079 · glibc 版本锚定账本（2.31/2.35 行为差异登记）
- 域/批：C2/B14｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F9079-J1 两版本差异点（clock_gettime64/pthread 布局）登记判据过，差异不影响号面契约声明落账
### UNX-F9080 · ktest syscall 面 B14 批断言集
- 域/批：C2/B14｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F9080-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，消费核断言独立编号可单独复跑
