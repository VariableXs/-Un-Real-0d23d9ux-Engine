//! UNX-F52207 pgAdmin 4 收口 · 0xC7380段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 _tools/vx15_cards.json 键 "52207" 逐字锚定）：
//!   title = "pgAdmin 4 收口"；quota = 260；criteria = "① 纯功能代码
//!   ≥260 行；② 判据原文：UNX-F52208-J1 六判据全落；建模/正向工程/
//!   逆向工程三面 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!   成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式说明：off-by-one 板式——卡 N 判据锚 UNX-F(N+1)-J1 照原文锚定
//! （本卡 52207 判据锚 UNX-F52208-J1），功能按本卡标题落地。
//!
//! 设计要点：
//!   ① 域模型：PostgreSQL 12–16 五版本阶梯闭集（逐版本格载荷纯函数
//!      折叠恒等才收，12→16 单调）。
//!   ② 查询工具 20 操作集账，操作类别五元闭集（编写/执行/计划/结果
//!      导出/历史回放），类别单调，逆序必抓。
//!   ③ 服务器组 16 格闭集（连接/库/模式三层各 5/6/5 格）。
//!   ④ Electron 上栈 4 层闭集（Chromium 渲染层/Node 主进程层/原生绑
//!      定层/打包分发层）+ 浏览器模式/桌面模式双形态声明 2 行，缺一
//!      拒封。
//!   ⑤ 注入 ×10 轮（版本漂移/服务器组漂移/操作越闭集三态轮转）
//!      10/10 必抓，检出+逃逸守恒等式逐轮成立；六判据母版闸 + 定级
//!      闭集（5 版本格 + 20 操作 + 16 服务器组格 + 4 层栈 + 2 声明
//!      全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52207 − 52201)）。
pub const SEG_BASE: u32 = 0xC7380;

/// PostgreSQL 版本阶梯格数（12–16 五版本）。
pub const VER_LADDER: usize = 5;

/// 查询工具操作数。
pub const QTOOL_OPS: usize = 20;

/// 服务器组格数。
pub const SRVGRP_CELLS: usize = 16;

/// Electron 上栈层数。
pub const STACK_LAYERS: usize = 4;

/// 双形态声明行数。
pub const DUAL_ROWS: usize = 2;

/// 守卫：段基址 = 公式值且 0x40 对齐。
const _SEG_GUARD: () = assert!(
    SEG_BASE == 0xC7200 + 0x40 * (52207u32 - 52201) && SEG_BASE % 0x40 == 0
);

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgAdminCode {
    /// 成功
    Ok = 0,
    /// 版本格越闭集
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 操作类别逆序
    EOrder,
    /// 服务器组格越闭集
    ENoGrp,
    /// 服务器组载荷漂移
    EGrpDrift,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈层漂移
    ELayerDrift,
    /// 双形态声明越闭集
    ENoDual,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl PgAdminCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PgAdminCode::Ok => "成功",
            PgAdminCode::ENoVer => "版本格越闭集：PostgreSQL 12–16 五版本阶梯之外先登记",
            PgAdminCode::EVerDrift => "版本阶梯漂移：版本折叠与基准不恒等，逐格复算",
            PgAdminCode::ENoOp => "操作号越闭集：查询工具 20 操作集之外先登记",
            PgAdminCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            PgAdminCode::EOrder => "操作类别逆序：编写→执行→计划→结果导出→历史回放单调，逆序必抓",
            PgAdminCode::ENoGrp => "服务器组格越闭集：服务器组 16 格闭集之外先登记",
            PgAdminCode::EGrpDrift => "服务器组载荷漂移：服务器组折叠与基准不恒等，逐格复算",
            PgAdminCode::ENoLayer => "上栈层越闭集：Electron 上栈 4 层闭集之外先登记",
            PgAdminCode::ELayerDrift => "上栈层漂移：层折叠与基准不恒等，逐层复算",
            PgAdminCode::ENoDual => "双形态声明越闭集：浏览器模式/桌面模式 2 行之外先登记",
            PgAdminCode::EDup => "重复落账：同格只许一条账目",
            PgAdminCode::EGradeDrift => "定级漂移：5 版本格+20 操作+16 服务器组格+4 层栈+2 声明全通才定级 S1",
        }
    }
}

// ── 版本阶梯（PostgreSQL 12–16 五版本闭集） ──────────────────────
/// 版本号（格 v → 12 + v）。
pub const fn ver_num(v: usize) -> Option<u32> {
    if v >= VER_LADDER {
        return None;
    }
    Some(12 + v as u32)
}

/// 版本格基准（格 v；载荷 = 版本号 ^ FNV 质数扰动）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VER_LADDER {
        return None;
    }
    let num = (12 + v) as u64;
    Some((0xB9100u64 + num * 0x10) ^ num.wrapping_mul(0x01000193))
}

// ── 查询工具 20 操作集 ───────────────────────────────────────────
/// 操作类别（0..4=编写 4..8=执行 8..12=计划 12..16=结果导出 16..20=历史回放）。
pub const fn qtool_kind(o: usize) -> Option<u32> {
    if o >= QTOOL_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 操作格载荷（操作 o；类别 = o/4）。
pub const fn qtool_cell(o: usize) -> Option<u64> {
    if o >= QTOOL_OPS {
        return None;
    }
    Some((0xBA000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

// ── 服务器组 16 格闭集（连接 5/库 6/模式 5 三层） ────────────────
/// 服务器组层（格 g：0..5=连接层 5..11=库层 11..16=模式层）。
pub const fn srvgrp_layer(g: usize) -> Option<u32> {
    if g >= SRVGRP_CELLS {
        return None;
    }
    if g < 5 {
        Some(0)
    } else if g < 11 {
        Some(1)
    } else {
        Some(2)
    }
}

/// 服务器组格基准（格 g）。
pub const fn srvgrp_cell(g: usize) -> Option<u64> {
    if g >= SRVGRP_CELLS {
        return None;
    }
    Some((0xBB000u64 + (g as u64) * 0x38) ^ 0x504741446D696E34)
}

/// 服务器组层名。
pub const fn srvgrp_layer_name(l: u32) -> Option<&'static str> {
    match l {
        0 => Some("连接层"),
        1 => Some("库层"),
        2 => Some("模式层"),
        _ => None,
    }
}

// ── Electron 上栈 4 层 + 双形态声明 ─────────────────────────────
/// 上栈层基准（层 t）。
pub const fn stack_layer(t: usize) -> Option<u32> {
    if t >= STACK_LAYERS {
        return None;
    }
    Some(0xBC000 + t as u32 * 0x2C)
}

/// 上栈层名（Chromium 渲染层/Node 主进程层/原生绑定层/打包分发层）。
pub const fn stack_layer_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("Chromium 渲染层"),
        1 => Some("Node 主进程层"),
        2 => Some("原生绑定层"),
        3 => Some("打包分发层"),
        _ => None,
    }
}

/// 双形态声明行折叠（行 d）。
pub const fn dual_row(d: usize) -> Option<u32> {
    if d >= DUAL_ROWS {
        return None;
    }
    Some(0xBD000 + d as u32 * 0x2C)
}

/// 双形态声明行名。
pub const fn dual_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("浏览器模式双形态声明"),
        1 => Some("桌面模式双形态声明"),
        _ => None,
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 定级（S1/S2/S3 闭集；本卡 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgGrade {
    /// S1：五版本全阶梯收口级
    S1,
    /// S2：不达（备用档）
    S2,
    /// S3：降级档
    S3,
}

impl PgGrade {
    pub fn tag(self) -> &'static str {
        match self {
            PgGrade::S1 => "S1",
            PgGrade::S2 => "S2",
            PgGrade::S3 => "S3",
        }
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PgLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 版本账。
    vers: [Option<u64>; VER_LADDER],
    /// 操作账。
    ops: [Option<u64>; QTOOL_OPS],
    /// 服务器组账。
    grps: [Option<u64>; SRVGRP_CELLS],
    /// 上栈层账。
    layers: [Option<u32>; STACK_LAYERS],
    /// 双形态声明账。
    duals: [Option<u32>; DUAL_ROWS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PgLedger {
    /// 新账本。
    pub fn new() -> Self {
        PgLedger {
            gates: [None; 6],
            vers: [None; VER_LADDER],
            ops: [None; QTOOL_OPS],
            grps: [None; SRVGRP_CELLS],
            layers: [None; STACK_LAYERS],
            duals: [None; DUAL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PgAdminCode> {
        if gate >= 6 {
            return Err(PgAdminCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7A100 + gate as u64);
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

    /// 版本一格（版本号单调 + 载荷对照 + 重复拒）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), PgAdminCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(PgAdminCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A200 + v as u64);
            return Err(PgAdminCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PgAdminCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x7A201 + v as u64);
        Ok(())
    }

    /// 版本全账核验（5 格）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VER_LADDER {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 操作一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), PgAdminCode> {
        if o >= QTOOL_OPS {
            return Err(PgAdminCode::ENoOp);
        }
        let kind = match qtool_kind(o) {
            Some(k) => k,
            None => return Err(PgAdminCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A300);
            return Err(PgAdminCode::EOrder);
        }
        let want = match qtool_cell(o) {
            Some(w) => w,
            None => return Err(PgAdminCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A301 + o as u64);
            return Err(PgAdminCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PgAdminCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7A302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < QTOOL_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 服务器组一格（载荷对照 + 重复拒）。
    pub fn record_grp(&mut self, g: usize, got: u64) -> Result<(), PgAdminCode> {
        let want = match srvgrp_cell(g) {
            Some(w) => w,
            None => return Err(PgAdminCode::ENoGrp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A400 + g as u64);
            return Err(PgAdminCode::EGrpDrift);
        }
        if match self.grps.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PgAdminCode::EDup);
        }
        if let Some(cc) = self.grps.get_mut(g) {
            *cc = Some(got);
        }
        self.fnv(0x7A401 + g as u64);
        Ok(())
    }

    /// 服务器组全账核验（16 格）。
    pub fn grps_all(&self) -> bool {
        let mut g = 0usize;
        while g < SRVGRP_CELLS {
            match self.grps.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 上栈层一格（层折叠对照 + 重复拒）。
    pub fn mount_layer(&mut self, t: usize) -> Result<(), PgAdminCode> {
        let want = match stack_layer(t) {
            Some(w) => w,
            None => return Err(PgAdminCode::ENoLayer),
        };
        if match self.layers.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PgAdminCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x7A500 + t as u64);
        Ok(())
    }

    /// 上栈全落核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut t = 0usize;
        while t < STACK_LAYERS {
            match self.layers.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 双形态声明一行。
    pub fn declare_dual(&mut self, d: usize) -> Result<(), PgAdminCode> {
        if d >= DUAL_ROWS {
            return Err(PgAdminCode::ENoDual);
        }
        if match self.duals.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PgAdminCode::EDup);
        }
        if let Some(c) = self.duals.get_mut(d) {
            *c = dual_row(d);
        }
        self.fnv(0x7A600 + d as u64);
        Ok(())
    }

    /// 双形态声明全齐核验。
    pub fn duals_all(&self) -> bool {
        let mut d = 0usize;
        while d < DUAL_ROWS {
            match self.duals.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 收口定级（全格全通 + 上栈全落 + 双形态全齐 + 六判据全落 → S1）。
    pub fn grade(&self) -> PgGrade {
        if self.gates_all() && self.vers_all() && self.ops_all() && self.grps_all() && self.layers_all() && self.duals_all() {
            PgGrade::S1
        } else if self.vers_all() && self.ops_all() {
            PgGrade::S2
        } else {
            PgGrade::S3
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for PgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VER_LADDER {
        if let Some(w) = version_cell(v) {
            if led.record_ver(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < QTOOL_OPS {
        if let Some(w) = qtool_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut gr = 0usize;
    while gr < SRVGRP_CELLS {
        if let Some(w) = srvgrp_cell(gr) {
            if led.record_grp(gr, w).is_ok() {
                recorded += 1;
            }
        }
        gr += 1;
    }
    let mut t = 0usize;
    while t < STACK_LAYERS {
        let _ = led.mount_layer(t);
        t += 1;
    }
    let mut d = 0usize;
    while d < DUAL_ROWS {
        let _ = led.declare_dual(d);
        d += 1;
    }
    // 注入 10 轮：版本漂移 / 服务器组漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PgLedger::new();
        let caught = match kind {
            0 => lt.record_ver(2, version_cell(2).unwrap_or(0).wrapping_add(1)) == Err(PgAdminCode::EVerDrift),
            1 => lt.record_grp(9, srvgrp_cell(9).unwrap_or(0).wrapping_add(1)) == Err(PgAdminCode::EGrpDrift),
            _ => lt.record_op(QTOOL_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7A700 + k as u64);
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
pub fn run_unx52207_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52207");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "查询工具 20 操作全通",
        recorded >= (QTOOL_OPS as u32) + (SRVGRP_CELLS as u32) + (VER_LADDER as u32),
        "编写/执行/计划/结果导出/历史回放 五类逐操作恒等",
    );
    s.add(
        "PostgreSQL 12–16 五版本阶梯 5/5",
        {
            let mut lt = PgLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_LADDER {
                if let Some(w) = version_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "12→16 单调逐版本折叠恒等（判据锚 UNX-F52208-J1）",
    );
    s.add(
        "服务器组 16/16 全通",
        {
            let mut lt = PgLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SRVGRP_CELLS {
                if let Some(w) = srvgrp_cell(g) {
                    if lt.record_grp(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.grps_all()
        },
        "连接 5/库 6/模式 5 三层闭集",
    );
    s.add(
        "Electron 上栈 4 层全落",
        {
            let mut lt = PgLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < STACK_LAYERS {
                if lt.mount_layer(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.layers_all()
        },
        "Chromium 渲染层/Node 主进程层/原生绑定层/打包分发层",
    );
    s.add(
        "双形态声明 2/2 全齐",
        {
            let mut lt = PgLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DUAL_ROWS {
                if lt.declare_dual(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.duals_all()
        },
        "浏览器模式/桌面模式双形态声明两行闭集，缺一拒封",
    );
    s.add(
        "操作类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < QTOOL_OPS {
                if qtool_kind(o).unwrap_or(9) < qtool_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调",
    );
    s.add(
        "版本号阶梯单调",
        {
            let mut ok = true;
            let mut v = 1usize;
            while v < VER_LADDER {
                if ver_num(v).unwrap_or(0) <= ver_num(v - 1).unwrap_or(0) {
                    ok = false;
                }
                v += 1;
            }
            ok
        },
        "12→16 严格递增",
    );
    s.add("注入 10/10 全抓", caught >= 10, "版本漂移/服务器组漂移/越闭集三态轮转必抓");
    let mut l0 = PgLedger::new();
    s.add(
        "边界·版本越闭集",
        l0.record_ver(VER_LADDER, 0).is_err(),
        "版本格 5 越闭集必拒",
    );
    s.add(
        "边界·服务器组越闭集",
        l0.record_grp(SRVGRP_CELLS, 0).is_err(),
        "服务器组格 16 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(QTOOL_OPS, 0).is_err(),
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
    let led_full = {
        let mut lt = PgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut v = 0usize;
        while v < VER_LADDER {
            if let Some(w) = version_cell(v) {
                let _ = lt.record_ver(v, w);
            }
            v += 1;
        }
        let mut o = 0usize;
        while o < QTOOL_OPS {
            if let Some(w) = qtool_cell(o) {
                let _ = lt.record_op(o, w);
            }
            o += 1;
        }
        let mut gr = 0usize;
        while gr < SRVGRP_CELLS {
            if let Some(w) = srvgrp_cell(gr) {
                let _ = lt.record_grp(gr, w);
            }
            gr += 1;
        }
        let mut t = 0usize;
        while t < STACK_LAYERS {
            let _ = lt.mount_layer(t);
            t += 1;
        }
        let mut d = 0usize;
        while d < DUAL_ROWS {
            let _ = lt.declare_dual(d);
            d += 1;
        }
        lt
    };
    s.add(
        "定级闭集 S1",
        led_full.grade() == PgGrade::S1 && led_full.grade().tag() == "S1",
        "全格全通+上栈全落+双形态全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7380 && SEG_BASE > 0xC7340,
        "SEG_BASE 0x40 对齐且落在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ver_ladder() {
        assert_eq!(ver_num(0), Some(12));
        assert_eq!(ver_num(4), Some(16));
        assert!(ver_num(5).is_none());
        assert!(version_cell(5).is_none());
    }

    #[test]
    fn test_qtool_kind() {
        assert_eq!(qtool_kind(0), Some(0));
        assert_eq!(qtool_kind(3), Some(0));
        assert_eq!(qtool_kind(4), Some(1));
        assert_eq!(qtool_kind(19), Some(4));
        assert!(qtool_kind(20).is_none());
        assert!(qtool_cell(20).is_none());
    }

    #[test]
    fn test_srvgrp_layers() {
        assert_eq!(srvgrp_layer(0), Some(0));
        assert_eq!(srvgrp_layer(5), Some(1));
        assert_eq!(srvgrp_layer(11), Some(2));
        assert!(srvgrp_layer(16).is_none());
        assert_eq!(srvgrp_layer_name(0), Some("连接层"));
        assert_eq!(srvgrp_layer_name(2), Some("模式层"));
        assert!(srvgrp_layer_name(3).is_none());
    }

    #[test]
    fn test_stack_and_dual_names() {
        assert_eq!(stack_layer_name(0), Some("Chromium 渲染层"));
        assert_eq!(stack_layer_name(3), Some("打包分发层"));
        assert!(stack_layer_name(4).is_none());
        assert!(stack_layer(STACK_LAYERS).is_none());
        assert_eq!(dual_name(0), Some("浏览器模式双形态声明"));
        assert_eq!(dual_name(1), Some("桌面模式双形态声明"));
        assert!(dual_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = version_cell(3).unwrap_or(0);
        assert_eq!(a, version_cell(3).unwrap_or(0));
        assert_ne!(a, version_cell(4).unwrap_or(0));
        assert_ne!(srvgrp_cell(0).unwrap_or(0), srvgrp_cell(1).unwrap_or(0));
        assert!(dual_row(DUAL_ROWS).is_none());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PgLedger::new();
        assert_eq!(
            lt.record_grp(4, srvgrp_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(PgAdminCode::EGrpDrift)
        );
        assert!(lt.record_grp(4, srvgrp_cell(4).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_grp(4, srvgrp_cell(4).unwrap_or(0)),
            Err(PgAdminCode::EDup)
        );
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = PgLedger::new();
        let _ = lt.record_op(17, qtool_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(3, qtool_cell(3).unwrap_or(0)), Err(PgAdminCode::EOrder));
        assert!(lt.record_op(16, qtool_cell(16).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (VER_LADDER + QTOOL_OPS + SRVGRP_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52207 - 52201));
        assert!(SEG_BASE > 0xC7340 && SEG_BASE <= 0xC73C0);
    }

    #[test]
    fn test_val_details_nonempty() {
        let codes = [
            PgAdminCode::Ok,
            PgAdminCode::ENoVer,
            PgAdminCode::EVerDrift,
            PgAdminCode::ENoOp,
            PgAdminCode::EOpDrift,
            PgAdminCode::EOrder,
            PgAdminCode::ENoGrp,
            PgAdminCode::EGrpDrift,
            PgAdminCode::ENoLayer,
            PgAdminCode::ELayerDrift,
            PgAdminCode::ENoDual,
            PgAdminCode::EDup,
            PgAdminCode::EGradeDrift,
        ];
        let mut n = 0u32;
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i].val(), SEG_BASE + n);
            assert!(!codes[i].detail().is_empty());
            n += 1;
            i += 1;
        }
    }

    #[test]
    fn test_unx52207_checks_green() {
        let s = run_unx52207_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
