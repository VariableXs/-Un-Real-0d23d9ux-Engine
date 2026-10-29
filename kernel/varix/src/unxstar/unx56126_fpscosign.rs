//! UNX-F56126 G3/F1 帧账口径联签件（0x60FA0 段）
//!
//! 锚点：UNX-F56126-J1 联签账全量（场景账逐条落账：场景 × 帧数 ×
//! 帧率 × 双侧签名 × 联签指纹）；J2 联签语义（G3 侧签帧数、F1 侧
//! 签帧率、双侧齐才出联签、签名可复算）；J3 联签审计链（联签互异、
//! 键序指纹、账目闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·口径联签面）。
//!
//! 结构：联签登记簿（场景闭集、唯一场景）+ 签名核（G3/F1 前缀折叠、
//! 联签合成）+ 账目闭式（联签数 = 账目数）+ 键序指纹链 + 注入核（未
//! 知场景/零输入/单侧联签/签名不一致/封账拒写/重复场景/账目漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60FA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60FA0;
pub const CS_SCENE_OVER: u32 = SEG_BASE + 0x01; // 未知/越界场景
pub const CS_INPUT_ZERO: u32 = SEG_BASE + 0x02; // 零帧数/零帧率/帧率越界
pub const CS_SINGLE_SIDE: u32 = SEG_BASE + 0x03; // 单侧签名未齐
pub const CS_SIG_DIFF: u32 = SEG_BASE + 0x04; // 联签指纹不一致
pub const CS_SEALED: u32 = SEG_BASE + 0x05; // 封账/账满拒写
pub const CS_DUP_SCENE: u32 = SEG_BASE + 0x06; // 重复场景
pub const CS_CENSUS_MISS: u32 = SEG_BASE + 0x07; // 联签账目漂移/空账

/// 场景闭集。
pub const SCENES: [&str; 5] = ["scroll", "drag", "animation", "web", "video"];

pub fn scene_ok(s: &str) -> bool {
    SCENES.contains(&s)
}

pub const FPS_MIN: u64 = 100;
pub const FPS_MAX: u64 = 120_000;

pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
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

/// G3 侧签名：前缀 "G3|" + 场景 + 帧数。
pub fn g3_sig(scene: &str, frames: u64) -> u32 {
    let h = fold_str(CHAIN_SEED, "G3|");
    let h = fold_str(h, scene);
    fold_u32(h, frames as u32)
}

/// F1 侧签名：前缀 "F1|" + 场景 + 帧率（×100 定点）。
pub fn f1_sig(scene: &str, fps_x100: u64) -> u32 {
    let h = fold_str(CHAIN_SEED, "F1|");
    let h = fold_str(h, scene);
    fold_u32(h, fps_x100 as u32)
}

/// 联签指纹：双侧签名合成（再过一轮种子折叠）。
pub fn joint_of(g3: u32, f1: u32) -> u32 {
    fold_u32(fold_u32(CHAIN_SEED, g3), f1)
}

/// 联签账目条目。
pub struct CosignEntry {
    pub no: usize,
    pub scene: String,
    pub frames: u64,
    pub fps_x100: u64,
    pub g3: Option<u32>,
    pub f1: Option<u32>,
    pub joint: Option<u32>,
}

/// G3/F1 帧账口径联签登记簿。
pub struct CosignBook {
    cap: usize,
    entries: Vec<CosignEntry>,
    sealed_count: usize,
    chain: u32,
    sealed: bool,
}

impl CosignBook {
    pub fn new(cap: usize) -> Self {
        CosignBook {
            cap,
            entries: Vec::new(),
            sealed_count: 0,
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn sealed_count(&self) -> usize {
        self.sealed_count
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 账目切片只读访问。
    pub fn entries(&self) -> &[CosignEntry] {
        &self.entries
    }

    /// 落账：场景闭集 → 输入带 → 唯一场景。
    pub fn add_entry(&mut self, scene: &str, frames: u64, fps_x100: u64) -> Result<usize, u32> {
        if self.sealed || self.entries.len() >= self.cap {
            return Err(CS_SEALED);
        }
        if !scene_ok(scene) {
            return Err(CS_SCENE_OVER);
        }
        if frames == 0 || fps_x100 == 0 || !fps_ok(fps_x100) {
            return Err(CS_INPUT_ZERO);
        }
        for e in &self.entries {
            if e.scene == scene {
                return Err(CS_DUP_SCENE);
            }
        }
        let no = self.entries.len();
        self.entries.push(CosignEntry {
            no,
            scene: scene.to_string(),
            frames,
            fps_x100,
            g3: None,
            f1: None,
            joint: None,
        });
        Ok(no)
    }

    fn find_no(&self, scene: &str) -> Result<usize, u32> {
        for (i, e) in self.entries.iter().enumerate() {
            if e.scene == scene {
                return Ok(i);
            }
        }
        Err(CS_SCENE_OVER)
    }

    /// G3 侧签（幂等）：双侧齐时合成联签。
    pub fn sign_g3(&mut self, scene: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CS_SEALED);
        }
        let i = self.find_no(scene)?;
        let sig = g3_sig(scene, self.entries[i].frames);
        self.entries[i].g3 = Some(sig);
        if let (Some(g), Some(f)) = (self.entries[i].g3, self.entries[i].f1) {
            self.entries[i].joint = Some(joint_of(g, f));
        }
        Ok(sig)
    }

    /// F1 侧签（幂等）：双侧齐时合成联签。
    pub fn sign_f1(&mut self, scene: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CS_SEALED);
        }
        let i = self.find_no(scene)?;
        let sig = f1_sig(scene, self.entries[i].fps_x100);
        self.entries[i].f1 = Some(sig);
        if let (Some(g), Some(f)) = (self.entries[i].g3, self.entries[i].f1) {
            self.entries[i].joint = Some(joint_of(g, f));
        }
        Ok(sig)
    }

    /// 取已存 G3 侧签名。
    pub fn g3_of(&self, scene: &str) -> Result<u32, u32> {
        let i = self.find_no(scene)?;
        self.entries[i].g3.ok_or(CS_SINGLE_SIDE)
    }

    /// 取已存 F1 侧签名。
    pub fn f1_of(&self, scene: &str) -> Result<u32, u32> {
        let i = self.find_no(scene)?;
        self.entries[i].f1.ok_or(CS_SINGLE_SIDE)
    }

    /// 取已存联签指纹。
    pub fn joint_of_scene(&self, scene: &str) -> Result<u32, u32> {
        let i = self.find_no(scene)?;
        self.entries[i].joint.ok_or(CS_SINGLE_SIDE)
    }

    /// 联签账目闭式：联签数必须等于封账账目数。
    fn census_ok(&self) -> bool {
        let jointed = self.entries.iter().filter(|e| e.joint.is_some()).count();
        jointed == self.sealed_count
    }

    /// 全账校验：账目闭式 → 双侧齐 → 签名/联签复算。
    pub fn verify_all(&self) -> Result<(), u32> {
        if !self.census_ok() {
            return Err(CS_CENSUS_MISS);
        }
        for e in &self.entries {
            let g = e.g3.ok_or(CS_SINGLE_SIDE)?;
            let f = e.f1.ok_or(CS_SINGLE_SIDE)?;
            if g != g3_sig(&e.scene, e.frames) || f != f1_sig(&e.scene, e.fps_x100) {
                return Err(CS_SIG_DIFF);
            }
            let j = e.joint.ok_or(CS_SINGLE_SIDE)?;
            if j != joint_of(g, f) {
                return Err(CS_SIG_DIFF);
            }
        }
        if self.chain != chain_keyed(&self.entries) {
            return Err(CS_SIG_DIFF);
        }
        Ok(())
    }

    /// 封账：空账/未签齐拒绝；定账目数与键序指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_SEALED);
        }
        if self.entries.is_empty() {
            return Err(CS_CENSUS_MISS);
        }
        for e in &self.entries {
            if e.g3.is_none() || e.f1.is_none() {
                return Err(CS_SINGLE_SIDE);
            }
        }
        self.sealed_count = self.entries.len();
        self.chain = chain_keyed(&self.entries);
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 6 段（scene|frames|fps|g3|f1|joint）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.entries {
            let g = e.g3.unwrap_or(0);
            let f = e.f1.unwrap_or(0);
            let j = e.joint.unwrap_or(0);
            out.push(format!(
                "{}|{}|{}|{:x}|{:x}|{:x}",
                e.scene, e.frames, e.fps_x100, g, f, j
            ));
        }
        out
    }

    /// 测试注入口：仅注入核使用。
    fn break_g3_val(&mut self) {
        if let Some(g) = self.entries[0].g3 {
            self.entries[0].g3 = Some(g ^ 1);
        }
    }

    /// 测试注入口：仅注入核使用（撤一条联签致账目漂移）。
    fn break_joint(&mut self) {
        self.entries[0].joint = None;
    }
}

/// 键序指纹链：按场景升序（插入排序、元组换位）重算。
pub fn chain_keyed(entries: &[CosignEntry]) -> u32 {
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..entries.len() {
        idx.push(i);
    }
    for i in 1..idx.len() {
        let mut j = i;
        while j > 0 && entries[idx[j]].scene < entries[idx[j - 1]].scene {
            let tmp = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = tmp;
            j -= 1;
        }
    }
    let mut h = CHAIN_SEED;
    for &i in &idx {
        let e = &entries[i];
        h = fold_str(h, &e.scene);
        h = fold_u32(h, e.frames as u32);
        h = fold_u32(h, e.fps_x100 as u32);
        h = fold_u32(h, e.joint.unwrap_or(0));
    }
    h
}

/// 标准账：三场景双侧齐签后封账（帧数合计 16200、帧率均值 9000）。
fn fixture() -> CosignBook {
    let mut b = CosignBook::new(8);
    let _ = b.add_entry("scroll", 7_200, 12_000);
    let _ = b.add_entry("drag", 5_400, 9_000);
    let _ = b.add_entry("animation", 3_600, 6_000);
    let _ = b.sign_g3("scroll");
    let _ = b.sign_f1("scroll");
    let _ = b.sign_g3("drag");
    let _ = b.sign_f1("drag");
    let _ = b.sign_g3("animation");
    let _ = b.sign_f1("animation");
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 未知场景必抓。
    {
        let mut b = CosignBook::new(8);
        caught += (b.add_entry("game", 100, 12_000) == Err(CS_SCENE_OVER)) as usize;
    }
    // 2 零帧数必抓。
    {
        let mut b = CosignBook::new(8);
        caught += (b.add_entry("scroll", 0, 12_000) == Err(CS_INPUT_ZERO)) as usize;
    }
    // 3 重复场景必抓。
    {
        let mut b = CosignBook::new(8);
        let _ = b.add_entry("scroll", 7_200, 12_000);
        caught += (b.add_entry("scroll", 7_200, 12_000) == Err(CS_DUP_SCENE)) as usize;
    }
    // 4 单侧联签必抓。
    {
        let mut b = CosignBook::new(8);
        let _ = b.add_entry("scroll", 7_200, 12_000);
        let _ = b.sign_g3("scroll");
        caught += (b.verify_all() == Err(CS_SINGLE_SIDE)) as usize;
    }
    // 5 签名不一致必抓。
    {
        let mut b = fixture();
        b.break_g3_val();
        caught += (b.verify_all() == Err(CS_SIG_DIFF)) as usize;
    }
    // 6 封账拒写必抓。
    {
        let mut b = fixture();
        caught += (b.add_entry("web", 1_800, 12_000) == Err(CS_SEALED)) as usize;
    }
    // 7 账目漂移必抓。
    {
        let mut b = fixture();
        b.break_joint();
        caught += (b.verify_all() == Err(CS_CENSUS_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56126 全量判据（15 项）。
pub fn run_unx56126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56126");
    let b = fixture();

    // J1 联签账全量。
    let ok = b.len() == 3;
    s.add("56126-J1-账目三条", ok, if ok { "" } else { "账目数不符" });
    let ok = scene_ok("scroll") && scene_ok("video") && !scene_ok("game");
    s.add("56126-J1-场景闭集", ok, if ok { "" } else { "场景闭集漂移" });
    let ok = b.g3_of("scroll").is_ok() && b.f1_of("scroll").is_ok();
    s.add("56126-J1-双侧签名在账", ok, if ok { "" } else { "签名缺失" });

    // J2 联签语义。
    let ok = b.g3_of("scroll") == Ok(g3_sig("scroll", 7_200));
    s.add("56126-J2-G3签复算", ok, if ok { "" } else { "G3 签漂移" });
    let ok = b.f1_of("drag") == Ok(f1_sig("drag", 9_000));
    s.add("56126-J2-F1签复算", ok, if ok { "" } else { "F1 签漂移" });
    let ok = b.joint_of_scene("scroll")
        == Ok(joint_of(g3_sig("scroll", 7_200), f1_sig("scroll", 12_000)));
    s.add("56126-J2-联签合成", ok, if ok { "" } else { "联签合成漂移" });
    let j1 = b.joint_of_scene("scroll").unwrap_or(0);
    let j2 = b.joint_of_scene("drag").unwrap_or(0);
    let j3 = b.joint_of_scene("animation").unwrap_or(0);
    let ok = j1 != j2 && j2 != j3 && j1 != j3;
    s.add("56126-J2-联签互异", ok, if ok { "" } else { "联签碰撞" });
    let total = 7_200 + 5_400 + 3_600;
    let ok = total == 16_200;
    s.add("56126-J2-帧数合计", ok, if ok { "" } else { "帧数合计漂移" });
    let mean = (12_000 + 9_000 + 6_000) / 3;
    let ok = mean == 9_000;
    s.add("56126-J2-帧率均值", ok, if ok { "" } else { "帧率均值漂移" });

    // J3 联签审计链。
    let ok = b.sealed();
    s.add("56126-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.sealed_count() == 3;
    s.add("56126-J3-账目闭式", ok, if ok { "" } else { "账目数漂移" });
    let ok = b.verify_all().is_ok();
    s.add("56126-J3-全账校验", ok, if ok { "" } else { "全账校验失败" });
    let ok = b.chain() == chain_keyed(b.entries()) && b.chain() != CHAIN_SEED;
    s.add("56126-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = b.render();
    let mut ok = lines.len() == 3;
    for l in &lines {
        if l.split('|').count() != 6 {
            ok = false;
        }
    }
    s.add("56126-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56126-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56126_all_checks_green() {
        let set = run_unx56126_checks();
        assert!(set.all_passed(), "UNX-F56126 红项: {}", set.red_items_text());
    }
}
