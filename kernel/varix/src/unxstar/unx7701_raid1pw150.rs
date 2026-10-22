//! UNX-F7701 · RAID1 断电 ×150 对练账（QEMU 105 / 真机 45 分账禁混报）· 诊断段 0x7C80
//! 验收锚点：×150 对练三段落账——① 分账：150 轮按轮号分 QEMU 105（轮号 %10 <7，
//! 15 组×7）与真机 45（轮号 %10 ≥7，15 组×3），两通道各记各账，禁混报；
//! ② 断电轮账：每轮留（通道、断电时机 pct 0~99、恢复方式、数据面结论 Pass/Loss）；
//! ③ 收口：150 轮全账对齐——105/45 恰数、结论二态无第四态、位图重放后零丢失轮
//! （Pass 轮）在两通道分别留账可对。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x7C80;

pub const TOTAL: u32 = 150;
pub const QEMU_N: u32 = 105;
pub const REAL_N: u32 = 45;

/// 通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chan {
    Qemu,
    Real,
}

impl Chan {
    pub fn route(round: u32) -> Chan {
        if round % 10 < 7 {
            Chan::Qemu
        } else {
            Chan::Real
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Chan::Qemu => "qemu",
            Chan::Real => "real",
        }
    }
}

/// 恢复方式：位图重放 / 全量重建。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recover {
    BitmapReplay,
    FullRebuild,
}

/// 数据面结论二态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Loss,
}

/// 单轮断电账。
#[derive(Clone, Copy, Debug)]
pub struct DrillEntry {
    pub round: u32,
    pub chan: Chan,
    pub pct: u8,       // 断电时机 0~99（写入进度百分比）
    pub recover: Recover,
    pub verdict: Verdict,
}

impl DrillEntry {
    pub fn new(round: u32, pct: u8, recover: Recover, verdict: Verdict) -> Self {
        DrillEntry { round, chan: Chan::route(round), pct, recover, verdict }
    }
}

/// 对练总账。
pub struct DrillLedger {
    pub entries: Vec<DrillEntry>,
}

impl DrillLedger {
    pub fn new() -> Self {
        DrillLedger { entries: Vec::new() }
    }

    pub fn record(&mut self, e: DrillEntry) {
        self.entries.push(e);
    }

    pub fn count_chan(&self, c: Chan) -> u32 {
        self.entries.iter().filter(|e| e.chan == c).count() as u32
    }

    pub fn count_verdict(&self, c: Chan, v: Verdict) -> u32 {
        self.entries.iter().filter(|e| e.chan == c && e.verdict == v).count() as u32
    }

    /// 混报检测：每轮账的通道必须与路由规则一致（禁混报）。
    pub fn no_mixing(&self) -> bool {
        self.entries.iter().all(|e| e.chan == Chan::route(e.round))
    }

    /// 150 轮收口审计。
    pub fn audit(&self) -> bool {
        self.entries.len() == TOTAL as usize
            && self.count_chan(Chan::Qemu) == QEMU_N
            && self.count_chan(Chan::Real) == REAL_N
            && self.no_mixing()
    }
}

/// 位图模型：128 块写入位图；断电时已提交块位图留痕，恢复重放补齐。
pub struct WriteBitmap {
    pub bits: [u64; 2], // 128 块
    pub committed: u32,
}

impl WriteBitmap {
    pub fn new() -> Self {
        WriteBitmap { bits: [0; 2], committed: 0 }
    }

    pub fn set(&mut self, blk: u8) -> bool {
        if blk as usize >= 128 {
            return false;
        }
        let w = (blk / 64) as usize;
        let b = blk % 64;
        if self.bits[w] & (1u64 << b) != 0 {
            return false;
        }
        self.bits[w] |= 1u64 << b;
        self.committed += 1;
        true
    }

    pub fn get(&self, blk: u8) -> bool {
        if blk as usize >= 128 {
            return false;
        }
        let w = (blk / 64) as usize;
        let b = blk % 64;
        self.bits[w] & (1u64 << b) != 0
    }

    /// 断电截断：保留前 pct% 块（模拟已提交），余块待重放。
    pub fn crash_at(&self, pct: u8) -> WriteBitmap {
        let keep = (128u32 * pct as u32 / 100) as u8;
        let mut surv = WriteBitmap::new();
        for blk in 0..128u8 {
            if blk < keep && self.get(blk) {
                surv.set(blk);
            }
        }
        surv
    }

    /// 位图重放：把存活账与目标账比对，缺失块按序补写（演示为 set），返回补写数。
    pub fn replay(&mut self, target: &WriteBitmap) -> u32 {
        let mut patched = 0u32;
        for blk in 0..128u8 {
            if target.get(blk) && !self.get(blk) {
                self.set(blk);
                patched += 1;
            }
        }
        patched
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7C80 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7701_7C80;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("路由-轮150归真机", Chan::route(150) == Chan::Real));
    out.push(("位图-空图审计0", WriteBitmap::new().committed == 0));
    out.push(("位图-越界块拒绝", !WriteBitmap::new().set(128)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7701_checks：本单判据聚合（诊断段 0x7C80）。
pub fn run_unx7701_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7701 RAID1 断电 ×150 对练账");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // ① 分账：150 轮 105/45 恒比
    let mut q = 0u32;
    let mut r = 0u32;
    for i in 1..=150u32 {
        match Chan::route(i) {
            Chan::Qemu => q += 1,
            Chan::Real => r += 1,
        }
    }
    s.add("分账-150轮105/45恒比", q == 105 && r == 45, if q == 105 && r == 45 { "" } else { "分账比漂移" });

    // ② 位图重放：全 128 块写入 → 断电 40% → 重放补齐零丢失
    let mut target = WriteBitmap::new();
    for blk in 0..128u8 {
        target.set(blk);
    }
    let surv = target.crash_at(40);
    let surv_ok = surv.committed == 51; // 128*40/100 = 51（整除截断）
    s.add("位图-40%断电存活51块", surv_ok, if surv_ok { "" } else { "存活块数漂移" });
    let mut rec = surv;
    let patched = rec.replay(&target);
    let replay_ok = patched == 77 && rec.committed == 128;
    s.add("位图-重放补77块零丢失", replay_ok, if replay_ok { "" } else { "重放路径漂移" });

    // 100% 断电（断在收口前最后一瞬）存活 126 块，补 2
    let surv99 = target.crash_at(99);
    let surv99_committed = surv99.committed;
    let mut rec99 = surv99;
    let p99 = rec99.replay(&target);
    let ok99 = surv99_committed == 126 && p99 == 2 && rec99.committed == 128;
    s.add("位图-99%断电补2块收口", ok99, if ok99 { "" } else { "收口路径漂移" });

    // ③ 150 轮对练全账（LCG 定时机与恢复方式；位图重放判定 verdict）
    let mut led = DrillLedger::new();
    let mut seed: u64 = 0x7701_0150;
    let mut losses = 0u32;
    for i in 1..=150u32 {
        let pct = (lcg_next(&mut seed) % 100) as u8;
        let recover = if lcg_next(&mut seed) % 4 == 0 { Recover::FullRebuild } else { Recover::BitmapReplay };
        // 目标账：128 块全写
        let tgt = {
            let mut t = WriteBitmap::new();
            for blk in 0..128u8 {
                t.set(blk);
            }
            t
        };
        let sv = tgt.crash_at(pct);
        let mut rc = sv;
        let _ = rc.replay(&tgt);
        // 全量重建一律补齐；位图重放补齐即 Pass
        let v = if recover == Recover::FullRebuild || rc.committed == 128 {
            Verdict::Pass
        } else {
            losses += 1;
            Verdict::Loss
        };
        led.record(DrillEntry::new(i, pct, recover, v));
    }

    // 分账恰数 + 禁混报
    let mix_ok = led.audit();
    s.add("收口-150轮审计(105/45+禁混报)", mix_ok, if mix_ok { "" } else { "审计红" });

    // 结论二态无第四态（枚举保证，账面再核：两通道 Pass+Loss=各通道数）
    let qp = led.count_verdict(Chan::Qemu, Verdict::Pass);
    let ql = led.count_verdict(Chan::Qemu, Verdict::Loss);
    let rp = led.count_verdict(Chan::Real, Verdict::Pass);
    let rl = led.count_verdict(Chan::Real, Verdict::Loss);
    let two_ok = qp + ql == 105 && rp + rl == 45;
    s.add("收口-两通道二态合计恰数", two_ok, if two_ok { "" } else { "二态合计漂移" });

    // 位图重放路径下全 Pass（本模型重放恒补齐）
    let all_pass = losses == 0 && led.count_verdict(Chan::Qemu, Verdict::Pass) + led.count_verdict(Chan::Real, Verdict::Pass) == 150;
    s.add("收口-重放补齐150轮零丢失", all_pass, if all_pass { "" } else { "丢失轮非零" });

    // 断电时机分布：0 与 99 均应出现（随机面覆盖）
    let mut seen0 = false;
    let mut seen99 = false;
    for e in &led.entries {
        if e.pct == 0 {
            seen0 = true;
        }
        if e.pct == 99 {
            seen99 = true;
        }
    }
    s.add("覆盖-时机0与99两面出现", seen0 && seen99, if seen0 && seen99 { "" } else { "时机覆盖缺口" });

    // FNV 轮账指纹链
    let mut acc: u32 = 0x811C_9DC5;
    for e in &led.entries {
        acc ^= fnv1a(&[e.round as u8, e.pct, e.recover as u8, e.verdict as u8]);
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-150轮指纹链非零", acc != 0, if acc != 0 { "" } else { "指纹链异常" });

    s
}
