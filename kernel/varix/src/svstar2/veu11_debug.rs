//! VE-F4011 · 国际化调试器（VE-U 域 · 国际化组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4011`
//!
//! **判据（锚点原文六条）**：伪本地化、硬编码检出、方向可视化、
//! 家族二十七、零常态、判据。
//!
//! i18n 调试器 = Locale/方向/格式三维检视的**五列仪表** + **伪本地化
//! 引擎**（伪字符+膨胀模拟）+ **方向边界可视化**（复用 F4023 单源——
//! F4023 落位前以注入方向段为兑现点）+ **家族二十七成员复述协议**
//! （F2815 家族范式：三要素校验、立案流转、上游契约哈希对账）。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：伪本地化是**可断言的变换**，不是视觉把戏
//!
//! 伪变换 = 伪字符包裹（`[!!`…`!!]`）+ 字符映射 + 膨胀模拟（目标
//! +40% 定长填充）。膨胀比落在校准窗口 [`RATIO_MIN`]…[`RATIO_MAX`]
//! 内才放行；越窗 = `PseudoRatioDistorted`（模拟失真→校准，锚点降级
//! 矩阵原文）。伪检 O(字符串)：逐字符映射一次扫描，无递归无重扫。
//!
//! ## 要点二：硬编码检出的**漏报红线靠注入审计实测**
//!
//! 伪检模式开启后，一切渲染串都该带伪标记——**不带标记 = 硬编码
//! 嫌疑，立案**。漏报红线用注入审计实测：注入已知硬编码串，
//! 审计器**必须**立案（判据断言立案数增长）；带标记但不匹配任何
//! 已注册伪条目 = 规则漏检 → **规则版本号 +1**（规则修正，锚点
//! 错误路径原文），修正永远留痕不静默。
//!
//! ## 要点三：方向可视化复用**单源方向段**，边界模糊即立案
//!
//! 方向段（LTR/RTL）由单源注入（F4023 混合方向文本落位后由其喂养；
//! 复用 [`veu10_corpus::is_mixed_direction`] 做混合判定）。相邻异向
//! 边界**必须**带隔离标记——关闭标记生成再断言 = `BoundaryUnclear`
//! 立案（边界红线实测）。
//!
//! ## 要点四：家族二十七成员**复述单源**，契约哈希 O(1) 对账
//!
//! 家族 27 成员各自复述同一单源（源串+摘要）；上游契约按摘要对账，
//! O(1) 直取比对（成员下标直寻），不符 = `FamilyHashMismatch` 立案
//! （F2815 家族范式：哈希对账）。
//!
//! ## 要点五：五列仪表 O(列)，分歧立案
//!
//! 五列 = Locale/方向/格式产物/家族态/伪检态。同 Locale 两行方向
//! 不一致 = 仪表分歧 → `InspectorDiverged` 立案（锚点降级矩阵原文）。
//!
//! ## 要点六：零常态——**打开才采样**
//!
//! 调试器关闭时采样计数恒 0（不做任何仪表渲染与伪检扫描）；打开
//! 才计数。判据「零常态」= 关闭态采样计数恒 0 逐次断言。
//!
//! ## 要点七：诊断码独占 0x37xx 段
//!
//! 与 u 域既有 0x3Dxx 及全仓其他段互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veu10_corpus::CorpusLib;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 伪标记前缀。
pub const PSEUDO_PREFIX: &str = "[!!";
/// 伪标记后缀。
pub const PSEUDO_SUFFIX: &str = "!!]";
/// 膨胀目标（+40%）。
pub const EXPANSION_PERMILLE: u64 = 1400;
/// 膨胀比校准窗口下限（千分比）。
pub const RATIO_MIN: u64 = 1100;
/// 膨胀比校准窗口上限（千分比）。
pub const RATIO_MAX: u64 = 2500;

/// 家族成员数（家族二十七）。
pub const FAMILY_MEMBERS: usize = 27;

/// 仪表列数（五列）。
pub const INSPECTOR_COLS: usize = 5;

/// 立案簿上限（超出如实拒绝立案并计数——不静默吞案）。
pub const CASE_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 诊断码（0x37xx 独占段）
// ---------------------------------------------------------------------------

/// i18n 调试域错误（**自建诊断码**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IErr {
    /// 伪本地化膨胀比失真（校准窗口外）。
    PseudoRatioDistorted,
    /// 硬编码嫌疑已立案（不是错误，是可观测检出事件）。
    HardcodeFlagged,
    /// 带标记但不匹配已注册伪条目（规则漏检→修正）。
    UnknownMark,
    /// 仪表分歧（同 Locale 方向不一致）。
    InspectorDiverged,
    /// 方向边界模糊（隔离标记缺失）。
    BoundaryUnclear,
    /// 家族契约哈希不符。
    FamilyHashMismatch,
    /// 家族成员槽位越界。
    BadMemberSlot,
    /// 立案簿满（如实拒案）。
    CaseFull,
    /// 空方向源（可视化无输入）。
    DirSourceEmpty,
}

impl IErr {
    pub const fn code(self) -> u32 {
        match self {
            IErr::PseudoRatioDistorted => 0x3701,
            IErr::HardcodeFlagged => 0x3702,
            IErr::UnknownMark => 0x3703,
            IErr::InspectorDiverged => 0x3704,
            IErr::BoundaryUnclear => 0x3705,
            IErr::FamilyHashMismatch => 0x3706,
            IErr::BadMemberSlot => 0x3707,
            IErr::CaseFull => 0x3708,
            IErr::DirSourceEmpty => 0x3709,
        }
    }
    /// 专属 reason（不共用占位串）。
    pub fn reason(self) -> String {
        match self {
            IErr::PseudoRatioDistorted => String::from("伪本地化膨胀比失真"),
            IErr::HardcodeFlagged => String::from("硬编码嫌疑已立案"),
            IErr::UnknownMark => String::from("带标记但不匹配已注册伪条目"),
            IErr::InspectorDiverged => String::from("仪表同 Locale 方向分歧"),
            IErr::BoundaryUnclear => String::from("方向边界隔离标记缺失"),
            IErr::FamilyHashMismatch => String::from("家族契约哈希不符"),
            IErr::BadMemberSlot => String::from("家族成员槽位越界"),
            IErr::CaseFull => String::from("立案簿满如实拒案"),
            IErr::DirSourceEmpty => String::from("方向源为空无可视化"),
        }
    }
    pub const ALL: [IErr; 9] = [
        IErr::PseudoRatioDistorted,
        IErr::HardcodeFlagged,
        IErr::UnknownMark,
        IErr::InspectorDiverged,
        IErr::BoundaryUnclear,
        IErr::FamilyHashMismatch,
        IErr::BadMemberSlot,
        IErr::CaseFull,
        IErr::DirSourceEmpty,
    ];
}

/// 一条立案（F2815 家族范式：立案流转——案号+码+摘要）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseFile {
    /// 案号（自增）。
    pub case_id: u32,
    /// 立案码。
    pub code: u32,
    /// 摘要（列号或成员号或边界号，0=未指明）。
    pub ref_id: u32,
}

// ---------------------------------------------------------------------------
// 伪本地化引擎（要点一/二）
// ---------------------------------------------------------------------------

/// 字符伪映射表（显式映射，禁散落位算术；覆盖 ASCII 子集演示语义）。
fn pseudo_char(c: char) -> char {
    match c {
        'a' => 'à', 'e' => 'é', 'i' => 'î', 'o' => 'ö', 'u' => 'ü',
        'A' => 'À', 'E' => 'É', 'O' => 'Ö', 'U' => 'Ü',
        _ => c,
    }
}

/// 字符数（O(字符串) 单次扫描）。
fn char_len(s: &str) -> u64 {
    s.chars().count() as u64
}

/// 伪本地化变换：伪字符包裹 + 字符映射 + 膨胀模拟。
/// 返回 (伪串, 膨胀比千分比)；空输入 = 失真拒绝（伪检零长度无意义）。
pub fn pseudo_transform(text: &str) -> Result<(String, u64), IErr> {
    let in_len = char_len(text);
    if in_len == 0 {
        return Err(IErr::PseudoRatioDistorted);
    }
    let mut out = String::from(PSEUDO_PREFIX);
    let mut it = text.chars();
    while let Some(c) = it.next() {
        out.push(pseudo_char(c));
    }
    // 膨胀模拟：补足目标 +40% 的填充字符（定长确定性填充）。
    let target = in_len * EXPANSION_PERMILLE / 1000;
    let body_len = in_len;
    let mut extra = 0u64;
    if target > body_len {
        extra = target - body_len;
    }
    let mut k = 0u64;
    while k < extra {
        out.push('·');
        k += 1;
    }
    out.push_str(PSEUDO_SUFFIX);
    let out_len = char_len(&out);
    let ratio = out_len * 1000 / in_len;
    if ratio < RATIO_MIN || ratio > RATIO_MAX {
        return Err(IErr::PseudoRatioDistorted); // 模拟失真→校准（降级矩阵）
    }
    Ok((out, ratio))
}

// ---------------------------------------------------------------------------
// 家族二十七成员复述协议（要点四）
// ---------------------------------------------------------------------------

/// 家族复述协议（单源 + 27 成员各自复述 + 契约哈希 O(1) 对账）。
pub struct FamilyEcho {
    /// 单源文本。
    source: &'static str,
    /// 单源 FNV-1a 摘要。
    digest: u64,
    /// 各成员复述的摘要副本（复述单源——成员无自有内容）。
    restated: [u64; FAMILY_MEMBERS],
}

/// FNV-1a 摘要（判据侧独立重写同源）。
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut it = s.as_bytes();
    while let Some(&b) = it.first() {
        it = &it[1..];
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

impl FamilyEcho {
    /// 由单源构造（成员复述逐份拷贝摘要——复述协议本体）。
    pub fn new(source: &'static str) -> FamilyEcho {
        let d = fnv1a(source);
        let mut restated = [0u64; FAMILY_MEMBERS];
        let mut i = 0usize;
        while i < FAMILY_MEMBERS {
            restated[i] = d; // 复述：成员摘要 = 单源摘要
            i += 1;
        }
        FamilyEcho { source, digest: d, restated }
    }

    /// 单源摘要。
    pub const fn digest(&self) -> u64 {
        self.digest
    }

    /// 成员复述摘要（O(1) 直取；越界 `BadMemberSlot`）。
    pub fn member_digest(&self, member: usize) -> Result<u64, IErr> {
        if member >= FAMILY_MEMBERS {
            return Err(IErr::BadMemberSlot);
        }
        Ok(self.restated[member])
    }

    /// 上游契约接收（哈希对账）：声称的摘要与本源比对。
    /// 不符 = `FamilyHashMismatch`（立案由调用方凭返回执行）。
    pub fn accept_contract(&self, claimed: u64) -> Result<(), IErr> {
        if claimed != self.digest {
            return Err(IErr::FamilyHashMismatch);
        }
        Ok(())
    }

    /// 复述完整性：全部成员与单源一致（判据全扫；协议本身 O(1)）。
    pub fn restatement_intact(&self) -> bool {
        let mut i = 0usize;
        while i < FAMILY_MEMBERS {
            if self.restated[i] != self.digest {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 方向可视化（要点三）
// ---------------------------------------------------------------------------

/// 单个方向段（单源注入形态；F4023 落位后由其喂养）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirSpan {
    /// true = RTL。
    pub rtl: bool,
    /// 段字符数。
    pub chars: u32,
}

/// 方向边界可视化：相邻异向边界必须带隔离标记。
/// `markers_enabled=false` 时构造的视图必然边界模糊——供红线实测。
pub fn direction_view(spans: &[DirSpan], markers_enabled: bool) -> Result<String, IErr> {
    if spans.is_empty() {
        return Err(IErr::DirSourceEmpty);
    }
    let mut out = String::from("«");
    let mut i = 0usize;
    while i < spans.len() {
        if i > 0 && spans[i].rtl != spans[i - 1].rtl {
            if markers_enabled {
                out.push_str("|⇄|"); // 隔离标记（方向边界视觉清晰）
            }
            // 关闭标记 ⇒ 边界模糊（红线实测路径）。
        }
        out.push_str(if spans[i].rtl { "‹ر" } else { "›a" });
        let mut k = 0u32;
        while k < spans[i].chars {
            out.push('·');
            k += 1;
        }
        i += 1;
    }
    out.push('»');
    Ok(out)
}

/// 边界清晰断言：视图内异向相邻段之间必须出现隔离标记。
/// O(段数)：逐对检查标记存在性。
pub fn boundary_clear(spans: &[DirSpan], view: &str) -> Result<(), IErr> {
    let mut i = 1usize;
    while i < spans.len() {
        if spans[i].rtl != spans[i - 1].rtl && !view.contains("|⇄|") {
            return Err(IErr::BoundaryUnclear);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 调试器总控（五列仪表 + 立案簿 + 零常态）
// ---------------------------------------------------------------------------

/// 仪表一行（五列：Locale/方向/格式产物/家族态/伪检态）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InspectorRow {
    pub locale: String,
    /// 方向（"LTR"/"RTL"/"MIX"）。
    pub direction: String,
    /// 格式产物（伪检变换示例产物）。
    pub format_out: String,
    /// 家族态（复述完整性）。
    pub family_ok: bool,
    /// 伪检膨胀比（千分比；未采样 = 0）。
    pub pseudo_ratio: u64,
}

/// 调试器总控。
pub struct I18nDebugger {
    /// 调试开关（零常态：关闭时不采样不扫描）。
    pub enabled: bool,
    /// 伪检规则版本号（规则修正留痕）。
    pub rules_version: u32,
    /// 立案簿。
    cases: Vec<CaseFile>,
    /// 立案拒收计数（簿满）。
    pub cases_rejected: u32,
    /// 已注册伪条目摘要账（硬编码检出参照）。
    pseudo_digests: Vec<u64>,
    /// 采样计数（零常态判据对象）。
    pub sample_ops: u64,
    /// 硬编码检出计数。
    pub hardcode_flags: u32,
    /// 账本。
    pub audits: u32,
}

impl I18nDebugger {
    /// 构造（关闭态起步——零常态）。
    pub fn new() -> I18nDebugger {
        I18nDebugger {
            enabled: false,
            rules_version: 1,
            cases: Vec::new(),
            cases_rejected: 0,
            pseudo_digests: Vec::new(),
            sample_ops: 0,
            hardcode_flags: 0,
            audits: 0,
        }
    }

    /// 注册伪检参照：语料全量伪变换入账（伪检 O(条目) 一次性预付）。
    pub fn load_corpus(&mut self, lib: &CorpusLib) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < lib.entries().len() {
            let e = &lib.entries()[i];
            if let Ok((pseudo, _)) = pseudo_transform(e.text) {
                self.pseudo_digests.push(fnv1a(&pseudo));
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 立案（簿满如实拒案计数——不静默吞案）。
    fn file_case(&mut self, code: u32, ref_id: u32) -> Result<u32, IErr> {
        if self.cases.len() >= CASE_CAP {
            self.cases_rejected += 1;
            return Err(IErr::CaseFull);
        }
        let id = self.cases.len() as u32 + 1;
        self.cases.push(CaseFile { case_id: id, code, ref_id });
        Ok(id)
    }

    /// 已注册伪条目数（判据读）。
    pub fn pseudo_registered(&self) -> u32 {
        self.pseudo_digests.len() as u32
    }

    /// 立案簿读取（判据读）。
    pub fn cases(&self) -> &[CaseFile] {
        &self.cases
    }

    /// 硬编码审计（要点二）：
    /// ① 无伪标记 = 硬编码嫌疑 → 立案（HardcodeFlagged）；
    /// ② 有伪标记但不匹配已注册伪条目 = 规则漏检 → 规则版本 +1（UnknownMark）。
    /// 返回审计裁决（Ok(()) = 已本地化；Err = 检出/漏检事件）。
    pub fn audit_string(&mut self, rendered: &str) -> Result<(), IErr> {
        self.audits += 1;
        let marked = rendered.starts_with(PSEUDO_PREFIX) && rendered.ends_with(PSEUDO_SUFFIX);
        if !marked {
            // 硬编码嫌疑（漏报红线：注入审计实测此处必须触发）。
            self.hardcode_flags += 1;
            return match self.file_case(IErr::HardcodeFlagged.code(), 0) {
                Ok(_) => Err(IErr::HardcodeFlagged),
                Err(e) => Err(e), // 簿满如实透传
            };
        }
        // 有标记：须匹配已注册伪条目（哈希对账 O(1) 每条目扫描一次——
        // 伪检 O(字符串) 口径：对账本身逐条目哈希比较）。
        let h = fnv1a(rendered);
        let mut i = 0usize;
        while i < self.pseudo_digests.len() {
            if self.pseudo_digests[i] == h {
                return Ok(());
            }
            i += 1;
        }
        // 规则漏检 → 规则修正（留痕不静默）。
        self.rules_version += 1;
        let r = self.file_case(IErr::UnknownMark.code(), 0);
        let _ = r;
        Err(IErr::UnknownMark)
    }

    /// 五列仪表渲染（要点五/六）：关闭态零采样（零常态）；开启态
    /// O(列) 计数；同 Locale 方向分歧立案。
    pub fn render_inspector(&mut self, rows: &[InspectorRow]) -> Result<u32, IErr> {
        if !self.enabled {
            return Ok(0); // 零常态：不做任何工作
        }
        self.sample_ops += rows.len() as u64 * INSPECTOR_COLS as u64;
        // 分歧检测：同 Locale 方向不一致 → InspectorDiverged 立案。
        let mut i = 0usize;
        let mut diverged = false;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                if rows[i].locale == rows[j].locale && rows[i].direction != rows[j].direction {
                    diverged = true;
                    let r = self.file_case(IErr::InspectorDiverged.code(), i as u32);
                    let _ = r;
                }
                j += 1;
            }
            i += 1;
        }
        if diverged {
            return Err(IErr::InspectorDiverged);
        }
        Ok(rows.len() as u32)
    }

    /// 方向红线实测入口：关闭标记生成 → 边界断言必失败 → 立案。
    pub fn direction_audit(&mut self, spans: &[DirSpan], markers: bool) -> Result<(), IErr> {
        if !self.enabled {
            return Ok(()); // 零常态
        }
        let view = direction_view(spans, markers)?;
        match boundary_clear(spans, &view) {
            Ok(()) => Ok(()),
            Err(IErr::BoundaryUnclear) => {
                let r = self.file_case(IErr::BoundaryUnclear.code(), 0);
                let _ = r;
                Err(IErr::BoundaryUnclear)
            }
            Err(e) => Err(e),
        }
    }

    /// 读屏摘要（无隐私面——只有计数）。
    pub fn status_summary(&self) -> String {
        let mut s = String::from("i18n 调试：");
        s.push_str(if self.enabled { "开启" } else { "关闭" });
        s.push_str("；立案 ");
        s.push_str(&self.cases.len().to_string());
        s.push_str(" 件；硬编码检出 ");
        s.push_str(&self.hardcode_flags.to_string());
        s.push_str(" 次；规则版本 ");
        s.push_str(&self.rules_version.to_string());
        s.push_str("；采样 ");
        s.push_str(&self.sample_ops.to_string());
        s
    }
}
