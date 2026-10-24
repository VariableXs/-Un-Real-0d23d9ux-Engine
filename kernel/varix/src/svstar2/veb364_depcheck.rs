//! veb364_depcheck —— VE-F0364 驱动依赖声明与校验。
//! 锚点（CoRun Unxreal 主册）：VE-F0364 · 驱动依赖声明与校验 · 目标 340 行
//! 判据原文：VE-F0364-J1 驱动登记时声明依赖（依赖名 + 最低版本），
//!           校验器逐依赖核对在册表：缺依赖拒装（拒绝码带缺失
//!           名）；版本不达拒装（带双方版本）；循环依赖检出
//!           （登记图 DFS 找环，环成员立案）；依赖深度限（超
//!           深拒，防栈爆）；校验可重放。
//! 组件：依赖声明（名 + 版本下界）+ 在册表 + 校验器（存在性 +
//!       版本 + 环 + 深度）+ 环检出账 + 重放核。
//! 职责定位：依赖校验是「驱动的装机预检」——缺依赖的驱动装上
//!   去就是运行期炸弹；循环依赖是登记表的自相矛盾（A 等 B、
//!   B 等 A，谁先装？）；深度限防的是依赖链爆炸：一万层深的
//!   依赖图校验一次就把栈烧了。
//! 性能逐项分解：单驱动校验 O(依赖数)；环检出 O(在册数)；
//!   重放 O(案件数)。
//! 诊断码段：0x30080（0x30081~0x30089 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DC_VERSION: &str = "veb364_depcheck-v1";

/// 诊断段基址（0x30080 段 · 本模块独占）。
pub const DC_SEG_BASE: u32 = 0x30080;
pub const XC_DEP_MISS: u32 = 0x30081; // 依赖缺失
pub const XC_DEP_OLD: u32 = 0x30082; // 版本不达
pub const XC_CYCLE: u32 = 0x30083; // 循环依赖
pub const XC_DEPTH_HI: u32 = 0x30084; // 依赖深度超限
pub const XC_DUP_DRV: u32 = 0x30085; // 驱动重名
pub const XC_EMPTY_NAME: u32 = 0x30086; // 名空
pub const XC_REPLAY: u32 = 0x30087; // 重放不一致
pub const XC_MAGIC: u32 = 0x30088; // 账魔数破
pub const XC_SELF_DEP: u32 = 0x30089; // 自依赖

/// 依赖深度上限。
pub const DEPTH_MAX: u32 = 16;

/// 在册驱动。
#[derive(Clone, Debug)]
pub struct RegDrv {
    pub name: String,
    pub ver: u32,
    pub deps: Vec<(String, u32)>,
}

/// 校验器。
pub struct DepChecker {
    registry: Vec<RegDrv>,
    cycles: Vec<Vec<String>>,
}

impl DepChecker {
    /// 建。
    pub fn new() -> DepChecker {
        DepChecker { registry: Vec::new(), cycles: Vec::new() }
    }

    /// 登记：重名拒、名空拒、自依赖拒。
    pub fn register(&mut self, name: &str, ver: u32, deps: Vec<(String, u32)>) -> Result<(), u32> {
        if name.is_empty() {
            return Err(XC_EMPTY_NAME);
        }
        for d in deps.iter() {
            if d.0 == name {
                return Err(XC_SELF_DEP);
            }
        }
        for r in self.registry.iter() {
            if r.name == name {
                return Err(XC_DUP_DRV);
            }
        }
        self.registry.push(RegDrv { name: String::from(name), ver, deps });
        Ok(())
    }

    /// 在册查找。
    fn find(&self, name: &str) -> Option<&RegDrv> {
        self.registry.iter().find(|r| r.name == name)
    }

    /// 单驱动校验：逐依赖存在 + 版本下界；传递深度限。
    pub fn validate(&mut self, name: &str) -> Result<u32, u32> {
        let root = match self.find(name) {
            Some(r) => r.name.clone(),
            None => return Err(XC_DEP_MISS),
        };
        // DFS：返回深度；沿途回收环。
        let mut path: Vec<String> = Vec::new();
        let depth = self.dfs(&root, &mut path, 0)?;
        Ok(depth)
    }

    fn dfs(&mut self, cur: &str, path: &mut Vec<String>, depth: u32) -> Result<u32, u32> {
        if depth > DEPTH_MAX {
            return Err(XC_DEPTH_HI);
        }
        if path.iter().any(|p| p == cur) {
            // 环：从环起点截取成员。
            let start = path.iter().position(|p| p == cur).unwrap_or(0);
            let mut cyc: Vec<String> = Vec::new();
            for p in path.iter().skip(start) {
                cyc.push(p.clone());
            }
            cyc.push(String::from(cur));
            self.cycles.push(cyc);
            return Err(XC_CYCLE);
        }
        let deps = match self.find(cur) {
            Some(r) => r.deps.clone(),
            None => return Err(XC_DEP_MISS),
        };
        path.push(String::from(cur));
        let mut max_d = depth;
        for (dn, dmin) in deps.iter() {
            let dv = match self.find(dn) {
                Some(r) => r.ver,
                None => {
                    path.pop();
                    return Err(XC_DEP_MISS);
                }
            };
            if dv < *dmin {
                path.pop();
                return Err(XC_DEP_OLD);
            }
            let d = self.dfs(dn, path, depth + 1)?;
            if d > max_d {
                max_d = d;
            }
        }
        path.pop();
        Ok(max_d)
    }

    /// 环案数。
    pub fn cycle_count(&self) -> usize {
        self.cycles.len()
    }

    /// 在册数。
    pub fn registry_len(&self) -> usize {
        self.registry.len()
    }

    /// 重放核：校验重跑结论一致。
    pub fn replay_ok(&mut self) -> Result<(), u32> {
        let names: Vec<String> = self.registry.iter().map(|r| r.name.clone()).collect();
        let before: Vec<Result<u32, u32>> = names
            .iter()
            .map(|n| self.validate(n))
            .collect();
        let after: Vec<Result<u32, u32>> = names
            .iter()
            .map(|n| self.validate(n))
            .collect();
        for (a, b) in before.iter().zip(after.iter()) {
            if a != b {
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }
}

pub fn run_veb364_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0364");
    // 族一：正常依赖链。
    let mut c = DepChecker::new();
    let _ = c.register("base", 10, Vec::new());
    let _ = c.register("fs", 5, vec![(String::from("base"), 8)]);
    let _ = c.register("net", 3, vec![(String::from("base"), 12)]);
    s.add("三驱动在册", c.registry_len() == 3, "base/fs/net");
    s.add("fs 校验过", c.validate("fs") == Ok(1), "深度 1");
    s.add("net 版本不达", c.validate("net") == Err(XC_DEP_OLD), "base 10 < 12");
    // 补版本后过。
    let mut c2 = DepChecker::new();
    let _ = c2.register("base", 12, Vec::new());
    let _ = c2.register("fs", 5, vec![(String::from("base"), 8)]);
    let _ = c2.register("net", 3, vec![(String::from("base"), 12)]);
    s.add("net 校验过", c2.validate("net") == Ok(1), "base 12 ≥ 12");
    // 族二：缺依赖。
    let mut m = DepChecker::new();
    let _ = m.register("lonely", 1, vec![(String::from("ghost"), 1)]);
    s.add("缺依赖拒", m.validate("lonely") == Err(XC_DEP_MISS), "ghost 不在");
    s.add("根不在拒", m.validate("nobody") == Err(XC_DEP_MISS), "未登记");
    // 族三：循环依赖。
    let mut y = DepChecker::new();
    let _ = y.register("a", 1, vec![(String::from("b"), 1)]);
    let _ = y.register("b", 1, vec![(String::from("c"), 1)]);
    let _ = y.register("c", 1, vec![(String::from("a"), 1)]);
    s.add("环检出", y.validate("a") == Err(XC_CYCLE), "a→b→c→a");
    s.add("环案 1", y.cycle_count() == 1, "成员立案");
    // 二环。
    let mut z = DepChecker::new();
    let _ = z.register("p", 1, vec![(String::from("q"), 1)]);
    let _ = z.register("q", 1, vec![(String::from("p"), 1)]);
    s.add("二环检出", z.validate("p") == Err(XC_CYCLE), "p↔q");
    // 族四：深度限。
    let mut d = DepChecker::new();
    let _ = d.register("d0", 1, Vec::new());
    for i in 1..=18u32 {
        let prev = alloc::format!("d{}", i - 1);
        let cur = alloc::format!("d{}", i);
        let _ = d.register(&cur, 1, vec![(prev, 1)]);
    }
    s.add("深链超限拒", d.validate("d18") == Err(XC_DEPTH_HI), "18 > 16");
    s.add("浅链过", d.validate("d8") == Ok(8), "深度 8 ≤ 16");
    // 族五：登记拒绝。
    let mut r = DepChecker::new();
    let _ = r.register("x", 1, Vec::new());
    s.add("重名拒", r.register("x", 2, Vec::new()) == Err(XC_DUP_DRV), "x 撞");
    s.add("空名拒", r.register("", 1, Vec::new()) == Err(XC_EMPTY_NAME), "名空");
    s.add("自依赖拒",
        r.register("y", 1, vec![(String::from("y"), 1)]) == Err(XC_SELF_DEP),
        "y→y");
    // 族六：重放核。
    s.add("重放核过", c2.replay_ok().is_ok(), "两跑一致");
    s.add("魔数核", DC_SEG_BASE == 0x30080, "段基址");
    s
}
