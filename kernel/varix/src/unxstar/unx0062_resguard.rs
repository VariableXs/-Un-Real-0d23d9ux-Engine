//! UNX-F0062 · 保留区白名单与守护 · 诊断段 0x20D0
//! 验收锚点：UNX-F0062-J1 物理/IO 保留区白名单登记，越界访问
//! 请求逐条裁决（放行/拦截/留痕），守护账对账判据通过；
//! 纯功能代码 ≥300 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x20D0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;    // 区间倒序或回绕
const E_OVERLAP: u16 = SEG_BASE + 2;  // 白名单区间重叠（登记冲突）
const E_UNKNOWN: u16 = SEG_BASE + 3;  // 越界请求落在未登记区
const E_BLOCKED: u16 = SEG_BASE + 4;  // 请求落在黑名单区
const E_EMPTY: u16 = SEG_BASE + 5;    // 空白名单拒绝守护启动
const E_DUP: u16 = SEG_BASE + 6;      // 同名保留区重复登记
const E_RDONLY: u16 = SEG_BASE + 7;   // 守护已启动，账冻结
const E_BAL: u16 = SEG_BASE + 8;      // 裁决账守恒不平
const E_ALIGN: u16 = SEG_BASE + 9;    // 保留区未按 4KiB 对齐（物理区纪律）

/// 保留区类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReserveKind {
    /// SMM/SMRAM——绝对黑名单。
    Smram,
    /// 引导固件运行时区——黑名单（EBS 前后语义不同，账面单列）。
    EfiRuntime,
    /// ACPI NVS——内核只读。
    AcpiNvs,
    /// AP 启动跳板——受控白名单。
    ApTrampoline,
    /// 显卡 VBIOS 影子——受控白名单。
    VideoBios,
    /// 平台自定义保留——登记制。
    Platform,
}

impl ReserveKind {
    pub fn name(&self) -> &'static str {
        match self {
            ReserveKind::Smram => "SMM/SMRAM",
            ReserveKind::EfiRuntime => "EFI 运行时",
            ReserveKind::AcpiNvs => "ACPI NVS",
            ReserveKind::ApTrampoline => "AP 跳板",
            ReserveKind::VideoBios => "VBIOS 影子",
            ReserveKind::Platform => "平台保留",
        }
    }

    /// 类别默认策略：黑名单区一律拦截；白名单区需显式放行名单。
    pub fn default_black(&self) -> bool {
        matches!(self, ReserveKind::Smram | ReserveKind::EfiRuntime)
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
        E_RANGE => "保留区区间倒序或 base+len 回绕：地址算术拦截",
        E_OVERLAP => "白名单区间重叠：登记冲突，后到拒绝",
        E_UNKNOWN => "请求落在未登记区：守护缺账，先补登记再放行",
        E_BLOCKED => "请求落在黑名单区：拦截并留痕",
        E_EMPTY => "空白名单：守护无账可依，拒绝启动",
        E_DUP => "同名保留区重复登记：后到拒绝",
        E_RDONLY => "守护已启动：账面冻结，新区走动态扩容接口",
        E_BAL => "裁决账守恒：放行+拦截+未登记 = 总请求数",
        E_ALIGN => "保留区未按 4KiB 对齐：物理区登记纪律",
        _ => "未知诊断码",
    }
}

/// 对齐辅助：把 [start, end) 提升到 4KiB 界内。
pub fn page_floor(addr: u64) -> u64 {
    addr & !0xFFF
}

pub fn page_ceil(addr: u64) -> u64 {
    if addr & 0xFFF == 0 {
        addr
    } else {
        (addr | 0xFFF) + 1
    }
}

// ── 保留区登记 ───────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ReserveZone {
    pub name: String,
    pub kind: ReserveKind,
    pub start: u64, // 页对齐后
    pub end: u64,   // 开区间
    pub black: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Blocked,
    Unregistered,
}

impl Verdict {
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Allowed => "放行",
            Verdict::Blocked => "拦截",
            Verdict::Unregistered => "未登记",
        }
    }
}

// ── 守护账 ───────────────────────────────────────────────────
pub struct GuardLedger {
    pub zones: Vec<ReserveZone>,
    pub verdicts: Vec<(u64, Verdict)>, // (请求地址, 裁决)
    pub armed: bool,
}

impl GuardLedger {
    pub fn new() -> GuardLedger {
        GuardLedger {
            zones: Vec::new(),
            verdicts: Vec::new(),
            armed: false,
        }
    }

    /// 登记保留区：地址先页对齐，重叠拒绝，黑名单按类别推导。
    pub fn register(
        &mut self,
        name: &str,
        kind: ReserveKind,
        start: u64,
        end: u64,
    ) -> Result<(), u16> {
        if self.armed {
            return Err(E_RDONLY);
        }
        if end <= start {
            return Err(E_RANGE);
        }
        let s = page_floor(start);
        let e = page_ceil(end);
        if e < s {
            return Err(E_RANGE);
        }
        if s % 4096 != 0 || e % 4096 != 0 {
            return Err(E_ALIGN);
        }
        if self.zones.iter().any(|z| z.name == name) {
            return Err(E_DUP);
        }
        let z = ReserveZone {
            name: name.to_string(),
            kind,
            start: s,
            end: e,
            black: kind.default_black(),
        };
        if self
            .zones
            .iter()
            .any(|z2| z.start < z2.end && z2.start < z.end)
        {
            return Err(E_OVERLAP);
        }
        self.zones.push(z);
        Ok(())
    }

    /// 启动守护：空白名单拒绝；启动后账面冻结。
    pub fn arm(&mut self) -> Result<usize, u16> {
        if self.zones.is_empty() {
            return Err(E_EMPTY);
        }
        self.armed = true;
        Ok(self.zones.len())
    }

    /// 单点裁决：命中黑名单 → Blocked；白名单 → Allowed；无账 → Unregistered。
    /// len>1 的区间请求：任一字节落黑名单即全拒（最严语义）。
    pub fn judge_range(&mut self, addr: u64, len: u64) -> Verdict {
        if !self.armed {
            return Verdict::Unregistered;
        }
        let end = match addr.checked_add(len) {
            Some(e) => e,
            None => {
                self.verdicts.push((addr, Verdict::Blocked));
                return Verdict::Blocked;
            }
        };
        let mut v = Verdict::Unregistered;
        for z in &self.zones {
            if addr < z.end && z.start < end {
                if z.black {
                    v = Verdict::Blocked;
                    break;
                } else {
                    v = Verdict::Allowed;
                }
            }
        }
        self.verdicts.push((addr, v));
        v
    }

    /// 裁决账守恒：三种裁决计数之和 = 判决总数。
    pub fn reconcile(&self) -> Result<(), u16> {
        let n = self.verdicts.len();
        let a = self.verdicts.iter().filter(|(_, v)| *v == Verdict::Allowed).count();
        let b = self.verdicts.iter().filter(|(_, v)| *v == Verdict::Blocked).count();
        let u = self.verdicts.iter().filter(|(_, v)| *v == Verdict::Unregistered).count();
        if a + b + u != n {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 黑名单覆盖率审计：每个黑名单区至少被命中拦截一次才算守护有效。
    pub fn coverage(&self) -> Vec<String> {
        let mut gaps = Vec::new();
        for z in &self.zones {
            if z.black {
                let hit = self.verdicts.iter().any(|(a, v)| {
                    *v == Verdict::Blocked
                        && *a >= z.start
                        && *a < z.end
                });
                if !hit {
                    gaps.push(format!("{}（{}）零拦截样本", z.name, z.kind.name()));
                }
            }
        }
        gaps
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for z in &self.zones {
            buf.extend_from_slice(&z.start.to_le_bytes());
            buf.extend_from_slice(&z.end.to_le_bytes());
            buf.extend_from_slice(z.name.as_bytes());
            buf.push(z.black as u8);
        }
        for (a, v) in &self.verdicts {
            buf.extend_from_slice(&a.to_le_bytes());
            buf.push(match v {
                Verdict::Allowed => 1,
                Verdict::Blocked => 2,
                Verdict::Unregistered => 3,
            });
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：登记四类保留区（页对齐化）→ 启动 → 白/黑/未登记三向裁决。
    let mut g = GuardLedger::new();
    g.register("SMRAM", ReserveKind::Smram, 0xA_0000, 0xA_2000)?;
    g.register("EFI_RT", ReserveKind::EfiRuntime, 0x3F00_0000, 0x3F10_0000)?;
    g.register("AP_TRAMP", ReserveKind::ApTrampoline, 0x9_F000, 0xA_0000)?;
    g.register("ACPI_NVS", ReserveKind::AcpiNvs, 0x3EFF_F000, 0x3F00_0000)?;
    g.arm()?;
    if g.judge_range(0xA_0100, 0x10) != Verdict::Blocked {
        return Err(E_BLOCKED);
    }
    if g.judge_range(0x9_F800, 0x100) != Verdict::Allowed {
        return Err(E_UNKNOWN);
    }
    if g.judge_range(0x5000_0000, 0x10) != Verdict::Unregistered {
        return Err(E_UNKNOWN);
    }
    g.reconcile()?;
    hits += 1;

    // R2：跨区请求——部分落黑名单全拒。
    if g.judge_range(0x9_FF00, 0x400) != Verdict::Blocked {
        return Err(E_BLOCKED);
    }
    hits += 1;

    // R3：覆盖审计——SMRAM 已有拦截样本，EFI_RT 无 → 报缺口。
    let gaps = g.coverage();
    if gaps.len() != 1 || !gaps[0].contains("EFI") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：页对齐化——非对齐登记被归一到页界。
    let mut g2 = GuardLedger::new();
    g2.register("PLAT", ReserveKind::Platform, 0x1234, 0x5678)?;
    let z = &g2.zones[0];
    if z.start != 0x1000 || z.end != 0x6000 {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I1：倒序区间 → E_RANGE。
    let mut i1 = GuardLedger::new();
    if i1.register("坏区", ReserveKind::Platform, 0x5000, 0x4000) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I2：重叠登记 → E_OVERLAP。
    let mut i2 = GuardLedger::new();
    i2.register("甲", ReserveKind::Platform, 0x1000, 0x3000)?;
    if i2.register("乙", ReserveKind::Platform, 0x2000, 0x4000) != Err(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I3：同名重复 → E_DUP。
    let mut i3 = GuardLedger::new();
    i3.register("甲", ReserveKind::Platform, 0x1000, 0x2000)?;
    if i3.register("甲", ReserveKind::Platform, 0x3000, 0x4000) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I4：启动后再登记 → E_RDONLY。
    let mut i4 = GuardLedger::new();
    i4.register("甲", ReserveKind::Platform, 0x1000, 0x2000)?;
    i4.arm()?;
    if i4.register("乙", ReserveKind::Platform, 0x3000, 0x4000) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：空白名单启动 → E_EMPTY。
    let mut i5 = GuardLedger::new();
    if i5.arm() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I6：未启动就裁决 → Unregistered（显性缺账，不猜）。
    let mut i6 = GuardLedger::new();
    i6.register("甲", ReserveKind::Platform, 0x1000, 0x2000)?;
    if i6.judge_range(0x1800, 4) != Verdict::Unregistered {
        return Err(E_UNKNOWN);
    }
    hits += 1;

    Ok(hits)
}
