//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。
//! | [`cgi01_bandwidth`] | CGPU-F1281 | I 域开工与带宽总架构（**五主题闭集**PCIe带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因冻结、本域160项全挂靠表外不立题；**账户化**四类传输（纹理上传/下载/交换/流送）各开预算账户——每笔传输先申请余额不足即拒绝并计数留痕、退款只做消费迁移、账本守恒净消费口径自检；**边界**H域管显存驻留/I域管传输带宽/VE-AD存储流F5801+管到达三段各管一段、接口复用F0405集成面声明逐句可grep；**十组**规划表组名+起止单号逐字取自总纲批次锚点、F1281-F1440首尾连续无缺口、宣告行读屏可查；零panic面零IO零墙钟） |
//! | [`cgi01_bandwidth_checks`] | CGPU-F1281 域自检（判据五条映射：五主题闭集+标签独立重排全等、账户化申请/拒绝/退款/守恒全链+未开账户必拒反向语料、边界三域分工逐句grep、十组表与判据侧独立重排逐条全等+组间起止连续校验；守恒式与使用率判据侧独立重算） |
//! | [`cgd02_frametimer`] | CGPU-F0482 | 全帧精确计时器（**四段**输入采样→提交→GPU执行→呈现上屏全链分段闭段才出时长、重叠/倒序记账异常可判定检出；**双源**QPC/GPU时间戳闭集、同段双源跨度对账漂移超容差标注；**校准**CPU/GPU时钟域线性对齐两对样本完全确定映射、换算残差逐点给账超阈标注（换算误差可测）非单调样本拒绝标定；**开销**<0.1%帧预算实测值注入核算640ppm手算对账超标标注；零墙钟——时间戳全上游注入计时器只做账务对齐异常检测、零panic面零IO） |
//! | [`cgd02_frametimer_checks`] | CGPU-F0482 域自检（判据五条映射：四段开合时长+未闭段None反向+重叠倒序异常、双源闭集+漂移控制、线性标定+残差逐点+反向拒绝、开销实测核算+红线标注；时长/比率/ppm全部判据侧手算写死对账；stamp独立重排全等） |
//! | [`cgd03_frameledger`] | CGPU-F0483 | 帧时间账本（**完整记录**四段+帧间隔+场景标签+版本逐帧入账、缺段照记None不补造不跳过；**同schema**五元组与F0274/B08同构、三级命名d01.frameledger.*前缀逐条grep、schema版本进记录演进不破坏历史；**滚动**256帧逐帧配额恒定、超限退场进聚合桶min/max/sum/mean聚合不是丢弃；**查询**Exact/Aggregated/NotFound三答案各有其位；零panic面零IO零墙钟） |
//! | [`cgd03_frameledger_checks`] | CGPU-F0483 域自检（判据五条映射：完整记录+缺段如实+间隔手算对账、schema表与判据侧独立重排逐条全等+前缀grep+记录行按位对齐、滚动配额恒定+退场聚合min/max/sum手算对账、三答案各有其位+未记录NotFound反向；stamp独立重排全等） |
//! vcj01_powerarch — CGPU-F1441 J 域开工与功耗架构总览（域使命/五主题十组/三处核验/五段流水线/边界/采样不耗样本/风险回退/0x52xx）
//! cga02_threadpool —— CGPU-F0002 渲染线程池与工作窃取调度器（池规模核数减二/合成器专核隔离/同层优先窃取偷头不偷尾/瓦片独立缓冲确定性归并乱序逐像素一致/协作式 2ms 让位抢占三档优先/均衡≤5%/遥测三面/窃取开销≤3%账面/0x39xx 域码段）
//! vcl01_virtualarch — CGPU-F1761 L 域开工与虚拟化总架构（域使命三条款平等声明/八主题十组映射/K 域签收+I09 预留兑现/五段单向流水线/复用不重建 1601→1763+1466→1776/模式×合同等级表/场景四族/风险四条预案互异/0x53xx 五码/22 项域自检）
//! vcq01_reliability — CGPU-F2561 Q 域开工与可靠性总架构（恢复起点哲学三条款/P 域移交包 F2557 七件签收/五主题十组映射/五段单向流水线/O 域交接+混沌设施复用/不可恢复=最高缺陷红线+立案码/风险四条预案互异/0x56xx 六码/19 项域自检）
//! vcq02_metrics — CGPU-F2562 可靠性模型与指标（五指标闭集 MTBF/MTTR/可用性/RPO/RTO 定义与口径字面量冻结/口径复用/可用性万分比纯算术/目标表版本化不可变/RTO>0 RPO≤RTO 校验双向/V1 三条目钉死/0x58xx 五码/13 项域自检）
//! cgm01_display —— CGPU-F1921 M 域开工与显示输出总架构（最后一厘米三条款/L 域 F1917 签收七件/五主题十组映射覆盖守恒/五段单向流水线/F1768 贯通+V 域协同/呈现预算合同终端/风险四条预案/0x54xx 七码）
//! vct01_tbridge —— CGPU-F3041 T 域开工与 VE 对接总架构（S10 移交包七件逐件签收——衔接确认/协议桥梁定位声明=发起VE·执行CGPU·加速而非替代/四段架构闭集 命令·状态·资产·事件/四段→T02-T05 四批区间无缝兑现机检/SessionCtxTag 会话上下文传递不串户=衔接包约定落地/0.1ms 桥路预算在册/0x5Axx 五码）
//! vct01_tbridge_checks —— CGPU-F3041 域自检（14 项四组：签收 3=七件齐签收+缺件越界必拒重复去重+衔接包会话标签双非零哨兵 / 定位 2=定位三元组逐字在册+信封双校验与 0.1ms 预算恰端点 / 四段 3=闭集名实对应段序4拒+两两互异双证+兑现映射全可查 / 兑现 3=区间无缝首单紧随开工+职责句独立非空+批次名首单号对拍 / 码段 1=0x5A 独占+码互异 / 判据 2=实挂条数实取+名字互异）
//! cgm02_display —— CGPU-F1922 显示枚举与热插拔（枚举能力快照 id+EDID 指纹+seq 三件齐/去抖 leading-edge 恰端点/竞态=缺陷红线序列号恰一+快照新鲜度/三态单向状态机 mode-set 幂等/批量固定序去重重跑一致/0x5408~0x540C 域段续占互异）
//! cgm03_edid —— CGPU-F1923 显示能力查询 EDID（EDID1.4 基块解析 8 字节魔数+128 字节校验和 mod 256 硬门/首个 DTD@54 12 位拼装刷新率毫赫兹 u64 中间量防溢出 14850×10^7÷(2200×1125)=60000mHz 手算对账/CTA-861 扩展 YCbCr422·444 与 HDR 静态元数据 0xE6/损坏三向显性码拒+截断降级读出不虚构/能力缓存 display_id+指纹双键失效重读/能力投影纯裁剪诚实红线/覆盖白名单台账表外拒绝不突变/0x540D~0x5412 续占互异）
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
//! | [`cgp03_sampling`] | CGPU-F2403 | 采样策略引擎（**按价值采样**信号强度×成本→价值分整数除法零浮点、超阈值才采遥测有预算意识；**策略 DSL** 声明式可配五动作闭集 always/every_n/rate 万分比/value_above/adapt 逐行解析失败显性错四错闭集整批拒绝不部分装载；**自适应**负载档低/中/高闭集上游注入零墙钟、高负载 every_n 加倍/rate 折半/阈值上浮万分之 500 降频记账可查、降频只降频不减指标；**复用** F2402 六元组形状与隐私档不第二套 schema、复用 DSL 动作闭集做降频不另发明开关、F0274 三级命名作策略名表外不臆造；零panic面零IO零墙钟） |
//! | [`cgp03_sampling_checks`] | CGPU-F2403 域自检（判据三组映射：组一引擎价值分独立重算 500×1000÷25=20000+恰阈值双向+饱和不 panic+表外指标显性跳过+基线非全跳；组二 DSL 五动作逐字段对拍+四错闭集逐条+整批拒绝+rate 饱和+语法冻结片段；组三自适应 every_n 高负载加倍低采高跳+降频记账+阈值上浮 10000→10500 独立重算+Mid 反向+复用清单三条逐条 grep；stamp 五条独立重排全等） |
//! | [`vco01_degrade`] | CGPU-F2241 | O 域开工与降级链总架构（优雅降级哲学三条款字面量冻结；N 域移交包签收四字段+指纹 FNV 钉死；官方五主题十组映射守恒 160；架构五段单向流水线跳段/回退显性码拒；历史汇总五行+统一编排声明收编双向闸；双不变量底线闸恰端点钉死；风险四条预案互异；码段 0x55xx 独占） |
//! | [`vco01_degrade_checks`] | CGPU-F2241 域自检（16 条判据两族；哲学三条款+历史五行+风险四名+组名判据侧独立写死对拍、指纹独立 FNV 双源对账、五段全序列推进+跳/退/原地显性码拒、码段 0x55 独占、零 panic 双面自扫） |
//! | [`cgr01_secure`] | CGPU-F2721 | R 域开工与安全渲染总架构（Q10 移交包签收七件逐项登记缺项即域未开工/不可信内容渲染防线定位两条款字面量冻结——防线管渲染路径恶意与异常内容、不管业务正确性/验证→隔离→降级→恢复四段单向流水线跳段回退拒绝——未隔离即恢复=污染回流/Q10 兑现结构化确认非空头支票/诊断码独占 0x5701~0x5706） |
//! | [`cgr01_secure_checks`] | CGPU-F2721 域自检（签收/定位/四段/兑现四组判据承载+判据承载力自证；定位条款与六码判据侧独立对拍） |
//! | [`cgr02_threatmodel`] | CGPU-F2722 | 威胁模型与分类（五类威胁封闭枚举——恶意着色器/资源炸弹/越界/外泄/死循环表外不立类；分类表五规则每类一条可机检整数阈值字面量冻结判据独立对拍；攻击面清单五面封闭与五类一一承载恰五裁决清单外输入不存在进管线方式；特征只含元数据不含内容伪造签名拒绝；诊断码接续 0x5708~0x570D） |
//! | [`cgr02_threatmodel_checks`] | CGPU-F2722 域自检（判据五条映射 21 项六组：五类/分类表/清单/两组/判据；阈值 0/262144/1/1/65536 与六码字面量写死、恰阈不拦防等价变异、伪造签名反恒假、码段独占、条数离账） |
//! vco03_triggersrc — CGPU-F2243 降级触发源汇总（七类触发源闭集帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换/源注册追加式可扩展重复拒/源融合复用 F1459 主源 severity 最高平局取注册序最早确定性/贡献源账升序留痕/0x59xx 五码/12 项域自检）
//! | [`cga02_threadpool_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`cgm01_display_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`cgm02_display_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`cgm03_edid_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vcj01_powerarch_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vcl01_virtualarch_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vco03_triggersrc_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vcq01_reliability_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vcq02_metrics_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`cgm04_topology_checks`] | CGPU | 磁盘在账模块（注释行待该单负责人补全，先保编译面与聚合面完整） |
//! | [`vcv01_realverify`] | CGPU-F3361 | V 域开工与真机验收总架构（签收 U10 移交包 F3360 七件逐件对账缺件即空头签收拒；真机验收定位两条款字面量冻结——用手不用测试报告+十五章呼应不另立口径；四段单向流水线 矩阵→判据→走查→报告 跳段/回退/过早报告显性码拒；U10 预告兑现——Bench 分数与验收判据映射表整数口径恰阈值过差一分拒表外未映射拒；V_DOMAIN_TOTAL 160 守恒；0x5Bxx 六码独占） |
//! | [`vcv01_realverify_checks`] | CGPU-F3361 域自检（13 项六族：RECEIPT2 七件逐件对账+缺件错名反向必拒 / POSITION2 两条款逐字对拍+主证据篡改反向必拒 / PIPE2 四段恰一步全过+跳段回退双向拒与报告终端闸 / FULFILL3 兑现结构核对+映射表逐行独立对拍+恰阈值双向与表外拒 / CODE2 码段 0x5B 独占 !=0x50..0x5C 防自判死+码互异原因非空 / META2 域守恒独立重算+判据条数对账） |

//! vco04_priorcon — CGPU-F2244 降级优先级与冲突（四级序闭集安全>合同>体验>资源rank单调判据重算/优先级复用F1496四层模式对照表逐字/维度四闭集帧率画质延迟功耗归层映射/六对冲突显性消解规则R1~R6同层平局交互响应优先/裁决自检winners秩不劣于败者/码段0x5Axx五码/14项域自检）
//! | [`vcw01_sdkdoc`] | CGPU-F3521 | W 域开工与 SDK 文档总架构（签收 V10 移交包 F3520 七件逐件对账缺件即空头签收拒；SDK 文档定位两条款字面量冻结——十年接口的说明书冻结签名才进参考+文档即契约漂移即缺陷；四段单向流水线 参考→教程→质量→站点 跳段/回退/未过质量关即发布显性码拒；V10 预告兑现——验收判据入文档示例映射表与 V 域 vcv01 映射表跨单元同源对拍表外判据拒；W_DOMAIN_TOTAL 160 守恒；0x5Cxx 六码独占） |
//! | [`vcw01_sdkdoc_checks`] | CGPU-F3521 域自检（13 项六族：RECEIPT2 七件逐件对账+缺件错名反向必拒 / POSITION2 两条款逐字对拍+口径篡改反向必拒 / PIPE2 四段恰一步全过+跳段回退双向拒与发布终端闸 / FULFILL3 兑现结构核对+映射表逐行独立对拍+判据id与V域映射表跨单元同源 / CODE2 码段 0x5C 独占 !=0x50..0x5D 防自判死+码互异原因非空 / META2 域守恒独立重算+判据条数对账） |
//! vco05_statemachine — CGPU-F2245 全域降级状态机（四态闭集正常/降级中/降级档/恢复标签冻结/形式化复用J03=1475迁移表全枚举+不可达态检查/六条合法迁移条件显性逐字/跳段倒退自迁移逐类显性拒/带迁移史留痕与断裂拒/重入迁移恢复途中再触发/与vco01段态分工声明/码段0x5Bxx五码/14项域自检）
pub mod cga01_simdprim;
pub mod cga01_simdprim_checks;
pub mod cga02_threadpool;
pub mod cga02_threadpool_checks;
pub mod cgd02_frametimer;
pub mod cgd02_frametimer_checks;
pub mod cgd03_frameledger;
pub mod cgd03_frameledger_checks;
pub mod cge01_domain;
pub mod cge01_domain_checks;
pub mod cge02_lifecycle;
pub mod cge02_lifecycle_checks;
pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
pub mod cgm01_display;
pub mod cgm01_display_checks;
pub mod vct01_tbridge;
pub mod vct01_tbridge_checks;
pub mod cgm02_display;
pub mod cgm02_display_checks;
pub mod cgm03_edid;
pub mod cgm03_edid_checks;
pub mod cgm04_topology;
pub mod cgm04_topology_checks;
pub mod cgp01_adaptivearch;
pub mod cgp01_adaptivearch_checks;
pub mod cgp02_unifiedmodel;
pub mod cgp02_unifiedmodel_checks;
pub mod cgp03_sampling;
pub mod cgp03_sampling_checks;
pub mod cgr01_secure;
pub mod cgr01_secure_checks;
pub mod cgr02_threatmodel;
pub mod cgr02_threatmodel_checks;
pub mod vcj01_powerarch;
pub mod vcj01_powerarch_checks;
pub mod vcl01_virtualarch;
pub mod vcl01_virtualarch_checks;
pub mod vco01_degrade;
pub mod vco01_degrade_checks;
pub mod vco03_triggersrc;
pub mod vco03_triggersrc_checks;
pub mod vcq01_reliability;
pub mod vcq01_reliability_checks;
pub mod vcq02_metrics;
pub mod vcq02_metrics_checks;
pub mod vcv01_realverify;
pub mod vcv01_realverify_checks;
pub mod vcw01_sdkdoc;
pub mod vcw01_sdkdoc_checks;
pub mod vco04_priorcon;
pub mod vco04_priorcon_checks;
pub mod vco05_statemachine;
pub mod vco05_statemachine_checks;

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
/// 结构：21 个在账判据集顺序归约（与上方登记一一对应），新单入账在此追加。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            cga01_simdprim_checks::run_cga01_checks(),
            cga02_threadpool_checks::run_cga02_checks(),
        ),
        crate::checks::CheckSet::merge(
            cgd02_frametimer_checks::run_cgd02_checks(),
        crate::checks::CheckSet::merge(
            cgd03_frameledger_checks::run_cgd03_checks(),
            cgi01_bandwidth_checks::run_cgi01_checks(),
        ),
        ),
        ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            cgm01_display_checks::run_cgm01_checks(),
        crate::checks::CheckSet::merge(
            cgm02_display_checks::run_cgm02_checks(),
            cgp01_adaptivearch_checks::run_cgp01_checks(),
        ),
        ),
        crate::checks::CheckSet::merge(
            cgp02_unifiedmodel_checks::run_cgp02_checks(),
        crate::checks::CheckSet::merge(
            cgr01_secure_checks::run_cgr01_checks(),
            cgr02_threatmodel_checks::run_cgr02_checks(),
        ),
        ),
        ),
        ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            vcj01_powerarch_checks::run_vcj01_checks(),
            vcl01_virtualarch_checks::run_vcl01_checks(),
        ),
        crate::checks::CheckSet::merge(
            vco03_triggersrc_checks::run_vco03_checks(),
        crate::checks::CheckSet::merge(
            vcq01_reliability_checks::run_vcq01_checks(),
            vcq02_metrics_checks::run_vcq02_checks(),
        ),
        ),
        ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            vct01_tbridge_checks::run_vct01_checks(),
        crate::checks::CheckSet::merge(
            vcv01_realverify_checks::run_vcv01_checks(),
            vco04_priorcon_checks::run_vco04_checks(),
        ),
        ),
        crate::checks::CheckSet::merge(
            vcw01_sdkdoc_checks::run_vcw01_checks(),
        crate::checks::CheckSet::merge(
            cgp03_sampling_checks::run_cgp03_checks(),
            vco05_statemachine_checks::run_vco05_checks(),
        ),
        ),
        ),
        ),
        ),
}
