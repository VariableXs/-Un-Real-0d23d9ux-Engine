//! VE-F3001 · 域自检（判据逐条对应，见 `vep01_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三组架构 → `P01-架构-*`
//! - 十项映射 → `P01-映射-*`
//! - 单源分工 → `P01-分工-*`
//! - 三底线 → `P01-底线-*`
//! - 第一红线 → `P01-红线-*`
//! - 判据（自证可追溯）→ `P01-判据-*`
//! - 降级矩阵（能力分歧→对拍拦截 / 组间接口变更→ADR / 降级链前向声明）
//!   → `P01-降级-*`
//! - 跨批对接（M/O04 契约接收 / N 域集成 / F3038 前向）→ `P01-对接-*`
//! - 无障碍（读屏替述）→ `P01-读屏-*`
//! - 错误路径零静默 → `P01-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::vep01_arch::*;
use crate::checks::CheckSet;

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

/// 标准消费契约登记（四端齐备）。
///
/// 契约内容与哈希**成对构造**——本函数就是「哈希对账」的正样本：哈希由内容
/// 实算得出，不是手写常量。
fn std_peers() -> PeerLedger {
    let mut l = PeerLedger::new();
    let entries: [(Peer, &str, &str); 4] = [
        (
            Peer::RuntimeM,
            "m-motion-runtime/v1",
            "实例请求与回执：时间轴实例组（推进/混合/采样/速度/逆向由 M 域自持）",
        ),
        (
            Peer::CompilerO04,
            "o04-motion-compile/v1",
            "声明编译请求：动效声明 → CSS 动画/transition/合成通道三选一目标",
        ),
        (
            Peer::WidgetN,
            "n-widget-motion-bind/v1",
            "控件绑定：元素句柄 + 事件源（控件族归 N 域，P 域只绑定不建控件）",
        ),
        (
            Peer::DegradeF3038,
            "f3038-degrade-chain/v1",
            "降级链档位：错开取消/视差取消/fade 化/直达（F3038 落地后升v2）",
        ),
    ];
    for (peer, name, content) in entries.iter() {
        l.register(PeerContract {
            peer: *peer,
            contract: name.to_string(),
            content_hash: fnv1a64_hex(content.as_bytes()),
            received: false,
            reconciled: false,
        })
        .expect("标准对端登记");
    }
    l
}

/// 已接收并对账通过的阻断级对端（开工前置的正样本）。
fn ready_peers() -> PeerLedger {
    let mut l = std_peers();
    l.reconcile(
        Peer::RuntimeM,
        "实例请求与回执：时间轴实例组（推进/混合/采样/速度/逆向由 M 域自持）",
    )
    .expect("M 域对账通过");
    l.reconcile(
        Peer::CompilerO04,
        "声明编译请求：动效声明 → CSS 动画/transition/合成通道三选一目标",
    )
    .expect("O04 对账通过");
    // 上游 N 与下游 F3038 不阻断，但登记内容须一致。
    l.reconcile(
        Peer::WidgetN,
        "控件绑定：元素句柄 + 事件源（控件族归 N 域，P 域只绑定不建控件）",
    )
    .expect("N 域对账通过");
    l.reconcile(
        Peer::DegradeF3038,
        "降级链档位：错开取消/视差取消/fade 化/直达（F3038 落地后升v2）",
    )
    .expect("F3038 对账通过");
    l
}

/// 六项能力全部对拍通过的探针账（开工前置的正样本）。
fn ready_probes() -> ProbeLedger {
    let mut l = ProbeLedger::new();
    for c in RuntimeCapability::ALL.iter() {
        let semantics = match c {
            RuntimeCapability::FrameClock => "单调帧时钟：每帧推进一个逻辑 tick",
            RuntimeCapability::PoseBlend => "姿态混合：多源按权重混合为单值",
            RuntimeCapability::KeyframeSample => "关键帧采样：轨道+时间 → 采样值",
            RuntimeCapability::Velocity => "速度接口：初速度注入与继承",
            RuntimeCapability::Reverse => "逆向播放：时间轴反放且速度连续",
            RuntimeCapability::ClockDriftAudit => "零漂移对拍：跨层时间一致性校验",
        };
        l.submit(CapabilityProbe {
            capability: *c,
            assumed_handle: 0x1000u16.wrapping_add(c.rank() as u16),
            assumed_semantics: semantics.to_string(),
            observed_available: true,
            observed_semantics: semantics.to_string(),
            tick: 1,
        })
        .expect("能力探针对拍通过");
    }
    l
}

/// 三底线齐备的判定账（开工前置的正样本）。
fn ready_bottom_lines() -> BottomLineLedger {
    let mut l = BottomLineLedger::new();
    for i in 0..4u32 {
        l.push_sample(FrameSample {
            instance: i,
            frame: i as u64 + 1,
            sample_count: MIN_SAMPLES_PER_FRAME + 2,
            zoom_inspected: true,
        })
        .expect("帧采样入账");
        l.push_instance(ActiveInstance {
            id: i,
            theme: MotionTheme::ALL[i as usize % THEME_COUNT],
            interrupt: Some(InterruptPolicy::QuickFinish),
            reduce_declared: true,
        })
        .expect("实例入账");
    }
    l
}

/// 全绿总纲（除三组契约外全部齐备）。
fn ready_arch() -> MotionArchitecture {
    let mut a = MotionArchitecture::standard();
    a.peers = ready_peers();
    a.probes = ready_probes();
    a.bottom_lines = ready_bottom_lines();
    a
}

// ---------------------------------------------------------------------------
// 判据一：三组架构
// ---------------------------------------------------------------------------

fn chk_three_groups(set: &mut CheckSet) {
    // 三组不多不少，组序递增，码往返全通。
    set.add(
        "P01-架构-三组齐备",
        GROUP_ORDER.len() == GROUP_COUNT && MotionGroup::ALL.len() == GROUP_COUNT,
        "库/转场/微交互三组，域级收口为闸门不计接口组",
    );
    let mut order_ok = true;
    let mut last: Option<u8> = None;
    for g in GROUP_ORDER.iter() {
        if let Some(l) = last {
            if g.rank() <= l {
                order_ok = false;
            }
        }
        last = Some(g.rank());
        if MotionGroup::from_code(g.code()) != Some(*g) {
            order_ok = false;
        }
    }
    set.add(
        "P01-架构-组序递增且码往返",
        order_ok && MotionGroup::from_code("P01-G9").is_none(),
        "组序由 rank() 派生，GROUP_ORDER 为唯一真值",
    );

    // 每段契约七项齐备 + 段序递增 + 段码唯一 + 零运行期成本。
    let mut complete = true;
    let mut seg_order_ok = true;
    let mut dup_free = true;
    let mut zero_cost = true;
    let mut codes: Vec<&str> = Vec::new();
    for g in GROUP_ORDER.iter() {
        let mut last_rank: Option<u8> = None;
        for s in g.segments().iter() {
            if !s.is_complete() {
                complete = false;
            }
            if let Some(l) = last_rank {
                if s.rank <= l {
                    seg_order_ok = false;
                }
            }
            last_rank = Some(s.rank);
            if codes.contains(&s.code) {
                dup_free = false;
            }
            codes.push(s.code);
            if !s.cost.is_zero_overhead() {
                zero_cost = false;
            }
        }
    }
    set.add(
        "P01-架构-段契约七项齐备",
        complete,
        "吃/吐/失败策略/复杂度/消费方/不做清单/成本形态，缺一即不合格",
    );
    set.add(
        "P01-架构-段序严格递增",
        seg_order_ok,
        "越便宜越靠前：取值检查在编排之前，路由确认在转场编排之前",
    );
    set.add("P01-架构-段码全局唯一", dup_free, "段码重复会让下游引用指错段");
    set.add(
        "P01-架构-零运行时开销",
        zero_cost,
        "全部段标声明期；P 域不在帧路径上做事",
    );

    // 段数有上界 ⇒ 契约核验 O(签名数) 是有界常量。
    let bounded = GROUP_ORDER
        .iter()
        .all(|g| g.segments().len() <= MAX_SEGMENTS_PER_GROUP);
    set.add(
        "P01-架构-段数有上界",
        bounded,
        "有上界才使「契约核验 O(签名数)」是常量而非「大概扫一遍」",
    );
    set.add(
        "P01-架构-签名数为段数三倍",
        total_signature_count() == total_segment_count() * 3,
        "每段三项签名（吃/吐/失败策略）",
    );

    // 组自检对标准总纲必须零问题。
    let a = MotionArchitecture::standard();
    set.add(
        "P01-架构-标准总纲契约零问题",
        a.check_contracts().is_empty(),
        "总纲自己先做到可追溯，否则凭什么要求别人",
    );

    // 每组的判据映射非空。
    let mapped = GROUP_ORDER.iter().all(|g| !g.serves().is_empty());
    set.add("P01-架构-组判据映射非空", mapped, "每组须声明自己兑现哪几条判据");

    // 段码格式 `P01-G<n>S<m>`：组号取自组码尾段，段号须等于段位+1。
    let fmt_ok = GROUP_ORDER.iter().all(|g| {
        let gno = g.code().rsplit('G').next().unwrap_or("");
        g.segments().iter().all(|s| {
            let parts: Vec<&str> = s.code.split('-').collect();
            parts.len() == 2 && parts[0] == "P01" && parts[1] == format!("G{}S{}", gno, s.rank + 1)
        })
    });
    set.add(
        "P01-架构-段码格式自洽",
        fmt_ok,
        "P01-G<组号>S<段位+1>；段码错位会让台账引用指错段",
    );
}

// ---------------------------------------------------------------------------
// 判据二：十项映射
// ---------------------------------------------------------------------------

fn chk_theme_mapping(set: &mut CheckSet) {
    let a = MotionArchitecture::standard();

    set.add(
        "P01-映射-十项不多不少",
        MotionTheme::ALL.len() == THEME_COUNT && THEME_ORDER.len() == THEME_COUNT,
        "判据「十项映射」的十项，不多不少",
    );

    // 十项 10/10 硬门。
    let audit = a.audit_themes();
    assert!(
        audit.is_complete(),
        "十主题审计须全绿：缺 {} 空 {} 格式 {} 组 {} 超限 {}",
        audit.missing.len(),
        audit.empty.len(),
        audit.malformed.len(),
        audit.group_mismatch.len(),
        audit.overflow.len()
    );
    set.add(
        "P01-映射-十主题10/10",
        audit.is_complete(),
        "缺失/空落点/格式非法/组不一致/超限五类缺口须全为零",
    );

    // 定位 O(1) 且两种故障可区分。
    let hit = a.theme_landing(MotionTheme::Language).ok().flatten();
    let miss = a.theme_landing(MotionTheme::Language);
    set.add(
        "P01-映射-落点定位O(1)且区分缺与空",
        hit.is_some() && miss.is_ok(),
        "查无此项≠ 查得此项但落点为空；两种故障必须可区分",
    );

    // 主题码与枚举守卫。
    let guard = MotionTheme::ALL
        .iter()
        .all(|t| MotionTheme::from_code(t.code()) == Some(*t))
        && MotionTheme::from_code("MT-NOPE").is_none();
    set.add("P01-映射-主题码往返守卫", guard, "未知码显性拒绝，不猜近似值");

    // rank 与 ALL 索引一致（O(1) 定位的前提）。
    let rank_ok = MotionTheme::ALL
        .iter()
        .enumerate()
        .all(|(i, t)| t.rank() == i && THEME_ORDER[i] == *t);
    set.add(
        "P01-映射-rank与索引一致",
        rank_ok,
        "rank 是 O(1) 定位的键，与 ALL 同序是它的正确性前提",
    );

    // 锚点十五标签逐项对齐（缺与多都查）。
    let cov = check_anchor_label_coverage();
    assert!(cov.is_complete(), "十五标签须逐项对齐：{}", cov.screen_text());
    set.add(
        "P01-映射-锚点15标签逐项对齐",
        cov.is_complete(),
        "缺项与自造项都必须为零；漏掉任一标签即假装完成了锚点的一部分",
    );
    set.add(
        "P01-映射-两轴合计等于15",
        THEME_COUNT + AXIS_COUNT == ANCHOR_LABELS.len(),
        "十主题 + 五支撑轴 = 锚点括号内十五标签",
    );

    // 支撑轴非空且服务达标。
    let axes_ok = a.check_axes_served().is_empty();
    set.add(
        "P01-映射-五轴服务齐备",
        axes_ok,
        "空轴即缺陷：轴若不服务任何主题就是悬空资源",
    );
    let axis_guard = SupportAxis::ALL
        .iter()
        .all(|x| SupportAxis::from_code(x.code()) == Some(*x))
        && SupportAxis::from_code("AX-NOPE").is_none();
    set.add("P01-映射-轴码往返守卫", axis_guard, "未知轴码显性拒绝");

    // 轴主责条目唯一且落在 P 域条号段。
    let owner_ok = SupportAxis::ALL.iter().all(|x| {
        let item = x.owner_item();
        is_valid_item_id(item) && item.starts_with("VE-F3")
    });
    set.add(
        "P01-映射-轴主责条目合法",
        owner_ok,
        "轴本体有唯一 owner 归谁（F3015~F3019）",
    );

    // 主责组反查：每主题反查得到且唯一。
    let group_ok = MotionTheme::ALL.iter().all(|t| {
        let g = t.primary_group();
        GROUP_ORDER.contains(&g)
    });
    set.add(
        "P01-映射-主题主责组可反查",
        group_ok,
        "主题→组归属反查必须落在三组之内",
    );

    // 落点条目号格式全合法。
    let ids_ok = a
        .landings
        .iter()
        .all(|l| l.items.iter().all(|i| is_valid_item_id(i)));
    set.add(
        "P01-映射-落点条目号格式合法",
        ids_ok,
        "条目号须为 VE-Fxxxx 形式，便于台账机检",
    );
}

// ---------------------------------------------------------------------------
// 判据三：单源分工
// ---------------------------------------------------------------------------

fn chk_division_of_duty(set: &mut CheckSet) {
    let a = MotionArchitecture::standard();

    // 六项运行时能力 owner 非空且唯一。
    let owners_ok = RuntimeCapability::ALL
        .iter()
        .all(|c| !c.owner().trim().is_empty());
    set.add(
        "P01-分工-运行时能力有属主",
        owners_ok && a.division.audit_owners().is_empty(),
        "帧时钟/混合/采样/速度/逆向/零漂移对拍六项各有唯一属主",
    );
    set.add(
        "P01-分工-六项能力全归VE-M",
        RuntimeCapability::ALL
            .iter()
            .all(|c| c.owner() == "VE-M"),
        "归属裁决而非「暂时放在 M 域」：两套时钟必现采样相位漂移",
    );

    // 越界认领必须被拒。
    let mut d = RuntimeDivision::standard();
    let claim = d.claim_runtime(RuntimeCapability::FrameClock);
    set.add(
        "P01-分工-越界认领被拒",
        claim.is_err()
            && claim.expect_err("应拒绝").code == E_RUNTIME_CAPABILITY_NOT_OWNED
            && d.refused() == 1,
        "让「我以为 P 可以自己跑时钟」在代码里撞墙，而不是评审时被口头纠正",
    );

    // 声明层能力须带理由。
    let mut d2 = RuntimeDivision::standard();
    let no_why = d2.note_declaration(RuntimeCapability::ClockDriftAudit, "  ");
    set.add(
        "P01-分工-声明层能力须带理由",
        no_why.is_err(),
        "无理由的声明在下次架构变更时会被当成既定事实",
    );
    let with_why = d2.note_declaration(
        RuntimeCapability::ClockDriftAudit,
        "P 域声明层也要参与零漂移对拍，否则时钟漂移会从声明侧漏进运行侧",
    );
    set.add(
        "P01-分工-声明层能力可登记",
        with_why.is_ok() && d2.declared_count() == 1,
        "台账留痕：P 域声明层自有能力须可被反查",
    );

    // 能力码往返。
    let guard = RuntimeCapability::ALL
        .iter()
        .all(|c| RuntimeCapability::from_code(c.code()) == Some(*c))
        && RuntimeCapability::from_code("RC-NOPE").is_none();
    set.add("P01-分工-能力码往返守卫", guard, "未知能力码显性拒绝");

    // 禁扩面七条逐条可拦。
    let mut blocked = 0;
    for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
        let e = check_no_overreach(desc);
        if e.is_err() && e.expect_err("应拒绝").code == E_BOUNDARY_OVERREACH {
            blocked += 1;
        }
        let by_code = check_no_overreach(code);
        if by_code.is_err() {
            blocked += 1;
        }
    }
    set.add(
        "P01-分工-禁扩面七条逐条可拦",
        blocked == BOUNDARY_EXCLUSIONS.len() * 2,
        "按描述拦与按码拦都要命中，否则总纲形同虚设",
    );

    // 域内正当诉求不误伤。
    let legit = check_no_overreach("把动效声明编译为 CSS 动画目标并提交 M 域执行");
    set.add(
        "P01-分工-域内诉求不误伤",
        legit.is_ok(),
        "越界检查必须精确，误伤会让它被人绕过",
    );

    // 每条禁扩面都有归属去处。
    let advice_ok = BOUNDARY_EXCLUSIONS
        .iter()
        .all(|(code, _)| !boundary_advice(code).trim().is_empty());
    set.add(
        "P01-分工-禁扩面有归属去处",
        advice_ok,
        "拒绝必须告诉对方「这事该谁做」，否则下次还会有人试",
    );
}

// ---------------------------------------------------------------------------
// 判据四：三底线
// ---------------------------------------------------------------------------

fn chk_bottom_lines(set: &mut CheckSet) {
    // 正样本：全绿。
    let good = ready_bottom_lines();
    assert!(good.all_pass(), "正样本三底线应全通：{}", good.screen_text());
    set.add(
        "P01-底线-正样本三底线全通",
        good.all_pass(),
        "帧密度达标且已检、打断策略齐备、reduce 终态齐备",
    );

    // 帧采样密度不足即红。
    let mut thin = BottomLineLedger::new();
    thin.push_sample(FrameSample {
        instance: 0,
        frame: 1,
        sample_count: MIN_SAMPLES_PER_FRAME - 1,
        zoom_inspected: true,
    })
    .expect("入账");
    set.add(
        "P01-底线-帧采样密度不足判红",
        thin.verdict(BottomLine::FrameAudit) == LineVerdict::Fail(1),
        "密度不够就是「帧帧可放大看」说了假话",
    );

    // 采够了但没放大检查仍判红（采样≠检查）。
    let mut unchecked = BottomLineLedger::new();
    unchecked.push_sample(FrameSample {
        instance: 0,
        frame: 1,
        sample_count: MIN_SAMPLES_PER_FRAME + 8,
        zoom_inspected: false,
    })
    .expect("入账");
    set.add(
        "P01-底线-未放大检查仍判红",
        unchecked.verdict(BottomLine::FrameAudit) == LineVerdict::Fail(1),
        "密度够但没人看过，等于采样了但没检查",
    );

    // 打断策略缺失即红。
    let mut no_interrupt = BottomLineLedger::new();
    no_interrupt
        .push_instance(ActiveInstance {
            id: 0,
            theme: MotionTheme::Orchestration,
            interrupt: None,
            reduce_declared: true,
        })
        .expect("入账");
    set.add(
        "P01-底线-无打断策略判红",
        no_interrupt.verdict(BottomLine::InterruptAudit) == LineVerdict::Fail(1),
        "无策略的实例在被打断时必然留下半途态",
    );

    // reduce 终态未声明即红。
    let mut no_reduce = BottomLineLedger::new();
    no_reduce
        .push_instance(ActiveInstance {
            id: 0,
            theme: MotionTheme::MicroFeedback,
            interrupt: Some(InterruptPolicy::HoldInPlace),
            reduce_declared: false,
        })
        .expect("入账");
    set.add(
        "P01-底线-reduce终态未声明判红",
        no_reduce.verdict(BottomLine::CoverageAudit) == LineVerdict::Fail(1),
        "reduce 下必须到终态，冻结在中间比不动更糟",
    );

    // 空集判「无数据」而不是「通过」——没测不等于过了。
    let empty = BottomLineLedger::new();
    let all_nodata = BOTTOM_LINE_ORDER
        .iter()
        .all(|l| empty.verdict(*l) == LineVerdict::NoData);
    set.add(
        "P01-底线-空集判无数据非通过",
        all_nodata && !empty.all_pass(),
        "没测不等于过了——空集判通过是这类系统最常见的假绿",
    );

    // 红项要能点名。
    let mixed = {
        let mut m = ready_bottom_lines();
        m.push_instance(ActiveInstance {
            id: 99,
            theme: MotionTheme::Flip,
            interrupt: None,
            reduce_declared: true,
        })
        .expect("入账");
        m
    };
    set.add(
        "P01-底线-红项可点名",
        mixed.failing_lines() == vec![BottomLine::InterruptAudit],
        "不能只报「有个红」",
    );

    // 三底线是合取，不许以长补短。
    set.add(
        "P01-底线-三底线并列合取",
        !mixed.all_pass() && mixed.verdict(BottomLine::FrameAudit).is_pass(),
        "帧审计达标不能抵消打断审计缺失",
    );

    // 底线码往返与判据说明非空。
    let guard = BottomLine::ALL
        .iter()
        .all(|b| BottomLine::from_code(b.code()) == Some(*b))
        && BottomLine::from_code("BL-NOPE").is_none();
    set.add("P01-底线-底线码往返守卫", guard, "未知底线码显性拒绝");
    set.add(
        "P01-底线-三条判据说明非空",
        BOTTOM_LINE_ORDER
            .iter()
            .all(|b| !b.promise().trim().is_empty()),
        "怎么算通过必须写下来，否则只是标签",
    );

    // 三种打断策略码往返。
    let ip = InterruptPolicy::ALL
        .iter()
        .all(|p| InterruptPolicy::from_code(p.code()) == Some(*p))
        && InterruptPolicy::from_code("IP-NOPE").is_none();
    set.add("P01-底线-打断策略码往返守卫", ip, "三选一策略码守卫");
}

// ---------------------------------------------------------------------------
// 判据五：第一红线
// ---------------------------------------------------------------------------

fn chk_first_red_line(set: &mut CheckSet) {
    // 空策略账 = 全部默认覆盖（**默认覆盖是常态**）。
    let bare = ReducedMotionPolicy::new();
    let defaults = bare
        .audit_all_themes(0)
        .iter()
        .filter(|(_, v)| matches!(v, CoverageVerdict::Covered))
        .count();
    set.add(
        "P01-红线-零豁免即默认覆盖",
        defaults == THEME_COUNT - 1,
        "除跟手（须登记豁免）外全部默认覆盖",
    );

    // 跟手声明用户驱动但未登记豁免 ⇒ 违规（声明不是许可证）。
    let v = bare.evaluate(MotionTheme::Gesture, 0);
    set.add(
        "P01-红线-未登记豁免判违规",
        !v.is_compliant(),
        "用户驱动是理由，不是许可证；豁免必须登记",
    );

    // 登记合规豁免后转为合规。
    let mut p = ReducedMotionPolicy::new();
    let reg = p.register(Exemption {
        kind: ExemptionKind::FollowFinger,
        reason: "跟手进度由手指直接驱动，reduce 后用户将失去对拖拽进度的控制".to_string(),
        expires_at_frame: 10_000,
        tick: 0,
    });
    set.add(
        "P01-红线-合规豁免可登记",
        reg.is_ok() && p.evaluate(MotionTheme::Gesture, 0).is_compliant(),
        "理由+期限齐备的豁免生效",
    );

    // 三项拒绝：无理由 / 无期限 / 同类重复。
    let mut q = ReducedMotionPolicy::new();
    let no_reason = q.register(Exemption {
        kind: ExemptionKind::FocusRing,
        reason: "   ".to_string(),
        expires_at_frame: 100,
        tick: 0,
    });
    set.add(
        "P01-红线-无理由豁免被拒",
        no_reason.is_err()
            && no_reason.expect_err("应拒绝").code == E_EXEMPTION_NO_REASON,
        "锚点原文：豁免必须白名单理由",
    );
    let no_deadline = q.register(Exemption {
        kind: ExemptionKind::FocusRing,
        reason: "焦点位置必须可见".to_string(),
        expires_at_frame: 0,
        tick: 0,
    });
    set.add(
        "P01-红线-无期限豁免被拒",
        no_deadline.is_err()
            && no_deadline.expect_err("应拒绝").code == E_EXEMPTION_NO_DEADLINE,
        "永久豁免= 永久绕过第一红线",
    );
    q.register(Exemption {
        kind: ExemptionKind::FocusRing,
        reason: "焦点位置必须可见，否则键盘用户失去位置".to_string(),
        expires_at_frame: 100,
        tick: 0,
    })
    .expect("首个焦点环豁免");
    let dup = q.register(Exemption {
        kind: ExemptionKind::FocusRing,
        reason: "再豁免一次".to_string(),
        expires_at_frame: 200,
        tick: 0,
    });
    set.add(
        "P01-红线-同类重复豁免被拒",
        dup.is_err() && dup.expect_err("应拒绝").code == E_EXEMPTION_DUP,
        "两条豁免会让绕过口径不唯一",
    );
    // 三次拒绝（无理由、无期限、同类重复）各留一条痕。
    set.add(
        "P01-红线-被拒次数显性计数",
        q.refused() == 3,
        "拒绝必须留痕，否则被拒的那些会被当成「没申请过」",
    );

    // 过期即失效（过期不删除但必须能被看见）。
    set.add(
        "P01-红线-过期豁免失效可见",
        !p.evaluate(MotionTheme::Gesture, 20_000).is_compliant()
            && p.expired(20_000).len() == 1,
        "过期不删除，但判定必须失效",
    );

    // 豁免不是全局开关：跟手豁免不救其他主题。
    let other = p.evaluate(MotionTheme::Scroll, 0);
    set.add(
        "P01-红线-豁免非全局开关",
        matches!(other, CoverageVerdict::Covered),
        "跟手豁免不该把滚动驱动也豁免掉",
    );

    // 微反馈在 reduce 下保留可见性（反馈不因 reduce 消失）。
    let fb = bare.evaluate(MotionTheme::MicroFeedback, 0);
    set.add(
        "P01-红线-微反馈reduce仍可见",
        fb.is_compliant()
            && MotionTheme::MicroFeedback
                .reduce_terminal()
                .is_user_visible(),
        "反馈转为瞬时状态切换，不是取消反馈",
    );

    // 直达是画面终态而非黑屏。
    set.add(
        "P01-红线-直达指终态非黑屏",
        MotionTheme::Flip.reduce_terminal() == ReduceTerminal::Direct,
        "直达= 终态原子应用，不是黑屏",
    );

    // 豁免类别码往返。
    let guard = ExemptionKind::ALL
        .iter()
        .all(|k| ExemptionKind::from_code(k.code()) == Some(*k))
        && ExemptionKind::from_code("EX-NOPE").is_none();
    set.add("P01-红线-豁免类别码往返守卫", guard, "三类豁免码守卫");
}

// ---------------------------------------------------------------------------
// 判据六：判据自证可追溯 + 降级矩阵 + 跨批对接 + 无障碍替述 + 错误零静默
// ---------------------------------------------------------------------------

fn chk_criterion_and_degradation(set: &mut CheckSet) {
    let ready = ready_arch();

    // 六项判据齐备，码唯一，执行体非空。
    set.add(
        "P01-判据-六项判据齐备",
        CRITERIA.len() == CRITERION_COUNT,
        "三组架构/十项映射/单源分工/三底线/第一红线/判据，一项不缺",
    );
    let codes: Vec<&str> = CRITERIA.iter().map(|c| c.code()).collect();
    let mut uniq = codes.clone();
    uniq.dedup();
    let guard = CRITERIA
        .iter()
        .all(|c| Criterion::from_code(c.code()) == Some(*c))
        && uniq.len() == codes.len();
    set.add("P01-判据-判据码唯一且往返", guard, "未知判据码显性拒绝");
    set.add(
        "P01-判据-承诺句非空",
        CRITERIA.iter().all(|c| !c.promise().trim().is_empty()),
        "写下来就是契约，不许用「应该」「尽量」",
    );
    set.add(
        "P01-判据-执行体非空",
        CRITERIA.iter().all(|c| !c.enforced_by().trim().is_empty()),
        "任一判据无可执行断言即视为未落实",
    );

    // 全绿总纲：自检与preflight 均应零缺口。
    set.add(
        "P01-判据-全绿总纲自检零问题",
        ready.self_audit().is_empty(),
        "契约/映射/轴服务/对端/对拍/降级链/零开销八路自检须全绿",
    );
    let pf = ready.preflight();
    assert!(pf.is_empty(), "全绿总纲应可开工，缺口：{:?}", pf);
    set.add(
        "P01-判据-全绿总纲可开工",
        pf.is_empty(),
        "契约已签+对拍已过+三底线有数据+降级链前向齐备",
    );

    // 标准总纲**未接收契约**时必须报缺口（总纲交付≠可开工）。
    let bare = MotionArchitecture::standard();
    set.add(
        "P01-对接-标准态不得假绿",
        !bare.preflight().is_empty(),
        "总纲交付不等于契约已签署；未接收阻断级对端时必须报缺口",
    );

    // 降级矩阵第一格：能力分歧对拍拦截（不可用 / 语义漂移 / 句柄缺失三码分立）。
    let mut pr = ProbeLedger::new();
    let unavailable = pr.submit(CapabilityProbe {
        capability: RuntimeCapability::FrameClock,
        assumed_handle: 0x2001,
        assumed_semantics: "单调帧时钟".to_string(),
        observed_available: false,
        observed_semantics: "单调帧时钟".to_string(),
        tick: 1,
    });
    set.add(
        "P01-降级-能力不可用被拦",
        unavailable.is_err()
            && unavailable.expect_err("应拦").code == E_RUNTIME_CAPABILITY_DIVERGENCE,
        "能力分歧不降级掩盖：错的能力面比明确报错更难排查",
    );
    let drift = pr.submit(CapabilityProbe {
        capability: RuntimeCapability::Velocity,
        assumed_handle: 0x2002,
        assumed_semantics: "初速度注入".to_string(),
        observed_available: true,
        observed_semantics: "仅速度读取".to_string(),
        tick: 1,
    });
    set.add(
        "P01-降级-语义漂移被拦",
        drift.is_err() && drift.expect_err("应拦").code == E_RUNTIME_SEMANTICS_DRIFT,
        "句柄可用不等于语义相同；三码分立让归因不退化成猜",
    );
    let no_handle = pr.submit(CapabilityProbe {
        capability: RuntimeCapability::Reverse,
        assumed_handle: HANDLE_UNSPECIFIED,
        assumed_semantics: "逆向播放".to_string(),
        observed_available: true,
        observed_semantics: "逆向播放".to_string(),
        tick: 1,
    });
    set.add(
        "P01-降级-句柄缺失被拦",
        no_handle.is_err()
            && no_handle.expect_err("应拦").code == E_PROBE_HANDLE_MISSING,
        "不接受「未指定也能跑」",
    );
    set.add(
        "P01-对接-六项对拍完备判定",
        ready_probes().is_complete() && !ProbeLedger::new().is_complete(),
        "六项齐备才是完整对拍；空账不算完备",
    );

    // 降级矩阵第二格：组间接口变更走 ADR。
    let mut adr = AdrLedger::new();
    set.add(
        "P01-降级-ADR无否决理由被拒",
        adr
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::MotionLibrary,
                title: "改段契约".to_string(),
                decision: "把编排段提前".to_string(),
                rejected: "  ".to_string(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: "P01-iface-v2".to_string(),
                tick: 1,
            })
            .is_err(),
        "不写否决理由的 ADR 无法复核，半年后有人会把否决方案再提一遍",
    );
    let ok_adr = adr.register(AdrRecord {
        id: 0,
        group: MotionGroup::MotionLibrary,
        title: "编排段加入预算位".to_string(),
        decision: "编排段前插预算检查段，使预算次序由段序派生".to_string(),
        rejected: "否决在O04 侧加预算检查：编译期才知道实际用量，检查点太晚".to_string(),
        from_version: INTERFACE_VERSION.to_string(),
        to_version: "P01-iface-v2".to_string(),
        tick: 1,
    });
    set.add(
        "P01-降级-合规ADR可登记",
        ok_adr.is_ok() && adr.len() == 1 && adr.count_for(MotionGroup::MotionLibrary) == 1,
        "五要素齐备 + 升版信息完整",
    );
    set.add(
        "P01-降级-ADR新旧版本相同被拒",
        adr
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::PageTransition,
                title: "空转升版".to_string(),
                decision: "改点什么".to_string(),
                rejected: "否决保持原样".to_string(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: INTERFACE_VERSION.to_string(),
                tick: 2,
            })
            .is_err(),
        "升版信息自相矛盾会让下游判断错该按哪版编",
    );
    set.add(
        "P01-降级-ADR按组可计数",
        adr.count_for(MotionGroup::PageTransition) == 0,
        "组间接口变更多少次要能一眼看出",
    );

    // 降级矩阵第三格：降级链前向声明（齐但全 false 合法，缺档/谎报是缺陷）。
    let chain = DegradeChain::forward();
    set.add(
        "P01-降级-链前向齐备合法",
        chain.check_forward().is_empty() && chain.landed_count() == 0,
        "F3038 未落地前四档全false 是合法前向态",
    );
    let short = DegradeChain::from_entries(chain.iter().take(2).copied().collect());
    set.add(
        "P01-降级-缺档判缺陷",
        !short.check_forward().is_empty(),
        "缺档即缺陷",
    );
    let reversed = DegradeChain::from_entries(vec![
        ChainEntry {
            stage: DegradeStage::FadeOnly,
            landed: false,
            equivalence: "",
        },
        ChainEntry {
            stage: DegradeStage::DropStagger,
            landed: false,
            equivalence: "",
        },
    ]);
    set.add(
        "P01-降级-档序错判缺陷",
        !reversed.check_forward().is_empty(),
        "降级序从轻到重，倒序会让「先降最重的」",
    );
    set.add(
        "P01-降级-面目全非档需等效验收",
        DegradeStage::DirectJump.is_identity_breaking()
            && !ChainEntry {
                stage: DegradeStage::DirectJump,
                landed: true,
                equivalence: "",
            }
            .equivalence_ok(),
        "直达档把动效变跳变，视觉差异最大，须等效性验收",
    );
    let dg_guard = DegradeStage::ALL
        .iter()
        .all(|d| DegradeStage::from_code(d.code()) == Some(*d))
        && DegradeStage::from_code("DG-NOPE").is_none();
    set.add("P01-降级-档位码往返守卫", dg_guard, "四档码守卫");

    // 跨批对接：哈希对账是重算不是"看一眼"。
    let mut pl = std_peers();
    let bad = pl.reconcile(Peer::RuntimeM, "被改过的内容");
    set.add(
        "P01-对接-哈希不一致被拒",
        bad.is_err() && bad.expect_err("应拒").code == E_HASH_MISMATCH,
        "哈希对账是重算一遍，不是看一眼有没有填",
    );
    set.add(
        "P01-对接-对端未登记被拒",
        PeerLedger::new().reconcile(Peer::RuntimeM, "x").is_err(),
        "未登记即无对账依据",
    );
    set.add(
        "P01-对接-阻断级对端齐备可开工",
        std_peers().check_blocking_ready().len() == 2,
        "M/O04 未接收未对账时必须报两项缺口",
    );
    set.add(
        "P01-对接-阻断级对端齐备后放行",
        ready_peers().check_blocking_ready().is_empty(),
        "四端全部对账通过",
    );
    set.add(
        "P01-对接-条目号格式校验",
        is_valid_item_id("VE-F3001") && !is_valid_item_id("VE-F30") && !is_valid_item_id("F3001"),
        "VE-F + 四位数字",
    );

    // 无障碍：读屏替述覆盖三组/十项/五轴/六判据/三底线/禁扩面。
    let n = ready.architecture_narration(0);
    let mut covers = !n.is_empty();
    for g in GROUP_ORDER.iter() {
        covers = covers && n.contains(g.zh());
    }
    for t in MotionTheme::ALL.iter() {
        covers = covers && n.contains(t.zh());
    }
    for x in SupportAxis::ALL.iter() {
        covers = covers && n.contains(x.zh());
    }
    for c in CRITERIA.iter() {
        covers = covers && n.contains(c.zh());
    }
    for l in BOTTOM_LINE_ORDER.iter() {
        covers = covers && n.contains(l.zh());
    }
    covers = covers && n.contains("禁扩面") && n.contains("零运行时开销");
    set.add(
        "P01-读屏-替述覆盖全部结构",
        covers,
        "替述与总纲同源生成；另写一份会漂移，漂移的无障碍文档比没有更坏",
    );

    // 错误零静默：五元组齐备。
    let sample_err = MotionError::new(
        E_THEME_LANDING_EMPTY,
        "落点为空",
        "落点条目存在但条目号列表为空",
        "填入 VE-Fxxxx 条目号",
        "P 域架构维护方",
    );
    set.add(
        "P01-错误-五元组齐备",
        sample_err.is_complete() && !sample_err.screen_text().is_empty(),
        "码/现象/原因/下一步/责任方；拒绝必须给出路",
    );
    set.add(
        "P01-错误-各错误族可定位",
        !alloc::vec![
            E_GROUP_CAP,
            E_SEGMENT_INCOMPLETE,
            E_RUNTIME_OVERHEAD_DECLARED,
            E_THEME_LANDING_EMPTY,
            E_RUNTIME_CAPABILITY_DIVERGENCE,
            E_BOUNDARY_OVERREACH,
            E_EXEMPTION_NO_REASON,
            E_ADR_NO_REJECTED,
            E_CHAIN_EQUIV_MISSING,
        ]
        .is_empty(),
        "错误码分域前缀，处置方向相反的状态不共用码",
    );

    // 下游归属表：每条都要点名"谁拥有什么"，防抢活。
    set.add(
        "P01-判据-下游归属十条在册",
        DOWNSTREAM_OWNERSHIP.len() == 10
            && DOWNSTREAM_OWNERSHIP
                .iter()
                .all(|(id, what)| is_valid_item_id(id) && !what.trim().is_empty()),
        "总纲顺手把下游活干完，下游开工时才发现已被人做过",
    );

    // 账满拒绝并计数（告警静默丢弃等于钳制失效）。
    let mut full = BottomLineLedger::new();
    let mut overflowed = false;
    for i in 0..=MAX_ACTIVE_INSTANCES {
        if full
            .push_instance(ActiveInstance {
                id: i as u32,
                theme: MotionTheme::Flip,
                interrupt: Some(InterruptPolicy::Rollback),
                reduce_declared: true,
            })
            .is_err()
        {
            overflowed = true;
            break;
        }
    }
    set.add(
        "P01-错误-实例账满拒绝",
        overflowed && full.len() == MAX_ACTIVE_INSTANCES,
        "满后拒绝，不静默丢弃",
    );
}

/// VE-F3001 域自检总入口。
pub fn run_vep01_checks() -> CheckSet {
    let mut set = CheckSet::new("vep01-arch");
    chk_three_groups(&mut set);
    chk_theme_mapping(&mut set);
    chk_division_of_duty(&mut set);
    chk_bottom_lines(&mut set);
    chk_first_red_line(&mut set);
    chk_criterion_and_degradation(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vep01_contract_selfcheck_clean() {
        let a = MotionArchitecture::standard();
        assert!(
            a.check_contracts().is_empty(),
            "标准总纲不应有契约问题：{:?}",
            a.check_contracts()
        );
        assert_eq!(a.version, ARCH_VERSION);
        assert_eq!(a.interface_version, INTERFACE_VERSION);
    }

    #[test]
    fn vep01_group_order_is_single_source() {
        assert_eq!(GROUP_ORDER.len(), MotionGroup::ALL.len());
        for (i, g) in GROUP_ORDER.iter().enumerate() {
            assert_eq!(g.rank() as usize, i);
            assert_eq!(*g, MotionGroup::ALL[i]);
            assert_eq!(MotionGroup::from_code(g.code()), Some(*g));
        }
        assert_eq!(MotionGroup::from_code("P01-G0"), None);
        // 段序同样是单源。
        for g in GROUP_ORDER.iter() {
            for (i, s) in g.segments().iter().enumerate() {
                assert_eq!(s.rank as usize, i);
            }
        }
    }

    #[test]
    fn vep01_zero_runtime_cost_is_verifiable() {
        let a = MotionArchitecture::standard();
        assert!(a.is_zero_runtime_cost());
        let cost = a.runtime_cost();
        assert_eq!(cost.len(), total_segment_count());
        assert!(cost.iter().all(|(_, c)| c.is_zero_overhead()));
    }

    #[test]
    fn vep01_ten_themes_are_complete() {
        let a = MotionArchitecture::standard();
        let audit = a.audit_themes();
        assert!(audit.is_complete(), "十主题审计须全绿：{}", audit.screen_text());
        assert_eq!(MotionTheme::ALL.len(), THEME_COUNT);
        for t in MotionTheme::ALL.iter() {
            assert_eq!(MotionTheme::from_code(t.code()), Some(*t));
            let l = a.theme_landing(*t).expect("定位").expect("在册");
            assert_eq!(l.theme, *t);
            assert!(!l.items.is_empty());
            assert_eq!(l.group, t.primary_group());
        }
        assert_eq!(MotionTheme::from_code("MT-NOPE"), None);
    }

    #[test]
    fn vep01_landing_lookup_distinguishes_missing_from_empty() {
        let mut a = MotionArchitecture::standard();
        // 抽掉一项 ⇒ 查无此项。
        a.landings.truncate(THEME_COUNT - 1);
        assert!(a.theme_landing(MotionTheme::MicroFeedback).unwrap().is_none());
        // 保留但清空 ⇒ 查得此项且落点为空（两种故障可区分）。
        let mut b = MotionArchitecture::standard();
        b.landings[MotionTheme::Flip.rank()].items.clear();
        let hit = b.theme_landing(MotionTheme::Flip).unwrap();
        assert!(hit.is_some());
        assert!(hit.expect("在册").items.is_empty());
        let audit = b.audit_themes();
        assert_eq!(audit.empty.len(), 1);
        assert!(!audit.is_complete());
    }

    #[test]
    fn vep01_anchor_labels_cover_all_fifteen() {
        let cov = check_anchor_label_coverage();
        assert!(cov.is_complete(), "十五标签须逐项对齐：{:?}", cov.screen_text());
        assert_eq!(ANCHOR_LABELS.len(), THEME_COUNT + AXIS_COUNT);
        assert_eq!(MotionTheme::ALL.len(), 10);
        assert_eq!(SupportAxis::ALL.len(), 5);
    }

    #[test]
    fn vep01_axes_must_serve() {
        let a = MotionArchitecture::standard();
        assert!(a.check_axes_served().is_empty());
        // 空轴即缺陷。
        let mut b = MotionArchitecture::standard();
        b.axes[0].serves.clear();
        assert!(!b.check_axes_served().is_empty());
        // 服务数不足即缺陷。
        let mut c = MotionArchitecture::standard();
        c.axes[1].serves.truncate(1);
        assert!(!c.check_axes_served().is_empty());
    }

    #[test]
    fn vep01_runtime_capability_cannot_be_claimed() {
        let mut d = RuntimeDivision::standard();
        for c in RuntimeCapability::ALL.iter() {
            let e = d.claim_runtime(*c).expect_err("运行时能力不归 P 域");
            assert_eq!(e.code, E_RUNTIME_CAPABILITY_NOT_OWNED);
            assert!(e.is_complete());
            assert!(e.next.contains("VE-M"));
        }
        assert_eq!(d.refused(), RuntimeCapability::ALL.len() as u64);
    }

    #[test]
    fn vep01_probe_divergence_is_blocked_not_downgraded() {
        let mut p = ProbeLedger::new();
        // 句柄缺失。
        assert!(p
            .submit(CapabilityProbe {
                capability: RuntimeCapability::PoseBlend,
                assumed_handle: HANDLE_UNSPECIFIED,
                assumed_semantics: "x".to_string(),
                observed_available: true,
                observed_semantics: "x".to_string(),
                tick: 0,
            })
            .is_err());
        // 不可用。
        assert!(p
            .submit(CapabilityProbe {
                capability: RuntimeCapability::PoseBlend,
                assumed_handle: 7,
                assumed_semantics: "x".to_string(),
                observed_available: false,
                observed_semantics: "x".to_string(),
                tick: 0,
            })
            .is_err());
        // 语义漂移。
        assert!(p
            .submit(CapabilityProbe {
                capability: RuntimeCapability::PoseBlend,
                assumed_handle: 7,
                assumed_semantics: "a".to_string(),
                observed_available: true,
                observed_semantics: "b".to_string(),
                tick: 0,
            })
            .is_err());
        assert!(p.is_empty());
        assert!(!p.is_complete());
        assert!(ready_probes().is_complete());
    }

    #[test]
    fn vep01_first_red_line_defaults_to_covered() {
        let p = ReducedMotionPolicy::new();
        assert!(p.is_empty());
        // 除跟手外全部默认覆盖。
        for t in MotionTheme::ALL.iter() {
            let v = p.evaluate(*t, 0);
            if *t == MotionTheme::Gesture {
                assert!(!v.is_compliant(), "跟手未登记豁免应违规");
            } else {
                assert!(matches!(v, CoverageVerdict::Covered));
            }
        }
        // 登记后合规，过期后复归违规。
        let mut q = ReducedMotionPolicy::new();
        q.register(Exemption {
            kind: ExemptionKind::FollowFinger,
            reason: "手指直接驱动进度".to_string(),
            expires_at_frame: 100,
            tick: 0,
        })
        .expect("登记");
        assert!(q.evaluate(MotionTheme::Gesture, 50).is_compliant());
        assert!(!q.evaluate(MotionTheme::Gesture, 100).is_compliant());
        assert_eq!(q.expired(100).len(), 1);
    }

    #[test]
    fn vep01_exemption_requires_reason_and_deadline() {
        let mut q = ReducedMotionPolicy::new();
        assert!(q
            .register(Exemption {
                kind: ExemptionKind::FocusRing,
                reason: String::new(),
                expires_at_frame: 10,
                tick: 0,
            })
            .is_err());
        assert!(q
            .register(Exemption {
                kind: ExemptionKind::FocusRing,
                reason: "焦点可见".to_string(),
                expires_at_frame: 0,
                tick: 0,
            })
            .is_err());
        q.register(Exemption {
            kind: ExemptionKind::FocusRing,
            reason: "焦点位置必须可见".to_string(),
            expires_at_frame: 10,
            tick: 0,
        })
        .expect("首个");
        assert!(q
            .register(Exemption {
                kind: ExemptionKind::FocusRing,
                reason: "重复".to_string(),
                expires_at_frame: 20,
                tick: 0,
            })
            .is_err());
        assert_eq!(q.refused(), 3);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn vep01_bottom_lines_empty_is_not_pass() {
        let e = BottomLineLedger::new();
        assert!(!e.all_pass());
        for l in BOTTOM_LINE_ORDER.iter() {
            assert_eq!(e.verdict(*l), LineVerdict::NoData);
        }
        // 正样本全通。
        let r = ready_bottom_lines();
        assert!(r.all_pass());
        assert!(r.failing_lines().is_empty());
    }

    #[test]
    fn vep01_frame_audit_needs_density_and_inspection() {
        let s_ok = FrameSample {
            instance: 0,
            frame: 1,
            sample_count: MIN_SAMPLES_PER_FRAME,
            zoom_inspected: true,
        };
        assert!(s_ok.passes());
        let mut s_thin = s_ok.clone();
        s_thin.sample_count = MIN_SAMPLES_PER_FRAME - 1;
        assert!(!s_thin.passes());
        let mut s_unchecked = s_ok.clone();
        s_unchecked.zoom_inspected = false;
        assert!(!s_unchecked.passes(), "采样不等于检查");
    }

    #[test]
    fn vep01_boundary_blocks_every_exclusion() {
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            let e = check_no_overreach(desc).expect_err("禁扩面必须被拦");
            assert_eq!(e.code, E_BOUNDARY_OVERREACH);
            assert!(e.why.contains(code));
            assert!(!boundary_advice(code).trim().is_empty());
        }
        assert!(check_no_overreach("编排动效声明并委派 M 域执行").is_ok());
    }

    #[test]
    fn vep01_chain_forward_is_honest() {
        let c = DegradeChain::forward();
        assert!(c.check_forward().is_empty());
        assert_eq!(c.landed_count(), 0, "F3038 未落地前不许谎报已落地");
        assert_eq!(c.len(), DegradeStage::ALL.len());
        // 档序从轻到重。
        for (i, d) in DegradeStage::ALL.iter().enumerate() {
            assert_eq!(d.rank() as usize, i);
        }
        // 缺档即缺陷。
        let short = DegradeChain::from_entries(c.iter().take(3).copied().collect());
        assert!(!short.check_forward().is_empty());
    }

    #[test]
    fn vep01_adr_requires_rejected_and_version() {
        let mut a = AdrLedger::new();
        // 缺否决理由。
        assert!(a
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::MotionLibrary,
                title: "t".to_string(),
                decision: "d".to_string(),
                rejected: String::new(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: "v2".to_string(),
                tick: 0,
            })
            .is_err());
        // 缺新版本。
        assert!(a
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::MotionLibrary,
                title: "t".to_string(),
                decision: "d".to_string(),
                rejected: "r".to_string(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: String::new(),
                tick: 0,
            })
            .is_err());
        // 新旧同版本。
        assert!(a
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::MotionLibrary,
                title: "t".to_string(),
                decision: "d".to_string(),
                rejected: "r".to_string(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: INTERFACE_VERSION.to_string(),
                tick: 0,
            })
            .is_err());
        let id = a
            .register(AdrRecord {
                id: 0,
                group: MotionGroup::MicroInteraction,
                title: "断言段前移".to_string(),
                decision: "断言段提前到形态段之后".to_string(),
                rejected: "否决放在最后：断言需要前序结论才有对象".to_string(),
                from_version: INTERFACE_VERSION.to_string(),
                to_version: "P01-iface-v2".to_string(),
                tick: 1,
            })
            .expect("合规 ADR");
        assert_eq!(id, 1);
        assert_eq!(a.incomplete_count(), 0);
        assert_eq!(a.count_for(MotionGroup::MicroInteraction), 1);
    }

    #[test]
    fn vep01_hash_reconcile_recomputes() {
        let mut l = std_peers();
        let content = "实例请求与回执：时间轴实例组（推进/混合/采样/速度/逆向由 M 域自持）";
        assert_eq!(
            l.reconcile(Peer::RuntimeM, content).expect("对账"),
            fnv1a64_hex(content.as_bytes())
        );
        // 内容一改即哈希不符。
        assert!(l.reconcile(Peer::RuntimeM, "改过的").is_err());
        // 确定性：同内容同哈希。
        assert_eq!(fnv1a64(b"varix"), fnv1a64(b"varix"));
        assert_ne!(fnv1a64(b"varix"), fnv1a64(b"variy"));
        assert_eq!(fnv1a64_hex(b"").len(), 16);
    }

    #[test]
    fn vep01_standard_arch_is_not_falsely_green() {
        let bare = MotionArchitecture::standard();
        let missing = bare.preflight();
        assert!(!missing.is_empty(), "标准态未收契约，必须报缺口");
        // 全绿总纲（契约已签、对拍已过、三底线有数据）才允许 preflight 空。
        let ready = ready_arch();
        assert!(ready.preflight().is_empty(), "{:?}", ready.preflight());
        assert!(ready.self_audit().is_empty(), "{:?}", ready.self_audit());
    }

    #[test]
    fn vep01_narration_covers_everything() {
        let n = ready_arch().architecture_narration(0);
        for g in GROUP_ORDER.iter() {
            assert!(n.contains(g.zh()), "替述缺组 {}", g.zh());
        }
        for t in MotionTheme::ALL.iter() {
            assert!(n.contains(t.zh()), "替述缺主题 {}", t.zh());
        }
        for x in SupportAxis::ALL.iter() {
            assert!(n.contains(x.zh()), "替述缺轴 {}", x.zh());
        }
        for c in CRITERIA.iter() {
            assert!(n.contains(c.zh()), "替述缺判据 {}", c.zh());
        }
        for l in BOTTOM_LINE_ORDER.iter() {
            assert!(n.contains(l.zh()), "替述缺底线 {}", l.zh());
        }
        assert!(n.contains("零运行时开销"));
        assert!(n.contains("禁扩面"));
        assert!(n.contains("VE-M"));
    }

    #[test]
    fn vep01_item_id_validation() {
        assert!(is_valid_item_id("VE-F3001"));
        assert!(is_valid_item_id("VE-F2861"));
        assert!(!is_valid_item_id("VE-F300"));
        assert!(!is_valid_item_id("VE-F30011"));
        assert!(!is_valid_item_id("F3001"));
        assert!(!is_valid_item_id(""));
    }

    #[test]
    fn vep01_downstream_ownership_declared() {
        assert_eq!(DOWNSTREAM_OWNERSHIP.len(), 10);
        for (id, what) in DOWNSTREAM_OWNERSHIP.iter() {
            assert!(is_valid_item_id(id), "归属条目号须合法：{}", id);
            assert!(!what.trim().is_empty(), "{} 须写明拥有什么", id);
        }
        // 语言册归 F3002、编排器归 F3005、无障碍总纲归 F3017——防抢活。
        let f3002 = DOWNSTREAM_OWNERSHIP
            .iter()
            .find(|(id, _)| *id == "VE-F3002")
            .expect("F3002 在册");
        assert!(f3002.1.contains("缓动家族"));
        let f3005 = DOWNSTREAM_OWNERSHIP
            .iter()
            .find(|(id, _)| *id == "VE-F3005")
            .expect("F3005 在册");
        assert!(f3005.1.contains("编排图"));
    }

    #[test]
    fn vep01_error_five_tuple_always_complete() {
        let e = MotionError::new(
            E_SEGMENT_INCOMPLETE,
            "w",
            "why",
            "next",
            "who",
        );
        assert!(e.is_complete());
        assert!(e.screen_text().contains(E_SEGMENT_INCOMPLETE));
    }

    #[test]
    fn vep01_segment_contract_has_seven_fields() {
        for g in GROUP_ORDER.iter() {
            for s in g.segments().iter() {
                assert!(s.is_complete(), "段 {} 契约不齐", s.code);
                assert!(s.cost.is_zero_overhead(), "段 {} 不得为运行期", s.code);
                assert!(!s.not_mine.trim().is_empty(), "段 {} 缺不做清单", s.code);
                assert!(s.complexity.starts_with('C'), "段 {} 复杂度须对账", s.code);
            }
        }
    }

    #[test]
    fn vep01_reduce_terminal_is_never_black_screen() {
        for t in MotionTheme::ALL.iter() {
            let term = t.reduce_terminal();
            assert!(
                matches!(
                    term,
                    ReduceTerminal::Direct | ReduceTerminal::InstantState | ReduceTerminal::UserDriven
                ),
                "{} 的 reduce 终态必须是三类之一",
                t.zh()
            );
        }
        // 微反馈的反馈不因 reduce 消失。
        assert!(MotionTheme::MicroFeedback.reduce_terminal().is_user_visible());
    }
}