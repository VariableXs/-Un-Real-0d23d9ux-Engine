//! cgb865_amdtier —— CGPU-F0865 AMD 产品线分档。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0865 AMD 产品线分档  <SEG 0x76A0>
//! 目标行数：≥340（判据：纯功能代码 ≥340 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥340 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0865；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 产品线分档，整数账面）：
//!   1. 档位闭集五档（写死）：旗舰 / 高端 / 主流 / 入门 / APU；
//!      档号 0..=4，越界红码（XC_BAD_TIER）；
//!   2. 归档判据（写死打分表）：流处理器数 / 频率（MHz）/
//!      显存带宽（GB/s 整数）/ IC 容量（MiB）四因子加权打分
//!      （权和恒 1000 千分位），分档阈值：旗舰 ≥800 / 高端 ≥600 /
//!      主流 ≥400 / 入门 ≥200 / APU 依托管面另判；
//!   3. 设备账（定容 48 台）守恒：登记数 = 分档数 + 挂起数；漂移
//!      红码（XC_TIER_DRIFT）；
//!   4. 挂起判据（写死）：四因子任一为 0 即挂起（信息不全），
//!      挂起设备不出档（XC_INFO_INCOMPLETE）；
//!   5. APU 判据（写死）：无独立显存（bandwidth 走共享域标记）
//!      且流处理器 ≤ 2048 即 APU 档，优先于分数档；
//!   6. 档内排行：同档按分数降序排名（稳定序：先到先得并列）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x76A0 段为本模块独占（SEG_BASE=0x76A0），判别式
//!   SEG_BASE+1..+N；与 0x7680/0x76C0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AT_VERSION: &str = "cgb865_amdtier-v1";

/// 诊断段基址（0x76A0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x76A0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x76A1 档位越界（0..=4 之外）。
pub const XC_BAD_TIER: u16 = SEG_BASE + 1;
/// 0x76A2 设备号越界。
pub const XC_BAD_DEVICE: u16 = SEG_BASE + 2;
/// 0x76A3 分档账漂移（登记 ≠ 分档 + 挂起）。
pub const XC_TIER_DRIFT: u16 = SEG_BASE + 3;
/// 0x76A4 信息不全（因子为 0 即求出档）。
pub const XC_INFO_INCOMPLETE: u16 = SEG_BASE + 4;
/// 0x76A5 权重账漂移（权和 ≠ 1000）。
pub const XC_WEIGHT_DRIFT: u16 = SEG_BASE + 5;
/// 0x76A6 设备账满（48）。
pub const XC_DEVICE_FULL: u16 = SEG_BASE + 6;
/// 0x76A7 重复设备签名。
pub const XC_DUP_DEVICE: u16 = SEG_BASE + 7;
/// 0x76A8 因子越界（带宽 > 4096 GB/s 等荒谬值）。
pub const XC_FACTOR_ABSURD: u16 = SEG_BASE + 8;

/// 档位数（写死：5）。
pub const TIER_CAP: usize = 5;
/// 设备账容量（写死：48）。
pub const DEVICE_CAP: usize = 48;
/// 权和（千分位）。
pub const WEIGHT_SUM: u32 = 1000;
/// 带宽荒谬上限（GB/s）。
pub const BW_ABSURD: u32 = 4096;
/// 频率荒谬上限（MHz）。
pub const CLK_ABSURD: u32 = 5000;
/// 流处理器荒谬上限。
pub const CU_ABSURD: u32 = 32768;
/// APU 流处理器上限（写死）。
pub const APU_CU_MAX: u32 = 2048;

/// 档位闭集：0=旗舰 1=高端 2=主流 3=入门 4=APU。
pub const T_FLAGSHIP: usize = 0;
pub const T_ENTHUSIAST: usize = 1;
pub const T_MAINSTREAM: usize = 2;
pub const T_ENTRY: usize = 3;
pub const T_APU: usize = 4;

/// 档名（闭集原文）。
pub fn tier_name(t: usize) -> &'static str {
    match t {
        T_FLAGSHIP => "flagship",
        T_ENTHUSIAST => "enthusiast",
        T_MAINSTREAM => "mainstream",
        T_ENTRY => "entry",
        _ => "apu",
    }
}

/// 四因子权重（和恒 1000）：CU×4 / 频×3 / 带宽×2 / IC×1。
pub const FACTOR_W: [u32; 4] = [400, 300, 200, 100];

/// 因子归一上限（打分分母）。
pub const FACTOR_NORM: [u32; 4] = [16384, 3000, 1024, 128];

/// 四因子面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Factors {
    /// 流处理器数。
    pub cus: u32,
    /// 频率（MHz）。
    pub clock: u32,
    /// 带宽（GB/s）。
    pub bandwidth: u32,
    /// IC 容量（MiB；0 = 无 IC/APU 共享）。
    pub ic: u32,
}

impl Factors {
    /// 信息齐全判定（前三维非零）。
    pub fn complete(&self) -> bool {
        self.cus != 0 && self.clock != 0 && self.bandwidth != 0
    }

    /// 荒谬值扫描。
    pub fn absurd(&self) -> bool {
        self.cus > CU_ABSURD
            || self.clock > CLK_ABSURD
            || self.bandwidth > BW_ABSURD
            || self.ic > 512
    }

    /// 共享域标记（无独立带宽域的 APU 判据输入）。
    pub fn is_apu_shape(&self) -> bool {
        self.cus <= APU_CU_MAX && self.ic == 0
    }
}

/// 四因子加权打分（千分位满分 1000）。
pub fn score_of(f: &Factors) -> Result<u32, u16> {
    let vals = [f.cus, f.clock, f.bandwidth, f.ic];
    let mut acc: u64 = 0;
    let mut i = 0;
    while i < 4 {
        let norm = FACTOR_NORM[i] as u64;
        let v = vals[i].min(FACTOR_NORM[i]) as u64;
        acc += v * FACTOR_W[i] as u64 * 1000 / norm;
        i += 1;
    }
    Ok((acc / WEIGHT_SUM as u64) as u32)
}

/// 分档判定（APU 优先于分数档）。
pub fn tier_of(f: &Factors, score: u32) -> usize {
    if f.is_apu_shape() {
        return T_APU;
    }
    if score >= 800 {
        T_FLAGSHIP
    } else if score >= 600 {
        T_ENTHUSIAST
    } else if score >= 400 {
        T_MAINSTREAM
    } else {
        T_ENTRY
    }
}

/// 单台设备。
#[derive(Clone, Debug)]
pub struct Device {
    /// 签名（去重键）。
    pub sig: String,
    /// 四因子。
    pub factors: Factors,
    /// 分数（千分位面）。
    pub score: u32,
    /// 档位（挂起设备 = 255）。
    pub tier: u8,
    /// 挂起标记。
    pub suspended: bool,
}

/// AMD 产品线分档册。
pub struct TierBook {
    /// 设备账。
    pub rows: Vec<Device>,
    /// 各档计数。
    pub tier_count: [u64; TIER_CAP],
    /// 分档数。
    pub tiered: u64,
    /// 挂起数。
    pub suspended: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl TierBook {
    /// 空册。
    pub fn new() -> Self {
        TierBook {
            rows: Vec::new(),
            tier_count: [0; TIER_CAP],
            tiered: 0,
            suspended: 0,
            red_count: 0,
        }
    }

    /// 登记设备（闭集校验 + 去重 + 容量；自动挂起或分档）。
    pub fn enroll(&mut self, sig: &str, f: Factors) -> Result<usize, u16> {
        if f.absurd() {
            self.red_count += 1;
            return Err(XC_FACTOR_ABSURD);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DEVICE);
        }
        if self.rows.len() == DEVICE_CAP {
            self.red_count += 1;
            return Err(XC_DEVICE_FULL);
        }
        let dev = if !f.complete() {
            Device {
                sig: String::from(sig),
                factors: f,
                score: 0,
                tier: 255,
                suspended: true,
            }
        } else {
            let sc = score_of(&f)?;
            Device {
                sig: String::from(sig),
                factors: f,
                score: sc,
                tier: tier_of(&f, sc) as u8,
                suspended: false,
            }
        };
        self.rows.push(dev);
        let idx = self.rows.len() - 1;
        if self.rows[idx].suspended {
            self.suspended += 1;
        } else {
            self.tier_count[self.rows[idx].tier as usize] += 1;
            self.tiered += 1;
        }
        Ok(idx)
    }

    /// 签名查重。
    pub fn has_sig(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 挂起设备补全（补因子后重新分档）。
    pub fn resume(&mut self, idx: usize, f: Factors) -> Result<usize, u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if !self.rows[idx].suspended {
            self.red_count += 1;
            return Err(XC_INFO_INCOMPLETE);
        }
        if f.absurd() {
            self.red_count += 1;
            return Err(XC_FACTOR_ABSURD);
        }
        if !f.complete() {
            self.red_count += 1;
            return Err(XC_INFO_INCOMPLETE);
        }
        let sc = score_of(&f)?;
        self.rows[idx].factors = f;
        self.rows[idx].score = sc;
        let t = tier_of(&f, sc) as u8;
        self.rows[idx].tier = t;
        self.rows[idx].suspended = false;
        self.suspended -= 1;
        self.tier_count[t as usize] += 1;
        self.tiered += 1;
        Ok(t as usize)
    }

    /// 档内排行（返回该档设备索引按分数降序；并列先到先得）。
    pub fn rank_tier(&mut self, tier: usize) -> Result<Vec<usize>, u16> {
        if tier >= TIER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_TIER);
        }
        let mut idxs: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if !self.rows[i].suspended && self.rows[i].tier as usize == tier {
                idxs.push(i);
            }
            i += 1;
        }
        // 简单插入排序（稳定，降序）。
        let mut a = 1;
        while a < idxs.len() {
            let key = idxs[a];
            let kv = self.rows[key].score;
            let mut b = a;
            while b > 0 && self.rows[idxs[b - 1]].score < kv {
                idxs[b] = idxs[b - 1];
                b -= 1;
            }
            idxs[b] = key;
            a += 1;
        }
        Ok(idxs)
    }

    /// 账守恒。
    pub fn balanced(&self) -> bool {
        let mut sum: u64 = 0;
        let mut t = 0;
        while t < TIER_CAP {
            sum += self.tier_count[t];
            t += 1;
        }
        self.rows.len() as u64 == sum + self.suspended
            && self.tiered == sum
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdtier devices=");
        s += &self.rows.len().to_string();
        s += " tiered=";
        s += &self.tiered.to_string();
        s += " suspended=";
        s += &self.suspended.to_string();
        let mut t = 0;
        while t < TIER_CAP {
            s += " ";
            s += tier_name(t);
            s += "=";
            s += &self.tier_count[t].to_string();
            t += 1;
        }
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_TIER_DRIFT);
        }
        // 权重和复核。
        let mut wsum: u32 = 0;
        let mut i = 0;
        while i < 4 {
            wsum += FACTOR_W[i];
            i += 1;
        }
        if wsum != WEIGHT_SUM {
            return Err(XC_WEIGHT_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：登记五档设备 → 排行 → 挂起补全 → 审计 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut b = TierBook::new();
    // 旗舰：满因子。
    let flag = Factors { cus: 16384, clock: 2500, bandwidth: 1000, ic: 128 };
    // 高端。
    let enth = Factors { cus: 10240, clock: 2300, bandwidth: 700, ic: 96 };
    // 主流。
    let main = Factors { cus: 4096, clock: 2000, bandwidth: 400, ic: 64 };
    // 入门。
    let entry = Factors { cus: 2048, clock: 1800, bandwidth: 200, ic: 0 };
    // APU 形（CU ≤ 2048 且无 IC）。
    let apu = Factors { cus: 1536, clock: 2800, bandwidth: 120, ic: 0 };
    b.enroll("rx-9900", flag)?;
    b.enroll("rx-8800", enth)?;
    b.enroll("rx-7600", main)?;
    b.enroll("rx-6400", entry)?;
    b.enroll("apu-8700g", apu)?;
    // 负例：信息不全。
    if b.enroll("no-info", Factors { cus: 0, clock: 2000, bandwidth: 300, ic: 0 })
        != Err(XC_INFO_INCOMPLETE)
    {
        return Err(XC_INFO_INCOMPLETE);
    }
    // 负例：荒谬因子。
    if b.enroll("absurd", Factors { cus: 99999, clock: 2000, bandwidth: 300, ic: 0 })
        != Err(XC_FACTOR_ABSURD)
    {
        return Err(XC_FACTOR_ABSURD);
    }
    // 负例：重名。
    if b.enroll("rx-9900", flag) != Err(XC_DUP_DEVICE) {
        return Err(XC_DUP_DEVICE);
    }
    b.audit()?;
    // 挂起补全。
    b.enroll("mystery", Factors { cus: 8192, clock: 0, bandwidth: 500, ic: 64 })?;
    if b.rows[5].suspended != true {
        return Err(XC_TIER_DRIFT);
    }
    let t5 = b.resume(5, Factors { cus: 8192, clock: 2200, bandwidth: 500, ic: 64 })?;
    if t5 != T_MAINSTREAM && t5 != T_ENTHUSIAST {
        return Err(XC_BAD_TIER);
    }
    // 负例：非挂起设备补全。
    if b.resume(0, flag) != Err(XC_INFO_INCOMPLETE) {
        return Err(XC_INFO_INCOMPLETE);
    }
    b.audit()?;
    // 分档核对：旗舰应入 T_FLAGSHIP。
    if b.rows[0].tier as usize != T_FLAGSHIP {
        return Err(XC_BAD_TIER);
    }
    if b.rows[4].tier as usize != T_APU {
        return Err(XC_BAD_TIER);
    }
    // 排行：旗舰档内 rx-9900 应列首。
    let rank = b.rank_tier(T_FLAGSHIP)?;
    if rank.is_empty() || b.rows[rank[0]].sig != "rx-9900" {
        return Err(XC_TIER_DRIFT);
    }
    // 负例：档位越界。
    if b.rank_tier(TIER_CAP).is_err() {
        // 期望红码，继续。
    } else {
        return Err(XC_BAD_TIER);
    }
    // 权重账：分数上限复核（满因子 = 1000）。
    let sc = score_of(&flag)?;
    if sc > 1000 {
        return Err(XC_WEIGHT_DRIFT);
    }
    if b.summary().len() == 0 || !b.balanced() {
        return Err(XC_TIER_DRIFT);
    }
    Ok(0)
}
