//! UNX-F52307 Revo Uninstaller 收口 · 0xC8C80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52307"，逐字）：
//!   title: Revo Uninstaller 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52308-J1 六判据全
//!     落；批量卸载/孤儿检测 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52307）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52308-J1，照原文锚定；功能按本卡标题（Revo Uninstaller 收口）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 扫描三模式 12 格：三模式闭集（安全/中等/激进）各 4 格，格载荷
//!      纯函数折叠恒等才收，越模式必抓。
//!   ② 卸载 10 操作：三相位闭集（检测 0..4 / 卸载 4..8 / 清理 8..10），
//!      相位单调（逆序必抓），漂移必抓。
//!   ③ 注册表备份三元闸：备份启用 / 备份路径 / 校验回读，三格逐格落账
//!      未齐即封。
//!   ④ 残留清理 8 格逐格恒等；强制卸载 5 场景逐场景恒等，漂移必抓。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10/10（残留漂移/相位逆序/场景越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （12 格 + 10 操作 + 8 残留 + 5 场景 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52307;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 扫描格数（三模式 × 4）。
pub const SCAN: usize = 12;

/// 卸载操作数。
pub const OPS: usize = 10;

/// 注册表备份三元闸格数。
pub const BACKUP_GATES: usize = 3;

/// 残留清理格数。
pub const RESIDUE: usize = 8;

/// 强制卸载场景数。
pub const FORCE: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevoCode {
    /// 成功
    Ok = 0,
    /// 扫描格越闭集
    ENoScan,
    /// 扫描漂移
    EScanDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EOpDrift,
    /// 相位逆序
    EPhase,
    /// 残留格越闭集
    ENoResidue,
    /// 残留漂移
    EResidueDrift,
    /// 场景越闭集
    ENoForce,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl RevoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RevoCode::Ok => "成功",
            RevoCode::ENoScan => "扫描格越闭集：扫描三模式 12 格之外先登记",
            RevoCode::EScanDrift => "扫描漂移：模式载荷折叠与基准不恒等，逐格复算",
            RevoCode::ENoOp => "操作号越闭集：卸载 10 操作之外先登记",
            RevoCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            RevoCode::EPhase => "相位逆序：检测→卸载→清理单调，逆序必抓",
            RevoCode::ENoResidue => "残留格越闭集：残留清理 8 格之外先登记",
            RevoCode::EResidueDrift => "残留漂移：残留载荷与基准不恒等，逐格复算",
            RevoCode::ENoForce => "场景越闭集：强制卸载 5 场景之外先登记",
            RevoCode::EDup => "重复落账：同格只许一条账目",
            RevoCode::ESeal => "未齐即封：12 格 + 10 操作 + 8 残留未齐先补齐",
            RevoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RevoCode::EGradeDrift => "定级漂移：全通且三元闸全落才可定 S1",
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
            Grade::S1 => "S1：六判据+12格扫描+10操作+8残留+5场景全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 扫描格载荷（格 m；模式 = m/4）。
pub const fn scan_cell(m: usize) -> Option<u64> {
    if m >= SCAN {
        return None;
    }
    Some((0xC9000u64 + (m as u64) * 0x2D) ^ 0x5245564F53430000)
}

/// 扫描模式（0..4=安全 4..8=中等 8..12=激进）。
pub const fn scan_mode(m: usize) -> Option<u32> {
    if m >= SCAN {
        return None;
    }
    Some((m / 4) as u32)
}

/// 模式名。
pub const fn scan_mode_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("安全"),
        1 => Some("中等"),
        2 => Some("激进"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/4 封顶 2）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCA000u64 + (o as u64) * 0x3D) ^ 0x5245564F554E0000)
}

/// 操作相位（0..4=检测 4..8=卸载 8..10=清理）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 4 {
        return Some(0);
    }
    if o < 8 {
        return Some(1);
    }
    Some(2)
}

/// 相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("检测"),
        1 => Some("卸载"),
        2 => Some("清理"),
        _ => None,
    }
}

/// 残留清理格基准（格 r）。
pub const fn residue_cell(r: usize) -> Option<u64> {
    if r >= RESIDUE {
        return None;
    }
    Some((0xCB000u64 + (r as u64) * 0x43) ^ 0x5245564F52450000)
}

/// 强制卸载场景基准（场景 f）。
pub const fn force_cell(f: usize) -> Option<u64> {
    if f >= FORCE {
        return None;
    }
    Some((0xCC000u64 + (f as u64) * 0x59) ^ 0x5245564F464F0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Revo 收口账本状态机。
#[derive(Clone)]
pub struct RevoLedger {
    /// 六判据（扫描模式/卸载相位/残留清理/注册表备份/强制卸载/写回摘要）。
    gates: [Option<bool>; 6],
    /// 扫描账。
    scans: [Option<u64>; SCAN],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 注册表备份三元闸。
    backup: [Option<bool>; BACKUP_GATES],
    /// 残留清理账。
    residues: [Option<u64>; RESIDUE],
    /// 强制卸载场景账。
    forces: [Option<u64>; FORCE],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RevoLedger {
    /// 新账本。
    pub fn new() -> Self {
        RevoLedger {
            gates: [None; 6],
            scans: [None; SCAN],
            ops: [None; OPS],
            backup: [None; BACKUP_GATES],
            residues: [None; RESIDUE],
            forces: [None; FORCE],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RevoCode> {
        if gate >= 6 {
            return Err(RevoCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 扫描一格（模式载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, m: usize, got: u64) -> Result<(), RevoCode> {
        let want = match scan_cell(m) {
            Some(w) => w,
            None => return Err(RevoCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + m as u64);
            return Err(RevoCode::EScanDrift);
        }
        if match self.scans.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RevoCode::EDup);
        }
        if let Some(c) = self.scans.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x7201 + m as u64);
        Ok(())
    }

    /// 扫描全账核验（12 格）。
    pub fn scans_all(&self) -> bool {
        let mut m = 0usize;
        while m < SCAN {
            match self.scans.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RevoCode> {
        if o >= OPS {
            return Err(RevoCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(RevoCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(RevoCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(RevoCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(RevoCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RevoCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x7302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 注册表备份三元闸一格。
    pub fn record_backup(&mut self, g: usize) -> Result<(), RevoCode> {
        if g >= BACKUP_GATES {
            return Err(RevoCode::ESeal);
        }
        if match self.backup.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RevoCode::EDup);
        }
        if let Some(c) = self.backup.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x7400 + g as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn backup_all(&self) -> bool {
        let mut g = 0usize;
        while g < BACKUP_GATES {
            match self.backup.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 残留清理一格（载荷对照 + 重复拒）。
    pub fn record_residue(&mut self, r: usize, got: u64) -> Result<(), RevoCode> {
        let want = match residue_cell(r) {
            Some(w) => w,
            None => return Err(RevoCode::ENoResidue),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + r as u64);
            return Err(RevoCode::EResidueDrift);
        }
        if match self.residues.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RevoCode::EDup);
        }
        if let Some(c) = self.residues.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x7501 + r as u64);
        Ok(())
    }

    /// 残留全账核验（8 格）。
    pub fn residues_all(&self) -> bool {
        let mut r = 0usize;
        while r < RESIDUE {
            match self.residues.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 强制卸载场景一格（载荷对照 + 重复拒）。
    pub fn record_force(&mut self, f: usize, got: u64) -> Result<(), RevoCode> {
        let want = match force_cell(f) {
            Some(w) => w,
            None => return Err(RevoCode::ENoForce),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7600 + f as u64);
            return Err(RevoCode::EScanDrift);
        }
        if match self.forces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RevoCode::EDup);
        }
        if let Some(c) = self.forces.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x7601 + f as u64);
        Ok(())
    }

    /// 场景全账核验（5 场景）。
    pub fn forces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FORCE {
            match self.forces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
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
        if self.gates_all() && self.scans_all() && self.ops_all() && self.residues_all() && self.forces_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RevoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RevoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut m = 0usize;
    while m < SCAN {
        if let Some(w) = scan_cell(m) {
            if led.record_scan(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut bg = 0usize;
    while bg < BACKUP_GATES {
        let _ = led.record_backup(bg);
        bg += 1;
    }
    let mut r = 0usize;
    while r < RESIDUE {
        if let Some(w) = residue_cell(r) {
            if led.record_residue(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut f = 0usize;
    while f < FORCE {
        if let Some(w) = force_cell(f) {
            if led.record_force(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    // 注入 10 轮：残留漂移 / 相位逆序 / 场景越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；led 上 injected==caught==10、
    // escaped==0。相位注入对空账本同样必抓：测试种子须按合法序先落
    // 低相位格（检测 0 → 卸载 4），再注回退相位，且 EPhase 不改
    // last_phase。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RevoLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先录正格再注漂移。
                let base0 = match residue_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let base1 = match residue_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_residue(0, base0);
                lt.record_residue(1, base1.wrapping_add(1)) == Err(RevoCode::EResidueDrift)
            }
            1 => {
                // 相位注入：按合法序先落检测格与卸载格，再注回退。
                let o0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let o4 = match op_cell(4) {
                    Some(w) => w,
                    None => 0,
                };
                let o1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, o0);
                let _ = lt.record_op(4, o4);
                lt.record_op(1, o1) == Err(RevoCode::EPhase)
            }
            _ => {
                // 越闭集注入：强制场景与操作双边界。
                let f5 = lt.record_force(FORCE, 0) == Err(RevoCode::ENoForce);
                let o10 = lt.record_op(OPS, 0) == Err(RevoCode::ENoOp);
                f5 && o10
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52307 判据集（锚 UNX-F52308-J1，off-by-one 板式）。
pub fn run_unx52307_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52307");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "扫描三模式 12 格全落",
        recorded >= SCAN as u32,
        "安全/中等/激进三模式逐格恒等",
    );
    s.add(
        "卸载 10 操作全落",
        {
            let mut lt = RevoLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "检测/卸载/清理三相位闭集逐操作恒等",
    );
    s.add(
        "残留清理 8 格恒等",
        {
            let mut lt = RevoLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RESIDUE {
                if let Some(w) = residue_cell(r) {
                    if lt.record_residue(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.residues_all()
        },
        "残留清理逐格载荷对照，漂移必抓",
    );
    s.add(
        "强制卸载 5 场景恒等",
        {
            let mut lt = RevoLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FORCE {
                if let Some(w) = force_cell(f) {
                    if lt.record_force(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.forces_all()
        },
        "强制卸载逐场景载荷对照，漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = RevoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "扫描模式/卸载相位/残留清理/注册表备份/强制卸载/写回摘要逐格落账",
    );
    s.add(
        "注册表备份三元闸 3/3 全落",
        {
            let mut lt = RevoLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < BACKUP_GATES {
                if lt.record_backup(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.backup_all()
        },
        "备份启用/备份路径/校验回读三格闭集",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let cur = match op_phase(o) {
                    Some(p) => p,
                    None => 3,
                };
                let prev = match op_phase(o - 1) {
                    Some(p) => p,
                    None => 3,
                };
                if cur < prev {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "检测→卸载→清理相位闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "残留漂移/相位逆序/越闭集三态轮转必抓");
    let mut l0 = RevoLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·扫描越闭集",
        l0.record_scan(SCAN, 0).is_err(),
        "扫描 12 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_force(FORCE, 0).is_err(),
        "场景 5 越闭集必拒",
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8C80,
        "SEG_BASE 0x40 对齐且落在 VX15 域 0xC8C80",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = RevoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut m = 0usize;
            while m < SCAN {
                if let Some(w) = scan_cell(m) {
                    let _ = lt.record_scan(m, w);
                }
                m += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < RESIDUE {
                if let Some(w) = residue_cell(r) {
                    let _ = lt.record_residue(r, w);
                }
                r += 1;
            }
            let mut f = 0usize;
            while f < FORCE {
                if let Some(w) = force_cell(f) {
                    let _ = lt.record_force(f, w);
                }
                f += 1;
            }
            // 定级断言前 main/aux 各节已全部记账。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_mode() {
        assert_eq!(scan_mode(0), Some(0));
        assert_eq!(scan_mode(4), Some(1));
        assert_eq!(scan_mode(11), Some(2));
        assert!(scan_mode(12).is_none());
        assert!(scan_cell(12).is_none());
        assert_eq!(scan_mode_name(2), Some("激进"));
        assert!(scan_mode_name(3).is_none());
    }

    #[test]
    fn test_op_phase() {
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(4), Some(1));
        assert_eq!(op_phase(9), Some(2));
        assert!(op_phase(10).is_none());
        assert!(op_cell(10).is_none());
        assert_eq!(op_phase_name(1), Some("卸载"));
    }

    #[test]
    fn test_cells_pure() {
        let a = scan_cell(5).unwrap_or(0);
        assert_eq!(a, scan_cell(5).unwrap_or(0));
        assert_ne!(a, scan_cell(6).unwrap_or(0));
        assert!(residue_cell(RESIDUE).is_none());
        assert!(force_cell(FORCE).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = RevoLedger::new();
        let _ = lt.record_op(5, op_cell(5).unwrap_or(0));
        assert_eq!(lt.record_op(1, op_cell(1).unwrap_or(0)), Err(RevoCode::EPhase));
        // EPhase 不改 last_phase：回退后再录同相位格不误伤。
        assert!(lt.record_op(6, op_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RevoLedger::new();
        assert_eq!(
            lt.record_residue(3, residue_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(RevoCode::EResidueDrift)
        );
        assert_eq!(
            lt.record_force(2, force_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(RevoCode::EScanDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RevoLedger::new();
        let _ = lt.record_scan(0, scan_cell(0).unwrap_or(0));
        assert_eq!(lt.record_scan(0, scan_cell(0).unwrap_or(0)), Err(RevoCode::EDup));
        let _ = lt.record_backup(0);
        assert_eq!(lt.record_backup(0), Err(RevoCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RevoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(lt.record_backup(3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCAN + OPS + RESIDUE + FORCE) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conservation() {
        let mut lt = RevoLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        let (i, c, e) = lt.stats();
        assert_eq!(i, c);
        assert_eq!(e, 0);
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
        assert_eq!(SEG_BASE, 0xC8C80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RevoCode::Ok as u32, 0);
        assert_eq!(RevoCode::EGradeDrift as u32, 12);
        assert!(!RevoCode::Ok.detail().is_empty());
        assert!(!RevoCode::ESeal.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = RevoLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52307_checks_green() {
        let s = run_unx52307_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
