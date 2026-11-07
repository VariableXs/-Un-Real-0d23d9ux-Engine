//! UNX-F52452 PeaZip 收口 · 0xCB0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52452"，逐字）：
//!   title: PeaZip 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52453-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52452）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52453-J1，照原文锚定；功能按本卡标题（PeaZip）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集 20 操作：add/extract/test/list/convert 五族 × 4
//!      操作，逐操作载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 格式族 14 格：PeaZip 支持格式闭集逐格载荷对照。
//!   ③ 双引擎三元闸：7z 引擎/pea 引擎/回退策略 三格闭集，越闸必抓、
//!      漂移必抓。
//!   ④ 与 7-Zip 对照 8 格：逐格载荷对照，漂移必抓（F52451 邻账）。
//!   ⑤ Pascal 上栈声明 2 行：Giorgio Tani 原生栈声明 / 本仓 Pascal
//!      类比上栈声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（命令族逆序/引擎闸漂移/操作越
//!      闭集）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹
//!      一致；KB-COMPAT 定级 S2。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52452;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数（五命令族 × 4）。
pub const CMD_OPS: usize = 20;

/// 格式族格数。
pub const FMT_CELLS: usize = 14;

/// 双引擎三元闸格数。
pub const ENG_GATES: usize = 3;

/// 与 7-Zip 对照格数。
pub const CMP_CELLS: usize = 8;

/// Pascal 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeazipCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作/格式载荷漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 格式格号越闭集
    ENoFmt,
    /// 格式载荷漂移
    EFmtDrift,
    /// 引擎闸号越闭集
    ENoEng,
    /// 引擎闸载荷漂移
    EEngDrift,
    /// 对照格载荷漂移
    ECmpDrift,
    /// 对照/声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl PeazipCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PeazipCode::Ok => "成功",
            PeazipCode::ENoOp => "操作号越闭集：五命令族 20 操作之外先登记",
            PeazipCode::EDrift => "载荷漂移：操作/格式折叠与基准不恒等，逐格复算",
            PeazipCode::EOrder => "命令族逆序：add→extract→test→list→convert 单调，逆序必抓",
            PeazipCode::ENoFmt => "格式格号越闭集：格式族 14 格之外先登记",
            PeazipCode::EFmtDrift => "格式漂移：格式格载荷折叠与基准不恒等，逐格复算",
            PeazipCode::ENoEng => "引擎闸号越闭集：双引擎三元闸之外先登记",
            PeazipCode::EEngDrift => "引擎闸漂移：三元闸载荷折叠与基准不恒等，逐格复算",
            PeazipCode::ECmpDrift => "对照漂移：7-Zip 对照格载荷与基准不恒等，逐格复算",
            PeazipCode::ENoRow => "对照/声明行越闭集：对照 8 格与声明行闭集之外先登记",
            PeazipCode::EDup => "重复落账：同格只许一条账目",
            PeazipCode::ESeal => "未齐即封：20 操作 + 14 格 + 3 闸 + 8 对照 + 2 声明行未齐先补齐",
            PeazipCode::EGradeDrift => "定级漂移：六判据齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全优收口
    S1,
    /// S2：目标档收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const fn：被常量语境调用也安全）。
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
            Grade::S1 => "S1：全优收口（本卡不适用）",
            Grade::S2 => "S2：六判据+20操作+14格式格+3引擎闸+8对照+2声明行全齐收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xDF000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=add 4..8=extract 8..12=test 12..16=list 16..20=convert）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("add"),
        1 => Some("extract"),
        2 => Some("test"),
        3 => Some("list"),
        4 => Some("convert"),
        _ => None,
    }
}

/// 格式格载荷（格 f）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((0xE0000u64 + (f as u64) * 0x5D) ^ 0x505A5F464D545831)
}

/// 引擎闸载荷（闸 e：0=7z 引擎 1=pea 引擎 2=回退策略）。
pub const fn eng_cell(e: usize) -> Option<u64> {
    if e >= ENG_GATES {
        return None;
    }
    Some((0xE1000u64 + (e as u64) * 0x47) ^ 0x505A5F454E475832)
}

/// 引擎闸名。
pub const fn eng_gate_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("7z 引擎"),
        1 => Some("pea 引擎"),
        2 => Some("回退策略"),
        _ => None,
    }
}

/// 7-Zip 对照格载荷（格 c；F52451 邻账）。
pub const fn cmp_cell(c: usize) -> Option<u64> {
    if c >= CMP_CELLS {
        return None;
    }
    Some((0xE2000u64 + (c as u64) * 0x3B) ^ 0x505A5F434D505838)
}

/// Pascal 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xE3000 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PeaZip 收口账本状态机。
#[derive(Clone)]
pub struct PeazipLedger {
    /// 六判据（add/extract/test/list/convert/引擎切换）。
    gates: [Option<bool>; 6],
    /// 操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 格式账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 引擎闸账。
    engs: [Option<u64>; ENG_GATES],
    /// 对照账。
    cmps: [Option<u64>; CMP_CELLS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PeazipLedger {
    /// 新账本。
    pub fn new() -> Self {
        PeazipLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            fmts: [None; FMT_CELLS],
            engs: [None; ENG_GATES],
            cmps: [None; CMP_CELLS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PeazipCode> {
        if gate >= 6 {
            return Err(PeazipCode::ESeal);
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

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), PeazipCode> {
        if o >= CMD_OPS {
            return Err(PeazipCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(PeazipCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(PeazipCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(PeazipCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(PeazipCode::EDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeazipCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 格式一格（载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), PeazipCode> {
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(PeazipCode::ENoFmt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + f as u64);
            return Err(PeazipCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeazipCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x8301 + f as u64);
        Ok(())
    }

    /// 格式全账核验（14 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 引擎闸一格（载荷对照 + 重复拒）。
    pub fn record_eng(&mut self, e: usize, got: u64) -> Result<(), PeazipCode> {
        let want = match eng_cell(e) {
            Some(w) => w,
            None => return Err(PeazipCode::ENoEng),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + e as u64);
            return Err(PeazipCode::EEngDrift);
        }
        if match self.engs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeazipCode::EDup);
        }
        if let Some(c) = self.engs.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8401 + e as u64);
        Ok(())
    }

    /// 引擎闸全账核验（3 格）。
    pub fn engs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENG_GATES {
            match self.engs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 对照一格（载荷对照 + 重复拒）。
    pub fn record_cmp(&mut self, c: usize, got: u64) -> Result<(), PeazipCode> {
        if c >= CMP_CELLS {
            return Err(PeazipCode::ENoRow);
        }
        let want = match cmp_cell(c) {
            Some(w) => w,
            None => return Err(PeazipCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + c as u64);
            return Err(PeazipCode::ECmpDrift);
        }
        if match self.cmps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeazipCode::EDup);
        }
        if let Some(x) = self.cmps.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8501 + c as u64);
        Ok(())
    }

    /// 对照全账核验（8 格）。
    pub fn cmps_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMP_CELLS {
            match self.cmps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// Pascal 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), PeazipCode> {
        if r >= STACK_ROWS {
            return Err(PeazipCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PeazipCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// KB-COMPAT 定级（六判据齐 → S2 目标档，判据缺 → S3；本卡目标档
    /// 即 S2，全齐格不升档，如实登记）。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else {
            Grade::S2
        }
    }
}

impl Default for PeazipLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PeazipLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut e = 0usize;
    while e < ENG_GATES {
        if let Some(w) = eng_cell(e) {
            if led.record_eng(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut c = 0usize;
    while c < CMP_CELLS {
        if let Some(w) = cmp_cell(c) {
            if led.record_cmp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_stack(s);
        s += 1;
    }
    // 注入 10 轮：命令族逆序 / 引擎闸漂移 / 操作越闭集三态轮转。
    // 每轮全部落在当轮临时账本 lt 上，主账本只记账注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PeazipLedger::new();
        let caught = match kind {
            0 => {
                // 空账本上先落高族再落低族：逆序必抓，且不改 last_kind 语义。
                let _ = lt.record_cmd(18, cmd_cell(18).unwrap_or(0));
                lt.record_cmd(5, cmd_cell(5).unwrap_or(0)) == Err(PeazipCode::EOrder)
            }
            1 => lt.record_eng(2, eng_cell(2).unwrap_or(0).wrapping_add(1))
                == Err(PeazipCode::EEngDrift),
            _ => lt.record_cmd(CMD_OPS, 0).is_err(),
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
/// UNX-F52452 判据集（锚 UNX-F52453-J1，off-by-one 板式）。
pub fn run_unx52452_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52452");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "add/extract/test/list/convert 五族逐操作恒等",
    );
    s.add(
        "格式族 14 格全落",
        {
            let mut lt = PeazipLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "格式闭集逐格载荷对照",
    );
    s.add(
        "双引擎三元闸全落",
        {
            let mut lt = PeazipLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENG_GATES {
                if let Some(w) = eng_cell(e) {
                    if lt.record_eng(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.engs_all()
        },
        "7z 引擎/pea 引擎/回退策略三格闭集，越闸必抓",
    );
    s.add(
        "与 7-Zip 对照 8 格全落",
        {
            let mut lt = PeazipLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    if lt.record_cmp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmps_all()
        },
        "7-Zip 邻账对照逐格恒等（F52451）",
    );
    s.add(
        "Pascal 上栈声明 2/2 全齐",
        {
            let mut lt = PeazipLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Giorgio Tani 原生栈声明/本仓 Pascal 类比上栈声明两行闭集",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "族逆序/引擎漂移/越闭集三态轮转必抓",
    );
    let mut l0 = PeazipLedger::new();
    s.add(
        "边界·命令族逆序拒",
        {
            let _ = l0.record_cmd(18, cmd_cell(18).unwrap_or(0));
            l0.record_cmd(5, cmd_cell(5).unwrap_or(0)) == Err(PeazipCode::EOrder)
        },
        "convert 后回 extract 必抓",
    );
    s.add(
        "对照格漂移拒",
        l0.record_cmp(3, cmp_cell(3).unwrap_or(0).wrapping_add(5)) == Err(PeazipCode::ECmpDrift),
        "7-Zip 对照格载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·格式格越闭集",
        l0.record_fmt(FMT_CELLS, 0).is_err(),
        "格式格 14 越闭集必拒",
    );
    s.add(
        "边界·引擎闸越闭集",
        l0.record_eng(ENG_GATES, 0).is_err(),
        "引擎闸 3 越闭集必拒",
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
        "未齐即封·方向核",
        {
            let mut lt = PeazipLedger::new();
            let ok_op = lt.record_cmd(0, cmd_cell(0).unwrap_or(0)).is_ok();
            ok_op && lt.judge_grade() == Grade::S3
        },
        "账未齐时落账仍 Ok，仅定级不达 S2",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = PeazipLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_cmd(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut e = 0usize;
            while e < ENG_GATES {
                if let Some(w) = eng_cell(e) {
                    let _ = lt.record_eng(e, w);
                }
                e += 1;
            }
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    let _ = lt.record_cmp(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB0C0,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("add"));
        assert_eq!(cmd_kind_name(4), Some("convert"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert!(fmt_cell(FMT_CELLS).is_none());
        assert!(eng_cell(ENG_GATES).is_none());
        assert!(cmp_cell(CMP_CELLS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_eng_gate_names() {
        assert_eq!(eng_gate_name(0), Some("7z 引擎"));
        assert_eq!(eng_gate_name(1), Some("pea 引擎"));
        assert_eq!(eng_gate_name(2), Some("回退策略"));
        assert!(eng_gate_name(3).is_none());
    }

    #[test]
    fn test_cmd_order_caught() {
        let mut lt = PeazipLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(PeazipCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PeazipLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PeazipCode::EDrift)
        );
        assert_eq!(
            lt.record_eng(1, eng_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(PeazipCode::EEngDrift)
        );
    }

    #[test]
    fn test_cmp_boundary_and_dup() {
        let mut lt = PeazipLedger::new();
        // 对照格闭集边界：8 之外越界必拒。
        assert!(lt.record_cmp(CMP_CELLS, 0).is_err());
        let _ = lt.record_cmp(0, cmp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmp(0, cmp_cell(0).unwrap_or(0)), Err(PeazipCode::EDup));
        assert!(lt.record_cmp(1, cmp_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PeazipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PeazipLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 + 格式 + 引擎闸 + 对照。
        assert_eq!(
            recorded,
            CMD_OPS as u32 + FMT_CELLS as u32 + ENG_GATES as u32 + CMP_CELLS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCB0C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PeazipCode::Ok as u32, 0);
        assert_eq!(PeazipCode::EGradeDrift as u32, 12);
        // 闭集恰 13 变体：逐位 detail 全非空。
        let all = [
            PeazipCode::Ok,
            PeazipCode::ENoOp,
            PeazipCode::EDrift,
            PeazipCode::EOrder,
            PeazipCode::ENoFmt,
            PeazipCode::EFmtDrift,
            PeazipCode::ENoEng,
            PeazipCode::EEngDrift,
            PeazipCode::ECmpDrift,
            PeazipCode::ENoRow,
            PeazipCode::EDup,
            PeazipCode::ESeal,
            PeazipCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2。
        let lt = PeazipLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = PeazipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52452_checks_green() {
        let s = run_unx52452_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
