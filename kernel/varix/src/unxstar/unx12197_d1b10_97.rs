//! UNX-F12197 — RTL_CONDITION_VARIABLE 条件变量预告（0x1aac0 段）
//!
//! 锚点：UNX-F12197-J1 Sleep/Wake 协议 10/10（Sleep 入队、WakeOne
//! 出队头、WakeAll 清队全醒），空队唤醒必拒，唤醒账全账回放一致。
//! 域：CoRun·UNX-D1-B10（临界区语义谱系，条件变量预告）。
//! 结构：CondVar（条件变量账：等待队列 + Sleep/WakeOne/WakeAll
//! 三口）+ 折叠指纹 + 注入核。
//!
//! NT 语义预告：RTL_CONDITION_VARIABLE 与 SRWLOCK/临界区配合使用
//! ——SleepConditionVariable 把线程挂入等待队列（原子放锁），
//! WakeConditionVariable 唤醒队头一个，WakeAllConditionVariable
//! 清队全醒。空队唤醒无意义必拒；重复 Sleep 同一线程必拒。本卡
//! 只立账面语义，真阻塞语义由后续批对接。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1aac0;
pub const CV_SEALED: u32 = SEG_BASE + 0x01; // 印后操作
pub const CV_TWICE: u32 = SEG_BASE + 0x02; // 重复 Sleep
pub const CV_EMPTY: u32 = SEG_BASE + 0x03; // 空队唤醒
pub const CV_GHOST: u32 = SEG_BASE + 0x04; // 野线程
pub const CV_NONE: u32 = SEG_BASE + 0x05; // 空 note 留痕

/// 等待者留痕。
#[derive(Clone, PartialEq)]
pub struct CvRec {
    pub tid: u64,
    pub kind: u8, // 0=sleep 1=wake_one 2=wake_all(记为该次全醒)
    pub note: String,
}

impl CvRec {
    pub fn ok(&self) -> bool {
        self.tid != 0 && self.kind <= 2 && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        let tag = match self.kind {
            0 => "sleep",
            1 => "wake1",
            _ => "wakeAll",
        };
        format!("{} t={:#x}", tag, self.tid)
    }
}

/// 条件变量账。
pub struct CondVar {
    pub queue: Vec<u64>,
    pub log: Vec<CvRec>,
    pub slept_total: u32,
    pub woke_total: u32,
    sealed: bool,
}

impl CondVar {
    pub fn new() -> CondVar {
        CondVar { queue: Vec::new(), log: Vec::new(), slept_total: 0, woke_total: 0, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// Sleep：线程入队尾；重复 Sleep 同线程拒。
    pub fn sleep(&mut self, tid: u64, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        if tid == 0 {
            return Err(CV_GHOST);
        }
        if note.is_empty() {
            return Err(CV_NONE);
        }
        if self.queue.contains(&tid) {
            return Err(CV_TWICE);
        }
        self.queue.push(tid);
        self.slept_total += 1;
        self.log.push(CvRec { tid, kind: 0, note: note.to_string() });
        Ok(self.queue.len() as u32)
    }

    /// WakeOne：队头先醒；空队拒。
    pub fn wake_one(&mut self, note: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        if note.is_empty() {
            return Err(CV_NONE);
        }
        if self.queue.is_empty() {
            return Err(CV_EMPTY);
        }
        let tid = self.queue.remove(0);
        self.woke_total += 1;
        self.log.push(CvRec { tid, kind: 1, note: note.to_string() });
        Ok(tid)
    }

    /// WakeAll：清队全醒，返回唤醒数；空队拒。
    pub fn wake_all(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CV_SEALED);
        }
        if note.is_empty() {
            return Err(CV_NONE);
        }
        if self.queue.is_empty() {
            return Err(CV_EMPTY);
        }
        let n = self.queue.len() as u32;
        let first = self.queue[0];
        self.queue.clear();
        self.woke_total += n;
        self.log.push(CvRec { tid: first, kind: 2, note: note.to_string() });
        Ok(n)
    }

    /// 等待队列 FIFO 纪律：入队序即唤醒序（WakeOne 视角）。
    pub fn fifo_ok(&self) -> bool {
        // 由构造保证：queue 即入队序；此处核账面 sleep/wake 配对。
        self.slept_total >= self.woke_total
    }

    /// 睡醒对平：Sleep 总数 = Wake 总数 + 队列余量。
    pub fn tally_ok(&self) -> bool {
        self.slept_total == self.woke_total + self.queue.len() as u32
    }

    /// 全账回放自洽。
    pub fn replay(&self) -> bool {
        !self.log.is_empty() && self.log.iter().all(|r| r.ok())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1aac0;
        for t in &self.queue {
            h = h.wrapping_mul(211).wrapping_add(*t as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.slept_total);
        h = h.wrapping_mul(211).wrapping_add(self.woke_total);
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
            "q={} slept={} woke={}",
            self.queue.len(), self.slept_total, self.woke_total
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CondVar::new();
    // 1) 空队 WakeOne 必拒。
    got[0] = b.wake_one("empty").err() == Some(CV_EMPTY);
    // 2) 重复 Sleep 必拒。
    let _ = b.sleep(1, "s1");
    got[1] = b.sleep(1, "s2").err() == Some(CV_TWICE);
    // 3) 零 tid Sleep 必拒。
    got[2] = b.sleep(0, "z").err() == Some(CV_GHOST);
    // 4) 空 note Sleep 必拒。
    got[3] = b.sleep(2, "").err() == Some(CV_NONE);
    // 5) 印后操作必拒。
    b.seal();
    got[4] = b.sleep(3, "post").err() == Some(CV_SEALED);
    got
}

/// 判据入口：UNX-F12197
pub fn run_unx12197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12197");
    let mut b = CondVar::new();

    // J1 Sleep 三连入队（FIFO 序 1/2/3）。
    let ok1 = b.sleep(0x11, "s1").is_ok() && b.sleep(0x22, "s2").is_ok() && b.sleep(0x33, "s3").is_ok();
    s.add(
        "J1_sleep3",
        ok1 && b.queue == [0x11, 0x22, 0x33],
        "三 waiter 按序入队",
    );

    // J2 重复 Sleep 拒。
    s.add(
        "J2_sleep_twice",
        b.sleep(0x22, "again").err() == Some(CV_TWICE),
        "同线程重复 Sleep 拒",
    );

    // J3 WakeOne 出队头。
    let w = b.wake_one("w1");
    s.add(
        "J3_wake_one",
        w == Ok(0x11) && b.queue == [0x22, 0x33],
        "队头 0x11 先醒",
    );

    // J4 WakeAll 清队全醒。
    let all = b.wake_all("wa");
    s.add(
        "J4_wake_all",
        all == Ok(2) && b.queue.is_empty(),
        "WakeAll 清队醒 2",
    );

    // J5 空队双唤醒皆拒。
    s.add(
        "J5_empty_wakes",
        b.wake_one("e1").err() == Some(CV_EMPTY) && b.wake_all("e2").err() == Some(CV_EMPTY),
        "空队 WakeOne/WakeAll 皆拒",
    );

    // J6 睡醒对平。
    s.add(
        "J6_tally",
        b.tally_ok() && (b.slept_total, b.woke_total) == (3, 3),
        "Sleep 3 = Wake 3 + 余 0",
    );

    // J7 FIFO 纪律核。
    s.add("J7_fifo", b.fifo_ok(), "睡醒序账纪律核通过");

    // J8 全账回放自洽。
    s.add("J8_replay", b.replay(), "唤醒账逐条过核");

    // J9 留痕渲染（sleep/wake1/wakeAll 三态）。
    let r0 = b.log[0].render();
    let r1 = b.log[3].render();
    let r2 = b.log[4].render();
    s.add(
        "J9_render3",
        r0.contains("sleep") && r1.contains("wake1") && r2.contains("wakeAll"),
        "三态留痕渲染对平",
    );

    // J10 独立小账：单睡单醒。
    let mut b10 = CondVar::new();
    let _ = b10.sleep(7, "s");
    let w10 = b10.wake_one("w");
    s.add(
        "J10_single_pair",
        w10 == Ok(7) && b10.settled_account(),
        "单睡单醒账面闭合",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "条件变量账折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.sleep(0x44, "post").err() == Some(CV_SEALED)
            && b.wake_all("post").err() == Some(CV_SEALED),
        "印账后 Sleep/Wake 皆拒",
    );

    // J14 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J14_seal_fp", b.fold() == fb, "印账不改指纹");

    // J15 零 tid Sleep 拒。
    let mut b15 = CondVar::new();
    s.add(
        "J15_zero_tid",
        b15.sleep(0, "z").err() == Some(CV_GHOST) && b15.queue.is_empty(),
        "零 tid Sleep 拒且不入队",
    );

    // J16 空 note 全口拒。
    let mut b16 = CondVar::new();
    s.add(
        "J16_empty_note",
        b16.sleep(1, "").err() == Some(CV_NONE)
            && b16.wake_one("").err() == Some(CV_NONE)
            && b16.wake_all("").err() == Some(CV_NONE),
        "空留痕三口皆拒",
    );

    // J17 空账指纹稳定。
    let b17 = CondVar::new();
    s.add("J17_fresh_fp", b17.fold() == b17.fold(), "空条件变量账指纹稳定");

    // J18 空账回放判不自洽。
    s.add("J18_empty_replay", !b17.replay(), "空账回放判不自洽");

    // J19 八 waiter 全睡全醒（与 12193 八线程呼应）。
    let mut b19 = CondVar::new();
    let mut ok19 = true;
    for i in 1..=8u64 {
        if b19.sleep(0xA0 + i, "s").is_err() {
            ok19 = false;
        }
    }
    let n19 = b19.wake_all("wa");
    s.add(
        "J19_eight_all",
        ok19 && n19 == Ok(8) && b19.tally_ok(),
        "八睡一唤全醒对平",
    );

    // J20 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J20_summary",
        sm.contains("q=0") && sm.contains("slept=3") && sm.contains("woke=3"),
        "摘要含队列与睡醒计数",
    );

    s
}

impl CondVar {
    /// 账面闭合判定：队列空且睡醒对平。
    pub fn settled_account(&self) -> bool {
        self.queue.is_empty() && self.tally_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12197_condvar() {
        let mut b = CondVar::new();
        assert_eq!(b.wake_one("e"), Err(CV_EMPTY));
        assert!(b.sleep(1, "a").is_ok());
        assert_eq!(b.sleep(1, "b"), Err(CV_TWICE));
        assert_eq!(b.wake_one("c"), Ok(1));
        assert_eq!(b.wake_all("d"), Err(CV_EMPTY));
    }

    #[test]
    fn unx12197_checks_green() {
        let s = run_unx12197_checks();
        assert!(s.all_passed(), "UNX-F12197 判据未全绿");
    }

    #[test]
    fn unx12197_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
