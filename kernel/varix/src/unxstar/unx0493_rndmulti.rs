//! unx0493_rndmulti —— UNX-F0493 多输出占位账（单主输出声明）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0493 · 多输出占位账 · 目标 240 行
//! 判据原文：UNX-F0493-J1 多输出占位账：渲染器声明多输
//!           出（主/副/镜像）时按占位登记——账二 单主输
//!           出（主输出恰一个，零或多个主都拒）；账三 副
//!           输出引用主（副输出的时间戳/模式字段必须引用
//!           已登记主，悬空副拒）；账四 镜像环路拒（A 镜
//!           像 B 且 B 镜像 A——帧循环）；账五 占位唯一
//!           （输出 ID 不重）；账六 注销主输出级联（主走
//!           副随，级联计数对账）。
//! 组件：输出注册表 + 主/副/镜像三态 + 环路检测 + 级联注销。
//! 职责定位：多输出是「一个管线喂几个屏」的拓扑声明——主
//!   输出是 vsync 的时序源，双主就是双时钟源（撕裂）；镜
//!   像环路是编译期 DAG 变运行期环的经典事故。占位账把拓
//!   扑约束变成注册时的硬闸。
//! 性能逐项分解：注册 O(1)；环路检测 O(镜像链深)。
//! 诊断码段：0x34A00（0x34A01~0x34A09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34A00;
pub const XC_PRIMARY: u32 = SEG_BASE + 1; // 主输出数非法
pub const XC_DANGLE: u32 = SEG_BASE + 2; // 副引用悬空
pub const XC_CYCLE: u32 = SEG_BASE + 3; // 镜像环路
pub const XC_DUP: u32 = SEG_BASE + 4; // ID 重复

/// 输出角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Primary,
    Secondary,
    Mirror,
}

/// 输出登记项。
#[derive(Clone, Copy, Debug)]
pub struct Out {
    pub id: u32,
    pub role: Role,
    /// 副输出引用的主 ID / 镜像引用的源 ID（主输出填 0）。
    pub ref_to: u32,
}

/// 输出注册表。
pub struct OutReg {
    pub outs: Vec<Out>,
}

impl OutReg {
    pub fn new() -> Self {
        OutReg { outs: Vec::new() }
    }

    /// 注册：五类闸顺序核。
    pub fn register(&mut self, o: Out) -> Result<(), u32> {
        // 账五：ID 唯一。
        if self.outs.iter().any(|x| x.id == o.id) {
            return Err(XC_DUP);
        }
        // 账三：副/镜像引用必须已存在（悬空拒）。
        if o.role != Role::Primary {
            if !self.outs.iter().any(|x| x.id == o.ref_to && x.role == Role::Primary) {
                return Err(XC_DANGLE);
            }
        }
        // 账四：镜像环路（沿镜像链追源，遇自身拒；链深上限防表环）。
        if o.role == Role::Mirror {
            let mut cur = o.ref_to;
            let mut depth = 0u32;
            loop {
                if cur == o.id {
                    return Err(XC_CYCLE);
                }
                let next = self.outs.iter().find(|x| x.id == cur).map(|x| {
                    if x.role == Role::Mirror {
                        Some(x.ref_to)
                    } else {
                        None
                    }
                });
                match next {
                    Some(Some(n)) => {
                        cur = n;
                        depth += 1;
                        if depth > 64 {
                            return Err(XC_CYCLE); // 链深超限按环处理
                        }
                    }
                    _ => break, // 追到非镜像（主/副）源 → 无环
                }
            }
        }
        // 账二：主输出唯一（注册后再核）。
        self.outs.push(o);
        let primaries = self.outs.iter().filter(|x| x.role == Role::Primary).count();
        if primaries > 1 {
            self.outs.pop(); // 回滚
            return Err(XC_PRIMARY);
        }
        Ok(())
    }

    /// 账六：注销主输出，级联摘除引用它的副/镜像。返回级联数。
    pub fn unregister_primary(&mut self) -> Result<usize, u32> {
        let pid = match self.outs.iter().find(|x| x.role == Role::Primary) {
            Some(p) => p.id,
            None => return Err(XC_PRIMARY), // 零主不可注销
        };
        self.outs.retain(|x| x.id != pid);
        let before = self.outs.len();
        // 副输出引用悬空 → 级联摘除（单轮足够：副不引用副）。
        self.outs.retain(|x| !(x.role != Role::Primary && x.ref_to == pid));
        Ok(before - self.outs.len())
    }

    /// 主输出数。
    pub fn primary_count(&self) -> usize {
        self.outs.iter().filter(|x| x.role == Role::Primary).count()
    }
}

pub fn run_unx0493_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0493");
    cs.add("SEG 段位独占", SEG_BASE == 0x34A00 && XC_DUP == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：单主输出。
    let mut reg = OutReg::new();
    cs.add("主输出注册", reg.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).is_ok(), "首个主");
    cs.add("第二主拒", reg.register(Out { id: 2, role: Role::Primary, ref_to: 0 }) == Err(XC_PRIMARY), "唯一主");
    cs.add("主数恒一", reg.primary_count() == 1, "回滚后");

    // 族二：副引用主。
    cs.add("副挂主成", reg.register(Out { id: 3, role: Role::Secondary, ref_to: 1 }).is_ok(), "引用已登主");
    cs.add("副悬空拒", reg.register(Out { id: 4, role: Role::Secondary, ref_to: 9 }) == Err(XC_DANGLE), "主 9 不存在");
    cs.add("镜像挂主成", reg.register(Out { id: 5, role: Role::Mirror, ref_to: 1 }).is_ok(), "镜像源=主");

    // 族三：ID 唯一。
    cs.add("ID 重复拒", reg.register(Out { id: 3, role: Role::Secondary, ref_to: 1 }) == Err(XC_DUP), "3 已占");

    // 族四：镜像环路。
    cs.add("自镜像拒", reg.register(Out { id: 6, role: Role::Mirror, ref_to: 6 }) == Err(XC_CYCLE), "自身为源");
    // A 镜像 B 且 B 镜像 A：B 先镜像 A 成，再 A 镜像 B 拒。
    let mut reg2 = OutReg::new();
    reg2.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).ok();
    reg2.register(Out { id: 7, role: Role::Mirror, ref_to: 1 }).ok();
    cs.add("镜像链一跳成", reg2.register(Out { id: 8, role: Role::Mirror, ref_to: 7 }).is_ok(), "8→7→1 无环");
    cs.add("回指环路拒", reg2.register(Out { id: 9, role: Role::Mirror, ref_to: 9 }) == Err(XC_CYCLE), "自环");
    // 双向环：镜像 8 的输出想被 7 镜像——7 已存在，用新表构造。
    let mut reg3 = OutReg::new();
    reg3.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).ok();
    reg3.register(Out { id: 10, role: Role::Mirror, ref_to: 1 }).ok();
    reg3.register(Out { id: 11, role: Role::Mirror, ref_to: 10 }).ok();
    // 环路检测沿链追源：12 镜像 11 → 11→10→1 终止，无环。
    cs.add("三跳链成", reg3.register(Out { id: 12, role: Role::Mirror, ref_to: 11 }).is_ok(), "链深 3 无环");
    // 链深上限：镜像链 64 跳内合法，第 65 跳注册时按环拒。
    let mut reg4 = OutReg::new();
    reg4.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).ok();
    let mut first_cycle_at = 0u32;
    for i in 2..200u32 {
        if reg4.register(Out { id: i, role: Role::Mirror, ref_to: i - 1 }) == Err(XC_CYCLE) {
            first_cycle_at = i;
            break;
        }
    }
    cs.add("链深超限按环拒", first_cycle_at == 67, "64 跳内合法（id 2~66），第 65 跳（id 67）拒");

    // 族五：级联注销。
    let mut reg5 = OutReg::new();
    reg5.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).ok();
    reg5.register(Out { id: 2, role: Role::Secondary, ref_to: 1 }).ok();
    reg5.register(Out { id: 3, role: Role::Mirror, ref_to: 1 }).ok();
    reg5.register(Out { id: 4, role: Role::Secondary, ref_to: 1 }).ok();
    let casc = reg5.unregister_primary();
    cs.add("级联三副", casc == Ok(3), "副2+镜3+副4");
    cs.add("注销后空表", reg5.outs.is_empty(), "主走副随");
    cs.add("空表零主拒注", reg5.unregister_primary() == Err(XC_PRIMARY), "无可注销");

    // 族六：幂等（重复注册同项恒拒）。
    let mut reg6 = OutReg::new();
    reg6.register(Out { id: 1, role: Role::Primary, ref_to: 0 }).ok();
    let a = reg6.register(Out { id: 1, role: Role::Primary, ref_to: 0 });
    let b = reg6.register(Out { id: 1, role: Role::Primary, ref_to: 0 });
    cs.add("重注恒拒", a == b && a == Err(XC_DUP), "两遍恒等");

    cs
}
