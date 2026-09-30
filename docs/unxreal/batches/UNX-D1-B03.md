# UNX-D1-B03 · PEB 深化（F12041–F12060 · 20 条）

> AI-16 承办｜批主题：PEB 逐字段深化——偏移总账/调试位/ApiSet/TLS/版本字段/写路径白名单/静态断言器/对照表制度｜域账累计：11,680 + 本批 5,480 = 17,160 / 240,000｜嫁接源：纯自研域；偏移数值锚=Windows Internals 7th Part1 ch.5 与公开 PEB 布局资料（版本锚 ADR-UNX-008 波 07 前钉定 Win10/11 主流面），ReactOS 对照不抄｜防重：F12041–F12060 唯一；与 F12005（B01 PEB 实装）为分层深化——F12005 立结构与快照协议，本批立逐字段语义档与防御面，判据无重叠｜防幻觉高压线：每个偏移注出处章节号，查无资料标"待基准机实测"。

### UNX-F12041 · PEB 字段总账：x64 逐字段偏移表与版本分档
- 域/批：D1/B03｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12041-J1 x64 PEB 全字段偏移表（BeingDebugged 0x2 起至 GdiHandleBuffer）逐字段注出处，Win10/11 两档差异列分列，抽样 20 字段双机偏移零差异
### UNX-F12042 · ImageBaseAddress 语义与 D2 装载交接挂点
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12042-J1 映像基址写入时机与 D2 装载握手协议一致，读回值与实际装载基址逐字节一致 50/50 次
### UNX-F12043 · ProcessHeap 指针族与默认堆挂点（D2 联签）
- 域/批：D1/B03｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12043-J1 ProcessHeap 指向 D2 默认堆实例，堆标志（Flags/ForceFlags）读回与堆初始化参数一致 10/10 组
### UNX-F12044 · BeingDebugged/NtGlobalFlag 调试位语义与唯一写者
- 域/批：D1/B03｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12044-J1 调试器挂接/摘除时 BeingDebugged 翻转可观测，NtGlobalFlag 位集与调试选项映射一致，用户态写 10/10 次被拒
### UNX-F12045 · ApiSetMap 指针语义与 API Set 解析预告
- 域/批：D1/B03｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12045-J1 ApiSetMap 指针落位与契约解析器对接判据绿，同机 Windows api-ms-* 重定向抽样 10 例行为一致（解析主体归 D2 防重）
### UNX-F12046 · PEB TlsSlot 数组与 TlsExpansionBitmap
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12046-J1 TlsSlot[64] 与扩展位图分配/回收守恒（分配位-回收位差为零），64 槽耗尽路径返回正确错误非静默
### UNX-F12047 · PEB LoaderLock 与 LdrLockLoaderLock 占位语义
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12047-J1 LoaderLock 结构挂点落位，锁语义判据（重入计数/所有者线程）与版本锚一致，死锁注入走 A5 检测器联签
### UNX-F12048 · PEB 版本字段账：OsMajorVersion/OsMinorVersion/OsBuildNumber
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12048-J1 版本三元组与版本锚钉定档一致，双机同版本读回逐值一致，版本注入（伪装低版本）10/10 次正确反映
### UNX-F12049 · PEB CSDVersion 与 NTDDI 版本对照
- 域/批：D1/B03｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F12049-J1 CSDVersion UNICODE_STRING 语义档齐，服务包位语义与版本锚一致（Win10+ 恒空档有账）
### UNX-F12050 · PEB FastPebLock 语义
- 域/批：D1/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12050-J1 FastPebLock 争用路径（快锁自旋）语义档齐，双线程争用压测零竞态（M3 母版）
### UNX-F12051 · PEB GdiHandleBuffer 与 GDI 联签预告（F 部）
- 域/批：D1/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12051-J1 GdiHandleBuffer 槽位数与版本锚一致，GDI 句柄计数联签接口冻结（消费方 F 部）
### UNX-F12052 · PEB ActivationContextData 指针占位语义
- 域/批：D1/B03｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12052-J1 激活上下文指针族（ActivationContextData/ProcessAssemblyStorageMap/SystemAssemblyStorageMap）占位语义档齐，空值路径有账非静默
### UNX-F12053 · PEB AtlThunkSListPtr 检测面占位
- 域/批：D1/B03｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F12053-J1 AtlThunkSListPtr/AtlThunkSListLock 挂点语义档齐，读回缺省值与版本锚一致
### UNX-F12054 · PEB 内核写路径白名单：可写字段全集与审计账
- 域/批：D1/B03｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12054-J1 可写字段白名单全集登记（BeingDebugged/NtGlobalFlag/ImageBaseAddress/ProcessParameters/ProcessHeap 等），白名单外内核写 10/10 次红账拦截，审计账逐笔可查
### UNX-F12055 · PEB 跨线程读一致性：只读快照与撕裂防御
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12055-J1 8 线程并发读快照页 10^6 次零撕裂（校验和防御），内核写期间读者见一致快照
### UNX-F12056 · 8 内联读取 API 双机对照底座
- 域/批：D1/B03｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12056-J1 IsDebuggerPresent/GetCurrentProcessId/GetCurrentThreadId/GetCurrentProcess/NtCurrentTeb 等 8 API 同码双机逐值一致（S1 抽样母版）
### UNX-F12057 · PEB 越界写检测与红账
- 域/批：D1/B03｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12057-J1 快照页前后哨兵注入越界写 10/10 次定位到越界偏移并红账，不崩内核
### UNX-F12058 · PEB 布局静态断言器（const 断言集）
- 域/批：D1/B03｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12058-J1 编译期 const 断言覆盖全字段偏移（≥30 条），人为改错一偏移编译即失败（断言防手滑）
### UNX-F12059 · PEB 对照表制度：逐字段语义档与可复测判据
- 域/批：D1/B03｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12059-J1 全字段入对照表（字段名/偏移/版本档/出处/判据 ID 五列齐），抽样 30 字段判据可独立复测
### UNX-F12060 · ktest PEB 面断言集（B03 批判据聚合）
- 域/批：D1/B03｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12060-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，偏移断言族与写路径白名单族独立可单跑
