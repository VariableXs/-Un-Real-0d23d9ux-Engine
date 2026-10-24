//! veb334_comint —— VE-F0334 社区协作接口。
//! 锚点：VE-F0334 · 目标 320 行。
//! 判据原文：四要素模板（环境信息 + 复现步骤 + 期望 + 实际）、
//!           上游优先（补丁贡献上游优先原则）、双账映射
//!           （内部缺陷 ↔ 上游缺陷）、发布前确认（对外
//!           动作红线）、判据。
//! 组件：报告模板校验 + 双账映射表 + 发布确认闸。
//! 职责定位：报告模板是「报警电话的标准话术」——四要素缺一
//!           就是说了等于没说；双账映射是内外两本账的对账
//!           单：上游修了，内部账也要销。
//! 性能逐项分解：映射 O(账目) 增量；模板校验 O(1)；更新 O(1)。
//! 诊断码段：0x31C80（0x31C81~0x31C89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CI_VERSION: &str = "veb334_comint-v1";

/// 诊断段基址（0x31C80 段 · 本模块独占）。
pub const CI_SEG_BASE: u32 = 0x31C80;
pub const XC_MISSING: u32 = 0x31C81; // 模板缺要素
pub const XC_NOMAP: u32 = 0x31C82; // 映射不存在
pub const XC_DUP: u32 = 0x31C83; // 映射重复
pub const XC_UNCONFIRM: u32 = 0x31C84; // 未确认即发布
pub const XC_REJECT: u32 = 0x31C85; // 上游拒绝

/// 四要素位掩码。
pub const F_ENV: u32 = 1; // 环境信息
pub const F_STEP: u32 = 2; // 复现步骤
pub const F_EXPECT: u32 = 4; // 期望
pub const F_ACTUAL: u32 = 8; // 实际
/// 四要素齐备掩码。
pub const F_ALL: u32 = F_ENV | F_STEP | F_EXPECT | F_ACTUAL;

/// 报告模板。
pub struct Report {
    pub id: u32,
    /// 要素掩码。
    pub feats: u32,
}

/// 映射状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapState {
    /// 双账在册。
    Open,
    /// 上游已修（待同步销账）。
    FixedUp,
    /// 上游拒绝（策略调整）。
    Rejected,
}

/// 映射条目。
pub struct MapItem {
    /// 内部缺陷号。
    pub inner: u32,
    /// 上游缺陷号。
    pub outer: u32,
    pub state: MapState,
}

/// 协作簿。
pub struct ComBook {
    maps: Vec<MapItem>,
    /// 待发布队列（发布前需确认）。
    pending: Vec<u32>,
    /// 确认计数。
    pub confirmed: u32,
    /// 拒绝登记数。
    pub rejected: u32,
}

impl ComBook {
    /// 空簿。
    pub fn new() -> ComBook {
        ComBook {
            maps: Vec::new(),
            pending: Vec::new(),
            confirmed: 0,
            rejected: 0,
        }
    }

    /// 模板校验：四要素齐才可提交。
    pub fn tpl_ok(feats: u32) -> bool {
        feats & F_ALL == F_ALL
    }

    /// 提交报告：缺要素拒。
    pub fn submit(&self, id: u32, feats: u32) -> Result<Report, u32> {
        if !Self::tpl_ok(feats) {
            return Err(XC_MISSING);
        }
        Ok(Report { id, feats })
    }

    /// 建映射：内外号都非零，重复拒。
    pub fn map(&mut self, inner: u32, outer: u32) -> Result<(), u32> {
        if inner == 0 || outer == 0 {
            return Err(XC_MISSING);
        }
        if self
            .maps
            .iter()
            .any(|m| m.inner == inner && m.outer == outer)
        {
            return Err(XC_DUP);
        }
        self.maps.push(MapItem {
            inner,
            outer,
            state: MapState::Open,
        });
        Ok(())
    }

    /// 上游拒绝登记。
    pub fn mark_reject(&mut self, inner: u32) -> Result<(), u32> {
        let m = self
            .maps
            .iter_mut()
            .find(|m| m.inner == inner)
            .ok_or(XC_NOMAP)?;
        m.state = MapState::Rejected;
        self.rejected += 1;
        Ok(())
    }

    /// 上游已修 → 同步销账（双账同步）。
    pub fn sync_close(&mut self, outer: u32) -> Result<u32, u32> {
        let m = self
            .maps
            .iter_mut()
            .find(|m| m.outer == outer)
            .ok_or(XC_NOMAP)?;
        if m.state != MapState::Open {
            return Err(XC_NOMAP);
        }
        m.state = MapState::FixedUp;
        Ok(m.inner)
    }

    /// 状态查询。
    pub fn state_of(&self, inner: u32) -> Option<MapState> {
        self.maps.iter().find(|m| m.inner == inner).map(|m| m.state)
    }

    /// 对外发布：未确认拒（对外动作红线）。
    pub fn publish(&mut self, id: u32, confirmed: bool) -> Result<(), u32> {
        if !confirmed {
            self.pending.push(id);
            return Err(XC_UNCONFIRM);
        }
        self.confirmed += 1;
        Ok(())
    }

    /// 待发布积压。
    pub fn backlog(&self) -> usize {
        self.pending.len()
    }

    /// 映射在账数。
    pub fn len(&self) -> usize {
        self.maps.len()
    }
}

pub fn run_veb334_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0334");
    // 族零：四要素模板。
    s.add("全空拒", ComBook::tpl_ok(0) == false, "零要素");
    s.add("缺实际拒", ComBook::tpl_ok(F_ENV | F_STEP | F_EXPECT) == false, "缺 F_ACTUAL");
    s.add("缺期望拒", ComBook::tpl_ok(F_ENV | F_STEP | F_ACTUAL) == false, "缺 F_EXPECT");
    s.add("齐备过", ComBook::tpl_ok(F_ALL), "四要素全");
    let cb = ComBook::new();
    s.add("提交拒", matches!(cb.submit(1, F_ENV | F_STEP), Err(XC_MISSING)), "话术不全");
    s.add("提交过", cb.submit(1, F_ALL).is_ok(), "可提交");
    // 族一：双账映射。
    let mut cb2 = ComBook::new();
    s.add("零号拒", cb2.map(0, 100) == Err(XC_MISSING), "内部号空");
    s.add("零上游拒", cb2.map(1, 0) == Err(XC_MISSING), "上游号空");
    s.add("建映过", cb2.map(7, 100).is_ok(), "内 7 ↔ 外 100");
    s.add("重复拒", cb2.map(7, 100) == Err(XC_DUP), "一条一映");
    s.add("在账一", cb2.len() == 1, "一映");
    // 族二：上游拒绝。
    s.add("查无拒改", cb2.mark_reject(99) == Err(XC_NOMAP), "无此内号");
    s.add("拒登过", cb2.mark_reject(7).is_ok(), "登记原因");
    s.add("态在拒", cb2.state_of(7) == Some(MapState::Rejected), "策略调整");
    s.add("拒计一", cb2.rejected == 1, "拒账");
    // 族三：上游已修销账。
    let _ = cb2.map(8, 200);
    s.add("销账过", cb2.sync_close(200) == Ok(8), "外 200 修");
    s.add("态在修", cb2.state_of(8) == Some(MapState::FixedUp), "双账同步");
    s.add("重销拒", cb2.sync_close(200) == Err(XC_NOMAP), "已销");
    s.add("查无销拒", cb2.sync_close(999) == Err(XC_NOMAP), "无此外号");
    // 族四：发布前确认。
    let mut cb3 = ComBook::new();
    s.add("未确拒", cb3.publish(1, false) == Err(XC_UNCONFIRM), "红线");
    s.add("积压一", cb3.backlog() == 1, "待发布");
    s.add("确后过", cb3.publish(2, true).is_ok(), "确认后发");
    s.add("确计一", cb3.confirmed == 1, "发账");
    // 族五：混合账。
    s.add("账三", cb2.len() == 2, "7,8 两映");
    s.add("魔数核", CI_SEG_BASE == 0x31C80, "段基址");
    s
}
