//! UNX-F12747 RTL_CRITICAL_SECTION 双源漂移压测 · 0x824C0段
//!
//! 判据原文（派工卡 12747 逐字锚定）：
//!   UNX-F12748-J1 联动回归 20/20（B25 注入矩阵×B26 坏表拒止），链上零断。
//!
//! 设计要点：
//! 1. 临界区账本双源镜像：源 A=直接账、源 B=镜像账，登记/进入/离开一律双账同笔
//!    同序写入；任一侧被单独篡改即构成双源漂移，逐字段对账必抓零静默；
//! 2. 账本字段闭集：锁号/持有者/进入计数/等待队列计数/状态/双源视图旗标；可漂移
//!    字段子集 DriftField 五项冻结（持有者/进入计数/等待计数/状态/旗标），越集即拒；
//! 3. 漂移判定器 judge_drift(lock_no) 次序冻结：未登记锁 → ENOENT；重复判定 →
//!    EALREADY；双源视图逐字段不等 → EDRIFT 并指明字段；三闸先后不可换；
//! 4. 进入/离开状态机 Free→Owned→Contended→Free 单调：Contended→Owned 倒退、
//!    Free→Contended 跳变、带等待 Owned→Free 跳级一律显性拒（EREWIND）；
//! 5. 漂移注入器三类：A 侧篡改 / B 侧篡改 / 计数翻转，各 10 轮 LCG 定点注入，
//!    全部必须被判定器抓住并指明被篡字段——20/20 联动回归（A10+B10），
//!    计数翻转 10 轮归 B26 坏表语料；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12747 − 12600)）。
pub const SEG_BASE: u32 = 0x824C0;

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 压测账本锁容量：B25 注入矩阵语料锁号域 0..16（临界区句柄表压测窗口）。
pub const LOCK_CAP: usize = 16;
/// 递归重入上限：对应 RTL_CRITICAL_SECTION 的 RecursionCount 语义压测值。
pub const RECUR_CAP: u32 = 4;
/// 等待队列计数上限：对应 LockCount/等待旗语语义压测值。
pub const WAIT_CAP: u32 = 8;
/// 双源视图旗标全通位型：源 A | 源 B 双账使能（0b11）。
pub const DUAL_FLAG_BOTH: u8 = 0b11;
/// 每类注入轮数（A 侧 10 + B 侧 10 = 判据 20/20；计数翻转另计 10 轮）。
pub const INJ_ROUNDS: usize = 10;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsDiag {
    /// 未登记锁：锁号不在账本闭集内。
    ENoent,
    /// 锁号重复登记。
    EDup,
    /// 容量耗尽：登记表满或计数越上限。
    ECap,
    /// 状态机倒退：Free→Owned→Contended→Free 只进不退。
    ERewind,
    /// 持有者不符：无主可离或非持有者代离。
    EOwner,
    /// 双源视图旗标非法：零旗标或越 0b11 位域。
    EFlag,
    /// 双源漂移：源 A 与源 B 逐字段不等。
    EDrift,
    /// 重复判定：同一锁已判定过，不许重判。
    EAlready,
    /// 注入逃脱：篡改未被判定器抓住，判定面失效。
    ETamper,
    /// 账本已冻结：冻结后一切写入拒绝。
    EFrozen,
}

impl CsDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CsDiag::ENoent => 0x01,
                CsDiag::EDup => 0x02,
                CsDiag::ECap => 0x03,
                CsDiag::ERewind => 0x04,
                CsDiag::EOwner => 0x05,
                CsDiag::EFlag => 0x06,
                CsDiag::EDrift => 0x07,
                CsDiag::EAlready => 0x08,
                CsDiag::ETamper => 0x09,
                CsDiag::EFrozen => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CsDiag::ENoent => "未登记锁：锁号不在账本闭集内，先 register 留账再进入/判定",
            CsDiag::EDup => "锁号重复：每锁只落一笔，重复登记先查 find_index 已有锁号",
            CsDiag::ECap => "容量耗尽：登记表满或重入/等待计数越上限，先释放再申请新额度",
            CsDiag::ERewind => "状态机倒退：Free→Owned→Contended→Free 只进不退，倒退即账本可疑",
            CsDiag::EOwner => "持有者不符：无主可离或非持有者代离，先核对 tid 与持有者账",
            CsDiag::EFlag => "双源视图旗标非法：只认 0b11 全通位型，先改旗标再登记",
            CsDiag::EDrift => "双源漂移：源 A 与源 B 逐字段不等，按发现记录的字段回修被篡侧",
            CsDiag::EAlready => "重复判定：该锁已判定过，重判先清 judged 旗标或换锁号",
            CsDiag::ETamper => "注入逃脱：篡改未被判定器抓住，对账面整表复核并封链排查",
            CsDiag::EFrozen => "账本已冻结：冻结后写入拒绝，改动走下一轮压测滚动",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CD_ALL: &[CsDiag] = &[
    CsDiag::ENoent,
    CsDiag::EDup,
    CsDiag::ECap,
    CsDiag::ERewind,
    CsDiag::EOwner,
    CsDiag::EFlag,
    CsDiag::EDrift,
    CsDiag::EAlready,
    CsDiag::ETamper,
    CsDiag::EFrozen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CD_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x824C1..0x824CA 闭集内"
}

// ── 可漂移字段闭集 ───────────────────────────────────────────────
/// 双源可漂移字段闭集（冻结不许扩：持有者/进入计数/等待计数/状态/旗标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftField {
    /// 持有者字段（源 A 持有者 ≠ 源 B 持有者）。
    DfOwner,
    /// 进入计数字段（递归重入计数两账不等）。
    DfEnter,
    /// 等待队列计数字段（等待者数两账不等）。
    DfWait,
    /// 状态字段（Free/Owned/Contended 两账不等）。
    DfState,
    /// 双源视图旗标字段（位型两账不等）。
    DfFlag,
}

impl DriftField {
    /// 字段序（闭集序 0..4）。
    pub fn index(self) -> u32 {
        match self {
            DriftField::DfOwner => 0,
            DriftField::DfEnter => 1,
            DriftField::DfWait => 2,
            DriftField::DfState => 3,
            DriftField::DfFlag => 4,
        }
    }

    /// 字段中文名。
    pub fn name(self) -> &'static str {
        match self {
            DriftField::DfOwner => "持有者",
            DriftField::DfEnter => "进入计数",
            DriftField::DfWait => "等待计数",
            DriftField::DfState => "状态",
            DriftField::DfFlag => "旗标",
        }
    }

    /// 由序号反查闭集字段（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<DriftField> {
        match idx {
            0 => Some(DriftField::DfOwner),
            1 => Some(DriftField::DfEnter),
            2 => Some(DriftField::DfWait),
            3 => Some(DriftField::DfState),
            4 => Some(DriftField::DfFlag),
            _ => None,
        }
    }
}

/// 可漂移字段闭集数组（序即判定序）。
pub const DRIFT_FIELD_ALL: &[DriftField] = &[
    DriftField::DfOwner,
    DriftField::DfEnter,
    DriftField::DfWait,
    DriftField::DfState,
    DriftField::DfFlag,
];

// ── 账本两侧与计数选择闭集 ───────────────────────────────────────
/// 双源视图侧别闭集：源 A=直接账，源 B=镜像账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 源 A：直接账。
    SrcA,
    /// 源 B：镜像账。
    SrcB,
}

impl Side {
    /// 侧别中文名。
    pub fn name(self) -> &'static str {
        match self {
            Side::SrcA => "A直接账",
            Side::SrcB => "B镜像账",
        }
    }
}

/// 计数选择闭集：进入计数 / 等待队列计数（计数翻转注入器语料）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountSel {
    /// 进入计数（递归重入计数）。
    CntEnter,
    /// 等待队列计数。
    CntWait,
}

// ── 临界区状态机 ─────────────────────────────────────────────────
/// 临界区状态机：Free→Owned→Contended→Free 单调，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsState {
    /// 自由态：无持有者无等待者。
    Free,
    /// 持有态：持有者在位，无等待者。
    Owned,
    /// 竞争态：持有者在位且有等待者。
    Contended,
}

impl CsState {
    /// 状态序（单调推进参照）。
    pub fn rank(self) -> u8 {
        match self {
            CsState::Free => 0,
            CsState::Owned => 1,
            CsState::Contended => 2,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            CsState::Free => "自由",
            CsState::Owned => "持有",
            CsState::Contended => "竞争",
        }
    }
}

/// 状态转移表闭集校验：合法集 = 同态 / Free→Owned / Owned→Contended /
/// Owned→Free（仅零等待）/ Contended→Free；其余一律 EREWIND。
/// `wait` 为当前等待队列计数，供 Owned→Free 跳级判定。
pub fn checked_transit(cur: CsState, next: CsState, wait: u32) -> Result<(), u32> {
    if cur == next {
        return Ok(());
    }
    let legal = match (cur, next) {
        (CsState::Free, CsState::Owned) => true,
        (CsState::Owned, CsState::Contended) => true,
        (CsState::Owned, CsState::Free) => wait == 0,
        (CsState::Contended, CsState::Free) => true,
        _ => false,
    };
    if legal {
        Ok(())
    } else {
        Err(CsDiag::ERewind.val())
    }
}

// ── 临界区账本单条记录 ───────────────────────────────────────────
/// 临界区账本单条记录：锁号/持有者/进入计数/等待队列计数/状态/双源视图旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsLock {
    /// 锁号（账本闭集内唯一）。
    pub lock_no: u32,
    /// 持有者线程号（0 = 无主）。
    pub owner: u32,
    /// 进入计数（递归重入计数）。
    pub enter_count: u32,
    /// 等待队列计数。
    pub wait_count: u32,
    /// 状态机当前态。
    pub state: CsState,
    /// 双源视图旗标（0b11 = 双账使能）。
    pub dual_flag: u8,
}

impl CsLock {
    /// 取可漂移字段的标量值（对账用；状态折算为 rank）。
    fn field_val(&self, f: DriftField) -> u32 {
        match f {
            DriftField::DfOwner => self.owner,
            DriftField::DfEnter => self.enter_count,
            DriftField::DfWait => self.wait_count,
            DriftField::DfState => self.state.rank() as u32,
            DriftField::DfFlag => self.dual_flag as u32,
        }
    }
}

/// 漂移发现记录：判定器抓住漂移时指明锁号、字段与两侧取值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftFinding {
    /// 漂移锁号。
    pub lock_no: u32,
    /// 漂移字段（闭集）。
    pub field: DriftField,
    /// 源 A（直接账）取值。
    pub val_a: u32,
    /// 源 B（镜像账）取值。
    pub val_b: u32,
}

impl DriftFinding {
    /// 发现记录摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "锁{}字段{}：A={} B={}",
            self.lock_no,
            self.field.name(),
            self.val_a,
            self.val_b
        )
    }
}

/// 对账结果行：双源零漂移时返回的直账快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftRow {
    /// 锁号。
    pub lock_no: u32,
    /// 持有者。
    pub owner: u32,
    /// 进入计数。
    pub enter_count: u32,
    /// 等待队列计数。
    pub wait_count: u32,
    /// 状态。
    pub state: CsState,
    /// 双源视图旗标。
    pub flag: u8,
}

impl DriftRow {
    /// 快照摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "锁{} 持{} 进{} 等{} 态{} 旗{:02x}",
            self.lock_no,
            self.owner,
            self.enter_count,
            self.wait_count,
            self.state.name(),
            self.flag
        )
    }
}

// ── 临界区双源账本 ───────────────────────────────────────────────
/// 临界区双源账本：源 A=直接账、源 B=镜像账，同序同笔写入；
/// 判定器逐字段对账，任一侧被单独篡改即构成漂移并告警。
#[derive(Clone, Debug)]
pub struct CdLedger {
    /// 源 A：直接账（按下标与镜像账同序对齐）。
    direct: Vec<CsLock>,
    /// 源 B：镜像账。
    mirror: Vec<CsLock>,
    /// 已判定旗标（与 direct 同序）。
    judged: Vec<bool>,
    /// 漂移发现记录（判定器抓到的漂移流水）。
    findings: Vec<DriftFinding>,
    /// 冻结旗标：冻结后一切写入拒绝。
    frozen: bool,
}

/// 0/1 翻转：零翻一、非零翻零（计数翻转注入器核心算子）。
fn flip01(v: u32) -> u32 {
    if v == 0 {
        1
    } else {
        0
    }
}

/// FNV-1a 32 位单步（指纹压缩函数，偏移基/素数取标准 FNV 常量）。
fn fnv_step(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 单步进入推演（纯函数：直接账/镜像账各推一步，双步皆成才同步落账）。
fn cs_enter_step(e: &CsLock, tid: u32) -> Result<CsLock, u32> {
    match e.state {
        CsState::Free => {
            // 坏表闸：自由态却带等待者——账本矛盾。
            if e.wait_count != 0 {
                return Err(CsDiag::ERewind.val());
            }
            checked_transit(CsState::Free, CsState::Owned, e.wait_count)?;
            Ok(CsLock {
                lock_no: e.lock_no,
                owner: tid,
                enter_count: 1,
                wait_count: e.wait_count,
                state: CsState::Owned,
                dual_flag: e.dual_flag,
            })
        }
        CsState::Owned | CsState::Contended => {
            // 坏表闸：竞争/持有态却无主——账本矛盾。
            if e.owner == 0 {
                return Err(CsDiag::ERewind.val());
            }
            if tid == e.owner {
                // 递归重入：进入计数 +1（上限 RECUR_CAP，越限显性拒）。
                let next = match e.enter_count.checked_add(1) {
                    Some(n) => n,
                    None => return Err(CsDiag::ECap.val()),
                };
                if next > RECUR_CAP {
                    return Err(CsDiag::ECap.val());
                }
                Ok(CsLock {
                    enter_count: next,
                    ..*e
                })
            } else {
                // 他人争用：等待队列计数 +1，Owned 单调推进到 Contended。
                let next = match e.wait_count.checked_add(1) {
                    Some(n) => n,
                    None => return Err(CsDiag::ECap.val()),
                };
                if next > WAIT_CAP {
                    return Err(CsDiag::ECap.val());
                }
                checked_transit(e.state, CsState::Contended, e.wait_count)?;
                Ok(CsLock {
                    wait_count: next,
                    state: CsState::Contended,
                    ..*e
                })
            }
        }
    }
}

/// 单步离开推演（纯函数：直接账/镜像账各推一步，双步皆成才同步落账）。
fn cs_leave_step(e: &CsLock, tid: u32) -> Result<CsLock, u32> {
    match e.state {
        CsState::Free => {
            // 坏表闸：自由态却带主/带计数——账本矛盾。
            if e.owner != 0 || e.enter_count != 0 {
                return Err(CsDiag::ERewind.val());
            }
            Err(CsDiag::EOwner.val())
        }
        CsState::Owned => {
            if tid != e.owner {
                return Err(CsDiag::EOwner.val());
            }
            if e.enter_count > 1 {
                // 递归退出：进入计数 −1，态不变。
                let next = match e.enter_count.checked_sub(1) {
                    Some(n) => n,
                    None => return Err(CsDiag::ERewind.val()),
                };
                Ok(CsLock {
                    enter_count: next,
                    ..*e
                })
            } else {
                // 终释：Owned→Free 仅当零等待（带等待跳级属倒退，显性拒）。
                checked_transit(CsState::Owned, CsState::Free, e.wait_count)?;
                Ok(CsLock {
                    owner: 0,
                    enter_count: 0,
                    wait_count: e.wait_count,
                    state: CsState::Free,
                    ..*e
                })
            }
        }
        CsState::Contended => {
            // 坏表闸：竞争态却无主——账本矛盾。
            if e.owner == 0 {
                return Err(CsDiag::ERewind.val());
            }
            if tid != e.owner {
                return Err(CsDiag::EOwner.val());
            }
            if e.enter_count > 1 {
                let next = match e.enter_count.checked_sub(1) {
                    Some(n) => n,
                    None => return Err(CsDiag::ERewind.val()),
                };
                Ok(CsLock {
                    enter_count: next,
                    ..*e
                })
            } else {
                // 坏表闸：竞争态却零等待——账本矛盾（应已 Free）。
                if e.wait_count == 0 {
                    return Err(CsDiag::ERewind.val());
                }
                let w = match e.wait_count.checked_sub(1) {
                    Some(n) => n,
                    None => return Err(CsDiag::ERewind.val()),
                };
                if w == 0 {
                    // 等待耗尽：Contended→Free 交接收尾。
                    checked_transit(CsState::Contended, CsState::Free, w)?;
                    Ok(CsLock {
                        owner: 0,
                        enter_count: 0,
                        wait_count: 0,
                        state: CsState::Free,
                        ..*e
                    })
                } else {
                    // 仍有等待者：保持竞争态，持有者继续运转。
                    Ok(CsLock {
                        wait_count: w,
                        ..*e
                    })
                }
            }
        }
    }
}

impl CdLedger {
    /// 开一本空账。
    pub fn new() -> CdLedger {
        CdLedger {
            direct: Vec::new(),
            mirror: Vec::new(),
            judged: Vec::new(),
            findings: Vec::new(),
            frozen: false,
        }
    }

    /// 已登记锁数。
    pub fn len(&self) -> usize {
        self.direct.len()
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结账本：冻结后 register/enter/leave 一律拒绝（对账不受限）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 已判定锁数。
    pub fn judged_count(&self) -> usize {
        let mut n = 0usize;
        for j in self.judged.iter() {
            if *j {
                n += 1;
            }
        }
        n
    }

    /// 漂移发现记录快照（判定器流水）。
    pub fn findings_snapshot(&self) -> Vec<DriftFinding> {
        self.findings.clone()
    }

    /// 按锁号查直接账（源 A）。
    pub fn entry_a(&self, lock_no: u32) -> Option<CsLock> {
        let idx = self.find_index(lock_no)?;
        self.direct.get(idx).copied()
    }

    /// 按锁号查镜像账（源 B）。
    pub fn entry_b(&self, lock_no: u32) -> Option<CsLock> {
        let idx = self.find_index(lock_no)?;
        self.mirror.get(idx).copied()
    }

    /// 按锁号查直接账下标（线性扫描，无裸下标）。
    fn find_index(&self, lock_no: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.direct.len() {
            match self.direct.get(i) {
                Some(e) if e.lock_no == lock_no => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 登记：双账同笔落锁（默认旗标 0b11 全通）。
    pub fn register(&mut self, lock_no: u32) -> Result<(), u32> {
        self.register_with_flag(lock_no, DUAL_FLAG_BOTH)
    }

    /// 登记带旗标：旗标闭集闸 + 重复闸 + 容量闸 + 冻结闸，四闸过后双账同笔。
    pub fn register_with_flag(&mut self, lock_no: u32, flag: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(CsDiag::EFrozen.val());
        }
        // 旗标闭集闸：只认 0b11 位型内的非零旗标。
        if flag == 0 || (flag & !DUAL_FLAG_BOTH) != 0 {
            return Err(CsDiag::EFlag.val());
        }
        // 重复闸。
        if self.find_index(lock_no).is_some() {
            return Err(CsDiag::EDup.val());
        }
        // 容量闸。
        if self.direct.len() >= LOCK_CAP {
            return Err(CsDiag::ECap.val());
        }
        let e = CsLock {
            lock_no,
            owner: 0,
            enter_count: 0,
            wait_count: 0,
            state: CsState::Free,
            dual_flag: flag,
        };
        self.direct.push(e);
        self.mirror.push(e);
        self.judged.push(false);
        Ok(())
    }

    /// 双账同笔落账（下标对齐写入，越界静默不可能——idx 来自 find_index）。
    fn write_pair(&mut self, idx: usize, a: CsLock, b: CsLock) {
        if let Some(slot) = self.direct.get_mut(idx) {
            *slot = a;
        }
        if let Some(slot) = self.mirror.get_mut(idx) {
            *slot = b;
        }
    }

    /// 进入临界区：双账各推一步，双步皆成才同步落账（原子双写）。
    pub fn enter(&mut self, lock_no: u32, tid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CsDiag::EFrozen.val());
        }
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        let ea = match self.direct.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        let eb = match self.mirror.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        let na = cs_enter_step(&ea, tid)?;
        let nb = cs_enter_step(&eb, tid)?;
        self.write_pair(idx, na, nb);
        Ok(())
    }

    /// 离开临界区：双账各推一步，双步皆成才同步落账（原子双写）。
    pub fn leave(&mut self, lock_no: u32, tid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CsDiag::EFrozen.val());
        }
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        let ea = match self.direct.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        let eb = match self.mirror.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        let na = cs_leave_step(&ea, tid)?;
        let nb = cs_leave_step(&eb, tid)?;
        self.write_pair(idx, na, nb);
        Ok(())
    }

    /// 状态机注入口（压测专用）：绕过转移表直写两侧状态，
    /// 用于构造非法倒退场景（Contended→Owned、Free→Contended）。
    pub fn raw_set_state(&mut self, lock_no: u32, st: CsState) -> Result<(), u32> {
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        if let Some(slot) = self.direct.get_mut(idx) {
            slot.state = st;
        }
        if let Some(slot) = self.mirror.get_mut(idx) {
            slot.state = st;
        }
        Ok(())
    }

    /// 单侧篡改注入：只写指定侧的指定字段，制造双源漂移（压测面）。
    pub fn inject_tamper(&mut self, side: Side, lock_no: u32, field: DriftField) -> Result<(), u32> {
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        let slot: &mut CsLock = match side {
            Side::SrcA => match self.direct.get_mut(idx) {
                Some(e) => e,
                None => return Err(CsDiag::ENoent.val()),
            },
            Side::SrcB => match self.mirror.get_mut(idx) {
                Some(e) => e,
                None => return Err(CsDiag::ENoent.val()),
            },
        };
        match field {
            DriftField::DfOwner => slot.owner ^= 0x5A,
            DriftField::DfEnter => slot.enter_count = flip01(slot.enter_count),
            DriftField::DfWait => slot.wait_count = flip01(slot.wait_count),
            DriftField::DfState => {
                slot.state = match slot.state {
                    CsState::Free => CsState::Owned,
                    CsState::Owned => CsState::Free,
                    CsState::Contended => CsState::Owned,
                };
            }
            DriftField::DfFlag => slot.dual_flag ^= 0b01,
        }
        Ok(())
    }

    /// 计数翻转注入：把指定侧的进入/等待计数 0↔非零 翻转（压测面）。
    pub fn inject_counter_flip(&mut self, side: Side, lock_no: u32, sel: CountSel) -> Result<(), u32> {
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        let slot: &mut CsLock = match side {
            Side::SrcA => match self.direct.get_mut(idx) {
                Some(e) => e,
                None => return Err(CsDiag::ENoent.val()),
            },
            Side::SrcB => match self.mirror.get_mut(idx) {
                Some(e) => e,
                None => return Err(CsDiag::ENoent.val()),
            },
        };
        match sel {
            CountSel::CntEnter => slot.enter_count = flip01(slot.enter_count),
            CountSel::CntWait => slot.wait_count = flip01(slot.wait_count),
        }
        Ok(())
    }

    /// 漂移判定器：次序冻结——
    /// ①未登记锁 → ENOENT；②重复判定 → EALREADY；③双源逐字段不等 →
    /// EDRIFT 并指明字段；全等 → Ok(直账快照行)。
    pub fn judge_drift(&mut self, lock_no: u32) -> Result<DriftRow, u32> {
        // 次序一：未登记锁。
        let idx = match self.find_index(lock_no) {
            Some(i) => i,
            None => return Err(CsDiag::ENoent.val()),
        };
        // 次序二：重复判定（旗标缺位防御性视同已判定，不许重判）。
        let already = match self.judged.get(idx) {
            Some(j) => *j,
            None => true,
        };
        if already {
            return Err(CsDiag::EAlready.val());
        }
        // 次序三：双源视图逐字段对账。
        let ea = match self.direct.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        let eb = match self.mirror.get(idx).copied() {
            Some(e) => e,
            None => return Err(CsDiag::ENoent.val()),
        };
        for f in DRIFT_FIELD_ALL.iter() {
            let va = ea.field_val(*f);
            let vb = eb.field_val(*f);
            if va != vb {
                self.findings.push(DriftFinding {
                    lock_no,
                    field: *f,
                    val_a: va,
                    val_b: vb,
                });
                if let Some(j) = self.judged.get_mut(idx) {
                    *j = true;
                }
                return Err(CsDiag::EDrift.val());
            }
        }
        if let Some(j) = self.judged.get_mut(idx) {
            *j = true;
        }
        Ok(DriftRow {
            lock_no,
            owner: ea.owner,
            enter_count: ea.enter_count,
            wait_count: ea.wait_count,
            state: ea.state,
            flag: ea.dual_flag,
        })
    }

    /// 双跑一致指纹：FNV-1a 压缩直接账/镜像账/判定旗标/冻结旗标全量。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.direct.iter() {
            h = fnv_step(h, e.lock_no);
            h = fnv_step(h, e.owner);
            h = fnv_step(h, e.enter_count);
            h = fnv_step(h, e.wait_count);
            h = fnv_step(h, e.state.rank() as u32);
            h = fnv_step(h, e.dual_flag as u32);
        }
        for e in self.mirror.iter() {
            h = fnv_step(h, e.lock_no);
            h = fnv_step(h, e.owner);
            h = fnv_step(h, e.enter_count);
            h = fnv_step(h, e.wait_count);
            h = fnv_step(h, e.state.rank() as u32);
            h = fnv_step(h, e.dual_flag as u32);
        }
        for j in self.judged.iter() {
            h = fnv_step(h, if *j { 1 } else { 0 });
        }
        h = fnv_step(h, if self.frozen { 1 } else { 0 });
        h
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性；LCG 常数取 Knuth MMIX 推荐值）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 标准语料账本 ─────────────────────────────────────────────────
/// 构造标准语料账本：16 锁全登记，按锁号确定性编排进入/重入/争用/释放，
/// 终态覆盖 Free/Owned/Contended 三态与计数组合，双账同构。
pub fn build_standard_ledger() -> CdLedger {
    let mut led = CdLedger::new();
    let mut no = 0u32;
    while no < LOCK_CAP as u32 {
        let _ = led.register(no);
        let tid = 100u32 + no;
        // 基础进入：Free→Owned。
        let _ = led.enter(no, tid);
        // 每 3 号锁递归重入一次：进入计数 2。
        if no % 3 == 0 {
            let _ = led.enter(no, tid);
        }
        // 每 4 号锁他人争用一次：等待 1，Owned→Contended。
        if no % 4 == 0 {
            let _ = led.enter(no, tid + 0x100);
        }
        // 每 5 余 1 号锁部分释放一次（0..16 内与 4 号位不重叠，无跳级风险）。
        if no % 5 == 1 {
            let _ = led.leave(no, tid);
        }
        no += 1;
    }
    led
}

// ── 注入面：三类注入器 × 10 轮 ──────────────────────────────────
/// 单轮注入扫描报告：抓获数/总轮数/首个逃脱描述（空串 = 零逃脱）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepReport {
    /// 被判定器抓住的轮数。
    pub caught: usize,
    /// 注入总轮数。
    pub total: usize,
    /// 首个逃脱轮描述（空串 = 零静默）。
    pub escape: String,
}

/// 单侧篡改扫描（源 A 或源 B）：10 轮，字段按闭集轮转，锁号 LCG 定点；
/// 每轮注入后判定器必须报 EDRIFT 且指明被篡字段，否则记逃脱。
pub fn sweep_side(side: Side) -> SweepReport {
    let seed = match side {
        Side::SrcA => SEG_BASE.wrapping_add(0x11),
        Side::SrcB => SEG_BASE.wrapping_add(0x22),
    } as u64;
    let mut rng = Lcg::new(seed);
    let mut rep = SweepReport {
        caught: 0,
        total: 0,
        escape: String::new(),
    };
    let mut k = 0usize;
    while k < INJ_ROUNDS {
        let field = match DRIFT_FIELD_ALL.get(k % DRIFT_FIELD_ALL.len()) {
            Some(f) => *f,
            None => DriftField::DfOwner,
        };
        let lock_pick = rng.below(LOCK_CAP as u32);
        let mut led = build_standard_ledger();
        let _ = led.inject_tamper(side, lock_pick, field);
        let verdict = led.judge_drift(lock_pick);
        rep.total += 1;
        let hit = match verdict {
            Err(e) => e == CsDiag::EDrift.val(),
            Ok(_) => false,
        };
        let field_named = led
            .findings_snapshot()
            .iter()
            .any(|f| f.lock_no == lock_pick && f.field == field);
        if hit && field_named {
            rep.caught += 1;
        } else if rep.escape.is_empty() {
            rep.escape = format!("轮{}锁{}字段{}逃脱({})", k, lock_pick, field.name(), side.name());
        }
        k += 1;
    }
    rep
}

/// 计数翻转扫描：10 轮，进入/等待计数轮转 + 侧别轮转；
/// 每轮注入后判定器必须报 EDRIFT 且指明对应计数字段，否则记逃脱。
pub fn sweep_counter_flip() -> SweepReport {
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut rep = SweepReport {
        caught: 0,
        total: 0,
        escape: String::new(),
    };
    let mut k = 0usize;
    while k < INJ_ROUNDS {
        let sel = match k % 2 {
            0 => CountSel::CntEnter,
            _ => CountSel::CntWait,
        };
        let side = match k % 3 {
            0 => Side::SrcA,
            _ => Side::SrcB,
        };
        let field = match sel {
            CountSel::CntEnter => DriftField::DfEnter,
            CountSel::CntWait => DriftField::DfWait,
        };
        let lock_pick = rng.below(LOCK_CAP as u32);
        let mut led = build_standard_ledger();
        let _ = led.inject_counter_flip(side, lock_pick, sel);
        let verdict = led.judge_drift(lock_pick);
        rep.total += 1;
        let hit = match verdict {
            Err(e) => e == CsDiag::EDrift.val(),
            Ok(_) => false,
        };
        let field_named = led
            .findings_snapshot()
            .iter()
            .any(|f| f.lock_no == lock_pick && f.field == field);
        if hit && field_named {
            rep.caught += 1;
        } else if rep.escape.is_empty() {
            rep.escape = format!("轮{}锁{}{}翻转逃脱({})", k, lock_pick, field.name(), side.name());
        }
        k += 1;
    }
    rep
}

/// 单侧扫描裁决：10/10 全抓零逃脱，否则回 ETAMPER（注入逃脱告警码）。
fn side_sweep_verdict(side: Side) -> Result<SweepReport, u32> {
    let r = sweep_side(side);
    if r.caught != INJ_ROUNDS || r.total != INJ_ROUNDS || !r.escape.is_empty() {
        return Err(CsDiag::ETamper.val());
    }
    Ok(r)
}

/// 计数翻转扫描裁决：10/10 全抓零逃脱，否则回 ETAMPER。
fn flip_sweep_verdict() -> Result<SweepReport, u32> {
    let r = sweep_counter_flip();
    if r.caught != INJ_ROUNDS || r.total != INJ_ROUNDS || !r.escape.is_empty() {
        return Err(CsDiag::ETamper.val());
    }
    Ok(r)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件，末行为成立条件） ──
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 全窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：10 码互异落窗，查表命中已知码、回落未知码。
fn diag_closed_ok() -> bool {
    if CD_ALL.len() != 10 {
        return false;
    }
    for d in CD_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(CsDiag::ENoent.val()) != CsDiag::ENoent.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == CsDiag::ENoent.detail() {
        return false;
    }
    true
}

/// 漂移字段闭集守卫：5 字段序号/名称/反查三方一致。
fn drift_field_closed_ok() -> bool {
    if DRIFT_FIELD_ALL.len() != 5 {
        return false;
    }
    let mut i = 0usize;
    while i < DRIFT_FIELD_ALL.len() {
        let f = match DRIFT_FIELD_ALL.get(i) {
            Some(f) => *f,
            None => return false,
        };
        if f.index() as usize != i {
            return false;
        }
        if f.name().is_empty() {
            return false;
        }
        match DriftField::from_index(f.index()) {
            Some(g) => {
                if g != f {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 正常流：Free→Owned→递归→争用→递归退出→交接耗尽→Free，全程单调。
fn happy_path_ok() -> bool {
    let mut l = CdLedger::new();
    if l.register(0).is_err() {
        return false;
    }
    // Free→Owned。
    if l.enter(0, 900).is_err() {
        return false;
    }
    match l.entry_a(0) {
        Some(e) => {
            if e.state != CsState::Owned || e.enter_count != 1 || e.owner != 900 {
                return false;
            }
        }
        None => return false,
    }
    // 递归重入：计数 2，态不变。
    if l.enter(0, 900).is_err() {
        return false;
    }
    // 他人争用：Owned→Contended，等待 1。
    if l.enter(0, 901).is_err() {
        return false;
    }
    match l.entry_a(0) {
        Some(e) => {
            if e.state != CsState::Contended || e.enter_count != 2 || e.wait_count != 1 {
                return false;
            }
        }
        None => return false,
    }
    // 双账同笔：两侧逐字段一致。
    if l.entry_a(0) != l.entry_b(0) {
        return false;
    }
    // 递归退出：计数 2→1，态不变。
    if l.leave(0, 900).is_err() {
        return false;
    }
    match l.entry_a(0) {
        Some(e) => {
            if e.enter_count != 1 || e.state != CsState::Contended {
                return false;
            }
        }
        None => return false,
    }
    // 交接耗尽：Contended→Free，账面清零。
    if l.leave(0, 900).is_err() {
        return false;
    }
    match l.entry_a(0) {
        Some(e) => {
            if e.state != CsState::Free || e.owner != 0 || e.wait_count != 0 {
                return false;
            }
        }
        None => return false,
    }
    // 对账零漂移收口。
    match l.judge_drift(0) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// 双源对账零漂移：标准语料 16 锁全判全绿，双账逐锁同构。
fn clean_judge_ok() -> bool {
    let mut led = build_standard_ledger();
    let mut no = 0u32;
    while no < LOCK_CAP as u32 {
        if led.entry_a(no) != led.entry_b(no) {
            return false;
        }
        if led.judge_drift(no).is_err() {
            return false;
        }
        no += 1;
    }
    led.judged_count() == LOCK_CAP
}

/// 判定次序三闸：ENOENT 先于 EALREADY，EALREADY 先于 EDRIFT 再比对。
fn judge_order_ok() -> bool {
    let mut led = build_standard_ledger();
    // 已有已判定锁的前提下判未知锁：仍 ENOENT（次序一先于次序二）。
    if led.judge_drift(0).is_err() {
        return false;
    }
    if led.judge_drift(0xFFFF) != Err(CsDiag::ENoent.val()) {
        return false;
    }
    // 已判定锁重判：EALREADY（次序二先于漂移再比对）。
    if led.judge_drift(0) != Err(CsDiag::EAlready.val()) {
        return false;
    }
    true
}

/// 判定次序补闸：单侧篡改后首判 EDRIFT、再判 EALREADY（漂移仍在不再重报）。
fn judge_order_drift_already_ok() -> bool {
    let mut led = CdLedger::new();
    let _ = led.register(7);
    if led.inject_tamper(Side::SrcA, 7, DriftField::DfOwner).is_err() {
        return false;
    }
    // 首判：漂移成立并指明字段。
    if led.judge_drift(7) != Err(CsDiag::EDrift.val()) {
        return false;
    }
    // 再判：EALREADY 先于漂移再比对（次序二）。
    if led.judge_drift(7) != Err(CsDiag::EAlready.val()) {
        return false;
    }
    // 发现记录指明被篡字段与锁号。
    let fs = led.findings_snapshot();
    if fs.len() != 1 {
        return false;
    }
    match fs.first() {
        Some(f) => f.lock_no == 7 && f.field == DriftField::DfOwner,
        None => false,
    }
}

/// 坏表拒止（B26 语料）：重复/旗标非法/未知锁进出/无主或代离 全数拒。
fn bad_table_reject_ok() -> bool {
    let mut l = CdLedger::new();
    if l.register(0).is_err() {
        return false;
    }
    // 坏表一：重复锁号。
    if l.register(0) != Err(CsDiag::EDup.val()) {
        return false;
    }
    // 坏表二：旗标非法（零旗标 / 越 0b11 位域）。
    if l.register_with_flag(1, 0) != Err(CsDiag::EFlag.val()) {
        return false;
    }
    if l.register_with_flag(1, 0b100) != Err(CsDiag::EFlag.val()) {
        return false;
    }
    // 坏表三：未知锁进入/离开。
    if l.enter(0xFFFF, 1) != Err(CsDiag::ENoent.val()) {
        return false;
    }
    if l.leave(0xFFFF, 1) != Err(CsDiag::ENoent.val()) {
        return false;
    }
    // 坏表四：无主可离 / 非持有者代离。
    let mut l2 = CdLedger::new();
    let _ = l2.register(4);
    if l2.leave(4, 9) != Err(CsDiag::EOwner.val()) {
        return false;
    }
    let _ = l2.enter(4, 9);
    if l2.leave(4, 10) != Err(CsDiag::EOwner.val()) {
        return false;
    }
    true
}

/// 状态机倒退拒绝：转移表闭集逐一校验 + 两个注入场景显性拒。
fn rewind_ok() -> bool {
    // 转移表闭集：倒退与跳变全拒。
    if checked_transit(CsState::Contended, CsState::Owned, 0).is_ok() {
        return false;
    }
    if checked_transit(CsState::Free, CsState::Contended, 0).is_ok() {
        return false;
    }
    if checked_transit(CsState::Owned, CsState::Free, 1).is_ok() {
        return false;
    }
    // 合法链全通（单调）。
    if checked_transit(CsState::Free, CsState::Owned, 0).is_err() {
        return false;
    }
    if checked_transit(CsState::Owned, CsState::Contended, 0).is_err() {
        return false;
    }
    if checked_transit(CsState::Contended, CsState::Free, 0).is_err() {
        return false;
    }
    if checked_transit(CsState::Owned, CsState::Free, 0).is_err() {
        return false;
    }
    // 场景一：带等待强写 Owned 后离开 → 跳级倒退拒。
    let mut l = CdLedger::new();
    let _ = l.register(2);
    let _ = l.enter(2, 21);
    let _ = l.enter(2, 22);
    if l.raw_set_state(2, CsState::Owned).is_err() {
        return false;
    }
    if l.leave(2, 21) != Err(CsDiag::ERewind.val()) {
        return false;
    }
    // 场景二：Free 强写 Contended（无主竞争态矛盾）→ 进出皆拒。
    let mut l2 = CdLedger::new();
    let _ = l2.register(3);
    if l2.raw_set_state(3, CsState::Contended).is_err() {
        return false;
    }
    if l2.enter(3, 31) != Err(CsDiag::ERewind.val()) {
        return false;
    }
    if l2.leave(3, 31) != Err(CsDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 双跑一致：两次构建指纹相同；同一注入+判定序列后仍逐项一致。
fn dual_run_ok() -> bool {
    let a = build_standard_ledger();
    let b = build_standard_ledger();
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    if a.len() != b.len() || a.judged_count() != b.judged_count() {
        return false;
    }
    let mut i = 0usize;
    while i < LOCK_CAP {
        let no = i as u32;
        if a.entry_a(no) != b.entry_a(no) {
            return false;
        }
        if a.entry_b(no) != b.entry_b(no) {
            return false;
        }
        i += 1;
    }
    // 同一注入与判定序列后仍一致。
    let mut a2 = build_standard_ledger();
    let mut b2 = build_standard_ledger();
    let mut k = 0u32;
    while k < 10 {
        let lock = (k * 3) % (LOCK_CAP as u32);
        let field = match DRIFT_FIELD_ALL.get((k % 5) as usize) {
            Some(f) => *f,
            None => DriftField::DfOwner,
        };
        let _ = a2.inject_tamper(Side::SrcA, lock, field);
        let _ = b2.inject_tamper(Side::SrcA, lock, field);
        let _ = a2.judge_drift(lock);
        let _ = b2.judge_drift(lock);
        k += 1;
    }
    if a2.fingerprint() != b2.fingerprint() {
        return false;
    }
    if a2.findings_snapshot() != b2.findings_snapshot() {
        return false;
    }
    true
}

/// 边界四点：首锁 0 / 尾锁 15 / 重复判定 EALREADY / 冻结后写入拒绝。
fn boundary_ok() -> bool {
    // 边界一：首锁 0 可登记可判定。
    let mut l1 = CdLedger::new();
    if l1.register(0).is_err() {
        return false;
    }
    if l1.enter(0, 7).is_err() {
        return false;
    }
    if l1.judge_drift(0).is_err() {
        return false;
    }
    // 边界二：尾锁 15 可登记可判定。
    let mut l2 = CdLedger::new();
    if l2.register(15).is_err() {
        return false;
    }
    if l2.enter(15, 8).is_err() {
        return false;
    }
    if l2.judge_drift(15).is_err() {
        return false;
    }
    // 边界三：重复判定 EALREADY。
    if l2.judge_drift(15) != Err(CsDiag::EAlready.val()) {
        return false;
    }
    // 边界四：冻结后 register/enter/leave 全拒且账面不变。
    l2.freeze();
    if !l2.is_frozen() {
        return false;
    }
    let before = l2.len();
    if l2.register(1) != Err(CsDiag::EFrozen.val()) {
        return false;
    }
    if l2.enter(15, 8) != Err(CsDiag::EFrozen.val()) {
        return false;
    }
    if l2.leave(15, 8) != Err(CsDiag::EFrozen.val()) {
        return false;
    }
    if l2.len() != before {
        return false;
    }
    true
}

/// 容量耗尽：登记表满 / 递归计数上限 / 等待队列上限 三路全拒。
fn cap_ok() -> bool {
    // 登记表满：第 17 把锁拒。
    let mut l = CdLedger::new();
    let mut no = 0u32;
    while no < LOCK_CAP as u32 {
        if l.register(no).is_err() {
            return false;
        }
        no += 1;
    }
    if l.register(0x40) != Err(CsDiag::ECap.val()) {
        return false;
    }
    // 递归计数上限：第 RECUR_CAP+1 次重入拒。
    let mut l2 = CdLedger::new();
    let _ = l2.register(0);
    let mut r = 0u32;
    while r < RECUR_CAP {
        if l2.enter(0, 5).is_err() {
            return false;
        }
        r += 1;
    }
    if l2.enter(0, 5) != Err(CsDiag::ECap.val()) {
        return false;
    }
    // 等待队列上限：第 WAIT_CAP+1 个等待者拒。
    let mut l3 = CdLedger::new();
    let _ = l3.register(0);
    if l3.enter(0, 1).is_err() {
        return false;
    }
    let mut w = 0u32;
    while w < WAIT_CAP {
        if l3.enter(0, 10 + w).is_err() {
            return false;
        }
        w += 1;
    }
    if l3.enter(0, 99) != Err(CsDiag::ECap.val()) {
        return false;
    }
    true
}

/// 判据 J1 联动回归 20/20：A 侧 10 + B 侧 10 全抓零逃脱（B25 注入矩阵），
/// 且计数翻转 10 轮与坏表拒止面全绿（B26 语料），链上零断。
fn criteria_2020_ok() -> bool {
    let a = side_sweep_verdict(Side::SrcA);
    let b = side_sweep_verdict(Side::SrcB);
    match (a, b) {
        (Ok(ra), Ok(rb)) => {
            if ra.caught + rb.caught != 20 || ra.total + rb.total != 20 {
                return false;
            }
            flip_sweep_verdict().is_ok() && bad_table_reject_ok()
        }
        _ => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12748 自检（判据 12747 卡：双源漂移压测，联动回归 20/20 链上零断）。
pub fn run_unx12747_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12747");
    let mut led = build_standard_ledger();

    s.add_dyn(
        "F12748-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} 窗口[0x80000,0x831C0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    s.add_dyn(
        "F12748-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            CD_ALL.len(),
            CD_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12748-03 漂移字段闭集",
        drift_field_closed_ok(),
        &format!(
            "fields={} 首={} 尾={}",
            DRIFT_FIELD_ALL.len(),
            DRIFT_FIELD_ALL.first().map(|f| f.name()).unwrap_or("?"),
            DRIFT_FIELD_ALL.last().map(|f| f.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12748-04 正常流状态机",
        happy_path_ok(),
        &format!("locks={} 转移Free→Owned→Contended→Free单调", led.len()),
    );

    // 在标准语料上逐锁判定：全绿即双源同构零漂移。
    let mut judged_green = 0usize;
    let mut no = 0u32;
    while no < LOCK_CAP as u32 {
        if led.judge_drift(no).is_ok() {
            judged_green += 1;
        }
        no += 1;
    }
    s.add_dyn(
        "F12748-05 双源对账零漂移",
        judged_green == LOCK_CAP && clean_judge_ok(),
        &format!(
            "green={}/{} judged={}/{}",
            judged_green,
            LOCK_CAP,
            led.judged_count(),
            LOCK_CAP
        ),
    );

    // 判定次序样证：零漂移判得快照行，单侧篡改判得指明字段的发现记录。
    let mut lo = CdLedger::new();
    let _ = lo.register(1);
    let _ = lo.enter(1, 42);
    let row_text = match lo.judge_drift(1) {
        Ok(r) => r.brief(),
        Err(_) => String::from("?"),
    };
    let mut lt = CdLedger::new();
    let _ = lt.register(2);
    let _ = lt.enter(2, 50);
    let _ = lt.inject_tamper(Side::SrcB, 2, DriftField::DfWait);
    let find_text = match lt.judge_drift(2) {
        Ok(_) => String::from("?"),
        Err(_) => match lt.findings_snapshot().first() {
            Some(f) => f.brief(),
            None => String::from("?"),
        },
    };
    s.add_dyn(
        "F12748-06 判定次序三闸",
        judge_order_ok() && judge_order_drift_already_ok(),
        &format!("{}|{}", row_text, find_text),
    );

    // 注入必抓：A 侧篡改 10 轮，人为构造漂移必被判定器抓住。
    let va = side_sweep_verdict(Side::SrcA);
    let va_text = match &va {
        Ok(r) => format!("caught={}/{} 零逃脱", r.caught, r.total),
        Err(e) => format!("逃脱 码=0x{:05x}", *e),
    };
    s.add_dyn("F12748-07 A侧篡改10轮必抓", va.is_ok(), &va_text);

    // 注入必抓：B 侧篡改 10 轮。
    let vb = side_sweep_verdict(Side::SrcB);
    let vb_text = match &vb {
        Ok(r) => format!("caught={}/{} 零逃脱", r.caught, r.total),
        Err(e) => format!("逃脱 码=0x{:05x}", *e),
    };
    s.add_dyn("F12748-08 B侧篡改10轮必抓", vb.is_ok(), &vb_text);

    // 注入必抓：计数翻转 10 轮（B26 语料）。
    let vf = flip_sweep_verdict();
    let vf_text = match &vf {
        Ok(r) => format!("caught={}/{} 零逃脱", r.caught, r.total),
        Err(e) => format!("逃脱 码=0x{:05x}", *e),
    };
    s.add_dyn("F12748-09 计数翻转10轮必抓", vf.is_ok(), &vf_text);

    // 判据 J1 锚定：联动回归 20/20（B25 注入矩阵 × B26 坏表拒止）。
    s.add_dyn(
        "F12748-10 联动回归20/20",
        criteria_2020_ok(),
        "B25注入矩阵A10+B10全抓 × B26坏表拒止 链上零断",
    );

    s.add_dyn(
        "F12748-11 坏表拒止四闸",
        bad_table_reject_ok(),
        "重复/旗标非法/未知锁进出/无主或代离 全数拒",
    );

    s.add_dyn(
        "F12748-12 状态机倒退拒绝",
        rewind_ok(),
        "倒退/跳变/带等待跳级 显性拒",
    );

    // 双跑一致：两次构建指纹 + 同注入序列后指纹与发现流水逐项一致。
    let fp1 = build_standard_ledger().fingerprint();
    let fp2 = build_standard_ledger().fingerprint();
    s.add_dyn(
        "F12748-13 双跑一致指纹",
        dual_run_ok() && fp1 == fp2,
        &format!("fp1=0x{:08x} fp2=0x{:08x}", fp1, fp2),
    );

    s.add_dyn(
        "F12748-14 边界四点",
        boundary_ok(),
        "首锁0/尾锁15/重复判定/冻结后写拒 四点",
    );

    s.add_dyn(
        "F12748-15 容量耗尽",
        cap_ok(),
        &format!(
            "表{} 重入上限{} 等待上限{} 三路全拒",
            LOCK_CAP, RECUR_CAP, WAIT_CAP
        ),
    );

    // 收口：判据条数声明（前 15 条全绿 + 本条 = 16 条链上零断）。
    let reds_before = s.failed_count();
    let items_before = s.len();
    s.add_dyn(
        "F12748-16 判据收口·条数声明",
        reds_before == 0 && items_before == 15,
        &format!(
            "判据链共{}条 红项{} UNX-F12748-J1 联动回归20/20 链上零断",
            items_before + 1,
            reds_before
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x824C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 码互异、全部落窗、查表命中已知码并回落未知码。
        assert_eq!(CD_ALL.len(), 10);
        let mut i = 0usize;
        while i < CD_ALL.len() {
            let vi = CD_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CD_ALL.len() {
                let vj = CD_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(detail(CsDiag::EDrift.val()), CsDiag::EDrift.detail());
    }

    #[test]
    fn happy_path_state_machine() {
        // 正常流：标准语料 16 锁 + 逐锁判定全绿 + 状态抽查。
        let led = build_standard_ledger();
        assert_eq!(led.len(), LOCK_CAP);
        assert!(clean_judge_ok());
        // 锁 0：递归重入 + 争用 → 竞争态 进2 等1。
        match led.entry_a(0) {
            Some(e) => {
                assert_eq!(e.state, CsState::Contended);
                assert_eq!(e.enter_count, 2);
                assert_eq!(e.wait_count, 1);
            }
            None => assert!(false, "锁 0 应已登记"),
        }
        // 锁 1：进入后释放 → 自由态。
        match led.entry_a(1) {
            Some(e) => {
                assert_eq!(e.state, CsState::Free);
                assert_eq!(e.owner, 0);
            }
            None => assert!(false, "锁 1 应已登记"),
        }
        // 锁 4：争用持有 → 竞争态 进1 等1。
        match led.entry_a(4) {
            Some(e) => {
                assert_eq!(e.state, CsState::Contended);
                assert_eq!(e.enter_count, 1);
            }
            None => assert!(false, "锁 4 应已登记"),
        }
        // 锁 6：重入后部分释放 → 持有态 进1。
        match led.entry_a(6) {
            Some(e) => {
                assert_eq!(e.state, CsState::Owned);
                assert_eq!(e.enter_count, 1);
            }
            None => assert!(false, "锁 6 应已登记"),
        }
        assert!(happy_path_ok());
    }

    #[test]
    fn judge_order_enoent_ealready_edrift() {
        // 判定次序：ENOENT → EALREADY → EDRIFT 三闸先后不可换。
        assert!(judge_order_ok());
        assert!(judge_order_drift_already_ok());
        let mut led = CdLedger::new();
        // 次序一：未登记 → ENOENT。
        assert_eq!(led.judge_drift(0xFFFF), Err(CsDiag::ENoent.val()));
        let _ = led.register(7);
        // 次序三触发前提：单侧篡改。
        assert!(led.inject_tamper(Side::SrcA, 7, DriftField::DfOwner).is_ok());
        assert_eq!(led.judge_drift(7), Err(CsDiag::EDrift.val()));
        // 次序二：漂移仍在，重复判定 → EALREADY（先于再比对）。
        assert_eq!(led.judge_drift(7), Err(CsDiag::EAlready.val()));
        // 次序一先于次序二：未登记锁即便已有已判定锁也报 ENOENT。
        assert_eq!(led.judge_drift(0x1234), Err(CsDiag::ENoent.val()));
        // 发现记录指明被篡字段与锁号。
        let fs = led.findings_snapshot();
        assert_eq!(fs.len(), 1);
        match fs.first() {
            Some(f) => {
                assert_eq!(f.lock_no, 7);
                assert_eq!(f.field, DriftField::DfOwner);
            }
            None => assert!(false, "发现记录应存在"),
        }
    }

    #[test]
    fn injections_all_caught() {
        // 三类注入 10 轮全抓：A 侧 / B 侧 / 计数翻转，漂移即告警零静默。
        let ra = sweep_side(Side::SrcA);
        assert_eq!(ra.total, 10);
        assert_eq!(ra.caught, 10, "逃脱：{}", ra.escape);
        assert!(ra.escape.is_empty());
        let rb = sweep_side(Side::SrcB);
        assert_eq!(rb.total, 10);
        assert_eq!(rb.caught, 10, "逃脱：{}", rb.escape);
        assert!(rb.escape.is_empty());
        let rf = sweep_counter_flip();
        assert_eq!(rf.total, 10);
        assert_eq!(rf.caught, 10, "逃脱：{}", rf.escape);
        assert!(rf.escape.is_empty());
        assert!(criteria_2020_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次构建指纹相同；同一注入+判定序列后仍逐项一致。
        let a = build_standard_ledger();
        let b = build_standard_ledger();
        assert_eq!(a.fingerprint(), b.fingerprint());
        let mut a2 = build_standard_ledger();
        let mut b2 = build_standard_ledger();
        let mut k = 0u32;
        while k < 10 {
            let lock = (k * 3) % (LOCK_CAP as u32);
            let field = DRIFT_FIELD_ALL
                .get((k % 5) as usize)
                .copied()
                .unwrap_or(DriftField::DfOwner);
            let _ = a2.inject_tamper(Side::SrcA, lock, field);
            let _ = b2.inject_tamper(Side::SrcA, lock, field);
            let _ = a2.judge_drift(lock);
            let _ = b2.judge_drift(lock);
            k += 1;
        }
        assert_eq!(a2.fingerprint(), b2.fingerprint());
        assert_eq!(a2.findings_snapshot(), b2.findings_snapshot());
        assert!(dual_run_ok());
    }

    #[test]
    fn boundary_four_points() {
        // 四类边界：首锁 / 尾锁 / 重复判定 / 冻结后写拒。
        assert!(boundary_ok());
        let mut l1 = CdLedger::new();
        assert!(l1.register(0).is_ok());
        assert!(l1.enter(0, 11).is_ok());
        assert!(l1.judge_drift(0).is_ok());
        let mut l2 = CdLedger::new();
        assert!(l2.register(15).is_ok());
        assert!(l2.enter(15, 12).is_ok());
        assert!(l2.judge_drift(15).is_ok());
        assert_eq!(l2.judge_drift(15), Err(CsDiag::EAlready.val()));
        l2.freeze();
        assert!(l2.is_frozen());
        assert_eq!(l2.register(1), Err(CsDiag::EFrozen.val()));
        assert_eq!(l2.enter(15, 12), Err(CsDiag::EFrozen.val()));
    }

    #[test]
    fn capacity_exhaustion_ecap() {
        // 容量耗尽：登记表满 / 递归计数上限 / 等待队列上限 三路全拒。
        assert!(cap_ok());
        let mut l = CdLedger::new();
        let mut no = 0u32;
        while no < LOCK_CAP as u32 {
            assert!(l.register(no).is_ok());
            no += 1;
        }
        assert_eq!(l.register(0x40), Err(CsDiag::ECap.val()));
        let mut l2 = CdLedger::new();
        let _ = l2.register(0);
        let mut r = 0u32;
        while r < RECUR_CAP {
            assert!(l2.enter(0, 5).is_ok());
            r += 1;
        }
        assert_eq!(l2.enter(0, 5), Err(CsDiag::ECap.val()));
        let mut l3 = CdLedger::new();
        let _ = l3.register(0);
        assert!(l3.enter(0, 1).is_ok());
        let mut w = 0u32;
        while w < WAIT_CAP {
            assert!(l3.enter(0, 10 + w).is_ok());
            w += 1;
        }
        assert_eq!(l3.enter(0, 99), Err(CsDiag::ECap.val()));
    }

    #[test]
    fn rewind_rejected() {
        // 状态机倒退：转移表闭集逐一校验 + 注入场景显性拒。
        assert!(rewind_ok());
        assert_eq!(
            checked_transit(CsState::Contended, CsState::Owned, 0),
            Err(CsDiag::ERewind.val())
        );
        assert_eq!(
            checked_transit(CsState::Free, CsState::Contended, 0),
            Err(CsDiag::ERewind.val())
        );
        assert_eq!(
            checked_transit(CsState::Owned, CsState::Free, 1),
            Err(CsDiag::ERewind.val())
        );
        assert!(checked_transit(CsState::Free, CsState::Owned, 0).is_ok());
        assert!(checked_transit(CsState::Owned, CsState::Contended, 0).is_ok());
        assert!(checked_transit(CsState::Contended, CsState::Free, 0).is_ok());
        assert!(checked_transit(CsState::Owned, CsState::Free, 0).is_ok());
        // 场景：带等待强写 Owned 后离开 → 跳级倒退拒。
        let mut l = CdLedger::new();
        let _ = l.register(2);
        let _ = l.enter(2, 21);
        let _ = l.enter(2, 22);
        assert!(l.raw_set_state(2, CsState::Owned).is_ok());
        assert_eq!(l.leave(2, 21), Err(CsDiag::ERewind.val()));
    }

    #[test]
    fn bad_table_and_structure_gates() {
        // 坏表拒止：重复/旗标非法/未知锁进出/无主或代离 全数拒。
        assert!(bad_table_reject_ok());
        let mut l = CdLedger::new();
        assert!(l.register(0).is_ok());
        assert_eq!(l.register(0), Err(CsDiag::EDup.val()));
        assert_eq!(l.register_with_flag(1, 0), Err(CsDiag::EFlag.val()));
        assert_eq!(l.register_with_flag(1, 0b100), Err(CsDiag::EFlag.val()));
        assert_eq!(l.enter(0xFFFF, 1), Err(CsDiag::ENoent.val()));
        assert_eq!(l.leave(0xFFFF, 1), Err(CsDiag::ENoent.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 16 条）。
        let s = run_unx12747_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(s.len() >= 16);
        assert_eq!(s.tally().1, 0);
    }
}
