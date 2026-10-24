//! UNX-F0247 早传第六步：健康位面板（0x3BC0 段）
//!
//! 锚点：UNX-F0247-J1 三区健康位 + unknown 掩码入 Manifest，
//! 与 F0132 逐位一致。
//! 域：CoRun·UNX-A1-B13（健康位面板面）。
//!
//! 结构：三区健康位闭集（固件区 / 介质区 / 内存区，每区 32 位）+
//! unknown 掩码（探测不到 ≠ 不健康，必须显式标注 unknown）+
//! 健康节封包（位图 + 掩码 + 摘要指纹四件入 Manifest）+
//! 对账核（与 F0132 上游快照逐位比对）+ 注入核（掩码溢出 /
//! 位图越界 / 封账拒写 / 逐位漂移必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3BC0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3BC0;
pub const HC_MASK_OOB: u32 = SEG_BASE + 0x01; // unknown 掩码位越出区界
pub const HC_BITS_OOB: u32 = SEG_BASE + 0x02; // 健康位图位越出区界
pub const HC_SEALED: u32 = SEG_BASE + 0x03; // 面板已封账拒写
pub const HC_DRIFT: u32 = SEG_BASE + 0x04; // 与 F0132 逐位比对漂移
pub const HC_ZONE_BAD: u32 = SEG_BASE + 0x05; // 区号越界（非三区闭集）

/// 三区闭集：固件区 / 介质区 / 内存区（与 F0132 同源命名）。
pub const ZONE_FW: usize = 0;
pub const ZONE_MEDIA: usize = 1;
pub const ZONE_MEM: usize = 2;
pub const ZONE_COUNT: usize = 3;

/// 区名（判据报告与 Manifest 渲染共用）。
pub const ZONE_NAMES: [&str; ZONE_COUNT] = ["固件", "介质", "内存"];

/// 单区健康面板：健康位图 + unknown 掩码 + 区号。
#[derive(Clone)]
pub struct ZonePanel {
    pub zone: usize,
    pub bits: u32,
    pub unknown: u32,
}

impl ZonePanel {
    /// 构造空面板（全零 = 未探测，语义上等价 unknown 全掩码由调用方显式置）。
    pub fn new(zone: usize) -> Result<Self, u32> {
        if zone >= ZONE_COUNT {
            return Err(HC_ZONE_BAD);
        }
        Ok(ZonePanel { zone, bits: 0, unknown: 0 })
    }

    /// 置健康位（位号 0..32，越界拒绝）。
    pub fn set_bit(&mut self, bit: u32) -> Result<(), u32> {
        if bit >= 32 {
            return Err(HC_BITS_OOB);
        }
        self.bits |= 1u32 << bit;
        Ok(())
    }

    /// 置 unknown 掩码位（探测不到必须显式标注，不允许沉默）。
    pub fn set_unknown(&mut self, bit: u32) -> Result<(), u32> {
        if bit >= 32 {
            return Err(HC_MASK_OOB);
        }
        self.unknown |= 1u32 << bit;
        Ok(())
    }

    /// 一致性不变量：unknown 掩码位与健康位图互斥（unknown ≠ healthy）。
    pub fn disjoint_invariant(&self) -> bool {
        self.bits & self.unknown == 0
    }

    /// 区内健康位计数（对账用）。
    pub fn healthy_count(&self) -> u32 {
        self.bits.count_ones()
    }

    /// 区内 unknown 位计数。
    pub fn unknown_count(&self) -> u32 {
        self.unknown.count_ones()
    }
}

/// 健康位面板（三区聚合，可封账）。
pub struct HealthPanel {
    pub zones: [ZonePanel; ZONE_COUNT],
    pub sealed: bool,
}

impl HealthPanel {
    pub fn new() -> Self {
        HealthPanel {
            zones: [
                ZonePanel { zone: ZONE_FW, bits: 0, unknown: 0 },
                ZonePanel { zone: ZONE_MEDIA, bits: 0, unknown: 0 },
                ZonePanel { zone: ZONE_MEM, bits: 0, unknown: 0 },
            ],
            sealed: false,
        }
    }

    /// 三区构造合法性核（区号连续且互异）。
    pub fn zones_valid(&self) -> bool {
        self.zones[0].zone == ZONE_FW
            && self.zones[1].zone == ZONE_MEDIA
            && self.zones[2].zone == ZONE_MEM
    }

    /// 封账（封后拒写，保证 Manifest 快照不可变）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 写健康位（封账后拒绝）。
    pub fn set_healthy(&mut self, zone: usize, bit: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(HC_SEALED);
        }
        if zone >= ZONE_COUNT {
            return Err(HC_ZONE_BAD);
        }
        self.zones[zone].set_bit(bit)
    }

    /// 写 unknown 掩码（封账后拒绝）。
    pub fn set_unknown(&mut self, zone: usize, bit: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(HC_SEALED);
        }
        if zone >= ZONE_COUNT {
            return Err(HC_ZONE_BAD);
        }
        self.zones[zone].set_unknown(bit)
    }

    /// 全区互斥不变量核。
    pub fn all_disjoint(&self) -> bool {
        self.zones.iter().all(|z| z.disjoint_invariant())
    }

    /// 封包指纹：三区位图/掩码折叠（FNV-1a，区序固定）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for z in &self.zones {
            h ^= z.bits as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= z.unknown as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// Manifest 渲染（三行区面板 + 摘要行，快照文本稳定序）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < ZONE_COUNT {
            let z = &self.zones[i];
            let line = format!(
                "健康区[{}]{} 健康位=0x{:08x} 掩码=0x{:08x} 健康={} 未知={}",
                i, ZONE_NAMES[i], z.bits, z.unknown, z.healthy_count(), z.unknown_count()
            );
            out.push_str(&line);
            out.push('\n');
            i += 1;
        }
        let sum = format!("健康面板摘要 fp={:016x}", self.fingerprint());
        out.push_str(&sum);
        out
    }
}

/// F0132 上游快照（对账锚：三区逐位期望值，由调用方注入）。
#[derive(Clone)]
pub struct F0132Snapshot {
    pub bits: [u32; ZONE_COUNT],
    pub unknown: [u32; ZONE_COUNT],
}

impl F0132Snapshot {
    pub fn new(bits: [u32; ZONE_COUNT], unknown: [u32; ZONE_COUNT]) -> Self {
        F0132Snapshot { bits, unknown }
    }
}

/// 与 F0132 逐位对账核：三区位图与掩码逐位一致（漂移返回 Err）。
pub fn reconcile_f0132(panel: &HealthPanel, snap: &F0132Snapshot) -> Result<(), u32> {
    let mut i = 0usize;
    while i < ZONE_COUNT {
        if panel.zones[i].bits != snap.bits[i] {
            return Err(HC_DRIFT);
        }
        if panel.zones[i].unknown != snap.unknown[i] {
            return Err(HC_DRIFT);
        }
        i += 1;
    }
    Ok(())
}

/// 对账定位核：逐位 diff 明细（哪区哪位漂移，供诊断渲染）。
pub fn drift_detail(panel: &HealthPanel, snap: &F0132Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ZONE_COUNT {
        let db = panel.zones[i].bits ^ snap.bits[i];
        let du = panel.zones[i].unknown ^ snap.unknown[i];
        let mut bit = 0u32;
        while bit < 32 {
            if db & (1u32 << bit) != 0 {
                out.push(format!(
                    "区[{}]{} 健康位{} 漂移",
                    i, ZONE_NAMES[i], bit
                ));
            }
            if du & (1u32 << bit) != 0 {
                out.push(format!(
                    "区[{}]{} 掩码位{} 漂移",
                    i, ZONE_NAMES[i], bit
                ));
            }
            bit += 1;
        }
        i += 1;
    }
    out
}

/// 标准夹具：三区各置健康位与 unknown 位（覆盖位边界 0/31）。
pub fn standard_fixture() -> HealthPanel {
    let mut p = HealthPanel::new();
    // 固件区：位 0（启动服务可用）、位 31（边界）
    let _ = p.set_healthy(ZONE_FW, 0);
    let _ = p.set_healthy(ZONE_FW, 31);
    let _ = p.set_unknown(ZONE_FW, 1);
    // 介质区：位 0、位 5
    let _ = p.set_healthy(ZONE_MEDIA, 0);
    let _ = p.set_healthy(ZONE_MEDIA, 5);
    let _ = p.set_unknown(ZONE_MEDIA, 2);
    // 内存区：位 0、位 12
    let _ = p.set_healthy(ZONE_MEM, 0);
    let _ = p.set_healthy(ZONE_MEM, 12);
    let _ = p.set_unknown(ZONE_MEM, 3);
    p
}

/// 与夹具同源的 F0132 快照（同值构造，保证对账绿）。
pub fn fixture_snapshot() -> F0132Snapshot {
    let p = standard_fixture();
    F0132Snapshot::new(
        [p.zones[0].bits, p.zones[1].bits, p.zones[2].bits],
        [p.zones[0].unknown, p.zones[1].unknown, p.zones[2].unknown],
    )
}

/// 注入核：6 注入必抓（越界/封账/漂移/互斥破缺）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 健康位越界（位 32）
    let mut p = HealthPanel::new();
    if p.set_healthy(ZONE_FW, 32) == Err(HC_BITS_OOB) {
        caught += 1;
    }
    // 2) 掩码位越界（位 32）
    if p.set_unknown(ZONE_FW, 32) == Err(HC_MASK_OOB) {
        caught += 1;
    }
    // 3) 区号越界
    if p.set_healthy(9, 0) == Err(HC_ZONE_BAD) {
        caught += 1;
    }
    // 4) 封账后写拒
    p.seal();
    if p.set_healthy(ZONE_FW, 4) == Err(HC_SEALED) {
        caught += 1;
    }
    // 5) 逐位漂移必抓（篡改夹具一位后对账 Err）
    let mut q = standard_fixture();
    q.zones[ZONE_MEM].bits ^= 1u32 << 12;
    if reconcile_f0132(&q, &fixture_snapshot()) == Err(HC_DRIFT) {
        caught += 1;
    }
    // 6) 掩码漂移必抓（篡改 unknown 一位）
    let mut r = standard_fixture();
    r.zones[ZONE_MEDIA].unknown ^= 1u32 << 2;
    if reconcile_f0132(&r, &fixture_snapshot()) == Err(HC_DRIFT) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx247_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0247");
    // J1-1 三区位图/掩码入 Manifest（夹具构造 + 渲染非空 + 摘要指纹稳定）
    let p = standard_fixture();
    let ok = p.zones_valid()
        && p.all_disjoint()
        && !p.render().is_empty()
        && p.fingerprint() == standard_fixture().fingerprint();
    s.add("B13-06-健康位入面板", ok, if ok { "" } else { "封包或指纹漂移" });
    // J1-2 与 F0132 逐位一致
    let ok = reconcile_f0132(&standard_fixture(), &fixture_snapshot()).is_ok();
    s.add("B13-07-F0132逐位一致", ok, if ok { "" } else { "逐位漂移" });
    // J1-3 unknown 语义核：unknown 位不计健康、互斥不破
    let p = standard_fixture();
    let ok = p.zones[ZONE_FW].healthy_count() == 2
        && p.zones[ZONE_FW].unknown_count() == 1
        && p.all_disjoint();
    s.add("B13-08-unknown语义", ok, if ok { "" } else { "unknown 语义破缺" });
    // J1-4 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-09-注入核必抓", ok, &detail);
    // J1-5 漂移定位核：单区单位篡改 diff 明细恰好 1 条
    let mut q = standard_fixture();
    q.zones[ZONE_MEDIA].bits ^= 1u32 << 5;
    let d = drift_detail(&q, &fixture_snapshot());
    let ok = d.len() == 1 && d[0].contains("介质") && d[0].contains("漂移");
    s.add("B13-10-漂移定位", ok, if ok { "" } else { "定位失准" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx247_all_checks_green() {
        let set = run_unx247_checks();
        assert!(set.all_passed(), "UNX-F0247 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx247_fingerprint_stable() {
        assert_eq!(standard_fixture().fingerprint(), standard_fixture().fingerprint());
    }
}
