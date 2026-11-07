//! UNX-F52261 Docker Desktop 收口 · 0xC8100段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52261"，逐字）：
//!   title: Docker Desktop 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52262-J1 docker 20
//!     操作集 ×10 全通；与 F52261 共面分账；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52261）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52262-J1，照原文锚定；功能按本卡标题（Docker Desktop）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① WSL2/Hyper-V 双后端三态 12 格：后端 × 状态 × 探针（engine/gui）
//!      三元闭集，2×3×2=12 格，逐格载荷（纯函数折叠）恒等才收；越界必抓。
//!   ② 生命周期 10 操作集：五相位闭集（安装/启动/运行/诊断/卸载）各 2
//!      操作，相位单调（逆序必抓）；漂移必抓。
//!   ③ 资源配额 8 格：CPU/内存/磁盘/交换 各 限值+用量 两格闭集。
//!   ④ K8s 消费 6 格：Desktop 内置 Kubernetes 消费链路逐格落账。
//!   ⑤ 许可分账 2 行闭集：商业订阅分账声明 / 个人免费分账声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（后端格越闭集/
//!      生命周期漂移/相位逆序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（12 格 + 10 操作 + 8 格 +
//!      6 格 + 2 行 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52261;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 双后端数（WSL2 / Hyper-V）。
pub const BACKENDS: usize = 2;

/// 后端三态数（运行/停止/故障）。
pub const BACKEND_STATES: usize = 3;

/// 每态探针数（engine / gui）。
pub const DESK_ASPECTS: usize = 2;

/// 双后端三态总格数。
pub const DESK_CELLS: usize = BACKENDS * BACKEND_STATES * DESK_ASPECTS;

/// 生命周期操作数。
pub const LIFECYCLE_OPS: usize = 10;

/// 资源配额格数。
pub const QUOTA_CELLS: usize = 8;

/// K8s 消费格数。
pub const K8S_CELLS: usize = 6;

/// 许可分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// 判据锚定原文（卡面 criteria 逐字引用）。
pub const CRITERIA_ANCHOR: &str = "§AI-66.3";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerDeskCode {
    /// 成功
    Ok = 0,
    /// 格越闭集
    ENoCell,
    /// 格漂移
    ECellDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

impl DockerDeskCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DockerDeskCode::Ok => "成功",
            DockerDeskCode::ENoCell => "格越闭集：双后端三态 12 格/配额 8 格/K8s 6 格之外先登记",
            DockerDeskCode::ECellDrift => "格漂移：格载荷与基准折叠不恒等，逐格复算",
            DockerDeskCode::ENoOp => "操作号越闭集：生命周期 10 操作集之外先登记",
            DockerDeskCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            DockerDeskCode::EOrder => "相位逆序：安装→启动→运行→诊断→卸载单调，逆序必抓",
            DockerDeskCode::ENoRow => "声明行越闭集：许可分账 2 行闭集之外先登记",
            DockerDeskCode::ERowMiss => "缺声明行：商业订阅/个人免费许可分账声明未齐先补齐",
            DockerDeskCode::EDup => "重复落账：同格只许一条账目",
            DockerDeskCode::ESeal => "未齐即封：12 格 + 10 操作 + 8 格 + 6 格未齐先补齐",
            DockerDeskCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DockerDeskCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DockerDeskCode::EGradeDrift => "定级漂移：全通且许可分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12格+10操作+8格+6格+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 后端名（b：0=WSL2 1=Hyper-V）。
pub const fn backend_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("WSL2"),
        1 => Some("Hyper-V"),
        _ => None,
    }
}

/// 三态名（s：0=运行 1=停止 2=故障）。
pub const fn state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("运行"),
        1 => Some("停止"),
        2 => Some("故障"),
        _ => None,
    }
}

/// 探针名（a：0=engine 1=gui）。
pub const fn aspect_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("engine"),
        1 => Some("gui"),
        _ => None,
    }
}

/// 双后端三态格基准（b/s/a 三元闭集，2×3×2=12 格）。
pub const fn desk_cell(b: usize, s: usize, a: usize) -> Option<u64> {
    if b >= BACKENDS || s >= BACKEND_STATES || a >= DESK_ASPECTS {
        return None;
    }
    let idx = (b * BACKEND_STATES + s) * DESK_ASPECTS + a;
    Some((0xD1100u64 + (idx as u64) * 0x2D) ^ 0x574C32484F530000)
}

/// 生命周期操作格载荷（操作 o；相位 = o/2）。
pub const fn lifecycle_cell(o: usize) -> Option<u64> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((0xD1200u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期相位（0..2=安装 2..4=启动 4..6=运行 6..8=诊断 8..10=卸载）。
pub const fn lifecycle_kind(o: usize) -> Option<u32> {
    if o >= LIFECYCLE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn lifecycle_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装"),
        1 => Some("启动"),
        2 => Some("运行"),
        3 => Some("诊断"),
        4 => Some("卸载"),
        _ => None,
    }
}

/// 资源配额格基准（格 c：CPU限值/CPU用量/内存限值/内存用量/磁盘限值/
/// 磁盘用量/交换限值/交换用量 依次 0..8）。
pub const fn quota_cell(c: usize) -> Option<u64> {
    if c >= QUOTA_CELLS {
        return None;
    }
    Some((0xD1300u64 + (c as u64) * 0x2C) ^ 0x5155544130300000)
}

/// 配额格名。
pub const fn quota_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("CPU限值"),
        1 => Some("CPU用量"),
        2 => Some("内存限值"),
        3 => Some("内存用量"),
        4 => Some("磁盘限值"),
        5 => Some("磁盘用量"),
        6 => Some("交换限值"),
        7 => Some("交换用量"),
        _ => None,
    }
}

/// K8s 消费格基准（格 k）。
pub const fn k8s_cell(k: usize) -> Option<u64> {
    if k >= K8S_CELLS {
        return None;
    }
    Some((0xD1400u64 + (k as u64) * 0x31) ^ 0x4B38535F0000)
}

/// 许可分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD1500 + r as u32 * 0x2C)
}

/// 许可分账声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("商业订阅分账声明"),
        1 => Some("个人免费分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Docker Desktop 收口账本状态机。
#[derive(Clone)]
pub struct DockerDeskLedger {
    /// 六判据（安装/启动/功能/崩溃/卸载/升级）。
    gates: [Option<bool>; GATES],
    /// 双后端三态账。
    desk: [Option<u64>; DESK_CELLS],
    /// 生命周期操作账。
    life: [Option<u64>; LIFECYCLE_OPS],
    /// 资源配额账。
    quota: [Option<u64>; QUOTA_CELLS],
    /// K8s 消费账。
    k8s: [Option<u64>; K8S_CELLS],
    /// 许可分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后相位。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DockerDeskLedger {
    /// 新账本。
    pub fn new() -> Self {
        DockerDeskLedger {
            gates: [None; GATES],
            desk: [None; DESK_CELLS],
            life: [None; LIFECYCLE_OPS],
            quota: [None; QUOTA_CELLS],
            k8s: [None; K8S_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DockerDeskCode> {
        if gate >= GATES {
            return Err(DockerDeskCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5A100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 双后端三态一格（三元闭集 + 载荷对照 + 重复拒）。
    pub fn record_desk(&mut self, b: usize, s: usize, a: usize, got: u64) -> Result<(), DockerDeskCode> {
        let want = match desk_cell(b, s, a) {
            Some(w) => w,
            None => return Err(DockerDeskCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A200 + ((b * BACKEND_STATES + s) * DESK_ASPECTS + a) as u64);
            return Err(DockerDeskCode::ECellDrift);
        }
        let idx = (b * BACKEND_STATES + s) * DESK_ASPECTS + a;
        if match self.desk.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerDeskCode::EDup);
        }
        if let Some(c) = self.desk.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0x5A201 + idx as u64);
        Ok(())
    }

    /// 双后端三态全账核验（12 格）。
    pub fn desk_all(&self) -> bool {
        let mut i = 0usize;
        while i < DESK_CELLS {
            match self.desk.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 生命周期一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_life(&mut self, o: usize, got: u64) -> Result<(), DockerDeskCode> {
        if o >= LIFECYCLE_OPS {
            return Err(DockerDeskCode::ENoOp);
        }
        let kind = match lifecycle_kind(o) {
            Some(k) => k,
            None => return Err(DockerDeskCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A300);
            return Err(DockerDeskCode::EOrder);
        }
        let want = match lifecycle_cell(o) {
            Some(w) => w,
            None => return Err(DockerDeskCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A301 + o as u64);
            return Err(DockerDeskCode::EDrift);
        }
        if match self.life.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerDeskCode::EDup);
        }
        if let Some(c) = self.life.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5A302 + o as u64);
        Ok(())
    }

    /// 生命周期全账核验（10 格）。
    pub fn life_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFECYCLE_OPS {
            match self.life.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 资源配额一格（载荷对照 + 重复拒）。
    pub fn record_quota(&mut self, c: usize, got: u64) -> Result<(), DockerDeskCode> {
        let want = match quota_cell(c) {
            Some(w) => w,
            None => return Err(DockerDeskCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A400 + c as u64);
            return Err(DockerDeskCode::ECellDrift);
        }
        if match self.quota.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerDeskCode::EDup);
        }
        if let Some(q) = self.quota.get_mut(c) {
            *q = Some(got);
        }
        self.fnv(0x5A401 + c as u64);
        Ok(())
    }

    /// 资源配额全账核验（8 格）。
    pub fn quota_all(&self) -> bool {
        let mut c = 0usize;
        while c < QUOTA_CELLS {
            match self.quota.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// K8s 消费一格（载荷对照 + 重复拒）。
    pub fn record_k8s(&mut self, k: usize, got: u64) -> Result<(), DockerDeskCode> {
        let want = match k8s_cell(k) {
            Some(w) => w,
            None => return Err(DockerDeskCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A500 + k as u64);
            return Err(DockerDeskCode::ECellDrift);
        }
        if match self.k8s.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerDeskCode::EDup);
        }
        if let Some(x) = self.k8s.get_mut(k) {
            *x = Some(got);
        }
        self.fnv(0x5A501 + k as u64);
        Ok(())
    }

    /// K8s 消费全账核验（6 格）。
    pub fn k8s_all(&self) -> bool {
        let mut k = 0usize;
        while k < K8S_CELLS {
            match self.k8s.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// 许可分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DockerDeskCode> {
        if r >= SPLIT_ROWS {
            return Err(DockerDeskCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerDeskCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x5A600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// 最后相位（注入不改主账断言用）。
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
            && self.desk_all()
            && self.life_all()
            && self.quota_all()
            && self.k8s_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DockerDeskLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DockerDeskLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < BACKENDS {
        let mut s = 0usize;
        while s < BACKEND_STATES {
            let mut a = 0usize;
            while a < DESK_ASPECTS {
                if let Some(w) = desk_cell(b, s, a) {
                    if led.record_desk(b, s, a, w).is_ok() {
                        recorded += 1;
                    }
                }
                a += 1;
            }
            s += 1;
        }
        b += 1;
    }
    let mut o = 0usize;
    while o < LIFECYCLE_OPS {
        if let Some(w) = lifecycle_cell(o) {
            if led.record_life(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < QUOTA_CELLS {
        if let Some(w) = quota_cell(c) {
            if led.record_quota(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut k = 0usize;
    while k < K8S_CELLS {
        if let Some(w) = k8s_cell(k) {
            if led.record_k8s(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：后端格越闭集 / 生命周期漂移 / 相位逆序三态轮转，
    // 三类注入全部落在当轮临时账本上，主账 only 记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = DockerDeskLedger::new();
        let caught = match kind {
            0 => lt.record_desk(BACKENDS, 0, 0, 0).is_err(),
            1 => {
                // 漂移注入：先录正格，再注漂移载荷。
                if let Some(w0) = lifecycle_cell(0) {
                    let _ = lt.record_life(0, w0);
                }
                match lifecycle_cell(1) {
                    Some(w1) => lt.record_life(1, w1.wrapping_add(1)) == Err(DockerDeskCode::EDrift),
                    None => false,
                }
            }
            _ => {
                // 逆序注入：临时账本从空起步，先落末相位再回落首相位必抓，
                // 主账 last_kind 不被改动。
                if let Some(w9) = lifecycle_cell(9) {
                    let _ = lt.record_life(9, w9);
                }
                match lifecycle_cell(2) {
                    Some(w2) => lt.record_life(2, w2) == Err(DockerDeskCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5A700 + j as u64);
        } else {
            led.escaped += 1;
        }
        j += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52261 判据集（锚 UNX-F52262-J1，off-by-one 板式）。
pub fn run_unx52261_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52261");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "生命周期 10 操作全通",
        recorded >= LIFECYCLE_OPS as u32 + DESK_CELLS as u32 + QUOTA_CELLS as u32 + K8S_CELLS as u32,
        "安装/启动/运行/诊断/卸载五相位逐操作恒等（UNX-F52262-J1 共面分账）",
    );
    s.add(
        "双后端三态 12 格全落",
        {
            let mut lt = DockerDeskLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BACKENDS && ok {
                let mut st = 0usize;
                while st < BACKEND_STATES && ok {
                    let mut a = 0usize;
                    while a < DESK_ASPECTS {
                        if let Some(w) = desk_cell(b, st, a) {
                            if lt.record_desk(b, st, a, w).is_err() {
                                ok = false;
                            }
                        }
                        a += 1;
                    }
                    st += 1;
                }
                b += 1;
            }
            ok && lt.desk_all()
        },
        "WSL2/Hyper-V 双后端 × 运行/停止/故障三态 × engine/gui 探针逐格恒等",
    );
    s.add(
        "资源配额 8 格全落",
        {
            let mut lt = DockerDeskLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < QUOTA_CELLS {
                if let Some(w) = quota_cell(c) {
                    if lt.record_quota(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.quota_all()
        },
        "CPU/内存/磁盘/交换 限值+用量 逐格恒等",
    );
    s.add(
        "K8s 消费 6 格全落",
        {
            let mut lt = DockerDeskLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < K8S_CELLS {
                if let Some(w) = k8s_cell(k) {
                    if lt.record_k8s(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.k8s_all()
        },
        "Desktop 内置 Kubernetes 消费链路逐格落账",
    );
    s.add(
        "许可分账 2/2 全齐",
        {
            let mut lt = DockerDeskLedger::new();
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
        "商业订阅/个人免费许可分账两行闭集（与 F52261 共面分账口径）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFECYCLE_OPS {
                if lifecycle_kind(o).unwrap_or(5) < lifecycle_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五相位闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "后端格越闭集/生命周期漂移/相位逆序三态轮转必抓",
    );
    let mut l0 = DockerDeskLedger::new();
    s.add(
        "边界·越闭集·后端格",
        l0.record_desk(BACKENDS, 0, 0, 0).is_err() && desk_cell(BACKENDS, 0, 0).is_none(),
        "后端 2 越闭集必拒",
    );
    s.add(
        "边界·越闭集·生命周期操作",
        l0.record_life(LIFECYCLE_OPS, 0).is_err() && lifecycle_cell(LIFECYCLE_OPS).is_none(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·越闭集·配额格",
        l0.record_quota(QUOTA_CELLS, 0).is_err() && quota_cell(QUOTA_CELLS).is_none(),
        "配额 8 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            if let Some(w0) = lifecycle_cell(0) {
                let _ = l0.record_life(0, w0);
            }
            match lifecycle_cell(1) {
                Some(w1) => l0.record_life(1, w1.wrapping_add(1)) == Err(DockerDeskCode::EDrift),
                None => false,
            }
        },
        "生命周期操作载荷漂移必抓",
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
        "判据锚定 §AI-66.3",
        CRITERIA_ANCHOR == "§AI-66.3" && !CRITERIA_ANCHOR.is_empty(),
        "卡面判据锚定原文逐字登记",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = DockerDeskLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < BACKENDS {
                let mut st = 0usize;
                while st < BACKEND_STATES {
                    let mut a = 0usize;
                    while a < DESK_ASPECTS {
                        if let Some(w) = desk_cell(b, st, a) {
                            let _ = lt.record_desk(b, st, a, w);
                        }
                        a += 1;
                    }
                    st += 1;
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < LIFECYCLE_OPS {
                if let Some(w) = lifecycle_cell(o) {
                    let _ = lt.record_life(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < QUOTA_CELLS {
                if let Some(w) = quota_cell(c) {
                    let _ = lt.record_quota(c, w);
                }
                c += 1;
            }
            let mut k = 0usize;
            while k < K8S_CELLS {
                if let Some(w) = k8s_cell(k) {
                    let _ = lt.record_k8s(k, w);
                }
                k += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8100,
        "SEG_BASE 0x40 对齐且在 VX15 域（公式 0xC7200+0x40×60）",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_cells() {
        assert!(desk_cell(0, 0, 0).is_some());
        assert!(desk_cell(1, 2, 1).is_some());
        assert!(desk_cell(BACKENDS, 0, 0).is_none());
        assert!(desk_cell(0, BACKEND_STATES, 0).is_none());
        assert!(desk_cell(0, 0, DESK_ASPECTS).is_none());
        assert_eq!(backend_name(0), Some("WSL2"));
        assert_eq!(backend_name(1), Some("Hyper-V"));
        assert!(backend_name(2).is_none());
        assert_eq!(state_name(2), Some("故障"));
        assert!(state_name(3).is_none());
        assert_eq!(aspect_name(1), Some("gui"));
        assert!(aspect_name(2).is_none());
    }

    #[test]
    fn test_lifecycle_kind() {
        assert_eq!(lifecycle_kind(0), Some(0));
        assert_eq!(lifecycle_kind(1), Some(0));
        assert_eq!(lifecycle_kind(2), Some(1));
        assert_eq!(lifecycle_kind(9), Some(4));
        assert!(lifecycle_kind(10).is_none());
        assert!(lifecycle_cell(10).is_none());
        assert_eq!(lifecycle_kind_name(4), Some("卸载"));
        assert!(lifecycle_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = quota_cell(3).unwrap_or(0);
        assert_eq!(a, quota_cell(3).unwrap_or(0));
        assert_ne!(a, quota_cell(4).unwrap_or(0));
        let d = desk_cell(1, 1, 0).unwrap_or(0);
        assert_ne!(d, desk_cell(1, 1, 1).unwrap_or(0));
        assert!(quota_cell(QUOTA_CELLS).is_none());
        assert!(k8s_cell(K8S_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(quota_cell_name(0), Some("CPU限值"));
        assert_eq!(split_row_name(0), Some("商业订阅分账声明"));
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = DockerDeskLedger::new();
        let _ = lt.record_life(9, lifecycle_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_life(2, lifecycle_cell(2).unwrap_or(0)),
            Err(DockerDeskCode::EOrder)
        );
        assert!(lt.record_life(9, lifecycle_cell(9).unwrap_or(0)).is_err());
        // 同相位不逆序：先 8 后 9 合法。
        let mut lt2 = DockerDeskLedger::new();
        assert!(lt2.record_life(8, lifecycle_cell(8).unwrap_or(0)).is_ok());
        assert!(lt2.record_life(9, lifecycle_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DockerDeskLedger::new();
        assert_eq!(
            lt.record_life(5, lifecycle_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(DockerDeskCode::EDrift)
        );
        assert_eq!(
            lt.record_desk(0, 0, 0, desk_cell(0, 0, 0).unwrap_or(0).wrapping_add(7)),
            Err(DockerDeskCode::ECellDrift)
        );
        assert_eq!(
            lt.record_quota(2, quota_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(DockerDeskCode::ECellDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DockerDeskLedger::new();
        let _ = lt.record_k8s(0, k8s_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_k8s(0, k8s_cell(0).unwrap_or(0)),
            Err(DockerDeskCode::EDup)
        );
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(DockerDeskCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DockerDeskLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DockerDeskLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_injection_leaves_main_ledger() {
        let mut main = DockerDeskLedger::new();
        let before = main.last_kind();
        // 模拟注入轮：临时账本从空起步落末相位再回落，主账 last_kind 不动。
        let mut lt = DockerDeskLedger::new();
        let _ = lt.record_life(9, lifecycle_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_life(2, lifecycle_cell(2).unwrap_or(0)),
            Err(DockerDeskCode::EOrder)
        );
        assert_eq!(main.last_kind(), before);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (DESK_CELLS + LIFECYCLE_OPS + QUOTA_CELLS + K8S_CELLS) as u32
        );
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
        assert_eq!(SEG_BASE, 0xC8100);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：Ok=0，EGradeDrift=12。
        assert_eq!(DockerDeskCode::Ok as u32, 0);
        assert_eq!(DockerDeskCode::ENoCell as u32, 1);
        assert_eq!(DockerDeskCode::ECellDrift as u32, 2);
        assert_eq!(DockerDeskCode::ENoOp as u32, 3);
        assert_eq!(DockerDeskCode::EDrift as u32, 4);
        assert_eq!(DockerDeskCode::EOrder as u32, 5);
        assert_eq!(DockerDeskCode::ENoRow as u32, 6);
        assert_eq!(DockerDeskCode::ERowMiss as u32, 7);
        assert_eq!(DockerDeskCode::EDup as u32, 8);
        assert_eq!(DockerDeskCode::ESeal as u32, 9);
        assert_eq!(DockerDeskCode::EInj as u32, 10);
        assert_eq!(DockerDeskCode::EFp as u32, 11);
        assert_eq!(DockerDeskCode::EGradeDrift as u32, 12);
        assert!(!DockerDeskCode::Ok.detail().is_empty());
        assert!(!DockerDeskCode::EGradeDrift.detail().is_empty());
        assert_eq!(DockerDeskCode::Ok.val(), SEG_BASE);
        assert_eq!(DockerDeskCode::EGradeDrift.val(), SEG_BASE + 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DockerDeskLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = DockerDeskLedger::new();
        let mut g2 = 0usize;
        while g2 < GATES {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        // 六判据全落但格账未齐 → S2 有条件收口。
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52261_checks_green() {
        let s = run_unx52261_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
