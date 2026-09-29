# UNX-C4-B09 · unix socket 流式：bind/listen/accept（F10561–F10580 · 20 条）

> AI-14 承办｜域账累计：B01–B08 45,380 + 本批 5,620 = 51,000 / 240,000｜嫁接源：Linux unix(7)/socket(2)/bind(2)/listen(2)/accept(2)、System V AMD64 ABI（sockaddr 布局，注出处，禁凭记忆）｜防重：与现存 proc/ipc.rs IpcBus（定长端口消息总线）为不同抽象层——unix socket 是 fd 型 socket 地址族；与 B01 pipe 不同层（socket 面向连接/命名/多客户端），流式通路复用环形缓冲体而判据独立｜批注：本批立 AF_UNIX SOCK_STREAM 全生命周期；DGRAM/SEQPACKET/抽象命名空间在 B10；F10580 联签 C2 挂号（socket 族号段）。

### UNX-F10561 · AF_UNIX socket()/socketpair 基础
- 域/批：C4/B09｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10561-J1 socket(AF_UNIX,SOCK_STREAM,0) 得 fd 且域/型/协议账可查（100/100），socketpair 得一对互联 fd（往返 10/10），错误域 EAFNOSUPPORT/EPROTOTYPE 面
### UNX-F10562 · sockaddr_un 路径命名（108 字节）
- 域/批：C4/B09｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10562-J1 sun_path 108 字节边界（超长 ENAMETOOLONG 10/10），地址解析往返（bind 后 getsockname 逐字节一致 100/100）
### UNX-F10563 · bind 与文件系统 socket 节点（B1 联签）
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10563-J1 bind 后路径节点存在（socket 型 inode）且 stat 可查 100/100，已占用 EADDRINUSE、坏路径 EACCES 面 10/10
### UNX-F10564 · listen backlog 队列
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10564-J1 backlog=16 时 16 个挂起连接全入队，第 17 个按溢出策略处理（B10/F10576 联签），listen 两次覆盖语义 10/10
### UNX-F10565 · accept 四参（addr/addrlen）
- 域/批：C4/B09｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10565-J1 accept 返回新 fd（原监听 fd 不动）100/100，addr/addrlen 填充与客户端 bind 地址一致 100/100，坏 addrlen EFAULT 面
### UNX-F10566 · connect 状态机（阻塞/非阻塞）
- 域/批：C4/B09｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10566-J1 阻塞 connect 在 backlog 有位时立即成、无位时阻塞至有位；非阻塞满时 EAGAIN 且连接请求不丢失语义落账，100/100 次
### UNX-F10567 · 流式字节通路
- 域/批：C4/B09｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10567-J1 连接后双向字节流 1 MB 逐字节一致零错位，双端独立缓冲水位账（F10493 同款）对平
### UNX-F10568 · shutdown(SHUT_RD/WR/RDWR)
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10568-J1 三面行为：RD 后本端读 EOF/对端写 EPIPE 链、WR 后本端写 EPIPE/对端读 EOF、RDWR 双向终态，六格矩阵 10/10
### UNX-F10569 · EOF 语义（对端关闭 0 字节读）
- 域/批：C4/B09｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10569-J1 对端 close 后缓冲排空读得 EOF（0 字节），本端 write 得 EPIPE+SIGPIPE（B01 链复用），10/10 次
### UNX-F10570 · MSG_OOB 预埋（显式 ENOSYS）
- 域/批：C4/B09｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10570-J1 MSG_OOB 发送/接收路径显式 ENOSYS（红账落册），普通路径零误伤（不带旗标行为不变 100/100）
### UNX-F10571 · MSG_PEEK 窥视
- 域/批：C4/B09｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10571-J1 peek 后缓冲不变（再读得同内容 100/100），peek 与并发读竞态守恒（读总量==写入量）
### UNX-F10572 · MSG_DONTWAIT/NONBLOCK 面
- 域/批：C4/B09｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F10572-J1 旗标与 fd 态两源合一（DONTWAIT 压倒 fd 态），读空/写满 EAGAIN 10/10，与 B12 poll 面协同声明
### UNX-F10573 · send/recv 与 write/read 等价面
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10573-J1 无旗标 send==write、recv==read 行为逐字节一致（1 MB 对照 100/100），旗标面差异表落账
### UNX-F10574 · socketpair 双工通路
- 域/批：C4/B09｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10574-J1 socketpair 双向 1 MB 回环零错位，双端 fd 权限面（读写俱备），关闭单端另一端终态链正确（10/10）
### UNX-F10575 · unix socket 与 epoll 就绪面（B11 前置）
- 域/批：C4/B09｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10575-J1 监听/连接 fd 就绪位（POLLIN=挂起连接/可读、POLLOUT=可写）与状态一致 100 次，统一 poll 接口挂接（B11 消费）签名在账
### UNX-F10576 · 并发连接与 backlog 溢出
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10576-J1 100 并发 connect 在 backlog 内全成，溢出路径按策略返 ECONNREFUSED（unix(7) 语义注出处）10/10
### UNX-F10577 · 节点清理（close 不 unlink）声明
- 域/批：C4/B09｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10577-J1 close 全部 socket fd 后路径节点仍在（Linux 语义声明落账），复 bind 同路径 EADDRINUSE，节点删除走显式 unlink（B1）
### UNX-F10578 · EADDRINUSE/EISCONN/ENOTCONN 错误面
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10578-J1 三错误逐条可触发（重复 bind/重复 connect/未连接 recv-send），各 10 次，错误码 errno.rs 单点
### UNX-F10579 · 多客户端多路复用压测
- 域/批：C4/B09｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10579-J1 1 服务器 + 32 客户端并发收发 1 MB/端：零错位零串扰（每连接独立校验和），退出-重连循环 100 轮零泄漏
### UNX-F10580 · B09 批收口与 C2 挂号联签
- 域/批：C4/B09｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10580-J1 socket/socketpair/bind/listen/accept/connect/send/recv/shutdown 族挂号登记项（table.rs 0x4000/0x4100 段）逐条核验过，资源回收断言（连接账归零）过
