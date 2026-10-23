//! VE-F0107 · 跨适配器表面迁移协议（VE-A 域 · A05 多 GPU 与异构组 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0107`
//!
//! **职责定位（锚点原文）**：表面在卡间迁移的完整协议（导出→传输→导入→验证四段），
//! 迁移原子性（半迁移状态不存在），迁移失败的回退（留在原卡降级运行）；含迁移的
//! 带宽预算（迁移不挤占渲染带宽）。
//!
//! **铁律**：
//! 1. **四段协议**——导出→传输→导入→验证，缺段即拒。
//! 2. **原子迁移**——半迁移状态不存在，提交点单一切换。
//! 3. **失败回退**——留原卡降级运行，不丢表面。
//! 4. **验证必过**——验证失败即重迁，不带病切换。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B1（表面迁移）。
pub const SEG_BASE: u16 = 0xE8B1;

/// 协议版本漂移。
pub const XC_PROTO_DRIFT: u16 = SEG_BASE | 0x01;
/// 迁移失败。
pub const XC_MIGRATE_FAIL: u16 = SEG_BASE | 0x02;
/// 验证失败。
pub const XC_VERIFY_FAIL: u16 = SEG_BASE | 0x03;
/// 带宽预算不足。
pub const XC_BANDWIDTH: u16 = SEG_BASE | 0x04;
/// 半迁移状态违规。
pub const XC_HALF_STATE: u16 = SEG_BASE | 0x05;
/// 未知阶段。
pub const XC_BAD_STAGE: u16 = SEG_BASE | 0x06;

/// 协议当前版本。
pub const PROTO_VERSION: u16 = 0x0101;

/// 迁移四段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigStage {
    /// 导出（源卡句柄导出）。
    Export,
    /// 传输（跨卡搬运）。
    Transfer,
    /// 导入（目标卡句柄导入）。
    Import,
    /// 验证（完整性与几何核验）。
    Verify,
}

impl MigStage {
    /// 阶段序号（0..3）。
    pub fn ordinal(&self) -> u8 {
        match self {
            MigStage::Export => 0,
            MigStage::Transfer => 1,
            MigStage::Import => 2,
            MigStage::Verify => 3,
        }
    }

    /// 下一段。
    pub fn next(&self) -> Option<MigStage> {
        match self {
            MigStage::Export => Some(MigStage::Transfer),
            MigStage::Transfer => Some(MigStage::Import),
            MigStage::Import => Some(MigStage::Verify),
            MigStage::Verify => None,
        }
    }
}

/// 表面描述（迁移单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceDesc {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub format: u16,
    /// 字节大小。
    pub size: u64,
    /// 校验指纹。
    pub fingerprint: u32,
}

impl SurfaceDesc {
    /// 指纹重算（验证用）。
    pub fn compute_fingerprint(&self) -> u32 {
        self.id
            ^ self.width.rotate_left(3)
            ^ self.height.rotate_left(7)
            ^ ((self.format as u32) << 16)
            ^ (self.size as u32).rotate_left(11)
    }
}

/// 迁移结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrateOutcome {
    /// 迁移成功（表面已驻目标卡）。
    Migrated,
    /// 失败回退（留原卡降级）。
    FellBack,
    /// 验证失败重迁中。
    Retrying,
}

/// 迁移事务（原子：四段全过才提交）。
#[derive(Clone, Debug)]
pub struct MigrateTxn {
    pub surface: SurfaceDesc,
    pub src: u16,
    pub dst: u16,
    /// 已完成段位图（bit0..3）。
    stage_bits: u8,
    /// 当前段。
    pub stage: MigStage,
    /// 提交点：true=已原子切换。
    committed: bool,
    /// 回退标记。
    fell_back: bool,
}

impl MigrateTxn {
    pub fn new(surface: SurfaceDesc, src: u16, dst: u16) -> Self {
        MigrateTxn {
            surface,
            src,
            dst,
            stage_bits: 0,
            stage: MigStage::Export,
            committed: false,
            fell_back: false,
        }
    }

    /// 段完成登记。
    pub fn complete_stage(&mut self) -> Result<(), u16> {
        if self.committed {
            return Err(XC_HALF_STATE);
        }
        self.stage_bits |= 1 << self.stage.ordinal();
        match self.stage.next() {
            Some(n) => {
                self.stage = n;
                Ok(())
            }
            None => {
                // 四段全过：原子提交。
                self.committed = true;
                Ok(())
            }
        }
    }

    /// 跳段拒（相位错）。
    pub fn skip_to(&mut self, stage: MigStage) -> Result<(), u16> {
        if stage.ordinal() != self.stage.ordinal() + 1 && !(self.stage == MigStage::Verify) {
            return Err(XC_BAD_STAGE);
        }
        self.stage = stage;
        Ok(())
    }

    pub fn all_stages_done(&self) -> bool {
        self.stage_bits == 0x0F
    }

    pub fn is_committed(&self) -> bool {
        self.committed
    }

    pub fn is_fell_back(&self) -> bool {
        self.fell_back
    }

    pub fn mark_fallback(&mut self) {
        self.fell_back = true;
    }
}

/// 带宽预算（迁移不挤占渲染带宽）。
#[derive(Clone, Copy, Debug)]
pub struct BandwidthBudget {
    /// 总迁移带宽（字节/tick）。
    pub total: u64,
    /// 已用。
    pub used: u64,
    /// 渲染保留（不可被迁移挤占）。
    pub render_reserve: u64,
}

impl BandwidthBudget {
    pub fn new(total: u64, render_reserve: u64) -> Result<Self, u16> {
        if total == 0 || render_reserve >= total {
            return Err(XC_BANDWIDTH);
        }
        Ok(BandwidthBudget {
            total,
            used: 0,
            render_reserve,
        })
    }

    /// 可用于迁移的额度。
    pub fn available(&self) -> u64 {
        self.total - self.render_reserve - self.used
    }

    pub fn try_reserve(&mut self, bytes: u64) -> Result<(), u16> {
        if self.used + bytes > self.total - self.render_reserve {
            return Err(XC_BANDWIDTH);
        }
        self.used += bytes;
        Ok(())
    }

    pub fn release(&mut self, bytes: u64) {
        self.used = self.used.saturating_sub(bytes);
    }
}

/// 迁移管理器。
pub struct SurfaceMigrator {
    budget: BandwidthBudget,
    proto_version: u16,
    /// 进行中事务（最多一个——原子面）。
    inflight: Option<MigrateTxn>,
    /// 成功迁移计数。
    migrated: u32,
    /// 回退计数。
    fell_back: u32,
    /// 重迁计数。
    retries: u32,
    last_code: u16,
}

impl SurfaceMigrator {
    pub fn new(budget: BandwidthBudget, proto_version: u16) -> Result<Self, u16> {
        if proto_version != PROTO_VERSION {
            return Err(XC_PROTO_DRIFT);
        }
        Ok(SurfaceMigrator {
            budget,
            proto_version,
            inflight: None,
            migrated: 0,
            fell_back: 0,
            retries: 0,
            last_code: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 开迁移事务。
    pub fn begin(&mut self, surface: SurfaceDesc, src: u16, dst: u16) -> Result<(), u16> {
        if self.inflight.is_some() {
            self.last_code = XC_HALF_STATE;
            return Err(XC_HALF_STATE);
        }
        if src == dst {
            self.last_code = XC_BAD_STAGE;
            return Err(XC_BAD_STAGE);
        }
        // 带宽预占。
        self.budget.try_reserve(surface.size).map_err(|e| {
            self.last_code = e;
            e
        })?;
        self.inflight = Some(MigrateTxn::new(surface, src, dst));
        self.last_code = 0;
        Ok(())
    }

    /// 推进一段。
    pub fn advance(&mut self) -> Result<MigStage, u16> {
        let txn = match &mut self.inflight {
            Some(t) => t,
            None => {
                self.last_code = XC_BAD_STAGE;
                return Err(XC_BAD_STAGE);
            }
        };
        let done_stage = txn.stage;
        txn.complete_stage()?;
        self.last_code = 0;
        Ok(done_stage)
    }

    /// 验证段核验（指纹 + 几何）。
    pub fn verify(&mut self) -> Result<(), u16> {
        let txn = match &self.inflight {
            Some(t) => t,
            None => {
                self.last_code = XC_BAD_STAGE;
                return Err(XC_BAD_STAGE);
            }
        };
        if txn.stage != MigStage::Verify && !txn.is_committed() {
            self.last_code = XC_BAD_STAGE;
            return Err(XC_BAD_STAGE);
        }
        let expect = txn.surface.compute_fingerprint();
        if expect != txn.surface.fingerprint {
            self.last_code = XC_VERIFY_FAIL;
            return Err(XC_VERIFY_FAIL);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 提交（原子切换：验证过才换驻卡）。
    pub fn commit(&mut self) -> Result<MigrateOutcome, u16> {
        // 验证必过。
        self.verify()?;
        let txn = match &mut self.inflight {
            Some(t) => t,
            None => {
                self.last_code = XC_BAD_STAGE;
                return Err(XC_BAD_STAGE);
            }
        };
        if !txn.is_committed() {
            txn.complete_stage()?;
        }
        if !txn.all_stages_done() {
            self.last_code = XC_HALF_STATE;
            return Err(XC_HALF_STATE);
        }
        // 释放带宽。
        self.budget.release(txn.surface.size);
        self.migrated += 1;
        self.inflight = None;
        self.last_code = 0;
        Ok(MigrateOutcome::Migrated)
    }

    /// 失败回退：留原卡降级运行。
    pub fn fallback(&mut self) -> MigrateOutcome {
        if let Some(mut t) = self.inflight.take() {
            t.mark_fallback();
            self.budget.release(t.surface.size);
            self.fell_back += 1;
        }
        self.last_code = XC_MIGRATE_FAIL;
        MigrateOutcome::FellBack
    }

    /// 验证失败重迁。
    pub fn retry(&mut self) -> Result<(), u16> {
        if let Some(t) = self.inflight.as_mut() {
            // 重置到传输段（导出已过，从传输入手重迁）。
            t.stage = MigStage::Transfer;
            t.stage_bits = 1; // 仅导出段有效。
            t.committed = false;
            self.retries += 1;
            self.last_code = 0;
            Ok(())
        } else {
            self.last_code = XC_BAD_STAGE;
            Err(XC_BAD_STAGE)
        }
    }

    pub fn migrated(&self) -> u32 {
        self.migrated
    }

    pub fn fell_back(&self) -> u32 {
        self.fell_back
    }

    pub fn retries(&self) -> u32 {
        self.retries
    }

    pub fn budget(&self) -> &BandwidthBudget {
        &self.budget
    }

    /// 跳段尝试（只允许跳到相邻下一段；跨段拒 = XC_BAD_STAGE）。
    /// 语义：刻意暴露「跳段」入口供调用方显式探测，非法跳段必须显性报错
    /// 而不是静默推进——迁移相位错会让验证段核验到错误的中间态。
    pub fn skip_to(&mut self, stage: MigStage) -> Result<(), u16> {
        let txn = match self.inflight.as_mut() {
            Some(t) => t,
            None => {
                self.last_code = XC_BAD_STAGE;
                return Err(XC_BAD_STAGE);
            }
        };
        match txn.skip_to(stage) {
            Ok(()) => {
                self.last_code = 0;
                Ok(())
            }
            Err(e) => {
                self.last_code = e;
                Err(e)
            }
        }
    }

    pub fn proto_version(&self) -> u16 {
        self.proto_version
    }
}

/// 自检：四段协议、原子迁移、失败回退、验证必过。
pub fn run_vea107_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn surf() -> SurfaceDesc {
        let s = SurfaceDesc {
            id: 1,
            width: 256,
            height: 256,
            format: 1,
            size: 4096,
            fingerprint: 0,
        };
        SurfaceDesc {
            fingerprint: s.compute_fingerprint(),
            ..s
        }
    }

    fn mgr() -> SurfaceMigrator {
        SurfaceMigrator::new(BandwidthBudget::new(1_000_000, 100_000).unwrap(), PROTO_VERSION)
            .unwrap()
    }

    let mut set = CheckSet::new("vea107_migrate");

    // C107-01 协议版本拦截
    {
        let r = SurfaceMigrator::new(BandwidthBudget::new(1000, 100).unwrap(), 0x9999);
        chk(
            &mut set,
            "C107-01 版本拦截",
            r.err() == Some(XC_PROTO_DRIFT),
            "漂移被放行",
        );
    }

    // C107-02 四段全过提交
    {
        let mut m = mgr();
        let _ = m.begin(surf(), 0, 1);
        let _ = m.advance(); // Export
        let _ = m.advance(); // Transfer
        let _ = m.advance(); // Import
        let _ = m.advance(); // Verify
        let r = m.commit();
        chk(
            &mut set,
            "C107-02 四段提交",
            r == Ok(MigrateOutcome::Migrated) && m.migrated() == 1,
            "提交失败",
        );
    }

    // C107-03 半迁移不存在（提交前无驻卡变更）
    {
        let mut m = mgr();
        let _ = m.begin(surf(), 0, 1);
        let _ = m.advance();
        let _ = m.advance();
        // 未到验证即 commit 应拒。
        let r = m.commit();
        chk(
            &mut set,
            "C107-03 原子性",
            r.is_err() && m.migrated() == 0,
            "半迁移落地",
        );
    }

    // C107-04 验证失败拒提交
    {
        let mut m = mgr();
        let mut s = surf();
        s.fingerprint ^= 0xFFFF; // 坏指纹
        let _ = m.begin(s, 0, 1);
        for _ in 0..4 {
            let _ = m.advance();
        }
        let r = m.commit();
        chk(
            &mut set,
            "C107-04 验证必过",
            r.is_err() && m.last_code() == XC_VERIFY_FAIL,
            "带病提交",
        );
    }

    // C107-05 失败回退留原卡
    {
        let mut m = mgr();
        let _ = m.begin(surf(), 0, 1);
        let out = m.fallback();
        chk(&mut set, "C107-05 失败回退", out == MigrateOutcome::FellBack && m.fell_back() == 1, "未回退");
    }

    // C107-06 验证失败重迁
    {
        let mut m = mgr();
        let mut s = surf();
        s.fingerprint ^= 0xFF;
        let _ = m.begin(s, 0, 1);
        for _ in 0..4 {
            let _ = m.advance();
        }
        let _ = m.commit();
        let r = m.retry();
        chk(&mut set, "C107-06 重迁", r.is_ok() && m.retries() == 1, "未重迁");
    }

    // C107-07 带宽不挤占渲染
    {
        let b = BandwidthBudget::new(1000, 400);
        let ok = b.is_ok() && b.unwrap().available() == 600;
        chk(&mut set, "C107-07 带宽保留", ok, "渲染带宽被挤");
    }

    // C107-08 带宽不足拒迁移
    {
        let mut m = SurfaceMigrator::new(
            BandwidthBudget::new(1000, 800).unwrap(),
            PROTO_VERSION,
        )
        .unwrap();
        let mut s = surf();
        s.size = 500; // 超可用 200
        let r = m.begin(s, 0, 1);
        chk(
            &mut set,
            "C107-08 带宽拒绝",
            r.is_err() && m.last_code() == XC_BANDWIDTH,
            "超额放行",
        );
    }

    // C107-09 跳段拒绝
    {
        let mut m = mgr();
        let _ = m.begin(surf(), 0, 1);
        let r = m.skip_to(MigStage::Verify);
        chk(&mut set, "C107-09 跳段拒", r.is_err() && m.last_code() == XC_BAD_STAGE, "跳段被放");
    }

    // C107-10 同卡迁移拒绝
    {
        let mut m = mgr();
        let r = m.begin(surf(), 3, 3);
        chk(&mut set, "C107-10 同卡拒", r.is_err(), "同卡被放");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea107_all_green() {
        let s = run_vea107_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
