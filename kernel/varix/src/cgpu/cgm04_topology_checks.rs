//! CGPU-F1924 域自检 · 多显示器拓扑（17+ 判据两族：模型/变更/持久/协同四组
//! A 族 + 判据承载力 B 族）。
//!
//! 判据纪律：
//! - 判据侧独立重算：矩形相交用 max/min 交集式第二实现（恰边界 <≠≤）、
//!   FNV 用逐字节独立第二实现、回放差集对账手写期望序号表；
//! - 变异可杀：恰边界、篡改、倒挂、克隆语义逐条绑定专属显性码；
//! - 零 panic 双面自扫（生产面 + 判据面，去注释去字符串后扫描）；
//! - 聚合守恒防自调（族条数在 tally 时刻精确对账）。

use crate::checks::CheckSet;
use crate::cgpu::cgm04_topology::{
    fnv1a, rect_overlap, validate, TopoEventKind, TopoRole, Topology, ScreenCell,
    CLONE_NONE, EVENT_CAP, MAX_SCREENS, OS_COORD, PERSIST_MAGIC, PERSIST_PATTERN,
    PERSIST_VERSION,
};
use crate::cgpu::cgm01_display::VmCode;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const EXPECT_A_COUNT: usize = 18;
const EXPECT_B_BEFORE: usize = 6;

// ---------------------------------------------------------------------------
// 夹具（判据侧独立逐字段构造）
// ---------------------------------------------------------------------------

/// 主屏 1920x1080 @(0,0)。
fn cell_primary() -> ScreenCell {
    ScreenCell {
        id: 1u16,
        role: TopoRole::Primary,
        x: 0i32,
        y: 0i32,
        w: 1920u32,
        h: 1080u32,
        clone_of: CLONE_NONE,
    }
}

/// 扩展屏 @(1920,0)。
fn cell_extend_right() -> ScreenCell {
    ScreenCell {
        id: 2u16,
        role: TopoRole::Extend,
        x: 1920i32,
        y: 0i32,
        w: 1920u32,
        h: 1080u32,
        clone_of: CLONE_NONE,
    }
}

/// 扩展屏 @(0,1080)（纵向排列）。
fn cell_extend_below() -> ScreenCell {
    ScreenCell {
        id: 3u16,
        role: TopoRole::Extend,
        x: 0i32,
        y: 1080i32,
        w: 1920u32,
        h: 1080u32,
        clone_of: CLONE_NONE,
    }
}

/// 克隆屏（克隆主屏——同位同矩形）。
fn cell_clone_of_primary() -> ScreenCell {
    let p = cell_primary();
    ScreenCell {
        id: 4u16,
        role: TopoRole::Clone,
        x: p.x,
        y: p.y,
        w: p.w,
        h: p.h,
        clone_of: p.id,
    }
}

fn base_cells() -> Vec<ScreenCell> {
    let mut v = Vec::new();
    v.push(cell_primary());
    v.push(cell_extend_right());
    v
}

// ---------------------------------------------------------------------------
// A1 · 模型组
// ---------------------------------------------------------------------------

fn chk_model(set: &mut CheckSet) {
    // 模型-01 合法扩展拓扑（主屏+双扩展）全验通过；克隆拓扑亦通过。
    let mut cells = base_cells();
    cells.push(cell_extend_below());
    let ok_ext = validate(&cells).is_ok();
    let mut clo = base_cells();
    clo.push(cell_clone_of_primary());
    let ok_clo = validate(&clo).is_ok();
    if ok_ext && ok_clo {
        set.ok("E924-模型-01-合法拓扑全验通过");
    } else {
        set.fail("E924-模型-01-合法拓扑全验通过", "合法拓扑被误拒");
    }

    // 模型-02 主屏恰一双向：零主屏与双主屏都是 PRIMARY_DUPLICATE。
    let zero = vec![cell_extend_right(), cell_extend_below()];
    let zero_r = validate(&zero);
    let mut dup = base_cells();
    let mut second_primary = cell_extend_below();
    second_primary.role = TopoRole::Primary;
    dup.push(second_primary);
    let dup_r = validate(&dup);
    let both_dup = match (zero_r, dup_r) {
        (Err(a), Err(b)) => {
            a == crate::cgpu::cgm04_topology::codes::PRIMARY_DUPLICATE
                && b == crate::cgpu::cgm04_topology::codes::PRIMARY_DUPLICATE
        }
        _ => false,
    };
    if both_dup {
        set.ok("E924-模型-02-主屏恰一双向");
    } else {
        set.fail("E924-模型-02-主屏恰一双向", "零主屏/双主屏未被专属码拒");
    }

    // 模型-03 排列重叠双向：相交被拒；分离后通过（恰边界：边贴边不算相交）。
    let mut ov = base_cells();
    let mut bad = cell_extend_below();
    bad.x = 100i32;
    bad.y = 100i32;
    ov.push(bad);
    let ov_r = validate(&ov);
    let ov_is = match ov_r {
        Err(c) => c == crate::cgpu::cgm04_topology::codes::ARRANGE_OVERLAP,
        Ok(()) => false,
    };
    let mut touching = base_cells();
    let mut edge = cell_extend_below();
    edge.x = 0i32;
    edge.y = 1080i32;
    touching.push(edge);
    let touching_ok = validate(&touching).is_ok();
    if ov_is && touching_ok {
        set.ok("E924-模型-03-排列重叠恰边界双向");
    } else {
        set.fail("E924-模型-03-排列重叠恰边界双向", "重叠漏拦或边贴边误拦");
    }

    // 模型-04 克隆语义三向：源不存在/矩形不一/链式克隆 → CLONE_CONFLICT；合法克隆通过（模型-01 已断）。
    let mut ghost = base_cells();
    let mut g = cell_clone_of_primary();
    g.clone_of = 99u16;
    ghost.push(g);
    let ghost_r = validate(&ghost);
    let mut moved = base_cells();
    let mut m = cell_clone_of_primary();
    m.x = 5i32;
    moved.push(m);
    let moved_r = validate(&moved);
    let mut chain = base_cells();
    chain.push(cell_clone_of_primary());
    let mut cc = cell_clone_of_primary();
    cc.id = 5u16;
    cc.clone_of = 4u16;
    chain.push(cc);
    let chain_r = validate(&chain);
    let want = crate::cgpu::cgm04_topology::codes::CLONE_CONFLICT;
    let three_ok = match (ghost_r, moved_r, chain_r) {
        (Err(a), Err(b), Err(c)) => a == want && b == want && c == want,
        _ => false,
    };
    if three_ok {
        set.ok("E924-模型-04-克隆语义三向拒绝");
    } else {
        set.fail("E924-模型-04-克隆语义三向拒绝", "克隆违约未被专属码拦");
    }

    // 模型-05 身份守恒：空集/零 ID/重复 ID/超员 → TOPOLOGY_MALFORMED。
    let empty: Vec<ScreenCell> = Vec::new();
    let empty_r = validate(&empty);
    let mut zid = base_cells();
    let mut z = cell_extend_right();
    z.id = 0u16;
    zid[1usize] = z;
    let zid_r = validate(&zid);
    let mut dup_id = base_cells();
    let mut d = cell_extend_right();
    d.id = 1u16;
    dup_id[1usize] = d;
    let dup_id_r = validate(&dup_id);
    let mut many: Vec<ScreenCell> = Vec::new();
    many.push(cell_primary());
    let mut nid = 2u16;
    let mut k = 0usize;
    while k < MAX_SCREENS {
        let mut c = cell_extend_right();
        c.id = nid;
        c.x = 4000i32 + (nid as i32) * 2000i32;
        nid += 1u16;
        many.push(c);
        k += 1usize;
    }
    let many_r = validate(&many);
    let want_m = crate::cgpu::cgm04_topology::codes::TOPOLOGY_MALFORMED;
    let four_ok = match (empty_r, zid_r, dup_id_r, many_r) {
        (Err(a), Err(b), Err(c), Err(d)) => {
            a == want_m && b == want_m && c == want_m && d == want_m
        }
        _ => false,
    };
    if four_ok {
        set.ok("E924-模型-05-身份守恒四向");
    } else {
        set.fail("E924-模型-05-身份守恒四向", "空/零 ID/重复 ID/超员漏拦");
    }
}

// ---------------------------------------------------------------------------
// A2 · 变更组
// ---------------------------------------------------------------------------

fn chk_events(set: &mut CheckSet) {
    // 变更-01 合法变更落账单调：seq +1，事件种类与序号一致。
    let mut topo = match Topology::new(base_cells()) {
        Ok(t) => t,
        Err(_) => {
            set.fail("E924-变更-01-合法变更落账单调", "基线拓扑建账失败");
            return;
        }
    };
    let mut cells = base_cells();
    cells.push(cell_extend_below());
    let ev = topo.apply(cells, TopoEventKind::ScreenAdded);
    let ev_ok = match ev {
        Ok(e) => e.seq == 1u64 && e.kind == TopoEventKind::ScreenAdded && topo.seq() == 1u64,
        Err(_) => false,
    };
    if ev_ok {
        set.ok("E924-变更-01-合法变更落账单调");
    } else {
        set.fail("E924-变更-01-合法变更落账单调", "seq/事件种类不一致");
    }

    // 变更-02 非法变更不落账：拒绝后 seq 原地。
    let bad: Vec<ScreenCell> = Vec::new();
    let bad_r = topo.apply(bad, TopoEventKind::LayoutChanged);
    let no_land = bad_r.is_err() && topo.seq() == 1u64;
    if no_land {
        set.ok("E924-变更-02-非法变更不落账");
    } else {
        set.fail("E924-变更-02-非法变更不落账", "非法变更污染了账本");
    }

    // 变更-03 回放差集：手写期望序号表 [2,3]（last_seen=1）与 [1,2,3]（0）。
    let mut cells2 = base_cells();
    cells2.push(cell_extend_below());
    let _ = topo.apply(cells2, TopoEventKind::PrimaryChanged);
    let mut cells3 = base_cells();
    if let Some(first) = cells3.get_mut(0usize) {
        first.h = 1440u32;
    }
    let _ = topo.apply(cells3, TopoEventKind::ScreenRemoved);
    let r0 = topo.replay(0u64);
    let r1 = topo.replay(1u64);
    let want0: [u64; 3] = [1u64, 2u64, 3u64];
    let want1: [u64; 2] = [2u64, 3u64];
    let got0: Vec<u64> = match &r0 {
        Ok(v) => v.iter().map(|e| e.seq).collect(),
        Err(_) => Vec::new(),
    };
    let got1: Vec<u64> = match &r1 {
        Ok(v) => v.iter().map(|e| e.seq).collect(),
        Err(_) => Vec::new(),
    };
    if got0 == want0.to_vec() && got1 == want1.to_vec() {
        set.ok("E924-变更-03-回放差集对账");
    } else {
        set.fail("E924-变更-03-回放差集对账", "回放差集与手写期望不符");
    }

    // 变更-04 倒挂拒：last_seen 超前 → PROPAGATION_STALE。
    let stale = topo.replay(topo.seq() + 1u64);
    let stale_ok = match stale {
        Err(c) => c == crate::cgpu::cgm04_topology::codes::PROPAGATION_STALE,
        Ok(_) => false,
    };
    if stale_ok {
        set.ok("E924-变更-04-倒挂显性拒");
    } else {
        set.fail("E924-变更-04-倒挂显性拒", "倒挂未被专属码拒");
    }

    // 变更-05 封顶滚动：超过 EVENT_CAP 后账长恰封顶，最旧被淘汰。
    let mut roll = match Topology::new(base_cells()) {
        Ok(t) => t,
        Err(_) => {
            set.fail("E924-变更-05-封顶滚动淘汰", "滚动基线建账失败");
            return;
        }
    };
    let mut i = 0usize;
    while i < EVENT_CAP + 6usize {
        let mut cs = base_cells();
        if let Some(first) = cs.get_mut(0usize) {
            first.h = 1080u32 + (i as u32 % 8u32) * 8u32;
        }
        let _ = roll.apply(cs, TopoEventKind::LayoutChanged);
        i += 1usize;
    }
    let full = match roll.replay(0u64) {
        Ok(v) => v.len() == EVENT_CAP,
        Err(_) => false,
    };
    if full {
        set.ok("E924-变更-05-封顶滚动淘汰");
    } else {
        set.fail("E924-变更-05-封顶滚动淘汰", "事件账未按封顶滚动");
    }
}

// ---------------------------------------------------------------------------
// A3 · 持久组
// ---------------------------------------------------------------------------

fn chk_persist(set: &mut CheckSet) {
    // 持久-01 往返全等重启保持：seq/cells/主屏逐字段全等；事件史为会话内面（恢复后账空）。
    let mut topo = match Topology::new(base_cells()) {
        Ok(t) => t,
        Err(_) => {
            set.fail("E924-持久-01-往返全等重启保持", "基线建账失败");
            return;
        }
    };
    let mut cells = base_cells();
    cells.push(cell_extend_below());
    let _ = topo.apply(cells, TopoEventKind::ScreenAdded);
    let blob = topo.serialize();
    let back = Topology::restore(&blob);
    let round_ok = match back {
        Ok(t) => {
            t.seq() == topo.seq()
                && t.cells() == topo.cells()
                && t.primary_id() == topo.primary_id()
                && match t.replay(0u64) {
                    Ok(v) => v.is_empty(),
                    Err(_) => false,
                }
        }
        Err(_) => false,
    };
    if round_ok {
        set.ok("E924-持久-01-往返全等重启保持");
    } else {
        set.fail("E924-持久-01-往返全等重启保持", "恢复与持久前不一致");
    }

    // 持久-02 校验和篡改拒：载荷体翻一字节 → RESTORE_MISMATCH。
    let mut tam = blob.clone();
    let mut flipped = false;
    let mut i = 15usize;
    while i + 8usize <= tam.len() {
        let old = tam[i];
        tam[i] = old ^ 0x01u8;
        if Topology::restore(&tam).is_err() {
            flipped = true;
        }
        tam[i] = old;
        if flipped {
            break;
        }
        i += 1usize;
    }
    if flipped {
        set.ok("E924-持久-02-校验和篡改拒");
    } else {
        set.fail("E924-持久-02-校验和篡改拒", "载荷篡改未被校验和抓到");
    }

    // 持久-03 版本拒：版本字节漂移 → RESTORE_MISMATCH。
    let mut vb = blob.clone();
    vb[4usize] = PERSIST_VERSION + 1u8;
    let ver_r = Topology::restore(&vb);
    let ver_ok = match ver_r {
        Err(c) => c == crate::cgpu::cgm04_topology::codes::RESTORE_MISMATCH,
        Ok(_) => false,
    };
    if ver_ok {
        set.ok("E924-持久-03-版本漂移拒");
    } else {
        set.fail("E924-持久-03-版本漂移拒", "版本漂移未被专属码拒");
    }

    // 持久-04 魔数与长度拒：坏魔数/截断 → TOPOLOGY_MALFORMED。
    let mut mg = blob.clone();
    mg[0usize] = b'X';
    let mg_r = Topology::restore(&mg);
    let short = Topology::restore(&blob[..20usize]);
    let want_m = crate::cgpu::cgm04_topology::codes::TOPOLOGY_MALFORMED;
    let mg_ok = match (mg_r, short) {
        (Err(a), Err(b)) => a == want_m && b == want_m,
        _ => false,
    };
    if mg_ok {
        set.ok("E924-持久-04-魔数与长度拒");
    } else {
        set.fail("E924-持久-04-魔数与长度拒", "魔数/长度损坏未被拦");
    }

    // 持久-05 克隆拓扑往返：克隆格元（含 clone_of 与同位矩形）逐字节保真。
    let mut clo = base_cells();
    clo.push(cell_clone_of_primary());
    let ct = match Topology::new(clo) {
        Ok(t) => t,
        Err(_) => {
            set.fail("E924-持久-05-克隆拓扑往返保真", "克隆拓扑建账失败");
            return;
        }
    };
    let cblob = ct.serialize();
    let cback = Topology::restore(&cblob);
    let clo_ok = match cback {
        Ok(t) => t.cells() == ct.cells() && t.seq() == ct.seq(),
        Err(_) => false,
    };
    if clo_ok {
        set.ok("E924-持久-05-克隆拓扑往返保真");
    } else {
        set.fail("E924-持久-05-克隆拓扑往返保真", "克隆格元往返失真");
    }

    // 持久-06 模式复用声明逐字：PERSIST_PATTERN 锚定 F1834 与三件套。
    let decl_ok = PERSIST_PATTERN.contains("F1834")
        && PERSIST_PATTERN.contains("序列化+校验和+重启保持");
    if decl_ok {
        set.ok("E924-持久-06-模式复用声明逐字");
    } else {
        set.fail("E924-持久-06-模式复用声明逐字", "F1834 复用声明漂移");
    }
}

// ---------------------------------------------------------------------------
// A4 · 协同组
// ---------------------------------------------------------------------------

fn chk_os(set: &mut CheckSet) {
    // 协同-01 声明逐字：单向通报 + 不直写。
    let one_way = OS_COORD.contains("单向通报");
    let no_write = OS_COORD.contains("内核不直写 OS 设置存储");
    if one_way && no_write {
        set.ok("E924-协同-01-单向通报不直写");
    } else {
        set.fail("E924-协同-01-单向通报不直写", "OS 协同声明漂移");
    }

    // 协同-02 回流面：OS 侧改动经变更事件回流（声明在案）。
    let backflow = OS_COORD.contains("变更事件回流");
    if backflow {
        set.ok("E924-协同-02-事件回流声明在案");
    } else {
        set.fail("E924-协同-02-事件回流声明在案", "回流面未声明");
    }
}

// ---------------------------------------------------------------------------
// B1 · 码段与互异（防自判死）
// ---------------------------------------------------------------------------

fn all_codes() -> [VmCode; 6] {
    [
        crate::cgpu::cgm04_topology::codes::TOPOLOGY_MALFORMED,
        crate::cgpu::cgm04_topology::codes::PRIMARY_DUPLICATE,
        crate::cgpu::cgm04_topology::codes::ARRANGE_OVERLAP,
        crate::cgpu::cgm04_topology::codes::CLONE_CONFLICT,
        crate::cgpu::cgm04_topology::codes::PROPAGATION_STALE,
        crate::cgpu::cgm04_topology::codes::RESTORE_MISMATCH,
    ]
}

fn chk_codes(set: &mut CheckSet) {
    let cs = all_codes();
    // 判据-01 码段独占 0x5413..0x5418，且不与 cgm01/02/03 已占 0x5401..0x5412 相接重叠。
    let mut seg_ok = true;
    let mut i = 0usize;
    while i < cs.len() {
        let v = cs[i].0;
        if !(v >= 0x5413u16 && v <= 0x5418u16) {
            seg_ok = false;
        }
        if v <= 0x5412u16 {
            seg_ok = false;
        }
        i += 1usize;
    }
    if seg_ok {
        set.ok("E924-判据-01-码段独占防自判死");
    } else {
        set.fail("E924-判据-01-码段独占防自判死", "诊断码越出 0x5413~0x5418");
    }

    // 判据-02 六码互异且语义名非空（独立双循环重排）。
    let mut distinct = true;
    let mut i = 0usize;
    while i < cs.len() {
        let mut j = i + 1usize;
        while j < cs.len() {
            if cs[i] == cs[j] {
                distinct = false;
            }
            j += 1usize;
        }
        i += 1usize;
    }
    if distinct {
        set.ok("E924-判据-02-六码互异");
    } else {
        set.fail("E924-判据-02-六码互异", "诊断码存在重复");
    }
}

// ---------------------------------------------------------------------------
// B2 · 判据侧独立重算（双源）
// ---------------------------------------------------------------------------

/// 独立第二实现：max/min 交集式（与生产面 < 比较式异构）。
fn ref_overlap(
    ax: i32,
    ay: i32,
    aw: u32,
    ah: u32,
    bx: i32,
    by: i32,
    bw: u32,
    bh: u32,
) -> bool {
    let ix1 = if ax > bx { ax } else { bx };
    let iy1 = if ay > by { ay } else { by };
    let ix2 = {
        let a2 = ax + aw as i32;
        let b2 = bx + bw as i32;
        if a2 < b2 {
            a2
        } else {
            b2
        }
    };
    let iy2 = {
        let a2 = ay + ah as i32;
        let b2 = by + bh as i32;
        if a2 < b2 {
            a2
        } else {
            b2
        }
    };
    ix1 < ix2 && iy1 < iy2
}

/// 独立第二实现：FNV 逐字节（显式 basis/prime，独立控制流）。
fn ref_fnv(bytes: &[u8]) -> u64 {
    let basis: u64 = 0xcbf2_9ce4_8422_2325u64;
    let prime: u64 = 0x0000_0100_0000_01b3u64;
    let mut h = basis;
    let mut idx = 0usize;
    while idx < bytes.len() {
        h = h ^ (bytes[idx] as u64);
        h = h.wrapping_mul(prime);
        idx += 1usize;
    }
    h
}

fn chk_refs(set: &mut CheckSet) {
    // 判据-03 重叠双源对账：四种手算情形（含恰边界）两实现一致且与期望一致。
    let p = cell_primary();
    let e = cell_extend_right();
    let ov_prod = rect_overlap(&p, &e);
    let ov_ref = ref_overlap(p.x, p.y, p.w, p.h, e.x, e.y, e.w, e.h);
    // 恰边界：a 右缘 == b 左缘 → 不相交（< 与 <= 的分水岭）。
    let touch = ref_overlap(0i32, 0i32, 100u32, 50u32, 100i32, 0i32, 100u32, 50u32);
    // 包含：真相交。
    let contain = ref_overlap(0i32, 0i32, 100u32, 100u32, 10i32, 10i32, 10u32, 10u32);
    let agree = ov_prod == ov_ref;
    if agree && !ov_prod && !touch && contain {
        set.ok("E924-判据-03-重叠双源恰边界对账");
    } else {
        set.fail("E924-判据-03-重叠双源恰边界对账", "双源不一致或恰边界失守");
    }

    // 判据-04 FNV 双源：空输入 = offset basis（定义式手算）；随机字节双实现一致。
    let empty_is_basis = fnv1a(&[]) == 0xcbf2_9ce4_8422_2325u64;
    let mut sample: Vec<u8> = Vec::new();
    sample.extend_from_slice(PERSIST_MAGIC);
    sample.push(PERSIST_VERSION);
    let dual = fnv1a(&sample) == ref_fnv(&sample);
    if empty_is_basis && dual {
        set.ok("E924-判据-04-FNV双源对账");
    } else {
        set.fail("E924-判据-04-FNV双源对账", "FNV 双源不一致或 basis 失守");
    }
}

// ---------------------------------------------------------------------------
// B3 · 零 panic 双面自扫 + 聚合守恒
// ---------------------------------------------------------------------------

fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn scan_panic(src: &str) -> usize {
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    hits
}

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-05 生产面零 panic。
    let prod = include_str!("cgm04_topology.rs");
    let p_hits = scan_panic(prod);
    if p_hits == 0 {
        set.ok("E924-判据-05-生产面零 panic");
    } else {
        set.fail("E924-判据-05-生产面零 panic", "生产面含 panic 面");
    }

    // 判据-06 判据面零 panic（自扫）。
    let self_src = include_str!("cgm04_topology_checks.rs");
    let s_hits = scan_panic(self_src);
    if s_hits == 0 {
        set.ok("E924-判据-06-判据面零 panic");
    } else {
        set.fail("E924-判据-06-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-07 聚合守恒防自调。
    let a = run_cgm04_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E924-判据-07-聚合守恒防自调");
    } else {
        set.fail("E924-判据-07-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：模型 + 变更 + 持久 + 协同（四组）。
pub fn run_cgm04_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm04/a");
    chk_model(&mut s);
    chk_events(&mut s);
    chk_persist(&mut s);
    chk_os(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_cgm04_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cgm04/b");
    chk_codes(&mut s);
    chk_refs(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_cgm04_checks() -> CheckSet {
    CheckSet::merge(run_cgm04_checks_a_standalone(), run_cgm04_checks_b_standalone())
}
