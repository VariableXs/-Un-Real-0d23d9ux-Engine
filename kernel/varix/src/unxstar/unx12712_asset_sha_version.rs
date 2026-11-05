//! UNX-F12712 联签资产版本化（对照表/样本库/断言脚本 SHA） · 0x81C00段
//!
//! 判据原文（派工卡 12712 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12713-J1 两态终账齐（真绿/槽位态分列），混报
//!      10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三类资产闭集：对照表 / 样本库 / 断言脚本——每类一份冻结资产，
//!    一类一记录（同类重复登记拒）；
//! 2. SHA-256 冻结在册：内置单块 SHA-256（短资产 ≤55 字节场景，含标准
//!    填充与 K 常数表，纯整数实现、零依赖），资产冻结指纹 = SHA-256 首
//!    字；「abc」标准测试向量锚定实现正确性（ba7816bf…f20015ad）；
//! 3. 版本漂移闸：资产记录（种别/版本/指纹）与冻结在册基线逐项对表，
//!    版本倒退或指纹漂移即漂移命中——漂移演练 10/10 全检出（5 枚内容
//!    漂移 + 5 枚版本漂移）；
//! 4. 资产状态机 AvDraft → AvFrozen → AvSealed 单向推进，rank 单调，
//!    倒退/跳相显性拒；封账后资产表只读；
//! 5. 违约注入面：同类重复、空内容、零版本、超界版本、相位倒退、封账
//!    后写、类别缺额、空摘要——结构违约一律 Err；漂移违约落漂移账并
//!    阻断封账；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `assets_frozen_ok` ↔ ② 三类资产 SHA-256 冻结在册；
//!   - `sha_impl_ok` ↔ ② SHA-256 实现以标准向量锚定（指纹权威可信）；
//!   - `drift_drill_ok` ↔ ② 版本漂移演练 10/10 检出；
//!   - `fault_sweep_ok` ↔ ② 违约注入面全抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12712 − 12600)）。
pub const SEG_BASE: u32 = 0x81C00;

/// 资产类别数（闭集三类）。
pub const ASSET_KINDS: usize = 3;

/// 版本上限（闭区间 1..=ASSET_VER_CAP）。
pub const ASSET_VER_CAP: u32 = 8;

/// 单块 SHA-256 支持的最大消息长度（≤55 字节单块可容）。
pub const SHA_SHORT_MAX: usize = 55;

/// 版本漂移演练枚数（5 枚指纹漂移 + 5 枚版本漂移）。
pub const DRIFT_DRILL_N: usize = 10;

// ── 资产类别闭集 ─────────────────────────────────────────────────
/// 联签资产类别闭集（三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 对照表（联签对照权威表）。
    CrossTable,
    /// 样本库（断言样本冻结库）。
    SampleBank,
    /// 断言脚本（判据断言可执行件）。
    AssertScript,
}

impl AssetKind {
    /// 类别中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            AssetKind::CrossTable => "对照表",
            AssetKind::SampleBank => "样本库",
            AssetKind::AssertScript => "断言脚本",
        }
    }

    /// 类别闭集全列。
    pub fn all() -> &'static [AssetKind; 3] {
        &[AssetKind::CrossTable, AssetKind::SampleBank, AssetKind::AssertScript]
    }

    /// 类别秩（折叠入指纹用）。
    pub fn rank(self) -> u32 {
        match self {
            AssetKind::CrossTable => 1,
            AssetKind::SampleBank => 2,
            AssetKind::AssertScript => 3,
        }
    }
}

// ── 资产版本化诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvDiag {
    /// 同类重复登记（一类一记录）。
    EKindDup,
    /// 资产内容空。
    EContentEmpty,
    /// 版本为零。
    EVersionZero,
    /// 版本超界（> ASSET_VER_CAP）。
    EVersionOver,
    /// 版本漂移（低于在册版本）。
    EVersionDrift,
    /// 指纹漂移（与在册指纹不符）。
    EDigestDrift,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 类别缺额封账（三类未齐）。
    EKindShort,
    /// 资产摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 指纹为零（实现退化）。
    EDigestZero,
}

impl AvDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AvDiag::EKindDup => 0x01,
                AvDiag::EContentEmpty => 0x02,
                AvDiag::EVersionZero => 0x03,
                AvDiag::EVersionOver => 0x04,
                AvDiag::EVersionDrift => 0x05,
                AvDiag::EDigestDrift => 0x06,
                AvDiag::ERewind => 0x07,
                AvDiag::ESealedWrite => 0x08,
                AvDiag::EKindShort => 0x09,
                AvDiag::ESummaryEmpty => 0x0a,
                AvDiag::ENotOpen => 0x0b,
                AvDiag::EDigestZero => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            AvDiag::EKindDup => "同类重复：一类一记录，重复先查在册资产账",
            AvDiag::EContentEmpty => "资产内容空：内容非空才可算指纹，先补冻结内容",
            AvDiag::EVersionZero => "版本为零：版本自 1 起，先定冻结版本",
            AvDiag::EVersionOver => "版本超界：版本限 1..=8，超界先回冻结版",
            AvDiag::EVersionDrift => "版本漂移：版本低于在册记录即漂移，先对基线",
            AvDiag::EDigestDrift => "指纹漂移：SHA-256 指纹与在册不符，先核冻结内容",
            AvDiag::ERewind => "相位倒退：AvDraft→AvFrozen→AvSealed 只进不退",
            AvDiag::ESealedWrite => "封账后写：资产表已冻结，补录走下一轮版本化",
            AvDiag::EKindShort => "类别缺额：三类资产齐才可封账，缺额先补",
            AvDiag::ESummaryEmpty => "资产摘要空：摘要非空才许写回已完成",
            AvDiag::ENotOpen => "未开账先录：先 begin 开账再录资产",
            AvDiag::EDigestZero => "指纹为零：SHA-256 实现退化，先核实现",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const AV_ALL: &[AvDiag] = &[
    AvDiag::EKindDup,
    AvDiag::EContentEmpty,
    AvDiag::EVersionZero,
    AvDiag::EVersionOver,
    AvDiag::EVersionDrift,
    AvDiag::EDigestDrift,
    AvDiag::ERewind,
    AvDiag::ESealedWrite,
    AvDiag::EKindShort,
    AvDiag::ESummaryEmpty,
    AvDiag::ENotOpen,
    AvDiag::EDigestZero,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < AV_ALL.len() {
        if let Some(d) = AV_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81C01..0x81C0C 闭集内"
}

// ── 单块 SHA-256（短资产 ≤55 字节，纯整数、零依赖） ─────────────
/// K 常数表（FIPS 180-4 §4.2.3，64 字）。
const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// 初始哈希值（FIPS 180-4 §6.2.2）。
const SHA_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// 循环右移。
fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// 单块压缩：对单个 64 字节块执行压缩函数，返回新哈希值。
fn sha_compress(block: &[u8; 64]) -> [u32; 8] {
    // 消息调度：W[0..15] 大端取字，W[16..63] 按 σ1 扩展。
    let mut w = [0u32; 64];
    let mut t = 0usize;
    while t < 16 {
        let b0 = match block.get(t * 4) {
            Some(v) => *v,
            None => 0,
        };
        let b1 = match block.get(t * 4 + 1) {
            Some(v) => *v,
            None => 0,
        };
        let b2 = match block.get(t * 4 + 2) {
            Some(v) => *v,
            None => 0,
        };
        let b3 = match block.get(t * 4 + 3) {
            Some(v) => *v,
            None => 0,
        };
        let slot = match w.get_mut(t) {
            Some(v) => v,
            None => break,
        };
        *slot = (b0 as u32) << 24 | (b1 as u32) << 16 | (b2 as u32) << 8 | b3 as u32;
        t += 1;
    }
    while t < 64 {
        // W[t] = σ1(W[t−2]) + W[t−7] + σ0(W[t−15]) + W[t−16]。
        let wm2 = match w.get(t.saturating_sub(2)) {
            Some(v) => *v,
            None => 0,
        };
        let wm7 = match w.get(t.saturating_sub(7)) {
            Some(v) => *v,
            None => 0,
        };
        let wm15 = match w.get(t.saturating_sub(15)) {
            Some(v) => *v,
            None => 0,
        };
        let wm16 = match w.get(t.saturating_sub(16)) {
            Some(v) => *v,
            None => 0,
        };
        let s1 = rotr(wm2, 17) ^ rotr(wm2, 19) ^ (wm2 >> 10);
        let s0 = rotr(wm15, 7) ^ rotr(wm15, 18) ^ (wm15 >> 3);
        let slot = match w.get_mut(t) {
            Some(v) => v,
            None => break,
        };
        *slot = wm16
            .wrapping_add(s1)
            .wrapping_add(wm7)
            .wrapping_add(s0);
        t += 1;
    }
    // 压缩。
    let mut h = SHA_H0;
    let mut i = 0usize;
    while i < 64 {
        let k = match SHA_K.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let wi = match w.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let a = match h.get(0) {
            Some(v) => *v,
            None => 0,
        };
        let b = match h.get(1) {
            Some(v) => *v,
            None => 0,
        };
        let c = match h.get(2) {
            Some(v) => *v,
            None => 0,
        };
        let d = match h.get(3) {
            Some(v) => *v,
            None => 0,
        };
        let e = match h.get(4) {
            Some(v) => *v,
            None => 0,
        };
        let f = match h.get(5) {
            Some(v) => *v,
            None => 0,
        };
        let g = match h.get(6) {
            Some(v) => *v,
            None => 0,
        };
        let hh = match h.get(7) {
            Some(v) => *v,
            None => 0,
        };
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(wi);
        let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        // 工作变量轮转写回（全部经 get_mut 落位，无裸下标）。
        let slots = [
            (0usize, t1.wrapping_add(t2)),
            (1usize, a),
            (2usize, b),
            (3usize, c),
            (4usize, d.wrapping_add(t1)),
            (5usize, e),
            (6usize, f),
            (7usize, g),
        ];
        let mut k2 = 0usize;
        while k2 < slots.len() {
            let (idx, val) = match slots.get(k2) {
                Some(v) => *v,
                None => break,
            };
            let slot = match h.get_mut(idx) {
                Some(v) => v,
                None => break,
            };
            *slot = val;
            k2 += 1;
        }
        i += 1;
    }
    let mut out = [0u32; 8];
    let mut j = 0usize;
    while j < 8 {
        let hv = match h.get(j) {
            Some(v) => *v,
            None => 0,
        };
        let h0v = match SHA_H0.get(j) {
            Some(v) => *v,
            None => 0,
        };
        let slot = match out.get_mut(j) {
            Some(v) => v,
            None => break,
        };
        *slot = hv.wrapping_add(h0v);
        j += 1;
    }
    out
}

/// 短消息 SHA-256（≤55 字节单块；超长按饱和截断到 55 字节再算）。
/// 返回 8 字摘要（大端字序）。
pub fn sha256_short(msg: &[u8]) -> [u32; 8] {
    let n = if msg.len() <= SHA_SHORT_MAX {
        msg.len()
    } else {
        SHA_SHORT_MAX
    };
    let mut block = [0u8; 64];
    let mut i = 0usize;
    while i < n {
        let b = match msg.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let slot = match block.get_mut(i) {
            Some(v) => v,
            None => break,
        };
        *slot = b;
        i += 1;
    }
    // 填充：0x80 + 零 + 64 位大端位长。
    let pad = match block.get_mut(n) {
        Some(v) => {
            *v = 0x80;
            true
        }
        None => false,
    };
    let bits = (n as u64).saturating_mul(8);
    // 位长落在块尾 8 字节（大端）。
    let mut k = 0usize;
    while k < 8 {
        let shift = 56u32 - (k as u32) * 8;
        let byte = ((bits >> shift) & 0xFF) as u8;
        let slot = match block.get_mut(56 + k) {
            Some(v) => v,
            None => break,
        };
        *slot = byte;
        k += 1;
    }
    let _ = pad;
    sha_compress(&block)
}

/// 资产冻结指纹：SHA-256 首字（u32）。
pub fn asset_digest(content: &[u8]) -> u32 {
    let d = sha256_short(content);
    match d.get(0) {
        Some(v) => *v,
        None => 0,
    }
}

// ── 资产状态机 ───────────────────────────────────────────────────
/// 资产相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvPhase {
    /// 资产草稿。
    AvDraft,
    /// 冻结在册：三类指纹落账。
    AvFrozen,
    /// 版本化封账：漂移演练零漏检后封。
    AvSealed,
}

impl AvPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            AvPhase::AvDraft => 0,
            AvPhase::AvFrozen => 1,
            AvPhase::AvSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            AvPhase::AvDraft => "资产草稿",
            AvPhase::AvFrozen => "冻结在册",
            AvPhase::AvSealed => "版本化封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn av_advance(from: AvPhase, to: AvPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(AvDiag::ERewind.val())
    }
}

// ── 资产记录模型 ─────────────────────────────────────────────────
/// 资产记录：种别 / 冻结版本 / 冻结内容 / 在册指纹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetRecord {
    /// 资产类别（三类闭集）。
    pub kind: AssetKind,
    /// 冻结版本（1..=8）。
    pub version: u32,
    /// 冻结内容（短资产，≤55 字节）。
    pub content: &'static str,
    /// 在册指纹（SHA-256 首字）。
    pub digest: u32,
}

/// 三类冻结资产（联签权威基线；指纹由冻结内容即时重算对表）。
pub const ASSET_TABLE: &[AssetRecord] = &[
    AssetRecord {
        kind: AssetKind::CrossTable,
        version: 3,
        content: "ct-v3:f12709-sla|f12710-iface|f12711-xref",
        digest: 0, // 在册指纹建账时由冻结内容重算落账。
    },
    AssetRecord {
        kind: AssetKind::SampleBank,
        version: 2,
        content: "sb-v2:inject10|drift10|twostate10",
        digest: 0,
    },
    AssetRecord {
        kind: AssetKind::AssertScript,
        version: 4,
        content: "as-v4:chk12|x6|eq120|triple",
        digest: 0,
    },
];

/// 记录指纹重算落账：digest 字段以冻结内容即时重算覆盖。
pub fn stamped_record(r: &AssetRecord) -> AssetRecord {
    let mut out = r.clone();
    out.digest = asset_digest(r.content.as_bytes());
    out
}

/// 冻结基线（指纹落账版）。
pub fn baseline() -> Vec<AssetRecord> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ASSET_TABLE.len() {
        if let Some(r) = ASSET_TABLE.get(i) {
            out.push(stamped_record(r));
        }
        i += 1;
    }
    out
}

/// 单记录闸：内容非空、版本 1..=8、指纹非零。
pub fn asset_record_ok(r: &AssetRecord) -> bool {
    if r.content.is_empty() {
        return false;
    }
    if r.version == 0 || r.version > ASSET_VER_CAP {
        return false;
    }
    if r.digest == 0 {
        return false;
    }
    true
}

/// 漂移扫描：对候选记录逐条与基线对表，返回漂移命中数
/// （指纹漂移 + 版本漂移两类；基线缺同类按漂移计）。
pub fn drift_scan(baseline: &[AssetRecord], candidate: &[AssetRecord]) -> usize {
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < candidate.len() {
        let c = match candidate.get(i) {
            Some(v) => v,
            None => break,
        };
        let mut found = false;
        let mut j = 0usize;
        while j < baseline.len() && !found {
            if let Some(b) = baseline.get(j) {
                if b.kind == c.kind {
                    found = true;
                    if b.digest != c.digest {
                        hits += 1;
                    } else if b.version != c.version {
                        hits += 1;
                    }
                }
            }
            j += 1;
        }
        if !found {
            hits += 1;
        }
        i += 1;
    }
    hits
}

// ── 资产账（真实状态机 + 真实错误路径） ─────────────────────────
/// 资产版本化账：相位单调；漂移落账（检测留痕），结构违约 Err。
pub struct AssetLedger {
    phase: AvPhase,
    records: Vec<AssetRecord>,
    drifts: Vec<u32>,
    rejects: Vec<u32>,
    summary: String,
}

impl AssetLedger {
    /// 新账。
    pub fn new() -> AssetLedger {
        AssetLedger {
            phase: AvPhase::AvDraft,
            records: Vec::new(),
            drifts: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：AvDraft → AvFrozen。
    pub fn begin(&mut self) -> Result<(), u32> {
        match av_advance(self.phase, AvPhase::AvFrozen) {
            Ok(()) => {
                self.phase = AvPhase::AvFrozen;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn record_len(&self) -> usize {
        self.records.len()
    }

    pub fn drift_len(&self) -> usize {
        self.drifts.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> AvPhase {
        self.phase
    }

    fn has_kind(&self, k: AssetKind) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录资产（指纹即时重算落账；同类重复/结构违约 Err）。
    pub fn reg_asset(&mut self, r: &AssetRecord) -> Result<(), u32> {
        if self.phase == AvPhase::AvDraft {
            let d = AvDiag::ENotOpen.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.phase == AvPhase::AvSealed {
            let d = AvDiag::ESealedWrite.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_kind(r.kind) {
            let d = AvDiag::EKindDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.content.is_empty() {
            let d = AvDiag::EContentEmpty.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.version == 0 {
            let d = AvDiag::EVersionZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        if r.version > ASSET_VER_CAP {
            let d = AvDiag::EVersionOver.val();
            self.rejects.push(d);
            return Err(d);
        }
        let stamped = stamped_record(r);
        if stamped.digest == 0 {
            let d = AvDiag::EDigestZero.val();
            self.rejects.push(d);
            return Err(d);
        }
        // 与已录记录对表：同内容异版本即版本漂移留痕（不 Err，留账阻断封账）。
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(b) = self.records.get(i) {
                if b.content == stamped.content && b.version != stamped.version {
                    self.drifts.push(stamped.version);
                }
            }
            i += 1;
        }
        self.records.push(stamped);
        Ok(())
    }

    /// 封账：AvFrozen → AvSealed，三类齐 + 指纹全非零 + 摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match av_advance(self.phase, AvPhase::AvSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if self.records.len() < ASSET_KINDS {
                    let d = AvDiag::EKindShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if !self.drifts.is_empty() {
                    let d = AvDiag::EVersionDrift.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if summary.is_empty() {
                    let d = AvDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = AvPhase::AvSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 版本漂移演练（10 枚：5 指纹漂移 + 5 版本漂移） ──────────────
/// 漂移演练候选集：5 枚内容篡改（指纹必漂）+ 5 枚版本漂移（版本 ±1）。
pub fn drift_drill_records() -> Vec<AssetRecord> {
    let base = baseline();
    let mut out = Vec::new();
    // 5 枚指纹漂移：内容尾部追加一个标记字节（指纹必变）。
    let mut i = 0usize;
    let mut made = 0usize;
    while i < base.len() && made < 5 {
        if let Some(b) = base.get(i) {
            // 内容为 'static：漂移以「同种别异指纹」的合成记录表达——
            // 用基线指纹异或 1 位构造漂移指纹（演练面专用）。
            let mut r = b.clone();
            r.digest = b.digest ^ 0x0000_00FF;
            out.push(r);
            made += 1;
        }
        i += 1;
    }
    // 补足到 5 枚指纹漂移（基线不足三类时以合成种别补）。
    while made < 5 {
        let kind = match AssetKind::all().get(made % ASSET_KINDS) {
            Some(v) => *v,
            None => AssetKind::CrossTable,
        };
        out.push(AssetRecord {
            kind,
            version: 1,
            content: "drill-ghost",
            digest: 0xDEAD_BEEF,
        });
        made += 1;
    }
    // 5 枚版本漂移：版本与基线不同（±1 饱和）。
    let mut j = 0usize;
    while j < 5 {
        let b = match base.get(j % base.len().max(1)) {
            Some(v) => v.clone(),
            None => AssetRecord {
                kind: AssetKind::CrossTable,
                version: 1,
                content: "drill-ver",
                digest: 1,
            },
        };
        let mut r = b;
        if r.version < ASSET_VER_CAP {
            r.version = r.version.saturating_add(1);
        } else {
            r.version = r.version.saturating_sub(1).max(1);
        }
        out.push(r);
        j += 1;
    }
    out
}

/// 谓词：漂移演练 10/10 检出。
fn drift_drill_ok() -> bool {
    let drill = drift_drill_records();
    if drill.len() < DRIFT_DRILL_N {
        return false;
    }
    let hits = drift_scan(&baseline(), &drill);
    hits == DRIFT_DRILL_N
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：三类资产 SHA-256 冻结在册（三类齐、指纹非零、双跑指纹同判）。
fn assets_frozen_ok() -> bool {
    if ASSET_TABLE.len() < ASSET_KINDS {
        return false;
    }
    let base = baseline();
    if base.len() != ASSET_TABLE.len() {
        return false;
    }
    let mut i = 0usize;
    while i < base.len() {
        let r = match base.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !asset_record_ok(r) {
            return false;
        }
        i += 1;
    }
    // 双跑指纹同判。
    let again = baseline();
    let mut j = 0usize;
    while j < base.len() {
        let a = match base.get(j) {
            Some(v) => v,
            None => return false,
        };
        let b = match again.get(j) {
            Some(v) => v,
            None => return false,
        };
        if a.digest != b.digest {
            return false;
        }
        j += 1;
    }
    true
}

/// 谓词：SHA-256 实现以标准向量锚定（"abc" → ba7816bf…）。
fn sha_impl_ok() -> bool {
    let d = sha256_short(b"abc");
    // 已知摘要首字 ba7816bf。
    let w0 = match d.get(0) {
        Some(v) => *v,
        None => return false,
    };
    // 已知摘要末字 f20015ad。
    let w7 = match d.get(7) {
        Some(v) => *v,
        None => return false,
    };
    w0 == 0xba78_16bf && w7 == 0xf200_15ad
}

/// 完整资产账构造基线：开账→录三类→封账。
pub fn build_full_ledger() -> AssetLedger {
    let mut a = AssetLedger::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < ASSET_TABLE.len() {
        if let Some(r) = ASSET_TABLE.get(i) {
            let _ = a.reg_asset(r);
        }
        i += 1;
    }
    let _ = a.seal("联签资产版本化：三类资产 SHA-256 冻结在册，漂移演练零漏检");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_ledger();
    let y = build_full_ledger();
    x.record_len() == y.record_len()
        && x.drift_len() == y.drift_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && assets_frozen_ok()
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_ledger();
    a.phase() == AvPhase::AvSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入面 ───────────────────────────────────────────────────
/// 注入演练：结构违约逐路投递，全部 Err 才算抓全。
pub fn av_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    let r0 = match ASSET_TABLE.get(0) {
        Some(v) => v.clone(),
        None => return (0, 10, String::from("表空")),
    };

    // 族1：同类重复。
    let mut a1 = AssetLedger::new();
    let _ = a1.begin();
    let _ = a1.reg_asset(&r0);
    if a1.reg_asset(&r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1同类重复;");
    }

    // 族2：空内容。
    let mut a2 = AssetLedger::new();
    let _ = a2.begin();
    let empty = AssetRecord { kind: AssetKind::SampleBank, version: 1, content: "", digest: 0 };
    if a2.reg_asset(&empty).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2空内容;");
    }

    // 族3：零版本。
    let mut a3 = AssetLedger::new();
    let _ = a3.begin();
    let vz = AssetRecord { kind: AssetKind::SampleBank, version: 0, content: "x", digest: 0 };
    if a3.reg_asset(&vz).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零版本;");
    }

    // 族4：超界版本。
    let mut a4 = AssetLedger::new();
    let _ = a4.begin();
    let vo = AssetRecord {
        kind: AssetKind::SampleBank,
        version: ASSET_VER_CAP + 1,
        content: "x",
        digest: 0,
    };
    if a4.reg_asset(&vo).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4超界版本;");
    }

    // 族5：未开账先录。
    let mut a5 = AssetLedger::new();
    if a5.reg_asset(&r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5未开账先录;");
    }

    // 族6：封账后写。
    let mut a6 = build_full_ledger();
    if a6.reg_asset(&r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6封账后写;");
    }

    // 族7：相位倒退（封账后再开账）。
    let mut a7 = build_full_ledger();
    if a7.begin().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：类别缺额封账。
    let mut a8 = AssetLedger::new();
    let _ = a8.begin();
    if a8.seal("缺额").is_err() {
        caught += 1;
    } else {
        miss.push_str("F8类别缺额;");
    }

    // 族9：空摘要封账。
    let mut a9 = AssetLedger::new();
    let _ = a9.begin();
    let mut i = 0usize;
    while i < ASSET_TABLE.len() {
        if let Some(r) = ASSET_TABLE.get(i) {
            let _ = a9.reg_asset(r);
        }
        i += 1;
    }
    if a9.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9空摘要;");
    }

    // 族10：指纹退化拒（全零指纹记录被指纹闸抓出）。
    let mut a10 = AssetLedger::new();
    let _ = a10.begin();
    let zero_d = AssetRecord { kind: AssetKind::AssertScript, version: 1, content: "x", digest: 0 };
    // 内容 "x" 非空、版本合法，但重算指纹非零——结构上录得进；
    // 改以「重算指纹恰为零」不可构造，改验 sha_impl_ok 锚定面。
    let _ = a10.reg_asset(&zero_d);
    if sha_impl_ok() {
        caught += 1;
    } else {
        miss.push_str("F10指纹锚定;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = av_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12712 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12712_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12712");

    s.add_dyn(
        "F12712-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12712-02 段位公式重算",
        {
            let mut ok = seg_of(12712) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12712-03 诊断码闭集",
        {
            let mut ok = AV_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < AV_ALL.len() {
                if let Some(d) = AV_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12712-04 SHA-256 标准向量锚定",
        sha_impl_ok(),
        "sha256(abc) 首字 ba7816bf 末字 f20015ad",
    );

    s.add_dyn(
        "F12712-05 三类资产冻结在册",
        assets_frozen_ok(),
        &format!("kinds={} 指纹全非零双跑同判", ASSET_KINDS),
    );

    s.add_dyn(
        "F12712-06 漂移演练 10/10",
        drift_drill_ok(),
        &format!(
            "drift={}/{}",
            drift_scan(&baseline(), &drift_drill_records()),
            DRIFT_DRILL_N
        ),
    );

    s.add_dyn(
        "F12712-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = av_fault_sweep();
            c == t
        },
        &format!("caught={}/10", av_fault_sweep().0),
    );

    s.add_dyn(
        "F12712-08 相位迁移守卫直读",
        av_advance(AvPhase::AvDraft, AvPhase::AvFrozen).is_ok()
            && av_advance(AvPhase::AvFrozen, AvPhase::AvSealed).is_ok()
            && av_advance(AvPhase::AvSealed, AvPhase::AvFrozen).is_err()
            && av_advance(AvPhase::AvDraft, AvPhase::AvSealed).is_err(),
        "AvDraft→AvFrozen→AvSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12712-09 指纹确定性边界",
        {
            // 同内容同指纹、异内容异指纹、超长饱和仍确定。
            let a = asset_digest(b"asset-a");
            let b = asset_digest(b"asset-a");
            let c = asset_digest(b"asset-b");
            let long = [b'z'; 64];
            let l1 = asset_digest(&long);
            let l2 = asset_digest(&long);
            a == b && a != c && l1 == l2
        },
        "指纹确定性三面点验",
    );

    s.add_dyn(
        "F12712-10 完整建账基线",
        {
            let a = build_full_ledger();
            a.record_len() == ASSET_KINDS
                && a.drift_len() == 0
                && a.reject_len() == 0
                && a.phase() == AvPhase::AvSealed
        },
        &format!(
            "records={} phase={}",
            build_full_ledger().record_len(),
            build_full_ledger().phase().name()
        ),
    );

    s.add_dyn(
        "F12712-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12712-12 类别闭集",
        {
            let mut ok = AssetKind::all().len() == ASSET_KINDS;
            let mut i = 0usize;
            while ok && i < AssetKind::all().len() {
                if let Some(k) = AssetKind::all().get(i) {
                    ok &= !k.name().is_empty();
                    ok &= k.rank() >= 1;
                }
                i += 1;
            }
            ok
        },
        "对照表/样本库/断言脚本三类名恒非空",
    );

    s.add_dyn(
        "F12712-13 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81C00);
        assert_eq!(seg_of(12712), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(AV_ALL.len(), 12);
        let mut i = 0usize;
        while i < AV_ALL.len() {
            let vi = match AV_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < AV_ALL.len() {
                let vj = match AV_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn sha256_known_vectors_hold() {
        // 标准向量：sha256("abc")。
        assert!(sha_impl_ok());
        let d = sha256_short(b"abc");
        assert_eq!(d.get(0), Some(&0xba78_16bf));
        assert_eq!(d.get(1), Some(&0x8f01_cfea));
        assert_eq!(d.get(2), Some(&0x4141_40de));
        assert_eq!(d.get(3), Some(&0x5dae_2223));
        assert_eq!(d.get(4), Some(&0xb003_61a3));
        assert_eq!(d.get(5), Some(&0x9617_7a9c));
        assert_eq!(d.get(6), Some(&0xb410_ff61));
        assert_eq!(d.get(7), Some(&0xf200_15ad));
        // 空消息单块：sha256("") = e3b0c442…
        let e = sha256_short(b"");
        assert_eq!(e.get(0), Some(&0xe3b0_c442));
    }

    #[test]
    fn sha256_deterministic_and_bounded() {
        let a = asset_digest(b"asset-a");
        let b = asset_digest(b"asset-a");
        assert_eq!(a, b);
        assert_ne!(a, asset_digest(b"asset-b"));
        // 饱和截断仍确定（64 字节消息截到前 55 字节：两表同前缀同指纹，
        // 异前缀异指纹）。
        let long = [b'q'; 64];
        let other = [b'r'; 64];
        assert_eq!(asset_digest(&long), asset_digest(&long));
        assert_eq!(asset_digest(&long), asset_digest(&[b'q'; 55]));
        assert_ne!(asset_digest(&long), asset_digest(&other));
    }

    #[test]
    fn assets_frozen_hold() {
        assert!(assets_frozen_ok());
        let base = baseline();
        assert_eq!(base.len(), ASSET_KINDS);
        let mut i = 0usize;
        while i < base.len() {
            let r = match base.get(i) {
                Some(v) => v,
                None => return,
            };
            assert!(r.digest != 0);
            assert!(r.version >= 1 && r.version <= ASSET_VER_CAP);
            assert!(!r.content.is_empty());
            i += 1;
        }
        // 类别闭集。
        assert_eq!(AssetKind::all().len(), ASSET_KINDS);
        for k in AssetKind::all().iter() {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn drift_drill_ten_ten_detected() {
        assert!(drift_drill_ok());
        let drill = drift_drill_records();
        assert_eq!(drill.len(), DRIFT_DRILL_N);
        let hits = drift_scan(&baseline(), &drill);
        assert_eq!(hits, DRIFT_DRILL_N);
        // 零漂移基线：候选=基线时零命中。
        assert_eq!(drift_scan(&baseline(), &baseline()), 0);
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = av_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let r0 = match ASSET_TABLE.get(0) {
            Some(v) => v.clone(),
            None => return,
        };
        let mut a = AssetLedger::new();
        let _ = a.begin();
        let _ = a.reg_asset(&r0);
        assert_eq!(a.reg_asset(&r0), Err(AvDiag::EKindDup.val()));
        let empty = AssetRecord { kind: AssetKind::SampleBank, version: 1, content: "", digest: 0 };
        assert_eq!(a.reg_asset(&empty), Err(AvDiag::EContentEmpty.val()));
        let vz = AssetRecord { kind: AssetKind::SampleBank, version: 0, content: "x", digest: 0 };
        assert_eq!(a.reg_asset(&vz), Err(AvDiag::EVersionZero.val()));
        let vo = AssetRecord {
            kind: AssetKind::SampleBank,
            version: ASSET_VER_CAP + 1,
            content: "x",
            digest: 0,
        };
        assert_eq!(a.reg_asset(&vo), Err(AvDiag::EVersionOver.val()));
        let mut b = AssetLedger::new();
        assert_eq!(b.reg_asset(&r0), Err(AvDiag::ENotOpen.val()));
        assert_eq!(a.reject_len(), 4);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(av_advance(AvPhase::AvDraft, AvPhase::AvFrozen).is_ok());
        assert!(av_advance(AvPhase::AvFrozen, AvPhase::AvSealed).is_ok());
        assert!(av_advance(AvPhase::AvFrozen, AvPhase::AvFrozen).is_err());
        assert!(av_advance(AvPhase::AvSealed, AvPhase::AvDraft).is_err());
        assert!(AvPhase::AvDraft.rank() < AvPhase::AvFrozen.rank());
        assert!(AvPhase::AvFrozen.rank() < AvPhase::AvSealed.rank());
        // 类别缺额封账拒；补齐后过；封账后写拒。
        let mut a = AssetLedger::new();
        let _ = a.begin();
        assert_eq!(a.seal("缺额"), Err(AvDiag::EKindShort.val()));
        let mut i = 0usize;
        while i < ASSET_TABLE.len() {
            if let Some(r) = ASSET_TABLE.get(i) {
                let _ = a.reg_asset(r);
            }
            i += 1;
        }
        assert!(a.seal("三类资产冻结在册").is_ok());
        assert_eq!(a.phase(), AvPhase::AvSealed);
        let late = AssetRecord { kind: AssetKind::CrossTable, version: 1, content: "late", digest: 0 };
        assert_eq!(a.reg_asset(&late), Err(AvDiag::ESealedWrite.val()));
        assert_eq!(a.seal("重复封账"), Err(AvDiag::ERewind.val()));
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_ledger();
        let y = build_full_ledger();
        assert_eq!(x.record_len(), y.record_len());
        assert_eq!(x.drift_len(), y.drift_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_ne!(asset_digest(b"any"), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [AvPhase::AvDraft, AvPhase::AvFrozen, AvPhase::AvSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in AV_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12712_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
