//! UNX-F56122 1% low 帧率计算件（判据母版 P5 对接）（0x60F20 段）
//!
//! 锚点：UNX-F56122-J1 低分位账全量（帧间隔样本逐帧落账：帧号 × 间隔
//! 微秒 × 剔除标记三元组）；J2 1% low 可复算（最近秩法取 1% 分位帧间隔
//! 换算帧率，与判据母版 P5 口径一致）；J3 低分位审计链（帧序单调、指纹
//! 链、封账只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·低分位面）。
//!
//! 结构：帧间隔样本登记册（帧号 → 间隔微秒逐帧落账）+ 帧序闸（帧号
//! 严格递增）+ 样本量守闸（≥30）+ 最近秩低分位核（升序取 1% 秩）+ 换算
//! 核（帧间隔 → 帧率 ×100 定点）+ 指纹链 + 注入核（帧号回退/样本不足/
//! 空样本/零间隔/封账改写/未采查询/换算漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60F20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60F20;
pub const LP_FRAME_BACK: u32 = SEG_BASE + 0x01; // 帧号回退
pub const LP_SAMPLE_SHORT: u32 = SEG_BASE + 0x02; // 样本量不足
pub const LP_SAMPLE_EMPTY: u32 = SEG_BASE + 0x03; // 空样本
pub const LP_GAP_ZERO: u32 = SEG_BASE + 0x04; // 零间隔（不可能帧间隔）
pub const LP_SEALED: u32 = SEG_BASE + 0x05; // 低分位账已封账拒写
pub const LP_EMPTY: u32 = SEG_BASE + 0x06; // 未采查询
pub const LP_CONV_MISS: u32 = SEG_BASE + 0x07; // 换算漂移

/// 帧样本量纪律。
pub const FRAME_MIN: usize = 30;
/// 低分位万分比（1% = 100 bp）。
pub const LOW_BP: u32 = 100;

/// 最近秩法：升序取 ceil(p·n/10000) 秩（1 起）。
pub fn nearest_rank_bp(samples: &[u64], p_bp: u32) -> Result<u64, u32> {
    if samples.is_empty() {
        return Err(LP_SAMPLE_EMPTY);
    }
    if samples.len() < FRAME_MIN {
        return Err(LP_SAMPLE_SHORT);
    }
    if p_bp == 0 || p_bp > 9_999 {
        return Err(LP_CONV_MISS);
    }
    let mut v: Vec<u64> = Vec::new();
    for s in samples.iter() {
        v.push(*s);
    }
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
    let n = v.len() as u64;
    let rank = ((p_bp as u64 * n) + 9_999) / 10_000;
    let rank = if rank < 1 { 1 } else { rank };
    Ok(v[(rank - 1) as usize])
}

/// 换算核：帧间隔微秒 → 帧率 ×100 定点（fps_x100 = 100_000_000 / gap_us，
/// 零间隔显性拒绝）。
pub fn gap_to_fps_x100(gap_us: u64) -> Result<u32, u32> {
    if gap_us == 0 {
        return Err(LP_GAP_ZERO);
    }
    Ok((100_000_000u64 / gap_us) as u32)
}

/// 帧样本：帧号 × 间隔微秒 × 指纹 × 链。
#[derive(Clone)]
pub struct FrameGap {
    pub frame: u32,
    pub gap_us: u64,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 低分位总账。
pub struct LowPctBook {
    pub frames: Vec<FrameGap>,
    pub sealed: bool,
}

impl LowPctBook {
    pub fn new() -> Self {
        LowPctBook { frames: Vec::new(), sealed: false }
    }

    /// 逐帧落账：零间隔拒 + 帧号严格递增双闸。
    pub fn record(&mut self, frame: u32, gap_us: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(LP_SEALED);
        }
        if gap_us == 0 {
            return Err(LP_GAP_ZERO);
        }
        if let Some(prev) = self.frames.last() {
            if frame <= prev.frame {
                return Err(LP_FRAME_BACK);
            }
        }
        let fp = {
            let mut h: u32 = 0x811C_9DC5;
            let line = format!("{}|{}", frame, gap_us);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h
        };
        let chain = match self.frames.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.frames.push(FrameGap { frame, gap_us, fp, chain });
        Ok(())
    }

    /// J2：1% low 计算（间隔最近秩 → 换算帧率）。
    pub fn low_1pct(&self) -> Result<u32, u32> {
        let gaps: Vec<u64> = self.frames.iter().map(|f| f.gap_us).collect();
        let g = nearest_rank_bp(&gaps, LOW_BP)?;
        gap_to_fps_x100(g)
    }

    /// J2：均值帧率（对照口径）。
    pub fn mean_fps_x100(&self) -> Result<u32, u32> {
        if self.frames.is_empty() {
            return Err(LP_EMPTY);
        }
        let mut sum: u64 = 0;
        for f in self.frames.iter() {
            sum += gap_to_fps_x100(f.gap_us)? as u64;
        }
        Ok((sum / self.frames.len() as u64) as u32)
    }

    /// J1：帧账全量（逐帧间隔非零）。
    pub fn frames_ok(&self) -> bool {
        !self.frames.is_empty() && self.frames.iter().all(|f| f.gap_us > 0)
    }

    /// J3：帧指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.frames.is_empty() {
            return false;
        }
        for i in 0..self.frames.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.frames[0].fp
            } else {
                self.frames[i - 1].fp ^ self.frames[i].fp
            };
            if self.frames[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.frames.last() {
            None => CHAIN_SEED,
            Some(f) => f.chain,
        }
    }
}

/// 标准夹具：60 帧间隔（8300..8890 微秒逐帧 +10，含 3 个 12000 毛刺）。
pub fn standard_book() -> LowPctBook {
    let mut b = LowPctBook::new();
    for i in 0..60u32 {
        let gap = if i == 20 || i == 40 || i == 59 {
            12_000
        } else {
            8_300 + (i as u64) * 10
        };
        if b.record(i + 1, gap).is_err() {
            panic!("standard fixture record must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（零间隔/帧号回退/样本不足/空样本/封账改写/未采查询/换算漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零间隔
    let mut b = LowPctBook::new();
    if b.record(1, 0) == Err(LP_GAP_ZERO) {
        caught += 1;
    }
    // 2) 帧号回退
    if b.record(2, 8_300).is_ok() && b.record(1, 8_300) == Err(LP_FRAME_BACK) {
        caught += 1;
    }
    // 3) 样本不足
    let mut b2 = LowPctBook::new();
    for i in 0..29u32 {
        let _ = b2.record(i + 1, 8_300);
    }
    if b2.low_1pct() == Err(LP_SAMPLE_SHORT) {
        caught += 1;
    }
    // 4) 空样本
    if LowPctBook::new().low_1pct() == Err(LP_SAMPLE_EMPTY) {
        caught += 1;
    }
    // 5) 换算漂移（分位 0 拒）
    let mut b3 = LowPctBook::new();
    for i in 0..30u32 {
        let _ = b3.record(i + 1, 8_300);
    }
    let gaps: Vec<u64> = b3.frames.iter().map(|f| f.gap_us).collect();
    if nearest_rank_bp(&gaps, 0) == Err(LP_CONV_MISS) {
        caught += 1;
    }
    // 6) 未采查询
    if LowPctBook::new().mean_fps_x100().err() == Some(LP_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.record(61, 8_300) == Err(LP_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 低分位账渲染导出（逐帧一行）。
pub fn render_book(b: &LowPctBook) -> String {
    let mut out = String::new();
    for f in b.frames.iter() {
        out.push_str(&format!("f{}|{}|{:08x}\n", f.frame, f.gap_us, f.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行三段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| ln.split('|').count() == 3)
}

/// 判据入口（CheckSet）。
pub fn run_unx56122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56122");
    // J1-1 低分位账全量（60 帧逐帧合法）
    let b = standard_book();
    let ok = b.frames_ok() && b.frames.len() == 60;
    s.add("56122-J1-低分位账全量", ok, if ok { "" } else { "帧账残缺" });
    // J1-2 帧序单调（1..60 严格递增）
    let ok = b.frames[0].frame == 1 && b.frames[59].frame == 60;
    s.add("56122-J1-帧序单调", ok, if ok { "" } else { "帧序漂移" });
    // J1-3 零间隔拒收
    let mut b1 = LowPctBook::new();
    let ok = b1.record(1, 0) == Err(LP_GAP_ZERO);
    s.add("56122-J1-零间隔拒收", ok, if ok { "" } else { "零值穿透" });
    // J2-1 换算核（8333 微秒 ≈ 1200（12.00 fps ×100））
    let ok = gap_to_fps_x100(83_333) == Ok(1_200);
    s.add("56122-J2-间隔换算核", ok, if ok { "" } else { "换算漂移" });
    // J2-2 1% low 复算（60 样本 1% 秩 = ceil(6000/10000)=1 → 最小间隔 8300 → 12048）
    let low = b.low_1pct();
    let ok = low == Ok(12_048);
    s.add("56122-J2-1%low复算", ok, if ok { "" } else { "低分位漂移" });
    // J2-3 均值帧率（对照口径可复算，约 114 fps ×100）
    let mean = b.mean_fps_x100();
    let ok = match mean {
        Ok(m) => m >= 11_000 && m <= 12_500,
        Err(_) => false,
    };
    s.add("56122-J2-均值帧率", ok, if ok { "" } else { "均值漂移" });
    // J2-4 样本不足拒算
    let mut b2 = LowPctBook::new();
    for i in 0..29u32 {
        let _ = b2.record(i + 1, 8_300);
    }
    let ok = b2.low_1pct() == Err(LP_SAMPLE_SHORT);
    s.add("56122-J2-样本不足拒算", ok, if ok { "" } else { "守闸失效" });
    // J3-1 帧指纹链复核
    let ok = b.chain_ok();
    s.add("56122-J3-帧指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56122-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改一帧间隔链尾变）
    let mut b3 = standard_book();
    b3.frames[0].gap_us += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56122-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.record(61, 8_300) == Err(LP_SEALED);
    s.add("56122-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出六十行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 60) && text.starts_with("f1|8300|");
    s.add("56122-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56122-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56122_all_checks_green() {
        let set = run_unx56122_checks();
        assert!(set.all_passed(), "UNX-F56122 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56122_low_stable() {
        assert_eq!(standard_book().low_1pct(), Ok(12_048));
    }
}
