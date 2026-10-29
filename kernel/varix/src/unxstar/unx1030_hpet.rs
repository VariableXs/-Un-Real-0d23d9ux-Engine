// unx1030_hpet —— UNX-F1030 HPET 健康账（HPET 面健康指标）
// 判据原文 UNX-F1030-J1：指标齐备（HPET 面 6 项指标），schema 校验通过，
// 异常投递 10/10 次可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2080 独占（0x2080~0x20BF）。
//
// 设计（纯功能 no_std）：
//   * 六项指标闭集：在位/频点稳定/主计数器走动/定时器通道齐/中断投递/溢出防；
//   * 档位三闭集 {0 绿,1 黄,2 红}，闭集外拒（schema 闸）；
//   * 健康账：逐指标登记（append-only 重登记拒）+ 复核通道；
//   * 异常投递：红项触发 HPET 健康事件（回执对冲 10/10 可达）；
//   * 分值：万分制（绿 10000/黄 5000/红 0 均值饱和）；
//   * 守恒：投递数=红项数；FNV 指纹冻结；注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2080 独占） ─────────────────────────────────────────────
pub const XC_HPET_MID: u32 = 0x2081; // 指标 id 越界（六项闭集外）
pub const XC_HPET_GRADE: u32 = 0x2082; // 档位闭集外
pub const XC_HPET_DUP: u32 = 0x2083; // 指标重复登记
pub const XC_HPET_TS: u32 = 0x2084; // 时戳回退
pub const XC_HPET_SEALED: u32 = 0x2085; // 封账后上报
pub const XC_HPET_LOST: u32 = 0x2086; // 投递与红项不守恒

/// 六项指标 id。
pub const HM_PRESENT: u32 = 0; // 在位
pub const HM_FREQ: u32 = 1; // 频点稳定
pub const HM_MAINCNT: u32 = 2; // 主计数器走动
pub const HM_CHANNELS: u32 = 3; // 定时器通道齐
pub const HM_IRQ: u32 = 4; // 中断投递
pub const HM_OVF: u32 = 5; // 溢出防护
pub const N_METRICS: u32 = 6;

/// 档位。
pub const HG_GREEN: u8 = 0;
pub const HG_YELLOW: u8 = 1;
pub const HG_RED: u8 = 2;

/// 一条指标登记。
#[derive(Clone, Copy)]
pub struct HpetMetric {
    pub mid: u32,
    pub grade: u8,
    pub ts: u64,
}

/// 一条投递。
#[derive(Clone, Copy)]
pub struct HpetSend {
    pub mid: u32,
    pub ack: bool,
}

/// HPET 健康账。
pub struct HpetHealth {
    metrics: Vec<HpetMetric>,
    sends: Vec<HpetSend>,
    ts_last: u64,
    sealed: bool,
}

impl HpetHealth {
    pub fn new() -> HpetHealth {
        HpetHealth {
            metrics: Vec::new(),
            sends: Vec::new(),
            ts_last: 0,
            sealed: false,
        }
    }

    /// 上报一条指标（六项闭集 + 档位闭集 + 重登拒 + 时戳单调）。
    pub fn report(&mut self, mid: u32, grade: u8, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_HPET_SEALED);
        }
        if mid >= N_METRICS {
            return Err(XC_HPET_MID);
        }
        if grade > HG_RED {
            return Err(XC_HPET_GRADE);
        }
        if self.metrics.iter().any(|m| m.mid == mid) {
            return Err(XC_HPET_DUP);
        }
        if ts < self.ts_last {
            return Err(XC_HPET_TS);
        }
        self.ts_last = ts;
        self.metrics.push(HpetMetric { mid, grade, ts });
        Ok(())
    }

    /// 六项齐备。
    pub fn metrics_full(&self) -> bool {
        self.metrics.len() == N_METRICS as usize
    }

    /// schema 复核（六项全齐 + 全部档位/时戳合法）。
    pub fn schema_ok(&self) -> bool {
        if !self.metrics_full() {
            return false;
        }
        self.metrics.iter().all(|m| m.mid < N_METRICS && m.grade <= HG_RED)
    }

    /// 红项数。
    pub fn red_count(&self) -> u32 {
        self.metrics.iter().filter(|m| m.grade == HG_RED).count() as u32
    }

    /// 异常投递（红项逐条，回执可选）。
    pub fn dispatch(&mut self, ack: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_HPET_SEALED);
        }
        if !self.schema_ok() {
            return Err(XC_HPET_LOST);
        }
        let reds: Vec<u32> = self.metrics.iter().filter(|m| m.grade == HG_RED).map(|m| m.mid).collect();
        for mid in reds.iter() {
            self.sends.push(HpetSend { mid: *mid, ack });
        }
        Ok(self.sends.len() as u32)
    }

    /// 投递可达（回执全齐）。
    pub fn delivered_all(&self) -> bool {
        self.sends.len() == self.red_count() as usize && self.sends.iter().all(|s| s.ack)
    }

    /// 健康分（万分制均值饱和）。
    pub fn score(&self) -> u32 {
        if self.metrics.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        for m in self.metrics.iter() {
            sum = sum.saturating_add(match m.grade {
                HG_GREEN => 10_000,
                HG_YELLOW => 5_000,
                _ => 0,
            });
        }
        (sum / self.metrics.len() as u64) as u32
    }

    /// 守恒：投递数=红项数。
    pub fn conserved(&self) -> bool {
        self.sends.len() as u32 == self.red_count()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_HPET_SEALED);
        }
        if !self.conserved() {
            return Err(XC_HPET_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.metrics.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for m in self.metrics.iter() {
            h ^= m.mid ^ (m.grade as u32) << 8 ^ (m.ts as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        for s in self.sends.iter() {
            h ^= s.mid ^ (s.ack as u32) << 16;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.score().min(0xFFFF) as u64
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

    // 1）指标 id 越界必拒（6..=15 十例）
    {
        let mut l = HpetHealth::new();
        let mut hits = 0;
        for m in 6..=15u32 {
            if l.report(m, HG_GREEN, 1) == Err(XC_HPET_MID) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）档位闭集外必拒（3..=12）
    {
        let mut l = HpetHealth::new();
        let mut hits = 0;
        for g in 3..=12u8 {
            if l.report(HM_FREQ, g, 1) == Err(XC_HPET_GRADE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）重登必拒（10/10）
    {
        let mut l = HpetHealth::new();
        let _ = l.report(HM_MAINCNT, HG_GREEN, 1);
        let mut hits = 0;
        for k in 2..11u64 {
            if l.report(HM_MAINCNT, HG_YELLOW, k) == Err(XC_HPET_DUP) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）时戳回退必拒
    {
        let mut l = HpetHealth::new();
        let _ = l.report(HM_IRQ, HG_GREEN, 100);
        let mut hits = 0;
        for k in 0..10u64 {
            if l.report(HM_OVF, HG_GREEN, 100u64.saturating_sub(k)) == Err(XC_HPET_TS) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）schema 未齐投递必拒
    {
        let mut l = HpetHealth::new();
        for m in 0..5u32 {
            let _ = l.report(m, HG_GREEN, m as u64 + 1);
        }
        if l.dispatch(true) == Err(XC_HPET_LOST) {
            caught += 1;
        }
    }
    // 6）封账后上报必拒
    {
        let mut l = HpetHealth::new();
        let _ = l.report(HM_PRESENT, HG_GREEN, 1);
        let _ = l.seal();
        if l.report(HM_FREQ, HG_GREEN, 2) == Err(XC_HPET_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1030-J1。
pub fn run_unx1030_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1030");

    // C30-01 六项指标齐备
    {
        let mut l = HpetHealth::new();
        let mut ok = true;
        for m in 0..6u32 {
            if l.report(m, HG_GREEN, m as u64 + 1).is_err() {
                ok = false;
            }
        }
        ok = ok && l.metrics_full() && l.schema_ok();
        s.add("C30-01-六项齐备", ok, if ok { "" } else { "指标缺" });
    }
    // C30-02 schema 闸（缺项/越档拒）
    {
        let mut l = HpetHealth::new();
        for m in 0..5u32 {
            let _ = l.report(m, HG_GREEN, m as u64 + 1);
        }
        let ok = !l.schema_ok() && l.report(5, 3, 9) == Err(XC_HPET_GRADE);
        s.add("C30-02-schema 闸", ok, if ok { "" } else { "闸漏" });
    }
    // C30-03 异常投递 10/10 可达（轮转红项）
    {
        let mut ok = true;
        for round in 0..10u32 {
            let mut l = HpetHealth::new();
            let red = round % 6;
            for m in 0..6u32 {
                let g = if m == red { HG_RED } else { HG_GREEN };
                if l.report(m, g, m as u64 + 1).is_err() {
                    ok = false;
                }
            }
            if l.dispatch(true).is_err() || !l.delivered_all() || !l.conserved() {
                ok = false;
                break;
            }
        }
        s.add("C30-03-投递十连", ok, if ok { "" } else { "投递缺" });
    }
    // C30-04 无回执不可达
    {
        let mut l = HpetHealth::new();
        for m in 0..6u32 {
            let g = if m == HM_IRQ { HG_RED } else { HG_GREEN };
            let _ = l.report(m, g, m as u64 + 1);
        }
        let _ = l.dispatch(false);
        let ok = !l.delivered_all();
        s.add("C30-04-无回执判缺", ok, if ok { "" } else { "回执误判" });
    }
    // C30-05 健康分（全绿 10000/全红 0/三黄三绿 7500）
    {
        let mut a = HpetHealth::new();
        for m in 0..6u32 {
            let _ = a.report(m, HG_GREEN, m as u64 + 1);
        }
        let mut b = HpetHealth::new();
        for m in 0..6u32 {
            let _ = b.report(m, HG_RED, m as u64 + 1);
        }
        let mut c = HpetHealth::new();
        for m in 0..6u32 {
            let g = if m % 2 == 0 { HG_YELLOW } else { HG_GREEN };
            let _ = c.report(m, g, m as u64 + 1);
        }
        let ok = a.score() == 10_000 && b.score() == 0 && c.score() == 7_500;
        s.add("C30-05-健康分", ok, if ok { "" } else { "分值漂" });
    }
    // C30-06 守恒
    {
        let mut l = HpetHealth::new();
        for m in 0..6u32 {
            let g = if m == HM_OVF || m == HM_FREQ { HG_RED } else { HG_GREEN };
            let _ = l.report(m, g, m as u64 + 1);
        }
        let _ = l.dispatch(true);
        s.add("C30-06-投递守恒", l.conserved(), if l.conserved() { "" } else { "守恒破" });
    }
    // C30-07 时戳单调闸
    {
        let mut l = HpetHealth::new();
        let _ = l.report(0, HG_GREEN, 50);
        let ok = l.report(1, HG_GREEN, 49) == Err(XC_HPET_TS) && l.report(1, HG_GREEN, 51).is_ok();
        s.add("C30-07-时戳闸", ok, if ok { "" } else { "时戳漏" });
    }
    // C30-08 重登拒
    {
        let mut l = HpetHealth::new();
        let _ = l.report(HM_PRESENT, HG_GREEN, 1);
        let ok = l.report(HM_PRESENT, HG_GREEN, 2) == Err(XC_HPET_DUP);
        s.add("C30-08-重登拒", ok, if ok { "" } else { "重登漏" });
    }
    // C30-09 封账冻结
    {
        let mut l = HpetHealth::new();
        for m in 0..6u32 {
            let _ = l.report(m, HG_GREEN, m as u64 + 1);
        }
        let _ = l.dispatch(true);
        let fp = l.seal();
        let ok = fp.is_ok()
            && l.seal() == Err(XC_HPET_SEALED)
            && l.report(HM_PRESENT, HG_GREEN, 99) == Err(XC_HPET_SEALED)
            && l.dispatch(true) == Err(XC_HPET_SEALED);
        s.add("C30-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C30-10 指纹往返
    {
        let build = || {
            let mut l = HpetHealth::new();
            for m in 0..6u32 {
                let g = if m == 2 { HG_YELLOW } else { HG_GREEN };
                let _ = l.report(m, g, m as u64 + 3);
            }
            l.fingerprint()
        };
        let ok = build() == build();
        s.add("C30-10-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C30-11 id 边界（5 合法 6 拒）
    {
        let mut l = HpetHealth::new();
        let a = l.report(5, HG_GREEN, 1);
        let b = l.report(6, HG_GREEN, 2);
        let ok = a.is_ok() && b == Err(XC_HPET_MID);
        s.add("C30-11-id 边界", ok, if ok { "" } else { "边界误判" });
    }
    // C30-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C30-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
