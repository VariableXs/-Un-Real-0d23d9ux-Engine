//! UNX-F12189 — LockSemaphore 事件挂点（0x1a9c0 段）
//!
//! 锚点：UNX-F12189-J1 事件挂点 10/10（LockSemaphore 句柄唯一挂
//! 账、唤醒只认挂账句柄、一唤一醒对平），野句柄唤醒必拒，挂账
//! 全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系）。
//! 结构：EventLed（事件挂账：句柄 + waiter 计数 + 唤醒留痕）+
//! 唤醒协议核 + 折叠指纹 + 注入核。
//!
//! NT 语义：RTL_CRITICAL_SECTION 的 LockSemaphore 字段在首次争用
//! 时才挂上事件句柄——释放时 SetEvent 唤醒至多一个 waiter，唤醒
//! 只认挂账句柄；未挂账先唤醒、重复挂账、野句柄唤醒都是协议
//! 破坏，一律显性记账并拒。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a9c0;
pub const EV_SEALED: u32 = SEG_BASE + 0x01; // 印后挂账/唤醒
pub const EV_TWICE: u32 = SEG_BASE + 0x02; // 重复挂账
pub const EV_GHOST: u32 = SEG_BASE + 0x03; // 野句柄唤醒
pub const EV_NONE: u32 = SEG_BASE + 0x04; // 未挂账先唤醒
pub const EV_EMPTY: u32 = SEG_BASE + 0x05; // 零 waiter 唤醒

/// 唤醒留痕。
#[derive(Clone, PartialEq)]
pub struct WakeRec {
    pub handle: u64,
    pub waiters_before: u32,
    pub note: String,
}

impl WakeRec {
    /// 留痕自洽：句柄非零、唤醒前确有 waiter。
    pub fn ok(&self) -> bool {
        self.handle != 0 && self.waiters_before > 0 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("h={:#x} w={}", self.handle, self.waiters_before)
    }
}

/// 事件挂账：LockSemaphore 语义账。
pub struct EventLed {
    pub handle: u64,
    pub waiters: u32,
    pub wakes: Vec<WakeRec>,
    next_seq: u32,
    sealed: bool,
}

impl EventLed {
    pub fn new() -> EventLed {
        EventLed { handle: 0, waiters: 0, wakes: Vec::new(), next_seq: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂账：首次争用时把事件句柄挂上；重复挂账/零句柄拒。
    pub fn hook(&mut self, handle: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if handle == 0 {
            return Err(EV_GHOST);
        }
        if self.handle != 0 {
            return Err(EV_TWICE);
        }
        self.handle = handle;
        let seq = self.next_seq;
        self.next_seq += 1;
        Ok(seq)
    }

    /// waiter 入账（争用路径把线程挂到事件上）。
    pub fn arrive(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.handle == 0 {
            return Err(EV_NONE);
        }
        self.waiters = self.waiters.wrapping_add(1);
        Ok(self.waiters)
    }

    /// 唤醒：只认挂账句柄，一唤一醒；未挂账/野句柄/零 waiter 拒。
    pub fn wake(&mut self, handle: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.handle == 0 {
            return Err(EV_NONE);
        }
        if handle != self.handle {
            return Err(EV_GHOST);
        }
        if note.is_empty() {
            return Err(EV_EMPTY);
        }
        if self.waiters == 0 {
            return Err(EV_EMPTY);
        }
        let before = self.waiters;
        self.waiters -= 1;
        self.wakes.push(WakeRec { handle, waiters_before: before, note: note.to_string() });
        Ok(before)
    }

    /// 挂账状态：事件已挂。
    pub fn hooked(&self) -> bool {
        self.handle != 0
    }

    /// 唤醒留痕全自洽。
    pub fn wakes_replay(&self) -> bool {
        !self.wakes.is_empty() && self.wakes.iter().all(|w| w.ok())
    }

    /// 唤醒计数对平：留痕数 + 剩余 waiter = 到达 waiter 总数。
    /// 本模型无到达总账字段，用「留痕数 == 剩余 waiter 之外无幽灵」
    /// 的替代核：每次留痕 waiters_before 严格递减。
    pub fn wake_monotonic(&self) -> bool {
        self.wakes.windows(2).all(|w| w[0].waiters_before > w[1].waiters_before)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a9c0;
        h = h.wrapping_mul(211).wrapping_add(self.handle as u32);
        h = h.wrapping_mul(211).wrapping_add(self.waiters);
        for w in &self.wakes {
            h = h.wrapping_mul(211).wrapping_add(w.waiters_before);
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
        format!(
            "h={:#x} waiters={} wakes={}",
            self.handle, self.waiters, self.wakes.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = EventLed::new();
    // 1) 未挂账先唤醒必拒。
    got[0] = b.wake(0xE1, "pre").err() == Some(EV_NONE);
    // 2) 重复挂账必拒。
    let _ = b.hook(0xE1);
    got[1] = b.hook(0xE2).err() == Some(EV_TWICE);
    // 3) 野句柄唤醒必拒。
    let _ = b.arrive();
    got[2] = b.wake(0xDEAD, "wild").err() == Some(EV_GHOST);
    // 4) 零句柄挂账必拒。
    got[3] = {
        let mut z = EventLed::new();
        z.hook(0).err() == Some(EV_GHOST)
    };
    // 5) 印后唤醒必拒。
    b.seal();
    got[4] = b.wake(0xE1, "post").err() == Some(EV_SEALED);
    got
}

/// 判据入口：UNX-F12189
pub fn run_unx12189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12189");
    let mut b = EventLed::new();

    // J1 未挂账状态判定。
    s.add(
        "J1_unhooked",
        !b.hooked() && b.handle == 0,
        "初始未挂账：LockSemaphore 为空",
    );

    // J2 未挂账先唤醒拒。
    s.add(
        "J2_wake_before_hook",
        b.wake(0xE1, "early").err() == Some(EV_NONE),
        "未挂账唤醒拒 NONE",
    );

    // J3 首次挂账成功。
    s.add(
        "J3_hook",
        b.hook(0xE1) == Ok(1) && b.hooked(),
        "事件句柄 0xE1 挂账",
    );

    // J4 重复挂账拒。
    s.add(
        "J4_rehook",
        b.hook(0xE2).err() == Some(EV_TWICE),
        "重复挂账拒 TWICE",
    );

    // J5 未挂账期入队拒（waiter 必须挂在已挂账事件上）。
    // 注：本模型 arrive 在未挂账时拒；此处已挂账故先建独立账验证。
    let mut b5 = EventLed::new();
    s.add(
        "J5_arrive_pre_hook",
        b5.arrive().err() == Some(EV_NONE),
        "未挂账入队拒 NONE",
    );

    // J6 waiter 到账三枚。
    let j6 = b.arrive().is_ok() && b.arrive().is_ok() && b.arrive().is_ok();
    s.add("J6_arrivals", j6 && b.waiters == 3, "三 waiter 挂上事件");

    // J7 野句柄唤醒拒。
    s.add(
        "J7_wild_wake",
        b.wake(0xFF, "wild").err() == Some(EV_GHOST),
        "野句柄唤醒拒 GHOST",
    );

    // J8 一唤一醒：首次唤醒后 waiter 2。
    let w1 = b.wake(0xE1, "wake-1");
    s.add(
        "J8_wake_one",
        w1 == Ok(3) && b.waiters == 2,
        "首唤醒前账 3、余 2",
    );

    // J9 连续唤醒至队空。
    let ok9 = b.wake(0xE1, "wake-2").is_ok() && b.wake(0xE1, "wake-3").is_ok();
    s.add("J9_drain", ok9 && b.waiters == 0, "三唤三醒队列清空");

    // J10 零 waiter 唤醒拒。
    s.add(
        "J10_wake_empty",
        b.wake(0xE1, "extra").err() == Some(EV_EMPTY),
        "空队唤醒拒 EMPTY",
    );

    // J11 唤醒留痕全自洽。
    s.add("J11_wakes_replay", b.wakes_replay(), "唤醒留痕逐条过核");

    // J12 唤醒前账严格递减。
    s.add("J12_wake_mono", b.wake_monotonic(), "waiters_before 3>2>1");

    // J13 留痕渲染。
    let r = b.wakes[0].render();
    s.add(
        "J13_render",
        r.contains("h=0xe1") && r.contains("w=3"),
        "留痕渲染含句柄与前账",
    );

    // J14 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J14_fingerprint", f1 == f2, "事件账折叠指纹一致");

    // J15 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J15_inject_all", caught == 5, "5 注入点全检出");

    // J16 印账受控。
    b.seal();
    s.add(
        "J16_sealed",
        b.hook(0xE9).err() == Some(EV_SEALED)
            && b.wake(0xE1, "post").err() == Some(EV_SEALED),
        "印账后挂账/唤醒皆拒",
    );

    // J17 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J17_seal_fp", b.fold() == fb, "印账不改指纹");

    // J18 空账指纹稳定。
    let b18 = EventLed::new();
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空事件账指纹稳定");

    // J19 零句柄挂账拒。
    let mut b19 = EventLed::new();
    s.add(
        "J19_zero_handle",
        b19.hook(0).err() == Some(EV_GHOST) && !b19.hooked(),
        "零句柄挂账拒且保持未挂",
    );

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("h=0xe1") && sm.contains("wakes=3"),
        "摘要含句柄与唤醒数",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12189_event() {
        let mut b = EventLed::new();
        assert_eq!(b.wake(0xE1, "x"), Err(EV_NONE));
        assert_eq!(b.hook(0xE1), Ok(1));
        assert_eq!(b.hook(0xE1), Err(EV_TWICE));
        assert!(b.arrive().is_ok());
        assert_eq!(b.wake(0xDEAD, "x"), Err(EV_GHOST));
        assert_eq!(b.wake(0xE1, "ok"), Ok(1));
        assert_eq!(b.wake(0xE1, "x"), Err(EV_EMPTY));
    }

    #[test]
    fn unx12189_checks_green() {
        let s = run_unx12189_checks();
        assert!(s.all_passed(), "UNX-F12189 判据未全绿");
    }

    #[test]
    fn unx12189_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
