//! CGPU-F0725 · 场景变体（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0725，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景变体——由基准场景经确定性旋钮组合派生变体：
//!    旋钮四维（网络档/CPU 降速/内存上限/分辨率档），变体唯一性
//!    （旋钮向量指纹去重）、变体账本与派生谱系（基场景回溯）。
//!
//! 设计纪律：
//! - 旋钮四维闭集，各档位离散表写死（规格默认档）：
//   Net{Fiber,Lte3g,Slow2g} Cpu{100,70,40}% Mem{Full,Half,Quarter}
//   Res{100,75,50}%。
// - 变体指纹：FNV-1a(基场景号|四旋钮档序)——同旋钮组合
//   同指纹，重指纹拒绝（唯一性）。
// - 谱系账：每变体记基场景号；守恒：变体数 == 派生记录数。
// - 派生确定性：同基同旋钮必产出同标识串。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x48e0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x48e0，批内 CGPU-F0725 独占）。
pub const SEG_BASE: u16 = 0x48e0;

/// 旋钮档位越界。
pub const E_KNOB: u16 = SEG_BASE + 1;
/// 变体重指纹（唯一性被破坏）。
pub const E_DUP: u16 = SEG_BASE + 2;
/// 基场景号非法（0）。
pub const E_BASE: u16 = SEG_BASE + 3;
/// 变体账已满。
pub const E_FULL: u16 = SEG_BASE + 4;
/// 谱系守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 5;
/// 未登记变体被引用。
pub const E_UNKNOWN: u16 = SEG_BASE + 6;

/// 变体账容量。
pub const VARIANTS_CAP: usize = 48;

/// 网络档（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetTier {
    /// 光纤。
    Fiber,
    /// LTE/3G。
    Lte3g,
    /// 2G 慢速。
    Slow2g,
}

/// CPU 档（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTier {
    /// 全速。
    Full,
    /// 70%。
    T70,
    /// 40%。
    T40,
}

/// 内存档（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemTier {
    /// 全量。
    Full,
    /// 半量。
    Half,
    /// 四分之一。
    Quarter,
}

/// 分辨率档（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResTier {
    /// 100%。
    R100,
    /// 75%。
    R75,
    /// 50%。
    R50,
}

/// 档位序（指纹与标识用）。
pub fn net_ord(t: NetTier) -> u8 {
    match t {
        NetTier::Fiber => 0,
        NetTier::Lte3g => 1,
        NetTier::Slow2g => 2,
    }
}
pub fn cpu_ord(t: CpuTier) -> u8 {
    match t {
        CpuTier::Full => 0,
        CpuTier::T70 => 1,
        CpuTier::T40 => 2,
    }
}
pub fn mem_ord(t: MemTier) -> u8 {
    match t {
        MemTier::Full => 0,
        MemTier::Half => 1,
        MemTier::Quarter => 2,
    }
}
pub fn res_ord(t: ResTier) -> u8 {
    match t {
        ResTier::R100 => 0,
        ResTier::R75 => 1,
        ResTier::R50 => 2,
    }
}

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 旋钮组合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Knobs {
    /// 网络档。
    pub net: NetTier,
    /// CPU 档。
    pub cpu: CpuTier,
    /// 内存档。
    pub mem: MemTier,
    /// 分辨率档。
    pub res: ResTier,
}

impl Knobs {
    /// 档序向量（固定维序 net,cpu,mem,res）。
    pub fn ords(&self) -> [u8; 4] {
        [
            net_ord(self.net),
            cpu_ord(self.cpu),
            mem_ord(self.mem),
            res_ord(self.res),
        ]
    }
    /// 组合指纹（与基场景号绑定）。
    pub fn fingerprint(&self, base: u32) -> u64 {
        let mut bytes = Vec::with_capacity(8);
        bytes.extend_from_slice(&base.to_le_bytes());
        for o in self.ords() {
            bytes.push(o);
        }
        fnv1a64(&bytes)
    }
    /// 基线组合（全默认档）。
    pub fn baseline() -> Self {
        Knobs {
            net: NetTier::Fiber,
            cpu: CpuTier::Full,
            mem: MemTier::Full,
            res: ResTier::R100,
        }
    }
}

/// 变体标识串（确定性）："<基号>-n<c>c<c>m<c>r<c>"。
pub fn variant_tag(base: u32, k: &Knobs) -> String {
    let o = k.ords();
    let mut s = String::new();
    s.push_str(&base.to_string());
    s.push_str("-n");
    s.push((b'0' + o[0]) as char);
    s.push('c');
    s.push((b'0' + o[1]) as char);
    s.push('m');
    s.push((b'0' + o[2]) as char);
    s.push('r');
    s.push((b'0' + o[3]) as char);
    s
}

/// 变体条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// 基场景号（≥1）。
    pub base: u32,
    /// 旋钮组合。
    pub knobs: Knobs,
    /// 标识串。
    pub tag: String,
    /// 指纹。
    pub fp: u64,
}

/// 变体账本（含谱系）。
///
/// 谱系守恒（显式恒等式）：
///   variants.len() == 每条目的 base 派生记录数之和
///   （即每条变体恰有一条派生记录）。
#[derive(Clone, Debug, Default)]
pub struct VariantBook {
    items: Vec<Variant>,
}

impl VariantBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    /// 派生并登记：基号合法 + 指纹唯一 + 容量。
    pub fn derive(&mut self, base: u32, k: Knobs) -> Result<String, u16> {
        if base == 0 {
            return Err(E_BASE);
        }
        if self.items.len() >= VARIANTS_CAP {
            return Err(E_FULL);
        }
        let fp = k.fingerprint(base);
        if self.items.iter().any(|x| x.fp == fp) {
            return Err(E_DUP);
        }
        let tag = variant_tag(base, &k);
        self.items.push(Variant {
            base,
            knobs: k,
            tag: tag.clone(),
            fp,
        });
        Ok(tag)
    }
    /// 按标识查。
    pub fn find(&self, tag: &str) -> Option<&Variant> {
        self.items.iter().find(|x| x.tag == tag)
    }
    /// 变体数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 某基场景的变体数。
    pub fn count_base(&self, base: u32) -> usize {
        self.items.iter().filter(|x| x.base == base).count()
    }
    /// 谱系守恒审计：账内每条目指纹复算一致且 Σ基变体数 == 总数。
    pub fn audit(&self) -> Result<(), u16> {
        let mut sum = 0usize;
        for base in 1..=64u32 {
            let c = self.count_base(base);
            if c > 0 {
                sum += c;
            }
        }
        if sum != self.items.len() {
            return Err(E_LEDGER);
        }
        for x in &self.items {
            if x.fp != x.knobs.fingerprint(x.base) || x.tag != variant_tag(x.base, &x.knobs) {
                return Err(E_LEDGER);
            }
        }
        Ok(())
    }
    /// 变体成本估计（确定性）：基成本 × 旋钮系数
    /// （cpu 档系数 100/70/40%，res 100/75/50% 相乘，整数千分比）。
    pub fn est_cost_pm(&self, tag: &str, base_cost: u64) -> Result<u64, u16> {
        let v = self.find(tag).ok_or(E_UNKNOWN)?;
        let o = v.knobs.ords();
        let cpu_pm: u64 = match o[1] {
            0 => 1000,
            1 => 700,
            _ => 400,
        };
        let res_pm: u64 = match o[3] {
            0 => 1000,
            1 => 750,
            _ => 500,
        };
        Ok(base_cost * cpu_pm / 1000 * res_pm / 1000)
    }
}

/// 变体矩阵：3^4 = 81 组合覆盖计数（派生完备性核查）。
pub const COMBO_TOTAL: usize = 81;

/// 组合总数核算（3*3*3*3，显式）。
pub fn combo_total() -> usize {
    3 * 3 * 3 * 3
}

/// 自例程：主路径（基场景派生 4 变体→账本审计→成本估计）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut book = VariantBook::new();
    // 错误路径 0：基号 0。
    if book.derive(0, Knobs::baseline()) != Err(E_BASE) {
        return Err(E_BASE);
    }
    // 主路径：基场景 101 派生 4 变体。
    let t1 = book.derive(101, Knobs::baseline())?;
    let t2 = book.derive(101, Knobs {
        net: NetTier::Slow2g,
        cpu: CpuTier::Full,
        mem: MemTier::Full,
        res: ResTier::R100,
    })?;
    let t3 = book.derive(101, Knobs {
        net: NetTier::Lte3g,
        cpu: CpuTier::T70,
        mem: MemTier::Half,
        res: ResTier::R75,
    })?;
    let t4 = book.derive(202, Knobs {
        net: NetTier::Fiber,
        cpu: CpuTier::T40,
        mem: MemTier::Quarter,
        res: ResTier::R50,
    })?;
    if book.len() != 4 {
        return Err(E_LEDGER);
    }
    if book.count_base(101) != 3 || book.count_base(202) != 1 {
        return Err(E_LEDGER);
    }
    book.audit()?;
    // 标识串确定性：t1 = "101-n0c0m0r0"。
    if t1 != "101-n0c0m0r0" {
        return Err(E_KNOB);
    }
    if t2 != "101-n2c0m0r0" || t3 != "101-n1c1m1r1" || t4 != "202-n0c2m2r2" {
        return Err(E_KNOB);
    }
    // 查找回读。
    let v = book.find(&t3).ok_or(E_UNKNOWN)?;
    if v.base != 101 || net_ord(v.knobs.net) != 1 {
        return Err(E_UNKNOWN);
    }
    // 成本估计：基 100000，T70(700‰)×R75(750‰) → 100000*700/1000=70000 → *750/1000=52500。
    let c = book.est_cost_pm(&t3, 100_000)?;
    if c != 52_500 {
        return Err(E_LEDGER);
    }
    // T40×R50：100000*400/1000=40000 → *500/1000=20000。
    let c2 = book.est_cost_pm(&t4, 100_000)?;
    if c2 != 20_000 {
        return Err(E_LEDGER);
    }
    // 错误路径 1：重复指纹（同基同旋钮）。
    if book.derive(101, Knobs::baseline()) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 注意：不同基同旋钮是不同指纹（base 进指纹）。
    book.derive(102, Knobs::baseline())?;
    if book.len() != 5 {
        return Err(E_LEDGER);
    }
    // 错误路径 2：未登记 tag 成本查询。
    if book.est_cost_pm("999-n0c0m0r0", 1) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    // 组合总数核算。
    if combo_total() != COMBO_TOTAL {
        return Err(E_LEDGER);
    }
    // 档位序闭集：三档序 0/1/2。
    if net_ord(NetTier::Fiber) != 0
        || cpu_ord(CpuTier::T70) != 1
        || mem_ord(MemTier::Quarter) != 2
        || res_ord(ResTier::R50) != 2
    {
        return Err(E_KNOB);
    }
    Ok(0)
}
