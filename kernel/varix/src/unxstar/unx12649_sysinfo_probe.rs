//! UNX-F12650 NtQuerySystemInformation 探测面 · 0x80C40段
//!
//! 判据原文（派工卡 12649 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12650-J1 类型枚举探测对平断言绿（B12 类型清单回指），
//!      枚举完整性断言 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 探测面域模型：一次探测调用 =（信息类号 / 缓冲区账长 / 实回长度 /
//!    状态码）四元；状态码取整数闭集（0 成功 / 0xC0000004 长度不匹 /
//!    0xC000000D 参数无效 / 0xC0000003 类号无效，公开 NTSTATUS 结论，
//!    只引结论不抄码）；
//! 2. B12 类型清单回指：冻结 10 行对象类型清单（Process/Thread/Token/
//!    Event/Mutant/Semaphore/Section/File/Port/Key），每行带 B12 回指标记
//!    （锚号 = 行号），探测枚举必须逐行复现清单——名称/顺序/行数三对齐；
//! 3. 探测语料：枚举快照按清单行序生成（类型索引/类型名/句柄计数/
//!    探测状态/实回长度），语料行数恒等清单行数，任何缺行、换序、
//!    改名、清零都在完整性断言处翻红；
//! 4. 枚举完整性断言 10/10：清单行数、回指标记齐、名称非空、无重复名、
//!    类型索引单调、状态码全成功、句柄计数非零、语料行数恒等、名称对平、
//!    回长记账齐——十条独立写死，任何一条为假即红；
//! 5. 注入联测 10 路：十路语料突变（缺行/换序/改名/重名/索引乱/状态坏/
//!    句柄清零/回长清零/回指标记缺/多余行）逐路必须被完整性断言抓住，
//!    放行即判据失效；
//! 6. 状态机 待探测→已枚举→已对平 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12649 − 12600)）。
pub const SEG_BASE: u32 = 0x80C40;

// ── 探测面诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeDiag {
    /// 信息类号越探测闭集。
    EClassRange,
    /// 探测参数无效（缓冲区账长为零）。
    EParam,
    /// 探测状态码越状态闭集。
    EStatusRange,
    /// 语料缺行（枚举行数少于清单行数）。
    ERowMiss,
    /// 语料多余行（枚举行数多于清单行数）。
    ERowExtra,
    /// 语料换序（类型索引乱序）。
    EIndexOrder,
    /// 类型名对平失败（与 B12 清单不一致）。
    ENameMismatch,
    /// 类型名重复。
    ENameDup,
    /// 回指标记缺失（B12 锚号对不上）。
    EAnchorMiss,
    /// 未枚举就试图对平。
    ENotEnumerated,
    /// 对平后仍试图重枚举（状态机倒退）。
    ERewind,
    /// 完整性断言未全过。
    EIntegrity,
}

impl ProbeDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ProbeDiag::EClassRange => 0x01,
                ProbeDiag::EParam => 0x02,
                ProbeDiag::EStatusRange => 0x03,
                ProbeDiag::ERowMiss => 0x04,
                ProbeDiag::ERowExtra => 0x05,
                ProbeDiag::EIndexOrder => 0x06,
                ProbeDiag::ENameMismatch => 0x07,
                ProbeDiag::ENameDup => 0x08,
                ProbeDiag::EAnchorMiss => 0x09,
                ProbeDiag::ENotEnumerated => 0x0a,
                ProbeDiag::ERewind => 0x0b,
                ProbeDiag::EIntegrity => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ProbeDiag::EClassRange => "信息类号越探测闭集：只认本面登记的系统查询类，越类先对 B12 清单回指表",
            ProbeDiag::EParam => "探测参数无效：缓冲区账长为零不构成一次可记账探测，先补账长再发起",
            ProbeDiag::EStatusRange => "状态码越状态闭集：只认 0/0xC0000004/0xC000000D/0xC0000003 四值，先修状态再入语料",
            ProbeDiag::ERowMiss => "语料缺行：枚举行数少于 B12 清单行数，先补齐缺行再对平",
            ProbeDiag::ERowExtra => "语料多余行：枚举行数多于清单行数，多余行不构成清单成员，剔除后再对平",
            ProbeDiag::EIndexOrder => "类型索引乱序：枚举按清单序 0..10 逐行产出，跳行属乱序，回补后重枚举",
            ProbeDiag::ENameMismatch => "类型名对平失败：枚举名与 B12 清单名不一致，先核名再对平",
            ProbeDiag::ENameDup => "类型名重复：每类型只落一笔，重复名先查清单闭集",
            ProbeDiag::EAnchorMiss => "回指标记缺失：B12 锚号对不上，补锚号后再对平",
            ProbeDiag::ENotEnumerated => "未枚举禁止对平：先发起探测并把语料落账，再走对平口",
            ProbeDiag::ERewind => "状态机倒退：已对平后重枚举被拒，改动走下一轮探测滚动",
            ProbeDiag::EIntegrity => "完整性断言未全过：10/10 有红项，按红项编号回修对应语料行",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const PROBE_ALL: &[ProbeDiag] = &[
    ProbeDiag::EClassRange,
    ProbeDiag::EParam,
    ProbeDiag::EStatusRange,
    ProbeDiag::ERowMiss,
    ProbeDiag::ERowExtra,
    ProbeDiag::EIndexOrder,
    ProbeDiag::ENameMismatch,
    ProbeDiag::ENameDup,
    ProbeDiag::EAnchorMiss,
    ProbeDiag::ENotEnumerated,
    ProbeDiag::ERewind,
    ProbeDiag::EIntegrity,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in PROBE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80C41..0x80C4C 闭集内"
}

// ── B12 类型清单回指闭集 ─────────────────────────────────────────
/// 清单行：类型名 + B12 回指标记（锚号 = 行号，判据语境冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B12TypeRow {
    /// 类型名（非空）。
    pub name: &'static str,
    /// B12 回指标记（锚号）。
    pub anchor: u32,
}

/// B12 类型清单（10 行，行序冻结不许重排）。
pub const B12_TYPES: &[B12TypeRow] = &[
    B12TypeRow { name: "Process", anchor: 0 },
    B12TypeRow { name: "Thread", anchor: 1 },
    B12TypeRow { name: "Token", anchor: 2 },
    B12TypeRow { name: "Event", anchor: 3 },
    B12TypeRow { name: "Mutant", anchor: 4 },
    B12TypeRow { name: "Semaphore", anchor: 5 },
    B12TypeRow { name: "Section", anchor: 6 },
    B12TypeRow { name: "File", anchor: 7 },
    B12TypeRow { name: "Port", anchor: 8 },
    B12TypeRow { name: "Key", anchor: 9 },
];

/// 清单容量。
pub const B12_CAP: usize = 10;

/// 探测信息类闭集（本面登记的系统查询类，类号取公开文档结论）。
pub const PROBE_CLASSES: &[u32] = &[0, 2, 3, 5, 11, 16];

/// 类号是否在探测闭集内。
pub fn class_in_set(class: u32) -> bool {
    PROBE_CLASSES.iter().any(|c| *c == class)
}

// ── 探测状态码闭集（公开 NTSTATUS 结论，只引结论不抄码） ──────────
/// 状态码：成功。
pub const ST_OK: u32 = 0x0000_0000;
/// 状态码：信息长度不匹配。
pub const ST_LEN_MISMATCH: u32 = 0xC000_0004;
/// 状态码：参数无效。
pub const ST_INVALID_PARAM: u32 = 0xC000_000D;
/// 状态码：信息类号无效。
pub const ST_INVALID_CLASS: u32 = 0xC000_0003;

/// 状态码是否在闭集内。
pub fn status_in_set(st: u32) -> bool {
    st == ST_OK || st == ST_LEN_MISMATCH || st == ST_INVALID_PARAM || st == ST_INVALID_CLASS
}

// ── 探测调用与语料数据模型 ───────────────────────────────────────
/// 一次探测调用四元账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeCall {
    /// 信息类号。
    pub class: u32,
    /// 缓冲区账长（非零有效）。
    pub buf_len: u32,
    /// 实回长度（记账，成功时非零）。
    pub ret_len: u32,
    /// 状态码（闭集内）。
    pub status: u32,
}

impl ProbeCall {
    /// 调用账自检：类号/账长/状态全在闭集。
    pub fn well_formed(&self) -> bool {
        if !class_in_set(self.class) {
            return false;
        }
        if self.buf_len == 0 {
            return false;
        }
        if !status_in_set(self.status) {
            return false;
        }
        true
    }
}

/// 语料一行：枚举快照（类型索引/类型名/句柄计数/探测状态/实回长度）。
#[derive(Clone, Debug)]
pub struct EnumRow {
    /// 类型索引（应等于清单行号）。
    pub index: u32,
    /// 枚举出的类型名。
    pub name: String,
    /// 句柄计数（非零）。
    pub handle_count: u32,
    /// 探测状态码。
    pub status: u32,
    /// 实回长度（非零）。
    pub ret_len: u32,
    /// B12 回指标记（锚号）。
    pub anchor: u32,
}

impl EnumRow {
    /// 行名对平：枚举名与清单名逐字符相等。
    pub fn name_matches(&self) -> bool {
        match B12_TYPES.get(self.index as usize) {
            Some(t) => t.name == self.name.as_str() && t.anchor == self.anchor,
            None => false,
        }
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "行{}({}):句柄{} 状态0x{:08X} 回长{} 锚{}",
            self.index, self.name, self.handle_count, self.status, self.ret_len, self.anchor
        )
    }
}

/// 探测面状态机：待探测 → 已枚举 → 已对平，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    /// 待探测：只许发起探测。
    Idle,
    /// 已枚举：语料落账，等待对平。
    Enumerated,
    /// 已对平：完整性断言全绿，只读。
    Reconciled,
}

impl ProbeState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            ProbeState::Idle => 0,
            ProbeState::Enumerated => 1,
            ProbeState::Reconciled => 2,
        }
    }
}

/// 类型枚举探测面。
#[derive(Clone, Debug)]
pub struct EnumProbe {
    /// 语料行（按枚举序）。
    rows: Vec<EnumRow>,
    /// 探测调用账。
    call: Option<ProbeCall>,
    /// 面状态。
    state: ProbeState,
}

impl EnumProbe {
    /// 开一个新探测面。
    pub fn new() -> EnumProbe {
        EnumProbe {
            rows: Vec::new(),
            call: None,
            state: ProbeState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ProbeState {
        self.state
    }

    /// 语料行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&EnumRow> {
        self.rows.get(idx)
    }

    /// 当前探测调用账。
    pub fn call(&self) -> Option<ProbeCall> {
        self.call
    }

    /// 发起探测：类号/账长双闸 + 状态机闸。
    pub fn probe(&mut self, class: u32, buf_len: u32) -> Result<ProbeCall, u32> {
        // 状态机闸：已对平后重探测拒绝（倒退）。
        if self.state.rank() >= ProbeState::Reconciled.rank() {
            return Err(ProbeDiag::ERewind.val());
        }
        // 类号闸。
        if !class_in_set(class) {
            return Err(ProbeDiag::EClassRange.val());
        }
        // 账长闸。
        if buf_len == 0 {
            return Err(ProbeDiag::EParam.val());
        }
        let call = ProbeCall {
            class,
            buf_len,
            ret_len: buf_len,
            status: ST_OK,
        };
        self.call = Some(call);
        Ok(call)
    }

    /// 落一行语料（索引序/重复名/状态闭集/句柄非零/回长非零五闸）。
    pub fn push_row(&mut self, index: u32, name: &str, handle_count: u32, ret_len: u32) -> Result<(), u32> {
        // 状态机闸：未发起探测不许落语料。
        if self.call.is_none() {
            return Err(ProbeDiag::ENotEnumerated.val());
        }
        // 已对平后写语料拒绝。
        if self.state.rank() >= ProbeState::Reconciled.rank() {
            return Err(ProbeDiag::ERewind.val());
        }
        // 索引序闸：只许按 0..10 顺序逐行落。
        let expect = self.rows.len() as u32;
        if index != expect {
            return Err(ProbeDiag::EIndexOrder.val());
        }
        // 索引越清单闸。
        if index as usize >= B12_CAP {
            return Err(ProbeDiag::ERowExtra.val());
        }
        // 名称非空闸。
        if name.is_empty() {
            return Err(ProbeDiag::ENameMismatch.val());
        }
        // 重复名闸。
        for r in self.rows.iter() {
            if r.name == name {
                return Err(ProbeDiag::ENameDup.val());
            }
        }
        // 状态闭集闸（语料行状态恒成功）。
        if !status_in_set(ST_OK) {
            return Err(ProbeDiag::EStatusRange.val());
        }
        // 句柄非零闸。
        if handle_count == 0 {
            return Err(ProbeDiag::EIntegrity.val());
        }
        // 回长非零闸。
        if ret_len == 0 {
            return Err(ProbeDiag::EIntegrity.val());
        }
        // 回指标记闸：锚号 = 行号（B12 回指语义）。
        let anchor = match B12_TYPES.get(index as usize) {
            Some(t) => t.anchor,
            None => return Err(ProbeDiag::EAnchorMiss.val()),
        };
        self.rows.push(EnumRow {
            index,
            name: name.to_string(),
            handle_count,
            status: ST_OK,
            ret_len,
            anchor,
        });
        if self.rows.len() == B12_CAP {
            self.state = ProbeState::Enumerated;
        }
        Ok(())
    }

    /// 对平：完整性断言 10/10 全绿才许推进。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        if self.state.rank() >= ProbeState::Reconciled.rank() {
            return Err(ProbeDiag::ERewind.val());
        }
        if self.state.rank() < ProbeState::Enumerated.rank() {
            return Err(ProbeDiag::ENotEnumerated.val());
        }
        if !integrity_all_green(self) {
            return Err(ProbeDiag::EIntegrity.val());
        }
        self.state = ProbeState::Reconciled;
        Ok(())
    }
}

// ── 枚举完整性断言 10/10 ─────────────────────────────────────────
/// 完整性断言：清单行数 / 回指标记齐 / 名称非空 / 无重复名 / 类型索引单调 /
/// 状态全成功 / 句柄非零 / 语料行数恒等 / 名称对平 / 回长记账齐 = 10 条。
pub fn integrity_assertions(p: &EnumProbe) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0：语料行数恒等清单行数。
    out.push((
        "完整性0-行数恒等10".to_string(),
        p.len() == B12_CAP && !p.is_empty(),
    ));
    // 断言 1：类型索引单调（0..10 各一按序）。
    let mut idx_ok = true;
    for i in 0..p.len() {
        match p.row_at(i) {
            Some(r) => {
                if r.index != i as u32 {
                    idx_ok = false;
                }
            }
            None => idx_ok = false,
        }
    }
    out.push(("完整性1-索引单调".to_string(), idx_ok));
    // 断言 2：名称非空。
    let mut name_ok = true;
    for r in p.rows.iter() {
        if r.name.is_empty() {
            name_ok = false;
        }
    }
    out.push(("完整性2-名称非空".to_string(), name_ok));
    // 断言 3：无重复名。
    let mut nodup_ok = true;
    let mut i = 0usize;
    while i < p.len() {
        let a = match p.row_at(i) {
            Some(r) => r.name.clone(),
            None => {
                nodup_ok = false;
                break;
            }
        };
        let mut j = i + 1;
        while j < p.len() {
            if let Some(b) = p.row_at(j) {
                if a == b.name {
                    nodup_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out.push(("完整性3-无重复名".to_string(), nodup_ok));
    // 断言 4：状态全成功。
    let mut st_ok = true;
    for r in p.rows.iter() {
        if r.status != ST_OK {
            st_ok = false;
        }
    }
    out.push(("完整性4-状态全成功".to_string(), st_ok));
    // 断言 5：句柄计数非零。
    let mut hc_ok = true;
    for r in p.rows.iter() {
        if r.handle_count == 0 {
            hc_ok = false;
        }
    }
    out.push(("完整性5-句柄非零".to_string(), hc_ok));
    // 断言 6：回长记账齐（每行实回长度非零）。
    let mut rl_ok = true;
    for r in p.rows.iter() {
        if r.ret_len == 0 {
            rl_ok = false;
        }
    }
    out.push(("完整性6-回长记账齐".to_string(), rl_ok));
    // 断言 7：名称对平（逐行与 B12 清单相等）。
    let mut match_ok = true;
    for i in 0..p.len() {
        match p.row_at(i) {
            Some(r) => {
                if !r.name_matches() {
                    match_ok = false;
                }
            }
            None => match_ok = false,
        }
    }
    out.push(("完整性7-名称对平".to_string(), match_ok));
    // 断言 8：回指标记齐（锚号逐行对上清单）。
    let mut anchor_ok = true;
    for i in 0..p.len() {
        match (p.row_at(i), B12_TYPES.get(i)) {
            (Some(r), Some(t)) => {
                if r.anchor != t.anchor {
                    anchor_ok = false;
                }
            }
            _ => anchor_ok = false,
        }
    }
    out.push(("完整性8-回指标记齐".to_string(), anchor_ok));
    // 断言 9：探测调用账齐（调用在案且 well-formed）。
    let call_ok = match p.call() {
        Some(c) => c.well_formed(),
        None => false,
    };
    out.push(("完整性9-调用账齐".to_string(), call_ok));
    out
}

/// 10/10 全绿判定。
pub fn integrity_all_green(p: &EnumProbe) -> bool {
    let list = integrity_assertions(p);
    if list.len() != 10 {
        return false;
    }
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 构建器：完整枚举语料 ─────────────────────────────────────────
/// 构建完整枚举语料（B12 清单逐行复现，句柄计数确定性生成）。
pub fn build_full_probe() -> EnumProbe {
    let mut p = EnumProbe::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = p.probe(5, 0x1000);
    let mut i = 0u32;
    while i < B12_CAP as u32 {
        let name = match B12_TYPES.get(i as usize) {
            Some(t) => t.name,
            None => {
                let _ = i;
                break;
            }
        };
        let hc = 1 + rng.below(0xFFFF);
        let rl = 0x100 + rng.below(0xF00);
        match p.push_row(i, name, hc, rl) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（语料缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    p
}

// ── 注入面：十路语料突变 ─────────────────────────────────────────
/// 语料突变类型闭集（注入联测的十路手写突变）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    /// 缺一行（第 3 行不落）。
    DropRow,
    /// 换序（第 5 行与第 6 行互换索引落账）。
    SwapIndex,
    /// 改名（第 1 行类型名改写）。
    RenameRow,
    /// 重名（第 7 行复用第 0 行名）。
    DupName,
    /// 索引乱（首行索引写成 1）。
    BadIndex,
    /// 状态坏（语料状态写成参数无效）。
    BadStatus,
    /// 句柄清零（第 2 行句柄计数清零）。
    ZeroHandle,
    /// 回长清零（第 4 行实回长度清零）。
    ZeroRetLen,
    /// 回指标记错（第 9 行锚号写成 0）。
    BadAnchor,
    /// 多余行（第 11 行越清单落账）。
    ExtraRow,
}

impl Mutation {
    /// 路号（0..10）。
    pub fn index(self) -> u32 {
        match self {
            Mutation::DropRow => 0,
            Mutation::SwapIndex => 1,
            Mutation::RenameRow => 2,
            Mutation::DupName => 3,
            Mutation::BadIndex => 4,
            Mutation::BadStatus => 5,
            Mutation::ZeroHandle => 6,
            Mutation::ZeroRetLen => 7,
            Mutation::BadAnchor => 8,
            Mutation::ExtraRow => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            Mutation::DropRow => "缺行",
            Mutation::SwapIndex => "换序",
            Mutation::RenameRow => "改名",
            Mutation::DupName => "重名",
            Mutation::BadIndex => "索引乱",
            Mutation::BadStatus => "状态坏",
            Mutation::ZeroHandle => "句柄清零",
            Mutation::ZeroRetLen => "回长清零",
            Mutation::BadAnchor => "回指标记错",
            Mutation::ExtraRow => "多余行",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [Mutation] {
        &[
            Mutation::DropRow,
            Mutation::SwapIndex,
            Mutation::RenameRow,
            Mutation::DupName,
            Mutation::BadIndex,
            Mutation::BadStatus,
            Mutation::ZeroHandle,
            Mutation::ZeroRetLen,
            Mutation::BadAnchor,
            Mutation::ExtraRow,
        ]
    }
}

/// 构建带单一突变的语料面（除指定突变外逐行复现 B12 清单）。
/// 返回（语料面, 落账闸拒收次数）——突变要么落进语料被断言抓红，
/// 要么被落账闸拒收（拒收同样计入检出，不许静默放行）。
pub fn build_mutated_probe(m: Mutation) -> (EnumProbe, usize) {
    let mut p = EnumProbe::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2C) as u64);
    let _ = p.probe(5, 0x1000);
    let mut rejects = 0usize;
    let mut i = 0u32;
    while i < B12_CAP as u32 {
        let name = match B12_TYPES.get(i as usize) {
            Some(t) => t.name,
            None => break,
        };
        // 各突变路在对应行替换语义。
        let (use_name, hc, rl) = (name, 1 + rng.below(0xFFFF), 0x100 + rng.below(0xF00));
        match m {
            Mutation::DropRow => {
                if i == 3 {
                    // 第 3 行不落，直接跳行（后续行必然乱序被拒）。
                    i += 1;
                    continue;
                }
                if p.push_row(i, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::SwapIndex => {
                // 第 5 行写索引 6（乱序落账被拒即检出）。
                let idx = if i == 5 { 6 } else { i };
                if p.push_row(idx, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::RenameRow => {
                let n = if i == 1 { "Token2" } else { use_name };
                if p.push_row(i, n, hc, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::DupName => {
                let n = if i == 7 { "Process" } else { use_name };
                if p.push_row(i, n, hc, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::BadIndex => {
                let idx = if i == 0 { 1 } else { i };
                if p.push_row(idx, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::BadStatus => {
                // 状态坏：落账后直改第 4 行状态码为参数无效。
                if p.push_row(i, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
                if i == 4 {
                    if let Some(last) = p.rows.last_mut() {
                        last.status = ST_INVALID_PARAM;
                    }
                }
            }
            Mutation::ZeroHandle => {
                let h = if i == 2 { 0 } else { 1 + rng.below(0xFFFF) };
                if p.push_row(i, use_name, h, rl).is_err() {
                    rejects += 1;
                }
            }
            Mutation::ZeroRetLen => {
                let r = if i == 4 { 0 } else { 0x100 + rng.below(0xF00) };
                if p.push_row(i, use_name, 1 + rng.below(0xFFFF), r).is_err() {
                    rejects += 1;
                }
            }
            Mutation::BadAnchor => {
                // 锚号在 push_row 内按行号冻结，锚错突变经由直改语料行注入。
                if p.push_row(i, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
                if i == 9 {
                    if let Some(last) = p.rows.last_mut() {
                        last.anchor = 0;
                    }
                }
            }
            Mutation::ExtraRow => {
                if p.push_row(i, use_name, hc, rl).is_err() {
                    rejects += 1;
                }
                if i == B12_CAP as u32 - 1 {
                    // 第 11 行越清单落账（越界索引被落账闸拒收，计入检出）。
                    if p.push_row(B12_CAP as u32, "GhostType", 1, 1).is_err() {
                        rejects += 1;
                    }
                }
            }
        }
        i += 1;
    }
    (p, rejects)
}

/// 十路突变联测：每路必须被检出（断言翻红 / 行数异常 / 落账闸拒收，
/// 三者居一即算抓到，全不沾即静默放行——判据失效）。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn mutation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for m in Mutation::all().iter() {
        total += 1;
        let (p, rejects) = build_mutated_probe(*m);
        let bad = !integrity_all_green(&p) || p.len() != B12_CAP || rejects > 0;
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", m.index(), m.name());
        }
    }
    (caught, total, escape)
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 类型枚举探测对平绿（判据 J1 的"对平断言绿"直读口）。
fn reconcile_ok() -> bool {
    let mut p = build_full_probe();
    if p.len() != B12_CAP {
        return false;
    }
    if p.reconcile().is_err() {
        return false;
    }
    if p.state() != ProbeState::Reconciled {
        return false;
    }
    true
}

/// 枚举完整性断言 10/10（判据 J1 的完整性直读口）。
fn integrity_ok() -> bool {
    let p = build_full_probe();
    if !integrity_all_green(&p) {
        return false;
    }
    let list = integrity_assertions(&p);
    if list.len() != 10 {
        return false;
    }
    true
}

/// 注入联测 10/10 检出零静默。
fn sweep_ok() -> bool {
    let (caught, total, escape) = mutation_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 双跑一致（确定性 LCG 下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_probe();
    let b = build_full_probe();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(ra), Some(rb)) => {
                if ra.name != rb.name || ra.handle_count != rb.handle_count {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = mutation_sweep();
    let rb = mutation_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：未探测落语料被拒；越类探测被拒；零账长被拒；对平后重探测被拒。
fn boundary_ok() -> bool {
    let mut p = EnumProbe::new();
    // 未探测落语料被拒。
    if p.push_row(0, "Process", 1, 1).is_ok() {
        return false;
    }
    // 越类探测被拒。
    if p.probe(999, 0x1000).is_ok() {
        return false;
    }
    // 零账长被拒。
    if p.probe(5, 0).is_ok() {
        return false;
    }
    // 正常探测后落语料，对平后重探测被拒。
    let _ = p.probe(5, 0x1000);
    let mut full = build_full_probe();
    let _ = full.reconcile();
    if full.probe(5, 0x1000).is_ok() {
        return false;
    }
    true
}

/// 探测类闭集与状态闭集守卫。
fn closed_set_ok() -> bool {
    // 类闭集：0/2/3/5/11/16 在内，4/24/999 不在。
    if !class_in_set(0) || !class_in_set(16) {
        return false;
    }
    if class_in_set(4) || class_in_set(24) || class_in_set(999) {
        return false;
    }
    // 状态闭集：四值在内，其余不在。
    if !status_in_set(ST_OK) || !status_in_set(ST_LEN_MISMATCH) {
        return false;
    }
    if !status_in_set(ST_INVALID_PARAM) || !status_in_set(ST_INVALID_CLASS) {
        return false;
    }
    if status_in_set(0xC000_0001) || status_in_set(1) {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12649u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut p = build_full_probe();
    let _ = p.reconcile();
    if p.state() != ProbeState::Reconciled {
        return false;
    }
    match p.row_at(0) {
        Some(r) => {
            if r.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12650 类型枚举探测对平自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12649_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12649-sysinfo-probe");
    let probe = build_full_probe();

    s.add_dyn(
        "F12650-01 B12 清单十行齐",
        B12_TYPES.len() == B12_CAP,
        &format!("rows={}", B12_TYPES.len()),
    );

    s.add_dyn(
        "F12650-02 类型枚举对平绿",
        reconcile_ok(),
        &format!("green={}", integrity_all_green(&probe)),
    );

    s.add_dyn(
        "F12650-03 完整性断言 10/10",
        integrity_ok(),
        &format!("rows={}", integrity_assertions(&probe).len()),
    );

    s.add_dyn(
        "F12650-04 注入联测 10/10 检出",
        sweep_ok(),
        &format!("caught={}", mutation_sweep().0),
    );

    s.add_dyn(
        "F12650-05 探测类与状态闭集",
        closed_set_ok(),
        "类 0/2/3/5/11/16；状态 0/0xC0000004/0xC000000D/0xC0000003",
    );

    s.add_dyn(
        "F12650-06 双跑一致",
        dual_run_ok(),
        "两次构建与两次联测逐项一致",
    );

    s.add_dyn(
        "F12650-07 边界与状态机倒退",
        boundary_ok(),
        "未探测落语料/越类/零账长/对平后重探测 四点",
    );

    s.add_dyn(
        "F12650-08 完成写回语义",
        writeback_ok(),
        "对平即已完成且摘要非空",
    );

    s.add_dyn(
        "F12650-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 完整性断言表结构：恒 10 条且每条独立为真。
    let list = integrity_assertions(&probe);
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12650-10 完整性断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 语料行摘要可点验：末行摘要含行号 9。
    let brief_ok = match probe.row_at(B12_CAP - 1) {
        Some(r) => r.brief().contains("行9"),
        None => false,
    };
    s.add_dyn(
        "F12650-11 语料行摘要",
        brief_ok,
        "末行摘要含行号可点验",
    );

    // 探测调用账齐：调用在案且账长/回长非零。
    let call_ok = match probe.call() {
        Some(c) => c.well_formed() && c.buf_len == 0x1000 && c.ret_len == c.buf_len,
        None => false,
    };
    s.add_dyn(
        "F12650-12 探测调用账齐",
        call_ok,
        "类5 账长0x1000 回长=账长",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80C40);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(PROBE_ALL.len(), 12);
        let mut i = 0usize;
        while i < PROBE_ALL.len() {
            let vi = PROBE_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < PROBE_ALL.len() {
                let vj = PROBE_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_reconciles() {
        let mut p = build_full_probe();
        assert_eq!(p.len(), 10);
        assert_eq!(p.state(), ProbeState::Enumerated);
        assert!(p.reconcile().is_ok());
        assert_eq!(p.state(), ProbeState::Reconciled);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路突变全数检出，无静默。
        let (caught, total, escape) = mutation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_full_probe();
        let b = build_full_probe();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(ra), Some(rb)) => {
                    assert_eq!(ra.name, rb.name);
                    assert_eq!(ra.handle_count, rb.handle_count);
                    assert_eq!(ra.ret_len, rb.ret_len);
                }
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(mutation_sweep().0, mutation_sweep().0);
    }

    #[test]
    fn row_gates_reject() {
        // 越序：首行索引 1 被拒。
        let mut p = EnumProbe::new();
        let _ = p.probe(5, 0x1000);
        assert_eq!(
            p.push_row(1, "Process", 1, 1),
            Err(ProbeDiag::EIndexOrder.val())
        );
        // 重复名被拒。
        let mut p2 = EnumProbe::new();
        let _ = p2.probe(5, 0x1000);
        let _ = p2.push_row(0, "Process", 1, 1);
        assert_eq!(p2.push_row(1, "Process", 1, 1), Err(ProbeDiag::ENameDup.val()));
        // 空名被拒。
        let mut p3 = EnumProbe::new();
        let _ = p3.probe(5, 0x1000);
        assert_eq!(p3.push_row(0, "", 1, 1), Err(ProbeDiag::ENameMismatch.val()));
        // 零句柄被拒。
        let mut p4 = EnumProbe::new();
        let _ = p4.probe(5, 0x1000);
        assert_eq!(p4.push_row(0, "Process", 0, 1), Err(ProbeDiag::EIntegrity.val()));
    }

    #[test]
    fn probe_gates_reject() {
        // 越类探测被拒。
        let mut p = EnumProbe::new();
        assert_eq!(p.probe(999, 0x1000), Err(ProbeDiag::EClassRange.val()));
        // 零账长被拒。
        assert_eq!(p.probe(5, 0), Err(ProbeDiag::EParam.val()));
        // 未探测落语料被拒。
        assert_eq!(p.push_row(0, "Process", 1, 1), Err(ProbeDiag::ENotEnumerated.val()));
    }

    #[test]
    fn reconcile_requires_full_rows() {
        // 边界：十行未齐禁止对平。
        let mut p = EnumProbe::new();
        let _ = p.probe(5, 0x1000);
        assert_eq!(p.reconcile(), Err(ProbeDiag::ENotEnumerated.val()));
        // 齐后对平成功，重对平倒退拒绝。
        let mut full = build_full_probe();
        assert!(full.reconcile().is_ok());
        assert_eq!(full.reconcile(), Err(ProbeDiag::ERewind.val()));
        // 对平后重探测拒绝。
        assert_eq!(full.probe(5, 0x1000), Err(ProbeDiag::ERewind.val()));
    }

    #[test]
    fn integrity_assertions_count_is_ten() {
        // 完整性断言恒 10 条且全绿；缺行语料必红。
        let p = build_full_probe();
        let list = integrity_assertions(&p);
        assert_eq!(list.len(), 10);
        assert!(integrity_all_green(&p));
        // 突变语料：至少一路断言翻红（十路全查）。
        for m in Mutation::all().iter() {
            let (bad, rejects) = build_mutated_probe(*m);
            assert!(
                !integrity_all_green(&bad) || bad.len() != B12_CAP || rejects > 0,
                "突变路 {} 未检出",
                m.name()
            );
        }
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Idle→Enumerated→Reconciled。
        let mut p = build_full_probe();
        assert!(ProbeState::Idle.rank() < ProbeState::Enumerated.rank());
        assert!(ProbeState::Enumerated.rank() < ProbeState::Reconciled.rank());
        assert_eq!(p.state(), ProbeState::Enumerated);
        let _ = p.reconcile();
        assert_eq!(p.state(), ProbeState::Reconciled);
    }

    #[test]
    fn b12_list_is_frozen() {
        // B12 清单冻结：行数/锚号/名称语义不漂。
        assert_eq!(B12_TYPES.len(), B12_CAP);
        let mut i = 0usize;
        while i < B12_TYPES.len() {
            match B12_TYPES.get(i) {
                Some(t) => {
                    assert_eq!(t.anchor, i as u32, "行 {} 锚号漂移", i);
                    assert!(!t.name.is_empty());
                }
                None => assert!(false, "清单缺行"),
            }
            i += 1;
        }
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12649_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
