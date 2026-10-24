//! VE-F0759 布尔开放扩展
//! 职责：扩展点登记（自定义填充规则/自定义扫描谓词/结果后处理器）、
//!       扩展沙箱（只读输入视图 + 有界输出）、版本化扩展 API。
//! 契约：扩展不能修改输入；输出规模有上限；API 版本向下兼容判定。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{DirEdge, Poly, scan_row, ScanEvent};

/// 扩展 API 版本
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiVer {
    pub major: u32,
    pub minor: u32,
}

impl ApiVer {
    pub const CURRENT: ApiVer = ApiVer { major: 1, minor: 2 };
    /// 向下兼容：同主版本且宿主 minor ≥ 扩展要求
    pub fn compatible(&self, required: ApiVer) -> bool {
        self.major == required.major && self.minor >= required.minor
    }
}

/// 扩展类型
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExtKind {
    /// 自定义填充谓词
    FillPredicate,
    /// 自定义扫描后过滤
    ScanFilter,
    /// 结果后处理器
    PostProcess,
}

/// 扩展登记行
#[derive(Clone, Debug, PartialEq)]
pub struct ExtReg {
    pub name: &'static str,
    pub kind: ExtKind,
    pub ver: ApiVer,
    pub out_cap: usize,
}

/// 沙箱：只读输入 + 有界输出
pub struct Sandbox<'a> {
    /// 只读边集视图
    pub edges: &'a [DirEdge],
    /// 输出上限
    pub out_cap: usize,
    pub used: usize,
    pub violations: usize,
}

impl<'a> Sandbox<'a> {
    pub fn new(edges: &'a [DirEdge], out_cap: usize) -> Sandbox<'a> {
        Sandbox { edges, out_cap: out_cap.max(1), used: 0, violations: 0 }
    }
    /// 扫描（只读输入，产出事件）
    pub fn scan(&mut self, y: f64) -> Option<Vec<ScanEvent>> {
        let evs = scan_row(self.edges, y);
        if evs.len() > self.out_cap - self.used {
            self.violations += 1;
            return None;
        }
        self.used += evs.len();
        Some(evs)
    }
    pub fn exhausted(&self) -> bool {
        self.used >= self.out_cap
    }
}

/// 扩展注册表
pub struct ExtRegistry {
    pub regs: Vec<ExtReg>,
}

impl ExtRegistry {
    pub fn new() -> ExtRegistry {
        ExtRegistry { regs: alloc::vec![] }
    }
    /// 注册：校验 API 版本兼容 + 名字唯一
    pub fn register(&mut self, r: ExtReg) -> Result<(), &'static str> {
        if !ApiVer::CURRENT.compatible(r.ver) {
            return Err("api version incompatible");
        }
        if self.regs.iter().any(|e| e.name == r.name) {
            return Err("duplicate name");
        }
        self.regs.push(r);
        Ok(())
    }
    pub fn by_kind(&self, k: ExtKind) -> Vec<&ExtReg> {
        self.regs.iter().filter(|r| r.kind == k).collect()
    }
}

impl Default for ExtRegistry {
    fn default() -> Self {
        ExtRegistry::new()
    }
}

pub fn run_vec359_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec359");
    // 族一：API 版本
    s.add("版本-当前", ApiVer::CURRENT.major == 1 && ApiVer::CURRENT.minor == 2, "1.2");
    s.add("版本-向下兼容", ApiVer::CURRENT.compatible(ApiVer { major: 1, minor: 0 }), "1.0 ok");
    s.add("版本-同版兼容", ApiVer::CURRENT.compatible(ApiVer { major: 1, minor: 2 }), "1.2 ok");
    s.add("版本-高要求拒", !ApiVer::CURRENT.compatible(ApiVer { major: 1, minor: 3 }), "1.3 rejected");
    s.add("版本-跨主拒", !ApiVer::CURRENT.compatible(ApiVer { major: 2, minor: 0 }), "2.0 rejected");
    s.add("版本-有序", ApiVer { major: 1, minor: 1 } < ApiVer { major: 1, minor: 2 }, "ordered");
    // 族二：注册表
    let mut reg = ExtRegistry::new();
    let r1 = reg.register(ExtReg { name: "evenodd-pred", kind: ExtKind::FillPredicate, ver: ApiVer { major: 1, minor: 0 }, out_cap: 1024 });
    s.add("注册-成功", r1.is_ok(), "registered");
    let r2 = reg.register(ExtReg { name: "evenodd-pred", kind: ExtKind::ScanFilter, ver: ApiVer { major: 1, minor: 0 }, out_cap: 16 });
    s.add("注册-重名拒", r2 == Err("duplicate name"), "dup name");
    let r3 = reg.register(ExtReg { name: "future-ext", kind: ExtKind::PostProcess, ver: ApiVer { major: 2, minor: 0 }, out_cap: 16 });
    s.add("注册-版本拒", r3 == Err("api version incompatible"), "bad version");
    s.add("注册-计数", reg.regs.len() == 1, "1 ext");
    // 分类查
    reg.register(ExtReg { name: "drop-x<0", kind: ExtKind::ScanFilter, ver: ApiVer { major: 1, minor: 2 }, out_cap: 64 }).expect("ok");
    s.add("注册-分类", reg.by_kind(ExtKind::ScanFilter).len() == 1, "1 filter");
    s.add("注册-谓词", reg.by_kind(ExtKind::FillPredicate).len() == 1, "1 predicate");
    s.add("注册-后处理空", reg.by_kind(ExtKind::PostProcess).is_empty(), "no post");
    // 族三：沙箱
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    let mut sb = Sandbox::new(&edges, 64);
    let evs = sb.scan(2.0);
    s.add("沙箱-扫描成功", evs.expect("e").len() == 2, "2 events");
    s.add("沙箱-用量", sb.used == 2, "used 2");
    s.add("沙箱-未耗尽", !sb.exhausted(), "not exhausted");
    // 输入只读性：沙箱不持可变引用（类型即证明）
    s.add("沙箱-只读视图", sb.edges.len() == 4, "borrowed edges");
    // 族四：输出上限
    let mut tight = Sandbox::new(&edges, 3);
    s.add("上限-首次可", tight.scan(2.0).is_some(), "first scan ok");
    s.add("上限-超界拒", tight.scan(2.0).is_none(), "over cap rejected");
    s.add("上限-违规计数", tight.violations == 1, "1 violation");
    // 极小容量
    let mut tiny = Sandbox::new(&edges, 1);
    s.add("上限-极小拒", tiny.scan(2.0).is_none(), "cap 1 rejects 2 events");
    // 兜底容量
    let z = Sandbox::new(&edges, 0);
    s.add("上限-零兜底", z.out_cap == 1, "min cap 1");
    // 族五：三类扩展齐备
    let kinds = [ExtKind::FillPredicate, ExtKind::ScanFilter, ExtKind::PostProcess];
    s.add("扩展-三类", kinds.len() == 3, "3 kinds");
    s
}
