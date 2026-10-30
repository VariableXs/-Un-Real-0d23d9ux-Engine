//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15721 · ITypeLib 面谱（四核成文 + 越界索引注入全抓） · 0x67700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15721-J1 面谱成文（GetTypeInfoCount/GetTypeInfo/
//!      GetTypeInfoOfGuid/GetLibAttr 四核 10/10 次），越界索引注入
//!      （超 Count 取型）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四核闭面：Count / GetTypeInfo / GetTypeInfoOfGuid / GetLibAttr；
//!   - 型面账本：GUID 表 + 成员数 + 五种型别闭集，全部写死；
//!   - 正演：每核 10 次调用全部成功（10/10），账本逐次落账；
//!   - 注入三族：越界索引 / GUID 查无 / 闭库调用，各 10 次 10/10 抓；
//!   - 状态机：Closed → Open → Probed → Sealed，非法边拒收；
//!   - LCG 种子写死 = SEG_BASE（0x67700），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67700，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67700;

/// 每核正演次数（判据钉版：10/10）。
pub const RUNS_PER_CORE: u32 = 10;
/// 四核闭集基数。
pub const CORE_COUNT: usize = 4;
pub const CORE_NAMES: [&str; CORE_COUNT] = [
    "GetTypeInfoCount",
    "GetTypeInfo",
    "GetTypeInfoOfGuid",
    "GetLibAttr",
];

/// 型别五族闭集（ITypeLib 可挂型面）。
pub const KIND_ENUM: usize = 0;
pub const KIND_STRUCT: usize = 1;
pub const KIND_INTERFACE: usize = 2;
pub const KIND_DISPATCH: usize = 3;
pub const KIND_MODULE: usize = 4;
pub const KIND_COUNT: usize = 5;
pub const KIND_NAMES: [&str; KIND_COUNT] = [
    "枚举型", "结构型", "接口型", "分发型", "模块型",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacadeCode {
    /// 索引越界（超 GetTypeInfoCount）
    IndexOutOfRange,
    /// GUID 查无此型
    GuidNotFound,
    /// 库未开启即调用（相位非法）
    LibNotOpen,
    /// 库属性缺失（GetLibAttr 无账）
    LibAttrMissing,
    /// 型别越闭集
    KindOutOfRange,
    /// 状态机非法边
    IllegalPhaseEdge,
    /// 正演账未满（<10/10）
    RunLedgerShort,
    /// 双跑不一致
    NotReproducible,
    /// 面谱未封印
    FacadeUnsealed,
}

impl FacadeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FacadeCode::IndexOutOfRange => "索引越界：超 GetTypeInfoCount 取型",
            FacadeCode::GuidNotFound => "GUID 查无此型：GetTypeInfoOfGuid 落空",
            FacadeCode::LibNotOpen => "库未开启即调用：相位非法边",
            FacadeCode::LibAttrMissing => "库属性缺失：GetLibAttr 无账可返",
            FacadeCode::KindOutOfRange => "型别越闭集：五族之外",
            FacadeCode::IllegalPhaseEdge => "状态机非法边：Closed→Sealed 直跳等",
            FacadeCode::RunLedgerShort => "正演账未满：四核须 10/10",
            FacadeCode::NotReproducible => "双跑不一致：面谱复现红线",
            FacadeCode::FacadeUnsealed => "面谱未封印：Sealed 相位未达",
        }
    }
    pub const ALL: [FacadeCode; 9] = [
        FacadeCode::IndexOutOfRange,
        FacadeCode::GuidNotFound,
        FacadeCode::LibNotOpen,
        FacadeCode::LibAttrMissing,
        FacadeCode::KindOutOfRange,
        FacadeCode::IllegalPhaseEdge,
        FacadeCode::RunLedgerShort,
        FacadeCode::NotReproducible,
        FacadeCode::FacadeUnsealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FacadeCode::ALL.len() {
        if FacadeCode::ALL[i].val() == code {
            return FacadeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67700 段闭集内"
}

// ── 库属性与型面条目（来源：ITypeLib 语义推定，写死） ────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LibAttr {
    /// 系统字长代号：1=16 位 2=32 位 3=64 位
    pub syskind: u32,
    pub major: u16,
    pub minor: u16,
    pub flags: u32,
}

/// 型面条目：GUID + 型别 + 成员数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypeInfoEntry {
    pub guid: [u8; 16],
    pub kind: usize,
    pub name: &'static str,
    pub member_count: u32,
}

/// 种子 GUID 表（8 条，写死：byte 序列为人工编制的确定性值）。
pub const SEED_TABLE: [TypeInfoEntry; 8] = [
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01], kind: KIND_ENUM, name: "EVarKind", member_count: 12 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02], kind: KIND_STRUCT, name: "SPanel", member_count: 9 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03], kind: KIND_INTERFACE, name: "IPano", member_count: 21 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04], kind: KIND_DISPATCH, name: "DPano", member_count: 21 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05], kind: KIND_MODULE, name: "MPanorama", member_count: 7 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06], kind: KIND_ENUM, name: "ELockKind", member_count: 4 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x07], kind: KIND_INTERFACE, name: "ILockGate", member_count: 15 },
    TypeInfoEntry { guid: [0xA1, 0x57, 0x21, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08], kind: KIND_STRUCT, name: "SLockCell", member_count: 6 },
];

/// 缺席 GUID（注入用，写死：与种子表全量不同）。
pub const ABSENT_GUID: [u8; 16] = [0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF];

// ── 状态机：Closed → Open → Probed → Sealed ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacadePhase {
    Closed,
    Open,
    Probed,
    Sealed,
}

/// 合法边闭集：Closed→Open、Open→Probed、Probed→Sealed；其余皆非法。
pub fn phase_edge_legal(from: FacadePhase, to: FacadePhase) -> bool {
    match (from, to) {
        (FacadePhase::Closed, FacadePhase::Open) => true,
        (FacadePhase::Open, FacadePhase::Probed) => true,
        (FacadePhase::Probed, FacadePhase::Sealed) => true,
        _ => false,
    }
}

// ── 型面库模型 ───────────────────────────────────────────────────
pub struct TypeLibModel {
    pub entries: Vec<TypeInfoEntry>,
    pub lib_attr: Option<LibAttr>,
    pub phase: FacadePhase,
    /// 四核正演账：每核成功次数。
    pub core_ok: [u32; CORE_COUNT],
    /// 四核调用总次数（成功 + 拒收）。
    pub core_calls: [u32; CORE_COUNT],
}

impl TypeLibModel {
    pub fn new() -> TypeLibModel {
        let mut entries: Vec<TypeInfoEntry> = Vec::new();
        let mut i = 0;
        while i < SEED_TABLE.len() {
            entries.push(SEED_TABLE[i]);
            i += 1;
        }
        TypeLibModel {
            entries,
            lib_attr: None,
            phase: FacadePhase::Closed,
            core_ok: [0; CORE_COUNT],
            core_calls: [0; CORE_COUNT],
        }
    }

    /// 相位推进：非法边拒收。
    pub fn advance(&mut self, to: FacadePhase) -> Result<(), u32> {
        let from = self.phase;
        if !phase_edge_legal(from, to) {
            return Err(FacadeCode::IllegalPhaseEdge.val());
        }
        self.phase = to;
        Ok(())
    }

    /// 开库：装填库属性（写死：32 位系统 / 1.0 版 / 标志 0x2）。
    pub fn open(&mut self) -> Result<(), u32> {
        self.advance(FacadePhase::Open)?;
        self.lib_attr = Some(LibAttr { syskind: 2, major: 1, minor: 0, flags: 0x2 });
        Ok(())
    }

    /// 核 0：GetTypeInfoCount（相位须 Open 以上）。
    pub fn get_type_info_count(&mut self) -> Result<usize, u32> {
        self.core_calls[0] += 1;
        if self.phase == FacadePhase::Closed {
            return Err(FacadeCode::LibNotOpen.val());
        }
        self.core_ok[0] += 1;
        Ok(self.entries.len())
    }

    /// 核 1：GetTypeInfo（按索引取型面；越界即拒）。
    pub fn get_type_info(&mut self, index: usize) -> Result<(usize, usize), u32> {
        self.core_calls[1] += 1;
        if self.phase == FacadePhase::Closed {
            return Err(FacadeCode::LibNotOpen.val());
        }
        // 下标先比长度（铁律）。
        if index >= self.entries.len() {
            return Err(FacadeCode::IndexOutOfRange.val());
        }
        let kind = self.entries[index].kind;
        if kind >= KIND_COUNT {
            return Err(FacadeCode::KindOutOfRange.val());
        }
        self.core_ok[1] += 1;
        Ok((index, kind))
    }

    /// 核 2：GetTypeInfoOfGuid（线性扫 GUID，逐字节比对）。
    pub fn get_type_info_of_guid(&mut self, guid: &[u8; 16]) -> Result<(usize, usize), u32> {
        self.core_calls[2] += 1;
        if self.phase == FacadePhase::Closed {
            return Err(FacadeCode::LibNotOpen.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            let mut same = true;
            let mut b = 0;
            while b < 16 {
                if self.entries[i].guid[b] != guid[b] {
                    same = false;
                }
                b += 1;
            }
            if same {
                self.core_ok[2] += 1;
                return Ok((i, self.entries[i].kind));
            }
            i += 1;
        }
        Err(FacadeCode::GuidNotFound.val())
    }

    /// 核 3：GetLibAttr（库属性出账）。
    pub fn get_lib_attr(&mut self) -> Result<LibAttr, u32> {
        self.core_calls[3] += 1;
        if self.phase == FacadePhase::Closed {
            return Err(FacadeCode::LibNotOpen.val());
        }
        match self.lib_attr {
            Some(a) => {
                self.core_ok[3] += 1;
                Ok(a)
            }
            None => Err(FacadeCode::LibAttrMissing.val()),
        }
    }

    /// 四核正演：每核 RUNS_PER_CORE 次，全成功才落 Probed。
    pub fn probe_all_cores(&mut self) -> Result<(), u32> {
        let count = self.get_type_info_count()?;
        let mut run = 0u32;
        while run < RUNS_PER_CORE {
            // 确定性选型：LCG 取合法索引与 GUID。
            let mut g = Lcg::new(SEG_BASE as u64 + run as u64);
            let idx = (g.below(count as u64)) as usize;
            let (ri, _) = self.get_type_info(idx)?;
            if ri != idx {
                return Err(FacadeCode::IllegalPhaseEdge.val());
            }
            let guid_copy = self.entries[idx].guid;
            let (gi, _) = self.get_type_info_of_guid(&guid_copy)?;
            if gi != idx {
                return Err(FacadeCode::GuidNotFound.val());
            }
            let _ = self.get_lib_attr()?;
            run += 1;
        }
        // 账要满：四核各 10/10。
        let mut c = 0;
        while c < CORE_COUNT {
            if self.core_ok[c] < RUNS_PER_CORE {
                return Err(FacadeCode::RunLedgerShort.val());
            }
            c += 1;
        }
        self.advance(FacadePhase::Probed)?;
        Ok(())
    }

    /// 封印：Probed → Sealed。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance(FacadePhase::Sealed)
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["越界索引族", "GUID查无族", "闭库调用族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_CORE {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：越界索引注入（idx = count + k，k=0..9）→ IndexOutOfRange。
    let mut lib0 = TypeLibModel::new();
    let _ = lib0.open();
    let count = lib0.entries.len();
    let mut k = 0u32;
    while k < RUNS_PER_CORE {
        ledger.tries[0] += 1;
        if let Err(e) = lib0.get_type_info(count + k as usize) {
            if e == FacadeCode::IndexOutOfRange.val() {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：GUID 查无注入（缺席 GUID ×10）→ GuidNotFound。
    let mut lib1 = TypeLibModel::new();
    let _ = lib1.open();
    k = 0;
    while k < RUNS_PER_CORE {
        ledger.tries[1] += 1;
        if let Err(e) = lib1.get_type_info_of_guid(&ABSENT_GUID) {
            if e == FacadeCode::GuidNotFound.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：闭库调用注入（Closed 相位取型 ×10）→ LibNotOpen。
    let mut lib2 = TypeLibModel::new();
    k = 0;
    while k < RUNS_PER_CORE {
        ledger.tries[2] += 1;
        if let Err(e) = lib2.get_type_info(0) {
            if e == FacadeCode::LibNotOpen.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：开库→四核 10/10→封印→读回 ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub count: usize,
    pub core_ok: [u32; CORE_COUNT],
    pub sealed: bool,
    pub attr_ok: bool,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut lib = TypeLibModel::new();
    lib.open()?;
    let count = lib.get_type_info_count()?;
    lib.probe_all_cores()?;
    let attr = lib.get_lib_attr()?;
    let attr_ok = attr.syskind == 2 && attr.major == 1;
    lib.seal()?;
    let sealed = lib.phase == FacadePhase::Sealed;
    let core_ok = lib.core_ok;
    if !sealed {
        return Err(FacadeCode::FacadeUnsealed.val());
    }
    Ok(GoldenResult { count, core_ok, sealed, attr_ok })
}

/// 双跑复现：同输入两次黄金正演逐项一致。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.count != b.count || a.sealed != b.sealed || a.attr_ok != b.attr_ok {
        return Err(FacadeCode::NotReproducible.val());
    }
    let mut i = 0;
    while i < CORE_COUNT {
        if a.core_ok[i] != b.core_ok[i] {
            return Err(FacadeCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FacadeCode::ALL.len() {
        let v = FacadeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四核 10/10 + 封印达成。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            let mut full = g.sealed && g.attr_ok && g.count == SEED_TABLE.len();
            let mut c = 0;
            while c < CORE_COUNT {
                if g.core_ok[c] != RUNS_PER_CORE {
                    full = false;
                }
                c += 1;
            }
            full
        }
        Err(_) => false,
    };
    v.push((
        "J1-四核正演10/10+封印",
        golden_full,
        format!("{:?}", golden),
    ));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族10/10全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 状态机非法边拒收：Closed→Sealed 直跳必拒。
    let mut bad_edge_lib = TypeLibModel::new();
    let bad_edge = if let Err(e) = bad_edge_lib.advance(FacadePhase::Sealed) {
        e == FacadeCode::IllegalPhaseEdge.val()
    } else {
        false
    };
    v.push(("J1-非法相位边拒收", bad_edge, String::from("Closed→Sealed 必拒")));
    // 5. 型别五族闭集名点验 + 种子表逐条型别合法。
    let mut kind_ok = true;
    let mut i = 0;
    while i < SEED_TABLE.len() {
        if SEED_TABLE[i].kind >= KIND_COUNT || KIND_NAMES[SEED_TABLE[i].kind].len() == 0 {
            kind_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-型别五族闭集点验",
        kind_ok,
        format!("{}/8 条型别合法", SEED_TABLE.len()),
    ));
    // 6. 越界索引反向判据：合法索引必成（下标先比长度的补面）。
    let mut rev_ok = true;
    let mut lib3 = TypeLibModel::new();
    let _ = lib3.open();
    let mut idx = 0;
    while idx < lib3.entries.len() {
        if let Err(e) = lib3.get_type_info(idx) {
            if e == FacadeCode::IndexOutOfRange.val() {
                rev_ok = false;
            }
        }
        idx += 1;
    }
    v.push(("J1-反向判据合法索引必成", rev_ok, format!("0..{} 全成", idx)));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67700 窗口")));
    v
}

pub fn run_unx15721_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B27 ITypeLib 面谱（四核 10/10 + 越界注入 10/10）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
