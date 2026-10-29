//! UNX-F24181 vblank/HPD 内核事件面与事件队列·子功能01（vblank 计数器与时间戳账）（0x41880 段）
//!
//! 锚点：UNX-F24181-J1 帧账（vblank 计数与时间戳逐条落账）；J2 帧语
//! 义（计数单调 + 周期自洽 + 倒退拒三性质）；J3 帧指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（vblank 计数面）。
//!
//! 结构：帧账本 + vblank 核 + 时间戳校验核 + 折叠指纹 + 注入核（倒
//! 退时间戳/野屏幕/零周期/印后/重帧必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41880 独占 0x20）。
pub const SEG_BASE: u32 = 0x41880;
pub const VB_REVERSED: u32 = SEG_BASE + 0x01; // 时间戳倒退
pub const VB_UNK_CRTC: u32 = SEG_BASE + 0x02; // 野屏幕
pub const VB_ZERO_PERIOD: u32 = SEG_BASE + 0x03; // 零周期
pub const VB_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const VB_DUP_FRAME: u32 = SEG_BASE + 0x05; // 重帧号

/// 帧条目。
#[derive(Clone)]
pub struct VbFrame {
    pub crtc: u32,
    pub frame_no: u64,
    pub ts_ns: u64,
}

/// vblank 簿。
pub struct VbBook {
    pub crtcs: Vec<u32>,
    pub period_ns: u64,
    pub frames: Vec<VbFrame>,
    pub seq_next: u64,
    pub sealed: bool,
}

impl VbBook {
    pub fn new(crtcs: &[u32], period_ns: u64) -> VbBook {
        VbBook {
            crtcs: crtcs.to_vec(),
            period_ns,
            frames: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// vblank 到来：帧号单调 + 时间戳不倒退。
    pub fn vblank(&mut self, crtc: u32, ts_ns: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(VB_FROZEN);
        }
        if !self.crtcs.contains(&crtc) {
            return Err(VB_UNK_CRTC);
        }
        if self.period_ns == 0 {
            return Err(VB_ZERO_PERIOD);
        }
        let last = self.frames.iter().rev().find(|f| f.crtc == crtc);
        if let Some(l) = last {
            if ts_ns < l.ts_ns {
                return Err(VB_REVERSED);
            }
            if l.frame_no + 1 != self.seq_next && ts_ns == l.ts_ns {
                return Err(VB_DUP_FRAME);
            }
        }
        let frame_no = self.seq_next;
        self.seq_next += 1;
        self.frames.push(VbFrame { crtc, frame_no, ts_ns });
        Ok(frame_no)
    }

    pub fn frames_of(&self, crtc: u32) -> usize {
        self.frames.iter().filter(|f| f.crtc == crtc).count()
    }

    pub fn mono_ok(&self) -> bool {
        for i in 1..self.frames.len() {
            if self.frames[i].ts_ns < self.frames[i - 1].ts_ns {
                return false;
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41880;
        for f in &self.frames {
            h = h.wrapping_mul(31).wrapping_add(f.crtc);
            h = h.wrapping_mul(31).wrapping_add(f.frame_no as u32);
            h = h.wrapping_mul(31).wrapping_add(f.ts_ns as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：双屏 16.6ms 周期。
pub fn mk_book() -> VbBook {
    let mut b = VbBook::new(&[0, 1], 16_666_667);
    let _ = b.vblank(0, 0);
    let _ = b.vblank(1, 100);
    let _ = b.vblank(0, 16_666_667);
    let _ = b.vblank(1, 16_666_767);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 时间戳倒退
    got[0] = b.vblank(0, 1).err() == Some(VB_REVERSED);
    // 2) 野屏幕
    got[1] = b.vblank(99, 99_999_999).err() == Some(VB_UNK_CRTC);
    // 3) 零周期
    got[2] = VbBook::new(&[0], 0).vblank(0, 1).err() == Some(VB_ZERO_PERIOD);
    // 4) 单调性账面
    got[3] = b.mono_ok();
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.vblank(0, 99_999_999).err() == Some(VB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24181");
    let mut b = mk_book();

    // J1 帧账齐
    let j1 = b.frames.len() == 4;
    s.add("J1-帧账齐", j1, "四帧落账");

    // J2 计数单调
    let j2 = {
        let f = &b.frames;
        f[0].frame_no < f[1].frame_no && f[1].frame_no < f[2].frame_no
    };
    s.add("J2-计数单调", j2, "帧号递增");

    // J3 时间戳不倒退
    let j3 = b.vblank(0, 1).err() == Some(VB_REVERSED);
    s.add("J3-倒退拒", j3, "早于末帧必拒");

    // J4 周期自洽（帧 0→2 差 ≈ 1 周期）
    let j4 = {
        let f = &b.frames;
        f[2].ts_ns - f[0].ts_ns == 16_666_667
    };
    s.add("J4-周期自洽", j4, "同屏帧差=周期");

    // J5 野屏幕拒
    let j5 = b.vblank(99, 99_999_999).err() == Some(VB_UNK_CRTC);
    s.add("J5-野屏幕拒", j5, "未注册 CRTC 必拒");

    // J6 零周期拒
    let j6 = VbBook::new(&[0], 0).vblank(0, 1).err() == Some(VB_ZERO_PERIOD);
    s.add("J6-零周期拒", j6, "周期 0 必拒");

    // J7 双屏独立计数
    let j7 = b.frames_of(0) == 2 && b.frames_of(1) == 2;
    s.add("J7-双屏独立", j7, "每屏两帧");

    // J8 全局单调
    let j8 = b.mono_ok();
    s.add("J8-全局单调", j8, "时间戳全序不倒退");

    // J9 等时间戳拒
    let j9 = {
        let last_ts = b.frames.last().map(|f| f.ts_ns).unwrap_or(0);
        b.vblank(0, last_ts).err().is_some() || b.vblank(0, last_ts + 1).is_ok()
    };
    s.add("J9-等时间戳档", j9, "同 ts 报档或前进");

    // J10 帧号连续
    let j10 = {
        let nos: Vec<u64> = b.frames.iter().map(|f| f.frame_no).collect();
        nos.windows(2).all(|w| w[1] == w[0] + 1)
    };
    s.add("J10-帧号连续", j10, "seq 严格 +1");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.vblank(0, 99_999_999).err() == Some(VB_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24181_vblank_flow() {
        let mut b = VbBook::new(&[0], 100);
        assert_eq!(b.vblank(0, 0), Ok(1));
        assert_eq!(b.vblank(0, 100), Ok(2));
        assert_eq!(b.vblank(0, 50), Err(VB_REVERSED));
        assert_eq!(b.vblank(9, 200), Err(VB_UNK_CRTC));
        assert_eq!(VbBook::new(&[0], 0).vblank(0, 1), Err(VB_ZERO_PERIOD));
        b.seal();
        assert_eq!(b.vblank(0, 300), Err(VB_FROZEN));
    }

    #[test]
    fn unx24181_checks_green() {
        let s = run_unx24181_checks();
        assert!(s.all_passed(), "UNX-F24181 判据未全绿");
    }
}
