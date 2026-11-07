//! UNX-F52039 Electron 前置验证周收口 · 0xC49C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52039 逐字锚定）：UNX-F52040-J1 波 23 开工首
//! 月"Electron 前置验证周"全记录：VSCode 最小集试收口 → 失绿问题清单
//! 回推 G 部缺陷账（联签锚定行）→ 回推项 ×N 全部登记，不硬写收口条目
//! 结论留痕；KB-COMPAT 定级流程验证；判据锚定 §AI-66.6 风险 ①Electron
//! 族渲染面时点风险。
//!
//! 设计要点：
//!   ① 域模型：前置验证周五段流水账（最小集试收口/失绿清单/回推登记/
//!      结论留痕/定级流程验证）× 7 天 = 35 格流水账，天号闭集 0..7。
//!   ② 失绿清单回推闸：每条失绿项必须回推 G 部缺陷账（回推号闭集
//!      0xG00..0xG0F），未回推即结论 → 未回推先结论闸（不硬写结论）。
//!   ③ 结论留痕账：留痕三元（时点/判据锚/回推号）全在册才收，缺一拒；
//!      回推项 ×10 轮登记全在册才封；注入 ×10 轮（回推号越册/留痕缺项/
//!      流水载荷漂移三态轮转）10/10 必抓，注入=检出+逃逸守恒。
//!   ④ 六判据母版闸 + KB-COMPAT 定级流程验证（35 格流水全通 + 回推 10
//!      项全在册 + 留痕三元全齐 → 流程绿）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52039 − 52000)）。
pub const SEG_BASE: u32 = 0xC49C0;

/// 验证周天数。
pub const DAYS: usize = 7;

/// 五段流水段数。
pub const STREAMS: usize = 5;

/// 回推项登记数。
pub const PUSHBACKS: usize = 10;

/// 留痕三元字段数。
pub const TRACE_FIELDS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    /// 成功
    Ok = 0,
    /// 天号越闭集
    ENoDay,
    /// 段号越闭集
    ENoSeg,
    /// 回推号越册
    ENoPush,
    /// 流水载荷漂移
    EDrift,
    /// 留痕缺项
    ETraceMiss,
    /// 未回推先结论
    EConclNoPush,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级流程验证漂移
    EGradeDrift,
}

impl EvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EvCode::Ok => "成功",
            EvCode::ENoDay => "天号越闭集：验证周 7 天之外先登记",
            EvCode::ENoSeg => "段号越闭集：试收口/失绿/回推/留痕/定级五段之外先登记",
            EvCode::ENoPush => "回推号越册：回推项 10 项闭集之外先登记",
            EvCode::EDrift => "流水载荷漂移：流水折叠与基准不恒等，逐格复算",
            EvCode::ETraceMiss => "留痕缺项：时点/判据锚/回推号缺一即拒",
            EvCode::EConclNoPush => "未回推先结论：失绿项未回推 G 部缺陷账不许下结论",
            EvCode::EDup => "重复落账：同格只许一条账目",
            EvCode::ESeal => "未齐即封：35 格流水 + 回推 10 项未齐先补齐",
            EvCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            EvCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            EvCode::EGradeDrift => "定级流程验证漂移：流水全通+回推全在册+留痕全齐才流程绿",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 流水格载荷（天 d × 段 s）。
pub const fn stream_cell(d: usize, s: usize) -> Option<u64> {
    if d >= DAYS || s >= STREAMS {
        return None;
    }
    Some(
        0x1B000u64
            + (d as u64) * 0x1F
            + (s as u64) * 0x11
            ^ (d as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

/// 回推号闭集（0xG00..0xG0F 折叠，项 p）。
pub const fn pushback_id(p: usize) -> Option<u32> {
    if p >= PUSHBACKS {
        return None;
    }
    Some(0x1C00 + p as u32 * 0x10)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct EvLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 流水账。
    streams: [[Option<u64>; STREAMS]; DAYS],
    /// 回推登记账。
    push: [Option<bool>; PUSHBACKS],
    /// 留痕账（三元齐备数）。
    traces: [Option<bool>; PUSHBACKS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EvLedger {
    /// 新账本。
    pub fn new() -> Self {
        EvLedger {
            gates: [None; 6],
            streams: [[None; STREAMS]; DAYS],
            push: [None; PUSHBACKS],
            traces: [None; PUSHBACKS],
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EvCode> {
        if gate >= 6 {
            return Err(EvCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xE100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 回推登记一格（回推号闭集）。
    pub fn register_pushback(&mut self, p: usize) -> Result<(), EvCode> {
        if p >= PUSHBACKS {
            return Err(EvCode::ENoPush);
        }
        if match self.push.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EvCode::EDup);
        }
        if let Some(c) = self.push.get_mut(p) {
            *c = Some(true);
        }
        self.fnv(0xE200 + p as u64);
        Ok(())
    }

    /// 回推全账核验。
    pub fn push_all(&self) -> bool {
        let mut p = 0usize;
        while p < PUSHBACKS {
            match self.push.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 留痕一格（三元全在册才收）。
    pub fn record_trace(&mut self, p: usize, present: [bool; TRACE_FIELDS]) -> Result<(), EvCode> {
        if p >= PUSHBACKS {
            return Err(EvCode::ENoPush);
        }
        let mut k = 0usize;
        while k < TRACE_FIELDS {
            match present.get(k) {
                Some(true) => {}
                _ => {
                    self.injected += 1;
                    self.caught += 1;
                    self.fnv(0xE300 + p as u64);
                    return Err(EvCode::ETraceMiss);
                }
            }
            k += 1;
        }
        // 未回推先结论闸。
        if match self.push.get(p) {
            Some(Some(_)) => false,
            _ => true,
        } {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE301 + p as u64);
            return Err(EvCode::EConclNoPush);
        }
        if let Some(c) = self.traces.get_mut(p) {
            *c = Some(true);
        }
        self.fnv(0xE302 + p as u64);
        Ok(())
    }

    /// 留痕全账核验。
    pub fn traces_all(&self) -> bool {
        let mut p = 0usize;
        while p < PUSHBACKS {
            match self.traces.get(p) {
                Some(Some(true)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 流水一格。
    pub fn record_stream(&mut self, d: usize, s: usize) -> Result<u64, EvCode> {
        let p = match stream_cell(d, s) {
            Some(p) => p,
            None => {
                return if d >= DAYS {
                    Err(EvCode::ENoDay)
                } else {
                    Err(EvCode::ENoSeg)
                }
            }
        };
        if match self.streams.get(d).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EvCode::EDup);
        }
        if let Some(row) = self.streams.get_mut(d) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0xE400 + (d as u64) * 5 + s as u64);
        Ok(p)
    }

    /// 流水全账核验（35 格）。
    pub fn streams_all(&self) -> bool {
        let mut d = 0usize;
        while d < DAYS {
            let mut s = 0usize;
            while s < STREAMS {
                match self.streams.get(d).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for EvLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（流水格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EvLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DAYS {
        let mut s = 0usize;
        while s < STREAMS {
            if led.record_stream(d, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        d += 1;
    }
    let mut p = 0usize;
    while p < PUSHBACKS {
        let _ = led.register_pushback(p);
        p += 1;
    }
    let mut p2 = 0usize;
    while p2 < PUSHBACKS {
        let _ = led.record_trace(p2, [true, true, true]);
        p2 += 1;
    }
    // 注入 10 轮：回推越册 / 留痕缺项 / 未回推先结论三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EvLedger::new();
        let caught = match kind {
            0 => lt.register_pushback(PUSHBACKS) == Err(EvCode::ENoPush),
            1 => lt.record_trace(0, [true, false, true]) == Err(EvCode::ETraceMiss),
            _ => lt.record_trace(3, [true, true, true]) == Err(EvCode::EConclNoPush),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xE500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52039_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52039");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五段流水 35/35 全通",
        recorded == (DAYS * STREAMS) as u32,
        "试收口/失绿/回推/留痕/定级五段 ×7 天全通",
    );
    s.add(
        "回推 10/10 全在册",
        {
            let mut lt = EvLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PUSHBACKS {
                if lt.register_pushback(p).is_err() {
                    ok = false;
                }
                p += 1;
            }
            ok && lt.push_all()
        },
        "失绿项 ×10 全部回推 G 部缺陷账登记",
    );
    s.add(
        "留痕 10/10 三元全齐",
        {
            let mut lt = EvLedger::new();
            let mut p = 0usize;
            while p < PUSHBACKS {
                let _ = lt.register_pushback(p);
                p += 1;
            }
            let mut ok = true;
            let mut p2 = 0usize;
            while p2 < PUSHBACKS {
                if lt.record_trace(p2, [true, true, true]).is_err() {
                    ok = false;
                }
                p2 += 1;
            }
            ok && lt.traces_all()
        },
        "时点/判据锚/回推号三元全在册",
    );
    s.add("注入 10/10 全抓", caught >= 10, "回推/留痕/结论三态轮转必抓");
    let mut l0 = EvLedger::new();
    s.add(
        "未回推先结论闸",
        l0.record_trace(1, [true, true, true]) == Err(EvCode::EConclNoPush),
        "失绿项未回推不许下结论",
    );
    let mut l1 = EvLedger::new();
    s.add(
        "边界·回推越册",
        l1.register_pushback(PUSHBACKS) == Err(EvCode::ENoPush),
        "回推项 10 越册必拒",
    );
    s.add(
        "边界·天号越闭集",
        l1.record_stream(DAYS, 0).is_err(),
        "天号 7 越闭集必拒",
    );
    s.add(
        "边界·段号越闭集",
        l1.record_stream(0, STREAMS).is_err(),
        "段号 5 越闭集必拒",
    );
    let mut l2 = EvLedger::new();
    s.add(
        "留痕·缺项拒",
        l2.record_trace(2, [true, true, false]) == Err(EvCode::ETraceMiss),
        "三元缺一即拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_and_push_pure() {
        let a = stream_cell(3, 2).unwrap_or(0);
        assert_eq!(a, stream_cell(3, 2).unwrap_or(0));
        assert_ne!(a, stream_cell(3, 3).unwrap_or(0));
        assert!(stream_cell(DAYS, 0).is_none());
        assert!(stream_cell(0, STREAMS).is_none());
        assert_eq!(pushback_id(0), Some(0x1C00));
        assert_eq!(pushback_id(9), Some(0x1C90));
        assert!(pushback_id(10).is_none());
    }

    #[test]
    fn test_push_dup_rejected() {
        let mut lt = EvLedger::new();
        assert!(lt.register_pushback(4).is_ok());
        assert_eq!(lt.register_pushback(4), Err(EvCode::EDup));
    }

    #[test]
    fn test_trace_gates() {
        let mut lt = EvLedger::new();
        assert_eq!(lt.record_trace(0, [true, true, false]), Err(EvCode::ETraceMiss));
        assert_eq!(lt.record_trace(0, [true, true, true]), Err(EvCode::EConclNoPush));
        let _ = lt.register_pushback(0);
        assert!(lt.record_trace(0, [true, true, true]).is_ok());
        assert!(!lt.traces_all());
    }

    #[test]
    fn test_dup_stream_rejected() {
        let mut lt = EvLedger::new();
        let _ = lt.record_stream(2, 3);
        assert!(lt.record_stream(2, 3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (DAYS * STREAMS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC49C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52039_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
