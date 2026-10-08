//! CGPU 册模块登记（本文件由 CGPU-F1281 开工建立；后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433）。

pub mod cgi01_bandwidth;
pub mod cgi01_bandwidth_checks;
//! | [`cgi01_bandwidth`] | CGPU-F1281 | I 域开工与带宽总架构（**五主题闭集**PCIe带宽仲裁/共享内存带宽/上传下载调度/带宽遥测/瓶颈归因冻结、本域160项全挂靠表外不立题；**账户化**四类传输（纹理上传/下载/交换/流送）各开预算账户——每笔传输先申请余额不足即拒绝并计数留痕、退款只做消费迁移、账本守恒净消费口径自检；**边界**H域管显存驻留/I域管传输带宽/VE-AD存储流F5801+管到达三段各管一段、接口复用F0405集成面声明逐句可grep；**十组**规划表组名+起止单号逐字取自总纲批次锚点、F1281-F1440首尾连续无缺口、宣告行读屏可查；零panic面零IO零墙钟） |
//! | [`cgi01_bandwidth_checks`] | CGPU-F1281 域自检（判据五条映射：五主题闭集+标签独立重排全等、账户化申请/拒绝/退款/守恒全链+未开账户必拒反向语料、边界三域分工逐句grep、十组表与判据侧独立重排逐条全等+组间起止连续校验；守恒式与使用率判据侧独立重算） |
//! | [`cgd02_frametimer`] | CGPU-F0482 | 全帧精确计时器（**四段**输入采样→提交→GPU执行→呈现上屏全链分段闭段才出时长、重叠/倒序记账异常可判定检出；**双源**QPC/GPU时间戳闭集、同段双源跨度对账漂移超容差标注；**校准**CPU/GPU时钟域线性对齐两对样本完全确定映射、换算残差逐点给账超阈标注（换算误差可测）非单调样本拒绝标定；**开销**<0.1%帧预算实测值注入核算640ppm手算对账超标标注；零墙钟——时间戳全上游注入计时器只做账务对齐异常检测、零panic面零IO） |
//! | [`cgd02_frametimer_checks`] | CGPU-F0482 域自检（判据五条映射：四段开合时长+未闭段None反向+重叠倒序异常、双源闭集+漂移控制、线性标定+残差逐点+反向拒绝、开销实测核算+红线标注；时长/比率/ppm全部判据侧手算写死对账；stamp独立重排全等） |
pub mod cgd02_frametimer;
pub mod cgd02_frametimer_checks;
pub mod vcj01_powerarch;
pub mod vcj01_powerarch_checks;
pub mod cga02_threadpool;
pub mod cga02_threadpool_checks;
pub mod vcl01_virtualarch;
pub mod vcl01_virtualarch_checks;
//! vcl01_virtualarch — CGPU-F1761 L 域开工与虚拟化总架构（域使命三条款平等声明/八主题十组映射/K 域签收+I09 预留兑现/五段单向流水线/复用不重建 1601→1763+1466→1776/模式×合同等级表/场景四族/风险四条预案互异/0x53xx 五码/22 项域自检）
//! cga02_threadpool —— CGPU-F0002 渲染线程池与工作窃取调度器（池规模核数减二/合成器专核隔离/同层优先窃取偷头不偷尾/瓦片独立缓冲确定性归并乱序逐像素一致/协作式 2ms 让位抢占三档优先/均衡≤5%/遥测三面/窃取开销≤3%账面/0x39xx 域码段）
//! vcj01_powerarch — CGPU-F1441 J 域开工与功耗架构总览（域使命/五主题十组/三处核验/五段流水线/边界/采样不耗样本/风险回退/0x52xx）

/// CGPU 域自检聚合（在账判据集合并，供下游/探针一条命令调用）。
pub fn run_cgpu_checks() -> crate::checks::CheckSet {

    crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
        crate::checks::CheckSet::merge(
            cgi01_bandwidth_checks::run_cgi01_checks(),
            cgd02_frametimer_checks::run_cgd02_checks(),
        ),
        crate::checks::CheckSet::merge(
            vcj01_powerarch_checks::run_vcj01_checks(),
        crate::checks::CheckSet::merge(
            cga02_threadpool_checks::run_cga02_checks(),
            vcl01_virtualarch_checks::run_vcl01_checks(),
        ),
        ),
        ),
    )
}
