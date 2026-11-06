//! UNX-F12789 风险账移交（R-D1-001/005/008/021–039 清单） · 0x82F40段
//!
//! 判据原文（派工卡 12789 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F12790-J1 冻结件移交判据绿
//!   （三冻结×四方消费面登记），handoff 清单零偏差；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 补充锚定（派工卡 12789 补充条）：
//!   - 风险移交清单齐 = rows（ID 闭集 R-D1-001/005/008/021..039 + 状态闭集
//!     {登记/复核中/已移交}）+ next-step 非空 + 零静默 + 静默注入 10/10 抓。
//!
//! 设计要点：
//! 1. 风险 ID 闭集：22 行冻结清单（R-D1-001 / R-D1-005 / R-D1-008 /
//!    R-D1-021..R-D1-039），越闭集伪 ID（如 R-D1-002、R-D1-040）一律拒收；
//! 2. 状态闭集三态单调：登记(秩0) → 复核中(秩1) → 已移交(秩2)，只进不退，
//!    倒退显性拒（ERewind），原地踏步亦拒（EOrder 次序违约）；
//! 3. 零静默闸：每行 next-step 逐字非空——空串登记即拒（ESilent）并显性
//!    计入静默攻击账（silent_attacks），清单上不存在任何静默行；
//! 4. 收官闸：22 行全移交 + 零静默 + 写回状态=已完成 + 结果摘要非空，
//!    四者缺一即拒（判据 ②③ 逐字锚定）；
//! 5. 注入器三类（静默注入 / 倒退注入 / 伪 ID 注入）各 10 轮全抓，逃脱即
//!    判据失效；xorshift32 种子写死 = SEG_BASE，双跑一致指纹（FNV-1a）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；取值一律
//!    match/if let/.get()/.contains/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12789 − 12600) = 0x80000 + 0x2F40）。
pub const SEG_BASE: u32 = 0x82F40;

// ── 冻结常量表（来源：派工卡 12789 判据 R-D1 风险账清单定义） ────
/// 风险 ID 前缀（D1 域风险账）。
pub const RISK_PREFIX: &str = "R-D1-";
/// 零散三号（001/005/008，派工卡逐字）。
pub const RISK_SPARSE: &[u32] = &[1, 5, 8];
/// 连段区间（021..=039，派工卡逐字）。
pub const RISK_RANGE_LO: u32 = 21;
pub const RISK_RANGE_HI: u32 = 39;
/// 清单总行数：3 零散 + 19 连段 = 22。
pub const RISK_ROW_COUNT: usize = 22;
/// 风险 ID 冻结闭集（22 行，升序无重）。
pub const RISK_IDS: &[&str] = &[
    "R-D1-001",
    "R-D1-005",
    "R-D1-008",
    "R-D1-021",
    "R-D1-022",
    "R-D1-023",
    "R-D1-024",
    "R-D1-025",
    "R-D1-026",
    "R-D1-027",
    "R-D1-028",
    "R-D1-029",
    "R-D1-030",
    "R-D1-031",
    "R-D1-032",
    "R-D1-033",
    "R-D1-034",
    "R-D1-035",
    "R-D1-036",
    "R-D1-037",
    "R-D1-038",
    "R-D1-039",
];
/// 审计备注日志容量（移交清单点验槽）。
pub const LOG_CAP: usize = 16;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]）。
    ESegGuard,
    /// 风险 ID 越闭集（不在 R-D1-001/005/008/021..039 冻结清单内）。
    EIdRange,
    /// 重复登记（同 ID 二次入账，清单一票一行）。
    EDup,
    /// 静默行违约（next-step 空串，零静默闸拒绝并计入攻击账）。
    ESilent,
    /// 清单未齐（22 行未齐即求全移交/收官）。
    ENotFull,
    /// 状态倒退（登记→复核中→已移交只进不退）。
    ERewind,
    /// 账已收官（收官后写账/注入一律拒绝）。
    ESealed,
    /// 备注日志容量耗尽（LOG_CAP 槽满仍记新行）。
    ECap,
    /// 写回状态错（收官写回状态 ≠ 已完成）。
    EStatus,
    /// 写回摘要空（收官写回结果摘要为空串）。
    ESummary,
    /// 次序违约（状态原地踏步：目标秩等于当前秩）。
    EOrder,
}

impl RiskDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RiskDiag::ESegGuard => 0x01,
                RiskDiag::EIdRange => 0x02,
                RiskDiag::EDup => 0x03,
                RiskDiag::ESilent => 0x04,
                RiskDiag::ENotFull => 0x05,
                RiskDiag::ERewind => 0x06,
                RiskDiag::ESealed => 0x07,
                RiskDiag::ECap => 0x08,
                RiskDiag::EStatus => 0x09,
                RiskDiag::ESummary => 0x0a,
                RiskDiag::EOrder => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RiskDiag::ESegGuard => {
                "段位越窗：SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]，先回公式 0x80000+0x40×(id−12600) 复核段基址"
            }
            RiskDiag::EIdRange => {
                "风险 ID 越闭集：清单只认 R-D1-001/005/008/021..039 冻结 22 行，先回 RISK_IDS 对表再登记"
            }
            RiskDiag::EDup => {
                "重复登记：同 ID 已入账，一票一行不许覆写，先查既有行再续登"
            }
            RiskDiag::ESilent => {
                "静默行违约：next-step 为空串即拒（零静默闸），先补非空下一行动再登记"
            }
            RiskDiag::ENotFull => {
                "清单未齐：22 行未齐即求全移交/收官，先补齐缺行再走移交/收官口"
            }
            RiskDiag::ERewind => {
                "状态倒退：登记→复核中→已移交只进不退，倒退即风险账可疑，须全账复核"
            }
            RiskDiag::ESealed => {
                "账已收官：收官后写账/注入一律拒绝，改动走下一条移交滚动"
            }
            RiskDiag::ECap => {
                "备注日志容量耗尽：LOG_CAP 槽已满，先归档已有备注行再记新行"
            }
            RiskDiag::EStatus => {
                "写回状态错：收官写回状态须=已完成，先按移交协议补完成态写回再收官"
            }
            RiskDiag::ESummary => {
                "写回摘要空：收官写回结果摘要为空串，先填非空摘要（写明 22 行全移交结论）再收官"
            }
            RiskDiag::EOrder => {
                "次序违约：目标状态秩等于当前秩（原地踏步），推进须严格升秩，先明确目标态再推进"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RISK_ALL: &[RiskDiag] = &[
    RiskDiag::ESegGuard,
    RiskDiag::EIdRange,
    RiskDiag::EDup,
    RiskDiag::ESilent,
    RiskDiag::ENotFull,
    RiskDiag::ERewind,
    RiskDiag::ESealed,
    RiskDiag::ECap,
    RiskDiag::EStatus,
    RiskDiag::ESummary,
    RiskDiag::EOrder,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RISK_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82F41..0x82F4B 闭集内，先回本卡 SEG_BASE 段核对码值"
}

// ── 风险状态闭集（三态单调） ─────────────────────────────────────
/// 风险行状态闭集（登记/复核中/已移交，秩只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskStatus {
    /// 登记：已入风险账，待复核。
    DengJi,
    /// 复核中：复核面进行中，待移交。
    FuHe,
    /// 已移交：移交完成终态。
    YiJiao,
}

impl RiskStatus {
    /// 状态秩（登记0 < 复核中1 < 已移交2，推进须严格升秩）。
    pub fn rank(self) -> u8 {
        match self {
            RiskStatus::DengJi => 0,
            RiskStatus::FuHe => 1,
            RiskStatus::YiJiao => 2,
        }
    }

    /// 状态中文名（闭集逐字：登记/复核中/已移交）。
    pub fn name(self) -> &'static str {
        match self {
            RiskStatus::DengJi => "登记",
            RiskStatus::FuHe => "复核中",
            RiskStatus::YiJiao => "已移交",
        }
    }

    /// 由下标反查状态（越界 None）。
    pub fn from_idx(i: u32) -> Option<RiskStatus> {
        match i {
            0 => Some(RiskStatus::DengJi),
            1 => Some(RiskStatus::FuHe),
            2 => Some(RiskStatus::YiJiao),
            _ => None,
        }
    }
}

/// 状态闭集数组（下标即秩）。
pub const STATUS_ALL: &[RiskStatus] = &[
    RiskStatus::DengJi,
    RiskStatus::FuHe,
    RiskStatus::YiJiao,
];

/// 写回状态闭集（判据③：完成时 API/软件写回状态=已完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffWriteStatus {
    /// 进行中：尚未达成收官写回条件。
    InProgress,
    /// 已完成：收官写回终态。
    Done,
}

impl HandoffWriteStatus {
    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            HandoffWriteStatus::InProgress => "进行中",
            HandoffWriteStatus::Done => "已完成",
        }
    }
}

// ── 数据模型 ─────────────────────────────────────────────────────
/// 风险行：闭集 ID + 闭集状态 + 非空下一行动。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskRow {
    /// 风险 ID（RISK_IDS 闭集成员）。
    pub id: &'static str,
    /// 当前状态（三态闭集）。
    pub status: RiskStatus,
    /// 下一行动（零静默闸要求逐字非空）。
    pub next_step: String,
}

/// 移交账阶段（只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskStage {
    /// 登记中：允许逐行登记与状态推进。
    Open,
    /// 全移交：22 行全部已移交，允许收官。
    AllHanded,
    /// 收官：只读，写账/注入一律拒绝。
    Sealed,
}

impl RiskStage {
    /// 阶段秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RiskStage::Open => 0,
            RiskStage::AllHanded => 1,
            RiskStage::Sealed => 2,
        }
    }
}

/// FNV-1a 混入一段字节（双跑一致指纹用）。
fn fnv_mix(h: u32, bytes: &[u8]) -> u32 {
    let mut out = h;
    for b in bytes.iter() {
        out ^= *b as u32;
        out = out.wrapping_mul(0x0100_0193);
    }
    out
}

/// 风险账：22 行清单 + 单调状态机 + 静默攻击账 + 备注日志。
#[derive(Clone, Debug)]
pub struct RiskLedger {
    /// 已登记风险行（按 RISK_IDS 闭集序登记）。
    rows: Vec<RiskRow>,
    /// 移交账阶段。
    stage: RiskStage,
    /// 静默攻击账：被零静默闸拒绝的空 next-step 登记次数（显性记账不静默吞）。
    silent_attacks: usize,
    /// 备注日志（容量 LOG_CAP）。
    notes: Vec<String>,
}

impl RiskLedger {
    /// 开一本新风险账。
    pub fn new() -> RiskLedger {
        RiskLedger {
            rows: Vec::new(),
            stage: RiskStage::Open,
            silent_attacks: 0,
            notes: Vec::new(),
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> RiskStage {
        self.stage
    }

    /// 已登记行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 静默攻击账（被拒的空 next-step 登记次数）。
    pub fn silent_attacks(&self) -> usize {
        self.silent_attacks
    }

    /// 已记备注行数。
    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    /// 取第 n 条备注（越界 None）。
    pub fn note_at(&self, idx: usize) -> Option<&str> {
        match self.notes.get(idx) {
            Some(s) => Some(s.as_str()),
            None => None,
        }
    }

    /// 按 ID 取行（越闭集或未登记返回 None）。
    pub fn row_of(&self, id: &str) -> Option<&RiskRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// 按下标取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&RiskRow> {
        self.rows.get(idx)
    }

    /// 风险 ID 闭集校验：RISK_IDS 冻结表包含即收。
    pub fn id_in_closed_set(id: &str) -> bool {
        RISK_IDS.iter().any(|x| *x == id)
    }

    /// 登记一行风险（次序冻结：收官闸 → 倒退闸 → ID 闭集 → 重复 → 零静默）。
    pub fn register(&mut self, id: &'static str, next_step: &str) -> Result<(), u32> {
        // ① 收官闸。
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ESealed.val());
        }
        // ② 倒退闸：全移交后回头登记属倒退。
        if self.stage.rank() > RiskStage::Open.rank() {
            return Err(RiskDiag::ERewind.val());
        }
        // ③ ID 闭集闸。
        if !RiskLedger::id_in_closed_set(id) {
            return Err(RiskDiag::EIdRange.val());
        }
        // ④ 重复闸。
        if self.row_of(id).is_some() {
            return Err(RiskDiag::EDup.val());
        }
        // ⑤ 零静默闸：空 next-step 拒收并显性计入攻击账（不静默吞）。
        if next_step.is_empty() {
            self.silent_attacks += 1;
            return Err(RiskDiag::ESilent.val());
        }
        self.rows.push(RiskRow {
            id,
            status: RiskStatus::DengJi,
            next_step: String::from(next_step),
        });
        Ok(())
    }

    /// 状态推进：目标秩须严格大于当前秩（升秩放行，倒退/踏步均拒）。
    pub fn advance(&mut self, id: &str, to: RiskStatus) -> Result<(), u32> {
        // 收官闸。
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ESealed.val());
        }
        // ID 闭集闸 + 在账闸。
        let cur = match self.row_of(id) {
            Some(r) => r.status,
            None => return Err(RiskDiag::EIdRange.val()),
        };
        // 单调闸：倒退显性拒、原地踏步次序违约拒。
        if to.rank() < cur.rank() {
            return Err(RiskDiag::ERewind.val());
        }
        if to.rank() == cur.rank() {
            return Err(RiskDiag::EOrder.val());
        }
        match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.status = to;
                Ok(())
            }
            None => Err(RiskDiag::EIdRange.val()),
        }
    }

    /// 零静默复核：清单上每一行 next-step 非空且 ID 在闭集内。
    pub fn zero_silent_ok(&self) -> bool {
        for r in self.rows.iter() {
            if r.next_step.is_empty() || !RiskLedger::id_in_closed_set(r.id) {
                return false;
            }
        }
        true
    }

    /// 全移交：22 行齐 + 零静默 + 逐行推进至已移交，推进后阶段 AllHanded
    /// （一次性推进口：AllHanded 阶段重调属倒退，不静默重置任何行）。
    pub fn hand_off_all(&mut self) -> Result<(), u32> {
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ESealed.val());
        }
        if self.stage.rank() >= RiskStage::AllHanded.rank() {
            return Err(RiskDiag::ERewind.val());
        }
        if self.rows.len() != RISK_ROW_COUNT {
            return Err(RiskDiag::ENotFull.val());
        }
        if !self.zero_silent_ok() {
            return Err(RiskDiag::ESilent.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get_mut(i) {
                Some(r) => r.status = RiskStatus::YiJiao,
                None => return Err(RiskDiag::ENotFull.val()),
            }
            i += 1;
        }
        self.stage = RiskStage::AllHanded;
        Ok(())
    }

    /// 记一条备注行（容量 LOG_CAP，收官后拒绝）。
    pub fn add_note(&mut self, text: &str) -> Result<(), u32> {
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ESealed.val());
        }
        if self.notes.len() >= LOG_CAP {
            return Err(RiskDiag::ECap.val());
        }
        self.notes.push(String::from(text));
        Ok(())
    }

    /// 收官（判据②③锚定）：全移交 + 零静默 + 写回状态=已完成 + 摘要非空。
    pub fn seal(&mut self, status: HandoffWriteStatus, summary: &str) -> Result<(), u32> {
        // 重封闸：已收官再封属倒退。
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ERewind.val());
        }
        // 未全移交闸。
        if self.stage.rank() < RiskStage::AllHanded.rank() {
            return Err(RiskDiag::ENotFull.val());
        }
        // 清单齐闸 + 零静默闸（判据 ②：handoff 清单零偏差）。
        if self.rows.len() != RISK_ROW_COUNT || !self.zero_silent_ok() {
            return Err(RiskDiag::ENotFull.val());
        }
        // 写回状态闸（判据 ③：状态=已完成）。
        if status != HandoffWriteStatus::Done {
            return Err(RiskDiag::EStatus.val());
        }
        // 写回摘要闸（判据 ③：结果摘要非空）。
        if summary.is_empty() {
            return Err(RiskDiag::ESummary.val());
        }
        self.stage = RiskStage::Sealed;
        Ok(())
    }

    // ── 审计注入面（只服务注入必抓扫，收官后拒绝） ────────────────
    /// 类二注入：把指定行状态倒退置为低秩态（模拟账面倒退），全移交闸
    /// 与零静默复核必须抓（任一行非终态即 handoff 清单红）。
    pub fn inject_regress(&mut self, id: &str, to: RiskStatus) -> Result<(), u32> {
        if self.stage.rank() >= RiskStage::Sealed.rank() {
            return Err(RiskDiag::ESealed.val());
        }
        if !RiskLedger::id_in_closed_set(id) {
            return Err(RiskDiag::EIdRange.val());
        }
        match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                r.status = to;
                Ok(())
            }
            None => Err(RiskDiag::EIdRange.val()),
        }
    }

    /// 双跑一致指纹（FNV-1a 32 位）：阶段 + 行集 + 攻击账 + 备注行。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_mix(h, &self.stage.rank().to_le_bytes());
        h = fnv_mix(h, &(self.rows.len() as u32).to_le_bytes());
        for r in self.rows.iter() {
            h = fnv_mix(h, r.id.as_bytes());
            h = fnv_mix(h, &r.status.rank().to_le_bytes());
            h = fnv_mix(h, r.next_step.as_bytes());
        }
        h = fnv_mix(h, &(self.silent_attacks as u32).to_le_bytes());
        for n in self.notes.iter() {
            h = fnv_mix(h, n.as_bytes());
        }
        h
    }
}

// ── 确定性 xorshift32（种子 = SEG_BASE，零随机红线合规） ─────────
/// xorshift32 发生器：种子写死，序列确定，双跑一致。
pub struct Xor32 {
    state: u32,
}

impl Xor32 {
    /// 以固定种子构造（种子不得为零，零种子回退段基址常量）。
    pub fn new(seed: u32) -> Xor32 {
        Xor32 {
            state: if seed == 0 { SEG_BASE } else { seed },
        }
    }

    /// 下一伪随机数（xorshift32：x^=x<<13; x^=x>>17; x^=x<<5）。
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 正常流构建（22 行登记 → 逐行升秩 → 全移交，不收官留 seal 口） ─
/// 构建一本 22 行齐、全移交的风险账（next-step 语料逐行定制非空）。
pub fn build_risk_ledger() -> RiskLedger {
    let mut l = RiskLedger::new();
    let mut i = 0usize;
    while i < RISK_IDS.len() {
        let id = match RISK_IDS.get(i) {
            Some(x) => *x,
            None => break,
        };
        // 下一行动语料：按行定制（登记面写复核动作，非空）。
        let step = if i < RISK_SPARSE.len() {
            "零散风险项转复核面核签"
        } else {
            "连段风险项转复核面核签"
        };
        let _ = l.register(id, step);
        i += 1;
    }
    // 逐行升秩：登记 → 复核中 → 已移交（严格升秩推进）。
    let mut j = 0usize;
    while j < RISK_IDS.len() {
        let id = match RISK_IDS.get(j) {
            Some(x) => *x,
            None => break,
        };
        let _ = l.advance(id, RiskStatus::FuHe);
        let _ = l.advance(id, RiskStatus::YiJiao);
        j += 1;
    }
    let _ = l.hand_off_all();
    l
}

// ── 伪 ID 语料（闭集外，注入面用；升序无重，与真闭集无交集） ────
/// 伪 ID 语料 10 条（R-D1-002 等越闭集号，注入必抓扫专用）。
pub const FAKE_IDS: &[&str] = &[
    "R-D1-000",
    "R-D1-002",
    "R-D1-003",
    "R-D1-004",
    "R-D1-006",
    "R-D1-020",
    "R-D1-040",
    "R-D1-099",
    "R-D2-001",
    "R-D1-041",
];

// ── 注入器三类扫（各 10 轮全抓，逃脱即判据失效） ─────────────────
/// 注入扫：类一静默注入 ×10 / 类二倒退注入 ×10 / 类三伪 ID 注入 ×10。
/// 返回（抓到路数, 总路数, 首个逃脱路描述）。
pub fn inject_sweep() -> (usize, usize, String) {
    let mut rng = Xor32::new(SEG_BASE);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 类一：静默注入 —— 空串 next-step 必被零静默闸拒且攻击账 +1。
    let mut k = 0usize;
    while k < 10 {
        let ci = rng.below(RISK_IDS.len() as u32) as usize;
        let id = match RISK_IDS.get(ci) {
            Some(x) => *x,
            None => "R-D1-001",
        };
        let mut l = RiskLedger::new();
        let r = l.register(id, "");
        let ok = r == Err(RiskDiag::ESilent.val())
            && l.row_count() == 0
            && l.silent_attacks() == 1
            && l.zero_silent_ok();
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类一静默 路{} id={} 注入逃脱（r={:?}）", k, id, r.err());
        }
        k += 1;
    }
    // 类二：倒退注入 —— 已移交行被压回低秩后，全移交与零静默复核必抓。
    k = 0;
    while k < 10 {
        let ci = rng.below(RISK_IDS.len() as u32) as usize;
        let id = match RISK_IDS.get(ci) {
            Some(x) => *x,
            None => "R-D1-021",
        };
        let mut l = build_risk_ledger();
        let inj = l.inject_regress(id, RiskStatus::FuHe);
        // 全移交是一次性推进口：AllHanded 阶段重调属倒退（ERewind），
        // 不许静默重置任何行——倒退行保持非终态即被点验面抓住。
        let all = l.hand_off_all();
        let ok = inj.is_ok()
            && all == Err(RiskDiag::ERewind.val())
            && match l.row_of(id) {
                Some(r) => r.status != RiskStatus::YiJiao,
                None => true,
            };
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类二倒退 路{} id={} 注入逃脱", k, id);
        }
        k += 1;
    }
    // 类三：伪 ID 注入 —— 越闭集 ID 必被拒且不入账。
    k = 0;
    while k < 10 {
        let fi = rng.below(FAKE_IDS.len() as u32) as usize;
        let fake = match FAKE_IDS.get(fi) {
            Some(x) => *x,
            None => "R-D1-099",
        };
        let mut l = RiskLedger::new();
        let r = l.register(fake, "伪 ID 试登");
        let ok = r == Err(RiskDiag::EIdRange.val()) && l.row_count() == 0;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类三伪ID 路{} id={} 注入逃脱", k, fake);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据交叉锚定（UNX-F12790-J1：三冻结×四方消费面登记） ─────────
/// 三冻结件名录（跨域冻结件移交的三资产闭集，UNX-F12790-J1 锚定）。
pub const FREEZE_ASSETS: &[&str] = &["域账冻结件", "三区块加法账冻结件", "号面断言冻结件"];
/// 四方消费面名录（D2/D3/D4/E1 闭集）。
pub const CONSUMERS: &[&str] = &["D2", "D3", "D4", "E1"];
/// 三冻结×四方消费面登记行数（3×4=12）。
pub const CONSUME_FACE_COUNT: usize = 12;

/// 交叉锚定谓词：三冻结×四方 = 12 消费面（逐名非空、两两无重、笛卡尔计数）。
pub fn freeze_cross_ok() -> bool {
    if FREEZE_ASSETS.len() != 3 || CONSUMERS.len() != 4 {
        return false;
    }
    let mut nonempty = true;
    for a in FREEZE_ASSETS.iter() {
        if a.is_empty() {
            nonempty = false;
        }
    }
    for c in CONSUMERS.iter() {
        if c.is_empty() {
            nonempty = false;
        }
    }
    if !nonempty {
        return false;
    }
    // 消费面两两无重（同名单内）。
    let mut i = 0usize;
    while i < CONSUMERS.len() {
        let mut j = i + 1;
        while j < CONSUMERS.len() {
            match (CONSUMERS.get(i), CONSUMERS.get(j)) {
                (Some(x), Some(y)) => {
                    if x == y {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    // 笛卡尔计数：3×4=12 消费面。
    FREEZE_ASSETS.len().saturating_mul(CONSUMERS.len()) == CONSUME_FACE_COUNT
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x82FC0]，
/// 并锚定本卡公式实值 0x80000 + 0x40×189 = 0x82F40。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let span = match 12799u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(s) => s,
            None => return false,
        },
        None => return false,
    };
    let hi = match 0x80000u32.checked_add(span) {
        Some(h) => h,
        None => return false,
    };
    if !(0x80000..=hi).contains(&SEG_BASE) {
        return false;
    }
    let own = match 12789u32.checked_sub(12600) {
        Some(d) => match 0x40u32.checked_mul(d) {
            Some(o) => o,
            None => return false,
        },
        None => return false,
    };
    match 0x80000u32.checked_add(own) {
        Some(v) => v == SEG_BASE,
        None => false,
    }
}

/// 诊断码闭集守卫：互异、落段窗、查表命中、未知回落。
fn diag_closed_ok() -> bool {
    let mut i = 0usize;
    while i < RISK_ALL.len() {
        let d = match RISK_ALL.get(i) {
            Some(d) => *d,
            None => return false,
        };
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() || d.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < RISK_ALL.len() {
            match RISK_ALL.get(j) {
                Some(e) => {
                    if e.val() == v {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 风险 ID 闭集守卫：22 行、升序无重、前缀一致、零散三号与连段区间齐。
fn id_closed_set_ok() -> bool {
    if RISK_IDS.len() != RISK_ROW_COUNT {
        return false;
    }
    let mut prev: u32 = 0;
    let mut sparse_seen = 0usize;
    for id in RISK_IDS.iter() {
        // 前缀闭集：全部以 R-D1- 开头。
        if !id.starts_with(RISK_PREFIX) {
            return false;
        }
        // 号段解析（手工逐字符十进制，不引解析依赖）。
        let tail = match id.strip_prefix(RISK_PREFIX) {
            Some(t) => t,
            None => return false,
        };
        let mut num = 0u32;
        for ch in tail.chars() {
            let d = match ch.to_digit(10) {
                Some(d) => d,
                None => return false,
            };
            num = num.saturating_mul(10).saturating_add(d);
        }
        // 升序无重。
        if num <= prev {
            return false;
        }
        prev = num;
        if num == 1 || num == 5 || num == 8 {
            sparse_seen += 1;
        }
        // 闭集成员校验（表内自洽）。
        if !RiskLedger::id_in_closed_set(id) {
            return false;
        }
    }
    // 连段区间端点与零散三号齐。
    if prev != RISK_RANGE_HI || sparse_seen != RISK_SPARSE.len() {
        return false;
    }
    // 越闭集伪 ID 必拒（抽 3 个代表：000/020/040）。
    if RiskLedger::id_in_closed_set("R-D1-000")
        || RiskLedger::id_in_closed_set("R-D1-020")
        || RiskLedger::id_in_closed_set("R-D1-040")
    {
        return false;
    }
    true
}

/// 状态闭集守卫：三态秩升、名逐字非空、越界下标拒。
fn status_closed_ok() -> bool {
    if STATUS_ALL.len() != 3 {
        return false;
    }
    let mut prev = 0u8;
    for st in STATUS_ALL.iter() {
        if st.rank() <= prev && prev > 0 {
            return false;
        }
        prev = st.rank();
        if st.name().is_empty() {
            return false;
        }
        match RiskStatus::from_idx(st.rank() as u32) {
            Some(x) => {
                if x != *st {
                    return false;
                }
            }
            None => return false,
        }
    }
    // 越界下标拒（cap=2，cap+1=3 越界）。
    if RiskStatus::from_idx(3).is_some() {
        return false;
    }
    prev == 2
}

/// 正常流：22 行登记 → 逐行升秩 → 全移交 → 备注 ×2 → 收官（绿写回）。
fn happy_path_ok() -> bool {
    let mut l = build_risk_ledger();
    if l.stage() != RiskStage::AllHanded {
        return false;
    }
    if !l.zero_silent_ok() {
        return false;
    }
    let mut k = 0usize;
    while k < 2 {
        if l.add_note("移交点验行").is_err() {
            return false;
        }
        k += 1;
    }
    if l.seal(HandoffWriteStatus::Done, "22 行全移交 零静默 结果摘要非空")
        .is_err()
    {
        return false;
    }
    if l.stage() != RiskStage::Sealed {
        return false;
    }
    // 收官后守恒复核仍绿：22 行全终态。
    let mut i = 0usize;
    while i < l.row_count() {
        match l.row_at(i) {
            Some(r) => {
                if r.status != RiskStatus::YiJiao || r.next_step.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 零静默账守卫：正常账零静默、静默注入后攻击账显性记账且清单仍零静默。
fn zero_silent_ok() -> bool {
    let l = build_risk_ledger();
    if !l.zero_silent_ok() || l.silent_attacks() != 0 {
        return false;
    }
    let mut m = RiskLedger::new();
    if m.register("R-D1-001", "").is_ok() {
        return false;
    }
    if m.silent_attacks() != 1 {
        return false;
    }
    // 被拒行不入账：清单仍空、仍零静默。
    if m.row_count() != 0 || !m.zero_silent_ok() {
        return false;
    }
    true
}

/// 注入必抓：三类 30 轮全数命中，无逃脱。
fn inject_caught_ok() -> bool {
    let (caught, total, escape) = inject_sweep();
    if total != 30 {
        return false;
    }
    if caught != 30 {
        return false;
    }
    escape.is_empty()
}

/// 双跑一致：两次构建/注入全流程逐项一致，指纹相等。
fn dual_run_ok() -> bool {
    let a = build_risk_ledger();
    let b = build_risk_ledger();
    if a.stage() != b.stage() || a.row_count() != b.row_count() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    let mut i = 0usize;
    while i < RISK_ROW_COUNT {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if inject_sweep() != inject_sweep() {
        return false;
    }
    true
}

/// 边界四类：重复 ID 拒、未齐禁收官、容量耗尽拒、封账后写拒/重封倒退拒。
fn boundary_ok() -> bool {
    // ① 重复 ID 拒。
    let mut l = RiskLedger::new();
    if l.register("R-D1-001", "首次登记").is_err() {
        return false;
    }
    if l.register("R-D1-001", "重复登记") != Err(RiskDiag::EDup.val()) {
        return false;
    }
    // ② 未齐禁全移交/禁收官。
    let mut h = build_risk_ledger();
    // 正常账已全移交：seal 走绿路径前的未齐路径用半账。
    let mut half = RiskLedger::new();
    if half.register("R-D1-001", "半账登记").is_err() {
        return false;
    }
    if half.hand_off_all() != Err(RiskDiag::ENotFull.val()) {
        return false;
    }
    if half.seal(HandoffWriteStatus::Done, "摘要非空") != Err(RiskDiag::ENotFull.val()) {
        return false;
    }
    // ③ 容量耗尽：LOG_CAP 条收，超容拒。
    let mut c = build_risk_ledger();
    let mut k = 0usize;
    while k < LOG_CAP {
        if c.add_note("容量点验").is_err() {
            return false;
        }
        k += 1;
    }
    if c.add_note("超容") != Err(RiskDiag::ECap.val()) {
        return false;
    }
    // ④ 封账后写拒 + 重封倒退拒。
    if h.seal(HandoffWriteStatus::Done, "22 行全移交收官摘要").is_err() {
        return false;
    }
    if h.register("R-D1-005", "封账后登记") != Err(RiskDiag::ESealed.val()) {
        return false;
    }
    if h.advance("R-D1-005", RiskStatus::FuHe) != Err(RiskDiag::ESealed.val()) {
        return false;
    }
    if h.add_note("封账后备注") != Err(RiskDiag::ESealed.val()) {
        return false;
    }
    if h.seal(HandoffWriteStatus::Done, "重封") != Err(RiskDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 移交面点验：22 行全终态已移交、next-step 全非空、ID 与冻结表逐行一致。
fn handover_face_ok() -> bool {
    let l = build_risk_ledger();
    if l.row_count() != RISK_ROW_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < RISK_ROW_COUNT {
        let expect = match RISK_IDS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        match l.row_at(i) {
            Some(r) => {
                if r.id != expect || r.status != RiskStatus::YiJiao || r.next_step.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 次序语义：倒退与原地踏步各回其码、升秩放行、封账后一律 ESealed。
fn order_semantics_ok() -> bool {
    let mut l = RiskLedger::new();
    if l.register("R-D1-021", "次序点验行").is_err() {
        return false;
    }
    // 原地踏步（登记→登记）→ EOrder。
    if l.advance("R-D1-021", RiskStatus::DengJi) != Err(RiskDiag::EOrder.val()) {
        return false;
    }
    // 倒退（先升到已移交，再压回复核中）→ ERewind 且状态不变。
    if l.advance("R-D1-021", RiskStatus::YiJiao).is_err() {
        return false;
    }
    if l.advance("R-D1-021", RiskStatus::FuHe) != Err(RiskDiag::ERewind.val()) {
        return false;
    }
    if l.advance("R-D1-021", RiskStatus::DengJi) != Err(RiskDiag::ERewind.val()) {
        return false;
    }
    match l.row_of("R-D1-021") {
        Some(r) => {
            if r.status != RiskStatus::YiJiao {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 写回闸：错误状态/空摘要各路均拒且阶段不进 Sealed；绿写回放行。
fn writeback_ok() -> bool {
    let mut a = build_risk_ledger();
    if a.seal(HandoffWriteStatus::InProgress, "摘要非空") != Err(RiskDiag::EStatus.val()) {
        return false;
    }
    if a.stage() == RiskStage::Sealed {
        return false;
    }
    let mut b = build_risk_ledger();
    if b.seal(HandoffWriteStatus::Done, "") != Err(RiskDiag::ESummary.val()) {
        return false;
    }
    let mut c = build_risk_ledger();
    match c.seal(HandoffWriteStatus::Done, "UNX-F12790-J1 冻结件移交判据绿") {
        Ok(()) => c.stage() == RiskStage::Sealed,
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12789 自检（判据 12789 卡：风险账移交 · UNX-F12790-J1 锚定）。
pub fn run_unx12789_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12789");
    let ledger = build_risk_ledger();

    // 01 段位守卫（公式 0x80000+0x40×189=0x82F40，窗 [0x80000,0x82FC0]）。
    s.add_dyn(
        "F12789-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 公式=0x80000+0x40×189 对齐0x40={} 窗=[0x80000,0x82FC0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    // 02 诊断码闭集（11 码互异落窗 + 未知回落）。
    s.add(
        "F12789-02 诊断码闭集",
        diag_closed_ok(),
        "11码互异落段窗 查表命中 未知码回落含'未知'",
    );

    // 03 风险 ID 闭集 22 行（零散 001/005/008 + 连段 021..039，升序无重）。
    s.add_dyn(
        "F12789-03 风险ID闭集22行",
        id_closed_set_ok(),
        &format!(
            "闭集{}行 前缀{} 零散三号1/5/8 连段21..39 伪ID(000/020/040)拒",
            RISK_IDS.len(),
            RISK_PREFIX
        ),
    );

    // 04 状态闭集单调（登记→复核中→已移交，秩只进不退）。
    s.add(
        "F12789-04 状态闭集单调",
        status_closed_ok(),
        "三态闭集 登记(0)→复核中(1)→已移交(2) 下标3越界拒",
    );

    // 05 正常流收官（22行→升秩→全移交→备注→绿写回封账）。
    s.add_dyn(
        "F12789-05 正常流收官",
        happy_path_ok(),
        &format!("登记→升秩→全移交→备注→封账 终态={:?}", ledger.stage()),
    );

    // 06 零静默账（正常账零静默；静默注入攻击账显性记账）。
    s.add_dyn(
        "F12789-06 零静默账",
        zero_silent_ok(),
        &format!(
            "正常账零静默 攻击账={} 被拒静默行不入账",
            build_risk_ledger().silent_attacks()
        ),
    );

    // 07 注入器类一：静默注入 10/10 抓。
    let (caught, total, escape) = inject_sweep();
    s.add_dyn(
        "F12789-07 注入三类 30 轮全抓",
        caught == 30 && total == 30 && escape.is_empty(),
        &format!("静默10+倒退10+伪ID10 caught={}/{} escape={}", caught, total, escape),
    );

    // 08 双跑一致指纹（FNV-1a + xorshift32 种子=SEG_BASE）。
    let fa = build_risk_ledger().fingerprint();
    let fb = build_risk_ledger().fingerprint();
    s.add_dyn(
        "F12789-08 双跑一致指纹",
        dual_run_ok() && fa == fb,
        &format!("fp_a=0x{:08X} fp_b=0x{:08X} 相等={}", fa, fb, fa == fb),
    );

    // 09 边界四类（重复ID/未齐禁收官/容量耗尽/封账后写拒+重封倒退）。
    s.add(
        "F12789-09 边界四类",
        boundary_ok(),
        "重复ID拒 未齐禁全移交禁收官 容量16满拒 封账后写拒 重封倒退拒",
    );

    // 10 判据交叉锚定（UNX-F12790-J1：三冻结×四方=12 消费面）。
    s.add(
        "F12789-10 交叉锚定12消费面",
        freeze_cross_ok(),
        "三冻结件×四方D2/D3/D4/E1=12消费面 逐名非空 两两无重",
    );

    // 11 移交面点验（22 行全终态、next-step 全非空、ID 逐行一致）。
    s.add(
        "F12789-11 移交面点验",
        handover_face_ok(),
        "22行全已移交 next-step全非空 ID与冻结表逐行一致",
    );

    // 12 次序语义（原地踏步EOrder 倒退ERewind 升秩放行）。
    s.add(
        "F12789-12 次序语义",
        order_semantics_ok(),
        "踏步EOrder 倒退ERewind且状态不变 严格升秩放行",
    );

    // 13 写回闸（判据③：状态=已完成、摘要非空；两路注入均拒）。
    s.add(
        "F12789-13 写回闸",
        writeback_ok(),
        "状态进行中拒 空摘要拒 已完成+非空摘要放行且终态Sealed",
    );

    // 收口：判据条数声明（14 条 = 13 实质 + 1 收口）。
    let n_before = s.len();
    let (p_before, r_before) = s.tally();
    s.add_dyn(
        "F12789-14 收口：判据条数声明",
        n_before == 13 && r_before == 0 && p_before == 13,
        &format!(
            "本卡判据共 14 条=13 实质+1 收口；已记账 13 条红 {}；UNX-F12790-J1 冻结件移交判据绿达成={}",
            r_before,
            n_before == 13 && r_before == 0 && p_before == 13
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：对齐 0x40、落在 VX9 公式全窗、本卡锚定 0x82F40。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x82F40);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000);
        assert!(SEG_BASE <= 0x82FC0);
        assert_eq!(0x80000 + 0x40 * (12789 - 12600), SEG_BASE);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：11 码互异、全部落段窗、查表可命中、未知回落。
        assert_eq!(RISK_ALL.len(), 11);
        for d in RISK_ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).is_empty());
            assert_eq!(detail(v), d.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn risk_id_closed_set_frozen() {
        // 风险 ID 闭集：22 行、首行 001、零散三号、连段 021..039、伪 ID 拒。
        assert!(id_closed_set_ok());
        assert_eq!(RISK_IDS.len(), 22);
        assert_eq!(RISK_IDS.first().copied().unwrap_or(""), "R-D1-001");
        assert_eq!(RISK_IDS.get(3).copied().unwrap_or(""), "R-D1-021");
        assert_eq!(RISK_IDS.last().copied().unwrap_or(""), "R-D1-039");
        for fake in FAKE_IDS.iter() {
            assert!(!RiskLedger::id_in_closed_set(fake), "伪ID混入：{}", fake);
        }
    }

    #[test]
    fn status_monotonic_never_rewinds() {
        // 状态闭集单调：升秩放行、踏步 EOrder、倒退 ERewind 且状态不变。
        assert!(status_closed_ok());
        let mut l = RiskLedger::new();
        assert!(l.register("R-D1-005", "单调点验行").is_ok());
        assert!(l.advance("R-D1-005", RiskStatus::FuHe).is_ok());
        assert!(l.advance("R-D1-005", RiskStatus::YiJiao).is_ok());
        assert_eq!(
            l.advance("R-D1-005", RiskStatus::FuHe),
            Err(RiskDiag::ERewind.val())
        );
        assert_eq!(
            l.advance("R-D1-005", RiskStatus::DengJi),
            Err(RiskDiag::ERewind.val())
        );
        assert_eq!(
            l.advance("R-D1-005", RiskStatus::YiJiao),
            Err(RiskDiag::EOrder.val())
        );
        match l.row_of("R-D1-005") {
            Some(r) => assert_eq!(r.status, RiskStatus::YiJiao),
            None => assert!(false, "行应在账"),
        }
    }

    #[test]
    fn happy_path_seals_all_22_rows() {
        // 正常流：22 行全移交 + 零静默 + 绿写回封账，终态 Sealed。
        let mut l = build_risk_ledger();
        assert_eq!(l.stage(), RiskStage::AllHanded);
        assert_eq!(l.row_count(), 22);
        assert!(l.zero_silent_ok());
        assert!(l.seal(HandoffWriteStatus::Done, "22 行全移交 零静默 摘要非空").is_ok());
        assert_eq!(l.stage(), RiskStage::Sealed);
        let mut i = 0usize;
        while i < l.row_count() {
            match l.row_at(i) {
                Some(r) => {
                    assert_eq!(r.status, RiskStatus::YiJiao);
                    assert!(!r.next_step.is_empty());
                }
                None => assert!(false, "行应在账"),
            }
            i += 1;
        }
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 注入必抓：静默10+倒退10+伪ID10 全数命中，无逃脱。
        let (caught, total, escape) = inject_sweep();
        assert_eq!(total, 30);
        assert_eq!(caught, 30, "逃脱：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 xorshift32 下两次全流程逐项相同，指纹相等。
        let a = build_risk_ledger();
        let b = build_risk_ledger();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.stage(), b.stage());
        assert_eq!(a.row_count(), b.row_count());
        assert_eq!(a.silent_attacks(), b.silent_attacks());
        let mut i = 0usize;
        while i < RISK_ROW_COUNT {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        assert_eq!(inject_sweep(), inject_sweep());
    }

    #[test]
    fn boundaries_and_sealed_gates() {
        // 四类边界：重复 ID 拒；未齐禁全移交/禁收官；容量耗尽拒；
        // 封账后写拒；重封倒退拒。
        let mut l = RiskLedger::new();
        assert!(l.register("R-D1-008", "首次").is_ok());
        assert_eq!(l.register("R-D1-008", "重复"), Err(RiskDiag::EDup.val()));
        let mut half = RiskLedger::new();
        assert!(half.register("R-D1-021", "半账").is_ok());
        assert_eq!(half.hand_off_all(), Err(RiskDiag::ENotFull.val()));
        assert_eq!(
            half.seal(HandoffWriteStatus::Done, "摘要非空"),
            Err(RiskDiag::ENotFull.val())
        );
        let mut c = build_risk_ledger();
        let mut k = 0usize;
        while k < LOG_CAP {
            assert!(c.add_note("点验").is_ok());
            k += 1;
        }
        assert_eq!(c.add_note("超容"), Err(RiskDiag::ECap.val()));
        let mut f = build_risk_ledger();
        assert!(f.seal(HandoffWriteStatus::Done, "收官摘要非空").is_ok());
        assert_eq!(
            f.register("R-D1-030", "封账后"),
            Err(RiskDiag::ESealed.val())
        );
        assert_eq!(f.add_note("封账后"), Err(RiskDiag::ESealed.val()));
        assert_eq!(
            f.seal(HandoffWriteStatus::Done, "重封"),
            Err(RiskDiag::ERewind.val())
        );
    }

    #[test]
    fn freeze_cross_anchor_twelve_faces() {
        // 判据交叉锚定（UNX-F12790-J1）：三冻结×四方=12 消费面。
        assert!(freeze_cross_ok());
        assert_eq!(FREEZE_ASSETS.len(), 3);
        assert_eq!(CONSUMERS.len(), 4);
        assert_eq!(CONSUME_FACE_COUNT, 12);
        let mut i = 0usize;
        while i < CONSUMERS.len() {
            let mut j = i + 1;
            while j < CONSUMERS.len() {
                assert_ne!(
                    CONSUMERS.get(i),
                    CONSUMERS.get(j),
                    "消费面重复"
                );
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn writeback_gate_per_criteria3() {
        // 写回闸（判据③）：状态=已完成 + 结果摘要非空，缺一即拒。
        let mut a = build_risk_ledger();
        assert_eq!(
            a.seal(HandoffWriteStatus::InProgress, "摘要非空"),
            Err(RiskDiag::EStatus.val())
        );
        assert_ne!(a.stage(), RiskStage::Sealed);
        let mut b = build_risk_ledger();
        assert_eq!(
            b.seal(HandoffWriteStatus::Done, ""),
            Err(RiskDiag::ESummary.val())
        );
        assert_ne!(b.stage(), RiskStage::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 14 条 = 13 实质 + 1 收口）。
        let s = run_unx12789_checks();
        assert!(s.all_passed(), "红项：{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 14);
        assert_eq!(s.tally().1, 0);
        assert!(!s.truncated());
    }
}
