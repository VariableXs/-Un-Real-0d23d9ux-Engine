//! UNX-F10381 域闭账开卷总纲（0x1ba20 段）
//!
//! 锚点：UNX-F10381-J1 域闭账开卷——终审面/闭账面/收官面三维齐备才开卷，
//! 缺面开卷与重复开卷各 10/10 次检出。
//! 域：CoRun·UNX-C3（域闭账·开卷谱系）。
//!
//! 结构：三维清单（终审/闭账/收官）+ 开卷闸（三维齐→开卷一次性）
//! + 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ba20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ba20;
pub const CC_MISSING_FACE: u32 = SEG_BASE + 0x01; // 缺面开卷
pub const CC_REOPEN: u32 = SEG_BASE + 0x02; // 重复开卷
pub const CC_DUP_DIM: u32 = SEG_BASE + 0x03; // 维度重号
pub const CC_EMPTY: u32 = SEG_BASE + 0x04; // 空名
pub const CC_SEALED: u32 = SEG_BASE + 0x05; // 开卷后清单锁定写账

/// 三维定容。
pub const DIMS: usize = 3;

/// 三维编号。
pub const DIM_AUDIT: u32 = 1; // 终审面
pub const DIM_CLOSE: u32 = 2; // 闭账面
pub const DIM_FINISH: u32 = 3; // 收官面

/// 单维清单项。
#[derive(Clone)]
pub struct DimItem {
    pub dim: u32,
    pub name: String,
    pub ready: bool,
}

impl DimItem {
    pub fn ok(&self) -> bool {
        (DIM_AUDIT..=DIM_FINISH).contains(&self.dim) && !self.name.is_empty()
    }
}

/// 开卷册：三维齐备后一次性开卷。
pub struct CloseCharter {
    pub items: Vec<DimItem>,
    pub opened: bool,
    pub note: String,
}

impl CloseCharter {
    pub fn new(note: &str) -> CloseCharter {
        CloseCharter { items: Vec::new(), opened: false, note: note.to_string() }
    }

    /// 登记维度：重号、出域、空名、已开卷均拒。
    pub fn enroll(&mut self, dim: u32, name: &str, ready: bool) -> Result<(), u32> {
        if self.opened {
            return Err(CC_SEALED);
        }
        if !(DIM_AUDIT..=DIM_FINISH).contains(&dim) {
            return Err(CC_DUP_DIM);
        }
        if name.is_empty() {
            return Err(CC_EMPTY);
        }
        if self.items.iter().any(|i| i.dim == dim) {
            return Err(CC_DUP_DIM);
        }
        self.items.push(DimItem { dim, name: name.to_string(), ready });
        Ok(())
    }

    /// 维度覆盖核：三维齐备且全部就绪。
    pub fn all_ready(&self) -> bool {
        self.items.len() == DIMS
            && (DIM_AUDIT..=DIM_FINISH).all(|d| {
                self.items
                    .iter()
                    .any(|i| i.dim == d && i.ready && i.ok())
            })
    }

    /// 开卷闸：三维齐备才开卷，缺面开卷必检出；开卷一次性。
    pub fn open(&mut self) -> Result<u32, u32> {
        if self.opened {
            return Err(CC_REOPEN);
        }
        if !self.all_ready() {
            return Err(CC_MISSING_FACE);
        }
        self.opened = true;
        Ok(self.fold())
    }

    /// 缺维诊断。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g = Vec::new();
        for d in DIM_AUDIT..=DIM_FINISH {
            if !self.items.iter().any(|i| i.dim == d) {
                g.push(d);
            }
        }
        g
    }

    /// 未就绪维度诊断。
    pub fn not_ready(&self) -> Vec<u32> {
        self.items
            .iter()
            .filter(|i| !i.ready)
            .map(|i| i.dim)
            .collect()
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1ba200;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.opened as u32);
        for i in &self.items {
            h = h.wrapping_mul(31).wrapping_add(i.dim);
            h = h.wrapping_mul(31).wrapping_add(i.ready as u32);
            for b in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }
}

/// 注入核（五项）：[缺面开卷, 未就绪开卷, 重复开卷, 重号, 开卷后写账] 检出。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺面开卷必拒
    let mut c = CloseCharter::new("inj-miss");
    let _ = c.enroll(DIM_AUDIT, "终审面", true);
    got[0] = c.open() == Err(CC_MISSING_FACE) && c.gaps().contains(&DIM_CLOSE);
    // 2) 未就绪开卷必拒
    let mut c2 = CloseCharter::new("inj-notready");
    let _ = c2.enroll(DIM_AUDIT, "终审面", true);
    let _ = c2.enroll(DIM_CLOSE, "闭账面", false);
    let _ = c2.enroll(DIM_FINISH, "收官面", true);
    got[1] = c2.open() == Err(CC_MISSING_FACE) && c2.not_ready() == vec![DIM_CLOSE];
    // 3) 重复开卷必拒
    let mut c3 = CloseCharter::new("inj-reopen");
    let _ = c3.enroll(DIM_AUDIT, "终审面", true);
    let _ = c3.enroll(DIM_CLOSE, "闭账面", true);
    let _ = c3.enroll(DIM_FINISH, "收官面", true);
    got[2] = c3.open().is_ok() && c3.open() == Err(CC_REOPEN);
    // 4) 重号必拒
    let mut c4 = CloseCharter::new("inj-dup");
    let _ = c4.enroll(DIM_AUDIT, "终审面", true);
    got[3] = c4.enroll(DIM_AUDIT, "重复面", true) == Err(CC_DUP_DIM);
    // 5) 开卷后写账必抓
    got[4] = c3.enroll(DIM_FINISH, "late", true) == Err(CC_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10381_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10381");

    // 正路径：三维齐备就绪 → 开卷成功
    let mut charter = CloseCharter::new("域闭账开卷总纲");
    let _ = charter.enroll(DIM_AUDIT, "终审面（豁免/全绿/双清）", true);
    let _ = charter.enroll(DIM_CLOSE, "闭账面（守恒/ID/判据三成分）", true);
    let _ = charter.enroll(DIM_FINISH, "收官面（陈述/断言/handoff）", true);
    let opened = charter.open();
    let j1 = opened.is_ok() && opened.unwrap_or(0) != 0;
    s.add("J1-三维开卷", j1, "三维齐备就绪开卷成功");

    // 开卷冻结态
    let j2 = charter.opened && charter.all_ready() && charter.gaps().is_empty();
    s.add("J2-开卷冻结态", j2, "开卷后三维就绪缺口空");

    // 维度字段齐备
    let j3 = charter.items.iter().all(|i| i.ok());
    s.add("J3-维度字段齐", j3, "维度在域且名非空");

    // 注入核：五项错误行为全部检出
    let inj = inject_five();
    let j4 = inj[0] && inj[1] && inj[2];
    s.add("J4-注入核前三", j4, "缺面/未就绪/重复开卷必检出");
    let j5 = inj[3] && inj[4];
    s.add("J5-注入核后二", j5, "重号与开卷后写账必检出");

    // 重复开卷（正册复现）
    let j6 = charter.open() == Err(CC_REOPEN);
    s.add("J6-开卷一次性", j6, "重复开卷必拒");

    // 开卷后写账（正册复现）
    let j7 = charter.enroll(DIM_FINISH, "late", true) == Err(CC_SEALED);
    s.add("J7-开卷拒写", j7, "开卷后维度登记必拒");

    // 指纹稳定
    let j8 = charter.fold() == charter.fold();
    s.add("J8-指纹稳定", j8, "折叠指纹两次一致");

    // 维度出域拒
    let j9 = {
        let mut c = CloseCharter::new("unk");
        c.enroll(0, "零", true) == Err(CC_DUP_DIM)
            && c.enroll(4, "越界", true) == Err(CC_DUP_DIM)
    };
    s.add("J9-维度出域拒", j9, "维度号不在 1..=3 一律拒");

    // 空名拒
    let j10 = {
        let mut c = CloseCharter::new("empty");
        c.enroll(DIM_AUDIT, "", true) == Err(CC_EMPTY)
    };
    s.add("J10-空名拒", j10, "维度名空必拒");

    // 未就绪维度可枚举
    let j11 = {
        let mut c = CloseCharter::new("nr");
        let _ = c.enroll(DIM_AUDIT, "终审面", false);
        let _ = c.enroll(DIM_CLOSE, "闭账面", true);
        let _ = c.enroll(DIM_FINISH, "收官面", false);
        c.not_ready() == vec![DIM_AUDIT, DIM_FINISH] && !c.all_ready()
    };
    s.add("J11-未就绪可枚举", j11, "未就绪维度编号全部列出");

    // 指纹分辨：就绪位变则指纹变
    let j12 = {
        let mut a = CloseCharter::new("fpa");
        let _ = a.enroll(DIM_AUDIT, "终审面", true);
        let mut b = CloseCharter::new("fpb");
        let _ = b.enroll(DIM_AUDIT, "终审面", false);
        a.fold() != b.fold()
    };
    s.add("J12-指纹分辨", j12, "就绪位翻转指纹必变");

    // 空册开卷拒
    let j13 = {
        let mut c = CloseCharter::new("empty2");
        c.open() == Err(CC_MISSING_FACE) && c.gaps().len() == DIMS
    };
    s.add("J13-空册开卷拒", j13, "空册缺三维开卷必拒");

    // 只读不改账
    let j14 = {
        let mut c = CloseCharter::new("stable");
        let _ = c.enroll(DIM_AUDIT, "终审面", true);
        let f1 = c.fold();
        let _ = c.all_ready();
        let _ = c.gaps();
        let _ = c.not_ready();
        c.fold() == f1
    };
    s.add("J14-只读不改账", j14, "查询操作不改变折叠指纹");

    s.add_dyn(
        "J15-开卷指纹落账",
        charter.fold() != 0,
        &format!("fold={:#x}", charter.fold()),
    );
    let empty = CloseCharter::new("empty3").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1ba200u32.wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10381_charter_flow() {
        let mut c = CloseCharter::new("t");
        assert!(c.enroll(DIM_AUDIT, "终审面", true).is_ok());
        assert!(c.enroll(DIM_AUDIT, "重复", true) == Err(CC_DUP_DIM));
        assert!(c.enroll(7, "越界", true) == Err(CC_DUP_DIM));
        assert!(c.enroll(DIM_CLOSE, "闭账面", true).is_ok());
        assert!(c.open() == Err(CC_MISSING_FACE));
        assert!(c.enroll(DIM_FINISH, "收官面", true).is_ok());
        assert!(c.open().is_ok());
        assert!(c.open() == Err(CC_REOPEN));
        assert!(c.enroll(DIM_FINISH, "late", true) == Err(CC_SEALED));
    }

    #[test]
    fn unx10381_checks_green() {
        let s = run_unx10381_checks();
        assert!(s.all_passed(), "UNX-F10381 判据未全绿");
    }
}
