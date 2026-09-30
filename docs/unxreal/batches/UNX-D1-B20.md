# UNX-D1-B20 · 查询与设置信息族矩阵 NT API 语义档（F12381–F12400 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 5 批——Query/SetInformation* 双族矩阵（Process/Thread/System/Object/VirtualMemory/TimerResolution 六面档位矩阵）、ReturnLength 二段探测协议、未知档位拒止、与 D2/D3 消费联签（M 型尾段 180 条第 5 批）｜域账累计：110,740 + 本批 6,000 = 116,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Get/Set Information 类接口与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B03/B04 为消费关系（PEB/TEB 信息档回读）｜防重：F12381–F12400 唯一；与 K1 分界——K1 管系统级信息面总账，本批立 ntdll 调用面语义（SystemInformationClass 消费）；与 B13 分界——B13 管 Virtual* 操作语义，NtQueryVirtualMemory 查询类归本批。

### UNX-F12381 · NtQueryInformationProcess 档位矩阵
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12381-J1 ≥10 档位（BasicInformation/PEB 基址等）判据绿，逐档缓冲区语义注入 30/30 一致
### UNX-F12382 · NtSetInformationProcess 档位矩阵
- 域/批：D1/B20｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12382-J1 ≥8 档位写语义（含优先级档）判据绿，写权限不足注入 10/10 次正确拒止
### UNX-F12383 · NtQueryInformationThread 档位矩阵
- 域/批：D1/B20｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12383-J1 ≥8 档位判据绿（含线程起始地址档），逐档注入 20/20 一致
### UNX-F12384 · NtSetInformationThread 档位矩阵
- 域/批：D1/B20｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12384-J1 ≥6 档位判据绿，隐藏线程档（不可查询档）注入 10/10 与锚一致
### UNX-F12385 · NtQuerySystemInformation 档位矩阵
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12385-J1 基础档位（Basic/Processor/Process 列表）判据绿，与 K1 信息面分界对平 10/10
### UNX-F12386 · NtQueryObject/NtSetInformationObject 语义档
- 域/批：D1/B20｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12386-J1 对象类型/名称/信息三档判据绿，坏句柄注入 10/10 次正确拒止
### UNX-F12387 · NtQueryVirtualMemory 语义档
- 域/批：D1/B20｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12387-J1 MemoryBasicInformation 档与 B13 Virtual 账逐页对平 10^2 页，坏地址注入 10/10 拒止
### UNX-F12388 · NtQueryTimerResolution/NtSetTimerResolution 语义档
- 域/批：D1/B20｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12388-J1 分辨率查询/设置边界值判据绿，与 A2 时钟口径联签断言 10/10
### UNX-F12389 · 信息族缓冲区协议：ReturnLength 二段探测
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12389-J1 两段式（首探 Length/再取数据）判据绿，BUFFER_TOO_SMALL 注入 20/20 正确
### UNX-F12390 · 信息族错误矩阵
- 域/批：D1/B20｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12390-J1 族错误矩阵（INFO_LENGTH_MISMATCH/ACCESS_DENIED 档）全行齐码，抽样 30 例一致
### UNX-F12391 · 档位未知类拒止判据
- 域/批：D1/B20｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12391-J1 InvalidInfoClass 注入 10/10 次 STATUS_INVALID_INFO_CLASS 正确拒止，越界档全行齐
### UNX-F12392 · 信息族边界样本集
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12392-J1 经典坑样本（Length=0/NULL 缓冲/半缓冲）≥40 例入库全过
### UNX-F12393 · 信息族双机对照判据
- 域/批：D1/B20｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12393-J1 同码双跑 15 场景档位返回一致（S1 母版），值域类零容差（时变值白名单）
### UNX-F12394 · 信息族对照表批入账
- 域/批：D1/B20｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12394-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测
### UNX-F12395 · 信息族性能预算（O1 对标口径）
- 域/批：D1/B20｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12395-J1 查询类 P95 预算在册（O1 配套账），大缓冲拷贝账无红账
### UNX-F12396 · 信息族与 D2/D3 消费联签
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12396-J1 PEB 基址档（→D2）与句柄名查询档（→D3）联签消费面判据绿，登记入冻结三件
### UNX-F12397 · 信息族防幻觉出处账
- 域/批：D1/B20｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12397-J1 出处字段非空率 100%，"待基准机实测"清单在册
### UNX-F12398 · 信息族文档对齐
- 域/批：D1/B20｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12398-J1 族文档与对照表逐行一致，漂移抽查零命中
### UNX-F12399 · ktest 信息族断言集（B20 批判据聚合）
- 域/批：D1/B20｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12399-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，查询族与设置族独立可单跑
### UNX-F12400 · 批小结与 B21 预告
- 域/批：D1/B20｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12400-J1 B20 集成账（19 条互引）零悬空，节对象映射族批（B21）预告登记入域待办账
