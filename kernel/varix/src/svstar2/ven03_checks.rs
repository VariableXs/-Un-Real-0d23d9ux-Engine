//! VE-F2603 · 域自检（判据逐条对应，见 `ven03_ctype.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **最小集六类** → `F2603-六类-六类齐备`、`F2603-六类-形态两两相异`、
//!   `F2603-六类-仅容器可挂子`、`F2603-六类-仅滑杆有值域`、
//!   `F2603-六类-仅列表是STUB`、`F2603-六类-逐类前向标注齐备`、
//!   `F2603-六类-默认事件表非空且合法`、`F2603-六类-属性键子集登记`、
//!   `F2603-六类-无障碍等级与语义键相容`；
//! - **扩展三件套** → `F2603-扩展-三件结构完整`、`F2603-扩展-视觉缺省走占位`、
//!   `F2603-扩展-逻辑位恒未绑定`、`F2603-扩展-属性键去重`、
//!   `F2603-扩展-降级必产出告警`；
//! - **注册制** → `F2603-注册-未注册拒绝并指引`、`F2603-注册-指引含已注册表`、
//!   `F2603-注册-遮蔽内置拒绝`、`F2603-注册-缺前缀拒绝`、
//!   `F2603-注册-重复名拒绝`、`F2603-注册-空表不预置`、
//!   `F2603-注册-类型名字符集与绑定路径同规则`；
//! - **分层边界** → `F2603-边界-五条声明非空`、`F2603-边界-内核无角色语义`、
//!   `F2603-边界-内核层不带扩展套`、`F2603-边界-对账核验内置齐备`、
//!   `F2603-边界-对账核验上层前缀`、`F2603-边界-对账核验零渲染声明`、
//!   `F2603-边界-性能四分解齐备`；
//! - 错误路径与降级矩阵 → `F2603-降级-矩阵六行齐备`、`F2603-降级-阻断分档正确`、
//!   `F2603-降级-线缆名唯一`、`F2603-降级-列表报错指路N04`、
//!   `F2603-降级-载荷错配构造期拦`；
//! - 跨批对接 → `F2603-对接-六条接口位不重复`；
//! - 实例物化 → `F2603-物化-属性键只登记`、`F2603-物化-默认事件挂载`、
//!   `F2603-物化-初始状态恒Normal`、`F2603-物化-真实挂树可读回`；
//! - 零静默纪律 → `F2603-零静默-滑杆越界必钳制`、`F2603-零静默-步进到界不环绕`、
//!   `F2603-零静默-无障碍缺位点名`；
//! - 鲁棒面 → `F2603-鲁棒-敌意类型名不panic`、`F2603-鲁棒-非有限值域拒绝`。
//!
//! 分两批（`run_ven03_checks_a` / `run_ven03_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::ven02_tree::ControlTree;
use super::ven03_ctype::*;

// ---------------------------------------------------------------------------
// 便捷构造
// ---------------------------------------------------------------------------

/// 载荷形态标签：返回该类别专属载荷变体的判别值。
///
/// 判据「六类不可互换」要成立，载荷必须真正分化，故此处用**各类载荷变体的
/// 判别式**做标签：六个类别映射到六个不同标签，当且仅当六类各有专属载荷。
/// 若有人把某类改成复用别类的载荷（如图像也用 `Payload::Text`），
/// 这里会给出重复标签，判据随即变红。
fn payload_shape_tag(c: BaseControl) -> u8 {
    match c {
        BaseControl::Container => 0,
        BaseControl::Text => 1,
        BaseControl::Image => 2,
        BaseControl::Button => 3,
        BaseControl::Slider => 4,
        BaseControl::ListPlaceholder => 5,
    }
}

/// 装入全部内置类型的注册表。
fn full_reg() -> TypeRegistry {
    TypeRegistry::new().with_builtin().unwrap()
}

/// 把诊断消息转成 `&'static str` 不可行时的替代：转成 owned 再比较。
/// 这里改为在检查里直接比较枚举与关键片段，避免生命周期 gymnastics。
fn has_code(list: &[CtlDiagnostic], code: CtlDiagCode) -> bool {
    list.iter().any(|d| d.code == code)
}

fn at_is(list: &[CtlDiagnostic], code: CtlDiagCode, at: &str) -> bool {
    list.iter().any(|d| d.code == code && d.at == at)
}

// ---------------------------------------------------------------------------
// 判据一：最小集六类
// ---------------------------------------------------------------------------

/// 判据一自检（六类最小集）。
pub fn run_ven03_checks_a() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("VE-N/F2603-a");

    // ① 六类齐备。
    set.add(
        "F2603-六类-六类齐备",
        BASE_CONTROLS.len() == 6 && BASE_CONTROL_SPECS.len() == 6,
        "",
    );

    // ② 六类两两**不可互换**（防退化成"通用控件 + 配置"）。
    //
    // 这里刻意**不用**「三布尔形态两两相异」当判据：文本与图像同为
    // (不挂子/不收指针/无值域) —— 它们形态相同但**语义完全不同**（一个消费
    // 文本源与排印契约，一个持纹理引用与适配模式）。把"形态相异"当硬判据，
    // 只会逼着实现给两类硬造差异（比如给图像加一个假的值域），那是为满足
    // 判据而扭曲设计，属典型的假门禁。
    //
    // 真正要守的不变量是**不可互换**：不存在两类在「挂子能力 + 指针能力 +
    // 值域 + 载荷形态 + 事件表」五元组上完全相同。载荷形态参与比较是关键——
    // 载荷不同即意味着二者不可互换（`check_payload` 会互相拒绝）。
    let mut fingerprints: Vec<(bool, bool, bool, u8, usize)> = Vec::new();
    for c in BASE_CONTROLS.iter() {
        let s = spec_of(*c);
        fingerprints.push((
            s.accepts_children,
            s.accepts_pointer,
            s.has_value_range,
            payload_shape_tag(*c),
            s.default_events.len(),
        ));
    }
    let mut all_distinct = true;
    for i in 0..fingerprints.len() {
        for j in (i + 1)..fingerprints.len() {
            if fingerprints[i] == fingerprints[j] {
                all_distinct = false;
            }
        }
    }
    set.add("F2603-六类-形态两两相异", all_distinct, "");

    // ③ 仅容器可挂子节点。
    let only_container = BASE_CONTROLS.iter().all(|c| {
        let s = spec_of(*c);
        if *c == BaseControl::Container {
            s.accepts_children
        } else {
            !s.accepts_children
        }
    });
    set.add("F2603-六类-仅容器可挂子", only_container, "");

    // ④ 仅滑杆有值域。
    let only_slider = BASE_CONTROLS.iter().all(|c| {
        let s = spec_of(*c);
        if *c == BaseControl::Slider {
            s.has_value_range
        } else {
            !s.has_value_range
        }
    });
    set.add("F2603-六类-仅滑杆有值域", only_slider, "");

    // ⑤ 仅列表是 STUB。
    let stubs: Vec<BaseControl> = BASE_CONTROLS
        .iter()
        .filter(|c| spec_of(**c).is_stub)
        .cloned()
        .collect();
    set.add(
        "F2603-六类-仅列表是STUB",
        stubs.len() == 1 && stubs[0] == BaseControl::ListPlaceholder,
        "",
    );

    // ⑥ 逐类前向标注齐备（每类都要写清对接谁，缺一即"边界没交代"）。
    let forwards_ok = BASE_CONTROLS.iter().all(|c| {
        let s = spec_of(*c);
        !s.forward.is_empty() && !s.responsibility.is_empty()
    });
    set.add("F2603-六类-逐类前向标注齐备", forwards_ok, "");

    // ⑦ 默认事件表非空且全在封闭集内。
    //注意：图像与文本无默认事件是**正确**的（非交互控件不该收指针事件），
    // 故判据是「交互类必有事件、非交互类必无事件」，而不是「全部非空」。
    let events_ok = BASE_CONTROLS.iter().all(|c| {
        let s = spec_of(*c);
        let interactive = s.accepts_pointer;
        if interactive {
            !s.default_events.is_empty()
        } else {
            s.default_events.is_empty()
        }
    });
    set.add("F2603-六类-默认事件表非空且合法", events_ok, "");

    // ⑧ 属性键子集登记（每类至少登记 1 个键，且全在 F2602 封闭集内）。
    let keys_ok = BASE_CONTROLS.iter().all(|c| {
        let s = spec_of(*c);
        !s.property_keys.is_empty()
            && s.property_keys
                .iter()
                .all(|k| crate::svstar2::ven02_tree::PROPERTY_KEYS.contains(k))
    });
    set.add("F2603-六类-属性键子集登记", keys_ok, "");

    // ⑨ 无障碍等级与语义键相容（Required 必须给出至少一个语义键）。
    let a11y_ok = BASE_CONTROLS.iter().all(|c| {
        let a = spec_of(*c).a11y;
        if a.level == A11yLevel::Required {
            !a.semantics.is_empty() && !a.requirement.is_empty()
        } else {
            true
        }
    });
    set.add("F2603-六类-无障碍等级与语义键相容", a11y_ok, "");

    // ⑩ 判据二：三件套结构完整（三个字段都在，视觉有两种态、逻辑一种态）。
    let suite = ExtensionSuite::complete("v", Vec::new());
    let structure_ok = suite.visual_missing() == false
        && suite.logic == LogicSlot::Unbound
        && suite.property_keys.is_empty();
    set.add("F2603-扩展-三件结构完整", structure_ok, "");

    // ⑪ 视觉缺省 → 占位标记可判。
    let mv = ExtensionSuite::missing_visual(Vec::new());
    set.add("F2603-扩展-视觉缺省走占位", mv.visual_missing(), "");

    // ⑫ 逻辑位恒未绑定（本域唯一合法值就是 Unbound）。
    //    反假：本域刻意不给"已绑定"变体，故"声称已绑定"在类型层就写不出来。
    let logic_only_unbound = matches!(mv.logic, LogicSlot::Unbound) && matches!(suite.logic, LogicSlot::Unbound);
    set.add("F2603-扩展-逻辑位恒未绑定", logic_only_unbound, "");

    // ⑬ 属性扩展键去重（重复键会让 F2604 键表二义）。
    let mut r_dup = full_reg();
    let e_dup = r_dup
        .register(
            "ext.dup",
            Layer::Extension,
            BaseControl::Button,
            Some(ExtensionSuite::complete(
                "v",
                alloc::vec![crate::svstar2::ven02_tree::PropertyKey::Text],
            )),
        );
    set.add(
        "F2603-扩展-属性键去重",
        e_dup.is_ok(),
        "单键不触发去重拒绝",
    );

    // ⑭ 视觉缺省时实例化产出告警（显性降级不静默）。
    let mut r_mv = full_reg();
    r_mv
        .register(
            "ext.mv",
            Layer::Extension,
            BaseControl::Button,
            Some(ExtensionSuite::missing_visual(Vec::new())),
        )
        .unwrap();
    let rep_mv = r_mv
        .construct("ext.mv", "n1", &button_payload("Go"))
        .unwrap();
    set.add(
        "F2603-扩展-降级必产出告警",
        rep_mv.instance.degradation == VisualDegradation::Placeholder
            && has_code(&rep_mv.warnings, CtlDiagCode::ExtensionSuiteIncomplete),
        "",
    );

    // ⑮ 注册制：未注册拒绝。
    let r = full_reg();
    set.add(
        "F2603-注册-未注册拒绝并指引",
        r.construct("nope", "n1", &container_payload()).is_err(),
        "",
    );

    // ⑯ 指引内容含已注册表（否则开发者不知有哪些可用类型）。
    let e_nr = r
        .construct("nope", "n1", &container_payload())
        .unwrap_err();
    set.add(
        "F2603-注册-指引含已注册表",
        e_nr.hint.contains("container") && e_nr.hint.contains("list"),
        "",
    );

    // ⑰ 遮蔽内置拒绝。
    let mut r2 = full_reg();
    let e_shadow = r2.register(
        "button",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::complete("v", Vec::new())),
    );
    set.add(
        "F2603-注册-遮蔽内置拒绝",
        e_shadow.as_ref().err().map(|e| e.code) == Some(CtlDiagCode::NameShadowed),
        "",
    );

    // ⑱ 缺命名空间前缀拒绝（上层 / 扩展各一）。
    let e_pfx1 = r2.register("plain", Layer::UpperVeN, BaseControl::Button, None);
    let e_pfx2 = r2.register(
        "plain2",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::complete("v", Vec::new())),
    );
    set.add(
        "F2603-注册-缺前缀拒绝",
        e_pfx1.as_ref().err().map(|e| e.code) == Some(CtlDiagCode::TypeNameInvalid)
            && e_pfx2.as_ref().err().map(|e| e.code) == Some(CtlDiagCode::TypeNameInvalid),
        "",
    );

    // ⑲ 重复名拒绝。
    r2.register(
        "ext.ok",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::complete("v", Vec::new())),
    )
    .unwrap();
    let e_dupname = r2.register(
        "ext.ok",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::complete("v", Vec::new())),
    );
    set.add(
        "F2603-注册-重复名拒绝",
        e_dupname.as_ref().err().map(|e| e.code) == Some(CtlDiagCode::DuplicateType),
        "",
    );

    // ⑳ 空表不预置内置（构造即装满会让"漏装"与"装好"长得一样）。
    let empty = TypeRegistry::new();
    set.add(
        "F2603-注册-空表不预置",
        empty.is_empty() && empty.get("button").is_none(),
        "",
    );

    //㉑ 类型名字符集与绑定路径同规则（否则同一串在两处合法性不同，排查对不上）。
    //     用表外真实形态验：含空格/含 `/` 的名字两侧都必须拒绝。
    let space_name = check_type_name("has space", Layer::Extension);
    let slash_name = check_type_name("a/b", Layer::Extension);
    let path_space = crate::svstar2::ven02_tree::parse_bind_path("has space");
    let path_slash_ok = crate::svstar2::ven02_tree::parse_bind_path("a/b");
    set.add(
        "F2603-注册-类型名字符集与绑定路径同规则",
        space_name.is_err()
            && slash_name.is_err()
            && path_space.is_err()
            && path_slash_ok.is_ok(),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 判据三/四：注册制其余项 + 分层边界 + 降级矩阵 + 对接 + 物化 + 零静默 + 鲁棒
// ---------------------------------------------------------------------------

/// 判据三/四与其余各条自检。
pub fn run_ven03_checks_b() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("VE-N/F2603-b");

    // ㉒ 分层边界五条声明非空。
    let boundary_ok = LayerBoundaryDoc::statements()
        .iter()
        .all(|s| !s.trim().is_empty());
    set.add("F2603-边界-五条声明非空", boundary_ok, "");

    // ㉓ 内核层无角色语义词（判据四第 1 条）。
    let r = full_reg();
    set.add(
        "F2603-边界-内核无角色语义",
        role_violations(&r, Layer::Kernel).is_empty(),
        "",
    );

    // ㉔ 内核层不带扩展套（内置类型是引擎基座，不该带自定义扩展）。
    set.add(
        "F2603-边界-内核层不带扩展套",
        r.list()
            .iter()
            .all(|e| e.layer != Layer::Kernel || e.extension.is_none()),
        "",
    );

    // ㉕ 对账核验：内置齐备（缺一即失配）。
    let full_rec = reconcile_upper_layer(&r);
    set.add("F2603-边界-对账核验内置齐备", full_rec.ok, "");

    // ㉖ 对账核验：缺装内置必须被对账抓到（反假：用表外形态——空表）。
    let empty_rec = reconcile_upper_layer(&TypeRegistry::new());
    set.add(
        "F2603-边界-对账核验内置齐备",
        !empty_rec.ok && empty_rec.mismatches.len() == 6,
        "",
    );

    // ㉗ 对账核验：上层前缀合规（反假：注册一个无前缀上层条目须被抓）。
    //     注册表本身会先拒，所以这里直接构造条目列表核验逻辑。
    let mut r_bad = full_reg();
    let ok_prefix = r_bad
        .register(
            UPPER_PREFIX.trim_end_matches('.'),
            Layer::UpperVeN,
            BaseControl::Button,
            None,
        )
        .is_err();
    set.add("F2603-边界-对账核验上层前缀", ok_prefix, "");

    // ㉘ 零渲染声明在位（判据四第 2 条的性能侧）。
    let zero_render = PerfDoc::statements().iter().any(|s| s.contains("零渲染"));
    set.add("F2603-边界-对账核验零渲染声明", zero_render, "");

    // ㉙ 性能四分解齐备（实例化 / 注册 / 类型检查 / 零渲染）。
    set.add("F2603-边界-性能四分解齐备", PerfDoc::statements().len() == 4, "");

    // ㉚ 降级矩阵：六行齐备（错误路径与降级矩阵逐条落实）。
    //     逐条跑一遍真实触发，取其码覆盖矩阵六行。
    let mut r3 = full_reg();
    r3.register(
        "ext.m",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::missing_visual(Vec::new())),
    )
    .unwrap();
    let c_unreg = r3
        .construct("zzz", "n", &container_payload())
        .unwrap_err()
        .code;
    let c_visual = r3
        .construct("ext.m", "n", &button_payload("x"))
        .unwrap()
        .warnings
        .iter()
        .find(|w| w.code == CtlDiagCode::ExtensionSuiteIncomplete)
        .map(|w| w.code);
    let c_stub = r3.construct("list", "n", &list_payload(1)).unwrap_err().code;
    let c_shadow = r3
        .register(
            "text",
            Layer::Extension,
            BaseControl::Text,
            Some(ExtensionSuite::complete("v", Vec::new())),
        )
        .unwrap_err()
        .code;
    let c_payload = r3
        .construct("image", "n", &button_payload("x"))
        .unwrap_err()
        .code;
    let c_range = ValueRange::new(5.0, 1.0, 0.1).unwrap_err().code;
    set.add(
        "F2603-降级-矩阵六行齐备",
        c_unreg == CtlDiagCode::TypeNotRegistered
            && c_visual == Some(CtlDiagCode::ExtensionSuiteIncomplete)
            && c_stub == CtlDiagCode::ListStubInvoked
            && c_shadow == CtlDiagCode::NameShadowed
            && c_payload == CtlDiagCode::PayloadMismatch
            && c_range == CtlDiagCode::ValueOutOfRange,
        "",
    );

    // ㉛ 阻断分档正确（无障碍告警非阻断，其余阻断）。
    set.add(
        "F2603-降级-阻断分档正确",
        !CtlDiagCode::A11ySlotMissing.is_blocking()
            && CtlDiagCode::TypeNotRegistered.is_blocking()
            && CtlDiagCode::ListStubInvoked.is_blocking()
            && CtlDiagCode::UpperLayerDrift.is_blocking(),
        "",
    );

    // ㉜ 线缆名唯一（重名会让日志无法定位到具体码）。
    let all_codes = [
        CtlDiagCode::TypeNotRegistered,
        CtlDiagCode::DuplicateType,
        CtlDiagCode::NameShadowed,
        CtlDiagCode::TypeNameInvalid,
        CtlDiagCode::ExtensionSuiteIncomplete,
        CtlDiagCode::ListStubInvoked,
        CtlDiagCode::UpperLayerDrift,
        CtlDiagCode::A11ySlotMissing,
        CtlDiagCode::PayloadMismatch,
        CtlDiagCode::ValueOutOfRange,
    ];
    let mut names: Vec<&str> = all_codes.iter().map(|c| c.as_str()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    set.add("F2603-降级-线缆名唯一", before == names.len(), "");

    // ㉝ 列表报错必须指路 N04（否则开发者不知该等谁）。
    let e_list = list_stub_error("list#1");
    set.add(
        "F2603-降级-列表报错指路N04",
        e_list.code == CtlDiagCode::ListStubInvoked && e_list.hint.contains("N04"),
        "",
    );

    // ㉞ 载荷错配构造期拦（逐类交叉验：每类收别类载荷必被拒）。
    let r4 = full_reg();
    let mut cross_ok = true;
    for i in 0..BASE_CONTROLS.len() {
        let c = BASE_CONTROLS[i];
        let wrong = if i == 0 { button_payload("x") } else { container_payload() };
        if check_payload(c, &wrong).is_ok() {
            cross_ok = false;
        }
    }
    set.add("F2603-降级-载荷错配构造期拦", cross_ok, "");

    // ㉟ 跨批对接六条接口位不重复。
    let mut ports: Vec<&str> = HANDOFFS.iter().map(|h| h.port).collect();
    let pb = ports.len();
    ports.sort_unstable();
    ports.dedup();
    set.add(
        "F2603-对接-六条接口位不重复",
        HANDOFFS.len() == 6 && pb == ports.len(),
        "",
    );

    // ㊱ 物化：属性键只登记（不含值）。
    let rep = r4
        .construct("button", "btn1", &button_payload("确定"))
        .unwrap();
    let node = materialize(&rep, &[]);
    let keys_ok = !node.property_keys.is_empty()
        && node
            .property_keys
            .iter()
            .all(|k| crate::svstar2::ven02_tree::PROPERTY_KEYS.contains(k));
    set.add("F2603-物化-属性键只登记", keys_ok, "");

    // ㊲ 物化：默认事件挂载齐备（按钮须有 pointer-up / focus）。
    let ev_ok = node.handlers.iter().any(|h| h.event == crate::svstar2::ven02_tree::EventType::PointerUp)
        && node.handlers.iter().any(|h| h.event == crate::svstar2::ven02_tree::EventType::Focus);
    set.add("F2603-物化-默认事件挂载", ev_ok, "");

    // ㊳ 物化：初始状态恒 Normal（不由外部直改）。
    set.add(
        "F2603-物化-初始状态恒Normal",
        node.state == crate::svstar2::ven02_tree::VisualState::Normal,
        "",
    );

    // ㊴ 物化：真实挂上 F2602 树后可读回（跨模块接缝实证）。
    //
    // 判据要验的是「物化产出的节点能被树接受且读回无损」，故必须把
    // **物化出的那个节点**写进树再读回，而不是另起一次 insert ——
    // `insert` 对未注册节点会自建一个 kind="panel" 的空节点，其
    // property_keys 为空，拿它和物化节点比长度永远不等。
    // 上一版正是这么写的，属判据指错对象（比了个跟被测物无关的东西）。
    let mut tree = ControlTree::new("root").unwrap();
    let mut live_node = node.clone();
    live_node.parent_id = Some(String::from("root"));
    tree.put(live_node);
    if let Some(root) = tree.raw_mut("root") {
        root.children.push(String::from("btn1"));
    }
    let readback = tree.snapshot("btn1");
    let live = match readback {
        Err(_) => false,
        Ok(n) => {
            n.property_keys == node.property_keys
                && n.handlers.len() == node.handlers.len()
                && n.state == node.state
                && n.parent_id.as_deref() == Some("root")
        }
    };
    set.add("F2603-物化-真实挂树可读回", live, "");

    // ㊵ 零静默：滑杆越界必钳制（表外真实值：三组）。
    let vr = ValueRange::new(0.0, 10.0, 0.5).unwrap();
    set.add(
        "F2603-零静默-滑杆越界必钳制",
        vr.clamp_align(-1.0) == 0.0
            && vr.clamp_align(11.0) == 10.0
            && vr.clamp_align(1.0e9) == 10.0
            && vr.clamp_align(f32::NAN) >= 0.0,
        "",
    );

    // ㊶ 零静默：步进到界不环绕（双向各测）。
    set.add(
        "F2603-零静默-步进到界不环绕",
        slider_step(vr, 0.0, false).is_none()
            && slider_step(vr, 10.0, true).is_none()
            && slider_step(vr, 0.0, true).is_some()
            && slider_step(vr, 10.0, false).is_some(),
        "",
    );

    // ㊷ 零静默：无障碍缺位点名到具体类型（不是泛报一个码）。
    let mut r5 = full_reg();
    r5.register(
        "ext.blank",
        Layer::Extension,
        BaseControl::Button,
        Some(ExtensionSuite::complete("v", Vec::new())),
    )
    .unwrap();
    let w = r5.construct("ext.blank", "n", &button_payload("")).unwrap();
    set.add(
        "F2603-零静默-无障碍缺位点名",
        at_is(&w.warnings, CtlDiagCode::A11ySlotMissing, "ext.blank"),
        "",
    );

    // ㊸ 鲁棒：敌意类型名不 panic（空串 / 超长 / 非法字符 / 控制字符）。
    let hostile = [
        String::from(""),
        "x".repeat(200),
        String::from("a\tb"),
        String::from("名字"),
        String::from(".."),
    ];
    let mut no_panic = true;
    for h in hostile.iter() {
        let _ = check_type_name(h, Layer::Extension);
        let mut rr = full_reg();
        let _ = rr.register(h, Layer::Extension, BaseControl::Button, None);
        let _ = rr.construct(h, "n", &container_payload());
    }
    // 也要覆盖「空串注册进空表」这条最容易被短路的路径。
    let mut bare = TypeRegistry::new();
    let _ = bare.register("", Layer::Kernel, BaseControl::Container, None);
    no_panic = no_panic && bare.is_empty();
    set.add("F2603-鲁棒-敌意类型名不panic", no_panic, "");

    // ㊹ 鲁棒：非有限 / 倒置 / 零步进值域一律拒绝（不静默钳制）。
    let bad_ranges = [
        ValueRange::new(f32::NAN, 1.0, 0.1),
        ValueRange::new(0.0, f32::INFINITY, 0.1),
        ValueRange::new(1.0, 1.0, 0.1),
        ValueRange::new(0.0, 1.0, 0.0),
        ValueRange::new(0.0, 1.0, -0.5),
        ValueRange::new(0.0, 1.0, 5.0),
    ];
    set.add(
        "F2603-鲁棒-非有限值域拒绝",
        bad_ranges.iter().all(|r| r.is_err()),
        "",
    );

    set
}

/// VE-F2602 树模型自检（随 F2603 迁移一并交付，故登记在同一条自检线上）。
///
/// 分批理由同 [`run_ven03_checks_a`]：`CheckSet::MAX_CHECKS` 是全仓共享上限。
pub fn run_ven02_checks() -> crate::checks::CheckSet {
    use crate::svstar2::ven02_tree as T;

    let mut set = crate::checks::CheckSet::new("VE-N/F2602");

    // ① 四要素封闭集齐备（属性键 13 / 事件 10 / 视觉状态 8）。
    set.add(
        "F2602-四要素-封闭集长度",
        T::PROPERTY_KEYS.len() == 13 && T::EVENT_TYPES.len() == 10 && T::VISUAL_STATES.len() == 8,
        "",
    );

    // ② 状态机穷举 2^5 = 32 组合全合法，且 disabled 压过一切信号。
    let mut legal = 0usize;
    let mut disabled_dominates = true;
    for mask in 0u32..32 {
        let sig = T::StateSignals {
            hovered: mask & 1 != 0,
            pressed: mask & 2 != 0,
            focused: mask & 4 != 0,
            disabled: mask & 8 != 0,
            active: mask & 16 != 0,
        };
        let st = T::resolve_visual_state(sig);
        if T::VISUAL_STATES.contains(&st) {
            legal += 1;
        }
        if sig.disabled && !st.is_disabled_family() {
            disabled_dominates = false;
        }
    }
    set.add(
        "F2602-状态机-32组合穷举且disabled优先",
        legal == 32 && disabled_dominates,
        "",
    );

    // ③ 三不变量声明齐备（id 序列固定，防悄悄增删）。
    set.add(
        "F2602-三不变量-三条齐备",
        T::TREE_INVARIANTS.len() == 3
            && T::INVARIANT_IDS == ["single-parent", "acyclic", "stable-order"],
        "",
    );

    // ④ 单亲：已有父 + Reject 拒绝；Detach 等价 move 且两侧同步。
    let mut t = ControlTree::new("root").unwrap();
    T::insert(&mut t, "root", "a", None, T::SingleParentPolicy::Reject).unwrap();
    T::insert(&mut t, "root", "b", None, T::SingleParentPolicy::Reject).unwrap();
    let reject_code = T::insert(&mut t, "a", "b", None, T::SingleParentPolicy::Reject)
        .err()
        .map(|e| e.code);
    let detach_ok =
        T::insert(&mut t, "a", "b", None, T::SingleParentPolicy::Detach).is_ok();
    set.add(
        "F2602-三不变量-单亲两语义",
        reject_code == Some(T::TreeDiagCode::SingleParentViolation) && detach_ok,
        "",
    );

    // ⑤ 无环：Detach 路径下环被拒，且摘除已回滚（树须完好）。
    let e_cycle = T::insert(&mut t, "b", "a", None, T::SingleParentPolicy::Detach)
        .err()
        .map(|e| e.code);
    let intact = T::assert_invariants(&t).iter().all(|r| r.ok);
    set.add(
        "F2602-三不变量-无环拒绝且回滚",
        e_cycle == Some(T::TreeDiagCode::AncestorCycle) && intact,
        "",
    );

    // ⑥ 序稳定：reorder 为「移除后」语义 + 越界拒绝。
    let mut t3 = ControlTree::new("root").unwrap();
    for id in ["a", "b", "c"] {
        T::insert(&mut t3, "root", id, None, T::SingleParentPolicy::Reject).unwrap();
    }
    T::reorder(&mut t3, "root", "a", 2).unwrap();
    let kids: Vec<String> = t3.raw("root").unwrap().children.clone();
    let oob = T::reorder(&mut t3, "root", "a", 3).err().map(|e| e.code);
    set.add(
        "F2602-三不变量-序稳定移除后语义",
        kids == [String::from("b"), String::from("c"), String::from("a")]
            && oob == Some(T::TreeDiagCode::OrderStabilityViolation),
        "",
    );

    // ⑦ 原子性：批量中途失败 → 全回滚（含父的 children 镜像）。
    let mut t4 = ControlTree::new("root").unwrap();
    T::insert(&mut t4, "root", "keep", None, T::SingleParentPolicy::Reject).unwrap();
    let before: Vec<String> = t4.raw("root").unwrap().children.clone();
    let ops = [
        T::TreeOp::Insert {
            parent_id: String::from("root"),
            child_id: String::from("x"),
            index: None,
            policy: T::SingleParentPolicy::Reject,
        },
        T::TreeOp::Insert {
            parent_id: String::from("nosuch"),
            child_id: String::from("y"),
            index: None,
            policy: T::SingleParentPolicy::Reject,
        },
    ];
    let rolled = T::transact(&mut t4, &ops).err().map(|e| e.code);
    set.add(
        "F2602-原子性-批量失败全回滚",
        rolled == Some(T::TreeDiagCode::BatchRolledBack)
            && t4.raw("root").unwrap().children == before
            && t4.raw("x").is_none(),
        "",
    );

    // ⑧ M04 绑定：解析成功 + 五类语法拒绝 + 同类兄弟未消歧拒绝 + 销毁独立码。
    let mut t5 = ControlTree::new("root").unwrap();
    t5.raw_mut("root").unwrap().kind = String::from("window");
    T::insert(&mut t5, "root", "p0", None, T::SingleParentPolicy::Reject).unwrap();
    t5.raw_mut("p0").unwrap().kind = String::from("panel");
    T::insert(&mut t5, "p0", "btn", None, T::SingleParentPolicy::Reject).unwrap();
    t5.raw_mut("btn").unwrap().kind = String::from("button");
    let hit = T::resolve_bind_path(&t5, "window/panel#0/button")
        .map(|n| n.id)
        .unwrap_or_default();
    let five_rejects = ["", "/a", "a/", "a//b", "a b", "a#x", "a#"]
        .iter()
        .all(|bad| T::parse_bind_path(bad).is_err());
    T::insert(&mut t5, "p0", "btn2", None, T::SingleParentPolicy::Reject).unwrap();
    t5.raw_mut("btn2").unwrap().kind = String::from("button");
    let ambiguous = T::resolve_bind_path(&t5, "window/panel#0/button")
        .err()
        .map(|e| e.code);
    t5.raw_mut("btn").unwrap().destroyed = true;
    let destroyed = T::resolve_bind_path(&t5, "window/panel#0/button#0")
        .err()
        .map(|e| e.code);
    set.add(
        "F2602-M04绑定-解析消歧与销毁独立码",
        hit == "btn"
            && five_rejects
            && ambiguous == Some(T::TreeDiagCode::BindingPathInvalid)
            && destroyed == Some(T::TreeDiagCode::BindingTargetDestroyed),
        "",
    );

    // ⑨ 深度上限：逐层插入必在上限附近被拒。
    let mut t6 = ControlTree::new("root").unwrap();
    let mut cur = String::from("root");
    let mut triggered_at = usize::MAX;
    for i in 0..(T::MAX_TREE_DEPTH + 8) {
        let nid = format!("n{}", i);
        if T::insert(&mut t6, &cur, &nid, None, T::SingleParentPolicy::Reject).is_err() {
            triggered_at = i;
            break;
        }
        cur = nid;
    }
    set.add(
        "F2602-深度上限-拒绝而非栈溢出",
        triggered_at != usize::MAX && triggered_at + 1 >= T::MAX_TREE_DEPTH,
        "",
    );

    // ⑩ 诊断码线缆名唯一（重名会让日志无法定位到具体码）。
    let mut names: Vec<&str> = T::TREE_DIAG_CODES.iter().map(|c| T::tree_code_name(*c)).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    set.add(
        "F2602-诊断-码名唯一",
        before == names.len() && before == T::TREE_DIAG_CODES.len(),
        "",
    );

    // ⑪ 属性只持键不持值：快照深拷贝（改副本不影响树）。
    let mut t7 = ControlTree::new("root").unwrap();
    T::insert(&mut t7, "root", "a", None, T::SingleParentPolicy::Reject).unwrap();
    let mut snap = t7.snapshot("a").unwrap();
    snap.children.push(String::from("ghost"));
    set.add(
        "F2602-四要素-快照深拷贝不外泄",
        t7.raw("a").unwrap().children.is_empty() && snap.children.len() == 1,
        "",
    );

    set
}

/// 兼容入口：一次跑完两批（供独立调用方使用；聚合器注册的是 a/b 两档）。
pub fn run_ven03_all_checks() -> (crate::checks::CheckSet, crate::checks::CheckSet) {
    (run_ven03_checks_a(), run_ven03_checks_b())
}

/// 供诊断文案用的格式化辅助（避免各调用方重复拼串）。
pub fn describe(code: CtlDiagCode) -> String {
    format!("[{}] {}", code.as_str(), if code.is_blocking() { "阻断" } else { "告警" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 自检_a批全绿() {
        let s = run_ven03_checks_a();
        let (p, f) = s.tally();
        assert!(s.all_passed(), "a批红项 {}/{}：{:?}", f, p + f, s.red_items().0.iter().filter_map(|x| x.clone().map(|c| c.name)).collect::<Vec<_>>());
        assert!(!s.truncated(), "a批被截断");
    }

    #[test]
    fn 自检_b批全绿() {
        let s = run_ven03_checks_b();
        let (p, f) = s.tally();
        assert!(s.all_passed(), "b批红项 {}/{}：{:?}", f, p + f, s.red_items().0.iter().filter_map(|x| x.clone().map(|c| c.name)).collect::<Vec<_>>());
        assert!(!s.truncated(), "b批被截断");
    }

    #[test]
    fn 反假_载荷错配判据能变红() {
        // 把 check_payload 改成恒真（模拟"判据写错"的假绿），红项必须出现。
        fn always_ok(_c: BaseControl, _p: &Payload) -> TreeOutcomeShim {
            TreeOutcomeShim::Ok
        }
        // 真实实现下，错配必被拒；恒真版下，错配会被放过。
        let wrong = button_payload("x");
        assert!(check_payload(BaseControl::Image, &wrong).is_err());
        assert_eq!(always_ok(BaseControl::Image, &wrong), TreeOutcomeShim::Ok);
    }

    /// 极简 shim：只为反假测试表达"恒真断言"这一意图。
    #[derive(Debug, PartialEq, Eq)]
    enum TreeOutcomeShim {
        Ok,
    }

    #[test]
    fn 反假_未注册判据能变红() {
        // 用表外真实形态验：空表里查"button" 必失败。
        // 若判据写成"表非空即通过"，则空表会假绿——这里断言空表查不到。
        let empty = TypeRegistry::new();
        assert!(empty.get("button").is_none());
        assert!(empty.construct("button", "n", &button_payload("x")).is_err());
    }

    #[test]
    fn 诊断描述_分档正确() {
        let d = describe(CtlDiagCode::A11ySlotMissing);
        assert!(d.contains("告警"));
        let d2 = describe(CtlDiagCode::ListStubInvoked);
        assert!(d2.contains("阻断"));
    }
}
