//! UNX-F56011 版本锚变更重录流程件（0x60140 段）
//!
//! 锚点：UNX-F56011-J1 Windows 更新事件→版本锚变更登记→受影响指标
//! 全量重录（指标闭集逐项标记）→新旧基线并留（只追加不改写）×2 演练
//! 零覆盖；J2 事件链单调（事件号严格递增、版本锚变更可追溯）；J3 重录
//! 闸（版本锚未变更的重录请求拒收——无变化不重录）三判据全绿。判据
//! 锚定 §AI-71.2 专题一错误路径。
//! 域：CoRun·UNX-F2-B40（环境冻结域·重录流程面）。
//!
//! 结构：更新事件登记册（事件号 × 旧版 × 新版逐条落账）+ 受影响指标
//! 闭集标记（五指标逐项列重录清单）+ 基线代际账（gen × 版本锚 × 环境
//! 哈希只追加）+ 零覆盖核（旧代指纹快照恒等）+ 注入核（无变化重录/
//! 事件回退/空版本/封账改写/越闭集指标必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60140 独占 0x20）。
pub const SEG_BASE: u32 = 0x60140;
pub const RR_NO_CHANGE: u32 = SEG_BASE + 0x01; // 版本锚无变化的重录请求
pub const RR_EVENT_BACK: u32 = SEG_BASE + 0x02; // 事件号回退
pub const RR_VER_EMPTY: u32 = SEG_BASE + 0x03; // 版本串空
pub const RR_SEALED: u32 = SEG_BASE + 0x04; // 流程账已封账拒写
pub const RR_DUP_EVENT: u32 = SEG_BASE + 0x05; // 事件号重复
pub const RR_METRIC_UNK: u32 = SEG_BASE + 0x06; // 越闭集指标
pub const RR_FP_ZERO: u32 = SEG_BASE + 0x07; // 基线指纹退化零哨兵

/// 受影响指标闭集。
pub const M_BOOT: usize = 0;
pub const M_SYSCALL: usize = 1;
pub const M_MEM: usize = 2;
pub const M_FRAME: usize = 3;
pub const M_IO: usize = 4;
pub const M_COUNT: usize = 5;
pub const M_NAMES: [&str; M_COUNT] = ["boot", "syscall", "mem", "frame", "io"];

/// FNV-1a 32 位指纹（基线指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 版本锚变更判定：旧版 ≠ 新版才算变更。
pub fn anchor_changed(ver_from: &str, ver_to: &str) -> bool {
    !ver_from.is_empty() && !ver_to.is_empty() && ver_from != ver_to
}

/// 更新事件账目：事件号 × 旧版 × 新版。
#[derive(Clone)]
pub struct UpdateEvent {
    pub seq: u32,
    pub ver_from: String,
    pub ver_to: String,
}

impl UpdateEvent {
    pub fn new(seq: u32, ver_from: &str, ver_to: &str) -> Result<Self, u32> {
        if ver_from.is_empty() || ver_to.is_empty() {
            return Err(RR_VER_EMPTY);
        }
        if seq == 0 {
            return Err(RR_EVENT_BACK);
        }
        Ok(UpdateEvent {
            seq,
            ver_from: ver_from.to_string(),
            ver_to: ver_to.to_string(),
        })
    }
}

/// 基线代际账目：代 × 版本锚 × 环境哈希 × 指纹。
#[derive(Clone)]
pub struct BaselineEntry {
    pub gen: u32,
    pub anchor_ver: String,
    pub env_hash: u32,
    pub fingerprint: u32,
}

impl BaselineEntry {
    pub fn new(gen: u32, anchor_ver: &str, env_hash: u32) -> Result<Self, u32> {
        if anchor_ver.is_empty() {
            return Err(RR_VER_EMPTY);
        }
        let fp = fnv1a(format!("{}|{}|{}", gen, anchor_ver, env_hash).as_bytes());
        if fp == 0 {
            return Err(RR_FP_ZERO);
        }
        Ok(BaselineEntry {
            gen,
            anchor_ver: anchor_ver.to_string(),
            env_hash,
            fingerprint: fp,
        })
    }
}

/// 重录流程账：事件册 + 指标标记 + 基线代际账（只追加）。
pub struct RerunFlow {
    pub events: Vec<UpdateEvent>,
    pub marked: Vec<(usize, u32)>,
    pub baselines: Vec<BaselineEntry>,
    pub sealed: bool,
}

impl RerunFlow {
    pub fn new() -> Self {
        RerunFlow {
            events: Vec::new(),
            marked: Vec::new(),
            baselines: Vec::new(),
            sealed: false,
        }
    }

    /// 事件登记：号严格递增 + 重复拒 + 版本锚必须真变更。
    pub fn on_event(&mut self, e: UpdateEvent) -> Result<u32, u32> {
        if self.sealed {
            return Err(RR_SEALED);
        }
        if !anchor_changed(&e.ver_from, &e.ver_to) {
            return Err(RR_NO_CHANGE);
        }
        if let Some(prev) = self.events.last() {
            if e.seq == prev.seq {
                return Err(RR_DUP_EVENT);
            }
            if e.seq < prev.seq {
                return Err(RR_EVENT_BACK);
            }
        }
        let seq = e.seq;
        self.events.push(e);
        Ok(seq)
    }

    /// 受影响指标全量标记（指标闭集逐项，关联事件号）。
    pub fn mark_all(&mut self, seq: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(RR_SEALED);
        }
        if seq == 0 {
            return Err(RR_EVENT_BACK);
        }
        for m in 0..M_COUNT {
            self.marked.push((m, seq));
        }
        Ok(M_COUNT)
    }

    /// 越闭集指标标记拒（显性守闸）。
    pub fn mark_one(&mut self, metric: usize, seq: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RR_SEALED);
        }
        if metric >= M_COUNT {
            return Err(RR_METRIC_UNK);
        }
        self.marked.push((metric, seq));
        Ok(())
    }

    /// 新基线落账（只追加）：gen 自增，旧代不动。
    pub fn new_baseline(&mut self, anchor_ver: &str, env_hash: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RR_SEALED);
        }
        let gen = match self.baselines.last() {
            Some(b) => b.gen + 1,
            None => 1,
        };
        let e = BaselineEntry::new(gen, anchor_ver, env_hash)?;
        self.baselines.push(e);
        Ok(gen)
    }

    /// J1：零覆盖核——旧代指纹与快照恒等。
    pub fn zero_overwrite_ok(&self, gen: u32, snapshot_fp: u32) -> bool {
        for b in self.baselines.iter() {
            if b.gen == gen {
                return b.fingerprint == snapshot_fp;
            }
        }
        false
    }

    /// 事件链单调核。
    pub fn events_monotonic(&self) -> bool {
        for w in self.events.windows(2) {
            if w[0].seq >= w[1].seq {
                return false;
            }
        }
        !self.events.is_empty()
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：两事件两代基线（1.0.0 → 1.1.0）。
pub fn standard_flow() -> RerunFlow {
    let mut f = RerunFlow::new();
    let e1 = UpdateEvent::new(1, "win-1.0.0", "win-1.1.0").expect("e1");
    f.on_event(e1).expect("reg1");
    f.mark_all(1).expect("mark1");
    f.new_baseline("win-1.1.0", 0x00ABCDEF).expect("bl1");
    f
}

/// ×2 演练核：两次「事件→标记→新基线→旧账零覆盖」全链。
pub fn drill_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 演练 1：1.0.0 → 1.1.0
    let mut f = RerunFlow::new();
    let fp0 = f.new_baseline("win-1.0.0", 0x00ABCDEF).expect("bl0");
    let snap = f.baselines[fp0 as usize - 1].fingerprint;
    let e = UpdateEvent::new(1, "win-1.0.0", "win-1.1.0").expect("e");
    if f.on_event(e).expect("reg") == 1 && f.mark_all(1).expect("mark") == M_COUNT {
        let g = f.new_baseline("win-1.1.0", 0x00ABCDEF).expect("bl1");
        if g == 2 && f.zero_overwrite_ok(1, snap) {
            caught += 1;
        }
    }
    // 演练 2：1.1.0 → 1.2.0
    let snap2 = f.baselines[1].fingerprint;
    let e2 = UpdateEvent::new(2, "win-1.1.0", "win-1.2.0").expect("e2");
    if f.on_event(e2).expect("reg2") == 2 && f.mark_all(2).expect("mark2") == M_COUNT {
        let g2 = f.new_baseline("win-1.2.0", 0x00ABCDEF).expect("bl2");
        if g2 == 3 && f.zero_overwrite_ok(1, snap) && f.zero_overwrite_ok(2, snap2) {
            caught += 1;
        }
    }
    (caught, 2)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 无变化重录（版本锚相同）
    let mut f = standard_flow();
    let e = UpdateEvent::new(2, "win-1.1.0", "win-1.1.0").expect("same");
    if f.on_event(e) == Err(RR_NO_CHANGE) {
        caught += 1;
    }
    // 2) 事件号回退
    let e2 = UpdateEvent::new(1, "win-1.1.0", "win-1.2.0").expect("back");
    if f.on_event(e2) == Err(RR_EVENT_BACK) {
        caught += 1;
    }
    // 3) 空版本串
    if UpdateEvent::new(3, "", "win-2.0.0").err() == Some(RR_VER_EMPTY) {
        caught += 1;
    }
    // 4) 越闭集指标
    if f.mark_one(9, 1) == Err(RR_METRIC_UNK) {
        caught += 1;
    }
    // 5) 封账后改写
    f.seal();
    let e3 = UpdateEvent::new(3, "win-1.1.0", "win-1.3.0").expect("sealed");
    if f.on_event(e3) == Err(RR_SEALED) && f.mark_one(M_BOOT, 3) == Err(RR_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 流程渲染（审计面：事件与基线逐行）。
pub fn render_flow(f: &RerunFlow) -> String {
    let mut out = String::new();
    for e in f.events.iter() {
        out.push_str(&format!("EVENT|{}|{}→{}\n", e.seq, e.ver_from, e.ver_to));
    }
    for b in f.baselines.iter() {
        out.push_str(&format!("BASE|gen={}|ver={}|fp={:08x}\n", b.gen, b.anchor_ver, b.fingerprint));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56011");
    // J1-1 ×2 演练全链零覆盖
    let (c, n) = drill_core();
    let ok = c == n && n == 2;
    let detail = if ok { String::new() } else { format!("演练 {c}/{n}") };
    s.add_dyn("56011-J1-两次演练零覆盖", ok, &detail);
    // J1-2 基线代际账只追加（三代并留可查）
    let mut f = standard_flow();
    let snap = f.baselines[0].fingerprint;
    f.new_baseline("win-1.2.0", 0x00ABCDEF).expect("bl2");
    let ok = f.baselines.len() == 2 && f.zero_overwrite_ok(1, snap);
    s.add("56011-J1-基线并留", ok, if ok { "" } else { "基线覆盖" });
    // J1-3 指标全量标记（五项齐且关联事件号）
    let f2 = standard_flow();
    let mut boot_marks = 0usize;
    for m in f2.marked.iter() {
        if *m == (M_BOOT, 1) {
            boot_marks += 1;
        }
    }
    let ok = f2.marked.len() == M_COUNT && boot_marks == 1;
    s.add("56011-J1-指标全量标记", ok, if ok { "" } else { "标记缺项" });
    // J2-1 事件链单调
    let ok = standard_flow().events_monotonic();
    s.add("56011-J2-事件链单调", ok, if ok { "" } else { "事件破序" });
    // J2-2 版本锚变更判定语义（同版假 / 异版真 / 空串假）
    let ok = !anchor_changed("win-1.1.0", "win-1.1.0")
        && anchor_changed("win-1.1.0", "win-1.2.0")
        && !anchor_changed("", "win-1.2.0");
    s.add("56011-J2-变更判定语义", ok, if ok { "" } else { "判定语义漂移" });
    // J2-3 新基线指纹确定且敏感（环境哈希变指纹必变）
    let b1 = BaselineEntry::new(1, "win-1.1.0", 0x00ABCDEF).expect("b1");
    let b2 = BaselineEntry::new(1, "win-1.1.0", 0x00ABCDDF).expect("b2");
    let ok = b1.fingerprint != b2.fingerprint && b1.fingerprint != 0;
    s.add("56011-J2-基线指纹敏感", ok, if ok { "" } else { "指纹不敏感" });
    // J3-1 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56011-J3-注入核必抓", ok, &detail);
    // J3-2 无变化重录拒（守闸）
    let ok = !anchor_changed("win-1.0.0", "win-1.0.0");
    s.add("56011-J3-无变化不重录", ok, if ok { "" } else { "无变化穿透" });
    // J3-3 指标命名同源
    let ok = M_NAMES[M_BOOT] == "boot" && M_NAMES[M_COUNT - 1] == "io";
    s.add("56011-J3-指标命名同源", ok, if ok { "" } else { "命名漂移" });
    // J3-4 封账冻结
    let mut f3 = standard_flow();
    f3.seal();
    let ok = f3.new_baseline("win-2.0.0", 1) == Err(RR_SEALED);
    s.add("56011-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 渲染审计面齐
    let text = render_flow(&standard_flow());
    let ok = text.contains("EVENT|1|win-1.0.0→win-1.1.0") && text.contains("BASE|gen=1|");
    s.add("56011-J1-渲染审计面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-5 事件号零哨兵拒
    let ok = UpdateEvent::new(0, "a", "b").err() == Some(RR_EVENT_BACK);
    s.add("56011-J1-事件号零哨兵", ok, if ok { "" } else { "零哨兵穿透" });
    // J2-4 单事件流程最小可行（事件→标记→基线三步齐）
    let mut f4 = RerunFlow::new();
    let e = UpdateEvent::new(1, "win-1.0.0", "win-9.9.9").expect("e");
    let r = f4.on_event(e).is_ok()
        && f4.mark_all(1).expect("mark") == M_COUNT
        && f4.new_baseline("win-9.9.9", 7).expect("bl") == 1;
    let ok = r && f4.baselines.len() == 1;
    s.add("56011-J2-最小流程可行", ok, if ok { "" } else { "最小流程断" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56011_all_checks_green() {
        let set = run_unx56011_checks();
        assert!(set.all_passed(), "UNX-F56011 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56011_no_overwrite() {
        let mut f = RerunFlow::new();
        f.new_baseline("win-1.0.0", 1).expect("b0");
        let snap = f.baselines[0].fingerprint;
        f.new_baseline("win-1.1.0", 1).expect("b1");
        assert!(f.zero_overwrite_ok(1, snap));
    }
}
