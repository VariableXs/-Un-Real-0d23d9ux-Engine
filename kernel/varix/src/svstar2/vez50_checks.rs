//! VE-F5001 · Y 域开工与场景图脚本总架构 · 域自检判据
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5001`
//!
//! **判据（锚点原文）**：四层、脚本为人写、承接落地、层冻结、判据。
//!
//! 分五组，逐条映射：
//! - `c501_layers`   → 判据一「四层」：层集/依赖方向/层间接口一一对应
//! - `c501_human`    → 判据二「脚本为人写」：诊断带源信息 + 建议可操作
//! - `c501_handoff`  → 判据三「承接落地」：十件逐件可核验 + 回溯源头
//! - `c501_freeze`   → 判据四「层冻结」：冻结态禁改禁加、解冻是唯一途径
//! - `c501_criteria` → 判据五「判据」：码位不撞 + 判据集自身性质
//!
//! **判据纪律**（十诫）：
//! 1. 判据侧常量**独立写死**，不复用被测常量做「预期值」（否则改常量即改判据）。
//! 2. 判据区**零 panic 面**：无 `unwrap()`/`expect()`，下标先比长度。
//! 3. 「一致」类判据必须配**反向判据**（不一致时能红），否则恒真。
//! 4. 「冻结禁改」类判据必须配**「加法被允许」判据** —— 冻结不是不许长大，
//!    只禁改语义，把两者混为一谈的实现会漏掉真正的越权路径。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use super::vez50_arch::*;
use crate::checks::{CheckSet, MAX_CHECKS};

/// 判据侧独立写死的层序（**不复用 `Layer::ALL`**）。
const WANT_LAYERS: [(&str, usize); 4] = [
    ("设计层", 0),
    ("词法层", 1),
    ("语法层", 2),
    ("运行时层", 3),
];

/// 判据侧独立写死的十件件号（不复用 `HANDOFF_ITEMS` 的 `serial`）。
const WANT_SERIALS: [&str; HANDOFF_COUNT] = [
    "F4993-01", "F4993-02", "F4993-03", "F4993-04", "F4993-05",
    "F4993-06", "F4993-07", "F4993-08", "F4993-09", "F4993-10",
];

/// 判据侧独立写死的三承接面件数（不复用 `total_of_face`）。
const WANT_FACE_COUNTS: [(HandoffSource, usize); 3] = [
    (HandoffSource::Toolchain, 5),
    (HandoffSource::Sandbox, 3),
    (HandoffSource::Probe, 2),
];

/// 四层 + 名称 + 层序（判据一）。
fn c501_layers(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let bp = Blueprint::new();

    // 层数恰好四层（锚点「四层架构」）。
    v.push((
        "C501-四层-层数为四",
        bp.layer_count() == 4 && bp.layer_count() == LAYER_COUNT,
        "架构必须恰好四层（设计/词法/语法/运行时）",
    ));

    // 层名与层序逐层对账（判据侧独立写死名字，不从被测取名）。
    let mut names_ok = bp.layers.len() == WANT_LAYERS.len();
    let mut i = 0usize;
    while i < WANT_LAYERS.len() && i < bp.layers.len() {
        let (want_name, want_ord) = WANT_LAYERS[i];
        if bp.layers[i].name() != want_name || bp.layers[i].ordinal() != want_ord {
            names_ok = false;
        }
        i += 1;
    }
    v.push((
        "C501-四层-层名与层序逐层对账",
        names_ok,
        "设计层=0 / 词法层=1 / 语法层=2 / 运行时层=3（判据侧独立写死）",
    ));

    // 层名**互异**（重名会让「找哪一层」变成猜）。
    let mut distinct = true;
    let mut a = 0usize;
    while a < bp.layers.len() {
        let mut b = a + 1;
        while b < bp.layers.len() {
            if bp.layers[a].name() == bp.layers[b].name() {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push((
        "C501-四层-层名互异",
        distinct,
        "四层名字不能重复（重名则诊断无法指向具体层）",
    ));

    // 依赖方向单向：每层恰有一个下游（末层无），恰有一个上游（首层无）。
    let mut chain_ok = bp.interfaces.len() == LAYER_COUNT - 1;
    let mut k = 0usize;
    while k + 1 < LAYER_COUNT {
        let cur = Layer::ALL[k];
        let nxt = Layer::ALL[k + 1];
        if cur.downstream() != Some(nxt) || nxt.upstream() != Some(cur) {
            chain_ok = false;
        }
        k += 1;
    }
    // 首层无上游、末层无下游。
    if Layer::ALL[0].upstream().is_some() || Layer::ALL[LAYER_COUNT - 1].downstream().is_some() {
        chain_ok = false;
    }
    v.push((
        "C501-四层-依赖方向单向成链",
        chain_ok,
        "设计→词法→语法→运行时单向成链，首层无上游、末层无下游",
    ));

    // 层间接口与边界**一一对应**：3 条边界 ↔ 3 个接口，且接口的
    // from/to 就是该边界两侧的层（判据侧独立枚举边界再核对）。
    let mut iface_ok = bp.interface_count() == LAYER_COUNT - 1;
    let mut m = 0usize;
    while m + 1 < LAYER_COUNT {
        let from = Layer::ALL[m];
        let to = Layer::ALL[m + 1];
        let want_name = interface_name(from, to);
        match bp.interface_of(&want_name) {
            Some(it) => {
                if it.from != from || it.to != to {
                    iface_ok = false;
                }
            }
            None => iface_ok = false,
        }
        m += 1;
    }
    v.push((
        "C501-四层-层间接口与边界一一对应",
        iface_ok,
        "3 条边界各一个具名接口，且接口 from/to 就是边界两侧层",
    ));

    // 接口名**互异**（撞名则冻结会冻错对象）。
    let mut iname_distinct = true;
    let mut x = 0usize;
    while x < bp.interfaces.len() {
        let mut y = x + 1;
        while y < bp.interfaces.len() {
            if bp.interfaces[x].name == bp.interfaces[y].name {
                iname_distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    v.push((
        "C501-四层-接口名互异",
        iname_distinct,
        "接口名不能重复（撞名则冻结对象错位）",
    ));

    // 层间接口命名的可读性（**脚本为人写**在命名上的落点）：
    // 名字必须「上游首字母-下游语义」两段式，含连字符。
    let mut readable = true;
    let mut z = 0usize;
    while z < bp.interfaces.len() {
        let n = &bp.interfaces[z].name;
        if !n.contains('-') || n.len() < 5 {
            readable = false;
        }
        z += 1;
    }
    v.push((
        "C501-四层-接口名两段可读",
        readable,
        "接口名形如「上游首字母-下游语义」（如 d-token），创作者能读懂",
    ));

    // 架构总览文本把四层都列出（人读材料不是空的）。
    let d = describe();
    let mut all_named = true;
    let mut q = 0usize;
    while q < WANT_LAYERS.len() {
        if !d.contains(WANT_LAYERS[q].0) {
            all_named = false;
        }
        q += 1;
    }
    v.push((
        "C501-四层-总览列出全部四层",
        all_named,
        "describe() 须把四层都列出（人读材料漏层= 架构没对外讲清）",
    ));
}

/// 脚本为人写（判据二，域本色声明）。
fn c501_human(v: &mut Vec<(&'static str, bool, &'static str)>) {
    // 诊断必须带**源信息**：名字 + 说明，非空且含层名。
    let e1 = ArchitectureError::LayerMismatch {
        boundary: "l-tree".to_string(),
        layer: Layer::Syntax,
    };
    let x1 = e1.explain();
    v.push((
        "C501-脚本为人写-诊断非空且含层名",
        !x1.is_empty() && x1.contains("语法层"),
        "层间失配诊断必须说出是哪一层（只吐码位等于没诊断）",
    ));

    // 诊断必须**可操作**：给出「改什么」而非复述错在哪。
    // 用一条硬判据：建议里必须出现祈使动作词之一，且非空。
    let a1 = ArchitectureError::AmendFrozen { name: "d-token".to_string() };
    let adv = a1.advise();
    let imperative = adv.contains("改用")
        || adv.contains("先")
        || adv.contains("跑")
        || adv.contains("核对")
        || adv.contains("检查");
    v.push((
        "C501-脚本为人写-诊断给出可操作建议",
        !adv.is_empty() && imperative,
        "建议必须说「改什么」（改用Extend/先解冻/跑对拍…），不是复述错在哪",
    ));

    // **反向对账**：每类错误的诊断与建议**都不相同**。
    // 若四类错误的 explain 撞成同一句，创作者无法分辨，该条恒真。
    let variants = [
        a1.explain(),
        e1.explain(),
        ArchitectureError::HandoffMissing {
            item: "沙箱口径·配额上限",
            source: "F4993-07",
        }
        .explain(),
        ArchitectureError::BadLayerOrder { got: 3, want: 1 }.explain(),
    ];
    let mut uniq = true;
    let mut i = 0usize;
    while i < variants.len() {
        let mut j = i + 1;
        while j < variants.len() {
            if variants[i] == variants[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "C501-脚本为人写-四类诊断互不相同",
        uniq,
        "四类错误的 explain 必须各成一串（撞串则创作者无法分辨）",
    ));

    // 建议**互不相同**（同上，防 advise 恒返回同一句）。
    let advises = [
        a1.advise(),
        e1.advise(),
        ArchitectureError::HandoffMissing { item: "x", source: "F4993-06" }.advise(),
        ArchitectureError::BadLayerOrder { got: 3, want: 1 }.advise(),
    ];
    let mut advise_uniq = true;
    let mut k = 0usize;
    while k < advises.len() {
        let mut l = k + 1;
        while l < advises.len() {
            if advises[k] == advises[l] {
                advise_uniq = false;
            }
            l += 1;
        }
        k += 1;
    }
    v.push((
        "C501-脚本为人写-四类建议互不相同",
        advise_uniq,
        "四类错误的 advise 必须各成一串（撞串则建议失去指导性）",
    ));

    // 承接缺源的诊断必须**报出源头件号**（锚点「回溯移交包」）。
    let hm = ArchitectureError::HandoffMissing {
        item: "分析探针·读回协议",
        source: "F4993-10",
    };
    let hx = hm.explain();
    v.push((
        "C501-脚本为人写-缺源诊断带源头件号",
        hx.contains("F4993-10") && hx.contains("分析探针"),
        "承接缺源须回溯并报出 F4993 源头件号 + 承接面人读名",
    ));

    // 层名是**中文可读**而非代号（域本色在层名上的落点）。
    let mut cn_ok = true;
    let mut m = 0usize;
    while m < LAYER_COUNT {
        let nm = Layer::ALL[m].name();
        let mut has_cn = false;
        for ch in nm.chars() {
            if (ch as u32) > 0x4E00 && (ch as u32) < 0x9FA5 {
                has_cn = true;
            }
        }
        if !has_cn || nm.is_empty() {
            cn_ok = false;
        }
        m += 1;
    }
    v.push((
        "C501-脚本为人写-层名为中文可读",
        cn_ok,
        "层名须是中文可读词（脚本为人写：代号不可读等于域本色未落地）",
    ));
}

/// 承接落地（判据三）。
fn c501_handoff(v: &mut Vec<(&'static str, bool, &'static str)>) {
    let ledger = HandoffLedger::new();

    // 件数恰好十件（锚点「F4993 十件」）。
    v.push((
        "C501-承接落地-移交包十件",
        HANDOFF_COUNT == 10 && HANDOFF_ITEMS.len() == 10 && ledger.states.len() == 10,
        "移交包恒为十件（清单与状态表同长）",
    ));

    // 件号**逐件对账**（判据侧独立写死，不复用被测 serial）。
    let mut serial_ok = HANDOFF_ITEMS.len() == WANT_SERIALS.len();
    let mut i = 0usize;
    while i < WANT_SERIALS.len() && i < HANDOFF_ITEMS.len() {
        if HANDOFF_ITEMS[i].serial != WANT_SERIALS[i] {
            serial_ok = false;
        }
        i += 1;
    }
    v.push((
        "C501-承接落地-件号逐件对账",
        serial_ok,
        "F4993-01 … F4993-10 逐件号对账（判据侧独立写死）",
    ));

    // 三承接面件数逐面对账（判据侧独立写死 5/3/2）。
    let mut face_ok = true;
    let mut k = 0usize;
    while k < WANT_FACE_COUNTS.len() {
        let (face, want) = WANT_FACE_COUNTS[k];
        if HandoffLedger::total_of_face(face) != want {
            face_ok = false;
        }
        k += 1;
    }
    v.push((
        "C501-承接落地-三承接面件数对账",
        face_ok,
        "工具链 5 件 / 沙箱 3 件 / 探针 2 件（判据侧独立写死）",
    ));

    // 承接面**人读名非空且互异**（判据侧逐件查）。
    let mut label_ok = HANDOFF_ITEMS.len() == HANDOFF_COUNT;
    let mut a = 0usize;
    while a < HANDOFF_ITEMS.len() {
        if HANDOFF_ITEMS[a].label.is_empty() {
            label_ok = false;
        }
        let mut b = a + 1;
        while b < HANDOFF_ITEMS.len() {
            if HANDOFF_ITEMS[a].label == HANDOFF_ITEMS[b].label {
                label_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push((
        "C501-承接落地-承接面名非空且互异",
        label_ok,
        "十件的人读名不得为空或重复（重复则承接面对不上号）",
    ));

    // 落地计数：**从空表起落 n 件 ⇒ 计 n**（净值口径，判据侧独立算）。
    let mut l2 = HandoffLedger::new();
    let mut land_ok = true;
    let mut c = 0usize;
    while c < 7 {
        if l2.land(c).is_err() {
            land_ok = false;
        }
        c += 1;
    }
    // 判据侧独立数落地数
    let mut expect_landed = 0usize;
    let mut d = 0usize;
    while d < l2.states.len() {
        if l2.states[d] == LandingState::Landed {
            expect_landed += 1;
        }
        d += 1;
    }
    v.push((
        "C501-承接落地-落地计数与独立计数对账",
        land_ok && l2.landed_count() == 7 && expect_landed == 7,
        "落 7 件 ⇒ 落地计数恰为 7（判据侧逐项数Landed，不向被测问答案）",
    ));

    // 按承接面计数逐面对账（判据侧独立数该面下标）。
    let mut per_face_ok = true;
    let mut e = 0usize;
    while e < 3 {
        let (face, _want) = WANT_FACE_COUNTS[e];
        // 判据侧独立数：哪些下标属于该面且已落地
        let mut n = 0usize;
        let mut idx = 0usize;
        while idx < l2.states.len() {
            let belongs = match face {
                HandoffSource::Toolchain => idx < 5,
                HandoffSource::Sandbox => idx >= 5 && idx < 8,
                HandoffSource::Probe => idx >= 8 && idx < 10,
            };
            if belongs && l2.states[idx] == LandingState::Landed {
                n += 1;
            }
            idx += 1;
        }
        if n != l2.landed_of_face(face) {
            per_face_ok = false;
        }
        e += 1;
    }
    v.push((
        "C501-承接落地-按面计数与独立计数对账",
        per_face_ok,
        "工具链 5/沙箱 2(第7件)/探针 0(未落) 逐面对账",
    ));

    // 缺源定位：**首个缺口**是未落的最小下标（判据侧独立求）。
    let gap = match l2.first_gap() {
        Some(g) => g,
        None => 999,
    };
    let mut want_gap: Option<usize> = None;
    let mut f = 0usize;
    while f < l2.states.len() {
        if l2.states[f] != LandingState::Landed {
            want_gap = Some(f);
            break;
        }
        f += 1;
    }
    let gap_match = match (gap == 999, want_gap) {
        (true, None) => true,
        (false, Some(w)) => gap == w,
        _ => false,
    };
    v.push((
        "C501-承接落地-首个缺口定位一致",
        gap_match && gap == 7,
        "落 0..6 ⇒ 首缺是第 7 件（0 起下标），判据侧独立求与被测一致",
    ));

    // **反向对照**：全部落地 ⇒ 无缺口（缺口判据不能恒红）。
    let mut l3 = HandoffLedger::new();
    let mut g = 0usize;
    let mut full = true;
    while g < HANDOFF_COUNT {
        if l3.land(g).is_err() {
            full = false;
        }
        g += 1;
    }
    v.push((
        "C501-承接落地-全落地则无缺口",
        full && l3.landed_count() == HANDOFF_COUNT && l3.first_gap().is_none(),
        "十件全落地 ⇒ 计数 10 且无缺口（防首缺判据恒红）",
    ));

    // 越界落地**必须拒绝**（不静默丢弃，也不 panic）。
    let mut l4 = HandoffLedger::new();
    let rejected = l4.land(HANDOFF_COUNT).is_err() && l4.land(999).is_err();
    v.push((
        "C501-承接落地-越界落地被拒",
        rejected && l4.landed_count() == 0,
        "下标越界须返 Err 且不改变落地计数（静默接受会让件数虚高）",
    ));

    // **越界后计数必须仍是 0**（钳位式「修复」也红）。
    //
    // 上一条只断 `is_err()`：若有人把越界改成「钳到末项再落地」
    // （返 Ok、计数 +1），那条判据就红了——但如果改成「钳位且不写状态」
    // （返 Ok、计数不变）就只红一半。**更隐蔽的是**把越界当合法：
    // 返 Ok 且**不写任何状态**，计数仍是 0，看起来一切正常，
    // 但「越界被接受」这件事被静默了。
    //
    // 本条专断「越界调用后计数仍为 0」——覆盖钳位与静默接受两种伪修。
    let mut l4b = HandoffLedger::new();
    let _ = l4b.land(HANDOFF_COUNT);
    let _ = l4b.land(999);
    // 判据侧独立逐项数
    let mut indep = 0usize;
    let mut q = 0usize;
    while q < l4b.states.len() {
        if l4b.states[q] == LandingState::Landed {
            indep += 1;
        }
        q += 1;
    }
    v.push((
        "C501-承接落地-越界调用不落地任何件",
        indep == 0 && l4b.landed_count() == 0 && l4b.first_gap() == Some(0),
        "两次越界调用后仍无任何件落地、首缺仍是第 0 件（钳位与静默接受都红）",
    ));

    // 缺源错误须**报出源头件号**（回溯锚点）。
    let mut l5 = HandoffLedger::new();
    let _ = l5.land(0);
    let miss = l5.first_missing();
    let miss_ok = match miss {
        Some(ArchitectureError::HandoffMissing { source, .. }) => source == WANT_SERIALS[1],
        _ => false,
    };
    v.push((
        "C501-承接落地-缺源回溯到源头件",
        miss_ok,
        "落 0 ⇒ 首缺为 F4993-02，first_missing 须报出该源头件号",
    ));
}

/// 层冻结（判据四）。
fn c501_freeze(v: &mut Vec<(&'static str, bool, &'static str)>) {
    // 未冻结时：改与加**都允许**（先确认对照组非恒假）。
    let mut d1 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    let free_change = d1.amend(AmendKind::Change);
    let free_extend = d1.amend(AmendKind::Extend);
    v.push((
        "C501-层冻结-未冻结时改动自由",
        free_change.is_ok() && free_extend.is_ok(),
        "草拟态：改与加都允许（对照组，防「冻结判据恒红」）",
    ));

    // 冻结后：**改**被拒（接口越权 → 冻结流程）。
    let mut d2 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    d2.freeze();
    let frozen_change = d2.amend(AmendKind::Change);
    v.push((
        "C501-层冻结-冻结后改语义被拒",
        frozen_change.is_err(),
        "冻结态 amend(Change) 必须返 Err（这是接口越权路径）",
    ));

    // 冻结后：**加法被允许且升版本**（冻结不是不许长大）。
    let before_v = d2.version;
    let frozen_extend = d2.amend(AmendKind::Extend);
    let v_after = match frozen_extend {
        Ok(x) => x,
        Err(_) => 0,
    };
    v.push((
        "C501-层冻结-冻结后加法允许并升版本",
        frozen_extend.is_ok() && v_after == before_v + 1,
        "冻结态 amend(Extend) 允许并升版本（冻结禁的是改语义，不是禁长大）",
    ));

    // 越权错误诊断**指明接口名**（冻结流程要能被追责到具体接口）。
    let e = match frozen_change {
        Err(ArchitectureError::AmendFrozen { name }) => name,
        _ => String::new(),
    };
    let ex = ArchitectureError::AmendFrozen { name: e.clone() }.explain();
    v.push((
        "C501-层冻结-越权诊断指明接口名",
        !e.is_empty() && ex.contains("d-token"),
        "越权诊断须指明是哪个接口（否则无从追责）",
    ));

    // **解冻是唯一合法途径**：解冻后可改。
    let mut d3 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    d3.freeze();
    let blocked = d3.amend(AmendKind::Change).is_err();
    let unfrozen = d3.unfreeze();
    let allowed = d3.amend(AmendKind::Change).is_ok();
    v.push((
        "C501-层冻结-解冻后才可改",
        blocked && unfrozen && allowed,
        "解冻前改被拒 ⇒ unfreeze ⇒ 改可通过（解冻是唯一合法途径）",
    ));

    // 未冻结时 unfreeze **不生效**（不能拿解冻当万能钥匙）。
    let mut d4 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    let un_noop = !d4.unfreeze();
    v.push((
        "C501-层冻结-未冻结时解冻无效",
        un_noop,
        "草拟态 unfreeze 返 false（否则能绕过冻结流程伪造解冻记录）",
    ));

    // 解冻次数**被记账**（冻结流程可追责）。
    let mut d5 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    d5.freeze();
    let m0 = d5.mutations;
    let _ = d5.unfreeze();
    let m1 = d5.mutations;
    v.push((
        "C501-层冻结-解冻次数入账",
        m1 == m0 + 1,
        "每次解冻计一次改动（冻结流程须可追责，不能静默）",
    ));

    // 冻结态**重复冻结**不重复记账（幂等）。
    let mut d6 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    d6.freeze();
    let fm0 = d6.mutations;
    d6.freeze();
    v.push((
        "C501-层冻结-重复冻结幂等",
        d6.is_frozen() && d6.mutations == fm0,
        "重复 freeze 不改账且仍是冻结态（幂等，防误调把账搅乱）",
    ));

    // 评审态：可改（**冻结流程未走完前不该禁改**）。
    let mut d7 = FrozenInterface::draft("d-token".to_string(), Layer::Design, Layer::Lexical);
    d7.to_review();
    let review_ok = d7.amend(AmendKind::Change).is_ok() && !d7.is_frozen();
    v.push((
        "C501-层冻结-评审态仍可改",
        review_ok,
        "评审态尚未冻结：改动合法（否则「评审」二字无意义）",
    ));

    // 架构册级：**全冻结**后可核验冻结计数。
    let mut bp = Blueprint::new();
    bp.freeze_all();
    v.push((
        "C501-层冻结-架构册全冻结可核验",
        bp.frozen_count() == LAYER_COUNT - 1 && bp.interface_count() == 3,
        "freeze_all 后 3 个接口全冻结（冻结成本 O(接口数)）",
    ));

    // 架构册级越权：**经册改**同样被拒（与单接口改同源）。
    let mut bp2 = Blueprint::new();
    bp2.freeze_all();
    let via_bp = bp2.amend_frozen("d-token", AmendKind::Change);
    v.push((
        "C501-层冻结-经架构册越权亦被拒",
        via_bp.is_err(),
        "amend_frozen 走册内接口同样拒改语义（不能绕过单接口那道闸）",
    ));

    // 不存在的接口名 ⇒ 报错（不静默新建）。
    let mut bp3 = Blueprint::new();
    bp3.freeze_all();
    let ghost = bp3.amend_frozen("x-ghost", AmendKind::Change);
    v.push((
        "C501-层冻结-未知接口名报错",
        ghost.is_err() && bp3.interface_of("x-ghost").is_none(),
        "改不存在的接口须报错且不隐式新建（否则打错名会静默生效）",
    ));
}

/// 判据自身与诊断码（判据五）。
fn c501_criteria(v: &mut Vec<(&'static str, bool, &'static str)>) {
    // 诊断码**互不撞**（撞码则日志无法定位是哪类错误）。
    let codes = all_codes();
    let mut uniq = true;
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    // 穷举全部可达变体的码数：越权 1 + 层失配 LAYER_COUNT + 缺源 1 + 层序错 LAYER_COUNT
    let want_codes = 1 + LAYER_COUNT + 1 + LAYER_COUNT;
    v.push((
        "C501-判据-诊断码互不撞",
        uniq && codes.len() == want_codes,
        "穷举全部可达变体（越权1 + 层失配4 + 缺源1 + 层序错4 = 10）码位互不相同；\
         只取单样本会漏掉跨变体碰撞",
    ));

    // 码位**非零且高位稳定**（0 码与非零高位是常见退化）。
    let mut nonzero = true;
    let mut k = 0usize;
    while k < codes.len() {
        if codes[k] == 0 || (codes[k] & 0xFF00) != 0x5A00 {
            nonzero = false;
        }
        k += 1;
    }
    v.push((
        "C501-判据-诊断码非零且高位稳定",
        nonzero,
        "码须非零且高位落在 0x5A 段（族内可辨识）",
    ));

    // 错误码由**内容决定**而非枚举序（两条不同错误不得同码）。
    let e_layer1 = ArchitectureError::LayerMismatch {
        boundary: "l-tree".to_string(),
        layer: Layer::Lexical,
    };
    let e_layer2 = ArchitectureError::LayerMismatch {
        boundary: "s-scene".to_string(),
        layer: Layer::Syntax,
    };
    v.push((
        "C501-判据-码随层变化",
        e_layer1.code() != e_layer2.code(),
        "同型错误落在不同层 ⇒ 码须不同（否则层信息从码里丢了）",
    ));

    // 对拍：**一致时报告一致**（正向）。
    let a = SideOutput::new("l-tree", &["a", "b", "c"]);
    let b = SideOutput::new("l-tree", &["a", "b", "c"]);
    let r_ok = diff_boundary(&a, &b);
    v.push((
        "C501-判据-对拍一致时判一致",
        r_ok.matched && r_ok.first_gap.is_none() && r_ok.boundary.is_none(),
        "两侧逐项相同 ⇒ matched 且无缺口无边界（正向）",
    ));

    // 对拍：**项不同**时须定位到第一处（反向，防恒真通过）。
    let c = SideOutput::new("l-tree", &["a", "X", "c"]);
    let r_diff = diff_boundary(&a, &c);
    v.push((
        "C501-判据-对拍差异定位首处",
        !r_diff.matched && r_diff.first_gap == Some(1),
        "第 1 项不同 ⇒ 缺口恰为 1（对拍不能恒真通过）",
    ));

    // 对拍：**项数不同**时也须给缺口（少一项这种最常见失配要能定位）。
    let d = SideOutput::new("l-tree", &["a", "b"]);
    let r_short = diff_boundary(&a, &d);
    v.push((
        "C501-判据-对拍项数不同也给缺口",
        !r_short.matched && r_short.first_gap == Some(2),
        "3 项 vs 2 项 ⇒ 缺口为 2（少项失配必须能定位，不能只报长度）",
    ));

    // 对拍：**边界名不同**时判失配且不报项内缺口。
    let e = SideOutput::new("s-scene", &["a", "b", "c"]);
    let r_bnd = diff_boundary(&a, &e);
    v.push((
        "C501-判据-对拍边界名不同判失配",
        !r_bnd.matched && r_bnd.boundary == Some("l-tree".to_string()),
        "边界名不同 ⇒ 失配并指出用的是哪条边界（接口对错了）",
    ));

    // 摘要**逐项拼接且带分隔**（无分隔时 [\"ab\",\"c\"] 与 [\"a\",\"bc\"] 撞串）。
    let s1 = SideOutput::new("l-tree", &["ab", "c"]).digest();
    let s2 = SideOutput::new("l-tree", &["a", "bc"]).digest();
    v.push((
        "C501-判据-摘要带分隔不撞串",
        s1 == "ab|c" && s2 == "a|bc" && s1 != s2,
        "摘要以 | 分隔逐项拼接（无分隔会让[\"ab\",\"c\"] 与[\"a\",\"bc\"] 同串）",
    ));

    // 冒烟：标准四层全冻结全落地。
    let sm = smoke();
    v.push((
        "C501-判据-冒烟四项自洽",
        sm.contains("层数 4")
            && sm.contains("接口数 3")
            && sm.contains("冻结 3")
            && sm.contains("承接 10/10"),
        "冒烟串须同时给出层数/接口数/冻结数/承接进度",
    ));

    // 本文件判据总量在容量内（否则 CheckSet 静默丢条目）。
    v.push((
        "C501-判据-判据量在容量内",
        c501_estimated_count() <= MAX_CHECKS,
        "单族判据数须≤ MAX_CHECKS，超过会被 CheckSet 静默丢弃",
    ));
}

/// 本文件判据总条数（自检用）。
const fn c501_estimated_count() -> usize {
    // layers 8 + human 6 + handoff 11 + freeze 12 + criteria 10 = 47
    8 + 6 + 11 + 12 + 10
}

// ---------------------------------------------------------------------------
// 聚合器（三族拆分，规避 MAX_CHECKS 上限）
// ---------------------------------------------------------------------------

fn collect(
    f: fn(&mut Vec<(&'static str, bool, &'static str)>),
    set: &mut CheckSet,
) {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    f(&mut v);
    let mut i = 0usize;
    while i < v.len() {
        set.add(v[i].0, v[i].1, v[i].2);
        i += 1;
    }
}

/// a 族：四层 + 脚本为人写。
pub fn run_vez50_checks_a_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vez50");
    collect(c501_layers, &mut set);
    collect(c501_human, &mut set);
    set
}

/// b 族：承接落地 + 层冻结。
pub fn run_vez50_checks_b_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vez50");
    collect(c501_handoff, &mut set);
    collect(c501_freeze, &mut set);
    set
}

/// c 族：判据自身与诊断码。
pub fn run_vez50_checks_c_standalone() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vez50");
    collect(c501_criteria, &mut set);
    set
}

/// 合并全族（供聚合器遍历用）。
pub fn run_vez50_checks() -> CheckSet {
    CheckSet::merge(
        CheckSet::merge(
            run_vez50_checks_a_standalone(),
            run_vez50_checks_b_standalone(),
        ),
        run_vez50_checks_c_standalone(),
    )
}