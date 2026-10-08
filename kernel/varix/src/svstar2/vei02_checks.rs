//! VE-F4002 · 域自检（判据逐条对应，见 `vei02_locale.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - BCP47 全量（四段+扩展/规范式/枚举守卫） → `C02-BCP47-*`
//! - 容错表（畸形容忍/容忍表自检/拒绝三要素） → `C02-容错-*`
//! - 回退链（逐级/规范逐条/耗尽显性/匹配） → `C02-回退-*`
//! - 解析缓存（命中 O(1)/表满拒绝/参数域钳制） → `C02-缓存-*`
//! - 单源复用（owner 唯一/不越权/前向如实） → `C02-单源-*`
//!
//! 纯静态校验 + 纯函数推演，无时钟无 IO，回归可复现。

use alloc::format;
use alloc::vec::Vec;

use super::vei02_locale::*;
use crate::checks::CheckSet;

/// VE-F4002 域自检。
pub fn run_vei02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vei02");
    let budget = ParseBudget::DOMAIN_DEFAULT;

    // 容忍表 / 规格表 / 单源 / 前向：四项域级不变量先行，
    // 任一红则后续断言的前提不成立，直接标红并继续跑其余项（不早退）。
    set.add(
        "C02-容错-容忍表每种畸形恰一行",
        check_defect_table().is_ok(),
        "",
    );
    set.add(
        "C02-单源-规格表判据全覆盖",
        check_spec_coverage().is_ok(),
        "",
    );
    set.add(
        "C02-单源-每能力唯一owner",
        check_single_source().is_ok(),
        "",
    );
    set.add(
        "C02-单源-前向槽位落地数如实",
        check_forward_slots().is_ok(),
        "",
    );

    // ---- 判据：BCP47 全量 ----

    // 四段齐备：language-script-region-variant 全部归位且规范式一致。
    {
        let ok = match parse_lenient("zh-Hans-CN-1901", &budget) {
            Ok(p) => {
                p.tag.language == "zh"
                    && p.tag.script.as_deref() == Some("Hans")
                    && p.tag.region.as_deref() == Some("CN")
                    && p.tag.variants == alloc::vec!["1901"]
                    && p.canonical == "zh-Hans-CN-1901"
            }
            Err(_) => false,
        };
        set.add("C02-BCP47-四段齐备", ok, "");
    }

    // 扩展段全量：多个独立 singleton 各成一段（`t` 与 `u` 是两个 singleton）。
    // 注意：`nu`/`latn` 这类 2 字符段形式上是 `u` 的取值——BCP47 无法表达嵌套；
    // 且扩展取值段一律小写化（`de-DE` → `de-de`），故 `t` 段的规范式是 `t-de-de`。
    {
        let ok = match parse_lenient("en-t-de-DE-u-ca-gregory", &budget) {
            Ok(p) => {
                p.tag.extensions.len() == 2
                    && p.tag.extensions[0].singleton == 't'
                    && p.tag.extensions[0].values == alloc::vec!["de", "de"]
                    && p.tag.extensions[1].singleton == 'u'
                    && p.canonical == "en-t-de-de-u-ca-gregory"
            }
            Err(_) => false,
        };
        set.add("C02-BCP47-扩展段全量且排序", ok, "");
    }

    // Unicode 扩展取值口：扁平取值上按 keyword 取 type（历法/数字制的入口）。
    {
        let ok = match parse_lenient("zh-Hans-CN-u-ca-buddhist-nu-latn", &budget) {
            Ok(p) => {
                p.tag.unicode_extension_key("ca") == Some("buddhist")
                    && p.tag.unicode_extension_key("nu") == Some("latn")
                    // 未知 keyword 与末位悬空都显性缺值，不猜。
                    && p.tag.unicode_extension_key("xx").is_none()
            }
            Err(_) => false,
        };
        set.add("C02-BCP47-Unicode扩展取值口", ok, "");
    }

    // 私有用途段收尾：x- 段必须能解析且不进语言语义。
    {
        let ok = match parse_lenient("en-US-x-internal-build7", &budget) {
            Ok(p) => {
                p.tag.language == "en"
                    && p.tag.region.as_deref() == Some("US")
                    && p.tag.privateuse == alloc::vec!["internal", "build7"]
            }
            Err(_) => false,
        };
        set.add("C02-BCP47-私有用途段收尾", ok, "");
    }

    // 数字区域（UN M.49）与字母区域（ISO 3166-1）双形态可判别。
    {
        let alpha = parse_lenient("pt-BR", &budget).is_ok();
        let numeric = parse_lenient("es-419", &budget).is_ok();
        // 三字母既非 2ALPHA 也非 3DIGIT，必须判为"不是区域"。
        let neither = region_kind_of("USA").is_none();
        set.add("C02-BCP47-区域双形态", alpha && numeric && neither, "");
    }

    // 规范化幂等：等价标签归一后规范式一致（缓存键与回退链可比的前提）。
    {
        let ok = match (
            parse_lenient("ZH-hans-cn", &budget),
            parse_lenient("zh-Hans-CN", &budget),
        ) {
            (Ok(a), Ok(b)) => a.canonical == b.canonical && check_canonical_idempotent(&a.tag).is_ok(),
            _ => false,
        };
        set.add("C02-BCP47-规范化幂等", ok, "");
    }

    // 枚举守卫：位置对调/重复/悬空/私有段错位一律拒；多扩展段的
    // `value → singleton` 转移必须**放行**（朴素 rank 非降检查会误杀合法标签）。
    {
        use SubtagKind as K;
        let swapped = guard_segment_order(&[K::Language, K::Region, K::Script]).is_err();
        let dup = guard_segment_order(&[K::Language, K::Script, K::Script]).is_err();
        let dangling = guard_segment_order(&[K::Language, K::ExtensionSingleton]).is_err();
        let tail = guard_segment_order(&[
            K::Language,
            K::PrivateUse,
            K::ExtensionSingleton,
            K::ExtensionValue,
        ])
        .is_err();
        let multi_ext = guard_segment_order(&[
            K::Language,
            K::Script,
            K::Region,
            K::Variant,
            K::ExtensionSingleton,
            K::ExtensionValue,
            K::ExtensionValue,
            K::ExtensionSingleton,
            K::ExtensionValue,
            K::PrivateUse,
        ])
        .is_ok();
        set.add(
            "C02-BCP47-枚举守卫",
            swapped && dup && dangling && tail && multi_ext,
            "",
        );
    }

    // 扩展语言（extlang，历史遗留）仍可解析，不因 BCP47 弃用而失配。
    {
        let ok = match parse_lenient("zh-cmn-Hans", &budget) {
            Ok(p) => p.tag.extlangs == alloc::vec!["cmn"] && p.tag.script.as_deref() == Some("Hans"),
            Err(_) => false,
        };
        set.add("C02-BCP47-扩展语言段", ok, "");
    }

    // ---- 判据：容错表 ----

    // 常见畸形逐类容错，且每类都留下诊断（畸形→容错表+诊断）。
    {
        let cases: [(&str, &str); 6] = [
            (" zh-CN ", D_SPACES),
            ("zh_CN", D_UNDERSCORE),
            ("en_US.UTF-8", D_CODESET),
            ("de_DE@euro", D_AT_MODIFIER),
            ("zh--CN", D_EMPTY_SEGMENT),
            ("zh-", D_TRAILING_HYPHEN),
        ];
        let mut all = true;
        let mut codes: Vec<&'static str> = Vec::new();
        for (input, code) in cases {
            match parse_lenient(input, &budget) {
                Ok(p) => {
                    if !p.repaired_codes().contains(&code) {
                        all = false;
                    }
                    codes.push(code);
                }
                Err(_) => all = false,
            }
        }
        set.add("C02-容错-六类畸形全容错且留诊断", all && codes.len() == 6, "");
    }

    // 大小写非规范被规范化并留诊断（不静默）。
    {
        let ok = match parse_lenient("ZH-HANS-cn", &budget) {
            Ok(p) => p.canonical == "zh-Hans-CN" && p.repaired_codes().contains(&D_CASE),
            Err(_) => false,
        };
        set.add("C02-容错-大小写规范化留痕", ok, "");
    }

    // 非法扩展拒绝三要素（现象/根因/建议齐备——不给路的拒绝按缺陷处理）。
    {
        let mut complete = true;
        let mut codes_ok = true;
        // 悬空 singleton
        match parse_lenient("en-u", &budget) {
            Err(e) => {
                codes_ok &= e.code == E_SINGLETON_NO_VALUE;
                complete &= e.is_complete();
            }
            Ok(_) => codes_ok = false,
        }
        // 重复 singleton（用例里不能混进 x-：私有段会吸收其后全部段）
        match parse_lenient("en-u-ca-gregory-u-nu-latn", &budget) {
            Err(e) => {
                codes_ok &= e.code == E_SINGLETON_DUPLICATE;
                complete &= e.is_complete();
            }
            Ok(_) => codes_ok = false,
        }
        // 空私有用途段
        match parse_lenient("en-x", &budget) {
            Err(e) => {
                codes_ok &= e.code == E_PRIVATEUSE_EMPTY;
                complete &= e.is_complete();
            }
            Ok(_) => codes_ok = false,
        }
        set.add("C02-容错-非法扩展拒绝三要素", codes_ok && complete, "");
    }

    // 无法归位的段显性拒绝，不猜（余项即证据）。
    {
        let ok = match parse_lenient("zh-CN-Hans", &budget) {
            Err(e) => e.code == E_SEGMENT_UNCONSUMED && e.is_complete(),
            Ok(_) => false,
        };
        set.add("C02-容错-无法归位显性拒绝", ok, "");
    }

    // 首段非语言标签拒绝并给出可执行建议。
    {
        let ok = match parse_lenient("1", &budget) {
            Err(e) => e.code == E_LANGUAGE_INVALID && e.is_complete(),
            Ok(_) => false,
        };
        set.add("C02-容错-首段非法拒绝", ok, "");
    }

    // 私有用途段的**吸收语义**（RFC 5646: privateuse = "x" 1*("-" (1*8alphanum))）。
    //
    // 这条替换了原先的"私用段错位拒绝"断言——后者测的是一个**在本语法下不可达**
    // 的行为：私有段取值长度含 1 字符，所以 `x` 之后出现的 `u`（单字符）本身
    // 就是合法私有取值，"错位"无法构造。断言不可达行为等于假门禁，
    // 红了会误导、删了才是真问题。这里改为钉住真实语义：私有段吸收其后
    // 全部 1-8 字母数字段，且这些段**不参与**语言语义（回退链整段去掉）。
    {
        let ok = match parse_lenient("en-x-priv-u-nu-latn", &budget) {
            Ok(p) => {
                p.tag.language == "en"
                    && p.tag.extensions.is_empty()
                    && p.tag.privateuse == alloc::vec!["priv", "u", "nu", "latn"]
                    && fallback_chain(&p.tag).tags()[0] == "en-x-priv-u-nu-latn"
            }
            Err(_) => false,
        };
        set.add("C02-容错-私有段吸收且不入语言语义", ok, "");
    }

    // ---- 判据：回退链 ----

    // 锚点钦定链：zh-CN→zh→und，逐级不跳段。
    {
        let ok = match parse_lenient("zh-CN", &budget) {
            Ok(p) => {
                let c = fallback_chain(&p.tag);
                c.tags() == alloc::vec!["zh-CN", "zh", "und"] && c.exhausted && c.terminal == "und"
            }
            Err(_) => false,
        };
        set.add("C02-回退-zh-CN逐级到und", ok, "");
    }

    // 每跳带规则留痕（回退链规范逐条）：规则数 == 跳数且非空。
    {
        let ok = match parse_lenient("zh-Hans-CN-1901", &budget) {
            Ok(p) => {
                let c = fallback_chain(&p.tag);
                let rules = c.rules();
                c.depth() == rules.len() && rules.iter().all(|r| !r.is_empty())
            }
            Err(_) => false,
        };
        set.add("C02-回退-每跳规则留痕", ok, "");
    }

    // 扩展段整段去掉，不截成无意义的半段（RFC 4647 lookup 语义）。
    {
        let ok = match parse_lenient("de-DE-u-co-phonebk", &budget) {
            Ok(p) => {
                let c = fallback_chain(&p.tag);
                c.tags() == alloc::vec!["de-DE-u-co-phonebk", "de-DE", "de", "und"]
            }
            Err(_) => false,
        };
        set.add("C02-回退-扩展整段截断", ok, "");
    }

    // 起点已是 und 时不重复追加终点，且耗尽位为假（真的配了 und ≠ 回退耗尽）。
    {
        let ok = match parse_lenient("und", &budget) {
            Ok(p) => {
                let c = fallback_chain(&p.tag);
                c.depth() == 1 && !c.exhausted && c.terminal == "und"
            }
            Err(_) => false,
        };
        set.add("C02-回退-und起点不重复追加", ok, "");
    }

    // 匹配：命中第一跳即止，耗尽显性。
    // 注册的是回退链上的**相邻跳**（zh-Hans），不是 zh-CN——
    // RFC 4647 lookup 逐段截断，zh-Hans-CN 的链永不到达 zh-CN。
    {
        let mut reg = LocaleRegistry::new();
        let ok = reg.insert(LanguageTag::lang_region("zh", "TW")).is_ok()
            && match parse_lenient("zh-Hans", &budget) {
                Ok(p) => reg.insert(p.tag).is_ok(),
                Err(_) => false,
            }
            && reg.insert(LanguageTag::language_only("ja")).is_ok();
        let hit = match parse_lenient("zh-Hans-CN", &budget) {
            Ok(p) => {
                let m = lookup(&reg, &p.tag);
                m.matched == "zh-Hans" && m.hop == 1 && !m.exact && !m.exhausted
            }
            Err(_) => false,
        };
        let miss = match parse_lenient("ko-KR", &budget) {
            Ok(p) => {
                let m = lookup(&reg, &p.tag);
                m.matched == "und" && m.exhausted && !m.exact
            }
            Err(_) => false,
        };
        let exact = match parse_lenient("ja", &budget) {
            Ok(p) => {
                let m = lookup(&reg, &p.tag);
                m.exact && m.hop == 0
            }
            Err(_) => false,
        };
        set.add("C02-回退-匹配命中与耗尽显性", ok && hit && miss && exact, "");
    }

    // lookup ≠ filtering：只注册 zh-CN 时，zh-Hans-CN 应耗尽到 und。
    // 这条断言防止把"逐段截断"误实现成"子标签过滤"。
    {
        let mut reg = LocaleRegistry::new();
        let ok = reg.insert(LanguageTag::lang_region("zh", "CN")).is_ok()
            && match parse_lenient("zh-Hans-CN", &budget) {
                Ok(p) => {
                    let m = lookup(&reg, &p.tag);
                    m.matched == "und" && m.exhausted
                }
                Err(_) => false,
            };
        set.add("C02-回退-逐段截断非过滤", ok, "");
    }

    // 私有段不进回退链语义：截断时整段去掉后仍能继续往下退。
    {
        let ok = match parse_lenient("en-US-x-internal-build7", &budget) {
            Ok(p) => {
                let c = fallback_chain(&p.tag);
                c.tags() == alloc::vec!["en-US-x-internal-build7", "en-US", "en", "und"]
            }
            Err(_) => false,
        };
        set.add("C02-回退-私有段整段截断", ok, "");
    }

    // ---- 判据：解析缓存 ----

    // 第二次解析命中，且等价标签共享同一缓存条目（O(1) 的实际收益来源）。
    {
        let mut c = LocaleCache::domain_default();
        let ok = match c.resolve("zh-Hans-CN", &budget) {
            Ok((_, o1)) => {
                o1 == CacheOutcome::MissStored
                    && match c.resolve("ZH_hans_cn", &budget) {
                        Ok((p, o2)) => o2 == CacheOutcome::Hit && p.canonical == "zh-Hans-CN",
                        Err(_) => false,
                    }
                    && c.entries() == 1
                    && c.hits() == 1
                    && c.misses() == 1
            }
            Err(_) => false,
        };
        set.add("C02-缓存-命中且等价标签共享条目", ok, "");
    }

    // 带壳变体（POSIX 字符集后缀）走别名登记：解析后按规范式判为同一条目，
    // 不新增结构——这是"容错形态也共享条目"的证据。
    {
        let mut c = LocaleCache::domain_default();
        let ok = match c.resolve("zh-Hans-CN", &budget) {
            Ok(_) => match c.resolve("zh_Hans_CN.UTF-8", &budget) {
                Ok((p, o)) => {
                    o == CacheOutcome::AliasHit
                        && p.canonical == "zh-Hans-CN"
                        && c.entries() == 1
                        // 别名登记后两种写法都走命中路径。
                        && c.resolve("zh_Hans_CN.UTF-8", &budget).is_ok()
                        && c.entries() == 1
                }
                Err(_) => false,
            },
            Err(_) => false,
        };
        set.add("C02-缓存-带壳变体别名登记", ok, "");
    }

    // 缓存只存成功产物：失败不入表（避免"命中即返回旧错"的陷阱）。
    {
        let mut c = LocaleCache::domain_default();
        let before = c.entries();
        let failed = c.resolve("en-u", &budget).is_err();
        set.add(
            "C02-缓存-失败不入表",
            failed && c.entries() == before,
            "",
        );
    }

    // 表满显性拒绝，不静默覆盖（静默覆盖 → 命中率无征兆掉下去 → 偶发变慢）。
    // 喂入必须是**合法且互异**的标签：`q0`/`q1` 首段含数字，解析阶段就被拒，
    // 根本走不到索引；标签相同又会命中同一条目。采用 2 字母码（BCP47
    // language 段=2-8 纯字母）保证每条都真能入表。
    {
        const CODES: [&str; 16] = [
            "aa", "ab", "ac", "ad", "ae", "af", "ag", "ah", "ai", "aj", "ak", "al", "am", "an",
            "ao", "ap",
        ];
        let (mut tiny, _) = LocaleCache::new(MIN_INDEX_SLOTS);
        let mut last_err = None;
        let mut stored = 0usize;
        for code in CODES.iter() {
            assert!(
                parse_lenient(code, &budget).is_ok(),
                "表满用例的喂入标签必须合法：{}",
                code
            );
            match tiny.resolve(code, &budget) {
                Ok(_) => stored += 1,
                Err(e) => {
                    last_err = Some(e);
                    break;
                }
            }
        }
        let ok = matches!(&last_err, Some(e) if e.code == E_INDEX_FULL && e.is_complete())
            && stored >= 1;
        set.add("C02-缓存-表满显性拒绝", ok, "");
    }

    // 参数域钳制可见：越界请求被钳到边界且留记录。
    {
        let (b, log) = ParseBudget::requested(usize::MAX);
        let clamped_len = b.max_len == MAX_TAG_LEN && log.len() == 1;
        let (c, rec) = LocaleCache::new(1);
        let clamped_slots = c.slots() == MIN_INDEX_SLOTS
            && matches!(&rec, Some(r) if r.requested == 1 && r.effective == MIN_INDEX_SLOTS);
        set.add("C02-缓存-参数域钳制可见", clamped_len && clamped_slots, "");
    }

    // 超预算标签显性拒绝（不静默截断成半个标签）。
    {
        let e = parse_lenient("zh-Hans-CN-u-ca-buddhist", &(ParseBudget {
            max_len: 8,
            ..ParseBudget::DOMAIN_DEFAULT
        }));
        let ok = matches!(&e, Err(r) if r.code == E_BUDGET_EXCEEDED && r.is_complete());
        set.add("C02-缓存-超预算显性拒绝", ok, "");
    }

    // ---- 判据：单源复用 ----

    // 本项确实持有三项核心能力（否则单源声明是空话）。
    {
        let owns_parse = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == CAP_TAG_PARSE && c.owner == OWNER_SELF);
        let owns_fb = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == CAP_FALLBACK_CHAIN && c.owner == OWNER_SELF);
        let owns_tol = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == CAP_DEFECT_TOLERANCE && c.owner == OWNER_SELF);
        set.add("C02-单源-本项持三项核心能力", owns_parse && owns_fb && owns_tol, "");
    }

    // F2948 / F2915 / N02 / F4003 均为 consumer 而非本项 owner（不越权施工）。
    {
        let not_mine = LANG_SINGLE_SOURCE
            .iter()
            .filter(|c| c.owner == OWNER_SELF)
            .all(|c| {
                !matches!(
                    c.key,
                    CAP_LANG_MATCH_TREE | CAP_LANG_FONT_PREF | CAP_FONT_PREF_TABLE
                        | CAP_TYPOGRAPHY_CONSUME | CAP_DIRECTION_MODEL
                )
            });
        let f2948_owns = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == CAP_LANG_MATCH_TREE && c.owner == "VE-F2948");
        let f2915_owns = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == CAP_FONT_PREF_TABLE && c.owner == "VE-F2915");
        set.add(
            "C02-单源-对端为consumer不越权",
            not_mine && f2948_owns && f2915_owns,
            "",
        );
    }

    // 消费方必须在本表里**作为某项能力的 owner 出现过**。
    // 命名空间纪律：`consumes` 装的是**项号**（VE-F2948…），`key` 装的是
    // **能力键**（tag-parse…）。拿项号去和 declared 的能力键表比对必然全失配——
    // 那是两种命名空间，混在一列里比会让这条断言恒假（看着像边界严，实则无效）。
    {
        let owners: Vec<&str> = LANG_SINGLE_SOURCE.iter().map(|c| c.owner).collect();
        let every_consumer_is_a_declared_owner = LANG_SINGLE_SOURCE
            .iter()
            .all(|c| c.consumes.iter().all(|p| owners.contains(p)));
        // 反向：本项登记的三项能力，对端确实都列为 consumer（不是只有本项自说自话）。
        let peer_side = ["VE-F2948", "VE-F2915", "VE-N02", "VE-F4003", "VE-F4011"];
        let peers_consume_ours = LANG_SINGLE_SOURCE
            .iter()
            .filter(|c| c.owner == OWNER_SELF)
            .all(|c| c.consumes.iter().all(|p| peer_side.contains(p)));
        let f2948_consumes_parse = LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.owner == "VE-F2948" && c.consumes.contains(&OWNER_SELF));
        set.add(
            "C02-单源-消费方均指向已声明能力",
            every_consumer_is_a_declared_owner
                && peers_consume_ours
                && f2948_consumes_parse,
            "",
        );
    }

    // 前向槽位：F4003 方向槽位必须未落地（谎报已落地 → RTL 整体失效）。
    {
        let dir_slot = FORWARD_SLOTS
            .iter()
            .find(|s| s.key == CAP_DIRECTION_MODEL);
        let ok = matches!(dir_slot, Some(s) if !s.landed && s.consumer == "VE-F4003");
        set.add("C02-单源-方向槽位未谎报落地", ok, "");
    }

    // 前向取值口可用：RTL 语言查表得 Rtl，其他得 Ltr（只查表不做判定）。
    {
        let ar = default_direction(&LanguageTag::language_only("ar"));
        let he = default_direction(&LanguageTag::language_only("he"));
        let zh = default_direction(&LanguageTag::lang_region("zh", "CN"));
        let en = default_direction(&LanguageTag::lang_region("en", "US"));
        set.add(
            "C02-单源-方向取值口可查",
            ar == WritingDirection::Rtl
                && he == WritingDirection::Rtl
                && zh == WritingDirection::Ltr
                && en == WritingDirection::Ltr,
            "",
        );
    }

    // ---- 无障碍与隐私面 ----

    // 替述可读：spoken() 不依赖本地化资源即可产出人话串（内核侧可用）。
    {
        let ok = match parse_lenient("zh-Hans-CN-x-internal", &budget) {
            Ok(p) => {
                let s = p.tag.spoken();
                s.contains("zh") && s.contains("Hans") && s.contains("CN") && s.contains("私有用途")
            }
            Err(_) => false,
        };
        set.add("C02-无障碍-替述可读", ok, "");
    }

    // 段形态描述可读（调试器/读屏拿得到形态名），且**全局去重**不重复罗列。
    // 注意：不能用带 script 的标签去断言"扩展单键/扩展取值"——`en-t-de-...`
    // 没有文种段，形态串里就不会有"文种"。这里分两个标签各钉一段形态。
    {
        let ext_shape = match parse_lenient("en-t-de-u-ca-gregory-nu-latn", &budget) {
            Ok(p) => {
                let s = describe_tag_shape(&p.tag);
                // 全局去重：扩展段序列是 [singleton,value,value,singleton,value]，
                // 朴素相邻去重会留下重复项，这里必须只出现一次。
                s.contains("主语言")
                    && s.contains("扩展单键")
                    && s.contains("扩展取值")
                    && s.matches("扩展单键").count() == 1
                    && s.matches("扩展取值").count() == 1
            }
            Err(_) => false,
        };
        let four_shape = match parse_lenient("zh-Hans-CN", &budget) {
            Ok(p) => {
                let s = describe_tag_shape(&p.tag);
                s == "主语言→文种→区域"
            }
            Err(_) => false,
        };
        let priv_shape = match parse_lenient("en-US-x-internal", &budget) {
            Ok(p) => describe_tag_shape(&p.tag).contains("私有用途"),
            Err(_) => false,
        };
        set.add(
            "C02-无障碍-形态名可读",
            ext_shape && four_shape && priv_shape,
            "",
        );
    }

    // 隐私面：本项不引入任何用户数据面（零隐私面复述）。
    // 判据落在"输入只有标签字符串、产物只有标签结构"这一可核事实上。
    {
        let ok = match parse_lenient("zh-CN", &budget) {
            Ok(p) => p.tag.subtag_count() == 2 && p.repairs.is_empty(),
            Err(_) => false,
        };
        set.add("C02-隐私-零隐私面（只标签无用户数据）", ok, "");
    }

    // 一体入口：解析/回退/匹配出自同一次解析（防三份结果分叉）。
    {
        let mut reg = LocaleRegistry::new();
        let ok = reg.insert(LanguageTag::lang_region("zh", "CN")).is_ok()
            && match resolve_locale(&reg, "ZH_hans_cn", &budget) {
                Ok((parsed, chain, m)) => {
                    parsed.canonical == "zh-Hans-CN"
                        && chain.origin == parsed.canonical
                        && m.requested == parsed.canonical
                        // 表内只有 zh-CN，故逐段截断后耗尽到 und（exhausted 显性）。
                        && m.matched == "und"
                        && m.exhausted
                }
                Err(_) => false,
            };
        set.add("C02-回退-一体入口三结果同源", ok, "");
    }

    // 规格表规模与编号连续（域级元不变量）。
    {
        let ids_ok = SPEC_SHEET
            .iter()
            .enumerate()
            .all(|(i, s)| s.id == format!("S-{:02}", i + 1));
        set.add("C02-单源-规格表编号连续", ids_ok, "");
    }

    set
}