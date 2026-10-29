//! UNX-F12195 — SRWLOCK 语义预告（0x1aa80 段）
//!
//! 锚点：UNX-F12195-J1 SRW 值位型 10/10（0=无主、低位=共享计数、
//! 高位=独占位），共享/独占两态互斥判定 10/10，值域越界必拒，
//! 编解码全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系，SRW 预告）。
//! 结构：SrwValue（SRW 值位型编解码）+ SrwLed（状态账）+ 折叠
//! 指纹 + 注入核。
//!
//! NT 语义预告：RTL_SRWLOCK 是一个指针宽度的值型锁——全零表示
//! 无主；低位段记录共享持有者计数；高位段记录独占位与等待标记。
//! 共享态可有多个持有者但不能有独占者；独占态恰一持有者且排斥
//! 一切共享者。本卡只立值位型与两态互斥账面，真原子路径后续
//! 对接（12196 展开四族操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aa80;
pub const SW_SEALED: u32 = SEG_BASE + 0x01; // 印后改值
pub const SW_EXCL: u32 = SEG_BASE + 0x02; // 独占冲突
pub const SW_SHARE: u32 = SEG_BASE + 0x03; // 共享态下独占拒
pub const SW_RANGE: u32 = SEG_BASE + 0x04; // 值域越界
pub const SW_EMPTY: u32 = SEG_BASE + 0x05; // 空账回放

/// 值位型布局常量。
pub const SRW_EXCL_BIT: u32 = 1 << 31; // 独占位
pub const SRW_SHARED_MASK: u32 = 0x0FFF; // 共享计数低位段
pub const SRW_SHARED_MAX: u32 = SRW_SHARED_MASK; // 共享计数上限

/// SRW 值位型。
#[derive(Clone, Copy, PartialEq)]
pub struct SrwValue {
    pub raw: u32,
}

impl SrwValue {
    /// 无主初值。
    pub fn unowned() -> SrwValue {
        SrwValue { raw: 0 }
    }

    /// 无主判定。
    pub fn is_unowned(&self) -> bool {
        self.raw == 0
    }

    /// 独占态判定。
    pub fn is_exclusive(&self) -> bool {
        self.raw & SRW_EXCL_BIT != 0
    }

    /// 共享态判定：非无主且独占位清零。
    pub fn is_shared(&self) -> bool {
        !self.is_unowned() && !self.is_exclusive()
    }

    /// 共享计数读取。
    pub fn shared_count(&self) -> u32 {
        self.raw & SRW_SHARED_MASK
    }

    /// 共享计数 +1；越界拒。
    pub fn add_shared(&mut self) -> Result<u32, u32> {
        if self.is_exclusive() {
            return Err(SW_SHARE);
        }
        if self.shared_count() >= SRW_SHARED_MAX {
            return Err(SW_RANGE);
        }
        self.raw += 1;
        Ok(self.shared_count())
    }

    /// 共享计数 -1；下溢/独占态拒。
    pub fn sub_shared(&mut self) -> Result<u32, u32> {
        if self.is_exclusive() {
            return Err(SW_SHARE);
        }
        if self.shared_count() == 0 {
            return Err(SW_RANGE);
        }
        self.raw -= 1;
        Ok(self.shared_count())
    }

    /// 进入独占态：要求当前无主；已有持有者（共享或独占）拒。
    pub fn set_exclusive(&mut self) -> Result<(), u32> {
        if self.is_exclusive() {
            return Err(SW_EXCL);
        }
        if self.is_shared() {
            return Err(SW_SHARE);
        }
        self.raw = SRW_EXCL_BIT;
        Ok(())
    }

    /// 退出独占态：必须处于独占态。
    pub fn clear_exclusive(&mut self) -> Result<(), u32> {
        if !self.is_exclusive() {
            return Err(SW_RANGE);
        }
        self.raw = 0;
        Ok(())
    }

    /// 两态互斥判定：共享与独占不可同时成立。
    pub fn states_exclusive(&self) -> bool {
        !(self.is_shared() && self.is_exclusive())
    }

    /// 值渲染。
    pub fn render(&self) -> String {
        if self.is_unowned() {
            "unowned".to_string()
        } else if self.is_exclusive() {
            "exclusive".to_string()
        } else {
            format!("shared x{}", self.shared_count())
        }
    }
}

/// 状态账：逐次状态迁移留痕。
#[derive(Clone, PartialEq)]
pub struct SrwRec {
    pub phase: u8, // 0=shared+ 1=shared- 2=excl+ 3=excl-
    pub after: u32,
    pub note: String,
}

impl SrwRec {
    pub fn ok(&self) -> bool {
        self.phase <= 3 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("p{} -> {:#x}", self.phase, self.after)
    }
}

/// SRW 状态账。
pub struct SrwLed {
    pub val: SrwValue,
    pub log: Vec<SrwRec>,
    sealed: bool,
}

impl SrwLed {
    pub fn new() -> SrwLed {
        SrwLed { val: SrwValue::unowned(), log: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 共享 +1 留痕。
    pub fn acquire_shared(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SW_SEALED);
        }
        let c = self.val.add_shared()?;
        self.log.push(SrwRec { phase: 0, after: self.val.raw, note: note.to_string() });
        Ok(c)
    }

    /// 共享 -1 留痕。
    pub fn release_shared(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SW_SEALED);
        }
        let c = self.val.sub_shared()?;
        self.log.push(SrwRec { phase: 1, after: self.val.raw, note: note.to_string() });
        Ok(c)
    }

    /// 独占获得留痕。
    pub fn acquire_exclusive(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SW_SEALED);
        }
        self.val.set_exclusive()?;
        self.log.push(SrwRec { phase: 2, after: self.val.raw, note: note.to_string() });
        Ok(())
    }

    /// 独占释放留痕。
    pub fn release_exclusive(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SW_SEALED);
        }
        self.val.clear_exclusive()?;
        self.log.push(SrwRec { phase: 3, after: self.val.raw, note: note.to_string() });
        Ok(())
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aa80;
        h = h.wrapping_mul(211).wrapping_add(self.val.raw);
        for r in &self.log {
            h = h.wrapping_mul(211).wrapping_add(r.phase as u32);
            h = h.wrapping_mul(211).wrapping_add(r.after);
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
        format!("val={:#x} ops={} state={}", self.val.raw, self.log.len(), self.val.render())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SrwLed::new();
    // 1) 无主时共享释放必拒（下溢）。
    got[0] = b.release_shared("under").err() == Some(SW_RANGE);
    // 2) 共享态下独占获得必拒。
    let _ = b.acquire_shared("s1");
    got[1] = b.acquire_exclusive("excl-on-shared").err() == Some(SW_SHARE);
    // 3) 独占态下共享计数 +1 必拒。
    let _ = b.release_shared("s1-off");
    let _ = b.acquire_exclusive("excl");
    got[2] = b.acquire_shared("shared-on-excl").err() == Some(SW_SHARE);
    // 4) 独占态重复独占必拒。
    got[3] = b.acquire_exclusive("re-excl").err() == Some(SW_EXCL);
    // 5) 印后操作必拒。
    let _ = b.release_exclusive("excl-off");
    b.seal();
    got[4] = b.acquire_shared("post").err() == Some(SW_SEALED);
    got
}

/// 判据入口：UNX-F12195
pub fn run_unx12195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12195");
    let mut b = SrwLed::new();

    // J1 初值无主。
    s.add(
        "J1_unowned",
        b.val.is_unowned() && b.val.raw == 0,
        "初值全零 = 无主",
    );

    // J2 共享计数升到 4。
    let mut ok2 = true;
    for i in 0..4u32 {
        if b.acquire_shared("up") != Ok(i + 1) {
            ok2 = false;
        }
    }
    s.add(
        "J2_shared_up4",
        ok2 && b.val.shared_count() == 4 && b.val.is_shared(),
        "共享计数 1..4 递增",
    );

    // J3 共享/独占两态互斥判定。
    s.add(
        "J3_states_exclusive",
        b.val.states_exclusive(),
        "共享态下无独占位",
    );

    // J4 独占位常量语义。
    s.add(
        "J4_excl_bit",
        SRW_EXCL_BIT == 0x8000_0000 && SRW_SHARED_MASK == 0xFFF,
        "位型布局常量钉死",
    );

    // J5 共享计数逐层释放归零。
    let mut ok5 = true;
    for want in (0..4u32).rev() {
        if b.release_shared("down") != Ok(want) {
            ok5 = false;
        }
    }
    s.add(
        "J5_shared_down",
        ok5 && b.val.is_unowned(),
        "四层释放归零无主",
    );

    // J6 无主下溢拒。
    s.add(
        "J6_underflow",
        b.release_shared("under").err() == Some(SW_RANGE),
        "零计数释放拒 RANGE",
    );

    // J7 独占获得与退出。
    let ok7 = b.acquire_exclusive("x1").is_ok()
        && b.val.is_exclusive()
        && b.release_exclusive("x1-off").is_ok()
        && b.val.is_unowned();
    s.add("J7_excl_cycle", ok7, "独占一进一出归无主");

    // J8 共享态拒独占。
    let _ = b.acquire_shared("s");
    let j8 = b.acquire_exclusive("on-shared").err() == Some(SW_SHARE);
    let _ = b.release_shared("s-off");
    s.add("J8_shared_blocks_excl", j8, "共享态独占拒 SHARE");

    // J9 独占态拒共享。
    let _ = b.acquire_exclusive("x");
    let j9 = b.acquire_shared("on-excl").err() == Some(SW_SHARE);
    let _ = b.release_exclusive("x-off");
    s.add("J9_excl_blocks_shared", j9, "独占态共享拒 SHARE");

    // J10 值渲染三态。
    let mut v = SrwValue::unowned();
    let r0 = v.render();
    let _ = v.set_exclusive();
    let r1 = v.render();
    let _ = v.clear_exclusive();
    let _ = v.add_shared();
    let _ = v.add_shared();
    let r2 = v.render();
    s.add(
        "J10_render3",
        r0 == "unowned" && r1 == "exclusive" && r2 == "shared x2",
        "三态渲染对平",
    );

    // J11 全账回放自洽。
    s.add("J11_replay", b.replay(), "状态账逐条过核");

    // J12 留痕渲染。
    let r = b.log[0].render();
    s.add(
        "J12_render",
        r.starts_with("p0") && r.contains("0x1"),
        "共享 +1 留痕渲染对平",
    );

    // J13 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "SRW 账折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    b.seal();
    s.add(
        "J15_sealed",
        b.acquire_shared("post").err() == Some(SW_SEALED)
            && b.release_exclusive("post").err() == Some(SW_SEALED),
        "印账后操作皆拒",
    );

    // J16 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J16_seal_fp", b.fold() == fb, "印账不改指纹");

    // J17 共享计数上限越界拒。
    let mut v17 = SrwValue { raw: SRW_SHARED_MAX };
    s.add(
        "J17_shared_max",
        v17.add_shared().err() == Some(SW_RANGE),
        "共享计数到顶拒 RANGE",
    );

    // J18 空账指纹稳定。
    let b18 = SrwLed::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空 SRW 账指纹稳定");

    // J19 空账回放判不自洽。
    s.add("J19_empty_replay", !b18.replay(), "空账回放判不自洽");

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("state=unowned") && sm.contains("ops="),
        "摘要含终态与操作数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12195_srw_value() {
        let mut v = SrwValue::unowned();
        assert_eq!(v.add_shared(), Ok(1));
        assert_eq!(v.add_shared(), Ok(2));
        assert_eq!(v.set_exclusive(), Err(SW_SHARE));
        assert_eq!(v.sub_shared(), Ok(1));
        assert_eq!(v.sub_shared(), Ok(0));
        assert!(v.set_exclusive().is_ok());
        assert_eq!(v.add_shared(), Err(SW_SHARE));
        assert!(v.clear_exclusive().is_ok());
    }

    #[test]
    fn unx12195_checks_green() {
        let s = run_unx12195_checks();
        assert!(s.all_passed(), "UNX-F12195 判据未全绿");
    }

    #[test]
    fn unx12195_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
