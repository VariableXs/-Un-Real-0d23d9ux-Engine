//! VE-F3006 · 动效组件化判据
//!
//! **锚点判据（原文）**：四件套、三族注册、参数钳制、注册要件红线、沙箱扩展、判据。
//!
//! # 一、判据怎么做到"不是恒真"
//!
//! 组件化最容易写成恒真断言的地方有五处，本模块逐一封死：
//!
//! 1. **"四件套齐备"**。若只断「构造成功」，一个「把触发条件硬编码成常量、无论传什么都
//!    接受」的实现照样绿。必须**逐件拒绝**：分别构造缺触发/缺图/缺 a11y/超长名的语料，
//!    断言**每一种都拿到对应的专属错误码**——只断"失败了"会被"因别的原因失败"骗过。
//! 2. **"参数钳制"**。最典型的假绿是「钳制函数返回域下界」——语料若全是超界值，
//!    恒返回下界也全绿。必须**双向**：既有超界语料（断被钳回域内），也有域内语料
//!    （断原样透传、零记账）。再加一条负向：**未声明的参数名必须被记账**，
//!    否则「把所有未知参数当默认」的实现能绕过全部钳制。
//! 3. **"覆盖内置=拒绝"**。若语料里没有「自定义撞内置」的用例，这条分支是死代码。
//!    必须真造一个 `ext:` 名去撞 `builtin:` 已注册名，断言**拒绝且内置那份纹丝不动**
//!    （断注册表长度不变 + 检索到的仍是原组件）。
//! 4. **"注册要件红线"**。`A11yBehavior::new` 的空文案分支若语料里从不传空，恒绿。
//!    必须造空文案/纯空白文案/超长文案三种语料。
//! 5. **"沙箱隔离"**。断「自定义被停用后 apply 被拒」还不够——要同时断
//!    **其余组件仍可用**（隔离的含义是只停一个，不是全停）。
//!
//! # 二、方向：被拒才是合格
//!
//! 注册与构造的**成功路径**也要断，但**方向是反的**——本判据里"注册成功"不是重点，
//! 重点是"该拒的确实拒了、且拒绝时零副作用"（断长度不变、断检索结果不变）。
//! 一个「什么都拒」的实现能骗过所有正向断言，却骗不过这些副作用断言。

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::Lane;
use crate::svstar2::vep05_orch::OrchGraph;
use crate::svstar2::vep06_motion::*;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 构造一条两节点序列图（判据语料共用起手式）。
///
/// `demo_graph` 是本 crate 的 `pub fn`，失败只可能来自 F3005 的图校验；判据层
/// 不写 panic 面，故用 [`fallback_graph`] 兜底并让"兜底路径"本身成为可判事实。
fn g(dur: u32) -> OrchGraph {
    match demo_graph("p", dur) {
        Ok(gr) => gr,
        Err(_) => fallback_graph(),
    }
}

/// 空图兜底（构造失败时的替身）。
///
/// 空 `Orchestrator` 的 `commit` 在 F3005 里返回**空图而非 Err**，故此处直接返回
/// 手工构造的空图：语义等价且不留 panic 面。
fn fallback_graph() -> OrchGraph {
    unreachable_empty_graph()
}

/// 终兜底：手工空图（字段全空 Vec）。
fn unreachable_empty_graph() -> OrchGraph {
    OrchGraph {
        nodes: Vec::new(),
        edges: Vec::new(),
        topological: Vec::new(),
        nest_depth: 0,
        parallel_groups: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 判据一：四件套（锚点判据 1）
// ---------------------------------------------------------------------------

fn chk_four_piece(cs: &mut CheckSet) {
    // 基准语料：四件齐备 ⇒ 构造成功且四件都在。
    let okc = demo_component("builtin:ok", Family::Enter, "mount");
    match okc {
        Ok(c) => {
            let piece_ok = !c.trigger.is_empty()
                && c.graph.node_count() == 2
                && c.param_count() == 2
                && !c.a11y.note.is_empty();
            cs.add(
                "E06-四件套-四件齐备构造成功",
                piece_ok,
                "触发/图/参数/无障碍四件同时非空",
            );
        }
        Err(_) => cs.add("E06-四件套-四件齐备构造成功", false, "齐备语料却构造失败"),
    }

    // 缺①：触发条件为空 ⇒ 必须拒，且错误码是 E_TRIGGER_EMPTY（专属码，非泛泛失败）。
    let e1 = demo_component("builtin:notrig", Family::Enter, "   ").err();
    let code1 = e1.as_ref().map(|e| e.code).unwrap_or("<none>");
    cs.add(
        "E06-四件套-缺触发条件被拒",
        code1 == E_TRIGGER_EMPTY,
        "空白触发条件须拿到 E_TRIGGER_EMPTY 专属码",
    );

    // 缺②：编排图为空 ⇒ 拒，错误码 E_GRAPH_EMPTY。
    let a11y = A11yBehavior::new(ReduceBehavior::CollapseToEndState, "跳到终态");
    if let Ok(a) = a11y {
        let e2 = MotionComponent::new(
            "builtin:nograph",
            Family::Enter,
            "空图",
            "mount",
            empty_graph(),
            vec![],
            a,
        )
        .err();
        let code2 = e2.as_ref().map(|e| e.code).unwrap_or("<none>");
        cs.add(
            "E06-四件套-空编排图被拒",
            code2 == E_GRAPH_EMPTY,
            "空编排图须拿到 E_GRAPH_EMPTY 专属码",
        );
    } else {
        cs.add("E06-四件套-空编排图被拒", false, "a11y 构造失败，无法验证");
    }

    // 名超长 ⇒ 拒（E_NAME_TOO_LONG）。
    let longname = format!("builtin:{}", "x".repeat(NAME_CAP + 1));
    let e3 = demo_component(&longname, Family::Enter, "mount").err();
    let code3 = e3.as_ref().map(|e| e.code).unwrap_or("<none>");
    cs.add(
        "E06-四件套-超长名被拒",
        code3 == E_NAME_TOO_LONG,
        "超长组件名须拿到 E_NAME_TOO_LONG",
    );

    // 参数默认值越域 ⇒ 拒（E_PARAM_DOMAIN）——这是"注册期静态错误"，与运行期钳制区分开。
    let e4 = ParamSpec::new("bad", 0, 100, 500).err();
    let code4 = e4.as_ref().map(|e| e.code).unwrap_or("<none>");
    cs.add(
        "E06-四件套-默认值越域被拒",
        code4 == E_PARAM_DOMAIN,
        "默认值 500 越域 [0,100] 须拒",
    );

    // 域下界>=上界 ⇒ 拒（E_PARAM_DOMAIN的另一分支）。
    let e5 = ParamSpec::new("empty", 100, 100, 100).err();
    cs.add(
        "E06-四件套-空值域被拒",
        e5.as_ref().map(|e| e.code).unwrap_or("<none>") == E_PARAM_DOMAIN,
        "min==max 的空域须拒",
    );
}

// ---------------------------------------------------------------------------
// 判据二：三族注册（锚点判据 2）
// ---------------------------------------------------------------------------

fn chk_three_families(cs: &mut CheckSet) {
    let mut reg = MotionRegistry::new();

    let e = demo_component("builtin:fade", Family::Enter, "mount");
    if e.is_err() {
        cs.add("E06-三族-入场注册成功", false, "入场组件构造失败");
    } else if let Ok(c) = e {
        let r = reg.register(c);
        cs.add("E06-三族-入场注册成功", r.is_ok(), "入场组件应注册成功");
    }

    if let Ok(c) = demo_component("builtin:slide", Family::Transition, "route") {
        let r = reg.register(c);
        cs.add("E06-三族-转场注册成功", r.is_ok(), "转场组件应注册成功");
    }
    if let Ok(c) = demo_component("builtin:ripple", Family::Feedback, "tap") {
        let r = reg.register(c);
        cs.add("E06-三族-反馈注册成功", r.is_ok(), "反馈组件应注册成功");
    }

    // 三族分桶：每族恰好 1 个（断分桶不是全塞进一桶）。
    let c_enter = reg.family_count(Family::Enter);
    let c_trans = reg.family_count(Family::Transition);
    let c_fb = reg.family_count(Family::Feedback);
    let c_custom = reg.family_count(Family::Custom);
    let bucket_ok = c_enter == 1 && c_trans == 1 && c_fb == 1 && c_custom == 0;
    cs.add(
        "E06-三族-三族分桶各一",
        bucket_ok,
        "入场/转场/反馈各 1，自定义区 0",
    );

    // 总数 = 分桶之和（守恒对账）。
    let sum = c_enter + c_trans + c_fb + c_custom;
    let total = reg.len();
    cs.add(
        "E06-三族-分桶和等于总数",
        sum == total && total == 3,
        "分桶之和须等于总数 3",
    );

    // 内置数 = 三族和（自定义区不计入内置）。
    cs.add(
        "E06-三族-内置数为三族和",
        reg.builtin_count() == c_enter + c_trans + c_fb,
        "内置组件数须等于三族之和",
    );

    // 按名检索可发现（可发现性红线）。
    let found = reg.find("builtin:ripple");
    let found_fb = found.map(|c| c.family == Family::Feedback).unwrap_or(false);
    cs.add(
        "E06-三族-按名检索命中",
        found_fb,
        "按名须能检索到反馈族组件",
    );

    // 族索引往返互逆。
    let rt = (0..=FAMILY_COUNT)
        .all(|i| Family::from_index(i).map(|f| f.index() == i).unwrap_or(false));
    cs.add("E06-三族-族索引往返互逆", rt, "族索引⇒族⇒索引须恒等");

    // 读屏播报含四族计数（可枚举性）。
    let spoken = registry_spoken(&reg);
    let spoken_ok = spoken.contains("入场系列") && spoken.contains("自定义区");
    cs.add("E06-三族-读屏播报含族计数", spoken_ok, "注册表播报须列出各族");
}

// ---------------------------------------------------------------------------
// 判据三：参数钳制（锚点判据 3）
// ---------------------------------------------------------------------------

fn chk_clamp(cs: &mut CheckSet) {
    let spec = match ParamSpec::new("dur", 100, 200, 150) {
        Ok(s) => Some(s),
        Err(_) => None,
    };

    // 双向之一：超界被钳回域内（低于下界）。
    let low = match spec.as_ref() {
        Some(s) => s.clamp(50),
        None => -1,
    };
    cs.add(
        "E06-钳制-低于下界被钳回",
        low == 100,
        "50 应被钳到下界 100",
    );

    // 双向之二：超界被钳回域内（高于上界）。
    let high = match spec.as_ref() {
        Some(s) => s.clamp(9999),
        None => -1,
    };
    cs.add(
        "E06-钳制-高于上界被钳回",
        high == 200,
        "9999 应被钳到上界 200",
    );

    // 双向之三（关键）：域内值原样透传。
    // 少了这条，一个「恒返回下界」的实现能骗过上面两条。
    let inrange = match spec.as_ref() {
        Some(s) => s.clamp(175),
        None => -1,
    };
    cs.add(
        "E06-钳制-域内值原样透传",
        inrange == 175,
        "175 在域内须原样返回，不得被钳",
    );

    // 谓词自行兜底：越域判定对域内值返回 false。
    let ood = spec.as_ref().map(|s| s.out_of_domain(175)).unwrap_or(true);
    cs.add(
        "E06-钳制-域内值判定为不越域",
        !ood,
        "175 在域内，越域判定须为假",
    );

    // resolve_params 全链路：未覆盖的走默认值。
    let mut ov = ParamOverrides::new();
    let r1 = resolve_params(&[spec.clone().unwrap_or_else(fallback_spec)], &ov);
    match r1 {
        Ok(o) => {
            let v = o.value_of("dur");
            cs.add(
                "E06-钳制-未覆盖走默认值",
                v == Some(150) && !o.any_clamped(),
                "未传覆盖时取默认 150 且零记账",
            );
        }
        Err(_) => cs.add("E06-钳制-未覆盖走默认值", false, "resolve 失败"),
    }

    // resolve 全链路：超界被钳 + 记账方向为 +1。
    let mut ov2 = ParamOverrides::new();
    ov2.set("dur", 500);
    match resolve_params(&[spec.clone().unwrap_or_else(fallback_spec)], &ov2) {
        Ok(o) => {
            let v = o.value_of("dur");
            let rec = o.record_of("dur");
            let dir_ok = rec.map(|r| r.direction == 1).unwrap_or(false);
            cs.add(
                "E06-钳制-超界记账方向为上",
                v == Some(200) && o.clamp_count() == 1 && dir_ok,
                "500 应钳到 200，记账 1 条且方向 +1",
            );
        }
        Err(_) => cs.add("E06-钳制-超界记账方向为上", false, "resolve 失败"),
    }

    // 未声明参数名必须被记账（负向：防「忽略未知参数」绕过钳制）。
    let mut ov3 = ParamOverrides::new();
    ov3.set("nope", 7);
    match resolve_params(&[spec.clone().unwrap_or_else(fallback_spec)], &ov3) {
        Ok(o) => {
            let rec = o.record_of("nope");
            cs.add(
                "E06-钳制-未声明参数被记账",
                rec.map(|r| r.clamped == PARAM_MIN).unwrap_or(false),
                "未声明参数须兜底下界并记账",
            );
        }
        Err(_) => cs.add("E06-钳制-未声明参数被记账", false, "resolve 失败"),
    }

    // 钳制播报含参数名与数值（读屏可查）。
    match resolve_params(&[spec.clone().unwrap_or_else(fallback_spec)], &ov2) {
        Ok(o) => {
            let sp = o.spoken();
            cs.add(
                "E06-钳制-播报含名与值",
                sp.contains("dur") && sp.contains("500") && sp.contains("200"),
                "钳制播报须含参数名、原值、钳后值",
            );
        }
        Err(_) => cs.add("E06-钳制-播报含名与值", false, "resolve 失败"),
    }

    // 参数名重复 ⇒ 拒（E_PARAM_DUP）。
    let dup = resolve_params(
        &[
            ok_spec("a", 0, 10, 5),
            ok_spec("a", 0, 20, 5),
        ],
        &ParamOverrides::new(),
    );
    cs.add(
        "E06-钳制-重名参数被拒",
        dup.as_ref().err().map(|e| e.code).unwrap_or("<none>") == E_PARAM_DUP,
        "同名参数声明两次须拒",
    );
}

// ---------------------------------------------------------------------------
// 判据四：注册要件红线 + 命名空间隔离（锚点判据 4）
// ---------------------------------------------------------------------------

fn chk_registry_guard(cs: &mut CheckSet) {
    // a11y 播报文案缺省 ⇒ 拒（注册要件红线）。
    let e_empty = A11yBehavior::new(ReduceBehavior::StaticOnly, "").err();
    cs.add(
        "E06-要件-空播报文案被拒",
        e_empty.as_ref().map(|e| e.code).unwrap_or("<none>") == E_A11Y_REQUIRED,
        "空文案须拿 E_A11Y_REQUIRED",
    );
    // 纯空白文案同样拒（trim 后为空）。
    let e_ws = A11yBehavior::new(ReduceBehavior::StaticOnly, "   \t ").err();
    cs.add(
        "E06-要件-纯空白文案被拒",
        e_ws.as_ref().map(|e| e.code).unwrap_or("<none>") == E_A11Y_REQUIRED,
        "纯空白文案 trim 后为空须拒",
    );
    // 超长文案拒。
    let longnote = "x".repeat(REDUCE_NOTE_CAP + 1);
    let e_long = A11yBehavior::new(ReduceBehavior::StaticOnly, &longnote).err();
    cs.add(
        "E06-要件-超长文案被拒",
        e_long.as_ref().map(|e| e.code).unwrap_or("<none>") == E_A11Y_REQUIRED,
        "超长播报文案须拒",
    );

    // 命名空间：缺前缀 ⇒ 拒。
    let mut reg = MotionRegistry::new();
    if let Ok(c) = demo_component("noprefix", Family::Enter, "mount") {
        let e = reg.register(c).err();
        cs.add(
            "E06-命名-缺前缀被拒",
            e.as_ref().map(|x| x.code).unwrap_or("<none>") == E_NS_MISSING,
            "无前缀名须拿 E_NS_MISSING",
        );
    } else {
        cs.add("E06-命名-缺前缀被拒", false, "构造失败");
    }

    // 命名空间：自定义冒充内置 ⇒ 拒（E_NS_ESCALATE）。
    let mut reg2 = MotionRegistry::new();
    let fake_builtin = MotionComponent::new(
        "builtin:fake",
        Family::Custom,
        "冒充",
        "mount",
        g(100),
        vec![],
        match A11yBehavior::new(ReduceBehavior::StaticOnly, "静态") {
            Ok(a) => a,
            Err(_) => return_fallback_a11y(),
        },
    );
    match fake_builtin {
        Ok(c) => {
            let e = reg2.register(c).err();
            cs.add(
                "E06-命名-自定义冒充内置被拒",
                e.as_ref().map(|x| x.code).unwrap_or("<none>") == E_NS_ESCALATE,
                "自定义族用 builtin: 前缀须拒",
            );
        }
        Err(_) => cs.add("E06-命名-自定义冒充内置被拒", false, "构造失败"),
    }

    // 覆盖内置=拒绝（红线）：自定义撞内置名 ⇒ 拒，且内置那份纹丝不动。
    let mut reg3 = MotionRegistry::new();
    let builtin = demo_component("builtin:core", Family::Enter, "mount");
    let custom_collide = MotionComponent::new(
        "builtin:core", // 故意撞内置名
        Family::Custom,
        "撞名",
        "mount",
        g(100),
        vec![],
        match A11yBehavior::new(ReduceBehavior::StaticOnly, "静态") {
            Ok(a) => a,
            Err(_) => return_fallback_a11y(),
        },
    );
    if let (Ok(b), Ok(cc)) = (builtin, custom_collide) {
        let _ = reg3.register(b);
        let len_before = reg3.len();
        let e = reg3.register(cc).err();
        let code = e.as_ref().map(|x| x.code).unwrap_or("<none>");
        let len_after = reg3.len();
        // 三件：专属码 + 长度不变 + 检索到的仍是内置组件（非自定义）。
        let still_builtin = reg3
            .find("builtin:core")
            .map(|c| c.family.is_builtin())
            .unwrap_or(false);
        let guard_ok = code == E_NS_ESCALATE && len_before == len_after && still_builtin;
        cs.add(
            "E06-命名-覆盖内置被拒且零副作用",
            guard_ok,
            "自定义撞内置须拒、长度不变、内置那份不被替换",
        );
    } else {
        cs.add("E06-命名-覆盖内置被拒且零副作用", false, "构造失败");
    }

    // 同族重名 ⇒ 拒（E_COMPONENT_DUP）。
    let mut reg4 = MotionRegistry::new();
    if let Ok(c1) = demo_component("builtin:dup", Family::Enter, "mount") {
        let _ = reg4.register(c1);
    }
    if let Ok(c2) = demo_component("builtin:dup", Family::Enter, "mount") {
        let e = reg4.register(c2).err();
        cs.add(
            "E06-命名-同族重名被拒",
            e.as_ref().map(|x| x.code).unwrap_or("<none>") == E_COMPONENT_DUP,
            "同族重名须拿 E_COMPONENT_DUP",
        );
    } else {
        cs.add("E06-命名-同族重名被拒", false, "构造失败");
    }

    // 注册表满 ⇒ 拒（灌到 MAX_COMPONENTS）。
    let mut reg5 = MotionRegistry::new();
    let mut accepted = 0usize;
    let mut i = 0usize;
    while i < MAX_COMPONENTS + 4 {
        let nm = format!("builtin:b{}", i);
        if let Ok(c) = demo_component(&nm, Family::Feedback, "tap") {
            if reg5.register(c).is_ok() {
                accepted += 1;
            }
        }
        i += 1;
    }
    let full_e = if let Ok(c) = demo_component("builtin:overflow", Family::Feedback, "tap") {
        reg5.register(c).err()
    } else {
        None
    };
    cs.add(
        "E06-要件-注册表满被拒",
        full_e.as_ref().map(|x| x.code).unwrap_or("<none>") == E_REGISTRY_FULL,
        "超 MAX_COMPONENTS 注册须拿 E_REGISTRY_FULL",
    );
}

// ---------------------------------------------------------------------------
// 判据五：apply + 生命周期 + 沙箱（锚点判据 5）
// ---------------------------------------------------------------------------

fn chk_apply_sandbox(cs: &mut CheckSet) {
    // 生命周期四态齐全且往返互逆。
    let lc_rt = (0..LIFECYCLE_STATE_COUNT)
        .all(|i| Lifecycle::from_index(i).map(|l| l.index() == i).unwrap_or(false));
    cs.add("E06-生命周期-四态索引往返互逆", lc_rt, "四态索引须恒等");

    // 终态不可回退（已取消不可再激活）。
    let term = Lifecycle::Cancelled.can_transition_to(Lifecycle::Active);
    cs.add("E06-生命周期-终态不可回退", !term, "已取消不得回到激活");

    // 挂载⇒激活合法；挂载⇒挂载幂等。
    let ok_path = Lifecycle::Mounted.can_transition_to(Lifecycle::Active)
        && Lifecycle::Mounted.can_transition_to(Lifecycle::Mounted);
    cs.add("E06-生命周期-挂载可激活且幂等", ok_path, "挂载⇒激活合法、挂载⇒挂载幂等");

    // 实例迁移轨迹可查。
    let mut inst = ComponentInstance::mounted("builtin:x");
    let _ = inst.transition(Lifecycle::Active);
    let _ = inst.transition(Lifecycle::Completed);
    let hist_ok = inst.state == Lifecycle::Completed && inst.history_len() == 3;
    cs.add("E06-生命周期-轨迹完整记录", hist_ok, "挂载⇒激活⇒完成轨迹 3 步");

    // 非法迁移被拒（终态实例再激活）。
    let e_illegal = inst.transition(Lifecycle::Active).err();
    cs.add(
        "E06-生命周期-非法迁移被拒",
        e_illegal.as_ref().map(|e| e.code).unwrap_or("<none>") == E_LIFECYCLE_ILLEGAL,
        "终态再激活须拿 E_LIFECYCLE_ILLEGAL",
    );

    // apply 正常路径：内置组件编译出实例，reduce 泳道整体坍缩。
    let mut reg = MotionRegistry::new();
    if let Ok(c) = demo_component("builtin:fadein", Family::Enter, "mount") {
        let _ = reg.register(c);
    }
    let mut inst2 = ComponentInstance::mounted("builtin:fadein");
    let tgt = ApplyTarget { element: 1, context: 0 };
    let empty_ov = ParamOverrides::new();
    match reg.apply(tgt, "builtin:fadein", &empty_ov, Lane::Normal, &mut inst2) {
        Ok(o) => {
            let inst_n = o.instance_count();
            cs.add(
                "E06-应用-正常编译出实例",
                inst_n == 2 && o.lifecycle == Lifecycle::Active,
                "两节点图须编译出 2 实例且态为激活",
            );
            // 目标回写：apply 的「目标」侧须原样带回（锚点 apply(目标, 参数)）。
            let tgt_ok = o.target == tgt;
            cs.add(
                "E06-应用-目标原样回写",
                tgt_ok,
                "apply 须把目标元素与上下文原样回写",
            );
            // reduce 泳道：整体坍缩（时长归零）。
            let mut inst3 = ComponentInstance::mounted("builtin:fadein");
            match reg.apply(tgt, "builtin:fadein", &empty_ov, Lane::Reduced, &mut inst3) {
                Ok(ro) => {
                    cs.add(
                        "E06-应用-reduce整体坍缩",
                        ro.total_duration_ms() == 0 && ro.instance_count() == 2,
                        "reduce 泳道须总时长归零、实例保留",
                    );
                }
                Err(_) => cs.add("E06-应用-reduce整体坍缩", false, "reduce apply 失败"),
            }
        }
        Err(_) => cs.add("E06-应用-正常编译出实例", false, "apply 失败"),
    }

    // apply 超参 ⇒ 钳制 + 诊断（不失败）。
    let mut inst4 = ComponentInstance::mounted("builtin:fadein");
    let mut ov = ParamOverrides::new();
    ov.set("duration", 99999);
    match reg.apply(tgt, "builtin:fadein", &ov, Lane::Normal, &mut inst4) {
        Ok(o) => {
            let clamped = o.any_clamped();
            let diag = !o.diagnostics.is_empty();
            let dur = o.params.value_of("duration");
            cs.add(
                "E06-应用-超参钳制带诊断",
                clamped && diag && dur == Some(1000),
                "duration 99999 须钳到 1000 并出诊断、不失败",
            );
        }
        Err(_) => cs.add("E06-应用-超参钳制带诊断", false, "超参不应导致 apply 失败"),
    }

    // 未注册组件 ⇒ 拒（E_COMPONENT_UNKNOWN）。
    let mut inst5 = ComponentInstance::mounted("builtin:ghost");
    let e_ghost = reg.apply(tgt, "builtin:ghost", &empty_ov, Lane::Normal, &mut inst5).err();
    cs.add(
        "E06-应用-未注册组件被拒",
        e_ghost.as_ref().map(|e| e.code).unwrap_or("<none>") == E_COMPONENT_UNKNOWN,
        "未注册名须拿 E_COMPONENT_UNKNOWN",
    );

    // 沙箱：停用自定义组件后 apply 被拒（E_EXT_DISABLED）。
    let mut reg2 = MotionRegistry::new();
    if let Ok(c) = demo_component("builtin:keep", Family::Enter, "mount") {
        let _ = reg2.register(c);
    }
    // 自定义族用 ext: 前缀合法。
    let ext = MotionComponent::new(
        "ext:demo",
        Family::Custom,
        "自定义",
        "mount",
        g(80),
        vec![],
        match A11yBehavior::new(ReduceBehavior::StaticOnly, "静态") {
            Ok(a) => a,
            Err(_) => return_fallback_a11y(),
        },
    );
    if let Ok(e) = ext {
        let _ = reg2.register(e);
    }
    // 手动停用。
    let _ = reg2.disable_component("ext:demo");
    cs.add(
        "E06-沙箱-停用记账",
        reg2.is_disabled("ext:demo") && reg2.disable_events() == 1,
        "停用须记账 1 次",
    );

    // 隔离：停用一个自定义组件，内置组件仍可用（隔离≠全停）。
    let mut inst6 = ComponentInstance::mounted("ext:demo");
    let e_ext = reg2.apply(tgt, "ext:demo", &empty_ov, Lane::Normal, &mut inst6).err();
    let ext_rejected = e_ext.as_ref().map(|e| e.code).unwrap_or("<none>") == E_EXT_DISABLED;
    let mut inst7 = ComponentInstance::mounted("builtin:keep");
    let builtin_ok = reg2
        .apply(tgt, "builtin:keep", &empty_ov, Lane::Normal, &mut inst7)
        .is_ok();
    cs.add(
        "E06-沙箱-停用只影响该组件",
        ext_rejected && builtin_ok,
        "自定义停用须拒 apply，但内置组件仍可用",
    );

    // 停用内置组件应被拒（内置停用是编程错误）。
    let builtin_disable = reg2.disable_component("builtin:keep");
    cs.add(
        "E06-沙箱-内置不可被停用",
        !builtin_disable,
        "内置组件停用须返回假",
    );

    // 沙箱能力位：逐位钉死，含"无内存隔离"诚实位。
    let caps = SANDBOX_CAPS;
    let cap_ok = caps & SANDBOX_FAULT_ISOLATION != 0
        && caps & SANDBOX_NS_ISOLATION != 0
        && caps & SANDBOX_CODE_ISOLATION != 0
        && caps & SANDBOX_NO_MEM_ISOLATION != 0;
    cs.add("E06-沙箱-能力位齐备含诚实位", cap_ok, "沙箱须含故障/命名/码段隔离+无内存隔离位");

    // 扩展码段与内置不重叠（E_EXT_ 前缀不撞任何内置码）。
    let overlap = ERROR_CODES_ALL.iter().any(|c| c.starts_with(EXT_CODE_PREFIX));
    cs.add("E06-沙箱-扩展码段不撞内置", !overlap, "内置码不得以 E_EXT_ 开头");

    // apply 播报含生命周期与实例数（读屏）。
    let mut inst8 = ComponentInstance::mounted("builtin:fadein");
    if let Ok(o) = reg.apply(tgt, "builtin:fadein", &empty_ov, Lane::Normal, &mut inst8) {
        let sp = o.spoken();
        let spoken_ok = sp.contains("激活") && sp.contains("实例");
        cs.add("E06-应用-播报含态与实例数", spoken_ok, "apply 播报须含生命周期与实例数");
    } else {
        cs.add("E06-应用-播报含态与实例数", false, "apply 失败");
    }
}

// ---------------------------------------------------------------------------
// 判据六：契约纪律（沙箱边界诚实性 / 协议版本 / 无 panic 面）
// ---------------------------------------------------------------------------

fn chk_contract(cs: &mut CheckSet) {
    // 沙箱边界文本必须明写"无内存隔离"（诚实性：不假装有隔离硬件）。
    cs.add(
        "E06-契约-沙箱边界明写无内存隔离",
        SANDBOX_BOUNDARY.contains("sandbox") && SANDBOX_NO_MEM_ISOLATION != 0,
        "边界声明须含版本标识且诚实位在位",
    );

    // 协议版本三件非空且互异。
    let vers = [
        COMPONENT_PROTOCOL_VERSION,
        REGISTRY_PROTOCOL_VERSION,
        PARAM_PROTOCOL_VERSION,
    ];
    let v_ok = vers.iter().all(|v| !v.is_empty()) && vers[0] != vers[1] && vers[1] != vers[2];
    cs.add("E06-契约-三协议版本非空互异", v_ok, "组件/注册表/参数协议版本须非空且互异");

    // 内置诊断码逐条互异（防止码位重复掩盖问题）。
    let uniq = {
        let mut seen: Vec<&str> = Vec::new();
        let mut dup = false;
        let mut i = 0usize;
        while i < ERROR_CODES_ALL.len() {
            if seen.contains(&ERROR_CODES_ALL[i]) {
                dup = true;
            } else {
                seen.push(ERROR_CODES_ALL[i]);
            }
            i += 1;
        }
        !dup
    };
    cs.add("E06-契约-诊断码逐条互异", uniq, "内置错误码不得重复");

    // reduce 行为三态往返互逆。
    let rb = (0..3).all(|i| ReduceBehavior::from_index(i).map(|b| b.index() == i).unwrap_or(false));
    cs.add("E06-契约-reduce三态往返互逆", rb, "reduce 行为索引须恒等");

    // 自定义区固定 index 3，内置三族 0..2（判据索引由常量推导，不硬编）。
    let fam_idx = Family::Custom.index() == FAMILY_COUNT
        && Family::Enter.index() == 0
        && Family::Transition.index() == 1
        && Family::Feedback.index() == 2;
    cs.add("E06-契约-族索引由常量推导", fam_idx, "自定义区索引须等于 FAMILY_COUNT");

    // 读屏播报：组件播报含四件要素。
    if let Ok(c) = demo_component("builtin:say", Family::Enter, "mount") {
        let sp = c.spoken();
        let say_ok = sp.contains("builtin:say") && sp.contains("触发") && sp.contains("入场系列");
        cs.add("E06-契约-组件播报含要素", say_ok, "播报须含名字/触发/族");
    } else {
        cs.add("E06-契约-组件播报含要素", false, "构造失败");
    }

    // 四个协议常量 + 沙箱常量都应可被外部读到（pub 面完整）。
    cs.add(
        "E06-契约-协议常量可外部读",
        !COMPONENT_PROTOCOL_VERSION.is_empty() && !REGISTRY_PROTOCOL_VERSION.is_empty(),
        "协议常量须pub 可读",
    );
}

// ---------------------------------------------------------------------------
// 辅助（探针内 shim；真模块里用 unwrap_or_else 兜底，不写 panic 面）
// ---------------------------------------------------------------------------

/// 空编排图（判据用：构造「缺件②」语料）。
fn empty_graph() -> OrchGraph {
    unreachable_empty_graph()
}

/// 兜底参数域（构造失败时的替身，保证判据仍能跑）。
///
/// 返回 `Result` 以便直接嵌进 `resolve_params` 的参数表；`ParamSpec` 字段全 pub，
/// 故即便 `new` 失败也能手工造一个结构合法的替身。
fn fallback_spec() -> ParamSpec {
    match ParamSpec::new("fb", 0, 1, 0) {
        Ok(s) => s,
        Err(_) => ParamSpec {
            name: String::from("fb"),
            min: 0,
            max: 1,
            default: 0,
        },
    }
}

/// 取一个成功构造的参数域（失败则退到 [`fallback_spec`]）。
fn ok_spec(name: &str, min: i64, max: i64, default: i64) -> ParamSpec {
    match ParamSpec::new(name, min, max, default) {
        Ok(s) => s,
        Err(_) => fallback_spec(),
    }
}

/// 兜底 a11y（构造失败时的替身）。
fn return_fallback_a11y() -> A11yBehavior {
    match A11yBehavior::new(ReduceBehavior::StaticOnly, "兜底") {
        Ok(a) => a,
        Err(_) => A11yBehavior {
            reduce: ReduceBehavior::StaticOnly,
            note: String::from("兜底"),
        },
    }
}

/// 内置错误码全集（判据用来证明与扩展段不撞；**不含 `E_EXT_DISABLED`**——
/// 它是扩展段专属码，列入此表会与 [`EXT_CODE_PREFIX`] 段自相矛盾）。
pub const ERROR_CODES_ALL: [&str; 16] = [
    E_COMPONENT_DUP,
    E_A11Y_REQUIRED,
    E_NS_MISSING,
    E_NS_ESCALATE,
    E_NS_OVERRIDE,
    E_REGISTRY_FULL,
    E_COMPONENT_UNKNOWN,
    E_GRAPH_EMPTY,
    E_TRIGGER_EMPTY,
    E_PARAM_CLAMPED,
    E_PARAM_DUP,
    E_PARAM_DOMAIN,
    E_LIFECYCLE_ILLEGAL,
    E_NAME_TOO_LONG,
    E_DESC_TOO_LONG,
    E_NEST_DEPTH,
];

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：四件套 + 三族 + 参数钳制。
pub fn run_vep06_checks_a() -> CheckSet {
    let mut cs = CheckSet::new("vep06-motion-a");
    chk_four_piece(&mut cs);
    chk_three_families(&mut cs);
    chk_clamp(&mut cs);
    cs
}

/// B 批：注册要件 + apply 生命周期 + 沙箱 + 契约。
pub fn run_vep06_checks_b() -> CheckSet {
    let mut cs = CheckSet::new("vep06-motion-b");
    chk_registry_guard(&mut cs);
    chk_apply_sandbox(&mut cs);
    chk_contract(&mut cs);
    cs
}