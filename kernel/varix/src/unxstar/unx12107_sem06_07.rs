//! UNX-F12107 RtlUnicodeStringCopy/Cat 安全族预检语义（0x19f80 段）
//!
//! 卡面：Rtl 安全字符串族（Copy/Cat）两步制——先预检出状态码、
//! 后执行；预检不过则不落任何写入；源奇长/目标无容显性拒。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 目标在场：未登记目标必拒。
//! - J2 目标无容拒：max == 0（未初始化目标）必拒。
//! - J3 源奇长拒：UTF-16 源长必偶。
//! - J4 预检两步制：预检通过后执行必成功，预检失败后执行必拒。
//! - J5 拒不落笔：预检失败时目标 len 不漂移。
//! - J6 印账冻结：seal 后操作必拒。
//! - J7 注入五况全抓。
//!
//! 结构：SafeDest 目标档 + SafeBook 目标册 + 预检核 + 执行核 +
//! 折叠指纹（211 进制卷积）+ 注入核（无目标/无容/奇长/预检失败执行/
//! 印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19f80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19f80;
pub const SC_NO_DEST: u32 = SEG_BASE + 0x01; // 无目标
pub const SC_DEST_UNINIT: u32 = SEG_BASE + 0x02; // 目标无容
pub const SC_SRC_ODD: u32 = SEG_BASE + 0x03; // 源奇长
pub const SC_TOO_SMALL: u32 = SEG_BASE + 0x04; // 容不足
pub const SC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 安全目标档。
#[derive(Clone, Copy)]
pub struct SafeDest {
    pub id: u32,
    pub len: u32,
    pub max: u32,
}

impl SafeDest {
    pub fn free(&self) -> u32 {
        if self.max >= self.len + 2 {
            self.max - self.len - 2
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("S{}|{}+{}", self.id, self.len, self.max)
    }
}

/// 预检结果：通过/状态码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precheck {
    Ok,
    Err(u32),
}

/// 目标册。
pub struct SafeBook {
    pub dests: Vec<SafeDest>,
    pub sealed: bool,
}

impl SafeBook {
    pub fn new() -> SafeBook {
        SafeBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标：max 可为 0（未初始化态，预检将拒）。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_FROZEN);
        }
        if id == 0 {
            return Err(SC_NO_DEST); // 零号视作无目标面
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(SC_NO_DEST);
        }
        self.dests.push(SafeDest { id, len: 0, max });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 预检核：目标在场 → 目标有容 → 源偶长 → 容足。
    pub fn precheck(&self, id: u32, src_len: u32) -> Precheck {
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Precheck::Err(SC_NO_DEST),
        };
        if self.dests[i].max == 0 {
            return Precheck::Err(SC_DEST_UNINIT);
        }
        if src_len % 2 != 0 {
            return Precheck::Err(SC_SRC_ODD);
        }
        if self.dests[i].len + src_len + 2 > self.dests[i].max {
            return Precheck::Err(SC_TOO_SMALL);
        }
        Precheck::Ok
    }

    /// 执行核：仅在预检通过后落笔。
    pub fn execute(&mut self, id: u32, src_len: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_FROZEN);
        }
        match self.precheck(id, src_len) {
            Precheck::Err(e) => Err(e),
            Precheck::Ok => {
                if let Some(i) = self.pos(id) {
                    self.dests[i].len += src_len;
                }
                Ok(())
            }
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.dests {
            h = h.wrapping_mul(211).wrapping_add(d.id);
            h = h.wrapping_mul(211).wrapping_add(d.len);
            h = h.wrapping_mul(211).wrapping_add(d.max);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("dests={}", self.dests.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.dests.iter().find(|d| d.id == id) {
            Some(d) => d.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SafeBook::new();
    let _ = b.add_dest(4, 16);
    // 1) 无目标必拒。
    got[0] = b.execute(9, 4).err() == Some(SC_NO_DEST);
    // 2) 目标无容必拒。
    let _ = b.add_dest(5, 0);
    got[1] = b.execute(5, 4).err() == Some(SC_DEST_UNINIT);
    // 3) 源奇长必拒。
    got[2] = b.execute(4, 5).err() == Some(SC_SRC_ODD);
    // 4) 预检失败执行必拒且不落笔（源须偶长，否则先撞奇长守卫）。
    let before = b.dests[0].len;
    got[3] = b.execute(4, 100).err() == Some(SC_TOO_SMALL)
        && b.dests[0].len == before;
    // 5) 印账后操作必抓。
    b.seal();
    got[4] = b.execute(4, 2).err() == Some(SC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12107_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12107");

    // J1 无目标拒。
    let j1 = {
        let mut b = SafeBook::new();
        b.precheck(1, 4) == Precheck::Err(SC_NO_DEST)
    };
    s.add("J1-无目标拒", j1, "预检即拒");

    // J2 目标无容拒。
    let j2 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 0);
        b.precheck(1, 4) == Precheck::Err(SC_DEST_UNINIT)
    };
    s.add("J2-无容拒", j2, "未初始化目标必拒");

    // J3 源奇长拒。
    let j3 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 32);
        b.precheck(1, 5) == Precheck::Err(SC_SRC_ODD)
    };
    s.add("J3-奇长拒", j3, "UTF-16 源长必偶");

    // J4 容不足拒。
    let j4 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 8);
        b.precheck(1, 8) == Precheck::Err(SC_TOO_SMALL)
    };
    s.add("J4-容不足拒", j4, "len+src+2 越容必拒");

    // J5 预检通过执行必成。
    let j5 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 32);
        let _ = b.execute(1, 8);
        b.dests[0].len == 8
    };
    s.add("J5-两步制", j5, "预检过则执行成");

    // J6 拒不落笔。
    let j6 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 8);
        let _ = b.execute(1, 2);
        let before = b.dests[0].len;
        let _ = b.execute(1, 8);
        b.dests[0].len == before
    };
    s.add("J6-拒不落笔", j6, "失败时 len 不漂移");

    // J7 零号拒。
    let j7 = {
        let mut b = SafeBook::new();
        b.add_dest(0, 16).err() == Some(SC_NO_DEST)
    };
    s.add("J7-零号拒", j7, "零号必拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(6, 24);
        b.line_of(6).contains("S6|0+24") && b.line_of(7) == "missing"
    };
    s.add("J8-回读渲染", j8, "目标行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.execute(1, 2).err() == Some(SC_FROZEN)
            && b.add_dest(2, 16).err() == Some(SC_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = SafeBook::new();
    let _ = b.add_dest(1, 32);
    let _ = b.execute(1, 10);
    let _ = b.add_dest(2, 8);
    let _ = b.execute(2, 4);

    s.add("摘要回读", b.summary().contains("dests=2"), "双目标在册");
    s.add("双档长度", b.dests[0].len == 10 && b.dests[1].len == 4, "10/4 分账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12107_precheck_flow() {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 16);
        assert_eq!(b.precheck(1, 4), Precheck::Ok);
        assert_eq!(b.precheck(9, 4), Precheck::Err(SC_NO_DEST));
        assert_eq!(b.precheck(1, 5), Precheck::Err(SC_SRC_ODD));
    }

    #[test]
    fn unx12107_checks_green() {
        let s = run_unx12107_checks();
        assert!(s.all_passed(), "UNX-F12107 判据未全绿");
    }

    #[test]
    fn two_step_discipline() {
        let mut b = SafeBook::new();
        let _ = b.add_dest(1, 8);
        assert_eq!(b.execute(1, 8), Err(SC_TOO_SMALL));
        assert!(b.execute(1, 2).is_ok());
    }
}
