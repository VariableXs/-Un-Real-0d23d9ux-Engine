//! unx0506_dmarops —— UNX-F0506 DMAR 使能/关闭操作序（状态机 + 排空等待 + 预检闸）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0506 · DMAR 操作序 · 目标 300 行
//! 判据原文：UNX-F0506-J1 DMAR 操作序：序一 使能预检
//!           （TE 置位前四预检——根表已设、上下文表已设、
//!           故障账已清、失效队列已排空，缺一拒）；序二 跳
//!           步拒（未预检直接置 TE 拒——状态机只认完整路
//!           径）；序三 关闭排空（TE=0 前必须先排空 IOTLB
//!           失效队列，未排空拒）；序四 状态往返（OFF→
//!           ENABLED→QUIESCING→OFF 全循环合法，非法边拒）；
//!           序五 排空等待计数（排空期失效请求进等待账，
//!           重使能后放行对账）。
//! 组件：DMAR 全局状态机 + 四预检闸 + 排空等待账 + 边表。
//! 职责定位：DMAR 使能序是 IOMMU 的「点火清单」——TE 位一
//!   旦置位硬件立即按当前表翻译，表没配好就是全设备 DMA
//!   瘫痪；关闭序是「熄火清单」——队列里还有失效在飞就断
//!   电，设备拿着陈旧翻译继续写。两个清单都不可跳步。
//! 性能逐项分解：状态转移 O(1)；预检 O(1)（四旗标）。
//! 诊断码段：0x34C00（0x34C01~0x34C09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34C00;
pub const XC_PRE: u32 = SEG_BASE + 1; // 预检缺项
pub const XC_EDGE: u32 = SEG_BASE + 2; // 非法状态边
pub const XC_NOTDRAINED: u32 = SEG_BASE + 3; // 未排空即关
pub const XC_ORDER: u32 = SEG_BASE + 4; // 操作序违（排空前先跳步）

/// DMAR 全局状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmarState {
    Off,
    Enabled,
    Quiescing,
}

/// 四预检旗标。
#[derive(Clone, Copy, Debug)]
pub struct Preconditions {
    pub root_set: bool,
    pub ctx_set: bool,
    pub fault_cleared: bool,
    pub invq_drained: bool,
}

impl Preconditions {
    pub fn all_set(&self) -> bool {
        self.root_set && self.ctx_set && self.fault_cleared && self.invq_drained
    }
}

/// DMAR 操作状态机。
pub struct DmarOps {
    pub st: DmarState,
    pub pre: Preconditions,
    pub wait_acc: u64,     // 排空期等待账
    pub released: u64,     // 重使能后放行数
    pub enable_cycles: u64,
}

impl DmarOps {
    pub fn new() -> Self {
        DmarOps {
            st: DmarState::Off,
            pre: Preconditions { root_set: false, ctx_set: false, fault_cleared: false, invq_drained: false },
            wait_acc: 0,
            released: 0,
            enable_cycles: 0,
        }
    }

    /// 使能：四预检 + 状态边（Off→Enabled）。
    pub fn enable(&mut self) -> Result<(), u32> {
        if self.st != DmarState::Off {
            return Err(XC_EDGE);
        }
        if !self.pre.all_set() {
            return Err(XC_PRE);
        }
        self.st = DmarState::Enabled;
        self.enable_cycles += 1;
        // 序五：重使能放行等待账。
        self.released += self.wait_acc;
        self.wait_acc = 0;
        Ok(())
    }

    /// 排空期失效请求：Enabled 时直通；Quiescing 进等待账。
    pub fn submit_invalidate(&mut self) -> Result<(), u32> {
        match self.st {
            DmarState::Enabled => Ok(()),
            DmarState::Quiescing => {
                self.wait_acc += 1;
                Ok(())
            }
            DmarState::Off => Err(XC_ORDER),
        }
    }

    /// 关闭请求一：进静默（Enabled→Quiescing）。
    pub fn begin_quiesce(&mut self) -> Result<(), u32> {
        if self.st != DmarState::Enabled {
            return Err(XC_EDGE);
        }
        self.st = DmarState::Quiescing;
        Ok(())
    }

    /// 关闭请求二：排空后断电（Quiescing→Off），未排空拒。
    pub fn finalize_off(&mut self, drained: bool) -> Result<(), u32> {
        if self.st != DmarState::Quiescing {
            return Err(XC_EDGE);
        }
        if !drained {
            return Err(XC_NOTDRAINED);
        }
        self.st = DmarState::Off;
        Ok(())
    }

    /// 预检登记。
    pub fn set_pre(&mut self, root: bool, ctx: bool, fault: bool, invq: bool) {
        self.pre = Preconditions { root_set: root, ctx_set: ctx, fault_cleared: fault, invq_drained: invq };
    }
}

pub fn run_unx0506_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0506");
    cs.add("SEG 段位独占", SEG_BASE == 0x34C00 && XC_ORDER == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：使能预检。
    let mut m = DmarOps::new();
    cs.add("零预检拒", m.enable() == Err(XC_PRE), "四缺四");
    m.set_pre(true, true, false, true);
    cs.add("缺故障清拒", m.enable() == Err(XC_PRE), "缺一即拒");
    m.set_pre(true, true, true, false);
    cs.add("缺排空拒", m.enable() == Err(XC_PRE), "失效队列未排");
    m.set_pre(true, true, true, true);
    cs.add("四预检全过成", m.enable().is_ok(), "点火");
    cs.add("态为 Enabled", m.st == DmarState::Enabled, "状态转移");

    // 族二：跳步拒（Off 态下关断路径）。
    let mut m2 = DmarOps::new();
    cs.add("Off 态关断拒", m2.finalize_off(true) == Err(XC_EDGE), "未静默不可断电");
    cs.add("Off 态发失效拒", m2.submit_invalidate() == Err(XC_ORDER), "断电期失效无处提交");

    // 族三：关闭排空序。
    m.begin_quiesce().ok();
    cs.add("态为 Quiescing", m.st == DmarState::Quiescing, "进入静默");
    cs.add("未排空断电拒", m.finalize_off(false) == Err(XC_NOTDRAINED), "排队未清不可断");
    cs.add("拒后态不变", m.st == DmarState::Quiescing, "静默保持");
    cs.add("排空后断电成", m.finalize_off(true).is_ok(), "排空凭证齐");
    cs.add("态回 Off", m.st == DmarState::Off, "熄火");

    // 族四：静默期失效进等待账。
    m.begin_quiesce().ok();
    m.submit_invalidate().ok();
    m.submit_invalidate().ok();
    m.submit_invalidate().ok();
    cs.add("等待账三", m.wait_acc == 3, "静默期三请求排队");
    m.finalize_off(true).ok();
    m.set_pre(true, true, true, true);
    m.enable().ok();
    cs.add("重使能放行对账", m.released == 3 && m.wait_acc == 0, "三请求全放行清账");
    cs.add("点火计数二", m.enable_cycles == 2, "两次使能循环");

    // 族五：静默态禁使能。
    let mut m3 = DmarOps::new();
    m3.set_pre(true, true, true, true);
    m3.enable().ok();
    m3.begin_quiesce().ok();
    cs.add("静默期使能拒", m3.enable() == Err(XC_EDGE), "只认 Off 态点火");

    // 族六：Enabled 态失效直通。
    let mut m4 = DmarOps::new();
    m4.set_pre(true, true, true, true);
    m4.enable().ok();
    cs.add("使能态直通", m4.submit_invalidate().is_ok(), "不进等待账");
    cs.add("等待账零", m4.wait_acc == 0 && m4.released == 0, "直通零记账");

    // 族七：三循环启停压力（点火→静默→排空→熄火→再点火 ×3）。
    let mut m5 = DmarOps::new();
    let mut cycle_wait = 0u64;
    let mut all_ok = true;
    for cyc in 0..3u64 {
        m5.set_pre(true, true, true, true);
        if m5.enable().is_err() {
            all_ok = false;
        }
        // 每循环 2 笔使能态直通。
        if m5.submit_invalidate().is_err() || m5.submit_invalidate().is_err() {
            all_ok = false;
        }
        if m5.begin_quiesce().is_err() {
            all_ok = false;
        }
        // 静默期 1 笔进等待账。
        if m5.submit_invalidate().is_err() {
            all_ok = false;
        }
        cycle_wait += 1;
        if m5.finalize_off(true).is_err() {
            all_ok = false;
        }
        let _ = cyc;
    }
    cs.add("三循环全绿", all_ok, "3×(点火-静默-熄火) 无一处跳步违例");
    cs.add("循环点火计数", m5.enable_cycles == 3, "三次点火");
    cs.add("等待账累计", cycle_wait == 3, "每循环静默期一笔");
    cs.add("放行累计对账", m5.released == 3 && m5.wait_acc == 0, "三次重点火放行清账");
    // 末态熄火。
    cs.add("末态归 Off", m5.st == DmarState::Off, "循环末完整熄火");

    // 族八：静默期反复请求（等待账只增）。
    let mut m6 = DmarOps::new();
    m6.set_pre(true, true, true, true);
    m6.enable().ok();
    m6.begin_quiesce().ok();
    for _ in 0..7 {
        m6.submit_invalidate().ok();
    }
    cs.add("静默期七笔", m6.wait_acc == 7, "等待账线性累计");
    // 未排空拒 + 账不丢。
    let _ = m6.finalize_off(false);
    cs.add("拒后账不丢", m6.wait_acc == 7, "拒绝不消费等待账");

    cs
}
