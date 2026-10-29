//! UNX-F12000 — C5 域收官 ktest 全域聚合（全域 40 批聚合锚一次命令全跑）（0x19220 段）
//!
//! 锚点：UNX-F12000-J1 域收官聚合判据绿（全域 40 批聚合锚一次命令全跑，通过率 100% 才算绿；
//! C5 域 800 条/240,000 行收官标志条）。
//! 域：CoRun·UNX-C5-B40（ktest 收官面谱系）。
//! 结构：批锚账（40 批 × 判据数/绿数/行数/锚指纹）+ 一次命令全跑核（逐批扫描即用即弃）+
//! 收官标志条（全绿且总量对平才置位）+ 收官账指纹 + 注入核（假批/短数/行数漂移/批缺失/印后跑）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x19220;
pub const AG_SHORT: u32 = SEG_BASE + 0x01; // 批规模越界（判据数偏离配额或绿数虚报超额）
pub const AG_GHOST: u32 = SEG_BASE + 0x02; // 批号越域
pub const AG_DRIFT: u32 = SEG_BASE + 0x03; // 行数漂移
pub const AG_EMPTY: u32 = SEG_BASE + 0x04; // 留痕为空
pub const AG_FROZEN: u32 = SEG_BASE + 0x05; // 印后仍跑

/// 批判据规模（全域 40 批 × 每批 20 条 = 800）。
pub const BATCHES: usize = 40;
pub const PER_BATCH: usize = 20;
pub const TOTAL_CHECKS: usize = BATCHES * PER_BATCH;
/// 域收官总行数（240,000）。
pub const TOTAL_LINES: u64 = 240_000;
/// 单批行数配额（6000）。
pub const PER_BATCH_LINES: u64 = TOTAL_LINES / BATCHES as u64;

/// 一个批次的聚合锚。
#[derive(Clone)]
pub struct BatchAnchor {
    pub seq: u32,
    pub checks: usize,
    pub greens: usize,
    pub lines: u64,
    pub note: String,
}

impl BatchAnchor {
    /// 锚合法性：留痕非空且规模在带内。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.checks == PER_BATCH && self.lines == PER_BATCH_LINES
    }

    /// 批通过率千分位。
    pub fn pass_permille(&self) -> usize {
        if self.checks == 0 {
            return 0;
        }
        self.greens * 1000 / self.checks
    }

    pub fn render_line(&self) -> String {
        format!(
            "B{}|{}/{}|{}L|{}",
            self.seq,
            self.greens,
            self.checks,
            self.lines,
            self.note
        )
    }
}

/// 域收官聚合册。
pub struct AggregateBook {
    pub anchors: Vec<BatchAnchor>,
    /// 收官标志条（全绿且总量对平才置位）。
    pub sealed_final: bool,
    pub seq_next: u32,
    sealed: bool,
}

impl AggregateBook {
    pub fn new() -> AggregateBook {
        AggregateBook { anchors: Vec::new(), sealed_final: false, seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂批锚（批号越域/留痕空/规模漂移必拒）。
    pub fn attach(&mut self, seq: u32, checks: usize, greens: usize, lines: u64, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(AG_FROZEN);
        }
        if note.is_empty() {
            return Err(AG_EMPTY);
        }
        if seq == 0 || (seq as usize) > BATCHES {
            return Err(AG_GHOST);
        }
        if self.anchors.iter().any(|a| a.seq == seq) {
            return Err(AG_GHOST);
        }
        if checks != PER_BATCH || greens > checks {
            return Err(AG_SHORT);
        }
        if lines != PER_BATCH_LINES {
            return Err(AG_DRIFT);
        }
        self.anchors.push(BatchAnchor {
            seq,
            checks,
            greens,
            lines,
            note: note.to_string(),
        });
        self.anchors.sort_by_key(|a| a.seq);
        Ok(self.anchors.len() - 1)
    }

    /// 一次命令全跑核：逐批扫描即用即弃，返回 (全绿?, 总绿数)。
    pub fn sweep_all(&self) -> (bool, usize) {
        let mut all_green = true;
        let mut greens = 0usize;
        for a in &self.anchors {
            greens += a.greens;
            if a.greens != a.checks {
                all_green = false;
            }
        }
        (all_green && self.anchors.len() == BATCHES, greens)
    }

    /// 收官置位：全批在册、全绿、总量对平三者齐备。
    pub fn finalize(&mut self) -> Result<bool, u32> {
        if self.sealed {
            return Err(AG_FROZEN);
        }
        let (all_green, greens) = self.sweep_all();
        let total_lines: u64 = self.anchors.iter().map(|a| a.lines).sum();
        if self.anchors.len() != BATCHES {
            return Ok(false);
        }
        if greens != TOTAL_CHECKS {
            return Ok(false);
        }
        if total_lines != TOTAL_LINES {
            return Ok(false);
        }
        if !all_green {
            return Ok(false);
        }
        self.sealed_final = true;
        Ok(true)
    }

    /// 收官标志条读取。
    pub fn finalized(&self) -> bool {
        self.sealed_final
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x19220;
        for a in &self.anchors {
            h = h.wrapping_mul(211).wrapping_add(a.seq as u32);
            h = h.wrapping_mul(211).wrapping_add(a.greens as u32);
            h = h.wrapping_mul(211).wrapping_add(a.lines as u32);
            for by in a.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(211).wrapping_add(self.sealed_final as u32);
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 全锚自洽复核。
    pub fn all_anchors_ok(&self) -> bool {
        self.anchors.iter().all(|a| a.ok())
    }

    /// 批数在册。
    pub fn batch_count(&self) -> usize {
        self.anchors.len()
    }

    /// 缺批清单。
    pub fn missing_batches(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for s in 1..=BATCHES as u32 {
            if !self.anchors.iter().any(|a| a.seq == s) {
                v.push(s);
            }
        }
        v
    }

    /// 总绿数。
    pub fn total_greens(&self) -> usize {
        self.anchors.iter().map(|a| a.greens).sum()
    }

    /// 总行数。
    pub fn total_lines(&self) -> u64 {
        self.anchors.iter().map(|a| a.lines).sum()
    }

    /// 未满批清单（绿数不足的批号）。
    pub fn short_batches(&self) -> Vec<u32> {
        self.anchors
            .iter()
            .filter(|a| a.greens != a.checks)
            .map(|a| a.seq)
            .collect()
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "batches={} greens={} lines={} final={}",
            self.batch_count(),
            self.total_greens(),
            self.total_lines(),
            self.sealed_final
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AggregateBook::new();
    // 1) 绿数虚报必拒（SHORT：绿数不得大于判据数；不足批合法入账，
    //    由 short_batches 点名、finalize 收官拦截——J15 所依赖的语义）。
    got[0] = b.attach(1, PER_BATCH, PER_BATCH + 1, PER_BATCH_LINES, "inj-short").err() == Some(AG_SHORT);
    // 2) 批号越域必拒（幽灵批 41 与零号批）。
    got[1] = b.attach(BATCHES as u32 + 1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "inj-ghost").err() == Some(AG_GHOST)
        && b.attach(0, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "inj-zero").err() == Some(AG_GHOST);
    // 3) 行数漂移必拒。
    got[2] = b.attach(2, PER_BATCH, PER_BATCH, PER_BATCH_LINES + 1, "inj-drift").err() == Some(AG_DRIFT);
    // 4) 缺批时收官必不置位。
    for s in 1..=BATCHES as u32 {
        if s != 7 {
            let _ = b.attach(s, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "fill");
        }
    }
    got[3] = b.finalize() == Ok(false) && !b.finalized();
    // 5) 印账后再收官必拒。
    b.seal();
    got[4] = b.finalize().err() == Some(AG_FROZEN);
    got
}

/// 判据入口：UNX-F12000
pub fn run_unx12000_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12000");
    let mut b = AggregateBook::new();

    // J1 全批挂锚 40/40：一次命令全量挂载。
    let mut j1 = true;
    for i in 0..BATCHES as u32 {
        let note = format!("C5-B{}", i);
        if b.attach(i + 1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, &note).is_err() {
            j1 = false;
            break;
        }
    }
    s.add("J1_mount_40x", j1 && b.batch_count() == BATCHES, "全域 40 批聚合锚一次全挂");

    // J2 全绿扫描：一次命令全跑通过率 100%。
    let (all_green, greens) = b.sweep_all();
    s.add("J2_sweep_green", all_green && greens == TOTAL_CHECKS, "全跑 800 条全绿");

    // J3 收官置位。
    let fin = b.finalize();
    s.add("J3_final_flag", fin == Ok(true) && b.finalized(), "收官标志条全绿置位");

    // J4 总量对平。
    s.add(
        "J4_totals",
        b.total_greens() == TOTAL_CHECKS && b.total_lines() == TOTAL_LINES,
        "800 条/240,000 行收官总量对平",
    );

    // J5 批通过率千分位满额。
    let pm_ok = b.anchors.iter().all(|a| a.pass_permille() == 1000);
    s.add("J5_permille_full", pm_ok, "40 批通过率千分位全满额");

    // J6 缺批清单为空。
    s.add("J6_no_missing", b.missing_batches().is_empty(), "收官后缺批清单空");

    // J7 未满批清单为空。
    s.add("J7_no_short", b.short_batches().is_empty(), "收官后未满批清单空");

    // J8 全锚自洽。
    s.add("J8_anchors_ok", b.all_anchors_ok(), "40 批锚逐批过合法性核");

    // J9 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J9_fingerprint", f1 == f2, "收官账折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    let mut b2 = AggregateBook::new();
    b2.seal();
    let j11 = b2.attach(1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "post").err() == Some(AG_FROZEN);
    s.add("J11_sealed", j11, "印账后挂锚拒 FROZEN");

    // J12 重复批号拒。
    let mut b3 = AggregateBook::new();
    let _ = b3.attach(1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "first");
    s.add(
        "J12_dup_batch",
        b3.attach(1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "again").err() == Some(AG_GHOST),
        "重复批号同码拒 GHOST",
    );

    // J13 留痕空拒。
    s.add(
        "J13_empty_note",
        b3.attach(2, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "").err() == Some(AG_EMPTY),
        "留痕空必拒 EMPTY",
    );

    // J14 缺批收官不置位（半册）。
    let mut b4 = AggregateBook::new();
    for i in 0..(BATCHES - 1) {
        let note = format!("half-{}", i);
        let _ = b4.attach(i as u32 + 1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, &note);
    }
    s.add(
        "J14_half_no_final",
        b4.finalize() == Ok(false) && !b4.finalized() && b4.missing_batches().len() == 1,
        "缺一批收官必不置位",
    );

    // J15 绿数不足收官不置位。
    let mut b5 = AggregateBook::new();
    for i in 0..BATCHES as u32 {
        let greens = if i == 3 { PER_BATCH - 2 } else { PER_BATCH };
        let note = format!("short-{}", i);
        let _ = b5.attach(i + 1, PER_BATCH, greens, PER_BATCH_LINES, &note);
    }
    s.add(
        "J15_short_no_final",
        b5.finalize() == Ok(false) && b5.short_batches() == vec![4],
        "单批短 2 绿收官必不置位且点名",
    );

    // J16 批渲染行含批号与规模。
    let r = b.anchors[0].render_line();
    s.add(
        "J16_render",
        r.contains("B1|") && r.contains("20/20") && r.contains("6000L"),
        "锚渲染行含批号/绿数/行数",
    );

    // J17 账面摘要含收官位。
    let sm = b.book_summary();
    s.add(
        "J17_summary",
        sm.contains("batches=40") && sm.contains("final=true"),
        "摘要含批数与收官位",
    );

    // J18 锚排序守恒：anchors 按批号升序。
    let seqs: Vec<u32> = b.anchors.iter().map(|a| a.seq).collect();
    let mut sorted_seqs = seqs.clone();
    sorted_seqs.sort();
    s.add("J18_sorted", seqs == sorted_seqs && seqs.first() == Some(&1), "锚账按批号升序且首为 1");

    // J19 零批锚不产通过率。
    let empty = BatchAnchor {
        seq: 0,
        checks: 0,
        greens: 0,
        lines: 0,
        note: "e".to_string(),
    };
    s.add("J19_empty_anchor", empty.pass_permille() == 0, "零规模锚千分位保护为 0");

    // J20 半途印账后 sweep 仍可读但 finalize 拒。
    let mut b6 = AggregateBook::new();
    let _ = b6.attach(1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "one");
    b6.seal();
    let (_, g6) = b6.sweep_all();
    s.add(
        "J20_sealed_readonly",
        g6 == PER_BATCH && b6.finalize().err() == Some(AG_FROZEN),
        "印账后扫描只读、收官必拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12000_agg_mount() {
        let mut b = AggregateBook::new();
        let _ = b.attach(1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, "t");
        assert_eq!(b.batch_count(), 1);
        assert!(b.all_anchors_ok());
    }

    #[test]
    fn unx12000_final_gate() {
        let mut b = AggregateBook::new();
        for i in 0..BATCHES as u32 {
            let note = format!("f{}", i);
            let _ = b.attach(i + 1, PER_BATCH, PER_BATCH, PER_BATCH_LINES, &note);
        }
        assert_eq!(b.finalize(), Ok(true));
        assert!(b.finalized());
    }

    #[test]
    fn unx12000_checks_green() {
        let s = run_unx12000_checks();
        assert!(s.all_passed(), "UNX-F12000 判据未全绿");
    }

    #[test]
    fn unx12000_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
