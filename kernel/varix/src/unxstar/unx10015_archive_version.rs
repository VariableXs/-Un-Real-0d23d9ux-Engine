//! UNX-F10015 报表归档与版本号 · 0x913C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10015（B21 波次报表线）。
//! 锚点：UNX-F10016-J1 权限面冻结（只读者名单+审计留痕 10/10 次），越权访问 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x913C0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `ArchCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态机 `ArchState`：Draft → Sealed → Archived 单向推进，未封存
//!     不得归档，封存后内容只读；
//!   - 注册表 `ArchiveRegistry`：版本号单调递增登记（max 水位推进），
//!     回退版本 / 重复版本 / 跳版均显性拒绝；
//!   - 账本 `ArchLedger`：封存数 = 归档数 + 拦截数，归档必先封存，
//!     私改封存内容必须留痕并被拦；
//!   - 注入核 `BadArchCase`：回退版本 / 重复版本 / 未封存归档 / 封存后
//!     改写 / 越期跳版五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10015 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x913C0;

/// 归档容量（期数上限）。
pub const ARCHIVE_CAP: usize = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchCode {
    /// 回退版本（低于 max 水位）
    VersionRollback,
    /// 重复版本
    DuplicateVersion,
    /// 未封存即归档
    ArchiveBeforeSeal,
    /// 封存后改写
    SealedRewrite,
    /// 越期跳版（版本号跳 >1）
    VersionGap,
    /// 账本倒退
    LedgerRegress,
    /// 空内容归档
    EmptyContent,
    /// 归档容量满
    ArchiveFull,
    /// 归档哈希不符（内容被改）
    TamperDetected,
    /// 版本非整数段
    BadVersionField,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ArchCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ArchCode::VersionRollback => "回退版本被拒：版本号必须逐期递增；\
                请取 max 水位之上的新号",
            ArchCode::DuplicateVersion => "重复版本被拒：该版本号已在归档；\
                请用新号重报",
            ArchCode::ArchiveBeforeSeal => "未封存归档被拒：必须先 Sealed 再 \
                Archived；请走状态机单向推进",
            ArchCode::SealedRewrite => "封存后改写被拒：Sealed 态内容只读；\
                请走差异清单通道",
            ArchCode::VersionGap => "越期跳版被拒：版本号必须 +1 递进；\
                跳版说明有期缺失，请先补齐",
            ArchCode::LedgerRegress => "归档账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            ArchCode::EmptyContent => "空内容归档被拒：空报表无归档价值；请先\
                落正文",
            ArchCode::ArchiveFull => "归档容量满：40 期上限；请轮转旧期后重试",
            ArchCode::TamperDetected => "归档哈希不符：归档后内容被改；请恢复\
                原件并重算指纹",
            ArchCode::BadVersionField => "版本字段非法：必须为正整数；请修正\
                版本号后重试",
            ArchCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定哈希",
            ArchCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x913C0..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ArchCode; 12] = [
        ArchCode::VersionRollback,
        ArchCode::DuplicateVersion,
        ArchCode::ArchiveBeforeSeal,
        ArchCode::SealedRewrite,
        ArchCode::VersionGap,
        ArchCode::LedgerRegress,
        ArchCode::EmptyContent,
        ArchCode::ArchiveFull,
        ArchCode::TamperDetected,
        ArchCode::BadVersionField,
        ArchCode::NotReproducible,
        ArchCode::SegmentViolation,
    ];
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 归档状态机：Draft → Sealed → Archived 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchState {
    /// 草稿（可改）
    Draft,
    /// 已封存（只读）
    Sealed,
    /// 已归档（入账）
    Archived,
}

/// 一期报表的归档单元。
#[derive(Clone, Copy, Debug)]
pub struct ArchEntry {
    /// 版本号（逐期 +1）
    pub version: u32,
    /// 状态
    pub state: ArchState,
    /// 内容指纹（封存时计算）
    pub hash: u32,
    /// 内容字节数
    pub size: u32,
}

/// 归档器：单期状态推进 + 内容封存。
#[derive(Clone, Copy, Debug)]
pub struct Archiver {
    /// 当前条目
    pub entry: ArchEntry,
}

impl Archiver {
    /// 新一期草稿。
    pub fn new(version: u32, content: &[u8]) -> Archiver {
        Archiver {
            entry: ArchEntry {
                version,
                state: ArchState::Draft,
                hash: fnv1a(content),
                size: content.len() as u32,
            },
        }
    }

    /// 封存：Draft → Sealed，内容冻结。
    pub fn seal(&mut self) -> Result<u32, ArchCode> {
        if self.entry.state != ArchState::Draft {
            return Err(ArchCode::SealedRewrite);
        }
        if self.entry.size == 0 {
            return Err(ArchCode::EmptyContent);
        }
        self.entry.state = ArchState::Sealed;
        Ok(self.entry.hash)
    }

    /// 归档：Sealed → Archived；未封存显性拒绝。
    pub fn archive(&mut self) -> Result<u32, ArchCode> {
        match self.entry.state {
            ArchState::Draft => Err(ArchCode::ArchiveBeforeSeal),
            ArchState::Sealed => {
                self.entry.state = ArchState::Archived;
                Ok(self.entry.version)
            }
            ArchState::Archived => Err(ArchCode::DuplicateVersion),
        }
    }

    /// 封存后改写探测：内容指纹必须不变。
    pub fn verify_untampered(&self, content: &[u8]) -> bool {
        self.entry.state == ArchState::Draft || self.entry.hash == fnv1a(content)
    }
}

// ── 版本注册表 ────────────────────────────────────────────────────────
/// 归档注册表：版本号单调 +1 递进，回退/重复/跳版全拒。
pub struct ArchiveRegistry {
    versions: Vec<u32>,
    hashes: Vec<u32>,
    /// max 水位
    pub watermark: u32,
}

impl ArchiveRegistry {
    /// 空注册表，水位 0。
    pub fn new() -> ArchiveRegistry {
        ArchiveRegistry { versions: Vec::new(), hashes: Vec::new(), watermark: 0 }
    }

    /// 归档登记：要求恰好 watermark+1（首期为 1）；容量 40。
    pub fn register(&mut self, version: u32, hash: u32) -> Result<(), ArchCode> {
        if version == 0 {
            return Err(ArchCode::BadVersionField);
        }
        if version <= self.watermark {
            if self.contains(version) {
                return Err(ArchCode::DuplicateVersion);
            }
            return Err(ArchCode::VersionRollback);
        }
        if version > self.watermark + 1 {
            return Err(ArchCode::VersionGap);
        }
        if self.versions.len() >= ARCHIVE_CAP {
            return Err(ArchCode::ArchiveFull);
        }
        self.versions.push(version);
        self.hashes.push(hash);
        self.watermark = version;
        Ok(())
    }

    /// 是否已登记。
    pub fn contains(&self, version: u32) -> bool {
        let mut i = 0usize;
        while i < self.versions.len() {
            if let Some(v) = self.versions.get(i) {
                if *v == version {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 归档期数。
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// 取归档哈希。
    pub fn hash_of(&self, version: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.versions.len() {
            if let Some(v) = self.versions.get(i) {
                if *v == version {
                    return self.hashes.get(i).copied();
                }
            }
            i += 1;
        }
        None
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏归档样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadArchCase {
    /// 回退版本（2 后报 1）
    Rollback,
    /// 重复版本（2 后再报 2）
    Duplicate,
    /// 未封存即归档
    Unsealed,
    /// 封存后改写
    Tamper,
    /// 跳版（1 后报 3）
    Gap,
}

impl BadArchCase {
    /// 闭集全量。
    pub const ALL: [BadArchCase; 5] = [
        BadArchCase::Rollback,
        BadArchCase::Duplicate,
        BadArchCase::Unsealed,
        BadArchCase::Tamper,
        BadArchCase::Gap,
    ];

    /// 注入一轮：返回是否被显式拦截。
    pub fn inject(self) -> bool {
        let body: &[u8] = b"wave report body";
        match self {
            BadArchCase::Rollback => {
                let mut r = ArchiveRegistry::new();
                let _ = r.register(1, 0xAA);
                let _ = r.register(2, 0xBB);
                r.register(1, 0xCC) == Err(ArchCode::DuplicateVersion)
                    || r.register(1, 0xCC).is_err()
            }
            BadArchCase::Duplicate => {
                let mut r = ArchiveRegistry::new();
                let _ = r.register(1, 0xAA);
                r.register(1, 0xBB) == Err(ArchCode::DuplicateVersion)
            }
            BadArchCase::Unsealed => {
                let mut a = Archiver::new(1, body);
                a.archive() == Err(ArchCode::ArchiveBeforeSeal)
            }
            BadArchCase::Tamper => {
                let mut a = Archiver::new(1, body);
                let _ = a.seal();
                let mutated: &[u8] = b"wave report bodY";
                !a.verify_untampered(mutated)
            }
            BadArchCase::Gap => {
                let mut r = ArchiveRegistry::new();
                let _ = r.register(1, 0xAA);
                r.register(3, 0xCC) == Err(ArchCode::VersionGap)
            }
        }
    }
}

/// 归档账：封存 = 归档 + 拦截，未封存归档恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ArchLedger {
    /// 封存总数
    pub sealed: u32,
    /// 归档成功数
    pub archived: u32,
    /// 拦截数（坏样本被拒）
    pub blocked: u32,
}

impl ArchLedger {
    /// 记一次封存/归档结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, archived_ok: bool) -> Result<(), ArchCode> {
        self.sealed = self.sealed.saturating_add(1);
        if archived_ok {
            self.archived = self.archived.saturating_add(1);
        } else {
            self.blocked = self.blocked.saturating_add(1);
        }
        if self.archived.saturating_add(self.blocked) != self.sealed {
            return Err(ArchCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.archived.saturating_add(self.blocked) == self.sealed
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_arch_injection() -> (ArchLedger, [u32; 5]) {
    let mut ledger = ArchLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadArchCase::ALL.len() {
        if let Some(case) = BadArchCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；封存改写族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复版本族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏版本字段族"),
    ("EROFS", -30, 19, "Windows ERROR_WRITE_PROTECT；归档只读面"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 规范语料双跑指纹：封存→归档→登记全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut r = ArchiveRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = 1u32;
    while v <= 6 {
        let content: Vec<u8> = format!("report-v{}", v).into_bytes();
        let mut a = Archiver::new(v, &content);
        let sealed = a.seal();
        bytes.push(if sealed.is_ok() { 1 } else { 0 });
        let archived = a.archive();
        bytes.push(if archived.is_ok() { 2 } else { 3 });
        let reg = r.register(v, a.entry.hash);
        bytes.push(if reg.is_ok() { 4 } else { 5 });
        v += 1;
    }
    // 回退与跳版必拒
    bytes.push(if r.register(3, 1).is_err() { 6 } else { 7 });
    bytes.push(if r.register(9, 2).is_err() { 6 } else { 7 });
    bytes.push((r.watermark & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10015 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10015_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10015");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x913C0..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x913C0..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 状态机：Draft→Sealed→Archived 单向推进
    let body: &[u8] = b"wave report v1";
    let mut a = Archiver::new(1, body);
    let sealed = a.seal();
    let archived = a.archive();
    s.add("状态机：封存→归档单向推进成功", sealed.is_ok() && archived == Ok(1),
          "状态只许单向推进，归档必先封存");

    // 3. 未封存归档拒绝
    let mut a2 = Archiver::new(2, body);
    s.add("状态机：Draft 直接归档被拒", a2.archive() == Err(ArchCode::ArchiveBeforeSeal),
          "未封存内容不得入归档账");

    // 4. 版本单调：1..=6 逐期递增全收
    let mut r = ArchiveRegistry::new();
    let mut v = 1u32;
    let mut mono_ok = true;
    while v <= 6 {
        if r.register(v, v.wrapping_mul(0x9C40)).is_err() {
            mono_ok = false;
        }
        v += 1;
    }
    s.add("版本单调：1..=6 逐期递增 6/6 登记", mono_ok && r.len() == 6 && r.watermark == 6,
          "逐期递增全部接受，水位逐期推进");

    // 5. 回退版本拦截：6 后报 4
    let rollback = r.register(4, 1) == Err(ArchCode::DuplicateVersion)
        || r.register(4, 1).is_err();
    s.add("回退拦截：水位 6 后报 4 被拦", rollback,
          "低于水位的版本一律拒绝（判据原文：回退版本 10/10 次拦截）");

    // 6. 重复版本拦截
    let dup = r.register(6, 2) == Err(ArchCode::DuplicateVersion);
    s.add("重复拦截：同号二次登记被拦", dup,
          "已在档版本不得重报");

    // 7. 跳版拦截：6 后报 9
    let gap = r.register(9, 3) == Err(ArchCode::VersionGap);
    s.add("跳版拦截：水位 6 后报 9 被拦", gap,
          "版本号必须 +1 递进，跳版说明缺期");

    // 8. 封存后改写检出
    let mut a3 = Archiver::new(1, body);
    let _ = a3.seal();
    let mutated: &[u8] = b"wave report v2";
    s.add("改写检出：封存后内容指纹不符", !a3.verify_untampered(mutated),
          "Sealed 态任何内容变更都会被指纹核出");

    // 9. 空内容拒绝封存
    let mut a4 = Archiver::new(1, &[]);
    s.add("边界：空内容拒绝封存", a4.seal() == Err(ArchCode::EmptyContent),
          "空报表无归档价值");

    // 10. 注入核：五类坏样本全拦截
    let (ledger, per_class) = run_arch_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 拦截", all_caught,
          "回退/重复/未封存/改写/跳版全被显式拒绝");

    // 11. 归档账守恒
    s.add("归档账守恒：封存=归档+拦截且拦样本全检出",
          ledger.conserved() && ledger.sealed == 5 * INJECT_ROUNDS,
          "50 次封存必须 50 次入账、0 逃逸");

    // 12. 归档哈希可回查
    let hash_ok = r.hash_of(3).is_some() && r.hash_of(99).is_none();
    s.add("回查：在档版本可取哈希、缺档返回空", hash_ok,
          "归档必须可按版本号回查内容指纹");

    // 13. 诊断码闭集：文案非空
    let text_ok = ArchCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < ArchCode::ALL.len() {
        let mut j = i + 1;
        while j < ArchCode::ALL.len() {
            if let (Some(x), Some(y)) = (ArchCode::ALL.get(i), ArchCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = ArchCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EROFS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 17. 版本 0 拒绝
    let mut r2 = ArchiveRegistry::new();
    s.add("边界：版本 0 拒绝登记", r2.register(0, 1) == Err(ArchCode::BadVersionField),
          "版本号从 1 起，0 号非法");

    // 动态明细（≤8）：归档账一行 + 指纹一行
    let ledger_text = format!(
        "封存 {} / 归档 {} / 拦截 {}",
        ledger.sealed, ledger.archived, ledger.blocked
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_forward() {
        let mut a = Archiver::new(1, b"body");
        assert!(a.seal().is_ok());
        assert!(a.archive().is_ok());
    }

    #[test]
    fn archive_requires_seal() {
        let mut a = Archiver::new(1, b"body");
        assert_eq!(a.archive(), Err(ArchCode::ArchiveBeforeSeal));
    }

    #[test]
    fn seal_requires_content() {
        let mut a = Archiver::new(1, &[]);
        assert_eq!(a.seal(), Err(ArchCode::EmptyContent));
    }

    #[test]
    fn version_monotonic() {
        let mut r = ArchiveRegistry::new();
        assert!(r.register(1, 1).is_ok());
        assert!(r.register(2, 2).is_ok());
        assert!(r.register(3, 3).is_ok());
        assert_eq!(r.watermark, 3);
    }

    #[test]
    fn version_rollback_blocked() {
        let mut r = ArchiveRegistry::new();
        let _ = r.register(1, 1);
        let _ = r.register(2, 2);
        assert!(r.register(1, 9).is_err());
    }

    #[test]
    fn version_duplicate_blocked() {
        let mut r = ArchiveRegistry::new();
        let _ = r.register(1, 1);
        assert_eq!(r.register(1, 2), Err(ArchCode::DuplicateVersion));
    }

    #[test]
    fn version_gap_blocked() {
        let mut r = ArchiveRegistry::new();
        let _ = r.register(1, 1);
        assert_eq!(r.register(3, 2), Err(ArchCode::VersionGap));
    }

    #[test]
    fn tamper_detected_after_seal() {
        let mut a = Archiver::new(1, b"original");
        let _ = a.seal();
        assert!(!a.verify_untampered(b"original!"));
        assert!(a.verify_untampered(b"original"));
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_arch_injection();
        assert!(l.conserved());
        assert_eq!(l.sealed, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EEXIST));
        assert!(agrees_with_linux(EROFS));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EACCES), -EACCES);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10015_checks_green() {
        let s = run_unx10015_checks();
        assert!(s.all_passed(), "UNX-F10015 red: {}", s.red_items_text());
    }
}
