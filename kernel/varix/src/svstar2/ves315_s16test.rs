//! ves315_s16test — VE-F4075 · T04 翻译测试
//! 三层测试（单测：抽取/schema/指纹 + 集成：四段流水线端到端 + 回归：TM 命中率不回退断言）+ 合成语料生成器（多语种×多形态）+ 占位符完整性断言。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8EC0;

pub const HIT_RATE_FLOOR: u32 = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TtCode {
    Ok = 0,
    HitRateRegress = SEG_BASE + 0x01,
    GeneratorFail = SEG_BASE + 0x02,
    LayerMismatch = SEG_BASE + 0x03,
    PlaceholderBroken = SEG_BASE + 0x04,
    NotSynthetic = SEG_BASE + 0x05,
}

/// 合成语料生成器：多语种×多形态。
pub struct CorpusGen {
    pub langs: Vec<&'static str>,
    pub forms: u8,
    pub synthetic: bool,
}

impl CorpusGen {
    pub fn new() -> Self {
        CorpusGen { langs: vec!["zh", "en", "ja", "de"], forms: 3, synthetic: false }
    }
    /// 生成 N 条合成键。
    pub fn generate(&self, n: u32) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..n {
            let mut s = String::from("synthetic.");
            let li = (i as usize) % self.langs.len();
            s.push_str(self.langs[li]);
            s.push('.');
            push_num(&mut s, i);
            s.push('.');
            push_num(&mut s, (i % self.forms as u32) as u32);
            out.push(s);
        }
        out
    }
    /// 合成红线：非合成语料拒。
    pub fn guard(&self) -> Result<(), TtCode> {
        if self.synthetic { Ok(()) } else { Err(TtCode::NotSynthetic) }
    }
}

fn push_num(s: &mut String, mut n: u32) {
    if n == 0 { s.push('0'); return; }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

/// 单测三件：键命名/schema 版/指纹稳定。
pub fn unit_key_ok(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.')
}

pub fn unit_schema_ok(declared: u32, frozen: u32) -> bool {
    declared == frozen
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn unit_fp_stable(s: &str) -> bool {
    fnv1a(s.as_bytes()) == fnv1a(s.as_bytes())
}

/// 集成端到端：四段依次跑通。
pub fn e2e_pipeline(stages: &[bool]) -> Result<(), TtCode> {
    const NEED: usize = 4;
    if stages.len() != NEED { return Err(TtCode::LayerMismatch); }
    for (i, ok) in stages.iter().enumerate() {
        if !ok {
            return Err(TtCode::LayerMismatch);
        }
        let _ = i;
    }
    Ok(())
}

/// 回归：TM 命中率不回退。
pub struct HitRateWatch {
    pub last: u32,
}

impl HitRateWatch {
    pub fn new() -> Self { HitRateWatch { last: 0 } }
    pub fn check(&mut self, current: u32) -> Result<(), TtCode> {
        if self.last > 0 && current < self.last {
            return Err(TtCode::HitRateRegress);
        }
        self.last = current;
        Ok(())
    }
    pub fn below_floor(&self) -> bool {
        self.last < HIT_RATE_FLOOR
    }
}

/// 占位符完整性（测试断言用）。
pub fn placeholder_intact(src: &str, tgt: &str) -> Result<(), TtCode> {
    let a: Vec<&str> = src.split('{').skip(1).filter_map(|x| x.split('}').next()).collect();
    let b: Vec<&str> = tgt.split('{').skip(1).filter_map(|x| x.split('}').next()).collect();
    if a != b { return Err(TtCode::PlaceholderBroken); }
    Ok(())
}

pub fn run_ves315_checks() -> CheckSet {
    let mut set = CheckSet::new("ves315_s16test");

    // 1) 单测三件。
    set.add("C4075-01", unit_key_ok("app.title.ok") && !unit_key_ok("BAD KEY"), "键命名单测");
    set.add("C4075-02", unit_schema_ok(3, 3) && !unit_schema_ok(3, 2), "schema 单测");
    set.add("C4075-03", unit_fp_stable("保存"), "指纹单测");

    // 4) 合成语料生成。
    let mut gen = CorpusGen::new();
    let corpus = gen.generate(8);
    set.add("C4075-04", corpus.len() == 8 && corpus[0].starts_with("synthetic.zh."), "语料生成");

    // 5) 多语种形态覆盖。
    set.add("C4075-05", gen.langs.len() == 4 && gen.forms == 3, "多语种形态");

    // 6) 合成红线。
    set.add("C4075-06", gen.guard() == Err(TtCode::NotSynthetic), "非合成拒");
    gen.synthetic = true;
    set.add("C4075-07", gen.guard().is_ok(), "合成声明");

    // 8) 端到端。
    set.add("C4075-08", e2e_pipeline(&[true, true, true, true]).is_ok(), "端到端贯通");
    set.add("C4075-09", e2e_pipeline(&[true, false, true, true]) == Err(TtCode::LayerMismatch), "断段检出");

    // 10) 命中率回归。
    let mut hw = HitRateWatch::new();
    let _ = hw.check(70);
    set.add("C4075-10", hw.check(65).is_ok(), "命中率升不拒");
    set.add("C4075-11", hw.check(50) == Err(TtCode::HitRateRegress), "命中率回退阻断");

    // 12) 低于下限。
    let mut hw2 = HitRateWatch::new();
    let _ = hw2.check(30);
    set.add("C4075-12", hw2.below_floor() && HIT_RATE_FLOOR == 60, "下限判定");

    // 13) 占位符完整。
    set.add("C4075-13", placeholder_intact("{0} 项", "{0} 项").is_ok()
        && placeholder_intact("{0} 项", "{1} 项") == Err(TtCode::PlaceholderBroken), "占位符断言");

    // 14) 段位自查。
    set.add("C4075-14", SEG_BASE == 0x8EC0, "段位 0x8EC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layers() {
        assert!(unit_key_ok("a.b"));
        let mut hw = HitRateWatch::new();
        let _ = hw.check(80);
        assert!(hw.check(80).is_ok());
        let mut gen = CorpusGen::new();
        assert_eq!(gen.generate(3).len(), 3);
    }
}
