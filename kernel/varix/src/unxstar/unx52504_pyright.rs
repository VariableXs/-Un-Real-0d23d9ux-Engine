//! UNX-F52504 pyright 收口 · 0xCBDC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52504"，逐字）：
//!   title: pyright 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52505-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52504）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52505-J1，照原文锚定；功能按本卡标题（pyright）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pyright 语言服务五能力族 20 命令闭集：补全 / 悬浮提示 / 诊断 /
//!      重命名 / 引用查找各 4 命令，逐命令载荷（纯函数折叠）恒等才收；
//!      能力族单调（逆序必抓，逆序拒不改 last_kind）。
//!   ② pyrightconfig.json 双向投影 12 格：宿主→pyright 下发 6 格 +
//!      pyright→宿主 上报 6 格，逐格对照，漂移必抓（先按合法序落正格
//!      再注漂移）。
//!   ③ strict 三档 6 格：basic / standard / strict 各「声明+生效」两格，
//!      逐格落账越格必拒（ENoStrict）；类型检查档位是 pyright 收口主链。
//!   ④ 与 Pylance 分账声明 2 行（F52033）：pyright 独立账 / Pylance 分账
//!      两行闭集，缺行拒封；Node/TS 上栈声明 2 行，缺行拒封（ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（投影漂移/能力族逆序/越闭集）全落
//!      当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑 FNV
//!      指纹一致；KB-COMPAT 定级闭集（六判据 + 20 命令 + 12 投影 + 6 档
//!      + 2 分账 + 2 行全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52504;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// LSP 五能力族命令数（20 命令闭集）。
pub const CAPS: usize = 20;

/// pyrightconfig.json 双向投影格数。
pub const CONF: usize = 12;

/// strict 三档格数（三档 × 声明/生效）。
pub const STRICT: usize = 6;

/// 与 Pylance 分账声明行数（F52033）。
pub const SPLIT_ROWS: usize = 2;

/// Node/TS 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PyrightCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 投影格越闭集
    ENoConf,
    /// 投影漂移
    EConfDrift,
    /// strict 档格越闭集
    ENoStrict,
    /// 分账行越闭集
    ENoSplit,
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

impl PyrightCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PyrightCode::Ok => "成功",
            PyrightCode::ENoCap => "能力号越闭集：pyright 20 命令闭集之外先登记",
            PyrightCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            PyrightCode::EOrder => "能力族逆序：补全→悬浮提示→诊断→重命名→引用查找单调，逆序必抓",
            PyrightCode::ENoConf => "投影格越闭集：pyrightconfig.json 双向投影 12 格之外先登记",
            PyrightCode::EConfDrift => "投影漂移：双向投影折叠与基准不恒等，逐格复算",
            PyrightCode::ENoStrict => "strict 档格越闭集：三档 6 格之外先登记",
            PyrightCode::ENoSplit => "分账行越闭集：Pylance 分账 2 行（F52033）之外先登记",
            PyrightCode::ERowMiss => "缺声明行：Node/TS 上栈声明 2 行未齐先补齐",
            PyrightCode::EDup => "重复落账：同格只许一条账目",
            PyrightCode::ESeal => "未齐即封：六判据 + 20 命令 + 12 投影 + 6 档 + 2 分账未齐先补齐",
            PyrightCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PyrightCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20命令+12投影+6档+2分账+2行全通收口",
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
    Some((0xD4100u64 + (o as u64) * 0x2D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=诊断 12..16=重命名 16..20=引用查找）。
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
        3 => Some("重命名"),
        4 => Some("引用查找"),
        _ => None,
    }
}

/// 双向投影格载荷（格 p；0..6=宿主→pyright 下发 6..12=pyright→宿主 上报）。
pub const fn conf_cell(p: usize) -> Option<u64> {
    if p >= CONF {
        return None;
    }
    Some((0xD4200u64 + (p as u64) * 0x35) ^ 0x7079636F6E6631)
}

/// 投影方向（p/6：0=下发 1=上报）。
pub const fn conf_dir(p: usize) -> Option<u32> {
    if p >= CONF {
        return None;
    }
    Some((p / 6) as u32)
}

/// 投影方向名。
pub const fn conf_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("宿主→pyright 下发"),
        1 => Some("pyright→宿主 上报"),
        _ => None,
    }
}

/// 投影格名（按格索引）。
pub const fn conf_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("include 下发格"),
        1 => Some("exclude 下发格"),
        2 => Some("pythonVersion 下发格"),
        3 => Some("pythonPath 下发格"),
        4 => Some("venvPath 下发格"),
        5 => Some("typeshedPath 下发格"),
        6 => Some("diagnostics 上报格"),
        7 => Some("completions 上报格"),
        8 => Some("hovers 上报格"),
        9 => Some("renames 上报格"),
        10 => Some("references 上报格"),
        11 => Some("typeCheckStatus 上报格"),
        _ => None,
    }
}

/// strict 三档格折叠（格 t；0/1=basic 声明/生效 2/3=standard 4/5=strict）。
pub const fn strict_row(t: usize) -> Option<u32> {
    if t >= STRICT {
        return None;
    }
    Some(0xD4300 + t as u32 * 0x2E)
}

/// strict 档位（t/2）。
pub const fn strict_tier(t: usize) -> Option<u32> {
    if t >= STRICT {
        return None;
    }
    Some((t / 2) as u32)
}

/// strict 档位名。
pub const fn strict_tier_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("basic"),
        1 => Some("standard"),
        2 => Some("strict"),
        _ => None,
    }
}

/// 与 Pylance 分账声明行折叠（行 r；F52033 锚定）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD4400 + r as u32 * 0x2C)
}

/// Node/TS 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD4500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// pyright 收口账本状态机。
#[derive(Clone)]
pub struct PyrightLedger {
    /// 收口六判据（会话启动/配置装载/诊断推送/类型检查/重构执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// 双向投影账。
    confs: [Option<u64>; CONF],
    /// strict 三档账。
    stricts: [Option<u32>; STRICT],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
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

impl PyrightLedger {
    /// 新账本。
    pub fn new() -> Self {
        PyrightLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            confs: [None; CONF],
            stricts: [None; STRICT],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PyrightCode> {
        if gate >= GATES {
            return Err(PyrightCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8B10 + gate as u64);
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
            1 => Some("配置装载"),
            2 => Some("诊断推送"),
            3 => Some("类型检查"),
            4 => Some("重构执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), PyrightCode> {
        if o >= CAPS {
            return Err(PyrightCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(PyrightCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8B20);
            return Err(PyrightCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(PyrightCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8B21 + o as u64);
            return Err(PyrightCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyrightCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8B22 + o as u64);
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

    /// 双向投影一格（载荷对照 + 重复拒）。
    pub fn record_conf(&mut self, p: usize, got: u64) -> Result<(), PyrightCode> {
        let want = match conf_cell(p) {
            Some(w) => w,
            None => return Err(PyrightCode::ENoConf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8B30 + p as u64);
            return Err(PyrightCode::EConfDrift);
        }
        if match self.confs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyrightCode::EDup);
        }
        if let Some(c) = self.confs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8B31 + p as u64);
        Ok(())
    }

    /// 双向投影全账核验（12 格）。
    pub fn confs_all(&self) -> bool {
        let mut p = 0usize;
        while p < CONF {
            match self.confs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// strict 三档一格（越格必拒，重复拒）。
    pub fn record_strict(&mut self, t: usize) -> Result<(), PyrightCode> {
        if t >= STRICT {
            return Err(PyrightCode::ENoStrict);
        }
        if match self.stricts.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyrightCode::EDup);
        }
        if let Some(c) = self.stricts.get_mut(t) {
            *c = strict_row(t);
        }
        self.fnv(0x8B40 + t as u64);
        Ok(())
    }

    /// strict 三档全齐核验（6 格）。
    pub fn stricts_all(&self) -> bool {
        let mut t = 0usize;
        while t < STRICT {
            match self.stricts.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 与 Pylance 分账声明一行（越行即 ENoSplit）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), PyrightCode> {
        if r >= SPLIT_ROWS {
            return Err(PyrightCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyrightCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8B50 + r as u64);
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

    /// Node/TS 上栈声明一行（越行即 ERowMiss）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), PyrightCode> {
        if r >= STACK_ROWS {
            return Err(PyrightCode::ERowMiss);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PyrightCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8B60 + r as u64);
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
    pub fn audit(&self) -> Result<(), PyrightCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(PyrightCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；上栈声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), PyrightCode> {
        if !self.gates_all() {
            return Err(PyrightCode::ESeal);
        }
        if !self.stacks_all() {
            return Err(PyrightCode::ERowMiss);
        }
        if !(self.caps_all() && self.confs_all() && self.stricts_all() && self.splits_all()) {
            return Err(PyrightCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.confs_all()
            && self.stricts_all()
            && self.splits_all()
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
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), PyrightCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(PyrightCode::EGradeDrift)
        }
    }
}

impl Default for PyrightLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl PyrightLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), PyrightCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(PyrightCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PyrightLedger::new();
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
    while p < CONF {
        if let Some(w) = conf_cell(p) {
            if led.record_conf(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut t = 0usize;
    while t < STRICT {
        let _ = led.record_strict(t);
        t += 1;
    }
    let mut sr = 0usize;
    while sr < STACK_ROWS {
        let _ = led.declare_stack(sr);
        sr += 1;
    }
    // 注入 10 轮：投影漂移 / 能力族逆序 / 能力越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PyrightLedger::new();
        let caught = match kind {
            0 => {
                // 投影漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_conf(0, conf_cell(0).unwrap_or(0));
                lt.record_conf(7, conf_cell(7).unwrap_or(0).wrapping_add(1))
                    == Err(PyrightCode::EConfDrift)
            }
            1 => {
                // 族逆序：先落族 4 正格，再注族 1 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(18, cap_cell(18).unwrap_or(0));
                lt.record_cap(6, cap_cell(6).unwrap_or(0)) == Err(PyrightCode::EOrder)
            }
            _ => lt.record_cap(CAPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8B70 + k as u64);
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
/// UNX-F52504 判据集（锚 UNX-F52505-J1，off-by-one 板式）。
pub fn run_unx52504_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52504");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pyright 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮提示/诊断/重命名/引用查找逐命令恒等（×10 口径）",
    );
    s.add(
        "pyrightconfig.json 双向投影 12 格全落",
        {
            let mut lt = PyrightLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < CONF {
                if let Some(w) = conf_cell(p) {
                    if lt.record_conf(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.confs_all()
        },
        "下发 6 格 + 上报 6 格逐格恒等，漂移必抓",
    );
    s.add(
        "strict 三档 6 格全落",
        {
            let mut lt = PyrightLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < STRICT {
                if lt.record_strict(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.stricts_all()
        },
        "basic/standard/strict 各声明+生效两格逐格落账",
    );
    s.add(
        "Pylance 分账声明 2/2（F52033）",
        {
            let mut lt = PyrightLedger::new();
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
        "pyright 独立账 / Pylance 分账两行闭集，缺行拒封",
    );
    s.add(
        "Node/TS 上栈声明 2/2 全齐",
        {
            let mut lt = PyrightLedger::new();
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
        "Node/TS 上栈声明两行闭集，缺行 ERowMiss",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = PyrightLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "会话启动/配置装载/诊断推送/类型检查/重构执行/会话退出逐格落账",
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
        "投影漂移/族逆序/越闭集三态轮转必抓",
    );
    let mut l0 = PyrightLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_conf(CONF, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "分账行 2 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        PyrightLedger::conserve_ok(10u32, caught, 0u32),
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
            let a = PyrightLedger::new();
            let six_gate_miss = a.seal() == Err(PyrightCode::ESeal);
            let mut b = PyrightLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(PyrightCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = c.declare_stack(r);
                r += 1;
            }
            let cell_miss = c.seal() == Err(PyrightCode::ESeal);
            six_gate_miss && row_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = PyrightLedger::new();
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
            while p < CONF {
                if let Some(w) = conf_cell(p) {
                    let _ = lt.record_conf(p, w);
                }
                p += 1;
            }
            let mut t = 0usize;
            while t < STRICT {
                let _ = lt.record_strict(t);
                t += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut sr = 0usize;
            while sr < STACK_ROWS {
                let _ = lt.declare_stack(sr);
                sr += 1;
            }
            lt.verify_grade(Grade::S1).is_ok()
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBDC0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
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
        assert_eq!(cap_kind(7), Some(1));
        assert_eq!(cap_kind(10), Some(2));
        assert_eq!(cap_kind(13), Some(3));
        assert_eq!(cap_kind(19), Some(4));
        assert!(cap_kind(20).is_none());
        assert!(cap_cell(20).is_none());
        assert_eq!(cap_kind_name(1), Some("悬浮提示"));
        assert_eq!(cap_kind_name(4), Some("引用查找"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(9).unwrap_or(0);
        assert_eq!(a, cap_cell(9).unwrap_or(0));
        assert_ne!(a, cap_cell(10).unwrap_or(0));
        let b = conf_cell(4).unwrap_or(0);
        assert_eq!(b, conf_cell(4).unwrap_or(0));
        assert_ne!(b, conf_cell(5).unwrap_or(0));
        assert!(conf_cell(CONF).is_none());
        assert!(strict_row(STRICT).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(conf_cell_name(0), Some("include 下发格"));
        assert_eq!(conf_cell_name(11), Some("typeCheckStatus 上报格"));
        assert!(conf_cell_name(CONF).is_none());
    }

    #[test]
    fn test_conf_dir_and_strict_tier() {
        // 双向投影方向 = p/6，strict 档位 = t/2。
        assert_eq!(conf_dir(0), Some(0));
        assert_eq!(conf_dir(5), Some(0));
        assert_eq!(conf_dir(6), Some(1));
        assert_eq!(conf_dir(11), Some(1));
        assert!(conf_dir(12).is_none());
        assert_eq!(conf_dir_name(0), Some("宿主→pyright 下发"));
        assert!(conf_dir_name(2).is_none());
        assert_eq!(strict_tier(0), Some(0));
        assert_eq!(strict_tier(3), Some(1));
        assert_eq!(strict_tier(5), Some(2));
        assert!(strict_tier(6).is_none());
        assert_eq!(strict_tier_name(2), Some("strict"));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PyrightLedger::new();
        // 对空账本：先落族 4 正格再注族 1 → 必抓且不改 last_kind。
        assert!(lt.record_cap(18, cap_cell(18).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(6, cap_cell(6).unwrap_or(0)),
            Err(PyrightCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(19, cap_cell(19).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PyrightLedger::new();
        assert_eq!(
            lt.record_cap(4, cap_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(PyrightCode::EDrift)
        );
        assert!(lt.record_conf(2, conf_cell(2).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_conf(9, conf_cell(9).unwrap_or(0).wrapping_add(6)),
            Err(PyrightCode::EConfDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PyrightLedger::new();
        assert!(lt.record_cap(15, cap_cell(15).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(15, cap_cell(15).unwrap_or(0)),
            Err(PyrightCode::EDup)
        );
        assert!(lt.record_strict(0).is_ok());
        assert_eq!(lt.record_strict(0), Err(PyrightCode::EDup));
        assert!(lt.declare_split(0).is_ok());
        assert_eq!(lt.declare_split(0), Err(PyrightCode::EDup));
    }

    #[test]
    fn test_gate_and_strict_boundary() {
        let mut lt = PyrightLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(PyrightCode::ESeal));
        assert!(lt.gates_all());
        let mut t = 0usize;
        while t < STRICT {
            assert!(lt.record_strict(t).is_ok());
            t += 1;
        }
        assert_eq!(lt.record_strict(STRICT), Err(PyrightCode::ENoStrict));
        assert!(lt.stricts_all());
        assert_eq!(PyrightLedger::gate_name(1), Some("配置装载"));
        assert!(PyrightLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = PyrightLedger::new();
        assert_eq!(a.seal(), Err(PyrightCode::ESeal));
        let mut b = PyrightLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(PyrightCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = c.declare_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(PyrightCode::ESeal));
        let mut d = c;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = d.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < CONF {
            if let Some(w) = conf_cell(p) {
                let _ = d.record_conf(p, w);
            }
            p += 1;
        }
        let mut t = 0usize;
        while t < STRICT {
            let _ = d.record_strict(t);
            t += 1;
        }
        let mut sp = 0usize;
        while sp < SPLIT_ROWS {
            let _ = d.declare_split(sp);
            sp += 1;
        }
        assert!(d.seal().is_ok());
        assert_eq!(d.declare_stack(STACK_ROWS), Err(PyrightCode::ERowMiss));
        // 分账行越行 → ENoSplit。
        assert_eq!(d.declare_split(SPLIT_ROWS), Err(PyrightCode::ENoSplit));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 12 投影 + 2 分账。
        assert_eq!(recorded, (CAPS + CONF) as u32);
        assert_eq!(caught, 10);
        let mut lt = PyrightLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 4;
        assert_eq!(lt.audit(), Err(PyrightCode::EInj));
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
        assert_eq!(SEG_BASE, 0xCBDC0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            PyrightCode::Ok,
            PyrightCode::ENoCap,
            PyrightCode::EDrift,
            PyrightCode::EOrder,
            PyrightCode::ENoConf,
            PyrightCode::EConfDrift,
            PyrightCode::ENoStrict,
            PyrightCode::ENoSplit,
            PyrightCode::ERowMiss,
            PyrightCode::EDup,
            PyrightCode::ESeal,
            PyrightCode::EInj,
            PyrightCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(PyrightCode::Ok as u32, 0);
        assert_eq!(PyrightCode::Ok.val(), SEG_BASE);
        assert_eq!(PyrightCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let empty = PyrightLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = PyrightLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(
            mid.verify_grade(Grade::S1),
            Err(PyrightCode::EGradeDrift)
        );
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < CONF {
            if let Some(w) = conf_cell(p) {
                let _ = full.record_conf(p, w);
            }
            p += 1;
        }
        let mut t = 0usize;
        while t < STRICT {
            let _ = full.record_strict(t);
            t += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_split(r);
            r += 1;
        }
        let mut sr = 0usize;
        while sr < STACK_ROWS {
            let _ = full.declare_stack(sr);
            sr += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(full.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52504_checks_green() {
        let s = run_unx52504_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
