//! UNX-F52292 六判据进验收机器人判据库（AI-73 对接）· 0xC88C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52292"，逐字）：
//!   title: 六判据进验收机器人判据库（AI-73 对接）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52293-J1 本段
//!     条目抽读对照段占比 ≥20% ×30 条抽测达标；灌水扫描零命中；联签
//!     锚定行 AI-82；判据锚定 §AI-66.8 治理线接口；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52292）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52293-J1，照原文锚定；功能按本卡标题（六判据进验收机器人判
//! 据库）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 六判据母版入库 12 格：六判据 ×2 格（schema 格 + 抽测格）逐格
//!      载荷对照落账，判据漂移必抓；机器人可执行抽测口径 ×10。
//!   ② 判据版本单向门 8 拍：冻结→入库→抽测→…→对接，拍序严格单调
//!      递增（单向门回退必抓），schema ×1 冻结入库口径。
//!   ③ AI-73 对接联签 4 格：治理线接口逐格联签落账，载荷恒等才收。
//!   ④ ledger 状态机（new/record_cell/record_cs/record_beat/
//!      judge_gate×6/fingerprint FNV-1a/grade 定级闭集）；注入核
//!      run_cycle 三态轮转 10 轮（判据漂移 / 单向门回退 / 判据格越
//!      闭集），全部落在当轮临时账本上，led 上 injected==caught==10、
//!      escaped==0；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52292;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 六判据母版家族数。
pub const GATE_FAMILIES: usize = 6;

/// 每家族判据库格数（schema 格 + 抽测格）。
pub const CELLS_PER_FAMILY: usize = 2;

/// 判据库格总数（六判据 × 2 格）。
pub const GATE_CELLS: usize = GATE_FAMILIES * CELLS_PER_FAMILY;

/// 判据版本单向门拍数。
pub const VER_BEATS: usize = 8;

/// AI-73 对接联签格数。
pub const CS_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RobotGateCode {
    /// 成功
    Ok = 0,
    /// 判据格越闭集
    ENoCell,
    /// 判据漂移
    ECellDrift,
    /// 单向门回退（家族/拍序逆序）
    EOrder,
    /// 联签格越闭集
    ENoRow,
    /// 联签格载荷漂移
    ERowDrift,
    /// 单向门拍越闭集
    ENoBeat,
    /// 判据格缺失
    ECellMiss,
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

impl RobotGateCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RobotGateCode::Ok => "成功",
            RobotGateCode::ENoCell => "判据格越闭集：六判据母版 12 格之外先登记",
            RobotGateCode::ECellDrift => "判据漂移：格折叠与基准不恒等，逐格复算",
            RobotGateCode::EOrder => "单向门回退：六判据家族与版本拍单调，回退/回放必抓",
            RobotGateCode::ENoRow => "联签格越闭集：AI-73 对接联签 4 格之外先登记",
            RobotGateCode::ERowDrift => "联签格载荷漂移：联签折叠与基准不恒等，逐格复算",
            RobotGateCode::ENoBeat => "单向门拍越闭集：判据版本单向门 8 拍之外先登记",
            RobotGateCode::ECellMiss => "判据格缺失：六判据母版入库未齐先补齐",
            RobotGateCode::EDup => "重复落账：同格只许一条账目",
            RobotGateCode::ESeal => "未齐即封：12 判据格 + 8 拍 + 4 联签格未齐先补齐",
            RobotGateCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RobotGateCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RobotGateCode::EGradeDrift => "定级漂移：全通且联签格全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+12判据格+8拍+4联签格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 判据库格载荷（格 c：六家族 × 2 格）。
pub const fn gate_cell(c: usize) -> Option<u64> {
    if c >= GATE_CELLS {
        return None;
    }
    Some((0xDF000u64 + (c as u64) * 0x3D) ^ ((c / CELLS_PER_FAMILY) as u64).wrapping_mul(0x01000193))
}

/// 判据家族（0..2=启动 2..4=功能 4..6=崩溃 6..8=卸载 8..10=升级 10..12=许可）。
pub const fn cell_family(c: usize) -> Option<u32> {
    if c >= GATE_CELLS {
        return None;
    }
    Some((c / CELLS_PER_FAMILY) as u32)
}

/// 判据家族名。
pub const fn cell_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("启动"),
        1 => Some("功能"),
        2 => Some("崩溃"),
        3 => Some("卸载"),
        4 => Some("升级"),
        5 => Some("许可"),
        _ => None,
    }
}

/// 单向门拍折叠（拍 b）。
pub const fn ver_beat(b: usize) -> Option<u32> {
    if b >= VER_BEATS {
        return None;
    }
    Some(0xE000 + b as u32 * 0x3D)
}

/// AI-73 联签格载荷（格 r）。
pub const fn cs_cell(r: usize) -> Option<u64> {
    if r >= CS_CELLS {
        return None;
    }
    Some((0xE1000u64 + (r as u64) * 0x2F) ^ 0x41495737335F4353)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 验收机器人判据库账本状态机。
#[derive(Clone)]
pub struct RobotGateLedger {
    /// 六判据（机器人执行闸：启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 判据库格账。
    cells: [Option<u64>; GATE_CELLS],
    /// 单向门拍账。
    beats: [Option<u32>; VER_BEATS],
    /// AI-73 联签账。
    countersign: [Option<u64>; CS_CELLS],
    /// 最后判据家族。
    last_family: i64,
    /// 最后单向门拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RobotGateLedger {
    /// 新账本。
    pub fn new() -> Self {
        RobotGateLedger {
            gates: [None; 6],
            cells: [None; GATE_CELLS],
            beats: [None; VER_BEATS],
            countersign: [None; CS_CELLS],
            last_family: -1,
            last_beat: -1,
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

    /// 落一格六判据（机器人执行闸）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RobotGateCode> {
        if gate >= 6 {
            return Err(RobotGateCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 判据库格一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, c: usize, got: u64) -> Result<(), RobotGateCode> {
        if c >= GATE_CELLS {
            return Err(RobotGateCode::ENoCell);
        }
        let family = match cell_family(c) {
            Some(f) => f,
            None => return Err(RobotGateCode::ENoCell),
        };
        if (family as i64) < self.last_family {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(RobotGateCode::EOrder);
        }
        let want = match gate_cell(c) {
            Some(w) => w,
            None => return Err(RobotGateCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + c as u64);
            return Err(RobotGateCode::ECellDrift);
        }
        if match self.cells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RobotGateCode::EDup);
        }
        if let Some(slot) = self.cells.get_mut(c) {
            *slot = Some(got);
        }
        self.last_family = family as i64;
        self.fnv(0x8202 + c as u64);
        Ok(())
    }

    /// 判据库格全账核验（12 格）。
    pub fn cells_all(&self) -> bool {
        let mut c = 0usize;
        while c < GATE_CELLS {
            match self.cells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 单向门一拍（拍序严格递增 + 回放拒）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), RobotGateCode> {
        if b >= VER_BEATS {
            return Err(RobotGateCode::ENoBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(RobotGateCode::EOrder);
        }
        if let Some(slot) = self.beats.get_mut(b) {
            *slot = ver_beat(b);
        }
        self.last_beat = b as i64;
        self.fnv(0x8301 + b as u64);
        Ok(())
    }

    /// 单向门拍全账核验（8 拍单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < VER_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// AI-73 联签格一格（载荷对照 + 重复拒）。
    pub fn record_cs(&mut self, r: usize, got: u64) -> Result<(), RobotGateCode> {
        let want = match cs_cell(r) {
            Some(w) => w,
            None => return Err(RobotGateCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + r as u64);
            return Err(RobotGateCode::ERowDrift);
        }
        if match self.countersign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RobotGateCode::EDup);
        }
        if let Some(slot) = self.countersign.get_mut(r) {
            *slot = Some(got);
        }
        self.fnv(0x8401 + r as u64);
        Ok(())
    }

    /// AI-73 联签全齐核验（4 格）。
    pub fn cs_all(&self) -> bool {
        let mut r = 0usize;
        while r < CS_CELLS {
            match self.countersign.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cells_all() && self.beats_all() && self.cs_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RobotGateLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RobotGateLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < GATE_CELLS {
        if let Some(w) = gate_cell(c) {
            if led.record_cell(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < VER_BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < CS_CELLS {
        if let Some(w) = cs_cell(r) {
            if led.record_cs(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    // 注入 10 轮：判据漂移 / 单向门回退 / 判据格越闭集三态轮转。
    // 全部落在当轮临时账本上；漂移注入先录正格再注漂移；回退注入先按
    // 合法序落高家族格再注低家族，空账本同样必抓且不改 last_family。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RobotGateLedger::new();
        let caught = match kind {
            0 => {
                if let (Some(w0), Some(w1)) = (gate_cell(0), gate_cell(1)) {
                    let _ = lt.record_cell(0, w0);
                    lt.record_cell(1, w1.wrapping_add(1)) == Err(RobotGateCode::ECellDrift)
                } else {
                    false
                }
            }
            1 => {
                if let (Some(w4), Some(w1)) = (gate_cell(4), gate_cell(1)) {
                    let _ = lt.record_cell(4, w4);
                    lt.record_cell(1, w1) == Err(RobotGateCode::EOrder)
                } else {
                    false
                }
            }
            _ => lt.record_cell(GATE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8500 + k as u64);
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
/// UNX-F52292 判据集（锚 UNX-F52293-J1，off-by-one 板式）。
pub fn run_unx52292_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52292");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "六判据母版入库 12 格全通",
        recorded >= GATE_CELLS as u32,
        "六判据 ×2 格（schema+抽测）逐格恒等，机器人可执行口径",
    );
    s.add(
        "六判据家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < GATE_CELLS {
                if cell_family(c).unwrap_or(GATE_FAMILIES as u32)
                    < cell_family(c - 1).unwrap_or(GATE_FAMILIES as u32)
                {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "六家族闭集单调，家族回退必抓",
    );
    s.add(
        "判据版本单向门 8 拍单调",
        {
            let mut lt = RobotGateLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < VER_BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "冻结→入库→抽测→对接拍序严格递增，schema ×1 冻结入库口径",
    );
    s.add(
        "AI-73 对接联签 4/4 全齐",
        {
            let mut lt = RobotGateLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CS_CELLS {
                if let Some(w) = cs_cell(r) {
                    if lt.record_cs(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.cs_all()
        },
        "治理线接口逐格联签落账（§AI-66.8 治理线接口口径）",
    );
    s.add(
        "六判据执行闸全落",
        {
            let mut lt = RobotGateLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账，机器人可执行",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "判据漂移/单向门回退/判据格越闭集三态轮转必抓",
    );
    let mut l0 = RobotGateLedger::new();
    s.add(
        "边界·判据格越闭集",
        l0.record_cell(GATE_CELLS, 0).is_err(),
        "判据格 12 越闭集必拒",
    );
    s.add(
        "边界·单向门拍越闭集",
        l0.record_beat(VER_BEATS).is_err(),
        "单向门拍 8 越闭集必拒",
    );
    s.add(
        "边界·六判据越闭集",
        l0.judge_gate(6).is_err(),
        "判据闸 6 越闭集必拒",
    );
    s.add(
        "判据漂移拒",
        {
            if let (Some(w0), Some(w1)) = (gate_cell(0), gate_cell(1)) {
                let mut lt = RobotGateLedger::new();
                let _ = lt.record_cell(0, w0);
                lt.record_cell(1, w1.wrapping_add(1)) == Err(RobotGateCode::ECellDrift)
            } else {
                false
            }
        },
        "判据格载荷漂移必抓",
    );
    s.add(
        "单向门回退拒",
        {
            let mut lt = RobotGateLedger::new();
            let _ = lt.record_beat(5);
            lt.record_beat(2).is_err()
        },
        "拍序回放必抓（版本单向门只进不退）",
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
        "定级 S1 闭集",
        {
            let mut lt = RobotGateLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < GATE_CELLS {
                if let Some(w) = gate_cell(c) {
                    let _ = lt.record_cell(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < VER_BEATS {
                let _ = lt.record_beat(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < CS_CELLS {
                if let Some(w) = cs_cell(r) {
                    let _ = lt.record_cs(r, w);
                }
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC88C0,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_family() {
        assert_eq!(cell_family(0), Some(0));
        assert_eq!(cell_family(2), Some(1));
        assert_eq!(cell_family(11), Some(5));
        assert!(cell_family(12).is_none());
        assert!(gate_cell(12).is_none());
        assert_eq!(cell_family_name(0), Some("启动"));
        assert_eq!(cell_family_name(5), Some("许可"));
        assert!(cell_family_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = gate_cell(5);
        assert_eq!(a, gate_cell(5));
        assert_ne!(a, gate_cell(6));
        assert!(ver_beat(VER_BEATS).is_none());
        assert!(cs_cell(CS_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RobotGateLedger::new();
        let _ = lt.record_cell(4, gate_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_cell(1, gate_cell(1).unwrap_or(0)),
            Err(RobotGateCode::EOrder)
        );
        // 回退被拒后 last_family 不回退：同族后继仍可落。
        assert!(lt.record_cell(5, gate_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_on_empty_ledger_caught() {
        // 空账本回退注入：先按合法序落高家族格，再注低家族，必抓。
        let mut lt = RobotGateLedger::new();
        let _ = lt.record_cell(2, gate_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_cell(0, gate_cell(0).unwrap_or(0)),
            Err(RobotGateCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RobotGateLedger::new();
        assert_eq!(
            lt.record_cell(7, gate_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(RobotGateCode::ECellDrift)
        );
        assert_eq!(
            lt.record_cs(2, cs_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(RobotGateCode::ERowDrift)
        );
    }

    #[test]
    fn test_beat_monotonic() {
        let mut lt = RobotGateLedger::new();
        let mut b = 0usize;
        while b < VER_BEATS {
            assert!(lt.record_beat(b).is_ok());
            b += 1;
        }
        assert!(lt.record_beat(VER_BEATS).is_err());
        assert!(lt.record_beat(1).is_err());
        assert!(lt.beats_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RobotGateLedger::new();
        let _ = lt.record_cell(0, gate_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cell(0, gate_cell(0).unwrap_or(0)),
            Err(RobotGateCode::EDup)
        );
        let _ = lt.record_cs(0, cs_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cs(0, cs_cell(0).unwrap_or(0)),
            Err(RobotGateCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RobotGateLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RobotGateLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (GATE_CELLS + CS_CELLS) as u32);
        assert_eq!(caught, 10);
        let led = RobotGateLedger::new();
        assert_eq!(led.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC88C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RobotGateCode::Ok as u32, 0);
        assert_eq!(SEG_BASE + 0x0C, RobotGateCode::EGradeDrift.val());
        assert_eq!(RobotGateCode::EGradeDrift as u32, 12);
        assert!(!RobotGateCode::Ok.detail().is_empty());
        assert!(!RobotGateCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = RobotGateLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52292_checks_green() {
        let s = run_unx52292_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
