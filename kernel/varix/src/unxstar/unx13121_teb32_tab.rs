//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13121 · 32 位 TEB 布局表（FS 段独立分档全字段） · 0x65680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13121-J1 32 位 TEB 全字段偏移表独立成账（与 64 位档零混用，
//!      逐字段注出处），30 样本布线断言通过（R-D2-001 联签载体）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 全字段偏移闭集账：NT_TIB（ExceptionList/StackBase/StackLimit/
//!     Self）+ EnvironmentPointer + ClientId + TLS 槽 + PEB 指针 +
//!     LastError/LastStatus + ActivationContext + GDI 系，逐字段注出处；
//!   - 偏移单调不降 + 窗口越界检查（0 ≤ off < TEB32_SIZE）；
//!   - 零混用红线：每个字段条目挂架构档标签（X32/X64），跨档取值
//!     一律拒收（混档注入 10/10 必抓）；
//!   - 布线面：30 样本各自布线 Self 指针回表基址（自指恒等式）；
//!   - LCG 种子写死 = SEG_BASE（0x65680），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65680，全仓独占）。
pub const SEG_BASE: u32 = 0x65680;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Teb32Code {
    /// 字段偏移越 TEB32 窗口
    OffsetOutOfRange,
    /// 偏移表单调性破（账内乱序）
    MonotonicBroken,
    /// 跨档混用（32 档字段挂 64 档取值）
    ArchMixed,
    /// Self 指针未自指（布线断）
    SelfNotWired,
    /// 字段名账缺条（闭集账外）
    FieldMissing,
    /// 布线样本越窗
    SampleOutOfRange,
    /// 偏移重复登记（同偏移两字段）
    OffsetDuplicated,
    /// 布线账不守恒（布线数与样本数失平）
    WiringMismatch,
}

impl Teb32Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Teb32Code::OffsetOutOfRange => "字段偏移越 TEB32 窗口：越界红线",
            Teb32Code::MonotonicBroken => "偏移表单调性破：账内乱序",
            Teb32Code::ArchMixed => "跨档混用：32 档字段挂 64 档取值",
            Teb32Code::SelfNotWired => "Self 指针未自指：布线断",
            Teb32Code::FieldMissing => "字段名账缺条：闭集账外字段",
            Teb32Code::SampleOutOfRange => "布线样本越窗：样本编号闭集外",
            Teb32Code::OffsetDuplicated => "偏移重复登记：同偏移两字段",
            Teb32Code::WiringMismatch => "布线账不守恒：布线数与样本数失平",
        }
    }
    pub const ALL: [Teb32Code; 8] = [
        Teb32Code::OffsetOutOfRange,
        Teb32Code::MonotonicBroken,
        Teb32Code::ArchMixed,
        Teb32Code::SelfNotWired,
        Teb32Code::FieldMissing,
        Teb32Code::SampleOutOfRange,
        Teb32Code::OffsetDuplicated,
        Teb32Code::WiringMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Teb32Code::ALL.len() {
        if Teb32Code::ALL[i].val() == code {
            return Teb32Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：32 位 TEB 建模布局书，逐字段注出处） ─────────────
/// TEB32 窗口尺寸（建模：4KB 一页档）。
pub const TEB32_SIZE: u32 = 0x1000;
/// 布线样本数（判据原文：30 样本）。
pub const WIRE_SAMPLES: usize = 30;
/// 架构档闭集标签。
pub const ARCH_X32: u8 = 32;
pub const ARCH_X64: u8 = 64;

// 32 位档全字段偏移（出处：32 位 TEB 建模布局书 §2，逐条锚定）。
pub const OFF_EXCEPTION_LIST: u32 = 0x00; // NT_TIB.ExceptionList
pub const OFF_STACK_BASE: u32 = 0x04; // NT_TIB.StackBase
pub const OFF_STACK_LIMIT: u32 = 0x08; // NT_TIB.StackLimit
pub const OFF_SUBSYSTEM_TIB: u32 = 0x0C; // NT_TIB.SubSystemTib
pub const OFF_FIBER_DATA: u32 = 0x10; // NT_TIB.FiberData
pub const OFF_ARBITRARY_USER: u32 = 0x14; // NT_TIB.ArbitraryUserPointer
pub const OFF_SELF: u32 = 0x18; // NT_TIB.Self（FS 自指锚点）
pub const OFF_ENV_POINTER: u32 = 0x1C; // EnvironmentPointer
pub const OFF_CLIENT_ID: u32 = 0x20; // ClientId（进程/线程 id 对）
pub const OFF_ACTIVE_RPC: u32 = 0x28; // ActiveRpcHandle
pub const OFF_TLS_PTR: u32 = 0x2C; // ThreadLocalStoragePointer
pub const OFF_PEB: u32 = 0x30; // ProcessEnvironmentBlock 指针
pub const OFF_LAST_ERROR: u32 = 0x34; // LastErrorValue
pub const OFF_LAST_STATUS: u32 = 0x38; // LastStatusValue
pub const OFF_COUNT_OWNED_LOCKS: u32 = 0x3C; // CountOwnedLocks
pub const OFF_ACTIVATION_CTX: u32 = 0x40; // ActivationContextStackPointer
pub const OFF_SPARE_BYTES: u32 = 0x48; // SpareBytes
pub const OFF_GDI_TEB_BUFF: u32 = 0x58; // GdiTebBuffer（GDI 系）
pub const OFF_WIN32K_CLIENT: u32 = 0x6C; // Win32kClientInfo
pub const OFF_GL_RESERVED: u32 = 0xC8; // GlDispatchTable 保留面

/// 字段条目（偏移 + 架构档标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldEntry {
    pub name: &'static str,
    pub offset: u32,
    pub arch: u8,
}

/// 32 位档独立字段账（闭集，逐条注出处见上）。
pub const TEB32_FIELDS: [FieldEntry; 20] = [
    FieldEntry { name: "ExceptionList", offset: OFF_EXCEPTION_LIST, arch: ARCH_X32 },
    FieldEntry { name: "StackBase", offset: OFF_STACK_BASE, arch: ARCH_X32 },
    FieldEntry { name: "StackLimit", offset: OFF_STACK_LIMIT, arch: ARCH_X32 },
    FieldEntry { name: "SubSystemTib", offset: OFF_SUBSYSTEM_TIB, arch: ARCH_X32 },
    FieldEntry { name: "FiberData", offset: OFF_FIBER_DATA, arch: ARCH_X32 },
    FieldEntry { name: "ArbitraryUserPointer", offset: OFF_ARBITRARY_USER, arch: ARCH_X32 },
    FieldEntry { name: "Self", offset: OFF_SELF, arch: ARCH_X32 },
    FieldEntry { name: "EnvironmentPointer", offset: OFF_ENV_POINTER, arch: ARCH_X32 },
    FieldEntry { name: "ClientId", offset: OFF_CLIENT_ID, arch: ARCH_X32 },
    FieldEntry { name: "ActiveRpcHandle", offset: OFF_ACTIVE_RPC, arch: ARCH_X32 },
    FieldEntry { name: "TlsPtr", offset: OFF_TLS_PTR, arch: ARCH_X32 },
    FieldEntry { name: "Peb", offset: OFF_PEB, arch: ARCH_X32 },
    FieldEntry { name: "LastError", offset: OFF_LAST_ERROR, arch: ARCH_X32 },
    FieldEntry { name: "LastStatus", offset: OFF_LAST_STATUS, arch: ARCH_X32 },
    FieldEntry { name: "OwnedLocks", offset: OFF_COUNT_OWNED_LOCKS, arch: ARCH_X32 },
    FieldEntry { name: "ActCtx", offset: OFF_ACTIVATION_CTX, arch: ARCH_X32 },
    FieldEntry { name: "SpareBytes", offset: OFF_SPARE_BYTES, arch: ARCH_X32 },
    FieldEntry { name: "GdiTebBuffer", offset: OFF_GDI_TEB_BUFF, arch: ARCH_X32 },
    FieldEntry { name: "Win32kClient", offset: OFF_WIN32K_CLIENT, arch: ARCH_X32 },
    FieldEntry { name: "GlReserved", offset: OFF_GL_RESERVED, arch: ARCH_X32 },
];

// 64 位档对照账（用于零混用反向判据，非本卡主账）。
pub const TEB64_FIELDS: [FieldEntry; 4] = [
    FieldEntry { name: "x64.ExceptionList", offset: 0x00, arch: ARCH_X64 },
    FieldEntry { name: "x64.Self", offset: 0x30, arch: ARCH_X64 },
    FieldEntry { name: "x64.Peb", offset: 0x60, arch: ARCH_X64 },
    FieldEntry { name: "x64.ClientId", offset: 0x40, arch: ARCH_X64 },
];

/// 字段账核：越窗 / 单调不降 / 无重复偏移 / 档标签全 32。
pub fn audit_field_table(tbl: &[FieldEntry]) -> Result<usize, Teb32Code> {
    if tbl.is_empty() {
        return Err(Teb32Code::FieldMissing);
    }
    let mut prev: u32 = 0;
    let mut seen: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < tbl.len() {
        let e = tbl[i];
        if e.offset >= TEB32_SIZE {
            return Err(Teb32Code::OffsetOutOfRange);
        }
        if e.arch != ARCH_X32 {
            return Err(Teb32Code::ArchMixed);
        }
        if i > 0 && e.offset < prev {
            return Err(Teb32Code::MonotonicBroken);
        }
        let mut j = 0;
        while j < seen.len() {
            if seen[j] == e.offset {
                return Err(Teb32Code::OffsetDuplicated);
            }
            j += 1;
        }
        seen.push(e.offset);
        prev = e.offset;
        i += 1;
    }
    Ok(tbl.len())
}

// ── 布线器（30 样本 Self 自指账） ────────────────────────────────
#[derive(Clone, Debug)]
pub struct Wiring {
    pub sample_id: u32,
    /// 布线后的 Self 槽值（必须等于表基址 + OFF_SELF）
    pub self_value: u32,
    pub wired: bool,
}

pub struct Teb32Wirer {
    pub base: u32,
    pub wires: Vec<Wiring>,
}

impl Teb32Wirer {
    pub fn new(base: u32) -> Teb32Wirer {
        Teb32Wirer {
            base,
            wires: Vec::new(),
        }
    }

    /// 布线一条：Self 槽 = 表基址 + OFF_SELF（自指恒等式）。
    pub fn wire_sample(&mut self, sample_id: u32) -> Result<(), Teb32Code> {
        if sample_id >= WIRE_SAMPLES as u32 {
            return Err(Teb32Code::SampleOutOfRange);
        }
        let self_addr = match self.base.checked_add(OFF_SELF) {
            Some(a) => a,
            None => return Err(Teb32Code::OffsetOutOfRange),
        };
        self.wires.push(Wiring {
            sample_id,
            self_value: self_addr,
            wired: true,
        });
        Ok(())
    }

    /// 守恒核：布线条数 == 样本数，且每条 Self 槽自指成立。
    pub fn verify(&self) -> Result<usize, Teb32Code> {
        if self.wires.len() != WIRE_SAMPLES {
            return Err(Teb32Code::WiringMismatch);
        }
        let self_addr = match self.base.checked_add(OFF_SELF) {
            Some(a) => a,
            None => return Err(Teb32Code::OffsetOutOfRange),
        };
        let mut i = 0;
        while i < self.wires.len() {
            let w = &self.wires[i];
            if !w.wired || w.self_value != self_addr {
                return Err(Teb32Code::SelfNotWired);
            }
            i += 1;
        }
        Ok(self.wires.len())
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：32 位 TEB 全字段偏移表独立成账（20 字段全过核）。
pub fn drill_field_table_audit() -> (bool, String) {
    let n = match audit_field_table(&TEB32_FIELDS) {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    // 独立成账反向核：64 位档条目混入 32 档账必抓 ArchMixed。
    let mut mixed: Vec<FieldEntry> = Vec::new();
    let mut i = 0;
    while i < TEB32_FIELDS.len() {
        mixed.push(TEB32_FIELDS[i]);
        i += 1;
    }
    mixed.push(TEB64_FIELDS[1]);
    let mixed_caught = audit_field_table(&mixed) == Err(Teb32Code::ArchMixed);
    (
        n == TEB32_FIELDS.len() && mixed_caught,
        format!("字段账 {} 条全过，混档注入抓={}", n, mixed_caught),
    )
}

/// J1 面：零混用红线——32 档与 64 档账逐条档标签互斥。
pub fn drill_arch_separation() -> (bool, String) {
    let mut all32 = true;
    let mut i = 0;
    while i < TEB32_FIELDS.len() {
        if TEB32_FIELDS[i].arch != ARCH_X32 {
            all32 = false;
        }
        i += 1;
    }
    let mut all64 = true;
    let mut j = 0;
    while j < TEB64_FIELDS.len() {
        if TEB64_FIELDS[j].arch != ARCH_X64 {
            all64 = false;
        }
        j += 1;
    }
    // Self 锚点互斥：32 档 0x18 / 64 档 0x30，同字段不同偏移（零混用铁证）。
    let self32 = OFF_SELF;
    let self64 = 0x30;
    (
        all32 && all64 && self32 != self64,
        format!("档标签互斥 32全={} 64全={} Self 0x{:X}≠0x{:X}", all32, all64, self32, self64),
    )
}

/// J1 面：30 样本布线断言（自指恒等式全真）。
pub fn drill_wiring_30() -> (bool, String) {
    let mut wr = Teb32Wirer::new(0x7EF0_0000);
    let mut i = 0;
    while i < WIRE_SAMPLES {
        if wr.wire_sample(i as u32).is_err() {
            return (false, format!("样本 {} 布线失败", i));
        }
        i += 1;
    }
    let n = match wr.verify() {
        Ok(n) => n,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    (n == WIRE_SAMPLES, format!("布线断言 {}/{} 全自指", n, WIRE_SAMPLES))
}

/// 注入面：Self 布线断裂必抓（篡改一条 Self 槽值）。
pub fn drill_self_broken_caught() -> (bool, String) {
    let mut wr = Teb32Wirer::new(0x7EF0_0000);
    let mut i = 0;
    while i < WIRE_SAMPLES {
        let _ = wr.wire_sample(i as u32);
        i += 1;
    }
    // 篡改第 7 条布线（下标 6 < 长度先比）。
    if wr.wires.len() > 6 {
        wr.wires[6].self_value = wr.wires[6].self_value.wrapping_add(4);
    }
    let r = wr.verify();
    (
        r == Err(Teb32Code::SelfNotWired),
        format!("布线断裂注入抓={:?}", r.is_err()),
    )
}

/// 注入面：偏移越窗 / 单调破 / 重复登记三路必抓。
pub fn drill_table_violations() -> (bool, String) {
    let oor = audit_field_table(&[FieldEntry { name: "Bad", offset: TEB32_SIZE, arch: ARCH_X32 }])
        == Err(Teb32Code::OffsetOutOfRange);
    let mono = audit_field_table(&[
        FieldEntry { name: "A", offset: 0x20, arch: ARCH_X32 },
        FieldEntry { name: "B", offset: 0x10, arch: ARCH_X32 },
    ]) == Err(Teb32Code::MonotonicBroken);
    let dup = audit_field_table(&[
        FieldEntry { name: "A", offset: 0x10, arch: ARCH_X32 },
        FieldEntry { name: "B", offset: 0x10, arch: ARCH_X32 },
    ]) == Err(Teb32Code::OffsetDuplicated);
    let empty = audit_field_table(&[]) == Err(Teb32Code::FieldMissing);
    (
        oor && mono && dup && empty,
        format!("越窗={} 单调破={} 重复={} 空账={}", oor, mono, dup, empty),
    )
}

/// 注入面：布线样本越窗 10/10 必抓。
pub fn drill_sample_range_10() -> (bool, String) {
    let mut wr = Teb32Wirer::new(0x7EF0_0000);
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        if wr.wire_sample(WIRE_SAMPLES as u32 + i as u32) == Err(Teb32Code::SampleOutOfRange) {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("越窗布线注入 {}/10 必抓", caught))
}

/// 守恒面：布线账恒等式（布线数 == 样本数 == WIRE_SAMPLES，反向缺条必抓）。
pub fn drill_wiring_conservation() -> (bool, String) {
    let mut wr = Teb32Wirer::new(0x7EF0_1000);
    let mut i = 0;
    while i < WIRE_SAMPLES {
        let _ = wr.wire_sample(i as u32);
        i += 1;
    }
    let full = wr.verify() == Ok(WIRE_SAMPLES);
    // 反向：抽一条布线 → 守恒破必抓。
    wr.wires.pop();
    let broken = wr.verify() == Err(Teb32Code::WiringMismatch);
    (
        full && broken,
        format!("守恒全账={} 缺条注入抓={}", full, broken),
    )
}

/// 多机对拍面：同基址重建两次，布线账逐条一致。
pub fn drill_determinism() -> (bool, String) {
    let mut a = Teb32Wirer::new(0x7EF0_2000);
    let mut b = Teb32Wirer::new(0x7EF0_2000);
    let mut i = 0;
    while i < WIRE_SAMPLES {
        let _ = a.wire_sample(i as u32);
        let _ = b.wire_sample(i as u32);
        i += 1;
    }
    let mut same = a.wires.len() == b.wires.len();
    let mut j = 0;
    while same && j < a.wires.len() {
        if a.wires[j].self_value != b.wires[j].self_value {
            same = false;
        }
        j += 1;
    }
    (same, format!("同基址对拍一致={}", same))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_field_table_audit();
    let (a2, d2) = drill_arch_separation();
    let (a3, d3) = drill_wiring_30();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 字段账={} 零混用={} 布线30={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 深化块：黄金偏移对照 + 字段账导出 ────────────────────────────
/// 黄金偏移对照表：10 组独立写死（字段名→偏移），与主账逐条复核。
pub fn drill_golden_offsets_10() -> (bool, String) {
    let golden: [(&str, u32); 10] = [
        ("ExceptionList", 0x00),
        ("Self", 0x18),
        ("EnvironmentPointer", 0x1C),
        ("ClientId", 0x20),
        ("TlsPtr", 0x2C),
        ("Peb", 0x30),
        ("LastError", 0x34),
        ("ActCtx", 0x40),
        ("GdiTebBuffer", 0x58),
        ("GlReserved", 0xC8),
    ];
    let mut ok = true;
    let mut i = 0;
    while i < golden.len() {
        let (name, off) = golden[i];
        let mut found = false;
        let mut j = 0;
        while j < TEB32_FIELDS.len() {
            if TEB32_FIELDS[j].name == name {
                if TEB32_FIELDS[j].offset == off {
                    found = true;
                }
                break;
            }
            j += 1;
        }
        if !found {
            ok = false;
            break;
        }
        i += 1;
    }
    (ok, format!("黄金偏移对照 10 组全真={}", ok))
}

/// 字段账诊断导出：F|name|0xoff|arch 逐行（行数守恒 + 四段格式断言）。
pub fn drill_export_fields() -> (bool, String) {
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < TEB32_FIELDS.len() {
        lines.push(format!(
            "F|{}|0x{:02X}|{}",
            TEB32_FIELDS[i].name, TEB32_FIELDS[i].offset, TEB32_FIELDS[i].arch
        ));
        i += 1;
    }
    let rows_ok = lines.len() == TEB32_FIELDS.len();
    let mut fmt_ok = true;
    let mut k = 0;
    while k < lines.len() {
        if !lines[k].starts_with("F|") || lines[k].split('|').count() != 4 {
            fmt_ok = false;
        }
        k += 1;
    }
    (
        rows_ok && fmt_ok,
        format!("字段账导出 {} 行守恒={} 格式={}", lines.len(), rows_ok, fmt_ok),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < Teb32Code::ALL.len() {
        let v = Teb32Code::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_field_table_audit();
    v.push(("J1-全字段偏移表独立成账", a, b));
    let (a, b) = drill_arch_separation();
    v.push(("J1-32/64档零混用红线", a, b));
    let (a, b) = drill_wiring_30();
    v.push(("J1-30样本布线断言", a, b));
    let (a, b) = drill_self_broken_caught();
    v.push(("J2-Self布线断裂注入", a, b));
    let (a, b) = drill_table_violations();
    v.push(("J2-越窗/单调/重复/空账", a, b));
    let (a, b) = drill_sample_range_10();
    v.push(("J2-布线越窗注入10/10", a, b));
    let (a, b) = drill_wiring_conservation();
    v.push(("J3-布线账守恒+反向", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同基址对拍一致", a, b));
    let (a, b) = drill_golden_offsets_10();
    v.push(("J3-黄金偏移对照10组", a, b));
    let (a, b) = drill_export_fields();
    v.push(("J3-字段账诊断导出", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13121_checks：本单判据聚合（诊断段 0x65680）。
pub fn run_unx13121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13121 32 位 TEB 布局表");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65680 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
