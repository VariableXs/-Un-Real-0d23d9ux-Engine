//! UNX-F50086 字体许可位检查（可嵌入位拒绝嵌入显式告警）（0x436e0 段）
//!
//! 锚点：UNX-F50086-J1 许可位 ×4 档 ×10 全部正确落账；判据锚定 §AI-63 §2 专题二。
//!
//! 为什么许可位要单独成账，而不是散在嵌入流程里做几个 if：
//! 字体厂商的 `fsType` 是**法律约束**，不是渲染偏好。一旦某个文档把「限制
//! 嵌入」的字体打进 PDF 或随打印任务下发，责任落在输出端。因此这里把
//! 「判定」与「记账」拆开：判定是纯函数（同样输入必得同样结论），记账是
//! 只追加台账（每条决定一行，封存后不可再写），两者都能被审计复核。
//!
//! 四档语义（OS/2 规范，低三位互斥）：
//! * 0x0000 可安装嵌入（Installable）：无限制；
//! * 0x0002 受限（Restricted）：禁止嵌入，任何请求一律拒绝并告警；
//! * 0x0004 预览打印（PreviewPrint）：只允许「只读子集」随文档流转，
//!   整字嵌入等于把字体文件送给读者，必须拒绝；
//! * 0x0008 可编辑（Editable）：允许嵌入且允许二次编辑。
//!
//! 两个附加位不放宽、只收紧：
//! * 0x0100 禁止子集：请求子集时改判为「整字嵌入」（AllowNoSubset），
//!   账上写明告警，绝不允许悄悄切子集；
//! * 0x0200 仅位图：只能落位图；若调用方要轮廓，直接报 E_BITMAP，
//!   让上层显式决定「降级成位图」还是「换字体」，不做静默降级。
//!
//! 结构清单：
//! * 解析器：`parse_os2`（长度/版本/保留位/未知位/互斥位五道闸）；
//! * 契约表：`LicenseTier`（4 档）× `LicenseVerdict`（4 种结论）；
//! * 台账：`LicenseLedger`（容量 + 去重 + 四档四结论计数 + FNV 指纹 + 封存）；
//! * 状态机：`LicenseSession` Idle→Parsed→Decided→Booked；
//! * 注入核：`injection_core` 10 类坏样本；
//! * 诊断码闭集：SEG_BASE = 0x436e0，n ∈ [1,13]。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x436e0 起 12 个码，段宽 0x20，不与 50085 的 0x436c0 重叠）。
pub const SEG_BASE: u32 = 0x436e0;

pub const E_EMPTY: u16 = 1;
pub const E_SHORT: u16 = 2;
pub const E_VERSION: u16 = 3;
pub const E_RESERVED: u16 = 4;
pub const E_BITS: u16 = 5;
pub const E_TIER: u16 = 6;
pub const E_BITMAP: u16 = 7;
pub const E_DOC: u16 = 8;
pub const E_FONT: u16 = 9;
pub const E_DUP: u16 = 10;
pub const E_CAP: u16 = 11;
pub const E_SEAL: u16 = 12;
pub const E_STATE: u16 = 13;

/// OS/2 表最小可判长度：`fsType` 在偏移 8，占 2 字节。
pub const OS2_MIN_LEN: usize = 10;

/// 台账默认容量。64 条够覆盖「一个打印任务里出现的全部字体 × 文档」
/// 这一典型规模；再大属于异常，应分任务记账而不是无限追加。
pub const LEDGER_CAP: usize = 64;

/// `fsType` 位：保留位，必须为 0。
pub const FS_RESERVED: u16 = 0x0001;
/// `fsType` 位：受限嵌入（禁止嵌入）。
pub const FS_RESTRICTED: u16 = 0x0002;
/// `fsType` 位：仅预览与打印。
pub const FS_PREVIEW_PRINT: u16 = 0x0004;
/// `fsType` 位：可编辑嵌入。
pub const FS_EDITABLE: u16 = 0x0008;
/// `fsType` 位：禁止子集化。
pub const FS_NO_SUBSET: u16 = 0x0100;
/// `fsType` 位：仅位图嵌入。
pub const FS_BITMAP_ONLY: u16 = 0x0200;
/// 低三位掩码（互斥组）。
pub const FS_EMBED_MASK: u16 = 0x000E;
/// 全部已知位；`fs_type & !FS_KNOWN` 非零即为未知位。
pub const FS_KNOWN: u16 = 0x030E;
/// 允许的最高 OS/2 版本（v5 为当前最新）。
pub const OS2_MAX_VERSION: u16 = 5;

/// 字体许可错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseError {
    Empty,
    Short,
    Version,
    Reserved,
    Bits,
    Tier,
    Bitmap,
    Doc,
    Font,
    Dup,
    Cap,
    Seal,
    State,
}

impl LicenseError {
    fn ordinal(self) -> u16 {
        match self {
            LicenseError::Empty => E_EMPTY,
            LicenseError::Short => E_SHORT,
            LicenseError::Version => E_VERSION,
            LicenseError::Reserved => E_RESERVED,
            LicenseError::Bits => E_BITS,
            LicenseError::Tier => E_TIER,
            LicenseError::Bitmap => E_BITMAP,
            LicenseError::Doc => E_DOC,
            LicenseError::Font => E_FONT,
            LicenseError::Dup => E_DUP,
            LicenseError::Cap => E_CAP,
            LicenseError::Seal => E_SEAL,
            LicenseError::State => E_STATE,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            LicenseError::Empty => "os2 table missing",
            LicenseError::Short => "os2 table shorter than fsType field",
            LicenseError::Version => "os2 version unsupported",
            LicenseError::Reserved => "os2 reserved bit set",
            LicenseError::Bits => "os2 fsType has unknown bits",
            LicenseError::Tier => "os2 embedding bits conflict",
            LicenseError::Bitmap => "bitmap-only font cannot provide outlines",
            LicenseError::Doc => "document id missing",
            LicenseError::Font => "font id missing",
            LicenseError::Dup => "duplicate license record",
            LicenseError::Cap => "license ledger capacity exhausted",
            LicenseError::Seal => "license ledger already sealed",
            LicenseError::State => "illegal license session transition",
        }
    }
}

/// 四档嵌入许可。档位只由低三位决定，附加位不改变档位、只收紧结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseTier {
    Installable,
    Restricted,
    PreviewPrint,
    Editable,
}

impl LicenseTier {
    /// 档位序号：0=可安装、1=受限、2=预览打印、3=可编辑（台账下标）。
    pub fn ordinal(self) -> u8 {
        match self {
            LicenseTier::Installable => 0,
            LicenseTier::Restricted => 1,
            LicenseTier::PreviewPrint => 2,
            LicenseTier::Editable => 3,
        }
    }

    /// 中文档名，用于告警文本与审计报表。
    pub fn label(self) -> &'static str {
        match self {
            LicenseTier::Installable => "installable",
            LicenseTier::Restricted => "restricted",
            LicenseTier::PreviewPrint => "preview-print",
            LicenseTier::Editable => "editable",
        }
    }

    /// 由 `fsType` 低三位推档；两位及以上同时置位属于规范冲突，显式报错。
    pub fn from_fs_type(fs_type: u16) -> Result<LicenseTier, LicenseError> {
        let emb = fs_type & FS_EMBED_MASK;
        let mut set = 0u32;
        if emb & FS_RESTRICTED != 0 {
            set += 1;
        }
        if emb & FS_PREVIEW_PRINT != 0 {
            set += 1;
        }
        if emb & FS_EDITABLE != 0 {
            set += 1;
        }
        if set > 1 {
            return Err(LicenseError::Tier);
        }
        if emb & FS_RESTRICTED != 0 {
            return Ok(LicenseTier::Restricted);
        }
        if emb & FS_PREVIEW_PRINT != 0 {
            return Ok(LicenseTier::PreviewPrint);
        }
        if emb & FS_EDITABLE != 0 {
            return Ok(LicenseTier::Editable);
        }
        Ok(LicenseTier::Installable)
    }
}

/// OS/2 表的许可视图：只保留判定需要的字段，不缓存整张表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Os2Info {
    pub version: u16,
    pub fs_type: u16,
    pub tier: LicenseTier,
    pub no_subset: bool,
    pub bitmap_only: bool,
}

impl Os2Info {
    /// 是否允许子集化（禁止子集位取反）。
    pub fn subset_allowed(&self) -> bool {
        !self.no_subset
    }
}

/// 大端读 16 位，越界返回 None（零 panic 的唯一下标入口）。
pub fn be16(data: &[u8], off: usize) -> Option<u16> {
    let hi = *data.get(off)? as u16;
    let lo = *data.get(off + 1)? as u16;
    Some((hi << 8) | lo)
}

/// 解析 OS/2 表的许可字段。空表与短表分开报，便于上层区分「字体没有 OS/2
/// 表」和「表被截断」两种修法不同的故障。
pub fn parse_os2(table: &[u8]) -> Result<Os2Info, LicenseError> {
    if table.is_empty() {
        return Err(LicenseError::Empty);
    }
    if table.len() < OS2_MIN_LEN {
        return Err(LicenseError::Short);
    }
    let version = match be16(table, 0) {
        Some(v) => v,
        None => return Err(LicenseError::Short),
    };
    if version > OS2_MAX_VERSION {
        return Err(LicenseError::Version);
    }
    let fs_type = match be16(table, 8) {
        Some(v) => v,
        None => return Err(LicenseError::Short),
    };
    // 保留位先判：规范要求 0，置位往往意味着表被写坏，不能当作「未知位」蒙混。
    if fs_type & FS_RESERVED != 0 {
        return Err(LicenseError::Reserved);
    }
    if fs_type & !FS_KNOWN != 0 {
        return Err(LicenseError::Bits);
    }
    let tier = LicenseTier::from_fs_type(fs_type)?;
    Ok(Os2Info {
        version,
        fs_type,
        tier,
        no_subset: fs_type & FS_NO_SUBSET != 0,
        bitmap_only: fs_type & FS_BITMAP_ONLY != 0,
    })
}

/// 一次嵌入请求。`font_id`/`doc_id` 为 0 视为未赋值，直接拒绝，
/// 避免台账里出现无法回溯的匿名记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbedRequest {
    pub font_id: u32,
    pub doc_id: u32,
    pub subset: bool,
    pub bitmap: bool,
}

impl EmbedRequest {
    /// 构造请求（不做校验，校验在 `decide` 里统一做，保证单点）。
    pub fn new(font_id: u32, doc_id: u32, subset: bool, bitmap: bool) -> EmbedRequest {
        EmbedRequest { font_id, doc_id, subset, bitmap }
    }

    /// 是否请求了轮廓（非位图）嵌入。
    pub fn wants_outline(&self) -> bool {
        !self.bitmap
    }
}

/// 四类结论。前三种是「可嵌入」，但后两种带强制告警；第四种是拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseVerdict {
    Allow,
    AllowNoSubset,
    AllowBitmapOnly,
    Deny,
}

impl LicenseVerdict {
    /// 结论序号：0=允许、1=整字嵌入、2=仅位图、3=拒绝（台账下标）。
    pub fn ordinal(self) -> u8 {
        match self {
            LicenseVerdict::Allow => 0,
            LicenseVerdict::AllowNoSubset => 1,
            LicenseVerdict::AllowBitmapOnly => 2,
            LicenseVerdict::Deny => 3,
        }
    }

    /// 是否允许嵌入（三种 Allow 变体都算允许）。
    pub fn allowed(self) -> bool {
        self != LicenseVerdict::Deny
    }

    /// 是否必须携带告警文本。
    pub fn needs_warn(self) -> bool {
        self != LicenseVerdict::Allow
    }
}

/// 判定结果：结论 + 档位 + 告警。告警为 `&'static str`，不分配、可跨线程
/// 直接写入报表，且「拒绝却无告警」在类型层面就被 `needs_warn` 卡住。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub verdict: LicenseVerdict,
    pub tier: LicenseTier,
    pub warn: &'static str,
}

/// 许可判定的唯一入口。顺序即优先级：输入完整性 → 位图可行性 → 档位策略。
pub fn decide(info: &Os2Info, req: &EmbedRequest) -> Result<Decision, LicenseError> {
    if req.font_id == 0 {
        return Err(LicenseError::Font);
    }
    if req.doc_id == 0 {
        return Err(LicenseError::Doc);
    }
    // 位图专供字体在物理上没有轮廓可给：这是可行性问题，不是许可宽严问题，
    // 因此排在档位判断之前，且必须报错而不是返回一个「拒绝」结论。
    if info.bitmap_only && req.wants_outline() {
        return Err(LicenseError::Bitmap);
    }
    let (verdict, warn) = match info.tier {
        LicenseTier::Restricted => (
            LicenseVerdict::Deny,
            "restricted license: embedding is not permitted",
        ),
        LicenseTier::PreviewPrint => {
            if req.subset {
                (LicenseVerdict::Allow, "")
            } else {
                (
                    LicenseVerdict::Deny,
                    "preview-print only: full-face embedding refused",
                )
            }
        }
        LicenseTier::Installable | LicenseTier::Editable => {
            if info.bitmap_only {
                (
                    LicenseVerdict::AllowBitmapOnly,
                    "bitmap-only font: outlines must not be embedded",
                )
            } else if req.subset && info.no_subset {
                (
                    LicenseVerdict::AllowNoSubset,
                    "no-subsetting: embed the full face instead",
                )
            } else {
                (LicenseVerdict::Allow, "")
            }
        }
    };
    Ok(Decision { verdict, tier: info.tier, warn })
}

/// 台账一行。字段与判定结果一一对应，不含派生量，便于逐行复核。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LicenseRecord {
    pub font_id: u32,
    pub doc_id: u32,
    pub tier: LicenseTier,
    pub verdict: LicenseVerdict,
    pub warned: bool,
}

impl LicenseRecord {
    /// 由请求与判定生成记录；告警标记由结论推导，杜绝「拒绝但 warned=false」。
    pub fn from_request(req: &EmbedRequest, dec: &Decision) -> LicenseRecord {
        LicenseRecord {
            font_id: req.font_id,
            doc_id: req.doc_id,
            tier: dec.tier,
            verdict: dec.verdict,
            warned: dec.verdict.needs_warn(),
        }
    }
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;
const TALLY_SLOTS: usize = 16;

/// FNV-1a 单字节混合。
pub fn fnv32(seed: u32, byte: u8) -> u32 {
    (seed ^ byte as u32).wrapping_mul(FNV_PRIME)
}

/// 把 32 位字段按小端逐字节混入指纹。
pub fn mix_u32(seed: u32, value: u32) -> u32 {
    let b = value.to_le_bytes();
    let mut h = seed;
    let mut i = 0usize;
    while i < 4 {
        h = fnv32(h, b[i]);
        i += 1;
    }
    h
}

/// 只追加许可台账。核心不变量：
/// * `count == sum(tally)`（每条记录恰好落在一个格子里）；
/// * `warned == count - tally[Allow]`（告警数与结论数严格对应）；
/// * 封存后不可增删，指纹冻结，任何后续写入报 E_SEAL。
#[derive(Clone, Debug)]
pub struct LicenseLedger {
    capacity: usize,
    records: Vec<LicenseRecord>,
    tally: [u32; TALLY_SLOTS],
    warned: usize,
    fingerprint: u32,
    sealed: bool,
}

impl LicenseLedger {
    /// 新建台账；容量超过 `LEDGER_CAP` 时按上限截断（防止误传巨大值）。
    pub fn new(capacity: usize) -> LicenseLedger {
        let cap = if capacity > LEDGER_CAP { LEDGER_CAP } else { capacity };
        LicenseLedger {
            capacity: cap,
            records: Vec::new(),
            tally: [0u32; TALLY_SLOTS],
            warned: 0,
            fingerprint: FNV_OFFSET,
            sealed: false,
        }
    }

    /// 追加一条记录。去重键是 (font_id, doc_id)：同一文档对同一字体只允许
    /// 记一条，重复说明调度层出了回流，必须暴露而不是覆盖。
    pub fn push(&mut self, rec: LicenseRecord) -> Result<(), LicenseError> {
        if self.sealed {
            return Err(LicenseError::Seal);
        }
        if rec.font_id == 0 {
            return Err(LicenseError::Font);
        }
        if rec.doc_id == 0 {
            return Err(LicenseError::Doc);
        }
        if self.records.len() >= self.capacity {
            return Err(LicenseError::Cap);
        }
        if self.contains(rec.font_id, rec.doc_id) {
            return Err(LicenseError::Dup);
        }
        let slot = (rec.tier.ordinal() as usize) * 4 + (rec.verdict.ordinal() as usize);
        if slot < TALLY_SLOTS {
            self.tally[slot] += 1;
        }
        if rec.warned {
            self.warned += 1;
        }
        let mut fp = self.fingerprint;
        fp = mix_u32(fp, rec.font_id);
        fp = mix_u32(fp, rec.doc_id);
        fp = fnv32(fp, rec.tier.ordinal());
        fp = fnv32(fp, rec.verdict.ordinal());
        fp = fnv32(fp, if rec.warned { 1 } else { 0 });
        self.fingerprint = fp;
        self.records.push(rec);
        Ok(())
    }

    /// 去重查询：(font_id, doc_id) 是否已在账上。
    pub fn contains(&self, font_id: u32, doc_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            let r = self.records[i];
            if r.font_id == font_id && r.doc_id == doc_id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 封存台账：冻结指纹并拒绝后续写入；重复封存报 E_SEAL。
    pub fn seal(&mut self) -> Result<u32, LicenseError> {
        if self.sealed {
            return Err(LicenseError::Seal);
        }
        self.sealed = true;
        Ok(self.fingerprint)
    }

    /// 当前条数。
    pub fn count(&self) -> usize {
        self.records.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 指定档位 × 结论的计数。
    pub fn tally(&self, tier: LicenseTier, verdict: LicenseVerdict) -> u32 {
        let slot = (tier.ordinal() as usize) * 4 + (verdict.ordinal() as usize);
        if slot < TALLY_SLOTS {
            self.tally[slot]
        } else {
            0
        }
    }

    /// 带告警的记录数。
    pub fn warned_count(&self) -> usize {
        self.warned
    }

    /// 当前/封存时的指纹。
    pub fn fingerprint(&self) -> u32 {
        self.fingerprint
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 取第 i 条记录。
    pub fn record(&self, i: usize) -> Option<LicenseRecord> {
        self.records.get(i).copied()
    }

    /// 重算指纹并与账上比对，同时校验两条守恒式。
    pub fn verify(&self) -> bool {
        let mut fp = FNV_OFFSET;
        let mut sum = 0u32;
        let mut warned = 0usize;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = self.records[i];
            fp = mix_u32(fp, r.font_id);
            fp = mix_u32(fp, r.doc_id);
            fp = fnv32(fp, r.tier.ordinal());
            fp = fnv32(fp, r.verdict.ordinal());
            fp = fnv32(fp, if r.warned { 1 } else { 0 });
            if r.warned {
                warned += 1;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < TALLY_SLOTS {
            sum += self.tally[j];
            j += 1;
        }
        let allows = self.tally[0] + self.tally[4] + self.tally[8] + self.tally[12];
        fp == self.fingerprint
            && sum as usize == self.records.len()
            && warned == self.warned
            && self.warned as u32 + allows == self.records.len() as u32
    }
}

/// 会话阶段。刻意把「解析」与「判定」分开：解析失败时不许留下半个判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicensePhase {
    Idle,
    Parsed,
    Decided,
    Booked,
}

/// 单字体许可会话：一次 open 只处理一个字体，避免跨字体串账。
#[derive(Clone, Debug)]
pub struct LicenseSession {
    font_id: u32,
    opened: bool,
    phase: LicensePhase,
    info: Option<Os2Info>,
    decision: Option<Decision>,
    request: Option<EmbedRequest>,
}

impl LicenseSession {
    /// 新建空会话（未 open，任何 load 都会被状态机拒绝）。
    pub fn new() -> LicenseSession {
        LicenseSession {
            font_id: 0,
            opened: false,
            phase: LicensePhase::Idle,
            info: None,
            decision: None,
            request: None,
        }
    }

    /// 绑定字体。可从 Idle 或 Booked 进入，已解析/已判定时重开会丢账，故拒绝。
    pub fn open(&mut self, font_id: u32) -> Result<(), LicenseError> {
        if font_id == 0 {
            return Err(LicenseError::Font);
        }
        if self.phase != LicensePhase::Idle && self.phase != LicensePhase::Booked {
            return Err(LicenseError::State);
        }
        self.font_id = font_id;
        self.opened = true;
        self.phase = LicensePhase::Idle;
        self.info = None;
        self.decision = None;
        self.request = None;
        Ok(())
    }

    /// 载入 OS/2 表并解析；失败时停留在 Idle，可换表重试。
    pub fn load(&mut self, table: &[u8]) -> Result<(), LicenseError> {
        if !self.opened || self.phase != LicensePhase::Idle {
            return Err(LicenseError::State);
        }
        let info = parse_os2(table)?;
        self.info = Some(info);
        self.phase = LicensePhase::Parsed;
        Ok(())
    }

    /// 在已解析的许可上做判定；失败不改变阶段，便于修正请求后重试。
    pub fn decide(&mut self, doc_id: u32, subset: bool, bitmap: bool) -> Result<Decision, LicenseError> {
        if self.phase != LicensePhase::Parsed {
            return Err(LicenseError::State);
        }
        let info = match self.info {
            Some(v) => v,
            None => return Err(LicenseError::State),
        };
        let req = EmbedRequest::new(self.font_id, doc_id, subset, bitmap);
        let dec = decide(&info, &req)?;
        self.request = Some(req);
        self.decision = Some(dec);
        self.phase = LicensePhase::Decided;
        Ok(dec)
    }

    /// 把已判定结果写入台账；写失败保持 Decided，可换台账重试。
    pub fn book(&mut self, ledger: &mut LicenseLedger) -> Result<(), LicenseError> {
        if self.phase != LicensePhase::Decided {
            return Err(LicenseError::State);
        }
        let req = match self.request {
            Some(v) => v,
            None => return Err(LicenseError::State),
        };
        let dec = match self.decision {
            Some(v) => v,
            None => return Err(LicenseError::State),
        };
        let rec = LicenseRecord::from_request(&req, &dec);
        ledger.push(rec)?;
        self.phase = LicensePhase::Booked;
        Ok(())
    }

    /// 回到初始态；不触碰台账（已落账的记录不可撤销）。
    pub fn reset(&mut self) {
        self.font_id = 0;
        self.opened = false;
        self.phase = LicensePhase::Idle;
        self.info = None;
        self.decision = None;
        self.request = None;
    }

    /// 当前阶段。
    pub fn phase(&self) -> LicensePhase {
        self.phase
    }

    /// 当前绑定的字体号。
    pub fn font_id(&self) -> u32 {
        self.font_id
    }

    /// 最近一次解析出的许可视图。
    pub fn info(&self) -> Option<Os2Info> {
        self.info
    }

    /// 最近一次判定结果。
    pub fn decision(&self) -> Option<Decision> {
        self.decision
    }
}

/// 构造最小 OS/2 表（10 字节），仅填 `version` 与 `fsType`，
/// 其余字段对许可判定无影响，置 0。
pub fn build_os2(version: u16, fs_type: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(OS2_MIN_LEN);
    let v = version.to_be_bytes();
    out.push(v[0]);
    out.push(v[1]);
    let mut i = 2usize;
    while i < 8 {
        out.push(0u8);
        i += 1;
    }
    let f = fs_type.to_be_bytes();
    out.push(f[0]);
    out.push(f[1]);
    out
}

/// 十类坏样本注入：每类都断言具体诊断码，任何一类漏抓都会让总数对不上。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut total = 0usize;

    // 1 空表
    total += 1;
    if parse_os2(&[]).err() == Some(LicenseError::Empty) {
        caught += 1;
    }

    // 2 短表（9 字节，fsType 只剩一半）
    total += 1;
    let mut short = build_os2(4, 0);
    short.truncate(9);
    if parse_os2(&short).err() == Some(LicenseError::Short) {
        caught += 1;
    }

    // 3 版本越界
    total += 1;
    if parse_os2(&build_os2(6, 0)).err() == Some(LicenseError::Version) {
        caught += 1;
    }

    // 4 保留位
    total += 1;
    if parse_os2(&build_os2(4, FS_RESERVED)).err() == Some(LicenseError::Reserved) {
        caught += 1;
    }

    // 5 未知位
    total += 1;
    if parse_os2(&build_os2(4, 0x8000)).err() == Some(LicenseError::Bits) {
        caught += 1;
    }

    // 6 互斥位冲突（受限 + 预览打印）
    total += 1;
    if parse_os2(&build_os2(4, FS_RESTRICTED | FS_PREVIEW_PRINT)).err() == Some(LicenseError::Tier) {
        caught += 1;
    }

    // 7 位图专供字体被索要轮廓
    total += 1;
    if let Ok(info) = parse_os2(&build_os2(4, FS_BITMAP_ONLY)) {
        let req = EmbedRequest::new(1, 1, false, false);
        if decide(&info, &req).err() == Some(LicenseError::Bitmap) {
            caught += 1;
        }
    }

    // 8 文档号缺失
    total += 1;
    if let Ok(info) = parse_os2(&build_os2(4, 0)) {
        let req = EmbedRequest::new(1, 0, true, false);
        if decide(&info, &req).err() == Some(LicenseError::Doc) {
            caught += 1;
        }
    }

    // 9 字体号缺失
    total += 1;
    if let Ok(info) = parse_os2(&build_os2(4, 0)) {
        let req = EmbedRequest::new(0, 1, true, false);
        if decide(&info, &req).err() == Some(LicenseError::Font) {
            caught += 1;
        }
    }

    // 10 台账重复落账
    total += 1;
    let mut ledger = LicenseLedger::new(8);
    let rec = LicenseRecord {
        font_id: 7,
        doc_id: 9,
        tier: LicenseTier::Installable,
        verdict: LicenseVerdict::Allow,
        warned: false,
    };
    if ledger.push(rec).is_ok() && ledger.push(rec).err() == Some(LicenseError::Dup) {
        caught += 1;
    }

    (caught, total)
}

/// 四档 × 十请求的标准期望表。这张表按规范手写，不复用 `decide` 的分支，
/// 因此能独立地检出错判（若实现与规范同时写错才会同错）。
pub const EXPECTED_MATRIX: [[LicenseVerdict; 10]; 4] = [
    [
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::AllowNoSubset,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::AllowBitmapOnly,
    ],
    [
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
        LicenseVerdict::Deny,
    ],
    [
        LicenseVerdict::Allow,
        LicenseVerdict::Deny,
        LicenseVerdict::Allow,
        LicenseVerdict::Deny,
        LicenseVerdict::Allow,
        LicenseVerdict::Deny,
        LicenseVerdict::Allow,
        LicenseVerdict::Deny,
        LicenseVerdict::Allow,
        LicenseVerdict::Deny,
    ],
    [
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::AllowNoSubset,
        LicenseVerdict::Allow,
        LicenseVerdict::Allow,
        LicenseVerdict::AllowBitmapOnly,
    ],
];

/// 档位下标 → 档位枚举。
pub fn tier_at(t: usize) -> LicenseTier {
    match t {
        0 => LicenseTier::Installable,
        1 => LicenseTier::Restricted,
        2 => LicenseTier::PreviewPrint,
        _ => LicenseTier::Editable,
    }
}

/// 档位 → 基础 `fsType` 低三位。
pub fn fs_of(tier: LicenseTier) -> u16 {
    match tier {
        LicenseTier::Installable => 0x0000,
        LicenseTier::Restricted => FS_RESTRICTED,
        LicenseTier::PreviewPrint => FS_PREVIEW_PRINT,
        LicenseTier::Editable => FS_EDITABLE,
    }
}

/// 第 i 个请求是否请求子集（偶数号请求）。
pub fn req_subset(i: usize) -> bool {
    i % 2 == 0
}

/// 第 i 个请求是否请求位图（仅 9 号）。
pub fn req_bitmap(i: usize) -> bool {
    i == 9
}

/// 第 i 个请求在档位 t 上是否附加「禁止子集」位（0/3 档的 6 号）。
pub fn extra_no_subset(t: usize, i: usize) -> bool {
    (t == 0 || t == 3) && i == 6
}

/// 第 i 个请求在档位 t 上是否附加「仅位图」位（0/3 档的 9 号）。
pub fn extra_bitmap_only(t: usize, i: usize) -> bool {
    (t == 0 || t == 3) && i == 9
}

/// 组一枚举出的 `fsType`。
pub fn matrix_fs_type(t: usize, i: usize) -> u16 {
    let mut fs = fs_of(tier_at(t));
    if extra_no_subset(t, i) {
        fs |= FS_NO_SUBSET;
    }
    if extra_bitmap_only(t, i) {
        fs |= FS_BITMAP_ONLY;
    }
    fs
}

/// 组一枚举出的字体号（同一档内互不相同，跨档也不撞）。
pub fn matrix_font_id(t: usize, i: usize) -> u32 {
    5000 + (t as u32) * 100 + i as u32
}

/// 组一枚举出的文档号。
pub fn matrix_doc_id(t: usize, i: usize) -> u32 {
    9000 + (t as u32) * 10 + i as u32
}

/// 构造「四档 × 十请求」的许可台账，返回 (台账, 不匹配数)。
pub fn build_matrix_ledger() -> (LicenseLedger, usize) {
    let mut ledger = LicenseLedger::new(LEDGER_CAP);
    let mut mismatched = 0usize;
    let mut t = 0usize;
    while t < 4 {
        let mut i = 0usize;
        while i < 10 {
            let table = build_os2(4, matrix_fs_type(t, i));
            let info = match parse_os2(&table) {
                Ok(v) => v,
                Err(_) => {
                    mismatched += 1;
                    i += 1;
                    continue;
                }
            };
            let req = EmbedRequest::new(matrix_font_id(t, i), matrix_doc_id(t, i), req_subset(i), req_bitmap(i));
            let dec = match decide(&info, &req) {
                Ok(v) => v,
                Err(_) => {
                    mismatched += 1;
                    i += 1;
                    continue;
                }
            };
            if dec.verdict != EXPECTED_MATRIX[t][i] {
                mismatched += 1;
            }
            if dec.verdict.needs_warn() && dec.warn.is_empty() {
                mismatched += 1;
            }
            if !dec.verdict.needs_warn() && !dec.warn.is_empty() {
                mismatched += 1;
            }
            if ledger.push(LicenseRecord::from_request(&req, &dec)).is_err() {
                mismatched += 1;
            }
            i += 1;
        }
        t += 1;
    }
    (ledger, mismatched)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50086_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50086");

    // J1-1 四档 × 十请求：40 条全部按规范矩阵落账，且每条告警状态与结论匹配
    let (mut ledger, mismatched) = build_matrix_ledger();
    let booked = ledger.count();
    let det = format!("booked={} mismatch={}", booked, mismatched);
    s.add_dyn(
        "50086-J1-许可位4档10落账",
        booked == 40 && mismatched == 0,
        &det,
    );

    // J1-2 分档计数守恒：每档恰好 10 条，四档合计 40 条
    let mut per_tier_ok = true;
    let mut t = 0usize;
    while t < 4 {
        let mut sum = 0u32;
        let mut v = 0usize;
        while v < 4 {
            sum += ledger.tally(
                tier_at(t),
                match v {
                    0 => LicenseVerdict::Allow,
                    1 => LicenseVerdict::AllowNoSubset,
                    2 => LicenseVerdict::AllowBitmapOnly,
                    _ => LicenseVerdict::Deny,
                },
            );
            v += 1;
        }
        if sum != 10 {
            per_tier_ok = false;
        }
        t += 1;
    }
    let det = format!("per-tier=10x4 total={}", ledger.count());
    s.add_dyn("50086-J1-四档各十条", per_tier_ok, &det);

    // J1-3 结论分布与手写矩阵逐格相等（4 档 × 4 结论 = 16 格）
    let mut cells_ok = true;
    let mut t = 0usize;
    while t < 4 {
        let mut v = 0usize;
        while v < 4 {
            let verdict = match v {
                0 => LicenseVerdict::Allow,
                1 => LicenseVerdict::AllowNoSubset,
                2 => LicenseVerdict::AllowBitmapOnly,
                _ => LicenseVerdict::Deny,
            };
            let mut want = 0u32;
            let mut i = 0usize;
            while i < 10 {
                if EXPECTED_MATRIX[t][i] == verdict {
                    want += 1;
                }
                i += 1;
            }
            if ledger.tally(tier_at(t), verdict) != want {
                cells_ok = false;
            }
            v += 1;
        }
        t += 1;
    }
    let det = format!("cells=16 warned={}", ledger.warned_count());
    s.add_dyn("50086-J1-结论分布对齐", cells_ok, &det);

    // J1-4 告警数与拒绝/降级数严格互补（Allow 之外必带告警）
    let allows = ledger.tally(LicenseTier::Installable, LicenseVerdict::Allow)
        + ledger.tally(LicenseTier::Restricted, LicenseVerdict::Allow)
        + ledger.tally(LicenseTier::PreviewPrint, LicenseVerdict::Allow)
        + ledger.tally(LicenseTier::Editable, LicenseVerdict::Allow);
    let ok = ledger.warned_count() as u32 + allows == booked as u32 && ledger.warned_count() == 19;
    let det = format!("warned={} allows={}", ledger.warned_count(), allows);
    s.add_dyn("50086-J1-告警互补", ok, &det);

    // J1-5 台账自检：指纹可重算、两条守恒式成立
    s.add("50086-J1-台账自检", ledger.verify(), if ledger.verify() { "" } else { "指纹或守恒失效" });

    // J1-6 档位解析：四位基础值逐一落在正确档位
    let t0 = parse_os2(&build_os2(4, 0x0000)).map(|v| v.tier) == Ok(LicenseTier::Installable);
    let t1 = parse_os2(&build_os2(4, FS_RESTRICTED)).map(|v| v.tier) == Ok(LicenseTier::Restricted);
    let t2 = parse_os2(&build_os2(4, FS_PREVIEW_PRINT)).map(|v| v.tier) == Ok(LicenseTier::PreviewPrint);
    let t3 = parse_os2(&build_os2(4, FS_EDITABLE)).map(|v| v.tier) == Ok(LicenseTier::Editable);
    let t4 = parse_os2(&build_os2(4, FS_RESTRICTED | FS_NO_SUBSET)).map(|v| v.tier) == Ok(LicenseTier::Restricted);
    let ok = t0 && t1 && t2 && t3 && t4;
    s.add("50086-J1-档位解析", ok, if ok { "" } else { "档位误判" });

    // J1-7 互斥位冲突：任意两位同时置位都应报 E_TIER
    let c1 = parse_os2(&build_os2(4, 0x0006)).err() == Some(LicenseError::Tier);
    let c2 = parse_os2(&build_os2(4, 0x000A)).err() == Some(LicenseError::Tier);
    let c3 = parse_os2(&build_os2(4, 0x000C)).err() == Some(LicenseError::Tier);
    let c4 = parse_os2(&build_os2(4, 0x000E)).err() == Some(LicenseError::Tier);
    let ok = c1 && c2 && c3 && c4;
    s.add("50086-J1-冲突位拒绝", ok, if ok { "" } else { "冲突位漏判" });

    // J1-8 保留位与未知位分别报错，不混为一谈
    let r1 = parse_os2(&build_os2(4, FS_RESERVED)).err() == Some(LicenseError::Reserved);
    let b1 = parse_os2(&build_os2(4, 0x0010)).err() == Some(LicenseError::Bits);
    let b2 = parse_os2(&build_os2(4, 0x8000)).err() == Some(LicenseError::Bits);
    let b3 = parse_os2(&build_os2(4, 0x0010 | 0x0001)).err() == Some(LicenseError::Reserved);
    let ok = r1 && b1 && b2 && b3;
    s.add("50086-J1-保留未知分离", ok, if ok { "" } else { "位判优先级错" });

    // J1-9 长度与版本闸门：空/短/越界三码各自命中，合法边界放行
    let e1 = parse_os2(&[]).err() == Some(LicenseError::Empty);
    let mut short = build_os2(4, 0);
    short.truncate(9);
    let e2 = parse_os2(&short).err() == Some(LicenseError::Short);
    let e3 = parse_os2(&build_os2(6, 0)).err() == Some(LicenseError::Version);
    let e4 = parse_os2(&build_os2(5, 0)).is_ok();
    let e5 = parse_os2(&build_os2(0, 0)).is_ok();
    let ok = e1 && e2 && e3 && e4 && e5;
    s.add("50086-J1-长度版本闸门", ok, if ok { "" } else { "闸门顺序错" });

    // J1-10 位图专供：轮廓请求硬报错，位图请求改判为仅位图并带告警
    let ok = match parse_os2(&build_os2(4, FS_BITMAP_ONLY)) {
        Ok(info) => {
            let outline = decide(&info, &EmbedRequest::new(1, 1, false, false)).err() == Some(LicenseError::Bitmap);
            let bmp = decide(&info, &EmbedRequest::new(1, 1, false, true));
            let bmp_ok = match bmp {
                Ok(d) => d.verdict == LicenseVerdict::AllowBitmapOnly && !d.warn.is_empty(),
                Err(_) => false,
            };
            outline && bmp_ok
        }
        Err(_) => false,
    };
    s.add("50086-J1-位图位", ok, if ok { "" } else { "位图位失效" });

    // J1-11 禁止子集位：请求子集改判整字嵌入，不请求子集则与常规一致
    let ok = match parse_os2(&build_os2(4, FS_EDITABLE | FS_NO_SUBSET)) {
        Ok(info) => {
            let sub = decide(&info, &EmbedRequest::new(2, 2, true, false));
            let full = decide(&info, &EmbedRequest::new(2, 2, false, false));
            let a = match sub {
                Ok(d) => d.verdict == LicenseVerdict::AllowNoSubset && !d.warn.is_empty(),
                Err(_) => false,
            };
            let b = match full {
                Ok(d) => d.verdict == LicenseVerdict::Allow && d.warn.is_empty(),
                Err(_) => false,
            };
            a && b
        }
        Err(_) => false,
    };
    s.add("50086-J1-禁止子集位", ok, if ok { "" } else { "子集位失效" });

    // J1-12 封存：封存后写入报 E_SEAL，重复封存报 E_SEAL，指纹不变
    let fp_before = ledger.fingerprint();
    let seal1 = ledger.seal();
    let seal2 = ledger.seal();
    let rec_after = LicenseRecord {
        font_id: 7777,
        doc_id: 8888,
        tier: LicenseTier::Installable,
        verdict: LicenseVerdict::Allow,
        warned: false,
    };
    let push_after = ledger.push(rec_after);
    let ok = seal1.is_ok()
        && seal2.err() == Some(LicenseError::Seal)
        && push_after.err() == Some(LicenseError::Seal)
        && ledger.fingerprint() == fp_before
        && ledger.count() == 40;
    s.add("50086-J1-封存不可写", ok, if ok { "" } else { "封存失效" });

    // J1-13 容量与去重：第 cap+1 条报 E_CAP，同键第二条报 E_DUP，计数不涨
    let mut small = LicenseLedger::new(3);
    let base = LicenseRecord {
        font_id: 11,
        doc_id: 22,
        tier: LicenseTier::Editable,
        verdict: LicenseVerdict::Allow,
        warned: false,
    };
    let mut filled = 0usize;
    let mut i = 0u32;
    while i < 3 {
        let mut r = base;
        r.doc_id = 22 + i;
        if small.push(r).is_ok() {
            filled += 1;
        }
        i += 1;
    }
    let cap_err = small.push(base).err() == Some(LicenseError::Cap);
    let mut dup = LicenseLedger::new(8);
    let first = dup.push(base);
    let second = dup.push(base);
    let ok = filled == 3
        && cap_err
        && small.count() == 3
        && first.is_ok()
        && second.err() == Some(LicenseError::Dup)
        && dup.count() == 1;
    s.add("50086-J1-容量去重", ok, if ok { "" } else { "容量或去重失效" });

    // J1-14 状态机：全部非法迁移报 E_STATE，合法路径走完 Idle→Booked
    let mut sess = LicenseSession::new();
    let early_decide = sess.decide(1, true, false).err() == Some(LicenseError::State);
    let zero_font = sess.open(0).err() == Some(LicenseError::Font);
    let open_ok = sess.open(31).is_ok();
    let load_early = sess.decide(1, true, false).err() == Some(LicenseError::State);
    let load_ok = sess.load(&build_os2(4, FS_EDITABLE)).is_ok();
    let load_twice = sess.load(&build_os2(4, FS_EDITABLE)).err() == Some(LicenseError::State);
    let zero_doc = sess.decide(0, true, false).err() == Some(LicenseError::Doc);
    let decided = sess.decide(41, true, false);
    let dec_ok = match decided {
        Ok(d) => d.verdict == LicenseVerdict::Allow,
        Err(_) => false,
    };
    let mut book_ledger = LicenseLedger::new(8);
    let book_ok = sess.book(&mut book_ledger).is_ok() && sess.phase() == LicensePhase::Booked;
    let book_twice = sess.book(&mut book_ledger).err() == Some(LicenseError::State);
    let reopen = sess.open(32).is_ok() && sess.phase() == LicensePhase::Idle;
    let ok = early_decide
        && zero_font
        && open_ok
        && load_early
        && load_ok
        && load_twice
        && zero_doc
        && dec_ok
        && book_ok
        && book_twice
        && reopen;
    s.add("50086-J1-会话状态机", ok, if ok { "" } else { "迁移漏判" });

    // J1-15 确定性：同输入两次建账，逐条字段与指纹完全一致
    let (a, ma) = build_matrix_ledger();
    let (b, mb) = build_matrix_ledger();
    let mut same = ma == 0 && mb == 0 && a.count() == b.count() && a.fingerprint() == b.fingerprint();
    let mut k = 0usize;
    while k < a.count() {
        match (a.record(k), b.record(k)) {
            (Some(x), Some(y)) => {
                if x != y {
                    same = false;
                }
            }
            _ => same = false,
        }
        k += 1;
    }
    s.add("50086-J1-建账确定", same, if same { "" } else { "两次建账不一致" });

    // J1-16 注入核：10 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total && total >= 8;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50086-J1-注入必抓", ok, &det);

    // J1-17 码值闭集：12 个码全部严格落在本段内
    let codes = [
        LicenseError::Empty,
        LicenseError::Short,
        LicenseError::Version,
        LicenseError::Reserved,
        LicenseError::Bits,
        LicenseError::Tier,
        LicenseError::Bitmap,
        LicenseError::Doc,
        LicenseError::Font,
        LicenseError::Dup,
        LicenseError::Cap,
        LicenseError::Seal,
        LicenseError::State,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = codes[i].code();
        if c <= SEG_BASE || c > SEG_BASE + 13 {
            ok = false;
        }
        if codes[i].message().is_empty() {
            ok = false;
        }
        i += 1;
    }
    s.add("50086-J1-码值闭集", ok, if ok { "" } else { "码值越段" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50086_checks_green() {
        let set = run_unx50086_checks();
        assert!(set.all_passed(), "UNX-F50086 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50086_matrix_books_40() {
        let (ledger, mismatch) = build_matrix_ledger();
        assert_eq!(mismatch, 0);
        assert_eq!(ledger.count(), 40);
        assert!(ledger.verify());
        assert_eq!(ledger.warned_count(), 19);
    }

    #[test]
    fn unx50086_restricted_always_denies() {
        let info = match parse_os2(&build_os2(4, FS_RESTRICTED)) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut i = 0u32;
        let mut bad = 0usize;
        while i < 8 {
            let req = EmbedRequest::new(1, 1 + i, i % 2 == 0, i % 3 == 0);
            match decide(&info, &req) {
                Ok(d) => {
                    if d.verdict != LicenseVerdict::Deny || d.warn.is_empty() {
                        bad += 1;
                    }
                }
                Err(_) => bad += 1,
            }
            i += 1;
        }
        assert_eq!(bad, 0);
    }

    #[test]
    fn unx50086_injection() {
        let (caught, total) = injection_core();
        assert_eq!(total, 10);
        assert_eq!(caught, total);
    }
}
