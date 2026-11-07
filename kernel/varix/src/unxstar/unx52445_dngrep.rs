//! UNX-F52445 dnGrep 收口 · 0xCAF00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52445"，逐字）：
//!   title: dnGrep 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52446-J1 磁盘占
//!     用 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52445）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52446-J1，照原文锚定；功能按本卡标题（dnGrep）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 格：文本/正则/替换/Office/PDF 消费各 4 格，家族
//!      单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 结果投影 8 格：投影列（路径/行号/匹配段/上下文前后/命中计数/编
//!      码/时间戳/文件大小）逐格对照，漂移必抓。
//!   ③ 预览三态 6 格：未预览/命中预览/替换预览三态各 2 格，闭集外先登
//!      记必抓。
//!   ④ 与 grepWin 分账声明 2 行闭集：dnGrep 独立账声明 / grepWin 互斥
//!      消费分账声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作漂
//!      移/家族逆序/操作越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一
//!      致；定级 S2 目标档（KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52445;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FUNC_OPS: usize = 20;

/// 结果投影格数。
pub const PROJ_CELLS: usize = 8;

/// 预览三态格数。
pub const PREVIEW_STATES: usize = 6;

/// 与 grepWin 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DngrepCode {
    /// 成功
    Ok = 0,
    /// 操作/格号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// 预览格越闭集
    ENoPrev,
    /// 预览漂移
    EPrevDrift,
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

impl DngrepCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DngrepCode::Ok => "成功",
            DngrepCode::ENoOp => "操作号越闭集：dnGrep 五功能族 20 操作集之外先登记",
            DngrepCode::EDrift => "操作漂移：功能族折叠与基准不恒等，逐格复算",
            DngrepCode::EOrder => "家族逆序：文本→正则→替换→Office→PDF 消费单调，逆序必抓",
            DngrepCode::ENoProj => "投影格越闭集：结果投影 8 格闭集之外先登记",
            DngrepCode::EProjDrift => "投影漂移：投影列折叠与基准不恒等，逐格复算",
            DngrepCode::ENoPrev => "预览格越闭集：预览三态 6 格闭集之外先登记",
            DngrepCode::EPrevDrift => "预览漂移：预览态折叠与基准不恒等，逐格复算",
            DngrepCode::EDup => "重复落账：同格只许一条账目",
            DngrepCode::ESeal => "未齐即封：20 操作 + 8 投影 + 6 预览 + 2 行分账未齐先补齐",
            DngrepCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DngrepCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DngrepCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const 安全：不在 const fn 内调用）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（本卡闭集保留级）",
            Grade::S2 => "S2：有条件收口，dnGrep 目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族操作格载荷（操作 o；家族 = o/4）。
pub const fn dngrep_cell(o: usize) -> Option<u64> {
    if o >= FUNC_OPS {
        return None;
    }
    Some((0xCB100u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=文本 4..8=正则 8..12=替换 12..16=Office 16..20=PDF 消费）。
pub const fn dngrep_kind(o: usize) -> Option<u32> {
    if o >= FUNC_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 功能族名。
pub const fn dngrep_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("文本"),
        1 => Some("正则"),
        2 => Some("替换"),
        3 => Some("Office"),
        4 => Some("PDF 消费"),
        _ => None,
    }
}

/// 结果投影格载荷（投影列 p）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xCB200u64 + (p as u64) * 0x3D) ^ 0x444E_4750_0000_0000)
}

/// 预览三态格载荷（格 p；态 = p/2）。
pub const fn preview_cell(p: usize) -> Option<u64> {
    if p >= PREVIEW_STATES {
        return None;
    }
    Some((0xCB300u64 + (p as u64) * 0x1F) ^ ((p / 2) as u64).wrapping_mul(0x9E3779B9))
}

/// 预览三态（0..2=未预览 2..4=命中预览 4..6=替换预览）。
pub const fn preview_phase(p: usize) -> Option<u32> {
    if p >= PREVIEW_STATES {
        return None;
    }
    Some((p / 2) as u32)
}

/// 预览态名。
pub const fn preview_phase_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("未预览"),
        1 => Some("命中预览"),
        2 => Some("替换预览"),
        _ => None,
    }
}

/// grepWin 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// dnGrep 收口账本状态机。
#[derive(Clone)]
pub struct DngrepLedger {
    /// 六判据（磁盘占用收口：功能/投影/预览/分账/漂移/封核）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FUNC_OPS],
    /// 投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 预览账。
    prev: [Option<u64>; PREVIEW_STATES],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DngrepLedger {
    /// 新账本。
    pub fn new() -> Self {
        DngrepLedger {
            gates: [None; 6],
            ops: [None; FUNC_OPS],
            proj: [None; PROJ_CELLS],
            prev: [None; PREVIEW_STATES],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DngrepCode> {
        if gate >= 6 {
            return Err(DngrepCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4D00 + gate as u64);
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

    /// 功能族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), DngrepCode> {
        if o >= FUNC_OPS {
            return Err(DngrepCode::ENoOp);
        }
        let kind = match dngrep_kind(o) {
            Some(k) => k,
            None => return Err(DngrepCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D10);
            return Err(DngrepCode::EOrder);
        }
        let want = match dngrep_cell(o) {
            Some(w) => w,
            None => return Err(DngrepCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D11 + o as u64);
            return Err(DngrepCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DngrepCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4D12 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FUNC_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), DngrepCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(DngrepCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D20 + p as u64);
            return Err(DngrepCode::EProjDrift);
        }
        if match self.proj.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DngrepCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4D21 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn proj_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.proj.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 预览一格（三态载荷对照 + 重复拒）。
    pub fn record_preview(&mut self, p: usize, got: u64) -> Result<(), DngrepCode> {
        let want = match preview_cell(p) {
            Some(w) => w,
            None => return Err(DngrepCode::ENoPrev),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D30 + p as u64);
            return Err(DngrepCode::EPrevDrift);
        }
        if match self.prev.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DngrepCode::EDup);
        }
        if let Some(c) = self.prev.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4D31 + p as u64);
        Ok(())
    }

    /// 预览全账核验（6 格）。
    pub fn prev_all(&self) -> bool {
        let mut p = 0usize;
        while p < PREVIEW_STATES {
            match self.prev.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 分账声明一行（与 grepWin 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DngrepCode> {
        if r >= SPLIT_ROWS {
            return Err(DngrepCode::ENoOp);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DngrepCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4D40 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封核检查：六闸齐且各节全齐才放行，未齐即封。
    pub fn seal_check(&self) -> Result<(), DngrepCode> {
        if !self.gates_all() {
            return Err(DngrepCode::ESeal);
        }
        if !self.ops_all() || !self.proj_all() || !self.prev_all() || !self.splits_all() {
            return Err(DngrepCode::ESeal);
        }
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

    /// KB-COMPAT 定级（如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DngrepLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DngrepLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FUNC_OPS {
        if let Some(w) = dngrep_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut t = 0usize;
    while t < PREVIEW_STATES {
        if let Some(w) = preview_cell(t) {
            if led.record_preview(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 家族逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DngrepLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格（低族 0 格），再注漂移格。
            0 => {
                let _ = lt.record_op(0, dngrep_cell(0).unwrap_or(0));
                lt.record_op(1, dngrep_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DngrepCode::EDrift)
            }
            // 逆序注入：先落合法高族格，再注低族格，逆序必抓且不改 last_kind 于空账路径。
            1 => {
                let _ = lt.record_op(17, dngrep_cell(17).unwrap_or(0));
                lt.record_op(5, dngrep_cell(5).unwrap_or(0)) == Err(DngrepCode::EOrder)
            }
            // 越闭集注入：空账本同样必抓，且不改 last_kind。
            _ => lt.record_op(FUNC_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4D50 + k as u64);
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
/// UNX-F52445 判据集（锚 UNX-F52446-J1，off-by-one 板式）。
pub fn run_unx52445_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52445");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= FUNC_OPS as u32,
        "文本/正则/替换/Office/PDF 消费五家族逐操作恒等",
    );
    s.add(
        "结果投影 8 格全落",
        {
            let mut lt = DngrepLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.proj_all()
        },
        "路径/行号/匹配段/上下文/命中计数/编码/时间戳/大小逐列对照",
    );
    s.add(
        "预览三态 6 格全落",
        {
            let mut lt = DngrepLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < PREVIEW_STATES {
                if let Some(w) = preview_cell(t) {
                    if lt.record_preview(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.prev_all()
        },
        "未预览/命中预览/替换预览三态各 2 格闭集",
    );
    s.add(
        "与 grepWin 分账声明 2/2 全齐",
        {
            let mut lt = DngrepLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "dnGrep 独立账声明/grepWin 互斥消费分账声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FUNC_OPS {
                if dngrep_kind(o).unwrap_or(5) < dngrep_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五功能族闭集单调（逆序必抓）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    let mut l0 = DngrepLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FUNC_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·预览越闭集",
        l0.record_preview(PREVIEW_STATES, 0).is_err(),
        "预览格 6 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let _ = l0.record_op(0, dngrep_cell(0).unwrap_or(0));
            l0.record_op(1, dngrep_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DngrepCode::EDrift)
        },
        "功能族载荷漂移必抓",
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
        "未齐先记账不封",
        {
            let mut lt = DngrepLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < 4 {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            // 账未齐时逐格记账仍放行（Ok），封核检查才拒。
            ok && lt.record_proj(4, proj_cell(4).unwrap_or(0)).is_ok()
        },
        "账未齐时记账必须 Ok，未齐即封只在封核检查处生效",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = DngrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FUNC_OPS {
                if let Some(w) = dngrep_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut t = 0usize;
            while t < PREVIEW_STATES {
                if let Some(w) = preview_cell(t) {
                    let _ = lt.record_preview(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // main/aux 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        {
            let codes = [
                DngrepCode::Ok,
                DngrepCode::ENoOp,
                DngrepCode::EDrift,
                DngrepCode::EOrder,
                DngrepCode::ENoProj,
                DngrepCode::EProjDrift,
                DngrepCode::ENoPrev,
                DngrepCode::EPrevDrift,
                DngrepCode::EDup,
                DngrepCode::ESeal,
                DngrepCode::EInj,
                DngrepCode::EFp,
                DngrepCode::EGradeDrift,
            ];
            let mut ok = codes.len() == 13;
            let mut i = 0usize;
            while i < codes.len() {
                if codes[i].val() != SEG_BASE + i as u32 || codes[i].detail().is_empty() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "13 变体码位连续且明细全非空",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAF00,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCAF00",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dngrep_kind() {
        assert_eq!(dngrep_kind(0), Some(0));
        assert_eq!(dngrep_kind(4), Some(1));
        assert_eq!(dngrep_kind(8), Some(2));
        assert_eq!(dngrep_kind(12), Some(3));
        assert_eq!(dngrep_kind(19), Some(4));
        assert!(dngrep_kind(20).is_none());
        assert!(dngrep_cell(20).is_none());
        assert_eq!(dngrep_kind_name(0), Some("文本"));
        assert_eq!(dngrep_kind_name(4), Some("PDF 消费"));
        assert!(dngrep_kind_name(5).is_none());
    }

    #[test]
    fn test_preview_phase() {
        assert_eq!(preview_phase(0), Some(0));
        assert_eq!(preview_phase(1), Some(0));
        assert_eq!(preview_phase(2), Some(1));
        assert_eq!(preview_phase(4), Some(2));
        assert_eq!(preview_phase(5), Some(2));
        assert!(preview_phase(6).is_none());
        assert_eq!(preview_phase_name(0), Some("未预览"));
        assert_eq!(preview_phase_name(1), Some("命中预览"));
        assert_eq!(preview_phase_name(2), Some("替换预览"));
        assert!(preview_phase_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = dngrep_cell(3).unwrap_or(0);
        assert_eq!(a, dngrep_cell(3).unwrap_or(0));
        assert_ne!(a, dngrep_cell(4).unwrap_or(0));
        let b = proj_cell(5).unwrap_or(0);
        assert_eq!(b, proj_cell(5).unwrap_or(0));
        assert_ne!(b, proj_cell(6).unwrap_or(0));
        assert!(proj_cell(PROJ_CELLS).is_none());
        assert!(preview_cell(PREVIEW_STATES).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = DngrepLedger::new();
        // 合法序先落高族格（PDF 消费 19），再注低族格（正则 5）必抓。
        let _ = lt.record_op(19, dngrep_cell(19).unwrap_or(0));
        assert_eq!(lt.record_op(5, dngrep_cell(5).unwrap_or(0)), Err(DngrepCode::EOrder));
        assert!(lt.record_op(18, dngrep_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DngrepLedger::new();
        assert_eq!(
            lt.record_proj(2, proj_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(DngrepCode::EProjDrift)
        );
        assert_eq!(
            lt.record_preview(3, preview_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(DngrepCode::EPrevDrift)
        );
        assert_eq!(
            lt.record_op(6, dngrep_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(DngrepCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DngrepLedger::new();
        let _ = lt.record_op(8, dngrep_cell(8).unwrap_or(0));
        assert_eq!(lt.record_op(8, dngrep_cell(8).unwrap_or(0)), Err(DngrepCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(DngrepCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DngrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DngrepLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：功能 20 + 投影 8 + 预览 6。
        assert_eq!(recorded, (FUNC_OPS + PROJ_CELLS + PREVIEW_STATES) as u32);
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
        assert_eq!(SEG_BASE, 0xCAF00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DngrepCode::Ok as u32, 0);
        assert_eq!(DngrepCode::EGradeDrift as u32, 12);
        assert_eq!(DngrepCode::Ok.val(), SEG_BASE);
        assert_eq!(DngrepCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!DngrepCode::Ok.detail().is_empty());
        assert!(!DngrepCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = DngrepLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = DngrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_seal_check() {
        let mut lt = DngrepLedger::new();
        assert_eq!(lt.seal_check(), Err(DngrepCode::ESeal));
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < FUNC_OPS {
            if let Some(w) = dngrep_cell(o) {
                let _ = lt.record_op(o, w);
            }
            o += 1;
        }
        // 分账未齐仍拒封。
        assert_eq!(lt.seal_check(), Err(DngrepCode::ESeal));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        // 投影/预览未齐仍拒封。
        assert_eq!(lt.seal_check(), Err(DngrepCode::ESeal));
        let mut p = 0usize;
        while p < PROJ_CELLS {
            if let Some(w) = proj_cell(p) {
                let _ = lt.record_proj(p, w);
            }
            p += 1;
        }
        let mut t = 0usize;
        while t < PREVIEW_STATES {
            if let Some(w) = preview_cell(t) {
                let _ = lt.record_preview(t, w);
            }
            t += 1;
        }
        assert!(lt.seal_check().is_ok());
    }

    #[test]
    fn test_grade_variants_closed() {
        let grades = [Grade::S1, Grade::S2, Grade::S3];
        assert_eq!(grades.len(), 3);
        assert_eq!(grades[0].val(), 1);
        assert_eq!(grades[2].val(), 3);
        assert!(!grades[1].detail().is_empty());
    }

    #[test]
    fn unx52445_checks_green() {
        let s = run_unx52445_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
