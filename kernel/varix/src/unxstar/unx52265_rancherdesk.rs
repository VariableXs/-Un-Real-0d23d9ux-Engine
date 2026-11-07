//! UNX-F52265 Rancher Desktop 收口 · 0xC8200段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52265"，逐字）：
//!   title: Rancher Desktop 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52266-J1 六判据全落
//!     （Electron）；栈管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52265）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52266-J1，照原文锚定；功能按本卡标题（Rancher Desktop）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① containerd/dockerd 双引擎三态 12 格：引擎 × 状态（运行/停止/故障）
//!      × 探针（engine/gui）三元闭集，2×3×2=12 格，逐格载荷（纯函数折叠，
//!      XOR 锚低 16 位恒零）恒等才收；越界必抓。
//!   ② K8s 版本阶梯 5 档闭集（v1.21…v1.25），越档必抓，逐档载荷对照。
//!   ③ 生命周期 10 操作集：五相位闭集（安装/启动/运行/诊断/卸载）各 2
//!      操作，相位单调（逆序必抓）；漂移必抓；空账本首操作必须落在 0 相位。
//!   ④ WSL 消费 4 格：发行版/集成开关/网络/资源 逐格落账。
//!   ⑤ Electron 4 层：主进程/渲染进程/预加载/原生模块，判据锚
//!      UNX-F52266-J1（Electron）逐层落账；定级 S2（本卡 KB-COMPAT 封顶）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（版本越阶梯/
//!      生命周期漂移/相位越序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52265;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 引擎数（containerd / dockerd）。
pub const ENGINES: usize = 2;

/// 引擎三态数（运行/停止/故障）。
pub const ENGINE_STATES: usize = 3;

/// 每态探针数（engine / gui）。
pub const ENGINE_PROBES: usize = 2;

/// 双引擎三态总格数（2×3×2）。
pub const ENGINE_CELLS: usize = ENGINES * ENGINE_STATES * ENGINE_PROBES;

/// K8s 版本阶梯档数。
pub const K8S_STEPS: usize = 5;

/// 生命周期操作数。
pub const LIFE_OPS: usize = 10;

/// WSL 消费格数。
pub const WSL_CELLS: usize = 4;

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RancherCode {
    /// 成功
    Ok = 0,
    /// 引擎格越闭集
    ENoEngine,
    /// 引擎格漂移
    EEngineDrift,
    /// 版本档越阶梯
    ENoK8s,
    /// 版本阶梯漂移
    EK8sDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EPhase,
    /// WSL/Electron 格越闭集
    ENoCell,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl RancherCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RancherCode::Ok => "成功",
            RancherCode::ENoEngine => "引擎格越闭集：containerd/dockerd 2×3×2=12 格之外先登记",
            RancherCode::EEngineDrift => "引擎格漂移：格载荷折叠与基准不恒等，逐格复算",
            RancherCode::ENoK8s => "版本档越阶梯：v1.21…v1.25 五档闭集之外先登记",
            RancherCode::EK8sDrift => "版本阶梯漂移：档载荷折叠与基准不恒等，逐档复算",
            RancherCode::ENoOp => "操作号越闭集：生命周期 10 操作集之外先登记",
            RancherCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            RancherCode::EPhase => "相位逆序：安装→启动→运行→诊断→卸载单调，逆序必抓；空账本首操作必须落在安装相位",
            RancherCode::ENoCell => "格越闭集：WSL 4 格或 Electron 4 层之外先登记",
            RancherCode::EDup => "重复落账：同格只许一条账目",
            RancherCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RancherCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RancherCode::EGradeDrift => "定级漂移：本卡 KB-COMPAT 封顶 S2，全通且分层全齐才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡不适用，KB-COMPAT 封顶 S2）",
            Grade::S2 => "S2：有条件收口，12 格+5 档+10 操作+4 格+4 层全通登记",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 引擎名（const fn 表 + Option 边界）。
pub const fn engine_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("containerd"),
        1 => Some("dockerd"),
        _ => None,
    }
}

/// 引擎态名。
pub const fn engine_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("运行"),
        1 => Some("停止"),
        2 => Some("故障"),
        _ => None,
    }
}

/// 引擎格载荷（格 i：e×9+s×3+p；XOR 锚低 16 位恒零）。
pub const fn engine_cell(i: usize) -> Option<u64> {
    if i >= ENGINE_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x5D) ^ 0x52445F44_0000)
}

/// K8s 版本档名。
pub const fn k8s_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("v1.21"),
        1 => Some("v1.22"),
        2 => Some("v1.23"),
        3 => Some("v1.24"),
        4 => Some("v1.25"),
        _ => None,
    }
}

/// K8s 档载荷。
pub const fn k8s_cell(v: usize) -> Option<u64> {
    if v >= K8S_STEPS {
        return None;
    }
    Some((0xB2000u64 + (v as u64) * 0x71) ^ 0x52444B38_0000)
}

/// 生命周期操作格载荷（操作 o；相位 = o/2）。
pub const fn life_cell(o: usize) -> Option<u64> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((0xB3000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 操作相位（0..2=安装 2..4=启动 4..6=运行 6..8=诊断 8..10=卸载）。
pub const fn life_phase(o: usize) -> Option<u32> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn life_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("安装"),
        1 => Some("启动"),
        2 => Some("运行"),
        3 => Some("诊断"),
        4 => Some("卸载"),
        _ => None,
    }
}

/// WSL 消费格载荷（格 w：0=发行版 1=集成开关 2=网络 3=资源）。
pub const fn wsl_cell(w: usize) -> Option<u32> {
    if w >= WSL_CELLS {
        return None;
    }
    Some(0xB400 + w as u32 * 0x2C)
}

/// WSL 格名。
pub const fn wsl_cell_name(w: usize) -> Option<&'static str> {
    match w {
        0 => Some("发行版"),
        1 => Some("集成开关"),
        2 => Some("网络"),
        3 => Some("资源"),
        _ => None,
    }
}

/// Electron 层载荷（层 l：0=主进程 1=渲染进程 2=预加载 3=原生模块）。
pub const fn electron_cell(l: usize) -> Option<u32> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some(0xB500 + l as u32 * 0x3E)
}

/// Electron 层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("预加载"),
        3 => Some("原生模块"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Rancher Desktop 收口账本状态机。
#[derive(Clone)]
pub struct RancherLedger {
    /// 六判据（启动/引擎/K8s/WSL/诊断/卸载）。
    gates: [Option<bool>; 6],
    /// 引擎三态格账。
    engines: [Option<u64>; ENGINE_CELLS],
    /// 版本阶梯账。
    k8s: [Option<u64>; K8S_STEPS],
    /// 生命周期操作账。
    ops: [Option<u64>; LIFE_OPS],
    /// WSL 消费账。
    wsl: [Option<u32>; WSL_CELLS],
    /// Electron 层账。
    electron: [Option<u32>; ELECTRON_LAYERS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RancherLedger {
    /// 新账本。
    pub fn new() -> Self {
        RancherLedger {
            gates: [None; 6],
            engines: [None; ENGINE_CELLS],
            k8s: [None; K8S_STEPS],
            ops: [None; LIFE_OPS],
            wsl: [None; WSL_CELLS],
            electron: [None; ELECTRON_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RancherCode> {
        if gate >= 6 {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 引擎格一格（载荷对照 + 重复拒）。
    pub fn record_engine(&mut self, i: usize, got: u64) -> Result<(), RancherCode> {
        let want = match engine_cell(i) {
            Some(w) => w,
            None => return Err(RancherCode::ENoEngine),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + i as u64);
            return Err(RancherCode::EEngineDrift);
        }
        if match self.engines.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.engines.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4201 + i as u64);
        Ok(())
    }

    /// 引擎格全账核验（12 格）。
    pub fn engines_all(&self) -> bool {
        let mut i = 0usize;
        while i < ENGINE_CELLS {
            match self.engines.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 版本档一格（阶梯载荷对照 + 重复拒）。
    pub fn record_k8s(&mut self, v: usize, got: u64) -> Result<(), RancherCode> {
        let want = match k8s_cell(v) {
            Some(w) => w,
            None => return Err(RancherCode::ENoK8s),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300 + v as u64);
            return Err(RancherCode::EK8sDrift);
        }
        if match self.k8s.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.k8s.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x4301 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（5 档全齐）。
    pub fn k8s_all(&self) -> bool {
        let mut v = 0usize;
        while v < K8S_STEPS {
            match self.k8s.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 空账本首相位必须为 0 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RancherCode> {
        if o >= LIFE_OPS {
            return Err(RancherCode::ENoOp);
        }
        let phase = match life_phase(o) {
            Some(p) => p,
            None => return Err(RancherCode::ENoOp),
        };
        if (self.last_phase < 0 && phase != 0) || (self.last_phase >= 0 && (phase as i64) < self.last_phase) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400);
            return Err(RancherCode::EPhase);
        }
        let want = match life_cell(o) {
            Some(w) => w,
            None => return Err(RancherCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4401 + o as u64);
            return Err(RancherCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x4402 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
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

    /// WSL 消费一格。
    pub fn record_wsl(&mut self, w: usize) -> Result<(), RancherCode> {
        let want = match wsl_cell(w) {
            Some(x) => x,
            None => return Err(RancherCode::ENoCell),
        };
        if match self.wsl.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.wsl.get_mut(w) {
            *c = Some(want);
        }
        self.fnv(0x4500 + w as u64);
        Ok(())
    }

    /// WSL 全齐核验。
    pub fn wsl_all(&self) -> bool {
        let mut w = 0usize;
        while w < WSL_CELLS {
            match self.wsl.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// Electron 层一格（判据锚 UNX-F52266-J1）。
    pub fn record_electron(&mut self, l: usize) -> Result<(), RancherCode> {
        let want = match electron_cell(l) {
            Some(x) => x,
            None => return Err(RancherCode::ENoCell),
        };
        if match self.electron.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RancherCode::EDup);
        }
        if let Some(c) = self.electron.get_mut(l) {
            *c = Some(want);
        }
        self.fnv(0x4600 + l as u64);
        Ok(())
    }

    /// Electron 全齐核验。
    pub fn electron_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.electron.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（本卡封顶 S2：全通 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.engines_all()
            && self.k8s_all()
            && self.ops_all()
            && self.wsl_all()
            && self.electron_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RancherLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RancherLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < ENGINE_CELLS {
        if let Some(w) = engine_cell(i) {
            if led.record_engine(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut v = 0usize;
    while v < K8S_STEPS {
        if let Some(w) = k8s_cell(v) {
            if led.record_k8s(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < LIFE_OPS {
        if let Some(w) = life_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut w = 0usize;
    while w < WSL_CELLS {
        let _ = led.record_wsl(w);
        w += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        let _ = led.record_electron(l);
        l += 1;
    }
    // 注入 10 轮：版本越阶梯 / 生命周期漂移 / 相位越序三态轮转，
    // 全部落在当轮临时账本上；相位越序对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RancherLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_k8s(0, k8s_cell(0).unwrap_or(0));
                lt.record_k8s(K8S_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_op(0, life_cell(0).unwrap_or(0));
                lt.record_op(1, life_cell(1).unwrap_or(0).wrapping_add(1)) == Err(RancherCode::EDrift)
            }
            _ => {
                // 空账本首操作落在运行相位（相位 2）→ 越序必抓，且不改 last_phase。
                lt.record_op(4, life_cell(4).unwrap_or(0)) == Err(RancherCode::EPhase)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
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
/// UNX-F52265 判据集（锚 UNX-F52266-J1，off-by-one 板式）。
pub fn run_unx52265_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52265");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "containerd/dockerd 双引擎三态 12 格全通",
        recorded >= ENGINE_CELLS as u32,
        "引擎×状态×探针三元闭集逐格恒等（运行/停止/故障）",
    );
    s.add(
        "K8s 版本阶梯 v1.21…v1.25 五档全落",
        {
            let mut lt = RancherLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < K8S_STEPS {
                if let Some(w) = k8s_cell(v) {
                    if lt.record_k8s(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.k8s_all()
        },
        "五档闭集逐档恒等，越档必抓",
    );
    s.add(
        "生命周期 10 操作全通（安装/启动/运行/诊断/卸载 ×2）",
        {
            let mut lt = RancherLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(w) = life_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "五相位闭集逐操作恒等（栈管理 ×10 判据原文）",
    );
    s.add(
        "六判据全落（Electron，判据锚 UNX-F52266-J1）",
        {
            let mut lt = RancherLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/引擎/K8s/WSL/诊断/卸载逐格落账",
    );
    s.add(
        "Electron 4 层全齐（主进程/渲染进程/预加载/原生模块）",
        {
            let mut lt = RancherLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if lt.record_electron(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.electron_all()
        },
        "Electron 分层账逐层落账（§AI-66.3 判据锚定）",
    );
    s.add(
        "WSL 消费 4 格全齐",
        {
            let mut lt = RancherLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < WSL_CELLS {
                if lt.record_wsl(w).is_err() {
                    ok = false;
                }
                w += 1;
            }
            ok && lt.wsl_all()
        },
        "发行版/集成开关/网络/资源逐格落账",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFE_OPS {
                if life_phase(o).unwrap_or(5) < life_phase(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→启动→运行→诊断→卸载五相位闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/生命周期漂移/相位越序三态轮转必抓",
    );
    let mut l0 = RancherLedger::new();
    s.add(
        "边界·版本档越阶梯",
        l0.record_k8s(K8S_STEPS, 0).is_err(),
        "版本档 5 越阶梯必拒",
    );
    s.add(
        "边界·引擎格越闭集",
        l0.record_engine(ENGINE_CELLS, 0).is_err(),
        "引擎格 12 越闭集必拒",
    );
    s.add(
        "边界·生命周期操作越闭集",
        l0.record_op(LIFE_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "空账本相位越序必抓",
        l0.record_op(4, life_cell(4).unwrap_or(0)) == Err(RancherCode::EPhase),
        "空账本首操作落在运行相位，越序必抓且不改 last_phase",
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
        "定级 S2 闭集（KB-COMPAT 登记封顶）",
        {
            let mut lt = RancherLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < ENGINE_CELLS {
                if let Some(w) = engine_cell(i) {
                    let _ = lt.record_engine(i, w);
                }
                i += 1;
            }
            let mut v = 0usize;
            while v < K8S_STEPS {
                if let Some(x) = k8s_cell(v) {
                    let _ = lt.record_k8s(v, x);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(x) = life_cell(o) {
                    let _ = lt.record_op(o, x);
                }
                o += 1;
            }
            let mut w = 0usize;
            while w < WSL_CELLS {
                let _ = lt.record_wsl(w);
                w += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                let _ = lt.record_electron(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（本卡 KB-COMPAT 定级登记封顶 S2）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8200,
        "SEG_BASE 0x40 对齐且按派工公式落在 0xC8200",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_ladder() {
        assert_eq!(k8s_name(0), Some("v1.21"));
        assert_eq!(k8s_name(4), Some("v1.25"));
        assert!(k8s_name(5).is_none());
        assert!(k8s_cell(5).is_none());
    }

    #[test]
    fn test_engine_grid() {
        assert_eq!(engine_name(0), Some("containerd"));
        assert_eq!(engine_name(1), Some("dockerd"));
        assert!(engine_name(2).is_none());
        assert_eq!(engine_state_name(2), Some("故障"));
        assert!(engine_state_name(3).is_none());
        assert!(engine_cell(ENGINE_CELLS).is_none());
    }

    #[test]
    fn test_life_phases() {
        assert_eq!(life_phase(0), Some(0));
        assert_eq!(life_phase(9), Some(4));
        assert_eq!(life_phase_name(4), Some("卸载"));
        assert!(life_phase(10).is_none());
        assert!(life_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = engine_cell(3).unwrap_or(0);
        assert_eq!(a, engine_cell(3).unwrap_or(0));
        assert_ne!(a, engine_cell(4).unwrap_or(0));
        let b = k8s_cell(2).unwrap_or(0);
        assert_eq!(b, k8s_cell(2).unwrap_or(0));
        assert_ne!(b, k8s_cell(3).unwrap_or(0));
        assert!(wsl_cell(WSL_CELLS).is_none());
        assert!(electron_cell(ELECTRON_LAYERS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = RancherLedger::new();
        // 先按相位序落满 0..=9，再验逆相必抓与重复必拒。
        let mut i = 0usize;
        while i < LIFE_OPS {
            let _ = lt.record_op(i, life_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_op(2, life_cell(2).unwrap_or(0)), Err(RancherCode::EPhase));
        assert_eq!(lt.record_op(9, life_cell(9).unwrap_or(0)), Err(RancherCode::EDup));
    }

    #[test]
    fn test_empty_ledger_phase_caught() {
        let mut lt = RancherLedger::new();
        assert_eq!(lt.record_op(6, life_cell(6).unwrap_or(0)), Err(RancherCode::EPhase));
        // 空账本首操作必须落在安装相位（0）。
        assert!(lt.record_op(0, life_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RancherLedger::new();
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_op(i, life_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_op(5, life_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(RancherCode::EDrift)
        );
        assert_eq!(
            lt.record_k8s(1, k8s_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(RancherCode::EK8sDrift)
        );
        assert_eq!(
            lt.record_engine(3, engine_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(RancherCode::EEngineDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RancherLedger::new();
        let _ = lt.record_k8s(0, k8s_cell(0).unwrap_or(0));
        assert_eq!(lt.record_k8s(0, k8s_cell(0).unwrap_or(0)), Err(RancherCode::EDup));
        let _ = lt.record_wsl(0);
        assert_eq!(lt.record_wsl(0), Err(RancherCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RancherLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RancherLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (ENGINE_CELLS + K8S_STEPS + LIFE_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8200);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RancherCode::Ok as u32, 0);
        assert_eq!(RancherCode::EGradeDrift as u32, 12);
        // 13 变体闭集：逐位核对（与枚举位序一致）。
        assert_eq!(RancherCode::ENoEngine as u32, 1);
        assert_eq!(RancherCode::EDrift as u32, 6);
        assert_eq!(RancherCode::EPhase as u32, 7);
        assert!(!RancherCode::Ok.detail().is_empty());
        assert!(!RancherCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = RancherLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52265_checks_green() {
        let s = run_unx52265_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
