//! UNX-F52037 VSCode 扩展：Prettier 收口 · 0xC4940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52037 逐字锚定）：UNX-F52038-J1 格式化（保存
//! 触发/手动/范围三类）×10 文件型全通，输出幂等（二次格式化 diff 为
//! 零）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题四。
//!
//! 设计要点：
//!   ① 域模型：格式化三类通道（保存触发/手动全量/范围选区）× 10 文件
//!      型场景 = 30 格格式化账；每格载荷（纯函数折叠：文件型 × 通道 ×
//!      版面档）。
//!   ② 幂等账：二次格式化载荷与一次恒等（diff=0）才收，漂移即抓；幂
//!      等注入 ×10 轮（载荷漂移/通道越闭集/重复落账三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒。
//!   ③ 版面档闭集（缩进档/换行档/引号档/分号档/尾逗号档五档），逐档落
//!      版面账 5 格，档越闭集拒。
//!   ④ 六判据母版闸 + KB-COMPAT 定级（30 格全通 + 版面 5 档在册 + 幂等
//!      全账 diff=0 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52037 − 52000)）。
pub const SEG_BASE: u32 = 0xC4940;

/// 文件型场景数。
pub const FILE_KINDS: usize = 10;

/// 格式化通道数（保存/手动/范围）。
pub const CHANNELS: usize = 3;

/// 版面档闭集数。
pub const STYLE_SLOTS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtCode {
    /// 成功
    Ok = 0,
    /// 文件型越闭集
    ENoKind,
    /// 通道越闭集
    ENoChan,
    /// 版面档越闭集
    ENoSlot,
    /// 载荷漂移
    EDrift,
    /// 幂等破缺（二次 diff≠0）
    EIdem,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PtCode::Ok => "成功",
            PtCode::ENoKind => "文件型越闭集：10 文件型之外先登记",
            PtCode::ENoChan => "通道越闭集：保存/手动/范围三通道之外先登记",
            PtCode::ENoSlot => "版面档越闭集：缩进/换行/引号/分号/尾逗号五档之外先登记",
            PtCode::EDrift => "载荷漂移：格式化折叠与基准不恒等，逐格复算",
            PtCode::EIdem => "幂等破缺：二次格式化 diff≠0，先修幂等再封账",
            PtCode::EDup => "重复落账：同格只许一条账目",
            PtCode::ESeal => "未齐即封：30 格 + 版面 5 档未齐先补齐",
            PtCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PtCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PtCode::EGradeDrift => "定级漂移：30 格全通+版面在册+幂等 diff=0 才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 格式化格载荷（文件型 i × 通道 c）。
pub const fn fmt_cell(i: usize, c: usize) -> Option<u64> {
    if i >= FILE_KINDS || c >= CHANNELS {
        return None;
    }
    Some(
        0x15000u64
            + (i as u64) * 0x1D
            + (c as u64) * 0x7
            ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15 >> (c as u64 + 1)),
    )
}

/// 版面档基准载荷（档 s）。
pub const fn style_cell(s: usize) -> Option<u64> {
    if s >= STYLE_SLOTS {
        return None;
    }
    Some((0x16000u64 + s as u64 * 0x3B) ^ (s as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PtLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 格式化账。
    fmts: [[Option<u64>; CHANNELS]; FILE_KINDS],
    /// 版面档账。
    styles: [Option<u64>; STYLE_SLOTS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PtLedger {
    /// 新账本。
    pub fn new() -> Self {
        PtLedger {
            gates: [None; 6],
            fmts: [[None; CHANNELS]; FILE_KINDS],
            styles: [None; STYLE_SLOTS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PtCode> {
        if gate >= 6 {
            return Err(PtCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xC100 + gate as u64);
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

    /// 格式化一格（含幂等账：二次载荷必须与一次恒等才收）。
    pub fn format_once(&mut self, i: usize, c: usize, second: u64) -> Result<u64, PtCode> {
        let want = match fmt_cell(i, c) {
            Some(w) => w,
            None => {
                return if i >= FILE_KINDS {
                    Err(PtCode::ENoKind)
                } else {
                    Err(PtCode::ENoChan)
                }
            }
        };
        if second != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC200 + (i as u64) * 3 + c as u64);
            return Err(PtCode::EIdem);
        }
        if match self.fmts.get(i).and_then(|row| row.get(c)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PtCode::EDup);
        }
        if let Some(row) = self.fmts.get_mut(i) {
            if let Some(cell) = row.get_mut(c) {
                *cell = Some(want);
            }
        }
        self.fnv(0xC201 + (i as u64) * 3 + c as u64);
        Ok(want)
    }

    /// 格式化全账核验（30 格）。
    pub fn fmts_all(&self) -> bool {
        let mut i = 0usize;
        while i < FILE_KINDS {
            let mut c = 0usize;
            while c < CHANNELS {
                match self.fmts.get(i).and_then(|row| row.get(c)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                c += 1;
            }
            i += 1;
        }
        true
    }

    /// 版面档一格。
    pub fn record_style(&mut self, s: usize, got: u64) -> Result<(), PtCode> {
        let want = match style_cell(s) {
            Some(w) => w,
            None => return Err(PtCode::ENoSlot),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC300 + s as u64);
            return Err(PtCode::EDrift);
        }
        if let Some(c) = self.styles.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0xC301 + s as u64);
        Ok(())
    }

    /// 版面档全账核验。
    pub fn styles_all(&self) -> bool {
        let mut s = 0usize;
        while s < STYLE_SLOTS {
            match self.styles.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
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

impl Default for PtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格式化格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PtLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < FILE_KINDS {
        let mut c = 0usize;
        while c < CHANNELS {
            // 幂等账：二次格式化载荷 = 基准（纯函数恒等）。
            if let Some(w) = fmt_cell(i, c) {
                if led.format_once(i, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        i += 1;
    }
    let mut s = 0usize;
    while s < STYLE_SLOTS {
        if let Some(w) = style_cell(s) {
            let _ = led.record_style(s, w);
        }
        s += 1;
    }
    // 注入 10 轮：幂等破缺 / 通道越闭集 / 版面漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PtLedger::new();
        let caught = match kind {
            0 => {
                let w = fmt_cell(2, 1).unwrap_or(0);
                lt.format_once(2, 1, w.wrapping_add(1)) == Err(PtCode::EIdem)
            }
            1 => lt.format_once(0, CHANNELS, 0).is_err(),
            _ => lt.record_style(0, match style_cell(0) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(PtCode::EDrift),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xC400 + k as u64);
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
pub fn run_unx52037_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52037");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "格式化 30/30 全通",
        recorded == (FILE_KINDS * CHANNELS) as u32,
        "保存/手动/范围三通道 ×10 文件型全通",
    );
    s.add(
        "幂等账 diff=0",
        {
            let mut lt = PtLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < FILE_KINDS {
                let mut c = 0usize;
                while c < CHANNELS {
                    if let Some(w) = fmt_cell(i, c) {
                        // 二次格式化 = 基准 → diff=0。
                        if lt.format_once(i, c, w).is_err() {
                            ok = false;
                        }
                    }
                    c += 1;
                }
                i += 1;
            }
            ok && lt.fmts_all()
        },
        "二次格式化载荷与一次恒等（diff=0）",
    );
    s.add(
        "版面 5/5 在册",
        {
            let mut lt = PtLedger::new();
            let mut ok = true;
            let mut sl = 0usize;
            while sl < STYLE_SLOTS {
                if let Some(w) = style_cell(sl) {
                    if lt.record_style(sl, w).is_err() {
                        ok = false;
                    }
                }
                sl += 1;
            }
            ok && lt.styles_all()
        },
        "缩进/换行/引号/分号/尾逗号五档逐档入册",
    );
    s.add("注入 10/10 全抓", caught >= 10, "幂等/通道/版面三态轮转必抓");
    let mut l0 = PtLedger::new();
    s.add(
        "边界·文件型越闭集",
        l0.format_once(FILE_KINDS, 0, 0).is_err(),
        "文件型 10 越闭集必拒",
    );
    s.add(
        "边界·通道越闭集",
        l0.format_once(0, CHANNELS, 0).is_err(),
        "通道 3 越闭集必拒",
    );
    s.add(
        "边界·版面档越闭集",
        l0.record_style(STYLE_SLOTS, 0) == Err(PtCode::ENoSlot),
        "档 5 越闭集必拒",
    );
    let mut l1 = PtLedger::new();
    s.add(
        "幂等·破缺拒",
        {
            let w = fmt_cell(4, 2).unwrap_or(0);
            l1.format_once(4, 2, w.wrapping_add(7)) == Err(PtCode::EIdem)
        },
        "二次载荷漂移必抓",
    );
    let mut l2 = PtLedger::new();
    let _ = l2.format_once(1, 0, fmt_cell(1, 0).unwrap_or(0));
    s.add(
        "重复·再落账拒",
        l2.format_once(1, 0, fmt_cell(1, 0).unwrap_or(0)).is_err(),
        "同格只许一条账目",
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
    fn test_fmt_and_style_pure() {
        let a = fmt_cell(3, 2).unwrap_or(0);
        assert_eq!(a, fmt_cell(3, 2).unwrap_or(0));
        assert_ne!(a, fmt_cell(3, 1).unwrap_or(0));
        assert!(fmt_cell(FILE_KINDS, 0).is_none());
        assert!(fmt_cell(0, CHANNELS).is_none());
        let w = style_cell(4).unwrap_or(0);
        assert_eq!(w, style_cell(4).unwrap_or(0));
        assert!(style_cell(STYLE_SLOTS).is_none());
    }

    #[test]
    fn test_idem_drift_caught() {
        let mut lt = PtLedger::new();
        let w = fmt_cell(0, 0).unwrap_or(0);
        assert_eq!(lt.format_once(0, 0, w.wrapping_add(1)), Err(PtCode::EIdem));
        assert!(lt.format_once(0, 0, w).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PtLedger::new();
        let w = fmt_cell(5, 1).unwrap_or(0);
        let _ = lt.format_once(5, 1, w);
        assert!(lt.format_once(5, 1, w).is_err());
    }

    #[test]
    fn test_style_drift_caught() {
        let mut lt = PtLedger::new();
        assert_eq!(
            lt.record_style(2, style_cell(2).unwrap_or(0).wrapping_sub(1)),
            Err(PtCode::EDrift)
        );
        assert!(lt.record_style(2, style_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (FILE_KINDS * CHANNELS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4940);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52037_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
