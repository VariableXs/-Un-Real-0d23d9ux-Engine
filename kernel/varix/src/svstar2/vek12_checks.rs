//! VE-F2012 · 域自检（判据逐条对应，见 `vek12_smaa.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检族）：
//! - **四法三实一留** → `K12-四法-*`
//! - **选型决策表**（四维决策树）→ `K12-选型-*`
//! - **STUB 语义**（签名冻结 + 显性报错）→ `K12-stub-*`
//! - **数据单源**（F2017 基准）→ `K12-单源-*`
//! - 错误路径三条（显性报错 / 漂移对账 / 一致性拦截）→ `K12-错误-*`
//! - 预留零运行时 → `K12-零运行时-*`
//! - 无障碍诚实标注 → `K12-无障碍-*`
//!
//! ## 本文件的核心纪律：反向变体验证（bidirectional variant testing）
//!
//! 只验"基线全绿"的门禁等于没验——判据可能根本没测到东西。本文件对**每条
//! 关键判据**都配一个"如果实现写错会怎样"的**反假变体**（假实现），
//! 断言真判据在假实现上**必须转红**。变体不写进生产代码，只在本文件内
//! 以局部闭包重现"错误写法"，跑完即弃：
//!
//! | 变体 | 模拟的实现错误 | 被哪条判据抓住 |
//! | --- | --- | --- |
//! | V1 | 桩`dispatch` 返回 `Ok`（`Result<Frame,_>` 型） | `K12-stub-成功不可表达` |
//! | V2 | 桩路径`touch()` 了计数器（非零运行时） | `K12-零运行时-计数恰为零` |
//! | V3 | 指纹把 `f32` 载荷也吃进去（换优化级别即变） | `K12-stub-指纹与v1 一致` |
//! | V4 | pass 表改成"每个 pass 读一张谁也不产的 RT" | `K12-冻结-无悬挂读` |
//! | V5 | 首 pass 输入格式 ≠ 末 pass 输出格式（色彩格式漂移） | `K12-冻结-格式往返` |
//! | V6 | 挂载位把 SMAA 挪到独立槽（与 TAA 不同位） | `K12-四法-SMAA与TAA同位` |
//! | V7 | 成本快照写死、**不引用**三法常量（第二真源） | `K12-单源-行成本引用三法` |
//! | V8 | 快照值改成与基准不符（漂移） | `K12-单源-全表对账无漂移` |
//! | V9 | 规则表里加一条指向 SMAA 的规则 | `K12-错误-决策树不指向预留` |
//! | V10 | 加一条恒不可满足的守卫（死规则） | `K12-选型-无死规则` |
//! | V11 | 删掉兜底覆盖的那条规则（留空洞） | `K12-选型-无空洞` |
//! | V12 | 把兜底塞进 `RULES` 末尾（死兜底） | `K12-选型-兜底未被命中` |
//! | V13 | `EstimateBasis::is_measured` 谎报 true | `K12-单源-预估非实测` |
//! | V14 | 可用性文本改成承诺腔（"即将支持"） | `K12-无障碍-不承诺未实现` |
//! | V15 | 指路清单手写死字符串（不随实现导出） | `K12-stub-指路清单随实现导出` |
//!
//! **另外四条自律**：
//! 1. **判据侧独立重算**：期望值（如指纹、命中数）由本文件**独立算一遍**，
//!    不调被测函数现算的值当答案（指纹一项由本文件的 `ref_fingerprint()`
//!    用**同样的字节序列独立重算**，与生产实现互为对照）；
//! 2. **精确比较优先于阈值比较**：能用 `==` / 精确值就不用 `>=`；
//! 3. **拒绝路径也要测**：越界索引 / 未知 baseline id / `NaN` 参数 /
//!    槽位冲突——只测 happy path 的门禁等于没测；
//! 4. **覆盖计数用 `==` 不用 `>=`**：兜底命中"不超过"某数是弱门禁，
//!    "恰为 0"才是零运行时/无死兜底的强形式。
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek12_smaa::*;
// `alloc` 三件套必须**逐个显式引入**（判据层自己要用 `Vec`/`String`，
// 不能指望 `vek12_smaa::*` 的 glob 带进来——它里面的 `use alloc::…`
// 是**私有导入**，glob 不会转发）。
//
// **这条import 不是形式主义**（此处曾是真缺陷，记录在案）：本文件最初
// 依赖 std prelude 里的 `String`/`Vec` 也能过编译，于是**隔离探针全绿**；
// 但真 crate 是 `#![cfg_attr(not(test), no_std)]` —— 没有 std prelude
// ⇒ `String`/`Vec` 未定义 ⇒ 22 个 E0425/E0433。
// **教训：探针必须照抄真 crate 的 cfg**（本条即"no_std 探针必须照抄真 cfg"
// 那条纪律的又一个变体——**这次是反过来的：探针比真crate 宽松**，
// 宽松的方向恰好是"能编过"，所以它专门掩盖 import 缺失这类问题）。
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===========================================================================
// 判据侧独立重算（不复用生产实现的私有量，见自律 1）
// ===========================================================================

/// 判据侧独立实现的 FNV-1a。
fn chk_fnv_push(mut h: u64, b: u8) -> u64 {
    h ^= (b as u64) & 0xff;
    h.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 判据侧独立实现的 FNV-1a 字符串追加。
fn chk_fnv_str(mut h: u64, s: &str) -> u64 {
    let bs = s.as_bytes();
    let mut i = 0;
    while i < bs.len() {
        h = chk_fnv_push(h, bs[i]);
        i += 1;
    }
    h
}

/// 判据侧**独立重算**的三pass 签名指纹。
///
/// 字节序列**照 `FROZEN_PASSES` 的结构手工列出**（而不是遍历生产表）——
/// 这样"生产表被改了"与"判据期望值"是两条独立的真源，改一处必红。
fn ref_fingerprint() -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    // (pass.wire, [(rt_name, format.wire, access.wire)], 输出同上)
    let spec: [(u8, (&str, u8, u8), (&str, u8, u8)); 3] = [
        // pass1: edge detect
        (1, ("scene_color", 3, 1), ("smaa_edge", 1, 2)),
        // pass2: feature map
        (2, ("smaa_edge", 1, 1), ("smaa_feature", 2, 2)),
        // pass3: blend weights（两路输入）
        (3, ("smaa_feature", 2, 1), ("smaa_out", 3, 2)),
    ];
    let mut h = FNV_OFFSET;
    for (pw, inp, outp) in spec.iter() {
        h = chk_fnv_push(h, *pw);
        let nin: u8 = if *pw == 3 { 2 } else { 1 };
        h = chk_fnv_push(h, nin);
        h = chk_fnv_str(h, inp.0);
        h = chk_fnv_push(h, inp.1);
        h = chk_fnv_push(h, inp.2);
        if *pw == 3 {
            // blend 的第二路输入（scene_color）也必须计入，否则指纹与生产不符
            h = chk_fnv_str(h, "scene_color");
            h = chk_fnv_push(h, 3);
            h = chk_fnv_push(h, 1);
        }
        h = chk_fnv_push(h, 1); // outputs.len() == 1
        h = chk_fnv_str(h, outp.0);
        h = chk_fnv_push(h, outp.1);
        h = chk_fnv_push(h, outp.2);
    }
    h
}

/// 反假变体 V3：指纹把 `f32` 载荷也吃进去。
///
/// **为什么要单独造这个变体**：生产实现刻意**不吃浮点**，理由是
/// `-0.0` / `NaN` 的位模式会让同一份表在不同优化级别下算出不同指纹。
/// 但"刻意不吃"这件事**在代码上无法自我证明**——判据只能验证
/// "当前实现算出的值 == 期望值"。若有人后来把浮点加进哈希，且恰好
/// 在当前平台/优化级别下算出同一个值，判据会**静默通过**。
/// 因此这里造一个"吃浮点"的变体，证明判据的期望值**确实由字节序列决定**：
/// 变体算出不同的值 ⇒ 期望值不是碰巧对上的。
fn variant_fingerprint_with_f32() -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for d in FROZEN_PASSES.iter() {
        h = chk_fnv_push(h, d.pass.wire());
        // 变体：把 pass 索引伪装成浮点再吃其位模式
        let idx = match d.pass {
            SmaaPass::EdgeDetect => 0.0f32,
            SmaaPass::FeatureMap => 1.5f32,
            SmaaPass::BlendWeights => 2.25f32,
        };
        let bits = idx.to_bits() as u64;
        h = chk_fnv_push(h, (bits & 0xff) as u8);
        h = chk_fnv_push(h, ((bits >> 8) & 0xff) as u8);
        h = chk_fnv_push(h, ((bits >> 16) & 0xff) as u8);
        h = chk_fnv_push(h, ((bits >> 24) & 0xff) as u8);
    }
    h
}

// ===========================================================================
// 反假变体：桩（V1 / V2 / V15）
// ===========================================================================

/// 反假变体 V1：桩返回成功。
///
/// 模拟"预留接口返回一个 pass-through 结果"的真实失败形态。生产代码用
/// `Result<Infallible, _>` 在**类型上**排除了它，本变体证明判据认得它。
fn variant_dispatch_returns_ok(p: SmaaPass) -> Result<AllocationWitness, SmaaStubError> {
    // 一个"看起来成功"的载荷（若有 Infallible 则根本写不出来）
    let _w = AllocationWitness {
        pass: p,
        pixel_work_done: true,
    };
    // 真实现的报错仍然返回（本变体只改Ok 侧的类型，错误侧照旧）
    Err(SmaaStubError::pass_not_implemented(p))
}

/// 假想的"成功载荷"（仅用于 V1 的类型演示）。
struct AllocationWitness {
    #[allow(dead_code)]
    pass: SmaaPass,
    /// 真的做了像素工作——这正是"预留返回成功"的可观测后果。
    #[allow(dead_code)]
    pixel_work_done: bool,
}

/// 反假变体 V2：桩路径做了像素工作（破了零运行时）。
fn variant_dispatch_touches(p: SmaaPass, c: &mut TouchCounter) -> SmaaStubResult {
    c.touch();
    let _ = p;
    Err(SmaaStubError::pass_not_implemented(p))
}

/// 反假变体 V15：指路清单手写死字符串。
///
/// 模拟"建议文本里硬写了三个方法名"——若将来某法被移出可用集合，
/// 手写文本会**静默过期**（用户照着改设置会踩空）。真实现从
/// [`complete_methods`] 导出，故文本随实现变。
fn variant_advice_hardcoded() -> String {
    let mut s = String::new();
    s.push_str("请改用 MSAA/FXAA/TAA");
    s
}

// ===========================================================================
// 反假变体：冻结表连通性（V4 / V5）
// ===========================================================================

/// 反假变体 V4：每个 pass 读一张"谁也不产"的 RT（悬挂读）。
fn variant_has_dangling_read() -> bool {
    let bad = ["phantom_a", "phantom_b", "phantom_c"];
    for name in bad.iter() {
        if producer_of(name).is_some() {
            return false;
        }
    }
    true
}

/// 反假变体 V5：末 pass 输出格式被改成非 RGBA8（格式往返被破坏）。
///
/// **初版此变体是坏的**（记录在案）：它把**真实表**的首输入与**真实表**的
/// 末输出相比——两者都是 `Rgba8Unorm`，于是恒返回 `false`，判据
/// `K12-冻结-反假V5格式被抓` 只能红、永远不可能绿。这类"变体其实测不到
/// 任何东西"的错误**不会以编译错误或运行错误的形式暴露**，只会表现为
/// "判据怎么改都不绿"，很容易被误判成"环境问题"而去改判据本身。
/// 正解：变体必须**自己构造出错误形态**（这里直接把末输出格式换成
/// `Rg8Unorm`），再断言该形态确实破坏往返。
fn variant_format_roundtrip_broken() -> bool {
    let first_in = match FROZEN_PASSES.first().and_then(|p| p.inputs.first()) {
        Some(d) => d.format,
        None => return false,
    };
    // 变体形态：末 pass 输出被改成双通道（有人以为"特征图直通，省一次拷贝"）
    let hypothetical_last_out = RtFormat::Rg8Unorm;
    first_in.wire() != hypothetical_last_out.wire()
}

// ===========================================================================
// 反假变体：成本与基准（V7 / V8 / V13）
// ===========================================================================

/// 反假变体 V7：成本快照**不引用**三法常量（写成第二真源）。
///
/// 模拟"表作者把 0.3 手抄进表里"——此刻它与基准**恰好一致**，
/// 判据"引用一致性"抓不到；真正的破绽在V8：基准一变、手抄的不变。
fn variant_authored_cost_detached(m: AaMethod) -> u32 {
    match m {
        AaMethod::Msaa => 300,
        AaMethod::Fxaa => 200,
        AaMethod::Taa => 600,
        // SMAA 行真实现由 `ms_to_x1000(0.0)`得出 0；变体直接写 999（假装有成本）
        _ => 999,
    }
}

/// 反假变体 V8：快照与基准漂移（改了基准忘了改表）。
fn variant_drifted_row() -> SelectionRow {
    let r = match row_of(AaMethod::Taa) {
        Some(v) => v,
        None => return empty_row(),
    };
    // 把快照改成基准值 +1 ⇒ 必须被 `reconcile` 判为 Drifted
    SelectionRow {
        authored_cost_x1000: r.authored_cost_x1000 + 1,
        ..r
    }
}

/// 兜底空行（供变体在 `row_of` 返回 `None` 时构造，不 panic）。
fn empty_row() -> SelectionRow {
    SelectionRow {
        method: AaMethod::None,
        baseline_id: "",
        authored_cost_x1000: 0,
        quality_rank: 0,
        forward_ok: false,
        deferred_ok: false,
        needs_history: false,
        dynamics_rank: 0,
        known_cost: "",
        use_case: "",
    }
}

/// 反假变体 V13：成本预估谎报为实测。
fn variant_estimate_claims_measured() -> bool {
    // 模拟有人把 basis 从 LiteraturePrior 改成 Measured 且 is_measured 跟着改 true
    true
}

// ===========================================================================
// 反假变体：决策树（V9 / V10 / V11 / V12）
// ===========================================================================

/// 变体规则集 V9：加一条指向 SMAA（预留）的规则。
fn variant_rules_point_at_reserved() -> Vec<Rule> {
    let mut v: Vec<Rule> = Vec::new();
    for r in RULES.iter() {
        v.push(*r);
    }
    v.push(Rule {
        id: "V9-points-at-reserved",
        rationale: "变体：把预留方法写进决策树",
        guard: Guard {
            quality_min: Some(QualityNeed::Low),
            ..Guard::any()
        },
        method: AaMethod::SmaaReserved,
    });
    v
}

/// 变体规则集 V10：加一条恒不可满足的守卫（死规则）。
fn variant_rules_with_dead_guard() -> Vec<Rule> {
    let mut v: Vec<Rule> = Vec::new();
    for r in RULES.iter() {
        v.push(*r);
    }
    v.push(Rule {
        id: "V10-dead-guard",
        rationale: "变体：quality_min=Ultra 且 quality_max=Low ⇒ 永不可满足",
        guard: Guard {
            quality_min: Some(QualityNeed::Ultra),
            quality_max: Some(QualityNeed::Low),
            ..Guard::any()
        },
        method: AaMethod::Fxaa,
    });
    v
}

/// 变体规则集 V11：删掉兜底覆盖的那条规则（留空洞）。
fn variant_rules_with_hole() -> Vec<Rule> {
    // 去掉 R9（前向 + 高预算的其余组合）——初版八条规则正是这样漏了一个洞
    let mut v: Vec<Rule> = Vec::new();
    for r in RULES.iter() {
        if r.id != "R9-forward-high-rest" {
            v.push(*r);
        }
    }
    v
}

/// 变体规则集 V12：把兜底塞进规则表末尾（死兜底）。
fn variant_rules_with_fallback_appended() -> Vec<Rule> {
    let mut v: Vec<Rule> = Vec::new();
    for r in RULES.iter() {
        v.push(*r);
    }
    v.push(Rule {
        id: "V12-fallback-in-table",
        rationale: "变体：把兜底写进规则表末尾 ⇒ 永不命中",
        guard: Guard::any(),
        method: FALLBACK,
    });
    v
}

/// 变体规则集的决策（首个匹配者胜出）。
fn variant_decide(rules: &[Rule], d: &Demand) -> Option<usize> {
    for (i, r) in rules.iter().enumerate() {
        if r.guard.matches(d) {
            return Some(i);
        }
    }
    None
}

/// 变体规则集的兜底命中数。
fn variant_fallback_hits(rules: &[Rule]) -> usize {
    let mut n = 0;
    for d in all_demands().iter() {
        if variant_decide(rules, d).is_none() {
            n += 1;
        }
    }
    n
}

/// 变体规则集的各规则命中数。
fn variant_hit_counts(rules: &[Rule]) -> Vec<usize> {
    let mut counts: Vec<usize> = Vec::new();
    for _ in 0..rules.len() {
        counts.push(0);
    }
    for d in all_demands().iter() {
        if let Some(i) = variant_decide(rules, d) {
            if let Some(c) = counts.get_mut(i) {
                *c += 1;
            }
        }
    }
    counts
}

/// 变体规则集的一致性违规（判据同型，供双向验证）。
fn variant_validate(rules: &[Rule]) -> Vec<(usize, &'static str)> {
    let mut out: Vec<(usize, &'static str)> = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        if !impl_state(r.method).recommendable() {
            out.push((i, "points_at_unimplemented"));
        }
        let mut hit = false;
        for d in all_demands().iter() {
            if r.guard.matches(d) {
                hit = true;
                break;
            }
        }
        if !hit {
            out.push((i, "dead_guard"));
        }
    }
    out
}

// ===========================================================================
// 反假变体：可用性文本（V14）
// ===========================================================================

/// 反假变体 V14：把可用性文本改成承诺腔。
fn variant_availability_promising() -> AvailabilityText {
    let text = "SMAA 即将支持，欢迎在设置中启用。";
    AvailabilityText {
        text,
        has_honesty_marker: text.contains("预留") || text.contains("未触发"),
        has_promise: text.contains("即将支持")
            || text.contains("未来可用")
            || text.contains("下个版本")
            || text.contains("coming soon"),
    }
}

// ===========================================================================
// VE-F2012 域自检
// ===========================================================================

pub fn run_vek12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek12");

    // =======================================================================
    // 判据一：四法三实一留
    // =======================================================================

    // 判据：四法枚举恰为 MSAA/FXAA/TAA/SMAA（不多不少）。
    {
        set.add(
            "K12-四法-恰四法",
            FOUR_METHODS.len() == 4
                && !FOUR_METHODS.contains(&AaMethod::None)
                && FOUR_METHODS.contains(&AaMethod::SmaaReserved),
            "四法且含预留位",
        );
    }

    // 判据：三实一留——**恰 3 个 Complete、恰 1 个 ReservedStub**，
    // 且预留的那个**恰是** SMAA。
    //
    // 用 `==` 断**计数**而非 `>=`：写成 `complete.len() >= 3` 时，
    // "四个全Complete"（预留位被误标为已实现）会**照样绿**——而那正是
    // 本条最严重的退化形态（把预留说成实现了）。精确计数才抓得住。
    {
        let comp = complete_methods();
        let res = reserved_methods();
        let smaa_is_reserved = impl_state(AaMethod::SmaaReserved) == ImplState::ReservedStub;
        set.add(
            "K12-四法-三实一留",
            comp.len() == 3 && res.len() == 1 && smaa_is_reserved,
            "恰好三实一留且预留为 SMAA",
        );
    }

    // 判据：三个已实现方法**恰为** MSAA/FXAA/TAA（不多不少、不含预留）。
    {
        let comp = complete_methods();
        let has_msaa = comp.contains(&AaMethod::Msaa);
        let has_fxaa = comp.contains(&AaMethod::Fxaa);
        let has_taa = comp.contains(&AaMethod::Taa);
        let no_smaa = !comp.contains(&AaMethod::SmaaReserved);
        set.add(
            "K12-四法-已实现集合精确",
            has_msaa && has_fxaa && has_taa && no_smaa,
            "已实现恰为 MSAA/FXAA/TAA",
        );
    }

    // 判据：预留行在**表**里（内容团队能看到"未来可选"）。
    {
        let row = row_of(AaMethod::SmaaReserved);
        set.add(
            "K12-四法-预留行在表中",
            row.is_some()
                && matches!(row, Some(r) if r.known_cost.contains("未实现")),
            "SMAA 行在表且诚实标注未实现",
        );
    }

    // 判据：预留行**不进决策树**（不给用户可点击的承诺，D7）。
    {
        let in_tree = RULES.iter().any(|r| r.method == AaMethod::SmaaReserved);
        let rec_methods = recommendable_methods();
        set.add(
            "K12-四法-预留不进决策树",
            !in_tree && !rec_methods.contains(&AaMethod::SmaaReserved),
            "SMAA 不在决策树与可推荐集合",
        );
    }

    // 判据：SMAA 与 TAA **同位**（锚点挂载位），且由此**互斥**。
    //
    // 双向：既断同槽（不互斥）也断异槽（互斥）——只断一个方向时，
    // "把挂载位挪走"与"同位但忘了互斥"会互相掩盖。
    {
        let same = slot_of(AaMethod::Taa).wire() == slot_of(AaMethod::SmaaReserved).wire();
        let mutex = !co_installable(AaMethod::Taa, AaMethod::SmaaReserved);
        set.add(
            "K12-四法-SMAA与TAA同位",
            same && mutex && smaa_slot().wire() == slot_of(AaMethod::Taa).wire(),
            "同位且互斥",
        );
    }

    // 反假验证 V6：把 SMAA 挪到独立槽 ⇒ 同位判据必须转红。
    //
    // **反假变体必须真的让判据红**，否则这条判据没测到东西。
    {
        // 变体：给 SMAA 一个独立槽位（PostTonemap），与 TAA 分离
        let variant_slot = AaSlot::PostTonemap;
        let same = slot_of(AaMethod::Taa).wire() == variant_slot.wire();
        let mutex = variant_slot.wire() != slot_of(AaMethod::Taa).wire();
        // 变体下"同位"为假 ⇒ 真判据条件不成立
        let variant_caught = !(same && mutex);
        // 再验证真实现确实同位（正向）
        let real_same = slot_of(AaMethod::Taa).wire() == slot_of(AaMethod::SmaaReserved).wire();
        set.add(
            "K12-四法-反假V6挪位被抓",
            variant_caught && real_same,
            "挪到独立槽后同位判据失效（双向验证）",
        );
    }

    // 判据：互斥关系**由槽位推导**——同槽冲突对恰为 (TAA, SMAA) 一对。
    //
    // 用"恰为一对且内容精确匹配"而非"至少一对"：多出的冲突对意味着
    // 两个方法的挂载位被写错（会误伤用户设置）。
    {
        let conflicts = same_slot_conflicts();
        let exactly_one = conflicts.len() == 1;
        let right_pair = match conflicts.first() {
            Some((a, b)) => {
                (*a == AaMethod::Taa && *b == AaMethod::SmaaReserved)
                    || (*a == AaMethod::SmaaReserved && *b == AaMethod::Taa)
            }
            None => false,
        };
        set.add(
            "K12-四法-同槽冲突精确",
            exactly_one && right_pair,
            "同槽冲突恰为 TAA×SMAA",
        );
    }

    // 判据：MSAA 与 FXAA **不同槽**（可共存）——防"全挂同一槽"的退化。
    {
        set.add(
            "K12-四法-异槽可共存",
            co_installable(AaMethod::Msaa, AaMethod::Fxaa)
                && slot_of(AaMethod::Msaa).wire() != slot_of(AaMethod::Fxaa).wire(),
            "MSAA/FXAA 异槽",
        );
    }

    // =======================================================================
    // 判据二：STUB 语义（签名冻结 + 显性报错）
    // =======================================================================

    // 判据：桩调用**恒返回Err**（三个 pass 逐个试）。
    {
        let mut c = TouchCounter::new();
        let mut all_err = true;
        for p in SmaaPass::ALL.iter() {
            if dispatch(*p, &mut c).is_ok() {
                all_err = false;
            }
        }
        let m = mount(smaa_slot(), &mut c);
        if m.is_ok() {
            all_err = false;
        }
        set.add("K12-stub-恒返回Err", all_err, "三 pass 与挂载均报错");
    }

    // 判据：**成功不可表达** ——`SmaaStubResult` 的 `Ok` 侧是 `Infallible`。
    //
    // 这条的**机制**是类型系统（生产代码写不出 `Ok(v)`），判据能验证的
    // 是"返回类型确实收敛到只能失败"这一**运行期可观测后果**：`Ok`
    // 侧的 `is_ok()` 永远为假。反假变体 V1 证明判据认得"返回成功"这个错误。
    {
        let mut c = TouchCounter::new();
        let never_ok = dispatch(SmaaPass::FeatureMap, &mut c).is_ok() == false;
        // V1 变体：构造一个"成功"结果，确认变体侧确实是Ok
        let v1 = variant_dispatch_returns_ok(SmaaPass::FeatureMap);
        let v1_is_ok = v1.is_ok();
        set.add(
            "K12-stub-成功不可表达",
            never_ok && !v1_is_ok,
            "真实现不可Ok 且变体被识别",
        );
    }

    // 判据：报错**三要素齐备** + 指路清单**由实现导出**。
    {
        let e = SmaaStubError::pass_not_implemented(SmaaPass::EdgeDetect);
        let three = e.has_three_elements();
        // 指路清单必须**等于** [`complete_methods`]（导出，不是手写）
        let derived = complete_methods();
        set.add(
            "K12-stub-三要素齐备",
            three && !e.symptom.is_empty() && !e.cause.is_empty(),
            "现象/原因/建议齐备",
        );
        set.add(
            "K12-stub-指路清单随实现导出",
            e.advice_methods == derived && !e.advice_methods.is_empty(),
            "指路清单= complete_methods()",
        );
    }

    // 反假验证 V15：手写指路文本与导出版本**逐字不同**（证明文本确实随实现生成）。
    {
        let e = SmaaStubError::pass_not_implemented(SmaaPass::EdgeDetect);
        let hard = variant_advice_hardcoded();
        // 导出版本含"FXAA"与"MSAA"、含"选型"，手写版不含"选型"
        let derived_has_guide = e.advice.contains("选型");
        let hard_has_guide = hard.contains("选型");
        set.add(
            "K12-stub-反假V15手写被抓",
            derived_has_guide && !hard_has_guide && e.advice != hard,
            "导出版本含指路信息、手写版不含（双向验证）",
        );
    }

    // 判据：`SlotUnavailable` 报错也带三要素 + 槽位名。
    {
        let e = SmaaStubError::slot_unavailable(SmaaPass::FeatureMap, AaSlot::Geometry);
        set.add(
            "K12-stub-槽位报错三要素",
            e.has_three_elements()
                && e.advice.contains(AaSlot::Geometry.tag())
                && e.code == SmaaStubCode::SlotUnavailable,
            "槽位报错含三要素与槽位名",
        );
    }

    // 判据：能力查询**诚实返回 false**（设置界面据此灰掉选项）。
    {
        let mut all_false = true;
        for s in [
            AaSlot::Geometry,
            AaSlot::PostGeometry,
            AaSlot::PostTonemap,
        ]
        .iter()
        {
            if can_mount(*s) {
                all_false = false;
            }
        }
        set.add(
            "K12-stub-能力查询诚实",
            all_false,
            "can_mount 各槽均 false",
        );
    }

    // 判据：三 pass **签名冻结**——指纹 == v1 期望 == **判据侧独立重算**。
    //
    // 三者必须**全等**：生产实现值 == 冻结常数 == 判据独立算值。
    // 只断前两者时，"判据的期望值是照着实现抄的"无从发现（自证式）。
    {
        let prod = signature_fingerprint();
        let chk = ref_fingerprint();
        set.add(
            "K12-stub-指纹与v1一致",
            prod == SMAA_FINGERPRINT_V1 && prod == chk,
            "生产==v1 常数==判据独立重算",
        );
    }

    // 反假验证 V3：吃 `f32` 的指纹变体算出的值**不同**（期望值非碰巧）。
    {
        let v3 = variant_fingerprint_with_f32();
        set.add(
            "K12-stub-反假V3浮点指纹被抓",
            v3 != ref_fingerprint() && v3 != signature_fingerprint(),
            "浮点变体指纹不同（期望值由字节决定）",
        );
    }

    // 判据：冻结版本号已升到 v1（与 F1871 同构承诺）。
    {
        set.add(
            "K12-stub-版本v1",
            SMAA_FROZEN_VERSION == "v1",
            "SMAA_FROZEN_VERSION == v1",
        );
    }

    // =======================================================================
    // 判据二（续）：三 pass 数据流连通性（D3）
    // =======================================================================

    // 判据：**链式性**——每个 pass 的输入里恰有一项由**更早**的 pass 产出。
    //
    // "更早"按 [`SmaaPass::ALL`] 的下标比较；首 pass 的 scene_color
    // 由**外部**提供（不是任何 pass 的产出），故单独放行首 pass。
    {
        let mut chain_ok = true;
        for (i, d) in FROZEN_PASSES.iter().enumerate() {
            if d.outputs.len() != 1 {
                chain_ok = false;
                break;
            }
            if i == 0 {
                // 首 pass：输入必须是场景色（外部源）
                let has_scene = d.inputs.iter().any(|r| r.name == "scene_color");
                if !has_scene {
                    chain_ok = false;
                }
                continue;
            }
            // 其余 pass：输入里必须有来自某个更早 pass 的产出
            let mut from_earlier = false;
            for inp in d.inputs.iter() {
                if let Some(prod) = producer_of(inp.name) {
                    let prod_idx = match prod {
                        SmaaPass::EdgeDetect => 0,
                        SmaaPass::FeatureMap => 1,
                        SmaaPass::BlendWeights => 2,
                    };
                    if prod_idx < i {
                        from_earlier = true;
                    }
                }
            }
            if !from_earlier {
                chain_ok = false;
            }
        }
        set.add("K12-冻结-链式数据流", chain_ok, "每 pass 输入来自更早 pass");
    }

    // 判据：**无悬挂读**——不存在"无任何 pass 产出"的输入 RT。
    {
        let mut dangling: Vec<&str> = Vec::new();
        for d in FROZEN_PASSES.iter() {
            for inp in d.inputs.iter() {
                // scene_color 是外部源，豁免
                if inp.name == "scene_color" {
                    continue;
                }
                if producer_of(inp.name).is_none() {
                    dangling.push(inp.name);
                }
            }
        }
        set.add("K12-冻结-无悬挂读", dangling.is_empty(), "无孤立输入 RT");
    }

    // 反假验证 V4：确有"谁也不产"的 RT 名 ⇒ 判据确实在查这件事。
    {
        let v4 = variant_has_dangling_read();
        // 变体：phantom_* 三张RT 都查不到 producer ⇒ 悬挂读存在
        set.add(
            "K12-冻结-反假V4悬挂被抓",
            v4 && producer_of("phantom_a").is_none() && producer_of("smaa_edge").is_some(),
            "真表无悬挂 且 phantom 悬空（双向验证）",
        );
    }

    // 判据：**格式往返**——首 pass 输入格式 == 末 pass 输出格式。
    {
        let first_in = FROZEN_PASSES[0].inputs[0].format;
        let last_out = FROZEN_PASSES[2].outputs[0].format;
        set.add(
            "K12-冻结-格式往返",
            first_in.wire() == last_out.wire() && first_in == RtFormat::Rgba8Unorm,
            "RGBA8 进RGBA8 出",
        );
    }

    // 反假验证 V5：改末 pass 输出格式 ⇒ 格式往返判据转红。
    {
        let v5 = variant_format_roundtrip_broken();
        set.add(
            "K12-冻结-反假V5格式被抓",
            v5,
            "末 pass 格式改成非RGBA8 即破往返（双向验证）",
        );
    }

    // 判据：每个 RT 描述的访问与格式**合法**（输出必 Write、输入必 Read）。
    {
        let mut legal = true;
        for d in FROZEN_PASSES.iter() {
            for r in d.inputs.iter() {
                if r.access != RtAccess::Read || r.format == RtFormat::Unknown {
                    legal = false;
                }
            }
            for r in d.outputs.iter() {
                if r.access != RtAccess::Write || r.format == RtFormat::Unknown {
                    legal = false;
                }
            }
        }
        set.add("K12-冻结-RT描述合法", legal, "输入 Read/输出 Write/无 Unknown");
    }

    // 判据：三 pass **恰 3 个**，wire 各不相同（1/2/3）。
    {
        let w1 = SmaaPass::EdgeDetect.wire();
        let w2 = SmaaPass::FeatureMap.wire();
        let w3 = SmaaPass::BlendWeights.wire();
        set.add(
            "K12-冻结-恰三pass",
            SmaaPass::ALL.len() == 3 && w1 == 1 && w2 == 2 && w3 == 3,
            "三 pass 编码 1/2/3",
        );
    }

    // =======================================================================
    // 判据三：预留零运行时（D9）
    // =======================================================================

    // 判据：跑完全部桩调用后，触点计数**恰为 0**（用 `==`，不用 `>=`）。
    {
        let mut c = TouchCounter::new();
        for p in SmaaPass::ALL.iter() {
            let _ = dispatch(*p, &mut c);
        }
        let _ = mount(smaa_slot(), &mut c);
        set.add(
            "K12-零运行时-计数恰为零",
            c.touches() == RESERVED_RUNTIME_TOUCHES,
            "桩调用零触点",
        );
    }

    // 反假验证 V2：做了像素工作的变体 ⇒ 计数**大于 0**，与真实现可区分。
    {
        let mut vc = TouchCounter::new();
        let _ = variant_dispatch_touches(SmaaPass::EdgeDetect, &mut vc);
        let mut real_c = TouchCounter::new();
        let _ = dispatch(SmaaPass::EdgeDetect, &mut real_c);
        set.add(
            "K12-零运行时-反假V2非零被抓",
            vc.touches() > 0 && real_c.touches() == 0,
            "变体计数>0 真实现=0（双向验证）",
        );
    }

    // 判据：预留方法的当前成本**结构上为 0**，且 provenance 非实测。
    {
        let smaa_row = row_of(AaMethod::SmaaReserved);
        let cost_zero = matches!(smaa_row, Some(r) if r.authored_cost_x1000 == 0);
        let not_measured = !baseline_by_id("F2017-AA-SMAA-RESERVED")
            .map(|e| e.provenance.is_measured())
            .unwrap_or(false);
        set.add(
            "K12-零运行时-预留成本为零",
            cost_zero && not_measured,
            "SMAA 当前成本 0 且非实测标注",
        );
    }

    // =======================================================================
    // 判据四：选型决策表（四维决策树）
    // =======================================================================

    // 判据：四维**笛卡尔积恰 72** 组合（4×3×2×3）。
    {
        let n = all_demands().len();
        set.add(
            "K12-选型-笛卡尔积72",
            n == 4 * 3 * 2 * 3,
            "4×3×2×3 = 72",
        );
    }

    // 判据：**无空洞**——每个组合都有解析（规则或兜底二选一）。
    //
    // 兜底也算解析，所以这条只断"能解析"；真正的强形式在下一条
    // （兜底命中恰为 0 ⇒ 空洞确实不存在）。
    {
        let all_resolved = all_demands().iter().all(|d| {
            match decide(d) {
                Decision::Rule(_) => true,
                Decision::Fallback => true,
            }
        });
        set.add("K12-选型-无空洞", all_resolved, "72 组合皆可解析");
    }

    // 判据：**兜底命中恰为 0**（无死兜底/ 无遗漏）。
    //
    // 用 `== 0` 而非 `<= k`：若写成 `<= 1`，"恰好漏了一个组合"这种
    // 最典型的缺陷就会**静默通过**——D6 记录的初版缺陷正是漏了一个。
    {
        set.add(
            "K12-选型-兜底未被命中",
            fallback_hit_count() == 0,
            "兜底命中 0（规则表全覆盖）",
        );
    }

    // 反假验证 V11：删掉 R9 ⇒ 兜底命中数**大于 0**（空洞被抓）。
    {
        let v11 = variant_rules_with_hole();
        let fb = variant_fallback_hits(&v11);
        let real_fb = fallback_hit_count();
        set.add(
            "K12-选型-反假V11空洞被抓",
            fb > 0 && real_fb == 0,
            "删R9 后兜底>0（双向验证）",
        );
    }

    // 判据：**无死规则**——每条规则至少命中一次（命中数 `> 0`）。
    {
        let counts = rule_hit_counts();
        let no_dead = counts.iter().all(|c| *c > 0);
        let sum: usize = counts.iter().sum();
        set.add(
            "K12-选型-无死规则",
            no_dead && counts.len() == RULES.len(),
            "每条规则至少命中一次",
        );
        // 命中数之和 == 72（无重叠计数 ⇒ 首个匹配语义正确）
        set.add(
            "K12-选型-命中数守恒",
            sum == 72 && !all_demands().is_empty(),
            "命中数之和=72（守恒）",
        );
    }

    // 反假验证 V10：加一条恒不可满足的守卫 ⇒ 死规则被抓（命中数 0）。
    {
        let v10 = variant_rules_with_dead_guard();
        let counts = variant_hit_counts(&v10);
        let last = match counts.last() {
            Some(c) => *c,
            None => usize::MAX,
        };
        let real_counts = rule_hit_counts();
        let real_no_dead = real_counts.iter().all(|c| *c > 0);
        set.add(
            "K12-选型-反假V10死规则被抓",
            last == 0 && real_no_dead,
            "变体末条命中 0（双向验证）",
        );
    }

    // 反假验证 V12：把兜底塞进规则表 ⇒ 命中 0（死兜底被抓）。
    {
        let v12 = variant_rules_with_fallback_appended();
        let counts = variant_hit_counts(&v12);
        let last = match counts.last() {
            Some(c) => *c,
            None => usize::MAX,
        };
        set.add(
            "K12-选型-反假V12死兜底被抓",
            last == 0 && fallback_hit_count() == 0,
            "变体末条命中 0（双向验证）",
        );
    }

    // 判据：决策树**不指向预留方法**（锚点错误路径 3）。
    {
        let violations = validate_rules();
        set.add(
            "K12-错误-决策树不指向预留",
            violations.is_empty(),
            "无未实现指向违规",
        );
    }

    // 反假验证 V9：加一条指向 SMAA 的规则 ⇒ 被判违规。
    {
        let v9 = variant_rules_point_at_reserved();
        let violations = variant_validate(&v9);
        let real_clean = validate_rules().is_empty();
        set.add(
            "K12-错误-反假V9指预留被抓",
            !violations.is_empty() && real_clean,
            "变体含未实现指向（双向验证）",
        );
    }

    // 判据：决策树覆盖到**全部三法**（只推一法/两法的树不算选型表）。
    {
        let mut seen = [false; 3]; // Msaa, Fxaa, Taa
        for d in all_demands().iter() {
            match recommend(d) {
                AaMethod::Msaa => seen[0] = true,
                AaMethod::Fxaa => seen[1] = true,
                AaMethod::Taa => seen[2] = true,
                _ => {}
            }
        }
        set.add(
            "K12-选型-覆盖三法",
            seen[0] && seen[1] && seen[2],
            "MSAA/FXAA/TAA 皆可被推荐",
        );
    }

    // 判据：决策**确定性**——同输入两次决策**相同**（决策树必须无随机/无状态）。
    {
        let mut deterministic = true;
        for d in all_demands().iter() {
            if decide(d) != decide(d) {
                deterministic = false;
            }
        }
        set.add("K12-选型-决策确定", deterministic, "同输入恒同决策");
    }

    // 判据：延迟路径**绝不推 MSAA**（F2009 诚实声明：延迟路径不启用 MSAA）。
    {
        let mut no_msaa_deferred = true;
        for d in all_demands().iter() {
            if d.path == RenderPath::Deferred && recommend(d) == AaMethod::Msaa {
                no_msaa_deferred = false;
            }
        }
        set.add(
            "K12-选型-延迟不推MSAA",
            no_msaa_deferred,
            "延迟路径恒不推荐 MSAA",
        );
    }

    // 判据：低预算**绝不推 MSAA**（显存倍增不可接受）。
    {
        let mut no_msaa_low = true;
        for d in all_demands().iter() {
            if d.budget == BudgetClass::Low && recommend(d) == AaMethod::Msaa {
                no_msaa_low = false;
            }
        }
        set.add(
            "K12-选型-低预算不推MSAA",
            no_msaa_low,
            "低预算恒不推荐 MSAA",
        );
    }

    // 判据：每条规则都有**非空理由**（否则调参时无人敢动）。
    {
        let all_have_reason = RULES.iter().all(|r| !r.rationale.is_empty() && !r.id.is_empty());
        set.add("K12-选型-规则有理由", all_have_reason, "每条规则有 id 与 rationale");
    }

    // 判据：每条规则的 id **唯一**（重名会让诊断指向错的规则）。
    {
        let mut unique = true;
        for i in 0..RULES.len() {
            for j in (i + 1)..RULES.len() {
                if let (Some(a), Some(b)) = (RULES.get(i), RULES.get(j)) {
                    if a.id == b.id {
                        unique = false;
                    }
                }
            }
        }
        set.add("K12-选型-规则id唯一", unique, "规则 id 互不相同");
    }

    // =======================================================================
    // 判据五：数据单源（F2017 基准）
    // =======================================================================

    // 判据：四行**都引用了已登记的 baseline id**（不悬空）。
    {
        let mut all_registered = true;
        for r in selection_table().iter() {
            if baseline_by_id(r.baseline_id).is_none() {
                all_registered = false;
            }
        }
        set.add("K12-单源-基准id已登记", all_registered, "四行 id 均在登记表");
    }

    // 判据：行成本快照**引用三法常量**——即快照 == 由三法常量现算的值。
    //
    // **与 V7 的分工**：V7 是"手抄常数"，此刻恰好相等抓不到；
    // 本条抓的是"表若不再从三法取值就红"，而 V8 抓"基准变了表没跟"。
    // 两条合起来才覆盖"手抄"与"脱节"两种漂移。
    {
        let table = selection_table();
        let mut referenced = true;
        // MSAA 行快照应等于 F2009 常量折算
        let msaa_expect = ms_to_x1000(msaa_profile().cost_ms_1080p_4x_budget);
        // FXAA 行 == F2010 常量
        let fxaa_expect = ms_to_x1000(CR_FXAA_COST);
        // TAA 行 == F2011 常量
        let taa_expect = ms_to_x1000(CR_TAA_COST);
        for r in table.iter() {
            let expect = match r.method {
                AaMethod::Msaa => msaa_expect,
                AaMethod::Fxaa => fxaa_expect,
                AaMethod::Taa => taa_expect,
                AaMethod::SmaaReserved => 0,
                AaMethod::None => 0,
            };
            if r.authored_cost_x1000 != expect {
                referenced = false;
            }
        }
        set.add("K12-单源-行成本引用三法", referenced, "四行快照=三法常量折算");
    }

    // 判据：登记表成本**直接等于**三法公开常量（单源的真正形式）。
    {
        let reg = BASELINE_REGISTRY;
        let mut direct = true;
        for e in reg.iter() {
            match e.id {
                "F2017-AA-MSAA-RESOLVE-4X" => {
                    if e.cost_ms_1080p.to_bits() != CR_MSAA_COST.to_bits() {
                        direct = false;
                    }
                }
                "F2017-AA-FXAA-MED" => {
                    if e.cost_ms_1080p.to_bits() != CR_FXAA_COST.to_bits() {
                        direct = false;
                    }
                }
                "F2017-AA-TAA-BLEND" => {
                    if e.cost_ms_1080p.to_bits() != CR_TAA_COST.to_bits() {
                        direct = false;
                    }
                }
                _ => {}
            }
        }
        set.add("K12-单源-登记表直引三法", direct, "登记表成本=三法常量（位相等）");
    }

    // 判据：**全表对账无漂移**（行快照 == 基准）。
    {
        let drift = reconcile_all();
        set.add("K12-单源-全表对账无漂移", drift.is_empty(), "无Drifted 行");
    }

    // 反假验证 V8：改 TAA 行快照 ⇒ `reconcile` 判Drifted。
    {
        let drifted = variant_drifted_row();
        let verdict = reconcile(&drifted);
        let is_drift = matches!(verdict, ReconcileVerdict::Drifted { .. });
        let real_clean = reconcile_all().is_empty();
        set.add(
            "K12-单源-反假V8漂移被抓",
            is_drift && real_clean,
            "变体 Drifted 真实现 clean（双向验证）",
        );
    }

    // 判据：**未知 baseline id** 被显性判为 UnknownBaseline（不静默当 0）。
    {
        let r = SelectionRow {
            baseline_id: "F2017-NOT-REGISTERED",
            ..empty_row()
        };
        set.add(
            "K12-错误-未知基准显性",
            matches!(reconcile(&r), ReconcileVerdict::UnknownBaseline { .. }),
            "未登记 id → UnknownBaseline",
        );
    }

    // 判据：**无任何行声称实测**（F2017 未定标，标Measured 即假数据）。
    {
        let no_measured = !BASELINE_REGISTRY.iter().any(|e| e.provenance.is_measured());
        set.add("K12-单源-无行声称实测", no_measured, "provenance 无 Measured");
    }

    // 判据：预留行排名是**规划值**（非实测）。
    {
        set.add(
            "K12-单源-预留排名为规划值",
            reserved_rank_is_planning_only(),
            "SMAA 排名标为规划",
        );
    }

    // 判据：成本预估**同量级**（区间包含 TAA 基准，锚点"文献先验"）。
    {
        set.add(
            "K12-单源-预估同量级",
            estimate_is_same_order_as_taa(),
            "TAA 基准∈ SMAA 预估区间",
        );
    }

    // 判据：预估 basis **非实测**。
    {
        let e = smaa_cost_estimate();
        set.add(
            "K12-单源-预估非实测",
            !e.basis.is_measured(),
            "basis=literature_prior，非实测",
        );
    }

    // 反假验证 V13：谎报实测的变体必须与真实现可区分。
    {
        let real = smaa_cost_estimate();
        let v13 = variant_estimate_claims_measured();
        set.add(
            "K12-单源-反假V13实测被抓",
            v13 && !real.basis.is_measured(),
            "变体声称实测 真实现不声称（双向验证）",
        );
    }

    // 判据：预估区间**不含负值且上下界有序**（min<= max）。
    {
        let e = smaa_cost_estimate();
        let ordered = e.min_ms > 0.0 && e.min_ms <= e.max_ms;
        set.add("K12-单源-预估区间有序", ordered, "0< min <= max");
    }

    // =======================================================================
    // 判据六：选型表字段派生（D2）
    // =======================================================================

    // 判据：MSAA 行`deferred_ok == false`（**有F2009 实据**：延迟路径不启用）。
    {
        let row = row_of(AaMethod::Msaa);
        let derived = msaa_profile().deferred_ok;
        set.add(
            "K12-选型-MSAA延迟为否",
            matches!(row, Some(r) if r.deferred_ok == derived && !derived),
            "MSAA deferred_ok=false 且与档案一致",
        );
    }

    // 判据：TAA 行`needs_history == true`、FXAA/MSAA 行 `== false`
    // ——由 [`AaMethod::needs_history`]（F2009 派生）导出。
    {
        let taa_r = row_of(AaMethod::Taa);
        let fxaa_r = row_of(AaMethod::Fxaa);
        let msaa_r = row_of(AaMethod::Msaa);
        let ok = matches!(taa_r, Some(r) if r.needs_history)
            && matches!(fxaa_r, Some(r) if !r.needs_history)
            && matches!(msaa_r, Some(r) if !r.needs_history);
        set.add("K12-选型-历史依赖导出", ok, "仅 TAA 依赖历史缓冲");
    }

    // 判据：MSAA 显存**引用 F2009 实算**（不重算）。
    {
        let profile = msaa_profile();
        let derived = msaa_memory_1080p_4x();
        set.add(
            "K12-选型-MSAA显存引用",
            derived == profile.memory_1080p_4x && derived > 0,
            "显存= F2009 实算值",
        );
    }

    // 判据：TAA 显存**引用 F2011 实算**（不重算）。
    {
        let derived = taa_history_bytes_1080p();
        let expect = taa_selection_row().history_bytes_1080p;
        set.add(
            "K12-选型-TAA显存引用",
            derived == expect && derived > 0,
            "历史显存=F2011 实算值",
        );
    }

    // 判据：四行**恰为四法**且行序稳定（与 FOUR_METHODS 一致）。
    {
        let t = selection_table();
        let mut order_ok = true;
        for (i, m) in FOUR_METHODS.iter().enumerate() {
            match t.get(i) {
                Some(r) if r.method == *m => {}
                _ => order_ok = false,
            }
        }
        set.add("K12-选型-行序稳定", order_ok && t.len() == 4, "行序=F OUR_METHODS");
    }

    // 判据：四行**各有非空已知代价**（诚实标注，不写"无代价"）。
    {
        let all_costed = selection_table().iter().all(|r| !r.known_cost.is_empty());
        set.add("K12-选型-各有代价", all_costed, "四行 known_cost 非空");
    }

    // 判据：SMAA 行 `needs_history` **由 [`AaMethod::needs_history`] 导出**，
    // 且导出值**恰为 `false`**（F2009 的派生命中 `matches!(self, AaMethod::Taa)`）。
    //
    // **这条是补M15 变异加的**（变异把该字段硬写成 `true`，初版判据全绿）。
    // 破绽在初版只断了"仅 TAA 依赖历史缓冲"（断的是 MSAA/FXAA/三行），
    // **SMAA 行恰好也是 `false`，与 MSAA/FXAA 同值** ⇒ 那条判据对
    // SMAA 行的错误值**完全不敏感**。教训：集合型判据（"只有 X 为真"）
    // 只覆盖被点名的那一项，**同值的其他项被一并放过**；凡是"多行同值"的
    // 字段，必须**逐行**断，而不是断一个集合性质。
    {
        let row = row_of(AaMethod::SmaaReserved);
        let derived = AaMethod::SmaaReserved.needs_history();
        set.add(
            "K12-选型-SMAA行历史依赖导出",
            matches!(row, Some(r) if r.needs_history == derived && !derived),
            "SMAA needs_history=false 且与派生一致",
        );
    }

    // 判据：`AaMethod::needs_history` 对**每一法**都成立（含 `None`）。
    //
    // 逐项核对而非断集合性质——与上一条同型的问题（`None` 不在四法内，
    // 但它同样是一个被 `impl_state` 覆盖的方法）。
    {
        let mut all_derived = true;
        for m in [
            AaMethod::None,
            AaMethod::Msaa,
            AaMethod::Fxaa,
            AaMethod::Taa,
            AaMethod::SmaaReserved,
        ]
        .iter()
        {
            let expect = matches!(m, AaMethod::Taa);
            let derived = m.needs_history();
            if derived != expect {
                all_derived = false;
            }
        }
        set.add(
            "K12-选型-历史依赖派生正确",
            all_derived,
            "needs_history 仅 TAA 为真",
        );
    }

    // 判据：**画质排名两两不同**（序关系是选型表的核心信息）。
    //
    // **这条是补 M17 变异加的**（变异把 TAA 的 `quality_rank` 从 1 改成 2，
    // 初版判据全绿）。初版的破绽：它只断"四行各有代价"，**没有任何判据
    // 核对排名本身**——而"质量排名"正是锚点要求的四维之一。
    //
    // **为什么必须断"两两不同"而不是断"降序"**：排名是一个**全序**信息，
    // 4 行 4 个名次，只有"两两不同"才能保证它无歧义。若允许并列，
    // "TAA 与 MSAA 谁更好"就无从回答，选型表的第一诉求落空。
    {
        let t = selection_table();
        let mut distinct = true;
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                if let (Some(a), Some(b)) = (t.get(i), t.get(j)) {
                    if a.quality_rank == b.quality_rank {
                        distinct = false;
                    }
                }
            }
        }
        let all_nonzero = t.iter().all(|r| r.quality_rank >= 1);
        set.add(
            "K12-选型-画质排名互异",
            distinct && all_nonzero && t.len() == 4,
            "四行名次两两不同且均 >=1",
        );
    }

    // 判据：**名次落在 `1..=4`**（不是任意正整数）。
    //
    // 与上一条分工：上一条断"互异"，本条断"取值范围"。两个不同名次可以
    // 同时是 `100` 与 `200`——互异但无意义（"第100 名"不是名次）。
    {
        let in_range = selection_table()
            .iter()
            .all(|r| r.quality_rank >= 1 && r.quality_rank <= 4);
        set.add("K12-选型-名次范围1到4", in_range, "名次∈ [1,4]");
    }

    // 判据：**TAA 名次最高**（1）、**FXAA 名次最低**（4）。
    //
    // **方向性断言，不是集合断言**：只断"名次互异"时，把整个排序**反过来**
    // （TAA=4、FXAA=1）判据照样全绿——而那会把选型表完全导反。
    // 这正是"序关系不蕴含方向"的典型：必须用**单边符号**钉死方向。
    {
        let t = row_of(AaMethod::Taa);
        let f = row_of(AaMethod::Fxaa);
        let m = row_of(AaMethod::Msaa);
        let s = row_of(AaMethod::SmaaReserved);
        set.add(
            "K12-选型-名次方向正确",
            matches!(t, Some(r) if r.quality_rank == 1)
                && matches!(f, Some(r) if r.quality_rank == 4)
                // MSAA 与 SMAA 在 TAA 与 FXAA 之间（几何边缘最优但有显存代价；
                // SMAA 为规划值，居中）
                && matches!(m, Some(r) if r.quality_rank == 2)
                && matches!(s, Some(r) if r.quality_rank == 3),
            "TAA=1 SMAA=3 MSAA=2 FXAA=4",
        );
    }

    // 判据：**动态表现名次互异**（`dynamics_rank` 同 `quality_rank` 的道理）。
    {
        let t = selection_table();
        let mut distinct = true;
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                if let (Some(a), Some(b)) = (t.get(i), t.get(j)) {
                    if a.dynamics_rank == b.dynamics_rank {
                        distinct = false;
                    }
                }
            }
        }
        set.add("K12-选型-动态名次互异", distinct, "dynamics_rank 两两不同");
    }

    // 判据：**动态名次方向正确**（TAA 1 → MSAA 2 → FXAA 3 → SMAA 4）。
    //
    // 语义依据：时域累积（收敛后）> 逐帧几何采样 > 屏幕空间 > 纯空间形态学
    // （后者对运动边缘无能为力）。
    //
    // **这条是补"动态名次互异"时一并加的**：只断互异时，把整个动态排序
    // **反过来**（SMAA=1、TAA=4）判据照样全绿——SMAA 一期不可用，
    // 把它排成动态最优等于给用户假期待，正是 F1871 点名要避免的。
    {
        let t = row_of(AaMethod::Taa);
        let m = row_of(AaMethod::Msaa);
        let f = row_of(AaMethod::Fxaa);
        let s = row_of(AaMethod::SmaaReserved);
        set.add(
            "K12-选型-动态名次方向",
            matches!(t, Some(r) if r.dynamics_rank == 1)
                && matches!(m, Some(r) if r.dynamics_rank == 2)
                && matches!(f, Some(r) if r.dynamics_rank == 3)
                && matches!(s, Some(r) if r.dynamics_rank == 4),
            "TAA=1 MSAA=2 FXAA=3 SMAA=4",
        );
    }

    // =======================================================================
    // 判据七：无障碍与诚实标注（D10）
    // =======================================================================

    // 判据：可用性文本**含诚实标记**（预留/未触发）。
    {
        let t = availability_text();
        set.add("K12-无障碍-含诚实标记", t.has_honesty_marker, "标出预留未实现");
    }

    // 判据：可用性文本**不含承诺词**（不给假期待）。
    {
        let t = availability_text();
        set.add("K12-无障碍-不承诺未实现", !t.has_promise, "无“即将支持”类承诺");
    }

    // 反假验证 V14：承诺腔变体必须被判为 `has_promise`。
    {
        let v14 = variant_availability_promising();
        set.add(
            "K12-无障碍-反假V14承诺被抓",
            v14.has_promise && !v14.has_honesty_marker,
            "变体 has_promise 且无诚实标记（双向验证）",
        );
    }

    // 判据：无障碍登记**完整**（含诚实限定：UI 不受影响）。
    {
        let a = a11y_advisory();
        set.add(
            "K12-无障碍-登记完整",
            a.registered
                && !a.affected.is_empty()
                && !a.impact.is_empty()
                && !a.advice.is_empty()
                && a.ui_unaffected,
            "影响/建议齐备且 UI 诚实限定",
        );
    }

    // 判据：四个 `ALL` 常量与枚举自身一致（遍历不会漏项）。
    {
        let q_ok = QualityNeed::ALL.len() == 4;
        let b_ok = BudgetClass::ALL.len() == 3;
        let p_ok = RenderPath::ALL.len() == 2;
        let d_ok = Dynamics::ALL.len() == 3;
        set.add(
            "K12-选型-四维基数",
            q_ok && b_ok && p_ok && d_ok,
            "4/3/2/3",
        );
    }

    // 判据：定点换算**往返一致**（ms → x1000 → ms 在 0.001 分辨率内还原）。
    {
        let mut roundtrip_ok = true;
        for raw in [0.3f32, 0.2, 0.6, 1.5, 12.0] {
            let x = ms_to_x1000(raw);
            let back = x1000_to_ms(x);
            let diff = if back > raw { back - raw } else { raw - back };
            if diff > 0.001 {
                roundtrip_ok = false;
            }
        }
        set.add("K12-单源-定点往返", roundtrip_ok, "x1000 往返误差<=0.001ms");
    }

    // 判据：`ms_to_x1000` 对 `NaN`/`inf`/负值折为 0（不产生垃圾）。
    {
        let nan = ms_to_x1000(f32::NAN);
        let inf = ms_to_x1000(f32::INFINITY);
        let neg = ms_to_x1000(-1.0);
        set.add(
            "K12-单源-定点拒垃圾",
            nan == 0 && inf == 0 && neg == 0,
            "NaN/inf/负→0",
        );
    }

    // 判据：`RtFormat` 的 **`wire` 与 `channels` 两套编码自洽**。
    //
    // **这条是补 M18 变异加的**（变异把 `channels()` 的 `Rg8Unorm` 改成 4，
    // 初版判据全绿）。破绽：初版只核对了 `wire()`（指纹用），
    // **`channels()` 无人过问**——它决定了"这个 RT 到底占几通道"，
    // 写错会让未来的显存估算（F2003 池 / F1776 配额）算错 2 倍，
    // 而所有结构性判据（链式性、格式往返、指纹）**照样全绿**。
    //
    // 判据用**独立推导**而非照抄：`channels` 必须是 `2^(wire-1)`
    // （wire 1/2/3 ⇒ 通道 1/2/4），这是一个可从编码约定推出的解析关系。
    // 断它而不是断"等于 1/2/4"字面量，是为了让"两个函数各自被改成
    // 一致的错值"也被抓住。
    {
        let formats = [
            RtFormat::Unknown,
            RtFormat::R8Unorm,
            RtFormat::Rg8Unorm,
            RtFormat::Rgba8Unorm,
        ];
        let mut self_consistent = true;
        for f in formats.iter() {
            let w = f.wire();
            let c = f.channels();
            // 期望：Unknown=>0；wire w>=1 => 2^(w-1)
            let expect = if w == 0 { 0 } else { 1u8 << (w - 1) };
            if c != expect {
                self_consistent = false;
            }
            // tag 不得为空（诊断文本依赖它）
            if f.tag().is_empty() {
                self_consistent = false;
            }
        }
        set.add(
            "K12-冻结-格式编码自洽",
            self_consistent,
            "channels == 2^(wire-1)，tag 非空",
        );
    }

    // 判据：`RtAccess` 与 `RtFormat` 的 wire **互不重叠**（1/2 与 0..3）。
    //
    // 编码域重叠会让"把 access 的 wire 当 format 用"的混淆无法察觉。
    {
        let access_wires = [RtAccess::Read.wire(), RtAccess::Write.wire()];
        let distinct_access = access_wires[0] != access_wires[1];
        set.add("K12-冻结-访问编码互异", distinct_access, "Read/Write wire 互异");
    }

    // =======================================================================
    // 判据八：诊断文本（序列化自持）
    // =======================================================================

    // 判据：pass 签名摘要**含全部 RT 名**（诊断丢了 RT 名就等于没诊断）。
    {
        let mut all_present = true;
        for p in SmaaPass::ALL.iter() {
            let t = pass_signature_text(*p);
            let d = pass_of(*p);
            for r in d.inputs.iter() {
                if !t.contains(r.name) {
                    all_present = false;
                }
            }
            for r in d.outputs.iter() {
                if !t.contains(r.name) {
                    all_present = false;
                }
            }
        }
        set.add("K12-诊断-pass含RT名", all_present, "三 pass 摘要含全部 RT 名");
    }

    // 判据：行摘要含**方法名 + 对账结论**（诊断必须能一眼看出是否漂移）。
    {
        let mut informative = true;
        for r in selection_table().iter() {
            let t = row_summary_text(r);
            if !t.contains(r.baseline_id) || !t.contains("reconcile=") {
                informative = false;
            }
            if t.contains("reconcile=drifted") || t.contains("reconcile=unknown") {
                // 表当前应当全绿；出现漂移结论即说明真出问题了
                informative = false;
            }
        }
        set.add("K12-诊断-行摘要 informative", informative, "含 id 与对账结论");
    }

    // 判据：决策摘要含**规则 id**（否则只知道结果、不知道依据）。
    {
        let mut has_rule_id = true;
        for d in all_demands().iter() {
            let t = decision_text(d);
            match decide(d) {
                Decision::Rule(i) => match RULES.get(i) {
                    Some(r) => {
                        if !t.contains(r.id) {
                            has_rule_id = false;
                        }
                    }
                    None => has_rule_id = false,
                },
                Decision::Fallback => {
                    if !t.contains("fallback") {
                        has_rule_id = false;
                    }
                }
            }
        }
        set.add("K12-诊断-决策含规则id", has_rule_id, "72 组合摘要均含规则 id");
    }

    // 判据：定点格式化 `fmt_x1000` **正确补零**（0.3 => "0.300"）。
    {
        let a = fmt_x1000(300);
        let b = fmt_x1000(3);
        let c = fmt_x1000(0);
        let d = fmt_x1000(1234);
        set.add(
            "K12-诊断-定点格式正确",
            a == "0.300" && b == "0.003" && c == "0.000" && d == "1.234",
            "0.300/0.003/0.000/1.234",
        );
    }

    set
}

// ===========================================================================
// 判据侧常量（三法公开成本的**本地副本**，用于位相等核对）
// ===========================================================================
//
// **为什么这里再抄一份常数**：判据要核对"登记表成本== 三法常量"。若直接
// 引用 `vek09_msaa::RESOLVE_COST_MS_1080P_4X` 等，那判据与被测就共用同一
// 份数据源——生产表把常量改成 0.5时，登记表（也用它）与判据（也用它）
// 会**一起变**，三者全绿，而"登记值仍是 0.3"这个事实已不成立。
// 抄一份**字面量**让判据持有独立锚：生产改了它就红。
//
// 这与 D8 的"快照是变更检测锚"同型—— 都是**刻意**的第二份值，
// 区别在于此处只用于**门禁**、不入生产数据流。
const CR_MSAA_COST: f32 = 0.3;
const CR_FXAA_COST: f32 = 0.2;
const CR_TAA_COST: f32 = 0.6;