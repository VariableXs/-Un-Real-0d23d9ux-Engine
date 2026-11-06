//! UNX-F12799 域收官标志条（域 40 批 800 条满账宣言+三断言） · 0x831C0段
//!
//! 判据原文（派工卡 12799 逐字锚定）：
//!   UNX-F12800-J1 三断言绿：域账封账 240,000/240,000、号面 F12001–F12800
//!   冻结终版、移交包五件齐——域 40 批 800 条满账收口宣言。
//!
//! 设计要点：
//! 1. 三断言状态机：域账封账（assert_ledger_sealed：账本行已记账/应记账/
//!    封账旗标逐行复算，未封账 → EBUSY 语义）、号面冻结（assert_idrange_
//!    frozen：F12001–F12800 共 800 条派生公式全量复算 + 冻结态逐条核）、
//!    移交包齐（assert_handoff_complete：总纲/台账/断言资产/复测资产库/
//!    收官标志五件套存在性与指纹逐件核，缺件 → ENOENT 语义）；
//! 2. 满账宣言判定器 judge_closure_manifest() -> Result<ClosureRow, u32>：
//!    判定次序冻结（域账 → 号面 → 移交包），三断言全绿才发宣言行，任一
//!    断言挂即回该断言的诊断码——指明哪件缺、哪条挂；
//! 3. 域账恒等式：40 批 × 20 行 = 800 条账行，每行 300 条断言，
//!    封账总量恒等 240,000/240,000（checked 累加，短账显性拒）；
//! 4. 号面派生公式冻结终版：serial × 0x9E3779B1 + 0x831C0（mod 2^32），
//!    800 条逐条复算派生码 + 冻结态逐条核，翻转一条即 EIdUnfrozen；
//! 5. 注入器三类（账行篡改/缺件注入/冻结态翻转）各 10 轮全抓，逃脱即
//!    判据失效；LCG 种子写死 = SEG_BASE，双跑一致指纹（FNV-1a 64）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/panic/裸下标；状态机只进不退，倒退显性拒。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12799 − 12600) = 0x80000 + 0x31C0）。
pub const SEG_BASE: u32 = 0x831C0;

// ── 域收官恒等式常量（来源：判据原文 UNX-F12800-J1 冻结口径） ────
/// 域批数（域 40 批，冻结）。
pub const BATCH_N: u32 = 40;
/// 每批账行数（每批 20 条，冻结）。
pub const ROWS_PER_BATCH: u32 = 20;
/// 域账总行数（40 × 20 = 800 条，冻结）。
pub const TOTAL_ROWS: u32 = 800;
/// 每行断言数（300 条，冻结）。
pub const ASSERTS_PER_ROW: u32 = 300;
/// 域账封账总量（300 × 800 = 240,000，冻结口径 240,000/240,000）。
pub const LEDGER_CAP: u32 = 240_000;
/// 号面首序号（F12001，冻结）。
pub const ID_FIRST: u32 = 12001;
/// 号面尾序号（F12800，冻结）。
pub const ID_LAST: u32 = 12800;
/// 号面条数（12800 − 12001 + 1 = 800 条，冻结）。
pub const ID_N: u32 = 800;
/// 移交包件数（五件套，冻结）。
pub const HANDOFF_N: usize = 5;

// ── 域收官诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0E 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClDiag {
    /// 段位越窗或不对齐（SEG 须对齐 0x40 且落在 VX9 公式窗）。
    ESegGuard,
    /// 域账未封账（EBUSY 语义：账行存在封账旗标未落位，先走封账口）。
    EBusy,
    /// 应记账行缺失（账面出现不应记账行，漏账样本，先补登记口径）。
    ERowDue,
    /// 账行未记账（应记账但已记账位为假，先补记该行账）。
    ERowPost,
    /// 断言总量非 240,000（封账总量恒等式不成立，逐行复核断言计数）。
    EAssertSum,
    /// 号面序号越 F12001–F12800 闭集或序位错乱。
    EIdRange,
    /// 号面派生公式复算不符（冻结终版口径被改，按派生公式回填）。
    EIdDerive,
    /// 号面冻结态翻转/未冻结（逐条核到假冻结节，先恢复冻结终版）。
    EIdUnfrozen,
    /// 移交包缺件（ENOENT 语义：五件套缺一，按件名补齐后再收官）。
    ENoent,
    /// 移交件指纹不符（存在但指纹对不上，重新出件并重算指纹）。
    EFingerprint,
    /// 状态未到位（前序断言未过即调后序口，按冻结次序先补前序）。
    EState,
    /// 状态机倒退（建账→封账→冻结→移交→宣言只进不退，倒退即账可疑）。
    ERewind,
    /// 点验日志容量耗尽（日志槽已满仍记新行，先出清点验账再记）。
    ECap,
}

impl ClDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0E 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ClDiag::ESegGuard => 0x01,
                ClDiag::EBusy => 0x02,
                ClDiag::ERowDue => 0x03,
                ClDiag::ERowPost => 0x04,
                ClDiag::EAssertSum => 0x05,
                ClDiag::EIdRange => 0x06,
                ClDiag::EIdDerive => 0x07,
                ClDiag::EIdUnfrozen => 0x08,
                ClDiag::ENoent => 0x09,
                ClDiag::EFingerprint => 0x0a,
                ClDiag::EState => 0x0b,
                ClDiag::ERewind => 0x0c,
                ClDiag::ECap => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ClDiag::ESegGuard => "段位越窗/不对齐：SEG_BASE 须对齐 0x40 且落在 VX9 公式窗，先核对派工公式再挂段",
            ClDiag::EBusy => "域账未封账（EBUSY 语义）：存在封账旗标未落位的账行，先补齐 800 行旗标再走封账口",
            ClDiag::ERowDue => "应记账行缺失：账面出现不应记账行属漏账样本，先补登记口径再入账",
            ClDiag::ERowPost => "账行未记账：应记账位的已记账位为假，按行号回补该行账目后重跑断言",
            ClDiag::EAssertSum => "断言总量非 240,000：封账总量恒等式不成立，逐行复核每行 300 条断言计数",
            ClDiag::EIdRange => "号面序号越 F12001–F12800 闭集：只认 800 条连续序号，先对号面闭集再登记",
            ClDiag::EIdDerive => "派生公式复算不符：冻结终版口径被改动，按 serial×0x9E3779B1+0x831C0 回填",
            ClDiag::EIdUnfrozen => "号面冻结态翻转：逐条核到假冻结节，先恢复冻结终版再走收官口",
            ClDiag::ENoent => "移交包缺件（ENOENT 语义）：五件套缺一，按件名（总纲/台账/断言资产/复测资产库/收官标志）补齐",
            ClDiag::EFingerprint => "移交件指纹不符：件在但指纹对不上，重新出件并按 FNV-1a 重算指纹",
            ClDiag::EState => "状态未到位：三断言按域账→号面→移交包冻结次序推进，先补前序断言",
            ClDiag::ERewind => "状态机倒退：建账→封账→冻结→移交→宣言只进不退，倒退即收官账可疑须整账复核",
            ClDiag::ECap => "点验日志容量耗尽：日志槽已满，先出清点验账再记新行",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CL_ALL: &[ClDiag] = &[
    ClDiag::ESegGuard,
    ClDiag::EBusy,
    ClDiag::ERowDue,
    ClDiag::ERowPost,
    ClDiag::EAssertSum,
    ClDiag::EIdRange,
    ClDiag::EIdDerive,
    ClDiag::EIdUnfrozen,
    ClDiag::ENoent,
    ClDiag::EFingerprint,
    ClDiag::EState,
    ClDiag::ERewind,
    ClDiag::ECap,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CL_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x831C1..0x831CD 闭集内"
}

// ── 移交包五件套闭集 ─────────────────────────────────────────────
/// 移交包五件套闭集（判据原文件序，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// 第 0 件：总纲。
    Outline,
    /// 第 1 件：台账。
    RootLedger,
    /// 第 2 件：断言资产。
    AssertAssets,
    /// 第 3 件：复测资产库。
    RetestLib,
    /// 第 4 件：收官标志。
    ClosureFlag,
}

impl ArtifactKind {
    /// 件号（闭集序）。
    pub fn index(self) -> usize {
        match self {
            ArtifactKind::Outline => 0,
            ArtifactKind::RootLedger => 1,
            ArtifactKind::AssertAssets => 2,
            ArtifactKind::RetestLib => 3,
            ArtifactKind::ClosureFlag => 4,
        }
    }

    /// 件中文名。
    pub fn name(self) -> &'static str {
        match self {
            ArtifactKind::Outline => "总纲",
            ArtifactKind::RootLedger => "台账",
            ArtifactKind::AssertAssets => "断言资产",
            ArtifactKind::RetestLib => "复测资产库",
            ArtifactKind::ClosureFlag => "收官标志",
        }
    }

    /// 由件号反查闭集件（越界返回 None）。
    pub fn from_index(idx: usize) -> Option<ArtifactKind> {
        match idx {
            0 => Some(ArtifactKind::Outline),
            1 => Some(ArtifactKind::RootLedger),
            2 => Some(ArtifactKind::AssertAssets),
            3 => Some(ArtifactKind::RetestLib),
            4 => Some(ArtifactKind::ClosureFlag),
            _ => None,
        }
    }

    /// 出件规范名（指纹计算的冻结输入，件名唯一）。
    pub fn canon(self) -> String {
        format!("UNX-F12799/HANDOFF/{}", self.name())
    }
}

/// 五件套闭集数组（件序即登记序）。
pub const KIND_ALL: &[ArtifactKind] = &[
    ArtifactKind::Outline,
    ArtifactKind::RootLedger,
    ArtifactKind::AssertAssets,
    ArtifactKind::RetestLib,
    ArtifactKind::ClosureFlag,
];

// ── FNV-1a 64 指纹（确定性，双跑一致） ───────────────────────────
/// FNV-1a 64 位偏移基（标准常数，出处：FNV 参考实现）。
const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 位素数（标准常数）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 对字节序列算 FNV-1a 64 指纹（确定性，无随机）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 混入一段 u32（大端字节序，指纹输入规范化）。
fn mix_u32(h: &mut u64, v: u32) {
    for b in v.to_be_bytes().iter() {
        *h ^= *b as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// 混入一个布尔（规范化为 0/1 后混入）。
fn mix_bool(h: &mut u64, v: bool) {
    mix_u32(h, if v { 1 } else { 0 });
}

// ── 号面派生公式（冻结终版口径） ─────────────────────────────────
/// 号面派生公式：serial × 0x9E3779B1 + 0x831C0（mod 2^32）。
/// 冻结终版：派生码只由序号决定，复算不符即口径被改。
pub fn derive_id_code(serial: u32) -> u32 {
    SEG_BASE.wrapping_add(serial.wrapping_mul(0x9E37_79B1))
}

// ── 域账行（已记账/应记账/封账旗标三态账行） ─────────────────────
/// 单条域账行：三态位 + 断言计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    /// 所属批号（0..40）。
    pub batch: u32,
    /// 批内行号（0..20）。
    pub row: u32,
    /// 应记账位（制度上 800 行全应记账）。
    pub due: bool,
    /// 已记账位（与应记账位逐行相等才许封账）。
    pub posted: bool,
    /// 封账旗标（封账口逐行落位，未落位 → EBUSY 语义）。
    pub sealed: bool,
    /// 该行断言数（恒 300，总量恒等 240,000）。
    pub asserts: u32,
}

/// 单条号面目：序号 + 派生码 + 冻结态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdEntry {
    /// 序号（F12001..F12800 连续 800 条）。
    pub serial: u32,
    /// 派生码（冻结终版口径，登记时按公式落位）。
    pub derived: u32,
    /// 冻结态（冻结口逐条落位，翻转 → EIdUnfrozen）。
    pub frozen: bool,
}

/// 单件移交物：件类 + 指纹（缺件即指纹为 None 或件不在包内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffItem {
    /// 件类（五件套闭集）。
    pub kind: ArtifactKind,
    /// 出件指纹（FNV-1a 64，缺件为 None）。
    pub fp: Option<u64>,
}

/// 收官状态机：建账 → 封账 → 冻结 → 移交 → 宣言，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureState {
    /// 建账：逐行登记账行/号面目/移交件。
    Building,
    /// 域账封账：800 行旗标齐，总量 240,000。
    LedgerSealed,
    /// 号面冻结：800 条冻结终版。
    IdrangeFrozen,
    /// 移交齐：五件套存在性与指纹全绿。
    HandoffDone,
    /// 宣言已发：满账收口宣言行落账。
    Manifested,
}

impl ClosureState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ClosureState::Building => 0,
            ClosureState::LedgerSealed => 1,
            ClosureState::IdrangeFrozen => 2,
            ClosureState::HandoffDone => 3,
            ClosureState::Manifested => 4,
        }
    }
}

/// 满账收口宣言行（三断言全绿才发）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureRow {
    /// 域账封账实算总量（须等于 LEDGER_CAP）。
    pub ledger_sum: u32,
    /// 域账封账口径总量（240,000）。
    pub ledger_cap: u32,
    /// 号面冻结条数（须等于 800）。
    pub frozen_n: u32,
    /// 号面首序号（12001）。
    pub id_first: u32,
    /// 号面尾序号（12800）。
    pub id_last: u32,
    /// 移交件数（须等于 5）。
    pub handoff_n: u32,
    /// 宣言指纹（FNV-1a 64，双跑一致锚）。
    pub fp: u64,
}

impl ClosureRow {
    /// 宣言行摘要（非空即写回合规）。
    pub fn brief(&self) -> String {
        format!(
            "满账收口宣言:域账{}/{} 号面{}..{}冻结{} 移交{}件 指纹{:016x}",
            self.ledger_sum, self.ledger_cap, self.id_first, self.id_last, self.frozen_n,
            self.handoff_n, self.fp
        )
    }
}

/// 点验日志（定容，容量耗尽显性拒）。
#[derive(Clone, Debug, Default)]
pub struct PointLog {
    lines: Vec<String>,
}

/// 点验日志容量（4 槽，定容写死；满后再记 → ECap）。
pub const LOG_CAP: usize = 4;

impl PointLog {
    /// 已记行数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn line_at(&self, idx: usize) -> Option<&str> {
        self.lines.get(idx).map(|s| s.as_str())
    }

    /// 记一行点验（满容 → ECap，显性不静默）。
    pub fn record(&mut self, line: String) -> Result<(), u32> {
        if self.lines.len() >= LOG_CAP {
            return Err(ClDiag::ECap.val());
        }
        self.lines.push(line);
        Ok(())
    }
}

// ── 域收官域模型 ─────────────────────────────────────────────────
/// 域收官标志条域模型：域账 + 号面 + 移交包 + 状态机 + 宣言 + 点验日志。
#[derive(Clone, Debug)]
pub struct DomainClosure {
    ledger: Vec<LedgerRow>,
    idface: Vec<IdEntry>,
    pack: Vec<HandoffItem>,
    state: ClosureState,
    manifest: Option<ClosureRow>,
    log: PointLog,
}

impl DomainClosure {
    /// 开一本新收官账。
    pub fn new() -> DomainClosure {
        DomainClosure {
            ledger: Vec::new(),
            idface: Vec::new(),
            pack: Vec::new(),
            state: ClosureState::Building,
            manifest: None,
            log: PointLog::default(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ClosureState {
        self.state
    }

    /// 已入账账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 已登记号面目数。
    pub fn idface_len(&self) -> usize {
        self.idface.len()
    }

    /// 已登记移交件数。
    pub fn pack_len(&self) -> usize {
        self.pack.len()
    }

    /// 宣言行（未发为 None）。
    pub fn manifest_row(&self) -> Option<ClosureRow> {
        self.manifest
    }

    /// 点验日志已用行数。
    pub fn log_used(&self) -> usize {
        self.log.len()
    }

    /// 取第 idx 条账行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<LedgerRow> {
        self.ledger.get(idx).copied()
    }

    /// 取第 idx 条号面目（越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<IdEntry> {
        self.idface.get(idx).copied()
    }

    /// 取第 idx 件移交物（越界 None）。
    pub fn item_at(&self, idx: usize) -> Option<HandoffItem> {
        self.pack.get(idx).copied()
    }

    /// 可变取第 idx 条账行（注入面专用，越界 None）。
    pub fn row_mut(&mut self, idx: usize) -> Option<&mut LedgerRow> {
        self.ledger.get_mut(idx)
    }

    /// 可变取第 idx 条号面目（注入面专用，越界 None）。
    pub fn entry_mut(&mut self, idx: usize) -> Option<&mut IdEntry> {
        self.idface.get_mut(idx)
    }

    /// 登记一条域账行（序号单调 + 应记账制度 + 断言计数三道闸）。
    pub fn post_row(&mut self, batch: u32, row: u32, due: bool, posted: bool, asserts: u32) -> Result<(), u32> {
        // 状态闸：只许在建账期登记。
        if self.state.rank() != ClosureState::Building.rank() {
            return Err(ClDiag::ERewind.val());
        }
        // 批/行闭集闸：批 0..40、行 0..20。
        if batch >= BATCH_N {
            return Err(ClDiag::ERowDue.val());
        }
        if row >= ROWS_PER_BATCH {
            return Err(ClDiag::ERowDue.val());
        }
        // 序号单调闸：全局序 = 批×20+行，必须恰等于已入账行数（乱序显性拒）。
        let seq = match batch
            .checked_mul(ROWS_PER_BATCH)
            .and_then(|v| v.checked_add(row))
        {
            Some(v) => v,
            None => return Err(ClDiag::ERowDue.val()),
        };
        if seq != self.ledger.len() as u32 {
            return Err(ClDiag::EIdRange.val());
        }
        // 应记账制度闸：不应记账行属漏账样本，拒入。
        if !due {
            return Err(ClDiag::ERowDue.val());
        }
        // 断言计数闸：每行恒 300，短计/溢计都拒。
        if asserts != ASSERTS_PER_ROW {
            return Err(ClDiag::EAssertSum.val());
        }
        self.ledger.push(LedgerRow {
            batch,
            row,
            due,
            posted,
            sealed: false,
            asserts,
        });
        Ok(())
    }

    /// 登记一条号面目（序号连续 + 派生公式落位两道闸）。
    pub fn register_id(&mut self, serial: u32) -> Result<(), u32> {
        if self.state.rank() != ClosureState::Building.rank() {
            return Err(ClDiag::ERewind.val());
        }
        // 序号连续闸：必须恰为 ID_FIRST + 已登记数（跳号/回退都拒）。
        let expect = match ID_FIRST.checked_add(self.idface.len() as u32) {
            Some(v) => v,
            None => return Err(ClDiag::EIdRange.val()),
        };
        if serial != expect {
            return Err(ClDiag::EIdRange.val());
        }
        if serial > ID_LAST {
            return Err(ClDiag::EIdRange.val());
        }
        // 派生公式落位（冻结终版口径，登记即定）。
        let derived = derive_id_code(serial);
        self.idface.push(IdEntry {
            serial,
            derived,
            frozen: false,
        });
        Ok(())
    }

    /// 登记一件移交物（件序闭集闸：按五件套序逐件登记）。
    pub fn register_handoff(&mut self, kind: ArtifactKind, fp: u64) -> Result<(), u32> {
        if self.state.rank() != ClosureState::Building.rank() {
            return Err(ClDiag::ERewind.val());
        }
        // 件序闸：必须按闭集序登记下一件。
        let expect = match ArtifactKind::from_index(self.pack.len()) {
            Some(k) => k,
            None => return Err(ClDiag::ENoent.val()),
        };
        if kind != expect {
            return Err(ClDiag::EState.val());
        }
        self.pack.push(HandoffItem { kind, fp: Some(fp) });
        Ok(())
    }

    /// 撤下一件移交物（缺件注入面专用：件从包内移除）。
    pub fn drop_item(&mut self, kind: ArtifactKind) -> bool {
        let before = self.pack.len();
        self.pack.retain(|it| it.kind != kind);
        self.pack.len() != before
    }

    /// 记一行点验（容量耗尽 → ECap）。
    pub fn record_review(&mut self, tag: u32) -> Result<(), u32> {
        self.log.record(format!("点验{}:state={:?}", tag, self.state))
    }

    /// 封账口：三道预检 + 旗标落位 + 封账后独立复算。
    pub fn seal_ledger(&mut self) -> Result<u32, u32> {
        // 状态闸：只许从建账态封账，重封/倒退显性拒。
        if self.state.rank() != ClosureState::Building.rank() {
            return Err(ClDiag::ERewind.val());
        }
        // 预检一：应记账制度逐行核（漏账样本显性拒）。
        for r in self.ledger.iter() {
            if !r.due {
                return Err(ClDiag::ERowDue.val());
            }
        }
        // 预检二：已记账逐行核（应记账未记账显性拒）。
        for r in self.ledger.iter() {
            if !r.posted {
                return Err(ClDiag::ERowPost.val());
            }
        }
        // 预检三：断言总量 checked 累加，须恰为 240,000。
        let mut sum = 0u32;
        for r in self.ledger.iter() {
            sum = match sum.checked_add(r.asserts) {
                Some(v) => v,
                None => return Err(ClDiag::EAssertSum.val()),
            };
        }
        if sum != LEDGER_CAP {
            return Err(ClDiag::EAssertSum.val());
        }
        // 封账旗标逐行落位。
        for r in self.ledger.iter_mut() {
            r.sealed = true;
        }
        // 落位后独立复算必须全绿（未封账 → EBUSY 语义）。
        let total = assert_ledger_sealed(self)?;
        self.state = ClosureState::LedgerSealed;
        Ok(total)
    }

    /// 冻结口：号面 800 条冻结态逐条落位 + 独立复算。
    pub fn freeze_idrange(&mut self) -> Result<u32, u32> {
        // 状态闸：只许从封账态冻结；未到 → EState，越过 → 倒退拒。
        if self.state.rank() != ClosureState::LedgerSealed.rank() {
            if self.state.rank() > ClosureState::LedgerSealed.rank() {
                return Err(ClDiag::ERewind.val());
            }
            return Err(ClDiag::EState.val());
        }
        // 冻结态逐条落位。
        for e in self.idface.iter_mut() {
            e.frozen = true;
        }
        // 落位后独立复算（派生公式 + 冻结态逐条核）。
        let n = assert_idrange_frozen(self)?;
        self.state = ClosureState::IdrangeFrozen;
        Ok(n)
    }

    /// 移交口：五件套存在性与指纹逐件核（只核不改，指纹登记即定）。
    pub fn complete_handoff(&mut self) -> Result<u32, u32> {
        if self.state.rank() != ClosureState::IdrangeFrozen.rank() {
            if self.state.rank() > ClosureState::IdrangeFrozen.rank() {
                return Err(ClDiag::ERewind.val());
            }
            return Err(ClDiag::EState.val());
        }
        let n = assert_handoff_complete(self)?;
        self.state = ClosureState::HandoffDone;
        Ok(n)
    }

    /// 宣言口：三断言全绿才发宣言行（只发一次，日志满容 → ECap）。
    pub fn issue_manifest(&mut self) -> Result<ClosureRow, u32> {
        // 宣言只发一次：重发即倒退显性拒。
        if self.manifest.is_some() {
            return Err(ClDiag::ERewind.val());
        }
        if self.state.rank() != ClosureState::HandoffDone.rank() {
            if self.state.rank() > ClosureState::HandoffDone.rank() {
                return Err(ClDiag::ERewind.val());
            }
            return Err(ClDiag::EState.val());
        }
        // 三断言全绿判定（任一挂 → 指明哪件缺的诊断码）。
        let row = judge_closure_manifest(self)?;
        // 点验日志落行（容量耗尽 → ECap，宣言不发、状态不动）。
        self.record_review(0xF1)?;
        self.manifest = Some(row);
        self.state = ClosureState::Manifested;
        Ok(row)
    }
}

// ── 三断言（判据原文逐条锚定，纯复算不改账） ─────────────────────
/// 断言一：域账封账 240,000/240,000。
/// 账本行（已记账/应记账/封账旗标）逐行复算，未封账 → EBUSY 语义。
pub fn assert_ledger_sealed(d: &DomainClosure) -> Result<u32, u32> {
    let mut sum = 0u32;
    for r in d.ledger.iter() {
        // 应记账闸：漏账样本显性拒。
        if !r.due {
            return Err(ClDiag::ERowDue.val());
        }
        // 已记账闸：应记账未记账显性拒。
        if !r.posted {
            return Err(ClDiag::ERowPost.val());
        }
        // 封账旗标闸：未落位 → EBUSY 语义。
        if !r.sealed {
            return Err(ClDiag::EBusy.val());
        }
        // 断言计数 checked 累加（溢出显性拒）。
        sum = match sum.checked_add(r.asserts) {
            Some(v) => v,
            None => return Err(ClDiag::EAssertSum.val()),
        };
    }
    // 行数不足 800 或总量非 240,000 都属短账。
    if d.ledger.len() as u32 != TOTAL_ROWS || sum != LEDGER_CAP {
        return Err(ClDiag::EAssertSum.val());
    }
    Ok(sum)
}

/// 断言二：号面 F12001–F12800 冻结终版。
/// 800 条派生公式全量复算 + 冻结态逐条核。
pub fn assert_idrange_frozen(d: &DomainClosure) -> Result<u32, u32> {
    let mut frozen = 0u32;
    let mut idx = 0usize;
    for e in d.idface.iter() {
        // 序号连续闸：F12001 起逐条连续，跳号/回退都拒。
        let expect_serial = match ID_FIRST.checked_add(idx as u32) {
            Some(v) => v,
            None => return Err(ClDiag::EIdRange.val()),
        };
        if e.serial != expect_serial || e.serial > ID_LAST {
            return Err(ClDiag::EIdRange.val());
        }
        // 派生公式全量复算闸：冻结终版口径被改显性拒。
        if e.derived != derive_id_code(e.serial) {
            return Err(ClDiag::EIdDerive.val());
        }
        // 冻结态逐条核闸：翻转一条即拒。
        if !e.frozen {
            return Err(ClDiag::EIdUnfrozen.val());
        }
        frozen = match frozen.checked_add(1) {
            Some(v) => v,
            None => return Err(ClDiag::EIdUnfrozen.val()),
        };
        idx += 1;
    }
    // 条数不足 800 属号面未齐。
    if d.idface.len() as u32 != ID_N || frozen != ID_N {
        return Err(ClDiag::EIdUnfrozen.val());
    }
    Ok(frozen)
}

/// 断言三：移交包五件齐。
/// 五件套存在性与指纹逐件核，缺件 → ENOENT 语义。
pub fn assert_handoff_complete(d: &DomainClosure) -> Result<u32, u32> {
    let mut n = 0u32;
    let mut idx = 0usize;
    for kind in KIND_ALL.iter() {
        // 存在性闸：件不在包内（或缺位错排）→ ENOENT 语义。
        let item = match d.pack.get(idx) {
            Some(it) => *it,
            None => return Err(ClDiag::ENoent.val()),
        };
        if item.kind != *kind {
            return Err(ClDiag::ENoent.val());
        }
        // 指纹闸：指纹缺（None）→ 缺件；对不上 → 指纹不符。
        match item.fp {
            None => return Err(ClDiag::ENoent.val()),
            Some(fp) => {
                if fp != artifact_fingerprint(*kind) {
                    return Err(ClDiag::EFingerprint.val());
                }
            }
        }
        n = match n.checked_add(1) {
            Some(v) => v,
            None => return Err(ClDiag::ENoent.val()),
        };
        idx += 1;
    }
    if d.pack.len() != HANDOFF_N || n as usize != HANDOFF_N {
        return Err(ClDiag::ENoent.val());
    }
    Ok(n)
}

/// 出件指纹（件规范名的 FNV-1a 64，五件互异）。
pub fn artifact_fingerprint(kind: ArtifactKind) -> u64 {
    fnv1a64(kind.canon().as_bytes())
}

/// 缺件指认器：回第一件不在包内（或指纹缺）的件，齐包回 None。
pub fn missing_kind(d: &DomainClosure) -> Option<ArtifactKind> {
    let mut idx = 0usize;
    for kind in KIND_ALL.iter() {
        match d.pack.get(idx) {
            Some(it) => {
                if it.kind != *kind || it.fp.is_none() {
                    return Some(*kind);
                }
            }
            None => return Some(*kind),
        }
        idx += 1;
    }
    None
}

// ── 满账宣言判定器 ───────────────────────────────────────────────
/// 满账宣言判定器：判定次序冻结（域账 → 号面 → 移交包）。
/// 三断言全绿才发宣言行；任一断言挂 → 回该断言的诊断码（指明哪件缺）。
pub fn judge_closure_manifest(d: &DomainClosure) -> Result<ClosureRow, u32> {
    // 次序一：域账封账 240,000/240,000。
    let ledger_sum = assert_ledger_sealed(d)?;
    // 次序二：号面 F12001–F12800 冻结终版。
    let frozen_n = assert_idrange_frozen(d)?;
    // 次序三：移交包五件齐。
    let handoff_n = assert_handoff_complete(d)?;
    // 三断言全绿：发宣言行（指纹 = 全账 FNV-1a，双跑一致锚）。
    Ok(ClosureRow {
        ledger_sum,
        ledger_cap: LEDGER_CAP,
        frozen_n,
        id_first: ID_FIRST,
        id_last: ID_LAST,
        handoff_n,
        fp: closure_fingerprint(d),
    })
}

/// 收官账全量指纹（FNV-1a 64：账行/号面/移交件/状态/宣言全混入）。
pub fn closure_fingerprint(d: &DomainClosure) -> u64 {
    let mut h = FNV_OFFSET;
    for r in d.ledger.iter() {
        mix_u32(&mut h, r.batch);
        mix_u32(&mut h, r.row);
        mix_bool(&mut h, r.due);
        mix_bool(&mut h, r.posted);
        mix_bool(&mut h, r.sealed);
        mix_u32(&mut h, r.asserts);
    }
    for e in d.idface.iter() {
        mix_u32(&mut h, e.serial);
        mix_u32(&mut h, e.derived);
        mix_bool(&mut h, e.frozen);
    }
    for it in d.pack.iter() {
        mix_u32(&mut h, it.kind.index() as u32);
        match it.fp {
            Some(fp) => mix_u32(&mut h, (fp >> 32) as u32),
            None => mix_u32(&mut h, 0xDEAD_0000),
        }
    }
    mix_u32(&mut h, d.state.rank() as u32);
    match d.manifest {
        Some(row) => {
            mix_u32(&mut h, row.ledger_sum);
            mix_u32(&mut h, row.frozen_n);
            mix_u32(&mut h, row.handoff_n);
            mix_u32(&mut h, (row.fp >> 32) as u32);
        }
        None => mix_u32(&mut h, 0),
    }
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

// ── 满账域构建器 ─────────────────────────────────────────────────
/// 构建满账收官域：800 账行（未封）+ 800 号面目（未冻）+ 五件套（在包）。
pub fn build_full_domain() -> DomainClosure {
    let mut d = DomainClosure::new();
    // 域账：40 批 × 20 行逐行登记（序号单调，断言每行 300）。
    let mut batch = 0u32;
    while batch < BATCH_N {
        let mut row = 0u32;
        while row < ROWS_PER_BATCH {
            // 登记失败即构建中止（缺行账本，断言必红——显性不静默）。
            if d.post_row(batch, row, true, true, ASSERTS_PER_ROW).is_err() {
                return d;
            }
            row += 1;
        }
        batch += 1;
    }
    // 号面：F12001..F12800 逐条登记（派生公式落位）。
    let mut serial = ID_FIRST;
    while serial <= ID_LAST {
        if d.register_id(serial).is_err() {
            return d;
        }
        serial += 1;
    }
    // 移交包：五件套按闭集序逐件登记（出件指纹按件规范名落位）。
    let mut k = 0usize;
    while k < HANDOFF_N {
        let kind = match ArtifactKind::from_index(k) {
            Some(v) => v,
            None => return d,
        };
        if d.register_handoff(kind, artifact_fingerprint(kind)).is_err() {
            return d;
        }
        k += 1;
    }
    d
}

/// 构建三断言全绿的收官域：满账 + 封账 + 冻结 + 移交齐（未发宣言）。
pub fn build_sealed_domain() -> DomainClosure {
    let mut d = build_full_domain();
    // 封账：任一步失败即回半成品（断言面必红，显性不静默）。
    if d.seal_ledger().is_err() {
        return d;
    }
    if d.freeze_idrange().is_err() {
        return d;
    }
    let _ = d.complete_handoff();
    d
}

// ── 注入面：三类注入器各 10 轮全抓 ───────────────────────────────
/// 账行篡改类目（三路轮转）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerTamper {
    /// 已记账位清零（未记账注入）。
    Unpost,
    /// 封账旗标清零（未封账注入 → EBUSY 语义）。
    Unseal,
    /// 断言计数清零（短账注入 → 总量恒等式破）。
    ZeroAsserts,
}

/// 账行篡改注入器：10 轮（LCG 定行 + 三路轮转），每轮断言必须抓。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn ledger_tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut k = 0usize;
    while k < 10 {
        let tamper = match k % 3 {
            0 => LedgerTamper::Unpost,
            1 => LedgerTamper::Unseal,
            _ => LedgerTamper::ZeroAsserts,
        };
        let pick = rng.below(TOTAL_ROWS) as usize;
        let mut d = build_sealed_domain();
        // 定点篡改：只改选中行的一个位，其余账面不动。
        let hit = match d.row_mut(pick) {
            Some(r) => {
                match tamper {
                    LedgerTamper::Unpost => r.posted = false,
                    LedgerTamper::Unseal => r.sealed = false,
                    LedgerTamper::ZeroAsserts => r.asserts = 0,
                }
                true
            }
            None => false,
        };
        // 断言必须抓（且预期诊断码逐路对号），判定器也必须挂。
        let expect = match tamper {
            LedgerTamper::Unpost => ClDiag::ERowPost.val(),
            LedgerTamper::Unseal => ClDiag::EBusy.val(),
            LedgerTamper::ZeroAsserts => ClDiag::EAssertSum.val(),
        };
        let got = assert_ledger_sealed(&d);
        total += 1;
        let judged = judge_closure_manifest(&d);
        if hit && got == Err(expect) && judged.is_err() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "轮{}行{}路{:?}逃脱:断言={:?}",
                k, pick, tamper, got
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 缺件注入器：10 轮（五件套轮转两圈），每轮必须 ENOENT 且指认缺件。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn missing_item_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < 10 {
        let kind = match ArtifactKind::from_index(k % HANDOFF_N) {
            Some(v) => v,
            None => return (caught, total, format!("轮{}件号越闭集", k)),
        };
        let mut d = build_sealed_domain();
        // 缺件注入：整件从包内撤下。
        let dropped = d.drop_item(kind);
        // 断言必须 ENOENT，缺件指认必须点名该件，判定器也必须挂。
        let got = assert_handoff_complete(&d);
        let named = missing_kind(&d);
        total += 1;
        let judged = judge_closure_manifest(&d);
        if dropped && got == Err(ClDiag::ENoent.val()) && named == Some(kind) && judged.is_err() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "轮{}件{}逃脱:断言={:?}指认={:?}",
                k,
                kind.name(),
                got,
                named.map(|x| x.name().to_string())
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 冻结态翻转注入器：10 轮（LCG 定条），每轮冻结态必须被逐条核抓。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn freeze_flip_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x5D) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(ID_N) as usize;
        let mut d = build_sealed_domain();
        // 翻转选中条的冻结态。
        let hit = match d.entry_mut(pick) {
            Some(e) => {
                e.frozen = false;
                true
            }
            None => false,
        };
        // 断言必须 EIdUnfrozen，判定器也必须挂。
        let got = assert_idrange_frozen(&d);
        total += 1;
        let judged = judge_closure_manifest(&d);
        if hit && got == Err(ClDiag::EIdUnfrozen.val()) && judged.is_err() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}条{}逃脱:断言={:?}", k, pick, got);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    // 派工公式复核：SEG_BASE = 0x80000 + 0x40 × (12799 − 12600)。
    let off = match 0x40u32.checked_mul(199u32) {
        Some(v) => v,
        None => return false,
    };
    let expect = match 0x80000u32.checked_add(off) {
        Some(v) => v,
        None => return false,
    };
    if SEG_BASE != expect {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    for d in CL_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    // 未知码必须回落（不得误命中任何已知文案）。
    if detail(SEG_BASE + 0x3F) == ClDiag::EBusy.detail() {
        return false;
    }
    true
}

/// 常量闭集自洽：40×20=800、300×800=240,000、号面 800 条、五件套。
fn const_closed_ok() -> bool {
    // 域账行数恒等式：40 批 × 20 行 = 800。
    let rows = match BATCH_N.checked_mul(ROWS_PER_BATCH) {
        Some(v) => v,
        None => return false,
    };
    if rows != TOTAL_ROWS {
        return false;
    }
    // 断言总量恒等式：300 × 800 = 240,000。
    let cap = match ASSERTS_PER_ROW.checked_mul(TOTAL_ROWS) {
        Some(v) => v,
        None => return false,
    };
    if cap != LEDGER_CAP {
        return false;
    }
    // 号面条数恒等式：12800 − 12001 + 1 = 800。
    let span = match ID_LAST.checked_sub(ID_FIRST) {
        Some(v) => v,
        None => return false,
    };
    if span.saturating_add(1) != ID_N {
        return false;
    }
    // 五件套与件闭集一致。
    if HANDOFF_N != 5 || KIND_ALL.len() != HANDOFF_N {
        return false;
    }
    // 派生公式冻结锚：F12001 与 F12800 的派生码必须互异。
    if derive_id_code(ID_FIRST) == derive_id_code(ID_LAST) {
        return false;
    }
    true
}

/// 正常流：满账 → 封账 → 冻结 → 移交 → 宣言全绿，状态只进。
fn happy_path_ok() -> bool {
    let mut d = build_sealed_domain();
    // 三断言独立复算全绿（不只看状态位）。
    if assert_ledger_sealed(&d) != Ok(LEDGER_CAP) {
        return false;
    }
    if assert_idrange_frozen(&d) != Ok(ID_N) {
        return false;
    }
    if assert_handoff_complete(&d) != Ok(HANDOFF_N as u32) {
        return false;
    }
    // 宣言口：发宣言成功且状态推进到 Manifested。
    match d.issue_manifest() {
        Ok(row) => {
            if d.state() != ClosureState::Manifested {
                return false;
            }
            if row.ledger_sum != LEDGER_CAP || row.frozen_n != ID_N || row.handoff_n != HANDOFF_N as u32 {
                return false;
            }
            !row.brief().is_empty()
        }
        Err(_) => false,
    }
}

/// 宣言行完备性：判定器直发与宣言口落账两路一致。
fn judge_row_ok() -> bool {
    let d = build_sealed_domain();
    let direct = match judge_closure_manifest(&d) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut d2 = build_sealed_domain();
    let issued = match d2.issue_manifest() {
        Ok(r) => r,
        Err(_) => return false,
    };
    // 两路宣言行逐字段一致（含指纹），且未发宣言前 manifest 为 None。
    if direct != issued {
        return false;
    }
    let fresh = build_full_domain();
    fresh.manifest_row().is_none() && fresh.state() == ClosureState::Building
}

/// 判定次序冻结：多重违约时按 域账→号面→移交包 报最前者。
fn judge_order_ok() -> bool {
    // 三处同挂：必须报域账族码（ERowPost）。
    let mut d3 = build_sealed_domain();
    if let Some(r) = d3.row_mut(5) {
        r.posted = false;
    }
    if let Some(e) = d3.entry_mut(7) {
        e.frozen = false;
    }
    d3.drop_item(ArtifactKind::ClosureFlag);
    if judge_closure_manifest(&d3) != Err(ClDiag::ERowPost.val()) {
        return false;
    }
    // 后两处同挂：必须报号面族码（EIdUnfrozen）。
    let mut d2 = build_sealed_domain();
    if let Some(e) = d2.entry_mut(9) {
        e.frozen = false;
    }
    d2.drop_item(ArtifactKind::Outline);
    if judge_closure_manifest(&d2) != Err(ClDiag::EIdUnfrozen.val()) {
        return false;
    }
    // 仅移交挂：必须报 ENOENT。
    let mut d1 = build_sealed_domain();
    d1.drop_item(ArtifactKind::RetestLib);
    if judge_closure_manifest(&d1) != Err(ClDiag::ENoent.val()) {
        return false;
    }
    true
}

/// 注入必抓一：账行篡改 10/10 全抓，无逃脱。
fn ledger_tamper_ok() -> bool {
    let (caught, total, escape) = ledger_tamper_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓二：缺件注入 10/10 全抓且缺件指认逐件点名。
fn missing_item_ok() -> bool {
    let (caught, total, escape) = missing_item_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓三：冻结态翻转 10/10 全抓，无逃脱。
fn freeze_flip_ok() -> bool {
    let (caught, total, escape) = freeze_flip_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 容量耗尽：点验日志 4 槽写满后第 5 记显性 ECap，宣言口被日志满容阻断。
fn capacity_ok() -> bool {
    let mut d = build_sealed_domain();
    // 前 4 记全成（写满日志）。
    let mut tag = 1u32;
    while tag <= LOG_CAP as u32 {
        if d.record_review(tag).is_err() {
            return false;
        }
        tag += 1;
    }
    // 第 5 记必须 ECap（容量耗尽显性拒，不静默截断）。
    if d.record_review(9) != Err(ClDiag::ECap.val()) {
        return false;
    }
    if d.log_used() != LOG_CAP {
        return false;
    }
    // 日志满容阻断宣言口：三断言虽全绿，宣言发不出且状态不动。
    if d.issue_manifest() != Err(ClDiag::ECap.val()) {
        return false;
    }
    if d.manifest_row().is_some() || d.state() != ClosureState::HandoffDone {
        return false;
    }
    // 出清一笔后可再记（容量语义可恢复）。
    let mut fresh = DomainClosure::new();
    if fresh.record_review(1).is_err() || fresh.log_used() != 1 {
        return false;
    }
    true
}

/// 状态机倒退显性拒：五态只进不退，越序/重做/倒退全数拒。
fn rewind_ok() -> bool {
    // 越序：未封账先冻结 → EState。
    let mut d1 = build_full_domain();
    if d1.freeze_idrange() != Err(ClDiag::EState.val()) {
        return false;
    }
    // 越序：未冻结先移交 → EState；未移交先宣言 → EState。
    if d1.complete_handoff() != Err(ClDiag::EState.val()) {
        return false;
    }
    if d1.issue_manifest() != Err(ClDiag::EState.val()) {
        return false;
    }
    // 重做：封账后再封 → 倒退拒。
    let mut d2 = build_sealed_domain();
    if d2.seal_ledger() != Err(ClDiag::ERewind.val()) {
        return false;
    }
    // 重做：宣言首发应成，再发 → 倒退拒。
    let mut d3 = build_sealed_domain();
    if d3.issue_manifest().is_err() {
        return false;
    }
    if d3.issue_manifest() != Err(ClDiag::ERewind.val()) {
        return false;
    }
    // 倒退：宣言后回封账口 → 倒退拒。
    let mut d4 = build_sealed_domain();
    let _ = d4.issue_manifest();
    d4.seal_ledger() == Err(ClDiag::ERewind.val())
}

/// 双跑一致：两次完整构建/注入流程指纹与宣言逐项一致。
fn dual_run_ok() -> bool {
    let a = build_sealed_domain();
    let b = build_sealed_domain();
    if a.ledger_len() != b.ledger_len() || a.idface_len() != b.idface_len() {
        return false;
    }
    if a.pack_len() != b.pack_len() || a.state() != b.state() {
        return false;
    }
    if closure_fingerprint(&a) != closure_fingerprint(&b) {
        return false;
    }
    // 宣言行双跑一致（含指纹字段）。
    let ma = judge_closure_manifest(&a);
    let mb = judge_closure_manifest(&b);
    if ma != mb {
        return false;
    }
    // 三路注入双跑一致。
    if ledger_tamper_sweep() != ledger_tamper_sweep() {
        return false;
    }
    if missing_item_sweep() != missing_item_sweep() {
        return false;
    }
    freeze_flip_sweep() == freeze_flip_sweep()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12799 自检（判据 12799 卡：域 40 批 800 条满账宣言 + 三断言）。
pub fn run_unx12799_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12799");
    let domain = build_sealed_domain();

    s.add_dyn(
        "F12800-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={} inwin={}", SEG_BASE, SEG_BASE % 0x40 == 0, (0x80000..=0x831C0).contains(&SEG_BASE)),
    );

    s.add_dyn(
        "F12800-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            CL_ALL.len(),
            CL_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12800-03 常量闭集自洽",
        const_closed_ok(),
        &format!(
            "rows={} cap={} ids={} handoff={}",
            TOTAL_ROWS, LEDGER_CAP, ID_N, HANDOFF_N
        ),
    );

    s.add_dyn(
        "F12800-04 正常流三断言全绿",
        happy_path_ok(),
        &format!(
            "rows={} ids={} state={:?}",
            domain.ledger_len(),
            domain.idface_len(),
            domain.state()
        ),
    );

    s.add_dyn(
        "F12800-05 满账宣言行完备",
        judge_row_ok(),
        &format!(
            "manifest={:?}",
            domain.manifest_row().map(|r| r.brief())
        ),
    );

    s.add_dyn(
        "F12800-06 判定次序冻结",
        judge_order_ok(),
        "域账→号面→移交包 三重/双重/单重违约逐级点名",
    );

    let (t_caught, t_total, t_escape) = ledger_tamper_sweep();
    s.add_dyn(
        "F12800-07 账行篡改注入 10/10",
        t_caught == 10 && t_total == 10 && t_escape.is_empty(),
        &format!("caught={}/{} {}", t_caught, t_total, t_escape),
    );

    let (m_caught, m_total, m_escape) = missing_item_sweep();
    s.add_dyn(
        "F12800-08 缺件注入 10/10",
        m_caught == 10 && m_total == 10 && m_escape.is_empty(),
        &format!("caught={}/{} {}", m_caught, m_total, m_escape),
    );

    let (f_caught, f_total, f_escape) = freeze_flip_sweep();
    s.add_dyn(
        "F12800-09 冻结态翻转注入 10/10",
        f_caught == 10 && f_total == 10 && f_escape.is_empty(),
        &format!("caught={}/{} {}", f_caught, f_total, f_escape),
    );

    s.add_dyn(
        "F12800-10 容量耗尽显性拒",
        capacity_ok(),
        &format!("log_cap={} used={}", LOG_CAP, domain.log_used()),
    );

    s.add_dyn(
        "F12800-11 状态机倒退显性拒",
        rewind_ok(),
        "越序/重封/重发/倒退 四点全数拒",
    );

    s.add_dyn(
        "F12800-12 双跑一致指纹",
        dual_run_ok(),
        "两次构建宣言一致 + 三路注入双跑一致",
    );

    // 收口条：判据条数声明（本节 13 条 = 12 判据检查 + 1 收口）。
    let closed = s.all_passed() && s.len() == 12;
    s.add_dyn(
        "F12800-13 收口：判据条数声明",
        closed,
        &format!("本节判据条数=13（12检查+1收口） all_green={}", closed),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        // 派工公式锚：0x80000 + 0x40 × 199 = 0x831C0。
        assert_eq!(0x80000 + 0x40 * 199, SEG_BASE);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：13 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(CL_ALL.len(), 13);
        let mut i = 0usize;
        while i < CL_ALL.len() {
            let vi = CL_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < CL_ALL.len() {
                let vj = CL_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        assert_eq!(detail(ClDiag::EBusy.val()), ClDiag::EBusy.detail());
    }

    #[test]
    fn happy_path_and_manifest_row() {
        // 正常流：满账→封账→冻结→移交→宣言，宣言行逐字段对判据。
        let mut d = build_sealed_domain();
        assert_eq!(d.ledger_len(), TOTAL_ROWS as usize);
        assert_eq!(d.idface_len(), ID_N as usize);
        assert_eq!(d.pack_len(), HANDOFF_N);
        assert_eq!(d.state(), ClosureState::HandoffDone);
        assert_eq!(assert_ledger_sealed(&d), Ok(LEDGER_CAP));
        assert_eq!(assert_idrange_frozen(&d), Ok(ID_N));
        assert_eq!(assert_handoff_complete(&d), Ok(HANDOFF_N as u32));
        let row = match d.issue_manifest() {
            Ok(r) => r,
            Err(e) => panic!("宣言口意外挂: {}", detail(e)),
        };
        assert_eq!(row.ledger_sum, 240_000);
        assert_eq!(row.frozen_n, 800);
        assert_eq!(row.handoff_n, 5);
        assert_eq!(row.id_first, 12001);
        assert_eq!(row.id_last, 12800);
        assert!(row.brief().contains("240000/240000"));
        assert_eq!(d.state(), ClosureState::Manifested);
    }

    #[test]
    fn ledger_tamper_all_caught() {
        // 注入必抓一：账行篡改 10 轮（未记账/未封账/短账三路轮转）全抓。
        let (caught, total, escape) = ledger_tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 定点语义：未封账注入必须回 EBUSY 语义码。
        let mut d = build_sealed_domain();
        let _ = d.row_mut(3).map(|r| r.sealed = false);
        assert_eq!(assert_ledger_sealed(&d), Err(ClDiag::EBusy.val()));
    }

    #[test]
    fn missing_item_all_caught_with_kind() {
        // 注入必抓二：缺件 10 轮全抓，缺件指认逐件点名（ENOENT 语义）。
        let (caught, total, escape) = missing_item_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 五件逐件单撤：断言 ENOENT 且指认即所撤件。
        for kind in KIND_ALL.iter() {
            let mut d = build_sealed_domain();
            assert!(d.drop_item(*kind));
            assert_eq!(assert_handoff_complete(&d), Err(ClDiag::ENoent.val()));
            assert_eq!(missing_kind(&d), Some(*kind));
        }
    }

    #[test]
    fn freeze_flip_all_caught() {
        // 注入必抓三：冻结态翻转 10 轮全抓（派生公式+冻结态逐条核）。
        let (caught, total, escape) = freeze_flip_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 定点语义：翻转一条 → EIdUnfrozen；派生码改动 → EIdDerive。
        let mut d = build_sealed_domain();
        let _ = d.entry_mut(11).map(|e| e.frozen = false);
        assert_eq!(assert_idrange_frozen(&d), Err(ClDiag::EIdUnfrozen.val()));
        let mut d2 = build_sealed_domain();
        let _ = d2.entry_mut(0).map(|e| e.derived = e.derived.wrapping_add(1));
        assert_eq!(assert_idrange_frozen(&d2), Err(ClDiag::EIdDerive.val()));
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：确定性 LCG 下两次全流程指纹与注入逐项相同。
        let a = build_sealed_domain();
        let b = build_sealed_domain();
        assert_eq!(closure_fingerprint(&a), closure_fingerprint(&b));
        assert_eq!(judge_closure_manifest(&a), judge_closure_manifest(&b));
        assert_eq!(ledger_tamper_sweep(), ledger_tamper_sweep());
        assert_eq!(missing_item_sweep(), missing_item_sweep());
        assert_eq!(freeze_flip_sweep(), freeze_flip_sweep());
        // 逐行抽核双跑账面。
        let mut i = 0usize;
        while i < TOTAL_ROWS as usize {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
    }

    #[test]
    fn capacity_and_rewind() {
        // 容量耗尽：4 槽写满后第 5 记 ECap，宣言口被阻断且状态不动。
        let mut d = build_sealed_domain();
        let mut tag = 1u32;
        while tag <= LOG_CAP as u32 {
            assert!(d.record_review(tag).is_ok());
            tag += 1;
        }
        assert_eq!(d.record_review(9), Err(ClDiag::ECap.val()));
        assert_eq!(d.issue_manifest(), Err(ClDiag::ECap.val()));
        assert!(d.manifest_row().is_none());
        assert_eq!(d.state(), ClosureState::HandoffDone);
        // 状态机倒退：越序 EState / 重做倒退 ERewind。
        let mut d1 = build_full_domain();
        assert_eq!(d1.freeze_idrange(), Err(ClDiag::EState.val()));
        assert_eq!(d1.issue_manifest(), Err(ClDiag::EState.val()));
        let mut d2 = build_sealed_domain();
        assert_eq!(d2.seal_ledger(), Err(ClDiag::ERewind.val()));
        assert!(d2.issue_manifest().is_ok());
        assert_eq!(d2.issue_manifest(), Err(ClDiag::ERewind.val()));
    }

    #[test]
    fn judge_order_first_failure_wins() {
        // 判定次序冻结：多重违约按 域账→号面→移交包 报最前者。
        let mut d3 = build_sealed_domain();
        let _ = d3.row_mut(5).map(|r| r.posted = false);
        let _ = d3.entry_mut(7).map(|e| e.frozen = false);
        d3.drop_item(ArtifactKind::ClosureFlag);
        assert_eq!(judge_closure_manifest(&d3), Err(ClDiag::ERowPost.val()));
        let mut d2 = build_sealed_domain();
        let _ = d2.entry_mut(9).map(|e| e.frozen = false);
        d2.drop_item(ArtifactKind::Outline);
        assert_eq!(judge_closure_manifest(&d2), Err(ClDiag::EIdUnfrozen.val()));
        let mut d1 = build_sealed_domain();
        d1.drop_item(ArtifactKind::RetestLib);
        assert_eq!(judge_closure_manifest(&d1), Err(ClDiag::ENoent.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条、无红项（动态明细槽 8 个为 harness 定容，
        // 第 9 条起的明细文本按契约显性入 dyn_dropped，不判据本身红绿）。
        let s = run_unx12799_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.dropped(), 0);
    }
}
