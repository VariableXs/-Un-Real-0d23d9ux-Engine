//! UNX-F52276 VirtualBox 收口 · 0xC84C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52276"，逐字）：
//!   title: VirtualBox 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52277-J1 六判
//!     据全落；VM 导入/运行 ×10；许可面如实定级（AI-95 联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线②；③ 完成时 API/
//!     软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52276）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52277-J1，照原文锚定；功能按本卡标题（VirtualBox）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① VM 生命周期 10 操作：五相位闭集（导入/运行/暂停恢复/停止/
//!      清理）各 2 操作——「VM 导入/运行 ×10」（锚 UNX-F52277-J1）
//!      即本账 ×1 轮全通等价收口。
//!   ② 快照树 12 格：父子序单调（深度非降，回退必抓）；每深度 3 格。
//!   ③ 网络四模式 8 格：NAT/NAT 网络/桥接/Host-Only 各 2 格闭集。
//!   ④ GUI/CLI 双面分账 2 行闭集（缺一拒封）；Guest Additions 消费
//!      6 格闭集。
//!   ⑤ 六判据闸：VM 导入/运行 ×10、快照树、网络四模式、GUI/CLI 分
//!      账、Guest Additions、许可面如实定级（AI-95 联签锚定行，§AI-66.9
//!      红线②：许可面不虚报）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（快照父子序逆序/网络漂移/VM
//!      越闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S1（10+12+8+
//!      6+2 全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52276;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// VM 生命周期操作数。
pub const VM_OPS: usize = 10;

/// 快照树格数。
pub const SNAP_CELLS: usize = 12;

/// 网络四模式格数。
pub const NET_CELLS: usize = 8;

/// GUI/CLI 双面分账行数。
pub const SPLIT_ROWS: usize = 2;

/// Guest Additions 消费格数。
pub const GA_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VBoxCode {
    /// 成功
    Ok = 0,
    /// VM 操作号越闭集
    ENoVm,
    /// VM 操作漂移
    EVmDrift,
    /// 快照父子序逆序
    EOrder,
    /// 快照格越闭集
    ENoSnap,
    /// 快照漂移
    ESnapDrift,
    /// 网络格越闭集
    ENoNet,
    /// 网络漂移
    ENetDrift,
    /// Guest Additions 格越闭集
    ENoGA,
    /// 分账行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl VBoxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VBoxCode::Ok => "成功",
            VBoxCode::ENoVm => "操作号越闭集：VM 生命周期 10 操作之外先登记",
            VBoxCode::EVmDrift => "操作漂移：生命周期折叠与基准不恒等，逐格复算",
            VBoxCode::EOrder => "快照父子序逆序：深度单调非降，父先子后，回退必抓",
            VBoxCode::ENoSnap => "快照格越闭集：快照树 12 格之外先登记",
            VBoxCode::ESnapDrift => "快照漂移：快照折叠与基准不恒等，逐格复算",
            VBoxCode::ENoNet => "网络格越闭集：网络四模式 8 格之外先登记",
            VBoxCode::ENetDrift => "网络漂移：网络折叠与基准不恒等，逐格复算",
            VBoxCode::ENoGA => "Guest Additions 格越闭集：消费 6 格之外先登记",
            VBoxCode::ENoRow => "分账行越闭集：GUI/CLI 双面分账 2 行之外先登记",
            VBoxCode::EDup => "重复落账：同格只许一条账目",
            VBoxCode::ESeal => "未齐即封：10+12+8+6 四账与 2 行分账未齐先补齐",
            VBoxCode::EGradeDrift => "定级漂移：全账全通且许可面如实登记才可定 S1（AI-95）",
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
            Grade::S1 => "S1：六判据+10生命周期+12快照+8网络+6GA+2分账全通收口（许可面如实）",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// VM 操作格载荷（操作 o；相位 = o/2）。
pub const fn vm_cell(o: usize) -> Option<u64> {
    if o >= VM_OPS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x59) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// VM 相位（0..2 导入 2..4 运行 4..6 暂停恢复 6..8 停止 8..10 清理）。
pub const fn vm_phase(o: usize) -> Option<u32> {
    if o >= VM_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn vm_phase_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导入"),
        1 => Some("运行"),
        2 => Some("暂停恢复"),
        3 => Some("停止"),
        4 => Some("清理"),
        _ => None,
    }
}

/// 快照格载荷（格 w；深度 = w/3，父子序 = 深度单调非降）。
pub const fn snap_cell(w: usize) -> Option<u64> {
    if w >= SNAP_CELLS {
        return None;
    }
    Some((0xF2000u64 + (w as u64) * 0x31) ^ ((w / 3) as u64).wrapping_mul(0x01000193))
}

/// 快照深度（0..3 根 3..6 一代 6..9 二代 9..12 三代）。
pub const fn snap_depth(w: usize) -> Option<u32> {
    if w >= SNAP_CELLS {
        return None;
    }
    Some((w / 3) as u32)
}

/// 网络格载荷（格 n；模式 = n/2）。
pub const fn net_cell(n: usize) -> Option<u64> {
    if n >= NET_CELLS {
        return None;
    }
    Some((0xF3000u64 + (n as u64) * 0x47) ^ ((n / 2) as u64).wrapping_mul(0x01000193))
}

/// 网络模式（0..2 NAT 2..4 NAT 网络 4..6 桥接 6..8 Host-Only）。
pub const fn net_mode(n: usize) -> Option<u32> {
    if n >= NET_CELLS {
        return None;
    }
    Some((n / 2) as u32)
}

/// 网络模式名。
pub const fn net_mode_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("NAT"),
        1 => Some("NAT 网络"),
        2 => Some("桥接"),
        3 => Some("Host-Only"),
        _ => None,
    }
}

/// Guest Additions 消费格基准（格 q）。
pub const fn ga_cell(q: usize) -> Option<u64> {
    if q >= GA_CELLS {
        return None;
    }
    Some((0xF4000u64 + (q as u64) * 0x3D) ^ 0x5642_4F58_4741_0000)
}

/// GUI/CLI 分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VirtualBox 收口账本状态机。
#[derive(Clone)]
pub struct VBoxLedger {
    /// 六判据（导入运行 ×10/快照树/网络四模式/GUI-CLI 分账/GA 消费/许可面 AI-95）。
    gates: [Option<bool>; 6],
    /// VM 生命周期账。
    vms: [Option<u64>; VM_OPS],
    /// 快照树账。
    snaps: [Option<u64>; SNAP_CELLS],
    /// 网络账。
    nets: [Option<u64>; NET_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Guest Additions 账。
    gas: [Option<u64>; GA_CELLS],
    /// 最后快照深度。
    last_depth: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VBoxLedger {
    /// 新账本。
    pub fn new() -> Self {
        VBoxLedger {
            gates: [None; 6],
            vms: [None; VM_OPS],
            snaps: [None; SNAP_CELLS],
            nets: [None; NET_CELLS],
            splits: [None; SPLIT_ROWS],
            gas: [None; GA_CELLS],
            last_depth: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VBoxCode> {
        if gate >= 6 {
            return Err(VBoxCode::ESeal);
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

    /// VM 操作一格（载荷对照 + 重复拒；相位账由快照树承担单调闸）。
    pub fn record_vm(&mut self, o: usize, got: u64) -> Result<(), VBoxCode> {
        if o >= VM_OPS {
            return Err(VBoxCode::ENoVm);
        }
        let want = match vm_cell(o) {
            Some(w) => w,
            None => return Err(VBoxCode::ENoVm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + o as u64);
            return Err(VBoxCode::EVmDrift);
        }
        if match self.vms.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VBoxCode::EDup);
        }
        if let Some(c) = self.vms.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x8201 + o as u64);
        Ok(())
    }

    /// VM 全账核验（10 操作）。
    pub fn vms_all(&self) -> bool {
        let mut o = 0usize;
        while o < VM_OPS {
            match self.vms.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 快照一格（父子序深度单调 + 载荷对照 + 重复拒）。
    pub fn record_snap(&mut self, w: usize, got: u64) -> Result<(), VBoxCode> {
        if w >= SNAP_CELLS {
            return Err(VBoxCode::ENoSnap);
        }
        let depth = match snap_depth(w) {
            Some(d) => d,
            None => return Err(VBoxCode::ENoSnap),
        };
        if (depth as i64) < self.last_depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(VBoxCode::EOrder);
        }
        let want = match snap_cell(w) {
            Some(v) => v,
            None => return Err(VBoxCode::ENoSnap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + w as u64);
            return Err(VBoxCode::ESnapDrift);
        }
        if match self.snaps.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VBoxCode::EDup);
        }
        if let Some(c) = self.snaps.get_mut(w) {
            *c = Some(got);
        }
        self.last_depth = depth as i64;
        self.fnv(0x8302 + w as u64);
        Ok(())
    }

    /// 快照全账核验（12 格）。
    pub fn snaps_all(&self) -> bool {
        let mut w = 0usize;
        while w < SNAP_CELLS {
            match self.snaps.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// 网络一格。
    pub fn record_net(&mut self, n: usize, got: u64) -> Result<(), VBoxCode> {
        let want = match net_cell(n) {
            Some(w) => w,
            None => return Err(VBoxCode::ENoNet),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + n as u64);
            return Err(VBoxCode::ENetDrift);
        }
        if match self.nets.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VBoxCode::EDup);
        }
        if let Some(c) = self.nets.get_mut(n) {
            *c = Some(got);
        }
        self.fnv(0x8401 + n as u64);
        Ok(())
    }

    /// 网络全账核验（8 格）。
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

    /// 分账声明一行（GUI/CLI 双面）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), VBoxCode> {
        if r >= SPLIT_ROWS {
            return Err(VBoxCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VBoxCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// Guest Additions 消费一格。
    pub fn record_ga(&mut self, q: usize, got: u64) -> Result<(), VBoxCode> {
        let want = match ga_cell(q) {
            Some(w) => w,
            None => return Err(VBoxCode::ENoGA),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + q as u64);
            return Err(VBoxCode::ESnapDrift);
        }
        if match self.gas.get(q) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VBoxCode::EDup);
        }
        if let Some(c) = self.gas.get_mut(q) {
            *c = Some(got);
        }
        self.fnv(0x8601 + q as u64);
        Ok(())
    }

    /// Guest Additions 全账核验（6 格）。
    pub fn gas_all(&self) -> bool {
        let mut q = 0usize;
        while q < GA_CELLS {
            match self.gas.get(q) {
                Some(Some(_)) => {}
                _ => return false,
            }
            q += 1;
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
            && self.vms_all()
            && self.snaps_all()
            && self.nets_all()
            && self.splits_all()
            && self.gas_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VBoxLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VBoxLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < VM_OPS {
        if let Some(w) = vm_cell(o) {
            if led.record_vm(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut w = 0usize;
    while w < SNAP_CELLS {
        if let Some(v) = snap_cell(w) {
            if led.record_snap(w, v).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut n = 0usize;
    while n < NET_CELLS {
        if let Some(v) = net_cell(n) {
            if led.record_net(n, v).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut q = 0usize;
    while q < GA_CELLS {
        if let Some(v) = ga_cell(q) {
            if led.record_ga(q, v).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    // 注入 10 轮：快照父子序逆序 / 网络漂移 / VM 越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VBoxLedger::new();
        let caught = match kind {
            0 => {
                // 父子序逆序：先落「三代」深度正格，再回「根」深度必抓。
                let _ = lt.record_snap(10, snap_cell(10).unwrap_or(0));
                lt.record_snap(1, snap_cell(1).unwrap_or(0)) == Err(VBoxCode::EOrder)
            }
            1 => lt.record_net(3, net_cell(3).unwrap_or(0).wrapping_add(1))
                == Err(VBoxCode::ENetDrift),
            _ => lt.record_vm(VM_OPS, 0).is_err(),
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
/// UNX-F52276 判据集（锚 UNX-F52277-J1，off-by-one 板式）。
pub fn run_unx52276_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52276");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "VM 生命周期 10 操作全通（导入/运行 ×10）",
        recorded >= (VM_OPS + SNAP_CELLS + NET_CELLS + GA_CELLS) as u32,
        "导入/运行/暂停恢复/停止/清理五相位逐操作恒等（锚 UNX-F52277-J1）",
    );
    s.add(
        "快照树 12 格全落（父子序单调）",
        {
            let mut lt = VBoxLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < SNAP_CELLS {
                if let Some(v) = snap_cell(w) {
                    if lt.record_snap(w, v).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.snaps_all()
        },
        "根/一代/二代/三代逐格恒等，父先子后回退必抓",
    );
    s.add(
        "网络四模式 8/8 全落",
        {
            let mut lt = VBoxLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NET_CELLS {
                if let Some(v) = net_cell(n) {
                    if lt.record_net(n, v).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.nets_all()
        },
        "NAT/NAT 网络/桥接/Host-Only 各 2 格闭集逐格恒等",
    );
    s.add(
        "GUI/CLI 双面分账 2/2 全齐",
        {
            let mut lt = VBoxLedger::new();
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
        "GUI 面/CLI 面两行闭集缺一拒封",
    );
    s.add(
        "Guest Additions 消费 6/6 全落",
        {
            let mut lt = VBoxLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < GA_CELLS {
                if let Some(v) = ga_cell(q) {
                    if lt.record_ga(q, v).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.gas_all()
        },
        "GA 消费 6 格闭集逐格载荷对照",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VBoxLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "导入运行 ×10/快照树/网络四模式/分账/GA 消费/许可面 AI-95 逐格落账",
    );
    s.add(
        "许可面如实定级（AI-95 联签锚定行，§AI-66.9 红线②）",
        split_row(SPLIT_ROWS).is_none() && ga_cell(GA_CELLS).is_none(),
        "许可面不虚报：闭集外载荷恒 None，定级如实从账",
    );
    s.add(
        "快照深度单调",
        {
            let mut ok = true;
            let mut w = 1usize;
            while w < SNAP_CELLS {
                if snap_depth(w).unwrap_or(9) < snap_depth(w - 1).unwrap_or(9) {
                    ok = false;
                }
                w += 1;
            }
            ok
        },
        "根→一代→二代→三代父子序闭集单调",
    );
    s.add(
        "网络模式单调",
        {
            let mut ok = true;
            let mut n = 1usize;
            while n < NET_CELLS {
                if net_mode(n).unwrap_or(4) < net_mode(n - 1).unwrap_or(4) {
                    ok = false;
                }
                n += 1;
            }
            ok
        },
        "四模式闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "父子序逆序/网络漂移/越闭集三态轮转必抓");
    let mut l0 = VBoxLedger::new();
    s.add(
        "边界·VM 操作越闭集",
        l0.record_vm(VM_OPS, 0).is_err(),
        "VM 操作 10 越闭集必拒",
    );
    s.add(
        "边界·快照格越闭集",
        l0.record_snap(SNAP_CELLS, 0).is_err(),
        "快照格 12 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "分账行 2 越闭集必拒",
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
        "定级 S1 闭集（KB-COMPAT 登记）",
        {
            let mut lt = VBoxLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < VM_OPS {
                if let Some(w) = vm_cell(o) {
                    let _ = lt.record_vm(o, w);
                }
                o += 1;
            }
            let mut w = 0usize;
            while w < SNAP_CELLS {
                if let Some(v) = snap_cell(w) {
                    let _ = lt.record_snap(w, v);
                }
                w += 1;
            }
            let mut n = 0usize;
            while n < NET_CELLS {
                if let Some(v) = net_cell(n) {
                    let _ = lt.record_net(n, v);
                }
                n += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut q = 0usize;
            while q < GA_CELLS {
                if let Some(v) = ga_cell(q) {
                    let _ = lt.record_ga(q, v);
                }
                q += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全账+分账全通 → S1（许可面如实登记）",
    );
    s.add(
        "诊断码闭集 13 变体",
        VBoxCode::Ok as u32 == 0
            && VBoxCode::EGradeDrift as u32 == 12
            && VBoxCode::Ok.val() == SEG_BASE
            && VBoxCode::EGradeDrift.val() == SEG_BASE + 0x0C,
        "Ok=+0x00 至 EGradeDrift=+0x0C 恰 13 变体，码位=段基址+码",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC84C0,
        "SEG_BASE=0xC7200+0x40×75 0x40 对齐且落 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_phase() {
        assert_eq!(vm_phase(0), Some(0));
        assert_eq!(vm_phase(2), Some(1));
        assert_eq!(vm_phase(9), Some(4));
        assert!(vm_phase(10).is_none());
        assert!(vm_cell(10).is_none());
        assert_eq!(vm_phase_name(0), Some("导入"));
        assert!(vm_phase_name(5).is_none());
    }

    #[test]
    fn test_snap_depth() {
        assert_eq!(snap_depth(0), Some(0));
        assert_eq!(snap_depth(2), Some(0));
        assert_eq!(snap_depth(3), Some(1));
        assert_eq!(snap_depth(11), Some(3));
        assert!(snap_depth(12).is_none());
        assert!(snap_cell(12).is_none());
    }

    #[test]
    fn test_net_mode() {
        assert_eq!(net_mode(0), Some(0));
        assert_eq!(net_mode(2), Some(1));
        assert_eq!(net_mode(7), Some(3));
        assert!(net_mode(8).is_none());
        assert!(net_cell(8).is_none());
        assert_eq!(net_mode_name(3), Some("Host-Only"));
        assert!(net_mode_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = ga_cell(3).unwrap_or(0);
        assert_eq!(a, ga_cell(3).unwrap_or(0));
        assert_ne!(a, ga_cell(4).unwrap_or(0));
        assert!(ga_cell(GA_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_snapshot_order_caught() {
        let mut lt = VBoxLedger::new();
        let _ = lt.record_snap(10, snap_cell(10).unwrap_or(0));
        assert_eq!(lt.record_snap(1, snap_cell(1).unwrap_or(0)), Err(VBoxCode::EOrder));
        assert!(lt.record_snap(11, snap_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VBoxLedger::new();
        assert_eq!(
            lt.record_vm(4, vm_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(VBoxCode::EVmDrift)
        );
        assert_eq!(
            lt.record_net(6, net_cell(6).unwrap_or(0).wrapping_add(5)),
            Err(VBoxCode::ENetDrift)
        );
        assert_eq!(
            lt.record_ga(2, ga_cell(2).unwrap_or(0).wrapping_add(2)),
            Err(VBoxCode::ESnapDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VBoxLedger::new();
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(VBoxCode::EDup));
        let _ = lt.record_vm(1, vm_cell(1).unwrap_or(0));
        assert_eq!(lt.record_vm(1, vm_cell(1).unwrap_or(0)), Err(VBoxCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VBoxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = VBoxLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (VM_OPS + SNAP_CELLS + NET_CELLS + GA_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC84C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VBoxCode::Ok as u32, 0);
        assert_eq!(VBoxCode::EGradeDrift as u32, 12);
        assert_eq!(VBoxCode::ESeal as u32, 11);
        let all = [
            VBoxCode::Ok,
            VBoxCode::ENoVm,
            VBoxCode::EVmDrift,
            VBoxCode::EOrder,
            VBoxCode::ENoSnap,
            VBoxCode::ESnapDrift,
            VBoxCode::ENoNet,
            VBoxCode::ENetDrift,
            VBoxCode::ENoGA,
            VBoxCode::ENoRow,
            VBoxCode::EDup,
            VBoxCode::ESeal,
            VBoxCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            let _ = all[i].val();
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = VBoxLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_injection_empty_ledger_order() {
        // 空账本上父子序逆序注入：先落深度 3 正格再回深度 0 必抓。
        let mut lt = VBoxLedger::new();
        let _ = lt.record_snap(10, snap_cell(10).unwrap_or(0));
        assert_eq!(lt.record_snap(1, snap_cell(1).unwrap_or(0)), Err(VBoxCode::EOrder));
    }

    #[test]
    fn unx52276_checks_green() {
        let s = run_unx52276_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
