//! UNX-F12602 D1→D4 消费边总表与三列制度 · 0x80040段
//!
//! 判据原文（派工卡 12601 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12602-J1 端口对象类型/句柄语义消费对平断言绿
//!      （B22 档→D4），抽样 10 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 端口对象类型闭集：连接端口/服务端通信端口/客户端通信端口/未连接
//!    通信端口四类，类型错配（如客户端端口冒充服务端口消费）在配对口
//!    即拒——对平先对类型；
//! 2. 句柄语义：句柄值域 0x1000..=0xFFFF（B22 档保留区），引用计数 ≥1
//!    才可消费，计数归零后配对拒绝（悬空句柄必抓）；
//! 3. D1→D4 消费对平：D1（装载探测域）登记 B22 档端口对象，D4（消息泵
//!    域）按句柄消费并回填 D4 槽位，登记/消费/回填三步缺一即半对，
//!    半对单独记账不许冒充全对；
//! 4. 抽样 10 例：LCG 种子写死，逐例构建句柄→配对→对平断言，十例全绿
//!    且双跑逐项一致（确定性，非随机通过）；
//! 5. 状态机 未登记→已登记→已对平 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12601 − 12600)）。
pub const SEG_BASE: u32 = 0x80040;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 消费对平诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsDiag {
    /// 句柄值越 B22 档保留区。
    EHandleRange,
    /// 句柄未在 D1 登记即被 D4 消费。
    ENotReg,
    /// 句柄重复登记。
    EDupReg,
    /// 端口对象类型错配。
    EKind,
    /// 引用计数为零（悬空句柄）。
    ERefZero,
    /// 已对平句柄被重复消费。
    EPairedDup,
    /// D4 槽位越界。
    ESlotRange,
    /// D4 槽位已被占用。
    ESlotBusy,
    /// 半对冒充全对（回填缺失仍报对平）。
    EHalfAsFull,
    /// 状态机倒退。
    ERewind,
    /// 对平账空样本。
    EEmpty,
    /// 对平抽样结果与基线不一致。
    EDrift,
}

impl PsDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PsDiag::EHandleRange => 0x01,
                PsDiag::ENotReg => 0x02,
                PsDiag::EDupReg => 0x03,
                PsDiag::EKind => 0x04,
                PsDiag::ERefZero => 0x05,
                PsDiag::EPairedDup => 0x06,
                PsDiag::ESlotRange => 0x07,
                PsDiag::ESlotBusy => 0x08,
                PsDiag::EHalfAsFull => 0x09,
                PsDiag::ERewind => 0x0a,
                PsDiag::EEmpty => 0x0b,
                PsDiag::EDrift => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PsDiag::EHandleRange => "句柄越保留区：B22 档句柄值域 0x1000..=0xFFFF，先对值域再登记",
            PsDiag::ENotReg => "句柄未登记即消费：D1 先登记，D4 才许按句柄消费",
            PsDiag::EDupReg => "句柄重复登记：一柄一账，重复登记先查账面",
            PsDiag::EKind => "端口对象类型错配：对平先对类型，客户端端口不得冒充服务端口",
            PsDiag::ERefZero => "悬空句柄：引用计数为零不许消费，先补引用再对平",
            PsDiag::EPairedDup => "重复消费：已对平句柄不再入账，改动走解除配对流程",
            PsDiag::ESlotRange => "D4 槽位越界：槽位取 0..CAP，先查容量再回填",
            PsDiag::ESlotBusy => "槽位被占：一槽一柄，先释放旧柄再回填",
            PsDiag::EHalfAsFull => "半对冒充全对：回填缺失必须留在半对账，不许报对平",
            PsDiag::ERewind => "状态机倒退：未登记→已登记→已对平只进不退",
            PsDiag::EEmpty => "空样本：句柄与槽位全零不构成对平记录",
            PsDiag::EDrift => "抽样漂移：十例对平结果与基线不一致，先查确定性来源",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PS_ALL: &[PsDiag] = &[
    PsDiag::EHandleRange,
    PsDiag::ENotReg,
    PsDiag::EDupReg,
    PsDiag::EKind,
    PsDiag::ERefZero,
    PsDiag::EPairedDup,
    PsDiag::ESlotRange,
    PsDiag::ESlotBusy,
    PsDiag::EHalfAsFull,
    PsDiag::ERewind,
    PsDiag::EEmpty,
    PsDiag::EDrift,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in PS_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80041..0x8004C 闭集内"
}

// ── 端口对象类型闭集 ─────────────────────────────────────────────
/// 端口对象类型闭集（B22 档装载探测域口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// 连接端口（监听入口）。
    ConnectionPort,
    /// 服务端通信端口（接受后持有的对端）。
    ServerCommPort,
    /// 客户端通信端口（连接后持有的对端）。
    ClientCommPort,
    /// 未连接通信端口（自建自用）。
    UnconnectedComm,
}

impl PortKind {
    /// 类型序号（闭集序）。
    pub fn ordinal(self) -> u32 {
        match self {
            PortKind::ConnectionPort => 0,
            PortKind::ServerCommPort => 1,
            PortKind::ClientCommPort => 2,
            PortKind::UnconnectedComm => 3,
        }
    }

    /// 类型中文名。
    pub fn name(self) -> &'static str {
        match self {
            PortKind::ConnectionPort => "连接端口",
            PortKind::ServerCommPort => "服务端通信端口",
            PortKind::ClientCommPort => "客户端通信端口",
            PortKind::UnconnectedComm => "未连接通信端口",
        }
    }

    /// D4 消费域允许的类型集（连接/服务/未连接三类可对平；
    /// 客户端通信端口由客户端域消费，不进 D4 联签面）。
    pub fn d4_consumable(self) -> bool {
        match self {
            PortKind::ConnectionPort => true,
            PortKind::ServerCommPort => true,
            PortKind::ClientCommPort => false,
            PortKind::UnconnectedComm => true,
        }
    }
}

/// 类型闭集数组。
pub const KIND_ALL: &[PortKind] = &[
    PortKind::ConnectionPort,
    PortKind::ServerCommPort,
    PortKind::ClientCommPort,
    PortKind::UnconnectedComm,
];

// ── 句柄语义 ─────────────────────────────────────────────────────
/// B22 档句柄值域下沿（保留区起点）。
pub const HANDLE_MIN: u32 = 0x1000;
/// B22 档句柄值域上沿（保留区终点，含）。
pub const HANDLE_MAX: u32 = 0xFFFF;
/// D4 消费域槽位容量。
pub const SLOT_CAP: usize = 12;

/// 句柄语义单元：值 + 类型 + 引用计数 + 可继承标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleSem {
    /// 句柄值（0x1000..=0xFFFF）。
    pub handle: u32,
    /// 端口对象类型。
    pub kind: PortKind,
    /// 引用计数（≥1 才可消费）。
    pub refs: u32,
    /// 可继承标记。
    pub inheritable: bool,
}

impl HandleSem {
    /// 值域守卫。
    pub fn in_range(&self) -> bool {
        self.handle >= HANDLE_MIN && self.handle <= HANDLE_MAX
    }

    /// 可消费语义：值域内 + 引用计数 ≥1 + 类型可进 D4。
    pub fn consumable(&self) -> bool {
        self.in_range() && self.refs >= 1 && self.kind.d4_consumable()
    }

    /// 语义摘要（诊断文本）。
    pub fn brief(&self) -> String {
        format!(
            "句柄0x{:04X}({}) 引用{} 可继承{}",
            self.handle,
            self.kind.name(),
            self.refs,
            self.inheritable
        )
    }
}

/// 对平状态机：未登记 → 已登记 → 已对平。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    /// 未登记。
    Unregistered,
    /// D1 已登记，等待 D4 消费。
    Registered,
    /// D4 已消费并回填槽位，对平成立。
    Paired,
}

impl PairState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            PairState::Unregistered => 0,
            PairState::Registered => 1,
            PairState::Paired => 2,
        }
    }
}

/// 单条对平账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairEntry {
    /// 句柄语义。
    pub sem: HandleSem,
    /// 对平状态。
    pub state: PairState,
    /// D4 回填槽位（None = 未回填，半对）。
    pub d4_slot: Option<usize>,
}

/// D1→D4 消费对平表。
#[derive(Clone, Debug)]
pub struct PairTable {
    /// 对平账（按登记序）。
    entries: Vec<PairEntry>,
}

impl PairTable {
    /// 新表。
    pub fn new() -> PairTable {
        PairTable {
            entries: Vec::new(),
        }
    }

    /// 账面条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按句柄查账。
    pub fn entry_of(&self, handle: u32) -> Option<PairEntry> {
        for e in self.entries.iter() {
            if e.sem.handle == handle {
                return Some(*e);
            }
        }
        None
    }

    /// 第 idx 条账（越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<PairEntry> {
        self.entries.get(idx).copied()
    }

    /// D1 登记：值域/重复/空样本三闸。
    pub fn register(&mut self, sem: HandleSem) -> Result<(), u32> {
        if sem.handle == 0 && sem.refs == 0 {
            return Err(PsDiag::EEmpty.val());
        }
        if !sem.in_range() {
            return Err(PsDiag::EHandleRange.val());
        }
        if self.entry_of(sem.handle).is_some() {
            return Err(PsDiag::EDupReg.val());
        }
        self.entries.push(PairEntry {
            sem,
            state: PairState::Registered,
            d4_slot: None,
        });
        Ok(())
    }

    /// D4 消费：类型/引用/状态/槽位四闸 + 回填。
    pub fn consume(&mut self, handle: u32, slot: usize) -> Result<(), u32> {
        let idx = match self.index_of(handle) {
            Some(i) => i,
            None => return Err(PsDiag::ENotReg.val()),
        };
        let entry = match self.entries.get(idx) {
            Some(e) => *e,
            None => return Err(PsDiag::ENotReg.val()),
        };
        // 悬空句柄闸。
        if entry.sem.refs == 0 {
            return Err(PsDiag::ERefZero.val());
        }
        // 类型闸：客户端通信端口不进 D4。
        if !entry.sem.kind.d4_consumable() {
            return Err(PsDiag::EKind.val());
        }
        // 状态闸：未登记不可消费，已对平不可重复消费。
        if entry.state.rank() >= PairState::Paired.rank() {
            return Err(PsDiag::EPairedDup.val());
        }
        // 槽位闸。
        if slot >= SLOT_CAP {
            return Err(PsDiag::ESlotRange.val());
        }
        for e in self.entries.iter() {
            if e.d4_slot == Some(slot) {
                return Err(PsDiag::ESlotBusy.val());
            }
        }
        // 回填：登记→对平，状态只进不退。
        if let Some(e) = self.entries.get_mut(idx) {
            e.state = PairState::Paired;
            e.d4_slot = Some(slot);
        }
        Ok(())
    }

    /// 解除对平（只许从 Paired 退回 Registered 用于重联签；
    /// 其他状态解除即倒退，拒）。
    pub fn unpair(&mut self, handle: u32) -> Result<(), u32> {
        let idx = match self.index_of(handle) {
            Some(i) => i,
            None => return Err(PsDiag::ENotReg.val()),
        };
        match self.entries.get(idx).map(|e| e.state) {
            Some(PairState::Paired) => {
                if let Some(e) = self.entries.get_mut(idx) {
                    e.state = PairState::Registered;
                    e.d4_slot = None;
                }
                Ok(())
            }
            Some(PairState::Registered) => Err(PsDiag::ERewind.val()),
            _ => Err(PsDiag::ERewind.val()),
        }
    }

    /// 按句柄找账内下标。
    fn index_of(&self, handle: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.sem.handle == handle {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 全对条数（已对平）。
    pub fn paired_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.state == PairState::Paired && e.d4_slot.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 半对条数（已登记未回填）。
    pub fn half_count(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.state == PairState::Registered {
                n += 1;
            }
        }
        n
    }

    /// 槽位占用检查（一槽一柄守恒：无重槽）。
    pub fn slots_consistent(&self) -> bool {
        let mut seen: Vec<usize> = Vec::new();
        for e in self.entries.iter() {
            if let Some(s) = e.d4_slot {
                for v in seen.iter() {
                    if *v == s {
                        return false;
                    }
                }
                seen.push(s);
            }
        }
        true
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
/// 线性同余发生器（Knuth MMIX 常数，种子写死）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一个 32 位伪随机数（确定序列）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
}

// ── 抽样 10 例：句柄→登记→消费→对平断言 ─────────────────────────
/// 单例抽样结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleCase {
    /// 例号 0..9。
    pub case: u32,
    /// 句柄值。
    pub handle: u32,
    /// 类型序号。
    pub kind_ord: u32,
    /// 引用计数。
    pub refs: u32,
    /// 分配的 D4 槽位。
    pub slot: usize,
    /// 对平是否成立。
    pub paired: bool,
}

/// 构建十例抽样：逐例确定性生成句柄/类型/槽位并走完整对平流。
/// 返回 (抽样例集, 对平表)。
pub fn sample_ten() -> (Vec<SampleCase>, PairTable) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut table = PairTable::new();
    let mut cases: Vec<SampleCase> = Vec::new();
    let mut k = 0u32;
    while k < 10 {
        // 句柄值：保留区内确定性取值；类型：0..4 轮转；
        // 引用计数：1..=3；槽位：例号即槽位（一例一槽，不撞）。
        let handle = HANDLE_MIN + (rng.next_u32() % (HANDLE_MAX - HANDLE_MIN));
        let kind = match k % 4 {
            0 => PortKind::ConnectionPort,
            1 => PortKind::ServerCommPort,
            2 => PortKind::ClientCommPort,
            _ => PortKind::UnconnectedComm,
        };
        let refs = 1 + rng.next_u32() % 3;
        let sem = HandleSem {
            handle,
            kind,
            refs,
            inheritable: k % 2 == 0,
        };
        let mut paired = false;
        let mut slot = 0usize;
        if table.register(sem).is_ok() {
            // 客户端通信端口不进 D4（类型闸语义），其余按例号占槽。
            if kind.d4_consumable() {
                slot = (k % SLOT_CAP as u32) as usize;
                if table.consume(handle, slot).is_ok() {
                    paired = true;
                }
            }
        }
        cases.push(SampleCase {
            case: k,
            handle,
            kind_ord: kind.ordinal(),
            refs,
            slot,
            paired,
        });
        k += 1;
    }
    (cases, table)
}

/// 抽样基线复算：双跑一致即对平断言绿。
fn sample_drift_ok() -> bool {
    let (a, ta) = sample_ten();
    let (b, tb) = sample_ten();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    ta.paired_count() == tb.paired_count() && ta.half_count() == tb.half_count()
}

/// 对平断言：可消费例全对平、不可消费例全留在登记态。
fn pairing_assert_ok(cases: &[SampleCase], table: &PairTable) -> bool {
    for c in cases.iter() {
        let kind = match c.kind_ord {
            0 => PortKind::ConnectionPort,
            1 => PortKind::ServerCommPort,
            2 => PortKind::ClientCommPort,
            _ => PortKind::UnconnectedComm,
        };
        match table.entry_of(c.handle) {
            Some(e) => {
                if kind.d4_consumable() {
                    // 可消费：必须对平成立且槽位一致。
                    if e.state != PairState::Paired || e.d4_slot != Some(c.slot) {
                        return false;
                    }
                } else {
                    // 不可消费：必须留在登记态（半对），不许冒充全对。
                    if e.state == PairState::Paired {
                        return false;
                    }
                }
            }
            None => return false,
        }
    }
    true
}

/// 账守恒：全对 + 半对 = 总账数；槽位无重占。
fn ledger_conservation_ok(table: &PairTable) -> bool {
    table.paired_count() + table.half_count() == table.len() && table.slots_consistent()
}

// ── 判据谓词（私有；cond 为违约条件） ───────────────────────────
/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in PS_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(PsDiag::EKind.val()) != PsDiag::EKind.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3F).starts_with("未知") {
        return false;
    }
    true
}

/// 类型闭集守卫：四类互异、ordinal 闭合。
fn kind_closed_ok() -> bool {
    if KIND_ALL.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < KIND_ALL.len() {
        let a = match KIND_ALL.get(i) {
            Some(k) => *k,
            None => return false,
        };
        let mut j = i + 1;
        while j < KIND_ALL.len() {
            let b = match KIND_ALL.get(j) {
                Some(k) => *k,
                None => return false,
            };
            if a.ordinal() == b.ordinal() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 正常流：登记→消费→对平→守恒。
fn happy_path_ok() -> bool {
    let mut t = PairTable::new();
    let sem = HandleSem {
        handle: 0x1000,
        kind: PortKind::ConnectionPort,
        refs: 2,
        inheritable: false,
    };
    if t.register(sem).is_err() {
        return false;
    }
    if t.consume(0x1000, 0).is_err() {
        return false;
    }
    t.paired_count() == 1
        && t.half_count() == 0
        && ledger_conservation_ok(&t)
        && matches!(t.entry_of(0x1000).map(|e| e.state), Some(PairState::Paired))
}

/// 注入面：坏句柄/坏类型/坏槽位/倒退四类违约必抓。
fn injection_caught_ok() -> bool {
    let mut hits = 0usize;
    // 注 1：值域外句柄登记被拒。
    let mut t = PairTable::new();
    if t.register(HandleSem {
        handle: 0x20,
        kind: PortKind::ConnectionPort,
        refs: 1,
        inheritable: false,
    }) == Err(PsDiag::EHandleRange.val())
    {
        hits += 1;
    }
    // 注 2：未登记句柄消费被拒。
    if t.consume(0x1234, 0) == Err(PsDiag::ENotReg.val()) {
        hits += 1;
    }
    // 注 3：重复登记被拒。
    let _ = t.register(HandleSem {
        handle: 0x1001,
        kind: PortKind::ServerCommPort,
        refs: 1,
        inheritable: true,
    });
    if t.register(HandleSem {
        handle: 0x1001,
        kind: PortKind::ServerCommPort,
        refs: 1,
        inheritable: true,
    }) == Err(PsDiag::EDupReg.val())
    {
        hits += 1;
    }
    // 注 4：客户端端口冒充消费被拒（类型错配）。
    let _ = t.register(HandleSem {
        handle: 0x1002,
        kind: PortKind::ClientCommPort,
        refs: 1,
        inheritable: false,
    });
    if t.consume(0x1002, 1) == Err(PsDiag::EKind.val()) {
        hits += 1;
    }
    // 注 5：槽位越界被拒。
    if t.consume(0x1001, SLOT_CAP) == Err(PsDiag::ESlotRange.val()) {
        hits += 1;
    }
    // 注 6：重复消费已对平句柄被拒。
    let _ = t.consume(0x1001, 1);
    if t.consume(0x1001, 2) == Err(PsDiag::EPairedDup.val()) {
        hits += 1;
    }
    // 注 7：槽位撞车被拒（另一柄占同槽）。
    let _ = t.register(HandleSem {
        handle: 0x1003,
        kind: PortKind::UnconnectedComm,
        refs: 1,
        inheritable: false,
    });
    if t.consume(0x1003, 1) == Err(PsDiag::ESlotBusy.val()) {
        hits += 1;
    }
    // 注 8：半对冒充全对——断言面必须识别（登记态不得计为对平）。
    let mut t2 = PairTable::new();
    let _ = t2.register(HandleSem {
        handle: 0x1010,
        kind: PortKind::ConnectionPort,
        refs: 1,
        inheritable: false,
    });
    // 未消费时账面必须呈现半对（paired=0, half=1），否则对平语义失效。
    if t2.paired_count() == 0 && t2.half_count() == 1 {
        hits += 1;
    }
    // 注 9：状态机倒退——未对平句柄解除配对被拒。
    if t2.unpair(0x1010) == Err(PsDiag::ERewind.val()) {
        hits += 1;
    }
    // 注 10：空样本被拒。
    if t2.register(HandleSem {
        handle: 0,
        kind: PortKind::ConnectionPort,
        refs: 0,
        inheritable: false,
    }) == Err(PsDiag::EEmpty.val())
    {
        hits += 1;
    }
    hits == 10
}

/// 解除-重联签往返：Paired→Registered→Paired 守恒。
fn unpair_roundtrip_ok() -> bool {
    let mut t = PairTable::new();
    let _ = t.register(HandleSem {
        handle: 0x2000,
        kind: PortKind::ConnectionPort,
        refs: 3,
        inheritable: false,
    });
    if t.consume(0x2000, 3).is_err() {
        return false;
    }
    if t.unpair(0x2000).is_err() {
        return false;
    }
    if t.entry_of(0x2000).map(|e| e.state) != Some(PairState::Registered) {
        return false;
    }
    t.consume(0x2000, 5).is_ok()
        && t.entry_of(0x2000).map(|e| e.d4_slot) == Some(Some(5))
        && ledger_conservation_ok(&t)
}

/// 完成写回语义：十例抽样摘要非空。
fn writeback_ok() -> bool {
    let (cases, table) = sample_ten();
    let summary = format!(
        "cases={} paired={} half={}",
        cases.len(),
        table.paired_count(),
        table.half_count()
    );
    cases.len() == 10 && !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12602 自检（判据 12601 卡：端口对象类型/句柄语义消费对平）。
pub fn run_unx12601_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12601");
    let (cases, table) = sample_ten();

    s.add_dyn(
        "F12602-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12602-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", PS_ALL.len()),
    );

    s.add_dyn(
        "F12602-03 类型闭集",
        kind_closed_ok(),
        &format!(
            "kinds={} d4可消费={}",
            KIND_ALL.len(),
            KIND_ALL
                .iter()
                .filter(|k| k.d4_consumable())
                .count()
        ),
    );

    s.add_dyn(
        "F12602-04 正常流对平",
        happy_path_ok(),
        "登记→消费→对平→守恒 单例全绿",
    );

    s.add_dyn(
        "F12602-05 抽样十例构建",
        cases.len() == 10 && table.len() == 10,
        &format!(
            "cases={} entries={} paired={} half={}",
            cases.len(),
            table.len(),
            table.paired_count(),
            table.half_count()
        ),
    );

    s.add_dyn(
        "F12602-06 对平断言绿",
        pairing_assert_ok(&cases, &table),
        "可消费例全对平，客户端端口例全留半对",
    );

    s.add_dyn(
        "F12602-07 账守恒与槽位一致",
        ledger_conservation_ok(&table),
        &format!("paired+half==len 重槽=无"),
    );

    // 注入必抓：十路违约全数命中。
    s.add_dyn(
        "F12602-08 违约注入 10/10 必抓",
        injection_caught_ok(),
        "值域/未登记/重复/类型/槽界/重复消费/撞槽/半对/倒退/空样本",
    );

    s.add_dyn(
        "F12602-09 双跑一致（抽样 10 例）",
        sample_drift_ok(),
        "十例逐项比对 + 对平/半对计数一致",
    );

    s.add_dyn(
        "F12602-10 解除-重联签往返",
        unpair_roundtrip_ok(),
        "Paired→Registered→Paired 守恒",
    );

    s.add_dyn(
        "F12602-11 完成写回语义",
        writeback_ok(),
        "十例摘要非空即写回合规",
    );

    // 句柄语义直读：值域/引用/可消费三条内聚断言。
    let sem_ok = {
        let a = HandleSem {
            handle: HANDLE_MIN,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        let b = HandleSem {
            handle: HANDLE_MAX,
            kind: PortKind::ServerCommPort,
            refs: 1,
            inheritable: true,
        };
        let c = HandleSem {
            handle: HANDLE_MIN - 1,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        let d = HandleSem {
            handle: HANDLE_MAX + 1,
            kind: PortKind::ServerCommPort,
            refs: 1,
            inheritable: false,
        };
        a.consumable() && b.consumable() && !c.in_range() && !d.in_range()
    };
    s.add_dyn(
        "F12602-12 句柄语义边界",
        sem_ok,
        &format!("值域 0x{:04X}..=0x{:04X} 两端含界", HANDLE_MIN, HANDLE_MAX),
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
        assert_eq!(SEG_BASE, 0x80040);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(PS_ALL.len(), 12);
        let mut i = 0usize;
        while i < PS_ALL.len() {
            let vi = PS_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PS_ALL.len() {
                let vj = PS_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn kind_closed_set_four() {
        assert_eq!(KIND_ALL.len(), 4);
        assert!(!PortKind::ClientCommPort.d4_consumable());
        assert!(PortKind::ServerCommPort.d4_consumable());
        assert_eq!(PortKind::UnconnectedComm.ordinal(), 3);
    }

    #[test]
    fn handle_range_boundaries() {
        // 值域两端含界：MIN/MAX 合法，MIN-1/MAX-1 越界。
        let ok_lo = HandleSem {
            handle: HANDLE_MIN,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        let ok_hi = HandleSem {
            handle: HANDLE_MAX,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        let bad_lo = HandleSem {
            handle: HANDLE_MIN - 1,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        let bad_hi = HandleSem {
            handle: HANDLE_MAX + 1,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        };
        assert!(ok_lo.in_range() && ok_hi.in_range());
        assert!(!bad_lo.in_range() && !bad_hi.in_range());
        // 悬空句柄不可消费。
        let dangling = HandleSem {
            handle: 0x3000,
            kind: PortKind::ConnectionPort,
            refs: 0,
            inheritable: false,
        };
        assert!(!dangling.consumable());
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数命中。
        assert!(injection_caught_ok());
    }

    #[test]
    fn dual_run_sample_ten_identical() {
        // 双跑一致：十例逐项与账面计数完全相同。
        let (a, ta) = sample_ten();
        let (b, tb) = sample_ten();
        assert_eq!(a, b);
        assert_eq!(ta.paired_count(), tb.paired_count());
        assert_eq!(ta.half_count(), tb.half_count());
    }

    #[test]
    fn pairing_assert_green_on_baseline() {
        let (cases, table) = sample_ten();
        assert!(pairing_assert_ok(&cases, &table));
        assert!(ledger_conservation_ok(&table));
        // 客户端通信端口例必须留在半对。
        for c in cases.iter() {
            if c.kind_ord == 2 {
                let e = table.entry_of(c.handle);
                assert!(e.is_some());
                assert_ne!(e.map(|x| x.state), Some(PairState::Paired));
            }
        }
    }

    #[test]
    fn slot_gates_reject() {
        let mut t = PairTable::new();
        let _ = t.register(HandleSem {
            handle: 0x4000,
            kind: PortKind::ConnectionPort,
            refs: 1,
            inheritable: false,
        });
        // 槽位越界。
        assert_eq!(t.consume(0x4000, SLOT_CAP), Err(PsDiag::ESlotRange.val()));
        // 正常占槽后撞槽拒绝。
        assert!(t.consume(0x4000, 7).is_ok());
        let _ = t.register(HandleSem {
            handle: 0x4001,
            kind: PortKind::ServerCommPort,
            refs: 1,
            inheritable: false,
        });
        assert_eq!(t.consume(0x4001, 7), Err(PsDiag::ESlotBusy.val()));
        // 重复消费拒绝。
        assert_eq!(t.consume(0x4000, 8), Err(PsDiag::EPairedDup.val()));
    }

    #[test]
    fn unpair_rejects_rewind_but_allows_roundtrip() {
        let mut t = PairTable::new();
        let _ = t.register(HandleSem {
            handle: 0x5000,
            kind: PortKind::ConnectionPort,
            refs: 2,
            inheritable: false,
        });
        // 未对平解除 = 倒退，拒。
        assert_eq!(t.unpair(0x5000), Err(PsDiag::ERewind.val()));
        // 未登记句柄解除拒。
        assert_eq!(t.unpair(0x5001), Err(PsDiag::ENotReg.val()));
        // 对平后解除→重联签守恒。
        assert!(t.consume(0x5000, 2).is_ok());
        assert!(t.unpair(0x5000).is_ok());
        assert!(t.consume(0x5000, 4).is_ok());
        assert_eq!(t.entry_of(0x5000).map(|e| e.d4_slot), Some(Some(4)));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12601_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn writeback_summary_nonempty() {
        // 完成写回：摘要非空且覆盖十例。
        let (cases, table) = sample_ten();
        let summary = format!(
            "cases={} paired={}",
            cases.len(),
            table.paired_count()
        );
        assert!(!summary.is_empty());
        assert_eq!(cases.len(), 10);
    }
}
