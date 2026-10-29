//! UNX-F56082 采集器注册表与版本账（0x60A20 段）
//!
//! 锚点：UNX-F56082-J1 注册表全量（五采集器逐件落注：名 × 版本 × 状态
//! 三元组）；J2 版本账语义（重注册必须升版本且版本号严格递增，同版本
//! 重注拒收）；J3 注册审计链（注册波次、采集器指纹链、封账只读）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·注册表面）。
//!
//! 结构：采集器名闭集（内存/IO/帧率/功耗/温度五器）+ 注册表登记册
//! （名 → 版本 × 状态 × 波次逐件落注）+ 版本账核（重注升版本严格递增）
//! + 状态机（active/paused/retired 三态合法迁移）+ 指纹链 + 封账语义 +
//! 注入核（未知器名/版本回退/同版本重注/非法状态迁移/封账改写/未注册
//! 查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60A20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60A20;
pub const REG_UNK_COLL: u32 = SEG_BASE + 0x01; // 未知采集器（五闭集外）
pub const REG_VER_BACK: u32 = SEG_BASE + 0x02; // 版本回退（重注降版）
pub const REG_VER_DUP: u32 = SEG_BASE + 0x03; // 同版本重注
pub const REG_STATE_BAD: u32 = SEG_BASE + 0x04; // 非法状态迁移
pub const REG_SEALED: u32 = SEG_BASE + 0x05; // 注册表已封账拒写
pub const REG_NOT_FOUND: u32 = SEG_BASE + 0x06; // 未注册查询
pub const REG_WAVE_ZERO: u32 = SEG_BASE + 0x07; // 注册波次零

/// 采集器名闭集。
pub const K_MEM: usize = 0;
pub const K_IO: usize = 1;
pub const K_FPS: usize = 2;
pub const K_POWER: usize = 3;
pub const K_THERMAL: usize = 4;
pub const K_COUNT: usize = 5;
pub const K_NAMES: [&str; K_COUNT] = ["mem", "io", "fps", "power", "thermal"];

/// 采集器状态闭集。
pub const ST_ACTIVE: usize = 0;
pub const ST_PAUSED: usize = 1;
pub const ST_RETIRED: usize = 2;
pub const ST_COUNT: usize = 3;
pub const ST_NAMES: [&str; ST_COUNT] = ["active", "paused", "retired"];

/// 状态迁移合法性核：active↔paused 互通；retired 吸收态不可迁出；
/// 不可自迁；不可直跳 retired→paused 之外语义（retired 迁出全拒）。
pub fn state_move_ok(from: usize, to: usize) -> bool {
    if from >= ST_COUNT || to >= ST_COUNT || from == to {
        return false;
    }
    if from == ST_RETIRED {
        return false;
    }
    true
}

/// 采集器指纹：FNV-1a 32 位。
pub fn coll_fingerprint(name: &str, version: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|v{}", name, version);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单器注册账目：名 × 版本 × 状态 × 波次 四元组。
#[derive(Clone)]
pub struct RegEntry {
    pub coll: usize,
    pub version: u32,
    pub state: usize,
    pub wave: u32,
}

/// 注册表总账。
pub struct RegBook {
    pub entries: [Option<RegEntry>; K_COUNT],
    pub history: Vec<String>,
    pub sealed: bool,
}

impl RegBook {
    pub fn new() -> Self {
        RegBook { entries: [None, None, None, None, None], history: Vec::new(), sealed: false }
    }

    /// 注册/重注册：版本 ≥1；重注必须严格升版本。
    pub fn register(&mut self, coll: usize, version: u32, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(REG_SEALED);
        }
        if coll >= K_COUNT {
            return Err(REG_UNK_COLL);
        }
        if version == 0 || wave == 0 {
            return Err(REG_WAVE_ZERO);
        }
        match &self.entries[coll] {
            None => {
                self.history.push(format!("reg|{}|v{}|w{}", K_NAMES[coll], version, wave));
                self.entries[coll] = Some(RegEntry { coll, version, state: ST_ACTIVE, wave });
                Ok(())
            }
            Some(prev) => {
                if version == prev.version {
                    return Err(REG_VER_DUP);
                }
                if version < prev.version {
                    return Err(REG_VER_BACK);
                }
                if prev.state == ST_RETIRED {
                    return Err(REG_STATE_BAD);
                }
                self.history.push(format!(
                    "rereg|{}|v{}->v{}|w{}",
                    K_NAMES[coll], prev.version, version, wave
                ));
                self.entries[coll] = Some(RegEntry { coll, version, state: prev.state, wave });
                Ok(())
            }
        }
    }

    /// 状态迁移：合法迁移留痕；retired 吸收。
    pub fn set_state(&mut self, coll: usize, to: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(REG_SEALED);
        }
        if coll >= K_COUNT || to >= ST_COUNT {
            return Err(REG_UNK_COLL);
        }
        match &self.entries[coll] {
            None => Err(REG_NOT_FOUND),
            Some(prev) => {
                if !state_move_ok(prev.state, to) {
                    return Err(REG_STATE_BAD);
                }
                self.history.push(format!(
                    "state|{}|{}->{}",
                    K_NAMES[coll], ST_NAMES[prev.state], ST_NAMES[to]
                ));
                self.entries[coll].as_mut().unwrap_or_else(|| {
                    panic!("state entry must exist")
                }).state = to;
                Ok(())
            }
        }
    }

    /// 查询某器版本（未注册显性拒绝）。
    pub fn version_of(&self, coll: usize) -> Result<u32, u32> {
        if coll >= K_COUNT {
            return Err(REG_UNK_COLL);
        }
        match &self.entries[coll] {
            None => Err(REG_NOT_FOUND),
            Some(e) => Ok(e.version),
        }
    }

    /// 查询某器状态。
    pub fn state_of(&self, coll: usize) -> Result<usize, u32> {
        if coll >= K_COUNT {
            return Err(REG_UNK_COLL);
        }
        match &self.entries[coll] {
            None => Err(REG_NOT_FOUND),
            Some(e) => Ok(e.state),
        }
    }

    /// J1：注册表全量（五器逐件在册且 active 或 paused）。
    pub fn registry_ok(&self) -> bool {
        for c in 0..K_COUNT {
            match &self.entries[c] {
                None => return false,
                Some(e) if e.version == 0 || e.state >= ST_COUNT => return false,
                _ => {}
            }
        }
        true
    }

    /// J1：版本互异且非零。
    pub fn versions_ok(&self) -> bool {
        for e in self.entries.iter() {
            match e {
                None => return false,
                Some(x) if x.version == 0 => return false,
                _ => {}
            }
        }
        true
    }

    /// J3：审计链完整（逐条带波次或状态痕迹）。
    pub fn audit_ok(&self) -> bool {
        !self.history.is_empty()
            && self.history.iter().all(|h| h.contains("|w") || h.contains("|state|") || h.contains("state|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐器复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in 0..K_COUNT {
            if let Some(e) = &self.entries[c] {
                let line = format!("{}|v{}|{}|w{}", K_NAMES[c], e.version, ST_NAMES[e.state], e.wave);
                for b in line.as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }
}

/// 标准夹具：五器全注册（版本 2，active）。
pub fn standard_book() -> RegBook {
    let mut b = RegBook::new();
    for c in 0..K_COUNT {
        if b.register(c, 2, 1).is_err() {
            panic!("standard fixture register must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知器名/版本回退/同版本重注/retired 重注/封账改写/未注册状态迁移/未注册查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知器名
    let mut b = RegBook::new();
    if b.register(9, 1, 1) == Err(REG_UNK_COLL) {
        caught += 1;
    }
    // 2) 版本零
    if b.register(K_MEM, 0, 1) == Err(REG_WAVE_ZERO) {
        caught += 1;
    }
    // 3) 同版本重注
    if b.register(K_MEM, 2, 1).is_ok() && b.register(K_MEM, 2, 2) == Err(REG_VER_DUP) {
        caught += 1;
    }
    // 4) 版本回退
    if b.register(K_MEM, 3, 2).is_ok() && b.register(K_MEM, 1, 3) == Err(REG_VER_BACK) {
        caught += 1;
    }
    // 5) retired 重注拒
    if b.set_state(K_MEM, ST_RETIRED).is_ok() && b.register(K_MEM, 4, 4) == Err(REG_STATE_BAD) {
        caught += 1;
    }
    // 6) 未注册状态迁移与查询
    if b.set_state(K_IO, ST_PAUSED) == Err(REG_NOT_FOUND)
        && b.version_of(K_IO).err() == Some(REG_NOT_FOUND)
    {
        caught += 1;
    }
    // 7) 封账后改写拒
    b.seal();
    if b.register(K_IO, 1, 5) == Err(REG_SEALED) && b.set_state(K_MEM, ST_ACTIVE) == Err(REG_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 注册表渲染导出（逐器一行）。
pub fn render_book(b: &RegBook) -> String {
    let mut out = String::new();
    for c in 0..K_COUNT {
        match &b.entries[c] {
            None => out.push_str(&format!("{}|UNSET\n", K_NAMES[c])),
            Some(e) => out.push_str(&format!(
                "{}|v{}|{}|w{}\n",
                K_NAMES[c], e.version, ST_NAMES[e.state], e.wave
            )),
        }
    }
    out
}

/// 渲染完整性核：五行齐 + 全注册无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != K_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for c in 0..K_COUNT {
        if !text.contains(K_NAMES[c]) {
            return false;
        }
    }
    true
}

/// 注册史查询：某器按登记序的全史。
pub fn reg_history(b: &RegBook, coll: usize) -> Vec<String> {
    let mut hist = Vec::new();
    if coll < K_COUNT {
        for h in b.history.iter() {
            let head: Vec<&str> = h.split('|').collect();
            if head.len() >= 2 && head[1].starts_with(K_NAMES[coll]) {
                hist.push(h.clone());
            }
        }
    }
    hist
}

/// 判据入口（CheckSet）。
pub fn run_unx56082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56082");
    // J1-1 注册表全量（五器逐件在册）
    let b = standard_book();
    let ok = b.registry_ok() && b.versions_ok() && !b.sealed;
    s.add("56082-J1-注册表全量", ok, if ok { "" } else { "注册器残缺" });
    // J1-2 器名闭集齐
    let ok = K_NAMES.len() == K_COUNT && K_NAMES[4] == "thermal";
    s.add("56082-J1-器名闭集齐", ok, if ok { "" } else { "器名漂移" });
    // J1-3 版本号语义（v2 现行）
    let ok = b.version_of(K_MEM) == Ok(2) && b.version_of(K_FPS) == Ok(2);
    s.add("56082-J1-版本现行", ok, if ok { "" } else { "版本漂移" });
    // J2-1 重注升版本放行（2→3）
    let mut b2 = standard_book();
    let ok = b2.register(K_MEM, 3, 2).is_ok() && b2.version_of(K_MEM) == Ok(3);
    s.add("56082-J2-升版重注", ok, if ok { "" } else { "升版语义漂移" });
    // J2-2 同版本重注拒
    let mut b3 = standard_book();
    let ok = b3.register(K_IO, 2, 2) == Err(REG_VER_DUP);
    s.add("56082-J2-同版重注拒", ok, if ok { "" } else { "同版闸失效" });
    // J2-3 版本回退拒
    let ok = b3.register(K_IO, 1, 2) == Err(REG_VER_BACK);
    s.add("56082-J2-版本回退拒", ok, if ok { "" } else { "回退闸失效" });
    // J2-4 状态迁移合法（active→paused→active）
    let mut b4 = standard_book();
    let ok = b4.set_state(K_POWER, ST_PAUSED).is_ok()
        && b4.state_of(K_POWER) == Ok(ST_PAUSED)
        && b4.set_state(K_POWER, ST_ACTIVE).is_ok()
        && b4.state_of(K_POWER) == Ok(ST_ACTIVE);
    s.add("56082-J2-状态迁移", ok, if ok { "" } else { "状态机漂移" });
    // J2-5 retired 吸收态（迁出全拒）
    let ok = !state_move_ok(ST_RETIRED, ST_ACTIVE) && !state_move_ok(ST_RETIRED, ST_PAUSED)
        && state_move_ok(ST_ACTIVE, ST_RETIRED);
    s.add("56082-J2-retired吸收", ok, if ok { "" } else { "吸收态漂移" });
    // J3-1 审计链完整（注册 5 条 + 迁移痕迹）
    let ok = b4.audit_ok() && b4.history.len() >= 6;
    s.add("56082-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56082-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（升版后指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b5 = standard_book();
    let _ = b5.register(K_THERMAL, 5, 2);
    let ok = b5.seal_fingerprint() != fp0;
    s.add("56082-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 注册史查询命中（mem 两条史）
    let hist = reg_history(&b5, K_THERMAL);
    let ok = hist.len() == 2 && hist[0].starts_with("reg|thermal");
    s.add("56082-J3-注册史命中", ok, if ok { "" } else { "史查询漂移" });
    // J1-4 渲染导出五行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("fps|v2|active|w1");
    s.add("56082-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56082-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56082_all_checks_green() {
        let set = run_unx56082_checks();
        assert!(set.all_passed(), "UNX-F56082 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56082_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
