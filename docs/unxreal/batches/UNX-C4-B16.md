# UNX-C4-B16 · unix socket 连接建立深水（F10701–F10720 · 20 条）

> AI-14 承办｜域账累计：B01–B15 85,680 + 本批 6,500 = 92,180 / 240,000｜嫁接源：Linux socket(2)/connect(2)/listen(2)/accept(2)/accept4(2)/shutdown(2)/unix(7)（注出处，禁凭记忆）｜防重：与 B09 流式册防重：B09 立 bind/listen/accept 主链正向面，本批立连接建立深水（backlog 窗口/非阻塞 connect/shutdown/close 语义），判据面不重叠；与 B10 DGRAM/SEQPACKET 防重：本批仅 stream 型连接面（seqpacket 三态 connect 对照注账不复判）｜批注：本批 B16 开批：B09–B20 M 型机制收尾段第 1/5 批——连接建立深水（backlog 窗口/非阻塞 connect/shutdown 三态/close 终止/竞争 accept）；B17–B20 随后

### UNX-F10701 · listen backlog 队列深度语义
- 域/批：C4/B16｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10701-J1 backlog 上限生效（somaxconn 截断 3 档 ×100/100），队列深度可观测（挂起数对账 1 千次），排满行为注账（选定拒绝面）
### UNX-F10702 · 非阻塞 connect 与 EINPROGRESS
- 域/批：C4/B16｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10702-J1 O_NONBLOCK 下 connect 立即返 EINPROGRESS（1 千次），后台连接状态机三态迁移账（进行中/已立/已拒）对平
### UNX-F10703 · connect 完成检测（SO_ERROR/POLLOUT）
- 域/批：C4/B16｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10703-J1 SO_ERROR 取零=已立/取错=拒绝（双路各 500 次），取出后清零（二次读 10/10），POLLOUT 就绪与完成同步（100/100）
### UNX-F10704 · 并发 connect 同一地址竞态
- 域/批：C4/B16｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10704-J1 N 进程并发 connect 同一监听端（N=32）全部按序入队零丢失（1 千轮），fd 互不串扰（对账 grep）
### UNX-F10705 · connect 自连（同一 socket 连自身地址）语义
- 域/批：C4/B16｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10705-J1 自连行为注账（Varix 选定允许并走完整握手面 100/100），回环数据通路验证（收发对平 1 千次）
### UNX-F10706 · accept 队列溢出行为（重传窗口）
- 域/批：C4/B16｜纯功能行数：330｜状态：[骨架]｜判据：UNX-F10706-J1 溢出期间 connect 拒绝计数对账（100/100），accept 排空后恢复接新（100/100），溢出事件账可查
### UNX-F10707 · accept4 SOCK_CLOEXEC/SOCK_NONBLOCK
- 域/批：C4/B16｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10707-J1 accept4 双标志位生效（CLOEXEC 经 fork+exec 验证 10/10、NONBLOCK 即时 100/100），与 fcntl 后置等价性对账
### UNX-F10708 · listen 未 bind 的 EINVAL 路径
- 域/批：C4/B16｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10708-J1 未 bind 直接 listen→EINVAL（10/10，流式），DGRAM 型 listen 语义注账（允许/对照差异）
### UNX-F10709 · 重复 listen 返回值
- 域/批：C4/B16｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F10709-J1 已 listen fd 二次 listen 成功且可更新 backlog（100/100），队列内容保持（对平）
### UNX-F10710 · accept 对端地址长度截断
- 域/批：C4/B16｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10710-J1 addrlen 足额返回完整地址（100/100），addrlen 过小截断且按传入值回写（10/10），匿名对端零长度注账
### UNX-F10711 · shutdown SHUT_WR 半关闭
- 域/批：C4/B16｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10711-J1 SHUT_WR 后写端 EPIPE 面（100/100）、读端收 EOF、对端读净残余后 EOF（对平 1 千次），写端读不受影响（100/100）
### UNX-F10712 · shutdown SHUT_RD 与 EOF 传播
- 域/批：C4/B16｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10712-J1 SHUT_RD 后本端读立即 EOF（100/100），对端继续写不报错（接收侧丢弃注账 100/100），恢复不可行注账
### UNX-F10713 · shutdown 三态组合矩阵
- 域/批：C4/B16｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10713-J1 SHUT_RD/WR/RDWR 三态×读写行为矩阵全对（9 格 ×100/100），二次 shutdown 幂等（10/10）
### UNX-F10714 · 对端未 accept 的数据缓冲窗口
- 域/批：C4/B16｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F10714-J1 已立连接未 accept 期间对端可写（数据入连接缓冲 1 千条对平），accept 后全量可读（序账保持），窗口上限注账
### UNX-F10715 · close 连接终止与 SO_LINGER 占位
- 域/批：C4/B16｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10715-J1 close 即终止：未读数据丢弃、对端读 EOF（100/100），SO_LINGER 显性 ENOPROTOOPT 注账（红账预埋族）
### UNX-F10716 · dup 后监听 fd 的 accept 语义
- 域/批：C4/B16｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10716-J1 dup 出的 fd 同样可 accept（100/100），新连接 fd 独立于监听 fd 副本（隔离账 1 千次），close 一份监听不失效
### UNX-F10717 · fork 后监听 socket 共享与竞争 accept
- 域/批：C4/B16｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10717-J1 N 子进程竞争 accept（N=8）每连接恰派发一个（1 千轮零重复零丢失），惊群行为注账（选定单唤醒）
### UNX-F10718 · connect 到正在关闭的监听端竞态
- 域/批：C4/B16｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F10718-J1 close 监听期间 connect 注入（×100 竞态窗口）：接住=正常建立、未接=拒绝，两侧对账零悬挂（EINPROGRESS 清扫）
### UNX-F10719 · backlog=0 边界（单挂起连接）
- 域/批：C4/B16｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10719-J1 backlog=0 允许恰一个挂起连接（100/100），第二个 connect 排满面拒绝（100/100），accept 后立即恢复
### UNX-F10720 · 连接建立段总收口（B16 断言聚合）
- 域/批：C4/B16｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F10720-J1 F10701–F10719 十九条判据全量回归（断言聚合 380 项全绿），段内域累计账对平（85,680+6,500=92,180）
