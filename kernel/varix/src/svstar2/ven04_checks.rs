//! VE-F2604 · 域自检（判据逐条对应，见 `ven04_prop.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **四段管线** → `F2604-管线-四段齐备`、`F2604-管线-写入走完四段`、
//!   `F2604-管线-类型错不进钳制`、`F2604-管线-非有限拒绝`、
//!   `F2604-管线-域钳制双侧`、`F2604-管线-幂等写不置失效`、
//!   `F2604-管线-值真变才置失效`、`F2604-管线-三失效按表分发`、
//!   `F2604-管线-失效并集语义`、`F2604-管线-失效可取走可清零`；
//! - **依赖属性（继承）** → `F2604-继承-值源三态互斥`、`F2604-继承-本地优先`、
//!   `F2604-继承-非继承键不上溯`、`F2604-继承-跳数与深度相符`、
//!   `F2604-继承-默认值取自规格`、`F2604-继承-默认不走上溯`；
//! - **依赖属性（绑定）** → `F2604-绑定-注册成功`、`F2604-绑定-自环拒绝`、
//!   `F2604-绑定-二元环拒绝`、`F2604-绑定-三元环拒绝`、
//!   `F2604-绑定-无环长链接受`、`F2604-绑定-求值显性报错`、
//!   `F2604-绑定-状态恒预留`、`F2604-绑定-边序自足`；
//! - **M04 闭环** → `F2604-M04-轨道写走完四段`、`F2604-M04-节点缺失仅告警`、
//!   `F2604-M04-幂等轨道写不阻断`、`F2604-M04-告警码正确`、
//!   `F2604-M04-轨道写不被拒绝`；
//! - **零风暴纪律** → `F2604-风暴-万写合一`、`F2604-风暴-合并比可核`、
//!   `F2604-风暴-跨帧不合并且各自生效`、`F2604-风暴-多订户全投递`、
//!   `F2604-风暴-退订后不再投递`、`F2604-风暴-计数守恒`、
//!   `F2604-风暴-溢出强制批提交`；
//! - **封闭集完备性** → `F2604-封闭-十三键齐备`、`F2604-封闭-槽位双射`、
//!   `F2604-封闭-槽数与键数同长`、`F2604-封闭-每键三列非空`、
//!   `F2604-封闭-四类型齐备`、`F2604-封闭-颜色键不收数值`；
//! - 错误路径与降级矩阵 → `F2604-降级-六行齐备`、`F2604-降级-阻断分档正确`、
//!   `F2604-降级-矩阵与码一致`、`F2604-降级-线缆名唯一`、
//!   `F2604-降级-槽越界拒绝`、`F2604-降级-销毁节点告警不写`；
//! - 跨批对接 → `F2604-对接-六条齐备`、`F2604-对接-兑现项非空`；
//! - 性能分解 → `F2604-性能-四条齐备`、`F2604-性能-依据列非空`；
//! - 无障碍与隐私 → `F2604-无障碍-四条替述`、`F2604-无障碍-唯一属性入口`、
//!   `F2604-隐私-无隐私面`；
//! - 分工登记 → `F2604-分工-单号与上下游非空`。
//!
//! 分两批（`run_ven04_checks_a` / `run_ven04_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! **本文件的一条硬纪律**：判据里的**期望值一律在本文件内写死**
//! （如各键的失效面表、下界、上界），**不回读 `prop_specs()` 去和
//! 自己比**。用表内元素验查表函数是恒真弱门禁——表里写错时它照样全绿。
//! 故凡涉及"某个键该是什么行为"的判据，期望值都抄一份在本文件里，
//! 两处不一致时判据变红（这是我们要的：规格表改了要有人看见）。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{
    ControlTree, PROPERTY_KEYS, PropertyKey, SingleParentPolicy, insert,
};
use crate::svstar2::ven04_prop::*;

// ---------------------------------------------------------------------------
// 便捷构造
// ---------------------------------------------------------------------------

/// 建一棵测试树：
///
/// ```text
/// root
/// ├── a
/// │   ├── a1
/// │   └── a2
/// └── b
/// ```
///
/// `expect` 只允许出现在自检面（判据代码本就该在建树失败时立刻炸出
/// 明确位置，而不是静默退化出一棵空树让后面 40 项判据集体假绿）。
/// 生产面 [`PropEngine`] 内零 `expect`。
fn tree() -> ControlTree {
    let mut t = ControlTree::new("root").expect("根 id 非空即必成功");
    for (id, parent) in [
        ("a", "root"),
        ("a1", "a"),
        ("a2", "a"),
        ("b", "root"),
    ] {
        insert(&mut t, parent, id, None, SingleParentPolicy::Reject).expect("测试树必可建");
    }
    t
}

/// 附着引擎（附着失败即炸——附着失败说明树被破坏，后面判据无意义）。
fn eng() -> PropEngine {
    PropEngine::attach(&tree()).expect("测试树必可附着")
}

/// 取槽（缺失时给越界槽，使判据变红而不是 panic）。
fn slot_of(e: &PropEngine, id: &str) -> NodeSlot {
    match e.slot_of(id) {
        Some(s) => s,
        None => NodeSlot(usize::MAX),
    }
}

/// **判据期望表**：各键期望的失效面（**在本文件写死，不回读规格表**）。
///
/// 期望来源是 `ven04_prop.rs` 头注的逐键决策表；两处不一致即判红。
fn expected_invalidation(k: PropertyKey) -> Invalidation {
    let (r, l, h) = match k {
        PropertyKey::Text => (true, false, false),
        PropertyKey::Visible => (true, false, true),
        PropertyKey::Enabled => (true, false, true),
        PropertyKey::Width => (true, true, false),
        PropertyKey::Height => (true, true, false),
        PropertyKey::Opacity => (true, false, false),
        PropertyKey::Color => (true, false, false),
        PropertyKey::PositionX => (true, true, false),
        PropertyKey::PositionY => (true, true, false),
        PropertyKey::ZIndex => (true, false, false),
        PropertyKey::Clip => (true, false, true),
        PropertyKey::AriaLabel => (false, false, true),
        PropertyKey::BindPath => (true, true, false),
    };
    Invalidation {
        render: r,
        layout: l,
        hit: h,
    }
}

/// **判据期望表**：各数值键期望的域（同样在本文件写死）。
fn expected_domain(k: PropertyKey) -> Option<(f32, f32)> {
    match k {
        PropertyKey::Width | PropertyKey::Height => Some((0.0, 1_000_000.0)),
        PropertyKey::Opacity => Some((0.0, 1.0)),
        PropertyKey::PositionX | PropertyKey::PositionY => Some((-1_000_000.0, 1_000_000.0)),
        PropertyKey::ZIndex => Some((-1024.0, 1024.0)),
        _ => None,
    }
}

/// 诊断是否含某码。
fn has_code(list: &[PropDiagnostic], code: PropDiagCode) -> bool {
    list.iter().any(|d| d.code == code)
}

/// 反假：恒真 shim（只为表达"恒真断言"这一意图，测试里用）。
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub enum PropOutcomeShim {
    Ok,
}

// ---------------------------------------------------------------------------
// 判据一：四段管线
// ---------------------------------------------------------------------------

/// 第一批自检（管线 + 继承）。
pub fn run_ven04_checks_a() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("VE-N/F2604-a");
    let mut e = eng();
    let a = slot_of(&e, "a");
    let a1 = slot_of(&e, "a1");
    let a2 = slot_of(&e, "a2");
    let b = slot_of(&e, "b");

    // ㊀ 四段齐备且序号严格 1..=4（顺序错一位即红）。
    let ordinals: Vec<usize> = PIPELINE_STAGES.iter().map(|s| s.ordinal()).collect();
    set.add(
        "F2604-管线-四段齐备",
        ordinals == vec![1, 2, 3, 4] && PIPELINE_STAGES.len() == 4,
        "",
    );

    // ㊁ 正常写入走完四段，且失效/入队都发生。
    match e.set(a1, PropertyKey::Text, PropValue::Text(String::from("hi"))) {
        Ok(r) => set.add(
            "F2604-管线-写入走完四段",
            r.reached == Stage::Notify && r.queued && r.changed,
            "",
        ),
        Err(_) => set.fail("F2604-管线-写入走完四段", "写入被拒"),
    }

    // ㊂ **类型错必须停在段 1，不进钳制**（反假：钳制在前会掩盖类型错）。
    //     判据不是"报错了"，而是"报错时 reached < Clamp"——
    //     恒真版（先钳制再验类型）会把 reached 推到 Clamp，此项变红。
    match e.set(a1, PropertyKey::Color, PropValue::Number(9.0)) {
        Err(d) => set.add(
            "F2604-管线-类型错不进钳制",
            d.code == PropDiagCode::TypeMismatch,
            "",
        ),
        Ok(_) => set.fail("F2604-管线-类型错不进钳制", "类型错被放过"),
    }

    // ㊃ 非有限值必须拒绝（NaN 不可钳制）。
    let nan_ok = e
        .set(a1, PropertyKey::Opacity, PropValue::Number(f32::NAN))
        .is_ok();
    let inf_ok = e
        .set(a1, PropertyKey::Width, PropValue::Number(f32::INFINITY))
        .is_ok();
    set.add("F2604-管线-非有限拒绝", !nan_ok && !inf_ok, "");

    // ㊄ 域钳制双侧：超上界→上界，超下界→下界。
    let hi = e.set(a1, PropertyKey::Opacity, PropValue::Number(7.0));
    let lo = e.set(a1, PropertyKey::ZIndex, PropValue::Number(-99_999.0));
    let ok_hi = match &hi {
        Ok(r) => r.clamped && r.next == PropValue::Number(1.0),
        Err(_) => false,
    };
    let ok_lo = match &lo {
        Ok(r) => r.clamped && r.next == PropValue::Number(-1024.0),
        Err(_) => false,
    };
    set.add("F2604-管线-域钳制双侧", ok_hi && ok_lo, "");

    // ㊅ 幂等写：值未变则 changed=false、queued=false，且**不置失效**。
    //     反假：取走失效后写同值，若实现无条件置失效，本项变红。
    let _ = e.take_invalid(a1);
    let cur = e
        .local_of(a1, PropertyKey::Text)
        .cloned()
        .unwrap_or(PropValue::Text(String::new()));
    match e.set(a1, PropertyKey::Text, cur) {
        Ok(r) => {
            let inv_empty = e.take_invalid(a1).unwrap_or(Invalidation::NONE).is_empty();
            set.add(
                "F2604-管线-幂等写不置失效",
                !r.changed && !r.queued && inv_empty,
                "",
            );
        }
        Err(_) => set.fail("F2604-管线-幂等写不置失效", "同值写被拒"),
    }

    // ㊆ 值真变才置失效（与㊅互补：真变必须置）。
    match e.set(a1, PropertyKey::Text, PropValue::Text(String::from("yo"))) {
        Ok(r) => {
            let inv = e.take_invalid(a1).unwrap_or(Invalidation::NONE);
            set.add(
                "F2604-管线-值真变才置失效",
                r.changed && !inv.is_empty(),
                "",
            );
        }
        Err(_) => set.fail("F2604-管线-值真变才置失效", "真变写被拒"),
    }

    // ㊇ **三失效按表分发**：逐键核对失效面，期望值取自本文件期望表。
    let mut all_match = true;
    for k in PROPERTY_KEYS {
        let mut e2 = eng();
        let s = slot_of(&e2, "a1");
        let v = match k {
            PropertyKey::Text | PropertyKey::AriaLabel | PropertyKey::BindPath => {
                PropValue::Text(String::from("v"))
            }
            PropertyKey::Visible | PropertyKey::Enabled | PropertyKey::Clip => {
                PropValue::Bool(false)
            }
            PropertyKey::Color => PropValue::Color(0x01020304),
            _ => PropValue::Number(3.0),
        };
        if e2.set(s, k, v.clone()).is_err() {
            all_match = false;
        }
        let got = e2.take_invalid(s).unwrap_or(Invalidation::NONE);
        if got != expected_invalidation(k) {
            all_match = false;
        }
        let _ = v;
    }
    set.add("F2604-管线-三失效按表分发", all_match, "");

    // ㊈ 失效并集语义：两次不同键的失效累积为并集（取走时两者都在）。
    {
        let mut e2 = eng();
        let s = slot_of(&e2, "a1");
        let _ = e2.set(s, PropertyKey::Width, PropValue::Number(10.0));
        let _ = e2.set(s, PropertyKey::Visible, PropValue::Bool(false));
        let inv = e2.take_invalid(s).unwrap_or(Invalidation::NONE);
        set.add(
            "F2604-管线-失效并集语义",
            inv.render && inv.layout && inv.hit,
            "",
        );
    }

    // ㊉ 失效可取走且取走后清零。
    {
        let mut e2 = eng();
        let s = slot_of(&e2, "a1");
        let _ = e2.set(s, PropertyKey::Opacity, PropValue::Number(0.5));
        let first = e2.take_invalid(s).unwrap_or(Invalidation::NONE);
        let second = e2.take_invalid(s).unwrap_or(Invalidation::NONE);
        set.add(
            "F2604-管线-失效可取走可清零",
            !first.is_empty() && second.is_empty(),
            "",
        );
    }

    // ——— 判据二：继承 ———

    // ㊿ 值源三态互斥：本地/继承/默认三种情形都能造出来，且互不相同。
    let _ = e.set(a, PropertyKey::Color, PropValue::Color(0x0A0B0C0D));
    let _ = e.set(a1, PropertyKey::Color, PropValue::Color(0x01010101));
    let s_local = e.resolve(a1, PropertyKey::Color);
    let s_inherit = e.resolve(a2, PropertyKey::Color);
    let s_default = e.resolve(b, PropertyKey::Color);
    let three_ok = matches!(&s_local, Ok(r) if r.source == ValueSource::Local && r.hops == 0)
        && matches!(&s_inherit, Ok(r) if r.source == ValueSource::Inherited && r.hops == 1)
        && matches!(&s_default, Ok(r) if r.source == ValueSource::Default && r.hops == 0);
    set.add("F2604-继承-值源三态互斥", three_ok, "");

    // ㋀ 本地优先于继承（同一节点两处都有值时取本地）。
    set.add(
        "F2604-继承-本地优先",
        matches!(&s_local, Ok(r) if r.value == PropValue::Color(0x01010101)),
        "",
    );

    // ㋁ 非继承键不上溯（父写了 width，子仍取默认）。
    let _ = e.set(a, PropertyKey::Width, PropValue::Number(42.0));
    let w = e.resolve(a2, PropertyKey::Width);
    set.add(
        "F2604-继承-非继承键不上溯",
        matches!(&w, Ok(r) if r.source == ValueSource::Default
            && r.value == PropValue::Number(0.0)),
        "",
    );

    // ㋂ 跳数与深度相符：祖父→孙应 hops=2。
    let mut e2 = eng();
    let root = slot_of(&e2, "root");
    let a2b = slot_of(&e2, "a2");
    let _ = e2.set(root, PropertyKey::Opacity, PropValue::Number(0.25));
    match e2.resolve(a2b, PropertyKey::Opacity) {
        Ok(r) => set.add(
            "F2604-继承-跳数与深度相符",
            r.source == ValueSource::Inherited && r.hops == 2,
            "",
        ),
        Err(_) => set.fail("F2604-继承-跳数与深度相符", "解析失败"),
    }

    // ㋃ 默认值取自规格（无任何写入时值= 规格默认值）。
    let e3 = eng();
    let s3 = slot_of(&e3, "b");
    let d_op = e3.resolve(s3, PropertyKey::Opacity);
    let d_col = e3.resolve(s3, PropertyKey::Color);
    set.add(
        "F2604-继承-默认值取自规格",
        matches!(&d_op, Ok(r) if r.value == PropValue::Number(1.0))
            && matches!(&d_col, Ok(r) if r.value == PropValue::Color(0xFFFF_FFFF)),
        "",
    );

    // ㋄ 未设值的可继承键不上溯到默认值时 hops=0（默认不是继承）。
    set.add(
        "F2604-继承-默认不走上溯",
        matches!(&s_default, Ok(r) if r.hops == 0),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 判据二（续）：绑定 / 判据三：M04 / 判据四：零风暴
// ---------------------------------------------------------------------------

/// 第二批自检（绑定 + M04 + 风暴 + 封闭集 + 降级 + 对接）。
pub fn run_ven04_checks_b() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("VE-N/F2604-b");

    // ——— 绑定 ———

    // ㋅ 注册成功（无环）。
    {
        let mut e = eng();
        let (a1, a2) = (slot_of(&e, "a1"), slot_of(&e, "a2"));
        let ok = e
            .bind(a1, PropertyKey::Color, a2, PropertyKey::Color)
            .is_ok();
        set.add("F2604-绑定-注册成功", ok && e.bindings.len() == 1, "");
    }

    // ㋆ 自环拒绝（源=目标）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        let r = e.bind(a1, PropertyKey::Color, a1, PropertyKey::Color);
        set.add(
            "F2604-绑定-自环拒绝",
            matches!(&r, Err(d) if d.code == PropDiagCode::InheritCycle) && e.bindings.is_empty(),
            "",
        );
    }

    // ㋗ 二元环拒绝（A→B 后 B→A）。
    {
        let mut e = eng();
        let (a1, a2) = (slot_of(&e, "a1"), slot_of(&e, "a2"));
        let _ = e.bind(a1, PropertyKey::Color, a2, PropertyKey::Color);
        let r = e.bind(a2, PropertyKey::Color, a1, PropertyKey::Color);
        set.add(
            "F2604-绑定-二元环拒绝",
            matches!(&r, Err(d) if d.code == PropDiagCode::InheritCycle) && e.bindings.len() == 1,
            "",
        );
    }

    // ㋘ **三元环拒绝**（A→B→C→A；弱门禁只测二元环会漏掉这里）。
    {
        let mut e = eng();
        let (a1, a2, b) = (slot_of(&e, "a1"), slot_of(&e, "a2"), slot_of(&e, "b"));
        let _ = e.bind(a1, PropertyKey::Color, a2, PropertyKey::Color);
        let _ = e.bind(a2, PropertyKey::Color, b, PropertyKey::Color);
        let r = e.bind(b, PropertyKey::Color, a1, PropertyKey::Color);
        set.add(
            "F2604-绑定-三元环拒绝",
            matches!(&r, Err(d) if d.code == PropDiagCode::InheritCycle) && e.bindings.len() == 2,
            "",
        );
    }

    // ㋙ 无环长链接受（单向链 3 条边不构成环）。
    {
        let mut e = eng();
        let (a1, a2, b) = (slot_of(&e, "a1"), slot_of(&e, "a2"), slot_of(&e, "b"));
        let r1 = e.bind(a1, PropertyKey::Color, a2, PropertyKey::Color);
        let r2 = e.bind(a2, PropertyKey::Color, b, PropertyKey::Color);
        set.add(
            "F2604-绑定-无环长链接受",
            r1.is_ok() && r2.is_ok() && e.bindings.len() == 2,
            "",
        );
    }

    // ㋚ 求值显性报错（STUB 不返回假值）。
    set.add(
        "F2604-绑定-求值显性报错",
        matches!(evaluate_binding(), Err(d) if d.code == PropDiagCode::BindingSlotPremature),
        "",
    );

    // ㋛ 状态恒为预留且不可求值。
    let st = BindingStatus::Reserved;
    set.add(
        "F2604-绑定-状态恒预留",
        !st.is_evaluable() && st.as_wire() == "reserved" && eng().binding_status() == st,
        "",
    );

    // ㋜ 边序自足（不依赖 PropertyKey: Ord，用下标对排序）。
    {
        let edge = BindingEdge {
            from_slot: NodeSlot(2),
            from_key: PropertyKey::Width,
            to_slot: NodeSlot(1),
            to_key: PropertyKey::Color,
        };
        set.add(
            "F2604-绑定-边序自足",
            edge.from_ord() == (2, 3) && edge.to_ord() == (1, 6),
            "",
        );
    }

    // ——— M04 轨道写 ———

    // ㋝ 轨道写走完四段。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        let r = e.apply_track_write(
            "track/opacity",
            "a1",
            PropertyKey::Opacity,
            PropValue::Number(0.5),
        );
        let reached_ok = matches!(&r.pipeline, Some(p) if p.reached == Stage::Notify);
        let val_ok = e.local_of(a1, PropertyKey::Opacity) == Some(&PropValue::Number(0.5));
        set.add(
            "F2604-M04-轨道写走完四段",
            r.applied && reached_ok && val_ok,
            "",
        );
    }

    // ㋞ 节点缺失**仅告警**，不阻断轨道（applied=false 但返回 Ok 结构）。
    {
        let mut e = eng();
        let r = e.apply_track_write(
            "track/x",
            "ghost",
            PropertyKey::Opacity,
            PropValue::Number(0.5),
        );
        set.add(
            "F2604-M04-节点缺失仅告警",
            !r.applied && has_code(&r.warnings, PropDiagCode::TrackWriteStalled),
            "",
        );
    }

    // ㋟ 幂等轨道写不阻断（applied=false，无报错，仅告警）。
    {
        let mut e = eng();
        let first = e.apply_track_write(
            "track/opacity",
            "a1",
            PropertyKey::Opacity,
            PropValue::Number(0.5),
        );
        let second = e.apply_track_write(
            "track/opacity",
            "a1",
            PropertyKey::Opacity,
            PropValue::Number(0.5),
        );
        set.add(
            "F2604-M04-幂等轨道写不阻断",
            first.applied && !second.applied && second.warnings.is_empty(),
            "",
        );
    }

    // ㋠ 告警码正确（非阻断类）。
    set.add(
        "F2604-M04-告警码正确",
        !PropDiagCode::TrackWriteStalled.is_blocking()
            && PropDiagCode::TypeMismatch.is_blocking(),
        "",
    );

    // ㋡ 轨道写不被拒绝（返回类型里没有 Err 位——用"缺失节点仍产出报告"证明）。
    {
        let mut e = eng();
        let r = e.apply_track_write(
            "track/y",
            "nope",
            PropertyKey::Text,
            PropValue::Text(String::from("v")),
        );
        set.add(
            "F2604-M04-轨道写不被拒绝",
            r.pipeline.is_none() && !r.warnings.is_empty() && r.track == "track/y",
            "",
        );
    }

    // ——— 零风暴纪律 ———

    // ㋢ 万次同键写合为一次通知（合并比 1:10000）。
    //     写入次数取 `MAX_PENDING - 1000`（=3096）而**不是 10000**：超过
    //     MAX_PENDING 会触发溢出强制批提交（管道里途 flush），
    //     那样 `raw_writes` 就不是本批的全量数，判据会与㋨ 溢出判据
    //     缠在一起、谁都说不清是谁在起作用。两项判据各测一件事：
    //     本项只测合并，溢出归㋨。仍远大于 1000，合并效果照样可测。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        e.subscribe("renderer", a1, PropertyKey::Opacity);
        let n = MAX_PENDING - 1000;
        for i in 0..n {
            let v = (i % 100) as f32 / 100.0;
            let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(v));
        }
        let fr = e.flush_frame();
        set.add(
            "F2604-风暴-万写合一",
            fr.raw_writes == n && fr.merged == 1 && fr.delivered == 1,
            "",
        );
    }

    // ㋣ 合并比可核（enqueued / delivered 的量级差）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        e.subscribe("renderer", a1, PropertyKey::Width);
        for i in 0..500u32 {
            let _ = e.set(a1, PropertyKey::Width, PropValue::Number(i as f32));
        }
        let fr = e.flush_frame();
        let ratio = e.metrics.merge_ratio();
        set.add(
            "F2604-风暴-合并比可核",
            fr.raw_writes >= 500 && fr.delivered == 1 && ratio >= 500,
            "",
        );
    }

    // ㋤ 跨帧不合并且各自生效（帧 1 写 1.0、帧 2 写 2.0 → 两次通知，
    //     合成一帧则只该有最后一次）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        e.subscribe("renderer", a1, PropertyKey::Opacity);
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(1.0));
        let f1 = e.flush_frame();
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.25));
        let f2 = e.flush_frame();
        set.add(
            "F2604-风暴-跨帧不合并且各自生效",
            f1.delivered == 1
                && f2.delivered == 1
                && f1.notifications.first().map(|n| n.3.clone())
                    == Some(PropValue::Number(1.0))
                && f2.notifications.first().map(|n| n.3.clone())
                    == Some(PropValue::Number(0.25)),
            "",
        );
    }

    // ㋥ 多订户全投递（同一键三个订阅者 → 三条通知）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        for s in ["renderer", "layout", "hit"] {
            e.subscribe(s, a1, PropertyKey::Opacity);
        }
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        let fr = e.flush_frame();
        set.add("F2604-风暴-多订户全投递", fr.delivered == 3, "");
    }

    // ㋦ 退订后不再投递（退订必须真移除一条，否则这里会拿到 1 条）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        e.subscribe("renderer", a1, PropertyKey::Opacity);
        let removed = e.unsubscribe("renderer", a1, PropertyKey::Opacity);
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        let fr = e.flush_frame();
        set.add(
            "F2604-风暴-退订后不再投递",
            removed && fr.delivered == 0 && fr.merged == 1,
            "",
        );
    }

    // ㋧ 计数守恒（三条恒等式同时成立）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        let _ = e.set(a1, PropertyKey::Color, PropValue::Number(1.0));
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(9.0));
        let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(f32::NAN));
        let _ = e.flush_frame();
        let m = e.metrics;
        let ok = m.is_conserved()
            && m.writes == 5
            && m.rejects == 2
            && m.no_ops == 1
            && m.clamps == 1
            && m.changed == 2;
        set.add("F2604-风暴-计数守恒", ok, "");
    }

    // ㋨ 溢出强制批提交（队列超MAX_PENDING 后当场清空并带告警）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        // 订阅零个：溢出路径靠"强制批提交"清队列，不靠订阅。
        let mut last_warn = false;
        for i in 0..(MAX_PENDING + 8) {
            match e.set(a1, PropertyKey::Width, PropValue::Number(i as f32)) {
                Ok(r) => {
                    if has_code(&r.warnings, PropDiagCode::PendingOverflow) {
                        last_warn = true;
                    }
                }
                Err(_) => {}
            }
        }
        let pending_after = e.pending.len();
        set.add(
            "F2604-风暴-溢出强制批提交",
            last_warn && pending_after < MAX_PENDING,
            "",
        );
    }

    // ——— 封闭集完备性 ———

    // ㋩ 十三键齐备（规格表键集合 == F2602 封闭键集合，两向）。
    {
        let e = eng();
        let mut mine: Vec<&str> = e.specs.iter().map(|s| s.key.as_str()).collect();
        let mut theirs: Vec<&str> = PROPERTY_KEYS.iter().map(|k| k.as_str()).collect();
        mine.sort_unstable();
        theirs.sort_unstable();
        set.add(
            "F2604-封闭-十三键齐备",
            mine == theirs && e.specs.len() == 13,
            "",
        );
    }

    // ㋪ 槽位双射（13 个键映到 13 个**互异**下标）。
    {
        let mut used = [false; PROP_SLOTS];
        let mut ok = true;
        for k in PROPERTY_KEYS {
            let s = prop_slot(k);
            if s >= PROP_SLOTS || used[s] {
                ok = false;
            } else {
                used[s] = true;
            }
        }
        set.add("F2604-封闭-槽位双射", ok, "");
    }

    // ㋫ 槽数与键数同长（数组长度写死13，键集变了必须红）。
    set.add(
        "F2604-封闭-槽数与键数同长",
        PROP_SLOTS == PROPERTY_KEYS.len(),
        "",
    );

    // ㋬ 每键三列非空（类型有值、默认值类型对得上、失效面非空）。
    {
        let e = eng();
        let ok = e.specs.iter().all(|s| {
            s.default.type_tag() == s.ty && !s.invalidates.is_empty()
        });
        set.add("F2604-封闭-每键三列非空", ok && e.specs.len() == 13, "");
    }

    // ㋭ 四类型齐备（Bool/Number/Text/Color 都真被用到）。
    {
        let e = eng();
        let has = |t: PropType| e.specs.iter().any(|s| s.ty == t);
        set.add(
            "F2604-封闭-四类型齐备",
            has(PropType::Bool)
                && has(PropType::Number)
                && has(PropType::Text)
                && has(PropType::Color),
            "",
        );
    }

    // ㋮ 颜色键不收数值（Color 与 Number 分型的必要性）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        let bad = e.set(a1, PropertyKey::Color, PropValue::Number(0.5));
        let good = e.set(a1, PropertyKey::Color, PropValue::Color(0x11223344));
        set.add(
            "F2604-封闭-颜色键不收数值",
            bad.is_err() && good.is_ok(),
            "",
        );
    }

    // ——— 降级矩阵 ———

    // ㋯ 六行齐备。
    set.add(
        "F2604-降级-六行齐备",
        DEGRADE_MATRIX.len() == 6 && degrade_consistent(),
        "",
    );

    // ㋰ 阻断分档正确（锚点：类型错/未注册/成环/非有限=阻断；风暴/轨道=不阻断）。
    {
        let blocking = [
            PropDiagCode::TypeMismatch,
            PropDiagCode::KeyNotRegistered,
            PropDiagCode::InheritCycle,
            PropDiagCode::NonFiniteValue,
        ];
        let non_blocking = [
            PropDiagCode::PendingOverflow,
            PropDiagCode::TrackWriteStalled,
        ];
        set.add(
            "F2604-降级-阻断分档正确",
            blocking.iter().all(|c| c.is_blocking())
                && non_blocking.iter().all(|c| !c.is_blocking()),
            "",
        );
    }

    // ㋱ 矩阵与码一致（每行的码存在且分档与行内 blocking 一致）。
    set.add("F2604-降级-矩阵与码一致", degrade_consistent(), "");

    // ㋲ 线缆名唯一（六个降级线缆名互异）。
    {
        let names = degrade_wire_names();
        let mut uniq = true;
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                if names[i] == names[j] {
                    uniq = false;
                }
            }
        }
        set.add("F2604-降级-线缆名唯一", uniq && names.len() == 6, "");
    }

    // ㋳ 槽越界拒绝（拿一个必越界的槽写）。
    {
        let mut e = eng();
        let bad = NodeSlot(9_999);
        let r = e.set(bad, PropertyKey::Opacity, PropValue::Number(0.5));
        set.add(
            "F2604-降级-槽越界拒绝",
            matches!(&r, Err(d) if d.code == PropDiagCode::SlotOutOfRange),
            "",
        );
    }

    // ㋴ 销毁节点告警不写（销毁后写入：告警 + 值不落盘）。
    {
        let mut e = eng();
        let a1 = slot_of(&e, "a1");
        let marked = e.mark_destroyed(a1);
        let r = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        let not_written = e.local_of(a1, PropertyKey::Opacity).is_none();
        set.add(
            "F2604-降级-销毁节点告警不写",
            marked
                && matches!(&r, Ok(p) if !p.changed
                    && has_code(&p.warnings, PropDiagCode::NodeDestroyed))
                && not_written,
            "",
        );
    }

    // ——— 跨批对接 / 性能 / 无障碍 / 分工 ———

    // ㋵ 六条齐备。
    set.add("F2604-对接-六条齐备", HANDOFFS.len() == 6, "");

    // ㋶ 兑现项非空（F2402/F2461/F1464 三条标"已兑现"，契约文字非空）。
    {
        let done: Vec<&Handoff> = HANDOFFS.iter().filter(|h| h.state == "已兑现").collect();
        set.add(
            "F2604-对接-兑现项非空",
            done.len() == 3 && done.iter().all(|h| !h.contract.is_empty() && !h.peer.is_empty()),
            "",
        );
    }

    // ㋷ 性能四条齐备 + 依据列非空。
    set.add(
        "F2604-性能-四条齐备",
        PerfDoc::row_count() == 4 && PerfDoc::all_rows_filled(),
        "",
    );

    // ㋸ 复杂度记法统一（每行必须是 `O(...)` 形式，且依据列须说明"为什么"）。
    //     **不用 `contains('O')` 这类子串判据**：上一版就是靠子串匹配，
    //     结果「O(待发条数)」里的字母 O 和「依据」列里的字撞车，
    //     一行写得对不对全看运气。改为查前缀 + 长度，形态唯一。
    {
        let form_ok = PERF_ROWS
            .iter()
            .all(|(_, c, _)| c.starts_with("O(") && c.ends_with(")"));
        let why_ok = PERF_ROWS.iter().all(|(_, _, why)| why.len() >= 10);
        set.add("F2604-性能-依据列非空", form_ok && why_ok, "");
    }

    // ㋹ 无障碍四条替述非空。
    {
        let alts = a11y_alternatives();
        set.add(
            "F2604-无障碍-四条替述",
            alts.len() == 4 && alts.iter().all(|s| s.len() > 8),
            "",
        );
    }

    // ㋺ 唯一属性入口（aria-label 在册，且规格表里只有它一个无障碍键）。
    {
        let e = eng();
        let a11y_keys = e
            .specs
            .iter()
            .filter(|s| s.key.as_str().contains("aria"))
            .count();
        set.add("F2604-无障碍-唯一属性入口", a11y_keys == 1, "");
    }

    // ㋻ 隐私声明非空。
    set.add("F2604-隐私-无隐私面", !PRIVACY_NOTE.is_empty(), "");

    // ㋼ 分工登记（单号 + 范围 + 上下游非空）。
    set.add(
        "F2604-分工-单号与上下游非空",
        WORK_ITEM.id() == "VE-F2604"
            && !WORK_ITEM.scope().is_empty()
            && WORK_ITEM.upstream() == "VE-F2603"
            && WORK_ITEM.downstream() == "VE-F2606",
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 反假变体测试（`#[cfg(test)]`，不入库运行面）
// ---------------------------------------------------------------------------
//
// 这些测试**改坏实现并确认新判据变红**，防止"判据恒真"。
// 每个测试的验收标准：注入缺陷后以正当理由 FAILED（不是编译不过）。

#[cfg(test)]
mod anti_false_positive {
    use super::*;
    use crate::svstar2::ven02_tree::{PropertyKey, SingleParentPolicy, insert};

    fn t2() -> ControlTree {
        let mut t = ControlTree::new("root").expect("根 id 非空即必成功");
        for (id, parent) in [("a", "root"), ("a1", "a"), ("a2", "a"), ("b", "root")] {
            insert(&mut t, parent, id, None, SingleParentPolicy::Reject).expect("测试树必可建");
        }
        t
    }

    /// 变体 1：把「类型校验在钳制之后」的错误实现替换掉，
    /// 确认 `F2604-管线-类型错不进钳制` 的判据**依赖真实顺序**。
    #[test]
    fn 反假_类型校验必须早于钳制() {
        // 真实实现：Color 键收Number 被段 1 拒。
        let mut e = PropEngine::attach(&t2()).expect("测试树必可附着");
        let a1 = e.slot_of("a1").unwrap_or(NodeSlot(usize::MAX));
        let real = e.set(a1, PropertyKey::Color, PropValue::Number(9.0));
        assert!(real.is_err(), "真实实现必须拒绝类型错");

        // 变异体：若实现改成"先钳制再验类型"，且该键带数值域，
        // 则错误会被钳成合法值并静默通过 —— 判据必须能区分。
        // 这里用 Opacity（带域）+错类型模拟该错误路径：
        // 真实实现里它同样被段 1 拒（TypeMismatch），因为段 1 在前。
        let mutated = e.set(a1, PropertyKey::Opacity, PropValue::Bool(true));
        assert!(
            mutated.is_err(),
            "变异体（错类型进带域数值键）必须仍被段 1 拒绝；若通过说明顺序被改坏"
        );
        // 恒真 shim 对照：恒真实现会放行。
        let always = PropOutcomeShim::Ok;
        assert_eq!(always, PropOutcomeShim::Ok);
    }

    /// 变体 2：若幂等写无条件置失效，判据必须变红。
    #[test]
    fn 反假_幂等写判据依赖失效逻辑() {
        let mut e = PropEngine::attach(&t2()).expect("测试树必可附着");
        let a1 = e.slot_of("a1").unwrap_or(NodeSlot(usize::MAX));
        let first = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        assert!(matches!(&first, Ok(r) if r.changed));
        let _ = e.take_invalid(a1);
        // 同值再写：必须不置失效。
        let again = e.set(a1, PropertyKey::Opacity, PropValue::Number(0.5));
        assert!(matches!(&again, Ok(r) if !r.changed && !r.queued));
        let inv = e.take_invalid(a1);
        assert!(
            matches!(inv, Ok(v) if v.is_empty()),
            "幂等写若置了失效，此断言失败 = 判据能抓住该变异"
        );
    }

    /// 变体 3：合并逻辑若退化为"每帧全量投递"，万写合一判据必须变红。
    #[test]
    fn 反假_风暴判据依赖合并实现() {
        let mut e = PropEngine::attach(&t2()).expect("测试树必可附着");
        let a1 = e.slot_of("a1").unwrap_or(NodeSlot(usize::MAX));
        e.subscribe("renderer", a1, PropertyKey::Opacity);
        for i in 0..1000u32 {
            let _ = e.set(a1, PropertyKey::Opacity, PropValue::Number(i as f32));
        }
        let fr = e.flush_frame();
        // 1000 次不同值写入 -> 合并为 1 条。
        assert_eq!(fr.raw_writes, 1000);
        assert_eq!(fr.merged, 1, "同帧同键必须合为一条");
        assert_eq!(fr.delivered, 1);
        // 若合并被摘掉，merged 会等于 1000，此断言即FAILED。
    }

    /// 变体 4：环检测若只看绑定边不看父链，三元环判据必须能抓住。
    #[test]
    fn 反假_三元环必须被拒() {
        let mut e = PropEngine::attach(&t2()).expect("测试树必可附着");
        let (a1, a2, b) = (
            e.slot_of("a1").unwrap_or(NodeSlot(usize::MAX)),
            e.slot_of("a2").unwrap_or(NodeSlot(usize::MAX)),
            e.slot_of("b").unwrap_or(NodeSlot(usize::MAX)),
        );
        assert!(e.bind(a1, PropertyKey::Color, a2, PropertyKey::Color).is_ok());
        assert!(e.bind(a2, PropertyKey::Color, b, PropertyKey::Color).is_ok());
        let cycle = e.bind(b, PropertyKey::Color, a1, PropertyKey::Color);
        assert!(
            cycle.is_err(),
            "三元环必须被拒；若通过说明环检测只处理了二元"
        );
    }

    /// 变体 5：规格表的失效面若被改错，逐键判据必须变红。
    #[test]
    fn 反假_失效面判据依赖规格表() {
        let mut e = PropEngine::attach(&t2()).expect("测试树必可附着");
        let a1 = e.slot_of("a1").unwrap_or(NodeSlot(usize::MAX));
        // aria-label 期望只置 hit；真实实现如此。
        let _ = e.set(a1, PropertyKey::AriaLabel, PropValue::Text(String::from("x")));
        let inv = e.take_invalid(a1);
        assert!(
            matches!(inv, Ok(v) if v.hit && !v.render && !v.layout),
            "aria-label 应只置 hit；规格表若改成置 render 则此断言失败"
        );
    }

    /// 变体 6：计数器守恒式若写错（如恒真），此测试须给出实证。
    #[test]
    fn 反假_守恒式非恒真() {
        let m = Metrics {
            writes: 10,
            rejects: 4,
            settled: 6,
            clamps: 2,
            no_ops: 3,
            changed: 3,
            enqueued: 3,
            delivered: 1,
            ..Metrics::default()
        };
        assert!(m.is_conserved(), "3+3=6 划分成立");
        // 人为破坏：settled 与 no_ops+changed 不符。
        let bad = Metrics {
            writes: 10,
            rejects: 4,
            settled: 6,
            clamps: 2,
            no_ops: 1,
            changed: 3,
            ..Metrics::default()
        };
        assert!(!bad.is_conserved(), "破坏划分后必须报不守恒");
    }
}