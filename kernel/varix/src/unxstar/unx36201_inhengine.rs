//! UNX-F36201-J1 继承计算引擎总设计（三层继承树基准模型）。
//! 判据：① 三层树基准 20 组构建求值结果与手算零差——基准机
//! （golden_ref_effective，独立扁平重导）逐组对平 + 5 组手写黄金锚
//! 逐位核对；② 引擎版本锚登记（INH_ENGINE_VER / SEG_BASE 双锚恒等，
//! 供 36202~36220 与 ktest 聚合消费）；③ 传播 / 规范序 / 求值三基元
//! 闭环自洽；④ 纯功能代码 ≥320 行。
//! 设计纪律：本模块是 B11 继承域的地基——Windows SECURITY_DESCRIPTOR
//! 继承语义的最小确定性模型（win32 ACL 继承：OI/CI/NP/IO 四位 +
//! INHERITED 标记；SE_DACL_PROTECTED 短路留给 36204；规范序重排
//! 留给 36205 消费但本模块落基线实现）。所有下游模块只准消费本
//! 模块的类型与基元，不准私设第二套树模型——第二套模型 = 语义
//! 漂移温床（B11 红线）。
//! 基准口径（本域冻结，与 win32 对齐的简化确定性形态）：
//!   - 容器子继承 CI 位的 ACE；叶对象子继承 OI 位的 ACE；
//!   - NP（NO_PROPAGATE）只传一代：副本上清 OI|CI；
//!   - IO（INHERIT_ONLY）不作用于本对象，但照常向子传播；副本清 IO；
//!   - 副本恒置 ID（INHERITED）位；
//!   - 规范序：显式 Deny < 显式 Allow < 继承 Deny < 继承 Allow，
//!     同组按收集序（近祖先、同祖按 ACE 原序）稳定；
//!   - 求值：规范序首遇匹配 Deny 即拒；Allow 累积，凑满即授。

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7b30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_TREE: u16 = SEG_BASE + 2; // 树结构非法（父越界/父非容器/容量爆）
pub const E_FLAGS: u16 = SEG_BASE + 3; // ACE 标志/种类越合法区
pub const E_EVAL: u16 = SEG_BASE + 4; // 求值与基准机零差破
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 黄金锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 引擎版本锚（登记给下游 36202~36220 消费——下游 drill 必须核对）。
pub const INH_ENGINE_VER: u32 = 1;

// ---- 继承位语义（win32 ACE header flags 与 SD control 子集） ---------------
pub const AI_OBJECT_INHERIT: u8 = 0x01; // OI：叶对象子继承
pub const AI_CONTAINER_INHERIT: u8 = 0x02; // CI：容器子继承
pub const AI_NO_PROPAGATE: u8 = 0x04; // NP：只传一代
pub const AI_INHERIT_ONLY: u8 = 0x08; // IO：不作用于本对象
pub const AI_INHERITED: u8 = 0x10; // ID：已继承标记
pub const AI_LEGAL: u8 = 0x1F;

pub const SE_DACL_PRESENT: u16 = 0x0004;
pub const SE_DACL_AUTO_INHERIT_REQ: u16 = 0x0100;
pub const SE_DACL_AUTO_INHERITED: u16 = 0x0400;
pub const SE_DACL_PROTECTED: u16 = 0x1000;

pub const AK_ALLOW: u8 = 0;
pub const AK_DENY: u8 = 1;
pub const V_GRANT: u8 = 0;
pub const V_DENY: u8 = 1;

/// 节点容量（三层基准远用不满；深树/批量面也在界内）。
pub const NODE_CAP: usize = 2048;

// ---- 领域类型 ---------------------------------------------------------------
/// ACE（运行态）。origin：显式 = 0；继承副本 = 来源祖先节点号 + 1
/// （0 值与显式区分——36212 溯源账消费此锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ace {
    pub sid: u16,
    pub mask: u32,
    pub flags: u8,
    pub kind: u8,
    pub origin: u32,
}

/// ACE 静态规格（常量表用——sid=0 表示缺省位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AceSpec {
    pub sid: u16,
    pub mask: u32,
    pub kind: u8,
    pub flags: u8,
}

pub const NONE_SPEC: AceSpec = AceSpec { sid: 0, mask: 0, kind: AK_ALLOW, flags: 0 };

impl AceSpec {
    pub const fn present(&self) -> bool {
        self.sid != 0
    }
}

/// 树节点（显式 DACL + 结构位）。
pub struct Node {
    pub id: u16,
    pub name: &'static str,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub is_container: bool,
    pub control: u16,
    pub aces: Vec<Ace>,
}

/// 继承树（确定性引擎载体）。
pub struct InhTree {
    pub nodes: Vec<Node>,
    pub ver: u32,
}

impl InhTree {
    pub fn new() -> InhTree {
        InhTree { nodes: Vec::new(), ver: INH_ENGINE_VER }
    }

    /// 挂节点（父必须存在且为容器；容量守恒）。
    pub fn add_node(
        &mut self,
        name: &'static str,
        parent: Option<usize>,
        is_container: bool,
        control: u16,
    ) -> Result<usize, u16> {
        if self.ver != INH_ENGINE_VER {
            return Err(E_VER);
        }
        if self.nodes.len() >= NODE_CAP {
            return Err(E_TREE);
        }
        if let Some(p) = parent {
            if p >= self.nodes.len() || !self.nodes[p].is_container {
                return Err(E_TREE);
            }
        }
        let id = self.nodes.len() as u16;
        self.nodes.push(Node {
            id,
            name,
            parent,
            children: Vec::new(),
            is_container,
            control,
            aces: Vec::new(),
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id as usize);
        }
        Ok(id as usize)
    }

    /// 写显式 DACL：显式面禁带 ID 位（继承标记由引擎写，不由调用方写
    /// ——36203 伪已继承位校验的地基）。
    pub fn set_dacl(&mut self, idx: usize, aces: Vec<Ace>) -> Result<(), u16> {
        if idx >= self.nodes.len() {
            return Err(E_TREE);
        }
        let mut i = 0;
        while i < aces.len() {
            let a = &aces[i];
            if a.flags & !AI_LEGAL != 0 || a.kind > AK_DENY {
                return Err(E_FLAGS);
            }
            if a.flags & AI_INHERITED != 0 {
                return Err(E_FLAGS);
            }
            i += 1;
        }
        self.nodes[idx].aces = aces;
        Ok(())
    }

    /// AceSpec 表装载（便捷面）。
    pub fn load_specs(&mut self, idx: usize, specs: &[AceSpec]) -> Result<(), u16> {
        let mut aces: Vec<Ace> = Vec::new();
        let mut i = 0;
        while i < specs.len() {
            let s = &specs[i];
            if s.present() {
                aces.push(Ace { sid: s.sid, mask: s.mask, flags: s.flags, kind: s.kind, origin: 0 });
            }
            i += 1;
        }
        self.set_dacl(idx, aces)
    }

    /// 收集继承副本（近祖先序；NP 只传一代；副本置 ID 清 IO，
    /// NP 副本再清 OI|CI——传播终止语义）。
    pub fn collect_inherited(&self, idx: usize) -> Result<Vec<Ace>, u16> {
        if idx >= self.nodes.len() {
            return Err(E_TREE);
        }
        let want = if self.nodes[idx].is_container {
            AI_CONTAINER_INHERIT
        } else {
            AI_OBJECT_INHERIT
        };
        let mut out: Vec<Ace> = Vec::new();
        let mut cur = self.nodes[idx].parent;
        let mut depth = 1u32;
        while let Some(p) = cur {
            if p >= self.nodes.len() {
                return Err(E_TREE);
            }
            let mut i = 0;
            while i < self.nodes[p].aces.len() {
                let a = self.nodes[p].aces[i];
                if a.flags & want != 0 && !(a.flags & AI_NO_PROPAGATE != 0 && depth > 1) {
                    let mut f = a.flags;
                    f |= AI_INHERITED;
                    f &= !AI_INHERIT_ONLY;
                    if f & AI_NO_PROPAGATE != 0 {
                        f &= !(AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT);
                    }
                    out.push(Ace { sid: a.sid, mask: a.mask, flags: f, kind: a.kind, origin: (p + 1) as u32 });
                }
                i += 1;
            }
            cur = self.nodes[p].parent;
            depth += 1;
        }
        Ok(out)
    }

    /// 有效 DACL：显式适用（非 IO）+ 继承副本 → 规范序合并。
    pub fn effective_dacl(&self, idx: usize) -> Result<Vec<Ace>, u16> {
        if idx >= self.nodes.len() {
            return Err(E_TREE);
        }
        let mut explicit: Vec<Ace> = Vec::new();
        let mut i = 0;
        while i < self.nodes[idx].aces.len() {
            let a = self.nodes[idx].aces[i];
            if a.flags & AI_INHERIT_ONLY == 0 {
                explicit.push(a);
            }
            i += 1;
        }
        let inherited = self.collect_inherited(idx)?;
        Ok(canonical_merge(explicit, inherited))
    }

    /// 节点深度（根 = 0；环不可能——add_node 结构保证父恒先在）。
    pub fn depth_of(&self, idx: usize) -> Result<u32, u16> {
        if idx >= self.nodes.len() {
            return Err(E_TREE);
        }
        let mut d = 0u32;
        let mut cur = self.nodes[idx].parent;
        while let Some(p) = cur {
            d += 1;
            cur = self.nodes[p].parent;
        }
        Ok(d)
    }

    /// 子树先序枚举（含自身）。
    pub fn subtree(&self, idx: usize, out: &mut Vec<usize>) -> Result<(), u16> {
        if idx >= self.nodes.len() {
            return Err(E_TREE);
        }
        out.push(idx);
        let mut k = 0;
        while k < self.nodes[idx].children.len() {
            let c = self.nodes[idx].children[k];
            self.subtree(c, out)?;
            k += 1;
        }
        Ok(())
    }
}

// ---- 规范序与求值基元 -------------------------------------------------------
/// 规范组：显式 Deny=0 < 显式 Allow=1 < 继承 Deny=2 < 继承 Allow=3。
pub fn canon_group(a: &Ace) -> u8 {
    let inh = a.flags & AI_INHERITED != 0;
    let deny = a.kind == AK_DENY;
    match (inh, deny) {
        (false, true) => 0,
        (false, false) => 1,
        (true, true) => 2,
        (true, false) => 3,
    }
}

/// 规范序合并（(组, 收集序) 全序排序——同组稳定性由收集序锚定，
/// 与排序算法稳定性无关）。
pub fn canonical_merge(mut explicit: Vec<Ace>, inherited: Vec<Ace>) -> Vec<Ace> {
    let mut tagged: Vec<(u8, u32, Ace)> = Vec::new();
    let mut seq = 0u32;
    let mut i = 0;
    while i < explicit.len() {
        let a = explicit[i];
        tagged.push((canon_group(&a), seq, a));
        seq += 1;
        i += 1;
    }
    let mut j = 0;
    while j < inherited.len() {
        let a = inherited[j];
        tagged.push((canon_group(&a), seq, a));
        seq += 1;
        j += 1;
    }
    tagged.sort_by(|x, y| {
        if x.0 != y.0 {
            x.0.cmp(&y.0)
        } else {
            x.1.cmp(&y.1)
        }
    });
    explicit.clear();
    let mut k = 0;
    while k < tagged.len() {
        explicit.push(tagged[k].2);
        k += 1;
    }
    explicit
}

/// 规范序校验（36205 消费——组号单调不减 + 同组收集序即序）。
pub fn is_canonical(acl: &[Ace]) -> bool {
    let mut prev_group = 0u8;
    let mut i = 0;
    while i < acl.len() {
        let g = canon_group(&acl[i]);
        if i > 0 && g < prev_group {
            return false;
        }
        prev_group = g;
        i += 1;
    }
    true
}

/// 求值基元：规范序首遇匹配 Deny 即拒；Allow 累积；凑满即授。
pub fn eval_request(acl: &[Ace], sid: u16, desired: u32) -> (u8, u32) {
    let mut granted = 0u32;
    let mut i = 0;
    while i < acl.len() {
        let a = &acl[i];
        if a.sid == sid {
            if a.kind == AK_DENY {
                if a.mask & desired != 0 {
                    return (V_DENY, granted);
                }
            } else {
                granted |= a.mask & desired;
                if granted & desired == desired {
                    return (V_GRANT, granted);
                }
            }
        }
        i += 1;
    }
    if granted & desired == desired {
        (V_GRANT, granted)
    } else {
        (V_DENY, granted)
    }
}

/// 引擎版本锚登记（下游消费面）。
pub fn engine_anchor() -> (u32, u16) {
    (INH_ENGINE_VER, SEG_BASE)
}

// ---- 三层树基准（20 组） ----------------------------------------------------
/// 基准机：独立扁平重导（不吃 InhTree 代码路径——手算口径的机器形态）。
/// ancestors[0] = 直接父，ancestors[1] = 祖父……近祖在前。
pub fn golden_ref_effective(
    is_container: bool,
    own: &[AceSpec],
    ancestors: &[&[AceSpec]],
) -> Vec<(u16, u32, u8, u8)> {
    let mut list: Vec<(u8, u32, (u16, u32, u8, u8))> = Vec::new();
    let mut seq = 0u32;
    // 显式适用（非 IO）。
    let mut i = 0;
    while i < own.len() {
        let s = &own[i];
        if s.present() && s.flags & AI_INHERIT_ONLY == 0 {
            list.push((group_of(s.flags, s.kind), seq, (s.sid, s.mask, s.kind, s.flags)));
            seq += 1;
        }
        i += 1;
    }
    // 继承：逐祖按位规则重导。
    let mut d = 0usize;
    while d < ancestors.len() {
        let depth = d + 1;
        let want = if is_container { AI_CONTAINER_INHERIT } else { AI_OBJECT_INHERIT };
        let mut k = 0;
        while k < ancestors[d].len() {
            let s = &ancestors[d][k];
            if s.present()
                && s.flags & want != 0
                && !(s.flags & AI_NO_PROPAGATE != 0 && depth > 1)
            {
                let mut f = s.flags;
                f |= AI_INHERITED;
                f &= !AI_INHERIT_ONLY;
                if f & AI_NO_PROPAGATE != 0 {
                    f &= !(AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT);
                }
                list.push((group_of(f, s.kind), seq, (s.sid, s.mask, s.kind, f)));
                seq += 1;
            }
            k += 1;
        }
        d += 1;
    }
    list.sort_by(|x, y| {
        if x.0 != y.0 {
            x.0.cmp(&y.0)
        } else {
            x.1.cmp(&y.1)
        }
    });
    let mut out: Vec<(u16, u32, u8, u8)> = Vec::new();
    let mut m = 0;
    while m < list.len() {
        out.push(list[m].2);
        m += 1;
    }
    out
}

fn group_of(flags: u8, kind: u8) -> u8 {
    let inh = flags & AI_INHERITED != 0;
    let deny = kind == AK_DENY;
    match (inh, deny) {
        (false, true) => 0,
        (false, false) => 1,
        (true, true) => 2,
        (true, false) => 3,
    }
}

/// 20 组基准规格（root/mid 显式面；叶自身面；叶容器性 = case%5==2；
/// 第二容器 mid1 = case%2==1，无 ACE——深度 2 传播的对照结构）。
const BENCH_ROOT: [[AceSpec; 2]; 20] = [
    [AceSpec { sid: 7, mask: 0xF, kind: AK_DENY, flags: AI_CONTAINER_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 7, mask: 0x3, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [
        AceSpec { sid: 5, mask: 0xF, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT },
        AceSpec { sid: 9, mask: 0x1, kind: AK_DENY, flags: AI_INHERIT_ONLY | AI_CONTAINER_INHERIT },
    ],
    [AceSpec { sid: 7, mask: 0xF, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x4, kind: AK_ALLOW, flags: AI_CONTAINER_INHERIT | AI_INHERIT_ONLY }, NONE_SPEC],
    [
        AceSpec { sid: 3, mask: 0x7, kind: AK_ALLOW, flags: AI_CONTAINER_INHERIT },
        AceSpec { sid: 4, mask: 0x2, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE },
    ],
    [AceSpec { sid: 6, mask: 0x5, kind: AK_DENY, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [
        AceSpec { sid: 8, mask: 0x3, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_NO_PROPAGATE | AI_INHERIT_ONLY },
        NONE_SPEC,
    ],
    [AceSpec { sid: 1, mask: 0xF, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT }, NONE_SPEC],
    [
        AceSpec { sid: 2, mask: 0x2, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT },
        AceSpec { sid: 3, mask: 0x4, kind: AK_DENY, flags: AI_CONTAINER_INHERIT | AI_NO_PROPAGATE },
    ],
    [AceSpec { sid: 5, mask: 0x6, kind: AK_ALLOW, flags: AI_CONTAINER_INHERIT }, NONE_SPEC],
    [
        AceSpec { sid: 4, mask: 0x8, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_INHERIT_ONLY },
        AceSpec { sid: 6, mask: 0x1, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT },
    ],
    [AceSpec { sid: 9, mask: 0x9, kind: AK_ALLOW, flags: AI_CONTAINER_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x3, kind: AK_DENY, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [
        AceSpec { sid: 7, mask: 0x7, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT },
        AceSpec { sid: 8, mask: 0x8, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE },
    ],
    [AceSpec { sid: 1, mask: 0x1, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_INHERIT_ONLY }, NONE_SPEC],
    [AceSpec { sid: 3, mask: 0xE, kind: AK_DENY, flags: AI_CONTAINER_INHERIT | AI_INHERIT_ONLY }, NONE_SPEC],
    [
        AceSpec { sid: 5, mask: 0x4, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE },
        AceSpec { sid: 7, mask: 0x2, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT },
    ],
    [AceSpec { sid: 6, mask: 0x6, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 9, mask: 0x7, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
];

const BENCH_MID: [[AceSpec; 2]; 20] = [
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 4, mask: 0x2, kind: AK_DENY, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 6, mask: 0x2, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 3, mask: 0x1, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x4, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
    [AceSpec { sid: 3, mask: 0x7, kind: AK_DENY, flags: AI_CONTAINER_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 8, mask: 0x4, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 1, mask: 0x8, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x2, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 5, mask: 0x2, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
    [AceSpec { sid: 4, mask: 0x8, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 9, mask: 0x1, kind: AK_DENY, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 7, mask: 0x4, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
    [AceSpec { sid: 1, mask: 0x1, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 3, mask: 0x6, kind: AK_DENY, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 5, mask: 0x4, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT }, NONE_SPEC],
    [AceSpec { sid: 6, mask: 0x2, kind: AK_DENY, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 9, mask: 0x7, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_NO_PROPAGATE }, NONE_SPEC],
];

const BENCH_LEAF: [[AceSpec; 2]; 20] = [
    [AceSpec { sid: 9, mask: 0x3, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 8, mask: 0x8, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x4, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 1, mask: 0x1, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 6, mask: 0x5, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 1, mask: 0x8, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 7, mask: 0x4, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 4, mask: 0x8, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x1, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 2, mask: 0x3, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 7, mask: 0x7, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
    [AceSpec { sid: 3, mask: 0x2, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 5, mask: 0x4, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [AceSpec { sid: 6, mask: 0x6, kind: AK_ALLOW, flags: 0 }, NONE_SPEC],
    [NONE_SPEC, NONE_SPEC],
];

pub const BENCH_N: usize = 20;

/// 构建第 case 组三层树（root / mid0 [+mid1] / 叶×n）。
pub fn bench_tree(case: usize) -> Result<(InhTree, usize, usize), u16> {
    if case >= BENCH_N {
        return Err(E_TREE);
    }
    let mut t = InhTree::new();
    let root = t.add_node("root", None, true, SE_DACL_PRESENT)?;
    let mid0 = t.add_node("mid0", Some(root), true, SE_DACL_PRESENT)?;
    if case % 2 == 1 {
        t.add_node("mid1", Some(root), true, SE_DACL_PRESENT)?;
    }
    let leaf0 = t.add_node("leaf0", Some(mid0), case % 5 == 2, SE_DACL_PRESENT)?;
    let n_leaf = 1 + case % 3;
    let mut k = 1;
    while k < n_leaf {
        t.add_node("leafx", Some(mid0), false, SE_DACL_PRESENT)?;
        k += 1;
    }
    t.load_specs(root, &BENCH_ROOT[case])?;
    t.load_specs(mid0, &BENCH_MID[case])?;
    t.load_specs(leaf0, &BENCH_LEAF[case])?;
    Ok((t, leaf0, mid0))
}

/// 引擎侧四元组投影（与基准机元组口径对齐）。
pub fn acl_tuples(acl: &[Ace]) -> Vec<(u16, u32, u8, u8)> {
    let mut out: Vec<(u16, u32, u8, u8)> = Vec::new();
    let mut i = 0;
    while i < acl.len() {
        out.push((acl[i].sid, acl[i].mask, acl[i].kind, acl[i].flags));
        i += 1;
    }
    out
}

/// 第 case 组对 leaf0 的手算期望（基准机独立重导）。
pub fn bench_expect(case: usize) -> Result<Vec<(u16, u32, u8, u8)>, u16> {
    if case >= BENCH_N {
        return Err(E_TREE);
    }
    let is_container = case % 5 == 2;
    Ok(golden_ref_effective(
        is_container,
        &BENCH_LEAF[case],
        &[&BENCH_MID[case], &BENCH_ROOT[case]],
    ))
}

/// 5 组手写黄金锚（case 0/1/2/3/4 的 leaf0 有效 DACL 逐位手算——
/// 基准机与引擎都要对齐到这三个字面上的锚，第三重独立来源）。
pub const HAND_ANCHORS: [(usize, &[(u16, u32, u8, u8)]); 5] = [
    // case 0：叶只继承自己显式 Allow；root CI 位不落到叶（叶非容器）。
    (0, &[(9, 0x3, AK_ALLOW, 0)]),
    // case 1：root OI Allow 传到叶（副本置 ID）；mid 显式 Deny 无 OI 不传。
    (1, &[(7, 0x3, AK_ALLOW, AI_OBJECT_INHERIT | AI_INHERITED)]),
    // case 2：叶是容器：显式 Allow + root 两条 CI（IO 副本清 IO）按
    // 继承 Deny < 继承 Allow 排序。
    (
        2,
        &[
            (8, 0x8, AK_ALLOW, 0),
            (9, 0x1, AK_DENY, AI_CONTAINER_INHERIT | AI_INHERITED),
            (5, 0xF, AK_ALLOW, AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_INHERITED),
        ],
    ),
    // case 3：root NP 传一代（叶是孙辈）+ mid 显式无 OI → 叶有效集为空。
    (3, &[]),
    // case 4：mid OI|NP 传一代副本（清 OI 留 NP）+ 自身显式；root CI|IO
    // 不落到叶（叶非容器）。
    (
        4,
        &[
            (2, 0x4, AK_ALLOW, 0),
            (2, 0x4, AK_ALLOW, AI_NO_PROPAGATE | AI_INHERITED),
        ],
    ),
];

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚登记（双锚恒等）。
    if VER != 1 || INH_ENGINE_VER != 1 || VER != INH_ENGINE_VER {
        return Err(E_VER);
    }
    let (av, ab) = engine_anchor();
    if av != INH_ENGINE_VER || ab != SEG_BASE {
        return Err(E_ANCHOR);
    }
    // R2：20 组构建求值与基准机零差（leaf0 全组 + mid0 抽验）。
    let mut case = 0;
    while case < BENCH_N {
        let (t, leaf0, mid0) = bench_tree(case)?;
        let got_leaf = acl_tuples(&t.effective_dacl(leaf0)?);
        if got_leaf != bench_expect(case)? {
            return Err(E_EVAL);
        }
        let got_mid = acl_tuples(&t.effective_dacl(mid0)?);
        let want_mid = golden_ref_effective(true, &BENCH_MID[case], &[&BENCH_ROOT[case]]);
        if got_mid != want_mid {
            return Err(E_EVAL);
        }
        case += 1;
    }
    // R3：手写黄金锚逐位核对（第三重独立来源）。
    let mut a = 0;
    while a < HAND_ANCHORS.len() {
        let (case, want) = HAND_ANCHORS[a];
        let got = bench_expect(case)?;
        if got.len() != want.len() {
            return Err(E_ANCHOR);
        }
        let mut i = 0;
        while i < want.len() {
            if got[i] != want[i] {
                return Err(E_ANCHOR);
            }
            i += 1;
        }
        a += 1;
    }
    // R4：求值基元（Deny 首遇即拒 / Allow 累积凑满即授 / 不凑满拒）。
    let (t, leaf0, _) = bench_tree(0)?;
    let acl = t.effective_dacl(leaf0)?;
    let (v, g) = eval_request(&acl, 9, 0x3);
    if v != V_GRANT || g != 0x3 {
        return Err(E_EVAL);
    }
    let (v2, _) = eval_request(&acl, 8, 0x3);
    if v2 != V_DENY {
        return Err(E_EVAL);
    }
    let (v3, g3) = eval_request(&acl, 9, 0x8);
    if v3 != V_DENY || g3 != 0 {
        return Err(E_EVAL);
    }
    // R5：结构守卫（父越界 / 父非容器 / 显式面带 ID / 越界读）。
    let mut t2 = InhTree::new();
    t2.add_node("root", None, false, 0)?; // 根可以是叶
    if t2.add_node("bad", Some(0), true, 0).err() != Some(E_TREE) {
        return Err(E_TREE);
    }
    t2.add_node("mid", None, true, 0)?;
    let mut bad_aces: Vec<Ace> = Vec::new();
    bad_aces.push(Ace { sid: 1, mask: 1, flags: AI_INHERITED, kind: AK_ALLOW, origin: 0 });
    if t2.set_dacl(1, bad_aces).err() != Some(E_FLAGS) {
        return Err(E_FLAGS);
    }
    if t2.effective_dacl(9).err() != Some(E_TREE) {
        return Err(E_TREE);
    }
    // 校验和：20 组有效 ACE 总数折叠（确定性总锚）。
    let mut sum = 0u32;
    case = 0;
    while case < BENCH_N {
        let (t, leaf0, _) = bench_tree(case)?;
        sum = sum.wrapping_add(t.effective_dacl(leaf0)?.len() as u32);
        sum = sum.wrapping_mul(3);
        case += 1;
    }
    Ok(sum.wrapping_add(BENCH_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TREE => "树结构非法（父越界/父非容器/容量爆）",
        E_FLAGS => "ACE 标志/种类越合法区（显式面带 ID 位等）",
        E_EVAL => "求值/传播与基准机零差破",
        E_ANCHOR => "黄金锚漂移（手算锚不符）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36201_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36201_inhengine");
    cs.add("SEG 段位独占", SEG_BASE == 0x7b30, "0x7b30~0x7b3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "20 组零差/5 锚逐位/求值基元/结构守卫全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("版本锚登记", VER == 1 && INH_ENGINE_VER == 1 && engine_anchor() == (1, 0x7b30), "引擎双锚恒等登记");
    cs.add("基准组数", BENCH_N == 20 && HAND_ANCHORS.len() == 5, "三层树基准 20 组 + 手写锚 5 组");
    cs.add("规范序基线", is_canonical(&{ let (t, l, _) = bench_tree(2).ok().unwrap_or_else(|| (InhTree::new(), 0, 0)); t.effective_dacl(l).ok().unwrap_or_default() }), "case2 有效 DACL 落规范序");
    cs.add("继承位闭集", AI_LEGAL == 0x1F && SE_DACL_PROTECTED == 0x1000, "OI|CI|NP|IO|ID 五位闭集 + PROTECTED 锚");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36201_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_np_stops_at_one_generation() {
        // 性质直证：root NP ACE 传 mid（一代）但不传 leaf（两代）。
        let mut t = InhTree::new();
        let root = t.add_node("root", None, true, 0).ok().unwrap_or(0);
        let mid = t.add_node("mid", Some(root), true, 0).ok().unwrap_or(0);
        let leaf = t.add_node("leaf", Some(mid), false, 0).ok().unwrap_or(0);
        let mut aces: Vec<Ace> = Vec::new();
        aces.push(Ace { sid: 3, mask: 0x7, kind: AK_ALLOW, flags: AI_OBJECT_INHERIT | AI_CONTAINER_INHERIT | AI_NO_PROPAGATE, origin: 0 });
        let _ = t.set_dacl(root, aces);
        let mid_eff = t.effective_dacl(mid).ok().unwrap_or_default();
        assert_eq!(mid_eff.len(), 1);
        assert_eq!(mid_eff[0].flags & AI_INHERITED, AI_INHERITED);
        assert_eq!(mid_eff[0].flags & AI_OBJECT_INHERIT, 0, "NP 副本清 OI——传播终止");
        let leaf_eff = t.effective_dacl(leaf).ok().unwrap_or_default();
        assert_eq!(leaf_eff.len(), 0, "NP 第二代不传播");
    }

    #[test]
    fn test_hand_anchor_case2_order() {
        // 账面守恒：case2 容器叶的三段规范序（显式 Allow < 继承 Deny < 继承 Allow）。
        let (t, leaf0, _) = bench_tree(2).ok().unwrap_or((InhTree::new(), 0, 0));
        let eff = t.effective_dacl(leaf0).ok().unwrap_or_default();
        assert_eq!(eff.len(), 3);
        assert_eq!(canon_group(&eff[0]), 1);
        assert_eq!(canon_group(&eff[1]), 2);
        assert_eq!(canon_group(&eff[2]), 3);
        assert!(is_canonical(&eff));
    }
}
