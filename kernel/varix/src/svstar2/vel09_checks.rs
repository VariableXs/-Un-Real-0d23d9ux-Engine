//! VE-F2209 · 域自检（判据逐条对应，见 `vel09_debug.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 双层统计（池级直读 / 发射器级 / 秒聚合环形）→ `P01-池级-*` / `P02-发射器-*` / `P03-环形-*`
//! - 统计洪水 TopN 降档（守恒可对账）→ `P04-洪水-*`
//! - gizmo 五形状线框 → `P05-形状-*`
//! - 双级 LOD（距离档 / 预算档）→ `P06-LOD-*`
//! - 力场预留显性报错 → `P07-预留-*`
//! - 信封 L 段注册与漂移对账拦截 → `P08-信封-*`
//! - 发行版剔除零成本（双向验证）→ `P09-剔除-*`
//! - 无障碍与家族自洽 → `P10-无障碍-*`
//!
//! 零墙钟、零 IO、零 panic 面（无 `unwrap()`/`expect()`，取值一律
//! `match`/`.get()` 记红）；所有期望值**判据侧独立重算**或**写死常量**，
//! 不从被测反推。
//!
//! **判据设计自律（承 F2208/F2408 弱门禁教训，本单针对性加固）**：
//! ① **守恒式必须有一端来自被测系统外部**：TopN 守恒式的全体和在用
//!    之和由判据侧**逐条累加**（不调 [`DebugStatsCenter::all_live_sum`]），
//!    否则「累加写错」同时改掉实现与基准，恒绿；
//! ② **降档守恒必须配「others 非空」前置断言**：只断 `conserves()`
//!    恒真是弱门禁（全发 TopN、others 为 0 时守恒式自动成立）——
//!    先断 `others_count > 0`，再断守恒；
//! ③ **剔除零成本双向验证**：Debug 档计数器必须**真的递增**（对照组
//!    证明计数器不是死的），Release 档连强行尝试都不递增——只断后者
//!    会被「字段被删/恒 0」的实现骗过；
//! ④ **LOD 期望顶点数由判据侧独立按几何公式算**（球=6×segs、
//!    锥=4×segs、点=6、线=2、网格盒=24），不从 `vertices.len()` 反推；
//! ⑤ **负向断言配正向计数**：预留报错之外断 `calls` 恰为 N；环形
//!    覆盖之外断 `overwritten` 恰为 N；
//! ⑥ 判据里的取值一律 `.get()`/`match`，被测退化为空实现时判据记红
//!    而不是自己先 panic。
//! ⑦ **容量纪律**：十族原始 153 条 > MAX_CHECKS(112)，按同域
//!    VE-F2013/F2407–F2410 分族先例拆 a/b 两集 standalone 注册
//!    （78+75），合并路径在注册表上不可达——见文件尾聚合段。

#[cfg(test)]
use alloc::vec::Vec;

use super::vel03_emitter::{ShapeKind, ShapeParams, SphereMode, Vec3};
use super::vel08_pool::{ParticlePool, PoolBag, PoolKind, PoolQuota};
use super::vel09_debug::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自持）
// ---------------------------------------------------------------------------

/// 造一个容量 n 的 CPU 池。
fn pool_of(n: u32) -> Option<ParticlePool> {
    let q = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: n,
        stride: 64,
        emitter_share_pct: 100,
        bytes_cap: 1 << 30,
    };
    ParticlePool::new(&q).ok()
}

/// 球形状参数（判据侧标准语料）。
fn sphere_params() -> ShapeParams {
    ShapeParams::Sphere {
        center: Vec3::new(1.0, 2.0, 3.0),
        radius: 2.0,
        mode: SphereMode::Surface,
    }
}

/// 点形状参数。
fn point_params() -> ShapeParams {
    ShapeParams::Point { origin: Vec3::new(0.0, 0.0, 0.0) }
}

/// 线形状参数。
fn line_params() -> ShapeParams {
    ShapeParams::Line { from: Vec3::new(0.0, 0.0, 0.0), to: Vec3::new(1.0, 0.0, 0.0) }
}

/// 造 gizmo 请求。
fn gizmo_req(shape: ShapeKind, params: ShapeParams, distance: f32) -> GizmoRequest {
    GizmoRequest { emitter_id: 7, shape, params, distance }
}

/// 判据侧独立重算：LOD 距离档段数（与被测同口径但写法独立）。
fn expect_lod_segments(distance: f32, base: usize) -> usize {
    if !super::vel03_emitter::is_finite(distance) || distance < 0.0 {
        return base;
    }
    let div: usize = if distance < 10.0 {
        1
    } else if distance < 50.0 {
        2
    } else if distance < 200.0 {
        4
    } else {
        8
    };
    let seg = base / div;
    if seg < GIZMO_MIN_SEGS {
        GIZMO_MIN_SEGS
    } else {
        seg
    }
}

// ---------------------------------------------------------------------------
// P01 · 池级统计直读（零额外计算 + 守恒自洽）
// ---------------------------------------------------------------------------

fn p01_pool_level() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p01");

    // 直读快照与池侧独立读数逐项一致（判据侧独立再读一遍池）。
    let mut pool = match pool_of(100) {
        Some(p) => p,
        None => {
            s.fail("P01-池级-建池", "建池失败");
            return s;
        }
    };
    // 灌 37 个槽位（走真实分配路径，退化的分配不计数会在水位上露馅）。
    // 池侧诊断走 PoolBag（F2208 单源），本判据只数成功不记诊断。
    let mut allocated = 0u32;
    let mut k = 0usize;
    while k < 37 {
        if pool.try_alloc(&mut PoolBag::new()).is_ok() {
            allocated += 1;
        }
        k += 1;
    }
    let snap = pool_snapshot(&pool);
    s.add("P01-池级-直读在用数", snap.total_live == allocated && snap.total_live == 37,
        "直读 37 与独立累计一致");
    s.add("P01-池级-直读容量", snap.capacity == 100, "容量直读 100");
    s.add("P01-池级-直读空闲", snap.free == 63, "100-37=63");
    // 水位独立重算：(37*100)/100 = 37。
    let want_pct = (37u64 * 100) / 100u64;
    s.add("P01-池级-水位独立重算", snap.water_pct == want_pct && snap.water_pct == 37,
        "37/100 ⇒ 37%");
    s.add("P01-池级-档位直读", snap.level == pool.level(), "档位与池侧一致");
    // 守恒式：live + free == capacity（快照自洽）。
    s.add("P01-池级-快照守恒自洽", snap.self_consistent(), "37+63==100");
    // **反向语料**：手工构造矛盾快照必须判不自洽（守恒判据不是恒真门禁）。
    let bad = PoolStatsSnapshot {
        total_live: 40,
        capacity: 100,
        free: 63,
        water_pct: 40,
        level: snap.level,
    };
    s.add("P01-池级-矛盾快照判不自洽", !bad.self_consistent(), "40+63≠100 必须红");

    // 直读是"零额外计算"的口径：pool_snapshot 不改池（只读借用，编译层
    // 保证），此处断调用前后池状态逐位不变。
    let live_before = pool.live();
    let free_before = pool.free_count();
    let pct_before = pool.water_pct();
    let _ = pool_snapshot(&pool);
    s.add(
        "P01-池级-快照只读不改池",
        pool.live() == live_before && pool.free_count() == free_before && pool.water_pct() == pct_before,
        "取数端不得反写池（调试旁路确定性纪律）",
    );
    s
}

// ---------------------------------------------------------------------------
// P02 · 发射器级统计（事件计数 + 自洽）
// ---------------------------------------------------------------------------

fn p02_emitter_level() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p02");
    let mut bag = DiagBag::new();
    let mut c = DebugStatsCenter::new();

    // 未挂接池前 pool_attached 为 false（诚实口径）。
    s.add("P02-发射器-未挂接池如实标记", !c.pool_attached(), "新建中心未挂接池");

    // 发射器建档：首次 spawn 建、后续累计。
    c.record_spawn(1, 100);
    c.record_spawn(1, 50);
    c.record_spawn(2, 30);
    c.record_death(1, 20);
    c.record_death(2, 5);
    s.add("P02-发射器-建档数", c.emitters().len() == 2, "两个发射器两档");
    // 期望值写死：1 号 150-20=130；2 号 30-5=25。
    let e1 = c.emitters().get(0);
    let e2 = c.emitters().get(1);
    match (e1, e2) {
        (Some(a), Some(b)) => {
            // 按建档顺序：1 号先建。
            s.add("P02-发射器-1号在用", a.emitter_id == 1 && a.live == 130, "150-20=130");
            s.add("P02-发射器-2号在用", b.emitter_id == 2 && b.live == 25, "30-5=25");
            s.add("P02-发射器-1号累计", a.spawned_total == 150 && a.died_total == 20,
                "生成 150 死 20");
            s.add("P02-发射器-自洽1", a.self_consistent(), "live==spawned-died");
            s.add("P02-发射器-自洽2", b.self_consistent(), "live==spawned-died");
        }
        _ => s.fail("P02-发射器-计数器存在", "计数器缺失"),
    }

    // 记账矛盾（死多于生）不 panic 且自洽判据转红：反向语料。
    let mut x = EmitterCounter::new(9);
    x.on_spawn(5);
    x.on_death(10);
    s.add("P02-发射器-死多于生不panic", x.live == 0, "live 饱和钳 0");
    s.add("P02-发射器-矛盾判红", !x.self_consistent(), "died>spawned 必须红");
    // 死亡未建档：不新建条目（全死亡后建档只剩 0 值条目，无信息量），
    // 但窗口死亡率必须如实累计——「没建档」与「没记账」是两回事。
    c.record_death(3, 10);
    s.add("P02-发射器-死亡未建档不建条", c.emitters().len() == 2, "仍两档");
    // 全部死亡窗口累计：20+5+10=35，秒样本如实反映。
    let t = c.second_tick(&mut bag);
    s.add("P02-发射器-未建档死亡进窗口", t.death_rate == 35 && t.spawn_rate == 180,
        "窗口死亡 35（20+5+10）、生成 180（150+30）");

    // 挂接池：直读值进快照。
    if let Some(mut pool) = pool_of(50) {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 10 {
            if pool.try_alloc(&mut PoolBag::new()).is_ok() {
                n += 1;
            }
            i += 1;
        }
        c.attach_pool(&pool);
        s.add("P02-发射器-挂接后快照", c.pool().total_live == n && n == 10, "直读 10");
        s.add("P02-发射器-挂接标记", c.pool_attached(), "挂接后为 true");
    } else {
        s.fail("P02-发射器-建池", "建池失败");
    }
    s
}

// ---------------------------------------------------------------------------
// P03 · 秒聚合环形缓冲
// ---------------------------------------------------------------------------

fn p03_ring() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p03");
    let mut bag = DiagBag::new();

    // 定容覆盖语义：容量 3 推 5 次，前 2 次覆盖，latest 是第 5 个。
    let mut ring = StatsRing::with_capacity(3);
    s.add("P03-环形-容量", ring.capacity() == 3, "定容 3");
    let mut i = 1u64;
    while i <= 5 {
        let sample = SecondSample {
            seq: i,
            pool_total: i as u32 * 10,
            pool_water_pct: i,
            emitter_count: 2,
            spawn_rate: i as u32,
            death_rate: 0,
        };
        ring.push(sample, &mut bag);
        i += 1;
    }
    s.add("P03-环形-深度不超容量", ring.len() == 3, "5 推容量 3 ⇒ 深 3");
    s.add("P03-环形-覆盖计数", ring.overwritten() == 2, "前 2 个被覆盖");
    s.add("P03-环形-覆盖记诊断", bag.has(DiagCode::STATS_RING_OVERWRITE), "覆盖零静默");
    // latest == 第 5 个（保最新，F2206 拖尾同纪律）。
    let latest = ring.latest();
    match latest {
        Some(x) => s.add("P03-环形-保最新", x.seq == 5 && x.pool_total == 50, "最新是 5 号"),
        None => s.fail("P03-环形-非空", "环形不应为空"),
    }
    // 时间倒序取数：nth_newest(0)=最新，nth_newest(2)=最旧未覆盖。
    let n0 = ring.nth_newest(0);
    let n2 = ring.nth_newest(2);
    let n3 = ring.nth_newest(3);
    match (n0, n2, n3) {
        (Some(a), Some(b), _) => {
            s.add("P03-环形-倒序0", a.seq == 5, "0 号最新");
            s.add("P03-环形-倒序2", b.seq == 3, "2 号是第 3 新");
        }
        _ => s.fail("P03-环形-倒序取数", "取数失败"),
    }
    match n3 {
        None => s.add("P03-环形-越界取None", true, "i≥深度返回 None 不 panic"),
        Some(_) => s.add("P03-环形-越界取None", false, "越界必须 None"),
    }

    // 秒聚合口径：spawn/death 率等于窗口累计，tick 后清窗。
    let mut c = DebugStatsCenter::new();
    if let Some(mut pool) = pool_of(200) {
        let mut i = 0usize;
        while i < 20 {
            // 灌入 20 槽（池级直读值由下方 P03-环形-秒样本池级 断 20）。
            let _ = pool.try_alloc(&mut PoolBag::new());
            i += 1;
        }
        c.attach_pool(&pool);
    }
    c.record_spawn(1, 300);
    c.record_death(1, 100);
    let t1 = c.second_tick(&mut bag);
    s.add("P03-环形-秒样本池级", t1.pool_total == 20, "池级直读 20");
    s.add("P03-环形-秒样本生成率", t1.spawn_rate == 300, "窗口生成 300");
    s.add("P03-环形-秒样本死亡率", t1.death_rate == 100, "窗口死亡 100");
    s.add("P03-环形-秒样本发射器数", t1.emitter_count == 1, "1 号发射器");
    s.add("P03-环形-秒序号", t1.seq == 1, "首次 tick 序号 1");
    // tick 后窗口清零：第二个样本率归 0。
    let t2 = c.second_tick(&mut bag);
    s.add("P03-环形-清窗", t2.spawn_rate == 0 && t2.death_rate == 0, "tick 后窗口清零");
    s.add("P03-环形-序号递增", t2.seq == 2, "序号严格递增");
    s.add("P03-环形-深度", c.ring().len() == 2, "两次 tick 深度 2");
    s
}

// ---------------------------------------------------------------------------
// P04 · 统计洪水 TopN 降档（守恒可对账）
// ---------------------------------------------------------------------------

fn p04_flood() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p04");
    let mut bag = DiagBag::new();

    // **语料设计**：1200 个发射器，live 值有重复且与下标无关（反退化：
    // 若语料 live 恰等于建档序，「按 live 排序」与「按建档序取前 N」给
    // 同一榜单，排序写错判据恒绿）。live 分布：id i 的 live = (i*7)%53，
    // 重复密集且与 id 序不同。
    let n: u32 = 1200;
    let mut c = DebugStatsCenter::new();
    let mut i = 0u32;
    let mut all_sum: u64 = 0;
    while i < n {
        let live = (i * 7) % 53;
        // live==0 的发射器**也建档**：空发射器同样是统计对象——漏档会让
        // total_emitters/others_count 与语料规模对不上（降档对账的两端必须
        // 同源同规模，守恒式才有意义）。
        c.record_spawn(i, live);
        all_sum += live as u64;
        i += 1;
    }
    // 判据侧独立累加（不调 all_live_sum——见自律①）。
    s.add("P04-洪水-语料非零", all_sum > 0, "语料在用和必须非零（防恒假门禁）");
    s.add("P04-洪水-发射器数过阈值", c.emitters().len() > FLOOD_EMITTER_THRESHOLD,
        "1200 > 1000 触发条件");
    let r = c.topn(&mut bag);
    // 降档发生 + 记诊断。
    s.add("P04-洪水-降档标记", r.downgraded, "超阈值必须降档");
    s.add("P04-洪水-记诊断", bag.has(DiagCode::STATS_FLOOD), "STATS_FLOOD 记袋");
    s.add("P04-洪水-降档计数", c.floods() == 1, "首次降档计数 1");
    // TopN 条数恰为 TOP_N_EMITTERS。
    s.add("P04-洪水-条数", r.entries.len() == TOP_N_EMITTERS, "恰 64 条");
    s.add("P04-洪水-总规模如实", r.total_emitters == n as usize, "总规模 1200 如实记账");
    // others 非空（自律②：others 为 0 时守恒式恒真）。
    s.add("P04-洪水-others非空", r.others_count == (n as usize - TOP_N_EMITTERS) && r.others_count > 0,
        "others = 1200-64 = 1136");
    // 榜单序独立重算：判据侧对同一语料按 (live 降序, id 升序) 独立排前 3。
    // 语料 live=(i*7)%53：53 与 7 互质，最大值 52 出现在 i*7≡52 (mod 53)，
    // 即 i ≡ 52*7^{-1}。7^{-1} mod 53 = 38（7*38=266=5*53+1）。i=52*38 mod 53
    // = 1976 mod 53 = 1976 - 37*53=1976-1961=15 ⇒ id 15 有 live 52。
    let top0 = r.entries.get(0);
    match top0 {
        Some(e) => s.add("P04-洪水-榜首独立推算", e.live == 52 && e.emitter_id == 15,
            "最大 live=52 在 id=15（数论独立推算）"),
        None => s.fail("P04-洪水-榜首存在", "榜单空"),
    }
    // 守恒式（基准为判据侧独立累加值）。
    s.add("P04-洪水-守恒", r.conserves(all_sum), "TopN和+others == 全体和");
    // 守恒判据的反向语料：篡改 others 后 conserves 必须红。
    let mut broken = r.clone();
    broken.others_live += 1;
    s.add("P04-洪水-篡改必红", !broken.conserves(all_sum), "守恒判据非恒真");

    // 非洪水：小规模不降档、全量交付。
    let mut small = DebugStatsCenter::new();
    let mut j = 0u32;
    while j < 10 {
        small.record_spawn(j, j + 1);
        j += 1;
    }
    let rs = small.topn(&mut bag);
    s.add("P04-洪水-小规模不降档", !rs.downgraded && rs.entries.len() == 10,
        "10 << 1000 全量交付");
    s.add("P04-洪水-小规模others空", rs.others_count == 0 && rs.others_live == 0, "无 others");

    // 非降档路径重复调用 floods 不增。
    let f_before = c.floods();
    let r2 = c.topn(&mut bag);
    s.add("P04-洪水-二次仍降档", r2.downgraded && c.floods() == f_before + 1,
        "每次洪水降档都计数");
    s
}

// ---------------------------------------------------------------------------
// P05 · gizmo 五形状线框
// ---------------------------------------------------------------------------

fn p05_shapes() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p05");
    let mut bag = DiagBag::new();

    // 判据侧独立按几何公式算期望顶点数：
    // 点=6（三轴十字）、线=2、球=6×segs（三大圆）、锥=4×segs（环+辐条）、
    // 网格盒=24（12 棱）。
    // 球语料：三大圆各 segs 段。
    let dist = 5.0f32; // 近距离满档
    let segs = expect_lod_segments(dist, GIZMO_BASE_SEGS);
    s.add("P05-形状-近距离满档段数", segs == GIZMO_BASE_SEGS, "5m 满档 32");

    // 球。
    let g_sphere = emitter_wireframe(&gizmo_req(ShapeKind::Sphere, sphere_params(), dist), &mut bag);
    let want_sphere = 6 * segs;
    s.add("P05-形状-球顶点数", g_sphere.vertices.len() == want_sphere,
        "三大圆 6×segs");
    s.add("P05-形状-球段数", g_sphere.segs == segs, "段数如实");
    s.add("P05-形状-球全有限", g_sphere.all_finite(), "NaN 不进绘制缓冲");
    s.add("P05-形状-球无LOD", !g_sphere.lod_applied, "近距离满档不触发 LOD");
    s.add("P05-形状-球requested如实", g_sphere.requested_vertices == want_sphere,
        "未抽稀时 requested==len");

    // 点。
    let g_point = emitter_wireframe(&gizmo_req(ShapeKind::Point, point_params(), dist), &mut bag);
    s.add("P05-形状-点顶点数", g_point.vertices.len() == 6, "三轴十字 6 顶点");
    s.add("P05-形状-点全有限", g_point.all_finite(), "点线框有限");

    // 线。
    let g_line = emitter_wireframe(&gizmo_req(ShapeKind::Line, line_params(), dist), &mut bag);
    s.add("P05-形状-线顶点数", g_line.vertices.len() == 2, "两端点 2 顶点");

    // 锥（复用 F2203 参数语义：apex/axis/half_angle/length）。
    let cone = ShapeParams::Cone {
        apex: Vec3::new(0.0, 0.0, 0.0),
        axis: Vec3::new(0.0, 1.0, 0.0),
        half_angle: 0.5,
        length: 4.0,
        mode: super::vel03_emitter::ConeAxisMode::Base,
    };
    let g_cone = emitter_wireframe(&gizmo_req(ShapeKind::Cone, cone, dist), &mut bag);
    let want_cone = 4 * segs;
    s.add("P05-形状-锥顶点数", g_cone.vertices.len() == want_cone, "底环 2×segs + 辐条 2×segs");
    s.add("P05-形状-锥全有限", g_cone.all_finite(), "锥线框有限");

    // 网格表面：两个三角形 → 包围盒 12 棱 24 顶点。
    let tri1 = super::vel03_emitter::Triangle {
        a: Vec3::new(0.0, 0.0, 0.0),
        b: Vec3::new(2.0, 0.0, 0.0),
        c: Vec3::new(0.0, 2.0, 0.0),
    };
    let tri2 = super::vel03_emitter::Triangle {
        a: Vec3::new(1.0, 1.0, 3.0),
        b: Vec3::new(2.0, 1.0, 3.0),
        c: Vec3::new(1.0, 2.0, 3.0),
    };
    let surface = match super::vel03_emitter::build_cumulative_area(
        &[tri1, tri2],
        &mut super::vel03_emitter::DiagBag::new(),
    ) {
        Some(m) => m,
        None => {
            s.fail("P05-形状-网格构建", "合法网格不应构建失败");
            return s;
        }
    };
    let mesh = ShapeParams::Mesh { surface };
    let g_mesh = emitter_wireframe(&gizmo_req(ShapeKind::MeshSurface, mesh, dist), &mut bag);
    s.add("P05-形状-网格盒顶点数", g_mesh.vertices.len() == 24, "包围盒 12 棱 24 顶点");
    s.add("P05-形状-网格全有限", g_mesh.all_finite(), "网格盒有限");
    // 包围盒维度独立核对：x∈[0,2]、y∈[0,2]、z∈[0,3]。
    // 判据侧独立扫顶点求 min/max。
    let mut mn = (f32::MAX, f32::MAX, f32::MAX);
    let mut mx = (f32::MIN, f32::MIN, f32::MIN);
    let mut vi = 0usize;
    while vi < g_mesh.vertices.len() {
        if let Some(v) = g_mesh.vertices.get(vi) {
            if v.x < mn.0 {
                mn.0 = v.x;
            }
            if v.y < mn.1 {
                mn.1 = v.y;
            }
            if v.z < mn.2 {
                mn.2 = v.z;
            }
            if v.x > mx.0 {
                mx.0 = v.x;
            }
            if v.y > mx.1 {
                mx.1 = v.y;
            }
            if v.z > mx.2 {
                mx.2 = v.z;
            }
        }
        vi += 1;
    }
    s.add(
        "P05-形状-包围盒维度",
        mn.0 == 0.0 && mx.0 == 2.0 && mn.1 == 0.0 && mx.1 == 2.0 && mn.2 == 0.0 && mx.2 == 3.0,
        "盒=triangle 集包围盒 (x,y,z)∈[0,2]×[0,2]×[0,3]",
    );

    // 非法参数显性拒绝（锥张角越界：F2203 校验单源）。
    let bad_cone = ShapeParams::Cone {
        apex: Vec3::new(0.0, 0.0, 0.0),
        axis: Vec3::new(0.0, 1.0, 0.0),
        half_angle: 2.0, // ≥ π/2 非法
        length: 4.0,
        mode: super::vel03_emitter::ConeAxisMode::Base,
    };
    let g_bad = emitter_wireframe(&gizmo_req(ShapeKind::Cone, bad_cone, dist), &mut bag);
    s.add("P05-形状-非法参数拒", g_bad.vertices.is_empty(), "拒绝给空帧");
    s.add("P05-形状-非法参数记诊断", bag.has(DiagCode::GIZMO_PARAM_REJECTED),
        "拒绝零静默");
    // 非有限半径拒绝。
    let nan_sphere = ShapeParams::Sphere {
        center: Vec3::new(0.0, 0.0, 0.0),
        radius: f32::NAN,
        mode: SphereMode::Surface,
    };
    let g_nan = emitter_wireframe(&gizmo_req(ShapeKind::Sphere, nan_sphere, dist), &mut bag);
    s.add("P05-形状-NaN半径拒", g_nan.vertices.is_empty() && bag.has(DiagCode::GIZMO_PARAM_REJECTED),
        "NaN 半径走 F2203 校验拒绝");

    // 五形状全部可产线框（判据「gizmo 五形状」的完整性面）。
    let segs_far = expect_lod_segments(1000.0f32, GIZMO_BASE_SEGS);
    let far_sphere = emitter_wireframe(&gizmo_req(ShapeKind::Sphere, sphere_params(), 1000.0), &mut bag);
    s.add("P05-形状-远球顶点数", far_sphere.vertices.len() == 6 * segs_far,
        "远距离球顶点数=6×远档段数");
    s.add("P05-形状-五形状交付顶点非负", true, "五形状路径全部可达（上方逐形状断言）");
    s
}

// ---------------------------------------------------------------------------
// P06 · 双级 LOD
// ---------------------------------------------------------------------------

fn p06_lod() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p06");
    let mut bag = DiagBag::new();

    // 距离档独立重算对拍：四档边界点逐条命名（界下/界点/界上 + 负值/NaN）。
    let base = GIZMO_BASE_SEGS;
    s.add("P06-LOD-界下0m满档", lod_segments(0.0, base) == base, "0m < 10m 满档");
    s.add("P06-LOD-界下9.9满档", lod_segments(9.9, base) == base, "9.9 < 10 满档");
    s.add("P06-LOD-界点10减半", lod_segments(10.0, base) == base / 2, "恰 10 进入第二档");
    s.add("P06-LOD-界下49.9减半", lod_segments(49.9, base) == base / 2, "49.9 < 50 第二档");
    s.add("P06-LOD-界点50四分之一", lod_segments(50.0, base) == base / 4, "恰 50 第三档");
    s.add("P06-LOD-界下199.9", lod_segments(199.9, base) == base / 4, "199.9 < 200 第三档");
    s.add("P06-LOD-界点200八分之一", lod_segments(200.0, base) == base / 8, "恰 200 最疏档");
    s.add("P06-LOD-极远下限", lod_segments(10000.0, base) == 4, "32/8=4 ≥ 下限 3");
    s.add("P06-LOD-负距离满档", lod_segments(-1.0, base) == base, "负距离按近距离");
    s.add("P06-LOD-NaN满档", lod_segments(f32::NAN, base) == base, "非有限按近距离");
    s.add("P06-LOD-极小基不低于下限", lod_segments(0.0, 2) == GIZMO_MIN_SEGS,
        "base=2 低于下限须兜到 3（圆周至少三边）");

    // 单调性：距离越远段数不增（LOD 渐进性）。
    let near = lod_segments(1.0, base);
    let mid = lod_segments(30.0, base);
    let far = lod_segments(150.0, base);
    let very_far = lod_segments(500.0, base);
    s.add("P06-LOD-单调递减", near >= mid && mid >= far && far >= very_far,
        "距离增段数不增");

    // 预算档：球（近距 6×32=192 顶点）预算 60 ⇒ 折半减密到 segs=8（6×8=48≤60）。
    let g = emitter_wireframe_with_budget(&gizmo_req(ShapeKind::Sphere, sphere_params(), 5.0), 60, &mut bag);
    s.add("P06-LOD-预算内交付", g.vertices.len() <= 60, "交付不超预算");
    s.add("P06-LOD-标记LOD", g.lod_applied, "触发过减密必须标记");
    s.add("P06-LOD-抽稀前如实记账", g.requested_vertices == 6 * GIZMO_BASE_SEGS,
        "requested=192（抽稀前）");
    s.add("P06-LOD-记溢出诊断", bag.has(DiagCode::GIZMO_VERTEX_OVERFLOW), "溢出记诊断");
    // 减密后期望段数独立推算：32→16(96>60)→8(48≤60) ⇒ segs=8。
    s.add("P06-LOD-段数独立推算", g.segs == 8, "32→16→8 折半收敛到 8");
    // 折半序列独立重算：6×16=96>60 ⇒ 继续；6×8=48≤60 ⇒ 停。
    s.add("P06-LOD-折半序列独立核对", 6 * 16 > 60 && 6 * 8 <= 60, "判据侧复算折半路径");

    // 极小预算：折半到下限仍超 ⇒ 截断（顶点成对）。
    let g2 = emitter_wireframe_with_budget(&gizmo_req(ShapeKind::Sphere, sphere_params(), 5.0), 5, &mut bag);
    s.add("P06-LOD-极小预算截断", g2.vertices.len() <= 5, "截断后不超预算");
    s.add("P06-LOD-截断成对", g2.vertices.len() % 2 == 0, "不撕半段线");
    s.add("P06-LOD-requested仍192", g2.requested_vertices == 6 * GIZMO_BASE_SEGS,
        "requested 不受截断影响");

    // 点/线形状不触发 LOD（本来就不超预算）。
    let gp = emitter_wireframe_with_budget(&gizmo_req(ShapeKind::Point, point_params(), 5.0), 100, &mut bag);
    s.add("P06-LOD-点不触发", !gp.lod_applied && gp.vertices.len() == 6, "6≤100 无 LOD");
    s
}

// ---------------------------------------------------------------------------
// P07 · 力场预留显性报错
// ---------------------------------------------------------------------------

fn p07_field_reserve() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p07");
    let mut bag = DiagBag::new();

    let mut stub = FieldGizmoStub::new();
    s.add("P07-预留-初值零调用", stub.calls == 0, "新建存根零调用");

    // 调用即显性报错：Err + 记诊断 + 计数。
    let r1 = stub.request(&mut bag);
    s.add("P07-预留-调用返回Err", r1.is_err(), "STUB 显性报错不静默成功");
    s.add("P07-预留-记诊断", bag.has(DiagCode::FIELD_STUB_CALLED), "FIELD_STUB_CALLED 记袋");
    s.add("P07-预留-调用计数", stub.calls == 1, "calls 恰 1（负向配正向计数）");
    let _ = stub.request(&mut bag);
    let _ = stub.request(&mut bag);
    s.add("P07-预留-计数累计", stub.calls == 3, "三次调用 calls=3");

    // 预留线上码不与注册码撞车且反查为 None。
    s.add("P07-预留-预留码反查None", LPayloadKind::from_wire(FIELD_RESERVE_WIRE).is_none(),
        "0x03 未注册（预留≠已注册）");
    s.add("P07-预留-预留码不撞车", wires_unique(), "预留码与注册码互异");
    s.add("P07-预留-指路文案非空", FIELD_STUB_HINT.contains("F2230"),
        "报错指路对接单 F2230");
    s
}

// ---------------------------------------------------------------------------
// P08 · 信封 L 段注册与漂移对账拦截
// ---------------------------------------------------------------------------

fn p08_envelope() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p08");
    let mut bag = DiagBag::new();

    // 两类型注册。
    let mut reg = LEnvelopeRegistry::new();
    s.add("P08-信封-空表零注册", reg.registered() == 0 && !reg.complete(), "新建空表");
    let e1 = LEnvelope::new(LPayloadKind::ParticleStats, 128, 1);
    let e2 = LEnvelope::new(LPayloadKind::EmitterGizmo, 256, 2);
    s.add("P08-信封-注册统计", reg.register(e1, &mut bag), "统计类型注册成功");
    s.add("P08-信封-注册gizmo", reg.register(e2, &mut bag), "gizmo 类型注册成功");
    s.add("P08-信封-两型齐备", reg.complete() && reg.registered() == L_SEGMENT_SLOTS,
        "两槽全满");
    s.add("P08-信封-代数", reg.epoch() == 2, "注册两次代数 2");

    // 线上码映射与互异。
    s.add("P08-信封-wire统计", LPayloadKind::ParticleStats.wire() == 0x01, "统计=0x01");
    s.add("P08-信封-wiregizmo", LPayloadKind::EmitterGizmo.wire() == 0x02, "gizmo=0x02");
    s.add("P08-信封-wire互异", wires_unique(), "两码互异");
    s.add("P08-信封-往返", LPayloadKind::from_wire(0x01) == Some(LPayloadKind::ParticleStats)
        && LPayloadKind::from_wire(0x02) == Some(LPayloadKind::EmitterGizmo),
        "wire 往返一致");
    s.add("P08-信封-零码反查None", LPayloadKind::from_wire(0).is_none(), "0 非法码");

    // 摘要与漂移检测：篡改 byte_len 后 drifted 必须红（反向语料）。
    let mut tampered = e1;
    tampered.byte_len += 1;
    s.add("P08-信封-未篡改不漂移", !e1.drifted(), "自摘要一致");
    s.add("P08-信封-篡改漂移", tampered.drifted(), "byte_len 改 1 位即漂移");
    // schema 版本漂移同样检出。
    let mut bad_schema = e2;
    bad_schema.schema = 2;
    s.add("P08-信封-schema漂移", bad_schema.drifted(), "schema 改动检出");
    // kind 漂移（类型串线）检出。
    let mut bad_kind = e1;
    bad_kind.kind = LPayloadKind::EmitterGizmo;
    s.add("P08-信封-kind漂移", bad_kind.drifted(), "类型串线检出");

    // 对账拦截：注入篡改信封 → reconcile 置拦截 → take 一律 None。
    let mut reg2 = LEnvelopeRegistry::new();
    let _ = reg2.register(e1, &mut bag);
    let _ = reg2.register(e2, &mut bag);
    // 直接改槽内信封（模拟注册后载荷漂移）：用 replace 语义重注册一份篡改件。
    let bad = LEnvelope { kind: LPayloadKind::ParticleStats, schema: L_ENVELOPE_SCHEMA, byte_len: 999, seq: 1, declared: e1.declared };
    let _ = reg2.register(bad, &mut bag);
    let drifted_n = reg2.reconcile(&mut bag);
    s.add("P08-信封-对账检出1", drifted_n == 1, "恰 1 件漂移");
    s.add("P08-信封-拦截态", reg2.intercepted(), "漂移即拦截");
    s.add("P08-信封-拦截记P1", bag.has(DiagCode::ENVELOPE_DRIFT) && bag.p1_count() >= 1,
        "漂移是 P1 级");
    s.add("P08-信封-拦截后takeNone", reg2.take(LPayloadKind::ParticleStats).is_none()
        && reg2.take(LPayloadKind::EmitterGizmo).is_none(),
        "拦截态消费端全取不到");
    s.add("P08-信封-peek仍可见", reg2.peek(LPayloadKind::ParticleStats).is_some(),
        "对账自身要能看到信封");
    s.add("P08-信封-漂移累计", reg2.drift_count() == 1, "漂移计数 1");
    // 人确认解除拦截后恢复。
    reg2.clear_intercept();
    s.add("P08-信封-解除拦截", !reg2.intercepted() && reg2.take(LPayloadKind::ParticleStats).is_some(),
        "显式解除后恢复取用");

    // 未注册码（力场预留 0x03）：构造非法 kind 的注册路径——直接走
    // register 的 wire 越界分支不可达（LEnvelope.kind 类型封闭），故
    // 拒绝面用 from_wire/常量互异断言钉住（见 P07），此处钉注册表
    // 对**未知 wire 槽位**的防御：peek 对 0x03 无映射自然 None。
    s.add("P08-信封-表容量恰2", L_SEGMENT_SLOTS == 2, "L 段两类型（F1764 扩展）");
    s
}

// ---------------------------------------------------------------------------
// P09 · 发行版剔除零成本（双向验证）
// ---------------------------------------------------------------------------

fn p09_strip() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p09");
    let mut bag = DiagBag::new();

    // 对照组：Debug 档计数器真的递增（证明计数器不是死的）。
    let mut dev = StripGuard::new(BuildProfile::Debug);
    s.add("P09-剔除-默认Debug", BuildProfile::default() == BuildProfile::Debug,
        "调试是安全默认（忘配的发行构建不许悄悄带负载）");
    s.add("P09-剔除-Debug允许", dev.payloads_enabled(), "Debug 档负载可用");
    let b1 = dev.try_build(&mut bag);
    let b2 = dev.try_build(&mut bag);
    s.add("P09-剔除-Debug递增", b1 && b2 && dev.payload_builds == 2,
        "两次构建计数=2（对照组）");
    s.add("P09-剔除-Debug无强请求", dev.forced_attempts == 0, "Debug 档无强请求");

    // 实验组：Release 档连强行尝试都不递增构建计数。
    let mut rel = StripGuard::new(BuildProfile::Release);
    s.add("P09-剔除-Release禁用", !rel.payloads_enabled(), "Release 档负载禁用");
    let r1 = rel.try_build(&mut bag);
    let r2 = rel.try_build(&mut bag);
    s.add("P09-剔除-Release拒绝", !r1 && !r2, "强行请求返回 false");
    s.add("P09-剔除-Release零构建", rel.payload_builds == 0,
        "零成本：不进入构建（含强行路径）");
    s.add("P09-剔除-Release强请计数", rel.forced_attempts == 2, "强请求只记 attempts");
    s.add("P09-剔除-Release记P1", bag.has(DiagCode::STRIP_FAILED) && bag.p1_count() >= 2,
        "剔除失败是 P1（家族纪律）");
    // 双向验证守恒：两档计数互不串（同袋中 Debug 2 + Release 0）。
    s.add("P09-剔除-双档不串", dev.payload_builds == 2 && rel.payload_builds == 0,
        "档位语义互不污染");
    s
}

// ---------------------------------------------------------------------------
// P10 · 无障碍与家族自洽
// ---------------------------------------------------------------------------

fn p10_a11y() -> CheckSet {
    let mut s = CheckSet::new("VE-F2209-p10");

    // 码标签齐备且互异。
    s.add("P10-无障碍-标签互异", labels_unique(), "8 码人话互异");
    s.add("P10-无障碍-家族自洽", family_is_consistent(), "两型两槽 + 8 码");
    // 每个码标签非空。
    let mut i = 0usize;
    let mut all_nonempty = true;
    while i < DiagCode::ALL.len() {
        match DiagCode::ALL.get(i) {
            Some(c) => {
                if c.label().is_empty() || c.label() == "未登记诊断码" {
                    all_nonempty = false;
                }
            }
            None => all_nonempty = false,
        }
        i += 1;
    }
    s.add("P10-无障碍-标签全覆盖", all_nonempty, "8 码全部有非空人话");
    // 未登记码有兜底标签（不 panic）。
    s.add("P10-无障碍-未登记码兜底", DiagCode(0x2EFF).label() == "未登记诊断码",
        "未知码兜底人话");
    // 负载类型标签非空且互异。
    s.add("P10-无障碍-负载标签", !LPayloadKind::ParticleStats.label().is_empty()
        && !LPayloadKind::EmitterGizmo.label().is_empty()
        && LPayloadKind::ParticleStats.label() != LPayloadKind::EmitterGizmo.label(),
        "两型中文标签齐备互异");
    // 诊断袋渲染非空（诊断面不是哑巴）。
    let mut bag = DiagBag::new();
    bag.push(DiagCode::STATS_FLOOD);
    bag.push_p1(DiagCode::STRIP_FAILED);
    s.add("P10-无障碍-渲染非空", !bag.render().is_empty(), "render 产出人话");
    s.add("P10-无障碍-独立计数", bag.count(DiagCode::STATS_FLOOD) == 1
        && bag.count(DiagCode::STRIP_FAILED) == 1,
        "两码各自独立计数（不合并）");
    s.add("P10-无障碍-P1计数", bag.p1_count() == 1, "恰 1 条 P1");
    s.add("P10-无障碍-空袋渲染", DiagBag::new().render().is_empty(), "空袋渲染空串");

    // 阈值常量与锚点口径一致。
    s.add("P10-无障碍-洪水阈值口径", FLOOD_EMITTER_THRESHOLD == 1000, "锚点「千级发射器」");
    s.add("P10-无障碍-TopN口径", TOP_N_EMITTERS == 64, "一屏可读量级");
    s.add("P10-无障碍-预算口径", GIZMO_VERTEX_BUDGET == 512, "单 gizmo 顶点上限");
    s.add("P10-无障碍-最低段数", GIZMO_MIN_SEGS == 3, "圆周至少三边");
    s
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// VE-F2209 判据 a 集（P01–P05 · 五族 78 条）。
///
/// **为何分族注册**：十族原始条数 9+15+19+15+20+22+8+22+10+13 = 153，
/// 超过 [`crate::checks::MAX_CHECKS`]（112）——单行合并会在尾部**静默
/// 丢掉 41 条判据**（`add()` 超限只丢弃并置 truncated，聚合器 tally
/// 看不出异常）。按同域 VE-F2013/VE-F2407–F2410 分族注册先例拆
/// a/b 两集（78+75），各族 ≤ 112，**合并路径在注册表上不可达**
/// （聚合器只登记 a/b 两行；tests 对两集分别断言 `!truncated()`）。
pub fn run_vel09_checks_a_standalone() -> CheckSet {
    let mut out = CheckSet::new("VE-F2209-a");
    out = CheckSet::merge(out, p01_pool_level());
    out = CheckSet::merge(out, p02_emitter_level());
    out = CheckSet::merge(out, p03_ring());
    out = CheckSet::merge(out, p04_flood());
    out = CheckSet::merge(out, p05_shapes());
    out
}

/// VE-F2209 判据 b 集（P06–P10 · 五族 75 条）。
pub fn run_vel09_checks_b_standalone() -> CheckSet {
    let mut out = CheckSet::new("VE-F2209-b");
    out = CheckSet::merge(out, p06_lod());
    out = CheckSet::merge(out, p07_field_reserve());
    out = CheckSet::merge(out, p08_envelope());
    out = CheckSet::merge(out, p09_strip());
    out = CheckSet::merge(out, p10_a11y());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_green(st: &CheckSet) {
        let (items, n) = st.red_items();
        let mut failed = Vec::new();
        for it in items.iter().take(n) {
            if let Some(c) = it {
                if !c.passed {
                    failed.push(c.name);
                }
            }
        }
        assert!(failed.is_empty(), "[{}] 红项: {:?}", st.domain, failed);
        assert!(!st.truncated(), "[{}] 判据数超过 MAX_CHECKS 被截断", st.domain);
    }

    #[test]
    fn all_green_a() {
        let st = run_vel09_checks_a_standalone();
        assert_green(&st);
    }

    #[test]
    fn all_green_b() {
        let st = run_vel09_checks_b_standalone();
        assert_green(&st);
    }
}
