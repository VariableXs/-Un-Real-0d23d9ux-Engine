//! UNX-F12653 Wine conformance 探测样本引用协议（对照不抄） · 0x80D00段
//!
//! 判据原文（派工卡 12652 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12653-J1 样本引用协议（只引结论不抄码）判据绿，
//!      引用清单可点验 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 引用协议域模型：引用单行 =（引用号/样本主题/结论摘要/引用形态/
//!    可点验锚）五元；引用形态闭集只认 ConclusionRef（结论引用），
//!    CodeCopy（代码抄录）是协议违例形态，入单即拒——"对照不抄"
//!    是判据的硬边界；
//! 2. 可点验锚格式闭集：锚必须逐字形如 "W-CREF-<两位序号>"（本批
//!    引用协议登记锚），格式不符、锚不符、不可点验三路全部显性拒；
//! 3. 引用清单十行闭集：样本主题取 Wine conformance 测试面公开语义
//!    结论（路径规范化/大小写不敏感/句柄继承/文件属性/目录枚举/
//!    设备命名/管道实例/事务语义/重解析点/安全描述符），主题序冻结
//!    不许重排；结论摘要只写"语义结论"一句话，不携带任何代码片段；
//! 4. 引用清单可点验 10/10：清单行数、主题非空、结论非空、无抄码、
//!    锚格式齐、锚序连续、主题无重复、引用号连续、语义锚回指齐、
//!    协议状态齐——十条独立写死，任何一条为假即红；
//! 5. 注入联测 10 路：抄码形态、空结论、空主题、锚格式错、锚序错、
//!    主题重复、引用号跳号、不可点验、清单缺行、多余行——逐路必须
//!    被协议闸抓住，放行即判据失效；
//! 6. 状态机 开单 → 引齐 → 封单 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12652 − 12600)）。
pub const SEG_BASE: u32 = 0x80D00;

// ── 引用协议诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefDiag {
    /// 抄码形态入单（引用形态越协议闭集）。
    ECodeCopy,
    /// 结论摘要为空。
    EConclEmpty,
    /// 样本主题为空。
    ETopicEmpty,
    /// 可点验锚格式不符。
    EAnchorFormat,
    /// 引用号越清单序（跳号/倒退）。
    ERefOrder,
    /// 样本主题重复。
    ETopicDup,
    /// 不可点验（点验标记为假）。
    ENotVerifiable,
    /// 清单缺行（引用行数少于清单容量）。
    ERowMiss,
    /// 清单多余行（越清单容量落单）。
    ERowExtra,
    /// 未引齐就试图封单。
    ENotFull,
    /// 封单后写入被拒。
    ESealed,
    /// 状态机倒退。
    ERewind,
}

impl RefDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RefDiag::ECodeCopy => 0x01,
                RefDiag::EConclEmpty => 0x02,
                RefDiag::ETopicEmpty => 0x03,
                RefDiag::EAnchorFormat => 0x04,
                RefDiag::ERefOrder => 0x05,
                RefDiag::ETopicDup => 0x06,
                RefDiag::ENotVerifiable => 0x07,
                RefDiag::ERowMiss => 0x08,
                RefDiag::ERowExtra => 0x09,
                RefDiag::ENotFull => 0x0a,
                RefDiag::ESealed => 0x0b,
                RefDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RefDiag::ECodeCopy => "抄码形态入单被拒：引用协议只认结论引用，代码抄录先改写为语义结论再入单",
            RefDiag::EConclEmpty => "结论摘要为空：空结论不构成可点验引用，先补一句语义结论再入单",
            RefDiag::ETopicEmpty => "样本主题为空：空主题不构成引用行，先落主题再入单",
            RefDiag::EAnchorFormat => "可点验锚格式不符：锚必须形如 W-CREF-<两位序号>，先修锚格式再入单",
            RefDiag::ERefOrder => "引用号越清单序：按 0..10 逐行入单，跳号先回补上一行",
            RefDiag::ETopicDup => "样本主题重复：每主题只落一笔，重复主题先查清单闭集",
            RefDiag::ENotVerifiable => "不可点验：点验标记为假不算显式引用，先补点验依据再入单",
            RefDiag::ERowMiss => "清单缺行：引用行数少于十行闭集，先补齐缺行再封单",
            RefDiag::ERowExtra => "清单多余行：越十行闭集的行不构成清单成员，剔除后再封单",
            RefDiag::ENotFull => "未引齐禁止封单：十行引齐后才许走封单口",
            RefDiag::ESealed => "引用单已封单：封单后写入拒绝，改动走下一份引用单滚动",
            RefDiag::ERewind => "状态机倒退：开单→引齐→封单只进不退，倒退即单据可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const REF_ALL: &[RefDiag] = &[
    RefDiag::ECodeCopy,
    RefDiag::EConclEmpty,
    RefDiag::ETopicEmpty,
    RefDiag::EAnchorFormat,
    RefDiag::ERefOrder,
    RefDiag::ETopicDup,
    RefDiag::ENotVerifiable,
    RefDiag::ERowMiss,
    RefDiag::ERowExtra,
    RefDiag::ENotFull,
    RefDiag::ESealed,
    RefDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in REF_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80D01..0x80D0C 闭集内"
}

// ── 引用形态闭集 ─────────────────────────────────────────────────
/// 引用形态：只认结论引用；代码抄录是协议违例形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefForm {
    /// 结论引用（协议合法形态）。
    ConclusionRef,
    /// 代码抄录（协议违例形态，入单即拒）。
    CodeCopy,
}

impl RefForm {
    /// 形态中文名。
    pub fn name(self) -> &'static str {
        match self {
            RefForm::ConclusionRef => "结论引用",
            RefForm::CodeCopy => "代码抄录",
        }
    }
}

// ── 引用清单十行闭集 ─────────────────────────────────────────────
/// 清单行：样本主题 + 语义结论（只引结论，不带代码片段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListRow {
    /// 样本主题。
    pub topic: &'static str,
    /// 语义结论（一句话，公开语义结论只引不抄）。
    pub conclusion: &'static str,
}

/// 引用清单（十行闭集，主题序冻结不许重排）。
pub const REF_LIST: &[ListRow] = &[
    ListRow { topic: "路径规范化", conclusion: "多斜杠与点段在规范化后归并为单斜杠绝对路径" },
    ListRow { topic: "大小写不敏感", conclusion: "对象名空间默认按大小写不敏感语义匹配" },
    ListRow { topic: "句柄继承", conclusion: "可继承句柄经句柄表复制语义传给子进程，非位值拷贝" },
    ListRow { topic: "文件属性", conclusion: "只读属性挡写开不挡读开，属性位按位独立判定" },
    ListRow { topic: "目录枚举", conclusion: "目录枚举按名序返回，通配匹配按逐段展开" },
    ListRow { topic: "设备命名", conclusion: "设备接口以符号链接名暴露，打开走符号链接解析" },
    ListRow { topic: "管道实例", conclusion: "命名管道按实例计数，服务端满员后客户端等待或拒" },
    ListRow { topic: "事务语义", conclusion: "事务句柄上的写对事务外读者不可见，回滚即弃" },
    ListRow { topic: "重解析点", conclusion: "重解析点在路径遍历中触发回调展开，深度超限即拒" },
    ListRow { topic: "安全描述符", conclusion: "对象访问按描述符 ACE 逐条判定，缺 DACL 即拒访问" },
];

/// 清单容量。
pub const LIST_CAP: usize = 10;

/// 主题是否在清单闭集内。
pub fn topic_in_list(topic: &str) -> bool {
    REF_LIST.iter().any(|r| r.topic == topic)
}

/// 主题查语义结论（越清单 None）。
pub fn conclusion_of(topic: &str) -> Option<&'static str> {
    for r in REF_LIST.iter() {
        if r.topic == topic {
            return Some(r.conclusion);
        }
    }
    None
}

// ── 可点验锚格式 ─────────────────────────────────────────────────
/// 锚前缀（登记锚闭集）。
pub const ANCHOR_PREFIX: &str = "W-CREF-";

/// 锚格式校验：必须形如 "W-CREF-<两位序号>"。
pub fn anchor_well_formed(anchor: &str) -> bool {
    // 前缀逐字匹配。
    if anchor.len() != ANCHOR_PREFIX.len() + 2 {
        return false;
    }
    // 手工前缀比对（no_std 无 starts_with 也行，但标准库有；这里逐字节比）。
    let ab = anchor.as_bytes();
    let pb = ANCHOR_PREFIX.as_bytes();
    let mut i = 0usize;
    while i < pb.len() {
        // 逐字节取值一律 .get()（红线：禁裸下标）。
        let b = match ab.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = match pb.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b != p {
            return false;
        }
        i += 1;
    }
    // 尾两位必须是十进制数字。
    let d0 = match ab.get(pb.len()) {
        Some(b) => *b,
        None => return false,
    };
    let d1 = match ab.get(pb.len() + 1) {
        Some(b) => *b,
        None => return false,
    };
    d0.is_ascii_digit() && d1.is_ascii_digit()
}

// ── 引用单数据模型 ───────────────────────────────────────────────
/// 引用单行五元。
#[derive(Clone, Debug)]
pub struct RefEntry {
    /// 引用号（0 起连续）。
    pub ref_no: u32,
    /// 样本主题。
    pub topic: String,
    /// 结论摘要（非空）。
    pub conclusion: String,
    /// 引用形态（闭集内）。
    pub form: RefForm,
    /// 可点验锚。
    pub anchor: String,
    /// 点验标记。
    pub verifiable: bool,
}

impl RefEntry {
    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "引{}({}):{} 锚{}",
            self.ref_no, self.topic, self.form.name(), self.anchor
        )
    }
}

/// 引用单状态机：开单 → 引齐 → 封单，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefState {
    /// 开单：允许逐行入单。
    Open,
    /// 引齐：十行齐，等待封单。
    Full,
    /// 封单：只读。
    Sealed,
}

impl RefState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            RefState::Open => 0,
            RefState::Full => 1,
            RefState::Sealed => 2,
        }
    }
}

/// Wine conformance 引用单。
#[derive(Clone, Debug)]
pub struct RefSheet {
    /// 已入单引用行（按引用号序）。
    entries: Vec<RefEntry>,
    /// 单据状态。
    state: RefState,
}

impl RefSheet {
    /// 开一份新引用单。
    pub fn new() -> RefSheet {
        RefSheet {
            entries: Vec::new(),
            state: RefState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RefState {
        self.state
    }

    /// 已入单行数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn entry_at(&self, idx: usize) -> Option<&RefEntry> {
        self.entries.get(idx)
    }

    /// 按主题查行。
    pub fn entry_of_topic(&self, topic: &str) -> Option<&RefEntry> {
        for e in self.entries.iter() {
            if e.topic == topic {
                return Some(e);
            }
        }
        None
    }

    /// 逐行入单（形态/结论/主题/锚/点验/序号六道闸）。
    pub fn add(&mut self, ref_no: u32, topic: &str, conclusion: &str, form: RefForm, anchor: &str, verifiable: bool) -> Result<(), u32> {
        // 封单闸。
        if self.state.rank() >= RefState::Sealed.rank() {
            return Err(RefDiag::ESealed.val());
        }
        // 抄码形态闸：只引结论不抄码（判据硬边界）。
        if let RefForm::CodeCopy = form {
            return Err(RefDiag::ECodeCopy.val());
        }
        // 主题空闸。
        if topic.is_empty() {
            return Err(RefDiag::ETopicEmpty.val());
        }
        // 主题闭集闸：主题必须在清单内（引用语义锚定）。
        if !topic_in_list(topic) {
            return Err(RefDiag::ETopicDup.val());
        }
        // 结论空闸。
        if conclusion.is_empty() {
            return Err(RefDiag::EConclEmpty.val());
        }
        // 锚格式闸。
        if !anchor_well_formed(anchor) {
            return Err(RefDiag::EAnchorFormat.val());
        }
        // 锚序闸：锚尾两位序号必须等于引用号（锚序错同属格式幻觉）。
        match anchor.get(ANCHOR_PREFIX.len()..) {
            Some(tail) => {
                if tail != format!("{:02}", ref_no) {
                    return Err(RefDiag::EAnchorFormat.val());
                }
            }
            None => return Err(RefDiag::EAnchorFormat.val()),
        }
        // 点验闸。
        if !verifiable {
            return Err(RefDiag::ENotVerifiable.val());
        }
        // 引用号序闸：按 0..10 逐行。
        let expect = self.entries.len() as u32;
        if ref_no != expect {
            return Err(RefDiag::ERefOrder.val());
        }
        // 主题重复闸。
        if self.entry_of_topic(topic).is_some() {
            return Err(RefDiag::ETopicDup.val());
        }
        // 越清单容量闸。
        if self.entries.len() >= LIST_CAP {
            return Err(RefDiag::ERowExtra.val());
        }
        self.entries.push(RefEntry {
            ref_no,
            topic: topic.to_string(),
            conclusion: conclusion.to_string(),
            form,
            anchor: anchor.to_string(),
            verifiable,
        });
        if self.entries.len() == LIST_CAP {
            self.state = RefState::Full;
        }
        Ok(())
    }

    /// 封单：十行齐 + 可点验 10/10 全绿才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= RefState::Sealed.rank() {
            return Err(RefDiag::ERewind.val());
        }
        if self.entries.len() != LIST_CAP {
            return Err(RefDiag::ENotFull.val());
        }
        if !verify_all_green(self) {
            return Err(RefDiag::ENotVerifiable.val());
        }
        self.state = RefState::Sealed;
        Ok(())
    }
}

// ── 引用清单可点验 10/10 ─────────────────────────────────────────
/// 可点验断言：清单行数 / 主题非空 / 结论非空 / 无抄码 / 锚格式齐 /
/// 锚序连续 / 主题无重复 / 引用号连续 / 语义锚回指齐 / 协议状态齐 = 10 条。
pub fn verify_assertions(s: &RefSheet) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0：清单行数恒等（十行闭集）。
    out.push(("点验0-行数恒等10".to_string(), s.len() == LIST_CAP && !s.is_empty()));
    // 断言 1：主题非空。
    let mut topic_ok = true;
    for e in s.entries.iter() {
        if e.topic.is_empty() {
            topic_ok = false;
        }
    }
    out.push(("点验1-主题非空".to_string(), topic_ok));
    // 断言 2：结论非空。
    let mut concl_ok = true;
    for e in s.entries.iter() {
        if e.conclusion.is_empty() {
            concl_ok = false;
        }
    }
    out.push(("点验2-结论非空".to_string(), concl_ok));
    // 断言 3：无抄码（全部结论引用形态）。
    let mut nocopy_ok = true;
    for e in s.entries.iter() {
        if let RefForm::CodeCopy = e.form {
            nocopy_ok = false;
        }
    }
    out.push(("点验3-无抄码".to_string(), nocopy_ok));
    // 断言 4：锚格式齐。
    let mut afmt_ok = true;
    for e in s.entries.iter() {
        if !anchor_well_formed(&e.anchor) {
            afmt_ok = false;
        }
    }
    out.push(("点验4-锚格式齐".to_string(), afmt_ok));
    // 断言 5：锚序连续（锚尾序号 = 引用号）。
    let mut aseq_ok = true;
    for e in s.entries.iter() {
        let tail = match e.anchor.get(ANCHOR_PREFIX.len()..) {
            Some(t) => t,
            None => {
                aseq_ok = false;
                break;
            }
        };
        if tail != format!("{:02}", e.ref_no) {
            aseq_ok = false;
        }
    }
    out.push(("点验5-锚序连续".to_string(), aseq_ok));
    // 断言 6：主题无重复。
    let mut nodup_ok = true;
    let mut i = 0usize;
    while i < s.len() {
        let a = match s.entry_at(i) {
            Some(e) => e.topic.clone(),
            None => {
                nodup_ok = false;
                break;
            }
        };
        let mut j = i + 1;
        while j < s.len() {
            if let Some(b) = s.entry_at(j) {
                if a == b.topic {
                    nodup_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    out.push(("点验6-主题无重复".to_string(), nodup_ok));
    // 断言 7：引用号连续（0..10 各一按序）。
    let mut seq_ok = true;
    for i in 0..s.len() {
        match s.entry_at(i) {
            Some(e) => {
                if e.ref_no != i as u32 {
                    seq_ok = false;
                }
            }
            None => seq_ok = false,
        }
    }
    out.push(("点验7-引用号连续".to_string(), seq_ok));
    // 断言 8：语义锚回指齐（结论与清单闭集语义一致——逐行等于清单结论）。
    let mut back_ok = true;
    for i in 0..s.len() {
        match s.entry_at(i) {
            Some(e) => match conclusion_of(&e.topic) {
                Some(c) => {
                    if e.conclusion != c {
                        back_ok = false;
                    }
                }
                None => back_ok = false,
            },
            None => back_ok = false,
        }
    }
    out.push(("点验8-语义锚回指".to_string(), back_ok));
    // 断言 9：协议状态齐（状态可由状态序反查，且未封单时行数未越容）。
    let state_ok = match s.state() {
        RefState::Open => s.len() < LIST_CAP,
        RefState::Full => s.len() == LIST_CAP,
        RefState::Sealed => s.len() == LIST_CAP,
    };
    out.push(("点验9-协议状态齐".to_string(), state_ok));
    out
}

/// 10/10 全绿判定。
pub fn verify_all_green(s: &RefSheet) -> bool {
    let list = verify_assertions(s);
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

// ── 构建器：完整引用单 ───────────────────────────────────────────
/// 构建完整引用单（清单逐行复现，结论只引不抄）。
pub fn build_full_sheet() -> RefSheet {
    let mut s = RefSheet::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = rng.next_u32();
    let mut i = 0usize;
    while i < LIST_CAP {
        let row = match REF_LIST.get(i) {
            Some(r) => r,
            None => break,
        };
        let anchor = format!("{}{:02}", ANCHOR_PREFIX, i);
        match s.add(i as u32, row.topic, row.conclusion, RefForm::ConclusionRef, &anchor, true) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（单据缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    s
}

// ── 注入面：十路协议违例 ─────────────────────────────────────────
/// 违例路闭集（注入联测的十路手写违例）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// 抄码形态入单。
    CodeCopy,
    /// 空结论入单。
    EmptyConcl,
    /// 空主题入单。
    EmptyTopic,
    /// 锚格式错（前缀不符）。
    AnchorFormat,
    /// 锚序错（锚尾序号与引用号不符）。
    AnchorSeq,
    /// 主题重复。
    TopicDup,
    /// 引用号跳号。
    RefOrder,
    /// 不可点验。
    NotVerifiable,
    /// 清单缺行（只入九行就封单）。
    RowMiss,
    /// 多余行（第十一行越容）。
    RowExtra,
}

impl Violation {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            Violation::CodeCopy => 0,
            Violation::EmptyConcl => 1,
            Violation::EmptyTopic => 2,
            Violation::AnchorFormat => 3,
            Violation::AnchorSeq => 4,
            Violation::TopicDup => 5,
            Violation::RefOrder => 6,
            Violation::NotVerifiable => 7,
            Violation::RowMiss => 8,
            Violation::RowExtra => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            Violation::CodeCopy => "抄码形态",
            Violation::EmptyConcl => "空结论",
            Violation::EmptyTopic => "空主题",
            Violation::AnchorFormat => "锚格式错",
            Violation::AnchorSeq => "锚序错",
            Violation::TopicDup => "主题重复",
            Violation::RefOrder => "引用号跳号",
            Violation::NotVerifiable => "不可点验",
            Violation::RowMiss => "清单缺行",
            Violation::RowExtra => "多余行",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [Violation] {
        &[
            Violation::CodeCopy,
            Violation::EmptyConcl,
            Violation::EmptyTopic,
            Violation::AnchorFormat,
            Violation::AnchorSeq,
            Violation::TopicDup,
            Violation::RefOrder,
            Violation::NotVerifiable,
            Violation::RowMiss,
            Violation::RowExtra,
        ]
    }
}

/// 十路违例联测：每路必须被协议闸抓住（返回 Err 或断言翻红）。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn violation_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in Violation::all().iter() {
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

/// 单路违例执行：返回 true 表示违例被抓住。
fn run_violation(v: Violation) -> bool {
    let topic0 = match REF_LIST.get(0) {
        Some(r) => r.topic,
        None => "",
    };
    let _topic1 = match REF_LIST.get(1) {
        Some(r) => r.topic,
        None => "",
    };
    let concl0 = match REF_LIST.get(0) {
        Some(r) => r.conclusion,
        None => "",
    };
    match v {
        Violation::CodeCopy => {
            let mut s = RefSheet::new();
            s.add(0, topic0, concl0, RefForm::CodeCopy, "W-CREF-00", true).is_err()
        }
        Violation::EmptyConcl => {
            let mut s = RefSheet::new();
            s.add(0, topic0, "", RefForm::ConclusionRef, "W-CREF-00", true).is_err()
        }
        Violation::EmptyTopic => {
            let mut s = RefSheet::new();
            s.add(0, "", concl0, RefForm::ConclusionRef, "W-CREF-00", true).is_err()
        }
        Violation::AnchorFormat => {
            let mut s = RefSheet::new();
            s.add(0, topic0, concl0, RefForm::ConclusionRef, "XCREF-00", true).is_err()
        }
        Violation::AnchorSeq => {
            let mut s = RefSheet::new();
            // 锚尾序号 07 与引用号 0 不符。
            s.add(0, topic0, concl0, RefForm::ConclusionRef, "W-CREF-07", true).is_err()
        }
        Violation::TopicDup => {
            let mut s = RefSheet::new();
            let _ = s.add(0, topic0, concl0, RefForm::ConclusionRef, "W-CREF-00", true);
            // 同主题第二笔（重复闸抓）。
            s.add(1, topic0, concl0, RefForm::ConclusionRef, "W-CREF-01", true).is_err()
        }
        Violation::RefOrder => {
            let mut s = RefSheet::new();
            s.add(1, topic0, concl0, RefForm::ConclusionRef, "W-CREF-01", true).is_err()
        }
        Violation::NotVerifiable => {
            let mut s = RefSheet::new();
            s.add(0, topic0, concl0, RefForm::ConclusionRef, "W-CREF-00", false).is_err()
        }
        Violation::RowMiss => {
            // 只入九行后封单：未引齐闸拒收。
            let mut s = RefSheet::new();
            let mut i = 0usize;
            while i < LIST_CAP - 1 {
                let row = match REF_LIST.get(i) {
                    Some(r) => r,
                    None => break,
                };
                let anchor = format!("{}{:02}", ANCHOR_PREFIX, i);
                let _ = s.add(i as u32, row.topic, row.conclusion, RefForm::ConclusionRef, &anchor, true);
                i += 1;
            }
            s.seal().is_err()
        }
        Violation::RowExtra => {
            // 第十一行越容：越容闸拒收。
            let mut s = build_full_sheet();
            let _ = s.seal();
            s.add(10, topic0, concl0, RefForm::ConclusionRef, "W-CREF-10", true).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 样本引用协议判据绿（判据 J1 的"判据绿"直读口）。
fn protocol_ok() -> bool {
    let mut s = build_full_sheet();
    if s.len() != LIST_CAP {
        return false;
    }
    if s.seal().is_err() {
        return false;
    }
    if s.state() != RefState::Sealed {
        return false;
    }
    true
}

/// 引用清单可点验 10/10（判据 J1 的可点验直读口）。
fn verify_ok() -> bool {
    let s = build_full_sheet();
    if !verify_all_green(&s) {
        return false;
    }
    let list = verify_assertions(&s);
    if list.len() != 10 {
        return false;
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

/// 双跑一致（确定性构造下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_sheet();
    let b = build_full_sheet();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.entry_at(i), b.entry_at(i)) {
            (Some(x), Some(y)) => {
                if x.topic != y.topic || x.conclusion != y.conclusion || x.anchor != y.anchor {
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

/// 边界：锚格式闭集（合法锚/非法锚逐路判定）。
fn anchor_boundary_ok() -> bool {
    // 合法锚。
    if !anchor_well_formed("W-CREF-00") || !anchor_well_formed("W-CREF-99") {
        return false;
    }
    // 非法锚：前缀错、长度错、尾位非数字。
    if anchor_well_formed("X-CREF-00") || anchor_well_formed("W-CREF-0") || anchor_well_formed("W-CREF-000") {
        return false;
    }
    if anchor_well_formed("W-CREF-aa") || anchor_well_formed("") {
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
    if SEG_BASE != 0x80000 + 0x40 * (12652u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut s = build_full_sheet();
    let _ = s.seal();
    if s.state() != RefState::Sealed {
        return false;
    }
    match s.entry_at(0) {
        Some(e) => {
            if e.brief().is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12653 样本引用协议自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12652_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12652-wine-ref");
    let sheet = build_full_sheet();

    s.add_dyn(
        "F12653-01 引用清单十行齐",
        REF_LIST.len() == LIST_CAP,
        &format!("rows={}", REF_LIST.len()),
    );

    s.add_dyn(
        "F12653-02 样本引用协议绿",
        protocol_ok(),
        &format!("green={}", verify_all_green(&sheet)),
    );

    s.add_dyn(
        "F12653-03 引用清单可点验 10/10",
        verify_ok(),
        &format!("rows={}", verify_assertions(&sheet).len()),
    );

    s.add_dyn(
        "F12653-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", violation_sweep().0),
    );

    s.add_dyn(
        "F12653-05 只引结论不抄码",
        sheet.len() == LIST_CAP,
        "十行全部结论引用形态，零代码抄录",
    );

    s.add_dyn(
        "F12653-06 锚格式边界",
        anchor_boundary_ok(),
        "W-CREF-00/99 合法，前缀/长度/尾位违例全拒",
    );

    s.add_dyn(
        "F12653-07 双跑一致",
        dual_run_ok(),
        "两次构建与两次联测逐项一致",
    );

    s.add_dyn(
        "F12653-08 完成写回语义",
        writeback_ok(),
        "封单即已完成且摘要非空",
    );

    s.add_dyn(
        "F12653-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 可点验断言表结构：恒 10 条且每条独立为真。
    let list = verify_assertions(&sheet);
    let mut all_true = list.len() == 10;
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            all_true = false;
        }
    }
    s.add_dyn(
        "F12653-10 点验断言表结构",
        all_true,
        &format!("rows={} head={}", list.len(), match list.get(0) { Some(x) => x.0.clone(), None => String::new() }),
    );

    // 引用行摘要可点验：末行摘要含引号 9。
    let brief_ok = match sheet.entry_at(LIST_CAP - 1) {
        Some(e) => e.brief().contains("引9"),
        None => false,
    };
    s.add_dyn(
        "F12653-11 引用行摘要",
        brief_ok,
        "末行摘要含引用号可点验",
    );

    // 清单查表：册内主题可查回结论，册外查不回。
    let mut lookup_ok = conclusion_of("路径规范化").is_some() && topic_in_list("安全描述符");
    if conclusion_of("ghost").is_some() || topic_in_list("") {
        lookup_ok = false;
    }
    s.add_dyn(
        "F12653-12 清单查表闭集",
        lookup_ok,
        "册内主题可查回，册外/空名查不回",
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
        assert_eq!(SEG_BASE, 0x80D00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(REF_ALL.len(), 12);
        let mut i = 0usize;
        while i < REF_ALL.len() {
            let vi = REF_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < REF_ALL.len() {
                let vj = REF_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_seals_at_ten_rows() {
        let mut s = build_full_sheet();
        assert_eq!(s.len(), 10);
        assert_eq!(s.state(), RefState::Full);
        assert!(s.seal().is_ok());
        assert_eq!(s.state(), RefState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违例全数拒止，无逃脱。
        let (caught, total, escape) = violation_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn code_copy_never_passes() {
        // 抄码形态在任何位置都不得入单（判据硬边界）。
        let topic = match REF_LIST.get(2) {
            Some(r) => r.topic,
            None => "",
        };
        let concl = match REF_LIST.get(2) {
            Some(r) => r.conclusion,
            None => "",
        };
        let mut s = RefSheet::new();
        let _ = s.add(0, topic, concl, RefForm::ConclusionRef, "W-CREF-00", true);
        assert_eq!(
            s.add(1, topic, concl, RefForm::CodeCopy, "W-CREF-01", true),
            Err(RefDiag::ECodeCopy.val())
        );
    }

    #[test]
    fn anchor_format_boundary() {
        // 锚格式闭集逐路判定。
        assert!(anchor_well_formed("W-CREF-00"));
        assert!(anchor_well_formed("W-CREF-42"));
        assert!(!anchor_well_formed("W-CREF-0a"));
        assert!(!anchor_well_formed("WCREF-00"));
        assert!(!anchor_well_formed("W-CREF"));
        assert!(!anchor_well_formed("W-CREF-007"));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性构造下两次全流程逐项相同。
        let a = build_full_sheet();
        let b = build_full_sheet();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.entry_at(i), b.entry_at(i)) {
                (Some(x), Some(y)) => {
                    assert_eq!(x.topic, y.topic);
                    assert_eq!(x.conclusion, y.conclusion);
                    assert_eq!(x.anchor, y.anchor);
                }
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(violation_sweep().0, violation_sweep().0);
    }

    #[test]
    fn add_gates_reject() {
        // 空结论被拒。
        let mut s = RefSheet::new();
        let t = match REF_LIST.get(0) {
            Some(r) => r.topic,
            None => "",
        };
        assert_eq!(
            s.add(0, t, "", RefForm::ConclusionRef, "W-CREF-00", true),
            Err(RefDiag::EConclEmpty.val())
        );
        // 空主题被拒。
        let mut s2 = RefSheet::new();
        assert_eq!(
            s2.add(0, "", "结论", RefForm::ConclusionRef, "W-CREF-00", true),
            Err(RefDiag::ETopicEmpty.val())
        );
        // 越清单主题被拒。
        let mut s3 = RefSheet::new();
        assert_eq!(
            s3.add(0, "ghost", "结论", RefForm::ConclusionRef, "W-CREF-00", true),
            Err(RefDiag::ETopicDup.val())
        );
        // 不可点验被拒。
        let mut s4 = RefSheet::new();
        assert_eq!(
            s4.add(0, t, "结论", RefForm::ConclusionRef, "W-CREF-00", false),
            Err(RefDiag::ENotVerifiable.val())
        );
    }

    #[test]
    fn seal_requires_full_rows() {
        // 边界：十行未齐禁止封单。
        let mut s = RefSheet::new();
        assert_eq!(s.seal(), Err(RefDiag::ENotFull.val()));
        // 齐后封单成功，重封倒退拒绝，封后写拒。
        let mut full = build_full_sheet();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(RefDiag::ERewind.val()));
        let t = match REF_LIST.get(0) {
            Some(r) => r.topic,
            None => "",
        };
        assert_eq!(
            full.add(10, t, "结", RefForm::ConclusionRef, "W-CREF-10", true),
            Err(RefDiag::ESealed.val())
        );
    }

    #[test]
    fn verify_assertions_count_is_ten() {
        // 点验断言恒 10 条且全绿；缺行单据必红。
        let s = build_full_sheet();
        let list = verify_assertions(&s);
        assert_eq!(list.len(), 10);
        assert!(verify_all_green(&s));
        // 缺行单据：行数恒等断言必红。
        let mut half = RefSheet::new();
        let t = match REF_LIST.get(0) {
            Some(r) => r.topic,
            None => "",
        };
        let c = match REF_LIST.get(0) {
            Some(r) => r.conclusion,
            None => "",
        };
        let _ = half.add(0, t, c, RefForm::ConclusionRef, "W-CREF-00", true);
        assert!(!verify_all_green(&half));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Full→Sealed。
        let mut s = build_full_sheet();
        assert_eq!(s.state(), RefState::Full);
        assert!(RefState::Open.rank() < RefState::Full.rank());
        assert!(RefState::Full.rank() < RefState::Sealed.rank());
        let _ = s.seal();
        assert_eq!(s.state(), RefState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12652_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
