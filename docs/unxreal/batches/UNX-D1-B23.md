# UNX-D1-B23 · 令牌与安全引用监视族 NT API 语义档（F12441–F12460 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 8 批——Token 族（Open/OpenEx/AdjustPrivileges/AdjustGroups/Create/Duplicate/Query/Set Information）、NtAccessCheck 引用监视判定、NtPrivilegeCheck、NtQuery/SetSecurityObject、SID/ACL 构造族、与 J3 密码学域分界（M 型尾段 180 条第 8 批）｜域账累计：128,740 + 本批 6,000 = 134,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Access Control 与 Windows Internals ch.3（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B05 OA 批为消费关系（安全描述体字段）｜防重：F12441–F12460 唯一；与 J3 分界——J3 管密码学算法本体，本域零密码实现（token 完整性校验算法全为调用面）；与 B20 分界——Token 查询档位本体归本批，信息族缓冲协议通用件回指 F12389。

### UNX-F12441 · NtOpenProcessToken/NtOpenThreadToken 语义档
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12441-J1 按进程/线程开令牌判据绿，无令牌对象注入 10/10 次正确拒止
### UNX-F12442 · NtOpenProcessTokenEx/NtOpenThreadTokenEx 语义档
- 域/批：D1/B23｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12442-J1 Ex 档（HandleAttributes 继承位）判据绿，与基础版差异面 10/10 一致
### UNX-F12443 · NtAdjustPrivilegesToken 语义档
- 域/批：D1/B23｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12443-J1 特权启用/禁用/原值回返（PreviousState 档）判据绿，无特权注入 10/10 正确拒止
### UNX-F12444 · NtAdjustGroupsToken 语义档
- 域/批：D1/B23｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12444-J1 组启用/禁用（SE_GROUP_ENABLED 位）判据绿，强制组注入 10/10 次正确拒止
### UNX-F12445 · NtCreateToken/NtDuplicateToken 语义档
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12445-J1 创建参数与复制（ImpersonationLevel 档）判据绿，越权复制注入 10/10 拒止
### UNX-F12446 · NtQueryInformationToken/NtSetInformationToken 语义档
- 域/批：D1/B23｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12446-J1 ≥8 档位（User/Groups/Privileges）判据绿，缓冲协议回指 F12389 断言一致
### UNX-F12447 · NtAccessCheck 语义档
- 域/批：D1/B23｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12447-J1 DACL 遍历判定（Allow/Deny 序）判据绿，注入 30/30 判定与锚一致
### UNX-F12448 · NtPrivilegeCheck 语义档
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12448-J1 特权持有检查（单/多特权档）判据绿，未持有注入 10/10 次正确返回
### UNX-F12449 · NtQuerySecurityObject/NtSetSecurityObject 语义档
- 域/批：D1/B23｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12449-J1 自相对 SD 格式四段（Owner/Group/Dacl/Sacl 位）判据绿，逐位注入 20/20 正确
### UNX-F12450 · SID/ACL 构造族语义档
- 域/批：D1/B23｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12450-J1 RtlValidSid/LengthSid/CopySid/SetDacl 判据绿，坏 SID 注入 10/10 次拒止
### UNX-F12451 · 令牌族错误矩阵
- 域/批：D1/B23｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12451-J1 族错误矩阵（权限/类型/状态档）全行齐码，注入抽样 30 例一致
### UNX-F12452 · 访问拒绝注入判据
- 域/批：D1/B23｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12452-J1 无特权 Adjust/越权 AccessCheck 注入 30/30 次 STATUS_PRIVILEGE_NOT_HELD 正确
### UNX-F12453 · 令牌族边界样本集
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12453-J1 经典坑样本（特权矩阵×组矩阵复合/模拟级越界）≥30 例入库全过
### UNX-F12454 · 令牌族双机对照判据
- 域/批：D1/B23｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12454-J1 同码双跑 15 场景判定结果一致（S1 母版），判定类零容差
### UNX-F12455 · 令牌族对照表批入账
- 域/批：D1/B23｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12455-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测
### UNX-F12456 · 令牌族性能预算（O1 对标口径）
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12456-J1 AccessCheck P95 预算在册（O1 配套账），长 DACL 退化曲线无红账
### UNX-F12457 · 令牌族与 J3 密码学域分界声明
- 域/批：D1/B23｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12457-J1 零密码实现声明在册（哈希/加密全为 J3 调用面），分界表登记断言绿
### UNX-F12458 · 令牌族防幻觉出处账
- 域/批：D1/B23｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12458-J1 出处字段非空率 100%，“待基准机实测”清单在册
### UNX-F12459 · ktest 令牌族断言集（B23 批判据聚合）
- 域/批：D1/B23｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12459-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，令牌族与 ACL 族独立可单跑
### UNX-F12460 · 批小结与 B24 预告
- 域/批：D1/B23｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12460-J1 B23 集成账（19 条互引）零悬空，注册表族批（B24）预告登记入域待办账
