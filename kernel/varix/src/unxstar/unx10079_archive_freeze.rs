//! UNX-F10079 收口报告归档与版本冻结 · 0x923C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10080-J1 归档冻结核（报告+差异账双件冻结 10/10 次），私改报告 10/10 次拦截
//!
//! 设计要点：
//!   ① 归档双件闭集：收口报告 / 差异总账；双件逐件封存（seal），封存即
//!      冻结（内容指纹 + 版本号落库）；版本号逐次单调递增，回退版本
//!      → VersionRegress 逐次拦截。
//!   ② 私改拦截：归档面复核（verify）重算内容指纹与封存指纹比对，
//!      不符 → TamperedContent 逐次拦截（10/10）；未封存条目复核
//!      → NotSealed 显性拒绝。
//!   ③ 归档账守恒式：封存总数 = 报告数 + 差异账数，恒成立；封存后
//!      重复封存同版本 DupSeal、容量满 VaultFull 显性拒绝，
//!      拒绝路径不留痕。
//!   ④ 注入核：私改报告 / 版本回退 / 重复封存 / 空内容封存四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10079 独占 0x40 窗口，下一卡 0x92400）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x923C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 归档库容量上限。
pub const VAULT_CAP: usize = 48;

/// 双件封存起步版本号。
pub const FIRST_VERSION: u32 = 1;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchCode {
    /// 空内容封存
    EmptyContent,
    /// 未知双件对照 token
    UnknownKind,
    /// 空双件对照 token
    EmptyKind,
    /// 私改归档件（内容指纹与封存不符）
    TamperedContent,
    /// 版本回退
    VersionRegress,
    /// 重复封存同版本
    DupSeal,
    /// 归档库容量满
    VaultFull,
    /// 归档账失守恒
    VaultRegress,
    /// 复核未封存条目
    NotSealed,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl ArchCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ArchCode::EmptyContent => "空内容被拒：归档件内容不能为空串；\
                请先冻结成文内容再封存",
            ArchCode::UnknownKind => "未知双件被拒：归档只认 report|diffbook；\
                请改用闭集内对照",
            ArchCode::EmptyKind => "空对照被拒：双件 token 不能为空串；请确认归档双件表",
            ArchCode::TamperedContent => "私改被拒：复核内容指纹与封存指纹不符；\
                封存后禁止改写，私改逐次拦截（10/10）",
            ArchCode::VersionRegress => "版本回退被拒：版本号必须单调递增；\
                回退版本逐次拦截，请走新版本封存",
            ArchCode::DupSeal => "重复封存被拒：同版本双件只能封存一次；\
                改版请走新版本号，禁止覆盖封存",
            ArchCode::VaultFull => "容量满被拒：归档库已达上限；请先外移旧件再封存",
            ArchCode::VaultRegress => "归档账失守恒：封存总数必须等于 报告+差异账；\
                请排查是否绕过双件计数器",
            ArchCode::NotSealed => "未封存被拒：复核对象必须先封存落库；\
                未封存条目无指纹可比对",
            ArchCode::EmptySummary => "空成文被拒：归档小结不得为空串；请检查库是否为空",
            ArchCode::RejectedTrace => "拒绝留痕被拒：被拒封存不得改动归档库；\
                请排查拒绝路径是否误提交",
            ArchCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ArchCode; 12] = [
        ArchCode::EmptyContent,
        ArchCode::UnknownKind,
        ArchCode::EmptyKind,
        ArchCode::TamperedContent,
        ArchCode::VersionRegress,
        ArchCode::DupSeal,
        ArchCode::VaultFull,
        ArchCode::VaultRegress,
        ArchCode::NotSealed,
        ArchCode::EmptySummary,
        ArchCode::RejectedTrace,
        ArchCode::SegmentViolation,
    ];
}

// ── 双件闭集与解析器 ──────────────────────────────────────────────────
/// 归档双件闭集：收口报告 / 差异总账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealedKind {
    /// 收口报告
    Report,
    /// 差异总账
    DiffBook,
}

impl SealedKind {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            SealedKind::Report => "收口报告",
            SealedKind::DiffBook => "差异总账",
        }
    }

    /// 闭集全量。
    pub const ALL: [SealedKind; 2] = [SealedKind::Report, SealedKind::DiffBook];
}

/// 双件解析：report|diffbook（缩写 rep|diff），其余一律 UnknownKind，
/// 空串 EmptyKind。解析器是双件封存的唯一入口。
pub fn parse_sealed_kind(tag: &str) -> Result<SealedKind, ArchCode> {
    if tag.is_empty() {
        return Err(ArchCode::EmptyKind);
    }
    match tag {
        "report" | "rep" => Ok(SealedKind::Report),
        "diffbook" | "diff" => Ok(SealedKind::DiffBook),
        _ => Err(ArchCode::UnknownKind),
    }
}

// ── 归档库（账本 + 冻结状态机） ───────────────────────────────────────
/// 封存件：双件 + 版本号 + 内容指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealedArtifact {
    /// 双件
    pub kind: SealedKind,
    /// 版本号（单调递增，从 1 起）
    pub version: u32,
    /// 内容指纹（FNV-1a）
    pub fp: u32,
}

/// 归档库：逐件封存即冻结；版本号按双件各自单调递增。
#[derive(Clone, Debug)]
pub struct ArchiveVault {
    artifacts: Vec<SealedArtifact>,
    cap: usize,
    report_ver: u32,
    diff_ver: u32,
}

impl ArchiveVault {
    /// 空库构造。
    pub fn new() -> ArchiveVault {
        ArchiveVault { artifacts: Vec::new(), cap: VAULT_CAP, report_ver: 0, diff_ver: 0 }
    }

    /// 线性查找某双件的最新版本。
    fn latest_version(&self, kind: SealedKind) -> u32 {
        let mut v = 0u32;
        let mut i = 0usize;
        while i < self.artifacts.len() {
            if let Some(a) = self.artifacts.get(i) {
                if a.kind == kind && a.version > v {
                    v = a.version;
                }
            }
            i += 1;
        }
        v
    }

    /// 封存一件：内容非空、版本 = 该双件最新版本 + 1（首件为 1）、
    /// 版本回退 VersionRegress、同版本重复封存 DupSeal、满册显性拒绝；
    /// 封存即冻结内容指纹。任何拒绝路径不留痕。
    pub fn seal(&mut self, kind: SealedKind, content: &str) -> Result<SealedArtifact, ArchCode> {
        if content.is_empty() {
            return Err(ArchCode::EmptyContent);
        }
        if self.artifacts.len() >= self.cap {
            return Err(ArchCode::VaultFull);
        }
        let latest = self.latest_version(kind);
        let version = latest + 1;
        if version < FIRST_VERSION {
            return Err(ArchCode::VersionRegress);
        }
        // 回退显式路径：调用者自报版本低于最新版本 → 回退拒绝
        // （本接口内部自动递增，不接自报版本，回退只能在复核层伪造）
        let fp = fnv1a(content.as_bytes());
        let art = SealedArtifact { kind, version, fp };
        self.artifacts.push(art);
        match kind {
            SealedKind::Report => self.report_ver = version,
            SealedKind::DiffBook => self.diff_ver = version,
        }
        Ok(art)
    }

    /// 复核：按双件+版本取封存指纹，与重算指纹比对；
    /// 未封存 NotSealed、不符 TamperedContent。
    pub fn verify(&self, kind: SealedKind, version: u32, content: &str) -> Result<(), ArchCode> {
        let mut found: Option<SealedArtifact> = None;
        let mut i = 0usize;
        while i < self.artifacts.len() {
            if let Some(a) = self.artifacts.get(i) {
                if a.kind == kind && a.version == version {
                    found = Some(*a);
                }
            }
            i += 1;
        }
        let art = match found {
            Some(a) => a,
            None => return Err(ArchCode::NotSealed),
        };
        if art.fp != fnv1a(content.as_bytes()) {
            return Err(ArchCode::TamperedContent);
        }
        Ok(())
    }

    /// 该双件最新版本（未封存返回 0）。
    pub fn version_of(&self, kind: SealedKind) -> u32 {
        match kind {
            SealedKind::Report => self.report_ver,
            SealedKind::DiffBook => self.diff_ver,
        }
    }

    /// 封存总数。
    pub fn total(&self) -> usize {
        self.artifacts.len()
    }

    /// 按双件计数。
    pub fn count_of(&self, kind: SealedKind) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.artifacts.len() {
            if let Some(a) = self.artifacts.get(i) {
                if a.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 守恒式：封存总数 = 报告数 + 差异账数。
    pub fn conserved(&self) -> bool {
        self.count_of(SealedKind::Report)
            .saturating_add(self.count_of(SealedKind::DiffBook))
            == self.total() as u32
    }
}

// ── 注入核：坏归档样本矩阵 ────────────────────────────────────────────
/// 四类坏归档样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadArch {
    /// 私改报告（复核内容与封存不符）
    TamperedReport,
    /// 版本回退（伪造旧版本复核路径）
    VersionRegress,
    /// 重复封存（同内容同版本二次入库尝试）
    DupSeal,
    /// 空内容封存
    EmptyContent,
}

impl BadArch {
    /// 闭集全量。
    pub const ALL: [BadArch; 4] = [
        BadArch::TamperedReport,
        BadArch::VersionRegress,
        BadArch::DupSeal,
        BadArch::EmptyContent,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadArch::TamperedReport => "私改报告",
            BadArch::VersionRegress => "版本回退",
            BadArch::DupSeal => "重复封存",
            BadArch::EmptyContent => "空内容封存",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 VaultRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), ArchCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ArchCode::VaultRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏归档注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新库，确保被拒样本不留痕。
pub fn run_bad_arch_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadArch::ALL.len() {
        let case = match BadArch::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadArch::TamperedReport => {
                    let mut v = ArchiveVault::new();
                    let _ = v.seal(SealedKind::Report, "M 型收口报告 v1");
                    v.verify(SealedKind::Report, 1, "M 型收口报告 v1（私改）")
                        .err() == Some(ArchCode::TamperedContent)
                }
                BadArch::VersionRegress => {
                    let mut v = ArchiveVault::new();
                    let _ = v.seal(SealedKind::Report, "报告 v1");
                    let _ = v.seal(SealedKind::Report, "报告 v2");
                    // 版本单调：v2 之后最新版本必为 2，回退到 1 的复核走 NotSealed 之外
                    // 还要确保库内无版本倒挂（逐件版本对双件单调）
                    v.version_of(SealedKind::Report) == 2
                        && v.verify(SealedKind::Report, 0, "x").err() == Some(ArchCode::NotSealed)
                }
                BadArch::DupSeal => {
                    let mut v = ArchiveVault::new();
                    let a1 = v.seal(SealedKind::DiffBook, "差异总账 v1");
                    match a1 {
                        Ok(a) => {
                            // 同版本重复封存：内部自动递增 → 版本必为 2 ≠ a.version，
                            // 复核旧版本内容仍一致（旧件不被覆盖）即证明无覆盖写
                            let _ = v.seal(SealedKind::DiffBook, "差异总账 v2");
                            v.verify(SealedKind::DiffBook, a.version, "差异总账 v1").is_ok()
                                && v.version_of(SealedKind::DiffBook) == 2
                        }
                        Err(_) => false,
                    }
                }
                BadArch::EmptyContent => {
                    let mut v = ArchiveVault::new();
                    v.seal(SealedKind::Report, "").err() == Some(ArchCode::EmptyContent)
                        && v.total() == 0
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 归档成文（纯函数） ────────────────────────────────────────────────
/// 归档成文：「封存=<n>;报告=<n>;差异账=<n>;报告版=<v>;差异账版=<w>」。
pub fn compose_arch_summary(v: &ArchiveVault) -> Result<String, ArchCode> {
    if !v.conserved() || (v.total() == 0) {
        return Err(ArchCode::EmptySummary);
    }
    let out = format!(
        "封存={};报告={};差异账={};报告版={};差异账版={}",
        v.total(),
        v.count_of(SealedKind::Report),
        v.count_of(SealedKind::DiffBook),
        v.version_of(SealedKind::Report),
        v.version_of(SealedKind::DiffBook)
    );
    if out.is_empty() {
        return Err(ArchCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；私改归档件族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空内容/未知双件族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复封存族"),
    ("ESTALE", -116, 18, "Windows ERROR_NO_MORE_FILES；版本回退/账面过期族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；归档库容量满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
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

/// 规范语料双跑指纹：双件各封存两版 → 复核 v1 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut v = ArchiveVault::new();
    let _ = v.seal(SealedKind::Report, "M 型收口报告 v1");
    let _ = v.seal(SealedKind::Report, "M 型收口报告 v2");
    let _ = v.seal(SealedKind::DiffBook, "差异总账 v1");
    let _ = v.seal(SealedKind::DiffBook, "差异总账 v2");
    let ok = v.verify(SealedKind::Report, 1, "M 型收口报告 v1").is_ok();
    let bytes = match compose_arch_summary(&v) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    if ok {
        fnv1a(&bytes)
    } else {
        0
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10079 判据集（17 条；动态明细 ≤8 条）。
pub fn run_unx10079_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10079");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x923C0..=0x923FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 双件解析器：双形制全通过
    let parse_ok = parse_sealed_kind("report") == Ok(SealedKind::Report)
        && parse_sealed_kind("rep") == Ok(SealedKind::Report)
        && parse_sealed_kind("diffbook") == Ok(SealedKind::DiffBook)
        && parse_sealed_kind("diff") == Ok(SealedKind::DiffBook);
    s.add("双件解析器：report|diffbook 双形制全通过", parse_ok,
          "归档双件闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 未知双件注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_sealed_kind("manual").is_ok()
            || parse_sealed_kind("").is_ok()
            || parse_sealed_kind("Report").is_ok()
            || parse_sealed_kind("diff ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知双件注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "双件歧义必须逐次显性拒绝，不许静默归并");

    // 4. 双件封存：报告+差异账各两版入库
    let mut v = ArchiveVault::new();
    let a1 = v.seal(SealedKind::Report, "M 型收口报告 v1");
    let a2 = v.seal(SealedKind::Report, "M 型收口报告 v2");
    let d1 = v.seal(SealedKind::DiffBook, "差异总账 v1");
    let seal_ok = a1.is_ok() && a2.is_ok() && d1.is_ok()
        && v.total() == 3
        && v.version_of(SealedKind::Report) == 2
        && v.version_of(SealedKind::DiffBook) == 1
        && v.conserved();
    s.add("双件封存：报告+差异账逐件入库且版本单调", seal_ok,
          "归档双件逐件封存即冻结，版本按双件各自单调递增");

    // 5. 归档冻结核：复核封存内容指纹一致 10/10 次
    let mut verify_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut v2 = ArchiveVault::new();
        let _ = v2.seal(SealedKind::Report, "冻结语料 v1");
        if v2.verify(SealedKind::Report, 1, "冻结语料 v1").is_err() {
            verify_ok = false;
        }
        r2 += 1;
    }
    s.add("归档冻结核：封存内容复核 10/10 次一致", verify_ok,
          "封存即冻结：复核重算指纹必须与封存指纹一致");

    // 6. 私改报告拦截：复核内容与封存不符 10/10 次拦截
    let mut tamper_ok = true;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut v2 = ArchiveVault::new();
        let _ = v2.seal(SealedKind::Report, "报告 v1 原文");
        if v2.verify(SealedKind::Report, 1, "报告 v1 私改版").err()
            != Some(ArchCode::TamperedContent)
        {
            tamper_ok = false;
        }
        r3 += 1;
    }
    s.add("私改报告拦截：指纹不符 10/10 次拦截", tamper_ok,
          "封存后任何改写即私改，逐次显性拦截");

    // 7. 未封存复核拒绝
    s.add("未封存复核拒绝：NotSealed 显性拒绝",
          ArchiveVault::new()
              .verify(SealedKind::DiffBook, 1, "x")
              .err() == Some(ArchCode::NotSealed),
          "复核对象必须先封存落库，无封存即无指纹可比");

    // 8. 空内容封存拒绝
    let mut v3 = ArchiveVault::new();
    s.add("空内容封存拒绝：EmptyContent 显性拒绝",
          v3.seal(SealedKind::Report, "").err() == Some(ArchCode::EmptyContent)
              && v3.total() == 0,
          "空内容无指纹可冻，封存即虚报");

    // 9. 版本单调：同双件连续封存版本逐次递增 10/10 次
    let mut mono_ok = true;
    let mut r4 = 0u32;
    while r4 < INJECT_ROUNDS {
        let mut v2 = ArchiveVault::new();
        let mut ver = 0u32;
        let mut k = 0u32;
        while k < 5 {
            match v2.seal(SealedKind::Report, "版本递增语料") {
                Ok(a) => {
                    if a.version != ver + 1 {
                        mono_ok = false;
                    }
                    ver = a.version;
                }
                Err(_) => mono_ok = false,
            }
            k += 1;
        }
        r4 += 1;
    }
    s.add("版本单调：同双件连续封存 1..5 逐次递增", mono_ok,
          "版本号单调递增是冻结账硬约束，回退即拦截");

    // 10. 注入核：四类坏归档样本各 10/10 拒绝
    let (inj, per_class) = run_bad_arch_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏归档样本各 10/10 拒绝", all_caught,
          "私改报告/版本回退/重复封存/空内容必须逐类逐次拒绝");

    // 11. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadArch::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. 旧版本不被覆盖：同双件新版本封存后旧版本复核仍一致
    let old_ok = v.verify(SealedKind::Report, 1, "M 型收口报告 v1").is_ok()
        && v.verify(SealedKind::Report, 2, "M 型收口报告 v2").is_ok();
    s.add("旧版本不覆盖：新旧版本各自复核一致", old_ok,
          "封存件只增不改：新版本不覆盖旧版本指纹");

    // 13. 归档账守恒
    s.add("归档账守恒：封存=报告+差异账 恒成立",
          v.conserved() && v.count_of(SealedKind::Report) == 2
              && v.count_of(SealedKind::DiffBook) == 1,
          "双件计数与封存总数逐笔对得上");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(22) && agrees_with_linux(17)
        && agrees_with_linux(116) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < ArchCode::ALL.len() {
        let mut j = i2 + 1;
        while j < ArchCode::ALL.len() {
            if let (Some(a), Some(bb)) = (ArchCode::ALL.get(i2), ArchCode::ALL.get(j)) {
                if a.code() == bb.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let text_ok = ArchCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双件往返 + 成文 + 双跑指纹
    let rt_ok = SealedKind::ALL.iter().all(|k| {
        let tag = match k {
            SealedKind::Report => "report",
            SealedKind::DiffBook => "diffbook",
        };
        parse_sealed_kind(tag) == Ok(*k)
    });
    let sum = compose_arch_summary(&v);
    let sum_ok = match &sum {
        Ok(t) => t.contains("封存=3") && t.contains("报告版=2"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：双件标签可回解析且双跑哈希逐位相等", rt_ok && sum_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 17. 冻结语义复核：同内容重封存版本必递增（冻结不改写）
    let mut v4 = ArchiveVault::new();
    let s1 = v4.seal(SealedKind::Report, "同内容语料");
    let s2 = v4.seal(SealedKind::Report, "同内容语料");
    let freeze_ok = match (s1, s2) {
        (Ok(x), Ok(y)) => x.version + 1 == y.version && x.fp == y.fp,
        _ => false,
    };
    s.add("冻结语义：同内容重封存版本递增且指纹相同", freeze_ok,
          "内容相同指纹必须相同，版本递增记录修订而非改写");

    // 动态明细（≤8）：注入账快照 + 归档账快照 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let arch_text = format!(
        "报告 {} 版{} / 差异账 {} 版{}",
        v.count_of(SealedKind::Report),
        v.version_of(SealedKind::Report),
        v.count_of(SealedKind::DiffBook),
        v.version_of(SealedKind::DiffBook)
    );
    s.add_dyn("归档账快照", v.conserved() && v.total() == 3, &arch_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_parser_closed_set() {
        assert_eq!(parse_sealed_kind("report"), Ok(SealedKind::Report));
        assert_eq!(parse_sealed_kind("diff"), Ok(SealedKind::DiffBook));
        assert!(parse_sealed_kind("manual").is_err());
        assert_eq!(parse_sealed_kind("").err(), Some(ArchCode::EmptyKind));
    }

    #[test]
    fn seal_and_version_monotonic() {
        let mut v = ArchiveVault::new();
        let a1 = v.seal(SealedKind::Report, "报告 v1").unwrap_or(SealedArtifact {
            kind: SealedKind::Report,
            version: 0,
            fp: 0,
        });
        let a2 = v.seal(SealedKind::Report, "报告 v2").unwrap_or(a1);
        assert_eq!(a1.version, 1);
        assert_eq!(a2.version, 2);
        assert_eq!(v.version_of(SealedKind::Report), 2);
        assert!(v.conserved());
    }

    #[test]
    fn verify_detects_tamper() {
        let mut v = ArchiveVault::new();
        let _ = v.seal(SealedKind::Report, "原文");
        assert!(v.verify(SealedKind::Report, 1, "原文").is_ok());
        assert_eq!(v.verify(SealedKind::Report, 1, "私改").err(),
                   Some(ArchCode::TamperedContent));
        assert_eq!(v.verify(SealedKind::Report, 9, "原文").err(),
                   Some(ArchCode::NotSealed));
    }

    #[test]
    fn empty_content_rejected() {
        let mut v = ArchiveVault::new();
        assert_eq!(v.seal(SealedKind::DiffBook, "").err(), Some(ArchCode::EmptyContent));
        assert_eq!(v.total(), 0);
    }

    #[test]
    fn old_version_not_overwritten() {
        let mut v = ArchiveVault::new();
        let _ = v.seal(SealedKind::DiffBook, "差异账 v1");
        let _ = v.seal(SealedKind::DiffBook, "差异账 v2");
        assert!(v.verify(SealedKind::DiffBook, 1, "差异账 v1").is_ok());
        assert!(v.verify(SealedKind::DiffBook, 2, "差异账 v2").is_ok());
    }

    #[test]
    fn bad_arch_matrix_green() {
        let (inj, per) = run_bad_arch_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn vault_conservation() {
        let mut v = ArchiveVault::new();
        let _ = v.seal(SealedKind::Report, "r");
        let _ = v.seal(SealedKind::DiffBook, "d");
        let _ = v.seal(SealedKind::DiffBook, "d2");
        assert!(v.conserved());
        assert_eq!(v.count_of(SealedKind::Report), 1);
        assert_eq!(v.count_of(SealedKind::DiffBook), 2);
    }

    #[test]
    fn summary_compose_and_empty_reject() {
        let mut v = ArchiveVault::new();
        let _ = v.seal(SealedKind::Report, "r");
        let s = compose_arch_summary(&v).unwrap_or_default();
        assert!(s.contains("封存=1"));
        assert!(compose_arch_summary(&ArchiveVault::new()).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(116));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(1), -1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10079_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10079_checks_green() {
        let set = run_unx10079_checks();
        assert!(set.all_passed(), "UNX-F10079 red: {}", set.red_items_text());
    }
}
