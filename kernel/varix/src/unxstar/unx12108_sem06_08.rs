//! UNX-F12108 RtlAnsiStringToUnicodeString 转换语义（0x19fa0 段）
//!
//! 卡面：ANSI→UTF-16 转换——ASCII 快通道逐字节展宽（×2）、需求容量
//! = (n+1)×2、源内嵌 NUL 拒、目标容不足拒；ASCII 之外字节（≥0x80）
//! 交多编码账（预告），本卡以高字节拒显性分界。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 展宽口径：输出长 = n×2，需求容量 = (n+1)×2。
//! - J2 内嵌 NUL 拒：源中 0 字节非末位必拒。
//! - J3 高字节分界：源含 ≥0x80 字节时拒收（交多编码账口径）。
//! - J4 目标在场：未登记目标必拒。
//! - J5 容足放行：需求容量 ≤ 目标 max 时转换成功。
//! - J6 印账冻结：seal 后转换必拒。
//! - J7 注入五况全抓。
//!
//! 结构：CvDest 转换目标档 + CvBook 目标册 + 转换核 + 折叠指纹
//! （211 进制卷积）+ 注入核（内嵌 NUL/高字节/无目标/容不足/印后转换）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19fa0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19fa0;
pub const CV_EMBED_NUL: u32 = SEG_BASE + 0x01; // 内嵌 NUL
pub const CV_HIGH_BYTE: u32 = SEG_BASE + 0x02; // 高字节
pub const CV_NO_DEST: u32 = SEG_BASE + 0x03; // 无目标
pub const CV_NO_ROOM: u32 = SEG_BASE + 0x04; // 容不足
pub const CV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后转换

/// 转换目标档。
#[derive(Clone, Copy)]
pub struct CvDest {
    pub id: u32,
    pub len: u32,
    pub max: u32,
}

impl CvDest {
    pub fn render(&self) -> String {
        format!("V{}|{}+{}", self.id, self.len, self.max)
    }
}

/// 需求容量。
pub fn need_capacity(nbytes: u32) -> u32 {
    (nbytes + 1) * 2
}

/// 目标册。
pub struct CvBook {
    pub dests: Vec<CvDest>,
    pub sealed: bool,
}

impl CvBook {
    pub fn new() -> CvBook {
        CvBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        if id == 0 {
            return Err(CV_NO_DEST);
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(CV_NO_DEST);
        }
        self.dests.push(CvDest { id, len: 0, max });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 转换核：源校验 → 目标在场 → 容足 → 展宽入账。
    pub fn convert(&mut self, id: u32, src: &[u8]) -> Result<u32, u32> {
        if self.sealed {
            return Err(CV_FROZEN);
        }
        // 源校验：内嵌 NUL 与高字节。
        if src.contains(&0) {
            return Err(CV_EMBED_NUL);
        }
        if src.iter().any(|b| *b >= 0x80) {
            return Err(CV_HIGH_BYTE);
        }
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(CV_NO_DEST),
        };
        let need = need_capacity(src.len() as u32);
        if need > self.dests[i].max {
            return Err(CV_NO_ROOM);
        }
        self.dests[i].len = src.len() as u32 * 2;
        Ok(need)
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
    let mut b = CvBook::new();
    let _ = b.add_dest(4, 32);
    // 1) 内嵌 NUL 必拒。
    got[0] = b.convert(4, &[0x41, 0, 0x42]).err() == Some(CV_EMBED_NUL);
    // 2) 高字节必拒。
    got[1] = b.convert(4, &[0x41, 0xC3, 0xA9]).err() == Some(CV_HIGH_BYTE);
    // 3) 无目标必拒。
    got[2] = b.convert(9, &[0x41, 0x42]).err() == Some(CV_NO_DEST);
    // 4) 容不足必拒。
    let _ = b.add_dest(5, 4);
    got[3] = b.convert(5, &[0x41, 0x42, 0x43]).err() == Some(CV_NO_ROOM);
    // 5) 印账后转换必抓。
    b.seal();
    got[4] = b.convert(4, &[0x41]).err() == Some(CV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12108");

    // J1 展宽口径。
    let j1 = need_capacity(3) == 8 && need_capacity(0) == 2;
    s.add("J1-展宽口径", j1, "需求容=(n+1)×2");

    // J2 内嵌 NUL 拒。
    let j2 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 32);
        b.convert(1, &[0x41, 0, 0x42]).err() == Some(CV_EMBED_NUL)
    };
    s.add("J2-内嵌NUL拒", j2, "中途零必拒");

    // J3 高字节分界。
    let j3 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 32);
        b.convert(1, &[0x80]).err() == Some(CV_HIGH_BYTE)
            && b.convert(1, &[0x7F]).is_ok()
    };
    s.add("J3-高字节分界", j3, "0x7F 过、0x80 拒");

    // J4 无目标拒。
    let j4 = {
        let mut b = CvBook::new();
        b.convert(1, &[0x41]).err() == Some(CV_NO_DEST)
    };
    s.add("J4-无目标拒", j4, "未登记必拒");

    // J5 容足放行且长度入账。
    let j5 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 16);
        let need = b.convert(1, &[0x41, 0x42, 0x43]).unwrap_or(0);
        need == 8 && b.dests[0].len == 6
    };
    s.add("J5-容足放行", j5, "need=8、len=6");

    // J6 容不足拒。
    let j6 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 6);
        b.convert(1, &[0x41, 0x42, 0x43]).err() == Some(CV_NO_ROOM)
    };
    s.add("J6-容不足拒", j6, "need 8 > max 6 必拒");

    // J7 空源放行。
    let j7 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 16);
        b.convert(1, &[]).is_ok() && b.dests[0].len == 0
    };
    s.add("J7-空源放行", j7, "空串转换合法");

    // J8 零号目标拒。
    let j8 = {
        let mut b = CvBook::new();
        b.add_dest(0, 16).err() == Some(CV_NO_DEST)
    };
    s.add("J8-零号拒", j8, "零号必拒");

    // J9 回读渲染。
    let j9 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(3, 32);
        b.line_of(3).contains("V3|0+32") && b.line_of(4) == "missing"
    };
    s.add("J9-回读渲染", j9, "目标行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.convert(1, &[0x41]).err() == Some(CV_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CvBook::new();
    let _ = b.add_dest(1, 64);
    let _ = b.convert(1, &[0x44, 0x65, 0x76, 0x69, 0x63, 0x65, 0x73]);

    s.add("摘要回读", b.summary().contains("dests=1"), "单目标在册");
    s.add("长度回读", b.dests[0].len == 14, "7 字节展宽 14");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12108_convert_flow() {
        let mut b = CvBook::new();
        let _ = b.add_dest(1, 16);
        assert!(b.convert(1, &[0x41, 0x42]).is_ok());
        assert_eq!(b.dests[0].len, 4);
        assert_eq!(b.convert(1, &[0x41, 0, 0x42]), Err(CV_EMBED_NUL));
    }

    #[test]
    fn unx12108_checks_green() {
        let s = run_unx12108_checks();
        assert!(s.all_passed(), "UNX-F12108 判据未全绿");
    }

    #[test]
    fn need_capacity_helper() {
        assert_eq!(need_capacity(1), 4);
        assert_eq!(need_capacity(0x7FFE), 0xFFFC);
    }
}
