//! UNX-F56134 双机帧率编排件（0x610A0 段）
//!
//! 锚点：UNX-F56134-J1 编排全量（A/B 双侧场景帧率逐条落账、双侧同
//! 场景同序）；J2 编排语义（B 侧顺序必须跟随 A 侧前缀、差值 i64 中
//! 转可复算、差值带 ±5000bp）；J3 编排审计链（封账门、指纹、渲染完
//! 整）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·双机编排面）。
//!
//! 结构：双侧登记簿（场景闭集、侧内唯一）+ 顺序核（B 侧前缀跟随）+
//! 差值核（i64 中转、截断向零）+ 差值带闭式 + 指纹链 + 注入核（场景
//! 越界/零帧率/侧内重复/顺序不一致/长度不等/封账拒录/差值漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x610A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x610A0;
pub const FD_SCENE_OVER: u32 = SEG_BASE + 0x01; // 场景越界
pub const FD_FPS_RANGE: u32 = SEG_BASE + 0x02; // 零帧率/帧率越界
pub const FD_DUP_SCENE: u32 = SEG_BASE + 0x03; // 侧内重复场景
pub const FD_ORDER_MISMATCH: u32 = SEG_BASE + 0x04; // B 侧顺序不一致
pub const FD_LEN_UNEQUAL: u32 = SEG_BASE + 0x05; // 双侧长度不等先收尾
pub const FD_SEALED: u32 = SEG_BASE + 0x06; // 封账拒录
pub const FD_DELTA_DRIFT: u32 = SEG_BASE + 0x07; // 差值漂移超带

/// 场景闭集。
pub const SCENES: [&str; 5] = ["scroll", "drag", "animation", "web", "video"];

pub fn scene_ok(s: &str) -> bool {
    SCENES.contains(&s)
}

/// 帧率 ×100 定点合法带。
pub const FPS_MIN: u64 = 100;
pub const FPS_MAX: u64 = 120_000;

pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
}

/// 差值带（万分比，双侧同场景帧率漂移上限）。
pub const DRIFT_LIMIT_BP: i64 = 5_000;

/// 差值核：delta_bp = (b − a) × 10000 / a（i64 中转、截断向零）。
pub fn delta_bp(a: u64, b: u64) -> i64 {
    ((b as i64 - a as i64) * 10_000) / a as i64
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

fn fold_str(mut h: u32, s: &str) -> u32 {
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(CHAIN_PRIME);
    }
    h
}

/// 双机帧率编排登记簿：A 侧先录、B 侧按 A 侧前缀顺序跟随。
pub struct FpsDuoBook {
    cap: usize,
    a_rows: Vec<(String, u64)>,
    b_rows: Vec<(String, u64)>,
    pairs: Vec<(String, u64, u64, i64)>,
    chain: u32,
    sealed: bool,
}

impl FpsDuoBook {
    pub fn new(cap: usize) -> Self {
        FpsDuoBook {
            cap,
            a_rows: Vec::new(),
            b_rows: Vec::new(),
            pairs: Vec::new(),
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn a_len(&self) -> usize {
        self.a_rows.len()
    }

    pub fn b_len(&self) -> usize {
        self.b_rows.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 配对切片只读访问。
    pub fn pairs(&self) -> &[(String, u64, u64, i64)] {
        &self.pairs
    }

    fn input_ok(&self, scene: &str, fps_x100: u64, rows: &[(String, u64)]) -> Result<(), u32> {
        if !scene_ok(scene) {
            return Err(FD_SCENE_OVER);
        }
        if fps_x100 == 0 || !fps_ok(fps_x100) {
            return Err(FD_FPS_RANGE);
        }
        for r in rows {
            if r.0 == scene {
                return Err(FD_DUP_SCENE);
            }
        }
        Ok(())
    }

    /// A 侧录场景帧率。
    pub fn record_a(&mut self, scene: &str, fps_x100: u64) -> Result<usize, u32> {
        if self.sealed || self.a_rows.len() >= self.cap {
            return Err(FD_SEALED);
        }
        self.input_ok(scene, fps_x100, &self.a_rows)?;
        self.a_rows.push((scene.to_string(), fps_x100));
        Ok(self.a_rows.len() - 1)
    }

    /// B 侧录场景帧率：必须与 A 侧对应序位同场景（前缀跟随）。
    pub fn record_b(&mut self, scene: &str, fps_x100: u64) -> Result<usize, u32> {
        if self.sealed || self.b_rows.len() >= self.cap {
            return Err(FD_SEALED);
        }
        self.input_ok(scene, fps_x100, &self.b_rows)?;
        match self.a_rows.get(self.b_rows.len()) {
            Some(expected) if expected.0 == scene => {}
            _ => return Err(FD_ORDER_MISMATCH),
        }
        self.b_rows.push((scene.to_string(), fps_x100));
        Ok(self.b_rows.len() - 1)
    }

    /// 封账：双侧长度必须相等；逐对差值带校验；定指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FD_SEALED);
        }
        if self.a_rows.len() != self.b_rows.len() {
            return Err(FD_LEN_UNEQUAL);
        }
        self.pairs.clear();
        for i in 0..self.a_rows.len() {
            let d = delta_bp(self.a_rows[i].1, self.b_rows[i].1);
            if d.abs() > DRIFT_LIMIT_BP {
                return Err(FD_DELTA_DRIFT);
            }
            self.pairs
                .push((self.a_rows[i].0.clone(), self.a_rows[i].1, self.b_rows[i].1, d));
        }
        self.chain = CHAIN_SEED;
        for p in &self.pairs {
            self.chain = fold_str(self.chain, &p.0);
            self.chain = fold_u32(self.chain, p.1 as u32);
            self.chain = fold_u32(self.chain, p.2 as u32);
            self.chain = fold_u32(self.chain, p.3 as u32);
        }
        self.sealed = true;
        Ok(())
    }

    /// 指纹复算：链与配对重放一致。
    pub fn chain_ok(&self) -> bool {
        let mut h = CHAIN_SEED;
        for p in &self.pairs {
            h = fold_str(h, &p.0);
            h = fold_u32(h, p.1 as u32);
            h = fold_u32(h, p.2 as u32);
            h = fold_u32(h, p.3 as u32);
        }
        h == self.chain
    }

    /// 渲染：每行 4 段（scene|a|b|delta）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in &self.pairs {
            out.push(format!("{}|{}|{}|{}", p.0, p.1, p.2, p.3));
        }
        out
    }
}

/// 标准账：A/B 三场景同序（差值 −41/33/−500）。
fn fixture() -> FpsDuoBook {
    let mut b = FpsDuoBook::new(8);
    let _ = b.record_a("scroll", 12_000);
    let _ = b.record_a("drag", 9_000);
    let _ = b.record_a("animation", 6_000);
    let _ = b.record_b("scroll", 11_950);
    let _ = b.record_b("drag", 9_030);
    let _ = b.record_b("animation", 5_700);
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 场景越界必抓。
    {
        let mut b = FpsDuoBook::new(8);
        caught += (b.record_a("game", 12_000) == Err(FD_SCENE_OVER)) as usize;
    }
    // 2 零帧率必抓。
    {
        let mut b = FpsDuoBook::new(8);
        caught += (b.record_a("scroll", 0) == Err(FD_FPS_RANGE)) as usize;
    }
    // 3 侧内重复必抓。
    {
        let mut b = FpsDuoBook::new(8);
        let _ = b.record_a("scroll", 12_000);
        caught += (b.record_a("scroll", 12_000) == Err(FD_DUP_SCENE)) as usize;
    }
    // 4 顺序不一致必抓。
    {
        let mut b = FpsDuoBook::new(8);
        let _ = b.record_a("scroll", 12_000);
        caught += (b.record_b("drag", 9_000) == Err(FD_ORDER_MISMATCH)) as usize;
    }
    // 5 长度不等收尾必抓。
    {
        let mut b = FpsDuoBook::new(8);
        let _ = b.record_a("scroll", 12_000);
        let _ = b.record_a("drag", 9_000);
        let _ = b.record_b("scroll", 11_950);
        caught += (b.seal() == Err(FD_LEN_UNEQUAL)) as usize;
    }
    // 6 封账拒录必抓。
    {
        let mut b = fixture();
        caught += (b.record_a("web", 12_000) == Err(FD_SEALED)) as usize;
    }
    // 7 差值漂移必抓（B=3000 对 A=12000 → −7500bp）。
    {
        let mut b = FpsDuoBook::new(8);
        let _ = b.record_a("scroll", 12_000);
        let _ = b.record_b("scroll", 3_000);
        caught += (b.seal() == Err(FD_DELTA_DRIFT)) as usize;
    }
    (caught, N)
}

/// UNX-F56134 全量判据（15 项）。
pub fn run_unx56134_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56134");
    let b = fixture();

    // J1 编排全量。
    let ok = b.a_len() == 3;
    s.add("56134-J1-A侧三条", ok, if ok { "" } else { "A 侧行数不符" });
    let ok = b.b_len() == 3;
    s.add("56134-J1-B侧三条", ok, if ok { "" } else { "B 侧行数不符" });
    let ok = scene_ok("scroll") && scene_ok("video") && !scene_ok("game");
    s.add("56134-J1-场景闭集", ok, if ok { "" } else { "场景闭集漂移" });
    let same_order = b.pairs().len() == 3
        && b.pairs()[0].0 == "scroll"
        && b.pairs()[1].0 == "drag"
        && b.pairs()[2].0 == "animation";
    let ok = same_order;
    s.add("56134-J1-双侧同序", ok, if ok { "" } else { "双侧顺序漂移" });

    // J2 编排语义。
    let ok = b.pairs()[0].3 == -41;
    s.add("56134-J2-差值行一", ok, if ok { "" } else { "scroll 差值非 -41" });
    let ok = b.pairs()[1].3 == 33;
    s.add("56134-J2-差值行二", ok, if ok { "" } else { "drag 差值非 33" });
    let ok = b.pairs()[2].3 == -500;
    s.add("56134-J2-差值行三", ok, if ok { "" } else { "animation 差值非 -500" });
    let ok = b.pairs().iter().all(|p| p.3.abs() <= DRIFT_LIMIT_BP);
    s.add("56134-J2-差值带", ok, if ok { "" } else { "差值越带" });
    let ok = b
        .pairs()
        .iter()
        .all(|p| fps_ok(p.1) && fps_ok(p.2));
    s.add("56134-J2-帧率带", ok, if ok { "" } else { "帧率越带" });
    let ok = delta_bp(12_000, 11_950) == -41 && delta_bp(9_000, 9_030) == 33;
    s.add("56134-J2-差值核复算", ok, if ok { "" } else { "差值核漂移" });

    // J3 编排审计链。
    let ok = b.sealed();
    s.add("56134-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.chain() != CHAIN_SEED && b.chain_ok();
    s.add("56134-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = b.render();
    let mut ok = lines.len() == 3;
    for l in &lines {
        if l.split('|').count() != 4 {
            ok = false;
        }
    }
    s.add("56134-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });
    let mut empty = FpsDuoBook::new(8);
    let ok = empty.record_b("scroll", 12_000) == Err(FD_ORDER_MISMATCH);
    s.add("56134-J3-B先录必拒", ok, if ok { "" } else { "B 先录未拒" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56134-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56134_all_checks_green() {
        let set = run_unx56134_checks();
        assert!(set.all_passed(), "UNX-F56134 红项: {}", set.red_items_text());
    }
}
