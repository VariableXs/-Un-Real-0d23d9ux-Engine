//! ves379_s20linkup — VE-F4139 · i18n 与 T 域各组衔接
//! T04 翻译管线性能面 + T05 资产加载性能面 + T06 地区引擎性能面三处衔接确认；
//! 衔接点各自挂预算与基准指针；缺口登记不越界（只登记不越界修复）；
//! 衔接断→补链；预算未挂→补挂+立案；越界→退回；无障碍渲染预算跨组一致性核验（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDBC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LkCode {
    Ok = 0,
    LinkBroken = SEG_BASE + 0x01,
    BudgetUnhooked = SEG_BASE + 0x02,
    OutOfScope = SEG_BASE + 0x03,
    A11yMismatch = SEG_BASE + 0x04,
}

/// 三处衔接面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkPoint { T04Pipeline = 0, T05AssetLoad = 1, T06LocaleEngine = 2 }

impl LinkPoint {
    pub const ALL: [LinkPoint; 3] =
        [LinkPoint::T04Pipeline, LinkPoint::T05AssetLoad, LinkPoint::T06LocaleEngine];
    pub fn name(self) -> &'static str {
        match self {
            LinkPoint::T04Pipeline => "T04-pipeline",
            LinkPoint::T05AssetLoad => "T05-asset-load",
            LinkPoint::T06LocaleEngine => "T06-locale-engine",
        }
    }
}

/// 衔接点：挂预算指针 + 基准指针（0 = 未挂）。
#[derive(Clone, Copy)]
pub struct LinkEntry {
    pub point: LinkPoint,
    pub budget: u32, // 预算指针（如 150ms 档位 id）
    pub bench: u32,  // 基准指针（基线哈希）
}

/// 衔接面册。
pub struct LinkFace { pub entries: Vec<LinkEntry> }

impl LinkFace {
    pub fn new() -> Self { LinkFace { entries: Vec::new() } }

    pub fn hook(&mut self, e: LinkEntry) {
        match self.entries.iter_mut().find(|x| x.point == e.point) {
            Some(x) => { *x = e; }
            None => self.entries.push(e),
        }
    }

    pub fn confirmed(&self, p: LinkPoint) -> bool {
        self.entries.iter().any(|x| x.point == p && x.budget != 0 && x.bench != 0)
    }

    pub fn all_confirmed(&self) -> bool { LinkPoint::ALL.iter().all(|p| self.confirmed(*p)) }

    /// 衔接断：已登记但指针缺一 → 补链名单。
    pub fn broken(&self) -> Vec<&'static str> {
        LinkPoint::ALL.iter()
            .filter(|p| self.entries.iter().any(|x| x.point == **p) && !self.confirmed(**p))
            .map(|p| p.name()).collect()
    }

    /// 预算未挂：登记了但预算指针为 0 → 补挂+立案名单。
    pub fn unbudgeted(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|x| x.budget == 0).map(|x| x.point.name()).collect()
    }

    /// 补链：给某衔接点补齐双指针。
    pub fn relink(&mut self, p: LinkPoint, budget: u32, bench: u32) -> bool {
        match self.entries.iter_mut().find(|x| x.point == p) {
            Some(x) => { x.budget = budget; x.bench = bench; true }
            None => false,
        }
    }

    /// 无障碍渲染预算跨组一致性核验（域本色）：三处 a11y 预算指针须一致且非零。
    pub fn a11y_consistent(&self, a11y_budget: u32) -> bool {
        a11y_budget != 0
            && LinkPoint::ALL.iter().all(|p| self.confirmed(*p))
            && self.entries.iter().all(|x| x.budget == a11y_budget)
    }
}

/// 缺口登记册：登记不越界——只登记事实与归因，不代行修复。
pub struct GapRegister { pub gaps: Vec<(&'static str, &'static str)> } // (衔接点, 缺口描述)

impl GapRegister {
    pub fn new() -> Self { GapRegister { gaps: Vec::new() } }

    /// 登记缺口；越界（空点/空描述）→ 退回。
    pub fn register(&mut self, point: &'static str, desc: &'static str) -> Result<(), LkCode> {
        if point.is_empty() || desc.is_empty() { return Err(LkCode::OutOfScope); }
        self.gaps.push((point, desc));
        Ok(())
    }

    /// 登记表中点名某衔接点存在缺口。
    pub fn has_gap(&self, point: &str) -> bool { self.gaps.iter().any(|x| x.0 == point) }

    pub fn len(&self) -> usize { self.gaps.len() }
    pub fn is_empty(&self) -> bool { self.gaps.is_empty() }
}

/// 立案单：预算未挂的补挂立案（回归留痕）。
pub struct CaseFile { pub cases: Vec<(&'static str, &'static str)> } // (衔接点, 立案因由)

impl CaseFile {
    pub fn new() -> Self { CaseFile { cases: Vec::new() } }
    pub fn open(&mut self, point: &'static str, why: &'static str) { self.cases.push((point, why)); }
    pub fn open_for(&self, point: &str) -> bool { self.cases.iter().any(|x| x.0 == point) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves379_checks() -> CheckSet {
    let mut set = CheckSet::new("ves379");
    let mut face = LinkFace::new();

    // C379-01 三处衔接逐点挂载（预算+基准双指针）。
    face.hook(LinkEntry { point: LinkPoint::T04Pipeline, budget: 40, bench: 0x1101 });
    face.hook(LinkEntry { point: LinkPoint::T05AssetLoad, budget: 60, bench: 0x1102 });
    face.hook(LinkEntry { point: LinkPoint::T06LocaleEngine, budget: 50, bench: 0x1103 });
    chk(&mut set, "C379-01", face.all_confirmed() && face.entries.len() == 3, "三处衔接齐");

    // C379-02 衔接断检出：T06 基准指针撤除 → 补链名单。
    face.relink(LinkPoint::T06LocaleEngine, 50, 0);
    chk(&mut set, "C379-02", face.broken() == vec!["T06-locale-engine"], "断链接定位");

    // C379-03 补链修复。
    face.relink(LinkPoint::T06LocaleEngine, 50, 0x1103);
    chk(&mut set, "C379-03", face.all_confirmed() && face.broken().is_empty(), "补链复通");

    // C379-04 预算未挂：新登记 T05 撤预算 → 未挂名单。
    face.relink(LinkPoint::T05AssetLoad, 0, 0x1102);
    chk(&mut set, "C379-04", face.unbudgeted() == vec!["T05-asset-load"], "预算未挂定位");

    // C379-05 补挂+立案。
    let mut cases = CaseFile::new();
    cases.open("T05-asset-load", "预算指针空挂立案");
    face.relink(LinkPoint::T05AssetLoad, 60, 0x1102);
    chk(&mut set, "C379-05", face.unbudgeted().is_empty() && cases.open_for("T05-asset-load"), "补挂且立案");

    // C379-06 缺口登记：登记不越界（只记事实）。
    let mut gaps = GapRegister::new();
    gaps.register("T06-locale-engine", "格式缓存命中率未挂基准").unwrap();
    chk(&mut set, "C379-06", gaps.len() == 1 && gaps.has_gap("T06-locale-engine"), "缺口登记一条");

    // C379-07 越界退回：空描述登记被拒。
    chk(&mut set, "C379-07",
        gaps.register("T06-locale-engine", "") == Err(LkCode::OutOfScope), "越界登记退回");

    // C379-08 登记不影响衔接面册（不越界修复）。
    chk(&mut set, "C379-08", face.all_confirmed() && gaps.len() == 1, "登记与修复分离");

    // C379-09 无障碍渲染预算跨组一致性（域本色）：三处同预算一致。
    chk(&mut set, "C379-09", face.a11y_consistent(60), "三处 a11y 预算=60 一致");

    // C379-10 一致性失配：单处预算漂移 → 失配。
    face.relink(LinkPoint::T04Pipeline, 40, 0x1101);
    chk(&mut set, "C379-10", !face.a11y_consistent(60), "T04 预算 40 ≠ 60 失配");

    // C379-11 对齐后一致；零预算不一致（红线）。
    face.relink(LinkPoint::T04Pipeline, 60, 0x1101);
    let aligned = face.a11y_consistent(60);
    let zero = face.a11y_consistent(0);
    chk(&mut set, "C379-11", aligned && !zero, "对齐绿；零预算红");

    // C379-12 未登记衔接点不算 confirmed。
    let empty = LinkFace::new();
    chk(&mut set, "C379-12", !empty.confirmed(LinkPoint::T04Pipeline) && !empty.all_confirmed(), "空册全未确认");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves379_smoke() {
        let set = run_ves379_checks();
        assert!(set.failed_count() == 0, "ves379 red: {}", set.red_items_text());
    }

    #[test]
    fn ves379_names() {
        assert_eq!(LinkPoint::ALL.len(), 3);
        assert_eq!(LinkPoint::T04Pipeline.name(), "T04-pipeline");
        assert_eq!(LinkPoint::T06LocaleEngine.name(), "T06-locale-engine");
    }

    #[test]
    fn ves379_gap_empty_point() {
        let mut g = GapRegister::new();
        assert_eq!(g.register("", "x"), Err(LkCode::OutOfScope));
        assert!(g.is_empty());
    }
}
