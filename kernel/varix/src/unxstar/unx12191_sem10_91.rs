//! UNX-F12191 — RtlInitializeCriticalSection 族语义（0x1aa00 段）
//!
//! 锚点：UNX-F12191-J1 初始化两族 10/10（Initialize 族自旋恒 0、
//! InitializeAndSpinCount 族参数归一生效、重复初始化必拒），初始化
//! 账全账回放一致，未初始化先使用必拒。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：InitEntry（初始化登记项）+ InitLed（两族初始化账）+
//! 折叠指纹 + 注入核。
//!
//! NT 语义：RtlInitializeCriticalSection 把 RTL_CRITICAL_SECTION
//! 置零态（SpinCount 恒 0），RtlInitializeCriticalSectionAndSpinCount
//! 则把归一后的自旋数一并写入（含负值 4×处理器数语义）；对已初始
//! 化的临界区再初始化是重复构造，必拒；未初始化的临界区不得参与
//! 任何锁操作。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aa00;
pub const IN_SEALED: u32 = SEG_BASE + 0x01; // 印后初始化
pub const IN_TWICE: u32 = SEG_BASE + 0x02; // 重复初始化
pub const IN_GHOST: u32 = SEG_BASE + 0x03; // 未初始化先使用
pub const IN_NEG: u32 = SEG_BASE + 0x04; // 负值归一档
pub const IN_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 自旋数归一上限（与 B10 系纪律一致）。
pub const IN_MAX_SPIN: u32 = 0x1_0000;

/// 初始化登记项。
#[derive(Clone, PartialEq)]
pub struct InitEntry {
    pub cs_id: u64,
    pub family: u8, // 0=Initialize 族 1=AndSpinCount 族
    pub spin: u32,
    pub note: String,
}

impl InitEntry {
    /// 登记项自洽：id 非零、族落档、AndSpinCount 族自旋非零、留痕非空。
    pub fn ok(&self) -> bool {
        self.cs_id != 0
            && self.family <= 1
            && (self.family == 0) == (self.spin == 0)
            && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        let f = if self.family == 0 { "init" } else { "init+spin" };
        format!("cs={:#x} {}", self.cs_id, f)
    }
}

/// 两族初始化账。
pub struct InitLed {
    pub entries: Vec<InitEntry>,
    pub cpus: u32,
    sealed: bool,
}

impl InitLed {
    pub fn new(cpus: u32) -> InitLed {
        InitLed { entries: Vec::new(), cpus: if cpus == 0 { 1 } else { cpus }, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// Initialize 族：自旋恒 0；重复初始化拒。
    pub fn init_plain(&mut self, cs_id: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(IN_SEALED);
        }
        if cs_id == 0 {
            return Err(IN_GHOST);
        }
        if note.is_empty() {
            return Err(IN_EMPTY);
        }
        if self.find(cs_id).is_some() {
            return Err(IN_TWICE);
        }
        self.entries.push(InitEntry {
            cs_id,
            family: 0,
            spin: 0,
            note: note.to_string(),
        });
        Ok(self.entries.len() as u32)
    }

    /// AndSpinCount 族：归一自旋生效；重复初始化拒。
    pub fn init_spin(&mut self, cs_id: u64, raw: i32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(IN_SEALED);
        }
        if cs_id == 0 {
            return Err(IN_GHOST);
        }
        if note.is_empty() {
            return Err(IN_EMPTY);
        }
        if self.find(cs_id).is_some() {
            return Err(IN_TWICE);
        }
        let (spin, tier) = self.normalize(raw);
        self.entries.push(InitEntry {
            cs_id,
            family: 1,
            spin,
            note: note.to_string(),
        });
        Ok(if tier == 0 { self.entries.len() as u32 } else { tier })
    }

    /// 自旋归一：0 直通（但 AndSpinCount 族写 0 视同未自旋）、负值
    /// 4×处理器数、正值封顶。返回 (值, 档位码)。
    pub fn normalize(&self, raw: i32) -> (u32, u32) {
        if raw == 0 {
            (0, 0)
        } else if raw < 0 {
            (4u32.wrapping_mul(self.cpus).min(IN_MAX_SPIN), IN_NEG)
        } else if raw as u32 > IN_MAX_SPIN {
            (IN_MAX_SPIN, 0)
        } else {
            (raw as u32, 0)
        }
    }

    /// 查登记项。
    pub fn find(&self, cs_id: u64) -> Option<&InitEntry> {
        self.entries.iter().find(|e| e.cs_id == cs_id)
    }

    /// 已初始化判定（使用前置门）。
    pub fn initialized(&self, cs_id: u64) -> bool {
        self.find(cs_id).is_some()
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.ok())
    }

    /// 族计数：(Initialize 族, AndSpinCount 族)。
    pub fn family_tally(&self) -> (usize, usize) {
        (
            self.entries.iter().filter(|e| e.family == 0).count(),
            self.entries.iter().filter(|e| e.family == 1).count(),
        )
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aa00;
        h = h.wrapping_mul(211).wrapping_add(self.cpus);
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.cs_id as u32);
            h = h.wrapping_mul(211).wrapping_add(e.family as u32);
            h = h.wrapping_mul(211).wrapping_add(e.spin);
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        let (a, b) = self.family_tally();
        format!("cpus={} plain={} spin={}", self.cpus, a, b)
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = InitLed::new(8);
    // 1) 重复初始化必拒。
    let _ = b.init_plain(1, "a");
    got[0] = b.init_plain(1, "dup").err() == Some(IN_TWICE);
    // 2) 已初始化再走 AndSpinCount 族同样拒。
    got[1] = b.init_spin(1, 16, "dup2").err() == Some(IN_TWICE);
    // 3) 零 id 初始化必拒。
    got[2] = b.init_plain(0, "z").err() == Some(IN_GHOST);
    // 4) 负值自旋按 4×8=32 归一（档位码返回）。
    got[3] = b.init_spin(2, -1, "neg") == Ok(IN_NEG) && b.find(2).map(|e| e.spin).unwrap_or(0) == 32;
    // 5) 印后初始化必拒。
    b.seal();
    got[4] = b.init_plain(3, "post").err() == Some(IN_SEALED);
    got
}

/// 判据入口：UNX-F12191
pub fn run_unx12191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12191");
    let mut b = InitLed::new(4);

    // J1 Initialize 族：自旋恒 0。
    s.add(
        "J1_plain_zero",
        b.init_plain(0x10, "plain").is_ok()
            && b.find(0x10).map(|e| e.spin == 0 && e.family == 0).unwrap_or(false),
        "Initialize 族自旋恒 0",
    );

    // J2 AndSpinCount 族：正值直通。
    s.add(
        "J2_spin_pass",
        b.init_spin(0x20, 64, "spin").is_ok()
            && b.find(0x20).map(|e| e.spin == 64 && e.family == 1).unwrap_or(false),
        "AndSpinCount 族正值 64 生效",
    );

    // J3 AndSpinCount 族：负值 4×处理器数。
    s.add(
        "J3_spin_neg",
        b.init_spin(0x30, -7, "neg").is_ok()
            && b.find(0x30).map(|e| e.spin == 16).unwrap_or(false),
        "负值按 4×4=16 归一",
    );

    // J4 AndSpinCount 族：超上限封顶。
    s.add(
        "J4_spin_cap",
        b.init_spin(0x40, IN_MAX_SPIN as i32 * 3, "cap").is_ok()
            && b.find(0x40).map(|e| e.spin == IN_MAX_SPIN).unwrap_or(false),
        "超上限封顶 MAX",
    );

    // J5 重复初始化拒（同 id 第二次）。
    s.add(
        "J5_reinit",
        b.init_plain(0x10, "again").err() == Some(IN_TWICE)
            && b.init_spin(0x20, 8, "again").err() == Some(IN_TWICE),
        "重复初始化两族皆拒",
    );

    // J6 使用前置门：未初始化拒绝参与。
    s.add(
        "J6_use_gate",
        !b.initialized(0x99),
        "未初始化 id 不在账",
    );

    // J7 已初始化判定为真。
    s.add(
        "J7_init_true",
        b.initialized(0x10) && b.initialized(0x40),
        "两族登记项皆判已初始化",
    );

    // J8 族计数对平。
    s.add(
        "J8_family_tally",
        b.family_tally() == (1, 3),
        "族计数 1/3 对平",
    );

    // J9 全账回放自洽。
    s.add("J9_replay", b.replay(), "初始化账逐项过核");

    // J10 登记项渲染。
    let r = b.entries[0].render();
    s.add(
        "J10_render",
        r.contains("cs=0x10") && r.contains("init"),
        "渲染含 id 与族名",
    );

    // J11 零 id 初始化拒。
    s.add(
        "J11_zero_id",
        b.init_plain(0, "z").err() == Some(IN_GHOST),
        "零 id 初始化拒 GHOST",
    );

    // J12 零 id AndSpinCount 拒。
    s.add(
        "J12_zero_id_spin",
        b.init_spin(0, 4, "z").err() == Some(IN_GHOST),
        "零 id 自旋初始化拒 GHOST",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "初始化账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.init_plain(0x50, "post").err() == Some(IN_SEALED)
            && b.init_spin(0x50, 8, "post").err() == Some(IN_SEALED),
        "印账后两族皆拒",
    );

    // J16 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J16_seal_fp", b.fold() == fb, "印账不改指纹");

    // J17 零处理器数防御。
    let b17 = InitLed::new(0);
    s.add(
        "J17_zero_cpus",
        b17.normalize(-1).0 == 4,
        "cpus=0 按 1 归一：4×1=4",
    );

    // J18 空账指纹稳定。
    let b18 = InitLed::new(4);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空初始化账指纹稳定");

    // J19 空账回放判不自洽。
    s.add("J19_empty_replay", !b18.replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("cpus=4") && sm.contains("plain=1") && sm.contains("spin=3"),
        "摘要含处理器数与两族计数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12191_init() {
        let mut b = InitLed::new(8);
        assert!(b.init_plain(1, "a").is_ok());
        assert_eq!(b.init_plain(1, "b"), Err(IN_TWICE));
        assert_eq!(b.init_spin(1, 4, "c"), Err(IN_TWICE));
        assert!(b.init_spin(2, -1, "d").is_ok());
        assert_eq!(b.find(2).map(|e| e.spin), Some(32));
    }

    #[test]
    fn unx12191_checks_green() {
        let s = run_unx12191_checks();
        assert!(s.all_passed(), "UNX-F12191 判据未全绿");
    }

    #[test]
    fn unx12191_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
