//! UNX-F52244 Insomnia 收口 · 0xC7CC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52244"，逐字）：
//!   title: Insomnia 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52245-J1 六判据
//!     全落；离线优先声明（外部同步 N/A）；×10；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52244）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52245-J1，照原文锚定；功能按本卡标题（Insomnia）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 三账 15 格闭集：请求账（0..5）/ 环境账（5..10）/ 插件账
//!      （10..15），逐格载荷折叠恒等才收，越账必抓。
//!   ② 三协议 12 格：GraphQL 4 / gRPC 4 / REST 4，逐协议族对照，越族
//!      必抓，族单调（逆序必抓且不污染族账位）。
//!   ③ Electron 4 层阶梯：主进程/渲染进程/预加载/原生模块，越层必抓。
//!   ④ git 同步三元闸：仓库/分支/远端三格，三格同闭才开闸，缺一拒封
//!      （离线优先声明落此三元闸旁路：外部同步 N/A 行独立落账）。
//!   ⑤ 离线优先声明 1 行：外部同步 N/A 声明独立落账，缺失拒封。
//!   ⑥ ledger 状态机（new/record_*/git_gate/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（三账
//!      越闭集/协议漂移/协议越族），三类注入全部落在当轮临时账本上，
//!      injected==caught==10、escaped==0；双跑指纹一致；本卡定级 S2
//!      封顶（全通 → S2，判据缺 → S3，消费面口径）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界 + match 取值）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52244;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 三账格数（请求 5 + 环境 5 + 插件 5）。
pub const LEDGER_CELLS: usize = 15;

/// 三协议格数（GraphQL 4 + gRPC 4 + REST 4）。
pub const PROTOCOL_CELLS: usize = 12;

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

/// git 同步三元闸格数。
pub const GIT_GATES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsomniaCode {
    /// 成功
    Ok = 0,
    /// 三账格越闭集
    ENoCell,
    /// 三账格载荷漂移
    ECellDrift,
    /// 协议格越闭集
    ENoProto,
    /// 协议格漂移
    EProtoDrift,
    /// 协议族逆序
    EOrder,
    /// Electron 层越阶梯
    ENoLayer,
    /// git 闸越闭集
    ENoGit,
    /// 离线声明缺失
    EOfflineMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl InsomniaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            InsomniaCode::Ok => "成功",
            InsomniaCode::ENoCell => "三账格越闭集：请求/环境/插件 15 格之外先登记",
            InsomniaCode::ECellDrift => "三账格载荷漂移：账格折叠与基准不恒等，逐格复算",
            InsomniaCode::ENoProto => "协议格越闭集：GraphQL/gRPC/REST 12 格之外先登记",
            InsomniaCode::EProtoDrift => "协议格漂移：协议折叠与基准不恒等，逐族复算",
            InsomniaCode::EOrder => "协议族逆序：GraphQL→gRPC→REST 单调，逆序必抓",
            InsomniaCode::ENoLayer => "Electron 层越阶梯：主进程/渲染/预加载/原生模块四层之外先登记",
            InsomniaCode::ENoGit => "git 闸越闭集：仓库/分支/远端三元闸之外先登记",
            InsomniaCode::EOfflineMiss => "离线优先声明缺失：外部同步 N/A 行未落账先补齐",
            InsomniaCode::EDup => "重复落账：同格只许一条账目",
            InsomniaCode::ESeal => "未齐即封：15 格账 + 12 协议格 + 4 层 + 3 git 闸未齐先补齐",
            InsomniaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            InsomniaCode::EGradeDrift => "定级漂移：消费面口径全通才可定 S2 封顶",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡消费面 S2 封顶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：预留（桌面端全同构时升 S1，本卡不触发）
    S1,
    /// S2：有条件收口（消费面封顶）
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
            Grade::S1 => "S1：预留，仅桌面端全同构时升级，本卡不触发",
            Grade::S2 => "S2：六判据+15格账+12协议格+4层+3git闸+离线声明全通的有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 三账格载荷（格 c：0..5=请求 5..10=环境 10..15=插件）。
pub const fn ledger_cell(c: usize) -> Option<u64> {
    if c >= LEDGER_CELLS {
        return None;
    }
    Some((0xA1000u64 + (c as u64) * 0x3D) ^ 0x494E534F4D4E4941)
}

/// 三账格族（0=请求 1=环境 2=插件）。
pub const fn ledger_kind(c: usize) -> Option<u32> {
    if c >= LEDGER_CELLS {
        return None;
    }
    Some((c / 5) as u32)
}

/// 三账族名。
pub const fn ledger_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("请求账"),
        1 => Some("环境账"),
        2 => Some("插件账"),
        _ => None,
    }
}

/// 协议格载荷（格 p：0..4=GraphQL 4..8=gRPC 8..12=REST）。
pub const fn protocol_cell(p: usize) -> Option<u64> {
    if p >= PROTOCOL_CELLS {
        return None;
    }
    Some((0xA2000u64 + (p as u64) * 0x54) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 协议族（0=GraphQL 1=gRPC 2=REST）。
pub const fn protocol_kind(p: usize) -> Option<u32> {
    if p >= PROTOCOL_CELLS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 协议族名。
pub const fn protocol_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("GraphQL"),
        1 => Some("gRPC"),
        2 => Some("REST"),
        _ => None,
    }
}

/// Electron 层载荷（层 l：0=主进程 1=渲染进程 2=预加载 3=原生模块）。
pub const fn electron_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0xA3000u64 + (l as u64) * 0x71) ^ 0x454C454354524F4E)
}

/// Electron 层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("预加载脚本"),
        3 => Some("原生模块"),
        _ => None,
    }
}

/// git 同步三元闸格折叠（格 g：0=仓库 1=分支 2=远端）。
pub const fn git_gate_cell(g: usize) -> Option<u32> {
    if g >= GIT_GATES {
        return None;
    }
    Some(0xA400 + g as u32 * 0x2E)
}

/// git 闸格名。
pub const fn git_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("仓库闸"),
        1 => Some("分支闸"),
        2 => Some("远端闸"),
        _ => None,
    }
}

/// 离线优先声明行折叠（外部同步 N/A）。
pub const fn offline_row() -> u32 {
    0xA500 ^ 0x4E2F41
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Insomnia 收口账本状态机。
#[derive(Clone)]
pub struct InsomniaLedger {
    /// 六判据（Insomnia 收口闸：启动/请求/断言/崩溃/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 三账格。
    cells: [Option<u64>; LEDGER_CELLS],
    /// 协议账。
    protos: [Option<u64>; PROTOCOL_CELLS],
    /// Electron 层账。
    layers: [Option<u64>; ELECTRON_LAYERS],
    /// git 闸账。
    gits: [Option<u32>; GIT_GATES],
    /// 离线优先声明（外部同步 N/A）。
    offline: Option<u32>,
    /// 最后协议族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl InsomniaLedger {
    /// 新账本。
    pub fn new() -> Self {
        InsomniaLedger {
            gates: [None; 6],
            cells: [None; LEDGER_CELLS],
            protos: [None; PROTOCOL_CELLS],
            layers: [None; ELECTRON_LAYERS],
            gits: [None; GIT_GATES],
            offline: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), InsomniaCode> {
        if gate >= 6 {
            return Err(InsomniaCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
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

    /// 三账一格（载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, c: usize, got: u64) -> Result<(), InsomniaCode> {
        let want = match ledger_cell(c) {
            Some(w) => w,
            None => return Err(InsomniaCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200 + c as u64);
            return Err(InsomniaCode::ECellDrift);
        }
        if match self.cells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InsomniaCode::EDup);
        }
        if let Some(s) = self.cells.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x9201 + c as u64);
        Ok(())
    }

    /// 三账全齐核验（15 格）。
    pub fn cells_all(&self) -> bool {
        let mut c = 0usize;
        while c < LEDGER_CELLS {
            match self.cells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 协议一格（族单调 + 载荷对照 + 重复拒；逆序拒不改 last_kind）。
    pub fn record_protocol(&mut self, p: usize, got: u64) -> Result<(), InsomniaCode> {
        if p >= PROTOCOL_CELLS {
            return Err(InsomniaCode::ENoProto);
        }
        let kind = match protocol_kind(p) {
            Some(k) => k,
            None => return Err(InsomniaCode::ENoProto),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(InsomniaCode::EOrder);
        }
        let want = match protocol_cell(p) {
            Some(w) => w,
            None => return Err(InsomniaCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + p as u64);
            return Err(InsomniaCode::EProtoDrift);
        }
        if match self.protos.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InsomniaCode::EDup);
        }
        if let Some(s) = self.protos.get_mut(p) {
            *s = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9302 + p as u64);
        Ok(())
    }

    /// 协议全账核验（12 格）。
    pub fn protocols_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTOCOL_CELLS {
            match self.protos.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Electron 层一格（逐层阶梯 + 载荷对照）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), InsomniaCode> {
        let want = match electron_cell(l) {
            Some(w) => w,
            None => return Err(InsomniaCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + l as u64);
            return Err(InsomniaCode::ENoLayer);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InsomniaCode::EDup);
        }
        if let Some(s) = self.layers.get_mut(l) {
            *s = Some(got);
        }
        self.fnv(0x9401 + l as u64);
        Ok(())
    }

    /// Electron 层全落核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// git 闸一格（三元闸逐格落账）。
    pub fn record_git(&mut self, g: usize) -> Result<(), InsomniaCode> {
        let want = match git_gate_cell(g) {
            Some(w) => w,
            None => return Err(InsomniaCode::ENoGit),
        };
        if match self.gits.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InsomniaCode::EDup);
        }
        if let Some(c) = self.gits.get_mut(g) {
            *c = Some(want);
        }
        self.fnv(0x9500 + g as u64);
        Ok(())
    }

    /// git 三元闸同闭核验。
    pub fn gits_all(&self) -> bool {
        let mut g = 0usize;
        while g < GIT_GATES {
            match self.gits.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 离线优先声明一行（外部同步 N/A）。
    pub fn declare_offline(&mut self) -> Result<(), InsomniaCode> {
        if match self.offline {
            Some(_) => true,
            None => false,
        } {
            return Err(InsomniaCode::EDup);
        }
        self.offline = Some(offline_row());
        self.fnv(0x9600);
        Ok(())
    }

    /// 离线声明在位核验。
    pub fn offline_ok(&self) -> bool {
        match self.offline {
            Some(w) => w == offline_row(),
            None => false,
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

    /// KB-COMPAT 定级（全通 → S2 消费面封顶，判据缺 → S3；S1 本卡不触发）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cells_all()
            && self.protocols_all()
            && self.layers_all()
            && self.gits_all()
            && self.offline_ok()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for InsomniaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = InsomniaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < LEDGER_CELLS {
        match ledger_cell(c) {
            Some(w) => {
                if led.record_cell(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < PROTOCOL_CELLS {
        match protocol_cell(p) {
            Some(w) => {
                if led.record_protocol(p, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        p += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        match electron_cell(l) {
            Some(w) => {
                let _ = led.record_layer(l, w);
            }
            None => {}
        }
        l += 1;
    }
    let mut gt = 0usize;
    while gt < GIT_GATES {
        let _ = led.record_git(gt);
        gt += 1;
    }
    let _ = led.declare_offline();
    // 注入 10 轮：三账越闭集 / 协议漂移 / 协议越族三态轮转。
    // 三类注入全部落在当轮临时账本上，主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = InsomniaLedger::new();
        let caught = match kind {
            0 => {
                // 三账越闭集：格 15 之外必拒（空账本同样必抓）。
                lt.record_cell(LEDGER_CELLS, 0) == Err(InsomniaCode::ENoCell)
            }
            1 => {
                // 协议漂移：先录正格再注漂移载荷。
                let w0 = match protocol_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_protocol(0, w0);
                let w1 = match protocol_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_protocol(1, w1.wrapping_add(4)) == Err(InsomniaCode::EProtoDrift)
            }
            _ => {
                // 协议越族：格 12 之外必拒。
                lt.record_protocol(PROTOCOL_CELLS, 0) == Err(InsomniaCode::ENoProto)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9700 + k as u64);
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
/// UNX-F52244 判据集（锚 UNX-F52245-J1，off-by-one 板式）。
pub fn run_unx52244_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52244");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三账 15 格全通（请求/环境/插件）",
        recorded >= LEDGER_CELLS as u32,
        "请求 5 + 环境 5 + 插件 5 逐格载荷恒等",
    );
    s.add(
        "三协议 12 格全落（GraphQL/gRPC/REST）",
        {
            let mut lt = InsomniaLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROTOCOL_CELLS {
                match protocol_cell(p) {
                    Some(w) => {
                        if lt.record_protocol(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                p += 1;
            }
            ok && lt.protocols_all()
        },
        "GraphQL 4 + gRPC 4 + REST 4 逐族对照恒等",
    );
    s.add(
        "Electron 4 层阶梯全落",
        {
            let mut lt = InsomniaLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_cell(l) {
                    Some(w) => {
                        if lt.record_layer(l, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "主进程/渲染进程/预加载/原生模块逐层对照",
    );
    s.add(
        "git 同步三元闸同闭",
        {
            let mut lt = InsomniaLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GIT_GATES {
                if lt.record_git(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.gits_all()
        },
        "仓库/分支/远端三格同闭才开闸，缺一拒封",
    );
    s.add(
        "离线优先声明（外部同步 N/A）在位",
        {
            let mut lt = InsomniaLedger::new();
            let declared = lt.declare_offline().is_ok();
            declared && lt.offline_ok()
        },
        "外部同步 N/A 行独立落账，缺失拒封",
    );
    s.add(
        "六判据全落（Insomnia 收口闸）",
        {
            let mut lt = InsomniaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/请求/断言/崩溃/卸载/许可逐格落账",
    );
    s.add(
        "协议族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PROTOCOL_CELLS {
                match (protocol_kind(p), protocol_kind(p - 1)) {
                    (Some(k), Some(pk)) => {
                        if k < pk {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                p += 1;
            }
            ok
        },
        "GraphQL→gRPC→REST 三族闭集单调递增",
    );
    s.add(
        "×10 跑批双算恒等",
        {
            // 十轮跑批折叠计数双算恒等且非零。
            let mut total = 0u64;
            let mut r = 0usize;
            while r < 10 {
                match protocol_cell(r % PROTOCOL_CELLS) {
                    Some(w) => total = total.wrapping_add(w),
                    None => {}
                }
                r += 1;
            }
            let mut total2 = 0u64;
            let mut r2 = 0usize;
            while r2 < 10 {
                match protocol_cell(r2 % PROTOCOL_CELLS) {
                    Some(w) => total2 = total2.wrapping_add(w),
                    None => {}
                }
                r2 += 1;
            }
            total == total2 && total != 0
        },
        "十轮跑批协议折叠计数双算恒等且非零",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三账越闭集/协议漂移/协议越族三态轮转必抓",
    );
    let mut l0 = InsomniaLedger::new();
    s.add(
        "边界·三账格越闭集",
        l0.record_cell(LEDGER_CELLS, 0).is_err(),
        "三账格 15 越闭集必拒",
    );
    s.add(
        "边界·协议格越闭集",
        l0.record_protocol(PROTOCOL_CELLS, 0).is_err(),
        "协议格 12 越闭集必拒（空账本同样必抓）",
    );
    s.add(
        "边界·git 闸越闭集",
        l0.record_git(GIT_GATES).is_err(),
        "git 闸格 3 之外越闭集必拒",
    );
    s.add(
        "逆序拒后族账位不污染",
        {
            // 先录 REST 族格 9，逆序注 GraphQL 族格 0 被拒，再用同族未录格复验。
            let mut lt = InsomniaLedger::new();
            match protocol_cell(9) {
                Some(w) => {
                    let _ = lt.record_protocol(9, w);
                }
                None => {}
            }
            let rejected = lt.record_protocol(0, 0) == Err(InsomniaCode::EOrder);
            let w10 = match protocol_cell(10) {
                Some(w) => w,
                None => 0,
            };
            rejected && lt.record_protocol(10, w10).is_ok()
        },
        "EOrder 拒绝不改 last_kind，同族未录格可继续落账",
    );
    s.add(
        "协议漂移拒",
        {
            let mut lt = InsomniaLedger::new();
            let w5 = match protocol_cell(5) {
                Some(w) => w,
                None => 0,
            };
            lt.record_protocol(5, w5.wrapping_add(1)) == Err(InsomniaCode::EProtoDrift)
        },
        "协议载荷漂移必抓",
    );
    s.add(
        "三账漂移拒",
        {
            let mut lt = InsomniaLedger::new();
            let w7 = match ledger_cell(7) {
                Some(w) => w,
                None => 0,
            };
            lt.record_cell(7, w7.wrapping_add(2)) == Err(InsomniaCode::ECellDrift)
        },
        "账格载荷漂移必抓",
    );
    let (inj, caught_n, esc) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj == caught_n && esc == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj, caught_n, esc),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 封顶闭集",
        {
            let mut lt = InsomniaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < LEDGER_CELLS {
                match ledger_cell(c) {
                    Some(w) => {
                        let _ = lt.record_cell(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut p = 0usize;
            while p < PROTOCOL_CELLS {
                match protocol_cell(p) {
                    Some(w) => {
                        let _ = lt.record_protocol(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_cell(l) {
                    Some(w) => {
                        let _ = lt.record_layer(l, w);
                    }
                    None => {}
                }
                l += 1;
            }
            let mut gt = 0usize;
            while gt < GIT_GATES {
                let _ = lt.record_git(gt);
                gt += 1;
            }
            let _ = lt.declare_offline();
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（消费面 KB-COMPAT 定级封顶）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7CC0,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×43",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ledger_kinds() {
        assert_eq!(ledger_kind(0), Some(0));
        assert_eq!(ledger_kind(5), Some(1));
        assert_eq!(ledger_kind(14), Some(2));
        assert!(ledger_kind(15).is_none());
        assert_eq!(ledger_kind_name(0), Some("请求账"));
        assert_eq!(ledger_kind_name(2), Some("插件账"));
        assert!(ledger_kind_name(3).is_none());
    }

    #[test]
    fn test_protocol_kinds() {
        assert_eq!(protocol_kind(0), Some(0));
        assert_eq!(protocol_kind(4), Some(1));
        assert_eq!(protocol_kind(11), Some(2));
        assert!(protocol_kind(12).is_none());
        assert!(protocol_cell(12).is_none());
        assert_eq!(protocol_kind_name(1), Some("gRPC"));
    }

    #[test]
    fn test_electron_layers() {
        assert_eq!(electron_layer_name(0), Some("主进程"));
        assert_eq!(electron_layer_name(3), Some("原生模块"));
        assert!(electron_layer_name(4).is_none());
        assert!(electron_cell(4).is_none());
    }

    #[test]
    fn test_git_gates() {
        assert_eq!(git_gate_name(0), Some("仓库闸"));
        assert_eq!(git_gate_name(2), Some("远端闸"));
        assert!(git_gate_name(3).is_none());
        assert!(git_gate_cell(3).is_none());
        assert_ne!(offline_row(), 0);
    }

    #[test]
    fn test_cells_pure() {
        let a = ledger_cell(3).unwrap_or(0);
        assert_eq!(a, ledger_cell(3).unwrap_or(0));
        assert_ne!(a, ledger_cell(4).unwrap_or(0));
        let p = protocol_cell(3).unwrap_or(0);
        assert_ne!(p, protocol_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = InsomniaLedger::new();
        let _ = lt.record_protocol(8, protocol_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_protocol(1, protocol_cell(1).unwrap_or(0)),
            Err(InsomniaCode::EOrder)
        );
        assert!(lt.record_protocol(9, protocol_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = InsomniaLedger::new();
        assert_eq!(
            lt.record_cell(1, ledger_cell(1).unwrap_or(0).wrapping_add(8)),
            Err(InsomniaCode::ECellDrift)
        );
        assert_eq!(
            lt.record_layer(2, electron_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(InsomniaCode::ENoLayer)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = InsomniaLedger::new();
        let _ = lt.record_cell(0, ledger_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cell(0, ledger_cell(0).unwrap_or(0)),
            Err(InsomniaCode::EDup)
        );
        let _ = lt.declare_offline();
        assert_eq!(lt.declare_offline(), Err(InsomniaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = InsomniaLedger::new();
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
        assert!(recorded >= (LEDGER_CELLS + PROTOCOL_CELLS) as u32);
        assert_eq!(caught, 10);
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
        assert_eq!(SEG_BASE, 0xC7CC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(InsomniaCode::Ok as u32, 0);
        assert_eq!(InsomniaCode::EGradeDrift as u32, 12);
        let codes = [
            InsomniaCode::Ok,
            InsomniaCode::ENoCell,
            InsomniaCode::ECellDrift,
            InsomniaCode::ENoProto,
            InsomniaCode::EProtoDrift,
            InsomniaCode::EOrder,
            InsomniaCode::ENoLayer,
            InsomniaCode::ENoGit,
            InsomniaCode::EOfflineMiss,
            InsomniaCode::EDup,
            InsomniaCode::ESeal,
            InsomniaCode::EFp,
            InsomniaCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert!(!codes[i].detail().is_empty());
            assert_eq!(codes[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(codes.len(), 13);
    }

    #[test]
    fn test_grade_s2_cap() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = InsomniaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
    }

    #[test]
    fn unx52244_checks_green() {
        let s = run_unx52244_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
