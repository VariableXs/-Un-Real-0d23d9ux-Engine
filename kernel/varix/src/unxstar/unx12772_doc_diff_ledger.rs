//! UNX-F12772 文档差异列账（发现即登记零静默） · 0x82B00段
//!
//! 判据原文（派工卡 12772 逐字锚定）：
//!   UNX-F12773-J1 双机抽验 10 场景一致（文档行为面），CONFLICT 显式。
//!
//! 设计要点：
//! 1. 差异登记表域模型：每条文档差异持四元组（差异号/资产引用/差异类别/
//!    三态相位），差异号由单调发生器签发，资产引用必须先入资产名册才能
//!    被判定——未登记资产一律 ENOENT，杜绝幽灵资产进账；
//! 2. 三态闭集 Discovered（发现未登记）→ Ledgered（已登记）→ Resolved
//!    （已销账）只进不退：倒退显性拒（ERewind）、同级重复推进拒
//!    （EOrder）、发现态越级销账拒（ENotLedgered），状态机单调性即协议；
//! 3. 发现即登记零静默：差异判定器 judge_diff 在判定新发现前先扫登记表，
//!    任何停留 Discovered 态未登记的旧发现（静默）都使判定拒绝并注入
//!    ESILENT 语义码——静默路径必被抓，不允许"发现了却没入账"旁路存活；
//! 4. 判定次序冻结：空引用 → 未登记资产（ENOENT）→ 类别越闭集
//!    （EINVAL）→ 静默（ESILENT），四道闸依序短路，次序本身是验收对象；
//! 5. 注入器三类（静默未登记/类别越闭集/三态倒退）各 10 轮全抓：每轮
//!    人为构造违约路径，判定器或状态机必须显性拒绝，逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE，
//!    指纹用 FNV-1a 确定性散列）；取值一律 match/if let/.get()，
//!    无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12772 − 12600) = 0x80000 + 0x2B00）。
pub const SEG_BASE: u32 = 0x82B00;

// ── 文档差异列账诊断码闭集 ───────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 资产未登记（ENOENT 语义）：资产引用不在名册内，判定拒绝。
    ENoAsset,
    /// 差异类别越闭集（EINVAL 语义）：类别序不在五类闭集内。
    EKindRange,
    /// 发现停留未登记（静默）：登记表存在 Discovered 态挂账，零静默红线。
    ESilent,
    /// 三态倒退：已销账行试图退回已登记/发现态，状态机显性拒。
    ERewind,
    /// 重复登记：差异号错位重复或资产名册重复登记。
    EDup,
    /// 资产引用为空：空串引用不构成可判定资产。
    EEmptyRef,
    /// 登记表容量耗尽：超过 DIFF_CAP 后一切新登记拒绝。
    ECap,
    /// 列账已封账：封账后写入拒绝，改动走下一轮滚动。
    EFrozen,
    /// 差异号不存在：按差异号寻行落空。
    ENoRow,
    /// 销账前未登记：发现态越级直取销账，跳过登记相位。
    ENotLedgered,
    /// 推进相位非法：同级重复推进或提案行相位错位。
    EOrder,
    /// 双跑指纹不一致：两次全流程指纹不恒等，确定性失效。
    EFingerprint,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoAsset => 0x01,
                Diag::EKindRange => 0x02,
                Diag::ESilent => 0x03,
                Diag::ERewind => 0x04,
                Diag::EDup => 0x05,
                Diag::EEmptyRef => 0x06,
                Diag::ECap => 0x07,
                Diag::EFrozen => 0x08,
                Diag::ENoRow => 0x09,
                Diag::ENotLedgered => 0x0a,
                Diag::EOrder => 0x0b,
                Diag::EFingerprint => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoAsset => "资产未登记（ENOENT）：先走资产名册登记再判定差异，幽灵资产一律拒收",
            Diag::EKindRange => "差异类别越闭集（EINVAL）：只认语义漂移/章节缺失/版本错位/口径冲突/交叉引用断裂五类，先对类目表",
            Diag::ESilent => "发现停留未登记（静默）：先把挂账的 Discovered 行推进到已登记再谈新发现，发现即登记零静默",
            Diag::ERewind => "三态倒退：发现→登记→销账只进不退，倒退即列账可疑，回查该差异号全过程",
            Diag::EDup => "重复登记：差异号/资产各只落一笔，重复先查已有登记再入账",
            Diag::EEmptyRef => "资产引用为空：空串不构成资产，先补齐文档路径引用",
            Diag::ECap => "登记表容量耗尽：已达 DIFF_CAP，先销账归档旧差异腾位再登记新差异",
            Diag::EFrozen => "列账已封账：封账后写入拒绝，改动走下一轮滚动列账",
            Diag::ENoRow => "差异号不存在：按号寻行落空，先核对差异号再推进",
            Diag::ENotLedgered => "销账前未登记：发现态不许越级直取销账，先走登记相位",
            Diag::EOrder => "推进相位非法：同级不许重复推进，提案行必须持发现态，先核相位再动",
            Diag::EFingerprint => "双跑指纹不一致：两次全流程指纹不恒等，确定性链路断裂须整表复核",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DIAG_ALL: &[Diag] = &[
    Diag::ENoAsset,
    Diag::EKindRange,
    Diag::ESilent,
    Diag::ERewind,
    Diag::EDup,
    Diag::EEmptyRef,
    Diag::ECap,
    Diag::EFrozen,
    Diag::ENoRow,
    Diag::ENotLedgered,
    Diag::EOrder,
    Diag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DIAG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82B01..0x82B0C 闭集内"
}

// ── 差异类别闭集（五类，冻结不许重排） ───────────────────────────
/// 文档差异类别闭集：判据"文档行为面"的类目表，越类即 EINVAL。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// 第 0 类：语义漂移（同一句话两机读出不同行为面）。
    SemanticDrift,
    /// 第 1 类：章节缺失（一方文档缺整段行为约定）。
    SectionMissing,
    /// 第 2 类：版本错位（两机引用的文档版本号不一致）。
    VersionSkew,
    /// 第 3 类：口径冲突（同名条目两机口径相反，CONFLICT 显式来源）。
    TermConflict,
    /// 第 4 类：交叉引用断裂（文档内引用指向不存在条目）。
    XrefBroken,
}

impl DiffKind {
    /// 类别序（闭集序，判定器按序收口）。
    pub fn index(self) -> u32 {
        match self {
            DiffKind::SemanticDrift => 0,
            DiffKind::SectionMissing => 1,
            DiffKind::VersionSkew => 2,
            DiffKind::TermConflict => 3,
            DiffKind::XrefBroken => 4,
        }
    }

    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            DiffKind::SemanticDrift => "语义漂移",
            DiffKind::SectionMissing => "章节缺失",
            DiffKind::VersionSkew => "版本错位",
            DiffKind::TermConflict => "口径冲突",
            DiffKind::XrefBroken => "交叉引用断裂",
        }
    }

    /// 由类别序反查闭集类（越界返回 None，即 EINVAL 来源）。
    pub fn from_index(idx: u32) -> Option<DiffKind> {
        match idx {
            0 => Some(DiffKind::SemanticDrift),
            1 => Some(DiffKind::SectionMissing),
            2 => Some(DiffKind::VersionSkew),
            3 => Some(DiffKind::TermConflict),
            4 => Some(DiffKind::XrefBroken),
            _ => None,
        }
    }
}

/// 五类闭集数组（类序即判定序）。
pub const KIND_ALL: &[DiffKind] = &[
    DiffKind::SemanticDrift,
    DiffKind::SectionMissing,
    DiffKind::VersionSkew,
    DiffKind::TermConflict,
    DiffKind::XrefBroken,
];

// ── 三态闭集状态机 ───────────────────────────────────────────────
/// 差异三态闭集：Discovered → Ledgered → Resolved，只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffState {
    /// 发现未登记：已发现挂账但尚未入登记表（静默红线盯防态）。
    Discovered,
    /// 已登记：差异号已入登记表，留痕完成。
    Ledgered,
    /// 已销账：差异闭环（两机口径一致或 CONFLICT 显式入账）。
    Resolved,
}

impl DiffState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            DiffState::Discovered => 0,
            DiffState::Ledgered => 1,
            DiffState::Resolved => 2,
        }
    }

    /// 状态中文名。
    pub fn name(self) -> &'static str {
        match self {
            DiffState::Discovered => "发现未登记",
            DiffState::Ledgered => "已登记",
            DiffState::Resolved => "已销账",
        }
    }
}

// ── 差异行数据模型 ───────────────────────────────────────────────
/// 登记表容量：容量耗尽显性拒绝（ECap），不许静默截断。
pub const DIFF_CAP: usize = 12;

/// 资产名册容量。
pub const ASSET_CAP: usize = 8;

/// 单条差异行：差异号/资产引用/差异类别/三态相位/登记证据码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    /// 差异号（单调发生器签发，账内唯一）。
    pub diff_no: u32,
    /// 资产引用（文档路径，必须已在名册登记）。
    pub asset: String,
    /// 差异类别序（五类闭集内）。
    pub kind: u32,
    /// 三态相位。
    pub state: DiffState,
    /// 登记证据码（由资产+类别确定性派生，非零）。
    pub evidence: u32,
}

impl DiffRow {
    /// 类别中文名（越闭集回落"?"）。
    pub fn kind_name(&self) -> &'static str {
        match DiffKind::from_index(self.kind) {
            Some(k) => k.name(),
            None => "?",
        }
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "差异#{} {} 类别{} {}",
            self.diff_no,
            self.asset,
            self.kind_name(),
            self.state.name()
        )
    }
}

// ── 差异登记表账本 ───────────────────────────────────────────────
/// 文档差异列账账本：资产名册 + 差异登记表 + 单调差异号发生器 + 封账位。
#[derive(Clone, Debug)]
pub struct DiffLedger {
    /// 已登记资产名册（引用字符串，按登记序）。
    assets: Vec<String>,
    /// 差异登记表（按登记序）。
    rows: Vec<DiffRow>,
    /// 下一差异号（单调递增，只发不放）。
    next_no: u32,
    /// 封账位：封账后一切写入拒绝。
    sealed: bool,
}

impl DiffLedger {
    /// 开一本新列账。
    pub fn new() -> DiffLedger {
        DiffLedger {
            assets: Vec::new(),
            rows: Vec::new(),
            next_no: 1,
            sealed: false,
        }
    }

    /// 是否已封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 下一差异号（预览，不消耗）。
    pub fn next_no(&self) -> u32 {
        self.next_no
    }

    /// 名册资产数。
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    /// 登记表行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 按差异号寻行（落空 None）。
    pub fn row_of(&self, diff_no: u32) -> Option<DiffRow> {
        for r in self.rows.iter() {
            if r.diff_no == diff_no {
                return Some(r.clone());
            }
        }
        None
    }

    /// 资产是否已在名册。
    pub fn has_asset(&self, name: &str) -> bool {
        for a in self.assets.iter() {
            if a == name {
                return true;
            }
        }
        false
    }

    /// 静默扫描：返回全部停留 Discovered 态的差异号（零静默盯防口）。
    pub fn pending_discovered(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in self.rows.iter() {
            if r.state == DiffState::Discovered {
                out.push(r.diff_no);
            }
        }
        out
    }

    /// 资产名册登记：空引用拒、重复拒、名册满拒。
    pub fn register_asset(&mut self, name: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(Diag::EFrozen.val());
        }
        if name.is_empty() {
            return Err(Diag::EEmptyRef.val());
        }
        if self.has_asset(name) {
            return Err(Diag::EDup.val());
        }
        if self.assets.len() >= ASSET_CAP {
            return Err(Diag::ECap.val());
        }
        self.assets.push(name.to_string());
        Ok((self.assets.len() as u32) - 1)
    }

    /// 差异判定器（判据核心口）：四道闸依序短路，次序冻结——
    ///   ① 空引用 → EEmptyRef；
    ///   ② 未登记资产 → ENoAsset（ENOENT）；
    ///   ③ 类别越闭集 → EKindRange（EINVAL）；
    ///   ④ 登记表存在 Discovered 态挂账（静默）→ ESilent。
    /// 全过则签发发现态提案行（发现即登记协议的第一步）。
    pub fn judge_diff(&self, asset: &str, kind: u32) -> Result<DiffRow, u32> {
        // 闸①：空引用最先拒（无论资产/类别是否合法，次序即契约）。
        if asset.is_empty() {
            return Err(Diag::EEmptyRef.val());
        }
        // 闸②：资产必须在名册（ENOENT）。
        if !self.has_asset(asset) {
            return Err(Diag::ENoAsset.val());
        }
        // 闸③：类别必须落五类闭集（EINVAL）。
        if DiffKind::from_index(kind).is_none() {
            return Err(Diag::EKindRange.val());
        }
        // 闸④：零静默——任何发现未登记挂账都封锁新判定。
        if !self.pending_discovered().is_empty() {
            return Err(Diag::ESilent.val());
        }
        Ok(DiffRow {
            diff_no: self.next_no,
            asset: asset.to_string(),
            kind,
            state: DiffState::Discovered,
            evidence: evidence_of(asset, kind),
        })
    }

    /// 登记入表：把判定签发的提案行落进登记表（保持发现态，
    /// 协议要求随即推进到已登记；提案相位错位/号错位/越容/封账均拒）。
    pub fn record(&mut self, row: DiffRow) -> Result<u32, u32> {
        if self.sealed {
            return Err(Diag::EFrozen.val());
        }
        if self.rows.len() >= DIFF_CAP {
            return Err(Diag::ECap.val());
        }
        if row.diff_no != self.next_no {
            return Err(Diag::EDup.val());
        }
        if row.state != DiffState::Discovered {
            return Err(Diag::EOrder.val());
        }
        if row.asset.is_empty() {
            return Err(Diag::EEmptyRef.val());
        }
        let no = row.diff_no;
        self.rows.push(row);
        // 差异号只进不退（saturating 兜底，正常流恒递增 1）。
        self.next_no = self.next_no.saturating_add(1);
        Ok(no)
    }

    /// 静默旁路（演练/注入专用）：发现挂账但不入登记流程，
    /// 人为制造"停留 Discovered 态未登记"的静默路径——判定器必须抓住。
    pub fn park_discovered(&mut self, row: DiffRow) -> Result<u32, u32> {
        if self.sealed {
            return Err(Diag::EFrozen.val());
        }
        if self.rows.len() >= DIFF_CAP {
            return Err(Diag::ECap.val());
        }
        if row.diff_no != self.next_no {
            return Err(Diag::EDup.val());
        }
        if row.state != DiffState::Discovered {
            return Err(Diag::EOrder.val());
        }
        let no = row.diff_no;
        self.rows.push(row);
        self.next_no = self.next_no.saturating_add(1);
        Ok(no)
    }

    /// 三态推进：只进不退、同级不重推、不许越级销账。
    pub fn advance(&mut self, diff_no: u32, to: DiffState) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::EFrozen.val());
        }
        // 寻行：落空即 ENoRow。
        let mut hit: Option<usize> = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.diff_no == diff_no {
                        hit = Some(i);
                    }
                }
                None => return Err(Diag::ENoRow.val()),
            }
            i += 1;
        }
        let idx = match hit {
            Some(k) => k,
            None => return Err(Diag::ENoRow.val()),
        };
        // 取出当前相位（用 get，不裸下标）。
        let cur = match self.rows.get(idx) {
            Some(r) => r.state,
            None => return Err(Diag::ENoRow.val()),
        };
        // 倒退闸：目标相位序低于当前相位序即倒退。
        if to.rank() < cur.rank() {
            return Err(Diag::ERewind.val());
        }
        // 同级闸：重复推进同一相位。
        if to.rank() == cur.rank() {
            return Err(Diag::EOrder.val());
        }
        // 越级闸：发现态直取销账，跳过登记相位。
        if cur == DiffState::Discovered && to == DiffState::Resolved {
            return Err(Diag::ENotLedgered.val());
        }
        // 就地改相位（get_mut，不裸下标）。
        match self.rows.get_mut(idx) {
            Some(r) => r.state = to,
            None => return Err(Diag::ENoRow.val()),
        }
        Ok(())
    }

    /// 封账：零静默前置（无 Discovered 挂账才许封），封后只读。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::EFrozen.val());
        }
        if !self.pending_discovered().is_empty() {
            return Err(Diag::ESilent.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// FNV-1a 双跑指纹：覆盖名册/登记表/差异号发生器/封账位，
    /// 确定性散列——两次等价全流程指纹必须恒等。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET;
        for a in self.assets.iter() {
            h = fnv1a(h, a.as_bytes());
            h = fnv1a(h, &[0x7c]);
        }
        for r in self.rows.iter() {
            h = fnv1a(h, &r.diff_no.to_le_bytes());
            h = fnv1a(h, r.asset.as_bytes());
            h = fnv1a(h, &r.kind.to_le_bytes());
            h = fnv1a(h, &[r.state.rank()]);
            h = fnv1a(h, &r.evidence.to_le_bytes());
        }
        h = fnv1a(h, &self.next_no.to_le_bytes());
        h = fnv1a(h, &[if self.sealed { 1 } else { 0 }]);
        h
    }
}

// ── 确定性散列与 LCG（种子 = SEG_BASE，零随机红线合规） ──────────
/// FNV-1a 64 位偏移基（散列标准常数）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数（散列标准常数）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单步折叠（逐字节，不裸下标）。
fn fnv1a(init: u64, bytes: &[u8]) -> u64 {
    let mut h = init;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 登记证据码：由资产引用+类别确定性派生，恒非零（证据码为零即空证）。
fn evidence_of(asset: &str, kind: u32) -> u32 {
    let h = fnv1a(FNV_OFFSET, asset.as_bytes()) ^ (kind as u64).wrapping_mul(0x9e37_79b9);
    (h as u32) | 1
}

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

// ── 语料与全流程构造 ─────────────────────────────────────────────
/// 资产引用语料（文档行为面三个冻结引用，不许重排）。
pub const ASSET_CORPUS: &[&str] = &[
    "doc://boot/handoff",
    "doc://sched/quantum",
    "doc://mm/maplayout",
];

/// 按语料把资产名册建满（失败即中断，断言面会红）。
fn build_assets(l: &mut DiffLedger) -> Result<(), u32> {
    for name in ASSET_CORPUS.iter() {
        l.register_asset(name)?;
    }
    Ok(())
}

/// 从语料取第 ai 条资产引用（越界回落首条）。
fn corpus_asset(ai: usize) -> &'static str {
    match ASSET_CORPUS.get(ai) {
        Some(s) => s,
        None => match ASSET_CORPUS.first() {
            Some(s) => s,
            None => "",
        },
    }
}

/// 全流程构造（双跑一致的对象）：登记名册 → 逐笔"判定→登记→销账"
/// 闭环 scenarios 笔（类别由固定 LCG 抖动）→ 封账。
pub fn protocol_run(scenarios: u32) -> Result<DiffLedger, u32> {
    let mut l = DiffLedger::new();
    build_assets(&mut l)?;
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(scenarios as u64));
    let mut done = 0u32;
    while done < scenarios {
        let ai = rng.below(ASSET_CORPUS.len() as u32) as usize;
        let asset = corpus_asset(ai);
        let kind = rng.below(KIND_ALL.len() as u32);
        let row = l.judge_diff(asset, kind)?;
        let no = l.record(row)?;
        l.advance(no, DiffState::Ledgered)?;
        l.advance(no, DiffState::Resolved)?;
        done = done.saturating_add(1);
    }
    l.seal()?;
    Ok(l)
}

// ── 静默演练：10/10 检出（任何发现未登记路径必被抓） ─────────────
/// 静默演练 10 场景：每场景旁路挂一条 Discovered 未登记行，
/// 静默扫描必须找到它、判定器必须拒绝新发现——两条检出面缺一即逃脱。
/// 返回 (检出场景数, 总场景数, 首个逃脱描述)。
pub fn silence_drill() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x77));
    let mut k = 0u32;
    while k < 10 {
        let mut l = DiffLedger::new();
        if build_assets(&mut l).is_err() {
            // 名册建不起来：场景直接算逃脱（检出面失效）。
            total += 1;
            if escape.is_empty() {
                escape = format!("场景{}：名册建立失败", k);
            }
            k = k.saturating_add(1);
            continue;
        }
        let ai = rng.below(ASSET_CORPUS.len() as u32) as usize;
        let asset = corpus_asset(ai);
        let kind = rng.below(KIND_ALL.len() as u32);
        let parked = match l.judge_diff(asset, kind) {
            Ok(row) => match l.park_discovered(row) {
                Ok(no) => no,
                Err(e) => {
                    total += 1;
                    if escape.is_empty() {
                        escape = format!("场景{}：旁路挂账被拒 0x{:x}", k, e);
                    }
                    k = k.saturating_add(1);
                    continue;
                }
            },
            Err(e) => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("场景{}：首判定被拒 0x{:x}", k, e);
                }
                k = k.saturating_add(1);
                continue;
            }
        };
        // 检出面一：静默扫描必须恰好找到挂账差异号。
        let scan_hit = {
            let pend = l.pending_discovered();
            pend.len() == 1 && match pend.first() {
                Some(n) => *n == parked,
                None => false,
            }
        };
        // 检出面二：判定器必须拒绝新发现（ESilent）。
        let judge_hit = l.judge_diff(asset, kind) == Err(Diag::ESilent.val());
        total += 1;
        if scan_hit && judge_hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("场景{}：扫描{}判定{}", k, scan_hit, judge_hit);
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

// ── 注入器一：静默未登记注入 10 轮 ───────────────────────────────
/// 每轮旁路挂一条静默行后尝试新判定，判定器必须以 ESilent 拒绝。
/// 返回 (抓到轮数, 总轮数, 首个逃脱描述)。
pub fn inject_silent_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x511));
    let mut k = 0u32;
    while k < 10 {
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        let ai = rng.below(ASSET_CORPUS.len() as u32) as usize;
        let asset = corpus_asset(ai);
        let kind = rng.below(KIND_ALL.len() as u32);
        if let Ok(row) = l.judge_diff(asset, kind) {
            let _ = l.park_discovered(row);
        }
        // 换一条语料再判：静默闸必须拦下（与语料无关，任何发现都拦）。
        let ai2 = (ai + 1) % ASSET_CORPUS.len();
        let kind2 = (kind + 1) % KIND_ALL.len() as u32;
        let verdict = l.judge_diff(corpus_asset(ai2), kind2);
        total += 1;
        if verdict == Err(Diag::ESilent.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}：静默注入逃脱 0x{:x}", k, match verdict {
                Ok(_) => 0,
                Err(e) => e,
            });
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

// ── 注入器二：类别越闭集注入 10 轮 ───────────────────────────────
/// 每轮以闭集外类别（5..21 抖动）判定，判定器必须以 EKindRange 拒绝
/// 且不产生任何登记行、不消耗差异号。
/// 返回 (抓到轮数, 总轮数, 首个逃脱描述)。
pub fn inject_kind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x622));
    let mut k = 0u32;
    while k < 10 {
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        let ai = rng.below(ASSET_CORPUS.len() as u32) as usize;
        let asset = corpus_asset(ai);
        let bad_kind = DIFF_CAP as u32 + rng.below(16);
        let verdict = l.judge_diff(asset, bad_kind);
        total += 1;
        let rows_before = l.row_count();
        let no_before = l.next_no();
        if verdict == Err(Diag::EKindRange.val())
            && l.row_count() == rows_before
            && l.next_no() == no_before
        {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}：越类注入逃脱 类别{}", k, bad_kind);
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

// ── 注入器三：三态倒退注入 10 轮 ─────────────────────────────────
/// 每轮把一笔闭环到销账的差异行试图倒退回已登记/发现态，
/// 状态机必须以 ERewind 双路拒绝。
/// 返回 (抓到轮数, 总轮数, 首个逃脱描述)。
pub fn inject_rewind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new((SEG_BASE as u64).wrapping_add(0x733));
    let mut k = 0u32;
    while k < 10 {
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        let ai = rng.below(ASSET_CORPUS.len() as u32) as usize;
        let asset = corpus_asset(ai);
        let kind = rng.below(KIND_ALL.len() as u32);
        let no = match l.judge_diff(asset, kind) {
            Ok(row) => match l.record(row) {
                Ok(n) => n,
                Err(e) => {
                    total += 1;
                    if escape.is_empty() {
                        escape = format!("轮{}：登记被拒 0x{:x}", k, e);
                    }
                    k = k.saturating_add(1);
                    continue;
                }
            },
            Err(e) => {
                total += 1;
                if escape.is_empty() {
                    escape = format!("轮{}：判定被拒 0x{:x}", k, e);
                }
                k = k.saturating_add(1);
                continue;
            }
        };
        let _ = l.advance(no, DiffState::Ledgered);
        let _ = l.advance(no, DiffState::Resolved);
        // 倒退双路：销账→已登记、销账→发现，都必须 ERewind。
        let r1 = l.advance(no, DiffState::Ledgered);
        let r2 = l.advance(no, DiffState::Discovered);
        total += 1;
        if r1 == Err(Diag::ERewind.val()) && r2 == Err(Diag::ERewind.val()) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{}：倒退注入逃脱", k);
        }
        k = k.saturating_add(1);
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 全批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗口、互异、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    for d in DIAG_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 互异：两两不同（不裸下标，双循环 get）。
    let mut i = 0usize;
    while i < DIAG_ALL.len() {
        let vi = match DIAG_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < DIAG_ALL.len() {
            let vj = match DIAG_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        let dt = match DIAG_ALL.get(i) {
            Some(d) => d.detail(),
            None => "",
        };
        if detail(vi) != dt {
            return false;
        }
        i += 1;
    }
    // 未知码回落。
    if detail(SEG_BASE + 0x30).contains("已知") {
        return false;
    }
    if !detail(SEG_BASE + 0x30).contains("未知") {
        return false;
    }
    true
}

/// 类别闭集守卫：五类齐、类序回环、越类落空、名非空。
fn kind_closed_ok() -> bool {
    if KIND_ALL.len() != 5 {
        return false;
    }
    for i in 0..5u32 {
        match DiffKind::from_index(i) {
            Some(k) => {
                if k.index() != i || k.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        // 闭集数组第 i 位必须正是类序 i 的类。
        match KIND_ALL.get(i as usize) {
            Some(k) => {
                if k.index() != i {
                    return false;
                }
            }
            None => return false,
        }
    }
    // 越类落空（5..9 全 None）。
    for bad in 5..9u32 {
        if DiffKind::from_index(bad).is_some() {
            return false;
        }
    }
    true
}

/// 三态单调守卫：rank 严格递增，相位名非空。
fn state_mono_ok() -> bool {
    let d = DiffState::Discovered.rank();
    let l = DiffState::Ledgered.rank();
    let r = DiffState::Resolved.rank();
    if !(d < l && l < r) {
        return false;
    }
    for s in [DiffState::Discovered, DiffState::Ledgered, DiffState::Resolved].iter() {
        if s.name().is_empty() {
            return false;
        }
    }
    true
}

/// 正常流：名册→逐笔判定/登记/销账闭环→封账→指纹非零。
fn happy_path_ok() -> bool {
    let l = match protocol_run(6) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if !l.sealed() || l.row_count() != 6 {
        return false;
    }
    if !l.pending_discovered().is_empty() {
        return false;
    }
    // 逐行核相位：全部 Resolved。
    let mut i = 1u32;
    while i <= 6 {
        match l.row_of(i) {
            Some(r) => {
                if r.state != DiffState::Resolved || r.evidence == 0 {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    l.fingerprint() != 0
}

/// 判定次序冻结：空引用 → 未登记 → 越类 → 静默，四闸依序短路。
fn judge_order_ok() -> bool {
    let mut l = DiffLedger::new();
    let _ = build_assets(&mut l);
    // 闸①压全：空引用 + 幽灵资产 + 越类，仍必须先报空引用。
    if l.judge_diff("", 99) != Err(Diag::EEmptyRef.val()) {
        return false;
    }
    // 闸②压③④：幽灵资产 + 越类，必须先报 ENOENT。
    if l.judge_diff("doc://ghost/none", 99) != Err(Diag::ENoAsset.val()) {
        return false;
    }
    // 闸③压④：合法资产 + 越类，必须先报 EINVAL（静默闸在后）。
    if l.judge_diff(corpus_asset(0), 99) != Err(Diag::EKindRange.val()) {
        return false;
    }
    // 挂一条静默行后再核闸④压底：合法资产+合法类别 → ESilent。
    if let Ok(row) = l.judge_diff(corpus_asset(1), 0) {
        let _ = l.park_discovered(row);
    } else {
        return false;
    }
    if l.judge_diff(corpus_asset(1), 0) != Err(Diag::ESilent.val()) {
        return false;
    }
    // 越级闸：发现态直取销账 → ENotLedgered（先寻出挂账号）。
    let pend = l.pending_discovered();
    let no = match pend.first() {
        Some(n) => *n,
        None => return false,
    };
    if l.advance(no, DiffState::Resolved) != Err(Diag::ENotLedgered.val()) {
        return false;
    }
    // 同级闸：重复登记同一相位 → EOrder。
    if l.advance(no, DiffState::Discovered) != Err(Diag::EOrder.val()) {
        return false;
    }
    // 寻行落空：不存在的差异号 → ENoRow。
    if l.advance(no + 100, DiffState::Ledgered) != Err(Diag::ENoRow.val()) {
        return false;
    }
    true
}

/// 四类边界：空引用 / 幽灵资产 / 重复登记 / 封账后写入（含提案错位）。
fn boundary_ok() -> bool {
    let mut l = DiffLedger::new();
    let _ = build_assets(&mut l);
    // 边界①：空资产引用登记被拒。
    let mut l1 = DiffLedger::new();
    if l1.register_asset("") != Err(Diag::EEmptyRef.val()) {
        return false;
    }
    // 边界②：幽灵资产判定被拒（ENOENT）。
    if l.judge_diff("doc://ghost/none", 0) != Err(Diag::ENoAsset.val()) {
        return false;
    }
    // 边界③：重复登记同一资产被拒。
    if l.register_asset(corpus_asset(0)) != Err(Diag::EDup.val()) {
        return false;
    }
    // 边界④：封账后一切写入拒绝（登记/判定走签名不受封账锁，
    // 但 record/advance/register 全锁）。
    let row = match l.judge_diff(corpus_asset(2), 1) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let no = match l.record(row) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let _ = l.advance(no, DiffState::Ledgered);
    let _ = l.advance(no, DiffState::Resolved);
    if l.seal().is_err() {
        return false;
    }
    if l.register_asset("doc://new/after") != Err(Diag::EFrozen.val()) {
        return false;
    }
    if l.seal() != Err(Diag::EFrozen.val()) {
        return false;
    }
    if l.advance(no, DiffState::Ledgered) != Err(Diag::EFrozen.val()) {
        return false;
    }
    // 补充边界：提案相位错位（持已登记态的提案行）被拒。
    let mut l2 = DiffLedger::new();
    let _ = build_assets(&mut l2);
    let mut forged = match l2.judge_diff(corpus_asset(0), 2) {
        Ok(r) => r,
        Err(_) => return false,
    };
    forged.state = DiffState::Ledgered;
    if l2.record(forged) != Err(Diag::EOrder.val()) {
        return false;
    }
    true
}

/// 静默演练 10/10 检出。
fn silence_drill_ok() -> bool {
    let (caught, total, escape) = silence_drill();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 注入一：静默未登记 10 轮全抓。
fn inject_silent_ok() -> bool {
    let (caught, total, escape) = inject_silent_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入二：类别越闭集 10 轮全抓。
fn inject_kind_ok() -> bool {
    let (caught, total, escape) = inject_kind_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入三：三态倒退 10 轮全抓。
fn inject_rewind_ok() -> bool {
    let (caught, total, escape) = inject_rewind_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 双跑指纹核对口：两次全流程指纹恒等，不等即注入 EFingerprint 语义码。
pub fn dual_fingerprint_check() -> Result<u64, u32> {
    let a = protocol_run(6)?;
    let b = protocol_run(6)?;
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    if fa != fb {
        return Err(Diag::EFingerprint.val());
    }
    Ok(fa)
}

/// 双跑一致指纹：两次全流程指纹恒等 + 三注入器序列恒等。
fn dual_run_ok() -> bool {
    let fp = match dual_fingerprint_check() {
        Ok(f) => f,
        Err(_) => return false,
    };
    if fp == 0 {
        return false;
    }
    let a = protocol_run(6);
    let b = protocol_run(6);
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x.row_count() != y.row_count() || x.sealed() != y.sealed() {
                return false;
            }
        }
        _ => return false,
    }
    if silence_drill() != silence_drill() {
        return false;
    }
    if inject_silent_sweep() != inject_silent_sweep() {
        return false;
    }
    if inject_kind_sweep() != inject_kind_sweep() {
        return false;
    }
    if inject_rewind_sweep() != inject_rewind_sweep() {
        return false;
    }
    true
}

/// 容量耗尽：DIFF_CAP 笔闭环占满后，第 DIFF_CAP+1 笔登记显性拒。
fn capacity_ok() -> bool {
    let mut l = DiffLedger::new();
    let _ = build_assets(&mut l);
    let mut done = 0usize;
    while done < DIFF_CAP {
        let ai = done % ASSET_CORPUS.len();
        let kind = (done % KIND_ALL.len()) as u32;
        let row = match l.judge_diff(corpus_asset(ai), kind) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let no = match l.record(row) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if l.advance(no, DiffState::Ledgered).is_err() {
            return false;
        }
        if l.advance(no, DiffState::Resolved).is_err() {
            return false;
        }
        done += 1;
    }
    let rows_full = l.row_count();
    // 第 13 笔：判定仍可通过（无静默挂账），登记必须 ECap 且账面不变。
    let extra = match l.judge_diff(corpus_asset(0), 0) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if l.record(extra) != Err(Diag::ECap.val()) {
        return false;
    }
    if l.row_count() != rows_full || l.row_count() != DIFF_CAP {
        return false;
    }
    true
}

/// 收口判据：前序判据全绿且条数恰为声明值。
fn closing_ok(passed: usize, failed: usize, declared: usize) -> bool {
    failed == 0 && passed == declared
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12772 自检（判据 12772 卡：文档差异列账 · 发现即登记零静默）。
pub fn run_unx12772_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12772");
    let ledger = match protocol_run(6) {
        Ok(l) => l,
        Err(_) => DiffLedger::new(),
    };

    s.add_dyn(
        "F12772-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12772-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            DIAG_ALL.len(),
            DIAG_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12772-03 差异类别闭集",
        kind_closed_ok(),
        &format!(
            "kinds={}首={}尾={}",
            KIND_ALL.len(),
            KIND_ALL.first().map(|k| k.name()).unwrap_or("?"),
            KIND_ALL.last().map(|k| k.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12772-04 三态单调守卫",
        state_mono_ok(),
        &format!(
            "rank={}<{}<{}",
            DiffState::Discovered.rank(),
            DiffState::Ledgered.rank(),
            DiffState::Resolved.rank()
        ),
    );

    s.add_dyn(
        "F12772-05 正常流闭环封账",
        happy_path_ok(),
        &format!(
            "rows={} sealed={} fp={:016x}",
            ledger.row_count(),
            ledger.sealed(),
            ledger.fingerprint()
        ),
    );

    s.add_dyn(
        "F12772-06 判定次序冻结",
        judge_order_ok(),
        "空引用→未登记→越类→静默 四闸依序短路全验",
    );

    s.add_dyn(
        "F12772-07 四类边界",
        boundary_ok(),
        "空引用/幽灵资产/重复登记/封账后写 四点全拒",
    );

    let (sc, st, se) = silence_drill();
    s.add_dyn(
        "F12772-08 静默演练 10/10 检出",
        silence_drill_ok() && sc == 10 && st == 10 && se.is_empty(),
        &format!("caught={}/{} {}", sc, st, se),
    );

    let (c1, t1, e1) = inject_silent_sweep();
    s.add_dyn(
        "F12772-09 注入一 静默未登记 10 轮",
        inject_silent_ok() && c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_kind_sweep();
    s.add_dyn(
        "F12772-10 注入二 类别越闭集 10 轮",
        inject_kind_ok() && c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_rewind_sweep();
    s.add_dyn(
        "F12772-11 注入三 三态倒退 10 轮",
        inject_rewind_ok() && c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12772-12 双跑一致指纹",
        dual_run_ok(),
        &format!("fp={:016x}", ledger.fingerprint()),
    );

    s.add_dyn(
        "F12772-13 容量耗尽显性拒",
        capacity_ok(),
        &format!("cap={} 13笔拒ECap", DIFF_CAP),
    );

    // 收口：判据条数声明——前序 13 条全绿才许收口。
    let (passed, failed) = s.tally();
    s.add_dyn(
        "F12772-14 收口判据条数声明",
        closing_ok(passed, failed, 13),
        &format!("判据13条 全绿{} 红{}", passed, failed),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82B00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中、未知码回落。
        assert_eq!(DIAG_ALL.len(), 12);
        let mut i = 0usize;
        while i < DIAG_ALL.len() {
            let vi = DIAG_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < DIAG_ALL.len() {
                let vj = DIAG_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert_eq!(detail(Diag::ESilent.val()), Diag::ESilent.detail());
    }

    #[test]
    fn happy_path_full_cycle() {
        // 正常流：判定→登记→登记态→销账态→封账，逐相位核对。
        let mut l = DiffLedger::new();
        assert!(build_assets(&mut l).is_ok());
        assert_eq!(l.asset_count(), 3);
        let row = l.judge_diff(corpus_asset(0), 2).expect("判定须过");
        assert_eq!(row.state, DiffState::Discovered);
        assert_eq!(row.diff_no, 1);
        assert_ne!(row.evidence, 0);
        let no = l.record(row).expect("登记须过");
        assert_eq!(no, 1);
        assert_eq!(l.next_no(), 2);
        assert!(l.advance(no, DiffState::Ledgered).is_ok());
        assert_eq!(l.row_of(no).map(|r| r.state), Some(DiffState::Ledgered));
        assert!(l.advance(no, DiffState::Resolved).is_ok());
        assert_eq!(l.row_of(no).map(|r| r.state), Some(DiffState::Resolved));
        assert!(l.seal().is_ok());
        assert!(l.sealed());
        assert!(happy_path_ok());
    }

    #[test]
    fn judge_order_is_frozen() {
        // 判定次序：四闸依序短路，越级/同级/落空三路相位守卫。
        assert!(judge_order_ok());
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        // 空引用压全（资产与类别同时非法仍先报空引用）。
        assert_eq!(l.judge_diff("", 0), Err(Diag::EEmptyRef.val()));
        // 幽灵资产压越类。
        assert_eq!(l.judge_diff("doc://x", 42), Err(Diag::ENoAsset.val()));
        // 越类压静默。
        assert_eq!(l.judge_diff(corpus_asset(1), 9), Err(Diag::EKindRange.val()));
    }

    #[test]
    fn silence_drill_ten_of_ten() {
        // 静默演练：10 场景全检出，扫描面与判定面双路命中。
        let (caught, total, escape) = silence_drill();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱场景：{}", escape);
        assert!(escape.is_empty());
        assert!(silence_drill_ok());
    }

    #[test]
    fn inject_silent_all_caught() {
        // 注入一：静默未登记 10 轮全抓。
        let (caught, total, escape) = inject_silent_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(inject_silent_ok());
    }

    #[test]
    fn inject_kind_all_caught() {
        // 注入二：类别越闭集 10 轮全抓，且不产生行、不耗号。
        let (caught, total, escape) = inject_kind_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        assert_eq!(l.judge_diff(corpus_asset(0), 5), Err(Diag::EKindRange.val()));
        assert_eq!(l.row_count(), 0);
        assert_eq!(l.next_no(), 1);
        assert!(inject_kind_ok());
    }

    #[test]
    fn inject_rewind_all_caught() {
        // 注入三：三态倒退 10 轮全抓（销账→登记、销账→发现双路）。
        let (caught, total, escape) = inject_rewind_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱轮：{}", escape);
        assert!(inject_rewind_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：指纹恒等 + 行数/封账位恒等 + 注入器序列恒等。
        let a = protocol_run(6).expect("全流程须过");
        let b = protocol_run(6).expect("全流程须过");
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.row_count(), b.row_count());
        assert_eq!(a.sealed(), b.sealed());
        assert_eq!(silence_drill(), silence_drill());
        assert_eq!(inject_silent_sweep(), inject_silent_sweep());
        assert_eq!(inject_kind_sweep(), inject_kind_sweep());
        assert_eq!(inject_rewind_sweep(), inject_rewind_sweep());
        assert!(dual_run_ok());
    }

    #[test]
    fn capacity_exhaustion_explicit() {
        // 容量耗尽：12 笔占满后第 13 笔登记显性拒且账面不变。
        assert!(capacity_ok());
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        let mut done = 0usize;
        while done < DIFF_CAP {
            let row = l
                .judge_diff(corpus_asset(done % ASSET_CORPUS.len()), 0)
                .expect("判定须过");
            let no = l.record(row).expect("登记须过");
            let _ = l.advance(no, DiffState::Ledgered);
            let _ = l.advance(no, DiffState::Resolved);
            done += 1;
        }
        assert_eq!(l.row_count(), DIFF_CAP);
        let extra = l.judge_diff(corpus_asset(0), 0).expect("判定须过");
        assert_eq!(l.record(extra), Err(Diag::ECap.val()));
        assert_eq!(l.row_count(), DIFF_CAP);
    }

    #[test]
    fn boundary_gates_reject() {
        // 四类边界：空引用/幽灵资产/重复登记/封账后写 + 提案错位。
        assert!(boundary_ok());
        let mut l = DiffLedger::new();
        // 封账前置：有静默挂账不许封。
        let _ = build_assets(&mut l);
        if let Ok(row) = l.judge_diff(corpus_asset(0), 0) {
            let _ = l.park_discovered(row);
        }
        assert_eq!(l.seal(), Err(Diag::ESilent.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：发现→登记→销账严格递增。
        let d = DiffState::Discovered.rank();
        let g = DiffState::Ledgered.rank();
        let r = DiffState::Resolved.rank();
        assert!(d < g && g < r);
        let mut l = DiffLedger::new();
        let _ = build_assets(&mut l);
        let row = l.judge_diff(corpus_asset(2), 3).expect("判定须过");
        let no = l.record(row).expect("登记须过");
        // 越级销账拒。
        assert_eq!(l.advance(no, DiffState::Resolved), Err(Diag::ENotLedgered.val()));
        assert!(l.advance(no, DiffState::Ledgered).is_ok());
        // 同级重推拒。
        assert_eq!(l.advance(no, DiffState::Ledgered), Err(Diag::EOrder.val()));
        assert!(l.advance(no, DiffState::Resolved).is_ok());
        // 倒退双路拒。
        assert_eq!(l.advance(no, DiffState::Ledgered), Err(Diag::ERewind.val()));
        assert_eq!(l.advance(no, DiffState::Discovered), Err(Diag::ERewind.val()));
        assert!(state_mono_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条、收口条数声明成立。
        let s = run_unx12772_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
