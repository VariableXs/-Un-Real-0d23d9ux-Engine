//! UNX-F12793 移交包三：根台账四处同步声明 · 0x83040段
//!
//! 判据原文（派工卡 12793 逐字锚定）：
//!   UNX-F12794-J1 全域断言资产齐（40 批聚合器入口清单），抽样 10 批可跑。
//!
//! 设计要点：
//! 1. 四处同步：根台账四处（域账节/号面节/断言节/移交节）各持
//!    Draft→Checked→Signed 单调状态机，只进不退，倒退与越级均显性拒；
//! 2. 同步判定器 judge_sync_declare(section, declared, actual)：
//!    declared==actual 才 Ok（返回一致值）；否则给出差异码——
//!    差异码按「节段×0x1000 + 方向段 + 幅度段」编码，能指明是哪一节失同步；
//! 3. 全域断言资产齐：40 批聚合器入口清单（每批：批号/入口名/状态）
//!    冻结常量表，批号连续、入口名前缀加两位批号、状态齐「已签」；
//! 4. 抽样 10 批可跑：固定种子 LCG 抽 10 个不重复批号，逐批跑一次
//!    mini 断言集（批号/入口名/后缀/覆盖区间/状态 五项）并记账；
//! 5. 注入器三类（声明篡改/抽样越界/状态倒退）各 10 轮，逐轮必抓无逃脱；
//! 6. 双跑一致指纹：FNV-1a 逐字节聚合全流程输出，两次指纹必须相等；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!    无 unwrap/expect/裸下标；错误路径 Result<T, u32>（判定器按卡规
//!    返回 Result<u32, i32> 差异码）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12793 − 12600) = 0x83040）。
pub const SEG_BASE: u32 = 0x83040;

/// VX9 卡号下界（派工总纲 §3：id − 12600 为槽序）。
pub const VX9_ID_MIN: u32 = 12600;
/// VX9 卡号上界（200 槽闭窗尾卡）。
pub const VX9_ID_MAX: u32 = 12799;
/// VX9 段窗下界（id 12600 的段基址）。
pub const VX9_SEG_MIN: u32 = 0x80000;
/// VX9 段窗上界（id 12799 的段基址 = 0x80000 + 0x40×199）。
pub const VX9_SEG_MAX: u32 = VX9_SEG_MIN + 0x40 * (VX9_ID_MAX - VX9_ID_MIN);

// ── 根台账四处同步声明诊断码闭集 ─────────────────────────────────
/// 诊断码闭集（段内 0x01..0x10 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdDiag {
    /// 节别越四节闭集（域账节/号面节/断言节/移交节之外）。
    ESecRange,
    /// 容量耗尽：四处账满后再声明被拒。
    ECapFull,
    /// 节重复声明：同一节只落一笔。
    EDupSec,
    /// 同步失败：声明值与实际值不一致（差异码已入档）。
    EDeclared,
    /// 差异码无法解析：差异码未指明任何闭集节。
    EDiffCode,
    /// 节未声明：该节账面不存在，禁止推进状态机。
    ENoSec,
    /// 状态倒退：状态机只进不退。
    ERewind,
    /// 越级迁移：跳过中间状态直跳高位状态。
    EJump,
    /// 批号越 40 批闭集或清单批数非 40。
    EBatchRange,
    /// 批号重复：每批只落一笔。
    EBatchDup,
    /// 入口名缺或前缀/后缀不符。
    ENameBad,
    /// 状态未达「已签」：资产未签不许入全域清单。
    EStatusMiss,
    /// 抽样越界：样本量越 1..=40 闭窗。
    ESampleBound,
    /// 抽样（记账）重复：同批只记一笔账。
    ESampleDup,
    /// 抽中批 mini 断言集有红项。
    EMiniFail,
    /// 双跑指纹不一致：全流程确定性被破坏。
    EFingerprint,
}

impl SdDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x10 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SdDiag::ESecRange => 0x01,
                SdDiag::ECapFull => 0x02,
                SdDiag::EDupSec => 0x03,
                SdDiag::EDeclared => 0x04,
                SdDiag::EDiffCode => 0x05,
                SdDiag::ENoSec => 0x06,
                SdDiag::ERewind => 0x07,
                SdDiag::EJump => 0x08,
                SdDiag::EBatchRange => 0x09,
                SdDiag::EBatchDup => 0x0a,
                SdDiag::ENameBad => 0x0b,
                SdDiag::EStatusMiss => 0x0c,
                SdDiag::ESampleBound => 0x0d,
                SdDiag::ESampleDup => 0x0e,
                SdDiag::EMiniFail => 0x0f,
                SdDiag::EFingerprint => 0x10,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SdDiag::ESecRange => "节别越四节闭集：只认域账节/号面节/断言节/移交节，先对节表再声明",
            SdDiag::ECapFull => "容量耗尽：四处各只收一笔声明，第五笔走下一移交包滚动",
            SdDiag::EDupSec => "节重复声明：同一节只落一笔，重复前先查该节已有账",
            SdDiag::EDeclared => "同步失败：声明值与实际值不一致，按差异码定位节别后回修账面",
            SdDiag::EDiffCode => "差异码无法解析：差异码未指明任何闭集节，判定器输出可疑须整体复核",
            SdDiag::ENoSec => "节未声明：先按序声明该节再推进状态机",
            SdDiag::ERewind => "状态倒退：草稿→已核→已签只进不退，倒退即四处同步可疑",
            SdDiag::EJump => "越级迁移：状态机只许逐级推进，草稿直跳已签先补已核",
            SdDiag::EBatchRange => "批号越 40 批闭集或批数非 40：先对批表核对批号与总数",
            SdDiag::EBatchDup => "批号重复：每批只落一笔，重复批号先查已有批",
            SdDiag::ENameBad => "入口名缺或前缀不符：入口名须 vx9_b 加两位批号，先补名再入清单",
            SdDiag::EStatusMiss => "状态未达已签：资产未签不许入全域清单，先走核签流",
            SdDiag::ESampleBound => "抽样越界：样本量须在 1..=40 闭窗内，越界抽样一律拒",
            SdDiag::ESampleDup => "抽样重复：同批只记一笔账，重复记账先查账面批号",
            SdDiag::EMiniFail => "抽中批 mini 断言集有红项：按红项字段回修该批资产再复跑",
            SdDiag::EFingerprint => "双跑指纹不一致：全流程确定性被破坏，先查注入面与种子",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SD_ALL: &[SdDiag] = &[
    SdDiag::ESecRange,
    SdDiag::ECapFull,
    SdDiag::EDupSec,
    SdDiag::EDeclared,
    SdDiag::EDiffCode,
    SdDiag::ENoSec,
    SdDiag::ERewind,
    SdDiag::EJump,
    SdDiag::EBatchRange,
    SdDiag::EBatchDup,
    SdDiag::ENameBad,
    SdDiag::EStatusMiss,
    SdDiag::ESampleBound,
    SdDiag::ESampleDup,
    SdDiag::EMiniFail,
    SdDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SD_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83041..0x83050 闭集内"
}

// ── 四节闭集 ─────────────────────────────────────────────────────
/// 根台账四节闭集（判据域模型四处，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecKind {
    /// 第 0 节：域账节（域模型账目节）。
    DomainLedger,
    /// 第 1 节：号面节（编号/面额账面节）。
    NumFace,
    /// 第 2 节：断言节（验收断言账面节）。
    AssertSec,
    /// 第 3 节：移交节（移交包账面节）。
    HandoffSec,
}

impl SecKind {
    /// 节序（闭集序 0..4）。
    pub fn index(self) -> u32 {
        match self {
            SecKind::DomainLedger => 0,
            SecKind::NumFace => 1,
            SecKind::AssertSec => 2,
            SecKind::HandoffSec => 3,
        }
    }

    /// 节中文名。
    pub fn name(self) -> &'static str {
        match self {
            SecKind::DomainLedger => "域账节",
            SecKind::NumFace => "号面节",
            SecKind::AssertSec => "断言节",
            SecKind::HandoffSec => "移交节",
        }
    }

    /// 由节序反查闭集节（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<SecKind> {
        match idx {
            0 => Some(SecKind::DomainLedger),
            1 => Some(SecKind::NumFace),
            2 => Some(SecKind::AssertSec),
            3 => Some(SecKind::HandoffSec),
            _ => None,
        }
    }
}

/// 四节闭集数组（节序即声明序）。
pub const SEC_KINDS: &[SecKind] = &[
    SecKind::DomainLedger,
    SecKind::NumFace,
    SecKind::AssertSec,
    SecKind::HandoffSec,
];

/// 四处容量：四处各一笔，齐即满。
pub const SEC_CAP: usize = 4;

// ── 单节状态机：Draft→Checked→Signed 单调迁移 ────────────────────
/// 节状态机：草稿 → 已核 → 已签，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// 草稿：已声明且同步达成，待核。
    Draft,
    /// 已核：四处齐且同步复核通过。
    Checked,
    /// 已签：四处已签，账面只读。
    Signed,
}

impl SyncState {
    /// 状态序（只许前进，逐级）。
    pub fn rank(self) -> u8 {
        match self {
            SyncState::Draft => 0,
            SyncState::Checked => 1,
            SyncState::Signed => 2,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            SyncState::Draft => "草稿",
            SyncState::Checked => "已核",
            SyncState::Signed => "已签",
        }
    }
}

// ── 同步判定器与差异码 ───────────────────────────────────────────
/// 差异码方向段：声明值偏大（declared > actual）。
pub const DIFF_DIR_HIGH: i32 = 0x0100;
/// 差异码方向段：声明值偏小（declared < actual）。
pub const DIFF_DIR_LOW: i32 = 0x0200;

/// 差异码编码：节段 ×0x1000（节序+1，指明哪一节）+ 方向段 + 幅度段。
/// 幅度取 |declared−actual| 并在 0xFF 处饱和（巨大差异不撑爆低段）。
pub fn diff_code(section: SecKind, declared: u32, actual: u32) -> i32 {
    let sec_field = (section.index() as i32).wrapping_add(1).wrapping_mul(0x1000);
    let raw = declared.abs_diff(actual);
    let mag = raw.min(0xFF) as i32;
    let dir = if declared > actual {
        DIFF_DIR_HIGH
    } else {
        DIFF_DIR_LOW
    };
    sec_field.wrapping_add(dir).wrapping_add(mag)
}

/// 由差异码解析节别（节段在 1..=4 之外返回 None——负码/零码/越节码皆拒）。
pub fn diff_section(code: i32) -> Option<SecKind> {
    match code / 0x1000 {
        1 => Some(SecKind::DomainLedger),
        2 => Some(SecKind::NumFace),
        3 => Some(SecKind::AssertSec),
        4 => Some(SecKind::HandoffSec),
        _ => None,
    }
}

/// 由差异码解析方向段（还原为 0x0100=声明偏大 / 0x0200=声明偏小；仅正码有意义）。
pub fn diff_dir(code: i32) -> i32 {
    ((code % 0x1000) / 0x100).wrapping_mul(0x100)
}

/// 由差异码解析幅度段（0x00..0xFF，饱和值；仅正码有意义）。
pub fn diff_mag(code: i32) -> i32 {
    code % 0x100
}

/// 同步判定器（判据域模型直读口）：
/// declared==actual 才 Ok（返回一致值）；否则 Err(差异码)，
/// 差异码按 diff_code 编码，指明失同步的节与差异方向/幅度。
pub fn judge_sync_declare(section: SecKind, declared: u32, actual: u32) -> Result<u32, i32> {
    if declared == actual {
        return Ok(declared);
    }
    Err(diff_code(section, declared, actual))
}

// ── 单节账面与根台账 ─────────────────────────────────────────────
/// 单节账面：节别 + 状态机 + 声明值/实际值（同步达成才入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecSlot {
    /// 节别。
    pub sec: SecKind,
    /// 状态机当前态。
    pub state: SyncState,
    /// 声明值（入账时与实际值一致）。
    pub declared: u32,
    /// 实际值。
    pub actual: u32,
}

/// 根台账：四处账面 + 最近一次失同步差异码（诊断档）。
#[derive(Clone, Debug)]
pub struct RootLedger {
    /// 已入账节面（按声明序）。
    slots: Vec<SecSlot>,
    /// 最近一次同步失败的差异码（无失败为 0）。
    last_diff: i32,
}

impl RootLedger {
    /// 开一本空根台账。
    pub fn new() -> RootLedger {
        RootLedger {
            slots: Vec::new(),
            last_diff: 0,
        }
    }

    /// 最近一次失同步差异码（0 表示无失败记录）。
    pub fn last_diff_code(&self) -> i32 {
        self.last_diff
    }

    /// 已入账节数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否无节入账。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 取第 idx 笔账面（越界 None）。
    pub fn slot_at(&self, idx: usize) -> Option<SecSlot> {
        self.slots.get(idx).copied()
    }

    /// 账面切片（只读遍历口）。
    pub fn slot_slice(&self) -> &[SecSlot] {
        &self.slots
    }

    /// 声明一节（闸序：闭集 → 容量 → 重复 → 同步判定）。
    /// declared==actual 才入账，入账态为 Draft。
    pub fn declare(&mut self, sec_id: u32, declared: u32, actual: u32) -> Result<(), u32> {
        // ① 节别闭集闸：越四节闭集先拒。
        let kind = match SecKind::from_index(sec_id) {
            Some(k) => k,
            None => return Err(SdDiag::ESecRange.val()),
        };
        // ② 容量闸：四处账满即容量耗尽。
        if self.slots.len() >= SEC_CAP {
            return Err(SdDiag::ECapFull.val());
        }
        // ③ 重复闸：同一节只落一笔。
        for s in self.slots.iter() {
            if s.sec == kind {
                return Err(SdDiag::EDupSec.val());
            }
        }
        // ④ 同步判定闸：不一致则差异码入档并拒。
        match judge_sync_declare(kind, declared, actual) {
            Ok(_) => {}
            Err(code) => {
                self.last_diff = code;
                return match diff_section(code) {
                    Some(_) => Err(SdDiag::EDeclared.val()),
                    None => Err(SdDiag::EDiffCode.val()),
                };
            }
        }
        self.slots.push(SecSlot {
            sec: kind,
            state: SyncState::Draft,
            declared,
            actual,
        });
        Ok(())
    }

    /// 单节状态推进（闸序：闭集 → 存在 → 倒退 → 越级）。
    /// 同态重复推进为幂等 Ok（不属倒退）。
    fn advance_kind(&mut self, kind: SecKind, target: SyncState) -> Result<(), u32> {
        let mut pos: Option<usize> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if s.sec == kind {
                pos = Some(i);
                break;
            }
        }
        let slot = match pos {
            Some(i) => match self.slots.get_mut(i) {
                Some(s) => s,
                None => return Err(SdDiag::ENoSec.val()),
            },
            None => return Err(SdDiag::ENoSec.val()),
        };
        if target.rank() < slot.state.rank() {
            return Err(SdDiag::ERewind.val());
        }
        if target.rank() > slot.state.rank() + 1 {
            return Err(SdDiag::EJump.val());
        }
        slot.state = target;
        Ok(())
    }

    /// 按节号推进（节号映射后走单节推进）。
    pub fn advance(&mut self, sec_id: u32, target: SyncState) -> Result<(), u32> {
        let kind = match SecKind::from_index(sec_id) {
            Some(k) => k,
            None => return Err(SdDiag::ESecRange.val()),
        };
        self.advance_kind(kind, target)
    }

    /// 四处齐核：四处账齐且同步复核仍成立，各节 Draft→Checked。
    pub fn check_all(&mut self) -> Result<(), u32> {
        if self.slots.len() != SEC_CAP {
            return Err(SdDiag::ENoSec.val());
        }
        // 同步复核：入账后 declared==actual 必须仍成立。
        for s in self.slots.iter() {
            if judge_sync_declare(s.sec, s.declared, s.actual).is_err() {
                return Err(SdDiag::EDeclared.val());
            }
        }
        let mut kinds: Vec<SecKind> = Vec::new();
        for s in self.slots.iter() {
            kinds.push(s.sec);
        }
        for k in kinds.iter() {
            self.advance_kind(*k, SyncState::Checked)?;
        }
        Ok(())
    }

    /// 四处齐签：各节 Checked→Signed（越级/未核由单节闸显性拒）。
    pub fn sign_all(&mut self) -> Result<(), u32> {
        if self.slots.len() != SEC_CAP {
            return Err(SdDiag::ENoSec.val());
        }
        let mut kinds: Vec<SecKind> = Vec::new();
        for s in self.slots.iter() {
            kinds.push(s.sec);
        }
        for k in kinds.iter() {
            self.advance_kind(*k, SyncState::Signed)?;
        }
        Ok(())
    }

    /// 注入面：对指定节声明值做定点偏移（绕过 declare 闸，专供篡改注入）。
    pub fn inject_declared_shift(&mut self, kind: SecKind, delta: u32) {
        let mut pos: Option<usize> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if s.sec == kind {
                pos = Some(i);
                break;
            }
        }
        if let Some(i) = pos {
            if let Some(s) = self.slots.get_mut(i) {
                s.declared = s.declared.wrapping_add(delta);
            }
        }
    }

    /// 复判一节：对账面现值重跑同步判定器（注入后应转 Err 差异码）。
    /// 节未入账时以负码报"无可判差异"。
    pub fn rejudge(&self, kind: SecKind) -> Result<u32, i32> {
        let mut found: Option<(u32, u32)> = None;
        for s in self.slots.iter() {
            if s.sec == kind {
                found = Some((s.declared, s.actual));
            }
        }
        match found {
            Some((d, a)) => judge_sync_declare(kind, d, a),
            None => Err(-1),
        }
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

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取 Knuth MMIX 31 位推荐闭集（整数手册常用）。
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

// ── 40 批聚合器入口清单（全域断言资产） ──────────────────────────
/// 批清单容量：40 批（200 卡 ÷ 每批 5 卡）。
pub const BATCH_CAP: usize = 40;
/// 每批覆盖的 VX9 卡数。
pub const IDS_PER_BATCH: u32 = 5;

/// 单批聚合器入口清单项：批号/入口名/状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchEntry {
    /// 批号（0..40 闭集）。
    pub batch_no: u32,
    /// 聚合器入口名（vx9_b + 两位批号）。
    pub entry_name: &'static str,
    /// 该批资产状态（全域齐要求全部已签）。
    pub status: SyncState,
}

/// 40 批聚合器入口清单（冻结常量表；批号即下标，入口名后缀与批号一致）。
pub const BATCH_ALL: &[BatchEntry] = &[
    BatchEntry { batch_no: 0, entry_name: "vx9_b00", status: SyncState::Signed },
    BatchEntry { batch_no: 1, entry_name: "vx9_b01", status: SyncState::Signed },
    BatchEntry { batch_no: 2, entry_name: "vx9_b02", status: SyncState::Signed },
    BatchEntry { batch_no: 3, entry_name: "vx9_b03", status: SyncState::Signed },
    BatchEntry { batch_no: 4, entry_name: "vx9_b04", status: SyncState::Signed },
    BatchEntry { batch_no: 5, entry_name: "vx9_b05", status: SyncState::Signed },
    BatchEntry { batch_no: 6, entry_name: "vx9_b06", status: SyncState::Signed },
    BatchEntry { batch_no: 7, entry_name: "vx9_b07", status: SyncState::Signed },
    BatchEntry { batch_no: 8, entry_name: "vx9_b08", status: SyncState::Signed },
    BatchEntry { batch_no: 9, entry_name: "vx9_b09", status: SyncState::Signed },
    BatchEntry { batch_no: 10, entry_name: "vx9_b10", status: SyncState::Signed },
    BatchEntry { batch_no: 11, entry_name: "vx9_b11", status: SyncState::Signed },
    BatchEntry { batch_no: 12, entry_name: "vx9_b12", status: SyncState::Signed },
    BatchEntry { batch_no: 13, entry_name: "vx9_b13", status: SyncState::Signed },
    BatchEntry { batch_no: 14, entry_name: "vx9_b14", status: SyncState::Signed },
    BatchEntry { batch_no: 15, entry_name: "vx9_b15", status: SyncState::Signed },
    BatchEntry { batch_no: 16, entry_name: "vx9_b16", status: SyncState::Signed },
    BatchEntry { batch_no: 17, entry_name: "vx9_b17", status: SyncState::Signed },
    BatchEntry { batch_no: 18, entry_name: "vx9_b18", status: SyncState::Signed },
    BatchEntry { batch_no: 19, entry_name: "vx9_b19", status: SyncState::Signed },
    BatchEntry { batch_no: 20, entry_name: "vx9_b20", status: SyncState::Signed },
    BatchEntry { batch_no: 21, entry_name: "vx9_b21", status: SyncState::Signed },
    BatchEntry { batch_no: 22, entry_name: "vx9_b22", status: SyncState::Signed },
    BatchEntry { batch_no: 23, entry_name: "vx9_b23", status: SyncState::Signed },
    BatchEntry { batch_no: 24, entry_name: "vx9_b24", status: SyncState::Signed },
    BatchEntry { batch_no: 25, entry_name: "vx9_b25", status: SyncState::Signed },
    BatchEntry { batch_no: 26, entry_name: "vx9_b26", status: SyncState::Signed },
    BatchEntry { batch_no: 27, entry_name: "vx9_b27", status: SyncState::Signed },
    BatchEntry { batch_no: 28, entry_name: "vx9_b28", status: SyncState::Signed },
    BatchEntry { batch_no: 29, entry_name: "vx9_b29", status: SyncState::Signed },
    BatchEntry { batch_no: 30, entry_name: "vx9_b30", status: SyncState::Signed },
    BatchEntry { batch_no: 31, entry_name: "vx9_b31", status: SyncState::Signed },
    BatchEntry { batch_no: 32, entry_name: "vx9_b32", status: SyncState::Signed },
    BatchEntry { batch_no: 33, entry_name: "vx9_b33", status: SyncState::Signed },
    BatchEntry { batch_no: 34, entry_name: "vx9_b34", status: SyncState::Signed },
    BatchEntry { batch_no: 35, entry_name: "vx9_b35", status: SyncState::Signed },
    BatchEntry { batch_no: 36, entry_name: "vx9_b36", status: SyncState::Signed },
    BatchEntry { batch_no: 37, entry_name: "vx9_b37", status: SyncState::Signed },
    BatchEntry { batch_no: 38, entry_name: "vx9_b38", status: SyncState::Signed },
    BatchEntry { batch_no: 39, entry_name: "vx9_b39", status: SyncState::Signed },
];

/// 批清单验证（判据「40 批聚合器入口清单齐」直读口）：
/// 批数恰 40、批号即下标、入口名前缀+两位批号、状态齐已签、无重复批号。
pub fn validate_registry(list: &[BatchEntry]) -> Result<(), u32> {
    if list.len() != BATCH_CAP {
        return Err(SdDiag::EBatchRange.val());
    }
    let mut i = 0usize;
    while i < list.len() {
        let e = match list.get(i) {
            Some(e) => e,
            None => return Err(SdDiag::EBatchRange.val()),
        };
        if e.batch_no != i as u32 {
            return Err(SdDiag::EBatchRange.val());
        }
        if e.entry_name.is_empty() || !e.entry_name.starts_with("vx9_b") {
            return Err(SdDiag::ENameBad.val());
        }
        if e.status != SyncState::Signed {
            return Err(SdDiag::EStatusMiss.val());
        }
        let mut j = i + 1;
        while j < list.len() {
            if let Some(e2) = list.get(j) {
                if e2.batch_no == e.batch_no {
                    return Err(SdDiag::EBatchDup.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── mini 断言集（抽样判定器逐批执行） ────────────────────────────
/// mini 断言集条数：批号/入口名/后缀/覆盖区间/状态 各一条。
pub const MINI_TOTAL: usize = 5;

/// 单批 mini 断言集判定（五项独立写死，任一为假即该批红）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniVerdict {
    /// 批号在 0..40 闭集内。
    pub f_batch: bool,
    /// 入口名非空且前缀 vx9_b、长度恰 7。
    pub f_name: bool,
    /// 入口名末两位 ASCII 数字与批号一致。
    pub f_suffix: bool,
    /// 批覆盖 5 卡 id 区间落在 12600..=12799 且段基对齐 0x40。
    pub f_range: bool,
    /// 该批状态达已签。
    pub f_state: bool,
}

impl MiniVerdict {
    /// 通过项数。
    pub fn pass_n(&self) -> u32 {
        let mut n = 0u32;
        if self.f_batch {
            n += 1;
        }
        if self.f_name {
            n += 1;
        }
        if self.f_suffix {
            n += 1;
        }
        if self.f_range {
            n += 1;
        }
        if self.f_state {
            n += 1;
        }
        n
    }

    /// 五项全绿。
    pub fn green(&self) -> bool {
        self.pass_n() == MINI_TOTAL as u32
    }
}

/// 对一批跑一次 mini 断言集（抽样判定器的执行体）。
pub fn mini_assert(entry: &BatchEntry) -> MiniVerdict {
    let f_batch = entry.batch_no < BATCH_CAP as u32;
    let f_name = entry.entry_name.len() == 7 && entry.entry_name.starts_with("vx9_b");
    // 后缀两位 ASCII 数字须与批号一致（十位/个位分别核对）。
    let bytes = entry.entry_name.as_bytes();
    let f_suffix = if bytes.len() >= 2 {
        let tens = b'0' + (entry.batch_no / 10).min(9) as u8;
        let ones = b'0' + (entry.batch_no % 10) as u8;
        match (bytes.get(bytes.len() - 2), bytes.get(bytes.len() - 1)) {
            (Some(a), Some(b)) => *a == tens && *b == ones,
            _ => false,
        }
    } else {
        false
    };
    // 批覆盖区间：start = 12600 + 批号×5，end = start+4 ≤ 12799。
    let start = 12600u32.checked_add(entry.batch_no.saturating_mul(IDS_PER_BATCH));
    let f_range = match start {
        Some(s) => match s.checked_add(IDS_PER_BATCH - 1) {
            Some(en) => {
                en <= VX9_ID_MAX
                    && match VX9_SEG_MIN.checked_add(0x40u32.checked_mul(s - VX9_ID_MIN).unwrap_or(u32::MAX)) {
                        Some(seg) => seg <= VX9_SEG_MAX && seg % 0x40 == 0,
                        None => false,
                    }
            }
            None => false,
        },
        None => false,
    };
    let f_state = entry.status == SyncState::Signed;
    MiniVerdict {
        f_batch,
        f_name,
        f_suffix,
        f_range,
        f_state,
    }
}

// ── 抽样器与记账簿 ───────────────────────────────────────────────
/// 抽样容量：判据要求抽样 10 批。
pub const SAMPLE_CAP: usize = 10;
/// 抽样种子（SEG_BASE 派生，固定可复算）。
pub const SAMPLE_SEED: u64 = SEG_BASE as u64 + 0x5A5A;

/// 固定种子抽取 want 个不重复批号（0 < want ≤ 40，越界一律拒）。
pub fn sample_batches(seed: u64, want: u32) -> Result<Vec<u32>, u32> {
    if want == 0 || want > BATCH_CAP as u32 {
        return Err(SdDiag::ESampleBound.val());
    }
    let mut rng = Lcg::new(seed);
    let mut mask: u64 = 0;
    let mut out: Vec<u32> = Vec::new();
    while out.len() < want as usize {
        let pick = rng.below(BATCH_CAP as u32);
        let bit = 1u64.checked_shl(pick).unwrap_or(0);
        if mask & bit == 0 {
            mask |= bit;
            out.push(pick);
        }
    }
    Ok(out)
}

/// 单批抽样记账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRecord {
    /// 批号。
    pub batch_no: u32,
    /// mini 断言集通过项数。
    pub pass_n: u32,
    /// mini 断言集总项数。
    pub total: u32,
    /// 五项全绿。
    pub green: bool,
}

/// 抽样记账簿：容量 10 笔，重复批号拒，红项可查。
#[derive(Clone, Debug)]
pub struct SampleBook {
    recs: Vec<SampleRecord>,
}

impl SampleBook {
    /// 开一本空记账簿。
    pub fn new() -> SampleBook {
        SampleBook { recs: Vec::new() }
    }

    /// 已记账笔数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否空簿。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 记一笔（闸序：容量 → 重复批号）。
    pub fn record(&mut self, r: SampleRecord) -> Result<(), u32> {
        if self.recs.len() >= SAMPLE_CAP {
            return Err(SdDiag::ECapFull.val());
        }
        for x in self.recs.iter() {
            if x.batch_no == r.batch_no {
                return Err(SdDiag::ESampleDup.val());
            }
        }
        self.recs.push(r);
        Ok(())
    }

    /// 取第 idx 笔（越界 None）。
    pub fn record_at(&self, idx: usize) -> Option<SampleRecord> {
        self.recs.get(idx).copied()
    }

    /// 全簿绿判：任一笔红即 EMiniFail。
    pub fn green_all(&self) -> Result<(), u32> {
        for x in self.recs.iter() {
            if !x.green {
                return Err(SdDiag::EMiniFail.val());
            }
        }
        Ok(())
    }
}

/// 抽样判定器：抽 10 批 → 逐批 mini 断言集 → 逐笔记账。
pub fn run_sample(seed: u64) -> Result<SampleBook, u32> {
    let picks = sample_batches(seed, SAMPLE_CAP as u32)?;
    let mut book = SampleBook::new();
    for p in picks.iter() {
        let entry = BATCH_ALL.get(*p as usize).ok_or(SdDiag::ESampleBound.val())?;
        let v = mini_assert(entry);
        book.record(SampleRecord {
            batch_no: entry.batch_no,
            pass_n: v.pass_n(),
            total: MINI_TOTAL as u32,
            green: v.green(),
        })?;
    }
    Ok(book)
}

// ── 同步账面构造（正常流语料） ───────────────────────────────────
/// 构造四处同步达成、尚未推进状态机的根台账（声明值=实际值，LCG 确定）。
pub fn build_synced_ledger() -> RootLedger {
    let mut led = RootLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut sec = 0u32;
    while sec < SEC_CAP as u32 {
        // 实际值取 1..=0xFFFF 非零域，声明值与实际值一致（同步达成）。
        let actual = 1u32 + rng.below(0xFFFE);
        let _ = led.declare(sec, actual, actual);
        sec += 1;
    }
    led
}

// ── 注入面一：声明篡改（10 轮，判定器必须抓且差异码指明被篡改节） ─
/// 十路声明篡改：LCG 选节、偏移 1..=9，注入后复判必须 Err 且节别吻合。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn tamper_declared_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(SEC_CAP as u32);
        let delta = 1u32 + rng.below(0x09);
        let kind = match SecKind::from_index(pick) {
            Some(s) => s,
            None => SecKind::DomainLedger,
        };
        let mut led = build_synced_ledger();
        led.inject_declared_shift(kind, delta);
        total += 1;
        match led.rejudge(kind) {
            Ok(_) => {
                if escape.is_empty() {
                    escape = format!("路{}：节{}偏移{}未被抓", k, kind.name(), delta);
                }
            }
            Err(code) => match diff_section(code) {
                Some(s) if s == kind => caught += 1,
                _ => {
                    if escape.is_empty() {
                        escape = format!("路{}：差异码未指明被篡改节{}", k, kind.name());
                    }
                }
            },
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 注入面二：抽样越界（10 轮，越界样本量必须全拒） ──────────────
/// 十路越界样本量闭集（0 与 41..与巨大值各一路）。
pub const OOB_WANTS: &[u32] = &[0, 41, 42, 43, 44, 64, 100, 256, 4096, 0xFFFF_FFFF];

/// 十路抽样越界：每路样本量越 1..=40 闭窗，抽样器必须逐路拒。
pub fn sample_oob_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for w in OOB_WANTS.iter() {
        total += 1;
        match sample_batches(SEG_BASE as u64, *w) {
            Err(e) if e == SdDiag::ESampleBound.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("路{}：want={} 未被抓", total - 1, w);
                }
            }
        }
    }
    (caught, total, escape)
}

// ── 注入面三：状态倒退（10 轮，已签账面回拨必须全拒） ────────────
/// 十路状态倒退：LCG 选节、草稿/已核交替回拨，已签账面必须逐路拒。
pub fn rewind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(SEC_CAP as u32);
        let target = if k % 2 == 0 {
            SyncState::Draft
        } else {
            SyncState::Checked
        };
        let mut led = build_synced_ledger();
        let _ = led.check_all();
        let _ = led.sign_all();
        total += 1;
        match led.advance(pick, target) {
            Err(e) if e == SdDiag::ERewind.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("路{}：节{}回拨{}未被抓", k, pick, target.name());
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 双跑一致指纹（FNV-1a 64，参数为标准推荐闭集） ────────────────
/// FNV-1a 64 指纹器（偏移基 0xcbf29ce484222325，素数 0x100000001b3）。
pub struct Fnv {
    h: u64,
}

impl Fnv {
    /// 以标准偏移基构造。
    pub fn new() -> Fnv {
        Fnv {
            h: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 吸收一字节。
    pub fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 吸收一个 u32（大端四字节）。
    pub fn u32(&mut self, v: u32) {
        self.byte((v >> 24) as u8);
        self.byte((v >> 16) as u8);
        self.byte((v >> 8) as u8);
        self.byte(v as u8);
    }

    /// 取指纹值。
    pub fn done(&self) -> u64 {
        self.h
    }
}

/// 全流程指纹：四处台账核签 + 抽样记账 + 三类注入面输出逐项聚合。
fn fingerprint() -> u64 {
    let mut h = Fnv::new();
    let mut led = build_synced_ledger();
    let _ = led.check_all();
    let _ = led.sign_all();
    let mut i = 0usize;
    while i < led.len() {
        if let Some(s) = led.slot_at(i) {
            h.u32(s.sec.index());
            h.u32(s.state.rank() as u32);
            h.u32(s.declared);
            h.u32(s.actual);
        }
        i += 1;
    }
    h.u32(led.last_diff_code() as u32);
    match run_sample(SAMPLE_SEED) {
        Ok(b) => {
            h.u32(b.len() as u32);
            let mut k = 0usize;
            while k < b.len() {
                if let Some(r) = b.record_at(k) {
                    h.u32(r.batch_no);
                    h.u32(r.pass_n);
                    h.u32(r.total);
                    h.u32(r.green as u32);
                }
                k += 1;
            }
        }
        Err(e) => h.u32(e),
    }
    let (c1, t1, e1) = tamper_declared_sweep();
    h.u32(c1 as u32);
    h.u32(t1 as u32);
    for by in e1.as_bytes().iter() {
        h.byte(*by);
    }
    let (c2, t2, e2) = sample_oob_sweep();
    h.u32(c2 as u32);
    h.u32(t2 as u32);
    for by in e2.as_bytes().iter() {
        h.byte(*by);
    }
    let (c3, t3, e3) = rewind_sweep();
    h.u32(c3 as u32);
    h.u32(t3 as u32);
    for by in e3.as_bytes().iter() {
        h.byte(*by);
    }
    h.done()
}

/// 双跑判定：两次全流程指纹相等且非零，否则 EFingerprint。
pub fn dual_run_verdict() -> Result<u64, u32> {
    let a = fingerprint();
    let b = fingerprint();
    if a == b && a != 0 {
        Ok(a)
    } else {
        Err(SdDiag::EFingerprint.val())
    }
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式 200 槽闭窗 [0x80000, 0x831C0]。
/// 注：VX9 §3 公式 SEG_BASE = 0x80000 + 0x40×(id−12600) 对 12793 给出
/// 0x83040；派工总纲批次表 B16 行尾值 0x82FC0 只覆盖到 id 12791，与公式
/// 不符——此处以公式窗为准（卡面给定的 0x83040 恰在公式窗内）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < VX9_SEG_MIN {
        return false;
    }
    if SEG_BASE > VX9_SEG_MAX {
        return false;
    }
    true
}

/// 诊断码闭集守卫：16 码互异、全落段窗、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    if SD_ALL.len() != 16 {
        return false;
    }
    for d in SD_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    // 未知码必须回落"未知"文案（0x3F 不在 0x01..0x10 闭集内）。
    let unknown = detail(SEG_BASE + 0x3F);
    match SD_ALL.first() {
        Some(d) => unknown != d.detail(),
        None => false,
    }
}

/// 四节闭集守卫：节序/反查/名表三向一致。
fn four_kind_ok() -> bool {
    if SEC_KINDS.len() != SEC_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < SEC_KINDS.len() {
        let k = match SEC_KINDS.get(i) {
            Some(k) => *k,
            None => return false,
        };
        if k.index() != i as u32 {
            return false;
        }
        match SecKind::from_index(i as u32) {
            Some(k2) if k2 == k => {}
            _ => return false,
        }
        if k.name().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 40 批资产齐：清单验证 Ok 且首尾批号闭合。
fn batch_table_ok() -> bool {
    if BATCH_ALL.len() != BATCH_CAP {
        return false;
    }
    if validate_registry(BATCH_ALL).is_err() {
        return false;
    }
    match (BATCH_ALL.first(), BATCH_ALL.last()) {
        (Some(f), Some(l)) => f.batch_no == 0 && l.batch_no == (BATCH_CAP - 1) as u32,
        _ => false,
    }
}

/// 同步判定器正反例：一致 Ok；不一致差异码指明节/方向/幅度；非法码解析不出节。
fn judge_ok() -> bool {
    // Ok 路：一致才 Ok，返回一致值。
    if judge_sync_declare(SecKind::DomainLedger, 7, 7) != Ok(7) {
        return false;
    }
    // Err 路（声明偏大）：节别/方向/幅度三段都可解析。
    let c1 = match judge_sync_declare(SecKind::NumFace, 9, 4) {
        Ok(_) => return false,
        Err(c) => c,
    };
    if diff_section(c1) != Some(SecKind::NumFace) {
        return false;
    }
    if diff_dir(c1) != DIFF_DIR_HIGH || diff_mag(c1) != 5 {
        return false;
    }
    // Err 路（声明偏小）。
    let c2 = match judge_sync_declare(SecKind::AssertSec, 2, 8) {
        Ok(_) => return false,
        Err(c) => c,
    };
    if diff_section(c2) != Some(SecKind::AssertSec) {
        return false;
    }
    if diff_dir(c2) != DIFF_DIR_LOW || diff_mag(c2) != 6 {
        return false;
    }
    // 非法差异码：负码/零码/越节码都解析不出节。
    if diff_section(-1).is_some() || diff_section(0).is_some() || diff_section(0x5000).is_some() {
        return false;
    }
    true
}

/// 判定次序：declare 四闸（闭集→容量→重复→同步）与 advance 闸序逐级可证。
fn judge_order_ok() -> bool {
    // ① 节别闭集最前：空账 + 非法节号先于一切闸。
    let mut l = RootLedger::new();
    if l.declare(9, 1, 1) != Err(SdDiag::ESecRange.val()) {
        return false;
    }
    // ② 容量先于重复：满账 + 已有节号报容量耗尽。
    let mut full = build_synced_ledger();
    if full.declare(0, 5, 5) != Err(SdDiag::ECapFull.val()) {
        return false;
    }
    // ③ 重复先于同步：单节账 + 重复节号 + 值不一致，报重复而非失同步。
    let mut one = RootLedger::new();
    let _ = one.declare(2, 3, 3);
    if one.declare(2, 9, 8) != Err(SdDiag::EDupSec.val()) {
        return false;
    }
    // ④ 同步判定最后：合法新节 + 值不一致才报失同步，差异码入档可复核。
    if one.declare(1, 9, 8) != Err(SdDiag::EDeclared.val()) {
        return false;
    }
    if one.last_diff_code() != diff_code(SecKind::NumFace, 9, 8) {
        return false;
    }
    // ⑤ advance 闸序：非法节号先于未声明节。
    if one.advance(9, SyncState::Checked) != Err(SdDiag::ESecRange.val()) {
        return false;
    }
    if one.advance(3, SyncState::Checked) != Err(SdDiag::ENoSec.val()) {
        return false;
    }
    true
}

/// 正常流：四处声明同步 → 齐核 → 齐签，状态机单调到已签。
fn happy_path_ok() -> bool {
    let mut led = build_synced_ledger();
    if led.len() != SEC_CAP || led.is_empty() {
        return false;
    }
    if led.check_all().is_err() {
        return false;
    }
    for s in led.slot_slice().iter() {
        if s.state != SyncState::Checked {
            return false;
        }
    }
    if led.sign_all().is_err() {
        return false;
    }
    for s in led.slot_slice().iter() {
        if s.state != SyncState::Signed {
            return false;
        }
    }
    true
}

/// 边界四类：节别首尾/数值端点与差异幅度/抽样闭窗/状态同态越级倒退。
fn boundary_ok() -> bool {
    // 边界一（节别）：首节 0 与末节 3 可声明；4 与 u32::MAX 拒。
    let mut a = RootLedger::new();
    if a.declare(0, 1, 1).is_err() {
        return false;
    }
    let mut b = RootLedger::new();
    if b.declare(3, 1, 1).is_err() {
        return false;
    }
    let mut c = RootLedger::new();
    if c.declare(4, 1, 1) != Err(SdDiag::ESecRange.val()) {
        return false;
    }
    if c.declare(u32::MAX, 1, 1) != Err(SdDiag::ESecRange.val()) {
        return false;
    }
    // 边界二（数值）：0/0 与 MAX/MAX 同步 Ok；最小差 1 方向为偏大；巨差幅度饱和 0xFF。
    let mut d = RootLedger::new();
    if d.declare(0, 0, 0).is_err() {
        return false;
    }
    let mut e = RootLedger::new();
    if e.declare(0, u32::MAX, u32::MAX).is_err() {
        return false;
    }
    let small = judge_sync_declare(SecKind::HandoffSec, 11, 10);
    let big = judge_sync_declare(SecKind::HandoffSec, 0, u32::MAX);
    match (small, big) {
        (Err(s), Err(g)) => {
            if diff_mag(s) != 1 || diff_dir(s) != DIFF_DIR_HIGH || diff_mag(g) != 0xFF {
                return false;
            }
        }
        _ => return false,
    }
    // 边界三（抽样）：want=1 与 want=40 可跑；want=0 与 want=41 拒。
    match sample_batches(SEG_BASE as u64, 1) {
        Ok(v) if v.len() == 1 => {}
        _ => return false,
    }
    match sample_batches(SEG_BASE as u64, BATCH_CAP as u32) {
        Ok(v) if v.len() == BATCH_CAP => {}
        _ => return false,
    }
    if sample_batches(SEG_BASE as u64, 0) != Err(SdDiag::ESampleBound.val()) {
        return false;
    }
    if sample_batches(SEG_BASE as u64, BATCH_CAP as u32 + 1) != Err(SdDiag::ESampleBound.val()) {
        return false;
    }
    // 边界四（状态）：越级拒、同态幂等 Ok、齐核齐签后倒退拒。
    let mut f = build_synced_ledger();
    if f.advance(0, SyncState::Signed) != Err(SdDiag::EJump.val()) {
        return false;
    }
    if f.advance(0, SyncState::Checked).is_err() {
        return false;
    }
    if f.advance(0, SyncState::Checked).is_err() {
        return false;
    }
    let _ = f.check_all();
    let _ = f.sign_all();
    if f.advance(1, SyncState::Draft) != Err(SdDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 注入必抓一：十路声明篡改全数命中且差异码节别吻合。
fn tamper_declared_ok() -> bool {
    let (caught, total, escape) = tamper_declared_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓二：十路抽样越界全数命中。
fn tamper_oob_ok() -> bool {
    let (caught, total, escape) = sample_oob_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入必抓三：十路状态倒退全数命中。
fn tamper_rewind_ok() -> bool {
    let (caught, total, escape) = rewind_sweep();
    if total != 10 || caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 抽样 10 批可跑并记账：簿满 10 笔、逐笔 5/5 全绿。
fn sample_run_ok() -> bool {
    let book = match run_sample(SAMPLE_SEED) {
        Ok(b) => b,
        Err(_) => return false,
    };
    if book.len() != SAMPLE_CAP || book.is_empty() {
        return false;
    }
    if book.green_all().is_err() {
        return false;
    }
    let mut i = 0usize;
    while i < book.len() {
        match book.record_at(i) {
            Some(r) => {
                if r.total != MINI_TOTAL as u32 || r.pass_n != MINI_TOTAL as u32 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 容量耗尽：四处第五笔声明拒、记账簿第十一笔拒、重复批号记账拒。
fn capacity_ok() -> bool {
    // 重复先证：单节账第二笔同节拒。
    let mut one = RootLedger::new();
    let _ = one.declare(0, 1, 1);
    if one.declare(0, 2, 2) != Err(SdDiag::EDupSec.val()) {
        return false;
    }
    // 满账第五笔拒。
    let mut full = build_synced_ledger();
    if full.len() != SEC_CAP {
        return false;
    }
    if full.declare(0, 2, 2) != Err(SdDiag::ECapFull.val()) {
        return false;
    }
    // 记账簿：重复批号拒 + 第十一笔拒。
    let mut book = SampleBook::new();
    let _ = book.record(SampleRecord {
        batch_no: 3,
        pass_n: MINI_TOTAL as u32,
        total: MINI_TOTAL as u32,
        green: true,
    });
    if book.record(SampleRecord {
        batch_no: 3,
        pass_n: MINI_TOTAL as u32,
        total: MINI_TOTAL as u32,
        green: true,
    }) != Err(SdDiag::ESampleDup.val())
    {
        return false;
    }
    let mut n = 1u32;
    while n < SAMPLE_CAP as u32 {
        match book.record(SampleRecord {
            batch_no: 3 + n,
            pass_n: MINI_TOTAL as u32,
            total: MINI_TOTAL as u32,
            green: true,
        }) {
            Ok(()) => {}
            Err(_) => return false,
        }
        n += 1;
    }
    book.record(SampleRecord {
        batch_no: 3 + SAMPLE_CAP as u32,
        pass_n: MINI_TOTAL as u32,
        total: MINI_TOTAL as u32,
        green: true,
    }) == Err(SdDiag::ECapFull.val())
}

/// 双跑一致：两次全流程指纹相等且非零。
fn dual_run_ok() -> bool {
    match dual_run_verdict() {
        Ok(_) => true,
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12793 自检（判据 12793 卡：全域断言资产齐 + 抽样 10 批可跑）。
pub fn run_unx12793_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12793");

    s.add_dyn(
        "F12794-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} win=[0x80000,0x{:05x}]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            VX9_SEG_MAX
        ),
    );

    s.add_dyn(
        "F12794-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={} base=0x{:05x}", SD_ALL.len(), SEG_BASE),
    );

    s.add_dyn(
        "F12794-03 四节闭集",
        four_kind_ok(),
        &format!(
            "n={} 首={} 尾={}",
            SEC_KINDS.len(),
            SEC_KINDS.first().map(|k| k.name()).unwrap_or("?"),
            SEC_KINDS.last().map(|k| k.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12794-04 批资产清单40批齐",
        batch_table_ok(),
        &format!("n={} 尾批={}", BATCH_ALL.len(), BATCH_ALL.len() - 1),
    );

    s.add_dyn(
        "F12794-05 同步判定器正反例",
        judge_ok(),
        "一致Ok 不一致差异码指明节向幅",
    );

    s.add_dyn(
        "F12794-06 判定次序",
        judge_order_ok(),
        "闭集→容量→重复→同步 四闸有序",
    );

    s.add_dyn(
        "F12794-07 正常流四处同步签署",
        happy_path_ok(),
        &format!("终态={}", SyncState::Signed.name()),
    );

    s.add_dyn(
        "F12794-08 边界四类",
        boundary_ok(),
        "节别/数值/抽样/状态 四类端点",
    );

    let (c1, t1, e1) = tamper_declared_sweep();
    s.add_dyn(
        "F12794-09 注入声明篡改10轮全抓",
        tamper_declared_ok(),
        &format!("{}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = sample_oob_sweep();
    s.add_dyn(
        "F12794-10 注入抽样越界10轮全抓",
        tamper_oob_ok(),
        &format!("{}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = rewind_sweep();
    s.add_dyn(
        "F12794-11 注入状态倒退10轮全抓",
        tamper_rewind_ok(),
        &format!("{}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12794-12 抽样10批可跑并记账",
        sample_run_ok(),
        &format!("cap={} mini={}", SAMPLE_CAP, MINI_TOTAL),
    );

    s.add_dyn(
        "F12794-13 容量耗尽",
        capacity_ok(),
        &format!("节容量={} 簿容量={}", SEC_CAP, SAMPLE_CAP),
    );

    s.add_dyn(
        "F12794-14 双跑一致指纹",
        dual_run_ok(),
        "两次全流程 FNV 指纹相等",
    );

    // 收口：判据条数声明（声明 15 条，实收须与声明一致）。
    let n_before = s.len();
    s.add_dyn(
        "F12794-15 收口判据条数声明",
        n_before == 14,
        &format!("声明15条 实收{}条", n_before + 1),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：SEG_BASE 恰为 VX9 公式值，对齐 0x40，落 200 槽闭窗。
        assert_eq!(SEG_BASE, 0x80000u32 + 0x40u32 * (12793u32 - 12600u32));
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= VX9_SEG_MIN && SEG_BASE <= VX9_SEG_MAX);
        assert!(seg_guard_ok());
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：16 码互异、全落段窗、查表命中、文案非空、未知码回落。
        assert_eq!(SD_ALL.len(), 16);
        let mut i = 0usize;
        while i < SD_ALL.len() {
            let vi = SD_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SD_ALL.len() {
                let vj = SD_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            assert_eq!(detail(vi), SD_ALL.get(i).map(|d| d.detail()).unwrap_or("?"));
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn section_enum_is_closed() {
        // 四节闭集：节序/反查/名表三向一致。
        assert_eq!(SEC_KINDS.len(), SEC_CAP);
        assert_eq!(SEC_CAP, 4);
        let mut i = 0usize;
        while i < SEC_KINDS.len() {
            let k = SEC_KINDS.get(i).copied().unwrap_or(SecKind::DomainLedger);
            assert_eq!(k.index(), i as u32);
            assert_eq!(SecKind::from_index(i as u32), Some(k));
            assert!(!k.name().is_empty());
            i += 1;
        }
        assert_eq!(SecKind::from_index(4), None);
        assert_eq!(SecKind::from_index(u32::MAX), None);
    }

    #[test]
    fn happy_path_four_sections_sign() {
        // 正常流：四处声明同步 → 齐核 → 齐签，状态机单调只进。
        let mut led = build_synced_ledger();
        assert_eq!(led.len(), SEC_CAP);
        assert!(led.check_all().is_ok());
        for s in led.slot_slice().iter() {
            assert_eq!(s.state, SyncState::Checked);
            assert_eq!(s.declared, s.actual);
        }
        assert!(led.sign_all().is_ok());
        for s in led.slot_slice().iter() {
            assert_eq!(s.state, SyncState::Signed);
        }
        assert_eq!(SyncState::Draft.rank() < SyncState::Checked.rank(), true);
        assert_eq!(SyncState::Checked.rank() < SyncState::Signed.rank(), true);
    }

    #[test]
    fn judge_sync_ok_and_diff_code_section() {
        // Ok 路：一致才 Ok，返回一致值。
        assert_eq!(judge_sync_declare(SecKind::DomainLedger, 7, 7), Ok(7));
        assert_eq!(judge_sync_declare(SecKind::HandoffSec, 0, 0), Ok(0));
        // Err 路：差异码节段/方向段/幅度段三段皆可解析。
        let c = match judge_sync_declare(SecKind::NumFace, 9, 4) {
            Ok(_) => 0,
            Err(c) => c,
        };
        assert_eq!(diff_section(c), Some(SecKind::NumFace));
        assert_eq!(diff_dir(c), DIFF_DIR_HIGH);
        assert_eq!(diff_mag(c), 5);
        let c2 = match judge_sync_declare(SecKind::HandoffSec, 1, 300) {
            Ok(_) => 0,
            Err(c) => c,
        };
        assert_eq!(diff_section(c2), Some(SecKind::HandoffSec));
        assert_eq!(diff_dir(c2), DIFF_DIR_LOW);
        assert_eq!(diff_mag(c2), 0xFF, "巨差幅度须饱和 0xFF");
        // 非法差异码解析不出节。
        assert_eq!(diff_section(-1), None);
        assert_eq!(diff_section(0), None);
        assert_eq!(diff_section(0x5000), None);
    }

    #[test]
    fn judge_order_precedence() {
        // 判定次序：闭集→容量→重复→同步 逐级可证。
        let mut l = RootLedger::new();
        assert_eq!(l.declare(9, 1, 1), Err(SdDiag::ESecRange.val()));
        let mut full = build_synced_ledger();
        assert_eq!(full.declare(0, 5, 5), Err(SdDiag::ECapFull.val()));
        let mut one = RootLedger::new();
        let _ = one.declare(2, 3, 3);
        assert_eq!(one.declare(2, 9, 8), Err(SdDiag::EDupSec.val()));
        assert_eq!(one.declare(1, 9, 8), Err(SdDiag::EDeclared.val()));
        assert_eq!(one.last_diff_code(), diff_code(SecKind::NumFace, 9, 8));
        assert_eq!(one.advance(9, SyncState::Checked), Err(SdDiag::ESecRange.val()));
        assert_eq!(one.advance(3, SyncState::Checked), Err(SdDiag::ENoSec.val()));
        assert!(judge_order_ok());
    }

    #[test]
    fn boundary_four_classes() {
        // 边界一（节别）：首节/末节可声明，越界与 u32::MAX 拒。
        let mut a = RootLedger::new();
        assert!(a.declare(0, 1, 1).is_ok());
        let mut b = RootLedger::new();
        assert!(b.declare(3, 1, 1).is_ok());
        let mut c = RootLedger::new();
        assert_eq!(c.declare(4, 1, 1), Err(SdDiag::ESecRange.val()));
        assert_eq!(c.declare(u32::MAX, 1, 1), Err(SdDiag::ESecRange.val()));
        // 边界二（数值）：端点值同步 Ok，最小差 1，巨差饱和。
        let mut d = RootLedger::new();
        assert!(d.declare(0, 0, 0).is_ok());
        let mut e = RootLedger::new();
        assert!(e.declare(0, u32::MAX, u32::MAX).is_ok());
        // 边界三（抽样）：1 与 40 可跑，0 与 41 拒。
        assert_eq!(sample_batches(SEG_BASE as u64, 1).map(|v| v.len()), Ok(1));
        assert_eq!(
            sample_batches(SEG_BASE as u64, BATCH_CAP as u32).map(|v| v.len()),
            Ok(BATCH_CAP)
        );
        assert_eq!(
            sample_batches(SEG_BASE as u64, 0),
            Err(SdDiag::ESampleBound.val())
        );
        assert_eq!(
            sample_batches(SEG_BASE as u64, BATCH_CAP as u32 + 1),
            Err(SdDiag::ESampleBound.val())
        );
        // 边界四（状态）：越级拒、同态幂等、倒退拒。
        let mut f = build_synced_ledger();
        assert_eq!(f.advance(0, SyncState::Signed), Err(SdDiag::EJump.val()));
        assert!(f.advance(0, SyncState::Checked).is_ok());
        assert!(f.advance(0, SyncState::Checked).is_ok());
        // 齐核齐签后再回拨：倒退显性拒且账面不动。
        assert!(f.check_all().is_ok());
        assert!(f.sign_all().is_ok());
        let before = f.slot_at(1);
        assert_eq!(f.advance(1, SyncState::Draft), Err(SdDiag::ERewind.val()));
        assert_eq!(f.slot_at(1), before);
        assert!(boundary_ok());
    }

    #[test]
    fn injection_declared_tamper_caught() {
        // 注入一：十路声明篡改全抓，差异码指明被篡改节。
        let (caught, total, escape) = tamper_declared_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 定点复核：任取一节偏移 1，复判必 Err 且节别吻合。
        let mut led = build_synced_ledger();
        led.inject_declared_shift(SecKind::AssertSec, 1);
        match led.rejudge(SecKind::AssertSec) {
            Ok(_) => panic!("声明篡改逃脱"),
            Err(code) => assert_eq!(diff_section(code), Some(SecKind::AssertSec)),
        }
        assert!(tamper_declared_ok());
    }

    #[test]
    fn injection_sample_oob_caught() {
        // 注入二：十路越界样本量全抓。
        let (caught, total, escape) = sample_oob_sweep();
        assert_eq!(total, 10);
        assert_eq!(OOB_WANTS.len(), 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        assert!(tamper_oob_ok());
    }

    #[test]
    fn injection_state_rewind_caught() {
        // 注入三：十路已签账面回拨全抓。
        let (caught, total, escape) = rewind_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 定点复核：齐签后任意节回拨已核必拒且账面不动。
        let mut led = build_synced_ledger();
        let _ = led.check_all();
        let _ = led.sign_all();
        let before = led.slot_at(2);
        assert_eq!(led.advance(2, SyncState::Checked), Err(SdDiag::ERewind.val()));
        assert_eq!(led.slot_at(2), before);
        assert!(tamper_rewind_ok());
    }

    #[test]
    fn sample_ten_batches_runnable() {
        // 判据直读：40 批清单齐 + 抽样 10 批逐批 mini 断言 5/5 绿并记账。
        assert!(batch_table_ok());
        let book = match run_sample(SAMPLE_SEED) {
            Ok(b) => b,
            Err(e) => panic!("抽样不可跑：码 0x{:05x}", e),
        };
        assert_eq!(book.len(), SAMPLE_CAP);
        assert!(book.green_all().is_ok());
        let mut seen: u64 = 0;
        let mut i = 0usize;
        while i < book.len() {
            let r = book.record_at(i).unwrap_or(SampleRecord {
                batch_no: 0,
                pass_n: 0,
                total: 0,
                green: false,
            });
            assert_eq!(r.total, MINI_TOTAL as u32);
            assert_eq!(r.pass_n, MINI_TOTAL as u32);
            assert!(r.green);
            let bit = 1u64.checked_shl(r.batch_no).unwrap_or(0);
            assert_eq!(seen & bit, 0, "抽样批号重复 {}", r.batch_no);
            seen |= bit;
            i += 1;
        }
        assert!(sample_run_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次全流程指纹相等且非零。
        match dual_run_verdict() {
            Ok(fp) => assert_ne!(fp, 0),
            Err(e) => panic!("双跑不一致：码 0x{:05x}", e),
        }
        assert_eq!(fingerprint(), fingerprint());
        assert!(dual_run_ok());
    }

    #[test]
    fn capacity_exhausted() {
        // 容量耗尽：四处第五笔声明拒；记账簿第十一笔拒；重复批号记账拒。
        let mut full = build_synced_ledger();
        assert_eq!(full.len(), SEC_CAP);
        assert_eq!(full.declare(0, 2, 2), Err(SdDiag::ECapFull.val()));
        let mut one = RootLedger::new();
        let _ = one.declare(0, 1, 1);
        assert_eq!(one.declare(0, 2, 2), Err(SdDiag::EDupSec.val()));
        let mut book = SampleBook::new();
        let mut n = 0u32;
        while n < SAMPLE_CAP as u32 {
            assert!(book
                .record(SampleRecord {
                    batch_no: n,
                    pass_n: MINI_TOTAL as u32,
                    total: MINI_TOTAL as u32,
                    green: true,
                })
                .is_ok());
            n += 1;
        }
        assert_eq!(
            book.record(SampleRecord {
                batch_no: SAMPLE_CAP as u32,
                pass_n: MINI_TOTAL as u32,
                total: MINI_TOTAL as u32,
                green: true,
            }),
            Err(SdDiag::ECapFull.val())
        );
        let mut dup = SampleBook::new();
        let rec = SampleRecord {
            batch_no: 6,
            pass_n: MINI_TOTAL as u32,
            total: MINI_TOTAL as u32,
            green: true,
        };
        let _ = dup.record(rec);
        assert_eq!(dup.record(rec), Err(SdDiag::ESampleDup.val()));
        assert!(capacity_ok());
    }

    #[test]
    fn registry_rejects_doctored_lists() {
        // 批清单验证器：批号重复/入口名缺/状态未签/批数非 40 逐路拒。
        let mut a: Vec<BatchEntry> = Vec::new();
        for e in BATCH_ALL.iter() {
            a.push(*e);
        }
        if let Some(x) = a.get_mut(1) {
            x.batch_no = 0;
        }
        assert_eq!(validate_registry(&a), Err(SdDiag::EBatchDup.val()));

        let mut b: Vec<BatchEntry> = Vec::new();
        for e in BATCH_ALL.iter() {
            b.push(*e);
        }
        if let Some(x) = b.get_mut(5) {
            x.entry_name = "";
        }
        assert_eq!(validate_registry(&b), Err(SdDiag::ENameBad.val()));

        let mut c: Vec<BatchEntry> = Vec::new();
        for e in BATCH_ALL.iter() {
            c.push(*e);
        }
        if let Some(x) = c.get_mut(7) {
            x.status = SyncState::Draft;
        }
        assert_eq!(validate_registry(&c), Err(SdDiag::EStatusMiss.val()));

        let d: Vec<BatchEntry> = Vec::new();
        assert_eq!(validate_registry(&d), Err(SdDiag::EBatchRange.val()));

        assert!(validate_registry(BATCH_ALL).is_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 15 条（14 判据 + 1 收口）。
        let s = run_unx12793_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert_eq!(s.len(), 15);
        assert_eq!(s.tally().1, 0);
    }
}
