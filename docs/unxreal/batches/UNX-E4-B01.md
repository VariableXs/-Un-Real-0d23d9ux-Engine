#### UNX-E4-B01 · 版本查询 API 基座（F18401–F18420 · 20 条）

> AI-24 承办｜域账累计：本批 6,000 / 240,000｜嫁接源：D1 RtlGetVersion NT 层入口与 D3 CurrentVersion hive 语义为承接契约｜防重：与 D1 划界（本批管 Win32 版本查询面与谎报裁决基座，D1 管 NT 层系统调用语义）｜批注：版本查询是兼容性第一道门，全批断言围绕"查询必裁决、谎报必有据、账目必闭合"三律｜红线声明：本批无写盘条目。

| 编号 | 功能条目 | 行数 | 状态 | 判据 |
|---|---|---|---|---|
| UNX-F18401 | 版本查询面总纲（GetVersion/GetVersionEx 族冻结） | 340 | 骨架 | UNX-F18401-J1 族入口（GetVersion/GetVersionExA/GetVersionExW）冻结 v1，入口签名与 Windows 口径逐一对齐断言全过，跳段注入 10/10 拒绝 |
| UNX-F18402 | GetVersion/GetVersionExA 返回值打包语义 | 300 | 骨架 | UNX-F18402-J1 DWORD 打包（主/次版本/Build 位布局）与 Windows 实机对照 20 例零偏差，越位注入必红 |
| UNX-F18403 | GetVersionExW 与 OSVERSIONINFOEXW 结构面 | 280 | 骨架 | UNX-F18403-J1 结构尺寸/字段偏移与 winnt.h 声明一致断言全过，dwOSVersionInfoSize 校验错误路径 10/10 覆盖 |
| UNX-F18404 | dwMajorVersion/dwMinorVersion 语义字段 | 320 | 骨架 | UNX-F18404-J1 主/次版本取值经裁决矩阵（F18417 预留）输出，伪造直通注入 10/10 检出 |
| UNX-F18405 | dwBuildNumber 与 dwPlatformId 字段 | 260 | 骨架 | UNX-F18405-J1 Build 号来源登记（锚基线/档案覆写）可导出，PlatformId 恒 VER_PLATFORM_WIN32_NT 断言全过 |
| UNX-F18406 | wServicePackMajor/Minor 语义 | 300 | 骨架 | UNX-F18406-J1 服务包号与注册表 CSD 值（AI-18 接口）联动一致断言全过，hive 缺键回退路径覆盖 |
| UNX-F18407 | wProductType（工作站/域控/服务器）语义 | 340 | 骨架 | UNX-F18407-J1 三类 ProductType 判定与 bottle 档案（E1 接口）联动断言全过，越权覆写注入检出 |
| UNX-F18408 | wSuiteMask 套件位面 | 280 | 骨架 | UNX-F18408-J1 套件位全集（VER_SUITE_*) 枚举冻结 v1，位或组合与 Windows 口径对照 15 例零偏差 |
| UNX-F18409 | szCSDVersion 服务包字符串语义 | 260 | 骨架 | UNX-F18409-J1 字符串拷贝边界（128 WCHAR）越界注入 10/10 检出，空串/缺省路径覆盖 |
| UNX-F18410 | GetProductInfo 产品 SKU 语义 | 320 | 骨架 | UNX-F18410-J1 SKU 枚举（PRODUCT_*）映射表冻结 v1，未登记 SKU 回退 PRODUCT_ESSENTIAL_BUSINESS 断言全过 |
| UNX-F18411 | VerifyVersionInfo 入口注册（条件掩码预告） | 300 | 骨架 | UNX-F18411-J1 入口签名与 B06 条件掩码引擎衔接断言全过，桩期返回口径登记可导出 |
| UNX-F18412 | VerSetConditionMask 位构造语义 | 280 | 骨架 | UNX-F18412-J1 64 位条件掩码构造（双 32 位折叠）逐位断言全过，非法操作数注入检出 |
| UNX-F18413 | 版本查询线程/进程上下文隔离 | 340 | 骨架 | UNX-F18413-J1 双线程并发查询各持上下文互不污染断言全过，跨进程隔离注入 10/10 通过 |
| UNX-F18414 | 版本查询与 bottle 上下文绑定（E1 衔接） | 300 | 骨架 | UNX-F18414-J1 查询结果随 bottle 档案切换断言全过（同进程双 bottle 场景），漂移注入检出 |
| UNX-F18415 | 版本查询性能账（热路径缓存） | 260 | 骨架 | UNX-F18415-J1 缓存命中路径 P95 <1µs 断言（任务书口径），失效联动登记可导出 |
| UNX-F18416 | 版本查询防重声明（与 D1 ntdll 划界） | 320 | 骨架 | UNX-F18416-J1 20 条主题聚类对 D1 可见判据对表，嫌疑清单=空断言 |
| UNX-F18417 | 版本谎报总开关数据结构（version_report_matrix 预留） | 280 | 骨架 | UNX-F18417-J1 三元组（进程,API,维度）键结构冻结 v1 与 B02 消费签名一致 |
| UNX-F18418 | 版本面错误路径账（非法参数/空结构） | 300 | 骨架 | UNX-F18418-J1 错误路径全集（NULL/尺寸不符/未知 API）10/10 注入返回码与 Windows 一致 |
| UNX-F18419 | ktest 引导面 B01 批断言集 | 340 | 骨架 | UNX-F18419-J1 本批 19 条判据聚合入 ktest 引导面，一次命令全跑通过率 100%，失败注入必红三条 |
| UNX-F18420 | B01 批守恒闭合断言 | 280 | 骨架 | UNX-F18420-J1 本批 20 条行数逐条求和=6,000 与批头/台账三方一致，红绿齐 |
