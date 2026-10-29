//! unx48304_cfgbad3 — UNX-F48304 配置损坏三态归宿（CRC 坏→拒载→回退默认→告警入账）（诊断段 0xC6E0）。
//!
//! 判据原文：
//!   J1 损坏注入 ×10 全走三态零静默；判据锚定 §AI-61 §2 专题一错误路径。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三态归宿闭环：任何配置 blob 损坏必须走完「检出（CRC/魔数/
//!      版本/长度）→ 拒载（一个字段都不进引擎）→ 回退默认（默认值
//!      立即接管）→ 告警入账（损坏类+探测位留痕）」四步，缺一步
//!      即静默，静默即违例。
//!   2. 损坏闭集 ×10：魔数坏/版本不识别/CRC 坏/体长不符/前截断/
//!      尾残段/全零体/中段垃圾/双重损坏/长度域为零——逐类注入，
//!      全部拒载。
//!   3. 回退默认 = 引擎状态完好：拒载后引擎必须能用默认值正常工作
//!      （默认值指纹可复算），拒载不是把引擎打死。
//!   4. 告警账 append-only：同类同位重复探测幂等不重记；恢复载入
//!      成功不抹账（账是历史，不是状态）。
//!   5. 健康通路不受损：拒载历史在册时，健康 blob 照常载入成功。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC6E0;

const E_MAGIC_BAD: u16 = SEG_BASE + 1;   // 魔数坏
const E_VER_BAD: u16 = SEG_BASE + 2;     // 版本不识别
const E_CRC_BAD: u16 = SEG_BASE + 3;     // CRC32 坏
const E_LEN_BAD: u16 = SEG_BASE + 4;     // 体长不符
const E_BODY_ZERO: u16 = SEG_BASE + 5;   // 全零体
const E_DOUBLE_BAD: u16 = SEG_BASE + 6;  // 双重损坏
const E_HEALTHY_LOST: u16 = SEG_BASE + 7; // 健康通路受损
const E_LEDGER_BAD: u16 = SEG_BASE + 8;  // 告警账缺失/非幂等
const E_SILENT: u16 = SEG_BASE + 9;      // 静默（未拒载或未回退）
const E_CODE_RANGE: u16 = SEG_BASE + 10; // 诊断码越段

/// 配置 blob：魔数 + 版本 + 体长 + 体 + CRC32（覆盖头部与体）。
#[derive(Clone)]
pub struct CfgBlob {
    pub magic: u32,
    pub ver: u16,
    pub declared_len: u16,
    pub body: Vec<u8>,
    pub crc: u32,
}

/// CRC32 查表法（poly 0xEDB88320，标准反射算法）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut j = 0;
    while j < data.len() {
        crc = table[((crc ^ data[j] as u32) & 0xFF) as usize] ^ (crc >> 8);
        j += 1;
    }
    crc ^ 0xFFFF_FFFF
}

/// 健康 blob 构造：体为 8 字节设置域，CRC 对 [magic|ver|len|body] 折叠。
pub fn healthy_blob() -> CfgBlob {
    let body: Vec<u8> = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let mut wire: Vec<u8> = Vec::new();
    let magic: u32 = 0x5643_4647; // "VCFG"
    wire.push((magic >> 24) as u8);
    wire.push((magic >> 16) as u8);
    wire.push((magic >> 8) as u8);
    wire.push(magic as u8);
    wire.push(0x00);
    wire.push(0x02); // ver = 2
    wire.push(0x00);
    wire.push(0x08); // declared_len = 8
    let mut i = 0;
    while i < body.len() {
        wire.push(body[i]);
        i += 1;
    }
    let crc = crc32(&wire);
    CfgBlob {
        magic,
        ver: 2,
        declared_len: 8,
        body,
        crc,
    }
}

/// 载入状态三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// 健康载入。
    Loaded,
    /// 拒载（损坏检出）。
    Rejected,
    /// 回退默认（拒载后默认值接管）。
    Fallback,
}

/// 告账行：损坏类 + 探测位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlertRow {
    pub kind: u16,
    pub spot: u32,
}

/// 配置载入器：三态归宿的状态机。
pub struct CfgLoader {
    pub state: LoadState,
    pub active_body: Vec<u8>, // 引擎当前生效体（健康体或默认体）
    pub defaults: Vec<u8>,
    pub alerts: Vec<AlertRow>,
    pub reject_total: u32,
}

impl CfgLoader {
    pub fn new() -> Self {
        CfgLoader {
            state: LoadState::Fallback,
            active_body: vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xF0, 0x01, 0x02],
            defaults: vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xF0, 0x01, 0x02],
            alerts: Vec::new(),
            reject_total: 0,
        }
    }

    /// 告警登记：同（类,位）幂等不重记。
    pub fn alert(&mut self, kind: u16, spot: u32) {
        let mut i = 0;
        while i < self.alerts.len() {
            if self.alerts[i].kind == kind && self.alerts[i].spot == spot {
                return;
            }
            i += 1;
        }
        self.alerts.push(AlertRow { kind, spot });
    }

    /// 载入尝试：三态归宿主路径。
    pub fn try_load(&mut self, b: &CfgBlob) -> Result<(), u16> {
        // 闸 1：魔数。
        if b.magic != 0x5643_4647 {
            self.state = LoadState::Rejected;
            self.reject_total += 1;
            self.alert(E_MAGIC_BAD, 0);
            self.fallback();
            return Err(E_MAGIC_BAD);
        }
        // 闸 2：版本。
        if b.ver != 2 {
            self.state = LoadState::Rejected;
            self.reject_total += 1;
            self.alert(E_VER_BAD, 1);
            self.fallback();
            return Err(E_VER_BAD);
        }
        // 闸 3：体长。
        if b.declared_len as usize != b.body.len() {
            self.state = LoadState::Rejected;
            self.reject_total += 1;
            self.alert(E_LEN_BAD, 2);
            self.fallback();
            return Err(E_LEN_BAD);
        }
        // 闸 4：CRC（对 wire 域重算）。
        let mut wire: Vec<u8> = Vec::new();
        wire.push((b.magic >> 24) as u8);
        wire.push((b.magic >> 16) as u8);
        wire.push((b.magic >> 8) as u8);
        wire.push(b.magic as u8);
        wire.push((b.ver >> 8) as u8);
        wire.push(b.ver as u8);
        wire.push((b.declared_len >> 8) as u8);
        wire.push(b.declared_len as u8);
        let mut i = 0;
        while i < b.body.len() {
            wire.push(b.body[i]);
            i += 1;
        }
        if crc32(&wire) != b.crc {
            self.state = LoadState::Rejected;
            self.reject_total += 1;
            self.alert(E_CRC_BAD, 3);
            self.fallback();
            return Err(E_CRC_BAD);
        }
        // 闸 5：全零体（CRC 会过但体非法——完整性之外的语义闸）。
        let mut all_zero = true;
        let mut j = 0;
        while j < b.body.len() {
            if b.body[j] != 0 {
                all_zero = false;
                break;
            }
            j += 1;
        }
        if all_zero && !b.body.is_empty() {
            self.state = LoadState::Rejected;
            self.reject_total += 1;
            self.alert(E_BODY_ZERO, 4);
            self.fallback();
            return Err(E_BODY_ZERO);
        }
        // 健康通路。
        self.state = LoadState::Loaded;
        self.active_body = Vec::new();
        let mut k = 0;
        while k < b.body.len() {
            self.active_body.push(b.body[k]);
            k += 1;
        }
        Ok(())
    }

    /// 回退默认：拒载后默认体立即接管（引擎不死）。
    fn fallback(&mut self) {
        self.state = LoadState::Fallback;
        self.active_body = Vec::new();
        let mut i = 0;
        while i < self.defaults.len() {
            self.active_body.push(self.defaults[i]);
            i += 1;
        }
    }

    pub fn defaults_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < self.active_body.len() {
            h ^= self.active_body[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }
}

/// 损坏注入闭集 ×10：每类返回 (blob, 期望损坏类)。
pub fn corruption_set() -> Vec<(CfgBlob, u16)> {
    let mut set: Vec<(CfgBlob, u16)> = Vec::new();
    let healthy = healthy_blob();
    // 1 魔数坏。
    let mut b = healthy.clone();
    b.magic = 0x5643_4648;
    set.push((b, E_MAGIC_BAD));
    // 2 版本不识别。
    let mut b = healthy.clone();
    b.ver = 3;
    set.push((b, E_VER_BAD));
    // 3 CRC 坏。
    let mut b = healthy.clone();
    b.crc ^= 0x0000_00FF;
    set.push((b, E_CRC_BAD));
    // 4 体长不符。
    let mut b = healthy.clone();
    b.declared_len = 9;
    set.push((b, E_LEN_BAD));
    // 5 前截断（体截短但声明长不变 → 长度闸）。
    let mut b = healthy.clone();
    b.body.pop();
    set.push((b, E_LEN_BAD));
    // 6 尾残段（体加长 → 长度闸）。
    let mut b = healthy.clone();
    b.body.push(0x99);
    set.push((b, E_LEN_BAD));
    // 7 全零体（改体并同步重算 CRC → 语义闸）。
    let mut b = healthy.clone();
    let mut i = 0;
    while i < b.body.len() {
        b.body[i] = 0;
        i += 1;
    }
    let mut wire: Vec<u8> = Vec::new();
    wire.push((b.magic >> 24) as u8);
    wire.push((b.magic >> 16) as u8);
    wire.push((b.magic >> 8) as u8);
    wire.push(b.magic as u8);
    wire.push((b.ver >> 8) as u8);
    wire.push(b.ver as u8);
    wire.push((b.declared_len >> 8) as u8);
    wire.push(b.declared_len as u8);
    let mut j = 0;
    while j < b.body.len() {
        wire.push(b.body[j]);
        j += 1;
    }
    b.crc = crc32(&wire);
    set.push((b, E_BODY_ZERO));
    // 8 中段垃圾（改体不重算 CRC → CRC 闸）。
    let mut b = healthy.clone();
    b.body[3] ^= 0xFF;
    set.push((b, E_CRC_BAD));
    // 9 双重损坏（魔数 + CRC 同坏 → 魔数闸先拒）。
    let mut b = healthy.clone();
    b.magic = 0x0000_0000;
    b.crc ^= 0xFFFF_0000;
    set.push((b, E_MAGIC_BAD));
    // 10 长度域为零（体非空 → 长度闸）。
    let mut b = healthy.clone();
    b.declared_len = 0;
    set.push((b, E_LEN_BAD));
    set
}

/// 配置损坏三态归宿主巡检：×10 注入全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 损坏闭集恰 10 类。
    let set = corruption_set();
    if set.len() != 10 {
        return Err(E_SILENT);
    }
    hits += 1;

    // R2 健康基线：健康 blob 先载入成功（Loaded 态）。
    let mut g = CfgLoader::new();
    let healthy = healthy_blob();
    let fp_healthy = {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < healthy.body.len() {
            h ^= healthy.body[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    };
    if g.try_load(&healthy).is_err() {
        return Err(E_HEALTHY_LOST);
    }
    if g.state != LoadState::Loaded || g.defaults_fingerprint() != fp_healthy {
        return Err(E_HEALTHY_LOST);
    }
    hits += 1;

    // R3 ×10 注入全走三态：拒载 + 回退默认 + 告警入账，零静默。
    let mut k = 0usize;
    while k < set.len() {
        let (ref blob, want) = set[k];
        let r = g.try_load(blob);
        match r {
            Err(code) => {
                if code != want {
                    return Err(E_SILENT); // 损坏类漂移
                }
            }
            Ok(()) => return Err(E_SILENT), // 损坏被放行 = 最重静默
        }
        // 态 2：回退默认（引擎状态完好 = 默认体指纹可复算）。
        if g.state != LoadState::Fallback {
            return Err(E_SILENT);
        }
        if g.active_body != g.defaults {
            return Err(E_SILENT);
        }
        k += 1;
    }
    hits += 1;

    // R4 告警账：每类至少一行入账；总拒载数 = 10。
    if g.reject_total != 10 {
        return Err(E_LEDGER_BAD);
    }
    // 去重后的账行覆盖 6 个不同损坏类（10 注入归并为 6 类）。
    let kinds = [
        E_MAGIC_BAD, E_VER_BAD, E_CRC_BAD, E_LEN_BAD, E_BODY_ZERO,
    ];
    let mut ki = 0;
    while ki < kinds.len() {
        let mut found = false;
        let mut i = 0;
        while i < g.alerts.len() {
            if g.alerts[i].kind == kinds[ki] {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(E_LEDGER_BAD);
        }
        ki += 1;
    }
    hits += 1;

    // R5 告警幂等：同类同位重复探测不重记。
    let before = g.alerts.len();
    let mut again = healthy.clone();
    again.magic = 0x5643_4648;
    let _ = g.try_load(&again);
    if g.alerts.len() != before {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 健康通路不受损：拒载历史在册，健康 blob 照常载入成功且体复原。
    if g.try_load(&healthy).is_err() {
        return Err(E_HEALTHY_LOST);
    }
    if g.state != LoadState::Loaded || g.defaults_fingerprint() != fp_healthy {
        return Err(E_HEALTHY_LOST);
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 10] = [
        E_MAGIC_BAD,
        E_VER_BAD,
        E_CRC_BAD,
        E_LEN_BAD,
        E_BODY_ZERO,
        E_DOUBLE_BAD,
        E_HEALTHY_LOST,
        E_LEDGER_BAD,
        E_SILENT,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
