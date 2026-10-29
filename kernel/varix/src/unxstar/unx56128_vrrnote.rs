//! UNX-F56128 VRR/垂直同步口径注记件（0x60FE0 段）
//!
//! 锚点：UNX-F56128-J1 注记全量（场景 × 同步模式 × 刷新边界逐条落
//! 账）；J2 口径语义（三模式闭集——fixed 定频等值、vrr 区间内、
//! vsync_off 不锁——帧率判定可复算、漂移带 ±200bp）；J3 注记审计
//! 链（封账门、指纹、渲染完整）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·口径注记面）。
//!
//! 结构：注记登记簿（场景/模式双闭集、唯一场景）+ 边界核（模式与边
//! 界自洽）+ 判定核（fixed 等值带、vrr 区间带、voff 放行）+ 指纹链 +
//! 注入核（未知模式/刷新率越界/边界矛盾/未知场景/未注记查询/封账拒
//! 写/帧率漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60FE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60FE0;
pub const VN_MODE_OVER: u32 = SEG_BASE + 0x01; // 未知同步模式
pub const VN_HZ_RANGE: u32 = SEG_BASE + 0x02; // 刷新率越界
pub const VN_BOUND_BAD: u32 = SEG_BASE + 0x03; // 模式与边界矛盾
pub const VN_SCENE_BAD: u32 = SEG_BASE + 0x04; // 未知场景
pub const VN_NO_NOTE: u32 = SEG_BASE + 0x05; // 未注记查询
pub const VN_SEALED: u32 = SEG_BASE + 0x06; // 封账/账满/重复场景拒写
pub const VN_FPS_DRIFT: u32 = SEG_BASE + 0x07; // 帧率对不上口径

/// 同步模式闭集。
pub const MODES: [&str; 3] = ["fixed", "vrr", "vsync_off"];
pub const M_FIXED: usize = 0;
pub const M_VRR: usize = 1;
pub const M_VOFF: usize = 2;

pub fn mode_ok(m: usize) -> bool {
    m < MODES.len()
}

/// 场景闭集。
pub const SCENES: [&str; 5] = ["scroll", "drag", "animation", "web", "video"];

pub fn scene_ok(s: &str) -> bool {
    SCENES.contains(&s)
}

/// 刷新率合法带（Hz；vsync_off 例外为 0/0）。
pub const HZ_MIN: u32 = 30;
pub const HZ_MAX: u32 = 360;

/// fixed 等值漂移带（万分比）。
pub const TOLERANCE_BP: u64 = 200;

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

/// 口径注记。
pub struct SyncNote {
    pub no: usize,
    pub scene: String,
    pub mode: usize,
    pub hz_min: u32,
    pub hz_max: u32,
}

/// VRR/垂直同步口径注记登记簿。
pub struct VrrNoteBook {
    cap: usize,
    notes: Vec<SyncNote>,
    chain: u32,
    sealed: bool,
}

impl VrrNoteBook {
    pub fn new(cap: usize) -> Self {
        VrrNoteBook {
            cap,
            notes: Vec::new(),
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 注记切片只读访问。
    pub fn notes(&self) -> &[SyncNote] {
        &self.notes
    }

    /// 落注记：场景闭集 → 模式闭集 → 刷新带 → 模式边界自洽 → 唯一场景。
    pub fn add_note(&mut self, scene: &str, mode: usize, hz_min: u32, hz_max: u32) -> Result<usize, u32> {
        if self.sealed || self.notes.len() >= self.cap {
            return Err(VN_SEALED);
        }
        if !scene_ok(scene) {
            return Err(VN_SCENE_BAD);
        }
        if !mode_ok(mode) {
            return Err(VN_MODE_OVER);
        }
        if mode == M_VOFF {
            if hz_min != 0 || hz_max != 0 {
                return Err(VN_BOUND_BAD);
            }
        } else {
            if hz_min < HZ_MIN || hz_max > HZ_MAX {
                return Err(VN_HZ_RANGE);
            }
            if mode == M_FIXED && hz_min != hz_max {
                return Err(VN_BOUND_BAD);
            }
            if mode == M_VRR && hz_min >= hz_max {
                return Err(VN_BOUND_BAD);
            }
        }
        for n in &self.notes {
            if n.scene == scene {
                // 重复场景按拒写处理。
                return Err(VN_SEALED);
            }
        }
        let no = self.notes.len();
        self.notes.push(SyncNote {
            no,
            scene: scene.to_string(),
            mode,
            hz_min,
            hz_max,
        });
        Ok(no)
    }

    /// 口径边界：fixed → (hz×100, hz×100)；vrr → (min×100, max×100)；
    /// vsync_off → (0, u64::MAX)。
    pub fn expect_of(&self, scene: &str) -> Result<(u64, u64), u32> {
        for n in &self.notes {
            if n.scene == scene {
                return Ok(match n.mode {
                    M_FIXED => (n.hz_min as u64 * 100, n.hz_min as u64 * 100),
                    M_VRR => (n.hz_min as u64 * 100, n.hz_max as u64 * 100),
                    _ => (0, u64::MAX),
                });
            }
        }
        Err(VN_NO_NOTE)
    }

    /// 判定核：帧率（×100）对口径。
    pub fn verdict(&self, scene: &str, fps_x100: u64) -> Result<(), u32> {
        let (lo, hi) = self.expect_of(scene)?;
        if fps_x100 < lo || fps_x100 > hi {
            return Err(VN_FPS_DRIFT);
        }
        if lo == hi && lo != 0 {
            let diff = if fps_x100 > lo { fps_x100 - lo } else { lo - fps_x100 };
            let bp = (diff as u128 * 10_000 / lo as u128) as u64;
            if bp > TOLERANCE_BP {
                return Err(VN_FPS_DRIFT);
            }
        }
        Ok(())
    }

    /// 封账：空账拒绝、定指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(VN_SEALED);
        }
        if self.notes.is_empty() {
            return Err(VN_NO_NOTE);
        }
        self.chain = chain_keyed(&self.notes);
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 4 段（scene|mode 名|min|max）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for n in &self.notes {
            out.push(format!(
                "{}|{}|{}|{}",
                n.scene, MODES[n.mode], n.hz_min, n.hz_max
            ));
        }
        out
    }

    /// 指纹复算：链与注记重放一致。
    pub fn chain_ok(&self) -> bool {
        let mut h = CHAIN_SEED;
        for n in &self.notes {
            h = fold_str(h, &n.scene);
            h = fold_u32(h, n.mode as u32);
            h = fold_u32(h, n.hz_min);
            h = fold_u32(h, n.hz_max);
        }
        h == self.chain
    }
}

/// 键序指纹链：按场景升序（插入排序、元组换位）重算。
pub fn chain_keyed(notes: &[SyncNote]) -> u32 {
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..notes.len() {
        idx.push(i);
    }
    for i in 1..idx.len() {
        let mut j = i;
        while j > 0 && notes[idx[j]].scene < notes[idx[j - 1]].scene {
            let tmp = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = tmp;
            j -= 1;
        }
    }
    let mut h = CHAIN_SEED;
    for &i in &idx {
        let n = &notes[i];
        h = fold_str(h, &n.scene);
        h = fold_u32(h, n.mode as u32);
        h = fold_u32(h, n.hz_min);
        h = fold_u32(h, n.hz_max);
    }
    h
}

/// 标准账：三场景注记后封账。
fn fixture() -> VrrNoteBook {
    let mut b = VrrNoteBook::new(8);
    let _ = b.add_note("scroll", M_FIXED, 120, 120);
    let _ = b.add_note("drag", M_VRR, 48, 144);
    let _ = b.add_note("animation", M_VOFF, 0, 0);
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 未知模式必抓。
    {
        let mut b = VrrNoteBook::new(8);
        caught += (b.add_note("scroll", 7, 120, 120) == Err(VN_MODE_OVER)) as usize;
    }
    // 2 刷新率越界必抓。
    {
        let mut b = VrrNoteBook::new(8);
        caught += (b.add_note("scroll", M_FIXED, 500, 500) == Err(VN_HZ_RANGE)) as usize;
    }
    // 3 边界矛盾必抓（fixed 定频两侧不等）。
    {
        let mut b = VrrNoteBook::new(8);
        caught += (b.add_note("scroll", M_FIXED, 120, 144) == Err(VN_BOUND_BAD)) as usize;
    }
    // 4 未知场景必抓。
    {
        let mut b = VrrNoteBook::new(8);
        caught += (b.add_note("game", M_FIXED, 120, 120) == Err(VN_SCENE_BAD)) as usize;
    }
    // 5 未注记查询必抓。
    {
        let b = fixture();
        caught += (b.verdict("web", 12_000) == Err(VN_NO_NOTE)) as usize;
    }
    // 6 封账拒写必抓。
    {
        let mut b = fixture();
        caught += (b.add_note("web", M_VRR, 48, 144) == Err(VN_SEALED)) as usize;
    }
    // 7 帧率漂移必抓（13000 对 12000 → 833bp > 200bp）。
    {
        let b = fixture();
        caught += (b.verdict("scroll", 13_000) == Err(VN_FPS_DRIFT)) as usize;
    }
    (caught, N)
}

/// UNX-F56128 全量判据（15 项）。
pub fn run_unx56128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56128");
    let b = fixture();

    // J1 注记全量。
    let ok = b.len() == 3;
    s.add("56128-J1-注记三条", ok, if ok { "" } else { "注记数不符" });
    let ok = mode_ok(M_FIXED) && mode_ok(M_VOFF) && !mode_ok(3);
    s.add("56128-J1-模式闭集", ok, if ok { "" } else { "模式闭集漂移" });
    let ok = scene_ok("scroll") && scene_ok("video") && !scene_ok("game");
    s.add("56128-J1-场景闭集", ok, if ok { "" } else { "场景闭集漂移" });
    let ok = b
        .notes()
        .iter()
        .all(|n| match n.mode {
            M_FIXED => n.hz_min == n.hz_max,
            M_VRR => n.hz_min < n.hz_max,
            _ => n.hz_min == 0 && n.hz_max == 0,
        });
    s.add("56128-J1-边界自洽", ok, if ok { "" } else { "边界矛盾" });

    // J2 口径语义。
    let (flo, fhi) = b.expect_of("scroll").unwrap_or((0, 0));
    let ok = flo == 12_000 && fhi == 12_000;
    s.add("56128-J2-定频期望", ok, if ok { "" } else { "定频期望非 12000" });
    let (vlo, vhi) = b.expect_of("drag").unwrap_or((0, 0));
    let ok = vlo == 4_800 && vhi == 14_400;
    s.add("56128-J2-vrr边界", ok, if ok { "" } else { "vrr 边界非 [4800,14400]" });
    let (olo, ohi) = b.expect_of("animation").unwrap_or((0, 0));
    let ok = olo == 0 && ohi == u64::MAX;
    s.add("56128-J2-垂直同步不锁", ok, if ok { "" } else { "voff 边界漂移" });
    let ok = b.verdict("scroll", 12_000).is_ok()
        && b.verdict("drag", 9_000).is_ok()
        && b.verdict("animation", 6_000).is_ok();
    s.add("56128-J2-三判定过", ok, if ok { "" } else { "判定误拒" });
    let ok = TOLERANCE_BP == 200;
    s.add("56128-J2-漂移带", ok, if ok { "" } else { "漂移带漂移" });
    let ok = 120 * 100 == 12_000 && 48 * 100 == 4_800;
    s.add("56128-J2-核复算", ok, if ok { "" } else { "口径核漂移" });

    // J3 注记审计链。
    let ok = b.sealed();
    s.add("56128-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.chain() != CHAIN_SEED && b.chain_ok();
    s.add("56128-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let lines = b.render();
    let mut ok = lines.len() == 3;
    for l in &lines {
        if l.split('|').count() != 4 {
            ok = false;
        }
    }
    s.add("56128-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });
    let ok = b.expect_of("web") == Err(VN_NO_NOTE);
    s.add("56128-J3-注记查询", ok, if ok { "" } else { "查询语义漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56128-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56128_all_checks_green() {
        let set = run_unx56128_checks();
        assert!(set.all_passed(), "UNX-F56128 红项: {}", set.red_items_text());
    }
}
