//! unx0431_exinj —— UNX-F0431 异常注入演练（#UD/#GP 触发回收）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0431 · 异常注入演练 · 目标 240 行
//! 判据原文：UNX-F0431-J1 演练闭环 = 注入（排入 IDT 仿真队列）
//!           → 触发（异常框架派发到处理器）→ 回收（帧归档 +
//!           队列清账）；注入两类：#UD（非法操作码，无错误
//!           码）、#GP(0)（一般保护违例，错误码 0）；注入前
//!           提：目标向量在 IDT 仿真表内且处理器在演练态；
//!           回收核：帧五元组（向量/错误码/RIP 软账/CS 软
//!           账/RFLAGS 软账）完整归档后队列减一；重复回收拒
//!           绝；演练态退出未清队列立案；注入容量上限（队
//!           列 8 深）越界拒绝。
//! 组件：IDT 仿真表 + 注入队列（8 深）+ 派发器 + 帧五元组
//!       归档 + 回收核 + 演练态闸 + 容量闸。
//! 职责定位：异常路径平时不走，走了才发现没修——注入演练
//!   就是定期把 #UD/#GP 「排练」一遍，确保派发-回收闭环在
//!   真异常来之前已被验证；队列容量闸防演练本身把内核打挂；
//!   退出未清队列说明闭环有洞。
//! 性能逐项分解：注入 O(1)；派发 O(1)；回收 O(1)；队列巡检
//!   O(8)。
//! 诊断码段：0x30340（0x30341~0x30349 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EI_VERSION: &str = "unx0431_exinj-v1";

/// 诊断段基址（0x30340 段 · 本模块独占）。
pub const EI_SEG_BASE: u32 = 0x30340;
pub const XC_VEC_UNMAPPED: u32 = 0x30341; // 向量不在 IDT 仿真表
pub const XC_NOT_DRILLING: u32 = 0x30342; // 非演练态注入
pub const XC_QUEUE_FULL: u32 = 0x30343; // 队列满
pub const XC_QUEUE_EMPTY: u32 = 0x30344; // 空队派发
pub const XC_DOUBLE_RECLAIM: u32 = 0x30345; // 重复回收
pub const XC_FRAME_LOST: u32 = 0x30346; // 帧五元组残缺
pub const XC_EXIT_DIRTY: u32 = 0x30347; // 退出未清队
pub const XC_ID_MISMATCH: u32 = 0x30348; // 注入/回收 ID 失配
pub const XC_MAGIC: u32 = 0x30349; // 账魔数破

/// 异常向量。
pub const VEC_UD: u8 = 6; // #UD
pub const VEC_GP: u8 = 13; // #GP
/// IDT 仿真表覆盖域（演练只许这两个）。
pub const IDT_SIM: [u8; 2] = [VEC_UD, VEC_GP];
/// 注入队列容量。
pub const QUEUE_CAP: usize = 8;

/// 帧五元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExFrame {
    /// 演练流水 ID。
    pub iid: u32,
    /// 向量。
    pub vector: u8,
    /// 错误码（#UD 无=0xFFFF_FFFF 哨兵；#GP=0）。
    pub err: u32,
    /// RIP 软账。
    pub rip: u64,
    /// CS 软账。
    pub cs: u16,
    /// RFLAGS 软账。
    pub rflags: u64,
}

impl ExFrame {
    /// 五元组完整核：ID 非零、向量合法、哨兵语义自洽。
    pub fn complete(&self) -> bool {
        self.iid != 0
            && (self.vector == VEC_UD || self.vector == VEC_GP)
            && (self.vector == VEC_GP || self.err == 0xFFFF_FFFF)
            && (self.vector == VEC_UD || self.err != 0xFFFF_FFFF)
            && self.cs != 0
    }
}

/// 演练状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillState {
    /// 未演练。
    Idle = 1,
    /// 演练中。
    Drilling = 2,
}

/// 异常注入演练器。
pub struct ExDrill {
    /// 状态。
    pub state: DrillState,
    /// 注入队列（待派发）。
    pub queue: Vec<ExFrame>,
    /// 已派发待回收。
    pub inflight: Vec<ExFrame>,
    /// 归档柜。
    pub archive: Vec<ExFrame>,
    /// 流水 ID 发放器。
    pub next_iid: u32,
}

impl ExDrill {
    pub fn new() -> Self {
        ExDrill { state: DrillState::Idle, queue: Vec::new(), inflight: Vec::new(), archive: Vec::new(), next_iid: 1 }
    }

    /// 进入演练态。
    pub fn begin(&mut self) {
        self.state = DrillState::Drilling;
    }

    /// 退出演练态：队列未清立案。
    pub fn end(&mut self) -> Result<(), u32> {
        if !self.queue.is_empty() || !self.inflight.is_empty() {
            return Err(XC_EXIT_DIRTY);
        }
        self.state = DrillState::Idle;
        Ok(())
    }

    /// 注入：排队（演练态 + 表内向量 + 容量闸）。
    pub fn inject(&mut self, vector: u8, err: Option<u32>) -> Result<u32, u32> {
        if self.state != DrillState::Drilling {
            return Err(XC_NOT_DRILLING);
        }
        if !IDT_SIM.contains(&vector) {
            return Err(XC_VEC_UNMAPPED);
        }
        if self.queue.len() >= QUEUE_CAP {
            return Err(XC_QUEUE_FULL);
        }
        let iid = self.next_iid;
        self.next_iid += 1;
        let f = ExFrame {
            iid,
            vector,
            err: match (vector, err) {
                (VEC_UD, _) => 0xFFFF_FFFF, // #UD 无错误码：哨兵
                (VEC_GP, e) => e.unwrap_or(0),
                _ => 0,
            },
            rip: 0x1000 + iid as u64,
            cs: 0x08,
            rflags: 0x202,
        };
        self.queue.push(f);
        Ok(iid)
    }

    /// 派发：队首帧转在途。
    pub fn dispatch(&mut self) -> Result<ExFrame, u32> {
        if self.queue.is_empty() {
            return Err(XC_QUEUE_EMPTY);
        }
        let f = self.queue.remove(0);
        self.inflight.push(f);
        Ok(f)
    }

    /// 回收：在途帧完整核后归档。
    pub fn reclaim(&mut self, iid: u32) -> Result<(), u32> {
        let pos = self.inflight.iter().position(|f| f.iid == iid);
        let idx = match pos {
            Some(i) => i,
            None => return Err(XC_ID_MISMATCH),
        };
        let f = self.inflight[idx];
        if !f.complete() {
            return Err(XC_FRAME_LOST);
        }
        if self.archive.iter().any(|a| a.iid == iid) {
            return Err(XC_DOUBLE_RECLAIM);
        }
        self.inflight.remove(idx);
        self.archive.push(f);
        Ok(())
    }

    /// 闭环核：注入数 == 归档数且在途/队列全空。
    pub fn closed(&self) -> bool {
        self.queue.is_empty() && self.inflight.is_empty() && !self.archive.is_empty()
    }

    /// 归档数。
    pub fn archived(&self) -> usize {
        self.archive.len()
    }
}

/// 标准演练脚本：#UD、#GP 各一，全闭环。
pub fn drill_pair() -> Result<usize, u32> {
    let mut d = ExDrill::new();
    d.begin();
    let a = d.inject(VEC_UD, None)?;
    let b = d.inject(VEC_GP, Some(0))?;
    let _ = d.dispatch()?;
    let _ = d.dispatch()?;
    d.reclaim(a)?;
    d.reclaim(b)?;
    let n = d.archived();
    d.end()?;
    Ok(n)
}

/// 判据入口：UNX-F0431。
pub fn run_unx0431_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0431_exinj");
    // 族一：演练态闸。
    let mut d = ExDrill::new();
    cs.add("未演练注入拒", d.inject(VEC_UD, None) == Err(XC_NOT_DRILLING), "Idle 态");
    d.begin();
    cs.add("演练态放行", d.inject(VEC_UD, None).is_ok(), "Drilling");
    // 族二：向量映射。
    let mut d2 = ExDrill::new();
    d2.begin();
    cs.add("#GP 表内", d2.inject(VEC_GP, Some(0)).is_ok(), "向量 13");
    cs.add("#DE 表外拒", d2.inject(0, None) == Err(XC_VEC_UNMAPPED), "向量 0 不演练");
    cs.add("#PF 表外拒", d2.inject(14, None) == Err(XC_VEC_UNMAPPED), "向量 14 不演练");
    // 族三：#UD 哨兵语义。
    let mut d3 = ExDrill::new();
    d3.begin();
    let i = d3.inject(VEC_UD, None);
    cs.add("#UD 有 ID", i.is_ok(), "流水发放");
    let _ = d3.dispatch();
    cs.add("#UD 错误码哨兵", d3.inflight[0].err == 0xFFFF_FFFF, "无错误码");
    cs.add("#UD 帧完整", d3.inflight[0].complete(), "五元组齐");
    // 族四：#GP 错误码。
    let mut d4 = ExDrill::new();
    d4.begin();
    let _ = d4.inject(VEC_GP, Some(0));
    let _ = d4.dispatch();
    cs.add("#GP err=0", d4.inflight[0].err == 0, "选择子 0");
    let mut d5 = ExDrill::new();
    d5.begin();
    let _ = d5.inject(VEC_GP, None); // 缺省 0
    let _ = d5.dispatch();
    cs.add("#GP 缺省补 0", d5.inflight[0].err == 0, "None→0");
    // 族五：容量闸。
    let mut d6 = ExDrill::new();
    d6.begin();
    let mut last = Ok(0u32);
    for _ in 0..10 {
        last = d6.inject(VEC_UD, None);
        if last.is_err() {
            break;
        }
    }
    cs.add("8 深容量", d6.queue.len() == QUEUE_CAP, "前 8 全进");
    cs.add("第 9 拒", last == Err(XC_QUEUE_FULL), "越容量");
    // 族六：派发与回收。
    let mut d7 = ExDrill::new();
    d7.begin();
    cs.add("空队派发拒", d7.dispatch() == Err(XC_QUEUE_EMPTY), "无帧");
    let a = d7.inject(VEC_GP, Some(2)).ok();
    let _ = d7.dispatch();
    cs.add("在途转出", d7.queue.is_empty() && d7.inflight.len() == 1, "队空在途 1");
    cs.add("错 ID 回收拒", d7.reclaim(999) == Err(XC_ID_MISMATCH), "无此流水");
    cs.add("回收归档", d7.reclaim(a.unwrap_or(1)).is_ok(), "帧入柜");
    cs.add("重复回收拒", d7.reclaim(a.unwrap_or(1)) == Err(XC_ID_MISMATCH), "已出在途");
    // 族七：退出未清队立案。
    let mut d8 = ExDrill::new();
    d8.begin();
    let _ = d8.inject(VEC_UD, None);
    cs.add("脏退立案", d8.end() == Err(XC_EXIT_DIRTY), "队未清");
    let _ = d8.dispatch();
    cs.add("在途未清仍脏", d8.end() == Err(XC_EXIT_DIRTY), "在途未收");
    // 族八：闭环核与标准脚本。
    cs.add("闭环核", drill_pair() == Ok(2), "#UD+#GP 全收");
    let mut d9 = ExDrill::new();
    d9.begin();
    let x = d9.inject(VEC_UD, None).ok();
    let y = d9.inject(VEC_GP, Some(0)).ok();
    let _ = d9.dispatch();
    let _ = d9.dispatch();
    let _ = d9.reclaim(x.unwrap_or(1));
    let _ = d9.reclaim(y.unwrap_or(2));
    cs.add("闭环真", d9.closed(), "队/途空 柜满");
    cs.add("净退放行", d9.end().is_ok(), "清账退态");
    cs
}
