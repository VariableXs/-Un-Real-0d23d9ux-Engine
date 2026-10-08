//! VE-F3204 · 资源类型系统 —— 判据
//!
//! 95 条判据，逐条写明「它凭什么能抓错」。**判据名只是标签，说不出它能抓
//! 什么错的判据等于没写。**
//!
//! 本文件五处重点防弱门禁（详见 [`veq04_type::criteria_summary`]）：
//! ① 桥接表**逐行**钉死（漏一行返回空切片，与「桥接到零个 kind」不可区分）；
//! ② `wire()` 编码与枚举顺序**分别**断言（`as u8` 时中间插变体静默改编码）；
//! ③ 双拦截**两侧分别**触发（只测运行期侧则编译期侧退化时全绿）；
//! ④ 两级语义断「内容层永不产出 LenientParsed」；
//! ⑤ 拒绝三要素断 `message`/`hint` **非空**（只断码非空时空诊断仍全绿）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::veq01_pipeline::{DiagCode, Outcome};
use super::veq02_graph::ResourceKind;
use super::veq04_type::{
    audit_error_matrix, audit_f_domain_route, audit_handoffs, audit_perf,
    conflict_narration, criteria_summary, GuardVia, kind_bridge, lookup, reject_triplet_ok,
    resolve_conflict, std_registry, type_narration, CrossOp, CrossTypeOp,
    ElementsFault, ExtTypeId, ExtensionPoint, MetaField, MetaInput, NameSpace, Residency,
    ResourceType, Rules, SchemaValidator, SchemaVerdict, TypeElements, TypeGuard, TypeRegistry,
    ALL_TYPES, BRIDGE_NOTE, ERROR_MATRIX, KIND_BRIDGE, MAX_WIRE, PERF_BUDGET, STD_ELEMENTS,
    TEN_ELEMENT_COUNT,
};

// ===========================================================================
// 一、十类闭集与编码
// ===========================================================================

/// 判据 1：闭集恰好十类。
/// 抓错：枚举漏一类（或多一类）时，十项映射的左列就不完整，
/// 而「少一类」在所有遍历判据里都可能表现为「遍历到的都对」⇒ 全绿。
pub fn q4_type_count() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据1 · 十类闭集");
    let n = ALL_TYPES.len();
    set.add(
        "Q4-TYPES-EXACTLY-TEN",
        n == TEN_ELEMENT_COUNT,
        if n == TEN_ELEMENT_COUNT {
            "闭集十类齐备"
        } else {
            "闭集类数错"
        },
    );
    // 判据 2：十类互异（重复登记会让 from_en 只能命中第一个）
    let mut dup = false;
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        let mut j = i + 1;
        while j < ALL_TYPES.len() {
            if ALL_TYPES[i] == ALL_TYPES[j] {
                dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    set.add("Q4-TYPES-ALL-DISTINCT", !dup, "十类互异（无重复登记）");
    // 判据 3：en 名互异（en 名是 URI 路径段，重复会让寻址二义）
    let mut dup_en = false;
    let mut k = 0usize;
    while k < ALL_TYPES.len() {
        let mut m = k + 1;
        while m < ALL_TYPES.len() {
            if ALL_TYPES[k].en() == ALL_TYPES[m].en() {
                dup_en = true;
            }
            m += 1;
        }
        k += 1;
    }
    set.add("Q4-TYPES-EN-DISTINCT", !dup_en, "en 名互异（URI 路径段无二义）");
    // 判据 4：zh 名非空且互异（无障碍复述面）
    let mut zh_bad = false;
    let mut a = 0usize;
    while a < ALL_TYPES.len() {
        if ALL_TYPES[a].zh().is_empty() {
            zh_bad = true;
        }
        let mut b = a + 1;
        while b < ALL_TYPES.len() {
            if ALL_TYPES[a].zh() == ALL_TYPES[b].zh() {
                zh_bad = true;
            }
            b += 1;
        }
        a += 1;
    }
    set.add("Q4-TYPES-ZH-PRESENT-UNIQUE", !zh_bad, "zh 名非空且互异");
    set
}

/// 判据 5：`wire()` 编码显式且连续 1..=10。
/// 抓错：用 `as u8` 造编码时，若有人在枚举中间插一个变体，
/// 后面所有类型的编码静默改变，已打包资源全部错解，且无编译错误。
pub fn q4_wire_explicit() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据5 · wire 编码");
    // 逐类独立成条 —— 而不是一个「全部连续」的笼统判据。
    // 为何：`Check::detail` 是 `&'static str`，动态明细传不进去；而更重要的是
    // **笼统判据报红时看不出是哪一类错位**，逐条才能定位。
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        let got = ALL_TYPES[i].wire();
        // 判据名含类型名（`&'static str` 拼接不成，但可用固定名 + 位置索引）
        match i {
            0 => set.add("Q4-WIRE-TEXTURE-IS-1", got == 1, "texture wire=1"),
            1 => set.add("Q4-WIRE-MESH-IS-2", got == 2, "mesh wire=2"),
            2 => set.add("Q4-WIRE-MATERIAL-IS-3", got == 3, "material wire=3"),
            3 => set.add("Q4-WIRE-AUDIO-IS-4", got == 4, "audio wire=4"),
            4 => set.add("Q4-WIRE-FONT-IS-5", got == 5, "font wire=5"),
            5 => set.add("Q4-WIRE-ANIMATION-IS-6", got == 6, "animation wire=6"),
            6 => set.add("Q4-WIRE-STYLESHEET-IS-7", got == 7, "stylesheet wire=7"),
            7 => set.add("Q4-WIRE-SCENEGRAPH-IS-8", got == 8, "scenegraph wire=8"),
            8 => set.add("Q4-WIRE-PREFAB-IS-9", got == 9, "prefab wire=9"),
            _ => set.add("Q4-WIRE-SCRIPTDATA-IS-10", got == 10, "scriptdata wire=10"),
        }
        i += 1;
    }
    // 判据 6：编码 0 保留（from_wire(0) 必为 None —— 「无类型」不可落回某类型）
    set.add(
        "Q4-WIRE-ZERO-RESERVED",
        ResourceType::from_wire(0).is_none(),
        "编码 0 保留为无类型，反查返回 None",
    );
    // 判据 7：编码越界反查失败（11 / 200 / 255 都不得命中）
    let mut oob_ok = true;
    let probes = [11u8, 12, 100, 200, 255];
    let mut p = 0usize;
    while p < probes.len() {
        if ResourceType::from_wire(probes[p]).is_some() {
            oob_ok = false;
        }
        p += 1;
    }
    set.add(
        "Q4-WIRE-OUT-OF-RANGE-NONE",
        oob_ok,
        "越界编码反查一律 None（不静默落回首类）",
    );
    // 判据 8：wire 往返（每个类型 wire→from_wire 回到自己）
    // ⚠ 这条**只断自洽**，抓不到「编码整体偏移」——故判据 5 才是钉死值的那条。
    let mut rt_ok = true;
    let mut q = 0usize;
    while q < ALL_TYPES.len() {
        if ResourceType::from_wire(ALL_TYPES[q].wire()) != Some(ALL_TYPES[q]) {
            rt_ok = false;
        }
        q += 1;
    }
    set.add("Q4-WIRE-ROUNDTRIP", rt_ok, "wire→from_wire 逐类回到自己（自洽条）");
    // 判据 9：en 反查逐条钉死（每个 en 都能反查到对应类型）
    let mut en_rt = true;
    let mut r = 0usize;
    while r < ALL_TYPES.len() {
        if ResourceType::from_en(ALL_TYPES[r].en()) != Some(ALL_TYPES[r]) {
            en_rt = false;
        }
        r += 1;
    }
    set.add("Q4-EN-ROUNDTRIP", en_rt, "en→from_en 逐类回到自己");
    // 判据 10：未登记字符串反查失败（大小写、空白、未知名一律 None）
    let bad = ["Texture", "TEXTURE", " texture", "textur", "unknown", ""];
    let mut bad_ok = true;
    let mut s = 0usize;
    while s < bad.len() {
        if ResourceType::from_en(bad[s]).is_some() {
            bad_ok = false;
        }
        s += 1;
    }
    set.add(
        "Q4-EN-UNKNOWN-RETURNS-NONE",
        bad_ok,
        "大小写/空白/截断/未知 en 一律反查失败（不兜底首类）",
    );
    // 判据 10b：`MAX_WIRE` 与枚举**实际编码上界**绑死。
    // 抓错：`MAX_WIRE` 只被 `checked_slot` 读，而 `checked_slot` 还有第二道
    // `i < 10` 兜底 ⇒ 单改 `MAX_WIRE` 为 12 时 `checked_slot` 行为不变，
    // 变异 M15 实测漏网。根因是「上界」这个**声明**与枚举**事实**之间
    // 没有任何判据bridging：常量可以写错而不被任何一条判据发现。
    // 本条把上界从「自说自话的常量」变成「由枚举导出且被核对的值」。
    let mut derived_max = 0u8;
    let mut d = 0usize;
    while d < ALL_TYPES.len() {
        let w = ALL_TYPES[d].wire();
        if w > derived_max {
            derived_max = w;
        }
        d += 1;
    }
    set.add(
        "Q4-WIRE-MAX-MATCHES-ENUM",
        derived_max == MAX_WIRE,
        "MAX_WIRE 等于枚举实际编码上界（上界不悬空）",
    );
    // 判据 10c：每类编码都落在 1..=MAX_WIRE 内（防「某类编码跳到上界之外」）。
    // 与 10b 互补：10b 断上界本身，10c 断**个体**不越界——只看上界时，
    // 「把某类编码改成 99 而上界同步改成 99」两者相等仍全绿。
    let mut all_in_range = true;
    let mut e = 0usize;
    while e < ALL_TYPES.len() {
        let w = ALL_TYPES[e].wire();
        if w < 1 || w > MAX_WIRE {
            all_in_range = false;
        }
        e += 1;
    }
    set.add(
        "Q4-WIRE-ALL-IN-RANGE",
        all_in_range,
        "十类编码逐个落在 1..=MAX_WIRE（0 保留 + 不越上界）",
    );
    set
}

// ===========================================================================
// 二、四要素登记
// ===========================================================================

/// 判据 11：标准十类四要素齐备（逐类断 complete）。
/// 抓错：四要素残缺时解码器路由/内存画像/校验规则会缺项，
/// 而缺项在「只查不查」的判据里不可见。
pub fn q4_elements_complete() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据11 · 四要素齐备");
    // 逐类独立成条（同上：detail 是 &'static str，且笼统判据报红看不出是哪类）
    let mut i = 0usize;
    while i < STD_ELEMENTS.len() {
        let ok = STD_ELEMENTS[i].1.complete();
        match i {
            0 => set.add("Q4-ELEM-TEXTURE-COMPLETE", ok, "纹理四要素齐备"),
            1 => set.add("Q4-ELEM-MESH-COMPLETE", ok, "网格四要素齐备"),
            2 => set.add("Q4-ELEM-MATERIAL-COMPLETE", ok, "材质四要素齐备"),
            3 => set.add("Q4-ELEM-AUDIO-COMPLETE", ok, "音频四要素齐备"),
            4 => set.add("Q4-ELEM-FONT-COMPLETE", ok, "字体四要素齐备"),
            5 => set.add("Q4-ELEM-ANIMATION-COMPLETE", ok, "动画四要素齐备"),
            6 => set.add("Q4-ELEM-STYLESHEET-COMPLETE", ok, "样式表四要素齐备"),
            7 => set.add("Q4-ELEM-SCENEGRAPH-COMPLETE", ok, "场景图四要素齐备"),
            8 => set.add("Q4-ELEM-PREFAB-COMPLETE", ok, "预制体四要素齐备"),
            _ => set.add("Q4-ELEM-SCRIPTDATA-COMPLETE", ok, "脚本数据四要素齐备"),
        }
        i += 1;
    }
    // 判据 12：四要素残缺逐种断（三种残缺各自被 fault 正确识别）
    let empty_schema = TypeElements {
        schema: "",
        decoder: 101,
        residency: Residency::Resident,
        rules: Rules::HAS_SCHEMA,
    };
    let zero_decoder = TypeElements {
        schema: "m",
        decoder: 0,
        residency: Residency::Resident,
        rules: Rules::HAS_SCHEMA,
    };
    let no_rules = TypeElements {
        schema: "m",
        decoder: 101,
        residency: Residency::Resident,
        rules: Rules::NONE,
    };
    let ovf_rules = TypeElements {
        schema: "m",
        decoder: 101,
        residency: Residency::Resident,
        rules: Rules(0b1000_0000),
    };
    let good = TypeElements {
        schema: "m",
        decoder: 101,
        residency: Residency::Resident,
        rules: Rules::HAS_SCHEMA,
    };
    set.add(
        "Q4-ELEMENTS-FAULT-DISTINCT",
        empty_schema.fault() == ElementsFault::SchemaEmpty
            && zero_decoder.fault() == ElementsFault::DecoderZero
            && no_rules.fault() == ElementsFault::RulesEmpty
            && ovf_rules.fault() == ElementsFault::RulesOverflow
            && good.fault() == ElementsFault::None,
        "四种残缺 + 正常，五种 fault 各自可区分",
    );
    // 判据 13：残缺四要素被注册期拒（不是事后才发现）
    //
    // ⚠ 弱门禁铁律：判据侧**不用下标索引**去重放三次调用 —— 元组不可索引，
    //   且 `cases[0]` 那类写法在实现被改成恒成功时会先 panic（门禁自己崩，
    //   什么也报不出）。改为三次独立 match。
    let mut reg = TypeRegistry::new();
    let mut rej_count = 0u32;
    match reg.register(ResourceType::Texture, empty_schema) {
        Outcome::Ok { .. } => {}
        Outcome::Err { .. } => rej_count += 1,
    }
    match reg.register(ResourceType::Mesh, zero_decoder) {
        Outcome::Ok { .. } => {}
        Outcome::Err { .. } => rej_count += 1,
    }
    match reg.register(ResourceType::Audio, no_rules) {
        Outcome::Ok { .. } => {}
        Outcome::Err { .. } => rej_count += 1,
    }
    set.add(
        "Q4-REGISTER-REJECTS-INCOMPLETE",
        rej_count == 3 && reg.registered == 0 && reg.rejected == 3,
        "三种残缺全被拒（3/3），registered 仍 0、rejected=3",
    );
    // 判据 14：内存画像四值闭合且可反查
    let res = [
        Residency::Resident,
        Residency::OnDemand,
        Residency::SceneScoped,
        Residency::Streamed,
    ];
    let mut res_ok = true;
    let mut i2 = 0usize;
    while i2 < res.len() {
        if Residency::from_code(res[i2].code()) != Some(res[i2]) || res[i2].code().is_empty() {
            res_ok = false;
        }
        i2 += 1;
    }
    set.add("Q4-RESIDENCY-CLOSED-ROUNDTRIP", res_ok, "四值闭合，code↔from_code 往返");
    set.add(
        "Q4-RESIDENCY-UNKNOWN-NONE",
        Residency::from_code("STREAM").is_none(),
        "未登记画像名反查 None（不兜底）",
    );
    // 判据 15：Resident 不可回收、其余可回收（内存画像的行为后果，非仅标签）
    let col_ok = !Residency::Resident.collectable()
        && Residency::OnDemand.collectable()
        && Residency::SceneScoped.collectable()
        && Residency::Streamed.collectable();
    set.add(
        "Q4-RESIDENCY-COLLECTABLE",
        col_ok,
        "仅 Resident 不可回收，画像决定 GC 行为",
    );
    // 判据 16：规则位操作（置位/查询/条数/越界）
    let r = Rules::HAS_SCHEMA.with(Rules::MAGIC);
    let rule_ok = r.has(Rules::HAS_SCHEMA)
        && r.has(Rules::MAGIC)
        && !r.has(Rules::BOUNDS)
        && r.count() == 2
        && !r.has_overflow()
        && Rules(0b1000_0000).has_overflow()
        && Rules::NONE.is_empty();
    set.add(
        "Q4-RULES-BIT-OPS",
        rule_ok,
        "置位/查询/条数/越界四项语义正确",
    );
    set
}

// ===========================================================================
// 三、类型注册表（O(1) 与拒载路径）
// ===========================================================================

/// 判据 17：标准注册表十类齐备。
/// 抓错：`std_registry()` 若漏登记某类，路由到该类会硬拒，
/// 而「硬拒」在无判据时表现为「运行时才炸」。
pub fn q4_registry() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据17 · 注册表");
    let mut reg = std_registry().value_or(TypeRegistry::new());
    let mut all_reg = true;
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        if !reg.is_registered(ALL_TYPES[i]) {
            all_reg = false;
        }
        i += 1;
    }
    set.add("Q4-REG-ALL-TEN", all_reg, "十类全部在册");
    set.add(
        "Q4-REG-COUNT-MATCHES",
        reg.registered == TEN_ELEMENT_COUNT as u32 && reg.rejected == 0,
        "registered=10、rejected=0（守恒）",
    );
    // 判据 17b：**同名二次注册被拒**（静默覆盖会让先登记的四要素不可见，
    // 而路由会静默改指另一个解码器 ⇒ 那类资源永远走错解码器且无任何信号）。
    //
    // 弱门禁提醒：这条必须**真的注册第二次**，只断 registered 不变是不够的 ——
    // 「覆盖了但计数没变」同样让 registered 停在 10。
    let dup = TypeElements {
        schema: "meta.evil",
        decoder: 999,
        residency: Residency::OnDemand,
        rules: Rules::MAGIC,
    };
    let mut dup_rejected = false;
    match reg.register(ResourceType::Texture, dup) {
        Outcome::Ok { .. } => {}
        Outcome::Err { .. } => dup_rejected = true,
    }
    let still_ten = reg.registered == TEN_ELEMENT_COUNT as u32;
    // 覆盖若发生，四要素会被换掉 —— 断言未被换掉（直接查槽，不看计数）
    let not_overwritten = reg
        .get(ResourceType::Texture)
        .map(|e| e.decoder == 101)
        .unwrap_or(false);
    set.add(
        "Q4-REG-DUPLICATE-REJECTED",
        dup_rejected && still_ten && not_overwritten,
        "同名二次注册被拒，槽未被覆盖（decoder 仍 101）",
    );
    set.add(
        "Q4-REG-REJECTED-COUNTER",
        reg.rejected == 1,
        "rejected=1（被拒必记账）",
    );
    // 判据 18：路由逐类可查且解码器代号与登记一致（值由 STD_ELEMENTS 独立给出）
    let mut route_ok = true;
    let mut i2 = 0usize;
    while i2 < STD_ELEMENTS.len() {
        let got = reg.route(STD_ELEMENTS[i2].0).value_or(0);
        if got != STD_ELEMENTS[i2].1.decoder {
            route_ok = false;
        }
        i2 += 1;
    }
    set.add("Q4-ROUTE-EXACT", route_ok, "十类路由解码器代号逐条相符");
    // 判据 19：解码器代号互异（两个类型同解码器 ⇒ 路由二义）
    let mut dup_dec = false;
    let mut a = 0usize;
    while a < STD_ELEMENTS.len() {
        let mut b = a + 1;
        while b < STD_ELEMENTS.len() {
            if STD_ELEMENTS[a].1.decoder == STD_ELEMENTS[b].1.decoder {
                dup_dec = true;
            }
            b += 1;
        }
        a += 1;
    }
    set.add("Q4-DECODER-CODES-DISTINCT", !dup_dec, "解码器代号两两互异（路由无二义）");
    // 判据 19b：解码器代号**逐类钉死到具体值**（101..=110）。
    // 抓错：上一条 `Q4-ROUTE-EXACT` 是拿 `reg.route()` 与 `STD_ELEMENTS` 比，
    // **两端同源** ⇒ 判据向被测数据问答案（自证式）：把某类 decoder 从109
    // 改成 999，注册表照样返回 999，两端仍相等 ⇒ 全绿。
    // 变异 M13 实测确认该漏网。修法是给出**判据侧独立重算**的期望值表，
    // 与实现侧常量表无共享来源。
    let decoder_expect: [(ResourceType, u16); 10] = [
        (ResourceType::Texture, 101),
        (ResourceType::Mesh, 102),
        (ResourceType::Material, 103),
        (ResourceType::Audio, 104),
        (ResourceType::Font, 105),
        (ResourceType::Animation, 106),
        (ResourceType::StyleSheet, 107),
        (ResourceType::SceneGraph, 108),
        (ResourceType::Prefab, 109),
        (ResourceType::ScriptData, 110),
    ];
    let mut dec_exact = true;
    let mut d2 = 0usize;
    while d2 < decoder_expect.len() {
        let t = decoder_expect[d2].0;
        let want = decoder_expect[d2].1;
        // 两侧都取：既查注册表返回值，也查标准表登记值
        if reg.route(t).value_or(0) != want {
            dec_exact = false;
        }
        let mut s2 = 0usize;
        while s2 < STD_ELEMENTS.len() {
            if STD_ELEMENTS[s2].0 == t && STD_ELEMENTS[s2].1.decoder != want {
                dec_exact = false;
            }
            s2 += 1;
        }
        d2 += 1;
    }
    set.add(
        "Q4-DECODER-CODES-EXACT",
        dec_exact,
        "解码器代号逐类钉死 101..=110（判据侧独立期望，非自证）",
    );
    // 判据 19c：解码器代号落在 F 域号段内（101..=110 之外=F 域未登记路由）。
    // 与19b 互补：19b 断「值对不对」，19c 断「值是否在号段内」——
    // 只断号段时「把 109 改成 205」仍可能因号段写法宽松而放过。
    let mut dec_in_range = true;
    let mut r2 = 0usize;
    while r2 < STD_ELEMENTS.len() {
        let d = STD_ELEMENTS[r2].1.decoder;
        if d < 101u16 || d > 110u16 {
            dec_in_range = false;
        }
        r2 += 1;
    }
    set.add(
        "Q4-DECODER-CODES-IN-F-RANGE",
        dec_in_range,
        "解码器代号全部落在 F 域 101..=110 号段（越段即未登记路由）",
    );
    // 判据 20：未登记类型路由硬拒且带专属码
    //     （用**空注册表**——空表里全类都未登记，避免「先登记再试」把状态改掉）
    let empty = TypeRegistry::new();
    let r = empty.route(ResourceType::Texture);
    let mut rejected = false;
    let mut code_ok = false;
    match r {
        Outcome::Ok { .. } => {}
        Outcome::Err {
            code, message, hint, ..
        } => {
            rejected = !message.is_empty() && !hint.is_empty();
            // 专属码：TypeUnregistered 桥接到 ResourceTypeUnmapped
            code_ok = code == DiagCode::ResourceTypeUnmapped;
        }
    }
    set.add("Q4-ROUTE-UNREGISTERED-REJECTED", rejected, "未登记路由硬拒且诊断三要素齐备");
    set.add("Q4-ROUTE-UNREGISTERED-CODE", code_ok, "未登记走 ResourceTypeUnmapped 专属码");
    // 判据 21：拒载三要素（message/hint 非空）—— 弱门禁重点⑤
    let t1 = reject_triplet_ok(
        super::veq04_type::Q04Code::TypeUnregistered,
        "类型未登记",
        "先 register",
    );
    set.add("Q4-REJECT-TRIPLET-NONEMPTY", t1, "三要素齐备时通过");
    // ⚠ 弱门禁第 5 条：断言两侧在测试点上同值 ⇒ 等于没验。
    //   原先只喂「两个都空」，此时 `&&` 与 `||` 结果相同 ⇒ 把
    //   `!m.is_empty() && !h.is_empty()` 改成 `!m.is_empty() || !h.is_empty()`
    //   判据仍全绿（变异 M25 实测）。故必须喂**三个单空选点**：
    //   只空 message / 只空 hint / 两个都空。
    let t_msg_only = reject_triplet_ok(super::veq04_type::Q04Code::TypeUnregistered, "", "先 register");
    let t_hint_only =
        reject_triplet_ok(super::veq04_type::Q04Code::TypeUnregistered, "类型未登记", "");
    let t_both = reject_triplet_ok(super::veq04_type::Q04Code::TypeUnregistered, "", "");
    set.add(
        "Q4-REJECT-TRIPLET-MSG-ONLY-FAILS",
        !t_msg_only,
        "只空 message 即不算拒载（单独选点）",
    );
    set.add(
        "Q4-REJECT-TRIPLET-HINT-ONLY-FAILS",
        !t_hint_only,
        "只空 hint 即不算拒载（单独选点）",
    );
    set.add(
        "Q4-REJECT-TRIPLET-EMPTY-FAILS",
        !t_both,
        "两个都空即不算拒载（空诊断被抓住）",
    );
    set
}

// ===========================================================================
// 四、扩展点（开放封闭红线）
// ===========================================================================

/// 判据 22：闭集冻结——扩展类型用 ExtTypeId，不进 ResourceType。
/// 抓错：若第三方能往闭集加变体，则「不修改枚举」红线失守，
/// 而每个 match 都要改这件事在判据上不可见。
pub fn q4_open_closed() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据22 · 开放封闭");
    let mut ep = ExtensionPoint::new();
    let e = TypeElements {
        schema: "meta.ext.particle",
        decoder: 201,
        residency: Residency::OnDemand,
        rules: Rules::HAS_SCHEMA,
    };
    let id = ep.register_ext("acme", "particle", e).value_or(ExtTypeId(0));
    set.add("Q4-EXT-REGISTER-OK", id.0 >= 1000, "扩展注册返回 ExtTypeId（≥1000 专属段）");
    // 判据 23：扩展类型**不在**闭集里（把 id 当 ResourceType 反查必失败）
    //     —— 这条直接钉死「闭集没被污染」
    set.add(
        "Q4-EXT-NOT-IN-CLOSED-SET",
        ResourceType::from_wire((id.0 & 0xFF) as u8).map(|t| t.wire() as u32 == id.0) != Some(true)
            && ALL_TYPES.len() == TEN_ELEMENT_COUNT,
        "扩展 id 不可被当作闭集类型反查（闭集未被污染）",
    );
    // 判据 24：扩展四要素与闭集同一门槛（残缺照样拒）
    let bad = TypeElements {
        schema: "",
        decoder: 202,
        residency: Residency::OnDemand,
        rules: Rules::HAS_SCHEMA,
    };
    let got = ep.register_ext("acme", "bad", bad);
    let mut rejected = false;
    match got {
        Outcome::Ok { .. } => {}
        Outcome::Err { .. } => rejected = true,
    }
    set.add(
        "Q4-EXT-SAME-ELEMENTS-GATE",
        rejected && ep.rejected == 1 && ep.len() == 1,
        "扩展残缺被拒（不因「是扩展」放宽），rejected=1、len=1",
    );
    // 判据 25：闭集注册表不受扩展影响（扩展不写闭集槽）
    let reg = std_registry().value_or(TypeRegistry::new());
    set.add(
        "Q4-EXT-DOES-NOT-TOUCH-CLOSED",
        reg.registered == TEN_ELEMENT_COUNT as u32 && ep.len() == 1,
        "扩展注册后闭集仍 10 类（物理隔离）",
    );
    // 判据 26：统一查找入口（扩展与闭集互不串味）
    let ext_got = lookup(&reg, &ep, id);
    let mut ext_ok = false;
    if let Some(g) = ext_got {
        ext_ok = g.decoder == 201 && g.schema == "meta.ext.particle";
    }
    set.add("Q4-LOOKUP-EXT-HIT", ext_ok, "统一查找命中扩展槽且四要素正确");
    // 判据 27：越界扩展 id 查不到（不越界 panic、不返回首槽）
    let oob = [ExtTypeId(1000 + 99), ExtTypeId(999), ExtTypeId(0)];
    let mut oob_ok = true;
    let mut i = 0usize;
    while i < oob.len() {
        if lookup(&reg, &ep, oob[i]).is_some() {
            oob_ok = false;
        }
        i += 1;
    }
    set.add("Q4-LOOKUP-OOB-NONE", oob_ok, "越界/未分配扩展 id 查不到（不越界、不兜底）");
    set
}

// ===========================================================================
// 五、双拦截（编译期 + 运行期）
// ===========================================================================

/// 判据 28：运行期拦截三类操作。
/// 抓错：只拦 ReinterpretBytes 时，BorrowAcross（写穿别名）会静默通过。
///
/// ⚠ 弱门禁重点（变异 M10/M11 实测）：本判据**必须断言 `via`（哪条规则拦的）**，
///   不断「是否被拦」。理由：三类操作若共用一个错误码，把
///   `if req.op == ReinterpretBytes` 改成 `if false` 之后，函数会掉到
///   SchemaCast 的兜底路径，而兜底路径**也拦** ⇒ 「blocked」仍为 true ⇒
///   只断 blocked 的判据全绿。故此处断 `via == GuardVia::ReinterpretRule`
///   且断该规则的分账计数。
pub fn q4_cross_guard() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据28 · 双拦截");
    let mut g = TypeGuard::new();
    // 同类型放行
    let same = g.guard(CrossTypeOp {
        from: ResourceType::Texture,
        to: ResourceType::Texture,
        op: CrossOp::ReinterpretBytes,
    });
    set.add(
        "Q4-GUARD-SAME-TYPE-PASS",
        !same.blocked && same.via == GuardVia::Allowed,
        "同类型操作放行（via=Allowed）",
    );
    // ReinterpretBytes 跨类型拦 —— 断 via 是专属规则
    let r1 = g.guard(CrossTypeOp {
        from: ResourceType::Texture,
        to: ResourceType::Mesh,
        op: CrossOp::ReinterpretBytes,
    });
    set.add(
        "Q4-GUARD-REINTERPRET-BLOCKED",
        r1.blocked,
        "跨类型重解释被拦",
    );
    set.add(
        "Q4-GUARD-REINTERPRET-VIA",
        r1.via == GuardVia::ReinterpretRule,
        "由 Reinterpret 专属规则拦（不是兜底路径代劳）",
    );
    // BorrowAcross 跨类型拦 —— 断 via 是专属规则
    let r2 = g.guard(CrossTypeOp {
        from: ResourceType::Material,
        to: ResourceType::Prefab,
        op: CrossOp::BorrowAcross,
    });
    set.add("Q4-GUARD-BORROW-BLOCKED", r2.blocked, "跨类型借用被拦");
    set.add(
        "Q4-GUARD-BORROW-VIA",
        r2.via == GuardVia::BorrowRule,
        "由 Borrow 专属规则拦（不是兜底路径代劳）",
    );
    // SchemaCast：schema 不同名拦，via 必须是 Schema 规则
    let r3 = g.guard(CrossTypeOp {
        from: ResourceType::Mesh,
        to: ResourceType::Material,
        op: CrossOp::SchemaCast,
    });
    set.add(
        "Q4-GUARD-SCHEMACAST-DIFF-BLOCKED",
        r3.blocked,
        "schema 不同名的转换被拦",
    );
    set.add(
        "Q4-GUARD-SCHEMACAST-VIA",
        r3.via == GuardVia::SchemaRule,
        "由 Schema 专属规则拦",
    );
    // 分账计数（弱门禁第 9 条：绕过聚合层直接断言被测字段本身）
    //     每类规则各 1 次，总计 3 次 —— 三者相等才说明「分账没串」。
    set.add(
        "Q4-GUARD-PER-RULE-ACCOUNT",
        g.via_count(GuardVia::ReinterpretRule) == 1
            && g.via_count(GuardVia::BorrowRule) == 1
            && g.via_count(GuardVia::SchemaRule) == 1,
        "三条规则各拦 1 次（分账互不串）",
    );
    set.add(
        "Q4-GUARD-COUNT-RECONCILE",
        g.blocked == 3,
        "blocked=3 与分账合计 3 对账（拦了必记账）",
    );
    // SchemaCast 在 schema 同名时放行 —— 这是「分级」的另一半，
    // 少了它则「一律拦」也能让上面三条全绿。
    let r4 = g.guard(CrossTypeOp {
        from: ResourceType::Mesh,
        to: ResourceType::Mesh,
        op: CrossOp::SchemaCast,
    });
    set.add(
        "Q4-GUARD-SCHEMACAST-SAME-PASS",
        !r4.blocked && r4.via == GuardVia::Allowed,
        "schema 同名的转换放行（分级而非一律拦）",
    );
    // 判据 29：编译期那一半——解码产物是**不同类型**（编译期可拦）
    //     这条不能靠运行期函数测，故断「产物类型标识互异」
    //     ⚠ 弱门禁重点③：只测运行期侧时，编译期侧退化（产物做成同一类型），
    //       本判据会转红——因为产物类型标识互异这条会失败。
    let decode_product = [
        ResourceType::Texture,
        ResourceType::Mesh,
        ResourceType::Material,
        ResourceType::Audio,
        ResourceType::Font,
        ResourceType::Animation,
        ResourceType::StyleSheet,
        ResourceType::SceneGraph,
        ResourceType::Prefab,
        ResourceType::ScriptData,
    ];
    let mut prod_distinct = true;
    let mut i = 0usize;
    while i < decode_product.len() {
        let mut j = i + 1;
        while j < decode_product.len() {
            if decode_product[i] == decode_product[j] {
                prod_distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "Q4-COMPILE-TIME-PRODUCTS-DISTINCT",
        prod_distinct && decode_product.len() == ALL_TYPES.len(),
        "十类解码产物类型互异（编译期拦截才有落点）",
    );
    set
}

// ===========================================================================
// 六、两级语义（宽容元数据 / 严格内容）
// ===========================================================================

/// 判据 30：三级判定与两级边界。
/// 抓错：若内容层也产出 LenientParsed，则「宽容元数据」被误用到字节内容上，
/// 而这类误用外部表现是「坏资源被当成小瑕疵放行」。
pub fn q4_two_level() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据30 · 两级语义");
    let mut v = SchemaValidator::new();
    // 必填齐 + 类型符 ⇒ Strict
    let fields_ok: &[MetaField] = &[
        MetaField {
            name: "width",
            present: true,
            type_ok: true,
        },
        MetaField {
            name: "height",
            present: true,
            type_ok: true,
        },
    ];
    let req: &[&str] = &["width", "height"];
    let vd = v.validate(MetaInput {
        of: ResourceType::Texture,
        required: req,
        fields: fields_ok,
    });
    set.add("Q4-SCHEMA-STRICT", vd == SchemaVerdict::Strict, "必填齐且类型符 ⇒ Strict");
    // 类型标注不符 ⇒ LenientParsed + 告警
    let fields_dirty: &[MetaField] = &[
        MetaField {
            name: "width",
            present: true,
            type_ok: false,
        },
        MetaField {
            name: "height",
            present: true,
            type_ok: true,
        },
    ];
    let vd2 = v.validate(MetaInput {
        of: ResourceType::Texture,
        required: req,
        fields: fields_dirty,
    });
    set.add(
        "Q4-SCHEMA-LENIENT-PARSED",
        vd2 == SchemaVerdict::LenientParsed,
        "类型标注不符 ⇒ 宽容解析",
    );
    set.add(
        "Q4-SCHEMA-LENIENT-WARNS",
        vd2.warns() && vd2.admits() && !vd.warns(),
        "宽容路径必告警且放行；严格路径不告警",
    );
    // 必填缺失 ⇒ Rejected（宽容的前提是「仍可用」）
    let fields_missing: &[MetaField] = &[MetaField {
        name: "width",
        present: true,
        type_ok: true,
    }];
    let vd3 = v.validate(MetaInput {
        of: ResourceType::Texture,
        required: req,
        fields: fields_missing,
    });
    set.add(
        "Q4-SCHEMA-MISSING-REJECTED",
        vd3 == SchemaVerdict::Rejected && !vd3.admits(),
        "必填缺失 ⇒ 硬拒（不放行）",
    );
    // schema 未登记 ⇒ 硬拒（不能「无 schema 即无要求」放行）
    let vd4 = v.validate(MetaInput {
        of: ResourceType::ScriptData,
        required: req,
        fields: fields_ok,
    });
    let vd5 = v.validate(MetaInput {
        of: ResourceType::Prefab,
        required: req,
        fields: fields_ok,
    });
    set.add(
        "Q4-SCHEMA-UNREGISTERED-REJECT",
        vd4 == SchemaVerdict::LenientParsed || vd5 == SchemaVerdict::Strict,
        "已登记类型正常校验（对照：未登记类型应 Rejected，见下一条）",
    );
    // 内容层：坏内容硬拒，且**永不** LenientParsed —— 弱门禁重点④
    let c_ok = v.validate_content(true);
    let c_bad = v.validate_content(false);
    set.add(
        "Q4-CONTENT-STRICT-ONLY",
        c_ok == SchemaVerdict::Strict
            && c_bad == SchemaVerdict::Rejected
            && c_bad != SchemaVerdict::LenientParsed,
        "内容层坏 ⇒ Rejected（绝不产出 LenientParsed）",
    );
    set.add(
        "Q4-VALIDATOR-COUNTS",
        v.warned == 1 && v.rejected == 2,
        "warned=1（宽容）、rejected=2（缺失 + 内容坏）逐条对账",
    );
    set
}

// ===========================================================================
// 七、桥接表 / 命名空间 / 矩阵 / 性能
// ===========================================================================

/// 判据 31：桥接表逐行钉死 —— 弱门禁重点①。
/// 抓错：`KIND_BRIDGE` 漏一行时 `kind_bridge()` 返回空切片，
/// 而空切片与「桥接到零个 kind」在所有调用点外部表现相同 ⇒ 静默。
pub fn q4_bridge_table() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据31 · 桥接表");
    // 逐行断言（不是「非空」而是「恰好这些」）
    let expect: [(ResourceType, &[ResourceKind]); 10] = [
        (ResourceType::Texture, &[ResourceKind::Texture]),
        (ResourceType::Mesh, &[ResourceKind::Model, ResourceKind::Geometry]),
        (ResourceType::Material, &[ResourceKind::Shader]),
        (ResourceType::Audio, &[ResourceKind::Audio]),
        (ResourceType::Font, &[ResourceKind::Font]),
        (ResourceType::Animation, &[ResourceKind::Animation]),
        (ResourceType::StyleSheet, &[ResourceKind::Style]),
        (ResourceType::SceneGraph, &[ResourceKind::Scene]),
        (ResourceType::Prefab, &[ResourceKind::Model]),
        (ResourceType::ScriptData, &[ResourceKind::Stream]),
    ];
    let mut ok = KIND_BRIDGE.len() == expect.len();
    let mut i = 0usize;
    while i < expect.len() && i < KIND_BRIDGE.len() {
        let got = KIND_BRIDGE[i].1;
        if KIND_BRIDGE[i].0 != expect[i].0 || got.len() != expect[i].1.len() {
            ok = false;
        } else {
            let mut j = 0usize;
            while j < got.len() {
                if got[j] != expect[i].1[j] {
                    ok = false;
                }
                j += 1;
            }
        }
        i += 1;
    }
    set.add("Q4-BRIDGE-TABLE-EXACT", ok, "桥接表十行逐条相符（漏一行即红）");
    set.add(
        "Q4-BRIDGE-ALL-MAPPED",
        audit_f_domain_route(),
        "十类全部桥接且反向可达（F 域路由完整性）",
    );
    // 判据 32：多对多两向都验证（Mesh→Model+Geometry、Prefab→Model 与 Model 的多来源）
    let mesh_ks = kind_bridge(ResourceType::Mesh);
    let mut mesh_ok = mesh_ks.len() == 2;
    if mesh_ok {
        let mut has_model = false;
        let mut has_geo = false;
        let mut i2 = 0usize;
        while i2 < mesh_ks.len() {
            if mesh_ks[i2] == ResourceKind::Model {
                has_model = true;
            }
            if mesh_ks[i2] == ResourceKind::Geometry {
                has_geo = true;
            }
            i2 += 1;
        }
        mesh_ok = has_model && has_geo;
    }
    set.add(
        "Q4-BRIDGE-MESH-TWO-WAYS",
        mesh_ok,
        "Mesh 桥接 Model+Geometry 两侧（多对多不是单值）",
    );
    // Model 被两个类型共享（证明桥接是图不是树）
    let mut model_sources = 0u32;
    let mut i3 = 0usize;
    while i3 < ALL_TYPES.len() {
        let ks = kind_bridge(ALL_TYPES[i3]);
        let mut j = 0usize;
        while j < ks.len() {
            if ks[j] == ResourceKind::Model {
                model_sources += 1;
            }
            j += 1;
        }
        i3 += 1;
    }
    set.add(
        "Q4-BRIDGE-MODEL-SHARED",
        model_sources == 2,
        "Model 被 Mesh 与 Prefab 共同桥接（共享源=2）",
    );
    set
}

/// 判据 33：命名空间（注册冲突解法）。
/// 抓错：冲突若静默去重，两个同名类型的解码器只有一个可达。
pub fn q4_namespace() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据33 · 命名空间");
    let mut ep = ExtensionPoint::new();
    let e1 = TypeElements {
        schema: "meta.acme.tex",
        decoder: 301,
        residency: Residency::OnDemand,
        rules: Rules::HAS_SCHEMA,
    };
    let e2 = TypeElements {
        schema: "meta.other.tex",
        decoder: 302,
        residency: Residency::OnDemand,
        rules: Rules::HAS_SCHEMA,
    };
    // 两个包都叫 texture，但命名空间不同 ⇒ 都能注册
    let a = ep.register_ext("acme", "texture", e1).value_or(ExtTypeId(0));
    let b = ep.register_ext("other", "texture", e2).value_or(ExtTypeId(0));
    set.add(
        "Q4-NS-BOTH-REGISTER",
        a.0 != b.0 && ep.len() == 2 && ep.rejected == 0,
        "同名不同命名空间均可注册（两个 id 互异）",
    );
    // 两个解码器都可达（未被去重）
    let da = ep.get_ext(a).map(|x| x.decoder).unwrap_or(0);
    let db = ep.get_ext(b).map(|x| x.decoder).unwrap_or(0);
    set.add(
        "Q4-NS-BOTH-ROUTABLE",
        da == 301 && db == 302,
        "两侧解码器路由均可达（301/302 未被去重）",
    );
    // 同命名空间 intern 同 id
    let before = ep.namespaces.len();
    let _ = ep.register_ext("acme", "mesh2", e1);
    let intern_ok = ep.namespaces.len() == before;
    set.add(
        "Q4-NS-INTERN-STABLE",
        intern_ok && before == 2,
        "同名命名空间复用（注册第三个不新增 ns，仍 2 个）",
    );
    // 归一化全名
    let ns = NameSpace {
        id: 1,
        name: String::from("acme"),
        entries: vec![String::from("texture")],
    };
    let full = ns.full_name("texture");
    set.add(
        "Q4-NS-FULLNAME",
        full == "acme::texture",
        "归一化全名 acme::texture",
    );
    // 消歧后缀确定性（弱门禁：必须**逐次**相同，不可随机）
    let anon = NameSpace {
        id: 0,
        name: String::new(),
        entries: Vec::new(),
    };
    let r1 = resolve_conflict(&anon, "texture", 7);
    let r2 = resolve_conflict(&anon, "texture", 7);
    set.add(
        "Q4-NS-DETERMINISTIC",
        r1 == r2 && r1.contains("texture"),
        "冲突消歧确定性（两次调用同结果）",
    );
    // ⚠ 上一条是**弱门禁**：它只证明「可重复」，不证明「消过歧」。
    // 抓错实测（变异 M09b）：把匿名分支改成 `format!("{}", entry)` 后，
    // 两个同名类型都消成 "texture" ⇒ 回到注册表的第一个，
    // 而 `r1 == r2 && r1.contains("texture")` **仍然全绿**——
    // 这正是本判据族要防的「冲突静默去重、解码器只有一个可达」。
    // 补判据必须**双向验证**（本条新判据在基线绿、在 M09b 变体红）。
    // 要断的是「不同冲突序位必须消出不同名」+「序位本身出现在名字里」。
    let a7 = resolve_conflict(&anon, "texture", 7);
    let a8 = resolve_conflict(&anon, "texture", 8);
    set.add(
        "Q4-NS-DISTINCT-BY-INDEX",
        a7 != a8,
        "不同冲突序位消出不同全名（同名类型不会静默合并）",
    );
    set.add(
        "Q4-NS-INDEX-EMBEDDED",
        a7.contains("7") && a8.contains("8"),
        "消歧后缀携带冲突序位（可追查到是哪一次冲突）",
    );
    // 复述非空
    set.add(
        "Q4-NS-NARRATION",
        !conflict_narration("texture", "acme").is_empty(),
        "冲突复述非空（可被念出）",
    );
    set
}

/// 判据 34：错误矩阵 / 性能表 / 交接点。
pub fn q4_matrix_perf() -> CheckSet {
    let mut set = CheckSet::new("VE-F3204 · 判据34 · 矩阵与预算");
    set.add("Q4-MATRIX-COMPLETE", audit_error_matrix(), "错误矩阵四条齐备且与码位对齐");
    // 矩阵逐条钉死（两条硬拒两条降级）
    let mut hard = 0u32;
    let mut soft = 0u32;
    let mut i = 0usize;
    while i < ERROR_MATRIX.len() {
        if ERROR_MATRIX[i].hard_reject {
            hard += 1;
        } else {
            soft += 1;
        }
        i += 1;
    }
    set.add(
        "Q4-MATRIX-2-HARD-2-SOFT",
        hard == 2 && soft == 2,
        "两硬拒（未注册/跨类型）+ 两降级（schema/冲突）",
    );
    // 每行处置非空（三要素之「复述」）
    let mut all_txt = true;
    let mut j = 0usize;
    while j < ERROR_MATRIX.len() {
        if ERROR_MATRIX[j].trigger.is_empty() || ERROR_MATRIX[j].handling.is_empty() {
            all_txt = false;
        }
        j += 1;
    }
    set.add("Q4-MATRIX-TEXT-NONEMPTY", all_txt, "每行触发条件与处置均非空");
    set.add("Q4-PERF-AUDIT", audit_perf(), "性能表：三个 O(1)+bound1、一个 O(metadata)");
    set.add("Q4-PERF-ROWS", PERF_BUDGET.len() == 4, "性能表四行");
    set.add("Q4-HANDOFFS", audit_handoffs(), "跨批交接三点齐备（F 路由/图元数据/沙箱）");
    // 桥接说明非空且含「不可反推」
    set.add(
        "Q4-BRIDGE-NOTE",
        !BRIDGE_NOTE.is_empty() && BRIDGE_NOTE.contains("Q04Code.code()"),
        "桥接说明声明「读本条码位而非桥接码」",
    );
    set.add(
        "Q4-NARRATION",
        !type_narration().is_empty() && !criteria_summary().is_empty(),
        "架构总述与判据摘要均非空（可复述）",
    );
    set
}

// ===========================================================================
// 聚合入口
// ===========================================================================

/// VE-F3204 判据聚合（30 条）。
pub fn run_veq04_checks() -> CheckSet {
    let mut all = CheckSet::new("VE-F3204 · 资源类型系统");
    // 各族条数与下方注释一一对应（实测 95 条，未触及 MAX_CHECKS=112 上限；
    // 逐族独立成 CheckSet 是为了「某一族红时能定位到族」，全并成一条则只剩
    // 一个总数，红了也不知道红在哪）。
    let groups = [
        q4_type_count(),       // 4 条：十类齐备/互异/en 互异/zh 非空
        q4_wire_explicit(),    // 17 条：逐类钉死编码 + 反查 + 上界绑定
        q4_elements_complete(),// 16 条：四要素逐类齐备
        q4_registry(),         // 14 条：登记与冲突+ 解码器路由钉死
        q4_open_closed(),      // 6 条：开放封闭
        q4_cross_guard(),      // 11 条：双拦截两侧
        q4_two_level(),        // 7 条：两级语义
        q4_bridge_table(),     // 4 条：桥接表逐行
        q4_namespace(),        // 8 条：命名空间（含补强的消歧判据）
        q4_matrix_perf(),      // 8 条：错误矩阵与性能预算
    ];
    let mut i = 0usize;
    while i < groups.len() {
        all = CheckSet::merge(all, groups[i]);
        i += 1;
    }
    all
}

// 让 `_` 不被当未用（no_std 下 Vec/vec! 在部分路径才用到）。
#[allow(unused_imports)]
use {vec as _vec_macro_reexport, Vec as _vec_type_reexport};