//! UNX-F56198 M 型批推进体检件（B10 批位）（0x618C0 段）
//!
//! 锚点：UNX-F56198-J1 体检账（B10 批位 M 型批推进体检——体检项 ×
//! 项类别 × 通过态逐项落账）；J2 体检语义（项闭集全覆盖 + 类别闭集
//! 合法 + 未过项显式三性质）；J3 体检指纹（体检账折叠指纹）三判据全
//! 绿。
//! 域：CoRun·UNX-F2-B40（启动域·M 型批推进体检面）。
//!
//! 结构：体检类别闭集 + 体检项闭集 + 体检账 + 体检核（项闸 + 类别闸
//! + 显式闸）+ 体检指纹 + 注入核（未知项/未知类别/未显式未过/重体检
//! 项/印后体检必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x618C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x618C0;
pub const HB_UNK_ITEM: u32 = SEG_BASE + 0x01; // 未知体检项
pub const HB_UNK_KIND: u32 = SEG_BASE + 0x02; // 未知类别
pub const HB_INEXPLICIT: u32 = SEG_BASE + 0x03; // 未显式未过
pub const HB_DUP_ITEM: u32 = SEG_BASE + 0x04; // 重体检项
pub const HB_FROZEN: u32 = SEG_BASE + 0x05; // 印后体检

/// 体检类别闭集。
pub const HK_COMPILE: usize = 0; // 编译面
pub const HK_CHECKSET: usize = 1; // 判据面
pub const HK_SEAL: usize = 2; // 印记面
pub const HK_COUNT: usize = 3;

pub const KIND_NAMES: [&str; HK_COUNT] = ["compile", "checkset", "seal"];

/// B10 批位体检项闭集。
pub const HI_SEG_PROFILERS: usize = 0; // 四段子段剖析件齐
pub const HI_SUBDOMAP: usize = 1; // 子段-域映射齐
pub const HI_WINMAP: usize = 2; // Windows 侧映射齐
pub const HI_REPLAY: usize = 3; // 重放复测齐
pub const HI_LEDGER: usize = 4; // 总账组装齐
pub const HI_ARCHIVE: usize = 5; // 归档索引齐
pub const HI_COUNT: usize = 6;

pub const ITEM_NAMES: [&str; HI_COUNT] = [
    "seg-profilers", "subdomap", "winmap", "replay", "ledger", "archive",
];

/// 一条体检结果。
#[derive(Clone)]
pub struct HealthEntry {
    pub item: usize,
    pub kind: usize,
    pub passed: bool,
    pub note: String,
}

impl HealthEntry {
    /// 未过必须显式记 note。
    pub fn explicit_ok(&self) -> bool {
        self.passed || !self.note.is_empty()
    }

    fn render_line(&self) -> String {
        format!(
            "HI{}|{}|{}|{}",
            self.item,
            ITEM_NAMES[self.item],
            KIND_NAMES[self.kind],
            if self.passed { "PASS" } else { "FAIL" }
        )
    }
}

/// B10 批位体检账。
pub struct BatchHealth {
    entries: Vec<HealthEntry>,
    sealed: bool,
}

impl BatchHealth {
    pub fn new() -> Self {
        BatchHealth {
            entries: Vec::new(),
            sealed: false,
        }
    }

    /// 体检一项：项闸 + 类别闸 + 显式闸 + 重项拒 + 印后拒。
    pub fn examine(&mut self, e: HealthEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(HB_FROZEN);
        }
        if e.item >= HI_COUNT {
            return Err(HB_UNK_ITEM);
        }
        if e.kind >= HK_COUNT {
            return Err(HB_UNK_KIND);
        }
        if !e.explicit_ok() {
            return Err(HB_INEXPLICIT);
        }
        if self.entries.iter().any(|x| x.item == e.item) {
            return Err(HB_DUP_ITEM);
        }
        self.entries.push(e);
        Ok(())
    }

    pub fn entry(&self, item: usize) -> Option<&HealthEntry> {
        self.entries.iter().find(|x| x.item == item)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 覆盖核：体检项闭集全覆盖。
    pub fn covers_all(&self) -> bool {
        (0..HI_COUNT).all(|i| self.entry(i).is_some())
    }

    /// 未过项清单。
    pub fn failed_items(&self) -> Vec<usize> {
        self.entries
            .iter()
            .filter(|x| !x.passed)
            .map(|x| x.item)
            .collect()
    }

    /// 批推进合格：全覆盖 + 全过。
    pub fn batch_ok(&self) -> bool {
        self.covers_all() && self.failed_items().is_empty()
    }

    /// 各类别计数。
    pub fn kind_counts(&self) -> [usize; HK_COUNT] {
        let mut c = [0usize; HK_COUNT];
        for x in &self.entries {
            if x.kind < HK_COUNT {
                c[x.kind] += 1;
            }
        }
        c
    }

    /// 体检指纹。
    pub fn health_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.entries {
            h = h.wrapping_mul(0x0100_0193) ^ x.item as u32;
            h = h.wrapping_mul(0x0100_0193) ^ x.kind as u32;
            h ^= x.passed as u32;
            for b in x.note.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || !self.batch_ok() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_health(&self) -> String {
        let mut out = String::new();
        for x in &self.entries {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("B10HEALTH-SEAL\n");
        }
        out
    }
}

impl Default for BatchHealth {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：六项全过体检。
fn standard_health() -> BatchHealth {
    let mut h = BatchHealth::new();
    let items: [(usize, usize); 6] = [
        (HI_SEG_PROFILERS, HK_COMPILE),
        (HI_SUBDOMAP, HK_CHECKSET),
        (HI_WINMAP, HK_CHECKSET),
        (HI_REPLAY, HK_CHECKSET),
        (HI_LEDGER, HK_SEAL),
        (HI_ARCHIVE, HK_SEAL),
    ];
    for (item, kind) in items {
        let _ = h.examine(HealthEntry {
            item,
            kind,
            passed: true,
            note: String::new(),
        });
    }
    h
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let mk = |item: usize, kind: usize, passed: bool| HealthEntry {
        item,
        kind,
        passed,
        note: String::new(),
    };
    // 注 1：未知项
    let mut h = BatchHealth::new();
    if h.examine(mk(9, HK_COMPILE, true)) == Err(HB_UNK_ITEM) {
        caught += 1;
    }
    // 注 2：未知类别
    let mut h = BatchHealth::new();
    if h.examine(mk(HI_LEDGER, 9, true)) == Err(HB_UNK_KIND) {
        caught += 1;
    }
    // 注 3：未过未显式
    let mut h = BatchHealth::new();
    if h.examine(mk(HI_LEDGER, HK_SEAL, false)) == Err(HB_INEXPLICIT) {
        caught += 1;
    }
    // 注 4：重体检项
    let mut h = standard_health();
    if h.examine(mk(HI_LEDGER, HK_SEAL, true)) == Err(HB_DUP_ITEM) {
        caught += 1;
    }
    // 注 5：印后体检
    let mut h = standard_health();
    let _ = h.seal();
    if h.examine(mk(HI_LEDGER, HK_SEAL, true)) == Err(HB_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 体检账；J2 体检语义；J3 体检指纹。
pub fn run_unx56198_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56198");
    // J1-1 六项全体检
    let h = standard_health();
    let ok = h.len() == HI_COUNT && h.covers_all();
    s.add("56198-J1-六项全体检", ok, if ok { "" } else { "体检账不全" });
    // J1-2 渲染导出齐
    let mut h2 = standard_health();
    let _ = h2.seal();
    let text = h2.render_health();
    let ok = text.contains("HI0|seg-profilers|compile|PASS")
        && text.contains("HI5|archive|seal|PASS")
        && text.contains("B10HEALTH-SEAL");
    s.add("56198-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 覆盖核
    let ok = h.covers_all();
    s.add("56198-J2-覆盖核", ok, if ok { "" } else { "覆盖缺失" });
    // J2-2 类别闭集合法
    let ok = h.entries.iter().all(|x| x.kind < HK_COUNT);
    s.add("56198-J2-类别闭集", ok, if ok { "" } else { "类别越界" });
    // J2-3 未过项清单（空）
    let ok = h.failed_items().is_empty();
    s.add("56198-J2-未过清单空", ok, if ok { "" } else { "有未过项" });
    // J2-4 批推进合格
    let ok = h.batch_ok();
    s.add("56198-J2-批推进合格", ok, if ok { "" } else { "推进不合格" });
    // J2-5 类别计数（compile 1 / checkset 3 / seal 2）
    let kc = h.kind_counts();
    let ok = kc == [1, 3, 2];
    let detail = if ok { String::new() } else { format!("{},{},{}", kc[0], kc[1], kc[2]) };
    s.add_dyn("56198-J2-类别计数", kc == [1, 3, 2], &detail);
    // J2-6 未过显式闸
    let mut h3 = BatchHealth::new();
    let ok = h3.examine(HealthEntry {
        item: HI_REPLAY,
        kind: HK_CHECKSET,
        passed: false,
        note: "replay-var-over".to_string(),
    })
    .is_ok();
    s.add("56198-J2-未过显式", ok, if ok { "" } else { "显式闸失效" });
    // J3-1 体检指纹确定
    let ok = standard_health().health_fp() == standard_health().health_fp();
    s.add("56198-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 项变签变
    let mut h4 = standard_health();
    let fp0 = h4.health_fp();
    if let Some(x) = h4.entries.first_mut() {
        x.passed = !x.passed;
        x.note = "flip".to_string();
    }
    let ok = h4.health_fp() != fp0;
    s.add("56198-J3-项变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut h5 = standard_health();
    let ok = h5.seal() && h5.sealed();
    s.add("56198-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒体检
    let ok = h5
        .examine(HealthEntry {
            item: HI_ARCHIVE,
            kind: HK_SEAL,
            passed: true,
            note: String::new(),
        })
        == Err(HB_FROZEN);
    s.add("56198-J3-印后拒体检", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56198-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56198_all_checks_green() {
        let set = run_unx56198_checks();
        assert!(set.all_passed(), "UNX-F56198 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56198_health_stable() {
        assert!(standard_health().batch_ok());
    }
}
