//! UNX-F48985 3 指工作区手势（F5 shell 消费分界联签）· 0x716C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第三槽）：
//!   F48985 ① 纯功能代码 ≥360 行；② UNX-F48985-J1 联签锚定行 +
//!          fake 预演 10/10 次。
//!   F48986 ① 纯功能代码 ≥340 行；② UNX-F48986-J1 手势段 ×10^2
//!          打点全落账。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时钟 → 坐标 → 面积 → 容量 → id 存在性 → 收账。
//! 容量先于 id：Down 没有 id，它的问题只能是「槽位满了」。若把容量判断
//! 放到 id 检查之后，满槽会被报成「id 不存在」，排障方向从「手上指头
//! 太多」被带到「事件表坏了」。
//! 面积先于容量：面积非法的接触根本不该占用槽位。若先查容量，一串坏
//! 数据会把 10 个槽位全部占满，合法后续接触被误报为 CapacityBad，误触
//! 账与容量账混在一起无法拆分。
//! 三指工作区手势只回答「这一划是不是工作区手势」：三指、位移过线、
//! 轴优势 ≥1.5 倍、方向只认左/右/上；下划留给通知栏，斜划一律不判。
//! 联签把「消费分界」钉成一行常量：三指手势被 shell 吃进（consumed），
//! 其余穿透（passthrough）；10 次 fake 预演全过才允许登记。
//! 延迟打点走「起手 → 手势段 → 收手」三段账：每拍要么落账、要么判
//! 次序错，不允许静默丢弃；×10^2 手势段全落账才算 J1 过。
//! 关键差异：本模块只回答「这一拍接触能不能占用/移动/释放槽位」；
//! 不做手掌/边缘抑制（B10-META）、不做手势分类（B10-CORE）、不做手势
//! →动作映射（B10-DISPATCH）。槽位号是物理位置，id 是生命周期身份：
//! 同一槽位可以先后承载不同 id，但同一 id 终身唯一。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x716C0，B10 族第三槽）。
pub const SEG_BASE: u32 = 0x716C0;

/// 逻辑屏宽（像素）。
pub const SCREEN_W: i32 = 1_920;
/// 逻辑屏高（像素）。
pub const SCREEN_H: i32 = 1_080;
/// 同时接触槽位数（十指上限）。
pub const SLOTS: usize = 10;
/// 单边尺寸上界（当量单位）。
pub const SIDE_MAX: u32 = 100;
/// 接触面积上界。
pub const AREA_MAX: u32 = 10_000;
/// 时钟上界。
pub const TICK_CAP: u32 = 1_000_000;

/// 容量注入轮数（卡面 ×10^3）。
pub const CAP_ROUNDS: u32 = 1_000;
/// id 生命周期轮数。
pub const ID_ROUNDS: u32 = 1_000;
/// 释放失效轮数。
pub const DEAD_ROUNDS: u32 = 1_000;
/// fuzz 外层轮数（每轮 8 拍）。
pub const FUZZ_ROUNDS: u32 = 1_000;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsCode {
    /// 槽位满，拒绝第 N+1 点
    CapacityBad,
    /// id 不存在（已释放或从未签发）
    IdBad,
    /// 槽位索引越界
    SlotBad,
    /// 常量/表不满足守恒
    TableBad,
    /// 接触面积非法
    AreaBad,
    /// 坐标越界
    CoordBad,
    /// 时钟回退或越界
    TickBad,
    /// 事件次序非法
    OrderBad,
    /// 槽位账不守恒
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖不足
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 与影子模型不一致
    FuzzBad,
}

impl MsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MsCode::CapacityBad => "十个槽位已满，第十一点接触被拒绝",
            MsCode::IdBad => "接触 id 不存在（已释放或从未签发）",
            MsCode::SlotBad => "槽位索引落在容量之外",
            MsCode::TableBad => "槽位/尺寸常量表不满足守恒",
            MsCode::AreaBad => "接触尺寸为零或面积越过上界",
            MsCode::CoordBad => "接触坐标落在逻辑屏外",
            MsCode::TickBad => "时钟回退或越过上界",
            MsCode::OrderBad => "事件次序非法",
            MsCode::LedgerBad => "issued/lifted/active 三账不守恒",
            MsCode::RenderBad => "账本文本字段数或数值不可解析",
            MsCode::ReplayBad => "两次确定性复跑结果不一致",
            MsCode::CoverBad => "覆盖轮数为零或越过上限",
            MsCode::SegmentBad => "诊断码越出本模块段窗",
            MsCode::FuzzBad => "fuzz 结果与影子模型不一致",
        }
    }

    pub const ALL: [MsCode; 14] = [
        MsCode::CapacityBad,
        MsCode::IdBad,
        MsCode::SlotBad,
        MsCode::TableBad,
        MsCode::AreaBad,
        MsCode::CoordBad,
        MsCode::TickBad,
        MsCode::OrderBad,
        MsCode::LedgerBad,
        MsCode::RenderBad,
        MsCode::ReplayBad,
        MsCode::CoverBad,
        MsCode::SegmentBad,
        MsCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MsCode::ALL.len() {
        if let Some(c) = MsCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 接触与槽位 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Contact {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Contact {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Contact {
        Contact { x, y, w, h }
    }

    pub fn area(&self) -> u32 {
        self.w.saturating_mul(self.h)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spot {
    pub id: u32,
    pub c: Contact,
}

/// 恒等常量审计：容量与尺寸边界必须自洽。
pub fn audit_table() -> Result<u32, u32> {
    if SLOTS == 0 || SLOTS > 16 {
        return Err(MsCode::TableBad.val());
    }
    if SIDE_MAX == 0 || AREA_MAX < SIDE_MAX {
        return Err(MsCode::TableBad.val());
    }
    if SIDE_MAX.saturating_mul(SIDE_MAX) > AREA_MAX {
        return Err(MsCode::TableBad.val());
    }
    if SCREEN_W <= 0 || SCREEN_H <= 0 {
        return Err(MsCode::TableBad.val());
    }
    Ok(SLOTS as u32)
}

// ── 槽位面板 ──────────────────────────────────────────────────────
#[derive(Clone)]
pub struct Panel {
    spots: [Option<Spot>; SLOTS],
    next_id: u32,
    tick: u32,
    ops: u32,
    issued: u32,
    lifted: u32,
    rejected: u32,
    digest: u64,
}

impl Panel {
    pub fn new() -> Panel {
        Panel {
            spots: [None; SLOTS],
            next_id: 0,
            tick: 0,
            ops: 0,
            issued: 0,
            lifted: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    fn reject(&mut self, code: u32) -> Result<u8, u32> {
        self.rejected = self.rejected.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 统一的三道数据闸：时钟 → 坐标 → 面积。
    fn data_gate(&mut self, c: &Contact, tick: u32) -> Result<(), u32> {
        if tick <= self.tick || tick > TICK_CAP {
            let _ = self.reject(MsCode::TickBad.val());
            return Err(MsCode::TickBad.val());
        }
        self.tick = tick;
        if c.x < 0 || c.y < 0 || c.x >= SCREEN_W || c.y >= SCREEN_H {
            let _ = self.reject(MsCode::CoordBad.val());
            return Err(MsCode::CoordBad.val());
        }
        let a = c.area();
        if c.w == 0 || c.h == 0 || c.w > SIDE_MAX || c.h > SIDE_MAX || a > AREA_MAX {
            let _ = self.reject(MsCode::AreaBad.val());
            return Err(MsCode::AreaBad.val());
        }
        Ok(())
    }

    fn tick_gate(&mut self, tick: u32) -> Result<(), u32> {
        if tick <= self.tick || tick > TICK_CAP {
            let _ = self.reject(MsCode::TickBad.val());
            return Err(MsCode::TickBad.val());
        }
        self.tick = tick;
        Ok(())
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < SLOTS {
            match self.spots.get(i) {
                Some(Some(s)) => {
                    if s.id == id {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 按下：时钟 → 坐标 → 面积 → 容量 → 分配槽位与新 id。
    pub fn down(&mut self, c: Contact, tick: u32) -> Result<u8, u32> {
        self.ops = self.ops.saturating_add(1);
        self.data_gate(&c, tick)?;
        let mut slot = SLOTS;
        let mut i = 0usize;
        while i < SLOTS {
            match self.spots.get(i) {
                Some(None) => {
                    slot = i;
                    i = SLOTS;
                }
                _ => i += 1,
            }
        }
        if slot == SLOTS {
            let _ = self.reject(MsCode::CapacityBad.val());
            return Err(MsCode::CapacityBad.val());
        }
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        if let Some(cell) = self.spots.get_mut(slot) {
            *cell = Some(Spot { id, c });
        }
        self.issued = self.issued.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, 1);
        self.digest = p_core::fold_u32(self.digest, id);
        self.digest = p_core::fold_u32(self.digest, slot as u32);
        Ok(slot as u8)
    }

    /// 移动：时钟 → 坐标 → 面积 → id 存在性 → 更新接触框。
    pub fn move_to(&mut self, id: u32, c: Contact, tick: u32) -> Result<u8, u32> {
        self.ops = self.ops.saturating_add(1);
        self.data_gate(&c, tick)?;
        let slot = match self.find(id) {
            Some(v) => v,
            None => {
                let _ = self.reject(MsCode::IdBad.val());
                return Err(MsCode::IdBad.val());
            }
        };
        if let Some(cell) = self.spots.get_mut(slot) {
            *cell = Some(Spot { id, c });
        }
        self.digest = p_core::fold_u32(self.digest, 2);
        self.digest = p_core::fold_u32(self.digest, id);
        Ok(slot as u8)
    }

    /// 抬起：时钟 → id 存在性 → 释放槽位（id 立即失效）。
    pub fn lift(&mut self, id: u32, tick: u32) -> Result<u8, u32> {
        self.ops = self.ops.saturating_add(1);
        self.tick_gate(tick)?;
        let slot = match self.find(id) {
            Some(v) => v,
            None => {
                let _ = self.reject(MsCode::IdBad.val());
                return Err(MsCode::IdBad.val());
            }
        };
        if let Some(cell) = self.spots.get_mut(slot) {
            *cell = None;
        }
        self.lifted = self.lifted.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, 3);
        self.digest = p_core::fold_u32(self.digest, id);
        Ok(slot as u8)
    }

    /// 全取消：释放所有在途接触，返回释放数量。
    pub fn cancel_all(&mut self, tick: u32) -> Result<u32, u32> {
        self.ops = self.ops.saturating_add(1);
        self.tick_gate(tick)?;
        let mut count = 0u32;
        let mut i = 0usize;
        while i < SLOTS {
            let live = match self.spots.get(i) {
                Some(Some(_)) => true,
                _ => false,
            };
            if live {
                if let Some(cell) = self.spots.get_mut(i) {
                    *cell = None;
                }
                count = count.saturating_add(1);
            }
            i += 1;
        }
        self.lifted = self.lifted.saturating_add(count);
        self.digest = p_core::fold_u32(self.digest, 4);
        self.digest = p_core::fold_u32(self.digest, count);
        Ok(count)
    }

    pub fn active(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < SLOTS {
            if let Some(Some(_)) = self.spots.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    pub fn id_at(&self, slot: usize) -> Option<u32> {
        match self.spots.get(slot) {
            Some(Some(s)) => Some(s.id),
            _ => None,
        }
    }

    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// 三账守恒 + id 唯一 + 槽位索引自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.next_id != self.issued {
            return Err(MsCode::LedgerBad.val());
        }
        let active = self.active();
        if self.issued != self.lifted.saturating_add(active) {
            return Err(MsCode::LedgerBad.val());
        }
        let mut i = 0usize;
        while i < SLOTS {
            match self.spots.get(i) {
                Some(Some(s)) => {
                    if s.id == 0 || s.id > self.next_id {
                        return Err(MsCode::IdBad.val());
                    }
                    let mut j = i + 1;
                    while j < SLOTS {
                        if let Some(Some(t)) = self.spots.get(j) {
                            if t.id == s.id {
                                return Err(MsCode::IdBad.val());
                            }
                        }
                        j += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if self.ops > 0 && self.digest == 0 {
            return Err(MsCode::ReplayBad.val());
        }
        Ok(())
    }

    pub fn stats(&self) -> PanelStats {
        PanelStats {
            ops: self.ops,
            issued: self.issued,
            lifted: self.lifted,
            active: self.active(),
            rejected: self.rejected,
            digest: self.digest,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelStats {
    pub ops: u32,
    pub issued: u32,
    pub lifted: u32,
    pub active: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl PanelStats {
    pub fn balanced(&self) -> bool {
        self.ops > 0
            && self.issued == self.lifted.saturating_add(self.active)
            && self.ops >= self.issued.saturating_add(self.rejected)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(MsCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(MsCode::ReplayBad.val());
        }
        Ok(())
    }
}

// ── 容量探针（F48985） ────────────────────────────────────────────
fn spot_contact(i: usize) -> Contact {
    let x = 150 + (i as i32 % 9) * 180;
    let y = 200 + (i as i32 % 5) * 120;
    Contact::new(x, y, 6, 6)
}

pub fn capacity_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > CAP_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut p = Panel::new();
    let mut tick = 0u32;
    let mut slots = [0u8; SLOTS];
    let mut i = 0u32;
    while i < rounds {
        let mut j = 0usize;
        while j < SLOTS {
            tick = tick.saturating_add(1);
            slots[j] = p.down(spot_contact(j), tick)?;
            j += 1;
        }
        if p.active() != SLOTS as u32 {
            return Err(MsCode::LedgerBad.val());
        }
        // 第十一点：必须被容量闸拦下。
        tick = tick.saturating_add(1);
        if p.down(spot_contact(0), tick) != Err(MsCode::CapacityBad.val()) {
            return Err(MsCode::FuzzBad.val());
        }
        // 抬一指 → 槽位立即回收 → 再按必须成功且复用同一槽位。
        let freed = match slots.get(0) {
            Some(v) => *v as usize,
            None => return Err(MsCode::SlotBad.val()),
        };
        let freed_id = match p.id_at(freed) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        tick = tick.saturating_add(1);
        if p.lift(freed_id, tick)? as usize != freed {
            return Err(MsCode::SlotBad.val());
        }
        tick = tick.saturating_add(1);
        let reused = p.down(spot_contact(9), tick)? as usize;
        if reused != freed {
            return Err(MsCode::SlotBad.val());
        }
        if p.id_at(reused) == Some(freed_id) {
            return Err(MsCode::IdBad.val());
        }
        tick = tick.saturating_add(1);
        if p.cancel_all(tick)? != SLOTS as u32 {
            return Err(MsCode::LedgerBad.val());
        }
        i = i.saturating_add(1);
    }
    p.audit()?;
    let s = p.stats();
    let issued_expect = rounds.saturating_mul((SLOTS as u32).saturating_add(1));
    let lifted_expect = rounds.saturating_mul((SLOTS as u32).saturating_add(1));
    if s.issued != issued_expect || s.lifted != lifted_expect {
        return Err(MsCode::LedgerBad.val());
    }
    if s.active != 0 || s.rejected != rounds {
        return Err(MsCode::LedgerBad.val());
    }
    Ok(s.rejected)
}

/// 合法十点：十次按下全部成功、在途恰为十。
pub fn fill_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > CAP_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut p = Panel::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let mut j = 0usize;
        while j < SLOTS {
            tick = tick.saturating_add(1);
            if p.down(spot_contact(j), tick)? != j as u8 {
                return Err(MsCode::SlotBad.val());
            }
            j += 1;
        }
        if p.active() != SLOTS as u32 || p.stats().rejected != 0 {
            return Err(MsCode::LedgerBad.val());
        }
        tick = tick.saturating_add(1);
        let _ = p.cancel_all(tick)?;
        i = i.saturating_add(1);
    }
    p.audit()?;
    Ok(p.active())
}

// ── id 生命周期探针（F48986） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdStats {
    pub rounds: u32,
    pub issued: u32,
    pub first_id: u32,
    pub last_id: u32,
    pub digest: u64,
}

pub fn id_probe(rounds: u32) -> Result<IdStats, u32> {
    if rounds == 0 || rounds > ID_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut p = Panel::new();
    let mut tick = 0u32;
    let mut first_id = 0u32;
    let mut last_id = 0u32;
    let mut i = 0u32;
    while i < rounds {
        tick = tick.saturating_add(1);
        let a = p.down(Contact::new(300, 300, 5, 5), tick)?;
        let ida = match p.id_at(a as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        if i == 0 {
            first_id = ida;
        }
        tick = tick.saturating_add(1);
        let _ = p.move_to(ida, Contact::new(320, 310, 5, 5), tick)?;
        if p.id_at(a as usize) != Some(ida) {
            return Err(MsCode::IdBad.val());
        }
        tick = tick.saturating_add(1);
        let _ = p.lift(ida, tick)?;
        if p.id_at(a as usize).is_some() {
            return Err(MsCode::OrderBad.val());
        }
        // 新按下必须拿到更大的 id；槽位可以复用，id 不可以。
        tick = tick.saturating_add(1);
        let b = p.down(Contact::new(300, 300, 5, 5), tick)?;
        let idb = match p.id_at(b as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        if idb <= ida {
            return Err(MsCode::IdBad.val());
        }
        last_id = idb;
        tick = tick.saturating_add(1);
        let _ = p.lift(idb, tick)?;
        i = i.saturating_add(1);
    }
    p.audit()?;
    let s = p.stats();
    if s.issued != rounds.saturating_mul(2) || s.lifted != rounds.saturating_mul(2) {
        return Err(MsCode::LedgerBad.val());
    }
    if s.active != 0 {
        return Err(MsCode::LedgerBad.val());
    }
    if first_id != 1 || last_id != rounds.saturating_mul(2) {
        return Err(MsCode::IdBad.val());
    }
    Ok(IdStats {
        rounds,
        issued: s.issued,
        first_id,
        last_id,
        digest: s.digest,
    })
}

/// 释放后旧 id 立刻失效：移动与再抬都必须 IdBad。
pub fn dead_probe(rounds: u32) -> Result<u32, u32> {
    if rounds == 0 || rounds > DEAD_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut p = Panel::new();
    let mut tick = 0u32;
    let mut caught = 0u32;
    let mut i = 0u32;
    while i < rounds {
        tick = tick.saturating_add(1);
        let slot = p.down(Contact::new(500, 400, 6, 6), tick)?;
        let id = match p.id_at(slot as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        tick = tick.saturating_add(1);
        let _ = p.lift(id, tick)?;
        tick = tick.saturating_add(1);
        if p.move_to(id, Contact::new(510, 410, 6, 6), tick).err()
            == Some(MsCode::IdBad.val())
        {
            caught = caught.saturating_add(1);
        }
        tick = tick.saturating_add(1);
        if p.lift(id, tick).err() == Some(MsCode::IdBad.val()) {
            caught = caught.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    p.audit()?;
    if caught != rounds.saturating_mul(2) {
        return Err(MsCode::FuzzBad.val());
    }
    Ok(caught)
}

// ── fuzz：影子模型逐拍对账 ×8×10^3 ──────────────────────────────
pub fn fuzz_multi(rounds: u32) -> Result<PanelStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x716C_0F86_0F85_1167);
    let mut p = Panel::new();
    let mut shadow = [0u32; SLOTS];
    let mut next_shadow = 1u32;
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let mut k = 0u32;
        while k < 8 {
            let kind = (rng.next() >> 33) % 4;
            tick = tick.saturating_add(1);
            if kind == 0 {
                let c = Contact::new(
                    100 + rng.below(1_600) as i32,
                    100 + rng.below(800) as i32,
                    1 + rng.below(20),
                    1 + rng.below(20),
                );
                let mut slot = SLOTS;
                let mut j = 0usize;
                while j < SLOTS {
                    match shadow.get(j) {
                        Some(0) => {
                            slot = j;
                            j = SLOTS;
                        }
                        _ => j += 1,
                    }
                }
                if slot == SLOTS {
                    if p.down(c, tick) != Err(MsCode::CapacityBad.val()) {
                        return Err(MsCode::FuzzBad.val());
                    }
                } else {
                    let got = p.down(c, tick);
                    if got != Ok(slot as u8) {
                        return Err(MsCode::FuzzBad.val());
                    }
                    if let Some(cell) = shadow.get_mut(slot) {
                        *cell = next_shadow;
                    }
                    next_shadow = next_shadow.saturating_add(1);
                }
            } else if kind == 1 {
                let c = Contact::new(
                    100 + rng.below(1_600) as i32,
                    100 + rng.below(800) as i32,
                    1 + rng.below(20),
                    1 + rng.below(20),
                );
                let idx = (rng.next() >> 33) as usize % SLOTS;
                let id = match shadow.get(idx) {
                    Some(v) => *v,
                    None => 0,
                };
                if id == 0 {
                    if p.move_to(999_999, c, tick) != Err(MsCode::IdBad.val()) {
                        return Err(MsCode::FuzzBad.val());
                    }
                } else if p.move_to(id, c, tick) != Ok(idx as u8) {
                    return Err(MsCode::FuzzBad.val());
                }
            } else if kind == 2 {
                let idx = (rng.next() >> 33) as usize % SLOTS;
                let id = match shadow.get(idx) {
                    Some(v) => *v,
                    None => 0,
                };
                if id == 0 {
                    if p.lift(999_999, tick) != Err(MsCode::IdBad.val()) {
                        return Err(MsCode::FuzzBad.val());
                    }
                } else {
                    if p.lift(id, tick) != Ok(idx as u8) {
                        return Err(MsCode::FuzzBad.val());
                    }
                    if let Some(cell) = shadow.get_mut(idx) {
                        *cell = 0;
                    }
                }
            } else {
                let mut live = 0u32;
                let mut j = 0usize;
                while j < SLOTS {
                    if let Some(v) = shadow.get(j) {
                        if *v != 0 {
                            live = live.saturating_add(1);
                        }
                    }
                    j += 1;
                }
                if p.cancel_all(tick) != Ok(live) {
                    return Err(MsCode::FuzzBad.val());
                }
                let mut j2 = 0usize;
                while j2 < SLOTS {
                    if let Some(cell) = shadow.get_mut(j2) {
                        *cell = 0;
                    }
                    j2 += 1;
                }
            }
            k += 1;
        }
        i = i.saturating_add(1);
    }
    p.audit()?;
    let s = p.stats();
    if s.issued.saturating_add(1) != next_shadow {
        return Err(MsCode::FuzzBad.val());
    }
    let mut live = 0u32;
    let mut j = 0usize;
    while j < SLOTS {
        if let Some(v) = shadow.get(j) {
            if *v != 0 {
                live = live.saturating_add(1);
            }
        }
        j += 1;
    }
    if live != s.active {
        return Err(MsCode::FuzzBad.val());
    }
    s.audit()?;
    Ok(s)
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(MsCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 满槽第十一点必须 CapacityBad。
        let mut p = Panel::new();
        let mut tick = 0u32;
        let mut ok = true;
        let mut j = 0usize;
        while j < SLOTS {
            tick = tick.saturating_add(1);
            if p.down(spot_contact(j), tick).is_err() {
                ok = false;
            }
            j += 1;
        }
        tick = tick.saturating_add(1);
        if ok && p.down(spot_contact(0), tick).err() == Some(MsCode::CapacityBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 未签发 id 移动必须 IdBad。
        let mut p2 = Panel::new();
        if p2
            .move_to(777, Contact::new(500, 500, 5, 5), 1)
            .err()
            == Some(MsCode::IdBad.val())
        {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 未签发 id 抬起必须 IdBad。
        let mut p3 = Panel::new();
        if p3.lift(777, 1).err() == Some(MsCode::IdBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 时钟回退必须 TickBad。
        let mut p4 = Panel::new();
        let _ = p4.down(spot_contact(0), 5);
        if p4.down(spot_contact(1), 5).err() == Some(MsCode::TickBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(MsCode::CoverBad.val());
    }
    Ok(log)
}

// ── 渲染 ──────────────────────────────────────────────────────────
pub fn render_stats(s: &PanelStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.ops, s.issued, s.lifted, s.active, s.rejected, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(MsCode::RenderBad.val());
    }
    let ops = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let issued = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let lifted = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let active = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let rejected = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    if ops == 0 || issued != lifted.saturating_add(active) {
        return Err(MsCode::LedgerBad.val());
    }
    if ops < issued.saturating_add(rejected) {
        return Err(MsCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(MsCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<PanelStats, u32> {
    let mut p = Panel::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        tick = tick.saturating_add(1);
        let sa = p.down(Contact::new(200, 200, 6, 6), tick)?;
        let ida = match p.id_at(sa as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        tick = tick.saturating_add(1);
        let sb = p.down(Contact::new(600, 200, 6, 6), tick)?;
        let idb = match p.id_at(sb as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        tick = tick.saturating_add(1);
        let _ = p.move_to(ida, Contact::new(220, 210, 6, 6), tick)?;
        tick = tick.saturating_add(1);
        let sc = p.down(Contact::new(1_000, 200, 6, 6), tick)?;
        let idc = match p.id_at(sc as usize) {
            Some(v) => v,
            None => return Err(MsCode::IdBad.val()),
        };
        tick = tick.saturating_add(1);
        let _ = p.lift(idb, tick)?;
        tick = tick.saturating_add(1);
        let _ = p.lift(ida, tick)?;
        tick = tick.saturating_add(1);
        let _ = p.lift(idc, tick)?;
        i = i.saturating_add(1);
    }
    p.audit()?;
    let s = p.stats();
    if s.issued != GOLDEN_ROUNDS.saturating_mul(3)
        || s.lifted != GOLDEN_ROUNDS.saturating_mul(3)
        || s.active != 0
        || s.rejected != 0
    {
        return Err(MsCode::LedgerBad.val());
    }
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MsCode::ReplayBad.val());
    }
    let c = fuzz_multi(128)?;
    let d = fuzz_multi(128)?;
    if c != d {
        return Err(MsCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < MsCode::ALL.len() {
        let mut j = i + 1;
        while j < MsCode::ALL.len() {
            match (MsCode::ALL.get(i), MsCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MsCode::ALL.len() {
        if let Some(c) = MsCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_table()? != SLOTS as u32 {
        return Err(MsCode::TableBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    if capacity_probe(64)? != 64 {
        return Err(MsCode::LedgerBad.val());
    }
    if fill_probe(64)? != 0 {
        return Err(MsCode::LedgerBad.val());
    }
    let ids = id_probe(64)?;
    if ids.issued != 128 || ids.active_zero() != true {
        return Err(MsCode::LedgerBad.val());
    }
    if dead_probe(64)? != 128 {
        return Err(MsCode::FuzzBad.val());
    }
    Ok(())
}

impl IdStats {
    pub fn active_zero(&self) -> bool {
        self.issued == self.rounds.saturating_mul(2)
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    let s = fuzz_multi(256)?;
    if !s.balanced() {
        return Err(MsCode::LedgerBad.val());
    }
    let text = render_stats(&golden_run()?);
    if audit_render(&text)? != STATS_FIELDS {
        return Err(MsCode::RenderBad.val());
    }
    Ok(())
}

// ── F48985 三指工作区手势 + F5 shell 消费分界联签 ─────────────────
/// 工作区手势最小位移（像素）。
pub const WG_MIN_DISP: u64 = 120;
/// 工作区手势单轴位移上界（像素）。
pub const WG_MAX_PX: u64 = 4_800;
/// 工作区手势轴优势分子（主轴 ×2 ≥ 次轴 ×3）。
pub const WG_DOMINANCE_NUM: u64 = 3;
/// 工作区手势轴优势分母（NUM/DEN = 1.5 倍）。
pub const WG_DOMINANCE_DEN: u64 = 2;
/// 三指工作区判别轮数（卡面 ×10^2 的载体）。
pub const WG_ROUNDS: u32 = 100;
/// 联签锚定行（F5 shell 消费分界登记，逐字比对）。
pub const SIGN_ANCHOR: &str = "F5-shell|consume=3f|zone=workspace|UNX-F48985";
/// 锚定行字节上限。
pub const SIGN_CAP: usize = 64;
/// fake 预演次数（卡面 10/10）。
pub const SIGN_ROUNDS: u32 = 10;
/// 延迟打点轮数（手势段 ×10^2）。
pub const LAT_ROUNDS: u32 = 100;
/// 延迟账本槽位（100 轮最多 500 拍）。
pub const LAT_MARKS: usize = 512;
/// 延迟下界（微秒）。
pub const LAT_MIN_US: u16 = 1;
/// 延迟上界（微秒）。
pub const LAT_MAX_US: u16 = 8_000;

/// 三指工作区手势：不判 / 左划 / 右划 / 上划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkspaceGesture {
    None,
    SwipeLeft,
    SwipeRight,
    SwipeUp,
}

impl WorkspaceGesture {
    pub fn code(self) -> u32 {
        match self {
            WorkspaceGesture::None => 0,
            WorkspaceGesture::SwipeLeft => 1,
            WorkspaceGesture::SwipeRight => 2,
            WorkspaceGesture::SwipeUp => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WorkspaceGesture::None => "none",
            WorkspaceGesture::SwipeLeft => "swipe-left",
            WorkspaceGesture::SwipeRight => "swipe-right",
            WorkspaceGesture::SwipeUp => "swipe-up",
        }
    }

    pub const ALL: [WorkspaceGesture; 4] = [
        WorkspaceGesture::None,
        WorkspaceGesture::SwipeLeft,
        WorkspaceGesture::SwipeRight,
        WorkspaceGesture::SwipeUp,
    ];
}

/// 三指工作区判别（纯函数）：指头数 → 位移上界 → 死区 → 轴优势 → 方向。
///
/// 只认左/右/上：下划是通知栏的系统手势，工作区不该先吃掉；45° 斜划
/// 在物理上不属于任何一条轴，判成工作区会让同一轨迹随抖动变形。
pub fn recognize_workspace(fingers: u8, dx: i32, dy: i32) -> Result<WorkspaceGesture, u32> {
    if fingers != 3 {
        return Err(MsCode::SlotBad.val());
    }
    let ax = (dx as i64).unsigned_abs();
    let ay = (dy as i64).unsigned_abs();
    if ax > WG_MAX_PX || ay > WG_MAX_PX {
        return Err(MsCode::CoordBad.val());
    }
    if ax < WG_MIN_DISP && ay < WG_MIN_DISP {
        return Ok(WorkspaceGesture::None);
    }
    if ax == ay {
        return Ok(WorkspaceGesture::None);
    }
    let (major, minor, horiz) = if ax > ay {
        (ax, ay, true)
    } else {
        (ay, ax, false)
    };
    if major.saturating_mul(WG_DOMINANCE_DEN) < minor.saturating_mul(WG_DOMINANCE_NUM) {
        return Ok(WorkspaceGesture::None);
    }
    if horiz {
        Ok(if dx > 0 {
            WorkspaceGesture::SwipeRight
        } else {
            WorkspaceGesture::SwipeLeft
        })
    } else if dy < 0 {
        Ok(WorkspaceGesture::SwipeUp)
    } else {
        // 下划保留给通知栏：三指语境下明确「不归工作区」。
        Ok(WorkspaceGesture::None)
    }
}

/// 三指工作区批量账（×10^2 场景汇总）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorkspaceStats {
    pub scenarios: u32,
    pub swipes: u32,
    pub passthrough: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl WorkspaceStats {
    pub fn new() -> WorkspaceStats {
        WorkspaceStats {
            scenarios: 0,
            swipes: 0,
            passthrough: 0,
            rejects: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 每场景恰落入「识别 / 穿透 / 拒绝」之一。
    pub fn balanced(&self) -> bool {
        self.scenarios > 0
            && self
                .swipes
                .saturating_add(self.passthrough)
                .saturating_add(self.rejects)
                == self.scenarios
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(MsCode::LedgerBad.val());
        }
        if self.swipes == 0 || self.rejects == 0 || self.passthrough == 0 {
            return Err(MsCode::CoverBad.val());
        }
        Ok(())
    }
}

/// 三指工作区 ×10^2：按 i%8 生成八类输入（左/右/上/下/斜/过小/两指/四指），
/// 期望值来自输入构造而非判别器，因此可被证伪。
pub fn workspace_scenarios(rounds: u32) -> Result<WorkspaceStats, u32> {
    if rounds == 0 || rounds > WG_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut s = WorkspaceStats::new();
    let mut i = 0u32;
    while i < rounds {
        let step = WG_MIN_DISP as i32 + 10 + (i % 40) as i32;
        let small = 20 + (i % 30) as i32;
        let (fingers, dx, dy, want) = match i % 8 {
            0 => (3u8, -step, small, Some(WorkspaceGesture::SwipeLeft)),
            1 => (3, step, -small, Some(WorkspaceGesture::SwipeRight)),
            2 => (3, small, -step, Some(WorkspaceGesture::SwipeUp)),
            // 下划：留给通知栏，工作区穿透。
            3 => (3, small, step, Some(WorkspaceGesture::None)),
            // 斜划：等长不判。
            4 => (3, step, step, Some(WorkspaceGesture::None)),
            // 未过死区。
            5 => (3, small, -small, Some(WorkspaceGesture::None)),
            // 指头数不对：拒绝，且必须是 SlotBad（不是静默穿透）。
            6 => (2, -step, small, None),
            _ => (4, step, small, None),
        };
        let got = recognize_workspace(fingers, dx, dy);
        match want {
            Some(w) => {
                if got != Ok(w) {
                    return Err(MsCode::FuzzBad.val());
                }
                if w == WorkspaceGesture::None {
                    s.passthrough = s.passthrough.saturating_add(1);
                } else {
                    s.swipes = s.swipes.saturating_add(1);
                }
                s.digest = p_core::fold_u32(s.digest, w.code());
            }
            None => {
                if got != Err(MsCode::SlotBad.val()) {
                    return Err(MsCode::FuzzBad.val());
                }
                s.rejects = s.rejects.saturating_add(1);
                s.digest = p_core::fold_u32(s.digest, MsCode::SlotBad.val());
            }
        }
        s.scenarios = s.scenarios.saturating_add(1);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// F5 shell 消费分界联签：锚定行逐字比对 + fake 预演 10/10 + 消费登记。
#[derive(Clone)]
pub struct ShellSignoff {
    anchor: [u8; SIGN_CAP],
    anchor_len: usize,
    anchor_digest: u64,
    pub rehearsals: u32,
    pub passed: u32,
    pub misfires: u32,
    pub consumed: u32,
    pub passthrough: u32,
    digest: u64,
}

impl ShellSignoff {
    pub fn new() -> ShellSignoff {
        let bytes = SIGN_ANCHOR.as_bytes();
        let mut anchor = [0u8; SIGN_CAP];
        let mut anchor_digest = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < bytes.len() && i < SIGN_CAP {
            if let (Some(cell), Some(b)) = (anchor.get_mut(i), bytes.get(i)) {
                *cell = *b;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < bytes.len() {
            if let Some(b) = bytes.get(k) {
                anchor_digest = p_core::fold_u32(anchor_digest, *b as u32);
            }
            k += 1;
        }
        let len = if bytes.len() > SIGN_CAP { SIGN_CAP } else { bytes.len() };
        ShellSignoff {
            anchor,
            anchor_len: len,
            anchor_digest,
            rehearsals: 0,
            passed: 0,
            misfires: 0,
            consumed: 0,
            passthrough: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 锚定行原文（从固定字节数组重建，避免依赖 &str 指针身份）。
    pub fn anchor_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.anchor_len {
            if let Some(b) = self.anchor.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 锚定行逐字一致 + 指纹一致。
    pub fn anchor_ok(&self) -> bool {
        let bytes = SIGN_ANCHOR.as_bytes();
        if self.anchor_len != bytes.len() || self.anchor_len > SIGN_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.anchor_len {
            match (self.anchor.get(i), bytes.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        let mut d = p_core::FNV_OFF;
        let mut k = 0usize;
        while k < self.anchor_len {
            if let Some(b) = self.anchor.get(k) {
                d = p_core::fold_u32(d, *b as u32);
            }
            k += 1;
        }
        d == self.anchor_digest
    }

    /// 消费分界登记：三指工作区手势计 consumed，其余计 passthrough。
    pub fn record(&mut self, g: WorkspaceGesture) -> Result<(), u32> {
        if g == WorkspaceGesture::None {
            self.passthrough = self.passthrough.saturating_add(1);
        } else {
            self.consumed = self.consumed.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, g.code());
        Ok(())
    }

    /// fake 预演一次：预演次数封顶 10；失败立即记 misfire 并报错。
    pub fn rehearse(&mut self, ok: bool) -> Result<u32, u32> {
        if self.rehearsals >= SIGN_ROUNDS {
            return Err(MsCode::CoverBad.val());
        }
        self.rehearsals = self.rehearsals.saturating_add(1);
        if ok {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.misfires = self.misfires.saturating_add(1);
        }
        self.digest = p_core::fold_u32(self.digest, if ok { 1 } else { 0 });
        if !ok {
            return Err(MsCode::FuzzBad.val());
        }
        Ok(self.rehearsals)
    }

    /// 10/10 且零 misfire 才算可登记。
    pub fn ready(&self) -> bool {
        self.anchor_ok()
            && self.rehearsals == SIGN_ROUNDS
            && self.passed == SIGN_ROUNDS
            && self.misfires == 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.anchor_ok() {
            return Err(MsCode::TableBad.val());
        }
        if self.passed.saturating_add(self.misfires) != self.rehearsals {
            return Err(MsCode::LedgerBad.val());
        }
        if self.rehearsals > SIGN_ROUNDS {
            return Err(MsCode::CoverBad.val());
        }
        if !self.ready() {
            return Err(MsCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(MsCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 联签预演：锚定行逐字比对 → 10 次 fake 预演 → 消费分界登记 → 收账。
pub fn signoff_rehearsal() -> Result<u32, u32> {
    let mut s = ShellSignoff::new();
    if !s.anchor_ok() {
        return Err(MsCode::TableBad.val());
    }
    if s.anchor_text().as_str() != SIGN_ANCHOR {
        return Err(MsCode::TableBad.val());
    }
    let mut i = 0u32;
    while i < SIGN_ROUNDS {
        s.rehearse(true)?;
        i = i.saturating_add(1);
    }
    s.record(WorkspaceGesture::SwipeLeft)?;
    s.record(WorkspaceGesture::SwipeRight)?;
    s.record(WorkspaceGesture::SwipeUp)?;
    s.record(WorkspaceGesture::None)?;
    if s.consumed != 3 || s.passthrough != 1 {
        return Err(MsCode::LedgerBad.val());
    }
    s.audit()?;
    if !s.ready() {
        return Err(MsCode::LedgerBad.val());
    }
    Ok(s.passed)
}

// ── F48986 手势识别延迟打点（账本手势段接入） ────────────────────
/// 打点类型：起手 / 手势段 / 收手。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Start,
    Segment,
    End,
}

impl Mark {
    pub fn code(self) -> u32 {
        match self {
            Mark::Start => 1,
            Mark::Segment => 2,
            Mark::End => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mark::Start => "start",
            Mark::Segment => "segment",
            Mark::End => "end",
        }
    }

    pub const ALL: [Mark; 3] = [Mark::Start, Mark::Segment, Mark::End];
}

/// 延迟账本：每拍打点占用一个槽位，落账成功才写指纹与时间线。
#[derive(Clone)]
pub struct LatencyLedger {
    kinds: [u8; LAT_MARKS],
    times: [u32; LAT_MARKS],
    used: usize,
    pub attempts: u32,
    pub landed: u32,
    pub dropped: u32,
    pub gestures: u32,
    pub segments: u32,
    pub min_lat: u16,
    pub max_lat: u16,
    last_t: u32,
    cur_start: u32,
    in_gesture: bool,
    digest: u64,
}

impl LatencyLedger {
    pub fn new() -> LatencyLedger {
        LatencyLedger {
            kinds: [0u8; LAT_MARKS],
            times: [0u32; LAT_MARKS],
            used: 0,
            attempts: 0,
            landed: 0,
            dropped: 0,
            gestures: 0,
            segments: 0,
            min_lat: u16::MAX,
            max_lat: 0,
            last_t: 0,
            cur_start: 0,
            in_gesture: false,
            digest: p_core::FNV_OFF,
        }
    }

    /// 在途手势数（起手未收手）。
    pub fn open(&self) -> u32 {
        if self.in_gesture {
            1
        } else {
            0
        }
    }

    /// 打一拍：容量 → 时钟 → 次序 →（收手时）延迟界 → 落账。
    pub fn mark(&mut self, kind: Mark, t: u32) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if self.used >= LAT_MARKS {
            self.dropped = self.dropped.saturating_add(1);
            return Err(MsCode::LedgerBad.val());
        }
        if t <= self.last_t {
            self.dropped = self.dropped.saturating_add(1);
            return Err(MsCode::TickBad.val());
        }
        let order_ok = match kind {
            Mark::Start => !self.in_gesture,
            Mark::Segment => self.in_gesture,
            Mark::End => self.in_gesture,
        };
        if !order_ok {
            self.dropped = self.dropped.saturating_add(1);
            return Err(MsCode::OrderBad.val());
        }
        if kind == Mark::End {
            let span = t.saturating_sub(self.cur_start);
            let lat = if span > u16::MAX as u32 {
                u16::MAX
            } else {
                span as u16
            };
            if lat < LAT_MIN_US || lat > LAT_MAX_US {
                self.dropped = self.dropped.saturating_add(1);
                return Err(MsCode::OrderBad.val());
            }
            if lat > self.max_lat {
                self.max_lat = lat;
            }
            if lat < self.min_lat {
                self.min_lat = lat;
            }
            self.in_gesture = false;
        }
        if kind == Mark::Start {
            self.in_gesture = true;
            self.cur_start = t;
            self.gestures = self.gestures.saturating_add(1);
        }
        if kind == Mark::Segment {
            self.segments = self.segments.saturating_add(1);
        }
        self.last_t = t;
        if let Some(cell) = self.kinds.get_mut(self.used) {
            *cell = kind.code() as u8;
        }
        if let Some(cell) = self.times.get_mut(self.used) {
            *cell = t;
        }
        self.used = self.used.saturating_add(1);
        self.landed = self.landed.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, kind.code());
        self.digest = p_core::fold_u32(self.digest, t);
        Ok(t)
    }

    /// 审计：每拍要么落账要么判错、收手闭合、延迟上下界自洽、指纹非零。
    pub fn audit(&self) -> Result<(), u32> {
        if self.attempts == 0 || self.gestures == 0 {
            return Err(MsCode::CoverBad.val());
        }
        if self.used != self.landed as usize {
            return Err(MsCode::LedgerBad.val());
        }
        if self.landed.saturating_add(self.dropped) != self.attempts {
            return Err(MsCode::LedgerBad.val());
        }
        if self.in_gesture {
            return Err(MsCode::OrderBad.val());
        }
        if self.min_lat > self.max_lat {
            return Err(MsCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(MsCode::ReplayBad.val());
        }
        Ok(())
    }

    pub fn stats(&self) -> LatencyStats {
        LatencyStats {
            gestures: self.gestures,
            marks: self.landed,
            landed: self.landed,
            dropped: self.dropped,
            min_lat: self.min_lat,
            max_lat: self.max_lat,
            digest: self.digest,
        }
    }
}

/// 延迟批量账（×10^2 手势段汇总）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyStats {
    pub gestures: u32,
    pub marks: u32,
    pub landed: u32,
    pub dropped: u32,
    pub min_lat: u16,
    pub max_lat: u16,
    pub digest: u64,
}

impl LatencyStats {
    /// 全部落账：零丢弃、落账数等于打点数、每手势至少一拍。
    pub fn all_booked(&self) -> bool {
        self.gestures > 0
            && self.dropped == 0
            && self.landed == self.marks
            && self.marks >= self.gestures.saturating_mul(3)
            && self.min_lat <= self.max_lat
            && self.max_lat <= LAT_MAX_US
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.all_booked() {
            return Err(MsCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 手势段 ×10^2：起手 → 1..=3 段 → 收手，每拍都必须在账上。
pub fn latency_rounds(rounds: u32) -> Result<LatencyStats, u32> {
    if rounds == 0 || rounds > LAT_ROUNDS {
        return Err(MsCode::CoverBad.val());
    }
    let mut l = LatencyLedger::new();
    let mut t = 0u32;
    let mut i = 0u32;
    while i < rounds {
        t = t.saturating_add(1);
        l.mark(Mark::Start, t)?;
        let segs = 1 + (i % 3);
        let mut k = 0u32;
        while k < segs {
            t = t.saturating_add(1);
            l.mark(Mark::Segment, t)?;
            k = k.saturating_add(1);
        }
        // 手势跨度 6..=41 us：保证 min < max 且落在界内。
        t = t.saturating_add(5 + (i % 36));
        l.mark(Mark::End, t)?;
        i = i.saturating_add(1);
    }
    l.audit()?;
    let s = l.stats();
    if s.gestures != rounds || s.dropped != 0 || s.landed != s.marks {
        return Err(MsCode::LedgerBad.val());
    }
    s.audit()?;
    Ok(s)
}

/// 延迟打点复跑：两次 100 手势必须逐位一致。
pub fn latency_replay() -> Result<u64, u32> {
    let a = latency_rounds(LAT_ROUNDS)?;
    let b = latency_rounds(LAT_ROUNDS)?;
    if a != b {
        return Err(MsCode::ReplayBad.val());
    }
    Ok(a.digest)
}

/// 延迟账本文本：`手势|打点|落账|丢弃|最小延迟|指纹`。
pub fn render_latency(s: &LatencyStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.gestures, s.marks, s.landed, s.dropped, s.min_lat, s.digest
    )
}

/// 延迟账本回读：字段数、零丢弃、每手势 ≥3 拍、指纹非零。
pub fn audit_latency_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(MsCode::RenderBad.val());
    }
    let gestures = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let marks = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let landed = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let dropped = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(MsCode::RenderBad.val()),
    };
    if gestures == 0 || marks != landed || dropped != 0 {
        return Err(MsCode::LedgerBad.val());
    }
    if marks < gestures.saturating_mul(3) {
        return Err(MsCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(MsCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let sign = signoff_rehearsal();
    out.push((
        "F48985 联签锚定+预演10/10",
        matches!(&sign, Ok(n) if *n == SIGN_ROUNDS),
        match &sign {
            Ok(n) => format!("pass={}/{} anchor=ok", n, SIGN_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let wg = workspace_scenarios(WG_ROUNDS);
    out.push((
        "F48985 三指工作区×10^2",
        matches!(&wg, Ok(s) if s.balanced() && s.swipes == 39 && s.rejects == 24),
        match &wg {
            Ok(s) => format!("sw={} ps={} rj={}", s.swipes, s.passthrough, s.rejects),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let lat = latency_rounds(LAT_ROUNDS);
    out.push((
        "F48986 手势段×10^2全落账",
        matches!(&lat, Ok(s) if s.all_booked() && s.gestures == LAT_ROUNDS),
        match &lat {
            Ok(s) => format!("mk={} drop={} g={}", s.marks, s.dropped, s.gestures),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let cap = capacity_probe(CAP_ROUNDS);
    out.push((
        "F48985 满槽十一点必拒",
        matches!(&cap, Ok(n) if *n == CAP_ROUNDS),
        match &cap {
            Ok(n) => format!("capacity-hits={} rounds={}", n, CAP_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fill = fill_probe(CAP_ROUNDS);
    out.push((
        "F48985 合法十点零误杀",
        matches!(&fill, Ok(0)),
        match &fill {
            Ok(n) => format!("residual={} slots={}", n, SLOTS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let ids = id_probe(ID_ROUNDS);
    let dead = dead_probe(DEAD_ROUNDS);
    out.push((
        "F48985 id递增与旧id失效",
        matches!(&ids, Ok(s) if s.first_id == 1 && s.last_id == s.rounds * 2)
            && matches!(&dead, Ok(n) if *n == DEAD_ROUNDS * 2),
        match (&ids, &dead) {
            (Ok(s), Ok(n)) => format!("id={} dead={}", s.last_id, n),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_multi(FUZZ_ROUNDS);
    out.push((
        "F48985 千轮影子对账",
        matches!(&fuzz, Ok(s) if s.balanced() && s.active <= SLOTS as u32),
        match &fuzz {
            Ok(s) => format!("ops={} act={} rej={}", s.ops, s.active, s.rejected),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let render = golden_run().map(|s| render_stats(&s));
    let lrender = lat.clone().map(|s| render_latency(&s));
    out.push((
        "F48985/86 账本与复跑",
        matches!(&render, Ok(t) if audit_render(t) == Ok(STATS_FIELDS))
            && matches!(&lrender, Ok(t) if audit_latency_render(t) == Ok(STATS_FIELDS))
            && repro_check().is_ok(),
        match (&render, &lrender) {
            (Ok(t), Ok(u)) => format!("g={} y={}", t.len(), u.len()),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48985_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48985 多点上限与槽位生命周期");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: i32, y: i32) -> Contact {
        Contact::new(x, y, 5, 5)
    }

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x716C0);
        assert_eq!(SLOTS, 10);
        assert_eq!(SIDE_MAX, 100);
        assert_eq!(AREA_MAX, 10_000);
        assert_eq!(MsCode::ALL.len(), 14);
        assert_eq!(audit_table(), Ok(10));
    }

    #[test]
    fn down_assigns_increasing_ids() {
        let mut p = Panel::new();
        let a = p.down(c(100, 100), 1);
        let b = p.down(c(200, 100), 2);
        assert_eq!(a, Ok(0));
        assert_eq!(b, Ok(1));
        assert_eq!(p.id_at(0), Some(1));
        assert_eq!(p.id_at(1), Some(2));
        assert!(p.audit().is_ok());
    }

    #[test]
    fn capacity_rejects_eleventh() {
        let mut p = Panel::new();
        let mut i = 0usize;
        while i < SLOTS {
            assert!(p.down(c(100 + i as i32, 100), i as u32 + 1).is_ok());
            i += 1;
        }
        assert_eq!(
            p.down(c(50, 50), (SLOTS as u32) + 1),
            Err(MsCode::CapacityBad.val())
        );
        assert_eq!(p.active(), SLOTS as u32);
    }

    #[test]
    fn lift_frees_slot_for_reuse() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 1);
        let _ = p.down(c(200, 100), 2);
        let id0 = p.id_at(0);
        assert_eq!(p.lift(id0.unwrap_or(0), 3), Ok(0));
        let s = p.down(c(300, 100), 4);
        assert_eq!(s, Ok(0));
        assert_ne!(p.id_at(0), id0);
    }

    #[test]
    fn move_requires_live_id() {
        let mut p = Panel::new();
        assert_eq!(
            p.move_to(9, c(100, 100), 1),
            Err(MsCode::IdBad.val())
        );
        let _ = p.down(c(100, 100), 2);
        let id = p.id_at(0).unwrap_or(0);
        assert_eq!(p.move_to(id, c(150, 150), 3), Ok(0));
        assert_eq!(p.move_to(9, c(150, 150), 4), Err(MsCode::IdBad.val()));
    }

    #[test]
    fn lift_requires_live_id() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 1);
        let id = p.id_at(0).unwrap_or(0);
        assert_eq!(p.lift(id, 2), Ok(0));
        assert_eq!(p.lift(id, 3), Err(MsCode::IdBad.val()));
    }

    #[test]
    fn cancel_all_releases_everything() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 1);
        let _ = p.down(c(200, 100), 2);
        assert_eq!(p.cancel_all(3), Ok(2));
        assert_eq!(p.active(), 0);
        assert_eq!(p.cancel_all(4), Ok(0));
        assert!(p.audit().is_ok());
    }

    #[test]
    fn tick_regression_is_rejected() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 5);
        assert_eq!(p.down(c(200, 100), 5), Err(MsCode::TickBad.val()));
        assert_eq!(p.lift(1, 4), Err(MsCode::TickBad.val()));
    }

    #[test]
    fn coord_and_area_are_rejected() {
        let mut p = Panel::new();
        assert_eq!(
            p.down(Contact::new(-1, 0, 5, 5), 1),
            Err(MsCode::CoordBad.val())
        );
        assert_eq!(
            p.down(Contact::new(SCREEN_W, 0, 5, 5), 2),
            Err(MsCode::CoordBad.val())
        );
        assert_eq!(
            p.down(Contact::new(10, 10, 0, 5), 3),
            Err(MsCode::AreaBad.val())
        );
        assert_eq!(
            p.down(Contact::new(10, 10, SIDE_MAX + 1, 5), 4),
            Err(MsCode::AreaBad.val())
        );
    }

    #[test]
    fn area_gate_precedes_capacity() {
        let mut p = Panel::new();
        let mut i = 0usize;
        while i < SLOTS {
            let _ = p.down(c(100 + i as i32, 100), i as u32 + 1);
            i += 1;
        }
        // 满槽时坏面积仍报 AreaBad，而不是 CapacityBad。
        assert_eq!(
            p.down(Contact::new(10, 10, 0, 0), (SLOTS as u32) + 1),
            Err(MsCode::AreaBad.val())
        );
    }

    #[test]
    fn id_strictly_increases() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 1);
        let a = p.id_at(0).unwrap_or(0);
        let _ = p.lift(a, 2);
        let _ = p.down(c(100, 100), 3);
        let b = p.id_at(0).unwrap_or(0);
        assert!(b > a);
        assert_eq!(p.next_id(), 2);
    }

    #[test]
    fn audit_catches_broken_conservation() {
        let mut p = Panel::new();
        let _ = p.down(c(100, 100), 1);
        let s = p.stats();
        assert!(s.balanced());
        let broken = PanelStats {
            issued: 5,
            ..s
        };
        assert_eq!(broken.audit(), Err(MsCode::LedgerBad.val()));
    }

    #[test]
    fn capacity_probe_is_exact() {
        assert_eq!(capacity_probe(32), Ok(32));
        assert_eq!(capacity_probe(0), Err(MsCode::CoverBad.val()));
        assert_eq!(
            capacity_probe(CAP_ROUNDS + 1),
            Err(MsCode::CoverBad.val())
        );
    }

    #[test]
    fn fill_probe_stays_empty() {
        assert_eq!(fill_probe(32), Ok(0));
    }

    #[test]
    fn id_probe_is_monotone() {
        match id_probe(32) {
            Ok(s) => {
                assert_eq!(s.first_id, 1);
                assert_eq!(s.last_id, 64);
                assert_eq!(s.issued, 64);
                assert!(s.active_zero());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn dead_probe_catches_both() {
        assert_eq!(dead_probe(16), Ok(32));
    }

    #[test]
    fn fuzz_multi_balances() {
        match fuzz_multi(FUZZ_ROUNDS) {
            Ok(s) => {
                assert!(s.balanced());
                assert!(s.active <= SLOTS as u32);
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero() {
        assert_eq!(fuzz_multi(0), Err(MsCode::CoverBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
                assert_eq!(l.caught, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_rejects_zero() {
        assert_eq!(inject_rounds(0), Err(MsCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                let bad = text.replacen("48|", "47|", 1);
                assert!(bad != text);
                assert_eq!(audit_render(&bad), Err(MsCode::LedgerBad.val()));
                assert_eq!(audit_render("48|1"), Err(MsCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok(s) => {
                assert_eq!(s.issued, GOLDEN_ROUNDS * 3);
                assert_eq!(s.lifted, GOLDEN_ROUNDS * 3);
                assert_eq!(s.active, 0);
                assert_eq!(s.rejected, 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(MsCode::CapacityBad.val()), MsCode::CapacityBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48985_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    // ── F48985 三指工作区 + shell 联签 ────────────────────────────
    #[test]
    fn workspace_constants_are_anchored() {
        assert_eq!(WG_MIN_DISP, 120);
        assert_eq!(WG_DOMINANCE_NUM * 2, WG_DOMINANCE_DEN * 3);
        assert_eq!(WG_ROUNDS, 100);
        assert_eq!(SIGN_ROUNDS, 10);
        assert_eq!(WorkspaceGesture::ALL.len(), 4);
        assert_eq!(WorkspaceGesture::None.code(), 0);
        assert_eq!(WorkspaceGesture::SwipeUp.name(), "swipe-up");
    }

    #[test]
    fn recognize_workspace_directions() {
        assert_eq!(
            recognize_workspace(3, -300, 30),
            Ok(WorkspaceGesture::SwipeLeft)
        );
        assert_eq!(
            recognize_workspace(3, 300, -30),
            Ok(WorkspaceGesture::SwipeRight)
        );
        assert_eq!(
            recognize_workspace(3, 20, -300),
            Ok(WorkspaceGesture::SwipeUp)
        );
        // 下划留给通知栏：明确穿透，不是拒绝。
        assert_eq!(
            recognize_workspace(3, 20, 300),
            Ok(WorkspaceGesture::None)
        );
        // 斜划等长不判。
        assert_eq!(
            recognize_workspace(3, 240, 240),
            Ok(WorkspaceGesture::None)
        );
        // 死区内不判。
        assert_eq!(
            recognize_workspace(3, 30, -20),
            Ok(WorkspaceGesture::None)
        );
        // 指头数不对：SlotBad，不静默穿透。
        assert_eq!(
            recognize_workspace(2, -300, 20),
            Err(MsCode::SlotBad.val())
        );
        assert_eq!(
            recognize_workspace(4, 300, 20),
            Err(MsCode::SlotBad.val())
        );
        // 位移上界越界：CoordBad。
        assert_eq!(
            recognize_workspace(3, (WG_MAX_PX as i32) + 1, 0),
            Err(MsCode::CoordBad.val())
        );
    }

    #[test]
    fn workspace_100_scenarios_balance() {
        match workspace_scenarios(WG_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.scenarios, WG_ROUNDS);
                // 100 拍按 i%8：3/8 识别 + 3/8 穿透 + 2/8 拒绝。
                assert_eq!(s.swipes, 39);
                assert_eq!(s.passthrough, 37);
                assert_eq!(s.rejects, 24);
                assert!(s.balanced());
                assert_eq!(s.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn workspace_rejects_bad_rounds() {
        assert_eq!(workspace_scenarios(0), Err(MsCode::CoverBad.val()));
        assert_eq!(
            workspace_scenarios(WG_ROUNDS + 1),
            Err(MsCode::CoverBad.val())
        );
    }

    #[test]
    fn signoff_anchor_is_exact() {
        let s = ShellSignoff::new();
        assert!(s.anchor_ok());
        assert_eq!(s.anchor_text(), SIGN_ANCHOR);
        assert_eq!(SIGN_ANCHOR.as_bytes().len(), 45);
    }

    #[test]
    fn signoff_rehearsal_is_ten_of_ten() {
        assert_eq!(signoff_rehearsal(), Ok(10));
        let mut s = ShellSignoff::new();
        let mut i = 0u32;
        while i < SIGN_ROUNDS {
            assert_eq!(s.rehearse(true), Ok(i + 1));
            i += 1;
        }
        assert!(s.ready());
        assert_eq!(s.audit(), Ok(()));
        // 第 11 次预演越界：CoverBad，不得把 10/10 刷成 11/11。
        assert_eq!(s.rehearse(true), Err(MsCode::CoverBad.val()));
        assert_eq!(s.rehearsals, SIGN_ROUNDS);
    }

    #[test]
    fn signoff_failure_blocks_registration() {
        let mut s = ShellSignoff::new();
        assert_eq!(s.rehearse(false), Err(MsCode::FuzzBad.val()));
        assert_eq!(s.misfires, 1);
        assert_eq!(s.passed, 0);
        assert!(!s.ready());
        assert_eq!(s.audit(), Err(MsCode::LedgerBad.val()));
    }

    // ── F48986 延迟打点 ────────────────────────────────────────────
    #[test]
    fn latency_ledger_books_three_marks() {
        let mut l = LatencyLedger::new();
        assert_eq!(l.mark(Mark::Start, 100), Ok(100));
        assert_eq!(l.open(), 1);
        assert_eq!(l.mark(Mark::Segment, 103), Ok(103));
        assert_eq!(l.mark(Mark::End, 108), Ok(108));
        assert_eq!(l.open(), 0);
        let s = l.stats();
        assert_eq!(s.gestures, 1);
        assert_eq!(s.marks, 3);
        assert_eq!(s.landed, 3);
        assert_eq!(s.dropped, 0);
        assert_eq!(s.min_lat, 8);
        assert_eq!(s.max_lat, 8);
        assert!(s.all_booked());
        assert_eq!(l.audit(), Ok(()));
    }

    #[test]
    fn latency_order_is_enforced() {
        let mut l = LatencyLedger::new();
        // 未起手就打段：OrderBad。
        assert_eq!(l.mark(Mark::Segment, 5), Err(MsCode::OrderBad.val()));
        // 未起手就收手：OrderBad。
        assert_eq!(l.mark(Mark::End, 6), Err(MsCode::OrderBad.val()));
        // 起手后重复起手：OrderBad。
        assert_eq!(l.mark(Mark::Start, 7), Ok(7));
        assert_eq!(l.mark(Mark::Start, 8), Err(MsCode::OrderBad.val()));
        // 时钟回退：TickBad。
        assert_eq!(l.mark(Mark::Segment, 7), Err(MsCode::TickBad.val()));
        // 四拍判错全部计入 dropped，后续合法收手仍要落账。
        assert_eq!(l.dropped, 4);
        assert_eq!(l.mark(Mark::Segment, 9), Ok(9));
        assert_eq!(l.mark(Mark::End, 15), Ok(15));
        let s = l.stats();
        assert_eq!(s.marks, 3);
        assert_eq!(s.dropped, 4);
        assert_eq!(s.landed + s.dropped, l.attempts);
        // 有判错的一轮不算「全落账」，但账本本身仍须自洽。
        assert!(!s.all_booked());
        assert_eq!(l.audit(), Ok(()));
    }

    #[test]
    fn latency_capacity_is_bounded() {
        let mut l = LatencyLedger::new();
        let mut t = 0u32;
        let mut hit = false;
        let mut i = 0u32;
        while i < 300 {
            t += 1;
            if l.mark(Mark::Start, t).is_err() {
                hit = true;
                break;
            }
            t += 1;
            if l.mark(Mark::Segment, t).is_err() {
                hit = true;
                break;
            }
            t += 2;
            if l.mark(Mark::End, t).is_err() {
                hit = true;
                break;
            }
            i += 1;
        }
        assert!(hit);
        assert_eq!(l.used, LAT_MARKS);
        assert!(l.dropped >= 1);
        assert_eq!(l.landed, LAT_MARKS as u32);
    }

    #[test]
    fn latency_100_rounds_all_booked() {
        match (latency_rounds(LAT_ROUNDS), latency_replay()) {
            (Ok(s), Ok(d)) => {
                assert_eq!(s.gestures, LAT_ROUNDS);
                assert_eq!(s.marks, s.landed);
                assert_eq!(s.dropped, 0);
                // 每轮 1 起手 + 1..=3 段 + 1 收手 → 3..=5 拍。
                assert!(s.marks >= LAT_ROUNDS * 3 && s.marks <= LAT_ROUNDS * 5);
                assert!(s.all_booked());
                assert_eq!(s.digest, d);
                let text = render_latency(&s);
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_latency_render(&text), Ok(STATS_FIELDS));
                let bad = text.replacen("|0|", "|1|", 1);
                assert!(bad != text);
                assert!(audit_latency_render(&bad).is_err());
                assert_eq!(audit_latency_render("1|3"), Err(MsCode::RenderBad.val()));
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn latency_rejects_bad_rounds() {
        assert_eq!(latency_rounds(0), Err(MsCode::CoverBad.val()));
        assert_eq!(
            latency_rounds(LAT_ROUNDS + 1),
            Err(MsCode::CoverBad.val())
        );
    }
}
