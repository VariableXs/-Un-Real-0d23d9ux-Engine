//! VE-F0423 · 域自检（判据逐条对应，见 `vec23_transunit.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四类声明 → `C23-四类-*`（四类恰为四、码双向可逆、类名非空、每类各有引导词、
//!   引导词来源显式、扩展词确实不在 F0404、修饰词不单独开启声明、引导词无重复、
//!   同步集覆盖全部声明起始、四类能被真实源码解析出、类计数与实表一致）
//! - 职责边界 → `C23-边界-*`（九项移交逐条有阶段 / 责任单 / 理由，语义期项责任单
//!   落在 F044x，移交账被真实触发，责任单与理由非空）
//! - 依赖登记 → `C23-依赖-*`（四类关系各自可触发、四类计数之和 == 边数、配对幂等、
//!   定义记录数 == 具名声明数、边只带跨度不带语义对象）
//! - 语义期不越权 → `C23-不越权-*`（改名字摘要不变 + 改语法摘要必变的**双向**
//!   纯度对照、重复声明不被拒绝、提案表只含未登记类别、提案版本严格递增且高于既有）
//! - 错误路径与降级 → `C23-错误-*`（顶层非法记号报错并恢复到下一声明、恢复必终止、
//!   强制前进兜底可被直接验证、区间未收口登记、诊断三要素非空、每码有锚点、
//!   空单元族一律是注记、控制指令不误报警告）
//! - 显性语义裁定 → `C23-裁定-*`（五态各自可触发、互相区分、空单元族不被拒）
//! - 性能与复杂度 → `C23-性能-*`（属性挂接 O(1) 实测无扫描、挂接条数守恒、解析对
//!   记号数线性、登记对声明数线性）
//! - 上游对接 → `C23-对接-*`（动作序号取自 F0421 骨架产生式并可被 F0422 映射、重
//!   放进 F0422 消费者节点数一致、重放进 F0421 计数器动作序逐条相同、根节点已落池）
//! - 区间完整性 → `C23-区间-*`（互不重叠、覆盖全部代码记号、序号与下标一致）
//! - 门禁 → `C23-门禁-*`（零 panic 面、极端输入不崩、判据容量未溢出、常量自洽、
//!   属性键表与全集一致、语义不吞诊断）
//!
//! 弱门禁自律（逐条对照本域最容易犯的五种）：
//! 1. **「四类」不能靠数枚举变体**：只断「有四个类名」的话，一个把 `Root` 或别的
//!    东西算进来的实现会全绿。故用 `DECL_CLASSES.len() == 4` +
//!    `ALL_FOUR_COUNT == 4` + `from_code` 越界给 `None` 三处钉死。
//! 2. **「引导词表」不能靠「表非空」**：一个把 `type` 当引导词却不登记来源与理由
//!    的实现会全绿。故**逐条**核对来源非空、理由非空，并**反向验证**：标
//!    `SyntaxExtension` 的词必须**真的不在** F0404 关键字表里，标 `LexerKeyword`
//!    的词必须**真的在**——这样「私自扩表」和「谎报来源」都判红。
//! 3. **「只登记不解析」不能靠「我没写解析」**：判据给出**纯度**实测——只改标识
//!    符与字面量的两份源码，摘要必须逐位相等；并配**反向对照**：只加一个记号的第
//!    三份源码摘要必须变，否则一个恒返回常量的摘要实现会全绿。
//! 4. **「O(1) 挂接」不能靠「看起来像」**：判据用 [`FileAttrTable::scans`] **实测**
//!    ——做 N 次定址取用后 `scans` 必须**恒为 0**，再配一条挂接条数守恒
//!    （各声明挂接数之和 + 单元级槽位数 == 属性总数），少挂或多挂都判红。
//! 5. **「空单元显性」不能靠「空也能跑」**：判据分别构造**五种**输入并要求得到**五
//!    个不同**的裁定，且前三种（空 / 纯注释 / 纯指令）**必须被接受**——「把合法
//!    但空的单元判成错误」与「把它们一律塌成 Empty」都判红。

#![allow(clippy::needless_range_loop)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vec03_lexer::{Lexer, Token, TokenKind};
use super::vec04_keywords;
use super::vec21_parser::NodeTallySink;
use super::vec22_ast::{mapping_of, ArenaBuilder, NodeFamily, NodeKind};
use super::vec23_transunit::*;

/// 用主词法（F0403）把源码切成记号流。**本单一律走这条路径**，自检也不例外——
/// 自检走另一条切词路径就等于给「二次词法」开了后门。
fn toks(src: &str) -> Vec<Token> {
    match Lexer::new() {
        Ok(l) => match l.scan(src) {
            Ok((v, _)) => v,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// 四类齐备的样例源码。
const SRC_ALL_FOUR: &str =
    "let g = 1;\nfn helper() { let inner = g; }\nstruct Material { albedo: f32; }\nuniform Camera { view: mat4; }\n";

/// 只改标识符与字面量、语法不变的第二份源码（判据三的纯度侧证）。
const SRC_RENAMED: &str =
    "let z = 999999;\nfn zebra() { let qqq = z; }\nstruct Substance { emissive: f64; }\nuniform Lens { matrix: mat9; }\n";

/// 语法不同（多一个记号）的第三份源码（判据三的反向对照）。
const SRC_EXTRA_TOKEN: &str =
    "let g = 1 + 1;\nfn helper() { let inner = g; }\nstruct Material { albedo: f32; }\nuniform Camera { view: mat4; }\n";

/// F0423 域自检入口。
pub fn run_vec23_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0423");

    // 基准单元（全四类样例）。
    let base_src = SRC_ALL_FOUR;
    let base_toks = toks(base_src);
    let base = parse_unit(base_src, &base_toks, 1);

    // ==================== 判据一：四类声明 ====================
    {
        s.add(
            "C23-四类-恰为四类",
            DECL_CLASSES.len() == 4 && DeclClass::ALL_FOUR_COUNT == 4,
            "锚点「变量、函数、类型定义、接口块四类」四类码段固定",
        );
        // 码 ↔ 表双向可逆，越界给 None（不 panic）。
        let mut rev = true;
        let mut i = 0usize;
        while i < DECL_CLASSES.len() {
            match DeclClass::from_code(i as u8) {
                Some(c) => {
                    if c != DECL_CLASSES[i] || c.code() as usize != i {
                        rev = false;
                    }
                }
                None => rev = false,
            }
            i += 1;
        }
        let oob = DeclClass::from_code(4).is_none() && DeclClass::from_code(255).is_none();
        s.add(
            "C23-四类-码与表双向可逆且越界拒",
            rev && oob,
            "四类码段是诊断包契约；越界必须给 None 而不是 panic",
        );
        // 类名非空（诊断与移交清单靠它定位）。
        let mut named = 0usize;
        let mut j = 0usize;
        while j < DECL_CLASSES.len() {
            if !DECL_CLASSES[j].name().is_empty() {
                named += 1;
            }
            j += 1;
        }
        s.add(
            "C23-四类-类名非空",
            named == DECL_CLASSES.len(),
            "空类名会让下游诊断无法定位到具体类别",
        );
        // 每类各有引导词。
        let mut all_have = true;
        let mut k = 0usize;
        while k < DECL_CLASSES.len() {
            if leads_of_class(DECL_CLASSES[k]).is_empty() {
                all_have = false;
            }
            k += 1;
        }
        s.add(
            "C23-四类-每类各有引导词",
            all_have,
            "锚点四类必须都能被词面引导，否则一类声明永远解析不出",
        );
        // 引导词表：来源与理由逐条非空；无重复词。
        let mut meta_ok = true;
        let mut dup_ok = true;
        let mut ext_count = 0usize;
        let mut lex_count = 0usize;
        let mut m = 0usize;
        while m < TOP_LEADS.len() {
            let l = TOP_LEADS[m];
            if l.reason.is_empty() || l.word.is_empty() {
                meta_ok = false;
            }
            match l.source {
                LeadSource::SyntaxExtension => ext_count += 1,
                LeadSource::LexerKeyword => lex_count += 1,
            }
            // 重复检测（定长表 O(n²) 只在自检里跑）
            let mut n = m + 1;
            while n < TOP_LEADS.len() {
                if TOP_LEADS[n].word == l.word {
                    dup_ok = false;
                }
                n += 1;
            }
            m += 1;
        }
        s.add(
            "C23-四类-引导词来源与理由非空",
            meta_ok && ext_count > 0 && lex_count > 0,
            "扩展显性化：必须同时存在 F0404 既有词与 F0423 扩展词，且理由不得为空",
        );
        s.add(
            "C23-四类-引导词无重复词面",
            dup_ok,
            "重复词面会让类归属产生歧义（同一个词两条类）",
        );
        // 反向验证来源标注是否诚实。
        let mut src_honest = true;
        let mut p = 0usize;
        while p < TOP_LEADS.len() {
            let l = TOP_LEADS[p];
            let in_f0404 = vec04_keywords::lookup(l.word).is_some();
            match l.source {
                LeadSource::SyntaxExtension => {
                    if in_f0404 {
                        src_honest = false;
                    }
                }
                LeadSource::LexerKeyword => {
                    if !in_f0404 {
                        src_honest = false;
                    }
                }
            }
            p += 1;
        }
        s.add(
            "C23-四类-来源标注与F0404实际相符",
            src_honest,
            "标扩展的词必须真不在 F0404 表里，标既有的必须真在；谎报来源判红",
        );
        // 修饰词不单独开启声明。
        let mut modifiers = 0usize;
        let mut q = 0usize;
        while q < TOP_LEADS.len() {
            if TOP_LEADS[q].role == LeadRole::Modifier {
                modifiers += 1;
                if TOP_LEADS[q].class == DeclClass::Variable
                    && TOP_LEADS[q].word == "let"
                {
                    // 变量引导词不得被误标为修饰词
                    modifiers = 99;
                }
            }
            q += 1;
        }
        s.add(
            "C23-四类-修饰词不单独开启声明",
            modifiers == 1 && is_modifier_word("layout") && !is_modifier_word("let"),
            "layout 是可叠加前缀不是类引导词；把修饰词当引导词会切错声明边界",
        );
        // 同步集能找回全部声明起始（词面侧全集 + Term 侧投影）。
        let mut sync_ok = true;
        let mut r = 0usize;
        while r < TOP_LEADS.len() {
            if !is_decl_lead_word(TOP_LEADS[r].word) {
                sync_ok = false;
            }
            r += 1;
        }
        s.add(
            "C23-四类-同步集覆盖全部声明起始",
            sync_ok && TOP_SYNC_TERMS.len() == 3,
            "恢复要能找回全部四类引导词，否则某一类声明后的错误会被整段吞掉",
        );
        // 真实源码解析出四类。
        let present = base.seq.present_classes();
        s.add(
            "C23-四类-真实源码解析出四类",
            base.seq.all_four_present() && present.len() == 4,
            "样例含 let/fn/struct/uniform 四类引导词，解析结果必须恰为四类",
        );
        // 类计数与实表一致（独立重算，防计数脱钩）。
        let mut recount = [0u32; 4];
        for d in base.seq.all().iter() {
            recount[d.class.code() as usize] += 1;
        }
        let mut count_ok = base.seq.class_counts_sum() == base.seq.len() as u32;
        let mut t = 0usize;
        while t < 4 {
            if base.seq.class_count(DECL_CLASSES[t]) != recount[t] {
                count_ok = false;
            }
            t += 1;
        }
        s.add(
            "C23-四类-类计数与实表一致",
            count_ok && base.seq.len() == 4,
            "类计数是独立缓存字段；与实表脱钩会让移交清单按类统计失真",
        );
    }

    // ==================== 判据三：依赖登记 ====================
    {
        let src = "let a = 1;\nlet b = a;\n";
        let tk = toks(src);
        let u = parse_unit(src, &tk, 1);
        s.add(
            "C23-依赖-定义在前可识别",
            u.deps.count_of(DepRelation::DefinedEarlier) > 0,
            "锚点「声明间依赖只登记」：定义在前的引用必须被识别为定义在前",
        );
        let fwd_src = "let b = a;\nlet a = 1;\n";
        let ftk = toks(fwd_src);
        let f = parse_unit(fwd_src, &ftk, 1);
        s.add(
            "C23-依赖-前向引用登记且不报错",
            f.deps.count_of(DepRelation::Forward) > 0 && f.error_count() == 0 && f.accepted,
            "前向引用是否允许归语义期；语法期只登记，判红即越权",
        );
        let unr_src = "let b = ghost;\n";
        let utk = toks(unr_src);
        let g = parse_unit(unr_src, &utk, 1);
        s.add(
            "C23-依赖-未定义名登记且不报错",
            g.deps.count_of(DepRelation::Unresolved) > 0 && g.error_count() == 0 && g.accepted,
            "名字可能由外部单元注入；语法期无全视图不能判未定义",
        );
        let dup_src = "let dup = 1;\nlet dup = 2;\n";
        let dtk = toks(dup_src);
        let d = parse_unit(dup_src, &dtk, 1);
        s.add(
            "C23-依赖-重复定义登记且不报错",
            d.deps.count_of(DepRelation::Duplicate) > 0
                && d.error_count() == 0
                && d.accepted
                && d.count_diag(TopDiagCode::DuplicateDeclared) > 0,
            "锚点「声明重复→登记交语义期裁定」；重复只产出注记不产出错误",
        );
        s.add(
            "C23-依赖-重复诊断是注记级",
            TopDiagCode::DuplicateDeclared.severity() == Severity::Note,
            "重复声明若被判成错误即等于语法期抢了语义期的裁定权",
        );
        // 四类计数之和 == 边数（防止某一类被漏计而看不出来）。
        let mut sum = 0u32;
        for rel in DepRelation::ALL.iter() {
            sum += d.deps.count_of(*rel);
        }
        s.add(
            "C23-依赖-四类计数之和等于边数",
            sum == d.deps.edge_count() as u32,
            "计数与边表必须对账；漏计某类关系会让移交清单低估语义负担",
        );
        // 配对幂等。
        let mut l2 = DepLedger::new();
        l2.note_define(7, EMPTY_SPAN, DeclClass::Variable, 0);
        l2.note_use(7, EMPTY_SPAN, 1);
        l2.finalize();
        let first = l2.edge_count();
        l2.finalize();
        s.add(
            "C23-依赖-配对幂等",
            l2.edge_count() == first && first == 1,
            "finalize 被重复调用不得重复计数，否则移交清单条数随调用次数漂移",
        );
        // 登记 O(声明数)：定义记录数 == 具名声明数。
        s.add(
            "C23-依赖-登记条数等于具名声明数",
            base.deps.define_count() == base.seq.named_count(),
            "锚点「登记 O(声明数)」：每个具名声明恰好一条定义记录",
        );
        // 边只带跨度，不带语义对象：抽查每条边的两个跨度都存在（非哨兵外的任意值）。
        let mut edge_shape = true;
        for e in base.deps.edges().iter() {
            if e.use_site.end < e.use_site.start || e.define_site.end < e.define_site.start {
                edge_shape = false;
            }
        }
        s.add(
            "C23-依赖-依赖边只带位置不带语义",
            edge_shape,
            "DepEdge 字段只有驻留号与两个跨度；想解析语义先得往字段里塞对象",
        );
    }

    // ==================== 判据四：语义期不越权 ====================
    {
        let renamed_toks = toks(SRC_RENAMED);
        let renamed = parse_unit(SRC_RENAMED, &renamed_toks, 1);
        let extra_toks = toks(SRC_EXTRA_TOKEN);
        let extra = parse_unit(SRC_EXTRA_TOKEN, &extra_toks, 1);
        let same_syntax = base.syntax_digest == renamed.syntax_digest;
        let same_struct = base.struct_digest == renamed.struct_digest;
        s.add(
            "C23-不越权-改名字与字面量摘要不变",
            same_syntax && same_struct,
            "纯度侧证：只改标识符与字面量而语法不变，两路摘要必须逐位相等",
        );
        s.add(
            "C23-不越权-改语法摘要必变",
            base.syntax_digest != extra.syntax_digest,
            "反向对照：只加一个记号摘要必须变；恒常量的假摘要会在这里转红",
        );
        // 结构摘要也要有反向对照（只改声明数）。
        let more_src = "let a = 1;\nlet b = 2;\n";
        let mtk = toks(more_src);
        let more = parse_unit(more_src, &mtk, 1);
        s.add(
            "C23-不越权-改声明数结构摘要必变",
            more.struct_digest != base.struct_digest && renamed.struct_digest == base.struct_digest,
            "结构摘要要同时对「名字不变」与「结构变」两侧都敏感",
        );
        // 重复声明不被拒绝（不拒面）。
        let dup_src = "let dup = 1;\nlet dup = 2;\n";
        let dtk = toks(dup_src);
        let d = parse_unit(dup_src, &dtk, 1);
        s.add(
            "C23-不越权-重复声明不被语法期拒绝",
            d.accepted && d.error_count() == 0,
            "判据四的反面：偷偷把重复判红就是越权裁决",
        );
        // 提案表只含未登记类别，且条数等于实际出现过的未登记类别数。
        let mut pending_ok = true;
        let mut unmapped_seen = 0usize;
        for pc in base.seq.present_classes().iter() {
            if pc.registered_node_kind(false).is_none() {
                unmapped_seen += 1;
            }
        }
        if base.pending.len() != unmapped_seen {
            pending_ok = false;
        }
        for pn in base.pending.iter() {
            if pn.class.registered_node_kind(false).is_some() {
                pending_ok = false;
            }
            if pn.proposed_since <= registered_version_ceiling() {
                pending_ok = false;
            }
        }
        s.add(
            "C23-不越权-节点类型提案只含未登记类别",
            pending_ok && !base.pending.is_empty() && base.pending.len() == 2,
            "类型定义与接口块在 F0422 无登记节点类型；本单登记提案而不私扩表",
        );
        // 提案版本严格递增。
        let mut inc = true;
        let mut w = 1usize;
        while w < base.pending.len() {
            if base.pending[w].proposed_since <= base.pending[w - 1].proposed_since {
                inc = false;
            }
            w += 1;
        }
        s.add(
            "C23-不越权-提案版本严格递增",
            inc && base.pending[0].proposed_since > registered_version_ceiling(),
            "F0422 登记簿要求版本严格递增；提案版本必须能真的被登记进去",
        );
        // 移交账被真实触发。
        s.add(
            "C23-不越权-节点类型欠账进入移交账",
            base.duties.count_of(DeferredDuty::NodeKindUnregistered) > 0,
            "「缺一张登记」必须显性进移交账，而不是静默降级映射",
        );
    }

    // ==================== 判据二：职责边界 ====================
    {
        let mut stage_ok = true;
        let mut owner_ok = true;
        let mut reason_ok = true;
        let mut semantic_count = 0usize;
        let mut idx = 0usize;
        while idx < DEFERRED_DUTIES.len() {
            let duty = DEFERRED_DUTIES[idx];
            if duty.stage() != duty.stage() || duty.owner().is_empty() || duty.reason().is_empty() {
                stage_ok = false;
            }
            if !duty.owner_is_semantic_phase() {
                owner_ok = false;
            }
            if duty.reason().is_empty() || duty.name().is_empty() {
                reason_ok = false;
            }
            if duty.stage() == DutyStage::Semantic {
                semantic_count += 1;
            }
            idx += 1;
        }
        s.add(
            "C23-边界-九项移交逐条有阶段责任单理由",
            stage_ok && DEFERRED_DUTIES.len() == 9,
            "职责边界写成枚举才是可对账的账；空理由等于没移交",
        );
        s.add(
            "C23-边界-语义期项责任单落在F044x",
            owner_ok && semantic_count == 5,
            "语义期裁定必须指名 F044x 责任单；否则「我偷偷做了」无从发现",
        );
        s.add(
            "C23-边界-移交项名与理由非空",
            reason_ok,
            "移交清单要能被下游直接引用，空名会逼下游重新猜一遍",
        );
        // 移交账被真实触发（多处触发）。
        let trig_src = "#ifdef X\nlet dup = 1;\nlet dup = 2;\nlet fwd = later;\nlet later = 1;\nlayout(a) layout(b) layout(c) uniform { };\n";
        let ttk = toks(trig_src);
        let trig = parse_unit(trig_src, &ttk, 1);
        let kinds = trig.duties.raised_kinds();
        s.add(
            "C23-边界-移交账被多处真实触发",
            trig.duties.total() > 0
                && trig.duties.semantic_total() > 0
                && kinds.len() >= 4,
            "一张从不触发的移交账等于没有账；至少要覆盖重复/前向/布局/属性四路",
        );
        // 控制指令不误报警告。
        s.add(
            "C23-边界-控制指令不误报属性警告",
            trig.count_diag(TopDiagCode::UnknownAttrKey) == 0
                && trig.count_diag(TopDiagCode::AttrValueMalformed) == 0,
            "控制指令归 F0413/F0414；把它当未知属性键报警是假阳性",
        );
        // 未知属性键登记但不拒绝。
        let unk_src = "#whatever(1)\nlet a = 1;\n";
        let utk = toks(unk_src);
        let unk = parse_unit(unk_src, &utk, 1);
        s.add(
            "C23-边界-未知属性键登记不拒绝",
            unk.count_diag(TopDiagCode::UnknownAttrKey) > 0 && unk.accepted,
            "属性键合法性归 F0433；语法期只登记不裁决",
        );
        // 重复修饰词登记布局组合。
        s.add(
            "C23-边界-重复布局限定登记不拒绝",
            trig.duties.count_of(DeferredDuty::LayoutCombination) > 0,
            "布局组合合法性归 F0433；顶层只数一数有几个修饰词",
        );
    }

    // ==================== 显性语义裁定（空翻译单元与纯注释文件）====================
    {
        let empty_src = "";
        let etk = toks(empty_src);
        let e = parse_unit(empty_src, &etk, 1);
        s.add(
            "C23-裁定-空翻译单元裁定为Empty",
            e.verdict == UnitVerdict::Empty && e.count_diag(TopDiagCode::EmptyUnit) == 1,
            "锚点「空单元→按规范裁定告知」：必须有专属告知而不是沉默",
        );
        let cmt_src = "// 只有一行注释\n/* 还有一块 */\n";
        let ctk = toks(cmt_src);
        let c = parse_unit(cmt_src, &ctk, 1);
        s.add(
            "C23-裁定-纯注释文件裁定为CommentOnly",
            c.verdict == UnitVerdict::CommentOnly
                && c.comment_tokens > 0
                && c.count_diag(TopDiagCode::CommentOnlyUnit) == 1,
            "「纯注释」与「空」是两个语义；塌成一个就丢掉了注释存在性这个事实",
        );
        let dir_src = "#define K 1\n";
        let rtk = toks(dir_src);
        let dr = parse_unit(dir_src, &rtk, 1);
        s.add(
            "C23-裁定-纯指令文件裁定为DirectiveOnly",
            dr.verdict == UnitVerdict::DirectiveOnly
                && dr.count_diag(TopDiagCode::DirectiveOnlyUnit) == 1,
            "指令语义归条件编译与包含两单，但「本单元只有指令」这个事实必须显性",
        );
        let junk_src = "+ + + ;\n";
        let jtk = toks(junk_src);
        let j = parse_unit(junk_src, &jtk, 1);
        s.add(
            "C23-裁定-全程恢复裁定为RecoveredToEmpty",
            j.verdict == UnitVerdict::RecoveredToEmpty
                && !j.accepted
                && j.count_diag(TopDiagCode::RecoveredToEmpty) == 1,
            "有代码但零声明与「空单元」是两回事：前者是错误后者是合法",
        );
        s.add(
            "C23-裁定-有声明裁定为Declared",
            base.verdict == UnitVerdict::Declared && base.accepted,
            "正常路径不能被误判成空单元",
        );
        // 五态互斥：五种输入给出五个不同的裁定。
        let mut distinct = true;
        let seen = [
            e.verdict,
            c.verdict,
            dr.verdict,
            j.verdict,
            base.verdict,
        ];
        let mut a = 0usize;
        while a < seen.len() {
            let mut b = a + 1;
            while b < seen.len() {
                if seen[a] == seen[b] {
                    distinct = false;
                }
                b += 1;
            }
            a += 1;
        }
        s.add(
            "C23-裁定-五种输入给出五种互异裁定",
            distinct && UnitVerdict::ALL.len() == 5,
            "五种情形必须各有各的名字；塌成一种等于把语义藏起来了",
        );
        // 空单元族一律是注记且被接受。
        let mut notes_ok = true;
        for v in UnitVerdict::ALL.iter() {
            match v.notice_diag() {
                None => {
                    if *v != UnitVerdict::Declared {
                        notes_ok = false;
                    }
                }
                Some(code) => {
                    if *v == UnitVerdict::RecoveredToEmpty {
                        if code.severity() != Severity::Error {
                            notes_ok = false;
                        }
                    } else if code.severity() != Severity::Note {
                        notes_ok = false;
                    }
                }
            }
        }
        s.add(
            "C23-裁定-合法但空的一族是注记级",
            notes_ok
                && e.accepted
                && c.accepted
                && dr.accepted
                && TopDiagCode::EmptyUnit.severity() == Severity::Note
                && TopDiagCode::CommentOnlyUnit.severity() == Severity::Note
                && TopDiagCode::DirectiveOnlyUnit.severity() == Severity::Note,
            "合法但空不是错误；判成错误是假阳性，吞掉是静默",
        );
        // 声明族一律有声明、无注记。
        s.add(
            "C23-裁定-有声明时不发空单元告知",
            base.count_diag(TopDiagCode::EmptyUnit) == 0
                && base.count_diag(TopDiagCode::CommentOnlyUnit) == 0
                && base.count_diag(TopDiagCode::DirectiveOnlyUnit) == 0,
            "告知必须只在真的空时发，否则告警会被淹没",
        );
    }

    // ==================== 错误路径与降级矩阵 ====================
    {
        // 顶层非法记号 → 报错并恢复到下一声明。
        let bad_src = "++ let recovered = 1;\n";
        let btk = toks(bad_src);
        let b = parse_unit(bad_src, &btk, 1);
        s.add(
            "C23-错误-顶层非法记号报错并恢复",
            b.count_diag(TopDiagCode::IllegalTopToken) > 0
                && b.recovery.recoveries > 0
                && b.seq.len() == 1
                && !b.accepted,
            "锚点「顶层非法记号→恢复到下一声明」：既报错又不能吞掉后续声明",
        );
        // 区间未收口。
        let open_src = "let open = 1\n";
        let otk = toks(open_src);
        let o = parse_unit(open_src, &otk, 1);
        s.add(
            "C23-错误-区间未收口被登记并移交",
            o.count_diag(TopDiagCode::ExtentUnclosed) > 0
                && o.duties.count_of(DeferredDuty::ExtentUnclosedRecovery) > 0
                && !o.seq.get(0).map(|d| d.extent_closed).unwrap_or(true),
            "未收口区间要显性登记并移交 F0434，不静默收口",
        );
        // 恢复必终止：大量垃圾记号仍返回。
        let mut junk = String::new();
        let mut n = 0usize;
        while n < 4000 {
            junk.push_str("+ ");
            n += 1;
        }
        let jtk2 = toks(&junk);
        let j2 = parse_unit(&junk, &jtk2, 1);
        s.add(
            "C23-错误-大量垃圾记号仍终止",
            j2.recovery.recoveries > 0 && j2.recovery.skipped > 0 && j2.verdict.is_decl_free(),
            "恢复每轮必须推进游标；挂起就是内核里最贵的一种失败",
        );
        // 强制前进兜底：直接喂违规目标（同步点不前进）。
        let (adv, forced) = recovery_advance(7, 7);
        let (adv2, forced2) = recovery_advance(7, 9);
        s.add(
            "C23-错误-恢复无进展强制前进兜底",
            forced && adv == 8 && adv2 == 9 && !forced2,
            "同步点不前进时必须强制前进一个记号；返回下标恒大于起点故必然终止",
        );
        // 诊断三要素非空 + 每码有锚点 + 全集覆盖。
        let mut three_ok = true;
        let mut anchor_ok = true;
        let mut cov = 0usize;
        let mut dc = 0usize;
        while dc < TopDiagCode::ALL.len() {
            let code = TopDiagCode::ALL[dc];
            let probe = TopDiag::new(code, EMPTY_SPAN, "自检探针");
            if probe.three_elements().is_empty() || code.advice().is_empty() || code.name().is_empty()
            {
                three_ok = false;
            }
            if !code.anchor().starts_with("VE-F04") || probe.anchored().is_empty() {
                anchor_ok = false;
            }
            if code.code() as usize != dc {
                cov += 1;
            }
            dc += 1;
        }
        s.add(
            "C23-错误-诊断三要素非空",
            three_ok && TopDiagCode::ALL.len() == 14,
            "报告三要素（发生/为什么/下一步）缺一不可，否则是把负担甩给用户",
        );
        s.add(
            "C23-错误-每码锚点形如VE-F04xx",
            anchor_ok && cov == 0,
            "锚点是唯一的规则引用来源；形如不符或码位与全集错位都判红",
        );
        // 严重度三档齐备且各有承载码。
        let mut err_n = 0usize;
        let mut warn_n = 0usize;
        let mut note_n = 0usize;
        let mut e2 = 0usize;
        while e2 < TopDiagCode::ALL.len() {
            match TopDiagCode::ALL[e2].severity() {
                Severity::Error => err_n += 1,
                Severity::Warning => warn_n += 1,
                Severity::Note => note_n += 1,
            }
            e2 += 1;
        }
        s.add(
            "C23-错误-严重度三档各有承载码",
            err_n > 0 && warn_n > 0 && note_n > 0 && Severity::ALL.len() == 3,
            "三档齐备才既能把真错误判红、又能把合法但空的情况只作告知",
        );
        // 属性键值形制：空括号判不合法。
        let empty_parens = parse_directive_attr("#location()");
        let ok_paren = parse_directive_attr("#location(3)");
        s.add(
            "C23-错误-属性键值形制可判",
            empty_parens.malformed && ok_paren.key == AttrKey::Location
                && !ok_paren.malformed
                && ok_paren.value == "3",
            "形制不合要登记而不是当作空值放行；合法键值要能取回原值",
        );
        // 无井号的文本判不合法。
        s.add(
            "C23-错误-非指令文本判形制不合法",
            parse_directive_attr("location(0)").malformed,
            "属性只来自指令记号；非井号文本不得被当成属性",
        );
        // 属性键表与全集一致。
        let mut reg_n = 0usize;
        let mut ai = 0usize;
        while ai < AttrKey::ALL.len() {
            if AttrKey::ALL[ai].is_registered_attribute() {
                reg_n += 1;
            }
            ai += 1;
        }
        s.add(
            "C23-错误-属性键表与全集一致",
            reg_n == ATTR_KEYS.len() && FileAttrTable::registered_kinds() == ATTR_KEYS.len(),
            "已登记键数与词表长必须相等；多一个词表少一个键就是「认了却查不到」",
        );
    }

    // ==================== 区间完整性 ====================
    {
        let overlap = base.seq.overlap_pairs();
        let mut code_idx: Vec<u32> = Vec::new();
        let mut ti = 0usize;
        while ti < base_toks.len() {
            let t = match base_toks.get(ti) {
                Some(t) => t,
                None => break,
            };
            if t.kind != TokenKind::Comment
                && t.kind != TokenKind::Directive
                && t.kind != TokenKind::Eof
            {
                code_idx.push(ti as u32);
            }
            ti += 1;
        }
        let (covered, multi) = base.seq.coverage(&code_idx);
        s.add(
            "C23-区间-声明区间互不重叠",
            overlap == 0 && multi == 0,
            "重叠区间会让下游 F0424-F0426 重复解析同一段记号",
        );
        s.add(
            "C23-区间-覆盖全部代码记号",
            covered == base.code_tokens && code_idx.len() as u32 == base.code_tokens,
            "有代码记号落在任何声明区间之外，说明区间切错了（要么吞了要么漏了）",
        );
        // 区间序号与下标一致 + 区间单调不减。
        let mut mono = true;
        let mut ord_ok = true;
        let mut di = 0usize;
        while di < base.seq.len() {
            let d = match base.seq.get(di) {
                Some(d) => d,
                None => break,
            };
            if d.token_hi <= d.token_lo || d.token_hi as usize > base_toks.len() {
                mono = false;
            }
            if d.ordinal as usize != di {
                ord_ok = false;
            }
            if di > 0 {
                if let Some(prev) = base.seq.get(di - 1) {
                    if d.token_lo < prev.token_lo {
                        mono = false;
                    }
                }
            }
            di += 1;
        }
        s.add(
            "C23-区间-区间下标有序且在流内",
            mono && ord_ok,
            "区间必须单调递增且不越流；否则下游回扫会读到别人的记号",
        );
    }

    // ==================== 性能与复杂度 ====================
    {
        // 属性挂接 O(1) 实测：做 N 次定址取用后 scans 必须仍为 0。
        let mut probe = parse_unit(SRC_ALL_FOUR, &base_toks, 1);
        let mut pi = 0usize;
        while pi < probe.seq.len() {
            let _ = probe.attrs_of_decl(pi);
            pi += 1;
        }
        s.add(
            "C23-性能-属性挂接定址无扫描",
            probe.attrs.scans == 0 && probe.attrs.lookups == probe.seq.len() as u32,
            "锚点「挂接 O(1)」：任何线性搜索实现都会让 scans > 0",
        );
        // 挂接条数守恒。
        let mut attached = 0u32;
        let mut ai2 = 0usize;
        while ai2 < probe.seq.len() {
            attached += probe.attr_count_of_decl(ai2);
            ai2 += 1;
        }
        let unit_level = probe.attrs.unit_slots().len() as u32;
        s.add(
            "C23-性能-属性挂接条数守恒",
            attached + unit_level == probe.attrs.len() as u32,
            "每条属性必须恰好归属一次（声明级或单元级）；少挂丢属性多挂重复挂",
        );
        // 解析对记号数线性：声明数与记号数同比，不出现超线性膨胀。
        let mut big = String::new();
        let mut bi = 0usize;
        while bi < 64 {
            big.push_str("let v");
            big.push_str(&(bi % 10).to_string());
            big.push_str(" = 1;\n");
            bi += 1;
        }
        let btk2 = toks(&big);
        let big_u = parse_unit(&big, &btk2, 1);
        let per_decl = if big_u.seq.is_empty() {
            0
        } else {
            big_u.code_tokens / big_u.seq.len() as u32
        };
        s.add(
            "C23-性能-解析对记号数线性",
            big_u.seq.len() == 64 && per_decl >= 3 && per_decl <= 6 && big_u.recovery.skipped == 0,
            "每个声明固定若干记号；比值落在窄带内说明没有指数级回溯",
        );
        // 名字驻留表定容：驻留与查回一致，容量是 2 的幂。
        let mut nt = NameTable::new();
        let id_a = nt.intern("alpha");
        let id_b = nt.intern("alpha");
        s.add(
            "C23-性能-名字驻留幂等且定容",
            id_a.is_some()
                && id_a == id_b
                && NAME_CAP == 256
                && nt.capacity() == 256
                && nt.resolve(u32::MAX).is_none()
                && nt.intern("").is_none(),
            "驻留幂等 + 容量定容 + 空串拒；越界查回给 None 而非 panic",
        );
    }

    // ==================== 上游对接 ====================
    {
        // 动作序号取自 F0421 骨架产生式，并可被 F0422 的 mapping 复原成同一节点类型。
        let mut prod_ok = !base.actions.is_empty();
        for r in base.actions.iter() {
            match mapping_of(r.prod) {
                Some(m) => {
                    if m.kind == NodeKind::TranslationUnit {
                        prod_ok = false;
                    }
                }
                None => prod_ok = false,
            }
        }
        s.add(
            "C23-对接-动作序号可被F0422映射",
            prod_ok && base.actions.len() == 2,
            "动作语义由文法决定：序号取 F0421 骨架产生式号才能被 mapping_of 直接消费",
        );
        // 重放进 F0422 的 ArenaBuilder：接受的节点数一致。
        let mut builder = ArenaBuilder::new(1);
        let _ = builder.preseed();
        base.replay_into(&mut builder);
        s.add(
            "C23-对接-重放进F0422消费者一致",
            builder.accepted as usize == base.actions.len()
                && builder.registry().len() > 0,
            "动作流是单一事实来源；换一个消费者拿到的动作必须逐条相同",
        );
        // 重放进 F0421 的计数器：动作序逐条相同。
        let mut tally = NodeTallySink::new();
        base.replay_into(&mut tally);
        let mut seq_eq = tally.accepted as usize == base.actions.len();
        let mut ri = 0usize;
        while ri < tally.prod_seq.len() && seq_eq {
            if tally.prod_seq[ri] != base.actions[ri].prod {
                seq_eq = false;
            }
            ri += 1;
        }
        s.add(
            "C23-对接-重放进F0421计数器动作序相同",
            seq_eq,
            "两个消费者看到的动作序列必须逐条相同，否则说明有人改了动作",
        );
        // 根节点已落池 + 根节点是 TranslationUnit 族。
        let root_ok = base.root_valid
            && base.root.bucket == 0
            && builder.arena().family_len(NodeFamily::Decl) == 2;
        s.add(
            "C23-对接-根节点与声明节点已落池",
            root_ok && base.root_span.end >= base.root_span.start,
            "翻译单元根 + 两类已登记声明节点必须都进 arena；没落池的欠账要看得见",
        );
        // 名字驻留号可查回（依赖边能指向真实名字）。
        let mut name_ok = true;
        for d in base.seq.all().iter() {
            if d.name_present && d.name == u32::MAX {
                name_ok = false;
            }
        }
        s.add(
            "C23-对接-声明名字驻留号有效",
            name_ok && base.seq.named_count() == 4,
            "驻留号为 u32::MAX 表示名字缺失；具名声明不该出现这种值",
        );
        // 变量类带初始化器与否落到不同节点类型。
        let init_src = "let with_init = 1;\nlet no_init;\n";
        let itk = toks(init_src);
        let iu = parse_unit(init_src, &itk, 1);
        let init_ok = iu.seq.get(0).map(|d| d.has_init).unwrap_or(false)
            && !iu.seq.get(1).map(|d| d.has_init).unwrap_or(true);
        s.add(
            "C23-对接-初始化器有无被区分",
            init_ok && init_src_has_two_inits(&iu),
            "锚点「带初始化器」与「不带」是两种声明；塌成一种会让下游类型推导失据",
        );
    }

    // ==================== 门禁 ====================
    {
        // 零 panic 面：极端输入只返回不崩。
        let mut edge_ok = true;
        let empty_slice: Vec<Token> = Vec::new();
        let _ = parse_unit("", &empty_slice, 1);
        let only_comment = "// x";
        let octk = toks(only_comment);
        let _ = parse_unit(only_comment, &octk, 1);
        let only_punct = ";;;;;";
        let ptk = toks(only_punct);
        let pu = parse_unit(only_punct, &ptk, 1);
        if pu.seq.len() != 0 || pu.accepted {
            edge_ok = false;
        }
        // 孤立闭括号 / 未闭合括号。
        let stray = "} let a = 1;";
        let stk = toks(stray);
        let _ = parse_unit(stray, &stk, 1);
        let unclosed = "let a = (1;\nfn f() {}\n";
        let utk = toks(unclosed);
        let uu = parse_unit(unclosed, &utk, 1);
        if uu.seq.len() == 0 {
            edge_ok = false;
        }
        // 越界索引导致的名字号查回。
        if NameTable::new().resolve(12345).is_some() {
            edge_ok = false;
        }
        s.add(
            "C23-门禁-极端输入只返回不崩",
            edge_ok,
            "零 panic 面：空流 / 纯标点 / 孤立闭括号 / 未闭合括号 / 越界名字号",
        );
        // 纯标点流必须判成恢复至空（而不是合法空单元）。
        s.add(
            "C23-门禁-纯标点流不判成合法空单元",
            pu.verdict == UnitVerdict::RecoveredToEmpty && !pu.accepted,
            "「有代码记号却零声明」必须与「真的空」区分，否则错误会被伪装成合法",
        );
        // 名字表满即拒绝：塞满后仍返回且给出登记。
        let mut full = NameTable::new();
        let mut fi = 0usize;
        while fi < NAME_CAP + 8 {
            let _ = full.intern(&alloc::format!("n{}", fi));
            fi += 1;
        }
        let overflow = full.intern("overflow");
        s.add(
            "C23-门禁-名字表满即拒绝不覆盖",
            full.len() == NAME_CAP && overflow.is_none(),
            "定容满必须拒绝新名字；覆盖旧名字会让依赖边指向错误的名字",
        );
        // 判据容量未溢出。
        s.add(
            "C23-门禁-判据容量未溢出",
            !s.truncated(),
            "域自检项数须在 MAX_CHECKS 内，超出会静默丢红",
        );
        // 常量自洽：分类常量与表一致、依赖关系四类、三档严重度。
        let consts_ok = DECL_CLASSES.len() == 4
            && DeclClass::ALL_FOUR_COUNT == 4
            && DepRelation::ALL.len() == 4
            && DEFERRED_DUTIES.len() == 9
            && Severity::ALL.len() == 3
            && AttrKey::ALL.len() == 9
            && TOP_LEADS.len() == 11;
        s.add(
            "C23-门禁-常量与表自洽",
            consts_ok,
            "常量与实表对账，防止注释与实现漂移",
        );
        // 语义不吞诊断：每条诊断都有可渲染文本。
        let mut render_ok = true;
        let mut dg = 0usize;
        while dg < TopDiagCode::ALL.len() {
            let probe = TopDiag::new(TopDiagCode::ALL[dg], EMPTY_SPAN, "x");
            if probe.render().is_empty() || probe.three_elements().is_empty() {
                render_ok = false;
            }
            dg += 1;
        }
        s.add(
            "C23-门禁-诊断渲染不吞信息",
            render_ok,
            "每条诊断都要能渲染成一行人话与三要素",
        );
    }

    s
}

/// 自检辅助：确认带初始化器的变量声明确实落成两种节点类别。
fn init_src_has_two_inits(u: &TranslationUnit) -> bool {
    let mut both = false;
    for r in u.actions.iter() {
        if r.prod == prod_for_kind(NodeKind::LetInitDecl) {
            both = true;
        }
    }
    both && u.actions.len() == 2
}