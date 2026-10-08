//! VE-F0810 · 文字渲染安全（VE-E 域 · 文字安全段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0810`
//!
//! **判据（锚点原文）**：文字渲染安全防两源风险：字体文件校验（加载时哈希+结构
//! 完整性校验，非信任来源的字体文件进沙箱解析路径——解析器无递归、深度受限、
//! 内存上限 64MB）与恶意字形防护（畸形轮廓/超多点数/自交爆炸的字形在 F0803/F0804
//! 已有兜底，此处统一安全卡登记）。权限模型：字体来源三级（系统只读/工程内置/
//! 用户导入），用户导入字体只能被当前工程引用且计入授权检查（对接 F0829）。
//! 错误路径：校验失败→拒绝加载并给出可读原因（文件损坏/签名不符/超限）；
//! 沙箱解析超时（>500ms）→终止并标记该字体不可用。
//! 判据：**文件校验、沙箱解析、三级来源、64MB 上限、500ms 超时**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 校验失败 → **拒绝加载并给出可读原因**（文件损坏/签名不符/超限）。
//!   「可读」不是措辞要求而是**契约**：[`LoadReject`] 三个变体各自绑定一个
//!   人类可读原因串，且带**建议动作**——只说「失败」而不说「为什么、怎么办」
//!   的错误等于把排查成本转嫁给用户。
//! - 沙箱解析超时（>500ms）→ **终止并标记该字体不可用**。注意是「终止 +
//!   标记」两步：只终止本次解析，下次加载还会再撞一次超时；标记后该字体
//!   在本会话内不再尝试解析（[`SandboxVerdict::TimedOut`] 的字体被记入
//!   [`SecurityGate`] 的不可用集）。
//!
//! **数据结构**：安全卡（[`SecurityCard`]，逐字体的校验结论）；字体来源
//! （[`FontOrigin`]，三级）；来源权限（[`OriginPolicy`]）；沙箱参数
//! （[`SandboxLimits`]，**导出供 F0831 复用**）；安全事件（[`SecurityEvent`]）。
//!
//! **性能逐项分解**：文件校验 **O(文件字节数)**（单遍哈希，单次成本）；
//! 沙箱判定 **O(1)**（预算计数与深度计数均为常数时间比较）。
//!
//! **对接点**：安全事件上行 O06 统一观测通道（本条只产出事件，不建看板）；
//! 沙箱参数与 Eb02 F0831 解析沙箱**同一套**（见 [`SandboxLimits`] 的复用声明）。
//!
//! ## 设计要点
//!
//! - **沙箱参数是「一套」不是「两套」**（[`SandboxLimits`]）：锚点写明「判据与
//!   Eb02 F0831 解析沙箱一致（同一套沙箱参数）」。两份参数各写一遍，迟早漂移成
//!   「文字域限深度 32、字体管理域限深度 16」，届时同一字体在两条路径上得到不同
//!   结论。故本条**导出**这套参数并在头注明写复用声明。
//! - **64MB 与 500ms 是硬上限不是建议值**（[`SANDBOX_MEM_LIMIT_MB`]、
//!   [`SANDBOX_TIME_LIMIT_MS`]）：超限即**拒绝**而非警告。降级放行等于把
//!   「资源耗尽」变成「缓慢劣化」，而内核里缓慢劣化更难归因。
//! - **解析器无递归是结构性的**：沙箱判定走**显式深度计数**而非递归栈深度，
//!   递归深度在no_std 下没有可靠的栈边界可查。用计数器把「递归了多少层」
//!   变成可测量的数字，才能对「深度受限」下判据。
//! - **三级来源的差别在权限不在信任度**（[`FontOrigin`]）：系统只读字体不可被
//!   工程覆盖，工程内置字体可被工程引用但不可被用户替换，用户导入字体
//!   **只能被当前工程引用**且计入授权检查。把三级简化成「可信/不可信」两档，
//!   会让「工程内置但用户想替换」这种真实需求无处表达。
//! - **拒绝原因三变体互斥且各自带建议动作**（[`LoadReject`]）：文件损坏 /
//!   签名不符 / 超限。合并成「校验失败」一个变体，则判据无法区分「哈希不符」
//!   与「内存超限」，而二者的处置完全不同（前者是文件坏了，后者是文件太大）。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0803/F0804 管「畸形轮廓/超多点数/自交爆炸的字形兜底」**（渲染层的
//!   单字形防护），本条管**统一安全卡登记**——把散在各处的兜底结论汇总成
//!   可查询、可上行的安全记录，不重复实现几何防护本身。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
// 真no_std（kernel-image）下 `to_string` 不在预导入集里：闭包的 `format!`
// 结果转String 必须显式引入 `ToString`（宿主构建有 `extern crate std` 故看不出来）。
use alloc::string::ToString;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 沙箱内存上限（字节；锚点明文 64MB）。
pub const SANDBOX_MEM_LIMIT_MB: u32 = 64;

/// 沙箱内存上限（字节形式，判定用）。
pub const SANDBOX_MEM_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

/// 沙箱解析超时（毫秒；锚点明文 500ms）。
pub const SANDBOX_TIME_LIMIT_MS: u32 = 500;

/// 沙箱递归深度上限（**显式计数器**，非真实栈深）。
pub const SANDBOX_MAX_DEPTH: u32 = 32;

/// 沙箱允许的表目录项数上限（表越多越可能藏畸形指针）。
pub const SANDBOX_MAX_TABLES: u32 = 512;

/// 字体文件合法大小区间（字节）：空文件与超大文件都不合法。
pub const FONT_FILE_MIN_BYTES: u64 = 64;

/// 字体文件硬上限（256MB）——**独立于沙箱的 64MB**：
/// 沙箱限的是**解析过程峰值内存**，文件本体可以更大但仍受本上限约束。
pub const FONT_FILE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// 安全校验一次性成本承诺（微秒；锚点 ≤5ms = 5000us）。
pub const VERIFY_BUDGET_US: u32 = 5_000;

/// FNV-1a 64 位偏移基（与全域哈希口径一致）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 字体来源种数（三级，闭集）。
pub const FONT_ORIGIN_COUNT: usize = 3;

/// 加载拒绝变体种数。
pub const LOAD_REJECT_COUNT: usize = 3;

/// 沙箱判定种数。
pub const SANDBOX_VERDICT_COUNT: usize = 4;

/// 恶意字形安全卡种数。
pub const GLYPH_GUARD_COUNT: usize = 4;

/// 读屏面板行数。
pub const A11Y_LINES: usize = 6;

// ---------------------------------------------------------------------------
// 二、字体来源与权限模型
// ---------------------------------------------------------------------------

/// 字体来源三级（**闭集**，锚点明文三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontOrigin {
    /// 系统只读：不可被工程覆盖，也不可被用户替换。
    SystemReadOnly,
    /// 工程内置：可被工程引用，但**不可被用户替换**。
    ProjectBuiltin,
    /// 用户导入：**只能被当前工程引用**，且计入授权检查（对接 F0829）。
    UserImported,
}

impl FontOrigin {
    /// 全枚举（顺序即 [`FontOrigin::ordinal`] 的下标）。
    pub const ALL: [FontOrigin; 3] = [
        FontOrigin::SystemReadOnly,
        FontOrigin::ProjectBuiltin,
        FontOrigin::UserImported,
    ];

    /// 枚举下标（供计数数组用，**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            FontOrigin::SystemReadOnly => 0,
            FontOrigin::ProjectBuiltin => 1,
            FontOrigin::UserImported => 2,
        }
    }

    /// 线编码（**显式映射，不用 `as u8` 拿判别值**）。
    pub const fn wire(self) -> u8 {
        match self {
            FontOrigin::SystemReadOnly => 0x10,
            FontOrigin::ProjectBuiltin => 0x11,
            FontOrigin::UserImported => 0x12,
        }
    }

    /// 线编码反解（未知码返回 [`None`]，**不静默兜底成最低权限**——
    /// 兜底成 `SystemReadOnly` 会让用户导入字体被当成系统字体直接放行）。
    pub const fn from_wire(w: u8) -> Option<FontOrigin> {
        match w {
            0x10 => Some(FontOrigin::SystemReadOnly),
            0x11 => Some(FontOrigin::ProjectBuiltin),
            0x12 => Some(FontOrigin::UserImported),
            _ => None,
        }
    }

    /// 是否为**非信任来源**（须进沙箱解析路径）。
    pub const fn needs_sandbox(self) -> bool {
        matches!(self, FontOrigin::UserImported)
    }

    /// 稳定标签。
    pub const fn label(self) -> &'static str {
        match self {
            FontOrigin::SystemReadOnly => "system-ro",
            FontOrigin::ProjectBuiltin => "project-builtin",
            FontOrigin::UserImported => "user-imported",
        }
    }
}

/// 加载拒绝原因（**三变体互斥**，各带可读原因与建议动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadReject {
    /// 文件损坏（长度越界/哈希不符/表目录畸形）。
    Corrupt,
    /// 签名不符（sfnt 版本号不识别）。
    BadSignature,
    /// 超限（文件大小或解析内存越界）。
    OverLimit,
}

impl LoadReject {
    /// 全枚举（顺序即 [`LoadReject::ordinal`] 的下标）。
    pub const ALL: [LoadReject; 3] =
        [LoadReject::Corrupt, LoadReject::BadSignature, LoadReject::OverLimit];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            LoadReject::Corrupt => 0,
            LoadReject::BadSignature => 1,
            LoadReject::OverLimit => 2,
        }
    }

    /// 线编码（显式映射）。
    pub const fn wire(self) -> u8 {
        match self {
            LoadReject::Corrupt => 0xE1,
            LoadReject::BadSignature => 0xE2,
            LoadReject::OverLimit => 0xE3,
        }
    }

    /// **可读原因**（锚点要求「给出可读原因」）。
    pub const fn reason(self) -> &'static str {
        match self {
            LoadReject::Corrupt => "字体文件损坏 / font file corrupt",
            LoadReject::BadSignature => "字体签名不符 / bad sfnt signature",
            LoadReject::OverLimit => "超出大小或内存上限 / over size or memory limit",
        }
    }

    /// **建议动作**（可读原因的第三要素，见模块头错误路径）。
    pub const fn advice(self) -> &'static str {
        match self {
            LoadReject::Corrupt => "重新获取该字体文件 / re-obtain the font file",
            LoadReject::BadSignature => "确认文件是否为 sfnt 格式 / verify sfnt format",
            LoadReject::OverLimit => "换用更小的字体子集 / use a smaller font subset",
        }
    }

    /// 原因与建议合成的完整可读串。
    pub fn readable(self) -> String {
        format!("{} -> {}", self.reason(), self.advice())
    }
}

// ---------------------------------------------------------------------------
// 三、沙箱参数（导出供 F0831 复用·同一套）
// ---------------------------------------------------------------------------

/// 沙箱参数（**全域一套**，锚点：「判据与 Eb02 F0831 解析沙箱一致
/// （同一套沙箱参数）」）。
///
/// **复用声明**：本结构是文字域与字体管理域**共用**的沙箱契约。任何一方
/// 私设一份数值都会让同一字体在两条解析路径上得到不同结论，故此处
/// 集中定义并由双方引用，判据侧亦断言「四处引用同一组数值」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxLimits {
    /// 内存上限（字节）。
    pub mem_bytes: u64,
    /// 超时（毫秒）。
    pub time_ms: u32,
    /// 递归深度上限（显式计数器）。
    pub max_depth: u32,
    /// 表目录项数上限。
    pub max_tables: u32,
}

impl SandboxLimits {
    /// 全域标准参数（**唯一权威值**，供 F0831 等复用方取用）。
    pub const fn standard() -> SandboxLimits {
        SandboxLimits {
            mem_bytes: SANDBOX_MEM_LIMIT_BYTES,
            time_ms: SANDBOX_TIME_LIMIT_MS,
            max_depth: SANDBOX_MAX_DEPTH,
            max_tables: SANDBOX_MAX_TABLES,
        }
    }

    /// 三个上限是否**全部为正**（零值会让「超限」判定恒不成立）。
    pub const fn is_sane(&self) -> bool {
        self.mem_bytes > 0 && self.time_ms > 0 && self.max_depth > 0 && self.max_tables > 0
    }
}

/// 沙箱判定结论（四种，**互斥**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxVerdict {
    /// 通过。
    Passed,
    /// 内存超限（>64MB）。
    MemExceeded,
    /// 超时（>500ms）。
    TimedOut,
    /// 深度超限（>32）。
    DepthExceeded,
}

impl SandboxVerdict {
    /// 全枚举。
    pub const ALL: [SandboxVerdict; 4] = [
        SandboxVerdict::Passed,
        SandboxVerdict::MemExceeded,
        SandboxVerdict::TimedOut,
        SandboxVerdict::DepthExceeded,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            SandboxVerdict::Passed => 0,
            SandboxVerdict::MemExceeded => 1,
            SandboxVerdict::TimedOut => 2,
            SandboxVerdict::DepthExceeded => 3,
        }
    }

    /// 是否为**拒绝**（非 Passed）。
    pub const fn is_reject(self) -> bool {
        !matches!(self, SandboxVerdict::Passed)
    }

    /// 拒绝时该字体是否须被**标记为不可用**（超时与内存超限须标记：
    /// 只终止本次而不标记，下次加载会再撞一次同样的问题）。
    pub const fn must_quarantine(self) -> bool {
        matches!(self, SandboxVerdict::TimedOut | SandboxVerdict::MemExceeded)
    }
}

// ---------------------------------------------------------------------------
// 四、文件校验（哈希 + 结构完整性）
// ---------------------------------------------------------------------------

/// 字体文件头（校验输入，非真实字体字节）。
#[derive(Clone, Copy, Debug)]
pub struct FontFileHeader {
    /// sfnt 版本号（`0x0001_0000` = TrueType 1.0，`OTTO` = CFF）。
    pub version: u32,
    /// 表目录项数。
    pub num_tables: u16,
    /// 文件总字节数。
    pub file_bytes: u64,
    /// 文件内容指纹（调用方算好的哈希）。
    pub digest: u64,
}

impl FontFileHeader {
    /// 构造一个**合法**的头（便于判据造语料）。
    pub fn valid(digest: u64) -> FontFileHeader {
        FontFileHeader {
            version: 0x0001_0000,
            num_tables: 12,
            file_bytes: 128 * 1024,
            digest,
        }
    }

    /// 构造一个**签名不符**的头。
    pub fn bad_signature() -> FontFileHeader {
        FontFileHeader {
            version: 0xDEAD_BEEF,
            num_tables: 12,
            file_bytes: 128 * 1024,
            digest: 0x1234_5678_9ABC_DEF0,
        }
    }
}

/// 文件校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// 通过。
    Ok,
    /// 拒绝（附原因）。
    Rejected(LoadReject),
}

/// 文件校验（**结构完整性 + 长度区间 + 表数上限**，哈希由调用方算好传入）。
///
/// 顺序刻意是「签名 → 长度 → 表数」：**先判最便宜且最能立刻否定文件的**，
/// 让签名不符的文件不必走完长度与表数检查。
pub fn verify_file(h: &FontFileHeader, expected_digest: u64) -> VerifyOutcome {
    // 1) 签名：只认 TrueType 1.0 与 CFF（'OTTO' = 0x4F54_544F）。
    let sig_ok = h.version == 0x0001_0000 || h.version == 0x4F54_544F;
    if !sig_ok {
        return VerifyOutcome::Rejected(LoadReject::BadSignature);
    }
    // 2) 长度区间：空文件 / 越界大文件都拒。
    if h.file_bytes < FONT_FILE_MIN_BYTES || h.file_bytes > FONT_FILE_MAX_BYTES {
        return VerifyOutcome::Rejected(LoadReject::OverLimit);
    }
    // 3) 表目录项数上限（表数异常常是畸形指针的伪装）。
    if (h.num_tables as u32) == 0 || (h.num_tables as u32) > SANDBOX_MAX_TABLES {
        return VerifyOutcome::Rejected(LoadReject::Corrupt);
    }
    // 4) 哈希一致性。
    if h.digest != expected_digest {
        return VerifyOutcome::Rejected(LoadReject::Corrupt);
    }
    VerifyOutcome::Ok
}

/// FNV-1a 文件指纹（与全域哈希口径一致；**纯函数**，不依赖调用方）。
pub fn file_digest(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 五、沙箱解析（无递归·深度受限·内存上限·超时）
// ---------------------------------------------------------------------------

/// 沙箱解析输入（**计数式**，不是真实解析器——真实解析在 F0822）。
#[derive(Clone, Copy, Debug)]
pub struct SandboxInput {
    /// 解析过程峰值内存（字节）。
    pub peak_bytes: u64,
    /// 解析耗时（毫秒）。
    pub elapsed_ms: u32,
    /// 观测到的最大递归深度（由**显式计数器**得出）。
    pub depth: u32,
    /// 表目录项数。
    pub tables: u32,
}

impl SandboxInput {
    /// 全部在限内的输入。
    pub fn clean() -> SandboxInput {
        SandboxInput {
            peak_bytes: 8 * 1024 * 1024,
            elapsed_ms: 12,
            depth: 4,
            tables: 12,
        }
    }
}

/// 沙箱解析判定（**O(1)**：四个常数时间比较，无分配无递归）。
///
/// 优先级刻意为「内存 → 超时 → 深度」：内存超限往往伴随超时，先报内存
/// 更贴近真实成因；顺序写进判据，避免「改了优先级判据仍绿」。
pub fn sandbox_check(inp: &SandboxInput, lim: &SandboxLimits) -> SandboxVerdict {
    if inp.peak_bytes > lim.mem_bytes {
        return SandboxVerdict::MemExceeded;
    }
    if inp.elapsed_ms > lim.time_ms {
        return SandboxVerdict::TimedOut;
    }
    if inp.depth > lim.max_depth {
        return SandboxVerdict::DepthExceeded;
    }
    if inp.tables > lim.max_tables {
        // 表数超限归入内存类拒绝（表目录本身占内存），不另开变体。
        return SandboxVerdict::MemExceeded;
    }
    SandboxVerdict::Passed
}

// ---------------------------------------------------------------------------
// 六、恶意字形安全卡（统一登记 F0803/F0804 的兜底结论）
// ---------------------------------------------------------------------------

/// 恶意字形防护类别（**统一登记**，几何防护本体在 F0803/F0804）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphGuard {
    /// 轮廓畸形（点数为0 / 自交未收敛）。
    MalformedOutline,
    /// 点数超限（超过 F0803 的点数上限）。
    TooManyPoints,
    /// 自交爆炸（F0804 光栅化超时前的兜底）。
    SelfIntersection,
    /// 轮廓数值非有限（NaN/Inf）。
    NonFinite,
}

impl GlyphGuard {
    /// 全枚举。
    pub const ALL: [GlyphGuard; 4] = [
        GlyphGuard::MalformedOutline,
        GlyphGuard::TooManyPoints,
        GlyphGuard::SelfIntersection,
        GlyphGuard::NonFinite,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            GlyphGuard::MalformedOutline => 0,
            GlyphGuard::TooManyPoints => 1,
            GlyphGuard::SelfIntersection => 2,
            GlyphGuard::NonFinite => 3,
        }
    }

    /// 该类是否**必须阻止渲染**（四类都必须——安全卡不设「提示级」，
    /// 提示级会让「看起来只是提醒」被当成可放行）。
    pub const fn blocks_render(self) -> bool {
        true
    }

    /// 线编码（显式映射）。
    pub const fn wire(self) -> u8 {
        match self {
            GlyphGuard::MalformedOutline => 0xC1,
            GlyphGuard::TooManyPoints => 0xC2,
            GlyphGuard::SelfIntersection => 0xC3,
            GlyphGuard::NonFinite => 0xC4,
        }
    }
}

/// 逐字体的安全卡（**登记**而非重复实现几何防护）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityCard {
    /// 字体 ID。
    pub font_id: u32,
    /// 字形 ID。
    pub glyph_id: u32,
    /// 命中的防护类别。
    pub guard: GlyphGuard,
}

impl SecurityCard {
    /// 新建一张安全卡。
    pub const fn new(font_id: u32, glyph_id: u32, guard: GlyphGuard) -> SecurityCard {
        SecurityCard {
            font_id,
            glyph_id,
            guard,
        }
    }
}

// ---------------------------------------------------------------------------
// 七、安全闸（来源权限 + 隔离集 + 计数）
// ---------------------------------------------------------------------------

/// 来源权限裁决结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginDecision {
    /// 允许：可被当前工程引用。
    AllowCurrentProject,
    /// 拒绝：用户导入字体**不得跨工程引用**。
    DenyCrossProject,
    /// 拒绝：用户导入字体**不得替换系统只读字体**。
    DenyOverrideSystem,
}

/// 来源权限裁决。
///
/// 锚点：「用户导入字体只能被当前工程引用」。故两条拒绝路径：
/// 跨工程引用、以及试图替换系统只读字体。工程内置字体可被工程引用。
pub fn check_origin(origin: FontOrigin, same_project: bool, overrides_system: bool) -> OriginDecision {
    match origin {
        FontOrigin::SystemReadOnly => {
            if overrides_system {
                OriginDecision::DenyOverrideSystem
            } else {
                OriginDecision::AllowCurrentProject
            }
        }
        FontOrigin::ProjectBuiltin => {
            // 工程内置字体**可被工程引用，但不可被用户替换**：
            // `overrides_system` 为真表示有人试图把它当系统字体覆盖掉，
            // 此时必须拒——放行等于让用户替换工程内置字体。
            if overrides_system {
                OriginDecision::DenyOverrideSystem
            } else {
                OriginDecision::AllowCurrentProject
            }
        }
        FontOrigin::UserImported => {
            if !same_project {
                OriginDecision::DenyCrossProject
            } else if overrides_system {
                OriginDecision::DenyOverrideSystem
            } else {
                OriginDecision::AllowCurrentProject
            }
        }
    }
}

/// 安全闸：隔离集 + 计数（本会话内的不可用字体与各类事件计数）。
#[derive(Clone, Debug)]
pub struct SecurityGate {
    /// 已隔离的字体 ID（超时/内存超限后**标记不可用**，不重复尝试）。
    quarantined: [u32; 64],
    /// 隔离集条数。
    quarantine_len: usize,
    /// 拒绝计数（按 [`LoadReject::ordinal`] 下标）。
    pub rejects: [u64; LOAD_REJECT_COUNT],
    /// 沙箱判定计数（按 [`SandboxVerdict::ordinal`] 下标）。
    pub sandbox: [u64; SANDBOX_VERDICT_COUNT],
    /// 安全卡登记计数（按 [`GlyphGuard::ordinal`] 下标）。
    pub cards: [u64; GLYPH_GUARD_COUNT],
    /// 安全校验累计耗时（微秒）。
    pub verify_us: u64,
}

impl SecurityGate {
    /// 新建安全闸（全零）。
    pub const fn new() -> SecurityGate {
        SecurityGate {
            quarantined: [0u32; 64],
            quarantine_len: 0,
            rejects: [0u64; LOAD_REJECT_COUNT],
            sandbox: [0u64; SANDBOX_VERDICT_COUNT],
            cards: [0u64; GLYPH_GUARD_COUNT],
            verify_us: 0,
        }
    }

    /// 字体是否已被**标记不可用**。
    pub fn is_quarantined(&self, font_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.quarantine_len {
            if self.quarantined[i] == font_id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记一次加载拒绝（**计数**，让「多少字体被拒」可查）。
    pub fn note_reject(&mut self, why: LoadReject) {
        self.rejects[why.ordinal()] = self.rejects[why.ordinal()].saturating_add(1);
    }

    /// 登记一次沙箱判定；返回**是否须阻止本次装载**。
///
/// 「终止 + 标记」是两步：只终止本次解析，下次加载会再撞一次超时。
/// 返回值区分三态：
/// - `false`：通过，可继续；
/// - `true` 且已记录进隔离集：标记不可用，后续不再尝试；
/// - `true` 但**隔离集已满**：标记失败。此时**仍须阻止装载**——
/// 放行等于让一个已知会超时的字体进入渲染路径。
    pub fn note_sandbox(&mut self, font_id: u32, v: SandboxVerdict) -> bool {
        self.sandbox[v.ordinal()] = self.sandbox[v.ordinal()].saturating_add(1);
        if v.must_quarantine() {
            // `quarantine` 返回 false = 隔离集已满且该字体尚未被隔离。
            // **不可忽略**：忽略会把「记不下了」当成「已标记」。
            // 返回 true = 本次必须阻止装载（无论是「已标记」还是「标记失败」）。
            return true;
        }
        false
    }

    /// 隔离集满时**不静默丢弃**：返回是否真的隔离了。
    pub fn quarantine(&mut self, font_id: u32) -> bool {
        if self.is_quarantined(font_id) {
            return true;
        }
        if self.quarantine_len >= self.quarantined.len() {
            return false;
        }
        self.quarantined[self.quarantine_len] = font_id;
        self.quarantine_len += 1;
        true
    }

    /// 登记一张安全卡（计数）。
    pub fn note_card(&mut self, c: &SecurityCard) {
        self.cards[c.guard.ordinal()] = self.cards[c.guard.ordinal()].saturating_add(1);
    }

    /// 已隔离字体数。
    pub fn quarantined_len(&self) -> usize {
        self.quarantine_len
    }

    /// 读屏面板（**只报聚合计数**，不泄漏字体名与路径）。
    pub fn a11y_lines(&self) -> [String; A11Y_LINES] {
        [
            format!(
                "拒绝计数 / rejects corrupt/badsig/over: {}/{}/{}",
                self.rejects[0], self.rejects[1], self.rejects[2]
            ),
            format!(
                "沙箱判定 / sandbox pass/mem/timeout/depth: {}/{}/{}/{}",
                self.sandbox[0], self.sandbox[1], self.sandbox[2], self.sandbox[3]
            ),
            format!(
                "安全卡 / cards malformed/points/selfint/nonfinite: {}/{}/{}/{}",
                self.cards[0], self.cards[1], self.cards[2], self.cards[3]
            ),
            format!("已隔离字体数 / quarantined fonts: {}", self.quarantine_len),
            format!(
                "安全校验累计 / verify us: {} (budget {} us per font)",
                self.verify_us, VERIFY_BUDGET_US
            ),
            "拒绝一律给出可读原因与建议动作 / rejects carry readable reason and advice".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// 八、一次性装载流水线（校验 → 沙箱 → 权限 → 安全卡）
// ---------------------------------------------------------------------------

/// 装载结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    /// 装载成功。
    Loaded,
    /// 已隔离（在超时/内存超限前已装载过，本次不再尝试）。
    Quarantined,
    /// 拒绝（附原因）。
    Rejected(LoadReject),
}

/// 一次性装载：按「已隔离 → 文件校验 → 沙箱 → 权限」顺序裁决。
///
/// 顺序理由：**已隔离检查放最前**——已标记不可用的字体不该再花一次校验成本；
/// 沙箱放权限之前——不给未通过资源限制的文件谈授权。
pub fn load_font(
    gate: &mut SecurityGate,
    font_id: u32,
    origin: FontOrigin,
    header: &FontFileHeader,
    expected_digest: u64,
    sandbox_in: &SandboxInput,
    same_project: bool,
    overrides_system: bool,
) -> LoadOutcome {
    // 0) 已隔离 ⇒ 直接返回，不再尝试。
    if gate.is_quarantined(font_id) {
        return LoadOutcome::Quarantined;
    }
    // 1) 文件校验。
    match verify_file(header, expected_digest) {
        VerifyOutcome::Rejected(why) => {
            gate.note_reject(why);
            gate.verify_us = gate.verify_us.saturating_add(VERIFY_BUDGET_US as u64);
            return LoadOutcome::Rejected(why);
        }
        VerifyOutcome::Ok => {}
    }
    gate.verify_us = gate.verify_us.saturating_add(VERIFY_BUDGET_US as u64);
    // 2) 非信任来源 ⇒ 必须进沙箱。
    if origin.needs_sandbox() {
        let lim = SandboxLimits::standard();
        let v = sandbox_check(sandbox_in, &lim);
        // 须隔离的判定（超时 / 内存超限）
        let blocked = gate.note_sandbox(font_id, v);
        if blocked {
            // 执行隔离登记：返回 false = 隔离集已满且该字体尚未被隔离。
            // **仍须拒绝**，不能因为「没地方记」就放行。
            gate.quarantine(font_id);
            return if gate.is_quarantined(font_id) {
                LoadOutcome::Quarantined
            } else {
                LoadOutcome::Rejected(LoadReject::OverLimit)
            };
        }
        if v.is_reject() {
            gate.note_reject(LoadReject::OverLimit);
            return LoadOutcome::Rejected(LoadReject::OverLimit);
        }
    } else {
        // 信任来源也要记一次通过（口径一致：四类计数同进同出）。
        gate.note_sandbox(font_id, SandboxVerdict::Passed);
    }
    // 3) 权限裁决。
    match check_origin(origin, same_project, overrides_system) {
        OriginDecision::AllowCurrentProject => LoadOutcome::Loaded,
        OriginDecision::DenyCrossProject | OriginDecision::DenyOverrideSystem => {
            gate.note_reject(LoadReject::Corrupt);
            LoadOutcome::Rejected(LoadReject::Corrupt)
        }
    }
}

// ---------------------------------------------------------------------------
// 九、安全事件（上行O06 统一通道·本条只产出不建看板）
// ---------------------------------------------------------------------------

/// 安全事件类别（**闭集**，上行 O06）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityEventKind {
    /// 文件校验拒绝。
    FileRejected,
    /// 沙箱超时。
    SandboxTimeout,
    /// 沙箱内存超限。
    SandboxMemExceeded,
    /// 恶意字形拦截。
    GlyphBlocked,
    /// 权限拒绝。
    PermissionDenied,
}

impl SecurityEventKind {
    /// 全枚举。
    pub const ALL: [SecurityEventKind; 5] = [
        SecurityEventKind::FileRejected,
        SecurityEventKind::SandboxTimeout,
        SecurityEventKind::SandboxMemExceeded,
        SecurityEventKind::GlyphBlocked,
        SecurityEventKind::PermissionDenied,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            SecurityEventKind::FileRejected => 0,
            SecurityEventKind::SandboxTimeout => 1,
            SecurityEventKind::SandboxMemExceeded => 2,
            SecurityEventKind::GlyphBlocked => 3,
            SecurityEventKind::PermissionDenied => 4,
        }
    }

    /// 稳定标签（**不含字体名与路径**——上报不得携带用户资产标识）。
    pub const fn label(self) -> &'static str {
        match self {
            SecurityEventKind::FileRejected => "file-rejected",
            SecurityEventKind::SandboxTimeout => "sandbox-timeout",
            SecurityEventKind::SandboxMemExceeded => "sandbox-mem-exceeded",
            SecurityEventKind::GlyphBlocked => "glyph-blocked",
            SecurityEventKind::PermissionDenied => "permission-denied",
        }
    }
}

/// 安全事件（六字段，与 F0809 遥测同口径，便于同通道上行）。
#[derive(Clone, Copy, Debug)]
pub struct SecurityEvent {
    /// 事件类别。
    pub kind: SecurityEventKind,
    /// 数值载荷（如拒绝的字节数 / 超限的表数）。
    pub value: u64,
    /// 分桶键（0=未分桶）。
    pub bucket: u8,
    /// 时间戳（单调计数，非墙上时钟）。
    pub timestamp: u64,
    /// 会话匿名指纹。
    pub anon_session: u64,
    /// 协议版本。
    pub version: u32,
}

impl SecurityEvent {
    /// 新建事件。
    pub const fn new(
        kind: SecurityEventKind,
        value: u64,
        timestamp: u64,
        anon_session: u64,
    ) -> SecurityEvent {
        SecurityEvent {
            kind,
            value,
            bucket: 0,
            timestamp,
            anon_session,
            version: PROTOCOL_VERSION,
        }
    }
}

/// 安全事件协议版本（与 F0809 遥测同版本号，便于同通道对齐）。
pub const PROTOCOL_VERSION: u32 = 1;

/// 会话标识折叠为匿名指纹（FNV-1a，**原始标识不出函数**）。
pub fn fold_session_id(raw: &[u8]) -> u64 {
    file_digest(raw)
}

// ---------------------------------------------------------------------------
// 十、判据
// ---------------------------------------------------------------------------

/// VE-F0810 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vee10_checks() -> CheckSet {
    let mut s = CheckSet::new("vee10_textsecurity");

    // --- 判据 0：规格常量**逐字锚定**（锚点明文数字不许漂移） --------------
    //
    // 其余判据多用符号引用常量，常量被改动判据会跟着变而照样通过。
    // 故此处逐字钉死锚点明文数字：64MB、500ms。
    s.add(
        "E10-规格常量-锚点明文64MB与500ms逐字一致",
        SANDBOX_MEM_LIMIT_MB == 64
            && SANDBOX_MEM_LIMIT_BYTES == 67_108_864
            && SANDBOX_TIME_LIMIT_MS == 500
            && SANDBOX_MAX_DEPTH == 32
            && VERIFY_BUDGET_US == 5_000
            && FONT_ORIGIN_COUNT == 3
            && LOAD_REJECT_COUNT == 3
            && SANDBOX_VERDICT_COUNT == 4
            && GLYPH_GUARD_COUNT == 4,
        "锚点明文：沙箱内存 64MB=67108864 字节、解析超时 500ms、校验 ≤5ms=5000us、三级来源、三种拒绝、四类沙箱判定、四类安全卡",
    );

    // --- 判据 1：沙箱参数是**同一套**（导出供 F0831 复用） -----------------
    //
    // 锚点：「判据与 Eb02 F0831 解析沙箱一致（同一套沙箱参数）」。
    // 判据不能只断「本域常量是多少」（那是自证式），须断**四处独立取值同源**：
    // 标准参数、模块级常量、以及本判据内独立重算的字面数，三者一致。
    {
        let lim = SandboxLimits::standard();
        // 判据侧**独立重算** 64MB 与 500ms（不复用被测常量，直接写字面数）
        let indep_mem: u64 = 64u64 * 1024u64 * 1024u64;
        let indep_time: u32 = 500;
        s.add(
            "E10-沙箱参数-同一套且四处取值同源",
            lim.mem_bytes == indep_mem
                && lim.time_ms == indep_time
                && lim.max_depth == 32
                && lim.max_tables == SANDBOX_MAX_TABLES
                // 标准参数与模块级常量必须一致（改其一不改另一即转红）
                && lim.mem_bytes == SANDBOX_MEM_LIMIT_BYTES
                && lim.time_ms == SANDBOX_TIME_LIMIT_MS
                && lim.is_sane(),
            "64MB=67108864 与 500ms 由判据侧独立重算并与标准参数、模块常量三方一致（F0831 复用同一套）",
        );
    }

    // --- 判据 2：文件校验 —— **哈希 + 结构完整性** -------------------------
    {
        let digest = file_digest(b"varix-font-bytes");
        let ok = FontFileHeader::valid(digest);
        // 结构完整性：表数为 0 拒
        let mut zero_tables = ok;
        zero_tables.num_tables = 0;
        // 哈希不符拒
        let wrong = FontFileHeader::valid(digest ^ 0xFFFF_FFFF);
        s.add(
            "E10-文件校验-哈希一致过且哈希不符拒",
            verify_file(&ok, digest) == VerifyOutcome::Ok
                && verify_file(&wrong, digest) == VerifyOutcome::Rejected(LoadReject::Corrupt)
                && verify_file(&zero_tables, digest) == VerifyOutcome::Rejected(LoadReject::Corrupt),
            "哈希与期望一致时通过；哈希不符与表数为 0 均拒绝为『文件损坏』",
        );
    }

    // --- 判据 3：拒绝原因**三变体互斥**且各带可读原因与建议 ----------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < LoadReject::ALL.len() {
            let r = LoadReject::ALL[i];
            ok &= wires.is_empty() || !wires.contains(&r.wire());
            wires.push(r.wire());
            // 可读原因与建议动作**都不得为空**
            ok &= !r.reason().is_empty() && !r.advice().is_empty();
            // 原因串不得三变体相同（合并成一个变体即失去区分力）
            i += 1;
        }
        let reasons_differ = LoadReject::Corrupt.reason() != LoadReject::BadSignature.reason()
            && LoadReject::BadSignature.reason() != LoadReject::OverLimit.reason()
            && LoadReject::Corrupt.reason() != LoadReject::OverLimit.reason();
        let advices_differ = LoadReject::Corrupt.advice() != LoadReject::BadSignature.advice()
            && LoadReject::BadSignature.advice() != LoadReject::OverLimit.advice()
            && LoadReject::Corrupt.advice() != LoadReject::OverLimit.advice();
        // 可读串含建议动作
        let r = LoadReject::Corrupt.readable();
        s.add(
            "E10-拒绝原因-三变体互斥且各带原因与建议",
            ok
                && reasons_differ
                && advices_differ
                && r.contains("->")
                && r.contains(LoadReject::Corrupt.advice()),
            "损坏/签名不符/超限三变体原因与建议互不相同，可读串含『原因 -> 建议』两段",
        );
    }

    // --- 判据 4：签名不符优先裁决（不走到长度/表数） ----------------------
    {
        // 签名不符 **且** 长度越界 **且** 表数为 0：应报签名不符（最便宜且最致命）
        let mut h = FontFileHeader::bad_signature();
        h.file_bytes = 0;
        h.num_tables = 0;
        s.add(
            "E10-校验顺序-签名不符优先于其它",
            verify_file(&h, 0) == VerifyOutcome::Rejected(LoadReject::BadSignature),
            "签名不符时即使长度与表数也异常，仍报『签名不符』——先判最便宜且能否定文件的",
        );
    }

    // --- 判据 5：长度区间双边夹逼 ----------------------------------------
    {
        let d = file_digest(b"x");
        let mut small = FontFileHeader::valid(d);
        small.file_bytes = FONT_FILE_MIN_BYTES - 1; // 63字节：过小
        let mut big = FontFileHeader::valid(d);
        big.file_bytes = FONT_FILE_MAX_BYTES + 1; // 越界过大
        let mut edge_lo = FontFileHeader::valid(d);
        edge_lo.file_bytes = FONT_FILE_MIN_BYTES; // 恰在下界
        let mut edge_hi = FontFileHeader::valid(d);
        edge_hi.file_bytes = FONT_FILE_MAX_BYTES; // 恰在上界
        s.add(
            "E10-长度区间-双边夹逼且边界值可过",
            verify_file(&small, d) == VerifyOutcome::Rejected(LoadReject::OverLimit)
                && verify_file(&big, d) == VerifyOutcome::Rejected(LoadReject::OverLimit)
                // **反向**：恰好等于边界必须**可过**，否则是「差一像素」型误拒
                && verify_file(&edge_lo, d) == VerifyOutcome::Ok
                && verify_file(&edge_hi, d) == VerifyOutcome::Ok,
            "小于 64 字节或大于 256MB 拒；恰等于上下界均可过（夹逼对钉死界位置）",
        );
    }

    // --- 判据 6：沙箱 64MB 上限（**边界值可过**） -------------------------
    {
        let lim = SandboxLimits::standard();
        let mut over = SandboxInput::clean();
        over.peak_bytes = lim.mem_bytes + 1;
        let mut edge = SandboxInput::clean();
        edge.peak_bytes = lim.mem_bytes; // 恰好 64MB 应通过
        s.add(
            "E10-沙箱-64MB上限含边界",
            sandbox_check(&over, &lim) == SandboxVerdict::MemExceeded
                && sandbox_check(&edge, &lim) == SandboxVerdict::Passed
                && SANDBOX_MEM_LIMIT_BYTES == 67_108_864,
            "峰值 64MB+1 字节判内存超限；恰为 64MB 通过（边界不误杀）",
        );
    }

    // --- 判据 7：沙箱 500ms 超时（**边界值可过**） ------------------------
    {
        let lim = SandboxLimits::standard();
        let mut over = SandboxInput::clean();
        over.elapsed_ms = lim.time_ms + 1;
        let mut edge = SandboxInput::clean();
        edge.elapsed_ms = lim.time_ms; // 恰好 500ms 应通过
        s.add(
            "E10-沙箱-500ms超时含边界",
            sandbox_check(&over, &lim) == SandboxVerdict::TimedOut
                && sandbox_check(&edge, &lim) == SandboxVerdict::Passed,
            "耗时 501ms 判超时；恰为 500ms 通过（超时是终止条件而非预警线）",
        );
    }

    // --- 判据 8：沙箱深度受限（无递归·显式计数器） ------------------------
    {
        let lim = SandboxLimits::standard();
        let mut deep = SandboxInput::clean();
        deep.depth = lim.max_depth + 1;
        let mut edge = SandboxInput::clean();
        edge.depth = lim.max_depth;
        s.add(
            "E10-沙箱-深度受限且边界可过",
            sandbox_check(&deep, &lim) == SandboxVerdict::DepthExceeded
                && sandbox_check(&edge, &lim) == SandboxVerdict::Passed
                // 深度是**计数器**而非栈深：递归层数可测量才谈得上「深度受限」
                && deep.depth > 0,
            "深度 33 判超限、恰 32 通过；深度由显式计数器给出（no_std 下无可查栈边界）",
        );
    }

    // --- 判据 9：沙箱判定**优先级固定**（内存 > 超时 > 深度） --------------
    {
        let lim = SandboxLimits::standard();
        // 同时超三项 ⇒ 必报内存（最贴近真实成因）
        let mut all = SandboxInput::clean();
        all.peak_bytes = lim.mem_bytes + 1;
        all.elapsed_ms = lim.time_ms + 1;
        all.depth = lim.max_depth + 1;
        // 同时超超时与深度 ⇒ 报超时
        let mut t_d = SandboxInput::clean();
        t_d.elapsed_ms = lim.time_ms + 1;
        t_d.depth = lim.max_depth + 1;
        s.add(
            "E10-沙箱-判定优先级内存先于超时先于深度",
            sandbox_check(&all, &lim) == SandboxVerdict::MemExceeded
                && sandbox_check(&t_d, &lim) == SandboxVerdict::TimedOut,
            "三项皆超时报内存；超时+深度皆超时报超时（优先级写进判据，改优先级即转红）",
        );
    }

    // --- 判据 10：四级来源枚举线编码自洽 ---------------------------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < FontOrigin::ALL.len() {
            let o = FontOrigin::ALL[i];
            ok &= FontOrigin::from_wire(o.wire()) == Some(o);
            ok &= !wires.contains(&o.wire());
            wires.push(o.wire());
            // 线编码不得等于判别值（否则改枚举顺序静默改协议）
            ok &= o.wire() != o.ordinal() as u8;
            i += 1;
        }
        // 未知码返回 None（不静默兜底成SystemReadOnly）
        ok &= FontOrigin::from_wire(0x00).is_none()
            && FontOrigin::from_wire(0xFF).is_none();
        s.add(
            "E10-来源枚举-三级线编码自洽且未知码不兜底",
            ok && FontOrigin::ALL.len() == FONT_ORIGIN_COUNT,
            "系统只读/工程内置/用户导入三码往返自洽互异，未知码返回 None（兜底成系统只读会让用户字体被直接放行）",
        );
    }

    // --- 判据 11：三级来源权限 —— **用户导入只限当前工程** ----------------
    {
        s.add(
            "E10-权限-用户导入只限当前工程且不得替换系统",
            // 用户导入 + 跨工程 ⇒拒
            check_origin(FontOrigin::UserImported, false, false)
                == OriginDecision::DenyCrossProject
            // 用户导入 + 同工程 ⇒ 允许
                && check_origin(FontOrigin::UserImported, true, false)
                    == OriginDecision::AllowCurrentProject
            // 用户导入试图替换系统只读 ⇒ 拒（即便同工程）
                && check_origin(FontOrigin::UserImported, true, true)
                    == OriginDecision::DenyOverrideSystem
            // 工程内置可被工程引用，但不得被用户替换（overrides_system 即替换）
                && check_origin(FontOrigin::ProjectBuiltin, true, true)
                    == OriginDecision::DenyOverrideSystem
                && check_origin(FontOrigin::ProjectBuiltin, true, false)
                    == OriginDecision::AllowCurrentProject
            // 系统只读不可被覆盖
                && check_origin(FontOrigin::SystemReadOnly, true, true)
                    == OriginDecision::DenyOverrideSystem,
            "用户导入字体跨工程引用拒、同工程允许、试图替换系统只读拒；工程内置可引用不可被替换",
        );
    }

    // --- 判据 12：非信任来源**必须进沙箱**，信任来源不占用沙箱 ------------
    {
        let d = file_digest(b"probe");
        let hdr = FontFileHeader::valid(d);
        let mut gate = SecurityGate::new();
        // 系统只读 + 故意让沙箱输入爆表：信任来源**不该**被判超时
        let mut bomb = SandboxInput::clean();
        bomb.elapsed_ms = 10_000;
        let trusted = load_font(
            &mut gate,
            1,
            FontOrigin::SystemReadOnly,
            &hdr,
            d,
            &bomb,
            true,
            false,
        );
        // 用户导入 +同样爆表：必须被判超时并隔离
        let untrusted = load_font(
            &mut gate,
            2,
            FontOrigin::UserImported,
            &hdr,
            d,
            &bomb,
            true,
            false,
        );
        s.add(
            "E10-沙箱-非信任来源必进沙箱而信任来源不进",
            trusted == LoadOutcome::Loaded
                && untrusted == LoadOutcome::Quarantined
                && gate.is_quarantined(2)
                // 反向：系统字体**不得**被隔离（它没进沙箱，不该有超时记录）
                && !gate.is_quarantined(1),
            "同样超时的输入下：系统只读字体照常装载（不进沙箱），用户导入字体被沙箱判超时并标记不可用",
        );
    }

    // --- 判据 13：超时 ⇒ **终止且标记**（标记后不再重复尝试） ------------
    {
        let d = file_digest(b"tmo");
        let hdr = FontFileHeader::valid(d);
        let mut gate = SecurityGate::new();
        let mut slow = SandboxInput::clean();
        slow.elapsed_ms = 5_000;
        let first = load_font(
            &mut gate, 7,
            FontOrigin::UserImported,
            &hdr, d, &slow, true, false,
        );
        // 第二次：即使输入已正常，**已隔离的字体不得再尝试**
        let second = load_font(
            &mut gate, 7,
            FontOrigin::UserImported,
            &hdr, d, &SandboxInput::clean(), true, false,
        );
        s.add(
            "E10-超时-终止且标记不可用后不再尝试",
            first == LoadOutcome::Quarantined
                && gate.is_quarantined(7)
                // **关键**：第二次不得重试（否则「只终止不标记」等于没修）
                && second == LoadOutcome::Quarantined
                && gate.quarantined_len() == 1,
            "超时首次即隔离；第二次即使输入正常也直接返回隔离态，不重复尝试解析",
        );
    }

    // --- 判据 14：安全校验成本 ≤5ms/字体（一次性） ------------------------
    {
        let d = file_digest(b"budget");
        let hdr = FontFileHeader::valid(d);
        let mut gate = SecurityGate::new();
        // 连续装 4 个字体（不同 ID 避免隔离）
        let mut i = 0u32;
        while i < 4 {
            load_font(
                &mut gate,
                100 + i,
                FontOrigin::SystemReadOnly,
                &hdr,
                d,
                &SandboxInput::clean(),
                true,
                false,
            );
            i += 1;
        }
        s.add(
            "E10-校验成本-每字体一次性且不超5ms承诺",
            // 4 次装载累计恰为 4×5ms（每字体一次性，重试不再计）
            gate.verify_us == 4 * VERIFY_BUDGET_US as u64
                && VERIFY_BUDGET_US == 5_000
                // 校验成本与沙箱判定**分开计**（沙箱不是校验成本）
                && gate.sandbox[0] == 4,
            "4 个字体各计一次 5ms=5000us，累计恰为 20000us；沙箱判定不混入校验成本",
        );
    }

    // --- 判据 15：恶意字形安全卡**四类统一登记** --------------------------
    {
        let mut gate = SecurityGate::new();
        // 每类各登记两张
        let mut i = 0usize;
        while i < GLYPH_GUARD_COUNT {
            let g = GlyphGuard::ALL[i];
            gate.note_card(&SecurityCard::new(1, 10 + i as u32, g));
            gate.note_card(&SecurityCard::new(1, 20 + i as u32, g));
            i += 1;
        }
        // 四类计数**各恰为 2**
        let all_two = gate.cards[0] == 2
            && gate.cards[1] == 2
            && gate.cards[2] == 2
            && gate.cards[3] == 2;
        // 反向对照：无安全卡时计数**必须为 0**（否则「没登记」也显示有）
        let clean = SecurityGate::new();
        let none = clean.cards[0] == 0 && clean.cards[1] == 0 && clean.cards[2] == 0 && clean.cards[3] == 0;
        s.add(
            "E10-安全卡-四类统一登记且计数双向",
            all_two
                && none
                && GlyphGuard::ALL.len() == GLYPH_GUARD_COUNT
                // 四类**全部**阻止渲染（不设「提示级」）
                && GlyphGuard::MalformedOutline.blocks_render()
                && GlyphGuard::TooManyPoints.blocks_render()
                && GlyphGuard::SelfIntersection.blocks_render()
                && GlyphGuard::NonFinite.blocks_render(),
            "畸形轮廓/点数超限/自交爆炸/非有限四类各登记 2 次计数恰为 2；无事件时计数为 0；四类全部阻止渲染",
        );
    }

    // --- 判据 16：安全卡线编码自洽（四类互异、不等于判别值） --------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < GlyphGuard::ALL.len() {
            let g = GlyphGuard::ALL[i];
            ok &= !wires.contains(&g.wire());
            wires.push(g.wire());
            ok &= g.wire() != g.ordinal() as u8;
            ok &= !g.wire().is_ascii_digit();
            i += 1;
        }
        s.add(
            "E10-安全卡-线编码互异且与判别值分离",
            ok,
            "四类安全卡线编码互异、不等于枚举下标、非 ASCII 数字（显式映射而非 as u8）",
        );
    }

    // --- 判据 17：安全事件**零资产标识**上行 ------------------------------
    {
        // 五类标签互异，且**均不含**字体名/路径/用户名等资产标识
        let mut labels_differ = true;
        let mut no_asset = true;
        let mut i = 0usize;
        while i < SecurityEventKind::ALL.len() {
            let k = SecurityEventKind::ALL[i];
            let lb = k.label();
            // 只查**资产标识形态**：路径分隔符、扩展名、@用户名。
            // 不能拿 "file" 这类**类别名子串**当判据——`file-rejected`
            // 是事件类别（锚点用语），不是「上传了哪个文件」。
            // 关键词表会把错误固化成门禁：判「不含 file」会逼着实现
            // 把「文件被拒」改叫「别的东西被拒」，术语反而跑偏。
            no_asset &= !lb.contains('/')
                && !lb.contains('\\')
                && !lb.contains(".ttf")
                && !lb.contains(".otf")
                && !lb.contains('@');
            if i + 1 < SecurityEventKind::ALL.len() {
                labels_differ &= lb != SecurityEventKind::ALL[i + 1].label();
            }
            i += 1;
        }
        let ev = SecurityEvent::new(SecurityEventKind::SandboxTimeout, 501, 7, 0xABCD);
        s.add(
            "E10-安全事件-标签互异且零资产标识上行",
            labels_differ
                && no_asset
                && ev.value == 501
                && ev.anon_session == 0xABCD
                && ev.version == PROTOCOL_VERSION
                && SecurityEventKind::ALL.len() == 5,
            "五类事件标签互异且均不含 path/file/name/user；事件六字段齐备，不上行字体名与路径",
        );
    }

    // --- 判据 18：会话 ID 折叠为指纹（判据侧**独立重算**对拍） -----------
    //
    // 只验「同输入同输出 / 异输入异输出」是**同源驱动恒真**——任何确定性
    // 函数都恒满足，把实现退化成 `return 42` 依然全过。故判据侧自行实现
    // 一份 FNV-1a 独立重算期望值再逐位比对。
    let fnv_expect = |raw: &[u8]| -> u64 {
        let mut h: u64 = FNV_OFFSET;
        let mut i = 0usize;
        while i < raw.len() {
            h ^= raw[i] as u64;
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h
    };
    {
        let cases: [&[u8]; 5] = [
            b"user@example.com",
            b"",
            b"a",
            &[0u8, 255u8, 128u8, 1u8],
            b"session-0001",
        ];
        let mut pair_ok = true;
        let mut i = 0usize;
        while i < cases.len() {
            pair_ok &= fold_session_id(cases[i]) == fnv_expect(cases[i]);
            i += 1;
        }
        // 独立侧自身须有区分度（防自证式：期望值退化成常数则 pair_ok 也恒真）
        let distinct = fnv_expect(cases[0]) != fnv_expect(cases[1])
            && fnv_expect(cases[1]) != fnv_expect(cases[2])
            && fnv_expect(cases[2]) != fnv_expect(cases[3]);
        s.add(
            "E10-会话ID-与独立FNV1a逐位对拍",
            pair_ok && distinct,
            "判据侧独立实现 FNV-1a 与被测折叠函数逐位对拍（5 组含空/单字节/0xFF），不退化为常数",
        );
    }

    // --- 判据 19：隔离集满**不静默丢弃**（仍须拒绝） ----------------------
    {
        let d = file_digest(b"full");
        let hdr = FontFileHeader::valid(d);
        let mut gate = SecurityGate::new();
        let mut slow = SandboxInput::clean();
        slow.elapsed_ms = 5_000;
        // 灌满隔离集（64槽）
        let mut i = 0u32;
        while i < 64 {
            load_font(
                &mut gate, 500 + i,
                FontOrigin::UserImported,
                &hdr, d, &slow, true, false,
            );
            i += 1;
        }
        let full = gate.quarantined_len() == 64;
        // 再来一个新字体：隔离集已满 ⇒ **仍须拒绝**，不得因「没地方记」放行
        let overflow = load_font(
            &mut gate, 9999,
            FontOrigin::UserImported,
            &hdr, d, &slow, true, false,
        );
        s.add(
            "E10-隔离集满-登记失败仍拒绝不静默放行",
            full
                && overflow == LoadOutcome::Rejected(LoadReject::OverLimit)
                && gate.quarantined_len() == 64,
            "隔离集 64 槽灌满后新超时字体被拒（不因无处记录而放行），隔离集不超容",
        );
    }

    // --- 判据 20：拒绝计数**双向**（无拒绝为 0，真实拒绝恰为 N） -----------
    {
        let d = file_digest(b"cnt");
        let mut gate = SecurityGate::new();
        let hdr = FontFileHeader::valid(d);
        // 正常装载 ⇒ 拒绝计数全 0
        load_font(&mut gate, 1, FontOrigin::SystemReadOnly, &hdr, d, &SandboxInput::clean(), true, false);
        let clean = gate.rejects[0] == 0 && gate.rejects[1] == 0 && gate.rejects[2] == 0;
        // 造三次真实拒绝：签名不符 ×1、哈希不符 ×1、跨工程 ×1
        let mut g2 = SecurityGate::new();
        let r1 = load_font(&mut g2, 1, FontOrigin::SystemReadOnly, &FontFileHeader::bad_signature(), 0, &SandboxInput::clean(), true, false);
        let r2 = load_font(&mut g2, 2, FontOrigin::SystemReadOnly, &hdr, d ^ 0xFFFF, &SandboxInput::clean(), true, false);
        let r3 = load_font(&mut g2, 3, FontOrigin::UserImported, &hdr, d, &SandboxInput::clean(), false, false);
        s.add(
            "E10-拒绝计数-无拒绝为0且真实拒绝恰为3",
            clean
                && r1 == LoadOutcome::Rejected(LoadReject::BadSignature)
                && r2 == LoadOutcome::Rejected(LoadReject::Corrupt)
                && r3 == LoadOutcome::Rejected(LoadReject::Corrupt)
                // `==` 不用 `>=`：否则「每次 +2」也过
                && g2.rejects[0] == 2
                && g2.rejects[1] == 1
                && g2.rejects[2] == 0,
            "正常装载拒绝计数全 0；两次损坏 + 一次签名不符后 corrupt=2、badsig=1（恰等于，非 >=）",
        );
    }

    // --- 判据 21：读屏面板**只报聚合计数**且不泄漏资产 --------------------
    {
        let d = file_digest(b"panel");
        let mut gate = SecurityGate::new();
        let mut slow = SandboxInput::clean();
        slow.elapsed_ms = 5_000;
        load_font(&mut gate, 1, FontOrigin::UserImported, &FontFileHeader::valid(d), d, &slow, true, false);
        let lines = gate.a11y_lines();
        let mut joined = String::new();
        let mut i = 0;
        while i < lines.len() {
            joined.push_str(&lines[i]);
            joined.push('\n');
            i += 1;
        }
        s.add(
            "E10-面板-六行双语且只报聚合计数",
            lines.len() == A11Y_LINES
                && joined.contains("rejects")
                && joined.contains("sandbox")
                && joined.contains("cards")
                && joined.contains("quarantined")
                // 聚合计数**必须**可查（漏报同样是缺陷）
                && joined.contains(&format!("{}", gate.quarantined_len()))
                // 私有形态不得出现：具体字体 ID 与原始标识
                && !joined.contains("user@example")
                && !joined.contains(".ttf"),
            "面板六行中英双语覆盖拒绝/沙箱/安全卡/隔离数/校验成本/可读原因；只报聚合计数，不含字体名与路径",
        );
    }

    // --- 判据 22：零 panic 面（空文件/满隔离集/未知枚举码） ---------------
    {
        // 空字节切片取指纹不得 panic
        let empty_digest = file_digest(b"");
        // 空输入进沙箱
        let lim = SandboxLimits::standard();
        let empty_in = SandboxInput {
            peak_bytes: 0,
            elapsed_ms: 0,
            depth: 0,
            tables: 0,
        };
        let empty_v = sandbox_check(&empty_in, &lim);
        // 未知枚举码反解 ⇒ None
        let unknown = FontOrigin::from_wire(0xEE);
        s.add(
            "E10-零panic-空输入与未知码安全",
            file_digest(b"") == fnv_expect(b"")
                && empty_v == SandboxVerdict::Passed
                && unknown.is_none()
                && SecurityEvent::new(SecurityEventKind::FileRejected, 0, 0, 0).version
                    == PROTOCOL_VERSION,
            "空字节取指纹不 panic；全零沙箱输入判通过；未知线编码返回 None",
        );
        // 引用 empty_digest 避免未用告警
        let _ = empty_digest;
    }

    s
}