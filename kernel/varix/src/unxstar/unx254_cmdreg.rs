//! UNX-F0254 cmdline 全参数链白名单注册（F0016 扩容账）（0x3D50 段）
//!
//! 锚点：UNX-F0254-J1 全域参数注册表（名称/类型/范围/消费条目）
//! 齐备，未注册参数拒绝计数。
//! 域：CoRun·UNX-A1-B13（cmdline 白名单面）。
//!
//! 结构：参数类型闭集（bool/u32/str/flag 四型）+ 注册表行（名称/
//! 类型/值域/消费条目）+ 白名单校验器（名称命中 + 类型判定 + 值域
//! 夹取 + 消费条目回链）+ 拒绝账（未注册名称/类型不符/值越域逐类
//! 计数）+ 注入核（未注册名/类型不符/值越域/空名必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3D50 独占 0x10）。
pub const SEG_BASE: u32 = 0x3D50;
pub const CR_NAME_UNREG: u32 = SEG_BASE + 0x01; // 名称未注册
pub const CR_TYPE_BAD: u32 = SEG_BASE + 0x02; // 类型不符
pub const CR_VAL_OOB: u32 = SEG_BASE + 0x03; // 值越域
pub const CR_NAME_EMPTY: u32 = SEG_BASE + 0x04; // 空名称
pub const CR_SEALED: u32 = SEG_BASE + 0x05; // 注册表封账拒写

/// 参数类型闭集。
pub const TY_BOOL: u8 = 0;
pub const TY_U32: u8 = 1;
pub const TY_STR: u8 = 2;
pub const TY_FLAG: u8 = 3;
pub const TY_COUNT: usize = 4;

/// 类型名。
pub fn type_name(t: u8) -> &'static str {
    match t {
        TY_BOOL => "bool",
        TY_U32 => "u32",
        TY_STR => "str",
        TY_FLAG => "flag",
        _ => "?",
    }
}

/// 值域（u32 型生效：lo..=hi；其余型忽略）。
#[derive(Clone, Copy)]
pub struct ValRange {
    pub lo: u32,
    pub hi: u32,
}

/// 注册表行：名称/类型/值域/消费条目。
#[derive(Clone)]
pub struct ParamRow {
    pub name: String,
    pub ty: u8,
    pub range: Option<ValRange>,
    pub consumer: String,
}

/// 白名单注册表。
pub struct ParamRegistry {
    pub rows: Vec<ParamRow>,
    pub rej_unreg: u32,
    pub rej_type: u32,
    pub rej_val: u32,
    pub sealed: bool,
}

impl ParamRegistry {
    pub fn new() -> Self {
        ParamRegistry {
            rows: Vec::new(),
            rej_unreg: 0,
            rej_type: 0,
            rej_val: 0,
            sealed: false,
        }
    }

    /// 注册参数（名称唯一；重名覆盖）。
    pub fn register(
        &mut self,
        name: &str,
        ty: u8,
        range: Option<ValRange>,
        consumer: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_SEALED);
        }
        if name.is_empty() {
            return Err(CR_NAME_EMPTY);
        }
        if (ty as usize) >= TY_COUNT {
            return Err(CR_TYPE_BAD);
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.name == name) {
            r.ty = ty;
            r.range = range;
            r.consumer = String::from(consumer);
        } else {
            self.rows.push(ParamRow {
                name: String::from(name),
                ty,
                range,
                consumer: String::from(consumer),
            });
        }
        Ok(())
    }

    /// 命中查行。
    pub fn lookup(&self, name: &str) -> Option<&ParamRow> {
        self.rows.iter().find(|r| r.name == name)
    }

    /// 白名单校验：名称命中 + 类型判定 + 值域判定。
    /// 拒绝逐类计数（未注册/类型不符/值越域）。
    pub fn validate(&mut self, name: &str, ty: u8, val: u32) -> Result<(), u32> {
        if name.is_empty() {
            self.rej_unreg += 1;
            return Err(CR_NAME_EMPTY);
        }
        let row = match self.lookup(name) {
            Some(r) => r,
            None => {
                self.rej_unreg += 1;
                return Err(CR_NAME_UNREG);
            }
        };
        if row.ty != ty {
            self.rej_type += 1;
            return Err(CR_TYPE_BAD);
        }
        if let Some(rg) = row.range {
            if val < rg.lo || val > rg.hi {
                self.rej_val += 1;
                return Err(CR_VAL_OOB);
            }
        }
        Ok(())
    }

    /// 拒绝账守恒核：三类计数之和 = 总校验次数 - 通过次数。
    pub fn ledger_consistent(&self, total: u32, passed: u32) -> bool {
        self.rej_unreg + self.rej_type + self.rej_val == total - passed
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 注册表渲染（名称/类型/值域/消费条目四列）。
    pub fn render(&self) -> String {
        let mut out = format!("cmdline 白名单 注册数={}\n", self.rows.len());
        for r in &self.rows {
            let rg = match r.range {
                Some(v) => format!("[{},{}]", v.lo, v.hi),
                None => String::from("-"),
            };
            let line = format!(
                "  {} {:?} {} 消费={}\n",
                r.name,
                rg,
                type_name(r.ty),
                r.consumer
            );
            out.push_str(&line);
        }
        let rej = format!(
            "拒绝账 未注册={} 类型不符={} 值越域={}\n",
            self.rej_unreg, self.rej_type, self.rej_val
        );
        out.push_str(&rej);
        out
    }
}

/// 标准夹具：F0016 扩容账全域参数（8 项覆盖四型与值域边界）。
pub fn standard_registry() -> ParamRegistry {
    let mut r = ParamRegistry::new();
    let _ = r.register("console", TY_STR, None, "A1引导面");
    let _ = r.register("loglevel", TY_U32, Some(ValRange { lo: 0, hi: 7 }), "A1日志面");
    let _ = r.register("quiet", TY_FLAG, None, "A1引导面");
    let _ = r.register("nokaslr", TY_BOOL, None, "A4随机化");
    let _ = r.register("memmap", TY_STR, None, "A2内存面");
    let _ = r.register("sched quantum", TY_U32, Some(ValRange { lo: 1, hi: 1000 }), "A3CPU面");
    let _ = r.register("watchdog", TY_BOOL, None, "A1健康面");
    let _ = r.register("timersrc", TY_STR, None, "A1时间面");
    r
}

/// 注入核：8 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut r = standard_registry();
    // 1) 未注册名称拒 + 计数
    if r.validate("nosuchparam", TY_U32, 1) == Err(CR_NAME_UNREG) && r.rej_unreg == 1 {
        caught += 1;
    }
    // 2) 空名称拒
    if r.validate("", TY_U32, 1) == Err(CR_NAME_EMPTY) {
        caught += 1;
    }
    // 3) 类型不符拒（loglevel 是 u32，传 str 型）
    if r.validate("loglevel", TY_STR, 3) == Err(CR_TYPE_BAD) {
        caught += 1;
    }
    // 4) 值越上界拒
    if r.validate("loglevel", TY_U32, 8) == Err(CR_VAL_OOB) {
        caught += 1;
    }
    // 5) 值越下界拒
    if r.validate("sched quantum", TY_U32, 0) == Err(CR_VAL_OOB) {
        caught += 1;
    }
    // 6) 边界值双向通过（lo 与 hi 恰好命中）
    if r.validate("loglevel", TY_U32, 0).is_ok() && r.validate("loglevel", TY_U32, 7).is_ok() {
        caught += 1;
    }
    // 7) 封账拒注册
    r.seal();
    if r.register("late", TY_FLAG, None, "x") == Err(CR_SEALED) {
        caught += 1;
    }
    // 8) 账守恒（7 次校验：2 过 5 拒）
    if r.ledger_consistent(7, 2) {
        caught += 1;
    }
    (caught, 8)
}

/// 判据入口（CheckSet）。
pub fn run_unx254_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0254");
    // J1-1 全域注册表齐备（8 项 × 四列齐）
    let r = standard_registry();
    let ok = r.rows.len() == 8 && r.rows.iter().all(|p| !p.name.is_empty() && !p.consumer.is_empty());
    s.add("B13-39-注册表齐备", ok, if ok { "" } else { "注册表缺行或缺列" });
    // J1-2 四型闭集齐
    let r = standard_registry();
    let mut seen = [false; TY_COUNT];
    for p in &r.rows {
        seen[p.ty as usize] = true;
    }
    let ok = seen.iter().all(|&b| b);
    s.add("B13-40-四型闭集齐", ok, if ok { "" } else { "类型闭集缺型" });
    // J1-3 未注册参数拒绝计数
    let mut r = standard_registry();
    let ok = r.validate("bogus", TY_U32, 1) == Err(CR_NAME_UNREG)
        && r.validate("bogus2", TY_STR, 0) == Err(CR_NAME_UNREG)
        && r.rej_unreg == 2;
    s.add("B13-41-未注册拒绝计数", ok, if ok { "" } else { "拒绝计数漂移" });
    // J1-4 渲染齐（四列 + 拒绝账行）
    let mut r = standard_registry();
    let _ = r.validate("bogus", TY_U32, 1);
    let t = r.render();
    let ok = t.contains("消费=A2内存面") && t.contains("拒绝账");
    s.add("B13-42-注册表渲染", ok, if ok { "" } else { "渲染缺列" });
    // J1-5 注入核 8/8
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-43-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx254_all_checks_green() {
        let set = run_unx254_checks();
        assert!(set.all_passed(), "UNX-F0254 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx254_range_clamp_bounds() {
        let mut r = standard_registry();
        assert!(r.validate("sched quantum", TY_U32, 1).is_ok());
        assert!(r.validate("sched quantum", TY_U32, 1000).is_ok());
    }
}
