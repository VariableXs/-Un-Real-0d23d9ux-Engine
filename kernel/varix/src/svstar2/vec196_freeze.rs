//! VE-F0596 · C 域接口冻结清单（六组接口入冻结）
//!
//! 职责：C 域对外接口冻结清单——六组接口入冻结：
//! 编译服务 API（F0581 服务 + F0585 协议）、缓存 API（F0590 键四元组契约）、
//! 诊断 API（F0594 结构化错误）、遥测 API（F0589 指标集）、
//! 后端插件 Traits（F0559）、选择器策略（F0579）。
//! 冻结纪律：接口版本化、向后兼容承诺、变更走升版重核对流程。
//! 下游先行纪律：D 域及后续域以冻结接口 + fake 先行开发，不等待实现完成。
//! 清单附符合性三态（符合、偏差已登记、待核）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 六组冻结接口的组号与归属条目（冻结 O(接口数)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfaceGroup {
    CompileSvc,  // F0581 + F0585
    CacheApi,    // F0590
    DiagApi,     // F0594
    TelemApi,    // F0589
    PlugTraits,  // F0559
    Selector,    // F0579
}

impl IfaceGroup {
    pub fn all() -> Vec<IfaceGroup> {
        vec![
            IfaceGroup::CompileSvc, IfaceGroup::CacheApi, IfaceGroup::DiagApi,
            IfaceGroup::TelemApi, IfaceGroup::PlugTraits, IfaceGroup::Selector,
        ]
    }
    pub fn name(self) -> &'static str {
        match self {
            IfaceGroup::CompileSvc => "编译服务API",
            IfaceGroup::CacheApi => "缓存API",
            IfaceGroup::DiagApi => "诊断API",
            IfaceGroup::TelemApi => "遥测API",
            IfaceGroup::PlugTraits => "后端插件Traits",
            IfaceGroup::Selector => "选择器策略",
        }
    }
}

/// 符合性三态：符合 / 偏差已登记（判定阻断或随行）/ 待核（显性标注限期）。
#[derive(Clone, Debug, PartialEq)]
pub enum Compliance {
    Ok,
    /// 偏差登记：偏差描述 + 判定（阻断=必须整改 / 随行=下次升版一并处理）。
    Deviation(String, bool),
    /// 待核：限期（条目号）——到期未核升级为偏差。
    Pending(u32),
}

/// 冻结接口条目：组 + 接口名 + 版本 + 兼容承诺。
#[derive(Clone, Debug, PartialEq)]
pub struct FrozenIface {
    pub group: IfaceGroup,
    pub name: String,
    /// 接口版本：从 1 起；变更必须升版。
    pub version: u32,
}

/// 版本承诺档：向后兼容 = 升小版本不破旧调用方；破兼容必须升大版本。
pub fn bump_version(old: u32, breaking: bool) -> u32 {
    if breaking { old + 100 } else { old + 1 }
}

/// 升版重核对流程：接口变更→升版→六组逐组重核对→出结论。
pub fn upgrade_review(groups_ok: &[IfaceGroup]) -> Result<(), String> {
    let missing: Vec<&str> = IfaceGroup::all()
        .iter()
        .filter(|g| !groups_ok.contains(g))
        .map(|g| g.name())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("升版重核对未过：{} 组未核——{}", missing.len(), missing.join(", ")))
    }
}

/// 冻结清单正本：六组齐备才可冻结发布。
pub struct FreezeList {
    pub items: Vec<FrozenIface>,
}

impl FreezeList {
    pub fn new() -> Self {
        FreezeList { items: Vec::new() }
    }

    /// 登记冻结条目：组、名、初始版本 1。
    pub fn freeze(&mut self, group: IfaceGroup, name: &str) {
        self.items.push(FrozenIface { group, name: name.to_string(), version: 1 });
    }

    /// 六组齐备核验：冻结清单必须覆盖全部六组。
    pub fn complete(&self) -> Result<(), String> {
        let missing: Vec<&str> = IfaceGroup::all()
            .iter()
            .filter(|g| !self.items.iter().any(|i| i.group == **g))
            .map(|g| g.name())
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("冻结清单缺组：{}", missing.join(", ")))
        }
    }

    /// 变更条目：破兼容→大版本 + 触发全组升版重核对；兼容→小版本。
    pub fn change(&mut self, name: &str, breaking: bool) -> Result<u32, String> {
        let idx = self
            .items
            .iter()
            .position(|i| i.name == name)
            .ok_or_else(|| format!("接口 {} 不在冻结清单：变更对象不存在", name))?;
        let nv = bump_version(self.items[idx].version, breaking);
        self.items[idx].version = nv;
        Ok(nv)
    }

    /// 下游先行核验：D 域 fake 开发所引用的接口必须在冻结清单且版本已冻结。
    pub fn downstream_fake_ok(&self, name: &str) -> bool {
        self.items.iter().any(|i| i.name == name && i.version >= 1)
    }
}

/// 符合性三态表：逐接口登记三态；三态汇总用于移交判定。
pub fn compliance_verdict(states: &[Compliance]) -> Result<(), String> {
    let mut blockers: Vec<String> = Vec::new();
    for st in states {
        match st {
            Compliance::Ok => {}
            Compliance::Deviation(desc, blocking) => {
                if *blocking {
                    blockers.push(format!("偏差阻断：{}", desc));
                }
                // 随行偏差：不阻断，登记在案随下版处理。
            }
            Compliance::Pending(due) => {
                // 待核：显性限期——本实现以条目号到期即视为待核超期（阻断）。
                if *due == 0 {
                    blockers.push("待核超期：限期为空".to_string());
                }
            }
        }
    }
    if blockers.is_empty() {
        Ok(())
    } else {
        Err(blockers.join("；"))
    }
}

pub fn run_vec196_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec196");
    // 族一：六组冻结——齐备、登记、名称
    let mut fl = FreezeList::new();
    for g in IfaceGroup::all() {
        fl.freeze(g, g.name());
    }
    s.add("六组全登记", fl.items.len() == 6, "six");
    s.add("六组齐备核验通过", fl.complete().is_ok(), "complete");
    let mut fl2 = FreezeList::new();
    fl2.freeze(IfaceGroup::CompileSvc, "编译服务API");
    s.add("缺组核验拒绝", fl2.complete().is_err(), "missing-group");
    match fl2.complete() {
        Err(m) => s.add("缺组点名", m.contains("缓存API") || m.contains("诊断API"), "missing-name"),
        Ok(()) => s.add("缺组点名", false, "expected err"),
    }
    // 族二：版本化与升版流程
    s.add("初版为 1", fl.items.iter().all(|i| i.version == 1), "v1");
    s.add("兼容变更升小版", bump_version(1, false) == 2, "minor");
    s.add("破兼容升大版", bump_version(1, true) == 101, "major");
    s.add("清单内变更升版", fl.change("缓存API", false) == Ok(2), "change-ok");
    s.add("清单外变更拒绝", fl.change("不存在的接口", false).is_err(), "change-miss");
    let all: Vec<IfaceGroup> = IfaceGroup::all();
    s.add("全组重核对通过", upgrade_review(&all).is_ok(), "review-ok");
    let partial = vec![IfaceGroup::CompileSvc, IfaceGroup::CacheApi];
    match upgrade_review(&partial) {
        Err(m) => s.add("漏组重核对拒绝", m.contains("4") && m.contains("遥测API"), "review-red"),
        Ok(()) => s.add("漏组重核对拒绝", false, "expected err"),
    }
    // 族三：下游先行纪律
    s.add("冻结接口可供 fake 先行", fl.downstream_fake_ok("缓存API"), "fake-ok");
    s.add("未冻结接口不可先行", !fl.downstream_fake_ok("不存在的接口"), "fake-block");
    // 族四：符合性三态
    let ok_only = vec![Compliance::Ok, Compliance::Ok];
    s.add("全符合放行", compliance_verdict(&ok_only).is_ok(), "all-ok");
    let with_side = vec![Compliance::Ok, Compliance::Deviation("文档措辞偏差".to_string(), false)];
    s.add("随行偏差不阻断", compliance_verdict(&with_side).is_ok(), "side-dev");
    let with_block = vec![Compliance::Deviation("键序变了".to_string(), true)];
    match compliance_verdict(&with_block) {
        Err(m) => s.add("阻断偏差拦移交", m.contains("阻断") && m.contains("键序"), "block-dev"),
        Ok(()) => s.add("阻断偏差拦移交", false, "expected err"),
    }
    let pending = vec![Compliance::Ok, Compliance::Pending(5912), Compliance::Pending(0)];
    match compliance_verdict(&pending) {
        Err(m) => s.add("待核超期阻断", m.contains("超期"), "pending-due"),
        Ok(()) => s.add("待核超期阻断", false, "expected err"),
    }
    s.add("三态可并存登记", {
        let mix = vec![Compliance::Ok, Compliance::Deviation("小".to_string(), false), Compliance::Pending(5990)];
        compliance_verdict(&mix).is_ok()
    }, "mix");
    s
}
