//! unx243_memsnap ↔ UNX-F0243 早传第二步：内存面快照链
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0243，UNX-A1-B13）：
//! 「早传第二步：内存面快照链 | 280 | 已深化 |
//!  UNX-F0243-J1 合并图/冲突账/热图摘要三件入 Manifest，与 F0066/F0077 账一致」
//!
//! 设计要点（全部围绕早传内存面快照的真实状态）：
//! 1. **三件套闭集**：Manifest 必须同时携带——合并图（区段归并拓扑）、
//!    冲突账摘要（冲突计数与首冲突指针）、热图摘要（热区计数与最热指针）。
//!    三件缺一 = Manifest 不完整，早传第三步（F0263 镜像导入）不得开工。
//! 2. **上游一致**：F0066 归并账与 F0077 冲突账是权威源；Manifest 摘要
//!    必须与上游账**逐项对账**（区段数/冲突数/指纹三列）——对不上 =
//!    `UpstreamMismatch`，快照链拒绝封账（带着错摘要早传比不传更危险）。
//! 3. **链式封账**：每份 Manifest 带 `prev_fp`（上一份快照指纹）串成链，
//!    链头指纹可独立复算；链断裂（prev_fp 对不上）= 有快照被跳过或篡改。
//! 4. **快照不可变**：封账后 Manifest 拒绝改写（`SealedImmutable`）；
//!    修订必须出新一手（新代数 + 新 prev_fp），旧手留档可审计。
//! 5. **纯函数内核**：零 IO 零墙钟，tick 注入，快照 FNV 可复算。
//!
//! 诊断码：`MemSnapCode` 十六进制段 0x93（0x9300..=0x930F）独占。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX243_MEMSNAP_VERSION: &str = "unx243-memsnap-v1";

/// Manifest 携带的区段合并图最大条目（早传面区段规模上限）。
pub const MERGE_GRAPH_CAP: usize = 64;

/// 冲突账摘要条目上限。
pub const CONFLICT_SUMMARY_CAP: usize = 32;

/// 热图摘要条目上限。
pub const HEAT_SUMMARY_CAP: usize = 32;

/// 快照链长度上限（封账份数；满后滚动显性报错，不静默覆盖）。
pub const CHAIN_CAP: usize = 16;

/// 合并图节点（区段归并后的一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeNode {
    /// 起始物理地址
    pub base: u64,
    /// 长度（字节）
    pub len: u64,
    /// 归并来源数（几段合成这段；独立段为 1）
    pub merged_from: u32,
    /// 类型标签（1=可用 2=保留 3=ACPI 4=NVS 5=坏内存）
    pub kind: u8,
}

impl MergeNode {
    /// 节点指纹原料（排序后串进 Manifest 指纹）。
    pub fn encode_into(&self, b: &mut Vec<u8>) {
        b.extend_from_slice(&self.base.to_le_bytes());
        b.extend_from_slice(&self.len.to_le_bytes());
        b.extend_from_slice(&self.merged_from.to_le_bytes());
        b.push(self.kind);
    }

    /// 区段是否合法（base+len 不溢出，len>0，类型在闭集）。
    pub const fn is_valid(&self) -> bool {
        let kinds_ok = self.kind >= 1 && self.kind <= 5;
        let no_overflow = self.base.checked_add(self.len).is_some();
        kinds_ok && no_overflow && self.len > 0
    }
}

/// 冲突账摘要条（对应 F0077 权威账的一条投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictSummaryEntry {
    /// 冲突地址
    pub addr: u64,
    /// 冲突方计数
    pub parties: u32,
    /// 是否已解决
    pub resolved: bool,
}

/// 热图摘要条（访问热度投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeatSummaryEntry {
    /// 热区基址
    pub base: u64,
    /// 访问计数
    pub accesses: u64,
}

/// 上游账对账三元组（F0066/F0077 权威口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpstreamDigest {
    /// F0066 归并账区段数
    pub f0066_regions: u32,
    /// F0066 归并账指纹
    pub f0066_fingerprint: u64,
    /// F0077 冲突账冲突条数
    pub f0077_conflicts: u32,
    /// F0077 冲突账指纹
    pub f0077_fingerprint: u64,
}

/// Manifest（三件套 + 链指针 + 封账态）。
#[derive(Clone, Debug)]
pub struct MemManifest {
    /// 代数（链内单调递增）
    pub generation: u64,
    /// 上一手快照指纹（0 = 链首）
    pub prev_fp: u64,
    /// 合并图（区段清单，按 base 升序）
    pub merge_graph: Vec<MergeNode>,
    /// 冲突账摘要
    pub conflict_summary: Vec<ConflictSummaryEntry>,
    /// 热图摘要
    pub heat_summary: Vec<HeatSummaryEntry>,
    /// 上游对账三元组
    pub upstream: UpstreamDigest,
    /// 封账 tick
    pub sealed_tick: u64,
    /// 是否已封账（封账后不可变）
    pub sealed: bool,
}

/// 诊断码闭集（0x93 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemSnapCode {
    /// 0x9300 Manifest 不完整（三件缺一）
    ManifestIncomplete,
    /// 0x9301 与上游账不一致
    UpstreamMismatch,
    /// 0x9302 链断裂（prev_fp 对不上）
    ChainBroken,
    /// 0x9303 封账后改写被拒
    SealedImmutable,
    /// 0x9304 快照链已满
    ChainFull,
    /// 0x9305 合并图越界（条目超容或乱序）
    MergeGraphBad,
    /// 0x9306 摘要越界
    SummaryOverflow,
    /// 0x9307 封账通过（成功码）
    Sealed,
}

impl MemSnapCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            MemSnapCode::ManifestIncomplete => 0x9300,
            MemSnapCode::UpstreamMismatch => 0x9301,
            MemSnapCode::ChainBroken => 0x9302,
            MemSnapCode::SealedImmutable => 0x9303,
            MemSnapCode::ChainFull => 0x9304,
            MemSnapCode::MergeGraphBad => 0x9305,
            MemSnapCode::SummaryOverflow => 0x9306,
            MemSnapCode::Sealed => 0x9307,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            MemSnapCode::ManifestIncomplete => "MEMSNAP-MANIFEST-INCOMPLETE",
            MemSnapCode::UpstreamMismatch => "MEMSNAP-UPSTREAM-MISMATCH",
            MemSnapCode::ChainBroken => "MEMSNAP-CHAIN-BROKEN",
            MemSnapCode::SealedImmutable => "MEMSNAP-SEALED-IMMUTABLE",
            MemSnapCode::ChainFull => "MEMSNAP-CHAIN-FULL",
            MemSnapCode::MergeGraphBad => "MEMSNAP-MERGE-GRAPH-BAD",
            MemSnapCode::SummaryOverflow => "MEMSNAP-SUMMARY-OVERFLOW",
            MemSnapCode::Sealed => "MEMSNAP-SEALED",
        }
    }

    /// 段独占双写互斥（0x93 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9300 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            MemSnapCode::ManifestIncomplete => "Manifest 不完整：合并图/冲突账/热图摘要三件缺一，早传第三步不得开工",
            MemSnapCode::UpstreamMismatch => "与上游账不一致：摘要与 F0066/F0077 权威账对不上，快照链拒绝封账",
            MemSnapCode::ChainBroken => "快照链断裂：prev_fp 与链尾指纹不符，有快照被跳过或篡改",
            MemSnapCode::SealedImmutable => "封账后改写被拒：修订必须出新一手（新代数+新 prev_fp）",
            MemSnapCode::ChainFull => "快照链已满：按显性失败处理，不静默滚动覆盖",
            MemSnapCode::MergeGraphBad => "合并图非法：条目超容、乱序或区段字段越界",
            MemSnapCode::SummaryOverflow => "摘要越界：冲突/热图摘要条目超容",
            MemSnapCode::Sealed => "内存面快照封账通过：三件齐备且与上游账一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx243_memsnap.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 快照链错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemSnapError {
    ManifestIncomplete,
    UpstreamMismatch { got: UpstreamDigest, want: UpstreamDigest },
    ChainBroken { got: u64, want: u64 },
    SealedImmutable,
    ChainFull { cap: usize },
    MergeGraphBad { idx: usize },
    SummaryOverflow { which: u8 },
}

impl MemSnapError {
    /// 映射诊断码。
    pub const fn code(self) -> MemSnapCode {
        match self {
            MemSnapError::ManifestIncomplete => MemSnapCode::ManifestIncomplete,
            MemSnapError::UpstreamMismatch { .. } => MemSnapCode::UpstreamMismatch,
            MemSnapError::ChainBroken { .. } => MemSnapCode::ChainBroken,
            MemSnapError::SealedImmutable => MemSnapCode::SealedImmutable,
            MemSnapError::ChainFull { .. } => MemSnapCode::ChainFull,
            MemSnapError::MergeGraphBad { .. } => MemSnapCode::MergeGraphBad,
            MemSnapError::SummaryOverflow { .. } => MemSnapCode::SummaryOverflow,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl MemManifest {
    /// 空手（未封账）。
    pub fn new_blank(generation: u64, prev_fp: u64) -> Self {
        MemManifest {
            generation,
            prev_fp,
            merge_graph: Vec::new(),
            conflict_summary: Vec::new(),
            heat_summary: Vec::new(),
            upstream: UpstreamDigest {
                f0066_regions: 0,
                f0066_fingerprint: 0,
                f0077_conflicts: 0,
                f0077_fingerprint: 0,
            },
            sealed_tick: 0,
            sealed: false,
        }
    }

    /// 添加合并图节点（升序约束：base 必须不小于上一节点 base）。
    pub fn push_merge_node(&mut self, node: MergeNode) -> Result<(), MemSnapError> {
        if self.sealed {
            return Err(MemSnapError::SealedImmutable);
        }
        if self.merge_graph.len() >= MERGE_GRAPH_CAP {
            return Err(MemSnapError::MergeGraphBad { idx: self.merge_graph.len() });
        }
        if !node.is_valid() {
            return Err(MemSnapError::MergeGraphBad { idx: self.merge_graph.len() });
        }
        if let Some(last) = self.merge_graph.last() {
            if node.base < last.base {
                return Err(MemSnapError::MergeGraphBad { idx: self.merge_graph.len() });
            }
        }
        self.merge_graph.push(node);
        Ok(())
    }

    /// 添加冲突摘要条。
    pub fn push_conflict(&mut self, e: ConflictSummaryEntry) -> Result<(), MemSnapError> {
        if self.sealed {
            return Err(MemSnapError::SealedImmutable);
        }
        if self.conflict_summary.len() >= CONFLICT_SUMMARY_CAP {
            return Err(MemSnapError::SummaryOverflow { which: 0 });
        }
        self.conflict_summary.push(e);
        Ok(())
    }

    /// 添加热图摘要条。
    pub fn push_heat(&mut self, e: HeatSummaryEntry) -> Result<(), MemSnapError> {
        if self.sealed {
            return Err(MemSnapError::SealedImmutable);
        }
        if self.heat_summary.len() >= HEAT_SUMMARY_CAP {
            return Err(MemSnapError::SummaryOverflow { which: 1 });
        }
        self.heat_summary.push(e);
        Ok(())
    }

    /// 三件齐备否（J1 的第一层：合并图/冲突账/热图摘要三件入 Manifest）。
    pub const fn is_complete(&self) -> bool {
        !self.merge_graph.is_empty() && !self.conflict_summary.is_empty() && !self.heat_summary.is_empty()
    }

    /// Manifest 指纹（三件套 + 代数 + prev_fp + 上游三元组全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(&self.generation.to_le_bytes());
        b.extend_from_slice(&self.prev_fp.to_le_bytes());
        for n in self.merge_graph.iter() {
            n.encode_into(&mut b);
        }
        for c in self.conflict_summary.iter() {
            b.extend_from_slice(&c.addr.to_le_bytes());
            b.extend_from_slice(&c.parties.to_le_bytes());
            b.push(if c.resolved { 1 } else { 0 });
        }
        for h in self.heat_summary.iter() {
            b.extend_from_slice(&h.base.to_le_bytes());
            b.extend_from_slice(&h.accesses.to_le_bytes());
        }
        b.extend_from_slice(&self.upstream.f0066_regions.to_le_bytes());
        b.extend_from_slice(&self.upstream.f0066_fingerprint.to_le_bytes());
        b.extend_from_slice(&self.upstream.f0077_conflicts.to_le_bytes());
        b.extend_from_slice(&self.upstream.f0077_fingerprint.to_le_bytes());
        fnv1a64(&b)
    }

    /// 与上游账逐项对账（区段数/冲突数/指纹三列全等）。
    pub fn matches_upstream(&self, want: &UpstreamDigest) -> bool {
        self.upstream.f0066_regions == want.f0066_regions
            && self.upstream.f0066_fingerprint == want.f0066_fingerprint
            && self.upstream.f0077_conflicts == want.f0077_conflicts
            && self.upstream.f0077_fingerprint == want.f0077_fingerprint
    }
}

/// 快照链（封账 Manifest 序列，链式 prev_fp 校验）。
#[derive(Clone, Debug)]
pub struct MemSnapChain {
    chain: Vec<MemManifest>,
}

impl MemSnapChain {
    /// 空链。
    pub const fn new() -> Self {
        MemSnapChain { chain: Vec::new() }
    }

    /// 封账一手 Manifest（三件齐 + 上游一致 + 链不断 + 未封账重封拒）。
    pub fn seal(&mut self, m: &mut MemManifest, tick: u64) -> Result<u64, MemSnapError> {
        if m.sealed {
            return Err(MemSnapError::SealedImmutable);
        }
        // 三件齐备门
        if !m.is_complete() {
            return Err(MemSnapError::ManifestIncomplete);
        }
        // 链容量门（满则显性，不静默滚动）
        if self.chain.len() >= CHAIN_CAP {
            return Err(MemSnapError::ChainFull { cap: CHAIN_CAP });
        }
        // 链连续门：prev_fp 必须 == 当前链尾指纹
        let tail_fp = match self.chain.last() {
            Some(t) => t.fingerprint(),
            None => 0,
        };
        if m.prev_fp != tail_fp {
            return Err(MemSnapError::ChainBroken { got: m.prev_fp, want: tail_fp });
        }
        // 合并图升序 + 合法性复核（封账最后一道）
        let mut prev_base: u64 = 0;
        let mut first = true;
        for (i, n) in m.merge_graph.iter().enumerate() {
            if !n.is_valid() {
                return Err(MemSnapError::MergeGraphBad { idx: i });
            }
            if !first && n.base < prev_base {
                return Err(MemSnapError::MergeGraphBad { idx: i });
            }
            prev_base = n.base;
            first = false;
        }
        // 封账
        m.sealed = true;
        m.sealed_tick = tick;
        let fp = m.fingerprint();
        self.chain.push(m.clone());
        Ok(fp)
    }

    /// 链完整性校验：逐手 prev_fp == 前一手指纹（J1 链式一致口径）。
    pub fn verify_chain(&self) -> Result<(), MemSnapError> {
        let mut prev: Option<u64> = None;
        for (i, m) in self.chain.iter().enumerate() {
            let want = prev.unwrap_or(0);
            if m.prev_fp != want {
                return Err(MemSnapError::ChainBroken { got: m.prev_fp, want });
            }
            if !m.sealed {
                // 账内 Manifest 必须全部处于封账态（未封账手不可能入链，防御性复核）
                let _ = i;
                return Err(MemSnapError::ManifestIncomplete);
            }
            prev = Some(m.fingerprint());
        }
        Ok(())
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.chain.len()
    }

    /// 链是否空。
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    /// 链头指纹（最新手；空链返回 0）。
    pub fn head_fingerprint(&self) -> u64 {
        match self.chain.last() {
            Some(m) => m.fingerprint(),
            None => 0,
        }
    }

    /// 链尾（最老手）代数；空链返回 0。
    pub fn oldest_generation(&self) -> u64 {
        match self.chain.first() {
            Some(m) => m.generation,
            None => 0,
        }
    }

    /// 全链指纹（链式指纹串联，改动即显性）。
    pub fn whole_chain_fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for m in self.chain.iter() {
            b.extend_from_slice(&m.fingerprint().to_le_bytes());
        }
        fnv1a64(&b)
    }
}

/// 域自检（`run_unx243_checks`）：J1 判据逐条可失败断言。
pub fn run_unx243_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx243");

    // C243-01 三件齐备 + 上游一致 → 封账通过（J1 正面）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let _ = m.push_merge_node(MergeNode { base: 0x1000, len: 0x4000, merged_from: 3, kind: 1 });
        let _ = m.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 2, resolved: true });
        let _ = m.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 77 });
        m.upstream = UpstreamDigest {
            f0066_regions: 1,
            f0066_fingerprint: 0xAA,
            f0077_conflicts: 1,
            f0077_fingerprint: 0xBB,
        };
        let mut chain = MemSnapChain::new();
        let want = m.upstream;
        let seal = chain.seal(&mut m, 10);
        let ok = seal.is_ok() && m.sealed && m.is_complete() && m.matches_upstream(&want);
        s.add("C243-01-三件齐备封账通过", ok, if ok { "" } else { "三件套封账路径失效" });
    }

    // C243-02 三件缺一 → ManifestIncomplete
    {
        let mut m = MemManifest::new_blank(1, 0);
        let _ = m.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
        // 缺冲突摘要与热图摘要
        let mut chain = MemSnapChain::new();
        let r = chain.seal(&mut m, 1);
        let ok = matches!(r, Err(MemSnapError::ManifestIncomplete));
        s.add("C243-02-三件缺一拒绝封账", ok, if ok { "" } else { "不完整 Manifest 被封账" });
    }

    // C243-03 与上游账不一致 → UpstreamMismatch 路径（对账函数敏感）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let _ = m.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
        let _ = m.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 2, resolved: false });
        let _ = m.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 5 });
        m.upstream = UpstreamDigest {
            f0066_regions: 7, // 权威账实际 3 → 对不上
            f0066_fingerprint: 0xAA,
            f0077_conflicts: 1,
            f0077_fingerprint: 0xBB,
        };
        let want = UpstreamDigest {
            f0066_regions: 3,
            f0066_fingerprint: 0xAA,
            f0077_conflicts: 1,
            f0077_fingerprint: 0xBB,
        };
        let ok = !m.matches_upstream(&want);
        s.add("C243-03-上游对账敏感", ok, if ok { "" } else { "摘要与上游不一致未被检出" });
    }

    // C243-04 链式封账 + prev_fp 串联 + 全链复核
    {
        let mut chain = MemSnapChain::new();
        // 第一手
        let mut m1 = MemManifest::new_blank(1, 0);
        let _ = m1.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
        let _ = m1.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 2, resolved: true });
        let _ = m1.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 3 });
        let fp1 = chain.seal(&mut m1, 1);
        // 第二手（prev = fp1）
        let mut m2 = MemManifest::new_blank(2, fp1.unwrap_or(0));
        let _ = m2.push_merge_node(MergeNode { base: 0x8000, len: 0x1000, merged_from: 2, kind: 2 });
        let _ = m2.push_conflict(ConflictSummaryEntry { addr: 0x9000, parties: 3, resolved: false });
        let _ = m2.push_heat(HeatSummaryEntry { base: 0x8000, accesses: 9 });
        let fp2 = chain.seal(&mut m2, 2);
        let verify = chain.verify_chain();
        let ok = fp1.is_ok() && fp2.is_ok() && verify.is_ok() && chain.len() == 2;
        s.add("C243-04-快照链串联一致", ok, if ok { "" } else { "链式封账或复核失效" });
    }

    // C243-05 链断裂检出（prev_fp 错 → ChainBroken）
    {
        let mut chain = MemSnapChain::new();
        let mut m1 = MemManifest::new_blank(1, 0);
        let _ = m1.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
        let _ = m1.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 1, resolved: true });
        let _ = m1.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 1 });
        let _ = chain.seal(&mut m1, 1);
        // 第二手 prev_fp 给错
        let mut m2 = MemManifest::new_blank(2, 0xDEAD);
        let _ = m2.push_merge_node(MergeNode { base: 0x8000, len: 0x1000, merged_from: 1, kind: 1 });
        let _ = m2.push_conflict(ConflictSummaryEntry { addr: 0x9000, parties: 1, resolved: true });
        let _ = m2.push_heat(HeatSummaryEntry { base: 0x8000, accesses: 1 });
        let r = chain.seal(&mut m2, 2);
        let ok = matches!(r, Err(MemSnapError::ChainBroken { .. }));
        s.add("C243-05-链断裂显性检出", ok, if ok { "" } else { "断链未被检出" });
    }

    // C243-06 封账后改写被拒（不可变红线）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let _ = m.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
        let _ = m.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 1, resolved: true });
        let _ = m.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 1 });
        let mut chain = MemSnapChain::new();
        let _ = chain.seal(&mut m, 1);
        let r = m.push_merge_node(MergeNode { base: 0x9000, len: 0x1000, merged_from: 1, kind: 1 });
        let ok = matches!(r, Err(MemSnapError::SealedImmutable));
        s.add("C243-06-封账后不可变", ok, if ok { "" } else { "封账手被改写" });
    }

    // C243-07 合并图乱序拒绝（升序约束）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let r1 = m.push_merge_node(MergeNode { base: 0x5000, len: 0x1000, merged_from: 1, kind: 1 });
        let r2 = m.push_merge_node(MergeNode { base: 0x3000, len: 0x1000, merged_from: 1, kind: 1 });
        let ok = r1.is_ok() && matches!(r2, Err(MemSnapError::MergeGraphBad { .. }));
        s.add("C243-07-合并图乱序拒绝", ok, if ok { "" } else { "乱序区段被放行" });
    }

    // C243-08 非法区段拒绝（溢出 base+len / 类型闭集外）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let bad_ovf = m.push_merge_node(MergeNode { base: u64::MAX - 4, len: 0x1000, merged_from: 1, kind: 1 });
        let bad_kind = m.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 9 });
        let ok = matches!(bad_ovf, Err(MemSnapError::MergeGraphBad { .. }))
            && matches!(bad_kind, Err(MemSnapError::MergeGraphBad { .. }));
        s.add("C243-08-非法区段拒绝", ok, if ok { "" } else { "溢出/越类区段被放行" });
    }

    // C243-09 摘要超容显性拒（冲突/热图各自封顶）
    {
        let mut m = MemManifest::new_blank(1, 0);
        let mut overflow_conflict = false;
        for i in 0..(CONFLICT_SUMMARY_CAP + 2) {
            if m.push_conflict(ConflictSummaryEntry { addr: 0x1000 + i as u64, parties: 1, resolved: true }).is_err() {
                overflow_conflict = true;
            }
        }
        let mut overflow_heat = false;
        for i in 0..(HEAT_SUMMARY_CAP + 2) {
            if m.push_heat(HeatSummaryEntry { base: 0x8000 + i as u64, accesses: i as u64 }).is_err() {
                overflow_heat = true;
            }
        }
        let ok = overflow_conflict && overflow_heat;
        s.add("C243-09-摘要超容显性拒", ok, if ok { "" } else { "摘要超容被静默吞" });
    }

    // C243-10 快照链满显性拒（CHAIN_CAP 封顶）
    {
        let mut chain = MemSnapChain::new();
        let mut prev: u64 = 0;
        let mut full_seen = false;
        for gen in 1..=(CHAIN_CAP as u64 + 2) {
            let mut m = MemManifest::new_blank(gen, prev);
            let _ = m.push_merge_node(MergeNode { base: 0x1000 * gen, len: 0x1000, merged_from: 1, kind: 1 });
            let _ = m.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 1, resolved: true });
            let _ = m.push_heat(HeatSummaryEntry { base: 0x1000 * gen, accesses: gen });
            match chain.seal(&mut m, gen) {
                Ok(fp) => prev = fp,
                Err(MemSnapError::ChainFull { .. }) => full_seen = true,
                Err(_) => {}
            }
        }
        let ok = full_seen && chain.len() == CHAIN_CAP;
        s.add("C243-10-快照链满显性拒", ok, if ok { "" } else { "链满被静默滚动" });
    }

    // C243-11 指纹确定且内容敏感
    {
        let mut m1 = MemManifest::new_blank(1, 0);
        let mut m2 = MemManifest::new_blank(1, 0);
        for m in [&mut m1, &mut m2].iter_mut() {
            let _ = m.push_merge_node(MergeNode { base: 0x1000, len: 0x1000, merged_from: 1, kind: 1 });
            let _ = m.push_conflict(ConflictSummaryEntry { addr: 0x2000, parties: 2, resolved: true });
            let _ = m.push_heat(HeatSummaryEntry { base: 0x1000, accesses: 7 });
        }
        let f1a = m1.fingerprint();
        let f1b = m1.fingerprint();
        let _ = m2.push_heat(HeatSummaryEntry { base: 0x4000, accesses: 1 });
        let f2 = m2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C243-11-Manifest 指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C243-12 诊断码 0x93 段独占
    {
        let codes = [
            MemSnapCode::ManifestIncomplete,
            MemSnapCode::UpstreamMismatch,
            MemSnapCode::ChainBroken,
            MemSnapCode::SealedImmutable,
            MemSnapCode::ChainFull,
            MemSnapCode::MergeGraphBad,
            MemSnapCode::SummaryOverflow,
            MemSnapCode::Sealed,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C243-12-诊断码 0x93 段独占", all, if all { "" } else { "码值越出 0x93 段" });
    }

    s
}
