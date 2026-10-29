//! VE-F4363 · U04-U06 组自查核验（VE-STAR-II 收官自检组）
//! 职责：序列化往返与解码边界自检引擎——把「编码 ∘ 解码 ≡ 恒等」
//! 与「坏面必显性拒」两条判据做成可复用的确定性夹具引擎：
//! 定长帧编解码、种类表完备性、万帧 LCG 往返回归、
//! 载荷漂移检出与帧指纹投影。
//! 核心性质：① 往返恒等（encode ∘ decode ≡ id，对全种类全域成立）；
//! ② 边界显性（错长 / 坏种类 / 坏校验一律 Err 不静默钳制）；
//! ③ 种类表完备（1..=K 每个种类要么可解要么显性拒，无第三态）；
//! ④ 帧指纹确定且对面敏感（改一字节即变）。
//! 段位：0x4860（续表 idx597）。行数预算 340。

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4860;

/// 本单错误闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtCode {
    /// 帧长非法（≠ FRAME_LEN）。
    FrameLen = SEG_BASE,
    /// 种类号越界（0 或 > KIND_MAX）。
    KindRange = SEG_BASE + 0x01,
    /// 载荷校验和破。
    PayloadBad = SEG_BASE + 0x02,
    /// 帧封存位被置但表未封。
    SealDrift = SEG_BASE + 0x03,
}

/// 定长帧长（种类 1 + 载荷 6 + 校验 1）。
pub const FRAME_LEN: usize = 8;
/// 种类闭集 1..=6。
pub const KIND_MAX: u8 = 6;

/// 一条逻辑记录（编解码的域模型面）。
#[derive(Clone, Copy, Debug)]
pub struct Record {
    /// 种类号（1..=6）。
    pub kind: u8,
    /// 载荷（6 字节语义域）。
    pub payload: [u8; 6],
}

/// 帧校验：种类 ⊕ 全载荷（异或面，检出单位翻转）。
fn frame_sum(kind: u8, payload: &[u8; 6]) -> u8 {
    let mut s = kind;
    for &b in payload {
        s ^= b;
    }
    s
}

/// 编码：8 字节定长帧 = [kind, p0..p5, sum]。
pub fn encode(r: &Record) -> [u8; FRAME_LEN] {
    let mut f = [0u8; FRAME_LEN];
    f[0] = r.kind;
    f[1..7].copy_from_slice(&r.payload);
    f[7] = frame_sum(r.kind, &r.payload);
    f
}

/// 解码：错长 / 坏种类 / 校验和破均显性 Err。
pub fn decode(b: &[u8]) -> Result<Record, RtCode> {
    if b.len() != FRAME_LEN {
        return Err(RtCode::FrameLen);
    }
    if b[0] == 0 || b[0] > KIND_MAX {
        return Err(RtCode::KindRange);
    }
    let mut payload = [0u8; 6];
    payload.copy_from_slice(&b[1..7]);
    if frame_sum(b[0], &payload) != b[7] {
        return Err(RtCode::PayloadBad);
    }
    Ok(Record { kind: b[0], payload })
}

/// 帧指纹：FNV-1a 64（对面敏感）。
pub fn frame_fp(f: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in f {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 种类表完备性：对每个种类号 1..=K 造一帧，
/// 要么解码成功且种类回读一致，要么显性 Err——无第三态。
pub fn kind_table_complete() -> bool {
    for k in 1..=KIND_MAX {
        let r = Record { kind: k, payload: [k; 6] };
        match decode(&encode(&r).as_slice()) {
            Ok(back) => {
                if back.kind != k {
                    return false;
                }
            }
            Err(e) => return false,
        }
    }
    true
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 本单判据入口（域折叠表注册项）。
pub fn run_ves597_checks() -> CheckSet {
    let mut set = CheckSet::new("ves597");

    // C597-01 往返恒等：六种类全遍历 encode∘decode≡id（含载荷全 0/全 FF）。
    let mut all_id = true;
    for k in 1..=KIND_MAX {
        for pat in [[0u8; 6], [0xFF; 6], [1, 2, 3, 4, 5, 6]] {
            let r = Record { kind: k, payload: pat };
            if decode(&encode(&r)) != Ok(r) {
                all_id = false;
            }
        }
    }
    chk(&mut set, "C597-01", all_id, "六种类三载荷全往返恒等");

    // C597-02 错长显性拒：7 字节与 9 字节均 FrameLen。
    let good = encode(&Record { kind: 1, payload: [7; 6] });
    let nine = { let mut v = good.to_vec(); v.push(0xAA); v };
    chk(&mut set, "C597-02", decode(&good[..7]) == Err(RtCode::FrameLen)
        && decode(&nine) == Err(RtCode::FrameLen),
        "错长显性拒");

    // C597-03 坏种类显性拒：种类 0 与 7。
    let mut k0 = good;
    k0[0] = 0;
    k0[7] = frame_sum(0, &[7; 6]);
    let mut k7 = good;
    k7[0] = 7;
    k7[7] = frame_sum(7, &[7; 6]);
    chk(&mut set, "C597-03", decode(&k0) == Err(RtCode::KindRange)
        && decode(&k7) == Err(RtCode::KindRange), "种类越界显性拒");

    // C597-04 校验和破显性拒：载荷单字节翻转（保留种类）。
    let mut bad = good;
    bad[3] ^= 0x10;
    chk(&mut set, "C597-04", decode(&bad) == Err(RtCode::PayloadBad), "校验破显性拒");

    // C597-05 种类表完备：1..=6 每格可解或显性拒，无第三态。
    chk(&mut set, "C597-05", kind_table_complete(), "种类表完备无第三态");

    // C597-06 帧指纹敏感：任一字节翻转指纹必变（全位面扫描 8 字节 × 8 位）。
    let base_fp = frame_fp(&good);
    let mut all_sensitive = true;
    for byte_i in 0..FRAME_LEN {
        for bit_i in 0..8u8 {
            let mut f = good;
            f[byte_i] ^= 1 << bit_i;
            if frame_fp(&f) == base_fp {
                all_sensitive = false;
            }
        }
    }
    chk(&mut set, "C597-06", all_sensitive, "64 位面逐位翻转指纹必变");

    // C597-07 万帧 LCG 往返回归：8192 帧 encode∘decode≡id 且指纹可复算。
    let mut rng = Lcg::new(0x5970_0001);
    let mut ok_round = 0usize;
    let mut fp_ok = 0usize;
    for _ in 0..8192u32 {
        rng.next();
        let kind = 1 + (rng.next() % KIND_MAX as u64) as u8;
        let mut payload = [0u8; 6];
        for p in payload.iter_mut() {
            *p = (rng.next() & 0xFF) as u8;
        }
        let r = Record { kind, payload };
        let f = encode(&r);
        if decode(&f) == Ok(r) {
            ok_round += 1;
        }
        if frame_fp(&f) == frame_fp(&encode(&decode(&f).unwrap_or(r))) {
            fp_ok += 1;
        }
    }
    chk(&mut set, "C597-07", ok_round == 8192 && fp_ok == 8192, "万帧往返与指纹复算全过");

    // C597-08 万帧边界回归：坏帧（随机位翻转）必显性 Err 或仍恒等。
    let mut rng2 = Lcg::new(0x5970_0002);
    let mut no_third = true;
    for _ in 0..4096u32 {
        rng2.next();
        let mut f = encode(&Record { kind: 1 + (rng2.next() % 6) as u8, payload: [9; 6] });
        let bit = (rng2.next() % 64) as usize;
        f[bit / 8] ^= 1 << (bit % 8);
        match decode(&f) {
            Ok(back) => {
                // 校验位翻转后仍可解 ⇒ 恒等必须保持（种类与载荷一致回读）。
                if encode(&back) != f {
                    no_third = false;
                }
            }
            Err(_) => {}
        }
    }
    chk(&mut set, "C597-08", no_third, "坏帧两态收口无第三态");

    // C597-09 载荷漂移检出：同种类载荷差一字节 → 帧必异且指纹必异。
    let ra = Record { kind: 3, payload: [1, 2, 3, 4, 5, 6] };
    let mut rb = ra;
    rb.payload[5] = 7;
    chk(&mut set, "C597-09", encode(&ra) != encode(&rb)
        && frame_fp(&encode(&ra)) != frame_fp(&encode(&rb)), "载荷漂移帧与指纹必异");

    // C597-10 校验面覆盖：① 帧内任意单比特翻转必显性 Err（异或面全覆盖）；
    // ② 载荷位与校验位成对翻转（自洽坏帧）必可解且恒等保持。
    let mut cover_ok = true;
    let f0 = encode(&Record { kind: 2, payload: [5; 6] });
    for byte_i in 0..FRAME_LEN {
        for bit_i in 0..8u8 {
            let mut f = f0;
            f[byte_i] ^= 1 << bit_i;
            if decode(&f).is_ok() {
                cover_ok = false;
            }
        }
    }
    let mut pair_ok = true;
    for byte_i in 1..7usize {
        for bit_i in 0..8u8 {
            let mut f = f0;
            f[byte_i] ^= 1 << bit_i;               // 载荷位翻转
            f[7] ^= 1 << bit_i;                    // 校验位同步翻转（面自洽）
            match decode(&f) {
                Ok(back) => {
                    if encode(&back) != f {
                        pair_ok = false;
                    }
                }
                Err(_) => pair_ok = false,
            }
        }
    }
    chk(&mut set, "C597-10", cover_ok && pair_ok, "单翻必拒成对翻恒等");

    set
}

// ─── 自查锚点 ───
// 覆盖：往返恒等 / 错长-坏种类-校验破三类边界 / 种类表完备 /
//       64 位面指纹敏感 / 万帧往返回归 / 坏帧两态收口 / 位翻转定位。
// 红项定位：CheckSet::red_items_text()（域折叠失败路径统一走它）。
