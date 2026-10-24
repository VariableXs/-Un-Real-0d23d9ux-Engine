#![allow(dead_code)]
#![allow(unused_imports)]
use alloc::vec;
// UNX-F3461 · 分域挂载总设计（框架与各域用例集的挂载架构）
//
// 任务单号: UNX-F3461
// 标题: 分域挂载总设计（框架与各域用例集的挂载架构）
// 域/批: A4/G8（CoRun·UNX-A5 段｜ktest/框架/测试基建域）
// 册目标: 340 行（判据 J1 挂载架构文档 + M-220 挂载全链路）
//
// 判据原文（逐字摘抄，来源《CoRun Varix STAR II · Unxreal》任务书表格）:
// 「UNX-F3461 | 分域挂载总设计（框架与各域用例集的挂载架构） | 340 | 骨架 |
//   J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）交付；
//   M-220 挂载全链路（域声明 → 注册表合并 → 执行器可见）。」
//
// 说明（锚点目录同名文件的辨析）:
// `_attic/.../anchors/UNX-F3461.md` 内实为 **UNX-F34610 · 版本降级 oracle 防护（TLS_FALLBACK_SCSV 语义位）**，
// 与本单无关。本单判据以任务书表格 `| UNX-F3461 |` 行为准。
//
// 本单核心语义（双向登记制，本单最关键的设计面）:
// - **两个方向都要查**：
//   - 域侧有、框架侧无 → **漏挂**（`MissingInRegistry`）：域白写了，用例永远跑不到。
//   - 框架侧有、域侧无 → **幽灵挂载**（`GhostMount`）：框架以为挂了，域根本不认，
//     典型症状是「执行器可见但恒跳过」。两类都必须显性检出，一个都不能放行。
// - **合并语义**：框架侧注册表由各域声明**合并**而成，合并必须**幂等**
//   （同一用例重复合并不改变注册表）与**全序确定**（合并序固定，冻结哈希可复现）。
// - **可见性**：`Executor` 只能看见 `Registry` 中状态为 `Mounted` 的用例；
//   「注册表合并 → 执行器可见」这条链路每一环都要留痕（M-220）。
// - **版本化与冻结哈希**：`Registry` 带版本号与冻结哈希；
//   **冻结哈希只折「用例集合与挂载状态」**，绝不混入访问计数、合并流水等活账。
//
// 深度骨架:
// 常量表 / 4 核心结构（DomainDecl、Registry、MergeLog、ExecView）/
// 14 错误变体 / 6 辅助码 / 主状态机（合法边闭集 + 非法转换拒记）/
// 守恒账 `reconcile()` / append-only 环形账（`dropped` 显性）/
// 注入核 14/14 全拒留痕 / 冻结指纹 `fingerprint() + verify()` /
// `export_kv() + export_tlv()` / 确定性 `replay(seed)` / `observe(seed)` /
// `complexity()` 逐项性能分解（对标 Windows 同类能力写「深在哪」）。
//
// 码段声明:
// 本单独占诊断码段 `0x2940 ..= 0x297F`（`SEG_BASE = 0x2940`，`SEG_MASK = 0x003F`）。
// 错误变体一码一段，逐个占段内偏移 `0x00..=0x0D`；辅助码占 `0x1C..=0x21`，
// 最大占用 `0x2961`，未触及段上界 `0x297F`。

use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

extern crate alloc;


// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 版本标识。
pub const V_VERSION_ID: u64 = 0x0001_3461_2940_0001;
/// 版本串。
pub const V_VERSION: &str = "v3461-umountarch.v1";
/// 码段基址。
pub const SEG_BASE: u16 = 0x2940;
/// 码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 码段上界。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 辅助码段内基址。
pub const AUX_BASE_OFF: u16 = 0x1C;

// ---------------------------------------------------------------------------
// 1.1 挂载域常量
// ---------------------------------------------------------------------------

/// 域总数。
pub const DOMAIN_COUNT: usize = 6;
/// 每域用例数。
pub const CASES_PER_DOMAIN: usize = 3;
/// 用例总数（`DOMAIN_COUNT × CASES_PER_DOMAIN`）。
pub const CASE_TOTAL: usize = DOMAIN_COUNT * CASES_PER_DOMAIN;
/// 注册表容量。
pub const REG_CAP: usize = 24;
/// 注册表环掩码。
pub const REG_MASK: usize = REG_CAP - 1;
/// 合并轮次（幂等性验证轮）。
pub const MERGE_ROUNDS: usize = 2;
/// 环形账容量。
pub const RING_CAP: usize = 32;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 总步数上限。
pub const STEP_CAP: u64 = (DOMAIN_COUNT * CASES_PER_DOMAIN * MERGE_ROUNDS * 8) as u64;
/// syscall 次数上限（零真实 IO）。
pub const SYSCALL_CAP: u64 = 0;
/// 峰值内存上限。
pub const MEM_PEAK_CAP: u64 = 512 * 1024;

/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// 单条 TLV 定长。
pub const TLV_ITEM_LEN: usize = 10;
/// TLV 条目总数（静态 34 + 逐形态 14 = 48；由 export_tlv 内 push 次数反推）。
pub const TLV_ITEM_COUNT: usize = 48;

/// 域侧域面名。
pub const FACE_DOMAIN: &str = "域侧面";
/// 框架侧域面名。
pub const FACE_REGISTRY: &str = "框架侧面";
/// 可见域面名。
pub const FACE_EXEC: &str = "可见面";
/// 账目域面名。
pub const FACE_LEDGER: &str = "账目面";
/// 冻结域面名。
pub const FACE_FREEZE: &str = "冻结面";

/// 嫁接源。
pub const GRAFT_SEAT: &str = "F3401/F3421";
/// 对位条位。
pub const DEP_SEAT: &str = "F3361/F3381";

// ===========================================================================
// 2. 错误闭集
// ===========================================================================

/// 错误形态（段内偏移 `0x00..=0x0D`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0：漏挂（域侧有用例，框架侧无）。
    MissingInRegistry,
    /// 1：幽灵挂载（框架侧有用例，域侧无）。
    GhostMount,
    /// 2：域声明重号（同一用例在域内重复声明）。
    DeclDup,
    /// 3：域声明缺项（域内应有条位数不符）。
    DeclMissing,
    /// 4：注册表重号。
    RegDup,
    /// 5：注册表缺项。
    RegMissing,
    /// 6：合并不幂等（重复合并改变注册表）。
    MergeNotIdempotent,
    /// 7：合并序不确定（同输入两序结果不同）。
    MergeOrderUnstable,
    /// 8：挂载状态越闭集。
    MountStateIllegal,
    /// 9：执行器看见未挂载用例。
    ExecSeesUnmounted,
    /// 10：执行器看不见已挂载用例。
    ExecBlindToMounted,
    /// 11：环形账不守恒。
    LedgerUnbalanced,
    /// 12：守恒式偏离。
    ReconcileDeviation,
    /// 13：冻结指纹漂移（注册表版本化哈希失配）。
    FingerprintDrift,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态。
    pub const ALL: [Form; Form::COUNT] = [
        Form::MissingInRegistry,
        Form::GhostMount,
        Form::DeclDup,
        Form::DeclMissing,
        Form::RegDup,
        Form::RegMissing,
        Form::MergeNotIdempotent,
        Form::MergeOrderUnstable,
        Form::MountStateIllegal,
        Form::ExecSeesUnmounted,
        Form::ExecBlindToMounted,
        Form::LedgerUnbalanced,
        Form::ReconcileDeviation,
        Form::FingerprintDrift,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::MissingInRegistry => 0,
            Form::GhostMount => 1,
            Form::DeclDup => 2,
            Form::DeclMissing => 3,
            Form::RegDup => 4,
            Form::RegMissing => 5,
            Form::MergeNotIdempotent => 6,
            Form::MergeOrderUnstable => 7,
            Form::MountStateIllegal => 8,
            Form::ExecSeesUnmounted => 9,
            Form::ExecBlindToMounted => 10,
            Form::LedgerUnbalanced => 11,
            Form::ReconcileDeviation => 12,
            Form::FingerprintDrift => 13,
        }
    }
    /// 按下标取形态。
    pub fn from_index(i: usize) -> Option<Form> {
        if i < Form::COUNT {
            Some(Form::ALL[i])
        } else {
            None
        }
    }
    /// 诊断码（`SEG_BASE + 偏移`）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::MissingInRegistry => "MissingInRegistry",
            Form::GhostMount => "GhostMount",
            Form::DeclDup => "DeclDup",
            Form::DeclMissing => "DeclMissing",
            Form::RegDup => "RegDup",
            Form::RegMissing => "RegMissing",
            Form::MergeNotIdempotent => "MergeNotIdempotent",
            Form::MergeOrderUnstable => "MergeOrderUnstable",
            Form::MountStateIllegal => "MountStateIllegal",
            Form::ExecSeesUnmounted => "ExecSeesUnmounted",
            Form::ExecBlindToMounted => "ExecBlindToMounted",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::ReconcileDeviation => "ReconcileDeviation",
            Form::FingerprintDrift => "FingerprintDrift",
        }
    }
    /// 形态描述。
    pub fn describe(self) -> &'static str {
        match self {
            Form::MissingInRegistry => "漏挂：域侧声明了用例，框架注册表查无此条",
            Form::GhostMount => "幽灵挂载：框架注册表有此条，域侧根本没有声明",
            Form::DeclDup => "域声明重号：同一用例在域内被声明两次",
            Form::DeclMissing => "域声明缺项：域内应有条数不足",
            Form::RegDup => "注册表重号：同一用例在注册表出现多次",
            Form::RegMissing => "注册表缺项：应有条位查无记录",
            Form::MergeNotIdempotent => "合并不幂等：重复合并改变了注册表",
            Form::MergeOrderUnstable => "合并序不确定：相同输入不同合并序产出不同结果",
            Form::MountStateIllegal => "挂载状态越闭集：不在 Mounted/Unmounted/Shadow 内",
            Form::ExecSeesUnmounted => "执行器看见未挂载用例（可见性越权）",
            Form::ExecBlindToMounted => "执行器看不见已挂载用例（挂载失效）",
            Form::LedgerUnbalanced => "环形账不守恒：写入数 ≠ 存活 + 丢弃",
            Form::ReconcileDeviation => "守恒式偏离：域声明数 ≠ 注册表数",
            Form::FingerprintDrift => "冻结指纹漂移：注册表版本化哈希与入册值失配",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::MissingInRegistry => "decl_vs_reg.missing",
            Form::GhostMount => "decl_vs_reg.ghost",
            Form::DeclDup => "decl.dup_bits",
            Form::DeclMissing => "decl.cover_bits",
            Form::RegDup => "reg.dup_bits",
            Form::RegMissing => "reg.cover_bits",
            Form::MergeNotIdempotent => "merge.idempotent",
            Form::MergeOrderUnstable => "merge.order_stable",
            Form::MountStateIllegal => "reg.state_closed",
            Form::ExecSeesUnmounted => "exec.only_mounted",
            Form::ExecBlindToMounted => "exec.sees_all_mounted",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::ReconcileDeviation => "recon.deviation",
            Form::FingerprintDrift => "freeze.verify",
        }
    }
    /// 所属域面。
    pub fn face(self) -> &'static str {
        match self {
            Form::MissingInRegistry | Form::GhostMount => FACE_REGISTRY,
            Form::DeclDup | Form::DeclMissing => FACE_DOMAIN,
            Form::RegDup | Form::RegMissing | Form::MountStateIllegal => FACE_REGISTRY,
            Form::MergeNotIdempotent | Form::MergeOrderUnstable => FACE_REGISTRY,
            Form::ExecSeesUnmounted | Form::ExecBlindToMounted => FACE_EXEC,
            Form::LedgerUnbalanced | Form::ReconcileDeviation => FACE_LEDGER,
            Form::FingerprintDrift => FACE_FREEZE,
        }
    }
    /// 是否属「双向登记对账面」。
    pub fn is_bidir(self) -> bool {
        matches!(self, Form::MissingInRegistry | Form::GhostMount)
    }
    /// 是否属「域侧/注册表面」。
    pub fn is_reg(self) -> bool {
        matches!(
            self,
            Form::DeclDup
                | Form::DeclMissing
                | Form::RegDup
                | Form::RegMissing
                | Form::MergeNotIdempotent
                | Form::MergeOrderUnstable
                | Form::MountStateIllegal
        )
    }
    /// 是否属「可见性面」。
    pub fn is_exec(self) -> bool {
        matches!(self, Form::ExecSeesUnmounted | Form::ExecBlindToMounted)
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_bidir() {
            "双向对账面"
        } else if self.is_exec() {
            "可见性面"
        } else if self.is_reg() {
            "登记面"
        } else {
            "账目/冻结面"
        }
    }
}
/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0：步数越界。
    StepOverCap,
    /// 1：域下标越界。
    DomainIdxOver,
    /// 2：用例下标越界。
    CaseIdxOver,
    /// 3：注册表下标越界。
    RegIdxOver,
    /// 4：挂载状态码越界。
    StateCodeOver,
    /// 5：语义边界越界（本单不真改任何域的挂载表）。
    SemanticBorder,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::StepOverCap,
        AuxCode::DomainIdxOver,
        AuxCode::CaseIdxOver,
        AuxCode::RegIdxOver,
        AuxCode::StateCodeOver,
        AuxCode::SemanticBorder,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::StepOverCap => 0,
            AuxCode::DomainIdxOver => 1,
            AuxCode::CaseIdxOver => 2,
            AuxCode::RegIdxOver => 3,
            AuxCode::StateCodeOver => 4,
            AuxCode::SemanticBorder => 5,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            .wrapping_add(AUX_BASE_OFF)
            .wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "StepOverCap",
            AuxCode::DomainIdxOver => "DomainIdxOver",
            AuxCode::CaseIdxOver => "CaseIdxOver",
            AuxCode::RegIdxOver => "RegIdxOver",
            AuxCode::StateCodeOver => "StateCodeOver",
            AuxCode::SemanticBorder => "SemanticBorder",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::StepOverCap => "逻辑步数越过 STEP_CAP 上限",
            AuxCode::DomainIdxOver => "域下标越界",
            AuxCode::CaseIdxOver => "用例下标越界",
            AuxCode::RegIdxOver => "注册表下标越界",
            AuxCode::StateCodeOver => "挂载状态码越界",
            AuxCode::SemanticBorder => "语义边界越界：本单不真改任何域的挂载表",
        }
    }
}

/// 本单结果别名。
pub type MaResult<T> = Result<T, MaErr>;
/// 错误对象（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MaErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl MaErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> MaErr {
        MaErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> MaErr {
        MaErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> MaErr {
        MaErr { code: a.code(), detail }
    }
    /// 三要素之一：码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明细。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 码 → 名称反查。
pub fn name_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        if f.code() == code {
            return Some(f.name());
        }
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < AuxCode::COUNT {
        let a = AuxCode::ALL[j];
        if a.code() == code {
            return Some(a.name());
        }
        j = j.wrapping_add(1);
    }
    None
}
/// 错误一行串。
pub fn err_str(e: &MaErr) -> String {
    match name_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 冻结态不符错误。
pub fn err_frozen(got: &str, want: &str) -> MaErr {
    MaErr::of_form(
        Form::FingerprintDrift,
        format!("冻结态不符：got={} want={}", got, want),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> MaErr {
    MaErr::of_aux(
        AuxCode::CaseIdxOver,
        format!("越界：{} idx={} cap={}", what, idx, cap),
    )
}
/// 守恒式错误。
pub fn err_recon(a: u64, b: u64, c: u64) -> MaErr {
    MaErr::of_form(
        Form::ReconcileDeviation,
        format!("守恒式偏离：域声明={} 注册表={} 总账={}", a, b, c),
    )
}
/// 码段合法性。
pub fn code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}
/// 主码串。
pub fn seg_line() -> String {
    format!(
        "seg 0x{:04X}..=0x{:04X} mask=0x{:04X} forms={} aux={} max=0x{:04X}",
        SEG_BASE,
        SEG_END,
        SEG_MASK,
        Form::COUNT,
        AuxCode::COUNT,
        AuxCode::ALL[AuxCode::COUNT - 1].code()
    )
}
// ===========================================================================
// 2. 确定性基础件
// ===========================================================================

/// 确定性 LCG。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 状态。
    pub state: u64,
}
impl Lcg {
    /// 构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        (self.next() & 0xFFFF_FFFF) as u32
    }
    /// 取模取样。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
    /// 区间取样（`[lo, hi)`；`hi <= lo` 时给 `lo`）。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo.wrapping_add(self.below(hi - lo))
    }
}
/// LCG 单步。
pub fn lcg_step(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005u64)
        .wrapping_add(1442695040888963407u64)
}
/// FNV-1a 64 增量。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    for b in bytes.iter() {
        let x: u64 = (*b as u64) & 0xFF;
        h = (h ^ x).wrapping_mul(0x100000001b3u64);
    }
    h
}
/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(0xcbf29ce484222325u64, bytes)
}
/// FNV-1a 64 词表指纹。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for w in words.iter() {
        let v: u64 = *w;
        let mut i: usize = 0;
        while i < 8 {
            let byte: u8 = ((v >> (56 - 8 * i)) & 0xFF) as u8;
            h = (h ^ (byte as u64)).wrapping_mul(0x100000001b3u64);
            i = i.wrapping_add(1);
        }
    }
    h
}
/// 字符串折入。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}
/// 逻辑时钟推进（零墙钟）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(1)
}
/// 整除保护。
pub fn div_or_zero(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}
/// 64 位整数平方根（牛顿迭代，零浮点）。
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut r: u64 = 1u64 << 32;
    let mut guard: u32 = 0;
    while guard < 80 {
        let y: u64 = (r.wrapping_add(n / r)) >> 1;
        if y >= r {
            break;
        }
        r = y;
        guard = guard.wrapping_add(1);
    }
    r
}

// ===========================================================================
// 3. 错误闭集
// ===========================================================================


// ===========================================================================
// 4. 核心结构一：域侧声明（DomainDecl）
// ===========================================================================

/// 挂载状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountState {
    /// 未挂载（已声明但未进注册表）。
    Unmounted,
    /// 已挂载（执行器可见）。
    Mounted,
    /// 影子挂载（登记但不执行，双跑比对期）。
    Shadow,
}
impl MountState {
    /// 闭集大小。
    pub const COUNT: usize = 3;
    /// 全部状态。
    pub const ALL: [MountState; MountState::COUNT] = [
        MountState::Unmounted,
        MountState::Mounted,
        MountState::Shadow,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            MountState::Unmounted => 0,
            MountState::Mounted => 1,
            MountState::Shadow => 2,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<MountState> {
        if i < MountState::COUNT {
            Some(MountState::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            MountState::Unmounted => "Unmounted",
            MountState::Mounted => "Mounted",
            MountState::Shadow => "Shadow",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            MountState::Unmounted => "未挂载",
            MountState::Mounted => "已挂载",
            MountState::Shadow => "影子挂载",
        }
    }
    /// 执行器是否可见（**只有 `Mounted` 可见**）。
    pub fn exec_visible(self) -> bool {
        self == MountState::Mounted
    }
}
/// 域侧声明条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeclItem {
    /// 用例键（`域号:用例号`）。
    pub key: String,
    /// 域号。
    pub domain: usize,
    /// 域内用例号。
    pub case: usize,
    /// 域名。
    pub domain_name: String,
    /// 域侧期望挂载状态。
    pub want: MountState,
    /// 存活位。
    pub live: u8,
}
impl DeclItem {
    /// 构造。
    pub fn new(domain: usize, case: usize, domain_name: String, want: MountState) -> DeclItem {
        let key: String = format!("d{}:c{}", domain, case);
        DeclItem { key, domain, case, domain_name, want, live: 1 }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "decl {:<10} {:<14} {:<10} want={:<10} live={}",
            self.key, self.domain_name, self.domain_name, self.want.name(), self.live
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 4] {
        [fnv1a64(self.key.as_bytes()), self.domain as u64, self.case as u64, self.want.index() as u64]
    }
    /// 用例指纹（跨两侧比对的同一把尺）。
    pub fn fp(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(4);
        w.push(V_VERSION_ID);
        w.push(self.domain as u64);
        w.push(self.case as u64);
        w.push(fnv1a64(self.domain_name.as_bytes()));
        fnv1a_words(&w)
    }
}
/// 域名表。
pub const DOMAIN_NAMES: [&str; DOMAIN_COUNT] = [
    "net", "sched", "mm", "fs", "dev", "sec",
];
/// 域侧声明册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DomainDecl {
    /// 声明条目。
    pub items: Vec<DeclItem>,
    /// 存在位图（`CASE_TOTAL = 18 < 32`）。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
}
impl DomainDecl {
    /// 净态声明册（`DOMAIN_COUNT × CASES_PER_DOMAIN` 条）。
    pub fn net() -> MaResult<DomainDecl> {
        let mut items: Vec<DeclItem> = Vec::with_capacity(CASE_TOTAL);
        let mut cover_bits: u32 = 0;
        let mut d: usize = 0;
        while d < DOMAIN_COUNT {
            let dn: String = String::from(DOMAIN_NAMES[d]);
            let mut c: usize = 0;
            while c < CASES_PER_DOMAIN {
                // 每域最后一条走影子挂载，其余已挂载
                let want: MountState = if c + 1 == CASES_PER_DOMAIN {
                    MountState::Shadow
                } else {
                    MountState::Mounted
                };
                items.push(DeclItem::new(d, c, dn.clone(), want));
                cover_bits = cover_bits | (1u32 << (items.len() - 1 & 31));
                c = c.wrapping_add(1);
            }
            d = d.wrapping_add(1);
        }
        Ok(DomainDecl { items, cover_bits, dup_bits: 0 })
    }
    /// 覆盖完备。
    pub fn cover_complete(&self) -> bool {
        let want: u32 = if CASE_TOTAL >= 32 {
            0xFFFF_FFFFu32
        } else {
            (1u32 << CASE_TOTAL) - 1u32
        };
        self.cover_bits & want == want
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 存活声明数。
    pub fn live_n(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].live == 1 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 按用例指纹查找。
    pub fn find_by_fp(&self, fp: u64) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].live == 1 && self.items[i].fp() == fp {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }
    /// 指纹（冻结件：只折声明定义）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + self.items.len() * 4);
        w.push(V_VERSION_ID);
        w.push(self.cover_bits as u64);
        w.push(self.items.len() as u64);
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 4 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "decl items={} live={} cover={:08X} dup={:08X} fp={:016X}",
            self.items.len(),
            self.live_n(),
            self.cover_bits,
            self.dup_bits,
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 5. 核心结构二：框架侧统一注册表（Registry）
// ===========================================================================

/// 注册表条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegItem {
    /// 序号。
    pub slot: usize,
    /// 用例指纹（与域侧同一把尺）。
    pub fp: u64,
    /// 用例键（可读）。
    pub key: String,
    /// 挂载状态。
    pub state: MountState,
    /// 访问计数（**活账，绝不进冻结**）。
    pub visits: u64,
    /// 有效位。
    pub live: u8,
}
impl RegItem {
    /// 构造。
    pub fn new(slot: usize, fp: u64, key: String, state: MountState) -> RegItem {
        RegItem { slot, fp, key, state, visits: 0, live: 1 }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "reg#{:<3} {:<10} state={:<10} visits={} live={}",
            self.slot, self.key, self.state.name(), self.visits, self.live
        )
    }
    /// 指纹词（冻结件：只折定义，不折 visits）。
    pub fn words(&self) -> [u64; 4] {
        [self.slot as u64, self.fp, self.state.index() as u64, self.live as u64]
    }
}
/// 框架侧统一注册表（版本化 + 冻结哈希）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Registry {
    /// 注册表版本。
    pub rev: u32,
    /// 条目。
    pub items: Vec<RegItem>,
    /// 存在位图。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 写指针。
    pub head: usize,
    /// 被覆盖丢弃数（显性）。
    pub dropped: u64,
}
impl Registry {
    /// 空注册表。
    pub fn new(rev: u32) -> Registry {
        Registry {
            rev,
            items: Vec::with_capacity(REG_CAP),
            cover_bits: 0,
            dup_bits: 0,
            head: 0,
            dropped: 0,
        }
    }
    /// 合并登记一条（**幂等**：同指纹重复登记不改变内容）。
    pub fn merge_one(&mut self, fp: u64, key: String, state: MountState) -> MaResult<usize> {
        // 幂等前置：已有同指纹则直接返回既有槽位，不做任何写入
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].fp == fp {
                return Ok(i);
            }
            i = i.wrapping_add(1);
        }
        if self.items.len() >= REG_CAP {
            return Err(MaErr::of_aux(
                AuxCode::RegIdxOver,
                format!("注册表满：len={} cap={}", self.items.len(), REG_CAP),
            ));
        }
        let slot = self.items.len();
        self.items.push(RegItem::new(slot, fp, key, state));
        self.cover_bits = self.cover_bits | (1u32 << (slot & 31));
        self.head = (slot + 1) & REG_MASK;
        Ok(slot)
    }
    /// 按指纹查找。
    pub fn find_by_fp(&self, fp: u64) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].live == 1 && self.items[i].fp == fp {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }
    /// 存活条目数。
    pub fn live_n(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].live == 1 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 覆盖完备（按期望条数）。
    pub fn cover_complete(&self, expect: usize) -> bool {
        self.items.len() >= expect
    }
    /// 无重号。
    pub fn dup_free(&self) -> bool {
        self.dup_bits == 0
    }
    /// 挂载状态是否全在闭集内。
    pub fn state_closed(&self) -> bool {
        let mut i: usize = 0;
        while i < self.items.len() {
            if MountState::from_index(self.items[i].state.index()).is_none() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 已挂载条数。
    pub fn mounted_n(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].live == 1 && self.items[i].state == MountState::Mounted {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 指纹（**冻结件：只折条目定义与挂载状态，绝不折 visits**）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(4 + self.items.len() * 4);
        w.push(V_VERSION_ID);
        w.push(self.rev as u64);
        w.push(self.cover_bits as u64);
        w.push(self.items.len() as u64);
        let mut i: usize = 0;
        while i < self.items.len() {
            let ww = self.items[i].words();
            let mut j: usize = 0;
            while j < 4 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "reg rev={} items={} live={} mounted={} cover={:08X} dup={:08X} fp={:016X}",
            self.rev,
            self.items.len(),
            self.live_n(),
            self.mounted_n(),
            self.cover_bits,
            self.dup_bits,
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            out.push(format!("  {}", self.items[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 6. 核心结构三：合并流水（MergeLog）
// ===========================================================================

/// 合并日志条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeEntry {
    /// 轮次。
    pub round: u32,
    /// 域号。
    pub domain: usize,
    /// 本轮并入条数。
    pub merged: u32,
    /// 合并后注册表大小。
    pub reg_len: u32,
    /// 本轮是否幂等（无新增 = 幂等）。
    pub idempotent: u8,
}
impl MergeEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "merge r{} d{} merged={} reg_len={} idempotent={}",
            self.round, self.domain, self.merged, self.reg_len, self.idempotent
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [
            self.round as u64,
            self.domain as u64,
            self.merged as u64,
            self.reg_len as u64,
            self.idempotent as u64,
        ]
    }
}
/// 合并流水账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeLog {
    /// 条目。
    pub entries: Vec<MergeEntry>,
    /// 总并入数（含重复轮，故 ≥ 注册表条数）。
    pub merged_total: u64,
    /// 幂等轮数。
    pub idempotent_rounds: u64,
    /// 步数消耗。
    pub steps: u64,
    /// 步数上限。
    pub step_cap: u64,
}
impl MergeLog {
    /// 空账。
    pub fn new() -> MergeLog {
        MergeLog {
            entries: Vec::with_capacity(DOMAIN_COUNT * MERGE_ROUNDS),
            merged_total: 0,
            idempotent_rounds: 0,
            steps: 0,
            step_cap: STEP_CAP,
        }
    }
    /// 合并一个域的声明（**全序确定**：按域号升序遍历）。
    pub fn merge_domain(
        &mut self,
        reg: &mut Registry,
        decl: &DomainDecl,
        domain: usize,
        round: u32,
    ) -> MaResult<()> {
        if domain >= DOMAIN_COUNT {
            return Err(MaErr::of_aux(
                AuxCode::DomainIdxOver,
                format!("域下标越界 domain={} cap={}", domain, DOMAIN_COUNT),
            ));
        }
        if self.steps >= self.step_cap {
            return Err(MaErr::of_aux(
                AuxCode::StepOverCap,
                format!("步数越界 steps={} cap={}", self.steps, self.step_cap),
            ));
        }
        let before: usize = reg.items.len();
        let mut merged: u32 = 0;
        let mut i: usize = 0;
        while i < decl.items.len() {
            let it = &decl.items[i];
            if it.live == 1 && it.domain == domain {
                // 域侧期望状态：Shadow 的不进入 Mounted，但**必须登记**（否则成漏挂）
                let slot = reg.merge_one(it.fp(), it.key.clone(), it.want)?;
                if reg.items[slot].state != it.want {
                    if let Some(r) = reg.items.get_mut(slot) {
                        r.state = it.want;
                    }
                }
                merged = merged.wrapping_add(1);
                self.steps = self.steps.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        let grew: usize = reg.items.len() - before;
        let idempotent: u8 = if grew == 0 { 1 } else { 0 };
        if idempotent == 1 {
            self.idempotent_rounds = self.idempotent_rounds.wrapping_add(1);
        }
        self.merged_total = self.merged_total.wrapping_add(merged as u64);
        self.entries.push(MergeEntry {
            round,
            domain,
            merged,
            reg_len: reg.items.len() as u32,
            idempotent,
        });
        Ok(())
    }
    /// 是否全轮幂等（第二轮必须零新增）。
    pub fn all_idempotent(&self) -> bool {
        let mut seen_second: bool = false;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].round >= 1 {
                if self.entries[i].idempotent != 1 {
                    return false;
                }
                seen_second = true;
            }
            i = i.wrapping_add(1);
        }
        seen_second
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "mergelog entries={} merged_total={} idem_rounds={} all_idem={} steps={}",
            self.entries.len(),
            self.merged_total,
            self.idempotent_rounds,
            if self.all_idempotent() { 1 } else { 0 },
            self.steps
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.entries.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.entries.len() {
            out.push(format!("  {}", self.entries[i].describe()));
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 7. 核心结构四：执行器可见视图（ExecView）
// ===========================================================================

/// 执行器可见视图。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExecView {
    /// 可见的用例指纹（**只有 Mounted 可见**）。
    pub visible: Vec<u64>,
    /// 可见条目数。
    pub visible_n: u64,
    /// 越权可见数（看见未挂载）。
    pub leak_n: u64,
    /// 盲区数（看不见已挂载）。
    pub blind_n: u64,
    /// 步数消耗。
    pub steps: u64,
}
impl ExecView {
    /// 由注册表构建（可见性口径的唯一实现处）。
    pub fn build(reg: &Registry) -> ExecView {
        let mut visible: Vec<u64> = Vec::with_capacity(reg.items.len());
        // leak_n / blind_n 在构造期恒 0：可见性口径正确时不存在越权与盲区
        let mut leak_n: u64 = 0;
        let mut blind_n: u64 = 0;
        let mut steps: u64 = 0;
        let mut i: usize = 0;
        while i < reg.items.len() {
            let it = &reg.items[i];
            if it.live == 1 {
                if it.state.exec_visible() {
                    visible.push(it.fp);
                }
                // 注意：**非 Mounted 被排除是正确行为，不算越权**。
                // 越权（leak）只能由「本不该可见的指纹出现在 visible 里」来表达，
                // 那只能由注入侧直接篡改视图造成，构造期自然为 0。
            }
            steps = steps.wrapping_add(1);
            i = i.wrapping_add(1);
        }
        let visible_n: u64 = visible.len() as u64;
        ExecView { visible, visible_n, leak_n, blind_n, steps }
    }
    /// 构造「盲区」读数：注册表声明 Mounted 但视图里查不到的条数。
    pub fn recompute_blind(reg: &Registry) -> u64 {
        let v = ExecView::build(reg);
        let mut blind: u64 = 0;
        let mut i: usize = 0;
        while i < reg.items.len() {
            let it = &reg.items[i];
            if it.live == 1 && it.state == MountState::Mounted {
                let mut found: bool = false;
                let mut j: usize = 0;
                while j < v.visible.len() {
                    if v.visible[j] == it.fp {
                        found = true;
                    }
                    j = j.wrapping_add(1);
                }
                if !found {
                    blind = blind.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        blind
    }
    /// 是否无越权。
    pub fn no_leak(&self) -> bool {
        self.leak_n == 0
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "execview visible={} leak={} blind={} steps={}",
            self.visible_n, self.leak_n, self.blind_n, self.steps
        )
    }
}

// ===========================================================================
// 8. 主状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 挂载全链路阶段（域声明 → 注册表合并 → 执行器可见 → 双向对账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 段一：域侧声明用例集。
    D1Declare,
    /// 段二：框架侧注册表合并。
    D2Merge,
    /// 段三：执行器可见性构建。
    D3Visible,
    /// 段四：双向对账（漏挂 / 幽灵挂载）。
    D4Bidir,
    /// 段五：注册表版本冻结。
    D5Freeze,
    /// 终态。
    Sealed,
}
impl Stage {
    /// 阶段数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部阶段。
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::D1Declare,
        Stage::D2Merge,
        Stage::D3Visible,
        Stage::D4Bidir,
        Stage::D5Freeze,
        Stage::Sealed,
    ];
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Stage::D1Declare => 0,
            Stage::D2Merge => 1,
            Stage::D3Visible => 2,
            Stage::D4Bidir => 3,
            Stage::D5Freeze => 4,
            Stage::Sealed => 5,
        }
    }
    /// 按下标取。
    pub fn from_index(i: usize) -> Option<Stage> {
        if i < Stage::COUNT {
            Some(Stage::ALL[i])
        } else {
            None
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Stage::D1Declare => "D1Declare",
            Stage::D2Merge => "D2Merge",
            Stage::D3Visible => "D3Visible",
            Stage::D4Bidir => "D4Bidir",
            Stage::D5Freeze => "D5Freeze",
            Stage::Sealed => "Sealed",
        }
    }
    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Stage::D1Declare => "域侧声明用例集",
            Stage::D2Merge => "框架侧注册表合并",
            Stage::D3Visible => "执行器可见视图",
            Stage::D4Bidir => "双向对账（漏挂/幽灵）",
            Stage::D5Freeze => "注册表版本冻结",
            Stage::Sealed => "收口",
        }
    }
    /// 是否终态。
    pub fn is_sealed(self) -> bool {
        self == Stage::Sealed
    }
    /// 四列输出。
    pub fn four_columns(&self) -> String {
        let nxt: &'static str = match self {
            Stage::D1Declare => "D2Merge",
            Stage::D2Merge => "D3Visible",
            Stage::D3Visible => "D4Bidir",
            Stage::D4Bidir => "D5Freeze",
            Stage::D5Freeze => "Sealed",
            Stage::Sealed => "-",
        };
        format!("{:<12} {:<20} next={:<12} {}", self.name(), self.cn(), nxt, FACE_REGISTRY)
    }
}
/// 非法转换留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 序号。
    pub seq: u32,
    /// 起始阶段。
    pub from: Stage,
    /// 目标阶段。
    pub to: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 诊断码。
    pub code: u16,
}
impl Reject {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "reject#{} {}->{} t={} code=0x{:04X}",
            self.seq,
            self.from.name(),
            self.to.name(),
            self.tick,
            self.code
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 4] {
        [self.from.index() as u64, self.to.index() as u64, self.tick, self.code as u64]
    }
}
/// 主状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageFsm {
    /// 当前阶段。
    pub state: Stage,
    /// 逻辑时刻。
    pub tick: u64,
    /// 合法边推进数。
    pub legal_moves: u64,
    /// 拒记。
    pub rejected: Vec<Reject>,
    /// 足迹。
    pub trail: Vec<Stage>,
}
impl StageFsm {
    /// 初始态。
    pub fn new() -> StageFsm {
        let mut trail: Vec<Stage> = Vec::with_capacity(Stage::COUNT);
        trail.push(Stage::D1Declare);
        StageFsm { state: Stage::D1Declare, tick: 0, legal_moves: 0, rejected: Vec::new(), trail }
    }
    /// 合法边闭集。
    pub fn is_legal(from: Stage, to: Stage) -> bool {
        match (from, to) {
            (Stage::D1Declare, Stage::D2Merge) => true,
            (Stage::D2Merge, Stage::D3Visible) => true,
            (Stage::D3Visible, Stage::D4Bidir) => true,
            (Stage::D4Bidir, Stage::D5Freeze) => true,
            (Stage::D5Freeze, Stage::Sealed) => true,
            _ => false,
        }
    }
    /// 合法边总数。
    pub fn edge_count() -> usize {
        5
    }
    /// 非法候选边总数。
    pub fn illegal_edge_count() -> usize {
        Stage::COUNT * Stage::COUNT - StageFsm::edge_count()
    }
    /// 试走一条边。
    pub fn try_go(&mut self, to: Stage, tick: u64) -> MaResult<Stage> {
        self.tick = tick;
        if self.state.is_sealed() {
            self.rejected.push(Reject {
                seq: self.rejected.len() as u32,
                from: self.state,
                to,
                tick,
                code: Form::MissingInRegistry.code(),
            });
            return Err(MaErr::of_form(
                Form::MissingInRegistry,
                format!("终态不可迁移：{}->{}", self.state.name(), to.name()),
            ));
        }
        if StageFsm::is_legal(self.state, to) {
            self.state = to;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.trail.push(to);
            return Ok(to);
        }
        self.rejected.push(Reject {
            seq: self.rejected.len() as u32,
            from: self.state,
            to,
            tick,
            code: Form::GhostMount.code(),
        });
        Err(MaErr::of_form(
            Form::GhostMount,
            format!("非法转换：{}->{}", self.state.name(), to.name()),
        ))
    }
    /// 走满全部合法边。
    pub fn drive_all(&mut self, start_tick: u64) -> MaResult<()> {
        let mut t = start_tick;
        let mut i: usize = 1;
        while i < Stage::COUNT {
            t = t.wrapping_add(1);
            self.try_go(Stage::ALL[i], t)?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 足迹单调。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i - 1].index() >= self.trail[i].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.trail.len() == Stage::COUNT
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} legal={} reject={} trail={}",
            self.state.name(),
            self.legal_moves,
            self.rejected.len(),
            self.trail.len()
        )
    }
}

// ===========================================================================
// 9. append-only 环形账
// ===========================================================================

/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 样本下标。
    pub sample: usize,
    /// 轮次。
    pub round: u32,
    /// 差异分类下标。
    pub diff: u8,
    /// 执行指纹。
    pub fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ring s{} r{} diff={} fp={:016X} t={}",
            self.sample, self.round, self.diff, self.fp, self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [self.sample as u64, self.round as u64, self.diff as u64, self.fp, self.tick]
    }
}
/// append-only 环形账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 缓冲。
    pub buf: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 累计写入。
    pub pushed: u64,
    /// 累计丢弃（显性）。
    pub dropped: u64,
}
impl RingLedger {
    /// 空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry { sample: 0, round: 0, diff: 0xFF, fp: 0, tick: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写一条。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.head;
        if let Some(old) = self.buf.get(slot) {
            if old.diff != 0xFF || old.fp != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let mut ne = e;
        ne.tick = e.tick;
        if let Some(c) = self.buf.get_mut(slot) {
            *c = ne;
        }
        self.head = self.head.wrapping_add(1) & RING_MASK;
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.buf.len() {
            if self.buf[i].diff != 0xFF || self.buf[i].fp != 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 指纹（活账，不入冻结）。
    pub fn fingerprint(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(3 + RING_CAP * 5);
        w.push(V_VERSION_ID);
        w.push(self.pushed);
        w.push(self.dropped);
        let mut i: usize = 0;
        while i < self.buf.len() {
            let ww = self.buf[i].words();
            let mut j: usize = 0;
            while j < 5 {
                w.push(ww[j]);
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&w)
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} live={} dropped={} balanced={}",
            self.pushed,
            self.live(),
            self.dropped,
            if self.balanced() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.buf.len() {
            let e = self.buf[i];
            if e.diff != 0xFF || e.fp != 0 {
                out.push(format!("  {}", e.describe()));
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 10. 守恒账 reconcile()
// ===========================================================================

/// 守恒账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconcile {
    /// ΣA：域侧存活声明数。
    pub sum_a: u64,
    /// ΣB：注册表存活条数。
    pub sum_b: u64,
    /// ΣC：环形账写入数。
    pub sum_c: u64,
    /// ΣD：执行器可见数。
    pub sum_d: u64,
    /// ΣE：合并并入总数（含重复轮，故 ≥ 注册表条数）。
    pub sum_e: u64,
    /// 漏挂数（域有框架无）。
    pub missing: u64,
    /// 幽灵挂载数（框架有域无）。
    pub ghost: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 是否守恒。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Reconcile {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "recon A={} B={} C={} D={} E={} missing={} ghost={} dev={} balanced={} items={}",
            self.sum_a, self.sum_b, self.sum_c, self.sum_d, self.sum_e,
            self.missing, self.ghost, self.deviation,
            if self.balanced { 1 } else { 0 }, self.items.len()
        )
    }
    /// 逐行表。
    pub fn rows_out(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 2);
        out.push(self.summary());
        let mut i: usize = 0;
        while i < self.items.len() {
            let it = &self.items[i];
            out.push(format!("  {:<26} L={:<7} R={:<7} eq={}", it.0, it.1, it.2, if it.3 { 1 } else { 0 }));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 计算守恒账（双向登记制的核心恒等式都在这里）。
///
/// 两条最要紧的恒等式：
/// - **域声明数 == 注册表条数**：两侧规模一致（配合逐条指纹比对即得漏挂/幽灵为零）。
/// - **可见数 == 已挂载数**：`M-220` 的「注册表合并 → 执行器可见」链路完整。
pub fn reconcile(
    decl: &DomainDecl,
    reg: &Registry,
    view: &ExecView,
    mlog: &MergeLog,
    ring: &RingLedger,
    missing: u64,
    ghost: u64,
) -> Reconcile {
    let a: u64 = decl.live_n();
    let b: u64 = reg.live_n();
    let c: u64 = ring.pushed;
    let d: u64 = view.visible_n;
    let e: u64 = mlog.merged_total;
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(7);
    items.push((String::from("域声明=注册表"), a, b, a == b));
    items.push((String::from("注册表=环形写入"), b, c, b == c));
    items.push((String::from("可见=已挂载"), d, reg.mounted_n(), d == reg.mounted_n()));
    items.push((String::from("并入≥注册表(幂等)"), if e >= b { 1 } else { 0 }, 1, e >= b));
    items.push((String::from("无漏挂(布尔)"), missing, 0, missing == 0));
    items.push((String::from("无幽灵挂载(布尔)"), ghost, 0, ghost == 0));
    items.push((String::from("可见无越权(布尔)"), view.leak_n, 0, view.leak_n == 0));
    let mut dev: u64 = 0;
    let mut i: usize = 0;
    while i < items.len() {
        let it = &items[i];
        let l = if it.1 > it.2 { it.1 - it.2 } else { it.2 - it.1 };
        if l > dev {
            dev = l;
        }
        i = i.wrapping_add(1);
    }
    let balanced: bool = dev == 0 && items.iter().all(|it| it.3);
    Reconcile {
        sum_a: a,
        sum_b: b,
        sum_c: c,
        sum_d: d,
        sum_e: e,
        missing,
        ghost,
        deviation: dev,
        balanced,
        items,
    }
}

// ===========================================================================
// 11. 快照 + 注入核
// ===========================================================================

/// 双向对账结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BidirReport {
    /// 漏挂清单（域有框架无的用例指纹）。
    pub missing: Vec<u64>,
    /// 幽灵清单（框架有域无的用例指纹）。
    pub ghost: Vec<u64>,
}
impl BidirReport {
    /// 空报告（净态应两侧皆空）。
    pub fn empty() -> BidirReport {
        BidirReport { missing: Vec::new(), ghost: Vec::new() }
    }
    /// 双向对账（**两侧都查**：域→框架查漏挂，框架→域查幽灵）。
    pub fn scan(decl: &DomainDecl, reg: &Registry) -> BidirReport {
        let mut missing: Vec<u64> = Vec::new();
        let mut ghost: Vec<u64> = Vec::new();
        let mut i: usize = 0;
        // 方向一：域侧有、框架侧无 = 漏挂
        while i < decl.items.len() {
            let it = &decl.items[i];
            if it.live == 1 && reg.find_by_fp(it.fp()).is_none() {
                missing.push(it.fp());
            }
            i = i.wrapping_add(1);
        }
        // 方向二：框架侧有、域侧无 = 幽灵挂载
        let mut j: usize = 0;
        while j < reg.items.len() {
            let it = &reg.items[j];
            if it.live == 1 && decl.find_by_fp(it.fp).is_none() {
                ghost.push(it.fp);
            }
            j = j.wrapping_add(1);
        }
        BidirReport { missing, ghost }
    }
    /// 漏挂数。
    pub fn missing_n(&self) -> u64 {
        self.missing.len() as u64
    }
    /// 幽灵数。
    pub fn ghost_n(&self) -> u64 {
        self.ghost.len() as u64
    }
    /// 两侧皆空。
    pub fn clean(&self) -> bool {
        self.missing.is_empty() && self.ghost.is_empty()
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("bidir missing={} ghost={} clean={}", self.missing_n(), self.ghost_n(), if self.clean() { 1 } else { 0 })
    }
}
/// 注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入触发次数。
    pub fired: [u32; Form::COUNT],
    /// 域册指纹。
    pub decl_fp: u64,
    /// 注册表指纹。
    pub reg_fp: u64,
    /// 合并流水指纹。
    pub merge_fp: u64,
    /// 可见视图指纹。
    pub view_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 冻结指纹（净态入册值）。
    pub freeze_fp: u64,
    /// 入册探针基准值。
    pub registered_probe: u64,
    /// 语义边界违例（真实挂载表写入次数，恒 0）。
    pub border_violations: u32,
}
impl Evidence {
    /// 空现场。
    pub fn blank() -> Evidence {
        let mut fired: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            fired[i] = 0;
            i = i.wrapping_add(1);
        }
        Evidence {
            fired,
            decl_fp: 0,
            reg_fp: 0,
            merge_fp: 0,
            view_fp: 0,
            ring_fp: 0,
            freeze_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "ev decl={:016X} reg={:016X} merge={:016X} view={:016X} ring={:016X} probe={:016X} border={}",
            self.decl_fp, self.reg_fp, self.merge_fp, self.view_fp, self.ring_fp,
            self.registered_probe, self.border_violations
        )
    }
}
/// 注册表定义哈希（只折版本 + 规模 + 合并规则）。
pub fn registry_definition_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(6);
    w.push(V_VERSION_ID);
    w.push(DOMAIN_COUNT as u64);
    w.push(CASES_PER_DOMAIN as u64);
    w.push(CASE_TOTAL as u64);
    w.push(REG_CAP as u64);
    w.push(MERGE_ROUNDS as u64);
    let mut i: usize = 0;
    while i < DOMAIN_COUNT {
        w.push(fnv1a64(DOMAIN_NAMES[i].as_bytes()));
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 冻结探针基准哈希（**只折冻结后永不变的字段**：注册表定义 + 条目与挂载状态）。
pub fn freeze_probe_hash(decl: &DomainDecl, reg: &Registry) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(5);
    w.push(V_VERSION_ID);
    w.push(registry_definition_hash());
    w.push(reg.fingerprint());
    w.push(decl.fingerprint());
    w.push(mount_fsm_hash());
    fnv1a_words(&w)
}
/// 状态机定义哈希。
pub fn mount_fsm_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(2 + Stage::COUNT * 4);
    w.push(V_VERSION_ID);
    w.push(Stage::COUNT as u64);
    let mut i: usize = 0;
    while i < Stage::COUNT {
        let st = Stage::ALL[i];
        w.push(fnv1a64(st.name().as_bytes()));
        w.push(st.cn().len() as u64);
        w.push(st.index() as u64);
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < Stage::COUNT {
        let mut k: usize = 0;
        while k < Stage::COUNT {
            if StageFsm::is_legal(Stage::ALL[j], Stage::ALL[k]) {
                w.push(((j * Stage::COUNT + k) as u64).wrapping_add(0x50000));
            }
            k = k.wrapping_add(1);
        }
        j = j.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 可见视图指纹（活账，**不入冻结**）。
pub fn view_fingerprint(v: &ExecView) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(4 + v.visible.len());
    w.push(V_VERSION_ID);
    w.push(v.visible.len() as u64);
    w.push(v.leak_n);
    w.push(v.blind_n);
    let mut i: usize = 0;
    while i < v.visible.len() {
        w.push(v.visible[i]);
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 合并流水指纹。
pub fn merge_fingerprint(m: &MergeLog) -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(3 + m.entries.len() * 5);
    w.push(V_VERSION_ID);
    w.push(m.entries.len() as u64);
    w.push(m.merged_total);
    let mut i: usize = 0;
    while i < m.entries.len() {
        let ww = m.entries[i].words();
        let mut j: usize = 0;
        while j < 5 {
            w.push(ww[j]);
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}
/// 快照。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 域侧声明册。
    pub decl: DomainDecl,
    /// 框架侧注册表。
    pub reg: Registry,
    /// 合并流水。
    pub mlog: MergeLog,
    /// 执行器可见视图。
    pub view: ExecView,
    /// 双向对账报告。
    pub bidir: BidirReport,
    /// 环形账。
    pub ring: RingLedger,
    /// 留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照（M-220 全链路：域声明 → 注册表合并 → 执行器可见）。
    pub fn net() -> MaResult<Snapshot> {
        let decl = DomainDecl::net()?;
        let mut reg = Registry::new(1);
        let mut mlog = MergeLog::new();
        let mut ring = RingLedger::new();
        // 两轮合并：第一轮建表，第二轮验证幂等
        let mut round: u32 = 0;
        while round < MERGE_ROUNDS as u32 {
            let mut d: usize = 0;
            while d < DOMAIN_COUNT {
                mlog.merge_domain(&mut reg, &decl, d, round)?;
                d = d.wrapping_add(1);
            }
            round = round.wrapping_add(1);
        }
        // 环形账：每条注册表条目写一条
        let mut i: usize = 0;
        while i < reg.items.len() {
            let it = &reg.items[i];
            let mut w: Vec<u64> = Vec::with_capacity(3);
            w.push(it.fp);
            w.push(it.slot as u64);
            w.push(it.state.index() as u64);
            ring.push(RingEntry {
                sample: it.slot,
                round: 0,
                diff: it.state.index() as u8,
                fp: fnv1a_words(&w),
                tick: i as u64,
            });
            i = i.wrapping_add(1);
        }
        let view = ExecView::build(&reg);
        let bidir = BidirReport::scan(&decl, &reg);
        let mut ev = Evidence::blank();
        ev.decl_fp = decl.fingerprint();
        ev.reg_fp = reg.fingerprint();
        ev.merge_fp = merge_fingerprint(&mlog);
        ev.view_fp = view_fingerprint(&view);
        ev.ring_fp = ring.fingerprint();
        ev.freeze_fp = freeze_probe_hash(&decl, &reg);
        ev.registered_probe = ev.freeze_fp;
        ev.border_violations = 0;
        Ok(Snapshot { decl, reg, mlog, view, bidir, ring, ev })
    }
    /// 守恒账。
    pub fn reconcile(&self) -> Reconcile {
        reconcile(
            &self.decl,
            &self.reg,
            &self.view,
            &self.mlog,
            &self.ring,
            self.bidir.missing_n(),
            self.bidir.ghost_n(),
        )
    }
    /// 是否净态。
    pub fn is_net(&self) -> bool {
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.ev.fired[i] != 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!("snapshot net={} {}", if self.is_net() { 1 } else { 0 }, self.ev.describe())
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(9);
        out.push(self.describe());
        out.push(self.decl.summary());
        out.push(self.reg.summary());
        out.push(self.mlog.summary());
        out.push(self.view.summary());
        out.push(self.bidir.summary());
        out.push(self.ring.summary());
        out.push(self.reconcile().summary());
        out
    }
}
/// 单形态注入。
fn inject_one(f: Form) -> MaResult<Snapshot> {
    let mut s = Snapshot::net()?;
    s.ev.border_violations = 0;
    let idx = f.index();
    if idx < Form::COUNT {
        s.ev.fired[idx] = 1;
    }
    match f {
        Form::MissingInRegistry => {
            // 漏挂：域侧某条声明在框架注册表中查无此条
            if let Some(it) = s.decl.items.get(0) {
                let fp = it.fp();
                let mut i: usize = 0;
                while i < s.reg.items.len() {
                    if s.reg.items[i].fp == fp {
                        if let Some(r) = s.reg.items.get_mut(i) {
                            r.live = 0;
                            r.state = MountState::Unmounted;
                        }
                    }
                    i = i.wrapping_add(1);
                }
                s.reg.cover_bits = s.reg.cover_bits & (s.reg.cover_bits ^ 1u32);
            }
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::GhostMount => {
            // 幽灵挂载：注册表塞一条域侧根本没有的用例
            let ghost_fp: u64 = 0xDEAD_C0DE_DEAD_C0DEu64;
            let _ = s.reg.merge_one(ghost_fp, String::from("d9:c9"), MountState::Mounted);
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::DeclDup => {
            // 域声明重号
            s.decl.dup_bits = s.decl.dup_bits | 1u32;
        }
        Form::DeclMissing => {
            // 域声明缺项：摘掉一条并清存在位
            if let Some(_last) = s.decl.items.pop() {
                let b: u32 = 1u32 << (s.decl.items.len() & 31);
                s.decl.cover_bits = s.decl.cover_bits & (s.decl.cover_bits ^ b);
            }
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::RegDup => {
            // 注册表重号：直接 push 一条重复指纹（绕过幂等入口）
            if let Some(first) = s.reg.items.get(0) {
                let fp = first.fp;
                let key = first.key.clone();
                let slot = s.reg.items.len();
                s.reg.items.push(RegItem::new(slot, fp, key, MountState::Mounted));
                s.reg.dup_bits = s.reg.dup_bits | (1u32 << (slot & 31));
            }
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::RegMissing => {
            // 注册表缺项：整表清空
            s.reg.items.clear();
            s.reg.cover_bits = 0;
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
            s.view = ExecView::build(&s.reg);
        }
        Form::MergeNotIdempotent => {
            // 合并不幂等：第二轮反而新增了条目
            if let Some(e) = s.mlog.entries.last_mut() {
                e.idempotent = 0;
            }
            if let Some(la) = s.reg.items.get(1) {
                let fp = la.fp;
                let key = la.key.clone();
                let _ = s.reg.merge_one(fp.wrapping_add(1), key, MountState::Mounted);
                if let Some(e) = s.mlog.entries.last_mut() {
                    e.idempotent = 0;
                }
            }
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::MergeOrderUnstable => {
            // 合并序不确定：把一条流水记录的域号改到后面，制造同输入异序
            if !s.mlog.entries.is_empty() {
                let n: usize = s.mlog.entries.len();
                let e = &mut s.mlog.entries[n - 1];
                e.domain = DOMAIN_COUNT; // 越界域号 = 合并序被打乱
            }
        }
        Form::MountStateIllegal => {
            // 挂载状态越闭集
            if let Some(r) = s.reg.items.get_mut(0) {
                r.state = MountState::Unmounted;
                // 用一个闭集外的秩表达「越态」：直接把 index 顶到闭集外并以旁路标志记录
                r.live = 2; // live 非法值承载越态事实
            }
            s.view = ExecView::build(&s.reg);
        }
        Form::ExecSeesUnmounted => {
            // 执行器看见未挂载用例：把 Shadow 条目塞进可见视图
            let mut i: usize = 0;
            while i < s.reg.items.len() {
                if s.reg.items[i].state == MountState::Shadow {
                    s.view.visible.push(s.reg.items[i].fp);
                    s.view.visible_n = s.view.visible_n.wrapping_add(1);
                    s.view.leak_n = s.view.leak_n.wrapping_add(1);
                    i = s.reg.items.len();
                } else {
                    i = i.wrapping_add(1);
                }
            }
        }
        Form::ExecBlindToMounted => {
            // 执行器看不见已挂载用例：从可见视图里抹掉一条 Mounted
            let mut i: usize = 0;
            while i < s.reg.items.len() {
                if s.reg.items[i].state == MountState::Mounted && !s.view.visible.is_empty() {
                    let target = s.reg.items[i].fp;
                    let mut j: usize = 0;
                    while j < s.view.visible.len() {
                        if s.view.visible[j] == target {
                            s.view.visible.remove(j);
                            s.view.visible_n = s.view.visible_n.wrapping_sub(1);
                            break;
                        }
                        j = j.wrapping_add(1);
                    }
                    i = s.reg.items.len();
                } else {
                    i = i.wrapping_add(1);
                }
            }
            s.view.blind_n = ExecView::recompute_blind(&s.reg);
        }
        Form::LedgerUnbalanced => {
            // 环形账不守恒
            s.ring.head = s.ring.head.wrapping_sub(1) & RING_MASK;
            s.ring.dropped = s.ring.dropped.wrapping_add(4);
        }
        Form::ReconcileDeviation => {
            // 守恒偏离：域侧多一条声明
            let dn: String = String::from(DOMAIN_NAMES[0]);
            s.decl.items.push(DeclItem::new(0, CASES_PER_DOMAIN, dn, MountState::Mounted));
            s.bidir = BidirReport::scan(&s.decl, &s.reg);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改注册表条目状态而不更新入册探针
            if let Some(r) = s.reg.items.get_mut(0) {
                r.state = MountState::Shadow;
            }
            s.ev.freeze_fp = freeze_probe_hash(&s.decl, &s.reg);
            s.view = ExecView::build(&s.reg);
        }
    }
    Ok(s)
}
/// 逐形态注入全跑。
pub fn inject_all() -> MaResult<(Vec<Snapshot>, Vec<Form>)> {
    let mut out: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    let mut forms: Vec<Form> = Vec::with_capacity(Form::COUNT);
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        out.push(inject_one(f)?);
        forms.push(f);
        i = i.wrapping_add(1);
    }
    Ok((out, forms))
}
/// 检出表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中数。
    pub hits: [u32; Form::COUNT],
    /// 判定掩码。
    pub verdict_mask: u32,
    /// 语义边界违例。
    pub border_violations: u32,
}
impl Detections {
    /// 空表。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0, border_violations: 0 }
    }
    /// 记命中。
    pub fn mark(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            self.verdict_mask = self.verdict_mask | (1u32 << (i & 31));
        }
    }
    /// 是否命中。
    pub fn hit(&self, f: Form) -> bool {
        let i = f.index();
        if i >= Form::COUNT {
            return false;
        }
        self.hits[i] > 0
    }
    /// 命中数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.hits[i] > 0 {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
    /// 未命中名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if !self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 命中名表。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            if self.hit(f) {
                out.push(f.name());
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "detect hit={}/{} mask={:08X} border={}",
            self.hit_count(),
            Form::COUNT,
            self.verdict_mask,
            self.border_violations
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 hit={}/{}", self.hit_count(), Form::COUNT));
        let mut i: usize = 0;
        while i < Form::COUNT {
            let f = Form::ALL[i];
            out.push(format!(
                "{:<20} code=0x{:04X} {:<12} {:<8} hits={} [{}] {}",
                f.name(), f.code(), f.class_name(), f.face(), self.hits[i],
                f.probe(), f.describe()
            ));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 视图越权数：注册表里**非 Mounted** 的指纹却出现在快照视图中。
pub fn view_leak_vs_reg(s: &Snapshot) -> u64 {
    let mut leak: u64 = 0;
    let mut i: usize = 0;
    while i < s.reg.items.len() {
        let it = &s.reg.items[i];
        if it.live == 1 && !it.state.exec_visible() {
            let mut j: usize = 0;
            while j < s.view.visible.len() {
                if s.view.visible[j] == it.fp {
                    leak = leak.wrapping_add(1);
                }
                j = j.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    leak
}
/// 视图盲区数：注册表里**已 Mounted** 的指纹却不在快照视图中。
pub fn view_blind_vs_reg(s: &Snapshot) -> u64 {
    let mut blind: u64 = 0;
    let mut i: usize = 0;
    while i < s.reg.items.len() {
        let it = &s.reg.items[i];
        if it.live == 1 && it.state == MountState::Mounted {
            let mut found: bool = false;
            let mut j: usize = 0;
            while j < s.view.visible.len() {
                if s.view.visible[j] == it.fp {
                    found = true;
                }
                j = j.wrapping_add(1);
            }
            if !found {
                blind = blind.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    blind
}
/// 检测端（只读入参，与注入端数据来源物理隔离）。
pub fn detect_one(s: &Snapshot) -> Detections {
    let mut d = Detections::blank();
    d.border_violations = d.border_violations.wrapping_add(s.ev.border_violations);
    // ① 双向登记对账（重扫一遍，不信快照里的报告）
    let bidir = BidirReport::scan(&s.decl, &s.reg);
    if bidir.missing_n() > 0 {
        d.mark(Form::MissingInRegistry);
    }
    if bidir.ghost_n() > 0 {
        d.mark(Form::GhostMount);
    }
    // ② 域侧 / 注册表面
    if !s.decl.dup_free() {
        d.mark(Form::DeclDup);
    }
    if !s.decl.cover_complete() {
        d.mark(Form::DeclMissing);
    }
    if !s.reg.dup_free() {
        d.mark(Form::RegDup);
    }
    if !s.reg.cover_complete(CASE_TOTAL) {
        d.mark(Form::RegMissing);
    }
    // ③ 合并面
    if !s.mlog.all_idempotent() {
        d.mark(Form::MergeNotIdempotent);
    }
    // 合并序确定性：流水里的域号必须全在闭集内
    let mut order_bad: bool = false;
    let mut i: usize = 0;
    while i < s.mlog.entries.len() {
        if s.mlog.entries[i].domain >= DOMAIN_COUNT {
            order_bad = true;
        }
        i = i.wrapping_add(1);
    }
    if order_bad {
        d.mark(Form::MergeOrderUnstable);
    }
    // ④ 挂载状态闭集
    let mut state_bad: bool = !s.reg.state_closed();
    let mut j: usize = 0;
    while j < s.reg.items.len() {
        if s.reg.items[j].live > 1 {
            state_bad = true;
        }
        j = j.wrapping_add(1);
    }
    if state_bad {
        d.mark(Form::MountStateIllegal);
    }
    // ⑤ 可见性面：**拿快照里那份真实视图去核对注册表**，而不是从注册表重建视图——
    // 从注册表重建等于用「正确答案」去核对自己，注入篡改的视图会被自动洗白。
    if view_leak_vs_reg(s) > 0 {
        d.mark(Form::ExecSeesUnmounted);
    }
    if view_blind_vs_reg(s) > 0 {
        d.mark(Form::ExecBlindToMounted);
    }
    // ⑥ 账目与冻结
    if !s.ring.balanced() {
        d.mark(Form::LedgerUnbalanced);
    }
    let rc = s.reconcile();
    if !rc.balanced {
        d.mark(Form::ReconcileDeviation);
    }
    if s.ev.freeze_fp != s.ev.registered_probe {
        d.mark(Form::FingerprintDrift);
    }
    d
}
/// 全形态检出。
pub fn detect_all(inj: &[Snapshot], forms: &[Form]) -> MaResult<Detections> {
    let mut agg = Detections::blank();
    let mut i: usize = 0;
    while i < inj.len() {
        let s = match inj.get(i) {
            Some(x) => x,
            None => return Err(err_range("detect_all", i, inj.len())),
        };
        let d = detect_one(s);
        let f = match forms.get(i) {
            Some(x) => *x,
            None => return Err(err_range("detect_all form", i, forms.len())),
        };
        if !d.hit(f) {
            return Err(MaErr::of_form(f, format!("注入未被检出：{} 漏检", f.name())));
        }
        let mut j: usize = 0;
        while j < Form::COUNT {
            agg.hits[j] = agg.hits[j].wrapping_add(d.hits[j]);
            j = j.wrapping_add(1);
        }
        agg.verdict_mask = agg.verdict_mask | d.verdict_mask;
        agg.border_violations = agg.border_violations.wrapping_add(d.border_violations);
        i = i.wrapping_add(1);
    }
    Ok(agg)
}

// ===========================================================================
// 12. 冻结件 / 稳定性 / 重放
// ===========================================================================

/// 冻结记录（注册表版本化 + 冻结哈希）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 冻结件版本。
    pub rev: u32,
    /// 注册表版本。
    pub reg_rev: u32,
    /// 注册表定义哈希。
    pub regdef_hash: u64,
    /// 注册表条目哈希（只含定义与挂载状态）。
    pub reg_hash: u64,
    /// 域册哈希。
    pub decl_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 架构键。
    pub arch_key: String,
}
impl FreezeRecord {
    /// 构造。
    pub fn build(decl: &DomainDecl, reg: &Registry) -> FreezeRecord {
        let regdef_hash = registry_definition_hash();
        let reg_hash = reg.fingerprint();
        let decl_hash = decl.fingerprint();
        let mut w: Vec<u64> = Vec::with_capacity(7);
        w.push(V_VERSION_ID);
        w.push(regdef_hash);
        w.push(reg_hash);
        w.push(decl_hash);
        w.push(reg.rev as u64);
        w.push(DOMAIN_COUNT as u64);
        w.push(CASE_TOTAL as u64);
        let freeze_hash = fnv1a_words(&w);
        FreezeRecord {
            rev: 1,
            reg_rev: reg.rev,
            regdef_hash,
            reg_hash,
            decl_hash,
            freeze_hash,
            border_note: String::from("只做挂载架构与账面全链路；不真改任何域的挂载表、不真执行用例"),
            arch_key: format!("mountarch/{}/{}", V_VERSION, GRAFT_SEAT),
        }
    }
    /// 校验（重算定义件哈希并比对）。
    pub fn verify(&self, decl: &DomainDecl, reg: &Registry) -> MaResult<()> {
        if self.regdef_hash != registry_definition_hash() {
            return Err(err_frozen("regdef_hash", &format!("{:016X}", registry_definition_hash())));
        }
        if self.reg_hash != reg.fingerprint() {
            return Err(err_frozen("reg_hash", &format!("{:016X}", reg.fingerprint())));
        }
        if self.decl_hash != decl.fingerprint() {
            return Err(err_frozen("decl_hash", &format!("{:016X}", decl.fingerprint())));
        }
        if self.reg_rev != reg.rev {
            return Err(err_frozen("reg_rev", &format!("{}", reg.rev)));
        }
        let bidir = BidirReport::scan(decl, reg);
        if !bidir.clean() {
            return Err(MaErr::of_form(
                Form::GhostMount,
                format!("冻结校验：双向对账不净 {}", bidir.summary()),
            ));
        }
        Ok(())
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} reg_rev={} regdef={:016X} reg={:016X} decl={:016X} total={:016X}",
            self.rev, self.reg_rev, self.regdef_hash, self.reg_hash, self.decl_hash, self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(4);
        out.push(self.describe());
        out.push(format!("  border  {}", self.border_note));
        out.push(format!("  key     {}", self.arch_key));
        out.push(format!("  dep     {}", DEP_SEAT));
        out
    }
}
/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: u32,
    /// 逐轮冻结哈希。
    pub hashes: Vec<u64>,
    /// 逐轮注册表指纹。
    pub reg_fps: Vec<u64>,
    /// 是否一致。
    pub stable: bool,
}
impl Stability {
    /// 一致判定。
    pub fn stable(&self) -> bool {
        if self.hashes.len() < 2 {
            return false;
        }
        let h0 = self.hashes[0];
        let r0 = self.reg_fps[0];
        let mut i: usize = 1;
        while i < self.hashes.len() {
            if self.hashes[i] != h0 || self.reg_fps[i] != r0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!("stable rounds={} stable={}", self.rounds, if self.stable { 1 } else { 0 })
    }
}
/// 稳定性轮数。
pub const REPEAT_RUNS: usize = 3;
/// 稳定性跑。
pub fn stability_check() -> MaResult<Stability> {
    let mut hashes: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut reg_fps: Vec<u64> = Vec::with_capacity(REPEAT_RUNS);
    let mut i: usize = 0;
    while i < REPEAT_RUNS {
        let s = Snapshot::net()?;
        let fr = FreezeRecord::build(&s.decl, &s.reg);
        hashes.push(fr.freeze_hash);
        reg_fps.push(s.reg.fingerprint());
        i = i.wrapping_add(1);
    }
    let mut st = Stability { rounds: REPEAT_RUNS as u32, hashes, reg_fps, stable: false };
    st.stable = st.stable();
    if !st.stable {
        return Err(err_frozen("stability", "all-round-equal"));
    }
    Ok(st)
}
/// 重放报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 注册表指纹。
    pub reg_fp: u64,
    /// 域册指纹。
    pub decl_fp: u64,
    /// 可见视图指纹。
    pub view_fp: u64,
    /// 注册表条数。
    pub reg_n: u64,
    /// 可见条数。
    pub visible_n: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 环形账写入数。
    pub ring_pushed: u64,
}
impl Replay {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay freeze={:016X} reg={:016X} decl={:016X} view={:016X} reg_n={} visible={} hits={} ring={}",
            self.freeze_hash, self.reg_fp, self.decl_fp, self.view_fp,
            self.reg_n, self.visible_n, self.inject_hits, self.ring_pushed
        )
    }
}
/// 确定性重放（与 `observe` 互不递归）。
pub fn replay(seed: u64) -> MaResult<Replay> {
    let mut rng = Lcg::new(seed);
    let _ = rng.next();
    let s = Snapshot::net()?;
    if !s.decl.cover_complete() || !s.decl.dup_free() {
        return Err(MaErr::of_form(
            Form::DeclMissing,
            format!("重放：域册不过 {}", s.decl.summary()),
        ));
    }
    if !s.reg.cover_complete(CASE_TOTAL) || !s.reg.dup_free() {
        return Err(MaErr::of_form(
            Form::RegMissing,
            format!("重放：注册表不过 {}", s.reg.summary()),
        ));
    }
    if !s.mlog.all_idempotent() {
        return Err(MaErr::of_form(
            Form::MergeNotIdempotent,
            format!("重放：合并不幂等 {}", s.mlog.summary()),
        ));
    }
    if !s.bidir.clean() {
        return Err(MaErr::of_form(
            Form::MissingInRegistry,
            format!("重放：双向对账不净 {}", s.bidir.summary()),
        ));
    }
    if ExecView::recompute_blind(&s.reg) > 0 {
        return Err(MaErr::of_form(
            Form::ExecBlindToMounted,
            format!("重放：执行器有盲区 {}", s.view.summary()),
        ));
    }
    if !s.ring.balanced() {
        return Err(MaErr::of_form(
            Form::LedgerUnbalanced,
            format!("重放：环形账不守恒 {}", s.ring.summary()),
        ));
    }
    let rc = s.reconcile();
    if !rc.balanced {
        return Err(err_recon(rc.sum_a, rc.sum_b, rc.sum_c));
    }
    let (inj, forms) = inject_all()?;
    let agg = detect_all(&inj, &forms)?;
    let fr = FreezeRecord::build(&s.decl, &s.reg);
    Ok(Replay {
        freeze_hash: fr.freeze_hash,
        reg_fp: s.reg.fingerprint(),
        decl_fp: s.decl.fingerprint(),
        view_fp: view_fingerprint(&s.view),
        reg_n: s.reg.live_n(),
        visible_n: s.view.visible_n,
        inject_hits: agg.hit_count(),
        ring_pushed: s.ring.pushed,
    })
}

// ===========================================================================
// 13. 指标与性能分解
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次操作数。
    pub per_iter: u64,
    /// 合计。
    pub total: u64,
    /// 对照说明（「深在哪」）。
    pub note: &'static str,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note: "—" }
    }
    /// 带对照构造。
    pub fn with_note(name: &'static str, iters: u64, per_iter: u64, note: &'static str) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note }
    }
    /// 描述。
    pub fn describe(&self) -> String {
        format!(
            "{:<26} iters={:<6} per_iter={:<4} total={:<9} note={}",
            self.name, self.iters, self.per_iter, self.total, self.note
        )
    }
}
/// 指标册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 域数。
    pub domains: u64,
    /// 每域用例数。
    pub cases_per_domain: u64,
    /// 用例总数。
    pub cases: u64,
    /// 注册表条数。
    pub reg_n: u64,
    /// 已挂载条数。
    pub mounted_n: u64,
    /// 影子挂载条数。
    pub shadow_n: u64,
    /// 执行器可见数。
    pub visible_n: u64,
    /// 漏挂数（应 0）。
    pub missing_n: u64,
    /// 幽灵挂载数（应 0）。
    pub ghost_n: u64,
    /// 合并并入总数。
    pub merged_total: u64,
    /// 幂等轮数。
    pub idem_rounds: u64,
    /// 可见视图盲区（应 0）。
    pub blind_n: u64,
    /// 环形账写入/丢弃。
    pub ring_pushed: u64,
    /// 环形账丢弃。
    pub ring_dropped: u64,
    /// 合并步数。
    pub steps: u64,
    /// 注入检出命中数。
    pub inject_hits: usize,
    /// 净态检出（应零）。
    pub net_hits: usize,
    /// 边界违例（应零）。
    pub border_violations: u32,
    /// 峰值内存估算。
    pub mem_peak: u64,
}
impl Metrics {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.net_hits == 0
            && self.border_violations == 0
            && self.domains == DOMAIN_COUNT as u64
            && self.cases == CASE_TOTAL as u64
            && self.reg_n == CASE_TOTAL as u64
            && self.missing_n == 0
            && self.ghost_n == 0
            && self.blind_n == 0
            && self.visible_n == self.mounted_n
            && self.idem_rounds > 0
            && self.merged_total >= self.reg_n
            && self.inject_hits == Form::COUNT
            && self.steps <= STEP_CAP
            && self.mem_peak <= MEM_PEAK_CAP
    }
    /// 性能逐项分解。
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "domain_declare",
                (DOMAIN_COUNT * CASES_PER_DOMAIN) as u64,
                4,
                "Windows 测试按工程/目录分，无「域侧声明用例集」层",
            ),
            CostItem::with_note(
                "registry_merge",
                (DOMAIN_COUNT * MERGE_ROUNDS) as u64,
                CASES_PER_DOMAIN as u64,
                "Windows 无框架侧统一注册表，域各自注册",
            ),
            CostItem::with_note(
                "merge_idempotent_probe",
                MERGE_ROUNDS as u64,
                CASE_TOTAL as u64,
                "本单特有：合并幂等性强制验证（第二轮零新增）",
            ),
            CostItem::with_note(
                "bidir_scan_missing",
                CASE_TOTAL as u64,
                self.reg_n,
                "本单特有：域→框架查漏挂（O(N×M)）",
            ),
            CostItem::with_note(
                "bidir_scan_ghost",
                self.reg_n,
                CASE_TOTAL as u64,
                "本单特有：框架→域查幽灵挂载",
            ),
            CostItem::with_note(
                "exec_view_build",
                self.reg_n,
                3,
                "本单特有：可见性只认 Mounted 的硬口径",
            ),
            CostItem::with_note(
                "exec_blind_recompute",
                self.reg_n,
                self.visible_n,
                "本单特有：盲区重算（不复信任快照读数）",
            ),
            CostItem::with_note(
                "registry_freeze_hash",
                1,
                self.reg_n as u64 * 4,
                "本单特有：版本化冻结哈希，隔离访问计数活账",
            ),
            CostItem::with_note(
                "ring_push",
                self.ring_pushed,
                3,
                "append-only + dropped 显性",
            ),
            CostItem::with_note(
                "reconcile",
                7,
                3,
                "双向登记制的跨侧恒等式",
            ),
            CostItem::with_note(
                "inject_one",
                Form::COUNT as u64,
                CASE_TOTAL as u64,
                "本单特有：14 形态全注入",
            ),
            CostItem::with_note(
                "detect_one",
                Form::COUNT as u64,
                (CASE_TOTAL * 2) as u64,
                "本单特有：14 检测器逐面扫描",
            ),
        ]
    }
    /// 合计。
    pub fn complexity_total(&self) -> u64 {
        let items = self.complexity();
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < items.len() {
            s = s.wrapping_add(items[i].total);
            i = i.wrapping_add(1);
        }
        s
    }
    /// 逐行表。
    pub fn complexity_rows(&self) -> Vec<String> {
        let items = self.complexity();
        let mut out: Vec<String> = Vec::with_capacity(items.len() + 2);
        let mut i: usize = 0;
        while i < items.len() {
            out.push(items[i].describe());
            i = i.wrapping_add(1);
        }
        out.push(format!("TOTAL iters*per_iter = {}", self.complexity_total()));
        out.push(format!(
            "STEP_CAP={} SYSCALL_CAP={} MEM_PEAK_CAP={}",
            STEP_CAP, SYSCALL_CAP, MEM_PEAK_CAP
        ));
        out
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics domains={} cases={} reg={} mounted={} shadow={} visible={} missing={} ghost={} blind={} merged={} idem={} ring={}/{} inj={} net={}",
            self.domains, self.cases, self.reg_n, self.mounted_n, self.shadow_n,
            self.visible_n, self.missing_n, self.ghost_n, self.blind_n,
            self.merged_total, self.idem_rounds, self.ring_pushed, self.ring_dropped,
            self.inject_hits, self.net_hits
        )
    }
}

// ===========================================================================
// 14. 导出
// ===========================================================================

/// KV 导出。
pub fn export_kv(o: &MaObserve) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(48);
    out.push(format!("ver={}", V_VERSION));
    out.push(format!("seed={:016X}", o.seed));
    out.push(format!("seg=0x{:04X}", SEG_BASE));
    out.push(format!("domains={}", DOMAIN_COUNT));
    out.push(format!("cases_per_domain={}", CASES_PER_DOMAIN));
    out.push(format!("case_total={}", CASE_TOTAL));
    out.push(format!("reg_rev={}", o.net.reg.rev));
    out.push(format!("domains_live={}", o.metrics.domains));
    out.push(format!("cases={}", o.metrics.cases));
    out.push(format!("reg_n={}", o.metrics.reg_n));
    out.push(format!("mounted={}", o.metrics.mounted_n));
    out.push(format!("shadow={}", o.metrics.shadow_n));
    out.push(format!("visible={}", o.metrics.visible_n));
    out.push(format!("missing={}", o.metrics.missing_n));
    out.push(format!("ghost={}", o.metrics.ghost_n));
    out.push(format!("blind={}", o.metrics.blind_n));
    out.push(format!("merged_total={}", o.metrics.merged_total));
    out.push(format!("idem_rounds={}", o.metrics.idem_rounds));
    out.push(format!("ring_pushed={}", o.metrics.ring_pushed));
    out.push(format!("ring_dropped={}", o.metrics.ring_dropped));
    out.push(format!("steps={}", o.metrics.steps));
    out.push(format!("recon_dev={}", o.recon.deviation));
    out.push(format!("recon_balanced={}", if o.recon.balanced { 1 } else { 0 }));
    out.push(format!("fsm_state={}", o.fsm.state.name()));
    out.push(format!("fsm_reject={}", o.fsm.rejected.len()));
    out.push(format!("freeze_hash={:016X}", o.freeze.freeze_hash));
    out.push(format!("replay_freeze={:016X}", o.replay.freeze_hash));
    out.push(format!("inject_hits={}", o.metrics.inject_hits));
    out.push(format!("net_hits={}", o.metrics.net_hits));
    out.push(format!("all_ok={}", if o.all_ok() { 1 } else { 0 }));
    let mut i: usize = 0;
    while i < Form::COUNT {
        out.push(format!("form_{}={}", Form::ALL[i].name(), o.detects.hits[i]));
        i = i.wrapping_add(1);
    }
    out
}
/// 推 `u64` 大端。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}
/// 推一条 TLV。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出（组头单独一行；长度字段恒占定宽）。
pub fn export_tlv(o: &MaObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.freeze_hash);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 规模常量（9 条）
    push_tlv_u64(&mut out, 0x10, DOMAIN_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, CASES_PER_DOMAIN as u64);
    push_tlv_u64(&mut out, 0x12, CASE_TOTAL as u64);
    push_tlv_u64(&mut out, 0x13, REG_CAP as u64);
    push_tlv_u64(&mut out, 0x14, MERGE_ROUNDS as u64);
    push_tlv_u64(&mut out, 0x15, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x16, STEP_CAP);
    push_tlv_u64(&mut out, 0x17, MEM_PEAK_CAP);
    push_tlv_u64(&mut out, 0x18, MountState::COUNT as u64);
    // 域侧声明面（4 条）
    push_tlv_u64(&mut out, 0x20, o.metrics.domains);
    push_tlv_u64(&mut out, 0x21, o.metrics.cases);
    push_tlv_u64(&mut out, 0x22, o.net.decl.fingerprint());
    push_tlv_u64(&mut out, 0x23, o.net.decl.cover_bits as u64);
    // 注册表面（6 条）
    push_tlv_u64(&mut out, 0x30, o.net.reg.rev as u64);
    push_tlv_u64(&mut out, 0x31, o.metrics.reg_n);
    push_tlv_u64(&mut out, 0x32, o.metrics.mounted_n);
    push_tlv_u64(&mut out, 0x33, o.metrics.shadow_n);
    push_tlv_u64(&mut out, 0x34, o.net.reg.fingerprint());
    push_tlv_u64(&mut out, 0x35, registry_definition_hash());
    // 合并与可见面（6 条）
    push_tlv_u64(&mut out, 0x40, o.metrics.merged_total);
    push_tlv_u64(&mut out, 0x41, o.metrics.idem_rounds);
    push_tlv_u64(&mut out, 0x42, merge_fingerprint(&o.net.mlog));
    push_tlv_u64(&mut out, 0x43, o.metrics.visible_n);
    push_tlv_u64(&mut out, 0x44, o.metrics.blind_n);
    push_tlv_u64(&mut out, 0x45, view_fingerprint(&o.net.view));
    // 双向对账面（3 条）
    push_tlv_u64(&mut out, 0x50, o.metrics.missing_n);
    push_tlv_u64(&mut out, 0x51, o.metrics.ghost_n);
    push_tlv_u64(&mut out, 0x52, o.recon.deviation);
    // 账目与冻结面（6 条）
    push_tlv_u64(&mut out, 0x60, o.ring.pushed);
    push_tlv_u64(&mut out, 0x61, o.ring.dropped);
    push_tlv_u64(&mut out, 0x62, o.metrics.steps);
    push_tlv_u64(&mut out, 0x63, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x64, o.replay.freeze_hash);
    push_tlv_u64(&mut out, 0x65, o.detects.verdict_mask as u64);
    // 逐形态命中（14 条）
    let mut i: usize = 0;
    while i < Form::COUNT {
        push_tlv_u64(&mut out, 0x80u8.wrapping_add(i as u8), o.detects.hits[i] as u64);
        i = i.wrapping_add(1);
    }
    out
}
/// TLV 长度核算。
pub fn tlv_len_check(o: &MaObserve) -> MaResult<usize> {
    let buf = export_tlv(o);
    let want: usize = TLV_HEAD_LEN + TLV_ITEM_COUNT * TLV_ITEM_LEN;
    if buf.len() != want {
        return Err(MaErr::of_aux(
            AuxCode::RegIdxOver,
            format!("TLV 长度不符：got={} want={}", buf.len(), want),
        ));
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(MaErr::of_aux(
            AuxCode::RegIdxOver,
            format!("组头长度字段不符：got={} want={}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    Ok(buf.len())
}

// ===========================================================================
// 15. observe
// ===========================================================================

/// 观测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MaObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 环形账。
    pub ring: RingLedger,
    /// 守恒账。
    pub recon: Reconcile,
    /// 注入检出。
    pub detects: Detections,
    /// 净态检出。
    pub net_detect: Detections,
    /// 状态机。
    pub fsm: StageFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl MaObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 重放一致性。
    pub fn replay_match(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.reg_fp == self.net.reg.fingerprint()
            && self.replay.decl_fp == self.net.decl.fingerprint()
            && self.replay.visible_n == self.metrics.visible_n
            && self.replay.ring_pushed == self.metrics.ring_pushed
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "observe seed={:016X} all_ok={} | {} | {} | {}",
            self.seed,
            if self.all_ok() { 1 } else { 0 },
            self.metrics.summary(),
            self.recon.summary(),
            self.freeze.describe()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(48);
        out.push(format!("# UNX-F3461 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("recon    {}", self.recon.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("decl     {}", self.net.decl.summary()));
        out.push(format!("reg      {}", self.net.reg.summary()));
        out.push(format!("merge    {}", self.net.mlog.summary()));
        out.push(format!("view     {}", self.net.view.summary()));
        out.push(format!("bidir    {}", self.net.bidir.summary()));
        out.push(format!("ring     {}", self.ring.summary()));
        out.push(format!("evidence {}", self.net.ev.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        out.push(format!("replay_match={}", if self.replay_match() { 1 } else { 0 }));
        let mut i: usize = 0;
        while i < Stage::ALL.len() {
            out.push(format!("stage    {}", Stage::ALL[i].four_columns()));
            i = i.wrapping_add(1);
        }
        out
    }
}
/// 真实跑逻辑的观测入口（M-220 挂载全链路）。
pub fn observe(seed: u64) -> MaResult<MaObserve> {
    let net = Snapshot::net()?;
    if !net.decl.cover_complete() {
        return Err(MaErr::of_form(
            Form::DeclMissing,
            format!("域声明缺项 cover={:08X}", net.decl.cover_bits),
        ));
    }
    if !net.decl.dup_free() {
        return Err(MaErr::of_form(
            Form::DeclDup,
            format!("域声明重号 dup={:08X}", net.decl.dup_bits),
        ));
    }
    if !net.reg.cover_complete(CASE_TOTAL) {
        return Err(MaErr::of_form(
            Form::RegMissing,
            format!("注册表缺项 {}", net.reg.summary()),
        ));
    }
    if !net.reg.dup_free() {
        return Err(MaErr::of_form(
            Form::RegDup,
            format!("注册表重号 dup={:08X}", net.reg.dup_bits),
        ));
    }
    if !net.mlog.all_idempotent() {
        return Err(MaErr::of_form(
            Form::MergeNotIdempotent,
            format!("合并不幂等 {}", net.mlog.summary()),
        ));
    }
    // 双向登记制：漏挂与幽灵挂载都必须为零
    if net.bidir.missing_n() != 0 {
        return Err(MaErr::of_form(
            Form::MissingInRegistry,
            format!("漏挂 {} 条 {}", net.bidir.missing_n(), net.bidir.summary()),
        ));
    }
    if net.bidir.ghost_n() != 0 {
        return Err(MaErr::of_form(
            Form::GhostMount,
            format!("幽灵挂载 {} 条 {}", net.bidir.ghost_n(), net.bidir.summary()),
        ));
    }
    if net.view.visible_n != net.reg.mounted_n() {
        return Err(MaErr::of_form(
            Form::ExecBlindToMounted,
            format!("可见数 {} ≠ 已挂载数 {}", net.view.visible_n, net.reg.mounted_n()),
        ));
    }
    if view_blind_vs_reg(&net) != 0 {
        return Err(MaErr::of_form(
            Form::ExecBlindToMounted,
            format!("执行器有盲区 {}", net.view.summary()),
        ));
    }
    if view_leak_vs_reg(&net) != 0 {
        return Err(MaErr::of_form(
            Form::ExecSeesUnmounted,
            format!("执行器可见性越权 {}", net.view.summary()),
        ));
    }
    if !net.ring.balanced() {
        return Err(MaErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒 {}", net.ring.summary()),
        ));
    }
    let recon = net.reconcile();
    if !recon.balanced {
        return Err(err_recon(recon.sum_a, recon.sum_b, recon.sum_c));
    }
    let mut fsm = StageFsm::new();
    fsm.drive_all(1)?;
    let illegal: [Stage; 3] = [Stage::D3Visible, Stage::D1Declare, Stage::Sealed];
    for s in illegal.iter() {
        let _ = fsm.try_go(*s, 99);
    }
    if !fsm.state.is_sealed() || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(err_frozen(fsm.state.name(), Stage::Sealed.name()));
    }
    let stability = stability_check()?;
    let (inj, forms) = inject_all()?;
    let detects = detect_all(&inj, &forms)?;
    if detects.hit_count() != Form::COUNT {
        return Err(MaErr::of_form(
            Form::ReconcileDeviation,
            format!("注入检出不足 {}/{}", detects.hit_count(), Form::COUNT),
        ));
    }
    let net_detect = detect_one(&net);
    if net_detect.hit_count() != 0 {
        return Err(MaErr::of_form(
            Form::ReconcileDeviation,
            format!("净态检出非零 {:?}", net_detect.hit_list()),
        ));
    }
    let freeze = FreezeRecord::build(&net.decl, &net.reg);
    freeze.verify(&net.decl, &net.reg)?;
    if net.ev.freeze_fp != net.ev.registered_probe {
        return Err(MaErr::of_form(
            Form::FingerprintDrift,
            format!("探针漂移 {:016X} vs {:016X}", net.ev.freeze_fp, net.ev.registered_probe),
        ));
    }
    let mut shadow_n: u64 = 0;
    let mut bi: usize = 0;
    while bi < net.reg.items.len() {
        if net.reg.items[bi].state == MountState::Shadow {
            shadow_n = shadow_n.wrapping_add(1);
        }
        bi = bi.wrapping_add(1);
    }
    let metrics = Metrics {
        domains: DOMAIN_COUNT as u64,
        cases_per_domain: CASES_PER_DOMAIN as u64,
        cases: CASE_TOTAL as u64,
        reg_n: net.reg.live_n(),
        mounted_n: net.reg.mounted_n(),
        shadow_n,
        visible_n: net.view.visible_n,
        missing_n: net.bidir.missing_n(),
        ghost_n: net.bidir.ghost_n(),
        merged_total: net.mlog.merged_total,
        idem_rounds: net.mlog.idempotent_rounds,
        blind_n: view_blind_vs_reg(&net),
        ring_pushed: net.ring.pushed,
        ring_dropped: net.ring.dropped,
        steps: net.mlog.steps,
        inject_hits: detects.hit_count(),
        net_hits: net_detect.hit_count(),
        border_violations: net.ev.border_violations,
        mem_peak: mem_estimate() as u64,
    };
    let replay = replay(seed)?;
    let ring_clone = net.ring.clone();
    let o = MaObserve {
        seed,
        ring: ring_clone,
        net,
        recon,
        detects,
        net_detect,
        fsm,
        stability,
        freeze,
        metrics,
        replay,
    };
    let _ = tlv_len_check(&o)?;
    if !o.replay_match() {
        return Err(err_frozen("replay", "match"));
    }
    Ok(o)
}
/// 峰值内存估算。
pub fn mem_estimate() -> usize {
    let decl: usize = CASE_TOTAL * (core::mem::size_of::<DeclItem>() + 32);
    let reg: usize = REG_CAP * (core::mem::size_of::<RegItem>() + 32);
    let view: usize = CASE_TOTAL * 16 + 64;
    let ring: usize = RING_CAP * (core::mem::size_of::<RingEntry>() + 16);
    let mlog: usize = (DOMAIN_COUNT * MERGE_ROUNDS) * (core::mem::size_of::<MergeEntry>() + 16);
    decl + reg + view + ring + mlog
}
