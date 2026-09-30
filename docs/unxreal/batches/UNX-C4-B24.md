# UNX-C4-B24 · E 型·unix socket 错误矩阵一（F10861–F10880 · 20 条）

> AI-14 承办｜域账累计：B01–B15 138,280 + 本批 6,900 = 145,180 / 240,000｜嫁接源：Linux unix(7)/recvmsg(2)/cmsg(3)（注出处，禁凭记忆）｜防重：与 B16–B18 防重：M 型立机制建立面（连接/地址/选项），本批立 unix socket 错误矩阵面，判据零交集；与 B21 防重：B21 立管道 EPIPE，本批 F10863 为 stream socket 侧 EPIPE（同码异域分账）｜批注：本批 B24：E 型错误矩阵段 4/8——unix socket 错误矩阵一（REFUSED/RESET/EPIPE/ENOTCONN/EISCONN/SCMR/零长/判定总序），正反双判据制

### UNX-F10861 · ECONNREFUSED 矩阵（connect 无监听）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10861-J1 正面：connect 无监听 socket→ECONNREFUSED（10/10）；反面：有监听零误报（千次 10/10）
### UNX-F10862 · ECONNRESET 矩阵（对端异常关闭）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10862-J1 正面：对端 close 后本端读 ECONNRESET（10/10）；反面：正常关闭读 EOF 零混报（千次 10/10）
### UNX-F10863 · EPIPE 矩阵（stream 对端关后写）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10863-J1 正面：对端关后 write→EPIPE（10/10）；反面：对端在场零误报（千次 10/10）
### UNX-F10864 · ENOTCONN 矩阵（未连接即收发）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10864-J1 正面：未连接 stream 收发→ENOTCONN（各 10/10）；反面：已连接零误报（千次 10/10）
### UNX-F10865 · EISCONN 矩阵（已连接 dgram 再 connect）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10865-J1 正面：已 connect dgram 再 connect 他址→EISCONN（10/10）；反面：connect 同址幂等零误报（千次 10/10）
### UNX-F10866 · EOPNOTSUPP 矩阵（listen/accept 误用）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10866-J1 正面：dgram 上 listen→EOPNOTSUPP、非监听上 accept→EINVAL（各 10/10）；反面：正确用法零误报（千次 10/10）
### UNX-F10867 · EPROTOTYPE 矩阵（类型不匹配 connect）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10867-J1 正面：stream socket connect 到 seqpacket 监听端→EPROTOTYPE（10/10）；反面：同型连接零误报（千次 10/10）
### UNX-F10868 · EALREADY 矩阵（进行中再 connect）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10868-J1 正面：非阻塞 connect 进行中再 connect→EALREADY（10/10）；反面：完成后再 connect 幂等（千次 10/10）
### UNX-F10869 · EMSGSIZE 矩阵（dgram 超限）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10869-J1 正面：dgram 报文超 SO_SNDBUF 上限→EMSGSIZE（10/10）；反面：限内零误报（千次 10/10）
### UNX-F10870 · EAGAIN dgram 满矩阵
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10870-J1 正面：非阻塞 dgram 满→EAGAIN（10/10）；反面：有空间零误报（千次 10/10）
### UNX-F10871 · EINTR 收发中断矩阵
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10871-J1 正面：阻塞收发被信号中断→EINTR（各 10/10）；反面：无信号零中断（千次 10/10）
### UNX-F10872 · msg 结构错误矩阵（msg_iov/msg_control 面EINVAL/EFAULT）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10872-J1 正面：坏 msg 指针→EFAULT、非法 iovcnt→EINVAL/EMSG（各 10/10）；反面：合法 msg 零误报（千次 10/10）
### UNX-F10873 · SCM_RIGHTS fd 传递错误矩阵（收方表满/非法 fd）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10873-J1 正面：收方 fd 表满时延迟报错路径显式（10/10）、传递已关闭 fd→EBADF（10/10）；反面：正常传递零误报（千次 10/10）
### UNX-F10874 · SCM_CREDENTIALS 伪造拒收矩阵
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10874-J1 正面：用户态注入凭据 cmsg→拒收 EINVAL（10/10）；反面：内核填充凭据正常通行（千次 10/10）
### UNX-F10875 · dgram 零长报文矩阵
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10875-J1 正面：send 0 长报文合法入队、recv 读出零长（各 10/10）；反面：与 EOF 零混报（千次 10/10）
### UNX-F10876 · 抽象命名空间错误矩阵（非法字符/占用）
- 域/批：C4/B24｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10876-J1 正面：抽象名含非法字符→EINVAL、占用→EADDRINUSE（各 10/10）；反面：合法名零误报（千次 10/10）
### UNX-F10877 · accept 错误矩阵（EINVAL/EMFILE）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10877-J1 正面：非监听 accept→EINVAL、fd 表满→EMFILE（各 10/10）；反面：正常 accept 零误报（千次 10/10）
### UNX-F10878 · shutdown 错误矩阵（how 值/ENOTCONN）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10878-J1 正面：非法 how→EINVAL、未连接 shutdown→ENOTCONN（各 10/10）；反面：合法三向零误报（千次 10/10）
### UNX-F10879 · unix socket 错误判定总序表（冻结件）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10879-J1 错误判定序表全枚举（10/10）、序间衔接零冲突（10/10）；反面：序表冻结后零漂移（千次 10/10）
### UNX-F10880 · unix socket 错误矩阵一段总收口（B24 断言聚合）
- 域/批：C4/B24｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10880-J1 F10861–F10879 十九条判据聚合回归（350 项全绿），段累计对平（138,280+6,900=145,180），域总账移交清单更新
