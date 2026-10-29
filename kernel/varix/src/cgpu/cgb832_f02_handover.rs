//! cgb832_f02_handover — CGPU-F0832 F02 组收口与 F03 移交。
//!
//! 判据原文（验收行摘录）：F02 组收口与 F03 移交：收口清单（F02 各子项
//! 关闭态核验——未关闭项挡移交）；移交清单（F03 承接项逐条登记、双向签
//! 收）；边界冻结（移交窗口内 F02 侧禁止新变更——违约即拒）；交接账守恒
//! （清单项总数 = 已收口 + 待收口 + 已移交 + 待签收）。判据：收口核验、
//! 双向签收、边界冻结、守恒交接账。
//!
//! 诊断段 0x7280（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；状态机显式迁移；定容清单账；drill() 自例
//! 程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb832-v1";

/// 诊断段基（本单独占 0x7280）。
pub const SEG_BASE: u16 = 0x7280;

/// 清单项未关闭（F02 侧有待收口项即申请移交——挡移交）。
pub const XC_OPEN_ITEMS: u16 = SEG_BASE + 1;
/// 冻结期变更（移交窗口内 F02 侧新增/修改——边界违约）。
pub const XC_FROZEN_MUTATE: u16 = SEG_BASE + 2;
/// 重复签收（同一移交项第二次签收）。
pub const XC_DOUBLE_SIGN: u16 = SEG_BASE + 3;
/// 清单项不存在（操作未登记项）。
pub const XC_ITEM_MISS: u16 = SEG_BASE + 4;
/// 账满（定容清单耗尽）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 5;
/// 交接账漂移（守恒破坏）。
pub const XC_HANDOVER_DRIFT: u16 = SEG_BASE + 6;
/// 阶段错序（未开移交窗即签收/未收口即冻结）。
pub const XC_BAD_PHASE: u16 = SEG_BASE + 7;

/// 清单项定容。
pub const ITEM_CAP: usize = 24;

/// 清单项状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemSt {
    /// F02 侧待收口。
    Open,
    /// F02 侧已收口。
    Closed,
    /// 已列入移交、待 F03 签收。
    Offered,
    /// F03 已签收。
    Signed,
}

impl ItemSt {
    /// 是否收口态（Closed/Offered/Signed 均视为"关闭"——判据"未关闭项挡移交"）。
    pub fn settled(self) -> bool {
        !matches!(self, ItemSt::Open)
    }
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            ItemSt::Open => "open",
            ItemSt::Closed => "closed",
            ItemSt::Offered => "offered",
            ItemSt::Signed => "signed",
        }
    }
}

/// 清单项。
#[derive(Clone, Debug)]
pub struct Item {
    /// 项名。
    pub title: String,
    /// 归属组（0=F02，1=F03）。
    pub side: u8,
    /// 状态。
    pub st: ItemSt,
    /// 收口人代号（0=未收口）。
    pub closer: u8,
    /// 签收人代号（0=未签收）。
    pub signer: u8,
}

/// 交接阶段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 收口中：F02 侧清单管理。
    Closing,
    /// 冻结：F02 侧边界冻结（判据"边界冻结"）。
    Frozen,
    /// 移交中：F03 承接签收。
    Handover,
    /// 完成：交接收束。
    Done,
}

impl Phase {
    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Closing => "closing",
            Phase::Frozen => "frozen",
            Phase::Handover => "handover",
            Phase::Done => "done",
        }
    }
}

/// 交接账。
pub struct Handover {
    /// 账名。
    pub name: String,
    /// 阶段。
    phase: Phase,
    /// 清单项。
    items: Vec<Item>,
    /// F02 关闭数（守恒项）。
    pub closed_cnt: u64,
    /// 签收数（守恒项）。
    pub signed_cnt: u64,
    /// 冻结违约计数。
    pub frozen_hits: u64,
}

/// 守恒恒等式：items.len() == F02 收口数 + F02 待收口数 + 已移交待签收数 + 已签收数。
/// 即任一时刻四类计数之和恒等于清单总长，每次迁移后 verify() 复核。
impl Handover {
    /// 构造：Closing 态。
    pub fn new(name: &str) -> Self {
        Handover { name: name.to_string(), phase: Phase::Closing, items: Vec::new(), closed_cnt: 0, signed_cnt: 0, frozen_hits: 0 }
    }

    /// 阶段只读。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// F02 侧登记收口项（Closing 态可增；冻结后拒——判据"冻结期变更"）。
    pub fn add_item(&mut self, title: &str) -> Result<usize, u16> {
        if self.phase == Phase::Frozen || self.phase == Phase::Handover || self.phase == Phase::Done {
            self.frozen_hits += 1;
            return Err(XC_FROZEN_MUTATE);
        }
        if self.items.len() >= ITEM_CAP {
            return Err(XC_LEDGER_FULL);
        }
        self.items.push(Item { title: title.to_string(), side: 0, st: ItemSt::Open, closer: 0, signer: 0 });
        self.verify()?;
        Ok(self.items.len() - 1)
    }

    /// F03 侧登记承接项（Handover 态可增）。
    pub fn add_take_item(&mut self, title: &str) -> Result<usize, u16> {
        if self.phase != Phase::Handover {
            return Err(XC_BAD_PHASE);
        }
        if self.items.len() >= ITEM_CAP {
            return Err(XC_LEDGER_FULL);
        }
        self.items.push(Item { title: title.to_string(), side: 1, st: ItemSt::Signed, closer: 0, signer: 0 });
        self.signed_cnt += 1;
        self.verify()?;
        Ok(self.items.len() - 1)
    }

    /// 收口一项（Open→Closed）。
    pub fn close(&mut self, idx: usize, closer: u8) -> Result<(), u16> {
        if idx >= self.items.len() {
            return Err(XC_ITEM_MISS);
        }
        if self.items[idx].st != ItemSt::Open {
            return Err(XC_ITEM_MISS);
        }
        self.items[idx].st = ItemSt::Closed;
        self.items[idx].closer = closer;
        self.closed_cnt += 1;
        self.verify()?;
        Ok(())
    }

    /// 查询：F02 侧是否全部收口。
    pub fn all_closed(&self) -> Result<bool, u16> {
        for it in self.items.iter() {
            if it.side == 0 && !it.st.settled() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 冻结边界（Closing→Frozen，须全收口——判据"未关闭项挡移交"）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Closing {
            return Err(XC_BAD_PHASE);
        }
        if !self.all_closed()? {
            return Err(XC_OPEN_ITEMS);
        }
        self.phase = Phase::Frozen;
        Ok(())
    }

    /// 开移交窗（Frozen→Handover）。
    pub fn open_handover(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Frozen {
            return Err(XC_BAD_PHASE);
        }
        self.phase = Phase::Handover;
        Ok(())
    }

    /// 移交一项（Closed→Offered）。
    pub fn offer(&mut self, idx: usize) -> Result<(), u16> {
        if self.phase != Phase::Handover {
            return Err(XC_BAD_PHASE);
        }
        if idx >= self.items.len() {
            return Err(XC_ITEM_MISS);
        }
        if self.items[idx].st != ItemSt::Closed {
            return Err(XC_ITEM_MISS);
        }
        self.items[idx].st = ItemSt::Offered;
        self.verify()?;
        Ok(())
    }

    /// F03 签收（Offered→Signed，判据"双向签收"——offer 方与 sign 方两记）。
    pub fn sign(&mut self, idx: usize, signer: u8) -> Result<(), u16> {
        if self.phase != Phase::Handover {
            return Err(XC_BAD_PHASE);
        }
        if idx >= self.items.len() {
            return Err(XC_ITEM_MISS);
        }
        if self.items[idx].st != ItemSt::Offered {
            self.frozen_hits += 1;
            return Err(XC_DOUBLE_SIGN);
        }
        self.items[idx].st = ItemSt::Signed;
        self.items[idx].signer = signer;
        self.signed_cnt += 1;
        self.verify()?;
        Ok(())
    }

    /// 收束（Handover→Done：全部已签收）。
    pub fn finish(&mut self) -> Result<(), u16> {
        if self.phase != Phase::Handover {
            return Err(XC_BAD_PHASE);
        }
        for it in self.items.iter() {
            if it.side == 0 && it.st != ItemSt::Signed {
                return Err(XC_OPEN_ITEMS);
            }
        }
        self.phase = Phase::Done;
        Ok(())
    }

    /// 各状态计数。
    pub fn count_of(&self, st: ItemSt) -> u32 {
        let mut n = 0;
        for it in self.items.iter() {
            if it.st == st {
                n += 1;
            }
        }
        n
    }

    /// 守恒校验（判据"交接账守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        let open = self.count_of(ItemSt::Open) as u64;
        let closed = self.count_of(ItemSt::Closed) as u64;
        let offered = self.count_of(ItemSt::Offered) as u64;
        let signed = self.count_of(ItemSt::Signed) as u64;
        if open + closed + offered + signed != self.items.len() as u64 {
            return Err(XC_HANDOVER_DRIFT);
        }
        if closed != self.closed_cnt {
            return Err(XC_HANDOVER_DRIFT);
        }
        if signed != self.signed_cnt {
            return Err(XC_HANDOVER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb832[");
        s.push_str(&self.name);
        s.push_str("] phase=");
        s.push_str(self.phase.name());
        s.push_str(" items=");
        s.push_str(&self.items.len().to_string());
        s.push_str(" closed=");
        s.push_str(&self.closed_cnt.to_string());
        s.push_str(" signed=");
        s.push_str(&self.signed_cnt.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut h = Handover::new("w3-drill-832");
    if h.phase() != Phase::Closing {
        return Err(XC_BAD_PHASE);
    }

    // F02 登记四项。
    let i0 = h.add_item("共享内存收口")?;
    let i1 = h.add_item("媒体联动收口")?;
    let i2 = h.add_item("功耗画像收口")?;
    let i3 = h.add_item("遥测通道收口")?;
    if h.items.len() != 4 {
        return Err(XC_LEDGER_FULL);
    }

    // 负样本 1：未全收口即冻结——挡移交。
    h.close(i0, 1)?;
    h.close(i1, 1)?;
    match h.freeze() {
        Err(code) if code == XC_OPEN_ITEMS => {}
        Ok(_) => return Err(XC_OPEN_ITEMS),
        Err(other) => return Err(other),
    }
    // 收口剩余项。
    h.close(i2, 2)?;
    h.close(i3, 2)?;
    if !h.all_closed()? {
        return Err(XC_OPEN_ITEMS);
    }
    h.freeze()?;
    if h.phase() != Phase::Frozen {
        return Err(XC_BAD_PHASE);
    }

    // 负样本 2：冻结期新增——边界违约。
    match h.add_item("迟到的项") {
        Err(code) if code == XC_FROZEN_MUTATE => {}
        Ok(_) => return Err(XC_FROZEN_MUTATE),
        Err(other) => return Err(other),
    }
    if h.frozen_hits != 1 {
        return Err(XC_FROZEN_MUTATE);
    }

    h.open_handover()?;
    // 负样本 3：Handover 态再冻结——阶段错序。
    match h.freeze() {
        Err(code) if code == XC_BAD_PHASE => {}
        Ok(_) => return Err(XC_BAD_PHASE),
        Err(other) => return Err(other),
    }
    // 负样本 4：Closed 项直接签收——须先 offer。
    match h.sign(i0, 7) {
        Err(code) if code == XC_DOUBLE_SIGN => {}
        Ok(_) => return Err(XC_DOUBLE_SIGN),
        Err(other) => return Err(other),
    }
    // 正样本：offer 两项并签收。
    h.offer(i0)?;
    h.offer(i1)?;
    h.sign(i0, 7)?;
    h.sign(i1, 7)?;
    // 负样本 5：重复签收已签项。
    match h.sign(i0, 7) {
        Err(code) if code == XC_DOUBLE_SIGN => {}
        Ok(_) => return Err(XC_DOUBLE_SIGN),
        Err(other) => return Err(other),
    }
    // F03 登记承接项。
    h.add_take_item("承接：内存池运维")?;
    h.add_take_item("承接：媒体回归")?;
    // 剩余项也走 offer/sign。
    h.offer(i2)?;
    h.sign(i2, 8)?;
    h.offer(i3)?;
    h.sign(i3, 8)?;
    h.verify()?;

    // 负样本 6：收束前校验（此时 F02 侧全 Signed，可收束）。
    h.finish()?;
    if h.phase() != Phase::Done {
        return Err(XC_BAD_PHASE);
    }
    // 负样本 7：Done 后再签收——阶段错序。
    match h.sign(i0, 9) {
        Err(code) if code == XC_BAD_PHASE => {}
        Ok(_) => return Err(XC_BAD_PHASE),
        Err(other) => return Err(other),
    }
    // 守恒终检。
    h.verify()?;
    if h.closed_cnt != 4 || h.signed_cnt != 6 {
        return Err(XC_HANDOVER_DRIFT);
    }
    Ok(0)
}
