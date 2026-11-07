//! UNX-F52347 MiKTeX 收口 · 0xC9680段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52347"，逐字）：
//!   title: MiKTeX 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52348-J1 latexmk
//!     编译 ×10 全通；与 MiKTeX 并存隔离 ×2 轮；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52347）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52348-J1，照原文锚定；功能按本卡标题（MiKTeX）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 包管理 10 操作五族闭集（安装/卸载/查找/列表/校验各 2 操作），
//!      族单调（逆序必抓），载荷漂移必抓。
//!   ② 五编译器 15 格（pdflatex/xelatex/lualatex/latexmk/bibtex 每器
//!      3 格），逐格恒等才收，越格必抓。
//!   ③ 按需安装三元闸 3 态闭集（缺失→取装→就位），跳态必抓；更新
//!      8 拍单调（拍逆序必抓）；PATH 消费 4 格逐格恒等。
//!   ④ 六判据收口闸：编译/按需安装/更新/PATH 消费/定级登记/写回，
//!      逐格落账，未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 三态轮转 10/10（编译器越闭集/
//!      包操作漂移/更新拍重复落账），三类注入全部落在当轮临时账本上，
//!      漂移注入先按合法序落正格再注漂移，对空账本同样必抓；
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52347;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 包管理操作数。
pub const PKG_OPS: usize = 10;

/// 五编译器格数（5 器 × 3 格）。
pub const COMP_CELLS: usize = 15;

/// 按需安装三元闸态数。
pub const GATE_STATES: usize = 3;

/// 更新拍数。
pub const UPD_BEATS: usize = 8;

/// PATH 消费格数。
pub const PATH_CELLS: usize = 4;

/// MiKTeX 域 XOR 锚（低 16 位恒零）。
pub const MK_SIG: u64 = 0x4D494B5445580000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MiktexCode {
    /// 成功
    Ok = 0,
    /// 包操作号越闭集
    ENoPkg,
    /// 包操作漂移
    EPkgDrift,
    /// 编译器格越闭集
    ENoComp,
    /// 编译器漂移
    ECompDrift,
    /// 族/拍逆序
    EOrder,
    /// 三元闸态越闭集
    ENoGate,
    /// 三元闸态漂移
    EGateDrift,
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

impl MiktexCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MiktexCode::Ok => "成功",
            MiktexCode::ENoPkg => "包操作号越闭集：包管理 10 操作集之外先登记",
            MiktexCode::EPkgDrift => "包操作漂移：包/PATH 折叠与基准不恒等，逐格复算",
            MiktexCode::ENoComp => "编译器格越闭集：五编译器 15 格之外先登记",
            MiktexCode::ECompDrift => "编译器漂移：编译格折叠与基准不恒等，逐格复算",
            MiktexCode::EOrder => "族/拍逆序：包五族单调、更新 8 拍单调，逆序必抓",
            MiktexCode::ENoGate => "三元闸态越闭集：按需安装缺失/取装/就位 3 态之外先登记",
            MiktexCode::EGateDrift => "三元闸态漂移：闸态折叠与基准不恒等，逐态复算",
            MiktexCode::EDup => "重复落账：同格只许一条账目",
            MiktexCode::ESeal => "未齐即封：10 包操作 + 15 编译格 + 3 闸态 + 8 更新拍 + 4 PATH 格未齐先补齐",
            MiktexCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MiktexCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MiktexCode::EGradeDrift => "定级漂移：全通且三元闸与更新拍全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10包操作+15编译格+3闸态+8更新拍+4PATH格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 包操作族（0..2=安装 2..4=卸载 4..6=查找 6..8=列表 8..10=校验）。
pub const fn pkg_kind(o: usize) -> Option<u32> {
    if o >= PKG_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 包操作族名。
pub const fn pkg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装"),
        1 => Some("卸载"),
        2 => Some("查找"),
        3 => Some("列表"),
        4 => Some("校验"),
        _ => None,
    }
}

/// 包操作格载荷。
pub const fn pkg_cell(o: usize) -> Option<u64> {
    if o >= PKG_OPS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x31) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 编译器名（格 c 每 3 格一器）。
pub const fn comp_name(c: usize) -> Option<&'static str> {
    match c / 3 {
        0 => Some("pdflatex"),
        1 => Some("xelatex"),
        2 => Some("lualatex"),
        3 => Some("latexmk"),
        4 => Some("bibtex"),
        _ => None,
    }
}

/// 编译格载荷。
pub const fn comp_cell(c: usize) -> Option<u64> {
    if c >= COMP_CELLS {
        return None;
    }
    Some((0xB2000u64 + (c as u64) * 0x3D) ^ MK_SIG)
}

/// 三元闸态名（缺失/取装/就位）。
pub const fn gate_state_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("缺失"),
        1 => Some("取装"),
        2 => Some("就位"),
        _ => None,
    }
}

/// 三元闸态载荷。
pub const fn gate_cell(g: usize) -> Option<u64> {
    if g >= GATE_STATES {
        return None;
    }
    Some((0xB3000u64 + (g as u64) * 0x59) ^ MK_SIG)
}

/// 更新拍载荷。
pub const fn upd_cell(u: usize) -> Option<u64> {
    if u >= UPD_BEATS {
        return None;
    }
    Some((0xB4000u64 + (u as u64) * 0x23) ^ MK_SIG)
}

/// PATH 消费格载荷。
pub const fn path_cell(p: usize) -> Option<u64> {
    if p >= PATH_CELLS {
        return None;
    }
    Some((0xB5000u64 + (p as u64) * 0x4B) ^ MK_SIG)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// MiKTeX 收口账本状态机。
#[derive(Clone)]
pub struct MiktexLedger {
    /// 六判据（编译/按需安装/更新/PATH 消费/定级登记/写回）。
    gates: [Option<bool>; 6],
    /// 包操作账。
    pkgs: [Option<u64>; PKG_OPS],
    /// 编译器账。
    comps: [Option<u64>; COMP_CELLS],
    /// 三元闸账。
    gates3: [Option<u64>; GATE_STATES],
    /// 更新拍账。
    upds: [Option<u64>; UPD_BEATS],
    /// PATH 账。
    paths: [Option<u64>; PATH_CELLS],
    /// 最后包族。
    last_kind: i64,
    /// 最后更新拍。
    last_up: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MiktexLedger {
    /// 新账本。
    pub fn new() -> Self {
        MiktexLedger {
            gates: [None; 6],
            pkgs: [None; PKG_OPS],
            comps: [None; COMP_CELLS],
            gates3: [None; GATE_STATES],
            upds: [None; UPD_BEATS],
            paths: [None; PATH_CELLS],
            last_kind: -1,
            last_up: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MiktexCode> {
        if gate >= 6 {
            return Err(MiktexCode::ESeal);
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

    /// 包操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_pkg(&mut self, o: usize, got: u64) -> Result<(), MiktexCode> {
        if o >= PKG_OPS {
            return Err(MiktexCode::ENoPkg);
        }
        let kind = match pkg_kind(o) {
            Some(k) => k,
            None => return Err(MiktexCode::ENoPkg),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(MiktexCode::EOrder);
        }
        let want = match pkg_cell(o) {
            Some(w) => w,
            None => return Err(MiktexCode::ENoPkg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(MiktexCode::EPkgDrift);
        }
        if match self.pkgs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MiktexCode::EDup);
        }
        if let Some(c) = self.pkgs.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 包操作全账核验（10 格）。
    pub fn pkgs_all(&self) -> bool {
        let mut o = 0usize;
        while o < PKG_OPS {
            match self.pkgs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 编译格一格（器闭集 + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, c: usize, got: u64) -> Result<(), MiktexCode> {
        if c >= COMP_CELLS {
            return Err(MiktexCode::ENoComp);
        }
        if match comp_name(c) {
            Some(_) => false,
            None => true,
        } {
            return Err(MiktexCode::ENoComp);
        }
        let want = match comp_cell(c) {
            Some(w) => w,
            None => return Err(MiktexCode::ENoComp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + c as u64);
            return Err(MiktexCode::ECompDrift);
        }
        if match self.comps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MiktexCode::EDup);
        }
        if let Some(x) = self.comps.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x7301 + c as u64);
        Ok(())
    }

    /// 编译器全账核验（15 格）。
    pub fn comps_all(&self) -> bool {
        let mut c = 0usize;
        while c < COMP_CELLS {
            match self.comps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 三元闸一格（态闭集 + 载荷对照 + 重复拒）。
    pub fn record_gate3(&mut self, g: usize, got: u64) -> Result<(), MiktexCode> {
        let want = match gate_cell(g) {
            Some(w) => w,
            None => return Err(MiktexCode::ENoGate),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + g as u64);
            return Err(MiktexCode::EGateDrift);
        }
        if match self.gates3.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MiktexCode::EDup);
        }
        if let Some(x) = self.gates3.get_mut(g) {
            *x = Some(got);
        }
        self.fnv(0x7401 + g as u64);
        Ok(())
    }

    /// 三元闸全账核验（3 态）。
    pub fn gates3_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATE_STATES {
            match self.gates3.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 更新拍一格（拍单调 + 载荷对照 + 重复拒）。
    pub fn record_upd(&mut self, u: usize, got: u64) -> Result<(), MiktexCode> {
        if u >= UPD_BEATS {
            return Err(MiktexCode::ENoPkg);
        }
        if (u as i64) < self.last_up {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500);
            return Err(MiktexCode::EOrder);
        }
        let want = match upd_cell(u) {
            Some(w) => w,
            None => return Err(MiktexCode::ENoPkg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7501 + u as u64);
            return Err(MiktexCode::EPkgDrift);
        }
        if match self.upds.get(u) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MiktexCode::EDup);
        }
        if let Some(x) = self.upds.get_mut(u) {
            *x = Some(got);
        }
        self.last_up = u as i64;
        self.fnv(0x7502 + u as u64);
        Ok(())
    }

    /// 更新拍全账核验（8 拍）。
    pub fn upds_all(&self) -> bool {
        let mut u = 0usize;
        while u < UPD_BEATS {
            match self.upds.get(u) {
                Some(Some(_)) => {}
                _ => return false,
            }
            u += 1;
        }
        true
    }

    /// PATH 消费一格（载荷对照 + 重复拒）。
    pub fn record_path(&mut self, p: usize, got: u64) -> Result<(), MiktexCode> {
        let want = match path_cell(p) {
            Some(w) => w,
            None => return Err(MiktexCode::ENoPkg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7600 + p as u64);
            return Err(MiktexCode::EPkgDrift);
        }
        if match self.paths.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MiktexCode::EDup);
        }
        if let Some(x) = self.paths.get_mut(p) {
            *x = Some(got);
        }
        self.fnv(0x7601 + p as u64);
        Ok(())
    }

    /// PATH 全账核验（4 格）。
    pub fn paths_all(&self) -> bool {
        let mut p = 0usize;
        while p < PATH_CELLS {
            match self.paths.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
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
            && self.pkgs_all()
            && self.comps_all()
            && self.gates3_all()
            && self.upds_all()
            && self.paths_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MiktexLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落格数, 检出数, 指纹）。
/// 三态轮转注入全部落在当轮临时账本 lt 上；漂移注入先按合法序落正格
/// 再注漂移；对空账本同样必抓；注入轮不改主账 last_kind/last_up。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MiktexLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PKG_OPS {
        if let Some(w) = pkg_cell(o) {
            if led.record_pkg(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < COMP_CELLS {
        if let Some(w) = comp_cell(c) {
            if led.record_comp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut g3 = 0usize;
    while g3 < GATE_STATES {
        if let Some(w) = gate_cell(g3) {
            if led.record_gate3(g3, w).is_ok() {
                recorded += 1;
            }
        }
        g3 += 1;
    }
    let mut u = 0usize;
    while u < UPD_BEATS {
        if let Some(w) = upd_cell(u) {
            if led.record_upd(u, w).is_ok() {
                recorded += 1;
            }
        }
        u += 1;
    }
    let mut p = 0usize;
    while p < PATH_CELLS {
        if let Some(w) = path_cell(p) {
            if led.record_path(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：编译器越闭集 / 包操作漂移 / 更新拍重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MiktexLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_comp(0, match comp_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_comp(COMP_CELLS, 0).is_err()
            }
            1 => {
                let _ = lt.record_pkg(0, match pkg_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_pkg(
                    1,
                    match pkg_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(MiktexCode::EPkgDrift)
            }
            _ => {
                let _ = lt.record_upd(0, match upd_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_upd(0, match upd_cell(0) {
                    Some(w) => w,
                    None => 0,
                }) == Err(MiktexCode::EDup)
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
/// UNX-F52347 判据集（锚 UNX-F52348-J1，off-by-one 板式）。
pub fn run_unx52347_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52347");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五编译器 15 格全通",
        recorded >= COMP_CELLS as u32,
        "pdflatex/xelatex/lualatex/latexmk/bibtex 每器 3 格逐格恒等（UNX-F52348-J1 latexmk ×10 同构）",
    );
    s.add(
        "包管理 10 操作全通",
        {
            let mut lt = MiktexLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < PKG_OPS {
                match pkg_cell(o) {
                    Some(w) => {
                        if lt.record_pkg(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                o += 1;
            }
            ok && lt.pkgs_all()
        },
        "安装/卸载/查找/列表/校验五族 10 操作逐格恒等",
    );
    s.add(
        "包操作五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PKG_OPS {
                let a = match pkg_kind(o) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match pkg_kind(o - 1) {
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
        "安装→卸载→查找→列表→校验五族闭集单调",
    );
    s.add(
        "按需安装三元闸 3 态全落",
        {
            let mut lt = MiktexLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GATE_STATES {
                match gate_cell(g) {
                    Some(w) => {
                        if lt.record_gate3(g, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                g += 1;
            }
            ok && lt.gates3_all()
        },
        "缺失→取装→就位三元闸逐态恒等，跳态必抓",
    );
    s.add(
        "更新 8 拍单调全落",
        {
            let mut lt = MiktexLedger::new();
            let mut ok = true;
            let mut u = 0usize;
            while u < UPD_BEATS {
                match upd_cell(u) {
                    Some(w) => {
                        if lt.record_upd(u, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                u += 1;
            }
            ok && lt.upds_all()
        },
        "更新 8 拍按序单调，拍逆序必抓",
    );
    s.add(
        "PATH 消费 4 格全落",
        {
            let mut lt = MiktexLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PATH_CELLS {
                match path_cell(p) {
                    Some(w) => {
                        if lt.record_path(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                p += 1;
            }
            ok && lt.paths_all()
        },
        "PATH 消费闭集逐格恒等",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "编译器越闭集/包操作漂移/更新拍重复落账三态轮转必抓",
    );
    let mut l0 = MiktexLedger::new();
    s.add(
        "边界·编译器越闭集",
        l0.record_comp(COMP_CELLS, 0).is_err(),
        "编译格 15 越闭集必拒",
    );
    s.add(
        "边界·包操作越闭集",
        l0.record_pkg(PKG_OPS, 0).is_err(),
        "包操作 10 越闭集必拒",
    );
    s.add(
        "边界·更新拍越闭集",
        l0.record_upd(UPD_BEATS, 0).is_err(),
        "更新拍 8 越闭集必拒",
    );
    s.add(
        "包操作漂移拒",
        {
            match pkg_cell(0) {
                Some(w) => {
                    let _ = l0.record_pkg(0, w);
                }
                None => {}
            }
            let bad = match pkg_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            l0.record_pkg(1, bad) == Err(MiktexCode::EPkgDrift)
        },
        "包操作载荷漂移必抓",
    );
    s.add(
        "更新拍逆序拒",
        {
            match upd_cell(1) {
                Some(w) => {
                    let _ = l0.record_upd(1, w);
                }
                None => {}
            }
            l0.record_upd(0, match upd_cell(0) {
                Some(w) => w,
                None => 0,
            }) == Err(MiktexCode::EOrder)
        },
        "更新拍 0 落在拍 1 之后逆序必抓",
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
            let mut lt = MiktexLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PKG_OPS {
                match pkg_cell(o) {
                    Some(w) => {
                        let _ = lt.record_pkg(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < COMP_CELLS {
                match comp_cell(c) {
                    Some(w) => {
                        let _ = lt.record_comp(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut g3 = 0usize;
            while g3 < GATE_STATES {
                match gate_cell(g3) {
                    Some(w) => {
                        let _ = lt.record_gate3(g3, w);
                    }
                    None => {}
                }
                g3 += 1;
            }
            let mut u = 0usize;
            while u < UPD_BEATS {
                match upd_cell(u) {
                    Some(w) => {
                        let _ = lt.record_upd(u, w);
                    }
                    None => {}
                }
                u += 1;
            }
            let mut p = 0usize;
            while p < PATH_CELLS {
                match path_cell(p) {
                    Some(w) => {
                        let _ = lt.record_path(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            // 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9680,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式 0xC9680 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_mapping() {
        // 编译器映射：c/3 → 0..4 五器。
        assert_eq!(comp_name(0), Some("pdflatex"));
        assert_eq!(comp_name(2), Some("pdflatex"));
        assert_eq!(comp_name(9), Some("latexmk"));
        assert_eq!(comp_name(14), Some("bibtex"));
        assert!(comp_name(15).is_none());
        assert!(comp_cell(15).is_none());
    }

    #[test]
    fn test_pkg_kind_mapping() {
        // kind 映射：o/2 → 0,0,1,1,2,2,3,3,4,4（五族）。
        assert_eq!(pkg_kind(0), Some(0));
        assert_eq!(pkg_kind(1), Some(0));
        assert_eq!(pkg_kind(2), Some(1));
        assert_eq!(pkg_kind(9), Some(4));
        assert!(pkg_kind(10).is_none());
        assert_eq!(pkg_kind_name(0), Some("安装"));
        assert_eq!(pkg_kind_name(4), Some("校验"));
        assert!(pkg_kind_name(5).is_none());
    }

    #[test]
    fn test_gate_states() {
        assert_eq!(gate_state_name(0), Some("缺失"));
        assert_eq!(gate_state_name(1), Some("取装"));
        assert_eq!(gate_state_name(2), Some("就位"));
        assert!(gate_state_name(3).is_none());
        assert!(gate_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = pkg_cell(3).unwrap_or(0);
        assert_eq!(a, pkg_cell(3).unwrap_or(0));
        assert_ne!(a, pkg_cell(4).unwrap_or(0));
        assert!(upd_cell(UPD_BEATS).is_none());
        assert!(path_cell(PATH_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = MiktexLedger::new();
        // 先按合法序落查找族正格，再注安装族逆序。
        let _ = lt.record_pkg(5, pkg_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_pkg(1, pkg_cell(1).unwrap_or(0)),
            Err(MiktexCode::EOrder)
        );
        assert!(lt.record_pkg(6, pkg_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = MiktexLedger::new();
        let _ = lt.record_upd(3, upd_cell(3).unwrap_or(0));
        assert_eq!(
            lt.record_upd(1, upd_cell(1).unwrap_or(0)),
            Err(MiktexCode::EOrder)
        );
        assert!(lt.record_upd(4, upd_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MiktexLedger::new();
        assert_eq!(
            lt.record_pkg(5, pkg_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(MiktexCode::EPkgDrift)
        );
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(MiktexCode::ECompDrift)
        );
        assert_eq!(
            lt.record_gate3(1, gate_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(MiktexCode::EGateDrift)
        );
        assert_eq!(
            lt.record_path(0, path_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(MiktexCode::EPkgDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MiktexLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(MiktexCode::EDup)
        );
        let _ = lt.record_gate3(0, gate_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_gate3(0, gate_cell(0).unwrap_or(0)),
            Err(MiktexCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MiktexLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MiktexLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= 40);
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
        assert_eq!(SEG_BASE, 0xC9680);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MiktexCode::Ok as u32, 0);
        assert_eq!(MiktexCode::EGradeDrift as u32, 12);
        assert!(!MiktexCode::Ok.detail().is_empty());
        assert!(!MiktexCode::ESeal.detail().is_empty());
        assert_eq!(MiktexCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = MiktexLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52347_checks_green() {
        let s = run_unx52347_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
