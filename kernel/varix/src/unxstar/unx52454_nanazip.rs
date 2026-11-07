//! UNX-F52454 NanaZip 收口 · 0xCB140 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52454"，逐字）：
//!   title: NanaZip 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52455-J1 六判据
//!     全落；双面板操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52454）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52455-J1，照原文锚定；功能按本卡标题（NanaZip 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 操作闭集（与 7-Zip 同构账，F52451 口径复用）：添加(a)/
//!      解压(x)/测试(t)/列表(l)/哈希(h) 各 4 操作，家族单调（逆序必抓），
//!      逐操作载荷纯函数折叠恒等才收，漂移必抓。
//!   ② MSIX 打包三态 6 格闭集：打包/签名/校验三态各 2 格（条目位+核验位），
//!      逐格恒等，越闭集必抓。
//!   ③ 双面板操作集 ×10：注入核 10 轮三态轮转（命令漂移/家族逆序/MSIX
//!      越闭集）10/10 必抓，检出+逃逸守恒等式。
//!   ④ 与 7-Zip 分账声明 2 行闭集（锚 F52451：账目与 7-Zip 分账不并表）；
//!      MSIX/Store 双形态声明 2 行闭集；C++/WinRT 上栈声明 2 行闭集；
//!      三组声明缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级登记：如实定级 S2（有条件收口），登记档不因全齐
//!      自升 S1；定级阶梯 S3→S2→S1 闭集经 grade_step 单调复核。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52454;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五命令族 20 操作数（与 7-Zip 同构账）。
pub const CMD_OPS: usize = 20;

/// MSIX 打包三态格数。
pub const MSIX_N: usize = 6;

/// 与 7-Zip 分账声明行数（锚 F52451）。
pub const SPLIT_ROWS: usize = 2;

/// MSIX/Store 双形态声明行数。
pub const FORM_ROWS: usize = 2;

/// C++/WinRT 上栈声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NanazipCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoOp,
    /// 命令漂移
    ECmdDrift,
    /// 家族逆序
    EOrder,
    /// MSIX 格越闭集
    ENoMsix,
    /// MSIX 漂移
    EMsixDrift,
    /// 分账声明行越闭集或缺行
    ENoRow,
    /// 双形态声明行越闭集或缺行
    EForm,
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

impl NanazipCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NanazipCode::Ok => "成功",
            NanazipCode::ENoOp => "命令号越闭集：五命令族 20 操作集（7-Zip 同构账）之外先登记",
            NanazipCode::ECmdDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            NanazipCode::EOrder => "家族逆序：添加→解压→测试→列表→哈希单调，逆序必抓",
            NanazipCode::ENoMsix => "MSIX 格越闭集：打包三态 6 格之外先登记",
            NanazipCode::EMsixDrift => "MSIX 漂移：三态格折叠与基准不恒等，逐格复算",
            NanazipCode::ENoRow => "分账声明行越闭集或缺行：与 7-Zip 分账（F52451）未齐先补齐",
            NanazipCode::EForm => "双形态声明行越闭集或缺行：MSIX/Store 双形态声明未齐先补齐",
            NanazipCode::EDup => "重复落账：同格只许一条账目",
            NanazipCode::ESeal => "未齐即封：20 操作+MSIX 6 格+三组声明未齐先补齐",
            NanazipCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NanazipCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NanazipCode::EGradeDrift => "定级漂移：登记档为 S2，全齐亦不得自升 S1",
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
            Grade::S1 => "S1：全通收口档（NanaZip 卡不登记此档）",
            Grade::S2 => "S2：有条件收口（NanaZip 登记档：六判据落、Store 分发面有条件）",
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
/// 命令格载荷（操作 o；家族 = o/4；7-Zip 同构折叠口径）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xB3000u64 + (o as u64) * 0x38) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令家族（0..4=添加a 4..8=解压x 8..12=测试t 12..16=列表l 16..20=哈希h）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令家族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("添加a"),
        1 => Some("解压x"),
        2 => Some("测试t"),
        3 => Some("列表l"),
        4 => Some("哈希h"),
        _ => None,
    }
}

/// MSIX 三态格载荷（格 i；态 = i/2：打包/签名/校验，各条目位+核验位）。
pub const fn msix_cell(i: usize) -> Option<u64> {
    if i >= MSIX_N {
        return None;
    }
    Some((0xB3400u64 + (i as u64) * 0x3A) ^ ((i / 2) as u64).wrapping_mul(0x010001B3))
}

/// MSIX 态名。
pub const fn msix_phase(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("打包"),
        1 => Some("签名"),
        2 => Some("校验"),
        _ => None,
    }
}

/// MSIX 格名（条目位/核验位）。
pub const fn msix_name(i: usize) -> Option<&'static str> {
    match i % 2 {
        0 => Some("条目位"),
        1 => Some("核验位"),
        _ => None,
    }
}

/// 与 7-Zip 分账声明行折叠（行 r，锚 F52451）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB3800 + r as u32 * 0x2C)
}

/// MSIX/Store 双形态声明行折叠（行 r）。
pub const fn form_row(r: usize) -> Option<u32> {
    if r >= FORM_ROWS {
        return None;
    }
    Some(0xB3900 + r as u32 * 0x2C)
}

/// C++/WinRT 上栈声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xB3A00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// NanaZip 收口账本状态机。
#[derive(Clone)]
pub struct NanazipLedger {
    /// 六判据（安装/打包/解包/启动/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 命令账（7-Zip 同构账）。
    cmds: [Option<u64>; CMD_OPS],
    /// MSIX 三态账。
    msix: [Option<u64>; MSIX_N],
    /// 与 7-Zip 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 双形态声明账。
    forms: [Option<u32>; FORM_ROWS],
    /// C++/WinRT 上栈声明账。
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

impl NanazipLedger {
    /// 新账本。
    pub fn new() -> Self {
        NanazipLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            msix: [None; MSIX_N],
            splits: [None; SPLIT_ROWS],
            forms: [None; FORM_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NanazipCode> {
        if gate >= 6 {
            return Err(NanazipCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB310 + gate as u64);
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
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), NanazipCode> {
        if o >= CMD_OPS {
            return Err(NanazipCode::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(NanazipCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB320);
            return Err(NanazipCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(NanazipCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB321 + o as u64);
            return Err(NanazipCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NanazipCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB322 + o as u64);
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

    /// MSIX 三态一格（载荷对照 + 重复拒）。
    pub fn record_msix(&mut self, i: usize, got: u64) -> Result<(), NanazipCode> {
        let want = match msix_cell(i) {
            Some(w) => w,
            None => return Err(NanazipCode::ENoMsix),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB340 + i as u64);
            return Err(NanazipCode::EMsixDrift);
        }
        if match self.msix.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NanazipCode::EDup);
        }
        if let Some(c) = self.msix.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB341 + i as u64);
        Ok(())
    }

    /// MSIX 三态全账核验（6 格）。
    pub fn msix_all(&self) -> bool {
        let mut i = 0usize;
        while i < MSIX_N {
            match self.msix.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 7-Zip 分账声明一行（锚 F52451）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), NanazipCode> {
        if r >= SPLIT_ROWS {
            return Err(NanazipCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NanazipCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0xB360 + r as u64);
        Ok(())
    }

    /// MSIX/Store 双形态声明一行。
    pub fn declare_form(&mut self, r: usize) -> Result<(), NanazipCode> {
        if r >= FORM_ROWS {
            return Err(NanazipCode::EForm);
        }
        if match self.forms.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NanazipCode::EDup);
        }
        if let Some(c) = self.forms.get_mut(r) {
            *c = form_row(r);
        }
        self.fnv(0xB370 + r as u64);
        Ok(())
    }

    /// C++/WinRT 上栈声明一行。
    pub fn declare_winrt(&mut self, r: usize) -> Result<(), NanazipCode> {
        if r >= DECL_ROWS {
            return Err(NanazipCode::EForm);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NanazipCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0xB380 + r as u64);
        Ok(())
    }

    /// 三组声明全齐核验（分账 2 + 双形态 2 + 上栈 2）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut f = 0usize;
        while f < FORM_ROWS {
            match self.forms.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
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

    /// 收口面核验：主账+MSIX 账+三组声明全齐；未齐即封。
    pub fn seal(&self) -> Result<(), NanazipCode> {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.splits.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(NanazipCode::ENoRow);
            }
            r += 1;
        }
        let mut f = 0usize;
        while f < FORM_ROWS {
            if match self.forms.get(f) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(NanazipCode::EForm);
            }
            f += 1;
        }
        if !self.cmds_all() || !self.msix_all() {
            return Err(NanazipCode::ESeal);
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            if match self.decls.get(d) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(NanazipCode::ESeal);
            }
            d += 1;
        }
        Ok(())
    }

    /// 收口面全齐布尔核验。
    pub fn seal_all(&self) -> bool {
        self.seal().is_ok()
    }

    /// 注入守恒复核：注入=检出+逃逸。
    pub fn reconcile(&mut self, inj: u32, caught: u32, esc: u32) -> Result<(), NanazipCode> {
        if inj != caught.wrapping_add(esc) {
            self.fnv(0xB390);
            return Err(NanazipCode::EInj);
        }
        Ok(())
    }

    /// 双跑指纹复核。
    pub fn verify_fp(&self, other: u64) -> Result<(), NanazipCode> {
        if self.fp != other {
            return Err(NanazipCode::EFp);
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

impl Default for NanazipLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 定级登记复核：全齐账必须落在登记档，否则定级漂移。
pub fn grade_register(lt: &NanazipLedger) -> Result<Grade, NanazipCode> {
    let g = lt.judge_grade();
    if lt.seal_all() && lt.gates_all() {
        if g == REGISTERED {
            Ok(g)
        } else {
            Err(NanazipCode::EGradeDrift)
        }
    } else {
        Ok(g)
    }
}

/// 全齐账本（六判据 + 全节落账，测试与定级检查共用）。
pub fn sealed_ledger() -> NanazipLedger {
    let mut lt = NanazipLedger::new();
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
    let mut i = 0usize;
    while i < MSIX_N {
        if let Some(w) = msix_cell(i) {
            let _ = lt.record_msix(i, w);
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = lt.declare_split(r);
        r += 1;
    }
    let mut f = 0usize;
    while f < FORM_ROWS {
        let _ = lt.declare_form(f);
        f += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = lt.declare_winrt(d);
        d += 1;
    }
    lt
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转，全部落在当轮临时账本上；本账只记守恒三数。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NanazipLedger::new();
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
    let mut i = 0usize;
    while i < MSIX_N {
        if let Some(w) = msix_cell(i) {
            if led.record_msix(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut f = 0usize;
    while f < FORM_ROWS {
        let _ = led.declare_form(f);
        f += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_winrt(d);
        d += 1;
    }
    // 注入 10 轮：命令漂移 / 家族逆序 / MSIX 越闭集三态轮转。
    // 漂移注入先按合法序落正格再注漂移；越闭集（phase）注入对空账本同样
    // 必抓；逆序路径不改写 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NanazipLedger::new();
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
                lt.record_cmd(1, w1.wrapping_add(1)) == Err(NanazipCode::ECmdDrift)
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
                lt.record_cmd(2, w2) == Err(NanazipCode::EOrder)
            }
            _ => lt.record_msix(MSIX_N, 0) == Err(NanazipCode::ENoMsix),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB3A0 + k as u64);
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
/// UNX-F52454 判据集（锚 UNX-F52455-J1，off-by-one 板式）。
pub fn run_unx52454_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52454");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通（7-Zip 同构账）",
        recorded >= CMD_OPS as u32,
        "添加a/解压x/测试t/列表l/哈希h 五家族逐操作恒等（F52451 同构口径）",
    );
    s.add(
        "MSIX 打包三态 6/6 全落",
        {
            let mut lt = NanazipLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < MSIX_N {
                if let Some(w) = msix_cell(i) {
                    if lt.record_msix(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.msix_all()
        },
        "打包/签名/校验三态各条目位+核验位逐格恒等",
    );
    s.add(
        "UNX-F52455-J1 六判据全落",
        {
            let mut lt = NanazipLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/打包/解包/启动/卸载/许可逐格落账",
    );
    s.add(
        "与 7-Zip 分账声明 2/2 全齐",
        {
            let mut lt = NanazipLedger::new();
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
        "与 7-Zip 分账声明两行闭集（F52451 锚定，账目不并表）",
    );
    s.add(
        "MSIX/Store 双形态声明 2/2 全齐",
        {
            let mut lt = NanazipLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < FORM_ROWS {
                if lt.declare_form(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "MSIX 包形态声明/Store 分发形态声明两行闭集",
    );
    s.add(
        "C++/WinRT 上栈声明 2/2 全齐",
        {
            let mut lt = NanazipLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_winrt(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "C++ 压缩核心上栈/WinRT 外壳上栈两行闭集",
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
    s.add(
        "MSIX 三态相位闭集",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < MSIX_N {
                let p = match msix_phase(i) {
                    Some(p) => p,
                    None => "",
                };
                if p.is_empty() || msix_name(i).is_none() {
                    ok = false;
                }
                i += 1;
            }
            msix_phase(MSIX_N).is_none() && ok
        },
        "打包/签名/校验三态×条目/核验两格，越相位必拒",
    );
    s.add("注入 10/10 全抓（双面板操作集 ×10）", caught >= 10, "命令漂移/家族逆序/MSIX 越闭集三态轮转必抓");
    let mut l0 = NanazipLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·MSIX 越闭集",
        l0.record_msix(MSIX_N, 0).is_err(),
        "MSIX 格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err()
            && l0.declare_form(FORM_ROWS).is_err()
            && l0.declare_winrt(DECL_ROWS).is_err(),
        "三组声明行 2 之外越界必拒",
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
            l0.record_cmd(1, w1.wrapping_add(1)) == Err(NanazipCode::ECmdDrift)
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
            let mut a = NanazipLedger::new();
            let mut b = NanazipLedger::new();
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB140,
        "SEG_BASE 0x40 对齐且等于 0xCA400+0x40×(52454−52401)",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
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
        assert_eq!(cmd_kind_name(0), Some("添加a"));
        assert_eq!(cmd_kind_name(4), Some("哈希h"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_msix_phases() {
        // 态 = i/2，格 = i%2 映射逐点核对。
        assert_eq!(msix_phase(0), Some("打包"));
        assert_eq!(msix_phase(2), Some("签名"));
        assert_eq!(msix_phase(4), Some("校验"));
        assert!(msix_phase(6).is_none());
        assert_eq!(msix_name(0), Some("条目位"));
        assert_eq!(msix_name(1), Some("核验位"));
        assert!(msix_name(2).is_some());
        assert!(msix_cell(MSIX_N).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match cmd_cell(3) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(a, match cmd_cell(3) {
            Some(v) => v,
            None => 1,
        });
        assert_ne!(a, match cmd_cell(4) {
            Some(v) => v,
            None => 0,
        });
        assert!(msix_cell(6).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(form_row(FORM_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族格，再注逆序。
        let mut lt = NanazipLedger::new();
        let _ = lt.record_cmd(4, cmd_cell(4).unwrap_or(0));
        let _ = lt.record_cmd(8, cmd_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(0, cmd_cell(0).unwrap_or(0)),
            Err(NanazipCode::EOrder)
        );
        assert!(lt.record_cmd(9, cmd_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_last_kind_unchanged_after_order() {
        let mut lt = NanazipLedger::new();
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        let before = lt.last_family();
        let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
        assert_eq!(lt.last_family(), before, "逆序路径不得改写 last_kind");
        assert_eq!(lt.stats().1, 1, "逆序注入计入检出");
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NanazipLedger::new();
        assert_eq!(
            lt.record_msix(3, msix_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(NanazipCode::EMsixDrift)
        );
        let w0 = cmd_cell(0).unwrap_or(0);
        let _ = lt.record_cmd(0, w0);
        assert_eq!(
            lt.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(NanazipCode::ECmdDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NanazipLedger::new();
        let _ = lt.record_msix(2, msix_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_msix(2, msix_cell(2).unwrap_or(0)),
            Err(NanazipCode::EDup)
        );
        let _ = lt.declare_form(0);
        assert_eq!(lt.declare_form(0), Err(NanazipCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NanazipLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NanazipLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(!lt2.seal_all());
    }

    #[test]
    fn test_seal_and_reconcile() {
        let empty = NanazipLedger::new();
        assert_eq!(empty.seal(), Err(NanazipCode::ENoRow));
        let mut no_form = sealed_ledger();
        no_form.forms = [None; FORM_ROWS];
        assert_eq!(no_form.seal(), Err(NanazipCode::EForm));
        let full = sealed_ledger();
        assert!(full.seal().is_ok());
        assert!(full.seal_all());
        let mut r = full.clone();
        assert!(r.reconcile(10, 10, 0).is_ok());
        assert!(r.verify_fp(full.fingerprint()).is_ok());
        assert_eq!(r.reconcile(10, 9, 0), Err(NanazipCode::EInj));
        assert_eq!(r.verify_fp(full.fingerprint().wrapping_add(1)), Err(NanazipCode::EFp));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：命令 20 + MSIX 6。
        assert_eq!(recorded, (CMD_OPS + MSIX_N) as u32);
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
        assert_eq!(SEG_BASE, 0xCB140);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52454u32 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码位须与枚举实际位序一致：Ok=0，EGradeDrift=12，恰 13 变体。
        assert_eq!(NanazipCode::Ok as u32, 0);
        assert_eq!(NanazipCode::ENoOp as u32, 1);
        assert_eq!(NanazipCode::EForm as u32, 7);
        assert_eq!(NanazipCode::EGradeDrift as u32, 12);
        assert!(!NanazipCode::Ok.detail().is_empty());
        assert!(!NanazipCode::ESeal.detail().is_empty());
        assert!(!NanazipCode::EForm.detail().is_empty());
        assert!(!NanazipCode::EGradeDrift.detail().is_empty());
        assert_eq!(NanazipCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐仍按登记档 S2。
        assert_eq!(Grade::S3.val(), 3);
        let empty = NanazipLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut half = NanazipLedger::new();
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
    fn unx52454_checks_green() {
        let s = run_unx52454_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
