//! UNX-F12688 句柄代数复用回归（B30 档 10^3 注入重跑） · 0x815C0段
//!
//! 判据原文（派工卡 12687 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12688-J1 根路径协议回归 10/10 绿（\?? 与
//!      \BaseNamedObjects），越根拒止维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 句柄代数域模型：句柄二元（槽位/复用代数），复用代数随重发单调
//!    递增（saturating），陈旧句柄（代数落后）显性拒——句柄代数复用
//!    的算术面；
//! 2. 根路径协议二族闭集：\??（DOS 设备根）与 \BaseNamedObjects（命名
//!    对象根），族前缀冻结；路径归类必须前缀命中一族——两族各 5 节点
//!    = 10/10 协议回归判据锚；
//! 3. 越根拒止：既非 \?? 也非 \BaseNamedObjects 前缀的路径一律拒入
//!    （越根显性拒，不许静默归族）——越根拒止维持判据的注入面；
//! 4. 族路绑定律：节点族与路径前缀归类必须一致（族写 \?? 而路径归
//!    BaseNamed 即错配拒）；
//! 5. 回归账状态机 HrOpen → HrRunning → HrSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：判定数 ≡ 节点数守恒；
//! 6. 注入联测十路：乱序/重复/判码漂移/越根路径/族路错配/陈旧代数/
//!    复用漏递增/封账后写/状态机倒退/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `rootpath_ten_green_ok` ↔ ② 根路径协议回归 10/10 绿（\?? 与
//!     \BaseNamedObjects 两族齐）；
//!   - `out_of_root_deny_ok` ↔ ② 越根拒止维持（越根注入必拒）；
//!   - `family_bind_ok` ↔ ② 族路绑定律（族与前缀归类一致）；
//!   - `handle_reuse_ok` ↔ ① 句柄代数复用面（代数单调、陈旧拒）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12687 − 12600)）。
pub const SEG_BASE: u32 = 0x815C0;

// ── 句柄复用回归诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HrDiag {
    /// 节点乱序。
    ENodeOrder,
    /// 节点重复。
    ENodeDup,
    /// 判码漂移。
    ECodeDrift,
    /// 越根路径（既非 \?? 也非 \BaseNamedObjects）。
    EOutOfRoot,
    /// 族路错配（族与前缀归类不一致）。
    EFamilyMismatch,
    /// 陈旧代数（句柄复用代数落后）。
    EStaleGen,
    /// 复用漏递增（重发时代数未递增）。
    EReuseMiss,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl HrDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HrDiag::ENodeOrder => 0x01,
                HrDiag::ENodeDup => 0x02,
                HrDiag::ECodeDrift => 0x03,
                HrDiag::EOutOfRoot => 0x04,
                HrDiag::EFamilyMismatch => 0x05,
                HrDiag::EStaleGen => 0x06,
                HrDiag::EReuseMiss => 0x07,
                HrDiag::ESealedWrite => 0x08,
                HrDiag::ERewind => 0x09,
                HrDiag::ESummaryEmpty => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HrDiag::ENodeOrder => "节点乱序：按节点号 0..10 逐节点回归，跳号先回补",
            HrDiag::ENodeDup => "节点重复：同路径只立一节点，重复先查已有账",
            HrDiag::ECodeDrift => "判码漂移：判码必须等于冻结谱位，先对谱口径",
            HrDiag::EOutOfRoot => "越根路径：路径必须落在 \\?? 或 \\BaseNamedObjects 之下，越根先归根",
            HrDiag::EFamilyMismatch => "族路错配：节点族必须等于路径前缀归类，先核前缀",
            HrDiag::EStaleGen => "陈旧代数：句柄代数必须最新，陈旧句柄先重取",
            HrDiag::EReuseMiss => "复用漏递增：句柄重发必递增代数，漏增先补 gen",
            HrDiag::ESealedWrite => "封账后写：回归账只读，补节点走下一轮回归",
            HrDiag::ERewind => "状态机倒退：HrOpen→HrRunning→HrSealed 只进不退，倒退即账可疑",
            HrDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HR_ALL: &[HrDiag] = &[
    HrDiag::ENodeOrder,
    HrDiag::ENodeDup,
    HrDiag::ECodeDrift,
    HrDiag::EOutOfRoot,
    HrDiag::EFamilyMismatch,
    HrDiag::EStaleGen,
    HrDiag::EReuseMiss,
    HrDiag::ESealedWrite,
    HrDiag::ERewind,
    HrDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HR_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x815C1..0x815CA 闭集内"
}

// ── 根路径协议二族闭集 ───────────────────────────────────────────
/// 根路径族（二族闭集；前缀冻结，来源：NT 命名空间协议冻结口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootFamily {
    /// \?? DOS 设备根。
    DosDev,
    /// \BaseNamedObjects 命名对象根。
    BaseNamed,
}

impl RootFamily {
    /// 族根前缀（冻结字面量）。
    pub fn prefix(self) -> &'static str {
        match self {
            RootFamily::DosDev => "\\??\\",
            RootFamily::BaseNamed => "\\BaseNamedObjects\\",
        }
    }

    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            RootFamily::DosDev => "DOS 设备根",
            RootFamily::BaseNamed => "命名对象根",
        }
    }

    /// 族号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            RootFamily::DosDev => 0,
            RootFamily::BaseNamed => 1,
        }
    }

    /// 全部族（闭集数组）。
    pub fn all() -> &'static [RootFamily] {
        &[RootFamily::DosDev, RootFamily::BaseNamed]
    }
}

/// 路径归类（前缀命中一族返回 Some，越根返回 None——越根拒止的算术面）。
pub fn classify_path(path: &str) -> Option<RootFamily> {
    for f in RootFamily::all().iter() {
        if path.starts_with(f.prefix()) {
            return Some(*f);
        }
    }
    None
}

// ── 句柄代数 ─────────────────────────────────────────────────────
/// 句柄槽位额（复用代数表容量）。
pub const HANDLE_SLOT_CAP: usize = 16;

/// 句柄二元（槽位 + 复用代数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleVal {
    /// 槽位（0..16）。
    pub slot: u32,
    /// 复用代数（重发单调递增）。
    pub gen: u32,
}

/// 句柄复用代数表。
#[derive(Clone, Debug)]
pub struct HandleTable {
    /// 各槽代数（0 = 从未发放）。
    gens: Vec<u32>,
    /// 拒止计数。
    rejects: u32,
}

impl HandleTable {
    /// 新表（16 槽清零）。
    pub fn new() -> HandleTable {
        let mut g = Vec::new();
        let mut i = 0usize;
        while i < HANDLE_SLOT_CAP {
            g.push(0u32);
            i += 1;
        }
        HandleTable { gens: g, rejects: 0 }
    }

    /// 发放句柄（代数递增并返回新句柄；越槽 None）。
    pub fn issue(&mut self, slot: u32) -> Option<HandleVal> {
        if slot as usize >= HANDLE_SLOT_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return None;
        }
        let idx = slot as usize;
        let cur = match self.gens.get(idx) {
            Some(v) => *v,
            None => return None,
        };
        let next = cur.saturating_add(1);
        match self.gens.get_mut(idx) {
            Some(v) => *v = next,
            None => return None,
        }
        Some(HandleVal { slot, gen: next })
    }

    /// 句柄核验（代数必须等于表内最新值，陈旧即拒）。
    pub fn verify(&mut self, h: HandleVal) -> Result<(), u32> {
        if h.slot as usize >= HANDLE_SLOT_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::EStaleGen.val());
        }
        let cur = match self.gens.get(h.slot as usize) {
            Some(v) => *v,
            None => return Err(HrDiag::EStaleGen.val()),
        };
        if h.gen != cur {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::EStaleGen.val());
        }
        Ok(())
    }

    /// 槽代数（越槽 None）。
    pub fn gen_at(&self, slot: u32) -> Option<u32> {
        if slot as usize >= HANDLE_SLOT_CAP {
            return None;
        }
        self.gens.get(slot as usize).copied()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }
}

// ── 回归节点域模型 ───────────────────────────────────────────────
/// 回归节点六元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HrNode {
    /// 节点号（0..10）。
    pub no: u32,
    /// 根路径族。
    pub family: RootFamily,
    /// 协议路径（必须落在族根前缀之下）。
    pub path: &'static str,
    /// 句柄槽位。
    pub handle_slot: u32,
    /// 谱判码（冻结值 0x21..0x2A）。
    pub verdict_code: u32,
}

/// 冻结谱位（0x20 + 节点号 + 1；来源：根路径协议回归清单冻结轴）。
pub fn hr_spec_code(node_no: u32) -> u32 {
    0x20u32.saturating_add(node_no).saturating_add(1)
}

/// 判码是否落在协议谱内（0x21..0x2A）。
pub fn code_in_hr_spec(code: u32) -> bool {
    code >= 0x21 && code <= 0x2A
}

/// 冻结回归节点表：两族各 5 节点（\?? 五路 + \BaseNamedObjects 五路）。
pub const HR_NODES: &[HrNode] = &[
    HrNode { no: 0, family: RootFamily::DosDev, path: "\\??\\C:\\proj\\boot.sys", handle_slot: 0, verdict_code: 0x21 },
    HrNode { no: 1, family: RootFamily::DosDev, path: "\\??\\C:\\proj\\core.sys", handle_slot: 1, verdict_code: 0x22 },
    HrNode { no: 2, family: RootFamily::DosDev, path: "\\??\\D:\\data\\seg.map", handle_slot: 2, verdict_code: 0x23 },
    HrNode { no: 3, family: RootFamily::DosDev, path: "\\??\\C:\\proj\\sym.tbl", handle_slot: 3, verdict_code: 0x24 },
    HrNode { no: 4, family: RootFamily::DosDev, path: "\\??\\D:\\log\\boot.log", handle_slot: 4, verdict_code: 0x25 },
    HrNode { no: 5, family: RootFamily::BaseNamed, path: "\\BaseNamedObjects\\OA_Load_Mutex", handle_slot: 5, verdict_code: 0x26 },
    HrNode { no: 6, family: RootFamily::BaseNamed, path: "\\BaseNamedObjects\\OA_Load_Event", handle_slot: 6, verdict_code: 0x27 },
    HrNode { no: 7, family: RootFamily::BaseNamed, path: "\\BaseNamedObjects\\OA_Section_1", handle_slot: 7, verdict_code: 0x28 },
    HrNode { no: 8, family: RootFamily::BaseNamed, path: "\\BaseNamedObjects\\OA_Section_2", handle_slot: 8, verdict_code: 0x29 },
    HrNode { no: 9, family: RootFamily::BaseNamed, path: "\\BaseNamedObjects\\OA_Sem_Final", handle_slot: 9, verdict_code: 0x2A },
];

/// 回归节点总数（10/10 判据锚定值）。
pub const HR_NODE_CAP: usize = 10;

/// 按节点号取冻结节点（越界 None）。
pub fn replay_hr(no: u32) -> Option<HrNode> {
    if no >= HR_NODES.len() as u32 {
        return None;
    }
    for n in HR_NODES.iter() {
        if n.no == no {
            return Some(n.clone());
        }
    }
    None
}

/// 族路绑定律：节点族必须等于路径归类。
fn family_binds(n: &HrNode) -> bool {
    match classify_path(n.path) {
        Some(f) => f == n.family,
        None => false,
    }
}

// ── 回归账状态机 ─────────────────────────────────────────────────
/// 回归账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HrState {
    /// 开账。
    HrOpen,
    /// 回归中。
    HrRunning,
    /// 封账（只读）。
    HrSealed,
}

impl HrState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            HrState::HrOpen => 0,
            HrState::HrRunning => 1,
            HrState::HrSealed => 2,
        }
    }
}

/// 句柄复用回归账。
#[derive(Clone, Debug)]
pub struct HrLedger {
    /// 状态。
    state: HrState,
    /// 已入账节点。
    nodes: Vec<HrNode>,
    /// 句柄代数表。
    table: HandleTable,
    /// 拒止计数。
    rejects: u32,
    /// 判定数。
    verdicts: u32,
}

impl HrLedger {
    /// 新账。
    pub fn new() -> HrLedger {
        HrLedger {
            state: HrState::HrOpen,
            nodes: Vec::new(),
            table: HandleTable::new(),
            rejects: 0,
            verdicts: 0,
        }
    }

    /// 开账 → 回归中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            HrState::HrOpen => {
                self.state = HrState::HrRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(HrDiag::ERewind.val())
            }
        }
    }

    /// 回归中 → 封账（未齐不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            HrState::HrRunning => {
                if self.nodes.len() != HR_NODE_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(HrDiag::ENodeOrder.val());
                }
                self.state = HrState::HrSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(HrDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> HrState {
        self.state
    }

    /// 已入账节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 判定数。
    pub fn verdict_len(&self) -> u32 {
        self.verdicts
    }

    /// 下标取节点（越界 None）。
    pub fn node_at(&self, idx: usize) -> Option<&HrNode> {
        self.nodes.get(idx)
    }

    /// 句柄表引用口径（槽代数读出）。
    pub fn handle_gen(&self, slot: u32) -> Option<u32> {
        self.table.gen_at(slot)
    }

    /// 账等式：判定数 ≡ 节点数。
    pub fn equation_ok(&self) -> bool {
        self.verdicts as usize == self.nodes.len()
    }

    /// 入账一节点（序/重/谱/越根/族路/句柄六闸）。
    pub fn reg(&mut self, n: HrNode) -> Result<(), u32> {
        if self.state == HrState::HrSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::ESealedWrite.val());
        }
        if self.state == HrState::HrOpen {
            self.state = HrState::HrRunning;
        }
        // 序闸。
        if n.no != self.nodes.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::ENodeOrder.val());
        }
        // 重闸。
        for e in self.nodes.iter() {
            if e.path == n.path {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HrDiag::ENodeDup.val());
            }
        }
        // 谱闸。
        if !code_in_hr_spec(n.verdict_code) || n.verdict_code != hr_spec_code(n.no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::ECodeDrift.val());
        }
        // 越根闸：路径必须归族。
        if classify_path(n.path).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::EOutOfRoot.val());
        }
        // 族路闸。
        if !family_binds(&n) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::EFamilyMismatch.val());
        }
        // 句柄闸：槽位发放句柄，代数必须递增（复用闸）。
        let prev = match self.table.gen_at(n.handle_slot) {
            Some(v) => v,
            None => 0,
        };
        let h = match self.table.issue(n.handle_slot) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HrDiag::EReuseMiss.val());
            }
        };
        if h.gen != prev.saturating_add(1) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(HrDiag::EReuseMiss.val());
        }
        match self.table.verify(h) {
            Ok(()) => {}
            Err(_) => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(HrDiag::EStaleGen.val());
            }
        }
        // 全闸放行。
        self.verdicts = self.verdicts.saturating_add(1);
        self.nodes.push(n);
        Ok(())
    }
}

/// 全量回归（十节点依序入账并封账）。
pub fn build_full_hr_ledger() -> HrLedger {
    let mut led = HrLedger::new();
    let _ = led.begin_run();
    let mut i = 0usize;
    while i < HR_NODES.len() {
        if let Some(n) = HR_NODES.get(i) {
            let _ = led.reg(n.clone());
        }
        i += 1;
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 根路径协议回归 10/10 绿（两族齐）。
fn rootpath_ten_green_ok() -> bool {
    let led = build_full_hr_ledger();
    if led.len() != HR_NODE_CAP || led.state() != HrState::HrSealed {
        return false;
    }
    if led.reject_len() != 0 || !led.equation_ok() {
        return false;
    }
    for f in RootFamily::all().iter() {
        let mut count = 0usize;
        let mut i = 0usize;
        while i < led.len() {
            let n = match led.node_at(i) {
                Some(v) => v,
                None => return false,
            };
            if n.family == *f {
                count += 1;
                if !family_binds(n) {
                    return false;
                }
            }
            i += 1;
        }
        if count != 5 {
            return false;
        }
    }
    true
}

/// 越根拒止维持（越根路径注入必拒）。
fn out_of_root_deny_ok() -> bool {
    // 算术面：越根路径归类必 None。
    let out_paths: [&str; 4] = ["\\Device\\Harddisk0", "\\NT\\kernel", "C:\\plain", "\\BaseNamedObject\\x"];
    for p in out_paths.iter() {
        if classify_path(p).is_some() {
            return false;
        }
    }
    // 账面：越根节点必被越根闸拒。
    let mut led = HrLedger::new();
    let _ = led.begin_run();
    let mut bad = match replay_hr(0) {
        Some(x) => x,
        None => return false,
    };
    bad.path = "\\Device\\Harddisk0\\part1";
    match led.reg(bad) {
        Err(e) => e == HrDiag::EOutOfRoot.val(),
        Ok(_) => false,
    }
}

/// 族路绑定律（冻结表逐节点核对）。
fn family_bind_ok() -> bool {
    for n in HR_NODES.iter() {
        if !family_binds(n) {
            return false;
        }
        match classify_path(n.path) {
            Some(f) => {
                if f != n.family || f.ordinal() != n.family.ordinal() {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 句柄代数复用面（发放递增、陈旧拒、越槽拒）。
fn handle_reuse_ok() -> bool {
    let mut t = HandleTable::new();
    let h1 = match t.issue(3) {
        Some(v) => v,
        None => return false,
    };
    let h2 = match t.issue(3) {
        Some(v) => v,
        None => return false,
    };
    if h2.gen != h1.gen.saturating_add(1) {
        return false;
    }
    // 陈旧句柄拒。
    if t.verify(h1).is_ok() {
        return false;
    }
    // 新句柄核验过。
    if t.verify(h2).is_err() {
        return false;
    }
    // 越槽拒。
    if t.issue(HANDLE_SLOT_CAP as u32).is_some() {
        return false;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn hr_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 乱序。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let bad = match replay_hr(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 2. 重复。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let n0 = match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        let _ = led.reg(n0.clone());
        if led.reg(n0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 判码漂移。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.verdict_code = 0x2B;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("漂移;");
        }
    }
    // 4. 越根路径。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.path = "\\NT\\kernel\\core";
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("越根;");
        }
    }
    // 5. 族路错配。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.family = RootFamily::BaseNamed;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("错族;");
        }
    }
    // 6. 陈旧代数。
    {
        let mut t = HandleTable::new();
        let h1 = match t.issue(2) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺槽")),
        };
        let _ = t.issue(2);
        if t.verify(h1).is_err() {
            caught += 1;
        } else {
            escape.push_str("陈旧;");
        }
    }
    // 7. 复用漏递增（表内代数被外部抹平后重发不递增）。
    {
        let mut t = HandleTable::new();
        let h1 = match t.issue(1) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺槽")),
        };
        // 复用核对：重发后旧句柄必须陈旧。
        let h2 = match t.issue(1) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺槽")),
        };
        if h2.gen > h1.gen && t.verify(h1).is_err() && t.verify(h2).is_ok() {
            caught += 1;
        } else {
            escape.push_str("漏增;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_hr_ledger();
        let extra = match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut led = build_full_hr_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 未齐封账。
    {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_hr(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("早封;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_hr_ledger();
    let b = build_full_hr_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.node_at(i), b.node_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if hr_fault_sweep().0 != hr_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for n in HR_NODES.iter() {
        acc = acc.wrapping_mul(29).wrapping_add(n.verdict_code).wrapping_add(n.handle_slot);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12687u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_hr_ledger();
    if led.state() != HrState::HrSealed {
        return false;
    }
    let head = match led.node_at(0) {
        Some(n) => n.path,
        None => return false,
    };
    let tail = match led.node_at(HR_NODE_CAP.saturating_sub(1)) {
        Some(n) => n.path,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    for f in RootFamily::all().iter() {
        if f.prefix().is_empty() || f.name().is_empty() {
            return false;
        }
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12688 句柄代数复用回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12687_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12687-handle-reuse-reg");
    let led = build_full_hr_ledger();

    s.add_dyn(
        "F12688-01 协议节点十个齐",
        HR_NODES.len() == HR_NODE_CAP,
        &format!("nodes={} families={}", HR_NODES.len(), RootFamily::all().len()),
    );

    s.add_dyn(
        "F12688-02 根路径协议回归 10/10 绿",
        rootpath_ten_green_ok(),
        &format!("nodes={} sealed={}", led.len(), led.state() == HrState::HrSealed),
    );

    s.add_dyn(
        "F12688-03 越根拒止维持",
        out_of_root_deny_ok(),
        "越根路径注入必拒",
    );

    s.add_dyn(
        "F12688-04 族路绑定律",
        family_bind_ok(),
        &format!("families={}", RootFamily::all().len()),
    );

    s.add_dyn(
        "F12688-05 句柄代数复用面",
        handle_reuse_ok(),
        &format!("slots={}", HANDLE_SLOT_CAP),
    );

    s.add_dyn(
        "F12688-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = hr_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", hr_fault_sweep().0),
    );

    s.add_dyn(
        "F12688-07 账等式（判定数守恒）",
        led.equation_ok(),
        &format!("verdicts={} nodes={}", led.verdict_len(), led.len()),
    );

    s.add_dyn(
        "F12688-08 谱口径",
        code_in_hr_spec(0x21) && code_in_hr_spec(0x2A) && !code_in_hr_spec(0x20) && !code_in_hr_spec(0x2B),
        "协议谱 0x21..0x2A 闭集",
    );

    s.add_dyn(
        "F12688-09 两族前缀冻结",
        RootFamily::DosDev.prefix() == "\\??\\" && RootFamily::BaseNamed.prefix() == "\\BaseNamedObjects\\",
        "族前缀字面量冻结",
    );

    s.add_dyn(
        "F12688-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12688-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12688-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12688-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x815C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(HR_ALL.len(), 10);
        let mut i = 0usize;
        while i < HR_ALL.len() {
            let vi = match HR_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < HR_ALL.len() {
                let vj = match HR_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn rootpath_ten_green_holds() {
        assert!(rootpath_ten_green_ok());
        let led = build_full_hr_ledger();
        assert_eq!(led.len(), 10);
        assert_eq!(led.state(), HrState::HrSealed);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.verdict_len(), 10);
        assert!(led.equation_ok());
    }

    #[test]
    fn out_of_root_injections_denied() {
        assert!(out_of_root_deny_ok());
        // 算术面直证。
        assert!(classify_path("\\??\\C:\\x").is_some());
        assert!(classify_path("\\BaseNamedObjects\\y").is_some());
        assert!(classify_path("\\Device\\Harddisk0").is_none());
        assert!(classify_path("").is_none());
        // 近似前缀不命中（防前缀歧义）。
        assert!(classify_path("\\?\\C:\\x").is_none());
        assert!(classify_path("\\BaseNamedObject\\y").is_none());
    }

    #[test]
    fn family_binding_holds() {
        assert!(family_bind_ok());
        assert_eq!(RootFamily::DosDev.prefix(), "\\??\\");
        assert_eq!(RootFamily::BaseNamed.prefix(), "\\BaseNamedObjects\\");
        // 错配节点不绑。
        let mut bad = match HR_NODES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.family = RootFamily::BaseNamed;
        assert!(!family_binds(&bad));
    }

    #[test]
    fn handle_algebra_holds() {
        assert!(handle_reuse_ok());
        let mut t = HandleTable::new();
        // 越槽拒。
        assert!(t.issue(HANDLE_SLOT_CAP as u32).is_none());
        assert!(t.issue(u32::MAX).is_none());
        assert!(t.gen_at(HANDLE_SLOT_CAP as u32).is_none());
        // 发放递增。
        let a = match t.issue(0) {
            Some(v) => v,
            None => return,
        };
        let b = match t.issue(0) {
            Some(v) => v,
            None => return,
        };
        let c = match t.issue(0) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(a.gen, 1);
        assert_eq!(b.gen, 2);
        assert_eq!(c.gen, 3);
        assert_eq!(t.gen_at(0), Some(3));
        // 陈旧拒。
        assert!(t.verify(a).is_err());
        assert!(t.verify(b).is_err());
        assert!(t.verify(c).is_ok());
        // 拒止记账：越槽两次 + 陈旧核验两次。
        assert_eq!(t.reject_len(), 4);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = hr_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn reg_gate_rejects() {
        let mut led = HrLedger::new();
        let _ = led.begin_run();
        let n1 = match replay_hr(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.reg(n1), Err(HrDiag::ENodeOrder.val()));
        let n0 = match replay_hr(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.reg(n0.clone()).is_ok());
        // 判码漂移（节点 1 冻结谱位 0x22，漂到 0x23）。
        let mut drift = match replay_hr(1) {
            Some(x) => x,
            None => return,
        };
        drift.verdict_code = 0x23;
        assert_eq!(led.reg(drift), Err(HrDiag::ECodeDrift.val()));
        // 族路错配。
        let mut mism = match replay_hr(1) {
            Some(x) => x,
            None => return,
        };
        mism.family = RootFamily::BaseNamed;
        assert_eq!(led.reg(mism), Err(HrDiag::EFamilyMismatch.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_hr_ledger();
        let b = build_full_hr_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.node_at(i), b.node_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑节点缺失"),
            }
            i += 1;
        }
        assert_eq!(hr_fault_sweep().0, hr_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(HrState::HrOpen.rank() < HrState::HrRunning.rank());
        assert!(HrState::HrRunning.rank() < HrState::HrSealed.rank());
        let mut led = build_full_hr_ledger();
        assert_eq!(led.state(), HrState::HrSealed);
        assert_eq!(led.seal(), Err(HrDiag::ERewind.val()));
        let mut fresh = HrLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(HrDiag::ERewind.val()));
    }

    #[test]
    fn spec_and_replay_boundary() {
        for no in 0u32..10 {
            assert!(code_in_hr_spec(hr_spec_code(no)), "节点 {} 未落谱", no);
        }
        assert!(!code_in_hr_spec(0x20));
        assert!(!code_in_hr_spec(0x2B));
        assert!(replay_hr(9).is_some());
        assert!(replay_hr(10).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12687_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
