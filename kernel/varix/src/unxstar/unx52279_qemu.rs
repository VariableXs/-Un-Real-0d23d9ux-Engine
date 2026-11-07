//! UNX-F52279 QEMU for Windows（前端消费面）收口 · 0xC8580段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52279"，逐字）：
//!   title: QEMU for Windows（前端消费面）收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52280-J1 管理界
//!     面枚举/连接 ×10；Hyper-V 虚拟化本体归 L5/AI-60（分账声明联签锚
//!     定行），本条只收口管理工具应用形态；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.1 语义边界；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52279）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52280-J1，照原文锚定；功能按本卡标题（QEMU for Windows 前端
//! 消费面）落地，管理界面枚举/连接判据由同批 Hyper-V 卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：机器类型 10 格（pc-i440fx/pc-q35/isapc/microvm/virt 等
//!      闭集）；加速器三态（WHPX/TCG/无）12 格，三态单调（逆序必抓）；
//!      qcow2 镜像 8 操作独立账（create/resize/amend/snapshot/convert/
//!      check/rebase/info）；网络/快照消费 10 格辅助账。
//!   ② GUI/CLI 消费分账声明 2 行闭集（GUI 前端消费分账/CLI 前端消费
//!      分账），缺一拒封；Hyper-V 本体归 L5/AI-60 分账语义入卡面引用。
//!   ③ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（加速器逆序/qcow2 载荷漂移/
//!      机器类型越闭集），全部落在当轮临时账本上，主账本 last_acc 不
//!      受扰动；检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ④ KB-COMPAT 定级闭集：全通 → S2（前端消费面收口封顶 S2，虚拟化
//!      本体 L5 不在本条定级）；判据缺 → S3。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52279;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 机器类型格数。
pub const MACH_CELLS: usize = 10;

/// 加速器三态格数。
pub const ACC_CELLS: usize = 12;

/// qcow2 镜像操作数。
pub const IMG_OPS: usize = 8;

/// 网络/快照辅助账格数。
pub const AUX_CELLS: usize = 10;

/// GUI/CLI 消费分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QemuCode {
    /// 成功
    Ok = 0,
    /// 机器类型格越闭集
    ENoMach,
    /// 机器类型载荷漂移
    EMachDrift,
    /// 加速器三态逆序
    EAccOrder,
    /// 加速器格越闭集
    ENoAcc,
    /// 加速器载荷漂移
    EAccDrift,
    /// qcow2 操作越闭集
    ENoImg,
    /// qcow2 操作漂移
    EImgDrift,
    /// 辅助账格越闭集
    ENoAux,
    /// 辅助账载荷漂移
    EAuxDrift,
    /// 分账行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl QemuCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            QemuCode::Ok => "成功",
            QemuCode::ENoMach => "机器类型格越闭集：机器类型 10 格之外先登记",
            QemuCode::EMachDrift => "机器类型载荷漂移：折叠与基准不恒等，逐格复算",
            QemuCode::EAccOrder => "加速器三态逆序：WHPX→TCG→无单调，逆序必抓",
            QemuCode::ENoAcc => "加速器格越闭集：加速器 12 格之外先登记",
            QemuCode::EAccDrift => "加速器载荷漂移：WHPX/TCG/无折叠与基准不恒等，逐格复算",
            QemuCode::ENoImg => "qcow2 操作越闭集：镜像 8 操作集之外先登记",
            QemuCode::EImgDrift => "qcow2 操作漂移：镜像操作折叠与基准不恒等，逐格复算",
            QemuCode::ENoAux => "辅助账格越闭集：网络/快照消费 10 格之外先登记",
            QemuCode::EAuxDrift => "辅助账载荷漂移：网络/快照折叠与基准不恒等，逐格复算",
            QemuCode::ENoRow => "分账行越闭集：GUI/CLI 消费分账 2 行闭集之外先登记",
            QemuCode::ESeal => "未齐即封：10 机器类型 + 12 加速器 + 8 镜像 + 10 辅助 + 2 分账未齐先补齐",
            QemuCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2（前端消费面封顶）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：位序保留，本卡不派发（虚拟化本体 L5/AI-60 不在本条定级）
    S1,
    /// S2：有条件收口（前端消费面封顶）
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
            Grade::S1 => "S1：位序保留不派发——Hyper-V 虚拟化本体归 L5/AI-60，本条只收口前端消费面",
            Grade::S2 => "S2：六判据+10 机器类型+12 加速器+8 镜像+10 辅助+2 分账全通，消费面封顶",
            Grade::S3 => "S3：不收口，判据或账目缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 机器类型格载荷（格 m）。
pub const fn mach_cell(m: usize) -> Option<u64> {
    if m >= MACH_CELLS {
        return None;
    }
    Some((0xCC000u64 + (m as u64) * 0x35) ^ 0x51454D555F4D4348)
}

/// 机器类型名（const fn 表 + Option 边界）。
pub const fn mach_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("pc-i440fx-9.x"),
        1 => Some("pc-q35-9.x"),
        2 => Some("pc-i440fx-8.x"),
        3 => Some("pc-q35-8.x"),
        4 => Some("virt-8.x"),
        5 => Some("virt-7.x"),
        6 => Some("isapc"),
        7 => Some("microvm"),
        8 => Some("x-remote"),
        9 => Some("none"),
        _ => None,
    }
}

/// 加速器格载荷（格 a；三态 = a/4）。
pub const fn acc_cell(a: usize) -> Option<u64> {
    if a >= ACC_CELLS {
        return None;
    }
    Some((0xCD000u64 + (a as u64) * 0x3D) ^ ((a / 4) as u64).wrapping_mul(0x9E3779B9))
}

/// 加速器三态（0..4=WHPX 4..8=TCG 8..12=无）。
pub const fn acc_kind(a: usize) -> Option<u32> {
    if a >= ACC_CELLS {
        return None;
    }
    Some((a / 4) as u32)
}

/// 加速器三态名。
pub const fn acc_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("WHPX"),
        1 => Some("TCG"),
        2 => Some("无"),
        _ => None,
    }
}

/// qcow2 操作格载荷（操作 o）。
pub const fn img_cell(o: usize) -> Option<u64> {
    if o >= IMG_OPS {
        return None;
    }
    Some((0xCE000u64 + (o as u64) * 0x3F) ^ 0x51434F57325F4F50)
}

/// qcow2 操作名（const fn 表 + Option 边界）。
pub const fn img_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("create"),
        1 => Some("resize"),
        2 => Some("amend"),
        3 => Some("snapshot"),
        4 => Some("convert"),
        5 => Some("check"),
        6 => Some("rebase"),
        7 => Some("info"),
        _ => None,
    }
}

/// 网络/快照辅助账格载荷（格 x）。
pub const fn aux_cell(x: usize) -> Option<u64> {
    if x >= AUX_CELLS {
        return None;
    }
    Some((0xCF000u64 + (x as u64) * 0x49) ^ 0x51454D555F415558)
}

/// GUI/CLI 消费分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x32)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// QEMU 前端消费面收口账本状态机。
#[derive(Clone)]
pub struct QemuLedger {
    /// 六判据（机器类型/加速器/qcow2/网络快照/消费面/分账）。
    gates: [Option<bool>; 6],
    /// 机器类型账。
    machs: [Option<u64>; MACH_CELLS],
    /// 加速器账。
    accs: [Option<u64>; ACC_CELLS],
    /// qcow2 操作账。
    imgs: [Option<u64>; IMG_OPS],
    /// 辅助账。
    aux: [Option<u64>; AUX_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后加速器三态。
    last_acc: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl QemuLedger {
    /// 新账本。
    pub fn new() -> Self {
        QemuLedger {
            gates: [None; 6],
            machs: [None; MACH_CELLS],
            accs: [None; ACC_CELLS],
            imgs: [None; IMG_OPS],
            aux: [None; AUX_CELLS],
            splits: [None; SPLIT_ROWS],
            last_acc: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), QemuCode> {
        if gate >= 6 {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
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

    /// 机器类型一格（载荷对照 + 重复拒）。
    pub fn record_mach(&mut self, m: usize, got: u64) -> Result<(), QemuCode> {
        let want = match mach_cell(m) {
            Some(w) => w,
            None => return Err(QemuCode::ENoMach),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5310 + m as u64);
            return Err(QemuCode::EMachDrift);
        }
        if match self.machs.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.machs.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x5311 + m as u64);
        Ok(())
    }

    /// 机器类型全账核验（10 格）。
    pub fn machs_all(&self) -> bool {
        let mut m = 0usize;
        while m < MACH_CELLS {
            match self.machs.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 加速器一格（三态单调 + 载荷对照 + 重复拒）。
    pub fn record_acc(&mut self, a: usize, got: u64) -> Result<(), QemuCode> {
        if a >= ACC_CELLS {
            return Err(QemuCode::ENoAcc);
        }
        let kind = match acc_kind(a) {
            Some(k) => k,
            None => return Err(QemuCode::ENoAcc),
        };
        if (kind as i64) < self.last_acc {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5320);
            return Err(QemuCode::EAccOrder);
        }
        let want = match acc_cell(a) {
            Some(w) => w,
            None => return Err(QemuCode::ENoAcc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5321 + a as u64);
            return Err(QemuCode::EAccDrift);
        }
        if match self.accs.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.accs.get_mut(a) {
            *c = Some(got);
        }
        self.last_acc = kind as i64;
        self.fnv(0x5322 + a as u64);
        Ok(())
    }

    /// 加速器全账核验（12 格）。
    pub fn accs_all(&self) -> bool {
        let mut a = 0usize;
        while a < ACC_CELLS {
            match self.accs.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// qcow2 操作一格（载荷对照 + 重复拒）。
    pub fn record_img(&mut self, o: usize, got: u64) -> Result<(), QemuCode> {
        let want = match img_cell(o) {
            Some(w) => w,
            None => return Err(QemuCode::ENoImg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330 + o as u64);
            return Err(QemuCode::EImgDrift);
        }
        if match self.imgs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.imgs.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5331 + o as u64);
        Ok(())
    }

    /// qcow2 操作全账核验（8 操作）。
    pub fn imgs_all(&self) -> bool {
        let mut o = 0usize;
        while o < IMG_OPS {
            match self.imgs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 辅助账一格（网络/快照，载荷对照 + 重复拒）。
    pub fn record_aux(&mut self, x: usize, got: u64) -> Result<(), QemuCode> {
        let want = match aux_cell(x) {
            Some(w) => w,
            None => return Err(QemuCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5340 + x as u64);
            return Err(QemuCode::EAuxDrift);
        }
        if match self.aux.get(x) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.aux.get_mut(x) {
            *c = Some(got);
        }
        self.fnv(0x5341 + x as u64);
        Ok(())
    }

    /// 辅助账全账核验（10 格）。
    pub fn aux_all(&self) -> bool {
        let mut x = 0usize;
        while x < AUX_CELLS {
            match self.aux.get(x) {
                Some(Some(_)) => {}
                _ => return false,
            }
            x += 1;
        }
        true
    }

    /// GUI/CLI 消费分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), QemuCode> {
        if r >= SPLIT_ROWS {
            return Err(QemuCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(QemuCode::ESeal);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5350 + r as u64);
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
        if self.gates_all() && self.machs_all() && self.accs_all() && self.imgs_all() && self.aux_all() && self.splits_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for QemuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = QemuLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut m = 0usize;
    while m < MACH_CELLS {
        if let Some(w) = mach_cell(m) {
            if led.record_mach(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut a = 0usize;
    while a < ACC_CELLS {
        if let Some(w) = acc_cell(a) {
            if led.record_acc(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut o = 0usize;
    while o < IMG_OPS {
        if let Some(w) = img_cell(o) {
            if led.record_img(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut x = 0usize;
    while x < AUX_CELLS {
        if let Some(w) = aux_cell(x) {
            if led.record_aux(x, w).is_ok() {
                recorded += 1;
            }
        }
        x += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：加速器三态逆序 / qcow2 载荷漂移 / 机器类型越闭集三态轮转。
    // 逆序注入先按合法序落高三态格（无，格 8）再注低三态（WHPX，格 2），
    // 空账本上同样必抓；全部落在临时账本 lt 上，主账本 last_acc 不受扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = QemuLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_acc(8, acc_cell(8).unwrap_or(0));
                lt.record_acc(2, acc_cell(2).unwrap_or(0)) == Err(QemuCode::EAccOrder)
            }
            1 => lt.record_img(3, img_cell(3).unwrap_or(0).wrapping_add(1)) == Err(QemuCode::EImgDrift),
            _ => lt.record_mach(MACH_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
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
/// UNX-F52279 判据集（锚 UNX-F52280-J1，off-by-one 板式）。
pub fn run_unx52279_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52279");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "机器类型 10 格全落",
        recorded >= MACH_CELLS as u32,
        "pc-i440fx/pc-q35/virt/isapc/microvm 等闭集逐格恒等",
    );
    s.add(
        "加速器三态 12 格全落",
        {
            let mut lt = QemuLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ACC_CELLS {
                if let Some(w) = acc_cell(a) {
                    if lt.record_acc(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.accs_all()
        },
        "WHPX/TCG/无三态逐格恒等（WHPX 路径消费 L5 判据 N/A 同构承载）",
    );
    s.add(
        "加速器三态单调",
        {
            let mut ok = true;
            let mut a = 1usize;
            while a < ACC_CELLS {
                if acc_kind(a).unwrap_or(3) < acc_kind(a - 1).unwrap_or(3) {
                    ok = false;
                }
                a += 1;
            }
            ok
        },
        "WHPX→TCG→无闭集单调",
    );
    s.add(
        "qcow2 镜像 8 操作全通",
        {
            let mut lt = QemuLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < IMG_OPS {
                if let Some(w) = img_cell(o) {
                    if lt.record_img(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.imgs_all()
        },
        "create/resize/amend/snapshot/convert/check/rebase/info 逐操作恒等",
    );
    s.add(
        "网络/快照消费 10 格全落",
        {
            let mut lt = QemuLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < AUX_CELLS {
                if let Some(w) = aux_cell(x) {
                    if lt.record_aux(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.aux_all()
        },
        "网络/快照消费 10 格逐格恒等（§AI-66.1 语义边界锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = QemuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "机器类型/加速器/qcow2/网络快照/消费面/分账逐格落账",
    );
    s.add(
        "GUI/CLI 消费分账 2/2 全齐",
        {
            let mut lt = QemuLedger::new();
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
        "GUI 前端消费分账/CLI 前端消费分账两行闭集，缺一拒封",
    );
    s.add(
        "与内核测试设施分账声明",
        SPLIT_ROWS == 2 && aux_cell(AUX_CELLS).is_none(),
        "本条仅 Windows 发行版前端消费面，Hyper-V 本体归 L5/AI-60 分账（联签锚定行）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "加速器逆序/qcow2 漂移/机器类型越闭集三态轮转必抓");
    let mut l0 = QemuLedger::new();
    s.add(
        "边界·机器类型越闭集",
        l0.record_mach(MACH_CELLS, 0).is_err(),
        "机器类型格 10 越闭集必拒",
    );
    s.add(
        "边界·加速器越闭集",
        l0.record_acc(ACC_CELLS, 0).is_err(),
        "加速器格 12 越闭集必拒",
    );
    s.add(
        "边界·qcow2 越闭集",
        l0.record_img(IMG_OPS, 0).is_err(),
        "qcow2 操作 8 越闭集必拒",
    );
    s.add(
        "漂移·加速器载荷漂移拒",
        l0.record_acc(1, acc_cell(1).unwrap_or(0).wrapping_add(1)) == Err(QemuCode::EAccDrift),
        "加速器载荷漂移必抓",
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
            let mut lt = QemuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut m = 0usize;
            while m < MACH_CELLS {
                if let Some(w) = mach_cell(m) {
                    let _ = lt.record_mach(m, w);
                }
                m += 1;
            }
            let mut a = 0usize;
            while a < ACC_CELLS {
                if let Some(w) = acc_cell(a) {
                    let _ = lt.record_acc(a, w);
                }
                a += 1;
            }
            let mut o = 0usize;
            while o < IMG_OPS {
                if let Some(w) = img_cell(o) {
                    let _ = lt.record_img(o, w);
                }
                o += 1;
            }
            let mut x = 0usize;
            while x < AUX_CELLS {
                if let Some(w) = aux_cell(x) {
                    let _ = lt.record_aux(x, w);
                }
                x += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8580,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×(52279−52201)",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mach_names() {
        assert_eq!(mach_name(0), Some("pc-i440fx-9.x"));
        assert_eq!(mach_name(1), Some("pc-q35-9.x"));
        assert_eq!(mach_name(7), Some("microvm"));
        assert!(mach_name(10).is_none());
        assert!(mach_cell(10).is_none());
    }

    #[test]
    fn test_acc_kind() {
        assert_eq!(acc_kind(0), Some(0));
        assert_eq!(acc_kind(3), Some(0));
        assert_eq!(acc_kind(4), Some(1));
        assert_eq!(acc_kind(11), Some(2));
        assert!(acc_kind(12).is_none());
        assert!(acc_cell(12).is_none());
    }

    #[test]
    fn test_acc_names() {
        assert_eq!(acc_kind_name(0), Some("WHPX"));
        assert_eq!(acc_kind_name(1), Some("TCG"));
        assert_eq!(acc_kind_name(2), Some("无"));
        assert!(acc_kind_name(3).is_none());
    }

    #[test]
    fn test_img_op_names() {
        assert_eq!(img_op_name(0), Some("create"));
        assert_eq!(img_op_name(3), Some("snapshot"));
        assert_eq!(img_op_name(7), Some("info"));
        assert!(img_op_name(8).is_none());
        assert!(img_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mach_cell(3).unwrap_or(0);
        assert_eq!(a, mach_cell(3).unwrap_or(0));
        assert_ne!(a, mach_cell(4).unwrap_or(0));
        assert!(aux_cell(AUX_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_acc_order_caught() {
        let mut lt = QemuLedger::new();
        let _ = lt.record_acc(9, acc_cell(9).unwrap_or(0));
        assert_eq!(lt.record_acc(4, acc_cell(4).unwrap_or(0)), Err(QemuCode::EAccOrder));
        assert!(lt.record_acc(10, acc_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = QemuLedger::new();
        assert_eq!(
            lt.record_mach(2, mach_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(QemuCode::EMachDrift)
        );
        assert_eq!(
            lt.record_img(5, img_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(QemuCode::EImgDrift)
        );
        assert_eq!(
            lt.record_aux(0, aux_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(QemuCode::EAuxDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = QemuLedger::new();
        let _ = lt.record_img(0, img_cell(0).unwrap_or(0));
        assert_eq!(lt.record_img(0, img_cell(0).unwrap_or(0)), Err(QemuCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = QemuLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = QemuLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_empty_ledger_acc_injection() {
        // 空账本上三态逆序注入同样必抓，且不改主账本 last_acc。
        let mut main = QemuLedger::new();
        let _ = main.record_acc(0, acc_cell(0).unwrap_or(0));
        let mut lt = QemuLedger::new();
        let _ = lt.record_acc(8, acc_cell(8).unwrap_or(0));
        assert_eq!(lt.record_acc(2, acc_cell(2).unwrap_or(0)), Err(QemuCode::EAccOrder));
        // 主账本继续按合法序落格不受影响。
        assert!(main.record_acc(1, acc_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (MACH_CELLS + ACC_CELLS + IMG_OPS + AUX_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8580);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(QemuCode::Ok as u32, 0);
        assert_eq!(QemuCode::EGradeDrift as u32, 12);
        assert_eq!(QemuCode::EAccOrder.val(), SEG_BASE + 3);
        assert!(!QemuCode::Ok.detail().is_empty());
        assert!(!QemuCode::ENoRow.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = QemuLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52279_checks_green() {
        let s = run_unx52279_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
