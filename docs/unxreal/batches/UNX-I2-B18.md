# UNX-I2-B18 · Winsock 族 III：扩展函数 AcceptEx/ConnectEx/TransmitFile（UNX-F33141–F33160 · 20 条）

> AI-42 承办（波 16 二段收官（B16–B40 · 500 项新功能））｜批主题：Winsock 族 III：扩展函数 AcceptEx/ConnectEx/TransmitFile｜域账累计：108,000 / 240,000｜判据主轴：BSD socket ↔ winsock 语义对照、overlapped IO｜域 ID F32801–F33600 与已收口域零撞号｜上游：AI-12（C2 分发表）、AI-37（H2 事件惯例）、AI-21（E1 wine 冻结面）、AI-14（C4 挂载点）——未收口面 Schema 先行+fake（R-I2-001）｜下游：AI-23（E3）、AI-21（winsock 全量面）、AI-14（AF_UNIX）、AI-50（J5 数据源）｜红线声明：无引导设施与硬件数据安全红线触发条目；升级接管 compatstar2/winsock.rs 留痕；存量零堆纪律保留；禁灌水引用——对照账禁『同上参照』；防幻觉——winsock 行为带 MSDN/版本锚，双名称问题以基准机实测为准；真机判据随闸门补测（R-I2-002，开发期零 QEMU 零实机写）

### UNX-F33141 · B17 卷内登记与 B18 计划预告
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33141-J1 B17 段 20 条登记，域账 102,000/240,000 断言，B18 预告
### UNX-F33142 · AcceptEx 语义与本地/远程地址预留缓冲
- 域/批：I2/B18｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F33142-J1 AcceptEx 接受 1000 次，sAcceptDevice 预绑定语义与基准对照，地址提取 GetAcceptExSockaddrs 100 组逐字节一致
### UNX-F33143 · ConnectEx 语义与未连接 socket 前置条件
- 域/批：I2/B18｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F33143-J1 前置 bind 要求行为与基准一致 100 组，overlapped 完成通知恰好一次（B05 联动），未 bind 拒绝码一致
### UNX-F33144 · TransmitFile 语义与 TransmitPackets 登记面
- 域/批：I2/B18｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F33144-J1 文件-套接字直达 1GB 零错（校验和），断点续传 flags（TF_DISCONNECT/REUSE）矩阵 9 格对照，Packets 面登记消费边界
### UNX-F33145 · GetAcceptExSockaddrs 地址解析全语义
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33145-J1 三地址族解析 100 组与基准一致，缓冲布局偏移账，畸形缓冲注入 20 例全拒
### UNX-F33146 · 扩展函数指针获取路径（SIO_GET_EXTENSION_FUNCTION_POINTER）
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33146-J1 四扩展函数指针获取 100 次非空且可调用，与 B16 WSAIoctl 面对账，伪指针注入拦截 10/10
### UNX-F33147 · AcceptEx 与 listen backlog/EPOLLEXCLUSIVE 联动
- 域/批：I2/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F33147-J1 抢占 accept 联动 10^5 次零饥饿（B14 判据跨批联动），队列溢出拒绝账
### UNX-F33148 · 扩展函数 overlapped 取消语义
- 域/批：I2/B18｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F33148-J1 CancelIoEx 取消 1000 次全部 ERROR_OPERATION_ABORTED，取消竞态（完成瞬间取消）注入 100 次与基准一致
### UNX-F33149 · 扩展函数资源账与池化纪律
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33149-J1 扩展上下文池化 10^6 次零堆断言（grep 零命中），池耗尽降级可诊断
### UNX-F33150 · ktest Winsock-III B18 断言集
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33150-J1 本批 13 条判据聚合全跑 100%，失败注入必红三条（预留缓冲越界/前置条件漏检/取消漏标）
### UNX-F33151 · B18 防重与边界登记（与 I1 直达面分界）
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33151-J1 文件直达零拷贝归 I2 调用面、块设备路径归 C3 边界声明，防重 grep（AcceptEx/TransmitFile 族）零撞号
### UNX-F33152 · B18 批收口：Winsock-III 段对账与移交
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33152-J1 批内 20 条守恒 6,000 求和精确，对账落盘，B19 依赖清单
### UNX-F33153 · Winsock-III 基准机对照快照
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33153-J1 对照快照 30 组落盘，偏差 0，J1 C 段累计 70/80
### UNX-F33154 · Winsock-III 性能基线账
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33154-J1 AcceptEx 接受 P95 ≤ 60μs、TransmitFile 吞吐基线记账，基线入域档
### UNX-F33155 · Winsock-III 语义书（MSDN 锚逐条）
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33155-J1 扩展函数手册落盘，锚 ≥95%，缓冲布局章节复核
### UNX-F33156 · Winsock-III 体验日志与异常显性化
- 域/批：I2/B18｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F33156-J1 缓冲预留误配指纹自动标记，三要素审计
### UNX-F33157 · Winsock-III 与 E1 fake 消费验证
- 域/批：I2/B18｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33157-J1 wine fake 消费扩展函数面 100 场景通过，契合度断言
### UNX-F33158 · Winsock-III fuzz 专项（缓冲/取消）
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33158-J1 fuzz 10^4 零崩溃，缓冲边界/取消竞态种子 ≥60，断言入 ktest 长驻
### UNX-F33159 · Winsock-III 诊断转储（扩展上下文生命线）
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33159-J1 扩展上下文全生命线转储 1000 例 < 20ms，一致性 50/50
### UNX-F33160 · Winsock-III 与 B05/B14 跨批聚合回归
- 域/批：I2/B18｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33160-J1 三批判据聚合一次全跑（≤3min）100%，交叉断言（通知×抢占×扩展）验证