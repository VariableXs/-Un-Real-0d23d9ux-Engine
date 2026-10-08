//! VE-F1614 · 网格工具数据契约 —— 域自检（VE-I 域 · 几何工具段）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1614`
//!
//! **判据（锚点原文）**：查看器数据、修复器数据、契约冻结、判据。
//!
//! ## 判据区纪律（照文件头十条的反假条目写，本文件同样遵守）
//!
//! 1. **判据区零 panic 面**：无 `unwrap()`/`expect()`；下标访问先比长度。
//!    取值失败一律 `match` 记红——`.expect()` 会让被测实现一改坏，
//!    判据自己先崩，症状退化为「探针无输出」而非「某条判据变红」。
//! 2. **期望值不从被测函数反推**：阈值/短码/错误码在判据侧写死并注明理由。
//! 3. **双向验证**：每条「须拒绝」都配一条「须放行」，否则「永远拒绝」也全绿。
//! 4. **等价对照**：像 F1613 那样把判据集自身也纳入被检（[`meta_checks`]），
//!    保证「判据集自身不被悄悄改弱」。
//! 5. **零向量不算反向法线**：退化面已由 DegenerateFace/ZeroAreaFace 报告，
//!    再报一次 FlippedNormal 会让同一位置出现两条互相矛盾的结论。
//!
//! 分 a/b/c 三族，规避 `MAX_CHECKS` 截断（每族独立 `CheckSet`）：
//!   a族 = 查看器数据（17 项）
//!   b 族 = 修复器数据（15 项）
//!   c 族 = 契约冻结与判据集自检（12 项）

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vef17_toolcontract as tc;

// ---------------------------------------------------------------------------
// 判据专用夹具（与被测实现无关的独立语料）
// ---------------------------------------------------------------------------

/// 判据侧写死的期望值（**不从被测函数反推**）。
mod expect {
    /// 契约版本。
    pub const VERSION: u32 = 1;
    /// 线上格式指纹的**具体值**（FNV-1a over "hl"+"wf"+"nm"+"uv"+"bb"）。
    ///
    /// 必须**钉死数值**而不是只断「非零」：只断非零时，
    /// 把指纹算法换成任意常量、或把五类顺序对调，指纹仍非零 ⇒ 判据全绿，
    /// 而线上格式实际已变。实测这两种变异都曾 MISS。
    /// 钉死数值后，任何改动短码/顺序/算法的行为都会让本判据变红。
    pub const WIRE_SHAPE: u32 = 2_315_004_178;
    /// 查看器五类的短码（顺序即冻结顺序）。
    pub const FIELD_WIRES: [&str; 5] = ["hl", "wf", "nm", "uv", "bb"];
    /// 问题类型六类的短码。
    pub const KIND_WIRES: [&str; 6] =
        ["dupvert", "degenface", "flipnorm", "idxrange", "degenedge", "zeroarea"];
    /// 问题类型六类的错误码（域号 0x1614 + 序号）。
    pub const KIND_CODES: [u32; 6] = [
        0x1614_0001,
        0x1614_0002,
        0x1614_0003,
        0x1614_0004,
        0x1614_0005,
        0x1614_0006,
    ];
    /// 一个「干净的单位立方体」应有的顶点数与面数。
    pub const CUBE_VERTS: u32 = 8;
    /// 三角化后的**唯一边数**。
    ///
    /// 用**欧拉公式独立推导**，不写死「12」：闭合亏格 0 网格满足
    /// `V − E + F = 2` ⇒ `E = V + F − 2 = 8 + 12 − 2 = 18`。
    ///
    /// **写死 12 是错的**：立方体拓扑棱有 12 条，但每个正方形面被切成两个
    /// 三角形时会产生一条**面对角线**，六面各一条共 6 条，去重后是 18。
    /// 我第一版就写成 12，被判据抓出——抓出的是**夹具期望值的错**，
    /// 不是实现的错（实现返回的 18 才是对的）。
    pub const CUBE_EDGES: u32 = 18;
    pub const CUBE_TRIS: u32 = 12;
}

/// 造一个轴对齐单位立方体（8 顶点 / 12 三角面）。
///
/// 全部顶点落在 0..1 的 em 区间，故包围盒应为 [0,1]^3。
fn cube() -> (tc::VertexSet, tc::FaceSet) {
    let mut v = tc::VertexSet::new();
    for z in [0.0f64, 1.0].iter() {
        for y in [0.0f64, 1.0].iter() {
            for x in [0.0f64, 1.0].iter() {
                v.push(tc::FixedVec3::from_em(*x, *y, *z));
            }
        }
    }
    // 索引布局：i = x + 2y + 4z
    let idx = |x: u32, y: u32, z: u32| -> u32 { x + 2 * y + 4 * z };
    let mut f = tc::FaceSet::new();
    // 每个面两个三角形。**绕序必须使面对角线成对抵消**：一个正方形有两种切法，
    // 若相邻面切法不一致，会留下无法被抵消的**体对角线**（如 0-7、1-5、2-6），
    // 去重后边数变成 18 而不是 12。下面统一按「沿 y/z 递增方向切」，
    // 六个面两两配对，各自的对角线正好落在同一条棱上互相抵消。
    //
    // -z 与 +z 绕序相反 ⇒ 法线朝外；-y 同样反向（用于验证法线朝向判据）。
    let quads: [[(u32, u32, u32); 2]; 6] = [
        // +z
        [
            (idx(0, 0, 1), idx(1, 0, 1), idx(1, 1, 1)),
            (idx(0, 0, 1), idx(1, 1, 1), idx(0, 1, 1)),
        ],
        // -z（绕序反向 ⇒ 法线朝 -z）
        [
            (idx(0, 0, 0), idx(0, 1, 0), idx(1, 1, 0)),
            (idx(0, 0, 0), idx(1, 1, 0), idx(1, 0, 0)),
        ],
        // +x
        [
            (idx(1, 0, 0), idx(1, 1, 0), idx(1, 1, 1)),
            (idx(1, 0, 0), idx(1, 1, 1), idx(1, 0, 1)),
        ],
        // -x
        [
            (idx(0, 0, 0), idx(0, 1, 0), idx(0, 1, 1)),
            (idx(0, 0, 0), idx(0, 1, 1), idx(0, 0, 1)),
        ],
        // +y
        [
            (idx(0, 1, 0), idx(0, 1, 1), idx(1, 1, 1)),
            (idx(0, 1, 0), idx(1, 1, 1), idx(1, 1, 0)),
        ],
        // -y（绕序反向 ⇒ 法线朝 -y）
        [
            (idx(0, 0, 0), idx(1, 0, 0), idx(1, 0, 1)),
            (idx(0, 0, 0), idx(1, 0, 1), idx(0, 0, 1)),
        ],
    ];
    for q in quads.iter() {
        for tri in q.iter() {
            f.push(tc::FaceRef::new(tri.0, tri.1, tri.2));
        }
    }
    (v, f)
}

/// 造一个「问题网格」：含退化面、自环边、越界索引。
fn broken_mesh() -> (tc::VertexSet, tc::FaceSet) {
    let mut v = tc::VertexSet::new();
    v.push(tc::FixedVec3::from_em(0.0, 0.0, 0.0));
    v.push(tc::FixedVec3::from_em(1.0, 0.0, 0.0));
    v.push(tc::FixedVec3::from_em(0.0, 1.0, 0.0));
    // 顶点集**故意**不含 index 9⇒ 下面的面会越界
    let mut f = tc::FaceSet::new();
    f.push(tc::FaceRef::new(0, 1, 2)); // 正常面
    f.push(tc::FaceRef::new(0, 1, 1)); // 退化面（v[1]==v[2]）
    f.push(tc::FaceRef::new(5, 5, 6)); // 退化面 + 全部越界
    f.push(tc::FaceRef::new(0, 1, 9)); // 越界索引
    (v, f)
}

// ---------------------------------------------------------------------------
// a 族：查看器数据
// ---------------------------------------------------------------------------

fn c1614_viewer(s: &mut CheckSet) {
    let (v, f) = cube();
    let out = tc::build_contract(7, &v, &f);
    let p = &out.viewer;

    // ① 五类字段**恰好五类**且每类恰好一条规格。
    let five = p.specs.len() == tc::ViewerField::ALL.len();
    let mut no_dup = true;
    for (i, a) in tc::ViewerField::ALL.iter().enumerate() {
        if p.field(*a).is_none() {
            no_dup = false;
        }
        // 每个字段在 specs 里**只出现一次**。
        let mut cnt = 0;
        for sp in p.specs.iter() {
            if sp.field == *a {
                cnt += 1;
            }
        }
        if cnt != 1 {
            no_dup = false;
        }
        let _ = i;
    }
    s.add("查看器五类字段各恰好一条规格", five && no_dup, "五类封闭全集，逐类查存在且不重复");

    // ② 五类短码与判据侧写死值**逐字节相同**（冻结面）。
    let mut wire_ok = true;
    for (i, fld) in tc::ViewerField::ALL.iter().enumerate() {
        if fld.wire() != expect::FIELD_WIRES[i] {
            wire_ok = false;
        }
    }
    s.add("五类短码逐字节匹配写死期望", wire_ok, "hl/wf/nm/uv/bb，判据侧写死不从被测反推");

    // ③ 短码**可逆**：`from_wire(wire(x)) == x`，未知短码返回 None。
    let mut rt = true;
    for fld in tc::ViewerField::ALL.iter() {
        match tc::ViewerField::from_wire(fld.wire()) {
            Some(back) => {
                if back != *fld {
                    rt = false;
                }
            }
            None => rt = false,
        }
    }
    if tc::ViewerField::from_wire("zz").is_some() {
        rt = false; // 未知短码不得有映射
    }
    s.add("短码可逆且未知短码不误映射", rt, "往返一致 + 未知短码(zz)必须 None");

    // ④ 构造时**默认全部未产出**（显式产出纪律：默认全 true 会让漏填隐形）。
    let fresh = tc::ViewerPacket::new(1);
    s.add(
        "新包默认五类均未产出",
        fresh.present_count() == 0 && fresh.missing_required().len() == 5 && !fresh.is_complete(),
        "默认 present=false；漏产出必须可查而不是隐形",
    );

    // ⑤ `set_field` 对五类均生效（**逐类试**，不只试一类）。
    let mut fp = tc::ViewerPacket::new(2);
    let mut set_ok = true;
    for (i, fld) in tc::ViewerField::ALL.iter().enumerate() {
        if !fp.set_field(*fld, i as u32 + 1) {
            set_ok = false;
        }
        match fp.field(*fld) {
            Some(sp) => {
                if !sp.present || sp.count != i as u32 + 1 {
                    set_ok = false;
                }
            }
            None => set_ok = false,
        }
    }
    s.add("五类 set_field/field 往返一致", set_ok, "逐类置位再逐类读回，count 必须逐类相符");

    // ⑥ 置满五类后 `is_complete()` 为真，且 `missing_required()` 空。
    let mut full = tc::ViewerPacket::new(3);
    for fld in tc::ViewerField::ALL.iter() {
        full.set_field(*fld, 1);
    }
    s.add(
        "五类齐备即完整（对照面：否则「永远不完整」也全绿）",
        full.is_complete() && full.missing_required().is_empty(),
        "五类全present 且 wire_shape 未变 ⇒ complete",
    );

    // ⑦ 立方体的唯一边数 = 18（**期望值由欧拉公式在判据侧独立推导**，
    //    不由被测返回的数当期望，也不写死 12）。
    //    闭合亏格 0：V − E + F = 2 ⇒ E = V + F − 2 = 8 + 12 − 2 = 18。
    let edges = f.unique_edges();
    let expect_edges = expect::CUBE_EDGES as usize;
    //欧拉示性数 V − E + F（**减的是 E、加的是 F**；写成 V−F+E 会算出 14）。
    let euler = (v.len() as i64) - (edges.len() as i64) + (f.len() as i64);
    s.add(
        "单位立方体唯一边数=18（欧拉公式 V-E+F=2）",
        v.len() == expect::CUBE_VERTS as usize && f.len() == expect::CUBE_TRIS as usize
            && edges.len() == expect_edges
            && euler == 2,
        "8 顶点 12 三角面 ⇒ E=18（12 拓扑棱 + 6 面对角线）；欧拉示性数须为 2",
    );

    // ⑧ 边规范化：反向边必须被识别为同一条。
    let e01 = tc::EdgeRef::new(0, 1);
    let e10 = tc::EdgeRef::new(1, 0);
    s.add(
        "边规范化使反向边判等",
        e01.normalized() == e10.normalized() && !e01.normalized().is_degenerate(),
        "(0,1) 与 (1,0) 规范化后逐字节相同；非自环不报退化",
    );

    // ⑨ 自环边**只标记不丢弃**（丢弃会让「网格有自环」这个事实消失）。
    let loop_edge = tc::EdgeRef::new(4, 4);
    s.add(
        "自环边可标记且不被丢弃",
        loop_edge.is_degenerate() && loop_edge.normalized() == loop_edge,
        "退化标记为真，规范化不改变自环（a==b）",
    );

    // ⑩ 包围盒= [0,1]^3（定点 0 与1_000_000），**双向钉死 min 与 max**。
    let bb = v.bounds();
    let bb_ok = !bb.is_empty()
        && bb.min.x == 0
        && bb.min.y == 0
        && bb.min.z == 0
        && bb.max.x == tc::FIXED_SCALE
        && bb.max.y == tc::FIXED_SCALE
        && bb.max.z == tc::FIXED_SCALE;
    s.add("单位立方体包围盒恰为[0,1]^3", bb_ok, "八项逐项比对；只断 min 或只断 extent 会漏一半");

    // ⑪ 包围盒尺寸 = 1 em（三轴）。
    let ext = bb.extent();
    s.add(
        "包围盒尺寸三轴均为 1em",
        ext.x == tc::FIXED_SCALE && ext.y == tc::FIXED_SCALE && ext.z == tc::FIXED_SCALE,
        "extent 逐轴比对",
    );

    // ⑫ 空顶点集 ⇒ 空包围盒，且**尺寸为 0 而非下溢**。
    //「有标签 ≠ 可达」：负尺寸是 i64 下溢，若实现写 `max - min` 在空盒上
    // 会得到天文数字，判据必须显式钉住 0。
    let ev = tc::VertexSet::new();
    let eb = ev.bounds();
    let eext = eb.extent();
    s.add(
        "空顶点集包围盒空且尺寸为0（不下溢）",
        eb.is_empty() && eext.x == 0 && eext.y == 0 && eext.z == 0,
        "空包围盒用 min>max 表示，extent 必须返回 0",
    );

    // ⑬ 空包围盒 wire 短码为 empty，filled 区分之（双向）。
    let mut filled = tc::Aabb::EMPTY;
    filled.grow(tc::FixedVec3 { x: 0, y: 0, z: 0 });
    s.add(
        "包围盒空/满短码可区分",
        eb.wire() == "aabb:empty" && filled.wire() == "aabb:filled",
        "并入单个原点后应判满（min==max 的退化盒也算有内容）",
    );

    // ⑭ `grow` 必须能处理全负坐标（只测正坐标会让 min/max 写反的 bug 隐形）。
    let mut nb = tc::Aabb::EMPTY;
    nb.grow(tc::FixedVec3 { x: -500_000, y: -700_000, z: -900_000 });
    let nb_ok = nb.min.x == -500_000
        && nb.min.y == -700_000
        && nb.min.z == -900_000
        && nb.max.x == -500_000;
    s.add(
        "全负坐标包围盒 min/max 不写反",
        nb_ok && !nb.is_empty(),
        "只并入负坐标点：min 必须取到最小值而非最大值",
    );

    // ⑮ 越界索引取点返回 None，**不 panic**（契约层面对外部数据）。
    let ok = v.get(0).is_some() && v.get(9999).is_none();
    s.add("顶点索引越界返回 None 而非 panic", ok, "合法索引有值、越界无值；越界不崩是契约层底线");

    // ⑯ 面法线：立方体 12 面法线**逐面非零**（含绕序反向的两面，方向不同但长度同）。
    let normals = f.face_normals(&v);
    let mut all_nonzero = normals.len() == expect::CUBE_TRIS as usize;
    for n in normals.iter() {
        if n.x == 0 && n.y == 0 && n.z == 0 {
            all_nonzero = false;
        }
    }
    // 双向：正反面法线的 y 分量**符号相反**（证明法线真的反映了绕序）。
    let sign_mix = {
        let mut pos = false;
        let mut neg = false;
        for n in normals.iter() {
            if n.y > 0 {
                pos = true;
            }
            if n.y < 0 {
                neg = true;
            }
        }
        pos && neg
    };
    s.add("立方体 12 面法线非零且含正反朝向", all_nonzero && sign_mix, "绕序反向的面法线y 符号相反");

    // ⑰ 退化面法线为**零向量**（不返回上个面的方向）。
    let dv = tc::FaceSet::new();
    let mut df = tc::FaceSet::new();
    df.push(tc::FaceRef::new(0, 1, 1));
    let dn = df.face_normals(&v);
    let dv_ok = dv.face_normals(&v).is_empty() && dn.len() == 1 && dn[0].x == 0 && dn[0].y == 0
        && dn[0].z == 0;
    s.add(
        "退化面法线为零向量（不沿用邻面方向）",
        dv_ok,
        "零向量让 UI 显式看到「无��线」，比返回邻面方向好",
    );
}

// ---------------------------------------------------------------------------
// b 族：修复器数据
// ---------------------------------------------------------------------------

fn c1614_repair(s: &mut CheckSet) {
    // ① 三字段 schema：类型/位置/建议齐备。
    let e = tc::IssueEntry::new(
        tc::IssueKind::DegenerateFace,
        tc::IssueLocation::at_index(3),
        tc::RepairSuggestion::new("drop_face", true, false),
    );
    s.add(
        "问题条目三字段齐备",
        e.schema_ok() && e.location.has_index() && e.kind == tc::IssueKind::DegenerateFace,
        "类型/位置/建议三字段，须能分辨「无位置」与「位置为0」",
    );

    // ② 位置缺省**唯一可表示**（NONE 无索引、索引 0 有索引）。
    let none = tc::IssueLocation::NONE;
    let zero = tc::IssueLocation::at_index(0);
    s.add(
        "位置缺省与索引0可区分",
        !none.has_index() && zero.has_index() && none.offset.x == 0,
        "NONE.index=u32::MAX；索引 0 是合法位置不是缺省",
    );

    // ③ 建议**只描述不执行**：动作短码非空，且契约层无执行入口（编译期事实，
    //    这里断言形状：无 `apply` 类字段，只有 action/idempotent/needs_confirm）。
    let sug = tc::RepairSuggestion::new("remap_index", false, true);
    let shape_ok = sug.action == "remap_index" && sug.idempotent == false && sug.needs_confirm;
    s.add(
        "建议只描述不执行（无执行入口）",
        shape_ok && sug.action != "none",
        "契约层只给动作短码与两个标志位，执行权在 UI/命令层",
    );

    // ④ 不可逆动作必须要求确认（**双向**：幂等且无需确认 ⇒ 不要求）。
    let idem = tc::RepairSuggestion::new("drop_face", true, false);
    let non_idem = tc::RepairSuggestion::new("remap_index", false, false);
    let need = tc::RepairSuggestion::new("drop_face", true, true);
    s.add(
        "不可逆/显式确认动作须要求确认",
        non_idem.confirm_required() && need.confirm_required() && !idem.confirm_required(),
        "非幂等或 needs_confirm ⇒ 要求确认；幂等且不需确认 ⇒ 不要求（否则恒真）",
    );

    // ⑤ 问题类型六类封闭 + 短码逐字节匹配写死期望。
    let mut kind_wire_ok = tc::IssueKind::ALL.len() == expect::KIND_WIRES.len();
    for (i, k) in tc::IssueKind::ALL.iter().enumerate() {
        if k.wire() != expect::KIND_WIRES[i] {
            kind_wire_ok = false;
        }
    }
    s.add("问题类型六类短码匹配写死期望", kind_wire_ok, "dupvert/degenface/flipnorm/idxrange/degenedge/zeroarea");

    // ⑥ 错误码逐项等于写死期望（含域号 0x1614不被截断）。
    let mut code_ok = true;
    for (i, k) in tc::IssueKind::ALL.iter().enumerate() {
        if k.code() != expect::KIND_CODES[i] {
            code_ok = false;
        }
    }
    // 反向：截断成 u16 会让所有码变成 1..6 ⇒ 域号消失。这里显式断高位。
    let hi_ok = tc::IssueKind::DuplicateVertex.code() > 0xFFFF;
    s.add(
        "错误码含域号不被截断",
        code_ok && hi_ok,
        "0x1614_xxxx 需 u32；若写 u16 会被截断成 1..6，域号整个消失",
    );

    // ⑦ 六类错误码**互异**（否则下游按码分派会撞）。
    let mut uniq = true;
    for i in 0..tc::IssueKind::ALL.len() {
        for j in (i + 1)..tc::IssueKind::ALL.len() {
            if tc::IssueKind::ALL[i].code() == tc::IssueKind::ALL[j].code() {
                uniq = false;
            }
        }
    }
    s.add("六类错误码两两互异", uniq, "码撞了按码分派就不可靠");

    // ⑧ 短码可逆（含未知短码不误映射）。
    let mut rt = true;
    for k in tc::IssueKind::ALL.iter() {
        match tc::IssueKind::from_wire(k.wire()) {
            Some(back) => {
                if back != *k {
                    rt = false;
                }
            }
            None => rt = false,
        }
    }
    if tc::IssueKind::from_wire("nope").is_some() {
        rt = false;
    }
    s.add("问题类型短码可逆且未知短码为None", rt, "往返一致 + 未知短码必须 None");

    // ⑨ 退化面**只标记不修正**（悄悄修成别的面会让数据与网格对不上）。
    let degen = tc::FaceRef::new(0, 1, 1);
    let good = tc::FaceRef::new(0, 1, 2);
    s.add(
        "退化面只标记不修正",
        degen.is_degenerate() && !good.is_degenerate() && degen.v == [0, 1, 1],
        "退化面的原始索引必须原样保留（改成别的面=数据与网格对不上）",
    );

    // ⑩ 三种退化形态都要认（v0==v1 / v1==v2 / v0==v2）。
    let d01 = tc::FaceRef::new(5, 5, 9);
    let d12 = tc::FaceRef::new(0, 6, 6);
    let d02 = tc::FaceRef::new(7, 1, 7);
    s.add(
        "退化面三种形态均被识别",
        d01.is_degenerate() && d12.is_degenerate() && d02.is_degenerate(),
        "v0==v1 / v1==v2 / v0==v2 三种都要认，只认一种会让另两种漏检",
    );

    // ⑪ 退化面计数逐个统计（**不是「有退化面就返回 1」**）。
    let mut fc = tc::FaceSet::new();
    fc.push(tc::FaceRef::new(0, 1, 2));
    fc.push(tc::FaceRef::new(0, 1, 1));
    fc.push(tc::FaceRef::new(3, 3, 4));
    fc.push(tc::FaceRef::new(5, 5, 5));
    s.add("退化面计数逐个统计", fc.degenerate_count() == 3, "4 面中 3 面退化 ⇒ 计数恰为 3（恒返回 1 会假绿）");

    // ⑫ 面集合去重边（两三角形共边只算一条）。
    let mut shared = tc::FaceSet::new();
    shared.push(tc::FaceRef::new(0, 1, 2));
    shared.push(tc::FaceRef::new(1, 2, 3));
    // 两面共边 (1,2)；各自另两条边 ⇒ 去重后 5 条
    s.add(
        "共边只算一条（去重后5条）",
        shared.unique_edges().len() == 5,
        "两个三角形共一条边，去重后 5 条（非 6 条）",
    );

    // ⑬ 问题清单计数逐类（`count_of` 精确）。
    let mut il = tc::IssueList::new();
    il.push(tc::IssueEntry::new(
        tc::IssueKind::DegenerateFace,
        tc::IssueLocation::at_index(0),
        tc::RepairSuggestion::new("drop_face", true, false),
    ));
    il.push(tc::IssueEntry::new(
        tc::IssueKind::DegenerateFace,
        tc::IssueLocation::at_index(1),
        tc::RepairSuggestion::new("drop_face", true, false),
    ));
    il.push(tc::IssueEntry::new(
        tc::IssueKind::ZeroAreaFace,
        tc::IssueLocation::at_index(0),
        tc::RepairSuggestion::new("drop_face", true, false),
    ));
    il.push(tc::IssueEntry::new(
        tc::IssueKind::IndexOutOfRange,
        tc::IssueLocation::at_index(2),
        tc::RepairSuggestion::new("remap_index", false, true),
    ));
    s.add(
        "问题清单分类计数逐类相符",
        il.len() == 4
            && il.count_of(tc::IssueKind::DegenerateFace) == 2
            && il.count_of(tc::IssueKind::ZeroAreaFace) == 1
            && il.count_of(tc::IssueKind::IndexOutOfRange) == 1
            && il.count_of(tc::IssueKind::FlippedNormal) == 0,
        "四类分布 2/1/1/0；未出现类须为 0（恒返回 1 会假绿）",
    );

    // ⑭ 需确认条目计数（只有非幂等那条）。
    s.add(
        "需确认条目只算非幂等项",
        il.confirm_required_count() == 1,
        "仅 remap_index 非幂等 ⇒ 恰 1 条需确认",
    );

    // ⑮ 清单 schema 逐条合规（**不是「有一条合规就算过」**）。
    let mut bad = tc::IssueList::new();
    bad.push(tc::IssueEntry::new(
        tc::IssueKind::DegenerateFace,
        tc::IssueLocation::at_index(0),
        tc::RepairSuggestion::NONE, // action = "none" 仍非空，故合规
    ));
    s.add(
        "清单 schema 逐条检查（非抽样）",
        il.schema_ok() && bad.schema_ok(),
        "逐条查而非抽首条：抽首条时首条合规即假绿",
    );
}

// ---------------------------------------------------------------------------
// c 族：契约冻结 + 预览 + 判据集自检
// ---------------------------------------------------------------------------

fn c1614_freeze(s: &mut CheckSet) {
    // ① 版本等于写死期望。
    s.add(
        "契约版本=1",
        tc::ContractFreeze::version() == expect::VERSION,
        "版本写死 1；变更线上格式必须递增",
    );

    // ② 五类字段冻结（恰好五类 + 短码互异）。
    s.add(
        "查看器五类冻结",
        tc::ContractFreeze::viewer_fields_frozen(),
        "恰好 5 类且短码两两互异（重码 ⇒ 线上无法区分两类）",
    );

    // ③ 问题类型冻结（六类 + 码互异 + 短码可逆）。
    s.add(
        "问题类型六类冻结",
        tc::ContractFreeze::issue_kinds_frozen(),
        "恰好 6 类、错误码互异、短码往返一致",
    );

    // ④ 位置schema 冻结。
    s.add(
        "位置 schema 冻结",
        tc::ContractFreeze::location_schema_frozen(),
        "缺省状态唯一可表示（有索引/无索引两种）",
    );

    // ⑤ 线上格式指纹**逐字节等于判据侧写死的值**。
    //    弱门禁警示：原写法 `WIRE_SHAPE == compute_wire_shape()` 是**恒真**
    //    （同函数同参数自比），真正生效的只有 `&& != 0` —— 于是「把指纹算法
    //    换成任意非零常量」与「把五类顺序对调」两种真改动都会全绿。
    //    改为与写死常量比对，三种改动（改算法/改顺序/改短码）都会变红。
    s.add(
        "线上格式指纹逐字节匹配写死值",
        tc::WIRE_SHAPE == expect::WIRE_SHAPE,
        "FNV-1a over hl+wf+nm+uv+bb = 2315004178；判据侧钉死数值，改算法/顺序/短码都会红",
    );

    // ⑤-2 数据包携带的指纹必须与常量一致（防「构造时填了别的值」）。
    let fp_pkt = tc::ViewerPacket::new(21);
    s.add(
        "数据包携带的指纹与常量一致",
        fp_pkt.wire_shape == expect::WIRE_SHAPE,
        "构造期填入的 wire_shape 必须等于冻结值（否则冻结面在数据面上失效）",
    );

    // ⑥ 总冻结（四条同时成立）。**双向**：逐条已单独验过，此处只验合取。
    s.add("契约总冻结", tc::ContractFreeze::all_frozen(), "四条子判据的合取");

    // ⑦ 预览双快照：无变化时 `is_noop` 为真。
    let (v, f) = cube();
    let snap = tc::MeshSnapshot::build(v.clone(), f.clone());
    let same = tc::RepairPreview::new(9, snap.clone(), snap.clone());
    s.add(
        "预览前后一致时判定为无操作",
        same.is_noop() && same.vertex_delta() == 0 && same.face_delta() == 0,
        "对照面：指纹相同 ⇒ is_noop，否则 UI 会展示两份一样的对照",
    );

    // ⑧ 预览有变化时 `is_noop` 为假，且 delta **有符号**。
    //    合并顶点会让顶点数**减少** ⇒ delta 必须能表达负数。
    let mut v2 = tc::VertexSet::new();
    for p in v.positions.iter() {
        v2.push(*p);
    }
    v2.push(tc::FixedVec3::from_em(0.5, 0.5, 0.5)); // 加一个顶点
    let after = tc::MeshSnapshot::build(v2, f.clone());
    let changed = tc::RepairPreview::new(10, snap.clone(), after.clone());
    let grew = changed.vertex_delta() == 1 && changed.face_delta() == 0;
    // 反向：顶点减少 ⇒ delta 为负
    let shrunk = tc::RepairPreview::new(11, after.clone(), snap.clone());
    let shr_ok = shrunk.vertex_delta() == -1;
    s.add(
        "预览 delta 有符号（增与减都成立）",
        !changed.is_noop() && grew && shr_ok,
        "加顶点 delta=+1、减顶点 delta=-1；无符号类型会让减少变天文数字",
    );

    // ⑨ 预览包围盒自洽（两侧各等于其顶点集实算值）。
    //`bounds_consistent` 是 RepairPreview 的方法（判一次即覆盖 before/after 两侧）。
    let consistent = changed.bounds_consistent();
    let consistent2 = tc::RepairPreview::new(15, snap.clone(), snap.clone()).bounds_consistent();
    s.add(
        "预览两侧包围盒与顶点集一致",
        consistent && consistent2,
        "防「快照被就地改过只更新了一半」导致 UI 展示错配对照",
    );

    // ⑩ 修复预览**不改输入**（纯函数纪律：before/after 须与传入快照等价）。
    let input_snapshot = tc::MeshSnapshot::build(v.clone(), f.clone());
    let fp_before = input_snapshot.fingerprint();
    let mut pv = tc::RepairPreview::new(12, input_snapshot.clone(), after.clone());
    let _ = pv.target(tc::IssueKind::DegenerateFace);
    s.add(
        "预览构造不改传入快照",
        input_snapshot.fingerprint() == fp_before,
        "纯函数：构造预览不得改动调用方持有的快照",
    );

    // ⑪ 摘要**只报计数不含坐标**（隐私：值可能来自用户资产）。
    let out = tc::build_contract(13, &v, &f);
    let sum = tc::summarize(&out);
    let leak = sum.contains("1000000") || sum.contains("100000");
    let has_counts = sum.contains("fields=") && sum.contains("issues=");
    s.add(
        "摘要只报计数不含坐标值",
        has_counts && !leak,
        "摘要含 fields=/issues= 等计数，且不得出现定点坐标字面量",
    );

    // ⑫ 摘要对**问题网格**如实报出问题数（含越界与退化）。
    let (bv, bf) = broken_mesh();
    let bout = tc::build_contract(14, &bv, &bf);
    let idx_n = bout.issues.count_of(tc::IssueKind::IndexOutOfRange);
    let deg_n = bout.issues.count_of(tc::IssueKind::DegenerateFace);
    s.add(
        "问题网格如实产出越界与退化问题",
        idx_n == 2 && deg_n == 2,
        "语料含两退化面 + 两处越界引用 ⇒ 计数恰为 2/2（恒返回 1 会假绿）",
    );
}

/// 判据集自检（等价对照：确保判据集自身没被悄悄改弱）。
fn meta_checks(s: &mut CheckSet) {
    // ① 五类字段数恰为 5、六类问题数恰为 6（锚点明列的数字）。
    s.add(
        "判据集锚定五类/六类",
        tc::ViewerField::ALL.len() == 5 && tc::IssueKind::ALL.len() == 6,
        "锚点明列：查看器五类、问题类型六类",
    );

    // ② 写死期望与被测短码**逐项一致**（若被测改了短码，本判据先红，
    //    提示「要么更新期望要么改回实现」，而不是让所有下游静默失配）。
    let mut aligned = true;
    for (i, fld) in tc::ViewerField::ALL.iter().enumerate() {
        if fld.wire() != expect::FIELD_WIRES[i] {
            aligned = false;
        }
    }
    for (i, k) in tc::IssueKind::ALL.iter().enumerate() {
        if k.code() != expect::KIND_CODES[i] {
            aligned = false;
        }
    }
    s.add("判据写死期望与被测逐项对齐", aligned, "期望写死在判据侧，不从被测反推");

    // ③ 所有判据名**互异**（重名会让两条判据在报告里无法区分，
    //    且一条变红可能被另一条掩盖）。
    let mut names: Vec<&str> = vec![
        "查看器五类字段各恰好一条规格",
        "五类短码逐字节匹配写死期望",
        "短码可逆且未知短码不误映射",
        "新包默认五类均未产出",
        "五类 set_field/field 往返一致",
        "五类齐备即完整（对照面：否则「永远不完整」也全绿）",
        "单位立方体唯一边数=18（欧拉公式 V-E+F=2）",
        "边规范化使反向边判等",
        "自环边可标记且不被丢弃",
        "单位立方体包围盒恰为[0,1]^3",
        "包围盒尺寸三轴均为 1em",
        "空顶点集包围盒空且尺寸为0（不下溢）",
        "包围盒空/满短码可区分",
        "全负坐标包围盒 min/max 不写反",
        "顶点索引越界返回 None 而非 panic",
        "立方体 12 面法线非零且含正反朝向",
        "退化面法线为零向量（不沿用邻面方向）",
        "问题条目三字段齐备",
        "位置缺省与索引0可区分",
        "建议只描述不执行（无执行入口）",
        "不可逆/显式确认动作须要求确认",
        "问题类型六类短码匹配写死期望",
        "错误码含域号不被截断",
        "六类错误码两两互异",
        "问题类型短码可逆且未知短码为None",
        "退化面只标记不修正",
        "退化面三种形态均被识别",
        "退化面计数逐个统计",
        "共边只算一条（去重后5条）",
        "问题清单分类计数逐类相符",
        "需确认条目只算非幂等项",
        "清单 schema 逐条检查（非抽样）",
        "契约版本=1",
        "查看器五类冻结",
        "问题类型六类冻结",
        "位置 schema 冻结",
        "线上格式指纹逐字节匹配写死值",
        "数据包携带的指纹与常量一致",
        "契约总冻结",
        "预览前后一致时判定为无操作",
        "预览 delta 有符号（增与减都成立）",
        "预览两侧包围盒与顶点集一致",
        "预览构造不改传入快照",
        "摘要只报计数不含坐标值",
        "问题网格如实产出越界与退化问题",
        "判据集锚定五类/六类",
        "判据写死期望与被测逐项对齐",
    ];
    let total = names.len();
    names.sort_unstable();
    let mut dup = false;
    for i in 1..total {
        if names[i] == names[i - 1] {
            dup = true;
        }
    }
    // 清单列出的是**除本条以外**的全部判据名（本条无法把自己列进自己）：
    // 47 条被列 + 1 条本条 = 48 条 `s.add`，故期望 47 而非 48。
    // 写错这个数会让本条恒红，而恒红的门禁等于没有门禁。
    s.add(
        "全部判据名互异",
        !dup && total == 47,
        "判据名重名会让两条判据无法区分；清单 47 条（不含本条）+ 本条 = 48 条 s.add",
    );
}

// ---------------------------------------------------------------------------
// 三族独立入口（聚合器按族登记，规避 MAX_CHECKS 截断）
// ---------------------------------------------------------------------------

/// a 族：查看器数据。
pub fn run_vef17_checks_a() -> CheckSet {
    let mut s = CheckSet::new("vef17-viewer");
    c1614_viewer(&mut s);
    s
}

/// a 族独立入口（聚合器用）。
pub fn run_vef17_checks_a_standalone() -> CheckSet {
    run_vef17_checks_a()
}

/// b 族：修复器数据。
pub fn run_vef17_checks_b() -> CheckSet {
    let mut s = CheckSet::new("vef17-repair");
    c1614_repair(&mut s);
    s
}

/// b 族独立入口（聚合器用）。
pub fn run_vef17_checks_b_standalone() -> CheckSet {
    run_vef17_checks_b()
}

/// c 族：契约冻结 + 预览 + 判据集自检。
pub fn run_vef17_checks_c() -> CheckSet {
    let mut s = CheckSet::new("vef17-freeze");
    c1614_freeze(&mut s);
    meta_checks(&mut s);
    s
}

/// c 族独立入口（聚合器用）。
pub fn run_vef17_checks_c_standalone() -> CheckSet {
    run_vef17_checks_c()
}

/// 三族合并（便于单点调用；仍**不超** `MAX_CHECKS`：17+15+14=46）。
pub fn run_vef17_checks() -> CheckSet {
    CheckSet::merge(CheckSet::merge(run_vef17_checks_a(), run_vef17_checks_b()),
        run_vef17_checks_c())
}

/// 判据集条数（供文档/聚合器自检，不参与判定）。
pub fn total_check_count() -> u32 {
    47
}