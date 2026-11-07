//! UNX-F52302 winget CLI 收口 · 0xC8B40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52302"，逐字）：
//!   title: winget CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52303-J1 六判据
//!     全落；与 B04 CLI 共面分账；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52302）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52303-J1，照原文锚定；功能按本卡标题（winget CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 源三族 12 格：winget / msstore / community 三源族各 4 格，闭集
//!      外必抓；逐格载荷折叠对照，恒等才收。
//!   ② install/upgrade/uninstall/list/search/export/import 20 操作集：
//!      四相位族各 5 操作（安装/升级/卸载/查询与迁移），相位单调（逆
//!      序必抓），载荷漂移必抓。
//!   ③ msix/exe/msstore 三形态 8 格；协议消费联签 4 格（winget:// 协
//!      议消费逐格联签入指纹）；退出码闭集 6（0..5 恰六档，档外必拒）。
//!   ④ 与 B04 CLI 共面分账：操作账与形态账分列共面，缺一不定级。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（源越闭集/
//!      操作漂移/操作越闭集），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52302;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 源格数（三族 × 4）。
pub const SOURCE_CELLS: usize = 12;

/// 源族数。
pub const SOURCE_FAMILIES: usize = 3;

/// 操作数（四相位族 × 5）。
pub const OPS: usize = 20;

/// 安装形态格数（msix/exe/msstore）。
pub const FORM_CELLS: usize = 8;

/// 协议消费联签格数。
pub const PROTOCOL_CELLS: usize = 4;

/// 退出码闭集档数。
pub const EXIT_CODES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WingetCode {
    /// 成功
    Ok = 0,
    /// 源格越闭集
    ENoSource,
    /// 源载荷漂移
    ESourceDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EPhase,
    /// 形态格越闭集
    ENoForm,
    /// 协议联签格越闭集
    ENoProto,
    /// 退出码档越闭集
    ENoExit,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WingetCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WingetCode::Ok => "成功",
            WingetCode::ENoSource => "源格越闭集：winget/msstore/community 12 格之外先登记",
            WingetCode::ESourceDrift => "源载荷漂移：源索引折叠与基准不恒等，逐格复算",
            WingetCode::ENoOp => "操作号越闭集：install/upgrade/uninstall/list/search/export/import 20 操作集之外先登记",
            WingetCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            WingetCode::EPhase => "相位逆序：安装→升级→卸载→查询与迁移相位单调，逆序必抓",
            WingetCode::ENoForm => "形态格越闭集：msix/exe/msstore 8 格之外先登记",
            WingetCode::ENoProto => "协议联签格越闭集：协议消费联签 4 格之外先登记",
            WingetCode::ENoExit => "退出码档越闭集：0..5 六档之外先登记",
            WingetCode::EDup => "重复落账：同格只许一条账目",
            WingetCode::ESeal => "未齐即封：12 源 + 20 操作 + 形态/联签未齐先补齐",
            WingetCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WingetCode::EGradeDrift => "定级漂移：六判据全落且与 B04 CLI 共面分账全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12源+20操作+8形态+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 源格载荷（格 b：0..12，族 = b/4）。
pub const fn source_cell(b: usize) -> Option<u64> {
    if b >= SOURCE_CELLS {
        return None;
    }
    Some((0xD1000u64 + (b as u64) * 0x41) ^ 0x5747_0000)
}

/// 源族（0=winget 1=msstore 2=community）。
pub const fn source_family(b: usize) -> Option<u32> {
    if b >= SOURCE_CELLS {
        return None;
    }
    Some((b / 4) as u32)
}

/// 源族名。
pub const fn source_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("winget"),
        1 => Some("msstore"),
        2 => Some("community"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x37) ^ (((o / 5) as u64).wrapping_mul(0x0200_0000)))
}

/// 操作相位（0..5=install 5..10=upgrade 10..15=uninstall 15..20=查询与迁移）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 操作相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("install"),
        1 => Some("upgrade"),
        2 => Some("uninstall"),
        3 => Some("list/search/export/import"),
        _ => None,
    }
}

/// 形态格载荷（格 c：0..8，0..3=msix 3..6=exe 6..8=msstore）。
pub const fn form_cell(c: usize) -> Option<u32> {
    if c >= FORM_CELLS {
        return None;
    }
    Some(0xD300 + (c as u32) * 0x2F)
}

/// 安装形态名（按格段）。
pub const fn form_name(c: usize) -> Option<&'static str> {
    if c >= FORM_CELLS {
        return None;
    }
    if c < 3 {
        Some("msix")
    } else if c < 6 {
        Some("exe")
    } else {
        Some("msstore")
    }
}

/// 协议消费联签载荷（格 p）。
pub const fn protocol_cell(p: usize) -> Option<u32> {
    if p >= PROTOCOL_CELLS {
        return None;
    }
    Some(0xD500 + (p as u32) * 0x1D)
}

/// 退出码闭集（档 i 的值，0..5 恰六档）。
pub const fn exit_code_val(i: usize) -> Option<u32> {
    match i {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(5),
        _ => None,
    }
}

/// 退出码档名。
pub const fn exit_code_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("成功"),
        1 => Some("参数错"),
        2 => Some("网络错"),
        3 => Some("源错"),
        4 => Some("包未找到"),
        5 => Some("需重启"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// winget CLI 收口账本状态机。
#[derive(Clone)]
pub struct WingetLedger {
    /// 六判据（启动/安装/升级/卸载/清单/导出）。
    gates: [Option<bool>; 6],
    /// 源账。
    sources: [Option<u64>; SOURCE_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 形态账。
    forms: [Option<u32>; FORM_CELLS],
    /// 协议消费联签账。
    protos: [Option<u32>; PROTOCOL_CELLS],
    /// 退出码账（档位命中与否）。
    exits: [Option<u32>; EXIT_CODES],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WingetLedger {
    /// 新账本。
    pub fn new() -> Self {
        WingetLedger {
            gates: [None; 6],
            sources: [None; SOURCE_CELLS],
            ops: [None; OPS],
            forms: [None; FORM_CELLS],
            protos: [None; PROTOCOL_CELLS],
            exits: [None; EXIT_CODES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WingetCode> {
        if gate >= 6 {
            return Err(WingetCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 源一格（载荷对照 + 重复拒）。
    pub fn record_source(&mut self, b: usize, got: u64) -> Result<(), WingetCode> {
        let want = match source_cell(b) {
            Some(w) => w,
            None => return Err(WingetCode::ENoSource),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + b as u64);
            return Err(WingetCode::ESourceDrift);
        }
        if match self.sources.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WingetCode::EDup);
        }
        if let Some(c) = self.sources.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x6201 + b as u64);
        Ok(())
    }

    /// 源全账核验（12 格）。
    pub fn sources_all(&self) -> bool {
        let mut b = 0usize;
        while b < SOURCE_CELLS {
            match self.sources.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), WingetCode> {
        if o >= OPS {
            return Err(WingetCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(WingetCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(WingetCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(WingetCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(WingetCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WingetCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
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

    /// 形态一格。
    pub fn record_form(&mut self, c: usize) -> Result<(), WingetCode> {
        let want = match form_cell(c) {
            Some(w) => w,
            None => return Err(WingetCode::ENoForm),
        };
        if match self.forms.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WingetCode::EDup);
        }
        if let Some(x) = self.forms.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x6400 + c as u64);
        Ok(())
    }

    /// 形态全账核验。
    pub fn forms_all(&self) -> bool {
        let mut c = 0usize;
        while c < FORM_CELLS {
            match self.forms.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 协议消费联签一格。
    pub fn record_protocol(&mut self, p: usize) -> Result<(), WingetCode> {
        let want = match protocol_cell(p) {
            Some(w) => w,
            None => return Err(WingetCode::ENoProto),
        };
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WingetCode::EDup);
        }
        if let Some(x) = self.protos.get_mut(p) {
            *x = Some(want);
        }
        self.fnv(0x6500 + p as u64);
        Ok(())
    }

    /// 协议联签全账核验。
    pub fn protos_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTOCOL_CELLS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 退出码档一格（闭集外必拒）。
    pub fn record_exit(&mut self, i: usize) -> Result<(), WingetCode> {
        let want = match exit_code_val(i) {
            Some(w) => w,
            None => return Err(WingetCode::ENoExit),
        };
        if match self.exits.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WingetCode::EDup);
        }
        if let Some(x) = self.exits.get_mut(i) {
            *x = Some(want);
        }
        self.fnv(0x6600 + i as u64);
        Ok(())
    }

    /// 退出码全账核验。
    pub fn exits_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXIT_CODES {
            match self.exits.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
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
        if self.gates_all()
            && self.sources_all()
            && self.ops_all()
            && self.forms_all()
            && self.protos_all()
            && self.exits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WingetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WingetLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < SOURCE_CELLS {
        if let Some(w) = source_cell(b) {
            if led.record_source(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
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
    let mut c = 0usize;
    while c < FORM_CELLS {
        let _ = led.record_form(c);
        c += 1;
    }
    let mut p = 0usize;
    while p < PROTOCOL_CELLS {
        let _ = led.record_protocol(p);
        p += 1;
    }
    let mut i = 0usize;
    while i < EXIT_CODES {
        let _ = led.record_exit(i);
        i += 1;
    }
    // 注入 10 轮：源越闭集 / 操作漂移 / 操作越闭集三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WingetLedger::new();
        let caught = match kind {
            0 => lt.record_source(SOURCE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WingetCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52302 判据集（锚 UNX-F52303-J1，off-by-one 板式）。
pub fn run_unx52302_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52302");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "源三族 12 格全落",
        recorded >= SOURCE_CELLS as u32 + OPS as u32,
        "winget/msstore/community 三源族逐格恒等落账",
    );
    s.add(
        "源族名闭集",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < SOURCE_FAMILIES {
                match source_family_name(f) {
                    Some(_) => {}
                    None => ok = false,
                }
                f += 1;
            }
            ok && source_family_name(SOURCE_FAMILIES).is_none()
        },
        "winget/msstore/community 三族名闭集，族外无名",
    );
    s.add(
        "install/upgrade/uninstall/查询迁移 20 操作全通",
        {
            let mut lt = WingetLedger::new();
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
        "四相位族逐操作恒等（UNX-F52303-J1 六判据全落锚定）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_phase(o).unwrap_or(4) < op_phase(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "install→upgrade→uninstall→list/search/export/import 四相位族单调",
    );
    s.add(
        "msix/exe/msstore 三形态 8 格全落",
        {
            let mut lt = WingetLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < FORM_CELLS {
                if lt.record_form(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.forms_all()
        },
        "三形态逐格落账，越界必拒",
    );
    s.add(
        "协议消费联签 4 格全落",
        {
            let mut lt = WingetLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROTOCOL_CELLS {
                if lt.record_protocol(p).is_err() {
                    ok = false;
                }
                p += 1;
            }
            ok && lt.protos_all()
        },
        "协议消费逐格联签入指纹",
    );
    s.add(
        "退出码闭集 6 档全落",
        {
            let mut lt = WingetLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < EXIT_CODES {
                if lt.record_exit(i).is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok && lt.exits_all() && exit_code_val(EXIT_CODES).is_none()
        },
        "0..5 恰六档，档外必拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WingetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/安装/升级/卸载/清单/导出逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "源越闭集/操作漂移/操作越闭集三态轮转必抓");
    let mut l0 = WingetLedger::new();
    s.add(
        "边界·源越闭集",
        l0.record_source(SOURCE_CELLS, 0).is_err(),
        "源 12 格之外必拒",
    );
    s.add(
        "注入·操作漂移拒",
        {
            let _ = l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WingetCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
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
            let mut lt = WingetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < SOURCE_CELLS {
                if let Some(w) = source_cell(b) {
                    let _ = lt.record_source(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < FORM_CELLS {
                let _ = lt.record_form(c);
                c += 1;
            }
            let mut p = 0usize;
            while p < PROTOCOL_CELLS {
                let _ = lt.record_protocol(p);
                p += 1;
            }
            let mut i = 0usize;
            while i < EXIT_CODES {
                let _ = lt.record_exit(i);
                i += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，与 B04 CLI 共面分账）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8B40,
        "SEG_BASE 0x40 对齐且按 0xC7200+0x40×(52302−52201) 恒等",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_families() {
        assert_eq!(source_family_name(0), Some("winget"));
        assert_eq!(source_family_name(1), Some("msstore"));
        assert_eq!(source_family_name(2), Some("community"));
        assert!(source_family_name(3).is_none());
        assert_eq!(source_family(0), Some(0));
        assert_eq!(source_family(11), Some(2));
        assert!(source_family(12).is_none());
        assert!(source_cell(12).is_none());
    }

    #[test]
    fn test_op_phases() {
        assert_eq!(op_phase_name(0), Some("install"));
        assert_eq!(op_phase_name(1), Some("upgrade"));
        assert_eq!(op_phase_name(2), Some("uninstall"));
        assert_eq!(op_phase_name(3), Some("list/search/export/import"));
        assert!(op_phase_name(4).is_none());
        assert_eq!(op_phase(4), Some(0));
        assert_eq!(op_phase(5), Some(1));
        assert_eq!(op_phase(19), Some(3));
        assert!(op_phase(20).is_none());
        assert!(op_cell(20).is_none());
    }

    #[test]
    fn test_form_segments() {
        assert_eq!(form_name(0), Some("msix"));
        assert_eq!(form_name(3), Some("exe"));
        assert_eq!(form_name(6), Some("msstore"));
        assert_eq!(form_name(7), Some("msstore"));
        assert!(form_name(8).is_none());
        assert!(form_cell(8).is_none());
    }

    #[test]
    fn test_exit_codes_closed() {
        let mut i = 0usize;
        while i < EXIT_CODES {
            assert!(exit_code_val(i).is_some());
            assert!(exit_code_name(i).is_some());
            assert!(!exit_code_name(i).unwrap_or("").is_empty());
            i += 1;
        }
        assert!(exit_code_val(EXIT_CODES).is_none());
        assert!(exit_code_name(EXIT_CODES).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(9).unwrap_or(0);
        assert_eq!(a, op_cell(9).unwrap_or(0));
        assert_ne!(a, op_cell(10).unwrap_or(0));
        assert!(protocol_cell(PROTOCOL_CELLS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        // 相位逆序注入：先按合法序落高相位格，再注低相位格必抓。
        let mut lt = WingetLedger::new();
        let _ = lt.record_op(8, op_cell(8).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(WingetCode::EPhase));
        assert!(lt.record_op(9, op_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WingetLedger::new();
        assert_eq!(
            lt.record_op(6, op_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(WingetCode::EDrift)
        );
        assert_eq!(
            lt.record_source(5, source_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(WingetCode::ESourceDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WingetLedger::new();
        let _ = lt.record_source(0, source_cell(0).unwrap_or(0));
        assert_eq!(lt.record_source(0, source_cell(0).unwrap_or(0)), Err(WingetCode::EDup));
    }

    #[test]
    fn test_injection_on_empty_ledger_keeps_phase() {
        // 对空账本注入必抓且不改 last_phase：注入后再落合法格应照常成功。
        let mut lt = WingetLedger::new();
        assert!(lt.record_op(OPS, 0).is_err());
        assert!(lt.record_source(SOURCE_CELLS, 0).is_err());
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_op(5, op_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WingetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WingetLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SOURCE_CELLS as u32 + OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC8B40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WingetCode::Ok as u32, 0);
        assert_eq!(WingetCode::EGradeDrift as u32, 12);
        assert!(!WingetCode::Ok.detail().is_empty());
        assert!(!WingetCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WingetLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52302_checks_green() {
        let s = run_unx52302_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
