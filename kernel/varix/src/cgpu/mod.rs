//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。

pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
//! | [`cgi01_bandwidth`] | CGPU-F1281 | I 域开工与带宽总架构（**五主题闭集**PCIe带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因冻结、本域160项全挂靠表外不立题；**账户化**四类传输（纹理上传/下载/交换/流送）各开预算账户——每笔传输先申请余额不足即拒绝并计数留痕、退款只做消费迁移、账本守恒净消费口径自检；**边界**H域管显存驻留/I域管传输带宽/VE-AD存储流F5801+管到达三段各管一段、接口复用F0405集成面声明逐句可grep；**十组**规划表组名+起止单号逐字取自总纲批次锚点、F1281-F1440首尾连续无缺口、宣告行读屏可查；零panic面零IO零墙钟） |
//! | [`cgi01_bandwidth_checks`] | CGPU-F1281 域自检（判据五条映射：五主题闭集+标签独立重排全等、账户化申请/拒绝/退款/守恒全链+未开账户必拒反向语料、边界三域分工逐句grep、十组表与判据侧独立重排逐条全等+组间起止连续校验；守恒式与使用率判据侧独立重算） |
//! | [`vco01_degrade`] | CGPU-F2241 O 域开工与降级链总架构（优雅降级哲学三条款字面量冻结——有设计的/可预期/可恢复；N 域移交包 F2237 七件签收记录 HandoffReceipt 四字段+指纹 const 期 FNV 钉死；官方五主题（降级模型/决策引擎/链路全图/可观测/生态）十组映射表 O01..O10 区间 2241-2400 连续守恒 160；架构五段单向流水线 触发→决策→执行→恢复→观测 跳段/回退显性码拒；历史汇总五行（D 域掉帧链/J03 热阶梯/J04 续航/Q02 流送/N01 效果降级）+统一编排声明 O 域=全域降级的统一编排层各域保留专业降级逻辑+收编双向闸表外孤儿立案；双不变量底线闸（80 帧适配表/信息完整性）恰端点行为钉死；风险四条（决策振荡/链路冲突/恢复过冲/可见性失控）预案互异非空；码段 0x55xx 独占） |
//! | [`vco01_degrade_checks`] | CGPU-F2241 域自检（16 条判据两族：使命+签收+映射+五段+统一编排+不变量+风险+摘要 11 / 判据承载力 5；哲学三条款+历史五行+风险四名+组名抽检判据侧独立写死对拍、签收四字段齐备+指纹篡改被拒+判据侧手写 FNV 双源对账、十组守恒独立重算+主题覆盖双算、五段全序列推进+跳/退/原地显性码拒、底线闸 79/80 拒 80/80 过、码段 0x55 独占、零 panic 双面自扫、聚合防自调） |
//! | [`cgd02_frametimer`] | CGPU-F0482 | 全帧精确计时器（**四段**输入采样→提交→GPU执行→呈现上屏全链分段闭段才出时长、重叠/倒序记账异常可判定检出；**双源**QPC/GPU时间戳闭集、同段双源跨度对账漂移超容差标注；**校准**CPU/GPU时钟域线性对齐两对样本完全确定映射、换算残差逐点给账超阈标注（换算误差可测）非单调样本拒绝标定；**开销**<0.1%帧预算实测值注入核算640ppm手算对账超标标注；零墙钟——时间戳全上游注入计时器只做账务对齐异常检测、零panic面零IO） |
//! | [`cgd02_frametimer_checks`] | CGPU-F0482 域自检（判据五条映射：四段开合时长+未闭段None反向+重叠倒序异常、双源闭集+漂移控制、线性标定+残差逐点+反向拒绝、开销实测核算+红线标注；时长/比率/ppm全部判据侧手算写死对账；stamp独立重排全等） |
//! | [`cgd03_frameledger`] | CGPU-F0483 | 帧时间账本（**完整记录**四段+帧间隔+场景标签+版本逐帧入账、缺段照记None不补造不跳过；**同schema**五元组（名称/维度/单位/类型/采样）与F0274/B08同构不第二套口径、三级命名d01.frameledger.*前缀逐条grep、schema版本进记录演进不破坏历史；**滚动**256帧逐帧配额恒定、超限退场进聚合桶min/max/sum/mean聚合不是丢弃；**查询**Exact/Aggregated/NotFound三答案各有其位没有第四种「查不到就说零」；零panic面零IO零墙钟） |
//! | [`cgd03_frameledger_checks`] | CGPU-F0483 域自检（判据五条映射：完整记录+缺段如实+间隔手算对账、schema表与判据侧独立重排逐条全等+前缀grep+记录行按位对齐、滚动配额恒定+退场聚合min/max/sum手算对账、三答案各有其位+未记录NotFound反向；stamp独立重排全等） |
//! | [`cgp01_adaptivearch`] | CGPU-F2401 | P 域开工与自适应遥测总架构（**使命**让遥测本身成为自适应系统——按需采集/按价值存储/按洞察呈现三条字面量冻结、遥测有预算意识不做帧预算隐形税；**签收**O域移交包F2397结构化记录来源/单号/项数/状态四元在案；**兑现起点**F1453/F1553/F2216三处遥测预留逐条认领Pending、表外查询None不臆造；**映射**官方五主题闭集（统一模型/采样策略/数据管道/分析洞察/看板）+十组规划表F2401-F2560逐字对账首尾连续；**五段**统一模型→自适应采集→管道→分析→消费单向流转、跳段/回退/原地双向拒绝末段AtEnd；**基座**J08/K07/O04三域遥测收编统一P=基座各域=生产者收编单向零反向依赖；**红线**本地默认/匿名化/用户控制三条硬约束逐条grep；**风险**数据量/口径漂移/隐私/性能四条预案两两互异；零panic面零IO零墙钟） |
//! | [`cgp01_adaptivearch_checks`] | CGPU-F2401 域自检（锚点判据十条映射八组+stamp：使命三条/签收四元/三预留认领+表外反向/五主题十组独立重排全等连续校验/五段流转双向拒绝/三域收编/红线三条/风险四条互异；判据侧字面量写死对拍） |
pub mod cgd02_frametimer;
pub mod cgd02_frametimer_checks;
pub mod vcj01_powerarch;
pub mod vcj01_powerarch_checks;
pub mod cga02_threadpool;
pub mod cga02_threadpool_checks;
pub mod vcl01_virtualarch;
pub mod vcl01_virtualarch_checks;
pub mod vco01_degrade;
pub mod vco01_degrade_checks;
pub mod vcq01_reliability;
pub mod vcq01_reliability_checks;
pub mod cgm01_display;
pub mod cgm01_display_checks;
//! cgm01_display —— CGPU-F1921 M 域开工与显示输出总架构（最后一厘米三条款/L 域 F1917 签收七件/五主题十组映射覆盖守恒/五段单向流水线/F1768 贯通+V 域协同/呈现预算合同终端/风险四条预案/0x54xx 七码）
pub mod cgm02_display;
pub mod cgm02_display_checks;
pub mod cgd03_frameledger_checks;
pub mod cgd03_frameledger;
pub mod cgp01_adaptivearch_checks;
pub mod cgp01_adaptivearch;
pub mod cgr01_secure;
pub mod cgr01_secure_checks;
//! cgr01_secure —— CGPU-F2721 R 域开工与安全渲染总架构（Q10 移交包 F2720 签收七件+两项附告逐项对账/不可信内容渲染防线定位两条款字面量冻结/防线边界表八条入出表裁决双向/四段单向流水线验证→隔离→降级→恢复跳段回退拒绝含污染回流路径/Q10 预告兑现确认四字段齐与签收互证/域区间 2721~2880 守恒/0x5701~0x5706 六码独占互异）
//! cgm02_display —— CGPU-F1922 显示枚举与热插拔（枚举能力快照 id+EDID 指纹+seq 三件齐/去抖 leading-edge 恰端点/竞态=缺陷红线序列号恰一+快照新鲜度/三态单向状态机 mode-set 幂等/批量固定序去重重跑一致/0x5408~0x540C 域段续占互异）
//! vcq01_reliability — CGPU-F2561 Q 域开工与可靠性总架构（恢复起点哲学三条款/P 域移交包 F2557 七件签收/五主题十组映射/五段单向流水线/O 域交接+混沌设施复用/不可恢复=最高缺陷红线+立案码/风险四条预案互异/0x56xx 六码/19 项域自检）
//! vcl01_virtualarch — CGPU-F1761 L 域开工与虚拟化总架构（域使命三条款平等声明/八主题十组映射/K 域签收+I09 预留兑现/五段单向流水线/复用不重建 1601→1763+1466→1776/模式×合同等级表/场景四族/风险四条预案互异/0x53xx 五码/22 项域自检）
//! cga02_threadpool —— CGPU-F0002 渲染线程池与工作窃取调度器（池规模核数减二/合成器专核隔离/同层优先窃取偷头不偷尾/瓦片独立缓冲确定性归并乱序逐像素一致/协作式 2ms 让位抢占三档优先/均衡≤5%/遥测三面/窃取开销≤3%账面/0x39xx 域码段）
//! vcj01_powerarch — CGPU-F1441 J 域开工与功耗架构总览（域使命/五主题十组/三处核验/五段流水线/边界/采样不耗样本/风险回退/0x52xx）

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {
    crate::checks::CheckSet::merge(
            crate::checks::CheckSet::merge(
            crate::checks::CheckSet::merge(
            cgi01_bandwidth_checks::run_cgi01_checks(),
            crate::checks::CheckSet::merge(
            cgd02_frametimer_checks::run_cgd02_checks(),
            vcj01_powerarch_checks::run_vcj01_checks(),
        ),
        ),
            crate::checks::CheckSet::merge(
            cga02_threadpool_checks::run_cga02_checks(),
            crate::checks::CheckSet::merge(
            vcl01_virtualarch_checks::run_vcl01_checks(),
            vcq01_reliability_checks::run_vcq01_checks(),
        ),
        ),
        ),
            crate::checks::CheckSet::merge(
            crate::checks::CheckSet::merge(
            cgm01_display_checks::run_cgm01_checks(),
            cgr01_secure_checks::run_cgr01_checks(),
            crate::checks::CheckSet::merge(
            cgm02_display_checks::run_cgm02_checks(),
            vco01_degrade_checks::run_vco01_checks(),
        ),
        ),
            crate::checks::CheckSet::merge(
            cgd02_frametimer_checks::run_cgd02_frametimer(),
            crate::checks::CheckSet::merge(
            cgd03_frameledger_checks::run_cgd03_frameledger(),
            cgp01_adaptivearch_checks::run_cgp01_adaptivearch(),
            cgm01_display_checks::run_cgm01_checks(),
            cgm02_display_checks::run_cgm02_checks(),
        ),
        ),
        ),
        )
}
