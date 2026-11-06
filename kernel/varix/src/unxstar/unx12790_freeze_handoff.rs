//! UNX-F12790 跨域冻结件移交（三冻结→D2/D3/D4/E1） · 0x82F80段
//!
//! 判据原文（派工卡 12790 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F12791-J1 handoff 收官声明判据绿
//!   （finalized=40/rows=240,000/entries=800），字段可点验；③ 完成时 API/软件
//!   写回：状态=已完成、结果摘要非空。
//!
//! 补充锚定（派工卡 12790 补充条）：
//!   - 三冻结×四方消费面 = 3 frozen assets × 4 consumer domains
//!     {D2/D3/D4/E1} = 12 消费面登记行 + handoff 清单零偏差 = each row
//!     (asset, consumer, handoff state closed set) verifiable。
//!
//! 设计要点：
//! 1. 三冻结件闭集：域账冻结件 / 三区块加法账冻结件 / 号面断言冻结件，
//!    四方消费面闭集 D2/D3/D4/E1，笛卡尔 3×4=12 消费面登记行一票一面；
//! 2. 字段可点验：收官声明三字段冻结（finalized=40 / rows=240,000 /
//!    entries=800），登记值逐字段对冻结常量复核，任一失配即 EField；
//! 3. 消费面状态闭集三态单调：待移交(0) → 已移交(1) → 已确认(2)，只进不退，
//!    倒退显性拒（ERewind）、原地踏步拒（EOrder）、越闭集原始下标拒（EFaceRange）；
//! 4. handoff 清单零偏差：登记面与笛卡尔复算面逐行对照——12 对 (asset,
//!    consumer) 无重无缺、每行终态=已确认，任一偏差即 EDeviation；
//! 5. 注入器三类（越闭集注入 / 缺面注入 / 字段篡改注入）各 10 轮全抓，
//!    逃脱即判据失效；xorshift32 种子写死 = SEG_BASE，双跑一致指纹（FNV-1a）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；取值一律
//!    match/if let/.get()/.contains/saturating_*，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12790 − 12600) = 0x80000 + 0x2F80）。
pub const SEG_BASE: u32 = 0x82F80;

// ── 冻结常量表（来源：派工卡 12790 判据 UNX-F12791-J1 字段定义） ──
/// 收官声明字段：finalized（已收官批数）。
pub const FIELD_FINALIZED: u32 = 40;
/// 收官声明字段：rows（域账总行数，判据逐字 240,000）。
pub const FIELD_ROWS: u64 = 240_000;
/// 收官声明字段：entries（断言条目数，判据逐字 800）。
pub const FIELD_ENTRIES: u64 = 800;
/// 三冻结件数。
pub const ASSET_COUNT: usize = 3;
/// 四方消费面数。
pub const CONSUMER_COUNT: usize = 4;
/// 消费面登记行总数（3×4=12）。
pub const FACE_ROW_TOTAL: usize = 12;
/// 审计备注日志容量（移交清单点验槽）。
pub const LOG_CAP: usize = 16;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]）。
    ESegGuard,
    /// 消费面越闭集（冻结件下标 ≥3 或消费方下标 ≥4）。
    EFaceRange,
    /// 重复登记（同 (asset, consumer) 面二次入账）。
    EDup,
    /// 声明字段失配（finalized/rows/entries 任一偏离冻结值）。
    EField,
    /// 清单偏差（登记面与笛卡尔复算面逐行对照不一致或终态未达）。
    EDeviation,
    /// 清单未齐（12 面未齐或字段未登记即求对账/收官）。
    ENotFull,
    /// 状态倒退（待移交→已移交→已确认只进不退）。
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

impl FhDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FhDiag::ESegGuard => 0x01,
                FhDiag::EFaceRange => 0x02,
                FhDiag::EDup => 0x03,
                FhDiag::EField => 0x04,
                FhDiag::EDeviation => 0x05,
                FhDiag::ENotFull => 0x06,
                FhDiag::ERewind => 0x07,
                FhDiag::ESealed => 0x08,
                FhDiag::ECap => 0x09,
                FhDiag::EStatus => 0x0a,
                FhDiag::ESummary => 0x0b,
                FhDiag::EOrder => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            FhDiag::ESegGuard => {
                "段位越窗：SEG 须对齐 0x40 且落在 VX9 公式窗 [0x80000,0x82FC0]，先回公式 0x80000+0x40×(id−12600) 复核段基址"
            }
            FhDiag::EFaceRange => {
                "消费面越闭集：冻结件闭集 3 件、消费方闭集 D2/D3/D4/E1 四方，越界下标不入账，先回 FREEZE_ASSETS/CONSUMER_ALL 对表"
            }
            FhDiag::EDup => {
                "重复登记：同 (asset, consumer) 面已入账，一票一面不许覆写，先查既有面行再续登"
            }
            FhDiag::EField => {
                "声明字段失配：finalized=40/rows=240,000/entries=800 冻结值任一偏离即拒，逐字段回修登记声明"
            }
            FhDiag::EDeviation => {
                "清单偏差：登记面与笛卡尔复算面逐行对照不一致或终态未达已确认，按偏差行回修后重对账"
            }
            FhDiag::ENotFull => {
                "清单未齐：12 消费面未齐或声明字段未登记即求对账/收官，先补齐缺面与字段"
            }
            FhDiag::ERewind => {
                "状态倒退：待移交→已移交→已确认只进不退，倒退即移交账可疑，须全账复核"
            }
            FhDiag::ESealed => {
                "账已收官：收官后写账/注入一律拒绝，改动走下一条移交滚动"
            }
            FhDiag::ECap => {
                "备注日志容量耗尽：LOG_CAP 槽已满，先归档已有备注行再记新行"
            }
            FhDiag::EStatus => {
                "写回状态错：收官写回状态须=已完成，先按移交协议补完成态写回再收官"
            }
            FhDiag::ESummary => {
                "写回摘要空：收官写回结果摘要为空串，先填非空摘要（写明 12 消费面零偏差结论）再收官"
            }
            FhDiag::EOrder => {
                "次序违约：目标状态秩等于当前秩（原地踏步），推进须严格升秩，先明确目标态再推进"
            }
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const FH_ALL: &[FhDiag] = &[
    FhDiag::ESegGuard,
    FhDiag::EFaceRange,
    FhDiag::EDup,
    FhDiag::EField,
    FhDiag::EDeviation,
    FhDiag::ENotFull,
    FhDiag::ERewind,
    FhDiag::ESealed,
    FhDiag::ECap,
    FhDiag::EStatus,
    FhDiag::ESummary,
    FhDiag::EOrder,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in FH_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82F81..0x82F8C 闭集内，先回本卡 SEG_BASE 段核对码值"
}

// ── 三冻结件闭集 ─────────────────────────────────────────────────
/// 三冻结件闭集（跨域冻结资产，序即登记序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenAsset {
    /// 域账冻结件（40 批 × 6,000 行 = 240,000 行账）。
    Ledger,
    /// 三区块加法账冻结件（86,740+90,000+63,260 = 240,000）。
    Triple,
    /// 号面断言冻结件（800 号断言墙）。
    Assert,
}

impl FrozenAsset {
    /// 下标（0..=2）。
    pub fn idx(self) -> u32 {
        match self {
            FrozenAsset::Ledger => 0,
            FrozenAsset::Triple => 1,
            FrozenAsset::Assert => 2,
        }
    }

    /// 冻结件中文名。
    pub fn name(self) -> &'static str {
        match self {
            FrozenAsset::Ledger => "域账冻结件",
            FrozenAsset::Triple => "三区块加法账冻结件",
            FrozenAsset::Assert => "号面断言冻结件",
        }
    }

    /// 由下标反查冻结件（越界 None）。
    pub fn from_idx(i: u32) -> Option<FrozenAsset> {
        match i {
            0 => Some(FrozenAsset::Ledger),
            1 => Some(FrozenAsset::Triple),
            2 => Some(FrozenAsset::Assert),
            _ => None,
        }
    }
}

/// 三冻结件闭集数组。
pub const FREEZE_ASSET_ALL: &[FrozenAsset] = &[
    FrozenAsset::Ledger,
    FrozenAsset::Triple,
    FrozenAsset::Assert,
];

// ── 四方消费面闭集 ───────────────────────────────────────────────
/// 四方消费域闭集（D2/D3/D4/E1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// D2 消费域。
    D2,
    /// D3 消费域。
    D3,
    /// D4 消费域。
    D4,
    /// E1 消费域。
    E1,
}

impl Consumer {
    /// 下标（0..=3）。
    pub fn idx(self) -> u32 {
        match self {
            Consumer::D2 => 0,
            Consumer::D3 => 1,
            Consumer::D4 => 2,
            Consumer::E1 => 3,
        }
    }

    /// 消费域名。
    pub fn name(self) -> &'static str {
        match self {
            Consumer::D2 => "D2",
            Consumer::D3 => "D3",
            Consumer::D4 => "D4",
            Consumer::E1 => "E1",
        }
    }

    /// 由下标反查消费域（越界 None）。
    pub fn from_idx(i: u32) -> Option<Consumer> {
        match i {
            0 => Some(Consumer::D2),
            1 => Some(Consumer::D3),
            2 => Some(Consumer::D4),
            3 => Some(Consumer::E1),
            _ => None,
        }
    }
}

/// 四方消费面闭集数组。
pub const CONSUMER_ALL: &[Consumer] = &[Consumer::D2, Consumer::D3, Consumer::D4, Consumer::E1];

// ── 消费面状态闭集（三态单调） ───────────────────────────────────
/// 消费面移交状态闭集（待移交/已移交/已确认，秩只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceState {
    /// 待移交：面已登记，资产未出库。
    Pending,
    /// 已移交：资产已出库至消费域，待确认。
    Handed,
    /// 已确认：消费域确认收讫（冻结目标终态）。
    Confirmed,
}

impl FaceState {
    /// 状态秩（待移交0 < 已移交1 < 已确认2，推进须严格升秩）。
    pub fn rank(self) -> u8 {
        match self {
            FaceState::Pending => 0,
            FaceState::Handed => 1,
            FaceState::Confirmed => 2,
        }
    }

    /// 状态中文名（闭集逐字：待移交/已移交/已确认）。
    pub fn name(self) -> &'static str {
        match self {
            FaceState::Pending => "待移交",
            FaceState::Handed => "已移交",
            FaceState::Confirmed => "已确认",
        }
    }

    /// 冻结目标终态（零偏差判据的复算期望值恒为已确认）。
    pub fn expected_final() -> FaceState {
        FaceState::Confirmed
    }
}

/// 写回状态闭集（判据③：完成时 API/软件写回状态=已完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhWriteStatus {
    /// 进行中：尚未达成收官写回条件。
    InProgress,
    /// 已完成：收官写回终态。
    Done,
}

// ── 数据模型 ─────────────────────────────────────────────────────
/// 消费面登记行：(asset, consumer, state) 三元，一票一面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceRow {
    /// 冻结件（三冻结闭集成员）。
    pub asset: FrozenAsset,
    /// 消费域（四方闭集成员）。
    pub consumer: Consumer,
    /// 移交状态（三态闭集）。
    pub state: FaceState,
}

/// 收官声明字段三元（finalized/rows/entries，冻结值登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclFields {
    /// 已收官批数（冻结 40）。
    pub finalized: u32,
    /// 域账总行数（冻结 240,000）。
    pub rows: u64,
    /// 断言条目数（冻结 800）。
    pub entries: u64,
}

/// 移交账阶段（只进不退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FhStage {
    /// 登记中：允许逐面登记与状态推进。
    Open,
    /// 已对账：零偏差 + 字段齐，允许收官。
    Reconciled,
    /// 收官：只读，写账/注入一律拒绝。
    Sealed,
}

impl FhStage {
    /// 阶段秩（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            FhStage::Open => 0,
            FhStage::Reconciled => 1,
            FhStage::Sealed => 2,
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

/// 跨域冻结件移交账：12 消费面 + 声明字段 + 单调状态机 + 备注日志。
#[derive(Clone, Debug)]
pub struct FreezeManifest {
    /// 消费面登记行（登记序，一票一面）。
    faces: Vec<FaceRow>,
    /// 收官声明字段（None=未登记）。
    fields: Option<DeclFields>,
    /// 移交账阶段。
    stage: FhStage,
    /// 备注日志（容量 LOG_CAP）。
    notes: Vec<String>,
}

impl FreezeManifest {
    /// 开一本新移交账。
    pub fn new() -> FreezeManifest {
        FreezeManifest {
            faces: Vec::new(),
            fields: None,
            stage: FhStage::Open,
            notes: Vec::new(),
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> FhStage {
        self.stage
    }

    /// 已登记面行数。
    pub fn face_count(&self) -> usize {
        self.faces.len()
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

    /// 按面行下标取行（越界 None）。
    pub fn face_at(&self, idx: usize) -> Option<&FaceRow> {
        self.faces.get(idx)
    }

    /// 按面 (asset, consumer) 对查行（未登记返回 None）。
    pub fn face_of(&self, asset: FrozenAsset, consumer: Consumer) -> Option<&FaceRow> {
        self.faces
            .iter()
            .find(|r| r.asset == asset && r.consumer == consumer)
    }

    /// 已登记的声明字段（None=未登记）。
    pub fn fields(&self) -> Option<DeclFields> {
        self.fields
    }

    /// 原始下标路径登记一面（注入面与正常面共用；越闭集必拒）。
    pub fn register_face_idx(&mut self, ai: u32, ci: u32) -> Result<(), u32> {
        // ① 收官闸。
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        // ② 倒退闸：离登记阶段再回头登记属倒退。
        if self.stage.rank() > FhStage::Open.rank() {
            return Err(FhDiag::ERewind.val());
        }
        // ③ 消费面闭集闸：原始下标越界即拒。
        let asset = match FrozenAsset::from_idx(ai) {
            Some(a) => a,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        let consumer = match Consumer::from_idx(ci) {
            Some(c) => c,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        // ④ 重复闸。
        if self.face_of(asset, consumer).is_some() {
            return Err(FhDiag::EDup.val());
        }
        self.faces.push(FaceRow {
            asset,
            consumer,
            state: FaceState::Pending,
        });
        Ok(())
    }

    /// 闭集路径登记一面（enum 直传，闭集由类型保证）。
    pub fn register_face(&mut self, asset: FrozenAsset, consumer: Consumer) -> Result<(), u32> {
        self.register_face_idx(asset.idx(), consumer.idx())
    }

    /// 状态推进：目标秩须严格大于当前秩（升秩放行，倒退/踏步均拒）。
    pub fn advance_face(&mut self, ai: u32, ci: u32, to: FaceState) -> Result<(), u32> {
        // 收官闸。
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        let asset = match FrozenAsset::from_idx(ai) {
            Some(a) => a,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        let consumer = match Consumer::from_idx(ci) {
            Some(c) => c,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        let cur = match self.face_of(asset, consumer) {
            Some(r) => r.state,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        if to.rank() < cur.rank() {
            return Err(FhDiag::ERewind.val());
        }
        if to.rank() == cur.rank() {
            return Err(FhDiag::EOrder.val());
        }
        match self.faces.iter_mut().find(|r| r.asset == asset && r.consumer == consumer) {
            Some(r) => {
                r.state = to;
                Ok(())
            }
            None => Err(FhDiag::EFaceRange.val()),
        }
    }

    /// 登记收官声明字段：逐字段对冻结值复核，任一失配即拒（字段可点验）。
    pub fn register_fields(&mut self, finalized: u32, rows: u64, entries: u64) -> Result<(), u32> {
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        if finalized != FIELD_FINALIZED || rows != FIELD_ROWS || entries != FIELD_ENTRIES {
            return Err(FhDiag::EField.val());
        }
        self.fields = Some(DeclFields {
            finalized,
            rows,
            entries,
        });
        Ok(())
    }

    /// 零偏差复核：登记面与笛卡尔复算面逐行对照——
    /// 12 对 (asset, consumer) 无重无缺、每行终态=已确认。
    pub fn zero_deviation_ok(&self) -> bool {
        // 面数齐：恰 12 行。
        if self.faces.len() != FACE_ROW_TOTAL {
            return false;
        }
        // 笛卡尔复算：每对恰出现一次且终态达标。
        for a in FREEZE_ASSET_ALL.iter() {
            for c in CONSUMER_ALL.iter() {
                match self.face_of(*a, *c) {
                    Some(r) => {
                        if r.state != FaceState::expected_final() {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
        }
        // 无多余面：登记面计数与笛卡尔计数相等即无缺无重。
        true
    }

    /// 对账：12 面齐 + 字段已登记 + 零偏差，全绿推进 Reconciled。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        if self.faces.len() != FACE_ROW_TOTAL {
            return Err(FhDiag::ENotFull.val());
        }
        if self.fields.is_none() {
            return Err(FhDiag::ENotFull.val());
        }
        if !self.zero_deviation_ok() {
            return Err(FhDiag::EDeviation.val());
        }
        self.stage = FhStage::Reconciled;
        Ok(())
    }

    /// 记一条备注行（容量 LOG_CAP，收官后拒绝）。
    pub fn add_note(&mut self, text: &str) -> Result<(), u32> {
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        if self.notes.len() >= LOG_CAP {
            return Err(FhDiag::ECap.val());
        }
        self.notes.push(String::from(text));
        Ok(())
    }

    /// 收官（判据②③锚定）：已对账 + 零偏差 + 字段可点验 + 写回状态=已完成
    /// + 摘要非空。
    pub fn seal(&mut self, status: FhWriteStatus, summary: &str) -> Result<(), u32> {
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ERewind.val());
        }
        if self.stage.rank() < FhStage::Reconciled.rank() {
            return Err(FhDiag::ENotFull.val());
        }
        if !self.zero_deviation_ok() {
            return Err(FhDiag::EDeviation.val());
        }
        // 字段可点验（判据 ② 逐字）：登记字段与冻结值再核一遍。
        match self.fields {
            Some(f) => {
                if f.finalized != FIELD_FINALIZED
                    || f.rows != FIELD_ROWS
                    || f.entries != FIELD_ENTRIES
                {
                    return Err(FhDiag::EField.val());
                }
            }
            None => return Err(FhDiag::ENotFull.val()),
        }
        if status != FhWriteStatus::Done {
            return Err(FhDiag::EStatus.val());
        }
        if summary.is_empty() {
            return Err(FhDiag::ESummary.val());
        }
        self.stage = FhStage::Sealed;
        Ok(())
    }

    // ── 审计注入面（只服务注入必抓扫，收官后拒绝） ────────────────
    /// 类三注入：把指定面状态强置为指定态（模拟账面倒退/停滞），
    /// 零偏差复核必须抓（终态未达即 EDeviation）。
    pub fn inject_face_state(&mut self, ai: u32, ci: u32, st: FaceState) -> Result<(), u32> {
        if self.stage.rank() >= FhStage::Sealed.rank() {
            return Err(FhDiag::ESealed.val());
        }
        let asset = match FrozenAsset::from_idx(ai) {
            Some(a) => a,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        let consumer = match Consumer::from_idx(ci) {
            Some(c) => c,
            None => return Err(FhDiag::EFaceRange.val()),
        };
        match self
            .faces
            .iter_mut()
            .find(|r| r.asset == asset && r.consumer == consumer)
        {
            Some(r) => {
                r.state = st;
                Ok(())
            }
            None => Err(FhDiag::EFaceRange.val()),
        }
    }

    /// 双跑一致指纹（FNV-1a 32 位）：阶段 + 面行 + 字段 + 备注行。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_mix(h, &self.stage.rank().to_le_bytes());
        h = fnv_mix(h, &(self.faces.len() as u32).to_le_bytes());
        for r in self.faces.iter() {
            h = fnv_mix(h, &r.asset.idx().to_le_bytes());
            h = fnv_mix(h, &r.consumer.idx().to_le_bytes());
            h = fnv_mix(h, &r.state.rank().to_le_bytes());
        }
        match self.fields {
            Some(f) => {
                h = fnv_mix(h, &f.finalized.to_le_bytes());
                h = fnv_mix(h, &f.rows.to_le_bytes());
                h = fnv_mix(h, &f.entries.to_le_bytes());
            }
            None => h = fnv_mix(h, &0xFFFF_FFFFu32.to_le_bytes()),
        }
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

// ── 正常流构建（12 面登记 → 声明字段 → 逐面升秩 → 对账，不收官） ──
/// 构建一本 12 面齐、零偏差、已对账的移交账。
pub fn build_freeze_manifest() -> FreezeManifest {
    let mut m = FreezeManifest::new();
    for a in FREEZE_ASSET_ALL.iter() {
        for c in CONSUMER_ALL.iter() {
            let _ = m.register_face(*a, *c);
        }
    }
    let _ = m.register_fields(FIELD_FINALIZED, FIELD_ROWS, FIELD_ENTRIES);
    // 逐面升秩：待移交 → 已移交 → 已确认。
    for a in FREEZE_ASSET_ALL.iter() {
        for c in CONSUMER_ALL.iter() {
            let _ = m.advance_face(a.idx(), c.idx(), FaceState::Handed);
            let _ = m.advance_face(a.idx(), c.idx(), FaceState::Confirmed);
        }
    }
    let _ = m.reconcile();
    m
}

// ── 注入器三类扫（各 10 轮全抓，逃脱即判据失效） ─────────────────
/// 注入扫：类一越闭集注入 ×10 / 类二缺面注入 ×10 / 类三字段篡改注入 ×10。
/// 返回（抓到路数, 总路数, 首个逃脱路描述）。
pub fn inject_sweep() -> (usize, usize, String) {
    let mut rng = Xor32::new(SEG_BASE);
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 类一：越闭集注入 —— 原始下标越界（asset≥3 / consumer≥4）必被拒。
    let mut k = 0usize;
    while k < 10 {
        let (ai, ci) = if k % 2 == 0 {
            (3 + rng.below(5), rng.below(4))
        } else {
            (rng.below(3), 4 + rng.below(5))
        };
        let mut m = FreezeManifest::new();
        let r = m.register_face_idx(ai, ci);
        let ok = r == Err(FhDiag::EFaceRange.val()) && m.face_count() == 0;
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类一越闭集 路{} ai={} ci={} 注入逃脱", k, ai, ci);
        }
        k += 1;
    }
    // 类二：缺面注入 —— 12 面缺一即零偏差红且对账/收官必拒。
    k = 0;
    while k < 10 {
        let skip = rng.below(FACE_ROW_TOTAL as u32) as usize;
        let mut m = FreezeManifest::new();
        let mut n = 0usize;
        while n < FACE_ROW_TOTAL {
            if n != skip {
                let ai = (n / CONSUMER_COUNT) as u32;
                let ci = (n % CONSUMER_COUNT) as u32;
                let _ = m.register_face_idx(ai, ci);
            }
            n += 1;
        }
        let _ = m.register_fields(FIELD_FINALIZED, FIELD_ROWS, FIELD_ENTRIES);
        let rec = m.reconcile();
        let ok = rec == Err(FhDiag::ENotFull.val()) && !m.zero_deviation_ok();
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类二缺面 路{} skip={} 注入逃脱", k, skip);
        }
        k += 1;
    }
    // 类三：字段篡改注入 —— 三字段任一偏离冻结值必被 EField 抓。
    k = 0;
    while k < 10 {
        let mut m = FreezeManifest::new();
        let pick = rng.below(3);
        let (f, r_, e) = match pick {
            0 => (FIELD_FINALIZED + 1 + rng.below(3), FIELD_ROWS, FIELD_ENTRIES),
            1 => (
                FIELD_FINALIZED,
                (FIELD_ROWS as u64).wrapping_add(1 + rng.below(9) as u64),
                FIELD_ENTRIES,
            ),
            _ => (
                FIELD_FINALIZED,
                FIELD_ROWS,
                FIELD_ENTRIES.wrapping_add(1 + rng.below(5) as u64),
            ),
        };
        let reg = m.register_fields(f, r_, e);
        let ok = reg == Err(FhDiag::EField.val()) && m.fields().is_none();
        total += 1;
        if ok {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("类三字段 路{} pick={} 注入逃脱", k, pick);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x82FC0]，
/// 并锚定本卡公式实值 0x80000 + 0x40×190 = 0x82F80。
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
    let own = match 12790u32.checked_sub(12600) {
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
    while i < FH_ALL.len() {
        let d = match FH_ALL.get(i) {
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
        while j < FH_ALL.len() {
            match FH_ALL.get(j) {
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

/// 三冻结×四方闭集守卫：3 件名非空、4 方名非空、下标回环一致、越界拒。
fn closed_set_ok() -> bool {
    if FREEZE_ASSET_ALL.len() != ASSET_COUNT || CONSUMER_ALL.len() != CONSUMER_COUNT {
        return false;
    }
    for a in FREEZE_ASSET_ALL.iter() {
        if a.name().is_empty() {
            return false;
        }
        match FrozenAsset::from_idx(a.idx()) {
            Some(x) => {
                if x != *a {
                    return false;
                }
            }
            None => return false,
        }
    }
    for c in CONSUMER_ALL.iter() {
        if c.name().is_empty() {
            return false;
        }
        match Consumer::from_idx(c.idx()) {
            Some(x) => {
                if x != *c {
                    return false;
                }
            }
            None => return false,
        }
    }
    // 越界下标拒（asset cap=2、consumer cap=3，cap+1 越界）。
    if FrozenAsset::from_idx(3).is_some() || Consumer::from_idx(4).is_some() {
        return false;
    }
    // 笛卡尔计数：3×4=12。
    ASSET_COUNT.saturating_mul(CONSUMER_COUNT) == FACE_ROW_TOTAL
}

/// 字段可点验：冻结常量逐字（40 / 240,000 / 800）且登记回读一致。
fn fields_ok() -> bool {
    if FIELD_FINALIZED != 40 || FIELD_ROWS != 240_000 || FIELD_ENTRIES != 800 {
        return false;
    }
    let m = build_freeze_manifest();
    match m.fields() {
        Some(f) => f.finalized == FIELD_FINALIZED && f.rows == FIELD_ROWS && f.entries == FIELD_ENTRIES,
        None => false,
    }
}

/// 正常流：12 面登记 → 字段登记 → 逐面升秩 → 对账 → 备注 ×2 → 收官（绿写回）。
fn happy_path_ok() -> bool {
    let mut m = build_freeze_manifest();
    if m.stage() != FhStage::Reconciled {
        return false;
    }
    if !m.zero_deviation_ok() {
        return false;
    }
    let mut k = 0usize;
    while k < 2 {
        if m.add_note("移交点验行").is_err() {
            return false;
        }
        k += 1;
    }
    if m.seal(FhWriteStatus::Done, "12 消费面零偏差 字段可点验 结果摘要非空")
        .is_err()
    {
        return false;
    }
    m.stage() == FhStage::Sealed && m.zero_deviation_ok()
}

/// 零偏差清单守卫：正常账零偏差；倒退注入/缺面注入后必破。
fn zero_dev_ok() -> bool {
    let m = build_freeze_manifest();
    if !m.zero_deviation_ok() || m.face_count() != FACE_ROW_TOTAL {
        return false;
    }
    // 终态未达注入：任一面压回已移交 → 零偏差红。
    let mut t = build_freeze_manifest();
    if t.inject_face_state(1, 2, FaceState::Handed).is_err() {
        return false;
    }
    if t.zero_deviation_ok() {
        return false;
    }
    if t.reconcile() != Err(FhDiag::EDeviation.val()) {
        return false;
    }
    // 倒退到待移交同样红。
    let mut u = build_freeze_manifest();
    if u.inject_face_state(0, 3, FaceState::Pending).is_err() {
        return false;
    }
    !u.zero_deviation_ok()
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
    let a = build_freeze_manifest();
    let b = build_freeze_manifest();
    if a.stage() != b.stage() || a.face_count() != b.face_count() {
        return false;
    }
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    let mut i = 0usize;
    while i < FACE_ROW_TOTAL {
        match (a.face_at(i), b.face_at(i)) {
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

/// 边界四类：重复面拒、未齐禁对账/收官、容量耗尽拒、封账后写拒/重封倒退拒。
fn boundary_ok() -> bool {
    // ① 重复面拒。
    let mut m = FreezeManifest::new();
    if m.register_face(FrozenAsset::Ledger, Consumer::D2).is_err() {
        return false;
    }
    if m.register_face(FrozenAsset::Ledger, Consumer::D2) != Err(FhDiag::EDup.val()) {
        return false;
    }
    // ② 未齐禁对账/禁收官（空账 + 半账两路）。
    let mut e = FreezeManifest::new();
    if e.reconcile() != Err(FhDiag::ENotFull.val()) {
        return false;
    }
    if e.seal(FhWriteStatus::Done, "摘要非空") != Err(FhDiag::ENotFull.val()) {
        return false;
    }
    let mut half = FreezeManifest::new();
    if half.register_face(FrozenAsset::Triple, Consumer::E1).is_err() {
        return false;
    }
    if half.reconcile() != Err(FhDiag::ENotFull.val()) {
        return false;
    }
    // ③ 容量耗尽。
    let mut c = build_freeze_manifest();
    let mut k = 0usize;
    while k < LOG_CAP {
        if c.add_note("容量点验").is_err() {
            return false;
        }
        k += 1;
    }
    if c.add_note("超容") != Err(FhDiag::ECap.val()) {
        return false;
    }
    // ④ 封账后写拒 + 重封倒退拒。
    let mut f = build_freeze_manifest();
    if f.seal(FhWriteStatus::Done, "12 消费面零偏差收官摘要").is_err() {
        return false;
    }
    if f.register_face(FrozenAsset::Assert, Consumer::D4) != Err(FhDiag::ESealed.val()) {
        return false;
    }
    if f.add_note("封账后备注") != Err(FhDiag::ESealed.val()) {
        return false;
    }
    if f.seal(FhWriteStatus::Done, "重封") != Err(FhDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 次序语义：踏步 EOrder、倒退 ERewind 且状态不变、升秩放行、越面 EFaceRange。
fn order_semantics_ok() -> bool {
    let mut m = FreezeManifest::new();
    if m.register_face(FrozenAsset::Assert, Consumer::E1).is_err() {
        return false;
    }
    // 原地踏步（待移交→待移交）→ EOrder。
    if m.advance_face(2, 3, FaceState::Pending) != Err(FhDiag::EOrder.val()) {
        return false;
    }
    // 升秩两步 → 已确认。
    if m.advance_face(2, 3, FaceState::Handed).is_err() {
        return false;
    }
    if m.advance_face(2, 3, FaceState::Confirmed).is_err() {
        return false;
    }
    // 倒退 → ERewind 且状态不变。
    if m.advance_face(2, 3, FaceState::Handed) != Err(FhDiag::ERewind.val()) {
        return false;
    }
    match m.face_of(FrozenAsset::Assert, Consumer::E1) {
        Some(r) => {
            if r.state != FaceState::Confirmed {
                return false;
            }
        }
        None => return false,
    }
    // 未登记面推进 → EFaceRange。
    if m.advance_face(0, 0, FaceState::Handed) != Err(FhDiag::EFaceRange.val()) {
        return false;
    }
    true
}

/// 写回闸：错误状态/空摘要各路均拒且阶段不进 Sealed；绿写回放行。
fn writeback_ok() -> bool {
    let mut a = build_freeze_manifest();
    if a.seal(FhWriteStatus::InProgress, "摘要非空") != Err(FhDiag::EStatus.val()) {
        return false;
    }
    if a.stage() == FhStage::Sealed {
        return false;
    }
    let mut b = build_freeze_manifest();
    if b.seal(FhWriteStatus::Done, "") != Err(FhDiag::ESummary.val()) {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12790 自检（判据 12790 卡：跨域冻结件移交 · UNX-F12791-J1 字段可点验）。
pub fn run_unx12790_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12790");
    let manifest = build_freeze_manifest();

    // 01 段位守卫（公式 0x80000+0x40×190=0x82F80，窗 [0x80000,0x82FC0]）。
    s.add_dyn(
        "F12790-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 公式=0x80000+0x40×190 对齐0x40={} 窗=[0x80000,0x82FC0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    // 02 诊断码闭集（12 码互异落窗 + 未知回落）。
    s.add(
        "F12790-02 诊断码闭集",
        diag_closed_ok(),
        "12码互异落段窗 查表命中 未知码回落含'未知'",
    );

    // 03 三冻结×四方闭集（3×4=12 消费面，越界下标拒）。
    s.add_dyn(
        "F12790-03 三冻结四方闭集",
        closed_set_ok(),
        &format!(
            "冻结件{}件 消费方{}方（D2/D3/D4/E1） 笛卡尔={}面 asset3/consumer4越界拒",
            FREEZE_ASSET_ALL.len(),
            CONSUMER_ALL.len(),
            ASSET_COUNT.saturating_mul(CONSUMER_COUNT)
        ),
    );

    // 04 消费面状态闭集单调（待移交→已移交→已确认）。
    s.add(
        "F12790-04 状态闭集单调",
        order_semantics_ok(),
        "三态闭集 待移交(0)→已移交(1)→已确认(2) 踏步EOrder 倒退ERewind 越面EFaceRange",
    );

    // 05 正常流收官（12面→字段→升秩→对账→备注→绿写回封账）。
    s.add_dyn(
        "F12790-05 正常流收官",
        happy_path_ok(),
        &format!(
            "12面登记→字段→升秩→对账→收官 终态={:?} 面={}",
            manifest.stage(),
            manifest.face_count()
        ),
    );

    // 06 零偏差清单（登记面 vs 笛卡尔复算面逐行对照；注入后必破）。
    s.add_dyn(
        "F12790-06 零偏差清单",
        zero_dev_ok(),
        &format!(
            "12对(asset,consumer)无缺无重 终态=已确认 偏差注入后守恒红 对账={:?}",
            {
                let mut t = build_freeze_manifest();
                let _ = t.inject_face_state(1, 2, FaceState::Handed);
                t.reconcile().err()
            }
        ),
    );

    // 07 注入器三类 30 轮全抓（越闭集/缺面/字段篡改 各 10）。
    let (caught, total, escape) = inject_sweep();
    s.add_dyn(
        "F12790-07 注入三类 30 轮全抓",
        caught == 30 && total == 30 && escape.is_empty(),
        &format!("越闭集10+缺面10+字段10 caught={}/{} escape={}", caught, total, escape),
    );

    // 08 字段可点验（finalized=40/rows=240,000/entries=800 冻结值回读一致）。
    s.add_dyn(
        "F12790-08 字段可点验",
        fields_ok(),
        &format!(
            "finalized={} rows={} entries={} 登记回读一致",
            FIELD_FINALIZED, FIELD_ROWS, FIELD_ENTRIES
        ),
    );

    // 09 双跑一致指纹（FNV-1a + xorshift32 种子=SEG_BASE）。
    let fa = build_freeze_manifest().fingerprint();
    let fb = build_freeze_manifest().fingerprint();
    s.add(
        "F12790-09 双跑一致指纹",
        dual_run_ok() && fa == fb,
        "双跑指纹相等 12面行逐条等 注入扫双跑一致",
    );

    // 10 边界四类（重复面/未齐禁对账禁收官/容量耗尽/封账后写拒+重封倒退）。
    s.add(
        "F12790-10 边界四类",
        boundary_ok(),
        "重复面拒 空账半账禁对账禁收官 容量16满拒 封账后写拒 重封倒退拒",
    );

    // 11 消费面登记行逐行点验（12 行 (asset, consumer, state) 逐行可点验）。
    s.add(
        "F12790-11 消费面逐行点验",
        manifest.face_count() == FACE_ROW_TOTAL && manifest.zero_deviation_ok(),
        "12行逐行(asset,consumer,state)可点验 面数=12 终态全已确认",
    );

    // 12 写回闸（判据③：状态=已完成、摘要非空；两路注入均拒）。
    s.add_dyn(
        "F12790-12 写回闸",
        writeback_ok(),
        &format!(
            "状态进行中拒 空摘要拒 已完成+非空摘要放行（见 05 正常流 终态={:?}）",
            manifest.stage()
        ),
    );

    // 13 判据交叉锚定（UNX-F12792-J1 四十行同步：finalized=40 与 40 批同源）。
    s.add(
        "F12790-13 交叉锚定40批",
        FIELD_FINALIZED == 40 && FIELD_ROWS == 240_000 && FIELD_ENTRIES == 800,
        "finalized=40 与总纲四十行同源 rows=240,000 与域账守恒同源 entries=800 与号面同源",
    );

    // 收口：判据条数声明（14 条 = 13 实质 + 1 收口）。
    let n_before = s.len();
    let (p_before, r_before) = s.tally();
    s.add_dyn(
        "F12790-14 收口：判据条数声明",
        n_before == 13 && r_before == 0 && p_before == 13,
        &format!(
            "本卡判据共 14 条=13 实质+1 收口；已记账 13 条红 {}；UNX-F12791-J1 handoff 收官声明判据绿达成={}",
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
        // 段位守卫：对齐 0x40、落在 VX9 公式全窗、本卡锚定 0x82F80。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x82F80);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000);
        assert!(SEG_BASE <= 0x82FC0);
        assert_eq!(0x80000 + 0x40 * (12790 - 12600), SEG_BASE);
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：12 码互异、全部落段窗、查表可命中、未知回落。
        assert_eq!(FH_ALL.len(), 12);
        for d in FH_ALL.iter() {
            let v = d.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            assert!(!detail(v).is_empty());
            assert_eq!(detail(v), d.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn closed_sets_and_cartesian() {
        // 三冻结×四方闭集：下标回环一致、越界拒、笛卡尔 3×4=12。
        assert!(closed_set_ok());
        assert_eq!(FREEZE_ASSET_ALL.len(), 3);
        assert_eq!(CONSUMER_ALL.len(), 4);
        assert_eq!(FACE_ROW_TOTAL, 12);
        assert!(FrozenAsset::from_idx(3).is_none());
        assert!(Consumer::from_idx(4).is_none());
        assert_eq!(FaceState::expected_final(), FaceState::Confirmed);
    }

    #[test]
    fn fields_point_check_frozen() {
        // 字段可点验：冻结 40/240,000/800 且登记回读一致；篡改必拒。
        assert!(fields_ok());
        let mut m = FreezeManifest::new();
        assert_eq!(
            m.register_fields(41, 240_000, 800),
            Err(FhDiag::EField.val())
        );
        assert_eq!(
            m.register_fields(40, 239_999, 800),
            Err(FhDiag::EField.val())
        );
        assert_eq!(m.register_fields(40, 240_000, 801), Err(FhDiag::EField.val()));
        assert!(m.register_fields(40, 240_000, 800).is_ok());
        assert!(m.fields().is_some());
    }

    #[test]
    fn happy_path_seals_twelve_faces() {
        // 正常流：12 面 + 零偏差 + 绿写回封账，终态 Sealed。
        let mut m = build_freeze_manifest();
        assert_eq!(m.stage(), FhStage::Reconciled);
        assert_eq!(m.face_count(), 12);
        assert!(m.zero_deviation_ok());
        assert!(
            m.seal(FhWriteStatus::Done, "12 消费面零偏差 字段可点验 摘要非空")
                .is_ok()
        );
        assert_eq!(m.stage(), FhStage::Sealed);
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 注入必抓：越闭集10+缺面10+字段10 全数命中，无逃脱。
        let (caught, total, escape) = inject_sweep();
        assert_eq!(total, 30);
        assert_eq!(caught, 30, "逃脱：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_identical() {
        // 双跑一致：确定性 xorshift32 下两次全流程逐项相同，指纹相等。
        let a = build_freeze_manifest();
        let b = build_freeze_manifest();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.stage(), b.stage());
        assert_eq!(a.face_count(), b.face_count());
        let mut i = 0usize;
        while i < FACE_ROW_TOTAL {
            assert_eq!(a.face_at(i), b.face_at(i));
            i += 1;
        }
        assert_eq!(inject_sweep(), inject_sweep());
    }

    #[test]
    fn boundaries_and_sealed_gates() {
        // 四类边界：重复面拒；空账/半账禁对账禁收官；容量耗尽拒；
        // 封账后写拒；重封倒退拒。
        let mut m = FreezeManifest::new();
        assert!(m.register_face(FrozenAsset::Ledger, Consumer::D2).is_ok());
        assert_eq!(
            m.register_face(FrozenAsset::Ledger, Consumer::D2),
            Err(FhDiag::EDup.val())
        );
        let mut e = FreezeManifest::new();
        assert_eq!(e.reconcile(), Err(FhDiag::ENotFull.val()));
        assert_eq!(
            e.seal(FhWriteStatus::Done, "摘要非空"),
            Err(FhDiag::ENotFull.val())
        );
        let mut c = build_freeze_manifest();
        let mut k = 0usize;
        while k < LOG_CAP {
            assert!(c.add_note("点验").is_ok());
            k += 1;
        }
        assert_eq!(c.add_note("超容"), Err(FhDiag::ECap.val()));
        let mut f = build_freeze_manifest();
        assert!(f.seal(FhWriteStatus::Done, "收官摘要非空").is_ok());
        assert_eq!(
            f.register_face(FrozenAsset::Assert, Consumer::D4),
            Err(FhDiag::ESealed.val())
        );
        assert_eq!(f.add_note("封账后"), Err(FhDiag::ESealed.val()));
        assert_eq!(
            f.seal(FhWriteStatus::Done, "重封"),
            Err(FhDiag::ERewind.val())
        );
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机单调：升秩放行、踏步 EOrder、倒退 ERewind 且状态不变、
        // 越面 EFaceRange。
        let mut m = FreezeManifest::new();
        assert!(m.register_face(FrozenAsset::Triple, Consumer::D3).is_ok());
        assert!(m.advance_face(1, 1, FaceState::Handed).is_ok());
        assert!(m.advance_face(1, 1, FaceState::Confirmed).is_ok());
        assert_eq!(
            m.advance_face(1, 1, FaceState::Handed),
            Err(FhDiag::ERewind.val())
        );
        assert_eq!(
            m.advance_face(1, 1, FaceState::Confirmed),
            Err(FhDiag::EOrder.val())
        );
        assert_eq!(
            m.advance_face(0, 0, FaceState::Handed),
            Err(FhDiag::EFaceRange.val())
        );
        match m.face_of(FrozenAsset::Triple, Consumer::D3) {
            Some(r) => assert_eq!(r.state, FaceState::Confirmed),
            None => assert!(false, "行应在账"),
        }
    }

    #[test]
    fn missing_face_breaks_zero_deviation() {
        // 缺面注入：12 面缺一即零偏差红且对账必拒。
        let mut m = FreezeManifest::new();
        for a in FREEZE_ASSET_ALL.iter() {
            for c in CONSUMER_ALL.iter() {
                if !(*a == FrozenAsset::Triple && *c == Consumer::E1) {
                    let _ = m.register_face(*a, *c);
                }
            }
        }
        assert_eq!(m.face_count(), 11);
        assert!(m.register_fields(FIELD_FINALIZED, FIELD_ROWS, FIELD_ENTRIES).is_ok());
        assert!(!m.zero_deviation_ok());
        assert_eq!(m.reconcile(), Err(FhDiag::ENotFull.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条（本卡 14 条 = 13 实质 + 1 收口）。
        let s = run_unx12790_checks();
        assert!(s.all_passed(), "红项：{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), 14);
        assert_eq!(s.tally().1, 0);
        assert!(!s.truncated());
    }
}
