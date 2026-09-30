# UNX-D1-B04 · TEB 深化（F12061–F12080 · 20 条）

> AI-16 承办｜批主题：TEB 逐字段深化——NT_TIB 三锚/ClientId/LastError/TLS/WOW32/Win32ThreadInfo/gs 写路径/生命周期/对照表制度｜域账累计：17,160 + 本批 5,420 = 22,580 / 240,000｜嫁接源：纯自研域；偏移锚=公开 TEB 布局资料与 Windows Internals（版本锚 ADR-UNX-008），应用硬编码偏移读取是本批"艺术品级"验收落点（总册 D1 域行原文），ReactOS 对照不抄｜防重：F12061–F12080 唯一；与 F12006（B01 TEB 实装）分层——F12006 立结构与 Self 自指，本批立逐字段语义档与联签挂点｜防幻觉高压线：每偏移注出处，查无资料标"待基准机实测"。

### UNX-F12061 · TEB 字段总账：x64 逐字段偏移与版本分档
- 域/批：D1/B04｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12061-J1 x64 TEB 全字段偏移表（NT_TIB 0x0 起至 GdiReserved）逐字段注出处，Win10/11 分档列分列，抽样 20 字段双机零差异
### UNX-F12062 · NT_TIB 语义：StackBase/StackLimit/Self 三锚
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12062-J1 栈界两锚与线程栈实际范围一致（±1 页容差判据），Self 自指 50/50 线程通过，x64 链式 ExceptionList 结构位保留语义档齐
### UNX-F12063 · ClientId 语义：UniqueProcess/UniqueThread 双槽
- 域/批：D1/B04｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12063-J1 双槽值与进程/线程台账 ID 逐值一致 50/50，与 GetCurrentProcessId 同源判据绿
### UNX-F12064 · LastErrorValue 读写语义与 GetLastError 链
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12064-J1 槽位写后读回逐值一致 10^4 次，Win32 错误码→LastError 链与 RtlNtStatusToDosError 消费判据绿
### UNX-F12065 · ThreadLocalStoragePointer：TLS 槽数组挂点
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12065-J1 TLS 扩展数组寻址判据绿（TlsSlots 64 槽 + 扩展指针），分配/回收守恒账零漂移
### UNX-F12066 · WOW32Reserved 与 WOW64 联签预告（D2）
- 域/批：D1/B04｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12066-J1 WOW32Reserved 槽位语义档齐，WOW64 转换跳板挂点接口冻结（消费方 D2 wow64cpu 链）
### UNX-F12067 · Win32ThreadInfo 与 D4 消息面联签预告
- 域/批：D1/B04｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12067-J1 Win32ThreadInfo/User32Reserved 占位语义档齐，D4 消息队列宿主挂点接口冻结零偏移
### UNX-F12068 · CurrentLocale 与区域语义账
- 域/批：D1/B04｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12068-J1 LCID 槽位读写语义档齐，双机同区域设置读回一致，非法 LCID 注入 10/10 次拒止
### UNX-F12069 · ActivationContextStackPointer 与激活上下文栈
- 域/批：D1/B04｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12069-J1 激活上下文栈指针（gs:[0x2c8] 档）语义档齐注出处，压入/弹出守恒判据绿
### UNX-F12070 · CountOfOwnedCriticalSections 语义
- 域/批：D1/B04｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12070-J1 与 RTL_CRITICAL_SECTION 进出联动计数守恒（B10 消费），退出进程时计数非零即泄漏红账
### UNX-F12071 · TEB gs 段基址写路径：GDT/MSR 链与 A3 联签
- 域/批：D1/B04｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12071-J1 线程切换时 gs 基址原子更新判据绿（wrmsr KERNEL_GS_BASE 路径注出处），跨核迁移线程 TEB 挂点一致性 50/50
### UNX-F12072 · TEB 静态布置与线程创建交接（A3/C1 联签）
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12072-J1 TEB 页从线程栈预留区布置（不走堆），创建交接前后字段快照零缺失，销毁回收守恒
### UNX-F12073 · TEB 越界写检测与只读快照
- 域/批：D1/B04｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12073-J1 调试档哨兵注入越界写 10/10 次定位红账，用户态对保护字段写 10/10 次触发 #PF
### UNX-F12074 · TEB 布局静态断言器
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12074-J1 编译期 const 断言 ≥25 字段偏移，改错一偏移编译失败（断言防手滑，与 F12058 同型不同表）
### UNX-F12075 · TEB Version/SpareBytes 保留字段账
- 域/批：D1/B04｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F12075-J1 保留字段缺省值与版本锚一致，写入保留区 10/10 次红账（调试档）
### UNX-F12076 · TxFsContext 字段语义
- 域/批：D1/B04｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F12076-J1 TxFsContext（事务上下文）槽位语义档齐注出处，未启用事务路径恒零判据绿
### UNX-F12077 · EnvironmentPointer/ArbitraryUserPointer 语义
- 域/批：D1/B04｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12077-J1 两字段语义档齐（NT_TIB.EnvironmentPointer/ArbitraryUserPointer），NtCurrentTeb 直读判据绿
### UNX-F12078 · TEB 生命周期：创建/销毁/复用语义链
- 域/批：D1/B04｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12078-J1 创建→挂 gs→活→摘 gs→回收五步全链日志可回放，复用前字段清洗断言零残留
### UNX-F12079 · TEB 对照表制度：逐字段语义档与可复测判据
- 域/批：D1/B04｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12079-J1 全字段入对照表五列齐（字段名/偏移/版本档/出处/判据 ID），抽样 30 字段独立可复测
### UNX-F12080 · ktest TEB 面断言集（B04 批判据聚合）
- 域/批：D1/B04｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12080-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，Self 自指族与偏移断言族独立可单跑
