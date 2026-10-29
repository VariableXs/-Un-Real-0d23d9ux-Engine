//! UNX-F12073 TEB 越界写检测与只读快照（0x19b40 段）
//!
//! 卡面：TEB 页只读快照协议——快照页对内核只读，仅白名单字段
//! （LastErrorValue 0x34 等内核写者字段）可写；越界/越权写一律显性拒。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 界内判定：写偏移 + 写长必须整体落在快照页 [0, LEN) 内。
//! - J2 跨界溢出：offset+size 回绕（溢出 u32）必拒。
//! - J3 白名单：仅内核写者字段偏移可写，其余一律只读拒。
//! - J4 零长写拒：写长为零必拒。
//! - J5 写账：合法写逐笔登记可回读。
//! - J6 印账冻结：seal 后一切写必拒。
//! - J7 注入五况全抓。
//!
//! 结构：TebSnapshot 页描述 + 白名单闭集 + 写账本 + 折叠指纹 +
//! 注入核（页尾越界/回绕跨界/白名单外/零长/印后写）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19b40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19b40;
pub const TW_OOB: u32 = SEG_BASE + 0x01; // 页尾越界
pub const TW_SPAN: u32 = SEG_BASE + 0x02; // 回绕跨界
pub const TW_READONLY: u32 = SEG_BASE + 0x03; // 白名单外
pub const TW_ZERO_LEN: u32 = SEG_BASE + 0x04; // 零长写
pub const TW_FROZEN: u32 = SEG_BASE + 0x05; // 印账后写

/// 快照页长（x64 TEB 常规 4K）。
pub const SNAPSHOT_LEN: u32 = 0x1000;

/// 内核写者白名单：TEB 页内允许写偏移闭集。
pub const WRITABLE: [u32; 5] = [0x34, 0x58, 0x1254 % SNAPSHOT_LEN, 0x2c, 0x1240 % SNAPSHOT_LEN];

/// 一次写操作的留痕。
#[derive(Clone)]
pub struct WriteRec {
    pub off: u32,
    pub size: u32,
    pub val: u32,
    pub note: String,
}

impl WriteRec {
    pub fn render(&self) -> String {
        format!("W@{:x}|{}|{}", self.off, self.size, self.val)
    }
}

/// TEB 只读快照页：白名单写核 + 写账。
pub struct TebSnapshot {
    pub len: u32,
    pub writes: Vec<WriteRec>,
    pub max_writes: usize,
    pub sealed: bool,
}

impl TebSnapshot {
    pub fn new(len: u32, max_writes: usize) -> TebSnapshot {
        TebSnapshot { len, writes: Vec::new(), max_writes, sealed: false }
    }

    fn whitelisted(&self, off: u32) -> bool {
        WRITABLE.contains(&off)
    }

    /// 写核：界内 → 无回绕 → 白名单 → 非零长 → 账容，逐门推进。
    pub fn write(&mut self, off: u32, size: u32, val: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TW_FROZEN);
        }
        if size == 0 {
            return Err(TW_ZERO_LEN);
        }
        // 回绕判定：off+size 溢出 u32 必跨界。必须先于 OOB 门——回绕样本
        // 的 off 本身远超页长，若先走 OOB 门则 TW_SPAN 成死分支。
        if off.checked_add(size).is_none() {
            return Err(TW_SPAN);
        }
        if off >= self.len {
            return Err(TW_OOB);
        }
        // 界内起点但跨度出页必拒。
        if off > self.len || size > self.len - off {
            return Err(TW_SPAN);
        }
        if off + size > self.len {
            return Err(TW_SPAN);
        }
        if !self.whitelisted(off) {
            return Err(TW_READONLY);
        }
        if self.writes.len() >= self.max_writes {
            return Err(TW_READONLY); // 账容上限按只读面显性拒
        }
        self.writes.push(WriteRec { off, size, val, note: note.to_string() });
        Ok(())
    }

    /// 某偏移累计写次数。
    pub fn write_count_at(&self, off: u32) -> usize {
        self.writes.iter().filter(|w| w.off == off).count()
    }

    /// 最近一次写值回读。
    pub fn last_val_at(&self, off: u32) -> Option<u32> {
        self.writes.iter().filter(|w| w.off == off).last().map(|w| w.val)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.len);
        for w in &self.writes {
            h = h.wrapping_mul(211).wrapping_add(w.off);
            h = h.wrapping_mul(211).wrapping_add(w.size);
            h = h.wrapping_mul(211).wrapping_add(w.val);
            for by in w.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：写账条数。
    pub fn write_count(&self) -> usize {
        self.writes.len()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("len={:x} writes={}", self.len, self.writes.len())
    }

    /// 诊断面：留痕渲染行。
    pub fn rec_line(&self, i: usize) -> String {
        match self.writes.get(i) {
            Some(w) => w.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut s = TebSnapshot::new(SNAPSHOT_LEN, 16);
    // 1) 页尾越界：偏移超出页长必拒。
    got[0] = s.write(0x1000, 4, 1, "oob").err() == Some(TW_OOB);
    // 2) 回绕跨界：offset+size 溢出必拒。
    got[1] = s.write(0xFFFF_FFF0, 0x20, 1, "wrap").err() == Some(TW_SPAN);
    // 3) 白名单外必拒。
    got[2] = s.write(0x800, 4, 1, "ro").err() == Some(TW_READONLY);
    // 4) 零长写必拒。
    got[3] = s.write(0x34, 0, 1, "zero").err() == Some(TW_ZERO_LEN);
    // 5) 印账后写必抓。
    s.seal();
    got[4] = s.write(0x34, 4, 7, "post").err() == Some(TW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12073");

    // J1 白名单内合法写并记账。
    let j1 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(0x34, 4, 5, "lasterr").is_ok()
            && t.write_count_at(0x34) == 1
            && t.last_val_at(0x34) == Some(5)
    };
    s.add("J1-白名单写", j1, "LastErrorValue 可写可回读");

    // J2 页尾越界拒。
    let j2 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(SNAPSHOT_LEN - 2, 4, 1, "tail").err() == Some(TW_SPAN)
            && t.write(SNAPSHOT_LEN + 4, 4, 1, "far").err() == Some(TW_OOB)
    };
    s.add("J2-越界两门", j2, "页尾跨度与远端越界分码");

    // J3 回绕跨界拒。
    let j3 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(0xFFFF_FFFF, 2, 1, "wrap").err() == Some(TW_SPAN)
    };
    s.add("J3-回绕拒", j3, "溢出必拒");

    // J4 白名单外只读拒。
    let j4 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(0x800, 4, 1, "ro").err() == Some(TW_READONLY)
    };
    s.add("J4-只读拒", j4, "非白名单偏移必拒");

    // J5 零长写拒。
    let j5 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(0x34, 0, 1, "zero").err() == Some(TW_ZERO_LEN)
    };
    s.add("J5-零长拒", j5, "零长必拒");

    // J6 跨界白名单拒：起于白名单但跨出页面的写亦拒。
    let j6 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.write(WRITABLE[0], SNAPSHOT_LEN, 1, "span")
            .err() == Some(TW_SPAN)
    };
    s.add("J6-跨界白名单", j6, "白名单起点跨界仍拒");

    // J7 写账逐笔可渲染。
    let j7 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        let _ = t.write(0x34, 4, 9, "a");
        let _ = t.write(0x2c, 4, 8, "b");
        t.write_count() == 2 && t.rec_line(1).contains("2c")
    };
    s.add("J7-写账渲染", j7, "逐笔可回读");

    // J8 印账冻结。
    let j8 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        t.seal();
        t.write(0x34, 4, 1, "post").err() == Some(TW_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
        let _ = t.write(0x34, 4, 1, "a");
        t.fold() == t.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut t = TebSnapshot::new(SNAPSHOT_LEN, 16);
    let _ = t.write(0x34, 4, 3, "le1");
    let _ = t.write(0x34, 4, 4, "le2");

    s.add("累计写次", t.write_count_at(0x34) == 2, "同偏移两笔");
    s.add("末值回读", t.last_val_at(0x34) == Some(4), "末值 4");
    s.add("摘要", t.summary().contains("writes=2"), "摘要含写数");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12073_write_guards() {
        let mut t = TebSnapshot::new(SNAPSHOT_LEN, 8);
        assert!(t.write(0x34, 4, 1, "ok").is_ok());
        assert_eq!(t.write(0x800, 4, 1, "ro"), Err(TW_READONLY));
        assert_eq!(t.write(0x1000, 4, 1, "oob"), Err(TW_OOB));
    }

    #[test]
    fn unx12073_checks_green() {
        let s = run_unx12073_checks();
        assert!(s.all_passed(), "UNX-F12073 判据未全绿");
    }

    #[test]
    fn whitelist_closed_set() {
        assert!(WRITABLE.contains(&0x34));
        assert!(WRITABLE.iter().all(|o| *o < SNAPSHOT_LEN));
    }
}
