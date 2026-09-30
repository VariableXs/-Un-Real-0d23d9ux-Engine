# UNX-D2-B01 · DOS/NT headers 解析（F12801–F12820 · 20 条）

> AI-17 承办｜域账累计：本批 5,800 / 240,000（B01–B15 骨架累计 84,100）｜批型：F 型（地基批 · 部 D 微调 F 型扩至 10 批）｜嫁接源：纯自研（部 D 无现成上游语义底座可嫁接，判据对照 Windows 行为与真实样本库）｜防重：四范围 grep 已执行——命中 kernel/varix/src/proc/pe.rs（最小 PE 解析：DOS 头/e_lfanew/SizeOfImage 上限/节映射）与 compatstar/{peblend.rs,wow64.rs,dblrun.rs}（兼容层位数门与签名嗅探），处置按升级接管体例（样板 F0039 同款）：D2 立原生装载全链为本体，现存面为被接管基线，判据含现存面行为回归对照，非重复功能；docs/VE 与已 finalize 批次（A1 B01–B03）零命中｜红线：无引导/数据安全红线适用；样本库禁编造红线在册（每样本来源+哈希，深化期落账 AI-94）｜批注（AI-17）：任务书示例条目 F12801 名/判据/行数 400 保真归位本批锚位；上游 AI-16（D1）未收口，跨域条目依赖已在 open_risks R-D2-001 登记

### UNX-F12801 · PE 头解析与映像布局校验器
- 域/批：D2/B01｜纯功能行数：400｜状态：[已深化]｜判据：UNX-F12801-J1 100 个真实 PE 样本头解析逐字段与工具输出一致（L1 级），非 MZ/坏节表/对齐非法三类拒载逐码对齐
### UNX-F12802 · DOS header e_magic/e_lfanew 定位与 MZ 存根语义
- 域/批：D2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12802-J1 e_lfanew 定位链在 100 样本全通，e_lfanew 越界/悬空 DOS 头内两族畸形拒载且错误码与现存 proc/pe.rs PeError::BadPeOffset 回归一致
### UNX-F12803 · PE 签名与 COFF 头逐字段解析（IMAGE_FILE_HEADER）
- 域/批：D2/B01｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12803-J1 20 字段（Machine/NumberOfSections/TimeDateStamp/PointerToSymbolTable 等）解析值与 pefile 工具输出逐字段一致，TimeDateStamp 零值/伪造值有账
### UNX-F12804 · Machine 双档识别与架构分档路由（0x8664/0x014c）
- 域/批：D2/B01｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12804-J1 64/32 位样本各 ≥20 个识别分档全对，未知 Machine 码拒载落账，与 compatstar/wow64.rs gate_machine 行为回归对照一致
### UNX-F12805 · IMAGE_OPTIONAL_HEADER64 解析（Magic/AddressOfEntryPoint/ImageBase）
- 域/批：D2/B01｜纯功能行数：380｜状态：[已深化]｜判据：UNX-F12805-J1 0x20B Magic 样本 100 个全字段解析一致，入口 RVA 越出 SizeOfImage 拒载（对齐现存 PeError::EntryOutsideImage 回归）
### UNX-F12806 · IMAGE_OPTIONAL_HEADER32 双轨差异解析（PE32 vs PE32+）
- 域/批：D2/B01｜纯功能行数：360｜状态：[已深化]｜判据：UNX-F12806-J1 0x10B Magic 样本 ≥30 个字段宽度差异（BaseOfData/32 位 ImageBase/AddressOfEntryPoint）逐字段对照一致，双轨混读零误判
### UNX-F12807 · SectionAlignment/FileAlignment 合法性矩阵校验
- 域/批：D2/B01｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12807-J1 SectionAlignment<PageSize 且≠FileAlignment 拒载矩阵 16 组合全过，2^k 对齐边界样本逐组落账
### UNX-F12808 · SizeOfImage/SizeOfHeaders/SizeOfCode 一致性校验
- 域/批：D2/B01｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12808-J1 三 Size 字段交叉校验在 100 样本零误报，SizeOfImage 超上限拒载与现存 256MiB 口径（proc/pe.rs）行为一致
### UNX-F12809 · NumberOfSections 边界与节表越界防护
- 域/批：D2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12809-J1 96 节上限与节表出文件尾两族畸形样本全部拒载不崩，fuzz 截断语料 1,000 例回归零 panic
### UNX-F12810 · Characteristics 位语义账（16 位逐位）
- 域/批：D2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12810-J1 DLL/SYSTEM/DEBUG_STRIPPED/RELOCS_STRIPPED 等 16 位逐位解析落账，位掩码组合样本 50 组与工具输出一致
### UNX-F12811 · Subsystem 字段与子系统路由（GUI/CUI/NATIVE/BOOT）
- 域/批：D2/B01｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12811-J1 五类 Subsystem 样本路由判定全对，未知子系统值行为与 Windows 对照（拒绝装载/默认路径）一致并落账
### UNX-F12812 · DllCharacteristics 位语义（DYNAMICBASE/NX_COMPAT/TERMINAL_SERVER_AWARE）
- 域/批：D2/B01｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12812-J1 16 位 DllCharacteristics 逐位解析，DYNAMICBASE/NX_COMPAT 位驱动装载策略（挂 B04/B02 消费）开关样本各 20 组生效断言
### UNX-F12813 · CheckSum 字段校验算法与加载期行为对照
- 域/批：D2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12813-J1 CheckSum 算法实现与官方算法对 100 样本输出一致，驱动位（IMAGE_DLLCHARACTERISTICS）校验失败路径与 Windows 加载期行为对照落账
### UNX-F12814 · DataDirectory[16] 数组定位与目录索引语义总表
- 域/批：D2/B01｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12814-J1 16 目录索引语义总表成账，100 样本目录 RVA/Size 读取与 pefile 一致（专项越界校验归 B08 防重分层）
### UNX-F12815 · 头部区保护（SizeOfHeaders 内页只读属性）
- 域/批：D2/B01｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12815-J1 头区映射只读位生效断言，写头区访问触发保护异常样本 10 组全过
### UNX-F12816 · Rich header 只读识别与工具链指纹登记
- 域/批：D2/B01｜纯功能行数：200｜状态：[已深化]｜判据：UNX-F12816-J1 MSVC 产物 Rich header 存在性识别 ≥30 样本一致，指纹（工具链版本）登记账导出可比对，坏 DanS 签名样本降级路径有账
### UNX-F12817 · 头解析器零拷贝视图设计（文件视图到解析结构）
- 域/批：D2/B01｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12817-J1 解析全程零拷贝断言（无全文件复制），1GB 级大映像头解析内存峰值落账并低于阈值
### UNX-F12818 · 头解析分段计时埋点（解析耗时账本）
- 域/批：D2/B01｜纯功能行数：220｜状态：[已深化]｜判据：UNX-F12818-J1 DOS/COFF/OPTIONAL/节表四段计时埋点落账，100 样本 P95 耗时分布导出（对接 O1/O3 对标账）
### UNX-F12819 · 头解析错误码面（拒载码族与 Windows 对照表）
- 域/批：D2/B01｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12819-J1 拒载错误码族逐码与 Windows 行为对照表成账（返回值/错误码/副作用三栏），20 族污染样本逐码复测一致
### UNX-F12820 · ktest PE 头解析面断言集
- 域/批：D2/B01｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12820-J1 本批 19 条判据聚合入 ktest 断言面一次命令全跑，通过率 100% 才算绿（挂 O3 机器人判据库）
