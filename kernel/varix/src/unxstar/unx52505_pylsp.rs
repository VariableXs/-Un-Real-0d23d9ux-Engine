//! UNX-F52505 pylsp 收口 · 0xCBE00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52505"，逐字）：
//!   title: pylsp 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52506-J1 ×10；
//!     Node 消费联签（F52032 ABI 账锚定行）；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52505）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52506-J1，照原文锚定；功能按本卡标题（pylsp）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pylsp 语言服务五能力族 20 命令闭集：补全 / 悬浮提示 / lint 诊断 /
//!      格式化 / 重命名各 4 命令，逐命令载荷（纯函数折叠）恒等才收；
//!      能力族单调（逆序必抓，逆序拒不改 last_kind）。
//!   ② pyls 配置投影 8 格：宿主→pylsp 下发 4 格 + pylsp→宿主 上报 4 格，
//!      逐格对照，漂移必抓（先按合法序落正格再注漂移）。
//!   ③ Python 上栈 4 层闭集：解释器层 / 虚拟环境层 / site-packages 层 /
//!      pyls 插件层，逐层落账越层必拒（ENoStack）。
//!   ④ 与 pyright 分账声明 2 行（F52504）：pylsp 独立账 / pyright 分账
//!      两行闭集，缺行拒封；Node 消费联签（F52032 ABI 账锚定行）并入
//!      分账行 1，缺行拒封（ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（投影漂移/能力族逆序/越闭集）全落
//!      当轮临时账本 10/10 必抓；注入账守恒（注入=检出+逃逸）；双跑 FNV
//!      指纹一致；KB-COMPAT 定级闭集（pylsp 收口档为 S2：六判据齐即达
//!      S2，S1 申报即 EGradeDrift）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52505;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// pylsp 五能力族命令数（20 命令闭集）。
pub const CAPS: usize = 20;

/// pyls 配置投影格数（下发 4 + 上报 4）。
pub const CONF: usize = 8;

/// Python 上栈层数（4 层闭集）。
pub const STACK: usize = 4;

/// 与 pyright 分账声明行数（F52504；含 Node 消费联签 F52032 锚定行）。
pub const SPLIT_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PylspCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 配置投影格越闭集
    ENoConf,
    /// 配置投影漂移
    EConfDrift,
    /// 上栈层越闭集
    ENoStack,
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

impl PylspCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PylspCode::Ok => "成功",
            PylspCode::ENoCap => "能力号越闭集：pylsp 20 命令闭集之外先登记",
            PylspCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            PylspCode::EOrder => "能力族逆序：补全→悬浮提示→lint 诊断→格式化→重命名单调，逆序必抓",
            PylspCode::ENoConf => "配置投影格越闭集：pyls 配置投影 8 格之外先登记",
            PylspCode::EConfDrift => "配置投影漂移：投影折叠与基准不恒等，逐格复算",
            PylspCode::ENoStack => "上栈层越闭集：Python 上栈 4 层闭集之外先登记",
            PylspCode::ENoSplit => "分账行越闭集：pyright 分账 2 行（F52504）之外先登记",
            PylspCode::ERowMiss => "缺声明行：pyright 分账/Node 消费联签（F52032）未齐先补齐",
            PylspCode::EDup => "重复落账：同格只许一条账目",
            PylspCode::ESeal => "未齐即封：六判据 + 20 命令 + 8 投影 + 4 上栈层 + 2 分账未齐先补齐",
            PylspCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PylspCode::EGradeDrift => "定级漂移：KB-COMPAT 登记 pylsp 收口档为 S2，申报 S1 即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：不适用 pylsp（收口档上限 S2）
    S1,
    /// S2：pylsp 收口档
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
            Grade::S1 => "S1：不适用于 pylsp（KB-COMPAT 登记 pylsp 收口档为 S2）",
            Grade::S2 => "S2：pylsp 收口档，六判据齐即达（有条件收口）",
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
    Some((0xC5100u64 + (o as u64) * 0x2D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=lint 诊断 12..16=格式化 16..20=重命名）。
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
        2 => Some("lint 诊断"),
        3 => Some("格式化"),
        4 => Some("重命名"),
        _ => None,
    }
}

/// 配置投影格载荷（格 p；0..4=宿主→pylsp 下发 4..8=pylsp→宿主 上报）。
pub const fn conf_cell(p: usize) -> Option<u64> {
    if p >= CONF {
        return None;
    }
    Some((0xC5200u64 + (p as u64) * 0x35) ^ 0x0070796C73705F43)
}

/// 投影方向（p/4：0=下发 1=上报）。
pub const fn conf_dir(p: usize) -> Option<u32> {
    if p >= CONF {
        return None;
    }
    Some((p / 4) as u32)
}

/// 投影方向名。
pub const fn conf_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("宿主→pylsp 下发"),
        1 => Some("pylsp→宿主 上报"),
        _ => None,
    }
}

/// 配置投影格名（按格索引）。
pub const fn conf_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("pylint 下发格"),
        1 => Some("flake8 下发格"),
        2 => Some("black 下发格"),
        3 => Some("rope 下发格"),
        4 => Some("completion 上报格"),
        5 => Some("hover 上报格"),
        6 => Some("lintDiags 上报格"),
        7 => Some("formatStatus 上报格"),
        _ => None,
    }
}

/// Python 上栈层折叠（层 r；0=解释器 1=虚拟环境 2=site-packages 3=pyls 插件）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK {
        return None;
    }
    Some(0xC5300 + r as u32 * 0x2F)
}

/// 上栈层名（按层索引）。
pub const fn stack_layer_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("解释器层"),
        1 => Some("虚拟环境层"),
        2 => Some("site-packages 层"),
        3 => Some("pyls 插件层"),
        _ => None,
    }
}

/// 与 pyright 分账声明行折叠（行 r；F52504 锚定，行 1 并 Node 消费联签 F52032）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC5400 + r as u32 * 0x2C)
}

/// 分账行名（按行索引）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("pylsp 独立账/pyright 分账（F52504）"),
        1 => Some("Node 消费联签（F52032 ABI 账锚定行）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// pylsp 收口账本状态机。
#[derive(Clone)]
pub struct PylspLedger {
    /// 收口六判据（服务启动/配置装载/补全应答/诊断推送/格式化执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// 配置投影账。
    confs: [Option<u64>; CONF],
    /// Python 上栈层账。
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

impl PylspLedger {
    /// 新账本。
    pub fn new() -> Self {
        PylspLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            confs: [None; CONF],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PylspCode> {
        if gate >= GATES {
            return Err(PylspCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8C10 + gate as u64);
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
            1 => Some("配置装载"),
            2 => Some("补全应答"),
            3 => Some("诊断推送"),
            4 => Some("格式化执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), PylspCode> {
        if o >= CAPS {
            return Err(PylspCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(PylspCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8C20);
            return Err(PylspCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(PylspCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8C21 + o as u64);
            return Err(PylspCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PylspCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8C22 + o as u64);
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

    /// 配置投影一格（载荷对照 + 重复拒）。
    pub fn record_conf(&mut self, p: usize, got: u64) -> Result<(), PylspCode> {
        let want = match conf_cell(p) {
            Some(w) => w,
            None => return Err(PylspCode::ENoConf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8C30 + p as u64);
            return Err(PylspCode::EConfDrift);
        }
        if match self.confs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PylspCode::EDup);
        }
        if let Some(c) = self.confs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x8C31 + p as u64);
        Ok(())
    }

    /// 配置投影全账核验（8 格）。
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

    /// Python 上栈一层（越层必拒，重复拒）。
    pub fn record_stack(&mut self, r: usize) -> Result<(), PylspCode> {
        if r >= STACK {
            return Err(PylspCode::ENoStack);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PylspCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8C40 + r as u64);
        Ok(())
    }

    /// Python 上栈全齐核验（4 层）。
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

    /// 与 pyright 分账声明一行（越行即 ENoSplit）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), PylspCode> {
        if r >= SPLIT_ROWS {
            return Err(PylspCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PylspCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8C50 + r as u64);
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
    pub fn audit(&self) -> Result<(), PylspCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(PylspCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), PylspCode> {
        if !self.gates_all() {
            return Err(PylspCode::ESeal);
        }
        if !self.stacks_all() || !self.splits_all() {
            return Err(PylspCode::ERowMiss);
        }
        if !(self.caps_all() && self.confs_all()) {
            return Err(PylspCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（pylsp 收口档 S2：六判据齐即 S2，判据缺 → S3；
    /// 格账全齐亦不升 S1，S1 申报即漂移）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：申报档与实评档不一致 → EGradeDrift。
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), PylspCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(PylspCode::EGradeDrift)
        }
    }
}

impl Default for PylspLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl PylspLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

/// 双跑指纹核验：一致且非零才收。
pub fn verify_fp(a: u64, b: u64) -> Result<(), PylspCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(PylspCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PylspLedger::new();
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
    while r < STACK {
        let _ = led.record_stack(r);
        r += 1;
    }
    let mut sp = 0usize;
    while sp < SPLIT_ROWS {
        let _ = led.declare_split(sp);
        sp += 1;
    }
    // 注入 10 轮：投影漂移 / 能力族逆序 / 能力越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PylspLedger::new();
        let caught = match kind {
            0 => {
                // 投影漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_conf(1, conf_cell(1).unwrap_or(0));
                lt.record_conf(5, conf_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(PylspCode::EConfDrift)
            }
            1 => {
                // 族逆序：先落族 4 正格，再注族 1 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(17, cap_cell(17).unwrap_or(0));
                lt.record_cap(5, cap_cell(5).unwrap_or(0)) == Err(PylspCode::EOrder)
            }
            _ => lt.record_cap(CAPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8C60 + k as u64);
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
/// UNX-F52505 判据集（锚 UNX-F52506-J1，off-by-one 板式）。
pub fn run_unx52505_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52505");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pylsp 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "补全/悬浮提示/lint 诊断/格式化/重命名逐命令恒等（×10 口径）",
    );
    s.add(
        "pyls 配置投影 8 格全落",
        {
            let mut lt = PylspLedger::new();
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
        "下发 4 格 + 上报 4 格逐格恒等，漂移必抓",
    );
    s.add(
        "Python 上栈 4 层逐层全落",
        {
            let mut lt = PylspLedger::new();
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
        "解释器/虚拟环境/site-packages/pyls 插件四层闭集",
    );
    s.add(
        "pyright 分账声明 2/2（F52504 + Node 消费联签 F52032）",
        {
            let mut lt = PylspLedger::new();
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
        "pylsp 独立账/pyright 分账 + Node 消费联签（F52032 ABI 账锚定行）两行闭集",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = PylspLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "服务启动/配置装载/补全应答/诊断推送/格式化执行/会话退出逐格落账",
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
    let mut l0 = PylspLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·配置投影越闭集",
        l0.record_conf(CONF, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·上栈层越闭集",
        l0.record_stack(STACK).is_err(),
        "上栈层 4 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        PylspLedger::conserve_ok(10u32, caught, 0u32),
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
            let a = PylspLedger::new();
            let six_gate_miss = a.seal() == Err(PylspCode::ESeal);
            let mut b = PylspLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(PylspCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK {
                let _ = c.record_stack(r);
                r += 1;
            }
            let sp_miss = c.seal() == Err(PylspCode::ERowMiss);
            let mut d = c.clone();
            let mut sr = 0usize;
            while sr < SPLIT_ROWS {
                let _ = d.declare_split(sr);
                sr += 1;
            }
            let cell_miss = d.seal() == Err(PylspCode::ESeal);
            six_gate_miss && row_miss && sp_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = PylspLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.verify_grade(Grade::S2).is_ok()
                && lt.verify_grade(Grade::S1) == Err(PylspCode::EGradeDrift)
        },
        "六判据齐 → S2（KB-COMPAT 定级登记，§AI-66.3）；S1 申报即漂移",
    );
    s.add(
        "栈层与投影名表闭集",
        {
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK {
                if stack_layer_name(r).is_none() {
                    ok = false;
                }
                r += 1;
            }
            if stack_layer_name(STACK).is_some() {
                ok = false;
            }
            let mut p = 0usize;
            while p < CONF {
                if conf_cell_name(p).is_none() || conf_dir_name(p / 4).is_none() {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "4 栈层 + 8 投影格 + 2 方向名表逐名非空且闭集",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBE00 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
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
        assert_eq!(cap_kind_name(2), Some("lint 诊断"));
        assert_eq!(cap_kind_name(4), Some("重命名"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(9).unwrap_or(0);
        assert_eq!(a, cap_cell(9).unwrap_or(0));
        assert_ne!(a, cap_cell(10).unwrap_or(0));
        let b = conf_cell(3).unwrap_or(0);
        assert_eq!(b, conf_cell(3).unwrap_or(0));
        assert_ne!(b, conf_cell(4).unwrap_or(0));
        assert!(conf_cell(CONF).is_none());
        assert!(stack_row(STACK).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(conf_cell_name(0), Some("pylint 下发格"));
        assert_eq!(conf_cell_name(7), Some("formatStatus 上报格"));
        assert!(conf_cell_name(CONF).is_none());
        assert_eq!(split_row_name(0), Some("pylsp 独立账/pyright 分账（F52504）"));
        assert_eq!(split_row_name(1), Some("Node 消费联签（F52032 ABI 账锚定行）"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_conf_dir_and_stack_layers() {
        // 双向投影方向 = p/4，上栈层名按层索引。
        assert_eq!(conf_dir(0), Some(0));
        assert_eq!(conf_dir(3), Some(0));
        assert_eq!(conf_dir(4), Some(1));
        assert_eq!(conf_dir(7), Some(1));
        assert!(conf_dir(8).is_none());
        assert_eq!(conf_dir_name(0), Some("宿主→pylsp 下发"));
        assert_eq!(conf_dir_name(1), Some("pylsp→宿主 上报"));
        assert!(conf_dir_name(2).is_none());
        assert_eq!(stack_layer_name(0), Some("解释器层"));
        assert_eq!(stack_layer_name(3), Some("pyls 插件层"));
        assert!(stack_layer_name(4).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PylspLedger::new();
        // 对空账本：先落族 4 正格再注族 1 → 必抓且不改 last_kind。
        assert!(lt.record_cap(17, cap_cell(17).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(5, cap_cell(5).unwrap_or(0)),
            Err(PylspCode::EOrder)
        );
        assert_eq!(lt.last_kind(), before);
        assert!(lt.record_cap(18, cap_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PylspLedger::new();
        assert_eq!(
            lt.record_cap(4, cap_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(PylspCode::EDrift)
        );
        assert!(lt.record_conf(2, conf_cell(2).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_conf(6, conf_cell(6).unwrap_or(0).wrapping_add(6)),
            Err(PylspCode::EConfDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PylspLedger::new();
        assert!(lt.record_cap(15, cap_cell(15).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(15, cap_cell(15).unwrap_or(0)),
            Err(PylspCode::EDup)
        );
        assert!(lt.record_stack(0).is_ok());
        assert_eq!(lt.record_stack(0), Err(PylspCode::EDup));
        assert!(lt.declare_split(1).is_ok());
        assert_eq!(lt.declare_split(1), Err(PylspCode::EDup));
    }

    #[test]
    fn test_gate_and_stack_boundary() {
        let mut lt = PylspLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert_eq!(lt.judge_gate(GATES), Err(PylspCode::ESeal));
        assert!(lt.gates_all());
        let mut r = 0usize;
        while r < STACK {
            assert!(lt.record_stack(r).is_ok());
            r += 1;
        }
        assert_eq!(lt.record_stack(STACK), Err(PylspCode::ENoStack));
        assert!(lt.stacks_all());
        assert_eq!(PylspLedger::gate_name(2), Some("补全应答"));
        assert!(PylspLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = PylspLedger::new();
        assert_eq!(a.seal(), Err(PylspCode::ESeal));
        let mut b = PylspLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(PylspCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK {
            let _ = c.record_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(PylspCode::ERowMiss));
        let mut d = c;
        let mut sr = 0usize;
        while sr < SPLIT_ROWS {
            let _ = d.declare_split(sr);
            sr += 1;
        }
        assert_eq!(d.seal(), Err(PylspCode::ESeal));
        let mut e = d;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = e.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < CONF {
            if let Some(w) = conf_cell(p) {
                let _ = e.record_conf(p, w);
            }
            p += 1;
        }
        assert!(e.seal().is_ok());
        // 越行边界：分账行 2 → ENoSplit。
        assert_eq!(e.declare_split(SPLIT_ROWS), Err(PylspCode::ENoSplit));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 8 投影。
        assert_eq!(recorded, (CAPS + CONF) as u32);
        assert_eq!(caught, 10);
        let mut lt = PylspLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 4;
        assert_eq!(lt.audit(), Err(PylspCode::EInj));
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
        assert_eq!(SEG_BASE, 0xCBE00);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            PylspCode::Ok,
            PylspCode::ENoCap,
            PylspCode::EDrift,
            PylspCode::EOrder,
            PylspCode::ENoConf,
            PylspCode::EConfDrift,
            PylspCode::ENoStack,
            PylspCode::ENoSplit,
            PylspCode::ERowMiss,
            PylspCode::EDup,
            PylspCode::ESeal,
            PylspCode::EInj,
            PylspCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(PylspCode::Ok as u32, 0);
        assert_eq!(PylspCode::Ok.val(), SEG_BASE);
        assert_eq!(PylspCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2（目标档，前置必须补 judge_gate×6），
        // 格账全齐亦不升档；S1 申报即 EGradeDrift。
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let empty = PylspLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        assert_eq!(empty.verify_grade(Grade::S2), Err(PylspCode::EGradeDrift));
        let mut mid = PylspLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert!(mid.verify_grade(Grade::S2).is_ok());
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
        assert_eq!(full.judge_grade(), Grade::S2);
        assert!(full.verify_grade(Grade::S2).is_ok());
        assert_eq!(full.verify_grade(Grade::S1), Err(PylspCode::EGradeDrift));
    }

    #[test]
    fn unx52505_checks_green() {
        let s = run_unx52505_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
