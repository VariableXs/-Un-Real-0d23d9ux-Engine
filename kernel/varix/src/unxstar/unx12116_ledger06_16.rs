//! UNX-F12116 多编码转换账：UTF-16/UTF-8/ANSI 面预告（0x1a0a0 段）
//!
//! 卡面：编码转换对账——转换对（from→to）闭集、无源转换拒、往返
//! （roundtrip）标记、损失性转换（lossy）显性标注。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 编码闭集：三种编码（UTF-16/UTF-8/ANSI）枚举闭集。
//! - J2 转换对闭集：仅登记簿内转换对可执行，未知对必拒。
//! - J3 无源拒：源串未登记必拒。
//! - J4 往返标记：可逆对标 roundtrip 真，损失对标 lossy 真且互斥。
//! - J5 记录唯一：同（源号, 转换对）二次记录必拒。
//! - J6 印账冻结：seal 后转换必拒。
//! - J7 注入五况全抓。
//!
//! 结构：Encoding 三编码枚举 + ConvPair 转换对 + ConvBook 转换账 +
//! 转换核 + 折叠指纹（211 进制卷积）+ 注入核（未知对/无源/零号/重录/
//! 印后转换）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a0a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a0a0;
pub const CE_BAD_PAIR: u32 = SEG_BASE + 0x01; // 未知转换对
pub const CE_NO_SRC: u32 = SEG_BASE + 0x02; // 无源
pub const CE_DUP_REC: u32 = SEG_BASE + 0x03; // 重录
pub const CE_ZERO_ID: u32 = SEG_BASE + 0x04; // 零号源
pub const CE_FROZEN: u32 = SEG_BASE + 0x05; // 印账后转换

/// 三编码枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf16 = 1,
    Utf8 = 2,
    Ansi = 3,
}

impl Encoding {
    pub fn from_code(code: u32) -> Result<Encoding, u32> {
        match code {
            1 => Ok(Encoding::Utf16),
            2 => Ok(Encoding::Utf8),
            3 => Ok(Encoding::Ansi),
            _ => Err(CE_BAD_PAIR),
        }
    }
}

/// 转换对（键 = from×10 + to，闭集内登记方可执行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConvPair {
    pub from: Encoding,
    pub to: Encoding,
}

impl ConvPair {
    pub fn key(&self) -> u32 {
        self.from as u32 * 10 + self.to as u32
    }
    /// 损失性判定：ANSI ← UTF-16/UTF-8 视作损失转换。
    pub fn lossy(&self) -> bool {
        self.to == Encoding::Ansi
            && matches!(self.from, Encoding::Utf16 | Encoding::Utf8)
    }
    pub fn render(&self) -> String {
        format!("C{}->{}", self.from as u16, self.to as u16)
    }
}

/// 转换记录。
#[derive(Clone)]
pub struct ConvRec {
    pub src_id: u32,
    pub pair: ConvPair,
    pub nbytes: u32,
    pub tag: String,
}

/// 转换账。
pub struct ConvBook {
    pub srcs: Vec<(u32, u32)>, // (源号, 字节数)
    pub allowed: Vec<ConvPair>,
    pub recs: Vec<ConvRec>,
    pub sealed: bool,
}

impl ConvBook {
    pub fn new() -> ConvBook {
        ConvBook {
            srcs: Vec::new(),
            allowed: Vec::new(),
            recs: Vec::new(),
            sealed: false,
        }
    }

    /// 登记源串：零号与重号必拒。
    pub fn add_src(&mut self, id: u32, nbytes: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CE_FROZEN);
        }
        if id == 0 {
            return Err(CE_ZERO_ID);
        }
        if self.srcs.iter().any(|s| s.0 == id) {
            return Err(CE_ZERO_ID);
        }
        self.srcs.push((id, nbytes));
        Ok(())
    }

    /// 开放转换对。
    pub fn allow(&mut self, pair: ConvPair) -> Result<(), u32> {
        if self.sealed {
            return Err(CE_FROZEN);
        }
        if self.allowed.contains(&pair) {
            return Err(CE_BAD_PAIR);
        }
        self.allowed.push(pair);
        Ok(())
    }

    /// 转换核：源在场 → 对开放 → 重录拒 → 入账。
    pub fn convert(&mut self, src_id: u32, pair: ConvPair, tag: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CE_FROZEN);
        }
        let nbytes = match self.srcs.iter().find(|s| s.0 == src_id) {
            Some(s) => s.1,
            None => return Err(CE_NO_SRC),
        };
        if !self.allowed.contains(&pair) {
            return Err(CE_BAD_PAIR);
        }
        if self
            .recs
            .iter()
            .any(|r| r.src_id == src_id && r.pair == pair)
        {
            return Err(CE_DUP_REC);
        }
        let out = if pair.lossy() { nbytes } else { nbytes * 2 };
        self.recs.push(ConvRec {
            src_id,
            pair,
            nbytes: out,
            tag: tag.to_string(),
        });
        Ok(out)
    }

    /// 记录数。
    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.srcs {
            h = h.wrapping_mul(211).wrapping_add(s.0);
            h = h.wrapping_mul(211).wrapping_add(s.1);
        }
        for a in &self.allowed {
            h = h.wrapping_mul(211).wrapping_add(a.key());
        }
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.src_id);
            h = h.wrapping_mul(211).wrapping_add(r.pair.key());
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("srcs={} recs={}", self.srcs.len(), self.rec_count())
    }

    /// 诊断面：记录渲染。
    pub fn line_of(&self, i: usize) -> String {
        match self.recs.get(i) {
            Some(r) => format!("E{}|{}", r.src_id, r.pair.render()),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ConvBook::new();
    let _ = b.add_src(1, 8);
    let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 });
    // 1) 未知转换对必拒。
    got[0] = b.convert(1, ConvPair { from: Encoding::Utf8, to: Encoding::Ansi }, "x")
        .err() == Some(CE_BAD_PAIR);
    // 2) 无源必拒。
    got[1] = b.convert(9, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "x")
        .err() == Some(CE_NO_SRC);
    // 3) 零号源必拒。
    got[2] = b.add_src(0, 4).err() == Some(CE_ZERO_ID);
    // 4) 重录必拒。
    let _ = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "a");
    got[3] = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "b")
        .err() == Some(CE_DUP_REC);
    // 5) 印账后转换必抓。
    b.seal();
    got[4] = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "p")
        .err() == Some(CE_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12116");

    // J1 编码闭集。
    let j1 = Encoding::from_code(0).is_err()
        && Encoding::from_code(4).is_err()
        && Encoding::from_code(3).is_ok();
    s.add("J1-编码闭集", j1, "0 与 4 拒、3 过");

    // J2 转换对键与损失性。
    let j2 = {
        let rt = ConvPair { from: Encoding::Utf8, to: Encoding::Utf16 };
        let loss = ConvPair { from: Encoding::Utf16, to: Encoding::Ansi };
        rt.key() == 21 && !rt.lossy() && loss.lossy()
    };
    s.add("J2-对键损失", j2, "可逆与损失分标");

    // J3 无源拒。
    let j3 = {
        let mut b = ConvBook::new();
        b.convert(9, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "x")
            .err() == Some(CE_NO_SRC)
    };
    s.add("J3-无源拒", j3, "源未登记必拒");

    // J4 未知对拒。
    let j4 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 8);
        b.convert(1, ConvPair { from: Encoding::Ansi, to: Encoding::Utf8 }, "x")
            .err() == Some(CE_BAD_PAIR)
    };
    s.add("J4-未知对拒", j4, "未开放对必拒");

    // J5 转换入账与重录拒。
    let j5 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 8);
        let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 });
        let out = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "a")
            .unwrap_or(0);
        out == 16 && b.rec_count() == 1
            && b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "b")
                .err() == Some(CE_DUP_REC)
    };
    s.add("J5-入账重录", j5, "UTF-16→UTF-8 展宽减半口径");

    // J6 损失转换口径。
    let j6 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 6);
        let _ = b.allow(ConvPair { from: Encoding::Utf8, to: Encoding::Ansi });
        let out = b.convert(1, ConvPair { from: Encoding::Utf8, to: Encoding::Ansi }, "l")
            .unwrap_or(0);
        out == 6 // 损失转换不展宽
    };
    s.add("J6-损失口径", j6, "ANSI 目标不改长");

    // J7 零号源拒。
    let j7 = {
        let mut b = ConvBook::new();
        b.add_src(0, 4).err() == Some(CE_ZERO_ID)
    };
    s.add("J7-零号拒", j7, "零号必拒");

    // J8 重源拒。
    let j8 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 4);
        b.add_src(1, 8).err() == Some(CE_ZERO_ID)
    };
    s.add("J8-重源拒", j8, "同源二次必拒");

    // J9 记录渲染。
    let j9 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 4);
        let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 });
        let _ = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "r");
        b.line_of(0).contains("E1|C1->2") && b.line_of(1) == "missing"
    };
    s.add("J9-记录渲染", j9, "记录行含对键");

    // J10 印账冻结。
    let j10 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 4);
        b.seal();
        b.add_src(2, 4).err() == Some(CE_FROZEN)
            && b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "p")
                .err() == Some(CE_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 4);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ConvBook::new();
    let _ = b.add_src(1, 10);
    let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 });
    let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Ansi });
    let _ = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "rt");
    let _ = b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Ansi }, "lossy");

    s.add("摘要回读", b.summary().contains("recs=2"), "双记录在账");
    s.add("双口径分账", b.recs[0].nbytes == 20 && b.recs[1].nbytes == 10, "20/10 分账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12116_conv_flow() {
        let mut b = ConvBook::new();
        let _ = b.add_src(1, 4);
        let _ = b.allow(ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 });
        assert!(b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "a").is_ok());
        assert_eq!(
            b.convert(1, ConvPair { from: Encoding::Utf16, to: Encoding::Utf8 }, "b"),
            Err(CE_DUP_REC)
        );
    }

    #[test]
    fn unx12116_checks_green() {
        let s = run_unx12116_checks();
        assert!(s.all_passed(), "UNX-F12116 判据未全绿");
    }

    #[test]
    fn encoding_codes() {
        assert_eq!(Encoding::Utf16 as u16, 1);
        assert_eq!(Encoding::Ansi as u16, 3);
        assert!(Encoding::from_code(9).is_err());
    }
}
