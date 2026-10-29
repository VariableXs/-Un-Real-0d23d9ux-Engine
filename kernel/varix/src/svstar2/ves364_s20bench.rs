//! ves364_s20bench — VE-F4124 · T07 i18n 基准套件
//! 五组基准（查找/切换/排版/资源加载/翻译管线）×语种矩阵 + 基准运行器（一键全量+单组模式）+ 基线哈希冻结制（漂移立案）+ 矩阵缺格补测 + 无障碍渲染基准组（域本色）+ 数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD240;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BnCode {
    Ok = 0,
    BaselineDrift = SEG_BASE + 0x01,
    MatrixHole = SEG_BASE + 0x02,
    BenchDead = SEG_BASE + 0x03,
}

/// 五组基准 + 无障碍专组（域本色，第六组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchGroup { Lookup = 0, Switch = 1, Layout = 2, Load = 3, Pipeline = 4, A11yRender = 5 }

impl BenchGroup {
    pub const ALL: [BenchGroup; 6] = [
        BenchGroup::Lookup, BenchGroup::Switch, BenchGroup::Layout,
        BenchGroup::Load, BenchGroup::Pipeline, BenchGroup::A11yRender];
    pub fn name(self) -> &'static str {
        match self { BenchGroup::Lookup => "lookup", BenchGroup::Switch => "switch",
            BenchGroup::Layout => "layout", BenchGroup::Load => "load",
            BenchGroup::Pipeline => "pipeline", BenchGroup::A11yRender => "a11y-render" }
    }
}

/// 语种矩阵。
pub const BENCH_LANGS: [&'static str; 4] = ["en", "zh", "ar", "th"];

/// 一次基准结果。
#[derive(Clone, Copy)]
pub struct BenchRun { pub group: BenchGroup, pub lang_idx: usize, pub ns: u32 }

/// 基线册：哈希冻结。
pub struct BaselineBook { pub frozen_hash: u32, pub runs: Vec<BenchRun>, pub dead_groups: Vec<&'static str> }

pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl BaselineBook {
    pub fn new() -> Self { BaselineBook { frozen_hash: 0, runs: Vec::new(), dead_groups: Vec::new() } }

    /// 冻结基线：对全部结果序列哈希。
    pub fn freeze(&mut self) -> u32 {
        let mut buf = Vec::new();
        for r in self.runs.iter() {
            buf.push(r.group as u8);
            buf.push(r.lang_idx as u8);
            buf.extend_from_slice(&r.ns.to_le_bytes());
        }
        self.frozen_hash = fnv1a(&buf);
        self.frozen_hash
    }

    /// 基线漂移检测：重算哈希与冻结不符 → 立案。
    pub fn drift_check(&self) -> Result<(), BnCode> {
        let mut buf = Vec::new();
        for r in self.runs.iter() {
            buf.push(r.group as u8);
            buf.push(r.lang_idx as u8);
            buf.extend_from_slice(&r.ns.to_le_bytes());
        }
        if fnv1a(&buf) != self.frozen_hash { return Err(BnCode::BaselineDrift); }
        Ok(())
    }

    /// 基准失效：某组全零 ns → 失效。
    pub fn scan_dead(&mut self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for g in BenchGroup::ALL.iter() {
            let rs: Vec<u32> = self.runs.iter().filter(|r| r.group == *g).map(|r| r.ns).collect();
            if !rs.is_empty() && rs.iter().all(|v| *v == 0) {
                out.push(g.name());
            }
        }
        self.dead_groups = out.clone();
        out
    }

    /// 矩阵缺格：组×语种全覆盖。
    pub fn holes(&self, groups: &[BenchGroup]) -> Vec<(BenchGroup, &'static str)> {
        let mut out = Vec::new();
        for g in groups.iter() {
            for (i, l) in BENCH_LANGS.iter().enumerate() {
                if !self.runs.iter().any(|r| r.group == *g && r.lang_idx == i) {
                    out.push((*g, *l));
                }
            }
        }
        out
    }
}

/// 运行器：一键全量（六组×四语种）+ 单组模式。
pub struct Runner { pub lang_ns: [[u32; 4]; 6] } // 组×语种 → 模拟耗时（合成）

impl Runner {
    pub fn new() -> Self {
        // 合成数据：确定性伪耗时。
        let mut lang_ns = [[0u32; 4]; 6];
        for (gi, row) in lang_ns.iter_mut().enumerate() {
            for (li, v) in row.iter_mut().enumerate() {
                *v = 100 * (gi as u32 + 1) + 30 * (li as u32 + 1);
            }
        }
        Runner { lang_ns }
    }

    /// 全量：六组×四语种 = 24 跑。
    pub fn run_all(&self) -> Vec<BenchRun> {
        let mut out = Vec::new();
        for g in BenchGroup::ALL.iter() {
            for li in 0..BENCH_LANGS.len() {
                out.push(BenchRun { group: *g, lang_idx: li, ns: self.lang_ns[*g as usize][li] });
            }
        }
        out
    }

    /// 单组模式。
    pub fn run_group(&self, g: BenchGroup) -> Vec<BenchRun> {
        (0..BENCH_LANGS.len()).map(|li| BenchRun { group: g, lang_idx: li, ns: self.lang_ns[g as usize][li] }).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves364_checks() -> CheckSet {
    let mut set = CheckSet::new("ves364");
    let rn = Runner::new();

    // C364-01 六组名单单源（含无障碍专组）。
    chk(&mut set, "C364-01", BenchGroup::ALL.len() == 6 && BenchGroup::ALL[5].name() == "a11y-render", "六组含专组");

    // C364-02 全量 24 跑。
    let all = rn.run_all();
    chk(&mut set, "C364-02", all.len() == 24, "六组×四语种 24 跑");

    // C364-03 单组模式：仅 4 跑且组一致。
    let one = rn.run_group(BenchGroup::Layout);
    chk(&mut set, "C364-03", one.len() == 4 && one.iter().all(|r| r.group == BenchGroup::Layout), "单组 4 跑");

    // C364-04 矩阵满格。
    let mut bl = BaselineBook::new();
    bl.runs = all.clone();
    chk(&mut set, "C364-04", bl.holes(&BenchGroup::ALL).is_empty(), "矩阵无缺格");

    // C364-05 缺格补测定位。
    bl.runs.retain(|r| !(r.group == BenchGroup::Load && r.lang_idx == 2));
    bl.runs.retain(|r| !(r.group == BenchGroup::Pipeline && r.lang_idx == 0));
    chk(&mut set, "C364-05", bl.holes(&BenchGroup::ALL) == vec![
        (BenchGroup::Pipeline, "en"), (BenchGroup::Load, "ar")], "缺两格定位");

    // C364-06 哈希冻结。
    bl.runs = all.clone();
    let h1 = bl.freeze();
    chk(&mut set, "C364-06", h1 != 0 && bl.frozen_hash == h1, "基线冻结");

    // C364-07 基线漂移检出。
    bl.runs[0].ns += 1;
    chk(&mut set, "C364-07", bl.drift_check() == Err(BnCode::BaselineDrift), "一字漂移 => 立案");

    // C364-08 漂移复位后复检过。
    bl.runs[0].ns -= 1;
    chk(&mut set, "C364-08", bl.drift_check().is_ok(), "复位复检过");

    // C364-09 基准失效扫描：全零组。
    bl.runs.extend((0..4).map(|li| BenchRun { group: BenchGroup::Switch, lang_idx: li, ns: 0 }));
    chk(&mut set, "C364-09", bl.scan_dead() == vec!["switch"], "switch 组全零失效");

    // C364-10 失效修复后重冻结。
    for r in bl.runs.iter_mut() {
        if r.group == BenchGroup::Switch && r.ns == 0 { r.ns = 777; }
    }
    chk(&mut set, "C364-10", bl.scan_dead().is_empty(), "修复后无失效");

    // C364-11 无障碍专组数据在册（域本色）。
    chk(&mut set, "C364-11", all.iter().filter(|r| r.group == BenchGroup::A11yRender).count() == 4, "专组四语种全跑");

    // C364-12 哈希确定性。
    let mut bl2 = BaselineBook::new();
    bl2.runs = all;
    chk(&mut set, "C364-12", bl2.freeze() == h1, "同数据同哈希");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves364_smoke() {
        let set = run_ves364_checks();
        assert!(set.failed_count() == 0, "ves364 red: {}", set.red_items_text());
    }

    #[test]
    fn ves364_matrix() {
        let rn = Runner::new();
        let g = rn.run_group(BenchGroup::Lookup);
        assert_eq!(g.len(), BENCH_LANGS.len());
        assert_eq!(g[0].ns, 130); // 100*1 + 30*1
    }
}
