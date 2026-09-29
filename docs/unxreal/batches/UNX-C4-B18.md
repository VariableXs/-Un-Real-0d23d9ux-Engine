# UNX-C4-B18 · SOL_SOCKET 选项族全语义（F10741–F10760 · 20 条）

> AI-14 承办｜域账累计：B01–B15 98,580 + 本批 6,300 = 104,880 / 240,000｜嫁接源：Linux socket(7)/unix(7)/getsockopt(2)/setsockopt(2)（注出处，禁凭记忆）｜防重：与 B16 防重：B16 立连接建立深水（SO_ERROR 完成面 F10705、close 终止+SO_LINGER 行为面 F10717），本批立选项读写面（SO_ERROR/SO_LINGER 选项面与 F10705/F10717 行为面分工注账）；与 B17 防重：B17 立地址族全语义，本批立 SOL_SOCKET 选项族，判据面零交集｜批注：本批 B18：M 型机制收尾段第 3/5 批——SOL_SOCKET 选项族（读写链/缓冲账/超时/凭据/继承/错误矩阵/并发竞态）；B19–B20 随后

### UNX-F10741 · getsockopt/setsockopt 基本读写链（SO_TYPE）
- 域/批：C4/B18｜纯功能行数：330｜状态：[骨架]｜判据：UNX-F10741-J1 setsockopt/getsockopt 读写链 100/100，SO_TYPE 回读 SOCK_STREAM/SOCK_DGRAM 各 10/10，回读字面与 socket(2) 创建参数零偏离（对账表）
### UNX-F10742 · SO_ACCEPTCONN 监听态判定
- 域/批：C4/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10742-J1 listen 后 SO_ACCEPTCONN 回读 1（10/10），listen 前 0（10/10），dgram socket 恒 0（10/10）
### UNX-F10743 · SO_SNDBUF/SO_RCVBUF 读写与双界
- 域/批：C4/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10743-J1 两选项 set→get 回读一致（各 100/100），上限封顶 10/10，下限收敛 10/10，双界对账零偏离
### UNX-F10744 · SO_SNDBUF 双倍记账与下限收敛值
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10744-J1 set v→get 返回记账值（双倍规则 10/10），下限收敛 2048→记账 4096（10/10），规则条文誊录核对 10/10
### UNX-F10745 · SO_RCVLOWAT 生效与 SO_SNDLOWAT ENOPROTOOPT
- 域/批：C4/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10745-J1 SO_RCVLOWAT 设阈后 poll 就绪判定联动（10/10），SO_SNDLOWAT set→ENOPROTOOPT（10/10），差异注账在册
### UNX-F10746 · SO_RCVTIMEO/SO_SNDTIMEO 超时选项
- 域/批：C4/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10746-J1 设超时后阻塞 recv/send 到点 EAGAIN（各 10/10），零值还原无限阻塞（10/10），超时精度对账 10/10
### UNX-F10747 · SO_ERROR 选项面通用清零语义
- 域/批：C4/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10747-J1 注错后 get SO_ERROR 取错并清零（10/10），二读返回 0（10/10），与 F10705 完成面分工对账 10/10
### UNX-F10748 · SO_KEEPALIVE 域 socket 可设无效果
- 域/批：C4/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10748-J1 AF_UNIX 上 set→get SO_KEEPALIVE 回读一致（10/10），设后收发行为零差异（千次对账），注账在册
### UNX-F10749 · MSG_OOB EOPNOTSUPP 与 SO_OOBINLINE 无效果
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10749-J1 AF_UNIX send MSG_OOB→EOPNOTSUPP（10/10），SO_OOBINLINE 可设回读一致（10/10），带外面零效果对账 10/10
### UNX-F10750 · SO_PASSCRED 每报文凭据附着
- 域/批：C4/B18｜纯功能行数：330｜状态：[骨架]｜判据：UNX-F10750-J1 设 SO_PASSCRED 后每条报文附 SCM_CREDENTIALS cmsg（100/100），未设零附着（100/100），凭据与发送方一致 10/10
### UNX-F10751 · SO_PEERCRED 对端凭据读取
- 域/批：C4/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F10751-J1 连接后 get SO_PEERCRED 三字段与对端一致（10/10），listen 端读到 accept 对端凭据（10/10），快照时点注账 10/10
### UNX-F10752 · SO_SNDBUFFORCE 特权守卫面
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10752-J1 无特权 set FORCE→EPERM（10/10），特权路径经 guard.rs 放行（10/10），守卫判定序对账 10/10
### UNX-F10753 · 选项层级判定序（SOL_SOCKET/协议层）
- 域/批：C4/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10753-J1 SOL_SOCKET 层命中 10/10，未知层 ENOPROTOOPT（10/10），未知选项同层 ENOPROTOOPT（10/10），判定序对账 10/10
### UNX-F10754 · accept 继承面（选项遗传/文件标志不遗传）
- 域/批：C4/B18｜纯功能行数：330｜状态：[骨架]｜判据：UNX-F10754-J1 listener 选项被 accept 新 fd 继承（10/10），O_NONBLOCK 不继承（10/10），继承清单对账 10/10
### UNX-F10755 · getsockopt optlen 截断与 EINVAL
- 域/批：C4/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10755-J1 optlen 过小→EINVAL（10/10），optlen 过大按实际回写（10/10），出参回写对账 10/10
### UNX-F10756 · setsockopt 错误矩阵（EBADF/EINVAL/EFAULT）
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10756-J1 EBADF/EINVAL/EFAULT 三错误各 10/10（判定序对账），EFAULT 走 B2 copy 面联签（10/10），矩阵全绿
### UNX-F10757 · SO_DONTROUTE/SO_BROADCAST 域 socket 不适用注账
- 域/批：C4/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F10757-J1 两选项 AF_UNIX 可设回读一致（10/10），设后行为零差异（千次对账），不适用注账在册
### UNX-F10758 · SO_LINGER 选项读写面（与 F10717 行为面分工）
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10758-J1 linger 结构 set→get 回读逐字段一致（10/10），l_onoff 非 0/1 外值 EINVAL（10/10），与 F10717 分工对账 10/10
### UNX-F10759 · 选项并发设置竞态（锁序单点）
- 域/批：C4/B18｜纯功能行数：310｜状态：[骨架]｜判据：UNX-F10759-J1 并发 set/get 千次无撕裂（回读全为合法值 100/100），set 与收发并发零错账（10/10），锁序对账 10/10
### UNX-F10760 · 选项族段总收口（B18 断言聚合）
- 域/批：C4/B18｜纯功能行数：350｜状态：[骨架]｜判据：UNX-F10760-J1 F10741–F10759 十九条判据聚合回归（350 项全绿），段累计对平（98,580+6,300=104,880），域总账移交清单更新
