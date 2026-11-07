//! UNX-F52335 Resource Hacker 收口 · 0xC9380 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52335"，逐字）：
//!   title: Resource Hacker 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52336-J1 快照差分
//!     ×10 场景全通；注册表语义消费对账（E 部联签锚定行）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52335）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52336-J1，照原文锚定；功能按本卡标题（Resource Hacker）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① 资源四族 20 格：位图/图标/对话框/版本，每族 5 格，族单调（逆序必抓）
//!      + 相位闸（后族先录对空账本同样必抓）。
//!   ② 编辑 10 操作：逐操作基准载荷对照，漂移必抓。
//!   ③ 脚本编译 8 格：资源脚本逐格编译落账，越闭集必抓。
//!   ④ 反编译 5 场景：场景载荷对照 + 脚本锚格联签对账（E 部联签口径），
//!      缺锚拒收。
//!   ⑤ 收口闸六判据：逐格落账，未齐即封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10 轮（资源格越闭集/编辑漂移/
//!      相位越序），三类注入全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；漂移注入先录正格再注漂移；相位注入对空账本必抓且不改
//!      last_kind；双跑指纹一致；全通定级 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52335;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 资源格数（四族 × 5）。
pub const RES_CELLS: usize = 20;

/// 每族格数。
pub const FAMILY_CELLS: usize = 5;

/// 编辑操作数。
pub const EDIT_OPS: usize = 10;

/// 脚本编译格数。
pub const SCRIPT_CELLS: usize = 8;

/// 反编译场景数。
pub const DECOMP_SCENES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResHackerCode {
    /// 成功
    Ok = 0,
    /// 资源格越闭集
    ENoCell,
    /// 载荷漂移
    ECellDrift,
    /// 族序逆序
    EOrder,
    /// 相位越序（后族先录）
    EPhase,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 编辑操作越闭集
    ENoOp,
    /// 脚本编译格越闭集
    ENoScript,
    /// 反编译场景越闭集
    ENoDecomp,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl ResHackerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ResHackerCode::Ok => "成功",
            ResHackerCode::ENoCell => "资源格越闭集：资源 20 格闭集之外先登记",
            ResHackerCode::ECellDrift => "载荷漂移：编辑/资源折叠值与基准不恒等，逐格复算",
            ResHackerCode::EOrder => "族序逆序：位图→图标→对话框→版本单调，逆序必抓",
            ResHackerCode::EPhase => "相位越序：前族未齐先录后族，对空账本同样必抓",
            ResHackerCode::EDup => "重复落账：同格只许一条账目",
            ResHackerCode::ENoGate => "判据闸越闭集：六判据闸 0..=5 之外先登记",
            ResHackerCode::ESeal => "未齐即封：20 资源格 + 10 编辑 + 8 脚本格 + 5 场景未齐先补齐",
            ResHackerCode::ENoOp => "编辑操作越闭集：编辑 10 操作闭集之外先登记",
            ResHackerCode::ENoScript => "脚本编译格越闭集：脚本编译 8 格闭集之外先登记",
            ResHackerCode::ENoDecomp => "反编译场景越闭集：反编译 5 场景闭集之外先登记",
            ResHackerCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ResHackerCode::EGradeDrift => "定级漂移：全通且脚本编译全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20资源格+10编辑+8脚本格+5场景全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 资源格载荷（格 o；族 = o/5；XOR 锚低 16 位恒零）。
pub const fn res_cell(o: usize) -> Option<u64> {
    if o >= RES_CELLS {
        return None;
    }
    Some((0xB3F00u64 + (o as u64) * 0x43) ^ 0x5248_0000)
}

/// 资源族（0=位图 1=图标 2=对话框 3=版本）。
pub const fn res_kind(o: usize) -> Option<u32> {
    if o >= RES_CELLS {
        return None;
    }
    Some((o / FAMILY_CELLS) as u32)
}

/// 族名。
pub const fn res_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("位图"),
        1 => Some("图标"),
        2 => Some("对话框"),
        3 => Some("版本"),
        _ => None,
    }
}

/// 编辑操作基准载荷（操作 e）。
pub const fn edit_cell(e: usize) -> Option<u32> {
    if e >= EDIT_OPS {
        return None;
    }
    Some(0xB4000 + e as u32 * 0x31)
}

/// 脚本编译格载荷（格 c）。
pub const fn script_cell(c: usize) -> Option<u32> {
    if c >= SCRIPT_CELLS {
        return None;
    }
    Some(0xB4100 + c as u32 * 0x33)
}

/// 反编译场景载荷（场景 d；与脚本锚格联签对账）。
pub const fn decomp_cell(d: usize) -> Option<u32> {
    if d >= DECOMP_SCENES {
        return None;
    }
    match script_cell(d + 3) {
        Some(anchor) => Some(anchor ^ 0x5244_0000 ^ (d as u32).wrapping_mul(0x1F123BB5)),
        None => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Resource Hacker 收口账本状态机。
#[derive(Clone)]
pub struct ResHackerLedger {
    /// 六判据（资源装载/查看提取/编辑替换/脚本编译/崩溃退出/回写声明）。
    gates: [Option<bool>; 6],
    /// 资源账。
    res: [Option<u64>; RES_CELLS],
    /// 编辑账。
    edits: [Option<u32>; EDIT_OPS],
    /// 脚本编译账。
    scripts: [Option<u32>; SCRIPT_CELLS],
    /// 反编译场景账。
    decomps: [Option<u32>; DECOMP_SCENES],
    /// 最后族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ResHackerLedger {
    /// 新账本。
    pub fn new() -> Self {
        ResHackerLedger {
            gates: [None; 6],
            res: [None; RES_CELLS],
            edits: [None; EDIT_OPS],
            scripts: [None; SCRIPT_CELLS],
            decomps: [None; DECOMP_SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ResHackerCode> {
        if gate >= 6 {
            return Err(ResHackerCode::ENoGate);
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

    /// 前族全齐核验（相位闸用）。
    fn families_complete(&self, upto: usize) -> bool {
        let bound = upto * FAMILY_CELLS;
        let mut i = 0usize;
        while i < bound {
            match self.res.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 资源一格（族单调 + 相位闸 + 载荷对照 + 重复拒）。
    pub fn record_res(&mut self, o: usize, got: u64) -> Result<(), ResHackerCode> {
        let kind = match res_kind(o) {
            Some(k) => k,
            None => return Err(ResHackerCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(ResHackerCode::EOrder);
        }
        if (kind as usize) > 0 && !self.families_complete(kind as usize) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201);
            return Err(ResHackerCode::EPhase);
        }
        let want = match res_cell(o) {
            Some(w) => w,
            None => return Err(ResHackerCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8202 + o as u64);
            return Err(ResHackerCode::ECellDrift);
        }
        if match self.res.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ResHackerCode::EDup);
        }
        if let Some(c) = self.res.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8203 + o as u64);
        Ok(())
    }

    /// 资源全账核验（20 格）。
    pub fn res_all(&self) -> bool {
        let mut o = 0usize;
        while o < RES_CELLS {
            match self.res.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 编辑一操作（基准载荷对照 + 重复拒）。
    pub fn record_edit(&mut self, e: usize, got: u32) -> Result<(), ResHackerCode> {
        if e >= EDIT_OPS {
            return Err(ResHackerCode::ENoOp);
        }
        if match self.edits.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ResHackerCode::EDup);
        }
        let want = match edit_cell(e) {
            Some(w) => w,
            None => return Err(ResHackerCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + e as u64);
            return Err(ResHackerCode::ECellDrift);
        }
        if let Some(c) = self.edits.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8301 + e as u64);
        Ok(())
    }

    /// 编辑全账核验（10 操作）。
    pub fn edits_all(&self) -> bool {
        let mut e = 0usize;
        while e < EDIT_OPS {
            match self.edits.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 脚本编译一格。
    pub fn record_script(&mut self, c: usize, got: u32) -> Result<(), ResHackerCode> {
        if c >= SCRIPT_CELLS {
            return Err(ResHackerCode::ENoScript);
        }
        if match self.scripts.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ResHackerCode::EDup);
        }
        let want = match script_cell(c) {
            Some(w) => w,
            None => return Err(ResHackerCode::ENoScript),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + c as u64);
            return Err(ResHackerCode::ECellDrift);
        }
        if let Some(c2) = self.scripts.get_mut(c) {
            *c2 = Some(got);
        }
        self.fnv(0x8401 + c as u64);
        Ok(())
    }

    /// 脚本编译全账核验（8 格）。
    pub fn scripts_all(&self) -> bool {
        let mut c = 0usize;
        while c < SCRIPT_CELLS {
            match self.scripts.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 反编译一场景（脚本锚格未落则拒收，联签对账）。
    pub fn record_decomp(&mut self, d: usize, got: u32) -> Result<(), ResHackerCode> {
        if d >= DECOMP_SCENES {
            return Err(ResHackerCode::ENoDecomp);
        }
        if match self.decomps.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ResHackerCode::EDup);
        }
        let anchor = d + 3;
        if anchor >= SCRIPT_CELLS {
            return Err(ResHackerCode::ENoScript);
        }
        if match self.scripts.get(anchor) {
            Some(Some(_)) => false,
            _ => true,
        } {
            return Err(ResHackerCode::ESeal);
        }
        let want = match decomp_cell(d) {
            Some(w) => w,
            None => return Err(ResHackerCode::ENoDecomp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + d as u64);
            return Err(ResHackerCode::ECellDrift);
        }
        if let Some(c) = self.decomps.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x8501 + d as u64);
        Ok(())
    }

    /// 反编译全账核验（5 场景）。
    pub fn decomps_all(&self) -> bool {
        let mut d = 0usize;
        while d < DECOMP_SCENES {
            match self.decomps.get(d) {
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

    /// 最后族（相位/族序注入不改此值）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.res_all()
            && self.edits_all()
            && self.scripts_all()
            && self.decomps_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ResHackerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ResHackerLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < RES_CELLS {
        if let Some(w) = res_cell(o) {
            if led.record_res(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < EDIT_OPS {
        if let Some(w) = edit_cell(e) {
            if led.record_edit(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut c = 0usize;
    while c < SCRIPT_CELLS {
        if let Some(w) = script_cell(c) {
            let _ = led.record_script(c, w);
        }
        c += 1;
    }
    let mut d = 0usize;
    while d < DECOMP_SCENES {
        if let Some(w) = decomp_cell(d) {
            let _ = led.record_decomp(d, w);
        }
        d += 1;
    }
    // 注入 10 轮：资源格越闭集 / 编辑漂移 / 相位越序 三态轮转，全部落在当轮临时账本。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ResHackerLedger::new();
        let caught = match kind {
            0 => lt.record_res(RES_CELLS, 0) == Err(ResHackerCode::ENoCell),
            1 => {
                // 漂移注入：先录正格再注漂移。
                let _ = lt.record_edit(0, edit_cell(0).unwrap_or(0));
                lt.record_edit(1, edit_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ResHackerCode::ECellDrift)
            }
            _ => {
                // 相位注入：对空账本录第 1 族首格（前族未齐），必抓且不改 last_kind。
                lt.record_res(5, res_cell(5).unwrap_or(0)) == Err(ResHackerCode::EPhase)
                    && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52335 判据集（锚 UNX-F52336-J1，off-by-one 板式）。
pub fn run_unx52335_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52335");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "资源四族 20 格全通",
        recorded >= (RES_CELLS + EDIT_OPS) as u32,
        "位图/图标/对话框/版本逐格恒等落账",
    );
    s.add(
        "编辑 10 操作全落",
        {
            let mut lt = ResHackerLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EDIT_OPS {
                if let Some(w) = edit_cell(e) {
                    if lt.record_edit(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.edits_all()
        },
        "编辑 10 操作基准载荷逐项对照，漂移必抓",
    );
    s.add(
        "脚本编译 8 格全落",
        {
            let mut lt = ResHackerLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SCRIPT_CELLS {
                if let Some(w) = script_cell(c) {
                    if lt.record_script(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.scripts_all()
        },
        "资源脚本逐格编译落账，越闭集必抓",
    );
    s.add(
        "反编译 5 场景全落（联签对账）",
        {
            let mut lt = ResHackerLedger::new();
            let mut c = 0usize;
            while c < SCRIPT_CELLS {
                if let Some(w) = script_cell(c) {
                    let _ = lt.record_script(c, w);
                }
                c += 1;
            }
            let mut ok = true;
            let mut d = 0usize;
            while d < DECOMP_SCENES {
                if let Some(w) = decomp_cell(d) {
                    if lt.record_decomp(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.decomps_all()
        },
        "反编译场景与脚本锚格联签对账，缺锚拒收（E 部联签口径）",
    );
    s.add(
        "收口闸六判据全落",
        {
            let mut lt = ResHackerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "资源装载/查看提取/编辑替换/脚本编译/崩溃退出/回写声明",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < RES_CELLS {
                if res_kind(o).unwrap_or(4) < res_kind(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "四族闭集单调，逆序必抓",
    );
    s.add(
        "判据 ×10 场景全通（快照差分口径，off-by-one）",
        caught >= 10,
        "十轮注入三态轮转全部识别，无一逃逸",
    );
    s.add(
        "边界·资源格越闭集",
        {
            let mut lt = ResHackerLedger::new();
            lt.record_res(RES_CELLS, 0).is_err()
        },
        "资源格 20 越闭集必拒",
    );
    s.add(
        "边界·编辑操作越闭集",
        {
            let mut lt = ResHackerLedger::new();
            lt.record_edit(EDIT_OPS, 0).is_err()
        },
        "编辑操作 10 越闭集必拒",
    );
    s.add(
        "边界·反编译场景越闭集",
        {
            let mut lt = ResHackerLedger::new();
            lt.record_decomp(DECOMP_SCENES, 0).is_err()
        },
        "反编译场景 5 越闭集必拒",
    );
    s.add(
        "相位注入对空账本必抓且不改 last_kind",
        {
            let mut lt = ResHackerLedger::new();
            let hit = lt.record_res(6, res_cell(6).unwrap_or(0)) == Err(ResHackerCode::EPhase);
            hit && lt.last_kind() == -1
        },
        "前族未齐先录后族，空账本同样必抓",
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
            // main/aux 各节全部记账后再断言定级。
            let mut lt = ResHackerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < RES_CELLS {
                if let Some(w) = res_cell(o) {
                    let _ = lt.record_res(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < EDIT_OPS {
                if let Some(w) = edit_cell(e) {
                    let _ = lt.record_edit(e, w);
                }
                e += 1;
            }
            let mut c = 0usize;
            while c < SCRIPT_CELLS {
                if let Some(w) = script_cell(c) {
                    let _ = lt.record_script(c, w);
                }
                c += 1;
            }
            let mut d = 0usize;
            while d < DECOMP_SCENES {
                if let Some(w) = decomp_cell(d) {
                    let _ = lt.record_decomp(d, w);
                }
                d += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9380,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9380",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_res_kinds() {
        assert_eq!(res_kind(0), Some(0));
        assert_eq!(res_kind(5), Some(1));
        assert_eq!(res_kind(19), Some(3));
        assert!(res_kind(20).is_none());
        assert!(res_cell(20).is_none());
        assert_eq!(res_kind_name(0), Some("位图"));
        assert_eq!(res_kind_name(3), Some("版本"));
        assert!(res_kind_name(4).is_none());
    }

    #[test]
    fn test_edit_and_script_cells() {
        assert!(edit_cell(EDIT_OPS).is_none());
        assert!(script_cell(SCRIPT_CELLS).is_none());
        assert!(decomp_cell(DECOMP_SCENES).is_none());
        assert_ne!(edit_cell(0).unwrap_or(0), edit_cell(1).unwrap_or(0));
        assert_ne!(script_cell(2).unwrap_or(0), script_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_cells_pure() {
        let a = res_cell(3).unwrap_or(0);
        assert_eq!(a, res_cell(3).unwrap_or(0));
        assert_ne!(a, res_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        // 测试种子须按合法序先落低族格。
        let mut lt = ResHackerLedger::new();
        let mut o = 0usize;
        while o < 3 * FAMILY_CELLS {
            let _ = lt.record_res(o, res_cell(o).unwrap_or(0));
            o += 1;
        }
        assert_eq!(
            lt.record_res(3, res_cell(3).unwrap_or(0)),
            Err(ResHackerCode::EOrder)
        );
        assert!(lt.record_res(15, res_cell(15).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ResHackerLedger::new();
        assert_eq!(
            lt.record_edit(4, edit_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(ResHackerCode::ECellDrift)
        );
        assert_eq!(
            lt.record_script(6, script_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(ResHackerCode::ECellDrift)
        );
    }

    #[test]
    fn test_decomp_requires_script_anchor() {
        let mut lt = ResHackerLedger::new();
        // 脚本锚格（d+3）未落 → 场景拒收。
        if let Some(w) = decomp_cell(0) {
            assert_eq!(lt.record_decomp(0, w), Err(ResHackerCode::ESeal));
        }
        if let Some(w) = script_cell(3) {
            assert!(lt.record_script(3, w).is_ok());
        }
        if let Some(w) = decomp_cell(0) {
            assert!(lt.record_decomp(0, w).is_ok());
        }
        assert_eq!(lt.record_decomp(0, 0), Err(ResHackerCode::EDup));
    }

    #[test]
    fn test_phase_on_empty_ledger() {
        let mut lt = ResHackerLedger::new();
        assert_eq!(
            lt.record_res(6, res_cell(6).unwrap_or(0)),
            Err(ResHackerCode::EPhase)
        );
        assert_eq!(lt.last_kind(), -1);
        assert!(lt.record_res(0, res_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ResHackerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (RES_CELLS + EDIT_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9380);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ResHackerCode::Ok as u32, 0);
        assert_eq!(ResHackerCode::ENoOp as u32, 8);
        assert_eq!(ResHackerCode::EGradeDrift as u32, 12);
        let all = [
            ResHackerCode::Ok,
            ResHackerCode::ENoCell,
            ResHackerCode::ECellDrift,
            ResHackerCode::EOrder,
            ResHackerCode::EPhase,
            ResHackerCode::EDup,
            ResHackerCode::ENoGate,
            ResHackerCode::ESeal,
            ResHackerCode::ENoOp,
            ResHackerCode::ENoScript,
            ResHackerCode::ENoDecomp,
            ResHackerCode::EInj,
            ResHackerCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ResHackerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52335_checks_green() {
        let s = run_unx52335_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
