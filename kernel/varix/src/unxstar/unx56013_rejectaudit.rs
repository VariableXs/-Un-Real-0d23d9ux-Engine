//! UNX-F56013 环境漂移拒收审计记录器（0x60180 段）
//!
//! 锚点：UNX-F56013-J1 全部拒收事件（哈希不符/白名单外进程/频率漂移/
//! 拓扑变更）统一落审计账（逐条含日序/原因/哈希双值/处置四元组）；
//! J2 处置闭集（重录/拒收/复测三选一，禁泛化处置）；J3 拒收率月度可
//! 导出（按日序分段计数 + 万分比口径）三判据全绿。判据锚定 §AI-71.2
//! 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·拒收审计面）。
//!
//! 结构：统一审计账（逐拒收事件追加落账，序号单调）+ 原因闭集四类 +
//! 处置闭集三类 + 拒收率导出核（窗口内计数 ÷ 总窗万分比）+ 机器可读
//! 渲染面 + 注入核（越闭集原因/越闭集处置/哈希缺值/序号回退/封账改写
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60180 独占 0x20）。
pub const SEG_BASE: u32 = 0x60180;
pub const AU_CAUSE_UNK: u32 = SEG_BASE + 0x01; // 越闭集拒收原因
pub const AU_DISP_UNK: u32 = SEG_BASE + 0x02; // 越闭集处置
pub const AU_HASH_MISS: u32 = SEG_BASE + 0x03; // 哈希双值缺一
pub const AU_SEQ_BACK: u32 = SEG_BASE + 0x04; // 序号回退
pub const AU_SEALED: u32 = SEG_BASE + 0x05; // 审计账已封账拒追加
pub const AU_DUP_SEQ: u32 = SEG_BASE + 0x06; // 序号重复
pub const AU_DAY_ZERO: u32 = SEG_BASE + 0x07; // 日序零哨兵

/// 拒收原因闭集。
pub const C_HASH: usize = 0;
pub const C_PROC: usize = 1;
pub const C_FREQ: usize = 2;
pub const C_TOPO: usize = 3;
pub const C_COUNT: usize = 4;
pub const C_NAMES: [&str; C_COUNT] = ["hash_miss", "proc_outside", "freq_drift", "topo_change"];

/// 处置闭集。
pub const D_RERECORD: usize = 0;
pub const D_REJECT: usize = 1;
pub const D_RESAMPLE: usize = 2;
pub const D_COUNT: usize = 3;
pub const D_NAMES: [&str; D_COUNT] = ["rerecord", "reject", "resample"];

/// 原因/处置闭集核。
pub fn cause_ok(c: usize) -> bool {
    c < C_COUNT
}
pub fn disp_ok(d: usize) -> bool {
    d < D_COUNT
}

/// 审计条目：序号 × 日序 × 原因 × 哈希双值 × 处置。
#[derive(Clone)]
pub struct AuditEntry {
    pub seq: u32,
    pub day: u32,
    pub cause: usize,
    pub hash_old: u32,
    pub hash_new: u32,
    pub disp: usize,
}

impl AuditEntry {
    pub fn new(seq: u32, day: u32, cause: usize, hash_old: u32, hash_new: u32, disp: usize) -> Result<Self, u32> {
        if !cause_ok(cause) {
            return Err(AU_CAUSE_UNK);
        }
        if !disp_ok(disp) {
            return Err(AU_DISP_UNK);
        }
        if hash_old == 0 || hash_new == 0 {
            return Err(AU_HASH_MISS);
        }
        if day == 0 {
            return Err(AU_DAY_ZERO);
        }
        if seq == 0 {
            return Err(AU_SEQ_BACK);
        }
        Ok(AuditEntry { seq, day, cause, hash_old, hash_new, disp })
    }

    /// 机器可读行（AUDIT|序|日|原因|双哈希|处置 六段）。
    pub fn render(&self) -> String {
        format!(
            "AUDIT|{}|d{}|{}|{:08x}/{:08x}|{}",
            self.seq, self.day, C_NAMES[self.cause], self.hash_old, self.hash_new, D_NAMES[self.disp]
        )
    }
}

/// 统一审计账：只追加 + 按窗口导出拒收率。
pub struct AuditBook {
    pub entries: Vec<AuditEntry>,
    pub sealed: bool,
}

impl AuditBook {
    pub fn new() -> Self {
        AuditBook { entries: Vec::new(), sealed: false }
    }

    /// 追加一条：序号严格递增 + 重复拒。
    pub fn append(&mut self, e: AuditEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(AU_SEALED);
        }
        if let Some(prev) = self.entries.last() {
            if e.seq == prev.seq {
                return Err(AU_DUP_SEQ);
            }
            if e.seq < prev.seq {
                return Err(AU_SEQ_BACK);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 某日序窗内拒收计数（day_from..=day_to 闭区间）。
    pub fn count_in_window(&self, day_from: u32, day_to: u32) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.day >= day_from && e.day <= day_to {
                n += 1;
            }
        }
        n
    }

    /// J3：拒收率导出（窗口计数 × 10000 / 全账计数；全账零则 0）。
    pub fn reject_rate_pbp(&self, day_from: u32, day_to: u32) -> u32 {
        let total = self.entries.len();
        if total == 0 {
            return 0;
        }
        let w = self.count_in_window(day_from, day_to);
        ((w as u64 * 10_000) / total as u64) as u32
    }

    /// 按原因分类计数。
    pub fn count_by_cause(&self, cause: usize) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if e.cause == cause {
                n += 1;
            }
        }
        n
    }

    /// J1：四类原因皆可归账（每类至少一透传计数接口即可用）。
    pub fn cause_breakdown(&self) -> [usize; C_COUNT] {
        let mut out = [0usize; C_COUNT];
        for e in self.entries.iter() {
            if cause_ok(e.cause) {
                out[e.cause] += 1;
            }
        }
        out
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：8 条四类拒收（day 1..8，哈希双值固定）。
pub fn standard_audit() -> AuditBook {
    let mut b = AuditBook::new();
    let rows = [
        (1u32, 1u32, C_HASH, D_RERECORD),
        (2, 2, C_PROC, D_REJECT),
        (3, 3, C_FREQ, D_RESAMPLE),
        (4, 4, C_TOPO, D_RERECORD),
        (5, 5, C_HASH, D_REJECT),
        (6, 6, C_PROC, D_RESAMPLE),
        (7, 7, C_FREQ, D_RERECORD),
        (8, 8, C_TOPO, D_REJECT),
    ];
    for r in rows.iter() {
        b.append(AuditEntry::new(r.0, r.1, r.2, 0x00ABCDEF, 0x00ABCDF0, r.3).expect("fixture"))
            .expect("append");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集原因（cause=9）
    if AuditEntry::new(1, 1, 9, 1, 2, D_REJECT).err() == Some(AU_CAUSE_UNK) {
        caught += 1;
    }
    // 2) 越闭集处置（disp=7）
    if AuditEntry::new(1, 1, C_HASH, 1, 2, 7).err() == Some(AU_DISP_UNK) {
        caught += 1;
    }
    // 3) 哈希缺值（old=0）
    if AuditEntry::new(1, 1, C_HASH, 0, 2, D_REJECT).err() == Some(AU_HASH_MISS) {
        caught += 1;
    }
    // 4) 序号回退
    let mut b = standard_audit();
    let e = AuditEntry::new(3, 9, C_HASH, 1, 2, D_REJECT).expect("back");
    if b.append(e) == Err(AU_SEQ_BACK) {
        caught += 1;
    }
    // 5) 封账后追加
    let mut b2 = standard_audit();
    b2.seal();
    let e2 = AuditEntry::new(9, 9, C_HASH, 1, 2, D_REJECT).expect("sealed");
    if b2.append(e2) == Err(AU_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 审计渲染（机器可读面，逐条一行）。
pub fn render_audit(b: &AuditBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(e.render().as_str());
        out.push('\n');
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56013");
    // J1-1 四类拒收统一落账（各类 ≥2 条）
    let b = standard_audit();
    let bd = b.cause_breakdown();
    let ok = bd[C_HASH] == 2 && bd[C_PROC] == 2 && bd[C_FREQ] == 2 && bd[C_TOPO] == 2;
    s.add("56013-J1-四类统一落账", ok, if ok { "" } else { "分类计数漂移" });
    // J1-2 条目四元组齐（机读行六段）
    let line = b.entries[0].render();
    let ok = line.split('|').count() == 6 && line.starts_with("AUDIT|1|d1|hash_miss|");
    s.add("56013-J1-条目四元组齐", ok, if ok { "" } else { "条目字段缺" });
    // J1-3 哈希双值必落（旧新皆非零）
    let ok = b.entries.iter().all(|e| e.hash_old != 0 && e.hash_new != 0);
    s.add("56013-J1-哈希双值必落", ok, if ok { "" } else { "哈希缺值" });
    // J2-1 处置闭集三选一（三类计数皆 ≥2）
    let mut dr = 0usize;
    let mut dj = 0usize;
    let mut ds = 0usize;
    for e in b.entries.iter() {
        if e.disp == D_RERECORD {
            dr += 1;
        }
        if e.disp == D_REJECT {
            dj += 1;
        }
        if e.disp == D_RESAMPLE {
            ds += 1;
        }
    }
    let ok = dr == 3 && dj == 3 && ds == 2;
    s.add("56013-J2-处置闭集", ok, if ok { "" } else { "处置泛化" });
    // J2-2 闭集核函数语义
    let ok = cause_ok(C_TOPO) && !cause_ok(9) && disp_ok(D_RESAMPLE) && !disp_ok(3);
    s.add("56013-J2-闭集核语义", ok, if ok { "" } else { "闭集核松" });
    // J3-1 拒收率导出语义（后 4 条 / 全 8 条 = 5000 万分比）
    let ok = b.reject_rate_pbp(5, 8) == 5_000 && b.reject_rate_pbp(1, 4) == 5_000;
    s.add("56013-J3-拒收率导出", ok, if ok { "" } else { "拒收率算式漂移" });
    // J3-2 窗口计数语义（day 2..3 恰 2 条）
    let ok = b.count_in_window(2, 3) == 2 && b.count_in_window(9, 12) == 0;
    s.add("56013-J3-窗口计数", ok, if ok { "" } else { "窗口计数漂移" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56013-J3-注入核必抓", ok, &detail);
    // J1-4 序号单调（8 条 1..8）
    let mut mono = true;
    for w in b.entries.windows(2) {
        if w[0].seq >= w[1].seq {
            mono = false;
        }
    }
    let ok = mono && b.entries.len() == 8;
    s.add("56013-J1-序号单调", ok, if ok { "" } else { "序号破序" });
    // J2-3 渲染面行数与条数一致
    let text = render_audit(&b);
    let ok = text.lines().count() == b.entries.len() && text.contains("AUDIT|8|d8|topo_change|");
    s.add("56013-J2-渲染面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-4 封账冻结
    let mut b2 = standard_audit();
    b2.seal();
    let ok = b2.append(AuditEntry::new(9, 9, C_HASH, 1, 2, D_REJECT).expect("s")) == Err(AU_SEALED);
    s.add("56013-J2-封账冻结", ok, if ok { "" } else { "封账后可追加" });
    // J1-5 日序零哨兵拒
    let ok = AuditEntry::new(1, 0, C_HASH, 1, 2, D_REJECT).err() == Some(AU_DAY_ZERO);
    s.add("56013-J1-日序零哨兵", ok, if ok { "" } else { "零哨兵穿透" });
    // J3-4 空账拒收率零（不除零）
    let empty = AuditBook::new();
    let ok = empty.reject_rate_pbp(1, 30) == 0 && empty.cause_breakdown() == [0, 0, 0, 0];
    s.add("56013-J3-空账零导出", ok, if ok { "" } else { "空账除零" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56013_all_checks_green() {
        let set = run_unx56013_checks();
        assert!(set.all_passed(), "UNX-F56013 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56013_rate_math() {
        let b = standard_audit();
        assert_eq!(b.reject_rate_pbp(5, 8), 5_000);
    }
}
