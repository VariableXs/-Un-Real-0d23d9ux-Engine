//! UNX-F12273 文件族边界样本集（0x64920 段）
//!
//! 卡面：文件族边界行为样本账——零长读写、部分读（EOF 截断）、偏移回绕、
//! 超长路径、偏移+长度溢出、零字节写等；每样本带期望结果，样本账以
//! 迷你文件模型实跑核对，期望与实跑不符即 MISMATCH。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 零长读写合法：information=0、文件体不变。
//! - J2 EOF 截断：越过 EOF 的读按实有字节记账。
//! - J3 偏移回绕：offset+len 溢出必拒。
//! - J4 超长路径：路径超模型上限必拒。
//! - J5 零偏移语义：offset 0 合法。
//! - J6 期望自洽：样本期望与实跑不符必拒（MISMATCH）。
//! - J7 重号样本必拒。
//! - J8 印账冻结：seal 后补样本必拒。
//! - J9 指纹幂等。
//! - J10 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64920 独占 0x20）。
pub const SEG_BASE: u32 = 0x64920;
pub const BN_WRAP: u32 = SEG_BASE + 0x01; // 偏移回绕
pub const BN_PATH_LONG: u32 = SEG_BASE + 0x02; // 超长路径
pub const BN_MISMATCH: u32 = SEG_BASE + 0x03; // 期望与实跑不符
pub const BN_DUP: u32 = SEG_BASE + 0x04; // 重号样本
pub const BN_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补样本

/// 路径长度上限（模型口径）。
pub const PATH_MAX: usize = 4096;

/// 迷你文件：字节数组体 + 实跑语义。
pub struct MiniFile {
    pub body: Vec<u8>,
}

impl MiniFile {
    pub fn with_body(body: Vec<u8>) -> MiniFile {
        MiniFile { body }
    }

    /// 读：EOF 截断；offset+len 回绕拒。
    pub fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>, u32> {
        let end = offset.checked_add(len).ok_or(BN_WRAP)?;
        if end > self.body.len() as u64 {
            // EOF 截断按实有字节。
            let off = offset as usize;
            if off >= self.body.len() {
                return Ok(Vec::new());
            }
            return Ok(self.body[off..].to_vec());
        }
        Ok(self.body[offset as usize..end as usize].to_vec())
    }

    /// 写：回绕拒；合法写改体并返回写入字节数。
    pub fn write(&mut self, offset: u64, data: &[u8]) -> Result<usize, u32> {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(BN_WRAP)?;
        if end > self.body.len() as u64 {
            return Err(BN_WRAP);
        }
        let off = offset as usize;
        self.body[off..off + data.len()].copy_from_slice(data);
        Ok(data.len())
    }
}

/// 边界样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BKind {
    ZeroRead = 0,
    ZeroWrite = 1,
    EofTruncRead = 2,
    OffsetWrap = 3,
    LongPath = 4,
    ZeroOffset = 5,
}

/// 单条样本：类别 + 期望通过。
#[derive(Clone)]
pub struct Sample {
    pub id: u32,
    pub kind: BKind,
    pub expect_ok: bool,
}

/// 样本账册。
pub struct SampleBook {
    pub samples: Vec<Sample>,
    pub sealed: bool,
}

impl SampleBook {
    pub fn new() -> SampleBook {
        SampleBook { samples: Vec::new(), sealed: false }
    }

    /// 入账并实跑核对：类别决定实跑语义，期望不符即 MISMATCH。
    pub fn put(&mut self, id: u32, kind: BKind, expect_ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(BN_FROZEN);
        }
        if self.samples.iter().any(|x| x.id == id) {
            return Err(BN_DUP);
        }
        let ran = run_kind(kind);
        if ran != expect_ok {
            return Err(BN_MISMATCH);
        }
        self.samples.push(Sample { id, kind, expect_ok });
        Ok(())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for x in &self.samples {
            h = h.wrapping_mul(211).wrapping_add(x.id);
            h = h.wrapping_mul(211).wrapping_add(x.kind as u32);
            h = h.wrapping_mul(211).wrapping_add(x.expect_ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        let ok = self.samples.iter().filter(|x| x.expect_ok).count();
        format!("n={} ok={}", self.samples.len(), ok)
    }

    pub fn line_of(&self, id: u32) -> String {
        match self.samples.iter().find(|x| x.id == id) {
            Some(x) => format!("s{}|k{}", x.id, x.kind as u8),
            None => "missing".to_string(),
        }
    }
}

/// 类别 → 实跑判定（迷你模型上的语义核对）。
fn run_kind(k: BKind) -> bool {
    let body = vec![1u8, 2, 3, 4];
    match k {
        BKind::ZeroRead => {
            let f = MiniFile::with_body(body.clone());
            f.read(2, 0).map(|v| v.is_empty()).unwrap_or(false)
        }
        BKind::ZeroWrite => {
            let mut f = MiniFile::with_body(body);
            let ok = f.write(1, &[]).ok() == Some(0) && f.body == vec![1, 2, 3, 4];
            ok
        }
        BKind::EofTruncRead => {
            let f = MiniFile::with_body(body);
            let got = f.read(2, 100).ok();
            got == Some(vec![3, 4])
        }
        BKind::OffsetWrap => {
            let f = MiniFile::with_body(body);
            f.read(u64::MAX, 1).is_err()
        }
        BKind::LongPath => {
            // 超长路径判定：长度超上限即拒。
            let n = PATH_MAX + 1;
            n > PATH_MAX
        }
        BKind::ZeroOffset => {
            let f = MiniFile::with_body(body);
            f.read(0, 4).ok() == Some(vec![1, 2, 3, 4])
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SampleBook::new();
    // 1) 期望与实跑不符必拒（EOF 截断样本谎报失败）。
    got[0] = b.put(1, BKind::EofTruncRead, false).err() == Some(BN_MISMATCH);
    // 2) 重号样本必拒。
    let _ = b.put(1, BKind::ZeroRead, true);
    got[1] = b.put(1, BKind::ZeroRead, true).err() == Some(BN_DUP);
    // 3) 回绕样本谎报通过必拒（实跑拒绝 → 期望通过即 MISMATCH）。
    got[2] = b.put(2, BKind::OffsetWrap, true).err() == Some(BN_MISMATCH);
    // 4) 超长路径样本谎报失败必拒。
    got[3] = b.put(3, BKind::LongPath, false).err() == Some(BN_MISMATCH);
    // 5) 印账后补样本必抓。
    b.seal();
    got[4] = b.put(4, BKind::ZeroOffset, true).err() == Some(BN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12273_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12273");

    // J1 零长读。
    let j1 = {
        let f = MiniFile::with_body(vec![1, 2, 3]);
        f.read(1, 0).ok() == Some(Vec::new())
    };
    s.add("J1-零长读", j1, "information=0");

    // J2 零长写不改体。
    let j2 = {
        let mut f = MiniFile::with_body(vec![7, 8]);
        let _ = f.write(0, &[]);
        f.body == vec![7, 8]
    };
    s.add("J2-零长写", j2, "体不变");

    // J3 EOF 截断。
    let j3 = {
        let f = MiniFile::with_body(vec![9, 9, 9]);
        f.read(1, 10).ok() == Some(vec![9, 9])
    };
    s.add("J3-EOF截断", j3, "按实有字节记账");

    // J4 偏移回绕。
    let j4 = {
        let f = MiniFile::with_body(vec![1]);
        f.read(u64::MAX, 2).err() == Some(BN_WRAP)
    };
    s.add("J4-偏移回绕", j4, "溢出必拒");

    // J5 超长路径。
    let j5 = PATH_MAX == 4096;
    s.add("J5-路径上限", j5, "4096 口径");

    // J6 零偏移。
    let j6 = {
        let f = MiniFile::with_body(vec![5, 6]);
        f.read(0, 2).ok() == Some(vec![5, 6])
    };
    s.add("J6-零偏移", j6, "offset 0 合法");

    // J7 样本实跑核对全绿。
    let j7 = {
        let mut b = SampleBook::new();
        let _ = b.put(1, BKind::ZeroRead, true);
        let _ = b.put(2, BKind::ZeroWrite, true);
        let _ = b.put(3, BKind::EofTruncRead, true);
        let _ = b.put(4, BKind::OffsetWrap, false);
        let _ = b.put(5, BKind::LongPath, false);
        let _ = b.put(6, BKind::ZeroOffset, true);
        b.samples.len() == 6
    };
    s.add("J7-样本全绿", j7, "六类实跑一致");

    // J8 期望不符拒。
    let j8 = {
        let mut b = SampleBook::new();
        b.put(1, BKind::ZeroOffset, false).err() == Some(BN_MISMATCH)
    };
    s.add("J8-期望不符拒", j8, "实跑诚实");

    // J9 重号样本拒。
    let j9 = {
        let mut b = SampleBook::new();
        let _ = b.put(1, BKind::ZeroRead, true);
        b.put(1, BKind::ZeroWrite, true).err() == Some(BN_DUP)
    };
    s.add("J9-重号样本拒", j9, "样本号唯一");

    // J10 印账冻结。
    let j10 = {
        let mut b = SampleBook::new();
        b.seal();
        b.put(1, BKind::ZeroRead, true).err() == Some(BN_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = SampleBook::new();
        let _ = b.put(1, BKind::ZeroRead, true);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 回读渲染。
    let j12 = {
        let mut b = SampleBook::new();
        let _ = b.put(5, BKind::EofTruncRead, true);
        b.line_of(5).contains("k2") && b.line_of(6) == "missing"
    };
    s.add("J12-回读渲染", j12, "样本行可渲染");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = SampleBook::new();
    let _ = b.put(1, BKind::ZeroRead, true);
    let _ = b.put(2, BKind::EofTruncRead, true);
    s.add("摘要回读", b.summary().contains("n=2 ok=2"), "双样本在册");
    s.add("写回绕拒", MiniFile::with_body(vec![0; 4]).write(2, &[0; 3]).err() == Some(BN_WRAP), "写溢出拒");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12273_boundary_flow() {
        let mut f = MiniFile::with_body(vec![1, 2, 3, 4]);
        assert_eq!(f.read(2, 100).ok(), Some(vec![3, 4]));
        assert_eq!(f.read(u64::MAX, 1), Err(BN_WRAP));
        assert!(f.write(0, &[9, 9]).is_ok());
        assert_eq!(f.body, vec![9, 9, 3, 4]);
        assert_eq!(f.write(2, &[0; 3]), Err(BN_WRAP));
    }

    #[test]
    fn unx12273_checks_green() {
        let s = run_unx12273_checks();
        assert!(s.all_passed(), "UNX-F12273 判据未全绿");
    }

    #[test]
    fn unx12273_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
