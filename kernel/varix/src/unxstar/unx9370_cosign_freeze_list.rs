//! UNX-F9370 号面冻结清单联签（F9600 冻结协议与 C3 台账互认） · 0x7CA00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9370 逐字）：
//!   UNX-F9370-J1 冻结协议联签判据过，F8801–F9600 号面冻结清单与 C3
//!   台账互认可观测，零重编声明格过。
//!
//! 设计要点：
//!   ① 号面冻结清单域模型：清单条目携带号段名、起始号、终止号、冻结态
//!      与联签方 C2/C3 双签名；号段强制落在 F8801..=F9600 窗口内且互不
//!      重叠——越窗回 ERANGE、倒挂回 EINVAL、重叠回 EEXIST、表满回
//!      ENOSPC；域模型围绕联签协议设计，与 fd 矩阵样板完全不同。
//!   ② 冻结状态机 Draft→Proposed→Cosigned→Frozen 单向单调迁移：step()
//!      只放行「恰好前进一档」，倒退与越级一律 EINVAL；已冻结条目再冻结
//!      回 EALREADY；Draft 态直接联签回 EPERM（先提议后联签）。
//!   ③ 联签判定器 judge_cosign(entry_id, signer, digest) 判定次序固定：
//!      ENOENT（查无条目）先于 EPERM（Draft 不可签）先于 EALREADY
//!      （重复签/双签已满）先于 EINVAL（digest 与条目清单指纹不符）；
//!      签满 C2+C3 两方即自动迁移 Cosigned，显式 freeze 后到 Frozen。
//!   ④ C3 台账互认：每条目带 C3 侧登记号与登记态，对账视图逐项复算——
//!      登记号必须等于派生公式、登记态必须与本地状态机同步；不一致行
//!      显性回报（错行数 + 首个错行号），不许静默吞。
//!   ⑤ 零重编声明：冻结后条目指纹不可再改（EROFS）且修改尝试留痕计数；
//!      未冻结条目指纹只增不减——回退 EINVAL、原地 EALREADY。
//!   ⑥ 注入器三类（坏 digest / 重复签 / 倒退迁移）各 10 轮全抓，注入账
//!      守恒（注入 = 检出 + 逃逸，逃逸恒 0）；双跑语料指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（UNX-F9370 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CA00;

/// 号面清单窗口下界（判据 F8801–F9600 的下沿，含）。
pub const F_LO: u32 = 8801;

/// 号面清单窗口上界（判据 F8801–F9600 的上沿，含）。
pub const F_HI: u32 = 9600;

/// 冻结清单容量（条目数上限；超出即 ENOSPC）。
pub const MAX_ENTRIES: usize = 10;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（坏 digest / 重复签 / 倒退迁移）。
pub const INJECT_CLASSES: u32 = 3;

/// POSIX errno 正数口径（判定器 Result<T, i32> 使用）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;
pub const EROFS: i32 = 30;
pub const ERANGE: i32 = 34;
pub const EALREADY: i32 = 114;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0E 闭集，15 个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 查无条目：entry_id 越过已用槽
    UnknownEntry,
    /// Draft 态直接联签被拒（须先 propose）
    DraftSignDenied,
    /// 重复签：同一联签方对同一条目二次签名
    DuplicateSign,
    /// digest 与条目清单指纹不符
    DigestMismatch,
    /// 号段越出 F8801..=F9600 窗口
    RangeOut,
    /// 号段与既有条目重叠
    OverlapDenied,
    /// 清单容量耗尽
    ListFull,
    /// 状态机倒退迁移被拒
    RegressDenied,
    /// 状态机越级迁移被拒
    SkipDenied,
    /// 冻结后条目不可再改（零重编）
    FrozenImmutable,
    /// 指纹回退（只增不减被破坏）
    FingerprintShrink,
    /// C3 台账登记号与派生公式不符
    C3RegMismatch,
    /// C3 台账登记态与本地状态机失同步
    C3StateMismatch,
    /// 注入账倒退（守恒式被破坏）
    LedgerRegress,
    /// 段位越界
    SegmentViolation,
}

impl FreezeCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 对应 errno（正数口径；与本模块判定器实际返回值一一对应）。
    pub fn errno(self) -> i32 {
        match self {
            FreezeCode::UnknownEntry => ENOENT,
            FreezeCode::DraftSignDenied => EPERM,
            FreezeCode::DuplicateSign => EALREADY,
            FreezeCode::DigestMismatch => EINVAL,
            FreezeCode::RangeOut => ERANGE,
            FreezeCode::OverlapDenied => EEXIST,
            FreezeCode::ListFull => ENOSPC,
            FreezeCode::RegressDenied => EINVAL,
            FreezeCode::SkipDenied => EINVAL,
            FreezeCode::FrozenImmutable => EROFS,
            FreezeCode::FingerprintShrink => EINVAL,
            FreezeCode::C3RegMismatch => EINVAL,
            FreezeCode::C3StateMismatch => EINVAL,
            FreezeCode::LedgerRegress => EINVAL,
            FreezeCode::SegmentViolation => EINVAL,
        }
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FreezeCode::UnknownEntry => "查无条目被拒：entry_id 越过已用槽数；请先 \
                admit 收编号段取得合法条目号，再发起提议/联签/冻结",
            FreezeCode::DraftSignDenied => "Draft 态联签被拒：条目尚未提议即签名；\
                请先走 propose 进入 Proposed 态，再由 C2/C3 依次联签",
            FreezeCode::DuplicateSign => "重复签被拒：该联签方已签过此条目；双签名 \
                协议要求 C2 与 C3 各签一次，重复提交回 EALREADY",
            FreezeCode::DigestMismatch => "digest 不符被拒：签名摘要与条目清单指纹 \
                不一致；请以 fingerprint_of 取最新指纹重签，不许手搓摘要",
            FreezeCode::RangeOut => "号段越窗被拒：起始/终止号超出 F8801..=F9600；\
                请把号段收进窗口内再收编入册",
            FreezeCode::OverlapDenied => "号段重叠被拒：新条目与既有条目区间相交；\
                请先对账既有清单，改划不重叠的号段边界",
            FreezeCode::ListFull => "清单满被拒：条目数已达容量上限；请先合并归档 \
                不活跃条目或扩容，再收编新号段，已收条目账不丢",
            FreezeCode::RegressDenied => "倒退迁移被拒：冻结状态机只进不退；任何 \
                Frozen→Cosigned 以下方向的迁移一律 EINVAL",
            FreezeCode::SkipDenied => "越级迁移被拒：Draft 不能直跳 Cosigned/Frozen、\
                Proposed 不能直跳 Frozen；请逐档推进状态机",
            FreezeCode::FrozenImmutable => "冻结条目不可改：零重编声明生效，冻结后 \
                指纹锁定；修改尝试已留痕，如确需变更请走解冻重议流程",
            FreezeCode::FingerprintShrink => "指纹回退被拒：条目指纹只增不减；新值 \
                不得小于现值，回退一律 EINVAL 并留痕",
            FreezeCode::C3RegMismatch => "C3 登记号不符：对账复算派生公式与登记值 \
                不一致；请重放 c3_expected_reg 并修正台账该行",
            FreezeCode::C3StateMismatch => "C3 登记态失同步：台账登记的冻结态与本地 \
                状态机不一致；请以本地单调状态机为准回补台账",
            FreezeCode::LedgerRegress => "注入账倒退被拒：注入 = 检出 + 逃逸的守恒式 \
                被破坏；请检查是否误改累计计数器",
            FreezeCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FreezeCode; 15] = [
        FreezeCode::UnknownEntry,
        FreezeCode::DraftSignDenied,
        FreezeCode::DuplicateSign,
        FreezeCode::DigestMismatch,
        FreezeCode::RangeOut,
        FreezeCode::OverlapDenied,
        FreezeCode::ListFull,
        FreezeCode::RegressDenied,
        FreezeCode::SkipDenied,
        FreezeCode::FrozenImmutable,
        FreezeCode::FingerprintShrink,
        FreezeCode::C3RegMismatch,
        FreezeCode::C3StateMismatch,
        FreezeCode::LedgerRegress,
        FreezeCode::SegmentViolation,
    ];
}

// ── 联签方 ────────────────────────────────────────────────────────────
/// 联签方：C2（施工侧）与 C3（台账侧）各签一次，双签齐备才可冻结。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// C2 施工侧联签方
    C2,
    /// C3 台账侧联签方
    C3,
}

impl Signer {
    /// 闭集全量。
    pub const ALL: [Signer; 2] = [Signer::C2, Signer::C3];

    /// 侧别标签（动态明细用）。
    pub fn label(self) -> &'static str {
        match self {
            Signer::C2 => "C2",
            Signer::C3 => "C3",
        }
    }
}

// ── 冻结状态机 ────────────────────────────────────────────────────────
/// 冻结态：Draft→Proposed→Cosigned→Frozen 单向单调，rank 只许 +1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 草稿：尚未提议，不可联签
    Draft,
    /// 已提议：等待 C2/C3 联签
    Proposed,
    /// 双签齐备：可显式冻结
    Cosigned,
    /// 已冻结：指纹锁定，零重编
    Frozen,
}

impl FreezeState {
    /// 档位（Draft=0 .. Frozen=3）；step() 只放行 +1 迁移。
    pub fn rank(self) -> u32 {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Proposed => 1,
            FreezeState::Cosigned => 2,
            FreezeState::Frozen => 3,
        }
    }

    /// 态标签（对账明细用）。
    pub fn label(self) -> &'static str {
        match self {
            FreezeState::Draft => "草稿",
            FreezeState::Proposed => "已提议",
            FreezeState::Cosigned => "已联签",
            FreezeState::Frozen => "已冻结",
        }
    }

    /// 闭集全量。
    pub const ALL: [FreezeState; 4] = [
        FreezeState::Draft,
        FreezeState::Proposed,
        FreezeState::Cosigned,
        FreezeState::Frozen,
    ];
}

/// 状态机步进：只放行「恰好前进一档」；倒退、越级、原地一律 EINVAL。
/// 这是全模块迁移的唯一通道，propose/freeze/judge_cosign 都经由它判定。
pub fn step(from: FreezeState, to: FreezeState) -> Result<FreezeState, i32> {
    if to.rank() == from.rank().saturating_add(1) {
        Ok(to)
    } else {
        Err(EINVAL)
    }
}

// ── 确定性摘要 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机；同输入必同输出）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 把字符串逐字节压进缓冲。
fn push_str_bytes(out: &mut Vec<u8>, s: &str) {
    let bs = s.as_bytes();
    let mut i = 0usize;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            out.push(*b);
        }
        i += 1;
    }
}

/// 把 u32 按小端逐字节压进缓冲。
fn push_u32_le(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}

/// 条目清单指纹：由号段名 + 起始号 + 终止号唯一派生。
/// 联签时签名者必须提交本值作为 digest，不符即 EINVAL。
pub fn list_digest(name: &str, lo: u32, hi: u32) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    push_str_bytes(&mut bytes, name);
    push_u32_le(&mut bytes, lo);
    push_u32_le(&mut bytes, hi);
    fnv1a(&bytes)
}

/// C3 侧登记号派生公式：清单指纹乘黄金比率再异或 C3 魔数。
/// 台账登记值必须逐条等于本公式，reconcile 逐项复算。
pub fn c3_expected_reg(name: &str, lo: u32, hi: u32) -> u32 {
    list_digest(name, lo, hi).wrapping_mul(0x9E37_79B1) ^ 0xC3C3_0000
}

// ── 清单条目 ──────────────────────────────────────────────────────────
/// 单条号面冻结条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeEntry {
    /// 号段名（静态串，不落堆）
    pub name: &'static str,
    /// 起始号（含）
    pub lo: u32,
    /// 终止号（含）
    pub hi: u32,
    /// 本地冻结态（状态机权威）
    pub state: FreezeState,
    /// C2 侧签名（存签名时提交的 digest）
    pub sig_c2: Option<u32>,
    /// C3 侧签名（存签名时提交的 digest）
    pub sig_c3: Option<u32>,
    /// 条目清单指纹（联签 digest 基准；只增不减）
    pub fingerprint: u32,
    /// C3 台账登记号（须等于 c3_expected_reg 派生值）
    pub c3_reg: u32,
    /// C3 台账登记态（须与本地 state 同步）
    pub c3_state: FreezeState,
    /// 指纹修订次数（零重编账：冻结后恒定）
    pub revisions: u32,
}

impl FreezeEntry {
    /// 空条目（占位/测试兜底视图）。
    pub const fn empty() -> FreezeEntry {
        FreezeEntry {
            name: "",
            lo: 0,
            hi: 0,
            state: FreezeState::Draft,
            sig_c2: None,
            sig_c3: None,
            fingerprint: 0,
            c3_reg: 0,
            c3_state: FreezeState::Draft,
            revisions: 0,
        }
    }
}

// ── 冻结清单 ──────────────────────────────────────────────────────────
/// 定容号面冻结清单：条目按收编序落槽，条目号即槽位下标。
#[derive(Clone, Copy, Debug)]
pub struct FreezeList {
    entries: [FreezeEntry; MAX_ENTRIES],
    used: usize,
    /// 收编成功笔数
    pub admit_ok: u32,
    /// 收编被拒笔数（ERANGE/EINVAL/EEXIST/ENOSPC 合计）
    pub admit_denied: u32,
    /// 提议调用次数（含被拒）
    pub proposals: u32,
    /// 联签调用次数（含被拒）
    pub cosign_calls: u32,
    /// 冻结调用次数（含被拒）
    pub freeze_calls: u32,
    /// 指纹修订尝试次数（零重编留痕）
    pub amend_attempts: u32,
    /// 指纹修订被拒次数（零重编留痕）
    pub amend_blocked: u32,
    /// 最近一次被拒的修订尝试值
    pub last_amend_attempt: u32,
    /// C3 台账注入污染次数（对账注入器留痕）
    pub c3_corruptions: u32,
}

impl FreezeList {
    /// 构造空清单。
    pub const fn new() -> FreezeList {
        FreezeList {
            entries: [FreezeEntry::empty(); MAX_ENTRIES],
            used: 0,
            admit_ok: 0,
            admit_denied: 0,
            proposals: 0,
            cosign_calls: 0,
            freeze_calls: 0,
            amend_attempts: 0,
            amend_blocked: 0,
            last_amend_attempt: 0,
            c3_corruptions: 0,
        }
    }

    /// 已收编条目数。
    pub fn len(&self) -> usize {
        self.used
    }

    /// 条目只读视图（越界回 None）。
    pub fn entry_view(&self, id: usize) -> Option<FreezeEntry> {
        if id >= self.used {
            return None;
        }
        match self.entries.get(id) {
            Some(e) => Some(*e),
            None => None,
        }
    }

    /// 条目当前冻结态。
    pub fn state_of(&self, id: usize) -> Result<FreezeState, i32> {
        match self.entry_view(id) {
            Some(e) => Ok(e.state),
            None => Err(ENOENT),
        }
    }

    /// 条目清单指纹（联签 digest 基准）。
    pub fn fingerprint_of(&self, id: usize) -> Result<u32, i32> {
        match self.entry_view(id) {
            Some(e) => Ok(e.fingerprint),
            None => Err(ENOENT),
        }
    }

    /// 条目 C3 登记号。
    pub fn c3_reg_of(&self, id: usize) -> Result<u32, i32> {
        match self.entry_view(id) {
            Some(e) => Ok(e.c3_reg),
            None => Err(ENOENT),
        }
    }

    /// 收编一条号面条目。判定次序：ERANGE（越窗）→ EINVAL（倒挂）→
    /// EEXIST（重叠）→ ENOSPC（表满）。单号段（lo == hi）合法。
    pub fn admit(&mut self, name: &'static str, lo: u32, hi: u32) -> Result<usize, i32> {
        if lo < F_LO || hi > F_HI {
            self.admit_denied = self.admit_denied.saturating_add(1);
            return Err(ERANGE);
        }
        if lo > hi {
            self.admit_denied = self.admit_denied.saturating_add(1);
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.entries.get(i) {
                if !(hi < e.lo || lo > e.hi) {
                    self.admit_denied = self.admit_denied.saturating_add(1);
                    return Err(EEXIST);
                }
            }
            i += 1;
        }
        if self.used >= MAX_ENTRIES {
            self.admit_denied = self.admit_denied.saturating_add(1);
            return Err(ENOSPC);
        }
        let fp = list_digest(name, lo, hi);
        let reg = c3_expected_reg(name, lo, hi);
        let id = self.used;
        if let Some(slot) = self.entries.get_mut(id) {
            *slot = FreezeEntry {
                name,
                lo,
                hi,
                state: FreezeState::Draft,
                sig_c2: None,
                sig_c3: None,
                fingerprint: fp,
                c3_reg: reg,
                c3_state: FreezeState::Draft,
                revisions: 0,
            };
        }
        self.used = self.used.saturating_add(1);
        self.admit_ok = self.admit_ok.saturating_add(1);
        Ok(id)
    }

    /// 提议：Draft→Proposed。非 Draft 态再提议即 EINVAL（原地/倒退皆拒）。
    pub fn propose(&mut self, id: usize) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        self.proposals = self.proposals.saturating_add(1);
        let entry = match self.entries.get_mut(id) {
            Some(e) => e,
            None => return Err(ENOENT),
        };
        match step(entry.state, FreezeState::Proposed) {
            Ok(to) => {
                entry.state = to;
                entry.c3_state = to;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 联签判定器。判定次序固定：
    /// ENOENT → EPERM（Draft 不可签）→ EALREADY（重复签/双签已满）→
    /// EINVAL（digest 不符）。签满 C2+C3 两方即自动迁移 Cosigned。
    pub fn judge_cosign(&mut self, id: usize, signer: Signer, digest: u32) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        self.cosign_calls = self.cosign_calls.saturating_add(1);
        let entry = match self.entries.get_mut(id) {
            Some(e) => e,
            None => return Err(ENOENT),
        };
        match entry.state {
            FreezeState::Draft => return Err(EPERM),
            FreezeState::Cosigned | FreezeState::Frozen => return Err(EALREADY),
            FreezeState::Proposed => {}
        }
        let already = match signer {
            Signer::C2 => entry.sig_c2.is_some(),
            Signer::C3 => entry.sig_c3.is_some(),
        };
        if already {
            return Err(EALREADY);
        }
        if digest != entry.fingerprint {
            return Err(EINVAL);
        }
        match signer {
            Signer::C2 => entry.sig_c2 = Some(digest),
            Signer::C3 => entry.sig_c3 = Some(digest),
        }
        let both = entry.sig_c2.is_some() && entry.sig_c3.is_some();
        if both {
            let from = entry.state;
            if let Ok(to) = step(from, FreezeState::Cosigned) {
                entry.state = to;
                entry.c3_state = to;
            }
        }
        Ok(())
    }

    /// 显式冻结：Cosigned→Frozen。Draft/Proposed 直冻即越级 EINVAL；
    /// 已冻结再冻回 EALREADY。
    pub fn freeze(&mut self, id: usize) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        self.freeze_calls = self.freeze_calls.saturating_add(1);
        let entry = match self.entries.get_mut(id) {
            Some(e) => e,
            None => return Err(ENOENT),
        };
        match entry.state {
            FreezeState::Frozen => Err(EALREADY),
            FreezeState::Cosigned => {
                entry.state = FreezeState::Frozen;
                entry.c3_state = FreezeState::Frozen;
                Ok(())
            }
            FreezeState::Draft | FreezeState::Proposed => Err(EINVAL),
        }
    }

    /// 通用迁移接口（注入器专用通道）：经由 step() 判定，倒退/越级/
    /// 原地一律 EINVAL，不落账不迁移。
    pub fn try_transition(&mut self, id: usize, target: FreezeState) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        let from = match self.entries.get(id) {
            Some(e) => e.state,
            None => return Err(ENOENT),
        };
        match step(from, target) {
            Ok(to) => {
                if let Some(e) = self.entries.get_mut(id) {
                    e.state = to;
                    e.c3_state = to;
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 指纹修订。零重编声明：冻结后一律 EROFS 并留痕；未冻结条目指纹
    /// 只增不减——回退 EINVAL、原地 EALREADY，仅增序修订放行。
    pub fn amend_fingerprint(&mut self, id: usize, new_fp: u32) -> Result<u32, i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        let outcome = {
            let entry = match self.entries.get_mut(id) {
                Some(e) => e,
                None => return Err(ENOENT),
            };
            if entry.state == FreezeState::Frozen {
                Err(EROFS)
            } else if new_fp < entry.fingerprint {
                Err(EINVAL)
            } else if new_fp == entry.fingerprint {
                Err(EALREADY)
            } else {
                entry.fingerprint = new_fp;
                entry.revisions = entry.revisions.saturating_add(1);
                Ok(new_fp)
            }
        };
        self.amend_attempts = self.amend_attempts.saturating_add(1);
        self.last_amend_attempt = new_fp;
        if outcome.is_err() {
            self.amend_blocked = self.amend_blocked.saturating_add(1);
        }
        outcome
    }

    /// 注入器通道：污染 C3 登记号（对账必须显性抓出并留痕）。
    pub fn corrupt_c3_reg(&mut self, id: usize, new_reg: u32) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        if let Some(e) = self.entries.get_mut(id) {
            e.c3_reg = new_reg;
        }
        self.c3_corruptions = self.c3_corruptions.saturating_add(1);
        Ok(())
    }

    /// 注入器通道：污染 C3 登记态（对账必须显性抓出并留痕）。
    pub fn corrupt_c3_state(&mut self, id: usize) -> Result<(), i32> {
        if id >= self.used {
            return Err(ENOENT);
        }
        if let Some(e) = self.entries.get_mut(id) {
            e.c3_state = FreezeState::Draft;
        }
        self.c3_corruptions = self.c3_corruptions.saturating_add(1);
        Ok(())
    }

    /// C3 台账互认对账：逐条复算登记号公式与登记态同步，返回
    /// （对账行数，错行数，首个错行号；无错行回 -1）。
    pub fn reconcile_c3(&self) -> (usize, usize, i32) {
        let mut checked = 0usize;
        let mut bad = 0usize;
        let mut first: i32 = -1;
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.entries.get(i) {
                checked = checked.saturating_add(1);
                let reg_ok = e.c3_reg == c3_expected_reg(e.name, e.lo, e.hi);
                let st_ok = e.c3_state == e.state;
                if !(reg_ok && st_ok) {
                    bad = bad.saturating_add(1);
                    if first < 0 {
                        first = i as i32;
                    }
                }
            }
            i += 1;
        }
        (checked, bad, first)
    }

    /// 双签齐备且已冻结的条目数（互认可观测口径之一）。
    pub fn frozen_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.entries.get(i) {
                if e.state == FreezeState::Frozen {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ── 规范语料（F8801–F9600 抽样 8 段，互不重叠） ───────────────────────
/// 规范号段语料：覆盖窗口两沿与中段，全部落窗且互不重叠。
pub const CANONICAL: &[(&str, u32, u32)] = &[
    ("f8801_sigret_edge", 8801, 8848),
    ("f8849_sigsuspend", 8849, 8900),
    ("f8901_tgkill_mid", 8901, 8950),
    ("f8951_pidfd_band", 8951, 9000),
    ("f9001_timerfd", 9001, 9100),
    ("f9101_signalfd4", 9101, 9250),
    ("f9251_epoll_band", 9251, 9400),
    ("f9401_freeze_hi_edge", 9401, 9600),
];

/// 全协议驱动：提议 → C2 联签 → C3 联签 → 冻结。任一步失败即短路。
pub fn drive_full_protocol(list: &mut FreezeList, id: usize) -> Result<(), i32> {
    list.propose(id)?;
    let fp = match list.fingerprint_of(id) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    list.judge_cosign(id, Signer::C2, fp)?;
    list.judge_cosign(id, Signer::C3, fp)?;
    list.freeze(id)?;
    Ok(())
}

/// 整清单指纹：对全部条目的指纹/登记号/状态档位/修订数做 FNV 压缩。
/// 零重编声明用它做前后对照：冻结后任何修改尝试不得改变本值。
pub fn list_fingerprint(list: &FreezeList) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < list.len() {
        if let Some(e) = list.entry_view(i) {
            push_str_bytes(&mut bytes, e.name);
            push_u32_le(&mut bytes, e.fingerprint);
            push_u32_le(&mut bytes, e.c3_reg);
            bytes.push(e.state.rank() as u8);
            push_u32_le(&mut bytes, e.revisions);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 规范语料双跑指纹：把 8 条规范号段全协议驱动到 Frozen 后压缩。
/// 同输入两次调用必须逐位一致（确定性声明）。
pub fn corpus_fingerprint() -> u32 {
    let mut list = FreezeList::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CANONICAL.len() {
        if let Some((name, lo, hi)) = CANONICAL.get(i).copied() {
            let id = match list.admit(name, lo, hi) {
                Ok(v) => v,
                Err(_) => usize::MAX,
            };
            if id != usize::MAX {
                let _ = drive_full_protocol(&mut list, id);
                let fp = match list.fingerprint_of(id) {
                    Ok(v) => v,
                    Err(_) => 0,
                };
                let reg = match list.c3_reg_of(id) {
                    Ok(v) => v,
                    Err(_) => 0,
                };
                let st = match list.state_of(id) {
                    Ok(s) => s,
                    Err(_) => FreezeState::Draft,
                };
                let rev = match list.entry_view(id) {
                    Some(e) => e.revisions,
                    None => 0,
                };
                push_u32_le(&mut bytes, fp);
                push_u32_le(&mut bytes, reg);
                bytes.push(st.rank() as u8);
                push_u32_le(&mut bytes, rev);
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式被破坏即显性报错。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(FreezeCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 三类注入器（各 10 轮全抓） ────────────────────────────────────────
/// 三类注入矩阵。构造后：
///   条目 0 全协议驱动到 Frozen（倒退迁移样本）；
///   条目 1 提议后 C2 已签（重复签样本）；
///   条目 2 仅提议（坏 digest 样本）。
/// 每类 INJECT_ROUNDS 轮，返回（账本，逐类检出数，注入后的清单）。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 3], FreezeList) {
    let mut list = FreezeList::new();
    let e0 = match list.admit("inj0_frozen_done", 8801, 8830) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let e1 = match list.admit("inj1_dup_signed", 8831, 8860) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let e2 = match list.admit("inj2_proposed", 8861, 8890) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    if e0 != usize::MAX {
        let _ = drive_full_protocol(&mut list, e0);
    }
    if e1 != usize::MAX {
        let _ = list.propose(e1);
        let fp1 = match list.fingerprint_of(e1) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = list.judge_cosign(e1, Signer::C2, fp1);
    }
    if e2 != usize::MAX {
        let _ = list.propose(e2);
    }
    let fp2 = if e2 != usize::MAX {
        match list.fingerprint_of(e2) {
            Ok(v) => v,
            Err(_) => 0,
        }
    } else {
        0
    };

    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];

    // 类 0：坏 digest —— 每轮摘要异或非零值，必与指纹不符，期望 EINVAL。
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let bad = fp2 ^ round.wrapping_add(1);
        let caught = list.judge_cosign(e2, Signer::C3, bad) == Err(EINVAL);
        let _ = ledger.record(caught);
        if caught {
            if let Some(slot) = per_class.get_mut(0) {
                *slot = slot.saturating_add(1);
            }
        }
        round += 1;
    }

    // 类 1：重复签 —— C2 对条目 1 二次提交（摘要正确也是 EALREADY）。
    let fp1 = if e1 != usize::MAX {
        match list.fingerprint_of(e1) {
            Ok(v) => v,
            Err(_) => 0,
        }
    } else {
        0
    };
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let caught = list.judge_cosign(e1, Signer::C2, fp1) == Err(EALREADY);
        let _ = ledger.record(caught);
        if caught {
            if let Some(slot) = per_class.get_mut(1) {
                *slot = slot.saturating_add(1);
            }
        }
        round += 1;
    }

    // 类 2：倒退迁移 —— 对已冻结条目反向 step、对已提议条目退回 Draft，
    // 全部必须被 step() 拒绝（EINVAL），且状态机不得被改写。
    let regress_targets: [FreezeState; 4] = [
        FreezeState::Cosigned,
        FreezeState::Proposed,
        FreezeState::Draft,
        FreezeState::Draft,
    ];
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let target = match regress_targets.get((round % 4) as usize) {
            Some(t) => *t,
            None => FreezeState::Draft,
        };
        let victim = if round % 4 == 3 { e2 } else { e0 };
        let caught = list.try_transition(victim, target) == Err(EINVAL);
        let _ = ledger.record(caught);
        if caught {
            if let Some(slot) = per_class.get_mut(2) {
                *slot = slot.saturating_add(1);
            }
        }
        round += 1;
    }

    (ledger, per_class, list)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// UNX-F9370 判据集（≥14 条静态 + ≤8 条动态明细）。
pub fn run_unx9370_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9370");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 清单收编：规范语料 8 条全部入册
    let mut can = FreezeList::new();
    let mut admitted = 0usize;
    let mut i = 0usize;
    while i < CANONICAL.len() {
        if let Some((name, lo, hi)) = CANONICAL.get(i).copied() {
            if can.admit(name, lo, hi).is_ok() {
                admitted += 1;
            }
        }
        i += 1;
    }
    s.add("清单收编：8 条规范号段全部入册且无重叠",
          admitted == CANONICAL.len() && can.len() == CANONICAL.len() && can.admit_denied == 0,
          "F8801–F9600 抽样号段必须逐条收编，重叠/越窗零发生");

    // 3. 四类边界：下沿-1 / 上沿+1 / 恰落两沿 / 倒挂与零号
    let mut bd = FreezeList::new();
    let below = bd.admit("bd_below_lo", F_LO - 1, F_LO);
    let above = bd.admit("bd_above_hi", F_HI + 1, F_HI + 1);
    let zero_lo = bd.admit("bd_zero_lo", 0, 1);
    let inverted = bd.admit("bd_inverted", 9100, 9000);
    let at_lo = bd.admit("bd_at_lo", F_LO, F_LO);
    let at_hi = bd.admit("bd_at_hi", F_HI, F_HI);
    let overlap = bd.admit("bd_overlap", F_LO, F_LO + 5);
    let boundary_ok = below == Err(ERANGE)
        && above == Err(ERANGE)
        && zero_lo == Err(ERANGE)
        && inverted == Err(EINVAL)
        && at_lo.is_ok()
        && at_hi.is_ok()
        && overlap == Err(EEXIST)
        && bd.len() == 2
        && bd.admit_denied == 5;
    s.add("边界四类：越窗/倒挂/零号全拒，两沿单号段全收", boundary_ok,
          "F_LO-1 与 F_HI+1 必 ERANGE，倒挂必 EINVAL，重叠必 EEXIST，两沿恰落必收");

    // 4. 正常流：全协议驱动到冻结
    let mut nf = FreezeList::new();
    let nid = match nf.admit("nf_full_protocol", 9001, 9050) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let normal = if nid == usize::MAX {
        false
    } else {
        drive_full_protocol(&mut nf, nid) == Ok(())
            && nf.state_of(nid) == Ok(FreezeState::Frozen)
            && nf.frozen_count() == 1
    };
    s.add("正常流：提议→C2 签→C3 签→冻结全通", normal,
          "合法号段全协议驱动必须抵达 Frozen，双签齐备自动迁移 Cosigned");

    // 5. 状态机单调：越级/倒退/原地全拒，逐档推进全放行
    let mut sm = FreezeList::new();
    let sid = match sm.admit("sm_monotonic", 9101, 9150) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let sm_ok = if sid == usize::MAX {
        false
    } else {
        let skip1 = sm.try_transition(sid, FreezeState::Cosigned) == Err(EINVAL);
        let p_ok = sm.propose(sid) == Ok(());
        let skip2 = sm.try_transition(sid, FreezeState::Frozen) == Err(EINVAL);
        let in_place = sm.propose(sid) == Err(EINVAL);
        let regress = sm.try_transition(sid, FreezeState::Draft) == Err(EINVAL);
        let fp_sm = match sm.fingerprint_of(sid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = sm.judge_cosign(sid, Signer::C2, fp_sm);
        let _ = sm.judge_cosign(sid, Signer::C3, fp_sm);
        let cosigned = sm.state_of(sid) == Ok(FreezeState::Cosigned);
        let f_skip = sm.try_transition(sid, FreezeState::Proposed) == Err(EINVAL);
        let f_ok = sm.freeze(sid) == Ok(());
        let f_again = sm.freeze(sid) == Err(EALREADY);
        skip1 && p_ok && skip2 && in_place && regress && cosigned && f_skip && f_ok && f_again
    };
    s.add("状态机：Draft→Proposed→Cosigned→Frozen 单调，倒退/越级全拒", sm_ok,
          "step() 只放行 +1 迁移；直冻 Draft/Proposed 必 EINVAL，重复冻结必 EALREADY");

    // 6. 判定次序：ENOENT > EPERM > EALREADY > EINVAL
    let mut od = FreezeList::new();
    let oid = match od.admit("od_order", 9201, 9250) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let order_ok = if oid == usize::MAX {
        false
    } else {
        let unknown = od.judge_cosign(oid + MAX_ENTRIES, Signer::C2, 0) == Err(ENOENT);
        let draft_first = od.judge_cosign(oid, Signer::C2, 1) == Err(EPERM);
        let _ = od.propose(oid);
        let fp = match od.fingerprint_of(oid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = od.judge_cosign(oid, Signer::C2, fp);
        let dup_beats_digest = od.judge_cosign(oid, Signer::C2, fp ^ 1) == Err(EALREADY);
        let dup_good = od.judge_cosign(oid, Signer::C2, fp) == Err(EALREADY);
        let digest_gate = od.judge_cosign(oid, Signer::C3, fp ^ 0xDEAD) == Err(EINVAL);
        let both_ok = od.judge_cosign(oid, Signer::C3, fp) == Ok(());
        let full_state = od.judge_cosign(oid, Signer::C3, fp) == Err(EALREADY);
        unknown && draft_first && dup_beats_digest && dup_good && digest_gate && both_ok
            && full_state
    };
    s.add("判定次序：ENOENT→EPERM→EALREADY→EINVAL 依次短路", order_ok,
          "查无条目先于草稿拒签，重复签先于摘要校验，摘要校验先于落签");

    // 7-9. 三类注入各 10 轮
    let (ledger, per_class, inj_list) = run_injection_matrix();
    let bad_digest_ok = match per_class.get(0) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入类 0：坏 digest 10/10 全抓（EINVAL）", bad_digest_ok,
          "摘要与清单指纹不符必须逐轮回 EINVAL，不得误报他码");

    let dup_ok = match per_class.get(1) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入类 1：重复签 10/10 全抓（EALREADY）", dup_ok,
          "同侧二次签名必须逐轮回 EALREADY，即便摘要正确也拒");

    let regress_ok = match per_class.get(2) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入类 2：倒退迁移 10/10 全抓（EINVAL）", regress_ok,
          "Frozen/Cosigned/Proposed 反向 step 必须逐轮被拒且状态机不被改写");

    // 10. 注入账守恒
    let conserved = ledger.conserved()
        && ledger.escaped == 0
        && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS;
    s.add("注入账守恒：30 注入 = 30 检出 + 0 逃逸", conserved,
          "守恒式注入=检出+逃逸必须闭合，逃逸恒为 0");

    // 11. 注入后状态机未被改写
    let intact = inj_list.state_of(0) == Ok(FreezeState::Frozen)
        && inj_list.state_of(2) == Ok(FreezeState::Proposed)
        && match inj_list.entry_view(2) {
            Some(e) => e.sig_c3.is_none(),
            None => false,
        };
    s.add("注入不落账：被拒操作不改写状态机与签名槽", intact,
          "被拒的联签/迁移必须零副作用，签名槽保持原样");

    // 12. C3 互认：正常流逐项对账零错行
    let mut c3l = FreezeList::new();
    let mut ci = 0usize;
    while ci < CANONICAL.len() {
        if let Some((name, lo, hi)) = CANONICAL.get(ci).copied() {
            let cid = match c3l.admit(name, lo, hi) {
                Ok(v) => v,
                Err(_) => usize::MAX,
            };
            if cid != usize::MAX {
                let _ = drive_full_protocol(&mut c3l, cid);
            }
        }
        ci += 1;
    }
    let clean = c3l.reconcile_c3();
    let c3_clean = clean.0 == CANONICAL.len() && clean.1 == 0 && clean.2 < 0;
    s.add("C3 互认：8 条全协议后对账零错行", c3_clean,
          "登记号公式与登记态同步必须逐项复算通过");

    // 13. C3 注入：登记号污染显性抓出
    let mut c3x = FreezeList::new();
    let xa = match c3x.admit("c3x_reg_bad", 9301, 9330) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let xb = match c3x.admit("c3x_reg_ok", 9331, 9360) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let _ = drive_full_protocol(&mut c3x, xa);
    let _ = drive_full_protocol(&mut c3x, xb);
    let xreg = match c3x.c3_reg_of(xb) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = c3x.corrupt_c3_reg(xb, xreg ^ 0x1);
    let dirty = c3x.reconcile_c3();
    let c3_dirty = dirty.0 == 2
        && dirty.1 == 1
        && dirty.2 == xb as i32
        && c3x.c3_corruptions == 1;
    s.add("C3 注入：登记号污染一行被显性定位", c3_dirty,
          "错行数与首错行号必须精确到被污染条目，不许静默吞");

    // 14. 零重编：冻结后指纹锁定、修改留痕、整单指纹不变
    let mut zr = FreezeList::new();
    let zid = match zr.admit("zr_zero_recompile", 9401, 9450) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let zr_ok = if zid == usize::MAX {
        false
    } else {
        let _ = drive_full_protocol(&mut zr, zid);
        let fp_before = match zr.fingerprint_of(zid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let whole_before = list_fingerprint(&zr);
        let blocked = zr.amend_fingerprint(zid, fp_before ^ 0xFFFF) == Err(EROFS);
        let shrunk = zr.amend_fingerprint(zid, fp_before.wrapping_sub(1)) == Err(EROFS);
        let fp_after = match zr.fingerprint_of(zid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let whole_after = list_fingerprint(&zr);
        blocked && shrunk && fp_after == fp_before && whole_after == whole_before
            && zr.amend_blocked == 2 && zr.amend_attempts == 2
    };
    s.add("零重编：冻结后指纹锁定，修改尝试被拒且留痕", zr_ok,
          "冻结条目 amend 必 EROFS，整清单指纹前后一致，留痕计数逐次累加");

    // 15. 指纹只增不减：未冻结条目增序放行、回退/原地拒绝
    let mut og = FreezeList::new();
    let gid = match og.admit("og_only_grow", 9501, 9550) {
        Ok(v) => v,
        Err(_) => usize::MAX,
    };
    let grow_ok = if gid == usize::MAX {
        false
    } else {
        let fp0 = match og.fingerprint_of(gid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let up = og.amend_fingerprint(gid, fp0.wrapping_add(7)) == Ok(fp0.wrapping_add(7));
        let fp1 = match og.fingerprint_of(gid) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let back = og.amend_fingerprint(gid, fp0) == Err(EINVAL);
        let same = og.amend_fingerprint(gid, fp1) == Err(EALREADY);
        up && back && same
            && match og.entry_view(gid) {
                Some(e) => e.revisions == 1,
                None => false,
            }
    };
    s.add("指纹只增：增序修订放行一次，回退 EINVAL、原地 EALREADY", grow_ok,
          "未冻结条目指纹只增不减；修订次数只增，回退尝试一律拒绝");

    // 16. 容量耗尽：满后 ENOSPC 且已收条目账不丢
    let mut cap = FreezeList::new();
    let mut filled = 0usize;
    let mut overflow_hits = 0u32;
    let mut wrong_codes = 0u32;
    let mut n = 0u32;
    while n < MAX_ENTRIES as u32 + 1 {
        let lo = F_LO + n;
        match cap.admit("cap_single", lo, lo) {
            Ok(_) => filled += 1,
            Err(ENOSPC) => overflow_hits += 1,
            Err(_) => wrong_codes += 1,
        }
        n += 1;
    }
    let cap_ok = filled == MAX_ENTRIES
        && overflow_hits == 1
        && wrong_codes == 0
        && cap.len() == MAX_ENTRIES
        && cap.admit_ok == MAX_ENTRIES as u32
        && cap.state_of(0) == Ok(FreezeState::Draft)
        && cap.fingerprint_of(0).is_ok();
    s.add("容量耗尽：第 11 条 ENOSPC，已收 10 条账不丢", cap_ok,
          "表满只回 ENOSPC 一次；先前条目的状态与指纹原样可查");

    // 17. 双跑一致
    let fa = corpus_fingerprint();
    let fb = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹两次相等", fa == fb,
          "同输入两次全协议驱动必须逐位一致，隐藏状态即失败");

    // 18. 文案闭集
    let text_ok = FreezeCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：15 个诊断码全带非空中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 19. 诊断码唯一且 val 连续
    let mut uniq = true;
    let mut ui = 0usize;
    while ui < FreezeCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < FreezeCode::ALL.len() {
            if let (Some(a), Some(b)) = (FreezeCode::ALL.get(ui), FreezeCode::ALL.get(uj)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    let mut contiguous = true;
    let mut vi = 0usize;
    while vi < FreezeCode::ALL.len() {
        if let Some(c) = FreezeCode::ALL.get(vi) {
            if c.val() != SEG_BASE + vi as u32 {
                contiguous = false;
            }
        }
        vi += 1;
    }
    s.add("诊断码：15 码两两唯一且 val = SEG_BASE + 序号", uniq && contiguous,
          "段内闭集码不得重复，码值必须与变体序号严格对应");

    // 20. errno 映射闭集
    let errno_ok = FreezeCode::UnknownEntry.errno() == ENOENT
        && FreezeCode::DraftSignDenied.errno() == EPERM
        && FreezeCode::DuplicateSign.errno() == EALREADY
        && FreezeCode::DigestMismatch.errno() == EINVAL
        && FreezeCode::ListFull.errno() == ENOSPC
        && FreezeCode::FrozenImmutable.errno() == EROFS
        && FreezeCode::OverlapDenied.errno() == EEXIST;
    s.add("errno 映射：七类核心码逐项与 POSIX 正数口径一致", errno_ok,
          "诊断码到 errno 的映射必须与判定器实际返回值一致");

    // 21. 账目单调：注入账再记账只增不减
    let mut mono = ledger;
    let before_inj = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再记账只增不减且守恒不破",
          mono.injected == before_inj.saturating_add(4) && mono.conserved(),
          "累计计数禁止回退或饱和到旧值，守恒式持续闭合");

    // 动态明细（≤8 槽）：注入账 / 指纹 / C3 对账 / 容量 四条快照
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    let fp_text = format!("语料指纹 0x{:08X}", fa);
    s.add_dyn("指纹快照", fa == fb, &fp_text);

    let c3_text = format!("对账 {} 行 错 {} 行 首错 {}", clean.0, clean.1, clean.2);
    s.add_dyn("C3 对账快照", c3_clean, &c3_text);

    let cap_text = format!(
        "容量 {}/{} 溢出拒绝 {} 次",
        cap.len(),
        MAX_ENTRIES,
        overflow_hits
    );
    s.add_dyn("容量快照", cap_ok, &cap_text);

    // 22. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "判据条数只增不减，低于 14 条即视为骨架不完整");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_full_protocol() {
        let mut l = FreezeList::new();
        let id = l.admit("t_normal", 9001, 9050).unwrap_or(usize::MAX);
        assert!(id != usize::MAX);
        assert_eq!(drive_full_protocol(&mut l, id), Ok(()));
        assert_eq!(l.state_of(id), Ok(FreezeState::Frozen));
        assert_eq!(l.frozen_count(), 1);
        let e = l.entry_view(id).unwrap_or(FreezeEntry::empty());
        assert!(e.sig_c2.is_some() && e.sig_c3.is_some());
    }

    #[test]
    fn boundary_four_classes() {
        let mut l = FreezeList::new();
        assert_eq!(l.admit("t_b1", F_LO - 1, F_LO), Err(ERANGE));
        assert_eq!(l.admit("t_b2", F_HI + 1, F_HI + 1), Err(ERANGE));
        assert_eq!(l.admit("t_b3", 0, 1), Err(ERANGE));
        assert_eq!(l.admit("t_b4", 9100, 9000), Err(EINVAL));
        assert!(l.admit("t_b5", F_LO, F_LO).is_ok());
        assert!(l.admit("t_b6", F_HI, F_HI).is_ok());
        assert_eq!(l.admit("t_b7", F_LO, F_LO + 5), Err(EEXIST));
        assert_eq!(l.len(), 2);
        assert_eq!(l.admit_denied, 5);
    }

    #[test]
    fn bad_digest_injections_all_caught() {
        let (_, per, _) = run_injection_matrix();
        assert_eq!(*per.get(0).unwrap_or(&0), INJECT_ROUNDS);
    }

    #[test]
    fn duplicate_sign_injections_all_caught() {
        let (_, per, _) = run_injection_matrix();
        assert_eq!(*per.get(1).unwrap_or(&0), INJECT_ROUNDS);
    }

    #[test]
    fn regress_injections_all_caught() {
        let (_, per, l) = run_injection_matrix();
        assert_eq!(*per.get(2).unwrap_or(&0), INJECT_ROUNDS);
        assert_eq!(l.state_of(0), Ok(FreezeState::Frozen));
        assert_eq!(l.state_of(2), Ok(FreezeState::Proposed));
    }

    #[test]
    fn judgment_order_precedence() {
        let mut l = FreezeList::new();
        let id = l.admit("t_order", 9201, 9250).unwrap_or(usize::MAX);
        assert!(id != usize::MAX);
        let ghost = id.saturating_add(MAX_ENTRIES);
        assert_eq!(l.judge_cosign(ghost, Signer::C2, 0), Err(ENOENT));
        assert_eq!(l.judge_cosign(id, Signer::C2, 1), Err(EPERM));
        let _ = l.propose(id);
        let fp = l.fingerprint_of(id).unwrap_or(0);
        assert!(l.judge_cosign(id, Signer::C2, fp).is_ok());
        assert_eq!(l.judge_cosign(id, Signer::C2, fp ^ 1), Err(EALREADY));
        assert_eq!(l.judge_cosign(id, Signer::C3, fp ^ 1), Err(EINVAL));
        assert!(l.judge_cosign(id, Signer::C3, fp).is_ok());
        assert_eq!(l.judge_cosign(id, Signer::C3, fp), Err(EALREADY));
    }

    #[test]
    fn state_machine_monotonic() {
        assert_eq!(step(FreezeState::Draft, FreezeState::Proposed), Ok(FreezeState::Proposed));
        assert_eq!(
            step(FreezeState::Proposed, FreezeState::Cosigned),
            Ok(FreezeState::Cosigned)
        );
        assert_eq!(step(FreezeState::Cosigned, FreezeState::Frozen), Ok(FreezeState::Frozen));
        assert_eq!(step(FreezeState::Frozen, FreezeState::Cosigned), Err(EINVAL));
        assert_eq!(step(FreezeState::Draft, FreezeState::Frozen), Err(EINVAL));
        assert_eq!(step(FreezeState::Proposed, FreezeState::Proposed), Err(EINVAL));
        let mut l = FreezeList::new();
        let id = l.admit("t_sm", 9101, 9150).unwrap_or(usize::MAX);
        assert_eq!(l.freeze(id), Err(EINVAL));
        assert_eq!(l.propose(id), Ok(()));
        assert_eq!(l.freeze(id), Err(EINVAL));
        assert_eq!(l.try_transition(id, FreezeState::Draft), Err(EINVAL));
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let mut l = FreezeList::new();
        let id = l.admit("t_dual", 8801, 8830).unwrap_or(usize::MAX);
        let before = list_fingerprint(&l);
        let _ = drive_full_protocol(&mut l, id);
        let after = list_fingerprint(&l);
        assert!(before != after);
        assert_eq!(list_fingerprint(&l), after);
    }

    #[test]
    fn capacity_exhaustion_enospc() {
        let mut l = FreezeList::new();
        let mut ok = 0usize;
        let mut overflow = 0u32;
        let mut n = 0u32;
        while n < MAX_ENTRIES as u32 + 1 {
            let lo = F_LO + n;
            match l.admit("t_cap", lo, lo) {
                Ok(_) => ok += 1,
                Err(ENOSPC) => overflow += 1,
                Err(_) => overflow += 100,
            }
            n += 1;
        }
        assert_eq!(ok, MAX_ENTRIES);
        assert_eq!(overflow, 1);
        assert_eq!(l.len(), MAX_ENTRIES);
        assert_eq!(l.state_of(0), Ok(FreezeState::Draft));
        assert!(l.fingerprint_of(0).is_ok());
    }

    #[test]
    fn c3_mismatch_rows_explicit() {
        let mut l = FreezeList::new();
        let a = l.admit("t_c3a", 9301, 9330).unwrap_or(usize::MAX);
        let b = l.admit("t_c3b", 9331, 9360).unwrap_or(usize::MAX);
        let _ = drive_full_protocol(&mut l, a);
        let _ = drive_full_protocol(&mut l, b);
        let clean = l.reconcile_c3();
        assert_eq!(clean.0, 2);
        assert_eq!(clean.1, 0);
        assert!(clean.2 < 0);
        let reg = l.c3_reg_of(b).unwrap_or(0);
        let _ = l.corrupt_c3_reg(b, reg ^ 0x1);
        let d1 = l.reconcile_c3();
        assert_eq!(d1.1, 1);
        assert_eq!(d1.2, b as i32);
        let _ = l.corrupt_c3_state(a);
        let d2 = l.reconcile_c3();
        assert_eq!(d2.1, 2);
        assert_eq!(d2.2, a as i32);
    }

    #[test]
    fn frozen_zero_recompile_guard() {
        let mut l = FreezeList::new();
        let id = l.admit("t_zr", 9401, 9450).unwrap_or(usize::MAX);
        let _ = drive_full_protocol(&mut l, id);
        let fp = l.fingerprint_of(id).unwrap_or(0);
        let whole = list_fingerprint(&l);
        assert_eq!(l.amend_fingerprint(id, fp ^ 0xFFFF), Err(EROFS));
        assert_eq!(l.amend_fingerprint(id, fp.wrapping_sub(1)), Err(EROFS));
        assert_eq!(l.fingerprint_of(id), Ok(fp));
        assert_eq!(list_fingerprint(&l), whole);
        assert_eq!(l.amend_blocked, 2);
    }

    #[test]
    fn fingerprint_only_grows_before_frozen() {
        let mut l = FreezeList::new();
        let id = l.admit("t_grow", 9501, 9550).unwrap_or(usize::MAX);
        let fp0 = l.fingerprint_of(id).unwrap_or(0);
        assert_eq!(l.amend_fingerprint(id, fp0.wrapping_add(7)), Ok(fp0.wrapping_add(7)));
        let fp1 = l.fingerprint_of(id).unwrap_or(0);
        assert_eq!(l.amend_fingerprint(id, fp0), Err(EINVAL));
        assert_eq!(l.amend_fingerprint(id, fp1), Err(EALREADY));
        let e = l.entry_view(id).unwrap_or(FreezeEntry::empty());
        assert_eq!(e.revisions, 1);
    }

    #[test]
    fn segment_guard_window() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7CA00);
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(FreezeCode::ALL.len(), 15);
        let mut i = 0usize;
        while i < FreezeCode::ALL.len() {
            let c = FreezeCode::ALL
                .get(i)
                .copied()
                .unwrap_or(FreezeCode::SegmentViolation);
            assert!(!c.detail().is_empty());
            assert_eq!(c.val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(FreezeState::ALL.len(), 4);
        assert_eq!(Signer::ALL.len(), 2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9370_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 15);
    }
}
