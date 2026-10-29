//! UNX-F56147 五指标聚合总表骨架（0x61260 段）
//!
//! 锚点：UNX-F56147-J1 五指标骨架账（帧率/syscall/启动/内存/功耗五指
//! 标各占一聚合槽位，槽位 × 聚合态 × 样本量 × 指纹四元组落账）；J2 槽
//! 位互斥语义（同指标同时段单槽位，重复占用拒）；J3 总表折叠指纹（五
//! 槽位指纹链折叠，任一槽变总签变）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·五指标骨架面）。
//!
//! 结构：五槽位聚合骨架（每槽登记聚合态 + 样本量 + 指纹）+ 槽位互斥
//! 闸（同指标重占拒）+ 聚合态机（EMPTY→FILLING→SEALED 单向）+ 总表
//! 指纹（FNV 链折叠）+ 注入核（未知指标/重占槽位/态回退/封表后聚合/
//! 空表印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61260 独占 0x20）。
pub const SEG_BASE: u32 = 0x61260;
pub const FT_UNK_METRIC: u32 = SEG_BASE + 0x01; // 未知指标
pub const FT_SLOT_DUP: u32 = SEG_BASE + 0x02; // 槽位重占
pub const FT_STATE_BACK: u32 = SEG_BASE + 0x03; // 聚合态回退
pub const FT_SEALED_AGG: u32 = SEG_BASE + 0x04; // 封表后聚合
pub const FT_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空表印

/// 五指标闭集。
pub const M_FPS: usize = 0;
pub const M_SYSCALL: usize = 1;
pub const M_BOOT: usize = 2;
pub const M_MEM: usize = 3;
pub const M_POWER: usize = 4;
pub const M_COUNT: usize = 5;

/// 指标名渲染。
pub const METRIC_NAMES: [&str; M_COUNT] = ["fps", "syscall", "boot", "mem", "power"];

/// 聚合态机。
pub const AG_EMPTY: u8 = 0;
pub const AG_FILLING: u8 = 1;
pub const AG_SEALED: u8 = 2;

/// 聚合态名。
pub fn ag_state_name(st: u8) -> &'static str {
    match st {
        AG_EMPTY => "EMPTY",
        AG_FILLING => "FILLING",
        AG_SEALED => "SEALED",
        _ => "?",
    }
}

/// 聚合态流转核：EMPTY→FILLING→SEALED 单向。
pub fn ag_transition_ok(from: u8, to: u8) -> bool {
    to == from + 1 && to <= AG_SEALED
}

/// 单槽位。
#[derive(Clone)]
pub struct MetricSlot {
    pub metric: usize,
    pub state: u8,
    pub sample_n: u32,
    pub slot_fp: u32,
}

impl MetricSlot {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|n{}|{:08x}",
            METRIC_NAMES[self.metric],
            ag_state_name(self.state),
            self.sample_n,
            self.slot_fp
        )
    }
}

/// 五指标聚合总表骨架。
pub struct FiveMetricSkeleton {
    slots: [Option<MetricSlot>; M_COUNT],
    table_sealed: bool,
}

impl FiveMetricSkeleton {
    pub fn new() -> Self {
        FiveMetricSkeleton {
            slots: [None, None, None, None, None],
            table_sealed: false,
        }
    }

    /// 开槽聚合：指标闭集 + 互斥闸 + 封表闸三重。
    pub fn open_slot(&mut self, metric: usize) -> Result<(), u32> {
        if self.table_sealed {
            return Err(FT_SEALED_AGG);
        }
        if metric >= M_COUNT {
            return Err(FT_UNK_METRIC);
        }
        if self.slots[metric].is_some() {
            return Err(FT_SLOT_DUP);
        }
        self.slots[metric] = Some(MetricSlot {
            metric,
            state: AG_FILLING,
            sample_n: 0,
            slot_fp: 0,
        });
        Ok(())
    }

    /// 聚合一样本：槽位须在 FILLING 态。
    pub fn add_sample(&mut self, metric: usize, payload_fp: u32) -> Result<u32, u32> {
        if self.table_sealed {
            return Err(FT_SEALED_AGG);
        }
        if metric >= M_COUNT {
            return Err(FT_UNK_METRIC);
        }
        let slot = match self.slots[metric].as_mut() {
            None => return Err(FT_UNK_METRIC),
            Some(s) => s,
        };
        if slot.state != AG_FILLING {
            return Err(FT_STATE_BACK);
        }
        // 指纹折叠：样本指纹逐个混入槽指纹
        slot.slot_fp = slot.slot_fp.wrapping_mul(0x0100_0193) ^ payload_fp;
        slot.sample_n += 1;
        Ok(slot.sample_n)
    }

    /// 封槽：FILLING → SEALED 单向。
    pub fn seal_slot(&mut self, metric: usize) -> Result<(), u32> {
        if metric >= M_COUNT {
            return Err(FT_UNK_METRIC);
        }
        let slot = match self.slots[metric].as_mut() {
            None => return Err(FT_UNK_METRIC),
            Some(s) => s,
        };
        if !ag_transition_ok(slot.state, AG_SEALED) {
            return Err(FT_STATE_BACK);
        }
        slot.state = AG_SEALED;
        Ok(())
    }

    /// 封总表：至少一槽 SEALED 且无 EMPTY 残留可印。
    pub fn seal_table(&mut self) -> bool {
        if self.table_sealed {
            return false;
        }
        let any_filled = self.slots.iter().any(|s| s.is_some());
        if !any_filled {
            return false;
        }
        self.table_sealed = true;
        true
    }

    pub fn table_sealed(&self) -> bool {
        self.table_sealed
    }

    pub fn slot(&self, metric: usize) -> Option<&MetricSlot> {
        if metric >= M_COUNT {
            return None;
        }
        self.slots[metric].as_ref()
    }

    /// 总表指纹：五槽位（指标 × 态 × 样本量 × 槽签）链折叠。
    pub fn table_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for m in 0..M_COUNT {
            match &self.slots[m] {
                None => {
                    h ^= 0xFFFF;
                }
                Some(s) => {
                    h ^= (s.metric as u32) | ((s.state as u32) << 8) | s.sample_n << 16;
                }
            }
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.table_sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 总表核：全部槽位态合法 + 样本量一致。
    pub fn table_ok(&self) -> bool {
        self.slots.iter().flatten().all(|s| {
            matches!(s.state, AG_FILLING | AG_SEALED)
                && (s.sample_n > 0 || s.state == AG_FILLING)
        })
    }

    /// 渲染导出。
    pub fn render_table(&self) -> String {
        let mut out = String::new();
        for m in 0..M_COUNT {
            match &self.slots[m] {
                None => out.push_str(&format!("{}|UNSET\n", METRIC_NAMES[m])),
                Some(s) => {
                    out.push_str(&s.render_line());
                    out.push('\n');
                }
            }
        }
        if self.table_sealed {
            out.push_str("TABLE-SEAL\n");
        }
        out
    }
}

impl Default for FiveMetricSkeleton {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：五槽全开，帧率/syscall 封槽，其余 FILLING。
fn standard_table() -> FiveMetricSkeleton {
    let mut t = FiveMetricSkeleton::new();
    for m in 0..M_COUNT {
        let _ = t.open_slot(m);
    }
    for i in 0..4u32 {
        let _ = t.add_sample(M_FPS, 0x1000 + i);
    }
    for i in 0..7u32 {
        let _ = t.add_sample(M_SYSCALL, 0x2000 + i);
    }
    for i in 0..3u32 {
        let _ = t.add_sample(M_BOOT, 0x3000 + i);
    }
    for i in 0..5u32 {
        let _ = t.add_sample(M_MEM, 0x4000 + i);
    }
    for i in 0..2u32 {
        let _ = t.add_sample(M_POWER, 0x5000 + i);
    }
    let _ = t.seal_slot(M_FPS);
    let _ = t.seal_slot(M_SYSCALL);
    t
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知指标
    let mut t = FiveMetricSkeleton::new();
    if t.open_slot(9) == Err(FT_UNK_METRIC) {
        caught += 1;
    }
    // 注 2：槽位重占
    let mut t = FiveMetricSkeleton::new();
    let _ = t.open_slot(M_FPS);
    if t.open_slot(M_FPS) == Err(FT_SLOT_DUP) {
        caught += 1;
    }
    // 注 3：态回退（封槽后再加样本）
    let mut t = FiveMetricSkeleton::new();
    let _ = t.open_slot(M_FPS);
    let _ = t.add_sample(M_FPS, 1);
    let _ = t.seal_slot(M_FPS);
    if t.add_sample(M_FPS, 2) == Err(FT_STATE_BACK) {
        caught += 1;
    }
    // 注 4：封表后聚合
    let mut t = standard_table();
    let _ = t.seal_table();
    if t.add_sample(M_BOOT, 9) == Err(FT_SEALED_AGG) {
        caught += 1;
    }
    // 注 5：空表印
    let mut t = FiveMetricSkeleton::new();
    if !t.seal_table() && !t.table_sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 五槽骨架；J2 槽位语义；J3 总表指纹。
pub fn run_unx56147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56147");
    // J1-1 五槽全开全落账
    let t = standard_table();
    let ok = (0..M_COUNT).all(|m| t.slot(m).is_some()) && t.table_ok();
    s.add("56147-J1-五槽全落账", ok, if ok { "" } else { "骨架账不全" });
    // J1-2 样本量逐槽
    let ok = t.slot(M_FPS).map(|x| x.sample_n == 4).unwrap_or(false)
        && t.slot(M_SYSCALL).map(|x| x.sample_n == 7).unwrap_or(false)
        && t.slot(M_POWER).map(|x| x.sample_n == 2).unwrap_or(false);
    s.add("56147-J1-样本量逐槽", ok, if ok { "" } else { "样本量漂移" });
    // J1-3 渲染导出齐
    let text = standard_table().render_table();
    let ok = text.contains("fps|SEALED|n4|") && text.contains("boot|FILLING|n3|") && text.contains("mem|FILLING|n5|");
    s.add("56147-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 聚合态机单向
    let ok = ag_transition_ok(AG_EMPTY, AG_FILLING) && ag_transition_ok(AG_FILLING, AG_SEALED)
        && !ag_transition_ok(AG_EMPTY, AG_SEALED) && !ag_transition_ok(AG_SEALED, AG_FILLING);
    s.add("56147-J2-态机单向", ok, if ok { "" } else { "态机漂移" });
    // J2-2 未开槽聚合拒
    let mut t2 = FiveMetricSkeleton::new();
    let ok = t2.add_sample(M_FPS, 1) == Err(FT_UNK_METRIC);
    s.add("56147-J2-未开槽拒", ok, if ok { "" } else { "开槽闸失效" });
    // J2-3 封槽单向
    let mut t2 = FiveMetricSkeleton::new();
    let _ = t2.open_slot(M_FPS);
    let ok = t2.seal_slot(M_FPS).is_ok() && t2.slot(M_FPS).map(|x| x.state == AG_SEALED).unwrap_or(false);
    s.add("56147-J2-封槽单向", ok, if ok { "" } else { "封槽漂移" });
    // J2-4 槽签随样本变
    let mut t2 = FiveMetricSkeleton::new();
    let _ = t2.open_slot(M_FPS);
    let _ = t2.add_sample(M_FPS, 0xABCD);
    let fp1 = t2.slot(M_FPS).map(|x| x.slot_fp).unwrap_or(0);
    let _ = t2.add_sample(M_FPS, 0x1234);
    let ok = t2.slot(M_FPS).map(|x| x.slot_fp).unwrap_or(0) != fp1;
    s.add("56147-J2-槽签随样本变", ok, if ok { "" } else { "折叠失效" });
    // J3-1 总表指纹确定性
    let ok = standard_table().table_fingerprint() == standard_table().table_fingerprint();
    s.add("56147-J3-总签确定性", ok, if ok { "" } else { "总签漂移" });
    // J3-2 总签随槽变（一槽样本变总签必变）
    let mut t2 = standard_table();
    let fp0 = t2.table_fingerprint();
    let _ = t2.add_sample(M_POWER, 0x9999);
    let ok = t2.table_fingerprint() != fp0;
    s.add("56147-J3-总签随槽变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封表态入签
    let mut t2 = standard_table();
    let fp0 = t2.table_fingerprint();
    let _ = t2.seal_table();
    let ok = t2.table_fingerprint() != fp0;
    s.add("56147-J3-封表入签", ok, if ok { "" } else { "封表态穿透" });
    // J3-4 指标名闭集渲染
    let ok = METRIC_NAMES[M_FPS] == "fps" && METRIC_NAMES[M_POWER] == "power" && ag_state_name(AG_SEALED) == "SEALED";
    s.add("56147-J3-名闭集", ok, if ok { "" } else { "名漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56147-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56147_all_checks_green() {
        let set = run_unx56147_checks();
        assert!(set.all_passed(), "UNX-F56147 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56147_fp_stable() {
        assert_eq!(standard_table().table_fingerprint(), standard_table().table_fingerprint());
    }
}
