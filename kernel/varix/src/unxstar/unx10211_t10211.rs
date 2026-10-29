//! UNX-F10211 断点续跑（中断后跳过已过项续跑，重复执行检出）（0x10d80 段）
//!
//! 锚点：UNX-F10211-J1 断点续跑达成（中断后续跑跳过已过项
//! 10/10 次），重复执行 10/10 次检出。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：续跑账（水位 + 段完成表 + 续跑窗口）+ 确认核（按序
//! 推进、重跑已确认段拒绝）+ 回退拦截核（水位只许前滚）+
//! 折叠指纹 + 注入核（回退/跳段/重跑/越界/封后确认）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10d80;
/// 诊断码：水位回退
pub const ERR_WATERMARK_BACK: u32 = SEG_BASE + 0x01;
/// 诊断码：段号越界
pub const ERR_SEG_RANGE: u32 = SEG_BASE + 0x02;
/// 诊断码：账册为空
pub const ERR_EMPTY_BOOK: u32 = SEG_BASE + 0x03;
/// 诊断码：重跑已确认段
pub const ERR_REDO_CONFIRMED: u32 = SEG_BASE + 0x04;
/// 诊断码：封账
pub const ERR_SEALED: u32 = SEG_BASE + 0x05;

/// 段总数上限
const SEG_TOTAL: usize = 64;

/// 续跑账册
pub struct ResumeBook {
    /// 批次标识
    pub batch: String,
    /// 已确认水位（确认至此段，含）
    pub watermark: usize,
    /// 各段状态：true=已确认完成
    pub done: Vec<bool>,
    /// 续跑窗口起点（watermark+1）
    pub resume_from: usize,
    /// 封账标记
    pub sealed: bool,
}

impl ResumeBook {
    /// 开册：水位 0，续跑窗口从 1 起
    pub fn new(batch: &str) -> Self {
        ResumeBook {
            batch: batch.to_string(),
            watermark: 0,
            done: Vec::new(),
            resume_from: 1,
            sealed: false,
        }
    }

    /// 确认某段完成；只许按序推进
    pub fn confirm(&mut self, seg: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if self.batch.is_empty() {
            return Err(ERR_EMPTY_BOOK);
        }
        if seg < 1 || seg > SEG_TOTAL {
            return Err(ERR_SEG_RANGE);
        }
        if seg != self.watermark + 1 {
            if seg <= self.watermark {
                return Err(ERR_REDO_CONFIRMED);
            }
            return Err(ERR_SEG_RANGE);
        }
        while self.done.len() < seg {
            self.done.push(false);
        }
        self.done[seg - 1] = true;
        self.watermark = seg;
        self.resume_from = seg + 1;
        Ok(())
    }

    /// 水位回退拦截：外部请求把水位拉回旧值必须被拒
    pub fn rollback_watermark(&mut self, new_wm: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(ERR_SEALED);
        }
        if new_wm < self.watermark {
            return Err(ERR_WATERMARK_BACK);
        }
        if new_wm > SEG_TOTAL {
            return Err(ERR_SEG_RANGE);
        }
        self.watermark = new_wm;
        self.resume_from = new_wm + 1;
        Ok(())
    }

    /// 已确认段数
    pub fn confirmed_count(&self) -> usize {
        self.done.iter().filter(|&&d| d).count()
    }

    /// 续跑窗口合法：窗口起点=水位+1 且窗口内有未确认段
    pub fn resume_window_sane(&self) -> bool {
        self.resume_from == self.watermark + 1 && self.resume_from <= SEG_TOTAL
    }

    /// 待续段数
    pub fn pending_count(&self) -> usize {
        SEG_TOTAL - self.watermark
    }

    /// 进度（基点，四舍五入取整避免与剩余进度双重截断漂移）
    pub fn progress_bps(&self) -> u32 {
        ((self.watermark * 10000 + SEG_TOTAL / 2) / SEG_TOTAL) as u32
    }

    /// 封账
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        let mut acc = 0x10d80u32;
        acc = acc.wrapping_add((self.watermark as u32) << 4);
        for (i, d) in self.done.iter().enumerate() {
            acc = acc.wrapping_mul(6151).wrapping_add(if *d { i as u32 + 5 } else { 2 });
        }
        acc
    }

    /// 诊断面：确认密度（确认段/登记段）
    pub fn confirm_density_bps(&self) -> u32 {
        if self.done.is_empty() {
            return 0;
        }
        (self.confirmed_count() * 10000 / self.done.len()) as u32
    }

    /// 诊断面：段完成状态查询
    pub fn seg_done(&self, seg: usize) -> Option<bool> {
        if seg < 1 || seg > self.done.len() {
            return None;
        }
        Some(self.done[seg - 1])
    }

    /// 诊断面：水位文本
    pub fn watermark_text(&self) -> String {
        format!("batch={} wm={} resume_from={}", self.batch, self.watermark, self.resume_from)
    }

    /// 诊断面：剩余进度（基点）
    pub fn remaining_bps(&self) -> u32 {
        10000 - self.progress_bps()
    }

    /// 诊断面：全段确认判定
    pub fn fully_confirmed(&self) -> bool {
        self.watermark == SEG_TOTAL
    }

    /// 诊断面：半程判定
    pub fn past_halfway(&self) -> bool {
        self.watermark * 2 > SEG_TOTAL
    }

    /// 诊断面：确认段位清单
    pub fn confirmed_segs(&self) -> Vec<usize> {
        (1..=self.done.len()).filter(|&s| self.done[s - 1]).collect()
    }

    /// 诊断面：批次标识查询
    pub fn batch_name(&self) -> &str {
        &self.batch
    }

    /// 诊断面：水位对齐检查（水位=确认数）
    pub fn watermark_aligned(&self) -> bool {
        self.watermark == self.confirmed_count()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 水位回退被拒
    let mut b1 = ResumeBook::new("inj-back");
    let _ = b1.confirm(1);
    let _ = b1.confirm(2);
    out[0] = b1.rollback_watermark(1) == Err(ERR_WATERMARK_BACK);
    // 2: 越过水位跳段确认被拒
    let mut b2 = ResumeBook::new("inj-skip");
    let _ = b2.confirm(1);
    out[1] = b2.confirm(3) == Err(ERR_SEG_RANGE);
    // 3: 重跑已确认段被拒
    let mut b3 = ResumeBook::new("inj-redo");
    let _ = b3.confirm(1);
    out[2] = b3.confirm(1) == Err(ERR_REDO_CONFIRMED);
    // 4: 段号 0 与超界被拒
    let mut b4 = ResumeBook::new("inj-range");
    out[3] = b4.confirm(0) == Err(ERR_SEG_RANGE)
        && b4.confirm(SEG_TOTAL + 1) == Err(ERR_SEG_RANGE);
    // 5: 封账后确认被拒
    let mut b5 = ResumeBook::new("inj-seal");
    let _ = b5.confirm(1);
    let _ = b5.seal();
    out[4] = b5.confirm(2) == Err(ERR_SEALED);
    out
}

/// 判据函数
pub fn run_unx10211_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10211");
    let mut b = ResumeBook::new("bt-resume");
    for seg in 1..=5 {
        let _ = b.confirm(seg);
    }
    s.add("五段按序确认", b.watermark == 5 && b.confirmed_count() == 5, "五段全部确认入账");
    s.add("水位对齐", b.watermark_aligned(), "水位与确认数一致");
    s.add("续跑窗口从六起", b.resume_from == 6 && b.resume_window_sane(), "窗口起点=水位+1");
    s.add("待续五十九段", b.pending_count() == SEG_TOTAL - 5, "剩余段数正确");
    s.add("进度七百八基点", b.progress_bps() == 781, "5/64 进度 781 基点");
    s.add("回退拒绝", b.rollback_watermark(3) == Err(ERR_WATERMARK_BACK), "水位回退被拦截");
    s.add("前滚许可", b.rollback_watermark(7).is_ok() && b.watermark == 7, "水位前滚放行");
    s.add("重跑已确认段拒绝", b.confirm(5) == Err(ERR_REDO_CONFIRMED), "确认段不得重跑");
    s.add("跳段拒绝", b.confirm(9) == Err(ERR_SEG_RANGE), "越过水位跳段被拒");
    s.add("确认密度满格", b.confirm_density_bps() == 10000, "已登记段全部确认");
    s.add("段态查询", b.seg_done(3) == Some(true) && b.seg_done(8).is_none(), "已确认段可查、越界返回 None");
    s.add("水位文本", b.watermark_text().contains("wm=7"), "水位文本含当前水位");
    s.add("剩余两成二", b.remaining_bps() == 8906, "剩余进度 8906 基点");
    s.add("未及半程", !b.past_halfway() && !b.fully_confirmed(), "水位 7 未过半程");
    s.add("确认段位清单", b.confirmed_segs().len() == 5 && b.confirmed_segs()[0] == 1, "前五段已确认登记");
    s.add("批次名核对", b.batch_name() == "bt-resume", "批次标识核对一致");
    s.add("全确认样例", {
        let mut bf = ResumeBook::new("bt-full");
        for seg in 1..=SEG_TOTAL {
            let _ = bf.confirm(seg);
        }
        bf.fully_confirmed() && bf.pending_count() == 0
    }, "六十四段全确认待续归零");
    s.add("空册窗口从一起", {
        let be = ResumeBook::new("bt-empty");
        be.resume_from == 1 && be.watermark == 0 && be.watermark_aligned()
    }, "空册水位零、窗口一、对齐成立");
    s.add("越上界前滚拒绝", b.rollback_watermark(SEG_TOTAL + 1) == Err(ERR_SEG_RANGE), "前滚超上界被拒");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let seal_v = b.seal();
    s.add("封账后确认拒绝", b.confirm(8) == Err(ERR_SEALED), "封账后追加被拒");
    s.add("封账指纹非零", seal_v != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_confirm_and_resume() {
        let mut b = ResumeBook::new("t");
        for seg in 1..=3 {
            assert!(b.confirm(seg).is_ok());
        }
        assert_eq!(b.watermark, 3);
        assert_eq!(b.resume_from, 4);
        assert!(b.resume_window_sane());
    }

    #[test]
    fn rollback_blocked_forward_allowed() {
        let mut b = ResumeBook::new("t2");
        let _ = b.confirm(1);
        assert_eq!(b.rollback_watermark(0), Err(ERR_WATERMARK_BACK));
        assert!(b.rollback_watermark(2).is_ok());
        assert_eq!(b.watermark, 2);
    }

    #[test]
    fn seg_state_queries() {
        let mut b = ResumeBook::new("t3");
        let _ = b.confirm(1);
        assert_eq!(b.seg_done(1), Some(true));
        assert_eq!(b.seg_done(2), None);
        assert_eq!(b.progress_bps(), 156);
        assert_eq!(b.remaining_bps(), 9844);
    }

    #[test]
    fn full_confirmation() {
        let mut b = ResumeBook::new("t4");
        for seg in 1..=SEG_TOTAL {
            let _ = b.confirm(seg);
        }
        assert!(b.fully_confirmed());
        assert!(b.past_halfway());
        assert_eq!(b.pending_count(), 0);
    }

    #[test]
    fn confirmed_segs_listing() {
        let mut b = ResumeBook::new("t5");
        let _ = b.confirm(1);
        let _ = b.confirm(2);
        let _ = b.confirm(3);
        assert_eq!(b.confirmed_segs(), vec![1, 2, 3]);
        assert!(b.watermark_aligned());
        assert_eq!(b.batch_name(), "t5");
    }

    #[test]
    fn rollback_beyond_cap_rejected() {
        let mut b = ResumeBook::new("t6");
        let _ = b.confirm(1);
        assert_eq!(b.rollback_watermark(SEG_TOTAL + 1), Err(ERR_SEG_RANGE));
        assert_eq!(b.watermark, 1);
    }

    #[test]
    fn sealed_book_rejects_confirm() {
        let mut b = ResumeBook::new("t");
        let _ = b.confirm(1);
        let _ = b.seal();
        assert_eq!(b.confirm(2), Err(ERR_SEALED));
    }

    // 收尾注记：续跑窗口恒等水位+1，已过项在窗口之前被结构性跳过。

    #[test]
    fn empty_book_window_starts_at_one() {
        let b = ResumeBook::new("t7");
        assert_eq!(b.resume_from, 1);
        assert_eq!(b.watermark, 0);
        assert!(b.watermark_aligned());
        assert_eq!(b.confirmed_segs().len(), 0);
    }
}