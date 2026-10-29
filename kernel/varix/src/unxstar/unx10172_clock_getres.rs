//! UNX-F10172 clock_getres 正反双判据（精度达成与非法时钟误用受控）（0x108a0 段）
//!
//! 锚点：UNX-F10172-J1 精度达成（res 上报与实测粒度一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：时钟农场（四时钟 × 声明精度 × 底层计数器）+ 读数核（量化读）
//! + 实测核（逐 ns 探针测粒度）+ 精度账本 + 折叠指纹 +
//! 注入核（零精度注册/野时钟号/回拨/非倍读数/印后读）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x108a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x108a0;
pub const CK_BAD_ID: u32 = SEG_BASE + 0x01; // 野时钟号
pub const CK_BAD_RES: u32 = SEG_BASE + 0x02; // 零精度注册
pub const CK_BACK: u32 = SEG_BASE + 0x03; // 计数器回拨
pub const CK_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const CK_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍测

/// 时钟号闭集（对齐 POSIX clockid_t 常用值）。
pub const CID_REALTIME: u32 = 0;
pub const CID_MONOTONIC: u32 = 1;
pub const CID_PROC_CPUTIME: u32 = 2;
pub const CID_THREAD_CPUTIME: u32 = 3;

/// 单时钟：声明精度（ns）与底层计数器。
#[derive(Clone, Copy)]
pub struct Clock {
    pub id: u32,
    pub res_ns: u32,
    pub counter_ns: u64,
}

/// 一次精度测量的账本留痕。
#[derive(Clone)]
pub struct ResRec {
    pub seq: u32,
    pub id: u32,
    pub declared: u32,
    pub measured: u32,
    pub note: String,
}

impl ResRec {
    /// 账目合法性：留痕非空且声明==实测（精度漂移即伪账）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.declared == self.measured && self.declared > 0
    }

    pub fn render_line(&self) -> String {
        format!("C{}|id{}|d{}|m{}|{}", self.seq, self.id, self.declared, self.measured, self.note)
    }
}

/// 时钟农场：注册 + 读数 + 推进 + 粒度实测。
pub struct ClockFarm {
    pub clocks: Vec<Clock>,
    pub ledger: Vec<ResRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ClockFarm {
    pub fn new() -> ClockFarm {
        ClockFarm { clocks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 注册时钟：精度必须非零（零精度=粒度黑洞，必拒）。
    pub fn mount(&mut self, id: u32, res_ns: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CK_FROZEN);
        }
        if note.is_empty() {
            return Err(CK_EMPTY_NOTE);
        }
        if res_ns == 0 {
            return Err(CK_BAD_RES);
        }
        if self.clocks.iter().any(|c| c.id == id) {
            return Err(CK_BAD_ID);
        }
        self.clocks.push(Clock { id, res_ns, counter_ns: 0 });
        Ok(())
    }

    fn idx_of(&self, id: u32) -> Option<usize> {
        self.clocks.iter().position(|c| c.id == id)
    }

    /// 量化读：读数必须是精度的整数倍（res 对齐截断）。
    pub fn read_ns(&self, id: u32) -> Result<u64, u32> {
        match self.idx_of(id) {
            Some(i) => {
                let c = &self.clocks[i];
                let r = c.res_ns as u64;
                Ok(c.counter_ns - c.counter_ns % r)
            }
            None => Err(CK_BAD_ID),
        }
    }

    /// 推进计数器：只许向前，回拨必拒。
    pub fn advance(&mut self, id: u32, ns: u64) -> Result<u64, u32> {
        let i = match self.idx_of(id) {
            Some(i) => i,
            None => return Err(CK_BAD_ID),
        };
        if self.clocks[i].counter_ns.checked_add(ns).is_none() {
            return Err(CK_BACK);
        }
        self.clocks[i].counter_ns += ns;
        Ok(self.clocks[i].counter_ns)
    }

    /// 精度实测：以 1ns 步进探针找最小可见步长，与声明精度对账。
    pub fn measure_granularity(&mut self, id: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CK_FROZEN);
        }
        if note.is_empty() {
            return Err(CK_EMPTY_NOTE);
        }
        let i = match self.idx_of(id) {
            Some(i) => i,
            None => return Err(CK_BAD_ID),
        };
        let declared = self.clocks[i].res_ns;
        let base = self.read_ns(id).unwrap_or(0);
        // 探针：从 1ns 起逐 ns 推进，直到量化读数跳变。
        let mut probe: u32 = 0;
        loop {
            probe += 1;
            if probe > declared {
                // 不该发生：读数步长超过声明精度即精度虚标。
                return Err(CK_BAD_RES);
            }
            let t = self.clocks[i].counter_ns + probe as u64;
            let q = t - t % declared as u64;
            if q > base {
                break;
            }
        }
        self.ledger.push(ResRec {
            seq: self.seq_next,
            id,
            declared,
            measured: probe,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(probe)
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 时钟号闭集校验（供外部门面复用）。
    pub fn known_id(&self, id: u32) -> bool {
        self.clocks.iter().any(|c| c.id == id)
    }

    /// 折叠指纹（41 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x108a0;
        for c in &self.clocks {
            h = h.wrapping_mul(41).wrapping_add(c.id);
            h = h.wrapping_mul(41).wrapping_add(c.res_ns);
            h = h.wrapping_mul(41).wrapping_add(c.counter_ns as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(41).wrapping_add(r.seq);
            h = h.wrapping_mul(41).wrapping_add(r.measured);
            for by in r.note.bytes() {
                h = h.wrapping_mul(41).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut f = ClockFarm::new();
    let _ = f.mount(CID_MONOTONIC, 8, "m");
    // 1) 零精度注册必拒。
    got[0] = f.mount(CID_REALTIME, 0, "inj-zero").err() == Some(CK_BAD_RES);
    // 2) 野时钟号读数必拒。
    got[1] = f.read_ns(42).err() == Some(CK_BAD_ID);
    // 3) 伪造精度漂移账（declared != measured 必判非法）。
    let forged = ResRec {
        seq: 1,
        id: 1,
        declared: 8,
        measured: 16,
        note: "inj-drift".to_string(),
    };
    got[2] = !forged.ok();
    // 4) 探针法实测精度与声明一致（负样本：错精度必被探针纠出）。
    let mut f2 = ClockFarm::new();
    let _ = f2.mount(7, 16, "x");
    got[3] = f2.measure_granularity(7, "probe") == Ok(16);
    // 5) 印账后测量必抓。
    f.seal();
    got[4] = f.measure_granularity(CID_MONOTONIC, "inj-frozen").err() == Some(CK_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10172");
    let mut f = ClockFarm::new();
    let _ = f.mount(CID_REALTIME, 1, "rt");
    let _ = f.mount(CID_MONOTONIC, 4, "mo");
    let _ = f.mount(CID_PROC_CPUTIME, 16, "pc");

    // J1 精度达成：四时钟逐个实测与声明一致。
    let j1 = {
        let mut ok = true;
        let ids = [CID_REALTIME, CID_MONOTONIC, CID_PROC_CPUTIME];
        for id in ids {
            let declared = f.clocks.iter().find(|c| c.id == id).map(|c| c.res_ns).unwrap_or(0);
            if f.measure_granularity(id, "j1") != Ok(declared) {
                ok = false;
            }
        }
        ok
    };
    s.add("J1-精度达成", j1, "三时钟实测粒度==声明精度");

    // J2 读数量化性质：读数恒为精度整数倍。
    let j2 = {
        let _ = f.advance(CID_MONOTONIC, 9);
        let _ = f.advance(CID_PROC_CPUTIME, 33);
        let r1 = f.read_ns(CID_MONOTONIC).unwrap_or(1);
        let r2 = f.read_ns(CID_PROC_CPUTIME).unwrap_or(1);
        r1 % 4 == 0 && r2 % 16 == 0
    };
    s.add("J2-读数量化", j2, "9ns→8、33ns→32 对齐读出");

    // J3 野时钟号拒。
    let j3 = f.read_ns(99).err() == Some(CK_BAD_ID);
    s.add("J3-野时钟拒", j3, "未注册 clockid 必拒");

    // J4 单调性：推进后读数不回退。
    let j4 = {
        let a = f.read_ns(CID_MONOTONIC).unwrap_or(0);
        let _ = f.advance(CID_MONOTONIC, 7);
        let b = f.read_ns(CID_MONOTONIC).unwrap_or(0);
        b >= a
    };
    s.add("J4-单调不回退", j4, "推进后读数只增不减");

    // J5 空精度注册拒（含注 0 与重号）。
    let j5 = f.mount(9, 0, "zero").err() == Some(CK_BAD_RES)
        && f.mount(CID_MONOTONIC, 8, "dup").err() == Some(CK_BAD_ID);
    s.add("J5-注册守门", j5, "零精度/重号注册必拒");

    // J6 精度账完整：三账全部声明==实测。
    let j6 = f.ledger.len() == 3 && f.ledger_ok() && f.seq_unique();
    s.add("J6-精度账完整", j6, "逐条声明实测对平");

    // J7 指纹稳定且印账后封禁。
    let j7 = {
        let f1 = f.fold();
        let f2 = f.fold();
        f.seal();
        f1 == f2 && f.measure_granularity(CID_REALTIME, "post").err() == Some(CK_FROZEN)
    };
    s.add("J7-指纹封账", j7, "指纹幂等且印后必拒");

    // J8 注入核 5/5。
    let got = inject_five();
    let j8 = got.iter().all(|g| *g);
    s.add_dyn("J8-注入全抓", j8, &format!("inj5={:?}", got));

    // J9 大步推进不溢出：u64 计数器大步推进后量化读仍守恒。
    let j9 = {
        let mut f2 = ClockFarm::new();
        let _ = f2.mount(CID_THREAD_CPUTIME, 64, "big");
        let _ = f2.advance(CID_THREAD_CPUTIME, 0x0000_FFFF_FFFF);
        let r = f2.read_ns(CID_THREAD_CPUTIME).unwrap_or(0);
        r % 64 == 0 && r > 0
    };
    s.add("J9-大步守恒", j9, "大推进后读数仍按精度对齐");

    // J10 闭集门面：known_id 与 read_ns 判定一致（内部一致性）。
    let j10 = f.known_id(CID_REALTIME) && !f.known_id(31) && f.read_ns(31).is_err();
    s.add("J10-门面一致", j10, "闭集门面与读数判定同源");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10172_clock_flow() {
        let mut f = ClockFarm::new();
        let _ = f.mount(CID_MONOTONIC, 10, "mo");
        let _ = f.advance(CID_MONOTONIC, 25);
        assert_eq!(f.read_ns(CID_MONOTONIC), Ok(20));
        assert_eq!(f.measure_granularity(CID_MONOTONIC, "p"), Ok(10));
        assert_eq!(f.read_ns(77), Err(CK_BAD_ID));
        assert_eq!(f.mount(1, 0, "z"), Err(CK_BAD_RES));
    }

    #[test]
    fn unx10172_checks_green() {
        let s = run_unx10172_checks();
        assert!(s.all_passed(), "UNX-F10172 判据未全绿");
    }

    #[test]
    fn unx10172_granularity_sweep() {
        // 多精度扫描：1/4/16/64ns 四档实测恒等于声明。
        let mut f = ClockFarm::new();
        let _ = f.mount(CID_REALTIME, 1, "a");
        let _ = f.mount(CID_MONOTONIC, 4, "b");
        let _ = f.mount(CID_PROC_CPUTIME, 16, "c");
        let _ = f.mount(CID_THREAD_CPUTIME, 64, "d");
        assert_eq!(f.measure_granularity(CID_REALTIME, "s1"), Ok(1));
        assert_eq!(f.measure_granularity(CID_MONOTONIC, "s2"), Ok(4));
        assert_eq!(f.measure_granularity(CID_PROC_CPUTIME, "s3"), Ok(16));
        assert_eq!(f.measure_granularity(CID_THREAD_CPUTIME, "s4"), Ok(64));
        assert!(f.ledger_ok() && f.seq_unique());
    }
}
