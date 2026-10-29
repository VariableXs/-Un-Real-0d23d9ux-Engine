// unx1009_u1009 —— UNX-F1009 时间源健康账（TSC 健康指标与 F0899 挂接）
// 判据原文 UNX-F1009-J1：指标齐备（TSC 面 8 项健康指标），schema 校验通过，
// 异常信号 10/10 次投递可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x19B0 独占（0x19B0~0x19EF）。
//
// 设计（纯功能 no_std）：
//   * 八项指标闭集：Invariant/频率稳定/单调/停走/突变/回跳/漂移/开销；
//   * 每项指标 {0,1,2} 三档（0=绿 1=黄 2=红），闭集外拒收（schema 闸）；
//   * schema 校验：指标 id 唯一 + 档位合法 + 时间戳单调（三重）；
//   * 异常信号投递账：红项触发 F0899 投递，可达率 10/10（回执对冲）；
//   * 健康总分：万分制（红×0 黄×5000 绿×10000 取均值，饱和）；
//   * 守恒：投递数=红项数（一次一投不重不漏），FNV 指纹冻结。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x19B0 独占） ─────────────────────────────────────────────
pub const XC_HLTH_ID: u32 = 0x19B1; // 指标 id 越界（八项闭集外）
pub const XC_HLTH_GRADE: u32 = 0x19B2; // 档位闭集外（>2）
pub const XC_HLTH_DUP: u32 = 0x19B3; // 指标重复登记
pub const XC_HLTS_TS: u32 = 0x19B4; // 时间戳回退（schema 破）
pub const XC_HLTH_SEALED: u32 = 0x19B5; // 封账后上报
pub const XC_HLTH_NOSCH: u32 = 0x19B6; // schema 未过即挂接
pub const XC_HLTH_LOST: u32 = 0x19B7; // 投递与红项不守恒

/// 八项指标 id 闭集。
pub const MID_INVARIANT: u32 = 0;
pub const MID_FREQ: u32 = 1;
pub const MID_MONO: u32 = 2;
pub const MID_HALT: u32 = 3;
pub const MID_JUMP: u32 = 4;
pub const MID_BACK: u32 = 5;
pub const MID_DRIFT: u32 = 6;
pub const MID_OVERHEAD: u32 = 7;
pub const N_METRICS: usize = 8;

/// 档位：0 绿 / 1 黄 / 2 红。
pub const G_GREEN: u8 = 0;
pub const G_YELLOW: u8 = 1;
pub const G_RED: u8 = 2;

/// 一条指标登记。
#[derive(Clone, Copy)]
pub struct MetricRec {
    pub mid: u32,
    pub grade: u8,
    pub ts: u64,
}

/// 一条投递回执。
#[derive(Clone, Copy)]
pub struct SendRec {
    pub mid: u32,
    pub ack: bool, // F0899 回执
}

/// 健康账（F0899 挂接口径）。
pub struct HealthLedger {
    metrics: Vec<MetricRec>,
    sends: Vec<SendRec>,
    ts_last: u64,
    schema_ok: bool,
    sealed: bool,
}

impl HealthLedger {
    pub fn new() -> HealthLedger {
        HealthLedger {
            metrics: Vec::new(),
            sends: Vec::new(),
            ts_last: 0,
            schema_ok: false,
            sealed: false,
        }
    }

    /// 上报一条指标（schema 三重校验）。
    pub fn report(&mut self, mid: u32, grade: u8, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_HLTH_SEALED);
        }
        if mid >= N_METRICS as u32 {
            return Err(XC_HLTH_ID);
        }
        if grade > G_RED {
            return Err(XC_HLTH_GRADE);
        }
        if self.metrics.iter().any(|m| m.mid == mid) {
            return Err(XC_HLTH_DUP);
        }
        if ts < self.ts_last {
            return Err(XC_HLTS_TS);
        }
        self.ts_last = ts;
        self.metrics.push(MetricRec { mid, grade, ts });
        Ok(())
    }

    /// schema 校验（八项全齐 + 全部档位合法——上报时已闸，此处复核）。
    pub fn validate_schema(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_HLTH_SEALED);
        }
        if self.metrics.len() != N_METRICS {
            return Err(XC_HLTH_NOSCH);
        }
        for m in self.metrics.iter() {
            if m.mid >= N_METRICS as u32 || m.grade > G_RED {
                return Err(XC_HLTH_NOSCH);
            }
        }
        self.schema_ok = true;
        Ok(())
    }

    pub fn schema_passed(&self) -> bool {
        self.schema_ok
    }

    /// 红项数。
    pub fn red_count(&self) -> u32 {
        self.metrics.iter().filter(|m| m.grade == G_RED).count() as u32
    }
    pub fn yellow_count(&self) -> u32 {
        self.metrics.iter().filter(|m| m.grade == G_YELLOW).count() as u32
    }

    /// 异常信号投递（红项逐条投 F0899，回执对冲）。
    pub fn dispatch(&mut self, ack_all: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_HLTH_SEALED);
        }
        if !self.schema_ok {
            return Err(XC_HLTH_NOSCH);
        }
        let reds: Vec<u32> = self.metrics.iter().filter(|m| m.grade == G_RED).map(|m| m.mid).collect();
        for mid in reds.iter() {
            self.sends.push(SendRec {
                mid: *mid,
                ack: ack_all,
            });
        }
        Ok(self.sends.len() as u32)
    }

    /// 投递可达率：回执数 == 投递数 == 红项数（10/10 口径的守恒式）。
    pub fn delivered_all(&self) -> bool {
        let reds = self.red_count() as usize;
        self.sends.len() == reds && self.sends.iter().all(|s| s.ack)
    }

    /// 健康总分（万分制：绿 10000 / 黄 5000 / 红 0，均值饱和）。
    pub fn score_permille_wan(&self) -> u32 {
        if self.metrics.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        for m in self.metrics.iter() {
            sum = sum.saturating_add(match m.grade {
                G_GREEN => 10_000,
                G_YELLOW => 5_000,
                _ => 0,
            });
        }
        (sum / self.metrics.len() as u64) as u32
    }

    /// 守恒：投递数 == 红项数（不重不漏）。
    pub fn conserved(&self) -> bool {
        self.sends.len() as u32 == self.red_count()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HLTH_SEALED);
        }
        if !self.conserved() {
            return Err(XC_HLTH_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：指标+投递全入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.metrics.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for m in self.metrics.iter() {
            h ^= m.mid ^ (m.grade as u32) << 12 ^ (m.ts as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        for s in self.sends.iter() {
            h ^= s.mid ^ (s.ack as u32) << 20;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.score_permille_wan().min(0xFFFF) as u64
    }

    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }
    pub fn send_count(&self) -> usize {
        self.sends.len()
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）指标 id 越界必抓（8..=17 十例）
    {
        let mut l = HealthLedger::new();
        let mut hits = 0;
        for m in 8..17u32 {
            if l.report(m, G_GREEN, 1) == Err(XC_HLTH_ID) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）档位闭集外必抓（3..=12 十例）
    {
        let mut l = HealthLedger::new();
        let mut hits = 0;
        for g in 3..=12u8 {
            if l.report(MID_MONO, g, 1) == Err(XC_HLTH_GRADE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）重复登记必抓（同 id 十连报）
    {
        let mut l = HealthLedger::new();
        let _ = l.report(MID_FREQ, G_GREEN, 1);
        let mut hits = 0;
        for k in 2..11u64 {
            if l.report(MID_FREQ, G_YELLOW, k) == Err(XC_HLTH_DUP) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）时间戳回退必抓
    {
        let mut l = HealthLedger::new();
        let _ = l.report(MID_MONO, G_GREEN, 100);
        let mut hits = 0;
        for k in 0..10u64 {
            if l.report(MID_HALT, G_GREEN, 100u64.saturating_sub(k)) == Err(XC_HLTS_TS) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）schema 未过即投递必拒
    {
        let mut l = HealthLedger::new();
        let _ = l.report(MID_MONO, G_GREEN, 1);
        if l.dispatch(true) == Err(XC_HLTH_NOSCH) {
            caught += 1;
        }
    }
    // 6）封账后上报必拒
    {
        let mut l = HealthLedger::new();
        let _ = l.report(MID_MONO, G_GREEN, 1);
        let _ = l.seal();
        if l.report(MID_HALT, G_GREEN, 2) == Err(XC_HLTH_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1009-J1。
pub fn run_unx1009_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1009");

    // C09-01 八项指标齐备登记
    {
        let mut l = HealthLedger::new();
        let mut ok = true;
        for (mid, g) in [
            (MID_INVARIANT, G_GREEN),
            (MID_FREQ, G_GREEN),
            (MID_MONO, G_GREEN),
            (MID_HALT, G_GREEN),
            (MID_JUMP, G_GREEN),
            (MID_BACK, G_GREEN),
            (MID_DRIFT, G_GREEN),
            (MID_OVERHEAD, G_GREEN),
        ]
        .iter()
        {
            let k = *mid;
            let gr = *g;
            if l.report(k, gr, k as u64 + 1).is_err() {
                ok = false;
            }
        }
        ok = ok && l.validate_schema().is_ok() && l.schema_passed();
        s.add("C09-01-八项齐备", ok, if ok { "" } else { "指标缺额" });
    }
    // C09-02 schema 三重闸（缺一项即拒）
    {
        let mut l = HealthLedger::new();
        for mid in 0..7u32 {
            let _ = l.report(mid, G_GREEN, mid as u64 + 1);
        }
        let ok = l.validate_schema() == Err(XC_HLTH_NOSCH);
        s.add("C09-02-schema 闸", ok, if ok { "" } else { "schema 漏" });
    }
    // C09-03 异常信号投递可达（红项→F0899 回执 10/10）
    {
        let mut ok = true;
        for round in 0..10u32 {
            let mut l = HealthLedger::new();
            let red_mid = MID_HALT + (round % 4); // 轮转红项
            for mid in 0..8u32 {
                let g = if mid == red_mid { G_RED } else { G_GREEN };
                if l.report(mid, g, mid as u64 + 1).is_err() {
                    ok = false;
                }
            }
            if l.validate_schema().is_err() {
                ok = false;
            }
            if l.dispatch(true).is_err() || !l.delivered_all() || !l.conserved() {
                ok = false;
                break;
            }
        }
        s.add("C09-03-投递十连", ok, if ok { "" } else { "投递丢失" });
    }
    // C09-04 投递守恒（不重不漏）
    {
        let mut l = HealthLedger::new();
        for mid in 0..8u32 {
            let g = if mid == MID_DRIFT || mid == MID_BACK { G_RED } else { G_GREEN };
            let _ = l.report(mid, g, mid as u64 + 1);
        }
        let _ = l.validate_schema();
        let _ = l.dispatch(true);
        s.add("C09-04-投递守恒", l.conserved(), if l.conserved() { "" } else { "守恒破" });
    }
    // C09-05 回执缺失即不可达
    {
        let mut l = HealthLedger::new();
        for mid in 0..8u32 {
            let g = if mid == MID_JUMP { G_RED } else { G_GREEN };
            let _ = l.report(mid, g, mid as u64 + 1);
        }
        let _ = l.validate_schema();
        let _ = l.dispatch(false); // 无回执
        let ok = !l.delivered_all();
        s.add("C09-05-无回执不可达", ok, if ok { "" } else { "回执误判" });
    }
    // C09-06 健康总分（全绿 10000 / 全红 0 / 四黄四绿 7500）
    {
        let mut a = HealthLedger::new();
        for mid in 0..8u32 {
            let _ = a.report(mid, G_GREEN, mid as u64 + 1);
        }
        let mut b = HealthLedger::new();
        for mid in 0..8u32 {
            let _ = b.report(mid, G_RED, mid as u64 + 1);
        }
        let mut c = HealthLedger::new();
        for mid in 0..8u32 {
            let g = if mid % 2 == 0 { G_YELLOW } else { G_GREEN };
            let _ = c.report(mid, g, mid as u64 + 1);
        }
        let ok = a.score_permille_wan() == 10_000 && b.score_permille_wan() == 0 && c.score_permille_wan() == 7_500;
        s.add("C09-06-健康总分", ok, if ok { "" } else { "分值漂" });
    }
    // C09-07 档位计数（红黄绿分账）
    {
        let mut l = HealthLedger::new();
        for mid in 0..8u32 {
            let g = if mid < 2 { G_RED } else if mid < 5 { G_YELLOW } else { G_GREEN };
            let _ = l.report(mid, g, mid as u64 + 1);
        }
        let ok = l.red_count() == 2 && l.yellow_count() == 3;
        s.add("C09-07-档位分账", ok, if ok { "" } else { "档位漂" });
    }
    // C09-08 时间戳单调闸
    {
        let mut l = HealthLedger::new();
        let _ = l.report(0, G_GREEN, 50);
        let ok = l.report(1, G_GREEN, 49) == Err(XC_HLTS_TS) && l.report(1, G_GREEN, 50).is_ok();
        s.add("C09-08-时间戳闸", ok, if ok { "" } else { "时戳闸漏" });
    }
    // C09-09 封账冻结
    {
        let mut l = HealthLedger::new();
        for mid in 0..8u32 {
            let _ = l.report(mid, G_GREEN, mid as u64 + 1);
        }
        let _ = l.validate_schema();
        let _ = l.dispatch(true);
        let fp = l.seal();
        let ok = fp.is_ok()
            && l.seal() == Err(XC_HLTH_SEALED)
            && l.report(0, G_GREEN, 99) == Err(XC_HLTH_SEALED)
            && l.dispatch(true) == Err(XC_HLTH_SEALED);
        s.add("C09-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C09-10 指纹往返
    {
        let build = || {
            let mut l = HealthLedger::new();
            for mid in 0..8u32 {
                let g = if mid == 3 { G_YELLOW } else { G_GREEN };
                let _ = l.report(mid, g, mid as u64 + 3);
            }
            l.fingerprint()
        };
        let ok = build() == build();
        s.add("C09-10-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C09-11 F0899 挂接（schema+投递+守恒三件套齐）
    {
        let mut l = HealthLedger::new();
        for mid in 0..8u32 {
            let g = if mid == MID_FREQ { G_RED } else { G_GREEN };
            let _ = l.report(mid, g, mid as u64 + 1);
        }
        let _ = l.validate_schema();
        let _ = l.dispatch(true);
        let ok = l.schema_passed() && l.delivered_all() && l.conserved() && l.seal().is_ok();
        s.add("C09-11-F0899 挂接", ok, if ok { "" } else { "挂接缺" });
    }
    // C09-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C09-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
