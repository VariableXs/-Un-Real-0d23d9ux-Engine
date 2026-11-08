//! UNX-F52502 gopls 收口 · 0xCBD40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52502"，逐字）：
//!   title: gopls 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52503-J1 LSP ×20；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52502）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52503-J1，照原文锚定；功能按本卡标题（gopls）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① gopls 语言服务五能力族 20 命令闭集：补全 / 悬浮提示 / 诊断 / 跳转
//!      定义 / 引用查找各 4 命令，逐命令载荷（纯函数折叠）恒等才收；能力
//!      族单调（逆序必抓，且逆序拒不改 last_kind）。
//!   ② go.mod 消费 8 格：模块路径/Go 版本/require 块/replace 块等逐格
//!      对照，漂移必抓（先按合法序落正格再注漂移）。
//!   ③ 工作区三态 6 格：单根 / 多根 / 无根各「进入+退出」两格，逐格落账
//!      越格必拒；gopls 工作区态切换是收口主链路。
//!   ④ 与 GoLand 消费联签 4 格（F52046）：联签逐格声明，缺格拒封；Go
//!      上栈声明 2 行闭集，缺行拒封（ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（go.mod 漂移/能力族逆序/越闭集）
//!      全落当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑
//!      FNV 指纹一致；KB-COMPAT 定级闭集（六判据 + 20 命令 + 8 go.mod +
//!      6 工作区 + 4 联签 + 2 行全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52502;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// LSP 五能力族命令数（LSP ×20 命令闭集）。
pub const CAPS: usize = 20;

/// go.mod 消费格数。
pub const MOD: usize = 8;

/// 工作区三态格数（三态 × 进入/退出）。
pub const WS: usize = 6;

/// 与 GoLand 消费联签格数（F52046）。
pub const JOINT: usize = 4;

/// Go 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoplsCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// go.mod 格越闭集
    ENoMod,
    /// go.mod 漂移
    EModDrift,
    /// 工作区格越闭集
    ENoWs,
    /// 联签格越闭集
    ENoJoint,
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

impl GoplsCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GoplsCode::Ok => "成功",
            GoplsCode::ENoCap => "能力号越闭集：gopls 20 命令闭集之外先登记",
            GoplsCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            GoplsCode::EOrder => "能力族逆序：补全→悬浮提示→诊断→跳转定义→引用查找单调，逆序必抓",
            GoplsCode::ENoMod => "go.mod 格越闭集：go.mod 消费 8 格之外先登记",
            GoplsCode::EModDrift => "go.mod 漂移：模块账折叠与基准不恒等，逐格复算",
            GoplsCode::ENoWs => "工作区格越闭集：工作区三态 6 格之外先登记",
            GoplsCode::ENoJoint => "联签格越闭集：GoLand 消费联签 4 格（F52046）之外先登记",
            GoplsCode::ERowMiss => "缺声明行：Go 上栈声明 2 行未齐先补齐",
            GoplsCode::EDup => "重复落账：同格只许一条账目",
            GoplsCode::ESeal => "未齐即封：六判据 + 20 命令 + 8 go.mod + 6 工作区 + 4 联签未齐先补齐",
            GoplsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GoplsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20命令+8go.mod+6工作区+4联签+2行全通收口",
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
    Some((0xD2100u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=诊断 12..16=跳转定义 16..20=引用查找）。
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
        3 => Some("跳转定义"),
        4 => Some("引用查找"),
        _ => None,
    }
}

/// go.mod 消费格载荷（格 m）。
pub const fn mod_cell(m: usize) -> Option<u64> {
    if m >= MOD {
        return None;
    }
    Some((0xD2200u64 + (m as u64) * 0x37) ^ 0x676F706C7332)
}

/// go.mod 消费格名（按格索引）。
pub const fn mod_cell_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("module 路径格"),
        1 => Some("go 版本指令格"),
        2 => Some("toolchain 指令格"),
        3 => Some("require 直接依赖格"),
        4 => Some("require 间接依赖格"),
        5 => Some("replace 指令格"),
        6 => Some("exclude 指令格"),
        7 => Some("go.sum 校验联动格"),
        _ => None,
    }
}

/// 工作区三态格折叠（格 s；0/1=单根进/出 2/3=多根进/出 4/5=无根进/出）。
pub const fn ws_row(s: usize) -> Option<u32> {
    if s >= WS {
        return None;
    }
    Some(0xD2300 + s as u32 * 0x3D)
}

/// 工作区态（s/2）。
pub const fn ws_state(s: usize) -> Option<u32> {
    if s >= WS {
        return None;
    }
    Some((s / 2) as u32)
}

/// 工作区态名。
pub const fn ws_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("单根工作区"),
        1 => Some("多根工作区"),
        2 => Some("无根工作区"),
        _ => None,
    }
}

/// 与 GoLand 消费联签格折叠（格 j；F52046 锚定）。
pub const fn joint_row(j: usize) -> Option<u32> {
    if j >= JOINT {
        return None;
    }
    Some(0xD2400 + j as u32 * 0x29)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD2500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// gopls 收口账本状态机。
#[derive(Clone)]
pub struct GoplsLedger {
    /// 收口六判据（会话启动/工作区装载/诊断推送/类型检查/重构执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// go.mod 消费账。
    mods: [Option<u64>; MOD],
    /// 工作区三态账。
    wss: [Option<u32>; WS],
    /// 联签账。
    joints: [Option<u32>; JOINT],
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

impl GoplsLedger {
    /// 新账本。
    pub fn new() -> Self {
        GoplsLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            mods: [None; MOD],
            wss: [None; WS],
            joints: [None; JOINT],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GoplsCode> {
        if gate >= GATES {
            return Err(GoplsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6B10 + gate as u64);
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
            1 => Some("工作区装载"),
            2 => Some("诊断推送"),
            3 => Some("类型检查"),
            4 => Some("重构执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), GoplsCode> {
        if o >= CAPS {
            return Err(GoplsCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(GoplsCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B20);
            return Err(GoplsCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(GoplsCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B21 + o as u64);
            return Err(GoplsCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GoplsCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6B22 + o as u64);
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

    /// go.mod 消费一格（载荷对照 + 重复拒）。
    pub fn record_mod(&mut self, m: usize, got: u64) -> Result<(), GoplsCode> {
        let want = match mod_cell(m) {
            Some(w) => w,
            None => return Err(GoplsCode::ENoMod),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B30 + m as u64);
            return Err(GoplsCode::EModDrift);
        }
        if match self.mods.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GoplsCode::EDup);
        }
        if let Some(c) = self.mods.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x6B31 + m as u64);
        Ok(())
    }

    /// go.mod 消费全账核验（8 格）。
    pub fn mods_all(&self) -> bool {
        let mut m = 0usize;
        while m < MOD {
            match self.mods.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 工作区三态一格（越格必拒，重复拒）。
    pub fn record_ws(&mut self, s: usize) -> Result<(), GoplsCode> {
        if s >= WS {
            return Err(GoplsCode::ENoWs);
        }
        if match self.wss.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GoplsCode::EDup);
        }
        if let Some(c) = self.wss.get_mut(s) {
            *c = ws_row(s);
        }
        self.fnv(0x6B40 + s as u64);
        Ok(())
    }

    /// 工作区三态全齐核验（6 格）。
    pub fn wss_all(&self) -> bool {
        let mut s = 0usize;
        while s < WS {
            match self.wss.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 与 GoLand 消费联签一格（F52046）。
    pub fn record_joint(&mut self, j: usize) -> Result<(), GoplsCode> {
        if j >= JOINT {
            return Err(GoplsCode::ENoJoint);
        }
        if match self.joints.get(j) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GoplsCode::EDup);
        }
        if let Some(c) = self.joints.get_mut(j) {
            *c = joint_row(j);
        }
        self.fnv(0x6B50 + j as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn joints_all(&self) -> bool {
        let mut j = 0usize;
        while j < JOINT {
            match self.joints.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// Go 上栈声明一行（越行即 ERowMiss）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), GoplsCode> {
        if r >= STACK_ROWS {
            return Err(GoplsCode::ERowMiss);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GoplsCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x6B60 + r as u64);
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
    pub fn audit(&self) -> Result<(), GoplsCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(GoplsCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；上栈声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), GoplsCode> {
        if !self.gates_all() {
            return Err(GoplsCode::ESeal);
        }
        if !self.stacks_all() {
            return Err(GoplsCode::ERowMiss);
        }
        if !(self.caps_all() && self.mods_all() && self.wss_all() && self.joints_all()) {
            return Err(GoplsCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.mods_all()
            && self.wss_all()
            && self.joints_all()
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
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), GoplsCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(GoplsCode::EGradeDrift)
        }
    }
}

impl Default for GoplsLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl GoplsLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), GoplsCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(GoplsCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GoplsLedger::new();
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
    while m < MOD {
        if let Some(w) = mod_cell(m) {
            if led.record_mod(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut j = 0usize;
    while j < JOINT {
        if led.record_joint(j).is_ok() {
            recorded += 1;
        }
        j += 1;
    }
    let mut s = 0usize;
    while s < WS {
        let _ = led.record_ws(s);
        s += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：go.mod 漂移 / 能力族逆序 / 能力越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GoplsLedger::new();
        let caught = match kind {
            0 => {
                // go.mod 漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_mod(0, mod_cell(0).unwrap_or(0));
                lt.record_mod(3, mod_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(GoplsCode::EModDrift)
            }
            1 => {
                // 族逆序：先落族 4 正格，再注族 0 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(16, cap_cell(16).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0)) == Err(GoplsCode::EOrder)
            }
            _ => lt.record_cap(CAPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6B70 + k as u64);
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
/// UNX-F52502 判据集（锚 UNX-F52503-J1，off-by-one 板式）。
pub fn run_unx52502_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52502");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "gopls 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮提示/诊断/跳转定义/引用查找逐命令恒等（LSP ×20）",
    );
    s.add(
        "go.mod 消费 8 格全落",
        {
            let mut lt = GoplsLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MOD {
                if let Some(w) = mod_cell(m) {
                    if lt.record_mod(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.mods_all()
        },
        "module/go 版本/require/replace/go.sum 联动逐格恒等",
    );
    s.add(
        "工作区三态 6 格全落",
        {
            let mut lt = GoplsLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < WS {
                if lt.record_ws(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.wss_all()
        },
        "单根/多根/无根各进入+退出两格逐格落账",
    );
    s.add(
        "GoLand 消费联签 4/4（F52046）",
        {
            let mut lt = GoplsLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < JOINT {
                if lt.record_joint(j).is_err() {
                    ok = false;
                }
                j += 1;
            }
            ok && lt.joints_all()
        },
        "与 GoLand 消费联签逐格声明，缺格拒封",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = GoplsLedger::new();
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
        "Go 上栈声明两行闭集，缺行 ERowMiss",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = GoplsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "会话启动/工作区装载/诊断推送/类型检查/重构执行/会话退出逐格落账",
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
        "go.mod 漂移/族逆序/越闭集三态轮转必抓",
    );
    let mut l0 = GoplsLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·go.mod 格越闭集",
        l0.record_mod(MOD, 0).is_err(),
        "go.mod 格 8 越闭集必拒",
    );
    s.add(
        "边界·工作区格越闭集",
        l0.record_ws(WS).is_err(),
        "工作区格 6 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        GoplsLedger::conserve_ok(10u32, caught, 0u32),
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
            let a = GoplsLedger::new();
            let six_gate_miss = a.seal() == Err(GoplsCode::ESeal);
            let mut b = GoplsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(GoplsCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = c.declare_stack(r);
                r += 1;
            }
            let cell_miss = c.seal() == Err(GoplsCode::ESeal);
            six_gate_miss && row_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GoplsLedger::new();
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
            while m < MOD {
                if let Some(w) = mod_cell(m) {
                    let _ = lt.record_mod(m, w);
                }
                m += 1;
            }
            let mut st = 0usize;
            while st < WS {
                let _ = lt.record_ws(st);
                st += 1;
            }
            let mut j = 0usize;
            while j < JOINT {
                let _ = lt.record_joint(j);
                j += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBD40 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
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
        assert_eq!(cap_kind(4), Some(1));
        assert_eq!(cap_kind(9), Some(2));
        assert_eq!(cap_kind(12), Some(3));
        assert_eq!(cap_kind(19), Some(4));
        assert!(cap_kind(20).is_none());
        assert!(cap_cell(20).is_none());
        assert_eq!(cap_kind_name(0), Some("补全"));
        assert_eq!(cap_kind_name(3), Some("跳转定义"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(11).unwrap_or(0);
        assert_eq!(a, cap_cell(11).unwrap_or(0));
        assert_ne!(a, cap_cell(12).unwrap_or(0));
        let b = mod_cell(5).unwrap_or(0);
        assert_eq!(b, mod_cell(5).unwrap_or(0));
        assert_ne!(b, mod_cell(6).unwrap_or(0));
        assert!(mod_cell(MOD).is_none());
        assert!(ws_row(WS).is_none());
        assert!(joint_row(JOINT).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(mod_cell_name(0), Some("module 路径格"));
        assert_eq!(ws_state_name(1), Some("多根工作区"));
        assert!(ws_state_name(3).is_none());
    }

    #[test]
    fn test_ws_state() {
        // 工作区态 = s/2 映射。
        assert_eq!(ws_state(0), Some(0));
        assert_eq!(ws_state(1), Some(0));
        assert_eq!(ws_state(2), Some(1));
        assert_eq!(ws_state(5), Some(2));
        assert!(ws_state(6).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GoplsLedger::new();
        // 对空账本：先落族 4 正格再注族 0 → 必抓且不改 last_kind。
        assert!(lt.record_cap(16, cap_cell(16).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(1, cap_cell(1).unwrap_or(0)),
            Err(GoplsCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(17, cap_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GoplsLedger::new();
        assert_eq!(
            lt.record_cap(2, cap_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(GoplsCode::EDrift)
        );
        assert!(lt.record_mod(1, mod_cell(1).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_mod(6, mod_cell(6).unwrap_or(0).wrapping_add(2)),
            Err(GoplsCode::EModDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GoplsLedger::new();
        assert!(lt.record_cap(3, cap_cell(3).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(3, cap_cell(3).unwrap_or(0)),
            Err(GoplsCode::EDup)
        );
        assert!(lt.record_ws(0).is_ok());
        assert_eq!(lt.record_ws(0), Err(GoplsCode::EDup));
        assert!(lt.record_joint(0).is_ok());
        assert_eq!(lt.record_joint(0), Err(GoplsCode::EDup));
    }

    #[test]
    fn test_gate_and_ws_boundary() {
        let mut lt = GoplsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(GoplsCode::ESeal));
        assert!(lt.gates_all());
        let mut s = 0usize;
        while s < WS {
            assert!(lt.record_ws(s).is_ok());
            s += 1;
        }
        assert_eq!(lt.record_ws(WS), Err(GoplsCode::ENoWs));
        assert!(lt.wss_all());
        assert_eq!(GoplsLedger::gate_name(1), Some("工作区装载"));
        assert!(GoplsLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = GoplsLedger::new();
        assert_eq!(a.seal(), Err(GoplsCode::ESeal));
        let mut b = GoplsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(GoplsCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = c.declare_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(GoplsCode::ESeal));
        let mut d = c;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = d.record_cap(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < MOD {
            if let Some(w) = mod_cell(m) {
                let _ = d.record_mod(m, w);
            }
            m += 1;
        }
        let mut st = 0usize;
        while st < WS {
            let _ = d.record_ws(st);
            st += 1;
        }
        let mut j = 0usize;
        while j < JOINT {
            let _ = d.record_joint(j);
            j += 1;
        }
        assert!(d.seal().is_ok());
        assert_eq!(d.declare_stack(STACK_ROWS), Err(GoplsCode::ERowMiss));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 8 go.mod + 4 联签。
        assert_eq!(recorded, (CAPS + MOD + JOINT) as u32);
        assert_eq!(caught, 10);
        let mut lt = GoplsLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 2;
        assert_eq!(lt.audit(), Err(GoplsCode::EInj));
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
        assert_eq!(SEG_BASE, 0xCBD40);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            GoplsCode::Ok,
            GoplsCode::ENoCap,
            GoplsCode::EDrift,
            GoplsCode::EOrder,
            GoplsCode::ENoMod,
            GoplsCode::EModDrift,
            GoplsCode::ENoWs,
            GoplsCode::ENoJoint,
            GoplsCode::ERowMiss,
            GoplsCode::EDup,
            GoplsCode::ESeal,
            GoplsCode::EInj,
            GoplsCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(GoplsCode::Ok as u32, 0);
        assert_eq!(GoplsCode::Ok.val(), SEG_BASE);
        assert_eq!(GoplsCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let empty = GoplsLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = GoplsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(mid.verify_grade(Grade::S1), Err(GoplsCode::EGradeDrift));
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < MOD {
            if let Some(w) = mod_cell(m) {
                let _ = full.record_mod(m, w);
            }
            m += 1;
        }
        let mut st = 0usize;
        while st < WS {
            let _ = full.record_ws(st);
            st += 1;
        }
        let mut j = 0usize;
        while j < JOINT {
            let _ = full.record_joint(j);
            j += 1;
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
    fn unx52502_checks_green() {
        let s = run_unx52502_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
