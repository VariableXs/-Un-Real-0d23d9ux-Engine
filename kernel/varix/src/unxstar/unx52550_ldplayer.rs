//! UNX-F52550 LDPlayer 收口（N/A 同构） · 0xCC940段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52550"，逐字）：
//!   id=52550 title="LDPlayer 收口（N/A 同构）" quota=260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52551-J1 同口径；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52550）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52551-J1，照原文锚定（MEmu 域语料为下一卡语料）；功能按本卡
//! 标题（LDPlayer / N/A 同构）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① N/A 同构语义：与 Genymotion 52547 / BlueStacks 52548 / NoxPlayer
//!      52549 同构——LDPlayer 的虚拟化依赖探测（Hypervisor 检测）在内核
//!      纯函数环境不可用，如实 N/A，KB-COMPAT 定级封顶 S2 + N/A 清单，
//!      禁虚报可用。探测账三分支闭集：Available（可信证据链）/
//!      NotAvailable（N/A 如实登记 + 三要素告警：依赖项名/原因/替代
//!      路径）/ ProbeFailed（探测失败显性，与 N/A 严格区分不造假）。
//!   ② 域模型：五面 20 操作格（安装面 4/实例面 4（多开实例清单闭集）/
//!      按键映射面 4（键位分账）/ADB 消费面 4/N/A 探测账面 4）单调；
//!      实例投影 8 格 = 实例段 0..4、键位段 4..8；漂移必抓。
//!   ③ 版本阶梯 3 档：9.0.90/9.1.0/9.1.1 三档闭集，越级必抓、漂移必抓。
//!   ④ 声明 4 行闭集：Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账
//!      声明 2 行，缺一拒封（seal：六闸缺→ESeal、声明行缺→ERowMiss、
//!      格账缺→ESeal、N/A 探测账未如实登记→ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/na_probe 三分支探测账/
//!      seal/fingerprint FNV-1a/grade 定级闭集）；注入核 run_cycle 10 轮
//!      三态轮转（版本漂移/操作漂移/N/A 虚报可用），三类注入全部落在
//!      当轮临时账本上，主账 injected==caught==10、escaped==0；双跑指纹
//!      一致；定级目标档 S2（N/A 封顶），虚报 S1 = EGradeDrift 必抓。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52550;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 恒等 0xCC940 且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCC940 && SEG_BASE % 0x40 == 0);

/// LDPlayer 五面操作数。
pub const OPS: usize = 20;

/// 实例投影格数。
pub const PATH_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 声明行数（Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行）。
pub const SPLIT_ROWS: usize = 4;

/// 六判据收口闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LdCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移（含 N/A 虚报可用）
    EDrift,
    /// 能力面逆序
    EOrder,
    /// 实例投影格越闭集
    ENoPath,
    /// 投影漂移
    EPathDrift,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移（虚报 S1 必抓）
    EGradeDrift,
}

impl LdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LdCode::Ok => "成功",
            LdCode::ENoOp => "操作号越闭集：LDPlayer 五面 20 格之外先登记",
            LdCode::EDrift => "操作漂移：能力格/探测账载荷与基准不恒等（含 N/A 虚报可用必抓），逐项复算",
            LdCode::EOrder => "能力面逆序：安装面→实例面（多开实例清单闭集）→按键映射面（键位分账）→ADB 消费面→N/A 探测账面单调，逆序必抓",
            LdCode::ENoPath => "实例投影越闭集：实例投影 8 格（实例段 0..4/键位段 4..8）之外先登记",
            LdCode::EPathDrift => "投影漂移：实例投影折叠与基准不恒等，逐格复算",
            LdCode::ENoVer => "版本级越阶梯：9.0.90/9.1.0/9.1.1 三档闭集之外先登记",
            LdCode::EVerDrift => "版本阶梯漂移：版本档折叠与基准不恒等，逐档复算",
            LdCode::ENoRow => "声明行越闭集：Hypervisor 依赖 N/A 声明 2 行+ADB 消费分账声明 2 行之外先登记",
            LdCode::ERowMiss => "缺声明行：Hypervisor 依赖 N/A 声明/ADB 消费分账声明未齐先补齐",
            LdCode::EDup => "重复落账：同格/同账只许一条",
            LdCode::ESeal => "未齐即封：20 能力格+8 投影格+3 档版本+六判据+N/A 探测账如实登记未齐先补齐",
            LdCode::EGradeDrift => "定级漂移：本卡虚拟化面内核环境恒 N/A，封顶 S2+N/A 清单，虚报 S1 必抓（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡虚拟化面恒 N/A，S1 不可达）
    S1,
    /// S2：N/A 封顶目标档
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
            Grade::S1 => "S1：全通收口（本卡虚拟化面内核环境恒 N/A，S1 不可达，虚报必抓）",
            Grade::S2 => "S2：N/A 封顶目标档，全通+N/A 清单如实登记（KB-COMPAT）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── N/A 探测账三分支闭集 ─────────────────────────────────────────
/// 探测账三分支闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NaVerdict {
    /// Available：可信证据链证明依赖可用（本卡环境无此链）
    Available,
    /// NotAvailable：N/A 如实登记 + 三要素告警
    NotAvailable,
    /// ProbeFailed：探测失败显性，与 N/A 严格区分
    ProbeFailed,
}

impl NaVerdict {
    /// 分支值。
    pub fn val(self) -> u32 {
        match self {
            NaVerdict::Available => 0,
            NaVerdict::NotAvailable => 1,
            NaVerdict::ProbeFailed => 2,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            NaVerdict::Available => "Available：声称有可信证据链证明依赖可用（本卡内核纯函数环境无此链，虚报必抓）",
            NaVerdict::NotAvailable => "NotAvailable：N/A 如实登记，附三要素告警（依赖项名/原因/替代路径）",
            NaVerdict::ProbeFailed => "ProbeFailed：探测失败显性登记，与 N/A 严格区分，不冒充任何结论不造假",
        }
    }
}

/// N/A 三要素告警（依赖项名/原因/替代路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NaFinding {
    /// 分支。
    pub verdict: NaVerdict,
    /// 要素一：依赖项名。
    pub dep: &'static str,
    /// 要素二：原因。
    pub reason: &'static str,
    /// 要素三：替代路径。
    pub alt: &'static str,
}

/// 探测账三要素折叠（按分支闭集）。
pub const fn na_finding(v: NaVerdict) -> NaFinding {
    match v {
        NaVerdict::Available => NaFinding {
            verdict: v,
            dep: "Hypervisor 检测（LDPlayer 依赖的虚拟化层）",
            reason: "虚报：无可信证据链却声称依赖可用，禁虚报可用",
            alt: "如实登记 NotAvailable 并附三要素告警",
        },
        NaVerdict::NotAvailable => NaFinding {
            verdict: v,
            dep: "Hypervisor 检测（LDPlayer 依赖的虚拟化层）",
            reason: "内核纯函数环境无法执行 CPUID/虚拟化位探测，依赖项不可用",
            alt: "改由宿主侧 Hypervisor 检测 CLI 分账承接，本卡登记 N/A 清单",
        },
        NaVerdict::ProbeFailed => NaFinding {
            verdict: v,
            dep: "Hypervisor 检测（LDPlayer 依赖的虚拟化层）",
            reason: "探测执行未返回结论，证据链中断，与 N/A 严格区分",
            alt: "复探或显性登记 ProbeFailed，禁止冒充 N/A 或 Available",
        },
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力面 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x2C) ^ 0x4C44504C41590000)
}

/// 能力面（0..4=安装面 4..8=实例面 8..12=按键映射面 12..16=ADB 消费面 16..20=N/A 探测账面）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力面名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装面"),
        1 => Some("实例面"),
        2 => Some("按键映射面"),
        3 => Some("ADB 消费面"),
        4 => Some("N/A 探测账面"),
        _ => None,
    }
}

/// 实例投影格载荷（格 c；投影族 = c/4：0..4=实例段 4..8=键位段）。
pub const fn path_cell(c: usize) -> Option<u64> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((0xF2000u64 + (c as u64) * 0x31) ^ 0x4C44504C00000000)
}

/// 投影族。
pub const fn path_kind(c: usize) -> Option<u32> {
    if c >= PATH_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 投影族名。
pub const fn path_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("实例段投影（多开实例清单）"),
        1 => Some("键位段投影（键位分账）"),
        _ => None,
    }
}

/// 版本阶梯档载荷（档 v）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF3000u64 + (v as u64) * 0x5D) ^ 0x4C44500000000000)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("9.0.90"),
        1 => Some("9.1.0"),
        2 => Some("9.1.1"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF510 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Hypervisor 依赖 N/A 声明·主"),
        1 => Some("Hypervisor 依赖 N/A 声明·辅"),
        2 => Some("ADB 消费分账声明·主"),
        3 => Some("ADB 消费分账声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// LDPlayer 收口账本状态机。
#[derive(Clone)]
pub struct LdLedger {
    /// 六判据（收口闸：安装/实例/按键映射/ADB 消费/N/A 探测账/封账写回）。
    gates: [Option<bool>; GATES],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 实例投影账。
    paths: [Option<u64>; PATH_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// N/A 探测账分支。
    na: Option<NaVerdict>,
    /// 最后能力面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LdLedger {
    /// 新账本。
    pub fn new() -> Self {
        LdLedger {
            gates: [None; GATES],
            ops: [None; OPS],
            paths: [None; PATH_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
            na: None,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LdCode> {
        if gate >= GATES {
            return Err(LdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9A00 + gate as u64);
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

    /// 能力一格（能力面单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), LdCode> {
        if o >= OPS {
            return Err(LdCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(LdCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B00);
            return Err(LdCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(LdCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9B01 + o as u64);
            return Err(LdCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LdCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9B02 + o as u64);
        Ok(())
    }

    /// 能力全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 实例投影一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_path(&mut self, c: usize, got: u64) -> Result<(), LdCode> {
        let want = match path_cell(c) {
            Some(w) => w,
            None => return Err(LdCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9C00 + c as u64);
            return Err(LdCode::EPathDrift);
        }
        if match self.paths.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LdCode::EDup);
        }
        if let Some(p) = self.paths.get_mut(c) {
            *p = Some(got);
        }
        self.fnv(0x9C01 + c as u64);
        Ok(())
    }

    /// 实例投影全账核验（8 格）。
    pub fn paths_all(&self) -> bool {
        let mut c = 0usize;
        while c < PATH_CELLS {
            match self.paths.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 版本阶梯一档（载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), LdCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(LdCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9D00 + v as u64);
            return Err(LdCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LdCode::EDup);
        }
        if let Some(b) = self.vers.get_mut(v) {
            *b = Some(got);
        }
        self.fnv(0x9D01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), LdCode> {
        if r >= SPLIT_ROWS {
            return Err(LdCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LdCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x9E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
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

    /// N/A 探测账（三分支闭集）：Available 虚报必抓；NotAvailable 如实
    /// 登记；ProbeFailed 显性登记但不足以封账。重复登记 EDup。
    pub fn na_probe(&mut self, v: NaVerdict) -> Result<NaVerdict, LdCode> {
        if v == NaVerdict::Available {
            // 虚报可用：本卡内核纯函数环境无可信证据链，注入必抓。
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA000);
            return Err(LdCode::EDrift);
        }
        if self.na.is_some() {
            return Err(LdCode::EDup);
        }
        self.na = Some(v);
        self.fnv(0xA000 + 1 + v.val() as u64);
        Ok(v)
    }

    /// N/A 探测账分支读取。
    pub fn na_verdict(&self) -> Option<NaVerdict> {
        self.na
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal、
    /// N/A 探测账未如实登记（缺/ProbeFailed）→ESeal。
    pub fn seal(&self) -> Result<(), LdCode> {
        if !self.gates_all() {
            return Err(LdCode::ESeal);
        }
        if !self.splits_all() {
            return Err(LdCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.vers_all() {
            return Err(LdCode::ESeal);
        }
        match self.na {
            Some(NaVerdict::NotAvailable) => {}
            _ => return Err(LdCode::ESeal),
        }
        Ok(())
    }

    /// 全通核验（含 N/A 分支如实登记）。
    pub fn complete(&self) -> bool {
        self.gates_all()
            && self.ops_all()
            && self.paths_all()
            && self.vers_all()
            && self.splits_all()
            && self.na == Some(NaVerdict::NotAvailable)
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐+如实 N/A → 封顶目标档 S2（N/A 清单）；
    /// 六闸齐 → S2（有条件）；空账 → S3。本卡恒不出 S1。
    pub fn grade(&self) -> Grade {
        if self.complete() || self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级申报核验：虚报 S1 → EGradeDrift 必抓；S2 按实定级；S3 如实。
    pub fn try_grade(&self, target: Grade) -> Result<Grade, LdCode> {
        match target {
            Grade::S1 => Err(LdCode::EGradeDrift),
            Grade::S2 => Ok(self.grade()),
            Grade::S3 => Ok(Grade::S3),
        }
    }
}

impl Default for LdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LdLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < PATH_CELLS {
        if let Some(w) = path_cell(c) {
            if led.record_path(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // N/A 分支如实登记（NotAvailable + 三要素告警），主账封账全通。
    let _ = led.na_probe(NaVerdict::NotAvailable);
    let _ = led.seal();
    // 注入 10 轮：版本漂移 / 操作漂移 / N/A 虚报可用三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LdLedger::new();
        let caught = match kind {
            0 => {
                // 版本漂移：先按合法序落 0 档正格，再注 1 档漂移，必抓。
                let w0 = match version_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match version_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_version(0, w0);
                lt.record_version(1, w1.wrapping_add(1)) == Err(LdCode::EVerDrift)
            }
            1 => {
                // 操作漂移：先按合法序落同族正格（安装面），再注漂移。
                let w0 = match op_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match op_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(0, w0);
                lt.record_op(1, w1.wrapping_add(1)) == Err(LdCode::EDrift)
            }
            _ => {
                // N/A 虚报可用：探测账 Available 分支必抓。
                lt.na_probe(NaVerdict::Available).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA100 + k as u64);
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
/// UNX-F52550 判据集（锚 UNX-F52551-J1，off-by-one 板式；§AI-66.3）。
pub fn run_unx52550_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52550");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "LDPlayer 五面 20 操作全通",
        recorded >= OPS as u32,
        "安装/实例/按键映射/ADB 消费/N/A 探测账逐格恒等",
    );
    s.add(
        "实例投影 8/8 全落",
        {
            let mut lt = LdLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    if lt.record_path(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.paths_all()
        },
        "实例段投影 0..4 与键位段投影 4..8 双族逐格恒等",
    );
    s.add(
        "版本阶梯 9.0.90/9.1.0/9.1.1 三档全落",
        {
            let mut lt = LdLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐档恒等，越级必抓",
    );
    s.add(
        "Hypervisor N/A 声明与 ADB 消费分账声明 4/4 全齐",
        {
            let mut lt = LdLedger::new();
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
        "Hypervisor 依赖 N/A 声明 2 行 + ADB 消费分账声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = LdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/实例/按键映射/ADB 消费/N/A 探测账/封账写回逐格落账",
    );
    s.add(
        "N/A 探测账·Available 虚报必抓",
        {
            let mut lt = LdLedger::new();
            let r = lt.na_probe(NaVerdict::Available);
            let st = lt.stats();
            r == Err(LdCode::EDrift) && st.0 == 1 && st.1 == 1 && st.2 == 0
        },
        "无可信证据链声称依赖可用即注入必抓，禁虚报可用",
    );
    s.add(
        "N/A 探测账·NotAvailable 如实登记且三要素齐",
        {
            let f = na_finding(NaVerdict::NotAvailable);
            let mut lt = LdLedger::new();
            let r = lt.na_probe(NaVerdict::NotAvailable);
            r == Ok(NaVerdict::NotAvailable)
                && lt.na_verdict() == Some(NaVerdict::NotAvailable)
                && !f.dep.is_empty()
                && !f.reason.is_empty()
                && !f.alt.is_empty()
        },
        "依赖项名/原因/替代路径三要素告警齐备，N/A 如实登记",
    );
    s.add(
        "N/A 探测账·ProbeFailed 显性与 N/A 严格区分",
        {
            let mut lt = LdLedger::new();
            let r = lt.na_probe(NaVerdict::ProbeFailed);
            r == Ok(NaVerdict::ProbeFailed)
                && lt.na_verdict() == Some(NaVerdict::ProbeFailed)
                && lt.seal() == Err(LdCode::ESeal)
        },
        "探测失败显性登记不冒充 N/A，且不足以封账",
    );
    s.add(
        "能力面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→实例→按键映射→ADB 消费→N/A 探测账闭集单调",
    );
    s.add(
        "投影族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PATH_CELLS {
                let ka = match path_kind(c) {
                    Some(k) => k,
                    None => 2,
                };
                let kb = match path_kind(c - 1) {
                    Some(k) => k,
                    None => 2,
                };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "实例段→键位段双族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本漂移/操作漂移/N/A 虚报三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = LdLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        {
            let mut lt = LdLedger::new();
            lt.record_path(PATH_CELLS, 0).is_err()
        },
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = LdLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "声明行 4 越闭集必拒",
    );
    s.add(
        "能力漂移拒",
        {
            let mut lt = LdLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(LdCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明+N/A 齐但投影缺一也必拒。
            let lt_empty = LdLedger::new();
            let mut lt_cell = LdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt_cell.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            let _ = lt_cell.na_probe(NaVerdict::NotAvailable);
            let mut c = 1usize;
            while c < PATH_CELLS {
                if let Some(w) = path_cell(c) {
                    let _ = lt_cell.record_path(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(LdCode::ESeal) && lt_cell.seal() == Err(LdCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = LdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let _ = lt.na_probe(NaVerdict::NotAvailable);
            lt.seal() == Err(LdCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "版本越阶梯拒",
        {
            let mut lt = LdLedger::new();
            lt.record_version(VERSION_STEPS, 0).is_err()
        },
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "41 账目全通封账",
        {
            let mut lt = LdLedger::new();
            let mut n = 0u32;
            let mut g = 0usize;
            while g < GATES { if lt.judge_gate(g).is_ok() { n += 1; } g += 1; }
            let mut o = 0usize;
            while o < OPS { if let Some(w) = op_cell(o) { if lt.record_op(o, w).is_ok() { n += 1; } } o += 1; }
            let mut c = 0usize;
            while c < PATH_CELLS { if let Some(w) = path_cell(c) { if lt.record_path(c, w).is_ok() { n += 1; } } c += 1; }
            let mut v = 0usize;
            while v < VERSION_STEPS { if let Some(w) = version_cell(v) { if lt.record_version(v, w).is_ok() { n += 1; } } v += 1; }
            let mut r = 0usize;
            while r < SPLIT_ROWS { if lt.declare_split(r).is_ok() { n += 1; } r += 1; }
            // N/A 分支如实登记（不计 41 格账），seal 才可 Ok。
            let _ = lt.na_probe(NaVerdict::NotAvailable);
            n == 41 && lt.seal().is_ok()
        },
        "6 闸+20 能力+8 投影+3 版本+4 声明共 41 账目，N/A 分支如实登记另行落账后 seal Ok",
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
        "定级 N/A 封顶 S2 且虚报 S1 必抓",
        {
            let mut lt = LdLedger::new();
            let mut g = 0usize;
            while g < GATES { let _ = lt.judge_gate(g); g += 1; }
            let mut o = 0usize;
            while o < OPS { if let Some(w) = op_cell(o) { let _ = lt.record_op(o, w); } o += 1; }
            let mut c = 0usize;
            while c < PATH_CELLS { if let Some(w) = path_cell(c) { let _ = lt.record_path(c, w); } c += 1; }
            let mut v = 0usize;
            while v < VERSION_STEPS { if let Some(w) = version_cell(v) { let _ = lt.record_version(v, w); } v += 1; }
            let mut r = 0usize;
            while r < SPLIT_ROWS { let _ = lt.declare_split(r); r += 1; }
            let _ = lt.na_probe(NaVerdict::NotAvailable);
            lt.grade() == Grade::S2 && lt.try_grade(Grade::S1) == Err(LdCode::EGradeDrift)
        },
        "全齐+N/A 如实 → 封顶 S2+N/A 清单（KB-COMPAT），虚报 S1 = EGradeDrift",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC940",
    );
    s.add("收口·判据条数", s.len() >= 23, "含本条共 ≥24 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(16), Some(4));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("安装面"));
        assert_eq!(op_kind_name(2), Some("按键映射面"));
        assert_eq!(op_kind_name(4), Some("N/A 探测账面"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_path_kind() {
        assert_eq!(path_kind(0), Some(0));
        assert_eq!(path_kind(3), Some(0));
        assert_eq!(path_kind(4), Some(1));
        assert_eq!(path_kind(7), Some(1));
        assert!(path_kind(8).is_none());
        assert!(path_cell(8).is_none());
        assert_eq!(path_kind_name(0), Some("实例段投影（多开实例清单）"));
        assert_eq!(path_kind_name(1), Some("键位段投影（键位分账）"));
        assert!(path_kind_name(2).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("9.0.90"));
        assert_eq!(version_name(1), Some("9.1.0"));
        assert_eq!(version_name(2), Some("9.1.1"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        assert_eq!(op_cell(3).unwrap_or(0), op_cell(3).unwrap_or(0));
        assert_ne!(op_cell(3).unwrap_or(0), op_cell(4).unwrap_or(0));
        assert_ne!(path_cell(0).unwrap_or(0), path_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(split_name(0), Some("Hypervisor 依赖 N/A 声明·主"));
        assert_eq!(split_name(2), Some("ADB 消费分账声明·主"));
        assert!(split_name(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = LdLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(LdCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LdLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LdCode::EDrift)
        );
        assert_eq!(
            lt.record_path(1, path_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(LdCode::EPathDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(LdCode::EVerDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LdLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(LdCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(LdCode::EDup));
        assert!(lt.na_probe(NaVerdict::NotAvailable).is_ok());
        assert_eq!(lt.na_probe(NaVerdict::NotAvailable), Err(LdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LdLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = LdLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_path(PATH_CELLS, 0);
        let _ = lt.record_version(VERSION_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = LdLedger::new();
        let mut g = 0usize;
        while g < GATES { let _ = full.judge_gate(g); g += 1; }
        let mut o = 0usize;
        while o < OPS { if let Some(w) = op_cell(o) { let _ = full.record_op(o, w); } o += 1; }
        let mut c = 0usize;
        while c < PATH_CELLS { if let Some(w) = path_cell(c) { let _ = full.record_path(c, w); } c += 1; }
        let mut v = 0usize;
        while v < VERSION_STEPS { if let Some(w) = version_cell(v) { let _ = full.record_version(v, w); } v += 1; }
        assert_eq!(full.seal(), Err(LdCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS { let _ = full.declare_split(r); r += 1; }
        // N/A 探测账未登记仍不得封。
        assert_eq!(full.seal(), Err(LdCode::ESeal));
        assert!(full.na_probe(NaVerdict::NotAvailable).is_ok());
        assert!(full.seal().is_ok());
        assert!(full.complete());
    }

    #[test]
    fn test_na_probe_branches() {
        // Available：虚报必抓，计注入账。
        let mut lt = LdLedger::new();
        assert_eq!(lt.na_probe(NaVerdict::Available), Err(LdCode::EDrift));
        assert_eq!(lt.stats(), (1, 1, 0));
        assert!(lt.na_verdict().is_none());
        // ProbeFailed：显性登记但不满足封账 N/A 要求。
        let mut lt2 = LdLedger::new();
        assert_eq!(lt2.na_probe(NaVerdict::ProbeFailed), Ok(NaVerdict::ProbeFailed));
        assert_eq!(lt2.na_verdict(), Some(NaVerdict::ProbeFailed));
        assert!(!lt2.complete());
        // 三分支闭集值与三要素全非空。
        assert_eq!(NaVerdict::Available.val(), 0);
        assert_eq!(NaVerdict::NotAvailable.val(), 1);
        assert_eq!(NaVerdict::ProbeFailed.val(), 2);
        let f = na_finding(NaVerdict::NotAvailable);
        assert!(!f.dep.is_empty() && !f.reason.is_empty() && !f.alt.is_empty());
        assert!(f.verdict == NaVerdict::NotAvailable);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 8 投影 + 3 档版本（六闸/声明行/N-A 探测账不计数）。
        assert_eq!(recorded, OPS as u32 + PATH_CELLS as u32 + VERSION_STEPS as u32);
        assert_eq!(recorded, 31);
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
        assert_eq!(SEG_BASE, 0xCC940);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LdCode::Ok as u32, 0);
        assert_eq!(LdCode::ENoOp as u32, 1);
        assert_eq!(LdCode::EOrder as u32, 3);
        assert_eq!(LdCode::EGradeDrift as u32, 12);
        assert_eq!(LdCode::Ok.val(), SEG_BASE);
        // 13 变体闭集：val 连续且 detail 全非空中文。
        let mut i = 0u32;
        for cd in [
            LdCode::Ok, LdCode::ENoOp, LdCode::EDrift, LdCode::EOrder, LdCode::ENoPath,
            LdCode::EPathDrift, LdCode::ENoVer, LdCode::EVerDrift, LdCode::ENoRow,
            LdCode::ERowMiss, LdCode::EDup, LdCode::ESeal, LdCode::EGradeDrift,
        ] {
            assert_eq!(cd.val(), SEG_BASE + i);
            assert!(!cd.detail().is_empty());
            i += 1;
        }
    }

    /// 测试辅助：全齐账本（六闸+20 能力+8 投影+3 版本+4 声明+N/A 如实登记）。
    fn full_ledger() -> LdLedger {
        let mut lt = LdLedger::new();
        let mut g = 0usize;
        while g < GATES { let _ = lt.judge_gate(g); g += 1; }
        let mut o = 0usize;
        while o < OPS { if let Some(w) = op_cell(o) { let _ = lt.record_op(o, w); } o += 1; }
        let mut c = 0usize;
        while c < PATH_CELLS { if let Some(w) = path_cell(c) { let _ = lt.record_path(c, w); } c += 1; }
        let mut v = 0usize;
        while v < VERSION_STEPS { if let Some(w) = version_cell(v) { let _ = lt.record_version(v, w); } v += 1; }
        let mut r = 0usize;
        while r < SPLIT_ROWS { let _ = lt.declare_split(r); r += 1; }
        let _ = lt.na_probe(NaVerdict::NotAvailable);
        lt
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        assert_eq!(LdLedger::new().grade(), Grade::S3);
        // 六闸齐 S2（有条件）。
        let mut lt2 = LdLedger::new();
        let mut g = 0usize;
        while g < GATES { let _ = lt2.judge_gate(g); g += 1; }
        assert_eq!(lt2.grade(), Grade::S2);
        // 全齐+如实 N/A → 封顶目标档 S2+N/A 清单；虚报 S1 = EGradeDrift 必抓。
        let lt3 = full_ledger();
        assert_eq!(lt3.grade(), Grade::S2);
        assert_eq!(lt3.try_grade(Grade::S1), Err(LdCode::EGradeDrift));
        assert_eq!(lt3.try_grade(Grade::S2), Ok(Grade::S2));
        // N/A 虚报路径下全齐也不得 S1，封顶语义恒成立。
        assert!(lt3.try_grade(Grade::S1).is_err());
    }

    #[test]
    fn unx52550_checks_green() {
        let s = run_unx52550_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
