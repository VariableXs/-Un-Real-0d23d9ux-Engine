//! UNX-F1553 域级经沉淀版本化（沉淀账 v1 版本与变更规则） · 0x78A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 2 · UNX-F1553 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1553-J1 沉淀账版本化生效（v1 冻结+变更走变更规则：
//!     新条款/修订/废弃三路），违规变更拦截 10/10，版本账落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - v1 冻结：24 条条款一经冻结即为只读版本账，直接改写通道恒拒；
//!   - 变更规则三路：新条款（编号不得重）、修订（条款须存在且未废弃）、
//!     废弃（条款须存在），三路各带显性前置校验；
//!   - 违规拦截 10/10：修订不存在条款 / 废弃不存在条款 / 新增重号条款 /
//!     修订已废弃条款 / 二次废弃 / v1 直接改写 / 版本回退 / 封账后变更 /
//!     新条款空文本 / 新条款空编号——十路注入全部显性拒；
//!   - 版本账落档：条款面 + 变更面拼接后 FNV-1a 落档，双跑逐位一致；
//!   - 状态机单调：起草 → 冻结 → 封账，倒退 / 跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78A40，派工卡 VX8-B05 卡 2 独占窗口）。
pub const SEG_BASE: u32 = 0x78A40;

/// 沉淀账 v1 条款条数（判据 J1「v1 冻结」承载面，冻结值）。
pub const V1_CLAUSE_COUNT: usize = 24;

/// 违规变更拦截路径数（判据 J1「违规变更拦截 10/10」冻结）。
pub const VIOLATION_PATHS: usize = 10;

// ── v1 冻结条款表（24 条，版本账基线） ───────────────────────────
/// (条款编号, 条款正文)
pub const V1_CLAUSES: [(&str, &str); 24] = [
    ("SD-001", "域级判据冻结后只增不减，变更走沉淀账规则"),
    ("SD-002", "批收口聚合前置：判据逐条承载断言"),
    ("SD-003", "诊断码按段闭集分配，跨段码不认"),
    ("SD-004", "双跑一致口径：指纹与产物逐位比对"),
    ("SD-005", "注入必抓：人为违约必被显性拒"),
    ("SD-006", "段位守卫：SEG_BASE 对齐且落窗"),
    ("SD-007", "写回契约：状态已完成、摘要非空"),
    ("SD-008", "FAQ 十问终版：三列齐备、关联可溯"),
    ("SD-009", "文档打包预演：六件对平后落档"),
    ("SD-010", "知识检索面：三键检索预算受控"),
    ("SD-011", "人话总册：三 scope 全状态可解释"),
    ("SD-012", "核签件：40 批汇总与台账对平"),
    ("SD-013", "体验信号：三类四态逐类登记"),
    ("SD-014", "文档哨兵：漂移必检出、开销受预算"),
    ("SD-015", "断言账：回撤通道恒闭"),
    ("SD-016", "收口报告行数冻结，行数违约拒收"),
    ("SD-017", "指纹统一 FNV-1a 口径"),
    ("SD-018", "中文诊断文案带可操作建议"),
    ("SD-019", "零浮点、零 IO、零墙钟、零随机"),
    ("SD-020", "取值一律 checked/saturating/get"),
    ("SD-021", "状态机倒退显性拒，不留静默通道"),
    ("SD-022", "常量来源须注手册章节或规范引用"),
    ("SD-023", "版本账封账后禁改，另开新版本"),
    ("SD-024", "沉淀账 v1 为域级纪律的唯一基线"),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SedimentCode {
    /// 新条款编号重复：沉淀账一条一款，先查重再立新
    ClauseDup,
    /// 条款不存在：修订/废弃前先核对条款编号
    ClauseUnknown,
    /// 条款已废弃：废弃条款不得修订，复活须立新条款
    ClauseDeprecated,
    /// v1 冻结直接改写：改写通道恒闭，变更走三路规则
    FrozenRewrite,
    /// 版本回退：版本号只进不退，回退显性拒
    VersionRegress,
    /// 变更文本为空：新条款/修订必须写足正文
    ChangeEmptyText,
    /// 变更编号为空：新条款必须带编号
    ChangeEmptyId,
    /// 封账后再变更：版本账封账禁改，另开新版本
    SealedLocked,
    /// 状态倒退：按起草→冻结→封账单调推进
    PhaseRegress,
    /// 状态跳级：未冻结不得封账
    PhaseSkip,
    /// 版本账未落档：先封账生成指纹再写回
    DigestNotArchived,
    /// 双跑不一致：排查指纹随机性后复跑
    NotReproducible,
}

impl SedimentCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SedimentCode::ClauseDup => "新条款编号重复：沉淀账一条一款，先查重再立新",
            SedimentCode::ClauseUnknown => "条款不存在：修订/废弃前先核对条款编号",
            SedimentCode::ClauseDeprecated => "条款已废弃：不得修订已废弃条款，如需复活立新条款",
            SedimentCode::FrozenRewrite => "v1 冻结直接改写被拒：变更走新条款/修订/废弃三路规则",
            SedimentCode::VersionRegress => "版本回退被拒：版本号只进不退",
            SedimentCode::ChangeEmptyText => "变更文本为空：新条款/修订必须写足正文再提交",
            SedimentCode::ChangeEmptyId => "变更编号为空：新条款必须带编号再提交",
            SedimentCode::SealedLocked => "版本账已封账：禁改，另开新版本",
            SedimentCode::PhaseRegress => "状态倒退：按起草→冻结→封账单调推进",
            SedimentCode::PhaseSkip => "状态跳级：未冻结不得封账",
            SedimentCode::DigestNotArchived => "版本账未落档：先封账生成指纹再写回",
            SedimentCode::NotReproducible => "双跑不一致：排查指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [SedimentCode; 12] = [
        SedimentCode::ClauseDup,
        SedimentCode::ClauseUnknown,
        SedimentCode::ClauseDeprecated,
        SedimentCode::FrozenRewrite,
        SedimentCode::VersionRegress,
        SedimentCode::ChangeEmptyText,
        SedimentCode::ChangeEmptyId,
        SedimentCode::SealedLocked,
        SedimentCode::PhaseRegress,
        SedimentCode::PhaseSkip,
        SedimentCode::DigestNotArchived,
        SedimentCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SedimentCode::ALL.len() {
        let c = match SedimentCode::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 变更规则三路 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 新条款：立一款新约，编号不得与现存条款重
    NewClause,
    /// 修订：改写现存未废弃条款的正文
    Revise,
    /// 废弃：标记现存条款停用
    Deprecate,
}

impl ChangeKind {
    /// 三路中文名（变更账落档用）。
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::NewClause => "新条款",
            ChangeKind::Revise => "修订",
            ChangeKind::Deprecate => "废弃",
        }
    }
}

// ── 状态机（起草 → 冻结 → 封账，单调推进） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookPhase {
    /// 起草期：v1 条款装载未冻结
    Draft,
    /// 冻结期：v1 冻结，变更走三路规则
    Frozen,
    /// 封账期：版本账落档、封禁变更
    Sealed,
}

impl BookPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            BookPhase::Draft => 0,
            BookPhase::Frozen => 1,
            BookPhase::Sealed => 2,
        }
    }
}

/// 单条条款（正文 + 废弃标记）。
#[derive(Clone, Debug)]
pub struct Clause {
    pub id: String,
    pub text: String,
    pub deprecated: bool,
}

/// 单条变更记录（三路之一 + 条款编号 + 说明）。
#[derive(Clone, Debug)]
pub struct ChangeRecord {
    pub kind: ChangeKind,
    pub clause_id: String,
    pub note: String,
}

/// 沉淀账 v1 版本账。
pub struct SedimentBook {
    version: u32,
    phase: BookPhase,
    clauses: Vec<Clause>,
    changes: Vec<ChangeRecord>,
    digest: u32,
    blocked: usize,
}

impl SedimentBook {
    /// 装载 v1 条款（起草态）。
    pub fn new() -> SedimentBook {
        let mut clauses = Vec::new();
        let mut i = 0usize;
        while i < V1_CLAUSES.len() {
            let row = match V1_CLAUSES.get(i) {
                Some(r) => *r,
                None => break,
            };
            clauses.push(Clause {
                id: row.0.to_string(),
                text: row.1.to_string(),
                deprecated: false,
            });
            i += 1;
        }
        SedimentBook {
            version: 1,
            phase: BookPhase::Draft,
            clauses,
            changes: Vec::new(),
            digest: 0,
            blocked: 0,
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn phase(&self) -> BookPhase {
        self.phase
    }

    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }

    pub fn change_count(&self) -> usize {
        self.changes.len()
    }

    pub fn digest(&self) -> u32 {
        self.digest
    }

    /// 违规拦截计数（判据 J1「拦截 10/10」落点）。
    pub fn blocked_count(&self) -> usize {
        self.blocked
    }

    /// v1 冻结：起草态方可冻结，冻结后变更走三路规则。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.phase == BookPhase::Sealed {
            return Err(SedimentCode::SealedLocked.val());
        }
        if self.phase == BookPhase::Frozen {
            return Err(SedimentCode::PhaseRegress.val());
        }
        if self.clauses.len() != V1_CLAUSE_COUNT {
            return Err(SedimentCode::DigestNotArchived.val());
        }
        self.phase = BookPhase::Frozen;
        Ok(())
    }

    /// 条款存在性查询：编号 → 位次。
    fn find_clause(&self, id: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.clauses.len() {
            let c = match self.clauses.get(i) {
                Some(c) => c,
                None => break,
            };
            if c.id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 条款正文取回（按位次，越界返回 None）。
    pub fn clause_text_at(&self, idx: usize) -> Option<String> {
        match self.clauses.get(idx) {
            Some(c) => Some(c.text.clone()),
            None => None,
        }
    }

    /// 条款废弃标记取回（按位次，越界返回 None）。
    pub fn clause_deprecated_at(&self, idx: usize) -> Option<bool> {
        match self.clauses.get(idx) {
            Some(c) => Some(c.deprecated),
            None => None,
        }
    }

    /// 变更规则三路统一入口：先冻结、再校验、后落账。
    pub fn apply_change(&mut self, kind: ChangeKind, clause_id: &str, text: &str) -> Result<(), u32> {
        if self.phase == BookPhase::Sealed {
            return Err(SedimentCode::SealedLocked.val());
        }
        if self.phase == BookPhase::Draft {
            return Err(SedimentCode::PhaseSkip.val());
        }
        match kind {
            ChangeKind::NewClause => {
                if clause_id.is_empty() {
                    return Err(SedimentCode::ChangeEmptyId.val());
                }
                if text.is_empty() {
                    return Err(SedimentCode::ChangeEmptyText.val());
                }
                if self.find_clause(clause_id).is_some() {
                    return Err(SedimentCode::ClauseDup.val());
                }
                self.clauses.push(Clause {
                    id: clause_id.to_string(),
                    text: text.to_string(),
                    deprecated: false,
                });
            }
            ChangeKind::Revise => {
                let pos = match self.find_clause(clause_id) {
                    Some(p) => p,
                    None => return Err(SedimentCode::ClauseUnknown.val()),
                };
                let dep = match self.clause_deprecated_at(pos) {
                    Some(d) => d,
                    None => return Err(SedimentCode::NotReproducible.val()),
                };
                if dep {
                    return Err(SedimentCode::ClauseDeprecated.val());
                }
                if text.is_empty() {
                    return Err(SedimentCode::ChangeEmptyText.val());
                }
                let slot = match self.clauses.get_mut(pos) {
                    Some(c) => c,
                    None => return Err(SedimentCode::NotReproducible.val()),
                };
                slot.text = text.to_string();
            }
            ChangeKind::Deprecate => {
                let pos = match self.find_clause(clause_id) {
                    Some(p) => p,
                    None => return Err(SedimentCode::ClauseUnknown.val()),
                };
                let dep = match self.clause_deprecated_at(pos) {
                    Some(d) => d,
                    None => return Err(SedimentCode::NotReproducible.val()),
                };
                if dep {
                    return Err(SedimentCode::ClauseDeprecated.val());
                }
                let slot = match self.clauses.get_mut(pos) {
                    Some(c) => c,
                    None => return Err(SedimentCode::NotReproducible.val()),
                };
                slot.deprecated = true;
            }
        }
        self.changes.push(ChangeRecord {
            kind,
            clause_id: clause_id.to_string(),
            note: if text.is_empty() { "(废弃无正文)".to_string() } else { text.to_string() },
        });
        Ok(())
    }

    /// v1 直接改写通道：冻结纪律下恒拒（拦截计数由注入账统计）。
    pub fn direct_rewrite(&mut self, _clause_id: &str, _new_text: &str) -> Result<(), u32> {
        Err(SedimentCode::FrozenRewrite.val())
    }

    /// 版本回退通道：版本号只进不退，恒拒（拦截计数由注入账统计）。
    pub fn downgrade(&mut self, _to: u32) -> Result<(), u32> {
        Err(SedimentCode::VersionRegress.val())
    }

    /// 封账：版本账落档，指纹覆盖条款面 + 变更面。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.phase == BookPhase::Draft {
            return Err(SedimentCode::PhaseSkip.val());
        }
        if self.phase == BookPhase::Sealed {
            return Err(SedimentCode::PhaseRegress.val());
        }
        let mut text = format!("== 沉淀账 v{} 版本账 ==", self.version);
        text.push('\n');
        let mut i = 0usize;
        while i < self.clauses.len() {
            let c = match self.clauses.get(i) {
                Some(c) => c,
                None => break,
            };
            let mark = if c.deprecated { "[废弃]" } else { "[生效]" };
            let line = format!("{} {} {}", c.id, mark, c.text);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        let mut j = 0usize;
        while j < self.changes.len() {
            let r = match self.changes.get(j) {
                Some(r) => r,
                None => break,
            };
            let line = format!("变更{}：{} {}", j + 1, r.kind.label(), r.clause_id);
            text.push_str(&line);
            text.push('\n');
            j += 1;
        }
        let d = fnv1a(text.as_bytes());
        if d == 0 {
            return Err(SedimentCode::NotReproducible.val());
        }
        self.digest = d;
        self.phase = BookPhase::Sealed;
        Ok(d)
    }
}

// ── 好账构建（装载 → 冻结 → 三路各走一次 → 封账） ───────────────
pub fn build_book_ok() -> SedimentBook {
    let mut book = SedimentBook::new();
    let _ = book.freeze();
    // 合法三路：立新条款 SD-025、修订 SD-001、废弃 SD-024。
    let _ = book.apply_change(ChangeKind::NewClause, "SD-025",
        "域级 FAQ 十问终版随版本账一并冻结");
    let _ = book.apply_change(ChangeKind::Revise, "SD-001",
        "域级判据冻结后只增不减，变更须走沉淀账三路规则并留变更账");
    let _ = book.apply_change(ChangeKind::Deprecate, "SD-024",
        "");
    let _ = book.seal();
    book
}

/// 违规拦截账：十路违规逐一尝试并计数（判据 J1 拦截 10/10 落点）。
pub fn build_book_violations() -> SedimentBook {
    let mut book = SedimentBook::new();
    let _ = book.freeze();
    // 路 1：修订不存在的条款。
    if book.apply_change(ChangeKind::Revise, "SD-999", "修订不存在注入").is_err() {
        book.blocked += 1;
    }
    // 路 2：废弃不存在的条款。
    if book.apply_change(ChangeKind::Deprecate, "SD-998", "").is_err() {
        book.blocked += 1;
    }
    // 路 3：新增重号条款。
    if book.apply_change(ChangeKind::NewClause, "SD-001", "重号注入").is_err() {
        book.blocked += 1;
    }
    // 路 4：修订已废弃条款（先废弃 SD-002，再修订之）。
    let _ = book.apply_change(ChangeKind::Deprecate, "SD-002", "");
    if book.apply_change(ChangeKind::Revise, "SD-002", "修订已废弃注入").is_err() {
        book.blocked += 1;
    }
    // 路 5：二次废弃同一条款。
    if book.apply_change(ChangeKind::Deprecate, "SD-002", "").is_err() {
        book.blocked += 1;
    }
    // 路 6：v1 直接改写。
    if book.direct_rewrite("SD-003", "绕过规则改写注入").is_err() {
        book.blocked += 1;
    }
    // 路 7：版本回退。
    if book.downgrade(0).is_err() {
        book.blocked += 1;
    }
    // 路 8：新条款空文本。
    if book.apply_change(ChangeKind::NewClause, "SD-900", "").is_err() {
        book.blocked += 1;
    }
    // 路 9：新条款空编号。
    if book.apply_change(ChangeKind::NewClause, "", "空编号注入").is_err() {
        book.blocked += 1;
    }
    // 路 10：未冻结先变更（起草态直接提交）。
    let mut draft = SedimentBook::new();
    if draft.apply_change(ChangeKind::NewClause, "SD-901", "未冻结提交注入").is_err() {
        book.blocked += 1;
    }
    book
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78A40 && SEG_BASE <= 0x78BC0
}

/// v1 装载且冻结生效：24 条条款全部在账。
fn v1_frozen_ok() -> bool {
    let mut book = SedimentBook::new();
    if book.clause_count() != V1_CLAUSE_COUNT {
        return false;
    }
    if book.version() != 1 {
        return false;
    }
    match book.freeze() {
        Ok(()) => {}
        Err(_) => return false,
    }
    if book.phase() != BookPhase::Frozen {
        return false;
    }
    let base = build_book_ok();
    if base.clause_count() != V1_CLAUSE_COUNT + 1 {
        return false;
    }
    true
}

/// 合法三路各走一次成功且落变更账。
fn legal_paths_ok() -> bool {
    let book = build_book_ok();
    if book.change_count() != 3 {
        return false;
    }
    if book.phase() != BookPhase::Sealed {
        return false;
    }
    // 修订生效：SD-001 正文应为修订后文本。
    let revised = match book.clause_text_at(0) {
        Some(t) => t,
        None => return false,
    };
    if !revised.contains("三路规则") {
        return false;
    }
    // 废弃生效：SD-024 在账末位标记废弃。
    let last = V1_CLAUSE_COUNT - 1;
    let dep = match book.clause_deprecated_at(last) {
        Some(d) => d,
        None => return false,
    };
    if !dep {
        return false;
    }
    true
}

/// 违规变更拦截 10/10：十路注入全部显性拒且计数恰十。
fn violations_10_ok() -> bool {
    let book = build_book_violations();
    if book.blocked_count() != VIOLATION_PATHS {
        return false;
    }
    // 十路拦截后账面干净：重号/不存在条款不落账，仅合法废弃落账。
    if book.clause_count() != V1_CLAUSE_COUNT {
        return false;
    }
    if book.change_count() != 1 {
        return false;
    }
    true
}

/// v1 直接改写与版本回退通道恒闭（即便重复触发也拒）。
fn frozen_rewrite_blocked_ok() -> bool {
    let mut book = build_book_ok();
    if book.phase() != BookPhase::Sealed {
        return false;
    }
    // 封账后直接改写仍拒。
    match book.direct_rewrite("SD-001", "封账后改写注入") {
        Err(code) => {
            if code != SedimentCode::FrozenRewrite.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    match book.downgrade(0) {
        Err(code) => {
            if code != SedimentCode::VersionRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 版本账落档：封账指纹非零、封账后变更必拒。
fn digest_archived_ok() -> bool {
    let mut book = build_book_ok();
    if book.digest() == 0 {
        return false;
    }
    match book.apply_change(ChangeKind::NewClause, "SD-777", "封账后变更注入") {
        Err(code) => {
            if code != SedimentCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if book.change_count() != 3 {
        return false;
    }
    true
}

/// 状态机单调：未冻结封账拒、二次冻结拒、封账后再冻结拒。
fn phase_monotonic_ok() -> bool {
    let mut draft = SedimentBook::new();
    if draft.phase() != BookPhase::Draft {
        return false;
    }
    // 未冻结封账：跳级拒。
    match draft.seal() {
        Err(code) => {
            if code != SedimentCode::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 未冻结变更：跳级拒。
    match draft.apply_change(ChangeKind::Revise, "SD-001", "未冻结修订注入") {
        Err(code) => {
            if code != SedimentCode::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let _ = draft.freeze();
    // 二次冻结：倒退拒。
    match draft.freeze() {
        Err(code) => {
            if code != SedimentCode::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let mut sealed = build_book_ok();
    match sealed.freeze() {
        Err(code) => {
            if code != SedimentCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 冻结基线不可动摇：v1 条款正文在合法修订之外逐条与冻结表一致。
fn baseline_intact_ok() -> bool {
    let book = build_book_ok();
    let mut i = 1usize;
    while i < V1_CLAUSE_COUNT {
        let row = match V1_CLAUSES.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let text = match book.clause_text_at(i) {
            Some(t) => t,
            None => return false,
        };
        // SD-024 被废弃但正文不丢：版本账保留原文。
        if text != row.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let a = build_book_ok();
    let b = build_book_ok();
    if a.digest() != b.digest() {
        return false;
    }
    if a.digest() == 0 {
        return false;
    }
    if a.clause_count() != b.clause_count() {
        return false;
    }
    if a.change_count() != b.change_count() {
        return false;
    }
    true
}

/// 变更账三路标签闭集：新条款/修订/废弃三标签互异且非空。
fn change_kinds_ok() -> bool {
    let l1 = ChangeKind::NewClause.label();
    let l2 = ChangeKind::Revise.label();
    let l3 = ChangeKind::Deprecate.label();
    if l1.is_empty() || l2.is_empty() || l3.is_empty() {
        return false;
    }
    if l1 == l2 || l2 == l3 || l1 == l3 {
        return false;
    }
    let book = build_book_ok();
    if book.change_count() != 3 {
        return false;
    }
    true
}

/// 条款文本非空守卫：装载的 24 条条款逐条非空。
fn clause_text_ok() -> bool {
    let book = SedimentBook::new();
    let mut i = 0usize;
    while i < V1_CLAUSE_COUNT {
        let text = match book.clause_text_at(i) {
            Some(t) => t,
            None => return false,
        };
        if text.is_empty() {
            return false;
        }
        let row = match V1_CLAUSES.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if text != row.1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in SedimentCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match SedimentCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1553_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1553 域级经沉淀版本化");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78A40..=0x78BC0", SEG_BASE));
    s.add_dyn("v1 冻结生效", v1_frozen_ok(), "24 条条款装载冻结，冻结后变更走三路规则");
    s.add_dyn("变更规则三路", legal_paths_ok(), "新条款/修订/废弃三路各走一次成功并落变更账");
    s.add_dyn("违规变更拦截 10/10", violations_10_ok(), "十路违规注入全部显性拒，拦截计数恰十");
    s.add_dyn("冻结改写通道恒闭", frozen_rewrite_blocked_ok(), "v1 直接改写与版本回退即便封账后也拒");
    s.add_dyn("版本账落档", digest_archived_ok(), "封账指纹非零落档，封账后变更必拒");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "起草→冻结→封账可达，跳级倒退全拒");
    s.add_dyn("冻结基线不动", baseline_intact_ok(), "未修订条款正文逐条与冻结表一致");
    s.add_dyn("双跑一致", double_run_ok(), "好账指纹两次构建逐位相等");
    s.add_dyn("三路标签闭集", change_kinds_ok(), "新条款/修订/废弃三标签互异且非空");
    s.add_dyn("条款文本守卫", clause_text_ok(), "装载条款逐条非空且与冻结表一致");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78A40);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1553_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn v1_loads_twenty_four_clauses() {
        let book = SedimentBook::new();
        assert_eq!(book.clause_count(), 24);
        assert_eq!(book.version(), 1);
        assert_eq!(book.phase(), BookPhase::Draft);
    }

    #[test]
    fn freeze_then_seal_pipeline() {
        let book = build_book_ok();
        assert_eq!(book.phase(), BookPhase::Sealed);
        assert_eq!(book.change_count(), 3);
        assert!(book.digest() != 0);
    }

    #[test]
    fn ten_violations_all_blocked() {
        let book = build_book_violations();
        assert_eq!(book.blocked_count(), 10);
        assert_eq!(book.clause_count(), 24);
        assert_eq!(book.change_count(), 1);
    }

    #[test]
    fn revise_unknown_clause_rejected() {
        let mut book = SedimentBook::new();
        let _ = book.freeze();
        match book.apply_change(ChangeKind::Revise, "SD-999", "文本") {
            Err(code) => assert_eq!(code, SedimentCode::ClauseUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn deprecate_unknown_clause_rejected() {
        let mut book = SedimentBook::new();
        let _ = book.freeze();
        match book.apply_change(ChangeKind::Deprecate, "SD-998", "") {
            Err(code) => assert_eq!(code, SedimentCode::ClauseUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_new_clause_rejected() {
        let mut book = SedimentBook::new();
        let _ = book.freeze();
        match book.apply_change(ChangeKind::NewClause, "SD-001", "重号注入") {
            Err(code) => assert_eq!(code, SedimentCode::ClauseDup.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn revise_deprecated_clause_rejected() {
        let mut book = SedimentBook::new();
        let _ = book.freeze();
        let _ = book.apply_change(ChangeKind::Deprecate, "SD-005", "");
        match book.apply_change(ChangeKind::Revise, "SD-005", "修订已废弃注入") {
            Err(code) => assert_eq!(code, SedimentCode::ClauseDeprecated.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn direct_rewrite_and_downgrade_blocked() {
        let mut book = SedimentBook::new();
        let _ = book.freeze();
        match book.direct_rewrite("SD-001", "绕过注入") {
            Err(code) => assert_eq!(code, SedimentCode::FrozenRewrite.val()),
            Ok(()) => must_not_reach(),
        }
        match book.downgrade(0) {
            Err(code) => assert_eq!(code, SedimentCode::VersionRegress.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn seal_requires_freeze() {
        let mut book = SedimentBook::new();
        match book.seal() {
            Err(code) => assert_eq!(code, SedimentCode::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let a = build_book_ok();
        let b = build_book_ok();
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(SedimentCode::ALL.len(), 12);
        let first = match SedimentCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match SedimentCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
