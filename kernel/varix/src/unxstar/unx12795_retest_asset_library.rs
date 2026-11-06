//! UNX-F12795 移交包五：复测资产库（注入/双机/边界样本） · 0x830C0段
//!
//! 判据原文（派工卡 12795 逐字锚定）：
//!   UNX-F12796-J1 双轨声明判据绿（真机判据全量登记"随闸门补测"），零 QEMU
//!   纪律可点验。
//!
//! 设计要点：
//! 1. 双轨产线声明域模型：真机轨为唯一执行轨，登记表是判据的唯一身份账——
//!    登记态三态单调 PendingRegistered→Registered→Executed，只许 +1 推进，
//!    倒退（ERewind）、同秩重入（EJumpState）一律显性拒；
//! 2. 全量登记闭集：冻结登记表 64 行（判据号 12796..=12859 连续，行三元 =
//!    判据号/登记态/预算行，预算行全非零），全量登记"随闸门补测"即所有行
//!    登记态 ≥ Registered，缺一行即 ENotFull 不绿；
//! 3. 零 QEMU 纪律可点验：QEMU 轨请求优先于一切状态闸一律抓（EQemuDep），
//!    登记表逐行 QEMU 引用计数恒 0、被抓计数逐轮记账，任何注入 QEMU 依赖
//!    的行为必被抓，账面不增一笔 QEMU 引用；
//! 4. 三类注入器各 10 轮确定性全抓（LCG 种子写死）：跳级执行（未登记直执/
//!    Pending 跳级/已执行同秩重执）、QEMU 依赖注入（四种登记态轮转）、
//!    登记篡改（倒退确认/重复声明/同秩重确认/已执行重声明）——逃脱即判据
//!    失效；
//! 5. 双机一致指纹：FNV 指纹覆盖全账行态 + 执行序 + QEMU 账，两次独立构建
//!    逐位一致；判据号界外/容量耗尽/首尾判据号四类边界显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12795 − 12600)）。
pub const SEG_BASE: u32 = 0x830C0;

// ── 登记判定诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegDiag {
    /// 判据号越出冻结登记表闭集。
    EIdRange,
    /// 判据号重复声明。
    EDupDecl,
    /// 未登记判据被操作（未声明即确认/执行）。
    ENotDeclared,
    /// 跳级迁移（越过中间登记态或同秩重入）。
    EJumpState,
    /// 登记态倒退（Executed 不得回落）。
    ERewind,
    /// QEMU 轨依赖注入（零 QEMU 纪律，一律抓）。
    EQemuDep,
    /// 登记表容量耗尽（满员后再声明）。
    EFull,
    /// 预算行缺账（预算行为零的行不许入账）。
    EBudget,
    /// 冻结登记表被改写（连续性/预算行完整性破缺）。
    EFrozen,
    /// 执行序断链（执行序号不连续或越账）。
    ESeqMiss,
    /// 全量登记未齐（有判据未达"随闸门补测"登记态）。
    ENotFull,
    /// 双机指纹不一致（两次构建结果漂移）。
    EFingerprint,
}

impl RegDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RegDiag::EIdRange => 0x01,
                RegDiag::EDupDecl => 0x02,
                RegDiag::ENotDeclared => 0x03,
                RegDiag::EJumpState => 0x04,
                RegDiag::ERewind => 0x05,
                RegDiag::EQemuDep => 0x06,
                RegDiag::EFull => 0x07,
                RegDiag::EBudget => 0x08,
                RegDiag::EFrozen => 0x09,
                RegDiag::ESeqMiss => 0x0a,
                RegDiag::ENotFull => 0x0b,
                RegDiag::EFingerprint => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RegDiag::EIdRange => {
                "判据号越闭集：只认冻结登记表 12796..=12859 连续号段，先对表再申报"
            }
            RegDiag::EDupDecl => "判据号重复声明：一行只落一笔，重复申报先查账内已有判据号",
            RegDiag::ENotDeclared => {
                "未登记判据被操作：先走声明口入账，未登记判据不得确认更不得执行"
            }
            RegDiag::EJumpState => {
                "跳级迁移：登记态只许 +1 推进，Pending 不能直执、已执行不能重执，按序补齐中间态"
            }
            RegDiag::ERewind => {
                "登记态倒退：Pending→Registered→Executed 只进不退，倒退即账本可疑须整表复核"
            }
            RegDiag::EQemuDep => {
                "QEMU 依赖注入被拒：零 QEMU 纪律优先于一切状态闸，改走真机轨并留痕"
            }
            RegDiag::EFull => "登记表容量耗尽：64 行闭集已满员，新增判据走下一批滚动，不在本表扩容",
            RegDiag::EBudget => "预算行缺账：预算行为零的行不许入账，先回验收文档补行号再申报",
            RegDiag::EFrozen => {
                "冻结登记表破缺：判据号必须连续、预算行必须非零，闭集被改写须整表回滚重审"
            }
            RegDiag::ESeqMiss => {
                "执行序断链：执行序号必须 1..n 连续递增，断链先对执行留痕再补序"
            }
            RegDiag::ENotFull => {
                "全量登记未齐：有判据未达随闸门补测登记态或预算行与冻结表不符，补齐后才可绿"
            }
            RegDiag::EFingerprint => {
                "双机指纹不一致：两次构建漂移即环境可疑，先查 LCG 种子与账本推进序"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const REG_DIAG_ALL: &[RegDiag] = &[
    RegDiag::EIdRange,
    RegDiag::EDupDecl,
    RegDiag::ENotDeclared,
    RegDiag::EJumpState,
    RegDiag::ERewind,
    RegDiag::EQemuDep,
    RegDiag::EFull,
    RegDiag::EBudget,
    RegDiag::EFrozen,
    RegDiag::ESeqMiss,
    RegDiag::ENotFull,
    RegDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in REG_DIAG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x830C1..0x830CC 闭集内"
}

// ── 登记态三态单调状态机 ─────────────────────────────────────────
/// 真机判据登记态：PendingRegistered → Registered（随闸门补测）→ Executed，
/// 只许 +1 推进，倒退与同秩重入显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegState {
    /// 已声明待登记：判据号入账但尚未取得随闸门补测资格。
    PendingRegistered,
    /// 随闸门补测已登记：全量登记的目标态（判据原文登记态）。
    Registered,
    /// 已随闸门补测执行：真机轨执行完成，带执行序留痕。
    Executed,
}

impl RegState {
    /// 登记态秩（只许前进，倒退即 ERewind）。
    pub fn rank(self) -> u8 {
        match self {
            RegState::PendingRegistered => 0,
            RegState::Registered => 1,
            RegState::Executed => 2,
        }
    }

    /// 登记态中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            RegState::PendingRegistered => "已声明待登记",
            RegState::Registered => "随闸门补测",
            RegState::Executed => "已执行",
        }
    }
}

/// 登记动作闭集：声明 / 随闸门补测登记 / 真机执行 / QEMU 轨请求。
/// QEMU 轨请求不是合法动作，列在闭集里是为了让注入器有合法语法面可打——
/// 判定器对它无条件拒（零 QEMU 纪律优先于一切状态闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegAction {
    /// 首次声明：未登记判据号入账，态 = PendingRegistered。
    Declare,
    /// 随闸门补测登记：PendingRegistered → Registered。
    ConfirmGate,
    /// 真机轨执行：Registered → Executed，记录执行序。
    ExecuteReal,
    /// QEMU 轨执行请求：任何登记态一律抓（零 QEMU 纪律）。
    ExecuteQemu,
}

// ── 冻结闭集登记表（64 行全量登记） ──────────────────────────────
/// 冻结登记表行数（判据原文"全量登记"的闭集容量，≥60 行）。
pub const REG_CAP: usize = 64;
/// 首判据号（判据 UNX-F12796-J1 起，连续 64 号到 12859）。
pub const REG_ID_BASE: u32 = 12796;
/// 活账容量 = 冻结闭集行数（满员后再声明显性拒）。
pub const LIVE_CAP: usize = REG_CAP;
/// 双机场景中真机轨预执行条数（执行序 1..=EXEC_DEMO，留痕用）。
pub const EXEC_DEMO: u32 = 6;

/// 冻结登记表单行：判据号 + 预算行（登记态在活账中推进，冻结表只冻结
/// 身份与预算两列，登记态的"随闸门补测"由判定器单调推进落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegRow {
    /// 判据号（闭集内连续）。
    pub id: u32,
    /// 预算行（验收文档行号，非零有效）。
    pub budget: u32,
}

/// 冻结闭集登记表：判据号 12796..=12859 连续，预算行 = 复测验收文档
/// 行号（非零），行序冻结不许重排（来源：移交包五复测计划 F12796-J1 附表）。
pub const REG_ROWS: &[RegRow] = &[
    RegRow { id: 12796, budget: 118 },
    RegRow { id: 12797, budget: 131 },
    RegRow { id: 12798, budget: 104 },
    RegRow { id: 12799, budget: 142 },
    RegRow { id: 12800, budget: 109 },
    RegRow { id: 12801, budget: 133 },
    RegRow { id: 12802, budget: 126 },
    RegRow { id: 12803, budget: 150 },
    RegRow { id: 12804, budget: 111 },
    RegRow { id: 12805, budget: 138 },
    RegRow { id: 12806, budget: 121 },
    RegRow { id: 12807, budget: 147 },
    RegRow { id: 12808, budget: 116 },
    RegRow { id: 12809, budget: 155 },
    RegRow { id: 12810, budget: 129 },
    RegRow { id: 12811, budget: 140 },
    RegRow { id: 12812, budget: 107 },
    RegRow { id: 12813, budget: 136 },
    RegRow { id: 12814, budget: 123 },
    RegRow { id: 12815, budget: 152 },
    RegRow { id: 12816, budget: 113 },
    RegRow { id: 12817, budget: 144 },
    RegRow { id: 12818, budget: 119 },
    RegRow { id: 12819, budget: 158 },
    RegRow { id: 12820, budget: 125 },
    RegRow { id: 12821, budget: 132 },
    RegRow { id: 12822, budget: 108 },
    RegRow { id: 12823, budget: 146 },
    RegRow { id: 12824, budget: 117 },
    RegRow { id: 12825, budget: 149 },
    RegRow { id: 12826, budget: 124 },
    RegRow { id: 12827, budget: 141 },
    RegRow { id: 12828, budget: 103 },
    RegRow { id: 12829, budget: 137 },
    RegRow { id: 12830, budget: 127 },
    RegRow { id: 12831, budget: 153 },
    RegRow { id: 12832, budget: 110 },
    RegRow { id: 12833, budget: 148 },
    RegRow { id: 12834, budget: 120 },
    RegRow { id: 12835, budget: 156 },
    RegRow { id: 12836, budget: 114 },
    RegRow { id: 12837, budget: 134 },
    RegRow { id: 12838, budget: 122 },
    RegRow { id: 12839, budget: 159 },
    RegRow { id: 12840, budget: 128 },
    RegRow { id: 12841, budget: 143 },
    RegRow { id: 12842, budget: 115 },
    RegRow { id: 12843, budget: 151 },
    RegRow { id: 12844, budget: 106 },
    RegRow { id: 12845, budget: 135 },
    RegRow { id: 12846, budget: 130 },
    RegRow { id: 12847, budget: 157 },
    RegRow { id: 12848, budget: 112 },
    RegRow { id: 12849, budget: 145 },
    RegRow { id: 12850, budget: 118 },
    RegRow { id: 12851, budget: 154 },
    RegRow { id: 12852, budget: 101 },
    RegRow { id: 12853, budget: 139 },
    RegRow { id: 12854, budget: 126 },
    RegRow { id: 12855, budget: 160 },
    RegRow { id: 12856, budget: 105 },
    RegRow { id: 12857, budget: 150 },
    RegRow { id: 12858, budget: 119 },
    RegRow { id: 12859, budget: 162 },
];

/// 判据号 → 冻结表下标（闭集外返回 None）。
pub fn frozen_index(id: u32) -> Option<usize> {
    if id < REG_ID_BASE {
        return None;
    }
    let off = id - REG_ID_BASE;
    if off >= REG_CAP as u32 {
        return None;
    }
    Some(off as usize)
}

/// 判据号 → 冻结表行（闭集外返回 None）。
pub fn frozen_row(id: u32) -> Option<&'static RegRow> {
    let idx = frozen_index(id)?;
    REG_ROWS.get(idx)
}

// ── 活账（登记表账本） ───────────────────────────────────────────
/// 活账单行：判据号 + 当前登记态 + 预算行（自冻结表拷贝）+ 执行序留痕
/// + 逐行 QEMU 引用计数（零 QEMU 纪律下恒 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegRowLive {
    /// 判据号。
    pub id: u32,
    /// 当前登记态（三态单调）。
    pub state: RegState,
    /// 预算行（与冻结表一致，非零）。
    pub budget: u32,
    /// 执行序（0 = 未执行，已执行为 1..n 连续递增）。
    pub exec_seq: u32,
    /// 本行 QEMU 引用计数（零 QEMU 纪律：恒 0，任何附加企图必被抓）。
    pub qemu_refs: u32,
}

/// 真机判据登记表账本：行按声明序落账，登记态只进不退。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegBook {
    /// 已入账行（按声明序）。
    rows: Vec<RegRowLive>,
    /// 账本容量（= 冻结闭集行数）。
    cap: usize,
    /// 下一执行序号（从 1 起，Executed 落账时消费）。
    exec_seq_next: u32,
    /// 被抓的 QEMU 依赖注入笔数（零 QEMU 纪律点验账）。
    qemu_blocked: u32,
}

impl RegBook {
    /// 开一本新账（容量 = 活账容量）。
    pub fn new(cap: usize) -> RegBook {
        RegBook {
            rows: Vec::new(),
            cap,
            exec_seq_next: 1,
            qemu_blocked: 0,
        }
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 账本容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 下一执行序号。
    pub fn exec_seq_next(&self) -> u32 {
        self.exec_seq_next
    }

    /// 被抓 QEMU 注入笔数。
    pub fn qemu_blocked(&self) -> u32 {
        self.qemu_blocked
    }

    /// 登记表 QEMU 引用计数总额（零 QEMU 纪律点验口：恒 0）。
    pub fn qemu_ref_total(&self) -> u32 {
        let mut n = 0u32;
        for r in self.rows.iter() {
            n = n.saturating_add(r.qemu_refs);
        }
        n
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<RegRowLive> {
        self.rows.get(idx).copied()
    }

    /// 按判据号查行。
    pub fn row_of(&self, id: u32) -> Option<RegRowLive> {
        for r in self.rows.iter() {
            if r.id == id {
                return Some(*r);
            }
        }
        None
    }

    /// 按判据号取可变行。
    fn row_of_mut(&mut self, id: u32) -> Option<&mut RegRowLive> {
        for r in self.rows.iter_mut() {
            if r.id == id {
                return Some(r);
            }
        }
        None
    }

    /// 登记判定器：`judge_register(id, action) -> Result<RegState, u32>`。
    ///
    /// 闸序（先到先判）：
    /// ① 零 QEMU 纪律闸——QEMU 轨请求无条件抓（优先于一切状态闸）；
    /// ② 容量闸（仅声明）——满员后再声明一律 EFull；
    /// ③ 闭集闸——判据号必须落在冻结登记表内；
    /// ④ 重复/未登记闸——重复声明 EDupDecl，未登记被操作 ENotDeclared；
    /// ⑤ 单调闸——目标态必须恰为当前态 +1，倒退 ERewind，跳级 EJumpState。
    /// 未登记判据不可跳级 Executed；Registered 后才可随闸门补测执行；
    /// 倒退拒绝。
    pub fn judge_register(&mut self, id: u32, action: RegAction) -> Result<RegState, u32> {
        // ① 零 QEMU 纪律闸：任何登记态、任何判据号的 QEMU 轨请求一律抓，
        //    账面只记"被抓笔数"，逐行引用计数不增一笔。
        if action == RegAction::ExecuteQemu {
            self.qemu_blocked = self.qemu_blocked.saturating_add(1);
            return Err(RegDiag::EQemuDep.val());
        }
        if action == RegAction::Declare {
            // ② 容量闸：满员后再声明显性拒（容量耗尽显性不静默）。
            if self.rows.len() >= self.cap {
                return Err(RegDiag::EFull.val());
            }
            // ③ 闭集闸。
            if frozen_index(id).is_none() {
                return Err(RegDiag::EIdRange.val());
            }
            // ④ 重复声明闸。
            if self.row_of(id).is_some() {
                return Err(RegDiag::EDupDecl.val());
            }
            // 预算行闸：预算行为零的行不许入账（冻结表缺账即拒）。
            let budget = match frozen_row(id) {
                Some(r) => r.budget,
                None => 0,
            };
            if budget == 0 {
                return Err(RegDiag::EBudget.val());
            }
            self.rows.push(RegRowLive {
                id,
                state: RegState::PendingRegistered,
                budget,
                exec_seq: 0,
                qemu_refs: 0,
            });
            return Ok(RegState::PendingRegistered);
        }
        // ③ 闭集闸（执行类动作同受闭集约束）。
        if frozen_index(id).is_none() {
            return Err(RegDiag::EIdRange.val());
        }
        // ④ 未登记闸：未登记判据不可确认更不可执行。
        let cur = match self.row_of(id) {
            Some(r) => r.state,
            None => return Err(RegDiag::ENotDeclared.val()),
        };
        // ⑤ 单调闸：目标态必须恰为当前态 +1。
        let target = match action {
            RegAction::ConfirmGate => RegState::Registered,
            RegAction::ExecuteReal => RegState::Executed,
            // QEMU 已在闸①拦下，声明分支已返回，此处不可达。
            RegAction::Declare | RegAction::ExecuteQemu => RegState::PendingRegistered,
        };
        if target.rank() < cur.rank() {
            return Err(RegDiag::ERewind.val());
        }
        if target.rank() != cur.rank() + 1 {
            return Err(RegDiag::EJumpState.val());
        }
        let seq = self.exec_seq_next;
        let row = match self.row_of_mut(id) {
            Some(r) => r,
            None => return Err(RegDiag::ENotDeclared.val()),
        };
        row.state = target;
        if target == RegState::Executed {
            row.exec_seq = seq;
            self.exec_seq_next = seq.saturating_add(1);
        }
        Ok(target)
    }
}

// ── 双机场景构建（确定性，双跑一致） ─────────────────────────────
/// 构建全量登记账：64 行全声明 → 全量推进到"随闸门补测"（Registered）→
/// 前 EXEC_DEMO 条真机轨执行留痕（执行序 1..=EXEC_DEMO）。
/// 全程确定性，两次构建必须逐位一致。
pub fn build_full_book() -> RegBook {
    let mut b = RegBook::new(LIVE_CAP);
    // 全量声明（闭集序）。
    let mut i = 0u32;
    while i < REG_CAP as u32 {
        let _ = b.judge_register(REG_ID_BASE + i, RegAction::Declare);
        i += 1;
    }
    // 全量推进：PendingRegistered → Registered（随闸门补测登记）。
    let mut j = 0u32;
    while j < REG_CAP as u32 {
        let _ = b.judge_register(REG_ID_BASE + j, RegAction::ConfirmGate);
        j += 1;
    }
    // 真机轨预执行（前 EXEC_DEMO 条，执行序留痕）。
    let mut k = 0u32;
    while k < EXEC_DEMO {
        let _ = b.judge_register(REG_ID_BASE + k, RegAction::ExecuteReal);
        k += 1;
    }
    b
}

/// FNV-1a 指纹：覆盖全账行态 + 执行序 + 执行序游标 + QEMU 点验账。
/// 双机一致 = 两次独立构建指纹逐位一致。
pub fn book_fingerprint(b: &RegBook) -> u32 {
    let mut h = 0x811C9DC5u32;
    h = h.wrapping_mul(0x01000193).wrapping_add(b.len() as u32);
    let mut i = 0usize;
    while i < b.len() {
        if let Some(r) = b.row_at(i) {
            h = h.wrapping_mul(0x01000193).wrapping_add(r.id);
            h = h.wrapping_mul(0x01000193).wrapping_add(r.state.rank() as u32);
            h = h.wrapping_mul(0x01000193).wrapping_add(r.budget);
            h = h.wrapping_mul(0x01000193).wrapping_add(r.exec_seq);
            h = h.wrapping_mul(0x01000193).wrapping_add(r.qemu_refs);
        }
        i += 1;
    }
    h = h.wrapping_mul(0x01000193).wrapping_add(b.exec_seq_next());
    h = h.wrapping_mul(0x01000193).wrapping_add(b.qemu_blocked());
    h
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

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
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

// ── 注入器三类（各 10 轮确定性全抓） ─────────────────────────────
/// 注入器报告：抓到路数 / 总路数 / 首个逃脱路描述（空 = 无逃脱）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepReport {
    /// 被判定器抓住的注入轮数。
    pub caught: usize,
    /// 注入总轮数。
    pub total: usize,
    /// 首个逃脱轮描述（空串 = 全抓）。
    pub escape: String,
}

/// 注入器一：跳级执行注入，10 轮全抓。
/// 轮转三变体：未登记直执 / Pending 跳级 Executed / 已执行同秩重执。
pub fn jump_injection_sweep() -> SweepReport {
    let mut rep = SweepReport {
        caught: 0,
        total: 0,
        escape: String::new(),
    };
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let id = REG_ID_BASE + rng.below(REG_CAP as u32);
        let mut b = RegBook::new(LIVE_CAP);
        let expect = match k % 3 {
            // 未登记判据直接执行：ENotDeclared。
            0 => RegDiag::ENotDeclared.val(),
            // 已声明未登记，Pending → Executed 跳级：EJumpState。
            1 => {
                let _ = b.judge_register(id, RegAction::Declare);
                RegDiag::EJumpState.val()
            }
            // 已执行判据同秩重执行：EJumpState（同秩重入显性拒）。
            _ => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                let _ = b.judge_register(id, RegAction::ExecuteReal);
                RegDiag::EJumpState.val()
            }
        };
        rep.total += 1;
        if b.judge_register(id, RegAction::ExecuteReal) == Err(expect) {
            rep.caught += 1;
        } else if rep.escape.is_empty() {
            rep.escape = format!("跳级注入第{}轮逃脱：id={} 期待码非命", k, id);
        }
        k += 1;
    }
    rep
}

/// 注入器二：QEMU 依赖注入，10 轮全抓。
/// 轮转四变体（未登记/Pending/Registered/Executed 各态打 QEMU 轨请求），
/// 每轮同时点验：逐行 QEMU 引用计数恒 0、被抓笔数逐轮记账。
pub fn qemu_injection_sweep() -> SweepReport {
    let mut rep = SweepReport {
        caught: 0,
        total: 0,
        escape: String::new(),
    };
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut k = 0usize;
    while k < 10 {
        let id = REG_ID_BASE + rng.below(REG_CAP as u32);
        let mut b = RegBook::new(LIVE_CAP);
        match k % 4 {
            0 => {}
            1 => {
                let _ = b.judge_register(id, RegAction::Declare);
            }
            2 => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
            }
            _ => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                let _ = b.judge_register(id, RegAction::ExecuteReal);
            }
        }
        rep.total += 1;
        let hit = b.judge_register(id, RegAction::ExecuteQemu) == Err(RegDiag::EQemuDep.val());
        // 零 QEMU 纪律点验：账面引用计数恒 0，被抓笔数恰 1。
        let refs_zero = b.qemu_ref_total() == 0;
        let blocked_once = b.qemu_blocked() == 1;
        if hit && refs_zero && blocked_once {
            rep.caught += 1;
        } else if rep.escape.is_empty() {
            rep.escape = format!(
                "QEMU 注入第{}轮逃脱：命中{} 引用零{} 记账{}",
                k, hit, refs_zero, blocked_once
            );
        }
        k += 1;
    }
    rep
}

/// 注入器三：登记篡改注入，10 轮全抓。
/// 轮转四变体：已执行倒退确认（ERewind）/ 已登记重复声明（EDupDecl）/
/// 已登记同秩重确认（EJumpState）/ 已执行重复声明（EDupDecl）。
pub fn tamper_sweep() -> SweepReport {
    let mut rep = SweepReport {
        caught: 0,
        total: 0,
        escape: String::new(),
    };
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0usize;
    while k < 10 {
        let id = REG_ID_BASE + rng.below(REG_CAP as u32);
        let mut b = RegBook::new(LIVE_CAP);
        let (action, expect) = match k % 4 {
            // 已执行 → 请求确认（目标秩 1 < 当前秩 2）：倒退。
            0 => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                let _ = b.judge_register(id, RegAction::ExecuteReal);
                (RegAction::ConfirmGate, RegDiag::ERewind.val())
            }
            // 已登记 → 重复声明。
            1 => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                (RegAction::Declare, RegDiag::EDupDecl.val())
            }
            // 已登记 → 同秩重确认（目标秩 = 当前秩）：同秩重入。
            2 => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                (RegAction::ConfirmGate, RegDiag::EJumpState.val())
            }
            // 已执行 → 重复声明。
            _ => {
                let _ = b.judge_register(id, RegAction::Declare);
                let _ = b.judge_register(id, RegAction::ConfirmGate);
                let _ = b.judge_register(id, RegAction::ExecuteReal);
                (RegAction::Declare, RegDiag::EDupDecl.val())
            }
        };
        rep.total += 1;
        if b.judge_register(id, action) == Err(expect) {
            rep.caught += 1;
        } else if rep.escape.is_empty() {
            rep.escape = format!("登记篡改第{}轮逃脱：id={} 期待码非命", k, id);
        }
        k += 1;
    }
    rep
}

// ── 真实错误路径校验器（Result 语义，诊断码闭集消费口） ──────────
/// 冻结登记表完整性校验：行数 = 闭集容量、判据号连续、预算行全非零。
/// 任一破缺即 EFrozen（闭集被改写须整表回滚重审）。
pub fn verify_frozen_table() -> Result<(), u32> {
    if REG_ROWS.len() != REG_CAP {
        return Err(RegDiag::EFrozen.val());
    }
    let mut i = 0usize;
    while i < REG_CAP {
        let r = match REG_ROWS.get(i) {
            Some(r) => r,
            None => return Err(RegDiag::EFrozen.val()),
        };
        if r.id != REG_ID_BASE + i as u32 {
            return Err(RegDiag::EFrozen.val());
        }
        if r.budget == 0 {
            return Err(RegDiag::EBudget.val());
        }
        i += 1;
    }
    Ok(())
}

/// 执行序链校验：已执行行的执行序必须 1..n 连续递增（按账内行序），
/// 游标 = n+1，断链即 ESeqMiss。
pub fn verify_exec_chain(b: &RegBook) -> Result<(), u32> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(r) = b.row_at(i) {
            if r.exec_seq > 0 {
                if r.exec_seq != n + 1 {
                    return Err(RegDiag::ESeqMiss.val());
                }
                n += 1;
            }
        }
        i += 1;
    }
    if b.exec_seq_next() != n + 1 {
        return Err(RegDiag::ESeqMiss.val());
    }
    Ok(())
}

/// 全量登记校验：账满闭集、每行登记态 ≥ Registered（随闸门补测）、
/// 预算行与冻结表逐行一致。任一不满足即 ENotFull。
pub fn verify_full_register(b: &RegBook) -> Result<(), u32> {
    if b.len() != REG_CAP {
        return Err(RegDiag::ENotFull.val());
    }
    let mut i = 0usize;
    while i < b.len() {
        let r = match b.row_at(i) {
            Some(r) => r,
            None => return Err(RegDiag::ENotFull.val()),
        };
        if r.state.rank() < RegState::Registered.rank() {
            return Err(RegDiag::ENotFull.val());
        }
        match frozen_row(r.id) {
            Some(fr) => {
                if fr.budget != r.budget {
                    return Err(RegDiag::ENotFull.val());
                }
            }
            None => return Err(RegDiag::ENotFull.val()),
        }
        i += 1;
    }
    Ok(())
}

/// 双机指纹校验：两次独立构建指纹与账面逐位一致，漂移即 EFingerprint。
pub fn dual_fingerprint() -> Result<u32, u32> {
    let b1 = build_full_book();
    let b2 = build_full_book();
    let f1 = book_fingerprint(&b1);
    let f2 = book_fingerprint(&b2);
    if f1 != f2 || b1 != b2 {
        return Err(RegDiag::EFingerprint.val());
    }
    Ok(f1)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在复测资产库段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x831C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    for d in REG_DIAG_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 已知码必须命中文案，未知码必须回落"未知"。
    if detail(RegDiag::EIdRange.val()) != RegDiag::EIdRange.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 冻结登记表完整性（经 Result 校验器）。
fn frozen_table_ok() -> bool {
    verify_frozen_table().is_ok()
}

/// 正常流：声明 → 随闸门补测登记 → 真机执行，三态全链推进落账。
fn happy_path_ok() -> bool {
    let mut b = RegBook::new(LIVE_CAP);
    let id = REG_ID_BASE + 7;
    if b.judge_register(id, RegAction::Declare) != Ok(RegState::PendingRegistered) {
        return false;
    }
    if b.judge_register(id, RegAction::ConfirmGate) != Ok(RegState::Registered) {
        return false;
    }
    if b.judge_register(id, RegAction::ExecuteReal) != Ok(RegState::Executed) {
        return false;
    }
    match b.row_of(id) {
        Some(r) => r.state == RegState::Executed && r.exec_seq == 1 && r.qemu_refs == 0,
        None => false,
    }
}

/// 判定次序：未登记不可确认/执行；声明后执行仍拒（跳级）；确认放行。
fn judge_order_ok() -> bool {
    let mut b = RegBook::new(LIVE_CAP);
    let id = REG_ID_BASE + 3;
    if b.judge_register(id, RegAction::ConfirmGate) != Err(RegDiag::ENotDeclared.val()) {
        return false;
    }
    if b.judge_register(id, RegAction::ExecuteReal) != Err(RegDiag::ENotDeclared.val()) {
        return false;
    }
    let _ = b.judge_register(id, RegAction::Declare);
    if b.judge_register(id, RegAction::ExecuteReal) != Err(RegDiag::EJumpState.val()) {
        return false;
    }
    b.judge_register(id, RegAction::ConfirmGate) == Ok(RegState::Registered)
}

/// 三态秩单调：PendingRegistered < Registered < Executed，顶格秩 = 2。
fn rank_ok() -> bool {
    RegState::PendingRegistered.rank() < RegState::Registered.rank()
        && RegState::Registered.rank() < RegState::Executed.rank()
        && RegState::Executed.rank() == 2
}

/// 全量登记"随闸门补测"绿：64 行全部登记态 ≥ Registered，预算行逐行
/// 与冻结表一致，前 EXEC_DEMO 条已真机执行留痕。
fn register_full_ok() -> bool {
    let b = build_full_book();
    if verify_full_register(&b).is_err() {
        return false;
    }
    let mut executed = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(r) = b.row_at(i) {
            if r.state == RegState::Executed {
                executed += 1;
            }
        }
        i += 1;
    }
    executed == EXEC_DEMO && verify_exec_chain(&b).is_ok()
}

/// 未登记跳级点验：账外判据直执被拒且账面零增长。
fn undeclared_jump_ok() -> bool {
    let mut b = RegBook::new(LIVE_CAP);
    let id = REG_ID_BASE + 40;
    let before = b.len();
    let hit = b.judge_register(id, RegAction::ExecuteReal) == Err(RegDiag::ENotDeclared.val());
    hit && b.len() == before
}

/// 注入器一必抓：跳级执行 10 轮全数命中，无逃脱。
fn jump_caught_ok() -> bool {
    let rep = jump_injection_sweep();
    rep.total == 10 && rep.caught == 10 && rep.escape.is_empty()
}

/// 注入器二必抓：QEMU 依赖注入 10 轮全数命中，引用计数恒 0。
fn qemu_caught_ok() -> bool {
    let rep = qemu_injection_sweep();
    if rep.total != 10 || rep.caught != 10 || !rep.escape.is_empty() {
        return false;
    }
    // 点验口复核：全量账的 QEMU 引用计数恒 0、被抓账可复算。
    let b = build_full_book();
    b.qemu_ref_total() == 0
}

/// 注入器三必抓：登记篡改 10 轮全数命中，无逃脱。
fn tamper_caught_ok() -> bool {
    let rep = tamper_sweep();
    rep.total == 10 && rep.caught == 10 && rep.escape.is_empty()
}

/// 双机一致指纹：两次独立构建账面与指纹逐位一致，三类注入报告亦一致。
fn dual_run_ok() -> bool {
    if dual_fingerprint().is_err() {
        return false;
    }
    let j1 = jump_injection_sweep();
    let j2 = jump_injection_sweep();
    let q1 = qemu_injection_sweep();
    let q2 = qemu_injection_sweep();
    let t1 = tamper_sweep();
    let t2 = tamper_sweep();
    j1 == j2 && q1 == q2 && t1 == t2
}

/// 边界四类：首判据号 / 尾判据号入账放行，上越界 / 下越界显性拒。
fn boundary_ok() -> bool {
    let mut b = RegBook::new(LIVE_CAP);
    // 首 id：放行，落 PendingRegistered。
    if b.judge_register(REG_ID_BASE, RegAction::Declare) != Ok(RegState::PendingRegistered) {
        return false;
    }
    // 尾 id：放行。
    let tail = REG_ID_BASE + REG_CAP as u32 - 1;
    if b.judge_register(tail, RegAction::Declare) != Ok(RegState::PendingRegistered) {
        return false;
    }
    // 上越界：尾 id +1 拒。
    if b.judge_register(REG_ID_BASE + REG_CAP as u32, RegAction::Declare)
        != Err(RegDiag::EIdRange.val())
    {
        return false;
    }
    // 下越界：首 id -1 拒。
    if b.judge_register(REG_ID_BASE - 1, RegAction::Declare) != Err(RegDiag::EIdRange.val()) {
        return false;
    }
    true
}

/// 容量耗尽：64 行满员后任何声明（含闭集内重复号与闭集外号）一律 EFull。
fn capacity_ok() -> bool {
    let mut b = RegBook::new(LIVE_CAP);
    let mut i = 0u32;
    while i < REG_CAP as u32 {
        if b.judge_register(REG_ID_BASE + i, RegAction::Declare).is_err() {
            return false;
        }
        i += 1;
    }
    if b.len() != LIVE_CAP {
        return false;
    }
    // 容量闸先于闭集/重复闸：满员后一律 EFull。
    b.judge_register(REG_ID_BASE, RegAction::Declare) == Err(RegDiag::EFull.val())
        && b.judge_register(REG_ID_BASE + REG_CAP as u32, RegAction::Declare)
            == Err(RegDiag::EFull.val())
}

/// 执行序留痕：前 EXEC_DEMO 条执行序 1..=EXEC_DEMO 连续，游标 = n+1，
/// 未执行行序号恒 0。
fn exec_seq_ok() -> bool {
    let b = build_full_book();
    if b.exec_seq_next() != EXEC_DEMO + 1 {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        if let Some(r) = b.row_at(i) {
            let off = match r.id.checked_sub(REG_ID_BASE) {
                Some(off) => off,
                None => return false,
            };
            let expect_seq = if off < EXEC_DEMO { off + 1 } else { 0 };
            let expect_state = if off < EXEC_DEMO {
                RegState::Executed
            } else {
                RegState::Registered
            };
            if r.exec_seq != expect_seq || r.state != expect_state {
                return false;
            }
        }
        i += 1;
    }
    verify_exec_chain(&b).is_ok()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12795 自检（判据 UNX-F12796-J1：双轨声明判据绿 + 零 QEMU 纪律）。
pub fn run_unx12795_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12795");
    let book = build_full_book();

    s.add_dyn(
        "F12795-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12795-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            REG_DIAG_ALL.len(),
            REG_DIAG_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12795-03 冻结登记表完整性",
        frozen_table_ok(),
        &format!(
            "rows={} 首=({},{}) 尾=({},{})",
            REG_ROWS.len(),
            REG_ROWS.first().map(|r| r.id).unwrap_or(0),
            REG_ROWS.first().map(|r| r.budget).unwrap_or(0),
            REG_ROWS.last().map(|r| r.id).unwrap_or(0),
            REG_ROWS.last().map(|r| r.budget).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12795-04 正常流三态推进",
        happy_path_ok(),
        "声明→随闸门补测登记→真机执行 全链落账",
    );

    s.add_dyn(
        "F12795-05 判定次序闸",
        judge_order_ok(),
        "未登记拒确认/拒执行，声明后执行仍拒，确认放行",
    );

    s.add_dyn(
        "F12795-06 登记态秩单调",
        rank_ok(),
        "PendingRegistered<Registered<Executed 顶格秩=2",
    );

    s.add_dyn(
        "F12795-07 全量登记随闸门补测",
        register_full_ok(),
        &format!(
            "rows={} executed={} qemu_refs={}",
            book.len(),
            EXEC_DEMO,
            book.qemu_ref_total()
        ),
    );

    s.add_dyn(
        "F12795-08 未登记跳级拒",
        undeclared_jump_ok(),
        "账外判据直执被拒且账面零增长",
    );

    // 注入器一：跳级执行 10 轮全抓（人为构造违约必被拒）。
    let jr = jump_injection_sweep();
    s.add_dyn(
        "F12795-09 跳级执行注入 10/10 必抓",
        jr.caught == 10 && jr.total == 10 && jr.escape.is_empty(),
        &format!("caught={}/{} {}", jr.caught, jr.total, jr.escape),
    );

    // 注入器二：QEMU 依赖注入 10 轮全抓，引用计数恒 0（零 QEMU 纪律）。
    let qr = qemu_injection_sweep();
    s.add_dyn(
        "F12795-10 QEMU 依赖注入 10/10 必抓",
        qr.caught == 10 && qr.total == 10 && qr.escape.is_empty(),
        &format!(
            "caught={}/{} {} refs={}",
            qr.caught,
            qr.total,
            qr.escape,
            book.qemu_ref_total()
        ),
    );

    // 注入器三：登记篡改 10 轮全抓。
    let tr = tamper_sweep();
    s.add_dyn(
        "F12795-11 登记篡改注入 10/10 必抓",
        tr.caught == 10 && tr.total == 10 && tr.escape.is_empty(),
        &format!("caught={}/{} {}", tr.caught, tr.total, tr.escape),
    );

    s.add_dyn(
        "F12795-12 双机一致指纹",
        dual_run_ok(),
        &format!("fp=0x{:08x}", book_fingerprint(&book)),
    );

    s.add_dyn(
        "F12795-13 边界与容量耗尽",
        boundary_ok() && capacity_ok(),
        "首/尾判据号放行，上下越界拒，满员后声明一律 EFull",
    );

    s.add_dyn(
        "F12795-14 执行序留痕链",
        exec_seq_ok(),
        &format!("exec_seq_next={} expect={}", book.exec_seq_next(), EXEC_DEMO + 1),
    );

    // 收口：判据条数声明（本节共 15 条，含本条）。
    let declared_total = 15usize;
    let actual_total = s.len() + 1;
    s.add_dyn(
        "F12795-15 收口：判据条数声明",
        actual_total == declared_total,
        &format!("声明{}条 实际{}条", declared_total, actual_total),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在复测资产库段窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(REG_DIAG_ALL.len(), 12);
        let mut i = 0usize;
        while i < REG_DIAG_ALL.len() {
            let vi = REG_DIAG_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < REG_DIAG_ALL.len() {
                let vj = REG_DIAG_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn frozen_table_is_contiguous_with_budget() {
        // 冻结闭集：64 行、判据号连续、预算行全非零。
        assert!(verify_frozen_table().is_ok());
        assert_eq!(REG_ROWS.len(), REG_CAP);
        assert!(REG_CAP >= 60);
        assert_eq!(REG_ID_BASE, 12796);
        let last = REG_ID_BASE + REG_CAP as u32 - 1;
        assert_eq!(last, 12859);
        let mut i = 0usize;
        while i < REG_CAP {
            let r = REG_ROWS.get(i).expect("闭集内行必存在");
            assert_eq!(r.id, REG_ID_BASE + i as u32);
            assert!(r.budget > 0);
            i += 1;
        }
    }

    #[test]
    fn happy_path_and_full_register() {
        // 正常流：单行全链 + 全量登记"随闸门补测"绿。
        assert!(happy_path_ok());
        assert!(judge_order_ok());
        assert!(rank_ok());
        let b = build_full_book();
        assert_eq!(b.len(), REG_CAP);
        assert!(verify_full_register(&b).is_ok());
        assert!(register_full_ok());
        // 未登记跳级点验。
        assert!(undeclared_jump_ok());
    }

    #[test]
    fn jump_injection_all_caught() {
        // 注入器一：跳级执行 10 轮全抓。
        let rep = jump_injection_sweep();
        assert_eq!(rep.total, 10);
        assert_eq!(rep.caught, 10, "逃脱路：{}", rep.escape);
        assert!(rep.escape.is_empty());
        assert!(jump_caught_ok());
    }

    #[test]
    fn qemu_injection_all_caught_and_refs_zero() {
        // 注入器二：QEMU 依赖注入 10 轮全抓，引用计数恒 0。
        let rep = qemu_injection_sweep();
        assert_eq!(rep.total, 10);
        assert_eq!(rep.caught, 10, "逃脱路：{}", rep.escape);
        let b = build_full_book();
        assert_eq!(b.qemu_ref_total(), 0);
        assert!(qemu_caught_ok());
    }

    #[test]
    fn tamper_injection_all_caught() {
        // 注入器三：登记篡改 10 轮全抓（倒退/重复声明/同秩重入）。
        let rep = tamper_sweep();
        assert_eq!(rep.total, 10);
        assert_eq!(rep.caught, 10, "逃脱路：{}", rep.escape);
        assert!(tamper_caught_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双机一致：两次独立构建指纹与账面逐位一致，注入报告亦一致。
        match dual_fingerprint() {
            Ok(fp) => assert_ne!(fp, 0),
            Err(e) => panic!("双机指纹漂移：0x{:05x}", e),
        }
        assert!(dual_run_ok());
        let b1 = build_full_book();
        let b2 = build_full_book();
        assert_eq!(book_fingerprint(&b1), book_fingerprint(&b2));
        assert_eq!(jump_injection_sweep(), jump_injection_sweep());
        assert_eq!(qemu_injection_sweep(), qemu_injection_sweep());
        assert_eq!(tamper_sweep(), tamper_sweep());
    }

    #[test]
    fn boundary_and_capacity_gates() {
        // 边界四类 + 容量耗尽。
        assert!(boundary_ok());
        assert!(capacity_ok());
        // 容量闸先于闭集闸的显性次序。
        let mut b = RegBook::new(LIVE_CAP);
        let mut i = 0u32;
        while i < REG_CAP as u32 {
            let _ = b.judge_register(REG_ID_BASE + i, RegAction::Declare);
            i += 1;
        }
        assert_eq!(b.len(), LIVE_CAP);
        assert_eq!(
            b.judge_register(REG_ID_BASE, RegAction::Declare),
            Err(RegDiag::EFull.val())
        );
    }

    #[test]
    fn exec_seq_and_rewind_discipline() {
        // 执行序留痕：前 EXEC_DEMO 条序号连续，倒退/同秩重入显性拒。
        assert!(exec_seq_ok());
        let mut b = build_full_book();
        let head = REG_ID_BASE;
        // 已执行 → 确认：倒退。
        assert_eq!(
            b.judge_register(head, RegAction::ConfirmGate),
            Err(RegDiag::ERewind.val())
        );
        // 已执行 → 重执行：同秩重入。
        assert_eq!(
            b.judge_register(head, RegAction::ExecuteReal),
            Err(RegDiag::EJumpState.val())
        );
        // 已执行 → QEMU 轨请求：零 QEMU 纪律优先。
        assert_eq!(
            b.judge_register(head, RegAction::ExecuteQemu),
            Err(RegDiag::EQemuDep.val())
        );
        assert_eq!(b.qemu_ref_total(), 0);
        // 执行序链不受倒退企图影响。
        assert!(verify_exec_chain(&b).is_ok());
        // 闭集外判据号执行同样拒。
        assert_eq!(
            b.judge_register(REG_ID_BASE + REG_CAP as u32, RegAction::ExecuteReal),
            Err(RegDiag::EIdRange.val())
        );
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12795_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
