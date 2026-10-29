//! UNX-F56130 帧率离群剔除件（0x61020 段）
//!
//! 锚点：UNX-F56130-J1 样本全量（帧率样本逐条落账：帧号 × 帧率 ×
//! 中断标记）；J2 剔除语义（场景中断样本先剥、四分位 IQR 带外判离
//! 群、剔除率 ≤20% 闭式）；J3 剔除审计链（封账门、指纹、剔除账复算）
//! 三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·离群剔除面）。
//!
//! 结构：样本登记簿（帧率 ×100 定点带、中断标记）+ 四分位核（最近
//! 秩位、IQR ×3/2 带）+ 剔除账闭式（复算对账）+ 指纹链 + 注入核（零
//! 帧率/封账拒录/未封先剔/中断清空/剔除率超限/序号越界/剔除账漂移必
//! 抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61020 独占 0x20）。
pub const SEG_BASE: u32 = 0x61020;
pub const OO_FPS_ZERO: u32 = SEG_BASE + 0x01; // 零帧率
pub const OO_FPS_RANGE: u32 = SEG_BASE + 0x02; // 帧率/序号越界
pub const OO_ALL_GONE: u32 = SEG_BASE + 0x03; // 中断清空（有效样本不足）
pub const OO_SEALED: u32 = SEG_BASE + 0x04; // 封账拒录
pub const OO_UNSEALED: u32 = SEG_BASE + 0x05; // 未封账先剔
pub const OO_RATE_OVER: u32 = SEG_BASE + 0x06; // 剔除率超限
pub const OO_STAT_MISS: u32 = SEG_BASE + 0x07; // 剔除账漂移

/// 帧率 ×100 定点合法带。
pub const FPS_MIN: u64 = 100;
pub const FPS_MAX: u64 = 120_000;

/// 剔除率上限（万分比 2000 = 20%）。
pub const RATE_LIMIT_BP: u64 = 2_000;

pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
}

/// 帧率样本。
pub struct FpsSample {
    pub no: usize,
    pub fps_x100: u64,
    pub interrupted: bool,
}

/// 剔除结果（对有效样本集）。
#[derive(Clone, Copy)]
pub struct PruneOut {
    pub lo: u64,
    pub hi: u64,
    pub flagged: u64,
    pub rate_bp: u64,
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

/// 帧率离群剔除登记簿。
pub struct FpsOutlierBook {
    cap: usize,
    samples: Vec<FpsSample>,
    out: Option<PruneOut>,
    chain: u32,
    sealed: bool,
}

impl FpsOutlierBook {
    pub fn new(cap: usize) -> Self {
        FpsOutlierBook {
            cap,
            samples: Vec::new(),
            out: None,
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 样本切片只读访问。
    pub fn samples(&self) -> &[FpsSample] {
        &self.samples
    }

    /// 剔除结果访问。
    pub fn result(&self) -> Option<PruneOut> {
        self.out
    }

    /// 落样本：零帧率/带外拒绝。
    pub fn record(&mut self, fps_x100: u64, interrupted: bool) -> Result<usize, u32> {
        if self.sealed || self.samples.len() >= self.cap {
            return Err(OO_SEALED);
        }
        if fps_x100 == 0 {
            return Err(OO_FPS_ZERO);
        }
        if !fps_ok(fps_x100) {
            return Err(OO_FPS_RANGE);
        }
        let no = self.samples.len();
        self.samples.push(FpsSample {
            no,
            fps_x100,
            interrupted,
        });
        Ok(no)
    }

    /// 按序号取样本。
    pub fn get(&self, no: usize) -> Result<(u64, bool), u32> {
        for s in &self.samples {
            if s.no == no {
                return Ok((s.fps_x100, s.interrupted));
            }
        }
        Err(OO_FPS_RANGE)
    }

    /// 剔除核：中断样本先剥 → 排序 → 最近秩四分位 → IQR×3/2 带外
    /// 判离群 → 剔除率闭式（≤20%）。
    pub fn prune(&mut self) -> Result<PruneOut, u32> {
        if !self.sealed {
            return Err(OO_UNSEALED);
        }
        let mut clean: Vec<u64> = self
            .samples
            .iter()
            .filter(|s| !s.interrupted)
            .map(|s| s.fps_x100)
            .collect();
        if clean.len() < 4 {
            return Err(OO_ALL_GONE);
        }
        // 插入排序（元组换位思想：值寄存逐位后移）。
        for i in 1..clean.len() {
            let mut j = i;
            while j > 0 && clean[j] < clean[j - 1] {
                let tmp = clean[j];
                clean[j] = clean[j - 1];
                clean[j - 1] = tmp;
                j -= 1;
            }
        }
        let n = clean.len();
        let q1 = clean[(n + 3) / 4 - 1];
        let q3 = clean[(3 * n + 1) / 4 - 1];
        let iqr = q3 - q1;
        let lo = q1.saturating_sub(iqr * 3 / 2);
        let hi = q3.saturating_add(iqr * 3 / 2);
        let flagged = clean.iter().filter(|&&v| v < lo || v > hi).count() as u64;
        let rate_bp = flagged * 10_000 / n as u64;
        if rate_bp > RATE_LIMIT_BP {
            return Err(OO_RATE_OVER);
        }
        let out = PruneOut {
            lo,
            hi,
            flagged,
            rate_bp,
        };
        self.out = Some(out);
        Ok(out)
    }

    /// 剔除账复算：结果与样本重算四方全等。
    pub fn verify_prune(&self) -> Result<(), u32> {
        let o = self.out.ok_or(OO_STAT_MISS)?;
        let mut clean: Vec<u64> = self
            .samples
            .iter()
            .filter(|s| !s.interrupted)
            .map(|s| s.fps_x100)
            .collect();
        if clean.len() < 4 {
            return Err(OO_STAT_MISS);
        }
        for i in 1..clean.len() {
            let mut j = i;
            while j > 0 && clean[j] < clean[j - 1] {
                let tmp = clean[j];
                clean[j] = clean[j - 1];
                clean[j - 1] = tmp;
                j -= 1;
            }
        }
        let n = clean.len();
        let q1 = clean[(n + 3) / 4 - 1];
        let q3 = clean[(3 * n + 1) / 4 - 1];
        let iqr = q3 - q1;
        let lo = q1.saturating_sub(iqr * 3 / 2);
        let hi = q3.saturating_add(iqr * 3 / 2);
        let flagged = clean.iter().filter(|&&v| v < lo || v > hi).count() as u64;
        if lo != o.lo || hi != o.hi || flagged != o.flagged {
            return Err(OO_STAT_MISS);
        }
        if flagged * 10_000 / n as u64 != o.rate_bp {
            return Err(OO_STAT_MISS);
        }
        Ok(())
    }

    /// 封账：空账拒绝、定指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(OO_SEALED);
        }
        if self.samples.is_empty() {
            return Err(OO_ALL_GONE);
        }
        self.chain = CHAIN_SEED;
        for s in &self.samples {
            self.chain = fold_u32(self.chain, s.fps_x100 as u32);
            self.chain = fold_u32(self.chain, s.interrupted as u32);
        }
        self.sealed = true;
        Ok(())
    }

    /// 渲染：每行 4 段（no|fps|int|mark）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.samples {
            let mark = if s.interrupted {
                "drop_int"
            } else {
                match self.out {
                    Some(o) if s.fps_x100 < o.lo || s.fps_x100 > o.hi => "flag",
                    _ => "keep",
                }
            };
            out.push(format!(
                "{}|{}|{}|{}",
                s.no, s.fps_x100, s.interrupted as u32, mark
            ));
        }
        out
    }

    /// 测试注入口：仅注入核使用。
    fn break_flagged(&mut self) {
        if let Some(o) = self.out.as_mut() {
            o.flagged += 1;
        }
    }
}

/// 标准账：10 有效样本 + 1 中断样本，离群 1 格（6000）、剔除率 1000bp。
fn fixture() -> FpsOutlierBook {
    let mut b = FpsOutlierBook::new(16);
    let vals: [(u64, bool); 11] = [
        (11_900, false),
        (11_950, false),
        (12_000, false),
        (12_000, false),
        (12_000, false),
        (12_000, true), // 场景中断样本
        (12_050, false),
        (12_100, false),
        (12_150, false),
        (12_200, false),
        (6_000, false),
    ];
    for (v, it) in vals {
        let _ = b.record(v, it);
    }
    let _ = b.seal();
    let _ = b.prune();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 零帧率必抓。
    {
        let mut b = FpsOutlierBook::new(16);
        caught += (b.record(0, false) == Err(OO_FPS_ZERO)) as usize;
    }
    // 2 封账拒录必抓。
    {
        let mut b = fixture();
        caught += (b.record(12_000, false) == Err(OO_SEALED)) as usize;
    }
    // 3 未封账先剔必抓。
    {
        let mut b = FpsOutlierBook::new(16);
        for _ in 0..5 {
            let _ = b.record(12_000, false);
        }
        caught += (b.prune().err() == Some(OO_UNSEALED)) as usize;
    }
    // 4 中断清空必抓。
    {
        let mut b = FpsOutlierBook::new(16);
        for _ in 0..5 {
            let _ = b.record(12_000, true);
        }
        let _ = b.seal();
        caught += (b.prune().err() == Some(OO_ALL_GONE)) as usize;
    }
    // 5 剔除率超限必抓（IQR=0 带 [100,100]，2/8 = 2500bp > 2000bp）。
    {
        let mut b = FpsOutlierBook::new(16);
        for _ in 0..6 {
            let _ = b.record(100, false);
        }
        let _ = b.record(200, false);
        let _ = b.record(100_000, false);
        let _ = b.seal();
        caught += (b.prune().err() == Some(OO_RATE_OVER)) as usize;
    }
    // 6 序号越界必抓。
    {
        let b = fixture();
        caught += (b.get(99) == Err(OO_FPS_RANGE)) as usize;
    }
    // 7 剔除账漂移必抓。
    {
        let mut b = fixture();
        b.break_flagged();
        caught += (b.verify_prune() == Err(OO_STAT_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56130 全量判据（15 项）。
pub fn run_unx56130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56130");
    let b = fixture();

    // J1 样本全量。
    let ok = b.len() == 11;
    s.add("56130-J1-样本十一条", ok, if ok { "" } else { "样本数不符" });
    let it_n = b.samples().iter().filter(|s| s.interrupted).count();
    let ok = it_n == 1;
    s.add("56130-J1-中断侧一", ok, if ok { "" } else { "中断样本数漂移" });
    let ok = b.len() - it_n == 10;
    s.add("56130-J1-有效样本十", ok, if ok { "" } else { "有效样本数漂移" });

    // J2 剔除语义。
    let o = b.result().unwrap_or(PruneOut { lo: 0, hi: 0, flagged: 0, rate_bp: 0 });
    let ok = o.lo == 11_800;
    s.add("56130-J2-下剔除带", ok, if ok { "" } else { "下带非 11800" });
    let ok = o.hi == 12_200;
    s.add("56130-J2-上剔除带", ok, if ok { "" } else { "上带非 12200" });
    let ok = o.hi - o.lo == 400 && (o.hi - o.lo) == 4 * 100;
    s.add("56130-J2-带宽合 IQR", ok, if ok { "" } else { "带宽非 4×IQR" });
    let ok = o.flagged == 1;
    s.add("56130-J2-离群一格", ok, if ok { "" } else { "离群数漂移" });
    let ok = o.rate_bp == 1_000 && o.rate_bp <= RATE_LIMIT_BP;
    s.add("56130-J2-剔除率千分", ok, if ok { "" } else { "剔除率漂移" });
    let has_low = b
        .samples()
        .iter()
        .any(|s| !s.interrupted && s.fps_x100 < o.lo);
    let ok = has_low && b.get(10) == Ok((6_000, false));
    s.add("56130-J2-离群值复核", ok, if ok { "" } else { "离群值不在账" });

    // J3 剔除审计链。
    let ok = b.sealed();
    s.add("56130-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.chain() != CHAIN_SEED;
    s.add("56130-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let ok = b.verify_prune().is_ok();
    s.add("56130-J3-剔除账对账", ok, if ok { "" } else { "剔除账复算失败" });
    let lines = b.render();
    let mut ok = lines.len() == 11;
    for l in &lines {
        if l.split('|').count() != 4 {
            ok = false;
        }
    }
    s.add("56130-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56130-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56130_all_checks_green() {
        let set = run_unx56130_checks();
        assert!(set.all_passed(), "UNX-F56130 红项: {}", set.red_items_text());
    }
}
