//! 深化层四 · F137 API 稳定性承诺（2026-09-27 深化批次四 · g 层）。
//!
//! 迁移工作台（per-caller 步骤清单）、破坏影响评估（调用面×消费方
//! 等级）、冻结表（条件解冻）、API 目录渲染器（稳定徽标数据面）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 迁移工作台：每个受影响消费方的三步清单（改调用/改导入/跑测试）
// ---------------------------------------------------------------------------

pub struct WorkItem {
    pub caller: &'static str,
    pub api: &'static str,
    /// 三步完成位：0=改调用 1=改导入 2=跑测试。
    pub steps_done: [bool; 3],
}

impl WorkItem {
    pub fn new(caller: &'static str, api: &'static str) -> WorkItem {
        WorkItem { caller, api, steps_done: [false; 3] }
    }

    pub fn step(&mut self, idx: usize) -> Result<(), &'static str> {
        if idx >= 3 {
            return Err("步骤越界：三步制");
        }
        // 顺序门：第 n 步前置必须全绿（跳步 = 账面完成实未完成）。
        if self.steps_done[idx] {
            return Err("步骤已完成：重复记录拒绝");
        }
        if self.steps_done[..idx].iter().any(|d| !d) {
            return Err("前置步骤未完成：顺序门拦截");
        }
        self.steps_done[idx] = true;
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.steps_done.iter().all(|d| *d)
    }
}

// ---------------------------------------------------------------------------
// 破坏影响评估：受影响消费方 × 各自调用量 → 影响分（P0-P3）
// ---------------------------------------------------------------------------

/// 影响分 = Σ min(call_sites,100)，映射：≥300 P0 / ≥100 P1 / ≥20 P2 / 其余 P3。
pub fn impact_score(callees: &[(u32, &'static str)]) -> u8 {
    let sum: u32 = callees.iter().map(|(n, _)| (*n).min(100)).sum();
    if sum >= 300 {
        0
    } else if sum >= 100 {
        1
    } else if sum >= 20 {
        2
    } else {
        3
    }
}

// ---------------------------------------------------------------------------
// 冻结表：API 在某版本冻结，解冻必须带条件（条件为空 = 永久冻结）
// ---------------------------------------------------------------------------

pub struct FreezeEntry {
    pub api: &'static str,
    pub freeze_ver: u32,
    /// 解冻条件描述（None = 永久冻结——写死承诺面）。
    pub unfreeze_condition: Option<&'static str>,
}

pub struct FreezeTable {
    entries: alloc::vec::Vec<FreezeEntry>,
}

impl FreezeTable {
    pub fn new() -> FreezeTable {
        FreezeTable { entries: alloc::vec::Vec::new() }
    }

    pub fn freeze(&mut self, api: &'static str, ver: u32, cond: Option<&'static str>) -> Result<(), &'static str> {
        if self.entries.iter().any(|e| e.api == api) {
            return Err("重复冻结登记");
        }
        self.entries.push(FreezeEntry { api, freeze_ver: ver, unfreeze_condition: cond });
        Ok(())
    }

    /// 查询冻结态：Some(条件) = 条件解冻；Some(None 哨兵) 用 is_frozen 口径。
    pub fn is_frozen(&self, api: &str) -> bool {
        self.entries.iter().any(|e| e.api == api)
    }

    pub fn condition(&self, api: &str) -> Option<Option<&'static str>> {
        self.entries.iter().find(|e| e.api == api).map(|e| e.unfreeze_condition)
    }

    /// 条件解冻执行：条件非空的才可解冻；永久冻结拒绝（承诺不回收）。
    pub fn unfreeze(&mut self, api: &str) -> Result<(), &'static str> {
        let e = self.entries.iter_mut().find(|e| e.api == api).ok_or("未冻结");
        match e {
            Err(m) => Err(m),
            Ok(e) => match e.unfreeze_condition {
                Some(_) => {
                    // 解冻即移除登记（目录渲染随之消失——诚实面）。
                    self.entries.retain(|x| x.api != api);
                    Ok(())
                }
                None => Err("永久冻结不可解冻：写死的承诺不回收"),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// API 目录渲染：登记册 → 目录行（名字序），稳定级带徽标位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct CatalogRow {
    pub name: &'static str,
    pub stable: bool,
    pub frozen: bool,
}

pub fn render_catalog(mut rows: alloc::vec::Vec<CatalogRow>) -> alloc::vec::Vec<CatalogRow> {
    // 插入序按名字字节序（目录确定性——同输入同顺序）。
    for i in 1..rows.len() {
        let k = rows[i];
        let mut j = i;
        while j > 0 && rows[j - 1].name > k.name {
            rows[j] = rows[j - 1];
            j -= 1;
        }
        rows[j] = k;
    }
    rows
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137G_TAG: &str = "stareco-F137-deep4";

pub fn run_f137_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F137G_TAG);

    // 工作台
    let mut w = WorkItem::new("files-app", "fs_open2");
    set.add("f137g skip step", w.step(1).is_err(), "跳步拦截");
    let _ = w.step(0);
    let _ = w.step(1);
    set.add("f137g not complete", !w.complete(), "两步未完");
    let _ = w.step(2);
    set.add("f137g complete", w.complete() && w.step(0).is_err(), "三步齐+重复拒绝");

    // 影响评估
    let callees = [(120u32, "files"), (200, "editor"), (90, "search"), (50, "misc")];
    set.add("f137g p0", impact_score(&callees) == 0, "封顶合计 340 ≥300 → P0");
    set.add(
        "f137g p2",
        impact_score(&[(15u32, "a")]) == 3 && impact_score(&[(25u32, "a")]) == 2,
        "小调用面低级",
    );
    set.add(
        "f137g clamp",
        impact_score(&[(999u32, "a")]) == 1,
        "单方封顶 100 → P1（不独占 P0）",
    );

    // 冻结表
    let mut ft = FreezeTable::new();
    set.add("f137g dup freeze", ft.freeze("api_x", 3, None).is_ok() && ft.freeze("api_x", 4, None).is_err(), "重复冻结拒绝");
    set.add("f137g frozen", ft.is_frozen("api_x"), "冻结生效");
    set.add("f137g permanent", ft.unfreeze("api_x").is_err(), "永久冻结不可解");
    let _ = ft.freeze("api_y", 3, Some("双读窗满且零旧调用"));
    set.add("f137g cond unfreeze", ft.unfreeze("api_y").is_ok() && !ft.is_frozen("api_y"), "条件解冻放行");
    set.add("f137g unknown", ft.unfreeze("ghost").is_err(), "未冻结解冻拒绝");

    // 目录渲染
    let cat = render_catalog(alloc::vec![
        CatalogRow { name: "zed", stable: true, frozen: false },
        CatalogRow { name: "api_a", stable: true, frozen: true },
        CatalogRow { name: "mid", stable: false, frozen: false },
    ]);
    set.add(
        "f137g catalog order",
        cat.iter().map(|r| r.name).collect::<alloc::vec::Vec<_>>() == alloc::vec!["api_a", "mid", "zed"],
        "名字序确定性",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn workbench_full_chain() {
        let mut w = WorkItem::new("editor", "edit_save2");
        for i in 0..3 {
            assert!(w.step(i).is_ok());
        }
        assert!(w.complete());
        assert!(w.step(5).is_err()); // 越界
    }

    #[test]
    fn catalog_stable_markers() {
        let cat = render_catalog(alloc::vec![
            CatalogRow { name: "a", stable: true, frozen: true },
        ]);
        assert!(cat[0].stable && cat[0].frozen);
    }
}
