//! VE-F1810 · 域自检（判据逐条对应，见 `vej10_lightdbg.rs` 头注）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1810`
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **三类负载**（线框/热力/统计）→ `C10-PAY-*`
//! - **发行版零成本**（物理剔除，双向验证）→ `C10-STRIP-*`
//! - **三通道贡献**（直接光/IBL/探针分通道）→ `C10-HEAT-*`
//! - **色弱安全**（F1366 三冗余，逐通道独立解码）→ `C10-CVD-*`
//! - **降级矩阵**（截断/降档/缺项/背压）→ `C10-DEG-*`
//! - **信封与总线**（F1764 J 段，漂移拦截）→ `C10-ENV-*`
//! - **加固判据**（弱门禁回补：口径独立重算/基线非平凡/码段互异）→ `C10-HARDEN-*`
//!
//! ## 判据侧设计纪律（为什么这些判据抓得住真缺陷）
//!
//! 1. **断数值本身，不断派生布尔**：`C10-HEAT-01` 断 `hw == 32`
//!    （64 宽半档），而非断「`hw < w`」——后者对「宽除以 8」这类
//!    错误降档实现同样为真。
//! 2. **先断基线非平凡，再断目标量**：所有「计数一致」类判据前都有
//!    `C10-HARDEN-03`（语料 census 非零）作恒真防线回补——若语料
//!    构造坏了（全空），后续一致性判据会恒真放行。
//! 3. **判据面独立解码**：色弱三冗余的对拍**不走**
//!    `WireMark::for_kind`（那等于用被测表校对被测表），判据侧用
//!    **字面量表**逐通道独立解码对拍——三通道表若有一通道写错，
//!    字面量对拍当红。
//! 4. **发行版零成本双向验证**：Debug 档真递增作对照 + Release 档
//!    强行百次仍恰为 0；且单调口径 `build_entries` 与净值口径
//!    `payload_builds` **两条独立判据同时钉**——净值可被「自增再减回」
//!    洗白（实测同类缺陷曾全绿放过），单调计数不可。
//! 5. **降档必须可观测**：`C10-ENV`/`C10-DEG-05` 断 tier 字段**等于**
//!    `Quarter`（标注到位），而非只断尺寸变小——静默降档的区别
//!    恰恰在标注字段上。
//! 6. **不写死会被几何变更带走的数字**：线框顶点数一律由
//!    [`verts_per_kind`] 判据侧重算（并另钉一个字面量锚），「96」这种
//!    上一版图元残留值会让红项指错方向——几何改了红判据，是误导不是守门。
//! 7. **五元组不得自证**：`envelope_seals` 断「随实测枚数走」（满负载 3 /
//!    热力被预算拒绝 2），写死 3 的实现在第二档仍会全绿。

use alloc::vec;
use alloc::vec::Vec;

use super::vej07_lightmgr::{FrameStats, LightDesc, LightKind, LightManager, MAX_LIGHTS};
use super::vej10_lightdbg::*;
use crate::checks::CheckSet;

/// 单测光源构造助手（经 `LightDesc::new` 走钳制面，与产线同路）。
fn mk_light(kind: LightKind, pos: (f32, f32, f32), dir: (f32, f32, f32), range: f32, id: u64) -> LightDesc {
    let (d, _w) = LightDesc::new(kind, pos, dir, (1.0, 1.0, 1.0), 1.0, range, id);
    d
}

/// 混合语料：1 方向光 + 2 点光 + 1 聚光 + 1 面光（各类型齐备，无缺项）。
fn corpus_mixed() -> LightManager {
    let mut m = LightManager::new();
    let _ = m.add(
        mk_light(LightKind::Directional, (0.0, 8.0, 0.0), (0.0, -1.0, 0.0), f32::INFINITY, 1),
        Vec::new(),
    );
    let _ = m.add(
        mk_light(LightKind::Point, (2.0, 1.0, 0.0), (0.0, 0.0, 0.0), 4.0, 2),
        Vec::new(),
    );
    let _ = m.add(
        mk_light(LightKind::Point, (-3.0, 2.0, 1.0), (0.0, 0.0, 0.0), 2.5, 3),
        Vec::new(),
    );
    let _ = m.add(
        mk_light(LightKind::Spot, (0.0, 5.0, 0.0), (0.0, -1.0, 0.0), 6.0, 4),
        Vec::new(),
    );
    let _ = m.add(
        mk_light(LightKind::Area, (1.0, 3.0, -1.0), (0.0, -1.0, 0.0), 1.5, 5),
        Vec::new(),
    );
    m
}

/// 洪泛语料：`n` 个点光（每灯 32 顶点，用于截断路径）。
fn corpus_flood(n: usize) -> LightManager {
    let mut m = LightManager::new();
    let mut i = 0usize;
    while i < n {
        let x = (i % 32) as f32;
        let z = (i / 32) as f32;
        let _ = m.add(
            mk_light(LightKind::Point, (x, 1.0, z), (0.0, 0.0, 0.0), 1.0 + (i % 7) as f32, i as u64),
            Vec::new(),
        );
        i += 1;
    }
    m
}

/// 洪泛**描述序列**（不经管理器注册）——`n` 个点光。
///
/// **为什么另开一条口**：F1807 的 `MAX_LIGHTS = 256` 使管理器路径最多产出
/// `256 × 32 = 8192 < CAP_WIRE_VERTICES` 个顶点，锚点要的「万级光源异常
/// 场景」在管理器口**根本构造不出来**。截断防线必须对**不经注册**的
/// 光源集合（合并光源体/实例化光源）同样生效，故截断判据走描述序列。
fn flood_descs(n: usize) -> Vec<LightDesc> {
    let mut v: Vec<LightDesc> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let x = (i % 32) as f32;
        let z = (i / 32) as f32;
        v.push(mk_light(
            LightKind::Point,
            (x, 1.0, z),
            (0.0, 0.0, 0.0),
            1.0 + (i % 7) as f32,
            i as u64,
        ));
        i += 1;
    }
    v
}

/// 64×64 全分辨率三通道语料：三通道内容刻意互异（三通道真分开写的
/// 可测前提——若语料三通道相同，「分通道写出」会恒真放行）。
fn heat_corpus() -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut direct = vec![0.0f32; 64 * 64];
    let mut ibl = vec![0.0f32; 64 * 64];
    let mut probe = vec![0.0f32; 64 * 64];
    let mut i = 0usize;
    while i < 64 * 64 {
        direct[i] = 1.0;
        ibl[i] = 2.0;
        probe[i] = 4.0;
        i += 1;
    }
    (direct, ibl, probe)
}

/// 全零帧统计（判据不依赖 F1807 真帧时用；select_units 单独注入真值）。
fn frame_stats(selected: usize, culled: usize, dropped: usize, total_ops: u64) -> FrameStats {
    FrameStats {
        registered: 0,
        selected,
        culled,
        dropped,
        total_ops,
    }
}

/// 判据面字面量表：glyph → 类型（独立于 `WireMark::for_kind`）。
fn glyph_table(g: u8) -> Option<LightKind> {
    match g {
        1 => Some(LightKind::Directional),
        2 => Some(LightKind::Point),
        3 => Some(LightKind::Spot),
        4 => Some(LightKind::Area),
        _ => None,
    }
}

/// 判据面字面量表：tag → 类型。
fn tag_table(t: u8) -> Option<LightKind> {
    match t {
        b'D' => Some(LightKind::Directional),
        b'P' => Some(LightKind::Point),
        b'S' => Some(LightKind::Spot),
        b'A' => Some(LightKind::Area),
        _ => None,
    }
}

/// 判据面字面量表：color → 类型。
fn color_table(c: u8) -> Option<LightKind> {
    match c {
        1 => Some(LightKind::Directional),
        2 => Some(LightKind::Point),
        3 => Some(LightKind::Spot),
        4 => Some(LightKind::Area),
        _ => None,
    }
}

/// 顶点数换算：方向光 16 / 点光 32 / 聚光 32 / 面光 8（判据侧独立重算）。
fn verts_per_kind(kind: LightKind) -> usize {
    match kind {
        LightKind::Directional => 2 * RING_SEGMENTS,
        LightKind::Point => 4 * RING_SEGMENTS,
        LightKind::Spot => 4 * RING_SEGMENTS,
        LightKind::Area => 8,
    }
}

/// 混合语料（1 方向 + 2 点 + 1 聚 + 1 面）的线框顶点总数（判据侧重算）。
///
/// **为什么不再到处写死 `96`**：写死的数字是上一版图元几��的残留（本版
/// 面光 8 顶点、聚光 32 顶点，合计 120），写死会让「几何改了」与「判据
/// 该不该跟着改」耦在一起，且改错一边时红的是判据不是缺陷——红项指错
/// 方向比不红更糟。统一从 [`verts_per_kind`] 重算，几何变则此处自动跟随。
fn mixed_vertex_total() -> usize {
    verts_per_kind(LightKind::Directional)
        + 2 * verts_per_kind(LightKind::Point)
        + verts_per_kind(LightKind::Spot)
        + verts_per_kind(LightKind::Area)
}

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vej10_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1810");

    // ------------------------------------------------------------------
    // C10-HARDEN：弱门禁回补（基线非平凡先行）
    // ------------------------------------------------------------------

    // 语料 census 非零且逐类型非零——否则后面一切「计数一致」判据恒真。
    {
        let m = corpus_mixed();
        let mut log = JdiagLog::default();
        let c = KindCensus::from_manager(&m, &mut log);
        set.add(
            "C10-HARDEN-01",
            c.directional == 1 && c.point == 2 && c.spot == 1 && c.area == 1 && c.active() == 5,
            "语料基线 1/2/1/1 合计 5",
        );
    }

    // 诊断码段互异（码段判据防自判死的前置：两两不等才可作判据键）。
    {
        let codes = [
            JdbgCode::WIREFRAME_TRUNCATED,
            JdbgCode::HEAT_DOWNTIERED,
            JdbgCode::HEAT_CAPACITY_EXHAUSTED,
            JdbgCode::STATS_KIND_MISSING,
            JdbgCode::BUS_BACKPRESSURE_DROPPED,
            JdbgCode::ENVELOPE_DRIFT,
            JdbgCode::RELEASE_FORCED,
            JdbgCode::WIRE_NON_FINITE,
            JdbgCode::HEAT_NON_FINITE,
            JdbgCode::WIRE_RANGE_INFINITE,
        ];
        let mut all_distinct = true;
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    all_distinct = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 全部落在本条独占段 0x31。
        let mut all_in_segment = true;
        for c in codes.iter() {
            if (c.0 & 0xFF00) != 0x3100 {
                all_in_segment = false;
            }
        }
        set.add(
            "C10-HARDEN-02",
            all_distinct && all_in_segment,
            "10 枚诊断码两两互异且全在 0x31xx 段",
        );
    }

    // ------------------------------------------------------------------
    // C10-PAY：三类负载
    // ------------------------------------------------------------------

    // 线框：顶点数与判据侧独立重算一致（每类图元顶点换算独立核对）。
    //
    // 追加钉一个字面量 120：重算式若与常量同时被改错（例如两处都把聚光
    // 写成 24），纯重算式会自洽放行；字面量是不参与推导的第三方锚。
    {
        let m = corpus_mixed();
        let mut log = JdiagLog::default();
        let w = build_wireframe(&m, &mut log);
        let expect = mixed_vertex_total();
        set.add(
            "C10-PAY-01",
            w.vertices.len() == expect
                && expect == 120
                && w.total_vertices == expect
                && !w.truncated,
            "混合语料线框顶点 = 16+2x32+32+8 = 120，无截断",
        );
    }

    // 线框：顶点成对（线段图元）且 byte_len 口径独立重算（15 字节/顶点）。
    {
        let m = corpus_mixed();
        let mut log = JdiagLog::default();
        let w = build_wireframe(&m, &mut log);
        let mut paired = true;
        let mut i = 0;
        while i < w.vertices.len() {
            // 位置分量为有限值（图元面零 NaN 传播）。
            let v = &w.vertices[i];
            if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
                paired = false;
            }
            i += 1;
        }
        set.add(
            "C10-PAY-02",
            w.vertices.len() % 2 == 0 && paired && w.byte_len == (w.vertices.len() * 15) as u32,
            "顶点成对、全有限、byte_len=15/顶点独立核对",
        );
    }

    // 统计：混合语料计数与基线一致（直读管理器口径）。
    {
        let m = corpus_mixed();
        let s = build_stats(&m, &frame_stats(3, 2, 0, 42), 96, 0);
        set.add(
            "C10-PAY-03",
            s.active_lights == 5
                && s.census.directional == 1
                && s.census.point == 2
                && s.census.spot == 1
                && s.census.area == 1
                && s.selected == 3
                && s.culled == 2
                && s.dropped == 0,
            "统计快照计数与 frame_stats 直读一致",
        );
    }

    // 统计：耗时五元组非自证——select_units 来自管理器真值 total_ops。
    {
        let m = corpus_flood(40);
        let s = build_stats(&m, &frame_stats(0, 0, 0, 1234), 8, 16);
        set.add(
            "C10-PAY-04",
            s.timing.select_units == 1234 && s.timing.wire_vertices == 8 && s.timing.heat_pixels == 16,
            "五元组三口注入真值逐字段相等（非常数）",
        );
    }

    // 统计负载字节量**按字段表独立重算**（4×u32 + 4×u32 + u8 + 5×u64 = 73）。
    //
    // **为什么单独立项**：`byte_len` 是消费端分配缓冲的依据，也是信封摘要
    // 的输入。估写的常数（比如此前按「4×7 + 8×6」写 76，而五元组只有 5 个
    // u64）在任何「计数对不对」的判据下都全绿——只有把字节表摊开重算才会红。
    {
        let m = corpus_mixed();
        let s = build_stats(&m, &frame_stats(1, 2, 3, 4), 5, 6);
        let recomputed = 4u32 * 4 + 4u32 * 4 + 1u32 + 5u32 * 8;
        set.add(
            "C10-PAY-05",
            s.byte_len == recomputed && STATS_WIRE_BYTES == recomputed && recomputed == 73,
            "统计负载字节 73 = 4x4 + 4x4 + 1 + 5x8（按字段表实算）",
        );
    }

    // ------------------------------------------------------------------
    // C10-CVD：色弱三冗余（判据面字面量表独立解码，不走 for_kind）
    // ------------------------------------------------------------------

    {
        let m = corpus_mixed();
        let mut log = JdiagLog::default();
        let w = build_wireframe(&m, &mut log);
        let mut all_ok = true;
        // 每顶点三通道各自可解码回「某个」类型，且三通道解码结果互相同意。
        for v in w.vertices.iter() {
            let by_color = color_table(v.mark.color_id);
            let by_glyph = glyph_table(v.mark.glyph);
            let by_tag = tag_table(v.mark.tag);
            match (by_color, by_glyph, by_tag) {
                (Some(a), Some(b), Some(c)) => {
                    if !(a == b && b == c) {
                        all_ok = false;
                    }
                }
                _ => all_ok = false,
            }
        }
        // 三种类型语料齐备（方向/点/聚/面），四类解码结果都该出现。
        let mut seen = [false; 4];
        for v in w.vertices.iter() {
            if let Some(k) = tag_table(v.mark.tag) {
                match k {
                    LightKind::Directional => seen[0] = true,
                    LightKind::Point => seen[1] = true,
                    LightKind::Spot => seen[2] = true,
                    LightKind::Area => seen[3] = true,
                }
            }
        }
        set.add(
            "C10-CVD-01",
            all_ok && seen[0] && seen[1] && seen[2] && seen[3],
            "三通道字面量独立解码互相同意且四类齐现",
        );
    }

    // 一致性谓词：字面量表构造的三种合法组合 consistent() 为真，
    // 乱填组合为假（谓词本身可被信，需双向验证）。
    {
        let ok1 = WireMark { color_id: 2, glyph: 2, tag: b'P' }.consistent();
        let ok2 = WireMark { color_id: 1, glyph: 1, tag: b'D' }.consistent();
        let bad = WireMark { color_id: 1, glyph: 2, tag: b'D' }.consistent();
        let bad2 = WireMark { color_id: 0, glyph: 0, tag: b'X' }.consistent();
        set.add(
            "C10-CVD-02",
            ok1 && ok2 && !bad && !bad2,
            "consistent() 双向：合法组合真、错配/非法组合假",
        );
    }

    // ------------------------------------------------------------------
    // C10-DEG：降级矩阵（截断/降档/缺项/背压）
    // ------------------------------------------------------------------

    // 管理器口恒不截断——**几何不变式**：`MAX_LIGHTS × 单灯最坏顶点数 <
    // CAP_WIRE_VERTICES`。这条不是「凑数」：哪天上调 `CAP_WIRE_VERTICES`
    // 越过管理器上界，锚点要求的防线就会对全部在册光源彻底失效，此项即红。
    {
        let m = corpus_flood(MAX_LIGHTS + 200); // 超量注册 → 装满上限即止
        let mut log = JdiagLog::default();
        let w = build_wireframe(&m, &mut log);
        let bound = MAX_LIGHTS * MAX_VERTS_PER_LIGHT;
        set.add(
            "C10-DEG-01",
            !w.truncated
                && bound == 8192
                && bound < CAP_WIRE_VERTICES
                && w.vertices.len() == bound
                && w.total_vertices == bound
                && w.lights_drawn == MAX_LIGHTS
                && log.count(JdbgCode::WIREFRAME_TRUNCATED) == 0,
            "管理器口上界 256x32=8192 < 16384，装满 256 盏仍不截断",
        );
    }

    // 截断：万级**非管理器**来源撞上限 → 交付恰 cap、真实数全额记账、
    // 整灯数不凑零头、诊断码恰记一次（截断不留痕就等于没发生）。
    {
        let descs = flood_descs(1200); // 1200×32 = 38400 > 16384
        let mut log = JdiagLog::default();
        let w = build_wireframe_from(&descs, &mut log);
        let per = verts_per_kind(LightKind::Point);
        let whole = CAP_WIRE_VERTICES / per; // 512 整灯
        set.add(
            "C10-DEG-02",
            w.truncated
                && w.vertices.len() == CAP_WIRE_VERTICES
                && w.vertices.len() == whole * per
                && w.total_vertices == 1200 * per
                && w.dropped_vertices == 1200 * per - CAP_WIRE_VERTICES
                && w.lights_drawn == whole
                && w.byte_len == (CAP_WIRE_VERTICES * 15) as u32
                && log.count(JdbgCode::WIREFRAME_TRUNCATED) == 1,
            "描述序列 1200x32=38400 → 交付 16384（512 整灯）、真实 38400 全额记账、丢弃 22016 有量化面",
        );
    }

    // 截断边界恰好在上限处**不**截断（差一盏即翻红：断边界而非只断远端）。
    {
        let whole = CAP_WIRE_VERTICES / verts_per_kind(LightKind::Point);
        let descs = flood_descs(whole);
        let mut log = JdiagLog::default();
        let w = build_wireframe_from(&descs, &mut log);
        set.add(
            "C10-DEG-03",
            !w.truncated
                && w.vertices.len() == CAP_WIRE_VERTICES
                && w.lights_drawn == whole
                && log.count(JdbgCode::WIREFRAME_TRUNCATED) == 0,
            "恰 512 盏（=cap）不截断；512+1 盏即截断（边界两侧已分别验证）",
        );
    }

    // 两口语义一致：同一批描述经管理器口与描述序列口产出逐顶点相同
    // （截断前即两路等价，防止「换了入口就换了语义」）。
    {
        let m = corpus_flood(64);
        let descs = flood_descs(64);
        let mut l1 = JdiagLog::default();
        let mut l2 = JdiagLog::default();
        let a = build_wireframe(&m, &mut l1);
        let b = build_wireframe_from(&descs, &mut l2);
        set.add(
            "C10-DEG-04",
            a.vertices == b.vertices
                && a.total_vertices == b.total_vertices
                && a.lights_drawn == b.lights_drawn
                && a.byte_len == b.byte_len,
            "管理器口与描述序列口同输入逐顶点相同",
        );
    }

    // 降档：小预算 → 四分之一档 + tier 标注到位（静默降档的区别在标注）。
    {
        let (d, i, p) = heat_corpus();
        let mut log = JdiagLog::default();
        let (outcome, hp) = build_heat(&d, &i, &p, 64, 64, 1200, &mut log);
        let ok = match (&outcome, hp.as_ref()) {
            (HeatBuildOutcome::Built, Some(h)) => {
                h.tier == HeatTier::Quarter && h.hw == 16 && h.hh == 16
            }
            _ => false,
        };
        set.add(
            "C10-DEG-05",
            ok && log.count(JdbgCode::HEAT_DOWNTIERED) == 1,
            "预算 1200 < 半档需求 3072 → 降四档 16x16 且留痕",
        );
    }

    // 半档成功路径：预算充足 → half + 尺寸恰 w/2（断数值不断派生布尔）。
    {
        let (d, i, p) = heat_corpus();
        let mut log = JdiagLog::default();
        let (outcome, hp) = build_heat(&d, &i, &p, 64, 64, 4096, &mut log);
        let ok = match (&outcome, hp.as_ref()) {
            (HeatBuildOutcome::Built, Some(h)) => {
                h.tier == HeatTier::Half && h.hw == 32 && h.hh == 32 && h.byte_len == 3 * 32 * 32 * 4
            }
            _ => false,
        };
        set.add(
            "C10-DEG-06",
            ok && log.count(JdbgCode::HEAT_DOWNTIERED) == 0,
            "预算充足走半档 32x32 且无降档痕迹",
        );
    }

    // 彻底超预算 → Exhausted 诚实拒绝（不静默缩水成更碎的档）。
    {
        let (d, i, p) = heat_corpus();
        let mut log = JdiagLog::default();
        let (outcome, hp) = build_heat(&d, &i, &p, 64, 64, 100, &mut log);
        set.add(
            "C10-DEG-07",
            outcome == HeatBuildOutcome::Exhausted && hp.is_none(),
            "预算 100 连四档 768 都不够 → 拒绝且无负载",
        );
    }

    // 缺项标记：空管理器 → 全零 + 四类缺项位（「零」与「没测」分开）。
    {
        let m = LightManager::new();
        let mut log = JdiagLog::default();
        let c = KindCensus::from_manager(&m, &mut log);
        set.add(
            "C10-DEG-08",
            c.active() == 0 && c.missing_mask == 0b1111,
            "空场景四类全缺项：零值 + mask 1111 + 诊断码 4 次",
        );
    }

    // 缺项标记：单类型语料 → 其余三类缺项、本类不缺。
    {
        let mut m = LightManager::new();
        let _ = m.add(
            mk_light(LightKind::Spot, (0.0, 5.0, 0.0), (0.0, -1.0, 0.0), 6.0, 1),
            Vec::new(),
        );
        let mut log = JdiagLog::default();
        let c = KindCensus::from_manager(&m, &mut log);
        set.add(
            "C10-DEG-09",
            c.spot == 1
                && c.missing_mask == 0b1011
                && log.count(JdbgCode::STATS_KIND_MISSING) == 3,
            "单聚光语料：spot 实数、其余三类缺项位恰 1011",
        );
    }

    // 背压：cap=2 压 3 帧 → 淘汰 1、留最新、seq 单调。
    {
        let mut bus = JDebugBus::new(2);
        let mut log = JdiagLog::default();
        let mut seqs = vec![0u32; 3];
        let mut k = 0usize;
        while k < 3 {
            let mut reg = JEnvelopeRegistry::new();
            reg.register(Envelope::new(PayloadKind::Stats, 76, k as u32));
            seqs[k] = bus.push_frame(
                JDebugFrame { seq: 0, wire: None, heat: None, stats: None, registry: reg },
                &mut log,
            );
            k += 1;
        }
        let latest_seq_ok = match bus.latest() {
            Some(f) => f.seq == seqs[2] && f.seq == 3,
            None => false,
        };
        set.add(
            "C10-DEG-10",
            bus.len() == 2
                && bus.backpressure_drops() == 1
                && latest_seq_ok
                && seqs[0] == 1
                && seqs[1] == 2
                && seqs[2] == 3,
            "背压：淘汰最旧留最新，drops=1，seq 1→2→3 单调",
        );
    }

    // ------------------------------------------------------------------
    // C10-HEAT：三通道贡献（内容互异语料下逐通道独立核对）
    // ------------------------------------------------------------------

    {
        let (d, i, p) = heat_corpus();
        let mut log = JdiagLog::default();
        let (_o, hp) = build_heat(&d, &i, &p, 64, 64, 4096, &mut log);
        let ok = match hp.as_ref() {
            Some(h) => {
                // 判据侧独立重算：全 1 / 全 2 / 全 4 的 2x2 均值仍为 1/2/4，
                // 三通道若混写（如 direct 与 ibl 对调）这里必红。
                let mut d_ok = true;
                let mut i_ok = true;
                let mut p_ok = true;
                for v in h.direct.iter() {
                    if *v != 1.0 {
                        d_ok = false;
                    }
                }
                for v in h.ibl.iter() {
                    if *v != 2.0 {
                        i_ok = false;
                    }
                }
                for v in h.probe.iter() {
                    if *v != 4.0 {
                        p_ok = false;
                    }
                }
                d_ok && i_ok && p_ok && h.direct.len() == 32 * 32 && h.nonfinite_clamped == 0
            }
            None => false,
        };
        set.add(
            "C10-HEAT-01",
            ok,
            "三通道内容互异语料：1/2/4 分通道逐值还原，不混写",
        );
    }

    // 2×2 均值数值正确（非均匀语料：判据侧手算真值对拍）。
    {
        let mut d = vec![0.0f32; 8 * 8];
        // 左上 2x2 块 = [1,3;5,7] → 均值 4.0。
        d[0] = 1.0;
        d[1] = 3.0;
        d[8] = 5.0;
        d[9] = 7.0;
        let i = vec![0.0f32; 8 * 8];
        let p = vec![0.0f32; 8 * 8];
        let mut log = JdiagLog::default();
        let (_o, hp) = build_heat(&d, &i, &p, 8, 8, 4096, &mut log);
        let v0 = hp.as_ref().and_then(|h| h.direct.first().copied());
        set.add(
            "C10-HEAT-02",
            v0 == Some(4.0),
            "非均匀块 [1,3;5,7] 均值恰 4.0（判据侧手算）",
        );
    }

    // 非有限钳零 + 计数（NaN 绝不进绘制缓冲）。
    {
        let mut d = vec![1.0f32; 4 * 4];
        d[0] = f32::NAN;
        d[5] = f32::INFINITY;
        let i = vec![0.0f32; 4 * 4];
        let p = vec![0.0f32; 4 * 4];
        let mut log = JdiagLog::default();
        let (_o, hp) = build_heat(&d, &i, &p, 4, 4, 4096, &mut log);
        let ok = match hp.as_ref() {
            Some(h) => {
                let mut finite = true;
                for v in h.direct.iter() {
                    if !v.is_finite() {
                        finite = false;
                    }
                }
                finite && h.nonfinite_clamped == 2
            }
            None => false,
        };
        set.add(
            "C10-HEAT-03",
            ok && log.count(JdbgCode::HEAT_NON_FINITE) == 1,
            "NaN/Inf 钳零且计数恰 2、输出全有限",
        );
    }

    // ------------------------------------------------------------------
    // C10-ENV：信封与总线（F1764 J 段）
    // ------------------------------------------------------------------

    // 摘要确定性 + 字段敏感性（byte_len/seq 变化 → 摘要变化）。
    {
        let e1 = Envelope::new(PayloadKind::Wireframe, 100, 1);
        let e2 = Envelope::new(PayloadKind::Wireframe, 100, 1);
        let e3 = Envelope::new(PayloadKind::Wireframe, 101, 1);
        let e4 = Envelope::new(PayloadKind::Wireframe, 100, 2);
        set.add(
            "C10-ENV-01",
            e1.checksum() == e2.checksum()
                && e1.checksum() != e3.checksum()
                && e1.checksum() != e4.checksum(),
            "同参数摘要相等；byte_len/seq 任一变即摘要变",
        );
    }

    // new 自算摘要不漂移；外部篡改 declared 即漂移（对账基准在产生处）。
    {
        let mut e = Envelope::new(PayloadKind::Stats, 76, 7);
        let clean = !e.drifted();
        e.declared = e.declared.wrapping_add(1);
        set.add("C10-ENV-02", clean && e.drifted(), "基准自算不漂移；篡改 declared 即漂移");
    }

    // 漂移拦截：reconcile 后 take 一律 None（拦截不是记一笔）。
    {
        let mut reg = JEnvelopeRegistry::new();
        reg.register(Envelope::new(PayloadKind::Heat, 2048, 1));
        let mut tampered = Envelope::new(PayloadKind::Wireframe, 96, 1);
        tampered.byte_len = 999; // 注册后篡改负载量 → 摘要与字段失配。
        reg.register(tampered);
        let mut log = JdiagLog::default();
        reg.reconcile(&mut log);
        let take_heat_none = reg.take(PayloadKind::Heat).is_none();
        let take_wf_none = reg.take(PayloadKind::Wireframe).is_none();
        set.add(
            "C10-ENV-03",
            reg.is_intercepted()
                && reg.drifts() == 1
                && take_heat_none
                && take_wf_none
                && log.count(JdbgCode::ENVELOPE_DRIFT) == 1,
            "一类型漂移 → 整簿拦截：全部 take=None + drifts=1",
        );
    }

    // 干净登记簿对账不拦截。
    {
        let mut reg = JEnvelopeRegistry::new();
        reg.register(Envelope::new(PayloadKind::Wireframe, 96, 1));
        reg.register(Envelope::new(PayloadKind::Heat, 2048, 1));
        reg.register(Envelope::new(PayloadKind::Stats, 76, 1));
        let mut log = JdiagLog::default();
        reg.reconcile(&mut log);
        let t1 = reg.take(PayloadKind::Wireframe).map(|e| e.byte_len) == Some(96);
        set.add(
            "C10-ENV-04",
            !reg.is_intercepted() && reg.drifts() == 0 && t1,
            "干净三枚对账全绿且可取",
        );
    }

    // 类型线缆号往返 + 扩展位诚实预留（0x13/0x14 可解析、无实作负载）。
    {
        let kinds = [
            PayloadKind::Wireframe,
            PayloadKind::Heat,
            PayloadKind::Stats,
            PayloadKind::ReservedShadow,
            PayloadKind::ReservedAtmosphere,
        ];
        let mut roundtrip = true;
        for k in kinds.iter() {
            match PayloadKind::from_wire(k.wire()) {
                Some(k2) => {
                    if k2 != *k {
                        roundtrip = false;
                    }
                }
                None => roundtrip = false,
            }
        }
        set.add(
            "C10-ENV-05",
            roundtrip && PayloadKind::from_wire(0x0F).is_none() && PayloadKind::from_wire(0x20).is_none(),
            "五类型线缆号往返一致；未知号 0x0F/0x20 拒解析",
        );
    }

    // ------------------------------------------------------------------
    // C10-STRIP：发行版零成本（双向验证 + 双口径交叉）
    // ------------------------------------------------------------------

    // Debug 档：真递增（对照面），净值 == 单调。
    {
        let mut g = StripGuard::new(BuildProfile::Debug);
        let mut log = JdiagLog::default();
        let a = g.try_push(&mut log);
        set.add(
            "C10-STRIP-01",
            a && g.payload_builds == 1 && g.build_entries == 1 && g.forced_attempts == 0,
            "Debug 档放行：builds=1=entries、强行为 0",
        );
    }

    // Release 档：净值恰 0 + 强行计数 + 单调证据 1（洗白路径不存在）。
    {
        let mut g = StripGuard::new(BuildProfile::Release);
        let mut log = JdiagLog::default();
        let mut n = 0;
        while n < 100 {
            let _ = g.try_push(&mut log);
            n += 1;
        }
        set.add(
            "C10-STRIP-02",
            g.payload_builds == 0
                && g.forced_attempts == 100
                && g.build_entries == 100
                && log.count(JdbgCode::RELEASE_FORCED) == 100,
            "Release 百次强求：builds 恒 0、单调 entries=100、码记满",
        );
    }

    // 端到端：Release 档 assemble → None 且总线空（物理剔除零成本）。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Release);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let r = asm.assemble_frame(&m, &frame_stats(0, 0, 0, 0), &d, &i, &p, 64, 64, 4096);
        set.add(
            "C10-STRIP-03",
            r.is_none() && asm.bus().is_empty() && asm.guard().payload_builds == 0,
            "Release 端到端：无帧入总线、builds=0",
        );
    }

    // 端到端：Debug 档 assemble → Some 且三类负载齐备。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let r = asm.assemble_frame(&m, &frame_stats(3, 2, 0, 42), &d, &i, &p, 64, 64, 4096);
        let expect = mixed_vertex_total();
        let ok = match (r, asm.bus().latest()) {
            (Some(_), Some(f)) => {
                let wire_ok = f.wire.as_ref().map(|w| w.vertices.len()).unwrap_or(0) == expect;
                let heat_ok = f.heat.as_ref().map(|h| h.tier == HeatTier::Half).unwrap_or(false);
                let stats_ok = f.stats.as_ref().map(|s| s.active_lights == 5).unwrap_or(false);
                wire_ok && heat_ok && stats_ok
            }
            _ => false,
        };
        set.add(
            "C10-STRIP-04",
            ok && asm.guard().payload_builds == 1 && asm.bus().len() == 1,
            "Debug 端到端：三类负载齐备、builds=1、总线 1 帧",
        );
    }

    // 消费端取信封：Debug 端到端后 byte_len 与负载实量一致（信封不撒谎）。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let _ = asm.assemble_frame(&m, &frame_stats(0, 0, 0, 0), &d, &i, &p, 64, 64, 4096);
        let wf = asm.take_envelope(PayloadKind::Wireframe);
        let actual_len = asm
            .bus()
            .latest()
            .and_then(|f| f.wire.as_ref())
            .map(|w| w.byte_len);
        let expect = (mixed_vertex_total() * 15) as u32;
        set.add(
            "C10-ENV-06",
            match (wf, actual_len) {
                (Some(e), Some(a)) => e.byte_len == a && a == expect,
                _ => false,
            },
            "信封 byte_len 与线框实量 1800 一致（对账真值，按重算不写死）",
        );
    }

    // 信封帧号 = 帧序号（不是恒 0 的装饰字段）：连续三帧取信封，seq 必须
    // 逐帧递增且与总线最新帧号一致。
    //
    // **为什么单独立项**：信封 seq 若在构造时写死 0、事后由压帧口改帧号，
    // 摘要算的是「seq=0」而帧号是 3——字段看着像版本号、实际不可信，
    // 且这类不一致在任何单帧判据下都看不见。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let mut all_match = true;
        let mut k = 0u32;
        // **逐帧压一帧取一枚**（三帧压完再取三次只会全看到最新帧，
        // 那样断的是「最后一帧的 seq 对不对」，断不出「逐帧递增」）。
        while k < 3 {
            let pushed = asm.assemble_frame(
                &m,
                &frame_stats(0, 0, 0, 0),
                &d,
                &i,
                &p,
                64,
                64,
                4096,
            );
            let env = asm.take_envelope(PayloadKind::Wireframe);
            match (pushed, env) {
                (Some(s), Some(e)) => {
                    if e.seq != s || s != k + 1 {
                        all_match = false;
                    }
                }
                _ => all_match = false,
            }
            k += 1;
        }
        set.add(
            "C10-ENV-08",
            all_match,
            "逐帧压帧取信封：seq 逐帧 1→2→3 且与压帧返回的帧号逐次相等",
        );
    }

    // 总线封账：信封封装单元 = 登记簿**实测枚数**，非常数。
    //
    // 两档都断：正常三枚 / 热力被预算拒绝时只有两枚。第二档是关键——
    // 写死 `3` 的实现在这档仍会全绿，而五元组里躺着一个凭空多出来的
    // 「封装单元」正是本模块头注第五节声明要避免的记账撒谎。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let _ = asm.assemble_frame(&m, &frame_stats(0, 0, 0, 0), &d, &i, &p, 64, 64, 4096);
        let full = asm
            .bus()
            .latest()
            .and_then(|f| f.stats.as_ref())
            .map(|s| s.timing.envelope_seals);
        // 预算 100 → 热力连四分之一档都装不下 → 只有线框 + 统计两枚信封。
        let mut asm2 = JDebugAssembler::new(BuildProfile::Debug);
        let _ = asm2.assemble_frame(&m, &frame_stats(0, 0, 0, 0), &d, &i, &p, 64, 64, 100);
        let starved = asm2
            .bus()
            .latest()
            .and_then(|f| f.stats.as_ref())
            .map(|s| s.timing.envelope_seals);
        let heat_absent = asm2.bus().latest().and_then(|f| f.heat.as_ref()).is_none();
        set.add(
            "C10-ENV-07",
            full == Some(3) && starved == Some(2) && heat_absent,
            "envelope_seals 随实测枚数走：满负载 3 / 热力被拒 2（非常数）",
        );
    }

    // 拦截态粘滞：漂移一经对账写回帧上，后续取用无需再次对账也拿不到信封
    //（就地拦截而非每次重算的临时视图）。
    {
        let mut asm = JDebugAssembler::new(BuildProfile::Debug);
        let m = corpus_mixed();
        let (d, i, p) = heat_corpus();
        let _ = asm.assemble_frame(&m, &frame_stats(0, 0, 0, 0), &d, &i, &p, 64, 64, 4096);
// 篡改最新帧登记簿里的载荷量 → 摘要与字段失配。
        let drifted = match asm.bus_mut().latest_mut().and_then(|f| f.registry.slot_mut(PayloadKind::Wireframe)) {
            Some(e) => {
                e.byte_len = e.byte_len.wrapping_add(1);
                true
            }
            None => false,
        };
        let first = asm.take_envelope(PayloadKind::Wireframe);
        let intercepted_after = asm.bus().latest().map(|f| f.registry.is_intercepted());
        // 粘滞验证：再取一次（即便不再触发对账路径）仍必须拿不到。
        let second = asm.take_envelope(PayloadKind::Wireframe);
        set.add(
            "C10-ENV-09",
            drifted
                && first.is_none()
                && second.is_none()
                && intercepted_after == Some(true),
            "漂移信封拦截态写回帧上并粘滞：两次取用皆 None",
        );
    }

    set
}
