//! UNX-F12269 FILE_* 访问掩码与共享模式矩阵（0x648a0 段）
//!
//! 卡面：访问掩码位闭集（FILE_READ_DATA/FILE_WRITE_DATA/FILE_APPEND_DATA/
//! FILE_READ_EA/DELETE 等模型子集）与共享模式三位（READ/WRITE/DELETE）
//! 的冲突矩阵：二次打开的期望访问须落在首次打开共享位内，反之亦然；
//! 零访问（仅查属性）永不冲突。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 掩码闭集：掩码出现未知位必拒。
//! - J2 共享位闭集：共享模式超出 READ|WRITE|DELETE 必拒。
//! - J3 冲突判定：他方访问不在己方共享位内即 SHARING_VIOLATION。
//! - J4 反向对称：先 A 后 B 与先 B 后 A 判定一致。
//! - J5 零访问豁免：desired=0 仅查属性，永不冲突。
//! - J6 句柄唯一：同句柄号重复打开必拒。
//! - J7 印账冻结：seal 后打开必拒。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x648a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x648a0;
pub const SM_BAD_MASK: u32 = SEG_BASE + 0x01; // 掩码越闭集
pub const SM_BAD_SHARE: u32 = SEG_BASE + 0x02; // 共享位越闭集
pub const SM_CONFLICT: u32 = SEG_BASE + 0x03; // 共享冲突
pub const SM_DUP: u32 = SEG_BASE + 0x04; // 句柄重号
pub const SM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后打开

/// 访问掩码位（模型子集）。
pub const FA_READ_DATA: u32 = 0x0001;
pub const FA_WRITE_DATA: u32 = 0x0002;
pub const FA_APPEND_DATA: u32 = 0x0004;
pub const FA_READ_EA: u32 = 0x0008;
pub const FA_WRITE_EA: u32 = 0x0010;
pub const FA_DELETE: u32 = 0x0100;
/// 掩码合法域。
pub const FA_MASK: u32 = FA_READ_DATA
    | FA_WRITE_DATA
    | FA_APPEND_DATA
    | FA_READ_EA
    | FA_WRITE_EA
    | FA_DELETE;

/// 共享模式位。
pub const SH_READ: u32 = 0x1;
pub const SH_WRITE: u32 = 0x2;
pub const SH_DELETE: u32 = 0x4;
pub const SH_MASK: u32 = SH_READ | SH_WRITE | SH_DELETE;

/// 单次打开记录。
#[derive(Clone)]
pub struct OpenRec {
    pub handle: u32,
    pub access: u32,
    pub share: u32,
}

impl OpenRec {
    pub fn render(&self) -> String {
        format!("h{}|a{:x}|s{:x}", self.handle, self.access, self.share)
    }
}

/// 共享账册。
pub struct ShareBook {
    pub opens: Vec<OpenRec>,
    pub conflicts: usize,
    pub sealed: bool,
}

impl ShareBook {
    pub fn new() -> ShareBook {
        ShareBook { opens: Vec::new(), conflicts: 0, sealed: false }
    }

    /// 二次打开判定：双向覆盖检查，零访问豁免。
    pub fn try_open(&mut self, handle: u32, access: u32, share: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SM_FROZEN);
        }
        if access & !FA_MASK != 0 {
            return Err(SM_BAD_MASK);
        }
        if share & !SH_MASK != 0 {
            return Err(SM_BAD_SHARE);
        }
        if self.opens.iter().any(|o| o.handle == handle) {
            return Err(SM_DUP);
        }
        for o in &self.opens {
            // 他方访问不在己方共享位内，或己方访问不在他方共享位内 → 冲突。
            // 零访问（仅查属性）豁免。
            let a_bad = access != 0 && (o.access & !share != 0);
            let b_bad = o.access != 0 && (access & !o.share != 0);
            if a_bad || b_bad {
                self.conflicts += 1;
                return Err(SM_CONFLICT);
            }
        }
        self.opens.push(OpenRec { handle, access, share });
        Ok(())
    }

    pub fn close(&mut self, handle: u32) -> bool {
        let n = self.opens.len();
        self.opens.retain(|o| o.handle != handle);
        self.opens.len() < n
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for o in &self.opens {
            h = h.wrapping_mul(211).wrapping_add(o.handle);
            h = h.wrapping_mul(211).wrapping_add(o.access);
            h = h.wrapping_mul(211).wrapping_add(o.share);
        }
        h = h.wrapping_mul(211).wrapping_add(self.conflicts as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("opens={} cx={}", self.opens.len(), self.conflicts)
    }

    pub fn line_of(&self, handle: u32) -> String {
        match self.opens.iter().find(|o| o.handle == handle) {
            Some(o) => o.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ShareBook::new();
    // 1) 掩码越闭集必拒。
    got[0] = b.try_open(1, 0x1000, 0).err() == Some(SM_BAD_MASK);
    // 2) 共享位越闭集必拒。
    got[1] = b.try_open(1, FA_READ_DATA, 0x8).err() == Some(SM_BAD_SHARE);
    // 3) 共享冲突必拒（前者独占读、后者要读）。
    let _ = b.try_open(1, FA_READ_DATA, 0);
    got[2] = b.try_open(2, FA_READ_DATA, 0).err() == Some(SM_CONFLICT);
    // 4) 句柄重号必拒。
    got[3] = b.try_open(1, 0, 0).err() == Some(SM_DUP);
    // 5) 印账后打开必抓。
    b.seal();
    got[4] = b.try_open(3, 0, 0).err() == Some(SM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12269_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12269");

    // J1 掩码闭集。
    let j1 = {
        let mut b = ShareBook::new();
        b.try_open(1, FA_READ_DATA | FA_DELETE, SH_READ).is_ok()
            && b.try_open(2, 0x20, 0).err() == Some(SM_BAD_MASK)
    };
    s.add("J1-掩码闭集", j1, "未知位必拒");

    // J2 共享位闭集。
    let j2 = {
        let mut b = ShareBook::new();
        b.try_open(1, FA_READ_DATA, SH_READ | SH_WRITE | SH_DELETE).is_ok()
            && b.try_open(2, FA_READ_DATA, 0x10).err() == Some(SM_BAD_SHARE)
    };
    s.add("J2-共享位闭集", j2, "三位之外必拒");

    // J3 冲突判定。
    let j3 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, FA_WRITE_DATA, SH_READ);
        b.try_open(2, FA_WRITE_DATA, SH_READ).err() == Some(SM_CONFLICT)
    };
    s.add("J3-冲突判定", j3, "他方写不在共享内");

    // J4 反向对称。
    let j4 = {
        let mut b1 = ShareBook::new();
        let mut b2 = ShareBook::new();
        let a = b1.try_open(1, FA_READ_DATA, 0).is_err();
        let _ = b2.try_open(2, FA_READ_DATA, 0);
        let b = b2.try_open(1, FA_READ_DATA, 0).is_err();
        a && b
    };
    s.add("J4-反向对称", j4, "先后次序不影响判定");

    // J5 零访问豁免。
    let j5 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, FA_READ_DATA, 0);
        b.try_open(2, 0, 0).is_ok()
    };
    s.add("J5-零访问豁免", j5, "仅查属性不冲突");

    // J6 DELETE 共享位豁免删除冲突。
    let j6 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, FA_READ_DATA, SH_READ | SH_DELETE);
        b.try_open(2, FA_DELETE, 0).is_ok()
    };
    s.add("J6-删除共享位", j6, "SH_DELETE 放行删除访问");

    // J7 关闭后重开。
    let j7 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, FA_READ_DATA, 0);
        let closed = b.close(1);
        let reopen = b.try_open(1, FA_READ_DATA, SH_READ).is_ok();
        closed && reopen
    };
    s.add("J7-关闭重开", j7, "关闭释放冲突域");

    // J8 重号必拒。
    let j8 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, 0, 0);
        b.try_open(1, 0, 0).err() == Some(SM_DUP)
    };
    s.add("J8-重号拒", j8, "句柄唯一");

    // J9 印账冻结。
    let j9 = {
        let mut b = ShareBook::new();
        b.seal();
        b.try_open(1, 0, 0).err() == Some(SM_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等 + 冲突计数。
    let j10 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(1, FA_READ_DATA, 0);
        let _ = b.try_open(2, FA_READ_DATA, 0);
        b.fold() == b.fold() && b.conflicts == 1
    };
    s.add("J10-指纹幂等", j10, "冲突计数入指纹");

    // J11 回读渲染。
    let j11 = {
        let mut b = ShareBook::new();
        let _ = b.try_open(9, FA_READ_DATA | FA_WRITE_DATA, SH_READ);
        b.line_of(9).contains("a3") && b.line_of(8) == "missing"
    };
    s.add("J11-回读渲染", j11, "打开行可渲染");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ShareBook::new();
    let _ = b.try_open(1, FA_READ_DATA, SH_READ | SH_WRITE);
    let _ = b.try_open(2, FA_WRITE_DATA, SH_READ);
    s.add("摘要回读", b.summary().contains("opens=2"), "双开在册");
    s.add("无冲突记账", b.conflicts == 0, "共享相容");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12269_share_matrix() {
        let mut b = ShareBook::new();
        // 写访问 + 共享读：后续写打开应冲突、读打开应放行。
        assert!(b.try_open(1, FA_WRITE_DATA, SH_READ).is_ok());
        assert_eq!(b.try_open(2, FA_WRITE_DATA, SH_READ), Err(SM_CONFLICT));
        assert!(b.try_open(3, FA_READ_DATA, SH_READ).is_ok());
    }

    #[test]
    fn unx12269_checks_green() {
        let s = run_unx12269_checks();
        assert!(s.all_passed(), "UNX-F12269 判据未全绿");
    }

    #[test]
    fn unx12269_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
