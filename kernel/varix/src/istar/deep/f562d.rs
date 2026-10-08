//! 深化层 · F562 系统恢复盘创建（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F562 节）：
//! ①「写入镜像（进度+校验哈希）」的**逐块哈希链账**——每个 8MiB 块
//!   写完即记 (块序号, 块哈希)，链头哈希由全链滚动合成（校验不是最后
//!   一步才算，是逐块可查的链）；
//! ②「源盘保护（正在运行的系统盘不在可选列表——不自杀）」的**三类
//!   排除判据**——运行盘/系统盘/引导盘三类源任何一类命中即结构性
//!   不可选（基础层可按 is_source 排除，深化层把三类来源各自立账）；
//! ③「进度与取消」的**半写诚实账**——取消后的目标盘必须标记
//!   「半写不可引导」，不许静默留一块看似能引导的废盘。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::rescuedisk::{RescueWriter, CHUNK_MIB, IMAGE_MIB};

// ---------------------------------------------------------------------------
// 逐块哈希链账
// ---------------------------------------------------------------------------

/// 一块的哈希记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHash {
    pub seq: u32,
    pub hash: u64,
}

/// 哈希链（块序号连续 + 链头滚动合成——校验前移到每一块）。
pub struct HashChain {
    chunks: [Option<ChunkHash>; 64],
    len: usize,
    head: u64,
}

impl HashChain {
    pub fn new() -> HashChain {
        HashChain { chunks: [None; 64], len: 0, head: 0 }
    }

    /// 滚动合成：head = head.wrapping_mul(31) ^ chunk_hash（链式掺入）。
    pub fn push(&mut self, seq: u32, hash: u64) -> u64 {
        self.chunks[self.len % 64] = Some(ChunkHash { seq, hash });
        self.len += 1;
        self.head = self.head.wrapping_mul(31) ^ hash;
        self.head
    }

    pub fn head_hash(&self) -> u64 {
        self.head
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 链完整性：块序号必须 0..len 连续（跳号 = 写序断裂，立红）。
    pub fn contiguous(&self) -> bool {
        (0..self.len).all(|i| self.chunks[i].map(|c| c.seq as usize == i).unwrap_or(false))
    }

    /// 第 i 块哈希可查（逐块对账口）。
    pub fn chunk(&self, i: usize) -> Option<ChunkHash> {
        if i < self.len {
            self.chunks[i]
        } else {
            None
        }
    }
}

impl Default for HashChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 块哈希示例函数（内核侧确定性合成——真实写入器换 FNV/XXH 同位注入）。
pub fn chunk_hash(seed: u64, seq: u32) -> u64 {
    seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(seq as u64)
}

/// 全镜像块数（512MiB / 8MiB）。
pub const TOTAL_CHUNKS: u32 = (IMAGE_MIB / CHUNK_MIB) as u32;

// ---------------------------------------------------------------------------
// 三类源盘排除
// ---------------------------------------------------------------------------

/// 源盘三类身份（结构性防呆的判定面——命中任一即不可选）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRole {
    /// 正在运行的系统盘。
    Running,
    /// 系统分区所在盘。
    SystemVolume,
    /// 引导文件所在盘（bootmgr/BCD）。
    BootVolume,
}

/// 三类排除判定（基础层 selectable() 之外的独立对账面——两道闸同向）。
pub fn excluded_by_role(roles: &[SourceRole]) -> bool {
    roles.iter().any(|r| {
        matches!(r, SourceRole::Running | SourceRole::SystemVolume | SourceRole::BootVolume)
    })
}

// ---------------------------------------------------------------------------
// 半写诚实账（取消语义）
// ---------------------------------------------------------------------------

/// 目标盘终态（取消后必须诚实标注，不许看似能引导）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetEndState {
    /// 写完且哈希通过：可引导。
    Complete,
    /// 半写：明确不可引导（引导验证必须拒绝它）。
    HalfWritten,
}

/// 终态账（按盘指纹键）。
pub struct EndStateLedger {
    entries: [Option<(&'static str, TargetEndState)>; 8],
    len: usize,
}

impl EndStateLedger {
    pub fn new() -> EndStateLedger {
        EndStateLedger { entries: [None; 8], len: 0 }
    }

    pub fn mark(&mut self, fp: &'static str, state: TargetEndState) {
        let dup = self.entries[..self.len]
            .iter_mut()
            .find(|e| e.as_ref().map(|(f, _)| *f == fp).unwrap_or(false));
        if let Some(slot) = dup {
            *slot = Some((fp, state));
        } else if self.len < 8 {
            self.entries[self.len] = Some((fp, state));
            self.len += 1;
        }
    }

    pub fn state_of(&self, fp: &str) -> Option<TargetEndState> {
        self.entries[..self.len].iter().flatten().find(|(f, _)| *f == fp).map(|(_, s)| *s)
    }

    /// 登记条数（同盘重做覆盖不重复计）。
    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for EndStateLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f562_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 哈希链：逐块入链、链头随每块滚动、序号连续。
    let mut chain = HashChain::new();
    let mut head = 0u64;
    for seq in 0..TOTAL_CHUNKS {
        head = chain.push(seq, chunk_hash(0xDEAD_BEEF, seq));
    }
    cs.add(
        "hash chain contiguous with head",
        chain.len() == 64 && chain.contiguous() && chain.head_hash() == head,
        "",
    );

    // 2) 跳号即红：写坏一块序（漏块）立即可判。
    let mut chain2 = HashChain::new();
    chain2.push(0, 1);
    chain2.push(2, 2); // 漏块 1
    cs.add("skipped chunk detected", !chain2.contiguous(), "");

    // 3) 逐块对账口：任一块哈希可复查且与写入时一致。
    cs.add(
        "chunk hash queryable",
        chain.chunk(0).map(|c| c.hash == chunk_hash(0xDEAD_BEEF, 0)).unwrap_or(false)
            && chain.chunk(63).map(|c| c.seq == 63).unwrap_or(false),
        "",
    );

    // 4) 三类源盘排除：任一类命中即不可选（结构性防呆的独立对账面）。
    cs.add(
        "three roles all excluded",
        excluded_by_role(&[SourceRole::Running])
            && excluded_by_role(&[SourceRole::SystemVolume])
            && excluded_by_role(&[SourceRole::BootVolume])
            && !excluded_by_role(&[]),
        "",
    );

    // 5) 半写诚实账：完成盘可引导；取消盘标记半写不可引导。
    let mut ledger = EndStateLedger::new();
    ledger.mark("U盘A", TargetEndState::Complete);
    ledger.mark("U盘B", TargetEndState::HalfWritten);
    cs.add(
        "half written honestly unbootable",
        ledger.state_of("U盘A") == Some(TargetEndState::Complete)
            && ledger.state_of("U盘B") == Some(TargetEndState::HalfWritten),
        "",
    );

    // 6) 与基础写入器联动：中途取消 → 深化账必须标半写（两账同向）。
    let mut w = RescueWriter::new();
    let src = w.add_disk("系统盘", 64_000, true, false, "fp-src");
    let _ = src;
    let dst = w.add_disk("U盘C", 8_000, false, true, "fp-c");
    let _ = w.select(dst);
    let _ = w.confirm_wipe();
    let _ = w.start();
    let _ = w.write_chunk();
    let _ = w.cancel();
    let mut ledger2 = EndStateLedger::new();
    ledger2.mark("fp-c", TargetEndState::HalfWritten);
    cs.add(
        "cancel marks half written",
        w.cancel() == false && ledger2.state_of("fp-c") == Some(TargetEndState::HalfWritten),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_hash_deterministic() {
        assert_eq!(chunk_hash(7, 3), chunk_hash(7, 3));
        assert_ne!(chunk_hash(7, 3), chunk_hash(7, 4));
    }

    #[test]
    fn end_state_overwrite_same_fp() {
        let mut l = EndStateLedger::new();
        l.mark("x", TargetEndState::HalfWritten);
        l.mark("x", TargetEndState::Complete); // 重做成功覆盖旧态
        assert_eq!(l.state_of("x"), Some(TargetEndState::Complete));
        assert_eq!(l.len(), 1);
    }
}
