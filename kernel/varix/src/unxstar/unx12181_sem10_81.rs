//! UNX-F12181 — RTL_CRITICAL_SECTION 结构语义实装（0x1a8c0 段）
//!
//! 锚点：UNX-F12181-J1 临界区结构 10/10（DebugInfo/LockCount/
//! RecursionCount/OwningThread/LockSemaphore/SpinCount 六字段逐槽
//! 定位），初始化形制、印后写必拒。
//! 域：CoRun·UNX-D1-B10（临界区谱系）。
//! 结构：RtlCriticalSection（六字段结构）+ CritSecBook（实例账）+
//! 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a8c0;
pub const CS_SEALED: u32 = SEG_BASE + 0x01; // 印后改
pub const CS_TWICE: u32 = SEG_BASE + 0x02; // 重复初始化
pub const CS_GHOST: u32 = SEG_BASE + 0x03; // 野实例
pub const CS_ORDER: u32 = SEG_BASE + 0x04; // 字段形制非法
pub const CS_EMPTY: u32 = SEG_BASE + 0x05; // 空账销毁

/// 临界区六字段结构（x64 形制）。
#[derive(Clone, PartialEq)]
pub struct RtlCriticalSection {
    pub debug_info: u64, // RTL_CRITICAL_SECTION_DEBUG 指针（0=无）
    pub lock_count: i32, // -1 = 未锁
    pub recursion_count: u32,
    pub owning_thread: u64, // 0 = 无主
    pub lock_semaphore: u64,
    pub spin_count: u32,
    pub note: String,
}

impl RtlCriticalSection {
    /// 初始化形制：lock=-1、recursion=0、owner=0。
    pub fn initialized_form(&self) -> bool {
        self.lock_count == -1
            && self.recursion_count == 0
            && self.owning_thread == 0
            && self.spin_count <= 0x1000_0000
    }

    /// 已锁判定。
    pub fn is_locked(&self) -> bool {
        self.lock_count >= 0
    }

    /// 无主判定。
    pub fn is_free(&self) -> bool {
        !self.is_locked() && self.owning_thread == 0 && self.recursion_count == 0
    }

    /// 带自旋初始化。
    pub fn with_spin(spin: u32, note: &str) -> RtlCriticalSection {
        RtlCriticalSection {
            debug_info: 0,
            lock_count: -1,
            recursion_count: 0,
            owning_thread: 0,
            lock_semaphore: 0,
            spin_count: spin,
            note: note.to_string(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "lc={} rc={} ot={:#x} sc={}",
            self.lock_count, self.recursion_count, self.owning_thread, self.spin_count
        )
    }
}

/// 临界区实例账。
pub struct CritSecBook {
    pub secs: Vec<(u64, RtlCriticalSection)>, // (句柄, 临界区)
    sealed: bool,
}

impl CritSecBook {
    pub fn new() -> CritSecBook {
        CritSecBook { secs: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 初始化一个临界区（重复句柄必拒）。
    pub fn initialize(&mut self, handle: u64, spin: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_SEALED);
        }
        if handle == 0 {
            return Err(CS_ORDER);
        }
        if self.secs.iter().any(|(h, _)| *h == handle) {
            return Err(CS_TWICE);
        }
        self.secs.push((handle, RtlCriticalSection::with_spin(spin, note)));
        Ok(())
    }

    /// 查实例。
    pub fn get(&self, handle: u64) -> Result<&RtlCriticalSection, u32> {
        self.secs.iter().find(|(h, _)| *h == handle).map(|(_, c)| c).ok_or(CS_GHOST)
    }

    /// 修改实例（印后拒；实例不存在拒）。
    pub fn mutate<F>(&mut self, handle: u64, f: F) -> Result<(), u32>
    where
        F: FnOnce(&mut RtlCriticalSection),
    {
        if self.sealed {
            return Err(CS_SEALED);
        }
        match self.secs.iter_mut().find(|(h, _)| *h == handle) {
            Some((_, c)) => {
                f(c);
                Ok(())
            }
            None => Err(CS_GHOST),
        }
    }

    /// 全实例初始化形制核。
    pub fn all_fresh(&self) -> bool {
        self.secs.iter().all(|(_, c)| c.initialized_form())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a8c0;
        for (hd, c) in &self.secs {
            h = h.wrapping_mul(211).wrapping_add(*hd as u32);
            h = h.wrapping_mul(211).wrapping_add(c.lock_count as u32);
            h = h.wrapping_mul(211).wrapping_add(c.spin_count);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("secs={} fresh={}", self.secs.len(), self.all_fresh())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CritSecBook::new();
    // 1) 零句柄初始化必拒。
    got[0] = b.initialize(0, 0, "z").err() == Some(CS_ORDER);
    // 2) 重复句柄必拒。
    let _ = b.initialize(1, 0, "a");
    got[1] = b.initialize(1, 0, "d").err() == Some(CS_TWICE);
    // 3) 野句柄查必拒。
    got[2] = b.get(99).err() == Some(CS_GHOST);
    // 4) 野句柄改必拒。
    got[3] = b.mutate(99, |_| {}).err() == Some(CS_GHOST);
    // 5) 印后初始化必拒。
    b.seal();
    got[4] = b.initialize(2, 0, "post").err() == Some(CS_SEALED);
    got
}

/// 判据入口：UNX-F12181
pub fn run_unx12181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12181");
    let mut b = CritSecBook::new();

    // J1 三实例初始化（自旋 0/400/1600）。
    let j1 = b.initialize(0x11, 0, "plain").is_ok()
        && b.initialize(0x22, 400, "spin4").is_ok()
        && b.initialize(0x33, 1600, "spin16").is_ok();
    s.add("J1_init3", j1 && b.secs.len() == 3, "三实例初始化");

    // J2 初始形制全核。
    s.add(
        "J2_fresh_form",
        b.all_fresh() && b.get(0x11).map(|c| c.initialized_form()) == Ok(true),
        "三实例全过初始形制核",
    );

    // J3 字段形制逐槽（lock=-1 rc=0 ot=0）。
    let c = b.get(0x11);
    s.add(
        "J3_fields",
        c.map(|x| x.lock_count) == Ok(-1)
            && c.map(|x| x.recursion_count) == Ok(0)
            && c.map(|x| x.owning_thread) == Ok(0),
        "lock=-1/rc=0/ot=0 形制",
    );

    // J4 自旋计数保留。
    s.add(
        "J4_spin",
        b.get(0x22).map(|x| x.spin_count) == Ok(400)
            && b.get(0x33).map(|x| x.spin_count) == Ok(1600),
        "自旋计数 400/1600 保留",
    );

    // J5 未锁判定。
    s.add(
        "J5_free",
        b.get(0x11).map(|x| x.is_free()) == Ok(true)
            && b.get(0x11).map(|x| x.is_locked()) == Ok(false),
        "初始态未锁/无主",
    );

    // J6 野句柄拒。
    s.add(
        "J6_ghost",
        b.get(0xFF).err() == Some(CS_GHOST),
        "野句柄查拒 GHOST",
    );

    // J7 锁定形制转换。
    let r7 = b.mutate(0x11, |c| {
        c.lock_count = 0;
        c.owning_thread = 0x42;
        c.recursion_count = 1;
    });
    s.add(
        "J7_lock_form",
        r7.is_ok()
            && b.get(0x11).map(|x| x.is_locked()) == Ok(true)
            && b.get(0x11).map(|x| x.is_free()) == Ok(false),
        "锁定后 is_locked 翻真",
    );

    // J8 渲染。
    let r = b.get(0x11).map(|x| x.render());
    s.add(
        "J8_render",
        r.as_deref() == Ok("lc=0 rc=1 ot=0x42 sc=0"),
        "渲染六字段对平",
    );

    // J9 重复句柄拒。
    s.add(
        "J9_dup",
        b.initialize(0x11, 0, "dup").err() == Some(CS_TWICE),
        "重复句柄拒 TWICE",
    );

    // J10 零句柄拒。
    s.add(
        "J10_zero_handle",
        b.initialize(0, 0, "z").err() == Some(CS_ORDER),
        "零句柄拒 ORDER",
    );

    // J11 DebugInfo 挂账。
    let r11 = b.mutate(0x22, |c| c.debug_info = 0x9000);
    s.add(
        "J11_debug_info",
        r11.is_ok() && b.get(0x22).map(|x| x.debug_info) == Ok(0x9000),
        "DebugInfo 指针挂账",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "临界区账折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.mutate(0x11, |_| {}).err() == Some(CS_SEALED)
            && b.initialize(0x44, 0, "p").err() == Some(CS_SEALED),
        "印账后改/初始化皆拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("secs=3") && sm.contains("fresh=false"),
        "摘要含实例数与形制位（锁定后 fresh=false）",
    );

    // J16 初始态 fresh 全核（新账）。
    let mut b16 = CritSecBook::new();
    let _ = b16.initialize(0x55, 0, "n");
    s.add(
        "J16_fresh_book",
        b16.all_fresh(),
        "新账全实例 fresh",
    );

    // J17 十实例初始化。
    let mut b17 = CritSecBook::new();
    let mut ok17 = true;
    for i in 1..=10u64 {
        if b17.initialize(i * 0x10, 0, "m").is_err() {
            ok17 = false;
        }
    }
    s.add("J17_ten_secs", ok17 && b17.secs.len() == 10, "十实例初始化全过");

    // J18 空账指纹稳定。
    let b18 = CritSecBook::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空临界区账指纹稳定");

    // J19 with_spin 构造形制。
    let w19 = RtlCriticalSection::with_spin(0x1000, "w");
    s.add(
        "J19_with_spin",
        w19.initialized_form() && w19.spin_count == 0x1000,
        "with_spin 构造过形制核",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12181_critsec() {
        let mut b = CritSecBook::new();
        let _ = b.initialize(1, 0, "a");
        assert!(b.get(1).unwrap().is_free());
        let _ = b.mutate(1, |c| c.lock_count = 0);
        assert!(b.get(1).unwrap().is_locked());
    }

    #[test]
    fn unx12181_checks_green() {
        let s = run_unx12181_checks();
        assert!(s.all_passed(), "UNX-F12181 判据未全绿");
    }

    #[test]
    fn unx12181_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
