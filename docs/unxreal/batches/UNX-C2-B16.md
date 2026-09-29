# UNX-C2-B16 · fcntl 与 ioctl 挂点族（F9101-F9120 · 20 条）

> AI-12 承办｜批次类型：M 型（任务书 B09-B20 M 型收尾轴 · 文件族扩容 fcntl/ioctl 命令面）｜本批 [骨架] 20 条全为本会话新铺设｜域账累计：B01-B15 66,870 + 本批 6,500 = 73,370 / 240,000｜嫁接源：fcntl(2)/ioctl(2) man-pages 语义对齐（来源版本随深化册落账）；B1 VFS 冻结读路径（fd 表消费）；与 B08 dup 族/F8921 参数拷贝探针/F8966 管道容量账单源联动｜防重声明：fcntl/ioctl 族 20 号为号面新铺（B01-B15 未触及，dup 族 B08 零重叠）；本批为冻结协议 F9093 之后的扩号首段：扩号段 F9101-F9120 承接冻结面 F8801-F9100，零重编零私设号（只做 Linux 上游约定号面）｜红线注记：本批零写盘零引导零固件操作；ioctl 只走挂点协议登记，硬件端口类命令（ioperm/iopl）禁用格归 B24 EPERM 矩阵，本批零触及（双轨产线条款默认适用）｜批注（AI-12）：B16 是 M 型收尾第一轴：fcntl 四命令是 glibc FILE* 关闭语义与 shell 重定向的底层依赖，记录锁三命令按挂点协议落账（实装归 B1 联签），ioctl 以方向位解码+挂点路由为主轴——终端族三命令为 C4 pty 联签预留对装面｜判据与后续 deepen/C2-B16.md 逐条同名同判据同 ID

### UNX-F9101 · fcntl 语义档总纲（F_GETFD/F_SETFD/F_GETFL/F_SETFL 四命令）
- 域/批：C2/B16｜纯功能行数：400｜状态：｜判据：UNX-F9101-J1 四命令读写回环判据过，FD_CLOEXEC 位 execve 后生效可观测，坏 fd 注入全 EBADF 逐格核对
### UNX-F9102 · fcntl F_DUPFD/F_DUPFD_CLOEXEC 复制命令
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9102-J1 最小可用 fd ≥ arg 分配判据过，CLOEXEC 变体位翻转可观测，与 dup 族分配器单源对账
### UNX-F9103 · fcntl 记录锁挂点（F_GETLK/F_SETLK/F_SETLKW 三命令协议）
- 域/批：C2/B16｜纯功能行数：340｜状态：｜判据：UNX-F9103-J1 三命令协议落账判据过，l_type/l_whence/l_start/l_len 四字段探针格过，实装归 B1 联签声明格过
### UNX-F9104 · fcntl F_SETFL O_NONBLOCK/O_APPEND 位语义
- 域/批：C2/B16｜纯功能行数：340｜状态：｜判据：UNX-F9104-J1 O_NONBLOCK 置位后非阻塞路径可观测判据过，O_APPEND 追加位生效判据过，F_GETFL 回读逐位一致
### UNX-F9105 · fcntl 错误矩阵（EBADF/EINVAL/EINTR/EDEADLK/EMFILE）
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9105-J1 五错误码注入矩阵判据过，逐码与 Linux 语义一致，非法命令全 EINVAL 格过
### UNX-F9106 · ioctl 语义档总纲（第三参数装箱协议与方向位）
- 域/批：C2/B16｜纯功能行数：380｜状态：｜判据：UNX-F9106-J1 IO/IOR/IOW/IOWR 四方向位解码判据过，参数装箱协议落账，未知命令路由格过
### UNX-F9107 · ioctl 终端族挂点（TCGETS/TCSETS/TIOCGWINSZ）
- 域/批：C2/B16｜纯功能行数：340｜状态：｜判据：UNX-F9107-J1 三命令挂点协议落账判据过，winsize 结构回读格过，pty 缺席全 ENOTTY 与 C4 联签格过
### UNX-F9108 · ioctl FIONBIO/FIOASYNC 挂点协议（网络族号段预留）
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9108-J1 两命令挂点落账判据过，socket 号段预留声明与 I2 域边界一致，非网络 fd 全 ENOTTY 格过
### UNX-F9109 · ioctl 未知命令返回 ENOTTY 矩阵
- 域/批：C2/B16｜纯功能行数：260｜状态：｜判据：UNX-F9109-J1 未知命令注入矩阵判据过，全返回 -ENOTTY 与 Linux 一致，零 -EINVAL 混淆格过
### UNX-F9110 · fcntl F_GETOWN/F_SETOWN 信号投递属主挂点
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9110-J1 属主设置读取回环判据过，SIGIO 路由挂点协议落账，F_SETSIG 挂点声明格过
### UNX-F9111 · fcntl F_SETLKW 阻塞等待与 EINTR 重启联动
- 域/批：C2/B16｜纯功能行数：340｜状态：｜判据：UNX-F9111-J1 阻塞等待挂点与 EINTR 重启协议联动判据过，与 B12 四输入判定器对账一致，零阻塞对照格过
### UNX-F9112 · flock 挂点与 POSIX 锁差异账
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9112-J1 flock(2) 挂点落账判据过，LOCK_EX/LOCK_SH/LOCK_UN 三操作协议格过，两锁体系语义差异账四条全列
### UNX-F9113 · dup3 与 O_CLOEXEC 联动（dup 族闭环）
- 域/批：C2/B16｜纯功能行数：300｜状态：｜判据：UNX-F9113-J1 dup3 到期语义判据过，newfd==oldfd 返回 EINVAL 格过，与 dup 族分配器单源复测一致
### UNX-F9114 · fcntl F_GETPIPE_SZ/F_SETPIPE_SZ 管道容量命令
- 域/批：C2/B16｜纯功能行数：300｜状态：｜判据：UNX-F9114-J1 容量读写回环判据过，下限页对齐校验格过，与管道容量账对账一致
### UNX-F9115 · fcntl F_ADD_SEALS/F_GET_SEALS 密封挂点协议
- 域/批：C2/B16｜纯功能行数：300｜状态：｜判据：UNX-F9115-J1 密封四值挂点落账判据过，memfd 缺席全 EINVAL 格过，号面预留声明格过
### UNX-F9116 · ioctl 结构拷贝探针（copy_from_user/copy_to_user 联动）
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9116-J1 双向拷贝探针判据过，坏用户指针全 EFAULT 格过，与参数拷贝探针总成单源
### UNX-F9117 · fcntl/ioctl 参数探针总成（nargs=3 命令面）
- 域/批：C2/B16｜纯功能行数：300｜状态：｜判据：UNX-F9117-J1 nargs=3 寄存器提取判据过，三参逐位传递与 ABI 约定一致，r10 槽位复用格过
### UNX-F9118 · fcntl 与 glibc FILE* 关闭路径消费核
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9118-J1 glibc fclose 到 close 消费链判据过，FD_CLOEXEC 在 popen 路径生效可观测，消费号登记格过
### UNX-F9119 · fcntl/ioctl 升级接管注记（int 0x80 存量面）
- 域/批：C2/B16｜纯功能行数：320｜状态：｜判据：UNX-F9119-J1 接管边界声明判据过，存量 ioctl 零新增语义承诺格过，行为保持清单落账
### UNX-F9120 · ktest syscall 面 B16 批断言集
- 域/批：C2/B16｜纯功能行数：340｜状态：｜判据：UNX-F9120-J1 本批 19 条判据聚合判据过，fcntl 消费链与 ioctl 挂点路由一次跑通，域累计 73,370 收口断言
