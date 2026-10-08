//! VE-F3401 · 域自检（判据逐条对应，见 `ver01_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四件两律 → `E01-四件-*` / `E01-两律-*`
//! - 单源承诺 → `E01-单源-*`
//! - 有迹可循 → `E01-有迹-*`
//! - 越权拒绝 → `E01-覆盖-*`
//! - 降级矩阵（求值失败→降级默认 / 订阅泄漏→回收 / 覆盖越权→拒绝）→ `E01-降级-*`
//! - 跨批对接（K 域主题消费）→ `E01-对接-*`
//! - 无障碍（架构图读屏替代）→ `E01-读屏-*`
//! - 错误路径零静默 → `E01-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver01_arch::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 标准注册表（四个令牌，跨三个类别）：
/// - `color.accent.default` = `#4c8dff`，定义点 `tokens.css:10`
/// - `color.bg.canvas` = `#0b0d12`，定义点 `tokens.css:11`
/// - `color.text.primary` = `#e8ecf4`，定义点 `tokens.css:12`
/// - `space.gutter` = `16px`，定义点 `tokens.css:20`
fn std_registry() -> TokenRegistry {
    let mut r = TokenRegistry::new();
    let defs = [
        ("color.accent.default", "#4c8dff", "tokens.css:10", "强调色"),
        ("color.bg.canvas", "#0b0d12", "tokens.css:11", "画布底色"),
        ("color.text.primary", "#e8ecf4", "tokens.css:12", "主文字色"),
        ("space.gutter", "16px", "tokens.css:20", "栅格间距"),
    ];
    for (path, literal, site, doc) in defs.iter() {
        r.insert(TokenDef {
            path: path.to_string(),
            kind_id: 1,
            literal: literal.to_string(),
            canonical_site: site.to_string(),
            doc: doc.to_string(),
        })
        .expect("标准注册表入册");
    }
    r
}

/// 标准审计器。
fn std_auditor() -> SingleSourceAuditor {
    SingleSourceAuditor::new(std_registry())
}

/// 标准令牌源的四处定义点写法（全部应当豁免）。
fn canonical_sites() -> Vec<StyleSite> {
    vec![
        StyleSite {
            site: "tokens.css:10".to_string(),
            property: "--accent".to_string(),
            content: "#4c8dff".to_string(),
        },
        StyleSite {
            site: "tokens.css:11".to_string(),
            property: "--bg-canvas".to_string(),
            content: "#0b0d12".to_string(),
        },
        StyleSite {
            site: "tokens.css:12".to_string(),
            property: "--text-primary".to_string(),
            content: "#e8ecf4".to_string(),
        },
        StyleSite {
            site: "tokens.css:20".to_string(),
            property: "--gutter".to_string(),
            content: "16px".to_string(),
        },
    ]
}

/// VE-F3401 域自检。
pub fn run_ver01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ver01");

    // ---- 判据一：四件总纲 ----

    // 四件恰为解析/求值/订阅/覆盖，不多不少（总量铁律在总纲的投影）。
    {
        let names: Vec<&str> = PIECES.iter().map(|p| p.zh()).collect();
        set.add(
            "E01-四件-四件在册不多不少",
            PIECES.len() == 4 && names == vec!["解析", "求值", "订阅", "覆盖"],
            "",
        );
    }

    // 四件契约字段无空（挂名检测：只有名字没有边界的不算件）。
    {
        let mut all_filled = true;
        for p in PIECES.iter() {
            let s = piece_spec(*p);
            let fields = [
                s.duty_zh,
                s.duty_en,
                s.input,
                s.output,
                s.on_failure,
                s.complexity,
                s.consumers,
                s.not_mine,
            ];
            if fields.iter().any(|f| f.trim().is_empty()) {
                all_filled = false;
            }
        }
        set.add("E01-四件-契约字段无空", all_filled, "");
    }

    // 每件的降级策略与锚点降级矩阵对得上（三格各有归属，不是都写"报错"）。
    {
        let resolve = piece_spec(Piece::Resolve).on_failure;
        let evaluate = piece_spec(Piece::Evaluate).on_failure;
        let subscribe = piece_spec(Piece::Subscribe).on_failure;
        let over = piece_spec(Piece::Override).on_failure;
        set.add(
            "E01-四件-降级策略分件归属",
            resolve.contains("拒绝入册")
                && evaluate.contains("降级默认")
                && subscribe.contains("回收")
                && over.contains("拒绝"),
            "",
        );
    }

    // 每件都有"不做清单"——总纲不许当万能筐（越界即违约）。
    {
        let not_mine: Vec<&str> = PIECES.iter().map(|p| piece_spec(*p).not_mine).collect();
        set.add(
            "E01-四件-各有不做清单",
            not_mine.iter().all(|s| s.contains('不')),
            "",
        );
    }

    // 数据流序位：解析 0 → 覆盖 1 → 求值 2 → 订阅 3。
    // 覆盖必须在求值之前介入（否则就是"先算完再替换"，多一次无谓全链求值）。
    //
    // 断言按**序位集合**校验而非按枚举声明序：`PIECES` 的声明序是
    // 解析/求值/订阅/覆盖（便于人读），序位序是 0/2/3/1（数据流真实次序），
    // 两者本就不必一致。真正的判据是「序位恰为 0..3 无重复」且
    // 「解析 < 覆盖 < 求值 < 订阅」。
    {
        let stages: Vec<u8> = PIECES.iter().map(|p| p.stage()).collect();
        let mut sorted = stages.clone();
        sorted.sort_unstable();
        let contiguous = sorted == vec![0, 1, 2, 3];
        let ordered = piece_spec(Piece::Resolve).piece.stage() < piece_spec(Piece::Override).piece.stage()
            && piece_spec(Piece::Override).piece.stage() < piece_spec(Piece::Evaluate).piece.stage()
            && piece_spec(Piece::Evaluate).piece.stage() < piece_spec(Piece::Subscribe).piece.stage();
        set.add("E01-四件-数据流序位", contiguous && ordered, "");
    }

    // 四件都被至少一条律服务（无孤件），且服务的律都在册。
    {
        let served = PIECES
            .iter()
            .all(|p| !p.serves().is_empty() && p.serves().iter().all(|l| LAWS.contains(l)));
        set.add("E01-四件-无孤件", served, "");
    }

    // 四件的英文名唯一（标识符层不允许两件同名）。
    {
        let mut ens: Vec<&str> = PIECES.iter().map(|p| p.en()).collect();
        ens.sort();
        let mut dedup = ens.clone();
        dedup.dedup();
        set.add("E01-四件-英文名唯一", ens.len() == dedup.len(), "");
    }

    // ---- 判据二：两律声明 ----

    // 两律恰为令牌单源 + 变更有迹。
    {
        let names: Vec<&str> = LAWS.iter().map(|l| l.zh()).collect();
        set.add(
            "E01-两律-两律在册不多不少",
            LAWS.len() == 2 && names == vec!["令牌单源", "变更有迹"],
            "",
        );
    }

    // 两律四项声明（律文/违例/强制点/承诺）齐备—— unenforceable 的律等于没写。
    {
        let ok = LAWS.iter().all(|l| {
            let s = law_spec(*l);
            !s.statement.trim().is_empty()
                && !s.violation.trim().is_empty()
                && !s.enforcement.trim().is_empty()
                && !s.promise.trim().is_empty()
        });
        set.add("E01-两律-四项声明齐备", ok, "");
    }

    // 律编号可引用（E01-L1 / E01-L2）——律要能被下游点名引用。
    // 编号挂在 `Law` 上（`Law::code`），`LawSpec` 只是律的声明文本载体，
    // 故经 `.law` 取律本体再问编号——别把 `code` 喊在 spec 上。
    {
        set.add(
            "E01-两律-编号可引用",
            law_spec(Law::SingleSource).law.code() == "E01-L1"
                && law_spec(Law::Traceable).law.code() == "E01-L2",
            "",
        );
    }

    // 律文点名了单源的核心命题（"写死一个像素值就是破坏单源"要能在册里找到）。
    {
        let s = law_spec(Law::SingleSource).statement;
        set.add(
            "E01-两律-律文含单源命题",
            s.contains("令牌流出") && s.contains("字面量"),
            "",
        );
    }

    // 律二律文点名六要素（主体/动作/对象/前值/后值/理由）。
    {
        let s = law_spec(Law::Traceable).statement;
        let all = ["主体", "动作", "对象", "前值", "后值", "理由"]
            .iter()
            .all(|k| s.contains(k));
        set.add("E01-两律-律文含六要素", all, "");
    }

    // 契约自检：标准总纲零问题（总纲自己先可追溯）。
    {
        let a = Architecture::standard();
        let issues = a.check_contracts();
        set.add("E01-两律-标准总纲零契约问题", issues.is_empty(), "");
    }

    // 契约自检真能抓到问题：篡改件数/律数必须报 blocker，且每条都带建议。
    {
        let mut a = Architecture::standard();
        a.pieces.push(Piece::Resolve); // 重复登记
        a.laws.push(Law::Traceable); // 律数变三条
        let issues = a.check_contracts();
        let caught_dup = issues
            .iter()
            .any(|i| i.code == "CONTRACT_DUP" && i.severity == "blocker");
        let all_have_advice = issues.iter().all(|i| !i.advice.trim().is_empty());
        set.add(
            "E01-两律-契约断链可检出",
            caught_dup && all_have_advice && !issues.is_empty(),
            "",
        );
    }

    // 非法律（无执行者）能被检出：清空所有件后两律都成纸面律。
    {
        let mut a = Architecture::standard();
        a.pieces.clear();
        let issues = a.check_contracts();
        set.add(
            "E01-两律-纸面律可检出",
            issues.iter().any(|i| i.code == "CONTRACT_ORPHAN"),
            "",
        );
    }

    // 契约问题五要素齐备（现象/根因/建议/严重度齐发）。
    {
        let mut a = Architecture::standard();
        a.pieces.clear();
        let issues = a.check_contracts();
        let ok = !issues.is_empty()
            && issues.iter().all(|i| {
                !i.symptom.trim().is_empty()
                    && !i.cause.trim().is_empty()
                    && !i.advice.trim().is_empty()
                    && (i.severity == "blocker" || i.severity == "warn")
            });
        set.add("E01-两律-契约问题五要素齐", ok, "");
    }

    // ---- 判据三：单源承诺 ----

    // 定义点豁免：令牌源里的字面量写法不算破律（它就是单源本身）。
    {
        let a = std_auditor();
        let r = a.audit(&canonical_sites());
        set.add("E01-单源-定义点豁免", r.is_clean(), "");
    }

    // 抄值检出：同一字面量出现在第二处 → HardcodedLiteral，并指出该引哪个令牌。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Shell.tsx:142".to_string(),
            property: "backgroundColor".to_string(),
            content: "#4c8dff".to_string(),
        }]);
        let hit = r.breaches.iter().find_map(|b| match b {
            Breach::HardcodedLiteral {
                should_reference, ..
            } => Some(should_reference.clone()),
            _ => None,
        });
        set.add(
            "E01-单源-抄值检出",
            r.count_by_code("HARDCODE") == 1
                && hit.as_deref() == Some("color.accent.default")
                && !r.is_clean(),
            "",
        );
    }

    // 野生字面量检出：不属于任何令牌的值 → UnregisteredLiteral。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Panel.tsx:88".to_string(),
            property: "padding".to_string(),
            content: "13px".to_string(),
        }]);
        set.add(
            "E01-单源-野生字面量检出",
            r.count_by_code("UNREGISTERED") == 1,
            "",
        );
    }

    // 悬空引用检出：引用了不存在的令牌 → DanglingReference。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Bar.tsx:30".to_string(),
            property: "color".to_string(),
            content: "{color.accent.missing}".to_string(),
        }]);
        set.add(
            "E01-单源-悬空引用检出",
            r.count_by_code("DANGLING") == 1,
            "",
        );
    }

    // 腐烂令牌检出：在册无人引用 → OrphanToken（单源的另一半腐烂）。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "App.tsx:10".to_string(),
            property: "color".to_string(),
            content: "{color.text.primary}".to_string(),
        }]);
        // 只引用了一个令牌 → 其余三个令牌全是孤儿。
        set.add(
            "E01-单源-腐烂令牌检出",
            r.count_by_code("ORPHAN") == 3 && r.count_by_code("DANGLING") == 0,
            "",
        );
    }

    // 引用写法识别：{path} 是引用，裸值是字面量，`{}` 两者都不是。
    {
        let r = StyleSite {
            site: "x:1".to_string(),
            property: "color".to_string(),
            content: "{color.accent.default}".to_string(),
        };
        let lit = StyleSite {
            site: "x:2".to_string(),
            property: "color".to_string(),
            content: "#fff".to_string(),
        };
        let empty = StyleSite {
            site: "x:3".to_string(),
            property: "color".to_string(),
            content: "{}".to_string(),
        };
        set.add(
            "E01-单源-引用与字面量识别",
            r.is_reference()
                && r.referenced_path() == Some("color.accent.default")
                && lit.literal() == Some("#fff")
                && lit.referenced_path().is_none()
                && !empty.is_reference()
                && empty.literal() == Some("{}"),
            "",
        );
    }

    // 真值重复在册可检出（同一字面量被两个令牌占用——单源的反面）。
    {
        let mut r = TokenRegistry::new();
        for (path, site) in [("color.a", "t:1"), ("color.b", "t:2")] {
            r.insert(TokenDef {
                path: path.to_string(),
                kind_id: 1,
                literal: "#4c8dff".to_string(),
                canonical_site: site.to_string(),
                doc: "dup".to_string(),
            })
            .expect("入册");
        }
        set.add(
            "E01-单源-真值重复可检出",
            r.literal_is_ambiguous("#4c8dff")
                && r.find_by_literal("#4c8dff").is_some()
                && !r.literal_is_ambiguous("#000000"),
            "",
        );
    }

    // 破律自解释：每条破律的说明都要含位置与出路（无障碍 + 可操作）。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Shell.tsx:142".to_string(),
            property: "backgroundColor".to_string(),
            content: "#4c8dff".to_string(),
        }]);
        let ok = r.breaches.iter().all(|b| {
            let d = b.describe();
            !b.code().is_empty() && !d.contains("位置") || d.contains("Shell.tsx:142")
        }) && r
            .breaches
            .iter()
            .find(|b| b.code() == "HARDCODE")
            .map(|b| b.describe().contains("改为引用"))
            .unwrap_or(false);
        set.add("E01-单源-破律自解释", ok, "");
    }

    // 四类破律的说明各自给出路（野生值/悬空引用/腐烂令牌都要有"怎么办"）。
    {
        let a = std_auditor();
        let r = a.audit(&[
            StyleSite {
                site: "A.tsx:1".to_string(),
                property: "padding".to_string(),
                content: "13px".to_string(),
            },
            StyleSite {
                site: "B.tsx:2".to_string(),
                property: "color".to_string(),
                content: "{color.nope}".to_string(),
            },
        ]);
        let unreg = r
            .breaches
            .iter()
            .find(|b| b.code() == "UNREGISTERED")
            .map(|b| b.describe().contains("立为令牌"))
            .unwrap_or(false);
        let dang = r
            .breaches
            .iter()
            .find(|b| b.code() == "DANGLING")
            .map(|b| b.describe().contains("补上该令牌"))
            .unwrap_or(false);
        let orphan = r
            .breaches
            .iter()
            .find(|b| b.code() == "ORPHAN")
            .map(|b| b.describe().contains("接入界面或从注册表移除"))
            .unwrap_or(false);
        set.add("E01-单源-四类破律各有出路", unreg && dang && orphan, "");
    }

    // 审计报告读屏：不靠颜色，四类计数都在文字里。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Shell.tsx:142".to_string(),
            property: "backgroundColor".to_string(),
            content: "#4c8dff".to_string(),
        }]);
        let s = r.screen_text();
        set.add(
            "E01-单源-报告读屏含四类计数",
            s.contains("单源审计")
                && s.contains("抄值")
                && s.contains("野生")
                && s.contains("悬空")
                && s.contains("腐烂"),
            "",
        );
    }

    // 审计计数如实（站点数/令牌数进报告）。
    {
        let a = std_auditor();
        let r = a.audit(&canonical_sites());
        set.add(
            "E01-单源-审计计数如实",
            r.sites_audited == 4 && r.tokens_audited == 4 && r.count() == 0,
            "",
        );
    }

    // ---- 判据四：变更有迹 ----

    // 六元组完整落账（主体/动作/对象/前值/后值/理由）。
    {
        let mut l = ChangeLedger::new();
        let r = l
            .commit(ChangeRequest::new(
                "theme.dusk",
                ChangeAction::Override,
                "color.accent.default",
                "#4c8dff",
                "#7aa2ff",
                "夜间主题降饱和",
                120,
            ))
            .expect("提交");
        let e = l.entry(r.seq).expect("按序号取回");
        let ok = e.actor == "theme.dusk"
            && e.action == ChangeAction::Override
            && e.target == "color.accent.default"
            && e.before == "#4c8dff"
            && e.after == "#7aa2ff"
            && e.reason == "夜间主题降饱和"
            && e.tick == 120;
        set.add("E01-有迹-六元组完整落账", ok, "");
    }

    // 无痕变更不可表达：缺 actor / 缺 reason 直接拒绝。
    {
        let mut l = ChangeLedger::new();
        let no_actor = l.commit(ChangeRequest::new(
            "",
            ChangeAction::Override,
            "color.accent.default",
            "",
            "#fff",
            "夜间",
            1,
        ));
        let no_reason = l.commit(ChangeRequest::new(
            "theme.dusk",
            ChangeAction::Override,
            "color.accent.default",
            "",
            "#fff",
            "",
            2,
        ));
        let ok = matches!(no_actor, Err(ref e) if e.code == E_UNTRACEABLE)
            && matches!(no_reason, Err(ref e) if e.code == E_UNTRACEABLE)
            && l.is_empty()
            && l.rejected() == 2;
        set.add("E01-有迹-无痕变更被拒", ok, "");
    }

    // 纯空白的主体/理由同样算无痕（trim 后为空即拒）。
    {
        let mut l = ChangeLedger::new();
        let blank_actor = l.commit(ChangeRequest::new(
            "   ",
            ChangeAction::Define,
            "color.a",
            "",
            "#fff",
            "理由",
            1,
        ));
        let blank_reason = l.commit(ChangeRequest::new(
            "op",
            ChangeAction::Define,
            "color.a",
            "",
            "#fff",
            "\t\n ",
            1,
        ));
        set.add(
            "E01-有迹-空白主体理由算无痕",
            matches!(blank_actor, Err(ref e) if e.code == E_UNTRACEABLE)
                && matches!(blank_reason, Err(ref e) if e.code == E_UNTRACEABLE),
            "",
        );
    }

    // 无痕拒绝的错误必带下一步（拒绝要给出路，不是一句"不行"）。
    {
        let mut l = ChangeLedger::new();
        let e = l
            .commit(ChangeRequest::new(
                "",
                ChangeAction::Define,
                "color.a",
                "",
                "#fff",
                "理由",
                1,
            ))
            .expect_err("应被拒");
        set.add(
            "E01-有迹-无痕拒绝给出路",
            e.why.contains("actor") && e.next.contains("补上主体与理由"),
            "",
        );
    }

    // 账本只增不改 + seq 单调（时钟回拨不乱序）。
    {
        let mut l = ChangeLedger::new();
        // 故意让 tick 倒退：120 → 5 → 90。
        let ticks = [120u64, 5, 90];
        let mut last_seq = 0u64;
        let mut mono = true;
        for (i, t) in ticks.iter().enumerate() {
            let r = l
                .commit(ChangeRequest::new(
                    "theme.dusk",
                    ChangeAction::Override,
                    "color.accent.default",
                    "",
                    &format!("#v{}", i),
                    "改值",
                    *t,
                ))
                .expect("提交");
            if r.seq <= last_seq {
                mono = false;
            }
            last_seq = r.seq;
        }
        let seqs: Vec<u64> = l.iter().map(|e| e.seq).collect();
        set.add(
            "E01-有迹-seq单调不受时钟影响",
            mono && seqs == vec![1, 2, 3] && l.len() == 3,
            "",
        );
    }

    // seq 从 1 起，0 留作"无变更"哨兵。
    {
        let mut l = ChangeLedger::new();
        let r = l
            .commit(ChangeRequest::new(
                "op",
                ChangeAction::Define,
                "color.a",
                "",
                "#fff",
                "立册",
                0,
            ))
            .expect("提交");
        set.add("E01-有迹-seq起点为1", r.seq == 1 && l.entry(0).is_none(), "");
    }

    // 路径历史可反查（律二承诺：任取一次变化都能回答谁在什么理由下改的）。
    {
        let mut l = ChangeLedger::new();
        for (i, target) in ["color.a", "color.b", "color.a"].iter().enumerate() {
            l.commit(ChangeRequest::new(
                "op",
                ChangeAction::Override,
                target,
                "",
                &format!("#v{}", i),
                "调整",
                i as u64,
            ))
            .expect("提交");
        }
        let hist = l.history_of("color.a");
        set.add(
            "E01-有迹-按路径反查历史",
            hist.len() == 2 && hist[0].seq == 1 && hist[1].seq == 3,
            "",
        );
    }

    // 账本溢出拒绝 + 丢弃计数显性化（不静默丢）。
    {
        let mut l = ChangeLedger::new();
        for i in 0..LEDGER_CAP {
            l.commit(ChangeRequest::new(
                "op",
                ChangeAction::Override,
                "color.a",
                "",
                &format!("#v{}", i),
                "批量",
                i as u64,
            ))
            .expect("未满时应成功");
        }
        let over = l.commit(ChangeRequest::new(
            "op",
            ChangeAction::Override,
            "color.a",
            "",
            "#vX",
            "批量",
            LEDGER_CAP as u64,
        ));
        set.add(
            "E01-有迹-账本溢出拒绝且计数",
            matches!(over, Err(ref e) if e.code == E_LEDGER_FULL)
                && l.dropped() == 1
                && l.len() == LEDGER_CAP,
            "",
        );
    }

    // 溢出拒绝给出路（先归档轮转 / 按 ADR 提上限）。
    {
        let mut l = ChangeLedger::new();
        for i in 0..LEDGER_CAP {
            l.commit(ChangeRequest::new(
                "op",
                ChangeAction::Override,
                "color.a",
                "",
                &format!("#v{}", i),
                "批量",
                i as u64,
            ))
            .expect("提交");
        }
        let e = l
            .commit(ChangeRequest::new(
                "op",
                ChangeAction::Override,
                "color.a",
                "",
                "#vX",
                "批量",
                0,
            ))
            .expect_err("应被拒");
        set.add(
            "E01-有迹-溢出拒绝给出路",
            e.why.contains("可追溯性") && e.next.contains("归档") && e.next.contains("ADR"),
            "",
        );
    }

    // 回放供审计对拍（对拍红线）：逐条读屏文本，条数与账本一致。
    {
        let mut l = ChangeLedger::new();
        l.commit(ChangeRequest::new(
            "op",
            ChangeAction::Define,
            "color.a",
            "",
            "#fff",
            "立册",
            1,
        ))
        .expect("提交");
        let replay = l.replay();
        set.add(
            "E01-有迹-回放可对拍",
            replay.len() == l.len() && replay[0].contains("理由：立册"),
            "",
        );
    }

    // 变更读屏单行含六要素（前值为空时念"无"，不念空串）。
    {
        let e = ChangeEntry {
            seq: 3,
            tick: 42,
            actor: "theme.dusk".to_string(),
            action: ChangeAction::Override,
            target: "color.accent.default".to_string(),
            before: String::new(),
            after: "#7aa2ff".to_string(),
            reason: "夜间降饱和".to_string(),
        };
        let s = e.screen_line();
        set.add(
            "E01-有迹-变更读屏含六要素",
            s.contains("theme.dusk")
                && s.contains("color.accent.default")
                && s.contains("#7aa2ff")
                && s.contains("夜间降饱和")
                && s.contains("第 3 号")
                && s.contains("原 无"),
            "",
        );
    }

    // 三种动作各有可读名与动作码（审计对拍要按码聚合）。
    {
        let ok = ChangeAction::Define.zh() == "立册"
            && ChangeAction::Override.zh() == "覆盖"
            && ChangeAction::Deprecate.zh() == "废弃"
            && ChangeAction::Define.code() == "DEFINE"
            && ChangeAction::Override.code() == "OVERRIDE"
            && ChangeAction::Deprecate.code() == "DEPRECATE";
        set.add("E01-有迹-动作名与码齐备", ok, "");
    }

    // 空路径变更被拒（无对象的变更无法审计）。
    {
        let mut l = ChangeLedger::new();
        let r = l.commit(ChangeRequest::new(
            "op",
            ChangeAction::Override,
            "",
            "",
            "#fff",
            "理由",
            1,
        ));
        set.add(
            "E01-有迹-空对象变更被拒",
            matches!(r, Err(ref e) if e.code == E_EMPTY_PATH),
            "",
        );
    }

    // 回执带账本长度（调用方能确认落账位置）。
    {
        let mut l = ChangeLedger::new();
        let a = l
            .commit(ChangeRequest::new("op", ChangeAction::Define, "color.a", "", "#f", "r", 1))
            .expect("提交");
        let b = l
            .commit(ChangeRequest::new("op", ChangeAction::Define, "color.b", "", "#f", "r", 2))
            .expect("提交");
        set.add(
            "E01-有迹-回执带账本长度",
            a.ledger_len == 1 && b.ledger_len == 2 && b.seq == 2,
            "",
        );
    }

    // ---- 判据五：越权拒绝 ----

    // 默认拒绝：空授权表下任何覆盖都被拒。
    {
        let auth = OverrideAuthority::new();
        let r = auth.authorize("theme.dusk", OverrideLayer::Theme);
        set.add(
            "E01-覆盖-默认拒绝",
            matches!(r, Err(ref e) if e.code == E_OVERRIDE_UNAUTHORIZED) && auth.is_empty(),
            "",
        );
    }

    // 越权拒绝的错误带出路（申请哪一层 + 现持授权）。
    {
        let mut auth = OverrideAuthority::new();
        auth.grant("theme.dusk", OverrideLayer::Theme);
        let e = auth
            .authorize("ext.evil", OverrideLayer::Component)
            .expect_err("未授权应被拒");
        let ok = e.why.contains("ext.evil")
            && e.why.contains("组件")
            && e.why.contains("theme.dusk@主题")
            && e.next.contains("申请 组件 层授权");
        set.add("E01-覆盖-越权拒绝给出路", ok, "");
    }

    // 持权则放行；且只在该层放行（授权不外溢）。
    {
        let mut auth = OverrideAuthority::new();
        auth.grant("theme.dusk", OverrideLayer::Theme);
        set.add(
            "E01-覆盖-授权不跨层不外溢",
            auth.authorize("theme.dusk", OverrideLayer::Theme).is_ok()
                && auth.authorize("theme.dusk", OverrideLayer::Scene).is_err()
                && auth.authorize("other", OverrideLayer::Theme).is_err(),
            "",
        );
    }

    // 默认层不可被覆盖（覆盖它等于抹掉律一的唯一出处）——即使全量持权。
    {
        let mut auth = OverrideAuthority::new();
        for l in OverrideLayer::ALL.iter() {
            auth.grant("root", *l);
        }
        let r = auth.authorize("root", OverrideLayer::Default);
        set.add(
            "E01-覆盖-默认层不可覆盖",
            matches!(r, Err(ref e) if e.code == E_OVERRIDE_DEFAULT_LAYER)
                && OverrideLayer::Default.is_registry_layer()
                && auth.authorize("root", OverrideLayer::Component).is_ok(),
            "",
        );
    }

    // 四层齐全且层级序位严格递增（仲裁的前提，归 F3406 细化）。
    {
        let ok = OverrideLayer::ALL.len() == 4
            && OverrideLayer::Default < OverrideLayer::Theme
            && OverrideLayer::Theme < OverrideLayer::Scene
            && OverrideLayer::Scene < OverrideLayer::Component;
        set.add("E01-覆盖-四层序位递增", ok, "");
    }

    // 重复授权幂等合并；撤销后立即失权；重复撤销如实返回 false。
    {
        let mut auth = OverrideAuthority::new();
        auth.grant("a", OverrideLayer::Theme);
        auth.grant("a", OverrideLayer::Theme);
        let dedup = auth.len() == 1;
        let revoked = auth.revoke("a", OverrideLayer::Theme);
        set.add(
            "E01-覆盖-授权幂等与撤销",
            dedup
                && revoked
                && auth.authorize("a", OverrideLayer::Theme).is_err()
                && !auth.revoke("a", OverrideLayer::Theme),
            "",
        );
    }

    // 授权表读屏：无授权时说"无"，不装作有。
    {
        let mut auth = OverrideAuthority::new();
        let empty = auth.screen_text();
        auth.grant("b", OverrideLayer::Scene);
        let one = auth.screen_text();
        set.add(
            "E01-覆盖-授权读屏如实",
            empty.contains("现持授权 无") && one.contains("b@场景"),
            "",
        );
    }

    // 越权拒绝落进求值侧：未持权就不该产生覆盖条目（授权与记账分离的验证）。
    {
        let auth = OverrideAuthority::new();
        let mut res = TokenResolver::new(std_registry());
        let denied = auth.authorize("theme.dusk", OverrideLayer::Theme).is_err();
        // 记账端独立：只有拿到 seq 才允许落覆盖。
        let mut l = ChangeLedger::new();
        let receipt = l
            .commit(ChangeRequest::new(
                "theme.dusk",
                ChangeAction::Override,
                "color.accent.default",
                "#4c8dff",
                "#7aa2ff",
                "夜间",
                1,
            ))
            .expect("提交");
        res.apply_override(Override {
            path: "color.accent.default".to_string(),
            layer: OverrideLayer::Theme,
            value: "#7aa2ff".to_string(),
            seq: receipt.seq,
        });
        set.add(
            "E01-覆盖-拒绝与记账分离",
            denied
                && res.override_count() == 1
                && res.resolve("color.accent.default").provenance()
                    == Some(Provenance {
                        layer: OverrideLayer::Theme,
                        seq: 1,
                    }),
            "",
        );
    }

    // ---- 降级矩阵：求值失败→降级默认 ----

    // 三级判定：覆盖层 > 注册表 > 降级默认。
    {
        let mut res = TokenResolver::new(std_registry());
        // 第 2 级：注册表命中。
        let v = res.resolve("color.accent.default");
        let from_registry = matches!(&v, Resolution::Value { value, provenance }
            if value == "#4c8dff" && provenance.layer == OverrideLayer::Default);
        // 第 1 级：覆盖命中（同路径多层取层级最高者）。
        res.apply_override(Override {
            path: "color.accent.default".to_string(),
            layer: OverrideLayer::Theme,
            value: "#7aa2ff".to_string(),
            seq: 10,
        });
        res.apply_override(Override {
            path: "color.accent.default".to_string(),
            layer: OverrideLayer::Component,
            value: "#a0c0ff".to_string(),
            seq: 11,
        });
        let v2 = res.resolve("color.accent.default");
        let layered = matches!(&v2, Resolution::Value { value, provenance }
            if value == "#a0c0ff"
                && provenance.layer == OverrideLayer::Component
                && provenance.seq == 11);
        set.add("E01-降级-三级判定顺序", from_registry && layered, "");
    }

    // 求值失败 → 降级默认，且结果标 Fallback（兜底可辨，K 域约定 K2）。
    {
        let res = TokenResolver::new(std_registry());
        let r = res.resolve("color.brand.missing");
        let expected = fallback_for("color.brand.missing");
        set.add(
            "E01-降级-求值失败降级默认",
            r.is_fallback()
                && r.provenance().is_none()
                && r.value() == expected
                && expected == UNSPECIFIED_FALLBACK,
            "",
        );
    }

    // 降级默认按最长前缀匹配：color.text.* → 文字色；space.* → 间距。
    {
        set.add(
            "E01-降级-最长前缀匹配",
            fallback_for("color.text.secondary") == "#e8ecf4"
                && fallback_for("space.gutter") == "16px"
                && fallback_for("radius.card") == "10px"
                && fallback_for("motion.fast") == "180ms"
                && fallback_for("color.accent.hover") == "#4c8dff"
                && fallback_for("color.bg.canvas") == "#0b0d12"
                && fallback_for("unknown.thing") == UNSPECIFIED_FALLBACK,
            "",
        );
    }

    // 点分前缀判定：`color.bgx` 不算 `color.bg` 的前缀（防误配）。
    {
        set.add(
            "E01-降级-点分前缀严格",
            is_dot_prefix("color.bg.canvas", "color.bg")
                && is_dot_prefix("color.bg", "color.bg")
                && !is_dot_prefix("color.bgx.canvas", "color.bg"),
            "",
        );
    }

    // 兜底值的理由必须说清"为什么降级"，不是空字符串。
    {
        let res = TokenResolver::new(std_registry());
        let r = res.resolve("nope.path");
        let ok = match &r {
            Resolution::Fallback { reason, .. } => {
                reason.contains("nope.path") && reason.contains("降级矩阵")
            }
            _ => false,
        };
        set.add("E01-降级-兜底理由自解释", ok, "");
    }

    // 覆盖撤销后回到注册表真值（撤销不是把值删成空）。
    {
        let mut res = TokenResolver::new(std_registry());
        res.apply_override(Override {
            path: "space.gutter".to_string(),
            layer: OverrideLayer::Scene,
            value: "24px".to_string(),
            seq: 5,
        });
        let covered = res.resolve("space.gutter").value() == "24px";
        let dropped = res.drop_override("space.gutter", OverrideLayer::Scene);
        let back = res.resolve("space.gutter");
        set.add(
            "E01-降级-覆盖撤销回落真值",
            covered
                && dropped
                && back.value() == "16px"
                && back.provenance() == Some(Provenance::REGISTRY)
                && !res.drop_override("space.gutter", OverrideLayer::Scene),
            "",
        );
    }

    // 同路径同层重复覆盖：后者取代前者（覆盖表规模有界，不无限膨胀）。
    {
        let mut res = TokenResolver::new(std_registry());
        for seq in 1..=5u64 {
            res.apply_override(Override {
                path: "space.gutter".to_string(),
                layer: OverrideLayer::Theme,
                value: format!("{}px", 10 + seq),
                seq,
            });
        }
        set.add(
            "E01-降级-同层覆盖唯一",
            res.override_count() == 1 && res.resolve("space.gutter").value() == "15px",
            "",
        );
    }

    // 空覆盖表下求值等价于纯注册表（覆盖件未介入时不得改变任何结果）。
    {
        let res = TokenResolver::new(std_registry());
        let zero = res.override_count() == 0
            && res.resolve("color.accent.default").value() == "#4c8dff"
            && res.resolve("color.text.primary").value() == "#e8ecf4";
        set.add("E01-降级-零覆盖等价直通", zero, "");
    }

    // ---- 降级矩阵：订阅泄漏→回收 ----

    // 两条判据：主体已注销 / 静默超期。
    {
        let mut t = SubscriptionTable::new();
        t.subscribe("comp.a", &["color.accent.default"], 0)
            .expect("订阅");
        t.subscribe("comp.b", &["color.bg.canvas"], 0)
            .expect("订阅");
        // comp.a 仍在场但静默超期；comp.b 已注销。
        let records = t.sweep(&["comp.a"], 10_000);
        let silent = records
            .iter()
            .find(|r| r.owner == "comp.a")
            .map(|r| r.cause == "静默超期" && r.silent_ticks == 10_000);
        let revoked = records
            .iter()
            .find(|r| r.owner == "comp.b")
            .map(|r| r.cause == "主体已注销");
        set.add(
            "E01-降级-泄漏两判据",
            records.len() == 2 && silent == Some(true) && revoked == Some(true),
            "",
        );
    }

    // 回收后表内干净，累计计数如实。
    //
    // `sweep` 的两条判据是「主体注销**或**静默超期」——`live_owners` 只挡得住
    // 第一条，挡不住第二条。此处让comp.a 补一次心跳（tick=1000）再扫，
    // 才能真正区分「在册且活跃 → 留下」与「在册但失联 → 照收」。
    {
        let mut t = SubscriptionTable::new();
        let id_a = t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        t.subscribe("comp.b", &["color.b"], 0).expect("订阅");
        t.subscribe("comp.c", &["color.c"], 0).expect("订阅");
        assert!(t.touch(id_a, 1_000), "心跳应命中");
        let rec = t.sweep(&["comp.a"], 1_000);
        set.add(
            "E01-降级-回收留痕计数",
            rec.len() == 2 && t.len() == 1 && t.reclaimed_total() == 2,
            "",
        );
    }

    // 心跳续命：活着的订阅不该被误回收。
    {
        let mut t = SubscriptionTable::new();
        let id = t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        for tick in [1u64, 500, 900] {
            assert!(t.touch(id, tick), "心跳应命中");
        }
        let rec = t.sweep(&["comp.a"], 1_000);
        set.add(
            "E01-降级-心跳续命不误回收",
            rec.is_empty() && t.len() == 1 && !t.touch(999, 1),
            "",
        );
    }

    // 主动退订不算泄漏（不计入回收统计）。
    {
        let mut t = SubscriptionTable::new();
        let id = t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        let gone = t.unsubscribe(id);
        let rec = t.sweep(&["comp.a"], 1);
        set.add(
            "E01-降级-主动退订不计泄漏",
            gone && rec.is_empty() && t.reclaimed_total() == 0 && !t.unsubscribe(id),
            "",
        );
    }

    // 无主订阅与空关注路径被拒（无主订阅无法定位泄漏责任方）。
    {
        let mut t = SubscriptionTable::new();
        let no_owner = t.subscribe("", &["color.a"], 0);
        let no_paths = t.subscribe("comp.a", &[], 0);
        set.add(
            "E01-降级-非法订阅被拒",
            matches!(no_owner, Err(ref e) if e.code == E_NO_OWNER)
                && matches!(no_paths, Err(ref e) if e.code == E_EMPTY_PATH)
                && t.is_empty(),
            "",
        );
    }

    // 超期阈值边界：恰好等于阈值不回收（严格大于才回收）。
    {
        let mut t = SubscriptionTable::new();
        t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        let at_limit = t.sweep(&["comp.a"], SUB_TTL_TICKS);
        let over_limit = t.sweep(&["comp.a"], SUB_TTL_TICKS + 1);
        set.add(
            "E01-降级-超期边界严格",
            at_limit.is_empty() && over_limit.len() == 1,
            "",
        );
    }

    // 订阅号唯一且单调（回收后不复用号，避免陈旧句柄误命中）。
    {
        let mut t = SubscriptionTable::new();
        let a = t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        t.sweep(&[], 1);
        let b = t.subscribe("comp.b", &["color.b"], 2).expect("订阅");
        set.add("E01-降级-订阅号不复用", b > a && t.len() == 1, "");
    }

    // 回收记录读屏（含原因与静默时长）。
    {
        let r = ReclaimRecord {
            sub_id: 3,
            owner: "comp.x".to_string(),
            cause: "静默超期",
            silent_ticks: 901,
        };
        let s = r.screen_line();
        set.add(
            "E01-读屏-回收记录可念",
            s.contains("#3") && s.contains("comp.x") && s.contains("静默超期") && s.contains("901"),
            "",
        );
    }

    // ---- 判据六：跨批对接（K 域主题消费）----

    // K 域三条硬约定在册且可核对（取值带出处 / 兜底可辨 / 变更走订阅）。
    {
        let c = K_DOMAIN_CONTRACT;
        set.add(
            "E01-对接-K域三约定在册",
            c.contains("K1") && c.contains("K2") && c.contains("K3")
                && c.contains("provenance")
                && c.contains("Resolution::Fallback")
                && c.contains("Piece::Subscribe"),
            "",
        );
    }

    // K1 的可执行验证：真值与兜底都能被 K 域分辨（provenance 有无）。
    {
        let mut res = TokenResolver::new(std_registry());
        res.apply_override(Override {
            path: "color.accent.default".to_string(),
            layer: OverrideLayer::Theme,
            value: "#7aa2ff".to_string(),
            seq: 3,
        });
        let real = res.resolve("color.accent.default");
        let degraded = res.resolve("ghost.token");
        set.add(
            "E01-对接-真值与兜底可分辨",
            real.provenance().is_some()
                && !real.is_fallback()
                && degraded.is_fallback()
                && degraded.provenance().is_none(),
            "",
        );
    }

    // K3 的可执行验证：变更走订阅/账本，覆盖条目必带 seq（可回溯到变更记录）。
    {
        let mut l = ChangeLedger::new();
        let receipt = l
            .commit(ChangeRequest::new(
                "theme.dusk",
                ChangeAction::Override,
                "color.accent.default",
                "#4c8dff",
                "#7aa2ff",
                "夜间",
                1,
            ))
            .expect("提交");
        let mut res = TokenResolver::new(std_registry());
        res.apply_override(Override {
            path: "color.accent.default".to_string(),
            layer: OverrideLayer::Theme,
            value: "#7aa2ff".to_string(),
            seq: receipt.seq,
        });
        let prov = res.resolve("color.accent.default").provenance();
        set.add(
            "E01-对接-覆盖可溯回账本",
            prov.map(|p| p.seq) == Some(receipt.seq) && l.entry(prov.unwrap().seq).is_some(),
            "",
        );
    }

    // 下游归属表在册且不自指（F3401 不抢自己的活，也不漏关键组）。
    {
        let ids: Vec<&str> = DOWNSTREAM_OWNERSHIP.iter().map(|(i, _)| *i).collect();
        let duties_ok = DOWNSTREAM_OWNERSHIP.iter().all(|(_, d)| !d.trim().is_empty());
        set.add(
            "E01-对接-下游归属不抢活",
            ids.contains(&"VE-F3402")
                && ids.contains(&"VE-F3406")
                && ids.contains(&"VE-F3416")
                && !ids.contains(&"VE-F3401")
                && duties_ok,
            "",
        );
    }

    // ---- 判据七：无障碍（架构图读屏替代）----

    // 架构图读屏替代：四件、两律、数据流、K 域、当前状态全在文字里。
    {
        let a = Architecture::standard();
        let n = a.architecture_narration();
        set.add(
            "E01-读屏-架构图文字替代",
            n.contains("设计令牌运行时架构")
                && n.contains("逐件说明")
                && n.contains("两律说明")
                && n.contains("K 域消费约定")
                && n.contains("数据流顺序"),
            "",
        );
    }

    // 读屏替代必须真的替代图：四件名 + 两律名 + 各自职责都在。
    {
        let a = Architecture::standard();
        let n = a.architecture_narration();
        let pieces_ok = PIECES.iter().all(|p| n.contains(p.zh()));
        let laws_ok = LAWS.iter().all(|l| n.contains(l.zh()));
        let duties_ok = PIECES.iter().all(|p| n.contains(piece_spec(*p).duty_zh));
        let failures_ok = PIECES.iter().all(|p| n.contains(piece_spec(*p).on_failure));
        set.add(
            "E01-读屏-四件两律职责齐备",
            pieces_ok && laws_ok && duties_ok && failures_ok,
            "",
        );
    }

    // 读屏替代与图版同源：四件名按数据流序位出现（覆盖在求值之前）。
    {
        let a = Architecture::standard();
        let n = a.architecture_narration();
        let i_ov = n.find("，覆盖，").unwrap_or(usize::MAX);
        let i_ev = n.find("，求值，").unwrap_or(usize::MAX);
        set.add(
            "E01-读屏-数据流序位可听",
            i_ov != usize::MAX && i_ev != usize::MAX && i_ov < i_ev,
            "",
        );
    }

    // 读屏替代含每件的"不做清单"（边界也要能被听见）。
    {
        let a = Architecture::standard();
        let n = a.architecture_narration();
        set.add(
            "E01-读屏-边界可听",
            PIECES.iter().all(|p| n.contains(piece_spec(*p).not_mine)),
            "",
        );
    }

    // 总纲摘要（一行版）含版本/四件/两律/三处状态。
    {
        let a = Architecture::standard();
        let s = a.screen_text();
        set.add(
            "E01-读屏-总纲摘要含状态",
            s.contains(ARCH_VERSION)
                && s.contains("令牌单源")
                && s.contains("变更有迹")
                && s.contains("在册令牌 0 个")
                && s.contains("订阅表"),
            "",
        );
    }

    // 令牌读屏单行（令牌不能只有颜色没有文字）。
    {
        let d = TokenDef {
            path: "color.accent.default".to_string(),
            kind_id: 1,
            literal: "#4c8dff".to_string(),
            canonical_site: "tokens.css:10".to_string(),
            doc: "强调色".to_string(),
        };
        let s = d.screen_line();
        set.add(
            "E01-读屏-令牌单行含四要素",
            s.contains("color.accent.default")
                && s.contains("#4c8dff")
                && s.contains("tokens.css:10")
                && s.contains("强调色"),
            "",
        );
    }

    // 出处读屏：真值与覆盖值要能说清各自从哪来。
    {
        let reg = Provenance::REGISTRY.screen_text();
        let ov = Provenance {
            layer: OverrideLayer::Scene,
            seq: 7,
        }
        .screen_text();
        set.add(
            "E01-读屏-出处自解释",
            reg.contains("注册表真值") && ov.contains("场景") && ov.contains("第 7 号"),
            "",
        );
    }

    // 求值结果读屏单行：真值态与降级态都念得清。
    {
        let res = TokenResolver::new(std_registry());
        let a = res.resolve("space.gutter").screen_line("space.gutter");
        let b = res.resolve("ghost.token").screen_line("ghost.token");
        set.add(
            "E01-读屏-求值结果双态可念",
            a.contains("space.gutter") && a.contains("注册表真值")
                && b.contains("降级为") && b.contains("原因"),
            "",
        );
    }

    // 订阅表读屏含阈值（在册/累计回收/超期阈值三项）。
    {
        let mut t = SubscriptionTable::new();
        t.subscribe("comp.a", &["color.a"], 0).expect("订阅");
        t.sweep(&[], 1);
        let s = t.screen_text();
        set.add(
            "E01-读屏-订阅表含阈值",
            s.contains("在册 0 条")
                && s.contains("累计回收 1 条")
                && s.contains("静默超期阈值"),
            "",
        );
    }

    // ---- 错误路径（零静默）----

    // 注册表三条拒绝：空路径 / 空字面量 / 缺定义点，各带下一步。
    {
        let mut r = TokenRegistry::new();
        let empty_path = r.insert(TokenDef {
            path: String::new(),
            kind_id: 1,
            literal: "#fff".to_string(),
            canonical_site: "t:1".to_string(),
            doc: "x".to_string(),
        });
        let empty_lit = r.insert(TokenDef {
            path: "color.a".to_string(),
            kind_id: 1,
            literal: String::new(),
            canonical_site: "t:1".to_string(),
            doc: "x".to_string(),
        });
        let no_site = r.insert(TokenDef {
            path: "color.a".to_string(),
            kind_id: 1,
            literal: "#fff".to_string(),
            canonical_site: String::new(),
            doc: "x".to_string(),
        });
        let ok = matches!(empty_path, Err(ref e) if e.code == E_EMPTY_PATH)
            && matches!(empty_lit, Err(ref e) if e.code == E_EMPTY_LITERAL)
            && matches!(no_site, Err(ref e) if e.code == E_NO_CANONICAL_SITE)
            && r.is_empty();
        let all_advice = [empty_path, empty_lit, no_site].iter().all(|x| match x {
            Err(e) => !e.next.trim().is_empty() && !e.who.trim().is_empty(),
            _ => false,
        });
        set.add("E01-错误-入册拒绝三要素", ok && all_advice, "");
    }

    // 缺定义点的拒绝要说清"为什么必须有定义点"（律一的豁免凭据）。
    {
        let mut r = TokenRegistry::new();
        let e = r
            .insert(TokenDef {
                path: "color.a".to_string(),
                kind_id: 1,
                literal: "#fff".to_string(),
                canonical_site: String::new(),
                doc: "x".to_string(),
            })
            .expect_err("应被拒");
        set.add(
            "E01-错误-缺定义点说清缘由",
            e.why.contains("canonical_site") && e.next.contains("定义位置"),
            "",
        );
    }

    // 重复路径拒绝（单源不允许一个路径两个真值），且原值不被污染。
    {
        let mut r = std_registry();
        let dup = r.insert(TokenDef {
            path: "color.accent.default".to_string(),
            kind_id: 1,
            literal: "#000000".to_string(),
            canonical_site: "tokens.css:99".to_string(),
            doc: "重复".to_string(),
        });
        let unchanged = r.find("color.accent.default").map(|d| d.literal.clone());
        set.add(
            "E01-错误-重复路径拒绝",
            matches!(dup, Err(ref e) if e.code == E_DUPLICATE_PATH)
                && unchanged.as_deref() == Some("#4c8dff")
                && r.len() == 4,
            "",
        );
    }

    // 注册表超上限拒绝（不静默增长）。
    {
        let mut r = TokenRegistry::new();
        for i in 0..MAX_TOKENS {
            r.insert(TokenDef {
                path: format!("color.t{}", i),
                kind_id: 1,
                literal: format!("#{:06x}", i),
                canonical_site: "t:1".to_string(),
                doc: "x".to_string(),
            })
            .expect("未满时应成功");
        }
        let over = r.insert(TokenDef {
            path: "color.too_many".to_string(),
            kind_id: 1,
            literal: "#ffffff".to_string(),
            canonical_site: "t:1".to_string(),
            doc: "x".to_string(),
        });
        set.add(
            "E01-错误-注册表超限拒绝",
            matches!(over, Err(ref e) if e.code == E_TOKEN_CAP) && r.len() == MAX_TOKENS,
            "",
        );
    }

    // 错误五元组读屏齐发（现象/原因/下一步/责任方）。
    {
        let mut r = TokenRegistry::new();
        let e = r
            .insert(TokenDef {
                path: String::new(),
                kind_id: 1,
                literal: "#fff".to_string(),
                canonical_site: "t:1".to_string(),
                doc: "x".to_string(),
            })
            .expect_err("应被拒");
        let s = e.screen_text();
        set.add(
            "E01-错误-五元组读屏齐发",
            s.contains(E_EMPTY_PATH)
                && s.contains("原因")
                && s.contains("下一步")
                && s.contains("责任方")
                && s.contains("解析件 F3402"),
            "",
        );
    }

    // 空样式写法（既非引用也非合法字面量）不被漏检。
    {
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "X.tsx:1".to_string(),
            property: "color".to_string(),
            content: String::new(),
        }]);
        set.add(
            "E01-错误-空写法不被漏检",
            r.count_by_code("UNREGISTERED") == 1,
            "",
        );
    }

    // ---- 复杂度声明在册 ----

    {
        let d = COMPLEXITY_DOC;
        set.add(
            "E01-复杂度-声明在册",
            d.contains("C1") && d.contains("O(站点数 + 令牌数)")
                && d.contains("C2") && d.contains("O(1)")
                && d.contains("C5") && d.contains("O(令牌数)")
                && d.contains("C7") && d.contains("O(订阅数)"),
            "",
        );
    }

    // 声明与契约表口径一致（复杂度编号不悬空）。
    {
        let doc = COMPLEXITY_DOC;
        let specs_ok = PIECES.iter().all(|p| {
            let c = piece_spec(*p).complexity;
            doc.contains(c.split(' ').next().unwrap_or(""))
        });
        set.add("E01-复杂度-契约引用不悬空", specs_ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ver01_all_judgements_green() {
        let set = run_ver01_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F3401 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn ver01_four_pieces_shape() {
        // 四件形状固定：解析/求值/订阅/覆盖，各有契约与不做清单。
        assert_eq!(PIECES.len(), 4);
        assert_eq!(LAWS.len(), 2);
        for p in PIECES.iter() {
            let s = piece_spec(*p);
            assert!(!s.duty_zh.is_empty());
            assert!(s.not_mine.contains('不'));
        }
        for l in LAWS.iter() {
            let s = law_spec(*l);
            assert!(!s.statement.is_empty());
            assert!(!s.enforcement.is_empty());
        }
    }

    #[test]
    fn ver01_single_source_catches_hardcode() {
        // 律一的实际拦截能力：写死一个像素值必被检出。
        let a = std_auditor();
        let r = a.audit(&[StyleSite {
            site: "Shell.tsx:1".to_string(),
            property: "color".to_string(),
            content: "#4c8dff".to_string(),
        }]);
        assert!(!r.is_clean());
        assert_eq!(r.count_by_code("HARDCODE"), 1);
        // 而定义点写法必须放行。
        let clean = a.audit(&canonical_sites());
        assert_eq!(clean.count_by_code("HARDCODE"), 0);
        assert!(clean.is_clean());
    }

    #[test]
    fn ver01_ledger_refuses_untraceable() {
        // 律二的实际拦截能力：无痕变更没有入口。
        let mut l = ChangeLedger::new();
        assert!(l
            .commit(ChangeRequest::new(
                "ghost",
                ChangeAction::Override,
                "color.a",
                "",
                "#fff",
                "",
                1
            ))
            .is_err());
        assert!(l.is_empty());
    }

    #[test]
    fn ver01_override_requires_authority() {
        // 判据：覆盖越权→拒绝。
        let auth = OverrideAuthority::new();
        assert!(auth.authorize("x", OverrideLayer::Component).is_err());
    }

    #[test]
    fn ver01_arch_is_standard_clean() {
        // 总纲本体必须自洽（标准态零契约问题）。
        let a = Architecture::standard();
        assert!(a.check_contracts().is_empty());
        assert_eq!(a.version, ARCH_VERSION);
    }

    #[test]
    fn ver01_paper_law_detected_from_registry_not_static_set() {
        // 回归：`check_contracts` 的纸面律判定必须读**在册清单**。
        //
        // 缺陷史：原实现遍历静态 `PIECES`，于是无论总纲登记了什么，每条律恒有
        // 执行者——纸面律永远检不出来，「无执行者的律」这条判据形同虚设。
        // 本测试锁死数据源：清空在册件 → 必须报 `CONTRACT_ORPHAN`；
        // 同时反证静态全集确实做不到这点（否则本测试不会失败）。
        let mut a = Architecture::standard();
        a.pieces.clear();
        let issues = a.check_contracts();
        let orphans: Vec<&ContractIssue> = issues
            .iter()
            .filter(|i| i.code == "CONTRACT_ORPHAN")
            .collect();
        assert_eq!(
            orphans.len(),
            LAWS.len(),
            "清空在册件后每条律都该成纸面律，实得 {:?}",
            orphans.iter().map(|i| i.symptom.as_str()).collect::<Vec<_>>()
        );
        assert!(orphans.iter().all(|i| i.severity == "blocker"));

        // 反证：拿静态全集去数，每条律都"有执行者"——这正是原缺陷的形态。
        let static_serve = |l: &Law| PIECES.iter().filter(|p| spec_laws_of(**p).contains(l)).count();
        assert!(LAWS.iter().all(|l| static_serve(l) > 0));
    }

    #[test]
    fn ver01_stage_order_is_dataflow_not_declaration() {
        // 回归：序位是**数据流序**（覆盖在求值前介入），与枚举声明序无关。
        //
        // 缺陷史：原断言拿 `PIECES` 声明序当序位序，期望 `[0,1,2,3]`；
        // 而 `stage()` 返回 0/2/3/1（声明序 ≠ 数据流序），断言自身即错。
        // 现改为校验「序位恰为 0..3 无重复」+「解析<覆盖<求值<订阅」两条实质判据。
        let mut stages: Vec<u8> = PIECES.iter().map(|p| p.stage()).collect();
        let declared = stages.clone();
        stages.sort_unstable();
        assert_eq!(stages, vec![0, 1, 2, 3], "序位必须恰好占满 0..3 无重复");
        // 声明序（解析/求值/订阅/覆盖）与数据流序（解析/覆盖/求值/订阅）确实不同，
        // 这一点必须为真——否则本回归测试的前提就不成立。
        assert_ne!(declared, vec![0, 1, 2, 3]);
        let st = |p: Piece| piece_spec(p).piece.stage();
        assert!(st(Piece::Resolve) < st(Piece::Override));
        assert!(st(Piece::Override) < st(Piece::Evaluate));
        assert!(st(Piece::Evaluate) < st(Piece::Subscribe));
    }

    #[test]
    fn ver01_sweep_two_judges_are_disjunctive() {
        // 回归：`sweep` 的两条判据是「注销 **或** 静默超期」，缺一不可。
        //
        // 缺陷史：原断言以为「在 `live_owners` 里就安全」，漏了静默超期一判，
        // 期望值与实现不符。`live_owners` 只挡注销，挡不住失联。
        {
            // 形态一：在册（comp.a 活）但从未心跳 → now=1000 > TTL=900 → 仍须回收。
            let mut t = SubscriptionTable::new();
            t.subscribe("comp.a", &["color.a"], 0).unwrap();
            let rec = t.sweep(&["comp.a"], 1_000);
            assert_eq!(rec.len(), 1, "在册但失联者必须被回收");
            assert_eq!(rec[0].cause, "静默超期");
            assert!(t.is_empty());
            assert_eq!(t.reclaimed_total(), 1);
        }
        {
            // 形态二：已注销（comp.b 不在册）但刚心跳过 → 靠注销判据回收。
            let mut t = SubscriptionTable::new();
            t.subscribe("comp.b", &["color.b"], 0).unwrap();
            let rec = t.sweep(&["comp.a"], 10);
            assert_eq!(rec.len(), 1, "已注销者必须被回收");
            assert_eq!(rec[0].cause, "主体已注销");
            assert!(t.is_empty());
        }
        {
            // 形态三：两判据都不命中 → 留下，且心跳续命确实生效。
            let mut t = SubscriptionTable::new();
            let id = t.subscribe("comp.a", &["color.a"], 0).unwrap();
            assert!(t.touch(id, 1_000));
            let rec = t.sweep(&["comp.a"], 1_000);
            assert!(rec.is_empty(), "在册且活跃者不该被回收");
            assert_eq!(t.len(), 1);
            assert_eq!(t.reclaimed_total(), 0);
        }
    }
}