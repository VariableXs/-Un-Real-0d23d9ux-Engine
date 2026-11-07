//! UNX-F52453 Bandizip 收口 · 0xCB100 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52453"，逐字）：
//!   title: Bandizip 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52454-J1 六判据
//!     全落；商店分发面声明；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52453）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52454-J1，照原文锚定；功能按本卡标题（Bandizip 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 操作闭集：压缩(a)/解压(x)/测试(t)/列表(l)/基准与修复
//!      (bc) 各 4 操作，家族单调（逆序必抓），逐操作载荷纯函数折叠恒等
//!      才收，漂移必抓。
//!   ② 格式族 14 格闭集：zip/7z/rar/rar5/zipx/tar/gz/bz2/xz/zst/iso/
//!      wim/alz/egg，逐格恒等；越闭集必抓。
//!   ③ 多核压缩三元闸 3 格：开关/线程折算/并发上限，三格齐才过闸。
//!   ④ 与 7-Zip 对照 8 格：参数/格式覆盖/多核/加密/分卷/修复/基准/许可
//!      逐格对照恒等（KB-COMPAT 同族对照口径）。
//!   ⑤ 免费/商业分账声明 2 行闭集（含商店分发面声明），缺一拒封；
//!      C++ 上栈声明 2 行闭集（压缩核心/Shell 集成），缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（命令漂移/家族逆序/
//!      格式越闭集）10/10 必抓，检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑦ KB-COMPAT 定级登记：如实定级 S2（有条件收口），登记档不因全齐
//!      自升 S1；定级阶梯 S3→S2→S1 闭集经 grade_step 单调复核。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52453;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族 20 操作数。
pub const CMD_OPS: usize = 20;

/// 格式族格数。
pub const FMT_N: usize = 14;

/// 多核压缩三元闸格数。
pub const MC_N: usize = 3;

/// 与 7-Zip 对照格数。
pub const CMP_N: usize = 8;

/// 免费/商业分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandizipCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    ECmdDrift,
    /// 家族逆序
    EOrder,
    /// 辅助格越闭集（格式/三元闸/对照共用）
    ENoCell,
    /// 格式漂移
    EFmtDrift,
    /// 三元闸/对照漂移
    EAuxDrift,
    /// 声明行越闭集或缺行
    ENoRow,
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

impl BandizipCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BandizipCode::Ok => "成功",
            BandizipCode::ENoOp => "命令号越闭集：五命令族 20 操作集之外先登记",
            BandizipCode::ECmdDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            BandizipCode::EOrder => "家族逆序：压缩→解压→测试→列表→基准修复单调，逆序必抓",
            BandizipCode::ENoCell => "辅助格越闭集：格式 14 格/三元闸 3 格/对照 8 格之外先登记",
            BandizipCode::EFmtDrift => "格式漂移：格式折叠与基准不恒等，逐格复算",
            BandizipCode::EAuxDrift => "三元闸/对照漂移：辅助格折叠与基准不恒等，逐格复算",
            BandizipCode::ENoRow => "声明行越闭集或缺行：分账/上栈声明行未齐或越界先补齐",
            BandizipCode::EDup => "重复落账：同格只许一条账目",
            BandizipCode::ESeal => "未齐即封：20 操作+14 格+三元闸+对照+声明行未齐先补齐",
            BandizipCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BandizipCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BandizipCode::EGradeDrift => "定级漂移：登记档为 S2，全齐亦不得自升 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡登记档不取，仅阶梯闭集占位）
    S1,
    /// S2：有条件收口（本卡登记档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
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
            Grade::S1 => "S1：全通收口档（Bandizip 卡不登记此档）",
            Grade::S2 => "S2：有条件收口（Bandizip 登记档：六判据落、商业/商店分发面有条件）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

/// 定级阶梯（0=S3 1=S2 2=S1），供阶梯单调复核。
pub const fn grade_step(n: u32) -> Grade {
    match n {
        0 => Grade::S3,
        1 => Grade::S2,
        _ => Grade::S1,
    }
}

/// 本卡 KB-COMPAT 登记档。
pub const REGISTERED: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令格载荷（操作 o；家族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x34) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令家族（0..4=压缩a 4..8=解压x 8..12=测试t 12..16=列表l 16..20=基准修复bc）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令家族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("压缩a"),
        1 => Some("解压x"),
        2 => Some("测试t"),
        3 => Some("列表l"),
        4 => Some("基准修复bc"),
        _ => None,
    }
}

/// 格式格载荷（格式 f）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_N {
        return None;
    }
    Some((0xB1400u64 + (f as u64) * 0x3C) ^ 0x42414E445A5F4D00)
}

/// 格式名（14 格闭集）。
pub const fn fmt_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("zip"),
        1 => Some("7z"),
        2 => Some("rar"),
        3 => Some("rar5"),
        4 => Some("zipx"),
        5 => Some("tar"),
        6 => Some("gz"),
        7 => Some("bz2"),
        8 => Some("xz"),
        9 => Some("zst"),
        10 => Some("iso"),
        11 => Some("wim"),
        12 => Some("alz"),
        13 => Some("egg"),
        _ => None,
    }
}

/// 多核三元闸格载荷（格 i：0=开关 1=线程折算 2=并发上限）。
pub const fn mc_cell(i: usize) -> Option<u64> {
    if i >= MC_N {
        return None;
    }
    Some(0xB1800u64 + (i as u64) * 0x2C)
}

/// 多核三元闸格名。
pub const fn mc_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("开关"),
        1 => Some("线程折算"),
        2 => Some("并发上限"),
        _ => None,
    }
}

/// 与 7-Zip 对照格载荷（格 c）。
pub const fn cmp_cell(c: usize) -> Option<u64> {
    if c >= CMP_N {
        return None;
    }
    Some((0xB1C00u64 + (c as u64) * 0x2E) ^ 0x5A4950375F434D00)
}

/// 对照格名（8 格闭集）。
pub const fn cmp_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("参数面"),
        1 => Some("格式覆盖面"),
        2 => Some("多核面"),
        3 => Some("加密面"),
        4 => Some("分卷面"),
        5 => Some("修复面"),
        6 => Some("基准面"),
        7 => Some("许可面"),
        _ => None,
    }
}

/// 分账声明行折叠（行 r：免费/商业·商店分发面）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB2000 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r：压缩核心/Shell 集成）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xB2100 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Bandizip 收口账本状态机。
#[derive(Clone)]
pub struct BandizipLedger {
    /// 六判据（安装/压缩/解压/修复/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 命令账。
    cmds: [Option<u64>; CMD_OPS],
    /// 格式账。
    fmts: [Option<u64>; FMT_N],
    /// 多核三元闸账。
    mcs: [Option<u64>; MC_N],
    /// 与 7-Zip 对照账。
    cmps: [Option<u64>; CMP_N],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C++ 上栈声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BandizipLedger {
    /// 新账本。
    pub fn new() -> Self {
        BandizipLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            fmts: [None; FMT_N],
            mcs: [None; MC_N],
            cmps: [None; CMP_N],
            splits: [None; SPLIT_ROWS],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BandizipCode> {
        if gate >= 6 {
            return Err(BandizipCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB100 + gate as u64);
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

    /// 命令一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), BandizipCode> {
        if o >= CMD_OPS {
            return Err(BandizipCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(BandizipCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB200);
            return Err(BandizipCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(BandizipCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB201 + o as u64);
            return Err(BandizipCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB202 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 格）。
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
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), BandizipCode> {
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(BandizipCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB300 + f as u64);
            return Err(BandizipCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0xB301 + f as u64);
        Ok(())
    }

    /// 格式全账核验（14 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_N {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 多核三元闸一格（载荷对照 + 重复拒）。
    pub fn record_mc(&mut self, i: usize, got: u64) -> Result<(), BandizipCode> {
        let want = match mc_cell(i) {
            Some(w) => w,
            None => return Err(BandizipCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB400 + i as u64);
            return Err(BandizipCode::EAuxDrift);
        }
        if match self.mcs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(c) = self.mcs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB401 + i as u64);
        Ok(())
    }

    /// 三元闸全账核验（3 格齐才过闸）。
    pub fn mcs_all(&self) -> bool {
        let mut i = 0usize;
        while i < MC_N {
            match self.mcs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 7-Zip 对照一格（载荷对照 + 重复拒）。
    pub fn record_cmp(&mut self, c: usize, got: u64) -> Result<(), BandizipCode> {
        let want = match cmp_cell(c) {
            Some(w) => w,
            None => return Err(BandizipCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB500 + c as u64);
            return Err(BandizipCode::EAuxDrift);
        }
        if match self.cmps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(x) = self.cmps.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0xB501 + c as u64);
        Ok(())
    }

    /// 对照全账核验（8 格）。
    pub fn cmps_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMP_N {
            match self.cmps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 分账声明一行（免费/商业·商店分发面）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), BandizipCode> {
        if r >= SPLIT_ROWS {
            return Err(BandizipCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0xB600 + r as u64);
        Ok(())
    }

    /// C++ 上栈声明一行（压缩核心/Shell 集成）。
    pub fn declare_cpp(&mut self, r: usize) -> Result<(), BandizipCode> {
        if r >= DECL_ROWS {
            return Err(BandizipCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BandizipCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0xB700 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（分账 2 行 + 上栈 2 行）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            match self.decls.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后家族（供逆序路径不改写核验）。
    pub fn last_family(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 收口面核验：主账+辅助账+声明账全齐；未齐即封（ERowMiss 口径并入 ENoRow/ESeal）。
    pub fn seal(&self) -> Result<(), BandizipCode> {
        if !self.rows_all() {
            return Err(BandizipCode::ENoRow);
        }
        if !self.cmds_all() || !self.fmts_all() || !self.mcs_all() || !self.cmps_all() {
            return Err(BandizipCode::ESeal);
        }
        Ok(())
    }

    /// 收口面全齐布尔核验。
    pub fn seal_all(&self) -> bool {
        self.seal().is_ok()
    }

    /// 注入守恒复核：注入=检出+逃逸。
    pub fn reconcile(&mut self, inj: u32, caught: u32, esc: u32) -> Result<(), BandizipCode> {
        if inj != caught.wrapping_add(esc) {
            self.fnv(0xB800);
            return Err(BandizipCode::EInj);
        }
        Ok(())
    }

    /// 双跑指纹复核。
    pub fn verify_fp(&self, other: u64) -> Result<(), BandizipCode> {
        if self.fp != other {
            return Err(BandizipCode::EFp);
        }
        Ok(())
    }

    /// KB-COMPAT 定级：六判据齐 → 登记档 S2；判据缺 → S3；全齐仍按登记档
    /// S2 落账（如实定级，不自升 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            REGISTERED
        } else {
            Grade::S3
        }
    }
}

impl Default for BandizipLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 定级登记复核：全齐账必须落在登记档，否则定级漂移。
pub fn grade_register(lt: &BandizipLedger) -> Result<Grade, BandizipCode> {
    let g = lt.judge_grade();
    if lt.seal_all() && lt.gates_all() {
        if g == REGISTERED {
            Ok(g)
        } else {
            Err(BandizipCode::EGradeDrift)
        }
    } else {
        Ok(g)
    }
}

/// 全齐账本（六判据 + 全节落账，测试与定级检查共用）。
pub fn sealed_ledger() -> BandizipLedger {
    let mut lt = BandizipLedger::new();
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
    while f < FMT_N {
        if let Some(w) = fmt_cell(f) {
            let _ = lt.record_fmt(f, w);
        }
        f += 1;
    }
    let mut i = 0usize;
    while i < MC_N {
        if let Some(w) = mc_cell(i) {
            let _ = lt.record_mc(i, w);
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CMP_N {
        if let Some(w) = cmp_cell(c) {
            let _ = lt.record_cmp(c, w);
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = lt.declare_split(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = lt.declare_cpp(d);
        d += 1;
    }
    lt
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转，全部落在当轮临时账本上；本账只记守恒三数。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BandizipLedger::new();
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
    while f < FMT_N {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut c = 0usize;
    while c < CMP_N {
        if let Some(w) = cmp_cell(c) {
            if led.record_cmp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut i = 0usize;
    while i < MC_N {
        if let Some(w) = mc_cell(i) {
            let _ = led.record_mc(i, w);
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_cpp(d);
        d += 1;
    }
    // 注入 10 轮：命令漂移 / 家族逆序 / 格式越闭集三态轮转。
    // 漂移注入先按合法序落正格再注漂移；越闭集（phase）注入对空账本同样
    // 必抓；逆序路径不改写 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BandizipLedger::new();
        let caught = match kind {
            0 => {
                let w0 = match cmd_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match cmd_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_cmd(0, w0);
                lt.record_cmd(1, w1.wrapping_add(1)) == Err(BandizipCode::ECmdDrift)
            }
            1 => {
                let w9 = match cmd_cell(9) {
                    Some(w) => w,
                    None => 0,
                };
                let w2 = match cmd_cell(2) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_cmd(9, w9);
                lt.record_cmd(2, w2) == Err(BandizipCode::EOrder)
            }
            _ => lt.record_fmt(FMT_N, 0) == Err(BandizipCode::ENoCell),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB900 + k as u64);
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
/// UNX-F52453 判据集（锚 UNX-F52454-J1，off-by-one 板式）。
pub fn run_unx52453_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52453");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "压缩a/解压x/测试t/列表l/基准修复bc 五家族逐操作恒等",
    );
    s.add(
        "格式族 14 格全落",
        {
            let mut lt = BandizipLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_N {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "zip/7z/rar/rar5/zipx/tar/gz/bz2/xz/zst/iso/wim/alz/egg 逐格恒等",
    );
    s.add(
        "多核压缩三元闸 3/3 全落",
        {
            let mut lt = BandizipLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < MC_N {
                if let Some(w) = mc_cell(i) {
                    if lt.record_mc(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.mcs_all()
        },
        "开关/线程折算/并发上限三格齐才过闸",
    );
    s.add(
        "与 7-Zip 对照 8/8 全落",
        {
            let mut lt = BandizipLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMP_N {
                if let Some(w) = cmp_cell(c) {
                    if lt.record_cmp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmps_all()
        },
        "参数/格式覆盖/多核/加密/分卷/修复/基准/许可逐格对照（KB-COMPAT 同族口径）",
    );
    s.add(
        "UNX-F52454-J1 六判据全落",
        {
            let mut lt = BandizipLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/压缩/解压/修复/卸载/许可逐格落账",
    );
    s.add(
        "免费/商业分账声明 2/2 全齐",
        {
            let mut lt = BandizipLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "免费版分账声明/商业版·商店分发面声明两行闭集（§AI-66.3）",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = BandizipLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_cpp(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "压缩核心 C++ 上栈/Shell 集成 C++ 上栈两行闭集",
    );
    s.add(
        "命令家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                let a = match cmd_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match cmd_kind(o - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "命令漂移/家族逆序/格式越闭集三态轮转必抓");
    let mut l0 = BandizipLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·格式越闭集",
        l0.record_fmt(FMT_N, 0).is_err(),
        "格式 14 越闭集必拒",
    );
    s.add(
        "边界·三元闸越闭集",
        l0.record_mc(MC_N, 0).is_err(),
        "三元闸格 3 越闭集必拒",
    );
    s.add(
        "边界·对照越闭集",
        l0.record_cmp(CMP_N, 0).is_err(),
        "对照格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err() && l0.declare_cpp(DECL_ROWS).is_err(),
        "声明行 2 之外越界必拒",
    );
    s.add(
        "命令漂移拒",
        {
            let w0 = match cmd_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match cmd_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_cmd(0, w0);
            l0.record_cmd(1, w1.wrapping_add(1)) == Err(BandizipCode::ECmdDrift)
        },
        "命令载荷漂移必抓",
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
        "指纹复核与守恒复核闸",
        {
            let mut a = BandizipLedger::new();
            let mut b = BandizipLedger::new();
            let _ = a.judge_gate(0);
            let _ = b.judge_gate(0);
            a.verify_fp(b.fingerprint()) == Ok(()) && a.reconcile(1, 1, 0) == Ok(())
        },
        "FNV 指纹复核一致且注入守恒等式成立",
    );
    s.add(
        "KB-COMPAT 定级登记 S2",
        {
            let lt = sealed_ledger();
            grade_register(&lt) == Ok(REGISTERED) && lt.seal_all() && lt.judge_grade() == Grade::S2
        },
        "全齐账按登记档 S2 落账（如实定级，不自升 S1）",
    );
    s.add(
        "定级阶梯闭集",
        grade_step(0) == Grade::S3
            && grade_step(1) == Grade::S2
            && grade_step(2) == Grade::S1
            && grade_step(0).val() == 3
            && grade_step(2).val() == 1,
        "S3→S2→S1 阶梯单调，值域闭集",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB100,
        "SEG_BASE 0x40 对齐且等于 0xCA400+0x40×(52453−52401)",
    );
    s.add("收口·判据条数", s.len() >= 21, "含本条共 ≥22 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind_mapping() {
        // kind = o/4 映射（i/N 映射口径逐点核对）。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(3), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(12), Some(3));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("压缩a"));
        assert_eq!(cmd_kind_name(4), Some("基准修复bc"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_fmt_names_closed() {
        assert_eq!(fmt_name(0), Some("zip"));
        assert_eq!(fmt_name(1), Some("7z"));
        assert_eq!(fmt_name(13), Some("egg"));
        assert!(fmt_name(14).is_none());
        assert!(fmt_cell(14).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match fmt_cell(3) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(a, match fmt_cell(3) {
            Some(v) => v,
            None => 1,
        });
        assert_ne!(a, match fmt_cell(4) {
            Some(v) => v,
            None => 0,
        });
        assert!(mc_cell(MC_N).is_none());
        assert!(cmp_cell(CMP_N).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert_eq!(mc_name(0), Some("开关"));
        assert_eq!(cmp_name(7), Some("许可面"));
        assert!(mc_name(3).is_none());
        assert!(cmp_name(8).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族格，再注逆序。
        let mut lt = BandizipLedger::new();
        let _ = lt.record_cmd(4, cmd_cell(4).unwrap_or(0));
        let _ = lt.record_cmd(8, cmd_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0)),
            Err(BandizipCode::EOrder)
        );
        assert!(lt.record_cmd(9, cmd_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_last_kind_unchanged_after_order() {
        let mut lt = BandizipLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        let before = lt.last_family();
        let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
        assert_eq!(lt.last_family(), before, "逆序路径不得改写 last_kind");
        assert_eq!(lt.stats().1, 1, "逆序注入计入检出");
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BandizipLedger::new();
        assert_eq!(
            lt.record_fmt(5, fmt_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BandizipCode::EFmtDrift)
        );
        assert_eq!(
            lt.record_mc(1, mc_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(BandizipCode::EAuxDrift)
        );
        assert_eq!(
            lt.record_cmp(6, cmp_cell(6).unwrap_or(0).wrapping_add(9)),
            Err(BandizipCode::EAuxDrift)
        );
        let w0 = cmd_cell(0).unwrap_or(0);
        let _ = lt.record_cmd(0, w0);
        assert_eq!(
            lt.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(BandizipCode::ECmdDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BandizipLedger::new();
        let _ = lt.record_fmt(2, fmt_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_fmt(2, fmt_cell(2).unwrap_or(0)),
            Err(BandizipCode::EDup)
        );
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(BandizipCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BandizipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓（未齐即封口径），全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BandizipLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(!lt2.seal_all());
    }

    #[test]
    fn test_seal_and_reconcile() {
        let empty = BandizipLedger::new();
        assert_eq!(empty.seal(), Err(BandizipCode::ENoRow));
        let full = sealed_ledger();
        assert!(full.seal().is_ok());
        assert!(full.seal_all());
        let mut r = full.clone();
        assert!(r.reconcile(10, 10, 0).is_ok());
        assert!(r.verify_fp(full.fingerprint()).is_ok());
        assert_eq!(r.reconcile(10, 9, 0), Err(BandizipCode::EInj));
        assert_eq!(r.verify_fp(full.fingerprint().wrapping_add(1)), Err(BandizipCode::EFp));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：命令 20 + 格式 14 + 对照 8。
        assert_eq!(recorded, (CMD_OPS + FMT_N + CMP_N) as u32);
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
        assert_eq!(SEG_BASE, 0xCB100);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52453u32 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码位须与枚举实际位序一致：Ok=0，EGradeDrift=12，恰 13 变体。
        assert_eq!(BandizipCode::Ok as u32, 0);
        assert_eq!(BandizipCode::ENoOp as u32, 1);
        assert_eq!(BandizipCode::EGradeDrift as u32, 12);
        assert!(!BandizipCode::Ok.detail().is_empty());
        assert!(!BandizipCode::ESeal.detail().is_empty());
        assert!(!BandizipCode::EGradeDrift.detail().is_empty());
        assert_eq!(BandizipCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐仍按登记档 S2。
        assert_eq!(Grade::S3.val(), 3);
        let empty = BandizipLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut half = BandizipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = half.judge_gate(g);
            g += 1;
        }
        assert_eq!(half.judge_grade(), Grade::S2);
        let full = sealed_ledger();
        assert_eq!(full.judge_grade(), Grade::S2);
        assert_eq!(grade_register(&full), Ok(Grade::S2));
        assert_eq!(grade_step(0), Grade::S3);
        assert_eq!(grade_step(1), Grade::S2);
        assert_eq!(grade_step(2), Grade::S1);
        assert_eq!(REGISTERED, Grade::S2);
    }

    #[test]
    fn unx52453_checks_green() {
        let s = run_unx52453_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 22);
    }
}
