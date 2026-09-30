# UNX-D1-B06 · UNICODE_STRING 与 RTL 字符串族（F12101–F12120 · 20 条）

> AI-16 承办｜批主题：UNICODE_STRING 三元组约定与 RTL 字符串族全语义——Length 不含 NUL、MaximumLength 含 NUL 的约定判据是海量 Win 代码兼容的暗礁（任务书专题五原文）｜域账累计：28,100 + 本批 5,460 = 33,560 / 240,000｜嫁接源：纯自研域；语义锚=MSDN UNICODE_STRING/Rtl*String 文档与 wdm.h 头文件（版本锚 ADR-UNX-008），ReactOS 对照不抄｜防重：F12101–F12120 唯一；与 B10 CRITICAL_SECTION、B12 起对象管理族无重叠——字符串族是 M 型机制批的公共底座。

### UNX-F12101 · UNICODE_STRING 结构语义实装
- 域/批：D1/B06｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12101-J1 三字段（Length/MaximumLength/Buffer）布局与 wdm.h 锚一致，sizeof/对齐断言编译期通过
### UNX-F12102 · Length/MaximumLength/NUL 约定判据
- 域/批：D1/B06｜纯功能行数：300｜状态：[已深化]｜判据：UNX-F12102-J1 "Length 不含 NUL、MaximumLength 含 NUL"注入 50 样本串（含空串/恰满/越界）逐条判定 100% 正确，双机同码零偏差
### UNX-F12103 · RtlInitUnicodeString 语义
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12103-J1 初始化后 Length=strlen*2、MaximumLength=strlen*2+2 判据绿，NULL 源串语义（Length 双零）与锚一致
### UNX-F12104 · RtlInitAnsiString/OEM_STRING 语义
- 域/批：D1/B06｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12104-J1 ANSI/OEM 双串结构初始化判据绿（字节长约定），三串类型 sizeof 断言齐
### UNX-F12105 · RtlCopyUnicodeString 语义
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12105-J1 目标容量不足截断语义（Length 截到容内）与锚一致，自拷贝（源=目标）10/10 次安全
### UNX-F12106 · RtlAppendUnicodeStringToString/ToString 语义
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12106-J1 追加越容量返回 STATUS_BUFFER_TOO_SMALL 且目标不损坏 10/10 次，追加后 Length 守恒
### UNX-F12107 · RtlUnicodeStringCopy/Cat 安全族预检语义
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12107-J1 安全族（RtlUnicodeStringCopy/CopyString/Cat）预检失败路径不触碰目标 10/10 次，与截断族语义区分判据绿
### UNX-F12108 · RtlAnsiStringToUnicodeString 转换语义
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12108-J1 AllocateDestinationConsole 双档语义判据绿，扩展字符（>0x7F）转换表与 Windows 对照 30 组零偏差
### UNX-F12109 · RtlUpcaseUnicodeString 大小写转换
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12109-J1 UPCASE 表消费判据绿，NORM_FORM 类边界（简单转换档）抽样 50 字符与 Windows 一致
### UNX-F12110 · RtlEqualUnicodeString/RtlCompareUnicodeString 比较族
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12110-J1 CaseInsensitive 参数双档判据绿，前缀/等长/不等长三类注入 30 组与 Windows 一致
### UNX-F12111 · RtlPrefixUnicodeString 前缀判定
- 域/批：D1/B06｜纯功能行数：240｜状态：[已深化]｜判据：UNX-F12111-J1 前缀判定（含 InheritCaseIgnored 档）30 组样本与 Windows 一致，空串前缀语义档齐
### UNX-F12112 · RtlIntegerToUnicodeString/整数双向转换族
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12112-J1 进制参数（10/16）双向转换 10^4 随机值往返零损，溢出路径拒止正确
### UNX-F12113 · RtlStringCbPrintfW 类安全格式化预告
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12113-J1 安全格式化预检语义档齐（容量计算/截断报告），与 C 运行时 swprintf 语义分界判据绿（CRT 归 E2 防重）
### UNX-F12114 · 缓冲所有权模型与 PagedPool 分配挂点
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12114-J1 AllocateDestination 档分配/释放配对守恒，悬垂释放注入 10/10 次被哨兵拦截
### UNX-F12115 · 越界与截断防御矩阵
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12115-J1 全族函数越界注入矩阵（每函数 ≥3 例）10/10 次拒止不崩，调试档哨兵定位到函数名
### UNX-F12116 · 多编码转换账：UTF-16/UTF-8/ANSI 面预告
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12116-J1 编码转换接口账登记（RtlUnicodeToUTF8 类），代理对往返零损抽样 30 字符判据绿
### UNX-F12117 · UPCASE 表接口与不敏感哈希挂点
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12117-J1 UPCASE 表接口冻结（B05 OBJ_CASE_INSENSITIVE 消费），哈希挂点判据绿（D3 命名哈希消费预告）
### UNX-F12118 · UNICODE_STRING 对照表制度与样本集
- 域/批：D1/B06｜纯功能行数：280｜状态：[已深化]｜判据：UNX-F12118-J1 全族函数入对照表（函数/语义/错误矩阵/判据 ID 四列），50 样本串集入册可复测
### UNX-F12119 · ktest 字符串族断言集（B06 批判据聚合）
- 域/批：D1/B06｜纯功能行数：320｜状态：[已深化]｜判据：UNX-F12119-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，约定判据族与防御矩阵族独立可单跑
### UNX-F12120 · B06 批域内集成账（含 RtlAllocateHeap 对接预告）
- 域/批：D1/B06｜纯功能行数：260｜状态：[已深化]｜判据：UNX-F12120-J1 批内互引集成账零悬空，堆挂点接口（RtlAllocateHeap 对接 D2 堆语义）预告登记联签状态
