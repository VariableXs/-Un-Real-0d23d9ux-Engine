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
//! cgm01_display —— CGPU-F1921 M 域开工与显示输出总架构（最后一厘米三条款/L 域 F1917 签收七件/五主题十组映射覆盖守恒/五段单向流水线/F1768 贯通+V 域协同/呈现预算合同终端/风险四条预案/0x54xx 七码）
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
//! | [`cgp07_jmerge`] | CGPU-F2407 | 遥测与 J 域收口（**J08 指标全量并入**——J08_METRICS 六条列账 id/单位/采样等级/口径一行、逐条转 F2402 六元组形状注册进 P 域统一模型收编落地可机检 verify_merged；**幂等收编**重复执行已在册跳过不发新 ID 可重放；**单向收编** P 收编 J 零反向依赖、收编以 F2401 收编闸 J08 列名为限表外不臆造、表内 id 前缀全 j08. 校验；**收编复用** F2402 六元组形状与 SCHEMA_VERSION=3 不改形状、F2401 收编闸列名、F1558 采样等级 L0/L1/L2 口径；零panic面零IO零墙钟） |
//! | [`cgp07_jmerge_checks`] | CGPU-F2407 域自检（锚点一组判据映射：首次收编 6 条全量 merged=6 skipped=0+verify 通过/形状逐字段对拍 die_temp 条/幂等重放 merged=0 skipped=6/核验器反向缺条时红防恒绿/表内 id 前缀全 j08. 校验；复用清单三条含 F2402/F2401/F1558 逐条 grep；schema 同步=3 与表数=6 独立写死对拍；stamp 三条独立重排全等） |
//! | [`vco01_degrade`] | CGPU-F2241 | O 域开工与降级链总架构（优雅降级哲学三条款字面量冻结；N 域移交包签收四字段+指纹 FNV 钉死；官方五主题十组映射守恒 160；架构五段单向流水线跳段/回退显性码拒；历史汇总五行+统一编排声明收编双向闸；双不变量底线闸恰端点钉死；风险四条预案互异；码段 0x55xx 独占） |
//! | [`vco01_degrade_checks`] | CGPU-F2241 域自检（16 条判据两族；哲学三条款+历史五行+风险四名+组名判据侧独立写死对拍、指纹独立 FNV 双源对账、五段全序列推进+跳/退/原地显性码拒、码段 0x55 独占、零 panic 双面自扫） |
//! | [`cgr01_secure`] | CGPU-F2721 | R 域开工与安全渲染总架构（Q10 移交包签收七件逐项登记缺项即域未开工/不可信内容渲染防线定位两条款字面量冻结——防线管渲染路径恶意与异常内容、不管业务正确性/验证→隔离→降级→恢复四段单向流水线跳段回退拒绝——未隔离即恢复=污染回流/Q10 兑现结构化确认非空头支票/诊断码独占 0x5701~0x5706） |
//! | [`cgr01_secure_checks`] | CGPU-F2721 域自检（签收/定位/四段/兑现四组判据承载+判据承载力自证；定位条款与六码判据侧独立对拍） |
//! | [`cgr02_threatmodel`] | CGPU-F2722 | 威胁模型与分类（五类威胁封闭枚举——恶意着色器/资源炸弹/越界/外泄/死循环表外不立类；分类表五规则每类一条可机检整数阈值字面量冻结判据独立对拍；攻击面清单五面封闭与五类一一承载恰五裁决清单外输入不存在进管线方式；特征只含元数据不含内容伪造签名拒绝；诊断码接续 0x5708~0x570D） |
//! | [`cgr02_threatmodel_checks`] | CGPU-F2722 域自检（判据五条映射 21 项六组：五类/分类表/清单/两组/判据；阈值 0/262144/1/1/65536 与六码字面量写死、恰阈不拦防等价变异、伪造签名反恒假、码段独占、条数离账） |

pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
pub mod cgd02_frametimer;
pub mod cgd02_frametimer_checks;
pub mod vcj01_powerarch;
pub mod vcj01_powerarch_checks;
pub mod cga02_threadpool;
pub mod cga02_threadpool_checks;
pub mod vcl01_virtualarch;
pub mod vcl01_virtualarch_checks;
pub mod vcq01_reliability;
pub mod vcq01_reliability_checks;
pub mod vcq02_metrics;
pub mod vcq02_metrics_checks;
pub mod cgm01_display;
pub mod cgm01_display_checks;
pub mod cgm02_display;
pub mod cgm02_display_checks;
pub mod cgm03_edid;
pub mod cgm03_edid_checks;
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
pub mod cgp07_jmerge;
pub mod cgp07_jmerge_checks;
pub mod vco01_degrade;
pub mod vco01_degrade_checks;
pub mod cgr01_secure;
pub mod cgr01_secure_checks;
pub mod cgr02_threatmodel;
pub mod cgr02_threatmodel_checks;
pub mod vco03_triggersrc;
pub mod vco03_triggersrc_checks;
pub mod cgd03_frameledger_checks;
pub mod vcw04_apiref;
pub mod vcw04_apiref_checks;
//! vco03_triggersrc — CGPU-F2243 降级触发源汇总（七类触发源闭集帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换/源注册追加式可扩展重复拒/源融合复用 F1459 主源 severity 最高平局取注册序最早确定性/贡献源账升序留痕/0x59xx 五码/12 项域自检）
//! | [`vcw04_apiref`] | CGPU-F3524 | API 参考自动生成（承接 F3523 生成管线——参考页同样是自动派生物不手写 印记 pub use 同一常量不重写；双步架构 注释抽取→页面渲染 单向跳步/回退显性码拒 抽取产物是两步间唯一通道；冻结闸兑现 vcw01 条款——frozen 显性在账未冻结入参考 NOT_FROZEN 拒 条款是代码路径不是口号；参考页行数同源公式 1+Σ(标题+参数+返回+分隔)+两次渲染逐行相同；WqCode 0x5C13~0x5C18 续段与前三单十八码不重叠） |
//! | [`vcw04_apiref_checks`] | CGPU-F3524 域自检（13 项六族：STEP2 双步名序对拍+违序双向拒 / EXTRACT3 冻结签名全过字段逐项对拍+未冻结闸单条与批量拒+反向语料专属码分账 / RENDER3 行数三方同源+确定性与标题行逐字+印记与 F3523 管线同源 / LINK2 冻结条款兑现跨单元对拍 vcw01 POSITION_CLAUSES+域守恒 160 / CODE2 码段 0x5C13~18 连续与前三单十八码不重叠+码互异原因非空 / META1 判据条数对账） |

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
    crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        cga01_simdprim_checks::run_cga01_checks(),
        cga02_threadpool_checks::run_cga02_checks()
    ),
        crate::checks::CheckSet::merge(
        cge01_domain_checks::run_cge01_checks(),
        crate::checks::CheckSet::merge(
        cge02_lifecycle_checks::run_cge02_checks(),
        cgi01_bandwidth_checks::run_cgi01_checks()
    )
    )
    ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        cgm01_display_checks::run_cgm01_checks(),
        cgm02_display_checks::run_cgm02_checks()
    ),
        crate::checks::CheckSet::merge(
        vcj01_powerarch_checks::run_vcj01_checks(),
        crate::checks::CheckSet::merge(
        vcl01_virtualarch_checks::run_vcl01_checks(),
        vcq01_reliability_checks::run_vcq01_checks()
    )
    )
    )
    ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        vcq02_metrics_checks::run_vcq02_checks(),
        cgd02_frametimer_checks::run_cgd02_checks()
    ),
        crate::checks::CheckSet::merge(
        cgd03_frameledger_checks::run_cgd03_checks(),
        crate::checks::CheckSet::merge(
        cgp01_adaptivearch_checks::run_cgp01_checks(),
        cgp02_unifiedmodel_checks::run_cgp02_checks()
    )
    )
    ),
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        vco01_degrade_checks::run_vco01_checks(),
        crate::checks::CheckSet::merge(
        cgr01_secure_checks::run_cgr01_checks(),
        cgr02_threatmodel_checks::run_cgr02_checks()
    )
    ),
        crate::checks::CheckSet::merge(
        cgm03_edid_checks::run_cgm03_checks(),
        crate::checks::CheckSet::merge(
        vco03_triggersrc_checks::run_vco03_checks(),
        vcw04_apiref_checks::run_vcw04_checks(),
        cgp07_jmerge_checks::run_cgp07_checks()
    )
    )
    )
    )
    )
}
}
