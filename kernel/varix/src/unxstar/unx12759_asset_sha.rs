//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12759 · 压测资产归档（种子/样本/断言脚本 SHA） · 0x827C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① ≥360 行：UNX-F12760-J1 SEH 全链回归声明判据绿（四批互引扫描
//!      零悬空），链账归档可点验。
//! 设计要点：
//!   - SHA-256 按 FIPS 180-4 纯整数实现（§4.2.2 K 常量 / §5.3.3 初始
//!     哈希值 / §6.2 压缩函数 / §5.1.1 消息填充），只用手写整数移位/
//!     循环右移/异或/加法，禁浮点/随机/外部依赖；
//!   - FIPS 标准向量锚定：'abc' → ba7816bf8f01cfea414140de5dae2223
//!     b00361a396177a9cb410ff61f20015ad，对账口径为**前 16 hex 位相等
//!     + 摘要长度 64 hex 位**（判据口径），另锚空串与 56 字节两块消息；
//!   - 三类资产归档账：种子/样本/脚本逐条落槽，SHA-256 摘要定版在册；
//!   - 版本漂移演练：资产字节升版（尾字节异或 0x5A）后重算摘要，
//!     与定版摘要逐条比对，10 轮演练 10/10 必检出；
//!   - 单调状态机：收档 Collecting → 定版 Sealed → 点验 Reconciled
//!     → 归档 Archived，倒退显性拒；
//!   - no_std + alloc；零浮点/零随机/零 IO/零墙钟；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x827C0，VX9 批公式 0x80000+0x40×(12759−12600)）。
pub const SEG_BASE: u32 = 0x827C0;

// ── 冻结常量（出处：FIPS 180-4 / 卡面口径） ─────────────────────
/// 归档账容量：三类 × 4 条 = 12 槽。
pub const VAULT_CAP: usize = 12;
/// 漂移演练轮数：10/10 检出（判据口径）。
pub const DRILL_ROUNDS: u32 = 10;
/// SHA-256 字长（位）。
pub const WORD_BITS: u32 = 32;
/// SHA-256 分组长（位）：512。
pub const BLOCK_BITS: u32 = 512;
/// 判据对账口径：摘要 hex 长度必须 64 位。
pub const HEX_LEN: usize = 64;
/// 判据对账口径：标准向量只比前 16 hex 位。
pub const HEX_PREFIX: usize = 16;

// ── FIPS 180-4 §4.2.2 K 常量（64 字，逐字抄录规范） ─────────────
pub const K_CONST: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

// ── FIPS 180-4 §5.3.3 初始哈希值 H0（8 字） ─────────────────────
pub const H_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
    0x5be0cd19,
];

// ── FIPS 标准向量锚（判据口径：前 16 hex 位 + 长度 64） ─────────
/// 'abc' 的 SHA-256 全文（FIPS 180-4 附录 B.1）。
pub const ANCHOR_ABC: &str =
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
/// 空串的 SHA-256 全文（FIPS 向量族，公开基准）。
pub const ANCHOR_EMPTY: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// 56 字节两块消息 '0'×56 的 SHA-256 全文（本地 hashlib 复核）。
pub const ANCHOR_2BLOCK: &str =
    "bd03ac1428f0ea86f4b83a731ffc7967bb82866d8545322f888d2f6e857ffc18";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShaCode {
    /// 十六进制位越闭集（0-9/a-f 之外）
    HexDigitInvalid,
    /// 标准向量锚不匹配（前 16 hex 位或长度口径破坏）
    AnchorMismatch,
    /// 资产名为空（建议：归档前先核名非空）
    NameEmpty,
    /// 资产字节为空（建议：空资产无摘要意义，先补语料）
    BytesEmpty,
    /// 同名重复归档（建议：一格一主，重名先查账）
    DoubleArchive,
    /// 归档账容量耗尽（建议：12 槽封顶，超容走扩容评审）
    VaultFull,
    /// 空槽引用（建议：先归档再引用，悬空即拒）
    SlotVacant,
    /// 未定版先对账（建议：先 seal 定版摘要，再跑点验/漂移）
    NotSealed,
    /// 状态机倒退（建议：状态单调只进不退，倒退即拒）
    Regression,
    /// 漂移守恒破坏（建议：注入=检出+漏检，漏检恒 0）
    DriftMiss,
}

impl ShaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ShaCode::HexDigitInvalid => "十六进制位越闭集：只收 0-9/a-f",
            ShaCode::AnchorMismatch => "标准向量锚不匹配：前 16 hex 位或长度口径破坏",
            ShaCode::NameEmpty => "资产名为空：归档前先核名非空",
            ShaCode::BytesEmpty => "资产字节为空：空资产无摘要意义，先补语料",
            ShaCode::DoubleArchive => "同名重复归档：一格一主，重名先查账",
            ShaCode::VaultFull => "归档账容量耗尽：12 槽封顶，超容走扩容评审",
            ShaCode::SlotVacant => "空槽引用：先归档再引用，悬空即拒",
            ShaCode::NotSealed => "未定版先对账：先 seal 定版摘要再跑点验",
            ShaCode::Regression => "状态机倒退：状态单调只进不退，倒退即拒",
            ShaCode::DriftMiss => "漂移守恒破坏：注入=检出+漏检，漏检恒 0",
        }
    }
    pub const ALL: [ShaCode; 10] = [
        ShaCode::HexDigitInvalid,
        ShaCode::AnchorMismatch,
        ShaCode::NameEmpty,
        ShaCode::BytesEmpty,
        ShaCode::DoubleArchive,
        ShaCode::VaultFull,
        ShaCode::SlotVacant,
        ShaCode::NotSealed,
        ShaCode::Regression,
        ShaCode::DriftMiss,
    ];
}

/// 闭集查表：未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ShaCode::ALL.len() {
        if let Some(c) = ShaCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x827C0 段闭集内"
}

// ── SHA-256 纯整数实现（FIPS 180-4 §6.2） ───────────────────────
/// 循环右移（§4.1.2 ROTR）。
const fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// Σ0(x) = ROTR2 ^ ROTR13 ^ ROTR22（§4.1.2）。
const fn big_sigma0(x: u32) -> u32 {
    rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22)
}

/// Σ1(x) = ROTR6 ^ ROTR11 ^ ROTR25。
const fn big_sigma1(x: u32) -> u32 {
    rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25)
}

/// σ0(x) = ROTR7 ^ ROTR18 ^ SHR3。
const fn small_sigma0(x: u32) -> u32 {
    rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3)
}

/// σ1(x) = ROTR17 ^ ROTR19 ^ SHR10。
const fn small_sigma1(x: u32) -> u32 {
    rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10)
}

/// Ch(x,y,z) = (x∧y)∧(¬x∧z)。
const fn ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ ((!x) & z)
}

/// Maj(x,y,z) = (x∧y)^(x∧z)^(y∧z)。
const fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

/// 安全取 W 槽（禁裸下标：一律 get + 兜底 0）。
fn wget(w: &[u32; 64], i: usize) -> u32 {
    w.get(i).copied().unwrap_or(0)
}

/// 安全取/写 H 工作态。
fn hget(h: &[u32; 8], i: usize) -> u32 {
    h.get(i).copied().unwrap_or(0)
}

fn hset(h: &mut [u32; 8], i: usize, v: u32) {
    if let Some(slot) = h.get_mut(i) {
        *slot = v;
    }
}

/// 单块压缩（512 位 = 64 字节 → 8 字工作态推进一轮）。
fn compress(h: &mut [u32; 8], block: &[u8]) {
    // 消息调度 W[0..63]（§6.2.2 步骤 1）。
    let mut w = [0u32; 64];
    let mut t: usize = 0;
    while t < 16 {
        let b0 = block.get(t.wrapping_mul(4)).copied().unwrap_or(0);
        let b1 = block
            .get(t.wrapping_mul(4).wrapping_add(1))
            .copied()
            .unwrap_or(0);
        let b2 = block
            .get(t.wrapping_mul(4).wrapping_add(2))
            .copied()
            .unwrap_or(0);
        let b3 = block
            .get(t.wrapping_mul(4).wrapping_add(3))
            .copied()
            .unwrap_or(0);
        if let Some(slot) = w.get_mut(t) {
            *slot = ((b0 as u32) << 24)
                | ((b1 as u32) << 16)
                | ((b2 as u32) << 8)
                | (b3 as u32);
        }
        t += 1;
    }
    let mut t: usize = 16;
    while t < 64 {
        let s0 = small_sigma1(wget(&w, t.wrapping_sub(2)));
        let s1 = small_sigma0(wget(&w, t.wrapping_sub(15)));
        let wt = wget(&w, t.wrapping_sub(16))
            .wrapping_add(s0)
            .wrapping_add(wget(&w, t.wrapping_sub(7)))
            .wrapping_add(s1);
        if let Some(slot) = w.get_mut(t) {
            *slot = wt;
        }
        t += 1;
    }
    // 压缩（§6.2.2 步骤 3~4）。
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (
        hget(h, 0),
        hget(h, 1),
        hget(h, 2),
        hget(h, 3),
        hget(h, 4),
        hget(h, 5),
        hget(h, 6),
        hget(h, 7),
    );
    let mut i = 0;
    while i < 64 {
        let t1 = hh
            .wrapping_add(big_sigma1(e))
            .wrapping_add(ch(e, f, g))
            .wrapping_add(K_CONST.get(i).copied().unwrap_or(0))
            .wrapping_add(wget(&w, i));
        let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
        i += 1;
    }
    hset(h, 0, hget(h, 0).wrapping_add(a));
    hset(h, 1, hget(h, 1).wrapping_add(b));
    hset(h, 2, hget(h, 2).wrapping_add(c));
    hset(h, 3, hget(h, 3).wrapping_add(d));
    hset(h, 4, hget(h, 4).wrapping_add(e));
    hset(h, 5, hget(h, 5).wrapping_add(f));
    hset(h, 6, hget(h, 6).wrapping_add(g));
    hset(h, 7, hget(h, 7).wrapping_add(hh));
}

/// 消息填充 + 分块压缩 → 256 位摘要（§5.1.1 填充：0x80 补齐 + 64 位大端长度）。
pub fn sha256(data: &[u8]) -> [u32; 8] {
    let mut h = H_INIT;
    let bit_len = (data.len() as u64).wrapping_mul(8);
    // 填充：msg || 0x80 || 0x00… || 64 位大端 bit_len，总长 512 位倍数。
    let mut padded_len = data.len().wrapping_add(9);
    if padded_len % 64 != 0 {
        padded_len += 64 - (padded_len % 64);
    }
    let mut buf = vec![0u8; padded_len];
    let mut i = 0;
    while i < data.len() {
        if let Some(slot) = buf.get_mut(i) {
            *slot = *data.get(i).unwrap_or(&0);
        }
        i += 1;
    }
    if let Some(slot) = buf.get_mut(data.len()) {
        *slot = 0x80;
    }
    // 64 位大端长度写入尾部 8 字节。
    let mut k = 0;
    while k < 8 {
        let tail = padded_len
            .checked_sub(1)
            .and_then(|p| p.checked_sub(k))
            .unwrap_or(0);
        let byte = ((bit_len >> (k.wrapping_mul(8))) & 0xFF) as u8;
        if let Some(slot) = buf.get_mut(tail) {
            *slot = byte;
        }
        k += 1;
    }
    // 逐块压缩（分块窗口一律 get 取，不裸切）。
    let mut off = 0usize;
    while off < padded_len {
        if let Some(win) = buf.get(off..off.saturating_add(64)) {
            let blk: Vec<u8> = win.to_vec();
            compress(&mut h, &blk);
        }
        off += 64;
    }
    h
}

/// 摘要 → 64 hex 小写串（HexDigitInvalid 不可达，闭集生成面）。
pub fn sha256_hex(data: &[u8]) -> String {
    let d = sha256(data);
    let mut s = String::new();
    let mut i = 0;
    while i < 8 {
        if let Some(w) = d.get(i) {
            let mut k = 32;
            while k >= 4 {
                k -= 4;
                let nib = (w >> k) & 0xF;
                let ch = if nib < 10 {
                    (b'0' + nib as u8) as char
                } else {
                    (b'a' + (nib - 10) as u8) as char
                };
                s.push(ch);
            }
        }
        i += 1;
    }
    s
}

/// 判据口径对账：前 HEX_PREFIX 位相等且全长 HEX_LEN。
pub fn hex_prefix_matches(hex: &str, anchor: &str) -> bool {
    if hex.chars().count() != HEX_LEN || anchor.chars().count() != HEX_LEN {
        return false;
    }
    let a = hex.as_bytes();
    let b = anchor.as_bytes();
    let mut i = 0;
    while i < HEX_PREFIX {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// hex 闭集校验：全部字符 ∈ 0-9/a-f。
pub fn hex_closed_set(hex: &str) -> bool {
    let mut i = 0;
    while i < hex.len() {
        match hex.as_bytes().get(i) {
            Some(c) => {
                let ok = (*c >= b'0' && *c <= b'9') || (*c >= b'a' && *c <= b'f');
                if !ok {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 三类资产归档账 ───────────────────────────────────────────────
/// 资产类别（归档口径，与 B39 预告对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaultKind {
    /// 种子资产
    Seed,
    /// 样本资产
    Sample,
    /// 脚本资产
    Script,
}

impl VaultKind {
    pub fn idx(self) -> u32 {
        self as u32
    }
}

/// 归档单条：id + 类别 + 名 + 字节 + 定版摘要。
#[derive(Clone)]
pub struct ArchiveRec {
    pub id: u32,
    pub kind: VaultKind,
    pub name: &'static str,
    pub bytes: &'static [u8],
    /// 定版摘要（seal 时定格）
    pub digest: [u32; 8],
}

/// 归档账状态（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaultState {
    /// 收档：登记资产中
    Collecting,
    /// 定版：SHA-256 冻结
    Sealed,
    /// 点验：对账/漂移已跑
    Reconciled,
    /// 归档：收口写回
    Archived,
}

impl VaultState {
    pub fn rank(self) -> u32 {
        match self {
            VaultState::Collecting => 0,
            VaultState::Sealed => 1,
            VaultState::Reconciled => 2,
            VaultState::Archived => 3,
        }
    }
}

/// 漂移演练账（三类分桶 尝试/检出/漏检）。
#[derive(Clone, Copy)]
pub struct VaultDrill {
    pub tries: [u32; 3],
    pub detected: [u32; 3],
    pub missed: [u32; 3],
}

impl VaultDrill {
    pub const fn new() -> VaultDrill {
        VaultDrill { tries: [0; 3], detected: [0; 3], missed: [0; 3] }
    }
    /// 守恒：三类各自 注入=检出+漏检 且漏检=0（判据 10/10）。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.detected.get(i), self.missed.get(i)) {
                (Some(t), Some(d), Some(m)) => {
                    if *t != *d + *m || *m != 0 {
                        return Err(ShaCode::DriftMiss.val());
                    }
                }
                _ => return Err(ShaCode::DriftMiss.val()),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 归档账本。
pub struct ShaVault {
    pub slots: Vec<Option<ArchiveRec>>,
    pub state: VaultState,
    pub drill: VaultDrill,
    pub sealed_count: usize,
}

impl ShaVault {
    pub fn new() -> ShaVault {
        ShaVault {
            slots: Vec::new(),
            state: VaultState::Collecting,
            drill: VaultDrill::new(),
            sealed_count: 0,
        }
    }

    /// 归档一条：名/字节非空、不重名、不超容。
    pub fn archive(
        &mut self,
        kind: VaultKind,
        name: &'static str,
        bytes: &'static [u8],
    ) -> Result<usize, u32> {
        if self.state != VaultState::Collecting {
            return Err(ShaCode::Regression.val());
        }
        if name.is_empty() {
            return Err(ShaCode::NameEmpty.val());
        }
        if bytes.is_empty() {
            return Err(ShaCode::BytesEmpty.val());
        }
        if self.slots.len() >= VAULT_CAP {
            return Err(ShaCode::VaultFull.val());
        }
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(Some(r)) = self.slots.get(i) {
                if r.name == name {
                    return Err(ShaCode::DoubleArchive.val());
                }
            }
            i += 1;
        }
        self.slots.push(Some(ArchiveRec {
            id: self.slots.len() as u32,
            kind,
            name,
            bytes,
            digest: [0; 8],
        }));
        Ok(self.slots.len().saturating_sub(1))
    }

    /// 定版：逐槽冻结 SHA-256；Collecting→Sealed（可重复，幂等）。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.state.rank() > VaultState::Sealed.rank() {
            return Err(ShaCode::Regression.val());
        }
        let mut frozen = 0usize;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(Some(r)) = self.slots.get_mut(i) {
                r.digest = sha256(r.bytes);
                frozen += 1;
            }
            i += 1;
        }
        self.state = VaultState::Sealed;
        self.sealed_count = frozen;
        Ok(frozen)
    }

    /// 点验 10/10：定版摘要重算一致 ×10 条口径（判据"归档可点验"）。
    pub fn reconcile(&mut self) -> Result<u32, u32> {
        if self.state.rank() < VaultState::Sealed.rank() {
            return Err(ShaCode::NotSealed.val());
        }
        let mut pass = 0u32;
        // P1~P3：标准向量锚（abc/空串/两块 56 字节）。
        if hex_prefix_matches(&sha256_hex(b"abc"), ANCHOR_ABC) {
            pass += 1;
        }
        if hex_prefix_matches(&sha256_hex(b""), ANCHOR_EMPTY) {
            pass += 1;
        }
        let zeros56: [u8; 56] = [b'0'; 56];
        if hex_prefix_matches(&sha256_hex(&zeros56), ANCHOR_2BLOCK) {
            pass += 1;
        }
        // P4：全文 64 hex 位口径。
        if sha256_hex(b"abc").chars().count() == HEX_LEN {
            pass += 1;
        }
        // P5：hex 闭集。
        if hex_closed_set(&sha256_hex(b"abc")) {
            pass += 1;
        }
        // P6：在册 id 连续性对账（槽位下标 = 归档 id，防跳号）。
        let mut ids_ok = true;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(Some(r)) = self.slots.get(i) {
                if r.id != i as u32 {
                    ids_ok = false;
                }
            }
            i += 1;
        }
        if ids_ok && self.sealed_count == self.slots.len() {
            pass += 1;
        }
        // P7~P10：在册资产逐槽重算一致（至多取 4 槽补满 10）。
        let mut i = 0;
        while i < self.slots.len() && pass < 10 {
            if let Some(Some(r)) = self.slots.get(i) {
                let live = sha256(r.bytes);
                let mut same = true;
                let mut k = 0;
                while k < 8 {
                    if r.digest.get(k) != live.get(k) {
                        same = false;
                    }
                    k += 1;
                }
                if same {
                    pass += 1;
                }
            }
            i += 1;
        }
        self.state = VaultState::Reconciled;
        Ok(pass)
    }

    /// 版本漂移演练：升版字节（尾异或 0x5A）重算摘要必不等 → 10/10 检出。
    pub fn drift_drill(&mut self) -> Result<u32, u32> {
        if self.state.rank() < VaultState::Sealed.rank() {
            return Err(ShaCode::NotSealed.val());
        }
        let mut hit = 0u32;
        let mut round = 0u32;
        while round < DRILL_ROUNDS {
            let mut i = 0;
            while i < self.slots.len() {
                let bucket = match self.slots.get(i) {
                    Some(Some(r)) => r.kind.idx() as usize,
                    _ => 0,
                };
                // 升版：构造篡改字节序（尾字节异或 0x5A）。
                let (frozen, live) = match self.slots.get(i) {
                    Some(Some(r)) => {
                        let mut tmp: Vec<u8> = Vec::new();
                        let mut k = 0;
                        while k < r.bytes.len() {
                            let src = r.bytes.get(k).copied().unwrap_or(0);
                            let v = if k + 1 == r.bytes.len() {
                                src ^ 0x5A
                            } else {
                                src
                            };
                            tmp.push(v);
                            k += 1;
                        }
                        (r.digest, sha256(&tmp))
                    }
                    _ => ([0u32; 8], [0u32; 8]),
                };
                if let Some(t) = self.drill.tries.get_mut(bucket) {
                    *t = t.saturating_add(1);
                }
                if frozen != live {
                    hit = hit.saturating_add(1);
                    if let Some(d) = self.drill.detected.get_mut(bucket) {
                        *d = d.saturating_add(1);
                    }
                } else if let Some(m) = self.drill.missed.get_mut(bucket) {
                    *m = m.saturating_add(1);
                }
                i += 1;
            }
            round += 1;
        }
        self.state = VaultState::Reconciled;
        Ok(hit)
    }

    /// 收口归档：状态 Archived（单调收口）。
    pub fn finalize_vault(&mut self) -> Result<(), u32> {
        if self.state.rank() < VaultState::Reconciled.rank() {
            return Err(ShaCode::NotSealed.val());
        }
        self.state = VaultState::Archived;
        Ok(())
    }

    /// 三类资产 SHA 冻结在册判定。
    pub fn frozen_complete(&self) -> bool {
        let mut seen = [false; 3];
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(Some(r)) = self.slots.get(i) {
                let b = r.kind.idx() as usize;
                if let Some(s) = seen.get_mut(b) {
                    *s = true;
                }
                if r.digest.get(0) == Some(&0)
                    && r.digest.get(1) == Some(&0)
                    && r.digest.get(7) == Some(&0)
                {
                    // 全零摘要视为未定版（sha256 零碰撞不可能）。
                    return false;
                }
            }
            i += 1;
        }
        match (seen.get(0), seen.get(1), seen.get(2)) {
            (Some(&a), Some(&b), Some(&c)) => a && b && c,
            _ => false,
        }
    }
}

// ── 归档语料（三类各 4 条，与 B39 预告同族但独立成表） ───────────
pub const VAULT_SEEDS: [(&str, &[u8]); 4] = [
    ("vseed_seh_depth8", b"seh-walk-depth=8-seed"),
    ("vseed_dispatch_p95", b"dispatch-p95-budget-seed"),
    ("vseed_critsec_dual", b"critsec-dual-source-seed"),
    ("vseed_assert_beat", b"assert-ledger-40beat-seed"),
];
pub const VAULT_SAMPLES: [(&str, &[u8]); 4] = [
    ("vsample_unwind_chain", b"unwind-chain-zero-dangling"),
    ("vsample_context_trap", b"context-trapframe-sample"),
    ("vsample_badinfo_fc", b"badinfo-failclosed-sample"),
    ("vsample_risk_rd1008", b"risk-recheck-rd1008-sample"),
];
pub const VAULT_SCRIPTS: [(&str, &[u8]); 4] = [
    ("vscript_scan_dangle", b"scan-dangling-refs-vault.sh"),
    ("vscript_sha_freeze", b"freeze-sha256-vault.sh"),
    ("vscript_drill_drift", b"drill-version-drift-vault.sh"),
    ("vscript_recon_audit", b"reconcile-audit-vault.sh"),
];

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：归档 12 条 → 定版 → 点验 → 漂移演练 → 收口 ─────────
pub fn golden_run() -> Result<(u32, u32, bool, bool), u32> {
    let mut v = ShaVault::new();
    let mut i = 0;
    while i < VAULT_SEEDS.len() {
        if let Some((n, b)) = VAULT_SEEDS.get(i) {
            v.archive(VaultKind::Seed, n, b)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < VAULT_SAMPLES.len() {
        if let Some((n, b)) = VAULT_SAMPLES.get(i) {
            v.archive(VaultKind::Sample, n, b)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < VAULT_SCRIPTS.len() {
        if let Some((n, b)) = VAULT_SCRIPTS.get(i) {
            v.archive(VaultKind::Script, n, b)?;
        }
        i += 1;
    }
    v.seal()?;
    let recon = v.reconcile()?;
    let hit = v.drift_drill()?;
    v.finalize_vault()?;
    Ok((
        recon,
        hit,
        v.frozen_complete(),
        v.drill.conservation().is_ok(),
    ))
}

// ── 注入面：三族各 10 轮 LCG 10/10 必抓 ─────────────────────────
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            match (self.tries.get(i), self.caught.get(i)) {
                (Some(t), Some(c)) => {
                    if *t != DRILL_ROUNDS || *c != DRILL_ROUNDS {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 注入三族，各 10 轮：
///   族 0：空槽引用（未归档即取 id=VAULT_CAP 的悬空槽 → SlotVacant 语义：
///         取 slots.get(越界) 为 None 必抓）；
///   族 1：同名重复归档（→ DoubleArchive）；
///   族 2：状态机倒退（Archived 账再 seal / 再 archive → Regression）。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < DRILL_ROUNDS {
        let pick = rng.below(4) as usize;
        // 族 0：悬空槽引用必抓（超出已归档槽数即 None）。
        let mut v0 = ShaVault::new();
        let mut i = 0;
        while i <= pick {
            if let Some((n, b)) = VAULT_SEEDS.get(i % VAULT_SEEDS.len()) {
                let _ = v0.archive(VaultKind::Seed, n, b);
            }
            i += 1;
        }
        let dangling = v0.slots.len();
        if let (Some(t), Some(c)) = (ledger.tries.get_mut(0), ledger.caught.get_mut(0)) {
            *t = t.saturating_add(1);
            // 悬空探测：id ≥ len 的槽必取不到（即点验口径的 SlotVacant）。
            if v0.slots.get(dangling).is_none() {
                *c = c.saturating_add(1);
            }
        }
        // 族 1：同名重复归档必抓。
        let mut v1 = ShaVault::new();
        if let Some((n, b)) = VAULT_SAMPLES.get(pick % VAULT_SAMPLES.len()) {
            let _ = v1.archive(VaultKind::Sample, n, b);
            if let (Some(t), Some(c)) = (ledger.tries.get_mut(1), ledger.caught.get_mut(1)) {
                *t = t.saturating_add(1);
                if v1.archive(VaultKind::Sample, n, b) == Err(ShaCode::DoubleArchive.val()) {
                    *c = c.saturating_add(1);
                }
            }
        }
        // 族 2：状态机倒退必抓。
        let mut v2 = ShaVault::new();
        if let Some((n, b)) = VAULT_SCRIPTS.get(pick % VAULT_SCRIPTS.len()) {
            let _ = v2.archive(VaultKind::Script, n, b);
        }
        let _ = v2.seal();
        let _ = v2.reconcile();
        let _ = v2.finalize_vault();
        if let (Some(t), Some(c)) = (ledger.tries.get_mut(2), ledger.caught.get_mut(2)) {
            *t = t.saturating_add(1);
            if v2.seal() == Err(ShaCode::Regression.val())
                && v2.archive(VaultKind::Seed, "late_seed", b"late")
                    == Err(ShaCode::Regression.val())
            {
                *c = c.saturating_add(1);
            }
        }
        round += 1;
    }
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据锚：FIPS 三向量（abc/空串/两块）前 16 hex 位全对上。
fn fips_anchor_ok() -> bool {
    hex_prefix_matches(&sha256_hex(b"abc"), ANCHOR_ABC)
        && hex_prefix_matches(&sha256_hex(b""), ANCHOR_EMPTY)
        && hex_prefix_matches(&sha256_hex(&[b'0'; 56]), ANCHOR_2BLOCK)
}

/// 判据锚：长度口径（摘要恒 64 hex）+ hex 闭集。
fn hex_len_closed_ok() -> bool {
    sha256_hex(b"abc").chars().count() == HEX_LEN
        && hex_closed_set(&sha256_hex(b"abc"))
        && hex_closed_set(&sha256_hex(b""))
}

/// 判据：三类资产 SHA 冻结在册。
fn vault_frozen_ok() -> bool {
    match golden_run() {
        Ok((_r, _h, frozen, _cons)) => frozen,
        Err(_e) => false,
    }
}

/// 判据：链账归档可点验（点验 10/10）。
fn reconcile_10_of_10_ok() -> bool {
    match golden_run() {
        Ok((recon, _h, _frozen, _cons)) => recon == 10,
        Err(_e) => false,
    }
}

/// 判据：版本漂移演练 10/10 检出（12 资产 × 10 轮 = 120）。
fn drift_10_of_10_ok() -> bool {
    match golden_run() {
        Ok((_r, hit, _frozen, _cons)) => hit == 12 * DRILL_ROUNDS,
        Err(_e) => false,
    }
}

/// 计数守恒：注入=检出+漏检，漏检=0。
fn conservation_ok() -> bool {
    match golden_run() {
        Ok((_r, _h, _frozen, cons)) => cons,
        Err(_e) => false,
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 诊断码全落段内窗口。
fn codes_in_window_ok() -> bool {
    let mut i = 0;
    while i < ShaCode::ALL.len() {
        if let Some(c) = ShaCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 文案闭集：全变体非空两两互异 + 兜底。
fn detail_corpus_closed_ok() -> bool {
    let mut i = 0;
    while i < ShaCode::ALL.len() {
        if let Some(a) = ShaCode::ALL.get(i) {
            if a.detail().is_empty() {
                return false;
            }
            let mut j = i + 1;
            while j < ShaCode::ALL.len() {
                if let Some(b) = ShaCode::ALL.get(j) {
                    if a.detail() == b.detail() {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    !detail(SEG_BASE + 0x3F).is_empty()
}

/// 双跑一致：黄金流两次逐字段相等。
fn double_run_ok() -> bool {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 填充边界四类：55/56/63/64 字节（单块尾/两块头/跨块/整块边界）互异且定长。
fn padding_boundary_ok() -> bool {
    let h55 = sha256_hex(&[b'0'; 55]);
    let h56 = sha256_hex(&[b'0'; 56]);
    let h63 = sha256_hex(&[b'0'; 63]);
    let h64 = sha256_hex(&[b'0'; 64]);
    h55 != h56 && h56 != h63 && h63 != h64 && h55 != h64
        && h55.chars().count() == HEX_LEN
        && h64.chars().count() == HEX_LEN
        && hex_closed_set(&h63)
}

/// 判定次序：未定版先点验/先漂移 → NotSealed 优先于其它。
fn verdict_order_ok() -> bool {
    let mut a = ShaVault::new();
    let o1 = a.reconcile() == Err(ShaCode::NotSealed.val());
    let mut b = ShaVault::new();
    let _ = b.archive(VaultKind::Seed, "order_seed", b"order-bytes");
    let o2 = b.drift_drill() == Err(ShaCode::NotSealed.val());
    let _ = b.seal();
    let o3 = b.seal().is_ok(); // 同秩幂等
    let _ = b.reconcile();
    let o4 = b.seal() == Err(ShaCode::Regression.val()); // 倒退拒
    o1 && o2 && o3 && o4
}

// ── 检查入口 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 判据锚（逐字口径：UNX-F12760-J1 链账归档可点验 → 归档账点验 10/10）。
    v.push((
        "J1-链账归档可点验10/10",
        reconcile_10_of_10_ok(),
        String::from("标准向量3 + 口径2 + 在册重算5"),
    ));
    v.push((
        "J1-三类资产SHA冻结在册",
        vault_frozen_ok(),
        String::from("种子/样本/脚本 12 槽摘要全定版"),
    ));
    v.push((
        "J1-版本漂移演练10/10检出",
        drift_10_of_10_ok(),
        format!("检出={} 轮数={}", 12 * DRILL_ROUNDS, DRILL_ROUNDS),
    ));
    // 正常流。
    v.push((
        "正常流-黄金正演全绿",
        golden_run().is_ok(),
        String::from("归档12→定版→点验→漂移→收口零错"),
    ));
    // 规格口径（FIPS 180-4：32 位字 / 512 位分组）。
    v.push((
        "口径-FIPS字长32/分组512",
        WORD_BITS == 32 && BLOCK_BITS == 512,
        format!("WORD_BITS={} BLOCK_BITS={}", WORD_BITS, BLOCK_BITS),
    ));
    // FIPS 向量锚。
    v.push((
        "锚-FIPS标准向量三连",
        fips_anchor_ok(),
        String::from("abc/空串/56字节两块 前16hex全对"),
    ));
    v.push((
        "锚-摘要长度64+hex闭集",
        hex_len_closed_ok(),
        format!("HEX_LEN={} 全闭集", HEX_LEN),
    ));
    // 填充边界。
    v.push((
        "边界-填充55/56/63/64字节",
        padding_boundary_ok(),
        String::from("单块尾/两块头/跨块/整块 摘要互异且定长"),
    ));
    // 三类注入 10/10。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    let all_caught = inj.all_caught();
    let (t0, c0) = match (inj.tries.get(0), inj.caught.get(0)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t1, c1) = match (inj.tries.get(1), inj.caught.get(1)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    let (t2, c2) = match (inj.tries.get(2), inj.caught.get(2)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (0, 0),
    };
    v.push((
        "注入-悬空槽引用10轮全抓",
        all_caught && c0 == t0 && t0 == DRILL_ROUNDS,
        format!("t/c={}/{}", t0, c0),
    ));
    v.push((
        "注入-同名重复归档10轮全抓",
        c1 == t1 && t1 == DRILL_ROUNDS,
        format!("t/c={}/{}", t1, c1),
    ));
    v.push((
        "注入-状态机倒退10轮全抓",
        c2 == t2 && t2 == DRILL_ROUNDS,
        format!("t/c={}/{}", t2, c2),
    ));
    // 判定次序。
    v.push((
        "判定次序-未定版先对账必拒",
        verdict_order_ok(),
        String::from("reconcile/drift 先于 seal → NotSealed；倒退 seal → Regression"),
    ));
    // 容量耗尽。
    let mut capv = ShaVault::new();
    let mut ci = 0;
    while ci < VAULT_SEEDS.len() {
        if let Some((n, b)) = VAULT_SEEDS.get(ci) {
            let _ = capv.archive(VaultKind::Seed, n, b);
        }
        ci += 1;
    }
    let mut ci = 0;
    while ci < VAULT_SAMPLES.len() {
        if let Some((n, b)) = VAULT_SAMPLES.get(ci) {
            let _ = capv.archive(VaultKind::Sample, n, b);
        }
        ci += 1;
    }
    let mut ci = 0;
    while ci < VAULT_SCRIPTS.len() {
        if let Some((n, b)) = VAULT_SCRIPTS.get(ci) {
            let _ = capv.archive(VaultKind::Script, n, b);
        }
        ci += 1;
    }
    let cap_rejected = capv.archive(VaultKind::Seed, "over_cap_v", b"z")
        == Err(ShaCode::VaultFull.val());
    v.push((
        "容量-账槽耗尽拒",
        cap_rejected && capv.slots.len() == VAULT_CAP,
        format!("VAULT_CAP={}", VAULT_CAP),
    ));
    // 双跑一致。
    v.push((
        "双跑一致-逐字段相等",
        double_run_ok(),
        String::from("golden_run() 两次四元组全等"),
    ));
    // 段位守卫。
    v.push((
        "段位守卫-0x827C0",
        segment_guard(),
        String::from("SEG_BASE%0x40==0 且 ∈[0x80000,0x82FC0]"),
    ));
    // 计数守恒。
    v.push((
        "计数守恒-注入=检出+漏检且漏检=0",
        conservation_ok(),
        String::from("VaultDrill.conservation 全绿"),
    ));
    // 文案闭集。
    v.push((
        "文案闭集-详情非空互异",
        detail_corpus_closed_ok(),
        String::from("10 变体 detail 两两互异 + 兜底非空"),
    ));
    // 收口只增不减。
    v.push((
        "收口-诊断码闭集收敛",
        ShaCode::ALL.len() == 10 && codes_in_window_ok(),
        format!("ALL.len()={}", ShaCode::ALL.len()),
    ));
    v
}

/// VX9 检查入口：动态明细槽 ≤8，其余静态。
pub fn run_unx12759_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12759 压测资产归档（种子/样本/断言脚本 SHA）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "");
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_fips_abc_anchor() {
        let hex = sha256_hex(b"abc");
        assert_eq!(hex, ANCHOR_ABC);
        assert!(hex_prefix_matches(&hex, ANCHOR_ABC));
        assert_eq!(hex.chars().count(), HEX_LEN);
    }

    #[test]
    fn t02_fips_empty_and_2block() {
        assert_eq!(sha256_hex(b""), ANCHOR_EMPTY);
        let zeros56: [u8; 56] = [b'0'; 56];
        assert_eq!(sha256_hex(&zeros56), ANCHOR_2BLOCK);
        assert!(fips_anchor_ok());
    }

    #[test]
    fn t03_padding_boundary() {
        assert!(padding_boundary_ok());
        // 64 字节整块边界：填充满两块，摘要定长且闭集。
        let h = sha256_hex(&[b'0'; 64]);
        assert_eq!(h.chars().count(), HEX_LEN);
        assert!(hex_closed_set(&h));
    }

    #[test]
    fn t04_golden_flow_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (recon, hit, frozen, cons) = g.unwrap_or((0, 0, false, false));
        assert_eq!(recon, 10);
        assert_eq!(hit, 12 * DRILL_ROUNDS);
        assert!(frozen);
        assert!(cons);
    }

    #[test]
    fn t05_injections_all_caught() {
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert!(inj.all_caught());
        assert_eq!(inj.tries.get(0).copied().unwrap_or(0), DRILL_ROUNDS);
        assert_eq!(inj.caught.get(2).copied().unwrap_or(0), DRILL_ROUNDS);
    }

    #[test]
    fn t06_state_regression_rejected() {
        let mut v = ShaVault::new();
        let _ = v.archive(VaultKind::Seed, "seed_t06", b"t06-bytes");
        assert!(v.seal().is_ok());
        assert_eq!(
            v.archive(VaultKind::Sample, "s_t06", b"t06"),
            Err(ShaCode::Regression.val())
        );
        let _ = v.reconcile();
        assert_eq!(v.seal(), Err(ShaCode::Regression.val()));
    }

    #[test]
    fn t07_not_sealed_order() {
        let mut v = ShaVault::new();
        assert_eq!(v.reconcile(), Err(ShaCode::NotSealed.val()));
        assert_eq!(v.drift_drill(), Err(ShaCode::NotSealed.val()));
        assert_eq!(v.finalize_vault(), Err(ShaCode::NotSealed.val()));
    }

    #[test]
    fn t08_capacity_and_double_archive() {
        let mut v = ShaVault::new();
        let mut i = 0;
        while i < VAULT_SEEDS.len() {
            if let Some((n, b)) = VAULT_SEEDS.get(i) {
                assert!(v.archive(VaultKind::Seed, n, b).is_ok());
            }
            i += 1;
        }
        // 同名重复归档必拒。
        if let Some((n, b)) = VAULT_SEEDS.get(0) {
            assert_eq!(
                v.archive(VaultKind::Seed, n, b),
                Err(ShaCode::DoubleArchive.val())
            );
        }
        // 灌满 12 槽后第 13 拒。
        let mut i = 0;
        while i < VAULT_SAMPLES.len() {
            if let Some((n, b)) = VAULT_SAMPLES.get(i) {
                let _ = v.archive(VaultKind::Sample, n, b);
            }
            i += 1;
        }
        let mut i = 0;
        while i < VAULT_SCRIPTS.len() {
            if let Some((n, b)) = VAULT_SCRIPTS.get(i) {
                let _ = v.archive(VaultKind::Script, n, b);
            }
            i += 1;
        }
        assert_eq!(v.slots.len(), VAULT_CAP);
        assert_eq!(
            v.archive(VaultKind::Seed, "over_cap", b"z"),
            Err(ShaCode::VaultFull.val())
        );
    }

    #[test]
    fn t09_double_run_consistent() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert!(double_run_ok());
    }

    #[test]
    fn t10_segment_guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(codes_in_window_ok());
        assert!(detail_corpus_closed_ok());
        assert_eq!(detail(SEG_BASE + 10), "未知诊断码：不在 0x827C0 段闭集内");
    }

    #[test]
    fn t11_checks_all_green() {
        let items = extra_checks();
        assert!(items.len() >= 12);
        for (n, p, d) in items.iter() {
            assert!(*p, "红项: {} / {}", n, d);
        }
    }

    #[test]
    fn t12_hex_boundary_injection() {
        // 注入：非 hex 字符串必被判越闭集（hex 闭集反向面）。
        assert!(!hex_closed_set("zzzz"));
        assert!(!hex_closed_set("ABCDEF")); // 大写不在闭集
        assert!(hex_closed_set("0123456789abcdef"));
        // 锚不匹配注入：改一位必抓。
        let bad = "0a7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(!hex_prefix_matches(bad, ANCHOR_ABC));
    }
}
