//! UNX-F12771 域文档快照 SHA-256 冻结 · 0x82AC0段
//!
//! 判据原文（派工卡 12771 逐字锚定）：
//!   UNX-F12772-J1 差异列账判据绿（三态登记），静默演练 10/10 检出。
//!
//! 判据语义落点：
//!   · 「三态登记」＝每份资产快照走 草稿(Draft)→冻结(Frozen)→封版(Sealed)
//!     三态单向推进，任何倒退显性拒；
//!   · 「差异列账判据绿」＝四类资产快照全部冻结、在册摘要重算逐位相等、
//!     全库指纹封版锁定，列账无红项；
//!   · 「静默演练 10/10 检出」＝版本漂移注入器三路（版本篡改/摘要篡改/
//!     重复冻结）各 10 轮静默演练，冻结判定器必须全数检出，逃脱即失效。
//!
//! 设计要点：
//! 1. 四类资产快照闭集：总纲/台账/术语表/域经档，各带（资产名/版本号/
//!    摘要 digest/冻结态），登记即入槽（容量 8 槽闭集），封版即终态；
//! 2. 摘要算法：FNV-1a 64 位基座 + 自定三轮二次折叠（rotate-multiply、
//!    类别标签混入、hi/lo 交叉绑定），纯整数、零浮点、确定性、双跑逐位
//!    一致，模拟 SHA-256 冻结语义（128 位双字摘要，逐位可比）；
//! 3. 冻结判定器 judge_freeze 六层优先级：未登记→ENOENT；非草稿版本
//!    翻转→EROFS；封版重请求→倒退拒；摘要篡改→EDigest；重复冻结→
//!    EALREADY；草稿版本漂移→EVersion——次序写死，注入必按序命中；
//! 4. 版本漂移注入器三路各 10 轮：库侧/候选侧交替篡改，翻转位随轮次
//!    抖动（LCG 种子 = SEG_BASE 派生，固定可复跑），判定器必须逐轮
//!    返回指定诊断码，任何逃脱记入 ETamper；
//! 5. 冻结后 digest 只增不减声明：冻结轮次单调递增、版本号不回退、
//!    重算摘要与在册逐位相等（audit_frozen_digests 逐槽复核），
//!    任何在册摘要变动都被下一轮复核当场抓出；
//! 6. 收口三件套：容量 8 槽耗尽必拒（EFull）、段位守卫（0x40 对齐 +
//!    VX9 全窗口）、双跑一致指纹（verify_dual 逐位比对全库指纹）。
//!
//! no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//! 取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//! 无 unwrap/expect/panic/assert（测试区除外）、无裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12771 − 12600) = 0x82AC0）。
pub const SEG_BASE: u32 = 0x82AC0;

// ── 快照冻结诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapDiag {
    /// 未登记资产：冻结判定只认在册快照。
    ENoent,
    /// 重复冻结：已冻结且版本摘要一致的资产再次请求冻结。
    EAlready,
    /// 冻结后版本翻转：冻结资产的版本号只读，翻转即越权改账。
    ERofs,
    /// 摘要篡改：在册摘要与重算不符，冻结凭据失效。
    EDigest,
    /// 版本漂移：候选版本与在册不一致（篡改或越权改版）。
    EVersion,
    /// 资产名非法：空名或重名。
    EName,
    /// 资产类别越四类闭集：只收总纲/台账/术语表/域经档。
    EKind,
    /// 容量耗尽：快照库 8 槽已满。
    EFull,
    /// 冻结未齐：尚有草稿态资产禁止封版。
    ENotfull,
    /// 库已封版：封版后登记拒绝。
    ESealed,
    /// 状态机倒退：重复封版或封版后判冻。
    ERewind,
    /// 注入逃脱：静默演练有轮次未被判定器检出。
    ETamper,
    /// 双跑指纹不一致：确定性被破坏。
    EFinger,
}

impl SnapDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                SnapDiag::ENoent => 0x01,
                SnapDiag::EAlready => 0x02,
                SnapDiag::ERofs => 0x03,
                SnapDiag::EDigest => 0x04,
                SnapDiag::EVersion => 0x05,
                SnapDiag::EName => 0x06,
                SnapDiag::EKind => 0x07,
                SnapDiag::EFull => 0x08,
                SnapDiag::ENotfull => 0x09,
                SnapDiag::ESealed => 0x0a,
                SnapDiag::ERewind => 0x0b,
                SnapDiag::ETamper => 0x0c,
                SnapDiag::EFinger => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SnapDiag::ENoent => "未登记资产：冻结判定只认在册快照，先走登记通道再请求冻结",
            SnapDiag::EAlready => "重复冻结：该资产已冻结且版本摘要一致，无需再判；改动走新版本重登记",
            SnapDiag::ERofs => "冻结后版本翻转：冻结资产版本号只读，翻转即越权改账，先回滚版本再复核摘要",
            SnapDiag::EDigest => "摘要篡改：在册摘要与重算不符，冻结凭据失效，立即锁定该资产并追查写入路径",
            SnapDiag::EVersion => "版本漂移：候选版本与在册不一致，先确认是篡改还是新版本，再走对应通道",
            SnapDiag::EName => "资产名非法：空名或重名，先更名去重再登记",
            SnapDiag::EKind => "资产类别越四类闭集：只收总纲/台账/术语表/域经档，类别号 0..4",
            SnapDiag::EFull => "容量耗尽：快照库 8 槽已满，先封版归档或清退草稿再登记",
            SnapDiag::ENotfull => "冻结未齐：尚有草稿态资产，先全部冻结再封版",
            SnapDiag::ESealed => "库已封版：封版后登记拒绝，改动进入下一轮快照滚动",
            SnapDiag::ERewind => "状态机倒退：草稿→冻结→封版只进不退，重复封版或封版后判冻均拒",
            SnapDiag::ETamper => "注入逃脱：静默演练有轮次未被判定器检出，判定面失效须整库复核",
            SnapDiag::EFinger => "双跑指纹不一致：两次构建指纹逐位不等，确定性被破坏，先查种子与注入序列",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const SNAP_ALL: &[SnapDiag] = &[
    SnapDiag::ENoent,
    SnapDiag::EAlready,
    SnapDiag::ERofs,
    SnapDiag::EDigest,
    SnapDiag::EVersion,
    SnapDiag::EName,
    SnapDiag::EKind,
    SnapDiag::EFull,
    SnapDiag::ENotfull,
    SnapDiag::ESealed,
    SnapDiag::ERewind,
    SnapDiag::ETamper,
    SnapDiag::EFinger,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in SNAP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82AC1..0x82ACD 闭集内"
}

// ── 摘要算法：FNV-1a 64 位基座 + 自定二次折叠 ────────────────────
/// FNV-1a 64 位偏移基座（FNV-1a 规范：offset basis 64 位素数派生）。
const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 位素数（FNV-1a 规范：0x100000001B3）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
/// 二次折叠第三轮常数（黄金比例分数部分，整数搅拌用）。
const FOLD_GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// 128 位双字摘要（逐位可比，bitwise Eq 由 u64 派生保证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest {
    /// 高 64 位字。
    pub hi: u64,
    /// 低 64 位字。
    pub lo: u64,
}

impl Digest {
    /// 全零摘要（视为未定摘要，冻结路径禁收）。
    pub fn is_zero(&self) -> bool {
        self.hi == 0 && self.lo == 0
    }

    /// 32 位十六进制文本（诊断展示用，双跑恒等）。
    pub fn hex(&self) -> String {
        format!("{:016x}{:016x}", self.hi, self.lo)
    }
}

/// FNV-1a 64 位逐字节压叠（纯整数，字节序无关遍历）。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 域分隔前缀：类别标签 + 资产名 + 版本号（4 字节小端）。
fn digest_prefix(kind: AssetKind, name: &str, version: u32, out: &mut Vec<u8>) {
    let tag = kind.tag();
    out.push((tag & 0xFF) as u8);
    out.push(((tag >> 8) & 0xFF) as u8);
    out.push(((tag >> 16) & 0xFF) as u8);
    out.push(((tag >> 24) & 0xFF) as u8);
    out.push(0x1F);
    for b in name.as_bytes().iter() {
        out.push(*b);
    }
    out.push(0x1F);
    out.push((version & 0xFF) as u8);
    out.push(((version >> 8) & 0xFF) as u8);
    out.push(((version >> 16) & 0xFF) as u8);
    out.push(((version >> 24) & 0xFF) as u8);
    out.push(0x1E);
}

/// 摘要计算（字节体入口）：FNV-1a 64 位基座 + 自定三轮二次折叠。
///
/// 折叠语义（模拟 SHA-256 冻结：一经冻结，重算必须逐位相等）：
///   第一轮 rotate-multiply：扩散基座高位，混入版本号（防跨版本碰撞）；
///   第二轮 类别标签混入：黄金比例常数搅拌（防跨类别碰撞）；
///   第三轮 hi/lo 交叉绑定：单字节翻转同时扰动双字（雪崩面）。
pub fn digest_body(kind: AssetKind, name: &str, version: u32, body: &[u8]) -> Digest {
    let mut buf: Vec<u8> = Vec::new();
    digest_prefix(kind, name, version, &mut buf);
    for b in body.iter() {
        buf.push(*b);
    }
    let base = fnv1a64(&buf);
    // 第一轮：rotate-multiply 扩散 + 版本号混入。
    let f1 = base
        .rotate_left(23)
        .wrapping_mul(0xD6E8_FEB8_6659_FD93)
        .wrapping_add(version as u64);
    // 第二轮：类别标签做域分隔混入，黄金比例常数搅拌。
    let f2 = (f1 ^ (kind.tag() as u64).wrapping_mul(FOLD_GOLDEN))
        .rotate_left(17)
        .wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    // 第三轮：hi/lo 交叉绑定，双字同时受单字节翻转扰动。
    let f3 = (f1 ^ f2.rotate_left(31)).wrapping_add(0x1656_67B1_9E37_79F9);
    Digest {
        hi: f2 ^ f3,
        lo: f1 ^ f3.rotate_right(13),
    }
}

/// 摘要计算（正文串入口）。
pub fn digest_compute(kind: AssetKind, name: &str, version: u32, content: &str) -> Digest {
    digest_body(kind, name, version, content.as_bytes())
}

// ── 四类资产闭集 ─────────────────────────────────────────────────
/// 四类资产容量（总纲/台账/术语表/域经档）。
pub const KIND_CAP: usize = 4;

/// 四类资产闭集（判据域冻结，不许增删重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 第 0 类：总纲。
    MasterOutline,
    /// 第 1 类：台账。
    LedgerBook,
    /// 第 2 类：术语表。
    Glossary,
    /// 第 3 类：域经档。
    Chronicle,
}

impl AssetKind {
    /// 类别序号（闭集序）。
    pub fn index(self) -> u32 {
        match self {
            AssetKind::MasterOutline => 0,
            AssetKind::LedgerBook => 1,
            AssetKind::Glossary => 2,
            AssetKind::Chronicle => 3,
        }
    }

    /// 类别中文名。
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::MasterOutline => "总纲",
            AssetKind::LedgerBook => "台账",
            AssetKind::Glossary => "术语表",
            AssetKind::Chronicle => "域经档",
        }
    }

    /// 类别标签（摘要域分隔用，四类互异）。
    pub fn tag(self) -> u32 {
        match self {
            AssetKind::MasterOutline => 0x5E50_0A01,
            AssetKind::LedgerBook => 0x5E50_0B02,
            AssetKind::Glossary => 0x5E50_0C03,
            AssetKind::Chronicle => 0x5E50_0D04,
        }
    }

    /// 由序号反查闭集类别（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<AssetKind> {
        match idx {
            0 => Some(AssetKind::MasterOutline),
            1 => Some(AssetKind::LedgerBook),
            2 => Some(AssetKind::Glossary),
            3 => Some(AssetKind::Chronicle),
            _ => None,
        }
    }
}

/// 四类闭集数组（登记序即冻结演练轮转序）。
pub const KIND_ALL: &[AssetKind] = &[
    AssetKind::MasterOutline,
    AssetKind::LedgerBook,
    AssetKind::Glossary,
    AssetKind::Chronicle,
];

// ── 三态登记 ─────────────────────────────────────────────────────
/// 资产三态：草稿 → 冻结 → 封版，只进不退（判据「三态登记」锚点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    /// 草稿态：已登记未冻结，正文与版本仍可被库外注入篡改（判定器负责抓）。
    Draft,
    /// 冻结态：摘要已定，版本号只读。
    Frozen,
    /// 封版态：随全库封版进入终态，判冻通道关闭。
    Sealed,
}

impl FreezeState {
    /// 态序（只许前进，倒退显性拒）。
    pub fn rank(self) -> u8 {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Frozen => 1,
            FreezeState::Sealed => 2,
        }
    }
}

// ── 资产快照 ─────────────────────────────────────────────────────
/// 版本号上界（冻结协议版本闭集 [1, 0xFFFF]）。
pub const VERSION_MAX: u32 = 0xFFFF;
/// 快照库容量闭集（8 槽：四类正册 + 四条附录）。
pub const ASSET_CAP: usize = 8;

/// 单份资产快照：资产名/版本号/摘要 digest/冻结态由库槽持有，
/// 本体持 前三者（冻结态在 Slot 上，见 FreezeVault）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetSnapshot {
    /// 资产类别（四类闭集内）。
    pub kind: AssetKind,
    /// 资产名（库内唯一键）。
    pub name: String,
    /// 版本号（[1, VERSION_MAX] 闭集）。
    pub version: u32,
    /// 正文内容（摘要压叠源）。
    pub content: String,
    /// 登记时按 (类别, 名, 版本, 正文) 算出的摘要。
    pub digest: Digest,
}

impl AssetSnapshot {
    /// 构造快照：摘要随构造即时压定（登记即留凭）。
    pub fn new(kind: AssetKind, name: &str, version: u32, content: &str) -> AssetSnapshot {
        let digest = digest_compute(kind, name, version, content);
        AssetSnapshot {
            kind,
            name: String::from(name),
            version,
            content: String::from(content),
            digest,
        }
    }

    /// 重算摘要：冻结后必须与在册逐位相等（只增不减声明的复核口）。
    pub fn recompute(&self) -> Digest {
        digest_compute(self.kind, &self.name, self.version, &self.content)
    }
}

// ── 快照冻结库 ───────────────────────────────────────────────────
/// 库相位：收册（可登记可冻结）→ 封版（全库只读终态），只进不退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultPhase {
    /// 收册：登记与冻结开放。
    Collecting,
    /// 封版：全库只读，一切写入与判冻拒绝。
    Sealed,
}

/// 库槽：快照本体 + 冻结态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 快照本体。
    pub asset: AssetSnapshot,
    /// 三态登记态。
    pub state: FreezeState,
}

/// 快照冻结库：登记→冻结→封版的账面本体。
#[derive(Clone, Debug)]
pub struct FreezeVault {
    /// 槽列（按登记序，容量 ASSET_CAP）。
    slots: Vec<Slot>,
    /// 冻结轮次（每次成功冻结 +1，单调只增）。
    round: u32,
    /// 全库滚动指纹（每次冻结/封版折入，双跑一致性凭据）。
    fingerprint: Digest,
    /// 库相位。
    phase: VaultPhase,
}

/// 指纹折叠：把一份摘要折入滚动指纹（确定性，双跑恒等）。
fn fold_fingerprint(acc: Digest, d: Digest) -> Digest {
    Digest {
        hi: acc.hi ^ d.hi.rotate_left(3).wrapping_mul(FOLD_GOLDEN),
        lo: acc.lo.wrapping_add(d.lo).rotate_left(11) ^ d.hi,
    }
}

impl FreezeVault {
    /// 开一座新库：指纹取段基址派生的固定种子（零随机合规）。
    pub fn new() -> FreezeVault {
        FreezeVault {
            slots: Vec::new(),
            round: 0,
            fingerprint: Digest {
                hi: 0x243F_6A88_85A3_08D3 ^ (SEG_BASE as u64),
                lo: 0x1319_8A2E_0370_7344 ^ ((SEG_BASE as u64) << 17),
            },
            phase: VaultPhase::Collecting,
        }
    }

    /// 已登记槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 当前冻结轮次。
    pub fn round(&self) -> u32 {
        self.round
    }

    /// 当前全库指纹。
    pub fn fingerprint(&self) -> Digest {
        self.fingerprint
    }

    /// 当前库相位。
    pub fn phase(&self) -> VaultPhase {
        self.phase
    }

    /// 取第 idx 槽副本（越界 None，无裸下标）。
    pub fn slot_at(&self, idx: usize) -> Option<Slot> {
        self.slots.get(idx).cloned()
    }

    /// 已冻结（含封版）槽数。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() {
            if s.state.rank() >= FreezeState::Frozen.rank() {
                n += 1;
            }
        }
        n
    }

    /// 是否全部冻结（封版前置条件）。
    pub fn all_frozen(&self) -> bool {
        !self.slots.is_empty() && self.frozen_count() == self.slots.len()
    }

    /// 按名查三态。
    pub fn state_of(&self, name: &str) -> Option<FreezeState> {
        for s in self.slots.iter() {
            if s.asset.name == name {
                return Some(s.state);
            }
        }
        None
    }

    /// 按名取快照副本。
    pub fn asset_of(&self, name: &str) -> Option<AssetSnapshot> {
        for s in self.slots.iter() {
            if s.asset.name == name {
                return Some(s.asset.clone());
            }
        }
        None
    }

    /// 登记：封版闸/名闸/版本闸/容量闸四道，全过才入槽（草稿态）。
    pub fn register(
        &mut self,
        kind: AssetKind,
        name: &str,
        version: u32,
        content: &str,
    ) -> Result<(), u32> {
        // 封版闸：封版后登记拒绝。
        if self.phase == VaultPhase::Sealed {
            return Err(SnapDiag::ESealed.val());
        }
        // 名闸：空名拒。
        if name.is_empty() {
            return Err(SnapDiag::EName.val());
        }
        // 名闸：重名拒（资产名是冻结判定唯一键，重名即账目混乱）。
        let mut it = self.slots.iter();
        while let Some(s) = it.next() {
            if s.asset.name == name {
                return Err(SnapDiag::EName.val());
            }
        }
        // 版本闸：版本号必须落在 [1, VERSION_MAX]。
        if version == 0 || version > VERSION_MAX {
            return Err(SnapDiag::EVersion.val());
        }
        // 容量闸：8 槽闭集。
        if self.slots.len() >= ASSET_CAP {
            return Err(SnapDiag::EFull.val());
        }
        self.slots.push(Slot {
            asset: AssetSnapshot::new(kind, name, version, content),
            state: FreezeState::Draft,
        });
        Ok(())
    }

    /// 动态登记：类别号入口（越四类闭集 → EKind），供注入语料使用。
    pub fn register_dyn(
        &mut self,
        kind_idx: u32,
        name: &str,
        version: u32,
        content: &str,
    ) -> Result<(), u32> {
        let kind = match AssetKind::from_index(kind_idx) {
            Some(k) => k,
            None => return Err(SnapDiag::EKind.val()),
        };
        self.register(kind, name, version, content)
    }

    /// 冻结判定器（判据核心）：六层优先级写死——
    ///   ① 未登记资产 → ENOENT；
    ///   ② 非草稿（冻结/封版）版本翻转 → EROFS（优先于一切冻结态判定）；
    ///   ③ 封版重请求 → ERewind（终态判冻即倒退）；
    ///   ④ 冻结态摘要不符 → EDigest（先于 EALREADY）；
    ///   ⑤ 冻结态完全一致 → EALREADY（重复冻结）；
    ///   ⑥ 草稿态版本漂移 → EVersion（先于摘要复核）→ 草稿摘要篡改 →
    ///      EDigest → 全过则冻结落账，返回定案摘要。
    pub fn judge_freeze(&mut self, cand: &AssetSnapshot) -> Result<Digest, u32> {
        // ① 未登记闸。
        let pos = match self.slots.iter().position(|s| s.asset.name == cand.name) {
            Some(p) => p,
            None => return Err(SnapDiag::ENoent.val()),
        };
        // 取只读副本（避免借用冲突，无裸下标）。
        let snap = match self.slots.get(pos) {
            Some(s) => s.asset.clone(),
            None => return Err(SnapDiag::ENoent.val()),
        };
        let state = match self.slots.get(pos) {
            Some(s) => s.state,
            None => return Err(SnapDiag::ENoent.val()),
        };
        // ② 非草稿：版本翻转最优先（冻结后版本号只读，EROFS 语义锚点）。
        if state.rank() > FreezeState::Draft.rank() && cand.version != snap.version {
            return Err(SnapDiag::ERofs.val());
        }
        // ③ 封版终态：判冻通道关闭。
        if state == FreezeState::Sealed {
            return Err(SnapDiag::ERewind.val());
        }
        // ④⑤ 冻结态：摘要比对先行，再落重复冻结。
        if state == FreezeState::Frozen {
            if cand.digest != snap.digest {
                return Err(SnapDiag::EDigest.val());
            }
            return Err(SnapDiag::EAlready.val());
        }
        // ⑥ 草稿态：版本漂移闸（先于摘要复核，保证判定次序确定）。
        if cand.version != snap.version {
            return Err(SnapDiag::EVersion.val());
        }
        // 在册摘要复核：重算与在册不等 → 在册摘要被篡改。
        let recomputed = digest_compute(snap.kind, &snap.name, snap.version, &snap.content);
        if recomputed != snap.digest {
            return Err(SnapDiag::EDigest.val());
        }
        // 候选摘要复核：候选自带摘要与重算不等 → 候选被篡改。
        if cand.digest != recomputed {
            return Err(SnapDiag::EDigest.val());
        }
        // 全零摘要拒收（摘要域空即未定，不许冻结）。
        if recomputed.is_zero() {
            return Err(SnapDiag::EDigest.val());
        }
        // 冻结落账：三态 Draft→Frozen，轮次只增，指纹折入。
        if let Some(slot) = self.slots.get_mut(pos) {
            slot.state = FreezeState::Frozen;
        }
        self.round = self.round.saturating_add(1);
        self.fingerprint = fold_fingerprint(self.fingerprint, snap.digest);
        Ok(snap.digest)
    }

    /// 注入面（版本篡改）：把在册版本按位掩翻转，判定器必须抓出。
    pub fn tamper_version(&mut self, name: &str, mask: u32) -> Result<(), u32> {
        match self.slots.iter_mut().find(|s| s.asset.name == name) {
            Some(slot) => {
                slot.asset.version ^= mask;
                Ok(())
            }
            None => Err(SnapDiag::ENoent.val()),
        }
    }

    /// 注入面（摘要篡改）：把在册摘要双字翻转，判定器必须抓出。
    pub fn tamper_digest(&mut self, name: &str, mask: u32) -> Result<(), u32> {
        match self.slots.iter_mut().find(|s| s.asset.name == name) {
            Some(slot) => {
                slot.asset.digest.hi ^= mask as u64;
                slot.asset.digest.lo ^= ((mask as u64) << 17) | 1;
                Ok(())
            }
            None => Err(SnapDiag::ENoent.val()),
        }
    }

    /// 封版：全部冻结才许封；封后再封 → 倒退拒；封版时把终态项折入指纹。
    pub fn seal_all(&mut self) -> Result<(), u32> {
        if self.phase == VaultPhase::Sealed {
            return Err(SnapDiag::ERewind.val());
        }
        if !self.all_frozen() {
            return Err(SnapDiag::ENotfull.val());
        }
        for slot in self.slots.iter_mut() {
            if slot.state == FreezeState::Frozen {
                slot.state = FreezeState::Sealed;
            }
        }
        self.phase = VaultPhase::Sealed;
        self.fingerprint = fold_fingerprint(
            self.fingerprint,
            Digest {
                hi: 0x5163_3E2D_01A2_1F34 ^ (self.round as u64),
                lo: (self.slots.len() as u64).wrapping_mul(0x1F12_3BB5_9404_AB5D),
            },
        );
        Ok(())
    }
}

// ── 正册语料（四类各一份，判据域冻结文本） ───────────────────────
/// 正册语料条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpusAsset {
    /// 资产类别。
    pub kind: AssetKind,
    /// 资产名。
    pub name: &'static str,
    /// 版本号。
    pub version: u32,
    /// 正文。
    pub content: &'static str,
}

/// 四类正册语料（登记序即冻结演练轮转序，文本冻结不许改）。
pub const CORPUS: &[CorpusAsset] = &[
    CorpusAsset {
        kind: AssetKind::MasterOutline,
        name: "域总纲",
        version: 3,
        content: "本域以快照冻结为纪律：总纲条款一经登记即产出摘要，冻结后正文与版本号双双只读；任何修订必须开新版本号重新走登记-冻结-封版三态，禁止原地改账；全库指纹封版后作为下轮验收对账凭据。",
    },
    CorpusAsset {
        kind: AssetKind::LedgerBook,
        name: "差异台账",
        version: 7,
        content: "差异台账逐条登记差异来源、影响面与回滚口径；每条差异落账时同步刷新台账版本号，摘要随版本重算；冻结后差异列账以在册摘要为唯一凭据，摘要篡改当场由判定器抓出并列 ETamper 账。",
    },
    CorpusAsset {
        kind: AssetKind::Glossary,
        name: "术语表",
        version: 11,
        content: "术语表收录域内判定词：冻结、封版、漂移、篡改、逃脱、三态登记；每个术语的释义变更同样走快照冻结协议，术语表摘要进入全库指纹，防止口径静默漂移；静默演练检出率必须十轮全绿。",
    },
    CorpusAsset {
        kind: AssetKind::Chronicle,
        name: "域经档",
        version: 13,
        content: "域经档按时间序归档各轮验收结论与判据锚点；归档条目只增不删，档案版本号单调递增；封版后档案与全库指纹一并锁定，供下轮验收回溯对账；双跑指纹逐位一致是档案可信的前置声明。",
    },
];

/// 由语料序号构造候选快照（越界 None）。
pub fn corpus_candidate(idx: usize) -> Option<AssetSnapshot> {
    match CORPUS.get(idx) {
        Some(c) => Some(AssetSnapshot::new(c.kind, c.name, c.version, c.content)),
        None => None,
    }
}

/// 构造正册登记库（四类全草稿态）。
pub fn build_registry_vault() -> FreezeVault {
    let mut v = FreezeVault::new();
    let mut i = 0usize;
    while let Some(c) = CORPUS.get(i) {
        let _ = v.register(c.kind, c.name, c.version, c.content);
        i += 1;
    }
    v
}

/// 构造正册冻结库（四类全冻结态，轮次 = 4）。
pub fn build_frozen_vault() -> FreezeVault {
    let mut v = build_registry_vault();
    let mut i = 0usize;
    while let Some(c) = CORPUS.get(i) {
        let cand = AssetSnapshot::new(c.kind, c.name, c.version, c.content);
        let _ = v.judge_freeze(&cand);
        i += 1;
    }
    v
}

// ── 确定性 LCG（种子 = SEG_BASE 派生，零随机红线合规） ───────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（Knuth MMIX 31 位推荐常数，确定性）。
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

// ── 版本漂移注入器（判据「静默演练 10/10 检出」锚点） ────────────
/// 注入路别闭集：版本篡改 / 摘要篡改 / 重复冻结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    /// 版本篡改：草稿态 → EVersion，冻结态 → EROFS。
    VersionTamper,
    /// 摘要篡改：冻结后在册摘要翻转 → EDigest。
    DigestTamper,
    /// 重复冻结：首冻成功后再判 → EALREADY 且指纹不动。
    DupFreeze,
}

impl InjectKind {
    /// 路别中文名（逃脱描述用）。
    pub fn name(self) -> &'static str {
        match self {
            InjectKind::VersionTamper => "版本篡改",
            InjectKind::DigestTamper => "摘要篡改",
            InjectKind::DupFreeze => "重复冻结",
        }
    }
}

/// 注入演练总轮数（判据要求 10/10）。
pub const DRILL_ROUNDS: usize = 10;

/// 版本漂移注入器：三路各 10 轮静默演练，判定器必须逐轮返回指定码。
/// 返回 (检出轮数, 总轮数, 首个逃脱描述)。
pub fn inject_sweep(kind: InjectKind) -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut k = 0usize;
    while k < DRILL_ROUNDS {
        let idx = k % KIND_CAP;
        let ca = match CORPUS.get(idx) {
            Some(c) => *c,
            None => break,
        };
        // 翻转位随轮次抖动（1..3），库侧/候选侧按轮次奇偶交替。
        let delta = 1u32 + (k as u32) % 3;
        let mut hit = false;
        match kind {
            InjectKind::VersionTamper => {
                if k < DRILL_ROUNDS / 2 {
                    // 前半：草稿态版本篡改 → EVersion。
                    let mut v = build_registry_vault();
                    let got = if k % 2 == 0 {
                        // 库侧篡改：在册版本被翻转，候选持原版。
                        let _ = v.tamper_version(ca.name, delta);
                        let cand =
                            AssetSnapshot::new(ca.kind, ca.name, ca.version, ca.content);
                        v.judge_freeze(&cand)
                    } else {
                        // 候选侧篡改：候选版本漂移，在册持原版。
                        let cand = AssetSnapshot::new(
                            ca.kind,
                            ca.name,
                            ca.version.wrapping_add(delta),
                            ca.content,
                        );
                        v.judge_freeze(&cand)
                    };
                    hit = got == Err(SnapDiag::EVersion.val());
                } else {
                    // 后半：冻结后版本翻转 → EROFS。
                    let mut v = build_frozen_vault();
                    let got = if k % 2 == 0 {
                        let _ = v.tamper_version(ca.name, delta);
                        let cand =
                            AssetSnapshot::new(ca.kind, ca.name, ca.version, ca.content);
                        v.judge_freeze(&cand)
                    } else {
                        let cand = AssetSnapshot::new(
                            ca.kind,
                            ca.name,
                            ca.version.wrapping_add(delta),
                            ca.content,
                        );
                        v.judge_freeze(&cand)
                    };
                    hit = got == Err(SnapDiag::ERofs.val());
                }
            }
            InjectKind::DigestTamper => {
                // 冻结后在册摘要按轮次抖动位翻转 → EDigest。
                let mut v = build_frozen_vault();
                let _ = v.tamper_digest(ca.name, delta);
                let cand = AssetSnapshot::new(ca.kind, ca.name, ca.version, ca.content);
                hit = v.judge_freeze(&cand) == Err(SnapDiag::EDigest.val());
            }
            InjectKind::DupFreeze => {
                // 首冻成功 → 原候选再判 → EALREADY，且指纹不得变动。
                let mut v = build_registry_vault();
                let cand = AssetSnapshot::new(ca.kind, ca.name, ca.version, ca.content);
                let first = v.judge_freeze(&cand);
                let fp_before = v.fingerprint();
                let second = v.judge_freeze(&cand);
                hit = first.is_ok()
                    && second == Err(SnapDiag::EAlready.val())
                    && v.fingerprint() == fp_before;
            }
        }
        total += 1;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("{}路第{}轮逃脱：目标[{}]未被检出", kind.name(), k, ca.name);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入路全检：10/10 检出且无逃脱，否则 ETamper。
fn inject_all_caught(kind: InjectKind) -> Result<usize, u32> {
    let (caught, total, escape) = inject_sweep(kind);
    if total != DRILL_ROUNDS || caught != total || !escape.is_empty() {
        return Err(SnapDiag::ETamper.val());
    }
    Ok(caught)
}

/// 冻结摘要逐槽复核（只增不减声明的复核口）：
/// 每槽重算摘要与在册逐位相等，任一不等 → EDigest；返回冻结槽数。
pub fn audit_frozen_digests(v: &FreezeVault) -> Result<usize, u32> {
    let mut i = 0usize;
    let mut n = 0usize;
    while let Some(slot) = v.slot_at(i) {
        let a = &slot.asset;
        let recomputed = digest_compute(a.kind, &a.name, a.version, &a.content);
        if recomputed != a.digest {
            return Err(SnapDiag::EDigest.val());
        }
        if slot.state.rank() >= FreezeState::Frozen.rank() {
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 双跑指纹核验：两库相位/轮次/逐槽摘要/全库指纹全等，否则 EFinger。
pub fn verify_dual(a: &FreezeVault, b: &FreezeVault) -> Result<(), u32> {
    if a.phase() != b.phase() || a.round() != b.round() || a.len() != b.len() {
        return Err(SnapDiag::EFinger.val());
    }
    if a.fingerprint() != b.fingerprint() {
        return Err(SnapDiag::EFinger.val());
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.slot_at(i), b.slot_at(i)) {
            (Some(sa), Some(sb)) => {
                if sa.asset.digest != sb.asset.digest
                    || sa.state != sb.state
                    || sa.asset.version != sb.asset.version
                {
                    return Err(SnapDiag::EFinger.val());
                }
            }
            _ => return Err(SnapDiag::EFinger.val()),
        }
        i += 1;
    }
    Ok(())
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 全窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表闭环。
fn diag_closed_ok() -> bool {
    for d in SNAP_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    // 查表闭环：已知码必命中文案，未知码必回落。
    if detail(SnapDiag::ENoent.val()) != SnapDiag::ENoent.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == SnapDiag::ENoent.detail() {
        return false;
    }
    // 码值互异（闭集无重码）。
    let mut i = 0usize;
    while i < SNAP_ALL.len() {
        let vi = match SNAP_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < SNAP_ALL.len() {
            let vj = match SNAP_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 四类资产闭集守卫：序号回查、名称非空、名称与标签互异。
fn kind_closed_ok() -> bool {
    if KIND_ALL.len() != KIND_CAP {
        return false;
    }
    let mut i = 0u32;
    while i < KIND_CAP as u32 {
        match AssetKind::from_index(i) {
            Some(k) => {
                if k.index() != i || k.name().is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    if AssetKind::from_index(KIND_CAP as u32).is_some() {
        return false;
    }
    let mut a = 0usize;
    while a < KIND_ALL.len() {
        let ka = match KIND_ALL.get(a) {
            Some(k) => *k,
            None => return false,
        };
        let mut b = a + 1;
        while b < KIND_ALL.len() {
            let kb = match KIND_ALL.get(b) {
                Some(k) => *k,
                None => return false,
            };
            if ka.name() == kb.name() || ka.tag() == kb.tag() {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    true
}

/// 摘要确定性：同输入逐位相等、跨版本不碰撞、跨资产不碰撞。
fn digest_determinism_ok() -> bool {
    let ca = match CORPUS.first() {
        Some(c) => *c,
        None => return false,
    };
    let d1 = digest_compute(ca.kind, ca.name, ca.version, ca.content);
    let d2 = digest_compute(ca.kind, ca.name, ca.version, ca.content);
    if d1 != d2 || d1.is_zero() {
        return false;
    }
    // 版本抖动 10 轮：摘要必须逐轮偏离基线（跨版本不碰撞）。
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x71) as u64);
    let mut i = 0usize;
    while i < DRILL_ROUNDS {
        let v = 1u32 + rng.below(0xFFFE);
        if v != ca.version && digest_compute(ca.kind, ca.name, v, ca.content) == d1 {
            return false;
        }
        i += 1;
    }
    // 其余三类资产摘要不得与首类碰撞。
    let mut j = 1usize;
    while j < CORPUS.len() {
        let cb = match CORPUS.get(j) {
            Some(c) => *c,
            None => return false,
        };
        if digest_compute(cb.kind, cb.name, cb.version, cb.content) == d1 {
            return false;
        }
        j += 1;
    }
    true
}

/// 二次折叠雪崩：正文单字节翻转必须同时扰动双字（10 轮随机位点）。
fn avalanche_ok() -> bool {
    let ca = match CORPUS.get(1) {
        Some(c) => *c,
        None => return false,
    };
    let base = digest_compute(ca.kind, ca.name, ca.version, ca.content);
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x97) as u64);
    let mut k = 0usize;
    while k < DRILL_ROUNDS {
        let mut body: Vec<u8> = ca.content.as_bytes().to_vec();
        let pos = rng.below(body.len() as u32) as usize;
        match body.get_mut(pos) {
            Some(b) => *b ^= 0x01,
            None => return false,
        }
        let d = digest_body(ca.kind, ca.name, ca.version, &body);
        // 单字节翻转：hi 与 lo 必须双双偏离（交叉绑定保证）。
        if d.hi == base.hi || d.lo == base.lo {
            return false;
        }
        k += 1;
    }
    true
}

/// 正常流：四类登记 → 逐个冻结（各得定案摘要）→ 封版 → 终态核验。
fn happy_path_ok() -> bool {
    let mut v = build_registry_vault();
    if v.len() != KIND_CAP || v.phase() != VaultPhase::Collecting {
        return false;
    }
    let mut i = 0usize;
    while let Some(c) = CORPUS.get(i) {
        let cand = AssetSnapshot::new(c.kind, c.name, c.version, c.content);
        match v.judge_freeze(&cand) {
            Ok(d) => {
                if d != cand.digest || d.is_zero() {
                    return false;
                }
            }
            Err(_) => return false,
        }
        if v.state_of(c.name) != Some(FreezeState::Frozen) {
            return false;
        }
        i += 1;
    }
    if v.frozen_count() != KIND_CAP || v.round() != KIND_CAP as u32 {
        return false;
    }
    // 封版：三态 Frozen→Sealed，相位终态，指纹锁定。
    if v.seal_all().is_err() {
        return false;
    }
    if v.phase() != VaultPhase::Sealed {
        return false;
    }
    let mut j = 0usize;
    while let Some(c) = CORPUS.get(j) {
        if v.state_of(c.name) != Some(FreezeState::Sealed) {
            return false;
        }
        j += 1;
    }
    !v.fingerprint().is_zero()
}

/// 判定次序守卫：六层优先级逐层钉死（判据「三态登记」的判定面）。
fn judge_order_ok() -> bool {
    let ca0 = match CORPUS.first() {
        Some(c) => *c,
        None => return false,
    };
    let ca1 = match CORPUS.get(1) {
        Some(c) => *c,
        None => return false,
    };
    let cand0 = AssetSnapshot::new(ca0.kind, ca0.name, ca0.version, ca0.content);
    let cand1 = AssetSnapshot::new(ca1.kind, ca1.name, ca1.version, ca1.content);
    // ① 未登记资产最先判（即使类别/版本均合法）。
    let mut rv = build_registry_vault();
    let ghost = AssetSnapshot::new(ca0.kind, "未登记-幽灵", 1, "幽灵正文：不在册不许判冻。");
    if rv.judge_freeze(&ghost) != Err(SnapDiag::ENoent.val()) {
        return false;
    }
    // ② 冻结后版本翻转（版本+摘要同改）→ EROFS 优先于 EDigest/EALREADY。
    let mut fv = build_frozen_vault();
    let _ = fv.tamper_version(ca0.name, 1);
    let _ = fv.tamper_digest(ca0.name, 1);
    if fv.judge_freeze(&cand0) != Err(SnapDiag::ERofs.val()) {
        return false;
    }
    // ③ 封版终态重请求（版本未翻）→ ERewind。
    let mut sv = build_frozen_vault();
    if sv.seal_all().is_err() {
        return false;
    }
    if sv.judge_freeze(&cand0) != Err(SnapDiag::ERewind.val()) {
        return false;
    }
    // ④ 冻结后仅摘要被翻 → EDigest 先于 EALREADY。
    let mut dv = build_frozen_vault();
    let _ = dv.tamper_digest(ca1.name, 1);
    if dv.judge_freeze(&cand1) != Err(SnapDiag::EDigest.val()) {
        return false;
    }
    // ⑤ 冻结后完全一致 → EALREADY。
    let mut pv = build_frozen_vault();
    if pv.judge_freeze(&cand1) != Err(SnapDiag::EAlready.val()) {
        return false;
    }
    // ⑥ 草稿版本漂移 → EVersion 先于摘要比对。
    let mut wv = build_registry_vault();
    let _ = wv.tamper_version(ca1.name, 2);
    if wv.judge_freeze(&cand1) != Err(SnapDiag::EVersion.val()) {
        return false;
    }
    // ⑦ 草稿在册摘要被翻 → EDigest。
    let mut gv = build_registry_vault();
    let _ = gv.tamper_digest(ca1.name, 1);
    gv.judge_freeze(&cand1) == Err(SnapDiag::EDigest.val())
}

/// 冻结后 digest 只增不减声明：轮次严格递增、重复冻结不动账、
/// 重算摘要与在册逐位相等、注入篡改必被复核抓出。
fn frozen_digest_immutable_ok() -> bool {
    let mut v = build_registry_vault();
    // 逐个冻结：轮次必须逐次严格递增（只增）。
    let mut prev = v.round();
    let mut i = 0usize;
    while let Some(c) = CORPUS.get(i) {
        let cand = AssetSnapshot::new(c.kind, c.name, c.version, c.content);
        if v.judge_freeze(&cand).is_err() {
            return false;
        }
        if v.round() <= prev {
            return false;
        }
        prev = v.round();
        i += 1;
    }
    // 重复冻结不改变轮次（不减也不虚增——已冻结账目不可再动）。
    let cand0 = match corpus_candidate(0) {
        Some(c) => c,
        None => return false,
    };
    let _ = v.judge_freeze(&cand0);
    if v.round() != prev {
        return false;
    }
    // 重算摘要与在册逐位相等（冻结槽数 = 4）。
    match audit_frozen_digests(&v) {
        Ok(n) => {
            if n != CORPUS.len() {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 在册摘要一旦被注入翻转，复核必须当场抓出（不减声明可执行）。
    let _ = v.tamper_digest(CORPUS.first().map(|c| c.name).unwrap_or(""), 1);
    audit_frozen_digests(&v) == Err(SnapDiag::EDigest.val())
}

/// 容量耗尽：8 槽闭集填满后第 9 条登记必拒（EFull）。
fn capacity_ok() -> bool {
    let mut v = build_registry_vault();
    let extra = ASSET_CAP.saturating_sub(CORPUS.len());
    let mut k = 0u32;
    while (k as usize) < extra {
        let name = format!("附录快照-{}", k);
        let ver = 100u32.saturating_add(k);
        if v
            .register_dyn(3, &name, ver, "附录正文：随册快照，登记即入容量闭集。")
            .is_err()
        {
            return false;
        }
        k += 1;
    }
    if v.len() != ASSET_CAP {
        return false;
    }
    // 第 9 条：容量闸必拒。
    v.register_dyn(0, "超容快照", 1, "超容正文：8 槽已满必拒。") == Err(SnapDiag::EFull.val())
}

/// 双跑一致：两库逐槽等、指纹等，注入面三路双跑结果逐项等。
fn dual_run_ok() -> bool {
    let a = build_frozen_vault();
    let b = build_frozen_vault();
    if verify_dual(&a, &b).is_err() {
        return false;
    }
    let s1 = inject_sweep(InjectKind::VersionTamper);
    let s2 = inject_sweep(InjectKind::VersionTamper);
    let d1 = inject_sweep(InjectKind::DigestTamper);
    let d2 = inject_sweep(InjectKind::DigestTamper);
    let f1 = inject_sweep(InjectKind::DupFreeze);
    let f2 = inject_sweep(InjectKind::DupFreeze);
    s1 == s2 && d1 == d2 && f1 == f2
}

/// 状态机倒退守卫：草稿禁封版、封版禁重封/登记/判冻，账面不动。
fn rewind_ok() -> bool {
    // 草稿库直接封版 → ENotfull。
    let mut v = build_registry_vault();
    if v.seal_all() != Err(SnapDiag::ENotfull.val()) {
        return false;
    }
    // 正常封版后再封 → ERewind。
    let mut fv = build_frozen_vault();
    if fv.seal_all().is_err() {
        return false;
    }
    if fv.seal_all() != Err(SnapDiag::ERewind.val()) {
        return false;
    }
    // 封版后登记 → ESealed，账面槽数不变。
    let ca0 = match CORPUS.first() {
        Some(c) => *c,
        None => return false,
    };
    if fv
        .register(ca0.kind, "封版后来客", 9, "封版后登记正文：必拒。")
        != Err(SnapDiag::ESealed.val())
    {
        return false;
    }
    if fv.len() != CORPUS.len() {
        return false;
    }
    // 封版后判冻（版本未翻）→ ERewind。
    let cand0 = AssetSnapshot::new(ca0.kind, ca0.name, ca0.version, ca0.content);
    if fv.judge_freeze(&cand0) != Err(SnapDiag::ERewind.val()) {
        return false;
    }
    // 注入面对未知名操作 → ENoent（注入也不许凭空造账）。
    fv.tamper_version("不存在-幽灵", 1) == Err(SnapDiag::ENoent.val())
        && fv.tamper_digest("不存在-幽灵", 1) == Err(SnapDiag::ENoent.val())
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 声明检查条数（收口对账用）。
pub const EXPECT_CHECKS: usize = 15;

/// UNX-F12771 自检（判据 12771 卡：差异列账判据绿 + 静默演练 10/10 检出）。
pub fn run_unx12771_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12771");
    let vault = build_frozen_vault();

    s.add_dyn(
        "F12772-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} window=[0x80000,0x831C0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    s.add_dyn(
        "F12772-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            SNAP_ALL.len(),
            SNAP_ALL.first().map(|d| d.val()).unwrap_or(0),
            SNAP_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12772-03 四类资产闭集",
        kind_closed_ok(),
        &format!(
            "kinds={} 首={} 尾={}",
            KIND_ALL.len(),
            KIND_ALL.first().map(|k| k.name()).unwrap_or("?"),
            KIND_ALL.last().map(|k| k.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12772-04 摘要确定性（FNV-1a+二次折叠）",
        digest_determinism_ok(),
        &format!(
            "正册摘要={} bits=128",
            corpus_candidate(0)
                .map(|c| c.digest.hex())
                .unwrap_or_else(|| "无".to_string())
        ),
    );

    s.add_dyn(
        "F12772-05 二次折叠雪崩",
        avalanche_ok(),
        &format!("单字节翻转双字全偏离 轮数={}", DRILL_ROUNDS),
    );

    s.add_dyn(
        "F12772-06 正常流四类冻结封版",
        happy_path_ok(),
        &format!(
            "slots={} round={} frozen={} phase={:?} fp={}",
            vault.len(),
            vault.round(),
            vault.frozen_count(),
            vault.phase(),
            vault.fingerprint().hex()
        ),
    );

    s.add_dyn(
        "F12772-07 判定次序六层优先级",
        judge_order_ok(),
        "ENOENT>EROFS>封版倒退>EDigest>EALREADY>EVersion 逐层钉死",
    );

    // 静默演练一路：版本篡改（草稿→EVersion，冻结→EROFS）10/10 检出。
    let ver = inject_all_caught(InjectKind::VersionTamper);
    s.add_dyn(
        "F12772-08 版本篡改注入 10/10 检出",
        ver.is_ok(),
        &match &ver {
            Ok(n) => format!("检出 {}/{} 无逃脱", n, DRILL_ROUNDS),
            Err(e) => format!("逃脱 码=0x{:05x} {}", e, detail(*e)),
        },
    );

    // 静默演练二路：摘要篡改（冻结后翻转）10/10 检出。
    let dig = inject_all_caught(InjectKind::DigestTamper);
    s.add_dyn(
        "F12772-09 摘要篡改注入 10/10 检出",
        dig.is_ok(),
        &match &dig {
            Ok(n) => format!("检出 {}/{} 无逃脱", n, DRILL_ROUNDS),
            Err(e) => format!("逃脱 码=0x{:05x} {}", e, detail(*e)),
        },
    );

    // 静默演练三路：重复冻结（首冻后再判）10/10 检出。
    let dup = inject_all_caught(InjectKind::DupFreeze);
    s.add_dyn(
        "F12772-10 重复冻结注入 10/10 检出",
        dup.is_ok(),
        &match &dup {
            Ok(n) => format!("检出 {}/{} 无逃脱", n, DRILL_ROUNDS),
            Err(e) => format!("逃脱 码=0x{:05x} {}", e, detail(*e)),
        },
    );

    s.add_dyn(
        "F12772-11 冻结摘要只增不减声明",
        frozen_digest_immutable_ok(),
        &format!(
            "复核槽数={:?} 轮次单调 重算逐位等",
            audit_frozen_digests(&vault).ok()
        ),
    );

    s.add_dyn(
        "F12772-12 容量耗尽必拒",
        capacity_ok(),
        &format!("cap={} 正册={} 附录=4 第9条=EFull", ASSET_CAP, CORPUS.len()),
    );

    s.add_dyn(
        "F12772-13 双跑一致指纹",
        dual_run_ok(),
        &format!(
            "fp={} round={}",
            vault.fingerprint().hex(),
            vault.round()
        ),
    );

    s.add_dyn(
        "F12772-14 状态机倒退守卫",
        rewind_ok(),
        "草稿禁封版/重封拒/封版禁登记/封版禁判冻/未知名拒注入",
    );

    // 收口：判据条数声明（本节共 15 条，与 EXPECT_CHECKS 对账）。
    let n = s.len();
    s.add_dyn(
        "F12772-15 收口判据条数",
        n.saturating_add(1) == EXPECT_CHECKS,
        &format!("已登记={} 声明={}", n.saturating_add(1), EXPECT_CHECKS),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 测试取语料辅助：越界即断言失败（测试区允许 assert）。
    fn corpus_at(i: usize) -> CorpusAsset {
        let c = CORPUS.get(i).copied();
        assert!(c.is_some(), "语料越界 {}", i);
        match c {
            Some(x) => x,
            None => CorpusAsset {
                kind: AssetKind::MasterOutline,
                name: "",
                version: 1,
                content: "",
            },
        }
    }

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全窗口。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82AC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：13 码互异、全落窗口、查表命中文案、未知码回落。
        assert_eq!(SNAP_ALL.len(), 13);
        let mut i = 0usize;
        while i < SNAP_ALL.len() {
            let vi = SNAP_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < SNAP_ALL.len() {
                let vj = SNAP_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重码 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
        assert!(detail(SnapDiag::EFinger.val()).contains("双跑"));
    }

    #[test]
    fn four_kinds_closed_set_and_register_gates() {
        // 四类闭集：0..4 有解，越界无解，名/标签互异。
        assert_eq!(KIND_ALL.len(), KIND_CAP);
        assert!(AssetKind::from_index(0).is_some());
        assert!(AssetKind::from_index(3).is_some());
        assert!(AssetKind::from_index(4).is_none());
        assert!(kind_closed_ok());
        // 登记四闸：空名/重名/版本越界/类别越闭集逐路被拒。
        let mut v = FreezeVault::new();
        assert_eq!(v.register(ca0().kind, "", 1, "空名正文"), Err(SnapDiag::EName.val()));
        assert!(v.register(ca0().kind, ca0().name, ca0().version, ca0().content).is_ok());
        assert_eq!(
            v.register(ca1().kind, ca0().name, ca1().version, "重名正文"),
            Err(SnapDiag::EName.val())
        );
        assert_eq!(
            v.register(ca1().kind, "零版快照", 0, "零版正文"),
            Err(SnapDiag::EVersion.val())
        );
        assert_eq!(
            v.register(ca1().kind, "超版快照", VERSION_MAX + 1, "超版正文"),
            Err(SnapDiag::EVersion.val())
        );
        assert_eq!(
            v.register_dyn(9, "越类快照", 1, "越类正文"),
            Err(SnapDiag::EKind.val())
        );
    }

    fn ca0() -> CorpusAsset {
        corpus_at(0)
    }

    fn ca1() -> CorpusAsset {
        corpus_at(1)
    }

    #[test]
    fn happy_path_freezes_and_seals() {
        // 正常流：四类登记→逐个冻结→封版→三态终局。
        let mut v = build_registry_vault();
        assert_eq!(v.len(), 4);
        assert_eq!(v.phase(), VaultPhase::Collecting);
        let mut i = 0usize;
        while let Some(c) = CORPUS.get(i) {
            let cand = AssetSnapshot::new(c.kind, c.name, c.version, c.content);
            let d = v.judge_freeze(&cand);
            assert!(d.is_ok(), "冻结失败：{}", c.name);
            assert_eq!(d.ok(), Some(cand.digest));
            assert_eq!(v.state_of(c.name), Some(FreezeState::Frozen));
            i += 1;
        }
        assert_eq!(v.round(), 4);
        assert_eq!(v.frozen_count(), 4);
        assert!(v.all_frozen());
        assert!(v.seal_all().is_ok());
        assert_eq!(v.phase(), VaultPhase::Sealed);
        assert_eq!(v.state_of(ca0().name), Some(FreezeState::Sealed));
        assert!(!v.fingerprint().is_zero());
        assert!(happy_path_ok());
    }

    #[test]
    fn judge_order_precedence_is_fixed() {
        // 判定次序：六层优先级逐层断言。
        let mut rv = build_registry_vault();
        let ghost = AssetSnapshot::new(ca0().kind, "未登记-幽灵", 1, "幽灵正文");
        assert_eq!(rv.judge_freeze(&ghost), Err(SnapDiag::ENoent.val()));
        // 冻结后版本+摘要同改 → EROFS 优先。
        let mut fv = build_frozen_vault();
        let _ = fv.tamper_version(ca0().name, 1);
        let _ = fv.tamper_digest(ca0().name, 1);
        let cand0 = AssetSnapshot::new(ca0().kind, ca0().name, ca0().version, ca0().content);
        assert_eq!(fv.judge_freeze(&cand0), Err(SnapDiag::ERofs.val()));
        // 封版重请求 → ERewind。
        let mut sv = build_frozen_vault();
        assert!(sv.seal_all().is_ok());
        assert_eq!(sv.judge_freeze(&cand0), Err(SnapDiag::ERewind.val()));
        // 冻结摘要翻 → EDigest 先于 EALREADY。
        let mut dv = build_frozen_vault();
        let _ = dv.tamper_digest(ca1().name, 1);
        let cand1 = AssetSnapshot::new(ca1().kind, ca1().name, ca1().version, ca1().content);
        assert_eq!(dv.judge_freeze(&cand1), Err(SnapDiag::EDigest.val()));
        // 完全一致 → EALREADY。
        let mut pv = build_frozen_vault();
        assert_eq!(pv.judge_freeze(&cand1), Err(SnapDiag::EAlready.val()));
        // 草稿版本漂移 → EVersion 先于摘要。
        let mut wv = build_registry_vault();
        let _ = wv.tamper_version(ca1().name, 2);
        assert_eq!(wv.judge_freeze(&cand1), Err(SnapDiag::EVersion.val()));
        // 草稿在册摘要翻 → EDigest。
        let mut gv = build_registry_vault();
        let _ = gv.tamper_digest(ca1().name, 1);
        assert_eq!(gv.judge_freeze(&cand1), Err(SnapDiag::EDigest.val()));
        assert!(judge_order_ok());
    }

    #[test]
    fn inject_version_tamper_all_caught() {
        // 版本篡改 10 轮：草稿态 EVersion、冻结态 EROFS 全数命中。
        let (caught, total, escape) = inject_sweep(InjectKind::VersionTamper);
        assert_eq!(total, DRILL_ROUNDS);
        assert_eq!(caught, DRILL_ROUNDS, "逃脱：{}", escape);
        assert!(escape.is_empty());
        assert!(inject_all_caught(InjectKind::VersionTamper).is_ok());
    }

    #[test]
    fn inject_digest_tamper_all_caught() {
        // 摘要篡改 10 轮：冻结后在册摘要翻转全数命中 EDigest。
        let (caught, total, escape) = inject_sweep(InjectKind::DigestTamper);
        assert_eq!(total, DRILL_ROUNDS);
        assert_eq!(caught, DRILL_ROUNDS, "逃脱：{}", escape);
        assert!(escape.is_empty());
        assert!(inject_all_caught(InjectKind::DigestTamper).is_ok());
    }

    #[test]
    fn inject_dup_freeze_all_caught() {
        // 重复冻结 10 轮：首冻成功后再判全数命中 EALREADY 且指纹不动。
        let (caught, total, escape) = inject_sweep(InjectKind::DupFreeze);
        assert_eq!(total, DRILL_ROUNDS);
        assert_eq!(caught, DRILL_ROUNDS, "逃脱：{}", escape);
        assert!(escape.is_empty());
        assert!(inject_all_caught(InjectKind::DupFreeze).is_ok());
    }

    #[test]
    fn frozen_digest_bitwise_equal_and_round_monotonic() {
        // 只增不减声明：轮次严格递增、重复冻结不动、重算逐位相等、
        // 注入翻转必被复核抓出。
        let mut v = build_registry_vault();
        let mut prev = v.round();
        let mut i = 0usize;
        while let Some(c) = CORPUS.get(i) {
            let cand = AssetSnapshot::new(c.kind, c.name, c.version, c.content);
            assert!(v.judge_freeze(&cand).is_ok());
            assert!(v.round() > prev, "轮次未增");
            prev = v.round();
            i += 1;
        }
        let c0 = corpus_at(0);
        let cand0 = AssetSnapshot::new(c0.kind, c0.name, c0.version, c0.content);
        let _ = v.judge_freeze(&cand0);
        assert_eq!(v.round(), prev);
        assert_eq!(audit_frozen_digests(&v), Ok(4));
        let _ = v.tamper_digest(ca0().name, 1);
        assert_eq!(audit_frozen_digests(&v), Err(SnapDiag::EDigest.val()));
        assert!(frozen_digest_immutable_ok());
    }

    #[test]
    fn capacity_exhausted_at_eight() {
        // 容量闭集：8 槽填满后第 9 条必拒 EFull。
        let mut v = build_registry_vault();
        assert_eq!(v.len(), 4);
        let mut k = 0u32;
        while k < 4 {
            let name = format!("附录快照-{}", k);
            let r = v.register_dyn(3, &name, 100 + k, "附录正文：随册快照，登记即入容量闭集。");
            assert!(r.is_ok(), "附录 {} 登记失败", k);
            k += 1;
        }
        assert_eq!(v.len(), ASSET_CAP);
        assert_eq!(
            v.register_dyn(0, "超容快照", 1, "超容正文"),
            Err(SnapDiag::EFull.val())
        );
        assert!(capacity_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两库逐槽等、指纹等、注入三路双跑逐项等。
        let a = build_frozen_vault();
        let b = build_frozen_vault();
        assert!(verify_dual(&a, &b).is_ok());
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.round(), b.round());
        assert_eq!(inject_sweep(InjectKind::VersionTamper), inject_sweep(InjectKind::VersionTamper));
        assert_eq!(inject_sweep(InjectKind::DigestTamper), inject_sweep(InjectKind::DigestTamper));
        assert_eq!(inject_sweep(InjectKind::DupFreeze), inject_sweep(InjectKind::DupFreeze));
        assert!(dual_run_ok());
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：草稿禁封版、重封拒、封版禁登记/判冻、
        // 未知名拒注入。
        let mut v = build_registry_vault();
        assert_eq!(v.seal_all(), Err(SnapDiag::ENotfull.val()));
        let mut fv = build_frozen_vault();
        assert!(fv.seal_all().is_ok());
        assert_eq!(fv.seal_all(), Err(SnapDiag::ERewind.val()));
        assert_eq!(
            fv.register(ca0().kind, "封版后来客", 9, "封版后登记正文"),
            Err(SnapDiag::ESealed.val())
        );
        let cand0 = AssetSnapshot::new(ca0().kind, ca0().name, ca0().version, ca0().content);
        assert_eq!(fv.judge_freeze(&cand0), Err(SnapDiag::ERewind.val()));
        assert_eq!(fv.tamper_version("不存在-幽灵", 1), Err(SnapDiag::ENoent.val()));
        assert_eq!(fv.tamper_digest("不存在-幽灵", 1), Err(SnapDiag::ENoent.val()));
        assert!(rewind_ok());
    }

    #[test]
    fn digest_determinism_and_avalanche() {
        // 摘要确定性 + 雪崩面：同输入逐位等、版本抖动不碰撞、
        // 单字节翻转双字全偏离。
        assert!(digest_determinism_ok());
        assert!(avalanche_ok());
        let ca = corpus_at(2);
        let d1 = digest_compute(ca.kind, ca.name, ca.version, ca.content);
        let d2 = AssetSnapshot::new(ca.kind, ca.name, ca.version, ca.content).recompute();
        assert_eq!(d1, d2);
        assert_eq!(d1.hex().len(), 32);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且条数与收口声明对账。
        let s = run_unx12771_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert_eq!(s.len(), EXPECT_CHECKS);
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
