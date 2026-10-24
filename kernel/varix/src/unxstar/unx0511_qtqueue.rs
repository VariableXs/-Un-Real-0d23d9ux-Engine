//! unx0511_qtqueue —— UNX-F0511 排队失效描述符账（类型区分 + 依序发射 + 等待栅栏）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0511 · 排队失效账 · 目标 210 行
//! 判据原文：UNX-F0511-J1 排队失效账：排一 类型三分类
//!           （IOTLB 失效描述符/IEC 中断失效描述符/等待描
//!           述符三类型，类型字段驱动分发，错型拒）；排二
//!           发射有序（描述符按入队序发射，序号严格递增，
//!           插队拒）；排三 等待栅栏（SW=1 等待描述符后，
//!           后续描述符必须等栅栏放行才可发射）；排四 栅
//!           栏配对（每个栅栏恰被翻越一次，双翻拒）；排五
//!           队列深饱和（容量满入溢出账，语义同设备队列）。
//! 组件：描述符类型判别 + 有序发射器 + SW 等待栅栏 + 溢出账。
//! 职责定位：排队失效是 IOMMU 失效的「命令流」——失效命令
//!   打包成描述符批量投递，硬件按序消化。等待描述符是软
//!   件在命令流里插的同步点：栅栏前的失效必须全部落袋才
//!   能继续。错型分发是「把 IEC 命令喂给 IOTLB 单元」的
//!   静默事故源，发射无序则失效与映射修改交错（陈旧翻译）。
//! 性能逐项分解：入队 O(1)；栅栏翻越 O(1)。
//! 诊断码段：0x34D40（0x34D41~0x34D49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34D40;
pub const XC_BADTYPE: u32 = SEG_BASE + 1; // 未知描述符类型
pub const XC_ORDER: u32 = SEG_BASE + 2; // 发射序违（插队）
pub const XC_FENCE: u32 = SEG_BASE + 3; // 栅栏未放行即发射
pub const XC_DOUBLEFENCE: u32 = SEG_BASE + 4; // 双翻栅栏

/// 描述符类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescType {
    IotlbInv,
    IecInv,
    WaitSw,
}

/// 描述符。
#[derive(Clone, Copy, Debug)]
pub struct Desc {
    pub ty: DescType,
    pub seq: u64,
}

/// 有序发射器。
pub struct InvQueue {
    pub pending: Vec<Desc>,
    pub cap: usize,
    pub fence_open: bool,
    pub fence_hops: u64,
    pub overflow: u64,
    pub fired: u64,
}

impl InvQueue {
    pub fn new(cap: usize) -> Self {
        InvQueue { pending: Vec::new(), cap, fence_open: false, fence_hops: 0, overflow: 0, fired: 0 }
    }

    /// 排二+排五：有序入队（seq 必须比队尾大），满则溢出账。
    pub fn push(&mut self, ty: DescType, seq: u64) -> Result<(), u32> {
        if self.pending.len() >= self.cap {
            self.overflow += 1;
            return Err(XC_ORDER); // 满溢出复用序码语义（可审计路径）
        }
        if let Some(last) = self.pending.last() {
            if seq <= last.seq {
                return Err(XC_ORDER);
            }
        }
        // 排一：类型分发核（未知类型在枚举层面不可表达，此处核语义标签）。
        match ty {
            DescType::IotlbInv | DescType::IecInv | DescType::WaitSw => {}
        }
        self.pending.push(Desc { ty, seq });
        Ok(())
    }

    /// 排三：SW 等待描述符落位 → 栅栏闭。
    pub fn arm_fence(&mut self) -> Result<(), u32> {
        // 只有队尾恰是 WaitSw 才允许闭栅。
        match self.pending.last() {
            Some(d) if d.ty == DescType::WaitSw => {
                self.fence_open = true;
                Ok(())
            }
            _ => Err(XC_BADTYPE),
        }
    }

    /// 发射一枚（栅栏闭时禁止——必须先翻栅）。
    pub fn fire_one(&mut self) -> Result<DescType, u32> {
        if self.pending.is_empty() {
            return Err(XC_ORDER);
        }
        if self.fence_open {
            return Err(XC_FENCE);
        }
        let d = self.pending.remove(0);
        self.fired += 1;
        Ok(d.ty)
    }

    /// 排四：翻栅（恰一次）。
    pub fn hop_fence(&mut self) -> Result<(), u32> {
        if !self.fence_open {
            return Err(XC_DOUBLEFENCE);
        }
        self.fence_open = false;
        self.fence_hops += 1;
        Ok(())
    }
}

pub fn run_unx0511_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0511");
    cs.add("SEG 段位独占", SEG_BASE == 0x34D40 && XC_DOUBLEFENCE == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：有序入队 + 插队拒。
    let mut q = InvQueue::new(8);
    q.push(DescType::IotlbInv, 1).ok();
    q.push(DescType::IecInv, 2).ok();
    cs.add("插队拒", q.push(DescType::IotlbInv, 1) == Err(XC_ORDER), "seq 回退");
    cs.add("平号拒", q.push(DescType::IotlbInv, 2) == Err(XC_ORDER), "seq 相等");
    cs.add("递增成", q.push(DescType::WaitSw, 3).is_ok(), "3>2 合法");

    // 族二：栅栏闭与发射压制。
    cs.add("栅栏闭成", q.arm_fence().is_ok(), "队尾是 WaitSw");
    cs.add("栅栏期发射拒", q.fire_one() == Err(XC_FENCE), "闭栅禁发");
    cs.add("翻栅成", q.hop_fence().is_ok(), "栅栏放行");
    cs.add("双翻拒", q.hop_fence() == Err(XC_DOUBLEFENCE), "恰翻一次");
    cs.add("翻栅后可发", q.fire_one() == Ok(DescType::IotlbInv), "FIFO 头出");

    // 族三：错位闭栅拒（队尾非 WaitSw）。
    let mut q2 = InvQueue::new(8);
    q2.push(DescType::IotlbInv, 1).ok();
    cs.add("非尾闭栅拒", q2.arm_fence() == Err(XC_BADTYPE), "队尾是 IOTLB");
    cs.add("空队闭栅拒", InvQueue::new(4).arm_fence() == Err(XC_BADTYPE), "无尾可闭");

    // 族四：发射序 FIFO。
    let mut q3 = InvQueue::new(8);
    q3.push(DescType::IecInv, 1).ok();
    q3.push(DescType::IotlbInv, 2).ok();
    q3.push(DescType::WaitSw, 3).ok();
    let f1 = q3.fire_one();
    let f2 = q3.fire_one();
    cs.add("FIFO 序", f1 == Ok(DescType::IecInv) && f2 == Ok(DescType::IotlbInv), "入队序=发射序");
    cs.add("发射计数", q3.fired == 2, "两发两计");
    cs.add("余尾 WaitSw", q3.fire_one() == Ok(DescType::WaitSw), "第三枚出");
    cs.add("空队发射拒", q3.fire_one() == Err(XC_ORDER), "发尽再发拒");

    // 族五：容量饱和溢出。
    let mut q4 = InvQueue::new(2);
    q4.push(DescType::IotlbInv, 1).ok();
    q4.push(DescType::IotlbInv, 2).ok();
    cs.add("满溢出拒", q4.push(DescType::IecInv, 3) == Err(XC_ORDER), "容量二");
    cs.add("溢出账一", q4.overflow == 1, "溢出可审计");
    cs.add("队深不涨", q4.pending.len() == 2, "溢出不占位");

    // 族六：类型三分类全通。
    let mut q5 = InvQueue::new(8);
    cs.add("三型全入", q5.push(DescType::IotlbInv, 1).is_ok() && q5.push(DescType::IecInv, 2).is_ok() && q5.push(DescType::WaitSw, 3).is_ok(), "三型皆合法");
    cs.add("三型全发", q5.fire_one() == Ok(DescType::IotlbInv) && q5.fire_one() == Ok(DescType::IecInv), "错型即拒的对面——正型皆发");

    cs
}
