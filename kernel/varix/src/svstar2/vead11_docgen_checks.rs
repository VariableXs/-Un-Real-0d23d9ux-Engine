//! vead11_docgen_checks — VE-F6011 物理文档·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：同源生成（2~4、5~8 双写对拍）/ 三律解释（6）/
//! CI 防漂移（9、15、16）/ 样本库与失效检测（11~14）/ 判据面（17~20）。

use alloc::format;
use crate::checks::CheckSet;
use crate::svstar2::vead11_docgen::*;
use alloc::string::String;
use alloc::vec::Vec;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_vead11_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6011");

    // 1 词典闭集：静态 16 词全可查、长度自洽、重复注册幂等拒、空词拒。
    let ok1 = {
        let mut g = Glossary::new();
        let mut all_known = true;
        for (t, _d) in GLOSSARY.iter() {
            if !g.known(t) {
                all_known = false;
            }
        }
        all_known
            && g.len() == 16
            && g.lookup("物理体").is_some()
            && g.lookup("不存在词").is_none()
            && g.register("物理体", "重复注册应拒") == false
            && g.register("", "空词应拒") == false
            && g.register("新术语", "运行期追加词条") == true
            && g.extra_len() == 1
            && g.lookup("新术语").is_some()
    };
    s.add("glossary_closed_set", ok1, "16 static + register rules");

    // 2 同源生成：canonical 事实源一次生成六章全，行数 O(条目) 非零，
    //   schema/防线代际写入文档头。
    let ok2 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                doc.missing_sections().len() == 0
                    && doc.total_lines() > 0
                    && doc.schema_ver == DOC_SCHEMA_VER
                    && doc.active_gen == 4
                    && doc.sections.len() == 6
            }
            Err(_) => false,
        }
    };
    s.add("sot_generate_six_sections", ok2, "six sections closed set");

    // 3 结构级同源防线：裸正文注入一律 0x3C01 拒——手写混入在类型层不可达。
    let ok3 = match DocModel::from_raw("手写的四层架构说明正文") {
        Err(E_SOURCE_BROKEN) => true,
        _ => false,
    };
    s.add("raw_injection_rejected", ok3, "from_raw always 0x3C01");

    // 4 事实源前置闸：锚点未验证 / 防线代际非法 → 生成拒 0x3C01；
    //   空词典 → 0x3C03。
    let ok4 = {
        let mut f1 = SourceFacts::canonical();
        f1.anchors_verified = false;
        let mut f2 = SourceFacts::canonical();
        f2.active_defense_gen = 0;
        let mut f3 = SourceFacts::canonical();
        f3.active_defense_gen = 5;
        let g = Glossary::new();
        let r1 = match DocGen::generate(&f1, &g) {
            Err(E_SOURCE_BROKEN) => true,
            _ => false,
        };
        let r2 = match DocGen::generate(&f2, &g) {
            Err(E_SOURCE_BROKEN) => true,
            _ => false,
        };
        let r3 = match DocGen::generate(&f3, &g) {
            Err(E_SOURCE_BROKEN) => true,
            _ => false,
        };
        r1 && r2 && r3 && !f3.defense_gen_valid()
    };
    s.add("facts_precondition_gate", ok4, "anchor+gen gate 0x3C01");

    // 5 四层覆盖双写：PHYS_LAYERS 四层闭集 ↔ 架构章渲染正文逐层点名 +
    //   预算合计一致（单层预算求和 ↔ 正文合计行）。
    let ok5 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                let mut arch_ok = DocGen::verify_arch_coverage(&doc).is_ok();
                if let Some(sec) = doc.section(SectionKind::Architecture) {
                    let mut joined = String::new();
                    for line in sec.lines.iter() {
                        joined.push_str(line);
                        joined.push('\n');
                    }
                    let mut total: u64 = 0;
                    for layer in PhysLayer::all().iter() {
                        total += layer.budget_us();
                    }
                    let mut want = String::new();
                    want.push_str("合计 ");
                    push_u64(&mut want, total);
                    if !joined.contains(&want) {
                        arch_ok = false;
                    }
                } else {
                    arch_ok = false;
                }
                arch_ok
            }
            Err(_) => false,
        }
    };
    s.add("arch_dualwrite_budget_sum", ok5, "4 layers + budget sum");

    // 6 三律解释覆盖对拍：三律条文 + 锚点 + 违例后果三件全部出现在
    //   解释章正文（三律解释判据的完整兑现）。
    let ok6 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                let mut all_ok = DocGen::verify_laws_coverage(&doc).is_ok();
                if let Some(sec) = doc.section(SectionKind::ThreeLaws) {
                    let mut joined = String::new();
                    for line in sec.lines.iter() {
                        joined.push_str(line);
                        joined.push('\n');
                    }
                    for law in PhysLaw::all().iter() {
                        if !joined.contains(law.name())
                            || !joined.contains(law.clause())
                            || !joined.contains(law.anchor())
                            || !joined.contains(law.violation())
                        {
                            all_ok = false;
                        }
                    }
                } else {
                    all_ok = false;
                }
                all_ok && PhysLaw::all().len() == 3
            }
            Err(_) => false,
        }
    };
    s.add("three_laws_coverage", ok6, "clause+anchor+violation");

    // 7 步长事实双写：SourceFacts（step_hz/max_substeps/三重防护位）↔
    //   正文逐项渲染一致（1/60、4 子步、三防护启用态）。
    let ok7 = {
        let mut facts = SourceFacts::canonical();
        facts.step_hz = 120;
        facts.max_substeps = 6;
        facts.determinism_mask = 0b011; // 平台清单停用
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                if let Some(sec) = doc.section(SectionKind::StepDeterminism) {
                    let mut joined = String::new();
                    for line in sec.lines.iter() {
                        joined.push_str(line);
                        joined.push('\n');
                    }
                    let mut hz = String::new();
                    hz.push_str("1/");
                    push_u64(&mut hz, 120);
                    let mut sub = String::new();
                    push_u64(&mut sub, 6);
                    sub.push_str(" 个子步");
                    if !joined.contains(&hz) || !joined.contains(&sub) {
                        false
                    } else {
                        let mut on_cnt = 0usize;
                        let mut off_cnt = 0usize;
                        for (name, on) in facts.determinism_items().iter() {
                            let mut tag = String::new();
                            tag.push_str(name);
                            tag.push_str("：");
                            tag.push_str(if *on { "启用" } else { "停用" });
                            if joined.contains(&tag) {
                                if *on {
                                    on_cnt += 1;
                                } else {
                                    off_cnt += 1;
                                }
                            }
                        }
                        on_cnt == 2 && off_cnt == 1
                    }
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    };
    s.add("step_facts_dualwrite", ok7, "hz/substeps/mask rendered");

    // 8 NaN 教学覆盖：五案例标题+引入防线名全部点名；四代演进时间线
    //   全链出现（教学表 ↔ 渲染输出 双写对拍）。
    let ok8 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                let mut ok = DocGen::verify_teaching_coverage(&doc).is_ok();
                if let Some(sec) = doc.section(SectionKind::NanTeaching) {
                    let mut joined = String::new();
                    for line in sec.lines.iter() {
                        joined.push_str(line);
                        joined.push('\n');
                    }
                    for c in NAN_CASES.iter() {
                        if !joined.contains(c.title)
                            || !joined.contains(c.root_cause)
                            || !joined.contains(c.current_note)
                        {
                            ok = false;
                        }
                    }
                } else {
                    ok = false;
                }
                ok && NAN_CASES.len() == 5
            }
            Err(_) => false,
        }
    };
    s.add("nan_teaching_coverage", ok8, "5 cases + 4-gen timeline");

    // 9 缺章检测：抽掉一章 → missing_sections 精确点名 + CI 语义
    //   （缺章→补）。术语未知扫描同节覆盖（判据 10）。
    let ok9 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(mut doc) => {
                doc.sections.retain(|sec| sec.kind != SectionKind::RegressCovenant);
                let miss = doc.missing_sections();
                miss.len() == 1 && miss[0] == SectionKind::RegressCovenant
            }
            Err(_) => false,
        }
    };
    s.add("missing_section_detected", ok9, "drop one -> named");

    // 10 术语词典校验：未知术语扫描精确报出（术语→词典校验），
    //    已注册术语不误报；运行期追加术语即刻可查。
    let ok10 = {
        let facts = SourceFacts::canonical();
        let mut g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(mut doc) => {
                // 正常产物零未知
                let clean = doc.scan_unknown_terms(&g).len() == 0;
                // 注入未知术语 → 精确报出
                doc.sections[0].term_refs.push(alloc::format!("{}", "幽灵术语"));
                let unknown = doc.scan_unknown_terms(&g);
                clean && unknown.len() == 1 && unknown[0] == "幽灵术语"
            }
            Err(_) => false,
        }
    };
    s.add("term_unknown_scan", ok10, "glossary violation named");

    // 11 样本注册纪律：绑 Gen4 全捕样本注册 OK；绑 Gen1 的 Inf 注册拒
    //    （注册时必须可捕获）；未知案例回指拒；合法案例回指 OK。
    let ok11 = {
        let mut lib = NanRegressLibrary::new();
        let r1 = lib.register_sample(
            PayloadKind::QuietNaN,
            PayloadKind::QuietNaN.canonical_bits(),
            1,
            DefenseGen::Gen4SourceClamp,
            10,
        );
        let r2 = lib.register_sample(
            PayloadKind::Infinity,
            PayloadKind::Infinity.canonical_bits(),
            2,
            DefenseGen::Gen1EqCheck,
            10,
        );
        let r3 = lib.register_sample(
            PayloadKind::NegZero,
            PayloadKind::NegZero.canonical_bits(),
            99,
            DefenseGen::Gen4SourceClamp,
            10,
        );
        let r4 = lib.register_sample(
            PayloadKind::NegZero,
            PayloadKind::NegZero.canonical_bits(),
            3,
            DefenseGen::Gen4SourceClamp,
            10,
        );
        r1.is_ok() && r2 == Err(E_SAMPLE_STALE) && r3 == Err(E_SAMPLE_STALE) && r4.is_ok()
            && lib.len() == 2
    };
    s.add("sample_register_rules", ok11, "catchable+case-ref gate");

    // 12 退役补捕语义：绑 Gen1 quiet NaN，防线升 Gen4 后——Gen1 退役但
    //    现行代可补捕 → 存活；补捕路径也被失明 → 失效（R1 完整链）。
    let ok12 = {
        let mut lib = NanRegressLibrary::new();
        let bits = PayloadKind::QuietNaN.canonical_bits();
        let id = match lib.register_sample(PayloadKind::QuietNaN, bits, 1, DefenseGen::Gen1EqCheck, 10) {
            Ok(id) => id,
            Err(_) => 0,
        };
        if id == 0 {
            false
        } else {
            let newly = lib.verify_all(4, 20);
            let alive_after_rescue = match lib.get(id) {
                Some(s) => s.status == SampleStatus::Alive,
                None => false,
            };
            // 双失明：绑代 Gen1 与现行代 Gen4 都对该位型失明 → R1+R2 失效
            lib.inject_flaw(DefenseGen::Gen1EqCheck, bits);
            lib.inject_flaw(DefenseGen::Gen4SourceClamp, bits);
            let newly2 = lib.verify_all(4, 30);
            let stale_now = match lib.get(id) {
                Some(s) => s.status == SampleStatus::Stale,
                None => false,
            };
            newly == 0 && alive_after_rescue && newly2 == 1 && stale_now
        }
    };
    s.add("retire_rescue_dual_blind", ok12, "R1 rescue then stale");

    // 13 检测器失明 → 失效 → 清除复活：R2 魔数失明判废、clear 复活
    //    （防线回归的真实失效路径）。
    let ok13 = {
        let mut lib = NanRegressLibrary::new();
        let bits = PayloadKind::Denormal.canonical_bits();
        let id = match lib.register_sample(PayloadKind::Denormal, bits, 5, DefenseGen::Gen3FloodGate, 10) {
            Ok(id) => id,
            Err(_) => 0,
        };
        if id == 0 {
            false
        } else {
            lib.inject_flaw(DefenseGen::Gen3FloodGate, bits);
            let newly1 = lib.verify_all(4, 20);
            let stale1 = match lib.get(id) {
                Some(s) => s.status == SampleStatus::Stale && s.consecutive_failures == 1,
                None => false,
            };
            lib.clear_flaw();
            let newly2 = lib.verify_all(4, 30);
            let alive2 = match lib.get(id) {
                Some(s) => s.status == SampleStatus::Alive && s.consecutive_failures == 0,
                None => false,
            };
            newly1 == 1 && stale1 && newly2 == 0 && alive2
        }
    };
    s.add("flaw_blind_and_revive", ok13, "R2 stale then revive");

    // 14 自动清理门槛：连续失效 1 次不清（检测含自动清理的纪律面）；
    //    连续 2 次达标才清；Alive 样本连 force 也不动；清理账目自洽。
    let ok14 = {
        let mut lib = NanRegressLibrary::new();
        let bits = PayloadKind::QuietNaN.canonical_bits();
        let id1 = match lib.register_sample(PayloadKind::QuietNaN, bits, 1, DefenseGen::Gen4SourceClamp, 10) {
            Ok(id) => id,
            Err(_) => 0,
        };
        let id2 = match lib.register_sample(PayloadKind::Infinity, PayloadKind::Infinity.canonical_bits(), 2, DefenseGen::Gen4SourceClamp, 10) {
            Ok(id) => id,
            Err(_) => 0,
        };
        if id1 == 0 || id2 == 0 {
            false
        } else {
            lib.inject_flaw(DefenseGen::Gen4SourceClamp, bits);
            // 第一轮：id1 失效（失明），id2 存活
            lib.verify_all(4, 20);
            let first_purge = lib.auto_purge(false).unwrap_or(999);
            // 第二轮：id1 连续失效第 2 次
            lib.verify_all(4, 40);
            let second_purge = lib.auto_purge(false).unwrap_or(999);
            let (total, alive, stale, purged) = lib.stats();
            first_purge == 0 && second_purge == 1
                && total == 2 && alive == 1 && stale == 0 && purged == 1
                && lib.get(id1).map(|s| s.status == SampleStatus::Purged).unwrap_or(false)
                && lib.get(id2).map(|s| s.status == SampleStatus::Alive).unwrap_or(false)
                && lib.purged_total() == 1
        }
    };
    s.add("auto_purge_threshold", ok14, "2 strikes then purge");

    // 15 CI 防线回归：全存活样本被现行防线捕获 → pass（计数正确）；
    //    失明注入后不跑维护周期直接 CI → 漏捕 → RegressBlocked。
    let ok15 = {
        let mut lib = NanRegressLibrary::new();
        for kind in PayloadKind::all().iter() {
            let _ = lib.register_sample(*kind, kind.canonical_bits(), 0, DefenseGen::Gen4SourceClamp, 10);
        }
        let facts = SourceFacts::canonical();
        let mut gate = CiGate::new();
        let pass1 = gate.run_regress(&lib, &facts).unwrap_or(999);
        // 注入失明：Denormal 在 Gen4 失明 → CI 漏捕
        lib.inject_flaw(DefenseGen::Gen4SourceClamp, PayloadKind::Denormal.canonical_bits());
        let fail = gate.run_regress(&lib, &facts);
        pass1 == 5 && fail == Err(E_SAMPLE_STALE) && gate.state == GateState::RegressBlocked
            && gate.regress_hits() == 1
    };
    s.add("ci_regress_gate", ok15, "5 pass then miss blocked");

    // 16 漂移→CI 阻断→双签解除：批快照后事实源改动 → 重渲染封印漂移
    //    → DriftBlocked；单签重复拒、双签解除回 Armed（0x3C0A 全链）。
    let ok16 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        let doc = match DocGen::generate(&facts, &g) {
            Ok(d) => d,
            Err(_) => return_false(),
        };
        let seal_a = doc.seal();
        let mut gate = CiGate::new();
        let app = gate.approve_snapshot(seal_a, "塔主", "初版快照冻结", 100);
        // 无快照时漂移检查直接拒
        let mut gate0 = CiGate::new();
        let no_snap = gate0.drift_check(seal_a) == Err(E_DRIFT);
        // 事实源改动 → 新封印 → 漂移
        let mut facts2 = SourceFacts::canonical();
        facts2.step_hz = 120;
        let doc2 = match DocGen::generate(&facts2, &g) {
            Ok(d) => d,
            Err(_) => return_false(),
        };
        let drift = gate.drift_check(doc2.seal());
        let blocked = gate.state == GateState::DriftBlocked && gate.drift_hits() == 1;
        // 双签：同人重签拒
        let s1 = gate.unblock("小明");
        let dup = gate.unblock("小明");
        let armed = gate.unblock("塔主");
        app.is_ok() && no_snap
            && drift == Err(E_DRIFT) && blocked
            && s1.map(|st| st == GateState::DriftBlocked).unwrap_or(false)
            && dup == Err(E_DUAL_SIGN_MISSING)
            && armed.map(|st| st == GateState::Armed).unwrap_or(false)
            && seal_a != doc2.seal()
    };
    s.add("drift_block_dual_unblock", ok16, "seal drift + dual sign");

    // 17 无障碍三路：跳级拒 / 空节拒 / 裸链接拒；正常文档通过。
    let ok17 = {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        let good = match DocGen::generate(&facts, &g) {
            Ok(d) => d.a11y_check().is_ok(),
            Err(_) => false,
        };
        // 跳级：单节 A11yCovenant（level3）从头起
        let mut skip = DocModel {
            schema_ver: DOC_SCHEMA_VER,
            facts_ver: 1,
            sections: Vec::new(),
            active_gen: 4,
        };
        skip.sections.push(Section {
            kind: SectionKind::A11yCovenant,
            heading: alloc::format!("{}", SectionKind::A11yCovenant.as_str()),
            lines: {
                let mut v = Vec::new();
                v.push(alloc::format!("{}", "一行正文"));
                v
            },
            term_refs: Vec::new(),
        });
        let skip_fail = skip.a11y_check() == Err(E_A11Y_FAIL);
        // 空节
        let mut empty = DocModel {
            schema_ver: DOC_SCHEMA_VER,
            facts_ver: 1,
            sections: Vec::new(),
            active_gen: 4,
        };
        empty.sections.push(Section {
            kind: SectionKind::Architecture,
            heading: alloc::format!("{}", SectionKind::Architecture.as_str()),
            lines: Vec::new(),
            term_refs: Vec::new(),
        });
        let empty_fail = empty.a11y_check() == Err(E_A11Y_FAIL);
        // 裸链接
        let mut bare = DocModel {
            schema_ver: DOC_SCHEMA_VER,
            facts_ver: 1,
            sections: Vec::new(),
            active_gen: 4,
        };
        bare.sections.push(Section {
            kind: SectionKind::Architecture,
            heading: alloc::format!("{}", SectionKind::Architecture.as_str()),
            lines: {
                let mut v = Vec::new();
                v.push(alloc::format!("{}", "参考 [附录 未闭合"));
                v
            },
            term_refs: Vec::new(),
        });
        let bare_fail = bare.a11y_check() == Err(E_A11Y_FAIL);
        good && skip_fail && empty_fail && bare_fail
    };
    s.add("a11y_three_paths", ok17, "skip/empty/bare rejected");

    // 18 U 域 schema 对接：版本对拍（1 过 / 2 拒 0x3C08）；导出键集
    //    与样本库统计双写一致。
    let ok18 = {
        let mut lib = NanRegressLibrary::new();
        let _ = lib.register_sample(PayloadKind::QuietNaN, PayloadKind::QuietNaN.canonical_bits(), 1, DefenseGen::Gen4SourceClamp, 10);
        let _ = lib.register_sample(PayloadKind::Infinity, PayloadKind::Infinity.canonical_bits(), 2, DefenseGen::Gen4SourceClamp, 10);
        lib.inject_flaw(DefenseGen::Gen4SourceClamp, PayloadKind::QuietNaN.canonical_bits());
        lib.verify_all(4, 20);
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        match DocGen::generate(&facts, &g) {
            Ok(doc) => {
                let schema_ok = schema_version_check(U_SCHEMA_VER).is_ok()
                    && schema_version_check(2) == Err(E_SCHEMA_MISMATCH)
                    && schema_version_check(0) == Err(E_SCHEMA_MISMATCH);
                let kv = export_schema(&doc, &lib);
                let (total, alive, stale, purged) = lib.stats();
                let mut has_key = |k: &str| -> bool {
                    for (kk, _vv) in kv.iter() {
                        if kk == k {
                            return true;
                        }
                    }
                    false
                };
                let mut get_v = |k: &str| -> u64 {
                    for (kk, vv) in kv.iter() {
                        if kk == k {
                            return parse_u64(vv);
                        }
                    }
                    u64::MAX
                };
                schema_ok
                    && has_key("doc.schema_ver") && has_key("doc.seal_hex")
                    && has_key("nan.samples_total") && has_key("nan.samples_alive")
                    && get_v("nan.samples_total") == total as u64
                    && get_v("nan.samples_alive") == alive as u64
                    && get_v("nan.samples_stale") == stale as u64
                    && get_v("nan.samples_purged") == purged as u64
                    && total == 2 && stale == 1
            }
            Err(_) => false,
        }
    };
    s.add("u_schema_export_dualwrite", ok18, "ver check + stats mirror");

    // 19 两权分立：Runtime 注册术语/样本/批快照全拒 0x3C09 + 审计留痕；
    //    Maintainer 全通；Runtime 生成/导出可用（只读面）。
    let ok19 = {
        let mut eng = DocEngine::mount(Role::Runtime, SourceFacts::canonical());
        let r1 = eng.register_term("孤词", "定义", 10);
        let r2 = eng.register_sample(PayloadKind::QuietNaN, PayloadKind::QuietNaN.canonical_bits(), 1, DefenseGen::Gen4SourceClamp, 11);
        let r3 = eng.approve_snapshot(0, "塔主", "理由充足", 12);
        let gen_ok = eng.generate(13).is_ok();
        let mut eng2 = DocEngine::mount(Role::Maintainer, SourceFacts::canonical());
        let m1 = eng2.register_term("隔离体复核", "Maintainer 追加词条", 20).is_ok();
        let m2 = eng2.register_sample(PayloadKind::Denormal, PayloadKind::Denormal.canonical_bits(), 5, DefenseGen::Gen4SourceClamp, 21).is_ok();
        let audit_violations = eng.audit.count_of(AuditEvent::RoleViolation);
        r1 == Err(E_ROLE_VIOLATION) && r2 == Err(E_ROLE_VIOLATION) && r3 == Err(E_ROLE_VIOLATION)
            && gen_ok && m1 && m2 && audit_violations == 3
    };
    s.add("two_swords_role_gate", ok19, "runtime denied + audit");

    // 20 封印与自检：mount 封印可验、selfcheck 三件全绿、
    //    生成→管线全绿（覆盖对拍+无障碍+术语+缺章）端到端。
    let ok20 = {
        let eng = DocEngine::mount(Role::Maintainer, SourceFacts::canonical());
        let seal_ok = eng.verify_seal() && eng.seal() != 0;
        let sc = selfcheck_constants() && selfcheck_num_fmt() && selfcheck_detect_matrix();
        let mut eng2 = DocEngine::mount(Role::Maintainer, SourceFacts::canonical());
        let pipe = match eng2.run_pipeline(50) {
            Ok((doc, checked)) => doc.total_lines() > 0 && checked == 0,
            Err(_) => false,
        };
        seal_ok && sc && pipe
    };
    s.add("seal_selfcheck_pipeline", ok20, "seal+3 selfchecks+e2e");

    s
}

/// 判据辅助：恒 false（match 兜底用）
fn return_false() -> DocModel {
    DocModel {
        schema_ver: 0,
        facts_ver: 0,
        sections: Vec::new(),
        active_gen: 0,
    }
}

/// 判据辅助：简单 u64 解析（schema 导出值均为正整数）
fn parse_u64(v: &str) -> u64 {
    let mut acc: u64 = 0;
    for ch in v.chars() {
        if ch >= '0' && ch <= '9' {
            acc = acc.wrapping_mul(10).wrapping_add((ch as u64) - ('0' as u64));
        } else {
            return u64::MAX;
        }
    }
    acc
}

// ============================================================
// #[cfg(test)] 单元测试（checks 自带回归）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vead11_all_checks_green() {
        let s = run_vead11_checks();
        let (passed, failed) = s.tally();
        let mut di = 0;
        while di < s.len() {
            if let Some(it) = s.get(di) {
                if !it.passed {
                    println!("RED #{}: {} / {}", di, it.name, it.detail);
                }
            }
            di += 1;
        }
        assert_eq!(failed, 0, "failed={} passed={}", failed, passed);
        assert!(!s.truncated(), "checkset truncated");
        assert_eq!(passed, 20, "expect 20 checks, got {}", passed);
    }

    #[test]
    fn vead11_seal_stable() {
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        let d1 = DocGen::generate(&facts, &g).unwrap_or_else(|_| DocModel {
            schema_ver: 0,
            facts_ver: 0,
            sections: Vec::new(),
            active_gen: 0,
        });
        let d2 = DocGen::generate(&facts, &g).unwrap_or_else(|_| DocModel {
            schema_ver: 0,
            facts_ver: 0,
            sections: Vec::new(),
            active_gen: 0,
        });
        assert_eq!(d1.seal(), d2.seal(), "同源生成封印必须稳定");
    }

    #[test]
    fn vead11_capture_matrix_monotonic() {
        for kind in PayloadKind::all().iter() {
            let mut seen = false;
            for g in DefenseGen::all().iter() {
                let cur = detect(*g, *kind);
                if seen {
                    assert!(cur, "捕获面必须单调不减: {:?} @ {:?}", kind, g);
                }
                if cur {
                    seen = true;
                }
            }
        }
    }

    #[test]
    fn vead11_purge_never_touches_alive() {
        let mut lib = NanRegressLibrary::new();
        let _ = lib.register_sample(PayloadKind::QuietNaN, PayloadKind::QuietNaN.canonical_bits(), 1, DefenseGen::Gen4SourceClamp, 10);
        // Alive 样本强制清理也不动
        assert!(lib.auto_purge(true).unwrap_or(999) == 0);
        assert!(lib.stats().1 == 1);
    }

    #[test]
    fn vead11_export_schema_keys_stable() {
        let lib = NanRegressLibrary::new();
        let facts = SourceFacts::canonical();
        let g = Glossary::new();
        let doc = DocGen::generate(&facts, &g).unwrap_or_else(|_| DocModel {
            schema_ver: 0,
            facts_ver: 0,
            sections: Vec::new(),
            active_gen: 0,
        });
        let kv = export_schema(&doc, &lib);
        assert!(kv.len() >= 9);
        assert!(kv[0].0 == "doc.schema_ver");
        assert!(kv[0].1 == "1");
    }
}
