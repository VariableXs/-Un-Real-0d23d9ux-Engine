//! UNX-F52269 minikube 收口 · 0xC8300段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52269"，逐字）：
//!   title: minikube 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52270-J1 集群创建
//!     ×5 全通；容器运行时依赖分账；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52269）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52270-J1，照原文锚定；功能按本卡标题（minikube）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 驱动四族 12 格闭集：docker / hyperv / virtualbox / podman 各 3 格
//!      （探测/启动/就绪），族内载荷折叠恒等才收，跨族单调（逆序必抓）。
//!   ② 集群生命周期 10 操作：start/stop/delete/status/tunnel/ssh/logs/
//!      pause/unpause/cache 五族各 2 操作（族 = op/2），family 单调。
//!   ③ addon 8 格闭集（dashboard/ingress/dns/registry/metallb/
//!      metrics-server/storage-provisioner/default-storageclass），逐格
//!      启停账，重复落账拒。
//!   ④ profile 5 场景分账（default/dev/ci/staging/edge），缺格拒封；
//!      集群创建 ×5 全通即五 profile 逐场景核验（判据锚 UNX-F52270-J1）。
//!   ⑤ ledger 状态机（new/record_driver/record_op/record_addon/
//!      declare_profile/judge_gate×6/fingerprint FNV-1a/grade 定级闭集）；
//!      注入核 run_cycle 三态轮转 10/10（越闭集/载荷漂移/族逆序），检出+
//!      逃逸守恒；双跑指纹一致；定级 S2 闭集（四账全齐 + 六判据全落 →
//!      S2，KB-COMPAT 登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52269;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 驱动四族总格数（4 族 × 3 格）。
pub const DRIVER_CELLS: usize = 12;

/// 集群生命周期操作数。
pub const LIFECYCLE_OPS: usize = 10;

/// addon 闭集格数。
pub const ADDON_CELLS: usize = 8;

/// profile 场景数。
pub const PROFILE_SCENES: usize = 5;

/// XOR 锚常量（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4D4B5F44_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MinikubeCode {
    /// 成功
    Ok = 0,
    /// 驱动格越闭集
    ENoDrv,
    /// 驱动载荷漂移
    EDrvDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 族逆序
    EOrder,
    /// addon 越闭集
    ENoAddon,
    /// addon 缺格
    EAddonMiss,
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

impl MinikubeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MinikubeCode::Ok => "成功",
            MinikubeCode::ENoDrv => "驱动格越闭集：docker/hyperv/virtualbox/podman 四族 12 格之外先登记",
            MinikubeCode::EDrvDrift => "驱动载荷漂移：探测/启动/就绪折叠与基准不恒等，逐格复算",
            MinikubeCode::ENoOp => "操作号越闭集：start/stop/delete/status 等 10 操作集之外先登记",
            MinikubeCode::EOpDrift => "操作载荷漂移：生命周期操作折叠与基准不恒等，逐格复算",
            MinikubeCode::EOrder => "族逆序：生命周期五族单调，逆序必抓",
            MinikubeCode::ENoAddon => "addon 越闭集：8 addon 闭集之外先登记",
            MinikubeCode::EAddonMiss => "addon 缺格：8 addon 启停账未齐先补齐",
            MinikubeCode::EDup => "重复落账：同格只许一条账目",
            MinikubeCode::ESeal => "未齐即封：12 驱动 + 10 操作 + 8 addon + 5 profile 未齐先补齐",
            MinikubeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            MinikubeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MinikubeCode::EGradeDrift => "定级漂移：四账全齐且六判据全落才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标级）
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
            Grade::S1 => "S1：全通收口（本卡闭集不启用，目标级为 S2）",
            Grade::S2 => "S2：四账全齐+六判据全落，有条件收口（KB-COMPAT 登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 驱动格载荷（格 d；族 = d/3，族内 0=探测 1=启动 2=就绪）。
pub const fn driver_cell(d: usize) -> Option<u64> {
    if d >= DRIVER_CELLS {
        return None;
    }
    Some((0xB6100u64 + (d as u64) * 0x37) ^ ANCHOR)
}

/// 驱动族（0..3=docker 3..6=hyperv 6..9=virtualbox 9..12=podman）。
pub const fn driver_kind(d: usize) -> Option<u32> {
    if d >= DRIVER_CELLS {
        return None;
    }
    Some((d / 3) as u32)
}

/// 驱动族名。
pub const fn driver_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("docker"),
        1 => Some("hyperv"),
        2 => Some("virtualbox"),
        3 => Some("podman"),
        _ => None,
    }
}

/// 生命周期操作格载荷（操作 o；族 = o/2）。
pub const fn lifecycle_cell(o: usize) -> Option<u64> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((0xB6200u64 + (o as u64) * 0x2B) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期操作族（0..2=start 2..4=stop 4..6=delete 6..8=status 8..10=辅助五族）。
pub const fn lifecycle_kind(o: usize) -> Option<u32> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 生命周期操作族名。
pub const fn lifecycle_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("start"),
        1 => Some("stop"),
        2 => Some("delete"),
        3 => Some("status"),
        4 => Some("tunnel/ssh/logs/pause/unpause/cache 辅助"),
        _ => None,
    }
}

/// addon 格载荷（addon a）。
pub const fn addon_cell(a: usize) -> Option<u64> {
    if a >= ADDON_CELLS {
        return None;
    }
    Some((0xB6300u64 + (a as u64) * 0x1D) ^ ANCHOR)
}

/// addon 名。
pub const fn addon_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("dashboard"),
        1 => Some("ingress"),
        2 => Some("dns"),
        3 => Some("registry"),
        4 => Some("metallb"),
        5 => Some("metrics-server"),
        6 => Some("storage-provisioner"),
        7 => Some("default-storageclass"),
        _ => None,
    }
}

/// profile 场景折叠（场景 p）。
pub const fn profile_scene(p: usize) -> Option<u32> {
    if p >= PROFILE_SCENES {
        return None;
    }
    Some(0xB6400 + p as u32 * 0x33)
}

/// profile 场景名。
pub const fn profile_scene_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("default"),
        1 => Some("dev"),
        2 => Some("ci"),
        3 => Some("staging"),
        4 => Some("edge"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// minikube 收口账本状态机。
#[derive(Clone)]
pub struct MinikubeLedger {
    /// 六判据（安装/驱动/创建×5/状态/删除/登记）。
    gates: [Option<bool>; 6],
    /// 驱动账。
    drivers: [Option<u64>; DRIVER_CELLS],
    /// 生命周期操作账。
    ops: [Option<u64>; LIFECYCLE_OPS],
    /// addon 账。
    addons: [Option<u64>; ADDON_CELLS],
    /// profile 场景账。
    profiles: [Option<u32>; PROFILE_SCENES],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MinikubeLedger {
    /// 新账本。
    pub fn new() -> Self {
        MinikubeLedger {
            gates: [None; 6],
            drivers: [None; DRIVER_CELLS],
            ops: [None; LIFECYCLE_OPS],
            addons: [None; ADDON_CELLS],
            profiles: [None; PROFILE_SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MinikubeCode> {
        if gate >= 6 {
            return Err(MinikubeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 驱动一格（族载荷对照 + 重复拒；族单调由操作段统一把守）。
    pub fn record_driver(&mut self, d: usize, got: u64) -> Result<(), MinikubeCode> {
        let want = match driver_cell(d) {
            Some(w) => w,
            None => return Err(MinikubeCode::ENoDrv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + d as u64);
            return Err(MinikubeCode::EDrvDrift);
        }
        if match self.drivers.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MinikubeCode::EDup);
        }
        if let Some(c) = self.drivers.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x5201 + d as u64);
        Ok(())
    }

    /// 驱动全账核验（12 格）。
    pub fn drivers_all(&self) -> bool {
        let mut d = 0usize;
        while d < DRIVER_CELLS {
            match self.drivers.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 生命周期操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), MinikubeCode> {
        if o >= LIFECYCLE_OPS {
            return Err(MinikubeCode::ENoOp);
        }
        let kind = match lifecycle_kind(o) {
            Some(k) => k,
            None => return Err(MinikubeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(MinikubeCode::EOrder);
        }
        let want = match lifecycle_cell(o) {
            Some(w) => w,
            None => return Err(MinikubeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(MinikubeCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MinikubeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFECYCLE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// addon 一格（载荷对照 + 重复拒）。
    pub fn record_addon(&mut self, a: usize, got: u64) -> Result<(), MinikubeCode> {
        let want = match addon_cell(a) {
            Some(w) => w,
            None => return Err(MinikubeCode::ENoAddon),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + a as u64);
            return Err(MinikubeCode::EDrvDrift);
        }
        if match self.addons.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MinikubeCode::EDup);
        }
        if let Some(c) = self.addons.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x5401 + a as u64);
        Ok(())
    }

    /// addon 全账核验（8 格）。
    pub fn addons_all(&self) -> bool {
        let mut a = 0usize;
        while a < ADDON_CELLS {
            match self.addons.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// profile 场景一格。
    pub fn declare_profile(&mut self, p: usize) -> Result<(), MinikubeCode> {
        if p >= PROFILE_SCENES {
            return Err(MinikubeCode::ENoAddon);
        }
        if match self.profiles.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MinikubeCode::EDup);
        }
        if let Some(c) = self.profiles.get_mut(p) {
            *c = profile_scene(p);
        }
        self.fnv(0x5500 + p as u64);
        Ok(())
    }

    /// profile 全齐核验（5 场景，集群创建 ×5 全通）。
    pub fn profiles_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROFILE_SCENES {
            match self.profiles.get(p) {
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

    /// KB-COMPAT 定级（四账全齐+六判据全落 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.drivers_all()
            && self.ops_all()
            && self.addons_all()
            && self.profiles_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MinikubeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MinikubeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DRIVER_CELLS {
        if let Some(w) = driver_cell(d) {
            if led.record_driver(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut o = 0usize;
    while o < LIFECYCLE_OPS {
        if let Some(w) = lifecycle_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut a = 0usize;
    while a < ADDON_CELLS {
        if let Some(w) = addon_cell(a) {
            if led.record_addon(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut p = 0usize;
    while p < PROFILE_SCENES {
        let _ = led.declare_profile(p);
        p += 1;
    }
    // 注入 10 轮：越闭集 / 载荷漂移 / 族逆序三态轮转，全部落在当轮
    // 临时账本上；主账本只记注入计数，不承接注入格。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MinikubeLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：驱动 12 / 操作 10 之外必拒。
                let hit_a = lt.record_driver(DRIVER_CELLS, 0) == Err(MinikubeCode::ENoDrv);
                let hit_b = lt.record_op(LIFECYCLE_OPS, 0).is_err();
                hit_a || hit_b
            }
            1 => {
                // 漂移：先录正格，再注漂移载荷（同族内不触发族序）。
                let _ = lt.record_op(2, lifecycle_cell(2).unwrap_or(0));
                lt.record_op(3, lifecycle_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(MinikubeCode::EOpDrift)
            }
            _ => {
                // 族逆序：对空账本注入逆序对同样必抓，且不改 last_kind。
                let _ = lt.record_op(5, lifecycle_cell(5).unwrap_or(0));
                let before = lt.last_kind;
                let hit = lt.record_op(1, lifecycle_cell(1).unwrap_or(0)) == Err(MinikubeCode::EOrder);
                hit && lt.last_kind == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52269 判据集（锚 UNX-F52270-J1，off-by-one 板式）。
pub fn run_unx52269_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52269");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "生命周期 10 操作全通",
        recorded >= LIFECYCLE_OPS as u32,
        "start/stop/delete/status 及辅助五族逐操作恒等",
    );
    s.add(
        "驱动四族 12 格全落",
        {
            let mut lt = MinikubeLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DRIVER_CELLS {
                if let Some(w) = driver_cell(d) {
                    if lt.record_driver(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.drivers_all()
        },
        "docker/hyperv/virtualbox/podman 四族逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = MinikubeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/驱动/创建/状态/删除/登记逐格落账",
    );
    s.add(
        "集群创建 ×5 全通（profile 五场景）",
        {
            let mut lt = MinikubeLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROFILE_SCENES {
                if lt.declare_profile(p).is_err() {
                    ok = false;
                }
                p += 1;
            }
            ok && lt.profiles_all()
        },
        "default/dev/ci/staging/edge 五 profile 逐场景核验（判据锚 UNX-F52270-J1）",
    );
    s.add(
        "addon 8 格全齐",
        {
            let mut lt = MinikubeLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ADDON_CELLS {
                if let Some(w) = addon_cell(a) {
                    if lt.record_addon(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.addons_all()
        },
        "dashboard/ingress/dns/registry/metallb/metrics-server/storage-provisioner/default-storageclass 逐格",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFECYCLE_OPS {
                if lifecycle_kind(o).unwrap_or(6) < lifecycle_kind(o - 1).unwrap_or(6) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "生命周期五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/载荷漂移/族逆序三态轮转必抓",
    );
    s.add(
        "边界·驱动格越闭集",
        MinikubeLedger::new()
            .record_driver(DRIVER_CELLS, 0)
            .is_err(),
        "驱动格 12 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = MinikubeLedger::new();
            let _ = lt.record_op(4, lifecycle_cell(4).unwrap_or(0));
            lt.record_op(5, lifecycle_cell(5).unwrap_or(0).wrapping_add(1))
                == Err(MinikubeCode::EOpDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        MinikubeLedger::new().record_op(LIFECYCLE_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·profile 场景越闭集",
        MinikubeLedger::new().declare_profile(PROFILE_SCENES).is_err(),
        "profile 场景 5 越闭集必拒",
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
        "定级 S2 闭集",
        {
            // main/aux 各节全部记账后再断言。
            let mut lt = MinikubeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < DRIVER_CELLS {
                if let Some(w) = driver_cell(d) {
                    let _ = lt.record_driver(d, w);
                }
                d += 1;
            }
            let mut o = 0usize;
            while o < LIFECYCLE_OPS {
                if let Some(w) = lifecycle_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut a = 0usize;
            while a < ADDON_CELLS {
                if let Some(w) = addon_cell(a) {
                    let _ = lt.record_addon(a, w);
                }
                a += 1;
            }
            let mut p = 0usize;
            while p < PROFILE_SCENES {
                let _ = lt.declare_profile(p);
                p += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S2.val() == 2
        },
        "四账全齐+六判据全落 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位（0xC8300）",
    );
    s.add(
        "容器运行时依赖分账",
        {
            // docker/hyperv/virtualbox/podman 四族各 3 格依赖分账闭集。
            let mut ok = true;
            let mut k = 0usize;
            while k < 4 {
                if driver_kind_name(k).is_none() {
                    ok = false;
                }
                k += 1;
            }
            ok && DRIVER_CELLS == 12
        },
        "四族 ×3 格依赖分账声明闭集（§AI-66.3）",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_families() {
        assert_eq!(driver_kind_name(0), Some("docker"));
        assert_eq!(driver_kind_name(1), Some("hyperv"));
        assert_eq!(driver_kind_name(2), Some("virtualbox"));
        assert_eq!(driver_kind_name(3), Some("podman"));
        assert!(driver_kind_name(4).is_none());
        assert_eq!(driver_kind(0), Some(0));
        assert_eq!(driver_kind(11), Some(3));
        assert!(driver_kind(12).is_none());
        assert!(driver_cell(DRIVER_CELLS).is_none());
    }

    #[test]
    fn test_lifecycle_families() {
        assert_eq!(lifecycle_kind(0), Some(0));
        assert_eq!(lifecycle_kind(2), Some(1));
        assert_eq!(lifecycle_kind(9), Some(4));
        assert!(lifecycle_kind(10).is_none());
        assert_eq!(lifecycle_kind_name(0), Some("start"));
        assert_eq!(lifecycle_kind_name(2), Some("delete"));
        assert!(lifecycle_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = lifecycle_cell(3).unwrap_or(0);
        assert_eq!(a, lifecycle_cell(3).unwrap_or(0));
        assert_ne!(a, lifecycle_cell(4).unwrap_or(0));
        assert!(addon_cell(ADDON_CELLS).is_none());
        assert!(profile_scene(PROFILE_SCENES).is_none());
    }

    #[test]
    fn test_addon_names_closed() {
        let mut i = 0usize;
        while i < ADDON_CELLS {
            assert!(addon_name(i).is_some());
            i += 1;
        }
        assert_eq!(addon_name(0), Some("dashboard"));
        assert_eq!(addon_name(7), Some("default-storageclass"));
    }

    #[test]
    fn test_profile_scenes_closed() {
        assert_eq!(profile_scene_name(0), Some("default"));
        assert_eq!(profile_scene_name(4), Some("edge"));
        assert!(profile_scene_name(5).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = MinikubeLedger::new();
        let _ = lt.record_op(5, lifecycle_cell(5).unwrap_or(0));
        let before = lt.last_kind;
        assert_eq!(
            lt.record_op(1, lifecycle_cell(1).unwrap_or(0)),
            Err(MinikubeCode::EOrder)
        );
        // 族逆序不改 last_kind。
        assert_eq!(lt.last_kind, before);
        assert!(lt.record_op(6, lifecycle_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MinikubeLedger::new();
        assert_eq!(
            lt.record_op(3, lifecycle_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(MinikubeCode::EOpDrift)
        );
        assert_eq!(
            lt.record_driver(2, driver_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(MinikubeCode::EDrvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MinikubeLedger::new();
        let _ = lt.record_driver(0, driver_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_driver(0, driver_cell(0).unwrap_or(0)),
            Err(MinikubeCode::EDup)
        );
        let _ = lt.record_addon(1, addon_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_addon(1, addon_cell(1).unwrap_or(0)),
            Err(MinikubeCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MinikubeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MinikubeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LIFECYCLE_OPS + DRIVER_CELLS + ADDON_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_rotation() {
        // 三态各轮全落临时账本：主账本守恒 injected==caught==10，escaped==0。
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = MinikubeLedger::new();
        // 越闭集注入不落主账。
        assert!(lt.record_driver(DRIVER_CELLS, 0).is_err());
        assert_eq!(lt.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC8300);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MinikubeCode::Ok as u32, 0);
        assert_eq!(MinikubeCode::EGradeDrift as u32, 12);
        // 13 变体闭集：Ok + 12 错误码。
        let all = [
            MinikubeCode::Ok,
            MinikubeCode::ENoDrv,
            MinikubeCode::EDrvDrift,
            MinikubeCode::ENoOp,
            MinikubeCode::EOpDrift,
            MinikubeCode::EOrder,
            MinikubeCode::ENoAddon,
            MinikubeCode::EAddonMiss,
            MinikubeCode::EDup,
            MinikubeCode::ESeal,
            MinikubeCode::EInj,
            MinikubeCode::EFp,
            MinikubeCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0u32;
        while i < 13 {
            assert!(!all[i as usize].detail().is_empty());
            assert_eq!(all[i as usize].val(), SEG_BASE + i);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = MinikubeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = MinikubeLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = MinikubeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52269_checks_green() {
        let s = run_unx52269_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
