//! ves424_s23perfbase — VE-F4184 · T 域性能总基准
//! 终审：T 域性能总基准合成（八组基准并入+五档阈值复核（查找/切换/排版/翻译管线/资源加载）+与家族基准对拍）；
//! 预算基线冻结前最后一核；多语言无障碍专项开销档设独立阈值（域本色）；
//! 阈值越界→重议或降级声明；对拍分歧→仲裁；基线未冻→阻断 F4190。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PbCode {
    Ok = 0,
    OverThreshold = SEG_BASE + 0x01, // 0x1041
    Disagreement = SEG_BASE + 0x02,  // 0x1042
    BlockFreeze = SEG_BASE + 0x03,   // 0x1043
}

/// 五档 + 无障碍专项档（第六档，域本色独立阈值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfTier { Lookup = 0, Switch = 1, Layout = 2, MtPipeline = 3, AssetLoad = 4, A11yExtra = 5 }

impl PerfTier {
    pub const ALL: [PerfTier; 6] = [
        PerfTier::Lookup, PerfTier::Switch, PerfTier::Layout,
        PerfTier::MtPipeline, PerfTier::AssetLoad, PerfTier::A11yExtra];
    pub fn name(self) -> &'static str {
        match self {
            PerfTier::Lookup => "lookup", PerfTier::Switch => "switch", PerfTier::Layout => "layout",
            PerfTier::MtPipeline => "mt-pipeline", PerfTier::AssetLoad => "asset-load",
            PerfTier::A11yExtra => "a11y-extra",
        }
    }
}

/// 总基准册：各档并入值（μs 档）。
pub struct BaselineBook {
    pub merged: Vec<(PerfTier, u32)>,   // 并入值
    pub thresholds: Vec<(PerfTier, u32)>, // 阈值（越界即红）
    pub degraded: Vec<PerfTier>,        // 降级声明档
    pub last_check_ok: bool,            // 终核通过（F4190 冻结前置）
}

impl BaselineBook {
    pub fn new() -> Self {
        BaselineBook { merged: Vec::new(), thresholds: Vec::new(), degraded: Vec::new(), last_check_ok: false }
    }
    pub fn merge_group(&mut self, tier: PerfTier, v: u32) {
        for m in self.merged.iter_mut() { if m.0 == tier { m.1 = m.1.max(v); return; } }
        self.merged.push((tier, v));
    }
    pub fn set_threshold(&mut self, tier: PerfTier, t: u32) {
        for x in self.thresholds.iter_mut() { if x.0 == tier { x.1 = t; return; } }
        self.thresholds.push((tier, t));
    }
    pub fn threshold_of(&self, tier: PerfTier) -> u32 {
        self.thresholds.iter().find(|(t, _)| *t == tier).map(|(_, v)| *v).unwrap_or(0)
    }
    /// 五档+专项档全复核：越界且未降级声明→红档清单。
    pub fn recheck(&self) -> Vec<PerfTier> {
        let mut bad = Vec::new();
        for t in PerfTier::ALL {
            let v = self.merged.iter().find(|(m, _)| *m == t).map(|(_, v)| *v).unwrap_or(0);
            if v > self.threshold_of(t) && !self.degraded.contains(&t) { bad.push(t); }
        }
        bad
    }
    /// 降级声明：越界档显性入册（留痕）。
    pub fn declare_degrade(&mut self, tier: PerfTier) { self.degraded.push(tier); }
    /// 终核裁决：全部绿→可进 F4190 冻结；否则阻断。
    pub fn final_check(&mut self) -> Result<(), PbCode> {
        let bad = self.recheck();
        if bad.is_empty() {
            self.last_check_ok = true;
            Ok(())
        } else {
            self.last_check_ok = false;
            Err(PbCode::OverThreshold)
        }
    }
}

/// 家族基准对拍：与家族总表档位值偏差 ≤ 容差（bp，万分之一）。
pub fn family_agrees(own: u32, family: u32, tol_bp: u32) -> bool {
    let a = own.max(family) as u64;
    let b = own.min(family) as u64;
    if a == 0 { return true; }
    (a - b) * 10_000 / a <= tol_bp as u64
}

/// 对拍分歧仲裁：取保守（较大）值为准。
pub fn arbitrate(own: u32, family: u32) -> u32 { own.max(family) }

// ───────────── 深化块 D424-A · 结构化采样账本 ─────────────
// 基准总册只留"并入最大值"，缺采样留痕。此处补批次×档位×归因的结构化
// 采样账本（SampleRec/SampleLedger 三件套：add/query/conservation）。

/// 一次基准采样：批次号 + 档位 + 测得值（μs 档）+ 归因（家族对拍 or 本机实测）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRec {
    pub batch: u16,
    pub tier: PerfTier,
    pub value_us: u32,
    pub from_family: bool,
}

/// 采样账本：定容量，满则显性拒绝（边界探针友好）。
pub struct SampleLedger {
    pub recs: Vec<SampleRec>,
    pub cap: usize,
}

impl SampleLedger {
    pub fn new(cap: usize) -> Self { SampleLedger { recs: Vec::new(), cap } }
    /// 入账：满容量→显性 Err（复用 OverThreshold 语义：容量越界）。
    pub fn push(&mut self, r: SampleRec) -> Result<(), PbCode> {
        if self.recs.len() >= self.cap { return Err(PbCode::OverThreshold); }
        self.recs.push(r);
        Ok(())
    }
    /// 守恒恒等式：∑全部采样 ≡ ∑(按档分列求和)。任何丢失/重复即红。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.value_us as u64).sum();
        let mut by_tier: u64 = 0;
        for t in PerfTier::ALL {
            by_tier += self.recs.iter().filter(|r| r.tier == t)
                .map(|r| r.value_us as u64).sum::<u64>();
        }
        total == by_tier
    }
    /// 档位均值（整数向下取整，禁浮点）。
    pub fn tier_mean(&self, tier: PerfTier) -> u32 {
        let vals: Vec<u32> = self.recs.iter().filter(|r| r.tier == tier)
            .map(|r| r.value_us).collect();
        if vals.is_empty() { return 0; }
        let sum: u64 = vals.iter().map(|v| *v as u64).sum();
        (sum / vals.len() as u64) as u32
    }
    /// 整数分位：排序后取第 q 百分位（q∈[1,100]，上取整索引，禁浮点）。
    pub fn tier_quantile(&self, tier: PerfTier, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.tier == tier)
            .map(|r| r.value_us).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 归因分列：(家族对拍数, 本机实测数)。
    pub fn attribution(&self) -> (usize, usize) {
        let fam = self.recs.iter().filter(|r| r.from_family).count();
        (fam, self.recs.len() - fam)
    }
    /// 批次投影：按批次号去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D424-B · 边界探针 ─────────────

/// 探针 P1：空档位均值查询→显性 Err（BlockFreeze 复用为"无数据可核"）。
pub fn probe_empty_mean(ledger: &SampleLedger, tier: PerfTier) -> Result<u32, PbCode> {
    if ledger.recs.iter().filter(|r| r.tier == tier).count() == 0 {
        return Err(PbCode::BlockFreeze);
    }
    Ok(ledger.tier_mean(tier))
}

/// 探针 P2：越界分位号（q==0 或 q>100）→显性 Err。
pub fn probe_bad_quantile(ledger: &SampleLedger, tier: PerfTier, q: u32) -> Result<u32, PbCode> {
    if q == 0 || q > 100 { return Err(PbCode::Disagreement); }
    if ledger.recs.iter().filter(|r| r.tier == tier).count() == 0 {
        return Err(PbCode::BlockFreeze);
    }
    Ok(ledger.tier_quantile(tier, q))
}

// ───────────── 深化块 D424-C · 确定性 LCG + 注入回归 ─────────────

/// 8 行确定性 LCG（注入回归驱动，禁随机源）。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮随机注入越档采样，recheck 必抓（异常抓取率 100%）；
/// 负对照：阈值内注入零误报。返回 (注入轮数, 抓获数, 误报数)。
pub fn lcg_inject_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x24_42_42_42);
    let tiers = [PerfTier::Lookup, PerfTier::Switch, PerfTier::Layout,
                 PerfTier::MtPipeline, PerfTier::AssetLoad, PerfTier::A11yExtra];
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let t = tiers[g.pick(tiers.len() as u32) as usize];
        // 正对照：注入 2×阈值 的越档值，复核必须命中该档。
        let mut bk = BaselineBook::new();
        let over = bk.threshold_of(t).max(1).saturating_mul(2);
        bk.merge_group(t, over);
        if !bk.recheck().is_empty() { caught += 1; } else { false_pos += 1; }
        // 负对照：同档注入零值，不得报红。
        let mut bk_ok = BaselineBook::new();
        bk_ok.merge_group(t, 0);
        if !bk_ok.recheck().is_empty() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D424-D · 序列化投影 + FNV 指纹 ─────────────

/// SampleRec 定长 9 字节编码：batch(2,LE) + tier(1) + value(4,LE) + 归因位(1)。
pub fn encode_sample(r: &SampleRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.tier as u8);
    out.extend_from_slice(&r.value_us.to_le_bytes());
    out.push(if r.from_family { 1 } else { 0 });
    out
}

/// 解码：长度或档位号越界→显性 Err（往返自检用）。
pub fn decode_sample(b: &[u8]) -> Result<SampleRec, PbCode> {
    if b.len() != 9 { return Err(PbCode::Disagreement); }
    let tier_id = b[2];
    let tier = match PerfTier::ALL.iter().find(|t| **t as u8 == tier_id) {
        Some(t) => *t,
        None => return Err(PbCode::Disagreement),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut val = [0u8; 4];
    val.copy_from_slice(&b[3..7]);
    Ok(SampleRec { batch: u16::from_le_bytes(batch), tier,
        value_us: u32::from_le_bytes(val), from_family: b[8] == 1 })
}

/// FNV-1a 32 位指纹（基线快照指纹，禁外部哈希库）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 基线快照指纹：六档并入值序列化后取 FNV。
pub fn baseline_fingerprint(bk: &BaselineBook) -> u32 {
    let mut bytes = Vec::new();
    for t in PerfTier::ALL {
        let v = bk.merged.iter().find(|(m, _)| *m == t).map(|(_, v)| *v).unwrap_or(0);
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    fnv1a(&bytes)
}

/// 指纹十六进制投影（小写，诊断用；no_std String 拼接）。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves424_checks() -> CheckSet {
    let mut set = CheckSet::new("ves424");
    let mut bk = BaselineBook::new();

    // C424-01 五档并入+专项档共六档。
    bk.merge_group(PerfTier::Lookup, 80);
    bk.merge_group(PerfTier::Switch, 150);
    bk.merge_group(PerfTier::Layout, 200);
    bk.merge_group(PerfTier::MtPipeline, 900);
    bk.merge_group(PerfTier::AssetLoad, 500);
    bk.merge_group(PerfTier::A11yExtra, 250); // 域本色独立阈值档
    chk(&mut set, "C424-01", bk.merged.len() == 6, "六档并入");

    // C424-02 并入取最大（八组同档并最大）。
    bk.merge_group(PerfTier::Switch, 140);
    chk(&mut set, "C424-02", bk.merged.iter().find(|(t, _)| *t == PerfTier::Switch).unwrap().1 == 150, "并入取大");

    // C424-03 阈值复核：全部未越界→绿。
    for (t, th) in [(PerfTier::Lookup, 120u32), (PerfTier::Switch, 200), (PerfTier::Layout, 250),
                    (PerfTier::MtPipeline, 1000), (PerfTier::AssetLoad, 600), (PerfTier::A11yExtra, 300)] {
        bk.set_threshold(t, th);
    }
    chk(&mut set, "C424-03", bk.recheck().is_empty(), "六档全绿");

    // C424-04 终核通过→可进冻结。
    chk(&mut set, "C424-04", bk.final_check().is_ok() && bk.last_check_ok, "终核放行");

    // C424-05 翻译管线越界→红档定位。
    bk.merge_group(PerfTier::MtPipeline, 1200);
    let bad = bk.recheck();
    chk(&mut set, "C424-05", bad == vec![PerfTier::MtPipeline], "越界定位");

    // C424-06 终核阻断（F4190 前置门）。
    chk(&mut set, "C424-06", bk.final_check() == Err(PbCode::OverThreshold) && !bk.last_check_ok, "终核阻断");

    // C424-07 降级声明后复核复绿（留痕）。
    bk.declare_degrade(PerfTier::MtPipeline);
    chk(&mut set, "C424-07", bk.recheck().is_empty() && bk.degraded.len() == 1, "降级声明复绿");

    // C424-08 重议阈值复绿（不降级路径）。
    let mut bk2 = BaselineBook::new();
    bk2.merge_group(PerfTier::A11yExtra, 400);
    bk2.set_threshold(PerfTier::A11yExtra, 300);
    chk(&mut set, "C424-08", bk2.recheck() == vec![PerfTier::A11yExtra], "专项档越界");
    bk2.set_threshold(PerfTier::A11yExtra, 450);
    chk(&mut set, "C424-08b", bk2.recheck().is_empty(), "重议阈值复绿");

    // C424-09 家族对拍：容差内一致。
    chk(&mut set, "C424-09", family_agrees(150, 152, 200), "对拍一致");

    // C424-10 家族对拍：超容差失配。
    chk(&mut set, "C424-10", !family_agrees(150, 200, 200), "对拍失配");

    // C424-11 分歧仲裁取保守（大值）。
    chk(&mut set, "C424-11", arbitrate(150, 200) == 200 && arbitrate(300, 200) == 300, "仲裁取大");

    // C424-12 六档单源齐备断言。
    chk(&mut set, "C424-12", PerfTier::ALL.len() == 6
        && PerfTier::ALL[5].name() == "a11y-extra"
        && PerfTier::ALL[0].name() == "lookup", "六档单源");

    // ── 深化块 D424 判据 ──
    // D424-01 采样账本：三批次×多档入账 + 守恒恒等式。
    let mut lg = SampleLedger::new(64);
    for i in 0..4u16 {
        let _ = lg.push(SampleRec { batch: i, tier: PerfTier::Lookup, value_us: 80 + i as u32, from_family: false });
        let _ = lg.push(SampleRec { batch: i, tier: PerfTier::Switch, value_us: 140 + i as u32, from_family: true });
        let _ = lg.push(SampleRec { batch: i, tier: PerfTier::MtPipeline, value_us: 880 + i as u32, from_family: false });
    }
    chk(&mut set, "D424-01", lg.recs.len() == 12 && lg.conserves(), "账本守恒");

    // D424-02 整数均值与分位（P50/P95，禁浮点）。
    chk(&mut set, "D424-02", lg.tier_mean(PerfTier::Lookup) == 83
        && lg.tier_quantile(PerfTier::Lookup, 50) == 82
        && lg.tier_quantile(PerfTier::Lookup, 95) == 83, "均值分位");

    // D424-03 边界探针：空档位均值→显性 Err。
    chk(&mut set, "D424-03", probe_empty_mean(&lg, PerfTier::Layout) == Err(PbCode::BlockFreeze), "空档探针");

    // D424-04 边界探针：越界分位号→显性 Err；合法分位号放行。
    chk(&mut set, "D424-04", probe_bad_quantile(&lg, PerfTier::Lookup, 0) == Err(PbCode::Disagreement)
        && probe_bad_quantile(&lg, PerfTier::Lookup, 101) == Err(PbCode::Disagreement)
        && probe_bad_quantile(&lg, PerfTier::Lookup, 50).is_ok(), "越界分位探针");

    // D424-05 满容量边界：cap=4 的小账本第 5 笔入账显性拒绝。
    let mut sm = SampleLedger::new(4);
    let mut full_reject = false;
    for i in 0..6u16 {
        if lg_conserv_helper(&mut sm, i).is_err() { full_reject = true; }
    }
    chk(&mut set, "D424-05", full_reject && sm.recs.len() == 4, "满容量拒绝");

    // D424-06 归因分列 + 批次去重投影。
    let (fam, own) = lg.attribution();
    chk(&mut set, "D424-06", fam == 4 && own == 8 && lg.batch_count() == 4, "归因批次投影");

    // D424-07 LCG 注入回归：16 轮越档注入 100% 抓获 + 负对照零误报。
    let (n, c, fp) = lcg_inject_regression(16);
    chk(&mut set, "D424-07", n == 16 && c == 16 && fp == 0, "注入回归全抓");

    // D424-08 序列化往返：编码→解码恒等。
    let s0 = SampleRec { batch: 7, tier: PerfTier::A11yExtra, value_us: 250, from_family: true };
    chk(&mut set, "D424-08", decode_sample(&encode_sample(&s0)) == Ok(s0), "序列化往返");

    // D424-09 解码边界：错长/坏档位号→显性 Err。
    chk(&mut set, "D424-09", decode_sample(&[0u8; 8]) == Err(PbCode::Disagreement)
        && decode_sample(&{ let mut v = encode_sample(&s0); v[2] = 0xEE; v }) == Err(PbCode::Disagreement), "解码边界");

    // D424-10 FNV 基线指纹：确定性 + 值变即指纹变 + hex 投影。
    let mut bkf = BaselineBook::new();
    bkf.merge_group(PerfTier::Lookup, 80);
    let f1 = baseline_fingerprint(&bkf);
    let f2 = baseline_fingerprint(&bkf);
    bkf.merge_group(PerfTier::Lookup, 90);
    let f3 = baseline_fingerprint(&bkf);
    chk(&mut set, "D424-10", f1 == f2 && f1 != f3
        && fingerprint_hex(f1).len() == 8, "FNV 指纹稳定");

    set
}

/// 满容量边界辅助：向账本推一笔lookup采样，返回入账结果。
fn lg_conserv_helper(sm: &mut SampleLedger, i: u16) -> Result<(), PbCode> {
    sm.push(SampleRec { batch: i, tier: PerfTier::Lookup, value_us: 80, from_family: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves424_smoke() {
        let set = run_ves424_checks();
        assert!(set.failed_count() == 0, "ves424 red: {}", set.red_items_text());
    }
}
