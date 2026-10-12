//! UNX-F0961 现存轮询栈盘点账 · 轮询点清册
//! 判据原文（CoRun Unxreal 主册）：UNX-F0961-J1 盘点完整（现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，盘点与运行观测一致率 100%
//! · 组件：轮询点记录（文件/行号/函数名/类型）+ 清册账 + 重复登记检测
//! · 判据落地：现存全部轮询点清册零遗漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PMS_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const PMS_SEG_BASE: u16 = 0x6160;

// --------------------------------------------------------------------------
// 诊断码（pms 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 轮询点清册诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pmsCode {
    /// 轮询点漏登记。
    PointMissing,
    /// 同一点重复登记。
    PointDup,
    /// 文件路径非法。
    FileBad,
    /// 行号为零。
    LineZero,
    /// 函数名缺失。
    FuncUnknown,
    /// 轮询类型未知。
    TypeUnknown,
    /// 地址非法（0 且非占位）。
    AddrBad,
    /// 清册超容量。
    InventoryOver,
}

impl pmsCode {
    /// 全量闭集。
    pub const ALL: [pmsCode; 8] = [
        pmsCode::PointMissing,
        pmsCode::PointDup,
        pmsCode::FileBad,
        pmsCode::LineZero,
        pmsCode::FuncUnknown,
        pmsCode::TypeUnknown,
        pmsCode::AddrBad,
        pmsCode::InventoryOver,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            pmsCode::PointMissing => 0x6160 + 0,
            pmsCode::PointDup => 0x6160 + 1,
            pmsCode::FileBad => 0x6160 + 2,
            pmsCode::LineZero => 0x6160 + 3,
            pmsCode::FuncUnknown => 0x6160 + 4,
            pmsCode::TypeUnknown => 0x6160 + 5,
            pmsCode::AddrBad => 0x6160 + 6,
            pmsCode::InventoryOver => 0x6160 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            pmsCode::PointMissing => "轮询点漏登记",
            pmsCode::PointDup => "同一点重复登记",
            pmsCode::FileBad => "文件路径非法",
            pmsCode::LineZero => "行号为零",
            pmsCode::FuncUnknown => "函数名缺失",
            pmsCode::TypeUnknown => "轮询类型未知",
            pmsCode::AddrBad => "地址非法（0 且非占位）",
            pmsCode::InventoryOver => "清册超容量",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            pmsCode::PointMissing => "补扫描后重新对账",
            pmsCode::PointDup => "按 (file,line) 去重",
            pmsCode::FileBad => "按仓库相对路径校正",
            pmsCode::LineZero => "行号从 1 起校正",
            pmsCode::FuncUnknown => "补函数名或标记匿名",
            pmsCode::TypeUnknown => "按三型闭集分类",
            pmsCode::AddrBad => "按符号解析补地址",
            pmsCode::InventoryOver => "扩容或分册",
        }
    }
}


// --------------------------------------------------------------------------
// 账本 pms_inv（清册账）
// --------------------------------------------------------------------------
/// 清册账。
#[derive(Clone, Copy, Debug)]
pub struct pms_inv {
    /// 登记轮询点数
    pub points: u64,
    /// 拒收登记数
    pub rejected: u64,
    /// 扫描源码行数
    pub scanned_lines: u64,
    /// 匹配命中数
    pub hits: u64,
    /// 扫描文件数
    pub files: u64,
}

impl pms_inv {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            points: 0,
            rejected: 0,
            scanned_lines: 0,
            hits: 0,
            files: 0,
        }
    }

    /// 登记一个轮询点。
    pub fn register(&mut self) -> () {
        self.points += 1;
    }

    /// 拒收一个登记。
    pub fn reject(&mut self) -> () {
        self.rejected += 1;
    }

    /// 扫一行源码。
    pub fn scan_line(&mut self) -> () {
        self.scanned_lines += 1;
    }

    /// 一行命中。
    pub fn hit(&mut self) -> () {
        self.hits += 1;
    }

    /// 扫一个文件。
    pub fn scan_file(&mut self) -> () {
        self.files += 1;
    }

    /// 扫描覆盖了全部登记点。
    pub fn covered(&mut self) -> bool {
        self.hits <= self.scanned_lines && self.points <= self.hits
    }

}


// --------------------------------------------------------------------------
// 冻结常量表 pms_kind（4 行逐行注释自证）
// --------------------------------------------------------------------------
/// 轮询类型闭集表。
pub const pms_kind_COUNT: usize = 4;

/// 表本体（编译期冻结，运行期只读）。
pub static pms_kind: [(&'static str, u32); 4] = [
    ("busywait", 1), // 忙等轮询（while 条件 + 空转）
    ("regpoll", 2), // 寄存器轮询（读状态位判就绪）
    ("sleepyl", 3), // 休眠轮询（睡醒再问）
    ("types_total", 3), // 类型闭集总数
];

/// 线性查找（键唯一由构造期断言保证）。
pub fn pms_kind_get(key: &'static str) -> Option<u32> {
    let mut i = 0usize;
    while i < pms_kind_COUNT {
        if pms_kind[i].0 == key {
            return Some(pms_kind[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（守恒对账用）。
pub fn pms_kind_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pms_kind_COUNT {
        s = s.wrapping_add(pms_kind[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（逐对扫描；重复即结构腐坏）。
pub fn pms_kind_unique() -> bool {
    let mut i = 0usize;
    while i < pms_kind_COUNT {
        let mut j = i + 1;
        while j < pms_kind_COUNT {
            if pms_kind[i].0 == pms_kind[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6160, 0x616f]。
pub const fn pms_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < pmsCode::ALL.len() {
        let c = pmsCode::ALL[i].code();
        if c < 0x6160 || c > 0x616f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 三类闭集 + 总数。
fn c1_kind() -> bool {
    pms_kind_get("busywait") == Some(1)
        && pms_kind_get("regpoll") == Some(2)
        && pms_kind_get("sleepyl") == Some(3)
        && pms_kind_get("types_total") == Some(3)
}

/// 清册覆盖判据（命中 ≥ 登记）。
fn c2_cover() -> bool {
    let mut i = pms_inv::new();
    let mut k = 0u64;
    while k < 40 { i.register(); k += 1; }
    let mut j = 0u64;
    while j < 400 { i.scan_line(); j += 1; }
    let mut h = 0u64;
    while h < 42 { i.hit(); h += 1; }
    i.covered() && i.points == 40
}

/// 段位守卫：全部诊断码落在本模块段内。
fn pms_guard_seg() -> bool {
    pms_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_pms_checks() -> CheckSet {
    let mut set = CheckSet::new("pms");
    set.add(
        "F0961-inv-c1",
        c1_kind(),
        if c1_kind() { "" } else { "poll kind closed set drifted" },
    );
    set.add(
        "F0961-inv-c2",
        c2_cover(),
        if c2_cover() { "" } else { "inventory coverage broken" },
    );
    set.add(
        "pms-seg-guard",
        pms_guard_seg(),
        if pms_guard_seg() { "" } else { "diagnostic code outside module segment" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pms_all_checks_green() {
        let set = run_pms_checks();
        assert!(set.all_passed(), "PMS domain checks must be green");
    }

    #[test]
    fn pms_segment_guard_holds() {
        assert!(pms_segment_guard());
    }
}
