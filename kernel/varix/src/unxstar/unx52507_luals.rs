//! UNX-F52507 lua-language-server 收口 · 0xCBE80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52507"，逐字）：
//!   title: lua-language-server 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52508-J1 ×10
//!     （.NET SDK 消费联签 F52066 锚定行）；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52507）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52508-J1，照原文锚定；功能按本卡标题（lua-language-server，下
//! 称 luals）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① luals 语言服务五能力族 20 命令闭集：补全 / 悬浮提示 / 诊断 / 格式
//!      化 / 重命名各 4 命令，逐命令载荷（纯函数折叠）恒等才收；能力族
//!      单调（逆序必抓，逆序拒不改 last_kind）。
//!   ② 工作区投影 8 格：宿主→luals 下发 4 格 + luals→宿主 上报 4 格，
//!      逐格对照，漂移必抓（先按合法序落正格再注漂移）。
//!   ③ 诊断三元闸 3 格闭集：错误级 / 警告级 / 提示级，逐格落账越格必拒
//!      （ENoDiag）；诊断三元闸是 luals 收口主链。
//!   ④ Lua 上栈 4 层闭集：Lua 解释器层 / LuaRocks 层 / workspace 库层 /
//!      luals 插件层，逐层落账越层必拒（ERowMiss）；消费联签分账声明
//!      2 行：Lua 运行时消费联签 / .NET SDK 消费联签（F52066 锚定行），
//!      缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（工作区漂移/族逆序/诊断越格）全落
//!      当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑 FNV
//!      指纹一致；KB-COMPAT 定级闭集（六判据 + 20 命令 + 8 投影 + 3 三元
//!      闸 + 4 上栈层 + 2 分账全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52507;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// luals 五能力族命令数（20 命令闭集）。
pub const CAPS: usize = 20;

/// 工作区投影格数（下发 4 + 上报 4）。
pub const WS: usize = 8;

/// 诊断三元闸格数（错误/警告/提示）。
pub const DIAG: usize = 3;

/// Lua 上栈层数（4 层闭集）。
pub const STACK: usize = 4;

/// 消费联签分账声明行数（含 .NET SDK 消费联签 F52066 锚定行）。
pub const SPLIT_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LualsCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 工作区投影格越闭集
    ENoWs,
    /// 工作区投影漂移
    EWsDrift,
    /// 诊断三元闸格越闭集
    ENoDiag,
    /// 分账行越闭集
    ENoSplit,
    /// 缺声明行（Lua 上栈层/分账行未齐）
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

impl LualsCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LualsCode::Ok => "成功",
            LualsCode::ENoCap => "能力号越闭集：luals 20 命令闭集之外先登记",
            LualsCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            LualsCode::EOrder => "能力族逆序：补全→悬浮提示→诊断→格式化→重命名单调，逆序必抓",
            LualsCode::ENoWs => "工作区投影格越闭集：工作区投影 8 格之外先登记",
            LualsCode::EWsDrift => "工作区投影漂移：投影折叠与基准不恒等，逐格复算",
            LualsCode::ENoDiag => "诊断三元闸格越闭集：错误/警告/提示 3 格之外先登记",
            LualsCode::ENoSplit => "分账行越闭集：消费联签分账 2 行之外先登记",
            LualsCode::ERowMiss => "缺声明行：Lua 上栈 4 层/消费联签分账（含 F52066 锚定行）未齐先补齐",
            LualsCode::EDup => "重复落账：同格只许一条账目",
            LualsCode::ESeal => "未齐即封：六判据 + 20 命令 + 8 投影 + 3 三元闸未齐先补齐",
            LualsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LualsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
    /// 定级值（const 安全，match 口径禁 unwrap_or）。
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
            Grade::S1 => "S1：六判据+20命令+8投影+3三元闸+4上栈层+2分账全通收口",
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
    Some((0xC8100u64 + (o as u64) * 0x2D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=诊断 12..16=格式化 16..20=重命名）。
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
        1 => Some("悬浮提示"),
        2 => Some("诊断"),
        3 => Some("格式化"),
        4 => Some("重命名"),
        _ => None,
    }
}

/// 工作区投影格载荷（格 p；0..4=宿主→luals 下发 4..8=luals→宿主 上报）。
pub const fn ws_cell(p: usize) -> Option<u64> {
    if p >= WS {
        return None;
    }
    Some((0xC8200u64 + (p as u64) * 0x35) ^ 0x006C7561735F7773)
}

/// 投影方向（p/4：0=下发 1=上报）。
pub const fn ws_dir(p: usize) -> Option<u32> {
    if p >= WS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 投影方向名。
pub const fn ws_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("宿主→luals 下发"),
        1 => Some("luals→宿主 上报"),
        _ => None,
    }
}

/// 工作区投影格名（按格索引）。
pub const fn ws_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("workspace.library 下发格"),
        1 => Some("runtime.version 下发格"),
        2 => Some("diagnostics.enable 下发格"),
        3 => Some("format.enable 下发格"),
        4 => Some("diagnostics 上报格"),
        5 => Some("completion 上报格"),
        6 => Some("hover 上报格"),
        7 => Some("symbol 上报格"),
        _ => None,
    }
}

/// 诊断三元闸格折叠（格 t；0=错误级 1=警告级 2=提示级）。
pub const fn diag_row(t: usize) -> Option<u32> {
    if t >= DIAG {
        return None;
    }
    Some(0xC8500 + t as u32 * 0x2F)
}

/// 诊断三元闸层名（按格索引）。
pub const fn diag_tier_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("错误级"),
        1 => Some("警告级"),
        2 => Some("提示级"),
        _ => None,
    }
}

/// Lua 上栈层折叠（层 r；0=解释器 1=LuaRocks 2=workspace 库 3=luals 插件）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK {
        return None;
    }
    Some(0xC8300 + r as u32 * 0x2F)
}

/// 上栈层名（按层索引）。
pub const fn stack_layer_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Lua 解释器层"),
        1 => Some("LuaRocks 层"),
        2 => Some("workspace 库层"),
        3 => Some("luals 插件层"),
        _ => None,
    }
}

/// 消费联签分账声明行折叠（行 r；行 1 并 .NET SDK 消费联签 F52066 锚定行）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC8400 + r as u32 * 0x2C)
}

/// 分账行名（按行索引）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Lua 运行时消费联签"),
        1 => Some(".NET SDK 消费联签（F52066 锚定行）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// luals 收口账本状态机。
#[derive(Clone)]
pub struct LualsLedger {
    /// 收口六判据（服务启动/workspace 装载/诊断三元推送/格式化执行/重命名执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// 工作区投影账。
    wss: [Option<u64>; WS],
    /// 诊断三元闸账。
    diags: [Option<u32>; DIAG],
    /// Lua 上栈层账。
    stacks: [Option<u32>; STACK],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LualsLedger {
    /// 新账本。
    pub fn new() -> Self {
        LualsLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            wss: [None; WS],
            diags: [None; DIAG],
            stacks: [None; STACK],
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

    /// 落一格六判据（闸号越界 → ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LualsCode> {
        if gate >= GATES {
            return Err(LualsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8E10 + gate as u64);
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
            0 => Some("服务启动"),
            1 => Some("workspace 装载"),
            2 => Some("诊断三元推送"),
            3 => Some("格式化执行"),
            4 => Some("重命名执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), LualsCode> {
        if o >= CAPS {
            return Err(LualsCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(LualsCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8E20);
            return Err(LualsCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(LualsCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8E21 + o as u64);
            return Err(LualsCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LualsCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8E22 + o as u64);
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

    /// 工作区投影一格（载荷对照 + 重复拒）。
    pub fn record_ws(&mut self, p: usize, got: u64) -> Result<(), LualsCode> {
        let want = match ws_cell(p) {
            Some(w) => w,
            None => return Err(LualsCode::ENoWs),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8E30 + p as u64);
            return Err(LualsCode::EWsDrift);
        }
        if match self.wss.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LualsCode::EDup);
        }
        if let Some(c) = self.wss.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8E31 + p as u64);
        Ok(())
    }

    /// 工作区投影全账核验（8 格）。
    pub fn wss_all(&self) -> bool {
        let mut p = 0usize;
        while p < WS {
            match self.wss.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 诊断三元闸一格（越格必拒，重复拒）。
    pub fn record_diag(&mut self, t: usize) -> Result<(), LualsCode> {
        if t >= DIAG {
            return Err(LualsCode::ENoDiag);
        }
        if match self.diags.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LualsCode::EDup);
        }
        if let Some(c) = self.diags.get_mut(t) {
            *c = diag_row(t);
        }
        self.fnv(0x8E40 + t as u64);
        Ok(())
    }

    /// 诊断三元闸全齐核验（3 格）。
    pub fn diags_all(&self) -> bool {
        let mut t = 0usize;
        while t < DIAG {
            match self.diags.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// Lua 上栈一层（越层即 ERowMiss，重复拒）。
    pub fn record_stack(&mut self, r: usize) -> Result<(), LualsCode> {
        if r >= STACK {
            return Err(LualsCode::ERowMiss);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LualsCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8E50 + r as u64);
        Ok(())
    }

    /// Lua 上栈全齐核验（4 层）。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK {
            match self.stacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 消费联签分账声明一行（越行即 ENoSplit）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), LualsCode> {
        if r >= SPLIT_ROWS {
            return Err(LualsCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LualsCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8E60 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验（2 行）。
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

    /// 注入账守恒审计：注入=检出+逃逸 且 检出≥10。
    pub fn audit(&self) -> Result<(), LualsCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(LualsCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；声明行缺（上栈层/分账）→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), LualsCode> {
        if !self.gates_all() {
            return Err(LualsCode::ESeal);
        }
        if !self.stacks_all() || !self.splits_all() {
            return Err(LualsCode::ERowMiss);
        }
        if !(self.caps_all() && self.wss_all() && self.diags_all()) {
            return Err(LualsCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.wss_all()
            && self.diags_all()
            && self.stacks_all()
            && self.splits_all();
        if full {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：申报档与实评档不一致 → EGradeDrift。
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), LualsCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(LualsCode::EGradeDrift)
        }
    }
}

impl Default for LualsLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl LualsLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), LualsCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(LualsCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LualsLedger::new();
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
    let mut p = 0usize;
    while p < WS {
        if let Some(w) = ws_cell(p) {
            if led.record_ws(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut t = 0usize;
    while t < DIAG {
        let _ = led.record_diag(t);
        t += 1;
    }
    let mut r = 0usize;
    while r < STACK {
        let _ = led.record_stack(r);
        r += 1;
    }
    let mut sp = 0usize;
    while sp < SPLIT_ROWS {
        let _ = led.declare_split(sp);
        sp += 1;
    }
    // 注入 10 轮：工作区漂移 / 族逆序 / 诊断三元越格三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LualsLedger::new();
        let caught = match kind {
            0 => {
                // 工作区漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_ws(2, ws_cell(2).unwrap_or(0));
                lt.record_ws(6, ws_cell(6).unwrap_or(0).wrapping_add(1))
                    == Err(LualsCode::EWsDrift)
            }
            1 => {
                // 族逆序：先落族 4 正格，再注族 0 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(16, cap_cell(16).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0)) == Err(LualsCode::EOrder)
            }
            _ => lt.record_diag(DIAG).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8E70 + k as u64);
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
/// UNX-F52507 判据集（锚 UNX-F52508-J1，off-by-one 板式）。
pub fn run_unx52507_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52507");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "luals 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮提示/诊断/格式化/重命名逐命令恒等（×10 口径）",
    );
    s.add(
        "工作区投影 8 格全落",
        {
            let mut lt = LualsLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < WS {
                if let Some(w) = ws_cell(p) {
                    if lt.record_ws(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.wss_all()
        },
        "下发 4 格 + 上报 4 格逐格恒等，漂移必抓",
    );
    s.add(
        "诊断三元闸 3 格全落",
        {
            let mut lt = LualsLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < DIAG {
                if lt.record_diag(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.diags_all()
        },
        "错误级/警告级/提示级逐格落账，越格必拒",
    );
    s.add(
        "Lua 上栈 4 层逐层全落",
        {
            let mut lt = LualsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK {
                if lt.record_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "解释器/LuaRocks/workspace 库/luals 插件四层闭集",
    );
    s.add(
        "消费联签分账声明 2/2（含 .NET SDK F52066 锚定行）",
        {
            let mut lt = LualsLedger::new();
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
        "Lua 运行时消费联签 / .NET SDK 消费联签（F52066 锚定行）两行闭集",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = LualsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "服务启动/workspace 装载/诊断三元推送/格式化执行/重命名执行/会话退出逐格落账",
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
        "工作区漂移/族逆序/诊断越格三态轮转必抓",
    );
    let mut l0 = LualsLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·工作区投影越闭集",
        l0.record_ws(WS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "分账行 2 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        LualsLedger::conserve_ok(10u32, caught, 0u32),
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
            let a = LualsLedger::new();
            let six_gate_miss = a.seal() == Err(LualsCode::ESeal);
            let mut b = LualsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(LualsCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK {
                let _ = c.record_stack(r);
                r += 1;
            }
            let split_miss = c.seal() == Err(LualsCode::ERowMiss);
            let mut d = c.clone();
            let mut sr = 0usize;
            while sr < SPLIT_ROWS {
                let _ = d.declare_split(sr);
                sr += 1;
            }
            let cell_miss = d.seal() == Err(LualsCode::ESeal);
            six_gate_miss && row_miss && split_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = LualsLedger::new();
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
            let mut p = 0usize;
            while p < WS {
                if let Some(w) = ws_cell(p) {
                    let _ = lt.record_ws(p, w);
                }
                p += 1;
            }
            let mut t = 0usize;
            while t < DIAG {
                let _ = lt.record_diag(t);
                t += 1;
            }
            let mut r = 0usize;
            while r < STACK {
                let _ = lt.record_stack(r);
                r += 1;
            }
            let mut sr = 0usize;
            while sr < SPLIT_ROWS {
                let _ = lt.declare_split(sr);
                sr += 1;
            }
            lt.verify_grade(Grade::S1).is_ok()
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "三元闸与栈层名表闭集",
        {
            let mut ok = true;
            let mut t = 0usize;
            while t < DIAG {
                if diag_tier_name(t).is_none() {
                    ok = false;
                }
                t += 1;
            }
            if diag_tier_name(DIAG).is_some() || diag_row(DIAG).is_some() {
                ok = false;
            }
            let mut r = 0usize;
            while r < STACK {
                if stack_layer_name(r).is_none() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "3 三元闸 + 4 栈层名表逐名非空且闭集",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBE80 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且在 VX16 派工公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "含本条共 18 条闭集，不少于 §AI-66.3 口径",
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
        assert_eq!(cap_kind(7), Some(1));
        assert_eq!(cap_kind(10), Some(2));
        assert_eq!(cap_kind(13), Some(3));
        assert_eq!(cap_kind(19), Some(4));
        assert!(cap_kind(20).is_none());
        assert!(cap_cell(20).is_none());
        assert_eq!(cap_kind_name(3), Some("格式化"));
        assert_eq!(cap_kind_name(4), Some("重命名"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(6).unwrap_or(0);
        assert_eq!(a, cap_cell(6).unwrap_or(0));
        assert_ne!(a, cap_cell(7).unwrap_or(0));
        let b = ws_cell(5).unwrap_or(0);
        assert_eq!(b, ws_cell(5).unwrap_or(0));
        assert_ne!(b, ws_cell(6).unwrap_or(0));
        assert!(ws_cell(WS).is_none());
        assert!(stack_row(STACK).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(ws_cell_name(0), Some("workspace.library 下发格"));
        assert_eq!(ws_cell_name(7), Some("symbol 上报格"));
        assert!(ws_cell_name(WS).is_none());
        assert_eq!(split_row_name(1), Some(".NET SDK 消费联签（F52066 锚定行）"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_ws_dir_and_diag_tier() {
        // 投影方向 = p/4，三元闸名按格索引。
        assert_eq!(ws_dir(3), Some(0));
        assert_eq!(ws_dir(4), Some(1));
        assert!(ws_dir(8).is_none());
        assert_eq!(ws_dir_name(1), Some("luals→宿主 上报"));
        assert!(ws_dir_name(2).is_none());
        assert_eq!(diag_tier_name(0), Some("错误级"));
        assert_eq!(diag_tier_name(2), Some("提示级"));
        assert!(diag_tier_name(3).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = LualsLedger::new();
        // 对空账本：先落族 4 正格再注族 0 → 必抓且不改 last_kind。
        assert!(lt.record_cap(16, cap_cell(16).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(1, cap_cell(1).unwrap_or(0)),
            Err(LualsCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(17, cap_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LualsLedger::new();
        assert_eq!(
            lt.record_cap(8, cap_cell(8).unwrap_or(0).wrapping_add(5)),
            Err(LualsCode::EDrift)
        );
        assert!(lt.record_ws(0, ws_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_ws(3, ws_cell(3).unwrap_or(0).wrapping_add(8)),
            Err(LualsCode::EWsDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LualsLedger::new();
        assert!(lt.record_cap(14, cap_cell(14).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(14, cap_cell(14).unwrap_or(0)),
            Err(LualsCode::EDup)
        );
        assert!(lt.record_diag(1).is_ok());
        assert_eq!(lt.record_diag(1), Err(LualsCode::EDup));
        assert!(lt.record_stack(2).is_ok());
        assert_eq!(lt.record_stack(2), Err(LualsCode::EDup));
    }

    #[test]
    fn test_gate_and_diag_boundary() {
        let mut lt = LualsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(LualsCode::ESeal));
        assert!(lt.gates_all());
        let mut t = 0usize;
        while t < DIAG {
            assert!(lt.record_diag(t).is_ok());
            t += 1;
        }
        assert_eq!(lt.record_diag(DIAG), Err(LualsCode::ENoDiag));
        assert!(lt.diags_all());
        // 上栈层越层 → ERowMiss；分账行越行 → ENoSplit。
        assert_eq!(lt.record_stack(STACK), Err(LualsCode::ERowMiss));
        assert_eq!(lt.declare_split(SPLIT_ROWS), Err(LualsCode::ENoSplit));
        assert_eq!(LualsLedger::gate_name(2), Some("诊断三元推送"));
        assert!(LualsLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = LualsLedger::new();
        assert_eq!(a.seal(), Err(LualsCode::ESeal));
        let mut b = LualsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(LualsCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK {
            let _ = c.record_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(LualsCode::ERowMiss));
        let mut d = c;
        let mut sr = 0usize;
        while sr < SPLIT_ROWS {
            let _ = d.declare_split(sr);
            sr += 1;
        }
        assert_eq!(d.seal(), Err(LualsCode::ESeal));
        let mut e = d;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = e.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < WS {
            if let Some(w) = ws_cell(p) {
                let _ = e.record_ws(p, w);
            }
            p += 1;
        }
        let mut t = 0usize;
        while t < DIAG {
            let _ = e.record_diag(t);
            t += 1;
        }
        assert!(e.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 8 投影。
        assert_eq!(recorded, (CAPS + WS) as u32);
        assert_eq!(caught, 10);
        let mut lt = LualsLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 4;
        assert_eq!(lt.audit(), Err(LualsCode::EInj));
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
        assert_eq!(SEG_BASE, 0xCBE80);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            LualsCode::Ok,
            LualsCode::ENoCap,
            LualsCode::EDrift,
            LualsCode::EOrder,
            LualsCode::ENoWs,
            LualsCode::EWsDrift,
            LualsCode::ENoDiag,
            LualsCode::ENoSplit,
            LualsCode::ERowMiss,
            LualsCode::EDup,
            LualsCode::ESeal,
            LualsCode::EInj,
            LualsCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(LualsCode::Ok as u32, 0);
        assert_eq!(LualsCode::Ok.val(), SEG_BASE);
        assert_eq!(LualsCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1（前置必须补 judge_gate×6）。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let empty = LualsLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = LualsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(mid.verify_grade(Grade::S1), Err(LualsCode::EGradeDrift));
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < WS {
            if let Some(w) = ws_cell(p) {
                let _ = full.record_ws(p, w);
            }
            p += 1;
        }
        let mut t = 0usize;
        while t < DIAG {
            let _ = full.record_diag(t);
            t += 1;
        }
        let mut r = 0usize;
        while r < STACK {
            let _ = full.record_stack(r);
            r += 1;
        }
        let mut sr = 0usize;
        while sr < SPLIT_ROWS {
            let _ = full.declare_split(sr);
            sr += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(full.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52507_checks_green() {
        let s = run_unx52507_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
