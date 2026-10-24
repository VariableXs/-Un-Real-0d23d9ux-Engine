//! UNX-F0070 · EFI MemoryMap 持久化副本 · 诊断段 0x2150
//! 验收锚点：UNX-F0070-J1 MemoryMap 双副本（A/B）写入与版本代际
//! 对拍，任一副本损坏由另一副本恢复，副本一致性判据通过；
//! 纯功能代码 ≥220 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2150;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MAGIC: u16 = SEG_BASE + 1;    // 副本魔数不符
const E_VER: u16 = SEG_BASE + 2;      // 副本版本不支持
const E_FP: u16 = SEG_BASE + 3;       // 副本指纹不符
const E_GEN: u16 = SEG_BASE + 4;      // 代际倒退（旧图覆写新图）
const E_BOTH: u16 = SEG_BASE + 5;     // 双副本同帧损坏（灾难态）
const E_ENTRY: u16 = SEG_BASE + 6;    // 描述符流截断
const E_DUP: u16 = SEG_BASE + 7;      // 同代际重复写入
const E_BAL: u16 = SEG_BASE + 8;      // 副本账对账不平
const E_STALE: u16 = SEG_BASE + 9;    // 读取时未选定有效副本

/// 副本魔数 "VMM1"（Varix MemoryMap Mirror v1）。
pub const MIRROR_MAGIC: [u8; 4] = [0x56, 0x4D, 0x4D, 0x31];
/// 副本版本。
pub const MIRROR_VER: u32 = 1;
/// 描述符定长：基址 8 + 页数 8 + 类型 4 + 属性 8 = 28。
pub const DESC_LEN: usize = 28;

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
        E_MAGIC => "副本魔数非 VMM1：镜像槽损坏",
        E_VER => "副本版本不支持：仅 v1",
        E_FP => "副本指纹不符：载荷损坏，弃用本副本",
        E_GEN => "代际倒退：旧图覆写新图，拦截",
        E_BOTH => "双副本同帧损坏：灾难态，必须重取固件图",
        E_ENTRY => "描述符流截断：计数 × 28 ≠ 载荷长",
        E_DUP => "同代际重复写入：幂等写路径被误用",
        E_BAL => "副本账对账不平：有效副本数 ∈ {0,1,2}",
        E_STALE => "未选定有效副本就读：先 pick_valid",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorDesc {
    pub phys: u64,
    pub pages: u64,
    pub ty: u32,
    pub attrs: u64,
}

/// 序列化一代图：头（魔数 4 + 版本 4 + 代际 8 + 计数 4 + 指纹 8 = 28）
/// + 描述符流。
pub fn serialize_map(gen: u64, descs: &[MirrorDesc]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for d in descs {
        body.extend_from_slice(&d.phys.to_le_bytes());
        body.extend_from_slice(&d.pages.to_le_bytes());
        body.extend_from_slice(&d.ty.to_le_bytes());
        body.extend_from_slice(&d.attrs.to_le_bytes());
    }
    let fp = fnv1a64(&body);
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&MIRROR_MAGIC);
    out.extend_from_slice(&MIRROR_VER.to_le_bytes());
    out.extend_from_slice(&gen.to_le_bytes());
    out.extend_from_slice(&(descs.len() as u32).to_le_bytes());
    out.extend_from_slice(&fp.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// 反序列化并核验一代图。
pub fn deserialize_map(blob: &[u8]) -> Result<(u64, Vec<MirrorDesc>), u16> {
    if blob.len() < 28 {
        return Err(E_ENTRY);
    }
    if blob[0..4] != MIRROR_MAGIC {
        return Err(E_MAGIC);
    }
    let ver = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
    if ver != MIRROR_VER {
        return Err(E_VER);
    }
    let gen = u64::from_le_bytes([
        blob[8], blob[9], blob[10], blob[11],
        blob[12], blob[13], blob[14], blob[15],
    ]);
    let count = u32::from_le_bytes([blob[16], blob[17], blob[18], blob[19]]) as usize;
    let fp = u64::from_le_bytes([
        blob[20], blob[21], blob[22], blob[23],
        blob[24], blob[25], blob[26], blob[27],
    ]);
    let body = &blob[28..];
    if body.len() != count * DESC_LEN {
        return Err(E_ENTRY);
    }
    if fnv1a64(body) != fp {
        return Err(E_FP);
    }
    let mut descs = Vec::new();
    let mut off = 0usize;
    for _ in 0..count {
        let b = &body[off..off + DESC_LEN];
        descs.push(MirrorDesc {
            phys: u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]),
            pages: u64::from_le_bytes([
                b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
            ]),
            ty: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
            attrs: u64::from_le_bytes([
                b[20], b[21], b[22], b[23], b[24], b[25], b[26], b[27],
            ]),
        });
        off += DESC_LEN;
    }
    Ok((gen, descs))
}

// ── 双副本槽 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    Empty,
    Ok { gen: u64 },
    Corrupt,
}

pub struct MirrorPair {
    slots: [Vec<u8>; 2],
    states: [SlotState; 2],
    latest_gen: u64,
    pub picked: Option<usize>,
}

impl MirrorPair {
    pub fn new() -> MirrorPair {
        MirrorPair {
            slots: [Vec::new(), Vec::new()],
            states: [SlotState::Empty, SlotState::Empty],
            latest_gen: 0,
            picked: None,
        }
    }

    /// 写入一代图到最旧/空槽（代际倒退与同代重写拦截）。
    pub fn write(&mut self, gen: u64, descs: &[MirrorDesc]) -> Result<usize, u16> {
        if gen < self.latest_gen {
            return Err(E_GEN);
        }
        if gen == self.latest_gen && self.latest_gen != 0 {
            return Err(E_DUP);
        }
        let blob = serialize_map(gen, descs);
        // 选槽：Empty/Corrupt 优先；两槽皆 Ok 时写代际较旧者
        let mut target = 0usize;
        let mut chose = false;
        for i in 0..2 {
            match self.states[i] {
                SlotState::Empty | SlotState::Corrupt => {
                    target = i;
                    chose = true;
                    break;
                }
                SlotState::Ok { .. } => {}
            }
        }
        if !chose {
            if let (SlotState::Ok { gen: g0 }, SlotState::Ok { gen: g1 }) =
                (self.states[0], self.states[1])
            {
                target = if g0 <= g1 { 0 } else { 1 };
            }
        }
        self.slots[target] = blob;
        self.states[target] = SlotState::Ok { gen };
        self.latest_gen = gen;
        self.picked = None;
        Ok(target)
    }

    /// 巡检：校验两槽，损坏槽降级 Corrupt。
    pub fn scan(&mut self) -> Result<usize, u16> {
        let mut valid = 0usize;
        for i in 0..2 {
            let r = deserialize_map(&self.slots[i]);
            match r {
                Ok((gen, _)) => {
                    self.states[i] = SlotState::Ok { gen };
                    valid += 1;
                }
                Err(_) => {
                    if !self.slots[i].is_empty() {
                        self.states[i] = SlotState::Corrupt;
                    } else {
                        self.states[i] = SlotState::Empty;
                    }
                }
            }
        }
        if valid == 0
            && (self.states[0] == SlotState::Corrupt
                && self.states[1] == SlotState::Corrupt)
        {
            return Err(E_BOTH);
        }
        Ok(valid)
    }

    /// 选定有效副本：代际较大者胜出。
    pub fn pick_valid(&mut self) -> Result<(u64, Vec<MirrorDesc>), u16> {
        self.scan()?;
        let mut best: Option<(u64, usize)> = None;
        for i in 0..2 {
            if let SlotState::Ok { gen } = self.states[i] {
                match best {
                    Some((g, _)) if g >= gen => {}
                    _ => best = Some((gen, i)),
                }
            }
        }
        match best {
            Some((gen, i)) => {
                let (g, d) = deserialize_map(&self.slots[i])?;
                if g != gen {
                    return Err(E_BAL);
                }
                self.picked = Some(i);
                Ok((g, d))
            }
            None => Err(E_STALE),
        }
    }

    pub fn slot_state_name(&self, i: usize) -> &'static str {
        match self.states[i] {
            SlotState::Empty => "空",
            SlotState::Ok { .. } => "有效",
            SlotState::Corrupt => "损坏",
        }
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
fn sample_descs() -> Vec<MirrorDesc> {
    vec![
        MirrorDesc { phys: 0x1000, pages: 0x100, ty: 7, attrs: 0xF },
        MirrorDesc { phys: 0x2000_0000, pages: 0x400, ty: 6, attrs: 0x9 },
    ]
}

pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双代写入 → pick_valid 取新代。
    let mut mp = MirrorPair::new();
    mp.write(1, &sample_descs())?;
    let mut d2 = sample_descs();
    d2.push(MirrorDesc { phys: 0x3000_0000, pages: 0x80, ty: 4, attrs: 0 });
    mp.write(2, &d2)?;
    let (gen, descs) = mp.pick_valid()?;
    if gen != 2 || descs.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：单副本损坏 → 另一副本恢复。
    let mut mp2 = MirrorPair::new();
    mp2.write(1, &sample_descs())?;
    mp2.write(2, &d2)?;
    let n = mp2.slots[1].len();
    mp2.slots[1][n - 1] ^= 0xFF; // 损坏新代副本
    let (gen2, _) = mp2.pick_valid()?;
    if gen2 != 1 {
        return Err(E_BAL);
    }
    if mp2.slot_state_name(1) != "损坏" {
        return Err(E_FP);
    }
    hits += 1;

    // R3：序列化/反序列化往返。
    let blob = serialize_map(9, &sample_descs());
    let (g, d) = deserialize_map(&blob)?;
    if g != 9 || d.len() != 2 || d[0].phys != 0x1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：魔数注入 → E_MAGIC。
    let mut b = serialize_map(1, &sample_descs());
    b[0] = 0x58;
    if deserialize_map(&b) != Err(E_MAGIC) {
        return Err(E_MAGIC);
    }
    hits += 1;

    // I2：指纹破坏 → E_FP。
    let mut b2 = serialize_map(1, &sample_descs());
    let n2 = b2.len();
    b2[n2 - 1] ^= 0xFF;
    if deserialize_map(&b2) != Err(E_FP) {
        return Err(E_FP);
    }
    hits += 1;

    // I3：代际倒退 → E_GEN。
    let mut mp3 = MirrorPair::new();
    mp3.write(5, &sample_descs())?;
    if mp3.write(4, &sample_descs()) != Err(E_GEN) {
        return Err(E_GEN);
    }
    hits += 1;

    // I4：同代重写 → E_DUP。
    if mp3.write(5, &sample_descs()) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I5：双槽全坏 → E_BOTH。
    let mut mp4 = MirrorPair::new();
    mp4.write(1, &sample_descs())?;
    mp4.write(2, &sample_descs())?;
    mp4.slots[0].clear();
    mp4.slots[0].extend_from_slice(&[0x58u8; 40]);
    mp4.slots[1].clear();
    mp4.slots[1].extend_from_slice(&[0x58u8; 40]);
    if mp4.pick_valid() != Err(E_BOTH) {
        return Err(E_BOTH);
    }
    hits += 1;

    // I6：空对取读 → E_STALE。
    let mut mp5 = MirrorPair::new();
    if mp5.pick_valid() != Err(E_STALE) {
        return Err(E_STALE);
    }
    hits += 1;

    Ok(hits)
}
