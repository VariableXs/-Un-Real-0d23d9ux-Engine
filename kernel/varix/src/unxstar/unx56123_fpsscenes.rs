//! UNX-F56123 帧率负载场景库（滚动/拖拽/动画三型）（0x60F40 段）
//!
//! 锚点：UNX-F56123-J1 场景库全量（三型场景逐条落库：场景名 × 时长毫秒
//! × 负载强度三元组）；J2 场景语义（时长物理界、强度闭集三档、场景名
//! 闭集逐项可验）；J3 场景审计链（场景指纹链、封账只读、三型覆盖）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·场景库面）。
//!
//! 结构：三型场景闭集 + 场景登记册（场景名 → 时长 × 强度逐条落库）+
//! 时长界闸（10s..120s）+ 强度闭集（轻/中/重三档）+ 场景指纹链 + 注入
//! 核（未知场景/时长越界/强度越界/重复场景/封账改写/空库查询/三型覆盖
//! 缺失必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60F40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60F40;
pub const SL_SCENE_UNK: u32 = SEG_BASE + 0x01; // 未知场景
pub const SL_DUR_RANGE: u32 = SEG_BASE + 0x02; // 时长越物理界
pub const SL_INTEN_OVER: u32 = SEG_BASE + 0x03; // 强度越闭集
pub const SL_DUP: u32 = SEG_BASE + 0x04; // 重复场景
pub const SL_SEALED: u32 = SEG_BASE + 0x05; // 场景库已封账拒写
pub const SL_EMPTY: u32 = SEG_BASE + 0x06; // 空库查询
pub const SL_COVER_MISS: u32 = SEG_BASE + 0x07; // 三型覆盖缺失

/// 三型场景闭集。
pub const SC_ROLL: usize = 0;
pub const SC_DRAG: usize = 1;
pub const SC_ANIM: usize = 2;
pub const SC_COUNT: usize = 3;
pub const SC_NAMES: [&str; SC_COUNT] = ["scroll", "drag", "animation"];

/// 强度闭集。
pub const I_LIGHT: usize = 0;
pub const I_MID: usize = 1;
pub const I_HEAVY: usize = 2;
pub const I_COUNT: usize = 3;
pub const I_NAMES: [&str; I_COUNT] = ["light", "mid", "heavy"];

/// 时长物理界（毫秒）。
pub const DUR_MIN_MS: u32 = 10_000;
pub const DUR_MAX_MS: u32 = 120_000;

/// 场景/强度闭集核。
pub fn scene_ok(scene: usize) -> bool {
    scene < SC_COUNT
}
pub fn intensity_ok(i: usize) -> bool {
    i < I_COUNT
}

/// 时长界核。
pub fn dur_ok(ms: u32) -> bool {
    ms >= DUR_MIN_MS && ms <= DUR_MAX_MS
}

/// 场景指纹：FNV-1a 32 位。
pub fn scene_fingerprint(scene: usize, dur_ms: u32, inten: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let s = if scene < SC_COUNT { SC_NAMES[scene] } else { "?" };
    let it = if inten < I_COUNT { I_NAMES[inten] } else { "?" };
    let line = format!("{}|{}|{}", s, dur_ms, it);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 场景条目：场景 × 时长 × 强度。
#[derive(Clone)]
pub struct SceneEntry {
    pub scene: usize,
    pub dur_ms: u32,
    pub intensity: usize,
}

/// 场景总库。
pub struct SceneLib {
    pub entries: [Option<SceneEntry>; SC_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl SceneLib {
    pub fn new() -> Self {
        SceneLib { entries: [None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 落库：场景闭集 + 时长界 + 强度闭集 + 重复拒四闸。
    pub fn enroll(&mut self, scene: usize, dur_ms: u32, intensity: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if !scene_ok(scene) {
            return Err(SL_SCENE_UNK);
        }
        if !dur_ok(dur_ms) {
            return Err(SL_DUR_RANGE);
        }
        if !intensity_ok(intensity) {
            return Err(SL_INTEN_OVER);
        }
        if self.entries[scene].is_some() {
            return Err(SL_DUP);
        }
        self.audit.push(format!(
            "enroll|{}|{}|{}",
            SC_NAMES[scene], dur_ms, I_NAMES[intensity]
        ));
        self.entries[scene] = Some(SceneEntry { scene, dur_ms, intensity });
        Ok(())
    }

    /// 查询场景（未落库显性拒绝）。
    pub fn entry_of(&self, scene: usize) -> Result<(u32, usize), u32> {
        if !scene_ok(scene) {
            return Err(SL_SCENE_UNK);
        }
        match &self.entries[scene] {
            None => Err(SL_EMPTY),
            Some(e) => Ok((e.dur_ms, e.intensity)),
        }
    }

    /// J1：三型覆盖核。
    pub fn covered(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// J1：库全量（逐条合法）。
    pub fn entries_ok(&self) -> bool {
        self.entries.iter().all(|e| {
            e.as_ref().map(|x| scene_ok(x.scene) && dur_ok(x.dur_ms) && intensity_ok(x.intensity))
                .unwrap_or(false)
        })
    }

    /// J3：审计链完整（链长 = 已落库场景数）。
    pub fn audit_ok(&self) -> bool {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.is_some() {
                n += 1;
            }
        }
        self.audit.len() == n && self.audit.iter().all(|a| a.starts_with("enroll|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐场景复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for s in 0..SC_COUNT {
            if let Some(e) = &self.entries[s] {
                h ^= scene_fingerprint(e.scene, e.dur_ms, e.intensity);
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：三型全落库（滚动 30s 中、拖拽 30s 重、动画 60s 轻）。
pub fn standard_lib() -> SceneLib {
    let mut l = SceneLib::new();
    let plan: [(usize, u32, usize); 3] =
        [(SC_ROLL, 30_000, I_MID), (SC_DRAG, 30_000, I_HEAVY), (SC_ANIM, 60_000, I_LIGHT)];
    for p in plan.iter() {
        if l.enroll(p.0, p.1, p.2).is_err() {
            panic!("standard fixture enroll must succeed");
        }
    }
    l
}

/// 注入核：7 注入必抓（未知场景/时长越界/强度越界/重复场景/封账改写/空库查询/覆盖缺失）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知场景
    let mut l = SceneLib::new();
    if l.enroll(9, 30_000, I_MID) == Err(SL_SCENE_UNK) {
        caught += 1;
    }
    // 2) 时长越界
    if l.enroll(SC_ROLL, DUR_MIN_MS - 1, I_MID) == Err(SL_DUR_RANGE) {
        caught += 1;
    }
    // 3) 强度越界
    if l.enroll(SC_ROLL, 30_000, 9) == Err(SL_INTEN_OVER) {
        caught += 1;
    }
    // 4) 重复场景
    if l.enroll(SC_ROLL, 30_000, I_MID).is_ok()
        && l.enroll(SC_ROLL, 60_000, I_HEAVY) == Err(SL_DUP)
    {
        caught += 1;
    }
    // 5) 空库查询
    if SceneLib::new().entry_of(SC_ROLL).err() == Some(SL_EMPTY) {
        caught += 1;
    }
    // 6) 覆盖缺失（单场景库不覆盖）
    if !l.covered() {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_lib();
    sb.seal();
    if sb.enroll(SC_ANIM, 60_000, I_LIGHT) == Err(SL_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 场景库渲染导出。
pub fn render_lib(l: &SceneLib) -> String {
    let mut out = String::new();
    for s in 0..SC_COUNT {
        match &l.entries[s] {
            None => out.push_str(&format!("{}|UNSET\n", SC_NAMES[s])),
            Some(e) => out.push_str(&format!(
                "{}|{}|{}\n",
                SC_NAMES[s], e.dur_ms, I_NAMES[e.intensity]
            )),
        }
    }
    out
}

/// 渲染完整性核：三行齐 + 全落库无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != SC_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for s in 0..SC_COUNT {
        if !text.contains(SC_NAMES[s]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56123_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56123");
    // J1-1 场景库全量（三型逐条合法）
    let l = standard_lib();
    let ok = l.entries_ok() && l.covered() && !l.sealed;
    s.add("56123-J1-场景库全量", ok, if ok { "" } else { "场景残缺" });
    // J1-2 场景闭集齐
    let ok = SC_NAMES.len() == SC_COUNT && SC_NAMES[0] == "scroll";
    s.add("56123-J1-场景闭集齐", ok, if ok { "" } else { "场景漂移" });
    // J1-3 强度闭集齐
    let ok = I_NAMES.len() == I_COUNT && I_NAMES[2] == "heavy";
    s.add("56123-J1-强度闭集齐", ok, if ok { "" } else { "强度漂移" });
    // J2-1 时长界边界
    let ok = dur_ok(DUR_MIN_MS) && dur_ok(DUR_MAX_MS) && !dur_ok(DUR_MIN_MS - 1) && !dur_ok(DUR_MAX_MS + 1);
    s.add("56123-J2-时长界边界", ok, if ok { "" } else { "时长漂移" });
    // J2-2 场景查询命中（动画 60s 轻）
    let ok = l.entry_of(SC_ANIM) == Ok((60_000, I_LIGHT));
    s.add("56123-J2-场景查询命中", ok, if ok { "" } else { "查询漂移" });
    // J2-3 未知场景查询拒
    let ok = l.entry_of(9).err() == Some(SL_SCENE_UNK);
    s.add("56123-J2-未知场景拒", ok, if ok { "" } else { "越界穿透" });
    // J2-4 重复场景拒
    let mut l2 = standard_lib();
    let ok = l2.enroll(SC_ROLL, 45_000, I_LIGHT) == Err(SL_DUP);
    s.add("56123-J2-重复场景拒", ok, if ok { "" } else { "重复穿透" });
    // J3-1 审计链完整（链长 3）
    let ok = l.audit_ok() && l.audit.len() == 3;
    s.add("56123-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_lib().seal_fingerprint() == standard_lib().seal_fingerprint();
    s.add("56123-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（改强度指纹变）
    let fp0 = standard_lib().seal_fingerprint();
    let mut l3 = SceneLib::new();
    let _ = l3.enroll(SC_ROLL, 30_000, I_HEAVY);
    let _ = l3.enroll(SC_DRAG, 30_000, I_HEAVY);
    let _ = l3.enroll(SC_ANIM, 60_000, I_LIGHT);
    let ok = l3.seal_fingerprint() != fp0;
    s.add("56123-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut l4 = standard_lib();
    l4.seal();
    let ok = l4.enroll(SC_ROLL, 45_000, I_LIGHT) == Err(SL_SEALED);
    s.add("56123-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐无 UNSET
    let text = render_lib(&standard_lib());
    let ok = render_complete(&text, true) && text.contains("animation|60000|light");
    s.add("56123-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56123-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56123_all_checks_green() {
        let set = run_unx56123_checks();
        assert!(set.all_passed(), "UNX-F56123 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56123_fingerprint_stable() {
        assert_eq!(standard_lib().seal_fingerprint(), standard_lib().seal_fingerprint());
    }
}
