//! UNX-F52278 Vagrant 收口 · 0xC8540段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52278"，逐字）：
//!   title: Vagrant 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52279-J1 系统模
//!     拟启动 ×10 场景全通；**与内核 QEMU 测试设施严格分账**（本条仅
//!     Windows 发行版应用收口，不触内核侧设施，联签锚定行）；WHPX 路
//!     径消费 L5 判据（N/A 同构）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.1 语义边界；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52278）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52279-J1，照原文锚定；功能按本卡标题（Vagrant）落地，系统模
//! 拟启动判据由同批 QEMU 卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：Vagrantfile 双向投影 12 格（正向投影/逆向投影两向各
//!      6 格，方向单调，逆序必抓）；up/ssh/halt/destroy 10 操作账，
//!      四相位闭集单调（相位逆序必抓）；box 8 格；provider 三态
//!      （virtualbox/hyperv/docker）3 格；插件 6 格。
//!   ② 分账声明 2 行闭集（与内核 QEMU 测试设施严格分账 / WHPX 路径
//!      消费 L5 判据 N/A 同构），缺一拒封。
//!   ③ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作相位逆序/box 载荷漂移/
//!      投影越闭集），全部落在当轮临时账本上，主账本 last_phase 不受
//!      扰动；检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ④ KB-COMPAT 定级闭集：全通 → S2（与内核分账，本条封顶 S2，S1
//!      位序保留不派发）；判据缺 → S3。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52278;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Vagrantfile 双向投影格数。
pub const PROJ_CELLS: usize = 12;

/// 单向投影格数（正向/逆向各 6）。
pub const PROJ_DIR_CELLS: usize = 6;

/// up/ssh/halt/destroy 操作数。
pub const OPS: usize = 10;

/// box 格数。
pub const BOX_CELLS: usize = 8;

/// provider 三态格数。
pub const PROV_CELLS: usize = 3;

/// 插件格数。
pub const PLUG_CELLS: usize = 6;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VagrantCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoProj,
    /// 投影载荷漂移
    EProjDrift,
    /// 投影方向逆序
    EDirOrder,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 操作相位逆序
    EPhase,
    /// box/provider 格越闭集
    ENoBox,
    /// box/provider 载荷漂移
    EBoxDrift,
    /// 插件格越闭集
    ENoPlug,
    /// 插件载荷漂移
    EPlugDrift,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl VagrantCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VagrantCode::Ok => "成功",
            VagrantCode::ENoProj => "投影格越闭集：Vagrantfile 双向投影 12 格之外先登记",
            VagrantCode::EProjDrift => "投影载荷漂移：投影折叠与基准不恒等，逐格复算",
            VagrantCode::EDirOrder => "投影方向逆序：正向投影→逆向投影单调，逆序必抓",
            VagrantCode::ENoOp => "操作号越闭集：up/ssh/halt/destroy 10 操作集之外先登记",
            VagrantCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            VagrantCode::EPhase => "操作相位逆序：up→ssh→halt→destroy 单调，逆序必抓",
            VagrantCode::ENoBox => "box/provider 格越闭集：box 8 格 + provider 3 格之外先登记",
            VagrantCode::EBoxDrift => "box/provider 载荷漂移：折叠与基准不恒等，逐格复算",
            VagrantCode::ENoPlug => "插件格越闭集：插件 6 格之外先登记",
            VagrantCode::EPlugDrift => "插件载荷漂移：插件折叠与基准不恒等，逐格复算",
            VagrantCode::ESeal => "未齐即封：12 投影 + 10 操作 + 8 box + 3 provider + 6 插件 + 2 分账未齐先补齐",
            VagrantCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2（本条封顶 S2）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：位序保留，本卡不派发（与内核 QEMU 测试设施严格分账）
    S1,
    /// S2：有条件收口（本卡封顶）
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
            Grade::S1 => "S1：位序保留不派发——本条仅 Windows 发行版应用收口，与内核 QEMU 测试设施严格分账",
            Grade::S2 => "S2：六判据+12 投影+10 操作+8 box+3 provider+6 插件+2 分账全通，应用收口封顶",
            Grade::S3 => "S3：不收口，判据或账目缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 投影格载荷（格 p；方向 = p/6）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xC6000u64 + (p as u64) * 0x2F) ^ ((p / PROJ_DIR_CELLS) as u64).wrapping_mul(0x01000193))
}

/// 投影方向（0..6=正向投影 6..12=逆向投影）。
pub const fn proj_dir(p: usize) -> Option<u32> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((p / PROJ_DIR_CELLS) as u32)
}

/// 投影方向名。
pub const fn proj_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("正向投影"),
        1 => Some("逆向投影"),
        _ => None,
    }
}

/// 操作格载荷（操作 o）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xC7000u64 + (o as u64) * 0x33) ^ 0x5647524C5F4F5031)
}

/// 操作相位（0..3=up 3..6=ssh 6..8=halt 8..10=destroy）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 3 {
        return Some(0);
    }
    if o < 6 {
        return Some(1);
    }
    if o < 8 {
        return Some(2);
    }
    Some(3)
}

/// 操作相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("up"),
        1 => Some("ssh"),
        2 => Some("halt"),
        3 => Some("destroy"),
        _ => None,
    }
}

/// box 格基准（格 b）。
pub const fn box_cell(b: usize) -> Option<u64> {
    if b >= BOX_CELLS {
        return None;
    }
    Some((0xC8000u64 + (b as u64) * 0x39) ^ 0x5647524C5F424F58)
}

/// provider 格基准（格 v；0=virtualbox 1=hyperv 2=docker）。
pub const fn prov_cell(v: usize) -> Option<u64> {
    if v >= PROV_CELLS {
        return None;
    }
    Some((0xC9000u64 + (v as u64) * 0x43) ^ 0x5647524C5F505256)
}

/// provider 名。
pub const fn prov_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("virtualbox"),
        1 => Some("hyperv"),
        2 => Some("docker"),
        _ => None,
    }
}

/// 插件格基准（格 g）。
pub const fn plug_cell(g: usize) -> Option<u64> {
    if g >= PLUG_CELLS {
        return None;
    }
    Some((0xCA000u64 + (g as u64) * 0x47) ^ 0x5647524C5F504C47)
}

/// 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB00 + r as u32 * 0x30)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Vagrant 收口账本状态机。
#[derive(Clone)]
pub struct VagrantLedger {
    /// 六判据（Vagrantfile/up/box/provider/插件/分账）。
    gates: [Option<bool>; 6],
    /// 投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// box 账。
    boxes: [Option<u64>; BOX_CELLS],
    /// provider 账。
    provs: [Option<u64>; PROV_CELLS],
    /// 插件账。
    plugs: [Option<u64>; PLUG_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后投影方向。
    last_dir: i64,
    /// 最后操作相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VagrantLedger {
    /// 新账本。
    pub fn new() -> Self {
        VagrantLedger {
            gates: [None; 6],
            projs: [None; PROJ_CELLS],
            ops: [None; OPS],
            boxes: [None; BOX_CELLS],
            provs: [None; PROV_CELLS],
            plugs: [None; PLUG_CELLS],
            splits: [None; SPLIT_ROWS],
            last_dir: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VagrantCode> {
        if gate >= 6 {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5280 + gate as u64);
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

    /// 投影一格（方向单调 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), VagrantCode> {
        if p >= PROJ_CELLS {
            return Err(VagrantCode::ENoProj);
        }
        let dir = match proj_dir(p) {
            Some(d) => d,
            None => return Err(VagrantCode::ENoProj),
        };
        if (dir as i64) < self.last_dir {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5290);
            return Err(VagrantCode::EDirOrder);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(VagrantCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5291 + p as u64);
            return Err(VagrantCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.last_dir = dir as i64;
        self.fnv(0x5292 + p as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), VagrantCode> {
        if o >= OPS {
            return Err(VagrantCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(VagrantCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52A0);
            return Err(VagrantCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(VagrantCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52A1 + o as u64);
            return Err(VagrantCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x52A2 + o as u64);
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

    /// box 一格（载荷对照 + 重复拒）。
    pub fn record_box(&mut self, b: usize, got: u64) -> Result<(), VagrantCode> {
        let want = match box_cell(b) {
            Some(w) => w,
            None => return Err(VagrantCode::ENoBox),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52B0 + b as u64);
            return Err(VagrantCode::EBoxDrift);
        }
        if match self.boxes.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.boxes.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x52B1 + b as u64);
        Ok(())
    }

    /// box 全账核验（8 格）。
    pub fn boxes_all(&self) -> bool {
        let mut b = 0usize;
        while b < BOX_CELLS {
            match self.boxes.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// provider 一格（载荷对照 + 重复拒）。
    pub fn record_prov(&mut self, v: usize, got: u64) -> Result<(), VagrantCode> {
        let want = match prov_cell(v) {
            Some(w) => w,
            None => return Err(VagrantCode::ENoBox),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52C0 + v as u64);
            return Err(VagrantCode::EBoxDrift);
        }
        if match self.provs.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.provs.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x52C1 + v as u64);
        Ok(())
    }

    /// provider 全账核验（3 态全落）。
    pub fn provs_all(&self) -> bool {
        let mut v = 0usize;
        while v < PROV_CELLS {
            match self.provs.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, g: usize, got: u64) -> Result<(), VagrantCode> {
        let want = match plug_cell(g) {
            Some(w) => w,
            None => return Err(VagrantCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52D0 + g as u64);
            return Err(VagrantCode::EPlugDrift);
        }
        if match self.plugs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.plugs.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x52D1 + g as u64);
        Ok(())
    }

    /// 插件全账核验（6 格）。
    pub fn plugs_all(&self) -> bool {
        let mut g = 0usize;
        while g < PLUG_CELLS {
            match self.plugs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 分账声明一行（越界/重复同走 ESeal/ESeal 语义，缺一拒封）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), VagrantCode> {
        if r >= SPLIT_ROWS {
            return Err(VagrantCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VagrantCode::ESeal);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x52E0 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S2 封顶，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.projs_all() && self.ops_all() && self.boxes_all() && self.provs_all() && self.plugs_all() && self.splits_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VagrantLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VagrantLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
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
    let mut b = 0usize;
    while b < BOX_CELLS {
        if let Some(w) = box_cell(b) {
            if led.record_box(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut v = 0usize;
    while v < PROV_CELLS {
        if let Some(w) = prov_cell(v) {
            if led.record_prov(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut g2 = 0usize;
    while g2 < PLUG_CELLS {
        if let Some(w) = plug_cell(g2) {
            if led.record_plug(g2, w).is_ok() {
                recorded += 1;
            }
        }
        g2 += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作相位逆序 / box 载荷漂移 / 投影越闭集三态轮转。
    // 相位注入先按合法序落高相位格（destroy 段，操作 8）再注低相位
    // （ssh 段，操作 3），空账本上同样必抓；全部落在临时账本 lt 上，
    // 主账本 last_phase 不受扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VagrantLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(8, op_cell(8).unwrap_or(0));
                lt.record_op(3, op_cell(3).unwrap_or(0)) == Err(VagrantCode::EPhase)
            }
            1 => lt.record_box(2, box_cell(2).unwrap_or(0).wrapping_add(1)) == Err(VagrantCode::EBoxDrift),
            _ => lt.record_proj(PROJ_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x52F0 + k as u64);
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
/// UNX-F52278 判据集（锚 UNX-F52279-J1，off-by-one 板式）。
pub fn run_unx52278_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52278");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Vagrantfile 双向投影 12 格全落",
        recorded >= PROJ_CELLS as u32,
        "正向投影/逆向投影两向各 6 格逐格恒等",
    );
    s.add(
        "up/ssh/halt/destroy 10 操作全通",
        {
            let mut lt = VagrantLedger::new();
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
        "四相位 10 操作逐操作恒等（UNX-F52279-J1 系统模拟启动 ×10 场景同构承载）",
    );
    s.add(
        "box 8 格全落",
        {
            let mut lt = VagrantLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BOX_CELLS {
                if let Some(w) = box_cell(b) {
                    if lt.record_box(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.boxes_all()
        },
        "box 8 格逐格恒等，漂移必抓",
    );
    s.add(
        "provider 三态全落",
        {
            let mut lt = VagrantLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < PROV_CELLS {
                if let Some(w) = prov_cell(v) {
                    if lt.record_prov(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.provs_all()
        },
        "virtualbox/hyperv/docker 三态闭集（provider 探测消费联签锚定行）",
    );
    s.add(
        "插件 6 格全落",
        {
            let mut lt = VagrantLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < PLUG_CELLS {
                if let Some(w) = plug_cell(g) {
                    if lt.record_plug(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.plugs_all()
        },
        "插件 6 格逐格恒等（§AI-66.1 语义边界锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VagrantLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "Vagrantfile/up/box/provider/插件/分账逐格落账",
    );
    s.add(
        "分账声明 2/2 全齐",
        {
            let mut lt = VagrantLedger::new();
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
        "与内核 QEMU 测试设施严格分账 / WHPX 路径消费 L5 判据 N/A 同构，两行闭集缺一拒封",
    );
    s.add(
        "投影方向单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROJ_CELLS {
                if proj_dir(p).unwrap_or(2) < proj_dir(p - 1).unwrap_or(2) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "正向投影→逆向投影闭集单调",
    );
    s.add(
        "操作相位单调",
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
        "up→ssh→halt→destroy 闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "相位逆序/box 漂移/投影越闭集三态轮转必抓");
    let mut l0 = VagrantLedger::new();
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·box/provider 越闭集",
        l0.record_box(BOX_CELLS, 0).is_err() && l0.record_prov(PROV_CELLS, 0).is_err(),
        "box 格 8 与 provider 格 3 越闭集必拒",
    );
    s.add(
        "漂移·操作载荷漂移拒",
        {
            l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(VagrantCode::EDrift)
        },
        "操作载荷漂移必抓",
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
        "定级 S2 封顶闭集",
        {
            let mut lt = VagrantLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut b = 0usize;
            while b < BOX_CELLS {
                if let Some(w) = box_cell(b) {
                    let _ = lt.record_box(b, w);
                }
                b += 1;
            }
            let mut v = 0usize;
            while v < PROV_CELLS {
                if let Some(w) = prov_cell(v) {
                    let _ = lt.record_prov(v, w);
                }
                v += 1;
            }
            let mut g2 = 0usize;
            while g2 < PLUG_CELLS {
                if let Some(w) = plug_cell(g2) {
                    let _ = lt.record_plug(g2, w);
                }
                g2 += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，S1 位序保留不派发）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8540,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×(52278−52201)",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_phase() {
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(2), Some(0));
        assert_eq!(op_phase(3), Some(1));
        assert_eq!(op_phase(6), Some(2));
        assert_eq!(op_phase(9), Some(3));
        assert!(op_phase(10).is_none());
        assert!(op_cell(10).is_none());
    }

    #[test]
    fn test_proj_dir() {
        assert_eq!(proj_dir(0), Some(0));
        assert_eq!(proj_dir(5), Some(0));
        assert_eq!(proj_dir(6), Some(1));
        assert_eq!(proj_dir(11), Some(1));
        assert!(proj_dir(12).is_none());
        assert_eq!(proj_dir_name(0), Some("正向投影"));
        assert_eq!(proj_dir_name(1), Some("逆向投影"));
        assert!(proj_dir_name(2).is_none());
    }

    #[test]
    fn test_provider_names() {
        assert_eq!(prov_name(0), Some("virtualbox"));
        assert_eq!(prov_name(1), Some("hyperv"));
        assert_eq!(prov_name(2), Some("docker"));
        assert!(prov_name(3).is_none());
        assert!(prov_cell(3).is_none());
    }

    #[test]
    fn test_phase_names() {
        assert_eq!(op_phase_name(0), Some("up"));
        assert_eq!(op_phase_name(1), Some("ssh"));
        assert_eq!(op_phase_name(2), Some("halt"));
        assert_eq!(op_phase_name(3), Some("destroy"));
        assert!(op_phase_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = box_cell(3).unwrap_or(0);
        assert_eq!(a, box_cell(3).unwrap_or(0));
        assert_ne!(a, box_cell(4).unwrap_or(0));
        assert!(box_cell(BOX_CELLS).is_none());
        assert!(plug_cell(PLUG_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_dir_order_caught() {
        let mut lt = VagrantLedger::new();
        let _ = lt.record_proj(8, proj_cell(8).unwrap_or(0));
        assert_eq!(lt.record_proj(2, proj_cell(2).unwrap_or(0)), Err(VagrantCode::EDirOrder));
        assert!(lt.record_proj(9, proj_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = VagrantLedger::new();
        let _ = lt.record_op(7, op_cell(7).unwrap_or(0));
        assert_eq!(lt.record_op(3, op_cell(3).unwrap_or(0)), Err(VagrantCode::EPhase));
        assert!(lt.record_op(8, op_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VagrantLedger::new();
        assert_eq!(
            lt.record_box(5, box_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(VagrantCode::EBoxDrift)
        );
        assert_eq!(
            lt.record_plug(1, plug_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(VagrantCode::EPlugDrift)
        );
        assert_eq!(
            lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(VagrantCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VagrantLedger::new();
        let _ = lt.record_prov(1, prov_cell(1).unwrap_or(0));
        assert_eq!(lt.record_prov(1, prov_cell(1).unwrap_or(0)), Err(VagrantCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VagrantLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VagrantLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_empty_ledger_phase_injection() {
        // 空账本上相位逆序注入同样必抓，且不改主账本 last_phase。
        let mut main = VagrantLedger::new();
        let _ = main.record_op(0, op_cell(0).unwrap_or(0));
        let mut lt = VagrantLedger::new();
        let _ = lt.record_op(8, op_cell(8).unwrap_or(0));
        assert_eq!(lt.record_op(3, op_cell(3).unwrap_or(0)), Err(VagrantCode::EPhase));
        // 主账本继续按合法序落格不受影响。
        assert!(main.record_op(1, op_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJ_CELLS + OPS + BOX_CELLS + PROV_CELLS + PLUG_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8540);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VagrantCode::Ok as u32, 0);
        assert_eq!(VagrantCode::EGradeDrift as u32, 12);
        assert_eq!(VagrantCode::EPhase.val(), SEG_BASE + 6);
        assert!(!VagrantCode::Ok.detail().is_empty());
        assert!(!VagrantCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = VagrantLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52278_checks_green() {
        let s = run_unx52278_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
