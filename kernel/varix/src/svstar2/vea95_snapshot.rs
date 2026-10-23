//! VE-F0095 · 句柄表持久化快照（VE-A 域 · A04b 诊断·降级·安全组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0095`
//!
//! **职责定位（锚点原文）**：句柄表的定期持久化快照（崩溃后可恢复分析），
//! 快照增量（只存差异省体积），快照损坏检测。
//!
//! **铁律**：
//! 1. **定期快照**——按周期拍全量基线，期间增量补差。
//! 2. **增量差异**——只存差异，失配即全量回退。
//! 3. **损坏检测**——校验和不符即判损坏并重建。
//! 4. **崩溃分析**——快照可独立读出，供崩溃后恢复分析。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A6（句柄快照）。
pub const SEG_BASE: u16 = 0xE8A6;

/// 快照损坏。
pub const XC_CORRUPT: u16 = SEG_BASE | 0x01;
/// 增量失配（基线版本对不上）。
pub const XC_DELTA_MISMATCH: u16 = SEG_BASE | 0x02;
/// 快照超期。
pub const XC_STALE: u16 = SEG_BASE | 0x03;
/// 句柄表空，无法拍快照。
pub const XC_EMPTY_TABLE: u16 = SEG_BASE | 0x04;
/// 序号倒退。
pub const XC_SEQ_REGRESS: u16 = SEG_BASE | 0x05;

/// 句柄条目（快照单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandleEntry {
    pub id: u32,
    pub kind: u16,
    pub flags: u16,
    pub gen: u32,
}

impl HandleEntry {
    /// 条目指纹（增量比对用）。
    pub fn fingerprint(&self) -> u32 {
        (self.id as u32)
            ^ ((self.kind as u32) << 16)
            ^ ((self.flags as u32) << 8)
            ^ self.gen.rotate_left(7)
    }
}

/// 全量快照。
#[derive(Clone, Debug)]
pub struct FullSnapshot {
    pub seq: u64,
    pub base_version: u64,
    pub entries: Vec<HandleEntry>,
    pub checksum: u32,
}

impl FullSnapshot {
    fn compute_checksum(entries: &[HandleEntry]) -> u32 {
        let mut acc: u32 = 0xA5A5_5A5A;
        for e in entries {
            acc = acc.rotate_left(3) ^ e.fingerprint();
        }
        acc ^ (entries.len() as u32).rotate_left(11)
    }

    pub fn verify(&self) -> bool {
        Self::compute_checksum(&self.entries) == self.checksum
    }
}

/// 增量快照（只存差异）。
#[derive(Clone, Debug)]
pub struct DeltaSnapshot {
    pub seq: u64,
    /// 基于哪个基线版本。
    pub base_version: u64,
    /// 变更条目（新增/修改）。
    pub changed: Vec<HandleEntry>,
    /// 删除的句柄 id。
    pub removed: Vec<u32>,
    pub checksum: u32,
}

impl DeltaSnapshot {
    fn compute_checksum(base: u64, changed: &[HandleEntry], removed: &[u32]) -> u32 {
        let mut acc: u32 = 0x1234_ABCD ^ (base as u32);
        for e in changed {
            acc = acc.rotate_left(5) ^ e.fingerprint();
        }
        for &r in removed {
            acc = acc.rotate_left(7) ^ r;
        }
        acc
    }

    pub fn verify(&self) -> bool {
        Self::compute_checksum(self.base_version, &self.changed, &self.removed) == self.checksum
    }
}

/// 快照存储槽。
#[derive(Clone, Debug)]
pub enum SnapSlot {
    Full(FullSnapshot),
    Delta(DeltaSnapshot),
}

/// 持久化快照管理器。
pub struct SnapshotStore {
    /// 当前句柄表镜像（自持，供 diff）。
    table: Vec<HandleEntry>,
    /// 基线全量。
    base: Option<FullSnapshot>,
    /// 增量链。
    deltas: Vec<DeltaSnapshot>,
    /// 基线版本号。
    base_version: u64,
    seq: u64,
    /// 周期阈值（多少次 tick 拍一次全量）。
    period: u32,
    tick: u32,
    /// 超期阈值（tick 数）。
    stale_after: u32,
    last_full_tick: u32,
    last_code: u16,
    /// 全量拍取次数。
    full_count: u32,
    /// 增量拍取次数。
    delta_count: u32,
    /// 检出损坏次数。
    corrupt_hits: u32,
}

impl SnapshotStore {
    pub fn new(period: u32, stale_after: u32) -> Self {
        SnapshotStore {
            table: Vec::new(),
            base: None,
            deltas: Vec::new(),
            base_version: 0,
            seq: 0,
            period: if period == 0 { 8 } else { period },
            tick: 0,
            stale_after: if stale_after == 0 { 64 } else { stale_after },
            last_full_tick: 0,
            last_code: 0,
            full_count: 0,
            delta_count: 0,
            corrupt_hits: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 载入句柄表（全量替换镜像）。
    pub fn load_table(&mut self, entries: Vec<HandleEntry>) -> Result<(), u16> {
        if entries.is_empty() {
            self.last_code = XC_EMPTY_TABLE;
            return Err(XC_EMPTY_TABLE);
        }
        self.table = entries;
        self.last_code = 0;
        Ok(())
    }

    /// 拍全量基线。
    pub fn snapshot_full(&mut self) -> Result<u64, u16> {
        if self.table.is_empty() {
            self.last_code = XC_EMPTY_TABLE;
            return Err(XC_EMPTY_TABLE);
        }
        self.seq += 1;
        self.base_version += 1;
        let checksum = FullSnapshot::compute_checksum(&self.table);
        let snap = FullSnapshot {
            seq: self.seq,
            base_version: self.base_version,
            entries: self.table.clone(),
            checksum,
        };
        self.base = Some(snap);
        self.deltas.clear();
        self.full_count += 1;
        self.last_full_tick = self.tick;
        self.last_code = 0;
        Ok(self.base_version)
    }

    /// 拍增量（相对当前基线）。
    pub fn snapshot_delta(&mut self) -> Result<u64, u16> {
        let base = match &self.base {
            Some(b) => b.clone(),
            None => {
                // 无基线拍增量 = 失配，回退要求全量。
                self.last_code = XC_DELTA_MISMATCH;
                return Err(XC_DELTA_MISMATCH);
            }
        };
        // diff：找出相对基线变更/删除。
        let mut changed = Vec::new();
        for e in &self.table {
            match base.entries.iter().find(|b| b.id == e.id) {
                Some(b) if b == e => {}
                _ => changed.push(*e),
            }
        }
        let mut removed = Vec::new();
        for b in &base.entries {
            if !self.table.iter().any(|e| e.id == b.id) {
                removed.push(b.id);
            }
        }
        self.seq += 1;
        let checksum = DeltaSnapshot::compute_checksum(base.base_version, &changed, &removed);
        let d = DeltaSnapshot {
            seq: self.seq,
            base_version: base.base_version,
            changed,
            removed,
            checksum,
        };
        self.deltas.push(d);
        self.delta_count += 1;
        self.last_code = 0;
        Ok(self.base_version)
    }

    /// 周期 tick：到期拍全量，否则拍增量。
    pub fn tick_persist(&mut self) -> Result<u64, u16> {
        self.tick += 1;
        if self.tick.wrapping_sub(self.last_full_tick) >= self.period {
            self.snapshot_full()
        } else {
            self.snapshot_delta()
        }
    }

    /// 损坏检测：基线 + 全部增量逐一验签。
    pub fn detect_corruption(&mut self) -> bool {
        let mut corrupt = false;
        if let Some(b) = &self.base {
            if !b.verify() {
                corrupt = true;
            }
        }
        for d in &self.deltas {
            if !d.verify() {
                corrupt = true;
            }
        }
        if corrupt {
            self.corrupt_hits += 1;
            self.last_code = XC_CORRUPT;
        }
        corrupt
    }

    /// 损坏后重建：清增量链，强制重拍全量。
    pub fn rebuild(&mut self) -> Result<u64, u16> {
        self.deltas.clear();
        self.base = None;
        self.snapshot_full()
    }

    /// 增量失配检测：增量链的 base_version 必须与当前基线一致。
    pub fn verify_delta_chain(&mut self) -> bool {
        let base_ver = match &self.base {
            Some(b) => b.base_version,
            None => {
                self.last_code = XC_DELTA_MISMATCH;
                return false;
            }
        };
        for d in &self.deltas {
            if d.base_version != base_ver {
                self.last_code = XC_DELTA_MISMATCH;
                return false;
            }
            if !d.verify() {
                self.last_code = XC_CORRUPT;
                return false;
            }
        }
        true
    }

    /// 增量失配回退：丢增量，保留基线（下轮补全量）。
    pub fn fallback_to_base(&mut self) -> Result<(), u16> {
        if self.base.is_none() {
            self.last_code = XC_DELTA_MISMATCH;
            return Err(XC_DELTA_MISMATCH);
        }
        self.deltas.clear();
        self.last_code = 0;
        Ok(())
    }

    /// 超期判定：距上次全量超过阈值即超期。
    pub fn is_stale(&self) -> bool {
        self.tick.wrapping_sub(self.last_full_tick) >= self.stale_after
    }

    /// 超期重拍。
    pub fn refresh_if_stale(&mut self) -> Result<u64, u16> {
        if self.is_stale() {
            self.last_code = XC_STALE;
            self.snapshot_full()
        } else {
            Ok(self.base_version)
        }
    }

    /// 崩溃分析导出：还原「基线 ⊕ 增量链」得到恢复视图。
    pub fn recover_view(&self) -> Result<Vec<HandleEntry>, u16> {
        let base = match &self.base {
            Some(b) => b,
            None => {
                return Err(XC_EMPTY_TABLE);
            }
        };
        if !base.verify() {
            return Err(XC_CORRUPT);
        }
        let mut view = base.entries.clone();
        for d in &self.deltas {
            if !d.verify() {
                return Err(XC_CORRUPT);
            }
            if d.base_version != base.base_version {
                return Err(XC_DELTA_MISMATCH);
            }
            for r in &d.removed {
                view.retain(|e| e.id != *r);
            }
            for c in &d.changed {
                view.retain(|e| e.id != c.id);
                view.push(*c);
            }
        }
        Ok(view)
    }

    pub fn full_count(&self) -> u32 {
        self.full_count
    }

    pub fn delta_count(&self) -> u32 {
        self.delta_count
    }

    pub fn corrupt_hits(&self) -> u32 {
        self.corrupt_hits
    }

    pub fn base_version(&self) -> u64 {
        self.base_version
    }

    /// 供持久化层枚举槽位。
    pub fn slots(&self) -> Vec<SnapSlot> {
        let mut out = Vec::new();
        if let Some(b) = &self.base {
            out.push(SnapSlot::Full(b.clone()));
        }
        for d in &self.deltas {
            out.push(SnapSlot::Delta(d.clone()));
        }
        out
    }
}

/// 自检：定期快照、增量差异、损坏检测、崩溃分析。
pub fn run_vea95_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn sample_table() -> Vec<HandleEntry> {
        vec![
            HandleEntry {
                id: 1,
                kind: 1,
                flags: 0,
                gen: 1,
            },
            HandleEntry {
                id: 2,
                kind: 2,
                flags: 1,
                gen: 1,
            },
            HandleEntry {
                id: 3,
                kind: 1,
                flags: 0,
                gen: 2,
            },
        ]
    }

    let mut set = CheckSet::new("vea95_snapshot");

    // C95-01 全量快照可建可验
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let r = st.snapshot_full();
        let ok = r.is_ok() && st.detect_corruption() == false;
        chk(&mut set, "C95-01 全量快照", ok, "全量未过验");
    }

    // C95-02 增量只存差异
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        // 只改一条 + 删一条。
        let mut t = sample_table();
        t[0].gen = 9;
        t.retain(|e| e.id != 3);
        let _ = st.load_table(t);
        let _ = st.snapshot_delta();
        let ok = st.delta_count() == 1 && st.slots().len() == 2;
        chk(&mut set, "C95-02 增量差异", ok, "增量未落");
    }

    // C95-03 无基线拍增量显性失配
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let r = st.snapshot_delta();
        chk(
            &mut set,
            "C95-03 无基线失配",
            r.is_err() && st.last_code() == XC_DELTA_MISMATCH,
            "失配未显",
        );
    }

    // C95-04 损坏检测命中
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        // 篡改基线。
        if let Some(b) = st.base.as_mut() {
            b.entries[0].gen = 999;
        }
        let hit = st.detect_corruption();
        chk(&mut set, "C95-04 损坏检测", hit && st.corrupt_hits() == 1, "损坏漏检");
    }

    // C95-05 损坏后重建
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        if let Some(b) = st.base.as_mut() {
            b.checksum ^= 0xFFFF;
        }
        let r = st.rebuild();
        let ok = r.is_ok() && !st.detect_corruption();
        chk(&mut set, "C95-05 损坏重建", ok, "重建后仍坏");
    }

    // C95-06 崩溃分析还原视图
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        let mut t = sample_table();
        t[0].gen = 7;
        t.push(HandleEntry {
            id: 4,
            kind: 3,
            flags: 0,
            gen: 1,
        });
        let _ = st.load_table(t);
        let _ = st.snapshot_delta();
        let view = st.recover_view();
        let ok = view.is_ok() && view.unwrap().len() == 4;
        chk(&mut set, "C95-06 崩溃分析还原", ok, "还原失败");
    }

    // C95-07 超期重拍
    {
        // 期望手算：period=4 → tick4 拍全量（last_full=4）；stale_after=3 →
        // tick7 时 7-4=3 判超期；tick7 与 tick4 间不触发新全量（3<4）。
        let mut st = SnapshotStore::new(4, 3);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        for _ in 0..7 {
            let _ = st.tick_persist();
        }
        // 取证在变更操作之前：is_stale 先于 refresh 求值。
        let ok = st.is_stale() && st.refresh_if_stale().is_ok() && st.full_count() >= 2;
        chk(&mut set, "C95-07 超期重拍", ok, "超期未重拍");
    }

    // C95-08 增量链校验捕获版本失配
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        let _ = st.snapshot_delta();
        // 换基线（版本抬升）后旧增量失配。
        let _ = st.snapshot_full();
        // 旧增量已被 clear；重新制造失配：手工改 base_version。
        let _ = st.snapshot_delta();
        if let Some(d) = st.deltas.last_mut() {
            d.base_version = 999;
        }
        let ok = !st.verify_delta_chain() && st.last_code() == XC_DELTA_MISMATCH;
        chk(&mut set, "C95-08 链失配捕获", ok, "失配漏检");
    }

    // C95-09 失配回退保基线
    {
        let mut st = SnapshotStore::new(8, 64);
        let _ = st.load_table(sample_table());
        let _ = st.snapshot_full();
        let _ = st.snapshot_delta();
        let r = st.fallback_to_base();
        let ok = r.is_ok() && st.slots().len() == 1 && st.recover_view().is_ok();
        chk(&mut set, "C95-09 失配回退", ok, "回退失败");
    }

    // C95-10 空表拍快照显性拒绝
    {
        let mut st = SnapshotStore::new(8, 64);
        let r = st.snapshot_full();
        chk(
            &mut set,
            "C95-10 空表拒绝",
            r.is_err() && st.last_code() == XC_EMPTY_TABLE,
            "空表被放行",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea95_all_green() {
        let s = run_vea95_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
