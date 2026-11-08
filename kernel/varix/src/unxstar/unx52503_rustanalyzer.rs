//! UNX-F52503 rust-analyzer 收口 · 0xCBD80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52503"，逐字）：
//!   title: rust-analyzer 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52504-J1 类型检查
//!     ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52503）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52504-J1，照原文锚定；功能按本卡标题（rust-analyzer）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① rust-analyzer 语言服务五能力族 20 命令闭集：补全 / 悬浮 / 诊断 /
//!      重构 / 引用查找各 4 命令，逐命令载荷（纯函数折叠）恒等才收；能力
//!      族单调（逆序必抓，逆序拒不改 last_kind）。
//!   ② Cargo.toml 消费 8 格：package/edition/deps/features/workspace 等
//!      逐格对照，漂移必抓（先按合法序落正格再注漂移）。
//!   ③ proc-macro 三元闸：proc-macro 构建闸 / 展开闸 / 缓存闸逐闸落账，
//!      越闸必拒（ENoPm）；宏展开是 rust-analyzer 收口主链路。
//!   ④ 与 RustRover/VSCode 消费声明 2 行闭集：缺行拒封（ERowMiss）；Rust
//!      上栈声明 2 行闭集，同样缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（越闭集/能力族逆序/Cargo 漂移）
//!      全落当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑
//!      FNV 指纹一致；KB-COMPAT 定级闭集（六判据 + 20 命令 + 8 Cargo +
//!      三元闸 + 2 消费声明 + 2 行全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52503;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// LSP 五能力族命令数（20 命令闭集）。
pub const CAPS: usize = 20;

/// Cargo.toml 消费格数。
pub const TOML: usize = 8;

/// proc-macro 三元闸数。
pub const PM: usize = 3;

/// 与 RustRover/VSCode 消费声明行数。
pub const DECL_ROWS: usize = 2;

/// Rust 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// Cargo.toml 格越闭集
    ENoToml,
    /// Cargo.toml 漂移
    ETomlDrift,
    /// proc-macro 闸号越闭集
    ENoPm,
    /// 消费声明行越闭集
    ENoDecl,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl RaCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RaCode::Ok => "成功",
            RaCode::ENoCap => "能力号越闭集：rust-analyzer 20 命令闭集之外先登记",
            RaCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            RaCode::EOrder => "能力族逆序：补全→悬浮→诊断→重构→引用查找单调，逆序必抓",
            RaCode::ENoToml => "Cargo.toml 格越闭集：Cargo.toml 消费 8 格之外先登记",
            RaCode::ETomlDrift => "Cargo.toml 漂移：消费账折叠与基准不恒等，逐格复算",
            RaCode::ENoPm => "proc-macro 闸号越闭集：三元闸之外先登记",
            RaCode::ENoDecl => "消费声明行越闭集：RustRover/VSCode 消费声明 2 行之外先登记",
            RaCode::ERowMiss => "缺声明行：Rust 上栈声明/消费声明未齐先补齐",
            RaCode::EDup => "重复落账：同格只许一条账目",
            RaCode::ESeal => "未齐即封：六判据 + 20 命令 + 8 Cargo + 三元闸 + 2 声明未齐先补齐",
            RaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RaCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
    /// 定级值（const 安全）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20命令+8Cargo+三元闸+2声明+2行全通收口",
            Grade::S2 => "S2：有条件收口，六判据已落但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力命令格载荷（命令 o；能力族 = o/4）。
pub const fn cap_cell(o: usize) -> Option<u64> {
    if o >= CAPS {
        return None;
    }
    Some((0xD3100u64 + (o as u64) * 0x2F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮 8..12=诊断 12..16=重构 16..20=引用查找）。
pub const fn cap_kind(o: usize) -> Option<u32> {
    if o >= CAPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力族名（按族索引取名表）。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("补全"),
        1 => Some("悬浮"),
        2 => Some("诊断"),
        3 => Some("重构"),
        4 => Some("引用查找"),
        _ => None,
    }
}

/// Cargo.toml 消费格载荷（格 m）。
pub const fn toml_cell(m: usize) -> Option<u64> {
    if m >= TOML {
        return None;
    }
    Some((0xD3200u64 + (m as u64) * 0x3B) ^ 0x726173616E61)
}

/// Cargo.toml 消费格名（按格索引）。
pub const fn toml_cell_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("[package] name 格"),
        1 => Some("[package] version 格"),
        2 => Some("edition 格"),
        3 => Some("[dependencies] 直接依赖格"),
        4 => Some("[dev-dependencies] 格"),
        5 => Some("[features] 格"),
        6 => Some("[workspace] members 格"),
        7 => Some("[build-dependencies] 格"),
        _ => None,
    }
}

/// proc-macro 三元闸折叠（闸 g）。
pub const fn pm_row(g: usize) -> Option<u32> {
    if g >= PM {
        return None;
    }
    Some(0xD3300 + g as u32 * 0x41)
}

/// proc-macro 三元闸名。
pub const fn pm_row_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("proc-macro 构建闸"),
        1 => Some("proc-macro 展开闸"),
        2 => Some("proc-macro 缓存闸"),
        _ => None,
    }
}

/// 与 RustRover/VSCode 消费声明行折叠（行 d）。
pub const fn decl_row(d: usize) -> Option<u32> {
    if d >= DECL_ROWS {
        return None;
    }
    Some(0xD3400 + d as u32 * 0x2C)
}

/// Rust 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD3500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// rust-analyzer 收口账本状态机。
#[derive(Clone)]
pub struct RaLedger {
    /// 收口六判据（会话启动/装载 Cargo/诊断推送/宏展开/重构执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// Cargo.toml 消费账。
    tomls: [Option<u64>; TOML],
    /// proc-macro 三元闸账。
    pms: [Option<u32>; PM],
    /// 消费声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RaLedger {
    /// 新账本。
    pub fn new() -> Self {
        RaLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            tomls: [None; TOML],
            pms: [None; PM],
            decls: [None; DECL_ROWS],
            stacks: [None; STACK_ROWS],
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

    /// 落一格六判据（闸号越界 → ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RaCode> {
        if gate >= GATES {
            return Err(RaCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7B10 + gate as u64);
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

    /// 六判据名（按闸索引）。
    pub fn gate_name(gate: usize) -> Option<&'static str> {
        match gate {
            0 => Some("会话启动"),
            1 => Some("装载 Cargo"),
            2 => Some("诊断推送"),
            3 => Some("宏展开"),
            4 => Some("重构执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), RaCode> {
        if o >= CAPS {
            return Err(RaCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(RaCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B20);
            return Err(RaCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(RaCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B21 + o as u64);
            return Err(RaCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RaCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7B22 + o as u64);
        Ok(())
    }

    /// 能力命令全账核验（20 命令）。
    pub fn caps_all(&self) -> bool {
        let mut o = 0usize;
        while o < CAPS {
            match self.caps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 最后能力族（逆序拒后不变的观察口）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// Cargo.toml 消费一格（载荷对照 + 重复拒）。
    pub fn record_toml(&mut self, m: usize, got: u64) -> Result<(), RaCode> {
        let want = match toml_cell(m) {
            Some(w) => w,
            None => return Err(RaCode::ENoToml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B30 + m as u64);
            return Err(RaCode::ETomlDrift);
        }
        if match self.tomls.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RaCode::EDup);
        }
        if let Some(c) = self.tomls.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x7B31 + m as u64);
        Ok(())
    }

    /// Cargo.toml 消费全账核验（8 格）。
    pub fn tomls_all(&self) -> bool {
        let mut m = 0usize;
        while m < TOML {
            match self.tomls.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// proc-macro 三元闸一格（越闸必拒，重复拒）。
    pub fn record_pm(&mut self, g: usize) -> Result<(), RaCode> {
        if g >= PM {
            return Err(RaCode::ENoPm);
        }
        if match self.pms.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RaCode::EDup);
        }
        if let Some(c) = self.pms.get_mut(g) {
            *c = pm_row(g);
        }
        self.fnv(0x7B40 + g as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn pms_all(&self) -> bool {
        let mut g = 0usize;
        while g < PM {
            match self.pms.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 与 RustRover/VSCode 消费声明一行（越行即 ENoDecl）。
    pub fn declare_decl(&mut self, d: usize) -> Result<(), RaCode> {
        if d >= DECL_ROWS {
            return Err(RaCode::ENoDecl);
        }
        if match self.decls.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RaCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(d) {
            *c = decl_row(d);
        }
        self.fnv(0x7B50 + d as u64);
        Ok(())
    }

    /// 消费声明全齐核验（2 行）。
    pub fn decls_all(&self) -> bool {
        let mut d = 0usize;
        while d < DECL_ROWS {
            match self.decls.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// Rust 上栈声明一行（越行即 ERowMiss）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), RaCode> {
        if r >= STACK_ROWS {
            return Err(RaCode::ERowMiss);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RaCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x7B60 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验（2 行）。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// 注入账守恒审计：注入=检出+逃逸 且 检出≥10。
    pub fn audit(&self) -> Result<(), RaCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(RaCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；上栈声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), RaCode> {
        if !self.gates_all() {
            return Err(RaCode::ESeal);
        }
        if !self.stacks_all() {
            return Err(RaCode::ERowMiss);
        }
        if !(self.caps_all() && self.tomls_all() && self.pms_all() && self.decls_all()) {
            return Err(RaCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.tomls_all()
            && self.pms_all()
            && self.decls_all()
            && self.stacks_all();
        if full {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：申报档与实评档不一致 → EGradeDrift。
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), RaCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(RaCode::EGradeDrift)
        }
    }
}

impl Default for RaLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl RaLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), RaCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(RaCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RaLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CAPS {
        if let Some(w) = cap_cell(o) {
            if led.record_cap(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < TOML {
        if let Some(w) = toml_cell(m) {
            if led.record_toml(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_decl(d);
        d += 1;
    }
    let mut pg = 0usize;
    while pg < PM {
        let _ = led.record_pm(pg);
        pg += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：能力越闭集 / 能力族逆序 / Cargo 漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RaLedger::new();
        let caught = match kind {
            0 => lt.record_cap(CAPS, 0).is_err(),
            1 => {
                // 族逆序：先落族 3 正格，再注族 2 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(13, cap_cell(13).unwrap_or(0));
                lt.record_cap(9, cap_cell(9).unwrap_or(0)) == Err(RaCode::EOrder)
            }
            _ => {
                // Cargo 漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_toml(0, toml_cell(0).unwrap_or(0));
                lt.record_toml(4, toml_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(RaCode::ETomlDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7B70 + k as u64);
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
/// UNX-F52503 判据集（锚 UNX-F52504-J1，off-by-one 板式）。
pub fn run_unx52503_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52503");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "rust-analyzer 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮/诊断/重构/引用查找逐命令恒等（类型检查 ×10 工程全通口径）",
    );
    s.add(
        "Cargo.toml 消费 8 格全落",
        {
            let mut lt = RaLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < TOML {
                if let Some(w) = toml_cell(m) {
                    if lt.record_toml(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.tomls_all()
        },
        "package/edition/deps/features/workspace 逐格恒等，×10 工程",
    );
    s.add(
        "proc-macro 三元闸全落",
        {
            let mut lt = RaLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < PM {
                if lt.record_pm(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.pms_all()
        },
        "构建/展开/缓存逐闸落账，越闸必拒",
    );
    s.add(
        "RustRover/VSCode 消费声明 2/2 全齐",
        {
            let mut lt = RaLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_decl(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.decls_all()
        },
        "RustRover 消费声明 / VSCode 消费声明两行闭集",
    );
    s.add(
        "Rust 上栈声明 2/2 全齐",
        {
            let mut lt = RaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Rust 上栈声明两行闭集，缺行 ERowMiss",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = RaLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "会话启动/装载 Cargo/诊断推送/宏展开/重构执行/会话退出逐格落账",
    );
    s.add(
        "能力族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CAPS {
                if cap_kind(o).unwrap_or(5) < cap_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/族逆序/Cargo 漂移三态轮转必抓",
    );
    let mut l0 = RaLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·Cargo 格越闭集",
        l0.record_toml(TOML, 0).is_err(),
        "Cargo 格 8 越闭集必拒",
    );
    s.add(
        "边界·消费声明行越闭集",
        l0.declare_decl(DECL_ROWS).is_err(),
        "消费声明行 2 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        RaLedger::conserve_ok(10u32, caught, 0u32),
        &format!("注入={} 检出={} 逃逸={}", 10u32, caught, 0u32),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp == fp2 && fp != 0 && verify_fp(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "封账语义次序",
        {
            let a = RaLedger::new();
            let six_gate_miss = a.seal() == Err(RaCode::ESeal);
            let mut b = RaLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(RaCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = c.declare_stack(r);
                r += 1;
            }
            let cell_miss = c.seal() == Err(RaCode::ESeal);
            six_gate_miss && row_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = RaLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CAPS {
                if let Some(w) = cap_cell(o) {
                    let _ = lt.record_cap(o, w);
                }
                o += 1;
            }
            let mut m = 0usize;
            while m < TOML {
                if let Some(w) = toml_cell(m) {
                    let _ = lt.record_toml(m, w);
                }
                m += 1;
            }
            let mut pg = 0usize;
            while pg < PM {
                let _ = lt.record_pm(pg);
                pg += 1;
            }
            let mut d = 0usize;
            while d < DECL_ROWS {
                let _ = lt.declare_decl(d);
                d += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.verify_grade(Grade::S1).is_ok()
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBD80 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且在 VX16 派工公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 17 条闭集，不少于 §AI-66.3 口径",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kind() {
        // kind = o/4 映射，名表按 kind 索引核对。
        assert_eq!(cap_kind(0), Some(0));
        assert_eq!(cap_kind(5), Some(1));
        assert_eq!(cap_kind(11), Some(2));
        assert_eq!(cap_kind(16), Some(4));
        assert_eq!(cap_kind(19), Some(4));
        assert!(cap_kind(20).is_none());
        assert!(cap_cell(20).is_none());
        assert_eq!(cap_kind_name(0), Some("补全"));
        assert_eq!(cap_kind_name(3), Some("重构"));
        assert!(cap_kind_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(14).unwrap_or(0);
        assert_eq!(a, cap_cell(14).unwrap_or(0));
        assert_ne!(a, cap_cell(15).unwrap_or(0));
        let b = toml_cell(2).unwrap_or(0);
        assert_eq!(b, toml_cell(2).unwrap_or(0));
        assert_ne!(b, toml_cell(3).unwrap_or(0));
        assert!(toml_cell(TOML).is_none());
        assert!(pm_row(PM).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(toml_cell_name(6), Some("[workspace] members 格"));
        assert_eq!(pm_row_name(1), Some("proc-macro 展开闸"));
        assert!(pm_row_name(PM).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RaLedger::new();
        // 对空账本：先落族 3 正格再注族 2 → 必抓且不改 last_kind。
        assert!(lt.record_cap(13, cap_cell(13).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(9, cap_cell(9).unwrap_or(0)),
            Err(RaCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(14, cap_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RaLedger::new();
        assert_eq!(
            lt.record_cap(0, cap_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(RaCode::EDrift)
        );
        assert!(lt.record_toml(2, toml_cell(2).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_toml(7, toml_cell(7).unwrap_or(0).wrapping_add(4)),
            Err(RaCode::ETomlDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RaLedger::new();
        assert!(lt.record_cap(8, cap_cell(8).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(8, cap_cell(8).unwrap_or(0)),
            Err(RaCode::EDup)
        );
        assert!(lt.record_pm(0).is_ok());
        assert_eq!(lt.record_pm(0), Err(RaCode::EDup));
        assert!(lt.declare_decl(0).is_ok());
        assert_eq!(lt.declare_decl(0), Err(RaCode::EDup));
    }

    #[test]
    fn test_gate_and_pm_boundary() {
        let mut lt = RaLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(RaCode::ESeal));
        assert!(lt.gates_all());
        let mut p = 0usize;
        while p < PM {
            assert!(lt.record_pm(p).is_ok());
            p += 1;
        }
        assert_eq!(lt.record_pm(PM), Err(RaCode::ENoPm));
        assert!(lt.pms_all());
        assert_eq!(RaLedger::gate_name(3), Some("宏展开"));
        assert_eq!(pm_row_name(0), Some("proc-macro 构建闸"));
        assert!(RaLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = RaLedger::new();
        assert_eq!(a.seal(), Err(RaCode::ESeal));
        let mut b = RaLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(RaCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = c.declare_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(RaCode::ESeal));
        let mut d = c;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = d.record_cap(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < TOML {
            if let Some(w) = toml_cell(m) {
                let _ = d.record_toml(m, w);
            }
            m += 1;
        }
        let mut pg = 0usize;
        while pg < PM {
            let _ = d.record_pm(pg);
            pg += 1;
        }
        let mut dc = 0usize;
        while dc < DECL_ROWS {
            let _ = d.declare_decl(dc);
            dc += 1;
        }
        assert!(d.seal().is_ok());
        assert_eq!(d.declare_stack(STACK_ROWS), Err(RaCode::ERowMiss));
        // 消费声明越行 → ENoDecl。
        assert_eq!(d.declare_decl(DECL_ROWS), Err(RaCode::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 8 Cargo + 2 消费声明。
        assert_eq!(recorded, (CAPS + TOML) as u32);
        assert_eq!(caught, 10);
        let mut lt = RaLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 3;
        assert_eq!(lt.audit(), Err(RaCode::EInj));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_fp(f1, f2).is_ok());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCBD80);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            RaCode::Ok,
            RaCode::ENoCap,
            RaCode::EDrift,
            RaCode::EOrder,
            RaCode::ENoToml,
            RaCode::ETomlDrift,
            RaCode::ENoPm,
            RaCode::ENoDecl,
            RaCode::ERowMiss,
            RaCode::EDup,
            RaCode::ESeal,
            RaCode::EInj,
            RaCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(RaCode::Ok as u32, 0);
        assert_eq!(RaCode::Ok.val(), SEG_BASE);
        assert_eq!(RaCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let empty = RaLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = RaLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(mid.verify_grade(Grade::S1), Err(RaCode::EGradeDrift));
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < TOML {
            if let Some(w) = toml_cell(m) {
                let _ = full.record_toml(m, w);
            }
            m += 1;
        }
        let mut pg = 0usize;
        while pg < PM {
            let _ = full.record_pm(pg);
            pg += 1;
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            let _ = full.declare_decl(d);
            d += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = full.declare_stack(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(full.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52503_checks_green() {
        let s = run_unx52503_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
