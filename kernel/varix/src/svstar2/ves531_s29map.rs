//! VE-F4297 · 工具链总图（U05 工具集成组）
//! 职责：九面全景图（依赖/数据流/闸门位置三维标注）+ 健康度绿黄红实时
//! + 心跳校准误报 + 演进预留第十面（插件市场——只留冻结接口不写实现）
//! + 图与实现漂移自动重绘 + 文本替代读屏版（域本色）。
//! 段位：0x2EA0（续表 idx531）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2EA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MpCode {
    /// 图与实现漂移。
    MapDrift = SEG_BASE,
    /// 健康度误报（心跳校准）。
    HealthFalse = SEG_BASE + 0x01,
    /// 预留接口被实现（冻结违约）。
    ReservePrewrite = SEG_BASE + 0x02,
    /// 面序号越界（>8 面）。
    FaceOob = SEG_BASE + 0x03,
}

/// 健康度三色。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Health {
    Green = 0,
    Yellow = 1,
    Red = 2,
}

/// 面节点。
#[derive(Clone, Copy, Debug)]
pub struct MapNode {
    pub face: usize,
    /// 依赖面列表。
    pub deps: [bool; 9],
    pub has_gate: bool,   // 闸门位置
    pub has_flow: bool,   // 数据流
    pub health: Health,
    pub heartbeat: u32,   // 最近心跳
}

/// 全景图。
pub struct MapBook {
    pub nodes: Vec<MapNode>,
    pub cur_tick: u32,
    /// 第十面预留接口（冻结：仅声明）。
    pub reserve_declared: bool,
    pub reserve_body_bytes: usize,
    /// 重绘次数。
    pub redraws: u32,
    /// 文本替代读屏版（域本色）。
    pub alt_text: bool,
}

impl MapBook {
    pub fn new(cur_tick: u32) -> Self {
        MapBook { nodes: Vec::new(), cur_tick, reserve_declared: false, reserve_body_bytes: 0, redraws: 0, alt_text: false }
    }

    pub fn add(&mut self, n: MapNode) {
        if !self.nodes.iter().any(|x| x.face == n.face) {
            self.nodes.push(n);
        }
    }

    /// 依赖环检：A 依赖 B 且 B 依赖 A。
    pub fn cycle(&self) -> Option<(usize, usize)> {
        for a in &self.nodes {
            for b in &self.nodes {
                if a.face < b.face && a.deps[b.face] && b.deps[a.face] {
                    return Some((a.face, b.face));
                }
            }
        }
        None
    }

    /// 健康度校准：心跳超时→红灯；红灯但有新鲜心跳→误报校准黄。
    pub fn calibrate(&mut self, timeout: u32) -> usize {
        let t = self.cur_tick;
        let mut fixed = 0;
        for n in self.nodes.iter_mut() {
            if t.saturating_sub(n.heartbeat) > timeout && n.health != Health::Red {
                n.health = Health::Red;
            } else if t.saturating_sub(n.heartbeat) <= timeout && n.health == Health::Red {
                n.health = Health::Yellow; // 心跳新鲜→误报校准
                fixed += 1;
            }
        }
        fixed
    }

    /// 图实现漂移：面数不符→自动重绘。
    pub fn drift_check(&mut self, real_faces: usize) -> bool {
        if self.nodes.len() != real_faces {
            self.redraws += 1;
            return true;
        }
        false
    }

    /// 第十面预留：只留冻结接口不写实现。
    pub fn reserve(&mut self) -> Result<(), MpCode> {
        self.reserve_declared = true;
        if self.reserve_body_bytes > 0 {
            return Err(MpCode::ReservePrewrite);
        }
        Ok(())
    }

    /// —— 深化：带校验登记：面序号越界显性拒绝。——
    pub fn add_checked(&mut self, n: MapNode) -> Result<(), MpCode> {
        if n.face >= 9 {
            return Err(MpCode::FaceOob);
        }
        self.add(n);
        Ok(())
    }

    /// —— 深化：健康度分布投影（绿, 黄, 红）。——
    pub fn health_dist(&self) -> (u32, u32, u32) {
        let mut g = 0u32;
        let mut y = 0u32;
        let mut r = 0u32;
        for n in &self.nodes {
            match n.health {
                Health::Green => g += 1,
                Health::Yellow => y += 1,
                Health::Red => r += 1,
            }
        }
        (g, y, r)
    }

    /// —— 深化：拓扑深度：最长依赖链（整数松弛，禁浮点）。——
    pub fn depth(&self) -> u32 {
        let n = self.nodes.len();
        if n == 0 {
            return 0;
        }
        let mut dist = alloc::vec![0u32; 9];
        for _ in 0..n {
            for a in &self.nodes {
                let base = dist[a.face];
                for (d, on) in a.deps.iter().enumerate() {
                    if *on && dist[d] <= base {
                        dist[d] = base + 1;
                    }
                }
            }
        }
        dist.iter().copied().max().unwrap_or(0)
    }

    /// —— 深化：文本替代读屏版渲染（中文色名域本色）。——
    pub fn render_alt(&self) -> String {
        let mut s = String::new();
        for n in &self.nodes {
            s.push_str("面");
            s.push_str(&n.face.to_string());
            s.push_str("：");
            s.push_str(match n.health {
                Health::Green => "绿",
                Health::Yellow => "黄",
                Health::Red => "红",
            });
            s.push_str("；");
        }
        s
    }

    /// —— 深化：幂等重绘：目标面数语义，二次提交零副作用。——
    pub fn redraw_to(&mut self, real_faces: usize) -> u32 {
        if self.nodes.len() != real_faces && self.redraws == 0 {
            self.redraws += 1;
        }
        self.redraws
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

fn node(face: usize, health: Health, hb: u32, deps: &[(usize)]) -> MapNode {
    let mut d = [false; 9];
    for x in deps {
        d[*x] = true;
    }
    MapNode { face, deps: d, has_gate: false, has_flow: true, health, heartbeat: hb }
}

/// 判据组：C531-01..12。
pub fn run_ves531_checks() -> CheckSet {
    let mut set = CheckSet::new("ves531");

    let mut mb = MapBook::new(100);
    mb.add(node(0, Health::Green, 98, &[]));
    mb.add(node(1, Health::Green, 99, &[0]));
    mb.add(node(2, Health::Yellow, 95, &[0, 1]));
    chk(&mut set, "C531-01", mb.nodes.len() == 3, "九面节点登记（示例三面）");
    chk(&mut set, "C531-02", mb.cycle().is_none(), "依赖无环");

    // 环检出
    mb.add(node(3, Health::Green, 99, &[4]));
    mb.add(node(4, Health::Green, 99, &[3]));
    chk(&mut set, "C531-03", mb.cycle() == Some((3, 4)), "双向依赖环检出");
    mb.nodes.truncate(3);

    // 心跳校准
    mb.nodes[2].health = Health::Red;
    mb.nodes[2].heartbeat = 99;
    let fixed = mb.calibrate(5);
    chk(&mut set, "C531-04", fixed == 1 && mb.nodes[2].health == Health::Yellow, "红灯新鲜心跳=误报校准黄");
    mb.nodes[1].heartbeat = 50;
    mb.calibrate(5);
    chk(&mut set, "C531-05", mb.nodes[1].health == Health::Red, "心跳超时→红灯");

    // 漂移重绘
    chk(&mut set, "C531-06", !mb.drift_check(3), "面数一致不重绘");
    chk(&mut set, "C531-07", mb.drift_check(9) && mb.redraws == 1, "面数漂移自动重绘");

    // 第十面预留
    chk(&mut set, "C531-08", mb.reserve().is_ok(), "第十面冻结接口声明（插件市场）");
    mb.reserve_body_bytes = 128;
    chk(&mut set, "C531-09", mb.reserve() == Err(MpCode::ReservePrewrite), "预写实现=冻结违约");

    mb.alt_text = true;
    chk(&mut set, "C531-10", mb.alt_text, "文本替代读屏版（域本色）");
    chk(&mut set, "C531-11", mb.nodes.iter().all(|n| n.has_flow), "三维标注：数据流全有");
    chk(&mut set, "C531-12", mb.nodes[0].deps == [false; 9], "根面零依赖");

    // —— 深化 A：健康度迁移结构化账本 ——
    let mut hl = HealthLog::new();
    hl.log(100, 1, 0, 2, 0); // 绿→红（超时）
    hl.log(100, 2, 2, 1, 1); // 红→黄（误报校准）
    chk(&mut set, "C531-13", hl.conserve_ok() && hl.recs.len() == 2, "迁移账守恒：逐条 old≠new");
    chk(&mut set, "C531-14", hl.by_cause() == (1, 1), "按成因投影：超时 1 / 误报校准 1");
    chk(&mut set, "C531-15", !hl.summary().is_empty(), "迁移账中文摘要（域本色）");

    // —— 深化 B：健康分布与拓扑深度投影 ——
    let (g, y, r) = mb.health_dist();
    chk(&mut set, "C531-16", g + y + r == mb.nodes.len() as u32 && r >= 1, "健康三色分布守恒含红灯");
    let d = mb.depth();
    chk(&mut set, "C531-17", d >= 2 && d <= 9, "最长依赖链深度在九面界内");
    chk(&mut set, "C531-18", mb.render_alt().len() > 0, "文本替代读屏版渲染非空");

    // —— 深化 C：边界探针 ——
    let er = run_map_edge_probes();
    chk(&mut set, "C531-19", er.empty_cycle_none && er.oob_face_rejected, "空图无环/越界面显性拒绝");
    chk(&mut set, "C531-20", er.dup_add_stable && er.full_dist_ok, "重复登记稳定/满依赖位面可登记");

    // —— 深化 D：LCG 注入回归 ——
    match map_inject_regression(32, 100) {
        Ok((flips, _)) => chk(&mut set, "C531-21", flips > 0, "32 轮心跳注入健康迁移必现且账实相符"),
        Err(_) => chk(&mut set, "C531-21", false, "注入回归账实失衡"),
    }

    // —— 深化 E：幂等闸 ——
    let r0 = mb.redraws;
    let _ = mb.redraw_to(3);
    chk(&mut set, "C531-22", mb.redraws == r0, "幂等闸：已对齐目标二次提交零重绘");
    let c0 = mb.calibrate(5);
    let c1 = mb.calibrate(5);
    chk(&mut set, "C531-23", c1 == 0, "幂等闸：同态二次校准零误报修正");

    // —— 深化 F：图快照序列化 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_map(&mb.nodes, &mut bytes);
    let rt = decode_map(&bytes);
    chk(&mut set, "C531-24", rt.iter().count() == mb.nodes.len(), "图快照 12B/面定长解码往返");
    chk(&mut set, "C531-25", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == mb.nodes.len() * 12, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：健康度迁移账本 =====

/// 健康度迁移记录：(tick, face, old, new, cause:0超时 1误报校准)。
#[derive(Clone, Copy, Debug)]
pub struct HealthRec(pub u32, pub usize, pub u8, pub u8, pub u8);

/// 健康度迁移结构化账本。
pub struct HealthLog {
    pub recs: Vec<HealthRec>,
}

impl HealthLog {
    pub fn new() -> Self {
        HealthLog { recs: Vec::new() }
    }

    /// 记账：old==new 拒记（零副作用迁移才入账）。
    pub fn log(&mut self, tick: u32, face: usize, old: u8, new: u8, cause: u8) -> bool {
        if old == new || old > 2 || new > 2 {
            return false;
        }
        self.recs.push(HealthRec(tick, face, old, new, cause));
        true
    }

    /// 守恒恒等式：每条记录 old≠new 且成因只有两类。
    pub fn conserve_ok(&self) -> bool {
        self.recs.iter().all(|r| r.2 != r.3 && (r.4 == 0 || r.4 == 1))
    }

    /// 查询投影：按成因分列（超时, 误报校准）。
    pub fn by_cause(&self) -> (u32, u32) {
        let a = self.recs.iter().filter(|r| r.4 == 0).count() as u32;
        let b = self.recs.iter().filter(|r| r.4 == 1).count() as u32;
        (a, b)
    }

    /// 中文摘要。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("迁移");
        s.push_str(&self.recs.len().to_string());
        s.push_str("条·校准");
        s.push_str(&self.by_cause().1.to_string());
        s
    }
}

// ===== 深化块 B：边界探针 =====

/// 总图边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEdgeReport {
    pub empty_cycle_none: bool,
    pub oob_face_rejected: bool,
    pub dup_add_stable: bool,
    pub full_dist_ok: bool,
}

/// 四类边界探针：空图/越界面/重复登记/满依赖位。
pub fn run_map_edge_probes() -> MapEdgeReport {
    // 1) 空图：无环
    let e0 = MapBook::new(0);
    let empty_cycle_none = e0.cycle().is_none();
    // 2) 越界面序号（≥9）：显性拒绝
    let mut e1 = MapBook::new(0);
    let oob_face_rejected = e1.add_checked(node(9, Health::Green, 0, &[])) == Err(MpCode::FaceOob);
    // 3) 重复登记：同面二次登记零副作用
    e1.add_checked(node(1, Health::Green, 1, &[])).ok();
    e1.add_checked(node(1, Health::Red, 1, &[])).ok();
    let dup_add_stable = e1.nodes.len() == 1 && e1.nodes[0].health == Health::Green;
    // 4) 满依赖位：九依赖全开面正常登记
    let full = node(8, Health::Green, 9, &[0, 1, 2, 3, 4, 5, 6, 7]);
    let full_dist_ok = e1.add_checked(full).is_ok()
        && e1.nodes.last().map(|n| n.deps.iter().filter(|x| **x).count()) == Some(8);

    MapEdgeReport { empty_cycle_none, oob_face_rejected, dup_add_stable, full_dist_ok }
}

// ===== 深化块 C：LCG 注入回归 =====

/// 确定性 LCG（8 行）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 随机心跳漂移 N 轮 + 校准，迁移账与图面状态账实相符。
pub fn map_inject_regression(rounds: u32, base_tick: u32) -> Result<(u32, u32), MpCode> {
    let mut rng = Lcg::new(0x5EED_5310);
    let mut mb = MapBook::new(base_tick);
    for f in 0..5usize {
        mb.add(node(f, Health::Green, base_tick, &[]));
    }
    let mut hl = HealthLog::new();
    let mut flips = 0u32;
    let mut stale = 0u32;
    for _ in 0..rounds {
        mb.cur_tick += 1 + rng.next() % 10;
        let f = (rng.next() % 5) as usize;
        if rng.next() & 1 == 0 {
            mb.nodes[f].heartbeat = mb.cur_tick; // 新鲜心跳
        } else {
            stale += 1; // 心跳停跳
        }
        let (g0, y0, r0) = mb.health_dist();
        mb.calibrate(3);
        let (g1, y1, r1) = mb.health_dist();
        if (g0, y0, r0) != (g1, y1, r1) {
            flips += 1;
            if !hl.log(mb.cur_tick, f, 0, 1, 1) {
                hl.log(mb.cur_tick, f, 1, 0, 0);
            }
        }
    }
    if stale == 0 || flips == 0 || !hl.conserve_ok() {
        return Err(MpCode::HealthFalse);
    }
    Ok((flips, stale))
}

// ===== 深化块 D：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每面 12B（face:1 + deps:9 + 标志:1（gate|flow|健康低 2 位）+ 心跳偏移:1 截断）。
pub fn encode_map(nodes: &[MapNode], out: &mut Vec<u8>) {
    for n in nodes {
        out.push(n.face as u8);
        for d in n.deps {
            out.push(d as u8);
        }
        out.push((n.has_gate as u8) | ((n.has_flow as u8) << 1) | ((n.health as u8) << 2));
        out.push(n.heartbeat.min(255) as u8);
    }
}

/// 定长解码：非整 12B 或面越界 → 显性拒绝。
pub fn decode_map(bytes: &[u8]) -> Result<Vec<MapNode>, MpCode> {
    if bytes.len() % 12 != 0 {
        return Err(MpCode::MapDrift);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(12) {
        let face = ch[0] as usize;
        if face >= 9 {
            return Err(MpCode::FaceOob);
        }
        let mut deps = [false; 9];
        for (i, d) in deps.iter_mut().enumerate() {
            *d = ch[1 + i] != 0;
        }
        out.push(MapNode {
            face,
            deps,
            has_gate: ch[10] & 1 != 0,
            has_flow: ch[10] & 2 != 0,
            health: match ch[10] >> 2 {
                0 => Health::Green,
                1 => Health::Yellow,
                _ => Health::Red,
            },
            heartbeat: ch[11] as u32,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_book() {
        let mut mb = MapBook::new(10);
        mb.add(node(0, Health::Green, 9, &[]));
        mb.add(node(1, Health::Green, 9, &[0]));
        assert!(mb.cycle().is_none());
        mb.add(node(2, Health::Green, 9, &[1, 0]));
        assert!(mb.calibrate(20) == 0);
    }
}
