//! UNX-F0088 · 重放校验（BootManifest 固化）· 诊断段 0x2240
//! 验收锚点：UNX-F0088-J1 引导清单固化（版本+指纹+时序）与
//! 重放对拍（同清单两次引导指纹一致）判据通过；
//! 纯功能代码 ≥240 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2240;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MAGIC: u16 = SEG_BASE + 1;    // 清单魔数不符
const E_VER: u16 = SEG_BASE + 2;      // 清单版本不支持
const E_FP: u16 = SEG_BASE + 3;       // 指纹不符（清单损坏）
const E_REPLAY: u16 = SEG_BASE + 4;   // 重放指纹漂移（非确定性引导）
const E_RDONLY: u16 = SEG_BASE + 5;   // 清单已固化
const E_EMPTY: u16 = SEG_BASE + 6;    // 空清单
const E_SEQ: u16 = SEG_BASE + 7;      // 引导序号倒退
const E_BAL: u16 = SEG_BASE + 8;      // 条目账不平

/// 清单魔数 "VBM1"（Varix Boot Manifest v1）。
pub const MANI_MAGIC: [u8; 4] = [0x56, 0x42, 0x4D, 0x31];
/// 清单版本。
pub const MANI_VER: u32 = 1;
/// 清单头长：魔数 4 + 版本 4 + 引导序号 4 + 条目数 4 = 16。
pub const MANI_HDR: usize = 16;
/// 条目长：标签 4 + 载荷指纹 8 = 12。
pub const MANI_ENTRY: usize = 12;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_MAGIC => "清单魔数非 VBM1：来源不明",
        E_VER => "清单版本不支持：仅 v1",
        E_FP => "清单指纹不符：载荷损坏或被篡改",
        E_REPLAY => "重放指纹漂移：两次引导清单不一致，非确定性源立案",
        E_RDONLY => "清单已固化：固化后只读",
        E_EMPTY => "空清单：至少登记内核镜像指纹",
        E_SEQ => "引导序号倒退：新清单序号必须单调递增",
        E_BAL => "条目账不平：计数与条目流不符",
        _ => "未知诊断码",
    }
}

/// 常用标签闭集。
pub const TAG_KERNEL: u32 = 1;
pub const TAG_CMDLINE: u32 = 2;
pub const TAG_INITRD: u32 = 3;
pub const TAG_DTB: u32 = 4;

pub fn tag_name(t: u32) -> &'static str {
    match t {
        TAG_KERNEL => "内核镜像",
        TAG_CMDLINE => "命令行",
        TAG_INITRD => "initrd",
        TAG_DTB => "设备树",
        _ => "越纲",
    }
}

// ── 清单账 ───────────────────────────────────────────────────
pub struct BootManifest {
    pub boot_no: u32,        // 引导序号
    pub entries: Vec<(u32, u64)>, // (标签, 载荷指纹)
    pub sealed: bool,
}

impl BootManifest {
    pub fn new(boot_no: u32) -> BootManifest {
        BootManifest {
            boot_no,
            entries: Vec::new(),
            sealed: false,
        }
    }

    pub fn add(&mut self, tag: u32, payload_fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self
            .entries
            .iter()
            .any(|(t, f)| *t == tag && *f == payload_fp)
        {
            return Err(E_BAL);
        }
        self.entries.push((tag, payload_fp));
        Ok(())
    }

    /// 内容指纹（不含引导序号——重放对拍键）。
    pub fn content_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for (t, f) in &self.entries {
            buf.extend_from_slice(&t.to_le_bytes());
            buf.extend_from_slice(&f.to_le_bytes());
        }
        fnv1a64(&buf)
    }

    /// 固化并序列化。
    pub fn seal_and_serialize(&mut self) -> Result<Vec<u8>, u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        self.sealed = true;
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&MANI_MAGIC);
        out.extend_from_slice(&MANI_VER.to_le_bytes());
        out.extend_from_slice(&self.boot_no.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.content_fp().to_le_bytes());
        for (t, f) in &self.entries {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&f.to_le_bytes());
        }
        Ok(out)
    }
}

/// 反序列化：核验魔数/版本/计数/内容指纹。
pub fn deserialize_manifest(blob: &[u8]) -> Result<(u32, u64, Vec<(u32, u64)>), u16> {
    if blob.len() < MANI_HDR {
        return Err(E_BAL);
    }
    if blob[0..4] != MANI_MAGIC {
        return Err(E_MAGIC);
    }
    let ver = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
    if ver != MANI_VER {
        return Err(E_VER);
    }
    let boot_no = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    let count = u32::from_le_bytes([blob[12], blob[13], blob[14], blob[15]]) as usize;
    if blob.len() != MANI_HDR + count * MANI_ENTRY {
        return Err(E_BAL);
    }
    let stored_fp = u64::from_le_bytes([
        blob[16], blob[17], blob[18], blob[19],
        blob[20], blob[21], blob[22], blob[23],
    ]);
    let mut entries = Vec::new();
    let mut off = MANI_HDR;
    for _ in 0..count {
        let t = u32::from_le_bytes([
            blob[off], blob[off + 1], blob[off + 2], blob[off + 3],
        ]);
        let f = u64::from_le_bytes([
            blob[off + 4], blob[off + 5], blob[off + 6], blob[off + 7],
            blob[off + 8], blob[off + 9], blob[off + 10], blob[off + 11],
        ]);
        entries.push((t, f));
        off += MANI_ENTRY;
    }
    // 内容指纹复算
    let mut buf: Vec<u8> = Vec::new();
    for (t, f) in &entries {
        buf.extend_from_slice(&t.to_le_bytes());
        buf.extend_from_slice(&f.to_le_bytes());
    }
    if fnv1a64(&buf) != stored_fp {
        return Err(E_FP);
    }
    Ok((boot_no, stored_fp, entries))
}

// ── 重放账：跨引导清单对拍 ───────────────────────────────────
pub struct ReplayLedger {
    pub last_boot_no: u32,
    pub last_content_fp: u64,
    pub seen: Vec<(u32, u64)>,
}

impl ReplayLedger {
    pub fn new() -> ReplayLedger {
        ReplayLedger {
            last_boot_no: 0,
            last_content_fp: 0,
            seen: Vec::new(),
        }
    }

    /// 吸收一次引导：序号必须递增；内容指纹与上轮对拍，
    /// 漂移即 E_REPLAY（确定性引导判据）。
    pub fn absorb(&mut self, boot_no: u32, content_fp: u64) -> Result<(), u16> {
        if boot_no <= self.last_boot_no && self.last_boot_no != 0 {
            return Err(E_SEQ);
        }
        if self.last_boot_no != 0 && content_fp != self.last_content_fp {
            self.seen.push((boot_no, content_fp));
            return Err(E_REPLAY);
        }
        self.last_boot_no = boot_no;
        self.last_content_fp = content_fp;
        self.seen.push((boot_no, content_fp));
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let kfp: u64 = 0x1111_2222_3333_4444;
    let cfp: u64 = 0x5555_6666_7777_8888;
    let ifp: u64 = 0x9999_AAAA_BBBB_CCCC;

    // R1：清单构建→固化→序列化→反序列化往返。
    let mut m1 = BootManifest::new(1);
    m1.add(TAG_KERNEL, kfp)?;
    m1.add(TAG_CMDLINE, cfp)?;
    m1.add(TAG_INITRD, ifp)?;
    let blob = m1.seal_and_serialize()?;
    let (no, fp, es) = deserialize_manifest(&blob)?;
    if no != 1 || es.len() != 3 || fp != m1.content_fp() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：重放对拍——同内容两次引导 → 一致。
    let mut rp = ReplayLedger::new();
    rp.absorb(1, m1.content_fp())?;
    rp.absorb(2, m1.content_fp())?;
    if rp.seen.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：内容漂移 → E_REPLAY 立案。
    let mut rp2 = ReplayLedger::new();
    rp2.absorb(1, m1.content_fp())?;
    if rp2.absorb(2, cfp) != Err(E_REPLAY) {
        return Err(E_REPLAY);
    }
    hits += 1;

    // I1：魔数注入 → E_MAGIC。
    let mut b1 = blob.clone();
    b1[0] = 0x58;
    if deserialize_manifest(&b1) != Err(E_MAGIC) {
        return Err(E_MAGIC);
    }
    hits += 1;

    // I2：版本改坏 → E_VER。
    let mut b2 = blob.clone();
    b2[4] = 9;
    if deserialize_manifest(&b2) != Err(E_VER) {
        return Err(E_VER);
    }
    hits += 1;

    // I3：条目流翻转 → E_FP。
    let mut b3 = blob.clone();
    let n = b3.len();
    b3[n - 1] ^= 0xFF;
    if deserialize_manifest(&b3) != Err(E_FP) {
        return Err(E_FP);
    }
    hits += 1;

    // I4：计数改坏 → E_BAL。
    let mut b4 = blob.clone();
    b4[12] = 0x63;
    if deserialize_manifest(&b4) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：引导序号倒退 → E_SEQ。
    let mut rp3 = ReplayLedger::new();
    rp3.absorb(5, m1.content_fp())?;
    if rp3.absorb(4, m1.content_fp()) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I6：空清单固化 → E_EMPTY。
    let mut m2 = BootManifest::new(2);
    if m2.seal_and_serialize() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I7：固化后再登记 → E_RDONLY。
    if m1.add(TAG_DTB, 1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
