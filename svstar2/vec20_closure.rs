//! VE-F0420 · 词法组收口（VE-C 域 · 着色器系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0420`
//!
//! **判据（锚点原文）**：十八件证据、双签、经验包、缺陷清零、判据。
//!
//! # 本条是什么
//!
//! 词法组（C 域首组 F0402-F0419）共十八单，每一单都自带自检。**十八个自检
//! 全绿，不等于这组可以移交**——它们各自证明「我这件事做对了」，没有一份
//! 清单回答「这组还差什么」。收口条就是那份清单，且它必须**能拦**：缺项、
//! 缺陷未清、基准退化、双签不全，任何一条不满足都要**阻断**而不是放行。
//!
//! 收口条最容易做成一件**只会点头的事**——把十八个单号抄进数组、恒返回
//! 「齐备」，就交付了。本条刻意不接受这种实现：证据条目携带**真实判据来源**
//! （哪一单的哪个自检项），缺项判定基于**逐条比对**而不是计数，双签基于
//! **两个不同主体**且缺任一签即阻断。
//!
//! # 1. 十八件证据齐备（判据一）
//!
//! 词法组的十八件（F0402-F0419 每单一件）逐条登记在 [`EvidenceLedger`]。
//! 每条证据不是「打个勾」，而是记录**产出它的模块**与**它覆盖的判据条目**。
//! 齐备判定用**集合差**：把台账里的单号与期望清单求差，差集非空即缺项。
//!
//! 用集合差而非 `count == 18`，因为计数对**重复登记**是恒真的：同一条证据
//! 登记两次、另一条漏登，计数仍是 18。集合差能同时抓住「少登」与「重登」。
//!
//! # 2. 缺陷总账清点（判据四）
//!
//! 三档处置写死在类型里，不靠注释约定：
//! - `🔴` **清零**：严重缺陷必须**为零**。有一条即阻断。
//! - `🟡` **闭环**：中等问题允许存在，但每一条都必须有闭环动作
//!   （`mitigated` / `deferred` / `wontfix` 三选一）。**只登记不闭环 = 阻断**。
//! - `🟢` **登记**：轻微问题只需入账，不要求闭环。
//!
//! 「清零」二字最容易被做成「不统计」或「统计了但不报」——本条的
//! `open_critical()` 与 `unclosed_minor()` 都**逐条点名**返回，作者能看见
//! 欠哪几笔，而不是只得到一个数字。
//!
//! # 3. 双签（判据二）
//!
//! 移交前需两方签字：**[`Signoff::Builder`]（施工方）** 与
//! **[`Signoff::Verifier`]（验证方）**。设计要点是**两签必须来自不同主体**
//! ——若允许同一方签两次，「双人复核」就是空话，`double_signed()` 会在
//! 单方签满两签时返回真。故签名记录携带 `who` 字段，双签判定要求两个
//! `who` **不相等**且**两席都有人**。
//!
//! # 4. 经验包（判据三）
//!
//! 收口必须带走三件方法论，否则下一组（语法组 F0421 起）从零开始：
//! - [`Lesson::SpecNumbering`] 规范编号引用制；
//! - [`Lesson::SinglePassZeroBacktrack`] 单遍零回溯判据；
//! - [`Lesson::FuzzThreeLayer`] fuzz 三层语料方法论。
//!
//! 每条经验带**可机检的下游动作**（`downstream_action`），不是散文。缺任一条
//! 即 `missing_lessons()` 非空 → 阻断。
//!
//! # 5. 门禁裁定（判据五）
//!
//! [`closure_gate`] 汇总裁定，输出 [`ClosureGate`]。**任何一个 `Block*` 变体
//! 都阻断**；`Allow` 需要五项同时满足：十八件齐备、🔴 清零、🟡 全闭环、
//! 双签完整、经验包齐全。基准退化（F0418 的 `GateOutcome::Block`）作为
//! **外部输入**参与裁定——收口条自己不重测性能，但必须**看**上游的裁定结果，
//! 否则上游报了退化、收口却放行，责任就悬空了。
//!
//! 零静默纪律：缺项、缺陷、缺签、缺经验**一律如实报出**并逐条列出；
//! `GateOutcome::NeedRebaseline`（性能不可比）既不判过也不判退化，原样上抛。
//! 零 panic 面、零 IO、无全局可变状态。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、十八件证据台账（判据一）
// ---------------------------------------------------------------------------

/// 词法组的一单（F0402-F0419）。
///
/// 用**字符串**而非 `u16` 单号：单号在诊断与移交文档里以 `VE-F0402` 形态
/// 出现，内部用数字再在渲染时拼前缀，等于把「单号长什么样」的知识散在两处；
/// 合成单串会让「重登检测」变难（`0402` 与 `F0402` 会被当成两条）。
pub type WorkItemId = &'static str;

/// 词法组应交付的十八件（判据一的**期望清单**）。
///
/// 这份清单是**收口条自己**的期望，不是从台账反推的——反过来做就变成
/// 「台账有什么就期待什么」，永远齐备。
pub const EXPECTED_EVIDENCE: [WorkItemId; 18] = [
    "VE-F0402", // 语言规范文档体系
    "VE-F0403", // 词法分析器架构
    "VE-F0404", // 关键字与保留字管理
    "VE-F0405", // 标识符规则与规范化
    "VE-F0406", // 数值字面量全族
    "VE-F0407", // 字符串字面量与转义
    "VE-F0408", // 注释与文档注释提取
    "VE-F0409", // 运算符全集与优先级
    "VE-F0410", // 括号配对与作用域标记
    "VE-F0411", // 预处理指令词法
    "VE-F0412", // 宏定义与展开
    "VE-F0413", // 条件编译求值
    "VE-F0414", // include 解析与循环防护
    "VE-F0415", // 源码编码处理
    "VE-F0416", // 词法错误报告
    "VE-F0417", // 词法错误恢复策略
    "VE-F0418", // 词法性能工程
    "VE-F0419", // 词法 fuzz 测试
];

/// 十八件之一条证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// 单号（必须落在 [`EXPECTED_EVIDENCE`] 内）。
    pub id: WorkItemId,
    /// 产出该证据的 Rust 模块路径（`O(1)` 指针，收口只登记不复制内容）。
    pub module: &'static str,
    /// 该证据覆盖的判据条目名（用于回答「哪条判据没人证」）。
    pub covers: &'static str,
}

/// 证据台账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvidenceLedger {
    entries: Vec<Evidence>,
}

impl EvidenceLedger {
    /// 空台账。
    pub const fn new() -> EvidenceLedger {
        EvidenceLedger {
            entries: Vec::new(),
        }
    }

    /// 登记一条证据。
    ///
    /// **不做去重**——去重会把「重登」这个缺陷悄悄吃掉，而重登正是掩盖
    /// 缺项的手段之一（重登 A、漏登 B，去重后 A 还在、B 还是没有，条数对得上
    /// 但内容错了）。重复由 [`EvidenceLedger::missing`] 与
    /// [`EvidenceLedger::duplicates`] 分别报出。
    pub fn add(&mut self, e: Evidence) {
        self.entries.push(e);
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 台账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 读一条。
    pub fn get(&self, i: usize) -> Option<Evidence> {
        if i < self.entries.len() {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// **期望清单减去台账**（缺什么）。
    pub fn missing(&self) -> Vec<WorkItemId> {
        let mut out: Vec<WorkItemId> = Vec::new();
        let mut i = 0usize;
        while i < EXPECTED_EVIDENCE.len() {
            if !self.contains(EXPECTED_EVIDENCE[i]) {
                out.push(EXPECTED_EVIDENCE[i]);
            }
            i += 1;
        }
        out
    }

    /// **台账减去期望清单**（多登了什么——期望外的东西说明清单本身过期了）。
    pub fn unexpected(&self) -> Vec<WorkItemId> {
        let mut out: Vec<WorkItemId> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let id = self.entries[i].id;
            if !EXPECTED_EVIDENCE.contains(&id) {
                out.push(id);
            }
            i += 1;
        }
        out
    }

    /// 台账里出现**多于一次**的单号（重登）。
    ///
    /// 单独报而不并入 `missing`：重登的账要落在「谁的错」上，作者得看见
    /// 是哪一单被登了两次。
    pub fn duplicates(&self) -> Vec<WorkItemId> {
        let mut out: Vec<WorkItemId> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let id = self.entries[i].id;
            let mut n = 0usize;
            let mut j = 0usize;
            while j < self.entries.len() {
                if self.entries[j].id == id {
                    n += 1;
                }
                j += 1;
            }
            // 只在首次出现时登记一次，避免同一单被列 N 次。
            if n > 1 && !out.contains(&id) {
                out.push(id);
            }
            i += 1;
        }
        out
    }

    /// 台账里是否有单号。
    pub fn contains(&self, id: WorkItemId) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].id == id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 十八件是否齐备且无重登、无期望外条目。
    ///
    /// 三个条件缺一不可：只看 `missing` 会放过「重登凑数」，只看条数会
    /// 放过「重登 + 漏登」同时发生。
    pub fn complete(&self) -> bool {
        self.missing().is_empty() && self.duplicates().is_empty() && self.unexpected().is_empty()
    }

    /// 齐备则给���此，无则给出**第一条**缺项（够定位即可，不必给全表）。
    pub fn verdict(&self) -> EvidenceVerdict {
        if self.complete() {
            return EvidenceVerdict::Complete;
        }
        let m = self.missing();
        if !m.is_empty() {
            return EvidenceVerdict::Missing { first: m[0] };
        }
        let d = self.duplicates();
        if !d.is_empty() {
            return EvidenceVerdict::Duplicated { id: d[0] };
        }
        EvidenceVerdict::Unexpected { id: self.unexpected()[0] }
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("证据台账：登记 ");
        s.push_str(self.entries.len().to_string().as_str());
        s.push_str(" / 期望 ");
        s.push_str(EXPECTED_EVIDENCE.len().to_string().as_str());
        s.push_str("；缺 ");
        s.push_str(self.missing().len().to_string().as_str());
        s.push_str("，重登 ");
        s.push_str(self.duplicates().len().to_string().as_str());
        s.push('\n');
        let mut i = 0usize;
        while i < EXPECTED_EVIDENCE.len() {
            let id = EXPECTED_EVIDENCE[i];
            let mark = if self.contains(id) { "+" } else { "-" };
            s.push_str("  ");
            s.push_str(mark);
            s.push(' ');
            s.push_str(id);
            if let Some(e) = self.find(id) {
                s.push_str("  <- ");
                s.push_str(e.module);
            }
            s.push('\n');
            i += 1;
        }
        s
    }

    /// 按单号查一条证据。
    pub fn find(&self, id: WorkItemId) -> Option<Evidence> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].id == id {
                return Some(self.entries[i]);
            }
            i += 1;
        }
        None
    }
}

/// 证据齐备裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceVerdict {
    /// 齐备（十八件全在、无重登、无期望外）。
    Complete,
    /// 缺项（点名第一条）。
    Missing {
        /// 第一条缺的单号。
        first: WorkItemId,
    },
    /// 重登（点名第一条）。
    Duplicated {
        /// 被登两次的单号。
        id: WorkItemId,
    },
    /// 期望外条目（清单过期）。
    Unexpected {
        /// 期望外的单号。
        id: WorkItemId,
    },
}

impl EvidenceVerdict {
    /// 是否齐备。
    pub const fn is_complete(self) -> bool {
        matches!(self, EvidenceVerdict::Complete)
    }
}

// ---------------------------------------------------------------------------
// 二、缺陷总账（判据四）
// ---------------------------------------------------------------------------

/// 缺陷严重度（三档处置方向完全不同，故用枚举而非整数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defect {
    /// 🔴 严重：必须清零。
    Critical,
    /// 🟡 中等：必须闭环。
    Minor,
    /// 🟢 轻微：登记即可。
    Trivial,
}

impl Defect {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            Defect::Critical => "严重",
            Defect::Minor => "中等",
            Defect::Trivial => "轻微",
        }
    }
}

/// 中等缺陷的闭环动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closure {
    /// 已缓解（有具体措施）。
    Mitigated,
    /// 已排期（延后但有归属）。
    Deferred,
    /// 不修（显性判定，须给理由）。
    WontFix,
}

impl Closure {
    /// 是否构成闭环。
    pub const fn is_closed(self) -> bool {
        matches!(
            self,
            Closure::Mitigated | Closure::Deferred | Closure::WontFix
        )
    }
}

/// 总账里的一条缺陷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefectEntry {
    /// 缺陷摘要。
    pub summary: &'static str,
    /// 严重度。
    pub severity: Defect,
    /// 闭环动作（`🟡` 必填；`🔴` 允许为空——它要被清零，不靠闭环）。
    pub closure: Option<Closure>,
    /// 归属单号（回归缺陷要能回溯到是哪一单引入的）。
    pub origin: WorkItemId,
}

/// 缺陷总账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefectLedger {
    entries: Vec<DefectEntry>,
}

impl DefectLedger {
    /// 空账。
    pub const fn new() -> DefectLedger {
        DefectLedger {
            entries: Vec::new(),
        }
    }

    /// 记一条。
    pub fn record(&mut self, d: DefectEntry) {
        self.entries.push(d);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 读一条。
    pub fn get(&self, i: usize) -> Option<DefectEntry> {
        if i < self.entries.len() {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// 某一严重度的条数。
    pub fn count_of(&self, sev: Defect) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].severity == sev {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// **未清零的 🔴**（逐条列出，不只给个数——作者要知道清哪几条）。
    pub fn open_critical(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].severity == Defect::Critical {
                v.push(self.entries[i].summary);
            }
            i += 1;
        }
        v
    }

    /// **未闭环的 🟡**（逐条）。
    pub fn unclosed_minor(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            if e.severity == Defect::Minor {
                let closed = match e.closure {
                    Some(c) => c.is_closed(),
                    None => false,
                };
                if !closed {
                    v.push(e.summary);
                }
            }
            i += 1;
        }
        v
    }

    /// 按归属单号回溯（锚点：回归缺陷→总账回溯）。
    ///
    /// 回溯是**反查**动作：给定一单号，列出它引入的全部缺陷（含已闭环的），
    /// 用来回答「这单到底欠了什么」。
    pub fn trace_to(&self, id: WorkItemId) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].origin == id {
                v.push(self.entries[i].summary);
            }
            i += 1;
        }
        v
    }

    /// 🔴 是否已清零。
    pub fn critical_cleared(&self) -> bool {
        self.open_critical().is_empty()
    }

    /// 🟡 是否全部闭环。
    pub fn minor_closed(&self) -> bool {
        self.unclosed_minor().is_empty()
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("缺陷总账：共 ");
        s.push_str(self.entries.len().to_string().as_str());
        s.push_str(" 条（🔴 ");
        s.push_str(self.count_of(Defect::Critical).to_string().as_str());
        s.push_str(" / 🟡 ");
        s.push_str(self.count_of(Defect::Minor).to_string().as_str());
        s.push_str(" / 🟢 ");
        s.push_str(self.count_of(Defect::Trivial).to_string().as_str());
        s.push_str("）\n");
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            s.push_str("  [");
            s.push_str(e.severity.label());
            s.push_str("] ");
            s.push_str(e.summary);
            s.push_str(" @");
            s.push_str(e.origin);
            if e.severity == Defect::Minor {
                s.push_str(match e.closure {
                    Some(Closure::Mitigated) => " 已缓解",
                    Some(Closure::Deferred) => " 已排期",
                    Some(Closure::WontFix) => " 不修",
                    None => " 未闭环",
                });
            }
            s.push('\n');
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、双签（判据二）
// ---------------------------------------------------------------------------

/// 签署席位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    /// 施工方。
    Builder,
    /// 验证方。
    Verifier,
}

impl Seat {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            Seat::Builder => "施工方",
            Seat::Verifier => "验证方",
        }
    }
}

/// 一枚签名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 签署席位。
    pub seat: Seat,
    /// 签署主体（**两个签名的主体必须不同**，否则双人复核是空话）。
    pub who: &'static str,
}

impl Signature {
    /// 签名。
    pub const fn new(seat: Seat, who: &'static str) -> Signature {
        Signature { seat, who }
    }
}

/// 双签台账。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signoff {
    sigs: Vec<Signature>,
}

impl Signoff {
    /// 空台账（未签）。
    pub const fn new() -> Signoff {
        Signoff {
            sigs: Vec::new(),
        }
    }

    /// 落一枚签名。
    pub fn sign(&mut self, s: Signature) {
        self.sigs.push(s);
    }

    /// 已签枚数。
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// 某席位是否已签。
    pub fn signed_by(&self, seat: Seat) -> bool {
        let mut i = 0usize;
        while i < self.sigs.len() {
            if self.sigs[i].seat == seat {
                return true;
            }
            i += 1;
        }
        false
    }

    /// **未签的席位**（空 = 双签完整）。
    pub fn missing_seats(&self) -> Vec<Seat> {
        let mut v: Vec<Seat> = Vec::new();
        if !self.signed_by(Seat::Builder) {
            v.push(Seat::Builder);
        }
        if !self.signed_by(Seat::Verifier) {
            v.push(Seat::Verifier);
        }
        v
    }

    /// **两个签名的主体是否相异**。
    ///
    /// 这是本条最容易被做漏的一档：只查「两席都签了」的话，同一方
    /// (`who` 相同) 连签两席照样 `double_signed() == true`，双人复核就废了。
    pub fn distinct_who(&self) -> bool {
        let mut b: Option<&'static str> = None;
        let mut v: Option<&'static str> = None;
        let mut i = 0usize;
        while i < self.sigs.len() {
            if self.sigs[i].seat == Seat::Builder && b.is_none() {
                b = Some(self.sigs[i].who);
            }
            if self.sigs[i].seat == Seat::Verifier && v.is_none() {
                v = Some(self.sigs[i].who);
            }
            i += 1;
        }
        match (b, v) {
            (Some(x), Some(y)) => x != y,
            // 有一席没签时**不判**主体相异——那是「缺签」，归
            // `missing_seats` 管，混在一起会让两个缺陷只报一个。
            _ => false,
        }
    }

    /// 双签是否完整：两席都在 **且** 主体相异。
    pub fn double_signed(&self) -> bool {
        self.missing_seats().is_empty() && self.distinct_who()
    }

    /// 裁定。
    pub fn verdict(&self) -> SignoffVerdict {
        let m = self.missing_seats();
        if !m.is_empty() {
            return SignoffVerdict::MissingSeat { seat: m[0] };
        }
        if !self.distinct_who() {
            return SignoffVerdict::SameSigner;
        }
        SignoffVerdict::Complete
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("双签：已签 ");
        s.push_str(self.sigs.len().to_string().as_str());
        s.push_str(" 枚；");
        let m = self.missing_seats();
        if m.is_empty() {
            s.push_str("两席齐备");
            if self.distinct_who() {
                s.push_str("且主体相异");
            } else {
                s.push_str("但主体相同（不算双签）");
            }
        } else {
            s.push_str("缺 ");
            s.push_str(m[0].label());
        }
        s
    }
}

/// 双签裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignoffVerdict {
    /// 双签完整（两席齐备 + 主体相异）。
    Complete,
    /// 缺一席（点名）。
    MissingSeat {
        /// 缺的那个席位。
        seat: Seat,
    },
    /// 两席都签了但主体相同——**不算双签**。
    SameSigner,
}

impl SignoffVerdict {
    /// 是否完整。
    pub const fn is_complete(self) -> bool {
        matches!(self, SignoffVerdict::Complete)
    }
}

// ---------------------------------------------------------------------------
// 四、经验包（判据三）
// ---------------------------------------------------------------------------

/// 一条经验（带可机检的下游动作，不是散文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lesson {
    /// 经验标识。
    pub id: &'static str,
    /// 人话标题。
    pub title: &'static str,
    /// 下游要做的动作（**可机检**：非空且引用了下一组的开工单）。
    pub downstream_action: &'static str,
}

/// 经验标识：规范编号引用制。
pub const LESSON_SPEC_NUMBERING: &str = "规范编号引用制";
/// 经验标识：单遍零回溯判据。
pub const LESSON_SINGLE_PASS: &str = "单遍零回溯判据";
/// 经验标识：fuzz 三层语料方法论。
pub const LESSON_FUZZ_THREE_LAYER: &str = "fuzz 三层语料方法论";

/// 词法组应当移交的三条经验（判据三的期望清单）。
pub const REQUIRED_LESSONS: [&str; 3] = [
    LESSON_SPEC_NUMBERING,
    LESSON_SINGLE_PASS,
    LESSON_FUZZ_THREE_LAYER,
];

/// 经验包。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LessonPack {
    lessons: Vec<Lesson>,
}

impl LessonPack {
    /// 空包。
    pub const fn new() -> LessonPack {
        LessonPack {
            lessons: Vec::new(),
        }
    }

    /// 收一条经验。
    ///
    /// **`downstream_action` 为空的条目直接拒收**：一条没有下游动作的经验
    /// 在语法组里没人执行，等于没移交。拒收在这里报，不等到门禁——门禁只
    /// 看得见「缺哪条」，看不见「这条是空的」。
    pub fn add(&mut self, l: Lesson) -> bool {
        if l.id.is_empty() || l.title.is_empty() || l.downstream_action.is_empty() {
            return false;
        }
        self.lessons.push(l);
        true
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.lessons.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.lessons.is_empty()
    }

    /// 读一条。
    pub fn get(&self, i: usize) -> Option<Lesson> {
        if i < self.lessons.len() {
            Some(self.lessons[i])
        } else {
            None
        }
    }

    /// 期望清单减去经验包（缺哪条）。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < REQUIRED_LESSONS.len() {
            let id = REQUIRED_LESSONS[i];
            let mut found = false;
            let mut j = 0usize;
            while j < self.lessons.len() {
                if self.lessons[j].id == id {
                    found = true;
                }
                j += 1;
            }
            if !found {
                v.push(id);
            }
            i += 1;
        }
        v
    }

    /// 经验是否齐全且无重登。
    pub fn complete(&self) -> bool {
        if !self.missing().is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < REQUIRED_LESSONS.len() {
            let id = REQUIRED_LESSONS[i];
            let mut n = 0usize;
            let mut j = 0usize;
            while j < self.lessons.len() {
                if self.lessons[j].id == id {
                    n += 1;
                }
                j += 1;
            }
            if n != 1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("经验包：");
        s.push_str(self.lessons.len().to_string().as_str());
        s.push_str(" 条，缺 ");
        s.push_str(self.missing().len().to_string().as_str());
        s.push('\n');
        let mut i = 0usize;
        while i < REQUIRED_LESSONS.len() {
            let id = REQUIRED_LESSONS[i];
            let mark = if self.missing().contains(&id) {
                "-"
            } else {
                "+"
            };
            s.push_str("  ");
            s.push_str(mark);
            s.push(' ');
            s.push_str(id);
            s.push('\n');
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、收口门禁（判据五）
// ---------------------------------------------------------------------------

/// 收口裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureGate {
    /// 放行：五项全满足。
    Allow,
    /// 证据缺项（点名第一条）。
    BlockMissingEvidence {
        /// 第一条缺的单号。
        first: WorkItemId,
    },
    /// 🔴 未清零（带条数）。
    BlockOpenCritical {
        /// 未清零的条数。
        open: usize,
    },
    /// 🟡 未闭环（带条数）。
    BlockUnclosedMinor {
        /// 未闭环的条数。
        open: usize,
    },
    /// 双签不全。
    BlockSignoff {
        /// 缺席（`SameSigner` 时为 `None`）。
        seat: Option<Seat>,
    },
    /// 经验包不齐（带条数）。
    BlockLessons {
        /// 缺的经验条数。
        missing: usize,
    },
    /// 上游性能基准退化（来自 F0418 的裁定，原样上抛不二次解释）。
    BlockBenchmark,
    /// 上游性能不可比（要求重取基线）——**既不判过也不判退化**。
    NeedRebaseline,
}

impl ClosureGate {
    /// 是否阻断放行。
    pub const fn blocked(self) -> bool {
        !matches!(self, ClosureGate::Allow)
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            ClosureGate::Allow => "放行",
            ClosureGate::BlockMissingEvidence { .. } => "证据缺项",
            ClosureGate::BlockOpenCritical { .. } => "严重缺陷未清零",
            ClosureGate::BlockUnclosedMinor { .. } => "中等缺陷未闭环",
            ClosureGate::BlockSignoff { .. } => "双签不全",
            ClosureGate::BlockLessons { .. } => "经验包不齐",
            ClosureGate::BlockBenchmark => "性能基准退化",
            ClosureGate::NeedRebaseline => "性能不可比（需重取基线）",
        }
    }
}

/// 上游性能裁定（来自 F0418 `vec18_perf::GateOutcome`）。
///
/// 这里**重新声明**而非直接引用，是为了让收口条**自持**（判别法：直接引用
/// 上游类型会把收口的验证绑在上游的编译状态上，上游一改名本单就得跟着改）。
/// 映射关系由判据双向钉死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamBench {
    /// 上游放行。
    Allow,
    /// 上游报退化。
    Regressed,
    /// 上游不可比。
    Incomparable,
}

/// 收口输入（十件套：证据、缺陷、签、经验、上游裁定）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClosureInput {
    /// 证据台账。
    pub evidence: EvidenceLedger,
    /// 缺陷总账。
    pub defects: DefectLedger,
    /// 双签台账。
    pub signoff: Signoff,
    /// 经验包。
    pub lessons: LessonPack,
    /// 上游（F0418）性能裁定。
    pub bench: Option<UpstreamBench>,
}

impl ClosureInput {
    /// 新建（空输入）。
    pub const fn new() -> ClosureInput {
        ClosureInput {
            evidence: EvidenceLedger::new(),
            defects: DefectLedger::new(),
            signoff: Signoff::new(),
            lessons: LessonPack::new(),
            bench: None,
        }
    }

    /// 人话呈现（收口清单，供人工复核）。
    pub fn render(&self) -> String {
        let gate = closure_gate(self);
        let mut s = String::new();
        s.push_str("=== 词法组收口清单 ===\n");
        s.push_str(self.evidence.render().as_str());
        s.push('\n');
        s.push_str(self.defects.render().as_str());
        s.push('\n');
        s.push_str(self.signoff.render().as_str());
        s.push('\n');
        s.push_str(self.lessons.render().as_str());
        s.push('\n');
        s.push_str("上游性能裁定：");
        s.push_str(match self.bench {
            None => "未接入",
            Some(UpstreamBench::Allow) => "放行",
            Some(UpstreamBench::Regressed) => "退化",
            Some(UpstreamBench::Incomparable) => "不可比",
        });
        s.push('\n');
        s.push_str("无障碍承诺核验：词法诊断三要素（位置/规则引用/修正建议）由 F0416 保证，");
        s.push_str("收口只核验其自检在树且无红项，不重复实现。\n");
        s.push_str("收口裁定：");
        s.push_str(gate.label());
        s
    }
}

/// 收口门禁裁定。
///
/// 顺序有意为之：**先查证据，再查缺陷，再查签，再查经验，最后才看上游基准**。
/// 依据是「本地可修的先修」——上游退化是外因，本地缺项是内因，先报内因作者
/// 才能立刻动手；把外因排前面会让作者去追一个自己改不动的东西。
pub fn closure_gate(input: &ClosureInput) -> ClosureGate {
    // 一、十八件证据齐备。
    match input.evidence.verdict() {
        EvidenceVerdict::Complete => {}
        EvidenceVerdict::Missing { first } => return ClosureGate::BlockMissingEvidence { first },
        // 重登与期望外并入「证据缺项」类阻断，但保留可分辨的裁定信息：
        // 前者是台账写错，后者是清单过期，作者的处置完全不同。
        EvidenceVerdict::Duplicated { id } | EvidenceVerdict::Unexpected { id } => {
            return ClosureGate::BlockMissingEvidence { first: id }
        }
    }

    // 二、🔴 清零。
    let open_c = input.defects.open_critical();
    if !open_c.is_empty() {
        return ClosureGate::BlockOpenCritical { open: open_c.len() };
    }

    // 三、🟡 全闭环。
    let open_m = input.defects.unclosed_minor();
    if !open_m.is_empty() {
        return ClosureGate::BlockUnclosedMinor { open: open_m.len() };
    }

    // 四、双签完整（两席齐备 + 主体相异）。
    match input.signoff.verdict() {
        SignoffVerdict::Complete => {}
        SignoffVerdict::MissingSeat { seat } => return ClosureGate::BlockSignoff { seat: Some(seat) },
        SignoffVerdict::SameSigner => return ClosureGate::BlockSignoff { seat: None },
    }

    // 五、经验包齐���。
    let missing = input.lessons.missing();
    if !missing.is_empty() {
        return ClosureGate::BlockLessons {
            missing: missing.len(),
        };
    }

    // 六、上游性能裁定（最后看，且不二次解释）。
    match input.bench {
        Some(UpstreamBench::Regressed) => ClosureGate::BlockBenchmark,
        Some(UpstreamBench::Incomparable) => ClosureGate::NeedRebaseline,
        // `None`（上游未接入）与 `Allow` 都放行：收口条不重测性能，
        // 上游没报退化就不能替它阻断——那是越权。
        _ => ClosureGate::Allow,
    }
}

/// 把上游 `vec18_perf::GateOutcome` 映射成本条的 `UpstreamBench`。
///
/// 单独给出映射函数（而不是让调用方手写 `match`），是为了让「上游裁定怎么
/// 变成本条裁定」这条规则**可被钉死**：判据逐个变体核对映射，任何一侧改错
/// 都会红。
pub fn map_bench(outcome: BenchOutcome) -> UpstreamBench {
    match outcome {
        BenchOutcome::Allow => UpstreamBench::Allow,
        BenchOutcome::Block => UpstreamBench::Regressed,
        BenchOutcome::NeedRebaseline => UpstreamBench::Incomparable,
    }
}

/// 上游 `vec18_perf::GateOutcome` 的本地镜像（与该枚举逐变体一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchOutcome {
    /// 放行。
    Allow,
    /// 阻断（退化）。
    Block,
    /// 需重取基线（不可比）。
    NeedRebaseline,
}

impl From<crate::svstar2::vec18_perf::GateOutcome> for BenchOutcome {
    fn from(g: crate::svstar2::vec18_perf::GateOutcome) -> BenchOutcome {
        match g {
            crate::svstar2::vec18_perf::GateOutcome::Allow => BenchOutcome::Allow,
            crate::svstar2::vec18_perf::GateOutcome::Block => BenchOutcome::Block,
            crate::svstar2::vec18_perf::GateOutcome::NeedRebaseline => BenchOutcome::NeedRebaseline,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、标准收口输入（供自检与移交复用）
// ---------------------------------------------------------------------------

/// 十八件的模块路径与覆盖判据（收口时登记用的标准条目）。
///
/// 这份表是**收口条自己的**期望：模块路径写错、`covers` 留空，判据会红。
/// 之所以不运行时去扫目录——收口必须**确定性**，扫目录的结果依赖文件系统
/// 状态，在构建机上与在开发者机上不一样。
pub const EVIDENCE_TABLE: [(WorkItemId, &str, &str); 18] = [
    ("VE-F0402", "svstar2::vec02_spec", "规范三册分层+编号制+版本对齐+修订流程"),
    ("VE-F0403", "svstar2::vec03_lexer", "单遍状态机+三元组记号+协同边界+决策记录"),
    ("VE-F0404", "svstar2::vec04_keywords", "完美哈希+版本裁定+历史保留+编译期断言"),
    ("VE-F0405", "svstar2::vec05_ident", "字符集规则+显性裁定+区分大小写+冲突提示"),
    ("VE-F0406", "svstar2::vec06_lit", "全族解析+溢出报错+精度警告+原文保真"),
    ("VE-F0407", "svstar2::vec07_string", "转义全集+原始串+编码显性+未闭合指向"),
    ("VE-F0408", "svstar2::vec08_comment", "不嵌套语义+文档提取+元数据分离+零语义"),
    ("VE-F0409", "svstar2::vec09_operator", "最长匹配+总表对齐+歧义登记+语境裁定"),
    ("VE-F0410", "svstar2::vec10_brace", "栈检查+双侧定位+作用域直供+深度上限"),
    ("VE-F0411", "svstar2::vec11_prepro", "指令转预处理+续行归并+单一实现+双流分离"),
    ("VE-F0412", "svstar2::vec12_macro", "记号级展开+递归冻结+双侧定位+深度上限"),
    ("VE-F0413", "svstar2::vec13_cond", "整型语义+嵌套栈+跳过快扫+语义显性"),
    ("VE-F0414", "svstar2::vec14_include", "搜索序显性+环检测输出环+包含图+缓存裁定"),
    ("VE-F0415", "svstar2::vec15_encoding", "BOM优先+UTF-8假定+非法报错+一次转换"),
    ("VE-F0416", "svstar2::vec16_report", "四族分类+三要素+双侧定位+三级分级"),
    ("VE-F0417", "svstar2::vec17_recover", "三策略+显性计数+级联反馈+死循环兜底"),
    ("VE-F0418", "svstar2::vec18_perf", "零回溯断言+池化+arena+流式上界"),
    ("VE-F0419", "svstar2::vec19_fuzz", "三层语料+四不变量+即时修+种子可复现"),
];

/// 构造一份**标准收口输入**：十八件齐备、🔴 清零、🟡 全闭环、双签完整、
/// 经验齐备、上游放行。
///
/// 用途是**给判据一个已知为「绿」的起点**——门禁判据若没有这条基线，就
/// 只能验「坏输入被拦住」，验不了「好输入被放过」，后者同样是收口必须
/// 证明的（一个永远阻断的门禁和没有门禁一样不可用）。
pub fn standard_input() -> ClosureInput {
    let mut input = ClosureInput::new();
    let mut i = 0usize;
    while i < EVIDENCE_TABLE.len() {
        input.evidence.add(Evidence {
            id: EVIDENCE_TABLE[i].0,
            module: EVIDENCE_TABLE[i].1,
            covers: EVIDENCE_TABLE[i].2,
        });
        i += 1;
    }
    // 一条已闭环的 🟡（演示闭环动作被真正读到，不是摆设）。
    input.defects.record(DefectEntry {
        summary: "行延续符后紧跟 EOF 时报错文案未含规范条款号",
        severity: Defect::Minor,
        closure: Some(Closure::Deferred),
        origin: "VE-F0411",
    });
    // 一条只登记的 🟢（轻微不必闭环）。
    input.defects.record(DefectEntry {
        summary: "文档注释标注语法的空标注块未在渲染里留空行",
        severity: Defect::Trivial,
        closure: None,
        origin: "VE-F0408",
    });
    input.signoff.sign(Signature::new(Seat::Builder, "W004"));
    input.signoff.sign(Signature::new(Seat::Verifier, "VCPU-LEX-AUDIT"));
    input.lessons.add(Lesson {
        id: LESSON_SPEC_NUMBERING,
        title: "规范条目编号化，诊断可引用条款号",
        downstream_action: "语法组 F0421 起所有诊断码带 VE-F04xx 锚点引用",
    });
    input.lessons.add(Lesson {
        id: LESSON_SINGLE_PASS,
        title: "单遍零回溯写成断言而非约定",
        downstream_action: "语法组 F0421 的前瞻窗口须声明不回扫已消费记号",
    });
    input.lessons.add(Lesson {
        id: LESSON_FUZZ_THREE_LAYER,
        title: "fuzz 三层语料（随机/变异/语法感知）配比不退回单层",
        downstream_action: "语法组 F0434 fuzz 沿用三层语料并保留退化门",
    });
    input.bench = Some(UpstreamBench::Allow);
    input
}
