# UNX-D1-B10 · RTL_CRITICAL_SECTION 与同步地基（F12181–F12200 · 20 条）

> AI-16 承办｜批主题：RTL_CRITICAL_SECTION 全语义——结构域布局/Enter/Leave 争用路径/DebugInfo/SRWLOCK/条件变量预告（F 型地基批收官，任务书专题五判据：偏移断言+8 线程压测零竞态）｜域账累计：51,120 + 本批 5,740 = 56,860 / 240,000｜嫁接源：纯自研域；结构锚=winnt.h CRITICAL_SECTION 定义与 Windows Internals 同步章（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12181–F12200 唯一；等待原语内核侧（Event/Semaphore）归 B15，本批只立临界区自旋+等待块协议；内核互斥底座归 A3/C1 分层复用。

### UNX-F12181 · RTL_CRITICAL_SECTION 结构语义实装
- 域/批：D1/B10｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12181-J1 六字段布局（DebugInfo/LockCount/RecursionCount/OwningThread/LockSemaphore/SpinCount）偏移与 winnt.h 锚一致，sizeof 断言编译期通过
### UNX-F12182 · EnterCriticalSection 语义：自旋与等待路径
- 域/批：D1/B10｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12182-J1 无争用快路径 ≤20 周期档可观测，争用先自旋后等待路径切换 10/10 次正确，进入后 OwningThread 归属一致
### UNX-F12183 · LeaveCriticalSection 语义与唤醒协议
- 域/批：D1/B10｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12183-J1 递归递减/归零唤醒两档判据绿，唤醒后等待者获锁次序与基准机一致（先进先出档）
### UNX-F12184 · RTL_CRITICAL_SECTION_DEBUG 与 DebugInfo
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12184-J1 DebugInfo 链（Type/CreatorBackTraceIndex/CriticalSection/ProcessLocksList/EntryCount/ContentionCount）语义档齐，争用计数与实测一致
### UNX-F12185 · TryEnterCriticalSection 语义
- 域/批：D1/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12185-J1 忙时立即返回 FALSE 不阻塞判据绿，成功路径与 Enter 后续语义完全一致
### UNX-F12186 · 争用路径与等待块语义预告
- 域/批：D1/B10｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12186-J1 等待块（GUID 类 RTL_WAIT_BLOCK 挂点）协议语义档齐，等待队列入队/出队守恒账零漂移
### UNX-F12187 · SpinCount 语义与初始化参数
- 域/批：D1/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12187-J1 SpinCount 传参/缺省两档判据绿，高位标志位（RTL_CRITICAL_SECTION_FLAG_NO_DEBUG_INFO 档）语义注出处
### UNX-F12188 · RecursionCount/OwningThread 递归语义
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12188-J1 同线程重入递归计数一致，他线程重入注入 10/10 次正确阻塞非误判
### UNX-F12189 · LockSemaphore 事件挂点
- 域/批：D1/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12189-J1 惰性创建事件挂点判据绿（首次争用才建），事件句柄生命周期与临界区销毁联动守恒
### UNX-F12190 · 进程退出临界区泄漏核账
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12190-J1 退出时 CountOfOwnedCriticalSections 非零即泄漏红账并定位到临界区地址（TEB F12070 消费）
### UNX-F12191 · RtlInitializeCriticalSection 族语义
- 域/批：D1/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12191-J1 Initialize/InitializeAndSpinCount 双接口初始化后字段快照与锚一致，重复初始化注入 10/10 次红账
### UNX-F12192 · RtlDeleteCriticalSection 语义
- 域/批：D1/B10｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12192-J1 删除释放事件与 DebugInfo 资源守恒，使用中删除注入 10/10 次拒止红账
### UNX-F12193 · 8 线程争用压测判据（M3 母版）
- 域/批：D1/B10｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12193-J1 8 线程×10^5 次进出零竞态（账守恒+哨兵双检），争用计数与线程观测一致（任务书专题五判据原文）
### UNX-F12194 · 与 A5 死锁检测器联签：锁序账对接
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12194-J1 临界区进出报点入 A5 锁序账，注入 A→B/B→A 交叉持锁 10/10 次被检测器捕获
### UNX-F12195 · SRWLOCK 语义预告
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12195-J1 RTL_SRWLOCK 结构（Ptr 单字段）语义档齐注出处，共享/独占双模式语义判据绿
### UNX-F12196 · RTL_SRWLOCK Acquire/Release 族语义
- 域/批：D1/B10｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12196-J1 AcquireExclusive/Shared 与 ReleaseExclusive/Shared 四接口互斥/共享语义矩阵 16 组全过，递归获取注入正确阻塞
### UNX-F12197 · RTL_CONDITION_VARIABLE 条件变量预告
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12197-J1 SleepConditionVariableCS 与 SRW 档对接语义档齐，唤醒丢失（先 Notify 后 Sleep 经典坑）注入 10/10 次正确处理
### UNX-F12198 · 临界区对照表制度
- 域/批：D1/B10｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12198-J1 结构/接口/错误路径三表齐入对照表制度，抽样 20 行独立可复测
### UNX-F12199 · ktest 临界区面断言集（B10 批判据聚合）
- 域/批：D1/B10｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12199-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，压测族与守恒账族独立可单跑
### UNX-F12200 · F 型地基批集成账：B01–B10 十批联签总闸
- 域/批：D1/B10｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12200-J1 十批互引集成账（进程/状态码/PEB/TEB/对象协议/字符串/异常数据/分发/上下文/临界区）零悬空，联签总闸判据一次全跑绿（Top10 接口⑥立起开门件）
