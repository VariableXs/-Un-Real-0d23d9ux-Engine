//! VE-F6201 自检 · 输入域总架构（AE 域）
//!
//! **锚点判据逐条对应**（`#VE-F6201`「四层架构、输入如呼吸、三律、通道
//! 承诺、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 四层架构 | `AE01-架构-*`（层序/依赖方向 16 对独立重算/注册+耦合拦截+越权审计） |
//! | 输入如呼吸 | `AE01-通道-*`（零丢帧账：dropped 恒 0 判据钉死+三账守恒+红项可达） |
//! | 三律 | `AE01-三律-*`（声明闭集+判定码与词典第十章同源+违例整改闭环+违规史不可抹） |
//! | 通道承诺 | `AE01-通道-*`（同上——承诺=dropped 恒 0 的可机检面） |
//! | 判据 | `AE01-判据-*`（版本/错误码/同源契约位/读屏/条数对账） |
//!
//! **判据设计硬规矩**（承 AB/P 域先例）：期望值判据侧独立重算；不变量两头
//! 都测（违规被拒+合规放行）；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::veae01_inputarch as ia;

// ---------------------------------------------------------------------------
// 组一：四层架构
// ---------------------------------------------------------------------------

fn chk_arch(s: &mut CheckSet) {
    // AE01-架构-01：四层全集序号连续（0..=3 无空洞——归位链/依赖方向的地基）。
    let all = ia::InLayer::all();
    let ok = all.len() == ia::LAYER_COUNT
        && all[0].ordinal() == 0
        && all[1].ordinal() == 1
        && all[2].ordinal() == 2
        && all[3].ordinal() == 3;
    s.add("AE01-架构-01", ok, "四层序号连续无空洞");

    // AE01-架构-02：层短码/中文名非空互异（读屏可区分）。
    let mut ok = true;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            ok = ok && all[i].tag() != all[j].tag() && all[i].label() != all[j].label();
        }
    }
    ok = ok && all.iter().all(|l| !l.tag().is_empty() && !l.label().is_empty());
    s.add("AE01-架构-02", ok, "层码与层名非空互异");

    // AE01-架构-03：依赖方向 16 对独立重算（上层>下层真，其余假）。
    let mut ok = true;
    for a in all.iter() {
        for b in all.iter() {
            let expect = a.ordinal() > b.ordinal();
            ok = ok && a.may_depend_on(*b) == expect;
        }
    }
    s.add("AE01-架构-03", ok, "may_depend_on 全 16 对与层号定义一致");

    // AE01-架构-04：合法模块注册放行（依赖只含严格下层）。
    let mut r = ia::Registry::new();
    let m1 = ia::ArchModule {
        name: "event-bus",
        layer: ia::InLayer::Event,
        deps: vec_dep(&[ia::InLayer::Device]),
    };
    let ok = r.register(m1).is_ok() && r.len() == 1;
    s.add("AE01-架构-04", ok, "合法注册放行（deps 全为严格下层）");

    // AE01-架构-05：耦合违规拒绝（deps 含自身/上层=架构违例）。
    let bad1 = ia::ArchModule {
        name: "bad-self",
        layer: ia::InLayer::Mapping,
        deps: vec_dep(&[ia::InLayer::Mapping]),
    };
    let bad2 = ia::ArchModule {
        name: "bad-up",
        layer: ia::InLayer::Device,
        deps: vec_dep(&[ia::InLayer::Consume]),
    };
    let ok = r.register(bad1).is_err() && r.register(bad2).is_err() && r.len() == 1;
    s.add("AE01-架构-05", ok, "依赖自身/依赖上层均拒绝（耦合违规红线）");

    // AE01-架构-06：耦合错误码专属 + 越权审计入账（拒绝+记账双动作）。
    let e1 = r.register(ia::ArchModule {
        name: "bad-self2",
        layer: ia::InLayer::Mapping,
        deps: vec_dep(&[ia::InLayer::Mapping]),
    });
    let ok = matches!(e1, Err(c) if c == ia::E_COUPLE) && r.audit_count() == 3;
    s.add("AE01-架构-06", ok, "耦合拒绝码专属且审计计数 3（每次越权留痕）");

    // AE01-架构-07：重复注册拒绝（E_SQUAT——注册越权→审计）。
    let dup = ia::ArchModule {
        name: "event-bus",
        layer: ia::InLayer::Event,
        deps: vec_dep(&[]),
    };
    let ok = r.register(dup).is_err()
        && r.audit_count() == 4
        && r.count_of(ia::InLayer::Event) == 1;
    s.add("AE01-架构-07", ok, "重复模块名拒绝+审计（表未被污染）");

    // AE01-架构-08：分层计数（架构图读屏的分层数据面）。
    let _ = r.register(ia::ArchModule {
        name: "mapping-a",
        layer: ia::InLayer::Mapping,
        deps: vec_dep(&[ia::InLayer::Event, ia::InLayer::Device]),
    });
    let ok = r.count_of(ia::InLayer::Event) == 1 && r.count_of(ia::InLayer::Mapping) == 1
        && r.count_of(ia::InLayer::Consume) == 0;
    s.add("AE01-架构-08", ok, "分层计数与注册一致（含空层为 0）");
}

/// 辅助：从层数组构造 deps（判据侧独立构造，不走被测路径）。
fn vec_dep(ls: &[ia::InLayer]) -> alloc::vec::Vec<ia::InLayer> {
    let mut v = alloc::vec::Vec::new();
    for l in ls.iter() {
        v.push(*l);
    }
    v
}

// ---------------------------------------------------------------------------
// 组二：输入三律
// ---------------------------------------------------------------------------

fn chk_laws(s: &mut CheckSet) {
    // AE01-三律-01：三律声明全（闭集逐一）。
    let mut b = ia::LawBoard::new();
    let mut ok = true;
    for l in ia::InputLaw::all().iter() {
        ok = ok && b.declare(*l).is_ok();
    }
    ok = ok && b.fully_declared();
    s.add("AE01-三律-01", ok, "三律逐一声明后 fully_declared");

    // AE01-三律-02：重复声明拒绝。
    let ok = b.declare(ia::InputLaw::Reachable).is_err();
    s.add("AE01-三律-02", ok, "重复声明拒绝（三律各一次）");

    // AE01-三律-03：判定码双向可走（code/of_code 往返一致）。
    let mut ok = true;
    for l in ia::InputLaw::all().iter() {
        ok = ok && ia::InputLaw::of_code(l.code()) == Some(*l);
    }
    ok = ok && ia::InputLaw::of_code(0).is_none() && ia::InputLaw::of_code(4).is_none();
    s.add("AE01-三律-03", ok, "判定码往返一致+界外 None（同源编号可互查）");

    // AE01-三律-04：判定码恰为 1..=3（词典第十章同源编号体系的契约值）。
    let ok = ia::InputLaw::Reachable.code() == 1
        && ia::InputLaw::Consistent.code() == 2
        && ia::InputLaw::Rebindable.code() == 3;
    s.add("AE01-三律-04", ok, "三律判定码 1/2/3 钉死（词典同源契约值）");

    // AE01-三律-05：同源同判声明与词典章号钉死（前向契约位）。
    let ok = ia::U_DICT_CHAPTER == 10 && !ia::U_DICT_SAME_JUDGE.is_empty();
    s.add("AE01-三律-05", ok, "词典第十章同源声明钉死（不复制口径）");

    // AE01-三律-06：违规单立案（违例→整改闭环入口）。
    let mut b2 = ia::LawBoard::new();
    let _ = b2.declare(ia::InputLaw::Reachable);
    let _ = b2.declare(ia::InputLaw::Consistent);
    let _ = b2.declare(ia::InputLaw::Rebindable);
    b2.violate(ia::InputLaw::Reachable, "菜单未提供键盘触达");
    let ok = b2.open_violations() == 1 && !b2.clean();
    s.add("AE01-三律-06", ok, "违规立案后 clean 变假（违例不静默）");

    // AE01-三律-07：整改闭环（整改后 clean 恢复真——违规史保留）。
    let ok = b2.remediate(ia::InputLaw::Reachable).is_ok()
        && b2.open_violations() == 0
        && b2.clean()
        && b2.total_violations() == 1;
    s.add("AE01-三律-07", ok, "整改后 clean 恢复且违规史在册（史不可抹）");

    // AE01-三律-08：无未整改违规时整改拒绝（账实相符）。
    let ok = b2.remediate(ia::InputLaw::Reachable).is_err();
    s.add("AE01-三律-08", ok, "无单可改拒绝（不虚构整改）");

    // AE01-三律-09：分律整改（只清目标律，他律违规不动）。
    b2.violate(ia::InputLaw::Consistent, "同操作两处响应不一");
    b2.violate(ia::InputLaw::Rebindable, "某映射不可改");
    let _ = b2.remediate(ia::InputLaw::Consistent);
    let ok = b2.open_violations() == 1
        && b2
            .total_violations()
            .checked_sub(3)
            .map_or(false, |n| n >= 0);
    s.add("AE01-三律-09", ok, "分律整改隔离（只清目标律最早一条）");
}

// ---------------------------------------------------------------------------
// 组三：通道承诺（卡一帧=假一帧）
// ---------------------------------------------------------------------------

fn chk_channel(s: &mut CheckSet) {
    // AE01-通道-01：三态入账+总账守恒（handled+queued+dropped=总帧）。
    let mut led = ia::ChannelLedger::new();
    led.account(ia::FrameOutcome::Handled);
    led.account(ia::FrameOutcome::Handled);
    led.account(ia::FrameOutcome::Queued);
    let ok = led.handled == 2 && led.queued == 1 && led.total_frames() == 3;
    s.add("AE01-通道-01", ok, "三态入账+三账守恒（不丢不重）");

    // AE01-通道-02：承诺判定（零丢帧=promise holds——合规侧）。
    let ok = led.promise_holds();
    s.add("AE01-通道-02", ok, "零丢帧通道承诺成立（dropped=0）");

    // AE01-通道-03：丢帧打红（违例可观测——红项可达，防恒真门禁）。
    led.account(ia::FrameOutcome::Dropped);
    let ok = !led.promise_holds()
        && ia::E_CHANNEL_DROP == "E_CHANNEL_DROP"
        && led.total_frames() == 4;
    s.add("AE01-通道-03", ok, "一帧丢失即承诺破（红项可达非恒真）");

    // AE01-通道-04：空账承诺成立（基线非零先行——先有帧再谈承诺）。
    let empty = ia::ChannelLedger::new();
    let ok = empty.promise_holds() && empty.total_frames() == 0;
    s.add("AE01-通道-04", ok, "空账零丢帧（承诺的初始态）");
}

// ---------------------------------------------------------------------------
// 组四：读屏 + 判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // AE01-判据-01：架构读屏单行含四层与审计数。
    let mut r = ia::Registry::new();
    let _ = r.register(ia::ArchModule {
        name: "m1",
        layer: ia::InLayer::Device,
        deps: vec_dep(&[]),
    });
    let line = ia::screen_line_arch(&r);
    let ok = line.contains("设备抽象层") && line.contains("消费层") && line.contains("1");
    s.add("AE01-判据-01", ok, "架构读屏单行（四层+在册数——架构图读屏替代）");

    // AE01-判据-02：三律读屏单行含章号与整改态。
    let b = ia::LawBoard::new();
    let line = ia::screen_line_laws(&b);
    let ok = line.contains("10") && line.contains("未整改");
    s.add("AE01-判据-02", ok, "三律读屏单行（同源章号+整改态）");

    // AE01-判据-03：AD10 承接契约位钉死。
    s.add(
        "AE01-判据-03",
        !ia::AD10_HANDOVER.is_empty() && ia::AD10_HANDOVER.starts_with("AD10"),
        "AD10 移交承接位钉死（上游不漏接）",
    );

    // AE01-判据-04：协议版本与域标识。
    s.add(
        "AE01-判据-04",
        ia::INPUTARCH_VERSION.starts_with("AE01") && ia::INPUT_DOMAIN == "VE-AE",
        "协议版本 AE01-* 与域标识钉死",
    );

    // AE01-判据-05：错误码非空互异。
    s.add(
        "AE01-判据-05",
        !ia::E_COUPLE.is_empty()
            && !ia::E_SQUAT.is_empty()
            && !ia::E_LAW_OPEN.is_empty()
            && !ia::E_CHANNEL_DROP.is_empty()
            && ia::E_COUPLE != ia::E_SQUAT
            && ia::E_LAW_OPEN != ia::E_CHANNEL_DROP
            && ia::E_COUPLE != ia::E_CHANNEL_DROP,
        "错误码非空互异（外部可观测分支）",
    );

    // AE01-判据-06：判据条数对账（本条前已有 26 条，本条为第 27 条）。
    s.add("AE01-判据-06", s.len() == 26, "判据条数对账（声明 27）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 26 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F6201 域自检（聚合入口，注册表用）。
pub fn run_veae01_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6201");
    chk_arch(&mut s);
    chk_laws(&mut s);
    chk_channel(&mut s);
    chk_meta(&mut s);
    s
}
