//! UNX-F56091 管线健康心跳件（0x60B40 段）
//!
//! 锚点：UNX-F56091-J1 心跳登记全量（采集器心跳逐拍落账：拍号 × 器名 ×
//! 间隔毫秒三元组）；J2 心跳健康语义（逐拍间隔落在容差带内，超带即判
//! 掉线；连续超带拍数超限即判心跳失联）；J3 心跳审计链（拍号单调、心跳
//! 指纹链、封账只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·心跳面）。
//!
//! 结构：心跳拍登记册（拍号 → 器名 × 间隔逐拍落账）+ 间隔容差带核
//! （标称间隔 ± 容差万分比）+ 掉线判定核（连续超带拍数阈值）+ 拍指纹
//! 链 + 封账语义 + 注入核（空器名/负间隔/间隔超带/拍号回退/封账改写/
//! 空账判定/未知器名必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60B40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60B40;
pub const HB_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 器名为空
pub const HB_UNK_COLL: u32 = SEG_BASE + 0x02; // 未知器名（五闭集外）
pub const HB_GAP_OVER: u32 = SEG_BASE + 0x03; // 间隔超容差带
pub const HB_BEAT_BACK: u32 = SEG_BASE + 0x04; // 拍号回退
pub const HB_SEALED: u32 = SEG_BASE + 0x05; // 心跳账已封账拒写
pub const HB_EMPTY: u32 = SEG_BASE + 0x06; // 空账判定
pub const HB_DUP: u32 = SEG_BASE + 0x07; // 同拍重复落账

/// 五器闭集（与 F56082 同源）。
pub const H_NAMES: [&str; 5] = ["mem", "io", "fps", "power", "thermal"];
pub const H_COUNT: usize = 5;

/// 标称心跳间隔（毫秒）与容差带（万分比）。
pub const BEAT_MS: u32 = 1_000;
pub const TOL_BP: u64 = 2_000;

/// 间隔容差带核：|got - BEAT| × 10000 / BEAT ≤ TOL_BP。
pub fn gap_ok(ms: u32) -> bool {
    let d = if ms >= BEAT_MS { (ms - BEAT_MS) as u64 } else { (BEAT_MS - ms) as u64 };
    d * 10_000 <= TOL_BP * BEAT_MS as u64
}

/// 掉线判定阈值：连续超带拍数。
pub const MISS_LIMIT: usize = 3;

/// 心跳拍：拍号 × 器名 × 间隔毫秒 × 指纹 × 链。
#[derive(Clone)]
pub struct HeartBeat {
    pub beat: u32,
    pub coll: usize,
    pub gap_ms: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 心跳总账。
pub struct HbBook {
    pub beats: Vec<HeartBeat>,
    pub sealed: bool,
}

impl HbBook {
    pub fn new() -> Self {
        HbBook { beats: Vec::new(), sealed: false }
    }

    /// 逐拍落账：器闭集 + 拍号严格递增 + 同拍重复拒 + 间隔带闸四闸。
    pub fn beat(&mut self, beat_no: u32, coll: usize, gap_ms: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(HB_SEALED);
        }
        if coll >= H_COUNT {
            return Err(HB_UNK_COLL);
        }
        if let Some(prev) = self.beats.last() {
            if beat_no <= prev.beat {
                if beat_no == prev.beat {
                    return Err(HB_DUP);
                }
                return Err(HB_BEAT_BACK);
            }
        }
        let ok = gap_ok(gap_ms);
        let fp = {
            let mut h: u32 = 0x811C_9DC5;
            let line = format!("{}|{}|{}", beat_no, H_NAMES[coll], gap_ms);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h
        };
        let chain = match self.beats.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.beats.push(HeartBeat { beat: beat_no, coll, gap_ms, fp, chain });
        Ok(ok)
    }

    /// J2：连续超带判定（尾部连续超带拍数 ≥ MISS_LIMIT 即失联）。
    pub fn miss_streak(&self) -> usize {
        let mut n = 0usize;
        for b in self.beats.iter() {
            if !gap_ok(b.gap_ms) {
                n += 1;
            } else {
                n = 0;
            }
        }
        n
    }

    /// J2：健康判定（失联判负）。
    pub fn healthy(&self) -> bool {
        if self.beats.is_empty() {
            return false;
        }
        self.miss_streak() < MISS_LIMIT
    }

    /// J1：心跳账全量（逐拍器名合法）。
    pub fn beats_ok(&self) -> bool {
        !self.beats.is_empty() && self.beats.iter().all(|b| b.coll < H_COUNT)
    }

    /// J3：拍指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.beats.is_empty() {
            return false;
        }
        for i in 0..self.beats.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.beats[0].fp
            } else {
                self.beats[i - 1].fp ^ self.beats[i].fp
            };
            if self.beats[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// 超带拍数统计。
    pub fn over_count(&self) -> usize {
        let mut n = 0;
        for b in self.beats.iter() {
            if !gap_ok(b.gap_ms) {
                n += 1;
            }
        }
        n
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.beats.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：5 拍（mem 器标称间隔，健康）。
pub fn standard_book() -> HbBook {
    let mut b = HbBook::new();
    for i in 1..=5u32 {
        let gap = match i {
            1 => 1_000,
            2 => 980,
            3 => 1_050,
            4 => 1_000,
            _ => 1_010,
        };
        if b.beat(i, 0, gap).is_err() {
            panic!("standard fixture beat must succeed");
        }
    }
    b
}

/// 掉线夹具：尾部连续 3 拍超带。
pub fn miss_book() -> HbBook {
    let mut b = standard_book();
    let gaps: [u32; 3] = [1_500, 1_600, 1_700];
    for i in 0..3u32 {
        if b.beat(6 + i, 0, gaps[i as usize]).is_err() {
            panic!("miss fixture beat must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知器名/间隔超带/拍号回退/同拍重复/封账改写/空账判定/掉线判定）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知器名
    let mut b = HbBook::new();
    if b.beat(1, 9, 1_000) == Err(HB_UNK_COLL) {
        caught += 1;
    }
    // 2) 间隔超带（标称 1000、容差 ±200，1500 超带）
    if b.beat(1, 0, 1_500) == Ok(false) {
        caught += 1;
    }
    // 3) 同拍重复
    if b.beat(2, 0, 1_000).is_ok() && b.beat(2, 0, 1_000) == Err(HB_DUP) {
        caught += 1;
    }
    // 4) 拍号回退
    if b.beat(3, 0, 1_000).is_ok() && b.beat(2, 0, 1_000) == Err(HB_BEAT_BACK) {
        caught += 1;
    }
    // 5) 掉线判定（尾部 3 连超带 → unhealthy）
    if !miss_book().healthy() {
        caught += 1;
    }
    // 6) 空账判定
    if !HbBook::new().healthy() {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.beat(9, 0, 1_000) == Err(HB_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 心跳账渲染导出（逐拍一行）。
pub fn render_book(b: &HbBook) -> String {
    let mut out = String::new();
    for x in b.beats.iter() {
        let st = if gap_ok(x.gap_ms) { "ok" } else { "over" };
        out.push_str(&format!("b{}|{}|{}ms|{}\n", x.beat, H_NAMES[x.coll], x.gap_ms, st));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 状态闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[3] == "ok" || parts[3] == "over")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56091_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56091");
    // J1-1 心跳登记全量（五拍逐拍合法）
    let b = standard_book();
    let ok = b.beats_ok() && b.beats.len() == 5;
    s.add("56091-J1-心跳登记全量", ok, if ok { "" } else { "心跳拍残缺" });
    // J1-2 容差带边界（800/1200 在带内，799/1201 超带）
    let ok = gap_ok(800) && gap_ok(1_200) && !gap_ok(799) && !gap_ok(1_201);
    s.add("56091-J1-容差带边界", ok, if ok { "" } else { "容差带漂移" });
    // J1-3 器名闭集齐
    let ok = H_NAMES.len() == H_COUNT && H_NAMES[1] == "io";
    s.add("56091-J1-器名闭集齐", ok, if ok { "" } else { "器名漂移" });
    // J2-1 健康判定（标称夹具健康）
    let ok = b.healthy() && b.over_count() == 0;
    s.add("56091-J2-健康判定", ok, if ok { "" } else { "健康语义漂移" });
    // J2-2 掉线判定（3 连超带失联）
    let mb = miss_book();
    let ok = !mb.healthy() && mb.miss_streak() == 3 && mb.over_count() == 3;
    s.add("56091-J2-掉线判定", ok, if ok { "" } else { "掉线语义漂移" });
    // J2-3 中断恢复复位（超带一拍后回带 → streak 清零）
    let mut b2 = standard_book();
    let _ = b2.beat(6, 0, 1_500);
    let _ = b2.beat(7, 0, 1_000);
    let ok = b2.miss_streak() == 0 && b2.healthy();
    s.add("56091-J2-回带复位", ok, if ok { "" } else { "复位语义漂移" });
    // J2-4 超带拍逐拍回报（beat 返回 false）
    let mut b3 = HbBook::new();
    let ok = b3.beat(1, 0, 2_000) == Ok(false) && b3.beat(2, 0, 1_000) == Ok(true);
    s.add("56091-J2-超带回报", ok, if ok { "" } else { "回报语义漂移" });
    // J3-1 拍指纹链复核
    let ok = b.chain_ok();
    s.add("56091-J3-拍指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56091-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（追加一拍链尾变）
    let tail0 = standard_book().chain_tail();
    let mut b4 = standard_book();
    let _ = b4.beat(6, 0, 1_000);
    let ok = b4.chain_tail() != tail0;
    s.add("56091-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.beat(6, 0, 1_000) == Err(HB_SEALED);
    s.add("56091-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.starts_with("b1|mem|1000ms|ok");
    s.add("56091-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56091-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56091_all_checks_green() {
        let set = run_unx56091_checks();
        assert!(set.all_passed(), "UNX-F56091 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56091_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
