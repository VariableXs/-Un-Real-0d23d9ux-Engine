# UNX-D1-B05 · OBJECT_ATTRIBUTES 与对象协议（F12081–F12100 · 20 条）

> AI-16 承办｜批主题：OBJECT_ATTRIBUTES 结构全位语义 + 对象句柄协议与 D3 分界——"D1 管结构与调用协议、D3 管名字与存储"分界线写死（任务书专题四原文）｜域账累计：22,580 + 本批 5,520 = 28,100 / 240,000｜嫁接源：纯自研域；结构锚=wdm.h/ntdef.h 公开头文件与 MSDN OBJECT_ATTRIBUTES 文档（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12081–F12100 唯一；命名空间本体（\??\、\Device\、\BaseNamedObjects\ 目录树）归 D3，本域只到结构语义与句柄抽象为止（分界写进双方任务书并联签）。

### UNX-F12081 · OBJECT_ATTRIBUTES 结构语义实装
- 域/批：D1/B05｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12081-J1 六字段布局（Length/RootDirectory/ObjectName/Attributes/SecurityDescriptor/SecurityQualityOfService）偏移与头文件锚一致，sizeof 断言编译期通过
### UNX-F12082 · OBJ_CASE_INSENSITIVE 位语义
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12082-J1 大小写不敏感比较路径判据绿（UPCASE 表挂点消费），双机同路径注入 10/10 次行为一致
### UNX-F12083 · OBJ_INHERIT 位语义
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12083-J1 继承位对象在子进程创建时被消费判据绿（继承表机制归 D2，此处立位语义防重）
### UNX-F12084 · OBJ_KERNEL_HANDLE 位语义
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12084-J1 内核句柄位置位后句柄入系统表非进程表，用户态引用该句柄 10/10 次拒止返回 INVALID_HANDLE
### UNX-F12085 · OBJ_FORCE_ACCESS_CHECK/OPEN_IF/PERMANENT 位族
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12085-J1 三位语义档齐：FORCE_ACCESS_CHECK 绕过内核态豁免判据绿、OPEN_IF 打开已存语义绿、PERMANENT 对象生命周期挂 D3
### UNX-F12086 · RootDirectory 相对打开语义
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12086-J1 相对根句柄打开路径判据绿（根句柄+相对名解析归 D3，此处立协议），根句柄失效 10/10 次正确拒止
### UNX-F12087 · ObjectName 引用语义与缓冲所有权
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12087-J1 ObjectName 为 UNICODE_STRING 引用非拷贝判据绿，调用后源缓冲失活路径 10/10 次正确拒止悬垂
### UNX-F12088 · SecurityDescriptor 字段与 J1 安全挂点
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12088-J1 SD 指针挂点结构落位，访问检查走 J1 域接口分界判据绿（策略实现归 J 部防重）
### UNX-F12089 · SecurityQualityOfService 字段语义
- 域/批：D1/B05｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12089-J1 SQOS 结构指针语义档齐（模拟档/上下文跟踪位），NULL 缺省路径有账非静默
### UNX-F12090 · InitializeObjectAttributes 宏语义与静态校验
- 域/批：D1/B05｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12090-J1 初始化宏产出与手工结构逐字段一致，Length 字段恒 sizeof 校验 10/10 次
### UNX-F12091 · 内核句柄表对接接口冻结（D3 联签）
- 域/批：D1/B05｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12091-J1 句柄表四接口（插入/查询/复制/删除）签名冻结，D3 消费方对接判据绿（表实现归 D3 防重）
### UNX-F12092 · HANDLE_TABLE_ENTRY 语义预告与权限掩码位
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12092-J1 表项低位权限掩码约定语义档齐注出处，GrantedAccess 位提取判据绿
### UNX-F12093 · OBJECT_TYPE_INITIALIZER 类型对象语义预告
- 域/批：D1/B05｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12093-J1 类型对象方法表（open/close/parse/delete）挂点语义档齐，方法缺失路径显式记账非静默
### UNX-F12094 · OBJECT_ATTRIBUTES 非法组合拒止矩阵
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12094-J1 非法组合（Length 错/Object NULL+Root 设/KernelHandle+Inherit 冲突类）≥10 例 10/10 次正确拒止并落矩阵账
### UNX-F12095 · OBJECT_ATTRIBUTES 版本锚分档账
- 域/批：D1/B05｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12095-J1 结构跨版本档（XP→11）差异列登记，锚变全表回归判据绿（F12019 同型机制）
### UNX-F12096 · DuplicateObject 语义预告（D3 联签）
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12096-J1 复制协议参数语义（源/目标进程/DUPLICATE_SAME_ACCESS）档齐，实现归 D3 分界判据绿
### UNX-F12097 · "D1 管结构协议、D3 管名字存储"分界判据与联签文本
- 域/批：D1/B05｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12097-J1 分界联签文本在册（双方任务书互引），跨界条目注入 10/10 次被域守卫判据拦截（越权=缺陷）
### UNX-F12098 · OBJECT_ATTRIBUTES 对照表制度
- 域/批：D1/B05｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12098-J1 结构/位/协议三表齐入对照表制度，抽样 20 行独立可复测
### UNX-F12099 · ktest 对象协议面断言集（B05 批判据聚合）
- 域/批：D1/B05｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12099-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，位语义族与拒止矩阵族独立可单跑
### UNX-F12100 · B05 批域内集成账与联签清单
- 域/批：D1/B05｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12100-J1 批内 19 条互引集成账零悬空引用，联签清单（D3 两对/D2 一对/J1 一对）逐对登记状态
