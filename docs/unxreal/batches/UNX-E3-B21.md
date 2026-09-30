# UNX-E3-B21 · WinHTTP 基础面对账：会话-连接-请求-头（F18001–F18020 · 20 条）

> AI-23 承办（波 09 首轮立账 · 一次对话 300 项明令）｜批主题：WinHttpOpen/Connect/OpenRequest/SendRequest/ReceiveResponse/QueryHeaders/AddRequestHeaders 对账/双 API 单账/错误映射/超时对账/代理面同源/选项空间/联测（B21 WinHTTP 基础段）｜域账累计：126,000 / 240,000｜判据主轴：HTTPS 全链真站复测集（真站面随 B35+，本批为本地测试端锚）｜嫁接源：任务书六专题（HINTERNET 句柄树/代理解析/cookie 罐与缓存/Schannel 适配/异步回调与真站集）＋Wine 网络 DLL 语义参考（只跟随参考）＋MSDN 口径锚（预期值注来源）＋Windows 真机回调采样第一真值｜防重：全域 ID F17601–F18400 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、已 finalize deepen 册、总纲既有段）｜上游：AI-21（上栈环境）、AI-42（Winsock 冻结契约先行冻结+fake 对接，波 16 全量回归联调）、AI-44（TLS 通道冻结接口，B21+ 消费）、AI-18（cookie/凭据持久化载体）、AI-47（凭据管理器）｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目；真站复测集只读 GET 纪律、凭据禁明文落盘（走 AI-47）、TLS 降级选项缺省禁令——三条间接纪律全程生效

### UNX-F18001 · WinHTTP 面域段开篇：WinHttpOpen 会话对象
- 域/批：E3/B21｜纯功能行数：347｜状态：[骨架]｜判据：UNX-F18001-J1 WinHttpOpen 会话对象与 agent/access-type 全字段入账，三 access-type 分流矩阵与 WinInet 面同口径对照 20/20，会话句柄语义与根会话账同构断言
### UNX-F18002 · WinHttpConnect 连接对象与端口语义
- 域/批：E3/B21｜纯功能行数：327｜状态：[骨架]｜判据：UNX-F18002-J1 WinHttpConnect 目标/port/service 三要素与 INTERNET_DEFAULT_* 端口矩阵 12 格全过，连接对象与 B02 段同账复用断言（双 API 单账）
### UNX-F18003 · WinHttpOpenRequest 请求对象与标志位
- 域/批：E3/B21｜纯功能行数：337｜状态：[骨架]｜判据：UNX-F18003-J1 动词/目标/版本/标志四字段入账，WINHTTP_FLAG_SECURE/BYPASS_PROXY/REFRESH/NO_CACHE 四标志矩阵 16 格全过，与 WinInet 标志映射表逐项对账
### UNX-F18004 · 双 API 单账架构：WinInet/WinHTTP 共享底座
- 域/批：E3/B21｜纯功能行数：337｜状态：[骨架]｜判据：UNX-F18004-J1 同一目标（连接+请求+TLS 会话复用）经双 API 访问时底座账单实例断言（句柄树/cookie/缓存三账共享），统计可按 API 面分账查
### UNX-F18005 · WinHttpSendRequest 与可选体注入
- 域/批：E3/B21｜纯功能行数：317｜状态：[骨架]｜判据：UNX-F18005-J1 可选体（lpOptional）注入 Content-Length 自动补齐断言 20/20；同步/异步双模分流矩阵全过，异步模返回零并走回调完成
### UNX-F18006 · WinHttpReceiveResponse 与头就绪语义
- 域/批：E3/B21｜纯功能行数：327｜状态：[骨架]｜判据：UNX-F18006-J1 ReceiveResponse 完成语义=头就绪（体未读）状态机断言；头未读即关闭句柄资源回收完整（无悬空 socket）100 剧本全过
### UNX-F18007 · WinHttpQueryHeaders 与索引语义
- 域/批：E3/B21｜纯功能行数：307｜状态：[骨架]｜判据：UNX-F18007-J1 头查询索引（WINHTTP_QUERY_*/旗标组合）矩阵 30 格全过；缓冲不足回写需值长度口径与 WinInet 同构；多值头按索引遍历一致
### UNX-F18008 · WinHttpAddRequestHeaders 修饰符对账
- 域/批：E3/B21｜纯功能行数：287｜状态：[骨架]｜判据：UNX-F18008-J1 ADD/REPLACE/COALESCE 三修饰符语义与 B07 段 WinInet 面逐项对账 30/30 一致（跨 API 行为一致性宪法⑩）
### UNX-F18009 · WinHttp 状态回调全枚举面
- 域/批：E3/B21｜纯功能行数：317｜状态：[骨架]｜判据：UNX-F18009-J1 状态回调枚举（连接/发送/收头/收体/关闭/重定向/证书）七族全触发且次序可回放；dwContext 透传万次零错位
### UNX-F18010 · WinHttp 错误族映射表（单点出口）
- 域/批：E3/B21｜纯功能行数：297｜状态：[骨架]｜判据：UNX-F18010-J1 WinHTTP 错误→统一翻译器映射表 ≥35 格全过，翻译器单点出口零裸码透传 grep 留痕（宪法②双面通用）
### UNX-F18011 · WinHttp 超时族与 WinInet 四类超时对账
- 域/批：E3/B21｜纯功能行数：287｜状态：[骨架]｜判据：UNX-F18011-J1 resolve/connect/send/receive 四超时语义跨 API 对照矩阵 16 格全过，缺省值口径差异表逐项登记
### UNX-F18012 · WinHttpQueryAuthSchemes 与认证面预告
- 域/批：E3/B21｜纯功能行数：277｜状态：[骨架]｜判据：UNX-F18012-J1 认证方案枚举（Basic/Digest/Negotiate 契约位）与支持标志矩阵 12 格全过；认证续作挂 B29/B30 段预告位登记
### UNX-F18013 · WinHttp 代理面：WINHTTP_ACCESS_TYPE 三模式
- 域/批：E3/B21｜纯功能行数：287｜状态：[骨架]｜判据：UNX-F18013-J1 NO_PROXY/NamedProxy/AutoDetect 三模式分流 20 探针全过；与 B13 段三级判定链同源断言（单判定器双消费）
### UNX-F18014 · WinHttpSetOption/QueryOption 选项空间
- 域/批：E3/B21｜纯功能行数：307｜状态：[骨架]｜判据：UNX-F18014-J1 WinHTTP 选项 ID 注册表 ≥40 项建账（超时/代理/凭证/TLS 四族），不适用句柄级精确拒止 30/30
### UNX-F18015 · WinHttpCloseHandle 递归关闭对账
- 域/批：E3/B21｜纯功能行数：277｜状态：[骨架]｜判据：UNX-F18015-J1 递归关闭次序与 B05 段口径逐项一致 20/20；在途回调延迟完成协议跨 API 同构断言
### UNX-F18016 · 双 API 联测：同目标交叉访问剧本
- 域/批：E3/B21｜纯功能行数：307｜状态：[骨架]｜判据：UNX-F18016-J1 同一服务器先 WinInet 后 WinHTTP（及反向）50 剧本：连接复用/cookie 互通/缓存互通三账一致断言全过
### UNX-F18017 · WinHttp 统计账与 API 面分账
- 域/批：E3/B21｜纯功能行数：247｜状态：[骨架]｜判据：UNX-F18017-J1 请求计数/字节量/错误分布按 API 面分账可查，抽样 50 次与总账一致
### UNX-F18018 · WinHttp 体验日志埋点（入口/出口/耗时）
- 域/批：E3/B21｜纯功能行数：257｜状态：[骨架]｜判据：UNX-F18018-J1 每调用入口/出口/耗时/结果落统一日志框架，异步回调路径零阻塞断言；敏感头值零记录
### UNX-F18019 · ktest WinHttp 参数畸形注入集
- 域/批：E3/B21｜纯功能行数：287｜状态：[骨架]｜判据：UNX-F18019-J1 空指针/零长度/超长串/非法标志四族注入 10^4 次全拒精确错误码，零崩溃零静默
### UNX-F18020 · B21 收口：WinHTTP 基础面对账表
- 域/批：E3/B21｜纯功能行数：267｜状态：[骨架]｜判据：UNX-F18020-J1 WinInet↔WinHTTP 基础面 20 项对账表全绿留档；差异项逐项登记 ADR 决策记录挂点
