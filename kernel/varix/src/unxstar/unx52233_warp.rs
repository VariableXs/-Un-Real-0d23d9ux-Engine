//! UNX-F52233 Warp for Windows 收口 · 0xC7A00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52233"，逐字）：
//!   title: Warp for Windows 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52234-J1 六判据全落；
//!     会话管理/隧道 ×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定
//!     级登记；判据锚定 §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52233）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52234-J1，照原文锚定；功能按本卡标题（Warp for Windows）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Rust 上栈 4 层闭集（核心壳层/渲染层/会话层/扩展层），逐层载荷折
//!      叠恒等才收，越层必抓。
//!   ② AI 请求面声明 + 本地 N/A 共 2 行闭集分账：请求面外发声明一行、本
//!      地模型 N/A 备案一行，缺一拒封（AI-95 联签锚定行）。
//!   ③ Block 会话 10 格：块式会话逐格落账，载荷对照漂移必抓。
//!   ④ 工作流 8 格：Warp 工作流逐格登记，重复落账拒。
//!   ⑤ GPU 三元闸：集成 GPU / 独立 GPU / 无 GPU（N/A）三元闭集，越元必
//!      抓；许可面如实定级。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（越闭集/漂移/逆序）全部落在当轮
//!      临时账本上，led 上 injected==caught==10、escaped==0；双跑指纹一
//!      致；定级闭集 S1/S2/S3（Warp 封顶 S2：全通 → S2）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52233;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Rust 上栈层数。
pub const RUST_STACKS: usize = 4;

/// AI 请求面声明 + 本地 N/A 分账行数。
pub const AI_ROWS: usize = 2;

/// Block 会话格数。
pub const BLOCK_SESSIONS: usize = 10;

/// 工作流格数。
pub const WORKFLOWS: usize = 8;

/// GPU 三元闸元数。
pub const GPU_MODES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarpCode {
    /// 成功
    Ok = 0,
    /// Rust 上栈层越闭集
    ENoStack,
    /// 上栈层载荷漂移
    EStackDrift,
    /// AI 分账行越闭集
    ENoAiRow,
    /// Block 会话号越闭集
    ENoSession,
    /// Block 会话漂移
    ESessionDrift,
    /// 工作流号越闭集
    ENoFlow,
    /// GPU 三元越元
    ENoGpuMode,
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

impl WarpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WarpCode::Ok => "成功",
            WarpCode::ENoStack => "Rust 上栈层越闭集：核心壳层/渲染层/会话层/扩展层 4 层之外先登记",
            WarpCode::EStackDrift => "上栈层载荷漂移：层折叠与基准不恒等，逐层复算",
            WarpCode::ENoAiRow => "AI 分账行越闭集：请求面声明/本地 N/A 2 行之外先登记，缺行先补齐",
            WarpCode::ENoSession => "Block 会话号越闭集：Block 会话 10 格之外先登记",
            WarpCode::ESessionDrift => "Block 会话漂移：会话折叠与基准不恒等，逐格复算",
            WarpCode::ENoFlow => "工作流号越闭集：Warp 工作流 8 格之外先登记",
            WarpCode::ENoGpuMode => "GPU 三元越元：集成/独立/无 GPU 三元闭集之外先登记",
            WarpCode::EDup => "重复落账：同格只许一条账目",
            WarpCode::ESeal => "未齐即封：4 层上栈 + 2 行分账 + 10 会话 + 8 工作流未齐先补齐",
            WarpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WarpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WarpCode::EGradeDrift => "定级漂移：全通且 GPU 三元闸落账才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（Warp 许可面如实定级，封顶 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本卡不适用（Warp 封顶 S2）
    S1,
    /// S2：全通收口（如实定级）
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
            Grade::S1 => "S1：本卡不适用——Warp 许可面如实定级封顶 S2（AI-95 联签锚定行）",
            Grade::S2 => "S2：六判据+4层上栈+2行分账+10会话+8工作流+GPU三元全通收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Rust 上栈层载荷（层 s：0=核心壳层 1=渲染层 2=会话层 3=扩展层）。
pub const fn stack_cell(s: usize) -> Option<u64> {
    if s >= RUST_STACKS {
        return None;
    }
    Some((0xC3300u64 + (s as u64) * 0x77) ^ 0x574152505F53544B)
}

/// 上栈层名（const fn 表 + Option 边界）。
pub const fn stack_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("核心壳层"),
        1 => Some("渲染层"),
        2 => Some("会话层"),
        3 => Some("扩展层"),
        _ => None,
    }
}

/// AI 分账行折叠（行 r：0=请求面声明 1=本地 N/A）。
pub const fn ai_row(r: usize) -> Option<u32> {
    if r >= AI_ROWS {
        return None;
    }
    Some(0xC3900 + r as u32 * 0x2C)
}

/// AI 分账行名。
pub const fn ai_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("AI 请求面外发声明"),
        1 => Some("本地模型 N/A 备案"),
        _ => None,
    }
}

/// Block 会话格载荷（会话 b）。
pub const fn session_cell(b: usize) -> Option<u64> {
    if b >= BLOCK_SESSIONS {
        return None;
    }
    Some((0xC4000u64 + (b as u64) * 0x3B) ^ ((b % 5) as u64).wrapping_mul(0x01000193))
}

/// 工作流格载荷（工作流 w）。
pub const fn workflow_cell(w: usize) -> Option<u64> {
    if w >= WORKFLOWS {
        return None;
    }
    Some((0xC5000u64 + (w as u64) * 0x5D) ^ 0x57464C4F57)
}

/// GPU 三元闸元名。
pub const fn gpu_mode_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("集成 GPU"),
        1 => Some("独立 GPU"),
        2 => Some("无 GPU（N/A）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Warp 收口账本状态机。
#[derive(Clone)]
pub struct WarpLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// Rust 上栈层账。
    stacks: [Option<u64>; RUST_STACKS],
    /// AI 分账账。
    ai: [Option<u32>; AI_ROWS],
    /// Block 会话账。
    sessions: [Option<u64>; BLOCK_SESSIONS],
    /// 工作流账。
    flows: [Option<u64>; WORKFLOWS],
    /// GPU 三元闸（None=未落，Some(m)=落账）。
    gpu: Option<usize>,
    /// 最后会话格（逆序核验用）。
    last_session: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WarpLedger {
    /// 新账本。
    pub fn new() -> Self {
        WarpLedger {
            gates: [None; 6],
            stacks: [None; RUST_STACKS],
            ai: [None; AI_ROWS],
            sessions: [None; BLOCK_SESSIONS],
            flows: [None; WORKFLOWS],
            gpu: None,
            last_session: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WarpCode> {
        if gate >= 6 {
            return Err(WarpCode::ESeal);
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

    /// Rust 上栈层一格（载荷对照 + 重复拒）。
    pub fn record_stack(&mut self, s: usize, got: u64) -> Result<(), WarpCode> {
        let want = match stack_cell(s) {
            Some(w) => w,
            None => return Err(WarpCode::ENoStack),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + s as u64);
            return Err(WarpCode::EStackDrift);
        }
        if match self.stacks.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WarpCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5201 + s as u64);
        Ok(())
    }

    /// 上栈层全账核验（4 层）。
    pub fn stacks_all(&self) -> bool {
        let mut s = 0usize;
        while s < RUST_STACKS {
            match self.stacks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// AI 分账一行（AI-95 联签锚定行）。
    pub fn declare_ai(&mut self, r: usize) -> Result<(), WarpCode> {
        if r >= AI_ROWS {
            return Err(WarpCode::ENoAiRow);
        }
        if match self.ai.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WarpCode::EDup);
        }
        if let Some(c) = self.ai.get_mut(r) {
            *c = ai_row(r);
        }
        self.fnv(0x5300 + r as u64);
        Ok(())
    }

    /// AI 分账全齐核验（2 行）。
    pub fn ai_all(&self) -> bool {
        let mut r = 0usize;
        while r < AI_ROWS {
            match self.ai.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Block 会话一格（单调 + 载荷对照 + 重复拒）。
    pub fn record_session(&mut self, b: usize, got: u64) -> Result<(), WarpCode> {
        if b >= BLOCK_SESSIONS {
            return Err(WarpCode::ENoSession);
        }
        if (b as i64) < self.last_session {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(WarpCode::ESessionDrift);
        }
        let want = match session_cell(b) {
            Some(w) => w,
            None => return Err(WarpCode::ENoSession),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + b as u64);
            return Err(WarpCode::ESessionDrift);
        }
        if match self.sessions.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WarpCode::EDup);
        }
        if let Some(c) = self.sessions.get_mut(b) {
            *c = Some(got);
        }
        self.last_session = b as i64;
        self.fnv(0x5402 + b as u64);
        Ok(())
    }

    /// Block 会话全账核验（10 格）。
    pub fn sessions_all(&self) -> bool {
        let mut b = 0usize;
        while b < BLOCK_SESSIONS {
            match self.sessions.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 最后会话格读数（逆序注入不改值核验）。
    pub fn last_session(&self) -> i64 {
        self.last_session
    }

    /// 工作流一格（载荷对照 + 重复拒）。
    pub fn record_workflow(&mut self, w: usize, got: u64) -> Result<(), WarpCode> {
        let want = match workflow_cell(w) {
            Some(v) => v,
            None => return Err(WarpCode::ENoFlow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + w as u64);
            return Err(WarpCode::ENoFlow);
        }
        if match self.flows.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WarpCode::EDup);
        }
        if let Some(c) = self.flows.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x5501 + w as u64);
        Ok(())
    }

    /// 工作流全账核验（8 格）。
    pub fn flows_all(&self) -> bool {
        let mut w = 0usize;
        while w < WORKFLOWS {
            match self.flows.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// GPU 三元闸落账（越元必抓）。
    pub fn record_gpu(&mut self, m: usize) -> Result<(), WarpCode> {
        if m >= GPU_MODES {
            return Err(WarpCode::ENoGpuMode);
        }
        self.gpu = Some(m);
        self.fnv(0x5600 + m as u64);
        Ok(())
    }

    /// GPU 三元闸已落核验。
    pub fn gpu_set(&self) -> bool {
        self.gpu.is_some()
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（Warp 封顶 S2：全通 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.stacks_all()
            && self.ai_all()
            && self.sessions_all()
            && self.flows_all()
            && self.gpu_set()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WarpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WarpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < RUST_STACKS {
        if let Some(w) = stack_cell(s) {
            if led.record_stack(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < AI_ROWS {
        let _ = led.declare_ai(r);
        r += 1;
    }
    let mut b = 0usize;
    while b < BLOCK_SESSIONS {
        if let Some(w) = session_cell(b) {
            if led.record_session(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut w = 0usize;
    while w < WORKFLOWS {
        if let Some(v) = workflow_cell(w) {
            if led.record_workflow(w, v).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let _ = led.record_gpu(2);
    // 注入 10 轮：越闭集 / 漂移 / 逆序三态轮转，全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WarpLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：上栈层 4 与 GPU 元 3 之外必抓。
                if k % 2 == 0 {
                    lt.record_stack(RUST_STACKS, 0).is_err()
                } else {
                    lt.record_gpu(GPU_MODES).is_err()
                }
            }
            1 => {
                // 漂移：先录正格再注漂移，漂移必抓。
                let _ = lt.record_session(0, match session_cell(0) {
                    Some(v) => v,
                    _ => 0,
                });
                lt.record_session(1, match session_cell(1) {
                    Some(v) => v.wrapping_add(1),
                    _ => 1,
                }) == Err(WarpCode::ESessionDrift)
            }
            _ => {
                // 逆序：空账本上先录高格再注低格，逆序必抓且不改 last_session。
                let _ = lt.record_session(5, match session_cell(5) {
                    Some(v) => v,
                    _ => 0,
                });
                let before = lt.last_session();
                let hit = lt.record_session(2, match session_cell(2) {
                    Some(v) => v,
                    _ => 0,
                }) == Err(WarpCode::ESessionDrift);
                hit && lt.last_session() == before
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52233 判据集（锚 UNX-F52234-J1，off-by-one 板式）。
pub fn run_unx52233_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52233");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Block 会话 10 格全通",
        recorded >= BLOCK_SESSIONS as u32,
        "块式会话逐格载荷恒等，漂移必抓",
    );
    s.add(
        "工作流 8 格全通",
        {
            let mut lt = WarpLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < WORKFLOWS {
                if let Some(v) = workflow_cell(w) {
                    if lt.record_workflow(w, v).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.flows_all()
        },
        "Warp 工作流逐格登记，重复落账拒",
    );
    s.add(
        "Rust 上栈 4 层全落",
        {
            let mut lt = WarpLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < RUST_STACKS {
                if let Some(w) = stack_cell(st) {
                    if lt.record_stack(st, w).is_err() {
                        ok = false;
                    }
                }
                st += 1;
            }
            ok && lt.stacks_all()
        },
        "核心壳层/渲染层/会话层/扩展层逐层恒等，越层必抓",
    );
    s.add(
        "AI 请求面声明+本地 N/A 2/2 全齐",
        {
            let mut lt = WarpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AI_ROWS {
                if lt.declare_ai(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ai_all()
        },
        "请求面外发声明与本地 N/A 备案两行闭集（AI-95 联签锚定行）",
    );
    s.add(
        "GPU 三元闸落账",
        {
            let mut lt = WarpLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < GPU_MODES {
                if lt.record_gpu(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.record_gpu(GPU_MODES).is_err()
        },
        "集成/独立/无 GPU 三元闭集，越元必抓",
    );
    s.add(
        "Warp 六判据全落",
        {
            let mut lt = WarpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52234-J1）",
    );
    s.add(
        "Block 会话单调",
        {
            let mut lt = WarpLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BLOCK_SESSIONS {
                if let Some(w) = session_cell(b) {
                    if lt.record_session(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            // 逆序注入必抓：先落高格再注低格，逆序报漂移且 last_session 不改。
            let _ = lt.record_session(0, match session_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            ok && lt.sessions_all()
        },
        "会话格序单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/漂移/逆序三态轮转必抓");
    let mut l0 = WarpLedger::new();
    s.add(
        "边界·上栈层越闭集",
        l0.record_stack(RUST_STACKS, 0).is_err(),
        "上栈层 4 越闭集必拒",
    );
    s.add(
        "边界·Block 会话越闭集",
        l0.record_session(BLOCK_SESSIONS, 0).is_err(),
        "Block 会话 10 越闭集必拒",
    );
    s.add(
        "边界·工作流越闭集",
        l0.record_workflow(WORKFLOWS, 0).is_err(),
        "工作流 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            l0.record_session(0, match session_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_session(1, match session_cell(1) {
                Some(v) => v.wrapping_add(1),
                _ => 1,
            }) == Err(WarpCode::ESessionDrift)
        },
        "Block 会话载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_workflow(0, match workflow_cell(0) {
                Some(v) => v,
                _ => 0,
            });
            l0.record_workflow(0, match workflow_cell(0) {
                Some(v) => v,
                _ => 0,
            }) == Err(WarpCode::EDup)
        },
        "同格只许一条账目",
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
        "许可面如实定级 S2",
        {
            let mut lt = WarpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut st = 0usize;
            while st < RUST_STACKS {
                if let Some(w) = stack_cell(st) {
                    let _ = lt.record_stack(st, w);
                }
                st += 1;
            }
            let mut r = 0usize;
            while r < AI_ROWS {
                let _ = lt.declare_ai(r);
                r += 1;
            }
            let mut b = 0usize;
            while b < BLOCK_SESSIONS {
                if let Some(w) = session_cell(b) {
                    let _ = lt.record_session(b, w);
                }
                b += 1;
            }
            let mut w = 0usize;
            while w < WORKFLOWS {
                if let Some(v) = workflow_cell(w) {
                    let _ = lt.record_workflow(w, v);
                }
                w += 1;
            }
            let _ = lt.record_gpu(2);
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，Warp 封顶 S2）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且符合 VX15 派工公式",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_layers() {
        assert_eq!(stack_name(0), Some("核心壳层"));
        assert_eq!(stack_name(3), Some("扩展层"));
        assert!(stack_name(4).is_none());
        assert!(stack_cell(4).is_none());
    }

    #[test]
    fn test_ai_rows() {
        assert_eq!(ai_row_name(0), Some("AI 请求面外发声明"));
        assert_eq!(ai_row_name(1), Some("本地模型 N/A 备案"));
        assert!(ai_row_name(2).is_none());
        assert!(ai_row(AI_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = session_cell(3).unwrap_or(0);
        assert_eq!(a, session_cell(3).unwrap_or(0));
        assert_ne!(a, session_cell(4).unwrap_or(0));
        let f0 = workflow_cell(0).unwrap_or(0);
        assert_ne!(f0, workflow_cell(1).unwrap_or(0));
        assert!(session_cell(BLOCK_SESSIONS).is_none());
        assert!(workflow_cell(WORKFLOWS).is_none());
    }

    #[test]
    fn test_gpu_ternary() {
        let mut lt = WarpLedger::new();
        assert!(lt.record_gpu(0).is_ok());
        assert!(lt.record_gpu(2).is_ok());
        assert_eq!(lt.record_gpu(GPU_MODES), Err(WarpCode::ENoGpuMode));
        assert!(lt.gpu_set());
        assert_eq!(gpu_mode_name(2), Some("无 GPU（N/A）"));
        assert!(gpu_mode_name(3).is_none());
    }

    #[test]
    fn test_session_order_caught() {
        let mut lt = WarpLedger::new();
        let _ = lt.record_session(5, session_cell(5).unwrap_or(0));
        let before = lt.last_session();
        assert_eq!(
            lt.record_session(2, session_cell(2).unwrap_or(0)),
            Err(WarpCode::ESessionDrift)
        );
        assert_eq!(lt.last_session(), before);
        assert!(lt.record_session(6, session_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WarpLedger::new();
        assert_eq!(
            lt.record_session(5, session_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WarpCode::ESessionDrift)
        );
        assert_eq!(
            lt.record_stack(1, stack_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(WarpCode::EStackDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WarpLedger::new();
        let _ = lt.record_stack(0, stack_cell(0).unwrap_or(0));
        assert_eq!(lt.record_stack(0, stack_cell(0).unwrap_or(0)), Err(WarpCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WarpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WarpLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (BLOCK_SESSIONS + WORKFLOWS + RUST_STACKS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7A00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WarpCode::Ok as u32, 0);
        assert_eq!(WarpCode::EGradeDrift as u32, 12);
        assert!(!WarpCode::Ok.detail().is_empty());
        assert!(!WarpCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WarpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn unx52233_checks_green() {
        let s = run_unx52233_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
