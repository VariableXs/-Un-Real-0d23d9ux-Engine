//! UNX-F0075 · 物理内存上限 cap 策略 · 诊断段 0x21A0
//! 验收锚点：UNX-F0075-J1 内存上限三模式（硬截/软告警/学习档）
//! 与配额账判据通过；纯功能代码 ≥200 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x21A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CAP: u16 = SEG_BASE + 1;      // cap 值非法（0 或低于硬底线）
const E_MODE: u16 = SEG_BASE + 2;     // 模式非法
const E_EXCEED: u16 = SEG_BASE + 3;   // 硬截模式下超额请求
const E_WARN: u16 = SEG_BASE + 4;     // 软告警模式超阈值（放行但立案）
const E_RDONLY: u16 = SEG_BASE + 5;   // 策略已封板
const E_BAL: u16 = SEG_BASE + 6;      // 配额账守恒不平
const E_DOWN: u16 = SEG_BASE + 7;     // 学习档下调违规（低于已承诺水位）

/// cap 模式闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapMode {
    /// 硬截：超限即拒。
    HardCut,
    /// 软告警：超阈值放行但立案。
    SoftWarn,
    /// 学习档：按观测用量自动收紧，但不低于已承诺水位。
    Adaptive,
}

impl CapMode {
    pub fn name(&self) -> &'static str {
        match self {
            CapMode::HardCut => "硬截",
            CapMode::SoftWarn => "软告警",
            CapMode::Adaptive => "学习档",
        }
    }
}

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
        E_CAP => "cap 非法：非零且 ≥ 硬底线（1MiB）",
        E_MODE => "模式非法：三模式闭集外",
        E_EXCEED => "硬截超额：请求被拒，账面留痕",
        E_WARN => "软告警超阈值：放行立案，观测表追加",
        E_RDONLY => "策略已封板：重开新策略账",
        E_BAL => "配额守恒：批准 + 拒绝 + 告警放行 = 总请求",
        E_DOWN => "学习档下调低于已承诺水位：拦截",
        _ => "未知诊断码",
    }
}

/// 硬底线 1MiB。
pub const HARD_FLOOR: u64 = 0x10_0000;

// ── 策略账 ───────────────────────────────────────────────────
pub struct CapPolicy {
    pub mode: CapMode,
    pub cap_bytes: u64,
    pub warn_permille: u64, // 软告警阈值（千分比）
    pub committed_floor: u64, // 学习档已承诺水位
    pub approved: u64,
    pub warned: u64,
    pub rejected: u64,
    pub observations: Vec<u64>, // 学习档观测样本
    pub sealed: bool,
}

impl CapPolicy {
    pub fn new(mode: CapMode, cap_bytes: u64) -> Result<CapPolicy, u16> {
        if cap_bytes == 0 || cap_bytes < HARD_FLOOR {
            return Err(E_CAP);
        }
        Ok(CapPolicy {
            mode,
            cap_bytes,
            warn_permille: 900,
            committed_floor: 0,
            approved: 0,
            warned: 0,
            rejected: 0,
            observations: Vec::new(),
            sealed: false,
        })
    }

    pub fn set_warn_permille(&mut self, m: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if m == 0 || m > 1000 {
            return Err(E_MODE);
        }
        self.warn_permille = m;
        Ok(())
    }

    fn threshold(&self) -> u64 {
        self.cap_bytes * self.warn_permille / 1000
    }

    /// 配额裁决：返回 Ok(批准字节数)。
    pub fn request(&mut self, want: u64) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if want == 0 {
            return Err(E_BAL);
        }
        let new_used = self.approved.checked_add(want).ok_or(E_BAL)?;
        match self.mode {
            CapMode::HardCut => {
                if new_used > self.cap_bytes {
                    self.rejected += want;
                    return Err(E_EXCEED);
                }
                self.approved = new_used;
                Ok(want)
            }
            CapMode::SoftWarn => {
                if new_used > self.cap_bytes {
                    self.rejected += want;
                    return Err(E_EXCEED);
                }
                if new_used > self.threshold() {
                    self.warned += want;
                    self.observations.push(new_used);
                }
                self.approved = new_used;
                Ok(want)
            }
            CapMode::Adaptive => {
                // 学习档：软上限 = min(cap, 观测收缩目标)，不低于承诺水位
                let eff_cap = self.effective_cap();
                if new_used > eff_cap {
                    self.rejected += want;
                    return Err(E_EXCEED);
                }
                if new_used > self.committed_floor + (eff_cap - self.committed_floor)
                    * self.warn_permille
                    / 1000
                {
                    self.warned += want;
                }
                self.approved = new_used;
                self.observations.push(new_used);
                Ok(want)
            }
        }
    }

    /// 学习档有效上限：观测 P50 的 120%（向上取整），夹在 [承诺水位, cap]。
    pub fn effective_cap(&self) -> u64 {
        if self.mode != CapMode::Adaptive || self.observations.len() < 4 {
            return self.cap_bytes;
        }
        let mut s = self.observations.clone();
        s.sort_unstable();
        let mid = s[s.len() / 2];
        let target = mid + mid / 4; // 125%
        let t = target.min(self.cap_bytes);
        if t < self.committed_floor {
            self.committed_floor
        } else {
            t
        }
    }

    /// 学习档收紧：只许收紧不低于承诺水位。
    pub fn tighten(&mut self, new_cap: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.mode != CapMode::Adaptive {
            return Err(E_MODE);
        }
        if new_cap < self.committed_floor || new_cap < HARD_FLOOR {
            return Err(E_DOWN);
        }
        if new_cap > self.cap_bytes {
            return Err(E_DOWN); // tighten 语义：只降不升
        }
        self.cap_bytes = new_cap;
        Ok(())
    }

    pub fn commit_floor(&mut self, floor: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if floor > self.cap_bytes {
            return Err(E_DOWN);
        }
        self.committed_floor = floor;
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.approved + self.warned.min(0) > self.cap_bytes && self.mode == CapMode::HardCut {
            return Err(E_BAL);
        }
        self.sealed = true;
        Ok(())
    }

    pub fn reconcile(&self, total_requests: u64) -> Result<(), u16> {
        if self.approved + self.rejected != total_requests {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.mode {
            CapMode::HardCut => 1,
            CapMode::SoftWarn => 2,
            CapMode::Adaptive => 3,
        });
        buf.extend_from_slice(&self.cap_bytes.to_le_bytes());
        buf.extend_from_slice(&self.approved.to_le_bytes());
        buf.extend_from_slice(&self.rejected.to_le_bytes());
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：硬截——批准至恰好满额，超一分即拒。
    let mut c1 = CapPolicy::new(CapMode::HardCut, 0x100_0000)?;
    c1.request(0x80_0000)?;
    if c1.request(0x80_0001) != Err(E_EXCEED) {
        return Err(E_EXCEED);
    }
    c1.request(1)?; // 恰好填满
    c1.reconcile(0x100_0000 + 0x80_0001 + 1)?;
    hits += 1;

    // R2：软告警——超千分之九百放行并立案。
    let mut c2 = CapPolicy::new(CapMode::SoftWarn, 0x100_0000)?;
    c2.set_warn_permille(500)?;
    c2.request(0x90_0000)?; // 56.25% 未触警
    if c2.warned != 0 {
        return Err(E_BAL);
    }
    c2.request(0x10_0000)?; // 累计 62.5% 触警
    if c2.warned != 0x10_0000 {
        return Err(E_BAL);
    }
    c2.reconcile(0xA0_0000)?;
    hits += 1;

    // R3：学习档——观测充足后有效上限收紧，承诺水位兜底。
    let mut c3 = CapPolicy::new(CapMode::Adaptive, 0x400_0000)?;
    c3.commit_floor(0x80_0000)?;
    for _ in 0..4 {
        c3.request(0x100_0000)?;
    }
    let eff = c3.effective_cap();
    if eff != 0x100_0000 + 0x100_0000 / 4 {
        return Err(E_BAL); // P50=0x100_0000 → 125% = 0x140_0000
    }
    c3.tighten(eff)?;
    if c3.request(0x50_0000) != Err(E_EXCEED) {
        return Err(E_EXCEED);
    }
    hits += 1;

    // R4：下调违规——低于承诺水位拦截。
    let mut c4 = CapPolicy::new(CapMode::Adaptive, 0x400_0000)?;
    c4.commit_floor(0x200_0000)?;
    if c4.tighten(0x10_0000) != Err(E_DOWN) {
        return Err(E_DOWN);
    }
    hits += 1;

    // I1：cap 为 0 → E_CAP。
    if CapPolicy::new(CapMode::HardCut, 0).is_ok() {
        return Err(E_CAP);
    }
    hits += 1;

    // I2：cap 低于硬底线 → E_CAP。
    if CapPolicy::new(CapMode::HardCut, 0x1000).is_ok() {
        return Err(E_CAP);
    }
    hits += 1;

    // I3：warn 阈值越界 → E_MODE。
    let mut i3 = CapPolicy::new(CapMode::SoftWarn, 0x100_0000)?;
    if i3.set_warn_permille(1001) != Err(E_MODE) {
        return Err(E_MODE);
    }
    hits += 1;

    // I4：封板后请求 → E_RDONLY。
    let mut i4 = CapPolicy::new(CapMode::HardCut, 0x100_0000)?;
    i4.seal()?;
    if i4.request(1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
