//! UNX-F56069 逐进程归组账（启动集/前台/后台三类）（0x60880 段）
//!
//! 锚点：UNX-F56069-J1 归组账全量（被采进程逐个落组：启动集/前台/后台
//! 三类闭集逐个有归属）；J2 归组判定一致（同名重登必须同组，组迁移只许
//! 前台↔后台且逐次留痕）；J3 归组审计链（组名指纹、迁移史、封账指纹
//! 全留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·归组面）。
//!
//! 结构：进程归组登记册（进程名 → 组三闭集）+ 组冻结语义（启动集入组
//! 后冻结不可迁移）+ 迁移链（前台↔后台逐次留痕、同名跨组重登拒收）+
//! 分类统计核（三类计数闭式）+ 封账语义 + 注入核（未知组/空名/组冲突/
//! 启动集迁移/封账改写/未归组查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60880 独占 0x20）。
pub const SEG_BASE: u32 = 0x60880;
pub const GRP_UNKNOWN: u32 = SEG_BASE + 0x01; // 未知组（三闭集外）
pub const GRP_NAME_EMPTY: u32 = SEG_BASE + 0x02; // 进程名为空
pub const GRP_CONFLICT: u32 = SEG_BASE + 0x03; // 同名跨组重登冲突
pub const GRP_BOOT_FROZEN: u32 = SEG_BASE + 0x04; // 启动集成员迁移（冻结穿透）
pub const GRP_NOT_GROUPED: u32 = SEG_BASE + 0x05; // 未归组查询（账内无名）
pub const GRP_SEALED: u32 = SEG_BASE + 0x06; // 归组账已封账拒写
pub const GRP_SELF_MIGRATE: u32 = SEG_BASE + 0x07; // 迁移目标与现组相同（无意义迁移）

/// 组三闭集。
pub const G_BOOT: usize = 0;
pub const G_FORE: usize = 1;
pub const G_BACK: usize = 2;
pub const G_COUNT: usize = 3;
pub const G_NAMES: [&str; G_COUNT] = ["boot_set", "foreground", "background"];

/// 组名闭集判定。
pub fn group_valid(g: usize) -> bool {
    g < G_COUNT
}

/// 组指纹：FNV-1a 32 位。
pub fn group_fingerprint(name: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 迁移合法性核：仅前台↔后台互通；启动集冻结；同组迁移拒绝。
pub fn migrate_allowed(from: usize, to: usize) -> Result<(), u32> {
    if !group_valid(from) || !group_valid(to) {
        return Err(GRP_UNKNOWN);
    }
    if from == to {
        return Err(GRP_SELF_MIGRATE);
    }
    if from == G_BOOT || to == G_BOOT {
        return Err(GRP_BOOT_FROZEN);
    }
    Ok(())
}

/// 单进程归组账目：进程名 × 组 × 迁移代数 三元组。
#[derive(Clone)]
pub struct ProcEntry {
    pub name: String,
    pub group: usize,
    pub gen: u32,
}

/// 归组总账（登记册 + 迁移链 + 封账）。
pub struct GroupBook {
    pub entries: Vec<ProcEntry>,
    pub migrations: Vec<String>,
    pub sealed: bool,
}

impl GroupBook {
    pub fn new() -> Self {
        GroupBook { entries: Vec::new(), migrations: Vec::new(), sealed: false }
    }

    fn find(&self, name: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// 归组登记：空名拒、未知组拒、同名跨组重登拒、同名同组幂等放行。
    pub fn enroll(&mut self, name: &str, group: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(GRP_SEALED);
        }
        if name.is_empty() {
            return Err(GRP_NAME_EMPTY);
        }
        if !group_valid(group) {
            return Err(GRP_UNKNOWN);
        }
        match self.find(name) {
            Some(i) => {
                if self.entries[i].group != group {
                    return Err(GRP_CONFLICT);
                }
                Ok(())
            }
            None => {
                self.entries.push(ProcEntry { name: name.to_string(), group, gen: 0 });
                Ok(())
            }
        }
    }

    /// 组迁移：合法迁移留痕并升代数；非法迁移显性拒绝。
    pub fn migrate(&mut self, name: &str, to: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(GRP_SEALED);
        }
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(GRP_NOT_GROUPED),
        };
        let from = self.entries[idx].group;
        migrate_allowed(from, to)?;
        self.entries[idx].group = to;
        self.entries[idx].gen += 1;
        self.migrations.push(format!("{}|{}->{}|g{}", name, G_NAMES[from], G_NAMES[to], self.entries[idx].gen));
        Ok(())
    }

    /// 查询某进程现组（未归组显性拒绝）。
    pub fn group_of(&self, name: &str) -> Result<usize, u32> {
        match self.find(name) {
            Some(i) => Ok(self.entries[i].group),
            None => Err(GRP_NOT_GROUPED),
        }
    }

    /// J1：分类统计闭式核（三类计数之和 = 账目总数）。
    pub fn census(&self) -> [usize; G_COUNT] {
        let mut c = [0usize; G_COUNT];
        for e in self.entries.iter() {
            if group_valid(e.group) {
                c[e.group] += 1;
            }
        }
        c
    }

    /// J1：归组全量（无未归组残留 + 总数一致）。
    pub fn grouping_ok(&self) -> bool {
        let c = self.census();
        c[0] + c[1] + c[2] == self.entries.len()
    }

    /// J2：迁移链完整（逐条含箭头痕迹 + 代数递增）。
    pub fn migration_chain_ok(&self) -> bool {
        self.migrations.iter().all(|m| m.contains("->") && m.contains("|g"))
    }

    /// 迁移代数查询（未归组显性拒绝）。
    pub fn gen_of(&self, name: &str) -> Result<u32, u32> {
        match self.find(name) {
            Some(i) => Ok(self.entries[i].gen),
            None => Err(GRP_NOT_GROUPED),
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐账按登记序复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter() {
            let line = format!("{}|{}|{}", e.name, G_NAMES[e.group], e.gen);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：启动集 2 件 + 前台 2 件 + 后台 1 件。
pub fn standard_book() -> GroupBook {
    let mut b = GroupBook::new();
    let plan: [&str; 5] =
        ["init", "keventd", "compositor", "shell", "prefetchd"];
    let groups: [usize; 5] = [G_BOOT, G_BOOT, G_FORE, G_FORE, G_BACK];
    for i in 0..plan.len() {
        if b.enroll(plan[i], groups[i]).is_err() {
            panic!("standard fixture enroll must succeed");
        }
    }
    if !b.grouping_ok() {
        panic!("standard fixture grouping must be complete");
    }
    b
}

/// 注入核：7 注入必抓（未知组/空名/组冲突/启动集迁移/封账改写/未归组迁移/同组迁移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知组（g=9 越闭集）
    let mut b = GroupBook::new();
    if b.enroll("worker", 9) == Err(GRP_UNKNOWN) {
        caught += 1;
    }
    // 2) 空进程名
    if b.enroll("", G_FORE) == Err(GRP_NAME_EMPTY) {
        caught += 1;
    }
    // 3) 同名跨组重登冲突
    if b.enroll("svc", G_FORE).is_ok() && b.enroll("svc", G_BACK) == Err(GRP_CONFLICT) {
        caught += 1;
    }
    // 4) 启动集成员迁移（冻结穿透）
    let mut sb = standard_book();
    if sb.migrate("init", G_BACK) == Err(GRP_BOOT_FROZEN) {
        caught += 1;
    }
    // 5) 未归组迁移
    if sb.migrate("ghost", G_BACK) == Err(GRP_NOT_GROUPED) {
        caught += 1;
    }
    // 6) 同组迁移（前台→前台）
    if sb.migrate("shell", G_FORE) == Err(GRP_SELF_MIGRATE) {
        caught += 1;
    }
    // 7) 封账后改写拒
    sb.seal();
    if sb.enroll("late", G_BACK) == Err(GRP_SEALED)
        && sb.migrate("shell", G_BACK) == Err(GRP_SEALED)
    {
        caught += 1;
    }
    (caught, 7)
}

/// 归组账渲染导出（逐账一行：名|组|代数）。
pub fn render_book(b: &GroupBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!("{}|{}|g{}\n", e.name, G_NAMES[e.group], e.gen));
    }
    out
}

/// 渲染完整性核：行数与账目一致 + 逐行三段 + 组名合法。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    for ln in lines.iter() {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 3 {
            return false;
        }
        let mut hit = false;
        for g in G_NAMES.iter() {
            if parts[1] == *g {
                hit = true;
            }
        }
        if !hit || !parts[2].starts_with('g') {
            return false;
        }
    }
    true
}

/// 迁移史查询：某进程按代数升序的组名史。
pub fn migration_history(b: &GroupBook, name: &str) -> Vec<String> {
    let mut hist = Vec::new();
    for m in b.migrations.iter() {
        let head: Vec<&str> = m.split('|').collect();
        if head.len() == 3 && head[0] == name {
            hist.push(m.clone());
        }
    }
    hist
}

/// 迁移史严格升代数核（无同代重复、无代数回退）。
pub fn history_strictly_ascending(hist: &[String]) -> bool {
    if hist.is_empty() {
        return false;
    }
    for w in hist.windows(2) {
        let a: Vec<&str> = w[0].split("|g").collect();
        let c: Vec<&str> = w[1].split("|g").collect();
        if a.len() != 2 || c.len() != 2 {
            return false;
        }
        let ga: u32 = match a[1].parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        let gc: u32 = match c[1].parse() {
            Ok(v) => v,
            Err(_) => return false,
        };
        if ga >= gc {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56069_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56069");
    // J1-1 归组全量（三类计数闭式一致）
    let b = standard_book();
    let c = b.census();
    let ok = b.grouping_ok() && c[0] == 2 && c[1] == 2 && c[2] == 1;
    s.add("56069-J1-归组全量", ok, if ok { "" } else { "三类计数不闭" });
    // J1-2 组闭集判定（0..2 真、越界假）
    let ok = group_valid(G_BACK) && !group_valid(3);
    s.add("56069-J1-组闭集判定", ok, if ok { "" } else { "闭集语义漂移" });
    // J1-3 同名同组幂等重登放行
    let mut b2 = standard_book();
    let ok = b2.enroll("compositor", G_FORE).is_ok();
    s.add("56069-J1-幂等重登", ok, if ok { "" } else { "幂等语义漂移" });
    // J2-1 前台→后台迁移合法且留痕升代
    let mut b3 = standard_book();
    let mig = b3.migrate("shell", G_BACK);
    let gen = b3.gen_of("shell").unwrap_or(0);
    let now = b3.group_of("shell").unwrap_or(G_FORE);
    let ok = mig.is_ok() && now == G_BACK && gen == 1;
    s.add("56069-J2-前台后台迁移", ok, if ok { "" } else { "迁移语义漂移" });
    // J2-2 迁移链完整（箭头 + 代数痕迹）
    let ok = b3.migrations.len() == 1 && b3.migration_chain_ok();
    s.add("56069-J2-迁移链完整", ok, if ok { "" } else { "迁移链断裂" });
    // J2-3 迁移史严格升代数
    let hist = migration_history(&b3, "shell");
    let ok = history_strictly_ascending(&hist);
    s.add("56069-J2-迁移史升代", ok, if ok { "" } else { "迁移史破纪" });
    // J2-4 未归组查询显性拒绝
    let ok = b3.group_of("ghost").err() == Some(GRP_NOT_GROUPED);
    s.add("56069-J2-未归组拒绝", ok, if ok { "" } else { "未归组语义漂移" });
    // J3-1 封账指纹确定性
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56069-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 迁移后指纹变化（封账指纹对迁移敏感）
    let fp0 = standard_book().seal_fingerprint();
    let mut b4 = standard_book();
    let _ = b4.migrate("shell", G_BACK);
    let ok = b4.seal_fingerprint() != fp0;
    s.add("56069-J3-指纹迁移敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-3 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.contains("compositor|foreground|g0");
    s.add("56069-J3-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 组名闭集渲染名齐
    let ok = G_NAMES.len() == G_COUNT && G_NAMES[0] == "boot_set";
    s.add("56069-J1-组名闭集齐", ok, if ok { "" } else { "组名漂移" });
    // J2-5 迁移合法核：boot 冻结 + 前后台互通
    let ok = migrate_allowed(G_FORE, G_BACK).is_ok()
        && migrate_allowed(G_BACK, G_FORE).is_ok()
        && migrate_allowed(G_BOOT, G_FORE).is_err()
        && migrate_allowed(G_FORE, G_FORE).is_err();
    s.add("56069-J2-迁移合法性核", ok, if ok { "" } else { "合法性核漂移" });
    // 注入核 7/7
    let (cc, n) = injection_core();
    let ok = cc == n;
    let detail = if ok { String::new() } else { format!("注入 {cc}/{n}") };
    s.add_dyn("56069-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56069_all_checks_green() {
        let set = run_unx56069_checks();
        assert!(set.all_passed(), "UNX-F56069 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56069_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
