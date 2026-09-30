# UNX-D1-B14 · 文件族 NativeFile* NT API 语义档（F12261–F12280 · 20 条）

> AI-16 承办｜批主题：M 型文件族——NtCreateFile 全矩阵/Read/Write/Query/SetInformationFile/DirectoryFile/Flush/取消语义、FILE_* 掩码矩阵、与 B1 VFS 联签｜域账累计：74,740 + 本批 6,000 = 80,740 / 240,000｜嫁接源：纯自研域；语义锚=MSDN Nt*File 文档与 Windows Internals ch.12（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12261–F12280 唯一；与 B1 分界——B1 管通用 VFS 本体，本域管 NT 侧语义协议（NativeFile* 面）；与 B12 分界——B12 立对象协议面，本批立文件语义主体（两处 NtCreateFile 条目按"协议档/语义主体"分层防重，判据不同）。

### UNX-F12261 · NtCreateFile 完整语义档：disposition/share/access 矩阵
- 域/批：D1/B14｜纯功能行数：380｜状态：[已深化]｜判据：UNX-F12261-J1 disposition（SUPERSEDE/CREATE/OPEN/OVERWRITE/OVERWRITEIF）×share（共享位三轴）×access（GENERIC 位映射）矩阵全格语义档齐，注入抽样 40 组与锚一致
### UNX-F12262 · NtReadFile/NtWriteFile 语义档
- 域/批：D1/B14｜纯功能行数：340｜状态：[已深化]｜判据：UNX-F12262-J1 ByteOffset 语义（NULL=当前位）与同步/异步双档判据绿，双机同序列读写结果逐字节一致
### UNX-F12263 · NtQueryInformationFile 语义档
- 域/批：D1/B14｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12263-J1 信息类首批 ≥15 类（Basic/Standard/Position/Name 类）结构布局与锚一致，双机抽样 10 类逐字段一致
### UNX-F12264 · NtSetInformationFile 语义档
- 域/批：D1/B14｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12264-J1 信息类首批 ≥10 类（Position/Rename/Disposition/EndOfFile）语义判据绿，重命名跨目录语义档齐
### UNX-F12265 · NtQueryDirectoryFile 语义档
- 域/批：D1/B14｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12265-J1 枚举协议（RestartScan/Index/ReturnSingleEntry/通配匹配）判据绿，枚举序与 Windows 同目录抽样一致
### UNX-F12266 · NtFlushBuffersFile 语义档
- 域/批：D1/B14｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12266-J1 冲刷协议判据绿，掉电模拟注入后数据完整（与 B2 journal 工程经验复用判据）
### UNX-F12267 · NtCancelIoFile/NtCancelIoFileEx 取消语义
- 域/批：D1/B14｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12267-J1 同步/异步取消双接口判据绿，取消竞态（完成与取消同时）注入 10/10 次收敛正确
### UNX-F12268 · NtDeviceIoControlFile/NtFsControlFile 预告
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12268-J1 控制码通道协议语义档齐（FSCTL/IOCTL 分派），设备语义主体归 B4/I 部分界判据绿
### UNX-F12269 · FILE_* 访问掩码与共享模式矩阵
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12269-J1 访问掩码（FILE_READ_DATA→GENERIC_READ 映射）与共享冲突矩阵全格，SHARING_VIOLATION 触发 10/10 次正确
### UNX-F12270 · IO_STATUS_BLOCK 与异步 IO 预告
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12270-J1 IOSB 结构（Status/Information 联合体布局）判据绿，异步完成消费语义与 B12 F12023 同源判据绿
### UNX-F12271 · 文件族错误矩阵：SHARING_VIOLATION 等全表
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12271-J1 族错误矩阵（≥25 码）全行齐，注入抽样 30 例返回码一致
### UNX-F12272 · 文件族与 B1 VFS 联签收口
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12272-J1 NT 协议层↔VFS 本体对接判据绿（路径翻译挂 D3 命名空间预告），全生命周期账对平
### UNX-F12273 · 文件族边界样本集
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12273-J1 边界样本（零长读写/极端偏移/长路径/特殊字符名）≥40 例入库全过不崩
### UNX-F12274 · 文件族双机对照判据
- 域/批：D1/B14｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12274-J1 同码双跑抽样 15 场景（建/读写/枚举/删）输出逐值一致（S1 母版），差异显式列账
### UNX-F12275 · 文件族对照表 30 条批入账
- 域/批：D1/B14｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12275-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测
### UNX-F12276 · 文件族性能预算（O1 对标口径）
- 域/批：D1/B14｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12276-J1 族内 P95 延迟预算在册（O1 配套账），4KB 随机读写压测口径登记
### UNX-F12277 · 文件族防幻觉出处账
- 域/批：D1/B14｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12277-J1 出处字段非空率 100%，"待基准机实测"清单在册
### UNX-F12278 · 文件族文档对齐
- 域/批：D1/B14｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12278-J1 族文档与对照表逐行一致，漂移抽查零命中
### UNX-F12279 · ktest 文件族断言集（B14 批判据聚合）
- 域/批：D1/B14｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12279-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，disposition 矩阵族与错误矩阵族独立可单跑
### UNX-F12280 · 文件族集成账
- 域/批：D1/B14｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12280-J1 批内互引零悬空；B1/B2/D3 消费方预告登记，集成账与对照表行数一致
