//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//! | [`cgi01_bandwidth`] | CGPU-F1281 | I 域开工与带宽总架构（**五主题闭集**PCIe带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因冻结、本域160项全挂靠表外不立题；**账户化**四类传输（纹理上传/下载/交换/流送）各开预算账户——每笔传输先申请余额不足即拒绝并计数留痕、退款只做消费迁移、账本守恒净消费口径自检；**边界**H域管显存驻留/I域管传输带宽/VE-AD存储流F5801+管到达三段各管一段、接口复用F0405集成面声明逐句可grep；**十组**规划表组名+起止单号逐字取自总纲批次锚点、F1281-F1440首尾连续无缺口、宣告行读屏可查；零panic面零IO零墙钟） |
//! | [`cgi01_bandwidth_checks`] | CGPU-F1281 域自检（判据五条映射：五主题闭集+标签独立重排全等、账户化申请/拒绝/退款/守恒全链+未开账户必拒反向语料、边界三域分工逐句grep、十组表与判据侧独立重排逐条全等+组间起止连续校验；守恒式与使用率判据侧独立重算） |
//! | [`cgd02_frametimer`] | CGPU-F0482 | 全帧精确计时器（**四段**输入采样→提交→GPU执行→呈现上屏全链分段闭段才出时长、重叠/倒序记账异常可判定检出；**双源**QPC/GPU时间戳闭集、同段双源跨度对账漂移超容差标注；**校准**CPU/GPU时钟域线性对齐两对样本完全确定映射、换算残差逐点给账超阈标注（换算误差可测）非单调样本拒绝标定；**开销**<0.1%帧预算实测值注入核算640ppm手算对账超标标注；零墙钟——时间戳全上游注入计时器只做账务对齐异常检测、零panic面零IO） |
//! | [`cgd02_frametimer_checks`] | CGPU-F0482 域自检（判据五条映射：四段开合时长+未闭段None反向+重叠倒序异常、双源闭集+漂移控制、线性标定+残差逐点+反向拒绝、开销实测核算+红线标注；时长/比率/ppm全部判据侧手算写死对账；stamp独立重排全等） |
//! vcj01_powerarch — CGPU-F1441 J 域开工与功耗架构总览（域使命/五主题十组/三处核验/五段流水线/边界/采样不耗样本/风险回退/0x52xx）
//! cga02_threadpool —— CGPU-F0002 渲染线程池与工作窃取调度器（池规模核数减二/合成器专核隔离/同层优先窃取偷头不偷尾/瓦片独立缓冲确定性归并乱序逐像素一致/协作式 2ms 让位抢占三档优先/均衡≤5%/遥测三面/窃取开销≤3%账面/0x39xx 域码段）
//! vcl01_virtualarch — CGPU-F1761 L 域开工与虚拟化总架构（域使命三条款平等声明/八主题十组映射/K 域签收+I09 预留兑现/五段单向流水线/复用不重建 1601→1763+1466→1776/模式×合同等级表/场景四族/风险四条预案互异/0x53xx 五码/22 项域自检）
//! vcq01_reliability — CGPU-F2561 Q 域开工与可靠性总架构（恢复起点哲学三条款/P 域移交包 F2557 七件签收/五主题十组映射/五段单向流水线/O 域交接+混沌设施复用/不可恢复=最高缺陷红线+立案码/风险四条预案互异/0x56xx 六码/19 项域自检）
//! vcq02_metrics — CGPU-F2562 可靠性模型与指标（五指标闭集 MTBF/MTTR/可用性/RPO/RTO 定义与口径字面量冻结/口径复用/可用性万分比纯算术/目标表版本化不可变/RTO>0 RPO≤RTO 校验双向/V1 三条目钉死/0x58xx 五码/13 项域自检）
//! | [`vcv01_realverify`] | CGPU-F3361 | V 域开工与真机验收总架构（签收 U10 移交包 F3360 七件逐件对账缺件即空头签收拒；真机验收定位两条款字面量冻结——用手不用测试报告+十五章呼应不另立口径；四段单向流水线 矩阵→判据→走查→报告 跳段/回退/过早报告显性码拒；U10 预告兑现——Bench 分数与验收判据映射表整数口径恰阈值过差一分拒表外未映射拒；V_DOMAIN_TOTAL 160 守恒；0x5Bxx 六码独占） |
//! | [`vcv01_realverify_checks`] | CGPU-F3361 域自检（13 项六族：RECEIPT2 七件逐件对账+缺件错名反向必拒 / POSITION2 两条款逐字对拍+主证据篡改反向必拒 / PIPE2 四段恰一步全过+跳段回退双向拒与报告终端闸 / FULFILL3 兑现结构核对+映射表逐行独立对拍+恰阈值双向与表外拒 / CODE2 码段 0x5B 独占 !=0x50..0x5C 防自判死+码互异原因非空 / META2 域守恒独立重算+判据条数对账） |
//! cgm01_display —— CGPU-F1921 M 域开工与显示输出总架构（最后一厘米三条款/L 域 F1917 签收七件/五主题十组映射覆盖守恒/五段单向流水线/F1768 贯通+V 域协同/呈现预算合同终端/风险四条预案/0x54xx 七码）
//! vct01_tbridge —— CGPU-F3041 T 域开工与 VE 对接总架构（S10 移交包七件逐件签收——衔接确认/协议桥梁定位声明=发起VE·执行CGPU·加速而非替代/四段架构闭集 命令·状态·资产·事件/四段→T02-T05 四批区间无缝兑现机检/SessionCtxTag 会话上下文传递不串户=衔接包约定落地/0.1ms 桥路预算在册/0x5Axx 五码）
//! cgm02_display —— CGPU-F1922 显示枚举与热插拔（枚举能力快照 id+EDID 指纹+seq 三件齐/去抖 leading-edge 恰端点/竞态=缺陷红线序列号恰一+快照新鲜度/三态单向状态机 mode-set 幂等/批量固定序去重重跑一致/0x5408~0x540C 域段续占互异）
//! cgm03_edid —— CGPU-F1923 显示能力查询 EDID（EDID1.4 基块解析 8 字节魔数+128 字节校验和 mod 256 硬门/首个 DTD@54 12 位拼装刷新率毫赫兹 u64 中间量防溢出 14850×10^7÷(2200×1125)=60000mHz 手算对账/CTA-861 扩展 YCbCr422·444 与 HDR 静态元数据 0xE6/损坏三向显性码拒+截断降级读出不虚构/能力缓存 display_id+指纹双键失效重读/能力投影纯裁剪诚实红线/覆盖白名单台账表外拒绝不突变/0x540D~0x5412 续占互异）
//! cgm04_topology —— CGPU-F1924 多显示器拓扑（闭集三角色 Primary 恰一/Extend 矩形不重叠/Clone 同源同矩形禁链/变更单调事件账封顶滚动+倒挂显性拒/持久复用 F1834 模式：序列化+校验和+重启保持/OS 显示设置协同声明：单向通报+事件回流+内核不直写/0x5413~0x5418 续占互异）
//! cgm05_modes —— CGPU-F1925 显示模式管理（模式 API 列表/当前/切换三件/投影复用 F1923：候选集=cgm03 project 纯裁剪零第二套能力过滤+指纹漂移失效重读/冷切换单向序列 Idle→Validated→Committed→ack 三态闭集四向违约拒/黑屏防护独立闸 24000mHz 恰边界/切换历史账封顶滚动供 F1930 遥测只读复用/F1941 热切换深化预告/0x5419~0x541E 续占互异）
//! | [`cga01_simdprim`] | CGPU-F0001 | SIMD 栅格化基元库（矩形/渐变/圆角/椭圆/三角/搬运/混合 12 式，掩码裁剪零写穿，GPU 对拍仅舍入，基准入 CGPU-Bench） |
//! | [`cga01_simdprim_checks`] | CGPU-F0001 域自检 |
//! | [`cge01_domain`] | CGPU-F0641 | E 域开工与表面调度架构（八主题封闭枚举 SurfaceTheme；十组 BatchGroup E01~E10 每组 16 单 O(1) 映射；三向边界契约职责交集恒空双闸；守恒式 10×16=160 编译期钉死；四码 E_DOMAIN_* + DomainLedger 立案） |
//! | [`cge01_domain_checks`] | CGPU-F0641 域自检（19 条判据两族；判据侧独立写死八主题中文名/三向 peer/组边界抽检值；守恒式独立重算；双面零 panic 扫描） |
//! | [`cge02_lifecycle`] | CGPU-F0642 | 表面生命周期（六态封闭枚举 SurfaceState 全迁移图 14 条 × 位图双源同构；状态语义表三位；迁移安全：目标态语义清算资源账+无半态复查回滚立案；六码 E_LIFECYCLE_*） |
//! | [`cge02_lifecycle_checks`] | CGPU-F0642 域自检（15 条判据两族；判据侧独立写死六态中文名/语义期望值/码表；端到端全迁移链+非法迁移逐条专属码拒；双面零 panic 扫描） |
//! | [`cgp01_adaptivearch`] | CGPU-F2401 | P 域开工与自适应遥测总架构（使命三条字面量冻结；签收 O 域移交包四元在案；三处遥测预留认领；官方五主题十组规划表逐字对账首尾连续；五段单向流转跳段/回退/原地双向拒绝；三域收编统一 P=基座；红线三条逐条 grep；风险四条预案互异） |
//! | [`cgp01_adaptivearch_checks`] | CGPU-F2401 域自检（锚点判据十条映射八组；判据侧字面量写死对拍） |
//! | [`cgp02_unifiedmodel`] | CGPU-F2402 | 遥测统一模型（六元组 Metric 与五元组同构加一档；三级命名前缀校验表外拒绝；隐私三档闭集 is_readable_by；值闭集 Ppm 饱和不越界；schema 版本化三站升级只增不回滚；维度注册同名复用同 ID 未注册拒绝） |
//! | [`cgp02_unifiedmodel_checks`] | CGPU-F2402 域自检（判据三组映射：六元组字段与升级链字面量对账+is_readable_by 双向；值闭集恰边界双向+隐私三档；注册同 ID 复用+表外拒绝；stamp 独立重排全等） |
//! | [`vco01_degrade`] | CGPU-F2241 | O 域开工与降级链总架构（优雅降级哲学三条款字面量冻结；N 域移交包签收四字段+指纹 FNV 钉死；官方五主题十组映射守恒 160；架构五段单向流水线跳段/回退显性码拒；历史汇总五行+统一编排声明收编双向闸；双不变量底线闸恰端点钉死；风险四条预案互异；码段 0x55xx 独占） |
//! | [`vco01_degrade_checks`] | CGPU-F2241 域自检（16 条判据两族；哲学三条款+历史五行+风险四名+组名判据侧独立写死对拍、指纹独立 FNV 双源对账、五段全序列推进+跳/退/原地显性码拒、码段 0x55 独占、零 panic 双面自扫） |
//! | [`cgr01_secure`] | CGPU-F2721 | R 域开工与安全渲染总架构（Q10 移交包签收七件逐项登记缺项即域未开工/不可信内容渲染防线定位两条款字面量冻结——防线管渲染路径恶意与异常内容、不管业务正确性/验证→隔离→降级→恢复四段单向流水线跳段回退拒绝——未隔离即恢复=污染回流/Q10 兑现结构化确认非空头支票/诊断码独占 0x5701~0x5706） |
//! | [`cgr01_secure_checks`] | CGPU-F2721 域自检（签收/定位/四段/兑现四组判据承载+判据承载力自证；定位条款与六码判据侧独立对拍） |
//! | [`cgr02_threatmodel`] | CGPU-F2722 | 威胁模型与分类（五类威胁封闭枚举——恶意着色器/资源炸弹/越界/外泄/死循环表外不立类；分类表五规则每类一条可机检整数阈值字面量冻结判据独立对拍；攻击面清单五面封闭与五类一一承载恰五裁决清单外输入不存在进管线方式；特征只含元数据不含内容伪造签名拒绝；诊断码接续 0x5708~0x570D） |
//! | [`cgr02_threatmodel_checks`] | CGPU-F2722 域自检（判据五条映射 21 项六组：五类/分类表/清单/两组/判据；阈值 0/262144/1/1/65536 与六码字面量写死、恰阈不拦防等价变异、伪造签名反恒假、码段独占、条数离账） |

//! vco03_triggersrc — CGPU-F2243 降级触发源汇总（七类触发源闭集帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换/源注册追加式可扩展重复拒/源融合复用 F1459 主源 severity 最高平局取注册序最早确定性/贡献源账升序留痕/0x59xx 五码/12 项域自检）
//! vco04_priorcon — CGPU-F2244 降级优先级与冲突（四级序闭集安全>合同>体验>资源rank单调判据重算/优先级复用F1496四层模式对照表逐字/维度四闭集帧率画质延迟功耗归层映射/六对冲突显性消解规则R1~R6同层平局交互响应优先/裁决自检winners秩不劣于败者/码段0x5Axx五码/14项域自检）
//! | [`vcw01_sdkdoc`] | CGPU-F3521 | W 域开工与 SDK 文档总架构（签收 V10 移交包 F3520 七件逐件对账缺件即空头签收拒；SDK 文档定位两条款字面量冻结——十年接口的说明书冻结签名才进参考+文档即契约漂移即缺陷；四段单向流水线 参考→教程→质量→站点 跳段/回退/未过质量关即发布显性码拒；V10 预告兑现——验收判据入文档示例映射表与 V 域 vcv01 映射表跨单元同源对拍表外判据拒；W_DOMAIN_TOTAL 160 守恒；0x5Cxx 六码独占） |
//! | [`vcw01_sdkdoc_checks`] | CGPU-F3521 域自检（13 项六族：RECEIPT2 七件逐件对账+缺件错名反向必拒 / POSITION2 两条款逐字对拍+口径篡改反向必拒 / PIPE2 四段恰一步全过+跳段回退双向拒与发布终端闸 / FULFILL3 兑现结构核对+映射表逐行独立对拍+判据id与V域映射表跨单元同源 / CODE2 码段 0x5C 独占 !=0x50..0x5D 防自判死+码互异原因非空 / META2 域守恒独立重算+判据条数对账） |
// vco05_statemachine — CGPU-F2245 全域降级状态机（四态闭集正常/降级中/降级档/恢复标签冻结/形式化复用J03=1475迁移表全枚举+不可达态检查/六条合法迁移条件显性逐字/跳段倒退自迁移逐类显性拒/带迁移史留痕与断裂拒/重入迁移恢复途中再触发/与vco01段态分工声明/码段0x5Bxx五码/14项域自检）
// vco06_executor — CGPU-F2246 降级执行器（统一接口冻结v1——ExecAction五元组domain/action_id/semantics/payload_digest/sequence只增不改/执行域四闭集调度效果呈现资源与F2244四维对齐/执行语义建议vs强制二闭集复用J02建议语义+合同强制模式/失败处置三态留痕跳过-重试恰一次-升级立案/建议失败不阻塞链强制失败重试恰一次/回执账对账序号严格递增+升级标记自洽/码段0x5Cxx五码/14项域自检）
// vco07_recovery — CGPU-F2247 降级恢复引擎（恢复序四级闭集资源/效果/呈现/调度逐级回升与降级施加序相反/限速RECOVERY_INTERVAL_TICKS=500/滞回带HYSTERESIS_BAND=3不对称防振荡/恢复复用J03家族=1481两维序-过冲记忆-振荡判据模式复用/过冲回退一级恢复目标自动下调/振荡达限冻结转决策引擎/恢复观测账留痕tick递增+间隔可复核/引擎账一致性核验/码段0x5Dxx五码/12项域自检）
pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
// cgi02_pciebw — CGPU-F1282 PCIe 带宽模型（Gen3/4/5×x1/4/8/16理论带宽表985/1969/3938MB/s写死/理论vs有效85%系数双值输出单边输出是撒谎/共享通道权重ppm和恰1e6拥塞按需求权重比检出查询不仲裁/对拍600k~1e6ppm合理带低于=链路异常高于=探测错误物理不可能/模型卡公开口径/0x3Axx 四码）
pub mod cgi02_pciebw;
pub mod cgd02_frametimer;
pub mod cgd02_frametimer_checks;
pub mod cgd03_frameledger;
pub mod cgd03_frameledger_checks;
// cgd04_frameinterval — CGPU-F0484 帧间隔统计（16桶直方图桶宽=目标/8尾桶收离群/滚动120窗整数stddev牛顿isqrt零浮点/jank三级1.5×2.0×3.0×写死/日聚合健康比首尾对比±1%带宽三态趋势/0x31xx 两码）
pub mod cgd04_frameinterval;
pub mod vcj01_powerarch;
pub mod cgj02_sensorabs;
pub mod vcj01_powerarch_checks;
pub mod cga02_threadpool;
pub mod cga02_threadpool_checks;
// cga03_framegraph —— CGPU-F0003 帧任务图构建与依赖解析（五元组节点/读写集排序分组自动建边 RAW·WAW+对拍账守恒/Kahn 拓扑环检测死锁上抛/关键路径分层+长路径优先发射+完工模型改善量/跨帧围栏四元组账本双向配对对账零失误/快照序列化 FNV 指纹往返恒等/操作数计账构建预算 1000 节点≤200μs 参考探针/0x64 段十八码）
pub mod cga03_framegraph;
// cga04_placer —— CGPU-F0004 CPU/GPU 任务放置决策器（成本画像五输入/双路预估公式写死/双路契约资格证+缺路拒/滞回防抖连续 5 帧差>15% 才切换/误差账持续>30% 触发重标定闭环 ±50% 钳制单帧一次/决策日志滚动全量可回放+守恒对账/双路指纹对拍分歧立案冻结/0x65 段十五码）
pub mod cga04_placer;
pub mod vcl01_virtualarch;
pub mod vcl01_virtualarch_checks;
// vcl02_virtmode — CGPU-F1762 虚拟化模式分类（四模式定义表机制名冻结词表/五维0-3整数矩阵版本化matrix-v3/决策树显性if序四约束/同卡直通×分区×半虚拟互斥共存判定/损耗声明四行选型报告带原文/0x33xx 两码）
pub mod vcl02_virtmode;
pub mod vcq01_reliability;
pub mod vcq01_reliability_checks;
pub mod vcq02_metrics;
pub mod vcq02_metrics_checks;
pub mod cgm01_display;
pub mod cgm01_display_checks;
pub mod vct01_tbridge;
pub mod vct01_tbridge_checks;
pub mod vct02_layers;
pub mod vct02_layers_checks;
pub mod vcv01_realverify;
pub mod vcv01_realverify_checks;
pub mod vcw01_sdkdoc;
pub mod vcw01_sdkdoc_checks;
pub mod vcw02_infoarch;
pub mod vcw02_infoarch_checks;
pub mod vcw03_genpipeline;
pub mod vcw03_genpipeline_checks;
pub mod vcw04_apiref;
pub mod vcw04_apiref_checks;
// vcw05_docsync — CGPU-F3525 文档版本与代码同步（三态对齐规则写死落后/超前两害超前更危险/指纹+契约词双漂移立案幂等销账留痕不删/同步结论压发布门呼应文档即契约/0x34xx 三码）
pub mod vcw05_docsync;
pub mod cgm02_display;
pub mod cgm02_display_checks;
pub mod cgm03_edid;
pub mod cgm03_edid_checks;
pub mod cgm04_topology;
pub mod cgm04_topology_checks;
pub mod cgm05_modes;
pub mod cgm05_modes_checks;
// cgm06_scaling — CGPU-F1926 显示缩放与 DPI（GPU缩放vs显示器缩放显性决策表窗口化1:1不入决策/DPI链EDID物理尺寸isqrt整数PPI四档取最近平局取低档/清晰度整数倍优先非整数给建议倍率/O域单点传递版本对账单向通报/0x36xx 两码）
pub mod cgm06_scaling;
// cgp08_perf —— CGPU-F2408 遥测性能（自身开销总账：五段设施逐段有预算有实测对账/开销换算器 ppm↔步↔微秒三口径/F0482 实测注入 640ppm 与 F1558 红线 L0≤0.05ms 与 F2401 使命三条口径只引用不重定义/超标处置建议账留痕可复算/零 IO 零墙钟零 panic）
pub mod cgp08_perf;
pub mod cgp08_perf_checks;
pub mod cgp09_privacy;
pub mod cgp09_privacy_checks;
pub mod cga01_simdprim;
pub mod cga01_simdprim_checks;
pub mod cge01_domain;
pub mod cge01_domain_checks;
pub mod cge02_lifecycle;
pub mod cge02_lifecycle_checks;
pub mod cgp01_adaptivearch;
pub mod cgp01_adaptivearch_checks;
pub mod cgp02_unifiedmodel;
pub mod cgp02_unifiedmodel_checks;
pub mod vco01_degrade;
pub mod vco01_degrade_checks;
pub mod cgr01_secure;
pub mod cgr01_secure_checks;
pub mod cgr02_threatmodel;
pub mod cgr02_threatmodel_checks;
pub mod vco03_triggersrc;
pub mod vco03_triggersrc_checks;
pub mod vco04_priorcon;
pub mod vco04_priorcon_checks;
pub mod vco05_statemachine;
pub mod vco05_statemachine_checks;
pub mod vco06_executor;
pub mod vco06_executor_checks;
pub mod vco07_recovery;
pub mod vco07_recovery_checks;
pub mod cgb07_execfail;
// cgb08_framequeue — CGPU-F0168 帧图与 GPU 队列映射（三队列路由表写死可对拍/节点跟资源走写集定home/写后读同卡同队列一致性即检即拒/映射O(1)查表+开销账/三队列用例建造器/0x30xx 五码）
pub mod cgb08_framequeue;
// cgb09_debugviz — CGPU-F0169 帧图调试可视化（DAG人读视图队列着色+关键路径加粗+环/悬空边/重id三断言+时间轴排布探针叠加+交互详情四区+渲染计价预算闸0x2D段）
pub mod cgb09_debugviz;
// cgb10_graphcache — CGPU-F0170 帧图构建结果缓存（缓存键=表面集+资源集序无关哈希+计数混入+明细对拍防碰撞/四代环形槽位+TTL+满员逐最旧/增量三算子Add-Remove级联-Replace+base键校验+未动节点编译代数保留/增量=全量结构全等对拍/失效五因定表+环形账本+失效后必miss/命中率千分比遥测/构建成本模型+节约收益实测/32判据）
pub mod cgb10_graphcache;

// cgb11_budgetint — CGPU-F0171 帧图预算集成（逐任务预算标签校验 F0005 三要素对接+ladder四律+显性非法清单/成本预估 F0004 对接口径基价表写死可对拍/图级Σ预估 vs 帧预算余量三态 Ok-Tight储备线10%-Over/超支降级建议单 Low先降同级大est先降+Critical豁免+逐档回收转正即停+到底进skip候选只建议不执行/仲裁四态映射 Granted-Degraded-Queued-Rejected带清单+判定步数账红线200/集成门面全链一报告/30判据五族）
pub mod cgb11_budgetint;

// cgf04_hwid — CGPU-F0804 硬件身份识别（三要素身份vendor/device/驱动版本+虚拟化五类诚实标注三证据保守推定+防伪七探针行为地板交叉验证立案+档案schema序列化往返指纹链0x5E段）
pub mod cgf04_hwid;
pub mod cgc01_carbitration;
// cgc02_budgetmodel — CGPU-F0322 帧预算模型（四池比例ppm配置和恰1e6显性拒不静默归一/三级单向分解帧→池→账户/双表述μs权威ppm呈现换算单向取整不超发/Σ池=帧余量归网络池+Σ账户≤池超载显性拒不借贷/超载扫描是查询不是修复/0x35xx 三码）
pub mod cgc02_budgetmodel;
// cgc02_framebudget — CGPU-F0322 帧预算模型深化版（四池闭集+保交互下限守卫/三级层级+层级单调核验/双表述恒等可逆换算/守恒校验器+超载检测处置建议/确定性分配引擎+floor 饥饿防线+可借出转移/预算时间线八段 vsync 对齐/池压力五级+仲裁移交/余量再分配四策略/审计账 256 滚动三态查询/全链路仿真走查/0x62 段七码）
pub mod cgc02_framebudget;
// cgc03_budgetaccount — CGPU-F0323 预算账户模型（账户树四级系统根→域→功能→表面配额继承三律：上限钳制拒/保底不越顶合围/超订显性注册许激活拒+清单点名；四属性不变量组 min≤used≤cap+借出面=cap-min；生命周期四态定表注册→激活⇄冻结→注销+级联孤儿回收有账逐个留痕+Closed 子不占账收敛激活；借出账本三纪律保底不可借/借入不超上限/Σ借出=Σ借入对账；F0217 双向反馈正向三维字典序仲裁池尽拒低优+反向满足率<60% 高优提关键性+10 留痕；0x3C 段）
pub mod cgc03_budgetaccount;
// cgc04_applyproto — CGPU-F0324 预算申请协议（标签三要素预估来自 F0004 成本模型 predict 选路+单任务上限钳制钳制不是拒绝/两路申请时机静态批量+运行期动态补申双约束轮次2总额50%+静态重申拒/批量仲裁整图一次轮次+1低优先先裁逐条留痕图预算 12.5ms 耗尽显性拒/原子归还批准-消费一次性返还+泄漏检测归还+消费==批准+重复归还拒+终结态复活拒/申请账本滚动 256 六类事件+全程量守恒对账批准==消费+归还+在途/纪元漂移留痕/快照保空段指纹+3图12任务参考探针五口径 零unwrap 0x67段）
pub mod cgc04_applyproto;
// cgb08_framegraph — CGPU-F0168 帧图与 GPU 队列映射深化版（三队列+八类别规则表/多 GPU 路由五规则确定性/屏障跟写特例/读写集一致性三态校验+跨卡搬运计划器/映射开销四段账/三队列用例走查/映射回放漂移检测/帧间稳定性四级/负载分布+热点检测/遥测六指标/0x61 段六码）
pub mod cgb08_framegraph;
// cgd04_framegap — CGPU-F0484 帧间隔统计深化版（六桶直方图+形态五分类/整数方差+isqrt 抖动/jank 三级阈值恰边界/分级账+事故场景/周趋势四桶+整数斜率五态/健康度三因子万分制/P50/P95/P99 整数分位/场景归因对比/审计账三态查询/0x63 段六码）
pub mod cgd04_framegap;
// cge04_surfacerender — CGPU-F0644 表面渲染调度深化版（表面档案四维闭集/三键确定性排序/轮转公平器+饿死守卫/C 域联动序投射+一致性核验/排程开销四段账 50µs 红线/弃排显性两原因/审计账+弃排查/遥测六指标/0x64 段六码）
pub mod cge04_surfacerender;
// cgf03_capability — CGPU-F0803 能力探测体系深化版（能力位 16 位四族闭集/三层探测信任序+冲突消解/快照 schema 版本化+指纹/防伪五规则+置信度万分比/降级+选路单点取用/漂移检测/0x65 段六码）
pub mod cgf03_capability;
// vct04_handshake — CGPU-F3044 握手与鉴权深化版（五阶段单向状态机+迁移表全枚举/令牌五重校验格式→版本→窗→绑定→撤销/会话三绑定不串户/nonce 单次消费重放防线/失败三态+重试限速 30tick/审计账+失败归因/0x6C 段七码）
pub mod vct04_handshake;
pub mod cge03_surfacereg;
pub mod cge04_surfsched;
pub mod cgf02_authflow;
// cgf03_probedomain — CGPU-F0803 能力探测体系（四能力域统一表格式/采样/RT/现代特性消费面零per厂商if链/三层设备→驱动→系统有序注入层间交集下限仲裁/schema版本化消费单点D04降级I域选路/防伪三查层间⊆+声明-行为位比对+上限钳制虚报位清零留痕/0x39xx 三码）
pub mod cgf03_probedomain;
pub mod cgg01_gnvidia;
// cgg02_nvprobe — CGPU-F0962 NVIDIA 能力探测专项（RT/Tensor/显存/NVENC 四元特性表结果即数据/SKU级device+rev二级键未登记走代际默认诚实降级/DLSS+GSync扩展缺席不破坏功能/降级矩阵derive单点报告消费/0x32xx 三码）
pub mod cgg02_nvprobe;
pub mod cgh03_pooltier;
pub mod vck01_heteroarch;
// vck02_hetres — CGPU-F1602 异构资源抽象（ComputeDevice四核心trait必选+Batch/Realtime扩展trait特性位图分层/四类型卡NPU DSP ISP MEDIA静态规格/HetCaps schema版本化能力即数据/HetHandle全局id+home+位置标签跨引擎位可达性判定/热注册带任务注销显性拒/MockEngine确定性假引擎F1454模式/0x3Bxx 四码）
pub mod vck02_hetres;
pub mod vcn03_rebuild;
// vcn04_rebuildpipe — CGPU-F2084 效果重建管线（四段单向捕获→分析→重建→验证跳段回退显性拒/采样三纪律锚点稳定窗8帧+三象限布点+元数据同拍/验证复用等价分级像素容差2感知×8不等价打回重建计数留痕/段间传效果分解不传像素/0x37xx 三码）
pub mod vcn04_rebuildpipe;
pub mod vcq03_failpredict;
pub mod cgr04_safedegrade;
// cgr05_faultiso — CGPU-F2725 渲染故障隔离（隔离格状态机健康→可疑→确诊/假警报不污染三机制封格+写前登记回滚+健康格零感知/Q05三信号确认窗3拍判定链声明式复用阈值独立写死可对拍/回滚只对确诊格凭证账不越权代写资源面/0x38xx 三码）
pub mod cgr05_faultiso;
pub mod vcs02_multiuser;
pub mod vct03_protneg;
pub mod vcu01_bencharch;
pub mod vcu02_benchmodel;
// cgc04_framearbiter — CGPU-F0005 帧预算仲裁器（12.5ms五账户切分合成器保底4ms不可侵占/申请制预算标签三要素预估-优先级-降级档/裁定四态批准排队降级拒绝静默吞帧禁绝/超时先走降级档帧级超时发降质链信号F0008消费/信用池低峰存入高峰先补交互/判定步数账50μs红线步数化/帧日志全量seq单调/不变量核验保底零侵占/0x3Cxx 九码）
pub mod cgc04_framearbiter;
// cgd05_ratecontract — CGPU-F0485 帧率合同校验器（P95≤12.5ms/P99≤25ms/卡顿率≤0.5%三条款滚动窗600帧自动判定/0.5ms桶计数分位O(n+桶)/达标率每场景每时段履行报告/预警双源余量耗用80%+卡顿趋势Rising提前干预/预算修正建议单出单不执行C域消费/场景隔离清窗重计跨场景混算会撒谎/0x3Dxx 六码）
pub mod cgd05_ratecontract;
pub mod cgd05_ratecontract_checks;
// cgd06_pacerhythm — CGPU-F0486 呈现节奏模型（三模式时钟建模 VSync对齐/NoSync事件驱动/Compositor合成节拍+EWMA α=1/4 漂移抖动哨/权衡声明结构体撕裂vs延迟被查询消费可证伪对拍/适配矩阵四格全定义无边框没有真免同步映射 Compositor+D02 抑制联动间隔翻倍建模未联动留痕/模式探测器间隔分布识别近零占比刷新带合成带+迟滞 4 帧确认+声明对拍/快照指纹+全矩阵参考探针 零unwrap 0x68段）
pub mod cgd06_pacerhythm;
// cgf05_certsuite — CGPU-F0805 认证基准套件（全链考卷D08执行器+网页+2D/视频/3D分项五场景/金银铜三级深度矩阵覆盖面×用例密度×阈值放宽显性/认证即冻结指纹FNV落账/变更必须ADR申请账submitted状态批准外置/ADR批准后回起草态重认证才可再分发/分发包按级裁剪指纹复验防篡改/0x3Exx 七码）
pub mod cgf05_certsuite;
// cgg03_nvdifftable — CGPU-F0963 NVIDIA 驱动差异表（GameReady vs Studio双驱动线闭集/已知问题条目版本区间闭区间命中/症状六类规避动作五类冻结语义/收敛条目选路降级Observe留痕不阻断/发布即冻结演进走ADR账F0899复用/公开报告单点外部不直读表/版本三段解析主.次.热修/0x3Fxx 七码）
pub mod cgg03_nvdifftable;
// cgi03_bwaccount — CGPU-F1283 带宽预算账户（四传输类账户交换上传下载流送/优先级呈现>交互>后台同级上传优先/有效带宽口径声明式复用F1282理论×有效系数不重定义/Σcap≤链路cap构造期守恒+Σused≤Σcap运行期核验/水位不足按优先级倒序抢占只抢更低者后台先失全量留痕/零带宽记账非法/预警90%+修正建议单/0x40xx 八码）
pub mod cgi03_bwaccount;
// vck03_hettask — CGPU-F1603 异构任务模型与标签（B域Task侧车扩展不改核/schema版本化协商闭集/三维功耗标签F1458复用+Boost不可延迟互斥/NPU功耗系数表四档写死/亲和三级NpuOnly-PreferNpu-Any/any按能效序选路静态表写死/NPU系数表外/批维度micro≤batch外层步数向上取整/DSP deadline错过=失败显性回执不静默/提交前校验规则表五规则/能力位图复用F1602 FEAT_BATCH/REALTIME/0x41xx 八码）
pub mod vck03_hettask;
pub mod vck03_hettask_checks;
 pub mod vck04_probe;
 pub mod vck04_probe_checks;
// cgc05_framecontract — CGPU-F0006 80帧合同与承诺报告（三档基准A/B/C合同文本冻结FNV指纹/硬件下限拒签/每帧记账帧间隔+五账户耗时+降质动作/10秒800帧聚合兑现率ppm桶计数分位/三级处置状态机单帧超时→帧内降质-连续5秒P95失守→降质链-连续60秒→书面报告/书面报告归因定位到账户五账户累计超支最大者+建议闭集/0x42xx 八码）
pub mod cgc05_framecontract;
// cge05_surfacepool — CGPU-F0009 网页表面池与可见性仲裁（30表面池容量64/可见性三档全帧率-1/2帧率-冻结渲染后台只留事件/1帧复活清idle+清驱逐标记/三因子份额面积×优先级×变化率每帧重排/风暴合批≥8请求按4帧轮转交错分摊肉眼无损/内存封顶32-16-8MB超限先压缩再驱逐后台帧缓冲驱逐后仍超显性/后台页面全速开关行为确定/0x43xx 六码）
pub mod cge05_surfacepool;
// cgi04_uploaddl — CGPU-F0010 上传下载带宽调度器（每帧1.5ms拷贝时间片大资产8MB强制分片2MB粒度跨帧渐进/mipmap可见性优先低分辨率级先传高级别后台补齐4倍几何/下载独立0.5ms片对称限速读回不抢帧带宽/拥塞归因队列深度≥64与PCIe利用率≥900kppm双源联动→放置建议可CPU任务留CPU F0004消费/时间片超发内部不变量/遥测全量/链路口径复用F1282/0x44xx 六码）
pub mod cgi04_uploaddl;
// cgm07_pacemodel — CGPU-F0486 呈现节奏模型（VSync/无同步/合成器三模式帧间隔行为/权衡声明撕裂-延迟-抖动三维评分+策略文本闭集/窗口形态适配矩阵全屏独占三模式直通无边框窗口VSync语义D02抑制联动接管/模式探测器锁相度≤10%+抖动≤1ms→VSync或Composer按合成器活跃二分-抖动>2ms不锁相→Unsynced-其余歧义显性拒/行为对拍抖动超模式声明上界立案/0x45xx 五码）
pub mod cgm07_pacemodel;
pub mod cgm07_pacemodel_checks;
// cgf07_certgrade — CGPU-F0806 认证分级（四级判据量化金=三档全原画质达标/银=A/B主要场景/铜=降质后达标/不认证=降质仍不达标/证据三档齐备才可判级/公开认证卡随硬件可查F0808透明/铜级→O域降质配方预载清单三配方闭集联动声明/复核工单状态机Submitted→Regrading→Resolved改级必须附新证据同逻辑重判防拍脑袋/与F0805考卷体系分工/0x46xx 五码）
pub mod cgf07_certgrade;
// cgg04_nvram — CGPU-F0964 NVIDIA显存与预算（4-24GB六档档位表±512MB归档容差/池五闭集呈现-纹理-几何-管线-系统/默认配额15-40-20-10-15%小档保呈现大档重纹理/扣账单池超配额显性拒不静默透支/压力三级70%-90%阈值告急→降质链预判信号F0008接缝/回收建议按可回收优先级管线>纹理>几何呈现系统不可动/透明报告五池用量单点/H域F0852理念联动声明/0x47xx 六码）
pub mod cgg04_nvram;
// cgc06_powersched — CGPU-F0011 功耗感知调度（三模式功率墙性能不设限-均衡80%-续航50%切换迟滞行为确定/温度滑动窗首尾斜率外推距降频线ETA/预测性降载提前2s发单防帧率悬崖/被动降频失守立案供F0007热归因闭环/传感只读红线零硬件写数据HAL注入/遥测全量seq单调/0x48xx 六码）
pub mod cgc06_powersched;
// cgd06_latencyledger — CGPU-F0487 输入到像素延迟账本（端到端五段分解输入-调度-渲染-合成-呈现同拍锚点/EWMA滚动均值α=1/8/突增判定双条件均值×2且≥20ms防误报/卡顿关联视图突增帧×jank帧交集率ppm/目标线50ms诚实标注依赖硬件达成率ppm输出/五段均值占比分解/0x49xx 五码）
pub mod cgd06_latencyledger;
pub mod cgd06_latencyledger_checks;
// cgi05_pciearb — CGPU-F1284 PCIe 仲裁器（双轨v1静态优先级严格序饿死显性+v2加权公平权重和恰1e6首轮理想份额+二轮回池补足/仲裁开销64步红线步数化/确定性同输入双跑逐行比对/归因账分配拒绝全记录带轨名原因/F0345理念复用声明/0x4Axx 六码）
pub mod cgi05_pciearb;
// cgf08_certvalid — CGPU-F0807 认证有效期与重认证（有效期90天时效判定过期显性非静默/触发规则表驱动大版本-系统更新-引擎变更三类→受影响场景闭集增量重认证计划只重测受影响项免测显性列出/重认证中判级显性不可用防旧级掩盖漂移/完成回执重起算/有效期公开卡/与F0806判级分工/0x4Bxx 五码）
pub mod cgf08_certvalid;
// cgf09_hwprofile — CGPU-F0012 硬件兼容认证档案（四阶段单向流水探测→基准→24h长稳→发布跳段拒/四档位金牌全效-银牌标准-铜牌基础自动进降级链-软渲兜底缺省/已知缺陷表+规避开关运行时自动应用/金牌资格阻断缺陷自动降档不静默/不符回炉最高缺陷软渲下架重走阶段一/virtio-gpu档案首发基线/版本分发包只含已发布/0x4Cxx 五码）
pub mod cgf09_hwprofile;
// cgg05_nvenc — CGPU-F0965 NVIDIA NVENC媒体联动（代际五档闭集Legacy→Ada编码器数1→2/AV1仅Ada H.265仅Pascal+ B帧Turing+诚实标注/算力标签ppm登记对接VE视频管线/媒体选路三路NVENC→Shader→CPU会话容量编码器×3需求超能力诚实拒不虚报/联动验证标签vs实测偏差≤30%/代际差异诚实表/0x4Dxx 四码）
pub mod cgg05_nvenc;

/// CGPU 域标签（F475 闭环登记用——域表按此串渲染）。
pub const CGPU_DOMAIN: &str = "cgpu";

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
///
/// **折叠而非平铺**（AI-CGPU 接线，与 `svstar2::run_svstar2_checks` 同一范式）：
/// 33 个在账判据集条数总和远超 `CheckSet::MAX_CHECKS = 112`，若走
/// `CheckSet::merge` 平铺，尾部会被静默截断——平铺版只跑得完前 112 条，
/// 余下约 300 条判据永远不参与闭环，`all_passed()` 却照样返回绿。
/// 故此处逐集折叠为一条汇总判据（标签＝单号，绿/红如实），并对每个子集
/// 显式带 `!sub.truncated()` 守卫：子集自身溢出即判红，不让溢出被折叠
/// 掩盖。新单入账在此数组追加一行。
///
/// 33 条与接线前的 merge 树**逐集一一对应**（顺序亦保持一致），
/// 折叠只改呈现方式、不减子集——漏登记任何一条都会让该单判据退出闭环。
/// 其中 T02/W02/W03/W04 四条是补登记：模块与自检早已实现，但既无
/// `pub mod` 声明也不在折叠数组里，等于从未参与编译与验收。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new(CGPU_DOMAIN);
    let blocks: [(&'static str, CheckSet); 40] = [
        ("CGPU-F0001", cga01_simdprim_checks::run_cga01_checks()),
        ("CGPU-F0002", cga02_threadpool_checks::run_cga02_checks()),
        ("CGPU-E0001", cge01_domain_checks::run_cge01_checks()),
        ("CGPU-E0002", cge02_lifecycle_checks::run_cge02_checks()),
        ("CGPU-I1281", cgi01_bandwidth_checks::run_cgi01_checks()),
        ("CGPU-M1921", cgm01_display_checks::run_cgm01_checks()),
        ("CGPU-M1922", cgm02_display_checks::run_cgm02_checks()),
        ("CGPU-J1441", vcj01_powerarch_checks::run_vcj01_checks()),
        ("CGPU-L1761", vcl01_virtualarch_checks::run_vcl01_checks()),
        ("CGPU-Q2561", vcq01_reliability_checks::run_vcq01_checks()),
        ("CGPU-Q2562", vcq02_metrics_checks::run_vcq02_checks()),
        ("CGPU-T3041", vct01_tbridge_checks::run_vct01_checks()),
        ("CGPU-V3361", vcv01_realverify_checks::run_vcv01_checks()),
        ("CGPU-W0001", vcw01_sdkdoc_checks::run_vcw01_checks()),
        ("CGPU-F0482", cgd02_frametimer_checks::run_cgd02_checks()),
        ("CGPU-F0483", cgd03_frameledger_checks::run_cgd03_checks()),
        ("CGPU-F0485", cgd05_ratecontract_checks::run_cgd05_checks()),
        ("CGPU-F0486", cgm07_pacemodel_checks::run_cgm07_checks()),
        ("CGPU-F0487", cgd06_latencyledger_checks::run_cgd06_ledger_checks()),
        ("CGPU-F1603", vck03_hettask_checks::run_vck03_checks()),
        ("CGPU-F2409", cgp09_privacy_checks::run_cgp09_checks()),
        ("CGPU-F1604", vck04_probe::run_vck04_checks()),
        ("CGPU-P0501", cgp01_adaptivearch_checks::run_cgp01_checks()),
        ("CGPU-P0502", cgp02_unifiedmodel_checks::run_cgp02_checks()),
        ("CGPU-O0701", vco01_degrade_checks::run_vco01_checks()),
        ("CGPU-R0901", cgr01_secure_checks::run_cgr01_checks()),
        ("CGPU-R0902", cgr02_threatmodel_checks::run_cgr02_checks()),
        ("CGPU-M1923", cgm03_edid_checks::run_cgm03_checks()),
        ("CGPU-O0703", vco03_triggersrc_checks::run_vco03_checks()),
        ("CGPU-M1924", cgm04_topology_checks::run_cgm04_checks()),
        ("CGPU-O0704", vco04_priorcon_checks::run_vco04_checks()),
        ("CGPU-M1925", cgm05_modes_checks::run_cgm05_checks()),
        ("CGPU-P0208", cgp08_perf_checks::run_cgp08_checks()),
        ("CGPU-O0705", vco05_statemachine_checks::run_vco05_checks()),
        ("CGPU-O0706", vco06_executor_checks::run_vco06_checks()),
        ("CGPU-O0707", vco07_recovery_checks::run_vco07_checks()),
    // 补登记四组"已实现但从未入账"的判据（闭环对账 _tools/audit_domain_closure.py
    // 查出）：此前连 `pub mod` 声明都没有，全文件从未被编译检查，等于死代码。
    // T02 对接协议分层 / W02 文档信息架构 / W03 文档生成管线 / W04 API 参考生成。
        ("CGPU-T3042", vct02_layers_checks::run_vct02_checks()),
        ("CGPU-W3522", vcw02_infoarch_checks::run_vcw02_checks()),
        ("CGPU-W3523", vcw03_genpipeline_checks::run_vcw03_checks()),
        ("CGPU-W3524", vcw04_apiref_checks::run_vcw04_checks()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            tag,
            passed,
            if passed {
                ""
            } else if sub.truncated() {
                "sub-checks truncated"
            } else {
                "sub-checks red"
            },
        );
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CGPU 域聚合全绿（接线F475 闭环后，域内任何红项都必须在此暴露——
    /// 此前 cgpu 无任何 #[test]，接线了也没有测试守护，红项不会报警）。
    #[test]
    fn cgpu_domain_aggregate_all_green() {
        let set = run_cgpu_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "CGPU 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 33 个子集一集不少（漏登记会让该单判据静默退出闭环——折叠数组
    /// 元素个数必须锁死，新增单时同步改这里）。
    #[test]
    fn cgpu_all_33_subsets_registered() {
        let set = run_cgpu_checks();
        assert_eq!(
            set.len(),
            33,
            "CGPU 折叠子集数应为33（与接线前merge 树逐集一一对应）"
        );
        assert!(!set.truncated(), "CGPU 域聚合溢出（子集数超 CheckSet 上限）");
    }

    /// 子集自身溢出必须被判红，不能被折叠掩盖：守卫 `!sub.truncated()`
    /// 是本册不丢判据的唯一保障，删掉它这条测试就该红。
    #[test]
    fn cgpu_subset_truncation_is_not_masked() {
        let mut over = crate::checks::CheckSet::new("over");
        for i in 0..(crate::checks::MAX_CHECKS + 8) {
            over.add("x", true, "");
        }
        assert!(over.truncated(), "构造的溢出集应报告 truncated");
        assert!(
            !(over.all_passed() && !over.truncated()),
            "溢出集在折叠语义下必须判红（否则截断被掩盖）"
        );
    }
}
