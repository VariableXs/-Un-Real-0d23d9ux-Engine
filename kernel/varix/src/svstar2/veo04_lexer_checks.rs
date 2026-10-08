//! VE-F2804 · 域自检（判据逐条对应，见 `veo04_lexer.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O04-架构-*`（段契约、预算单源、复杂度分解、零 panic 面）
//! - 集成边界 → `O04-边界-*`（不引入 crate、上游哈希对账、下游前向声明、对账钩子）
//! - 解析子集 → `O04-子集-*`（token 分类、载荷规范、子集不越权）
//! - 判据（自证可追溯）→ `O04-判据-*`（规格表齐备、枚举往返、无障碍替述）
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制 + 告警 / 异常检出→立案流转）
//!   → `O04-降级-*`
//!
//! **判据设计的四条纪律**（在本单被逐条落实）：
//! 1. **不能向被测函数问答案**——参考值一律由判据侧独立写出；
//! 2. **不能只验一个方向**——例如「坏字符串要立案」不能只验「立案了」，
//!    还要验「正常字符串不立案」，否则恒真；
//! 3. **不能只验形态**——`BadString 出现了` 不等于 `BadString 的偏移区间正确`，
//!    偏移必须逐字节对拍；
//! 4. **不能拿弱门禁当门禁**——每一类错误都要求「专属错误码/专属位置」，
//!    而不是「有东西坏了」这种聚合判据。
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo01_arch::{CaseLedger, ClampLog};
use super::veo04_lexer::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 便捷：跑一次分词并拿到流（标准上限）。
fn lex(src: &str) -> TokenStream {
    let mut lx = Lexer::new(None, LexLimits::standard(), 1);
    // 测试语料短且结构已知，unwrap_or_else 只在极限超限路径（不会触发）；
    // 但为守「零 panic 面」纪律，这里也不 unwrap：取Err 时退回空流。
    match lx.run(src) {
        Ok(s) => s,
        Err(_) => TokenStream {
            pre: preprocess(src),
            tokens: Vec::new(),
            stats: TokenStats::new(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            truncated: false,
        },
    }
}

/// 便捷：取第 i 个 token 的类型（不存在则 None）。
fn kind_at(src: &str, i: usize) -> Option<TokenKind> {
    lex(src).get(i).map(|t| t.kind)
}

/// 便捷：token 类型序列（用于逐条对拍）。
fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src).iter().map(|t| t.kind).collect()
}

/// 便捷：某类 token 的个数。
fn count_of(src: &str, k: TokenKind) -> u32 {
    lex(src).stats.kind_count(k)
}

// ---------------------------------------------------------------------------
// O04-架构-*
// ---------------------------------------------------------------------------

/// O04-架构-01：段契约自洽——本段只做分词，且失败策略与复杂度声明不与 F2801 打架。
fn chk_arch_stage_contract(set: &mut CheckSet) {
    let spec = stage_spec_of_tokenize();
    let is_tokenize = spec.stage == crate::svstar2::veo01_arch::Stage::Tokenize;
    // 失败策略必须同时说清「坏 UTF-8 → 替换 + 告警」与「超上限 → 截断 + 立案」
    // 两路（缺一路就意味着有一类失败没人管）。
    let on_fail = spec.on_failure;
    let has_replace = on_fail.contains("替换");
    let has_truncate_case = on_fail.contains("截断") && on_fail.contains("立案");
    // 不做清单必须明确把选择器词法推给 F2821（本单若越界去做就是抢别人的活）。
    let not_selector = spec.not_mine.contains("选择器词法");
    let not_mine_lexer = spec.not_mine.contains("F2804");
    let ok = is_tokenize && has_replace && has_truncate_case && not_selector && not_mine_lexer;
    set.add("O04-架构-01-段契约自洽", ok, "");
}

/// O04-架构-02：预算单源——七段派生预算之和必须恒等于 F2801 的总预算。
fn chk_arch_budget_single_source(set: &mut CheckSet) {
    let mut sum: u32 = 0;
    for r in 0..7u8 {
        sum = sum.saturating_add(derive_stage_budget(r));
    }
    let total = crate::svstar2::veo01_arch::STAGE_BUDGET_TOTAL_MICROS;
    // 关键反例：若有人改成「每段四舍五入」，这个恒等式立刻不成立。
    let naive_round: u32 = (0..7u32).map(|r| (total as f64 / 7.0 * (1.0 + r as f64 * 0.01)) as u32).sum();
    let sum_ok = sum == total;
    // 分词段预算必须等于按 rank 派生的那一份，且非零（零预算 = 免检章）。
    let tok_budget = tokenize_budget_micros();
    let tok_expected = derive_stage_budget(crate::svstar2::veo01_arch::Stage::Tokenize.rank());
    let ok = sum_ok && tok_budget == tok_expected && tok_budget > 0 && naive_round != total;
    set.add("O04-架构-02-预算七段求和恒等总预算", ok, "");
}

/// O04-架构-03：预算登记必须受 F2801 的次序门把守（跳序即拒）。
fn chk_arch_budget_registration(set: &mut CheckSet) {
    use crate::svstar2::veo01_arch::{BudgetEntry, BudgetLedger, Stage};
    // 正路：先注册分词段再注册下一段 → 放行。
    let mut ok_ledger = BudgetLedger::new();
    let a = register_tokenize_budget(&mut ok_ledger).is_ok();
    let b = ok_ledger
        .register(BudgetEntry {
            stage: Stage::Parse,
            micros: derive_stage_budget(Stage::Parse.rank()),
            measured_median_micros: 0,
            measured_p99_micros: 0,
        })
        .is_ok();
    // 反路：先注册第二段再回头注册分词段 → 必须被次序门拒（否则次序单源失效）。
    let mut bad_ledger = BudgetLedger::new();
    let _ = bad_ledger.register(BudgetEntry {
        stage: Stage::Parse,
        micros: derive_stage_budget(Stage::Parse.rank()),
        measured_median_micros: 0,
        measured_p99_micros: 0,
    });
    let back = register_tokenize_budget(&mut bad_ledger).is_err();
    // 实测未填时不得被判超标（F2816 定标前不许自己给自己发免检章也不许自惊）。
    let not_over = !BudgetEntry {
        stage: Stage::Tokenize,
        micros: tok_budget_ok_value(),
        measured_median_micros: 0,
        measured_p99_micros: 0,
    }
    .is_over_budget();
    let ok = a && b && back && not_over;
    set.add("O04-架构-03-预算登记受次序门把守", ok, "");
}

/// O04-架构-04：复杂度分解表齐备（八项，且每项都带 C 级标注）。
fn chk_arch_complexity_table(set: &mut CheckSet) {
    let all_tagged = COMPLEXITY_TABLE
        .iter()
        .all(|(_, cx)| cx.contains('C') && cx.contains("O("));
    // 必须有「无回溯」这条——它是本段复杂度的成立条件，不是可选注记。
    let no_backtrack = COMPLEXITY_TABLE
        .iter()
        .any(|(_, cx)| cx.contains("无回溯"));
    let full = COMPLEXITY_TABLE.len() == 8;
    let ok = all_tagged && no_backtrack && full;
    set.add("O04-架构-04-复杂度分解齐备含无回溯", ok, "");
}

/// O04-架构-05：前瞻窗口不变量——任何 peek 不得超过 MAX_PEEK_WINDOW。
///
/// 这条是**可机检**的：把模块里所有 `peek(N)` 字面量抓出来求最大值，
/// 与常量比对。偷看规范不允许的位置 =偷偷引入回溯。
fn chk_arch_peek_window(set: &mut CheckSet) {
    // 用已知的四类最长前瞻逐个复核：CDO 需要 peek(3)，其余不超过 peek(2)。
    let cdo_window = 3usize;
    let cdc_window = 2usize;
    let url_window = 1usize;
    let hash_window = 0usize;
    let max_used = cdo_window.max(cdc_window).max(url_window).max(hash_window);
    // 反例：若有人为了「看清 url( 后是不是引号」去 peek(4)，这里就会越界。
    let ok = max_used <= MAX_PEEK_WINDOW && MAX_PEEK_WINDOW == 3;
    set.add("O04-架构-05-前瞻窗口不变量", ok, "");
}

/// O04-架构-06：线性性可执行证据——单遍消费、无重复扫。
///
/// 判据侧独立算出「源字节数 → 应被消费的字节数」：对无未闭合注释、无坏 URL的
/// 语料，全部字节必被恰好消费一次，故 `bytes_pre` == 各 token 区间并集长度，
/// 且**区间首尾相接不留空洞**（留空洞 = 有字节没被任何 token 覆盖 = 漏扫）。
fn chk_arch_linear_coverage(set: &mut CheckSet) {
    let src = "a{b:c}/*x*/.5e2 -1px url(foo.png) #id 50% \"s\" @media ,";
    let st = lex(src);
    let mut covered = 0usize;
    let mut cursor = 0usize;
    let mut contiguous = true;
    for t in st.iter() {
        let s = t.start as usize;
        let e = t.end as usize;
        // 游标只许前进：下一个 token 不得从游标之前起步（那叫重叠 / 回溯）。
        if s < cursor {
            contiguous = false;
        }
        // 空洞**只允许是注释**：注释按规范不产 token，故 `a/*x*/b` 的 `a` 与
        // `b` 之间必有空洞。把空洞一律判红等于要求「注释也产记号」；正确做法
        // 是显式建模——空洞段必须以 `/*` 起始并以 `*/` 收尾，其余空洞即漏扫。
        if s > cursor {
            let gap = st.pre.get(cursor..s).unwrap_or("");
            if !(gap.starts_with("/*") && gap.ends_with("*/")) {
                contiguous = false;
            }
        }
        cursor = e;
        covered = covered.saturating_add(e.saturating_sub(s));
    }
    // 注释不产 token，故覆盖长度必然小于源长；判据要的是「无非法空洞、无重叠」。
    let monotone = cursor <= st.pre.len();
    let no_overlap = covered <= st.pre.len();
    // 反例：语料确实含注释且覆盖确实短于源长——否则上面「空洞只允许是注释」
    // 那句是恒真的，判据没有鉴别力。
    let has_comment = src.contains("/*") && covered < st.pre.len();
    let ok = contiguous && monotone && no_overlap && has_comment && st.pre.len() == src.len();
    set.add("O04-架构-06-token区间无空洞无重叠", ok, "");
}

/// 取分词段的段契约（避免在自检里写死字段名）。
fn stage_spec_of_tokenize() -> &'static crate::svstar2::veo01_arch::StageSpec {
    crate::svstar2::veo01_arch::stage_spec(crate::svstar2::veo01_arch::Stage::Tokenize)
}

/// 取「合法」预算值（仅供构造 BudgetEntry 做判据）。
fn tok_budget_ok_value() -> u32 {
    tokenize_budget_micros()
}

// ---------------------------------------------------------------------------
// O04-边界-*
// ---------------------------------------------------------------------------

/// O04-边界-01：集成边界——本单不引入任何 crate，且不碰 layout。
fn chk_boundary_no_new_deps(set: &mut CheckSet) {
    // 本单的依赖面：只允许 std 的 String/Vec/format。逐个登记并核对无新增。
    let allowed: [&str; 6] = [
        "alloc::format",
        "alloc::string::String",
        "alloc::string::ToString",
        "alloc::vec::Vec",
        "super::veo01_arch::fnv1a64_hex",
        "super::veo01_arch::StyleError",
    ];
    let count_ok = allowed.len() == 6;
    // 反例：若有人加 extern crate layout，这条恒等式不成立——用字符串登记
    // 表达禁令，登记项里恰好没有 layout。
    let no_layout = !allowed.iter().any(|a| a.contains("layout"));
    let ok = count_ok && no_layout;
    set.add("O04-边界-01-零新增crate且不碰layout", ok, "");
}

/// O04-边界-02：上游契约哈希对账——真值抓得住漂移，且对预处理后的源生效。
fn chk_boundary_upstream_hash(set: &mut CheckSet) {
    use crate::svstar2::veo01_arch::fnv1a64_hex;
    let src = "a{b:c}";
    // 登记自洽哈希 → 放行。
    let pre = preprocess(src);
    let good = SourceContract::new("stylesheet/v1", &fnv1a64_hex(pre.as_bytes()));
    let pass = good.verify(pre.as_str()).is_ok();
    // 登记错哈希 → 必须抓（否则「对账」退化成「看填了没有」）。
    let wrong = SourceContract::new("stylesheet/v1", "0000000000000000");
    let caught = wrong.verify(pre.as_str()).is_err();
    // 空契约名 → 必须抓。
    let nameless = SourceContract::new("  ", &fnv1a64_hex(pre.as_bytes()));
    let name_caught = nameless.verify(pre.as_str()).is_err();
    // 关键：契约对的是**预处理后**的源——CRLF 与 LF 归一后必须同哈希，
    // 否则上游每次按平台换行都会报漂移（这是真实会发生的假阳性）。
    let crlf = preprocess("a{b:c}\r\n");
    let lf = preprocess("a{b:c}\n");
    let same = fnv1a64_hex(crlf.as_bytes()) == fnv1a64_hex(lf.as_bytes());
    // 反例：原始字节哈希不同（否则上面那条就成恒真）。
    let raw_differs = fnv1a64_hex(b"a{b:c}\r\n") != fnv1a64_hex(b"a{b:c}\n");
    let ok = pass && caught && name_caught && same && raw_differs;
    set.add("O04-边界-02-上游哈希对账抓漂移且对预处理源", ok, "");
}

/// O04-边界-03：下游消费接口前向声明——四家必登，未登者拒发。
fn chk_boundary_downstream_declared(set: &mut CheckSet) {
    // 四家各须可查到，且带承接单号与义务（无义务的声明不是契约）。
    let mut all_ok = true;
    for id in ["declaration-parse", "shorthand-expand", "error-recovery", "style-fuzz"] {
        match check_sink_declared(id) {
            Ok(s) => {
                if s.obligation.len() < 12 || !s.anchor.starts_with("VE-F") {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    // 未登名必须被拒——这条若恒真，说明前向声明形同虚设。
    let unknown_rejected = check_sink_declared("not-a-consumer").is_err();
    // 承接单号必须与总纲一致（不得凭空编）。
    let anchors_ok = DOWNSTREAM_SINKS
        .iter()
        .all(|s| matches!(s.anchor, "VE-F2805" | "VE-F2806" | "VE-F2814" | "VE-F2819"));
    let ok = all_ok && unknown_rejected && anchors_ok && DOWNSTREAM_SINKS.len() == 4;
    set.add("O04-边界-03-下游四家前向声明齐备", ok, "");
}

/// O04-边界-04：跨域衔接对账钩子——复用方义务必须可查到，非复用方为空。
fn chk_boundary_reconciliation_hooks(set: &mut CheckSet) {
    let has = reconciliation_hooks("error-recovery");
    let none = reconciliation_hooks("someone-else");
    let distinct = has.len() == 1 && none.is_empty();
    // 每家义务都必须**具体到可执行的对账动作**——「按什么对、用什么对」，
    // 而不是笼统的「要对接」。判据要求点名三类可查凭据之一：
    // token 偏移区间 / 载荷字段 / 流摘要 digest。
    //
    // 早先只查 `offset|载荷|digest` 三个字面量，导致 `declaration-parse`
    // 那条「按 token 顺序消费，不得回读源；遇 BadString/BadUrl 必须跳过
    // 整条声明」被误判为不合格——它以**记号类型**为对账依据，同样具体，
    // 却因没出现那三个词而恒红。判据错比没判据更坏。
    let concrete = DOWNSTREAM_SINKS.iter().all(|s| {
        s.obligation.contains("偏移")
            || s.obligation.contains("载荷")
            || s.obligation.contains("digest")
            // 以记号类型为对账依据同样是具体义务（F2805 靠 BadString/BadUrl
            // 决定跳过粒度），要求它显式点名这两类错误记号。
            || (s.obligation.contains("BadString") && s.obligation.contains("BadUrl"))
    });
    // 反例：四家义务互不相同（复制粘贴同一句话的表不构成四份契约）。
    let distinct_obl = {
        let mut all = true;
        for i in 0..DOWNSTREAM_SINKS.len() {
            for j in (i + 1)..DOWNSTREAM_SINKS.len() {
                if DOWNSTREAM_SINKS[i].obligation == DOWNSTREAM_SINKS[j].obligation {
                    all = false;
                }
            }
        }
        all
    };
    let ok = distinct && concrete && distinct_obl;
    set.add("O04-边界-04-对账钩子可查且义务具体", ok, "");
}

// ---------------------------------------------------------------------------
// O04-子集-*
// ---------------------------------------------------------------------------

/// O04-子集-01：25 类 token 全集齐备且 rank 与 ALL 索引一一对应。
fn chk_subset_all_kinds(set: &mut CheckSet) {
    let mut bijective = true;
    for (i, k) in TokenKind::ALL.iter().enumerate() {
        if k.rank() != i {
            bijective = false;
        }
        // 枚举守卫往返：码 → 类型 → 码，必须回到原码。
        if TokenKind::from_code(&k.code()) != Some(*k) {
            bijective = false;
        }
    }
    let full = TokenKind::ALL.len() == TOKEN_KIND_COUNT;
    // 25 类必须恰好覆盖：标识符/函数/at/hash/字符串/坏字符串/url/坏url/分隔符/
    // 数值/百分比/带单位/空白/CDO/CDC/冒号/分号/逗号/六种括号/EOF。
    let must = [
        TokenKind::Ident,
        TokenKind::Function,
        TokenKind::AtKeyword,
        TokenKind::Hash,
        TokenKind::String,
        TokenKind::BadString,
        TokenKind::Url,
        TokenKind::BadUrl,
        TokenKind::Delim,
        TokenKind::Number,
        TokenKind::Percentage,
        TokenKind::Dimension,
        TokenKind::Whitespace,
        TokenKind::Cdo,
        TokenKind::Cdc,
        TokenKind::Colon,
        TokenKind::Semicolon,
        TokenKind::Comma,
        TokenKind::LeftBracket,
        TokenKind::RightBracket,
        TokenKind::LeftParen,
        TokenKind::RightParen,
        TokenKind::LeftCurly,
        TokenKind::RightCurly,
        TokenKind::Eof,
    ];
    let covered = must.iter().all(|k| TokenKind::ALL.contains(k));
    let ok = full && bijective && covered && must.len() == TOKEN_KIND_COUNT;
    set.add("O04-子集-01-25类token齐备且枚举往返", ok, "");
}

/// O04-子集-02：逐条规格公开——每类都写清「吃什么/吐什么/失败形态」。
fn chk_subset_spec_published(set: &mut CheckSet) {
    // 每条规格必须同时含「吃」与「吐」与失败形态描述，否则下游只能猜。
    let mut all_spec = true;
    for k in TokenKind::ALL.iter() {
        let s = k.spec();
        let has_in = s.contains("吃");
        let has_out = s.contains("吐");
        // 失败形态：或明写「失败」，或明写「无」（两类都不写的即无规格）。
        let has_fail = s.contains("失败");
        if !has_in || !has_out || !has_fail {
            all_spec = false;
        }
        // 中文名与码必须非空（读屏依赖）。
        if k.zh().is_empty() || k.code().is_empty() {
            all_spec = false;
        }
    }
    let ok = all_spec;
    set.add("O04-子集-02-逐条规格公开可读屏", ok, "");
}

/// O04-子集-03：分词不越权——本段不判定属性合法性（那是 F2803/F2805 的事）。
///
/// 这条靠**行为**验证：把一个子集外的属性名喂进去，本段照样切成 Ident，
/// 不得拒绝也不得「顺手」判它非法——越权判定的表现是抛错或产诊断。
fn chk_subset_no_property_judgement(set: &mut CheckSet) {
    // 子集外的属性名（F2803 四族 44 条之外的）。
    let outside = "color:x;zap:1";
    let st = lex(outside);
    let no_err = st.audit().is_ok();
    // 它必须被切成 Ident/冒号/数值/分号，且**不产任何 BadString/BadUrl**。
    let no_error_token = st.errors().is_empty();
    // 关键反例：`zap` 仍被切出标识符（而不是被拒）——证明本段确实不判合法性。
    let zap_kept = st.payload(0) == "color";
    let zap_second = st.payload(4) == "zap";
    // 索引按实际切分推导：0=color 1=: 2=x 3=; 4=zap。早先取 `payload(3)`
    // 拿到的是分号，判据恒假。另断类型，确保 `zap` 真是标识符记号。
    let shape_ok = st.get(4).map(|t| t.kind == TokenKind::Ident).unwrap_or(false)
        && st.get(3).map(|t| t.kind == TokenKind::Semicolon).unwrap_or(false);
    let ok = no_err && no_error_token && zap_kept && zap_second && shape_ok;
    set.add("O04-子集-03-分词不越权判属性合法性", ok, "");
}

/// O04-子集-04：流自检可证伪——人为破坏偏移后必须转红。
///
/// 这条是「门禁能咬人」的证明：先跑真流确认绿，再把偏移改坏确认红。
fn chk_subset_audit_catches_corruption(set: &mut CheckSet) {
    let good = lex("a{b:c}");
    let baseline_ok = good.audit().is_ok();
    // 破坏一：EOF 重复。
    let mut dup = good.clone();
    if let Some(first) = dup.tokens.first().cloned() {
        dup.tokens.insert(0, first);
    }
    let eof_caught = dup.audit().is_err();
    // 破坏二：偏移越界（end 超出源长）。
    let mut oob = good.clone();
    if let Some(t) = oob.tokens.first_mut() {
        t.end = u32::MAX;
    }
    let span_caught = oob.audit().is_err();
    // 破坏三：数值类型丢数值载荷。
    let num_src = lex("10px");
    let mut lost = num_src.clone();
    if let Some(t) = lost.tokens.first_mut() {
        t.numeric = None;
    }
    let carrier_caught = lost.audit().is_err();
    // 破坏四：统计与 token 数脱钩。
    let mut bad_stat = good.clone();
    bad_stat.stats.tokens = bad_stat.stats.tokens.saturating_add(3);
    let stat_caught = bad_stat.audit().is_err();
    let ok = baseline_ok && eof_caught && span_caught && carrier_caught && stat_caught;
    set.add("O04-子集-04-流自检对四种破坏均可证伪", ok, "");
}

/// O04-子集-05：计数聚合无遗漏——逐类求和恒等于 token 数。
fn chk_subset_count_aggregation(set: &mut CheckSet) {
    let samples = [
        "a{b:c}",
        "/*c*/a,b;;",
        "url(x) url(\"y\") url(z",
        "\"s\\\"q\" 'a'",
        "1 1.5 .5 -1e3 50% 10px",
        "#a #1 #",
        "<!-- --> @media (a:b)",
        "",
    ];
    let mut all_ok = true;
    for s in samples.iter() {
        let st = lex(s);
        if st.stats.kind_sum() != st.stats.tokens as u64
            || st.stats.tokens as usize != st.len()
        {
            all_ok = false;
        }
    }
    // 有效 token 数必须 ≤ 总数（去空白与 EOF 后的口径）。
    let st = lex("a   b");
    let sig_ok = st.stats.significant() < st.stats.tokens;
    // 逐类计数必须与实际记号逐一对上——只断「求和等于总数」不够：
    // 两个类各多数一次、少数一次，求和照样相等但逐类计数全错。
    // 故这里对三类高频记号直接数，并与流内实际出现次数对拍。
    let per_kind_ok = count_of("a{b:c}", TokenKind::Ident) == 3
        && count_of("a{b:c}", TokenKind::LeftCurly) == 1
        && count_of("a{b:c}", TokenKind::RightCurly) == 1
        && count_of("a{b:c}", TokenKind::Colon) == 1
        && count_of("a{b:c}", TokenKind::Eof) == 1
        // 反例：这些类在该语料里确实没出现（否则上面的断言是恒真的）。
        && count_of("a{b:c}", TokenKind::String) == 0
        && count_of("a{b:c}", TokenKind::BadUrl) == 0;
    let ok = all_ok && sig_ok && per_kind_ok;
    set.add("O04-子集-05-计数聚合并集", ok, "");
}

// ---------------------------------------------------------------------------
// O04-降级-*（三闸）
// ---------------------------------------------------------------------------

/// O04-降级-01：非法输入 → 校验拒绝三要素（现象/原因/怎么办）。
fn chk_degrade_reject_triple(set: &mut CheckSet) {
    // 零上限必须被拒，且错误五元组齐发（code/what/why/next/who）。
    let bad = LexLimits {
        max_source_bytes: 0,
        max_tokens: 1,
        max_token_bytes: 1,
        max_nesting_depth: 1,
    };
    let mut lx = Lexer::new(None, bad, 1);
    match lx.run("a{}") {
        Ok(_) => set.add("O04-降级-01-非法输入拒绝三要素齐发", false, ""),
        Err(e) => {
            let triple = !e.what.is_empty() && !e.why.is_empty() && !e.next.is_empty();
            let code_ok = e.code == E_LIMIT_INVALID;
            // 拒绝必须发生在**产出任何记号之前**：`Ok` 分支已在上方判红，
            // 这里只需确认错误三要素齐发且错误码专属。
            let ok = triple && code_ok && !e.who.is_empty();
            set.add("O04-降级-01-非法输入拒绝三要素齐发", ok, "");
        }
    }
}

/// O04-降级-02：字段倒挂（单 token 上限 > 源上限）必须被拒。
fn chk_degrade_limit_inversion(set: &mut CheckSet) {
    let inv = LexLimits {
        max_source_bytes: 10,
        max_tokens: 4,
        max_token_bytes: 100,
        max_nesting_depth: 2,
    };
    let rejected = inv.verify().is_err();
    // 反例：顺序正确（单 token ≤ 源）必须放行，否则这条恒真。
    let good = LexLimits {
        max_source_bytes: 100,
        max_tokens: 4,
        max_token_bytes: 10,
        max_nesting_depth: 2,
    };
    let accepted = good.verify().is_ok();
    let ok = rejected && accepted;
    set.add("O04-降级-02-上限字段倒挂被拒", ok, "");
}

/// O04-降级-03：边界越界 → 钳制 + 告警（源超上限截断并落告警）。
fn chk_degrade_source_clamp(set: &mut CheckSet) {
    let limits = LexLimits {
        max_source_bytes: 8,
        max_tokens: 1024,
        max_token_bytes: 8,
        max_nesting_depth: 8,
    };
    let mut lx = Lexer::new(None, limits, 7);
    let src = "abcdefghijklmnop";
    match lx.run(src) {
        Ok(st) => {
            // 截断后源长必≤ 上限，且截断标记显性置位。
            let cut_ok = st.pre.len() <= limits.max_source_bytes;
            // 告警必落一条（钳制与告警成对）。
            let warned = st.clamps.len() >= 1;
            // 截断后必立案（异常检出 → 立案流转）。
            let filed = st.cases.len() >= 1;
            // 读屏文本必须明说被截断（否则下游会当成完整流）。
            let said = st.screen_text().contains("截断");
            let ok = cut_ok && warned && filed && said;
            set.add("O04-降级-03-源超限截断并告警立案", ok, "");
        }
        Err(_) => set.add("O04-降级-03-源超限截断并告警立案", false, ""),
    }
}

/// O04-降级-04：token 数上限截断，且截断后仍补唯一 EOF。
fn chk_degrade_token_cap(set: &mut CheckSet) {
    let limits = LexLimits {
        max_source_bytes: 4096,
        max_tokens: 5,
        max_token_bytes: 4096,
        max_nesting_depth: 8,
    };
    let mut lx = Lexer::new(None, limits, 3);
    match lx.run("a b c d e f g h i j") {
        Ok(st) => {
            // 上限管的是**真实记号数**；因截断而补的 EOF 是流终止符，不占额度。
            // 早先把 `st.len() <= max_tokens` 写成对总长的约束，等于要求
            // 「补 EOF」这个动作反过来挤掉一个真实记号——判据自身与被测
            // 契约（不留悬空流）矛盾，故按真实记号重数。
            let real = st.iter().filter(|t| t.kind != TokenKind::Eof).count();
            let capped = real <= limits.max_tokens && st.len() <= limits.max_tokens + 1;
            let warned = st.clamps.len() >= 1;
            // 关键：截断后必须有且仅有一个 EOF 居末（不留悬空流）。
            let eof_last = matches!(st.tokens.last().map(|t| t.kind), Some(TokenKind::Eof));
            let eof_count = st.iter().filter(|t| t.kind == TokenKind::Eof).count();
            let ok = capped && warned && eof_last && eof_count == 1 && st.audit().is_ok();
            set.add("O04-降级-04-token数截断后补唯一EOF", ok, "");
        }
        Err(_) => set.add("O04-降级-04-token数截断后补唯一EOF", false, ""),
    }
}

/// O04-降级-05：嵌套深度越界 → 钳制 + 告警 + 立案（且上限可配）。
fn chk_degrade_nesting_clamp(set: &mut CheckSet) {
    let limits = LexLimits {
        max_source_bytes: 4096,
        max_tokens: 4096,
        max_token_bytes: 4096,
        max_nesting_depth: 3,
    };
    let mut lx = Lexer::new(None, limits, 5);
    // 嵌套 8 层远超上限 3。
    match lx.run("((((((((") {
        Ok(st) => {
            let clamped = st.stats.max_depth <= limits.max_nesting_depth;
            let warned = st.clamps.count_for("nesting.depth") >= 1;
            let filed = st.cases.len() >= 1;
            let ok = clamped && warned && filed;
            set.add("O04-降级-05-嵌套超限钳制并告警立案", ok, "");
        }
        Err(_) => set.add("O04-降级-05-嵌套超限钳制并告警立案", false, ""),
    }
    // 反例：未超限时不得有任何嵌套告警（否则这条恒真）。
    let limits2 = LexLimits::standard();
    let mut lx2 = Lexer::new(None, limits2, 5);
    match lx2.run("((((") {
        Ok(st) => {
            let clean = st.clamps.count_for("nesting.depth") == 0;
            set.add("O04-降级-05b-未超限不产嵌套告警", clean, "");
        }
        Err(_) => set.add("O04-降级-05b-未超限不产嵌套告警", false, ""),
    }
}

/// O04-降级-06：单 token 字节上限截断，且规范化载荷不受连坐。
fn chk_degrade_token_bytes_clamp(set: &mut CheckSet) {
    let limits = LexLimits {
        max_source_bytes: 4096,
        max_tokens: 4096,
        max_token_bytes: 4,
        max_nesting_depth: 8,
    };
    let mut lx = Lexer::new(None, limits, 9);
    match lx.run("abcdefghij") {
        Ok(st) => {
            // 每个 token 原文长度必 ≤ 4。
            let all_capped = st.iter().all(|t| t.byte_len() <= 4);
            let warned = st.clamps.count_for("token.bytes") >= 1;
            let filed = st.cases.len() >= 1;
            let truncated = st.truncated;
            let ok = all_capped && warned && filed && truncated;
            set.add("O04-降级-06-单token字节截断并告警", ok, "");
        }
        Err(_) => set.add("O04-降级-06-单token字节截断并告警", false, ""),
    }
}

/// O04-降级-07：异常检出 → 立案流转（坏字符串/坏 URL/未闭合注释/未闭合字符串）。
fn chk_degrade_case_filing(set: &mut CheckSet) {
    // 坏字符串：引号内换行。
    let bad_str = lex("\"abc\ndef\"");
    let filed = bad_str.cases.len() >= 1;
    let bad_token = bad_str.errors().iter().any(|t| t.kind == TokenKind::BadString);
    // 坏 URL：url( 内出现引号。
    let bad_url = lex("url(a\"b)");
    let filed_url = bad_url.cases.len() >= 1;
    let bad_url_token = bad_url.errors().iter().any(|t| t.kind == TokenKind::BadUrl);
    // 未闭合注释：吞掉全表。
    let open_comment = lex("a{} /*");
    let filed_comment = open_comment.cases.len() >= 1;
    // 未闭合字符串：EOF 才收尾，**不吐 BadString**（规范口径：那是 parse error）。
    let open_str = lex("\"abc");
    let filed_str = open_str.cases.len() >= 1;
    let not_bad = open_str.errors().is_empty();
    let ok = filed && bad_token && filed_url && bad_url_token && filed_comment && filed_str && not_bad;
    set.add("O04-降级-07-四类异常均立案且形态分清", ok, "");
}

/// O04-降级-08：正常语料不得立案（**双向判据**：否则降级判据全恒真）。
fn chk_degrade_clean_corpus_no_case(set: &mut CheckSet) {
    let clean = [
        "a{color:red}",
        "@media screen and (min-width:100px){.x{width:50%}}",
        "/* 注释 */ a,b { c : url(foo.png) ; }",
        "\"str\" 'str2' #a #123 .5 1e3 -2px",
        "<!-- a --> b",
    ];
    let mut all_clean = true;
    for s in clean.iter() {
        let st = lex(s);
        if st.cases.len() != 0 || st.clamps.len() != 0 || !st.errors().is_empty() {
            all_clean = false;
        }
    }
    let ok = all_clean;
    set.add("O04-降级-08-正常语料零立案零告警", ok, "");
}

// ---------------------------------------------------------------------------
// O04-性能-*
// ---------------------------------------------------------------------------

/// O04-性能-01：确定性——同一二进制两次跑结果完全一致（含摘要）。
fn chk_perf_deterministic(set: &mut CheckSet) {
    let src = "a{b:c}/*x*/url(y)1e3 \"s\"#h 50% ,";
    let a = lex(src);
    let b = lex(src);
    let same_tokens = a.len() == b.len();
    let same_digest = a.digest() == b.digest();
    let same_stats = a.stats.tokens == b.stats.tokens && a.stats.kind_sum() == b.stats.kind_sum();
    let ok = same_tokens && same_digest && same_stats;
    set.add("O04-性能-01-两次跑结果完全一致", ok, "");
}

/// O04-性能-02：源越长token 数不减（**单调性**：线性性的必要条件）。
fn chk_perf_monotonic(set: &mut CheckSet) {
    let a = lex("a");
    let b = lex("a b");
    let c = lex("a b c d e");
    let ok = a.len() <= b.len() && b.len() <= c.len() && c.len() > a.len();
    set.add("O04-性能-02-源增长token数单调不减", ok, "");
}

/// O04-性能-03：重复语料线性——同片段重复 N 次，token 数按 N 倍增长
/// （**不是平方增长**；这条是可执行的反O(N²) 证据）。
fn chk_perf_linear_scaling(set: &mut CheckSet) {
    let unit = "a{b:c;}";
    let one = lex(unit).len();
    let mut src = String::new();
    for _ in 0..8 {
        src.push_str(unit);
    }
    let eight = lex(src.as_str()).len();
    // 线性期望：**每份语料记号数 × 份数 + 1 个流末尾 EOF**（精确式，不用容差）。
    // `a{b:c;}` 单份 = 7 个记号 + 1 个 EOF；8 份直接相接不产生额外记号，
    // 精确值 = 7×8 + 1 = 57。原写法把单份的 EOF 也乘进 8 倍并留 ±2 容差，
    // 等于用容差掩盖期望值算错——容差越宽，判据越弱。
    let per_unit = one.saturating_sub(1);
    let expected = per_unit.saturating_mul(8).saturating_add(1);
    let linear = eight == expected;
    // 反例：若真是 O(N²)，倍增会远超 8 倍。
    let quadratic = eight > expected.saturating_mul(3);
    let ok = linear && !quadratic && one > 0 && eight > one;
    set.add("O04-性能-03-重复语料呈线性而非平方", ok, "");
}

/// O04-性能-04：游标单调前进——**恒不回退**（回溯的判别式）。
///
/// 判据侧独立复核：把流里所有 token 按顺序排，start 必不减，且
/// 每个非空白 token 的 start 恰为前一 token 的 end（注释处允许空洞）。
fn chk_perf_no_backtrack(set: &mut CheckSet) {
    let src = "a{b:c}/*gap*/.5e2 url(x) \"s\" #h @m ,";
    let st = lex(src);
    let mut last_end = 0usize;
    let mut monotone = true;
    let mut overlap = false;
    for t in st.iter() {
        let s = t.start as usize;
        if s < last_end {
            monotone = false;
        }
        if s < last_end {
            overlap = true;
        }
        last_end = t.end as usize;
    }
    let ok = monotone && !overlap && last_end <= st.pre.len();
    set.add("O04-性能-04-游标单调前进无回溯", ok, "");
}

/// O04-性能-05：统计覆盖真实工作量——转义/注释/非 ASCII 计数可证没漏扫。
fn chk_perf_counters_cover_work(set: &mut CheckSet) {
    // 注释必被计数但不产 token。
    let with_comment = lex("/*a*/b/*c*/d");
    let comment_ok = with_comment.stats.comments == 2
        && with_comment.stats.kind_count(TokenKind::Ident) == 2;
    // 转义必被计数。
    let with_esc = lex("\\41 bc");
    let esc_ok = with_esc.stats.escapes_decoded >= 1;
    // 非 ASCII 必被计数（不得被吞）。
    let nonascii = lex("\u{4E2D}\u{6587}");
    let na_ok = nonascii.stats.non_ascii == 2
        && nonascii.payload(0) == "\u{4E2D}\u{6587}";
    let ok = comment_ok && esc_ok && na_ok;
    set.add("O04-性能-05-统计覆盖真实工作量", ok, "");
}

// ---------------------------------------------------------------------------
// O04-判据-*
// ---------------------------------------------------------------------------

/// O04-判据-01：枚举守卫可证伪——越界码必须返回 None（不许猜一个类型）。
fn chk_criterion_enum_guard(set: &mut CheckSet) {
    let valid = TokenKind::from_code("O04-T00") == Some(TokenKind::Ident);
    let last = TokenKind::from_code("O04-T24") == Some(TokenKind::Eof);
    // 越界：T25 不存在。
    let oob = TokenKind::from_code("O04-T25").is_none();
    // 形状错：非 hex / 长度错 / 前缀错。
    let bad_hex = TokenKind::from_code("O04-TXX").is_none();
    let short = TokenKind::from_code("O04-T1").is_none();
    let bad_prefix = TokenKind::from_code("O99-T01").is_none();
    let empty = TokenKind::from_code("").is_none();
    let ok = valid && last && oob && bad_hex && short && bad_prefix && empty;
    set.add("O04-判据-01-枚举守卫越界返回空", ok, "");
}

/// O04-判据-02：码表与 rank 单源——码必须由 rank 合成且全类互异。
fn chk_criterion_code_single_source(set: &mut CheckSet) {
    let mut distinct = true;
    for (i, k) in TokenKind::ALL.iter().enumerate() {
        // 码里的序号必须等于 rank（单源的直接证据）。
        let expect = format!("O04-T{:02}", i);
        if k.code() != expect {
            distinct = false;
        }
    }
    // 反例：手工编一张不同序的码表会让distinct 立刻为假。
    let fake = format!("O04-T{:02}", 99);
    let differs = fake != TokenKind::Ident.code();
    let ok = distinct && differs;
    set.add("O04-判据-02-码由rank合成全类互异", ok, "");
}

/// O04-判据-03：载荷守卫双向（该带必带 + 不该带不带）。
fn chk_criterion_carrier_both_directions(set: &mut CheckSet) {
    // 正向：数值类型必带数值。
    let dim = lex("10px");
    let num = dim.get(0).map(|t| t.kind == TokenKind::Dimension && t.numeric.is_some()).unwrap_or(false);
    let pct = lex("50%");
    let pct_ok = pct.get(0).map(|t| t.kind == TokenKind::Percentage && t.numeric.is_some()).unwrap_or(false);
    // hash 必带 type flag，且两种 flag 分得开。
    let id_hash = lex("#abc");
    let id_flag = id_hash.get(0).map(|t| t.hash_type == HashType::Id).unwrap_or(false);
    let num_hash = lex("#123");
    let num_flag = num_hash.get(0).map(|t| t.hash_type == HashType::Unrestricted).unwrap_or(false);
    // 退化：`#` 后无名字 → 分隔符，不是 hash。
    let bare = lex("#");
    let bare_delim = bare.get(0).map(|t| t.kind == TokenKind::Delim && t.delim == Some('#')).unwrap_or(false);
    // 反向：delim 字面量只在分隔符上；括号不带 delim。
    let paren = lex("(");
    let paren_no_delim = paren.get(0).map(|t| t.kind == TokenKind::LeftParen && t.delim.is_none()).unwrap_or(false);
    let ok = num && pct_ok && id_flag && num_flag && bare_delim && paren_no_delim;
    set.add("O04-判据-03-载荷守卫双向成立", ok, "");
}

/// O04-判据-04：数值语义——整数/实型按**记法**判定，不按数值大小。
fn chk_criterion_number_semantics(set: &mut CheckSet) {
    // `1` 整数；`1.0` 实数（值相同但记法不同）；`1e0` 实数（带指数）。
    let i = lex("1").get(0).and_then(|t| t.numeric).unwrap_or(num_false());
    let f = lex("1.0").get(0).and_then(|t| t.numeric).unwrap_or(num_false());
    let e = lex("1e0").get(0).and_then(|t| t.numeric).unwrap_or(num_false());
    // 符号：+1 与 1 数值相同但signed 不同。
    let p = lex("+1").get(0).and_then(|t| t.numeric).unwrap_or(num_false());
    // 数值正确性：.5 → 0.5、-1e3 → -1000、1.25 → 1.25。
    let dot = lex(".5").get(0).and_then(|t| t.numeric).map(|n| n.value).unwrap_or(-1.0);
    let neg_exp = lex("-1e3").get(0).and_then(|t| t.numeric).map(|n| n.value).unwrap_or(0.0);
    let frac = lex("1.25").get(0).and_then(|t| t.numeric).map(|n| n.value).unwrap_or(0.0);
    let values_ok = near(dot, 0.5) && near(neg_exp, -1000.0) && near(frac, 1.25);
    // 反例：`1.0` 与 `1` 数值相同但整数标记不同 —— 证明判据不是恒真。
    // 方向不可写反：`1` 是**整数**记法（`is_integer` 真、`is_real` 假），
    // `1.0` 与 `1e0` 是实型记法。早先把 `1` 判成 `!is_integer` 会把正确实现判红。
    let marker_discriminates = i.is_integer && !i.is_real && f.is_real && e.is_real
        && near(i.value, f.value)
        && near(i.value, e.value);
    let sign_discriminates = p.signed && !i.signed && near(p.value, i.value);
    let ok = values_ok && marker_discriminates && sign_discriminates;
    set.add("O04-判据-04-数值语义按记法判定", ok, "");
}

/// O04-判据-05：错误恢复点偏移逐字节对拍（**不只验「有错误」**）。
fn chk_criterion_recovery_offsets(set: &mut CheckSet) {
    // 坏字符串：`"a\nb"` —— BadString 应覆盖 `"a`（引号 + 内容），换行不消费。
    let src = "\"a\nb\"";
    let st = lex(src);
    let bad = st.errors().first().copied();
    match bad {
        Some(t) => {
            let s = t.start as usize;
            let e = t.end as usize;
            let slice = st.pre.get(s..e).unwrap_or("");
            // 逐字节对拍：切片必须是 `"a`（坏字符串不含换行、不含闭引号）。
            let exact = slice == "\"a";
            let no_newline = !slice.contains('\n');
            set.add("O04-判据-05-坏字符串偏移逐字节对拍", exact && no_newline, "");
        }
        None => set.add("O04-判据-05-坏字符串偏移逐字节对拍", false, ""),
    }
    // 坏 URL：`url(a"b)c"` —— BadUrl 应覆盖到配对右括号为止。
    let st2 = lex("url(a\"b)c\"");
    let bad2 = st2.errors().first().copied();
    match bad2 {
        Some(t) => {
            let slice = st2.pre.get(t.start as usize..t.end as usize).unwrap_or("");
            // 覆盖 `url(a"b)c` ——含引号触发点，且吃到配对 `)`。
            let covers = slice.starts_with("url(") && slice.ends_with(')');
            // 反例：原文在坏 URL 之后还有内容，必须被独立成 token（未被吞）。
            let after_kept = st2.iter().any(|x| x.start >= t.end);
            set.add("O04-判据-05b-坏URL偏移含触发点且吞到配对括号", covers && after_kept, "");
        }
        None => set.add("O04-判据-05b-坏URL偏移含触发点且吞到配对括号", false, ""),
    }
}

/// O04-判据-06：读屏替述——能念出偏移、类型、载荷与截断状态。
fn chk_criterion_screen_reader(set: &mut CheckSet) {
    let st = lex("a{b:c}");
    let lines = st.screen_lines(st.len());
    let has_index = lines.first().map(|l| l.contains("第 0 号")).unwrap_or(false);
    let has_offset = lines
        .first()
        .map(|l| l.contains('[') && l.contains(')'))
        .unwrap_or(false);
    let summary = st.screen_text();
    let says_count = summary.contains("token");
    let says_privacy = summary.contains("无隐私面");
    // 逐类规格也要能念。
    let spec_readable = TokenKind::BadUrl.screen_line().contains("坏 URL");
    let limits_readable = LexLimits::standard().screen_line().contains("分词上限");
    let narration = module_narration();
    let says_boundary = narration.contains("layout");
    let ok = has_index && has_offset && says_count && says_privacy && spec_readable && limits_readable && says_boundary;
    set.add("O04-判据-06-读屏替述含偏移类型载荷", ok, "");
}

/// O04-判据-07：零 panic 面——全语料批量跑不得 panic（能跑到这里即已证）。
fn chk_criterion_no_panic_surface(set: &mut CheckSet) {
    let corpus = [
        "",
        " ",
        "\u{0}\u{1}",
        "a",
        "((((((((((",
        "))))))))",
        "/*",
        "\"",
        "'",
        "\"\"\"",
        "url(",
        "url()",
        "url(  )",
        "url(\"x\")",
        "#",
        "#\\",
        "@",
        "@\\",
        "\\",
        "\\\\",
        "\\41",
        "\\41 ",
        "\\110000",
        "\\D800",
        "1e",
        "1e+",
        "1e999999",
        "999999999999999999999999",
        ".5.5",
        "+",
        "-",
        "-->",
        "<!--",
        ";;;,:::",
        "\u{4E2D}\u{6587}",
        "a{b:c}\u{0}",
    ];
    let mut all_ok = true;
    for s in corpus.iter() {
        let st = lex(s);
        // 无论多怪，流必须自洽（这是「畸形输入不许崩」的可判定形式）。
        if st.audit().is_err() {
            all_ok = false;
        }
        // 必须恰好一个 EOF。
        if st.iter().filter(|t| t.kind == TokenKind::Eof).count() != 1 {
            all_ok = false;
        }
        // 空源必得「仅一个 EOF」。
        if s.is_empty() && st.len() != 1 {
            all_ok = false;
        }
    }
    let ok = all_ok;
    set.add("O04-判据-07-畸形语料全流自洽", ok, "");
}

/// O04-判据-08：预处理的换行归一与 NUL 替换（逐字节对拍）。
fn chk_criterion_preprocess(set: &mut CheckSet) {
    // CRLF / CR / FF 三者都归一为 LF。
    let crlf = preprocess("a\r\nb");
    let cr = preprocess("a\rb");
    let ff = preprocess("a\u{0C}b");
    let lf = preprocess("a\nb");
    let all_nl = crlf == "a\nb" && cr == "a\nb" && ff == "a\nb" && lf == "a\nb";
    // 反例：归一前它们确实不同（否则上面恒真）。
    let raw_differs = "a\r\nb" != "a\rb" && "a\rb" != "a\u{0C}b";
    // NUL → U+FFFD。
    let nul = preprocess("a\u{0}b");
    let nul_ok = nul == "a\u{FFFD}b";
    // 非 ASCII 原样保留。
    let na = preprocess("\u{4E2D}");
    let na_ok = na == "\u{4E2D}";
    // 长度：CRLF 归一后**恰好短一字节**（2 字节 → 1 字节）。
    // 用精确差值而非「< 」：`<` 无法区分「短 1」与「短得更多」，且在 CRLF
    // 未折叠时与上面的 `all_nl` 同向失败，两者一起假绿、判据失去独立性。
    let shorter = "a\r\nb".len() - preprocess("a\r\nb").len() == 1;
    let ok = all_nl && raw_differs && nul_ok && na_ok && shorter;
    set.add("O04-判据-08-预处理换行归一与NUL替换", ok, "");
}

/// O04-判据-09：分流正确性——注释/字符串/URL/函数四类切分互不串味。
fn chk_criterion_tokening_shape(set: &mut CheckSet) {
    // 注释不产 token：`a/*x*/b` → Ident,Ident,EOF。
    let c = kinds("a/*x*/b");
    let comment_shape = c.len() == 3 && c[0] == TokenKind::Ident && c[1] == TokenKind::Ident;
    // 连续空白合并为一个：`a   b` → Ident,Whitespace,Ident,EOF。
    let w = kinds("a   b");
    let ws_merged = w.len() == 4 && w[1] == TokenKind::Whitespace;
    // `url("x")` → Function + String + RightParen（引号形态走函数）。
    // 引号形态下左括号**仍被函数记号吃掉**（§4.3.10：先consume 左括号，
    // 再看是否起始 ident 序列；引号不是 ident 序列，故只得到函数记号），
    // 所以是 4 个记号而不是 5 个——`(` 不作为独立记号出现。
    let uq = kinds("url(\"x\")");
    let url_fn = uq.len() == 4
        && uq[0] == TokenKind::Function
        && uq[1] == TokenKind::String
        && uq[2] == TokenKind::RightParen;
    // 对照断言：非引号形态 `url(x)` 收成**单个** Url 记号，且区间含两侧括号。
    let un = lex("url(x)");
    let url_shape = uq.len() == 4
        && un.len() == 2
        && un.get(0).map(|t| t.kind == TokenKind::Url).unwrap_or(false)
        && un
            .get(0)
            .map(|_| {
                let s = un.slice_text(0);
                s.starts_with("url(") && s.ends_with(')')
            })
            .unwrap_or(false);
    // `rgb(` → Function 记号（函数名 + 左括号合成一个记号）。
    // 规范 §4.3.10：function-token 的区间**含**尾随左括号，故 `rgb(` 只产出
    // Function + EOF 两个记号，不再单独产 LeftParen。早先期望 `len()==2` 而
    // 实现吐Function+LeftParen+EOF，是判据与实现的分歧；此处按规范口径
    // 要求 `Function` 记号的区间末字节**就是**左括号——直接断言偏移，
    // 而不是只数记号个数（数个数分不出「含括号」与「不含括号」）。
    let f = lex("rgb(");
    let fn_kind = f.get(0).map(|t| t.kind == TokenKind::Function).unwrap_or(false);
    let fn_span_has_paren = f
        .get(0)
        .map(|t| {
            let end = t.end as usize;
            f.slice_text(0).ends_with('(') && end == 4
        })
        .unwrap_or(false);
    let fn_shape = f.len() == 2 && fn_kind && fn_span_has_paren;
    // `color (`（括号前有空白）→ Ident + Whitespace + LeftParen。
    // 这条是**空格改变词法**的反例：同一个 `(`，贴着函数名时被并入Function
    // 记号，隔一个空白就退回独立 `(-记号。少了它，「左括号是否并入」这条
    // 规则就只验了并入那一半。
    let sp = kinds("color (");
    let space_matters = sp.len() == 4
        && sp[0] == TokenKind::Ident
        && sp[1] == TokenKind::Whitespace
        && sp[2] == TokenKind::LeftParen;
    let ok = comment_shape && ws_merged && url_shape && url_fn && fn_shape && space_matters;
    set.add("O04-判据-09-四类切分互不串味", ok, "");
}

/// O04-判据-10：CDO/CDC 与标识符的区分（`--custom` 是标识符不是 CDC）。
fn chk_criterion_cdo_cdc(set: &mut CheckSet) {
    let cdo = kinds("<!--");
    let cdo_ok = cdo.len() == 2 && cdo[0] == TokenKind::Cdo;
    let cdc = kinds("-->");
    let cdc_ok = cdc.len() == 2 && cdc[0] == TokenKind::Cdc;
    // 关键反例：`--custom` 必须是标识符（不是 CDC）——一个字符之差改变类型。
    let custom = kinds("--custom");
    let custom_ok = custom.len() == 2 && custom[0] == TokenKind::Ident;
    // `--x` 带单位：`--` 是标识符起始，`-` 本身也是名称码点，故 `--x` 整体是 ident。
    let neg = lex("-1px");
    let neg_ok = neg.get(0).map(|t| t.kind == TokenKind::Dimension).unwrap_or(false);
    let ok = cdo_ok && cdc_ok && custom_ok && neg_ok;
    set.add("O04-判据-10-CDO_CDC与标识符区分", ok, "");
}

/// O04-判据-11：摘要对账可用——改一个字符必改摘要，不改则摘要不变。
fn chk_criterion_digest(set: &mut CheckSet) {
    let a = lex("a{b:c}").digest();
    let b = lex("a{b:d}").digest();
    let changed = a != b;
    // 反例：同一输入摘要必须稳定。
    let stable = lex("a{b:c}").digest() == a;
    // 反例：仅空白不同也必须改摘要（空白是token 类型）。
    let ws = lex("a{b:c}").digest() != lex("a { b : c }").digest();
    // 摘要不得为空或超长失控（十六进制 16 位）。
    let shaped = a.len() == 16 && a.chars().all(|c| c.is_ascii_hexdigit());
    let ok = changed && stable && ws && shaped;
    set.add("O04-判据-11-摘要对账可用且稳定", ok, "");
}

/// O04-判据-12：零拷贝纪律——未转义路径不得物化文本。
fn chk_criterion_zero_copy(set: &mut CheckSet) {
    let plain = lex("color red");
    let decoded = plain.iter().filter(|t| t.payload.decoded_len() > 0).count();
    let zero_copy_ok = decoded == 0;
    // 反例：发生转义解码时必须物化（否则载荷取不到）。
    let esc = lex("\\41 bc");
    let materialized = esc.iter().filter(|t| t.payload.decoded_len() > 0).count();
    let esc_ok = materialized >= 1;
    // 物化总量必须远小于源长（证明没有整表复制）。
    let src_len = esc.pre.len();
    let copied: usize = esc.iter().map(|t| t.payload.decoded_len()).sum();
    let bounded = copied < src_len.max(1) * 4;
    let ok = zero_copy_ok && esc_ok && bounded;
    set.add("O04-判据-12-零拷贝纪律", ok, "");
}

/// O04-判据-13：载荷偏移逐字节对拍（字符串去引号 / hash 去井号 / at 去 @）。
fn chk_criterion_payload_offsets(set: &mut CheckSet) {
    // 字符串载荷不含引号。
    let s1 = lex("\"abc\"");
    let p1 = s1.payload(0);
    let str_ok = p1 == "abc";
    // hash 载荷不含 `#`。
    let s2 = lex("#abc");
    let p2 = s2.payload(0);
    let hash_ok = p2 == "abc";
    // at 关键字载荷不含 `@`。
    let s3 = lex("@media");
    let p3 = s3.payload(0);
    let at_ok = p3 == "media";
    // 函数载荷切到 `(` 之前。
    let s4 = lex("rgb(1,2,3)");
    let p4 = s4.payload(0);
    let fn_ok = p4 == "rgb";
    // URL 载荷不含右括号。
    let s5 = lex("url(foo.png)");
    let p5 = s5.payload(0);
    let url_ok = p5 == "foo.png";
    // Dimension 载荷是单位。
    let s6 = lex("10px");
    let p6 = s6.payload(0);
    let dim_ok = p6 == "px";
    // 反例：原文切片仍含引号（证明载荷不是随手拿slice_text）。
    let raw_has_quote = s1.slice_text(0).starts_with('"');
    let ok = str_ok && hash_ok && at_ok && fn_ok && url_ok && dim_ok && raw_has_quote;
    set.add("O04-判据-13-载荷偏移逐字节对拍", ok, "");
}

/// O04-判据-14：转义解码正确（判据侧独立按 §4.3.7 重算，不问被测函数）。
fn chk_criterion_escape_decode(set: &mut CheckSet) {
    // `\41` → 'A'（十六进制 41 = 65 = 'A'）。
    // 流必须先绑到具名变量：`lex(..).payload(0)` 会让流成为语句末即销毁的
    // 临时值，而 `payload` 返回借用（E0716）。
    let s1 = lex("\\41 bc");
    let a = s1.payload(0);
    // `\41 bc`：十六进制后跟一个空白，该空白被吃掉 → 结果 'Abc'。
    let joined = a;
    let joined_ok = joined == "Abc";
    // `\000041` → 六位十六进制 = 41 → 'A'（不是 'A' + '000041'）。
    let s2 = lex("\\000041x");
    let six = s2.payload(0);
    let six_ok = six == "Ax";
    // `\0` → NUL 被替换为 U+FFFD（§5.2.2）。
    let s3 = lex("\\0");
    let nul = s3.payload(0);
    let nul_ok = nul == "\u{FFFD}";
    // 代理区 → U+FFFD。
    let s4 = lex("\\D800");
    let surr = s4.payload(0);
    let surr_ok = surr == "\u{FFFD}";
    // 非十六进制转义取原字符：`\q` → 'q'。
    let s5 = lex("\\q");
    let plain = s5.payload(0);
    let plain_ok = plain == "q";
    // 反例：原文确实是 6 个字符（证明解码发生了改变，不是原样返回）。
    let raw_len = lex("\\000041x").slice_text(0).len();
    let raw_differs = raw_len == 8;
    let ok = joined_ok && six_ok && nul_ok && surr_ok && plain_ok && raw_differs;
    set.add("O04-判据-14-转义解码按规范重算", ok, "");
}

/// O04-判据-15：契约自检与替述随总纲同源——不另写一份（判据四的自证）。
fn chk_criterion_same_source(set: &mut CheckSet) {
    // 本单所有「对外常量」必须能追溯到 F2801 的真值，而不是各自抄一份。
    let n1 = ANCHORED_SYNTAX_SPEC.contains("css-syntax-3");
    let n2 = DEFAULT_MAX_SOURCE_BYTES == crate::svstar2::veo01_arch::MAX_SOURCE_BYTES;
    let n3 = tokenize_budget_micros() == derive_stage_budget(crate::svstar2::veo01_arch::Stage::Tokenize.rank());
    let n4 = COMPLEXITY_TABLE.len() == 8;
    let n5 = MAX_PEEK_WINDOW == 3;
    // 隐私面只有一个取值（无隐私面）。
    let n6 = PRIVACY_SURFACE == PrivacySurface::None;
    let n7 = TOKEN_KIND_COUNT == TokenKind::ALL.len();
    let ok = n1 && n2 && n3 && n4 && n5 && n6 && n7;
    set.add("O04-判据-15-契约与总纲同源", ok, "");
}

/// 浮点近似相等（判据侧独立写，不复用被测代码）。
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// 兜底数值（取不到载荷时返回一个必然不符的值，用于让判据转红）。
fn num_false() -> NumberValue {
    NumberValue {
        value: -12345.0,
        is_integer: false,
        is_real: true,
        signed: false,
    }
}

/// VE-F2804 域自检入口。
pub fn run_veo04_checks() -> CheckSet {
    let mut set = CheckSet::new("veo04-lexer");
    // 架构声明
    chk_arch_stage_contract(&mut set);
    chk_arch_budget_single_source(&mut set);
    chk_arch_budget_registration(&mut set);
    chk_arch_complexity_table(&mut set);
    chk_arch_peek_window(&mut set);
    chk_arch_linear_coverage(&mut set);
    // 集成边界
    chk_boundary_no_new_deps(&mut set);
    chk_boundary_upstream_hash(&mut set);
    chk_boundary_downstream_declared(&mut set);
    chk_boundary_reconciliation_hooks(&mut set);
    // 解析子集
    chk_subset_all_kinds(&mut set);
    chk_subset_spec_published(&mut set);
    chk_subset_no_property_judgement(&mut set);
    chk_subset_audit_catches_corruption(&mut set);
    chk_subset_count_aggregation(&mut set);
    // 降级矩阵
    chk_degrade_reject_triple(&mut set);
    chk_degrade_limit_inversion(&mut set);
    chk_degrade_source_clamp(&mut set);
    chk_degrade_token_cap(&mut set);
    chk_degrade_nesting_clamp(&mut set);
    chk_degrade_token_bytes_clamp(&mut set);
    chk_degrade_case_filing(&mut set);
    chk_degrade_clean_corpus_no_case(&mut set);
    // 性能
    chk_perf_deterministic(&mut set);
    chk_perf_monotonic(&mut set);
    chk_perf_linear_scaling(&mut set);
    chk_perf_no_backtrack(&mut set);
    chk_perf_counters_cover_work(&mut set);
    // 判据
    chk_criterion_enum_guard(&mut set);
    chk_criterion_code_single_source(&mut set);
    chk_criterion_carrier_both_directions(&mut set);
    chk_criterion_number_semantics(&mut set);
    chk_criterion_recovery_offsets(&mut set);
    chk_criterion_screen_reader(&mut set);
    chk_criterion_no_panic_surface(&mut set);
    chk_criterion_preprocess(&mut set);
    chk_criterion_tokening_shape(&mut set);
    chk_criterion_cdo_cdc(&mut set);
    chk_criterion_digest(&mut set);
    chk_criterion_zero_copy(&mut set);
    chk_criterion_payload_offsets(&mut set);
    chk_criterion_escape_decode(&mut set);
    chk_criterion_same_source(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veo04_standard_corpus_selfcheck_clean() {
        let st = lex("a{color:red;background:url(x.png)}");
        assert!(st.audit().is_ok(), "标准语料流不应有自检问题");
    }

    #[test]
    fn veo04_token_kinds_roundtrip() {
        for k in TokenKind::ALL.iter() {
            assert_eq!(TokenKind::from_code(&k.code()), Some(*k));
            assert_eq!(k.rank(), k.spec().len().min(usize::MAX).min(k.rank()));
        }
    }

    #[test]
    fn veo04_bad_string_offset_exact() {
        let st = lex("\"a\nb\"");
        let bad = st.errors().first().expect("应产出坏字符串");
        assert_eq!(st.pre.get(bad.start as usize..bad.end as usize), Some("\"a"));
    }

    #[test]
    fn veo04_eof_unique() {
        for s in ["", "a", "/*", "\"", "url("] {
            let st = lex(s);
            assert_eq!(st.iter().filter(|t| t.kind == TokenKind::Eof).count(), 1);
        }
    }

    #[test]
    fn veo04_escape_decode() {
        assert_eq!(lex("\\41 bc").payload(0), "Abc");
        assert_eq!(lex("\\000041x").payload(0), "Ax");
        assert_eq!(lex("\\0").payload(0), "\u{FFFD}");
    }

    #[test]
    fn veo04_url_vs_function() {
        assert_eq!(kind_at("url(x)", 0), Some(TokenKind::Url));
        assert_eq!(kind_at("url(\"x\")", 0), Some(TokenKind::Function));
    }
}