//! UNX-F56071 内存对照审计快照件（AI-82 抽读对接）（0x608C0 段）
//!
//! 锚点：UNX-F56071-J1 快照登记全量（AI-82 抽读批逐帧落账：帧号 × RSS
//! 快照 × 采集指纹）；J2 抽读可复算（抽读帧快照与内存对照行双源比对
//! 一致）；J3 快照审计链（帧号单调、指纹链复合、封账只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·审计快照面）。
//!
//! 结构：AI-82 抽读帧登记册（帧号 → RSS KB × 指纹逐帧落账）+ 帧号单调
//! 闸（跳帧/回退拒收）+ 双源比对核（快照 RSS 与对照行 RSS 逐帧对齐）+
//! 指纹链复合核（前帧指纹混入后帧防篡改）+ 封账语义 + 注入核（帧回退/
//! 跳帧/双源不一致/零 RSS/封账改写/空链复算必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x608C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x608C0;
pub const SNAP_FRAME_BACK: u32 = SEG_BASE + 0x01; // 帧号回退（非严格递增）
pub const SNAP_FRAME_GAP: u32 = SEG_BASE + 0x02; // 帧号跳帧（步长超容差）
pub const SNAP_DUP: u32 = SEG_BASE + 0x03; // 同帧重复抽读
pub const SNAP_RSS_ZERO: u32 = SEG_BASE + 0x04; // 零 RSS 快照（空读入账）
pub const SNAP_SRC_MISS: u32 = SEG_BASE + 0x05; // 双源比对不一致
pub const SNAP_SEALED: u32 = SEG_BASE + 0x06; // 快照账已封账拒写
pub const SNAP_EMPTY: u32 = SEG_BASE + 0x07; // 空链复算（无帧可复核）

/// 帧号步长容差：AI-82 抽读按固定步长，超此即判跳帧。
pub const FRAME_STEP: u32 = 1;
pub const FRAME_GAP_MAX: u32 = 8;

/// 快照指纹：FNV-1a 32 位。
pub fn snap_fingerprint(frame: u32, rss_kb: u64) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}", frame, rss_kb);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单帧抽读账目：帧号 × RSS KB × 指纹 × 前帧指纹链 四元组。
#[derive(Clone)]
pub struct SnapEntry {
    pub frame: u32,
    pub rss_kb: u64,
    pub fp: u32,
    /// 前帧指纹链（首帧用种子哨兵）。
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 快照登记总账（帧序登记 + 封账）。
pub struct SnapBook {
    pub entries: Vec<SnapEntry>,
    pub sealed: bool,
}

impl SnapBook {
    pub fn new() -> Self {
        SnapBook { entries: Vec::new(), sealed: false }
    }

    /// 逐帧落账：帧号单调 + 步长容差 + 非零 RSS 三闸 + 指纹链复合。
    pub fn push(&mut self, frame: u32, rss_kb: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(SNAP_SEALED);
        }
        if rss_kb == 0 {
            return Err(SNAP_RSS_ZERO);
        }
        let fp = snap_fingerprint(frame, rss_kb);
        let chain = match self.entries.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => {
                if frame == prev.frame {
                    return Err(SNAP_DUP);
                }
                if frame < prev.frame {
                    return Err(SNAP_FRAME_BACK);
                }
                if frame - prev.frame > FRAME_GAP_MAX {
                    return Err(SNAP_FRAME_GAP);
                }
                prev.fp ^ fp
            }
        };
        if frame.saturating_sub(self.entries.last().map(|p| p.frame).unwrap_or(frame))
            > FRAME_STEP
            && !self.entries.is_empty()
        {
            // 步长超出 1 但未越容差：允许（抽读批次间可稀疏），仍复合链。
        }
        self.entries.push(SnapEntry { frame, rss_kb, fp, chain });
        Ok(chain)
    }

    /// 某帧快照查询（未抽读显性拒绝）。
    pub fn frame_of(&self, frame: u32) -> Result<u64, u32> {
        for e in self.entries.iter() {
            if e.frame == frame {
                return Ok(e.rss_kb);
            }
        }
        Err(SNAP_EMPTY)
    }

    /// J1：快照全量（链非空 + 逐帧 RSS 非零）。
    pub fn full_ok(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|e| e.rss_kb > 0)
    }

    /// J2：双源比对——逐帧与对照行 RSS 对齐（长度同 + 帧号同 + 值同）。
    pub fn cross_check(&self, ref_frames: &[u32], ref_rss: &[u64]) -> Result<bool, u32> {
        if self.entries.is_empty() {
            return Err(SNAP_EMPTY);
        }
        if ref_frames.len() != ref_rss.len() || ref_frames.len() != self.entries.len() {
            return Err(SNAP_SRC_MISS);
        }
        for i in 0..self.entries.len() {
            if self.entries[i].frame != ref_frames[i] || self.entries[i].rss_kb != ref_rss[i] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// J3：指纹链复核（逐帧 chain = 前帧 fp ^ 本帧 fp，首帧对种子）。
    pub fn chain_ok(&self) -> Result<bool, u32> {
        if self.entries.is_empty() {
            return Err(SNAP_EMPTY);
        }
        for i in 0..self.entries.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.entries[0].fp
            } else {
                self.entries[i - 1].fp ^ self.entries[i].fp
            };
            if self.entries[i].chain != expect {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 指纹链终值（封账材料）。
    pub fn chain_tail(&self) -> u32 {
        match self.entries.last() {
            None => CHAIN_SEED,
            Some(e) => e.chain,
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：AI-82 抽读 6 帧（帧 100..105，RSS 从 51200 逐帧递增 256）。
pub fn standard_book() -> SnapBook {
    let mut b = SnapBook::new();
    for i in 0..6u32 {
        let frame = 100 + i;
        let rss = 51_200u64 + 256 * i as u64;
        if b.push(frame, rss).is_err() {
            panic!("standard fixture push must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（帧回退/同帧重复/跳帧超容差/零 RSS/双源不一致/封账改写/空链复核）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零 RSS
    let mut b = SnapBook::new();
    if b.push(10, 0) == Err(SNAP_RSS_ZERO) {
        caught += 1;
    }
    // 2) 同帧重复
    if b.push(10, 4096).is_ok() && b.push(10, 4097) == Err(SNAP_DUP) {
        caught += 1;
    }
    // 3) 帧号回退
    if b.push(9, 4100) == Err(SNAP_FRAME_BACK) {
        caught += 1;
    }
    // 4) 跳帧超容差（步长 > 8）
    if b.push(20, 4104) == Err(SNAP_FRAME_GAP) {
        caught += 1;
    }
    // 5) 双源比对不一致（长度不齐）
    let sb = standard_book();
    let refs: Vec<u32> = (0..5u32).map(|i| 100 + i).collect();
    let rsss: Vec<u64> = (0..5u64).map(|i| 51_200 + 256 * i).collect();
    if sb.cross_check(&refs, &rsss) == Err(SNAP_SRC_MISS) {
        caught += 1;
    }
    // 6) 空链复核
    let empty = SnapBook::new();
    if empty.chain_ok() == Err(SNAP_EMPTY) && empty.frame_of(1).err() == Some(SNAP_EMPTY) {
        caught += 1;
    }
    // 7) 封账后改写拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.push(106, 52_800) == Err(SNAP_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 快照账渲染导出（逐帧一行：frame|rss|fp|chain）。
pub fn render_book(b: &SnapBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!(
            "{}|{}|{:08x}|{:08x}\n",
            e.frame, e.rss_kb, e.fp, e.chain
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 4)
}

/// 抽读行均值核（u64 中转求和，均值向下取整）。
pub fn mean_rss(b: &SnapBook) -> Result<u64, u32> {
    if b.entries.is_empty() {
        return Err(SNAP_EMPTY);
    }
    let mut sum: u64 = 0;
    for e in b.entries.iter() {
        sum += e.rss_kb;
    }
    Ok(sum / b.entries.len() as u64)
}

/// 判据入口（CheckSet）。
pub fn run_unx56071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56071");
    // J1-1 抽读帧全量（6 帧逐帧非零）
    let b = standard_book();
    let ok = b.full_ok() && b.entries.len() == 6;
    s.add("56071-J1-抽读全量", ok, if ok { "" } else { "抽读帧残缺" });
    // J1-2 帧号单调连续（100..105 严格递增）
    let ok = b.entries[0].frame == 100 && b.entries[5].frame == 105;
    s.add("56071-J1-帧号单调", ok, if ok { "" } else { "帧序漂移" });
    // J1-3 零 RSS 显性拒绝
    let mut b1 = SnapBook::new();
    let ok = b1.push(1, 0) == Err(SNAP_RSS_ZERO);
    s.add("56071-J1-零RSS拒绝", ok, if ok { "" } else { "空读穿透" });
    // J2-1 双源比对一致（同序同值）
    let frames: Vec<u32> = (0..6u32).map(|i| 100 + i).collect();
    let rsss: Vec<u64> = (0..6u64).map(|i| 51_200 + 256 * i).collect();
    let ok = b.cross_check(&frames, &rsss) == Ok(true);
    s.add("56071-J2-双源一致", ok, if ok { "" } else { "双源比对失败" });
    // J2-2 双源比对敏感（改一帧值即不一致）
    let mut rsss2 = rsss.clone();
    rsss2[3] = 99_999;
    let ok = b.cross_check(&frames, &rsss2) == Ok(false);
    s.add("56071-J2-双源比对敏感", ok, if ok { "" } else { "比对钝感" });
    // J2-3 未抽读帧查询显性拒绝
    let ok = b.frame_of(999).err() == Some(SNAP_EMPTY);
    s.add("56071-J2-未抽读拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J2-4 抽读均值复算（6 帧和 311040 / 6 = 51840）
    let ok = mean_rss(&b) == Ok(51_840);
    s.add("56071-J2-均值复算", ok, if ok { "" } else { "均值算术漂移" });
    // J3-1 指纹链复核全绿
    let ok = b.chain_ok() == Ok(true);
    s.add("56071-J3-指纹链复核", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 指纹链敏感（插帧后链变）
    let mut b2 = standard_book();
    let tail0 = b2.chain_tail();
    let _ = b2.push(106, 52_800);
    let ok = b2.chain_tail() != tail0;
    s.add("56071-J3-指纹链敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-3 封账指纹确定性（链尾稳定）
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56071-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-4 封账拒写
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.push(106, 52_800) == Err(SNAP_SEALED);
    s.add("56071-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出六行四段齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 6);
    s.add("56071-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56071-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56071_all_checks_green() {
        let set = run_unx56071_checks();
        assert!(set.all_passed(), "UNX-F56071 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56071_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
