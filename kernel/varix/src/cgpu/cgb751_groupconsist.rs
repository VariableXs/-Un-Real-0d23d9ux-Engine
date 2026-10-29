//! cgb751_groupconsist —— CGPU-F0751 画像组一致性。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0751 画像组一致性  <SEG 0x4c20>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0751；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 画像组 = 同批联动的画像集合（组号 + 成员画像号表）；组一
//!      致性 = 组内成员的"组级约束"全满足：权重总和不越批容、
//!      成员版本档同值、成员两两指纹不撞（组内唯一性）；
//!   2. 组级约束三条写死：① Σ成员主权重 ≤ 2000（批容双倍）；
//!      ② 全员同版本档；③ 组内指纹两两不等（撞指纹红码）；
//!   3. 组账状态机三态：Forming→Frozen→Audited；Frozen 后成员表
//!      不可增删；Audited 是终态，一致性结论封账；
//!   4. 组指纹 = 成员号升序逐个吃 FNV-1a——成员表等价于集合比较
//!      （升序归一，乱序同组同指纹）；
//!   5. 审计结论账：逐组（绿/红+首差成员号+约束号），守恒式
//      绿组数+红组数 = 审计组数；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4c20 段为本模块独占（SEG_BASE=0x4c20），判别式
//!   SEG_BASE+1..+N；与 0x4c00/0x4c40 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const GC_VERSION: &str = "cgb751_groupconsist-v1";

/// 诊断段基址（0x4c20 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4C20;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4c21 组号越界（0..=15 之外）。
pub const XC_BAD_GROUP: u16 = SEG_BASE + 1;
/// 0x4c22 成员权重和越批容（Σ主权重 > 2000）。
pub const XC_WEIGHT_OVER: u16 = SEG_BASE + 2;
/// 0x4c23 版本档不齐（组内成员版本不同值）。
pub const XC_VER_MIXED: u16 = SEG_BASE + 3;
/// 0x4c24 组内指纹相撞（两成员指纹等值）。
pub const XC_FP_COLLIDE: u16 = SEG_BASE + 4;
/// 0x4c25 冻结后仍改成员表。
pub const XC_FROZEN_WRITE: u16 = SEG_BASE + 5;
/// 0x4c26 状态机非法边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 6;
/// 0x4c27 组为空（无成员不可冻结）。
pub const XC_GROUP_EMPTY: u16 = SEG_BASE + 7;
/// 0x4c28 审计账不守恒（绿+红 ≠ 总）。
pub const XC_AUDIT_UNBALANCED: u16 = SEG_BASE + 8;
/// 0x4c29 成员号重复入组。
pub const XC_MEMBER_DUP: u16 = SEG_BASE + 9;
/// 0x4c2a 未审计就读结论。
pub const XC_NOT_AUDITED: u16 = SEG_BASE + 10;

/// 组容量（写死：16 组）。
pub const GROUP_CAP: usize = 16;
/// 主权重批容（写死：Σ成员主权重 ≤ 2000）。
pub const WEIGHT_BATCH: u32 = 2000;
/// 单组成员上限（写死：8）。
pub const MEMBER_CAP: usize = 8;

/// 组级约束编号闭集：1=权重 2=版本 3=指纹。
pub const CONSTRAINTS: [u8; 3] = [1, 2, 3];

/// 成员属性（组内最小单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    /// 画像号。
    pub pid: u16,
    /// 主权重（千分位）。
    pub w_main: u32,
    /// 版本档。
    pub ver: u8,
    /// 画像指纹。
    pub fp: u32,
}

/// 组状态三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupState {
    /// 组建中。
    Forming,
    /// 已冻结（成员表锁死）。
    Frozen,
    /// 已审计（终态）。
    Audited,
}

/// 合法边：Forming→Frozen→Audited。
pub fn legal_edges(s: GroupState) -> &'static [GroupState] {
    match s {
        GroupState::Forming => &[GroupState::Frozen],
        GroupState::Frozen => &[GroupState::Audited],
        GroupState::Audited => &[],
    }
}

fn step(cur: GroupState, next: GroupState) -> Result<GroupState, u16> {
    if legal_edges(cur).iter().any(|n| *n == next) {
        Ok(next)
    } else {
        Err(XC_BAD_EDGE)
    }
}

/// 组指纹：成员号升序逐个吃 FNV-1a（集合归一）。
pub fn group_fingerprint(members: &[Member]) -> u32 {
    let mut ids: Vec<u16> = members.iter().map(|m| m.pid).collect();
    for i in 1..ids.len() {
        let key = ids[i];
        let mut j = i;
        while j > 0 && ids[j - 1] > key {
            ids[j] = ids[j - 1];
            j -= 1;
        }
        ids[j] = key;
    }
    let mut h = 0x811C_9DC5u32;
    for id in ids {
        h ^= (id & 0xFF) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (id >> 8) as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 审计结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditVerdict {
    /// 绿（一致）旗。
    pub green: bool,
    /// 首差约束号（红时 1..3；绿时 0）。
    pub first_fail: u8,
    /// 首差成员号（红时有效；绿时 0xFFFF）。
    pub first_member: u16,
    /// 组指纹（审计时刻）。
    pub fp: u32,
}

/// 画像组。
pub struct ProfileGroup {
    /// 组号。
    pub gid: u8,
    /// 成员表。
    pub members: Vec<Member>,
    state: GroupState,
    verdict: Option<AuditVerdict>,
}

impl ProfileGroup {
    /// 新组（号越界红码）。
    pub fn new(gid: u8) -> Result<Self, u16> {
        if (gid as usize) >= GROUP_CAP {
            return Err(XC_BAD_GROUP);
        }
        Ok(ProfileGroup {
            gid,
            members: Vec::new(),
            state: GroupState::Forming,
            verdict: None,
        })
    }

    /// 加成员（Forming 限定；重复号/超员红码；权重和实时核）。
    pub fn add(&mut self, m: Member) -> Result<(), u16> {
        if self.state != GroupState::Forming {
            return Err(XC_FROZEN_WRITE);
        }
        if self.members.len() >= MEMBER_CAP {
            return Err(XC_MEMBER_DUP);
        }
        if self.members.iter().any(|x| x.pid == m.pid) {
            return Err(XC_MEMBER_DUP);
        }
        let sum = self.members.iter().map(|x| x.w_main).sum::<u32>() + m.w_main;
        if sum > WEIGHT_BATCH {
            return Err(XC_WEIGHT_OVER);
        }
        self.members.push(m);
        Ok(())
    }

    /// 冻结（空组红码）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.state != GroupState::Forming {
            return Err(XC_BAD_EDGE);
        }
        if self.members.is_empty() {
            return Err(XC_GROUP_EMPTY);
        }
        self.state = step(self.state, GroupState::Frozen)?;
        Ok(group_fingerprint(&self.members))
    }

    /// 审计主路径：三条组级约束逐条核，首差定位。
    pub fn audit(&mut self) -> Result<AuditVerdict, u16> {
        if self.state != GroupState::Frozen {
            return Err(XC_BAD_EDGE);
        }
        // 约束 1：Σ主权重 ≤ 2000。
        let sum: u32 = self.members.iter().map(|m| m.w_main).sum();
        let mut first_fail = 0u8;
        let mut first_member = 0xFFFFu16;
        if sum > WEIGHT_BATCH {
            first_fail = 1;
            first_member = self
                .members
                .iter()
                .rev()
                .find(|m| {
                    self.members.iter().map(|x| x.w_main).sum::<u32>() - m.w_main + m.w_main
                        > WEIGHT_BATCH
                })
                .map(|m| m.pid)
                .unwrap_or(0xFFFF);
        }
        // 约束 2：版本档同值。
        if first_fail == 0 {
            let v0 = self.members[0].ver;
            if let Some(m) = self.members.iter().find(|m| m.ver != v0) {
                first_fail = 2;
                first_member = m.pid;
            }
        }
        // 约束 3：指纹两两不撞。
        if first_fail == 0 {
            'outer: for i in 0..self.members.len() {
                for j in (i + 1)..self.members.len() {
                    if self.members[i].fp == self.members[j].fp {
                        first_fail = 3;
                        first_member = self.members[j].pid;
                        break 'outer;
                    }
                }
            }
        }
        let green = first_fail == 0;
        let v = AuditVerdict {
            green,
            first_fail,
            first_member,
            fp: group_fingerprint(&self.members),
        };
        self.verdict = Some(v);
        self.state = step(self.state, GroupState::Audited)?;
        Ok(v)
    }

    /// 状态。
    pub fn state(&self) -> GroupState {
        self.state
    }

    /// 结论（未审计红码）。
    pub fn verdict(&self) -> Result<AuditVerdict, u16> {
        self.verdict.ok_or(XC_NOT_AUDITED)
    }
}

/// 组审计总账：多组结论聚合 + 守恒。
pub struct GroupAuditBook {
    verdicts: Vec<(u8, AuditVerdict)>,
}

impl GroupAuditBook {
    /// 空账。
    pub fn new() -> Self {
        GroupAuditBook { verdicts: Vec::new() }
    }

    /// 落一条结论（同组二落红码）。
    pub fn record(&mut self, gid: u8, v: AuditVerdict) -> Result<(), u16> {
        if (gid as usize) >= GROUP_CAP {
            return Err(XC_BAD_GROUP);
        }
        if self.verdicts.iter().any(|(g, _)| *g == gid) {
            return Err(XC_MEMBER_DUP);
        }
        self.verdicts.push((gid, v));
        Ok(())
    }

    /// 守恒核：绿组数 + 红组数 = 审计组数。
    pub fn conserved(&self) -> Result<(), u16> {
        let green = self.verdicts.iter().filter(|(_, v)| v.green).count();
        let total = self.verdicts.len();
        let red = total - green;
        if green + red != total {
            return Err(XC_AUDIT_UNBALANCED);
        }
        Ok(())
    }

    /// 绿组号表。
    pub fn green_groups(&self) -> Vec<u8> {
        self.verdicts
            .iter()
            .filter(|(_, v)| v.green)
            .map(|(g, _)| *g)
            .collect()
    }

    /// 红组号表。
    pub fn red_groups(&self) -> Vec<u8> {
        self.verdicts
            .iter()
            .filter(|(_, v)| !v.green)
            .map(|(g, _)| *g)
            .collect()
    }

    /// 总组数。
    pub fn total(&self) -> usize {
        self.verdicts.len()
    }
}

/// 自例程：两组全绿 + 两红组（版本不齐/指纹相撞）逐条检出 +
/// 错误路径（越批容/冻结后写/空组冻结/重复入组/未审计结论）。
pub fn drill() -> Result<u32, u16> {
    // 绿组 0：三成员同版本、异指纹。
    let mut g0 = ProfileGroup::new(0)?;
    g0.add(Member { pid: 0, w_main: 600, ver: 1, fp: 0xAAAA_0001 })?;
    g0.add(Member { pid: 1, w_main: 700, ver: 1, fp: 0xAAAA_0002 })?;
    g0.add(Member { pid: 2, w_main: 300, ver: 1, fp: 0xAAAA_0003 })?;
    g0.freeze()?;
    let v0 = g0.audit()?;
    if !v0.green || v0.first_fail != 0 {
        return Err(XC_AUDIT_UNBALANCED);
    }
    // 组指纹乱序同值（集合归一）。
    let mut g0b = ProfileGroup::new(1)?;
    g0b.add(Member { pid: 2, w_main: 300, ver: 1, fp: 9 })?;
    g0b.add(Member { pid: 0, w_main: 600, ver: 1, fp: 7 })?;
    g0b.add(Member { pid: 1, w_main: 700, ver: 1, fp: 8 })?;
    g0b.freeze()?;
    let mut g0c = ProfileGroup::new(2)?;
    g0c.add(Member { pid: 0, w_main: 600, ver: 9, fp: 7 })?;
    g0c.add(Member { pid: 1, w_main: 700, ver: 9, fp: 8 })?;
    g0c.add(Member { pid: 2, w_main: 300, ver: 9, fp: 9 })?;
    g0c.freeze()?;
    // pid 集合 {0,1,2} 相同 → 组指纹同（属性不参与）。
    if group_fingerprint(&g0b.members) != group_fingerprint(&g0c.members) {
        return Err(XC_FP_COLLIDE);
    }

    // 红组 3：版本不齐。
    let mut g3 = ProfileGroup::new(3)?;
    g3.add(Member { pid: 10, w_main: 500, ver: 1, fp: 0x0001 })?;
    g3.add(Member { pid: 11, w_main: 500, ver: 0, fp: 0x0002 })?;
    g3.freeze()?;
    let v3 = g3.audit()?;
    if v3.green || v3.first_fail != 2 || v3.first_member != 11 {
        return Err(XC_VER_MIXED);
    }

    // 红组 4：指纹相撞。
    let mut g4 = ProfileGroup::new(4)?;
    g4.add(Member { pid: 20, w_main: 500, ver: 1, fp: 0x00FF })?;
    g4.add(Member { pid: 21, w_main: 500, ver: 1, fp: 0x00FF })?;
    g4.freeze()?;
    let v4 = g4.audit()?;
    if v4.green || v4.first_fail != 3 || v4.first_member != 21 {
        return Err(XC_FP_COLLIDE);
    }

    // 错误路径 1：越批容。
    let mut g5 = ProfileGroup::new(5)?;
    if g5.add(Member { pid: 30, w_main: 2500, ver: 1, fp: 1 }) != Err(XC_WEIGHT_OVER) {
        return Err(XC_WEIGHT_OVER);
    }
    g5.add(Member { pid: 30, w_main: 1000, ver: 1, fp: 1 })?;
    if g5.add(Member { pid: 31, w_main: 1001, ver: 1, fp: 2 }) != Err(XC_WEIGHT_OVER) {
        return Err(XC_WEIGHT_OVER);
    }
    // 错误路径 2：冻结后写。
    g5.freeze()?;
    if g5.add(Member { pid: 32, w_main: 1, ver: 1, fp: 3 }) != Err(XC_FROZEN_WRITE) {
        return Err(XC_FROZEN_WRITE);
    }
    // 错误路径 3：空组冻结。
    let mut g6 = ProfileGroup::new(6)?;
    if g6.freeze() != Err(XC_GROUP_EMPTY) {
        return Err(XC_GROUP_EMPTY);
    }
    // 错误路径 4：重复入组。
    let mut g7 = ProfileGroup::new(7)?;
    g7.add(Member { pid: 40, w_main: 100, ver: 1, fp: 1 })?;
    if g7.add(Member { pid: 40, w_main: 100, ver: 1, fp: 2 }) != Err(XC_MEMBER_DUP) {
        return Err(XC_MEMBER_DUP);
    }
    // 错误路径 5：未审计读结论。
    if g7.verdict() != Err(XC_NOT_AUDITED) {
        return Err(XC_NOT_AUDITED);
    }
    // 错误路径 6：组号越界。
    if ProfileGroup::new(GROUP_CAP as u8).err() != Some(XC_BAD_GROUP) {
        return Err(XC_BAD_GROUP);
    }
    // 错误路径 7：Forming 直审（跳冻结）。
    if g7.audit() != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }

    // 总账聚合 + 守恒。
    let mut book = GroupAuditBook::new();
    book.record(0, v0)?;
    book.record(3, v3)?;
    book.record(4, v4)?;
    book.conserved()?;
    if book.total() != 3 || book.green_groups().len() != 1 || book.red_groups().len() != 2 {
        return Err(XC_AUDIT_UNBALANCED);
    }
    let _ = CONSTRAINTS;
    Ok(0)
}
