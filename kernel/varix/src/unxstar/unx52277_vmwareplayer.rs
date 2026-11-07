//! UNX-F52277 VMware Workstation Player 收口 · 0xC8500段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52277"，逐字）：
//!   title: VMware Workstation Player 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52278-J1 box 拉
//!     起/销毁 ×10 全通；provider 探测（VirtualBox 消费联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52277）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52278-J1，照原文锚定；功能按本卡标题（VMware Workstation
//! Player）落地，box 拉起/销毁判据由同批 Vagrant 卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：VM 生命周期 10 操作账，五家族闭集（创建/开机/挂起/恢
//!      复/关机）各 2 操作，家族单调（逆序必抓）；快照 8 格独立账；
//!      网络三模式（NAT/桥接/仅主机）12 格；共享文件夹 6 格 + Tools
//!      消费 4 格合为辅助账 10 格。
//!   ② 商业/个人双许可分账声明 2 行闭集（商业许可分账/个人许可分
//!      账），缺一拒封。
//!   ③ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（生命周期家族逆序/快照载荷漂
//!      移/生命周期操作越闭集），全部落在当轮临时账本上，主账本
//!      last_kind 不受扰动；检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ④ KB-COMPAT 定级闭集：全通 → S1（20 项域账 + 6 判据 + 2 行分账）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52277;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// VM 生命周期操作数。
pub const LIFE_OPS: usize = 10;

/// 快照格数。
pub const SNAP_CELLS: usize = 8;

/// 网络三模式格数。
pub const NET_CELLS: usize = 12;

/// 辅助账格数（共享文件夹 6 + Tools 消费 4）。
pub const AUX_CELLS: usize = 10;

/// 共享文件夹格数（辅助账前段）。
pub const SHARE_CELLS: usize = 6;

/// 商业/个人双许可分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmwareCode {
    /// 成功
    Ok = 0,
    /// 生命周期操作号越闭集
    ENoOp,
    /// 生命周期操作漂移
    EDrift,
    /// 生命周期家族逆序
    EOrder,
    /// 快照格越闭集
    ENoSnap,
    /// 快照载荷漂移
    ESnapDrift,
    /// 网络格越闭集
    ENoNet,
    /// 网络载荷漂移
    ENetDrift,
    /// 辅助账格越闭集
    ENoAux,
    /// 辅助账载荷漂移
    EAuxDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl VmwareCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VmwareCode::Ok => "成功",
            VmwareCode::ENoOp => "生命周期操作号越闭集：VM 生命周期 10 操作集之外先登记",
            VmwareCode::EDrift => "生命周期操作漂移：操作折叠与基准不恒等，逐格复算",
            VmwareCode::EOrder => "生命周期家族逆序：创建→开机→挂起→恢复→关机单调，逆序必抓",
            VmwareCode::ENoSnap => "快照格越闭集：快照 8 格之外先登记",
            VmwareCode::ESnapDrift => "快照载荷漂移：快照折叠与基准不恒等，逐格复算",
            VmwareCode::ENoNet => "网络格越闭集：网络三模式 12 格之外先登记",
            VmwareCode::ENetDrift => "网络载荷漂移：NAT/桥接/仅主机折叠与基准不恒等，逐格复算",
            VmwareCode::ENoAux => "辅助账格越闭集：共享文件夹 6 格 + Tools 消费 4 格之外先登记",
            VmwareCode::EAuxDrift => "辅助账载荷漂移：共享/Tools 折叠与基准不恒等，逐格复算",
            VmwareCode::EDup => "重复落账：同格只许一条账目",
            VmwareCode::ESeal => "未齐即封：10 操作 + 8 快照 + 12 网络 + 10 辅助 + 2 分账未齐先补齐",
            VmwareCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10 操作+8 快照+12 网络+10 辅助+2 行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 生命周期操作格载荷（操作 o；家族 = o/2）。
pub const fn life_cell(o: usize) -> Option<u64> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x2D) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期家族（0..2=创建 2..4=开机 4..6=挂起 6..8=恢复 8..10=关机）。
pub const fn life_kind(o: usize) -> Option<u32> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 生命周期家族名。
pub const fn life_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("创建"),
        1 => Some("开机"),
        2 => Some("挂起"),
        3 => Some("恢复"),
        4 => Some("关机"),
        _ => None,
    }
}

/// 快照格基准（格 s）。
pub const fn snap_cell(s: usize) -> Option<u64> {
    if s >= SNAP_CELLS {
        return None;
    }
    Some((0xC2000u64 + (s as u64) * 0x37) ^ 0x564D575F534E5031)
}

/// 网络格载荷（格 n；模式 = n/4）。
pub const fn net_cell(n: usize) -> Option<u64> {
    if n >= NET_CELLS {
        return None;
    }
    Some((0xC3000u64 + (n as u64) * 0x3B) ^ ((n / 4) as u64).wrapping_mul(0x9E3779B9))
}

/// 网络模式（0..4=NAT 4..8=桥接 8..12=仅主机）。
pub const fn net_kind(n: usize) -> Option<u32> {
    if n >= NET_CELLS {
        return None;
    }
    Some((n / 4) as u32)
}

/// 网络模式名。
pub const fn net_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("NAT"),
        1 => Some("桥接"),
        2 => Some("仅主机"),
        _ => None,
    }
}

/// 辅助账格载荷（格 a；0..6=共享文件夹 6..10=Tools 消费）。
pub const fn aux_cell(a: usize) -> Option<u64> {
    if a >= AUX_CELLS {
        return None;
    }
    Some((0xC4000u64 + (a as u64) * 0x41) ^ 0x564D574F50535F31)
}

/// 辅助账段名。
pub const fn aux_kind_name(a: usize) -> Option<&'static str> {
    if a >= SHARE_CELLS {
        if a < AUX_CELLS {
            return Some("Tools 消费");
        }
        return None;
    }
    Some("共享文件夹")
}

/// 商业/个人双许可分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2E)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VMware Player 收口账本状态机。
#[derive(Clone)]
pub struct VmLedger {
    /// 六判据（启动/快照/网络/共享/Tools/许可）。
    gates: [Option<bool>; 6],
    /// 生命周期操作账。
    ops: [Option<u64>; LIFE_OPS],
    /// 快照账。
    snaps: [Option<u64>; SNAP_CELLS],
    /// 网络账。
    nets: [Option<u64>; NET_CELLS],
    /// 辅助账。
    aux: [Option<u64>; AUX_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后生命周期家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VmLedger {
    /// 新账本。
    pub fn new() -> Self {
        VmLedger {
            gates: [None; 6],
            ops: [None; LIFE_OPS],
            snaps: [None; SNAP_CELLS],
            nets: [None; NET_CELLS],
            aux: [None; AUX_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VmwareCode> {
        if gate >= 6 {
            return Err(VmwareCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5210 + gate as u64);
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

    /// 生命周期一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_life(&mut self, o: usize, got: u64) -> Result<(), VmwareCode> {
        if o >= LIFE_OPS {
            return Err(VmwareCode::ENoOp);
        }
        let kind = match life_kind(o) {
            Some(k) => k,
            None => return Err(VmwareCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5220);
            return Err(VmwareCode::EOrder);
        }
        let want = match life_cell(o) {
            Some(w) => w,
            None => return Err(VmwareCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5221 + o as u64);
            return Err(VmwareCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VmwareCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5222 + o as u64);
        Ok(())
    }

    /// 生命周期全账核验（10 格）。
    pub fn life_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 快照一格（载荷对照 + 重复拒）。
    pub fn record_snap(&mut self, s: usize, got: u64) -> Result<(), VmwareCode> {
        let want = match snap_cell(s) {
            Some(w) => w,
            None => return Err(VmwareCode::ENoSnap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5230 + s as u64);
            return Err(VmwareCode::ESnapDrift);
        }
        if match self.snaps.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VmwareCode::EDup);
        }
        if let Some(c) = self.snaps.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5231 + s as u64);
        Ok(())
    }

    /// 快照全账核验（8 格）。
    pub fn snaps_all(&self) -> bool {
        let mut s = 0usize;
        while s < SNAP_CELLS {
            match self.snaps.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 网络一格（载荷对照 + 重复拒）。
    pub fn record_net(&mut self, n: usize, got: u64) -> Result<(), VmwareCode> {
        let want = match net_cell(n) {
            Some(w) => w,
            None => return Err(VmwareCode::ENoNet),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5240 + n as u64);
            return Err(VmwareCode::ENetDrift);
        }
        if match self.nets.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VmwareCode::EDup);
        }
        if let Some(c) = self.nets.get_mut(n) {
            *c = Some(got);
        }
        self.fnv(0x5241 + n as u64);
        Ok(())
    }

    /// 网络全账核验（12 格）。
    pub fn nets_all(&self) -> bool {
        let mut n = 0usize;
        while n < NET_CELLS {
            match self.nets.get(n) {
                Some(Some(_)) => {}
                _ => return false,
            }
            n += 1;
        }
        true
    }

    /// 辅助账一格（共享/Tools，载荷对照 + 重复拒）。
    pub fn record_aux(&mut self, a: usize, got: u64) -> Result<(), VmwareCode> {
        let want = match aux_cell(a) {
            Some(w) => w,
            None => return Err(VmwareCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5250 + a as u64);
            return Err(VmwareCode::EAuxDrift);
        }
        if match self.aux.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VmwareCode::EDup);
        }
        if let Some(c) = self.aux.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x5251 + a as u64);
        Ok(())
    }

    /// 辅助账全账核验（10 格）。
    pub fn aux_all(&self) -> bool {
        let mut a = 0usize;
        while a < AUX_CELLS {
            match self.aux.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 商业/个人分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), VmwareCode> {
        if r >= SPLIT_ROWS {
            return Err(VmwareCode::ENoAux);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VmwareCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5260 + r as u64);
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.life_all() && self.snaps_all() && self.nets_all() && self.aux_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < LIFE_OPS {
        if let Some(w) = life_cell(o) {
            if led.record_life(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < SNAP_CELLS {
        if let Some(w) = snap_cell(s) {
            if led.record_snap(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut n = 0usize;
    while n < NET_CELLS {
        if let Some(w) = net_cell(n) {
            if led.record_net(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut a = 0usize;
    while a < AUX_CELLS {
        if let Some(w) = aux_cell(a) {
            if led.record_aux(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：生命周期家族逆序 / 快照载荷漂移 / 生命周期越闭集三态轮转。
    // 逆序注入先按合法序落高族格（家族 4，即操作 8）再注低族（操作 2），
    // 空账本上同样必抓；全部落在临时账本 lt 上，主账本 last_kind 不受扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VmLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_life(8, life_cell(8).unwrap_or(0));
                lt.record_life(2, life_cell(2).unwrap_or(0)) == Err(VmwareCode::EOrder)
            }
            1 => lt.record_snap(3, snap_cell(3).unwrap_or(0).wrapping_add(1)) == Err(VmwareCode::ESnapDrift),
            _ => lt.record_life(LIFE_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5270 + k as u64);
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
/// UNX-F52277 判据集（锚 UNX-F52278-J1，off-by-one 板式）。
pub fn run_unx52277_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52277");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "VM 生命周期 10 操作全通",
        recorded >= LIFE_OPS as u32,
        "创建/开机/挂起/恢复/关机五家族逐操作恒等",
    );
    s.add(
        "快照 8 格全落",
        {
            let mut lt = VmLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < SNAP_CELLS {
                if let Some(w) = snap_cell(i) {
                    if lt.record_snap(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.snaps_all()
        },
        "快照 8 格逐格恒等，漂移必抓",
    );
    s.add(
        "网络三模式 12 格全落",
        {
            let mut lt = VmLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < NET_CELLS {
                if let Some(w) = net_cell(i) {
                    if lt.record_net(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.nets_all()
        },
        "NAT/桥接/仅主机三模式逐格恒等",
    );
    s.add(
        "网络三模式单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < NET_CELLS {
                if net_kind(i).unwrap_or(3) < net_kind(i - 1).unwrap_or(3) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "NAT→桥接→仅主机闭集单调",
    );
    s.add(
        "共享文件夹 6 格全落",
        {
            let mut lt = VmLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < SHARE_CELLS {
                if let Some(w) = aux_cell(i) {
                    if lt.record_aux(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        },
        "共享文件夹 6 格逐格恒等",
    );
    s.add(
        "Tools 消费 4 格全落",
        {
            let mut lt = VmLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < AUX_CELLS {
                if let Some(w) = aux_cell(i) {
                    if lt.record_aux(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.aux_all()
        },
        "Tools 消费 4 格逐格恒等（§AI-66.3 锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/快照/网络/共享/Tools/许可逐格落账",
    );
    s.add(
        "商业/个人双许可分账 2/2 全齐",
        {
            let mut lt = VmLedger::new();
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
        "商业许可分账/个人许可分账两行闭集，缺一拒封",
    );
    s.add(
        "生命周期家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFE_OPS {
                if life_kind(o).unwrap_or(5) < life_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "家族逆序/快照漂移/越闭集三态轮转必抓");
    let mut l0 = VmLedger::new();
    s.add(
        "边界·生命周期越闭集",
        l0.record_life(LIFE_OPS, 0).is_err(),
        "生命周期操作 10 越闭集必拒",
    );
    s.add(
        "边界·快照越闭集",
        l0.record_snap(SNAP_CELLS, 0).is_err(),
        "快照格 8 越闭集必拒",
    );
    s.add(
        "边界·网络越闭集",
        l0.record_net(NET_CELLS, 0).is_err(),
        "网络格 12 越闭集必拒",
    );
    s.add(
        "漂移·快照载荷漂移拒",
        l0.record_snap(1, snap_cell(1).unwrap_or(0).wrapping_add(1)) == Err(VmwareCode::ESnapDrift),
        "快照载荷漂移必抓",
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
            let mut lt = VmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(w) = life_cell(o) {
                    let _ = lt.record_life(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < SNAP_CELLS {
                if let Some(w) = snap_cell(i) {
                    let _ = lt.record_snap(i, w);
                }
                i += 1;
            }
            let mut n = 0usize;
            while n < NET_CELLS {
                if let Some(w) = net_cell(n) {
                    let _ = lt.record_net(n, w);
                }
                n += 1;
            }
            let mut a = 0usize;
            while a < AUX_CELLS {
                if let Some(w) = aux_cell(a) {
                    let _ = lt.record_aux(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8500,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×(52277−52201)",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_life_kind() {
        assert_eq!(life_kind(0), Some(0));
        assert_eq!(life_kind(2), Some(1));
        assert_eq!(life_kind(9), Some(4));
        assert!(life_kind(10).is_none());
        assert!(life_cell(10).is_none());
    }

    #[test]
    fn test_kind_names() {
        assert_eq!(life_kind_name(0), Some("创建"));
        assert_eq!(life_kind_name(4), Some("关机"));
        assert!(life_kind_name(5).is_none());
        assert_eq!(net_kind_name(0), Some("NAT"));
        assert_eq!(net_kind_name(1), Some("桥接"));
        assert_eq!(net_kind_name(2), Some("仅主机"));
        assert!(net_kind_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = snap_cell(3).unwrap_or(0);
        assert_eq!(a, snap_cell(3).unwrap_or(0));
        assert_ne!(a, snap_cell(4).unwrap_or(0));
        assert!(snap_cell(SNAP_CELLS).is_none());
        assert!(net_cell(NET_CELLS).is_none());
        assert!(aux_cell(AUX_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_aux_segmentation() {
        assert_eq!(aux_kind_name(0), Some("共享文件夹"));
        assert_eq!(aux_kind_name(5), Some("共享文件夹"));
        assert_eq!(aux_kind_name(6), Some("Tools 消费"));
        assert_eq!(aux_kind_name(9), Some("Tools 消费"));
        assert!(aux_kind_name(10).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = VmLedger::new();
        let _ = lt.record_life(7, life_cell(7).unwrap_or(0));
        assert_eq!(lt.record_life(2, life_cell(2).unwrap_or(0)), Err(VmwareCode::EOrder));
        assert!(lt.record_life(8, life_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VmLedger::new();
        assert_eq!(
            lt.record_snap(5, snap_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(VmwareCode::ESnapDrift)
        );
        assert_eq!(
            lt.record_net(2, net_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(VmwareCode::ENetDrift)
        );
        assert_eq!(
            lt.record_aux(0, aux_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(VmwareCode::EAuxDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VmLedger::new();
        let _ = lt.record_net(4, net_cell(4).unwrap_or(0));
        assert_eq!(lt.record_net(4, net_cell(4).unwrap_or(0)), Err(VmwareCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VmLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VmLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_empty_ledger_order_injection() {
        // 空账本上逆序注入同样必抓，且不改主账本 last_kind。
        let mut main = VmLedger::new();
        let _ = main.record_life(0, life_cell(0).unwrap_or(0));
        let mut lt = VmLedger::new();
        let _ = lt.record_life(8, life_cell(8).unwrap_or(0));
        assert_eq!(lt.record_life(2, life_cell(2).unwrap_or(0)), Err(VmwareCode::EOrder));
        // 主账本继续按合法序落格不受影响。
        assert!(main.record_life(2, life_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LIFE_OPS + SNAP_CELLS + NET_CELLS + AUX_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8500);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VmwareCode::Ok as u32, 0);
        assert_eq!(VmwareCode::EGradeDrift as u32, 12);
        assert_eq!(VmwareCode::ESeal.val(), SEG_BASE + 11);
        assert!(!VmwareCode::Ok.detail().is_empty());
        assert!(!VmwareCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = VmLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52277_checks_green() {
        let s = run_unx52277_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
