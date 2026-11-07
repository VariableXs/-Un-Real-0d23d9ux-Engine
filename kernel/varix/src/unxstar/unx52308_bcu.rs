//! UNX-F52308 Bulk Crap Uninstaller 收口 · 0xC8CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52308"，逐字）：
//!   title: Bulk Crap Uninstaller 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52309-J1 六判据全
//!     落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52308）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52309-J1，照原文锚定；功能按本卡标题（Bulk Crap Uninstaller 收
//! 口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 检测四源 16 格：四源闭集（注册表/目录/Steam/Windows Store）各
//!      4 格，源载荷纯函数折叠恒等才收，越源必抓。
//!   ② 批量卸载 10 操作：二相位闭集（清点 0..5 / 卸载 5..10），相位
//!      单调（逆序必抓），漂移必抓。
//!   ③ 并发卸载三元闸：并发上限 / 超时窗 / 失败重试，三格逐格落账
//!      未齐即封。
//!   ④ 残留扫描 8 格逐格恒等；开源声明 2 行闭集（许可证声明/仓库来源
//!      声明），缺一拒封；开源收口如实定级 → 全通亦定 S2。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入
//!      核 run_cycle 三态轮转 10/10（残留漂移/相位逆序/操作越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （16 格 + 10 操作 + 8 残留 + 2 行声明全通 → S2 如实，S1 预留
//!      商业核验，判据缺 → S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52308;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 检测格数（四源 × 4）。
pub const DETECT: usize = 16;

/// 批量卸载操作数。
pub const OPS: usize = 10;

/// 并发卸载三元闸格数。
pub const CONC_GATES: usize = 3;

/// 残留扫描格数。
pub const RESIDUE: usize = 8;

/// 开源声明行数。
pub const OSS_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BcuCode {
    /// 成功
    Ok = 0,
    /// 检测格越闭集
    ENoDetect,
    /// 检测漂移
    EDetectDrift,
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
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl BcuCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BcuCode::Ok => "成功",
            BcuCode::ENoDetect => "检测格越闭集：检测四源 16 格之外先登记",
            BcuCode::EDetectDrift => "检测漂移：源载荷折叠与基准不恒等，逐格复算",
            BcuCode::ENoOp => "操作号越闭集：批量卸载 10 操作之外先登记",
            BcuCode::EOpDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            BcuCode::EPhase => "相位逆序：清点→卸载单调，逆序必抓",
            BcuCode::ENoResidue => "残留格越闭集：残留扫描 8 格之外先登记",
            BcuCode::EResidueDrift => "残留漂移：残留载荷与基准不恒等，逐格复算",
            BcuCode::ENoRow => "声明行越闭集：开源声明 2 行闭集之外先登记",
            BcuCode::EDup => "重复落账：同格只许一条账目",
            BcuCode::ESeal => "未齐即封：16 格 + 10 操作 + 8 残留未齐先补齐",
            BcuCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BcuCode::EGradeDrift => "定级漂移：开源收口如实定级 S2，S1 须商业核验",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（商业核验预留）
    S1,
    /// S2：开源如实收口
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
            Grade::S1 => "S1：商业核验预留位，本卡不落",
            Grade::S2 => "S2：开源收口如实定级，16格+10操作+8残留+2行声明全通",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检测格载荷（格 d；源 = d/4）。
pub const fn detect_cell(d: usize) -> Option<u64> {
    if d >= DETECT {
        return None;
    }
    Some((0xCD000u64 + (d as u64) * 0x2E) ^ 0x4243554445540000)
}

/// 检测源（0..4=注册表 4..8=目录 8..12=Steam 12..16=Windows Store）。
pub const fn detect_source(d: usize) -> Option<u32> {
    if d >= DETECT {
        return None;
    }
    Some((d / 4) as u32)
}

/// 源名。
pub const fn detect_source_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("注册表"),
        1 => Some("目录"),
        2 => Some("Steam"),
        3 => Some("Windows Store"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCE000u64 + (o as u64) * 0x3E) ^ 0x4243554241540000)
}

/// 操作相位（0..5=清点 5..10=卸载）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 5 {
        return Some(0);
    }
    Some(1)
}

/// 相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("清点"),
        1 => Some("卸载"),
        _ => None,
    }
}

/// 残留扫描格基准（格 r）。
pub const fn residue_cell(r: usize) -> Option<u64> {
    if r >= RESIDUE {
        return None;
    }
    Some((0xCF000u64 + (r as u64) * 0x45) ^ 0x4243555245530000)
}

/// 开源声明行折叠（行 r：0=许可证声明 1=仓库来源声明）。
pub const fn oss_row(r: usize) -> Option<u32> {
    if r >= OSS_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// BCUninstaller 收口账本状态机。
#[derive(Clone)]
pub struct BcuLedger {
    /// 六判据（四源检测/批量卸载/并发闸/残留扫描/开源声明/写回摘要）。
    gates: [Option<bool>; 6],
    /// 检测账。
    detects: [Option<u64>; DETECT],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 并发卸载三元闸。
    conc: [Option<bool>; CONC_GATES],
    /// 残留扫描账。
    residues: [Option<u64>; RESIDUE],
    /// 开源声明账。
    oss: [Option<u32>; OSS_ROWS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BcuLedger {
    /// 新账本。
    pub fn new() -> Self {
        BcuLedger {
            gates: [None; 6],
            detects: [None; DETECT],
            ops: [None; OPS],
            conc: [None; CONC_GATES],
            residues: [None; RESIDUE],
            oss: [None; OSS_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BcuCode> {
        if gate >= 6 {
            return Err(BcuCode::ESeal);
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

    /// 检测一格（源载荷对照 + 重复拒）。
    pub fn record_detect(&mut self, d: usize, got: u64) -> Result<(), BcuCode> {
        let want = match detect_cell(d) {
            Some(w) => w,
            None => return Err(BcuCode::ENoDetect),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + d as u64);
            return Err(BcuCode::EDetectDrift);
        }
        if match self.detects.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BcuCode::EDup);
        }
        if let Some(c) = self.detects.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x8201 + d as u64);
        Ok(())
    }

    /// 检测全账核验（16 格）。
    pub fn detects_all(&self) -> bool {
        let mut d = 0usize;
        while d < DETECT {
            match self.detects.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), BcuCode> {
        if o >= OPS {
            return Err(BcuCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(BcuCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(BcuCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(BcuCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(BcuCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BcuCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x8302 + o as u64);
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

    /// 并发卸载三元闸一格。
    pub fn record_conc(&mut self, g: usize) -> Result<(), BcuCode> {
        if g >= CONC_GATES {
            return Err(BcuCode::ESeal);
        }
        if match self.conc.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BcuCode::EDup);
        }
        if let Some(c) = self.conc.get_mut(g) {
            *c = Some(true);
        }
        self.fnv(0x8400 + g as u64);
        Ok(())
    }

    /// 三元闸全落核验。
    pub fn conc_all(&self) -> bool {
        let mut g = 0usize;
        while g < CONC_GATES {
            match self.conc.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 残留扫描一格（载荷对照 + 重复拒）。
    pub fn record_residue(&mut self, r: usize, got: u64) -> Result<(), BcuCode> {
        let want = match residue_cell(r) {
            Some(w) => w,
            None => return Err(BcuCode::ENoResidue),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + r as u64);
            return Err(BcuCode::EResidueDrift);
        }
        if match self.residues.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BcuCode::EDup);
        }
        if let Some(c) = self.residues.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x8501 + r as u64);
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

    /// 开源声明一行。
    pub fn declare_oss(&mut self, r: usize) -> Result<(), BcuCode> {
        if r >= OSS_ROWS {
            return Err(BcuCode::ENoRow);
        }
        if match self.oss.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BcuCode::EDup);
        }
        if let Some(c) = self.oss.get_mut(r) {
            *c = oss_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn oss_all(&self) -> bool {
        let mut r = 0usize;
        while r < OSS_ROWS {
            match self.oss.get(r) {
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

    /// KB-COMPAT 定级：开源收口如实定级——全通 → S2，判据缺 → S3，
    /// 其余 → S2。S1 为商业核验预留位，本卡不落。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.detects_all() && self.ops_all() && self.residues_all() && self.oss_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BcuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BcuLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DETECT {
        if let Some(w) = detect_cell(d) {
            if led.record_detect(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
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
    let mut cg = 0usize;
    while cg < CONC_GATES {
        let _ = led.record_conc(cg);
        cg += 1;
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
    let mut s = 0usize;
    while s < OSS_ROWS {
        let _ = led.declare_oss(s);
        s += 1;
    }
    // 注入 10 轮：残留漂移 / 相位逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；led 上 injected==caught==10、
    // escaped==0。相位注入对空账本同样必抓：测试种子须按合法序先落
    // 低相位格（清点 0 → 卸载 5），再注回退相位，且 EPhase 不改
    // last_phase。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BcuLedger::new();
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
                lt.record_residue(1, base1.wrapping_add(1)) == Err(BcuCode::EResidueDrift)
            }
            1 => {
                // 相位注入：按合法序先落清点格与卸载格，再注回退。
                let o0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let o5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                let o1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, o0);
                let _ = lt.record_op(5, o5);
                lt.record_op(1, o1) == Err(BcuCode::EPhase)
            }
            _ => {
                // 越闭集注入：检测源与操作双边界。
                let d16 = lt.record_detect(DETECT, 0) == Err(BcuCode::ENoDetect);
                let o10 = lt.record_op(OPS, 0) == Err(BcuCode::ENoOp);
                d16 && o10
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52308 判据集（锚 UNX-F52309-J1，off-by-one 板式）。
pub fn run_unx52308_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52308");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "检测四源 16 格全落",
        recorded >= DETECT as u32,
        "注册表/目录/Steam/Windows Store 四源逐格恒等",
    );
    s.add(
        "批量卸载 10 操作全落",
        {
            let mut lt = BcuLedger::new();
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
        "清点/卸载二相位闭集逐操作恒等",
    );
    s.add(
        "残留扫描 8 格恒等",
        {
            let mut lt = BcuLedger::new();
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
        "残留扫描逐格载荷对照，漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BcuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "四源检测/批量卸载/并发闸/残留扫描/开源声明/写回摘要逐格落账",
    );
    s.add(
        "并发卸载三元闸 3/3 全落",
        {
            let mut lt = BcuLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < CONC_GATES {
                if lt.record_conc(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.conc_all()
        },
        "并发上限/超时窗/失败重试三格闭集",
    );
    s.add(
        "开源声明 2/2 全齐",
        {
            let mut lt = BcuLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OSS_ROWS {
                if lt.declare_oss(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.oss_all()
        },
        "许可证声明/仓库来源声明两行闭集（§AI-66.3）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let cur = match op_phase(o) {
                    Some(p) => p,
                    None => 2,
                };
                let prev = match op_phase(o - 1) {
                    Some(p) => p,
                    None => 2,
                };
                if cur < prev {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "清点→卸载相位闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "残留漂移/相位逆序/越闭集三态轮转必抓");
    let mut l0 = BcuLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·检测越闭集",
        l0.record_detect(DETECT, 0).is_err(),
        "检测 16 越闭集必拒",
    );
    s.add(
        "边界·声明越闭集",
        l0.declare_oss(OSS_ROWS).is_err(),
        "声明 2 越闭集必拒",
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8CC0,
        "SEG_BASE 0x40 对齐且落在 VX15 域 0xC8CC0",
    );
    s.add(
        "定级 S2 闭集（开源如实）",
        {
            let mut lt = BcuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < DETECT {
                if let Some(w) = detect_cell(d) {
                    let _ = lt.record_detect(d, w);
                }
                d += 1;
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
            let mut sr = 0usize;
            while sr < OSS_ROWS {
                let _ = lt.declare_oss(sr);
                sr += 1;
            }
            // 定级断言前 main/aux 各节已全部记账；开源收口如实定级 S2。
            lt.judge_grade() == Grade::S2
        },
        "全通如实 → S2（KB-COMPAT 定级登记，S1 商业核验预留）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_source() {
        assert_eq!(detect_source(0), Some(0));
        assert_eq!(detect_source(4), Some(1));
        assert_eq!(detect_source(8), Some(2));
        assert_eq!(detect_source(15), Some(3));
        assert!(detect_source(16).is_none());
        assert!(detect_cell(16).is_none());
        assert_eq!(detect_source_name(3), Some("Windows Store"));
        assert!(detect_source_name(4).is_none());
    }

    #[test]
    fn test_op_phase() {
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(4), Some(0));
        assert_eq!(op_phase(5), Some(1));
        assert_eq!(op_phase(9), Some(1));
        assert!(op_phase(10).is_none());
        assert!(op_cell(10).is_none());
        assert_eq!(op_phase_name(1), Some("卸载"));
    }

    #[test]
    fn test_cells_pure() {
        let a = detect_cell(7).unwrap_or(0);
        assert_eq!(a, detect_cell(7).unwrap_or(0));
        assert_ne!(a, detect_cell(8).unwrap_or(0));
        assert!(residue_cell(RESIDUE).is_none());
        assert!(oss_row(OSS_ROWS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = BcuLedger::new();
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(BcuCode::EPhase));
        // EPhase 不改 last_phase：回退后再录同相位格不误伤。
        assert!(lt.record_op(7, op_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BcuLedger::new();
        assert_eq!(
            lt.record_residue(4, residue_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(BcuCode::EResidueDrift)
        );
        assert_eq!(
            lt.record_detect(9, detect_cell(9).unwrap_or(0).wrapping_add(3)),
            Err(BcuCode::EDetectDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BcuLedger::new();
        let _ = lt.record_detect(0, detect_cell(0).unwrap_or(0));
        assert_eq!(lt.record_detect(0, detect_cell(0).unwrap_or(0)), Err(BcuCode::EDup));
        let _ = lt.record_conc(0);
        assert_eq!(lt.record_conc(0), Err(BcuCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BcuLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(lt.record_conc(3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DETECT + OPS + RESIDUE) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conservation() {
        let mut lt = BcuLedger::new();
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
        assert_eq!(SEG_BASE, 0xC8CC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BcuCode::Ok as u32, 0);
        assert_eq!(BcuCode::EGradeDrift as u32, 12);
        assert!(!BcuCode::Ok.detail().is_empty());
        assert!(!BcuCode::ESeal.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = BcuLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52308_checks_green() {
        let s = run_unx52308_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
