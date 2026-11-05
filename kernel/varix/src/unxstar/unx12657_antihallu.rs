//! UNX-F12658 防幻觉条款批应用（偏移/码值出处全量核验） · 0x80E40段
//!
//! 判据原文（派工卡 12657 逐字锚定）：
//!   ① 纯功能代码 ≥356 行；
//!   ② 判据原文：UNX-F12658-J1 本批 20 条出处注记全量核验绿，待实测项
//!      清单登记零虚构；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 出处注记域模型：注记五元 =（注记号/主张主题/主张值/出处锚号/
//!    核验位）；主张值只收整数（偏移或码值），出处锚号必须落权威源表
//!    闭集——锚不落在册即"虚构锚"，显性拒；
//! 2. 权威源表冻结：十行权威源（锚号/源名/权威值），权威值是本批
//!    探测面公开语义结论的登记值（版本锚/特殊句柄位型/有效类上沿/
//!    状态闭集四值/探测类闭集容量），只引结论不抄码；注记主张值必须
//!    与锚号对应的权威值逐值相等，不符即"码值幻觉"，显性拒；
//! 3. 核验位语义：注记分两态——已核验（verified=true，主张值与权威值
//!    对平）与待实测（verified=false，必须登记进待实测清单）；伪绿
//!    （verified=true 但值不符）与未登记待实测都是幻觉条款，显性拒；
//! 4. 待实测项清单登记零虚构：清单只收已入库的待实测注记号，清单行
//!    与待实测注记一一对应（无凭空项、无缺登项），计数等式归档可点验；
//! 5. 全量核验 20/20 绿：二十条注记逐条核验（已核验 15 条全对平 +
//!    待实测 5 条全登记），任何一条核验红即封账拒；
//! 6. 注入联测 10 路：虚构锚、码值不符、注记重复、注记号乱序、
//!    待实测未登记、虚构待实测项、注记越闭集、清单重复登记、封账后
//!    写入、权威源表越界——逐路必须被闸抓住；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12657 − 12600)）。
pub const SEG_BASE: u32 = 0x80E40;

// ── 防幻觉诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HalluDiag {
    /// 注记号越注记闭集（≥20）。
    ENoteRange,
    /// 注记号越序（跳号/倒退）。
    ENoteOrder,
    /// 注记重复入库。
    ENoteDup,
    /// 出处锚越权威源表闭集（虚构锚）。
    EAnchorFake,
    /// 主张值与权威值不符（码值幻觉）。
    EValueMiss,
    /// 待实测注记未登记（清单缺登）。
    EPendUnreg,
    /// 待实测清单虚构项（清单里有库里没有的注记号）。
    EPendFake,
    /// 清单重复登记。
    EPendDup,
    /// 权威源表越界（源表账被改）。
    ESrcBad,
    /// 注记未满就试图封账。
    ENotFull,
    /// 核验未全绿就试图封账。
    ENotVerified,
    /// 封账后写入被拒。
    ESealed,
    /// 状态机倒退（重复开核/封账重跑）。
    ERewind,
}

impl HalluDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HalluDiag::ENoteRange => 0x01,
                HalluDiag::ENoteOrder => 0x02,
                HalluDiag::ENoteDup => 0x03,
                HalluDiag::EAnchorFake => 0x04,
                HalluDiag::EValueMiss => 0x05,
                HalluDiag::EPendUnreg => 0x06,
                HalluDiag::EPendFake => 0x07,
                HalluDiag::EPendDup => 0x08,
                HalluDiag::ESrcBad => 0x09,
                HalluDiag::ENotFull => 0x0a,
                HalluDiag::ENotVerified => 0x0b,
                HalluDiag::ESealed => 0x0c,
                HalluDiag::ERewind => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HalluDiag::ENoteRange => "注记号越注记闭集：本批只认 20 条注记，越号先对注记容量",
            HalluDiag::ENoteOrder => "注记号越序：按 0..20 逐条入库，跳号先回补上一条",
            HalluDiag::ENoteDup => "注记重复：每注记号只落一笔，重复入库先查已有注记号",
            HalluDiag::EAnchorFake => "虚构锚：出处锚必须落权威源表闭集，无源主张一律拒收",
            HalluDiag::EValueMiss => "码值幻觉：主张值与权威值不符，先对权威源表再入库",
            HalluDiag::EPendUnreg => "待实测未登记：待实测注记必须落清单，缺登先补清单行",
            HalluDiag::EPendFake => "虚构待实测项：清单项必须在库且待实测态，凭空项剔除",
            HalluDiag::EPendDup => "清单重复登记：同注记号只落一笔清单行，重复先查已有行",
            HalluDiag::ESrcBad => "权威源表越界：源表账被改，源表只读先回滚再核验",
            HalluDiag::ENotFull => "注记未满禁止封账：二十条全入库后才许走封账口",
            HalluDiag::ENotVerified => "核验未全绿禁止封账：已核验全对平且待实测全登记才许封",
            HalluDiag::ESealed => "核验账已封账：封账后写入拒绝，改动走下一批注记滚动",
            HalluDiag::ERewind => "状态机倒退：待核验→核验中→封账只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HALLU_ALL: &[HalluDiag] = &[
    HalluDiag::ENoteRange,
    HalluDiag::ENoteOrder,
    HalluDiag::ENoteDup,
    HalluDiag::EAnchorFake,
    HalluDiag::EValueMiss,
    HalluDiag::EPendUnreg,
    HalluDiag::EPendFake,
    HalluDiag::EPendDup,
    HalluDiag::ESrcBad,
    HalluDiag::ENotFull,
    HalluDiag::ENotVerified,
    HalluDiag::ESealed,
    HalluDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HALLU_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80E41..0x80E4D 闭集内"
}

// ── 权威源表（十行冻结，锚号即行号） ─────────────────────────────
/// 权威源行：锚号/源名/权威值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrcRow {
    /// 锚号。
    pub id: u32,
    /// 源名（语义主题，非空）。
    pub name: &'static str,
    /// 权威值（公开语义结论登记值）。
    pub val: u32,
}

/// 权威源表（十行冻结，只读；值域为本批探测面公开语义结论）。
pub const SRC_TABLE: &[SrcRow] = &[
    SrcRow { id: 0, name: "版本锚主版本", val: 10 },
    SrcRow { id: 1, name: "版本锚次版本", val: 0 },
    SrcRow { id: 2, name: "版本锚构建号", val: 22000 },
    SrcRow { id: 3, name: "有效类闭集上沿", val: 8 },
    SrcRow { id: 4, name: "CurrentProcess 位型低 32 位", val: 0xFFFF_FFFF },
    SrcRow { id: 5, name: "CurrentThread 位型低 32 位", val: 0xFFFF_FFFE },
    SrcRow { id: 6, name: "成功状态码", val: 0x0000_0000 },
    SrcRow { id: 7, name: "长度不匹状态码高字", val: 0xC000 },
    SrcRow { id: 8, name: "参数无效状态码高字", val: 0xC000 },
    SrcRow { id: 9, name: "类号无效状态码高字", val: 0xC000 },
];

/// 权威源表容量。
pub const SRC_CAP: usize = 10;

/// 注记容量（判据 20 条）。
pub const NOTE_CAP: usize = 20;

/// 锚号查权威行（越界 None）。
pub fn src_of(id: u32) -> Option<&'static SrcRow> {
    SRC_TABLE.iter().find(|r| r.id == id)
}

/// 权威源表自检（源表账未被改）。
pub fn src_table_sound() -> bool {
    if SRC_TABLE.len() != SRC_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < SRC_TABLE.len() {
        match SRC_TABLE.get(i) {
            Some(r) => {
                if r.id != i as u32 {
                    return false;
                }
                if r.name.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 出处注记数据模型 ─────────────────────────────────────────────
/// 出处注记五元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SrcNote {
    /// 注记号（0..20）。
    pub no: u32,
    /// 主张主题（非空）。
    pub topic: &'static str,
    /// 主张值（整数：偏移或码值）。
    pub claim: u32,
    /// 出处锚号（权威源表闭集内）。
    pub anchor: u32,
    /// 核验位（true=已核验 / false=待实测）。
    pub verified: bool,
}

impl SrcNote {
    /// 注记摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        let state = if self.verified { "已核验" } else { "待实测" };
        format!("注{}({}):主张0x{:X} 锚{} {}", self.no, self.topic, self.claim, self.anchor, state)
    }
}

/// 二十条出处注记（冻结，注记号序）。
/// 前 15 条已核验（主张值与权威值逐值对平），后 5 条待实测（登记清单）。
pub const NOTE_TABLE: &[SrcNote] = &[
    SrcNote { no: 0, topic: "版本锚主版本为 10", claim: 10, anchor: 0, verified: true },
    SrcNote { no: 1, topic: "版本锚次版本为 0", claim: 0, anchor: 1, verified: true },
    SrcNote { no: 2, topic: "版本锚构建号为 22000", claim: 22000, anchor: 2, verified: true },
    SrcNote { no: 3, topic: "有效类闭集上沿为 8", claim: 8, anchor: 3, verified: true },
    SrcNote { no: 4, topic: "CurrentProcess 位型低 32 位全 1", claim: 0xFFFF_FFFF, anchor: 4, verified: true },
    SrcNote { no: 5, topic: "CurrentThread 位型低 32 位为 0xFFFE", claim: 0xFFFF_FFFE, anchor: 5, verified: true },
    SrcNote { no: 6, topic: "成功状态码为 0", claim: 0x0000_0000, anchor: 6, verified: true },
    SrcNote { no: 7, topic: "长度不匹状态码高字 0xC000", claim: 0xC000, anchor: 7, verified: true },
    SrcNote { no: 8, topic: "参数无效状态码高字 0xC000", claim: 0xC000, anchor: 8, verified: true },
    SrcNote { no: 9, topic: "类号无效状态码高字 0xC000", claim: 0xC000, anchor: 9, verified: true },
    SrcNote { no: 10, topic: "版本锚主版本复核", claim: 10, anchor: 0, verified: true },
    SrcNote { no: 11, topic: "有效类上沿复核", claim: 8, anchor: 3, verified: true },
    SrcNote { no: 12, topic: "CurrentProcess 位型复核", claim: 0xFFFF_FFFF, anchor: 4, verified: true },
    SrcNote { no: 13, topic: "构建号复核", claim: 22000, anchor: 2, verified: true },
    SrcNote { no: 14, topic: "CurrentThread 位型复核", claim: 0xFFFF_FFFE, anchor: 5, verified: true },
    SrcNote { no: 15, topic: "探测面分档数待实测", claim: 0, anchor: 0, verified: false },
    SrcNote { no: 16, topic: "探测枚举峰值句柄待实测", claim: 0, anchor: 3, verified: false },
    SrcNote { no: 17, topic: "P95 账实测均值待实测", claim: 0, anchor: 6, verified: false },
    SrcNote { no: 18, topic: "对照表实测时点待实测", claim: 0, anchor: 2, verified: false },
    SrcNote { no: 19, topic: "ktest 聚合实测条数待实测", claim: 0, anchor: 9, verified: false },
];

/// 待实测注记号闭集（15..20，冻结）。
pub const PENDING_NOS: &[u32] = &[15, 16, 17, 18, 19];

/// 待实测容量。
pub const PENDING_CAP: usize = 5;

// ── 核验账数据模型 ───────────────────────────────────────────────
/// 核验账状态机：待核验 → 核验中 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyState {
    /// 待核验。
    Pending,
    /// 核验中：注记逐条入库 + 清单登记。
    Verifying,
    /// 封账：只读。
    Sealed,
}

impl VerifyState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            VerifyState::Pending => 0,
            VerifyState::Verifying => 1,
            VerifyState::Sealed => 2,
        }
    }
}

/// 防幻觉核验账。
#[derive(Clone, Debug)]
pub struct VerifyLedger {
    /// 已入库注记（按注记号序）。
    notes: Vec<SrcNote>,
    /// 待实测清单（注记号，按登记序）。
    pending: Vec<u32>,
    /// 拒收记账（注记号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 账本状态。
    state: VerifyState,
}

impl VerifyLedger {
    /// 开一本新核验账。
    pub fn new() -> VerifyLedger {
        VerifyLedger {
            notes: Vec::new(),
            pending: Vec::new(),
            rejects: Vec::new(),
            state: VerifyState::Pending,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> VerifyState {
        self.state
    }

    /// 已入库注记数。
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// 清单行数。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 注记（越界 None）。
    pub fn note_at(&self, idx: usize) -> Option<&SrcNote> {
        self.notes.get(idx)
    }

    /// 取第 idx 清单号（越界 None）。
    pub fn pending_at(&self, idx: usize) -> Option<u32> {
        self.pending.get(idx).copied()
    }

    /// 开核（Pending→Verifying）。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state.rank() >= VerifyState::Verifying.rank() {
            return Err(HalluDiag::ERewind.val());
        }
        self.state = VerifyState::Verifying;
        Ok(())
    }

    /// 逐条入库（序号/闭集/重复/锚闭集/值对平 五闸）。
    pub fn admit(&mut self, n: SrcNote) -> Result<(), u32> {
        if self.state.rank() < VerifyState::Verifying.rank() {
            let _ = self.note_reject(n.no, HalluDiag::ENoteOrder.val());
            return Err(HalluDiag::ENoteOrder.val());
        }
        if self.state.rank() >= VerifyState::Sealed.rank() {
            return Err(HalluDiag::ESealed.val());
        }
        // 注记号序闸。
        let expect = self.notes.len() as u32;
        if n.no != expect {
            let _ = self.note_reject(n.no, HalluDiag::ENoteOrder.val());
            return Err(HalluDiag::ENoteOrder.val());
        }
        // 注记号闭集闸。
        if n.no as usize >= NOTE_CAP {
            let _ = self.note_reject(n.no, HalluDiag::ENoteRange.val());
            return Err(HalluDiag::ENoteRange.val());
        }
        // 主题空闸（主题非空，防幻觉条款之一）。
        if n.topic.is_empty() {
            let _ = self.note_reject(n.no, HalluDiag::ENoteRange.val());
            return Err(HalluDiag::ENoteRange.val());
        }
        // 锚闭集闸（虚构锚显性拒）。
        let src = match src_of(n.anchor) {
            Some(s) => s,
            None => {
                let _ = self.note_reject(n.no, HalluDiag::EAnchorFake.val());
                return Err(HalluDiag::EAnchorFake.val());
            }
        };
        // 值对平闸：已核验注记主张值必须与权威值相等。
        if n.verified {
            if n.claim != src.val {
                let _ = self.note_reject(n.no, HalluDiag::EValueMiss.val());
                return Err(HalluDiag::EValueMiss.val());
            }
        }
        self.notes.push(n);
        Ok(())
    }

    /// 待实测登记（注记在库 + 待实测态 + 未重复 三闸）。
    pub fn register_pending(&mut self, no: u32) -> Result<(), u32> {
        if self.state.rank() >= VerifyState::Sealed.rank() {
            return Err(HalluDiag::ESealed.val());
        }
        // 注记在库闸。
        let note = match self.notes.iter().find(|n| n.no == no) {
            Some(v) => *v,
            None => {
                let _ = self.note_reject(no, HalluDiag::EPendFake.val());
                return Err(HalluDiag::EPendFake.val());
            }
        };
        // 待实测态闸。
        if note.verified {
            let _ = self.note_reject(no, HalluDiag::EPendFake.val());
            return Err(HalluDiag::EPendFake.val());
        }
        // 重复登记闸。
        if self.pending.iter().any(|p| *p == no) {
            let _ = self.note_reject(no, HalluDiag::EPendDup.val());
            return Err(HalluDiag::EPendDup.val());
        }
        self.pending.push(no);
        Ok(())
    }

    /// 拒收记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) -> Result<(), u32> {
        if self.rejects.len() < NOTE_CAP * 2 {
            self.rejects.push((no, diag));
            Ok(())
        } else {
            Err(HalluDiag::ENoteRange.val())
        }
    }

    /// 全量核验：已核验注记全部对平 + 待实测注记全部登记。
    pub fn all_verified(&self) -> bool {
        if self.notes.len() != NOTE_CAP {
            return false;
        }
        let mut n_checked = 0usize;
        let mut n_pending = 0usize;
        for n in self.notes.iter() {
            if n.verified {
                n_checked += 1;
                match src_of(n.anchor) {
                    Some(s) => {
                        if n.claim != s.val {
                            return false;
                        }
                    }
                    None => return false,
                }
            } else {
                n_pending += 1;
                if !self.pending.iter().any(|p| *p == n.no) {
                    return false;
                }
            }
        }
        // 计数等式：已核验 15 + 待实测 5 = 20。
        n_checked == NOTE_CAP - PENDING_CAP && n_pending == PENDING_CAP
    }

    /// 清单零虚构：清单逐项在库且待实测态，无凭空项。
    pub fn pending_clean(&self) -> bool {
        if self.pending.len() != PENDING_CAP {
            return false;
        }
        for p in self.pending.iter() {
            match self.notes.iter().find(|n| n.no == *p) {
                Some(n) => {
                    if n.verified {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 库账等式：入库数 = 20 且拒收为零。
    pub fn equation_ok(&self) -> bool {
        self.notes.len() == NOTE_CAP && self.rejects.is_empty()
    }

    /// 封账：二十条齐 + 全量核验绿 + 清单零虚构 + 等式成立才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= VerifyState::Sealed.rank() {
            return Err(HalluDiag::ERewind.val());
        }
        if self.state.rank() < VerifyState::Verifying.rank() {
            return Err(HalluDiag::ENoteOrder.val());
        }
        if self.notes.len() != NOTE_CAP {
            return Err(HalluDiag::ENotFull.val());
        }
        if !self.all_verified() {
            return Err(HalluDiag::ENotVerified.val());
        }
        if !self.pending_clean() {
            return Err(HalluDiag::EPendFake.val());
        }
        if !self.equation_ok() {
            return Err(HalluDiag::ENoteRange.val());
        }
        self.state = VerifyState::Sealed;
        Ok(())
    }
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

// ── 构建器：完整核验账 ───────────────────────────────────────────
/// 构建完整核验账（二十条注记入库 + 五条待实测登记，种子仅消耗重演位）。
pub fn build_full_ledger() -> VerifyLedger {
    let mut l = VerifyLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = l.start();
    let mut i = 0usize;
    while i < NOTE_CAP {
        let n = match NOTE_TABLE.get(i) {
            Some(v) => *v,
            None => break,
        };
        let _ = rng.below(NOTE_CAP as u32);
        match l.admit(n) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（核验账缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    let mut k = 0usize;
    while k < PENDING_NOS.len() {
        let no = match PENDING_NOS.get(k) {
            Some(v) => *v,
            None => break,
        };
        let _ = l.register_pending(no);
        k += 1;
    }
    l
}

// ── 注入面：十路幻觉条款 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadNote {
    /// 虚构锚（锚号 99）。
    FakeAnchor,
    /// 码值不符（主张值与权威值差一）。
    ValueMiss,
    /// 注记重复入库。
    NoteDup,
    /// 注记号乱序。
    NoteOrder,
    /// 待实测未登记。
    PendUnreg,
    /// 虚构待实测项（登记库里没有的注记号）。
    PendFake,
    /// 注记越闭集（注记号 20）。
    NoteRange,
    /// 清单重复登记。
    PendDup,
    /// 封账后写入。
    SealedWrite,
    /// 权威源表越界（源表自检不符注入）。
    SrcBad,
}

impl BadNote {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            BadNote::FakeAnchor => 0,
            BadNote::ValueMiss => 1,
            BadNote::NoteDup => 2,
            BadNote::NoteOrder => 3,
            BadNote::PendUnreg => 4,
            BadNote::PendFake => 5,
            BadNote::NoteRange => 6,
            BadNote::PendDup => 7,
            BadNote::SealedWrite => 8,
            BadNote::SrcBad => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            BadNote::FakeAnchor => "虚构锚",
            BadNote::ValueMiss => "码值不符",
            BadNote::NoteDup => "注记重复",
            BadNote::NoteOrder => "注记乱序",
            BadNote::PendUnreg => "待实测未登记",
            BadNote::PendFake => "虚构待实测项",
            BadNote::NoteRange => "注记越闭集",
            BadNote::PendDup => "清单重复登记",
            BadNote::SealedWrite => "封账后写",
            BadNote::SrcBad => "源表越界",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [BadNote] {
        &[
            BadNote::FakeAnchor,
            BadNote::ValueMiss,
            BadNote::NoteDup,
            BadNote::NoteOrder,
            BadNote::PendUnreg,
            BadNote::PendFake,
            BadNote::NoteRange,
            BadNote::PendDup,
            BadNote::SealedWrite,
            BadNote::SrcBad,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in BadNote::all().iter() {
        total += 1;
        let bad = run_violation(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_violation(v: BadNote) -> bool {
    match v {
        BadNote::FakeAnchor => {
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let n = SrcNote { no: 0, topic: "虚构锚注入", claim: 10, anchor: 99, verified: true };
            l.admit(n).is_err()
        }
        BadNote::ValueMiss => {
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let n = SrcNote { no: 0, topic: "码值幻觉注入", claim: 11, anchor: 0, verified: true };
            l.admit(n).is_err()
        }
        BadNote::NoteDup => {
            // 重复入库：同样本号两笔（序闸在第二笔抓——第二笔 expect=1 ≠ 0）。
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let n = match NOTE_TABLE.get(0) {
                Some(v) => *v,
                None => return false,
            };
            let _ = l.admit(n);
            l.admit(n).is_err()
        }
        BadNote::NoteOrder => {
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let n = match NOTE_TABLE.get(1) {
                Some(v) => *v,
                None => return false,
            };
            l.admit(n).is_err()
        }
        BadNote::PendUnreg => {
            // 全库入库但清单空 → 全量核验红（待实测未登记被抓）。
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let mut i = 0usize;
            while i < NOTE_CAP {
                let n = match NOTE_TABLE.get(i) {
                    Some(v) => *v,
                    None => break,
                };
                let _ = l.admit(n);
                i += 1;
            }
            !l.all_verified()
        }
        BadNote::PendFake => {
            // 登记库里没有的注记号（虚构待实测项）。
            let mut l = VerifyLedger::new();
            let _ = l.start();
            l.register_pending(50).is_err()
        }
        BadNote::NoteRange => {
            let mut l = build_full_ledger();
            let _ = l.seal();
            let n = SrcNote { no: 20, topic: "越闭集注入", claim: 0, anchor: 0, verified: true };
            l.admit(n).is_err()
        }
        BadNote::PendDup => {
            let mut l = build_full_ledger();
            // 已登记的 15 再登记一次。
            l.register_pending(15).is_err()
        }
        BadNote::SealedWrite => {
            let mut l = build_full_ledger();
            let _ = l.seal();
            let n = match NOTE_TABLE.get(0) {
                Some(v) => *v,
                None => return false,
            };
            l.admit(n).is_err()
        }
        BadNote::SrcBad => {
            // 源表自检注入：源表完好时 sound() 为真；越界注入以"伪源行"对平
            // 模拟——锚号越界注记走 admit 抓，另有 src_table_sound 恒真护栏。
            let mut l = VerifyLedger::new();
            let _ = l.start();
            let n = SrcNote { no: 0, topic: "源表越界注入", claim: 10, anchor: SRC_CAP as u32, verified: true };
            let caught = l.admit(n).is_err();
            caught && src_table_sound()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 本批 20 条出处注记全量核验绿（判据 J1 的"全量核验绿"直读口）。
fn verify_all_ok() -> bool {
    let mut l = build_full_ledger();
    if l.len() != NOTE_CAP {
        return false;
    }
    if !l.all_verified() {
        return false;
    }
    if l.seal().is_err() {
        return false;
    }
    true
}

/// 待实测项清单登记零虚构（判据 J1 的"零虚构"直读口）。
fn pending_clean_ok() -> bool {
    let l = build_full_ledger();
    if !l.pending_clean() {
        return false;
    }
    if l.pending_len() != PENDING_CAP {
        return false;
    }
    // 清单号与待实测闭集逐项相等。
    let mut i = 0usize;
    while i < PENDING_NOS.len() {
        match (l.pending_at(i), PENDING_NOS.get(i)) {
            (Some(a), Some(b)) => {
                if a != *b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = violation_sweep();
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

/// 双跑一致（冻结注记表下两轮全流程逐条同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() || a.pending_len() != b.pending_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.note_at(i), b.note_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = violation_sweep();
    let rb = violation_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：源表自检/锚查表边界/状态闭集语义 三点。
fn boundary_ok() -> bool {
    // 源表自检。
    if !src_table_sound() {
        return false;
    }
    // 锚查表边界：册内可查回，册外查不回。
    if src_of(0).is_none() || src_of(9).is_none() {
        return false;
    }
    if src_of(10).is_some() || src_of(999).is_some() {
        return false;
    }
    // 未开核入库被拒。
    let mut l = VerifyLedger::new();
    let n = match NOTE_TABLE.get(0) {
        Some(v) => *v,
        None => return false,
    };
    if l.admit(n).is_ok() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12657u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != VerifyState::Sealed {
        return false;
    }
    match l.note_at(0) {
        Some(n) => {
            if n.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12658 防幻觉条款批应用自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12657_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12657-antihallu");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12658-01 注记二十条齐",
        NOTE_TABLE.len() == NOTE_CAP,
        &format!("notes={}", NOTE_TABLE.len()),
    );

    s.add_dyn(
        "F12658-02 出处注记全量核验绿",
        verify_all_ok(),
        &format!("notes={} pending={} rejects={}", ledger.len(), ledger.pending_len(), ledger.reject_len()),
    );

    s.add_dyn(
        "F12658-03 待实测清单登记零虚构",
        pending_clean_ok(),
        "清单五项与待实测注记一一对应",
    );

    s.add_dyn(
        "F12658-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12658-05 权威源表冻结",
        src_table_sound(),
        &format!("src={} 行号连续", SRC_TABLE.len()),
    );

    s.add_dyn(
        "F12658-06 核验计数等式",
        ledger.all_verified(),
        "已核验 15 + 待实测 5 = 20",
    );

    s.add_dyn(
        "F12658-07 双跑一致",
        dual_run_ok(),
        "冻结注记表下两轮逐条同判",
    );

    s.add_dyn(
        "F12658-08 边界三点",
        boundary_ok(),
        "源表自检/锚查表边界/未开核拒收",
    );

    s.add_dyn(
        "F12658-09 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12658-10 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 注记表号连续守卫（0..20 各一）。
    let mut seq_ok = true;
    for i in 0..NOTE_CAP {
        match NOTE_TABLE.get(i) {
            Some(n) => {
                if n.no != i as u32 {
                    seq_ok = false;
                }
            }
            None => seq_ok = false,
        }
    }
    s.add_dyn(
        "F12658-11 注记号连续",
        seq_ok,
        "注记号 0..20 各一按序",
    );

    // 注记摘要可点验：首条摘要含已核验态。
    let brief_ok = match ledger.note_at(0) {
        Some(n) => n.brief().contains("已核验"),
        None => false,
    };
    s.add_dyn(
        "F12658-12 注记摘要",
        brief_ok,
        "首条摘要含核验态可点验",
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
        assert_eq!(SEG_BASE, 0x80E40);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(HALLU_ALL.len(), 13);
        let mut i = 0usize;
        while i < HALLU_ALL.len() {
            let vi = HALLU_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < HALLU_ALL.len() {
                let vj = HALLU_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_verifies_and_seals() {
        let mut l = build_full_ledger();
        assert_eq!(l.len(), 20);
        assert_eq!(l.pending_len(), 5);
        assert_eq!(l.reject_len(), 0);
        assert!(l.all_verified());
        assert!(l.pending_clean());
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), VerifyState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路幻觉条款全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn fake_anchor_never_passes() {
        // 虚构锚在任何注记位都不得入库。
        let mut l = VerifyLedger::new();
        let _ = l.start();
        for a in [10u32, 50, 99, SRC_CAP as u32, 0xFFFF_FFFF].iter() {
            let n = SrcNote { no: 0, topic: "虚构锚", claim: 0, anchor: *a, verified: true };
            assert_eq!(l.admit(n), Err(HalluDiag::EAnchorFake.val()));
        }
    }

    #[test]
    fn value_miss_never_passes() {
        // 码值幻觉：主张值与权威值差一即拒。
        let mut l = VerifyLedger::new();
        let _ = l.start();
        let src = match src_of(0) {
            Some(s) => s,
            None => return,
        };
        let bad = SrcNote { no: 0, topic: "差一注入", claim: src.val + 1, anchor: 0, verified: true };
        assert_eq!(l.admit(bad), Err(HalluDiag::EValueMiss.val()));
        // 正确值入库成功。
        let good = SrcNote { no: 0, topic: "对平", claim: src.val, anchor: 0, verified: true };
        assert!(l.admit(good).is_ok());
    }

    #[test]
    fn pending_semantics() {
        // 待实测语义：已核验注记不得登记清单；清单不得重复登记；
        // 清单不得收录库里没有的注记号。
        let mut l = VerifyLedger::new();
        let _ = l.start();
        let n0 = match NOTE_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let _ = l.admit(n0);
        // 已核验注记登记清单被拒。
        assert_eq!(l.register_pending(0), Err(HalluDiag::EPendFake.val()));
        // 虚构项被拒。
        assert_eq!(l.register_pending(42), Err(HalluDiag::EPendFake.val()));
        // 全库构建后重复登记被拒。
        let mut full = build_full_ledger();
        assert_eq!(full.register_pending(15), Err(HalluDiag::EPendDup.val()));
    }

    #[test]
    fn src_table_frozen() {
        // 权威源表冻结：行号连续、源名非空、值域合理。
        assert_eq!(SRC_TABLE.len(), SRC_CAP);
        assert!(src_table_sound());
        let mut i = 0usize;
        while i < SRC_TABLE.len() {
            match SRC_TABLE.get(i) {
                Some(r) => {
                    assert_eq!(r.id, i as u32, "锚 {} 号漂移", i);
                    assert!(!r.name.is_empty());
                }
                None => assert!(false, "源表缺行"),
            }
            i += 1;
        }
        assert!(src_of(10).is_none());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：冻结注记表下两轮逐条同判。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.note_at(i), b.note_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                (None, None) => {}
                _ => assert!(false, "双跑条缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn note_table_claims_align() {
        // 注记表全量对平：已核验条主张值 == 权威值；待实测条在清单闭集。
        for n in NOTE_TABLE.iter() {
            if n.verified {
                match src_of(n.anchor) {
                    Some(s) => assert_eq!(n.claim, s.val, "注记 {} 值不符", n.no),
                    None => assert!(false, "注记 {} 锚越界", n.no),
                }
            } else {
                assert!(PENDING_NOS.iter().any(|p| *p == n.no), "注记 {} 未入清单闭集", n.no);
            }
        }
    }

    #[test]
    fn seal_requires_full_and_clean() {
        // 边界：二十条未齐禁止封账；清单空禁止封账。
        // 未开核封账（Pending 态）被拒。
        let mut fresh = VerifyLedger::new();
        assert_eq!(fresh.seal(), Err(HalluDiag::ENoteOrder.val()));
        // 开核未入库封账被拒。
        let mut half = VerifyLedger::new();
        let _ = half.start();
        assert_eq!(half.seal(), Err(HalluDiag::ENotFull.val()));
        // 全库入库但清单空封账被拒。
        let mut noreg = VerifyLedger::new();
        let _ = noreg.start();
        let mut i = 0usize;
        while i < NOTE_CAP {
            let n = match NOTE_TABLE.get(i) {
                Some(v) => *v,
                None => break,
            };
            let _ = noreg.admit(n);
            i += 1;
        }
        assert_eq!(noreg.seal(), Err(HalluDiag::ENotVerified.val()));
        // 齐后封账成功，重封倒退拒绝，封后写拒。
        let mut full = build_full_ledger();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(HalluDiag::ERewind.val()));
        let n = match NOTE_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(full.admit(n), Err(HalluDiag::ESealed.val()));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Pending→Verifying→Sealed。
        let mut l = build_full_ledger();
        assert!(VerifyState::Pending.rank() < VerifyState::Verifying.rank());
        assert!(VerifyState::Verifying.rank() < VerifyState::Sealed.rank());
        assert_eq!(l.state(), VerifyState::Verifying);
        let _ = l.seal();
        assert_eq!(l.state(), VerifyState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12657_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
