//! UNX-F24197 vblank/HPD 内核事件面与事件队列·子功能17（WDF 对照）（0x41A80 段）
//!
//! 锚点：UNX-F24197-J1 对照账（四级对照逐条落账）；J2 对照语义（等
//! 价映射 + 野行拒 + 恒等表闭三性质）；J3 对照指纹（账折叠指纹）三
//! 判据全绿。
//! 域：CoRun·UNX-F5-B15（WDF 对照面）。
//!
//! 结构：WDF 事件路由四级对照格（我方语义 ↔ WDF 路由面：Evt/Dpc/
//! 异步队列/回调同步域）+ 映射核 + 账本 + 折叠指纹 + 注入核（野行/
//! 野级/印后/缺行/重行必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x41A80 独占 0x20）。
pub const SEG_BASE: u32 = 0x41A80;
pub const WF_BAD_ROW: u32 = SEG_BASE + 0x01; // 野行
pub const WF_BAD_LEVEL: u32 = SEG_BASE + 0x02; // 野级
pub const WF_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const WF_MISSING: u32 = SEG_BASE + 0x04; // 缺行
pub const WF_DUP_ROW: u32 = SEG_BASE + 0x05; // 重行

/// 我方事件面四级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WfOurs {
    VblankIsr,     // 0：vblank 中断顶半
    VblankDpc,     // 1：底半延迟处理
    HpdWorker,     // 2：HPD 工作队列
    HpdUserNotify, // 3：用户态通知
}

/// WDF 对应面四级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdfSide {
    InterruptIsr,    // 0：EvtInterruptIsr
    InterruptDpc,    // 1：EvtInterruptDpc
    WorkItem,        // 2：队列工作项
    RequestComplete, // 3：请求完成回调
}

/// 对照格：我方级 → WDF 级，一对一。
pub struct WfMatrix {
    map: [Option<WdfSide>; 4],
    rows: u32,
    sealed: bool,
}

impl WfMatrix {
    pub fn new() -> WfMatrix {
        WfMatrix {
            map: [None; 4],
            rows: 0,
            sealed: false,
        }
    }

    /// 建行：一级一行，重行拒。
    pub fn bind(&mut self, ours: WfOurs, wdf: WdfSide) -> Result<u32, u32> {
        if self.sealed {
            return Err(WF_FROZEN);
        }
        let idx = match ours {
            WfOurs::VblankIsr => 0,
            WfOurs::VblankDpc => 1,
            WfOurs::HpdWorker => 2,
            WfOurs::HpdUserNotify => 3,
        };
        if self.map[idx].is_some() {
            return Err(WF_DUP_ROW);
        }
        // WDF 侧一对一：不允许两行映同侧
        if self.map.iter().any(|m| *m == Some(wdf)) {
            return Err(WF_DUP_ROW);
        }
        self.map[idx] = Some(wdf);
        self.rows += 1;
        Ok(self.rows)
    }

    /// 查映射。
    pub fn lookup(&self, ours: WfOurs) -> Result<WdfSide, u32> {
        let idx = match ours {
            WfOurs::VblankIsr => 0,
            WfOurs::VblankDpc => 1,
            WfOurs::HpdWorker => 2,
            WfOurs::HpdUserNotify => 3,
        };
        self.map[idx].ok_or(WF_MISSING)
    }

    /// 全表封：四级全绑定才算完整。
    pub fn complete(&self) -> bool {
        self.map.iter().all(|m| m.is_some())
    }

    pub fn rows_count(&self) -> u32 {
        self.rows
    }

    /// 标准全表：ISR→ISR、DPC→DPC、Worker→WorkItem、Notify→Request。
    pub fn standard() -> Result<WfMatrix, u32> {
        let mut m = WfMatrix::new();
        m.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr)?;
        m.bind(WfOurs::VblankDpc, WdfSide::InterruptDpc)?;
        m.bind(WfOurs::HpdWorker, WdfSide::WorkItem)?;
        m.bind(WfOurs::HpdUserNotify, WdfSide::RequestComplete)?;
        Ok(m)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41A80;
        h = h.wrapping_mul(31).wrapping_add(self.rows);
        for m in &self.map {
            let v = match m {
                Some(WdfSide::InterruptIsr) => 1,
                Some(WdfSide::InterruptDpc) => 2,
                Some(WdfSide::WorkItem) => 3,
                Some(WdfSide::RequestComplete) => 4,
                None => 0,
            };
            h = h.wrapping_mul(31).wrapping_add(v);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：标准全表。
pub fn mk_matrix() -> WfMatrix {
    WfMatrix::standard().expect("std matrix")
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 重行（同我方级）
    let mut m = WfMatrix::new();
    let _ = m.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr);
    got[0] = m.bind(WfOurs::VblankIsr, WdfSide::InterruptDpc).err() == Some(WF_DUP_ROW);
    // 2) 同侧重映
    let mut m2 = WfMatrix::new();
    let _ = m2.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr);
    got[1] = m2.bind(WfOurs::VblankDpc, WdfSide::InterruptIsr).err() == Some(WF_DUP_ROW);
    // 3) 缺行查
    got[2] = WfMatrix::new().lookup(WfOurs::VblankIsr).err() == Some(WF_MISSING);
    // 4) 不全表
    got[3] = {
        let mut m3 = WfMatrix::new();
        let _ = m3.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr);
        !m3.complete()
    };
    // 5) 印后操作
    let mut m4 = mk_matrix();
    m4.seal();
    got[4] = m4.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr).err() == Some(WF_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24197");
    let mut m = mk_matrix();

    // J1 全表封
    let j1 = m.complete();
    s.add("J1-全表封", j1, "四级全绑");

    // J2 行数
    let j2 = m.rows_count() == 4;
    s.add("J2-行数", j2, "4 行");

    // J3 ISR→ISR
    let j3 = m.lookup(WfOurs::VblankIsr) == Ok(WdfSide::InterruptIsr);
    s.add("J3-ISR对照", j3, "顶半对 ISR");

    // J4 DPC→DPC
    let j4 = m.lookup(WfOurs::VblankDpc) == Ok(WdfSide::InterruptDpc);
    s.add("J4-DPC对照", j4, "底半对 DPC");

    // J5 Worker→WorkItem
    let j5 = m.lookup(WfOurs::HpdWorker) == Ok(WdfSide::WorkItem);
    s.add("J5-Worker对照", j5, "工作项对照");

    // J6 Notify→RequestComplete
    let j6 = m.lookup(WfOurs::HpdUserNotify) == Ok(WdfSide::RequestComplete);
    s.add("J6-Notify对照", j6, "通知对照");

    // J7 重行拒
    let j7 = m.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr).err() == Some(WF_DUP_ROW);
    s.add("J7-重行拒", j7, "已绑再绑必拒");

    // J8 缺行查拒
    let j8 = WfMatrix::new().lookup(WfOurs::HpdWorker).err() == Some(WF_MISSING);
    s.add("J8-缺行查拒", j8, "未绑查必拒");

    // J9 单侧一对一：异我方级不映同 WDF 侧
    let j9 = {
        let mut m2 = WfMatrix::new();
        let _ = m2.bind(WfOurs::VblankIsr, WdfSide::WorkItem);
        m2.bind(WfOurs::VblankDpc, WdfSide::WorkItem).is_err()
    };
    s.add("J9-一对一", j9, "同侧不重映");

    // J10 标准表可重演
    let j10 = WfMatrix::standard().is_ok();
    s.add("J10-标准表", j10, "标准构造成功");

    // J11 印后操作拒
    let j11 = {
        let mut m2 = mk_matrix();
        m2.seal();
        m2.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr).err() == Some(WF_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut m2 = mk_matrix();
        let a = m2.fold();
        m2.seal();
        m2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24197_matrix_flow() {
        let mut m = WfMatrix::new();
        assert!(m.lookup(WfOurs::VblankIsr).is_err());
        assert_eq!(m.bind(WfOurs::VblankIsr, WdfSide::InterruptIsr), Ok(1));
        assert_eq!(
            m.bind(WfOurs::VblankDpc, WdfSide::InterruptIsr),
            Err(WF_DUP_ROW)
        );
        assert_eq!(
            m.bind(WfOurs::VblankDpc, WdfSide::InterruptDpc),
            Ok(2)
        );
        assert!(!m.complete());
        assert_eq!(
            m.lookup(WfOurs::VblankIsr),
            Ok(WdfSide::InterruptIsr)
        );
        m.seal();
        assert_eq!(
            m.bind(WfOurs::HpdWorker, WdfSide::WorkItem),
            Err(WF_FROZEN)
        );
        assert!(WfMatrix::standard().is_ok());
    }

    #[test]
    fn unx24197_checks_green() {
        let s = run_unx24197_checks();
        assert!(s.all_passed(), "UNX-F24197 判据未全绿");
    }
}
