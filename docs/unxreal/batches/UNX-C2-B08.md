# UNX-C2-B08 · 文件族扩容：dup/fcntl/ioctl/目录树（F8941–F8960 · 20 条）

> AI-12 承办｜批次类型：F 型地基收官批｜本批 [已深化] 收口：20 条全为本会话新深化｜域账累计：B01–B07 32,090 + 本批 4,540 = 36,630 / 240,000｜嫁接源：Linux man-pages dup(2)/fcntl(2)/ioctl(2)/getdents(2)/rename(2) 章节、现存 `syscall::calls::FdTable` 存量档为升级接管扩容｜防重：fd 分配本体（B02 F8823）与 open 语义（B02）不重铺，本批为描述符操控/指令基座/目录树扩容；ioctl 设备本体归 B4 域防重——本域只立 per-fd 指令登记协议｜判据与 deepen/C2-B08.md 逐条同名同判据同 ID

### UNX-F8941 · dup/dup2/dup3 语义档
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8941-J1 dup 返回最小空闲 fd 判据过，dup2 目标占用时原子替换判据过，dup3 NEWFD 标志（CLOEXEC）矩阵判据过
### UNX-F8942 · fcntl 五主指令（F_DUPFD/F_GETFD/F_SETFD/F_GETFL/F_SETFL）
- 域/批：C2/B08｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8942-J1 五指令逐指令断言过，F_GETFL 读回 O_ 标志与 open 时一致，F_SETFL 仅允许改运行态标志子集判据过
### UNX-F8943 · fcntl 记录锁指令（F_SETLK/F_SETLKW/F_GETLK）
- 域/批：C2/B08｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8943-J1 排他/共享锁互斥矩阵判据过，SETLKW 阻塞与信号打断 EINTR 判据过，GETLK 探测冲突锁返回归属 pid 判据过
### UNX-F8944 · fcntl 扩展指令（F_SETOWN/F_GETOWN/F_SETSIG/F_ADD_SEALS）
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8944-J1 扩展指令登记挂点注册判据过， seals 联动 memfd 挂点（B10 F8997 声明）一致性判据过，未支持指令返 EINVAL 不静默
### UNX-F8945 · ioctl 基座与指令编码（_IO/_IOR/_IOW/_IOWR）
- 域/批：C2/B08｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8945-J1 四类编码解包（方向/尺寸/类型/序号）断言过，编码-解码往返零差 10^3 次，坏编码返 ENOTTY 判据过
### UNX-F8946 · ioctl 终端指令集挂点（TCGETS/TCSETS/TIOCGWINSZ）
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8946-J1 终端三指令挂点注册判据过，非终端 fd 调用返 ENOTTY 判据过，termios 结构布局 ABI 对照零差
### UNX-F8947 · ioctl 设备指令登记协议（per-fd ioctl 表）
- 域/批：C2/B08｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F8947-J1 per-fd 指令表注册协议冻结判据过，未注册指令返 ENOTTY 且留痕，B4 域设备本体防重声明落账
### UNX-F8948 · EINTR/错误编码单点（跨批锚点）
- 域/批：C2/B08｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8948-J1 可中断慢调用统一返 EINTR 单点判据过（B06 F8904 引用锚点成立），编码路径与 F8923 单点同源，SA_RESTART 重启语义挂点与 B11 F9013 对接
### UNX-F8949 · getcwd 语义档
- 域/批：C2/B08｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8949-J1 缓冲不足返 ERANGE 判据过，size=0 自动分配语义判据过，路径与 B1 路径解析协议对账一致
### UNX-F8950 · chdir/fchdir 语义档
- 域/批：C2/B08｜纯功能行数：180｜状态：[已深化]｜判据：UNX-F8950-J1 切换后 getcwd 读回一致判据过，fchdir 对非目录 fd 返 ENOTDIR 判据过，权限不足 EACCES 矩阵格过
### UNX-F8951 · mkdir/rmdir 语义档
- 域/批：C2/B08｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8951-J1 建目录后 stat 类型为目录判据过，rmdir 非空目录返 ENOTEMPTY 判据过，mode 参数 umask 作用判据过
### UNX-F8952 · getdents64 目录读取
- 域/批：C2/B08｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8952-J1 dirent64 布局（d_ino/d_off/d_reclen/d_type）ABI 对照零差判据过，分批读取与偏移推进连续性判据过，目录尾返 0 判据过
### UNX-F8953 · readdir 遗留兼容挂点
- 域/批：C2/B08｜纯功能行数：160｜状态：[已深化]｜判据：UNX-F8953-J1 遗留号转接 getdents64 路径判据过，RouteNote::Permuted 差异登记，行为等价回归零差
### UNX-F8954 · 目录项缓存与 getdents 偏移 cookie
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8954-J1 cookie 单调性判据过，目录变更后 cookie 失效策略显式落账不静默，lseek 对目录定位 cookie 判据过
### UNX-F8955 · rename/renameat/renameat2
- 域/批：C2/B08｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F8955-J1 同目录/跨目录改名判据过，RENAME_EXCHANGE/RENAME_NOREPLACE 两旗标判据过，目标存在覆盖语义与 EEXIST 矩阵格过
### UNX-F8956 · unlink/unlinkat
- 域/批：C2/B08｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F8956-J1 打开文件 unlink 后 fd 仍可读写（引用计数语义）判据过，目录目标返 EISDIR 判据过，AT_REMOVEDIR 旗标判据过
### UNX-F8957 · link/linkat/symlink/readlink
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8957-J1 硬链接 nlink 计数判据过，符号链接读取零截断判据过，链接环 ELOOP 判据过，跨设备硬链接 EXDEV 判据过
### UNX-F8958 · 文件系统挂载点挂接协议（B1 联动）
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8958-J1 C2 号面与 B1 VFS 挂载点对接协议冻结判据过，跨挂载点操作 EXDEV 行为判据过，防重：挂载本体归 B1（AI-14）不重铺
### UNX-F8959 · 文件族错误矩阵（EBADF/ENOTDIR/EISDIR/ENOENT/ELOOP/ENAMETOOLONG）
- 域/批：C2/B08｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F8959-J1 六错误码 × 6 号（open/dup/getcwd/rename/unlink/link）矩阵 36 格逐格一致，路径深度边界（PATH_MAX）格过
### UNX-F8960 · ktest syscall 面 B08 批断言集
- 域/批：C2/B08｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F8960-J1 本批 19 条判据聚合入 ktest syscall 面，批跑一次命令全过，描述符操控与目录树断言独立编号可单独复跑
