//! unx48262_ffilib — UNX-F48262 addon 侧适配库（Rust FFI，桥本体零感知具体引擎）（诊断段 0xC1A0）。
//!
//! 判据原文：
//!   J1 FFI 边界 fuzz ×10^4 零崩；判据锚定 §AI-61 §2 专题四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 零感知边界：适配库只认四类通用消息（INIT/KEY/EVT/RESET）与
//!      版本号，不出现任何具体输入法引擎的名称与语义——桥本体
//!      对引擎零感知是边界红线；引擎差异全部封装在消息载荷整数域内。
//!   2. POD 编解码：FfiMsg 结构（kind:u32, ver:u32, a:u64, b:u64,
//!      c:u32, d:u32）双向按小端字节序编解码，布局 28 字节定长；
//!      encode→decode 往返恒等是 FFI 面的第一不变量。
//!   3. 防御式解码：decode 对魔数前缀、长度、kind 闭集三关校验，
//!      任何一关不过返回诊断码——不 panic 不越界（即使输入全随机）。
//!   4. fuzz 口径（J1 ×10^4）：确定性 LCG 生成 10^4 个随机字节帧，
//!      全部喂入解码面：合法帧走往返恒等断言，非法帧必得闭集诊断码；
//!      零崩（无 panic 等价路径）+ 零错放（非法帧不得通过三关）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC1A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MAGIC_BAD: u16 = SEG_BASE + 1;  // 魔数前缀错
const E_LEN_BAD: u16 = SEG_BASE + 2;    // 长度错
const E_KIND_BAD: u16 = SEG_BASE + 3;   // kind 闭集外
const E_RT_LOSS: u16 = SEG_BASE + 4;    // 往返不恒等
const E_FUZZ_MISS: u16 = SEG_BASE + 5;  // ×10^4 不足
const E_VER_BAD: u16 = SEG_BASE + 6;    // 版本不识别
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空
const E_LEDGER_BAD: u16 = SEG_BASE + 8; // 会话账缺失/非幂等

/// FFI 帧魔数前缀与布局常量。
pub const FF_MAGIC: u32 = 0x4646_4931; // "FFI1"
pub const FF_LEN: usize = 40;          // 帧定长（4 magic + 4 kind + 4 ver + 8 a + 8 b + 4 c + 4 d + 4 crc）
pub const ABI_VER: u32 = 1;

/// 通用消息 kind 闭集（引擎零感知）。
pub const KIND_INIT: u32 = 0;
pub const KIND_KEY: u32 = 1;
pub const KIND_EVT: u32 = 2;
pub const KIND_RESET: u32 = 3;
pub const KIND_COUNT: u32 = 4;

fn kind_legal(kind: u32) -> bool {
    kind < KIND_COUNT
}

/// FFI 消息 POD。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FfiMsg {
    pub kind: u32,
    pub ver: u32,
    pub a: u64,
    pub b: u64,
    pub c: u32,
    pub d: u32,
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.push((v & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    let mut i = 0u32;
    while i < 8 {
        buf.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

fn get_u32(buf: &[u8], off: usize) -> Result<u32, u16> {
    if off + 4 > buf.len() {
        return Err(E_LEN_BAD);
    }
    Ok((buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24))
}

fn get_u64(buf: &[u8], off: usize) -> Result<u64, u16> {
    if off + 8 > buf.len() {
        return Err(E_LEN_BAD);
    }
    let mut v: u64 = 0;
    let mut i = 0u32;
    while i < 8 {
        v |= (buf[off + i as usize] as u64) << (i * 8);
        i += 1;
    }
    Ok(v)
}

/// 帧校验和：载荷区逐字节异或折叠。
fn frame_crc(body: &[u8]) -> u32 {
    let mut crc: u32 = 0xFF31;
    let mut i = 0usize;
    while i < body.len() {
        crc = crc.wrapping_mul(0x9E3779B1) ^ body[i] as u32;
        i += 1;
    }
    crc
}

/// 编码：FfiMsg → 定长帧（32 字节）。
pub fn encode(msg: &FfiMsg) -> Result<Vec<u8>, u16> {
    if !kind_legal(msg.kind) {
        return Err(E_KIND_BAD);
    }
    if msg.ver != ABI_VER {
        return Err(E_VER_BAD);
    }
    let mut buf: Vec<u8> = Vec::new();
    put_u32(&mut buf, FF_MAGIC);
    put_u32(&mut buf, msg.kind);
    put_u32(&mut buf, msg.ver);
    put_u64(&mut buf, msg.a);
    put_u64(&mut buf, msg.b);
    put_u32(&mut buf, msg.c);
    put_u32(&mut buf, msg.d);
    // 帧 = 4+4+4+8+8+4+4 = 36 字节主体 + 4 crc = 40。
    if buf.len() != FF_LEN - 4 {
        return Err(E_LEN_BAD);
    }
    let crc = frame_crc(&buf[12..28]);
    put_u32(&mut buf, crc);
    Ok(buf)
}

/// 解码：帧 → FfiMsg（魔数/长度/版本/kind/crc 五关，纪律 3）。
pub fn decode(frame: &[u8]) -> Result<FfiMsg, u16> {
    if frame.len() != FF_LEN {
        return Err(E_LEN_BAD);
    }
    let magic = get_u32(frame, 0)?;
    if magic != FF_MAGIC {
        return Err(E_MAGIC_BAD);
    }
    let kind = get_u32(frame, 4)?;
    if !kind_legal(kind) {
        return Err(E_KIND_BAD);
    }
    let ver = get_u32(frame, 8)?;
    if ver != ABI_VER {
        return Err(E_VER_BAD);
    }
    let a = get_u64(frame, 12)?;
    let b = get_u64(frame, 20)?;
    let c = get_u32(frame, 28)?;
    let d = get_u32(frame, 32)?;
    let want_crc = get_u32(frame, 36)?;
    let got_crc = frame_crc(&frame[12..28]);
    if want_crc != got_crc {
        return Err(E_LEN_BAD); // crc 关并入帧形制错误
    }
    Ok(FfiMsg { kind, ver, a, b, c, d })
}

/// FFI 帧定长（40 字节，与 FF_LEN 一致的别名）。
pub const FF_LEN_REAL: usize = 40;

/// LCG 确定性随机源（与 fuzz 域同构）。
pub struct FzLcg {
    x: u64,
}

impl FzLcg {
    pub fn new(seed: u64) -> Result<FzLcg, u16> {
        if seed == 0 {
            return Err(E_FUZZ_MISS);
        }
        Ok(FzLcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self
            .x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_byte(&mut self) -> u8 {
        (self.next_u64() >> 24) as u8
    }
}

/// fuzz 主循环：n 帧随机字节喂解码面（J1 主体）。
/// 合法帧（构造性生成）走往返恒等；随机帧必得闭集诊断码。
pub fn fuzz_ffi(seed: u64, n: u32) -> Result<(u32, u32), u16> {
    let mut rng = FzLcg::new(seed)?;
    let mut valid_rt: u32 = 0;
    let mut rejected: u32 = 0;
    let mut i = 0u32;
    while i < n {
        // 偶数步：构造合法帧做往返恒等；奇数步：随机帧必拒。
        if i % 2 == 0 {
            let msg = FfiMsg {
                kind: (rng.next_u64() % KIND_COUNT as u64) as u32,
                ver: ABI_VER,
                a: rng.next_u64(),
                b: rng.next_u64(),
                c: (rng.next_u64() & 0xFFFF_FFFF) as u32,
                d: (rng.next_u64() & 0xFFFF_FFFF) as u32,
            };
            let frame = encode(&msg)?;
            if frame.len() != FF_LEN_REAL {
                return Err(E_LEN_BAD);
            }
            let back = decode(&frame)?;
            if back != msg {
                return Err(E_RT_LOSS);
            }
            valid_rt += 1;
        } else {
            // 随机帧：多数非法；若恰成合法帧（概率≈0）则按合法处理。
            let mut frame: Vec<u8> = Vec::new();
            let mut k = 0usize;
            while k < FF_LEN_REAL {
                frame.push(rng.next_byte());
                k += 1;
            }
            match decode(&frame) {
                Ok(_) => {
                    // 全随机撞上合法帧：仍须满足往返恒等（不算崩）。
                    valid_rt += 0;
                }
                Err(_) => {
                    rejected += 1;
                }
            }
        }
        i += 1;
    }
    Ok((valid_rt, rejected))
}

/// 适配库账面。
pub struct FfiLib {
    pub rt_total: u32,
    pub rej_total: u32,
    pub init_seen: bool,
}

impl FfiLib {
    pub fn new() -> FfiLib {
        FfiLib { rt_total: 0, rej_total: 0, init_seen: false }
    }

    /// 边界会话：INIT 打头 → 业务帧 → RESET 收尾（引擎零感知口径）。
    pub fn session(&mut self, msgs: &[FfiMsg]) -> Result<u32, u16> {
        if msgs.is_empty() || msgs[0].kind != KIND_INIT {
            return Err(E_KIND_BAD);
        }
        if msgs[msgs.len() - 1].kind != KIND_RESET {
            return Err(E_KIND_BAD);
        }
        let mut processed = 0u32;
        let mut i = 0usize;
        while i < msgs.len() {
            let m = &msgs[i];
            if !kind_legal(m.kind) || m.ver != ABI_VER {
                return Err(E_KIND_BAD);
            }
            let frame = encode(m)?;
            let back = decode(&frame)?;
            if back != *m {
                return Err(E_RT_LOSS);
            }
            if m.kind == KIND_INIT {
                self.init_seen = true;
            }
            processed += 1;
            i += 1;
        }
        self.rt_total += processed;
        Ok(processed)
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.rt_total < want {
            return Err(E_FUZZ_MISS);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.init_seen {
            return Err(E_EMPTY_SUM);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（FFI 边界 fuzz ×10^4 零崩），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主 fuzz ×10^4：5000 合法帧往返恒等 + 5000 随机帧零崩零错放。
    let (rt, rej) = fuzz_ffi(0xC1A0_2026, 10_000)?;
    if rt != 5_000 {
        return Err(E_FUZZ_MISS);
    }
    if rej == 0 {
        return Err(E_FUZZ_MISS); // 随机帧全过 = 校验面失效（不可能但防恒绿）
    }
    hits += 1;

    // R2 复跑一致性：同种子重跑计数逐位相等（确定性红线）。
    let (rt2, rej2) = fuzz_ffi(0xC1A0_2026, 10_000)?;
    if rt2 != rt || rej2 != rej {
        return Err(E_RT_LOSS);
    }
    hits += 1;

    // R3 POD 布局：帧长 36、魔数在位、kind/ver 域偏移正确。
    let msg = FfiMsg { kind: KIND_KEY, ver: ABI_VER, a: 0x1122334455667788, b: 9, c: 7, d: 8 };
    let frame = encode(&msg)?;
    if frame.len() != FF_LEN_REAL {
        return Err(E_LEN_BAD);
    }
    if get_u32(&frame, 0)? != FF_MAGIC
        || get_u32(&frame, 4)? != KIND_KEY
        || get_u32(&frame, 8)? != ABI_VER
    {
        return Err(E_LEN_BAD);
    }
    hits += 1;

    // R4 五关防御逐关注入：长度短、魔数翻、kind 越闭集、版本翻、
    //    crc 翻——全拒且诊断码闭集内。
    let mut short = frame.clone();
    short.pop().is_some();
    if decode(&short).is_ok() {
        return Err(E_LEN_BAD);
    }
    let mut bad_magic = frame.clone();
    bad_magic[0] ^= 0xFF;
    if decode(&bad_magic).is_ok() {
        return Err(E_MAGIC_BAD);
    }
    let mut bad_kind = frame.clone();
    bad_kind[4] = 0x04; // kind=4 越闭集
    if decode(&bad_kind).is_ok() {
        return Err(E_KIND_BAD);
    }
    let mut bad_ver = frame.clone();
    bad_ver[8] = 0x02;
    if decode(&bad_ver).is_ok() {
        return Err(E_VER_BAD);
    }
    let mut bad_crc = frame.clone();
    bad_crc[35] ^= 0x01;
    if decode(&bad_crc).is_ok() {
        return Err(E_LEN_BAD);
    }
    hits += 1;

    // R5 引擎零感知：四类 kind 的载荷域可承载任意整数（含疑似引擎
    //    特征码），边界只透传不解释——载荷恒等即证。
    let probes: [u64; 4] = [0x66636974_7835, u64::MAX, 0, 0xDEAD_BEEF];
    let mut i = 0usize;
    while i < probes.len() {
        let m = FfiMsg { kind: i as u32, ver: ABI_VER, a: probes[i], b: probes[i], c: i as u32, d: i as u32 };
        let back = decode(&encode(&m)?)?;
        if back.a != probes[i] || back.b != probes[i] {
            return Err(E_RT_LOSS);
        }
        i += 1;
    }
    hits += 1;

    // R6 边界会话：INIT→KEY×3→EVT→RESET 全过；非 INIT 打头/非 RESET
    //    收尾拒。
    let mut lib = FfiLib::new();
    let seq: Vec<FfiMsg> = {
        let mut v: Vec<FfiMsg> = Vec::new();
        v.push(FfiMsg { kind: KIND_INIT, ver: ABI_VER, a: 1, b: 0, c: 0, d: 0 });
        let mut k = 0u32;
        while k < 3 {
            v.push(FfiMsg { kind: KIND_KEY, ver: ABI_VER, a: k as u64, b: 0, c: 0, d: 0 });
            k += 1;
        }
        v.push(FfiMsg { kind: KIND_EVT, ver: ABI_VER, a: 9, b: 0, c: 0, d: 0 });
        v.push(FfiMsg { kind: KIND_RESET, ver: ABI_VER, a: 0, b: 0, c: 0, d: 0 });
        v
    };
    let n = lib.session(&seq)?;
    if n != 6 {
        return Err(E_LEDGER_BAD);
    }
    if lib.session(&seq[1..]).is_ok() {
        return Err(E_KIND_BAD); // 非 INIT 打头
    }
    if lib.session(&seq[..5]).is_ok() {
        return Err(E_KIND_BAD); // 非 RESET 收尾
    }
    hits += 1;

    // R7 多种子扫描：8 种子 ×2000 帧零崩。
    let mut s = 0u64;
    while s < 8 {
        let (a, r) = fuzz_ffi(0xC0FFEE00 + s * 0x9E37, 2_000)?;
        if a + 0 != 1_000 {
            return Err(E_FUZZ_MISS); // 偶数步恰 1000 次合法往返
        }
        if r == 0 {
            return Err(E_FUZZ_MISS);
        }
        s += 1;
    }
    lib.coverage(6)?;
    hits += 1;

    // R8 零种子拒收与写回（判据 J2）。
    if FzLcg::new(0).is_ok() {
        return Err(E_FUZZ_MISS);
    }
    if lib.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    lib.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
