//! VE-F5603 域自检（VE-AB 域 · 空间音频骨架判据层）
//!
//! 判据侧**独立写死**期望（声像中点 128/128、衰减单调、beep 帧长 48、
//! 绑定面 4 名、码段 0x57 四码），不复用实现侧常量。聚合防自调：族内
//! 判据只调另一族 standalone + 进行中 set 自身 tally。

use crate::checks::CheckSet;

use super::veab03_spatial as sp;
use super::veab03_spatial::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_CODES: [u16; 4] = [0x5700, 0x5701, 0x5702, 0x5703];
const EXPECT_BEEP_LEN: usize = 48;
const EXPECT_BINDING_COUNT: usize = 4;

/// 就绪图：听者原点 + 两个声源（近/远，衰减单调语料）。
fn ready_graph() -> SpatialGraph {
    let mut g = SpatialGraph::new();
    g.engine_ready = true;
    let _ = g.set_listener(Listener { pos: Pos3 { x: 0, y: 0, z: 0 } });
    let _ = g.add_source(Source {
        id: 1,
        pos: Pos3 { x: 0, y: 0, z: 0 },
        gain: 100,
    });
    let _ = g.add_source(Source {
        id: 2,
        pos: Pos3 { x: 100, y: 0, z: 0 },
        gain: 100,
    });
    g
}

// ---------------------------------------------------------------------------
// 一、规格（骨架先行 / 降级直通 / 最小实现 / 一行出声 / 自检）
// ---------------------------------------------------------------------------

fn chk_spec_skeleton_first(set: &mut CheckSet) {
    // 规格-01：骨架先行——图从 NotReady 到 Ready 的就绪翻面
    //（听者、声源、引擎三依赖逐个就位才翻面）。
    let mut g = SpatialGraph::new();
    let not_ready_0 = g.readiness() == Readiness::NotReady;
    let _ = g.set_listener(Listener { pos: Pos3::default() });
    let not_ready_1 = g.readiness() == Readiness::NotReady;
    let _ = g.add_source(Source { id: 1, pos: Pos3::default(), gain: 100 });
    let not_ready_2 = g.readiness() == Readiness::NotReady;
    g.engine_ready = true;
    let ready = g.readiness() == Readiness::Ready;
    if not_ready_0 && not_ready_1 && not_ready_2 && ready {
        set.ok("EAB3-规格-01-就绪逐依赖翻面");
    } else {
        set.fail("EAB3-规格-01-就绪逐依赖翻面", "就绪判定未逐依赖收敛");
    }
    // 规格-02：接口冻结征询账——AB02/AB03 双回执，绑定面四名非空互异。
    let recs = consult_records();
    let acked = recs.len() == 2
        && recs[0].acked
        && recs[1].acked
        && recs[0].who == "AB02"
        && recs[1].who == "AB03";
    let mut distinct = true;
    let mut i = 0usize;
    while i < BINDING_SURFACE.len() {
        if BINDING_SURFACE[i].is_empty() {
            distinct = false;
        }
        let mut j = i + 1;
        while j < BINDING_SURFACE.len() {
            if BINDING_SURFACE[i] == BINDING_SURFACE[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    if acked && distinct && BINDING_SURFACE.len() == EXPECT_BINDING_COUNT {
        set.ok("EAB3-规格-02-征询账与绑定面");
    } else {
        set.fail("EAB3-规格-02-征询账与绑定面", "回执缺失或绑定面漂移");
    }
}

fn chk_spec_passthrough(set: &mut CheckSet) {
    // 规格-03：降级直通——未就绪时 spatialize 不报错，直通（左右 255
    // 同响 + degraded 标记）且降级计数递增（降级可查不静默）。
    let mut g = SpatialGraph::new();
    let _ = g.add_source(Source { id: 7, pos: Pos3::default(), gain: 50 });
    let r = g.spatialize(7);
    let ok = match r {
        Ok(o) => o.degraded && o.pan_l == 255 && o.pan_r == 255 && o.attenuation == 127,
        Err(_) => false,
    } && g.degraded_count == 1;
    if ok {
        set.ok("EAB3-规格-03-降级直通可查");
    } else {
        set.fail("EAB3-规格-03-降级直通可查", "未就绪未直通或直通无账");
    }
    // 规格-04：衰减单调——近源（d²=0 → 255）衰减 > 远源（d²=10000）。
    let mut g = ready_graph();
    let near = g.spatialize(1);
    let far = g.spatialize(2);
    let mono = match (near, far) {
        (Ok(a), Ok(b)) => {
            !a.degraded && !b.degraded && a.attenuation == 255 && b.attenuation < a.attenuation
        }
        _ => false,
    };
    if mono {
        set.ok("EAB3-规格-04-衰减单调");
    } else {
        set.fail("EAB3-规格-04-衰减单调", "衰减未随距离单调或未空间化");
    }
    // 规格-05：最小声像——中心源均衡 128/128；右偏源右响。
    let mut g = ready_graph();
    let center = g.spatialize(1);
    let right = g.spatialize(2);
    let pan_ok = match (center, right) {
        (Ok(c), Ok(rt)) => {
            c.pan_l == 128 && c.pan_r == 128 && rt.pan_r > rt.pan_l
        }
        _ => false,
    };
    if pan_ok {
        set.ok("EAB3-规格-05-最小声像");
    } else {
        set.fail("EAB3-规格-05-最小声像", "声像未按方位偏转");
    }
}

fn chk_spec_beep_and_health(set: &mut CheckSet) {
    // 规格-06：一行出声——确定性（同参数同帧）+ 基线非零（恒零帧是
    // 恒真门禁的温床：先证明该量非平凡再断它）。
    let f1 = one_line_beep(4);
    let f2 = one_line_beep(4);
    let mut non_zero = false;
    let mut i = 0usize;
    while i < EXPECT_BEEP_LEN {
        if f1[i] != 0 {
            non_zero = true;
        }
        i += 1;
    }
    if f1 == f2 && f1.len() == EXPECT_BEEP_LEN && non_zero {
        set.ok("EAB3-规格-06-一行出声确定性");
    } else {
        set.fail("EAB3-规格-06-一行出声确定性", "帧漂移/长度漂移/全零帧");
    }
    // 规格-07：健康自检——空图三假一真结构，建图后四项全真。
    let empty = SpatialGraph::new();
    let h0 = empty.health_check();
    let empty_ok = !h0[0] && !h0[1] && h0[2] && h0[3];
    let g = ready_graph();
    let h1 = g.health_check();
    let mut all_true = true;
    let mut i = 0usize;
    while i < 4 {
        if !h1[i] {
            all_true = false;
        }
        i += 1;
    }
    if empty_ok && all_true {
        set.ok("EAB3-规格-07-健康自检翻面");
    } else {
        set.fail("EAB3-规格-07-健康自检翻面", "自检未随图状态翻面");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（接口误用 / 实现越界）
// ---------------------------------------------------------------------------

fn chk_bound_misuse(set: &mut CheckSet) {
    // 边界-01：接口误用——未注册源 0x5700；重复注册 0x5702。
    let mut g = ready_graph();
    let unreg = g.spatialize(99);
    let dup = g.add_source(Source { id: 1, pos: Pos3::default(), gain: 10 });
    let ok = unreg == Err(E_SP3_UNREGISTERED) && dup == Err(E_SP3_DUPLICATE);
    if ok {
        set.ok("EAB3-边界-01-接口误用双闸");
    } else {
        set.fail("EAB3-边界-01-接口误用双闸", "未注册/重复注册未按码拒绝");
    }
    // 边界-02：实现越界——越界坐标拒绝不钳制，且计数（听者/声源双向）。
    let mut g = SpatialGraph::new();
    let bad_l = g.set_listener(Listener {
        pos: Pos3 { x: GRAPH_BOUND + 1, y: 0, z: 0 },
    });
    let bad_s = g.add_source(Source {
        id: 5,
        pos: Pos3 { x: 0, y: 0, z: -(GRAPH_BOUND + 1) },
        gain: 10,
    });
    let ok = bad_l == Err(E_SP3_OUT_OF_BOUND)
        && bad_s == Err(E_SP3_OUT_OF_BOUND)
        && g.out_of_bound_count == 2
        && !g.has_listener()
        && g.source_count() == 0;
    if ok {
        set.ok("EAB3-边界-02-越界拒绝不钳制");
    } else {
        set.fail("EAB3-边界-02-越界拒绝不钳制", "越界被钳制入库或计数缺失");
    }
}

fn chk_idem_o1(set: &mut CheckSet) {
    // 幂等-01：O(1) 确定性——同 id 输出不随图内其他源增删而变
    //（spatialize 只查目标源，不扫全图）。
    let mut g = ready_graph();
    let before = g.spatialize(2);
    let _ = g.add_source(Source {
        id: 9,
        pos: Pos3 { x: 9000, y: 9000, z: 9000 },
        gain: 100,
    });
    let after = g.spatialize(2);
    if before.is_ok() && before == after {
        set.ok("EAB3-幂等-01-输出与图规模无关");
    } else {
        set.fail("EAB3-幂等-01-输出与图规模无关", "加源改变了既有源输出");
    }
    // 幂等-02：spatialize 重复调用同结果（读函数语义）。
    let mut g = ready_graph();
    let a = g.spatialize(1);
    let b = g.spatialize(1);
    if a.is_ok() && a == b && g.degraded_count == 0 {
        set.ok("EAB3-幂等-02-重复调用同结果");
    } else {
        set.fail("EAB3-幂等-02-重复调用同结果", "读调用产生漂移或误计降级");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
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
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("veab03_spatial_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAB3-判据-01-判据面零 panic");
    } else {
        set.fail("EAB3-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-08：生产面零 panic（扫实现文件）。
    let src = include_str!("veab03_spatial.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EAB3-规格-09-生产面零 panic");
    } else {
        set.fail("EAB3-规格-09-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——四码皆 0x57 细分段、互异、与写死值逐位等。
    let got = [E_SP3_UNREGISTERED, E_SP3_OUT_OF_BOUND, E_SP3_DUPLICATE, E_SP3_NOT_READY];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x5700 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EAB3-判据-02-码段独占独立复核");
    } else {
        set.fail("EAB3-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 11 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_veab03_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 11;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EAB3-判据-03-聚合守恒防自调");
    } else {
        set.fail("EAB3-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等。
pub fn run_veab03_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veab03/a");
    chk_spec_skeleton_first(&mut s);
    chk_spec_passthrough(&mut s);
    chk_spec_beep_and_health(&mut s);
    chk_bound_misuse(&mut s);
    chk_idem_o1(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_veab03_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veab03/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veab03_checks() -> CheckSet {
    CheckSet::merge(run_veab03_checks_a_standalone(), run_veab03_checks_b_standalone())
}
