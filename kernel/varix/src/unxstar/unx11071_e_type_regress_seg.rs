//! UNX-F11071 E 型八批回归段（0x1e8e0 段）
//!
//! B21–B28 错误矩阵在此重放：每格登记 (批次, 系统调用号, 期望 errno,
//! 实测 errno, 三态)，三态为绿/红/显式 skip。逐批配额必须与登记数
//! 相等，任何红格使该批判据短路为失配；skip 只允许覆盖红格（把已通过
//! 的格子标 skip 属于掩盖，必须拒绝）。
//!
//! 双判据面：期望面与实测面各折一次指纹，只有全部非 skip 格逐位相等
//! 时两面才一致。不变式：green + red + skip == 格数；冻结后写路径全拒。
//!
//! 结构：ErrnoCell（矩阵格）/ ErrMatrix（八批矩阵 + 双面重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11071 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e8e0;

/// 空账：批次无格、系统调用号为 0 或查无此格
pub const ET_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：批次不在 B21..=B28
pub const ET_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：同批同调用号二次登记或重复改写
pub const ET_DUP: u32 = SEG_BASE + 0x03;
/// 失配：红格未清、或对绿格标 skip
pub const ET_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：某批登记数与配额不符、总席不足
pub const ET_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：矩阵冻结后的登记/改写/skip
pub const ET_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x71 标记 11071 号卡
pub const ET_TAG: u64 = 0x1e80_71;

/// 八批编号下界（B21）
const BATCH_LO: u8 = 21;
/// 八批编号上界（B28）
const BATCH_HI: u8 = 28;
/// 本段固定十席
const SEATS: usize = 10;
/// 三态：绿
const TONE_GREEN: u8 = 0;
/// 三态：红
const TONE_RED: u8 = 1;
/// 三态：显式 skip
const TONE_SKIP: u8 = 2;

fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xff) as u8), ((v >> 8) & 0xff) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> k.wrapping_mul(8)) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

/// 错误矩阵单格
#[derive(Clone, Debug)]
pub struct ErrnoCell {
    /// 批次 B21..=B28
    pub batch: u8,
    /// 系统调用号
    pub syscall: u16,
    /// 判据期望的 errno
    pub expected: u32,
    /// 实测 errno
    pub actual: u32,
    /// 三态码（0 绿 / 1 红 / 2 skip）
    pub tone: u8,
}

impl ErrnoCell {
    /// 按期望/实测自动定态
    pub fn new(batch: u8, syscall: u16, expected: u32, actual: u32) -> ErrnoCell {
        let tone = if expected == actual { TONE_GREEN } else { TONE_RED };
        ErrnoCell { batch, syscall, expected, actual, tone }
    }

    /// 是否绿
    pub fn green(&self) -> bool {
        self.tone == TONE_GREEN
    }

    /// 是否显式跳过
    pub fn skipped(&self) -> bool {
        self.tone == TONE_SKIP
    }

    /// 三态名（可读面）
    pub fn tone_name(&self) -> &'static str {
        match self.tone {
            TONE_GREEN => "green",
            TONE_RED => "red",
            _ => "skip",
        }
    }

    /// 单格指纹
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_byte(h, self.batch);
        h = fnv_u16(h, self.syscall);
        h = fnv_u32(h, self.expected);
        h = fnv_u32(h, self.actual);
        fnv_byte(h, self.tone)
    }
}

/// B21–B28 错误矩阵
#[derive(Clone, Debug)]
pub struct ErrMatrix {
    /// 全部格
    pub cells: Vec<ErrnoCell>,
    /// 八批配额（下标 batch-21）
    pub quota: [u8; 8],
    /// 最近一次重放绿格数
    pub verified: u16,
    frozen: Option<u64>,
}

impl ErrMatrix {
    /// 以八批配额建空矩阵
    pub fn new(quota: [u8; 8]) -> ErrMatrix {
        ErrMatrix { cells: Vec::new(), quota, verified: 0, frozen: None }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(ET_FROZEN);
        }
        Ok(())
    }

    fn batch_index(batch: u8) -> Option<usize> {
        if batch >= BATCH_LO && batch <= BATCH_HI {
            Some((batch - BATCH_LO) as usize)
        } else {
            None
        }
    }

    fn find(&self, batch: u8, syscall: u16) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cells.len() {
            let c = &self.cells[k];
            if c.batch == batch && c.syscall == syscall {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记一格：批次/调用号/重号三道闸
    pub fn enroll(&mut self, cell: ErrnoCell) -> Result<(), u32> {
        self.guard()?;
        if ErrMatrix::batch_index(cell.batch).is_none() {
            return Err(ET_RANGE);
        }
        if cell.syscall == 0 {
            return Err(ET_EMPTY);
        }
        if self.find(cell.batch, cell.syscall).is_some() {
            return Err(ET_DUP);
        }
        self.cells.push(cell);
        Ok(())
    }

    /// 改写实测 errno（只对非 skip 格）
    pub fn revise(&mut self, batch: u8, syscall: u16, actual: u32) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.find(batch, syscall) {
            Some(i) => i,
            None => return Err(ET_EMPTY),
        };
        if self.cells[idx].skipped() {
            return Err(ET_DUP);
        }
        self.cells[idx].actual = actual;
        self.cells[idx].tone = if self.cells[idx].expected == actual {
            TONE_GREEN
        } else {
            TONE_RED
        };
        Ok(())
    }

    /// 对红格标 skip（对绿格标 skip 视为掩盖，拒）
    pub fn mark_skip(&mut self, batch: u8, syscall: u16) -> Result<(), u32> {
        self.guard()?;
        let idx = match self.find(batch, syscall) {
            Some(i) => i,
            None => return Err(ET_EMPTY),
        };
        if self.cells[idx].tone == TONE_GREEN {
            return Err(ET_MISMATCH);
        }
        if self.cells[idx].tone == TONE_SKIP {
            return Err(ET_DUP);
        }
        self.cells[idx].tone = TONE_SKIP;
        Ok(())
    }

    /// 单批统计：(green, red, skip)
    pub fn batch_stats(&self, batch: u8) -> (u16, u16, u16) {
        let mut g = 0u16;
        let mut r = 0u16;
        let mut s = 0u16;
        let mut k = 0usize;
        while k < self.cells.len() {
            let c = &self.cells[k];
            if c.batch == batch {
                match c.tone {
                    TONE_GREEN => g = g.wrapping_add(1),
                    TONE_RED => r = r.wrapping_add(1),
                    _ => s = s.wrapping_add(1),
                }
            }
            k += 1;
        }
        (g, r, s)
    }

    /// 单批判据：配额不符 ET_SHORT，存在红格 ET_MISMATCH
    pub fn judge(&self, batch: u8) -> Result<u8, u32> {
        let bi = match ErrMatrix::batch_index(batch) {
            Some(i) => i,
            None => return Err(ET_RANGE),
        };
        let (green, red, skip) = self.batch_stats(batch);
        let total = green.wrapping_add(red).wrapping_add(skip);
        if total != self.quota[bi] as u16 {
            return Err(ET_SHORT);
        }
        if red > 0 {
            return Err(ET_MISMATCH);
        }
        Ok(green as u8)
    }

    /// 期望面指纹（跳过 skip 格）
    pub fn expected_plane(&self) -> u64 {
        let mut h = ET_TAG;
        let mut k = 0usize;
        while k < self.cells.len() {
            let c = &self.cells[k];
            if !c.skipped() {
                h = fnv_byte(h, c.batch);
                h = fnv_u16(h, c.syscall);
                h = fnv_u32(h, c.expected);
            }
            k += 1;
        }
        h
    }

    /// 实测面指纹（跳过 skip 格）
    pub fn actual_plane(&self) -> u64 {
        let mut h = ET_TAG;
        let mut k = 0usize;
        while k < self.cells.len() {
            let c = &self.cells[k];
            if !c.skipped() {
                h = fnv_byte(h, c.batch);
                h = fnv_u16(h, c.syscall);
                h = fnv_u32(h, c.actual);
            }
            k += 1;
        }
        h
    }

    /// 双判据面一致
    pub fn dual_plane(&self) -> bool {
        self.expected_plane() == self.actual_plane()
    }

    /// 三态守恒：绿 + 红 + 跳过 == 格数
    pub fn tone_conserved(&self) -> bool {
        let mut g = 0u32;
        let mut r = 0u32;
        let mut s = 0u32;
        let mut k = 0usize;
        while k < self.cells.len() {
            match self.cells[k].tone {
                TONE_GREEN => g = g.wrapping_add(1),
                TONE_RED => r = r.wrapping_add(1),
                _ => s = s.wrapping_add(1),
            }
            k += 1;
        }
        g.wrapping_add(r).wrapping_add(s) == self.cells.len() as u32
    }

    /// 全量重放：八批逐批判据 + 双面指纹
    pub fn replay(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.cells.len() < SEATS {
            return Err(ET_SHORT);
        }
        let mut green = 0u16;
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            let g = self.judge(b)?;
            green = green.wrapping_add(g as u16);
            b = b.wrapping_add(1);
        }
        self.verified = green;
        if green as usize != SEATS || !self.dual_plane() {
            return Err(ET_MISMATCH);
        }
        Ok(green)
    }

    /// 矩阵指纹
    pub fn fold(&self) -> u64 {
        let mut h = ET_TAG;
        let mut k = 0usize;
        while k < self.quota.len() {
            h = fnv_byte(h, self.quota[k]);
            k += 1;
        }
        h = fnv_u16(h, self.verified);
        k = 0;
        while k < self.cells.len() {
            h = self.cells[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结矩阵
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：越批 / 零调用号 / 重格 / 绿格 skip
pub fn inject_four() -> [bool; 4] {
    let mut m0 = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
    let range = matches!(m0.enroll(ErrnoCell::new(29, 1, 0, 0)), Err(ET_RANGE));
    let mut m1 = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
    let empty = matches!(m1.enroll(ErrnoCell::new(21, 0, 0, 0)), Err(ET_EMPTY));
    let mut m2 = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
    let first = m2.enroll(ErrnoCell::new(21, 1, 0, 0)).is_ok();
    let dup = first && matches!(m2.enroll(ErrnoCell::new(21, 1, 0, 0)), Err(ET_DUP));
    let mut m3 = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
    let seeded = m3.enroll(ErrnoCell::new(21, 1, 0, 0)).is_ok();
    let skip_guard = seeded && matches!(m3.mark_skip(21, 1), Err(ET_MISMATCH));
    [range, empty, dup, skip_guard]
}

/// 构造八批样本（配额合计 10，全部绿）
fn sample_matrix() -> Result<ErrMatrix, u32> {
    let quota = [2u8, 2, 1, 1, 1, 1, 1, 1];
    let mut m = ErrMatrix::new(quota);
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let n = quota[(b - BATCH_LO) as usize];
        let mut j = 0u8;
        while j < n {
            let syscall = (b as u16).wrapping_mul(10).wrapping_add(j as u16);
            let errno = 22u32.wrapping_add(j as u32);
            m.enroll(ErrnoCell::new(b, syscall, errno, errno))?;
            j = j.wrapping_add(1);
        }
        b = b.wrapping_add(1);
    }
    Ok(m)
}

/// UNX-F11071 判据入口
pub fn run_unx11071_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11071");
    let mut m = match sample_matrix() {
        Ok(v) => v,
        Err(_) => ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]),
    };
    let green = m.replay().unwrap_or(0);
    s.add(
        "J1a-E型判据全量重放",
        green == 10 && m.cells.len() == SEATS,
        "B21–B28 十格必须逐批过判据",
    );
    s.add(
        "J1b-双判据面一致",
        m.dual_plane() && m.expected_plane() == m.actual_plane(),
        "期望面与实测面指纹必须逐位相等",
    );
    let mut quota_sum = 0u16;
    let mut k = 0usize;
    while k < m.quota.len() {
        quota_sum = quota_sum.wrapping_add(m.quota[k] as u16);
        k += 1;
    }
    s.add(
        "J2-三态守恒",
        m.tone_conserved() && quota_sum == 10 && m.cells.len() == quota_sum as usize,
        "绿+红+skip 等于格数，配额合计等于十席",
    );
    let inj = inject_four();
    s.add("J3a-越批拒止", inj[0], "B29 登记必须 ET_RANGE");
    s.add("J3b-零调用号拒", inj[1], "syscall=0 必须 ET_EMPTY");
    s.add("J3c-重格拒止", inj[2], "同批同号二次登记必须 ET_DUP");
    s.add("J3d-绿格skip拒", inj[3], "对绿格标 skip 必须 ET_MISMATCH");
    let mut fz = match sample_matrix() {
        Ok(v) => v,
        Err(_) => ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]),
    };
    let fp = fz.freeze();
    let frozen_ok = matches!(fz.revise(21, 21, 99), Err(ET_FROZEN))
        && matches!(fz.enroll(ErrnoCell::new(21, 99, 0, 0)), Err(ET_FROZEN))
        && matches!(fz.mark_skip(21, 21), Err(ET_FROZEN));
    s.add(
        "J4-冻结不可写",
        frozen_ok && fz.fold() == fp,
        "冻结后改写/登记/skip 全拒且指纹不变",
    );
    let fp_a = m.clone().fold();
    let mut sens = match sample_matrix() {
        Ok(v) => v,
        Err(_) => ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]),
    };
    let sens_ok = sens.revise(21, 210, 99).is_ok()
        && sens.fold() != fp_a
        && !sens.dual_plane();
    s.add(
        "J5-指纹敏感性",
        sens_ok,
        "单格实测改为 99 必须改变 fold 且双面失衡",
    );
    let short_judge = {
        let mut g = ErrMatrix::new([2, 1, 1, 1, 1, 1, 1, 1]);
        g.enroll(ErrnoCell::new(21, 1, 0, 0)).is_ok() && matches!(g.judge(21), Err(ET_SHORT))
    };
    let red_judge = {
        let mut g = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
        g.enroll(ErrnoCell::new(22, 2, 22, 13)).is_ok() && matches!(g.judge(22), Err(ET_MISMATCH))
    };
    let empty_judge = matches!(ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]).judge(30), Err(ET_RANGE));
    s.add(
        "J6-范围与配额拒止",
        short_judge && red_judge && empty_judge,
        "配额不符/红格/越批裁判三路都必须拒",
    );
    let mut unique = true;
    let mut i = 0usize;
    while i < m.cells.len() {
        let mut j = i.wrapping_add(1);
        while j < m.cells.len() {
            if m.cells[i].batch == m.cells[j].batch && m.cells[i].syscall == m.cells[j].syscall {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let dup_reject = {
        let mut g = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
        g.enroll(ErrnoCell::new(21, 7, 0, 0)).is_ok()
            && matches!(g.enroll(ErrnoCell::new(21, 7, 0, 0)), Err(ET_DUP))
    };
    s.add(
        "J7-唯一与序不变式",
        unique && dup_reject,
        "(batch, syscall) 组合全局唯一且重格当场拒",
    );
    let mut partial = match sample_matrix() {
        Ok(v) => v,
        Err(_) => ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]),
    };
    while partial.cells.len() > 5 {
        partial.cells.pop();
    }
    let negative = partial.cells.len() == 5 && matches!(partial.replay(), Err(ET_SHORT));
    s.add(
        "J8-半成品不得全绿",
        negative,
        "五格半账必须 ET_SHORT，不得伪装成绿",
    );
    let skip_flow = {
        let mut g = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
        match g.enroll(ErrnoCell::new(25, 25, 22, 13)) {
            Ok(()) => g.mark_skip(25, 25).is_ok() && matches!(g.judge(25), Ok(0)),
            Err(_) => false,
        }
    };
    let revise_back = {
        let mut g = ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]);
        match g.enroll(ErrnoCell::new(26, 26, 22, 13)) {
            Ok(()) => g.revise(26, 26, 22).is_ok()
                && g.cells[0].green()
                && matches!(g.judge(26), Ok(1)),
            Err(_) => false,
        }
    };
    s.add(
        "J10-skip与复位纪律",
        skip_flow && revise_back,
        "红格可显式 skip，改回期望值必须复绿",
    );
    let (g21, r21, s21) = m.batch_stats(21);
    s.add_dyn(
        "J9-动态账",
        green == 10 && m.verified == 10,
        &format!(
            "green={} 格数={} B21={}/{}/{}",
            green,
            m.cells.len(),
            g21,
            r21,
            s21
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        m.fold() != 0 && m.expected_plane() == m.actual_plane(),
        &format!("fold={:016x} tag={:x}", m.fold(), ET_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11071_flow() {
        let mut m = sample_matrix().unwrap_or(ErrMatrix::new([1, 1, 1, 1, 1, 1, 1, 1]));
        assert_eq!(m.replay().unwrap_or(0), 10);
        assert!(m.dual_plane());
        assert!(m.tone_conserved());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11071_checks_green() {
        assert!(run_unx11071_checks().all_passed(), "UNX-F11071 判据未全绿");
    }
}
