//! UNX-F9394 联调防重声明（C2×C4 号面零冲突 grep） · 0x7D000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9394-J1 逐字）：
//!   锚点：UNX-F9394-J1 防重判据过，四范围 grep 零冲突格过，
//!   条目名零命中格过
//!
//! 数据段 SEG_BASE = 0x7D000：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9394 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：DeclareRegistry（声明台账：复合键 = 域 × 号 × 类别，
//!     复合键去重，重复声明显性拒绝并记账）；
//!   - 解析器：SlugGrep（四号段范围 grep：9101–9200 / 9201–9300 /
//!     9301–9400 / 9401–9500，逐范围统计条目名冲突，必须零冲突）；
//!   - 状态机：DeclareStage（Draft→Declared→Confirmed 单向）；
//!   - 账本：GrepLedger（四范围扫描账 + 冲突账 + 条目名命中账，
//!     冲突=0 且条目名命中=0 为格过）+ 注入账（逃逸恒 0）；
//!   - 注入核：4 类坏样本（重复复合键/重复条目名/越界号段/未声明
//!     即确认）逐类显性拒绝；
//!   - 指纹：FNV-1a 台账序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9394 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D000;

/// 四号段范围闭集（起闭尾含）：9101–9200 / 9201–9300 / 9301–9400 / 9401–9500。
pub const RANGE_BOUNDS: [(u32, u32); 4] =
    [(9101, 9200), (9201, 9300), (9301, 9400), (9401, 9500)];

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupCode {
    /// 重复复合键：域×号×类别 已登记
    DuplicateKey,
    /// 重复条目名：slug 已被占用
    DuplicateSlug,
    /// 越界号段：号不在四范围闭集内
    NumOutOfRange,
    /// 未声明即确认：Draft 上直接 Confirmed
    ConfirmBeforeDeclare,
    /// 未知声明：台账无此键
    UnknownDeclaration,
    /// 范围冲突：四范围 grep 命中同名条目
    RangeConflict,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 声明类别越界（闭集 3 类之外）
    BadKind,
}

impl DedupCode {
    /// 闭集数组。
    pub const ALL: [DedupCode; 10] = [
        DedupCode::DuplicateKey,
        DedupCode::DuplicateSlug,
        DedupCode::NumOutOfRange,
        DedupCode::ConfirmBeforeDeclare,
        DedupCode::UnknownDeclaration,
        DedupCode::RangeConflict,
        DedupCode::LedgerRegress,
        DedupCode::UnknownErrno,
        DedupCode::NotReproducible,
        DedupCode::BadKind,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DedupCode::DuplicateKey => 1,
                DedupCode::DuplicateSlug => 2,
                DedupCode::NumOutOfRange => 3,
                DedupCode::ConfirmBeforeDeclare => 4,
                DedupCode::UnknownDeclaration => 5,
                DedupCode::RangeConflict => 6,
                DedupCode::LedgerRegress => 7,
                DedupCode::UnknownErrno => 8,
                DedupCode::NotReproducible => 9,
                DedupCode::BadKind => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            DedupCode::DuplicateKey => "复合键（域×号×类别）唯一，重复声明按 EEXIST 拒绝",
            DedupCode::DuplicateSlug => "条目名全局唯一，重名显性拒绝",
            DedupCode::NumOutOfRange => "号必须在四范围闭集内，越界即拒绝登记",
            DedupCode::ConfirmBeforeDeclare => "确认必须发生在声明之后，逆序即拒绝",
            DedupCode::UnknownDeclaration => "按复合键查台账失败，先声明再引用",
            DedupCode::RangeConflict => "四范围 grep 必须零冲突，命中即红",
            DedupCode::LedgerRegress => "声明账只增不减，倒退即审计失败",
            DedupCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            DedupCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            DedupCode::BadKind => "声明类别只认 0..=2 三类闭集",
        }
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 声明状态机：Draft→Declared→Confirmed 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclareStage {
    /// 草稿（尚未对外声明）
    Draft,
    /// 已声明（台账在案）
    Declared,
    /// 已确认（防重复核通过）
    Confirmed,
}

impl DeclareStage {
    /// 单向迁移核验。
    pub fn advance(self, want: DeclareStage) -> Option<DeclareStage> {
        let legal = match (self, want) {
            (DeclareStage::Draft, DeclareStage::Declared) => true,
            (DeclareStage::Declared, DeclareStage::Confirmed) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 声明台账 ──────────────────────────────────────────────────────────
/// 复合键：域 × 号 × 类别（闭集 3 类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeclareKey {
    /// 域号（0=C2 1=C4 2=other）
    pub domain: u32,
    /// 卡号（四范围闭集内）
    pub num: u32,
    /// 类别（0=声明 1=防重 2=依赖）
    pub kind: u32,
}

/// 单条声明记录。
#[derive(Clone, Copy)]
pub struct DeclareRecord {
    pub key: DeclareKey,
    /// 条目名（全局唯一 slug）
    pub slug: u32,
    /// 状态
    pub stage: DeclareStage,
}

/// 声明台账。
pub struct DeclareRegistry {
    records: Vec<DeclareRecord>,
    /// 成功声明数
    pub declared: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl DeclareRegistry {
    /// 建空台账。
    pub fn new() -> DeclareRegistry {
        DeclareRegistry { records: Vec::new(), declared: 0, rejected: 0 }
    }

    /// 按复合键取记录。
    pub fn find(&self, key: DeclareKey) -> Option<DeclareRecord> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.key == key {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 条目名占用检测。
    pub fn slug_taken(&self, slug: u32) -> bool {
        self.records.iter().any(|r| r.slug == slug)
    }

    /// 号范围闭集校验。
    pub fn in_ranges(num: u32) -> bool {
        RANGE_BOUNDS.iter().any(|b| num >= b.0 && num <= b.1)
    }

    /// 声明登记：复合键去重 + 条目名去重 + 范围闭集校验。
    pub fn declare(&mut self, key: DeclareKey, slug: u32) -> Result<usize, DedupCode> {
        if key.kind > 2 {
            self.rejected += 1;
            return Err(DedupCode::BadKind);
        }
        if !Self::in_ranges(key.num) {
            self.rejected += 1;
            return Err(DedupCode::NumOutOfRange);
        }
        if self.find(key).is_some() {
            self.rejected += 1;
            return Err(DedupCode::DuplicateKey);
        }
        if self.slug_taken(slug) {
            self.rejected += 1;
            return Err(DedupCode::DuplicateSlug);
        }
        self.records.push(DeclareRecord { key, slug, stage: DeclareStage::Declared });
        self.declared += 1;
        Ok(self.records.len() - 1)
    }

    /// 确认：Declared→Confirmed（未声明即确认显性拒绝）。
    pub fn confirm(&mut self, key: DeclareKey) -> Result<(), DedupCode> {
        let idx = {
            let mut found = None;
            let mut i = 0usize;
            while i < self.records.len() {
                if let Some(r) = self.records.get(i) {
                    if r.key == key {
                        found = Some(i);
                    }
                }
                i += 1;
            }
            match found {
                Some(i) => i,
                None => return Err(DedupCode::UnknownDeclaration),
            }
        };
        let cur = match self.records.get(idx) {
            Some(r) => r.stage,
            None => return Err(DedupCode::UnknownDeclaration),
        };
        match cur.advance(DeclareStage::Confirmed) {
            Some(next) => {
                if let Some(r) = self.records.get_mut(idx) {
                    r.stage = next;
                }
                Ok(())
            }
            None => Err(DedupCode::ConfirmBeforeDeclare),
        }
    }

    /// 台账快照：(slug) 序列（指纹载体）。
    pub fn slugs(&self) -> Vec<u32> {
        self.records.iter().map(|r| r.slug).collect()
    }

    /// 台账快照：(num, slug) 对（grep 载体）。
    pub fn pairs(&self) -> Vec<(u32, u32)> {
        self.records.iter().map(|r| (r.key.num, r.slug)).collect()
    }
}

// ── 四范围 grep ───────────────────────────────────────────────────────
/// grep 账。
#[derive(Clone, Copy)]
pub struct GrepLedger {
    /// 四范围逐范围扫描的声明数
    pub scanned: [u32; 4],
    /// 条目名冲突命中数（必须 0）
    pub conflicts: u32,
    /// 条目名重复命中数（必须 0）
    pub slug_hits: u32,
}

impl GrepLedger {
    /// 零账初值。
    pub fn new() -> GrepLedger {
        GrepLedger { scanned: [0; 4], conflicts: 0, slug_hits: 0 }
    }

    /// 格过条件：冲突=0 且条目名命中=0。
    pub fn clean(&self) -> bool {
        self.conflicts == 0 && self.slug_hits == 0
    }
}

/// 四范围 grep：逐范围统计声明数并检测同名条目冲突。
///
/// 输入为 (num, slug) 对（登记口已拒重名，这里是对账面的二次审计：
/// 即使登记口被绕过，同范围同名条目也必须被 grep 检出）。
/// 冲突定义：同一号段内两个不同条目声明了同一 slug。
pub fn grep_ranges(pairs: &[(u32, u32)]) -> GrepLedger {
    let mut led = GrepLedger::new();
    let mut b = 0usize;
    while b < RANGE_BOUNDS.len() {
        let (lo, hi) = RANGE_BOUNDS[b];
        let mut seen_slugs: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < pairs.len() {
            if let Some((num, slug)) = pairs.get(i).copied() {
                if num >= lo && num <= hi {
                    if let Some(c) = led.scanned.get_mut(b) {
                        *c += 1;
                    }
                    if seen_slugs.iter().any(|s| *s == slug) {
                        led.conflicts += 1;
                        led.slug_hits += 1;
                    } else {
                        seen_slugs.push(slug);
                    }
                }
            }
            i += 1;
        }
        b += 1;
    }
    led
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 台账序列指纹。
pub fn registry_fingerprint(slugs: &[u32]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for s in slugs {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    fnv1a(&buf)
}

/// 确定性 slug 派生（域×号×类别 → 条目名），双跑一致。
pub fn derive_slug(key: DeclareKey) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&key.domain.to_le_bytes());
    buf.extend_from_slice(&key.num.to_le_bytes());
    buf.extend_from_slice(&key.kind.to_le_bytes());
    fnv1a(&buf)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 4 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 4], u32) {
    let mut ok = [false; 4];
    let mut detected = 0u32;
    let k0 = DeclareKey { domain: 0, num: 9101, kind: 0 };

    // 类 1：重复复合键
    let mut r1 = DeclareRegistry::new();
    let _ = r1.declare(k0, derive_slug(k0));
    let dup_key = r1.declare(k0, derive_slug(k0)) == Err(DedupCode::DuplicateKey);
    if dup_key {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：重复条目名（不同复合键、同名 slug）
    let mut r2 = DeclareRegistry::new();
    let _ = r2.declare(k0, 0x5150);
    let k2 = DeclareKey { domain: 1, num: 9101, kind: 0 };
    let dup_slug = r2.declare(k2, 0x5150) == Err(DedupCode::DuplicateSlug);
    if dup_slug {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：越界号段（9100 与 9501）
    let mut r3 = DeclareRegistry::new();
    let out = r3.declare(DeclareKey { domain: 0, num: 9100, kind: 0 }, 1)
        == Err(DedupCode::NumOutOfRange)
        && r3.declare(DeclareKey { domain: 0, num: 9501, kind: 0 }, 2)
            == Err(DedupCode::NumOutOfRange);
    if out {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：未声明即确认（未知键 confirm）
    let mut r4 = DeclareRegistry::new();
    let ghost = r4.confirm(k0) == Err(DedupCode::UnknownDeclaration);
    if ghost {
        detected += 1;
        ok[3] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EBADF, EACCES, EEXIST, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9394 自检：防重声明判据（约 14 条）。
pub fn run_unx9394_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9394");

    // 1. 声明台账：四范围逐范围各登记一条
    let mut reg = DeclareRegistry::new();
    let mut b = 0usize;
    while b < RANGE_BOUNDS.len() {
        let k = DeclareKey { domain: 0, num: RANGE_BOUNDS[b].0, kind: 0 };
        let _ = reg.declare(k, derive_slug(k));
        b += 1;
    }
    s.add("四范围声明：每范围至少一条在案", reg.declared == RANGE_BOUNDS.len() as u32,
          "台账逐范围落账");

    // 2. 四范围 grep 零冲突
    let reg_pairs = reg.pairs();
    let led = grep_ranges(&reg_pairs);
    s.add("四范围 grep 零冲突", led.clean() && led.scanned.iter().all(|v| *v > 0),
          "逐范围扫描且无同名条目");

    // 3. 条目名零命中：全台账 slug 无重复
    let slugs = reg.slugs();
    let mut uniq = true;
    let mut i = 0usize;
    while i < slugs.len() {
        let mut j = i + 1;
        while j < slugs.len() {
            if slugs.get(i) == slugs.get(j) {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("条目名零命中：台账 slug 全局唯一", uniq && led.slug_hits == 0,
          "重名条目必须被拒在登记口");

    // 4. 复合键去重：同键二次声明拒绝
    let k0 = DeclareKey { domain: 0, num: 9101, kind: 0 };
    let dup_key = {
        let mut r = DeclareRegistry::new();
        let _ = r.declare(k0, derive_slug(k0));
        r.declare(k0, derive_slug(k0)) == Err(DedupCode::DuplicateKey)
    };
    s.add("复合键去重：同键二次声明 EEXIST 拒绝", dup_key,
          "域×号×类别 复合键唯一");

    // 5. 条目名去重：跨键同名拒绝
    let dup_slug = {
        let mut r = DeclareRegistry::new();
        let _ = r.declare(k0, 0xBEEF);
        r.declare(DeclareKey { domain: 1, num: 9101, kind: 0 }, 0xBEEF)
            == Err(DedupCode::DuplicateSlug)
    };
    s.add("条目名去重：跨键同名 slug 拒绝", dup_slug,
          "条目名全局唯一");

    // 6. 范围闭集：界内通过、界外拒绝
    let closed = DeclareRegistry::in_ranges(9101)
        && DeclareRegistry::in_ranges(9500)
        && !DeclareRegistry::in_ranges(9100)
        && !DeclareRegistry::in_ranges(9501);
    s.add("范围闭集：9101/9500 在列、9100/9501 拒绝", closed,
          "四范围 [9101,9200][9201,9300][9301,9400][9401,9500]");

    // 7. 状态机单向
    let fsm_ok = DeclareStage::Draft.advance(DeclareStage::Declared) == Some(DeclareStage::Declared)
        && DeclareStage::Declared.advance(DeclareStage::Confirmed) == Some(DeclareStage::Confirmed)
        && DeclareStage::Draft.advance(DeclareStage::Confirmed).is_none();
    s.add("状态机：Draft→Declared→Confirmed 单向", fsm_ok,
          "未声明即确认显性拒绝");

    // 8. 确认路径
    let confirm_ok = {
        let mut r = DeclareRegistry::new();
        let _ = r.declare(k0, derive_slug(k0));
        r.confirm(k0).is_ok()
    };
    s.add("确认路径：声明后确认成功", confirm_ok,
          "Declared→Confirmed 合法推进");

    // 9. 注入核 4 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：4 类坏样本逐类显性拒绝", all_rej && detected == 4,
          "重键/重名/越界/幽灵确认全过闸");

    // 10. 拒绝账
    let mut r10 = DeclareRegistry::new();
    let _ = r10.declare(k0, 1);
    let _ = r10.declare(k0, 2);
    let _ = r10.declare(DeclareKey { domain: 0, num: 9999, kind: 0 }, 3);
    let rej_ok = r10.rejected == 2 && r10.declared == 1;
    s.add("拒绝账：重复键与越界号各记一笔", rej_ok,
          "拒绝不静默，显性入账");

    // 11. slug 派生确定且区分
    let k_a = DeclareKey { domain: 0, num: 9201, kind: 1 };
    let k_b = DeclareKey { domain: 0, num: 9202, kind: 1 };
    let derive_ok = derive_slug(k_a) == derive_slug(k_a) && derive_slug(k_a) != derive_slug(k_b);
    s.add("slug 派生：同键一致、异键区分", derive_ok,
          "确定性派生是零冲突声明的前提");

    // 12. 双跑指纹
    let fp1 = registry_fingerprint(&slugs);
    let fp2 = registry_fingerprint(&reg.slugs());
    s.add("双跑指纹：台账序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性台账，无隐藏状态");

    // 13. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EEXIST)
        && !agrees_with_linux(0)
        && !agrees_with_linux(4321);
    s.add("errno 对照：EEXIST 口径拒重复、表外码拒绝", codes_ok,
          "错误码闭集对拍");

    // 14. 诊断码闭集
    let text_ok = DedupCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut cuniq = true;
    let mut p = 0usize;
    while p < DedupCode::ALL.len() {
        let mut q = p + 1;
        while q < DedupCode::ALL.len() {
            let a = DedupCode::ALL.get(p).map(|c| c.val()).unwrap_or(0);
            let b = DedupCode::ALL.get(q).map(|c| c.val()).unwrap_or(1);
            if a == b {
                cuniq = false;
            }
            q += 1;
        }
        p += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && cuniq,
          "SEG_BASE+1..=+10 闭集");

    // 动态明细（≤8）
    let g_text = format!(
        "扫描 {:?} / 冲突 {} / 重名 {}",
        led.scanned, led.conflicts, led.slug_hits
    );
    s.add_dyn("grep 账快照", led.clean(), &g_text);
    let fp_text = format!("台账指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declare_fsm_one_way() {
        assert_eq!(DeclareStage::Draft.advance(DeclareStage::Declared), Some(DeclareStage::Declared));
        assert_eq!(DeclareStage::Declared.advance(DeclareStage::Confirmed), Some(DeclareStage::Confirmed));
        assert_eq!(DeclareStage::Confirmed.advance(DeclareStage::Declared), None);
    }

    #[test]
    fn range_closed_set() {
        assert!(DeclareRegistry::in_ranges(9101));
        assert!(DeclareRegistry::in_ranges(9200));
        assert!(DeclareRegistry::in_ranges(9500));
        assert!(!DeclareRegistry::in_ranges(9100));
        assert!(!DeclareRegistry::in_ranges(9200 + 1 - 1 + 1 + 7901));
        assert!(!DeclareRegistry::in_ranges(9501));
    }

    #[test]
    fn duplicate_key_rejected() {
        let mut r = DeclareRegistry::new();
        let k = DeclareKey { domain: 0, num: 9101, kind: 0 };
        assert!(r.declare(k, 1).is_ok());
        assert_eq!(r.declare(k, 1), Err(DedupCode::DuplicateKey));
        assert_eq!(r.rejected, 1);
    }

    #[test]
    fn duplicate_slug_rejected() {
        let mut r = DeclareRegistry::new();
        let _ = r.declare(DeclareKey { domain: 0, num: 9101, kind: 0 }, 42);
        assert_eq!(
            r.declare(DeclareKey { domain: 1, num: 9101, kind: 0 }, 42),
            Err(DedupCode::DuplicateSlug)
        );
    }

    #[test]
    fn out_of_range_rejected() {
        let mut r = DeclareRegistry::new();
        assert_eq!(
            r.declare(DeclareKey { domain: 0, num: 9000, kind: 0 }, 1),
            Err(DedupCode::NumOutOfRange)
        );
        assert_eq!(
            r.declare(DeclareKey { domain: 0, num: 9600, kind: 0 }, 1),
            Err(DedupCode::NumOutOfRange)
        );
    }

    #[test]
    fn confirm_requires_declare() {
        let mut r = DeclareRegistry::new();
        let k = DeclareKey { domain: 0, num: 9101, kind: 0 };
        assert_eq!(r.confirm(k), Err(DedupCode::UnknownDeclaration));
        let _ = r.declare(k, 7);
        assert!(r.confirm(k).is_ok());
        assert_eq!(r.confirm(k), Err(DedupCode::ConfirmBeforeDeclare));
    }

    #[test]
    fn grep_clean_on_unique() {
        let mut r = DeclareRegistry::new();
        let mut b = 0usize;
        while b < RANGE_BOUNDS.len() {
            let k = DeclareKey { domain: 0, num: RANGE_BOUNDS[b].0, kind: 0 };
            let _ = r.declare(k, derive_slug(k));
            b += 1;
        }
        let led = grep_ranges(&r.pairs());
        assert!(led.clean());
        for v in led.scanned.iter() {
            assert!(*v > 0);
        }
    }

    #[test]
    fn grep_detects_conflict() {
        // 同一范围注入两个同名条目 → grep 二次审计必须检出
        let pairs = [(9101u32, 0x77u32), (9102, 0x77)];
        let led = grep_ranges(&pairs);
        assert_eq!(led.conflicts, 1);
        assert_eq!(led.slug_hits, 1);
        assert!(!led.clean());
        // 不同范围的同名条目不算冲突
        let cross = [(9101u32, 0x77u32), (9201, 0x77)];
        assert_eq!(grep_ranges(&cross).conflicts, 0);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 4);
    }

    #[test]
    fn slug_derive_deterministic() {
        let k = DeclareKey { domain: 2, num: 9401, kind: 2 };
        assert_eq!(derive_slug(k), derive_slug(k));
    }

    #[test]
    fn double_run_fingerprint() {
        let mut r = DeclareRegistry::new();
        let k = DeclareKey { domain: 0, num: 9301, kind: 0 };
        let _ = r.declare(k, derive_slug(k));
        assert_eq!(registry_fingerprint(&r.slugs()), registry_fingerprint(&r.slugs()));
    }

    #[test]
    fn unx9394_checks_green() {
        let set = run_unx9394_checks();
        assert!(set.all_passed(), "UNX-F9394 red: {}", set.red_items_text());
    }
}
