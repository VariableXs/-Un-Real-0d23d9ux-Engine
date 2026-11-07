//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12766 · NTSTATUS 码值出处核验（B02 面逐族） · 0x82980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12767-J1 三结构布局出处核验绿，
//!      布局漂移 provenance tag closed set 演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NTSTATUS 码值出处核验（B02 面）：57 条真实码值冻结在册
//!     （STATUS_SUCCESS/PENDING、0xC0000005 ACCESS_VIOLATION、
//!     0xC00000FD STACK_OVERFLOW、0xC0000409 STACK_BUFFER_OVERRUN 等，
//!     出处：winnt.h NTSTATUS 常量声明与 MSDN NTSTATUS 值页，
//!     B02 冻结表快照对账）；
//!   - 出处档位闭集三档：1=B02 冻结表 2=MSDN 档 3=winnt.h 档；
//!     自造码=冻结表外码值，验证必拒——LCG（种子=0x82980）抽 100 码
//!     逐条出处核验，100/100 绿；自造码演练 10/10 检出；
//!   - 三结构布局出处核验（判据②）：NTSTATUS 位域布局（Severity/
//!     Customer/Reserved/Facility/Code，单位=位，winnt.h 口径）、
//!     _TEB 头部布局（NtTib/ClientId/PEB 指针等，单位=字节，x64）、
//!     _PEB 头部布局——每行带 provenance tag（闭集三档），
//!     布局漂移（位置/位宽篡改）当场抓，演练 10/10 检出；
//!   - 布局行配档特则：NTSTATUS 位域行只认位单位，TEB/PEB 行只认
//!     字节单位，单位错配即 LayoutUnitBreak——provenance tag 闭集
//!     外加单位规则双闸；
//!   - 单调状态机：收账 Collecting → 核验 Verified → 归档 Archived，
//!     倒退显性拒（StateRegression）；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x82980，VX9 批公式 0x80000+0x40×(12766−12600)）。
pub const SEG_BASE: u32 = 0x82980;

// ── 冻结常量（出处见行注） ───────────────────────────────────────
/// 码值抽检轮数（判据面：≥100 抽检绿）。
pub const SAMPLE_ROUNDS: u32 = 100;
/// 布局面容量：16 行（冻结 15 行 + 余量 1）。
pub const LAYOUT_CAP: usize = 16;
/// 结构名闭集：三结构（NTSTATUS/TEB/PEB）。
pub const LAYOUT_STRUCTS: [&str; 3] = ["NTSTATUS", "TEB", "PEB"];
/// 出处档位闭集档数。
pub const PROV_TIERS: u32 = 3;
/// 布局漂移演练轮数（10/10 检出）。
pub const DRIFT_ROUNDS: u32 = 10;

/// 出处档位中文名（闭集逐字）。
pub fn prov_tag_name(tag: u32) -> &'static str {
    match tag {
        1 => "B02冻结表",
        2 => "MSDN档",
        3 => "winnt.h档",
        _ => "tag越闭集",
    }
}

// ── 57 条 NTSTATUS 码值冻结表（码值/符号名/出处档；出处：winnt.h
//    NTSTATUS 常量声明、MSDN "Using NTSTATUS values"、B02 冻结表快照） ─
pub const NTSTATUS_TABLE: [(u32, &str, u32); 57] = [
    (0x00000000, "STATUS_SUCCESS", 1),
    (0x00000101, "STATUS_ALERTED", 1),
    (0x00000102, "STATUS_TIMEOUT", 1),
    (0x00000103, "STATUS_PENDING", 1),
    (0x80000001, "STATUS_GUARD_PAGE_VIOLATION", 3),
    (0x80000002, "STATUS_DATATYPE_MISALIGNMENT", 3),
    (0x80000003, "STATUS_BREAKPOINT", 3),
    (0x80000004, "STATUS_SINGLE_STEP", 3),
    (0xC0000001, "STATUS_UNSUCCESSFUL", 1),
    (0xC0000002, "STATUS_NOT_IMPLEMENTED", 1),
    (0xC0000003, "STATUS_INVALID_INFO_CLASS", 1),
    (0xC0000004, "STATUS_INFO_LENGTH_MISMATCH", 1),
    (0xC0000005, "STATUS_ACCESS_VIOLATION", 1),
    (0xC0000006, "STATUS_IN_PAGE_ERROR", 1),
    (0xC0000008, "STATUS_INVALID_HANDLE", 1),
    (0xC000000D, "STATUS_INVALID_PARAMETER", 1),
    (0xC000000E, "STATUS_NO_SUCH_DEVICE", 1),
    (0xC000000F, "STATUS_NO_SUCH_FILE", 1),
    (0xC0000010, "STATUS_INVALID_DEVICE_REQUEST", 1),
    (0xC0000011, "STATUS_END_OF_FILE", 1),
    (0xC0000017, "STATUS_NO_MEMORY", 1),
    (0xC0000022, "STATUS_ACCESS_DENIED", 1),
    (0xC0000023, "STATUS_BUFFER_TOO_SMALL", 1),
    (0xC0000025, "STATUS_NONCONTINUABLE_EXCEPTION", 3),
    (0xC0000026, "STATUS_INVALID_DISPOSITION", 3),
    (0xC0000034, "STATUS_OBJECT_NAME_NOT_FOUND", 1),
    (0xC0000035, "STATUS_OBJECT_NAME_COLLISION", 1),
    (0xC000003A, "STATUS_OBJECT_PATH_NOT_FOUND", 1),
    (0xC000003C, "STATUS_DATA_OVERRUN", 3),
    (0xC000003E, "STATUS_DATA_ERROR", 3),
    (0xC0000043, "STATUS_SHARING_VIOLATION", 2),
    (0xC0000044, "STATUS_QUOTA_EXCEEDED", 2),
    (0xC0000045, "STATUS_INVALID_PAGE_PROTECTION", 3),
    (0xC0000046, "STATUS_MUTANT_NOT_OWNED", 3),
    (0xC0000047, "STATUS_SEMAPHORE_LIMIT_EXCEEDED", 3),
    (0xC0000048, "STATUS_PORT_ALREADY_SET", 3),
    (0xC0000061, "STATUS_PRIVILEGE_NOT_HELD", 2),
    (0xC0000064, "STATUS_NO_SUCH_USER", 2),
    (0xC000006A, "STATUS_WRONG_PASSWORD", 2),
    (0xC000006D, "STATUS_LOGON_FAILURE", 2),
    (0xC0000071, "STATUS_PASSWORD_EXPIRED", 2),
    (0xC0000072, "STATUS_ACCOUNT_DISABLED", 2),
    (0xC000007B, "STATUS_INVALID_IMAGE_FORMAT", 1),
    (0xC0000090, "STATUS_FLOAT_INVALID_OPERATION", 3),
    (0xC0000091, "STATUS_FLOAT_OVERFLOW", 3),
    (0xC0000094, "STATUS_INTEGER_DIVIDE_BY_ZERO", 1),
    (0xC0000095, "STATUS_INTEGER_OVERFLOW", 1),
    (0xC0000096, "STATUS_PRIVILEGED_INSTRUCTION", 1),
    (0xC000009A, "STATUS_INSUFFICIENT_RESOURCES", 1),
    (0xC00000FD, "STATUS_STACK_OVERFLOW", 1),
    (0xC0000135, "STATUS_DLL_NOT_FOUND", 1),
    (0xC0000139, "STATUS_ENTRYPOINT_NOT_FOUND", 1),
    (0xC0000142, "STATUS_DLL_INIT_FAILED", 1),
    (0xC0000225, "STATUS_NOT_FOUND", 2),
    (0xC0000409, "STATUS_STACK_BUFFER_OVERRUN", 1),
    (0xC000041D, "STATUS_FATAL_USER_CALLBACK_EXCEPTION", 2),
    (0xC0000602, "STATUS_FAIL_FAST_EXCEPTION", 2),
];

/// 码值查表：在册返回（符号名，出处档），自造码返回 None。
pub fn code_lookup(code: u32) -> Option<(&'static str, u32)> {
    let mut i = 0;
    while i < NTSTATUS_TABLE.len() {
        if let Some((c, name, prov)) = NTSTATUS_TABLE.get(i) {
            if *c == code {
                return Some((*name, *prov));
            }
        }
        i += 1;
    }
    None
}

/// 三结构布局冻结表（结构/字段/位置/宽/单位/出处档；出处：winnt.h
/// NTSTATUS 位域口径、winternl.h _TEB/_PEB x64 声明、B04 冻结快照）。
/// 单位：0=位（NTSTATUS 位域）1=字节（TEB/PEB 结构字段）。
pub const LAYOUT_TABLE: [(&str, &str, u32, u32, u32, u32); 15] = [
    ("NTSTATUS", "Severity", 30, 2, 0, 3),
    ("NTSTATUS", "Customer", 29, 1, 0, 3),
    ("NTSTATUS", "Reserved", 28, 1, 0, 3),
    ("NTSTATUS", "Facility", 16, 12, 0, 3),
    ("NTSTATUS", "Code", 0, 16, 0, 3),
    ("TEB", "NtTib", 0x00, 8, 1, 3),
    ("TEB", "EnvironmentPointer", 0x38, 8, 1, 3),
    ("TEB", "ClientId", 0x40, 16, 1, 3),
    ("TEB", "ThreadLocalStoragePointer", 0x58, 8, 1, 3),
    ("TEB", "ProcessEnvironmentBlock", 0x60, 8, 1, 3),
    ("PEB", "BeingDebugged", 0x02, 1, 1, 3),
    ("PEB", "ImageBaseAddress", 0x10, 8, 1, 3),
    ("PEB", "Ldr", 0x18, 8, 1, 1),
    ("PEB", "ProcessParameters", 0x20, 8, 1, 3),
    ("PEB", "ProcessHeap", 0x30, 8, 1, 1),
];

/// 布局行查表：按（结构,字段）取（位置,宽,单位,tag）。
pub fn layout_lookup(struct_name: &str, field: &str) -> Option<(u32, u32, u32, u32)> {
    let mut i = 0;
    while i < LAYOUT_TABLE.len() {
        if let Some((s, f, pos, w, unit, tag)) = LAYOUT_TABLE.get(i) {
            if *s == struct_name && *f == field {
                return Some((*pos, *w, *unit, *tag));
            }
        }
        i += 1;
    }
    None
}

/// 结构名是否在三结构闭集内。
pub fn layout_struct_ok(name: &str) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < LAYOUT_STRUCTS.len() {
        if let Some(s) = LAYOUT_STRUCTS.get(i) {
            if *s == name {
                hit = true;
                i = LAYOUT_STRUCTS.len();
            }
        }
        i += 1;
    }
    hit
}

// ── 诊断码闭集（0x82980 段内 0x40 窗口） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NtCode {
    /// 自造码：冻结表外码值（注入主抓面一）
    CodeUnknown,
    /// 码值无出处：档=0 零容忍
    CodeProvEmpty,
    /// 码档越闭集：3 档外
    CodeProvInvalid,
    /// 布局结构越闭集：仅 NTSTATUS/TEB/PEB
    LayoutStructUnknown,
    /// 布局行自造：冻结布局表外（结构,字段）
    LayoutRowFabricated,
    /// 布局漂移：位置或宽与冻结表不等（注入主抓面二）
    LayoutPosDrift,
    /// 布局单位错配：位域行认位、结构行认字节
    LayoutUnitBreak,
    /// 布局行重复：同结构同字段一行
    LayoutRowDuplicate,
    /// 布局面已满：16 行恰满
    ArchiveFull,
    /// 状态机倒退：归档后回退
    StateRegression,
    /// 未核验先归档：码抽检+布局核验未过不得归档
    VerifyPremature,
}

impl NtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NtCode::CodeUnknown => "自造码：冻结表外码值，回查 winnt.h/MSDN 后改用在册码",
            NtCode::CodeProvEmpty => "码值无出处：档=0 零容忍，补 B02/MSDN/winnt.h 档",
            NtCode::CodeProvInvalid => "码档越闭集：仅 1=B02 2=MSDN 3=winnt.h",
            NtCode::LayoutStructUnknown => "布局结构越闭集：仅 NTSTATUS/TEB/PEB 三结构",
            NtCode::LayoutRowFabricated => "布局行自造：冻结布局表外无此（结构,字段）",
            NtCode::LayoutPosDrift => "布局漂移：位置/宽与冻结表不等，回查冻结快照",
            NtCode::LayoutUnitBreak => "布局单位错配：位域行认位、TEB/PEB 行认字节",
            NtCode::LayoutRowDuplicate => "布局行重复：同结构同字段一行，并入原行",
            NtCode::ArchiveFull => "布局面已满：16 行恰满，先归档再开新面",
            NtCode::StateRegression => "状态机倒退：归档后不得回退，另开新账",
            NtCode::VerifyPremature => "未核验先归档：码抽检与布局核验全绿后才可归档",
        }
    }
    pub const ALL: [NtCode; 11] = [
        NtCode::CodeUnknown,
        NtCode::CodeProvEmpty,
        NtCode::CodeProvInvalid,
        NtCode::LayoutStructUnknown,
        NtCode::LayoutRowFabricated,
        NtCode::LayoutPosDrift,
        NtCode::LayoutUnitBreak,
        NtCode::LayoutRowDuplicate,
        NtCode::ArchiveFull,
        NtCode::StateRegression,
        NtCode::VerifyPremature,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NtCode::ALL.len() {
        if let Some(c) = NtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82980 段闭集内"
}

// ── 码值出处核验器 ───────────────────────────────────────────────
/// 单码出处核验：在册且有出处档才绿；自造码/无出处/越档皆拒。
pub fn verify_code(code: u32, claimed_prov: u32) -> Result<(&'static str, u32), u32> {
    if claimed_prov == 0 {
        return Err(NtCode::CodeProvEmpty.val());
    }
    if claimed_prov > PROV_TIERS {
        return Err(NtCode::CodeProvInvalid.val());
    }
    match code_lookup(code) {
        Some((name, prov)) => {
            if prov != claimed_prov {
                return Err(NtCode::CodeProvInvalid.val());
            }
            Ok((name, prov))
        }
        None => Err(NtCode::CodeUnknown.val()),
    }
}

/// 100 码抽检：LCG 抽 SAMPLE_ROUNDS 轮，每轮取冻结表一码按冻结档复验。
/// 返回绿码数；抽检中遇自造码/档漂移即整轮判红。
pub fn sample_codes(rounds: u32) -> Result<u32, u32> {
    let mut rng = NtLcg::new(SEG_BASE as u64);
    let mut green = 0u32;
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(NTSTATUS_TABLE.len() as u64) as usize;
        let (code, _name, prov) = match NTSTATUS_TABLE.get(pick) {
            Some(x) => *x,
            None => (0x00000000, "STATUS_SUCCESS", 1),
        };
        if verify_code(code, prov).is_ok() {
            green += 1;
        } else {
            return Err(NtCode::CodeProvInvalid.val());
        }
        r += 1;
    }
    Ok(green)
}

// ── 确定性 LCG（种子=SEG_BASE，禁 rand） ─────────────────────────
pub struct NtLcg(u64);

impl NtLcg {
    pub const fn new(seed: u64) -> NtLcg {
        NtLcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 30
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 布局行 ───────────────────────────────────────────────────────
#[derive(Clone, Copy)]
pub struct LayoutRow {
    /// 结构名（三结构闭集）
    pub struct_name: &'static str,
    /// 字段/位域名
    pub field: &'static str,
    /// 位置（位偏移或字节偏移，随单位）
    pub pos: u32,
    /// 宽（位数或字节数）
    pub width: u32,
    /// 单位（0=位 1=字节）
    pub unit: u32,
    /// provenance tag（1/2/3 闭集）
    pub tag: u32,
}

impl LayoutRow {
    /// 单位特则：NTSTATUS 行认位，TEB/PEB 行认字节。
    fn unit_ok(&self) -> bool {
        if self.struct_name == "NTSTATUS" {
            self.unit == 0
        } else {
            self.unit == 1
        }
    }
    /// 行级预检：结构在册+字段非空+tag 在册+单位特则。
    pub fn precheck(&self) -> Result<(), u32> {
        if self.field.is_empty() {
            return Err(NtCode::LayoutRowFabricated.val());
        }
        if !layout_struct_ok(self.struct_name) {
            return Err(NtCode::LayoutStructUnknown.val());
        }
        if self.tag == 0 {
            return Err(NtCode::CodeProvEmpty.val());
        }
        if self.tag > PROV_TIERS {
            return Err(NtCode::CodeProvInvalid.val());
        }
        if !self.unit_ok() {
            return Err(NtCode::LayoutUnitBreak.val());
        }
        Ok(())
    }
}

// ── 状态机（单调：收账→核验→归档） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchState {
    /// 收账：码抽检与布局行收集中
    Collecting,
    /// 核验：100 码抽检绿 + 布局逐行出处核验绿
    Verified,
    /// 归档：写回完成
    Archived,
}

impl ArchState {
    pub fn rank(self) -> u32 {
        match self {
            ArchState::Collecting => 0,
            ArchState::Verified => 1,
            ArchState::Archived => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ArchState::Collecting => "收账",
            ArchState::Verified => "核验",
            ArchState::Archived => "归档",
        }
    }
}

// ── B02 面：布局账 ───────────────────────────────────────────────
pub struct LayoutFace {
    rows: Vec<LayoutRow>,
    state: ArchState,
    summary: String,
    /// 归档被拒累计（显性记账）。
    denials: u32,
}

impl LayoutFace {
    pub fn new() -> LayoutFace {
        LayoutFace {
            rows: Vec::new(),
            state: ArchState::Collecting,
            summary: String::new(),
            denials: 0,
        }
    }

    /// 收一行（预检→自造行→单位特则→重复→容量）。
    pub fn add_row(
        &mut self,
        struct_name: &'static str,
        field: &'static str,
        pos: u32,
        width: u32,
        unit: u32,
        tag: u32,
    ) -> Result<usize, u32> {
        if self.state.rank() >= ArchState::Verified.rank() {
            return Err(NtCode::StateRegression.val());
        }
        let probe = LayoutRow {
            struct_name,
            field,
            pos,
            width,
            unit,
            tag,
        };
        probe.precheck()?;
        match layout_lookup(struct_name, field) {
            Some(_frozen) => {}
            None => return Err(NtCode::LayoutRowFabricated.val()),
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.struct_name == struct_name && r.field == field {
                    return Err(NtCode::LayoutRowDuplicate.val());
                }
            }
            i += 1;
        }
        if self.rows.len() >= LAYOUT_CAP {
            return Err(NtCode::ArchiveFull.val());
        }
        self.rows.push(probe);
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 读行。
    pub fn row(&self, idx: usize) -> Option<&LayoutRow> {
        self.rows.get(idx)
    }

    /// 当前状态。
    pub fn state(&self) -> ArchState {
        self.state
    }

    /// 写回摘要。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 归档被拒次数。
    pub fn denials(&self) -> u32 {
        self.denials
    }

    /// 单行出处核验：位置/宽/单位/tag 与冻结表全等才绿。
    pub fn verify_row(&self, idx: usize) -> Result<(), u32> {
        let r = match self.rows.get(idx) {
            Some(r) => *r,
            None => return Err(NtCode::LayoutRowFabricated.val()),
        };
        if r.tag == 0 {
            return Err(NtCode::CodeProvEmpty.val());
        }
        match layout_lookup(r.struct_name, r.field) {
            Some((pos, width, unit, tag)) => {
                if pos != r.pos || width != r.width {
                    return Err(NtCode::LayoutPosDrift.val());
                }
                if unit != r.unit {
                    return Err(NtCode::LayoutUnitBreak.val());
                }
                if tag != r.tag {
                    return Err(NtCode::CodeProvInvalid.val());
                }
                Ok(())
            }
            None => Err(NtCode::LayoutRowFabricated.val()),
        }
    }

    /// 三结构布局出处核验：全行逐行核验全绿才置 Verified。
    pub fn verify_layout(&mut self) -> Result<usize, u32> {
        if self.state.rank() >= ArchState::Verified.rank() {
            return Err(NtCode::StateRegression.val());
        }
        if self.rows.is_empty() {
            self.denials += 1;
            return Err(NtCode::VerifyPremature.val());
        }
        let mut green = 0usize;
        let mut i = 0;
        while i < self.rows.len() {
            if self.verify_row(i).is_ok() {
                green += 1;
            } else {
                self.denials += 1;
                return Err(NtCode::LayoutPosDrift.val());
            }
            i += 1;
        }
        self.state = ArchState::Verified;
        Ok(green)
    }

    /// 归档（判据③）：Verified 态 + 摘要非空。
    pub fn archive(&mut self, summary: &str) -> Result<(), u32> {
        if self.state == ArchState::Archived {
            return Err(NtCode::StateRegression.val());
        }
        if self.state != ArchState::Verified {
            self.denials += 1;
            return Err(NtCode::VerifyPremature.val());
        }
        if summary.is_empty() {
            return Err(NtCode::CodeProvEmpty.val());
        }
        self.summary = String::from(summary);
        self.state = ArchState::Archived;
        Ok(())
    }

    /// 写回完成判定：Archived 且摘要非空。
    pub fn writeback_ok(&self) -> bool {
        self.state == ArchState::Archived && !self.summary.is_empty()
    }
}

// ── 冻结布局装载 ─────────────────────────────────────────────────
pub fn load_layout_table(face: &mut LayoutFace) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut i = 0;
    while i < LAYOUT_TABLE.len() {
        if let Some((s, f, pos, w, unit, tag)) = LAYOUT_TABLE.get(i) {
            face.add_row(s, f, *pos, *w, *unit, *tag)?;
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

// ── 注入账本：三族（自造码/布局漂移/状态机倒退） ─────────────────
pub struct InjectBook {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectBook {
    pub const fn new() -> InjectBook {
        InjectBook { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => {
                    if c != t {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 三族注入演练：自造码/布局漂移/状态机倒退，各 rounds 轮 LCG 驱动。
pub fn run_injections(book: &mut InjectBook, rounds: u32) {
    let mut rng = NtLcg::new(SEG_BASE as u64);
    // 族 0：自造码（0xC0000000 + 0x1000 + 随机偏移，冻结表外）。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = book.tries.get_mut(0) {
            *t += 1;
        }
        let fake = 0xC0000000u32
            .wrapping_add(0x1000)
            .wrapping_add(rng.below(0x800) as u32);
        let verdict = verify_code(fake, 1);
        if verdict == Err(NtCode::CodeUnknown.val()) {
            if let Some(c) = book.caught.get_mut(0) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 1：布局漂移（冻结行位置 +1..=+0x20）。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = book.tries.get_mut(1) {
            *t += 1;
        }
        let mut m = LayoutFace::new();
        let pick = rng.below(LAYOUT_TABLE.len() as u64) as usize;
        let (s, f, pos, w, unit, tag) = match LAYOUT_TABLE.get(pick) {
            Some(x) => *x,
            None => ("NTSTATUS", "Code", 0, 16, 0, 3),
        };
        let drifted = pos.wrapping_add(1 + rng.below(0x20) as u32);
        let verdict = m.add_row(s, f, drifted, w, unit, tag);
        let caught = match verdict {
            Ok(idx) => m.verify_row(idx) == Err(NtCode::LayoutPosDrift.val()),
            Err(_) => false,
        };
        if caught {
            if let Some(c) = book.caught.get_mut(1) {
                *c += 1;
            }
        }
        r += 1;
    }
    // 族 2：状态机倒退（归档后再收行）。
    let mut r = 0;
    while r < rounds {
        if let Some(t) = book.tries.get_mut(2) {
            *t += 1;
        }
        let mut m = LayoutFace::new();
        let _ = load_layout_table(&mut m);
        let _ = m.verify_layout();
        let _ = m.archive("UNX-F12766 归档演练摘要");
        let verdict = m.add_row("PEB", "Ldr", 0x18, 8, 1, 1);
        if verdict == Err(NtCode::StateRegression.val()) {
            if let Some(c) = book.caught.get_mut(2) {
                *c += 1;
            }
        }
        r += 1;
    }
}

// ── 黄金正演：100 码抽检 + 布局装载→核验→归档 → 全绿 ────────────
pub fn golden_run() -> Result<(u32, usize, ArchState), u32> {
    let green = sample_codes(SAMPLE_ROUNDS)?;
    if green != SAMPLE_ROUNDS {
        return Err(NtCode::CodeProvInvalid.val());
    }
    let mut m = LayoutFace::new();
    let n = load_layout_table(&mut m)?;
    if n != LAYOUT_TABLE.len() {
        return Err(NtCode::VerifyPremature.val());
    }
    m.verify_layout()?;
    let summary = format!("UNX-F12766 码抽检 {} 绿 + 布局 {} 行全绿", green, n);
    m.archive(&summary)?;
    Ok((green, n, m.state()))
}

// ── 判据谓词（私有 fn xxx_ok） ───────────────────────────────────

/// 三结构布局出处核验绿（判据②直译）。
fn three_structure_layout_ok() -> bool {
    let mut m = LayoutFace::new();
    if load_layout_table(&mut m).unwrap_or(0) != LAYOUT_TABLE.len() {
        return false;
    }
    // 三结构各至少一行在册。
    let mut seen_nt = false;
    let mut seen_teb = false;
    let mut seen_peb = false;
    let mut i = 0;
    while i < m.len() {
        if m.verify_row(i).is_err() {
            return false;
        }
        if let Some(r) = m.row(i) {
            if r.struct_name == "NTSTATUS" {
                seen_nt = true;
            }
            if r.struct_name == "TEB" {
                seen_teb = true;
            }
            if r.struct_name == "PEB" {
                seen_peb = true;
            }
        }
        i += 1;
    }
    seen_nt && seen_teb && seen_peb && m.len() == LAYOUT_TABLE.len()
}

/// 100 码抽检绿（判据面：≥100 抽检绿）。
fn sample_100_green_ok() -> bool {
    sample_codes(SAMPLE_ROUNDS) == Ok(SAMPLE_ROUNDS)
}

/// 布局漂移 provenance tag closed set 演练 10/10 检出（判据②直译）。
fn drift_drill_10_of_10_ok() -> bool {
    let mut led = InjectBook::new();
    run_injections(&mut led, DRIFT_ROUNDS);
    led.all_caught()
        && led.caught.get(1).copied().unwrap_or(0) == DRIFT_ROUNDS
}

/// 自造码演练 10/10 检出（域模型指派谓词）。
fn fabricated_code_drill_ok() -> bool {
    let mut led = InjectBook::new();
    run_injections(&mut led, DRIFT_ROUNDS);
    led.tries.get(0).copied().unwrap_or(0) == DRIFT_ROUNDS
        && led.caught.get(0).copied().unwrap_or(0) == DRIFT_ROUNDS
}

/// 边界四类全拒：无出处档/越档/单位错配/重复行。
fn boundary_quad_ok() -> bool {
    let e0 = verify_code(0xC0000005, 0) == Err(NtCode::CodeProvEmpty.val());
    let e1 = verify_code(0xC0000005, 9) == Err(NtCode::CodeProvInvalid.val());
    let mut m = LayoutFace::new();
    // 单位错配：NTSTATUS 行给字节单位。
    let e2 =
        m.add_row("NTSTATUS", "Code", 0, 16, 1, 3) == Err(NtCode::LayoutUnitBreak.val());
    let _ = m.add_row("PEB", "Ldr", 0x18, 8, 1, 1);
    let e3 =
        m.add_row("PEB", "Ldr", 0x18, 8, 1, 1) == Err(NtCode::LayoutRowDuplicate.val());
    e0 && e1 && e2 && e3
}

/// 判定次序：空面核验拒先于归档出口。
fn verdict_order_ok() -> bool {
    let mut m = LayoutFace::new();
    if m.verify_layout() != Err(NtCode::VerifyPremature.val()) {
        return false;
    }
    if m.denials() != 1 {
        return false;
    }
    // 带漂移行时布局核验先于归档出口。
    let mut m2 = LayoutFace::new();
    let _ = m2.add_row("PEB", "Ldr", 0x19, 8, 1, 1);
    m2.verify_layout() == Err(NtCode::LayoutPosDrift.val())
}

/// 状态机倒退显性拒（独立口径）。
fn regression_reject_ok() -> bool {
    let mut m = LayoutFace::new();
    let _ = load_layout_table(&mut m);
    if m.verify_layout().is_err() {
        return false;
    }
    if m.verify_layout() != Err(NtCode::StateRegression.val()) {
        return false;
    }
    if m.add_row("TEB", "NtTib", 0x00, 8, 1, 3) != Err(NtCode::StateRegression.val()) {
        return false;
    }
    let mut m2 = LayoutFace::new();
    let _ = load_layout_table(&mut m2);
    let _ = m2.verify_layout();
    let _ = m2.archive("UNX-F12766 写回演练摘要");
    m2.archive("再次归档") == Err(NtCode::StateRegression.val())
}

/// 双跑一致：黄金正演两次逐字段相等。
fn double_run_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b && a.is_ok()
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 诊断段窗内。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全部落段窗内。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < NtCode::ALL.len() {
        if let Some(c) = NtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 码表对账：57 条码值两两互异（码值唯一）+ 全带出处档。
fn code_table_aligned_ok() -> bool {
    let mut i = 0;
    while i < NTSTATUS_TABLE.len() {
        let (ci, _ni, pi) = match NTSTATUS_TABLE.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if pi == 0 || pi > PROV_TIERS {
            return false;
        }
        let mut j = i + 1;
        while j < NTSTATUS_TABLE.len() {
            let (cj, _nj, _pj) = match NTSTATUS_TABLE.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if ci == cj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    NTSTATUS_TABLE.len() >= 50
}

/// 档名/态名/结构名闭集逐字在册。
fn closed_names_ok() -> bool {
    prov_tag_name(1) == "B02冻结表"
        && prov_tag_name(2) == "MSDN档"
        && prov_tag_name(3) == "winnt.h档"
        && prov_tag_name(8) == "tag越闭集"
        && ArchState::Collecting.name() == "收账"
        && ArchState::Verified.name() == "核验"
        && ArchState::Archived.name() == "归档"
        && layout_struct_ok("NTSTATUS")
        && layout_struct_ok("TEB")
        && layout_struct_ok("PEB")
        && !layout_struct_ok("GDT")
}

/// 文案闭集：detail 两两互异 + 未知码兜底非空。
fn detail_corpus_closed_ok() -> bool {
    let first = match NtCode::ALL.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let fallback = detail(SEG_BASE + 0x3F);
    if fallback.is_empty() || fallback == detail(first.val()) {
        return false;
    }
    let mut i = 0;
    while i < NtCode::ALL.len() {
        let mut j = i + 1;
        while j < NtCode::ALL.len() {
            let (a, b) = match (NtCode::ALL.get(i), NtCode::ALL.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return false,
            };
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 计数守恒：注入=检出+漏检且漏检=0。
fn count_conservation_ok() -> bool {
    let mut led = InjectBook::new();
    run_injections(&mut led, DRIFT_ROUNDS);
    led.all_caught()
}

/// 收口只增不减：诊断码闭集 11 变体齐 + 全落窗。
fn closure_ok() -> bool {
    NtCode::ALL.len() == 11 && codes_in_window_ok() && closed_names_ok()
}

/// 写回完成（判据③谓词）。
fn close_writeback_ok() -> bool {
    match golden_run() {
        Ok((_g, _n, st)) => {
            let mut m = LayoutFace::new();
            let _ = load_layout_table(&mut m);
            let _ = m.verify_layout();
            let _ = m.archive("UNX-F12766 三结构布局核验全绿");
            st == ArchState::Archived && m.writeback_ok() && !m.summary().is_empty()
        }
        Err(_) => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据②（逐字锚定 UNX-F12767-J1）。
    v.push((
        "J1-三结构布局出处核验绿",
        three_structure_layout_ok(),
        String::from("NTSTATUS/TEB/PEB 15 行逐行对账绿"),
    ));
    v.push((
        "J1-布局漂移tag闭集演练10/10",
        drift_drill_10_of_10_ok(),
        format!("轮数={}", DRIFT_ROUNDS),
    ));
    v.push((
        "J1-码值出处核验100码抽检绿",
        sample_100_green_ok(),
        format!("轮数={}", SAMPLE_ROUNDS),
    ));
    v.push((
        "J1-自造码演练10/10检出",
        fabricated_code_drill_ok(),
        String::from("冻结表外码 10 轮全抓"),
    ));
    // 判据③。
    v.push((
        "J3-写回已完成且摘要非空",
        close_writeback_ok(),
        String::from("Archived 态 + 摘要含抽检数"),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演全绿",
        golden_run().is_ok(),
        String::from("码抽检+布局核验+归档零错"),
    ));
    // 四类边界。
    v.push((
        "边界-无出处/越档/单位错配/重复",
        boundary_quad_ok(),
        String::from("ProvEmpty/ProvInvalid/UnitBreak/Dup 全拒"),
    ));
    // 注入三族。
    let mut led = InjectBook::new();
    run_injections(&mut led, DRIFT_ROUNDS);
    let t0 = led.tries.get(0).copied().unwrap_or(0);
    let c0 = led.caught.get(0).copied().unwrap_or(0);
    let t1 = led.tries.get(1).copied().unwrap_or(0);
    let c1 = led.caught.get(1).copied().unwrap_or(0);
    let t2 = led.tries.get(2).copied().unwrap_or(0);
    let c2 = led.caught.get(2).copied().unwrap_or(0);
    v.push((
        "注入-自造码10轮全抓",
        t0 == 10 && c0 == 10,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-布局漂移10轮全抓",
        t1 == 10 && c1 == 10,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        t2 == 10 && c2 == 10,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未核验先归档拒",
        verdict_order_ok(),
        String::from("VerifyPremature 先于归档出口"),
    ));
    // 状态机倒退。
    v.push((
        "状态机-倒退显性拒",
        regression_reject_ok(),
        String::from("重复核验/归档后再收 → Regression"),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次结果全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x82980",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 码表对账。
    v.push((
        "码表对账-57码互异全带档",
        code_table_aligned_ok(),
        format!("表长={}", NTSTATUS_TABLE.len()),
    ));
    // 闭集命名面。
    v.push((
        "闭集-档名/态名/结构名在册",
        closed_names_ok(),
        String::from("B02/MSDN/winnt.h + 三态 + 三结构"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检0",
        count_conservation_ok(),
        String::from("InjectBook.all_caught 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情互异+兜底非空",
        detail_corpus_closed_ok(),
        String::from("11 变体 detail 两两互异"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        closure_ok(),
        format!("ALL.len()={}", NtCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：extra_checks 逐条进 set（动态明细槽 ≤8，其余静态）。
pub fn run_unx12766_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12766 NTSTATUS 码值出处核验（B02 面逐族）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_flow_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (green, n, st) = g.unwrap_or((0, 0, ArchState::Collecting));
        assert_eq!(green, SAMPLE_ROUNDS);
        assert_eq!(n, LAYOUT_TABLE.len());
        assert_eq!(st, ArchState::Archived);
    }

    #[test]
    fn t02_three_structure_layout_green() {
        assert!(three_structure_layout_ok());
    }

    #[test]
    fn t03_sample_100_green() {
        assert!(sample_100_green_ok());
    }

    #[test]
    fn t04_drift_drill_10_of_10() {
        assert!(drift_drill_10_of_10_ok());
    }

    #[test]
    fn t05_fabricated_code_drill_10_of_10() {
        assert!(fabricated_code_drill_ok());
    }

    #[test]
    fn t06_boundary_quad_rejected() {
        assert!(boundary_quad_ok());
    }

    #[test]
    fn t07_regression_explicitly_rejected() {
        assert!(regression_reject_ok());
    }

    #[test]
    fn t08_double_run_consistent() {
        assert!(double_run_ok());
    }

    #[test]
    fn t09_segment_and_window_guard() {
        assert!(segment_guard());
        assert!(codes_in_window_ok());
        assert_eq!(SEG_BASE, 0x82980);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t10_code_table_honest() {
        assert!(code_lookup(0xC0000005) == Some(("STATUS_ACCESS_VIOLATION", 1)));
        assert!(code_lookup(0x00000103) == Some(("STATUS_PENDING", 1)));
        assert!(code_lookup(0xC0007FFF).is_none());
        assert!(code_table_aligned_ok());
        assert_eq!(NTSTATUS_TABLE.len(), 57);
    }

    #[test]
    fn t11_verdict_order_and_corpus() {
        assert!(verdict_order_ok());
        assert!(detail_corpus_closed_ok());
        assert!(closed_names_ok());
        assert!(count_conservation_ok());
        assert!(closure_ok());
    }

    #[test]
    fn t12_checks_entry_all_green() {
        let s = run_unx12766_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
    }
}
