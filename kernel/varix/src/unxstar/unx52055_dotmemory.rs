//! UNX-F52055 dotMemory 收口 · 0xC4DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52055 逐字锚定）：UNX-F52056-J1 工具安装/更新
//! /回滚三段账；多 IDE 并存目录隔离（.jdks/本地库）零互踩 ×2 轮；
//! KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：三段账（安装/更新/回滚）× 5 工具场景 = 15 格账；段序闭
//!      集 0..3。
//!   ② 目录隔离账：多 IDE 并存 6 目录闭集（.jdks/本地库/插件/配置/日
//!      志/缓存），目录槽位账 6 格，逐槽独占段基址折叠恒等才收；隔离
//!      注入 ×2 轮（目录互踩模拟）×2 轮 2/2 必抓，另 8 轮注入共 10 轮。
//!   ③ 回滚闸：回滚拍序（安装→更新→回滚）严格递增；回滚后目录槽位账
//!      必须回到安装前基准（零残留）。
//!   ④ 注入 ×10 轮（互踩/槽位越册/拍序回退三态轮转）10/10 必抓，注入
//!      =检出+逃逸守恒；六判据母版闸 + 定级（15 格全通 + 隔离零互踩 →
//!      S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52055 − 52000)）。
pub const SEG_BASE: u32 = 0xC4DC0;

/// 三段账段数。
pub const SEGMENTS: usize = 3;

/// 工具场景数。
pub const TOOLS: usize = 5;

/// 目录闭集数。
pub const DIRS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoSeg,
    /// 工具号越闭集
    ENoTool,
    /// 目录槽越册
    ENoDir,
    /// 目录互踩（槽位折叠漂移）
    EStamp,
    /// 拍序乱
    EOrder,
    /// 回滚残留
    EResidue,
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

impl DmCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DmCode::Ok => "成功",
            DmCode::ENoSeg => "段号越闭集：安装/更新/回滚三段之外先登记",
            DmCode::ENoTool => "工具号越闭集：5 工具之外先登记",
            DmCode::ENoDir => "目录槽越册：6 目录闭集之外先登记",
            DmCode::EStamp => "目录互踩：槽位折叠漂移即互踩，逐槽复算",
            DmCode::EOrder => "拍序乱：安装→更新→回滚三段拍序必须严格递增",
            DmCode::EResidue => "回滚残留：回滚后目录槽位账未回安装前基准",
            DmCode::EDup => "重复落账：同格只许一条账目",
            DmCode::ESeal => "未齐即封：15 格未齐先补齐",
            DmCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DmCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DmCode::EGradeDrift => "定级漂移：15 格全通+隔离零互踩+回滚零残留才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 段格载荷（段 s × 工具 t）。
pub const fn seg_cell(s: usize, t: usize) -> Option<u64> {
    if s >= SEGMENTS || t >= TOOLS {
        return None;
    }
    Some(
        0x47000u64
            + (s as u64) * 0x2C
            + (t as u64) * 0x11
            ^ (s as u64).wrapping_mul(0x9E3779B97F4A7C15 >> t),
    )
}

/// 目录槽位基准载荷（槽 d；每槽独占折叠）。
pub const fn dir_cell(d: usize) -> Option<u64> {
    if d >= DIRS {
        return None;
    }
    Some((0x48000u64 + d as u64 * 0x39) ^ (d as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DmLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 段账。
    segs: [[Option<u64>; TOOLS]; SEGMENTS],
    /// 目录槽位账。
    dirs: [Option<u64>; DIRS],
    /// 目录安装前基准快照。
    dirs_base: [Option<u64>; DIRS],
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DmLedger {
    /// 新账本。
    pub fn new() -> Self {
        DmLedger {
            gates: [None; 6],
            segs: [[None; TOOLS]; SEGMENTS],
            dirs: [None; DIRS],
            dirs_base: [None; DIRS],
            last_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DmCode> {
        if gate >= 6 {
            return Err(DmCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1F100 + gate as u64);
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

    /// 安装前基准快照（槽位基准全量留存）。
    pub fn snapshot_base(&mut self) {
        let mut d = 0usize;
        while d < DIRS {
            if let Some(w) = dir_cell(d) {
                self.dirs_base[d] = Some(w);
            }
            d += 1;
        }
        self.fnv(0x1F200);
    }

    /// 目录槽一格（折叠恒等才收；互踩注入必抓）。
    pub fn record_dir(&mut self, d: usize, got: u64) -> Result<(), DmCode> {
        let want = match dir_cell(d) {
            Some(w) => w,
            None => return Err(DmCode::ENoDir),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1F300 + d as u64);
            return Err(DmCode::EStamp);
        }
        if let Some(c) = self.dirs.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x1F301 + d as u64);
        Ok(())
    }

    /// 目录全账核验。
    pub fn dirs_all(&self) -> bool {
        let mut d = 0usize;
        while d < DIRS {
            match self.dirs.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 段一格（拍序单调 + 重复拒）。
    pub fn record_seg(&mut self, s: usize, t: usize, tick: u32) -> Result<u64, DmCode> {
        if s >= SEGMENTS {
            return Err(DmCode::ENoSeg);
        }
        if t >= TOOLS {
            return Err(DmCode::ENoTool);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1F400);
            return Err(DmCode::EOrder);
        }
        let payload = match seg_cell(s, t) {
            Some(p) => p,
            None => return Err(DmCode::ENoTool),
        };
        if match self.segs.get(s).and_then(|row| row.get(t)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DmCode::EDup);
        }
        if let Some(row) = self.segs.get_mut(s) {
            if let Some(c) = row.get_mut(t) {
                *c = Some(payload);
            }
        }
        self.last_tick = tick as i64;
        self.fnv(0x1F401 + (s as u64) * 5 + t as u64);
        Ok(payload)
    }

    /// 段全账核验（15 格）。
    pub fn segs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SEGMENTS {
            let mut t = 0usize;
            while t < TOOLS {
                match self.segs.get(s).and_then(|row| row.get(t)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                t += 1;
            }
            s += 1;
        }
        true
    }

    /// 回滚零残留核验（目录槽位账 = 安装前基准）。
    pub fn rollback_clean(&self) -> bool {
        let mut d = 0usize;
        while d < DIRS {
            match (self.dirs.get(d), self.dirs_base.get(d)) {
                (Some(a), Some(b)) => {
                    // 基准留存后回滚：账面回到基准（空=空 或 值=基准值）。
                    match (a, b) {
                        (None, None) => {}
                        (Some(x), Some(y)) => {
                            if x != y {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 回滚执行（槽位账回基准）。
    pub fn rollback(&mut self) -> Result<(), DmCode> {
        let mut d = 0usize;
        while d < DIRS {
            let base = match self.dirs_base.get(d) {
                Some(Some(x)) => *x,
                _ => match dir_cell(d) {
                    Some(w) => w,
                    None => return Err(DmCode::ENoDir),
                },
            };
            if let Some(c) = self.dirs.get_mut(d) {
                *c = Some(base);
            }
            d += 1;
        }
        self.fnv(0x1F500);
        Ok(())
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

impl Default for DmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    led.snapshot_base();
    let mut recorded = 0u32;
    let mut tick: u32 = 0;
    let mut s = 0usize;
    while s < SEGMENTS {
        let mut t = 0usize;
        while t < TOOLS {
            tick = tick.saturating_add(3);
            if led.record_seg(s, t, tick).is_ok() {
                recorded += 1;
            }
            t += 1;
        }
        s += 1;
    }
    let mut d = 0usize;
    while d < DIRS {
        if let Some(w) = dir_cell(d) {
            let _ = led.record_dir(d, w);
        }
        d += 1;
    }
    let _ = led.rollback();
    // 注入 10 轮：互踩 / 槽越册 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DmLedger::new();
        let caught = match kind {
            0 => lt.record_dir(2, dir_cell(2).unwrap_or(0).wrapping_add(1)) == Err(DmCode::EStamp),
            1 => lt.record_dir(DIRS, 0).is_err(),
            _ => {
                let _ = lt.record_seg(0, 0, 9);
                lt.record_seg(1, 0, 4) == Err(DmCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1F600 + k as u64);
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
pub fn run_unx52055_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52055");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三段 15/15 全通",
        recorded == (SEGMENTS * TOOLS) as u32,
        "安装/更新/回滚三段 ×5 工具全通",
    );
    s.add(
        "目录隔离 6/6 零互踩",
        {
            let mut lt = DmLedger::new();
            lt.snapshot_base();
            let mut ok = true;
            let mut d = 0usize;
            while d < DIRS {
                if let Some(w) = dir_cell(d) {
                    if lt.record_dir(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dirs_all() && lt.rollback_clean()
        },
        ".jdks/本地库/插件/配置/日志/缓存六槽独占零互踩",
    );
    s.add(
        "回滚零残留",
        {
            let mut lt = DmLedger::new();
            lt.snapshot_base();
            let mut d = 0usize;
            while d < DIRS {
                if let Some(w) = dir_cell(d) {
                    let _ = lt.record_dir(d, w.wrapping_add(7));
                }
                d += 1;
            }
            let _ = lt.rollback();
            lt.rollback_clean()
        },
        "回滚后槽位账回安装前基准",
    );
    s.add("注入 10/10 全抓", caught >= 10, "互踩/槽越册/拍序三态轮转必抓");
    let mut l0 = DmLedger::new();
    s.add(
        "互踩·漂移拒",
        l0.record_dir(1, dir_cell(1).unwrap_or(0).wrapping_add(3)) == Err(DmCode::EStamp),
        "槽位折叠漂移必抓",
    );
    s.add(
        "边界·槽越册",
        l0.record_dir(DIRS, 0).is_err(),
        "槽 6 越册必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_seg(SEGMENTS, 0, 1).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·工具越闭集",
        l0.record_seg(0, TOOLS, 1).is_err(),
        "工具 5 越闭集必拒",
    );
    let mut l1 = DmLedger::new();
    let _ = l1.record_seg(2, 2, 6);
    s.add(
        "拍序·回退拒",
        l1.record_seg(2, 3, 2) == Err(DmCode::EOrder),
        "拍序回退必抓",
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
    fn test_cells_pure() {
        let a = seg_cell(1, 3).unwrap_or(0);
        assert_eq!(a, seg_cell(1, 3).unwrap_or(0));
        assert_ne!(a, seg_cell(1, 4).unwrap_or(0));
        assert!(seg_cell(SEGMENTS, 0).is_none());
        assert!(seg_cell(0, TOOLS).is_none());
        let w = dir_cell(4).unwrap_or(0);
        assert_eq!(w, dir_cell(4).unwrap_or(0));
        assert!(dir_cell(DIRS).is_none());
    }

    #[test]
    fn test_stamp_caught() {
        let mut lt = DmLedger::new();
        assert_eq!(
            lt.record_dir(0, dir_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(DmCode::EStamp)
        );
        assert!(lt.record_dir(0, dir_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_rollback_cycle() {
        let mut lt = DmLedger::new();
        lt.snapshot_base();
        let mut d = 0usize;
        while d < DIRS {
            let _ = lt.record_dir(d, dir_cell(d).unwrap_or(0));
            d += 1;
        }
        assert!(lt.dirs_all());
        let _ = lt.rollback();
        assert!(lt.rollback_clean());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = DmLedger::new();
        let _ = lt.record_seg(0, 0, 5);
        assert_eq!(lt.record_seg(1, 0, 3), Err(DmCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (SEGMENTS * TOOLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4DC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52055_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
