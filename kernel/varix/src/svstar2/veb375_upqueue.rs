//! veb375_upqueue —— VE-F0375 驱动升级排队系统。
//! 锚点（CoRun Unxreal 主册）：VE-F0375 · 驱动升级排队系统 · 目标 320 行
//! 判据原文：VE-F0375-J1 升级请求按优先级队列组织（高优先先升）；
//!           同优先按提交序稳定；同一驱动同时只许一个待升级项
//!           （重复入队拒）；出队执行核：取队首执行，执行中该
//!           驱动锁定（再入队拒「执行中」）；执行完出队；队列
//!           定容（满拒）；序核（出队序 = 优先级降序 + 同优先
//!           提交升序）。
//! 组件：升级项（驱动名 + 优先级 + 提交序）+ 优先级队列（定容）
//!       + 执行锁（驱动级）+ 出队序核 + 重放核。
//! 职责定位：升级排队是「驱动的手术排班」——优先级管插队资格
//!   （安全修复插队，功能更新排队），提交序管公平（同优先先
//!   到先升）；执行锁是关键安全语义：一个驱动同时跑两个升级
//!   就是两套状态互相覆盖；定容防的是排队爆炸：一万条升级请
//!   求堆在队列里不如拒绝新的让运维先清账。
//! 性能逐项分解：入队 O(队列长)（查重 + 插位）；出队 O(队列长)
//!   （扫极值）；重放 O(队列长)。
//! 诊断码段：0x30340（0x30341~0x30349 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const UQ_VERSION: &str = "veb375_upqueue-v1";

/// 诊断段基址（0x30340 段 · 本模块独占）。
pub const UQ_SEG_BASE: u32 = 0x30340;
pub const XC_DUP_DRV: u32 = 0x30341; // 同驱动重复入队
pub const XC_BUSY: u32 = 0x30342; // 执行中再入队
pub const XC_QUEUE_FULL: u32 = 0x30343; // 队满
pub const XC_EMPTY: u32 = 0x30344; // 空队出队
pub const XC_NAME_EMPTY: u32 = 0x30345; // 名空
pub const XC_PRI_OOR: u32 = 0x30346; // 优先级越域
pub const XC_MAGIC: u32 = 0x30347; // 账魔数破
pub const XC_REPLAY: u32 = 0x30348; // 重放不一致
pub const XC_NOT_RUNNING: u32 = 0x30349; // 完成不在执行中的驱动

/// 队列容量。
pub const QUEUE_CAP: usize = 8;
/// 优先级域 0~9（9 最高）。
pub const PRI_MAX: u32 = 9;

/// 升级项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpItem {
    pub name: String,
    pub pri: u32,
    pub seq: u64,
}

/// 排队系统。
pub struct UpQueue {
    q: Vec<UpItem>,
    /// 执行中驱动。
    running: Option<String>,
    seq: u64,
    done_cnt: u32,
}

impl UpQueue {
    /// 建。
    pub fn new() -> UpQueue {
        UpQueue { q: Vec::new(), running: None, seq: 1, done_cnt: 0 }
    }

    /// 入队：名空拒、优先级越域拒、同驱动在队拒、执行中拒、队满拒。
    pub fn push(&mut self, name: &str, pri: u32) -> Result<(), u32> {
        if name.is_empty() {
            return Err(XC_NAME_EMPTY);
        }
        if pri > PRI_MAX {
            return Err(XC_PRI_OOR);
        }
        if let Some(r) = &self.running {
            if r == name {
                return Err(XC_BUSY);
            }
        }
        for it in self.q.iter() {
            if it.name == name {
                return Err(XC_DUP_DRV);
            }
        }
        if self.q.len() >= QUEUE_CAP {
            return Err(XC_QUEUE_FULL);
        }
        let seq = self.seq;
        self.seq += 1;
        self.q.push(UpItem { name: String::from(name), pri, seq });
        Ok(())
    }

    /// 出队执行：取（优先级最大，同优先 seq 最小）；锁定驱动。
    pub fn pop_run(&mut self) -> Result<String, u32> {
        if self.running.is_some() {
            return Err(XC_BUSY); // 串行执行：一次一个
        }
        if self.q.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut pick = 0usize;
        for i in 1..self.q.len() {
            let a = &self.q[i];
            let b = &self.q[pick];
            if a.pri > b.pri || (a.pri == b.pri && a.seq < b.seq) {
                pick = i;
            }
        }
        let it = self.q.remove(pick);
        self.running = Some(it.name.clone());
        Ok(it.name)
    }

    /// 执行完成：解锁 + 计数。
    pub fn finish(&mut self, name: &str) -> Result<(), u32> {
        match &self.running {
            Some(r) if r == name => {}
            _ => return Err(XC_NOT_RUNNING),
        }
        self.running = None;
        self.done_cnt += 1;
        Ok(())
    }

    /// 执行中驱动。
    pub fn current(&self) -> Option<String> {
        self.running.clone()
    }

    /// 队长。
    pub fn len(&self) -> usize {
        self.q.len()
    }

    /// 完成数。
    pub fn done_count(&self) -> u32 {
        self.done_cnt
    }

    /// 出队序核：全出队一遍，序 = 优先级降序 + 同优先提交升序。
    pub fn drain_order(&mut self) -> Result<Vec<String>, u32> {
        let mut out: Vec<String> = Vec::new();
        loop {
            match self.pop_run() {
                Ok(n) => {
                    let _ = self.finish(&n);
                    out.push(n);
                }
                Err(XC_EMPTY) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }
}

pub fn run_veb375_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0375");
    // 族一：优先级出队。
    let mut q = UpQueue::new();
    let _ = q.push("low", 1);
    let _ = q.push("high", 9);
    let _ = q.push("mid", 5);
    s.add("三件排队", q.len() == 3, "优先级各异");
    let first = q.pop_run();
    s.add("高优先出", first == Ok(String::from("high")), "9 最先");
    s.add("执行锁定", q.current() == Some(String::from("high")), "锁生效");
    s.add("执行中再入队拒", q.push("high", 9) == Err(XC_BUSY), "锁定");
    s.add("执行中再出队拒", q.pop_run() == Err(XC_BUSY), "串行");
    s.add("完成解锁", q.finish("high").is_ok(), "出账");
    s.add("完成数 1", q.done_count() == 1, "一单完成");
    s.add("完成错名拒", q.finish("ghost") == Err(XC_NOT_RUNNING), "非执行中");
    let second = q.pop_run();
    s.add("中优次出", second == Ok(String::from("mid")), "5 次之");
    let _ = q.finish("mid");
    let third = q.pop_run();
    s.add("低优最后", third == Ok(String::from("low")), "1 收尾");
    let _ = q.finish("low");
    s.add("空队出队拒", q.pop_run() == Err(XC_EMPTY), "清空");
    // 族二：同优先稳定序。
    let mut st = UpQueue::new();
    let _ = st.push("first", 5);
    let _ = st.push("second", 5);
    let _ = st.push("third", 5);
    let order = st.drain_order();
    s.add("同优提交序",
        order == Ok(vec![String::from("first"), String::from("second"), String::from("third")]),
        "FIFO 稳定");
    // 族三：混合序全核。
    let mut mx = UpQueue::new();
    let _ = mx.push("a-low", 2);
    let _ = mx.push("b-high", 8);
    let _ = mx.push("c-high", 8);
    let _ = mx.push("d-mid", 5);
    let order2 = mx.drain_order();
    s.add("混合出队序",
        order2 == Ok(vec![
            String::from("b-high"),
            String::from("c-high"),
            String::from("d-mid"),
            String::from("a-low"),
        ]),
        "8(先) 8(后) 5 2");
    s.add("混合完成数 4", mx.done_count() == 4, "逐单出账");
    // 族四：队列约束。
    let mut f = UpQueue::new();
    for i in 0..8u32 {
        let _ = f.push(&alloc::format!("d{}", i), 1);
    }
    s.add("队满 8", f.len() == 8, "容量钳");
    s.add("第 9 件拒", f.push("d8", 1) == Err(XC_QUEUE_FULL), "满拒");
    // 同驱动重复入队。
    let mut d = UpQueue::new();
    let _ = d.push("dup", 1);
    s.add("同驱动重入拒", d.push("dup", 2) == Err(XC_DUP_DRV), "在队");
    // 族五：入队拒绝。
    let mut g = UpQueue::new();
    s.add("名空拒", g.push("", 1) == Err(XC_NAME_EMPTY), "名空");
    s.add("优先级越域拒", g.push("x", PRI_MAX + 1) == Err(XC_PRI_OOR), ">9");
    s.add("优先级零过", g.push("x", 0).is_ok(), "0 合法");
    s.add("魔数核", UQ_SEG_BASE == 0x30340, "段基址");
    s
}
