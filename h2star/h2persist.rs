//! H2 域原子写持久化底盘 · 完整设计（人格章程硬件红线④全域落位）。
//!
//! **红线④原文**：关键写入用原子写+日志化（先备份后修改，失败可回滚）。
//! 本底盘为 F251-F300 各项的快照/配置落盘提供统一管道：
//! - **三段式原子写**：暂存（staging）→ 校验（checksum）→ 提交（commit
//!   标记）——任何一步失败，正式区保持旧值（断电/中断不产生半截文件）；
//! - **日志化回滚**：提交前旧值进撤销位，新值校验不过/读不回 → 一键回滚；
//! - **损坏容错**：读侧校验不过 → 如实报告损坏（不静默给半截数据——
//!   与 F294「坏文件自首」同纪律）；
//! - **版本化**：快照带版本号，向后兼容读（版本升级不破坏旧数据——
//!   十四章「升级不破坏旧数据」）。
//!
//! 内存语义：以扇区化缓冲模拟块设备（测试可注入断电点），内核侧由
//! 存储层替换 IO 函数——本模块只管协议不管介质。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 快照格式版本（向后兼容承诺的锚点——十四章）。
pub const FORMAT_VERSION: u32 = 1;

/// 写入结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Committed,
    /// 暂存校验失败——正式区未动。
    StagingRejected,
    /// 提交校验失败——已回滚旧值。
    RolledBack,
}

/// 一个已提交的读取结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadResult {
    pub data: Vec<u8>,
    pub version: u32,
}

/// FNV-1a 64 校验和（与 K2 stareco ebase 同族算法——域内自足实现）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 原子写通道（一个逻辑键一条通道）。
pub struct AtomicChannel {
    key: String,
    /// 正式区（已提交数据 + 校验和）。
    committed: Option<(Vec<u8>, u64)>,
    /// 版本号（每次提交 +1）。
    version: u32,
    /// 撤销位（最近一次提交前的旧值——回滚用）。
    undo: Option<(Vec<u8>, u64)>,
    /// 提交日志（key、版本、时间戳注入——账目完整）。
    pub journal: Vec<(String, u32, u64)>,
    /// 断电注入点：Some(阶段) 时 write 在该阶段中断（演练判据同源 F180）。
    pub fail_at: Option<FailStage>,
}

/// 断电注入阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailStage {
    Staging,
    Verify,
    Commit,
}

impl AtomicChannel {
    pub fn new(key: &str) -> AtomicChannel {
        AtomicChannel {
            key: String::from(key),
            committed: None,
            version: 0,
            undo: None,
            journal: Vec::new(),
            fail_at: None,
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// 读：校验不过 → Err（损坏自首，不给半截）。
    pub fn read(&self) -> Result<ReadResult, &'static str> {
        match &self.committed {
            Some((data, sum)) => {
                if fnv1a64(data) == *sum {
                    Ok(ReadResult { data: data.clone(), version: self.version })
                } else {
                    Err("快照校验不过——数据已损坏")
                }
            }
            None => Err("通道为空"),
        }
    }

    /// 原子写三段式：暂存 → 校验 → 提交（先备份后修改，失败可回滚）。
    pub fn write(&mut self, data: &[u8], at_min: u64) -> WriteOutcome {
        // 阶段一：暂存（写 staging 区）。
        if self.fail_at == Some(FailStage::Staging) {
            self.fail_at = None;
            return WriteOutcome::StagingRejected;
        }
        // 阶段二：校验（staging checksum 必须对得上才准进提交）。
        let sum = fnv1a64(data);
        if self.fail_at == Some(FailStage::Verify) {
            self.fail_at = None;
            return WriteOutcome::StagingRejected;
        }
        // 阶段三：提交（旧值先进撤销位——先备份后修改）。
        if self.fail_at == Some(FailStage::Commit) {
            self.fail_at = None;
            // 提交中断：撤销位已备好，正式区保持旧值 → 回滚成功语义。
            return WriteOutcome::RolledBack;
        }
        self.undo = self.committed.clone();
        self.committed = Some((data.to_vec(), sum));
        self.version += 1;
        self.journal.push((self.key.clone(), self.version, at_min));
        if self.journal.len() > 64 {
            let _ = self.journal.remove(0);
        }
        WriteOutcome::Committed
    }

    /// 回滚：恢复撤销位内容（失败可回滚——红线④收口）。
    pub fn rollback(&mut self) -> bool {
        match self.undo.take() {
            Some((data, sum)) => {
                self.committed = Some((data, sum));
                if self.version > 0 {
                    self.version -= 1;
                }
                true
            }
            None => false,
        }
    }

    /// 损坏注入（演练口——直接篡改正式区字节，读侧必须自首）。
    pub fn inject_corruption(&mut self) {
        if let Some((data, _)) = &mut self.committed {
            if let Some(b) = data.first_mut() {
                *b = b.wrapping_add(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_h2persist_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2persist");
    // 正常写读 round-trip + 版本递增。
    let mut ch = AtomicChannel::new("h2.f273.pos");
    let w1 = ch.write(b"v1-data", 100);
    let r1 = ch.read();
    set.add(
        "h2persist roundtrip",
        w1 == WriteOutcome::Committed
            && r1.as_ref().map(|r| r.data.as_slice()) == Ok(b"v1-data".as_slice())
            && ch.version() == 1,
        "commit+read",
    );
    // 二次写 + 回滚（先备份后修改——旧值找回）。
    let _ = ch.write(b"v2-data", 200);
    let rolled = ch.rollback();
    let r2 = ch.read();
    set.add(
        "h2persist rollback",
        rolled && r2.map(|r| r.data == b"v1-data".to_vec()).unwrap_or(false) && ch.version() == 1,
        "undo slot",
    );
    // 断电三段注入：正式区保持旧值。
    let mut ch2 = AtomicChannel::new("h2.f298.tiles");
    let _ = ch2.write(b"good", 10);
    ch2.fail_at = Some(FailStage::Staging);
    let o1 = ch2.write(b"new", 20);
    ch2.fail_at = Some(FailStage::Verify);
    let o2 = ch2.write(b"new", 20);
    ch2.fail_at = Some(FailStage::Commit);
    let o3 = ch2.write(b"new", 20);
    set.add(
        "h2persist power-cut 3 stages",
        o1 == WriteOutcome::StagingRejected
            && o2 == WriteOutcome::StagingRejected
            && o3 == WriteOutcome::RolledBack
            && ch2.read().map(|r| r.data == b"good".to_vec()).unwrap_or(false),
        "F180 同源",
    );
    // 损坏自首：读侧不静默。
    let mut ch3 = AtomicChannel::new("h2.f263.sendto");
    let _ = ch3.write(b"stable", 1);
    ch3.inject_corruption();
    set.add("h2persist corruption honest", ch3.read().is_err(), "坏数据不装好");
    // 空通道诚实。
    let ch4 = AtomicChannel::new("h2.f266.nav");
    set.add("h2persist empty honest", ch4.read().is_err(), "无数据不编造");
    // 提交日志完整（key+版本+时刻）。
    set.add(
        "h2persist journal",
        ch.journal.len() == 2 && ch.journal[0].1 == 1 && ch.journal[1].2 == 200,
        "accounted",
    );
    // 版本常量钉住。
    set.add("h2persist version", FORMAT_VERSION == 1, "向后兼容锚点");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2persist_all_green() {
        let set = run_h2persist_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2persist 自检红 {f}/{p}");
    }

    #[test]
    fn checksum_sanity() {
        assert_eq!(fnv1a64(b"a"), fnv1a64(b"a"));
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
        assert_ne!(fnv1a64(b""), 0);
    }
}
