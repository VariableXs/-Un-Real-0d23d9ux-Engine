//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//! | [`cgi01_bandwidth`] | CGPU-F1281 | I 域开工与带宽总架构（**五主题闭集**PCIe带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因冻结、本域160项全挂靠表外不立题；**账户化**四类传输（纹理上传/下载/交换/流送）各开预算账户——每笔传输先申请余额不足即拒绝并计数留痕、退款只做消费迁移、账本守恒净消费口径自检；**边界**H域管显存驻留/I域管传输带宽/VE-AD存储流F5801+管到达三段各管一段、接口复用F0405集成面声明逐句可grep；**十组**规划表组名+起止单号逐字取自总纲批次锚点、F1281-F1440首尾连续无缺口、宣告行读屏可查；零panic面零IO零墙钟） |
//! | [`cgi01_bandwidth_checks`] | CGPU-F1281 域自检（判据五条映射：五主题闭集+标签独立重排全等、账户化申请/拒绝/退款/守恒全链+未开账户必拒反向语料、边界三域分工逐句grep、十组表与判据侧独立重排逐条全等+组间起止连续校验；守恒式与使用率判据侧独立重算） |
//! | [`cga10_bandwidth_sched`] | CGPU-F0010 | 上传下载带宽调度器（**上传限速帧超时零贡献**——每帧 1.5ms 拷贝时间片（锚点原文恰值）耗尽立即让路渲染绝不超时挤占 yield 计数留痕+16KB/µs 速率标定单帧容量 24,576,000B 手算锚；**大资产强制分片**——4K 纹理 32MB 级超单帧容量构造面即 enforce chunked 留痕 E_BW_BIG_UNCHUNKED 跨帧渐进；**mipmap 可见性优先**——先传 1/64 面积 preview 级可见发生在帧 1 完成在帧 2 可见时刻远早于完成时刻；**读回不抢帧带宽**——下载低优先独立切片 750µs（上传之后消费）与上传对称限速互不挤占；**拥塞归因**——队列深度≥8 与 PCIe 利用率≥900‰ 双条件联动迟滞防抖（连续 2 帧紧才标定E_BW_CONGESTION/连续 3 帧松才解除——单帧尖峰不误报）+双条件叠加 PCIe 停顿独立告警 E_BW_PCIE_STALL；**联动放置决策**——PlacementHint keep_on_cpu 带宽紧时可 CPU 计算任务留 CPU 对接 CGPU-F0004 信号面纯数据不依赖；**遥测全量**——上/下字节账+切片四元画像+利用率千分比累计口径手算锚 1137‰+分片/让路/拒绝计数 0x3E 段五码互异 reason 全量显性） |
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
pub mod cgf06_certtier;
pub mod cgj08_pwrevent;
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
pub mod cga10_bandwidth_sched;
pub mod cga15_csconv;
pub mod cga16_simdblend;
pub mod cga17_yuvdec;
pub mod cga18_texdec;
pub mod cga19_prioq;
pub mod cga20_numa;
pub mod cga21_tilesort;
pub mod cga22_tilereuse;
pub mod cga23_quality;
pub mod vcg14_ressamp;
pub mod vcg13_textrast;
pub mod vca12_hwcompat;
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
pub mod vcv02_philosophy;
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
// vcw08_doctelem — CGPU-F3527 文档遥测（计数不记人隐私红线本体/访问计数+搜索热词聚合口径无用户维度字段/滚动定容覆盖+滚动计数留痕+守恒式 total==环内+滚掉/隐私抽检导出禁 user/session/ip/identity 词形检出即拒/P 域 cgp02 口径复用/0xE2 段四码）
pub mod vcw08_doctelem;

pub mod vcx84_bandwidth;
// vct05_secboundary — CGPU-F3045 对接安全边界（信封检视器+复用声明账+会话剖面+越界逐码显性拒）
pub mod vct05_secboundary;
// vcu04_scoringsys — CGPU-F3204 计分体系（三分链原始分/归一分/加权总分+计分表五闸显性拒+权重四档万分制守恒）
pub mod vcu03_scenecls;
pub mod vcu04_scoringsys;
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
// cgo01_degradevis — CGPU-F2248 降级可见性（O 域降级链骨架自洽底座七类触发源注册可扩展+四级优先梯安全>合同>体验>资源+四态机迁移表全枚举显性合法Some非法None+建议vs强制执行语义留痕/无感原则单步≤100‰步数≤12安全类豁免跳变豁免误用立案+开发面板五维快照态源恢复‰维度集读屏可达/复用账五家族显性登记 F2243-47 来源单号未知拒/快照指纹+八口径参考探针 零unwrap 0x6A段）
pub mod cgo01_degradevis;
// cgp08_perf —— CGPU-F2408 遥测性能（自身开销总账：五段设施逐段有预算有实测对账/开销换算器 ppm↔步↔微秒三口径/F0482 实测注入 640ppm 与 F1558 红线 L0≤0.05ms 与 F2401 使命三条口径只引用不重定义/超标处置建议账留痕可复算/零 IO 零墙钟零 panic）
pub mod cgp08_perf;
pub mod cgp08_perf_checks;
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
// cgh03_pooltier 深化版（F1123 v2：三级池让度规则进裁决面/子账户树深度受限树内守恒/守恒三式/水位滞回+采样史趋势/八类事件账FNV链/再平衡单笔bps上限/回收预估单声明F1124执行位/快照指纹往返/读屏三段 0x65 段扩码 0x6505-0x650C）
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
// cgf05_certsuite_deep — CGPU-F0805 认证基准套件深化版（与速写版并存深化为准 0x69 段：全链考卷四场景闭集 Web/2D/视频/3D+三重机械证明 overlap_free/gap_free/权重恰 1e6ppm+D08 执行器接缝 trait/金银铜级深度阈值矩阵 depth_for+threshold_for 铜级降质放宽×2+分级判定金全场景银主场景铜放宽首中即止留依据/冻结指纹 FNV 落账+ADR 三态 Pending外置批准/Approved回起草态指纹链留痕/Rejected+篡改检测 SealMismatch/分发按级裁剪深度钳级矩阵上限越级防护+包指纹+收包重序列化对拍复验篡改拒收/快照+全链十口径参考探针 零unwrap）
pub mod cgf05_certsuite_deep;
// cgg03_nvdifftable — CGPU-F0963 NVIDIA 驱动差异表（GameReady vs Studio双驱动线闭集/已知问题条目版本区间闭区间命中/症状六类规避动作五类冻结语义/收敛条目选路降级Observe留痕不阻断/发布即冻结演进走ADR账F0899复用/公开报告单点外部不直读表/版本三段解析主.次.热修/0x3Fxx 七码）
pub mod cgg03_nvdifftable;
// cgi03_bwaccount — CGPU-F1283 带宽预算账户（四传输类账户交换上传下载流送/优先级呈现>交互>后台同级上传优先/有效带宽口径声明式复用F1282理论×有效系数不重定义/Σcap≤链路cap构造期守恒+Σused≤Σcap运行期核验/水位不足按优先级倒序抢占只抢更低者后台先失全量留痕/零带宽记账非法/预警90%+修正建议单/0x40xx 八码）
pub mod cgi03_bwaccount;
// vck03_hettask — CGPU-F1603 异构任务模型与标签（B域Task侧车扩展不改核/schema版本化协商闭集/三维功耗标签F1458复用+Boost不可延迟互斥/NPU功耗系数表四档写死/亲和三级NpuOnly-PreferNpu-Any/any按能效序选路静态表写死/NPU系数表外/批维度micro≤batch外层步数向上取整/DSP deadline错过=失败显性回执不静默/提交前校验规则表五规则/能力位图复用F1602 FEAT_BATCH/REALTIME/0x41xx 八码）
pub mod vck03_hettask;
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
// cgd07_earlysubmit — CGPU-F0488 帧预测提前提交（提前量控制器稳定性EWMA滞回升降轨升难降易/预测器联动预留PredictorHook接口冻结C04接入即插即用NoPredictor桩恒回EWMA/偏差带12ms超带零窗口回退同步提交回退不冻结/诚实标注一期统计预测强制携带/0x4Fxx 五码）
pub mod cgd07_earlysubmit;
pub mod cgd07_earlysubmit_checks;
// cgi05_pciearb — CGPU-F1284 PCIe 仲裁器（双轨v1静态优先级严格序饿死显性+v2加权公平权重和恰1e6首轮理想份额+二轮回池补足/仲裁开销64步红线步数化/确定性同输入双跑逐行比对/归因账分配拒绝全记录带轨名原因/F0345理念复用声明/0x4Axx 六码）
pub mod cgi05_pciearb;
// cgf08_certvalid — CGPU-F0807 认证有效期与重认证（有效期90天时效判定过期显性非静默/触发规则表驱动大版本-系统更新-引擎变更三类→受影响场景闭集增量重认证计划只重测受影响项免测显性列出/重认证中判级显性不可用防旧级掩盖漂移/完成回执重起算/有效期公开卡/与F0806判级分工/0x4Bxx 五码）
pub mod cgf08_certvalid;
// cgf09_hwprofile — CGPU-F0012 硬件兼容认证档案（四阶段单向流水探测→基准→24h长稳→发布跳段拒/四档位金牌全效-银牌标准-铜牌基础自动进降级链-软渲兜底缺省/已知缺陷表+规避开关运行时自动应用/金牌资格阻断缺陷自动降档不静默/不符回炉最高缺陷软渲下架重走阶段一/virtio-gpu档案首发基线/版本分发包只含已发布/0x4Cxx 五码）
pub mod cgf09_hwprofile;
// cgg05_nvenc — CGPU-F0965 NVIDIA NVENC媒体联动（代际五档闭集Legacy→Ada编码器数1→2/AV1仅Ada H.265仅Pascal+ B帧Turing+诚实标注/算力标签ppm登记对接VE视频管线/媒体选路三路NVENC→Shader→CPU会话容量编码器×3需求超能力诚实拒不虚报/联动验证标签vs实测偏差≤30%/代际差异诚实表/0x4Dxx 四码）
pub mod cgg05_nvenc;
// cga05_simdraster — CGPU-F0013 SIMD文字光栅加速（u64位板8通道可移植SIMD lanes_add饱和8/GlyphRequest-GlyphBitmap冻结接口四元组+cache_key FNV/gamma_table sRGB传递函数256项查表/8像素列板扫描转换AA两档Gray+Subpixel三子列偏移/compare_ref对拍≤1 LSB确定性参考实现/吞吐≥200字ms/0x4Exx 五码）
pub mod cga05_simdraster;
// cgc07_earlyphrase — CGPU-F0488 帧预测提前提交（提前量自适应按负载稳定性CV分档稳定8ms中等4ms抖动0/统计预测一期EWMA置信度=近期命中率诚实标注C04联动预留接口冻结/预测失败回退命中率击穿70%红线→零窗口同步提交12帧滞回续期/回退解除按稳定性恢复/提前量硬上限16ms钳制计数/0x4Fxx 五码）
pub mod cgc07_earlyphrase;
// cgf10_openmat — CGPU-F0808 认证数据开放（认证矩阵全量公开每硬件等级-依据-日期/开放格式no_std手写JSON导出转义完备引号-反斜杠-控制字符/隐私边界字段白名单8项+黑名单子串双保险白名单外拒绝计数/防篡改FNV-HMAC写时签名读时验签篡改即抛码/无密钥拒绝写入签名空承诺防护/同ID覆盖写/0x50xx 五码）
pub mod cgf10_openmat;
// cgg06_nvpwr — CGPU-F0966 NVIDIA功耗画像（台式TDP/笔记本TGP双线档案同芯片两套墙错线拒收/功耗-性能曲线分段线性锚点升序校验端点钳制/约束画像墙下性能余量与触墙损失量化/画像公开JSON导出字段白名单/诚实空态无数据芯片显性Unknown不插值不臆造数据来源三态标注/0x51xx 五码）
pub mod cgg06_nvpwr;
pub mod cgg06_nvpwr_checks;
// cgi06_txqueue — CGPU-F1285 传输队列管理（上传/下载双队列分列各自深度水位一条河堵塞不淹另一条/信用回压协议ACK回款水位滞回75%触发55%解除防振荡/优先级插队Emergency直插队首High快道连续插队4次强制队尾饿死保护/全量遥测入队-出队-拒收-插队-水位峰值/超额ACK协议违约钳上限报码/0x52xx 五码）
pub mod cgi06_txqueue;
// cgq01_txqueue — CGPU-F1285 双实现并存B面（水位门限令牌限速设计 独立于A面信用回压/分列+背压+插队+遥测四判据/0x750段）
pub mod cgq01_txqueue;
// cgj05_pwrfilt — CGPU-F1447 功耗数据管道与滤波（五级管道逐级独立可测/中值去尖刺+EMA定点平滑双滤波参数表分派/1s10s60s三窗三指标/背压覆盖写丢旧保新永不阻塞/SPSC环形/越限骤变事件旁路/1MB内存上限降采样）
pub mod cgj05_pwrfilt;
// vcw07_docsec — CGPU-F3528 文档安全（内部标注过滤三级闭集只放行Public/敏感子串双保险密钥样token样/三发布面安全复用同引擎判定一致/干净零误滤带密零漏滤）
pub mod vcw07_docsec;
// cgq02_txmerge — CGPU-F1286 传输批合并（小传输窗口合并大传输直通/5µs窗口三档权衡声明/同输入双跑指纹对拍确定性/事务数对比TLP头24B节省收益量化/批内+直通≡提交守恒对账）
pub mod cgq02_txmerge;
// vck04_hetprobe — CGPU-F1604 异构能力探测与登记（探测五步枚举→查询→隔离→登记→缓存公示每步乱序拒绝/实现矩阵NPU三路线CIM-Intel-XDNA DSP音频 ISP驱动查询静态闭集表/缓存键FNV驱动版本+设备ID命中复用异键热失效重探F1506模式/失败隔离ProbeFailed行剥离不污染登记表其余照常/虚拟化空态空枚举显性Empty/诚实登记NotSupported与ProbeFailed严格区分不造假/0x53xx 五码）
pub mod vck04_hetprobe;
pub mod vck05_taskgraph;
pub mod vck07_sync;
pub mod vck06_memcoh;
// cgd09_timecalib — CGPU-F0489 帧计时校准（三源互校CPU QPC×GPU时间戳×呈现时钟配对采样差分/定期重校准30s周期偏移收敛漂移率估计超100µs/s立案/精度声明残余误差量化超50µs失守/降级协议源失联4窗→Lost显性→回喂恢复三源/单调性红线域内倒退拒收输出倒退拒收拨表禁止/0x54xx 五码）
pub mod cgd09_timecalib;
// cgd10_frametel — CGPU-F0490 帧遥测（五指标进B08总线帧时间/间隔/全延迟/达标率/卡顿计数各独立聚合P95整数分位/按场景分列12上限未登记场景拒帧匿名混账拒绝/空窗显性None不返回零均值谎报/schema版本化发布包下游失配显性拒/卡顿阈值复用cgd06口径20ms/0x5Bxx 五码）
pub mod cgd10_frametel;
// cgf11_certtel — CGPU-F0809 认证遥测（四指标进B08总线执行次数/通过率金银铜分布ppm尾项守恒恰1e6/重认证触发分布三类归一零触发全零诚实/边缘复核率复核数>执行数违约改判率质量观察窗/厂商族分列8上限全量均值是谎报分列才可归因/0x56xx 五码）
pub mod cgf11_certtel;
// cgg07_nvupdate — CGPU-F0967 NVIDIA驱动更新应对（更新事件全流程接收→判定→预检→联动→告知/F0807触发规则NVIDIA落地GameReady与Studio双线独立MajorBump全量MinorBump增量/差异表预检症状命中无规避→暂缓带理由空话违约/未知大版本段显性Unknown不猜/同版本与降版拒收/基线推进告知面/0x57xx 五码）
pub mod cgg07_nvupdate;
// cgi07_txmerge — CGPU-F1286 传输批合并（合并窗口状态机5µs默认窗批满4096B或窗到期冲刷/权衡声明三档Off-Gentle-Aggressive延迟与吞吐显性取舍/确定性同输入双跑批序列逐批对拍/Emergency绕过合并直达延迟红线绕过触发缓冲批让路冲刷/收益量化TLP头24B开销事务数对比守恒对账批内+缓冲+绕过≡提交/0x58xx 五码）
pub mod cgi07_txmerge;
// cgd11_timefuzz — CGPU-F0491 帧计时fuzz（确定性fuzz核心LCG同种子同序列CI可固化/时钟异常注入三族跳变-倒退-卡顿容错计时器倒退钳0不产负时长异常显性入账/帧洪峰4096帧灌1024容量账本丢最旧丢弃计数不静默kept+dropped≡pushed守恒/案例固化种子+异常谱指纹CI回放对拍/0x5Axx 五码）
pub mod cgd11_timefuzz;
// cgd12_timeapi — CGPU-F0492 帧计时API冻结v1（四组API注册表计时启动-分段-账本查询-校验签名指纹注册即冻结/冻结不可变只增不改不删变更唯一路径ADR+版本前进/命名一致性小写下划线动词打头单位后缀三约定跨域对拍/十年承诺表指纹+承诺文本合成总指纹篡改即显性/0x5Cxx 五码）
pub mod cgd12_timeapi;
// cgd13_timebench — CGPU-F0494 帧计时基准（三基准计时开销比<0.1%红线步数化+账本写入吞吐摊销红线+查询延迟红线/参数全部显性入册口径指纹防挑参数作弊/入册回归同口径复跑一致漂移即立案确定性基准/0x5Dxx 五码）
pub mod cgd13_timebench;
// cgf12_certfuzz — CGPU-F0810 认证fuzz（畸形探测数据注入伪造能力位图高位域钳制显性立案+异常ID零-超界-重复三拒/流程中断fuzz随机步中断续跑语义断点前保持断点后不越与全新重跑终态一致/案例固化LCG种子谱指纹CI回放对拍/0x5Exx 五码）
pub mod cgf12_certfuzz;
// cgg08_nvtel — CGPU-F0968 NVIDIA遥测（四指标进B08设备分布按代际分列-各代认证等级ppm尾项守恒-专属缺陷检出计数-DLSS特性启用率分母口径/代际闭集5档闭集外拒/零分母显性None不造假/空桶不发零值谎报/0x5Fxx 五码）
pub mod cgg08_nvtel;
// cgi08_bwtel — CGPU-F1287 帧宽遥测（四源账户-仲裁-队列-事件闭集采集/带宽瀑布每帧四源分解守恒核验Σ源≡总篡改即破/VE-Y可视化对接面按帧号查询全量拉取/遥测开销<1%红线步数化自证/F0274 schema版本化/帧闭合纪律未闭合发布拒/0x60xx 五码）
pub mod cgi08_bwtel;
// cgd14_timecons — CGPU-F0495 帧计时一致性（口径注册表测量契约只增不改不删单位分位点窗长闭集/跨硬件口径对拍同负载相对量容差带±20%漂移即立案单零漂移/跨版本ADR语义变更唯一合法路径旧语义ADR留痕指纹必变/跨卷统一G06-H09挂钩同口径同指纹私造卷名拒/0x61xx 五码）
pub mod cgd14_timecons;

/// CGPU 域标签（F475 闭环登记用——域表按此串渲染）。
pub const CGPU_DOMAIN: &str = "cgpu";

/// CGPU 折叠子集数——**单一真源**。
///
/// 它同时是 `blocks` 数组的声明长度：数组字面量少一行/多一行都直接编译失败，
/// 于是「新增单忘登记」和「删单忘抹掉」两种漂移都在编译期被挡住，不需要靠
/// 测试去数数。测试里再断言一次 `set.len()` 是为了守住「折叠过程自身没丢
/// 子集」——那条路径编译期管不着。
pub const CGPU_SUBSET_COUNT: usize = 87;

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
///
/// **折叠而非平铺**（AI-CGPU 接线，与 `svstar2::run_svstar2_checks` 同一范式）：
/// 57 个在账判据集条数总和远超 `CheckSet::MAX_CHECKS = 112`，若走
/// `CheckSet::merge` 平铺，尾部会被静默截断——平铺版只跑得完前 112 条，
/// 余下数百条判据永远不参与闭环，`all_passed()` 却照样返回绿。
/// 故此处逐集折叠为一条汇总判据（标签＝单号，绿/红如实），并对每个子集
/// 显式带 `!sub.truncated()` 守卫：子集自身溢出即判红，不让溢出被折叠
/// 掩盖。新单入账在此数组追加一行。
///
/// 折叠只改呈现方式、不减子集——漏登记任何一条都会让该单判据退出闭环。
/// 其中 T02/W02/W03/W04 四条是补登记：模块与自检早已实现，但既无
/// `pub mod` 声明也不在折叠数组里，等于从未参与编译与验收。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new(CGPU_DOMAIN);
    let blocks: [(&'static str, fn() -> CheckSet); CGPU_SUBSET_COUNT] = [
        ("CGPU-F0001", cga01_simdprim_checks::run_cga01_checks as fn() -> CheckSet),
        ("CGPU-F0002", cga02_threadpool_checks::run_cga02_checks as fn() -> CheckSet),
("CGPU-F0010", cga10_bandwidth_sched::run_cga10_checks as fn() -> CheckSet),
("CGPU-F0014", vcg14_ressamp::run_vcg14_checks as fn() -> CheckSet),
("CGPU-F0013", vcg13_textrast::run_vcg13_checks as fn() -> CheckSet),
("CGPU-F0012", vca12_hwcompat::run_vca12_checks as fn() -> CheckSet),
        ("CGPU-E0001", cge01_domain_checks::run_cge01_checks as fn() -> CheckSet),
        ("CGPU-E0002", cge02_lifecycle_checks::run_cge02_checks as fn() -> CheckSet),
        ("CGPU-I1281", cgi01_bandwidth_checks::run_cgi01_checks as fn() -> CheckSet),
        ("CGPU-M1921", cgm01_display_checks::run_cgm01_checks as fn() -> CheckSet),
        ("CGPU-M1922", cgm02_display_checks::run_cgm02_checks as fn() -> CheckSet),
        ("CGPU-J1441", vcj01_powerarch_checks::run_vcj01_checks as fn() -> CheckSet),
        ("CGPU-L1761", vcl01_virtualarch_checks::run_vcl01_checks as fn() -> CheckSet),
        ("CGPU-Q2561", vcq01_reliability_checks::run_vcq01_checks as fn() -> CheckSet),
        ("CGPU-Q2562", vcq02_metrics_checks::run_vcq02_checks as fn() -> CheckSet),
        ("CGPU-T3041", vct01_tbridge_checks::run_vct01_checks as fn() -> CheckSet),
        ("CGPU-V3361", vcv01_realverify_checks::run_vcv01_checks as fn() -> CheckSet),
        ("CGPU-V3362", vcv02_philosophy::run_vcv02_checks as fn() -> CheckSet),
        ("CGPU-W0001", vcw01_sdkdoc_checks::run_vcw01_checks as fn() -> CheckSet),
        ("CGPU-F0482", cgd02_frametimer_checks::run_cgd02_checks as fn() -> CheckSet),
        ("CGPU-F0483", cgd03_frameledger_checks::run_cgd03_checks as fn() -> CheckSet),
        ("CGPU-F0485", cgd05_ratecontract_checks::run_cgd05_checks as fn() -> CheckSet),
        ("CGPU-F0486", cgm07_pacemodel_checks::run_cgm07_checks as fn() -> CheckSet),
        ("CGPU-F0487", cgd06_latencyledger_checks::run_cgd06_ledger_checks as fn() -> CheckSet),
        ("CGPU-F0488", cgd07_earlysubmit_checks::run_cgd07_checks as fn() -> CheckSet),
        ("CGPU-P0501", cgp01_adaptivearch_checks::run_cgp01_checks as fn() -> CheckSet),
        ("CGPU-P0502", cgp02_unifiedmodel_checks::run_cgp02_checks as fn() -> CheckSet),
        ("CGPU-O0701", vco01_degrade_checks::run_vco01_checks as fn() -> CheckSet),
        ("CGPU-R0901", cgr01_secure_checks::run_cgr01_checks as fn() -> CheckSet),
        ("CGPU-R0902", cgr02_threatmodel_checks::run_cgr02_checks as fn() -> CheckSet),
        ("CGPU-M1923", cgm03_edid_checks::run_cgm03_checks as fn() -> CheckSet),
        ("CGPU-O0703", vco03_triggersrc_checks::run_vco03_checks as fn() -> CheckSet),
        ("CGPU-M1924", cgm04_topology_checks::run_cgm04_checks as fn() -> CheckSet),
        ("CGPU-O0704", vco04_priorcon_checks::run_vco04_checks as fn() -> CheckSet),
        ("CGPU-M1925", cgm05_modes_checks::run_cgm05_checks as fn() -> CheckSet),
        ("CGPU-P0208", cgp08_perf_checks::run_cgp08_checks as fn() -> CheckSet),
        ("CGPU-O0705", vco05_statemachine_checks::run_vco05_checks as fn() -> CheckSet),
        ("CGPU-O0706", vco06_executor_checks::run_vco06_checks as fn() -> CheckSet),
        ("CGPU-O0707", vco07_recovery_checks::run_vco07_checks as fn() -> CheckSet),
        // ── A 域 CPU 加速续组（F0015~F0023）──
        ("CGPU-F0015", cga15_csconv::run_cga15_checks as fn() -> CheckSet),
        ("CGPU-F0016", cga16_simdblend::run_cga16_checks as fn() -> CheckSet),
        ("CGPU-F0017", cga17_yuvdec::run_cga17_checks as fn() -> CheckSet),
        ("CGPU-F0018", cga18_texdec::run_cga18_checks as fn() -> CheckSet),
        ("CGPU-F0019", cga19_prioq::run_cga19_checks as fn() -> CheckSet),
        ("CGPU-F0020", cga20_numa::run_cga20_checks as fn() -> CheckSet),
        ("CGPU-F0021", cga21_tilesort::run_cga21_checks as fn() -> CheckSet),
        ("CGPU-F0022", cga22_tilereuse::run_cga22_checks as fn() -> CheckSet),
        ("CGPU-F0023", cga23_quality::run_cga23_checks as fn() -> CheckSet),
    // 补登记四组"已实现但从未入账"的判据（闭环对账 _tools/audit_domain_closure.py
    // 查出）：此前连 `pub mod` 声明都没有，全文件从未被编译检查，等于死代码。
    // T02 对接协议分层 / W02 文档信息架构 / W03 文档生成管线 / W04 API 参考生成。
        ("CGPU-T3042", vct02_layers_checks::run_vct02_checks as fn() -> CheckSet),
        ("CGPU-W3522", vcw02_infoarch_checks::run_vcw02_checks as fn() -> CheckSet),
        ("CGPU-W3523", vcw03_genpipeline_checks::run_vcw03_checks as fn() -> CheckSet),
        ("CGPU-W3524", vcw04_apiref_checks::run_vcw04_checks as fn() -> CheckSet),
        ("CGPU-F1605", vck05_taskgraph::run_vck05_checks as fn() -> CheckSet),
        ("CGPU-F1607", vck07_sync::run_vck07_checks as fn() -> CheckSet),
        ("CGPU-F1606", vck06_memcoh::run_vck06_checks as fn() -> CheckSet),
        ("CGPU-X3684", vcx84_bandwidth::run_vcx84_checks as fn() -> CheckSet),
    // W-VB1 · CGPU-F0966 NVIDIA 功耗画像（骨架深化 1500+ 行+判据 20 条：
    // 双线 TDP/TGP+墙序目录双闸/曲线物理约束三关/双写对拍 31 点/J 域 F0854
    // 遥测对位/密封指纹/两权分立/审计满容拒记/0x51xx 十码）
        ("CGPU-F0966", cgg06_nvpwr_checks::run_cgg06_checks as fn() -> CheckSet),
    // CGPU-F3528/F3529 收口两组（模块与判据早已实现，此前折叠数组未登记，
    // 等于从未参与闭环）：vcw07b 文档安全深化 / vcw09 文档站。
        ("CGPU-F3528-DEEP", vcw07b_docsec_deep::run_cgpu_f3528_deep_checks as fn() -> CheckSet),
        ("CGPU-F3529", vcw09_docsite::run_cgpu_f3529_checks as fn() -> CheckSet),
        ("CGPU-F0011", cga11_power_sched_checks::run_cga11_checks as fn() -> CheckSet),
        ("CGPU-F0323", cgc03_budgetaccount::run_cgc03_checks as fn() -> CheckSet),
        ("CGPU-F1124", cgh04_waterpress::run_cgh04_checks as fn() -> CheckSet),
        ("CGPU-F1445", cgj05_nvml::run_cgj05_checks as fn() -> CheckSet),
        ("CGPU-F1446", cgj06_pwrledger::run_cgj06_checks as fn() -> CheckSet),
        ("CGPU-F2403", cgp03_sampling_checks::run_cgp03_checks as fn() -> CheckSet),
        ("CGPU-F2404", cgp04_pipeline_checks::run_cgp04_checks as fn() -> CheckSet),
        ("CGPU-F2405", cgp05_tiers_checks::run_cgp05_checks as fn() -> CheckSet),
        ("CGPU-F2406", cgp06_query_checks::run_cgp06_checks as fn() -> CheckSet),
        ("CGPU-F2407", cgp07_jmerge_checks::run_cgp07_checks as fn() -> CheckSet),
        ("CGPU-F2409", cgp09_privacy_checks::run_cgp09_checks as fn() -> CheckSet),
        ("CGPU-F1448", cgj08_pwrevent::run_t448_checks as fn() -> CheckSet),
        ("CGPU-F0806", cgf06_certtier::run_t806_checks as fn() -> CheckSet),
        ("CGPU-F1447", cgj05_pwrfilt::run_tj05_checks as fn() -> CheckSet),
        ("CGPU-F1285", cgq01_txqueue::run_tq01_checks as fn() -> CheckSet),
        ("CGPU-F1286", cgq02_txmerge::run_tq02_checks as fn() -> CheckSet),
        ("CGPU-F3528", vcw07_docsec::run_tw07_checks as fn() -> CheckSet),
        ("CGPU-F1603", vck03_hettask_checks::run_vck03_checks as fn() -> CheckSet),
        ("CGPU-F1604", vck04_probe::run_vck04_checks as fn() -> CheckSet),
        ("CGPU-F1764", vck68_guestcap::run_vck68_checks as fn() -> CheckSet),
        ("CGPU-F2883", vcs03_session::run_vcs03_checks as fn() -> CheckSet),
        ("CGPU-F2884", vcs04_quota::run_vcs04_checks as fn() -> CheckSet),
        ("CGPU-F2885", vcs05_isolate::run_vcs05_checks as fn() -> CheckSet),
        ("CGPU-F3046", vct06_stackbudget::run_vct06_checks as fn() -> CheckSet),
        ("CGPU-F3203", vcu03_scenecls::run_vcu03_checks as fn() -> CheckSet),
        ("CGPU-F3204", vcu04_scoringsys::run_vcu04_checks as fn() -> CheckSet),
        ("CGPU-F3205", vcu05_benchrunner::run_vcu05_checks as fn() -> CheckSet),
        ("CGPU-F3526", vcw06_docsearch::run_vcw06_checks as fn() -> CheckSet),
    ];
    for (tag, f) in blocks.iter() {
        // 即用即弃：同一时刻栈上只存活一个子集 CheckSet（unxstar 同款手法）。
        let sub = f();
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

    /// 子集一集不少（漏登记会让该单判据静默退出闭环——折叠数组
    /// 元素个数由 `CGPU_SUBSET_COUNT` 在编译期锁死，此处再守一遍运行时）。
    #[test]
    fn cgpu_all_subsets_registered() {
        let set = run_cgpu_checks();
        assert_eq!(
            set.len(),
            CGPU_SUBSET_COUNT,
            "CGPU 折叠子集数必须等于 CGPU_SUBSET_COUNT"
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
// cgd15_d01close — CGPU-F0496 D01组收口与D02移交（16项自检点名制缺一即0x6201虚假收口/PrecisionReport快照指纹篡改即0x6203/双签纪律单签0x6202拒收/HandoverDoc三件套口径表-账本API-精度声明合成指纹/空手移交0x6204测量基础缺失/台账未更新0x6205/0x62xx 五码）
pub mod cgd15_d01close;
// cgf13_certapi — CGPU-F0811 认证API冻结v1（三组cert_query-cert_run-cert_export签名指纹冻结/跨族动词表对拍D08/E06命名一致性0x6302/重复注册0x6303/承诺域分隔PROMISE_DOMAIN_CERT与TIMER防混签0x6304/冻结表只增不改非法追加未开版本窗0x6305/0x63xx 五码）
pub mod cgf13_certapi;
// cgf14_certdoc — CGPU-F0812 认证文档（白皮书六步节完整性/悬空引用"规则表N"扫描0x6401/文档-代码对拍四级名+90天漂移即0x6402数字两套账/ReauthRuleTable三类触发表外0x6403/节号重复0x6404/白皮书过期指纹未随实现更新0x6405/0x64xx 五码）
pub mod cgf14_certdoc;
// cgg09_nvfuzz — CGPU-F0969 NVIDIA fuzz（代际/特性位/显存三类伪造钳制高位域越界虚报拦0x6501/0x6503/0x6505/驱动异常三族容错消费器未处理0x6502/案例固化LCG种子谱指纹CI回放不可复现0x6504/0x65xx 五码）
pub mod cgg09_nvfuzz;
// cgi09_bwfuzz — CGPU-F1288 带宽fuzz与安全（ConservationLedger四账户守恒Σ账≤带宽超即0x6601/PayloadIntegrity FNV载荷指纹写前算读后验篡改0x6602/QueueAcl默认deny白名单制未登记0x6603/畸形请求三族全拒采纳0x6604/风暴案例固化replay_ci不可复现0x6605/0x66xx 五码）
pub mod cgi09_bwfuzz;
// vck08_faildom — CGPU-F1608 异构失败域与降级（引擎级失败域四域闭集NPU崩不拖垮GPU隔离墙拦跨域传播0x6F01/逐引擎回退表NPU→GPU推理回退DSP→CPU多级链三跳封顶断裂显性0x6F02/部分失败语义节点可用性三态标注缺失0x6F03消费方决策/重置纪律触碰集恰单域越界0x6F04复探对接vck04不过即Retired/降级透明回退结果带Origin+性能差‰标注丢失0x6F05/kill注入演练五验证闭环六组自检/0x6Fxx 五码）
pub mod vck08_faildom;
// vck09_hetpower — CGPU-F1609 异构功耗热协同（NPU/DSP功耗按引擎系数表折算入J域一本账系数表版本化0x7001/热源扩展NPU温度源并入F1474热状态机输入未并入0x7002/协同降档Hot态NPU批减DSP任务延复用J03阶梯失步0x7003/能效路由fps-per-watt实测表跨引擎选路失据0x7004/场景×引擎矩阵视频MEDIA优先越界0x7005/F1460联合选点预留位显性登记/六组自检/0x70xx 五码）
pub mod vck09_hetpower;
// vck10_hetsched — CGPU-F1610 异构调度策略（五级决策树能力→亲和→能效→负载→功耗逐级留痕跳级0x7101/负载均衡水位表+成本感知迁移收益≤成本不迁不可迁移0x7102/优先级统一B域跨引擎可比+饥饿防护F1464复用声明失统0x7103/抢占能力表NPU不抢占越权0x7104排程适配SJF/公平指数Jain整数化红线申报制失守0x7105/策略配置化F1531模式/六组自检/0x71xx 五码）
pub mod vck10_hetsched;
// cgd16_jitterdet — CGPU-F0497 抖动检测器（双指标短时方差+长时趋势只算其一0x7201/谱分析整数自相关周期性vs随机区分失败0x7202/三档分级轻微-明显-严重阈值与F0484对齐越表0x7203/检测开销步数化<0.1%超标0x7204/样本不足半窗出判0x7205/端到端合判/五组自检/0x72xx 五码）
pub mod cgd16_jitterdet;
// cgd17_jittersrc — CGPU-F0498 抖动源分类（五源闭集OS抢占-驱动卡顿-引擎停顿-热降频-IO阻塞表外0x7301/指纹位图证据与源指纹不符0x7302/置信聚合多证据≥2才出判单证据0x7303归因冲突0x7305/可扩展分类表版本窗绕窗0x7304/端到端热降频双证据出判/五组自检/0x73xx 五码）
pub mod cgd17_jittersrc;
// cgg10_nvapi — CGPU-F0970 NVIDIA适配层API冻结（三组nv_probe-nv_diff-nv_media_tag签名FNV指纹冻结篡改0x7401重复0x7403绕版本窗0x7404/同构对拍F01/F02/F04形状逐位一致+承诺域分隔漂移0x7402/十年承诺PROMISE_DOMAIN_NV域分隔防混签失守0x7405/端到端全链/四组自检/0x74xx 五码）
pub mod cgg10_nvapi;
// cgi10_bwdoc — CGPU-F1289 带宽文档（白皮书三节PCIe模型-账户化-仲裁缺节0x7501/仲裁说明复用C域范式声明源单号非空失据0x7502/调优指南五步闭集+代际适用缺步0x7503/文档-代码对拍数字一套账漂移0x7504指纹新鲜过期0x7505/端到端全链/五组自检/0x75xx 五码）
pub mod cgi10_bwdoc;
// vck11_hettel — CGPU-F1611 异构遥测与归因（五指标集F1556v2注册表扩展利用率-队列-内存-功耗-温度缺指标0x7601/任务归因跨引擎复式账两列合计恒等不平0x7602/帧归因段Σ段≤帧长守恒C域联动超即0x7603/能效画像随实测更新批号新鲜滞后0x7604 F1499复用声明/J08对接桥异构指标入仓库+隐私红线遥测本地出域0x7605/六组自检/0x76xx 五码）
pub mod vck11_hettel;
// cgd18_frameeq — CGPU-F0499 帧间隔均衡器（三策略闭集延迟上屏-跳帧-补帧各策略延迟与流畅权衡声明在册失声明0x7705越集0x7701/内容自适应交互→跳帧视频→延迟上屏未登记选策略0x7702/无缝切换只影响后续帧回改已上屏帧0x7703/均衡开销步数化<0.2%超标0x7704/端到端交互→视频会话切换/五组自检/0x77xx 五码）
pub mod cgd18_frameeq;
// cgd19_timedoc — CGPU-F0493 帧计时文档（80帧合同条款三要素定义-测量方法-判定规则缺一即歧义0x7802条款数缺0x7801/计时方法论四段定义+校准方法锚cgd09段缺0x7803/账本查询手册键-形状条目重复键拒0x7804/新鲜度实现指纹对拍过期0x7805/端到端四件全链/五组自检/0x78xx 五码）
pub mod cgd19_timedoc;
// cgg11_nvdoc — CGPU-F0971 NVIDIA适配文档（白皮书三节六代覆盖-特性矩阵-双驱动线节缺0x7901/已知问题清单复现条件+影响面双锚纪律缺一0x7902空清单不健全/认证指南NVIDIA侧四步探测→差异→媒体标签→提交步缺0x7903/对拍漂移0x7904过期0x7905/端到端全链/五组自检/0x79xx 五码）
pub mod cgg11_nvdoc;
// cgf15_certbench — CGPU-F0813 认证基准（三基准闭集全流程耗时5000ms红线0x7A01+探测吞吐100/s红线0x7A02+判定延迟2000µs红线0x7A03阈值显性入册/口径版本防挑参作弊0x7A05/入册回归三样本同口径复跑漂移>10%立案0x7A04/口径指纹const稳定/四组自检/0x7Axx 五码）
pub mod cgf15_certbench;
// cgi11_bwbench — CGPU-F1290 带宽基准（三基准有效带宽各代PCIe实测vs理论闭集常量比≥75%红线0x7B01+仲裁开销50µs/轮红线0x7B02+批合并收益≥20%节时比负收益即红0x7B03/口径版本防挑参0x7B05/入册回归漂移立案0x7B04/五代全测端到端/四组自检/0x7Bxx 五码）
pub mod cgi11_bwbench;
// cgf16_certcons — CGPU-F0814 认证一致性（判定确定性同硬件同考卷同判定异判0x7C01/跨硬件归一化口径折算未归一比裸分0x7C02/判据变更ADR门无ADR0x7C03+重认证强制缺席0x7C04旧成绩跨版本失效/链闭合对账0x7C05/四组自检/0x7Cxx 五码）
pub mod cgf16_certcons;
// cgi12_bwcons — CGPU-F1291 带宽一致性（仲裁确定性同负载同分配异分配0x7D01/跨代归一Gen3/4/5折Gen1当量未归一0x7D02/仲裁语义变更ADR门无ADR0x7D03/语义账对拍不平0x7D04/ADR链断环0x7D05/四组自检/0x7Dxx 五码）
pub mod cgi12_bwcons;
// cgg12_nvbench — CGPU-F0972 NVIDIA基准（SKU成绩公开账六代D08逐代公开缺代0x7E01未公开拒/NVENC吞吐红线0x7E02/DLSS预留数据显性Empty不造假假数据0x7E03/入册回归漂移0x7E04口径防挑参0x7E05/四组自检/0x7Exx 五码）
pub mod cgg12_nvbench;
// vck12_hetsim — CGPU-F1612 异构模拟器（统一模拟框架F1454扩展声明NPU批处理模型+DSP实时调度模拟游离0x7F01/校准两态诚实真机vs文档典型值显性标注混标0x7F02/用途表四闭集CI-调度-回退-能效越表0x7F03/真机trace回放F1454复用漂移0x7F04/行为覆盖四类正常-满载-失败-抖动缺角0x7F05/五组自检/0x7Fxx 五码）
pub mod vck12_hetsim;
// vck13_hetsec — CGPU-F1613 异构安全边界（NPU模型=不可信输入三关格式-大小上限-沙箱缺一0x8101/DMA隔离IOMMU四域越界0x8102平台能力声明/配额防护超时+配额占死引擎0x8103/供应链签名校验预留失签0x8104/隐私生命周期推理数据不落持久区落盘0x8105/五组自检/0x81xx 五码）
pub mod vck13_hetsec;
// vck14_hetapi — CGPU-F1614 异构API冻结v1（14函数闭集五组注册2-查询2-提交3-同步3-遥测4缺函数0x8201/三要素签名-语义-错误路径缺一0x8202/只增承诺冻结篡改0x8203十年/扩展位新引擎类型F0406开放精神缺席0x8204/跨域对齐B/J域签名一致漂移0x8205/五组自检/0x82xx 五码）
pub mod vck14_hetapi;
// vck15_k06close — CGPU-F1696 K06组收口双签（15项接口总账Frozen+能效API6函数扩展0x8301/主题映射三主题Done证据CGPU-F源单0x8302/J03-J04-J06-J07四契约归档0x8303/四账测试118集成14组合12全绿+增益20%红线+遗留显性登记0x8304/双签工程+质量指纹失配拦截0x8305/六组自检/0x83xx 五码）
pub mod vck15_k06close;
// vck16_k07arch — CGPU-F1697 K07开工与异构遥测总架构（可观测三能力看得见-查得着-调得动各锚保证段0x8401/五段管道注册→采集→分析→呈现→调试单向流反向拒0x8402/K01→K07上下游口径锁0x8403/J08仓库-P看板-CIPC协议三复用源单号0x8404/四组自检/0x84xx 五码）
pub mod vck16_k07arch;
// vck17_obsmetric — CGPU-F1698 异构指标注册表终版（四族指标引擎运行-协同-能效-质量11指标闭集0x8501/口径唯一同名双公式拒幂等重注册放行0x8502/生命周期废弃停写在册可查未注册派生拒0x8503/派生守恒抽查重算失配拦0x8504/四组自检/0x85xx 五码）
pub mod vck17_obsmetric;
// vck18_frameattr2 — CGPU-F1699 异构帧归因深化（引擎×段双维矩阵行和列和守恒帧总时长12.5ms预算0x8601/等待归因fence锚自环拒阻塞定位top-1等待列对拍0x8602/互扰归因带宽热双轨分账自互拒换向重复拒置信万分比0x8603/C域段枚举F2953模式扩展同步对拍0x8604/四组自检/0x86xx 五码）
pub mod vck18_frameattr2;
// vck19_tldebug — CGPU-F1700 异构时间线调试器（Gantt产品化窗口查询同引擎重叠拒负时长拒0x8701/旅程五阶段提交→排队→执行→交接→完成时间戳回退拒跨引擎对端强制阶段分解0x8702/交互缩放万分比越界拒引擎位图过滤选择命中落空显性0x8703/跨卷衔接VE时间线F1442同源锚偏移1s限0x8704/四组自检/0x87xx 五码）
pub mod vck19_tldebug;
// vck20_dbgpanel — CGPU-F1701 引擎级调试面板（四面板NPU-F1629复用-DSP-多GPU-能效0x8801/统一框架元数据同构通道唯一0x8802/口径与注册表对齐漂移未注册拒0x8803/刷新1Hz基础可调500..10000mHz0x8804/CIPC四通道复用源单号F1704覆盖0x8805/五组自检/0x88xx 五码）
pub mod vck20_dbgpanel;
// vck21_tracefmt — CGPU-F1702 异构trace与开放格式（VXTR魔数schema v1版本化0x8901/导出导入往返FNV对拍截断拒0x8902/Perfetto兼容承诺窗0x8903/隐私红线字段白名单闭集审计0x8904/四组自检/0x89xx 五码）
pub mod vck21_tracefmt;
// vck22_profiler — CGPU-F1703 异构性能剖析（引擎级采样三轴聚合0x8A01/热点TOP三轴交接-等待-切换降序守卫0x8A02/火焰融合四层栈限父子包含校验0x8A03/建议规则库8条闭集挂锚0x8A04/四组自检/0x8Axx 五码）
pub mod vck22_profiler;
// vck23_dbgproto — CGPU-F1704 异构调试协议（CIPC消息族四闭集面板-时间线-trace-剖析字段数对schema0x8B01/协议冻结v1签名稳定审计0x8B02/远程调试预留接口位实占禁令0x8B03/调试态权限递增链Off<Passive<Active各族门槛0x8B04/四组自检/0x8Bxx 五码）
pub mod vck23_dbgproto;
// vck24_simdbg — CGPU-F1705 异构模拟器调试支持（模拟=真机同格式F1612/F1661复用编码往返恒等0x8C01/面板复用统一框架口径挂接0x8C02/CI确定性LCG同种子同指纹逐事件恒等0x8C03/三组自检/0x8Cxx 五码）
pub mod vck24_simdbg;
// vck25_telbudget — CGPU-F1706 异构遥测性能预算（三段50/30/20µs分账总增量100µs门禁0x8F01/零干扰三防护条热路径零分配零锁预算帽0x8F02/采样降级四档逐档不跳Off见底拒0x8F03/自举观测自身开销入账重复拒零开销拒深度帽1_0x8F04/四组自检/0x8Fxx 五码）
pub mod vck25_telbudget;
// vck26_telprivacy — CGPU-F1707 异构遥测隐私（三级分类Public-Internal-Sensitive表外拒级别闸0x9201/模型名指纹评估版本+量化后缀可定位即本地化未评估拒0x9202/控制三闸面板开导出默认关删除幂等0x9203/上传匿名白名单剥离敏感项0x9204/四组自检/0x92xx 五码）
pub mod vck26_telprivacy;
// vck27_selfheal — CGPU-F1708 异构遥测异常自愈（三类闭集断流-指标异常-仓库失败0x9301/动作表配对断流→估算态标注异常→熔断失败→重试上限3禁用拒0x9302/心跳账倒退拒停摆检出0x9303/缺口诚实未标注出流拒0x9304/四组自检/0x93xx 五码）
pub mod vck27_selfheal;
// vck28_reportgen — CGPU-F1709 异构报告生成（周报模板F1559复用三节七行协同-能效-异常0x9401/数字回源断言源锚重算失配拦0x9402/导出F1561复用格式闭集授权默认关正文指纹0x9403/三组自检/0x94xx 五码）
pub mod vck28_reportgen;
// vck29_obsdoc — CGPU-F1710 异构文档（观测指南三章指标-面板-trace行数红线0x9501/API参考10函数冻结清单指纹对拍0x9502/教程五步十分钟600s预算0x9503/文档新鲜度版本对拍0x9504/三组自检/0x95xx 五码）
pub mod vck29_obsdoc;
// vck30_k07integ — CGPU-F1711 K07联调（端到端20用例×四站采集→仓库→面板→报告0x9601/五断言口径-归因-开销-隐私-trace全绿0x9602/故障演练断流异常注入自愈接管配对0x9603/8h长跑三查漂移-泄漏-守恒0x9604/联调报告结论证据锚/五组自检/0x96xx 五码）
pub mod vck30_k07integ;
// vck31_k07close — CGPU-F1712 K07组收口双签（15项接口总账含API10冻结指纹0x9801/三主题兑现0x9802/J08-C-P-E四契约0x9803/四账单元126集成14用例20+增量100µs恰值+指南教程+遗留显性0x9804/双签指纹失配三反例0x9805/六组自检/0x98xx 五码）
pub mod vck31_k07close;
// vck32_k08arch — CGPU-F1713 K08开工与场景生态总纲（落地声明两受众开发者-用户能力锚0x9A01/四场景族游戏-创作-生产力-AIPC挂J07映射0x9A02/生态五件套SDK-示例-基准-认证-兼容矩阵源单锚0x9A03/三组自检/0x9Axx 五码）
pub mod vck32_k08arch;
// vck33_gamescene — CGPU-F1714 游戏×异构场景（五点表DL超分-帧生成预留-材质AI-音频空间化-解播0x9B01/插件接口位三引擎预留指纹0x9B02/量化三维度帧率-画质-功耗A-B证据0x9B03/NPU点GPU回退路径0x9B04/F1722认证衔接/五组自检/0x9Bxx 五码）
pub mod vck33_gamescene;
// vck34_createscene — CGPU-F1715 创作×异构场景（五点表特效预览-硬编-ISP调色-生图预留-音频实时0x9E01/流水F1657导入→编辑→导出单调0x9E02/预览门禁33333µs编辑流畅线0x9E03/量化导出-预览双维证据0x9E04/四组自检/0x9Exx 五码）
pub mod vck34_createscene;
// vck35_prodscene — CGPU-F1716 生产力×异构场景（四点表语义搜索-文档AI-iGPU优先-网页预留0x9F01/J07联动实例单0x9F02/响应红线2ms0x9F03/续航效率量化0x9F04/四组自检/0x9Fxx 五码）
pub mod vck35_prodscene;
// vck36_aipc — CGPU-F1717 AI PC场景体系（NPU常驻本地AI声明0xA001/能力目录四闭集超分-转写-摘要-生图+发现API0xA002/数据不出机出机通道零容0xA003/配额AI35%前台保底45%0xA004/空闲优先三档调度0/10000/3000bp/五组自检/0xA0xx 五码）
pub mod vck36_aipc;
// vck37_3pframework — CGPU-F1718 第三方引擎框架接入（K01上薄封装不加语义0xA101/适配矩阵ONNX-Basic-DirectML-引擎预留三级0xA102/版本跟随N-1窗边界声明夜版不承诺0xA103/每适配最小示例对齐0xA104/F3109治理复用/五组自检/0xA1xx 五码）
pub mod vck37_3pframework;
// vck38_sdk — CGPU-F1719 异构SDK与开发者体验（三件套API封装-模拟器-trace分析器源单挂既有0xA201/DX错误三要素码-人话-动作0xA202/版本对齐API冻结锚0xA203/双渠道分发+反馈闭环回执0xA204/五组自检/0xA2xx 五码）
pub mod vck38_sdk;
// vck39_examples — CGPU-F1720 异构示例库与教程（15例五要素闭集0xA301/三阶教程挂接表0xA302/CI全跑门禁0xA303/缺额对账0xA304/五组自检/0xA3xx 五码）
pub mod vck39_examples;
// vck40_benchrel — CGPU-F1721 异构基准发布（三基准定义0xA401/环境标注四要素+双跑ppm带0xA402/发布账修订链单调0xA403/对比诚实三红线0xA404/四组自检/0xA4xx 五码）
pub mod vck40_benchrel;
// vck41_certplan — CGPU-F1722 异构认证计划（设备+场景两级0xA501/判据公开九条可复现0xA502/四站单调流程0xA503/驱动更新复测维持0xA504/撤销公示0xA505/五组自检/0xA5xx 五码）
pub mod vck41_certplan;
// vck42_compatmx — CGPU-F1723 异构兼容性矩阵（硬件×驱动×场景三维格0xA601/驱动同步0xA602/安装前探测查询0xA603/夸大红线0xA604/四组自检/0xA6xx 五码）
pub mod vck42_compatmx;
// vck43_usercap — CGPU-F1724 异构用户可见能力（UE三接面0xA701/面板四项可关可查0xA702/文案三要素+句式表0xA703/克制四词审计+无障碍三查0xA704/五组自检/0xA7xx 五码）
pub mod vck43_usercap;
// vck44_mktgov — CGPU-F1725 异构市场与生态治理（市场三道闸+激励预留0xA801/FNV签名对拍0xA802/下架四步单调+时限0xA803/统计白名单匿名0xA804/五组自检/0xA8xx 五码）
pub mod vck44_mktgov;
// vck45_roadmap — CGPU-F1726 异构路线图（二期四向FPGA-在线学习-自动切分-分布式0xA901/开放位声明一致性0xA902/SYCL-ONNX标准跟踪0xA903/总路线衔接0xA904/四组自检/0xA9xx 五码）
pub mod vck45_roadmap;
// vck46_k08integ — CGPU-F1727 K08联调（4场景×4例16条0xAA01/五断言口径0xAA02/第三方模拟接入三步演练0xAA03/报告证据锚FNV0xAA04/四组自检/0xAAxx 五码）
pub mod vck46_k08integ;
// vck47_k08close — CGPU-F1728 K08组收口双签（15项接口总账FNV冻结v1 0xAB01/G-H-F-J07-N五契约归档0xAB02/测试118-14-16+性能四门禁+文档三件账0xAB03/遗留显性登记0xAB04/工程+质量双签/六组自检/0xABxx 五码）
pub mod vck47_k08close;
// vck48_k09arch — CGPU-F1729 K09开工与自查范围（O09-J09-K09范式三次复用0xAC01/七维范围+红黄绿GO门禁0xAC02/三纪律复用0xAC03/记录+F1742报告K10消费双锚0xAC04/五组自检/0xACxx 五码）
pub mod vck48_k09arch;
// vck49_k0103audit — CGPU-F1730 K01-K03组自查核验（接口14/12/10回读0xAD01/F0406枚举兑现闭环0xAD02/BJI安全四契约0xAD03/五场景延迟表+miss<0.1%门禁回读0xAD04/结论三态/0xADxx 五码）
pub mod vck49_k0103audit;
// vck50_k0406audit — CGPU-F1731 K04-K06组自查核验（接口8/8/6回读0xAE01/深化六向+离线声明0xAE02/无黑屏红线+三模式伪影声明0xAE03/能效增益20%+两态画像+决策可解释0xAE04/结论三态/0xAExx 五码）
pub mod vck50_k0406audit;
// vck51_k0708audit — CGPU-F1732 K07-K08组自查核验（接口10/6回读0xAF01/归因双维守恒+准确率≥950‰0xAF02/隐私三档回读0xAF03/16条端到端+矩阵诚实+判据公开0xAF04/结论三态/0xAFxx 五码）
pub mod vck51_k0708audit;
// vck52_pairaudit — CGPU-F1733 组间两两对账（重点10对在册+C(8,2)=28总账0xB001/逐对三查口径-依赖-漂移0xB002/发现显性登记带去向0xB003/指纹查重0xB004/三组自检/0xB0xx 五码）
pub mod vck52_pairaudit;
// vck53_apicensus — CGPU-F1734 K域API面终版清点（八组74函数总表实测0xB101/命名蛇形+无环+三要素复审0xB102/十年承诺封版v1 0xB103/扩展位开放+跨域B-J-C单向无环0xB104/六组自检/0xB1xx 五码）
pub mod vck53_apicensus;
// vck54_testassets — CGPU-F1735 测试资产汇总（单元1078八组分账+集成136+联测117恰值0xB201/全绿证据FNV复算对拍0xB202/组合八组全非零0xB203/跳过显性核因+去向0xB204/四组自检/0xB2xx 五码）
pub mod vck54_testassets;
// vck55_perfassets — CGPU-F1736 性能与精度资产汇总（开销恰值0.1ms双账0xB301/加速比≥1.5x+能效≥20%0xB302/预测≥800‰+画像门禁0xB303/未达标显性对照+长稳8h三查0xB304/五组自检/0xB3xx 五码）
pub mod vck55_perfassets;
// vck56_secassets — CGPU-F1737 安全红线资产汇总（六红线同源断言0xB401/模型+序fuzz零崩溃0xB402/kill-拔出-过热三类演练0xB403/供应链三依赖审计0xB404/五组自检/0xB4xx 五码）
pub mod vck56_secassets;
// vck57_soakreg — CGPU-F1738 长稳与漂移诚实登记（六份8h覆盖矩阵K01-K03-K05-K06-K07 0xB501/画像+开销漂移千分比账0xB502/未定位显性诚实0xB503/模拟器+trace容量水位≤800‰0xB504/四组自检/0xB5xx 五码）
pub mod vck57_soakreg;
// vck58_gapfill — CGPU-F1739 缺失与薄弱项补齐（三来源核验带条件-对账发现-资产缺口0xB601/红黄绿分级0xB602/补齐必复验证据锚0xB603/台账三条+K10移交声明0xB604/五组自检/0xB6xx 五码）
pub mod vck58_gapfill;
// vck59_riskreg — CGPU-F1740 九维汇总表与风险登记（8组×9维72格绿黄红0xB701/风险TOP10降序恰值0xB702/黄格=计划数对账0xB703/红格必触发纪律0xB704/F1742衔接锚/五组自检/0xB7xx 五码）
pub mod vck59_riskreg;
// vck60_prescore — CGPU-F1741 20维度预评分（预演声明+20维互异0xB801/评分≤1000定点+证据锚0xB802/<800必处置0xB803/J域940基线对标均分只算不裁0xB804/三组自检/0xB8xx 五码）
pub mod vck60_prescore;
// vck61_goreport — CGPU-F1742 K域预备报告GO/NO-GO（八节结构复用0xB901/GO五条件逐条判定同源对拍0xB902/NO-GO纪律一票否决0xB903/工程+质量+K10就绪三态双签0xB904/四组自检/0xB9xx 五码）
pub mod vck61_goreport;
// vck62_rehearsal — CGPU-F1743 预备演练（K10流程干跑五要素预生成0xBA01/彩排三查0xBA02/开发者+用户双旅程四步单调走查0xBA03/执行记录8步指纹入附录0xBA04/四组自检/0xBAxx 五码）
pub mod vck62_rehearsal;
// vck63_k09close — CGPU-F1744 K09组收口双签（15项接口+自查工具5函数FNV冻结v1 0xBB01/8组-10对-74清点总数字恰值0xBB02/五资产确认0xBB03/补齐三数字+K10移交GO确认0xBB04/双签/六组自检/0xBBxx 五码）
pub mod vck63_k09close;
// cgj06_pwrledger — CGPU-F1446 核显与内存功耗分账（五账户可分则分不可分整账显性/Package活动比例拆分估算非实测/显存三参数模型可校准/面板LUT查表/审计5%容差+负残差立案/不确定度保守传播+决策加权/H域提示单/幂等闸+快照对拍/0xBC段12码）
pub mod cgj06_pwrledger;
// vck64_k10ledger — CGPU-F1745 K域接口总账与主题映射终审（九组79函数恰值0xBC01/四资产v1冻结+十年承诺0xBC02/八主题8/8终审0xBC03/八域契约同源+预留闭环双锚0xBC04/五组自检/0xBCxx 五码）
pub mod vck64_k10ledger;
// vck65_k10metric — CGPU-F1746 K域指标与归因准确率终审（注册表v2四族+口径唯一0xBD01/归因四维≥950‰契约0xBD02/精度全表恰值0xBD03/五门禁回读+守恒±2%抽查+估算显性标注0xBD04/六组自检/0xBDxx 五码）
pub mod vck65_k10metric;
// vck66_k10scene — CGPU-F1747 K域场景覆盖终审（五段场景账63无空白+资产三件0xBE01/路由≥990‰0xBE02/量化全表效益≥承诺延迟≤承诺0xBE03/真机三对照回读0xBE04/五组自检/0xBExx 五码）
pub mod vck66_k10scene;
// vck67_k10disc — CGPU-F1748 K域三纪律终审（三开销恰值100/100/50µs回读0xBF01/采样≤50‰+降级四档连续0xBF02/不造假30路径零违例0xBF03/五断言CI固化+违例零未决0xBF04/五组自检/0xBFxx 五码）
pub mod vck67_k10disc;
// vck68_guestcap — CGPU-F1764 能力探测与虚拟化感知（guest探测模式判读矛盾保守仲裁+探测矩阵const表压舱+诚实三验证态虚标拦截+host协商四步交集语义越权留痕+探测缓存K01+失败三连保守模式/0xBD段12码）
pub mod vck68_guestcap;

// ==========================================================================
// CGPU 批次 3（CGPU-F3206 结果归一化 U05 组续单）
// ==========================================================================
pub mod vcu06_resultnorm;
pub mod vcu06grid;
pub mod vcu06mx;

// --------------------------------------------------------------------------
/// CGPU-F3206 结果归一化域聚合（跨硬件归一/基线对照）。
pub fn run_cgpu20_g06c() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("CGPU20-g06c");
    let mut ok = true;
    if !vcu06_resultnorm::run_g06c_checks().all_passed() { ok = false; }
    if !vcu06grid::run_g06c_checks().all_passed() { ok = false; }
    if !vcu06mx::run_g06c_checks().all_passed() { ok = false; }
    set.add("CGPU-F3206", ok, if ok { "" } else { "cgpu20-g06c family sub-checks red" });
    set
}
// vcw07b_docsec_deep — CGPU-F3528 文档安全深化版（裁决书留痕可追责/三面同实例真复用+引擎指纹同源/降级最严闸零泄漏/跨版本两向差异签字结案/误杀申诉只追加不抹账/0x7680 段十二码）
pub mod vcw07b_docsec_deep;
// vcw09_docsite — CGPU-F3529 文档站（路由归一双射+路径穿越拒+静态幂等零动态零外链+增量最小重建传递闭包+拓扑环拒+三账守恒/0x7700 段十二码）
pub mod vcw09_docsite;

// ==========================================================================
// WB1022 批一（CGPU-F0114~F0123 · A 域收尾组 · 判据押后模式：纯功能）
// ==========================================================================
pub mod cga114_detverify;
pub mod cga115_xcompiler;
pub mod cga116_jointbench;
pub mod cga117_opspower;
pub mod cga118_simdabi;
pub mod cga119_isaeval;
pub mod cga120_microarch;
pub mod cga121_teachdoc;
pub mod cga122_simdex;
pub mod cga123_batchclose;
pub mod cga124_handover;
pub mod cga125_apifreeze;
pub mod cga126_perfbudget;
pub mod cga127_memledger;
pub mod cga128_errtable;
pub mod cga129_docgen;
pub mod cga130_dedupfinal;
pub mod cga131_aceptmatrix;
pub mod cga132_depgraph;
pub mod cga133_defectzero;

// cgb17_xfbackpress ↔ CGPU-F0230 依赖深度与回压（0x63C0 段）（AI-WBX1022 B05 组批次）
pub mod cgb17_xfbackpress;
// cgb18_xfcycle ↔ CGPU-F0231 跨帧环检测（0x63D0 段）（AI-WBX1022 B05 组批次）
pub mod cgb18_xfcycle;
// cgb19_xfexecint ↔ CGPU-F0232 与帧图执行器集成（0x64A0 段）（AI-WBX1022 B05 组批次）
pub mod cgb19_xfexecint;
// cgb20_xfdet ↔ CGPU-F0233 跨帧确定性（0x64B0 段）（AI-WBX1022 B05 组批次）
pub mod cgb20_xfdet;
// cgb21_xftel ↔ CGPU-F0234 跨帧遥测（0x64D0 段）（AI-WBX1022 B05 组批次）
pub mod cgb21_xftel;
// cgb22_xffuzz ↔ CGPU-F0235 跨帧 fuzz（0x64E0 段）（AI-WBX1022 B05 组批次）
pub mod cgb22_xffuzz;
// cgb23_xfapi ↔ CGPU-F0236 跨帧 API 冻结 v1（0x24A0 段）（AI-WBX1022 B05 组批次）
pub mod cgb23_xfapi;
// cgb24_xfdoc ↔ CGPU-F0237 跨帧文档（0x24B0 段）（AI-WBX1022 B05 组批次）
pub mod cgb24_xfdoc;
// cgb25_xfbench ↔ CGPU-F0238 跨帧基准（0x2820 段）（AI-WBX1022 B05 组批次）
pub mod cgb25_xfbench;
// cgb26_xfcons ↔ CGPU-F0239 跨帧组一致性（0x2830 段）（AI-WBX1022 B05 组批次）
pub mod cgb26_xfcons;
// cgb27_xfclose ↔ CGPU-F0240 B05 组收口与 B06 移交（0x2840 段）（AI-WBX1022 B05 组批次）
pub mod cgb27_xfclose;
// --------------------------------------------------------------------------
// WB1022 批三（CGPU-F0134~F0143 · A 域收官组 · 纯功能）
// --------------------------------------------------------------------------
pub mod cga134_stress;
pub mod cga135_secscan;
pub mod cga136_compat;
pub mod cga137_declare;
pub mod cga138_hovpack;
pub mod cga139_kmbase;
pub mod cga140_midmile;
pub mod cga141_aggcheck;
pub mod cga142_freezeaudit;
pub mod cga143_docfinal;

// --------------------------------------------------------------------------
// WB1022 批四（CGPU-F0144~F0153 · A 域终验收官组 · 纯功能）
// --------------------------------------------------------------------------
pub mod cga144_dedupsupp;
pub mod cga145_acceptfin;
pub mod cga146_stressfin;
pub mod cga147_secfin;
pub mod cga148_compatfin;
pub mod cga149_declfin;
pub mod cga150_kmfin;
pub mod cga151_jointver;
pub mod cga152_opplog;
pub mod cga153_regressgate;

// --------------------------------------------------------------------------
// WB1022 批五（CGPU-F0154~F0163 · A域收官 + B域开局 · 纯功能）
// --------------------------------------------------------------------------
pub mod cga154_fuzzsched;
pub mod cga155_a_b_joint;
pub mod cga156_lessonbook;
pub mod cga157_codeaudit;
pub mod cga158_handoververify;
pub mod cga159_crosstarget;
pub mod cga160_domainfinal;
pub mod cgb161_framegraph;
pub mod cgb162_resautoedges;
pub mod cgb163_toposort;

// ---- 死判据补登记（2026-10-22 接线轮：模块早已实现但从未入链，等于从未参与编译与验收）----
// //! cga11_power_sched_checks — CGPU-F0011 功耗感知调度·判据集（20 条）。
pub mod cga11_power_sched_checks;
// //! CGPU-F0007 · 卡顿归因引擎（CGPU-C 域 · 帧预算域 · C08 组 · 深化版）。
pub mod cgc08_jankattrib;
// //! CGPU-F0008 · 掉帧降质链（CGPU-C 域 · 帧预算域 · C09 组 · 深化版）。
pub mod cgc09_degradechain;
// //! CGPU-F1124 · 显存压力水位（cgh04_waterpress）
pub mod cgh04_waterpress;
// //! cgj04_amdsmu — CGPU-F1444 AMD SMU/驱动功耗接口（深化版）
pub mod cgj04_amdsmu;
// //! CGPU-F1445 · NVIDIA NVML 功耗查询（cgj05_nvml）
pub mod cgj05_nvml;
// //! CGPU-F2403 · 采样策略引擎域自检（锚点测试三组：引擎/DSL/自适应 + stamp）。
pub mod cgp03_sampling_checks;
// //! CGPU-F2404 · 遥测数据管道域自检（锚点测试两组：四段/背压 + stamp）。
pub mod cgp04_pipeline_checks;
// //! CGPU-F2405 · 遥测存储分层域自检（锚点测试一组：分层 + stamp）。
pub mod cgp05_tiers_checks;
// //! CGPU-F2406 · 遥测查询引擎域自检（锚点测试一组：查询 + stamp）。
pub mod cgp06_query_checks;
// //! CGPU-F2407 · 遥测与 J 域收口域自检（锚点测试一组：收编 + stamp）。
pub mod cgp07_jmerge_checks;
// //! cgp09_privacy_checks — CGPU-F2409 遥测隐私 · 域自检判据。
pub mod cgp09_privacy_checks;
// //! vck03_hettask_checks — CGPU-F1603 异构任务模型与标签 · 域自检判据。
pub mod vck03_hettask_checks;
// //! CGPU-F1604 · 异构能力探测与登记（K 域 CGPU 段 · 目标 320 行）
pub mod vck04_probe;
// //! vck10_vgpu — CGPU-F1763 虚拟 GPU 设备抽象（深化版）
pub mod vck10_vgpu;
// //! vcs03_session — CGPU-F2883 用户会话管理（生命周期/并发上限/超时回收）。
pub mod vcs03_session;
// //! vcs04_quota — CGPU-F2884 资源配额按用户（三配额切分 · R05 复用声明）。
pub mod vcs04_quota;
// //! vcs05_isolate — CGPU-F2885 用户数据隔离（数据/配置/缓存三面 · W02 复用声明）。
pub mod vcs05_isolate;
// //! vct06_stackbudget — CGPU-F3046 对接性能预算（协议栈每帧 < 0.1ms 的分层预算账）。
pub mod vct06_stackbudget;
// //! vcu05_benchrunner — CGPU-F3205 基准运行器（场景调度/计时/采集三功能）。
pub mod vcu05_benchrunner;
// //! CGPU-F3526 · 文档搜索（vcw06_docsearch）
pub mod vcw06_docsearch;

// --------------------------------------------------------------------------
// WB1022 批六（CGPU-F0164~F0166 · B域跨帧/发射/执行引擎 · 纯功能）
// --------------------------------------------------------------------------
pub mod cgb164_xfdep;
pub mod cgb165_prioemit;
pub mod cgb166_graphexec;

// ---- 实现模块补登记（2026-10-22：判据接线时发现实现兄弟模块同样未入链）----
pub mod cga11_power_sched;
pub mod cgp03_sampling;
pub mod cgp04_pipeline;
pub mod cgp05_tiers;
pub mod cgp06_query;
pub mod cgp07_jmerge;
pub mod cgp09_privacy;

// --------------------------------------------------------------------------
// WB1022 批七（CGPU-F0172~F0176 · B01 组收口 · 纯功能）
// --------------------------------------------------------------------------
pub mod cgb172_framedet;
pub mod cgb173_batchlink;
pub mod cgb174_memgov;
pub mod cgb175_telemetry;
pub mod cgb176_b1close;

// --------------------------------------------------------------------------
// AI-MIMO-1022 批（CGPU-F0241~F0245 · B06 死锁检测组核心 · 纯功能）
// --------------------------------------------------------------------------
pub mod cgb241_dlarch;
pub mod cgb242_waitgraph;
pub mod cgb243_tarjan;
pub mod cgb244_confide;
pub mod cgb245_resolve;
