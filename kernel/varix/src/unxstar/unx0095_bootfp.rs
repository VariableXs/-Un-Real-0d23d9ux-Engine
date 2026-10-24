//! UNX-F0095 · 随机化指纹（Per-Boot ID）· 诊断段 0x22B0
//! 验收锚点：UNX-F0095-J1 每次引导唯一指纹（熵+时序+拓扑折叠），
//! 跨引导唯一性与同引导稳定性双判据通过；
//! 纯功能代码 ≥200 行。零浮点、零 unwrap、零 expect。


use alloc::vec;
use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x22B0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENTROPY: u16 = SEG_BASE + 1; // 熵零（全零折叠）
const E_STABLE: u16 = SEG_BASE + 2;  // 同引导两次派生不一致
const E_COLLIDE: u16 = SEG_BASE + 3; // 跨引导碰撞
const E_RDONLY: u16 = SEG_BASE + 4;  // 指纹已发行
const E_EMPTY: u16 = SEG_BASE + 5;   // 空原料
const E_BAL: u16 = SEG_BASE + 6;     // 演练断言失衡（账目/渲染不符）

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
        E_ENTROPY => "熵折叠为零：熵源全零，拒发指纹",
        E_STABLE => "同引导两次派生不一致：派生非纯函数",
        E_COLLIDE => "跨引导碰撞：指纹重复，唯一性判据破",
        E_RDONLY => "指纹已发行：一次性",
        E_EMPTY => "空原料：熵/时序/拓扑至少其一",
        E_BAL => "演练断言失衡：账目数量或渲染格式不符",
        _ => "未知诊断码",
    }
}

/// 指纹原料。
#[derive(Clone, Debug, Default)]
pub struct FingerprintMaterial {
    pub entropy_words: Vec<u64>, // 熵源字（RDRAND/熵帧折叠）
    pub tsc_boot: u64,           // 启动期 TSC 读数
    pub topology_fp: u64,        // 拓扑账指纹（NUMA/CPU 布局）
    pub boot_no: u32,
}

impl FingerprintMaterial {
    pub fn is_empty(&self) -> bool {
        self.entropy_words.is_empty() && self.tsc_boot == 0 && self.topology_fp == 0
    }
}

/// 纯函数派生：同原料必同指纹（稳定性判据的根）。
pub fn derive_fp(m: &FingerprintMaterial) -> Result<u64, u16> {
    if m.is_empty() {
        return Err(E_EMPTY);
    }
    let mut buf: Vec<u8> = Vec::new();
    for w in &m.entropy_words {
        buf.extend_from_slice(&w.to_le_bytes());
    }
    buf.extend_from_slice(&m.tsc_boot.to_le_bytes());
    buf.extend_from_slice(&m.topology_fp.to_le_bytes());
    buf.extend_from_slice(&m.boot_no.to_le_bytes());
    let fp = fnv1a64(&buf);
    if fp == 0 {
        return Err(E_ENTROPY);
    }
    Ok(fp)
}

/// 指纹发行账。
pub struct FingerprintIssuer {
    pub issued: Vec<(u32, u64)>, // (boot_no, fp)
    pub current: Option<u64>,
    pub frozen: bool,
}

impl FingerprintIssuer {
    pub fn new() -> FingerprintIssuer {
        FingerprintIssuer {
            issued: Vec::new(),
            current: None,
            frozen: false,
        }
    }

    /// 派生并发行：稳定性（双派生一致）与唯一性（跨引导不撞）双判据。
    pub fn issue(&mut self, m: &FingerprintMaterial) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        let fp1 = derive_fp(m)?;
        let fp2 = derive_fp(m)?;
        if fp1 != fp2 {
            return Err(E_STABLE);
        }
        if self.issued.iter().any(|(_, f)| *f == fp1) {
            return Err(E_COLLIDE);
        }
        self.issued.push((m.boot_no, fp1));
        self.current = Some(fp1);
        Ok(fp1)
    }

    /// 冻结（本引导发行结束）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        self.frozen = true;
        Ok(())
    }

    /// 唯一性核验：账内指纹无重复。
    pub fn uniqueness(&self) -> Result<(), u16> {
        let n = self.issued.len();
        let mut fps: Vec<u64> = self.issued.iter().map(|(_, f)| *f).collect();
        fps.sort_unstable();
        fps.dedup();
        if fps.len() != n {
            return Err(E_COLLIDE);
        }
        Ok(())
    }

    /// 指纹渲染（16 位十六进制大写）。
    pub fn render(&self) -> String {
        match self.current {
            Some(f) => format!("BOOT-{:016X}", f),
            None => "BOOT-未发行".to_string(),
        }
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for (b, f) in &self.issued {
            buf.extend_from_slice(&b.to_le_bytes());
            buf.extend_from_slice(&f.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三引导三指纹——唯一性全绿。
    let mut iss = FingerprintIssuer::new();
    for boot in 1..=3u32 {
        let m = FingerprintMaterial {
            entropy_words: vec![0xA5A5_5A5A_0000_0000 + boot as u64],
            tsc_boot: 0x1234_0000 + boot as u64,
            topology_fp: 0x777,
            boot_no: boot,
        };
        iss.issue(&m)?;
    }
    iss.uniqueness()?;
    if iss.issued.len() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：渲染格式。
    let r = iss.render();
    if !r.starts_with("BOOT-") || r.len() != 21 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：同引导重复发行 → E_COLLIDE（唯一性红线）。
    let dup = FingerprintMaterial {
        entropy_words: vec![0xA5A5_5A5A_0000_0001],
        tsc_boot: 0x1234_0001,
        topology_fp: 0x777,
        boot_no: 9,
    };
    if iss.issue(&dup) != Err(E_COLLIDE) {
        return Err(E_COLLIDE);
    }
    hits += 1;

    // I1：空原料 → E_EMPTY。
    let m0 = FingerprintMaterial::default();
    if derive_fp(&m0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I2：稳定性——同原料两次派生一致（纯函数根判据）。
    let m2 = FingerprintMaterial {
        entropy_words: vec![1, 2, 3],
        tsc_boot: 42,
        topology_fp: 7,
        boot_no: 1,
    };
    let f1 = derive_fp(&m2)?;
    let f2 = derive_fp(&m2)?;
    if f1 != f2 {
        return Err(E_STABLE);
    }
    hits += 1;

    // I3：原料变 → 指纹必变（灵敏度判据）。
    let mut m3 = m2.clone();
    m3.tsc_boot += 1;
    if derive_fp(&m3)? == f1 {
        return Err(E_STABLE);
    }
    hits += 1;

    // I4：冻结后发行 → E_RDONLY。
    iss.freeze()?;
    if iss.issue(&m3) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：空账唯一性 trivially 过——空账渲染未发行。
    let iss2 = FingerprintIssuer::new();
    if iss2.render() != "BOOT-未发行" {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
