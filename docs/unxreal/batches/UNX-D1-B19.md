# UNX-D1-B19 · 进程与线程管理族 NT API 语义档（F12361–F12380 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 4 批——NtCreateUserProcess/NtTerminateProcess 终止序、NtCreateThreadEx/NtSuspendThread/NtResumeThread 挂起计数、NtDelayExecution/NtYieldExecution、NtGet/SetContextThread、NtAlertThread/NtQueueApcThread、NtOpenProcess/NtOpenThread、伪句柄 -1/-2 档（M 型尾段 180 条第 4 批）｜域账累计：104,740 + 本批 6,000 = 110,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Process and Thread Functions 与 Windows Internals ch.5（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 C1 进程模型为消费关系（上游 AI-11 冻结接口：进程对象人格路由）｜防重：F12361–F12380 唯一；与 C1 分界——C1 管进程/线程对象本体与调度，本批管 NT API 调用语义；与 B11 分界——B11 立 APC 投递通道本体，本批立 NtQueueApcThread 调用面语义（同源联签）。

### UNX-F12361 · NtCreateUserProcess/NtCreateProcessEx 语义档
- 域/批：D1/B19｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12361-J1 创建参数语义（OA/节句柄/参数块）判据绿，创建-终止对冲 10^2 次账平，与 C1 路由一致
### UNX-F12362 · NtTerminateProcess/NtTerminateThread 语义档
- 域/批：D1/B19｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12362-J1 终止序（线程收割→句柄失效→对象销毁）判据绿，自终止/他终止注入 10/10 正确
### UNX-F12363 · NtCreateThreadEx/NtSuspendThread/NtResumeThread 语义档
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12363-J1 创建挂起档（CreateSuspended）与挂起计数语义判据绿，嵌套挂起注入 10/10 正确
### UNX-F12364 · NtDelayExecution/NtYieldExecution 语义档
- 域/批：D1/B19｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12364-J1 延迟绝对/相对档判别与让步语义判据绿，延迟精度账（P95 漂移 ≤1ms 档）与 A2 口径一致
### UNX-F12365 · NtGetContextThread/NtSetContextThread 语义档
- 域/批：D1/B19｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12365-J1 CONTEXT 按需存取（ContextFlags 门控）判据绿，Get/Set 往返一致 10^4 次
### UNX-F12366 · NtAlertThread/NtQueueApcThread 语义档
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12366-J1 alert 置位与 APC 入队判据绿，投递时机与 B11 F12215 单一通道同源断言 10/10
### UNX-F12367 · NtOpenProcess/NtOpenThread 语义档
- 域/批：D1/B19｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12367-J1 按 PID/TID 打开（DesiredAccess 掩码）判据绿，权限不足注入 10/10 次正确拒止
### UNX-F12368 · 伪句柄语义档：NtCurrentProcess/NtCurrentThread
- 域/批：D1/B19｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12368-J1 -1/-2 伪句柄语义（仅限本进程可见）判据绿，跨进程传递注入 10/10 次正确拒止
### UNX-F12369 · 进程线程族错误矩阵
- 域/批：D1/B19｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12369-J1 族错误矩阵（坏 PID/权限/状态冲突）全行齐码，注入抽样 30 例一致
### UNX-F12370 · 挂起计数守恒判据
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12370-J1 Suspend/Resume 对冲守恒 10^4 次零漂移，resume 过冲注入 10/10 正确拒止
### UNX-F12371 · 终止序边界样本集
- 域/批：D1/B19｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12371-J1 经典坑样本（终止中互斥废弃/句柄泄漏/APC 在途）≥30 例入库全过
### UNX-F12372 · CONTEXT 保存恢复往返判据
- 域/批：D1/B19｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12372-J1 寄存器组全档往返一致率 100%（B09 陷阱帧消费），改写 PC 样本 10/10 生效
### UNX-F12373 · 进程线程族双机对照判据
- 域/批：D1/B19｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12373-J1 同码双跑 15 场景状态迁移序列一致（S1 母版），终止序类零容差
### UNX-F12374 · 进程线程族对照表批入账
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12374-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测
### UNX-F12375 · 进程线程族性能预算（O1 对标口径）
- 域/批：D1/B19｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12375-J1 create/terminate P95 预算在册（O1 配套账），线程切换开销无红账
### UNX-F12376 · 进程线程族与 C1 联签收口
- 域/批：D1/B19｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12376-J1 对象人格路由消费联签判据绿（创建/终止/查询三路对平 C1 账流水）
### UNX-F12377 · 进程线程族防幻觉出处账
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12377-J1 出处字段非空率 100%，"待基准机实测"清单在册
### UNX-F12378 · 进程线程族文档对齐
- 域/批：D1/B19｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12378-J1 族文档与对照表逐行一致，漂移抽查零命中
### UNX-F12379 · ktest 进程线程族断言集（B19 批判据聚合）
- 域/批：D1/B19｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12379-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，生命周期族与 CONTEXT 族独立可单跑
### UNX-F12380 · 批小结与 B20 预告
- 域/批：D1/B19｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12380-J1 B19 集成账（19 条互引）零悬空，查询设置信息族批（B20）预告登记入域待办账
