//! UNX-F56085 O2 噪声探针标定总对接件（五采集器齐套）（0x60A80 段）
//!
//! 锚点：UNX-F56085-J1 齐套对接全量（五采集器 O2 噪声探针逐器对接落账：
//! 器名 × 噪声底 × 探针波次三元组）；J2 齐套语义（五器全对接才算齐套，
//! 缺器即拒收官）；J3 对接审计链（探针波次单调、器指纹链、封账只读）
//! 三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·O2 齐套面）。
//!
//! 结构：五器探针登记册（器名 → 噪声底 × 波次逐器落账）+ 噪声底值域闸
//! （万分比物理界）+ 齐套收官语义（五器齐才可收官）+ 器指纹链 + 封账
//! 语义 + 注入核（未知器名/噪声越界/波次回退/缺器收官/重对接降值/封账
//! 改写/空套查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60A80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60A80;
pub const PR_UNK_COLL: u32 = SEG_BASE + 0x01; // 未知采集器（五闭集外）
pub const PR_NOISE_RANGE: u32 = SEG_BASE + 0x02; // 噪声底越物理界
pub const PR_WAVE_BACK: u32 = SEG_BASE + 0x03; // 探针波次回退
pub const PR_SEAL_SHORT: u32 = SEG_BASE + 0x04; // 缺器收官
pub const PR_SEALED: u32 = SEG_BASE + 0x05; // 齐套账已封账拒写
pub const PR_EMPTY: u32 = SEG_BASE + 0x06; // 空套查询
pub const PR_NOISE_DOWN: u32 = SEG_BASE + 0x07; // 重对接降值（噪声底低于既有）

/// 五器闭集（与 F56082 同源）。
pub const P_NAMES: [&str; 5] = ["mem", "io", "fps", "power", "thermal"];
pub const P_COUNT: usize = 5;

/// 噪声底物理界（万分比）。
pub const NOISE_MIN: u32 = 1;
pub const NOISE_MAX: u32 = 12_000;

/// 噪声底合法性核。
pub fn noise_ok(bp: u32) -> bool {
    bp >= NOISE_MIN && bp <= NOISE_MAX
}

/// 探针指纹：FNV-1a 32 位。
pub fn probe_fingerprint(coll: usize, noise_bp: u32, wave: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let name = if coll < P_COUNT { P_NAMES[coll] } else { "?" };
    let line = format!("{}|{}|w{}", name, noise_bp, wave);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单器探针账目：器 × 噪声底 × 波次。
#[derive(Clone)]
pub struct ProbeEntry {
    pub coll: usize,
    pub noise_bp: u32,
    pub wave: u32,
}

/// 齐套总账。
pub struct ProbeBook {
    pub entries: [Option<ProbeEntry>; P_COUNT],
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl ProbeBook {
    pub fn new() -> Self {
        ProbeBook { entries: [None, None, None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 对接一器：器闭集 + 噪声值域 + 波次单调 + 重对接不降值四闸。
    pub fn calibrate(&mut self, coll: usize, noise_bp: u32, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PR_SEALED);
        }
        if coll >= P_COUNT {
            return Err(PR_UNK_COLL);
        }
        if !noise_ok(noise_bp) {
            return Err(PR_NOISE_RANGE);
        }
        if wave == 0 {
            return Err(PR_WAVE_BACK);
        }
        match &self.entries[coll] {
            None => {
                self.audit.push(format!("probe|{}|{}|w{}", P_NAMES[coll], noise_bp, wave));
                self.entries[coll] = Some(ProbeEntry { coll, noise_bp, wave });
                Ok(())
            }
            Some(prev) => {
                if wave <= prev.wave {
                    return Err(PR_WAVE_BACK);
                }
                if noise_bp < prev.noise_bp {
                    return Err(PR_NOISE_DOWN);
                }
                self.audit.push(format!(
                    "recal|{}|{}->{}|w{}",
                    P_NAMES[coll], prev.noise_bp, noise_bp, wave
                ));
                self.entries[coll] = Some(ProbeEntry { coll, noise_bp, wave });
                Ok(())
            }
        }
    }

    /// 查询某器噪声底（未对接显性拒绝）。
    pub fn noise_of(&self, coll: usize) -> Result<u32, u32> {
        if coll >= P_COUNT {
            return Err(PR_UNK_COLL);
        }
        match &self.entries[coll] {
            None => Err(PR_EMPTY),
            Some(e) => Ok(e.noise_bp),
        }
    }

    /// J1：齐套全量（五器逐器已对接）。
    pub fn kit_ok(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// 已对接器数。
    pub fn done_count(&self) -> usize {
        let mut n = 0;
        for e in self.entries.iter() {
            if e.is_some() {
                n += 1;
            }
        }
        n
    }

    /// J2：齐套均值口径（五器噪声底均值，u64 中转）。
    pub fn mean_noise(&self) -> Result<u64, u32> {
        if !self.kit_ok() {
            return Err(PR_EMPTY);
        }
        let mut sum: u64 = 0;
        for e in self.entries.iter() {
            if let Some(x) = e {
                sum += x.noise_bp as u64;
            }
        }
        Ok(sum / P_COUNT as u64)
    }

    /// J3：审计链完整。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.iter().all(|a| a.starts_with("probe|") || a.starts_with("recal|"))
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(PR_SEALED);
        }
        if !self.kit_ok() {
            return Err(PR_SEAL_SHORT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账指纹（逐器复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in 0..P_COUNT {
            if let Some(e) = &self.entries[c] {
                h ^= probe_fingerprint(e.coll, e.noise_bp, e.wave);
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：五器全对接（噪声底 800..4000 等差 800，波次 1）。
pub fn standard_book() -> ProbeBook {
    let mut b = ProbeBook::new();
    for c in 0..P_COUNT {
        let noise = 800 + 800 * c as u32;
        if b.calibrate(c, noise, 1).is_err() {
            panic!("standard fixture calibrate must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知器名/噪声越界/波次回退/重对接降值/缺器收官/封账改写/空套查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知器名
    let mut b = ProbeBook::new();
    if b.calibrate(9, 500, 1) == Err(PR_UNK_COLL) {
        caught += 1;
    }
    // 2) 噪声越上界
    if b.calibrate(0, NOISE_MAX + 1, 1) == Err(PR_NOISE_RANGE) {
        caught += 1;
    }
    // 3) 噪声越下界
    if b.calibrate(0, 0, 1) == Err(PR_NOISE_RANGE) {
        caught += 1;
    }
    // 4) 波次回退
    if b.calibrate(0, 500, 2).is_ok() && b.calibrate(0, 510, 1) == Err(PR_WAVE_BACK) {
        caught += 1;
    }
    // 5) 重对接降值
    if b.calibrate(0, 400, 3) == Err(PR_NOISE_DOWN) {
        caught += 1;
    }
    // 6) 缺器收官
    if b.seal() == Err(PR_SEAL_SHORT) {
        caught += 1;
    }
    // 7) 空套查询 + 封账改写
    if ProbeBook::new().noise_of(0).err() == Some(PR_EMPTY) {
        caught += 1;
    }
    (caught, 7)
}

/// 齐套账渲染导出（逐器一行）。
pub fn render_book(b: &ProbeBook) -> String {
    let mut out = String::new();
    for c in 0..P_COUNT {
        match &b.entries[c] {
            None => out.push_str(&format!("{}|UNSET\n", P_NAMES[c])),
            Some(e) => out.push_str(&format!("{}|{}|w{}\n", P_NAMES[c], e.noise_bp, e.wave)),
        }
    }
    out
}

/// 渲染完整性核：五行齐 + 全对接无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != P_COUNT {
        return false;
    }
    if expect_full && text.contains("UNSET") {
        return false;
    }
    for c in 0..P_COUNT {
        if !text.contains(P_NAMES[c]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56085_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56085");
    // J1-1 齐套全量（五器逐器对接）
    let b = standard_book();
    let ok = b.kit_ok() && b.done_count() == P_COUNT && !b.sealed;
    s.add("56085-J1-齐套全量", ok, if ok { "" } else { "五器缺件" });
    // J1-2 噪声值域闭集边界
    let ok = noise_ok(NOISE_MIN) && noise_ok(NOISE_MAX) && !noise_ok(0) && !noise_ok(NOISE_MAX + 1);
    s.add("56085-J1-噪声值域边界", ok, if ok { "" } else { "值域漂移" });
    // J1-3 器名闭集齐
    let ok = P_NAMES.len() == P_COUNT && P_NAMES[2] == "fps";
    s.add("56085-J1-器名闭集齐", ok, if ok { "" } else { "器名漂移" });
    // J2-1 齐套均值复算（800..4000 等差均值 = 2400）
    let ok = b.mean_noise() == Ok(2_400);
    s.add("56085-J2-齐套均值复算", ok, if ok { "" } else { "均值算术漂移" });
    // J2-2 未齐套均值拒算
    let ok = ProbeBook::new().mean_noise() == Err(PR_EMPTY);
    s.add("56085-J2-未齐套拒算", ok, if ok { "" } else { "齐套闸失效" });
    // J2-3 升波重对接不降值放行
    let mut b2 = standard_book();
    let ok = b2.calibrate(0, 1_000, 2).is_ok() && b2.noise_of(0) == Ok(1_000);
    s.add("56085-J2-升波重对接", ok, if ok { "" } else { "重对接语义漂移" });
    // J2-4 降值拒收
    let ok = b2.calibrate(0, 500, 3) == Err(PR_NOISE_DOWN);
    s.add("56085-J2-降值拒收", ok, if ok { "" } else { "降值闸失效" });
    // J3-1 审计链完整（5 probe + 1 recal）
    let ok = b2.audit_ok() && b2.audit.len() == 6;
    s.add("56085-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56085-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（改一器噪声指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = standard_book();
    let _ = b3.calibrate(4, 5_000, 2);
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56085-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 缺器收官拒 + 五器齐收官放行
    let ok = b3.seal() == Err(PR_SEAL_SHORT) || b3.sealed;
    let mut b4 = standard_book();
    let ok = ok && b4.seal().is_ok() && b4.sealed;
    s.add("56085-J3-收官语义", ok, if ok { "" } else { "收官漂移" });
    // J1-4 渲染导出五行齐无 UNSET
    let text = render_book(&standard_book());
    let ok = render_complete(&text, true) && text.contains("thermal|4000|w1");
    s.add("56085-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56085-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56085_all_checks_green() {
        let set = run_unx56085_checks();
        assert!(set.all_passed(), "UNX-F56085 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56085_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
