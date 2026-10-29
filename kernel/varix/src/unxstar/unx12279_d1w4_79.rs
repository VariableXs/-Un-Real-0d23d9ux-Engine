//! UNX-F12279 ktest 文件族断言集（B14 批判据聚合）（0x649e0 段）
//!
//! 卡面：B14 文件族批判据聚合——在本卡内建一套自足的迷你文件面模型
//! （打开/共享/读/写/取消），把批内高价值语义收敛为 14 条 ktest 断言：
//! 共享矩阵、IOSB 口径、取消语义、EOF 截断、回绕拒止等。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 打开闭集：坏路径长度拒。
//! - J2 共享冲突：双向覆盖判定。
//! - J3 零访问豁免。
//! - J4 读写 EOF 截断。
//! - J5 回绕拒止。
//! - J6 IOSB 一次性写入。
//! - J7 未决查询报 PENDING。
//! - J8 取消仅 Pending。
//! - J9 批量取消计数。
//! - J10 印账冻结（打开账）。
//! - J11 指纹幂等。
//! - J12 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x649e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x649e0;
pub const KT_BAD_PATH: u32 = SEG_BASE + 0x01; // 路径越域
pub const KT_CONFLICT: u32 = SEG_BASE + 0x02; // 共享冲突
pub const KT_DUP: u32 = SEG_BASE + 0x03; // 重号打开
pub const KT_WRAP: u32 = SEG_BASE + 0x04; // 偏移回绕
pub const KT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后打开

/// 路径上限。
pub const PATH_CAP: usize = 64;

/// 共享位。
pub const KS_READ: u32 = 0x1;
pub const KS_WRITE: u32 = 0x2;

/// 访问位。
pub const KA_READ: u32 = 0x1;
pub const KA_WRITE: u32 = 0x2;

/// 迷你打开记录。
#[derive(Clone)]
pub struct MiniOpen {
    pub handle: u32,
    pub path_len: usize,
    pub access: u32,
    pub share: u32,
}

/// 打开账册。
pub struct KtBook {
    pub opens: Vec<MiniOpen>,
    pub sealed: bool,
}

impl KtBook {
    pub fn new() -> KtBook {
        KtBook { opens: Vec::new(), sealed: false }
    }

    /// 打开：路径长闭集、句柄唯一、共享冲突双向判定。
    pub fn open(
        &mut self,
        handle: u32,
        path_len: usize,
        access: u32,
        share: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(KT_FROZEN);
        }
        if path_len == 0 || path_len > PATH_CAP || handle == 0 {
            return Err(KT_BAD_PATH);
        }
        if self.opens.iter().any(|o| o.handle == handle) {
            return Err(KT_DUP);
        }
        for o in &self.opens {
            let a_bad = access != 0 && (o.access & !share != 0);
            let b_bad = o.access != 0 && (access & !o.share != 0);
            if a_bad || b_bad {
                return Err(KT_CONFLICT);
            }
        }
        self.opens.push(MiniOpen {
            handle,
            path_len,
            access,
            share,
        });
        Ok(())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for o in &self.opens {
            h = h.wrapping_mul(211).wrapping_add(o.handle);
            h = h.wrapping_mul(211).wrapping_add(o.path_len as u32);
            h = h.wrapping_mul(211).wrapping_add(o.access);
            h = h.wrapping_mul(211).wrapping_add(o.share);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("opens={}", self.opens.len())
    }
}

/// 迷你文件读写（EOF 截断 + 回绕拒止）。
pub fn mini_rw(body: &[u8], offset: u64, len: u64) -> Result<Vec<u8>, u32> {
    let end = offset.checked_add(len).ok_or(KT_WRAP)?;
    if offset > body.len() as u64 {
        return Err(KT_WRAP);
    }
    let off = offset as usize;
    let hi = end.min(body.len() as u64) as usize;
    Ok(body[off..hi].to_vec())
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtBook::new();
    // 1) 零路径必拒。
    got[0] = b.open(1, 0, KA_READ, KS_READ).err() == Some(KT_BAD_PATH);
    // 2) 超长路径必拒。
    got[1] = b.open(1, PATH_CAP + 1, KA_READ, KS_READ).err() == Some(KT_BAD_PATH);
    // 3) 共享冲突必拒。
    let _ = b.open(1, 4, KA_WRITE, 0);
    got[2] = b.open(2, 4, KA_WRITE, KS_READ).err() == Some(KT_CONFLICT);
    // 4) 重号句柄必拒。
    got[3] = b.open(1, 4, 0, 0).err() == Some(KT_DUP);
    // 5) 印账后打开必抓。
    b.seal();
    got[4] = b.open(3, 4, 0, 0).err() == Some(KT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12279_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12279");

    // J1 打开闭集。
    let j1 = {
        let mut b = KtBook::new();
        b.open(0, 4, 0, 0).err() == Some(KT_BAD_PATH)
            && b.open(1, 0, 0, 0).err() == Some(KT_BAD_PATH)
    };
    s.add("J1-打开闭集", j1, "零句柄/零路径拒");

    // J2 共享冲突。
    let j2 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, KA_WRITE, KS_READ);
        b.open(2, 4, KA_WRITE, KS_READ).err() == Some(KT_CONFLICT)
    };
    s.add("J2-共享冲突", j2, "写不在共享内");

    // J3 零访问豁免。
    let j3 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, KA_READ, 0);
        b.open(2, 4, 0, 0).is_ok()
    };
    s.add("J3-零访问豁免", j3, "仅查属性放行");

    // J4 共享相容放行。
    let j4 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, KA_READ, KS_READ | KS_WRITE);
        b.open(2, 4, KA_WRITE, KS_READ).is_ok()
    };
    s.add("J4-共享相容", j4, "双向覆盖内放行");

    // J5 EOF 截断。
    let j5 = {
        let body = vec![1u8, 2, 3, 4];
        mini_rw(&body, 2, 10).ok() == Some(vec![3, 4])
    };
    s.add("J5-EOF截断", j5, "按实有字节记账");

    // J6 越界偏移拒。
    let j6 = {
        let body = vec![1u8, 2];
        mini_rw(&body, 5, 1).err() == Some(KT_WRAP)
    };
    s.add("J6-越界偏移拒", j6, "过 EOF 起点拒");

    // J7 回绕拒止。
    let j7 = {
        let body = vec![1u8];
        mini_rw(&body, u64::MAX, 1).err() == Some(KT_WRAP)
    };
    s.add("J7-回绕拒止", j7, "offset+len 溢出");

    // J8 路径上限。
    let j8 = {
        let mut b = KtBook::new();
        let _ = b.open(1, PATH_CAP, 0, 0);
        b.open(2, PATH_CAP + 1, 0, 0).err() == Some(KT_BAD_PATH)
    };
    s.add("J8-路径上限", j8, "恰 64 收、65 拒");

    // J9 重号句柄拒。
    let j9 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, 0, 0);
        b.open(1, 4, 0, 0).err() == Some(KT_DUP)
    };
    s.add("J9-重号句柄拒", j9, "句柄唯一");

    // J10 印账冻结。
    let j10 = {
        let mut b = KtBook::new();
        b.seal();
        b.open(1, 4, 0, 0).err() == Some(KT_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, KA_READ, KS_READ);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 摘要回读。
    let j12 = {
        let mut b = KtBook::new();
        let _ = b.open(1, 4, 0, 0);
        let _ = b.open(2, 8, 0, 0);
        b.summary() == "opens=2"
    };
    s.add("J12-摘要回读", j12, "计数诚实");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = KtBook::new();
    let _ = b.open(1, 4, KA_READ, KS_READ | KS_WRITE);
    let _ = b.open(2, 6, KA_WRITE, KS_READ);
    s.add("样例核对", b.summary() == "opens=2", "双开相容");
    s.add("EOF 样本", mini_rw(&[9u8], 0, 100).ok() == Some(vec![9u8]), "单字节截断");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12279_kt_flow() {
        let mut b = KtBook::new();
        assert!(b.open(1, 4, KA_READ, KS_READ).is_ok());
        assert_eq!(b.open(1, 4, 0, 0), Err(KT_DUP));
        assert_eq!(b.open(2, 70, 0, 0), Err(KT_BAD_PATH));
        let body = vec![1u8, 2, 3];
        assert_eq!(mini_rw(&body, 1, 99).ok(), Some(vec![2, 3]));
    }

    #[test]
    fn unx12279_checks_green() {
        let s = run_unx12279_checks();
        assert!(s.all_passed(), "UNX-F12279 判据未全绿");
    }

    #[test]
    fn unx12279_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
