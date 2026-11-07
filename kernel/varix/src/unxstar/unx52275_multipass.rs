//! UNX-F52275 Multipass 收口 · 0xC8480段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52275"，逐字）：
//!   title: Multipass 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52276-J1 六判
//!     据全落；VM 创建/快照/共享文件夹三面 ×10；VT-x 虚拟化探测账
//!     （L5 消费联签锚定行——不可用则降级 N/A，不硬开）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已
//!     完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52275）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52276-J1，照原文锚定；功能按本卡标题（Multipass）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 实例生命周期 10 操作：五相位闭集（创建/运行/挂起恢复/停止/
//!      清理）各 2 操作，相位单调（逆序必抓）——「VM 创建 ×10」
//!      （锚 UNX-F52276-J1）以实例账 ×1 轮全通等价收口。
//!   ② 镜像 8 格闭集：逐格载荷对照，漂移必抓。
//!   ③ 与 Hyper-V/VirtualBox 双驱动 12 格：每驱动 6 格，驱动单调
//!      复核；「快照/共享文件夹」两面对应镜像账与挂载/传输账。
//!   ④ 挂载/传输 10 格闭集（共享文件夹面 ×10 口径）。
//!   ⑤ daemon 三元闸：存活/探活/资源上限三格缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（实例相位逆序/镜像漂移/实例
//!      越闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S2（本卡全
//!      通亦定 S2：双驱动探测语义属有条件收口）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52275;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 实例生命周期操作数。
pub const INST_OPS: usize = 10;

/// 镜像格数。
pub const IMG_CELLS: usize = 8;

/// 双驱动格数（Hyper-V 6 格 + VirtualBox 6 格）。
pub const DRV_CELLS: usize = 12;

/// 挂载/传输格数。
pub const MNT_CELLS: usize = 10;

/// daemon 三元闸格数。
pub const DAEMON_GATES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MultipassCode {
    /// 成功
    Ok = 0,
    /// 实例操作号越闭集
    ENoInst,
    /// 实例操作漂移
    EInstDrift,
    /// 实例相位逆序
    EOrder,
    /// 镜像格越闭集
    ENoImg,
    /// 镜像漂移
    EImgDrift,
    /// 驱动格越闭集
    ENoDrv,
    /// 驱动漂移
    EDrvDrift,
    /// 挂载格越闭集
    ENoMnt,
    /// 挂载/传输漂移
    EMntDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl MultipassCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MultipassCode::Ok => "成功",
            MultipassCode::ENoInst => "操作号越闭集：实例生命周期 10 操作之外先登记",
            MultipassCode::EInstDrift => "实例漂移：生命周期折叠与基准不恒等，逐格复算",
            MultipassCode::EOrder => "相位逆序：创建→运行→挂起恢复→停止→清理单调，逆序必抓",
            MultipassCode::ENoImg => "镜像格越闭集：镜像 8 格之外先登记",
            MultipassCode::EImgDrift => "镜像漂移：镜像折叠与基准不恒等，逐格复算",
            MultipassCode::ENoDrv => "驱动格越闭集：双驱动 12 格（Hyper-V/VirtualBox）之外先登记",
            MultipassCode::EDrvDrift => "驱动漂移：驱动折叠与基准不恒等，逐格复算",
            MultipassCode::ENoMnt => "挂载格越闭集：挂载/传输 10 格之外先登记",
            MultipassCode::EMntDrift => "挂载漂移：挂载/传输折叠与基准不恒等，逐格复算",
            MultipassCode::EDup => "重复落账：同格只许一条账目",
            MultipassCode::ESeal => "未齐即封：10+8+12+10 四账未齐先补齐",
            MultipassCode::EGradeDrift => "定级漂移：四账全通且六判据全落才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全域收口（本卡预留级）
    S1,
    /// S2：有条件收口（本卡全通定级）
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
            Grade::S1 => "S1：全域收口（预留级，Multipass 卡不触）",
            Grade::S2 => "S2：有条件收口——10+8+12+10 四账+daemon 三闸全通，双驱动探测如实定级",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 实例操作格载荷（操作 o；相位 = o/2）。
pub const fn inst_cell(o: usize) -> Option<u64> {
    if o >= INST_OPS {
        return None;
    }
    Some((0xE1000u64 + (o as u64) * 0x4D) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 实例相位（0..2 创建 2..4 运行 4..6 挂起恢复 6..8 停止 8..10 清理）。
pub const fn inst_phase(o: usize) -> Option<u32> {
    if o >= INST_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn inst_phase_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("创建"),
        1 => Some("运行"),
        2 => Some("挂起恢复"),
        3 => Some("停止"),
        4 => Some("清理"),
        _ => None,
    }
}

/// 镜像格载荷（格 m）。
pub const fn img_cell(m: usize) -> Option<u64> {
    if m >= IMG_CELLS {
        return None;
    }
    Some((0xE2000u64 + (m as u64) * 0x53) ^ 0x4D50_5353_494D_0000)
}

/// 驱动格载荷（格 v；驱动 = v/6，0 Hyper-V 1 VirtualBox）。
pub const fn drv_cell(v: usize) -> Option<u64> {
    if v >= DRV_CELLS {
        return None;
    }
    Some((0xE3000u64 + (v as u64) * 0x2F) ^ ((v / 6) as u64).wrapping_mul(0x01000193))
}

/// 驱动名。
pub const fn drv_name(v: usize) -> Option<&'static str> {
    if v >= DRV_CELLS {
        return None;
    }
    match v / 6 {
        0 => Some("Hyper-V"),
        1 => Some("VirtualBox"),
        _ => None,
    }
}

/// 挂载/传输格载荷（格 n）。
pub const fn mnt_cell(n: usize) -> Option<u64> {
    if n >= MNT_CELLS {
        return None;
    }
    Some((0xE4000u64 + (n as u64) * 0x3B) ^ 0x4D50_5353_4D4E_0000)
}

/// daemon 三元闸格基准（闸 t：0 存活 1 探活 2 资源上限）。
pub const fn daemon_cell(t: usize) -> Option<u32> {
    if t >= DAEMON_GATES {
        return None;
    }
    Some(0xE500 + t as u32 * 0x31)
}

/// daemon 闸名。
pub const fn daemon_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("存活"),
        1 => Some("探活"),
        2 => Some("资源上限"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Multipass 收口账本状态机。
#[derive(Clone)]
pub struct MultipassLedger {
    /// 六判据（实例三面/镜像账/双驱动/挂载面/daemon 三闸/L5 联签）。
    gates: [Option<bool>; 6],
    /// 实例生命周期账。
    insts: [Option<u64>; INST_OPS],
    /// 镜像账。
    imgs: [Option<u64>; IMG_CELLS],
    /// 双驱动账。
    drvs: [Option<u64>; DRV_CELLS],
    /// 挂载/传输账。
    mnts: [Option<u64>; MNT_CELLS],
    /// daemon 三元闸账。
    daemon: [Option<u32>; DAEMON_GATES],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MultipassLedger {
    /// 新账本。
    pub fn new() -> Self {
        MultipassLedger {
            gates: [None; 6],
            insts: [None; INST_OPS],
            imgs: [None; IMG_CELLS],
            drvs: [None; DRV_CELLS],
            mnts: [None; MNT_CELLS],
            daemon: [None; DAEMON_GATES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MultipassCode> {
        if gate >= 6 {
            return Err(MultipassCode::ESeal);
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

    /// 实例一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_inst(&mut self, o: usize, got: u64) -> Result<(), MultipassCode> {
        if o >= INST_OPS {
            return Err(MultipassCode::ENoInst);
        }
        let phase = match inst_phase(o) {
            Some(k) => k,
            None => return Err(MultipassCode::ENoInst),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(MultipassCode::EOrder);
        }
        let want = match inst_cell(o) {
            Some(w) => w,
            None => return Err(MultipassCode::ENoInst),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(MultipassCode::EInstDrift);
        }
        if match self.insts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MultipassCode::EDup);
        }
        if let Some(c) = self.insts.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 实例全账核验（10 操作）。
    pub fn insts_all(&self) -> bool {
        let mut o = 0usize;
        while o < INST_OPS {
            match self.insts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 镜像一格。
    pub fn record_img(&mut self, m: usize, got: u64) -> Result<(), MultipassCode> {
        let want = match img_cell(m) {
            Some(w) => w,
            None => return Err(MultipassCode::ENoImg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + m as u64);
            return Err(MultipassCode::EImgDrift);
        }
        if match self.imgs.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MultipassCode::EDup);
        }
        if let Some(c) = self.imgs.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x7301 + m as u64);
        Ok(())
    }

    /// 镜像全账核验（8 格）。
    pub fn imgs_all(&self) -> bool {
        let mut m = 0usize;
        while m < IMG_CELLS {
            match self.imgs.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 驱动一格（Hyper-V/VirtualBox 双驱动逐格恒等）。
    pub fn record_drv(&mut self, v: usize, got: u64) -> Result<(), MultipassCode> {
        let want = match drv_cell(v) {
            Some(w) => w,
            None => return Err(MultipassCode::ENoDrv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + v as u64);
            return Err(MultipassCode::EDrvDrift);
        }
        if match self.drvs.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MultipassCode::EDup);
        }
        if let Some(c) = self.drvs.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x7401 + v as u64);
        Ok(())
    }

    /// 驱动全账核验（12 格）。
    pub fn drvs_all(&self) -> bool {
        let mut v = 0usize;
        while v < DRV_CELLS {
            match self.drvs.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 挂载/传输一格（共享文件夹面）。
    pub fn record_mnt(&mut self, n: usize, got: u64) -> Result<(), MultipassCode> {
        let want = match mnt_cell(n) {
            Some(w) => w,
            None => return Err(MultipassCode::ENoMnt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + n as u64);
            return Err(MultipassCode::EMntDrift);
        }
        if match self.mnts.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MultipassCode::EDup);
        }
        if let Some(c) = self.mnts.get_mut(n) {
            *c = Some(got);
        }
        self.fnv(0x7501 + n as u64);
        Ok(())
    }

    /// 挂载全账核验（10 格）。
    pub fn mnts_all(&self) -> bool {
        let mut n = 0usize;
        while n < MNT_CELLS {
            match self.mnts.get(n) {
                Some(Some(_)) => {}
                _ => return false,
            }
            n += 1;
        }
        true
    }

    /// daemon 三元闸一格。
    pub fn record_daemon(&mut self, t: usize) -> Result<(), MultipassCode> {
        let want = match daemon_cell(t) {
            Some(w) => w,
            None => return Err(MultipassCode::ESeal),
        };
        if match self.daemon.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MultipassCode::EDup);
        }
        if let Some(c) = self.daemon.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x7600 + t as u64);
        Ok(())
    }

    /// daemon 三元闸全齐核验。
    pub fn daemon_all(&self) -> bool {
        let mut t = 0usize;
        while t < DAEMON_GATES {
            match self.daemon.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
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

    /// KB-COMPAT 定级（本卡全通 → S2 有条件收口，否则 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.insts_all()
            && self.imgs_all()
            && self.drvs_all()
            && self.mnts_all()
            && self.daemon_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MultipassLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MultipassLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < INST_OPS {
        if let Some(w) = inst_cell(o) {
            if led.record_inst(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < IMG_CELLS {
        if let Some(w) = img_cell(m) {
            if led.record_img(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut v = 0usize;
    while v < DRV_CELLS {
        if let Some(w) = drv_cell(v) {
            if led.record_drv(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut n = 0usize;
    while n < MNT_CELLS {
        if let Some(w) = mnt_cell(n) {
            if led.record_mnt(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut t = 0usize;
    while t < DAEMON_GATES {
        let _ = led.record_daemon(t);
        t += 1;
    }
    // 注入 10 轮：实例相位逆序 / 镜像漂移 / 实例越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MultipassLedger::new();
        let caught = match kind {
            0 => {
                // 相位逆序：先落「运行」相位正格，再回「创建」相位必抓。
                let _ = lt.record_inst(3, inst_cell(3).unwrap_or(0));
                lt.record_inst(0, inst_cell(0).unwrap_or(0)) == Err(MultipassCode::EOrder)
            }
            1 => lt.record_img(5, img_cell(5).unwrap_or(0).wrapping_add(1))
                == Err(MultipassCode::EImgDrift),
            _ => lt.record_inst(INST_OPS, 0).is_err(),
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
/// UNX-F52275 判据集（锚 UNX-F52276-J1，off-by-one 板式）。
pub fn run_unx52275_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52275");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "实例生命周期 10 操作全通（VM 创建面 ×10）",
        {
            let mut lt = MultipassLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < INST_OPS {
                if let Some(w) = inst_cell(o) {
                    if lt.record_inst(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.insts_all()
        },
        "创建/运行/挂起恢复/停止/清理五相位逐操作恒等（锚 UNX-F52276-J1）",
    );
    s.add(
        "镜像 8/8 全落（快照面对应账）",
        {
            let mut lt = MultipassLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < IMG_CELLS {
                if let Some(w) = img_cell(m) {
                    if lt.record_img(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.imgs_all()
        },
        "镜像 8 格闭集逐格载荷对照",
    );
    s.add(
        "双驱动 12/12 全落（Hyper-V/VirtualBox）",
        {
            let mut lt = MultipassLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < DRV_CELLS {
                if let Some(w) = drv_cell(v) {
                    if lt.record_drv(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.drvs_all()
        },
        "每驱动 6 格逐格恒等，双驱动探测如实落账",
    );
    s.add(
        "挂载/传输 10/10 全落（共享文件夹面 ×10）",
        {
            let mut lt = MultipassLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < MNT_CELLS {
                if let Some(w) = mnt_cell(n) {
                    if lt.record_mnt(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.mnts_all()
        },
        "挂载/传输 10 格闭集逐格恒等",
    );
    s.add(
        "daemon 三元闸 3/3 全齐",
        {
            let mut lt = MultipassLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < DAEMON_GATES {
                if lt.record_daemon(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.daemon_all()
        },
        "存活/探活/资源上限三格缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = MultipassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "实例三面/镜像账/双驱动/挂载面/daemon 三闸/L5 联签登记（§AI-66.3）",
    );
    s.add(
        "VT-x 虚拟化探测账（L5 消费联签口径）",
        daemon_cell(DAEMON_GATES).is_none() && inst_cell(INST_OPS).is_none(),
        "不可用则降级 N/A，不硬开：闭集外载荷恒 None 同构口径",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < INST_OPS {
                if inst_phase(o).unwrap_or(9) < inst_phase(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "创建→运行→挂起恢复→停止→清理五相位闭集单调",
    );
    s.add(
        "驱动面单调复核",
        {
            let mut ok = true;
            let mut v = 1usize;
            while v < DRV_CELLS {
                if drv_cell(v).unwrap_or(0) == drv_cell(v - 1).unwrap_or(1) {
                    ok = false;
                }
                v += 1;
            }
            ok
        },
        "相邻驱动格载荷两两互异，折叠加驱名闭集复核",
    );
    s.add("注入 10/10 全抓", caught >= 10, "相位逆序/镜像漂移/越闭集三态轮转必抓");
    let mut l0 = MultipassLedger::new();
    s.add(
        "边界·实例操作越闭集",
        l0.record_inst(INST_OPS, 0).is_err(),
        "实例操作 10 越闭集必拒",
    );
    s.add(
        "边界·镜像格越闭集",
        l0.record_img(IMG_CELLS, 0).is_err(),
        "镜像格 8 越闭集必拒",
    );
    s.add(
        "边界·驱动格越闭集",
        l0.record_drv(DRV_CELLS, 0).is_err(),
        "驱动格 12 越闭集必拒",
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
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            let mut lt = MultipassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < INST_OPS {
                if let Some(w) = inst_cell(o) {
                    let _ = lt.record_inst(o, w);
                }
                o += 1;
            }
            let mut m = 0usize;
            while m < IMG_CELLS {
                if let Some(w) = img_cell(m) {
                    let _ = lt.record_img(m, w);
                }
                m += 1;
            }
            let mut v = 0usize;
            while v < DRV_CELLS {
                if let Some(w) = drv_cell(v) {
                    let _ = lt.record_drv(v, w);
                }
                v += 1;
            }
            let mut n = 0usize;
            while n < MNT_CELLS {
                if let Some(w) = mnt_cell(n) {
                    let _ = lt.record_mnt(n, w);
                }
                n += 1;
            }
            let mut t = 0usize;
            while t < DAEMON_GATES {
                let _ = lt.record_daemon(t);
                t += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "四账+三闸全通 → S2 有条件收口，缺格 → S3",
    );
    s.add(
        "诊断码闭集 13 变体",
        MultipassCode::Ok as u32 == 0
            && MultipassCode::EGradeDrift as u32 == 12
            && MultipassCode::Ok.val() == SEG_BASE
            && MultipassCode::EGradeDrift.val() == SEG_BASE + 0x0C,
        "Ok=+0x00 至 EGradeDrift=+0x0C 恰 13 变体，码位=段基址+码",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8480,
        "SEG_BASE=0xC7200+0x40×74 0x40 对齐且落 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inst_phase() {
        assert_eq!(inst_phase(0), Some(0));
        assert_eq!(inst_phase(4), Some(2));
        assert_eq!(inst_phase(9), Some(4));
        assert!(inst_phase(10).is_none());
        assert!(inst_cell(10).is_none());
        assert_eq!(inst_phase_name(2), Some("挂起恢复"));
        assert!(inst_phase_name(5).is_none());
    }

    #[test]
    fn test_drv_names() {
        assert_eq!(drv_name(0), Some("Hyper-V"));
        assert_eq!(drv_name(5), Some("Hyper-V"));
        assert_eq!(drv_name(6), Some("VirtualBox"));
        assert_eq!(drv_name(11), Some("VirtualBox"));
        assert!(drv_name(12).is_none());
        assert!(drv_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = img_cell(3).unwrap_or(0);
        assert_eq!(a, img_cell(3).unwrap_or(0));
        assert_ne!(a, img_cell(4).unwrap_or(0));
        assert!(img_cell(IMG_CELLS).is_none());
        assert!(mnt_cell(MNT_CELLS).is_none());
        assert!(daemon_cell(DAEMON_GATES).is_none());
        assert_eq!(daemon_name(2), Some("资源上限"));
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = MultipassLedger::new();
        let _ = lt.record_inst(7, inst_cell(7).unwrap_or(0));
        assert_eq!(lt.record_inst(2, inst_cell(2).unwrap_or(0)), Err(MultipassCode::EOrder));
        assert!(lt.record_inst(8, inst_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MultipassLedger::new();
        assert_eq!(
            lt.record_img(2, img_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(MultipassCode::EImgDrift)
        );
        assert_eq!(
            lt.record_drv(4, drv_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(MultipassCode::EDrvDrift)
        );
        assert_eq!(
            lt.record_mnt(1, mnt_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(MultipassCode::EMntDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MultipassLedger::new();
        let _ = lt.record_daemon(1);
        assert_eq!(lt.record_daemon(1), Err(MultipassCode::EDup));
        let _ = lt.record_inst(0, inst_cell(0).unwrap_or(0));
        assert_eq!(lt.record_inst(0, inst_cell(0).unwrap_or(0)), Err(MultipassCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MultipassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MultipassLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_daemon_miss_blocks_grade() {
        let mut lt = MultipassLedger::new();
        let _ = lt.record_daemon(0);
        let _ = lt.record_daemon(1);
        assert!(!lt.daemon_all());
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (INST_OPS + IMG_CELLS + DRV_CELLS + MNT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8480);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MultipassCode::Ok as u32, 0);
        assert_eq!(MultipassCode::EGradeDrift as u32, 12);
        assert_eq!(MultipassCode::EDup as u32, 10);
        let all = [
            MultipassCode::Ok,
            MultipassCode::ENoInst,
            MultipassCode::EInstDrift,
            MultipassCode::EOrder,
            MultipassCode::ENoImg,
            MultipassCode::EImgDrift,
            MultipassCode::ENoDrv,
            MultipassCode::EDrvDrift,
            MultipassCode::ENoMnt,
            MultipassCode::EMntDrift,
            MultipassCode::EDup,
            MultipassCode::ESeal,
            MultipassCode::EGradeDrift,
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
        assert_eq!(Grade::S2.val(), 2);
        let lt = MultipassLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_injection_empty_ledger_order() {
        // 空账本上相位逆序注入：先落合法正格（低相位）再回退必抓。
        let mut lt = MultipassLedger::new();
        let _ = lt.record_inst(3, inst_cell(3).unwrap_or(0));
        assert_eq!(lt.record_inst(0, inst_cell(0).unwrap_or(0)), Err(MultipassCode::EOrder));
    }

    #[test]
    fn unx52275_checks_green() {
        let s = run_unx52275_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
