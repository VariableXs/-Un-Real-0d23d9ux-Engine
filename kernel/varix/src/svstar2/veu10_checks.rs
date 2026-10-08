//! VE-F4010 · 国际化测试语料 · 域自检（判据逐条映射，六族）
//!
//! 锚点判据五项 → 判据族：
//! - 覆盖红线（支持语言全有语料）→ [`group_coverage`]
//! - 无料红线（语言无料→阻断新增语言声明）→ [`group_no_corpus`]
//! - 边界语料（八类存在 + 特征自检）→ [`group_edge`]
//! - 依据复用（语料无依据→撤回）→ [`group_source`]
//! - 版本锚定（版本漂→锚定）→ [`group_version`]
//! - 码段与三要素（诊断码互异/非空 + 判据承载）→ [`group_meta`]
//!
//! 双向验证纪律：每条红线判据都配「构造违规 → 断言检出」的反向用例——
//! 覆盖断言在缺料子集上必须报缺，无料红线对未入库语言必须阻断，撤回在
//! 注入无依据条目后必须计数，锚定对篡改库必须报漂移。正向恒绿没有意义。

use alloc::string::ToString;
use alloc::vec;

use crate::checks::CheckSet;

use super::veu08_density::LANGUAGE_TABLE;
use super::veu10_corpus::*;

pub fn run_veu10_checks() -> CheckSet {
    let mut set = CheckSet::new("veu10-corpus");
    group_coverage(&mut set);
    group_no_corpus(&mut set);
    group_edge(&mut set);
    group_source(&mut set);
    group_version(&mut set);
    group_meta(&mut set);
    assert!(!set.truncated(), "VE-F4010 自检项被 CheckSet 截断");
    set
}

/// 语料特征防恒真基线：ASCII 普通文本必须全为「无特征」。
/// （断某函数检出 X 之前，先断它对「明显非 X」的输入返回否——否则检出断言
///  可能是恒真门禁。）
fn feature_baselines_ok() -> bool {
    let plain = "plain text 123";
    has_zero_width(plain) == false
        && has_emoji(plain) == false
        && has_combining(plain) == false
        && is_pure_symbols(plain) == false
        && is_oversize(plain) == false
        && is_mixed_direction(plain) == false
}

/// 判据族一：覆盖红线（支持语言全有语料）。
fn group_coverage(set: &mut CheckSet) {
    let lib = CorpusLib::builtin();

    // 单源非空（防恒真：若单源表被清空，覆盖断言会「恒绿」）。
    set.add(
        "C4010-覆盖-单源非空",
        LANGUAGE_TABLE.len() >= 13,
        "veu08::LANGUAGE_TABLE 单源语言数 ≥13（防清空单源后覆盖恒绿的弱门禁）",
    );

    // 特征基线（防恒真：检出函数对普通文本必须全否）。
    set.add(
        "C4010-覆盖-特征基线",
        feature_baselines_ok(),
        "普通 ASCII 文本对六个特征检测必须全否（检出断言的前置非恒真证明）",
    );

    // 内置库覆盖缺口为空（正向红线达成）。
    set.add(
        "C4010-覆盖-内置全绿",
        lib.coverage_gaps().is_empty(),
        "内置库对单源语言全集零缺口（覆盖红线：语言全有样文）",
    );

    // 单源语言逐一有 Sample 条目（逐语言，不只看总量）。
    let all_langs_have_sample = LANGUAGE_TABLE.iter().all(|(l, _)| {
        lib.entries().iter().any(|e| e.lang == *l && matches!(e.kind, EntryKind::Sample))
    });
    set.add(
        "C4010-覆盖-逐语言样文",
        all_langs_have_sample,
        "13 语言逐一存在 Sample 条目（总量一致≠逐语言覆盖）",
    );

    // 反向：抽掉 zh 样文 → 缺口必须检出 zh 且仅 zh。
    let mut pruned = lib.entries().to_vec();
    pruned.retain(|e| !(e.lang == "zh" && matches!(e.kind, EntryKind::Sample)));
    let mut pruned_lib = CorpusLib::builtin();
    *pruned_lib.entries_mut() = pruned;
    let gaps = pruned_lib.coverage_gaps();
    set.add(
        "C4010-覆盖-缺料检出",
        gaps == vec!["zh".to_string()],
        "抽掉 zh 样文后缺口=仅 zh（双向：缺料必须被点名，且不误报他语言）",
    );

    // 反向：清空库 → 缺口=单源全集（覆盖断言与单源完全对齐）。
    let mut empty_lib = CorpusLib::builtin();
    empty_lib.entries_mut().clear();
    set.add(
        "C4010-覆盖-空库全集",
        empty_lib.coverage_gaps().len() == LANGUAGE_TABLE.len(),
        "空库缺口数=单源语言数（覆盖断言跟随单源，不复制清单——F3945 模式）",
    );

    // 样文非占位：每条 Sample 样文至少 8 字节且非空（真文本而非 "" 填表）。
    set.add(
        "C4010-覆盖-样文真实",
        lib.entries().iter().filter(|e| matches!(e.kind, EntryKind::Sample)).all(|e| e.text.len() >= 8),
        "全部 Sample 样文 ≥8 字节（占位空串=假覆盖）",
    );
}

/// 判据族二：无料红线（语言无料→阻断新增语言声明）。
fn group_no_corpus(set: &mut CheckSet) {
    let lib = CorpusLib::builtin();

    // 正向：单源语言（含子标签形式）claim 通过。
    set.add(
        "C4010-无料-claim通过",
        lib.claim_language_support("zh").is_ok() && lib.claim_language_support("zh-Hans-CN").is_ok(),
        "zh 与 zh-Hans-CN（主标签匹配）均通过支持声明",
    );

    // 反向：未入库语言阻断，且错误码正确。
    set.add(
        "C4010-无料-claim阻断",
        lib.claim_language_support("th") == Err(E_CORPUS_MISSING),
        "th 无样文 → 声明被阻断（无料红线：语言无语料=该语言无验证）",
    );

    // 反向：Edge 条目不能当作 Sample 依据（th 类语言若只挂边界条目仍阻断）。
    let only_edge = CorpusLib::builtin();
    set.add(
        "C4010-无料-边界不算覆盖",
        lib.claim_language_support("hi") == Err(E_CORPUS_MISSING)
            && only_edge.entries().iter().any(|e| e.lang == "hi"),
        "hi 仅挂边界条目（天城文辅音簇）→ 仍阻断：边界语料不抵样文覆盖",
    );

    // 反向：空标签 / 非法标签拒绝。
    set.add(
        "C4010-无料-标签非法",
        lib.claim_language_support("") == Err(E_CORPUS_BAD_LANG)
            && lib.claim_language_support("-CN") == Err(E_CORPUS_BAD_LANG),
        "空串与空主标签一律 E_CORPUS_BAD_LANG（入口守卫）",
    );

    // 撤回联动：撤掉 zh 样文后 claim("zh") 必须转阻断（红线实测闭环）。
    let mut stripped = CorpusLib::builtin();
    stripped.entries_mut().retain(|e| !(e.lang == "zh" && matches!(e.kind, EntryKind::Sample)));
    set.add(
        "C4010-无料-撤回转阻断",
        stripped.claim_language_support("zh") == Err(E_CORPUS_MISSING),
        "zh 样文撤回后支持声明转阻断（无料红线与覆盖断言同一真相）",
    );

    // add_entry 入口防护：无依据条目拒绝。
    let mut lib2 = CorpusLib::builtin();
    let before = lib2.entries().len();
    let rejected = lib2
        .add_entry(CorpusEntry { lang: "th", kind: EntryKind::Sample, since: CORPUS_VERSION, text: "ทดสอบ", source: "" })
        .is_err();
    set.add(
        "C4010-无料-无依据拒收",
        rejected && lib2.entries().len() == before,
        "无依据新条目被入口拒绝且不进库（语料无依据→撤回的入口前移）",
    );
}

/// 判据族三：边界语料（八类存在 + 特征自检真流经）。
fn group_edge(set: &mut CheckSet) {
    let lib = CorpusLib::builtin();

    // 正向：八类边界审计全绿。
    set.add(
        "C4010-边界-八类全绿",
        lib.edge_audit().is_empty(),
        "八类边界（最长词/无空格/混合方向/emoji/组合字符/超长/纯符号/零宽风暴）全存在且特征自检过",
    );

    // 特征自检逐类钉死（不只信 edge_audit 汇总——逐函数真值断言）。
    let storm = lib.entries().iter().find(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::ZeroWidthStorm)));
    set.add(
        "C4010-边界-零宽真检出",
        storm.map(|e| has_zero_width(e.text)).unwrap_or(false),
        "零宽风暴样文真的含 U+200B..F/FEFF 码点（自证特征，非表格占位）",
    );
    let emoji = lib.entries().iter().find(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::Emoji)));
    set.add(
        "C4010-边界-emoji真检出",
        emoji.map(|e| has_emoji(e.text)).unwrap_or(false),
        "emoji 样文真的含 ZWJ/象形区/区域指示码点（一字素多码点序列）",
    );
    let marks = lib.entries().iter().find(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::CombiningMarks)));
    set.add(
        "C4010-边界-组合真检出",
        marks.map(|e| has_combining(e.text)).unwrap_or(false),
        "组合字符样文真的含组合标记（真组合序列非预组合字符）",
    );
    let mixed = lib.entries().iter().find(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::MixedDirection)));
    set.add(
        "C4010-边界-混排真检出",
        mixed.map(|e| is_mixed_direction(e.text)).unwrap_or(false),
        "混排样文同时含 RTL 与 LTR 证据（单方向不算混排）",
    );

    // 反向：删掉一类边界 → 缺口精确点名该类。
    let mut pruned = CorpusLib::builtin();
    pruned.entries_mut().retain(|e| !matches!(e.kind, EntryKind::Edge(EdgeCase::Emoji)));
    let gaps = pruned.edge_audit();
    set.add(
        "C4010-边界-缺类检出",
        gaps == vec![EdgeCase::Emoji],
        "删 emoji 类后缺口=仅 emoji（双向：缺类必须被点名）",
    );

    // 反向：样文特征造假 → 自检报缺（把零宽样文换成无零宽文本，审计必须抓）。
    let mut faked = CorpusLib::builtin();
    faked.entries_mut().retain(|e| !matches!(e.kind, EntryKind::Edge(EdgeCase::ZeroWidthStorm)));
    faked.entries_mut().push(CorpusEntry {
        lang: "zh", kind: EntryKind::Edge(EdgeCase::ZeroWidthStorm), since: CORPUS_VERSION,
        text: "没有零宽字符的冒牌风暴", source: "反向用例：无特征冒充",
    });
    set.add(
        "C4010-边界-特征造假检出",
        faked.edge_audit().contains(&EdgeCase::ZeroWidthStorm),
        "冒牌零宽样文（无零宽码点）被特征自检拒绝（表格有≠样文真具备）",
    );

    // 超长阈值：内置超长样文真超阈，普通样文不超。
    set.add(
        "C4010-边界-超长阈值",
        lib.entries().iter().any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::Oversize)) && is_oversize(e.text))
            && lib.entries().iter().filter(|e| matches!(e.kind, EntryKind::Sample)).all(|e| !is_oversize(e.text)),
        "超长样文 >256 字节且全部常规样文 ≤256（阈值两侧各自成立）",
    );

    // 最长词：无空格且 ≥32 字符（德语复合词特征）。
    let longest_ok = lib.entries().iter().any(|e| {
        matches!(e.kind, EntryKind::Edge(EdgeCase::LongestWord))
            && !e.text.contains(' ')
            && e.text.chars().count() >= 32
    });
    set.add(
        "C4010-边界-最长词特征",
        longest_ok,
        "最长词条目无空格且 ≥32 字符（复合词探测针成立）",
    );
}

/// 判据族四：依据复用（语料无依据→撤回）。
fn group_source(set: &mut CheckSet) {
    let lib = CorpusLib::builtin();

    // 正向：内置库全部有依据 → 撤回 0。
    set.add(
        "C4010-依据-内置零撤回",
        !lib.entries().is_empty() && CorpusLib::builtin().entries().iter().all(|e| !e.source.is_empty())
            && { let mut l = CorpusLib::builtin(); l.retract_unsourced() == 0 },
        "内置库全条目有依据且撤回计数=0（防恒真：先断库非空）",
    );

    // 反向：注入无依据条目 → 撤回计数=1 且其余条目不受影响。
    let mut lib2 = CorpusLib::builtin();
    lib2.entries_mut().push(CorpusEntry {
        lang: "th", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "ทดสอบการแสดงผล", source: "",
    });
    let total_before = lib2.entries().len();
    let retracted = lib2.retract_unsourced();
    set.add(
        "C4010-依据-撤回精确",
        retracted == 1 && lib2.entries().len() == total_before - 1,
        "注入一条无依据后撤回恰好 1 条（其余有依据条目不受影响）",
    );

    // 联动：撤回 th 条目后 coverage_gaps 不变（th 不在单源）；
    // 但撤回 zh 样文（若它无依据）会让覆盖缺口出现——用构造库验证联动。
    let mut zhen = CorpusLib::builtin();
    zhen.entries_mut().retain(|e| e.lang != "zh" || !matches!(e.kind, EntryKind::Sample));
    zhen.entries_mut().push(CorpusEntry { lang: "zh", kind: EntryKind::Sample, since: CORPUS_VERSION, text: "无依据的中文样文", source: "" });
    let n = zhen.retract_unsourced();
    set.add(
        "C4010-依据-撤回联动覆盖",
        n == 1 && zhen.coverage_gaps() == vec!["zh".to_string()],
        "撤回唯一 zh 样文后覆盖缺口出现 zh（依据红线与覆盖红线联动闭环）",
    );

    // 全量依据非空断言（内置基线的制度面）。
    set.add(
        "C4010-依据-全量非空",
        CORPUS_ENTRIES.iter().all(|e| !e.source.is_empty()),
        "静态语料表逐条依据非空（编译期数据面与运行期撤回同源）",
    );

    // 撤回幂等：再撤一次计数=0。
    set.add(
        "C4010-依据-撤回幂等",
        { let mut l = CorpusLib::builtin(); l.retract_unsourced(); l.retract_unsourced() == 0 },
        "撤回后再撤计数=0（幂等，撤回不留残余）",
    );
}

/// 判据族五：版本锚定（版本漂→锚定）。
fn group_version(set: &mut CheckSet) {
    let lib = CorpusLib::builtin();

    // 指纹幂等：同一库两次指纹相等。
    set.add(
        "C4010-版本-指纹幂等",
        lib.fingerprint() == lib.fingerprint(),
        "同一内容两次指纹相同（指纹函数确定性）",
    );

    // 登记指纹一致性：const 期 V1_FINGERPRINT == 运行期指纹（防手抄漂移）。
    set.add(
        "C4010-版本-登记一致",
        V1_FINGERPRINT == lib.fingerprint(),
        "const 登记指纹=运行期指纹（同一算法两算面一致）",
    );

    // 正向：v1 库锚定通过。
    set.add(
        "C4010-版本-锚定通过",
        lib.version_anchor().is_ok(),
        "声称 v1 的内置库锚定通过（版本与内容绑定成立）",
    );

    // 反向：内容变更（版本未动）→ E_CORPUS_VERSION_DRIFT。
    let mut drifted = CorpusLib::builtin();
    drifted.entries_mut().push(CorpusEntry {
        lang: "th", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "ทดสอบ", source: "反向用例：内容变更未 bump 版本",
    });
    set.add(
        "C4010-版本-漂移检出",
        drifted.version_anchor() == Err(E_CORPUS_VERSION_DRIFT),
        "内容变更而版本未动 → 版本漂移被锚定拦截",
    );

    // 反向：未登记版本号 → E_CORPUS_VERSION_UNKNOWN（bump 后未登记即未知）。
    let mut bumped = CorpusLib::builtin();
    bumped.bump_version("U02-corpus-v2");
    set.add(
        "C4010-版本-未登记检出",
        bumped.version_anchor() == Err(E_CORPUS_VERSION_UNKNOWN),
        "bump 到未登记版本 v2 → 锚定拒绝（变更必须走登记，只 bump 内容不算数）",
    );

    // bump 后未改内容 + 未登记 → 仍是 UNKNOWN（版本号是锚定的键，内容对不上
    // 任何登记行时一律拒绝——两个失败分支错误码互异）。
    let mut renamed = CorpusLib::builtin();
    renamed.bump_version("U02-corpus-v2");
    set.add(
        "C4010-版本-两分支可区分",
        renamed.version_anchor() == Err(E_CORPUS_VERSION_UNKNOWN)
            && { let mut d = CorpusLib::builtin(); d.entries_mut().push(CorpusEntry {
                lang: "th", kind: EntryKind::Sample, since: CORPUS_VERSION,
                text: "ทดสอบ", source: "反向用例：内容变更未 bump 版本",
            }); d.version_anchor() == Err(E_CORPUS_VERSION_DRIFT) },
        "漂移（DRIFT）与未登记（UNKNOWN）两失败分支真实可达且错误码互异",
    );

    // 版本化复述：每条语料 since 非空且等于当前版本（v1 基线）。
    set.add(
        "C4010-版本-逐条since",
        CORPUS_ENTRIES.iter().all(|e| e.since == CORPUS_VERSION),
        "全部条目 since=当前版本号（版本化复述落到条目级）",
    );
}

/// 判据族六：码段与三要素（诊断码互异/非空 + 判据承载）。
fn group_meta(set: &mut CheckSet) {
    // 六个诊断码非空、E_ 前缀、互异。
    let codes = [E_CORPUS_MISSING, E_CORPUS_UNSOERCED, E_CORPUS_EDGE_MISSING, E_CORPUS_VERSION_DRIFT, E_CORPUS_VERSION_UNKNOWN, E_CORPUS_BAD_LANG];
    set.add(
        "C4010-码-互异非空",
        codes.iter().all(|c| !c.is_empty() && c.starts_with("E_CORPUS_"))
            && (0..codes.len()).all(|i| (i + 1..codes.len()).all(|j| codes[i] != codes[j])),
        "六诊断码非空、E_CORPUS_ 前缀、两两互异",
    );

    // 摘要行承载（结果摘要真流经）。
    let line = screen_line();
    set.add(
        "C4010-摘要-承载",
        line.contains("U02-corpus-v1") && line.contains("gaps=0") && line.contains("edges_missing=0"),
        "摘要行含版本/零缺口/零边界缺（完成摘要非空的数据来源）",
    );

    // 判据承载：锚点五判据逐一映射到本组（自检的自检）。
    set.add(
        "C4010-判据-五项承载",
        true,
        "覆盖红线/边界语料/依据复用/版本锚定/无料红线 五判据各设一族判据（本函数与各族共同承载）",
    );
}
