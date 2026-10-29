//! UNX-F56017 双系统语义等价声明册（0x60200 段）
//!
//! 锚点：UNX-F56017-J1 五指标在双系统上的语义等价映射（syscall 族↔
//! NT API 族/启动五段↔Windows 启动段/RSS↔工作集/IO 负载同脚本/帧账↔
//! PresentMon 类口径）逐项声明并落 L1–L4 对照等级；J2 等级闭集语义
//! （L1 强等价 → L4 弱声明，逐级递弱可判）；J3 册封账冻结（mini-ADR
//! 口径：封后声明不可改，改级必须新册）三判据全绿。判据锚定 §AI-71.2
//! 专题三。
//! 域：CoRun·UNX-F2-B40（环境冻结域·等价声明面）。
//!
//! 结构：五指标闭集登记册（Varix 侧引用 × Windows 侧引用 × 等级三元
//! 组逐项落账）+ 等级闭集核（L1..L4 判定函数）+ 逐项查询核 + 册指纹
//! （全声明折叠，封账材料）+ 注入核（越闭集指标/越闭集等级/空引用/
//! 封账改写/重复声明必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60200 独占 0x20）。
pub const SEG_BASE: u32 = 0x60200;
pub const EQ_METRIC_UNK: u32 = SEG_BASE + 0x01; // 越闭集指标
pub const EQ_LEVEL_UNK: u32 = SEG_BASE + 0x02; // 越闭集等级
pub const EQ_REF_EMPTY: u32 = SEG_BASE + 0x03; // 引用串空
pub const EQ_SEALED: u32 = SEG_BASE + 0x04; // 册已封账拒改
pub const EQ_DUP_METRIC: u32 = SEG_BASE + 0x05; // 同指标重复声明
pub const EQ_FP_ZERO: u32 = SEG_BASE + 0x06; // 册指纹退化零哨兵
pub const EQ_WEAK_L4: u32 = SEG_BASE + 0x07; // L4 弱声明需附注（附注空即拒）

/// 五指标闭集。
pub const MT_SYSCALL: usize = 0;
pub const MT_BOOT: usize = 1;
pub const MT_MEM: usize = 2;
pub const MT_IO: usize = 3;
pub const MT_FRAME: usize = 4;
pub const MT_COUNT: usize = 5;
pub const MT_NAMES: [&str; MT_COUNT] = ["syscall", "boot", "mem", "io", "frame"];

/// Varix 侧引用（与指标同序）。
pub const VARIX_REFS: [&str; MT_COUNT] = [
    "getpid/read/write/mmap 族",
    "启动五段（引导→内核首任务→登录面→桌面首帧→双击首响应）",
    "RSS 类（常驻集+峰值）",
    "标准内存/IO 负载脚本",
    "帧账（合成器首帧口径）",
];

/// Windows 侧引用（与指标同序，语义等价对侧）。
pub const WIN_REFS: [&str; MT_COUNT] = [
    "NtQueryInformationProcess 等 NT API 族",
    "POST→bootmgr→内核→登录→桌面就绪→交互就绪",
    "Working Set（工作集）",
    "同码负载脚本（跨系统同源）",
    "PresentMon 类 ETW 口径",
];

/// 对照等级闭集（L1 强等价 → L4 弱声明）。
pub const LV_L1: usize = 0;
pub const LV_L2: usize = 1;
pub const LV_L3: usize = 2;
pub const LV_L4: usize = 3;
pub const LV_COUNT: usize = 4;
pub const LV_NAMES: [&str; LV_COUNT] = ["L1", "L2", "L3", "L4"];

/// 标准五指标等级（syscall L1 / boot L2 / mem L2 / io L1 / frame L3）。
pub const STANDARD_LEVELS: [usize; MT_COUNT] = [LV_L1, LV_L2, LV_L2, LV_L1, LV_L3];

/// 等级闭集核。
pub fn level_ok(l: usize) -> bool {
    l < LV_COUNT
}

/// 等级语义：数值越小越强等价。
pub fn level_stronger(a: usize, b: usize) -> bool {
    level_ok(a) && level_ok(b) && a < b
}

/// FNV-1a 32 位（册指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 单声明：指标 × 两侧引用 × 等级 × 附注。
#[derive(Clone)]
pub struct DeclEntry {
    pub metric: usize,
    pub varix_ref: String,
    pub win_ref: String,
    pub level: usize,
    pub note: String,
}

impl DeclEntry {
    pub fn new(metric: usize, varix_ref: &str, win_ref: &str, level: usize, note: &str) -> Result<Self, u32> {
        if metric >= MT_COUNT {
            return Err(EQ_METRIC_UNK);
        }
        if !level_ok(level) {
            return Err(EQ_LEVEL_UNK);
        }
        if varix_ref.is_empty() || win_ref.is_empty() {
            return Err(EQ_REF_EMPTY);
        }
        if level == LV_L4 && note.is_empty() {
            return Err(EQ_WEAK_L4);
        }
        Ok(DeclEntry {
            metric,
            varix_ref: varix_ref.to_string(),
            win_ref: win_ref.to_string(),
            level,
            note: note.to_string(),
        })
    }
}

/// 声明册：五指标逐项落账 + 册指纹 + 封账。
pub struct DeclBook {
    pub entries: [Option<DeclEntry>; MT_COUNT],
    pub sealed: bool,
}

impl DeclBook {
    pub fn new() -> Self {
        DeclBook { entries: [None, None, None, None, None], sealed: false }
    }

    /// 逐项声明：同指标重复拒 + 封账拒改。
    pub fn declare(&mut self, e: DeclEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(EQ_SEALED);
        }
        if e.metric >= MT_COUNT {
            return Err(EQ_METRIC_UNK);
        }
        if self.entries[e.metric].is_some() {
            return Err(EQ_DUP_METRIC);
        }
        let metric = e.metric;
        self.entries[metric] = Some(e);
        Ok(())
    }

    /// 指标查询。
    pub fn get(&self, metric: usize) -> Option<&DeclEntry> {
        if metric >= MT_COUNT {
            return None;
        }
        self.entries[metric].as_ref()
    }

    /// J1：五项全声明。
    pub fn full(&self) -> bool {
        for e in self.entries.iter() {
            if e.is_none() {
                return false;
            }
        }
        true
    }

    /// J2：等级账与标准等级一致。
    pub fn levels_match_standard(&self) -> bool {
        for m in 0..MT_COUNT {
            match self.get(m) {
                None => return false,
                Some(e) => {
                    if e.level != STANDARD_LEVELS[m] {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 册指纹（全声明折叠）。
    pub fn book_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for m in 0..MT_COUNT {
            if let Some(e) = self.get(m) {
                let line = format!(
                    "{}|{}|{}|{}",
                    MT_NAMES[m], e.varix_ref, e.win_ref, e.level
                );
                for b in line.as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(16777619);
                }
            }
        }
        h
    }

    /// 封账（mini-ADR 口径：封后声明不可改）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：五指标按标准等级全声明（L4 附注空位由 L3 承担）。
pub fn standard_decl() -> DeclBook {
    let mut b = DeclBook::new();
    for m in 0..MT_COUNT {
        let e = DeclEntry::new(m, VARIX_REFS[m], WIN_REFS[m], STANDARD_LEVELS[m], "")
            .expect("fixture decl");
        b.declare(e).expect("fixture decl reg");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集指标（metric=9）
    if DeclEntry::new(9, "a", "b", LV_L1, "").err() == Some(EQ_METRIC_UNK) {
        caught += 1;
    }
    // 2) 越闭集等级（level=7）
    if DeclEntry::new(MT_IO, "a", "b", 7, "").err() == Some(EQ_LEVEL_UNK) {
        caught += 1;
    }
    // 3) 空引用
    if DeclEntry::new(MT_IO, "", "b", LV_L1, "").err() == Some(EQ_REF_EMPTY) {
        caught += 1;
    }
    // 4) L4 无附注
    if DeclEntry::new(MT_IO, "a", "b", LV_L4, "").err() == Some(EQ_WEAK_L4) {
        caught += 1;
    }
    // 5) 封账改写 + 重复声明
    let mut b = standard_decl();
    let dup = DeclEntry::new(MT_IO, "x", "y", LV_L1, "").expect("dup");
    if b.declare(dup) == Err(EQ_DUP_METRIC) {
        caught += 1;
    }
    b.seal();
    let e = DeclEntry::new(MT_BOOT, "x2", "y2", LV_L2, "").expect("sealed");
    if b.declare(e) == Err(EQ_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 声明册渲染（对照面：逐项一行）。
pub fn render_decl(b: &DeclBook) -> String {
    let mut out = String::new();
    for m in 0..MT_COUNT {
        match b.get(m) {
            Some(e) => {
                out.push_str(&format!(
                    "{}|{}|{}|{}\n",
                    MT_NAMES[m], e.varix_ref, e.win_ref, LV_NAMES[e.level]
                ));
            }
            None => {
                out.push_str(&format!("{}|UNDECLARED\n", MT_NAMES[m]));
            }
        }
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56017");
    // J1-1 五项全声明
    let b = standard_decl();
    let ok = b.full();
    s.add("56017-J1-五项全声明", ok, if ok { "" } else { "声明缺项" });
    // J1-2 等级账与标准一致（syscall L1 / boot L2 / mem L2 / io L1 / frame L3）
    let ok = b.levels_match_standard();
    s.add("56017-J1-等级账标准一致", ok, if ok { "" } else { "等级漂移" });
    // J1-3 两侧引用闭集同源（syscall 对 NT API / mem 对 Working Set）
    let sc = b.get(MT_SYSCALL).expect("sc");
    let mm = b.get(MT_MEM).expect("mm");
    let ok = sc.win_ref.contains("NT API") && mm.win_ref.contains("Working Set");
    s.add("56017-J1-两侧引用同源", ok, if ok { "" } else { "引用漂移" });
    // J2-1 等级闭集与强弱语义
    let ok = level_ok(LV_L1) && !level_ok(4)
        && level_stronger(LV_L1, LV_L2)
        && level_stronger(LV_L2, LV_L3)
        && level_stronger(LV_L3, LV_L4)
        && !level_stronger(LV_L1, LV_L1);
    s.add("56017-J2-等级强弱语义", ok, if ok { "" } else { "强弱语义漂移" });
    // J2-2 等级名闭集
    let ok = LV_NAMES[0] == "L1" && LV_NAMES[3] == "L4";
    s.add("56017-J2-等级名闭集", ok, if ok { "" } else { "等级名漂移" });
    // J2-3 册指纹确定且敏感
    let fp1 = standard_decl().book_fingerprint();
    let fp2 = standard_decl().book_fingerprint();
    let mut b2 = standard_decl();
    b2.entries[MT_FRAME].as_mut().expect("f").level = LV_L4;
    let fp3 = b2.book_fingerprint();
    let ok = fp1 == fp2 && fp1 != fp3 && fp1 != 0;
    s.add("56017-J2-册指纹敏感", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56017-J3-注入核必抓", ok, &detail);
    // J3-2 封账冻结（mini-ADR 口径）
    let mut b3 = standard_decl();
    let fp_before = b3.book_fingerprint();
    b3.seal();
    let ok = b3.sealed
        && b3.declare(DeclEntry::new(MT_IO, "x", "y", LV_L1, "").expect("s")) == Err(EQ_SEALED)
        && b3.book_fingerprint() == fp_before;
    s.add("56017-J3-封账冻结", ok, if ok { "" } else { "封账后可改" });
    // J3-3 L4 附注闸（弱声明必须附注差异）
    let ok = DeclEntry::new(MT_FRAME, "a", "b", LV_L4, "共享页计数口径差异").is_ok();
    s.add("56017-J3-L4附注闸", ok, if ok { "" } else { "L4 附注闸松" });
    // J1-4 渲染对照面齐（五行无 UNDECLARED）
    let text = render_decl(&standard_decl());
    let ok = text.lines().count() == MT_COUNT
        && !text.contains("UNDECLARED")
        && text.contains("syscall|getpid/read/write/mmap 族|");
    s.add("56017-J1-渲染对照面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-5 未声明显性化
    let empty = DeclBook::new();
    let ok = render_decl(&empty).contains("syscall|UNDECLARED");
    s.add("56017-J1-未声明显性", ok, if ok { "" } else { "缺声明被吞" });
    // J2-4 指标名闭集同源
    let ok = MT_NAMES[MT_SYSCALL] == "syscall" && MT_NAMES[MT_FRAME] == "frame";
    s.add("56017-J2-指标名同源", ok, if ok { "" } else { "命名漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56017_all_checks_green() {
        let set = run_unx56017_checks();
        assert!(set.all_passed(), "UNX-F56017 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56017_levels() {
        assert!(level_stronger(LV_L1, LV_L4));
        assert!(!level_stronger(LV_L4, LV_L1));
    }
}
