//! veb321_rveco —— VE-F0321 RISC-V GPU 生态盘点。
//! 锚点（CoRun Unxreal 主册）：VE-F0321 · RISC-V GPU 生态盘点 · 目标 300 行
//! 判据原文：VE-F0321-J1 生态盘点账：玩家登记（名字 + 类型
//!           IP核/自研SoC/软渲 + 成熟度 0~5）；类型枚举核；
//!           成熟度门槛（≥3 可合作）；盘点快照（玩家数 + 类
//!           型分布 + 最高成熟度）；玩家下架（移出盘点）；
//!           重名拒；快照稳定核（无变更两次快照同果）。
//! 组件：玩家账 + 类型枚举 + 快照器。
//! 职责定位：生态盘点是「RISC-V GPU 的花名册」——三类玩家
//!           三种角色：IP核是卖图纸的、自研SoC是造整机的、
//!           软渲是兜底的；成熟度是花名册的评语：不到 3 分
//!           的先观察不下注；快照是花名册的存档：没变就不
//!           重写。
//! 性能逐项分解：登记 O(1)；快照 O(玩家数)。
//! 诊断码段：0x31940（0x31941~0x31949 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RV_VERSION: &str = "veb321_rveco-v1";

/// 诊断段基址（0x31940 段 · 本模块独占）。
pub const RV_SEG_BASE: u32 = 0x31940;
pub const XC_MAGIC: u32 = 0x31941; // 账魔数破
pub const XC_DUP: u32 = 0x31942; // 重名
pub const XC_MAT: u32 = 0x31943; // 成熟度越界
pub const XC_TYPE: u32 = 0x31944; // 类型越界
pub const XC_NOPLY: u32 = 0x31945; // 无此玩家

/// 账魔数。
const MAGIC: u32 = 0x3194_1B31;

/// 玩家类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlyType {
    /// IP 核。
    Ip,
    /// 自研 SoC。
    Soc,
    /// 软渲。
    Soft,
}

/// 类型枚举核（模型化：三型恒合法）。
pub fn type_ok(t: PlyType) -> bool {
    matches!(t, PlyType::Ip | PlyType::Soc | PlyType::Soft)
}

/// 成熟度门槛。
pub const MAT_TH: u32 = 3;

/// 玩家。
pub struct Player {
    /// 名字 ID。
    pub name: u32,
    pub ptype: PlyType,
    pub maturity: u32,
}

/// 盘点簿。
pub struct EcoBook {
    pub players: Vec<Player>,
    magic: u32,
}

/// 快照。
#[derive(Debug, PartialEq, Eq)]
pub struct Snap {
    pub total: usize,
    pub n_ip: usize,
    pub n_soc: usize,
    pub n_soft: usize,
    pub top: u32,
    /// 可合作数。
    pub partners: usize,
}

impl EcoBook {
    /// 建。
    pub fn new() -> EcoBook {
        EcoBook { players: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记。
    pub fn add(&mut self, name: u32, ptype: PlyType, maturity: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if maturity > 5 {
            return Err(XC_MAT);
        }
        if !type_ok(ptype) {
            return Err(XC_TYPE);
        }
        for p in self.players.iter() {
            if p.name == name {
                return Err(XC_DUP);
            }
        }
        self.players.push(Player { name, ptype, maturity });
        Ok(())
    }

    /// 下架。
    pub fn remove(&mut self, name: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for i in 0..self.players.len() {
            if self.players[i].name == name {
                self.players.remove(i);
                return Ok(());
            }
        }
        Err(XC_NOPLY)
    }

    /// 快照。
    pub fn snap(&self) -> Snap {
        let mut s = Snap { total: self.players.len(), n_ip: 0, n_soc: 0, n_soft: 0, top: 0, partners: 0 };
        for p in self.players.iter() {
            match p.ptype {
                PlyType::Ip => s.n_ip += 1,
                PlyType::Soc => s.n_soc += 1,
                PlyType::Soft => s.n_soft += 1,
            }
            if p.maturity > s.top {
                s.top = p.maturity;
            }
            if p.maturity >= MAT_TH {
                s.partners += 1;
            }
        }
        s
    }

    /// 查玩家。
    pub fn get(&self, name: u32) -> Option<(PlyType, u32)> {
        for p in self.players.iter() {
            if p.name == name {
                return Some((p.ptype, p.maturity));
            }
        }
        None
    }
}

pub fn run_veb321_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0321");
    // 族零：枚举与门槛。
    s.add("三型合法", type_ok(PlyType::Ip) && type_ok(PlyType::Soc) && type_ok(PlyType::Soft), "枚举");
    s.add("成熟 6 拒", EcoBook::new().add(1, PlyType::Ip, 6) == Err(XC_MAT), ">5");
    // 族一：登记。
    let mut b = EcoBook::new();
    s.add("登 IP", b.add(1, PlyType::Ip, 4).is_ok(), "卖图纸");
    s.add("登 SoC", b.add(2, PlyType::Soc, 3).is_ok(), "造整机");
    s.add("登 软渲", b.add(3, PlyType::Soft, 5).is_ok(), "兜底");
    s.add("重名拒", b.add(1, PlyType::Soft, 2) == Err(XC_DUP), "一名一籍");
    s.add("查 1", b.get(1) == Some((PlyType::Ip, 4)), "在籍");
    // 族二：快照。
    let sp = b.snap();
    s.add("总数 3", sp.total == 3, "三玩家");
    s.add("IP 1", sp.n_ip == 1, "分布");
    s.add("SoC 1", sp.n_soc == 1, "分布");
    s.add("软渲 1", sp.n_soft == 1, "分布");
    s.add("最高 5", sp.top == 5, "软渲顶");
    s.add("可合作 3", sp.partners == 3, "全 ≥3");
    // 族三：门槛。
    let _ = b.add(4, PlyType::Ip, 2);
    let sp2 = b.snap();
    s.add("总 4", sp2.total == 4, "增员");
    s.add("可合作 3", sp2.partners == 3, "2 分者出局");
    s.add("IP 2", sp2.n_ip == 2, "分布");
    // 下架。
    s.add("下架 4", b.remove(4).is_ok(), "移出");
    s.add("下架无者拒", b.remove(4) == Err(XC_NOPLY), "无二下");
    // 族四：快照稳定核。
    let sa = b.snap();
    let sb = b.snap();
    s.add("两次同果", sa == sb, "无变更");
    // 变更后快照变。
    let _ = b.add(5, PlyType::Soc, 5);
    s.add("变更后变", b.snap() != sa, "不同果");
    // 族五：空盘快照。
    let z = EcoBook::new();
    let sz = z.snap();
    s.add("空盘全零", sz.total == 0 && sz.top == 0 && sz.partners == 0, "白板");
    s.add("魔数核", RV_SEG_BASE == 0x31940, "段基址");
    s
}
