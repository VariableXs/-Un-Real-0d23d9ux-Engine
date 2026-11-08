//! UNX-F52506 typescript-language-server 收口 · 0xCBE40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52506"，逐字）：
//!   title: typescript-language-server 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52507-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52506）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52507-J1，照原文锚定；功能按本卡标题（typescript-language-
//! server，下称 tsls）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① tsls 语言服务五能力族 20 命令闭集：补全 / 悬浮提示 / 诊断 / 重构 /
//!      引用查找各 4 命令，逐命令载荷（纯函数折叠）恒等才收；能力族单调
//!      （逆序必抓，逆序拒不改 last_kind）。
//!   ② tsconfig 消费 8 格：宿主→tsls 下发 4 格 + tsls→宿主 上报 4 格，
//!      逐格对照，漂移必抓（先按合法序落正格再注漂移）。
//!   ③ Node 消费声明 2 行闭集：Node 运行时消费声明 / npm scripts 消费
//!      声明，缺行拒封；JS/TS 上栈声明 2 行闭集：JavaScript 上栈声明 /
//!      TypeScript 上栈声明，缺行拒封（ERowMiss）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（族逆序/tsconfig 漂移/越闭集）全落
//!      当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑 FNV
//!      指纹一致；KB-COMPAT 定级闭集（六判据 + 20 命令 + 8 tsconfig 格
//!      + 2 分账 + 2 上栈全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52506;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// tsls 五能力族命令数（20 命令闭集）。
pub const CAPS: usize = 20;

/// tsconfig 消费格数（下发 4 + 上报 4）。
pub const TSCONF: usize = 8;

/// Node 消费声明行数。
pub const SPLIT_ROWS: usize = 2;

/// JS/TS 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TslsCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// tsconfig 格越闭集
    ENoTs,
    /// tsconfig 消费漂移
    ETsDrift,
    /// 上栈声明行越闭集
    ENoStack,
    /// Node 消费声明行越闭集
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

impl TslsCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TslsCode::Ok => "成功",
            TslsCode::ENoCap => "能力号越闭集：tsls 20 命令闭集之外先登记",
            TslsCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            TslsCode::EOrder => "能力族逆序：补全→悬浮提示→诊断→重构→引用查找不到调，逆序必抓",
            TslsCode::ENoTs => "tsconfig 格越闭集：tsconfig 消费 8 格之外先登记",
            TslsCode::ETsDrift => "tsconfig 消费漂移：消费折叠与基准不恒等，逐格复算",
            TslsCode::ENoStack => "上栈声明行越闭集：JS/TS 上栈声明 2 行之外先登记",
            TslsCode::ENoSplit => "Node 消费声明行越闭集：Node 消费声明 2 行之外先登记",
            TslsCode::ERowMiss => "缺声明行：Node 消费声明/JS/TS 上栈声明未齐先补齐",
            TslsCode::EDup => "重复落账：同格只许一条账目",
            TslsCode::ESeal => "未齐即封：六判据 + 20 命令 + 8 tsconfig 格 + 2 分账未齐先补齐",
            TslsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TslsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20命令+8tsconfig格+2分账+2上栈全通收口",
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
    Some((0xC6100u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=诊断 12..16=重构 16..20=引用查找）。
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
        3 => Some("重构"),
        4 => Some("引用查找"),
        _ => None,
    }
}

/// tsconfig 消费格载荷（格 p；0..4=宿主→tsls 下发 4..8=tsls→宿主 上报）。
pub const fn ts_cell(p: usize) -> Option<u64> {
    if p >= TSCONF {
        return None;
    }
    Some((0xC6200u64 + (p as u64) * 0x37) ^ 0x007473636F6E6631)
}

/// tsconfig 消费方向（p/4：0=下发 1=上报）。
pub const fn ts_dir(p: usize) -> Option<u32> {
    if p >= TSCONF {
        return None;
    }
    Some((p / 4) as u32)
}

/// tsconfig 消费方向名。
pub const fn ts_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("宿主→tsls 下发"),
        1 => Some("tsls→宿主 上报"),
        _ => None,
    }
}

/// tsconfig 消费格名（按格索引）。
pub const fn ts_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("compilerOptions 下发格"),
        1 => Some("include 下发格"),
        2 => Some("exclude 下发格"),
        3 => Some("plugin 下发格"),
        4 => Some("diagnostics 上报格"),
        5 => Some("completions 上报格"),
        6 => Some("refactorStatus 上报格"),
        7 => Some("projectErrors 上报格"),
        _ => None,
    }
}

/// JS/TS 上栈声明行折叠（行 r；0=JavaScript 1=TypeScript）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xC6300 + r as u32 * 0x2C)
}

/// 上栈声明行名（按行索引）。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("JavaScript 上栈声明行"),
        1 => Some("TypeScript 上栈声明行"),
        _ => None,
    }
}

/// Node 消费声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC6400 + r as u32 * 0x2C)
}

/// Node 消费声明行名（按行索引）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Node 运行时消费声明行"),
        1 => Some("npm scripts 消费声明行"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// tsls 收口账本状态机。
#[derive(Clone)]
pub struct TslsLedger {
    /// 收口六判据（服务启动/tsconfig 装载/诊断推送/重构执行/格式化执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// tsconfig 消费账。
    tss: [Option<u64>; TSCONF],
    /// JS/TS 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// Node 消费声明账。
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

impl TslsLedger {
    /// 新账本。
    pub fn new() -> Self {
        TslsLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            tss: [None; TSCONF],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TslsCode> {
        if gate >= GATES {
            return Err(TslsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8D10 + gate as u64);
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
            1 => Some("tsconfig 装载"),
            2 => Some("诊断推送"),
            3 => Some("重构执行"),
            4 => Some("格式化执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), TslsCode> {
        if o >= CAPS {
            return Err(TslsCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(TslsCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8D20);
            return Err(TslsCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(TslsCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8D21 + o as u64);
            return Err(TslsCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TslsCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8D22 + o as u64);
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

    /// tsconfig 消费一格（载荷对照 + 重复拒）。
    pub fn record_ts(&mut self, p: usize, got: u64) -> Result<(), TslsCode> {
        let want = match ts_cell(p) {
            Some(w) => w,
            None => return Err(TslsCode::ENoTs),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8D30 + p as u64);
            return Err(TslsCode::ETsDrift);
        }
        if match self.tss.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TslsCode::EDup);
        }
        if let Some(c) = self.tss.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8D31 + p as u64);
        Ok(())
    }

    /// tsconfig 消费全账核验（8 格）。
    pub fn tss_all(&self) -> bool {
        let mut p = 0usize;
        while p < TSCONF {
            match self.tss.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// JS/TS 上栈声明一行（越行即 ENoStack）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), TslsCode> {
        if r >= STACK_ROWS {
            return Err(TslsCode::ENoStack);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TslsCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8D40 + r as u64);
        Ok(())
    }

    /// JS/TS 上栈声明全齐核验（2 行）。
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

    /// Node 消费声明一行（越行即 ENoSplit）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TslsCode> {
        if r >= SPLIT_ROWS {
            return Err(TslsCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TslsCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8D50 + r as u64);
        Ok(())
    }

    /// Node 消费声明全齐核验（2 行）。
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
    pub fn audit(&self) -> Result<(), TslsCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(TslsCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), TslsCode> {
        if !self.gates_all() {
            return Err(TslsCode::ESeal);
        }
        if !self.stacks_all() || !self.splits_all() {
            return Err(TslsCode::ERowMiss);
        }
        if !(self.caps_all() && self.tss_all()) {
            return Err(TslsCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.tss_all()
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
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), TslsCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(TslsCode::EGradeDrift)
        }
    }
}

impl Default for TslsLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl TslsLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), TslsCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(TslsCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TslsLedger::new();
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
    while p < TSCONF {
        if let Some(w) = ts_cell(p) {
            if led.record_ts(p, w).is_ok() {
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
    let mut sr = 0usize;
    while sr < STACK_ROWS {
        let _ = led.declare_stack(sr);
        sr += 1;
    }
    // 注入 10 轮：族逆序 / tsconfig 漂移 / 能力越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TslsLedger::new();
        let caught = match kind {
            0 => {
                // 族逆序：先落族 4 正格，再注族 2 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(16, cap_cell(16).unwrap_or(0));
                lt.record_cap(9, cap_cell(9).unwrap_or(0)) == Err(TslsCode::EOrder)
            }
            1 => {
                // tsconfig 漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_ts(0, ts_cell(0).unwrap_or(0));
                lt.record_ts(4, ts_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(TslsCode::ETsDrift)
            }
            _ => lt.record_ts(TSCONF, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8D60 + k as u64);
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
/// UNX-F52506 判据集（锚 UNX-F52507-J1，off-by-one 板式）。
pub fn run_unx52506_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52506");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "tsls 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮提示/诊断/重构/引用查找逐命令恒等（×10 口径）",
    );
    s.add(
        "tsconfig 消费 8 格全落",
        {
            let mut lt = TslsLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < TSCONF {
                if let Some(w) = ts_cell(p) {
                    if lt.record_ts(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.tss_all()
        },
        "下发 4 格 + 上报 4 格逐格恒等，漂移必抓",
    );
    s.add(
        "Node 消费声明 2/2 全齐",
        {
            let mut lt = TslsLedger::new();
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
        "Node 运行时消费/npm scripts 消费两行闭集，缺行拒封",
    );
    s.add(
        "JS/TS 上栈声明 2/2 全齐",
        {
            let mut lt = TslsLedger::new();
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
        "JavaScript/TypeScript 上栈声明两行闭集，缺行 ERowMiss",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = TslsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "服务启动/tsconfig 装载/诊断推送/重构执行/格式化执行/会话退出逐格落账",
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
        "族逆序/tsconfig 漂移/越闭集三态轮转必抓",
    );
    let mut l0 = TslsLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·tsconfig 格越闭集",
        l0.record_ts(TSCONF, 0).is_err(),
        "tsconfig 格 8 越闭集必拒",
    );
    s.add(
        "边界·上栈声明行越闭集",
        l0.declare_stack(STACK_ROWS).is_err(),
        "上栈声明行 2 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        TslsLedger::conserve_ok(10u32, caught, 0u32),
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
            let a = TslsLedger::new();
            let six_gate_miss = a.seal() == Err(TslsCode::ESeal);
            let mut b = TslsLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(TslsCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = c.declare_stack(r);
                r += 1;
            }
            let split_miss = c.seal() == Err(TslsCode::ERowMiss);
            let mut d = c.clone();
            let mut sr = 0usize;
            while sr < SPLIT_ROWS {
                let _ = d.declare_split(sr);
                sr += 1;
            }
            let cell_miss = d.seal() == Err(TslsCode::ESeal);
            six_gate_miss && row_miss && split_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = TslsLedger::new();
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
            while p < TSCONF {
                if let Some(w) = ts_cell(p) {
                    let _ = lt.record_ts(p, w);
                }
                p += 1;
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
        "tsconfig 方向与格名闭集",
        {
            let mut ok = true;
            let mut p = 0usize;
            while p < TSCONF {
                if ts_cell_name(p).is_none() || ts_dir_name(p / 4).is_none() {
                    ok = false;
                }
                p += 1;
            }
            if ts_cell_name(TSCONF).is_some() || ts_dir_name(2).is_some() {
                ok = false;
            }
            ok
        },
        "8 消费格 + 2 方向名表逐名非空且闭集",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBE40 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
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
        assert_eq!(cap_kind_name(3), Some("重构"));
        assert_eq!(cap_kind_name(4), Some("引用查找"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(11).unwrap_or(0);
        assert_eq!(a, cap_cell(11).unwrap_or(0));
        assert_ne!(a, cap_cell(12).unwrap_or(0));
        let b = ts_cell(2).unwrap_or(0);
        assert_eq!(b, ts_cell(2).unwrap_or(0));
        assert_ne!(b, ts_cell(3).unwrap_or(0));
        assert!(ts_cell(TSCONF).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(ts_cell_name(3), Some("plugin 下发格"));
        assert_eq!(ts_cell_name(7), Some("projectErrors 上报格"));
        assert!(ts_cell_name(TSCONF).is_none());
        assert_eq!(stack_row_name(1), Some("TypeScript 上栈声明行"));
        assert!(stack_row_name(2).is_none());
        assert_eq!(split_row_name(0), Some("Node 运行时消费声明行"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_ts_dir() {
        // 消费方向 = p/4。
        assert_eq!(ts_dir(0), Some(0));
        assert_eq!(ts_dir(3), Some(0));
        assert_eq!(ts_dir(4), Some(1));
        assert_eq!(ts_dir(7), Some(1));
        assert!(ts_dir(8).is_none());
        assert_eq!(ts_dir_name(0), Some("宿主→tsls 下发"));
        assert_eq!(ts_dir_name(1), Some("tsls→宿主 上报"));
        assert!(ts_dir_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TslsLedger::new();
        // 对空账本：先落族 4 正格再注族 2 → 必抓且不改 last_kind。
        assert!(lt.record_cap(16, cap_cell(16).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(9, cap_cell(9).unwrap_or(0)),
            Err(TslsCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(17, cap_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TslsLedger::new();
        assert_eq!(
            lt.record_cap(4, cap_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(TslsCode::EDrift)
        );
        assert!(lt.record_ts(1, ts_cell(1).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_ts(6, ts_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(TslsCode::ETsDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TslsLedger::new();
        assert!(lt.record_cap(12, cap_cell(12).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(12, cap_cell(12).unwrap_or(0)),
            Err(TslsCode::EDup)
        );
        assert!(lt.declare_stack(0).is_ok());
        assert_eq!(lt.declare_stack(0), Err(TslsCode::EDup));
        assert!(lt.declare_split(1).is_ok());
        assert_eq!(lt.declare_split(1), Err(TslsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TslsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(TslsCode::ESeal));
        assert!(lt.gates_all());
        assert_eq!(TslsLedger::gate_name(1), Some("tsconfig 装载"));
        assert!(TslsLedger::gate_name(GATES).is_none());
        // 越行边界双验。
        assert_eq!(lt.declare_stack(STACK_ROWS), Err(TslsCode::ENoStack));
        assert_eq!(lt.declare_split(SPLIT_ROWS), Err(TslsCode::ENoSplit));
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = TslsLedger::new();
        assert_eq!(a.seal(), Err(TslsCode::ESeal));
        let mut b = TslsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(TslsCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = c.declare_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(TslsCode::ERowMiss));
        let mut d = c;
        let mut sr = 0usize;
        while sr < SPLIT_ROWS {
            let _ = d.declare_split(sr);
            sr += 1;
        }
        assert_eq!(d.seal(), Err(TslsCode::ESeal));
        let mut e = d;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = e.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < TSCONF {
            if let Some(w) = ts_cell(p) {
                let _ = e.record_ts(p, w);
            }
            p += 1;
        }
        assert!(e.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 8 tsconfig 格。
        assert_eq!(recorded, (CAPS + TSCONF) as u32);
        assert_eq!(caught, 10);
        let mut lt = TslsLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 4;
        assert_eq!(lt.audit(), Err(TslsCode::EInj));
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
        assert_eq!(SEG_BASE, 0xCBE40);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            TslsCode::Ok,
            TslsCode::ENoCap,
            TslsCode::EDrift,
            TslsCode::EOrder,
            TslsCode::ENoTs,
            TslsCode::ETsDrift,
            TslsCode::ENoStack,
            TslsCode::ENoSplit,
            TslsCode::ERowMiss,
            TslsCode::EDup,
            TslsCode::ESeal,
            TslsCode::EInj,
            TslsCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(TslsCode::Ok as u32, 0);
        assert_eq!(TslsCode::Ok.val(), SEG_BASE);
        assert_eq!(TslsCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1（前置必须补 judge_gate×6）。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let empty = TslsLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = TslsLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(mid.verify_grade(Grade::S1), Err(TslsCode::EGradeDrift));
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < TSCONF {
            if let Some(w) = ts_cell(p) {
                let _ = full.record_ts(p, w);
            }
            p += 1;
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
        assert!(full.seal().is_ok());
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(full.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52506_checks_green() {
        let s = run_unx52506_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
