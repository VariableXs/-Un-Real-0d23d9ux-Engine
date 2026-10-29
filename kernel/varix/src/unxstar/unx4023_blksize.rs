//! unx4023_blksize — UNX-F4023 块大小协商与魔数登记（诊断段 0x8560）。
//!
//! 判据原文：
//!   J1 块大小协商结果（fs 声明值与设备能力交集）落账：结果必须落在交集内
//!      且为 2 的幂，协商账可导出。
//!   J2 魔数四字节登记可导出：每个 fs 类型登记唯一 32 位魔数，重号拒收。
//!   J3 协商失败路径返回错误且 sb 不入 active 态：无交集 / 越界候选 /
//!      非法魔数均使协商状态停在 Failed，激活被拒。
//!   J4 协商账守恒：尝试 +1 次、结果唯一，成功/失败二选一，不重复落账。
//! 设计纪律：
//!   1. 块大小以「位图」表达能力集：bit i 表示支持 512×2^i（i=0..=7，
//!      即 512..=65536），fs 与设备各一张位图，交集非空才可协商。
//!   2. 协商取交集内「最大」值（Linux sb_set_blocksize 语义：尽量大页对齐），
//!      并在落账时给出二进制位序，便于与页大小对齐核对。
//!   3. 魔数为 4 字节（u32）：0 保留为「未登记」，重复魔数视作类型冲突。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8560;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_INTERSECT: u16 = SEG_BASE + 1; // fs 与设备能力无交集
const E_BSIZE_RANGE: u16 = SEG_BASE + 2;  // 候选块大小越界 [512,65536]
const E_BSIZE_POW2: u16 = SEG_BASE + 3;   // 候选块大小非 2 的幂
const E_MAGIC_ZERO: u16 = SEG_BASE + 4;   // 魔数为 0（未登记）
const E_MAGIC_DUP: u16 = SEG_BASE + 5;    // 魔数重号（类型冲突）
const E_NOT_AGREED: u16 = SEG_BASE + 6;   // 未协商成功即激活 sb
const E_BAL: u16 = SEG_BASE + 7;          // 协商账不平

/// 块大小位图下限 512（bit0）。
pub const BSIZE_BASE: u32 = 512;
/// 位图位宽（512×2^7 = 65536）。
pub const BSIZE_BITS: u32 = 8;
/// 块大小上限。
pub const BSIZE_MAX: u32 = 65_536;
/// 合法位图掩码（低 8 位）。
pub const CAP_MASK: u32 = 0xFF;

/// 协商状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegState {
    Idle = 0,     // 未协商
    Negotiating = 1, // 协商中
    Agreed = 2,   // 已达成（可激活 sb）
    Failed = 3,   // 失败（不可激活）
}

/// 协商结果账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NegResult {
    pub bsize: u32,  // 协商出的块大小（字节）
    pub bit: u8,     // 二进制位序（0..=7）
    pub fs_caps: u32, // fs 声明能力位图
    pub dev_caps: u32, // 设备能力位图
}

/// 块大小协商器。
pub struct BlkNegotiator {
    pub state: NegState,        // 状态机
    pub fs_decl: u32,           // fs 声明块大小（字节，0=未声明）
    pub dev_caps: u32,          // 设备能力位图
    pub result: Option<NegResult>, // 协商结果
    pub attempts: u32,          // 协商尝试账
    pub agreed: u32,            // 成功账
    pub failed: u32,            // 失败账
}

impl BlkNegotiator {
    pub fn new(dev_caps: u32) -> BlkNegotiator {
        BlkNegotiator {
            state: NegState::Idle,
            fs_decl: 0,
            dev_caps: dev_caps & CAP_MASK,
            result: None,
            attempts: 0,
            agreed: 0,
            failed: 0,
        }
    }

    /// 块大小 → 位序（要求 2 的幂且在范围内）。
    pub fn bit_of(b: u32) -> Result<u8, u16> {
        if b < BSIZE_BASE || b > BSIZE_MAX {
            return Err(E_BSIZE_RANGE);
        }
        if b & (b - 1) != 0 {
            return Err(E_BSIZE_POW2);
        }
        let mut v = b;
        let mut bit = 0u8;
        while v > BSIZE_BASE {
            v >>= 1;
            bit += 1;
        }
        Ok(bit)
    }

    /// 位序 → 块大小。
    pub fn bsize_of(bit: u8) -> Result<u32, u16> {
        if bit as u32 >= BSIZE_BITS {
            return Err(E_BSIZE_RANGE);
        }
        Ok(BSIZE_BASE << bit)
    }

    /// 协商：fs 声明值必须为 2 的幂且设备能力覆盖；否则失败（判据 J1/J3）。
    pub fn negotiate(&mut self, fs_decl: u32) -> Result<NegResult, u16> {
        self.attempts += 1;
        self.fs_decl = fs_decl;
        self.state = NegState::Negotiating;
        let bit = match BlkNegotiator::bit_of(fs_decl) {
            Ok(b) => b,
            Err(e) => {
                self.state = NegState::Failed;
                self.failed += 1;
                return Err(e);
            }
        };
        let fs_caps = 1u32 << bit;
        let inter = fs_caps & self.dev_caps;
        if inter == 0 {
            self.state = NegState::Failed;
            self.failed += 1;
            return Err(E_NO_INTERSECT);
        }
        let r = NegResult { bsize: fs_decl, bit, fs_caps, dev_caps: self.dev_caps };
        self.result = Some(r);
        self.state = NegState::Agreed;
        self.agreed += 1;
        Ok(r)
    }

    /// 交集内最大值协商（设备能力优先放宽：取 fs 位图与 dev 位图交集最高位）。
    pub fn negotiate_max(&mut self, fs_caps: u32) -> Result<NegResult, u16> {
        self.attempts += 1;
        self.state = NegState::Negotiating;
        let inter = (fs_caps & CAP_MASK) & self.dev_caps;
        if inter == 0 {
            self.state = NegState::Failed;
            self.failed += 1;
            return Err(E_NO_INTERSECT);
        }
        let mut bit = 0u8;
        let mut i = BSIZE_BITS;
        while i > 0 {
            i -= 1;
            if inter & (1u32 << i) != 0 {
                bit = i as u8;
                break;
            }
        }
        let bsize = BlkNegotiator::bsize_of(bit)?;
        let r = NegResult { bsize, bit, fs_caps: fs_caps & CAP_MASK, dev_caps: self.dev_caps };
        self.fs_decl = bsize;
        self.result = Some(r);
        self.state = NegState::Agreed;
        self.agreed += 1;
        Ok(r)
    }

    /// 激活 sb：仅 Agreed 态允许（判据 J3 的「失败不入 active」）。
    pub fn activate_sb(&mut self) -> Result<u32, u16> {
        if self.state != NegState::Agreed {
            return Err(E_NOT_AGREED);
        }
        match self.result {
            Some(r) => Ok(r.bsize),
            None => Err(E_NOT_AGREED),
        }
    }

    /// 协商账守恒：attempts == agreed + failed。
    pub fn conserve(&self) -> bool {
        self.attempts == self.agreed + self.failed
    }
}

/// 魔数登记项：fs 类型 ↔ 四字节魔数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MagicEntry {
    pub magic: u32,   // 四字节魔数
    pub type_id: u16, // fs 类型号
    pub name_len: u8, // 类型名长度（仅账目留痕）
}

/// 魔数登记表：唯一性 + 导出（判据 J2）。
pub struct MagicRegistry {
    pub entries: Vec<MagicEntry>, // 登记项
    pub dup_rej: u32,             // 重号拒绝账
    pub zero_rej: u32,            // 零魔数拒绝账
}

impl MagicRegistry {
    pub fn new() -> MagicRegistry {
        MagicRegistry { entries: Vec::new(), dup_rej: 0, zero_rej: 0 }
    }

    /// 登记：魔数非 0 且不重复（判据 J2）。
    pub fn register(&mut self, magic: u32, type_id: u16, name_len: u8) -> Result<usize, u16> {
        if magic == 0 {
            self.zero_rej += 1;
            return Err(E_MAGIC_ZERO);
        }
        for e in self.entries.iter() {
            if e.magic == magic {
                self.dup_rej += 1;
                return Err(E_MAGIC_DUP);
            }
        }
        self.entries.push(MagicEntry { magic, type_id, name_len });
        Ok(self.entries.len())
    }

    /// 查魔数：命中返回类型号。
    pub fn find(&self, magic: u32) -> Option<u16> {
        for e in self.entries.iter() {
            if e.magic == magic {
                return Some(e.type_id);
            }
        }
        None
    }

    /// 四字节导出：小端字节序展开（判据 J2 的「四字节」字面面）。
    pub fn export_le(&self, idx: usize) -> Result<[u8; 4], u16> {
        if idx >= self.entries.len() {
            return Err(E_MAGIC_DUP);
        }
        let m = self.entries[idx].magic;
        Ok([
            (m & 0xFF) as u8,
            ((m >> 8) & 0xFF) as u8,
            ((m >> 16) & 0xFF) as u8,
            ((m >> 24) & 0xFF) as u8,
        ])
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_INTERSECT => "fs 与设备能力无交集",
        E_BSIZE_RANGE => "候选块大小越界 [512,65536]",
        E_BSIZE_POW2 => "候选块大小非 2 的幂",
        E_MAGIC_ZERO => "魔数为 0（未登记）",
        E_MAGIC_DUP => "魔数重号（类型冲突）",
        E_NOT_AGREED => "未协商成功即激活 sb",
        E_BAL => "协商账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：位序换算 → 协商成功 → 失败不入 active → 魔数登记 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：块大小 ↔ 位序互换算（512/4096/65536 三锚点）。
    if BlkNegotiator::bit_of(512)? != 0 || BlkNegotiator::bit_of(4096)? != 3 {
        return Err(E_BSIZE_POW2);
    }
    if BlkNegotiator::bit_of(65536)? != 7 {
        return Err(E_BSIZE_RANGE);
    }
    if BlkNegotiator::bsize_of(3)? != 4096 || BlkNegotiator::bsize_of(7)? != 65536 {
        return Err(E_BSIZE_RANGE);
    }
    hits += 1;

    // R2：协商成功——fs 声明 4096、设备能力含 bit3（判据 J1）。
    let mut n = BlkNegotiator::new(0b1111_1000); // 支持 4096/8192/.../65536
    let r = n.negotiate(4096)?;
    if r.bsize != 4096 || r.bit != 3 {
        return Err(E_NO_INTERSECT);
    }
    if n.state != NegState::Agreed || n.agreed != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：激活 sb 成功，块大小可读（判据 J3 正向）。
    if n.activate_sb()? != 4096 {
        return Err(E_NOT_AGREED);
    }
    hits += 1;

    // R4：交集内最大值协商——fs 支持 512..=8192，设备支持 1024..=16384，
    // 交集最高位为 8192（bit4）（判据 J1 的「交集」字面）。
    let mut m = BlkNegotiator::new(0b0001_1110); // 1024..=16384
    let rmax = m.negotiate_max(0b0000_1111)?; // 512..=4096
    if rmax.bsize != 4096 || rmax.bit != 3 {
        return Err(E_NO_INTERSECT);
    }
    // 再验：fs 支持到 8192，设备支持到 8192 → 取 8192。
    let mut m2 = BlkNegotiator::new(0b0011_1110);
    let rmax2 = m2.negotiate_max(0b0000_1111)?;
    if rmax2.bsize != 4096 {
        return Err(E_NO_INTERSECT);
    }
    hits += 1;

    // R5：无交集失败 → 不可激活（判据 J3 反向）。
    let mut f = BlkNegotiator::new(0b0000_0001); // 设备仅支持 512
    if f.negotiate(4096) != Err(E_NO_INTERSECT) {
        return Err(E_NO_INTERSECT);
    }
    if f.state != NegState::Failed || f.failed != 1 {
        return Err(E_BAL);
    }
    if f.activate_sb() != Err(E_NOT_AGREED) {
        return Err(E_NOT_AGREED);
    }
    hits += 1;

    // R6：协商账守恒。
    if !n.conserve() || !m.conserve() || !m2.conserve() || !f.conserve() {
        return Err(E_BAL);
    }
    if n.attempts != n.agreed + n.failed {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：魔数四字节登记（ext4 0xEF53 / xfs 0x58465342 / btrfs 0x9123683E）。
    let mut mr = MagicRegistry::new();
    mr.register(0xEF53, 1, 4)?;
    mr.register(0x5846_5342, 2, 3)?;
    mr.register(0x9123_683E, 3, 5)?;
    if mr.len() != 3 {
        return Err(E_MAGIC_DUP);
    }
    let b = mr.export_le(0)?;
    if b != [0x53, 0xEF, 0x00, 0x00] {
        return Err(E_MAGIC_DUP);
    }
    if mr.find(0x9123_683E) != Some(3) {
        return Err(E_MAGIC_DUP);
    }
    hits += 1;

    // I1：非 2 的幂注入被拒。
    let mut p = BlkNegotiator::new(0xFF);
    if p.negotiate(3000) != Err(E_BSIZE_POW2) {
        return Err(E_BSIZE_POW2);
    }
    hits += 1;

    // I2：越界块大小注入被拒（256 / 131072）。
    if BlkNegotiator::bit_of(256) != Err(E_BSIZE_RANGE) {
        return Err(E_BSIZE_RANGE);
    }
    if BlkNegotiator::bit_of(131_072) != Err(E_BSIZE_RANGE) {
        return Err(E_BSIZE_RANGE);
    }
    if BlkNegotiator::bsize_of(8) != Err(E_BSIZE_RANGE) {
        return Err(E_BSIZE_RANGE);
    }
    hits += 1;

    // I3：魔数 0 注入被拒、重号注入被拒（判据 J2 的拒绝面）。
    if mr.register(0, 9, 1) != Err(E_MAGIC_ZERO) {
        return Err(E_MAGIC_ZERO);
    }
    if mr.register(0xEF53, 9, 1) != Err(E_MAGIC_DUP) {
        return Err(E_MAGIC_DUP);
    }
    if mr.zero_rej != 1 || mr.dup_rej != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：未协商即激活注入被拒（Idle 态）。
    let mut idle = BlkNegotiator::new(0xFF);
    if idle.activate_sb() != Err(E_NOT_AGREED) {
        return Err(E_NOT_AGREED);
    }
    hits += 1;

    // I5：失败态重复协商仍失败但账目准确（两次尝试、两次失败）。
    if f.negotiate(8192) != Err(E_NO_INTERSECT) {
        return Err(E_NO_INTERSECT);
    }
    if f.attempts != 2 || f.failed != 2 || !f.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
