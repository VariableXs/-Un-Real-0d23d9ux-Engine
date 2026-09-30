# UNX-G4-B01 · 域地基批：D3D12 设备对象与三队列族映射（F26401–F26420 · 20 条）

> AI-34 承办（波 15 首产段立账 · 一次对话 300 项明令）｜批主题：域地基批：D3D12 设备对象与三队列族映射｜域账累计：6000 / 240,000｜判据主轴：vkd3d 测试集全绿账（豁免挂 ADR ≤5%）+ D3D12 samples 20/20 全链账 + 三面 Windows 对照差=0（J-测/J-样/J-根/J-栅/J-稳）｜嫁接源：vkd3d-proton 波14 基线版（钉定，波内禁升，升级全量重跑）＋D3D12 规范 promote/decay 实测锚（ADR-UNX-008）＋Wine/vkd3d 语义参考（只跟随）｜防重：全域 ID F26401–F27200 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、已 finalize deepen 册、总纲既有段）｜上游：AI-32（Vulkan 设备面/WSI · 接口⑨消费方 · Schema 先行+fake 对接，B24 冻结签为前置）、AI-31（KMD 显存域/驻留事件）；横向 AI-33（DXGI 共享面与 hack 账基础设施 · B01 前联签，实现归属以 ID 段为准）；下游 AI-70（N5 收口消费样本账）｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；DXR 档位声明禁虚标（非 RT 机不虚报不冒充）；豁免不入账=虚报（同 AI-32 纪律）；体验日志/异常显性化/交互词典三条间接纪律全程生效

### UNX-F26401 · D3D12 设备创建与特性查询总入口（任务书示例锚·三队列族映射）
- 域/批：G4/B01｜纯功能行数：520｜状态：[骨架]｜判据：UNX-F26401-J1 D3D12CreateDevice→VkDevice 建链可测（feature 校验→版本仲裁→设备句柄发放全链）；三队列族（direct/compute/copy）功能探针全过；ExecuteCommandLists 批量提交语义与 Windows 对照一致（母版 M7 双机同码）；设备丢失→DXGI_ERROR_DEVICE_REMOVED 错误码一致
### UNX-F26402 · ID3D12Device 生命周期与引用计数语义
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26402-J1 引用计数 AddRef/Release 链 10^4 次零泄漏（计数账逐次可查）；析构序（子对象先行）与 Windows 一致；泄漏看门狗扫描 <5s 告警三要素呈现
### UNX-F26403 · CheckFeatureSupport 五级特性查询矩阵
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26403-J1 Options/FeatureLevels/FormatSupport/MultisampleQualityLevels/RootSignature 五级查询黄金样本 ≥40 组全过；返回值与实际能力一致性断言（禁虚报）；未知 D3D12_FEATURE 值→E_INVALIDARG 精确单一错误码
### UNX-F26404 · CreateCommandQueue 三类型与优先级语义
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26404-J1 DIRECT/COMPUTE/COPY 三型建队列各 20 次全过；PRIORITY 四档（NORMAL/HIGH/GLOBAL_REALTIME）映射 VkQueue 优先级断言；非法组合（COPY 型挂 REALTIME）拒止错误码与 Windows 一致
### UNX-F26405 · 命令队列提交批量化与提交序恒定账
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26405-J1 ExecuteCommandLists 批量（1/10/100 list）提交序与调用序全序一致断言 10^3 次；空 list 提交为合法 no-op 断言；提交在途时队列销毁→正确错误码不崩溃
### UNX-F26406 · CreateCommandAllocator 与 pool 映射地基
- 域/批：G4/B01｜纯功能行数：190｜状态：[骨架]｜判据：UNX-F26406-J1 allocator→VkCommandPool 一一映射断言；同 allocator 双 list 并发录制→Windows 语义原文校验错误（DXGI_ERROR_INVALID_CALL）精确返回 50/50
### UNX-F26407 · CreateCommandList 三型与初始态语义
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26407-J1 DIRECT/BUNDLE/COPY 三型 list 创建全过；新建 list 初始 RECORDING 态断言（Close 后 Reset 才可录制链）；Reset 与 allocator 在途提交竞态→显性错误不静默
### UNX-F26408 · CreateFence 与 timeline semaphore 映射地基
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26408-J1 fence 初始值/SIGNAL/WAIT 三操作黄金剧本 30 组全过；fence→VkSemaphore(VK_SEMAPHORE_TYPE_TIMELINE) 映射断言；fence 值倒退 wait 立即返回不挂起（怪癖复刻）断言
### UNX-F26409 · CreateDescriptorHeap 四类堆骨架（地基登记）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26409-J1 CBV_SRV_UAV/SAMPLER/RTV/DSV 四类创建与 heap type 校验 20 组全过；shader-visible 与 non-visible 双堆标志语义断言；非法 flags 拒止与 Windows 一致
### UNX-F26410 · CreateCommittedResource 三默认域映射（预告深水）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26410-J1 DEFAULT/UPLOAD/READBACK 三域 committed 资源创建映射 VkImage/VkBuffer 断言；heap properties 与内存域对应表 20 格全过；本条只立地基，深水归 B12
### UNX-F26411 · GetPrivateData/SetPrivateData 语义
- 域/批：G4/B01｜纯功能行数：190｜状态：[骨架]｜判据：UNX-F26411-J1 private data 槽位 10^3 次写读删全过；GUID 冲突覆盖语义断言；对象销毁后查询→S_FALSE 精确语义
### UNX-F26412 · 对象命名与调试信息面（SetName/DebugLayer 挂点）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26412-J1 SetName→Vulkan debug object name 传递断言；debug layer 未启用时零开销（探针实测）断言；长名（>256）截断策略登记
### UNX-F26413 · 设备创建失败路径矩阵（E 型地基）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26413-J1 不支持版本/无效 flags/驱动缺失/OOM 四路失败注入各 50 次零崩溃；错误三要素（发生什么/为什么/下一步）呈现断言；失败后零资源残留（句柄账闭合）
### UNX-F26414 · 接口⑨消费对账表 v1（AI-32 Vulkan 设备面）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26414-J1 AI-32 冻结接口逐方法消费对账（vkCreateDevice/vkGetPhysicalDeviceProperties/QueueFamily 三族）30 格全过；未收口项显性登记 R-G4-001 不静默；fake 端 10 剧本全链
### UNX-F26415 · DXGI 共享面联签消费位（AI-33 边界划账）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26415-J1 与 AI-33 联签边界表（DXGI 工厂/交换链归 AI-33、本域只引不复制）落档断言；ID 段归属核对（F25601–F26400 归 G3）零越界引用；共享 fence 基础设施接口冻结探针
### UNX-F26416 · 体验日志埋点地基（对象创建/销毁/错误全事件）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26416-J1 对象全生命周期事件（创建/销毁/失败）入统一时间轴断言；埋点异步批量零阻塞（写入 P95 <1ms）断言；不记敏感内容（着色器原文/密钥）脱敏复核
### UNX-F26417 · 异常显性化地基：隐蔽失败捕获探针（十三·补）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26417-J1 异步提交回调/队列销毁竞态/驱动返回码被忽略三处隐蔽路径逐处捕获断言（10^3 注入零静默）；早期失败（日志系统启动前）自检探针过；总日志中心可检索
### UNX-F26418 · vkd3d 基线版钉定与版本锚账（ADR-UNX-008 域内沿用）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26418-J1 vkd3d-proton 基线版 hash/日期钉定账落档；版本锚查询接口断言；升级预告位登记（波间全量重跑协议 F26800 段消费）
### UNX-F26419 · B01 段 ktest 断言面基建（域断言注册面）
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26419-J1 B01 全 20 条判据可注册为 ktest 断言（注册面 API 断言）；一条命令全量跑通（fast 档分钟级）；失败注入必红三条（判据灵敏度自检）
### UNX-F26420 · B01 收口：对象模型地基全链对账
- 域/批：G4/B01｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26420-J1 B01 段 10 剧本端到端全过（创建→查询→提交→销毁）；批内求和 6,000 与批头登记一致断言；未竟之账显性移交 B02
