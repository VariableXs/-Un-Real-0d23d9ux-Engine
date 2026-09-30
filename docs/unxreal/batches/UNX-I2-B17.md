# UNX-I2-B17 · Winsock 族 II：WSAConnect/WSAAccept/WSASend/WSARecv（UNX-F33121–F33140 · 20 条）

> AI-42 承办（波 16 二段收官（B16–B40 · 500 项新功能））｜批主题：Winsock 族 II：WSAConnect/WSAAccept/WSASend/WSARecv｜域账累计：102,000 / 240,000｜判据主轴：BSD socket ↔ winsock 语义对照、overlapped IO｜域 ID F32801–F33600 与已收口域零撞号｜上游：AI-12（C2 分发表）、AI-37（H2 事件惯例）、AI-21（E1 wine 冻结面）、AI-14（C4 挂载点）——未收口面 Schema 先行+fake（R-I2-001）｜下游：AI-23（E3）、AI-21（winsock 全量面）、AI-14（AF_UNIX）、AI-50（J5 数据源）｜红线声明：无引导设施与硬件数据安全红线触发条目；升级接管 compatstar2/winsock.rs 留痕；存量零堆纪律保留；禁灌水引用——对照账禁『同上参照』；防幻觉——winsock 行为带 MSDN/版本锚，双名称问题以基准机实测为准；真机判据随闸门补测（R-I2-002，开发期零 QEMU 零实机写）

### UNX-F33121 · B16 卷内登记与 B17 计划预告
- 域/批：I2/B17｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33121-J1 B16 段 20 条登记入总纲，域账 96,000/240,000 断言，B17 预告
### UNX-F33122 · WSAConnect 全语义与条件函数（lpfnCondition）
- 域/批：I2/B17｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F33122-J1 双模连接 1000 次与基准对照，条件函数回调点 100 组触发序一致，callerdata/calleedata 交换账落盘
### UNX-F33123 · WSAAccept 条件接受与 CF_ACCEPT/REJECT/DEFER 三态
- 域/批：I2/B17｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F33123-J1 三态各 100 次行为与基准一致，DEFER 重试语义账，条件函数异常不拖垮内核断言（沙箱隔离）
### UNX-F33124 · WSASend/WSARecv 聚散双模全语义
- 域/批：I2/B17｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F33124-J1 WSABUF 聚散 1000 组与 POSIX writev/readv 交叉对照，阻塞/非阻塞/overlapped 三模矩阵 12 格全过
### UNX-F33125 · WSASendTo/WSARecvFrom 数据报面
- 域/批：I2/B17｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F33125-J1 数据报收发 10^4 次零丢失，源地址回填与 B10 sendto 面一致性断言，EMSGSIZE 一致
### UNX-F33126 · WSARecvEx 与 MSG_PARTIAL 语义
- 域/批：I2/B17｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33126-J1 部分报文 MSG_PARTIAL 标记 100 组与基准对照，截断恢复路径断言，与 B11 MSG_TRUNC 语义差异登记
### UNX-F33127 · WSASendDisconnect/WSARecvDisconnect 有序释放面
- 域/批：I2/B17｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33127-J1 有序释放 100 次半关闭语义与 B10 shutdown 联动一致，disconnect data 传递账
### UNX-F33128 · winsock 双模底座与 E3 冻结契约 v1 对账
- 域/批：I2/B17｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F33128-J1 E3 契约 v1 消费对账 100%（connect/send/recv 双模底座），偏差登记入差异合同，Schema 先行纪律兑现
### UNX-F33129 · WSASend/Recv overlapped 路径与 B05 地基联动
- 域/批：I2/B17｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F33129-J1 overlapped 提交 10^4 次恰好一次通知不变量复验（B05 判据跨批联动），取消路径 ERROR_OPERATION_ABORTED 100%
### UNX-F33130 · winsock 面并发 8 线程收发安全
- 域/批：I2/B17｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33130-J1 并发 10^5 次零撕裂零错序，per-socket 锁账入域档，竞争指纹日志自动标记
### UNX-F33131 · ktest Winsock-II B17 断言集
- 域/批：I2/B17｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33131-J1 本批 13 条判据聚合全跑 100%，失败注入必红三条（条件函数异常/PARTIAL 漏标/overlapped 重复通知）
### UNX-F33132 · B17 防重与边界登记
- 域/批：I2/B17｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33132-J1 防重 grep（WSAConnect/WSASend 族）零撞号，条件函数执行域边界声明（用户态回调内核侧拦截）
### UNX-F33133 · B17 批收口：Winsock-II 段对账与移交
- 域/批：I2/B17｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F33133-J1 批内 20 条守恒 6,000 求和精确，对账落盘，B18 依赖清单
### UNX-F33134 · Winsock-II 基准机对照快照
- 域/批：I2/B17｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33134-J1 对照快照 30 组落盘，偏差 0，J1 C 段累计 40/80
### UNX-F33135 · Winsock-II 性能基线账
- 域/批：I2/B17｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F33135-J1 WSASend 聚散 4 向量 P95 ≤ 80μs（10^5 次实测），基线入域档
### UNX-F33136 · Winsock-II 语义书（MSDN 锚逐条）
- 域/批：I2/B17｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33136-J1 逐调用手册落盘，锚 ≥95%，条件函数协议章节复核
### UNX-F33137 · Winsock-II 体验日志与异常显性化
- 域/批：I2/B17｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F33137-J1 条件误配/阻塞误配指纹自动标记，三要素审计
### UNX-F33138 · Winsock-II 与 E1/E3 双 fake 消费验证
- 域/批：I2/B17｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F33138-J1 双 fake 消费 200 场景通过，契合度断言，契约反馈
### UNX-F33139 · Winsock-II LTP/LTP-winsock 映射登记
- 域/批：I2/B17｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F33139-J1 映射表 ≥20 条登记（winsock 侧 LTP 替代基准注明），随闸门补测
### UNX-F33140 · Winsock-II fuzz 专项
- 域/批：I2/B17｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F33140-J1 fuzz 10^4 零崩溃，畸形 WSABUF/条件函数注入全拦截，种子库 ≥50