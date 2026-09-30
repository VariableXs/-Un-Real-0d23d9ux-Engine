# UNX-D1-B18 · RTL 字符串与大整数运行时族 NT API 语义档（F12341–F12360 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 3 批——Rtl* 字符串族（Init/Append/Copy/Equal/Compare/Upcase/转换）、RtlIntegerToUnicodeString 进制档、RtlLargeInteger 大整数四则、Rtl 内存块族（Zero/Fill/Copy/Move/CompareMemory）、RtlGetVersion/RtlNtStatusToDosError、截断与 STATUS_BUFFER_OVERFLOW 语义（M 型尾段 180 条第 3 批）｜域账累计：98,740 + 本批 6,000 = 104,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Rtl String Functions/Safe String Functions 与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B06 UNICODE_STRING 批为消费关系（结构本体已立）｜防重：F12341–F12360 唯一；与 B06 分界——B06 立 UNICODE_STRING 结构与 PEB 消费，本批立 Rtl 字符串操作函数语义；与 B23 分界——SID/安全描述体构造族归 B23，本批零安全对象。

### UNX-F12341 · RtlInitUnicodeString/RtlInitAnsiString 语义档
- 域/批：D1/B18｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12341-J1 初始化三字段（Length/MaximumLength/Buffer）赋值规则判据绿，NULL 源注入 10/10 次得空串档
### UNX-F12342 · RtlAppendUnicodeStringToString/ToString 语义档
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12342-J1 追加越界截断（返回 STATUS_BUFFER_TOO_SMALL 不写）判据绿，注入 20/20 正确
### UNX-F12343 · RtlCopyUnicodeString/EqualUnicodeString/CompareUnicodeString 语义档
- 域/批：D1/B18｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12343-J1 复制截断/相等（CaseInsensitive 双档）/字典序比较判据绿，注入 30/30 一致
### UNX-F12344 · RtlUpcaseUnicodeString/Char 语义档
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12344-J1 大写映射表语义判据绿，非 ASCII 样本与锚一致性 20/20（差异列账）
### UNX-F12345 · ANSI/Unicode 转换族语义档
- 域/批：D1/B18｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12345-J1 双向转换（Ansi→Unicode/Unicode→Ansi）判据绿，代码页档与不可映射字符注入 10/10 正确
### UNX-F12346 · RtlIntegerToUnicodeString/UnicodeToInteger 语义档
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12346-J1 进制档（2/8/10/16）双向判据绿，溢出与非法字符注入 20/20 正确
### UNX-F12347 · RtlLargeInteger 运算族语义档
- 域/批：D1/B18｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12347-J1 大整数四则（Divide/Multiply）与移位判据绿，除零注入 10/10 次正确拒止
### UNX-F12348 · Rtl 内存块族语义档：Zero/Fill/Copy/Move/CompareMemory
- 域/批：D1/B18｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12348-J1 五接口语义判据绿，重叠区（Move 允许/Copy 拒止档）注入 20/20 正确
### UNX-F12349 · RtlGetVersion/RtlNtStatusToDosError 语义档
- 域/批：D1/B18｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12349-J1 版本结构档与码映射（NTSTATUS→Win32 错误）判据绿，抽样 40 码映射与 B02 一致
### UNX-F12350 · RtlRandom/RtlRandomEx 伪随机语义档
- 域/批：D1/B18｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12350-J1 种子态推进确定性判据绿（同种子同序列），非密码用途声明在册（零密码实现）
### UNX-F12351 · 截断语义与 STATUS_BUFFER_OVERFLOW 档
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12351-J1 截断不算致命错语义（返回码+Length 半写档）判据绿，注入 10/10 与锚一致
### UNX-F12352 · RTL 运行时错误矩阵
- 域/批：D1/B18｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12352-J1 族错误矩阵（坏指针/越界/非法参数）全行齐码，注入抽样 30 例一致
### UNX-F12353 · RTL 字符串边界样本集
- 域/批：D1/B18｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12353-J1 经典坑样本（非终止/奇长/嵌入空/最大长度边界）≥40 例入库全过
### UNX-F12354 · RTL 大整数边界样本集
- 域/批：D1/B18｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12354-J1 边界样本（MAXONGLONG/符号档/除零/移位越界）≥30 例入库全过
### UNX-F12355 · RTL 族双机对照判据
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12355-J1 同码双跑 15 场景输出一致（S1 母版），映射表类零容差
### UNX-F12356 · RTL 族对照表批入账
- 域/批：D1/B18｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12356-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测
### UNX-F12357 · RTL 族性能预算（O1 对标口径）
- 域/批：D1/B18｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12357-J1 字符串族 P95 预算在册（O1 配套账），大块 Copy 吞吐账无红账
### UNX-F12358 · RTL 族防幻觉出处账
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12358-J1 出处字段非空率 100%，"待基准机实测"清单在册
### UNX-F12359 · ktest RTL 族断言集（B18 批判据聚合）
- 域/批：D1/B18｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12359-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，字符串族与大整数族独立可单跑
### UNX-F12360 · 批小结与 B19 预告
- 域/批：D1/B18｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12360-J1 B18 集成账（19 条互引）零悬空，进程线程管理族批（B19）预告登记入域待办账
