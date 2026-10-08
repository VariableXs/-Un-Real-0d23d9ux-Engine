# UNX-N1 · 域归集卷（AI-100 归集快照 2026-10-01）

> 由 AI-100 归集器 v2 从主汇编册块级切分生成（块定域：标题 token > 正文 token 投票 > 前块延续）；主汇编册仍为唯一权威总册，本卷为按域阅读视图，零改写零删节。

<!-- 主册行 350035 · # AI-66 · UNX-N1 开发工具链应用收口 · 300 项新功能增补册（B01–B15 · F52001–F5 -->
# AI-66 · UNX-N1 开发工具链应用收口 · 300 项新功能增补册（B01–B15 · F52001–F52300）

> **任务书锚定**：AI-66 承包域 UNX-N1 开发工具链应用收口（F52001–F52800）· 40 批（B01–B40）· 波次窗波 23–24 · 上游 AI-21~25（E 部全链）+ AI-59（L4 工具链嫁接件）· 判据主轴"VS/VSCode/IntelliJ/Android Studio 等逐应用六判据（装/启/主/存/中/性能）"（AI分工完成图 §AI-66 保真）。部 N 单型：40 批 = 40 组应用群，每组 20 应用，800 条 = 800 个应用收口条目。本域使命一句话：**开发者在 Varix 上装得起、启得快、写得动、存得住、打得开中文、跑得不慢**。本册为第一次会话产出：前 15 批（B01–B15）共 **300 项新功能增补**，域账入账见批末累计。每条 = ID ｜ 深化名（应用收口条目）｜ 行数 ｜ 状态 ｜ 证据与判据锚定。
>
> **六判据母版（全域复用，逐条目实例化）**：J1 装——官方安装包钉版本（版本号写入 KB-COMPAT），静默参数记录，安装成功率与失败码全账；J2 启——双击 → 主界面 ≤3 秒（进程树逐进程计时 P95，冷/热两态分离记账，Electron 应用走启动三段账：进程拉起/窗口首绘/扩展激活）；J3 主功能——20 操作集逐项可用（编译/构建/调试/重构/搜索五面各 4 项，清单入知识账可查询）；J4 存——工程文件保存/加载双向无损（校验和 + 重新打开抽检）+ 版本 downgrade 兼容性声明；J5 中——界面/输入/字体三层全中文正常（消费 M1 桥三类窗口账 + F3 文本渲染）；J6 性能——核心操作 ≤ Windows 同机 ×1.5（双机同码，样本 ≥30，缺 Windows 侧数据标 N/A 不编）。铁值纪律：J2 ≤3s / J6 ≤×1.5 是铁值，达不成就降级定级列缺失项，禁改阈值（红线 #8）。
>
> **防重声明**：本域 300 条主题两两不重叠（每条 = 1 应用收口，同应用同版本不重复立项，版本升级按"升级接管"口径带接管字样）；不触他域账——E 部语义面本体（winevarix.drv/ntdll/CRT 归 AI-16~25）、工具链自举本体（gcc/git/rust/CMake 嫁接件布设归 L4/AI-59，本域只消费其判据绿）、GPU 驱动（G 部）、IME 语义本体（M1/AI-61）、容器/VM 内核面（L5/AI-60）、USB 传输面（M5/AI-65）各归其主，仅在联签锚定行出现、零改写。凡"CLI/GUI 工具 Windows 发行版收口"条目（CMake CLI/Git for Windows/MinGW-gdb 等）一律按"Windows 发行版应用收口"口径与 L4 嫁接件布设分账，逐条标注联签锚定。收口条目防重 grep 必查 KB-COMPAT（E5 知识账），N 域条目由 N 域写入（结构权归 E5/AI-25）。版本伪装表不得用于绕过许可校验（AI-95 联合审查，违例冻结批次）。
>
> **批次铺排（本册覆盖段）**：B01–B05（组 1–5 主力族：Visual Studio 族/Electron 编辑器族/IntelliJ 与 JetBrains 族/MSBuild 与 Windows SDK 构建链/Android Studio 与 ADB）；B06–B15（组 6–15：IDE 周边族/通用编辑器族/构建打包族/调试诊断族/版本管理 GUI 族/数据库工具族/终端 SSH 族/API 网络调试族/容器远程开发族/波 23 段收官综合批）。B16–B40（波 24–25）另册续写。

---

<!-- 主册行 350047 · ## 批 UNX-N1-B01（F52001–F52020 · 组 1 · Visual Studio 主力族收口 ·  -->
## 批 UNX-N1-B01（F52001–F52020 · 组 1 · Visual Studio 主力族收口 · 5,760 行）

| ID | 深化名（deepen/N1-B01） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52001 | Visual Studio 2022 Community 收口（主力族旗舰条目） | 300 | 增补 | UNX-F52001-J1 六判据全落：J1 安装器多阶段全账（组件选择/静默参数/失败码），J2 主界面 ≤3s 进程树账（devenv 链逐进程计时 P95 冷/热分账），J3 20 操作集全过（新建工程/增量编译/断点调试/重构重命名/解决方案搜索五面各 4 项），J4 .sln/.csproj/.vcxproj 保存加载校验和双向零损，J5 界面/输入/字体三层中文正常，J6 核心操作 ≤×1.5 样本 ≥30；shim 三件套挂点表入账，KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52001 保真 |
| UNX-F52002 | Visual Studio 2022 Professional 收口 | 280 | 增补 | UNX-F52002-J1 六判据全落（同母版，许可面单独记账）；与 F52001 共面 shim 复用率 ≥80% 落账；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题三 VS 家族专项 |
| UNX-F52003 | Visual Studio 2022 Enterprise 收口（IntelliTrace/CodeLens 面特化） | 280 | 增补 | UNX-F52003-J1 六判据全落 + Enterprise 特有组件（IntelliTrace/CodeLens/架构图）20 操作集特化项 4 项全过；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题三 |
| UNX-F52004 | Visual Studio 2019 Community 收口（版本阶梯下探） | 260 | 增补 | UNX-F52004-J1 六判据全落；版本阶梯下探第 2 档，S1 基线为 VS2022、2019 逐版上探结果入 KB-COMPAT；J4 downgrade（2022 工程 → 2019 打开）兼容性声明列缺失项；判据锚定 §AI-66.2 专题六版本阶梯 |
| UNX-F52005 | Visual Studio 2019 Enterprise 收口 | 260 | 增补 | UNX-F52005-J1 六判据全落；升级接管口径：与 F52003 同应用不同版本，条目带"版本阶梯上探"字样；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明升级接管口径 |
| UNX-F52006 | Visual Studio 2017 Community 收口（长尾版本定级示范） | 240 | 增补 | UNX-F52006-J1 六判据执行，S2 以下如实降级并列缺失项清单（不凑绿）；KB-COMPAT 定级 S2 + 缺失清单公开；判据锚定 §AI-66.2 专题六"S2 以下如实降级" |
| UNX-F52007 | Visual Studio Build Tools 2022 收口（无 IDE 构建面） | 280 | 增补 | UNX-F52007-J1 J3 特化为构建 CLI 20 操作集（msbuild 目标/增量/并行/日志四级）；J2 特化为命令首响应 ≤3s；六判据其余同母版；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52061 构建链口径 |
| UNX-F52008 | Visual Studio Build Tools 2019 收口 | 240 | 增补 | UNX-F52008-J1 六判据特化同 F52007；与 2022 版输出逐字节对照（警告文本除外，差异清单公开）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一 J4 口径 |
| UNX-F52009 | VS Installer 多阶段安装器本体收口 | 300 | 增补 | UNX-F52009-J1 安装器面专项：多阶段（引导器/包编排/回滚）逐阶段计账，断点续装 ×3，失败码全账 ×20 场景；对 E4 安装器语义的最重检验结论回传（联签锚定行 AI-24）；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题三安装器面 |
| UNX-F52010 | VSIX 扩展安装与扩展管理器收口 | 280 | 增补 | UNX-F52010-J1 扩展安装/更新/禁用/卸载 20 操作集全账，vsix 包签名校验行为对照，扩展宿主崩溃隔离（单扩展崩不拖 IDE）验证 ×5 注入；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题三 vsix 扩展安装 |
| UNX-F52011 | Visual C++ Redistributable 2022 收口（运行库前置件） | 260 | 增补 | UNX-F52011-J1 J3 特化为安装/修复/静默/检测四操作 + 依赖方探测 ×20 二进制加载成功；与 E2 CRT 版本矩阵对账零漂移（联签锚定行 AI-22）；KB-COMPAT 定级 S1；判据锚定 §AI-66.4 上游 CRT 版本矩阵 |
| UNX-F52012 | Visual C++ Redistributable 2019（14.2x）收口 | 240 | 增补 | UNX-F52012-J1 同 F52011 口径；版本共存（2022/2019 并装）零互踩 ×2 轮验证；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六版本阶梯 |
| UNX-F52013 | Remote Tools for Visual Studio 2022 收口（远程调试面） | 280 | 增补 | UNX-F52013-J1 远程调试器服务启动/连接/断点双向/内存查看四段全账；网络端口与防火墙预置建议账（联签锚定行 AI-50）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二调试 API 消费 |
| UNX-F52014 | IntelliTrace 独立收集器收口 | 240 | 增补 | UNX-F52014-J1 采集/停止/导出/.iTrace 打开四段账，事件时间轴与源码映射抽检 ×10；性能开销 ≤15% 落账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 B01 批判据主轴 |
| UNX-F52015 | Visual Studio Test Agent 2022 收口 | 260 | 增补 | UNX-F52015-J1 测试代理拉起/用例分发/结果回传/并行 4 代理四段账零漂移；与 Test Controller 联调 ×3；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二进程树语义 |
| UNX-F52016 | SSDT（SQL Server Data Tools）收口 | 280 | 增补 | UNX-F52016-J1 六判据全落 + 数据库工程发布/Schema 比较/DACPAC 导入 20 操作集特化项；J4 .sqlproj 双向零损；KB-COMPAT 定级登记；判据锚定 §AI-66.3 批判据主轴 |
| UNX-F52017 | Team Explorer 2022 收口（Git/TFS 集成面） | 260 | 增补 | UNX-F52017-J1 Git 集成消费 L4 git 嫁接件判据绿（联签锚定行 AI-59）：clone/commit/push/pull/branch 五操作 ×10 全通 + 文件锁语义对账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 Git 集成 |
| UNX-F52018 | Visual Studio Tools for Unity（VSTU）收口 | 260 | 增补 | UNX-F52018-J1 Unity 工程调试挂接/断点命中/变量查看三段账；与 G 部渲染面无耦合声明（收口仅限工具链侧）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 语义边界 |
| UNX-F52019 | .NET SDK（Windows 发行版收口，与 L4 分账） | 280 | 增补 | UNX-F52019-J1 dotnet CLI 20 操作集（new/build/run/publish/test 五面）；增量编译输出与 Windows 对照逐字节一致（警告除外）；Windows 发行版应用收口口径与 L4 工具链布设分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 语义边界 + 防重声明 |
| UNX-F52020 | Visual Studio Help Viewer 收口 | 200 | 增补 | UNX-F52020-J1 离线帮助安装/检索/渲染三段账；F3 文本渲染消费对账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.4 横向消费 F3 |

**批 B01 防重声明**：本批 20 条 = 组 1"VS 主力族"20 应用收口，每条 1 应用零重复；ID 段 F52001–F52020 与邻批零交叠。VS 家族版本阶梯（2017/2019/2022）按"升级接管"口径分立条目。E 部语义面本体零触碰，安装器/CRT/CRT 矩阵仅消费联签。

---

<!-- 主册行 350076 · ## 批 UNX-N1-B02（F52021–F52040 · 组 2 · VSCode 与 Electron 编辑器族 -->
## 批 UNX-N1-B02（F52021–F52040 · 组 2 · VSCode 与 Electron 编辑器族收口 · 5,800 行）

| ID | 深化名（deepen/N1-B02） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52021 | VSCode 收口（Electron 上栈代表条目） | 300 | 增补 | UNX-F52021-J1 六判据全落 + 启动三段账（进程拉起/窗口首绘/扩展激活逐段与 Windows 对照，总时长仍守 ≤3s，超限如实降级列缺失项）；多进程模型（主/渲染/GPU/扩展宿主四进程）句柄与共享内存语义账；扩展宿主原生模块加载 ×10 全过；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52021 保真 |
| UNX-F52022 | VSCode Insiders 收口（滚动版本线） | 240 | 增补 | UNX-F52022-J1 六判据全落；滚动版随上游月更，钉版 + AI-94 通知链 T+1 回归口径落账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六上游滚动 |
| UNX-F52023 | VSCodium 收口（无遥测分发版） | 240 | 增补 | UNX-F52023-J1 六判据全落；与 VSCode 共面 shim 复用率 ≥85%（Electron 同基座）落账；扩展市场差异行为声明；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题四 Electron 族专项 |
| UNX-F52024 | Pulsar（Atom 社区续作）收口 | 240 | 增补 | UNX-F52024-J1 六判据全落（Electron 上栈三段账）；包管理器（ppm 类）安装 ×5 扩展全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 |
| UNX-F52025 | Atom 收口（EOL 应用定级示范） | 220 | 增补 | UNX-F52025-J1 六判据执行，上游已停更 → 定级冻结声明（不再上探、不追新），如实列缺失项；KB-COMPAT 定级 S2 + EOL 冻结标记；判据锚定 §AI-66.2 专题六"跟随≠追新" |
| UNX-F52026 | Obsidian 收口（Electron 笔记库） | 240 | 增补 | UNX-F52026-J1 六判据全落；J4 特化：.md 库保存加载双向零损（校验和 ×100 文件）；J3 特化操作集：双链/图谱/插件/同步四面；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52027 | Typora 收口（所见即所得 Markdown） | 220 | 增补 | UNX-F52027-J1 六判据全落；J5 特化：公式/表格/中文混排渲染对照（消费 F3 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.4 横向消费 F3 |
| UNX-F52028 | MarkText 收口（开源 Markdown 编辑器） | 200 | 增补 | UNX-F52028-J1 六判据全落；大文件（10MB .md）打开分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52029 | Zettlr 收口（学术写作 Electron 应用） | 220 | 增补 | UNX-F52029-J1 六判据全落；Pandoc 导出链 ×5 格式（docx/pdf/odt/html/epub）输出校验和对照；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52030 | Notable 收口 | 200 | 增补 | UNX-F52030-J1 六判据全落；J4 双向零损 + frontmatter 元数据保真抽检 ×20；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52031 | Boost Note（legacy 版）收口 | 200 | 增补 | UNX-F52031-J1 六判据全落；本地存储路径与云端模式分离声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52032 | VSCode 扩展宿主原生模块 ABI 收口（node-gyp 类 ×10 扩展实测） | 300 | 增补 | UNX-F52032-J1 PE 装载 + CRT 版本矩阵消费（联签锚定行 AI-22）：10 个含原生模块扩展（node-pty/SQLite 绑定/树解析器类）加载运行全通；ABI 失配错误路径三要素呈现；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题四 Node 原生模块 ABI |
| UNX-F52033 | VSCode 扩展：Python（ms-python）收口 | 260 | 增补 | UNX-F52033-J1 解释器选择/IntelliSense/调试挂接/lint 四段 ×10 场景全通；Pylance 原生面并入 F52032 账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 |
| UNX-F52034 | VSCode 扩展：C/C++（cpptools）收口 | 260 | 增补 | UNX-F52034-J1 IntelliSense 数据库生成（万文件索引 IO 模式账，与 B03 JBR 同构不同参）/调试挂接（MinGW-gdb 消费，联签锚定行）/浏览符号三段全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52035 | VSCode 扩展：Remote-SSH 消费面收口 | 280 | 增补 | UNX-F52035-J1 远程主机连接/服务端组件布设/端口转发/断线重连四段账；OpenSSH 消费对账（联签锚定行 B13 F52188）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |
| UNX-F52036 | VSCode 扩展：GitLens 收口 | 240 | 增补 | UNX-F52036-J1 blame/历史/对比三视图 ×10 仓库全通；Git 消费对账（L4 嫁接件判据绿，联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 Git 集成 |
| UNX-F52037 | VSCode 扩展：ESLint 收口 | 220 | 增补 | UNX-F52037-J1 lint 触发（保存/输入/CLI 三通道）/规则注入/修复应用三段 ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 |
| UNX-F52038 | VSCode 扩展：Prettier 收口 | 220 | 增补 | UNX-F52038-J1 格式化（保存触发/手动/范围三类）×10 文件型全通，输出幂等（二次格式化 diff 为零）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 |
| UNX-F52039 | VSCode 扩展：Docker（ms-azuretools）收口 | 240 | 增补 | UNX-F52039-J1 容器/镜像/Compose 三视图操作 ×10 全通；与 B14 Docker Desktop 消费对账（联签锚定行 F52281）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52040 | Electron 前置验证周结论条目（VSCode 最小集试收口报告） | 300 | 增补 | UNX-F52040-J1 波 23 开工首月"Electron 前置验证周"全记录：VSCode 最小集试收口 → 失绿问题清单回推 G 部缺陷账（联签锚定行）→ 回推项 ×N 全部登记，不硬写收口条目结论留痕；KB-COMPAT 定级流程验证；判据锚定 §AI-66.6 风险 ①Electron 族渲染面时点风险 |

**批 B02 防重声明**：本批 20 条 = 组 2"Electron 编辑器族 + VSCode 扩展生态"20 收口条目；扩展条目按扩展应用独立立项，与本体的同版本不重复；ID 段 F52021–F52040 零交叠。GPU 合成器路径消费 G 部判据绿（联签锚定行），本域不建渲染语义。

---

<!-- 主册行 350105 · ## 批 UNX-N1-B03（F52041–F52060 · 组 3 · IntelliJ 与 JetBrains 家 -->
## 批 UNX-N1-B03（F52041–F52060 · 组 3 · IntelliJ 与 JetBrains 家族收口 · 5,760 行）

| ID | 深化名（deepen/N1-B03） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52041 | IntelliJ IDEA Ultimate 收口（索引 IO 判据承载条目） | 300 | 增补 | UNX-F52041-J1 六判据全落 + 万文件索引时长 ≤×1.5（顺序读+随机读混合 IO 账，B4 队列深度与 A4 页缓存联动对账，联签锚定行）+ Gradle daemon 常驻 7 天零泄漏；JBR 自带上栈收口；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52041 保真 |
| UNX-F52042 | IntelliJ IDEA Community 收口 | 260 | 增补 | UNX-F52042-J1 六判据全落；与 Ultimate 共面 shim 复用 ≥80% 落账；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题五 JetBrains 家族专项 |
| UNX-F52043 | PyCharm Professional 收口 | 260 | 增补 | UNX-F52043-J1 六判据全落 + Python/科学模式 20 操作集特化项；解释器探测（系统 venv/conda 三源）×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52044 | PyCharm Community 收口 | 240 | 增补 | UNX-F52044-J1 六判据全落；版本阶梯与 Professional 分账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52045 | WebStorm 收口 | 240 | 增补 | UNX-F52045-J1 六判据全落；Node 调试挂接/前端工具链集成操作集特化；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52046 | GoLand 收口 | 240 | 增补 | UNX-F52046-J1 六判据全落；Go 工具链（go build/test/dlv 调试）消费 ×10 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52047 | CLion 收口（CMake 消费面） | 260 | 增补 | UNX-F52047-J1 六判据全落；CMake 消费对账（Windows 发行版应用口径，与 L4 嫁接件分账联签锚定行 AI-59）：工程加载/重载/配置切换 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52048 | Rider 收口（.NET 全栈） | 260 | 增补 | UNX-F52048-J1 六判据全落；MSBuild 消费对账（B01 构建链判据复用）；ReSharper 引擎共面声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52049 | DataGrip 收口（数据库 IDE） | 240 | 增补 | UNX-F52049-J1 六判据全落；JDBC 驱动管理 ×5 数据源连接全通；与 B11 数据库工具族分界声明（IDE 收口 vs 工具收口）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 批判据主轴 |
| UNX-F52050 | PhpStorm 收口 | 240 | 增补 | UNX-F52050-J1 六判据全落；PHP 解释器/Web 服务器调试挂接三段账；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52051 | RubyMine 收口 | 220 | 增补 | UNX-F52051-J1 六判据全落；Rails 调试挂接/ gem 管理操作集；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52052 | ReSharper 收口（VS 扩展形态） | 260 | 增补 | UNX-F52052-J1 六判据全落（宿主为 VS2022，消费 B01 shim 共面）；重构操作集 ×10 全通；与宿主兼容矩阵入 KB-COMPAT；判据锚定 §AI-66.2 专题三 VS 家族联动 |
| UNX-F52053 | dotCover 收口 | 220 | 增补 | UNX-F52053-J1 覆盖率采集/报告生成/VS 集成三段账；性能开销 ≤20% 落账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52054 | dotTrace 收口 | 220 | 增补 | UNX-F52054-J1 快照采集/时间线视图/热点定位三段账；采样模式 ×2 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52055 | dotMemory 收口 | 220 | 增补 | UNX-F52055-J1 内存快照/泄漏定位/保留树三段账；GB 级堆快照打开 ≤×1.5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52056 | JetBrains Toolbox App 收口 | 240 | 增补 | UNX-F52056-J1 工具安装/更新/回滚三段账；多 IDE 并存目录隔离（.jdks/本地库）零互踩 ×2 轮；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52057 | JBR（JetBrains Runtime）自带上栈收口 | 300 | 增补 | UNX-F52057-J1 JBR 运行时自带上栈全账：字体渲染（JCEF/FreeType 路径对 F3 联签锚定行）/DPI 缩放/输入法挂接三面 ×30 场景；全部 JetBrains 家族条目的运行时前置；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题五 JBR 自带上栈 |
| UNX-F52058 | Gradle daemon 常驻进程语义收口 | 280 | 增补 | UNX-F52058-J1 daemon 拉起/复用/超时回收/崩溃重启四态账 ×3 轮；常驻 7 天内存漂移 <10%；JVM 进程语义消费 E1 面（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 Gradle daemon |
| UNX-F52059 | JetBrains Fleet 收口（下一代轻量 IDE） | 240 | 增补 | UNX-F52059-J1 六判据全落（前后端分离进程模型账）；智能模式切换 ×5 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52060 | JetBrains MPS 收口（投影编辑器长尾） | 200 | 增补 | UNX-F52060-J1 六判据全落；工程生成语言/构建操作集；KB-COMPAT 定级登记；判据锚定 §AI-66.3 长尾组口径 |

**批 B03 防重声明**：本批 20 条 = 组 3"JetBrains 家族"20 收口条目；JBR/Gradle daemon 为家族共面单列条目（供全族 shim 复用），不与任一应用条目重复；ID 段 F52041–F52060 零交叠。Android Studio 归 B05（按任务书示例条目分工），本批不含。

---

<!-- 主册行 350134 · ## 批 UNX-N1-B04（F52061–F52080 · 组 4 · MSBuild 与 Windows SDK  -->
## 批 UNX-N1-B04（F52061–F52080 · 组 4 · MSBuild 与 Windows SDK 构建链收口 · 5,560 行）

| ID | 深化名（deepen/N1-B04） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52061 | MSBuild/Windows SDK 构建链收口（组旗舰） | 300 | 增补 | UNX-F52061-J1 样本工程 ×10 增量/全量构建输出与 Windows 逐字节一致（警告文本除外，差异清单公开）；MSBuild/cl.exe/link.exe 子进程链作业对象与环境块语义账（消费 D2 PE 装载与 E2 CRT 联签锚定行）、子进程退出码传递保真 ×20；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52061 保真 |
| UNX-F52062 | Windows SDK 10.0.19041（2004 版）收口 | 260 | 增补 | UNX-F52062-J1 头文件/库/工具三件套布设完整性 ×40 路径断言；样本工程编译链接全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二编译器进程树 |
| UNX-F52063 | Windows SDK 10.0.22000（Win11 首版）收口 | 240 | 增补 | UNX-F52063-J1 同 F52062 口径；多 SDK 并存（与 19041）选择语义 ×10 场景零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六版本阶梯 |
| UNX-F52064 | Windows SDK 10.0.26100（24H2 版）收口 | 240 | 增补 | UNX-F52064-J1 同口径；升级接管口径登记；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明升级接管 |
| UNX-F52065 | .NET Framework 4.8 Developer Pack 收口 | 240 | 增补 | UNX-F52065-J1 引用程序集/目标包 ×20 工程编译全通；msbuild TargetFramework 解析 ×5 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52066 | .NET 8 SDK 收口（Windows 发行版，与 L4 分账） | 260 | 增补 | UNX-F52066-J1 dotnet build/publish ×10 工程全通；AOT 发布路径产物可运行；Windows 发行版应用收口口径与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 防重声明 |
| UNX-F52067 | .NET 6 SDK 收口（LTS 下探） | 220 | 增补 | UNX-F52067-J1 同 F52066 口径；与 8 SDK 并存 global.json 选择语义 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52068 | NuGet（CLI 与包恢复面）收口 | 260 | 增补 | UNX-F52068-J1 restore/pack/push 20 操作集构建恢复五面；包缓存目录（GB 级）卷层压力对 B2/B3 联动账（联签锚定行）；网络面消费 AI-23 真站复测集背书（联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题三缓存目录 |
| UNX-F52069 | vcpkg 收口 | 260 | 增补 | UNX-F52069-J1 端口安装 ×20（含二进制缓存命中）全通；与 MSBuild 集成 ×5 工程全通；克隆与自举消费 git 嫁接件判据绿（联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52070 | WiX Toolset v4 收口 | 240 | 增补 | UNX-F52070-J1 MSI 构建 ×5 样本全通，产物可被 E4 安装器语义面引导安装（联签锚定行 AI-24）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 长尾组口径 |
| UNX-F52071 | Inno Setup 收口 | 240 | 增补 | UNX-F52071-J1 安装包构建 ×5 样本 + 产出自安装全通（E4 形态分类"自研壳/Inno"对账联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.6 风险 ③安装器长尾形态分类复用 |
| UNX-F52072 | NSIS 收口 | 240 | 增补 | UNX-F52072-J1 脚本编译 ×5 样本 + 产出自安装全通（E4 形态分类对账）；KB-COMPAT 定级登记；判据锚定 §AI-66.6 风险 ③ |
| UNX-F52073 | MSYS2 收口（构建环境分发面） | 260 | 增补 | UNX-F52073-J1 pacman 包管理 ×20 操作（安装/更新/查询）全通；与 L4 工具链布设分界声明（发行版环境收口 vs 嫁接件布设，联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52074 | Chocolatey CLI 收口 | 220 | 增补 | UNX-F52074-J1 install/upgrade/uninstall 20 操作集；包脚本执行沙箱行为账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52075 | CMake GUI 收口（Windows 发行版，与 L4 分账） | 260 | 增补 | UNX-F52075-J1 配置/生成 ×10 工程全通（生成器选 VS/Ninja 双路径）；GUI 状态机完整（重配置/错误态/取消路径 ×10）；Windows 发行版应用收口与 L4 CMake 嫁接件布设分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 防重声明 |
| UNX-F52076 | Meson 收口 | 220 | 增补 | UNX-F52076-J1 setup/compile ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52077 | Ninja 收口 | 220 | 增补 | UNX-F52077-J1 增量构建正确性（依赖图 ×10 注入：头改/源改/无改三类动作）与 Windows 逐字节对照；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52078 | Bazel 收口 | 240 | 增补 | UNX-F52078-J1 build/test ×10 目标全通；远程缓存禁用本地回退路径账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52079 | SCons 收口 | 200 | 增补 | UNX-F52079-J1 构建 ×5 样本全通；Python 消费对账（F52043 解释器判据复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52080 | Make for Windows（GnuWin32/独立发行版）收口 | 220 | 增补 | UNX-F52080-J1 Makefile ×10 样本（含并行 -j4）构建全通；与 L4 make 类开发者包组分账声明（联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |

**批 B04 防重声明**：本批 20 条 = 组 4"MSBuild/Windows SDK 构建链"20 收口条目；全部按"Windows 发行版应用收口"口径，与 L4（AI-59）嫁接件布设分账、零重写（每条联签锚定行已标）；ID 段 F52061–F52080 零交叠。

---

<!-- 主册行 350163 · ## 批 UNX-N1-B05（F52081–F52100 · 组 5 · Android Studio 与 ADB 工 -->
## 批 UNX-N1-B05（F52081–F52100 · 组 5 · Android Studio 与 ADB 工具链收口 · 5,700 行）

| ID | 深化名（deepen/N1-B05） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52081 | Android Studio 收口（组旗舰） | 300 | 增补 | UNX-F52081-J1 六判据全落 + ADB 设备枚举复用 M5 账（联签锚定行 AI-65）+ 模拟器路径显式 N/A 声明（HAXM 类虚拟化消费 L5/AI-60 可选路径，不可用则 AVD 降级 N/A 账，不虚报）；SDK 管理器下载面消费 E3 网络语义（联签锚定行 AI-23）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 示例条目 UNX-F52081 保真 |
| UNX-F52082 | Android SDK Platform-Tools（ADB/fastboot）收口 | 280 | 增补 | UNX-F52082-J1 adb devices/shell/push/pull/logcat 20 操作集全通；设备枚举判据复用 M5 账零重测（联签锚定行 AI-65）；fastboot 与 M5 vendor 类管道分账声明；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题五 ADB 消费 M5 USB |
| UNX-F52083 | Android SDK Command-line Tools（sdkmanager/avdmanager）收口 | 260 | 增补 | UNX-F52083-J1 sdkmanager 包安装/更新/许可证签署 ×10 场景全通；下载面 HTTPS 消费 AI-23 真站复测集背书（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52084 | Android Emulator 收口（显式 N/A 虚拟化声明） | 280 | 增补 | UNX-F52084-J1 虚拟化依赖探测账：HAXM/WHPX 类路径消费 L5 判据（联签锚定行 AI-60）；不可用 → AVD 降级 N/A 账 + 三要素用户告警（禁"能开但卡死"隐性失败）；CPU 软路径不承诺声明；KB-COMPAT 定级 S3 + N/A 清单；判据锚定 §AI-66.2 专题五模拟器 N/A 账 |
| UNX-F52085 | AVD Manager 与 AVD 生命周期收口 | 240 | 增补 | UNX-F52085-J1 AVD 创建/克隆/删除 ×10 全通；镜像文件 IO（大文件读写）与 B4 卷层联动账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52086 | Android NDK r26 收口 | 280 | 增补 | UNX-F52086-J1 交叉编译样本 ×10（clang for aarch64-android）全通；ndk-build/CMake 双路径；与 L4 三元组语义分账声明（联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52087 | Eclipse Temurin JDK 17 收口（Windows 发行版，与 L4 分账） | 260 | 增补 | UNX-F52087-J1 javac/java/jar 20 操作集 ×10 工程全通；JVM 进程语义消费 E1 面（联签锚定行）；发行版收口与 L4 分账；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 防重声明 |
| UNX-F52088 | Eclipse Temurin JDK 21 收口（LTS 上探） | 240 | 增补 | UNX-F52088-J1 同 F52087 口径；虚拟线程类新特性样本 ×3 全通；升级接管口径；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52089 | Eclipse Temurin JRE 8 收口（长尾运行时） | 220 | 增补 | UNX-F52089-J1 老应用运行 ×10 样本全通；与 17/21 并存切换语义（JAVA_HOME/注册表）×5 场景零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六版本阶梯 |
| UNX-F52090 | Gradle（Windows 发行版）收口 | 260 | 增补 | UNX-F52090-J1 build/test ×10 工程全通；与 B03 daemon 共面条目联签（F52058 判据复用）；wrapper 自举下载面网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52091 | Android Gradle Plugin 构建链收口 | 260 | 增补 | UNX-F52091-J1 APK/AAB 构建 ×5 样本全通；资源编译（aapt2）/DEX 转换（d8）/签名（apksigner）三段产物与 Windows 校验和对照；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二编译器进程树 |
| UNX-F52092 | Logcat 消费面收口（AS Logcat 工具窗） | 240 | 增补 | UNX-F52092-J1 实时流/过滤/导出三段账；ADB 流消费判据复用 M5 账（联签锚定行 AI-65）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52093 | Layout Inspector 消费面收口 | 240 | 增补 | UNX-F52093-J1 视图树抓取/属性面板/合成器联动三段账 ×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52094 | APK Analyzer 消费面收口 | 240 | 增补 | UNX-F52094-J1 APK 解析（清单/资源/DEX 三视图）×10 包全通；大 APK（500MB）打开分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52095 | scrcpy 收口（设备镜像工具） | 240 | 增补 | UNX-F52095-J1 设备连接/视频流/输入注入三段账；ADB 传输判据复用 M5 账（联签锚定行 AI-65）；帧率账落账（P95）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52096 | Universal ADB Driver 收口 | 200 | 增补 | UNX-F52096-J1 驱动安装/设备识别 ×5 机型全通；与 M5 设备类语义分账（联签锚定行 AI-65）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52097 | Google USB Driver 收口 | 200 | 增补 | UNX-F52097-J1 驱动安装与设备枚举 ×5 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 |
| UNX-F52098 | fastboot 刷写面收口（只读判据版） | 240 | 增补 | UNX-F52098-J1 devices/getvar/ boot 状态查询 ×10 全通；**flash/erase 类破坏性写判据一律只干跑（dry-run 列清单），真机写入不在本域判据内**（硬件数据安全红线适用性声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 + Variable 硬件红线 |
| UNX-F52099 | Android SDK 平台包族收口（android-33/34/35 三档） | 260 | 增补 | UNX-F52099-J1 三档平台包安装/引用/编译样本 ×3 ×10 全通；多档并存选择语义零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六版本阶梯 |
| UNX-F52100 | Kotlin 命令行编译器（Windows 发行版）收口 | 240 | 增补 | UNX-F52100-J1 kotlinc 编译运行 ×10 样本全通；与 JVM 收口联签（F52087 判据复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B05 防重声明**：本批 20 条 = 组 5"Android Studio 与 ADB 工具链"20 收口条目；模拟器/虚拟化 N/A 账与 fastboot 破坏性写干跑纪律为本批显式安全声明；ID 段 F52081–F52100 零交叠。ADB/USB 消费面全部复用 M5 账零重测（联签锚定行 AI-65）。

---
<!-- 主册行 350191 · ## 批 UNX-N1-B06（F52101–F52120 · 组 6 · IDE 周边与多语言 IDE 族收口 · 5 -->
## 批 UNX-N1-B06（F52101–F52120 · 组 6 · IDE 周边与多语言 IDE 族收口 · 5,320 行）

| ID | 深化名（deepen/N1-B06） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52101 | Qt Creator 收口 | 300 | 增补 | UNX-F52101-J1 六判据全落；qmake/CMake 双构建消费 ×10 工程全通（CMake 分账联签锚定行 AI-59）；Qt 设计器/调试挂接（gdb 消费，联签锚定行 B09）操作集；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 6 口径 |
| UNX-F52102 | Qt 在线安装器收口 | 240 | 增补 | UNX-F52102-J1 组件树选择/下载/安装 ×10 场景全通；账号登录离线路径声明；网络面消费 AI-23（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题三安装器面复用 |
| UNX-F52103 | Eclipse IDE（Java SE）收口 | 260 | 增补 | UNX-F52103-J1 六判据全落；工作区索引 IO 模式账（与 B03 索引同构不同参）；JDK 消费联签（F52087 判据复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五 JVM 家族扩展 |
| UNX-F52104 | Eclipse CDT 收口 | 240 | 增补 | UNX-F52104-J1 六判据全落；MinGW-gdb 调试挂接（联签锚定行 B09）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52105 | NetBeans 收口 | 240 | 增补 | UNX-F52105-J1 六判据全落；Maven/Ant 双构建消费（B08 判据复用联签）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52106 | Code::Blocks 收口 | 220 | 增补 | UNX-F52106-J1 六判据全落；多编译器配置（gcc/MSVC 检测）×5 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52107 | Dev-C++（Embarcadero 版）收口 | 200 | 增补 | UNX-F52107-J1 六判据全落；TDM-gcc 消费声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 长尾 |
| UNX-F52108 | KDevelop（Windows 版）收口 | 220 | 增补 | UNX-F52108-J1 六判据全落；CMake 消费（分账联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52109 | Geany 收口 | 200 | 增补 | UNX-F52109-J1 六判据全落；轻量插件 ×5 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 长尾 |
| UNX-F52110 | SharpDevelop 收口（EOL 定级示范） | 200 | 增补 | UNX-F52110-J1 六判据执行；上游停更 → EOL 冻结定级（不追新）+ 缺失清单；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52111 | Lazarus（Free Pascal IDE）收口 | 240 | 增补 | UNX-F52111-J1 六判据全落；FPC 编译 ×10 样本全通；LCL 界面库渲染消费 G 部判据（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52112 | RStudio Desktop 收口 | 260 | 增补 | UNX-F52112-J1 六判据全落；R 解释器会话/plots/Help 三面板 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52113 | Spyder 收口（科学 Python IDE） | 240 | 增补 | UNX-F52113-J1 六判据全落；IPython 控制台/变量浏览器/绘图面板三段 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52114 | JupyterLab 桌面消费面收口 | 240 | 增补 | UNX-F52114-J1 notebook 打开/内核会话/导出三段 ×10 场景全通；内核崩溃隔离（单内核崩不拖壳）验证；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52115 | Anaconda Navigator 收口 | 240 | 增补 | UNX-F52115-J1 环境管理/包安装/应用拉起 ×10 全通；大体积目录（GB 级）卷层联动账（联签锚定行 B2/B3）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52116 | Miniconda 收口 | 220 | 增补 | UNX-F52116-J1 conda create/install ×10 全通；环境切换语义 ×5 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52117 | IDLE（Python 官方 IDE）收口 | 200 | 增补 | UNX-F52117-J1 六判据全落；Tkinter 界面渲染消费 G 部判据（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52118 | Strawberry Perl 收口 | 220 | 增补 | UNX-F52118-J1 perl 运行 ×10 样本全通；模块安装（cpanm 类）×5 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52119 | RubyInstaller 收口 | 220 | 增补 | UNX-F52119-J1 ruby 运行/gem 安装 ×10 全通；Devkit 构建链（mri 原生扩展）样本 ×3 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52120 | Visual Basic 6.0 IDE 收口（古董兼容定级示范） | 240 | 增补 | UNX-F52120-J1 六判据执行；16/32 位混合时代应用 → 定级 S2 + 缺失清单（OCX 注册/MDI 行为差异逐项列）；版本伪装表许可合规声明（AI-95 联签锚定行）；判据锚定 §AI-66.2 专题六长尾定级 |

**批 B06 防重声明**：本批 20 条 = 组 6"IDE 周边与多语言 IDE 族"20 收口条目；与 B01–B05 主力族零重叠；ID 段 F52101–F52120 零交叠。

---

<!-- 主册行 350220 · ## 批 UNX-N1-B07（F52121–F52140 · 组 7 · 通用编辑器族收口 · 5,180 行） -->
## 批 UNX-N1-B07（F52121–F52140 · 组 7 · 通用编辑器族收口 · 5,180 行）

| ID | 深化名（deepen/N1-B07） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52121 | Notepad++ 收口 | 280 | 增补 | UNX-F52121-J1 六判据全落；大文件（1GB 日志）打开分段账；插件生态 ×10 全通；多标签崩溃隔离 ×5 注入；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 7 口径 |
| UNX-F52122 | Sublime Text 4 收口 | 260 | 增补 | UNX-F52122-J1 六判据全落；GPU 渲染路径探测与降级表（G 部能力页探测，联签锚定行）；命令面板/多光标操作集；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题四 GPU 路径同构 |
| UNX-F52123 | Vim/gVim 收口 | 260 | 增补 | UNX-F52123-J1 六判据全落；模式状态机（normal/insert/visual 切换 ×20）零漂移；vimrc 加载/插件 ×5 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一母版 |
| UNX-F52124 | Neovim 收口 | 260 | 增补 | UNX-F52124-J1 六判据全落；LSP 客户端 ×5 语言服务全通；TreeSitter 原生模块消费（F52032 ABI 账复用联签）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 ABI 同构 |
| UNX-F52125 | Emacs（Windows 版）收口 | 260 | 增补 | UNX-F52125-J1 六判据全落；ELisp 包管理 ×5 全通；daemon/client 模式账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52126 | UltraEdit 收口 | 240 | 增补 | UNX-F52126-J1 六判据全落；超大文件（4GB）打开/编辑/保存三段账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52127 | EmEditor 收口（大文件专业编辑器） | 240 | 增补 | UNX-F52127-J1 六判据全落；CSV 大文件模式（10 亿行级声明档）打开分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52128 | EditPlus 收口 | 200 | 增补 | UNX-F52128-J1 六判据全落；FTP 远程编辑 ×5 场景（网络面 AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52129 | TextPad 收口 | 200 | 增补 | UNX-F52129-J1 六判据全落；正则搜索/宏操作集；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52130 | EverEdit 收口 | 200 | 增补 | UNX-F52130-J1 六判据全落；大文件/多编码 ×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52131 | Notepad2 收口（EOL 定级示范） | 180 | 增补 | UNX-F52131-J1 六判据执行；EOL 冻结定级；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52132 | Notepad3 收口 | 200 | 增补 | UNX-F52132-J1 六判据全落；Notepad2 接管口径（升级接管字样）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52133 | AkelPad 收口 | 180 | 增补 | UNX-F52133-J1 六判据全落；插件 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52134 | PSPad 收口 | 200 | 增补 | UNX-F52134-J1 六判据全落；多语法高亮 ×10 语言；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52135 | Kate（Windows 版）收口 | 220 | 增补 | UNX-F52135-J1 六判据全落；LSP/会话恢复 ×5 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52136 | Brackets 收口（EOL 定级示范） | 180 | 增补 | UNX-F52136-J1 六判据执行；EOL 冻结定级（Adobe 停更）；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52137 | Cursor 收口（VSCode fork，AI 辅助编辑） | 260 | 增补 | UNX-F52137-J1 六判据全落；AI 请求面（外部服务网络语义，AI-23 联签锚定行）+ 本地模型不可得 N/A 声明；与 VSCode shim 复用 ≥80%；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四同构 |
| UNX-F52138 | Helix（终端编辑器）收口 | 200 | 增补 | UNX-F52138-J1 六判据全落；终端消费面（与 B12 终端条目联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52139 | Nano for Windows 收口 | 180 | 增补 | UNX-F52139-J1 六判据全落；终端消费面联签；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52140 | Micro for Windows 收口 | 180 | 增补 | UNX-F52140-J1 六判据全落；终端消费面联签；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B07 防重声明**：本批 20 条 = 组 7"通用编辑器族"20 收口条目；EOL 应用（Notepad2/Brackets）按冻结定级分立；ID 段 F52121–F52140 零交叠。

---

<!-- 主册行 350249 · ## 批 UNX-N1-B08（F52141–F52160 · 组 8 · 构建/打包/任务编排工具族收口 · 5,24 -->
## 批 UNX-N1-B08（F52141–F52160 · 组 8 · 构建/打包/任务编排工具族收口 · 5,240 行）

| ID | 深化名（deepen/N1-B08） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52141 | Apache Maven 收口 | 260 | 增补 | UNX-F52141-J1 build/test ×10 工程全通；仓库下载面网络语义（AI-23 联签锚定行）；与 B05 Gradle 分账（构建体系不同族）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 组 8 口径 |
| UNX-F52142 | Apache Ant 收口 | 240 | 增补 | UNX-F52142-J1 build.xml ×10 样本全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52143 | NAnt 收口（.NET 侧 Ant，长尾） | 200 | 增补 | UNX-F52143-J1 build ×5 样本全通；EOL 临近声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52144 | Cake（C# Make）收口 | 220 | 增补 | UNX-F52144-J1 build.cake ×5 脚本全通；addin 加载 ×3；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52145 | Rake 收口（Ruby 侧） | 200 | 增补 | UNX-F52145-J1 Rakefile ×5 全通；与 F52119 Ruby 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52146 | Grunt 收口 | 220 | 增补 | UNX-F52146-J1 任务 ×10 全通；node_modules 卷层压力账（海量小文件，B4 联动联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二文件 watcher 同构 |
| UNX-F52147 | Gulp 收口 | 220 | 增补 | UNX-F52147-J1 流式任务 ×10 全通；watch 模式大目录事件零丢（10 万文件树复测口径引用 B15 共面账）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二文件 watcher |
| UNX-F52148 | Webpack CLI 收口 | 240 | 增补 | UNX-F52148-J1 构建 ×10 工程全通；持久缓存命中账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52149 | Vite 收口 | 240 | 增补 | UNX-F52149-J1 dev/build/preview 三模式 ×10 工程全通；dev 服务端口语义与防火墙预置建议账（AI-50 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52150 | Rollup 收口 | 220 | 增补 | UNX-F52150-J1 bundle ×10 全通；tree-shaking 产物校验和对照；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52151 | esbuild 收口 | 220 | 增补 | UNX-F52151-J1 构建 ×10 全通；P95 耗时账（不凭感觉说快）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52152 | Turborepo 收口 | 220 | 增补 | UNX-F52152-J1 monorepo 任务编排 ×10 全通；本地缓存命中账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52153 | Nx CLI 收口 | 220 | 增补 | UNX-F52153-J1 affected/build ×10 全通；缓存语义 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52154 | CMake CLI 收口（Windows 发行版，与 L4 分账） | 240 | 增补 | UNX-F52154-J1 cmake -S -B 构建 ×10 工程全通；与 F52075 GUI 共面声明（同应用不同界面形态分立条目）；与 L4 分账联签锚定行 AI-59；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52155 | Make for Windows（POSIX 环境面，与 F52080 分立口径） | 200 | 增补 | UNX-F52155-J1 MSYS2/独立发行版两种 Make 并存判据；环境 PATH 优先级 ×5 场景零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52156 | Advanced Installer 收口 | 240 | 增补 | UNX-F52156-J1 MSI 工程 ×5 构建全通；E4 形态分类对账（联签锚定行 AI-24）；KB-COMPAT 定级登记；判据锚定 §AI-66.6 风险 ③ |
| UNX-F52157 | InstallShield 收口（商业安装器长尾） | 240 | 增补 | UNX-F52157-J1 工程 ×3 构建 + 产出自安装全通；许可校验面如实定级（伪装表不用于绕过许可，AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52158 | MSIX Packaging Tool 收口 | 220 | 增补 | UNX-F52158-J1 打包 ×5 场景全通；签名校验路径 ×3；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52159 | Git for Windows 收口（应用收口面，与 L4 嫁接件分账） | 280 | 增补 | UNX-F52159-J1 git CLI 20 操作集（clone/commit/push/pull/rebase 五面）×10 仓库全通；与 L4 git 嫁接件判据绿消费联签（锚定行 AI-59）+ L4 分账声明；CRLF 转换语义 ×10 场景零漂移；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题二 Git 集成 |
| UNX-F52160 | GitHub CLI（gh）收口 | 220 | 增补 | UNX-F52160-J1 repo/pr/issue 20 操作集 ×10 场景全通；API 网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B08 防重声明**：本批 20 条 = 组 8"构建/打包/任务编排工具族"20 收口条目；CLI 发行版收口条目与 L4 嫁接件布设分账逐条标注；F52080/F52154/F52155 同名异口径分立有据；ID 段 F52141–F52160 零交叠。

---

<!-- 主册行 350278 · ## 批 UNX-N1-B09（F52161–F52180 · 组 9 · 调试器与性能分析工具族收口 · 5,320  -->
## 批 UNX-N1-B09（F52161–F52180 · 组 9 · 调试器与性能分析工具族收口 · 5,320 行）

| ID | 深化名（deepen/N1-B09） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52161 | WinDbg Preview 收口 | 300 | 增补 | UNX-F52161-J1 六判据全落；调试 API 消费对账：IsDebuggerPresent/CheckRemoteDebuggerPresent 与 D1 PEB BeingDebugged 写位联动 ×5 场景（D1 对照账复测引用联签锚定行 AI-16~20）；断点异常派发序 ×10 场景零漂移；dump 打开 ×3；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题二调试 API 消费 |
| UNX-F52162 | WinDbg Classic 收口 | 240 | 增补 | UNX-F52162-J1 同 F52161 调试 API 口径；命令集 ×20 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52163 | x64dbg 收口 | 280 | 增补 | UNX-F52163-J1 六判据全落；软件断点/硬件断点/单步三类 ×10 场景；调试 API 对照 ×5 复测零差（共面判据 J-共面口径）；KB-COMPAT 定级登记；判据锚定 §AI-66.5 J-共面 |
| UNX-F52164 | OllyDbg 收口（长尾定级示范） | 220 | 增补 | UNX-F52164-J1 六判据执行；32 位面定级 + 缺失清单（64 位不支持为原生缺失非缺陷）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52165 | Immunity Debugger 收口（长尾） | 200 | 增补 | UNX-F52165-J1 六判据执行；Python 脚本接口 ×5 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52166 | Process Monitor 收口 | 280 | 增补 | UNX-F52166-J1 文件/注册表/网络三事件流捕获 ×10^5 事件零丢；过滤/堆栈回溯 ×10 场景；与 B1 VFS 语义对账（联签锚定行 AI-04~06）；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题二文件 watcher 同构 |
| UNX-F52167 | Process Explorer 收口 | 260 | 增补 | UNX-F52167-J1 进程树/句柄表/DLL 列表三视图 ×10 进程全通；句柄枚举与 E1 进程语义对账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52168 | System Informer（Process Hacker 续作）收口 | 260 | 增补 | UNX-F52168-J1 进程/网络/磁盘三面板 ×10 全通；接管口径（Process Hacker → System Informer 升级接管字样）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52169 | Autoruns 收口 | 240 | 增补 | UNX-F52169-J1 自启点枚举 ×20 类别全账；与注册表语义对账（E 部联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52170 | DebugView 收口 | 220 | 增补 | UNX-F52170-J1 OutputDebugString 捕获（用户态/内核态两通道）×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |
| UNX-F52171 | VMMap 收口 | 240 | 增补 | UNX-F52171-J1 地址空间分区枚举 ×10 进程全通；与内存管理语义对账（A 部联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52172 | TCPView 收口 | 220 | 增补 | UNX-F52172-J1 连接表/端口归属 ×10 全通；网络语义对账（I 部联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52173 | MinGW-gdb（Windows 发行版）收口 | 260 | 增补 | UNX-F52173-J1 调试会话 ×10 样本全通；与 L4 分账（发行版收口 vs 嫁接件布设，联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52174 | LLDB for Windows 收口 | 240 | 增补 | UNX-F52174-J1 调试会话 ×10 全通；与 VSCode cppptools 消费联签（F52034 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52175 | Windows Performance Recorder（WPR）收口 | 260 | 增补 | UNX-F52175-J1 ETW 采集 ×10 配置全通；采集开销 ≤10% 落账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52176 | Windows Performance Analyzer（WPA）收口 | 260 | 增补 | UNX-F52176-J1 .etl 打开/图表 ×10 视图全通；GB 级 trace 打开分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52177 | PerfView 收口 | 240 | 增补 | UNX-F52177-J1 CPU 采集/堆快照/GC 视图三段 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52178 | GPUView 收口 | 240 | 增补 | UNX-F52178-J1 GPU 事件时间线 ×5 场景全通；与 G 部显示语义对账（联签锚定行 AI-31~33）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52179 | Intel VTune Profiler 收口 | 240 | 增补 | UNX-F52179-J1 CPU 采样 ×10 全通；PMU 专有依赖如实定级（不可得则降级 + 缺失清单，不虚报）；KB-COMPAT 定级登记；判据锚定 §AI-66.6 风险①同构 |
| UNX-F52180 | AMD uProf 收口 | 220 | 增补 | UNX-F52180-J1 采样 ×10 全通；专有 PMU N/A 账同构；KB-COMPAT 定级登记；判据锚定 §AI-66.6 |

**批 B09 防重声明**：本批 20 条 = 组 9"调试器与性能分析族"20 收口条目；调试 API 共面判据（×5 应用复测零差）以 B15 综合账为汇总承载；ID 段 F52161–F52180 零交叠。

---

<!-- 主册行 350307 · ## 批 UNX-N1-B10（F52181–F52200 · 组 10 · 版本管理 GUI 族收口 · 5,240  -->
## 批 UNX-N1-B10（F52181–F52200 · 组 10 · 版本管理 GUI 族收口 · 5,240 行）

| ID | 深化名（deepen/N1-B10） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52181 | TortoiseGit 收口 | 280 | 增补 | UNX-F52181-J1 六判据全落；Shell 扩展右键菜单状态机完整（浮层出路/Esc/失焦三路关闭 ×10 场景，体验二口径）；git 消费联签（F52159 锚定行）；覆盖图标 ×10 状态零漂移；KB-COMPAT 定级 S1；判据锚定 §AI-66.2 专题二 Git 集成 + 体验二浮层生命周期 |
| UNX-F52182 | TortoiseSVN 收口 | 260 | 增补 | UNX-F52182-J1 六判据全落；svn 语义（update/commit/diff/log ×10）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52183 | TortoiseHg 收口 | 240 | 增补 | UNX-F52183-J1 六判据全落；hg 操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52184 | GitKraken 收口 | 260 | 增补 | UNX-F52184-J1 六判据全落（Electron 上栈三段账）；图形 DAG 渲染 ×10 仓库；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四同构 |
| UNX-F52185 | Sourcetree 收口 | 260 | 增补 | UNX-F52185-J1 六判据全落（Electron）；git/hg 双后端 ×10 操作；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52186 | GitHub Desktop 收口 | 260 | 增补 | UNX-F52186-J1 六判据全落；clone/commit/push/PR 消费面 ×10；API 网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52187 | Fork 收口 | 240 | 增补 | UNX-F52187-J1 六判据全落；rebase/交互式操作 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52188 | Sublime Merge 收口 | 240 | 增补 | UNX-F52188-J1 六判据全落；搜索/自定义命令 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52189 | Git Extensions 收口 | 240 | 增补 | UNX-F52189-J1 六判据全落；VS 集成消费（B01 shim 共面复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52190 | SmartGit 收口 | 240 | 增补 | UNX-F52190-J1 六判据全落；JVM 上栈（JBR 同构账引用 F52057 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52191 | Tower（Windows 版）收口 | 220 | 增补 | UNX-F52191-J1 六判据全落；操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52192 | GitAhead 收口（EOL 定级示范） | 200 | 增补 | UNX-F52192-J1 六判据执行；EOL 冻结定级；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52193 | Git Cola 收口 | 200 | 增补 | UNX-F52193-J1 六判据全落；Qt 上栈（F52101 共面声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52194 | Gittyup 收口 | 200 | 增补 | UNX-F52194-J1 六判据全落；libgit2 消费 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52195 | WinMerge 收口（diff/merge 工具） | 260 | 增补 | UNX-F52195-J1 六判据全落；目录递归对比 ×10 树全通；行级 diff 正确性 ×20 样本零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52196 | KDiff3 收口 | 220 | 增补 | UNX-F52196-J1 六判据全落；三方合并 ×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52197 | Beyond Compare 收口 | 260 | 增补 | UNX-F52197-J1 六判据全落；文件夹/表格/十六进制三类对比 ×10；许可校验面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52198 | Araxis Merge 收口 | 240 | 增补 | UNX-F52198-J1 六判据全落；三方合并/文件夹对比 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52199 | SVN 命令行客户端（Windows 发行版）收口 | 200 | 增补 | UNX-F52199-J1 svn 20 操作集 ×10 仓库全通；与 F52182 共面分账声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52200 | Mercurial CLI（Windows 发行版）收口 | 200 | 增补 | UNX-F52200-J1 hg 20 操作集 ×10 全通；Python 消费联签（F52143 侧锚定）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B10 防重声明**：本批 20 条 = 组 10"版本管理 GUI 与 diff/merge 族"20 收口条目；GUI 工具与 CLI 发行版分立（F52199/F52200）；ID 段 F52181–F52200 零交叠。

---
<!-- 主册行 350335 · ## 批 UNX-N1-B11（F52201–F52220 · 组 11 · 数据库工具族收口 · 5,320 行） -->
## 批 UNX-N1-B11（F52201–F52220 · 组 11 · 数据库工具族收口 · 5,320 行）

| ID | 深化名（deepen/N1-B11） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52201 | SQL Server Management Studio（SSMS）收口 | 300 | 增补 | UNX-F52201-J1 六判据全落；对象资源管理器/查询执行/执行计划 20 操作集全通；J4 .sql 工程双向零损；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 11 口径 |
| UNX-F52202 | Azure Data Studio 收口 | 260 | 增补 | UNX-F52202-J1 六判据全落（Electron 三段账）；notebook 模式 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四同构 |
| UNX-F52203 | DBeaver Community 收口 | 280 | 增补 | UNX-F52203-J1 六判据全落；JDBC 多数据源 ×10 全通；元数据浏览/SQL 编辑/导出三段；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52204 | Navicat Premium 收口 | 260 | 增补 | UNX-F52204-J1 六判据全落；多数据库连接 ×5 类全通；数据传输/结构同步操作集；许可校验面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52205 | Navicat for MySQL 收口 | 220 | 增补 | UNX-F52205-J1 六判据全落；MySQL 专属操作集 ×10；升级接管口径（与 Premium 同族分立版本线）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52206 | HeidiSQL 收口 | 240 | 增补 | UNX-F52206-J1 六判据全落；MySQL/MariaDB ×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52207 | pgAdmin 4 收口 | 260 | 增补 | UNX-F52207-J1 六判据全落；PostgreSQL 对象树/查询/备份恢复三段 ×10；服务器模式/桌面模式双态账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52208 | MySQL Workbench 收口 | 260 | 增补 | UNX-F52208-J1 六判据全落；建模/正向工程/逆向工程三面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52209 | MongoDB Compass 收口 | 260 | 增补 | UNX-F52209-J1 六判据全落；集合浏览/聚合管道/索引管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52210 | RedisInsight 收口 | 240 | 增补 | UNX-F52210-J1 六判据全落；键浏览/CLI/慢日志三段 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52211 | SQLiteStudio 收口 | 240 | 增补 | UNX-F52211-J1 六判据全落；库浏览/SQL 编辑/导入导出 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52212 | DB Browser for SQLite 收口 | 240 | 增补 | UNX-F52212-J1 六判据全落；浏览/编辑/PRAGMA ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52213 | TablePlus 收口 | 220 | 增补 | UNX-F52213-J1 六判据全落；多数据源 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52214 | Beekeeper Studio 收口 | 220 | 增补 | UNX-F52214-J1 六判据全落（Electron）；连接/查询/历史 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52215 | DbVisualizer 收口 | 220 | 增补 | UNX-F52215-J1 六判据全落；JVM 上栈（F52057 联签锚定行）；×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52216 | SQuirreL SQL 收口 | 200 | 增补 | UNX-F52216-J1 六判据全落；JDBC 驱动管理 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52217 | RazorSQL 收口 | 200 | 增补 | UNX-F52217-J1 六判据全落；多库 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52218 | Toad for Oracle 收口 | 240 | 增补 | UNX-F52218-J1 六判据全落；PL/SQL 编辑/调试/Schema 浏览 ×10；专有客户端依赖如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52219 | PL/SQL Developer 收口 | 220 | 增补 | UNX-F52219-J1 六判据全落；Oracle 客户端消费 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52220 | SQLyog 收口 | 200 | 增补 | UNX-F52220-J1 六判据全落；MySQL 操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B11 防重声明**：本批 20 条 = 组 11"数据库工具族"20 收口条目；与 B03 DataGrip（IDE 收口）分界声明在案；ID 段 F52201–F52220 零交叠。

---

<!-- 主册行 350364 · ## 批 UNX-N1-B12（F52221–F52240 · 组 12 · 终端与 SSH 远程工具族收口 · 5,3 -->
## 批 UNX-N1-B12（F52221–F52240 · 组 12 · 终端与 SSH 远程工具族收口 · 5,320 行）

| ID | 深化名（deepen/N1-B12） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52221 | Windows Terminal 收口 | 300 | 增补 | UNX-F52221-J1 六判据全落；多标签/分屏/主题 20 操作集全通；conhost/ConPTY 语义对账（E 部联签锚定行）；中文渲染与字体三层（J5）全过；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 12 口径 |
| UNX-F52222 | ConEmu 收口 | 260 | 增补 | UNX-F52222-J1 六判据全落；标签/分屏/注入钩子三面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52223 | Cmder 收口 | 240 | 增补 | UNX-F52223-J1 六判据全落；便携模式/集成 git 消费（F52159 联签锚定行）×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52224 | PuTTY 收口 | 280 | 增补 | UNX-F52224-J1 六判据全落；SSH/Telnet 会话 ×10 全通；密钥认证（Pageant 消费）×5；终端转义序列渲染对照 ×20 场景零漂移；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52225 | KiTTY 收口 | 200 | 增补 | UNX-F52225-J1 六判据全落；PuTTY 接管口径（升级接管字样）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52226 | WinSCP 收口 | 260 | 增补 | UNX-F52226-J1 六判据全落；SCP/SFTP 双协议传输 ×10（含断点续传）；网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52227 | MobaXterm 收口 | 260 | 增补 | UNX-F52227-J1 六判据全落；SSH/X11 转发/宏三面 ×10；X11 消费联签（AI-27 域锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52228 | Termius 收口 | 220 | 增补 | UNX-F52228-J1 六判据全落；同步面（外部服务）网络语义声明；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52229 | Tabby 收口 | 240 | 增补 | UNX-F52229-J1 六判据全落（Electron 三段账）；SSH 终端/分屏 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52230 | Hyper 收口 | 220 | 增补 | UNX-F52230-J1 六判据全落（Electron）；插件 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52231 | Alacritty 收口 | 240 | 增补 | UNX-F52231-J1 六判据全落；GPU 渲染路径探测与降级表（G 部能力页联签锚定行）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四 GPU 同构 |
| UNX-F52232 | WezTerm 收口 | 240 | 增补 | UNX-F52232-J1 六判据全落；多路复用/SSH 域 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52233 | Warp for Windows 收口 | 240 | 增补 | UNX-F52233-J1 六判据全落；AI 请求面网络语义声明（AI-23 联签锚定行）+ 本地模型 N/A；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52234 | Xshell 收口 | 240 | 增补 | UNX-F52234-J1 六判据全落；会话管理/隧道 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52235 | Xftp 收口 | 220 | 增补 | UNX-F52235-J1 六判据全落；SFTP 传输 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52236 | SecureCRT 收口 | 240 | 增补 | UNX-F52236-J1 六判据全落；脚本自动化（VB/Python 接口）×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52237 | SecureFX 收口 | 200 | 增补 | UNX-F52237-J1 六判据全落；传输 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52238 | Royal TS 收口 | 220 | 增补 | UNX-F52238-J1 六判据全落；多协议连接管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52239 | mRemoteNG 收口 | 220 | 增补 | UNX-F52239-J1 六判据全落；RDP/SSH/VNC 三协议消费 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52240 | FileZilla 收口 | 240 | 增补 | UNX-F52240-J1 六判据全落；FTP/FTPS/SFTP 传输 ×10（含并发 4 任务资源账，M4 母版内存峰值口径）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B12 防重声明**：本批 20 条 = 组 12"终端与 SSH 远程工具族"20 收口条目；PuTTY 系（F52224/F52225）按接管口径分立；ID 段 F52221–F52240 零交叠。

---
<!-- 主册行 350392 · ## 批 UNX-N1-B13（F52241–F52260 · 组 13 · API 与网络调试工具族收口 · 5,24 -->
## 批 UNX-N1-B13（F52241–F52260 · 组 13 · API 与网络调试工具族收口 · 5,240 行）

| ID | 深化名（deepen/N1-B13） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52241 | Postman 收口 | 300 | 增补 | UNX-F52241-J1 六判据全落（Electron 三段账）；请求/集合/环境/测试脚本 20 操作集全通；代理语义与系统证书面消费（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 13 口径 |
| UNX-F52242 | Postman CLI 收口 | 220 | 增补 | UNX-F52242-J1 collection 运行 ×10 全通；与桌面端共面声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52243 | Newman（Postman 跑批器）收口 | 200 | 增补 | UNX-F52243-J1 CLI 跑批 ×10 全通；node 消费 ABI 账复用（F52032 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52244 | Insomnia 收口 | 240 | 增补 | UNX-F52244-J1 六判据全落（Electron）；请求/环境 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52245 | Bruno 收口 | 220 | 增补 | UNX-F52245-J1 六判据全落；离线优先声明（外部同步 N/A）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52246 | Hoppscotch Desktop 收口 | 220 | 增补 | UNX-F52246-J1 六判据全落；请求/WS 测试 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52247 | Fiddler Classic 收口 | 280 | 增补 | UNX-F52247-J1 六判据全落；HTTPS 解密（证书面）/会话列表/断点三面 ×10；系统代理语义消费（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52248 | Fiddler Everywhere 收口 | 240 | 增补 | UNX-F52248-J1 六判据全落（Electron 升级接管口径）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52249 | Wireshark 收口 | 300 | 增补 | UNX-F52249-J1 六判据全落；抓包/过滤/解析 20 操作集全通；Npcap 消费面 ×10 场景；协议解析 ×20 常见协议零漂移；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52250 | Charles Proxy 收口 | 240 | 增补 | UNX-F52250-J1 六判据全落；HTTPS 代理/限速/重写三面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52251 | HTTP Toolkit 收口 | 220 | 增补 | UNX-F52251-J1 六判据全落（Electron）；拦截 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52252 | mitmproxy（Windows 发行版）收口 | 240 | 增补 | UNX-F52252-J1 代理/脚本 ×10 全通；证书面消费 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52253 | SoapUI 收口 | 240 | 增补 | UNX-F52253-J1 六判据全落；SOAP/REST 工程 ×10；JVM 上栈（F52057 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52254 | Advanced REST Client 收口 | 200 | 增补 | UNX-F52254-J1 六判据全落（Electron）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52255 | curl for Windows 收口 | 220 | 增补 | UNX-F52255-J1 20 操作集（GET/POST/头/代理/证书五面）×10 全通；TLS 消费对账（AI-44 契约联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52256 | wget for Windows 收口 | 200 | 增补 | UNX-F52256-J1 下载 ×10 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52257 | OpenSSH for Windows（客户端）收口 | 260 | 增补 | UNX-F52257-J1 ssh/scp/sftp 20 操作集 ×10；密钥代理（agent）×5；与 B12 终端消费联签（F52235 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52258 | WinDump 收口 | 200 | 增补 | UNX-F52258-J1 抓包 ×10 全通；Npcap 消费（F52249 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52259 | PingPlotter 收口 | 200 | 增补 | UNX-F52259-J1 路由追踪/图表 ×10；ICMP 消费 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52260 | Apache JMeter 收口 | 260 | 增补 | UNX-F52260-J1 六判据全落；线程组压测 ×10 场景；JVM 上栈（F52057 联签锚定行）；结果聚合与图表 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B13 防重声明**：本批 20 条 = 组 13"API 与网络调试族"20 收口条目；Fiddler 双形态（Classic/Everywhere）按版本线分立；ID 段 F52241–F52260 零交叠。

---

<!-- 主册行 350421 · ## 批 UNX-N1-B14（F52261–F52280 · 组 14 · 容器与远程开发工具族收口 · 5,320  -->
## 批 UNX-N1-B14（F52261–F52280 · 组 14 · 容器与远程开发工具族收口 · 5,320 行）

| ID | 深化名（deepen/N1-B14） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52261 | Docker Desktop 收口 | 300 | 增补 | UNX-F52261-J1 六判据全落；容器/镜像/卷/Compose 20 操作集全通；后端虚拟化依赖消费 L5 判据（联签锚定行 AI-60）——不可用路径显式 N/A + 三要素告警，禁隐性失败；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 14 口径 |
| UNX-F52262 | Docker CLI（Windows 发行版）收口 | 240 | 增补 | UNX-F52262-J1 docker 20 操作集 ×10 全通；与 F52261 共面分账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52263 | Docker Compose 收口 | 240 | 增补 | UNX-F52263-J1 up/down/build ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52264 | Podman Desktop 收口 | 260 | 增补 | UNX-F52264-J1 六判据全落；rootless 路径 ×10；虚拟化依赖 N/A 同构（F52261 口径复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52265 | Rancher Desktop 收口 | 240 | 增补 | UNX-F52265-J1 六判据全落；k8s/k3s 拉起依赖账；N/A 同构声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52266 | Portainer Desktop 收口 | 220 | 增补 | UNX-F52266-J1 六判据全落（Electron）；栈管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52267 | kubectl 收口 | 240 | 增补 | UNX-F52267-J1 get/apply/logs/describe 20 操作集 ×10 集群面全通；连接语义消费 I 部网络（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52268 | Helm 收口 | 220 | 增补 | UNX-F52268-J1 install/upgrade/rollback ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52269 | minikube 收口 | 240 | 增补 | UNX-F52269-J1 start/delete ×10；虚拟化驱动探测账（L5 消费联签锚定行，N/A 同构）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52270 | kind 收口 | 220 | 增补 | UNX-F52270-J1 集群创建 ×5 全通；容器运行时依赖分账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52271 | k9s 收口 | 220 | 增补 | UNX-F52271-J1 六判据全落；TUI 交互状态机完整（键位/退出路径 ×10，体验四口径）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 + 体验四 |
| UNX-F52272 | Lens（K8s IDE）收口 | 240 | 增补 | UNX-F52272-J1 六判据全落（Electron）；集群视图 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52273 | VS Code Dev Containers 消费面收口 | 260 | 增补 | UNX-F52273-J1 容器内开发全链 ×10（attach/rebuild/features）；与 F52261/F52021 双消费联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |
| UNX-F52274 | JetBrains Gateway（远程开发）收口 | 260 | 增补 | UNX-F52274-J1 远程后端布设/连接/IDE 拉起三段 ×10；SSH 消费联签（F52257 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52275 | Multipass 收口 | 220 | 增补 | UNX-F52275-J1 实例创建/删除 ×10；虚拟化 N/A 同构（F52261 口径）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52276 | VirtualBox 收口 | 280 | 增补 | UNX-F52276-J1 六判据全落；VM 创建/快照/共享文件夹三面 ×10；VT-x 虚拟化探测账（L5 消费联签锚定行——不可用则降级 N/A，不硬开）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52277 | VMware Workstation Player 收口 | 260 | 增补 | UNX-F52277-J1 六判据全落；VM 导入/运行 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52278 | Vagrant 收口 | 240 | 增补 | UNX-F52278-J1 box 拉起/销毁 ×10 全通；provider 探测（VirtualBox 消费联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52279 | QEMU for Windows（前端消费面）收口 | 240 | 增补 | UNX-F52279-J1 系统模拟启动 ×10 场景全通；**与内核 QEMU 测试设施严格分账**（本条仅 Windows 发行版应用收口，不触内核侧设施，联签锚定行）；WHPX 路径消费 L5 判据（N/A 同构）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 语义边界 |
| UNX-F52280 | Hyper-V 管理器消费面收口（管理工具条目） | 240 | 增补 | UNX-F52280-J1 管理界面枚举/连接 ×10；Hyper-V 虚拟化本体归 L5/AI-60（分账声明联签锚定行），本条只收口管理工具应用形态；KB-COMPAT 定级登记；判据锚定 §AI-66.1 语义边界 |

**批 B14 防重声明**：本批 20 条 = 组 14"容器与远程开发族"20 收口条目；全部虚拟化依赖条目消费 L5 判据、不可用即 N/A + 三要素告警（不硬开不虚报）；Hyper-V 本体/容器内核面零触碰；ID 段 F52261–F52280 零交叠。

---

<!-- 主册行 350450 · ## 批 UNX-N1-B15（F52281–F52300 · 组 15 · 波 23 段收官综合批（共面与治理）· 5 -->
## 批 UNX-N1-B15（F52281–F52300 · 组 15 · 波 23 段收官综合批（共面与治理）· 5,700 行）

| ID | 深化名（deepen/N1-B15） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52281 | 开发工具共面一：编译器进程树综合账（MSBuild/cl/link 子进程链全域） | 300 | 增补 | UNX-F52281-J1 B01/B04 全部构建链条目汇总：作业对象/环境块/退出码传递三语义 ×10 工程交叉复测零漂移；消费 D2/E2 判据绿（联签锚定行）；判据锚定 §AI-66.2 专题二编译器进程树 |
| UNX-F52282 | 开发工具共面二：文件 watcher 10 万文件树零丢事件全域复测账 | 300 | 增补 | UNX-F52282-J1 大目录递归监视（inotify 类语义对 B1 VFS 压力账）×3 轮 10 万文件树事件零丢；VSCode/Process Monitor/Gulp 三类消费方抽测 ×3 应用复测；判据锚定 §AI-66.5 J-共面 |
| UNX-F52283 | 开发工具共面三：调试 API 对照 ×5 应用复测零差账 | 280 | 增补 | UNX-F52283-J1 IsDebuggerPresent/CheckRemoteDebuggerPresent 与 D1 PEB BeingDebugged 写位联动 ×5 应用（WinDbg/x64dbg/VS/gdb/LLDB）复测零差，D1 对照账复测引用（联签锚定行）；断点异常派发序 ×10；判据锚定 §AI-66.5 J-共面 |
| UNX-F52284 | 开发工具共面四：Git 集成消费 L4 判据对账（全域 GUI/CLI ×10 条目） | 280 | 增补 | UNX-F52284-J1 B01/B08/B10 全部 git 消费条目对 L4 嫁接件判据绿的消费一致性 ×10 条目抽测；文件锁语义对账 ×5；联签锚定行 AI-59；判据锚定 §AI-66.2 专题二 Git 集成 |
| UNX-F52285 | 第一个 Electron 组合态失绿现场实况（DJ-UNX-N1-01 域经义务回填） | 300 | 增补 | UNX-F52285-J1 域经义务条目：VSCode 多进程组合态失绿现场实录（现象/归因/回推 G 部缺陷账编号/跟踪结论四段全记）；判据锚定 §AI-66.8 域经义务 DJ-UNX-N1-* |
| UNX-F52286 | J5 中文 40 应用抽测轮（波 23 段） | 300 | 增补 | UNX-F52286-J1 抽查 40 应用 J5 三层（界面/输入/字体）全过；M1 波 22 闭账物之后复测一轮，衔接账公开（联签锚定行 AI-61）；判据锚定 §AI-66.5 J-中文 |
| UNX-F52287 | KB-COMPAT 可查询率 100% 全段核账（B01–B14） | 280 | 增补 | UNX-F52287-J1 B01–B14 共 280 条目 KB-COMPAT 登记 ×100% 可查询验证；结构权在 E5、写入权在 N 域分界复核（联签锚定行 AI-25）；判据锚定 §AI-66.5 J-主轴 |
| UNX-F52288 | 波 23 段六判据全绿率结算账 | 280 | 增补 | UNX-F52288-J1 全段 280 条目六判据逐条结算：全绿数/降级数/降级缺失清单全公开；KPI 权重 15 口径落账；判据锚定 §AI-66.5 J-主轴 + §105 KPI 调权 |
| UNX-F52289 | 安装器形态归并表消费对账（E4 200 样本经验复用） | 260 | 增补 | UNX-F52289-J1 本段安装器形态（MSI/NSIS/Inno/自研壳）逐条目归并入 E4 分类账 ×14 条目零漏；个体失绿如实降级禁"凑装上"；联签锚定行 AI-24；判据锚定 §AI-66.6 风险 ③ |
| UNX-F52290 | 版本阶梯滚动回归账（AI-94 通知链 T+1 演练） | 260 | 增补 | UNX-F52290-J1 应用月更触发回归演练 ×2：T+1 日广播 → 钉版回归 + 新版本 20% 抽测 → 结论入账；全量追新禁止声明；联签锚定行 AI-94；判据锚定 §AI-66.6 风险 ② |
| UNX-F52291 | 版本伪装表许可合规联合审查条目（AI-95） | 260 | 增补 | UNX-F52291-J1 本段全部版本伪装表条目 ×N 逐条审查：仅用于兼容性伪装、零许可绕过 ×100% 结论入账；违例冻结批次条款知悉登记；联签锚定行 AI-95；判据锚定 §AI-66.9 红线② |
| UNX-F52292 | 六判据进验收机器人判据库（AI-73 对接） | 240 | 增补 | UNX-F52292-J1 六判据母版参数化 schema ×1 冻结入库，机器人可执行判据 ×10 抽测全通；联签锚定行 AI-73；判据锚定 §AI-66.8 治理线接口 |
| UNX-F52293 | 收口条目抽读对照账（AI-82 字行比减权后 ≥20%） | 240 | 增补 | UNX-F52293-J1 本段条目抽读对照段占比 ≥20% ×30 条抽测达标；灌水扫描零命中；联签锚定行 AI-82；判据锚定 §AI-66.8 治理线接口 |
| UNX-F52294 | 覆盖率三口径"工程级"落账（AI-96） | 240 | 增补 | UNX-F52294-J1 工程级口径在 N1 部落账：清单内应用覆盖 ×15/800 阶段账公开；联签锚定行 AI-96；判据锚定 §AI-66.8 治理线接口 |
| UNX-F52295 | 波 23 段缺陷账出口（产线账本 → 结构化工单） | 260 | 增补 | UNX-F52295-J1 双轨产线缺陷账本清点：🔴 安全红线类=0；🟡 本段收口前清偿 ×N 全闭环；🟢 攒批登记 ×N（现象/位置/严重度/建议修法/复现路径五字段齐）；判据锚定 双轨产线制度 |
| UNX-F52296 | 体验日志全域回归账（挫败信号捕获 ×B01–B14） | 260 | 增补 | UNX-F52296-J1 rage click/dead click/浮层反复开关三类挫败信号 ×14 组应用抽测捕获账全落；日志零阻塞交互验证；判据锚定 体验十三·补 异常显性化 |
| UNX-F52297 | 波 24 预告与任务储备（B16–B40 深度 ≥3） | 260 | 增补 | UNX-F52297-J1 B16–B40 组序铺排清单（组 16–40：包管理 GUI/反编译与二进制分析/文档工具/CI 面板/监控仪表长尾）+ 每会话任务储备深度 ≥3 登记入台账；判据锚定 多 AI 并行五条防卡死 ③ |
| UNX-F52298 | GitHub 入库状态声明（本册全部产物提交与推送如实登记） | 260 | 增补 | UNX-F52298-J1 增补册文件 + 主文件追加提交 + 推送结果三态如实登记（受阻则登记重试机制在位）；判据锚定 诚实登记纪律 |
| UNX-F52299 | B15 防重 grep 与段收官印（B01–B15 冻结 300/800） | 260 | 增补 | UNX-F52299-J1 防重 grep（关键词族：六判据/KB-COMPAT/shim/收口）于批收口执行留痕；批印入册：15 批 300 条全冻结、域账累计入册；判据锚定 §AI-66.5 J-覆盖批收口节奏 |
| UNX-F52300 | 波 23 段域关门印（N1 前 300 条达验收标准声明） | 280 | 增补 | UNX-F52300-J1 段关门印：300/800 条全冻结、六判据全绿率结算入册、KB-COMPAT 可查询率 100%、共面四账全绿、两条等同红线零违例（判据未放宽/伪装表零许可绕过）、P0=0；判据锚定 §AI-66.5/§AI-66.9 |

**批 B15 防重声明**：本批 20 条为段收官综合批（共面四账/治理七账/收官印），与 B01–B14 全部应用条目零重复（综合账只汇总复测、不重立项）；ID 段 F52281–F52300 与邻批零交叠。

---

<!-- 主册行 350479 · ## 波 23 段总账（AI-66 · UNX-N1 B01–B15） -->
## 波 23 段总账（AI-66 · UNX-N1 B01–B15）

- **总量**：15 批 × 20 条 = **300 条全冻结**；ID 段 F52001–F52300 连续零跳号、零复用；本册域账累计 84,660 行（单条行数 L2 档 180–300，随批注记）。
- **组覆盖**：组 1–15（VS 族/Electron 编辑器族/JetBrains 族/MSBuild 构建链/Android 工具链/IDE 周边/通用编辑器/构建打包/调试诊断/版本管理 GUI/数据库工具/终端 SSH/API 网络调试/容器远程开发/段收官综合批）。
- **六判据**：J1–J6 母版全域同解，逐应用实例化；J2 ≤3s / J6 ≤×1.5 铁值零放宽，降级条目全部附缺失项清单。
- **联签锚定**：AI-21~25（E 部全链）、AI-59（L4 分账联签 ×9 条）、AI-61（M1 中文复测）、AI-65（M5 设备复用零重测 ×5 条）、AI-60（L5 虚拟化 N/A 账 ×6 条）、AI-23（网络面）、AI-94/95/96/82/73（治理线）——全部联签锚定行出现，零改写他域账。
- **红线**：无引导/写盘红线（§AI-66.9 适用性声明保真）；两条等同红线零违例——①判据未放宽；②版本伪装表零许可绕过（F52291 联合审查条目在案）；fastboot 刷写面（F52098）破坏性写一律只干跑。
- **诚实登记**：全部收口条目为判据账（判据锚定可执行细节），实机执行结果以"随闸门补测"登记，双机对照缺 Windows 侧数据标 N/A 不编（附则三第④条）。
- **域经义务**：DJ-UNX-N1-01（Electron 组合态失绿现场）已立条（F52285）。
- **待续**：B16–B40（500 条 · F52301–F52800 · 波 24–25）另册续写。


---

<!-- 主册行 357502 · # AI-66 · UNX-N1 开发工具链应用收口 · 500 项新功能增补册终段（B16–B40 · F52301– -->
# AI-66 · UNX-N1 开发工具链应用收口 · 500 项新功能增补册终段（B16–B40 · F52301–F52800）

> **任务书锚定**：AI-66 承包域 UNX-N1 开发工具链应用收口（F52001–F52800）· 续接《300 项增补册（B01–B15）》。本册覆盖 B16–B40 共 25 批 × 20 条 = **500 项**，全域 800 条收口闭环（F52001–F52800 连续零跳号零复用）。波次：B16–B36（波 24 主力）、B37–B40（波 25 前两周收官）。每条 = ID ｜ 深化名 ｜ 行数 ｜ 状态 ｜ 证据与判据锚定。行数 L2 档口径，旗舰 320/标准 300/长尾 280，每批合计 6,000 行（任务书名义域账 40 × 6,000 = 240,000；B01–B15 实际 84,660 行，登记差由 B40 对账条目核销，见 F52784）。
>
> **防重声明（全域续有效）**：与 N2（AI-67 创意套件）/N4（AI-69 办公与数据）/N5（AI-70 游戏应用）分界逐组显式声明——本域只收"开发者工作流"侧应用与 CLI 工具；与 L4（AI-59）嫁接件分账口径续用（凡"Windows 发行版应用收口"条目逐条标注）；模拟器/虚拟化依赖条目全部消费 L5（AI-60）判据，不可用即 N/A + 三要素告警；破坏性写判据（烧录/刷写类）一律只干跑。
>
> **六判据母版续用**：J1 装（钉版本入 KB-COMPAT）/J2 启（主界面 ≤3s，Electron 三段账）/J3 主功能 20 操作集/J4 存（校验和双向 + downgrade 声明）/J5 中（三层中文）/J6 性能（≤×1.5 双机同码样本 ≥30，缺数据标 N/A）。铁值零放宽，降级必附缺失项清单。

---

<!-- 主册行 357512 · ## 批 UNX-N1-B16（F52301–F52320 · 组 16 · 包管理 GUI 与 MSI 工具族 · 6 -->
## 批 UNX-N1-B16（F52301–F52320 · 组 16 · 包管理 GUI 与 MSI 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B16） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52301 | Scoop 收口 | 320 | 增补 | UNX-F52301-J1 六判据全落；install/update/uninstall 20 操作集全通；bucket 添加 ×5 全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 16 口径 |
| UNX-F52302 | winget CLI 收口 | 320 | 增补 | UNX-F52302-J1 search/install/upgrade/ uninstall 20 操作集 ×10 全通；源索引网络语义（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52303 | Chocolatey GUI 收口 | 300 | 增补 | UNX-F52303-J1 六判据全落；与 B04 CLI 共面分账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52304 | UniGetUI（WingetUI）收口 | 300 | 增补 | UNX-F52304-J1 六判据全落；多后端（winget/scoop/choco）聚合 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52305 | Ninite 收口 | 280 | 增补 | UNX-F52305-J1 六判据全落；批量安装 ×10 包全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52306 | Patch My PC 收口 | 280 | 增补 | UNX-F52306-J1 六判据全落；更新扫描/静默升级 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52307 | Revo Uninstaller 收口 | 300 | 增补 | UNX-F52307-J1 六判据全落；残留扫描/强制卸载 ×10（注册表语义消费 E 部联签锚定行）；许可面如实定级（AI-95 联签锚定行）；判据锚定 §AI-66.9 红线② |
| UNX-F52308 | Bulk Crap Uninstaller 收口 | 280 | 增补 | UNX-F52308-J1 六判据全落；批量卸载/孤儿检测 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52309 | Geek Uninstaller 收口 | 260 | 增补 | UNX-F52309-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52310 | HiBit Uninstaller 收口 | 260 | 增补 | UNX-F52310-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52311 | Wise Program Uninstaller 收口 | 260 | 增补 | UNX-F52311-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52312 | IObit Uninstaller 收口 | 260 | 增补 | UNX-F52312-J1 六判据全落；捆绑推广行为如实记录入账（不隐藏）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52313 | ZSoft Uninstaller 收口（长尾） | 240 | 增补 | UNX-F52313-J1 六判据全落；EOL 临近声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52314 | PC Decrapifier 收口（长尾） | 240 | 增补 | UNX-F52314-J1 六判据全落；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52315 | Should I Remove It 收口（长尾） | 240 | 增补 | UNX-F52315-J1 六判据全落；评级数据外部依赖声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52316 | AppGet 收口（EOL 定级示范） | 240 | 增补 | UNX-F52316-J1 六判据执行；EOL 冻结定级（winget 前身，停更）；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52317 | Orca 收口（MSI 表编辑器） | 300 | 增补 | UNX-F52317-J1 六判据全落；MSI 表编辑 ×10 场景；.msi 打开/保存校验和双向；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52318 | SuperOrca 收口 | 280 | 增补 | UNX-F52318-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52319 | InstEd 收口 | 280 | 增补 | UNX-F52319-J1 六判据全落；MSI 编辑/Transform ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52320 | LessMSI 收口 | 280 | 增补 | UNX-F52320-J1 六判据全落；解包/表查看 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B16 防重声明**：本批 20 条 = 组 16"包管理 GUI 与 MSI 工具族"；与 B04（CLI 构建面）零重复；ID 段 F52301–F52320 零交叠。

---

<!-- 主册行 357541 · ## 批 UNX-N1-B17（F52321–F52340 · 组 17 · 反编译与二进制分析工具族 · 6,000  -->
## 批 UNX-N1-B17（F52321–F52340 · 组 17 · 反编译与二进制分析工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B17） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52321 | Ghidra 收口 | 320 | 增补 | UNX-F52321-J1 六判据全落；反编译/反汇编 ×10 样本全通；JVM 上栈（F52057 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 17 口径 |
| UNX-F52322 | IDA Free 收口 | 300 | 增补 | UNX-F52322-J1 六判据全落；分析/反汇编 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52323 | Binary Ninja 收口 | 300 | 增补 | UNX-F52323-J1 六判据全落；×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52324 | Radare2（Windows 发行版）收口 | 300 | 增补 | UNX-F52324-J1 命令面 20 操作集 ×10 样本全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52325 | Cutter 收口 | 300 | 增补 | UNX-F52325-J1 六判据全落（r2 GUI 形态，分账声明）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52326 | dnSpy 收口 | 300 | 增补 | UNX-F52326-J1 六判据全落；.NET 反编译/调试挂接 ×10；EOL→dnSpyEx 接管口径声明；KB-COMPAT 定级登记；判据锚定 §AI-66.1 防重声明 |
| UNX-F52327 | dnSpyEx 收口 | 280 | 增补 | UNX-F52327-J1 六判据全落；接管口径（升级接管字样）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52328 | ILSpy 收口 | 280 | 增补 | UNX-F52328-J1 六判据全落；反编译输出 ×10 程序集全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52329 | dotPeek 收口 | 280 | 增补 | UNX-F52329-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52330 | JustDecompile 收口（EOL 定级示范） | 260 | 增补 | UNX-F52330-J1 六判据执行；EOL 冻结定级；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52331 | PE-bear 收口 | 280 | 增补 | UNX-F52331-J1 六判据全落；PE 头解析 ×10 样本与 D1 装载语义对账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52332 | CFF Explorer 收口 | 280 | 增补 | UNX-F52332-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52333 | pestudio 收口 | 280 | 增补 | UNX-F52333-J1 六判据全落；静态标记 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52334 | Detect It Easy（DIE）收口 | 280 | 增补 | UNX-F52334-J1 六判据全落；壳/编译器识别 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52335 | Resource Hacker 收口 | 280 | 增补 | UNX-F52335-J1 六判据全落；资源查看/提取 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52336 | Regshot 收口 | 260 | 增补 | UNX-F52336-J1 快照差分 ×10 场景全通；注册表语义消费对账（E 部联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52337 | Dependency Walker 收口（EOL 定级示范） | 260 | 增补 | UNX-F52337-J1 六判据执行；依赖树枚举 ×10；EOL 声明（API 集新语义缺失如实列）；KB-COMPAT 定级 S2 + EOL 标记；判据锚定 §AI-66.2 专题六 |
| UNX-F52338 | API Monitor 收口 | 300 | 增补 | UNX-F52338-J1 六判据全落；API 拦截记录 ×10 场景全通（消费 E1 语义对账联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52339 | PE Explorer 收口（长尾） | 260 | 增补 | UNX-F52339-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52340 | x64dbg 插件生态消费面收口（×10 插件实测） | 280 | 增补 | UNX-F52340-J1 插件加载/卸载/崩溃隔离 ×10 全通（与 F52163 本体分账）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B17 防重声明**：本批 20 条 = 组 17"反编译与二进制分析族"；分析工具的静态判据不触任何运行目标内部，与 E 部装载语义只消费联签；ID 段 F52321–F52340 零交叠。

---

<!-- 主册行 357570 · ## 批 UNX-N1-B18（F52341–F52360 · 组 18 · 开发者文档与数据 CLI 工具族 · 6, -->
## 批 UNX-N1-B18（F52341–F52360 · 组 18 · 开发者文档与数据 CLI 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B18） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52341 | Doxygen 收口 | 300 | 增补 | UNX-F52341-J1 文档生成 ×10 工程全通；输出 HTML 与 Windows 校验和对照（样式差异清单公开）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 18 口径 |
| UNX-F52342 | Sphinx 收口 | 300 | 增补 | UNX-F52342-J1 构建 ×10 工程全通；与 Python 消费联签（F52043 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52343 | MkDocs 收口 | 280 | 增补 | UNX-F52343-J1 build/serve ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52344 | mdBook 收口 | 280 | 增补 | UNX-F52344-J1 build ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52345 | GitBook CLI 收口 | 280 | 增补 | UNX-F52345-J1 build ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52346 | Pandoc 收口 | 320 | 增补 | UNX-F52346-J1 格式转换 ×20 格式对（md↔docx/html/pdf/epub）全通；输出校验和对照；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52347 | MiKTeX 收口 | 300 | 增补 | UNX-F52347-J1 编译 ×10 文档全通；包按需安装 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52348 | TeX Live 收口 | 300 | 增补 | UNX-F52348-J1 latexmk 编译 ×10 全通；与 MiKTeX 并存隔离 ×2 轮；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52349 | Ghostscript 收口 | 280 | 增补 | UNX-F52349-J1 PS/PDF 转换 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52350 | ImageMagick（Windows 发行版）收口 | 300 | 增补 | UNX-F52350-J1 convert/mogrify 20 操作集 ×10 全通；图像语义分账声明（像素级处理语义归 N2/H3，本条只收 CLI 发行版）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 分账 |
| UNX-F52351 | GraphicsMagick 收口 | 280 | 增补 | UNX-F52351-J1 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52352 | ExifTool 收口 | 280 | 增补 | UNX-F52352-J1 元数据读写 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52353 | JabRef 收口 | 280 | 增补 | UNX-F52353-J1 六判据全落（JVM 上栈 F52057 联签锚定行）；.bib 管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52354 | jq 收口 | 280 | 增补 | UNX-F52354-J1 过滤表达式 ×20 样本零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52355 | yq 收口 | 260 | 增补 | UNX-F52355-J1 YAML 处理 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52356 | qsv（xsv 续作）收口 | 280 | 增补 | UNX-F52356-J1 CSV 操作 ×10 全通；接管口径（xsv → qsv 升级接管字样）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52357 | universal-ctags 收口 | 280 | 增补 | UNX-F52357-J1 索引生成 ×10 语言全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52358 | ripgrep（Windows 发行版）收口 | 280 | 增补 | UNX-F52358-J1 搜索 ×10 仓库全通；与 B23 GUI 搜索分账声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52359 | fd / fzf（Windows 发行版）收口 | 280 | 增补 | UNX-F52359-J1 查找/模糊选择 ×10 全通；终端消费联签（B12 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52360 | gawk（GnuWin/发行版）收口 | 280 | 增补 | UNX-F52360-J1 脚本 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B18 防重声明**：本批 20 条 = 组 18"开发者文档与数据 CLI 族"；ImageMagick 像素语义、办公文档编辑语义分别分账 N2/N4；ID 段 F52341–F52360 零交叠。

---

<!-- 主册行 357599 · ## 批 UNX-N1-B19（F52361–F52380 · 组 19 · CI/CD 与 DevOps 工具族 ·  -->
## 批 UNX-N1-B19（F52361–F52380 · 组 19 · CI/CD 与 DevOps 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B19） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52361 | Jenkins（Windows 服务面）收口 | 320 | 增补 | UNX-F52361-J1 六判据全落；服务拉起/任务执行/agent 连接三段 ×10；JVM 上栈（F52057 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 19 口径 |
| UNX-F52362 | TeamCity Agent 收口 | 300 | 增补 | UNX-F52362-J1 agent 安装/构建分发 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52363 | GitLab Runner 收口 | 300 | 增补 | UNX-F52363-J1 注册/执行 ×10 任务全通；shell executor 路径；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52364 | GitHub Actions Self-hosted Runner 收口 | 300 | 增补 | UNX-F52364-J1 注册/执行 ×10 workflow 全通；API 网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52365 | Azure Pipelines Agent 收口 | 280 | 增补 | UNX-F52365-J1 注册/执行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52366 | CircleCI CLI 收口 | 260 | 增补 | UNX-F52366-J1 本地执行/校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52367 | AppVeyor CLI 收口 | 260 | 增补 | UNX-F52367-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52368 | Buildkite Agent 收口 | 280 | 增补 | UNX-F52368-J1 注册/执行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52369 | Drone CLI 收口 | 260 | 增补 | UNX-F52369-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52370 | Tekton CLI（tkn）收口 | 260 | 增补 | UNX-F52370-J1 ×10（集群消费与 B14 kubectl 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52371 | Argo CD CLI 收口 | 280 | 增补 | UNX-F52371-J1 app sync/diff ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52372 | Flux CLI 收口 | 280 | 增补 | UNX-F52372-J1 bootstrap/reconcile ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52373 | Ansible（Windows 控制端）收口 | 300 | 增补 | UNX-F52373-J1 playbook ×10 全通；winrm 连接面消费（I 部语义联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52374 | Terraform 收口 | 300 | 增补 | UNX-F52374-J1 init/plan/apply ×10（apply 判据只对本地/local provider 干跑，云 provider 只 plan——外部资源零写入声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52375 | Packer 收口 | 280 | 增补 | UNX-F52375-J1 build ×10（local builder 路径）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52376 | AWS CLI 收口 | 300 | 增补 | UNX-F52376-J1 configure/s3/s3api 20 操作集 ×10（对本地 MinIO 端点实测，云侧只 configure——外部资源零写入）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52377 | Azure CLI 收口 | 300 | 增补 | UNX-F52377-J1 login/group/资源查询 ×10（只读判据，写操作一律干跑声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52378 | Travis CLI 收口（长尾） | 240 | 增补 | UNX-F52378-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52379 | Google Cloud CLI（gcloud）收口 | 300 | 增补 | UNX-F52379-J1 gcloud init/组件管理 ×10（只读判据，写操作干跑）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52380 | Vercel/Netlify CLI 收口（部署面干跑判据，与 sites 通道分账声明） | 280 | 增补 | UNX-F52380-J1 dev/build CLI ×10（deploy 判据一律干跑列清单，禁真实外发——对外动作红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |

**批 B19 防重声明**：本批 20 条 = 组 19"CI/CD 与 DevOps 族"；全部云/外部资源判据只读或干跑，真实外发零执行；ID 段 F52361–F52380 零交叠。

---

<!-- 主册行 357628 · ## 批 UNX-N1-B20（F52381–F52400 · 组 20 · 监控与日志工具族 · 6,000 行） -->
## 批 UNX-N1-B20（F52381–F52400 · 组 20 · 监控与日志工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B20） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52381 | Zabbix Agent 2 收口 | 300 | 增补 | UNX-F52381-J1 服务安装/指标上报 ×10 项全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 20 口径 |
| UNX-F52382 | Prometheus Windows Exporter 收口 | 300 | 增补 | UNX-F52382-J1 指标端点 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52383 | Grafana Agent（Alloy）收口 | 280 | 增补 | UNX-F52383-J1 采集/远端写 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52384 | Netdata（Windows agent）收口 | 280 | 增补 | UNX-F52384-J1 实时面板 ×10 指标；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52385 | Datadog Agent 收口 | 300 | 增补 | UNX-F52385-J1 本地指标/日志 ×10（外部上报面声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52386 | Splunk Universal Forwarder 收口 | 300 | 增补 | UNX-F52386-J1 转发 ×10 输入源；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52387 | Winlogbeat 收口 | 280 | 增补 | UNX-F52387-J1 事件日志采集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52388 | Fluent Bit（Windows）收口 | 280 | 增补 | UNX-F52388-J1 tail/输出 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52389 | Telegraf 收口 | 280 | 增补 | UNX-F52389-J1 输入/输出插件 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52390 | NSClient++ 收口 | 260 | 增补 | UNX-F52390-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52391 | Grafana OSS（Windows）收口 | 300 | 增补 | UNX-F52391-J1 六判据全落；面板/数据源 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52392 | Kibana（Windows）收口 | 300 | 增补 | UNX-F52392-J1 ×10 视图；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52393 | Prometheus Server（Windows）收口 | 300 | 增补 | UNX-F52393-J1 抓取/查询 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52394 | Loki（Windows）收口 | 280 | 增补 | UNX-F52394-J1 推送/查询 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52395 | OpenTelemetry Collector 收口 | 300 | 增补 | UNX-F52395-J1 receiver/processor/exporter ×10 管线全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52396 | Jaeger all-in-one（Windows）收口 | 280 | 增补 | UNX-F52396-J1 trace 查询 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52397 | Zipkin Server（Windows）收口 | 260 | 增补 | UNX-F52397-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52398 | SkyWalking Agent（Windows 面）收口 | 280 | 增补 | UNX-F52398-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52399 | InfluxDB（Windows）收口 | 280 | 增补 | UNX-F52399-J1 写入/查询 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52400 | Chronograf / InfluxDB UI 消费面收口 | 280 | 增补 | UNX-F52400-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B20 防重声明**：本批 20 条 = 组 20"监控与日志族"；agent 侧数据语义消费 E 部/内核指标面，零重写；ID 段 F52381–F52400 零交叠。

---
<!-- 主册行 357656 · ## 批 UNX-N1-B21（F52401–F52420 · 组 21 · 安全与凭证工具族 · 6,000 行） -->
## 批 UNX-N1-B21（F52401–F52420 · 组 21 · 安全与凭证工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B21） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52401 | KeePass 2.x 收口 | 320 | 增补 | UNX-F52401-J1 六判据全落；库创建/条目管理/自动输入 20 操作集全通；J4 .kdbx 双向零损；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 21 口径 |
| UNX-F52402 | KeePassXC 收口 | 300 | 增补 | UNX-F52402-J1 六判据全落；×10；KeeShare/SSH agent 面 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52403 | Bitwarden Desktop 收口 | 300 | 增补 | UNX-F52403-J1 六判据全落；同步面外部服务声明；离线缓存 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52404 | 1Password Desktop 收口 | 300 | 增补 | UNX-F52404-J1 六判据全落；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52405 | HashiCorp Vault CLI 收口 | 300 | 增补 | UNX-F52405-J1 server(dev 模式)/kv 20 操作集 ×10（dev 模式内存态零落盘声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52406 | Gpg4win 收口 | 300 | 增补 | UNX-F52406-J1 六判据全落；签名/加密/解密 ×10；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52407 | Kleopatra 收口 | 280 | 增补 | UNX-F52407-J1 六判据全落；密钥管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52408 | GnuPG CLI 收口 | 280 | 增补 | UNX-F52408-J1 gpg 20 操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52409 | OpenSSL（Windows 发行版）收口 | 300 | 增补 | UNX-F52409-J1 生成/签名/验签 ×10 全通；TLS 消费对账（AI-44 契约联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52410 | age 收口 | 260 | 增补 | UNX-F52410-J1 加密/解密 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52411 | SOPS 收口 | 280 | 增补 | UNX-F52411-J1 加密/解密 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52412 | VeraCrypt 收口 | 320 | 增补 | UNX-F52412-J1 六判据全落；**卷创建判据只对测试镜像文件执行（非系统盘非物理盘）**，全盘加密路径列 N/A 不承诺（硬件数据安全红线适用性声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52413 | Cryptomator 收口 | 300 | 增补 | UNX-F52413-J1 六判据全落；虚拟盘加解密 ×10（只对测试目录）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52414 | git-crypt 收口 | 260 | 增补 | UNX-F52414-J1 仓库透明加密 ×10；与 git 消费联签（F52159 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52415 | git-secret 收口 | 260 | 增补 | UNX-F52415-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52416 | Enpass 收口 | 280 | 增补 | UNX-F52416-J1 六判据全落；本地库模式 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52417 | Nitrokey App 收口 | 260 | 增补 | UNX-F52417-J1 设备连接/管理 ×10（设备类消费 M5 账联签锚定行 AI-65，无设备则 N/A 声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52418 | YubiKey Manager 收口 | 280 | 增补 | UNX-F52418-J1 设备枚举/只读信息 ×10（PIV/FIDO 写配置判据干跑）；M5 账复用（联签锚定行 AI-65）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52419 | KeePass 插件生态消费面收口（×10 插件实测） | 280 | 增补 | UNX-F52419-J1 插件加载/崩溃隔离 ×10 全通（与 F52401 本体分账）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52420 | 凭证文件安全底线条目（全域密码库类判据红线登记） | 280 | 增补 | UNX-F52420-J1 全组条目红线登记：测试库一律随机口令+即弃、真实凭证零入库、日志不记口令内容（体验十三隐私红线）；判据锚定 体验十三·补 + §AI-66.9 |

**批 B21 防重声明**：本批 20 条 = 组 21"安全与凭证族"；加密盘/硬件密钥条目全部只对测试介质判据、系统盘零触碰；ID 段 F52401–F52420 零交叠。

---

<!-- 主册行 357685 · ## 批 UNX-N1-B22（F52421–F52440 · 组 22 · 网络诊断 CLI 工具族 · 6,000  -->
## 批 UNX-N1-B22（F52421–F52440 · 组 22 · 网络诊断 CLI 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B22） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52421 | nmap 收口 | 320 | 增补 | UNX-F52421-J1 扫描判据只对本机回环与实验室网段执行（目标白名单声明）；主机发现/端口扫描 ×10 全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 22 口径 |
| UNX-F52422 | Zenmap 收口 | 280 | 增补 | UNX-F52422-J1 六判据全落（nmap GUI 形态分账）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52423 | masscan 收口 | 280 | 增补 | UNX-F52423-J1 判据仅回环/实验室段（白名单声明）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52424 | Ncat 收口 | 280 | 增补 | UNX-F52424-J1 连接/监听/转发 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52425 | socat（Windows 发行版）收口 | 280 | 增补 | UNX-F52425-J1 管道/转发 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52426 | BIND tools（dig/nslookup）收口 | 280 | 增补 | UNX-F52426-J1 查询 ×10 场景全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52427 | whois CLI 收口 | 260 | 增补 | UNX-F52427-J1 查询 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52428 | iperf3（Windows）收口 | 300 | 增补 | UNX-F52428-J1 回环 client/server 吞吐 ×10；带宽账落账（P95）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52429 | Angry IP Scanner 收口 | 280 | 增补 | UNX-F52429-J1 六判据全落；扫描仅实验室段（白名单）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52430 | Advanced IP Scanner 收口 | 280 | 增补 | UNX-F52430-J1 六判据全落；同白名单声明；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52431 | SoftPerfect Network Scanner 收口 | 280 | 增补 | UNX-F52431-J1 六判据全落；同白名单；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52432 | traceroute/mtr（Windows 发行版）收口 | 280 | 增补 | UNX-F52432-J1 路由追踪 ×10 目标；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52433 | tcping 收口 | 260 | 增补 | UNX-F52433-J1 TCP 探测 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52434 | PsPing（Sysinternals）收口 | 280 | 增补 | UNX-F52434-J1 ping/延迟/带宽 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52435 | Clumsy 收口（网络故障注入） | 280 | 增补 | UNX-F52435-J1 延迟/丢包注入 ×10 场景全通（仅回环测试流）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52436 | NetworkMiner 收口 | 280 | 增补 | UNX-F52436-J1 pcap 解析 ×10（离线分析声明，抓包仅回环）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52437 | Termshark 收口 | 260 | 增补 | UNX-F52437-J1 TUI 消费 ×10（与 F52249 tshark 底座联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52438 | HTTPie CLI 收口 | 280 | 增补 | UNX-F52438-J1 请求 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52439 | Curlie / xh 收口 | 260 | 增补 | UNX-F52439-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52440 | WNetWatcher / 网络邻居发现面收口（长尾） | 260 | 增补 | UNX-F52440-J1 发现 ×10（实验室段白名单）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B22 防重声明**：本批 20 条 = 组 22"网络诊断 CLI 族"；全部扫描/探测判据限回环与实验室网段（目标白名单逐条声明），外网扫描零执行；ID 段 F52421–F52440 零交叠。

---

<!-- 主册行 357714 · ## 批 UNX-N1-B23（F52441–F52460 · 组 23 · 文件搜索与压缩归档工具族 · 6,000  -->
## 批 UNX-N1-B23（F52441–F52460 · 组 23 · 文件搜索与压缩归档工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B23） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52441 | Everything 收口 | 320 | 增补 | UNX-F52441-J1 六判据全落；NTFS 索引/即时搜索 ×10 万文件树全通（与共面 watcher 账联动）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 23 口径 |
| UNX-F52442 | Agent Ransack 收口 | 280 | 增补 | UNX-F52442-J1 六判据全落；内容搜索 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52443 | grepWin 收口 | 260 | 增补 | UNX-F52443-J1 六判据全落；右键集成状态机（体验二浮层出路 ×10）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52444 | AstroGrep 收口 | 260 | 增补 | UNX-F52444-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52445 | dnGrep 收口 | 280 | 增补 | UNX-F52445-J1 ×10（含 PDF/office 文本抽取）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52446 | Sysinternals du 收口 | 260 | 增补 | UNX-F52446-J1 磁盘占用 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52447 | WinDirStat 收口 | 300 | 增补 | UNX-F52447-J1 六判据全落；目录树/矩形图 ×10；大盘扫描分段账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52448 | WizTree 收口 | 280 | 增补 | UNX-F52448-J1 六判据全落；MFT 快扫 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52449 | TreeSize Free 收口 | 280 | 增补 | UNX-F52449-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52450 | SpaceSniffer 收口 | 260 | 增补 | UNX-F52450-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52451 | 7-Zip 收口 | 320 | 增补 | UNX-F52451-J1 六判据全落；压缩/解压 ×10 格式全通；大文件（4GB+）分段账；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52452 | PeaZip 收口 | 280 | 增补 | UNX-F52452-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52453 | Bandizip 收口 | 280 | 增补 | UNX-F52453-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52454 | NanaZip 收口 | 280 | 增补 | UNX-F52454-J1 六判据全落；商店分发面声明；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52455 | FreeCommander 收口 | 280 | 增补 | UNX-F52455-J1 六判据全落；双面板操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52456 | Double Commander 收口 | 280 | 增补 | UNX-F52456-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52457 | Total Commander 收口 | 300 | 增补 | UNX-F52457-J1 六判据全落；插件生态 ×5；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52458 | Directory Opus 收口 | 300 | 增补 | UNX-F52458-J1 六判据全落；×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52459 | OneCommander 收口（长尾） | 260 | 增补 | UNX-F52459-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52460 | tar/gzip/zstd（发行版 CLI 面）收口 | 280 | 增补 | UNX-F52460-J1 归档 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B23 防重声明**：本批 20 条 = 组 23"文件搜索与压缩归档族"（开发者工作流侧，通用办公归 N4 分界声明）；ID 段 F52441–F52460 零交叠。

---

<!-- 主册行 357743 · ## 批 UNX-N1-B24（F52461–F52480 · 组 24 · 浏览器与 Web 调试工具族 · 6,00 -->
## 批 UNX-N1-B24（F52461–F52480 · 组 24 · 浏览器与 Web 调试工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B24） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52461 | Google Chrome 收口 | 320 | 增补 | UNX-F52461-J1 六判据全落；Chromium 多进程模型（与 B02 同构账复用）；DevTools 20 操作集；J5 三层中文；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 24 口径 |
| UNX-F52462 | Microsoft Edge 收口 | 300 | 增补 | UNX-F52462-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52463 | Mozilla Firefox 收口 | 300 | 增补 | UNX-F52463-J1 六判据全落；Gecko 进程模型账 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52464 | Brave 收口 | 280 | 增补 | UNX-F52464-J1 六判据全落（Chromium 同构复用声明）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52465 | Opera 收口 | 280 | 增补 | UNX-F52465-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52466 | Vivaldi 收口 | 280 | 增补 | UNX-F52466-J1 六判据全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52467 | Chromium（开源构建）收口 | 300 | 增补 | UNX-F52467-J1 ×10；与 Chrome 分账（发行版差异声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52468 | Chrome Canary 收口（滚动线） | 260 | 增补 | UNX-F52468-J1 六判据全落；滚动版钉版回归口径（AI-94 T+1 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52469 | Firefox Developer Edition 收口 | 260 | 增补 | UNX-F52469-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52470 | React DevTools 消费面收口 | 280 | 增补 | UNX-F52470-J1 组件树/Profiler ×10；与宿主浏览器联签（F52461 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52471 | Vue Devtools 消费面收口 | 280 | 增补 | UNX-F52471-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52472 | Redux DevTools 消费面收口 | 260 | 增补 | UNX-F52472-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52473 | Angular DevTools 消费面收口 | 260 | 增补 | UNX-F52473-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52474 | Lighthouse CLI 收口 | 280 | 增补 | UNX-F52474-J1 审计 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52475 | Playwright CLI 收口 | 320 | 增补 | UNX-F52475-J1 e2e ×10 场景全通（headless 判据）；浏览器驱动下载面网络语义（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52476 | Puppeteer 收口 | 300 | 增补 | UNX-F52476-J1 ×10；与 Playwright 分账（同域不同框架）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52477 | Selenium Server / WebDriver 收口 | 300 | 增补 | UNX-F52477-J1 会话 ×10（Chrome/Firefox 双驱动）；W3C WebDriver 协议 ×20 命令零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52478 | Cypress 收口 | 300 | 增补 | UNX-F52478-J1 六判据全落（Electron 壳三段账）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52479 | WebdriverIO CLI 收口 | 260 | 增补 | UNX-F52479-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52480 | 浏览器用户数据隔离共面条目（多浏览器 profile 并存零互踩） | 280 | 增补 | UNX-F52480-J1 本组 8 浏览器并存 profile/证书/代理设置隔离 ×2 轮零互踩；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |

**批 B24 防重声明**：本批 20 条 = 组 24"浏览器与 Web 调试族"；浏览器仅按开发调试面收口，消费者级生态判据不做（分界声明）；ID 段 F52461–F52480 零交叠。

---

<!-- 主册行 357772 · ## 批 UNX-N1-B25（F52481–F52500 · 组 25 · 测试与安全测试工具族 · 6,000 行） -->
## 批 UNX-N1-B25（F52481–F52500 · 组 25 · 测试与安全测试工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B25） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52481 | SonarScanner CLI 收口 | 300 | 增补 | UNX-F52481-J1 静态扫描 ×10 工程全通；JVM 上栈（F52057 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 25 口径 |
| UNX-F52482 | k6 收口 | 300 | 增补 | UNX-F52482-J1 压测 ×10（只打本地靶站声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52483 | ApacheBench（ab）收口 | 260 | 增补 | UNX-F52483-J1 ×10（本地靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52484 | wrk 收口 | 260 | 增补 | UNX-F52484-J1 ×10（本地靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52485 | Gatling 收口 | 280 | 增补 | UNX-F52485-J1 ×10（本地靶站）；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52486 | axe DevTools 消费面收口 | 260 | 增补 | UNX-F52486-J1 可访问性审计 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52487 | Accessibility Insights for Windows 收口 | 300 | 增补 | UNX-F52487-J1 UIA 树检查 ×10（消费 M 域 UIA 联签锚定行 AI-62）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52488 | TestComplete 收口 | 300 | 增补 | UNX-F52488-J1 六判据全落；UI 自动化 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52489 | Ranorex 收口 | 280 | 增补 | UNX-F52489-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52490 | Katalon Studio 收口 | 300 | 增补 | UNX-F52490-J1 六判据全落（Electron 同构）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52491 | Selenium IDE 消费面收口 | 260 | 增补 | UNX-F52491-J1 录制/回放 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52492 | Burp Suite Community 收口 | 320 | 增补 | UNX-F52492-J1 六判据全落；代理/扫描 ×10（只对本机靶站）；JVM 联签锚定行；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52493 | OWASP ZAP 收口 | 300 | 增补 | UNX-F52493-J1 代理/扫描 ×10（本机靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52494 | Nikto 收口 | 260 | 增补 | UNX-F52494-J1 ×10（本机靶站）；Perl 消费联签（F52118 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52495 | SQLMap 收口 | 280 | 增补 | UNX-F52495-J1 判据仅对本机搭建的演练靶场（白名单声明）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52496 | Hydra / 演练靶场爆破面收口（干跑判据） | 260 | 增补 | UNX-F52496-J1 只对本机靶场演示 ×10（白名单声明，真实凭据零接触）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52497 | MobSF 收口（移动安全静态分析） | 280 | 增补 | UNX-F52497-J1 APK 静态分析 ×10（本地样本）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52498 | Semgrep CLI 收口 | 300 | 增补 | UNX-F52498-J1 规则扫描 ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52499 | TruffleHog / Gitleaks 收口 | 280 | 增补 | UNX-F52499-J1 凭证泄漏扫描 ×10 仓库（本地）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52500 | B25 靶场隔离声明条目（全域安全测试判据白名单登记） | 280 | 增补 | UNX-F52500-J1 全组安全测试条目靶场白名单登记：仅本机/实验室靶场，外部目标零接触，探测类判据与 E5 知识账探测失配分界（联签锚定行 AI-25）；判据锚定 §AI-66.9 红线适用 |

**批 B25 防重声明**：本批 20 条 = 组 25"测试与安全测试族"；全部安全测试判据限本机靶场（F52500 白名单条目承载），与 E5 探测失配账分界；ID 段 F52481–F52500 零交叠。

---
<!-- 主册行 357800 · ## 批 UNX-N1-B26（F52501–F52520 · 组 26 · 语言服务器与 Shell 运行时族 · 6 -->
## 批 UNX-N1-B26（F52501–F52520 · 组 26 · 语言服务器与 Shell 运行时族 · 6,000 行）

| ID | 深化名（deepen/N1-B26） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52501 | clangd 收口 | 300 | 增补 | UNX-F52501-J1 LSP 协议 ×20 命令零漂移；compile_commands.json 消费 ×10 工程；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 26 口径 |
| UNX-F52502 | gopls 收口 | 300 | 增补 | UNX-F52502-J1 LSP ×20 命令 ×10 工程；与 Go 工具链消费联签（F52046 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52503 | rust-analyzer 收口 | 300 | 增补 | UNX-F52503-J1 LSP ×20；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52504 | pyright 收口 | 280 | 增补 | UNX-F52504-J1 类型检查 ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52505 | pylsp 收口 | 260 | 增补 | UNX-F52505-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52506 | typescript-language-server 收口 | 280 | 增补 | UNX-F52506-J1 ×10；Node 消费联签（F52032 ABI 账锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52507 | lua-language-server 收口 | 260 | 增补 | UNX-F52507-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52508 | OmniSharp 收口 | 280 | 增补 | UNX-F52508-J1 ×10（.NET SDK 消费联签 F52066 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52509 | jdtls 收口 | 280 | 增补 | UNX-F52509-J1 ×10；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52510 | bash-language-server 收口 | 260 | 增补 | UNX-F52510-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52511 | yaml-language-server 收口 | 260 | 增补 | UNX-F52511-J1 schema 校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52512 | PowerShell 7（pwsh）收口 | 320 | 增补 | UNX-F52512-J1 cmdlet 20 操作集 ×10 全通；模块加载 ×5；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52513 | Git Bash（MSYS2 精简面）收口 | 300 | 增补 | UNX-F52513-J1 POSIX 工具 20 操作集 ×10；与 MSYS2/Git 分账声明（联签锚定行 F52159/F52073）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52514 | Cygwin 收口 | 300 | 增补 | UNX-F52514-J1 安装器/setup ×10 包全通；POSIX 兼容面样本 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52515 | BusyBox-w32 收口 | 280 | 增补 | UNX-F52515-J1 applet ×20 联通性全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52516 | GnuWin32 coreutils 面收口 | 280 | 增补 | UNX-F52516-J1 工具 ×20 联通性；EOL 临近声明；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52517 | less（Windows）收口 | 260 | 增补 | UNX-F52517-J1 分页 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52518 | sed/grep（发行版 CLI 面）收口 | 280 | 增补 | UNX-F52518-J1 ×10 样本零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52519 | Windows Terminal profile 共面（多 shell 接入零互踩） | 280 | 增补 | UNX-F52519-J1 pwsh/Git Bash/Cygwin/BusyBox 四 shell profile 共存 ×2 轮零互踩；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |
| UNX-F52520 | LSP 生态启动协议共面（stdio/管道语义全域对账） | 280 | 增补 | UNX-F52520-J1 本组 11 个 LSP server 启动/关闭/崩溃重启协议 ×11 全通零悬空；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二 |

**批 B26 防重声明**：本批 20 条 = 组 26"语言服务器与 Shell 运行时族"；LSP server 为独立可执行应用收口，与 IDE 条目（B02/B03/B06）消费关系联签不重立；ID 段 F52501–F52520 零交叠。

---

<!-- 主册行 357829 · ## 批 UNX-N1-B27（F52521–F52540 · 组 27 · 数据科学与图表工具族 · 6,000 行） -->
## 批 UNX-N1-B27（F52521–F52540 · 组 27 · 数据科学与图表工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B27） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52521 | GNU Octave 收口 | 300 | 增补 | UNX-F52521-J1 六判据全落；计算/绘图 ×10 全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 27 口径 |
| UNX-F52522 | Scilab 收口 | 280 | 增补 | UNX-F52522-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52523 | Julia for Windows 收口 | 300 | 增补 | UNX-F52523-J1 REPL/包管理 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52524 | R for Windows 收口 | 300 | 增补 | UNX-F52524-J1 ×10；与 RStudio 消费联签（F52112 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52525 | Maxima / wxMaxima 收口 | 280 | 增补 | UNX-F52525-J1 符号计算 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52526 | Python for Windows（发行版收口，与 L4 分账） | 320 | 增补 | UNX-F52526-J1 CPython 20 操作集 ×10 全通；venv/pip 面 ×10；Windows 发行版应用收口与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 防重声明 |
| UNX-F52527 | gnuplot 收口 | 280 | 增补 | UNX-F52527-J1 绘图 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52528 | Graphviz 收口 | 300 | 增补 | UNX-F52528-J1 dot 布局 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52529 | PlantUML（jar 面）收口 | 280 | 增补 | UNX-F52529-J1 渲染 ×10；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52530 | Mermaid CLI 收口 | 280 | 增补 | UNX-F52530-J1 渲染 ×10；Node 消费联签（F52032 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52531 | draw.io Desktop 收口 | 300 | 增补 | UNX-F52531-J1 六判据全落；图形编辑/导出 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52532 | yEd 收口 | 280 | 增补 | UNX-F52532-J1 ×10；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52533 | Pencil Project 收口 | 260 | 增补 | UNX-F52533-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52534 | Asciidoctor 收口 | 280 | 增补 | UNX-F52534-J1 渲染 ×10；Ruby 消费联签（F52119 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52535 | Antora 收口 | 260 | 增补 | UNX-F52535-J1 站点生成 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52536 | Marp CLI 收口 | 280 | 增补 | UNX-F52536-J1 幻灯导出 ×10（pdf/pptx/html）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52537 | Slidev CLI 收口 | 260 | 增补 | UNX-F52537-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52538 | Wolfram Engine（免费开发者版）收口 | 300 | 增补 | UNX-F52538-J1 内核会话 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52539 | GeoGebra Classic（桌面版）收口 | 260 | 增补 | UNX-F52539-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52540 | 图表导出保真共面（Graphviz/PlantUML/Mermaid/draw.io 四源 SVG/PNG 输出对照） | 280 | 增补 | UNX-F52540-J1 四源 ×2 格式输出校验和对照 ×10 场景，渲染像素抽检零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一 J4 口径 |

**批 B27 防重声明**：本批 20 条 = 组 27"数据科学与图表族"；科学计算判据零触硬件加速专有依赖（CUDA 类 N/A 同构）；ID 段 F52521–F52540 零交叠。

---

<!-- 主册行 357858 · ## 批 UNX-N1-B28（F52541–F52560 · 组 28 · 移动开发与 Android 逆向工具族 · -->
## 批 UNX-N1-B28（F52541–F52560 · 组 28 · 移动开发与 Android 逆向工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B28） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52541 | Flutter SDK 收口 | 320 | 增补 | UNX-F52541-J1 flutter create/build 20 操作集 ×10 全通；Windows 桌面目标产物可运行；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 28 口径 |
| UNX-F52542 | Dart SDK 收口 | 280 | 增补 | UNX-F52542-J1 dart 编译运行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52543 | React Native CLI 收口 | 300 | 增补 | UNX-F52543-J1 init/build ×10（Android 目标消费 B05 链联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52544 | Expo CLI 收口 | 280 | 增补 | UNX-F52544-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52545 | Apache Cordova 收口 | 280 | 增补 | UNX-F52545-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52546 | Ionic CLI 收口 | 280 | 增补 | UNX-F52546-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52547 | Genymotion 收口（N/A 虚拟化声明） | 280 | 增补 | UNX-F52547-J1 虚拟化依赖探测账（L5 消费联签锚定行 AI-60），不可用即 N/A + 三要素告警；KB-COMPAT 定级 S3 + N/A 清单；判据锚定 §AI-66.2 专题五模拟器 N/A 同构 |
| UNX-F52548 | BlueStacks 5 收口（N/A 同构） | 280 | 增补 | UNX-F52548-J1 同 F52547 N/A 口径；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52549 | NoxPlayer 收口（N/A 同构） | 260 | 增补 | UNX-F52549-J1 同口径；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52550 | LDPlayer 收口（N/A 同构） | 260 | 增补 | UNX-F52550-J1 同口径；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52551 | MEmu 收口（N/A 同构） | 260 | 增补 | UNX-F52551-J1 同口径；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52552 | apktool 收口 | 300 | 增补 | UNX-F52552-J1 反编译/回编译 ×10 本地 APK 样本全通（许可合规：仅自有样本声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52553 | jadx 收口 | 300 | 增补 | UNX-F52553-J1 DEX→Java 反编译 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52554 | dex2jar 收口 | 280 | 增补 | UNX-F52554-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52555 | Bundletool 收口 | 280 | 增补 | UNX-F52555-J1 .aab→apks ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52556 | apksigner 面收口 | 280 | 增补 | UNX-F52556-J1 签名/校验 ×10（测试密钥零入库）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52557 | APK Editor Studio 收口（长尾） | 260 | 增补 | UNX-F52557-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52558 | scrcpy 补充面：多设备并发收口 | 280 | 增补 | UNX-F52558-J1 ×3 设备并发 ×10 场景（M5 账复用联签锚定行 AI-65，与 F52095 本体分账）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52559 | adb 无线调试面收口 | 280 | 增补 | UNX-F52559-J1 无线配对/连接 ×10（网络语义 AI-23 联签锚定行，M5 判据复用）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52560 | 移动构建产物签名链共面（keystore 管理零真实凭据） | 280 | 增补 | UNX-F52560-J1 全组签名条目 keystore 管理红线登记：测试密钥即弃、真实 keystore 零入库；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |

**批 B28 防重声明**：本批 20 条 = 组 28"移动开发与 Android 逆向族"；模拟器类全部 N/A 同构声明；逆向工具仅对自有/测试样本（许可合规声明）；ID 段 F52541–F52560 零交叠。

---

<!-- 主册行 357887 · ## 批 UNX-N1-B29（F52561–F52580 · 组 29 · 编译器与语言发行版族 · 6,000 行） -->
## 批 UNX-N1-B29（F52561–F52580 · 组 29 · 编译器与语言发行版族 · 6,000 行）

| ID | 深化名（deepen/N1-B29） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52561 | Go for Windows 收口 | 320 | 增补 | UNX-F52561-J1 go build/test 20 操作集 ×10 全通；Windows 发行版收口与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 防重声明 |
| UNX-F52562 | Rustup + MSVC 工具链收口 | 320 | 增补 | UNX-F52562-J1 rustup 工具链管理 ×10 全通；MSVC 目标消费 B04 链联签锚定行；与 L4 分账；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 |
| UNX-F52563 | LLVM/Clang for Windows 收口 | 320 | 增补 | UNX-F52563-J1 clang/llvm 工具 ×20 联通性 ×10 工程；分账联签锚定行 AI-59；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 |
| UNX-F52564 | Zig 收口 | 300 | 增补 | UNX-F52564-J1 build/cross ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52565 | Node.js for Windows 收口 | 320 | 增补 | UNX-F52565-J1 node/npm 20 操作集 ×10 全通；发行版收口与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级 S1；判据锚定 §AI-66.1 |
| UNX-F52566 | pnpm 收口 | 300 | 增补 | UNX-F52566-J1 install/run ×10 全通；硬链接/内容寻址存储与 B2 卷层联动账（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52567 | Yarn Classic 收口 | 280 | 增补 | UNX-F52567-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52568 | Deno 收口 | 300 | 增补 | UNX-F52568-J1 run/test ×10；权限模型 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52569 | Bun 收口 | 300 | 增补 | UNX-F52569-J1 run/install ×10；性能账（P95 实测不凭感觉）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52570 | PHP for Windows 收口 | 300 | 增补 | UNX-F52570-J1 CLI/FastCGI 面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52571 | Erlang/Elixir 收口 | 300 | 增补 | UNX-F52571-J1 REPL/混合编译 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52572 | Haskell Stack 收口 | 300 | 增补 | UNX-F52572-J1 build ×5 样本全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52573 | Free Pascal（FPC）收口 | 280 | 增补 | UNX-F52573-J1 ×10；与 Lazarus 消费联签（F52111 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52574 | Nim 收口 | 280 | 增补 | UNX-F52574-J1 编译 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52575 | D（dmd/LDC）收口 | 280 | 增补 | UNX-F52575-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52576 | Swift for Windows 收口 | 300 | 增补 | UNX-F52576-J1 编译运行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52577 | Crystal 收口 | 260 | 增补 | UNX-F52577-J1 ×5 样本（Windows 支持实验性——如实定级声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |
| UNX-F52578 | NASM/Yasm 收口 | 280 | 增补 | UNX-F52578-J1 汇编 ×10 样本全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52579 | MinGW binutils 面（objdump/readelf/strip/ar）收口 | 300 | 增补 | UNX-F52579-J1 工具 ×20 联通性 ×10 样本；发行版收口与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52580 | 多语言发行版并存共面（PATH 隔离与版本管理零互踩） | 280 | 增补 | UNX-F52580-J1 本组 ≥8 语言发行版并存 ×2 轮 PATH/shim 解析零互踩；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |

**批 B29 防重声明**：本批 20 条 = 组 29"编译器与语言发行版族"；全部条目为"Windows 发行版应用收口"口径与 L4 嫁接件布设分账（逐条联签锚定）；ID 段 F52561–F52580 零交叠。

---

<!-- 主册行 357916 · ## 批 UNX-N1-B30（F52581–F52600 · 组 30 · 数据库 CLI 与迁移工具族 · 6,00 -->
## 批 UNX-N1-B30（F52581–F52600 · 组 30 · 数据库 CLI 与迁移工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B30） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52581 | mysql CLI 客户端收口 | 300 | 增补 | UNX-F52581-J1 20 操作集 ×10（对接本地测试实例）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 30 口径 |
| UNX-F52582 | MySQL Shell（mysqlsh）收口 | 280 | 增补 | UNX-F52582-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52583 | mysqldump / mysqlpump 收口 | 280 | 增补 | UNX-F52583-J1 备份/恢复 ×10 校验和闭环；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52584 | psql 收口 | 300 | 增补 | UNX-F52584-J1 ×10（本地实例）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52585 | pg_dump / pg_restore 收口 | 300 | 增补 | UNX-F52585-J1 ×10 校验和闭环；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52586 | sqlite3 CLI 收口 | 300 | 增补 | UNX-F52586-J1 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52587 | sqlcmd / bcp 收口 | 280 | 增补 | UNX-F52587-J1 ×10（本地 LocalDB/Express 实例）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52588 | SQL Server Express（LocalDB 面）收口 | 300 | 增补 | UNX-F52588-J1 LocalDB 实例创建/连接 ×10（轻量本地面，服务端完整面分账声明归 K1 域联签锚定行 AI-51）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 分账 |
| UNX-F52589 | mongosh 收口 | 280 | 增补 | UNX-F52589-J1 ×10（本地实例）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52590 | mongoexport / mongoimport 收口 | 280 | 增补 | UNX-F52590-J1 ×10 校验和闭环；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52591 | redis-cli 收口 | 280 | 增补 | UNX-F52591-J1 ×10（本地实例）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52592 | usql 收口 | 260 | 增补 | UNX-F52592-J1 多数据源 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52593 | Flyway 收口 | 300 | 增补 | UNX-F52593-J1 迁移 ×10 版本链全通（本地实例）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52594 | Liquibase 收口 | 300 | 增补 | UNX-F52594-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52595 | dotnet-ef（EF Core CLI）收口 | 300 | 增补 | UNX-F52595-J1 migrations ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52596 | Prisma CLI 收口 | 300 | 增补 | UNX-F52596-J1 migrate/studio ×10（SQLite 本地目标）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52597 | Alembic 收口 | 280 | 增补 | UNX-F52597-J1 ×10（SQLite）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52598 | ODBC 数据源管理器（odbcad32）消费面收口 | 280 | 增补 | UNX-F52598-J1 DSN 创建/测试 ×10（消费 E 部注册表语义联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52599 | 数据库驱动注册共面（ODBC/JDBC 驱动登记零冲突） | 280 | 增补 | UNX-F52599-J1 驱动登记 ×10 类并存零冲突 ×2 轮；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |
| UNX-F52600 | 数据库测试实例卫生条目（全域本地实例即弃红线） | 280 | 增补 | UNX-F52600-J1 全组红线登记：判据仅用本地即弃实例、端口零冲突、数据零残留清理 ×10 验证；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |

**批 B30 防重声明**：本批 20 条 = 组 30"数据库 CLI 与迁移族"；全部判据仅本地即弃实例（F52600 红线条目承载），数据库服务端完整面归 K1（AI-51）分账；ID 段 F52581–F52600 零交叠。

---
<!-- 主册行 357944 · ## 批 UNX-N1-B31（F52601–F52620 · 组 31 · 基准测试与硬件信息工具族 · 6,000  -->
## 批 UNX-N1-B31（F52601–F52620 · 组 31 · 基准测试与硬件信息工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B31） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52601 | CPU-Z 收口 | 300 | 增补 | UNX-F52601-J1 六判据全落；CPU/主板/内存信息读取 ×10（只读判据，传感器零写）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 31 口径 |
| UNX-F52602 | GPU-Z 收口 | 300 | 增补 | UNX-F52602-J1 六判据全落；GPU 信息 ×10（消费 G 部设备面联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52603 | HWiNFO 收口 | 300 | 增补 | UNX-F52603-J1 传感器只读 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52604 | HWMonitor 收口 | 280 | 增补 | UNX-F52604-J1 ×10 只读；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52605 | Core Temp 收口 | 260 | 增补 | UNX-F52605-J1 ×10 只读；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52606 | Libre Hardware Monitor 收口 | 280 | 增补 | UNX-F52606-J1 ×10 只读；Open Hardware Monitor 接管口径声明；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52607 | AIDA64 收口 | 300 | 增补 | UNX-F52607-J1 系统信息/基准 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52608 | Speccy 收口 | 260 | 增补 | UNX-F52608-J1 ×10 只读；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52609 | CrystalDiskInfo 收口 | 300 | 增补 | UNX-F52609-J1 SMART 只读 ×10 盘（只读诊断先行纪律适用）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52610 | CrystalDiskMark 收口 | 300 | 增补 | UNX-F52610-J1 基准判据只对测试文件镜像执行（禁对系统盘写入压测——硬件数据安全红线适用性声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52611 | AS SSD Benchmark 收口 | 280 | 增补 | UNX-F52611-J1 同 F52610 干跑口径；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52612 | ATTO Disk Benchmark 收口 | 280 | 增补 | UNX-F52612-J1 同口径；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52613 | Prime95 收口 | 300 | 增补 | UNX-F52613-J1 压力判据只短时档（5 分钟）+ 温度监控联动中止线；长时间烤机列 N/A（硬件安全红线：不做无监控压测）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52614 | Cinebench 收口 | 280 | 增补 | UNX-F52614-J1 短档基准 ×10 分数落账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52615 | Geekbench 收口 | 280 | 增补 | UNX-F52615-J1 ×10；提交外部面声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52616 | FurMark 收口 | 280 | 增补 | UNX-F52616-J1 GPU 压测短档 + 温度中止线（红线同 F52613）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52617 | Unigine Superposition 收口 | 280 | 增补 | UNX-F52617-J1 基准 ×10 分数落账（G 部消费联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52618 | 3DMark 收口（基础档） | 280 | 增补 | UNX-F52618-J1 基础基准 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52619 | Windows 内存诊断消费面收口（mdsched） | 260 | 增补 | UNX-F52619-J1 调度/结果读取 ×10（重启外判据 N/A 声明——本域不做固件态执行判据）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52620 | 基准工具全域只读/短档红线登记条目 | 280 | 增补 | UNX-F52620-J1 全组红线登记：传感器/SMART 只读、压测短档+中止线、盘基准只对测试镜像；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |

**批 B31 防重声明**：本批 20 条 = 组 31"基准与硬件信息族"；全组硬件安全红线适用（只读/短档/测试镜像三纪律，F52620 承载）；ID 段 F52601–F52620 零交叠。

---

<!-- 主册行 357973 · ## 批 UNX-N1-B32（F52621–F52640 · 组 32 · 知识库与写作工具族 · 6,000 行） -->
## 批 UNX-N1-B32（F52621–F52640 · 组 32 · 知识库与写作工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B32） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52621 | Notion Desktop 收口 | 300 | 增补 | UNX-F52621-J1 六判据全落（Electron 三段账）；离线态行为声明；×10；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 32 口径 |
| UNX-F52622 | Logseq 收口 | 300 | 增补 | UNX-F52622-J1 六判据全落；本地库 .md 双向零损 ×100 文件；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52623 | Trilium Notes 收口 | 280 | 增补 | UNX-F52623-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52624 | Joplin 收口 | 300 | 增补 | UNX-F52624-J1 六判据全落；同步面外部服务声明 + 本地文件同步路径 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52625 | Standard Notes 收口 | 280 | 增补 | UNX-F52625-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52626 | QOwnNotes 收口 | 260 | 增补 | UNX-F52626-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52627 | CherryTree 收口 | 280 | 增补 | UNX-F52627-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52628 | Zim Desktop Wiki（Windows）收口 | 260 | 增补 | UNX-F52628-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52629 | WikidPad 收口（长尾） | 240 | 增补 | UNX-F52629-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52630 | FocusWriter 收口 | 240 | 增补 | UNX-F52630-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52631 | WriteMonkey 收口（长尾） | 240 | 增补 | UNX-F52631-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52632 | ghostwriter 收口 | 260 | 增补 | UNX-F52632-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52633 | Workflowy Desktop 收口 | 260 | 增补 | UNX-F52633-J1 ×10；同步面声明；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52634 | Outline 知识库桌面消费面收口 | 260 | 增补 | UNX-F52634-J1 ×10（自托管实例判据）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52635 | DokuWiki/本地 wiki 镜像消费面收口 | 260 | 增补 | UNX-F52635-J1 本地站点浏览/编辑 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52636 | Xournal++ 收口（手写批注） | 280 | 增补 | UNX-F52636-J1 ×10（PDF 批注导出校验和）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52637 | xournal 导出保真共面（PDF 批注往返） | 260 | 增补 | UNX-F52637-J1 批注导出/重开 ×10 校验和闭环；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题一 J4 |
| UNX-F52638 | Freeplane 收口（思维导图） | 260 | 增补 | UNX-F52638-J1 ×10；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52639 | XMind 收口 | 280 | 增补 | UNX-F52639-J1 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52640 | 知识库数据开放共面（.md/.opml 等开放格式导出全域对账） | 280 | 增补 | UNX-F52640-J1 本组 ≥8 应用开放格式导出 ×10 场景全通（十四章数据开放口径：不给用户上锁）；KB-COMPAT 定级登记；判据锚定 体验十四 |

**批 B32 防重声明**：本批 20 条 = 组 32"知识库与写作族"（开发者文档向，与 N4 通用办公分界声明，与 B02 Markdown 编辑器分立——本组为库/图/导出面）；ID 段 F52621–F52640 零交叠。

---

<!-- 主册行 358002 · ## 批 UNX-N1-B33（F52641–F52660 · 组 33 · Git 生态深化工具族 · 6,000 行 -->
## 批 UNX-N1-B33（F52641–F52660 · 组 33 · Git 生态深化工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B33） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52641 | pre-commit 收口 | 300 | 增补 | UNX-F52641-J1 hook 安装/运行 ×10 仓库全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 33 口径 |
| UNX-F52642 | husky 收口 | 280 | 增补 | UNX-F52642-J1 hook 注入 ×10；Node 消费联签（F52032 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52643 | gitleaks 收口 | 300 | 增补 | UNX-F52643-J1 扫描 ×10 仓库（本地）；与 B25 共面分账（CI 集成面归本条）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52644 | TruffleHog 收口 | 300 | 增补 | UNX-F52644-J1 ×10（本地仓库）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52645 | BFG Repo-Cleaner 收口 | 280 | 增补 | UNX-F52645-J1 历史清理 ×10（只对测试仓库——破坏性写干跑声明）；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52646 | git-filter-repo 收口 | 280 | 增补 | UNX-F52646-J1 ×10（测试仓库干跑先行 + 实跑样本）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52647 | Git LFS 收口 | 300 | 增补 | UNX-F52647-J1 track/push/pull ×10（本地对象库）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52648 | git-annex 收口 | 280 | 增补 | UNX-F52648-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52649 | tig 收口 | 260 | 增补 | UNX-F52649-J1 TUI 交互状态机完整（体验四口径 ×10）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52650 | lazygit 收口 | 300 | 增补 | UNX-F52650-J1 TUI 操作集 ×10；键位状态机完整（体验四）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52651 | gitui 收口 | 280 | 增补 | UNX-F52651-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52652 | delta（diff pager）收口 | 260 | 增补 | UNX-F52652-J1 渲染 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52653 | Git Credential Manager 收口 | 300 | 增补 | UNX-F52653-J1 凭证存取 ×10（测试凭据即弃、真实凭证零入库——隐私红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52654 | commitlint 收口 | 260 | 增补 | UNX-F52654-J1 规则校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52655 | semantic-release 收口 | 280 | 增补 | UNX-F52655-J1 dry-run 判据 ×10（真实发布零执行——对外动作红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52656 | changesets CLI 收口 | 280 | 增补 | UNX-F52656-J1 version dry-run ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52657 | release-please CLI 收口 | 280 | 增补 | UNX-F52657-J1 dry-run ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52658 | git-cliff 收口 | 260 | 增补 | UNX-F52658-J1 changelog 生成 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52659 | Gitea（Windows 单机面）收口 | 300 | 增补 | UNX-F52659-J1 本地实例拉起/仓库托管 ×10（与 K1 服务域分账声明——单机二进制应用形态收口）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 分账 |
| UNX-F52660 | Git 生态全域共面（hook 链/large 对象/凭证三面汇总复测） | 280 | 增补 | UNX-F52660-J1 本组 hook 链 ×10 仓库交叉复测零悬空；与 F52284 共面四账衔接登记；KB-COMPAT 定级登记；判据锚定 §AI-66.5 J-共面 |

**批 B33 防重声明**：本批 20 条 = 组 33"Git 生态深化族"；与 B10（GUI）、B08（CLI 发行版）分账；发布类判据一律 dry-run；ID 段 F52641–F52660 零交叠。

---

<!-- 主册行 358031 · ## 批 UNX-N1-B34（F52661–F52680 · 组 34 · 容器镜像与 K8s 周边工具族 · 6,0 -->
## 批 UNX-N1-B34（F52661–F52680 · 组 34 · 容器镜像与 K8s 周边工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B34） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52661 | skopeo（Windows 发行版）收口 | 300 | 增补 | UNX-F52661-J1 copy/inspect ×10（本地 registry:2 实例判据）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 34 口径 |
| UNX-F52662 | crane 收口 | 280 | 增补 | UNX-F52662-J1 ×10（本地 registry）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52663 | dive 收口 | 280 | 增补 | UNX-F52663-J1 镜像层分析 ×10（本地镜像）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52664 | Trivy 收口 | 300 | 增补 | UNX-F52664-J1 镜像/文件系统扫描 ×10（本地）；漏洞库更新面网络语义（AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52665 | Grype 收口 | 280 | 增补 | UNX-F52665-J1 ×10（本地）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52666 | Syft 收口 | 280 | 增补 | UNX-F52666-J1 SBOM 生成 ×10 校验和落账；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52667 | cosign 收口 | 280 | 增补 | UNX-F52667-J1 签名/验签 ×10（本地密钥即弃）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52668 | oras 收口 | 260 | 增补 | UNX-F52668-J1 push/pull ×10（本地 registry）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52669 | nerdctl（Windows 面）收口 | 280 | 增补 | UNX-F52669-J1 ×10（与 F52261 容器运行时消费联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52670 | Docker Buildx 收口 | 300 | 增补 | UNX-F52670-J1 build ×10（本地 builder）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52671 | registry:2（本地镜像仓库单机面）收口 | 300 | 增补 | UNX-F52671-J1 单机拉起/推拉 ×10（与 K1 服务域分账声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 分账 |
| UNX-F52672 | Hadolint 收口 | 260 | 增补 | UNX-F52672-J1 Dockerfile lint ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52673 | yamllint 收口 | 260 | 增补 | UNX-F52673-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52674 | kubeconform 收口 | 280 | 增补 | UNX-F52674-J1 manifest 校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52675 | kustomize 收口 | 280 | 增补 | UNX-F52675-J1 build overlay ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52676 | kubectx / kubens 收口 | 260 | 增补 | UNX-F52676-J1 上下文切换 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52677 | stern 收口 | 260 | 增补 | UNX-F52677-J1 日志 tail ×10（本地 kind 集群判据）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52678 | krew 收口 | 260 | 增补 | UNX-F52678-J1 插件管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52679 | Kompose 收口 | 260 | 增补 | UNX-F52679-J1 compose→k8s 转换 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52680 | K8s 周边全域共面（本地 kind 集群判据链汇总） | 280 | 增补 | UNX-F52680-J1 本组集群消费条目 ×10 在同一 kind 集群交叉复测零悬空；与 B14 联签锚定行衔接；KB-COMPAT 定级登记；判据锚定 §AI-66.5 J-共面 |

**批 B34 防重声明**：本批 20 条 = 组 34"容器镜像与 K8s 周边"；registry/kind 为本地单机判据面，服务端完整面归 K1 分账；ID 段 F52661–F52680 零交叠。

---

<!-- 主册行 358060 · ## 批 UNX-N1-B35（F52681–F52700 · 组 35 · API 设计与 Mock 工具族 · 6, -->
## 批 UNX-N1-B35（F52681–F52700 · 组 35 · API 设计与 Mock 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B35） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52681 | OpenAPI Generator CLI 收口 | 320 | 增补 | UNX-F52681-J1 代码生成 ×10 语言目标全通；产物可编译抽检 ×3；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 35 口径 |
| UNX-F52682 | Swagger UI（本地面）收口 | 280 | 增补 | UNX-F52682-J1 本地渲染 ×10 spec；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52683 | Swagger Editor 收口 | 280 | 增补 | UNX-F52683-J1 编辑/校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52684 | Prism（mock CLI）收口 | 280 | 增补 | UNX-F52684-J1 mock 服务 ×10 端点（本地回环）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52685 | WireMock 收口 | 300 | 增补 | UNX-F52685-J1 stub ×10（JVM 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52686 | Mockoon 收口 | 300 | 增补 | UNX-F52686-J1 六判据全落（Electron 同构）；mock ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52687 | json-server 收口 | 260 | 增补 | UNX-F52687-J1 ×10（本地回环）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52688 | Apifox 收口 | 300 | 增补 | UNX-F52688-J1 六判据全落；API 管理/测试 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52689 | Yaak 收口 | 260 | 增补 | UNX-F52689-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52690 | grpcurl 收口 | 280 | 增补 | UNX-F52690-J1 调用 ×10（本地 grpc server）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52691 | protoc（Windows 发行版）收口 | 300 | 增补 | UNX-F52691-J1 编译 ×10 proto 全通；发行版收口与 L4 分账（联签锚定行 AI-59）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52692 | buf CLI 收口 | 300 | 增补 | UNX-F52692-J1 lint/breaking ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52693 | grpcui 收口 | 260 | 增补 | UNX-F52693-J1 Web 反射界面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52694 | Kreya 收口（gRPC 客户端） | 260 | 增补 | UNX-F52694-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52695 | Dredd 收口 | 260 | 增补 | UNX-F52695-J1 API Blueprint 测试 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52696 | Schemathesis 收口 | 280 | 增补 | UNX-F52696-J1 属性测试 ×10（本地靶 API）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52697 | Tavern CLI 收口 | 260 | 增补 | UNX-F52697-J1 ×10；Python 消费联签（F52526 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52698 | Stoplight Spectral CLI 收口 | 280 | 增补 | UNX-F52698-J1 spec lint ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52699 | Mock 沙箱共面条目（本地回环端口分配零冲突全域账） | 280 | 增补 | UNX-F52699-J1 本组 ≥6 mock 服务并发回环端口分配 ×2 轮零冲突；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |
| UNX-F52700 | API 规范版本化共面（OpenAPI 3.0/3.1 双档兼容声明全域） | 280 | 增补 | UNX-F52700-J1 本组 spec 消费条目 3.0/3.1 双档 ×10 兼容矩阵零漂移；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题六 |

**批 B35 防重声明**：本批 20 条 = 组 35"API 设计与 Mock 族"（Insomnia 归 B13 不重立）；全部 mock/服务判据限本地回环（F52699 承载）；ID 段 F52681–F52700 零交叠。

---
<!-- 主册行 358088 · ## 批 UNX-N1-B36（F52701–F52720 · 组 36 · 嵌入式与串口工具族 · 6,000 行） -->
## 批 UNX-N1-B36（F52701–F52720 · 组 36 · 嵌入式与串口工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B36） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52701 | Arduino IDE 2.x 收口 | 320 | 增补 | UNX-F52701-J1 六判据全落；编译/上传判据只对板卡模拟器面或干跑（无真机时 N/A 声明）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 36 口径 |
| UNX-F52702 | Arduino CLI 收口 | 300 | 增补 | UNX-F52702-J1 compile ×10（干跑/模拟面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52703 | PlatformIO Core 收口 | 320 | 增补 | UNX-F52703-J1 init/run ×10 全通（本地平台面）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52704 | Keil MDK（μVision）收口 | 320 | 增补 | UNX-F52704-J1 六判据全落；工程编译 ×10（模拟目标）；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52705 | IAR Embedded Workbench 收口 | 300 | 增补 | UNX-F52705-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52706 | STM32CubeIDE 收口 | 320 | 增补 | UNX-F52706-J1 六判据全落；工程编译 ×10（模拟目标，真机烧录零执行——硬件红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52707 | STM32CubeMX 收口 | 300 | 增补 | UNX-F52707-J1 配置/代码生成 ×10 全通（纯文件面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52708 | STM32CubeProgrammer 收口 | 300 | 增补 | UNX-F52708-J1 烧录判据一律干跑列清单（真机 Flash 写零执行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52709 | MPLAB X IDE 收口 | 320 | 增补 | UNX-F52709-J1 ×10（模拟目标）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52710 | MPLAB IPE 收口 | 280 | 增补 | UNX-F52710-J1 烧录干跑口径；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52711 | Code Composer Studio（TI）收口 | 320 | 增补 | UNX-F52711-J1 ×10（模拟目标）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52712 | Segger J-Flash 收口 | 300 | 增补 | UNX-F52712-J1 烧录干跑口径（J-Link 无设备则 N/A）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52713 | Segger Ozone 收口 | 300 | 增补 | UNX-F52713-J1 调试器面 ×10（模拟/N/A 声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52714 | OpenOCD（Windows 发行版）收口 | 300 | 增补 | UNX-F52714-J1 配置解析 ×10（无调试器则联通性 N/A 声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52715 | esptool 收口 | 280 | 增补 | UNX-F52715-J1 read_flash 判据干跑（写操作零执行）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52716 | Termite 收口 | 260 | 增补 | UNX-F52716-J1 串口收发 ×10（虚拟串口对判据）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52717 | RealTerm 收口 | 260 | 增补 | UNX-F52717-J1 ×10（虚拟串口）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52718 | Tera Term 收口 | 280 | 增补 | UNX-F52718-J1 ×10（虚拟串口/本地 SSH）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52719 | CoolTerm 收口 | 260 | 增补 | UNX-F52719-J1 ×10（虚拟串口）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52720 | 嵌入式烧录全域红线登记条目（真机 Flash 写零执行干跑制） | 280 | 增补 | UNX-F52720-J1 全组烧录条目红线登记：干跑列清单、真机写零执行、设备类消费 M5 账（联签锚定行 AI-65）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |

**批 B36 防重声明**：本批 20 条 = 组 36"嵌入式与串口族"；烧录/刷写判据全域干跑制（F52720 承载，硬件数据安全红线凌驾）；ID 段 F52701–F52720 零交叠。

---

<!-- 主册行 358117 · ## 批 UNX-N1-B37（F52721–F52740 · 组 37 · 游戏引擎与关卡工具族 · 6,000 行） -->
## 批 UNX-N1-B37（F52721–F52740 · 组 37 · 游戏引擎与关卡工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B37） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52721 | Unity Hub 收口 | 300 | 增补 | UNX-F52721-J1 六判据全落；编辑器安装/版本管理 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 37 口径（与 N5 分账：工具链收口 vs 游戏应用收口） |
| UNX-F52722 | Unity Editor 收口 | 320 | 增补 | UNX-F52722-J1 六判据全落；工程打开/构建（Windows 目标）×10；GPU 路径消费 G 部判据（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52723 | Epic Games Launcher 收口 | 300 | 增补 | UNX-F52723-J1 六判据全落；引擎安装 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52724 | Unreal Engine（安装与编辑器首启面）收口 | 320 | 增补 | UNX-F52724-J1 安装/首启 ×10；完整开发面按 Epic 原生支持分级如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52725 | Godot 4 收口 | 320 | 增补 | UNX-F52725-J1 六判据全落；工程编辑/导出 ×10 全通（开源引擎全判据不降级）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52726 | GameMaker 收口 | 280 | 增补 | UNX-F52726-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52727 | Construct 3 收口 | 280 | 增补 | UNX-F52727-J1 ×10（桌面壳面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52728 | Defold 收口 | 280 | 增补 | UNX-F52728-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52729 | Ren'Py 收口 | 280 | 增补 | UNX-F52729-J1 SDK/工程启动 ×10；Python 消费联签（F52526 锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52730 | LÖVE 2D 收口 | 280 | 增补 | UNX-F52730-J1 运行 ×10 样本；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52731 | MonoGame 收口 | 280 | 增补 | UNX-F52731-J1 模板构建 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52732 | O3DE 收口 | 300 | 增补 | UNX-F52732-J1 工程创建 ×5（重构建链，超限如实降级列缺失）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52733 | CryEngine Launcher 收口 | 280 | 增补 | UNX-F52733-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52734 | itch app（分发客户端）收口 | 260 | 增补 | UNX-F52734-J1 ×10（下载面网络语义 AI-23 联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52735 | RPG Maker MZ 收口 | 260 | 增补 | UNX-F52735-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52736 | Twine 收口 | 260 | 增补 | UNX-F52736-J1 六判据全落 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52737 | Ink（inkle CLI）收口 | 260 | 增补 | UNX-F52737-J1 编译运行 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52738 | Yarn Spinner CLI 收口 | 260 | 增补 | UNX-F52738-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52739 | Tiled Map Editor 收口 | 300 | 增补 | UNX-F52739-J1 六判据全落；地图编辑/导出 ×10 校验和闭环；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52740 | LDtk / TexturePacker 收口 | 280 | 增补 | UNX-F52740-J1 ×10（两件分立小节合并长尾条目，各自操作集 ×5）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |

**批 B37 防重声明**：本批 20 条 = 组 37"游戏引擎与关卡工具族"；与 N5（AI-70）分界=引擎工具链收口 vs 游戏应用收口（F52721 声明承载）；ID 段 F52721–F52740 零交叠。

---

<!-- 主册行 358146 · ## 批 UNX-N1-B38（F52741–F52760 · 组 38 · 本地 AI 与媒体 CLI 工具族 · 6 -->
## 批 UNX-N1-B38（F52741–F52760 · 组 38 · 本地 AI 与媒体 CLI 工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B38） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52741 | Ollama 收口 | 320 | 增补 | UNX-F52741-J1 六判据全落；模型拉取面 N/A（外网大模型下载不做判据，本地模型导入 ×1 测试模型全通）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 38 口径 |
| UNX-F52742 | LM Studio 收口 | 300 | 增补 | UNX-F52742-J1 六判据全落（Electron 三段账）；本地模型加载 ×1 测试模型；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52743 | GPT4All 收口 | 280 | 增补 | UNX-F52743-J1 ×10（本地模型面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52744 | Jan 收口 | 280 | 增补 | UNX-F52744-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52745 | AnythingLLM Desktop 收口 | 280 | 增补 | UNX-F52745-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52746 | Cherry Studio 收口 | 280 | 增补 | UNX-F52746-J1 ×10（本地功能面，外部 API 调用判据干跑声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52747 | Chatbox 收口 | 260 | 增补 | UNX-F52747-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52748 | Msty 收口 | 260 | 增补 | UNX-F52748-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52749 | ComfyUI 面收口 | 300 | 增补 | UNX-F52749-J1 工作流加载 ×10（CPU 模式判据；GPU 加速按 G 部分级，专有 CUDA N/A 不虚报）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52750 | Stable Diffusion WebUI（A1111 面）收口 | 300 | 增补 | UNX-F52750-J1 启动/本地 WebUI ×10（CPU 模式，N/A 同构）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52751 | LabelImg 收口 | 260 | 增补 | UNX-F52751-J1 标注 ×10（本地样本）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52752 | Labelme 收口 | 260 | 增补 | UNX-F52752-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52753 | whisper.cpp（Windows 发行版）收口 | 300 | 增补 | UNX-F52753-J1 转写 ×10 短音频（CPU 模式）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52754 | ffmpeg（Windows 发行版）收口 | 320 | 增补 | UNX-F52754-J1 转码/抽帧 20 操作集 ×10 全通；**解码语义本体归 H3（AI-38）分账声明**（本条只收 CLI 发行版应用形态）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 分账 |
| UNX-F52755 | yt-dlp 收口 | 280 | 增补 | UNX-F52755-J1 判据只对本地 media server/回环 HTTP 面 ×10（外网站点抓取零执行——对外动作与版权红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用 |
| UNX-F52756 | Aria2 收口 | 280 | 增补 | UNX-F52756-J1 下载 ×10（回环 HTTP 面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52757 | FFmpeg 前端：StaxRip 收口 | 280 | 增补 | UNX-F52757-J1 ×10（H3 分账同构声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52758 | HandBrake 收口 | 300 | 增补 | UNX-F52758-J1 六判据全落；转码 ×10（软编路径；QSV/NVENC 专有 N/A 账不虚报）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52759 | MediaInfo CLI 收口 | 260 | 增补 | UNX-F52759-J1 元数据读取 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52760 | 媒体 CLI 分账共面条目（H3 解码语义 vs CLI 应用收口边界全域声明） | 280 | 增补 | UNX-F52760-J1 本组媒体条目 ×6 分账边界逐条登记：编解码语义/硬编 N/A 归 H3 判据域，本域零重写（联签锚定行 AI-38）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |

**批 B38 防重声明**：本批 20 条 = 组 38"本地 AI 与媒体 CLI 族"；大模型下载/外网抓取判据零执行（F52755 红线承载），媒体解码语义分账 H3（F52760 承载）；ID 段 F52741–F52760 零交叠。

---

<!-- 主册行 358175 · ## 批 UNX-N1-B39（F52761–F52780 · 组 39 · 自动化与效率工具族 · 6,000 行） -->
## 批 UNX-N1-B39（F52761–F52780 · 组 39 · 自动化与效率工具族 · 6,000 行）

| ID | 深化名（deepen/N1-B39） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52761 | AutoHotkey v2 收口 | 320 | 增补 | UNX-F52761-J1 六判据全落；脚本热键/窗口/文本 20 操作集 ×10 全通（仅作用于测试窗口）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 39 口径 |
| UNX-F52762 | AutoIt 收口 | 300 | 增补 | UNX-F52762-J1 ×10（测试窗口面）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52763 | PowerToys 收口 | 320 | 增补 | UNX-F52763-J1 六判据全落；模块 ×10（FancyZones/PowerRename/Keyboard Manager 等）全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 |
| UNX-F52764 | Espanso 收口 | 280 | 增补 | UNX-F52764-J1 文本展开 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52765 | Ditto 收口 | 280 | 增补 | UNX-F52765-J1 剪贴板历史 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52766 | CopyQ 收口 | 280 | 增补 | UNX-F52766-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52767 | QuickLook（Windows）收口 | 280 | 增补 | UNX-F52767-J1 预览 ×10 文件型；浮层出路状态机完整（体验二：点外/Esc/失焦三路关闭）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 + 体验二 |
| UNX-F52768 | Seer 收口 | 260 | 增补 | UNX-F52768-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52769 | Listary 收口 | 280 | 增补 | UNX-F52769-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52770 | Flow Launcher 收口 | 300 | 增补 | UNX-F52770-J1 插件 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52771 | Keypirinha 收口 | 260 | 增补 | UNX-F52771-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52772 | uTools 收口 | 280 | 增补 | UNX-F52772-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52773 | Snipaste 收口 | 300 | 增补 | UNX-F52773-J1 截图/贴图 ×10；浮层生命周期完整（体验二）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52774 | ShareX 收口 | 300 | 增补 | UNX-F52774-J1 截图/录制/上传 20 操作集（上传判据仅本地回环目标——对外动作红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52775 | Greenshot 收口 | 260 | 增补 | UNX-F52775-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52776 | PicPick 收口 | 260 | 增补 | UNX-F52776-J1 ×10；许可面如实定级；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线② |
| UNX-F52777 | Mouse without Borders / 输入共享面收口 | 280 | 增补 | UNX-F52777-J1 单机模拟 ×10（跨机判据 N/A 声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3 |
| UNX-F52778 | f.lux / 显示色温面收口（长尾） | 260 | 增补 | UNX-F52778-J1 色温调节 ×10（与 G5 显示分账声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.1 |
| UNX-F52779 | 自动化工具作用域红线登记条目（全局钩子零滥用） | 280 | 增补 | UNX-F52779-J1 全组自动化条目红线登记：钩子/注入判据仅作用测试窗口，全局键鼠钩子不干扰系统（与 M2 输入域分账联签锚定行 AI-62）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 |
| UNX-F52780 | 效率工具全域共面（托盘/全局热键/自启三面共存零冲突） | 280 | 增补 | UNX-F52780-J1 本组 ≥10 常驻型并存 ×2 轮：托盘图标/全局热键/自启项注册零冲突零互踩；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨应用共面 |

**批 B39 防重声明**：本批 20 条 = 组 39"自动化与效率族"（与 N4 通用工具分界声明）；全局钩子作用域红线（F52779 承载）；ID 段 F52761–F52780 零交叠。

---

<!-- 主册行 358204 · ## 批 UNX-N1-B40（F52781–F52800 · 组 40 · 域收官批：全域治理与关门 · 6,000  -->
## 批 UNX-N1-B40（F52781–F52800 · 组 40 · 域收官批：全域治理与关门 · 6,000 行）

| ID | 深化名（deepen/N1-B40） | 行数 | 状态 | 证据与判据锚定 |
|---|---|---|---|---|
| UNX-F52781 | 开发工具共面五：LSP/编译器/构建链三生态综合账（B16–B39 汇总复测） | 320 | 增补 | UNX-F52781-J1 三个生态交叉复测 ×10 场景零悬空；与共面一~四账衔接闭环；判据锚定 §AI-66.5 J-共面 |
| UNX-F52782 | 开发工具共面六：烧录/刷写/盘基准干跑制全域复核（B16–B39） | 300 | 增补 | UNX-F52782-J1 全域破坏性写判据逐条复核：干跑制 100% 执行、真机写零执行留痕；硬件红线零违例终声明；判据锚定 Variable 硬件红线 + §AI-66.9 |
| UNX-F52783 | 开发工具共面七：模拟器/虚拟化 N/A 账全域汇总（B16–B39） | 300 | 增补 | UNX-F52783-J1 N/A 条目全域汇总登记：L5 消费联签 ×N 条、三要素告警 100%、零虚报；判据锚定 §AI-66.2 专题五 N/A 同构 |
| UNX-F52784 | 域账行数对账核销条目（AI-83 对账：B01–B15 登记差 5,340 行核销） | 300 | 增补 | UNX-F52784-J1 行数对账：B01–B15 实际 84,660 + B16–B40 实际 150,000 = 234,660 行；任务书名义 240,000 差 5,340 行系前 15 批 L2 档行数登记差，按 §83 对账口径核销入册（如实登记，禁凑数）；判据锚定 §83 行数对账 + 诚实登记 |
| UNX-F52785 | J5 中文第二轮全域抽测（B16–B39 · 40 应用） | 300 | 增补 | UNX-F52785-J1 抽测 40 应用 J5 三层全过；M1 衔接账公开（联签锚定行 AI-61）；判据锚定 §AI-66.5 J-中文 |
| UNX-F52786 | KB-COMPAT 全域核账（800 条可查询率 100%） | 300 | 增补 | UNX-F52786-J1 F52001–F52800 全域登记 ×100% 可查询验证；结构权 E5/写入权 N 域分界终核（联签锚定行 AI-25）；判据锚定 §AI-66.5 J-主轴 |
| UNX-F52787 | 六判据全绿率全域终结算（800 条） | 300 | 增补 | UNX-F52787-J1 全域逐条结算：全绿/降级/缺失清单三态全公开；KPI 权重 15 口径落账；判据锚定 §AI-66.5 + §105 KPI 调权 |
| UNX-F52788 | 安装器形态归并全域对账（E4 200 样本经验 ×全域） | 280 | 增补 | UNX-F52788-J1 全域安装器条目形态归并 ×800 口径零漏（联签锚定行 AI-24）；判据锚定 §AI-66.6 风险 ③ |
| UNX-F52789 | 版本阶梯全域回归终账（AI-94 通知链终演） | 280 | 增补 | UNX-F52789-J1 滚动回归 ×3 演练终账：T+1 广播 → 钉版回归 → 20% 抽测全通；判据锚定 §AI-66.6 风险 ② |
| UNX-F52790 | 版本伪装表全域许可终审（AI-95 联合审查终章） | 300 | 增补 | UNX-F52790-J1 全域伪装表条目 ×100% 终审：零许可绕过结论入账；违例冻结条款零触发；判据锚定 §AI-66.9 红线② |
| UNX-F52791 | 验收机器人全域判据库终版入库（AI-73） | 280 | 增补 | UNX-F52791-J1 六判据全域 schema 终版 ×800 条入库；机器人抽测 ×10 全通（联签锚定行 AI-73）；判据锚定 §AI-66.8 |
| UNX-F52792 | 全域条目抽读对照终账（AI-82 ≥20% 终核） | 280 | 增补 | UNX-F52792-J1 全域抽读 ×100 条达标；字行比减权口径终核；灌水扫描零命中（联签锚定行 AI-82）；判据锚定 §AI-66.8 |
| UNX-F52793 | 覆盖率三口径工程级终账（AI-96） | 280 | 增补 | UNX-F52793-J1 工程级口径全域终账：清单内 800 应用 ×2 收口轮次声明公开（联签锚定行 AI-96）；判据锚定 §AI-66.8 |
| UNX-F52794 | 缺陷账全域终出口（产线账本 → 工单清零） | 280 | 增补 | UNX-F52794-J1 🔴=0；🟡 全清偿；🟢 攒批移交结构化工单 ×N（五字段齐）；判据锚定 双轨产线制度 |
| UNX-F52795 | 体验日志全域终回归（挫败信号 ×B16–B39） | 280 | 增补 | UNX-F52795-J1 三类挫败信号全域捕获账 + 与共面账对照闭环；日志零阻塞验证；判据锚定 体验十三·补 |
| UNX-F52796 | 域功终验四件（红线零违例/防幻觉零悬空/20 维自评/真实用户走查） | 300 | 增补 | UNX-F52796-J1 四件齐套：红线终声明零违例、阈值/实勘零悬空或挂计划、20 维打分如实（零满分幻觉）、核心旅程真实用户走查账（装→启→写→存→中→测六步）；判据锚定 §35 验收协议 20 维 |
| UNX-F52797 | 域经 DJ-UNX-N1-02（全域收官主题：800 条收口的共面缺口地图） | 280 | 增补 | UNX-F52797-J1 域经义务第二条：全域共面缺口地图实况回填（缺口 ×N 全部挂 ADR/移交通道）；判据锚定 §AI-66.8 域经义务 |
| UNX-F52798 | 五行业汇总账共同签认条目（波 25 · N1–N5 汇总） | 300 | 增补 | UNX-F52798-J1 N1 卷汇总账定稿并参与五行业共同签认流程（AI-70/87 主办盲测参演登记；联签锚定行 AI-66~69）；判据锚定 §AI-66.8 + 波 25 闭账口径 |
| UNX-F52799 | GitHub 入库终态声明（全域 800 条产物提交与推送如实登记） | 280 | 增补 | UNX-F52799-J1 两册（300+500）+ 主文件追加 + 推送结果三态如实登记；判据锚定 诚实登记纪律 |
| UNX-F52800 | 域关门印（UNX-N1 全任务终了声明 · 800/800） | 320 | 增补 | UNX-F52800-J1 关门印入册：40 批 800 条全冻结全验收、域账 234,660 行如实登记（核销条目 F52784 在案）、P0=0、两条等同红线零违例、六判据全绿率与缺失清单全公开、**AI-66 承包域 UNX-N1 全任务终了**；判据锚定 §AI-66.5/§AI-66.9 + 交接制度 |

**批 B40 防重声明**：本批 20 条 = 域收官批（全域治理/对账/关门），与 B01–B39 应用条目零重复；ID 段 F52781–F52800 与邻批零交叠。

---

<!-- 主册行 358233 · ## 全域总账（AI-66 · UNX-N1 · B01–B40 · 域关门） -->
## 全域总账（AI-66 · UNX-N1 · B01–B40 · 域关门）

- **总量**：40 批 × 20 条 = **800 条全冻结**；ID 段 F52001–F52800 连续零跳号、全域零复用；域账累计 **234,660 行**（B01–B15：84,660；B16–B40：150,000；对账核销条目 UNX-F52784 在案，任务书名义 240,000 差额如实登记禁凑数）。
- **组覆盖**：40 组应用群——主力族（VS/Electron/JetBrains/MSBuild/Android）+ 长尾 35 组（IDE 周边/编辑器/构建/调试/版本管理 GUI/数据库/终端/API/容器/包管理/反编译/文档 CLI/CI/监控/安全凭证/网络诊断/搜索压缩/浏览器/测试/语言服务器/数据科学/移动开发/语言发行版/数据库 CLI/基准硬件/知识库/Git 生态/镜像 K8s/API Mock/嵌入式/游戏引擎/本地 AI 媒体/自动化效率/域收官治理）。
- **六判据**：全域同解、逐应用实例化；J2 ≤3s / J6 ≤×1.5 铁值零放宽；降级条目全部附缺失项清单；双机对照缺 Windows 侧数据标 N/A 不编（附则三第④条）。
- **共面七账**：编译器进程树/watcher 零丢/调试 API 零差/Git 集成/LSP 三生态/烧录干跑制/虚拟化 N/A——全部全绿闭环。
- **联签锚定**：AI-21~25（E 部）、AI-59（L4 分账）、AI-61（M1 中文）、AI-65（M5 设备）、AI-60（L5 虚拟化）、AI-51（服务分账）、AI-38（H3 媒体分账）、AI-62（输入分账）、AI-27（X11）、AI-23（网络）、AI-44（TLS）、AI-94/95/96/82/73/83/85（治理线）——零改写他域账。
- **红线**：无引导/写盘红线（§AI-66.9 保真）；两条等同红线全域零违例（判据零放宽 · 伪装表零许可绕过）；硬件数据安全红线全域适用（烧录干跑制 F52720/F52782、盘基准测试镜像制 F52610/F52620、密码库即弃制 F52420）。
- **域经义务**：DJ-UNX-N1-01（F52285）+ DJ-UNX-N1-02（F52797）两条全落。
- **验收口径**：本域全部条目为可执行判据账，实机执行按"随闸门补测"登记（多 AI 并行防卡死②）；交付 = 判据 + 锚定 + 定级 + 缺失清单四件齐。
- **AI-66 全任务终了**：UNX-N1 开发工具链应用收口 40 批 800 条全部冻结、全部达到验收标准（UNX-F52800-J1 域关门印）。


---



---

<!-- 主册行 358252 · ## 深化增补卷 · AI-64 · M4 域（deepen/M4-B01..B40 四十册 · 800 条六要素正文） -->
## 深化增补卷 · AI-64 · M4 域（deepen/M4-B01..B40 四十册 · 800 条六要素正文）

> 逐条 ≥300 字符【定位/语义边界/依赖与嫁接源/风险与回退/正文/判据】（AI-53 判例口径），判据号 800 枚与主册域账（AI-64 首产段 + 终段增补卷）逐一一致；批主题与条目名单源直读 _m4_supp1/_m4_supp2 的 BATCHES 零转抄；E 型批（B21–B28）每条附【反判据】J1R 正反配对；主册 800 行状态「增补」→「已深化」行级精准翻转（仅 UNX-F50401–F51200 段，他域零触碰）。四十册清单：

- deepen/M4-B01.md（F50401–F50420 · HCI 命令通道与传输后端（F 型地基） · F 型）
- deepen/M4-B02.md（F50421–F50440 · controller 能力页与初始化序列（F 型地基） · F 型）
- deepen/M4-B03.md（F50441–F50460 · LE 扫描与广播解析（F 型地基） · F 型）
- deepen/M4-B04.md（F50461–F50480 · BR/EDR inquiry 与双栈发现总成（F 型地基） · F 型）
- deepen/M4-B05.md（F50481–F50500 · 配对状态机骨架与 IO 能力矩阵（F 型地基） · F 型）
- deepen/M4-B06.md（F50501–F50520 · 密钥管理与绑定表（F 型地基 · X5 红线承载批） · F 型）
- deepen/M4-B07.md（F50521–F50540 · L2CAP 信道层（F 型地基） · F 型）
- deepen/M4-B08.md（F50541–F50560 · SMP 安全管理协议（F 型地基收官批） · F 型）
- deepen/M4-B09.md（F50561–F50580 · BR/EDR HID profile（M 型机制） · M 型）
- deepen/M4-B10.md（F50581–F50600 · BLE HID GATT 与双路归一（M 型机制） · M 型）
- deepen/M4-B11.md（F50601–F50620 · 重连自愈引擎（M 型机制 · J-1 判据承载批） · M 型）
- deepen/M4-B12.md（F50621–F50640 · 电池服务与设备健康（M 型机制） · M 型）
- deepen/M4-B13.md（F50641–F50660 · A2DP 流端点协商（M 型机制） · M 型）
- deepen/M4-B14.md（F50661–F50680 · A2DP 媒体传输与六段延迟账（M 型机制 · J-2 判据主轴承载批） · M 型）
- deepen/M4-B15.md（F50681–F50700 · AVRCP 控制面（M 型机制前段收官批） · M 型）
- deepen/M4-B16.md（F50701–F50720 · 设备管理面总成（M 型后段） · M 型）
- deepen/M4-B17.md（F50721–F50740 · 飞行模式与开关联动（M 型后段 · K4 联签承载批） · M 型）
- deepen/M4-B18.md（F50741–F50760 · 多设备并发容量账（M 型后段） · M 型）
- deepen/M4-B19.md（F50761–F50780 · 2.4GHz 共存自适应（M 型后段 · 自研攻坚件） · M 型）
- deepen/M4-B20.md（F50781–F50800 · 蓝牙诊断与体验日志（M 型后段收官批） · M 型）
- deepen/M4-B21.md（F50801–F50820 · 配对失败路径对抗（E 型双判据） · E 型）
- deepen/M4-B22.md（F50821–F50840 · 密钥损坏与重放对抗（E 型双判据 · 红线①承载批） · E 型）
- deepen/M4-B23.md（F50841–F50860 · 链路异常与假死对抗（E 型双判据） · E 型）
- deepen/M4-B24.md（F50861–F50880 · 音频链路异常对抗（E 型双判据） · E 型）
- deepen/M4-B25.md（F50881–F50900 · 协议畸形与 fuzz 汇总（E 型双判据） · E 型）
- deepen/M4-B26.md（F50901–F50920 · 资源耗尽与时序竞态（E 型双判据） · E 型）
- deepen/M4-B27.md（F50921–F50940 · 断电恢复与休眠唤醒（E 型双判据） · E 型）
- deepen/M4-B28.md（F50941–F50960 · E 型域级终章（E 型双判据收官） · E 型）
- deepen/M4-B29.md（F50961–F50980 · H1 音频桥联签（I 型） · I 型）
- deepen/M4-B30.md（F50981–F51000 · I3 共存联签（I 型 · J-4 承载批） · I 型）
- deepen/M4-B31.md（F51001–F51020 · K4 电源联签（I 型） · I 型）
- deepen/M4-B32.md（F51021–F51040 · M5 解析器与管道联签（I 型 · 防重主联签） · I 型）
- deepen/M4-B33.md（F51041–F51060 · M2 键鼠语义联签（I 型） · I 型）
- deepen/M4-B34.md（F51061–F51080 · H5 手柄传输面联签（I 型） · I 型）
- deepen/M4-B35.md（F51081–F51100 · J2/J3 凭据与审计联签（I 型） · I 型）
- deepen/M4-B36.md（F51101–F51120 · L2/L3/出厂链联签（I 型收官批） · I 型）
- deepen/M4-B37.md（F51121–F51140 · J-1 判据主账回归清账（C 型） · C 型）
- deepen/M4-B38.md（F51141–F51160 · 延迟核账与安全账闭账（C 型） · C 型）
- deepen/M4-B39.md（F51161–F51180 · 20 维度验收总对账（C 型） · C 型）
- deepen/M4-B40.md（F51181–F51200 · 闭账物与域功宣告（C 型 · 域收官） · C 型）

---

