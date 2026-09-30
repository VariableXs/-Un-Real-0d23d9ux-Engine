# UNX-D1-B02 · NTSTATUS 编码面（F12021–F12040 · 20 条）

> AI-16 承办｜批主题：NTSTATUS 全编码空间、错误码族语义档、RtlNtStatusToDosError 映射——D/E 部全链消费的状态码地基｜域账累计：B01 6,120 + 本批 5,560 = 11,680 / 240,000｜嫁接源：纯自研域，语义锚=MSDN System Error Codes/Windows Internals 版本锚，ReactOS 对照不抄｜防重：F12021–F12040 唯一；状态码数值一律注出处（公开资料章节/头文件名），查无资料标"待基准机实测"｜下游冻结：NTSTATUS 编码面→D2/D3/D4/D5/E1 全链消费。

### UNX-F12021 · NTSTATUS 编码面全景：位域结构与编码空间账
- 域/批：D1/B02｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12021-J1 Severity（2b）/Customer（1b）/Facility（12b）/Code（17b）位域切分与版本锚一致，编码空间账（已用区间/保留区间/客户区间）逐区间注出处
### UNX-F12022 · 成功族语义档：STATUS_SUCCESS 与 INFORMATIONAL 档
- 域/批：D1/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12022-J1 Severity=0/1 档判定函数对 100 个样本码分类 100% 正确，双机对照零偏差
### UNX-F12023 · STATUS_PENDING 挂起语义与异步完成协议预告
- 域/批：D1/B02｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12023-J1 挂起→完成两态迁移可观测，完成回调消费 IO_STATUS_BLOCK 语义档一致（异步 IO 主体归 B14，此处立状态语义防重）
### UNX-F12024 · 等待族语义档：STATUS_WAIT_0/TIMEOUT/ALERTED/APC
- 域/批：D1/B02｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12024-J1 等待五态（成功/超时/alerted/APC/废弃）在注入用例 10/10 次返回正确码，与 B15 等待族判据同源
### UNX-F12025 · STATUS_ACCESS_DENIED 访问拒绝族与安全联签预告
- 域/批：D1/B02｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12025-J1 访问拒绝触发路径（句柄权限/对象 DACL）分类正确，双机注入同拒绝场景返回同码 10/10 次
### UNX-F12026 · STATUS_INVALID_HANDLE 句柄错误族与句柄账联签
- 域/批：D1/B02｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12026-J1 伪造/已关/越界/权限掩码错四类坏句柄 10/10 次区分正确，与 D3 句柄表联签接口零偏移
### UNX-F12027 · STATUS_INVALID_PARAMETER 参数校验错误族
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12027-J1 参数校验失败注入（NULL/越界/错枚举）三类 10/10 次正确分类，错误定位含参数序号
### UNX-F12028 · STATUS_ACCESS_VIOLATION 与异常状态码族（SEH 联动）
- 域/批：D1/B02｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12028-J1 异常族码（ACCESS_VIOLATION/IN_PAGE_ERROR/STACK_OVERFLOW/ILLEGAL_INSTRUCTION）与 B08 异常分发联动映射一致，读/写违约参数位双机一致
### UNX-F12029 · STATUS_OBJECT_* 对象管理错误族（D3 联签预告）
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12029-J1 OBJECT_PATH_NOT_FOUND/NAME_COLLISION/TYPE_MISMATCH 等 8 码语义档齐，与 D3 名字解析错误映射零偏移
### UNX-F12030 · STATUS_FILE_* 文件错误族预告（D2/B1 联签）
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12030-J1 FILE_IS_A_DIRECTORY/END_OF_FILE/LOCK_CONFLICT 等 10 码语义档齐，与 B14 文件族消费零偏移
### UNX-F12031 · STATUS_MEMORY_* 内存错误族（A4 联签）
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12031-J1 COMMITMENT_LIMIT/NOT_COMMITTED/WORKING_SET_QUOTA 等码与 A4 页管理错误面映射一致
### UNX-F12032 · STATUS_INSUFFICIENT_RESOURCES 资源耗尽语义
- 域/批：D1/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12032-J1 资源池耗尽注入（句柄池/等待块池）10/10 次返回正确码且不崩内核
### UNX-F12033 · 未实现三档：NOT_IMPLEMENTED/UNSUPPORTED/INVALID_DEVICE_REQUEST
- 域/批：D1/B02｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12033-J1 三档语义区分判据绿（未实现/不支持/请求错对象），Varix 全域未实现类返回码审计零静默
### UNX-F12034 · 缓冲族：BUFFER_OVERFLOW/BUFFER_TOO_SMALL/PARTIAL_COPY
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12034-J1 三码触发条件判据绿，ReturnLength 回填语义（需要多大）双机一致 10/10 组
### UNX-F12035 · 取消与废弃族：CANCELLED/ABANDONED/ABANDONED_WAIT_0
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12035-J1 取消注入 10/10 次正确返回，Mutant 废弃等待语义与 B15 联动一致
### UNX-F12036 · RtlNtStatusToDosError 映射表底座
- 域/批：D1/B02｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12036-J1 映射表 ≥100 对（NTSTATUS→Win32 错误码），抽样 30 对与 Windows 真机 GetLastError 读回一致
### UNX-F12037 · 状态码文本化与诊断输出面
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12037-J1 ≥100 常用码文本化输出（三要素：含义/原因/下一步），未知码显式"未文本化"非静默
### UNX-F12038 · 客户码位与保留区间账（Facility 全表）
- 域/批：D1/B02｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12038-J1 Facility 全表（≥20 项）逐项注出处，保留区间写入 10/10 次被校验器拒止
### UNX-F12039 · NTSTATUS 对照表制度：语义档三件套母版
- 域/批：D1/B02｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12039-J1 对照表结构（api_name/category/semver_anchor/status_matrix/judge_ids）立母版，本批 18 码族逐条入表可复测
### UNX-F12040 · ktest NTSTATUS 面断言集（B02 批判据聚合）
- 域/批：D1/B02｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12040-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，位域切分断言族独立可单跑
