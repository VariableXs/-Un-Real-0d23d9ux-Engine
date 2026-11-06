//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1965 · transition latency 账（逐档切换延迟测量） · 0x727C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B19）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1965-J1 逐档延迟账格式冻结（分段计时+分位数+冷热
//!     分层对齐 F1773 体例），建模环境逻辑全过，逐档微秒级真机数字随闸门
//!     补测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 逐档账：16 档 × 64 样本，每样本三段计时（压摆/切频/栅障），总延迟 =
//!     三段和（整数微秒域，零浮点零墙钟，建模环境确定性派生）；
//!   · 分位数：P50/P90/P99 三分位（整数秩），排序副本独立于采样序；
//!   · 冷热分层（对齐 F1773 体例）：前 COLD_N 样本入冷层余入热层，冷层
//!     均值必 ≥ 热层均值（冷惩罚单调语义），反序即 ColdHotMiss；
//!   · 格式冻结：账行字段序逐字节冻结，10 路失真注入 10/10 必抓；
//!   · 建模环境：基线(档)+冷惩罚(层)+抖动(确定性)，模型自洽断言全过；
//!   · 真机数字随闸门补测：微秒位仅换数值不换格式（格式冻结语义承诺）。
//! ═══════════════════════════════════════════════════════════════════════
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
/// 诊断段基址（0x727C0，VX3-1103 切换延迟窗口独占）。
pub const SEG_BASE: u32 = 0x727C0;
/// 切换档数（冻结 16，P0..P15）。
pub const GEAR_N: usize = 16;
/// 每档样本数（冻结 64）。
pub const SAMPLE_N: usize = 64;
/// 计时段数（冻结 3：压摆/切频/栅障）。
pub const PHASE_N: usize = 3;
/// 冷层样本数（冻结 8，对齐 F1773 冷热分层体例）。
pub const COLD_N: usize = 8;
/// 分位闭集（冻结 3 分位：50/90/99 百分位）。
pub const PCTL_RANKS: [usize; 3] = [50, 90, 99];
/// 单样本延迟硬上限（冻结 4096 微秒，整数域界，越界即域坏）。
pub const LAT_CAP_US: u64 = 4096;
/// 冷惩罚下限（冻结 128 微秒，建模环境语义）。
pub const COLD_PENALTY_US: u64 = 128;
/// 账面纪元（冻结 9）。
pub const EPOCH: u64 = 9;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrlCode {
    /// 格式冻结被改：账行字段序/字符面/行数与冻结体例对不上
    FmtFrozen,
    /// 分位桶坏：秩越界 / 排序副本与样本集不一致 / 分位值落界外
    BucketBad,
    /// 冷热分层坏：分层切分不齐 / 冷层均值 < 热层均值（冷惩罚反序）
    ColdHotMiss,
    /// 延迟越界：单样本或分段或总分越出整数域界
    LatOver,
    /// 档位空洞：档号缺位/越界/重复入账
    GearGap,
    /// 段和失真：三段和 ≠ 总延迟（分段计时闭合破坏）
    SumMiss,
    /// 建模环境逻辑失败：模型自洽断言（单调/域界/闭合）有破
    ModelEnvBad,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}
impl TrlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TrlCode::FmtFrozen => "格式冻结被改：字段序/字符面冻结体例不得变更，逐字节定位差异",
            TrlCode::BucketBad => "分位桶坏：秩越界或排序副本不一致，查秩公式与排序实现",
            TrlCode::ColdHotMiss => "冷热分层坏：冷层均值必须 ≥ 热层均值，反序查冷惩罚语义",
            TrlCode::LatOver => "延迟越界：单样本超 4096 微秒域界，先查建模抖动幅度",
            TrlCode::GearGap => "档位空洞：档号 0..16 连续闭合，缺位/重复一律拒",
            TrlCode::SumMiss => "段和失真：三段和必须等于总延迟，查分段计时闭合",
            TrlCode::ModelEnvBad => "建模环境逻辑失败：模型自洽断言有破，查基线单调与域界",
            TrlCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [TrlCode; 8] = [
        TrlCode::FmtFrozen,
        TrlCode::BucketBad,
        TrlCode::ColdHotMiss,
        TrlCode::LatOver,
        TrlCode::GearGap,
        TrlCode::SumMiss,
        TrlCode::ModelEnvBad,
        TrlCode::NotReproducible,
    ];
}
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TrlCode::ALL.len() {
        if TrlCode::ALL[i].val() == code { return TrlCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x727C0 段闭集内"
}
// ── 测量状态机（Idle→Measuring→Aggregated→Sealed，单调） ────────
/// 空闲 / 测量中（≥1 样本入账）/ 已聚合（分位分层成型）/ 已封账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrlState {
    Idle,
    Measuring,
    Aggregated,
    Sealed,
}
impl TrlState {
    pub fn rank(self) -> u8 {
        match self {
            TrlState::Idle => 0,
            TrlState::Measuring => 1,
            TrlState::Aggregated => 2,
            TrlState::Sealed => 3,
        }
    }
    pub fn can_advance(self, to: TrlState) -> bool {
        to.rank() > self.rank()
    }
}
// ── 延迟样本（分段计时） ────────────────────────────────────────
/// 单样本：from→to 档切换，三段计时（ph_v=压摆 / ph_c=切频 / ph_f=栅障），
/// total=三段和（闭合校验域），cold=true 表冷层样本（idx < COLD_N）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrSample {
    pub gear: usize,
    pub idx: usize,
    pub ph_v: u64,
    pub ph_c: u64,
    pub ph_f: u64,
    pub total: u64,
    pub cold: bool,
}
impl TrSample {
    pub fn bounds_ok(&self) -> bool {
        let s = self.ph_v.saturating_add(self.ph_c).saturating_add(self.ph_f);
        s == self.total
            && self.total <= LAT_CAP_US
            && self.ph_v <= LAT_CAP_US
            && self.ph_c <= LAT_CAP_US
            && self.ph_f <= LAT_CAP_US
    }
}
// ── 建模环境（确定性延迟派生，零浮点零墙钟） ────────────────────
pub fn base_us(gear: usize) -> u64 {
    if gear >= GEAR_N { return 0; }
    512 - (gear as u64) * 16
}
pub fn jitter_us(gear: usize, idx: usize) -> u64 {
    ((gear * 13 + idx * 7) % 24) as u64
}
pub fn split_phases(gear: usize, idx: usize, total: u64) -> (u64, u64, u64) {
    let v = total / 2;
    let f = jitter_us(gear, idx).min(v);
    let c = total - v - f;
    (v, c, f)
}
pub fn model_sample(gear: usize, idx: usize) -> Result<TrSample, u32> {
    if gear >= GEAR_N || idx >= SAMPLE_N { return Err(TrlCode::GearGap.val()); }
    let j = jitter_us(gear, idx);
    let cold = idx < COLD_N;
    let total = base_us(gear).saturating_add(if cold { COLD_PENALTY_US } else { 0 }).saturating_add(j);
    if total > LAT_CAP_US { return Err(TrlCode::LatOver.val()); }
    let (v, c, f) = split_phases(gear, idx, total);
    let s = TrSample { gear, idx, ph_v: v, ph_c: c, ph_f: f, total, cold };
    if !s.bounds_ok() { return Err(TrlCode::SumMiss.val()); }
    Ok(s)
}
// ── 排序与分位（整数秩） ────────────────────────────────────────
/// 插入排序副本（升序；b 在 1..len 内界内索引，无越界面）。
pub fn sort_copy(vals: &[u64]) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < vals.len() {
        v.push(vals[i]);
        i += 1;
    }
    let mut a = 1usize;
    while a < v.len() {
        let mut b = a;
        while b > 0 && v[b - 1] > v[b] {
            let t = v[b - 1];
            v[b - 1] = v[b];
            v[b] = t;
            b -= 1;
        }
        a += 1;
    }
    v
}
pub fn percentile(sorted: &[u64], p: usize) -> Result<u64, u32> {
    let n = sorted.len();
    if n == 0 { return Err(TrlCode::BucketBad.val()); }
    let rank = ((p.saturating_mul(n)).saturating_add(99)) / 100;
    if rank == 0 { return Err(TrlCode::BucketBad.val()); }
    let idx = rank.min(n) - 1;
    match sorted.get(idx) {
        Some(v) => Ok(*v),
        None => Err(TrlCode::BucketBad.val()),
    }
}
// ── 逐档账行 ────────────────────────────────────────────────────
/// 逐档聚合账：冷热分层（cold/hot_n、cm/hm 整数均值）+ 三分位（p50/p90/
/// p99，全档排序副本口径）+ min/max + 段和守恒残差（必须 0）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GearLat {
    pub gear: usize,
    pub n: usize,
    pub cold_n: usize,
    pub hot_n: usize,
    pub cold_mean: u64,
    pub hot_mean: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
    pub sum_resid: u64,
}
impl GearLat {
    pub fn render(&self, sum: u32) -> String {
        format!(
            "@TRL1|gear={:02}|n={}|cold={}|hot={}|cm={}|hm={}|p50={}|p90={}|p99={}|min={}|max={}|resid={}|sum={:08X};\n",
            self.gear, self.n, self.cold_n, self.hot_n, self.cold_mean, self.hot_mean,
            self.p50, self.p90, self.p99, self.min, self.max, self.sum_resid, sum
        )
    }
}
pub fn aggregate_gear(gear: usize, samples: &[TrSample]) -> Result<GearLat, u32> {
    if samples.is_empty() { return Err(TrlCode::GearGap.val()); }
    if gear >= GEAR_N { return Err(TrlCode::GearGap.val()); }
    let mut totals: Vec<u64> = Vec::new();
    let mut cold_v: Vec<u64> = Vec::new();
    let mut hot_v: Vec<u64> = Vec::new();
    let mut mn = u64::MAX;
    let mut mx = 0u64;
    let mut resid = 0u64;
    let mut i = 0usize;
    while i < samples.len() {
        let s = match samples.get(i) {
            Some(s) => s,
            None => return Err(TrlCode::GearGap.val()),
        };
        if s.gear != gear { return Err(TrlCode::GearGap.val()); }
        if !s.bounds_ok() { return Err(TrlCode::SumMiss.val()); }
        // 冷热分层切分：以样本自带 cold 位 + idx 双核（切分不齐即拒）。
        if s.cold != (s.idx < COLD_N) { return Err(TrlCode::ColdHotMiss.val()); }
        if s.cold {
            cold_v.push(s.total);
        } else {
            hot_v.push(s.total);
        }
        totals.push(s.total);
        if s.total < mn {
            mn = s.total;
        }
        if s.total > mx {
            mx = s.total;
        }
        // 段和残差累计。
        resid = resid.saturating_add(
            s.ph_v.saturating_add(s.ph_c).saturating_add(s.ph_f).saturating_sub(s.total),
        );
        i += 1;
    }
    if cold_v.is_empty() || hot_v.is_empty() { return Err(TrlCode::ColdHotMiss.val()); }
    let all_sorted = sort_copy(&totals);
    // 冷热层均值（整数向下取整；排序副本同层同多重集）。
    let mut means: [u64; 2] = [0, 0];
    let mut li = 0usize;
    while li < 2 {
        let lay = if li == 0 { &cold_v } else { &hot_v };
        let lay_sorted = sort_copy(lay);
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < lay_sorted.len() {
            sum = sum.saturating_add(match lay_sorted.get(i) {
                Some(v) => *v,
                None => return Err(TrlCode::BucketBad.val()),
            });
            i += 1;
        }
        means[li] = sum / lay_sorted.len() as u64;
        li += 1;
    }
    // 冷惩罚单调语义：冷层均值必 ≥ 热层均值。
    if means[0] < means[1] { return Err(TrlCode::ColdHotMiss.val()); }
    let p50 = percentile(&all_sorted, PCTL_RANKS[0])?;
    let p90 = percentile(&all_sorted, PCTL_RANKS[1])?;
    let p99 = percentile(&all_sorted, PCTL_RANKS[2])?;
    // 分位单调：p50 ≤ p90 ≤ p99 ≤ max。
    if !(p50 <= p90 && p90 <= p99 && p99 <= mx) { return Err(TrlCode::BucketBad.val()); }
    Ok(GearLat {
        gear, n: samples.len(), cold_n: cold_v.len(), hot_n: hot_v.len(),
        cold_mean: means[0], hot_mean: means[1], p50, p90, p99,
        min: mn, max: mx, sum_resid: resid,
    })
}
// ── 测量账（状态机 + 逐档样本表 + 逐档账行） ────────────────────
/// 测量账：逐档样本表（gear → 样本列）+ 逐档账行（聚合产物）+
/// 全域 min/max 汇总 + 拒绝计数显性记账。
#[derive(Clone, Debug)]
pub struct TrlLedger {
    pub state: TrlState,
    pub epoch: u64,
    pub per_gear: Vec<Vec<TrSample>>,
    pub rows: Vec<GearLat>,
    pub sum_min: u64,
    pub sum_max: u64,
    pub rejects: u32,
}
impl TrlLedger {
    pub fn new(epoch: u64) -> TrlLedger {
        let mut per_gear = Vec::new();
        let mut g = 0usize;
        while g < GEAR_N {
            per_gear.push(Vec::new());
            g += 1;
        }
        TrlLedger {
            state: TrlState::Idle,
            epoch,
            per_gear,
            rows: Vec::new(),
            sum_min: 0,
            sum_max: 0,
            rejects: 0,
        }
    }
    pub fn advance_state(&mut self, to: TrlState) -> Result<(), u32> {
        if !self.state.can_advance(to) { self.rejects += 1; return Err(TrlCode::GearGap.val()); }
        self.state = to;
        Ok(())
    }
    pub fn ingest(&mut self, gear: usize, idx: usize) -> Result<TrSample, u32> {
        if self.state == TrlState::Sealed { self.rejects += 1; return Err(TrlCode::GearGap.val()); }
        if gear >= GEAR_N || idx >= SAMPLE_N { self.rejects += 1; return Err(TrlCode::GearGap.val()); }
        let s = model_sample(gear, idx)?;
        match self.per_gear.get_mut(gear) {
            Some(v) => v.push(s),
            None => return Err(TrlCode::GearGap.val()),
        }
        if self.state == TrlState::Idle { self.state = TrlState::Measuring; }
        Ok(s)
    }
    pub fn aggregate(&mut self) -> Result<(usize, u64, u64), u32> {
        if self.state == TrlState::Sealed || self.state == TrlState::Aggregated { self.rejects += 1; return Err(TrlCode::GearGap.val()); }
        self.rows.clear();
        let mut g = 0usize;
        let mut smn = u64::MAX;
        let mut smx = 0u64;
        while g < GEAR_N {
            let sv = match self.per_gear.get(g) {
                Some(v) => v,
                None => return Err(TrlCode::GearGap.val()),
            };
            if sv.len() != SAMPLE_N {
                // 档位空洞：每档必须满样。
                return Err(TrlCode::GearGap.val());
            }
            let row = aggregate_gear(g, sv)?;
            if row.min < smn {
                smn = row.min;
            }
            if row.max > smx {
                smx = row.max;
            }
            self.rows.push(row);
            g += 1;
        }
        if self.rows.len() != GEAR_N { return Err(TrlCode::GearGap.val()); }
        self.sum_min = smn;
        self.sum_max = smx;
        self.state = TrlState::Aggregated;
        Ok((self.rows.len(), smn, smx))
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(TrlState::Sealed)
    }
    pub fn render_all(&self) -> Result<String, u32> {
        if self.rows.len() != GEAR_N { return Err(TrlCode::GearGap.val()); }
        let mut s = String::new();
        let mut g = 0usize;
        while g < GEAR_N {
            let r = match self.rows.get(g) {
                Some(r) => r,
                None => return Err(TrlCode::GearGap.val()),
            };
            let sum = fnv1a(r.render(0).as_bytes());
            s.push_str(&r.render(sum));
            g += 1;
        }
        Ok(s)
    }
}
// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}
// ── 字节查找与解析（格式冻结核） ────────────────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle { return Some(i); }
        i += 1;
    }
    None
}
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() { return None; }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' { return None; }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
        i += 1;
    }
    Some(v)
}
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 { return None; }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}
/// 账行解析逐域对账：行文本（去 ';' 终结符）逐 token 提取 12 个数值域
/// + sum，域序错/闭集外字段/坏数字/任一域不等于源行一律 false。
pub fn parse_row_eq(line: &str, r: &GearLat, sum: u32) -> bool {
    if !line.starts_with("@TRL1|") { return false; }
    // 行尾 ';' 终结符先行剥离（终结符不属于任何域）。
    let line = match line.strip_suffix(';') {
        Some(l) => l,
        None => line,
    };
    let bts = line.as_bytes();
    let keys: [&str; 12] = ["gear", "n", "cold", "hot", "cm", "hm", "p50", "p90", "p99", "min", "max", "resid"];
    let want: [u64; 12] = [
        r.gear as u64, r.n as u64, r.cold_n as u64, r.hot_n as u64, r.cold_mean, r.hot_mean,
        r.p50, r.p90, r.p99, r.min, r.max, r.sum_resid,
    ];
    let mut kidx = 0usize;
    let mut sumv: Option<u32> = None;
    let mut i = 6usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &line[i..end];
        if !tok.is_empty() {
            let eq = match find_byte(tok.as_bytes(), 0, b'=') {
                Some(e) => e,
                None => return false,
            };
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            if k == "sum" {
                sumv = parse_hex8(v);
            } else {
                if kidx >= keys.len() || k != keys[kidx] || parse_dec(v) != Some(want[kidx]) { return false; }
                kidx += 1;
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    kidx == keys.len() && sumv == Some(sum)
}
pub fn parse_roundtrip(r: &GearLat, sum: u32) -> bool {
    let line = r.render(sum);
    parse_row_eq(&line[..line.len().saturating_sub(1)], r, sum)
}
// ── 失真注入（10 路逐字失真） ────────────────────────────────────
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}
fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() { return None; }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() { return Some(i); }
        i += 1;
    }
    None
}
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    let mut i = 0usize;
    while i < b.len() {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
        i += 1;
    }
}
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let le = match find_byte(b, 0, b'\n') {
        Some(p) => p,
        None => b.len(),
    };
    match kind % 10 {
        // 头域两路：gear / n（格式冻结+档位闭合必抓）。
        0 => patch_after(b, b"gear=", b'0', b'9', &mut v),
        1 => patch_after(b, b"n=", b'6', b'3', &mut v),
        // 冷热两路：cold / hm（分层断言必抓）。
        2 => patch_after(b, b"cold=", b'8', b'7', &mut v),
        3 => patch_after(b, b"hm=", b'2', b'9', &mut v),
        // 分位三路：p50 / p90 / p99（分位单调断言必抓）。
        4 => patch_after(b, b"p50=", b'0', b'9', &mut v),
        5 => patch_after(b, b"p90=", b'0', b'9', &mut v),
        6 => patch_after(b, b"p99=", b'0', b'9', &mut v),
        // sum 域改写（校验和必抓）。
        7 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        // 截尾：去末 8 字节。
        8 => push_range(b, 0, b.len().saturating_sub(9), &mut v),
        // 注控制符：册中插裸 0x01。
        _ => {
            push_range(b, 0, le / 2, &mut v);
            v.push(0x01);
            push_range(b, le / 2 + 1, b.len(), &mut v);
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}
// ── 全册核验器 ──────────────────────────────────────────────────
pub fn verify_ledger_text(led: &TrlLedger, out: &str) -> Result<(), u32> {
    if !out.starts_with("@TRL1|") { return Err(TrlCode::FmtFrozen.val()); }
    let b = out.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(TrlCode::FmtFrozen.val()); }
        i += 1;
    }
    let mut lines = 0u32;
    let mut p = 0usize;
    while p < b.len() {
        if b[p] == b'\n' {
            lines += 1;
        }
        p += 1;
    }
    if lines != led.rows.len() as u32 { return Err(TrlCode::FmtFrozen.val()); }
    let mut li = 0usize;
    let mut ri = 0usize;
    while li < b.len() {
        let le = match find_byte(b, li, b'\n') {
            Some(p) => p,
            None => b.len(),
        };
        let line = &out[li..le];
        let r = match led.rows.get(ri) {
            Some(r) => r,
            None => return Err(TrlCode::GearGap.val()),
        };
        let sum = fnv1a(r.render(0).as_bytes());
        let gold = r.render(sum);
        let gold_body = &gold[..gold.len().saturating_sub(1)];
        if line != gold_body { return Err(TrlCode::FmtFrozen.val()); }
        if !parse_roundtrip(r, sum) { return Err(TrlCode::FmtFrozen.val()); }
        // 分层断言复核：cold+hot == n 且冷均值 ≥ 热均值。
        if r.cold_n + r.hot_n != r.n || r.cold_mean < r.hot_mean { return Err(TrlCode::ColdHotMiss.val()); }
        if le >= b.len() {
            break;
        }
        li = le + 1;
        ri += 1;
    }
    if ri != led.rows.len() { return Err(TrlCode::GearGap.val()); }
    Ok(())
}
// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let ledg = build_ledger(EPOCH + r as u64)?;
        let gold = ledg.render_all()?;
        verify_ledger_text(&ledg, &gold)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_ledger_text(&ledg, &distort(&gold, kind)).is_err());
        r += 1;
    }
    Ok(led)
}
// ── 全册建账（建模环境全量派生） ────────────────────────────────
pub fn build_ledger(epoch: u64) -> Result<TrlLedger, u32> {
    let mut led = TrlLedger::new(epoch);
    let mut g = 0usize;
    while g < GEAR_N {
        let mut i = 0usize;
        while i < SAMPLE_N {
            led.ingest(g, i)?;
            i += 1;
        }
        g += 1;
    }
    led.aggregate()?;
    Ok(led)
}
// ── 建模环境逻辑核（判据②：建模环境逻辑全过） ──────────────────
pub fn model_env_checks(led: &TrlLedger) -> Result<u32, u32> {
    if led.rows.len() != GEAR_N { return Err(TrlCode::ModelEnvBad.val()); }
    // M1：档基线单调非增。
    let mut g = 0usize;
    while g + 1 < GEAR_N {
        if base_us(g) < base_us(g + 1) { return Err(TrlCode::ModelEnvBad.val()); }
        g += 1;
    }
    // M2/M3/M4/M5：逐档账行复核 + 全样本域界复抽。
    let mut k = 0u32;
    let mut g = 0usize;
    while g < GEAR_N {
        let row = match led.rows.get(g) {
            Some(r) => r,
            None => return Err(TrlCode::ModelEnvBad.val()),
        };
        if row.n != SAMPLE_N || row.cold_n != COLD_N || row.hot_n != SAMPLE_N - COLD_N { return Err(TrlCode::ColdHotMiss.val()); }
        if row.sum_resid != 0 { return Err(TrlCode::SumMiss.val()); }
        if row.max > LAT_CAP_US { return Err(TrlCode::LatOver.val()); }
        if !(row.p50 <= row.p90 && row.p90 <= row.p99) { return Err(TrlCode::BucketBad.val()); }
        if row.cold_mean < row.hot_mean { return Err(TrlCode::ColdHotMiss.val()); }
        // 复抽 4 样本（每 16 取 1）逐样本核域界与段和。
        let sv = match led.per_gear.get(g) {
            Some(v) => v,
            None => return Err(TrlCode::ModelEnvBad.val()),
        };
        let mut i = 0usize;
        while i < sv.len() {
            if i % 16 == 0 {
                let s = match sv.get(i) {
                    Some(s) => s,
                    None => return Err(TrlCode::ModelEnvBad.val()),
                };
                if !s.bounds_ok() { return Err(TrlCode::SumMiss.val()); }
                k += 1;
            }
            i += 1;
        }
        g += 1;
    }
    Ok(k)
}
// ── 黄金全链 ────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let led = build_ledger(EPOCH)?;
    let gold = led.render_all()?;
    verify_ledger_text(&led, &gold)?;
    let spot = model_env_checks(&led)?;
    // 四拒探：档号越界 / 状态平移倒退闭集 / 封账后写 / 档位空洞 / 越界样本。
    let mut probe = TrlLedger::new(EPOCH);
    let range_rej = probe.ingest(GEAR_N, 0).is_err() && probe.ingest(0, SAMPLE_N).is_err();
    let mono = probe.advance_state(TrlState::Measuring).is_ok()
        && probe.advance_state(TrlState::Idle).is_err()
        && probe.advance_state(TrlState::Measuring).is_err()
        && probe.advance_state(TrlState::Aggregated).is_ok()
        && probe.advance_state(TrlState::Measuring).is_err()
        && probe.advance_state(TrlState::Sealed).is_ok()
        && probe.ingest(0, 0).is_err();
    // 档位空洞拒：缺档聚合必拒；建模环境样本越界拒。
    let mut hole = TrlLedger::new(EPOCH);
    let _ = hole.ingest(0, 0);
    let hole_rej = hole.aggregate().is_err();
    let over_rej = model_sample(GEAR_N, 0).is_err() && model_sample(0, SAMPLE_N).is_err();
    let flags = (spot > 0) as u32 + verify_ledger_text(&led, &led.render_all()?).is_ok() as u32
        + inject_rounds(1).is_ok() as u32 + (led.sum_max <= LAT_CAP_US && led.sum_min > 0) as u32;
    let rejects = range_rej as u32 + mono as u32 + hole_rej as u32 + over_rej as u32;
    Ok((flags, rejects, spot, led.sum_max))
}
// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(TrlCode::NotReproducible.val()),
    }
}
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(TrlCode::FmtFrozen.val()),
    }
}
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x727C0 && TrlCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = GEAR_N == 16 && SAMPLE_N == 64 && PHASE_N == 3 && COLD_N == 8
        && PCTL_RANKS[0] == 50 && PCTL_RANKS[1] == 90 && PCTL_RANKS[2] == 99
        && LAT_CAP_US == 4096 && COLD_PENALTY_US == 128 && EPOCH == 9 && JUDGE_ROUNDS == 10 && in_window;
    if ok { Ok(()) } else { Err(TrlCode::NotReproducible.val()) }
}
// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(TrlCode::NotReproducible.val()); }
    Ok(())
}
// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TrlCode::ALL.len() {
        let v = TrlCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}
// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut led = build_ledger(EPOCH)?;
    let spot = model_env_checks(&led)?;
    let _ = led.seal();
    Ok(format!("状态=已完成 gears={} samples={} spot={} min={} max={}（微秒位为建模环境值，真机数字随闸门补测）",
        led.rows.len(), SAMPLE_N, spot, led.sum_min, led.sum_max))
}
// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, spot, mx)) => (*f == 4 && *r == 4, format!("flags={} rejects={} spot={} max={}us", f, r, spot, mx)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 格式冻结：建行→解析→逐域对账（含负探三路）。
    let ledg = build_ledger(EPOCH);
    let mut fmt_ok = false;
    let mut neg_ok = false;
    if let Ok(l) = &ledg {
        if let Some(r0) = l.rows.get(0) {
            let sum = fnv1a(r0.render(0).as_bytes());
            fmt_ok = parse_roundtrip(r0, sum);
            // 负探：坏前缀 / 未知字段 / 坏数字解析必败。
            let gold = r0.render(sum);
            let body = &gold[..gold.len().saturating_sub(1)];
            neg_ok = !parse_row_eq(&format!("@XXX1|{}", &body[6..]), r0, sum)
                && !parse_row_eq(&format!("{}|ghost=1", body), r0, sum)
                && !parse_row_eq(&format!("{}|n=x", body), r0, sum);
        }
    }
    v.push(("J1-账格式冻结", fmt_ok && neg_ok, String::from("字段序冻结逐域对账；坏前缀/未知字段/坏数字必败")));
    // J1 分位数闭集：秩公式 + 单调 + 坏桶拒 + 排序副本独立复核。
    let p_ok = {
        let mut vals: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < 64 {
            vals.push(((i * 31) % 97) as u64 + 100);
            i += 1;
        }
        let sorted = sort_copy(&vals);
        // 全档三分位可取且单调（p50 ≤ p90 ≤ p99），空集拒。
        let mono = match (percentile(&sorted, 50), percentile(&sorted, 90), percentile(&sorted, 99)) {
            (Ok(x), Ok(y), Ok(z)) => x <= y && y <= z,
            _ => false,
        };
        let empty_rej = percentile(&[], 50).is_err();
        // 排序副本与样本集同长，且排序后首位必为全局最小（独立复算）。
        let mut mn = u64::MAX;
        let mut k = 0usize;
        while k < vals.len() {
            if let Some(v) = vals.get(k) {
                if *v < mn { mn = *v; }
            }
            k += 1;
        }
        let head_min = match sorted.get(0) { Some(s) => *s == mn, None => false };
        sorted.len() == vals.len() && mono && empty_rej && head_min
    };
    v.push(("J1-分位数闭集", p_ok, String::from("千分位秩+单调+空集拒+排序副本复核")));
    // J1 冷热分层：逐档冷均值 ≥ 热均值 + 分层数冻结。
    let tier_ok = match &ledg {
        Ok(l) => {
            let mut ok = l.rows.len() == GEAR_N;
            let mut g = 0usize;
            while g < l.rows.len() {
                if let Some(r) = l.rows.get(g) {
                    if !(r.cold_n == COLD_N && r.hot_n == SAMPLE_N - COLD_N && r.cold_mean >= r.hot_mean) {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok
        }
        Err(_) => false,
    };
    v.push(("J1-冷热分层", tier_ok, format!("逐档 cold={} hot={} 冷均值≥热均值", COLD_N, SAMPLE_N - COLD_N)));
    // J1 建模环境逻辑全过。
    let env = match &ledg {
        Ok(l) => model_env_checks(l),
        Err(e) => Err(*e),
    };
    let (ep, ed) = match &env {
        Ok(k) => (*k > 0, format!("模型断言全过，复抽 {} 样本", k)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-建模环境逻辑", ep, ed));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 单调守卫：状态机进拒平封 + 档位空洞 + 越界样本。
    let mut probe = TrlLedger::new(EPOCH);
    let s1 = probe.advance_state(TrlState::Measuring).is_ok();
    let s2 = probe.advance_state(TrlState::Idle).is_err();
    let s3 = probe.advance_state(TrlState::Measuring).is_err();
    let s4 = probe.advance_state(TrlState::Aggregated).is_ok();
    let s5 = probe.advance_state(TrlState::Aggregated).is_err();
    let s6 = probe.advance_state(TrlState::Sealed).is_ok();
    let s7 = probe.ingest(0, 0).is_err();
    let s8 = probe.ingest(GEAR_N, 0).is_err();
    let s9 = model_sample(GEAR_N + 1, 0).is_err();
    v.push((
        "J2-单调守卫",
        s1 && s2 && s3 && s4 && s5 && s6 && s7 && s8 && s9,
        format!("状态进拒平封={}/{}/{}/{}/{}/{}/{} 封账写={} 越界档={}/{}",
            s1, s2, s3, s4, s5, s6, s7, s8, s8, s9),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x727C0 窗口")));
    v
}
pub fn run_unx1965_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1965 transition latency 账（逐档切换延迟测量）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}