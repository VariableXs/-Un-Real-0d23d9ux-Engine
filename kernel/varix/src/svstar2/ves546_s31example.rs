//! VE-F4312 · 场景应用示例（U06 场景应用组）
//! 职责：场景示例库——示例族（每景两例：标准路径例 + 边界乱序例共十四例）
//! + 示例可导入场景台沙箱试跑（冲突沙箱提示）+ 示例与剧本库同源（单源
//! 维护防分叉——同源校验失效下架）+ 边界例缺失补录排程 + 示例遵循交互
//! 词典（域本色）。
//! 段位：0x31A0（续表 idx546）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x31A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 同源校验失效（与剧本库分叉——下架）。
    Diverged = SEG_BASE,
    /// 沙箱试跑冲突（提示不落本体）。
    SandboxConflict = SEG_BASE + 0x01,
    /// 边界例缺失（补录排程）。
    EdgeMissing = SEG_BASE + 0x02,
    /// 景号越界。
    SceneRange = SEG_BASE + 0x03,
    /// 词典失配（示例输出不合交互词典——域色）。
    DictMismatch = SEG_BASE + 0x04,
}

/// 示例例目。
#[derive(Clone, Debug, PartialEq)]
pub struct Example {
    pub scene: u8,
    /// false=标准路径例 true=边界乱序例。
    pub edge: bool,
    /// 同源指纹（与剧本库剧本指纹比对）。
    pub src_hash: u32,
    /// 词典合规（输出全部走交互词典）。
    pub dict_ok: bool,
    pub retired: bool,
}

/// 沙箱试跑结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunOutcome {
    Pass,
    Conflict,
}

/// 示例库台。
#[derive(Default)]
pub struct ExampleDesk {
    pub examples: Vec<Example>,
    /// 剧本库同源登记：(景, 指纹)。
    pub script_src: Vec<(u8, u32)>,
    pub runs: u32,
    pub conflicts: u32,
    pub backfills: u32,
}

const SCENE_MAX: u8 = 7;

impl ExampleDesk {
    /// 登记剧本库同源指纹（单源正本）。
    pub fn register_script(&mut self, scene: u8, hash: u32) {
        if let Some(e) = self.script_src.iter_mut().find(|e| e.0 == scene) {
            e.1 = hash;
        } else {
            self.script_src.push((scene, hash));
        }
    }

    /// 登记示例：词典失配拒收（域色）；景号越界拒。
    pub fn add(&mut self, scene: u8, edge: bool, src_hash: u32, dict_ok: bool) -> Result<(), ExCode> {
        if scene == 0 || scene > SCENE_MAX {
            return Err(ExCode::SceneRange);
        }
        if !dict_ok {
            return Err(ExCode::DictMismatch);
        }
        self.examples.push(Example { scene, edge, src_hash, dict_ok, retired: false });
        Ok(())
    }

    /// 同源校验：与剧本库指纹不符 = 分叉 → 下架（返回下架数）。
    pub fn verify_src(&mut self) -> usize {
        let mut n = 0;
        for ex in self.examples.iter_mut() {
            let src = self.script_src.iter().find(|e| e.0 == ex.scene).map(|e| e.1);
            if src != Some(ex.src_hash) && !ex.retired {
                ex.retired = true;
                n += 1;
            }
        }
        n
    }

    /// 十四例齐备断言（七景 × 两例，未下架口径）。
    pub fn complete(&self) -> bool {
        (1..=SCENE_MAX).all(|s| {
            self.examples.iter().any(|e| e.scene == s && !e.edge && !e.retired)
                && self.examples.iter().any(|e| e.scene == s && e.edge && !e.retired)
        })
    }

    /// 边界例缺失景号清单（补录排程对象）。
    pub fn missing_edge(&self) -> Vec<u8> {
        (1..=SCENE_MAX)
            .filter(|s| !self.examples.iter().any(|e| e.scene == *s && e.edge && !e.retired))
            .collect()
    }

    /// 补录排程：补齐边界例（指纹须挂同源；指纹缺失 = 仍缺）。
    pub fn backfill_edge(&mut self, scene: u8, src_hash: u32) -> Result<(), ExCode> {
        if scene == 0 || scene > SCENE_MAX {
            return Err(ExCode::SceneRange);
        }
        if !self.script_src.iter().any(|e| e.0 == scene && e.1 == src_hash) {
            return Err(ExCode::Diverged);
        }
        self.examples.push(Example { scene, edge: true, src_hash, dict_ok: true, retired: false });
        self.backfills += 1;
        Ok(())
    }

    /// 沙箱试跑：冲突计数不入本体（沙箱提示语义）。
    pub fn sandbox_run(&mut self, scene: u8, outcome: RunOutcome) -> Result<(), ExCode> {
        if !self.examples.iter().any(|e| e.scene == scene && !e.retired) {
            return Err(ExCode::SceneRange);
        }
        self.runs += 1;
        if outcome == RunOutcome::Conflict {
            self.conflicts += 1;
            return Err(ExCode::SandboxConflict);
        }
        Ok(())
    }

    /// 在册有效示例（未下架）。
    pub fn active(&self) -> usize {
        self.examples.iter().filter(|e| !e.retired).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves546_checks() -> CheckSet {
    let mut set = CheckSet::new("ves546");
    let mut ed = ExampleDesk::default();

    // C546-01 剧本库同源登记 + 每景两例（标准 + 边界）登记。
    let mut ok = true;
    for s in 1..=7u8 {
        ed.register_script(s, 0xB000 | s as u32);
        if ed.add(s, false, 0xB000 | s as u32, true).is_err() {
            ok = false;
        }
        if ed.add(s, true, 0xB000 | s as u32, true).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C546-01", ok && ed.complete() && ed.active() == 14, "十四例同源齐备");

    // C546-02 词典失配拒收（域色硬线）。
    chk(&mut set, "C546-02", ed.add(1, false, 0xB001, false) == Err(ExCode::DictMismatch), "词典失配拒收");

    // C546-03 景号越界拒。
    chk(&mut set, "C546-03", ed.add(0, false, 0xB001, true).is_err() && ed.add(8, false, 0xB001, true).is_err(), "景号越界拒");

    // C546-04 分叉检出下架（剧本库指纹演进、示例未跟）。
    ed.register_script(3, 0xB0FF);
    let retired = ed.verify_src();
    chk(&mut set, "C546-04", retired == 2 && !ed.complete(), "分叉两例下架");

    // C546-05 沙箱试跑通过。
    chk(&mut set, "C546-05", ed.sandbox_run(1, RunOutcome::Pass).is_ok() && ed.runs == 1, "沙箱试跑通过");

    // C546-06 沙箱冲突：提示不入本体（本体示例不被改动）。
    let before = ed.active();
    chk(&mut set, "C546-06", ed.sandbox_run(1, RunOutcome::Conflict) == Err(ExCode::SandboxConflict)
        && ed.conflicts == 1 && ed.active() == before, "冲突沙箱提示不落本体");

    // C546-07 边界例缺失清单（3 景两例已下架）。
    let miss = ed.missing_edge();
    chk(&mut set, "C546-07", miss == [3], "缺失清单精确");

    // C546-08 补录排程：同源指纹挂载成功。
    chk(&mut set, "C546-08", ed.backfill_edge(3, 0xB0FF).is_ok() && ed.backfills == 1, "补录挂同源");

    // C546-09 补录指纹失配 = 拒（防再分叉）。
    chk(&mut set, "C546-09", ed.backfill_edge(3, 0xDEAD) == Err(ExCode::Diverged), "失配指纹拒补");

    // C546-10 复校同源：边界例已跟新指纹 → 无新分叉 + 缺失清单清空。
    let r2 = ed.verify_src();
    chk(&mut set, "C546-10", r2 == 0 && ed.missing_edge().is_empty(), "复校无新分叉");

    // C546-11 已下架件不复活（重复 verify 幂等）。
    let r3 = ed.verify_src();
    chk(&mut set, "C546-11", r3 == 0, "下架幂等");

    // C546-12 全下架后 complete 恒假 + 空库断言（非恒真：依赖真实下架路径）。
    let empty = ExampleDesk::default();
    let mut ed2 = ExampleDesk::default();
    ed2.register_script(1, 1);
    let _ = ed2.add(1, false, 1, true);
    let _ = ed2.add(1, true, 1, true);
    ed2.register_script(1, 2); // 剧本库演进、示例未跟
    let _ = ed2.verify_src(); // 两例全下架
    chk(&mut set, "C546-12", !ed2.complete() && ed2.active() == 0 && !empty.complete(), "全下架+空库断言");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divergence_retires() {
        let mut ed = ExampleDesk::default();
        ed.register_script(1, 100);
        assert!(ed.add(1, false, 100, true).is_ok());
        assert!(ed.add(1, true, 100, true).is_ok());
        ed.register_script(1, 200);
        assert_eq!(ed.verify_src(), 2);
        assert_eq!(ed.verify_src(), 0); // 幂等
    }

    #[test]
    fn dict_hardline() {
        let mut ed = ExampleDesk::default();
        assert_eq!(ed.add(1, false, 1, false), Err(ExCode::DictMismatch));
        assert!(!ed.add(8, false, 1, true).is_err() == false);
    }

    #[test]
    fn missing_and_backfill() {
        let mut ed = ExampleDesk::default();
        ed.register_script(2, 7);
        assert!(ed.add(2, false, 7, true).is_ok());
        assert_eq!(ed.missing_edge(), [2]);
        assert!(ed.backfill_edge(2, 7).is_ok());
        assert!(ed.missing_edge().is_empty());
        assert_eq!(ed.backfill_edge(2, 9), Err(ExCode::Diverged));
    }
}
