//! VE-F1401 · 域自检（判据逐条对应，见 `veh01_boundary.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **ADR 落册**（四主题各一条、无 ADR 不开工） → `H01-ADR-四主题落册`
//! - **ADR 落册**（重复落册拒绝不覆盖） → `H01-ADR-重复落册拒绝`
//! - **ADR 落册**（处置与共享核矛盾拒绝） → `H01-ADR-处置冲突拒绝`
//! - **四主题差异化**（集成 vs 本体视角文本可机检） → `H01-差异-视角文本机检`
//! - **四主题差异化**（两侧保留面不重合 + 四主题全覆盖） → `H01-差异-保留面不重合`
//! - **复用声明**（DSP 核共享同句柄 ⇒ 同核同对拍） → `H01-复用-共享核同句柄`
//! - **复用声明**（服务层各自独立、混层被拒） → `H01-复用-服务层不混层`
//! - **复用声明**（共享核面双侧并存合法 = 例外声明） → `H01-复用-共享核面例外`
//! - **查重对账**（域内零重复 + 报告渲染） → `H01-对账-零重复与报告`
//! - **查重对账**（重复 API 面显性拒绝入账） → `H01-对账-重复面拒绝`
//! - **查重对账**（未处置红项阻塞开工） → `H01-闸门-红项阻塞开工`
//! - **查重对账**（合规契约下四类开工全放行） → `H01-闸门-合规全放行`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::veh01_boundary::*;
use crate::checks::CheckSet;

/// 四主题规范 ADR（F1381 声明 + F1397 处置清单的机读形态）。
fn canonical_adrs() -> [AdrRecord; 4] {
    [
        AdrRecord {
            theme: OverlapTheme::Mixing,
            disposition: Disposition::HLead,
            g_surface: "播放总线（16 总线固定拓扑，面向媒体播放）",
            h_surface: "通用混音图（任意 DAG，面向多消费者引擎）",
            view_g: "集成视角：把已解码媒体分派到播放总线",
            view_h: "本体视角：为任意消费者图提供混音节点与路由",
            shared_core: Some(SharedCore::Biquad),
            state: AdrState::Filed,
        },
        AdrRecord {
            theme: OverlapTheme::Hrtf,
            disposition: Disposition::HLead,
            g_surface: "双耳播放（对已解码媒体做 HRIR 渲染）",
            h_surface: "世界空间引擎（listener/emitter 位置驱动渲染）",
            view_g: "集成视角：媒体内容自带的听觉呈现",
            view_h: "本体视角：三维场景中声源有位置的世界空间渲染",
            shared_core: Some(SharedCore::Convolution),
            state: AdrState::Filed,
        },
        AdrRecord {
            theme: OverlapTheme::Resample,
            disposition: Disposition::MergedByAdr,
            g_surface: "播放链重采样档位（五档，服务于播放缓冲）",
            h_surface: "统一重采样服务（离线批量 + 实时流式同核）",
            view_g: "集成视角：播放链按档位取重采样",
            view_h: "本体视角：向全部消费者供给同一重采样核",
            shared_core: Some(SharedCore::Fft),
            state: AdrState::Filed,
        },
        AdrRecord {
            theme: OverlapTheme::Loudness,
            disposition: Disposition::HLead,
            g_surface: "播放时响度对齐（单文件播放链测量与对齐）",
            h_surface: "批量响度归一服务（媒体库级扫描与归一）",
            view_g: "集成视角：当前播放这一路的对齐",
            view_h: "本体视角：全库批量的 LUFS 分析与归一",
            shared_core: Some(SharedCore::Biquad),
            state: AdrState::Filed,
        },
    ]
}

/// 落齐四主题 ADR 并全部接受的合规执行器。
fn compliant_contract() -> BoundaryContract {
    let mut c = BoundaryContract::new();
    c.tick();
    for rec in canonical_adrs() {
        let theme = rec.theme;
        let _ = c.adrs_mut().file(rec);
        let _ = c.adrs_mut().accept(theme);
    }
    c
}

/// 按差异化定位铺满四主题两侧能力面声明（每主题 G/H 各一条不重合 API 面）。
fn declare_differentiated(d: &mut DedupLedger) -> bool {
    let rows = [
        (OverlapTheme::Mixing, "playback_bus", "mix_graph"),
        (OverlapTheme::Hrtf, "binaural_playback", "spatial_engine"),
        (
            OverlapTheme::Resample,
            "playback_resample_tier",
            "resample_service",
        ),
        (OverlapTheme::Loudness, "playback_align", "loudness_batch"),
    ];
    let mut ok = true;
    for (theme, g_sym, h_sym) in rows {
        for (side, sym) in [(DomainSide::G07, g_sym), (DomainSide::H, h_sym)] {
            ok &= d
                .declare(SurfaceClaim {
                    theme,
                    kind: SurfaceKind::ApiEntry,
                    side,
                    symbol: sym,
                })
                .is_ok();
        }
    }
    ok
}

/// VE-F1401 域自检。
pub fn run_veh01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh01");

    // ---- ADR 落册：四主题各一条 + 无 ADR 不开工 ----

    {
        let c = compliant_contract();
        let all_filed = OverlapTheme::ALL
            .iter()
            .all(|t| c.adrs().adr_of(*t).is_some());
        let accepted = c.adrs().accepted_count() == 4;
        let unaccepted_empty = c.adrs().unaccepted_themes().is_empty();
        // 零 ADR 契约下四类开工全拒（无 ADR 不开工的执行面）。
        let mut bare = BoundaryContract::new();
        let all_blocked = [WorkKind::Session, WorkKind::Graph, WorkKind::Spatial, WorkKind::Asset]
            .iter()
            .all(|k| bare.gate_h_work(*k).is_err());
        let logged = bare
            .adrs()
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_NO_ADR");
        let doc = BOUNDARY_CONTRACT_DOC.contains("四主题边界契约")
            && NO_ADR_NO_WORK_DOC.contains("无 ADR 不开工");
        set.add(
            "H01-ADR-四主题落册",
            all_filed && accepted && unaccepted_empty && all_blocked && logged && doc,
            "",
        );
    }

    // ---- ADR 落册：重复落册拒绝且不覆盖既有 ----

    {
        let mut c = compliant_contract();
        let dup = c.adrs_mut().file(canonical_adrs()[0]).is_err();
        let logged = c
            .adrs()
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_ADR_DUPLICATE");
        let intact = c.adrs().adr_of(OverlapTheme::Mixing).unwrap().disposition
            == Disposition::HLead;
        set.add("H01-ADR-重复落册拒绝", dup && logged && intact, "");
    }

    // ---- ADR 落册：处置与共享核声明矛盾拒绝 ----

    {
        let mut c = BoundaryContract::new();
        let mut bad = canonical_adrs()[0];
        bad.disposition = Disposition::GKeep;
        bad.shared_core = Some(SharedCore::Biquad);
        let rejected = c.adrs_mut().file(bad).is_err();
        let logged = c
            .adrs()
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_DISPOSITION_CONFLICT");
        // 未落册（矛盾记录不进账）。
        let not_filed = c.adrs().adr_of(OverlapTheme::Mixing).is_none();
        set.add(
            "H01-ADR-处置冲突拒绝",
            rejected && logged && not_filed,
            "",
        );
    }

    // ---- 四主题差异化：视角文本机检 ----

    {
        let c = compliant_contract();
        let distinct_all = OverlapTheme::ALL
            .iter()
            .all(|t| c.adrs().view_distinct(*t));
        // 反例：两侧视角文本雷同 ⇒ 差异化不成立。
        let mut c2 = BoundaryContract::new();
        let mut same = canonical_adrs()[1];
        same.view_g = "同一句话";
        same.view_h = "同一句话";
        let _ = c2.adrs_mut().file(same);
        let caught = !c2.adrs().view_distinct(OverlapTheme::Hrtf);
        set.add("H01-差异-视角文本机检", distinct_all && caught, "");
    }

    // ---- 四主题差异化：两侧保留面不重合 + 四主题全覆盖 ----

    {
        let c = compliant_contract();
        let mut no_overlap = true;
        for t in OverlapTheme::ALL {
            let a = c.adrs().adr_of(t).unwrap();
            no_overlap &= a.g_surface != a.h_surface && !a.g_surface.is_empty() && !a.h_surface.is_empty();
        }
        // 四主题处置齐备（混音/HRTF/响度=H 主导，重采样=合并 ADR）。
        let disp_ok = c.adrs().adr_of(OverlapTheme::Mixing).unwrap().disposition
            == Disposition::HLead
            && c.adrs().adr_of(OverlapTheme::Hrtf).unwrap().disposition
                == Disposition::HLead
            && c.adrs().adr_of(OverlapTheme::Resample).unwrap().disposition
                == Disposition::MergedByAdr
            && c.adrs().adr_of(OverlapTheme::Loudness).unwrap().disposition
                == Disposition::HLead;
        set.add("H01-差异-保留面不重合", no_overlap && disp_ok, "");
    }

    // ---- 复用声明：共享核同句柄（同核同对拍） ----

    {
        let mut cores = CoreRegistry::new();
        // 两域对同一核取用 ⇒ 句柄相同。
        let same_handle = cores.open(SharedCore::Convolution, DomainSide::H)
            == cores.open(SharedCore::Convolution, DomainSide::G07);
        let no_break = cores.parity_breaks().is_empty();
        let two_consumers = cores.consumers_of(SharedCore::Convolution).len() == 2;
        // 句柄纯函数：同核必同句柄，不同核必不同句柄。
        let pure = SharedCore::Biquad.handle() == SharedCore::Biquad.handle()
            && SharedCore::Biquad.handle() != SharedCore::Fft.handle()
            && SharedCore::Fft.handle() != SharedCore::Convolution.handle();
        // 算法版本随句柄冻结（对拍可复现）。
        let versioned = SharedCore::Biquad.algo_version() == "biquad/v1"
            && SharedCore::Fft.algo_version() == "fft/v1"
            && SharedCore::Convolution.algo_version() == "conv/v1";
        let doc = REUSE_DECLARATION_DOC.contains("同核同对拍");
        set.add(
            "H01-复用-共享核同句柄",
            same_handle && no_break && two_consumers && pure && versioned && doc,
            "",
        );
    }

    // ---- 复用声明：服务层各自独立、混层被拒 ----

    {
        let mut lr = LayerRegistry::new();
        // 各归各层合法。
        let ok_g = lr
            .claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::G07, "bus_route")
            .is_ok();
        let ok_h = lr
            .claim(ServiceLayer::EngineScheduling, DomainSide::H, "engine_sched")
            .is_ok();
        // 跨层声明拒绝（H 域不得声明 G07 的层）。
        let cross_blocked = lr
            .claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::H, "engine_sched")
            .is_err();
        // 同域同层同符号幂等（不算污染）。
        let idempotent = lr
            .claim(ServiceLayer::EngineScheduling, DomainSide::H, "engine_sched")
            .is_ok();
        let clean = lr.mixing_violations().is_empty();
        // 检测面可验证：演练注入撞车声明必须被捕获。
        lr.force_claim_for_drill(ServiceClaim {
            layer: ServiceLayer::EngineScheduling,
            side: DomainSide::G07,
            symbol: "engine_sched",
        });
        let caught = lr
            .mixing_violations()
            .iter()
            .any(|s| s.contains("不可跨域共享"));
        let doc = REUSE_DECLARATION_DOC.contains("不混层");
        set.add(
            "H01-复用-服务层不混层",
            ok_g && ok_h && cross_blocked && idempotent && clean && caught && doc,
            "",
        );
    }

    // ---- 复用声明：共享核面双侧并存合法（复用边界例外） ----

    {
        let mut d = DedupLedger::new();
        d.tick();
        let both_ok = [DomainSide::G07, DomainSide::H]
            .iter()
            .all(|side| {
                d.declare(SurfaceClaim {
                    theme: OverlapTheme::Resample,
                    kind: SurfaceKind::SharedKernel,
                    side: *side,
                    symbol: "resample_core",
                })
                .is_ok()
            });
        let (dups, _) = d.reconcile();
        // 共享核面声明计入报告的取用条数（对拍证据）。
        let rep = d.report();
        set.add(
            "H01-复用-共享核面例外",
            both_ok && dups == 0 && rep.shared_kernel_takes == 2,
            "",
        );
    }

    // ---- 查重对账：域内零重复 + 报告渲染 ----

    {
        let c = compliant_contract();
        let mut d = DedupLedger::new();
        d.tick();
        let declared = declare_differentiated(&mut d);
        let (dups, viols) = d.reconcile();
        let rep = d.report();
        let machine = rep.themes_covered == 4
            && rep.duplicates == 0
            && rep.claims_total == 8
            && rep.verdict_clean;
        let text = d.render_report(c.adrs());
        let rendered = text.contains("四主题查重对账报告")
            && text.contains("域内零重复")
            && text.contains("H 主导");
        set.add(
            "H01-对账-零重复与报告",
            declared && dups == 0 && viols.is_empty() && machine && rendered,
            "",
        );
    }

    // ---- 查重对账：重复 API 面显性拒绝入账 ----

    {
        let mut d = DedupLedger::new();
        d.tick();
        let first = d
            .declare(SurfaceClaim {
                theme: OverlapTheme::Mixing,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::G07,
                symbol: "mix_bus_api",
            })
            .is_ok();
        let dup_rejected = d
            .declare(SurfaceClaim {
                theme: OverlapTheme::Mixing,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::H,
                symbol: "mix_bus_api",
            })
            .is_err();
        let logged = d
            .findings()
            .iter()
            .any(|(_, code, _)| *code == "E_SURFACE_DUPLICATE");
        // 不同主题同符号不判重复（主题是正交维度）。
        let mut d2 = DedupLedger::new();
        let _ = d2.declare(SurfaceClaim {
            theme: OverlapTheme::Mixing,
            kind: SurfaceKind::ApiEntry,
            side: DomainSide::G07,
            symbol: "shared_name",
        });
        let other_theme_ok = d2
            .declare(SurfaceClaim {
                theme: OverlapTheme::Loudness,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::H,
                symbol: "shared_name",
            })
            .is_ok();
        set.add(
            "H01-对账-重复面拒绝",
            first && dup_rejected && logged && other_theme_ok,
            "",
        );
    }

    // ---- 闸门：未处置红项阻塞开工 ----

    {
        let mut c = compliant_contract();
        let _ = declare_differentiated(c.dedup_mut());
        assert!(c.dedup().open_red_items() == 0, "差异化声明下应无红项");
        // 注入一条红项：同主题同符号 API 面双侧声明（被 declare 闸门拒绝）。
        let injected_rejected = c
            .dedup_mut()
            .declare(SurfaceClaim {
                theme: OverlapTheme::Loudness,
                kind: SurfaceKind::ApiEntry,
                side: DomainSide::H,
                symbol: "playback_align",
            })
            .is_err();
        assert!(injected_rejected, "对侧同符号声明必须被拒绝");
        assert!(c.dedup().open_red_items() > 0, "红项应被记账");
        let blocked = c.gate_h_work(WorkKind::Asset).is_err();
        let logged = c
            .adrs()
            .errors()
            .iter()
            .any(|(_, code, _)| *code == "E_DEDUP_OPEN");
        set.add("H01-闸门-红项阻塞开工", blocked && logged, "");
    }

    // ---- 闸门：合规契约下四类开工全放行 ----

    {
        let mut c = compliant_contract();
        // 共享核两域同取 + 服务层各归各位 + 能力面差异化。
        let _ = c.cores_mut().open(SharedCore::Biquad, DomainSide::H);
        let _ = c.cores_mut().open(SharedCore::Biquad, DomainSide::G07);
        let _ = c
            .layers_mut()
            .claim(ServiceLayer::PlaybackChainOrchestration, DomainSide::G07, "bus");
        let _ = c
            .layers_mut()
            .claim(ServiceLayer::EngineScheduling, DomainSide::H, "sched");
        let declared = declare_differentiated(c.dedup_mut());
        let kinds = [
            WorkKind::Session,
            WorkKind::Graph,
            WorkKind::Spatial,
            WorkKind::Asset,
        ];
        let all_granted = kinds.iter().all(|k| c.gate_h_work(*k).is_ok());
        let granted_log = c.grants().len() == 4
            && c.grants().iter().all(|g| g.contains("四主题 ADR 齐备"));
        let parity_ok = c.cores().parity_breaks().is_empty();
        let layers_ok = c.layers().mixing_violations().is_empty();
        set.add(
            "H01-闸门-合规全放行",
            declared && all_granted && granted_log && parity_ok && layers_ok,
            "",
        );
    }

    set
}