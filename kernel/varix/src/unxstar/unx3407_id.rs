//! UNX-F3407 用例元数据（域/批/ID/优先级/标签） · 0x79500段
//!
//! 判据原文（UNX-F3407 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：五元数据（域/批/ID/优先级/标签/kind）结构与校验（域枚举合法）；
//!     M-166 元数据全字段验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 五元数据闭集结构：域（枚举闭集）/批（B+数字）/ID（非零 u32）/
//!    优先级（枚举闭集）/标签（非空互异、定容）+ kind（枚举闭集）六面
//!    一次成条，字段全带类型闭集而非裸字符串；
//! 2. 域枚举合法：域以闭集枚举承载（内存/调度/文件/安全/电源），raw 索引
//!    入口 from_idx 越界返回 None——非法索引显性拒，不静默折叠；
//! 3. M-166 全字段验证：八档语料（齐档/越界域/坏批格式/零ID/非法优先级/
//!    空标签/重标签/非法kind）逐档过闸，齐档必过、缺档必拒、拒码逐档可辨；
//! 4. 元数据台账：批+ID 二元组唯一（同批同 ID 显性拒），台账序号连续，
//!    分域聚合摘要非空（写回面）；
//! 5. 注入矩阵 8 项：越界域/坏批/零ID/重标签/空标签/同批同ID/台账越位/
//!    封账后写入逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x79501..0x7950C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79500;

/// M-166 语料档数。
pub const M166_CASES: usize = 8;

/// 标签容量（定容防膨胀）。
pub const TAG_CAP: usize = 4;

/// 元数据诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 域枚举越界：raw 索引不在域闭集内。
    EBadDomain,
    /// 批格式非法：批须为 B+数字。
    EBadBatch,
    /// ID 为零：ID 必须非零。
    EZeroId,
    /// 优先级非法：优先级不在闭集内。
    EBadPriority,
    /// 标签空：至少一个标签。
    ENoTag,
    /// 标签重复：标签互异。
    EDupTag,
    /// 标签超容：标签数超 TAG_CAP。
    ETagFull,
    /// kind 非法：kind 不在闭集内。
    EBadKind,
    /// 同批同 ID：台账二元组唯一。
    EDupKey,
    /// 台账越位：取账下标越界。
    ERange,
    /// 账序断裂：台账序号不连续。
    ESeq,
    /// 封账后写入：账已封只读。
    ESealed,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EBadDomain => 0x01,
                Diag::EBadBatch => 0x02,
                Diag::EZeroId => 0x03,
                Diag::EBadPriority => 0x04,
                Diag::ENoTag => 0x05,
                Diag::EDupTag => 0x06,
                Diag::ETagFull => 0x07,
                Diag::EBadKind => 0x08,
                Diag::EDupKey => 0x09,
                Diag::ERange => 0x0a,
                Diag::ESeq => 0x0b,
                Diag::ESealed => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EBadDomain => "域枚举越界：域索引不在 内存/调度/文件/安全/电源 闭集内，按名册回填",
            Diag::EBadBatch => "批格式非法：批须为 B+数字（如 B10），按批册回填",
            Diag::EZeroId => "ID 为零：ID 必须非零，按用例册回填",
            Diag::EBadPriority => "优先级非法：优先级在 高/中/低 闭集内取值",
            Diag::ENoTag => "标签空：至少落一个标签，按用例语义回填",
            Diag::EDupTag => "标签重复：标签互异，去重后再入账",
            Diag::ETagFull => "标签超容：标签数超容量，精简到语义必要项",
            Diag::EBadKind => "kind 非法：kind 在 单元/集成/注入 闭集内取值",
            Diag::EDupKey => "同批同 ID：批+ID 二元组唯一，换 ID 或换批再入账",
            Diag::ERange => "取账越位：下标越出台账，按台账长度取账",
            Diag::ESeq => "账序断裂：台账序号须从一起连续递增，重建台账",
            Diag::ESealed => "账已封：封账后只读，写入走新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EBadDomain,
    Diag::EBadBatch,
    Diag::EZeroId,
    Diag::EBadPriority,
    Diag::ENoTag,
    Diag::EDupTag,
    Diag::ETagFull,
    Diag::EBadKind,
    Diag::EDupKey,
    Diag::ERange,
    Diag::ESeq,
    Diag::ESealed,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79501..0x7950C 闭集内"
}

/// 域枚举闭集（域枚举合法的载体——非法索引在构造口即拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainKind {
    /// 内存域。
    Mem,
    /// 调度域。
    Sched,
    /// 文件域。
    File,
    /// 安全域。
    Sec,
    /// 电源域。
    Power,
}

impl DomainKind {
    /// 由 raw 索引反查（越界返回 None——显性拒）。
    pub fn from_idx(idx: u8) -> Option<DomainKind> {
        match idx {
            0 => Some(DomainKind::Mem),
            1 => Some(DomainKind::Sched),
            2 => Some(DomainKind::File),
            3 => Some(DomainKind::Sec),
            4 => Some(DomainKind::Power),
            _ => None,
        }
    }

    /// 域索引。
    pub fn idx(self) -> u8 {
        match self {
            DomainKind::Mem => 0,
            DomainKind::Sched => 1,
            DomainKind::File => 2,
            DomainKind::Sec => 3,
            DomainKind::Power => 4,
        }
    }

    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            DomainKind::Mem => "内存",
            DomainKind::Sched => "调度",
            DomainKind::File => "文件",
            DomainKind::Sec => "安全",
            DomainKind::Power => "电源",
        }
    }
}

/// 域闭集名册（与 DomainKind 逐位对齐）。
pub const DOMAIN_NAMES: [&str; 5] = ["内存", "调度", "文件", "安全", "电源"];

/// 优先级枚举闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// 高优先级。
    High,
    /// 中优先级。
    Mid,
    /// 低优先级。
    Low,
}

impl Priority {
    /// 由 raw 索引反查（越界返回 None）。
    pub fn from_idx(idx: u8) -> Option<Priority> {
        match idx {
            0 => Some(Priority::High),
            1 => Some(Priority::Mid),
            2 => Some(Priority::Low),
            _ => None,
        }
    }

    /// 序数（排序面用，高在前）。
    pub fn ord(self) -> u8 {
        match self {
            Priority::High => 0,
            Priority::Mid => 1,
            Priority::Low => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Priority::High => "高",
            Priority::Mid => "中",
            Priority::Low => "低",
        }
    }
}

/// kind 枚举闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 单元用例。
    Unit,
    /// 集成用例。
    Integ,
    /// 注入用例。
    Inject,
}

impl CaseKind {
    /// 由 raw 索引反查（越界返回 None）。
    pub fn from_idx(idx: u8) -> Option<CaseKind> {
        match idx {
            0 => Some(CaseKind::Unit),
            1 => Some(CaseKind::Integ),
            2 => Some(CaseKind::Inject),
            _ => None,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            CaseKind::Unit => "单元",
            CaseKind::Integ => "集成",
            CaseKind::Inject => "注入",
        }
    }
}

/// 批格式校验：B 开头且余部非空全数字。
pub fn batch_legal(batch: &str) -> bool {
    let bytes = batch.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'B') {
        return false;
    }
    let mut i = 1usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b < b'0' || *b > b'9' {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 五元数据（域/批/ID/优先级/标签）+ kind，一次成条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseMeta {
    /// 域（枚举闭集）。
    pub domain: DomainKind,
    /// 批（B+数字）。
    pub batch: String,
    /// 用例 ID（非零）。
    pub id: u32,
    /// 优先级（枚举闭集）。
    pub priority: Priority,
    /// 标签（非空互异、定容）。
    pub tags: Vec<String>,
    /// kind（枚举闭集）。
    pub kind: CaseKind,
}

impl CaseMeta {
    /// M-166 全字段校验：六面逐面过闸，拒码逐面可辨。
    pub fn validate(&self) -> Result<(), u32> {
        if !batch_legal(&self.batch) {
            return Err(Diag::EBadBatch.val());
        }
        if self.id == 0 {
            return Err(Diag::EZeroId.val());
        }
        if self.tags.is_empty() {
            return Err(Diag::ENoTag.val());
        }
        if self.tags.len() > TAG_CAP {
            return Err(Diag::ETagFull.val());
        }
        let mut i = 0usize;
        while i < self.tags.len() {
            let t = match self.tags.get(i) {
                Some(x) => x,
                None => return Err(Diag::ENoTag.val()),
            };
            if t.is_empty() {
                return Err(Diag::ENoTag.val());
            }
            let mut j = i + 1;
            while j < self.tags.len() {
                let u = match self.tags.get(j) {
                    Some(x) => x,
                    None => return Err(Diag::ENoTag.val()),
                };
                if t == u {
                    return Err(Diag::EDupTag.val());
                }
                j += 1;
            }
            i += 1;
        }
        // 域/优先级/kind 是枚举闭集——构造口已合法，此处以名册对齐复核。
        let want = match DOMAIN_NAMES.get(self.domain.idx() as usize) {
            Some(x) => *x,
            None => return Err(Diag::EBadDomain.val()),
        };
        if self.domain.name() != want {
            return Err(Diag::EBadDomain.val());
        }
        if self.priority.ord() > 2 {
            return Err(Diag::EBadPriority.val());
        }
        if self.kind.name().is_empty() {
            return Err(Diag::EBadKind.val());
        }
        Ok(())
    }

    /// 元数据单行摘要（写回面：域/批/ID/优先级/标签/kind 全入行）。
    pub fn summary_line(&self) -> String {
        let mut tag_txt = String::new();
        let mut i = 0usize;
        while i < self.tags.len() {
            if i > 0 {
                tag_txt.push_str("|");
            }
            tag_txt.push_str(self.tags.get(i).map(|x| x.as_str()).unwrap_or(""));
            i += 1;
        }
        format!(
            "{} 批{} ID{} 优先{} kind{} 标签[{}]",
            self.domain.name(),
            self.batch,
            self.id,
            self.priority.name(),
            self.kind.name(),
            tag_txt
        )
    }

    /// 台账键（批+ID 二元组文本）。
    pub fn key(&self) -> String {
        format!("{}#{}", self.batch, self.id)
    }
}

/// M-166 语料档（齐档 + 各缺面档）。
pub struct M166Case {
    /// 档名。
    pub name: &'static str,
    /// raw 元数据（域/批/ID/优先级/标签/kind 逐面 raw 化）。
    pub raw: RawMeta,
    /// 钦定拒码（None=必过）。
    pub want_reject: Option<Diag>,
}

/// raw 元数据（六面 raw 化——非法索引在构造口显性拒）。
pub struct RawMeta {
    /// 域 raw 索引。
    pub domain_idx: u8,
    /// 批文本。
    pub batch: &'static str,
    /// ID。
    pub id: u32,
    /// 优先级 raw 索引。
    pub priority_idx: u8,
    /// 标签册。
    pub tags: [&'static str; TAG_CAP],
    /// 标签有效数。
    pub tag_n: usize,
    /// kind raw 索引。
    pub kind_idx: u8,
}

/// raw → CaseMeta（域/优先级/kind 枚举构造口越界即拒）。
pub fn meta_from_raw(r: &RawMeta) -> Result<CaseMeta, u32> {
    let domain = match DomainKind::from_idx(r.domain_idx) {
        Some(x) => x,
        None => return Err(Diag::EBadDomain.val()),
    };
    let priority = match Priority::from_idx(r.priority_idx) {
        Some(x) => x,
        None => return Err(Diag::EBadPriority.val()),
    };
    let kind = match CaseKind::from_idx(r.kind_idx) {
        Some(x) => x,
        None => return Err(Diag::EBadKind.val()),
    };
    let mut tags = Vec::new();
    let mut i = 0usize;
    while i < r.tag_n {
        match r.tags.get(i) {
            Some(t) => tags.push(String::from(*t)),
            None => break,
        }
        i += 1;
    }
    Ok(CaseMeta {
        domain,
        batch: String::from(r.batch),
        id: r.id,
        priority,
        tags,
        kind,
    })
}

/// 标准齐档 raw。
fn good_raw() -> RawMeta {
    RawMeta {
        domain_idx: 0,
        batch: "B10",
        id: 7,
        priority_idx: 0,
        tags: ["等值", "首差异", "", ""],
        tag_n: 2,
        kind_idx: 0,
    }
}

/// M-166 八档语料。
pub fn m166_corpus() -> Vec<M166Case> {
    let mut out = Vec::new();
    // 档1：齐档。
    out.push(M166Case {
        name: "齐档",
        raw: good_raw(),
        want_reject: None,
    });
    // 档2：域枚举越界。
    let mut r = good_raw();
    r.domain_idx = 9;
    out.push(M166Case {
        name: "越界域",
        raw: r,
        want_reject: Some(Diag::EBadDomain),
    });
    // 档3：坏批格式（小写 b）。
    let mut r = good_raw();
    r.batch = "b10";
    out.push(M166Case {
        name: "坏批格式",
        raw: r,
        want_reject: Some(Diag::EBadBatch),
    });
    // 档4：零 ID。
    let mut r = good_raw();
    r.id = 0;
    out.push(M166Case {
        name: "零ID",
        raw: r,
        want_reject: Some(Diag::EZeroId),
    });
    // 档5：非法优先级。
    let mut r = good_raw();
    r.priority_idx = 7;
    out.push(M166Case {
        name: "非法优先级",
        raw: r,
        want_reject: Some(Diag::EBadPriority),
    });
    // 档6：空标签。
    let mut r = good_raw();
    r.tag_n = 0;
    out.push(M166Case {
        name: "空标签",
        raw: r,
        want_reject: Some(Diag::ENoTag),
    });
    // 档7：重标签。
    let mut r = good_raw();
    r.tags = ["等值", "等值", "", ""];
    out.push(M166Case {
        name: "重标签",
        raw: r,
        want_reject: Some(Diag::EDupTag),
    });
    // 档8：非法 kind。
    let mut r = good_raw();
    r.kind_idx = 5;
    out.push(M166Case {
        name: "非法kind",
        raw: r,
        want_reject: Some(Diag::EBadKind),
    });
    out
}

/// M-166 全字段验证：逐档过闸，齐档必过、缺档必拒且拒码可辨。
pub fn m166_verify() -> (bool, Vec<bool>) {
    let corpus = m166_corpus();
    let mut per: Vec<bool> = Vec::new();
    if corpus.len() != M166_CASES {
        return (false, per);
    }
    let mut all = true;
    for c in &corpus {
        let got = meta_from_raw(&c.raw).and_then(|m| m.validate());
        let ok = match c.want_reject {
            None => got.is_ok(),
            Some(w) => got == Err(w.val()),
        };
        per.push(ok);
        if !ok {
            all = false;
        }
    }
    (all, per)
}

/// 元数据台账（批+ID 二元组唯一、序号连续、封账只读）。
#[derive(Clone, Debug)]
pub struct MetaLedger {
    sealed: bool,
    entries: Vec<(u32, CaseMeta)>,
    rejects: u32,
}

impl MetaLedger {
    /// 新台账。
    pub fn new() -> MetaLedger {
        MetaLedger {
            sealed: false,
            entries: Vec::new(),
            rejects: 0,
        }
    }

    /// 已入账条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 入账：校验闸 + 批ID 唯一 + 封账拒；序号连续递增。
    pub fn push(&mut self, m: CaseMeta) -> Result<u32, u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        m.validate()?;
        let key = m.key();
        for (_, x) in &self.entries {
            if x.key() == key {
                self.rejects += 1;
                return Err(Diag::EDupKey.val());
            }
        }
        let seq = (self.entries.len() as u32) + 1;
        self.entries.push((seq, m));
        Ok(seq)
    }

    /// 取账（越位返回 Err(ERange)）。
    pub fn at(&self, idx: usize) -> Result<(u32, &CaseMeta), u32> {
        if idx >= self.entries.len() {
            return Err(Diag::ERange.val());
        }
        match self.entries.get(idx) {
            Some((seq, m)) => Ok((*seq, m)),
            None => Err(Diag::ERange.val()),
        }
    }

    /// 账序自检。
    pub fn seq_ok(&self) -> bool {
        let mut want = 1u32;
        for (seq, _) in &self.entries {
            if *seq != want {
                return false;
            }
            want += 1;
        }
        true
    }

    /// 封账（单向）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 分域聚合摘要（写回面，非空）。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        let mut di = 0u8;
        while di < 5 {
            let name = match DOMAIN_NAMES.get(di as usize) {
                Some(x) => *x,
                None => "",
            };
            let mut n = 0usize;
            for (_, m) in &self.entries {
                if m.domain.idx() == di {
                    n += 1;
                }
            }
            if n > 0 {
                out.push_str(&format!("域{} 条{}\n", name, n));
            }
            di += 1;
        }
        out.push_str(&format!("账{} 拒{}\n", self.entries.len(), self.rejects));
        out
    }
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：越界域（raw 索引 255）。
    let mut r = good_raw();
    r.domain_idx = 255;
    mark(
        &mut out,
        0,
        meta_from_raw(&r) == Err(Diag::EBadDomain.val()),
    );
    // 注入2：坏批（无数字尾）。
    let mut r = good_raw();
    r.batch = "B";
    let bad_batch = meta_from_raw(&r)
        .and_then(|m| m.validate())
        == Err(Diag::EBadBatch.val());
    mark(&mut out, 1, bad_batch);
    // 注入3：零 ID。
    let mut r = good_raw();
    r.id = 0;
    let zero_id = meta_from_raw(&r)
        .and_then(|m| m.validate())
        == Err(Diag::EZeroId.val());
    mark(&mut out, 2, zero_id);
    // 注入4：重标签。
    let mut r = good_raw();
    r.tags = ["甲", "甲", "", ""];
    let dup_tag = meta_from_raw(&r)
        .and_then(|m| m.validate())
        == Err(Diag::EDupTag.val());
    mark(&mut out, 3, dup_tag);
    // 注入5：空标签册。
    let mut r = good_raw();
    r.tag_n = 0;
    let no_tag = meta_from_raw(&r)
        .and_then(|m| m.validate())
        == Err(Diag::ENoTag.val());
    mark(&mut out, 4, no_tag);
    // 注入6：同批同 ID。
    let mut led = MetaLedger::new();
    let m1 = meta_from_raw(&good_raw());
    let m2 = meta_from_raw(&good_raw());
    if let (Ok(a), Ok(b)) = (m1, m2) {
        let _ = led.push(a);
        mark(&mut out, 5, led.push(b) == Err(Diag::EDupKey.val()));
    }
    // 注入7：台账越位取账。
    mark(
        &mut out,
        6,
        led.at(9) == Err(Diag::ERange.val()) && led.len() == 1,
    );
    // 注入8：封账后写入。
    let mut sealed = MetaLedger::new();
    if let Ok(m) = meta_from_raw(&good_raw()) {
        let _ = sealed.push(m);
    }
    let _ = sealed.seal();
    if let Ok(m) = meta_from_raw(&good_raw()) {
        let mut m2 = m.clone();
        m2.id = m2.id + 1;
        mark(&mut out, 7, sealed.push(m2) == Err(Diag::ESealed.val()));
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79500
}

/// 枚举闭集三面自洽（域/优先级/kind 的 from_idx↔idx↔name 往返对齐）。
fn enums_closed_ok() -> bool {
    if DOMAIN_NAMES.len() != 5 {
        return false;
    }
    let mut i = 0u8;
    while i < 5 {
        let d = match DomainKind::from_idx(i) {
            Some(x) => x,
            None => return false,
        };
        let want = match DOMAIN_NAMES.get(i as usize) {
            Some(x) => *x,
            None => return false,
        };
        if d.idx() != i || d.name() != want {
            return false;
        }
        i += 1;
    }
    // 越界显性拒。
    if DomainKind::from_idx(5).is_some() || Priority::from_idx(3).is_some() || CaseKind::from_idx(3).is_some() {
        return false;
    }
    // 优先级序数严格递增（高在前）。
    if !(Priority::High.ord() < Priority::Mid.ord() && Priority::Mid.ord() < Priority::Low.ord()) {
        return false;
    }
    true
}

/// J1 主判据：五元数据 + kind 结构齐备、域枚举合法、校验闸逐面可辨。
fn j1_main_ok() -> bool {
    let m = match meta_from_raw(&good_raw()) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let validate_ok = m.validate().is_ok();
    let fields_ok = !m.batch.is_empty()
        && m.id > 0
        && !m.tags.is_empty()
        && m.tags.len() <= TAG_CAP
        && DomainKind::from_idx(m.domain.idx()) == Some(m.domain);
    let summary_ok = {
        let line = m.summary_line();
        line.contains("内存") && line.contains("批B10") && line.contains("ID7") && line.contains("单元")
    };
    validate_ok && fields_ok && summary_ok
}

/// M-166 全字段验证全绿。
fn m166_ok() -> bool {
    let (all, per) = m166_verify();
    all && per.len() == M166_CASES
}

/// 批格式闸（合法/非法两面对账）。
fn batch_gate_ok() -> bool {
    let legal = [
        batch_legal("B10"),
        batch_legal("B00"),
        batch_legal("B123"),
    ];
    let illegal = [
        batch_legal(""),
        batch_legal("B"),
        batch_legal("b10"),
        batch_legal("10"),
        batch_legal("B1x"),
        batch_legal("B 1"),
    ];
    legal.iter().all(|x| *x) && illegal.iter().all(|x| !*x)
}

/// 台账面：唯一键、序号连续、取账对位、分域摘要非空。
fn ledger_ok() -> bool {
    let mut led = MetaLedger::new();
    let mut i = 0u32;
    while i < 5 {
        let mut r = good_raw();
        r.id = i + 1;
        let m = match meta_from_raw(&r) {
            Ok(x) => x,
            Err(_) => return false,
        };
        if led.push(m).is_err() {
            return false;
        }
        i += 1;
    }
    if led.len() != 5 || !led.seq_ok() {
        return false;
    }
    let at4 = led.at(4);
    match at4 {
        Ok((seq, m)) => {
            if seq != 5 || m.id != 5 {
                return false;
            }
        }
        Err(_) => return false,
    }
    let sum = led.summary();
    sum.contains("域内存 条5") && !sum.is_empty()
}

/// 双跑一致（语料两次验证同判、摘要同文）。
fn dual_run_ok() -> bool {
    let (a, pa) = m166_verify();
    let (b, pb) = m166_verify();
    a && b && pa == pb
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3407 用例元数据。
pub fn run_unx3407_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3407");
    let good = match meta_from_raw(&good_raw()) {
        Ok(x) => x,
        Err(_) => return s,
    };

    s.add_dyn(
        "F3407-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 枚举闭集三面自洽（域枚举合法载体）。
    s.add_dyn(
        "F3407-02 域优先级kind枚举闭集",
        enums_closed_ok(),
        &format!("domains={}", DOMAIN_NAMES.len()),
    );

    // 正常流：五元数据+kind 结构齐备、域枚举合法（J1 主判据）。
    s.add_dyn(
        "F3407-03 五元数据结构校验域枚举合法",
        j1_main_ok(),
        &good.summary_line(),
    );

    // 批格式闸（合法/非法两面对账）。
    s.add_dyn(
        "F3407-04 批格式闸两面对账",
        batch_gate_ok(),
        &format!("batch={}", good.batch),
    );

    // M-166 八档全字段验证全绿。
    let (m_ok, m_per) = m166_verify();
    s.add_dyn(
        "F3407-05 M166全字段八档验证",
        m_ok,
        &format!("per={:?}", m_per),
    );

    // 台账面。
    s.add_dyn(
        "F3407-06 元数据台账唯一键取账对位",
        ledger_ok(),
        &format!("ledger={}", ledger_ok()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3407-07 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：标签容量（恰容必过、超容必拒）。
    let mut r_full = good_raw();
    r_full.tags = ["甲", "乙", "丙", "丁"];
    r_full.tag_n = TAG_CAP;
    let cap_ok = meta_from_raw(&r_full)
        .and_then(|m| m.validate())
        .is_ok();
    s.add_dyn(
        "F3407-08 标签恰容必过",
        cap_ok,
        &format!("cap={}", TAG_CAP),
    );

    // 边界：标签超容显性拒。
    let mut r_over = good_raw();
    r_over.tags = ["甲", "乙", "丙", "丁"];
    r_over.tag_n = TAG_CAP;
    let over_meta = meta_from_raw(&r_over);
    let over_ok = match over_meta {
        Ok(mut m) => {
            m.tags.push(String::from("戊"));
            m.validate() == Err(Diag::ETagFull.val())
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F3407-09 标签超容显性拒",
        over_ok,
        &format!("cap={}", TAG_CAP),
    );

    // 域枚举五域逐域成条校验（全域合法面）。
    let all_domains = {
        let mut ok = true;
        let mut i = 0u8;
        while i < 5 {
            let mut r = good_raw();
            r.domain_idx = i;
            match meta_from_raw(&r) {
                Ok(m) => {
                    if m.validate().is_err() {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        ok
    };
    s.add_dyn(
        "F3407-10 五域逐域成条校验",
        all_domains,
        &format!("domains={}", DOMAIN_NAMES.len()),
    );

    // 写回摘要非空（判据③写回面）。
    let mut led2 = MetaLedger::new();
    if let Ok(m) = meta_from_raw(&good_raw()) {
        let _ = led2.push(m);
    }
    let sum = led2.summary();
    s.add_dyn(
        "F3407-11 写回摘要非空含域",
        !sum.is_empty() && sum.contains("域内存 条1"),
        &format!("bytes={}", sum.len()),
    );

    // 元数据行摘要回含六面原值。
    let line = good.summary_line();
    s.add_dyn(
        "F3407-12 元数据行摘要回含六面",
        line.contains("批B10")
            && line.contains("优先高")
            && line.contains("标签[等值|首差异]"),
        &format!("bytes={}", line.len()),
    );

    // 双跑一致。
    s.add_dyn(
        "F3407-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集。
    s.add_dyn(
        "F3407-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ESealed.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EBadDomain.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESealed.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79500);
    }

    #[test]
    fn domain_enum_roundtrip() {
        for i in 0u8..5 {
            let d = DomainKind::from_idx(i);
            assert!(d.is_some());
            let d = d.unwrap_or(DomainKind::Mem);
            assert_eq!(d.idx(), i);
            assert_eq!(d.name(), DOMAIN_NAMES[i as usize]);
        }
        assert!(DomainKind::from_idx(5).is_none());
        assert!(DomainKind::from_idx(255).is_none());
    }

    #[test]
    fn priority_and_kind_closed() {
        assert!(Priority::from_idx(0).is_some());
        assert!(Priority::from_idx(3).is_none());
        assert!(Priority::High.ord() < Priority::Low.ord());
        assert!(CaseKind::from_idx(2).is_some());
        assert!(CaseKind::from_idx(3).is_none());
    }

    #[test]
    fn batch_format_gate() {
        assert!(batch_legal("B10"));
        assert!(batch_legal("B007"));
        assert!(!batch_legal("B"));
        assert!(!batch_legal("b10"));
        assert!(!batch_legal("B1a"));
        assert!(!batch_legal(""));
    }

    #[test]
    fn m166_eight_cases() {
        assert_eq!(m166_corpus().len(), M166_CASES);
        let (all, per) = m166_verify();
        assert!(all);
        assert!(per.iter().all(|x| *x));
    }

    #[test]
    fn ledger_unique_key() {
        let mut led = MetaLedger::new();
        let m1 = meta_from_raw(&good_raw()).unwrap_or_else(|_| CaseMeta {
            domain: DomainKind::Mem,
            batch: String::from("B10"),
            id: 1,
            priority: Priority::High,
            tags: Vec::new(),
            kind: CaseKind::Unit,
        });
        let mut r = good_raw();
        r.id = 2;
        let m2 = meta_from_raw(&r);
        assert!(led.push(m1).is_ok());
        assert!(m2.map(|x| led.push(x).is_ok()).unwrap_or(false));
        // 同批同 ID 拒。
        let m3 = meta_from_raw(&good_raw());
        assert!(m3.map(|x| led.push(x) == Err(Diag::EDupKey.val())).unwrap_or(false));
        assert_eq!(led.len(), 2);
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn seal_is_one_way() {
        let mut led = MetaLedger::new();
        assert_eq!(led.seal(), Ok(()));
        assert_eq!(led.seal(), Err(Diag::ESealed.val()));
        if let Ok(m) = meta_from_raw(&good_raw()) {
            assert_eq!(led.push(m), Err(Diag::ESealed.val()));
        }
        assert!(led.sealed());
    }

    #[test]
    fn tag_capacity_gate() {
        let mut r = good_raw();
        r.tags = ["甲", "乙", "丙", "丁"];
        r.tag_n = TAG_CAP;
        let m = meta_from_raw(&r);
        assert!(m.map(|x| x.validate().is_ok()).unwrap_or(false));
        // 恰容合法、第 5 个标签超容拒（构造面在 validate 里把关）。
        if let Ok(mut m) = meta_from_raw(&r) {
            m.tags.push(String::from("戊"));
            assert_eq!(m.validate(), Err(Diag::ETagFull.val()));
        }
    }

    #[test]
    fn summary_line_contains_all_fields() {
        let m = meta_from_raw(&good_raw());
        if let Ok(m) = m {
            let line = m.summary_line();
            assert!(line.contains("内存"));
            assert!(line.contains("批B10"));
            assert!(line.contains("ID7"));
            assert!(line.contains("优先高"));
            assert!(line.contains("单元"));
            assert!(line.contains("标签[等值|首差异]"));
        } else {
            assert!(false, "齐档必须能成条");
        }
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3407_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
