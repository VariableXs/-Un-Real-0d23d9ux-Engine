# UNX-D1 · 域归集卷（AI-100 归集快照 2026-10-01）

> 由 AI-100 归集器 v2 从主汇编册块级切分生成（块定域：标题 token > 正文 token 投票 > 前块延续）；主汇编册仍为唯一权威总册，本卷为按域阅读视图，零改写零删节。

<!-- 主册行 15207 · ### UNX-D1 · NT API 语义面（ntdll） -->
### UNX-D1 · NT API 语义面（ntdll）

> 域档｜承办 AI-16｜批册 40（01–40）｜条目 800｜F12001–F12800｜行数合计 240,000｜已深化 800 / 骨架 0

<!-- 主册行 15211 · #### UNX-D1-B01 · 进程对象与 PEB/TEB 地基（F12001–F12020 · 20 条） -->
#### UNX-D1-B01 · 进程对象与 PEB/TEB 地基（F12001–F12020 · 20 条）

> AI-16 承办｜批主题：Windows 人格进程对象全链 + PEB/TEB 实装起步 + ._pdata 解析（Top10 接口⑥地基立起）｜域账累计：本批 6,120 / 240,000｜嫁接源：无上游核心可嫁接（纯自研域），语义锚=Windows Internals/MSDN 公开资料版本锚（ADR-UNX-008），ReactOS 仅对照不抄｜防重：条目 ID F12001–F12020 唯一，与 C1（POSIX 进程模型）为分层复用不复用语义——D1 进程对象在 C1 地址空间/线程底座之上独立成层｜上游：AI-11（C1 进程模型，冻结接口：进程对象人格路由）｜批注：源册示例条目 F12005/F12006/F12007 均落本批，PEB/TEB 偏移类条目全部注出处（版本锚资料章节号），查无资料标"待基准机实测"，禁编造（§40 幻觉红线）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12001 | Windows 人格进程对象与内核写路径 | 360 | 已深化 | UNX-F12001-J1 进程对象创建/销毁全链可观测，PEB/TEB 由内核侧唯一写者填充，双人格并存隔离判据绿（与 C1 专题六联动） |
| UNX-F12002 | EPROCESS 语义模型与进程控制块字段账 | 340 | 已深化 | UNX-F12002-J1 字段账逐项注出处（版本锚章节号），核心字段（UniqueProcessId/ActiveProcessLinks/ProcessQuotaUsage 等）语义档双机对照零差异，查无资料字段显式标"待基准机实测"非零条 |
| UNX-F12003 | NT 进程创建内核路径语义（NtCreateUserProcess 底座） | 380 | 已深化 | UNX-F12003-J1 创建路径 10 步序列（参数校验→对象建→PEB 填→TEB 填→线程挂→句柄回）真机逐步日志可回放，失败注入 10/10 次按错误矩阵落正确 NTSTATUS |
| UNX-F12004 | 进程终止语义与退出状态传播链 | 320 | 已深化 | UNX-F12004-J1 NtTerminateProcess 退出码传播全链可观测，等待方取回的 ExitStatus 与终止方写入值逐字节一致 50/50 次 |
| UNX-F12005 | PEB 结构语义实装（x64） | 340 | 已深化 | UNX-F12005-J1 逐字段偏移与版本锚一致（L1 级）；BeingDebugged 写位可观测、IsDebuggerPresent 读回一致；8 内联 API 双机同码逐值一致 |
| UNX-F12006 | TEB 与 NT_TIB 语义实装 | 300 | 已深化 | UNX-F12006-J1 gs:[0x30] Self 指针自指正确，TLS 槽数组挂点可寻址，LastErrorCode 读写语义与版本锚一致 |
| UNX-F12007 | x64 .pdata 解析与 RUNTIME_FUNCTION 查找 | 320 | 已深化 | UNX-F12007-J1 RIP 二分查找命中/未命中路径正确，100 个函数样本偏移逐条核验，损坏 .pdata 拒止不崩 |
| UNX-F12008 | RTL_USER_PROCESS_PARAMETERS 进程参数块语义 | 300 | 已深化 | UNX-F12008-J1 命令行/环境块/当前目录三组 UNICODE_STRING 布局与版本锚一致，双机注入同一参数集读回逐值一致 30/30 组 |
| UNX-F12009 | 句柄抽象底座：HANDLE 语义与引用规则 | 320 | 已深化 | UNX-F12009-J1 句柄值编码（索引+掩码+generation）防重用判据绿，伪造句柄 10/10 次拒止返回 STATUS_INVALID_HANDLE |
| UNX-F12010 | PEB 只读快照页映射与内核唯一写者协议 | 320 | 已深化 | UNX-F12010-J1 用户态对 PEB 页写访问触发 #PF 可观测 10/10 次，内核写路径白名单外写入 10/10 次红账拦截 |
| UNX-F12011 | TEB Self 指针与 gs 段基址挂点初始化 | 280 | 已深化 | UNX-F12011-J1 每线程 gs 基址唯一且 Self 自指 50/50 线程通过，跨线程读他线程 Self 值与台账一致 |
| UNX-F12012 | 进程对象安全描述符挂点（与 J1 联签预告） | 280 | 已深化 | UNX-F12012-J1 安全描述符指针挂点结构落位，访问检查走 J1 域接口（本域只挂点不实现策略）分界判据绿 |
| UNX-F12013 | KAFFINITY 进程亲和性掩码语义 | 260 | 已深化 | UNX-F12013-J1 亲和掩码与在线核集交集校验 10/10 次，越界掩码按 STATUS_INVALID_PARAMETER 拒止 |
| UNX-F12014 | 进程基础优先级与优先级类语义账 | 260 | 已深化 | UNX-F12014-J1 优先级类到基础优先级映射表与版本锚一致，双机同参数读回逐值一致 10/10 组 |
| UNX-F12015 | 进程配额（PagedPool/NonPagedPool）语义账 | 260 | 已深化 | UNX-F12015-J1 配额用量账与分配动作守恒（分配-释放差为零），超配额拒止路径 10/10 次正确 NTSTATUS |
| UNX-F12016 | 进程拆除次序协议：句柄关→内存拆→对象销毁 | 300 | 已深化 | UNX-F12016-J1 拆除三阶段次序日志可回放且乱序注入 10/10 次被次序守卫拦截，拆除后句柄表/内存账归零 |
| UNX-F12017 | 双人格进程隔离判据与人格路由消费 | 280 | 已深化 | UNX-F12017-J1 双人格进程并存时 PEB/TEB 仅 NT 人格进程可见，人格路由指向错误注入 10/10 次被拒 |
| UNX-F12018 | PROCESSINFOCLASS 查询底座与信息类枚举账 | 300 | 已深化 | UNX-F12018-J1 信息类枚举账（ProcessBasicInformation 起步 ≥10 类）逐类有语义档，未实现类显式返回 STATUS_NOT_IMPLEMENTED 非静默 |
| UNX-F12019 | PEB/TEB 版本分档表隔离机制（Win10/11 共性字段先行） | 280 | 已深化 | UNX-F12019-J1 版本分档表切换锚版后偏移表自动随档（F12005 风险条目原文口径），锚未定档下共性字段实装零返工 |
| UNX-F12020 | ktest 进程对象面断言集（B01 批判据聚合） | 320 | 已深化 | UNX-F12020-J1 本批 19 条判据全部聚合入 ktest NT 语义面，一次命令全跑通过率 100%，skip 态强制带原因 |

<!-- 主册行 15238 · #### UNX-D1-B02 · NTSTATUS 编码面（F12021–F12040 · 20 条） -->
#### UNX-D1-B02 · NTSTATUS 编码面（F12021–F12040 · 20 条）

> AI-16 承办｜批主题：NTSTATUS 全编码空间、错误码族语义档、RtlNtStatusToDosError 映射——D/E 部全链消费的状态码地基｜域账累计：B01 6,120 + 本批 5,560 = 11,680 / 240,000｜嫁接源：纯自研域，语义锚=MSDN System Error Codes/Windows Internals 版本锚，ReactOS 对照不抄｜防重：F12021–F12040 唯一；状态码数值一律注出处（公开资料章节/头文件名），查无资料标"待基准机实测"｜下游冻结：NTSTATUS 编码面→D2/D3/D4/D5/E1 全链消费。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12021 | NTSTATUS 编码面全景：位域结构与编码空间账 | 340 | 已深化 | UNX-F12021-J1 Severity（2b）/Customer（1b）/Facility（12b）/Code（17b）位域切分与版本锚一致，编码空间账（已用区间/保留区间/客户区间）逐区间注出处 |
| UNX-F12022 | 成功族语义档：STATUS_SUCCESS 与 INFORMATIONAL 档 | 240 | 已深化 | UNX-F12022-J1 Severity=0/1 档判定函数对 100 个样本码分类 100% 正确，双机对照零偏差 |
| UNX-F12023 | STATUS_PENDING 挂起语义与异步完成协议预告 | 300 | 已深化 | UNX-F12023-J1 挂起→完成两态迁移可观测，完成回调消费 IO_STATUS_BLOCK 语义档一致（异步 IO 主体归 B14，此处立状态语义防重） |
| UNX-F12024 | 等待族语义档：STATUS_WAIT_0/TIMEOUT/ALERTED/APC | 300 | 已深化 | UNX-F12024-J1 等待五态（成功/超时/alerted/APC/废弃）在注入用例 10/10 次返回正确码，与 B15 等待族判据同源 |
| UNX-F12025 | STATUS_ACCESS_DENIED 访问拒绝族与安全联签预告 | 280 | 已深化 | UNX-F12025-J1 访问拒绝触发路径（句柄权限/对象 DACL）分类正确，双机注入同拒绝场景返回同码 10/10 次 |
| UNX-F12026 | STATUS_INVALID_HANDLE 句柄错误族与句柄账联签 | 280 | 已深化 | UNX-F12026-J1 伪造/已关/越界/权限掩码错四类坏句柄 10/10 次区分正确，与 D3 句柄表联签接口零偏移 |
| UNX-F12027 | STATUS_INVALID_PARAMETER 参数校验错误族 | 260 | 已深化 | UNX-F12027-J1 参数校验失败注入（NULL/越界/错枚举）三类 10/10 次正确分类，错误定位含参数序号 |
| UNX-F12028 | STATUS_ACCESS_VIOLATION 与异常状态码族（SEH 联动） | 300 | 已深化 | UNX-F12028-J1 异常族码（ACCESS_VIOLATION/IN_PAGE_ERROR/STACK_OVERFLOW/ILLEGAL_INSTRUCTION）与 B08 异常分发联动映射一致，读/写违约参数位双机一致 |
| UNX-F12029 | STATUS_OBJECT_* 对象管理错误族（D3 联签预告） | 260 | 已深化 | UNX-F12029-J1 OBJECT_PATH_NOT_FOUND/NAME_COLLISION/TYPE_MISMATCH 等 8 码语义档齐，与 D3 名字解析错误映射零偏移 |
| UNX-F12030 | STATUS_FILE_* 文件错误族预告（D2/B1 联签） | 260 | 已深化 | UNX-F12030-J1 FILE_IS_A_DIRECTORY/END_OF_FILE/LOCK_CONFLICT 等 10 码语义档齐，与 B14 文件族消费零偏移 |
| UNX-F12031 | STATUS_MEMORY_* 内存错误族（A4 联签） | 260 | 已深化 | UNX-F12031-J1 COMMITMENT_LIMIT/NOT_COMMITTED/WORKING_SET_QUOTA 等码与 A4 页管理错误面映射一致 |
| UNX-F12032 | STATUS_INSUFFICIENT_RESOURCES 资源耗尽语义 | 240 | 已深化 | UNX-F12032-J1 资源池耗尽注入（句柄池/等待块池）10/10 次返回正确码且不崩内核 |
| UNX-F12033 | 未实现三档：NOT_IMPLEMENTED/UNSUPPORTED/INVALID_DEVICE_REQUEST | 240 | 已深化 | UNX-F12033-J1 三档语义区分判据绿（未实现/不支持/请求错对象），Varix 全域未实现类返回码审计零静默 |
| UNX-F12034 | 缓冲族：BUFFER_OVERFLOW/BUFFER_TOO_SMALL/PARTIAL_COPY | 260 | 已深化 | UNX-F12034-J1 三码触发条件判据绿，ReturnLength 回填语义（需要多大）双机一致 10/10 组 |
| UNX-F12035 | 取消与废弃族：CANCELLED/ABANDONED/ABANDONED_WAIT_0 | 260 | 已深化 | UNX-F12035-J1 取消注入 10/10 次正确返回，Mutant 废弃等待语义与 B15 联动一致 |
| UNX-F12036 | RtlNtStatusToDosError 映射表底座 | 300 | 已深化 | UNX-F12036-J1 映射表 ≥100 对（NTSTATUS→Win32 错误码），抽样 30 对与 Windows 真机 GetLastError 读回一致 |
| UNX-F12037 | 状态码文本化与诊断输出面 | 260 | 已深化 | UNX-F12037-J1 ≥100 常用码文本化输出（三要素：含义/原因/下一步），未知码显式"未文本化"非静默 |
| UNX-F12038 | 客户码位与保留区间账（Facility 全表） | 260 | 已深化 | UNX-F12038-J1 Facility 全表（≥20 项）逐项注出处，保留区间写入 10/10 次被校验器拒止 |
| UNX-F12039 | NTSTATUS 对照表制度：语义档三件套母版 | 340 | 已深化 | UNX-F12039-J1 对照表结构（api_name/category/semver_anchor/status_matrix/judge_ids）立母版，本批 18 码族逐条入表可复测 |
| UNX-F12040 | ktest NTSTATUS 面断言集（B02 批判据聚合） | 320 | 已深化 | UNX-F12040-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，位域切分断言族独立可单跑 |

<!-- 主册行 15265 · #### UNX-D1-B03 · PEB 深化（F12041–F12060 · 20 条） -->
#### UNX-D1-B03 · PEB 深化（F12041–F12060 · 20 条）

> AI-16 承办｜批主题：PEB 逐字段深化——偏移总账/调试位/ApiSet/TLS/版本字段/写路径白名单/静态断言器/对照表制度｜域账累计：11,680 + 本批 5,480 = 17,160 / 240,000｜嫁接源：纯自研域；偏移数值锚=Windows Internals 7th Part1 ch.5 与公开 PEB 布局资料（版本锚 ADR-UNX-008 波 07 前钉定 Win10/11 主流面），ReactOS 对照不抄｜防重：F12041–F12060 唯一；与 F12005（B01 PEB 实装）为分层深化——F12005 立结构与快照协议，本批立逐字段语义档与防御面，判据无重叠｜防幻觉高压线：每个偏移注出处章节号，查无资料标"待基准机实测"。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12041 | PEB 字段总账：x64 逐字段偏移表与版本分档 | 340 | 已深化 | UNX-F12041-J1 x64 PEB 全字段偏移表（BeingDebugged 0x2 起至 GdiHandleBuffer）逐字段注出处，Win10/11 两档差异列分列，抽样 20 字段双机偏移零差异 |
| UNX-F12042 | ImageBaseAddress 语义与 D2 装载交接挂点 | 260 | 已深化 | UNX-F12042-J1 映像基址写入时机与 D2 装载握手协议一致，读回值与实际装载基址逐字节一致 50/50 次 |
| UNX-F12043 | ProcessHeap 指针族与默认堆挂点（D2 联签） | 280 | 已深化 | UNX-F12043-J1 ProcessHeap 指向 D2 默认堆实例，堆标志（Flags/ForceFlags）读回与堆初始化参数一致 10/10 组 |
| UNX-F12044 | BeingDebugged/NtGlobalFlag 调试位语义与唯一写者 | 280 | 已深化 | UNX-F12044-J1 调试器挂接/摘除时 BeingDebugged 翻转可观测，NtGlobalFlag 位集与调试选项映射一致，用户态写 10/10 次被拒 |
| UNX-F12045 | ApiSetMap 指针语义与 API Set 解析预告 | 300 | 已深化 | UNX-F12045-J1 ApiSetMap 指针落位与契约解析器对接判据绿，同机 Windows api-ms-* 重定向抽样 10 例行为一致（解析主体归 D2 防重） |
| UNX-F12046 | PEB TlsSlot 数组与 TlsExpansionBitmap | 260 | 已深化 | UNX-F12046-J1 TlsSlot[64] 与扩展位图分配/回收守恒（分配位-回收位差为零），64 槽耗尽路径返回正确错误非静默 |
| UNX-F12047 | PEB LoaderLock 与 LdrLockLoaderLock 占位语义 | 260 | 已深化 | UNX-F12047-J1 LoaderLock 结构挂点落位，锁语义判据（重入计数/所有者线程）与版本锚一致，死锁注入走 A5 检测器联签 |
| UNX-F12048 | PEB 版本字段账：OsMajorVersion/OsMinorVersion/OsBuildNumber | 260 | 已深化 | UNX-F12048-J1 版本三元组与版本锚钉定档一致，双机同版本读回逐值一致，版本注入（伪装低版本）10/10 次正确反映 |
| UNX-F12049 | PEB CSDVersion 与 NTDDI 版本对照 | 220 | 已深化 | UNX-F12049-J1 CSDVersion UNICODE_STRING 语义档齐，服务包位语义与版本锚一致（Win10+ 恒空档有账） |
| UNX-F12050 | PEB FastPebLock 语义 | 240 | 已深化 | UNX-F12050-J1 FastPebLock 争用路径（快锁自旋）语义档齐，双线程争用压测零竞态（M3 母版） |
| UNX-F12051 | PEB GdiHandleBuffer 与 GDI 联签预告（F 部） | 240 | 已深化 | UNX-F12051-J1 GdiHandleBuffer 槽位数与版本锚一致，GDI 句柄计数联签接口冻结（消费方 F 部） |
| UNX-F12052 | PEB ActivationContextData 指针占位语义 | 240 | 已深化 | UNX-F12052-J1 激活上下文指针族（ActivationContextData/ProcessAssemblyStorageMap/SystemAssemblyStorageMap）占位语义档齐，空值路径有账非静默 |
| UNX-F12053 | PEB AtlThunkSListPtr 检测面占位 | 220 | 已深化 | UNX-F12053-J1 AtlThunkSListPtr/AtlThunkSListLock 挂点语义档齐，读回缺省值与版本锚一致 |
| UNX-F12054 | PEB 内核写路径白名单：可写字段全集与审计账 | 300 | 已深化 | UNX-F12054-J1 可写字段白名单全集登记（BeingDebugged/NtGlobalFlag/ImageBaseAddress/ProcessParameters/ProcessHeap 等），白名单外内核写 10/10 次红账拦截，审计账逐笔可查 |
| UNX-F12055 | PEB 跨线程读一致性：只读快照与撕裂防御 | 260 | 已深化 | UNX-F12055-J1 8 线程并发读快照页 10^6 次零撕裂（校验和防御），内核写期间读者见一致快照 |
| UNX-F12056 | 8 内联读取 API 双机对照底座 | 320 | 已深化 | UNX-F12056-J1 IsDebuggerPresent/GetCurrentProcessId/GetCurrentThreadId/GetCurrentProcess/NtCurrentTeb 等 8 API 同码双机逐值一致（S1 抽样母版） |
| UNX-F12057 | PEB 越界写检测与红账 | 260 | 已深化 | UNX-F12057-J1 快照页前后哨兵注入越界写 10/10 次定位到越界偏移并红账，不崩内核 |
| UNX-F12058 | PEB 布局静态断言器（const 断言集） | 300 | 已深化 | UNX-F12058-J1 编译期 const 断言覆盖全字段偏移（≥30 条），人为改错一偏移编译即失败（断言防手滑） |
| UNX-F12059 | PEB 对照表制度：逐字段语义档与可复测判据 | 320 | 已深化 | UNX-F12059-J1 全字段入对照表（字段名/偏移/版本档/出处/判据 ID 五列齐），抽样 30 字段判据可独立复测 |
| UNX-F12060 | ktest PEB 面断言集（B03 批判据聚合） | 320 | 已深化 | UNX-F12060-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，偏移断言族与写路径白名单族独立可单跑 |

<!-- 主册行 15292 · #### UNX-D1-B04 · TEB 深化（F12061–F12080 · 20 条） -->
#### UNX-D1-B04 · TEB 深化（F12061–F12080 · 20 条）

> AI-16 承办｜批主题：TEB 逐字段深化——NT_TIB 三锚/ClientId/LastError/TLS/WOW32/Win32ThreadInfo/gs 写路径/生命周期/对照表制度｜域账累计：17,160 + 本批 5,420 = 22,580 / 240,000｜嫁接源：纯自研域；偏移锚=公开 TEB 布局资料与 Windows Internals（版本锚 ADR-UNX-008），应用硬编码偏移读取是本批"艺术品级"验收落点（总册 D1 域行原文），ReactOS 对照不抄｜防重：F12061–F12080 唯一；与 F12006（B01 TEB 实装）分层——F12006 立结构与 Self 自指，本批立逐字段语义档与联签挂点｜防幻觉高压线：每偏移注出处，查无资料标"待基准机实测"。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12061 | TEB 字段总账：x64 逐字段偏移与版本分档 | 340 | 已深化 | UNX-F12061-J1 x64 TEB 全字段偏移表（NT_TIB 0x0 起至 GdiReserved）逐字段注出处，Win10/11 分档列分列，抽样 20 字段双机零差异 |
| UNX-F12062 | NT_TIB 语义：StackBase/StackLimit/Self 三锚 | 280 | 已深化 | UNX-F12062-J1 栈界两锚与线程栈实际范围一致（±1 页容差判据），Self 自指 50/50 线程通过，x64 链式 ExceptionList 结构位保留语义档齐 |
| UNX-F12063 | ClientId 语义：UniqueProcess/UniqueThread 双槽 | 260 | 已深化 | UNX-F12063-J1 双槽值与进程/线程台账 ID 逐值一致 50/50，与 GetCurrentProcessId 同源判据绿 |
| UNX-F12064 | LastErrorValue 读写语义与 GetLastError 链 | 280 | 已深化 | UNX-F12064-J1 槽位写后读回逐值一致 10^4 次，Win32 错误码→LastError 链与 RtlNtStatusToDosError 消费判据绿 |
| UNX-F12065 | ThreadLocalStoragePointer：TLS 槽数组挂点 | 280 | 已深化 | UNX-F12065-J1 TLS 扩展数组寻址判据绿（TlsSlots 64 槽 + 扩展指针），分配/回收守恒账零漂移 |
| UNX-F12066 | WOW32Reserved 与 WOW64 联签预告（D2） | 260 | 已深化 | UNX-F12066-J1 WOW32Reserved 槽位语义档齐，WOW64 转换跳板挂点接口冻结（消费方 D2 wow64cpu 链） |
| UNX-F12067 | Win32ThreadInfo 与 D4 消息面联签预告 | 260 | 已深化 | UNX-F12067-J1 Win32ThreadInfo/User32Reserved 占位语义档齐，D4 消息队列宿主挂点接口冻结零偏移 |
| UNX-F12068 | CurrentLocale 与区域语义账 | 240 | 已深化 | UNX-F12068-J1 LCID 槽位读写语义档齐，双机同区域设置读回一致，非法 LCID 注入 10/10 次拒止 |
| UNX-F12069 | ActivationContextStackPointer 与激活上下文栈 | 260 | 已深化 | UNX-F12069-J1 激活上下文栈指针（gs:[0x2c8] 档）语义档齐注出处，压入/弹出守恒判据绿 |
| UNX-F12070 | CountOfOwnedCriticalSections 语义 | 240 | 已深化 | UNX-F12070-J1 与 RTL_CRITICAL_SECTION 进出联动计数守恒（B10 消费），退出进程时计数非零即泄漏红账 |
| UNX-F12071 | TEB gs 段基址写路径：GDT/MSR 链与 A3 联签 | 300 | 已深化 | UNX-F12071-J1 线程切换时 gs 基址原子更新判据绿（wrmsr KERNEL_GS_BASE 路径注出处），跨核迁移线程 TEB 挂点一致性 50/50 |
| UNX-F12072 | TEB 静态布置与线程创建交接（A3/C1 联签） | 280 | 已深化 | UNX-F12072-J1 TEB 页从线程栈预留区布置（不走堆），创建交接前后字段快照零缺失，销毁回收守恒 |
| UNX-F12073 | TEB 越界写检测与只读快照 | 260 | 已深化 | UNX-F12073-J1 调试档哨兵注入越界写 10/10 次定位红账，用户态对保护字段写 10/10 次触发 #PF |
| UNX-F12074 | TEB 布局静态断言器 | 280 | 已深化 | UNX-F12074-J1 编译期 const 断言 ≥25 字段偏移，改错一偏移编译失败（断言防手滑，与 F12058 同型不同表） |
| UNX-F12075 | TEB Version/SpareBytes 保留字段账 | 220 | 已深化 | UNX-F12075-J1 保留字段缺省值与版本锚一致，写入保留区 10/10 次红账（调试档） |
| UNX-F12076 | TxFsContext 字段语义 | 220 | 已深化 | UNX-F12076-J1 TxFsContext（事务上下文）槽位语义档齐注出处，未启用事务路径恒零判据绿 |
| UNX-F12077 | EnvironmentPointer/ArbitraryUserPointer 语义 | 240 | 已深化 | UNX-F12077-J1 两字段语义档齐（NT_TIB.EnvironmentPointer/ArbitraryUserPointer），NtCurrentTeb 直读判据绿 |
| UNX-F12078 | TEB 生命周期：创建/销毁/复用语义链 | 280 | 已深化 | UNX-F12078-J1 创建→挂 gs→活→摘 gs→回收五步全链日志可回放，复用前字段清洗断言零残留 |
| UNX-F12079 | TEB 对照表制度：逐字段语义档与可复测判据 | 320 | 已深化 | UNX-F12079-J1 全字段入对照表五列齐（字段名/偏移/版本档/出处/判据 ID），抽样 30 字段独立可复测 |
| UNX-F12080 | ktest TEB 面断言集（B04 批判据聚合） | 320 | 已深化 | UNX-F12080-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，Self 自指族与偏移断言族独立可单跑 |

<!-- 主册行 15319 · #### UNX-D1-B05 · OBJECT_ATTRIBUTES 与对象协议（F12081–F12100 · 20 -->
#### UNX-D1-B05 · OBJECT_ATTRIBUTES 与对象协议（F12081–F12100 · 20 条）

> AI-16 承办｜批主题：OBJECT_ATTRIBUTES 结构全位语义 + 对象句柄协议与 D3 分界——"D1 管结构与调用协议、D3 管名字与存储"分界线写死（任务书专题四原文）｜域账累计：22,580 + 本批 5,520 = 28,100 / 240,000｜嫁接源：纯自研域；结构锚=wdm.h/ntdef.h 公开头文件与 MSDN OBJECT_ATTRIBUTES 文档（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12081–F12100 唯一；命名空间本体（\??\、\Device\、\BaseNamedObjects\ 目录树）归 D3，本域只到结构语义与句柄抽象为止（分界写进双方任务书并联签）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12081 | OBJECT_ATTRIBUTES 结构语义实装 | 320 | 已深化 | UNX-F12081-J1 六字段布局（Length/RootDirectory/ObjectName/Attributes/SecurityDescriptor/SecurityQualityOfService）偏移与头文件锚一致，sizeof 断言编译期通过 |
| UNX-F12082 | OBJ_CASE_INSENSITIVE 位语义 | 260 | 已深化 | UNX-F12082-J1 大小写不敏感比较路径判据绿（UPCASE 表挂点消费），双机同路径注入 10/10 次行为一致 |
| UNX-F12083 | OBJ_INHERIT 位语义 | 260 | 已深化 | UNX-F12083-J1 继承位对象在子进程创建时被消费判据绿（继承表机制归 D2，此处立位语义防重） |
| UNX-F12084 | OBJ_KERNEL_HANDLE 位语义 | 280 | 已深化 | UNX-F12084-J1 内核句柄位置位后句柄入系统表非进程表，用户态引用该句柄 10/10 次拒止返回 INVALID_HANDLE |
| UNX-F12085 | OBJ_FORCE_ACCESS_CHECK/OPEN_IF/PERMANENT 位族 | 280 | 已深化 | UNX-F12085-J1 三位语义档齐：FORCE_ACCESS_CHECK 绕过内核态豁免判据绿、OPEN_IF 打开已存语义绿、PERMANENT 对象生命周期挂 D3 |
| UNX-F12086 | RootDirectory 相对打开语义 | 280 | 已深化 | UNX-F12086-J1 相对根句柄打开路径判据绿（根句柄+相对名解析归 D3，此处立协议），根句柄失效 10/10 次正确拒止 |
| UNX-F12087 | ObjectName 引用语义与缓冲所有权 | 260 | 已深化 | UNX-F12087-J1 ObjectName 为 UNICODE_STRING 引用非拷贝判据绿，调用后源缓冲失活路径 10/10 次正确拒止悬垂 |
| UNX-F12088 | SecurityDescriptor 字段与 J1 安全挂点 | 260 | 已深化 | UNX-F12088-J1 SD 指针挂点结构落位，访问检查走 J1 域接口分界判据绿（策略实现归 J 部防重） |
| UNX-F12089 | SecurityQualityOfService 字段语义 | 240 | 已深化 | UNX-F12089-J1 SQOS 结构指针语义档齐（模拟档/上下文跟踪位），NULL 缺省路径有账非静默 |
| UNX-F12090 | InitializeObjectAttributes 宏语义与静态校验 | 240 | 已深化 | UNX-F12090-J1 初始化宏产出与手工结构逐字段一致，Length 字段恒 sizeof 校验 10/10 次 |
| UNX-F12091 | 内核句柄表对接接口冻结（D3 联签） | 320 | 已深化 | UNX-F12091-J1 句柄表四接口（插入/查询/复制/删除）签名冻结，D3 消费方对接判据绿（表实现归 D3 防重） |
| UNX-F12092 | HANDLE_TABLE_ENTRY 语义预告与权限掩码位 | 280 | 已深化 | UNX-F12092-J1 表项低位权限掩码约定语义档齐注出处，GrantedAccess 位提取判据绿 |
| UNX-F12093 | OBJECT_TYPE_INITIALIZER 类型对象语义预告 | 300 | 已深化 | UNX-F12093-J1 类型对象方法表（open/close/parse/delete）挂点语义档齐，方法缺失路径显式记账非静默 |
| UNX-F12094 | OBJECT_ATTRIBUTES 非法组合拒止矩阵 | 280 | 已深化 | UNX-F12094-J1 非法组合（Length 错/Object NULL+Root 设/KernelHandle+Inherit 冲突类）≥10 例 10/10 次正确拒止并落矩阵账 |
| UNX-F12095 | OBJECT_ATTRIBUTES 版本锚分档账 | 240 | 已深化 | UNX-F12095-J1 结构跨版本档（XP→11）差异列登记，锚变全表回归判据绿（F12019 同型机制） |
| UNX-F12096 | DuplicateObject 语义预告（D3 联签） | 260 | 已深化 | UNX-F12096-J1 复制协议参数语义（源/目标进程/DUPLICATE_SAME_ACCESS）档齐，实现归 D3 分界判据绿 |
| UNX-F12097 | "D1 管结构协议、D3 管名字存储"分界判据与联签文本 | 260 | 已深化 | UNX-F12097-J1 分界联签文本在册（双方任务书互引），跨界条目注入 10/10 次被域守卫判据拦截（越权=缺陷） |
| UNX-F12098 | OBJECT_ATTRIBUTES 对照表制度 | 300 | 已深化 | UNX-F12098-J1 结构/位/协议三表齐入对照表制度，抽样 20 行独立可复测 |
| UNX-F12099 | ktest 对象协议面断言集（B05 批判据聚合） | 320 | 已深化 | UNX-F12099-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，位语义族与拒止矩阵族独立可单跑 |
| UNX-F12100 | B05 批域内集成账与联签清单 | 280 | 已深化 | UNX-F12100-J1 批内 19 条互引集成账零悬空引用，联签清单（D3 两对/D2 一对/J1 一对）逐对登记状态 |

<!-- 主册行 15346 · #### UNX-D1-B06 · UNICODE_STRING 与 RTL 字符串族（F12101–F12120 ·  -->
#### UNX-D1-B06 · UNICODE_STRING 与 RTL 字符串族（F12101–F12120 · 20 条）

> AI-16 承办｜批主题：UNICODE_STRING 三元组约定与 RTL 字符串族全语义——Length 不含 NUL、MaximumLength 含 NUL 的约定判据是海量 Win 代码兼容的暗礁（任务书专题五原文）｜域账累计：28,100 + 本批 5,460 = 33,560 / 240,000｜嫁接源：纯自研域；语义锚=MSDN UNICODE_STRING/Rtl*String 文档与 wdm.h 头文件（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12101–F12120 唯一；与 B10 CRITICAL_SECTION、B12 起对象管理族无重叠——字符串族是 M 型机制批的公共底座。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12101 | UNICODE_STRING 结构语义实装 | 300 | 已深化 | UNX-F12101-J1 三字段（Length/MaximumLength/Buffer）布局与 wdm.h 锚一致，sizeof/对齐断言编译期通过 |
| UNX-F12102 | Length/MaximumLength/NUL 约定判据 | 300 | 已深化 | UNX-F12102-J1 "Length 不含 NUL、MaximumLength 含 NUL"注入 50 样本串（含空串/恰满/越界）逐条判定 100% 正确，双机同码零偏差 |
| UNX-F12103 | RtlInitUnicodeString 语义 | 260 | 已深化 | UNX-F12103-J1 初始化后 Length=strlen*2、MaximumLength=strlen*2+2 判据绿，NULL 源串语义（Length 双零）与锚一致 |
| UNX-F12104 | RtlInitAnsiString/OEM_STRING 语义 | 240 | 已深化 | UNX-F12104-J1 ANSI/OEM 双串结构初始化判据绿（字节长约定），三串类型 sizeof 断言齐 |
| UNX-F12105 | RtlCopyUnicodeString 语义 | 260 | 已深化 | UNX-F12105-J1 目标容量不足截断语义（Length 截到容内）与锚一致，自拷贝（源=目标）10/10 次安全 |
| UNX-F12106 | RtlAppendUnicodeStringToString/ToString 语义 | 260 | 已深化 | UNX-F12106-J1 追加越容量返回 STATUS_BUFFER_TOO_SMALL 且目标不损坏 10/10 次，追加后 Length 守恒 |
| UNX-F12107 | RtlUnicodeStringCopy/Cat 安全族预检语义 | 280 | 已深化 | UNX-F12107-J1 安全族（RtlUnicodeStringCopy/CopyString/Cat）预检失败路径不触碰目标 10/10 次，与截断族语义区分判据绿 |
| UNX-F12108 | RtlAnsiStringToUnicodeString 转换语义 | 280 | 已深化 | UNX-F12108-J1 AllocateDestinationConsole 双档语义判据绿，扩展字符（>0x7F）转换表与 Windows 对照 30 组零偏差 |
| UNX-F12109 | RtlUpcaseUnicodeString 大小写转换 | 260 | 已深化 | UNX-F12109-J1 UPCASE 表消费判据绿，NORM_FORM 类边界（简单转换档）抽样 50 字符与 Windows 一致 |
| UNX-F12110 | RtlEqualUnicodeString/RtlCompareUnicodeString 比较族 | 280 | 已深化 | UNX-F12110-J1 CaseInsensitive 参数双档判据绿，前缀/等长/不等长三类注入 30 组与 Windows 一致 |
| UNX-F12111 | RtlPrefixUnicodeString 前缀判定 | 240 | 已深化 | UNX-F12111-J1 前缀判定（含 InheritCaseIgnored 档）30 组样本与 Windows 一致，空串前缀语义档齐 |
| UNX-F12112 | RtlIntegerToUnicodeString/整数双向转换族 | 260 | 已深化 | UNX-F12112-J1 进制参数（10/16）双向转换 10^4 随机值往返零损，溢出路径拒止正确 |
| UNX-F12113 | RtlStringCbPrintfW 类安全格式化预告 | 280 | 已深化 | UNX-F12113-J1 安全格式化预检语义档齐（容量计算/截断报告），与 C 运行时 swprintf 语义分界判据绿（CRT 归 E2 防重） |
| UNX-F12114 | 缓冲所有权模型与 PagedPool 分配挂点 | 280 | 已深化 | UNX-F12114-J1 AllocateDestination 档分配/释放配对守恒，悬垂释放注入 10/10 次被哨兵拦截 |
| UNX-F12115 | 越界与截断防御矩阵 | 280 | 已深化 | UNX-F12115-J1 全族函数越界注入矩阵（每函数 ≥3 例）10/10 次拒止不崩，调试档哨兵定位到函数名 |
| UNX-F12116 | 多编码转换账：UTF-16/UTF-8/ANSI 面预告 | 280 | 已深化 | UNX-F12116-J1 编码转换接口账登记（RtlUnicodeToUTF8 类），代理对往返零损抽样 30 字符判据绿 |
| UNX-F12117 | UPCASE 表接口与不敏感哈希挂点 | 260 | 已深化 | UNX-F12117-J1 UPCASE 表接口冻结（B05 OBJ_CASE_INSENSITIVE 消费），哈希挂点判据绿（D3 命名哈希消费预告） |
| UNX-F12118 | UNICODE_STRING 对照表制度与样本集 | 280 | 已深化 | UNX-F12118-J1 全族函数入对照表（函数/语义/错误矩阵/判据 ID 四列），50 样本串集入册可复测 |
| UNX-F12119 | ktest 字符串族断言集（B06 批判据聚合） | 320 | 已深化 | UNX-F12119-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，约定判据族与防御矩阵族独立可单跑 |
| UNX-F12120 | B06 批域内集成账（含 RtlAllocateHeap 对接预告） | 260 | 已深化 | UNX-F12120-J1 批内互引集成账零悬空，堆挂点接口（RtlAllocateHeap 对接 D2 堆语义）预告登记联签状态 |

<!-- 主册行 15373 · #### UNX-D1-B07 · .pdata/RUNTIME_FUNCTION 与展开数据地基（F12121–F12 -->
#### UNX-D1-B07 · .pdata/RUNTIME_FUNCTION 与展开数据地基（F12121–F12140 · 20 条）

> AI-16 承办｜批主题：x64 表式异常展开的数据地基——PE 异常目录/RUNTIME_FUNCTION/UNWIND_INFO/展开码全型/样本库制度（判据主轴"SEH 陷阱帧真机可验"的数据层）｜域账累计：33,560 + 本批 5,780 = 39,340 / 240,000｜嫁接源：纯自研域；格式锚=x86-64 SysV 与 Win64 ABI 展开码公开规范（MSDN "Exception Handling (x64)" 与 PE 格式文档，版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12121–F12140 唯一；与 D2 PE 装载分界——D2 管装载与节映射，本域管异常目录消费语义（联签 F12138）｜防幻觉高压线：展开码编码值一律注 MSDN 章节号，查无资料标"待基准机实测"。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12121 | PE 异常目录全景：.pdata/.xdata 定位与加载 | 320 | 已深化 | UNX-F12121-J1 异常目录（IMAGE_DIRECTORY_ENTRY_EXCEPTION）定位与节界校验判据绿，100 模块样本目录起止与 dumpbin /headers 一致 |
| UNX-F12122 | RUNTIME_FUNCTION 三元组语义：Begin/End/UnwindData | 280 | 已深化 | UNX-F12122-J1 三字段布局（3×DWORD）判据绿，Begin/End 函数界与反汇编抽样 50 函数一致 |
| UNX-F12123 | RIP 二分查找算法与边界语义 | 300 | 已深化 | UNX-F12123-J1 二分查找（表须按 BeginAddress 排序）命中/未命中/恰边界三类 10^4 查询与线性扫参照零偏差，损坏序表拒止 |
| UNX-F12124 | UNWIND_INFO 头部语义：版本/标志/序言/槽位 | 300 | 已深化 | UNX-F12124-J1 头部字节布局（Version 3b/Flags 5b/SizeOfProlog/CountOfCodes/FrameRegister+FrameOffset）判据绿，Version≠1/2 拒止 |
| UNX-F12125 | UNW_FLAG_NHANDLER/EHANDLER/UHANDLER/CHAININFO 标志族 | 280 | 已深化 | UNX-F12125-J1 四标志解码判据绿，E/U 双标志并存（C++ 链）注入 10/10 次正确分派 |
| UNX-F12126 | UWOP_PUSH_NONVOL/PUSH_MACHFRAME 展开码语义 | 300 | 已深化 | UNX-F12126-J1 两码栈回退效果逐码断言（RSP 调整量）与锚一致，PUSH_MACHFRAME 完整/错误码两档判据绿 |
| UNX-F12127 | UWOP_ALLOC_SMALL/ALLOC_LARGE 栈分配语义 | 280 | 已深化 | UNX-F12127-J1 两码 RSP 调整量（8–128 / 16 位×8 扩展）判据绿，边界值样本逐条与锚一致 |
| UNX-F12128 | UWOP_SAVE_NONVOL/SAVE_NONVOL_FAR 语义 | 280 | 已深化 | UNX-F12128-J1 寄存器保存槽定位（UOFF 槽位/扩展偏移）判据绿，恢复值与进入值逐寄存器一致 |
| UNX-F12129 | UWOP_SAVE_XMM128/SAVE_XMM128_FAR 语义 | 280 | 已深化 | UNX-F12129-J1 XMM 保存（16 字节对齐约束）判据绿，未对齐注入 10/10 次红账，恢复值逐位一致 |
| UNX-F12130 | UWOP_SAVE_REGS_FRAME/SAVE_XMM128_FRAME 大帧语义 | 280 | 已深化 | UNX-F12130-J1 大帧双码（FrameRegister 基准寻址）判据绿，FrameOffset 编码 16 步进语义与锚一致 |
| UNX-F12131 | UWOP_CHAINED_INFO 链式展开语义 | 280 | 已深化 | UNX-F12131-J1 链式（共享序言变体）跳转后继续解释判据绿，环链注入 10/10 次深度守卫拦截 |
| UNX-F12132 | 展开码解释器主体：逐码栈模拟 | 340 | 已深化 | UNX-F12132-J1 解释器对全部展开码（UWOP_END 0–11 全型）栈模拟正确，MSVC/MinGW 200 函数样本回放零偏差（难一主战场） |
| UNX-F12133 | 损坏 .pdata/.xdata 拒止与红账 | 280 | 已深化 | UNX-F12133-J1 坏表注入（截断/越界 UnwindData/坏版本/环链）≥8 类 10/10 次拒止不崩内核，红账定位到损坏类型 |
| UNX-F12134 | 真实编译产物样本库制度（MSVC/MinGW 各钉一版） | 300 | 已深化 | UNX-F12134-J1 样本库制度在册（版本钉定/来源审计/禁 Wine 生成样本），首版 200 函数样本入库（难一解法原文） |
| UNX-F12135 | 样本集 200 函数覆盖矩阵与回放面 | 300 | 已深化 | UNX-F12135-J1 覆盖矩阵（全展开码型×嵌套深度×帧寄存器）零空格，回放面一次命令全跑判据 100% |
| UNX-F12136 | 叶函数（无 .pdata 条目）行为语义 | 260 | 已深化 | UNX-F12136-J1 叶函数异常时 RSP 即返回址语义判据绿，无表函数与叶函数区分（表查不到≠叶函数的假叶函数陷阱）10/10 次正确 |
| UNX-F12137 | .xdata 压缩与对齐规则 | 260 | 已深化 | UNX-F12137-J1 内联 xdata（UnwindData 位0 标志）与外置双轨判据绿，四字节对齐要求注入验证 |
| UNX-F12138 | .pdata 对照表制度与异常目录交付协议（D2 联签） | 280 | 已深化 | UNX-F12138-J1 异常目录交付协议冻结（D2 装载→本域消费），联签状态登记，对照表抽样 20 行独立可复测 |
| UNX-F12139 | ktest .pdata 面断言集（B07 批判据聚合） | 320 | 已深化 | UNX-F12139-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，展开码族与拒止族独立可单跑 |
| UNX-F12140 | B07 批域内集成账 | 260 | 已深化 | UNX-F12140-J1 批内互引集成账零悬空引用，样本库/回放面/拒止矩阵三件交付状态登记 |

<!-- 主册行 15400 · #### UNX-D1-B08 · UNWIND_INFO 解释器与异常分发（F12141–F12160 · 20 条） -->
#### UNX-D1-B08 · UNWIND_INFO 解释器与异常分发（F12141–F12160 · 20 条）

> AI-16 承办｜批主题：SEH 机制核心——RtlDispatchException/逐帧回退/scope table/C 链 C++ 链/RtlUnwindEx 两阶段/VEH 与 UEF 次序（判据主轴"真机可验"的机制层，域最难单点集中批）｜域账累计：39,340 + 本批 6,140 = 45,480 / 240,000｜嫁接源：纯自研域；语义锚=MSDN "Exception Handling (x64)" 与 Windows Internals ch.4（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12141–F12160 唯一；与 B07 分层——B07 立数据层，本批立分发与展开机制层；B11 立全链收口与压测｜行为红线 #1 高压线：handler 命中序列任何"应该差不多"即虚报，一律基准机录制对照。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12141 | RtlDispatchException 主体语义 | 380 | 已深化 | UNX-F12141-J1 分发主循环（查表→回退→找 handler）真机注入三类异常命中序列与基准机逐帧一致（S3 录制对照） |
| UNX-F12142 | 异常帧 CONTEXT 采集与 RIP 定位 | 300 | 已深化 | UNX-F12142-J1 异常时 CONTEXT 快照（RIP/RSP 全寄存器）与硬件现场逐寄存器一致，RIP 定位查表命中 10/10 次 |
| UNX-F12143 | 逐帧回退栈模拟器（RtlVirtualUnwind 底座） | 360 | 已深化 | UNX-F12143-J1 回退后 RSP/RIP/非易失寄存器与基准机逐帧一致，200 样本函数全回放零偏差 |
| UNX-F12144 | scope table 语义与 __try 作用域映射 | 320 | 已深化 | UNX-F12144-J1 scope table（TryLow/TryHigh/Filter/Handler 四列）解析判据绿，RIP 落 try 区间映射 10/10 次正确 |
| UNX-F12145 | __C_specific_handler 分发表仿真（C 链） | 320 | 已深化 | UNX-F12145-J1 C 链异常映射（ExceptionIndex→scope 表）与基准机一致，异常码过滤命中/未命中 10/10 次正确 |
| UNX-F12146 | filter/except/finally 三型执行序语义 | 300 | 已深化 | UNX-F12146-J1 三型执行序（filter 求值→except 体→finally 清理）逐帧日志与基准机一致，filter 返回三值语义档齐 |
| UNX-F12147 | __CxxFrameHandler 挂点协议（C++ 链预告） | 300 | 已深化 | UNX-F12147-J1 C++ 链挂点接口冻结（EH 结构指针传递），类型匹配主体归 E2 联合域分界判据绿 |
| UNX-F12148 | RtlUnwindEx 两阶段展开语义 | 420 | 已深化 | UNX-F12148-J1 两阶段（搜索/展开）次序可观测，嵌套 try/finally ≥5 层展开次序与基准机一致（示例条目判据原文落地） |
| UNX-F12149 | 展开中再异常（nested exception）语义 | 300 | 已深化 | UNX-F12149-J1 展开路径触发新异常注入 10/10 次按嵌套语义收敛（STATUS_NESTED_EXCEPTION 链），栈不损毁 |
| UNX-F12150 | RtlAddFunctionTable/RtlInstallFunctionTableCallback 注册面 | 280 | 已深化 | UNX-F12150-J1 动态注册函数表三接口判据绿，注册表与静态表查找合并序一致 |
| UNX-F12151 | 动态函数表（DynamicFunctionTable）语义 | 280 | 已深化 | UNX-F12151-J1 动态表（JIT 场景）增删查判据绿，与 D2 模块卸载联动表回收守恒 |
| UNX-F12152 | 向量异常处理 VEH 语义预告 | 300 | 已深化 | UNX-F12152-J1 VEH 链（Add/RemoveVectoredExceptionHandler）先于 SEH 分发判据绿，链序 10/10 次正确 |
| UNX-F12153 | 异常分发次序账：VEH→SEH→UEF 终局 | 300 | 已深化 | UNX-F12153-J1 全链次序（VEH 链→帧链→未处理过滤器→默认终局）注入 10 场景逐级可观测且与基准机一致 |
| UNX-F12154 | UnhandledExceptionFilter 与 WER 联签预告 | 280 | 已深化 | UNX-F12154-J1 UEF 挂点与 SetUnhandledExceptionFilter 语义档齐，WER 报告主体归 K 部分界判据绿 |
| UNX-F12155 | 展开路径锁序巡检对接（A5 死锁检测器联签） | 260 | 已深化 | UNX-F12155-J1 展开路径持锁清单登记入 A5 锁序账，注入锁序违例 10/10 次被检测器捕获 |
| UNX-F12156 | 展开性能账：单次展开耗时预算 | 260 | 已深化 | UNX-F12156-J1 单次展开 P95 ≤50μs（200 样本实测口径在册），超预算帧标红可定位 |
| UNX-F12157 | 异常分发可观测性：命中序列日志 | 280 | 已深化 | UNX-F12157-J1 命中序列（每帧 handler 地址/结果）结构化日志可导出，与 S3 录制 diff 工具链判据绿 |
| UNX-F12158 | 异常分发对照表制度 | 300 | 已深化 | UNX-F12158-J1 机制层对照表（接口/语义/错误路径/判据 ID 四列）建齐，抽样 20 行独立可复测 |
| UNX-F12159 | ktest 异常分发面断言集（B08 批判据聚合） | 320 | 已深化 | UNX-F12159-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，分发次序族与性能账族独立可单跑 |
| UNX-F12160 | B08 批域内集成账与真机三类异常注入预告 | 280 | 已深化 | UNX-F12160-J1 批内互引零悬空；除零/非法访问/软件断言三类注入 ×N 次真机判据登记"随闸门补测"（双轨产线纪律） |

<!-- 主册行 15427 · #### UNX-D1-B09 · CONTEXT 陷阱帧与寄存器语义（F12161–F12180 · 20 条） -->
#### UNX-D1-B09 · CONTEXT 陷阱帧与寄存器语义（F12161–F12180 · 20 条）

> AI-16 承办｜批主题：CONTEXT 结构全寄存器语义——ContextFlags/陷阱帧转换/RtlCaptureContext/RestoreContext/段与 EFlags/诊断导出（SEH 真机可验的寄存器层）｜域账累计：45,480 + 本批 5,640 = 51,120 / 240,000｜嫁接源：纯自研域；结构锚=winnt.h CONTEXT 定义与 MSDn CONTEXT 文档（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12161–F12180 唯一；与 A3 调度器分界——A3 管线程上下文切换本体，本域管 NT 语义的 CONTEXT 结构与异常/调试消费面（F12175 分界判据）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12161 | CONTEXT 结构语义实装（x64 全寄存器集） | 340 | 已深化 | UNX-F12161-J1 CONTEXT 全字段布局（P1Home–P6Home/Rax–R15/Rip/SegXxx/EFlags/浮点位集/XMM0–15）与 winnt.h 锚一致，sizeof/16 对齐断言编译期通过 |
| UNX-F12162 | ContextFlags 位语义与部分加载/保存 | 300 | 已深化 | UNX-F12162-J1 标志位（CONTROL/INTEGER/FLOATING_POINT/DEBUG_REGISTERS/XMM）选择性加载/保存判据绿，未请求区零触碰 10/10 次 |
| UNX-F12163 | 陷阱帧与用户 CONTEXT 转换语义 | 300 | 已深化 | UNX-F12163-J1 内核陷阱帧→CONTEXT 双向转换逐寄存器一致 50/50 次，转换损耗区（SS:RSP 用户/内核档）语义档齐 |
| UNX-F12164 | RtlCaptureContext 语义 | 260 | 已深化 | UNX-F12164-J1 自捕获值与外部观测（调试寄存器参照）逐寄存器一致，连续两次捕获间差异仅指令指针 |
| UNX-F12165 | RtlRestoreContext 与恢复流程对接 | 300 | 已深化 | UNX-F12165-J1 恢复后寄存器现场逐值一致（含 XMM），与 RtlUnwindEx 终局路径对接判据绿 |
| UNX-F12166 | RtlRaiseException/RtlRaiseStatus 语义 | 280 | 已深化 | UNX-F12166-J1 软件注入异常路径与硬件异常分发同链（命中序列一致），ExceptionRecord 传递零损 |
| UNX-F12167 | XMM 寄存器保存语义与对齐 | 280 | 已深化 | UNX-F12167-J1 XMM0–15 保存/恢复逐位一致（含 NaN 载荷位），CONTEXT 区 16 字节对齐违规注入 10/10 次拒止 |
| UNX-F12168 | 调试/MMX/控制寄存器槽位语义 | 260 | 已深化 | UNX-F12168-J1 Dr0–Dr7/Cr0 控制位槽位语义档齐，DEBUG_REGISTERS 标志未置时不触碰判据绿 |
| UNX-F12169 | 段寄存器语义：cs/ss/ds/es/gs/fs | 260 | 已深化 | UNX-F12169-J1 x64 段寄存器档（cs/ss 恒值、gs 基址活、fs 遗留档）语义档齐注出处，双机同码逐值一致 |
| UNX-F12170 | EFlags 语义与标志位账 | 260 | 已深化 | UNX-F12170-J1 标志位账（CF/PF/AF/ZF/SF/OF/DF/TF/IF/NT/RF/VM）逐位语义档齐，TF 陷阱位与调试联签预告 |
| UNX-F12171 | CONTEXT 对齐与异常帧内存布局 | 260 | 已深化 | UNX-F12171-J1 异常帧布局（CONTEXT+EXCEPTION_RECORD 联合布置）判据绿，栈耗尽路径 STACK_OVERFLOW 档正确落账 |
| UNX-F12172 | NtContinue 语义预告 | 300 | 已深化 | UNX-F12172-J1 ContinueExecution 参数双档语义档齐，恢复执行点与传入 CONTEXT.Rip 一致 50/50 次（实现主体归系统调用层，此处立语义） |
| UNX-F12173 | NtGetContextThread/NtSetContextThread 语义预告 | 280 | 已深化 | UNX-F12173-J1 双接口 ContextFlags 协商语义档齐，目标线程挂起要求（非挂起拒止）判据绿 |
| UNX-F12174 | CONTEXT 校验器：非法标志组合拒止 | 260 | 已深化 | UNX-F12174-J1 非法组合（保留位置位/标志含未请求区）注入 10/10 次拒止返回 INVALID_PARAMETER |
| UNX-F12175 | 上下文切换语义边界（与 A3 调度器分界） | 280 | 已深化 | UNX-F12175-J1 分界判据绿：A3 管切换本体与性能，D1 管 NT CONTEXT 语义与消费面；跨界条目注入被域守卫拦截 |
| UNX-F12176 | CONTEXT 对照表制度 | 280 | 已深化 | UNX-F12176-J1 全字段入对照表五列齐（字段/偏移/版本档/出处/判据 ID），抽样 30 字段独立可复测 |
| UNX-F12177 | 陷阱帧诊断导出：崩溃现场快照 | 280 | 已深化 | UNX-F12177-J1 崩溃现场 CONTEXT+调用链回溯（借助 B07 展开器）导出判据绿，快照与 F0011 日志环合流 |
| UNX-F12178 | 信号/中断/异常三源上下文统一判据 | 280 | 已深化 | UNX-F12178-J1 三源（C1 信号/硬件中断/NT 异常）进入统一 CONTEXT 面判据绿，人格分表语义档齐 |
| UNX-F12179 | ktest CONTEXT 面断言集（B09 批判据聚合） | 320 | 已深化 | UNX-F12179-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，标志协商族与转换族独立可单跑 |
| UNX-F12180 | B09 批域内集成账 | 260 | 已深化 | UNX-F12180-J1 批内互引集成账零悬空引用，与 A3/B07/B08 三向联签状态登记 |

<!-- 主册行 15454 · #### UNX-D1-B10 · RTL_CRITICAL_SECTION 与同步地基（F12181–F12200 · -->
#### UNX-D1-B10 · RTL_CRITICAL_SECTION 与同步地基（F12181–F12200 · 20 条）

> AI-16 承办｜批主题：RTL_CRITICAL_SECTION 全语义——结构域布局/Enter/Leave 争用路径/DebugInfo/SRWLOCK/条件变量预告（F 型地基批收官，任务书专题五判据：偏移断言+8 线程压测零竞态）｜域账累计：51,120 + 本批 5,740 = 56,860 / 240,000｜嫁接源：纯自研域；结构锚=winnt.h CRITICAL_SECTION 定义与 Windows Internals 同步章（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12181–F12200 唯一；等待原语内核侧（Event/Semaphore）归 B15，本批只立临界区自旋+等待块协议；内核互斥底座归 A3/C1 分层复用。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12181 | RTL_CRITICAL_SECTION 结构语义实装 | 320 | 已深化 | UNX-F12181-J1 六字段布局（DebugInfo/LockCount/RecursionCount/OwningThread/LockSemaphore/SpinCount）偏移与 winnt.h 锚一致，sizeof 断言编译期通过 |
| UNX-F12182 | EnterCriticalSection 语义：自旋与等待路径 | 320 | 已深化 | UNX-F12182-J1 无争用快路径 ≤20 周期档可观测，争用先自旋后等待路径切换 10/10 次正确，进入后 OwningThread 归属一致 |
| UNX-F12183 | LeaveCriticalSection 语义与唤醒协议 | 300 | 已深化 | UNX-F12183-J1 递归递减/归零唤醒两档判据绿，唤醒后等待者获锁次序与基准机一致（先进先出档） |
| UNX-F12184 | RTL_CRITICAL_SECTION_DEBUG 与 DebugInfo | 280 | 已深化 | UNX-F12184-J1 DebugInfo 链（Type/CreatorBackTraceIndex/CriticalSection/ProcessLocksList/EntryCount/ContentionCount）语义档齐，争用计数与实测一致 |
| UNX-F12185 | TryEnterCriticalSection 语义 | 260 | 已深化 | UNX-F12185-J1 忙时立即返回 FALSE 不阻塞判据绿，成功路径与 Enter 后续语义完全一致 |
| UNX-F12186 | 争用路径与等待块语义预告 | 300 | 已深化 | UNX-F12186-J1 等待块（GUID 类 RTL_WAIT_BLOCK 挂点）协议语义档齐，等待队列入队/出队守恒账零漂移 |
| UNX-F12187 | SpinCount 语义与初始化参数 | 260 | 已深化 | UNX-F12187-J1 SpinCount 传参/缺省两档判据绿，高位标志位（RTL_CRITICAL_SECTION_FLAG_NO_DEBUG_INFO 档）语义注出处 |
| UNX-F12188 | RecursionCount/OwningThread 递归语义 | 280 | 已深化 | UNX-F12188-J1 同线程重入递归计数一致，他线程重入注入 10/10 次正确阻塞非误判 |
| UNX-F12189 | LockSemaphore 事件挂点 | 260 | 已深化 | UNX-F12189-J1 惰性创建事件挂点判据绿（首次争用才建），事件句柄生命周期与临界区销毁联动守恒 |
| UNX-F12190 | 进程退出临界区泄漏核账 | 280 | 已深化 | UNX-F12190-J1 退出时 CountOfOwnedCriticalSections 非零即泄漏红账并定位到临界区地址（TEB F12070 消费） |
| UNX-F12191 | RtlInitializeCriticalSection 族语义 | 260 | 已深化 | UNX-F12191-J1 Initialize/InitializeAndSpinCount 双接口初始化后字段快照与锚一致，重复初始化注入 10/10 次红账 |
| UNX-F12192 | RtlDeleteCriticalSection 语义 | 260 | 已深化 | UNX-F12192-J1 删除释放事件与 DebugInfo 资源守恒，使用中删除注入 10/10 次拒止红账 |
| UNX-F12193 | 8 线程争用压测判据（M3 母版） | 320 | 已深化 | UNX-F12193-J1 8 线程×10^5 次进出零竞态（账守恒+哨兵双检），争用计数与线程观测一致（任务书专题五判据原文） |
| UNX-F12194 | 与 A5 死锁检测器联签：锁序账对接 | 280 | 已深化 | UNX-F12194-J1 临界区进出报点入 A5 锁序账，注入 A→B/B→A 交叉持锁 10/10 次被检测器捕获 |
| UNX-F12195 | SRWLOCK 语义预告 | 280 | 已深化 | UNX-F12195-J1 RTL_SRWLOCK 结构（Ptr 单字段）语义档齐注出处，共享/独占双模式语义判据绿 |
| UNX-F12196 | RTL_SRWLOCK Acquire/Release 族语义 | 300 | 已深化 | UNX-F12196-J1 AcquireExclusive/Shared 与 ReleaseExclusive/Shared 四接口互斥/共享语义矩阵 16 组全过，递归获取注入正确阻塞 |
| UNX-F12197 | RTL_CONDITION_VARIABLE 条件变量预告 | 280 | 已深化 | UNX-F12197-J1 SleepConditionVariableCS 与 SRW 档对接语义档齐，唤醒丢失（先 Notify 后 Sleep 经典坑）注入 10/10 次正确处理 |
| UNX-F12198 | 临界区对照表制度 | 280 | 已深化 | UNX-F12198-J1 结构/接口/错误路径三表齐入对照表制度，抽样 20 行独立可复测 |
| UNX-F12199 | ktest 临界区面断言集（B10 批判据聚合） | 320 | 已深化 | UNX-F12199-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，压测族与守恒账族独立可单跑 |
| UNX-F12200 | F 型地基批集成账：B01–B10 十批联签总闸 | 300 | 已深化 | UNX-F12200-J1 十批互引集成账（进程/状态码/PEB/TEB/对象协议/字符串/异常数据/分发/上下文/临界区）零悬空，联签总闸判据一次全跑绿（Top10 接口⑥立起开门件） |

<!-- 主册行 15481 · #### UNX-D1-B11 · SEH 全链机制收口（F12201–F12220 · 20 条） -->
#### UNX-D1-B11 · SEH 全链机制收口（F12201–F12220 · 20 条）

> AI-16 承办｜批主题：M 型首批——SEH 全链机制收口与压测核账（RtlUnwindEx 主体/scope 处理器/C 链全路径/注入矩阵/基准机录制协议）｜域账累计：56,860 + 本批 6,080 = 62,940 / 240,000｜嫁接源：纯自研域；语义锚=MSDN "Exception Handling (x64)" 与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12201–F12220 唯一；与 B08 分层——B08 立分发机制件，本批收口全链与对照压测（任务书 B11–B14 机制批主轴）｜注记：源册示例"UNX-F12101｜RtlUnwindEx｜B11"为规划示意号，按区间恒等（B11=F12201–F12220）落地为 F12201，差异显式登记非静默修正。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12201 | RtlUnwindEx 主体机制 | 420 | 已深化 | UNX-F12201-J1 展开（Unwind）主体对 200 样本回放展开序列与基准机逐帧一致，TargetIp/ReturnValue 传递零损（区间恒等落地条目） |
| UNX-F12202 | 展开码全量解释器回归矩阵 | 320 | 已深化 | UNX-F12202-J1 全展开码型（0–11）×组合深度矩阵零空格，回归一次命令全跑与 B07 样本库共用零偏差 |
| UNX-F12203 | scope table 处理器主体 | 320 | 已深化 | UNX-F12203-J1 scope 查找主体（RIP 区间二分+嵌套序）10/10 场景与基准机一致，多 scope 同函数嵌套注入正确 |
| UNX-F12204 | __C_specific_handler 全路径收口 | 320 | 已深化 | UNX-F12204-J1 C 链全路径（异常过滤→处理→展开交接）与基准机一致，translator 挂点（__CxxDetectCallback 类）预告登记 |
| UNX-F12205 | 嵌套 try/finally ≥5 层展开判据 | 320 | 已深化 | UNX-F12205-J1 5 层嵌套 try/finally 展开次序与基准机一致（任务书判据原文），每层清理序日志可回放 |
| UNX-F12206 | 展开次序对照账（基准机录制比对） | 300 | 已深化 | UNX-F12206-J1 对照账（场景→录制→比对结果三列）≥30 场景齐，diff 零偏差场景占比 100% 才绿 |
| UNX-F12207 | 展开器与 D2 装载器联签：函数表来源统一 | 300 | 已深化 | UNX-F12207-J1 函数表来源（静态目录+动态注册）统一查找序判据绿，D2 模块卸载后表回收守恒 |
| UNX-F12208 | 展开器与 D4 消息面预告：窗口线程异常路径 | 280 | 已深化 | UNX-F12208-J1 消息泵内异常展开路径判据绿（泵帧注册表挂点），窗口线程崩溃不拖垮泵次序有账（D4 联签预告） |
| UNX-F12209 | 异常注入矩阵：三类异常 × 嵌套深度 | 320 | 已深化 | UNX-F12209-J1 注入矩阵（除零/非法访问/断言 × 嵌套 1–5 层）全格跑通命中序列一致（任务书 E 型矩阵判据前置收口） |
| UNX-F12210 | 坏 .pdata 拒止矩阵扩展 | 280 | 已深化 | UNX-F12210-J1 坏表注入扩展 ≥12 类（含展开中损坏）10/10 次拒止红账，不崩内核（任务书 E 型判据前置） |
| UNX-F12211 | 展开器性能预算与压测 | 280 | 已深化 | UNX-F12211-J1 10^5 次注入压测 P95 展开耗时在预算内（与 F12156 口径一致），无内存增长（泄漏账零漂移） |
| UNX-F12212 | 展开器错误路径全集：半初始化栈/展开越界 | 300 | 已深化 | UNX-F12212-J1 半初始化栈帧/展开越过栈界/RIP 非代码段三类注入 10/10 次安全收敛红账 |
| UNX-F12213 | __try/__except/__finally 语言扩展语义映射 | 300 | 已深化 | UNX-F12213-J1 三扩展到机制层映射（SEH 原语→scope 表）判据绿，GetExceptionCode/GetExceptionInformation 语义档齐 |
| UNX-F12214 | 线程终止展开语义预告（RtlExitUserThread 链） | 280 | 已深化 | UNX-F12214-J1 线程终止时 finally 清理语义档齐（是否执行展开的版本差异注出处），终止链日志可回放 |
| UNX-F12215 | APC 展开语义预告（延迟用户回调通道） | 280 | 已深化 | UNX-F12215-J1 APC 投递点与展开交互语义档齐（alertable 等待内异常），与 C1 共用投递通道分表判据绿（总册 APC 域行） |
| UNX-F12216 | 基准机录制协议（S3 录制对照母版） | 280 | 已深化 | UNX-F12216-J1 录制协议（场景脚本→基准机录制→格式归档→比对）母版在册，抽样 5 场景录制-回放往返一致 |
| UNX-F12217 | SEH 压测核账条目 | 280 | 已深化 | UNX-F12217-J1 压测核账（注入次数/命中序列一致率/坏表拒止率/性能 P95）四账齐且全绿，账随批归档 |
| UNX-F12218 | SEH 对照表制度（机制批母版） | 300 | 已深化 | UNX-F12218-J1 SEH 机制对照表母版（接口/语义/错误路径/录制场景/判据 ID 五列）建齐，M 型后续批按此母版复制 |
| UNX-F12219 | ktest SEH 全链断言集（B11 批判据聚合） | 320 | 已深化 | UNX-F12219-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，注入矩阵族与对照账族独立可单跑 |
| UNX-F12220 | 展开器域内集成账与联签清单 | 280 | 已深化 | UNX-F12220-J1 批内互引零悬空；D2/D4/E1 联签状态登记（E1 Wine 启动探测面为波 08 开工前置） |

<!-- 主册行 15508 · #### UNX-D1-B12 · 对象管理族 NT API 语义档（F12221–F12240 · 20 条） -->
#### UNX-D1-B12 · 对象管理族 NT API 语义档（F12221–F12240 · 20 条）

> AI-16 承办｜批主题：M 型对象管理族——NtCreateFile/OpenFile 协议、NtQueryObject/DuplicateObject/Close、Nt/Zw 双前缀与 PreviousMode、对照表首批 30 条入账｜域账累计：62,940 + 本批 5,820 = 68,760 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Native API 文档与 Windows Internals ch.8 对象管理器（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12221–F12240 唯一；与 D3 分界——D3 管名字解析与句柄表实现，本域立调用协议语义档（F12097 分界文本）；文件语义主体归 B14，本批只立对象协议面防重叠。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12221 | NtCreateFile/NtOpenFile 用户态协议语义档 | 340 | 已深化 | UNX-F12221-J1 协议语义档（参数序/OBJECT_ATTRIBUTES 消费/DesiredAccess 传递）与锚一致，Create/Open 分派语义 10/10 次正确（文件语义主体归 B14 防重） |
| UNX-F12222 | NtQueryObject 语义档 | 300 | 已深化 | UNX-F12222-J1 三信息类（ObjectBasicInformation/ObjectTypeInformation/ObjectNameInformation）返回结构布局与锚一致，双机抽样 10 对象逐字段一致 |
| UNX-F12223 | NtDuplicateObject 语义档 | 300 | 已深化 | UNX-F12223-J1 复制语义（源/目标进程/DUPLICATE_SAME_ACCESS/降权掩码）矩阵 8 组全过，句柄计数守恒账零漂移 |
| UNX-F12224 | NtClose 语义档：句柄关闭协议 | 300 | 已深化 | UNX-F12224-J1 关闭协议（引用递减→归零回收）判据绿，双关闭注入 10/10 次二次关闭正确拒止（PROTECT_FROM_CLOSE 位语义档齐） |
| UNX-F12225 | NtSetInformationObject 语义档 | 280 | 已深化 | UNX-F12225-J1 ObjectHandleFlagInformation 类（PROTECT_FROM_CLOSE/继承位）读写判据绿，非法信息类 10/10 次正确拒止 |
| UNX-F12226 | NtQuerySecurityObject 语义档预告 | 280 | 已深化 | UNX-F12226-J1 SECURITY_INFORMATION 四类（Owner/Group/DACL/SACL）语义档齐，SD 结构消费与 J1 接口联签状态登记 |
| UNX-F12227 | NtMakeTemporaryObject 语义档预告 | 260 | 已深化 | UNX-F12227-J1 临时化语义（PERMANENT 位清除→引用归零即回收）判据绿，与 D3 对象生命周期联签登记 |
| UNX-F12228 | NtQueryDirectoryObject 语义档预告（D3 边界） | 260 | 已深化 | UNX-F12228-J1 目录枚举协议语义档齐（RestartScan/索引续传），枚举本体归 D3 分界判据绿 |
| UNX-F12229 | 对象管理族存根面：用户态→内核调用约定 | 320 | 已深化 | UNX-F12229-J1 存根模板（参数寄存器分配/编号/展开码）生成判据绿，20 存根样本与真实 ntdll 反汇编形态抽样一致 |
| UNX-F12230 | Nt/Zw 双前缀语义与 PreviousMode 档 | 320 | 已深化 | UNX-F12230-J1 双前缀语义档齐（Varix 简化同入口+人格标注，总册域行原文），PreviousMode 三档（Kernel/User/either）参数捕获判据绿 |
| UNX-F12231 | 对象管理族错误矩阵 | 300 | 已深化 | UNX-F12231-J1 族错误矩阵（每接口 ≥5 错误路径）全行齐码，注入抽样 30 例返回码与矩阵一致 |
| UNX-F12232 | 对象管理族边界样本集 | 300 | 已深化 | UNX-F12232-J1 边界样本集（NULL 参数/越界缓冲/极端句柄值）≥40 例入库，全例拒止不崩 |
| UNX-F12233 | 对象管理族双机对照判据 | 280 | 已深化 | UNX-F12233-J1 同码双跑（基准机 vs Varix）抽样 15 场景输出逐值一致（S1 判据母版），差异场景显式列账 |
| UNX-F12234 | 对象管理族对照表 30 条首批入账 | 300 | 已深化 | UNX-F12234-J1 对照表首批 ≥30 API 行齐（api_name/category/semver_anchor/status_matrix/judge_ids 五列），抽样 10 行独立可复测 |
| UNX-F12235 | 对象管理族与 D3 句柄表联签收口 | 300 | 已深化 | UNX-F12235-J1 联签接口（F12091 冻结件）对接判据绿，句柄生命周期全程（建/复制/查/关）与 D3 账逐笔对平 |
| UNX-F12236 | 对象管理族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12236-J1 族内 P95 调用延迟预算在册（进 O1 五指标配套账），超预算路径标红可定位 |
| UNX-F12237 | 对象管理族防幻觉出处账 | 260 | 已深化 | UNX-F12237-J1 全族语义条目出处字段非空率 100%，"查无资料"条目显式标"待基准机实测"清单在册 |
| UNX-F12238 | 对象管理族文档对齐 | 260 | 已深化 | UNX-F12238-J1 族文档（接口说明/错误矩阵/复测指引）与对照表逐行一致，文档-账漂移抽查零命中 |
| UNX-F12239 | ktest 对象管理族断言集（B12 批判据聚合） | 320 | 已深化 | UNX-F12239-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，错误矩阵族与双机对照族独立可单跑 |
| UNX-F12240 | 对象管理族集成账 | 280 | 已深化 | UNX-F12240-J1 批内互引零悬空；D3/J1/O1 消费方预告登记，族集成账与对照表行数一致 |

<!-- 主册行 15535 · #### UNX-D1-B13 · 内存族 Virtual* NT API 语义档（F12241–F12260 · 20 -->
#### UNX-D1-B13 · 内存族 Virtual* NT API 语义档（F12241–F12260 · 20 条）

> AI-16 承办｜批主题：M 型内存族——Allocate/Free/Protect/QueryVirtualMemory、MEM_* 三态、PAGE_* 保护矩阵、与 A4 地址空间联签｜域账累计：68,760 + 本批 5,980 = 74,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Virtual Memory Functions 与 Windows Internals ch.5（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12241–F12260 唯一；与 A4 分界——A4 管页表与物理页本体，本域管 NT 语义协议层（状态机/错误面/双机对照）；SEC_IMAGE 映射语义挂 D2（总册域行原文）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12241 | NtAllocateVirtualMemory 语义档 | 360 | 已深化 | UNX-F12241-J1 协议语义档（BaseAddress 协商/零基址语义/RegionSize 取整）与锚一致，双机同参数集分配结果布局一致 30/30 组 |
| UNX-F12242 | NtFreeVirtualMemory 语义档 | 320 | 已深化 | UNX-F12242-J1 MEM_DECOMMIT/RELEASE 两档语义判据绿，区间不整注入 10/10 次正确拒止（FREE 必须全区间语义） |
| UNX-F12243 | NtProtectVirtualMemory 语义档 | 320 | 已深化 | UNX-F12243-J1 保护变更（含旧值回填 OldProtection）判据绿，跨已提交/保留区混合区间行为与锚一致 |
| UNX-F12244 | NtQueryVirtualMemory 语义档 | 340 | 已深化 | UNX-F12244-J1 MemoryBasicInformation 结构（BaseAddress/RegionSize/State/Protect/Type）逐字段与锚一致，双机同进程扫描区间序列一致 |
| UNX-F12245 | NtMapViewOfSection 语义档预告（D2 节对象） | 300 | 已深化 | UNX-F12245-J1 视图协议语义档（ViewBase 协商/InheritDisposition）齐，SEC_IMAGE 映射挂 D2 分界判据绿 |
| UNX-F12246 | NtUnmapViewOfSection 语义档预告 | 280 | 已深化 | UNX-F12246-J1 解映射协议（区间/Whole 档）语义档齐，视图计数守恒判据绿 |
| UNX-F12247 | NtReadVirtualMemory/NtWriteVirtualMemory 语义档 | 320 | 已深化 | UNX-F12247-J1 跨进程读写协议（含 PARTIAL_COPY 语义）判据绿，越界读注入返回已读字节量与锚一致 |
| UNX-F12248 | 内存族状态机：MEM_COMMIT/RESERVE/FREE 三态 | 320 | 已深化 | UNX-F12248-J1 三态迁移图全弧覆盖（Free→Reserve→Commit 及旁路），注入非法迁移 10/10 次正确拒止 |
| UNX-F12249 | 页保护矩阵：PAGE_* 全集语义 | 300 | 已深化 | UNX-F12249-J1 PAGE_*（NOACCESS/READONLY/READWRITE/EXECUTE_* /GUARD/WRITECOPY）全集语义档齐，GUARD 页触发即清行为与锚一致 |
| UNX-F12250 | 内存族错误矩阵 | 280 | 已深化 | UNX-F12250-J1 族错误矩阵（CONFLICTING_FREE_ADDRESS/NOT_COMMITTED/CANT_INITIALIZE 类）全行齐码，注入抽样 30 例一致 |
| UNX-F12251 | 内存族边界样本集：对齐/越界/零页 | 300 | 已深化 | UNX-F12251-J1 边界样本（页界对齐/粒度取整/零地址段）≥40 例入库全过，零页访问 10/10 次正确异常 |
| UNX-F12252 | 内存族与 A4 地址空间联签收口 | 300 | 已深化 | UNX-F12252-J1 协议层↔A4 页表层对接判据绿（每进程 CR3 消费），全进程销毁回收守恒账零漂移 |
| UNX-F12253 | 内存族双机对照判据 | 280 | 已深化 | UNX-F12253-J1 同码双跑（VMMap 类观测对照）抽样 15 场景区间序列一致（S1 母版），差异显式列账 |
| UNX-F12254 | 内存族对照表 30 条批入账 | 300 | 已深化 | UNX-F12254-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12255 | 内存族 WOW64 预告：32 位地址空间档 | 280 | 已深化 | UNX-F12255-J1 4GB 上限/高址预留语义档齐，32 位进程分配注入正确落位（WOW64 主体归 D2 分界） |
| UNX-F12256 | 内存族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12256-J1 族内 P95 延迟预算在册（O1 五指标配套账），分配/释放 10^5 次压测无增长 |
| UNX-F12257 | 内存族防幻觉出处账 | 260 | 已深化 | UNX-F12257-J1 出处字段非空率 100%，"待基准机实测"清单在册（幻觉红线） |
| UNX-F12258 | 内存族文档对齐 | 260 | 已深化 | UNX-F12258-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12259 | ktest 内存族断言集（B13 批判据聚合） | 320 | 已深化 | UNX-F12259-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，状态机族与保护矩阵族独立可单跑 |
| UNX-F12260 | 内存族集成账 | 280 | 已深化 | UNX-F12260-J1 批内互引零悬空；A4/D2/O1 消费方预告登记，集成账与对照表行数一致 |

<!-- 主册行 15562 · #### UNX-D1-B14 · 文件族 NativeFile* NT API 语义档（F12261–F12280 · -->
#### UNX-D1-B14 · 文件族 NativeFile* NT API 语义档（F12261–F12280 · 20 条）

> AI-16 承办｜批主题：M 型文件族——NtCreateFile 全矩阵/Read/Write/Query/SetInformationFile/DirectoryFile/Flush/取消语义、FILE_* 掩码矩阵、与 B1 VFS 联签｜域账累计：74,740 + 本批 6,000 = 80,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Nt*File 文档与 Windows Internals ch.12（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12261–F12280 唯一；与 B1 分界——B1 管通用 VFS 本体，本域管 NT 侧语义协议（NativeFile* 面）；与 B12 分界——B12 立对象协议面，本批立文件语义主体（两处 NtCreateFile 条目按"协议档/语义主体"分层防重，判据不同）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12261 | NtCreateFile 完整语义档：disposition/share/access 矩阵 | 380 | 已深化 | UNX-F12261-J1 disposition（SUPERSEDE/CREATE/OPEN/OVERWRITE/OVERWRITEIF）×share（共享位三轴）×access（GENERIC 位映射）矩阵全格语义档齐，注入抽样 40 组与锚一致 |
| UNX-F12262 | NtReadFile/NtWriteFile 语义档 | 340 | 已深化 | UNX-F12262-J1 ByteOffset 语义（NULL=当前位）与同步/异步双档判据绿，双机同序列读写结果逐字节一致 |
| UNX-F12263 | NtQueryInformationFile 语义档 | 320 | 已深化 | UNX-F12263-J1 信息类首批 ≥15 类（Basic/Standard/Position/Name 类）结构布局与锚一致，双机抽样 10 类逐字段一致 |
| UNX-F12264 | NtSetInformationFile 语义档 | 320 | 已深化 | UNX-F12264-J1 信息类首批 ≥10 类（Position/Rename/Disposition/EndOfFile）语义判据绿，重命名跨目录语义档齐 |
| UNX-F12265 | NtQueryDirectoryFile 语义档 | 320 | 已深化 | UNX-F12265-J1 枚举协议（RestartScan/Index/ReturnSingleEntry/通配匹配）判据绿，枚举序与 Windows 同目录抽样一致 |
| UNX-F12266 | NtFlushBuffersFile 语义档 | 280 | 已深化 | UNX-F12266-J1 冲刷协议判据绿，掉电模拟注入后数据完整（与 B2 journal 工程经验复用判据） |
| UNX-F12267 | NtCancelIoFile/NtCancelIoFileEx 取消语义 | 280 | 已深化 | UNX-F12267-J1 同步/异步取消双接口判据绿，取消竞态（完成与取消同时）注入 10/10 次收敛正确 |
| UNX-F12268 | NtDeviceIoControlFile/NtFsControlFile 预告 | 300 | 已深化 | UNX-F12268-J1 控制码通道协议语义档齐（FSCTL/IOCTL 分派），设备语义主体归 B4/I 部分界判据绿 |
| UNX-F12269 | FILE_* 访问掩码与共享模式矩阵 | 300 | 已深化 | UNX-F12269-J1 访问掩码（FILE_READ_DATA→GENERIC_READ 映射）与共享冲突矩阵全格，SHARING_VIOLATION 触发 10/10 次正确 |
| UNX-F12270 | IO_STATUS_BLOCK 与异步 IO 预告 | 300 | 已深化 | UNX-F12270-J1 IOSB 结构（Status/Information 联合体布局）判据绿，异步完成消费语义与 B12 F12023 同源判据绿 |
| UNX-F12271 | 文件族错误矩阵：SHARING_VIOLATION 等全表 | 300 | 已深化 | UNX-F12271-J1 族错误矩阵（≥25 码）全行齐，注入抽样 30 例返回码一致 |
| UNX-F12272 | 文件族与 B1 VFS 联签收口 | 300 | 已深化 | UNX-F12272-J1 NT 协议层↔VFS 本体对接判据绿（路径翻译挂 D3 命名空间预告），全生命周期账对平 |
| UNX-F12273 | 文件族边界样本集 | 300 | 已深化 | UNX-F12273-J1 边界样本（零长读写/极端偏移/长路径/特殊字符名）≥40 例入库全过不崩 |
| UNX-F12274 | 文件族双机对照判据 | 280 | 已深化 | UNX-F12274-J1 同码双跑抽样 15 场景（建/读写/枚举/删）输出逐值一致（S1 母版），差异显式列账 |
| UNX-F12275 | 文件族对照表 30 条批入账 | 300 | 已深化 | UNX-F12275-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12276 | 文件族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12276-J1 族内 P95 延迟预算在册（O1 配套账），4KB 随机读写压测口径登记 |
| UNX-F12277 | 文件族防幻觉出处账 | 260 | 已深化 | UNX-F12277-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12278 | 文件族文档对齐 | 260 | 已深化 | UNX-F12278-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12279 | ktest 文件族断言集（B14 批判据聚合） | 320 | 已深化 | UNX-F12279-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，disposition 矩阵族与错误矩阵族独立可单跑 |
| UNX-F12280 | 文件族集成账 | 280 | 已深化 | UNX-F12280-J1 批内互引零悬空；B1/B2/D3 消费方预告登记，集成账与对照表行数一致 |

<!-- 主册行 15589 · #### UNX-D1-B15 · 同步族 Wait/Event/Semaphore/Mutant NT API 语义档 -->
#### UNX-D1-B15 · 同步族 Wait/Event/Semaphore/Mutant NT API 语义档（F12281–F12300 · 20 条）

> AI-16 承办｜批主题：M 型同步族——Wait 单/多对象、Event/Semaphore/Mutant/Timer 五族语义档、WAIT 块与超时协议、alertable 联签（M 型前段收官批）｜域账累计：80,740 + 本批 6,000 = 86,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Synchronization Functions 与 Windows Internals ch.8（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 C2 timerfd/eventfd 底座为语义层对照复用（总册域行原文）｜防重：F12281–F12300 唯一；与 D3 分界——D3 管对象类型本体与命名，本域管等待协议语义；与 B10 分界——B10 立临界区，本批立内核等待族。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12281 | NtWaitForSingleObject 语义档 | 340 | 已深化 | UNX-F12281-J1 等待协议（Timeout 绝对/相对档判别、Alertable 参数双档）判据绿，五态返回（WAIT_0/TIMEOUT/ALERTED/APC/ABANDONED）注入 20/20 次正确 |
| UNX-F12282 | NtWaitForMultipleObjects 语义档 | 340 | 已深化 | UNX-F12282-J1 bWaitAll 双档+任意序满足判据绿，句柄上限（64 档）越界注入正确拒止，返回索引与满足对象一致 |
| UNX-F12283 | Event 族语义档：Create/Open/Set/Reset/Pulse | 360 | 已深化 | UNX-F12283-J1 Notification/Synchronization 两型五接口矩阵判据绿，Pulse 仅唤醒现等者语义与锚一致，双机同序列状态翻转逐值一致 |
| UNX-F12284 | Semaphore 族语义档：Create/Release/Open | 320 | 已深化 | UNX-F12284-J1 计数语义（MaximumCount 上界/ReleaseCount）判据绿，超上限 Release 10/10 次正确拒止，计数守恒账零漂移 |
| UNX-F12285 | Mutant 族语义档：Create/Release/Open | 320 | 已深化 | UNX-F12285-J1 递归所有权（同线程重入 Abandoned 语义）判据绿，废弃 Mutant 等待返回 ABANDONED_WAIT_0 与 B02 码族一致 |
| UNX-F12286 | Timer 族语义档预告：Create/Set/Cancel | 320 | 已深化 | UNX-F12286-J1 Notification/Synchronization 两型+周期档语义档齐，与 A2 时钟源消费联签登记（编程本体归 A2 分界） |
| UNX-F12287 | NtSignalAndWaitForSingleObject 语义档 | 280 | 已深化 | UNX-F12287-J1 信号+等待原子序判据绿（先信号后等待无唤醒丢失），注入交错序 10/10 次收敛正确 |
| UNX-F12288 | WAIT 块语义与超时协议 | 300 | 已深化 | UNX-F12288-J1 WAIT 块（多对象计数/满足索引）语义档齐，超时精度账（P95 漂移 ≤1ms 档）与 A2 时钟口径一致 |
| UNX-F12289 | 同步族错误矩阵 | 280 | 已深化 | UNX-F12289-J1 族错误矩阵（INVALID_HANDLE/ACCESS_DENIED/ TIMEOUT 负值类）全行齐码，注入抽样 30 例一致 |
| UNX-F12290 | 同步族与 A2/A3 联签收口：中断与调度底座 | 300 | 已深化 | UNX-F12290-J1 等待挂 A3 调度器唤醒链判据绿（中断叫醒消费 A2 底座），唤醒延迟 P95 账在预算内 |
| UNX-F12291 | 同步族边界样本集：唤醒丢失/竞态/超时漂移 | 300 | 已深化 | UNX-F12291-J1 经典坑样本（先唤醒后等待/多对象同时满足/信号风暴）≥40 例入库全过，竞态注入 10^4 次零丢失 |
| UNX-F12292 | 同步族双机对照判据 | 280 | 已深化 | UNX-F12292-J1 同码双跑抽样 15 场景状态序列一致（S1 母版），时序敏感场景容差档显式声明 |
| UNX-F12293 | 同步族对照表 30 条批入账 | 300 | 已深化 | UNX-F12293-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12294 | 同步族与 D4 消息泵联签预告：alertable 等待 | 280 | 已深化 | UNX-F12294-J1 alertable 等待与消息泵共存语义档齐（MsgWaitForMultipleObjects 类预告），APC 投递时机判据与 B11 F12215 同源 |
| UNX-F12295 | 同步族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12295-J1 族内 P95 延迟预算在册（O1 配套账），空转唤醒压测无内核抖动红账 |
| UNX-F12296 | 同步族防幻觉出处账 | 260 | 已深化 | UNX-F12296-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12297 | 同步族文档对齐 | 260 | 已深化 | UNX-F12297-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12298 | ktest 同步族断言集（B15 批判据聚合） | 320 | 已深化 | UNX-F12298-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，等待五态族与竞态注入族独立可单跑 |
| UNX-F12299 | M 型机制批阶段小结与 E 型预告 | 280 | 已深化 | UNX-F12299-J1 B11–B15 五族集成账（SEH/对象/内存/文件/同步）零悬空，E 型矩阵批（B25–B28）与 I 型联签批（B29–B36）预告登记 |
| UNX-F12300 | 三百条里程碑条目：D1 首三百条集成账与下阶段开门件 | 300 | 已深化 | UNX-F12300-J1 首三百条（F12001–F12300）互引集成账零悬空、行数守恒账对平，E1 Wine 启动探测面（PEB/SEH/NTSTATUS 组合）联测判据绿——波 08 开工前置门交付 |

<!-- 主册行 15616 · #### UNX-D1-B16 · 原子操作与互锁族 NT API 语义档（F12301–F12320 · 20 条） -->
#### UNX-D1-B16 · 原子操作与互锁族 NT API 语义档（F12301–F12320 · 20 条）

> AI-16 承办｜批主题：M 型尾段首发——Interlocked* 互锁全家族（Increment/Decrement/Exchange/ExchangeAdd/CompareExchange/And-Or-Xor/BitTest 系）、64 位与对齐语义、屏障后缀全矩阵、MemoryBarrier、EncodePointer cookie 派生、并发守恒判据（M 型尾段 180 条第 1 批）｜域账累计：86,740 + 本批 6,000 = 92,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Interlocked Variable Access 与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 A5 内存序指令域为语义层对照复用（总册域行原文）｜防重：F12301–F12320 唯一；与 B10 分界——B10 立临界区（用户态自旋+等待块协议），本批立无锁原子原语；与 A5 分界——A5 管内存序指令本体（LOCK 前缀/MFENCE），本批管 ntdll 互锁导出语义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12301 | InterlockedIncrement/Decrement 语义档 | 340 | 已深化 | UNX-F12301-J1 原子自增减返回值规则（返回修改后值）判据绿，16 线程并发 10^6 次计数守恒零漂移 |
| UNX-F12302 | InterlockedExchange/ExchangePointer 语义档 | 340 | 已深化 | UNX-F12302-J1 交换返回旧值语义 10/10 正确，指针宽档与 32/64 位差异注入 10/10 正确 |
| UNX-F12303 | InterlockedExchangeAdd/Add64 语义档 | 360 | 已深化 | UNX-F12303-J1 加法返回旧值语义判据绿，负增量档（等价减法）与溢出回绕注入 10/10 正确 |
| UNX-F12304 | InterlockedCompareExchange/CompareExchangePointer 语义档 | 320 | 已深化 | UNX-F12304-J1 相等才换/不等返回原值两路判据绿，CAS 自旋环重构样本 10^4 次收敛正确 |
| UNX-F12305 | InterlockedAnd/Or/Xor 位运算族语义档 | 320 | 已深化 | UNX-F12305-J1 位运算三接口返回旧值判据绿，掩码组合注入 20/20 结果一致 |
| UNX-F12306 | InterlockedBitTestAndSet/Reset/Complement 语义档 | 320 | 已深化 | UNX-F12306-J1 位测试置位/复位/取反返回原位值判据绿，位序（LSB 基准）注入 20/20 正确 |
| UNX-F12307 | 互锁族 64 位与对齐语义档 | 280 | 已深化 | UNX-F12307-J1 64 位档 8 字节对齐要求判据绿，未对齐注入显式 UB 档标注 10/10 |
| UNX-F12308 | 互锁族屏障后缀全矩阵语义档 | 300 | 已深化 | UNX-F12308-J1 _acquire/_release/_nf 后缀语义矩阵判据绿，乱序观测样本 10^4 次与内存序账一致 |
| UNX-F12309 | MemoryBarrier/YieldProcessor 语义档 | 280 | 已深化 | UNX-F12309-J1 全屏障（MemoryBarrier）与让步提示（YieldProcessor）语义档齐，屏障必要性样本（无屏障坏序）10/10 检出 |
| UNX-F12310 | EncodePointer/DecodePointer 语义档 | 300 | 已深化 | UNX-F12310-J1 cookie 派生往返（Encode→Decode 恒等）判据绿，跨进程 cookie 隔离注入 10/10 正确 |
| UNX-F12311 | 互锁族并发守恒判据 | 300 | 已深化 | UNX-F12311-J1 16 线程混用六接口 10^6 次操作计数守恒零漂移，丢更新检出率 100% |
| UNX-F12312 | 互锁族错误矩阵 | 280 | 已深化 | UNX-F12312-J1 族错误矩阵（未对齐/坏地址/越界档）全行齐码，注入抽样 30 例一致 |
| UNX-F12313 | 互锁族双机对照判据 | 300 | 已深化 | UNX-F12313-J1 同码双跑 15 场景结果序列一致（S1 母版），返回值类零容差 |
| UNX-F12314 | 互锁族对照表批入账 | 280 | 已深化 | UNX-F12314-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12315 | 互锁族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12315-J1 单原子操作 P95 预算在册（O1 配套账），高竞争退化曲线无红账 |
| UNX-F12316 | 互锁族防幻觉出处账 | 260 | 已深化 | UNX-F12316-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12317 | 互锁族文档对齐 | 260 | 已深化 | UNX-F12317-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12318 | 互锁族与 C1 调度可见性联签 | 320 | 已深化 | UNX-F12318-J1 原子写跨线程可见性挂 C1 调度/缓存一致性口径判据绿，可见性延迟账在预算内 |
| UNX-F12319 | 互锁族边界样本集 | 280 | 已深化 | UNX-F12319-J1 经典坑样本（ABA/丢更新/伪共享）≥40 例入库全过，ABA 检出样本在册 |
| UNX-F12320 | ktest 原子互锁族断言集（B16 批判据聚合） | 300 | 已深化 | UNX-F12320-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，并发守恒族与屏障族独立可单跑 |

<!-- 主册行 15643 · #### UNX-D1-B17 · TLS/FLS 与纤程族 NT API 语义档（F12321–F12340 · 20 -->
#### UNX-D1-B17 · TLS/FLS 与纤程族 NT API 语义档（F12321–F12340 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 2 批——TlsAlloc/TlsGetValue/TlsSetValue/TlsFree 四接口、FLS 族（纤程局部存储+退出回调）、TLS 槽位表与 TEB 扩展链、纤程五接口（ConvertThreadToFiber/CreateFiber/SwitchToFiber/DeleteFiber）、索引耗尽注入（M 型尾段 180 条第 2 批）｜域账累计：92,740 + 本批 6,000 = 98,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Thread Local Storage/Fiber Functions 与 Windows Internals ch.4（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B04 TEB 深化批为消费关系（TLS 槽表即 TEB 字段）｜防重：F12321–F12340 唯一；与 B04 分界——B04 立 TEB 布局与字段档，本批立 TLS/FLS 槽表操作语义；与 C1 分界——C1 管线程对象本体与调度，本批管存储槽与纤程切换语义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12321 | TlsAlloc 语义档 | 300 | 已深化 | UNX-F12321-J1 槽位分配（全线程可用/初始值 0）判据绿，分配序与回收复用账 10^3 次零漂移 |
| UNX-F12322 | TlsGetValue/TlsSetValue 语义档 | 320 | 已深化 | UNX-F12322-J1 Get/Set 往返一致 10^4 次，GetValue 失败不清 LastError 语义与锚一致注入 10/10 |
| UNX-F12323 | TlsFree 语义档 | 280 | 已深化 | UNX-F12323-J1 释放后槽位复用判据绿，释放后访问注入 10/10 次正确拒止 |
| UNX-F12324 | FLS 族语义档：FlsAlloc/FlsGetValue/FlsSetValue/FlsFree | 340 | 已深化 | UNX-F12324-J1 四接口矩阵判据绿，退出回调交付时机（纤程/线程退出）注入 10/10 正确 |
| UNX-F12325 | TLS 槽位表与 TEB 扩展链 | 300 | 已深化 | UNX-F12325-J1 64 槽基础表+扩展链结构档与 B04 TEB 偏移表一致，扩展档切换判据绿 |
| UNX-F12326 | 纤程五接口语义档：ConvertThreadToFiber/CreateFiber/SwitchToFiber/DeleteFiber | 340 | 已深化 | UNX-F12326-J1 栈切换语义（寄存器保存恢复往返）10^4 次一致，主纤程转换 10/10 正确 |
| UNX-F12327 | 纤程局部存储与退出回调 | 280 | 已深化 | UNX-F12327-J1 FLS 与纤程绑定语义判据绿，DeleteFiber 触发回调注入 10/10 交付正确 |
| UNX-F12328 | TLS/FLS 错误矩阵 | 280 | 已深化 | UNX-F12328-J1 族错误矩阵（坏索引/越界/无效槽）全行齐码，注入抽样 30 例一致 |
| UNX-F12329 | TLS 与线程终止联签 | 300 | 已深化 | UNX-F12329-J1 线程终止时 TLS 索引泄漏账归零判据绿，终止注入 10/10 次账平 |
| UNX-F12330 | 纤程调度边界样本集 | 300 | 已深化 | UNX-F12330-J1 栈切换坑样本（跨纤程句柄误用/删除自纤程）≥30 例入库全过，误用注入 10/10 拒止 |
| UNX-F12331 | TLS/FLS 双机对照判据 | 300 | 已深化 | UNX-F12331-J1 同码双跑 15 场景槽值序列一致（S1 母版），回调交付序类零容差 |
| UNX-F12332 | TLS/FLS 对照表批入账 | 340 | 已深化 | UNX-F12332-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12333 | TLS/FLS 性能预算（O1 对标口径） | 300 | 已深化 | UNX-F12333-J1 TlsGetValue P95 预算在册（O1 配套账），SwitchToFiber 切换开销无红账 |
| UNX-F12334 | TLS/FLS 防幻觉出处账 | 300 | 已深化 | UNX-F12334-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12335 | TLS/FLS 文档对齐 | 260 | 已深化 | UNX-F12335-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12336 | FLS 与 D2 装载联签预告：CRT 初始化消费面 | 320 | 已深化 | UNX-F12336-J1 CRT 启动期 FLS 消费语义档齐，D2 承接批号与联测用例清单登记入预告面 |
| UNX-F12337 | TLS 索引耗尽注入判据 | 300 | 已深化 | UNX-F12337-J1 槽位上限（基础 64+扩展档）耗尽注入 10/10 次正确拒止，耗尽边界值与锚一致 |
| UNX-F12338 | TLS/FLS 边界样本集 | 260 | 已深化 | UNX-F12338-J1 经典坑样本（跨线程取槽/索引 0 语义/值 0 与失败歧义）≥30 例入库全过 |
| UNX-F12339 | ktest TLS 纤程族断言集（B17 批判据聚合） | 280 | 已深化 | UNX-F12339-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，TLS 族与纤程族独立可单跑 |
| UNX-F12340 | 批小结与 B18 预告 | 300 | 已深化 | UNX-F12340-J1 B17 集成账（19 条互引）零悬空，RTL 运行时族批（B18）预告登记入域待办账 |

<!-- 主册行 15670 · #### UNX-D1-B18 · RTL 字符串与大整数运行时族 NT API 语义档（F12341–F12360 · -->
#### UNX-D1-B18 · RTL 字符串与大整数运行时族 NT API 语义档（F12341–F12360 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 3 批——Rtl* 字符串族（Init/Append/Copy/Equal/Compare/Upcase/转换）、RtlIntegerToUnicodeString 进制档、RtlLargeInteger 大整数四则、Rtl 内存块族（Zero/Fill/Copy/Move/CompareMemory）、RtlGetVersion/RtlNtStatusToDosError、截断与 STATUS_BUFFER_OVERFLOW 语义（M 型尾段 180 条第 3 批）｜域账累计：98,740 + 本批 6,000 = 104,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Rtl String Functions/Safe String Functions 与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B06 UNICODE_STRING 批为消费关系（结构本体已立）｜防重：F12341–F12360 唯一；与 B06 分界——B06 立 UNICODE_STRING 结构与 PEB 消费，本批立 Rtl 字符串操作函数语义；与 B23 分界——SID/安全描述体构造族归 B23，本批零安全对象。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12341 | RtlInitUnicodeString/RtlInitAnsiString 语义档 | 280 | 已深化 | UNX-F12341-J1 初始化三字段（Length/MaximumLength/Buffer）赋值规则判据绿，NULL 源注入 10/10 次得空串档 |
| UNX-F12342 | RtlAppendUnicodeStringToString/ToString 语义档 | 300 | 已深化 | UNX-F12342-J1 追加越界截断（返回 STATUS_BUFFER_TOO_SMALL 不写）判据绿，注入 20/20 正确 |
| UNX-F12343 | RtlCopyUnicodeString/EqualUnicodeString/CompareUnicodeString 语义档 | 320 | 已深化 | UNX-F12343-J1 复制截断/相等（CaseInsensitive 双档）/字典序比较判据绿，注入 30/30 一致 |
| UNX-F12344 | RtlUpcaseUnicodeString/Char 语义档 | 300 | 已深化 | UNX-F12344-J1 大写映射表语义判据绿，非 ASCII 样本与锚一致性 20/20（差异列账） |
| UNX-F12345 | ANSI/Unicode 转换族语义档 | 340 | 已深化 | UNX-F12345-J1 双向转换（Ansi→Unicode/Unicode→Ansi）判据绿，代码页档与不可映射字符注入 10/10 正确 |
| UNX-F12346 | RtlIntegerToUnicodeString/UnicodeToInteger 语义档 | 300 | 已深化 | UNX-F12346-J1 进制档（2/8/10/16）双向判据绿，溢出与非法字符注入 20/20 正确 |
| UNX-F12347 | RtlLargeInteger 运算族语义档 | 280 | 已深化 | UNX-F12347-J1 大整数四则（Divide/Multiply）与移位判据绿，除零注入 10/10 次正确拒止 |
| UNX-F12348 | Rtl 内存块族语义档：Zero/Fill/Copy/Move/CompareMemory | 340 | 已深化 | UNX-F12348-J1 五接口语义判据绿，重叠区（Move 允许/Copy 拒止档）注入 20/20 正确 |
| UNX-F12349 | RtlGetVersion/RtlNtStatusToDosError 语义档 | 280 | 已深化 | UNX-F12349-J1 版本结构档与码映射（NTSTATUS→Win32 错误）判据绿，抽样 40 码映射与 B02 一致 |
| UNX-F12350 | RtlRandom/RtlRandomEx 伪随机语义档 | 260 | 已深化 | UNX-F12350-J1 种子态推进确定性判据绿（同种子同序列），非密码用途声明在册（零密码实现） |
| UNX-F12351 | 截断语义与 STATUS_BUFFER_OVERFLOW 档 | 300 | 已深化 | UNX-F12351-J1 截断不算致命错语义（返回码+Length 半写档）判据绿，注入 10/10 与锚一致 |
| UNX-F12352 | RTL 运行时错误矩阵 | 320 | 已深化 | UNX-F12352-J1 族错误矩阵（坏指针/越界/非法参数）全行齐码，注入抽样 30 例一致 |
| UNX-F12353 | RTL 字符串边界样本集 | 260 | 已深化 | UNX-F12353-J1 经典坑样本（非终止/奇长/嵌入空/最大长度边界）≥40 例入库全过 |
| UNX-F12354 | RTL 大整数边界样本集 | 280 | 已深化 | UNX-F12354-J1 边界样本（MAXONGLONG/符号档/除零/移位越界）≥30 例入库全过 |
| UNX-F12355 | RTL 族双机对照判据 | 300 | 已深化 | UNX-F12355-J1 同码双跑 15 场景输出一致（S1 母版），映射表类零容差 |
| UNX-F12356 | RTL 族对照表批入账 | 340 | 已深化 | UNX-F12356-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12357 | RTL 族性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12357-J1 字符串族 P95 预算在册（O1 配套账），大块 Copy 吞吐账无红账 |
| UNX-F12358 | RTL 族防幻觉出处账 | 300 | 已深化 | UNX-F12358-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12359 | ktest RTL 族断言集（B18 批判据聚合） | 320 | 已深化 | UNX-F12359-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，字符串族与大整数族独立可单跑 |
| UNX-F12360 | 批小结与 B19 预告 | 300 | 已深化 | UNX-F12360-J1 B18 集成账（19 条互引）零悬空，进程线程管理族批（B19）预告登记入域待办账 |

<!-- 主册行 15697 · #### UNX-D1-B19 · 进程与线程管理族 NT API 语义档（F12361–F12380 · 20 条） -->
#### UNX-D1-B19 · 进程与线程管理族 NT API 语义档（F12361–F12380 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 4 批——NtCreateUserProcess/NtTerminateProcess 终止序、NtCreateThreadEx/NtSuspendThread/NtResumeThread 挂起计数、NtDelayExecution/NtYieldExecution、NtGet/SetContextThread、NtAlertThread/NtQueueApcThread、NtOpenProcess/NtOpenThread、伪句柄 -1/-2 档（M 型尾段 180 条第 4 批）｜域账累计：104,740 + 本批 6,000 = 110,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Process and Thread Functions 与 Windows Internals ch.5（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 C1 进程模型为消费关系（上游 AI-11 冻结接口：进程对象人格路由）｜防重：F12361–F12380 唯一；与 C1 分界——C1 管进程/线程对象本体与调度，本批管 NT API 调用语义；与 B11 分界——B11 立 APC 投递通道本体，本批立 NtQueueApcThread 调用面语义（同源联签）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12361 | NtCreateUserProcess/NtCreateProcessEx 语义档 | 320 | 已深化 | UNX-F12361-J1 创建参数语义（OA/节句柄/参数块）判据绿，创建-终止对冲 10^2 次账平，与 C1 路由一致 |
| UNX-F12362 | NtTerminateProcess/NtTerminateThread 语义档 | 280 | 已深化 | UNX-F12362-J1 终止序（线程收割→句柄失效→对象销毁）判据绿，自终止/他终止注入 10/10 正确 |
| UNX-F12363 | NtCreateThreadEx/NtSuspendThread/NtResumeThread 语义档 | 300 | 已深化 | UNX-F12363-J1 创建挂起档（CreateSuspended）与挂起计数语义判据绿，嵌套挂起注入 10/10 正确 |
| UNX-F12364 | NtDelayExecution/NtYieldExecution 语义档 | 340 | 已深化 | UNX-F12364-J1 延迟绝对/相对档判别与让步语义判据绿，延迟精度账（P95 漂移 ≤1ms 档）与 A2 口径一致 |
| UNX-F12365 | NtGetContextThread/NtSetContextThread 语义档 | 260 | 已深化 | UNX-F12365-J1 CONTEXT 按需存取（ContextFlags 门控）判据绿，Get/Set 往返一致 10^4 次 |
| UNX-F12366 | NtAlertThread/NtQueueApcThread 语义档 | 300 | 已深化 | UNX-F12366-J1 alert 置位与 APC 入队判据绿，投递时机与 B11 F12215 单一通道同源断言 10/10 |
| UNX-F12367 | NtOpenProcess/NtOpenThread 语义档 | 320 | 已深化 | UNX-F12367-J1 按 PID/TID 打开（DesiredAccess 掩码）判据绿，权限不足注入 10/10 次正确拒止 |
| UNX-F12368 | 伪句柄语义档：NtCurrentProcess/NtCurrentThread | 280 | 已深化 | UNX-F12368-J1 -1/-2 伪句柄语义（仅限本进程可见）判据绿，跨进程传递注入 10/10 次正确拒止 |
| UNX-F12369 | 进程线程族错误矩阵 | 340 | 已深化 | UNX-F12369-J1 族错误矩阵（坏 PID/权限/状态冲突）全行齐码，注入抽样 30 例一致 |
| UNX-F12370 | 挂起计数守恒判据 | 300 | 已深化 | UNX-F12370-J1 Suspend/Resume 对冲守恒 10^4 次零漂移，resume 过冲注入 10/10 正确拒止 |
| UNX-F12371 | 终止序边界样本集 | 280 | 已深化 | UNX-F12371-J1 经典坑样本（终止中互斥废弃/句柄泄漏/APC 在途）≥30 例入库全过 |
| UNX-F12372 | CONTEXT 保存恢复往返判据 | 260 | 已深化 | UNX-F12372-J1 寄存器组全档往返一致率 100%（B09 陷阱帧消费），改写 PC 样本 10/10 生效 |
| UNX-F12373 | 进程线程族双机对照判据 | 320 | 已深化 | UNX-F12373-J1 同码双跑 15 场景状态迁移序列一致（S1 母版），终止序类零容差 |
| UNX-F12374 | 进程线程族对照表批入账 | 300 | 已深化 | UNX-F12374-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12375 | 进程线程族性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12375-J1 create/terminate P95 预算在册（O1 配套账），线程切换开销无红账 |
| UNX-F12376 | 进程线程族与 C1 联签收口 | 340 | 已深化 | UNX-F12376-J1 对象人格路由消费联签判据绿（创建/终止/查询三路对平 C1 账流水） |
| UNX-F12377 | 进程线程族防幻觉出处账 | 300 | 已深化 | UNX-F12377-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12378 | 进程线程族文档对齐 | 260 | 已深化 | UNX-F12378-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12379 | ktest 进程线程族断言集（B19 批判据聚合） | 320 | 已深化 | UNX-F12379-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，生命周期族与 CONTEXT 族独立可单跑 |
| UNX-F12380 | 批小结与 B20 预告 | 300 | 已深化 | UNX-F12380-J1 B19 集成账（19 条互引）零悬空，查询设置信息族批（B20）预告登记入域待办账 |

<!-- 主册行 15724 · #### UNX-D1-B20 · 查询与设置信息族矩阵 NT API 语义档（F12381–F12400 · 20 条 -->
#### UNX-D1-B20 · 查询与设置信息族矩阵 NT API 语义档（F12381–F12400 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 5 批——Query/SetInformation* 双族矩阵（Process/Thread/System/Object/VirtualMemory/TimerResolution 六面档位矩阵）、ReturnLength 二段探测协议、未知档位拒止、与 D2/D3 消费联签（M 型尾段 180 条第 5 批）｜域账累计：110,740 + 本批 6,000 = 116,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Get/Set Information 类接口与 Windows Internals（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B03/B04 为消费关系（PEB/TEB 信息档回读）｜防重：F12381–F12400 唯一；与 K1 分界——K1 管系统级信息面总账，本批立 ntdll 调用面语义（SystemInformationClass 消费）；与 B13 分界——B13 管 Virtual* 操作语义，NtQueryVirtualMemory 查询类归本批。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12381 | NtQueryInformationProcess 档位矩阵 | 300 | 已深化 | UNX-F12381-J1 ≥10 档位（BasicInformation/PEB 基址等）判据绿，逐档缓冲区语义注入 30/30 一致 |
| UNX-F12382 | NtSetInformationProcess 档位矩阵 | 340 | 已深化 | UNX-F12382-J1 ≥8 档位写语义（含优先级档）判据绿，写权限不足注入 10/10 次正确拒止 |
| UNX-F12383 | NtQueryInformationThread 档位矩阵 | 320 | 已深化 | UNX-F12383-J1 ≥8 档位判据绿（含线程起始地址档），逐档注入 20/20 一致 |
| UNX-F12384 | NtSetInformationThread 档位矩阵 | 280 | 已深化 | UNX-F12384-J1 ≥6 档位判据绿，隐藏线程档（不可查询档）注入 10/10 与锚一致 |
| UNX-F12385 | NtQuerySystemInformation 档位矩阵 | 300 | 已深化 | UNX-F12385-J1 基础档位（Basic/Processor/Process 列表）判据绿，与 K1 信息面分界对平 10/10 |
| UNX-F12386 | NtQueryObject/NtSetInformationObject 语义档 | 320 | 已深化 | UNX-F12386-J1 对象类型/名称/信息三档判据绿，坏句柄注入 10/10 次正确拒止 |
| UNX-F12387 | NtQueryVirtualMemory 语义档 | 260 | 已深化 | UNX-F12387-J1 MemoryBasicInformation 档与 B13 Virtual 账逐页对平 10^2 页，坏地址注入 10/10 拒止 |
| UNX-F12388 | NtQueryTimerResolution/NtSetTimerResolution 语义档 | 340 | 已深化 | UNX-F12388-J1 分辨率查询/设置边界值判据绿，与 A2 时钟口径联签断言 10/10 |
| UNX-F12389 | 信息族缓冲区协议：ReturnLength 二段探测 | 300 | 已深化 | UNX-F12389-J1 两段式（首探 Length/再取数据）判据绿，BUFFER_TOO_SMALL 注入 20/20 正确 |
| UNX-F12390 | 信息族错误矩阵 | 280 | 已深化 | UNX-F12390-J1 族错误矩阵（INFO_LENGTH_MISMATCH/ACCESS_DENIED 档）全行齐码，抽样 30 例一致 |
| UNX-F12391 | 档位未知类拒止判据 | 320 | 已深化 | UNX-F12391-J1 InvalidInfoClass 注入 10/10 次 STATUS_INVALID_INFO_CLASS 正确拒止，越界档全行齐 |
| UNX-F12392 | 信息族边界样本集 | 300 | 已深化 | UNX-F12392-J1 经典坑样本（Length=0/NULL 缓冲/半缓冲）≥40 例入库全过 |
| UNX-F12393 | 信息族双机对照判据 | 260 | 已深化 | UNX-F12393-J1 同码双跑 15 场景档位返回一致（S1 母版），值域类零容差（时变值白名单） |
| UNX-F12394 | 信息族对照表批入账 | 340 | 已深化 | UNX-F12394-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12395 | 信息族性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12395-J1 查询类 P95 预算在册（O1 配套账），大缓冲拷贝账无红账 |
| UNX-F12396 | 信息族与 D2/D3 消费联签 | 300 | 已深化 | UNX-F12396-J1 PEB 基址档（→D2）与句柄名查询档（→D3）联签消费面判据绿，登记入冻结三件 |
| UNX-F12397 | 信息族防幻觉出处账 | 320 | 已深化 | UNX-F12397-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12398 | 信息族文档对齐 | 280 | 已深化 | UNX-F12398-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12399 | ktest 信息族断言集（B20 批判据聚合） | 300 | 已深化 | UNX-F12399-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，查询族与设置族独立可单跑 |
| UNX-F12400 | 批小结与 B21 预告 | 260 | 已深化 | UNX-F12400-J1 B20 集成账（19 条互引）零悬空，节对象映射族批（B21）预告登记入域待办账 |

<!-- 主册行 15751 · #### UNX-D1-B21 · 节对象与内存映射族 NT API 语义档（F12401–F12420 · 20 条） -->
#### UNX-D1-B21 · 节对象与内存映射族 NT API 语义档（F12401–F12420 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 6 批——NtCreateSection/NtMapViewOfSection/NtUnmapViewOfSection 语义、视图生命周期账、映射保护档、SEC_IMAGE 装载联签预告、引用计数与 LastClose、双进程共享视图（M 型尾段 180 条第 6 批）｜域账累计：116,740 + 本批 6,000 = 122,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Section Objects 与 Windows Internals ch.9（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B13 内存族为消费关系（Virtual 账页档）｜防重：F12401–F12420 唯一；与 B13 分界——B13 立进程地址空间 Virtual* 操作语义，本批立节对象（共享/映像映射）语义；与 D2 分界——D2 管 PE 装载本体，SEC_IMAGE 映射档为联签面。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12401 | NtCreateSection 语义档 | 320 | 已深化 | UNX-F12401-J1 属性语义（PAGE 保护档/SEC_COMMIT/SEC_IMAGE）判据绿，非法组合注入 20/20 次正确拒止 |
| UNX-F12402 | NtOpenSection/NtExtendSection 语义档 | 300 | 已深化 | UNX-F12402-J1 按名打开（OA 消费）与扩展（仅分页档）判据绿，扩展物理节注入 10/10 次正确拒止 |
| UNX-F12403 | NtMapViewOfSection 语义档 | 340 | 已深化 | UNX-F12403-J1 映射参数语义（ZeroBits/CommitSize/Protect/InheritDisposition）判据绿，逐参数注入 30/30 正确 |
| UNX-F12404 | NtUnmapViewOfSection/NtUnmapViewOfSectionEx 语义档 | 280 | 已深化 | UNX-F12404-J1 解除映射（整视/按范围档）判据绿，重复解除注入 10/10 次正确拒止 |
| UNX-F12405 | 视图生命周期账 | 300 | 已深化 | UNX-F12405-J1 映射-访问-解除守恒账 10^4 次零漂移，进程退出兜底解除判据绿 |
| UNX-F12406 | 映射保护与 B13 VirtualProtect 联签 | 320 | 已深化 | UNX-F12406-J1 视图页保护档与 B13 账逐页对平 10^2 页，改保护注入与 PAGE 档一致 |
| UNX-F12407 | SEC_IMAGE 与 PE 装载联签预告 | 280 | 已深化 | UNX-F12407-J1 映像映射档（节对齐/保护派生）语义预告齐，D2 承接批号登记入联测清单 |
| UNX-F12408 | 节对象引用计数与 LastClose 语义 | 300 | 已深化 | UNX-F12408-J1 引用计数（节/视图两层）判据绿，LastClose 触发视图回收注入 10/10 正确 |
| UNX-F12409 | 共享内存双进程语义档 | 260 | 已深化 | UNX-F12409-J1 双进程同节视图数据可见一致 10^4 次，写入序与缓存一致性账平 |
| UNX-F12410 | 节族错误矩阵 | 340 | 已深化 | UNX-F12410-J1 族错误矩阵（保护冲突/越界/类型错配）全行齐码，注入抽样 30 例一致 |
| UNX-F12411 | 映射越界注入判据 | 280 | 已深化 | UNX-F12411-J1 ViewSize 超节/偏移越界/ZeroBits 冲突注入 30/30 次正确拒止 |
| UNX-F12412 | 节族边界样本集 | 300 | 已深化 | UNX-F12412-J1 经典坑样本（零页视/大视口对齐/SEC_IMAGE 保护派生）≥30 例入库全过 |
| UNX-F12413 | 节族双机对照判据 | 320 | 已深化 | UNX-F12413-J1 同码双跑 15 场景视图账一致（S1 母版），保护档类零容差 |
| UNX-F12414 | 节族对照表批入账 | 300 | 已深化 | UNX-F12414-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12415 | 节族性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12415-J1 map/unmap P95 预算在册（O1 配套账），共享页翻转账无红账 |
| UNX-F12416 | 节族与 A4 页帧账联签 | 260 | 已深化 | UNX-F12416-J1 物理页消费挂 A4 页帧账判据绿（分页档），页账对平零漂移 |
| UNX-F12417 | 节族防幻觉出处账 | 340 | 已深化 | UNX-F12417-J1 出处字段非空率 100%，"待基准机实测"清单在册 |
| UNX-F12418 | 节族文档对齐 | 300 | 已深化 | UNX-F12418-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12419 | ktest 节族断言集（B21 批判据聚合） | 280 | 已深化 | UNX-F12419-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，映射族与共享族独立可单跑 |
| UNX-F12420 | 批小结与 B22 预告 | 300 | 已深化 | UNX-F12420-J1 B21 集成账（19 条互引）零悬空，LPC-ALPC 通信族批（B22）预告登记入域待办账 |

<!-- 主册行 15778 · #### UNX-D1-B22 · LPC/ALPC 通信族 NT API 语义档（F12421–F12440 · 20 -->
#### UNX-D1-B22 · LPC/ALPC 通信族 NT API 语义档（F12421–F12440 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 7 批——NtCreatePort/NtConnectPort/NtSecureConnectPort/NtListenPort/NtAcceptConnectPort/NtRequestWaitReplyPort/NtReplyPort 语义、端口消息协议（消息头/数据区/最大长度）、连接生命周期账（M 型尾段 180 条第 7 批）｜域账累计：122,740 + 本批 6,000 = 128,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Local Procedure Call 与 Windows Internals ch.8 ALPC 节（版本锚 ADR-UNX-008），ReactOS 对照不抄； undocumented 面按防幻觉高压线标注出处档｜防重：F12421–F12440 唯一；与 D4 分界——D4 管消息泵与 GUI 消息面，本批管 LPC/ALPC 端口通信语义；与 B13 分界——通信缓冲不落 Virtual 账（端口消息区独立记账）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12421 | NtCreatePort/NtCreateWaitablePort 语义档 | 340 | 已深化 | UNX-F12421-J1 建口参数（MaxMessageLength/MaxPoolUsage）判据绿，非法配比注入 10/10 次正确拒止 |
| UNX-F12422 | NtConnectPort 语义档 | 340 | 已深化 | UNX-F12422-J1 连接握手（请求/接受/拒绝三态）判据绿，共享内存协商档注入 10/10 正确 |
| UNX-F12423 | NtSecureConnectPort 语义档 | 360 | 已深化 | UNX-F12423-J1 安全连接（ServerSid 校验/QoS 档）判据绿，SID 错配注入 10/10 次正确拒止 |
| UNX-F12424 | NtListenPort/NtAcceptConnectPort 语义档 | 320 | 已深化 | UNX-F12424-J1 侦听阻塞语义与接受/拒绝二选一判据绿，并发连接注入 10/10 序正确 |
| UNX-F12425 | NtRequestPort/NtRequestWaitReplyPort 语义档 | 320 | 已深化 | UNX-F12425-J1 单向/等待回包两档判据绿，回包超时注入 10/10 与锚一致 |
| UNX-F12426 | NtReplyPort/NtReplyWaitReceivePort 语义档 | 320 | 已深化 | UNX-F12426-J1 回包/回包并接收循环语义判据绿，消息序守恒 10^4 次零漂移 |
| UNX-F12427 | 端口消息协议：消息头/数据区/最大长度 | 280 | 已深化 | UNX-F12427-J1 消息头结构档（含 CID 字段）判据绿，越最大长度注入 10/10 次正确拒止 |
| UNX-F12428 | 端口句柄语义：服务端/客户端两态 | 300 | 已深化 | UNX-F12428-J1 两态句柄能力矩阵判据绿，客户端句柄误调服务端接口注入 10/10 拒止 |
| UNX-F12429 | LPC 连接生命周期账 | 280 | 已深化 | UNX-F12429-J1 连接-通信-断开守恒账 10^3 次零漂移，服务端退出兜底断开判据绿 |
| UNX-F12430 | 端口族错误矩阵 | 300 | 已深化 | UNX-F12430-J1 族错误矩阵（坏口/坏消息/池耗尽档）全行齐码，注入抽样 30 例一致 |
| UNX-F12431 | 消息越界注入判据 | 300 | 已深化 | UNX-F12431-J1 超长消息/坏消息头/空数据区注入 30/30 次正确拒止，池账无泄漏 |
| UNX-F12432 | 端口族边界样本集 | 280 | 已深化 | UNX-F12432-J1 经典坑样本（并发连接风暴/回包乱序/半关闭口）≥30 例入库全过 |
| UNX-F12433 | 端口族双机对照判据 | 300 | 已深化 | UNX-F12433-J1 同码双跑 15 场景消息序一致（S1 母版），握手三态类零容差 |
| UNX-F12434 | 端口族对照表批入账 | 280 | 已深化 | UNX-F12434-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12435 | 端口族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12435-J1 roundtrip P95 预算在册（O1 配套账），高并发连接账无红账 |
| UNX-F12436 | 端口族与 D4 消息面联签预告 | 260 | 已深化 | UNX-F12436-J1 ALPC→D4 消息泵消费面预告齐，D4 承接批号与联测用例登记在册 |
| UNX-F12437 | 端口族防幻觉出处账 | 260 | 已深化 | UNX-F12437-J1 出处字段非空率 100%（undocumented 面显式标注），“待实测”清单在册 |
| UNX-F12438 | 端口族文档对齐 | 320 | 已深化 | UNX-F12438-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12439 | ktest 端口族断言集（B22 批判据聚合） | 280 | 已深化 | UNX-F12439-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，握手族与消息族独立可单跑 |
| UNX-F12440 | 批小结与 B23 预告 | 300 | 已深化 | UNX-F12440-J1 B22 集成账（19 条互引）零悬空，令牌安全引用族批（B23）预告登记入域待办账 |

<!-- 主册行 15805 · #### UNX-D1-B23 · 令牌与安全引用监视族 NT API 语义档（F12441–F12460 · 20 条 -->
#### UNX-D1-B23 · 令牌与安全引用监视族 NT API 语义档（F12441–F12460 · 20 条）

> AI-16 承办｜批主题：M 型尾段第 8 批——Token 族（Open/OpenEx/AdjustPrivileges/AdjustGroups/Create/Duplicate/Query/Set Information）、NtAccessCheck 引用监视判定、NtPrivilegeCheck、NtQuery/SetSecurityObject、SID/ACL 构造族、与 J3 密码学域分界（M 型尾段 180 条第 8 批）｜域账累计：128,740 + 本批 6,000 = 134,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Access Control 与 Windows Internals ch.3（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B05 OA 批为消费关系（安全描述体字段）｜防重：F12441–F12460 唯一；与 J3 分界——J3 管密码学算法本体，本域零密码实现（token 完整性校验算法全为调用面）；与 B20 分界——Token 查询档位本体归本批，信息族缓冲协议通用件回指 F12389。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12441 | NtOpenProcessToken/NtOpenThreadToken 语义档 | 300 | 已深化 | UNX-F12441-J1 按进程/线程开令牌判据绿，无令牌对象注入 10/10 次正确拒止 |
| UNX-F12442 | NtOpenProcessTokenEx/NtOpenThreadTokenEx 语义档 | 320 | 已深化 | UNX-F12442-J1 Ex 档（HandleAttributes 继承位）判据绿，与基础版差异面 10/10 一致 |
| UNX-F12443 | NtAdjustPrivilegesToken 语义档 | 280 | 已深化 | UNX-F12443-J1 特权启用/禁用/原值回返（PreviousState 档）判据绿，无特权注入 10/10 正确拒止 |
| UNX-F12444 | NtAdjustGroupsToken 语义档 | 340 | 已深化 | UNX-F12444-J1 组启用/禁用（SE_GROUP_ENABLED 位）判据绿，强制组注入 10/10 次正确拒止 |
| UNX-F12445 | NtCreateToken/NtDuplicateToken 语义档 | 300 | 已深化 | UNX-F12445-J1 创建参数与复制（ImpersonationLevel 档）判据绿，越权复制注入 10/10 拒止 |
| UNX-F12446 | NtQueryInformationToken/NtSetInformationToken 语义档 | 260 | 已深化 | UNX-F12446-J1 ≥8 档位（User/Groups/Privileges）判据绿，缓冲协议回指 F12389 断言一致 |
| UNX-F12447 | NtAccessCheck 语义档 | 320 | 已深化 | UNX-F12447-J1 DACL 遍历判定（Allow/Deny 序）判据绿，注入 30/30 判定与锚一致 |
| UNX-F12448 | NtPrivilegeCheck 语义档 | 300 | 已深化 | UNX-F12448-J1 特权持有检查（单/多特权档）判据绿，未持有注入 10/10 次正确返回 |
| UNX-F12449 | NtQuerySecurityObject/NtSetSecurityObject 语义档 | 340 | 已深化 | UNX-F12449-J1 自相对 SD 格式四段（Owner/Group/Dacl/Sacl 位）判据绿，逐位注入 20/20 正确 |
| UNX-F12450 | SID/ACL 构造族语义档 | 280 | 已深化 | UNX-F12450-J1 RtlValidSid/LengthSid/CopySid/SetDacl 判据绿，坏 SID 注入 10/10 次拒止 |
| UNX-F12451 | 令牌族错误矩阵 | 260 | 已深化 | UNX-F12451-J1 族错误矩阵（权限/类型/状态档）全行齐码，注入抽样 30 例一致 |
| UNX-F12452 | 访问拒绝注入判据 | 320 | 已深化 | UNX-F12452-J1 无特权 Adjust/越权 AccessCheck 注入 30/30 次 STATUS_PRIVILEGE_NOT_HELD 正确 |
| UNX-F12453 | 令牌族边界样本集 | 300 | 已深化 | UNX-F12453-J1 经典坑样本（特权矩阵×组矩阵复合/模拟级越界）≥30 例入库全过 |
| UNX-F12454 | 令牌族双机对照判据 | 280 | 已深化 | UNX-F12454-J1 同码双跑 15 场景判定结果一致（S1 母版），判定类零容差 |
| UNX-F12455 | 令牌族对照表批入账 | 340 | 已深化 | UNX-F12455-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12456 | 令牌族性能预算（O1 对标口径） | 300 | 已深化 | UNX-F12456-J1 AccessCheck P95 预算在册（O1 配套账），长 DACL 退化曲线无红账 |
| UNX-F12457 | 令牌族与 J3 密码学域分界声明 | 260 | 已深化 | UNX-F12457-J1 零密码实现声明在册（哈希/加密全为 J3 调用面），分界表登记断言绿 |
| UNX-F12458 | 令牌族防幻觉出处账 | 320 | 已深化 | UNX-F12458-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12459 | ktest 令牌族断言集（B23 批判据聚合） | 280 | 已深化 | UNX-F12459-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，令牌族与 ACL 族独立可单跑 |
| UNX-F12460 | 批小结与 B24 预告 | 300 | 已深化 | UNX-F12460-J1 B23 集成账（19 条互引）零悬空，注册表族批（B24）预告登记入域待办账 |

<!-- 主册行 15832 · #### UNX-D1-B24 · 注册表族与 M 型尾段收官 NT API 语义档（F12461–F12480 · 2 -->
#### UNX-D1-B24 · 注册表族与 M 型尾段收官 NT API 语义档（F12461–F12480 · 20 条）

> AI-16 承办｜批主题：M 型尾段收官批——NtCreateKey/NtOpenKey/Ex、NtDeleteKey/NtRenameKey/NtFlushKey、NtEnumerateKey/ValueKey 索引递进、NtQueryValueKey/MultipleValueKey、NtSetValueKey 值类型档、NtNotifyChangeKey 异步监视、F12480 M 型全段集成账（B11–B24 十四批）（M 型尾段 180 条第 9 批/收官）｜域账累计：134,740 + 本批 6,000 = 140,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Registry Key Objects 与 Windows Internals ch.10（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B05 OA 批为消费关系（RootDirectory/相对路径）｜防重：F12461–F12480 唯一；与 A4 分界——A4 管 hive 存储落盘本体，本批管注册表 NT API 调用语义；与 D3 分界——键对象命名经 D3 协议（联签引用）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12461 | NtCreateKey/NtOpenKey 语义档 | 280 | 已深化 | UNX-F12461-J1 创建/打开二态（Disposition 返回档）判据绿，坏路径注入 10/10 次正确拒止 |
| UNX-F12462 | NtOpenKeyEx/NtCreateKeyEx 语义档 | 300 | 已深化 | UNX-F12462-J1 Ex 档（AccessOptions/OpenOptions）判据绿，REG_LEGAL_OPTION 越界注入 10/10 拒止 |
| UNX-F12463 | NtDeleteKey/NtRenameKey/NtFlushKey 语义档 | 320 | 已深化 | UNX-F12463-J1 删除（含子树拒止档）/重命名/落盘判据绿，子键存在注入 10/10 次正确拒止 |
| UNX-F12464 | NtEnumerateKey/NtEnumerateValueKey 语义档 | 260 | 已深化 | UNX-F12464-J1 索引递进协议（Index 单调）判据绿，NO_MORE_ENTRIES 注入 10/10 正确 |
| UNX-F12465 | NtQueryKey/NtQueryValueKey/NtQueryMultipleValueKey 语义档 | 340 | 已深化 | UNX-F12465-J1 三查询档（名/全名/统计）判据绿，多值原子读注入 10/10 与锚一致 |
| UNX-F12466 | NtSetValueKey/NtDeleteValueKey 语义档 | 300 | 已深化 | UNX-F12466-J1 值类型档（REG_SZ/DWORD/BINARY 矩阵）判据绿，类型错配注入 10/10 次正确拒止 |
| UNX-F12467 | NtNotifyChangeKey/NtNotifyChangeMultipleKeys 语义档 | 280 | 已深化 | UNX-F12467-J1 异步监视（事件/完成档）判据绿，变更触发 10/10 次正确交付 |
| UNX-F12468 | 注册表键生命周期账 | 320 | 已深化 | UNX-F12468-J1 create-enum-set-delete 守恒账 10^3 次零漂移，句柄泄漏账归零判据绿 |
| UNX-F12469 | 注册表路径与对象命名联签 | 300 | 已深化 | UNX-F12469-J1 RootDirectory 相对路径与绝对路径双档判据绿，与 D3 命名协议对平 10/10 |
| UNX-F12470 | 注册表族错误矩阵 | 260 | 已深化 | UNX-F12470-J1 族错误矩阵（坏路径/权限/状态档）全行齐码，注入抽样 30 例一致 |
| UNX-F12471 | 枚举索引越界注入判据 | 340 | 已深化 | UNX-F12471-J1 越界索引/回退索引注入 30/30 次正确拒止，枚举快照一致性断言绿 |
| UNX-F12472 | 注册表族边界样本集 | 280 | 已深化 | UNX-F12472-J1 经典坑样本（深路径/长名/大值/键名大小写档）≥30 例入库全过 |
| UNX-F12473 | 注册表族双机对照判据 | 300 | 已深化 | UNX-F12473-J1 同码双跑 15 场景键树快照一致（S1 母版），值序类零容差 |
| UNX-F12474 | 注册表族对照表批入账 | 320 | 已深化 | UNX-F12474-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12475 | 注册表族性能预算（O1 对标口径） | 260 | 已深化 | UNX-F12475-J1 键操作 P95 预算在册（O1 配套账），深树枚举退化无红账 |
| UNX-F12476 | 注册表族与 A4 存储联签预告 | 300 | 已深化 | UNX-F12476-J1 hive 落盘分界预告齐（A4 承接批号登记），脏页计数对平断言绿 |
| UNX-F12477 | 注册表族防幻觉出处账 | 340 | 已深化 | UNX-F12477-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12478 | 注册表族文档对齐 | 280 | 已深化 | UNX-F12478-J1 族文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12479 | ktest 注册表族断言集（B24 批判据聚合） | 300 | 已深化 | UNX-F12479-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，键操作族与监视族独立可单跑 |
| UNX-F12480 | M 型机制批全段收官条目 | 320 | 已深化 | UNX-F12480-J1 B11–B24 十四批 280 条集成账零悬空、行数守恒对平 140,740，E 型矩阵批（B25–B28）开门预告登记 |

<!-- 主册行 15859 · #### UNX-D1-B25 · E 型异常注入矩阵批 NT API 语义档（F12481–F12500 · 20 条 -->
#### UNX-D1-B25 · E 型异常注入矩阵批 NT API 语义档（F12481–F12500 · 20 条）

> AI-16 承办｜批主题：E 型第 1 批——异常注入矩阵总纲与十二面矩阵（PEB/TEB/OA/UNICODE_STRING/RUNTIME_FUNCTION/UNWIND_INFO/CONTEXT/临界区/SEH/对象/内存/文件/同步/机制尾段），每面 ≥30 例全格注入，聚合账 ≥300 例（E 型 80 条第 1 批）｜域账累计：140,740 + 本批 6,000 = 146,740 / 240,000｜嫁接源：纯自研域；注入制度承 F12218 母版与防御矩阵制度（版本锚 ADR-UNX-008）；消费 B01–B24 全部语义档判据｜防重：F12481–F12500 唯一；与 B25–B28 其余三批分界——本批管"注入矩阵编制与全格执行"，坏表拒止归 B26、句柄错误码归 B27、PreviousMode 归 B28；注入例复用各批判据 ID 引用不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12481 | E 型方法论与注入总纲 | 320 | 已深化 | UNX-F12481-J1 注入三元组（动作×对象×阈值）制度档齐，矩阵编制规则与红账升级路径判据绿 |
| UNX-F12482 | PEB 面注入矩阵 | 280 | 已深化 | UNX-F12482-J1 坏签名/坏 Size/坏版本档 30 例注入全过，拒止码与 B03 档一致 |
| UNX-F12483 | TEB 面注入矩阵 | 300 | 已深化 | UNX-F12483-J1 坏槽位/坏 TLS 索引/坏异常链 30 例注入全过，拒止码与 B04 档一致 |
| UNX-F12484 | OBJECT_ATTRIBUTES 注入矩阵 | 340 | 已深化 | UNX-F12484-J1 坏 Length/坏 RootDirectory/坏 SecurityQos 30 例注入全过，与 B05 拒止档逐例一致 |
| UNX-F12485 | UNICODE_STRING 注入矩阵 | 260 | 已深化 | UNX-F12485-J1 超长/未终止/奇长 30 例注入全过，与 B06 边界档逐例一致 |
| UNX-F12486 | RUNTIME_FUNCTION 注入矩阵 | 300 | 已深化 | UNX-F12486-J1 坏 BeginAddress/链环/越界 30 例注入全过，与 B07 拒止档一致 |
| UNX-F12487 | UNWIND_INFO 注入矩阵 | 320 | 已深化 | UNX-F12487-J1 坏版本/坏标志/槽计数溢出 30 例注入全过，与 B08 拒止档一致 |
| UNX-F12488 | CONTEXT 注入矩阵 | 280 | 已深化 | UNX-F12488-J1 坏 ContextFlags/坏段寄存器/坏 RSP 30 例注入全过，与 B09 档一致 |
| UNX-F12489 | 临界区注入矩阵 | 340 | 已深化 | UNX-F12489-J1 未初始化进入/重复释放/未持有离开 30 例注入全过，与 B10 档一致 |
| UNX-F12490 | SEH 全链注入矩阵 | 300 | 已深化 | UNX-F12490-J1 坏 try 层级/坏 filter/坏 __finally 序 30 例注入全过，与 B11 档一致 |
| UNX-F12491 | 对象族注入矩阵 | 280 | 已深化 | UNX-F12491-J1 坏类型/坏权限/坏属性 30 例注入全过，与 B12 档一致 |
| UNX-F12492 | 内存族注入矩阵 | 260 | 已深化 | UNX-F12492-J1 坏保护/坏基址/坏 RegionSize 30 例注入全过，与 B13 档一致 |
| UNX-F12493 | 文件族注入矩阵 | 320 | 已深化 | UNX-F12493-J1 坏句柄/坏偏移/坏 IO 状态块 30 例注入全过，与 B14 档一致 |
| UNX-F12494 | 同步族注入矩阵 | 300 | 已深化 | UNX-F12494-J1 坏超时/坏句柄数组/超上限句柄 30 例注入全过，与 B15 档一致 |
| UNX-F12495 | 机制尾段注入矩阵（B16–B24 抽样） | 280 | 已深化 | UNX-F12495-J1 原子/TLS/RTL/进程/信息/节/端口/令牌/注册表九面抽样 60 例全过 |
| UNX-F12496 | 注入矩阵聚合账（≥300 例） | 340 | 已深化 | UNX-F12496-J1 十二面聚合 ≥300 例全登记全过，红账检出 10/10 定位可回溯 |
| UNX-F12497 | 注入矩阵双机对照判据 | 300 | 已深化 | UNX-F12497-J1 抽样 30 例双机拒止行为一致（S1 母版），拒止码类零容差 |
| UNX-F12498 | 注入矩阵防幻觉出处账 | 260 | 已深化 | UNX-F12498-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12499 | 注入矩阵文档对齐 | 320 | 已深化 | UNX-F12499-J1 矩阵文档与聚合账逐行一致，漂移抽查零命中 |
| UNX-F12500 | ktest E 型注入矩阵断言集（B25 批判据聚合） | 300 | 已深化 | UNX-F12500-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，十二面矩阵独立可单跑 |

<!-- 主册行 15886 · #### UNX-D1-B26 · E 型坏表拒止批 NT API 语义档（F12501–F12520 · 20 条） -->
#### UNX-D1-B26 · E 型坏表拒止批 NT API 语义档（F12501–F12520 · 20 条）

> AI-16 承办｜批主题：E 型第 2 批——坏表拒止（fail-closed）：PEB/TEB 槽表/RUNTIME_FUNCTION/UNWIND_INFO/CONTEXT 帧/对象类型表/句柄表坏项/等待块坏链八面坏表注入、统一错误码归类矩阵、拒止后状态守恒与审计账（E 型 80 条第 2 批）｜域账累计：146,740 + 本批 6,000 = 152,740 / 240,000｜嫁接源：纯自研域；拒止制度承防御矩阵制度（版本锚 ADR-UNX-008）；消费 B03–B15 各面数据结构档｜防重：F12501–F12520 唯一；与 B25 分界——B25 管参数级注入矩阵，本批管"表级/结构级损坏拒止"；与 D3 分界——句柄表本体归 D3，本批立 D1 侧坏表拒止语义（联签面 F12508）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12501 | E 型坏表拒止方法论 | 300 | 已深化 | UNX-F12501-J1 坏表定义与 fail-closed 原则档齐，拒止三原则（不写回/不留半态/必留痕）判据绿 |
| UNX-F12502 | PEB 坏表拒止 | 340 | 已深化 | UNX-F12502-J1 LoaderLock 坏态/模块链断环 20 例拒止全过，进程存活性与账守恒断言绿 |
| UNX-F12503 | TEB 坏表拒止 | 320 | 已深化 | UNX-F12503-J1 TLS 槽表损坏/异常链断 20 例拒止全过，线程可终止性判据绿 |
| UNX-F12504 | RUNTIME_FUNCTION 坏表拒止 | 280 | 已深化 | UNX-F12504-J1 函数表越界/重叠/乱序 20 例拒止全过，回退降级路径（域外处置档）显式 |
| UNX-F12505 | UNWIND_INFO 坏码拒止 | 300 | 已深化 | UNX-F12505-J1 操作码越界/槽计数溢出 20 例拒止全过，展开中断安全态判据绿 |
| UNX-F12506 | CONTEXT 坏帧拒止 | 320 | 已深化 | UNX-F12506-J1 RSP 越界/PC 非可执行域 20 例拒止全过，拒止码与 B09 档一致 |
| UNX-F12507 | 对象类型坏表拒止 | 260 | 已深化 | UNX-F12507-J1 类型索引越界/删除器缺位 20 例拒止全过，类型账守恒断言绿 |
| UNX-F12508 | 句柄表坏项拒止（D3 联签预告） | 340 | 已深化 | UNX-F12508-J1 坏代数/坏指针 20 例拒止全过，D3 联签面（拒止码联合验收）登记在册 |
| UNX-F12509 | 等待块坏链拒止 | 300 | 已深化 | UNX-F12509-J1 环链/悬空引用 20 例拒止全过，等待族账守恒断言绿 |
| UNX-F12510 | 坏表拒止统一错误码语义 | 280 | 已深化 | UNX-F12510-J1 错误码归类矩阵（ACCESS_VIOLATION/INVALID_PARAMETER 档）全行齐，抽样 30 例一致 |
| UNX-F12511 | 拒止后状态守恒判据 | 320 | 已深化 | UNX-F12511-J1 拒止不改全局状态 10^3 次断言（八面逐面），半态检出率 100% |
| UNX-F12512 | 坏表注入与审计账 | 300 | 已深化 | UNX-F12512-J1 每次拒止留审计痕迹（面/例/码三元组），审计账与注入账逐笔对平 |
| UNX-F12513 | 坏表拒止双机对照判据 | 260 | 已深化 | UNX-F12513-J1 抽样 20 场景双机拒止行为一致（S1 母版），拒止码类零容差 |
| UNX-F12514 | 坏表拒止对照表批入账 | 340 | 已深化 | UNX-F12514-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12515 | 坏表拒止性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12515-J1 拒止路径 P95 不劣于成功路径档判据绿，O1 对齐零漂移 |
| UNX-F12516 | 坏表拒止边界样本集 | 300 | 已深化 | UNX-F12516-J1 复合坏表样本（双面同坏/链式坏）≥30 例入库全过 |
| UNX-F12517 | 坏表拒止防幻觉出处账 | 320 | 已深化 | UNX-F12517-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12518 | 坏表拒止文档对齐 | 280 | 已深化 | UNX-F12518-J1 文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12519 | ktest 坏表拒止断言集（B26 批判据聚合） | 300 | 已深化 | UNX-F12519-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，八面拒止独立可单跑 |
| UNX-F12520 | 批小结与 B27 预告 | 260 | 已深化 | UNX-F12520-J1 B26 集成账（19 条互引）零悬空，句柄错误码批（B27）预告登记入域待办账 |

<!-- 主册行 15913 · #### UNX-D1-B27 · E 型句柄错误码矩阵批 NT API 语义档（F12521–F12540 · 20  -->
#### UNX-D1-B27 · E 型句柄错误码矩阵批 NT API 语义档（F12521–F12540 · 20 条）

> AI-16 承办｜批主题：E 型第 3 批——句柄错误码全矩阵：INVALID_HANDLE 值域、NtClose 双重关闭、NtDuplicateObject、权限错配、类型错配（OBJECT_TYPE_MISMATCH 全族行）、伪句柄 -1/-2 档、句柄表满、与 B02 NTSTATUS 归一（E 型 80 条第 3 批）｜域账累计：152,740 + 本批 6,000 = 158,740 / 240,000｜嫁接源：纯自研域；码族锚=MSDN NTSTATUS Values 与 B02 编码面冻结件（版本锚 ADR-UNX-008）｜防重：F12521–F12540 唯一；与 B02 分界——B02 立 NTSTATUS 编码面本体，本批立句柄族错误路径矩阵（逐码回指 B02）；与 B26 分界——B26 管坏表拒止，本批管合法调用+坏句柄的错误码语义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12521 | 句柄错误码方法论（值域/0 与 -1 语义） | 320 | 已深化 | UNX-F12521-J1 句柄值域与 0/-1 特值语义档齐，特值注入 10/10 与锚一致 |
| UNX-F12522 | NtClose 错误码矩阵 | 300 | 已深化 | UNX-F12522-J1 双重关闭/伪句柄/保护关闭注入 30/30 次码值正确 |
| UNX-F12523 | NtDuplicateObject 错误码矩阵 | 340 | 已深化 | UNX-F12523-J1 复制参数矩阵（源/目标/权限/选项）错误路径全行齐码，注入抽样 30 例一致 |
| UNX-F12524 | 句柄权限错误码矩阵 | 280 | 已深化 | UNX-F12524-J1 ACCESS_DENIED 全族行齐码（开/查/写/复制四路），注入 30/30 一致 |
| UNX-F12525 | 句柄类型错配错误码 | 300 | 已深化 | UNX-F12525-J1 OBJECT_TYPE_MISMATCH 全族行（≥10 接口）齐码，注入 20/20 一致 |
| UNX-F12526 | 伪句柄（-1/-2）错误码语义 | 320 | 已深化 | UNX-F12526-J1 current process/thread 档能力矩阵判据绿，越权使用注入 10/10 次正确拒止 |
| UNX-F12527 | 句柄继承与复制错误码 | 280 | 已深化 | UNX-F12527-J1 继承标志/复制选项错误路径矩阵齐码，注入 20/20 一致 |
| UNX-F12528 | 句柄表满错误码 | 300 | 已深化 | UNX-F12528-J1 上限注入拒止 10/10 次（INSUFFICIENT_RESOURCES 档），耗尽边界值与锚一致 |
| UNX-F12529 | 句柄错误码与 B02 NTSTATUS 归一 | 260 | 已深化 | UNX-F12529-J1 全矩阵逐码回指 B02 码族一致断言 100%，差异例显式列账零静默 |
| UNX-F12530 | 句柄错误码注入聚合账（≥200 例） | 340 | 已深化 | UNX-F12530-J1 聚合 ≥200 例全登记全过，红账检出 10/10 定位可回溯 |
| UNX-F12531 | 句柄错误码双机对照判据 | 280 | 已深化 | UNX-F12531-J1 抽样 30 例双机码值一致（S1 母版），码值类零容差 |
| UNX-F12532 | 句柄错误码对照表批入账 | 300 | 已深化 | UNX-F12532-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12533 | 句柄错误码性能预算（O1 对标口径） | 320 | 已深化 | UNX-F12533-J1 错误路径不拖慢成功路径判据绿（P95 差 ≤20% 档），O1 对齐零漂移 |
| UNX-F12534 | 句柄错误码边界样本集 | 300 | 已深化 | UNX-F12534-J1 复合错误样本（双错叠加/错误码优先级档）≥30 例入库全过 |
| UNX-F12535 | 句柄错误码防幻觉出处账 | 280 | 已深化 | UNX-F12535-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12536 | 句柄错误码文档对齐 | 260 | 已深化 | UNX-F12536-J1 文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12537 | 句柄错误码与 D3 联签预告 | 340 | 已深化 | UNX-F12537-J1 错误码联合验收面预告齐（D3 承接批号登记），联测用例清单在册 |
| UNX-F12538 | 句柄错误码复测资产入库 | 300 | 已深化 | UNX-F12538-J1 注入例全量入复测资产库，资产版本号与批归档一致断言绿 |
| UNX-F12539 | ktest 句柄错误码断言集（B27 批判据聚合） | 280 | 已深化 | UNX-F12539-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，错误矩阵族独立可单跑 |
| UNX-F12540 | 批小结与 B28 预告 | 300 | 已深化 | UNX-F12540-J1 B27 集成账（19 条互引）零悬空，PreviousMode 批（B28）预告登记入域待办账 |

<!-- 主册行 15940 · #### UNX-D1-B28 · E 型 PreviousMode 参数校验批 NT API 语义档（F12541–F -->
#### UNX-D1-B28 · E 型 PreviousMode 参数校验批 NT API 语义档（F12541–F12560 · 20 条）

> AI-16 承办｜批主题：E 型收官批——PreviousMode（UserMode/KernelMode）两档语义总纲、ProbeForRead/ProbeForWrite、UserMode 指针探测矩阵、参数/结构体大小/字符串缓冲校验矩阵、内核句柄特权档、校验顺序协议（先探测后使用/TOCTOU 防护）、捕获缓冲三段序、F12560 E 型段收官（E 型 80 条第 4 批/收官）｜域账累计：158,740 + 本批 6,000 = 164,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN ProbeForRead/PreviousMode 与 Windows Internals ch.2（版本锚 ADR-UNX-008），ReactOS 对照不抄；与 B05 OA 批为消费关系｜防重：F12541–F12560 唯一；与 B25 分界——B25 管跨面注入矩阵，本批管内核入口参数校验单一主题；与 B13 分界——探测校验协议归本批，地址空间操作归 B13。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12541 | PreviousMode 语义总纲（UserMode/KernelMode） | 340 | 已深化 | UNX-F12541-J1 两档定义与来源（线程字段档）判据绿，双档行为差异矩阵全格判据绿 |
| UNX-F12542 | ProbeForRead/ProbeForWrite 语义档 | 340 | 已深化 | UNX-F12542-J1 对齐+范围双校验判据绿，坏指针/越界注入 30/30 次正确拒止 |
| UNX-F12543 | UserMode 指针探测矩阵 | 360 | 已深化 | UNX-F12543-J1 内核指针伪装 UserMode 注入 30/30 次正确拒止，探测旁路检出 10/10 |
| UNX-F12544 | 参数范围校验矩阵 | 320 | 已深化 | UNX-F12544-J1 Length 溢出/负值/上界注入 30/30 次正确拒止，边界值档与锚一致 |
| UNX-F12545 | 结构体大小校验矩阵 | 320 | 已深化 | UNX-F12545-J1 Length < sizeof 拒止/长缓冲容忍双档判据绿，注入 20/20 一致 |
| UNX-F12546 | 字符串缓冲探测矩阵 | 280 | 已深化 | UNX-F12546-J1 MaximumLength 探测语义判据绿，坏 Buffer 注入 10/10 次正确拒止 |
| UNX-F12547 | 内核句柄特权档 | 300 | 已深化 | UNX-F12547-J1 KERNEL_HANDLE 位语义判据绿，UserMode 传内核句柄注入 10/10 次拒止 |
| UNX-F12548 | 访问掩码校验矩阵 | 280 | 已深化 | UNX-F12548-J1 Generic 映射/DesiredAccess 校验判据绿，非法位注入 20/20 一致 |
| UNX-F12549 | 校验顺序协议：先探测后使用/TOCTOU 防护 | 300 | 已深化 | UNX-F12549-J1 探测-使用序判据绿，TOCTOU 改页竞态注入 10^3 次零逃逸 |
| UNX-F12550 | 捕获缓冲协议：捕获-校验-拷贝三段序 | 300 | 已深化 | UNX-F12550-J1 三段序判据绿，段间改写注入 10^3 次数据一致性零破坏 |
| UNX-F12551 | 校验错误码矩阵 | 280 | 已深化 | UNX-F12551-J1 ACCESS_VIOLATION/DATATYPE_MISALIGNMENT 档全行齐码，抽样 30 例一致 |
| UNX-F12552 | PreviousMode 注入聚合账（≥200 例） | 300 | 已深化 | UNX-F12552-J1 聚合 ≥200 例全登记全过，红账检出 10/10 定位可回溯 |
| UNX-F12553 | PreviousMode 双机对照判据 | 280 | 已深化 | UNX-F12553-J1 抽样 30 场景双机校验行为一致（S1 母版），拒止码类零容差 |
| UNX-F12554 | PreviousMode 对照表批入账 | 300 | 已深化 | UNX-F12554-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12555 | PreviousMode 性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12555-J1 探测路径开销 P95 预算在册（O1 配套账），大缓冲探测账无红账 |
| UNX-F12556 | PreviousMode 边界样本集 | 260 | 已深化 | UNX-F12556-J1 复合探测样本（双探测叠加/页边界跨取）≥30 例入库全过 |
| UNX-F12557 | PreviousMode 防幻觉出处账 | 260 | 已深化 | UNX-F12557-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12558 | PreviousMode 文档对齐 | 320 | 已深化 | UNX-F12558-J1 文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12559 | ktest PreviousMode 断言集（B28 批判据聚合） | 280 | 已深化 | UNX-F12559-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，探测族与校验族独立可单跑 |
| UNX-F12560 | E 型段收官条目 | 300 | 已深化 | UNX-F12560-J1 B25–B28 四批 80 条集成账零悬空、行数守恒对平 164,740，I 型联签批（B29–B36）开门预告登记 |

<!-- 主册行 15967 · #### UNX-D1-B29 · I 型 D2 装载联签批 NT API 语义档（F12561–F12580 · 20 -->
#### UNX-D1-B29 · I 型 D2 装载联签批 NT API 语义档（F12561–F12580 · 20 条）

> AI-16 承办｜批主题：I 型第 1 批——D1→D2 装载联签：PEB/TEB 偏移表冻结件消费、NTSTATUS 编码面→装载错误语义、PEB LoaderLock→模块表、TLS 回调、IMPORT/EXPORT 表解析联签、.pdata 异常目录→SEH 全链、重定位表→内存映射（I 型 160 条第 1 批）｜域账累计：164,740 + 本批 6,000 = 170,740 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件（PEB/TEB 偏移表与版本分档协议、NTSTATUS 编码面、OBJECT_ATTRIBUTES 协议）与跨域联签制度（版本锚 ADR-UNX-008）；D2 承接面=PE 装载器域｜防重：F12561–F12580 唯一；与 B29–B36 其余批次分界——本批专管 D2 装载联签，D3 句柄表联签归 B30；联签面引用冻结件不重定义偏移值。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12561 | I 型联签方法论与 D1→D2 消费面总表 | 300 | 已深化 | UNX-F12561-J1 消费面总表（九件消费边）齐，联签三列（互引/冻结/联测）制度判据绿 |
| UNX-F12562 | PEB 偏移表→D2 装载器联签 | 320 | 已深化 | UNX-F12562-J1 偏移表冻结件逐字段消费断言绿，版本分档切换 10/10 一致 |
| UNX-F12563 | TEB 偏移表→D2 装载器联签 | 280 | 已深化 | UNX-F12563-J1 TEB 冻结件消费断言绿，装载期 TEB 建立序联测 10/10 正确 |
| UNX-F12564 | NTSTATUS 编码面→D2 装载错误语义联签 | 340 | 已深化 | UNX-F12564-J1 装载错误码逐码回指 B02 一致断言 100%，差异例列账零静默 |
| UNX-F12565 | PEB LoaderLock→D2 模块表联签 | 300 | 已深化 | UNX-F12565-J1 模块表更新持锁序判据绿，锁竞争注入 10^3 次零死锁 |
| UNX-F12566 | 模块加载序与 TLS 回调联签 | 260 | 已深化 | UNX-F12566-J1 TLS 回调交付序（B17 消费）联测判据绿，多模块回调序 10/10 一致 |
| UNX-F12567 | IMPORT 表解析→NT API 语义档联签 | 320 | 已深化 | UNX-F12567-J1 导入解析→本域语义档绑定判据绿，缺符号注入 10/10 次正确拒止 |
| UNX-F12568 | EXPORT 表解析与转发语义联签 | 300 | 已深化 | UNX-F12568-J1 导出解析与转发器档判据绿，转发环注入 10/10 次检出 |
| UNX-F12569 | 异常目录（.pdata）装载→SEH 全链联签 | 340 | 已深化 | UNX-F12569-J1 .pdata 装载→B07/B08/B11 消费链联测判据绿，坏目录注入 30/30 拒止 |
| UNX-F12570 | 重定位表→内存映射联签 | 280 | 已深化 | UNX-F12570-J1 重定位写回与 B13 页账对平判据绿，基址漂移注入 10/10 一致 |
| UNX-F12571 | D2 装载错误注入联测 | 260 | 已深化 | UNX-F12571-J1 坏 PE 样本 30 例联测全过，拒止码与 D2 侧一致断言绿 |
| UNX-F12572 | 联签判据双机对照 | 320 | 已深化 | UNX-F12572-J1 装载结果 PEB/TEB 快照 diff 双机一致（S1 母版），偏移类零容差 |
| UNX-F12573 | 联签对照表批入账 | 300 | 已深化 | UNX-F12573-J1 联签对照表 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12574 | 联签性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12574-J1 装载路径 P95 预算在册（O1 配套账），大模块装载账无红账 |
| UNX-F12575 | 联签边界样本集 | 340 | 已深化 | UNX-F12575-J1 边界样本（全转发/无重定位/超深导入）≥30 例入库全过 |
| UNX-F12576 | 联签桩期/真期两态账 | 300 | 已深化 | UNX-F12576-J1 两态账分列（桩件名/真件名），桩切换真件全量重跑判据绿 |
| UNX-F12577 | 联签防幻觉出处账 | 260 | 已深化 | UNX-F12577-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12578 | 联签文档对齐 | 320 | 已深化 | UNX-F12578-J1 文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12579 | ktest D2 联签断言集（B29 批判据聚合） | 280 | 已深化 | UNX-F12579-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，九件消费边独立可单跑 |
| UNX-F12580 | 批小结与 B30 预告 | 300 | 已深化 | UNX-F12580-J1 B29 集成账（19 条互引）零悬空，D3 句柄表联签批（B30）预告登记入域待办账 |

<!-- 主册行 15994 · #### UNX-D1-B30 · I 型 D3 句柄表联签批与六百条里程碑 NT API 语义档（F12581–F12 -->
#### UNX-D1-B30 · I 型 D3 句柄表联签批与六百条里程碑 NT API 语义档（F12581–F12600 · 20 条）

> AI-16 承办｜批主题：I 型第 2 批——D1→D3 句柄表联签：OBJECT_ATTRIBUTES 协议→对象创建、NTSTATUS 码族→错误语义、对象类型命名→类型本体、句柄代数与复用协议、NtQueryObject→类型信息、命名空间根路径、对象删除挂起协议；F12598–F12600 域待办/开门件/本会话 600 条里程碑（I 型 160 条第 2 批）｜域账累计：170,740 + 本批 6,000 = 176,740 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）；D3 承接面=对象管理域｜防重：F12581–F12600 唯一；与 B30 前批分界——本批专管 D3 联签与域级收口；F12600 里程碑条目引用前 599 条判据 ID 不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12581 | D1→D3 消费面总表（三冻结件） | 280 | 已深化 | UNX-F12581-J1 三冻结件消费边总表齐（OA/NTSTATUS/OBJECT_TYPE），联签三列制度判据绿 |
| UNX-F12582 | OBJECT_ATTRIBUTES 协议→D3 对象创建联签 | 300 | 已深化 | UNX-F12582-J1 OA 冻结协议逐字段消费断言绿（B05 档→D3），注入 20/20 一致 |
| UNX-F12583 | NTSTATUS 码族→D3 错误语义联签 | 320 | 已深化 | UNX-F12583-J1 D3 错误码逐码回指 B02 一致断言 100%，差异例列账零静默 |
| UNX-F12584 | 对象类型命名→D3 类型本体联签 | 260 | 已深化 | UNX-F12584-J1 类型命名协议对平判据绿（B12 分界声明回指），命名冲突注入 10/10 拒止 |
| UNX-F12585 | 句柄代数与复用协议联签 | 340 | 已深化 | UNX-F12585-J1 代数位/复用防陈旧（代数递增）判据绿，复用注入 10^3 次零串号 |
| UNX-F12586 | NtQueryObject→D3 类型信息联签 | 300 | 已深化 | UNX-F12586-J1 查询档与 D3 类型账对平判据绿（B20 F12386 消费回指），抽样 20 例一致 |
| UNX-F12587 | 命名空间根路径协议联签 | 280 | 已深化 | UNX-F12587-J1 \?? 与 \BaseNamedObjects 根路径协议判据绿，越根访问注入 10/10 拒止 |
| UNX-F12588 | 对象删除挂起协议联签 | 320 | 已深化 | UNX-F12588-J1 删除挂起（引用清零后销毁）判据绿，挂起期访问注入 10/10 次正确拒止 |
| UNX-F12589 | D3 错误注入联测 | 300 | 已深化 | UNX-F12589-J1 坏 OA/坏名/坏权限 30 例联测全过，拒止码双侧一致断言绿 |
| UNX-F12590 | 联签判据双机对照 | 260 | 已深化 | UNX-F12590-J1 抽样 20 场景双机行为一致（S1 母版），命名与代数类零容差 |
| UNX-F12591 | 联签对照表批入账 | 340 | 已深化 | UNX-F12591-J1 联签对照表 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12592 | 联签性能预算（O1 对标口径） | 280 | 已深化 | UNX-F12592-J1 句柄操作 P95 预算在册（O1 配套账），高并发注入账无红账 |
| UNX-F12593 | 联签边界样本集 | 300 | 已深化 | UNX-F12593-J1 边界样本（深命名/转发环/挂起叠加）≥30 例入库全过 |
| UNX-F12594 | 联签桩期/真期两态账 | 320 | 已深化 | UNX-F12594-J1 两态账分列（桩件名/真件名），桩切换全量重跑判据绿 |
| UNX-F12595 | 联签防幻觉出处账 | 260 | 已深化 | UNX-F12595-J1 出处字段非空率 100%，“待基准机实测”清单在册 |
| UNX-F12596 | 联签文档对齐 | 300 | 已深化 | UNX-F12596-J1 文档与对照表逐行一致，漂移抽查零命中 |
| UNX-F12597 | ktest D3 联签断言集（B30 批判据聚合） | 340 | 已深化 | UNX-F12597-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，八件消费边独立可单跑 |
| UNX-F12598 | D1 域剩余批次预告（B31–B40） | 280 | 已深化 | UNX-F12598-J1 B31–B40 十批主题框架（E 型尾段/I 型主体/C 型收官）预告登记入域待办账 |
| UNX-F12599 | 下会话开门件（I 型 B31–B36 联测判据框架） | 300 | 已深化 | UNX-F12599-J1 B31–B36 联测判据框架（D4 消息面/E1 Wine 探测面）交付齐，开工前置门判定两态留痕 |
| UNX-F12600 | 本会话 600 条里程碑条目 | 320 | 已深化 | UNX-F12600-J1 F12001–F12600 六百条互引集成账零悬空、行数守恒对平 176,740/240,000（三源一致脚本断言） |

<!-- 主册行 16021 · #### UNX-D1-B31 · I 型 D1→D4 消息面联签一（端口语义对平三列）NT API 语义档（F1260 -->
#### UNX-D1-B31 · I 型 D1→D4 消息面联签一（端口语义对平三列）NT API 语义档（F12601–F12620 · 20 条）

> AI-16 承办｜批主题：I 型第 3 批——D1→D4 消息面联签一：消费边总表与三列制度（F12599 框架兑现=端口语义对平三列）、ALPC 端口对象语义消费边、连接序/投递语义/安全属性/半对接收/区段共享/消息读写/应答等待对/消息属性对平、上下文端口与句柄传递、关闭序拒止、联签预演形态声明（D4 未开工）、注入联测/对照表/性能预算、ktest 聚合、B32 预告开门件｜域账累计：176,740 + 本批 6,326 = 183,066 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）；D4 承接面=USER32/GDI32 语义域（AI-19，未开工按联签预演形态推进：接口映射表先行+会签槽位预置，R-D3-001 同判例，R-D1-030 登记）｜防重：F12601–F12620 唯一；与 B22 LPC 端口本体分界——B22 立端口 API 语义本体，本批立 D1→D4 消息面联签消费面（引用冻结件不重定义）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12601 | D1→D4 消费边总表与三列制度 | 320 | 已深化 | UNX-F12601-J1 消费边总表八行齐（端口对象/连接序/投递/安全属性/半对/区段/读写/关闭），三列制度列齐断言 10/10 |
| UNX-F12602 | ALPC 端口对象语义→D4 消息队列消费联签 | 300 | 已深化 | UNX-F12602-J1 端口对象类型/句柄语义消费对平断言绿（B22 档→D4），抽样 10 例一致 |
| UNX-F12603 | NtConnectPort 连接序→D4 服务端联签 | 341 | 已深化 | UNX-F12603-J1 连接四段序（请求/接受/完成/信息回传）对平断言绿，坏连接注入 10/10 拒止 |
| UNX-F12604 | 端口消息投递语义→D4 消息泵消费边 | 300 | 已深化 | UNX-F12604-J1 投递-接收-应答三段语义对平断言绿，消息边界（零长/超长）注入 10/10 一致 |
| UNX-F12605 | LPC 连接端口安全属性（SQOS）→D4 联签 | 320 | 已深化 | UNX-F12605-J1 SQOS 五档（安全模拟级别等）消费对平断言绿，越权模拟注入 10/10 拒止 |
| UNX-F12606 | NtSecureConnectPort 安全连接→D4 联签 | 300 | 已深化 | UNX-F12606-J1 安全连接参数（ServerSID/RequiredServerSid）对平断言绿，SID 错配注入 10/10 拒止 |
| UNX-F12607 | NtListenPort/NtAcceptConnectPort 半对→D4 服务循环联签 | 342 | 已深化 | UNX-F12607-J1 监听-接受半对协议对平断言绿（接受参数回写档），拒绝连接注入 10/10 一致 |
| UNX-F12608 | 端口区段（Section）共享缓冲→D4 大消息消费 | 300 | 已深化 | UNX-F12608-J1 区段映射大消息消费边对平断言绿（视图大小/偏移档），越界读注入 10/10 拒止 |
| UNX-F12609 | NtReadRequest/NtWriteRequest 消息读写→D4 联签 | 320 | 已深化 | UNX-F12609-J1 读写请求语义（缓冲/数据长度/请求类型）对平断言绿，坏缓冲注入 10/10 一致 |
| UNX-F12610 | NtReplyWaitReceivePortPair 应答等待对→D4 联签 | 300 | 已深化 | UNX-F12610-J1 应答-等待-接收三段对平断言绿，双端口等待语义（Pair 档）抽样 10 例一致 |
| UNX-F12611 | 消息属性（MESSAGE_TYPE/数据长度）对平 | 341 | 已深化 | UNX-F12611-J1 消息类型字段（连接/请求/应答三类）与数据长度语义逐项对平断言绿，类型错标注入 10/10 检出 |
| UNX-F12612 | ALPC 上下文端口与直接/间接消息 | 300 | 已深化 | UNX-F12612-J1 上下文端口（ContextPort）与消息模式（直接/间接）语义档对平断言绿，抽样 10 例一致 |
| UNX-F12613 | 端口句柄传递（句柄复制跨端）联签 | 320 | 已深化 | UNX-F12613-J1 句柄随消息传递协议（复制语义/权限收缩）对平断言绿，坏句柄注入 10/10 拒止 |
| UNX-F12614 | 端口关闭序与未决消息拒止 | 300 | 已深化 | UNX-F12614-J1 关闭序（未决应答/未收消息拒止）判据绿，关闭竞态注入 10/10 次正确拒止 |
| UNX-F12615 | D4 未开工联签预演形态声明 | 342 | 已深化 | UNX-F12615-J1 预演形态三件齐（接口映射表/会签槽位/两态记账声明），槽位预置可点验 10/10 |
| UNX-F12616 | D4 消息面注入联测（坏端口/坏消息/越权连接 30 例） | 300 | 已深化 | UNX-F12616-J1 30 例联测全过，拒止码双侧一致断言绿，零静默 |
| UNX-F12617 | 消息面对照表批入账 | 320 | 已深化 | UNX-F12617-J1 对照表 ≥30 行五列齐（边号/冻结件/承接域/判据/复测），抽样 10 行独立可复测 |
| UNX-F12618 | 消息面性能预算（P95 投递延迟账） | 300 | 已深化 | UNX-F12618-J1 投递/应答 P95 预算在册（O1 配套账），高压注入账无红账 |
| UNX-F12619 | ktest 聚合器（本批 20 条单跑入口） | 341 | 已深化 | UNX-F12619-J1 20 条单跑入口齐，全跑条数与聚合账等式断言记录随批归档可点验 |
| UNX-F12620 | B32 预告与开门件（D4 消息面二：APC 消费边） | 319 | 已深化 | UNX-F12620-J1 B32 主题框架登记入域待办账，与 F12598/F12599 预告链衔接零悬空 |

<!-- 主册行 16048 · #### UNX-D1-B32 · I 型 D1→D4 消息面联签二（APC·异步投递与消息泵消费边）NT API 语义 -->
#### UNX-D1-B32 · I 型 D1→D4 消息面联签二（APC·异步投递与消息泵消费边）NT API 语义档（F12621–F12640 · 20 条）

> AI-16 承办｜批主题：I 型第 4 批——D1→D4 消息面联签二：线程警报与 Alertable 等待联动、用户 APC 投递语义消费边、NtQueueApcThread 联签、内核 APC 越界防护、可警报等待族（NtDelayExecution/NtWaitForSingleObject）、事件对精简唤醒、IO 完成端口语义（NtSetIoCompletion/NtRemoveIoCompletion/完成包三态）、异步错误注入、双机对照、会签槽位回执协议、长稳账、边界样本、两态记账、文档对齐、ktest 聚合、B33 预告开门件｜域账累计：183,066 + 本批 6,326 = 189,392 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）；D4 承接面=USER32/GDI32 语义域（AI-19，联签预演形态，R-D1-030）｜防重：F12621–F12640 唯一；与 B19 线程生命周期/B17 TLS 纤程族分界——本体归前批，本批立 D1→D4 异步投递联签消费面；与 B10 RTL_CRITICAL_SECTION 分界。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12621 | 线程警报与 Alertable 等待→D4 消息泵联动 | 300 | 已深化 | UNX-F12621-J1 警报状态（UserApcPending）与可警报等待联动语义对平断言绿，抽样 10 例一致 |
| UNX-F12622 | 用户 APC 投递语义→D4 消息唤醒消费边 | 340 | 已深化 | UNX-F12622-J1 用户 APC 投递（排队/交付/完成三段）消费对平断言绿，交付时机注入 10/10 一致 |
| UNX-F12623 | NtQueueApcThread→D4 联签 | 280 | 已深化 | UNX-F12623-J1 APC 排队参数（目标线程/例程/上下文）对平断言绿，坏线程句柄注入 10/10 拒止 |
| UNX-F12624 | 内核 APC 越界防护→D4 联签 | 320 | 已深化 | UNX-F12624-J1 KernelMode APC 禁越消费断言绿（PreviousMode 联动回指 B28），越权排队注入 10/10 拒止 |
| UNX-F12625 | NtDelayExecution 可警报等待→D4 泵等待 | 300 | 已深化 | UNX-F12625-J1 可警报延迟等待语义（Alertable=TRUE 优先交付 APC）对平断言绿，双机 10 场景一致 |
| UNX-F12626 | 事件对（EventPair）精简唤醒→D4 联签 | 340 | 已深化 | UNX-F12626-J1 事件对语义（Win8 后弃用档照录注出处）对平断言绿，弃用态声明可点验 |
| UNX-F12627 | NtWaitForSingleObject 可警报等待→D4 联签 | 280 | 已深化 | UNX-F12627-J1 等待语义（超时/警报/唤醒三因）消费对平断言绿，等待打断注入 10/10 一致 |
| UNX-F12628 | IO 完成端口语义→D4 IO 完成消费边 | 320 | 已深化 | UNX-F12628-J1 IoCompletion 对象语义（队列/并发/最大并发档）对平断言绿，并发参数注入 10/10 一致 |
| UNX-F12629 | NtSetIoCompletion/NtRemoveIoCompletion→D4 联签 | 300 | 已深化 | UNX-F12629-J1 完成包入队/出队语义对平断言绿，空队列等待注入 10/10 一致 |
| UNX-F12630 | IO 完成包状态/完成值/完成键对平 | 340 | 已深化 | UNX-F12630-J1 完成包三字段（Status/Information/Key）逐项对平断言绿，字段篡改注入 10/10 检出 |
| UNX-F12631 | 异步投递错误注入（坏 APC/坏完成包 30 例） | 280 | 已深化 | UNX-F12631-J1 30 例联测全过，拒止码双侧一致断言绿，零静默 |
| UNX-F12632 | 双机对照（APC/完成端口 20 场景） | 320 | 已深化 | UNX-F12632-J1 抽样 20 场景双机行为一致（S1 母版），交付时机类零容差 |
| UNX-F12633 | 消息面对照表二批入账 | 300 | 已深化 | UNX-F12633-J1 对照表二批 ≥30 行五列齐，与 F12617 表分批衔接零悬空 |
| UNX-F12634 | D4 会签槽位回执（承接域开工后对账协议） | 340 | 已深化 | UNX-F12634-J1 回执协议三段（开工通知/对账窗口/差异列账）判据绿，两态留痕 |
| UNX-F12635 | 消息泵长稳账（72h 预算登记） | 280 | 已深化 | UNX-F12635-J1 72h 长稳判据登记随闸门补测（双轨产线零 QEMU），预算线在册 |
| UNX-F12636 | 消息面边界样本集（深队列/零长消息/拒止叠加 ≥30 例） | 320 | 已深化 | UNX-F12636-J1 边界样本 ≥30 例入库全过，样本与联测资产同源可重演 |
| UNX-F12637 | 两态记账与防幻觉条款批应用 | 300 | 已深化 | UNX-F12637-J1 两态记账（真绿/桩期绿分列）20/20 齐，偏移/码值出处注记全量核验 |
| UNX-F12638 | 文档对齐（MSD ALPC/IO 完成端口注出处核验） | 346 | 已深化 | UNX-F12638-J1 注出处核验 ≥20 处抽检绿，查无资料项标待实测不虚构 |
| UNX-F12639 | ktest 聚合器（B31–B32 消息面 40 条聚合） | 360 | 已深化 | UNX-F12639-J1 两批 40 条聚合入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12640 | B33 预告与开门件（E1 Wine 探测面联签一） | 360 | 已深化 | UNX-F12640-J1 B33 主题框架登记，与 F12598/F12599/F12620 预告链衔接零悬空 |

<!-- 主册行 16075 · #### UNX-D1-B33 · I 型 D1→E1 Wine 探测面联签一（探测对平三列）NT API 语义档（F1 -->
#### UNX-D1-B33 · I 型 D1→E1 Wine 探测面联签一（探测对平三列）NT API 语义档（F12641–F12660 · 20 条）

> AI-16 承办｜批主题：I 型第 5 批——D1→E1 Wine 探测面联签一：探测面消费边总表、探测对平三列制度（F12599 框架兑现，与 F12561/F12581 同型）、PEB 字段探测面、TEB 槽位探测面、NTSTATUS 返回面侦探、NtQueryInformationProcess/Thread/SystemInformation 探测档、版本分档协议消费（ADR-UNX-008）、对象类型枚举探测、句柄值域探测（-1/-2 特殊句柄）、会签槽位预置（E1 未开工 R-D1-030）、Wine conformance 样本引用协议（对照不抄）、注入联测/对照表/性能预算/边界样本、防幻觉全量核验、ktest 聚合、B34 预告开门件｜域账累计：189,392 + 本批 6,326 = 195,718 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）；E1 承接面=Wine 上栈与 winevarix.drv（AI-21，未开工按联签预演形态推进，R-D1-030 登记）｜防重：F12641–F12660 唯一；与 B03 PEB/B04 TEB 本体分界——本体归前批冻结件，本批立兼容层探测消费面（引用不重定义）；与 B20 查询信息族分界——B20 立查询语义本体。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12641 | D1→E1 探测面消费边总表 | 310 | 已深化 | UNX-F12641-J1 探测面消费边总表八行齐（PEB/TEB/NTSTATUS/Process 查询/Thread 查询/System 查询/版本分档/对象枚举），三列制度列齐 10/10 |
| UNX-F12642 | 探测对平三列制度档 | 290 | 已深化 | UNX-F12642-J1 三列制度（互引/冻结/联测）与 F12561/F12581 同构声明在册，制度档版本号入账 |
| UNX-F12643 | PEB 字段探测面（兼容层读取 PEB 消费边） | 330 | 已深化 | UNX-F12643-J1 PEB 消费字段逐项对平断言绿（ImageBase/OSVersion 等），偏移锚注出处，注入 10/10 一致 |
| UNX-F12644 | TEB 槽位探测面（Self/ClientID/TEB 扩展） | 310 | 已深化 | UNX-F12644-J1 TEB 探测字段对平断言绿（NT_TIB/Self/ClientID 档），Slot 消费抽样 10 例一致 |
| UNX-F12645 | NTSTATUS 返回面探测（兼容层错误码侦探） | 290 | 已深化 | UNX-F12645-J1 错误码侦探消费边对平断言绿（B02 编码面回指），抽样 20 码一致 |
| UNX-F12646 | NtQueryInformationProcess 探测档清单 | 330 | 已深化 | UNX-F12646-J1 信息类清单档（公开类注出处/未公开类标待实测）齐备，类越档注入 10/10 拒止 |
| UNX-F12647 | NtQueryInformationThread 探测档 | 310 | 已深化 | UNX-F12647-J1 线程查询类探测档对平断言绿，坏线程句柄注入 10/10 一致 |
| UNX-F12648 | 版本分档协议消费（PEB OSVersion/版本锚 ADR-UNX-008） | 290 | 已深化 | UNX-F12648-J1 版本分档消费对平断言绿（分档协议回指 B03 冻结件），分档切换注入 10/10 一致 |
| UNX-F12649 | NtQuerySystemInformation 探测面 | 330 | 已深化 | UNX-F12649-J1 系统查询类探测档（SystemProcessInformation 等）对平断言绿，坏类注入 10/10 拒止 |
| UNX-F12650 | 对象类型枚举探测（ObjectTypeInformation） | 310 | 已深化 | UNX-F12650-J1 类型枚举探测对平断言绿（B12 类型清单回指），枚举完整性断言 10/10 |
| UNX-F12651 | 句柄值域探测面（伪句柄/特殊句柄 -1/-2） | 290 | 已深化 | UNX-F12651-J1 特殊句柄值域（CurrentProcess=-1/CurrentThread=-2 档注出处）对平断言绿，越值域注入 10/10 拒止 |
| UNX-F12652 | 会签槽位预置（E1 未开工，R-D1-030 登记） | 330 | 已深化 | UNX-F12652-J1 预演形态三件齐（接口映射表/槽位预置/两态声明），R-D1-030 显式登记可点验 |
| UNX-F12653 | Wine conformance 探测样本引用协议（对照不抄） | 310 | 已深化 | UNX-F12653-J1 样本引用协议（只引结论不抄码）判据绿，引用清单可点验 10/10 |
| UNX-F12654 | 探测面注入联测（探测条件突变 20 例） | 290 | 已深化 | UNX-F12654-J1 20 例突变联测全过（版本/偏移/类号突变），检出断言零静默 |
| UNX-F12655 | 探测面对照表批入账 | 330 | 已深化 | UNX-F12655-J1 对照表 ≥30 行五列齐，抽样 10 行独立可复测 |
| UNX-F12656 | 探测面性能预算（探测调用 P95 账） | 310 | 已深化 | UNX-F12656-J1 探测调用 P95 预算在册（O1 配套账），高频注入账无红账 |
| UNX-F12657 | 探测面边界样本（空参数/越界 ReturnLength/信息类越档） | 290 | 已深化 | UNX-F12657-J1 边界样本 ≥30 例入库全过，与 F12654 联测资产同源可重演 |
| UNX-F12658 | 防幻觉条款批应用（偏移/码值出处全量核验） | 356 | 已深化 | UNX-F12658-J1 本批 20 条出处注记全量核验绿，待实测项清单登记零虚构 |
| UNX-F12659 | ktest 聚合器（B33 20 条单跑入口） | 360 | 已深化 | UNX-F12659-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档可点验 |
| UNX-F12660 | B34 预告与开门件（E1 探测面二：注册表·句柄探测） | 360 | 已深化 | UNX-F12660-J1 B34 主题框架登记，与 F12598→F12640 预告链衔接零悬空 |

<!-- 主册行 16102 · #### UNX-D1-B34 · I 型 D1→E1 Wine 探测面联签二（版本侦探与注册表·句柄探测消费边）NT  -->
#### UNX-D1-B34 · I 型 D1→E1 Wine 探测面联签二（版本侦探与注册表·句柄探测消费边）NT API 语义档（F12661–F12680 · 20 条）

> AI-16 承办｜批主题：I 型第 6 批——D1→E1 Wine 探测面联签二：注册表探测消费边（hive 读→D3 联签面）、CurrentVersion 分支探测档、句柄表探测面（SystemHandleInformation）、NtQueryObject 探测消费回指链、文件对象与 \?? 设备映射探测、命名管道/邮槽探测档、Section/内存探测面、Token 探测面、同步对象探测、进程/线程枚举探测、探测结果一致性协议（快照口径）、注入联测二/双机对照/对照表二/性能预算二/边界样本二、会签槽位回执、文档对齐二、ktest 聚合、B35 预告开门件｜域账累计：195,718 + 本批 6,326 = 202,044 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）；E1 承接面=Wine 上栈（AI-21，联签预演形态，R-D1-030）；D3 消费面回指 B30 联签件｜防重：F12661–F12680 唯一；与 B14 文件族/B13 内存族/B23 Token 族本体分界——本体归前批，本批立探测消费面（引用不重定义）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12661 | 注册表探测消费边（兼容层读 hive→D3 联签面） | 280 | 已深化 | UNX-F12661-J1 注册表探测消费边对平断言绿（D3 键路径回指 B30 联签件），抽样 10 例一致 |
| UNX-F12662 | CurrentVersion 分支探测档（ProductType/CSDVersion） | 320 | 已深化 | UNX-F12662-J1 CurrentVersion 键值探测档逐项对平断言绿，值锚注出处，注入 10/10 一致 |
| UNX-F12663 | 句柄表探测面（SystemHandleInformation） | 300 | 已深化 | UNX-F12663-J1 系统句柄枚举探测档对平断言绿（计数/进程归属字段），权限不足注入 10/10 拒止 |
| UNX-F12664 | NtQueryObject 探测消费（回指 B20/F12386 链） | 340 | 已深化 | UNX-F12664-J1 探测消费回指链零悬空（B20 F12386→B30 F12586→本条），抽样 20 例一致 |
| UNX-F12665 | 文件对象探测（\?? 与 DOS 设备映射消费边） | 280 | 已深化 | UNX-F12665-J1 DOS 设备映射探测对平断言绿（B30 根路径协议回指），映射翻转注入 10/10 一致 |
| UNX-F12666 | 命名管道/邮槽探测档 | 320 | 已深化 | UNX-F12666-J1 管道/邮槽对象类型探测档对平断言绿，类型错配注入 10/10 检出 |
| UNX-F12667 | Section/内存探测面（分配粒度/区域查询） | 300 | 已深化 | UNX-F12667-J1 区域查询探测（MemoryBasicInformation 档/粒度 64KB 注出处）对平断言绿，越区注入 10/10 一致 |
| UNX-F12668 | Token 探测面（Query token 信息类消费边） | 340 | 已深化 | UNX-F12668-J1 Token 信息类探测档（B23 族回指）对平断言绿，坏 Token 注入 10/10 拒止 |
| UNX-F12669 | 同步对象探测（Wait/Semaphore/Mutant 计数探测） | 280 | 已深化 | UNX-F12669-J1 同步对象计数探测档对平断言绿（B15 族回指），计数突变注入 10/10 一致 |
| UNX-F12670 | 进程/线程枚举探测（SystemProcessInformation 消费） | 320 | 已深化 | UNX-F12670-J1 枚举探测消费对平断言绿（B33 F12649 档衔接），快照方差档显式 |
| UNX-F12671 | 探测结果一致性协议（快照口径/方差声明） | 300 | 已深化 | UNX-F12671-J1 快照口径协议判据绿（计数类快照方差显式），方差越界演练 10/10 检出 |
| UNX-F12672 | 探测面注入联测二（坏缓冲/坏类/坏权限 30 例） | 340 | 已深化 | UNX-F12672-J1 30 例联测全过，拒止码双侧一致断言绿，零静默 |
| UNX-F12673 | 双机对照（探测 20 场景一致率） | 280 | 已深化 | UNX-F12673-J1 抽样 20 场景双机一致（S1 母版），码值类零容差 |
| UNX-F12674 | 探测面对照表二批入账 | 320 | 已深化 | UNX-F12674-J1 对照表二批 ≥30 行五列齐，与 F12655 表衔接零悬空 |
| UNX-F12675 | 探测面性能预算二（枚举类 P95 账） | 300 | 已深化 | UNX-F12675-J1 枚举类 P95 预算在册，大表注入账无红账 |
| UNX-F12676 | 探测面边界样本二（超长路径/深枚举/大缓冲） | 340 | 已深化 | UNX-F12676-J1 边界样本 ≥30 例入库全过，与联测资产同源可重演 |
| UNX-F12677 | E1 会签槽位回执协议（承接开工对账） | 286 | 已深化 | UNX-F12677-J1 回执协议三段（通知/窗口/差异列账）判据绿，与 F12634 同构声明 |
| UNX-F12678 | 文档对齐二（Wine conformance/MSD 注出处） | 360 | 已深化 | UNX-F12678-J1 注出处核验 ≥20 处抽检绿，对照不抄声明可点验 |
| UNX-F12679 | ktest 聚合器（B33–B34 探测面 40 条聚合） | 360 | 已深化 | UNX-F12679-J1 两批 40 条聚合入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12680 | B35 预告与开门件（D2·D3 联签回归+总对账） | 360 | 已深化 | UNX-F12680-J1 B35 主题框架登记，与 F12598→F12660 预告链衔接零悬空 |

<!-- 主册行 16129 · #### UNX-D1-B35 · I 型 D2·D3 联签回归与跨域消费面总对账 NT API 语义档（F12681– -->
#### UNX-D1-B35 · I 型 D2·D3 联签回归与跨域消费面总对账 NT API 语义档（F12681–F12700 · 20 条）

> AI-16 承办｜批主题：I 型第 7 批——D2·D3 联签回归与跨域消费面总对账：B29/B30 判据 J1 全量复测回归（装载/句柄表/错误码/OA/LoaderLock/TLS 回调/代数复用/根路径/删除挂起）、跨域消费面总对账表（D2/D3/D4/E1 四方）、消费边接口登记总表、联签两态账汇总、双机账汇总、性能预算总表、风险账总表（R-D1-028/029/030 复核）、防幻觉总核验 ≥50 处、文档对齐总表、ktest 聚合、F12700 域中程对账锚（七百条集成账）｜域账累计：202,044 + 本批 6,326 = 208,370 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）｜防重：F12681–F12700 唯一；回归性质——引用原判据 ID 复测不重定义；F12700 中程锚引用前 699 条判据不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12681 | D2 装载联签回归（B29 判据 J1 全量复测） | 300 | 已深化 | UNX-F12681-J1 B29 二十条 J1 全量复测 20/20 绿，回归差异零静默 |
| UNX-F12682 | D3 句柄表联签回归（B30 判据 J1 全量复测） | 300 | 已深化 | UNX-F12682-J1 B30 二十条 J1 全量复测 20/20 绿，回归差异零静默 |
| UNX-F12683 | 装载错误码联签回归（F12564 回归） | 320 | 已深化 | UNX-F12683-J1 F12564 错误码联签回归一致率 100% 维持，比对器输出归档 |
| UNX-F12684 | OA 消费链回归（B29/B30 OA 双链） | 320 | 已深化 | UNX-F12684-J1 OA 双消费链（B29 装载/B30 对象创建）回归 20/20 绿，链上零断 |
| UNX-F12685 | LoaderLock 联签回归（B29 档） | 280 | 已深化 | UNX-F12685-J1 LoaderLock 递归/延迟加载回归 10/10 绿，R-D1-028 复核留痕 |
| UNX-F12686 | TLS 回调装载序回归（B29 档） | 280 | 已深化 | UNX-F12686-J1 TLS 回调序回归 10/10 绿，回调时机注入一致 |
| UNX-F12687 | 句柄代数复用回归（B30 档 10^3 注入重跑） | 340 | 已深化 | UNX-F12687-J1 复用注入 10^3 重跑零串号维持，种子同源可重演 |
| UNX-F12688 | 根路径协议回归（B30 档） | 340 | 已深化 | UNX-F12688-J1 根路径协议回归 10/10 绿（\?? 与 \BaseNamedObjects），越根拒止维持 |
| UNX-F12689 | 删除挂起协议回归（B30 档） | 300 | 已深化 | UNX-F12689-J1 挂起期访问拒止回归 10/10 维持，R-D1-029 复核留痕 |
| UNX-F12690 | 跨域消费面总对账表（D2/D3/D4/E1 四方） | 300 | 已深化 | UNX-F12690-J1 四方对账表齐（已收口方实对平/未开工方槽位态），两态分列零混报 |
| UNX-F12691 | 消费边接口登记总表（跨批消费边逐条可点验） | 320 | 已深化 | UNX-F12691-J1 登记总表 ≥60 行（B29–B34 消费边全量），三查逐条可点验 |
| UNX-F12692 | 联签两态账汇总（真绿/桩期绿分列） | 320 | 已深化 | UNX-F12692-J1 两态账汇总判据绿（D2/D3 真绿/D4·E1 槽位态），混报注入 10/10 检出 |
| UNX-F12693 | 联签双机账汇总（20+20+20 场景） | 280 | 已深化 | UNX-F12693-J1 三批双机账汇总一致率 ≥95% 维持，CONFLICT 显式 |
| UNX-F12694 | 联签性能预算总表（四域 P95 合账） | 280 | 已深化 | UNX-F12694-J1 四域预算合账齐（O1 配套），超线演练 10/10 告警 |
| UNX-F12695 | 风险账总表（R-D1-028/029/030 复核） | 340 | 已深化 | UNX-F12695-J1 三风险复核留痕（状态/证据/下一步三列），零静默 |
| UNX-F12696 | 防幻觉总核验（偏移/码值出处抽检 ≥50 处） | 340 | 已深化 | UNX-F12696-J1 抽检 ≥50 处出处核验绿，待实测项零虚构 |
| UNX-F12697 | 文档对齐总表（语义确认单对齐） | 300 | 已深化 | UNX-F12697-J1 确认单对齐表齐（批/确认单/版本三列），抽样 10 可点验 |
| UNX-F12698 | ktest 聚合器（B35 回归 20 条） | 346 | 已深化 | UNX-F12698-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12699 | B36 预告与开门件（I 型收官） | 360 | 已深化 | UNX-F12699-J1 B36 主题框架登记，与 F12598→F12680 预告链衔接零悬空 |
| UNX-F12700 | 域中程对账锚（F12001–F12700 七百条集成账） | 360 | 已深化 | UNX-F12700-J1 七百条互引集成账零悬空、域账 208,370/240,000 三源一致对平 |

<!-- 主册行 16156 · #### UNX-D1-B36 · I 型收官（联签一致性总对账 F12720 收官锚+开门件交接）NT API 语义档 -->
#### UNX-D1-B36 · I 型收官（联签一致性总对账 F12720 收官锚+开门件交接）NT API 语义档（F12701–F12720 · 20 条）

> AI-16 承办｜批主题：I 型第 8 批（I 型收官批）——联签一致性总对账协议档、四域消费边矩阵终对账、三冻结件消费覆盖率账、I 型 160 条判据唯一性终验、联签注入样本总账 ≥150 例、双机一致率终账 ≥95%、性能预算终表、回归可重演协议、会签槽位管理制度、跨域冻结接口登记终表、批间互引零悬空终验、联签资产版本化（SHA）、两态记账终账、风险账终表、文档对齐终验、ktest 聚合（六批）、I 型收官账、C 型开门件、B37–B40 预告、F12720 I 型收官锚三断言｜域账累计：208,370 + 本批 6,326 = 214,696 / 240,000｜嫁接源：纯自研域；联签制度承 D1 域三冻结件与跨域联签制度（版本锚 ADR-UNX-008）｜防重：F12701–F12720 唯一；收官性质——引用 B29–B35 判据 ID 复测对账不重定义；F12720 收官锚引用前批判据不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12701 | 联签一致性总对账协议档 | 320 | 已深化 | UNX-F12701-J1 总对账协议档（对账范围/口径/频率三段）判据绿，协议版本号入账 |
| UNX-F12702 | 四域消费边矩阵终对账（D2/D3/D4/E1） | 280 | 已深化 | UNX-F12702-J1 四域矩阵终对账齐（行=消费边/列=对账态），混报注入 10/10 检出 |
| UNX-F12703 | 三冻结件消费覆盖率账（冻结件×消费方覆盖矩阵） | 340 | 已深化 | UNX-F12703-J1 覆盖矩阵齐（3 冻结件×4 消费方=12 格逐格登记），漏格 10/10 检出 |
| UNX-F12704 | I 型 160 条判据唯一性终验 | 300 | 已深化 | UNX-F12704-J1 F12601–F12720 联签段判据 ID 唯一性终验绿，dups 报告仅防重行自引属预期 |
| UNX-F12705 | 联签注入样本总账（B29–B35 注入例汇总 ≥150 例） | 320 | 已深化 | UNX-F12705-J1 注入样本总账 ≥150 例齐（批号/样本/预期三列），抽样 20 例可重演 |
| UNX-F12706 | 联签双机一致率终账（≥95% 目标线） | 280 | 已深化 | UNX-F12706-J1 双机终账一致率 ≥95% 断言绿，CONFLICT 例显式列账 |
| UNX-F12707 | 联签性能预算终表（O1 对标口径） | 340 | 已深化 | UNX-F12707-J1 预算终表齐（I 型八批合账），超线告警演练 10/10 |
| UNX-F12708 | 联签回归可重演协议（种子/资产归档） | 300 | 已深化 | UNX-F12708-J1 可重演协议判据绿（种子/样本/脚本三件归档），重演演练 10/10 一致 |
| UNX-F12709 | 会签槽位管理制度（D4/E1 开工后对账 SLA） | 320 | 已深化 | UNX-F12709-J1 槽位管理制度档（SLA/回执/升级路径三段）判据绿，制度可点验 |
| UNX-F12710 | 跨域冻结接口登记终表（→D2/D3/D4/E1 消费面） | 280 | 已深化 | UNX-F12710-J1 冻结接口终表齐（接口/版本/消费方三列），与 handoff 冻结清单零偏差 |
| UNX-F12711 | I 型批间互引零悬空终验（B29–B35 互引扫描） | 340 | 已深化 | UNX-F12711-J1 互引扫描零悬空断言绿（引用 ID 全部可解析），悬空演练 10/10 检出 |
| UNX-F12712 | 联签资产版本化（对照表/样本库/断言脚本 SHA） | 300 | 已深化 | UNX-F12712-J1 三类资产 SHA-256 冻结在册，版本漂移演练 10/10 检出 |
| UNX-F12713 | I 型两态记账终账 | 320 | 已深化 | UNX-F12713-J1 两态终账齐（真绿/槽位态分列），混报 10/10 检出 |
| UNX-F12714 | I 型风险账终表（R-D1-030/031/032 复核） | 280 | 已深化 | UNX-F12714-J1 三风险复核留痕（状态/证据/下一步），零静默 |
| UNX-F12715 | I 型文档对齐终验 | 340 | 已深化 | UNX-F12715-J1 I 型八批文档对齐终验绿（确认单/出处/版本三查），抽检 ≥30 处 |
| UNX-F12716 | ktest 聚合器（I 型后段六批聚合） | 300 | 已深化 | UNX-F12716-J1 六批 120 条聚合入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12717 | I 型收官账（120 条集成账等式） | 320 | 已深化 | UNX-F12717-J1 I 型后段 120 条集成账等式断言绿（批账×6=120 对平） |
| UNX-F12718 | C 型开门件（对照表全量回归框架） | 326 | 已深化 | UNX-F12718-J1 C 型回归框架交付齐（三类对照表盘点+回归矩阵），框架可点验 |
| UNX-F12719 | B37–B40 预告（C 型四批框架） | 360 | 已深化 | UNX-F12719-J1 四批框架登记（对照表全量回归/SEH 压测核账/文档对齐/域收官），衔接零悬空 |
| UNX-F12720 | I 型收官锚（三断言：判据唯一/消费覆盖/行数对平） | 360 | 已深化 | UNX-F12720-J1 三断言绿：I 型 160 条判据唯一、四域消费覆盖矩阵齐、域账 214,696/240,000 三源一致对平 |

<!-- 主册行 16183 · #### UNX-D1-B37 · C 型对照表全量回归 NT API 语义档（F12721–F12740 · 20 条 -->
#### UNX-D1-B37 · C 型对照表全量回归 NT API 语义档（F12721–F12740 · 20 条）

> AI-16 承办｜批主题：C 型第 1 批——对照表全量回归：对照表资产盘点总纲、NT→Dos 错误码对照全量回归 ≥100 对、OBJ_* 位族对照回归、PEB/TEB 偏移对照表回归、UNICODE_STRING/.pdata/UNWIND_INFO/CONTEXT/RTL_CRITICAL_SECTION 语义对照回归、OA 六字段对照、同步族/内存族/文件族/注册表族/令牌族对照回归、差异列账三态零静默、双机终账、ktest 聚合、B38 预告开门件｜域账累计：214,696 + 本批 6,326 = 221,022 / 240,000｜嫁接源：纯自研域；对照制度承版本锚 ADR-UNX-008（Windows 公开资料对齐，ReactOS 仅源码对照参考）｜防重：F12721–F12740 唯一；回归性质——引用 B01–B30 判据 ID 复测不重定义；与本批各原批对照表分界——本批为全量回归合账批。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12721 | 对照表全量回归总纲（三类对照表盘点） | 310 | 已深化 | UNX-F12721-J1 三类对照表盘点齐（错误码类/结构偏移类/语义行为类），盘点清单可点验 10/10 |
| UNX-F12722 | NT→Dos 错误码对照全量回归（≥100 对） | 330 | 已深化 | UNX-F12722-J1 ≥100 对全量回归一致率 100% 维持，差异例三态列账零静默 |
| UNX-F12723 | OBJ_* 位族对照回归 | 290 | 已深化 | UNX-F12723-J1 OBJ_* 位族（INHERIT 等全档）对照回归 10/10 一致，位值锚注出处 |
| UNX-F12724 | PEB 字段对照表回归（偏移表逐行） | 300 | 已深化 | UNX-F12724-J1 PEB 偏移对照全行回归绿（偏移/类型/版本分档三列），偏移漂移演练 10/10 检出 |
| UNX-F12725 | TEB 槽位对照表回归 | 310 | 已深化 | UNX-F12725-J1 TEB 槽位对照全行回归绿，槽位漂移演练 10/10 检出 |
| UNX-F12726 | UNICODE_STRING 语义对照回归 | 330 | 已深化 | UNX-F12726-J1 UNICODE_STRING 语义（Upcase 折叠/UTF-16 码元）对照回归 10/10 一致 |
| UNX-F12727 | .pdata/RUNTIME_FUNCTION 对照回归 | 290 | 已深化 | UNX-F12727-J1 RUNTIME_FUNCTION 结构对照回归绿（Begin/End/Unwind 三字段），坏表拒止维持 |
| UNX-F12728 | UNWIND_INFO 码族对照回归 | 300 | 已深化 | UNX-F12728-J1 UNWIND_INFO 码族对照回归绿（版本/标志/码族档），码值锚注出处 |
| UNX-F12729 | CONTEXT 帧布局对照回归 | 310 | 已深化 | UNX-F12729-J1 CONTEXT 帧布局对照回归绿（x64 寄存器组档），布局漂移演练 10/10 检出 |
| UNX-F12730 | RTL_CRITICAL_SECTION 位约定对照回归 | 330 | 已深化 | UNX-F12730-J1 LockCount 位约定对照回归绿（B10 档），双源漂移注入维持 10/10 |
| UNX-F12731 | OA 六字段对照回归 | 290 | 已深化 | UNX-F12731-J1 OA 六字段对照回归绿（冻结件逐字段），字段漂移演练 10/10 检出 |
| UNX-F12732 | 同步族语义对照回归（Wait/Event/Semaphore/Mutant） | 300 | 已深化 | UNX-F12732-J1 同步族五态等待码对照回归绿，Abandoned Mutant 语义维持 |
| UNX-F12733 | 内存族 Virtual* 对照回归（三态六弧） | 310 | 已深化 | UNX-F12733-J1 Virtual* 三态状态机六弧对照回归绿，PAGE_* 全集矩阵维持 |
| UNX-F12734 | 文件族 NativeFile* 对照回归（disposition 五档） | 330 | 已深化 | UNX-F12734-J1 disposition 五档对照回归绿，IOSB 交付语义维持 |
| UNX-F12735 | 注册表族对照回归 | 290 | 已深化 | UNX-F12735-J1 注册表族（B24 档）对照回归绿，键路径语义维持 |
| UNX-F12736 | 令牌族对照回归 | 300 | 已深化 | UNX-F12736-J1 令牌族（B23 档）对照回归绿，特权调整语义维持 |
| UNX-F12737 | 对照回归差异列账（三态登记零静默） | 326 | 已深化 | UNX-F12737-J1 差异列账三态（本体差异/待归一/真差异）判据绿，静默演练 10/10 检出 |
| UNX-F12738 | 对照回归双机终账 | 360 | 已深化 | UNX-F12738-J1 双机终账一致率 ≥95%（码值/偏移类零容差），CONFLICT 显式 |
| UNX-F12739 | ktest 聚合器（B37 全量回归 20 条） | 360 | 已深化 | UNX-F12739-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12740 | B38 预告与开门件（SEH 压测核账框架） | 360 | 已深化 | UNX-F12740-J1 B38 压测框架登记（矩阵/预算/双机三段），与 F12719 预告链衔接零悬空 |

<!-- 主册行 16210 · #### UNX-D1-B38 · C 型 SEH 压测核账 NT API 语义档（F12741–F12760 · 20 -->
#### UNX-D1-B38 · C 型 SEH 压测核账 NT API 语义档（F12741–F12760 · 20 条）

> AI-16 承办｜批主题：C 型第 2 批——SEH 压测核账：压测矩阵总纲、异常分发嵌套深度压测（1–32 层）、展开链交叉压测（局部/全局展开）、UWOP 全码族压测回归、坏 UNWIND_INFO 拒止压测（fail-closed）、CONTEXT 陷阱帧一致性压测、RTL_CRITICAL_SECTION 双源漂移压测、与 B25/B26 注入矩阵回归联动、断言账本 40 拍对账、压测性能预算（P95 分发延迟）、双机对照、边界样本、两态记账、文档对齐、风险账复核（R-D1-008）、ktest 聚合、收官账、资产归档、SEH 全域回归声明（B07/B08/B11→B38 链零悬空）｜域账累计：221,022 + 本批 6,326 = 227,348 / 240,000｜嫁接源：纯自研域；语义锚承版本锚 ADR-UNX-008（MSD SEH/Windows Internals 对照不抄）｜防重：F12741–F12760 唯一；与 B07/B08/B11 SEH 本体分界——本体归前批冻结判据，本批为压测核账合账批（引用复测不重定义）。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12741 | SEH 压测核账总纲（压测矩阵登记） | 340 | 已深化 | UNX-F12741-J1 压测矩阵登记齐（深度/展开/码族/坏表四轴），矩阵可点验 10/10 |
| UNX-F12742 | 异常分发路径压测（嵌套深度 1–32 层） | 280 | 已深化 | UNX-F12742-J1 嵌套 1–32 层分发压测全过，深栈边界（32 层界碑）10/10 一致 |
| UNX-F12743 | 展开链压测（局部展开/全局展开交叉） | 300 | 已深化 | UNX-F12743-J1 局部/全局展开交叉压测全过，展开序判据 10/10 一致 |
| UNX-F12744 | UWOP 全码族压测回归（UWOP_PUSH_* 等档） | 320 | 已深化 | UNX-F12744-J1 UWOP 码族全档压测回归绿，码值锚注出处，演练 10/10 |
| UNX-F12745 | 坏 UNWIND_INFO 拒止压测（fail-closed） | 340 | 已深化 | UNX-F12745-J1 坏表拒止压测 20/20（截断/环/越界三态），fail-closed 语义维持 |
| UNX-F12746 | CONTEXT 陷阱帧一致性压测（CONTEXT 保存恢复） | 280 | 已深化 | UNX-F12746-J1 保存/恢复一致性压测 10/10，寄存器组逐段对平 |
| UNX-F12747 | RTL_CRITICAL_SECTION 双源漂移压测 | 300 | 已深化 | UNX-F12747-J1 双源漂移压测维持 10/10（B10 档回指），漂移即告警零静默 |
| UNX-F12748 | 异常注入与 B25/B26 矩阵回归联动 | 320 | 已深化 | UNX-F12748-J1 联动回归 20/20（B25 注入矩阵×B26 坏表拒止），链上零断 |
| UNX-F12749 | 断言账本压测对账（40 拍口径） | 340 | 已深化 | UNX-F12749-J1 断言账本 40 拍对账绿（A5 ktest 口径回指），账实差零静默 |
| UNX-F12750 | 压测性能预算（P95 分发延迟） | 280 | 已深化 | UNX-F12750-J1 分发 P95 预算在册（O1 配套账），高压注入账无红账 |
| UNX-F12751 | 压测双机对照（20 场景） | 300 | 已深化 | UNX-F12751-J1 双机 20 场景一致（S1 母版），展开序类零容差 |
| UNX-F12752 | 压测边界样本（深嵌套+并发展开 ≥30 例） | 320 | 已深化 | UNX-F12752-J1 边界样本 ≥30 例入库全过，与联测资产同源可重演 |
| UNX-F12753 | 压测两态记账 | 340 | 已深化 | UNX-F12753-J1 两态记账齐（真绿/桩期绿分列），混报注入 10/10 检出 |
| UNX-F12754 | SEH 文档对齐（MSD SEH 注出处核验） | 280 | 已深化 | UNX-F12754-J1 注出处核验 ≥20 处抽检绿，查无资料标待实测 |
| UNX-F12755 | 风险账复核（R-D1-008 真机判据悬置复查） | 300 | 已深化 | UNX-F12755-J1 R-D1-008 复核留痕（悬置项/证据/下一步三列），零静默 |
| UNX-F12756 | ktest 聚合器（B38 压测 20 条） | 320 | 已深化 | UNX-F12756-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12757 | SEH 压测收官账（等式断言） | 340 | 已深化 | UNX-F12757-J1 压测收官账等式断言绿（矩阵×样本×结果三账对平） |
| UNX-F12758 | B39 预告与开门件（文档对齐框架） | 306 | 已深化 | UNX-F12758-J1 B39 对齐框架登记（盘点/复核/冻结三段），衔接零悬空 |
| UNX-F12759 | 压测资产归档（种子/样本/断言脚本 SHA） | 360 | 已深化 | UNX-F12759-J1 三类资产 SHA-256 冻结在册，版本漂移演练 10/10 检出 |
| UNX-F12760 | SEH 全域回归声明（B07/B08/B11→B38 链零悬空） | 360 | 已深化 | UNX-F12760-J1 SEH 全链回归声明判据绿（四批互引扫描零悬空），链账归档可点验 |

<!-- 主册行 16237 · #### UNX-D1-B39 · C 型文档对齐 NT API 语义档（F12761–F12780 · 20 条） -->
#### UNX-D1-B39 · C 型文档对齐 NT API 语义档（F12761–F12780 · 20 条）

> AI-16 承办｜批主题：C 型第 3 批——文档对齐：域文档资产盘点总纲、语义确认单全量对齐（800 条出处抽检）、版本锚 ADR-UNX-008 全批复核、防幻觉高压线终核验（待实测清单终账）、偏移表出处核验（PEB/TEB 逐行）、NTSTATUS 码值出处核验（B02 逐族）、结构布局出处核验（OA/CONTEXT/UNWIND_INFO）、语义分界声明核验（D1–D3 分界/D4·E1 预演）、术语表一致性核验、域经十条撰写、域文档快照 SHA-256 冻结、差异列账零静默、双机抽验、ktest 聚合、收官账、B40 预告开门件、深化指引继承档、文档资产移交清单、待实测清单移交、域文档收官锚（对齐率 100%）｜域账累计：227,348 + 本批 6,326 = 233,674 / 240,000｜嫁接源：纯自研域；版本锚 ADR-UNX-008 全批适用｜防重：F12761–F12780 唯一；对齐性质——引用原判据 ID 复测不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12761 | 文档对齐总纲（域文档资产盘点） | 290 | 已深化 | UNX-F12761-J1 文档资产盘点齐（确认单/对照表/样本库/ADR 四类），盘点清单可点验 10/10 |
| UNX-F12762 | 语义确认单全量对齐（800 条出处抽检） | 310 | 已深化 | UNX-F12762-J1 确认单对齐率 100% 断言绿（40 批逐批核对），抽检 ≥50 处 |
| UNX-F12763 | 版本锚 ADR-UNX-008 全批复核 | 330 | 已深化 | UNX-F12763-J1 ADR-UNX-008 全批复核绿（40 批版本锚声明逐批在册），缺锚演练 10/10 检出 |
| UNX-F12764 | 防幻觉高压线终核验（待实测清单终账） | 280 | 已深化 | UNX-F12764-J1 待实测清单终账齐（项/批号/依据三列），虚构注入 10/10 检出 |
| UNX-F12765 | 偏移表出处核验（PEB/TEB 逐行注出处） | 290 | 已深化 | UNX-F12765-J1 偏移表逐行出处核验绿（B03/B04 冻结表），无出处行零容忍 |
| UNX-F12766 | NTSTATUS 码值出处核验（B02 面逐族） | 310 | 已深化 | UNX-F12766-J1 码值出处核验 ≥100 码抽检绿，自造码演练 10/10 检出 |
| UNX-F12767 | 结构布局出处核验（OA/CONTEXT/UNWIND_INFO） | 330 | 已深化 | UNX-F12767-J1 三结构布局出处核验绿，布局漂移演练 10/10 检出 |
| UNX-F12768 | 语义分界声明核验（D1–D3 分界/D4·E1 预演） | 280 | 已深化 | UNX-F12768-J1 分界声明核验绿（D1–D3 三分界+D4/E1 预演声明），越界声明 10/10 检出 |
| UNX-F12769 | 术语表一致性核验（全册术语统一） | 290 | 已深化 | UNX-F12769-J1 术语一致性核验绿（核心术语 ≥40 条全册统一），漂移演练 10/10 检出 |
| UNX-F12770 | 域经十条撰写（域经验沉淀档） | 310 | 已深化 | UNX-F12770-J1 域经十条齐（经验/适用面/证据三列），逐条可点验 |
| UNX-F12771 | 域文档快照 SHA-256 冻结 | 330 | 已深化 | UNX-F12771-J1 四类资产 SHA-256 冻结在册，版本漂移演练 10/10 检出 |
| UNX-F12772 | 文档差异列账（发现即登记零静默） | 280 | 已深化 | UNX-F12772-J1 差异列账判据绿（三态登记），静默演练 10/10 检出 |
| UNX-F12773 | 文档对齐双机抽验 | 290 | 已深化 | UNX-F12773-J1 双机抽验 10 场景一致（文档行为面），CONFLICT 显式 |
| UNX-F12774 | ktest 聚合器（B39 20 条） | 310 | 已深化 | UNX-F12774-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档 |
| UNX-F12775 | 文档对齐收官账（等式断言） | 330 | 已深化 | UNX-F12775-J1 对齐收官账等式断言绿（资产×抽检×差异三账对平） |
| UNX-F12776 | B40 预告与开门件（域收官清单） | 360 | 已深化 | UNX-F12776-J1 B40 收官清单登记（回归/封账/移交三段），衔接零悬空 |
| UNX-F12777 | 深化指引继承档（承接会话指引） | 360 | 已深化 | UNX-F12777-J1 深化指引档齐（体例/硬闸/复测三段），可点验 10/10 |
| UNX-F12778 | 文档资产移交清单 | 360 | 已深化 | UNX-F12778-J1 移交清单齐（四类资产逐项登记），交接演练 10/10 |
| UNX-F12779 | 待实测清单移交（随闸门补测项汇总） | 360 | 已深化 | UNX-F12779-J1 待实测移交清单齐（项/判据/闸门条件三列），零虚构 |
| UNX-F12780 | 域文档收官锚（对齐率 100% 断言） | 326 | 已深化 | UNX-F12780-J1 对齐率 100% 三断言绿（确认单/出处/术语），域文档闭账声明在册 |

<!-- 主册行 16264 · #### UNX-D1-B40 · C 型域收官（F12800 满账标志三断言+域账封账+号面冻结+移交包）NT API -->
#### UNX-D1-B40 · C 型域收官（F12800 满账标志三断言+域账封账+号面冻结+移交包）NT API 语义档（F12781–F12800 · 20 条）

> AI-16 承办｜批主题：C 型第 4 批（域收官批）——域级总回归矩阵（40 批全框架）、号面冻结终版（F12001–F12800 零重编声明）、升级接管清册（ntdll 语义档→内核接管路径）、域收官审计对照、域账封账条（240,000/240,000 守恒终验）、域闭账条（800 条集成账零悬空）、判据终账（800 判据唯一性终验）、行数终账（三源一致终断言）、风险账移交（R-D1 全清单）、跨域冻结件移交（三冻结→D2/D3/D4/E1）、移交包五件（handoff/总纲/根台账/ktest 资产/复测资产库）、双轨产线声明、真机判据随闸门补测清单、F12800 前哨互引预验、F12799 域收官标志条、F12800 满账收官条（域 40 批 800 条满账宣言）｜域账累计：233,674 + 本批 6,326 = **240,000 / 240,000（域满账封账）**｜嫁接源：纯自研域；版本锚 ADR-UNX-008 全批适用｜防重：F12781–F12800 唯一；收官性质——引用前 39 批判据 ID 复测不重定义；F12799/F12800 里程碑条引用前 799 条判据不重定义。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F12781 | 域级总回归矩阵（40 批全框架回归） | 320 | 已深化 | UNX-F12781-J1 40 批回归矩阵齐（批×回归态两列），全量复测入口可点验 |
| UNX-F12782 | 号面冻结终版（F12001–F12800 零重编声明） | 320 | 已深化 | UNX-F12782-J1 号面冻结声明判据绿（800 号连续零空洞零重编），越号注入 10/10 检出 |
| UNX-F12783 | 升级接管清册（ntdll 语义档→内核接管路径） | 300 | 已深化 | UNX-F12783-J1 接管清册齐（语义档/接管点/回归门槛三列），路径可点验 |
| UNX-F12784 | 域收官审计对照（验收协议对照） | 300 | 已深化 | UNX-F12784-J1 审计对照表齐（验收条款×证据两列），逐条可点验 |
| UNX-F12785 | 域账封账条（240,000/240,000 守恒终验） | 340 | 已深化 | UNX-F12785-J1 域账 240,000/240,000 守恒终验绿（86,740+90,000+63,260 加法账零偏离） |
| UNX-F12786 | 域闭账条（800 条集成账零悬空） | 340 | 已深化 | UNX-F12786-J1 800 条集成账零悬空终验绿（互引全扫描），悬空注入 10/10 检出 |
| UNX-F12787 | 判据终账（800 判据 ID 唯一性终验） | 280 | 已深化 | UNX-F12787-J1 800 判据 ID 全域唯一终验绿，dups 报告仅防重行自引属预期 |
| UNX-F12788 | 行数终账（三源一致终断言） | 280 | 已深化 | UNX-F12788-J1 三源一致终断言绿（骨架台账/深化册递推/域总账逐批一致） |
| UNX-F12789 | 风险账移交（R-D1-001/005/008/021–039 清单） | 320 | 已深化 | UNX-F12789-J1 风险移交清单齐（ID/状态/下一步三列），零静默 |
| UNX-F12790 | 跨域冻结件移交（三冻结→D2/D3/D4/E1） | 320 | 已深化 | UNX-F12790-J1 冻结件移交判据绿（三冻结×四方消费面登记），handoff 清单零偏差 |
| UNX-F12791 | 移交包一：handoff 域块收官声明 | 300 | 已深化 | UNX-F12791-J1 handoff 收官声明判据绿（finalized=40/rows=240,000/entries=800），字段可点验 |
| UNX-F12792 | 移交包二：总纲 §7.3-D1 满账收官行 | 300 | 已深化 | UNX-F12792-J1 总纲四十行 [已深化] 终态判据绿，域小结收官条款同步 |
| UNX-F12793 | 移交包三：根台账四处同步声明 | 340 | 已深化 | UNX-F12793-J1 根台账 §二/§三/§四/§六 同步声明判据绿，账目可点验 |
| UNX-F12794 | 移交包四：ktest 全域断言资产 | 340 | 已深化 | UNX-F12794-J1 全域断言资产齐（40 批聚合器入口清单），抽样 10 批可跑 |
| UNX-F12795 | 移交包五：复测资产库（注入/双机/边界样本） | 280 | 已深化 | UNX-F12795-J1 复测资产库齐（三类样本汇总 ≥200 例），同源可重演 |
| UNX-F12796 | 双轨产线声明（开发期零 QEMU 纪律全域适用） | 280 | 已深化 | UNX-F12796-J1 双轨声明判据绿（真机判据全量登记"随闸门补测"），零 QEMU 纪律可点验 |
| UNX-F12797 | 真机判据随闸门补测清单 | 320 | 已深化 | UNX-F12797-J1 补测清单齐（判据号/条件/预算三列），与 handoff 开放风险零偏差 |
| UNX-F12798 | F12800 前哨：八百条互引零悬空预验 | 326 | 已深化 | UNX-F12798-J1 互引预验绿（F12001–F12799 全扫描），悬空注入 10/10 检出 |
| UNX-F12799 | 域收官标志条（域 40 批 800 条满账宣言+三断言） | 360 | 已深化 | UNX-F12799-J1 三断言绿：40 批齐备、800 判据唯一、域账 233,674+6,326=240,000 满账对平 |
| UNX-F12800 | 满账收官条（域账封账 240,000/240,000+号面冻结+移交包完成三断言） | 360 | 已深化 | UNX-F12800-J1 三断言绿：域账封账 240,000/240,000、号面 F12001–F12800 冻结终版、移交包五件齐——域 40 批 800 条满账收口宣言 |

<a id="dom-D2"></a>

