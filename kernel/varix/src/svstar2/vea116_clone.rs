//! VE-F0116 · 跨卡资源克隆与同步副本（VE-A 域 · A05 多 GPU 与异构组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0116`
//!
//! **职责定位（锚点原文）**：只读资源的跨卡副本（每卡一份读局部化），副本一致性
//! 协议（源更新→副本同步），副本内存成本核算。
//!
//! **铁律**：
//! 1. **读局部化**——每卡持一份只读副本。
//! 2. **一致性协议**——源更新必同步副本。
//! 3. **成本核算**——副本内存显式计量。
//! 4. **过期检出**——副本过期可查。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BA（资源克隆）。
pub const SEG_BASE: u16 = 0xE8BA;

/// 副本过期。
pub const XC_REPLICA_STALE: u16 = SEG_BASE | 0x01;
/// 成本超支。
pub const XC_COST_OVER: u16 = SEG_BASE | 0x02;
/// 源损坏。
pub const XC_SOURCE_CORRUPT: u16 = SEG_BASE | 0x03;
/// 无副本。
pub const XC_NO_REPLICA: u16 = SEG_BASE | 0x04;

/// 资源源。
#[derive(Clone, Debug)]
pub struct ResSource {
    pub id: u32,
    pub bytes: u64,
    /// 源版本。
    pub version: u64,
    /// 指纹。
    pub fingerprint: u32,
    /// 健康。
    pub alive: bool,
}

impl ResSource {
    pub fn compute_fp(&self) -> u32 {
        self.id ^ (self.bytes as u32).rotate_left(7) ^ (self.version as u32)
    }
}

/// 副本。
#[derive(Clone, Debug)]
pub struct Replica {
    pub source_id: u32,
    pub gpu_id: u16,
    /// 同步到的源版本。
    pub synced_version: u64,
    pub bytes: u64,
}

/// 克隆同步器。
pub struct CloneSync {
    sources: Vec<ResSource>,
    replicas: Vec<Replica>,
    /// 副本内存预算（字节）。
    budget: u64,
    /// 已用。
    used: u64,
    last_code: u16,
    /// 同步次数。
    syncs: u32,
    /// 裁剪次数。
    prunes: u32,
    /// 重建次数。
    rebuilds: u32,
}

impl CloneSync {
    pub fn new(budget: u64) -> Result<Self, u16> {
        if budget == 0 {
            return Err(XC_COST_OVER);
        }
        Ok(CloneSync {
            sources: Vec::new(),
            replicas: Vec::new(),
            budget,
            used: 0,
            last_code: 0,
            syncs: 0,
            prunes: 0,
            rebuilds: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn add_source(&mut self, src: ResSource) -> Result<(), u16> {
        if !src.alive {
            self.last_code = XC_SOURCE_CORRUPT;
            return Err(XC_SOURCE_CORRUPT);
        }
        self.sources.retain(|s| s.id != src.id);
        self.sources.push(src);
        self.last_code = 0;
        Ok(())
    }

    /// 建副本（读局部化）。
    pub fn replicate(&mut self, source_id: u32, gpu_id: u16) -> Result<(), u16> {
        let src = match self.sources.iter().find(|s| s.id == source_id) {
            Some(s) => s.clone(),
            None => {
                self.last_code = XC_NO_REPLICA;
                return Err(XC_NO_REPLICA);
            }
        };
        if self.used + src.bytes > self.budget {
            self.last_code = XC_COST_OVER;
            return Err(XC_COST_OVER);
        }
        self.used += src.bytes;
        self.replicas.push(Replica {
            source_id,
            gpu_id,
            synced_version: src.version,
            bytes: src.bytes,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 源更新 → 副本同步（一致性协议）。
    pub fn sync_source(&mut self, source_id: u32, new_version: u64) -> Result<u32, u16> {
        let fp_check = match self.sources.iter_mut().find(|s| s.id == source_id) {
            Some(s) => {
                s.version = new_version;
                s.alive = true;
                s.compute_fp()
            }
            None => {
                self.last_code = XC_NO_REPLICA;
                return Err(XC_NO_REPLICA);
            }
        };
        let _ = fp_check;
        let mut n = 0;
        for r in self.replicas.iter_mut() {
            if r.source_id == source_id {
                r.synced_version = new_version;
                n += 1;
            }
        }
        self.syncs += n;
        self.last_code = 0;
        Ok(n)
    }

    /// 过期检出。
    pub fn check_stale(&mut self) -> Vec<u32> {
        let mut stale = Vec::new();
        for r in &self.replicas {
            if let Some(s) = self.sources.iter().find(|s| s.id == r.source_id) {
                if r.synced_version < s.version {
                    stale.push(r.source_id);
                }
            }
        }
        if !stale.is_empty() {
            self.last_code = XC_REPLICA_STALE;
        }
        stale
    }

    /// 成本超支裁剪（裁掉最大副本）。
    pub fn prune_to_budget(&mut self) -> Result<(), u16> {
        while self.used > self.budget && !self.replicas.is_empty() {
            // 找最大。
            let mut max_i = 0;
            for i in 1..self.replicas.len() {
                if self.replicas[i].bytes > self.replicas[max_i].bytes {
                    max_i = i;
                }
            }
            let removed = self.replicas.remove(max_i);
            self.used -= removed.bytes;
            self.prunes += 1;
        }
        self.last_code = 0;
        Ok(())
    }

    /// 源损坏重建。
    pub fn rebuild_source(&mut self, source_id: u32) -> Result<(), u16> {
        match self.sources.iter_mut().find(|s| s.id == source_id) {
            Some(s) => {
                s.alive = true;
                s.version += 1;
                self.rebuilds += 1;
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = XC_SOURCE_CORRUPT;
                Err(XC_SOURCE_CORRUPT)
            }
        }
    }

    pub fn replica_count(&self) -> usize {
        self.replicas.len()
    }

    pub fn used_bytes(&self) -> u64 {
        self.used
    }

    pub fn syncs(&self) -> u32 {
        self.syncs
    }

    pub fn prunes(&self) -> u32 {
        self.prunes
    }

    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }
}

/// 自检：读局部化、一致性协议、成本核算、过期检出。
pub fn run_vea116_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn src(id: u32, bytes: u64) -> ResSource {
        let s = ResSource {
            id,
            bytes,
            version: 1,
            fingerprint: 0,
            alive: true,
        };
        ResSource {
            fingerprint: s.compute_fp(),
            ..s
        }
    }

    let mut set = CheckSet::new("vea116_clone");

    // C116-01 读局部化建副本
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 100));
        let r = c.replicate(1, 0);
        chk(&mut set, "C116-01 建副本", r.is_ok() && c.replica_count() == 1, "副本失败");
    }

    // C116-02 一致性协议同步
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 100));
        let _ = c.replicate(1, 0);
        let _ = c.replicate(1, 1);
        let n = c.sync_source(1, 2);
        chk(&mut set, "C116-02 一致性", n == Ok(2), "同步失败");
    }

    // C116-03 过期检出
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 100));
        let _ = c.replicate(1, 0);
        let _ = c.sync_source(1, 5); // 源先更新
        // 手动制造过期。
        if let Some(r) = c.replicas.first_mut() {
            r.synced_version = 1;
        }
        let stale = c.check_stale();
        chk(&mut set, "C116-03 过期检出", stale.contains(&1), "过期漏检");
    }

    // C116-04 成本超支拒绝
    {
        let mut c = CloneSync::new(100).unwrap();
        let _ = c.add_source(src(1, 200));
        let r = c.replicate(1, 0);
        chk(
            &mut set,
            "C116-04 成本拒",
            r.is_err() && c.last_code() == XC_COST_OVER,
            "超支被放",
        );
    }

    // C116-05 裁剪回收
    {
        let mut c = CloneSync::new(300).unwrap();
        let _ = c.add_source(src(1, 100));
        let _ = c.add_source(src(2, 100));
        let _ = c.add_source(src(3, 100));
        let _ = c.replicate(1, 0);
        let _ = c.replicate(2, 0);
        let _ = c.replicate(3, 0);
        // 手工超支。
        c.used = 350;
        let _ = c.prune_to_budget();
        chk(&mut set, "C116-05 裁剪", c.prunes() >= 1 && c.used_bytes() <= 300, "未裁剪");
    }

    // C116-06 源损坏重建
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 100));
        if let Some(s) = c.sources.first_mut() {
            s.alive = false;
        }
        let r = c.rebuild_source(1);
        chk(&mut set, "C116-06 重建", r.is_ok() && c.rebuilds() == 1, "重建失败");
    }

    // C116-07 无源拒绝副本
    {
        let mut c = CloneSync::new(10000).unwrap();
        let r = c.replicate(99, 0);
        chk(&mut set, "C116-07 无源拒", r.is_err(), "无源被放");
    }

    // C116-08 零预算拒绝
    {
        let r = CloneSync::new(0);
        chk(&mut set, "C116-08 零预算拒", r.is_err(), "零预算被放");
    }

    // C116-09 成本计量准确
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 500));
        let _ = c.replicate(1, 0);
        chk(&mut set, "C116-09 计量", c.used_bytes() == 500, "计量错");
    }

    // C116-10 无过期不误报
    {
        let mut c = CloneSync::new(10000).unwrap();
        let _ = c.add_source(src(1, 100));
        let _ = c.replicate(1, 0);
        let stale = c.check_stale();
        chk(&mut set, "C116-10 不误报", stale.is_empty(), "误报过期");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea116_all_green() {
        let s = run_vea116_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
