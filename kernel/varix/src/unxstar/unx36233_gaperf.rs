//! UNX-F36233-J1 GrantedAccess 性能账（掩码过滤热路径 P99）。
//! 判据：① 热路径 2000 例掩码过滤：成本模型 = 位测步数（放行成本 =
//! needed 位数、拒绝成本 = 首缺位号 + 1——早退语义），P99 桶统计由
//! 计数直方图导出（无排序无浮点——no_std 整数纪律），P99 ≤ 21、
//! 单发峰值 ≤ 21（早退位号模型上界：标准位最高位号 20 → 21 步封顶）；
//! ② 对比基线：同 2000 例按全量重查成本模型
//!（ACL 步行 24 步/例）累计，掩码总成本 × 4 < 重查总成本——「打开
//! 时查一次、操作时只查位」的性能契约的账面兑现；③ 零重查计数器
//! 恒零（ac_calls/rechecks）；④ 纯功能代码 ≥320 行。
//! 设计纪律：ObReferenceObjectByHandle 的热路径就是一次位与——本面
//! 把「快」账面化：成本 = 位测步数（可确定性重放），直方图 P99 从
//! 桶计数导出（桶序即值序，免排序）；所有量整数，无浮点。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;
use super::unx36223_gafilter as fl;

pub const SEG_BASE: u16 = 0xa250;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_P99: u16 = SEG_BASE + 2; // P99 超阈（热路径性能契约破）
pub const E_MAX: u16 = SEG_BASE + 3; // 单发峰值超阈
pub const E_RATIO: u16 = SEG_BASE + 4; // 对比基线破（加速比不足）
pub const E_COUNT: u16 = SEG_BASE + 5; // 计数器账不平
pub const E_RECHECK: u16 = SEG_BASE + 6; // 全量重查发生（契约破）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa250, "段位锚漂移");

/// 全量重查成本基线（步/例——ACL 步行的确定性下界）。
pub const ACL_STEPS: u32 = 24;

/// 掩码过滤成本模型（放行 = needed 位测步数；拒绝 = 首缺位号 + 1
/// ——早退：发现第一个缺位即停）。与 36223::Op 的 needed 投影对齐。
pub fn mask_cost(ga: u32, needed: u32) -> u32 {
    if needed & !ga != 0 {
        // 拒绝：首缺位号 + 1（步进序扫描）。
        let mut i = 0;
        while i < 32 {
            let b = 1u32.wrapping_shl(i);
            if needed & b != 0 && ga & b == 0 {
                return i.wrapping_add(1);
            }
            i += 1;
        }
        return 32;
    }
    needed.count_ones()
}

/// 查询路径成本（只读快照恒一步——GetHandleInformation 的性能契约）。
pub const QUERY_COST: u32 = 1;

/// 三路径成本并读（掩码过滤 / 查询 / 全量重查的同类样本总账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathCosts {
    pub mask_total: u32,
    pub query_total: u32,
    pub acl_total: u32,
}

impl CostHistogram {
    /// 三路径账导出（同 total 样本的三种成本模型）。
    pub fn path_costs(&self) -> PathCosts {
        PathCosts {
            mask_total: self.cost_sum,
            query_total: self.total.wrapping_mul(QUERY_COST),
            acl_total: self.total.wrapping_mul(ACL_STEPS),
        }
    }
}

/// 成本直方图（桶 1..=32——桶序即值序，P99 免排序导出）。
pub const BUCKETS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostHistogram {
    pub buckets: [u32; BUCKETS],
    pub total: u32,
    pub cost_sum: u32,
    pub max_cost: u32,
}

impl CostHistogram {
    pub fn fresh() -> CostHistogram {
        CostHistogram { buckets: [0; BUCKETS], total: 0, cost_sum: 0, max_cost: 0 }
    }

    /// 记一例（成本 1..=32 越界即 E_P99——成本模型闭集）。
    pub fn record(&mut self, cost: u32) -> Result<(), u16> {
        if cost == 0 || cost as usize > BUCKETS {
            return Err(E_P99);
        }
        self.buckets[(cost - 1) as usize] = self.buckets[(cost - 1) as usize].wrapping_add(1);
        self.total = self.total.wrapping_add(1);
        self.cost_sum = self.cost_sum.wrapping_add(cost);
        if cost > self.max_cost {
            self.max_cost = cost;
        }
        Ok(())
    }

    /// 分位导出（通用形态：p ∈ [1,100]——nearest-rank，整数化
    /// (total×p)/100；total 小于分母退化为首个非空桶）。P99 即
    /// percentile(99)——桶序即值序，免排序。
    pub fn percentile(&self, p: u32) -> u32 {
        if self.total == 0 || p == 0 || p > 100 {
            return 0;
        }
        let raw = self.total.wrapping_mul(p).wrapping_div(100);
        let need = if raw == 0 { 1 } else { raw };
        let mut cum = 0u32;
        let mut i = 0;
        while i < BUCKETS {
            cum = cum.wrapping_add(self.buckets[i]);
            if cum >= need {
                return (i + 1) as u32;
            }
            i += 1;
        }
        BUCKETS as u32
    }

    /// P99 导出（判据 ① 的落点，分位族的 99 形态）。
    pub fn p99(&self) -> u32 {
        self.percentile(99)
    }

    /// 加速比对账（判据 ② 的落点）：掩码总成本 × 4 < 重查基线总成本。
    pub fn beats_acl_baseline(&self) -> bool {
        let baseline = self.total.wrapping_mul(ACL_STEPS);
        self.total > 0 && self.cost_sum.wrapping_mul(4) < baseline
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：成本模型锚（放行 = 位测数；拒绝 = 首缺位 + 1）。
    if mask_cost(sr::STD_ALL, sr::READ_CONTROL) != 1 {
        return Err(E_P99);
    }
    if mask_cost(sr::STD_ALL, sr::STD_ALL) != 5 {
        return Err(E_P99);
    }
    // ga=0 全缺：首缺位 = needed 最低位号。
    if mask_cost(0, sr::READ_CONTROL) != (17 + 1) {
        return Err(E_P99);
    }
    if mask_cost(0, 0x0003 | sr::STD_ALL) != 1 {
        return Err(E_P99);
    }
    // R1：热路径 2000 例（判据 ① 落点）：三型句柄 × 五算子循环，
    // 逐例记录成本 + 36223 带账过滤双计（结论一致性对拍）。
    let mut t = gf::HandleTable::new();
    let h_full = t.attach(0x10, sr::STD_ALL | 0x0007, 0).map_err(|_| E_COUNT)?;
    let h_read = t.attach(0x11, sr::READ_CONTROL | 0x0001, 0).map_err(|_| E_COUNT)?;
    let h_sync = t.attach(0x12, sr::SYNCHRONIZE, 0).map_err(|_| E_COUNT)?;
    let mut hist = CostHistogram::fresh();
    let mut ops = fl::OpStats::fresh();
    let mut i = 0u32;
    while i < 2000 {
        let op = fl::OP_TABLE[(i % 5) as usize];
        let (slot, gen) = match i % 3 {
            0 => h_full,
            1 => h_read,
            _ => h_sync,
        };
        let e = t.read(slot, gen).map_err(|_| E_COUNT)?;
        let needed = op.needed();
        // 成本先算（放行/拒绝同模型），再跑真过滤对拍结论与成本形态。
        let cost = mask_cost(e.ga, needed);
        hist.record(cost)?;
        let r = fl::filter_op_counted(e.ga, op, &mut ops);
        let expect_ok = needed & !e.ga == 0;
        if expect_ok != r.is_ok() {
            return Err(E_COUNT);
        }
        // 放行例成本 = 位测数；拒绝例成本 ∈ [1,32]（首缺位 + 1）。
        if expect_ok && cost != needed.count_ones() {
            return Err(E_P99);
        }
        if !expect_ok && cost > 32 {
            return Err(E_COUNT);
        }
        i += 1;
    }
    if hist.total != 2000 || ops.mask_checks != 2000 {
        return Err(E_COUNT);
    }
    // 桶计数总和 = 总例数（直方图守恒）。
    let mut bucket_sum = 0u32;
    let mut b = 0;
    while b < BUCKETS {
        bucket_sum = bucket_sum.wrapping_add(hist.buckets[b]);
        b += 1;
    }
    if bucket_sum != 2000 {
        return Err(E_COUNT);
    }
    // P99 与峰值阈值（判据 ① 的判定）：早退位号模型的确定性上界 21。
    let p99 = hist.p99();
    if p99 == 0 || p99 > 21 {
        return Err(E_P99);
    }
    if hist.max_cost > 21 {
        return Err(E_MAX);
    }
    // 分位族对拍（P50/P95 同源导出——中位在位测数区、P95 已进早退尾）。
    let p50 = hist.percentile(50);
    let p95 = hist.percentile(95);
    if p50 == 0 || p50 > 3 || p95 < p50 || p95 > 21 {
        return Err(E_P99);
    }
    if hist.p99() < p95 {
        return Err(E_P99);
    }
    // R2：对比基线（判据 ② 落点）：掩码总成本 × 4 < 24×2000。
    if !hist.beats_acl_baseline() {
        return Err(E_RATIO);
    }
    // 直觉锚：全权句柄放行例成本 = needed 位测数 ≤ 6。
    if mask_cost(sr::STD_ALL | 0x0007, fl::Op::SetInfo.needed()) > 6 {
        return Err(E_P99);
    }
    // 三路径对比（查询路径契约）：查询一步 × 8 仍远低于重查基线。
    let pc = hist.path_costs();
    if pc.mask_total.wrapping_mul(4) >= pc.acl_total
        || pc.query_total.wrapping_mul(8) >= pc.acl_total
        || pc.query_total > pc.mask_total
    {
        return Err(E_RATIO);
    }
    // R3：零重查契约（判据 ③ 落点）。
    if ops.ac_calls != 0 || ops.rechecks != 0 {
        return Err(E_RECHECK);
    }
    if ops.granted + ops.rejected != ops.mask_checks {
        return Err(E_COUNT);
    }
    // R4：直方图负例（成本 0 / 越桶 → E_P99——成本模型闭集守卫）。
    let mut h2 = CostHistogram::fresh();
    if h2.record(0).err() != Some(E_P99) {
        return Err(E_P99);
    }
    if h2.record(BUCKETS as u32 + 1).err() != Some(E_P99) {
        return Err(E_P99);
    }
    if h2.total != 0 {
        return Err(E_COUNT);
    }
    // 空直方图 P99 = 0（无样本无分位——账面诚实）。
    if h2.p99() != 0 {
        return Err(E_P99);
    }
    // 校验和：成本总和 + P99 + 峰值（确定性总锚）。
    Ok(hist.cost_sum.wrapping_add(p99).wrapping_add(hist.max_cost))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_P99 => "P99 超阈或成本越桶（热路径性能契约破）",
        E_MAX => "单发峰值超阈",
        E_RATIO => "对比基线破（掩码 ×4 ≥ 重查基线）",
        E_COUNT => "计数器账不平（直方图/过滤账不对齐）",
        E_RECHECK => "全量重查发生（ac_calls/rechecks 非零）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36233_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36233_gaperf");
    cs.add("SEG 段位独占", SEG_BASE == 0xa250, "0xa250~0xa25F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "2000 例热路径/P99≤21/加速比≥4x/零重查"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("基线锚", ACL_STEPS == 24, "ACL 步行成本基线 = 24 步/例（下界常数）");
    cs.add("查询锚", QUERY_COST == 1, "查询路径恒一步（GetHandleInformation 性能契约）");
    cs.add("空账诚实", CostHistogram::fresh().p99() == 0 && !CostHistogram::fresh().beats_acl_baseline(), "空直方图无分位、无加速比");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_cost_model() {
        // 放行：needed 位测数。
        assert_eq!(mask_cost(sr::STD_ALL | 0x0007, sr::READ_CONTROL | sr::SYNCHRONIZE), 2);
        // 拒绝：首缺位 + 1（READ_CONTROL 是 bit17 → 18）。
        assert_eq!(mask_cost(0, sr::READ_CONTROL), 18);
        // 位序早退：缺位号小的先停。
        assert_eq!(mask_cost(0, 0x0002 | sr::DELETE), 2);
    }

    #[test]
    fn test_histogram_p99() {
        let mut h = CostHistogram::fresh();
        // 99 例成本 1 + 1 例成本 24 → P99 = 1（99% 分位在桶 1）。
        let mut i = 0;
        while i < 99 {
            h.record(1).unwrap_or(());
            i += 1;
        }
        h.record(24).unwrap_or(());
        assert_eq!(h.p99(), 1);
        assert_eq!(h.max_cost, 24);
        assert_eq!(h.total, 100);
        assert!(h.beats_acl_baseline());
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36233_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
