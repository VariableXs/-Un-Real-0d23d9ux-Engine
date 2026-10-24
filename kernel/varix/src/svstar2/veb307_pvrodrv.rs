//! veb307_pvrodrv —— VE-F0307 PowerVR 开源驱动评估。
//! 锚点（CoRun Unxreal 主册）：VE-F0307 · PowerVR 开源驱动评估 · 目标 320 行
//! 判据原文：VE-F0307-J1 开源驱动评估账：候选二条（Mesa PVR
//!           开源 / IMG DDK 私有）；设备支持矩阵（候选 × 代
//!           际：支持 / 部分 / 不支持三态）；计分（支持 10 +
//!           部分 4）；候选总分（代际权和）；成熟补项（DDK
//!           恒加成熟分 20）；推荐门（开源总分 ≥ DDK 总分 →
//!           荐开源，否则荐私有）；矩阵完整性核（每候选每代
//!           际必须有三态之一，Blank 拒荐）。
//! 组件：候选账 + 支持矩阵 + 计分器 + 推荐门。
//! 职责定位：开源驱动评估是「PVR 的选择题」——两条路两种分
//!           数：开源是进步中的学生、DDK 是成熟的老将；部分
//!           支持不是支持：给 4 分是鼓励分不是及格分；Blank
//!           拒荐是交白卷的处理：没答的题不能当对的算。
//! 性能逐项分解：计分 O(代际数)。
//! 诊断码段：0x315C0（0x315C1~0x315C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const PO_VERSION: &str = "veb307_pvrodrv-v1";

/// 诊断段基址（0x315C0 段 · 本模块独占）。
pub const PO_SEG_BASE: u32 = 0x315C0;
pub const XC_MAGIC: u32 = 0x315C1; // 账魔数破
pub const XC_BLANK: u32 = 0x315C2; // 矩阵有白格

/// 账魔数。
const MAGIC: u32 = 0x315C_1B17;

/// 候选二条。
pub const CAND_MESA: usize = 0;
pub const CAND_DDK: usize = 1;
pub const NCAND: usize = 2;

/// 代际三窗。
pub const NGEN: usize = 3;

/// 三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sup {
    Full,
    Partial,
    No,
    Blank,
}

/// 支持分。
pub fn sup_score(st: Sup) -> u32 {
    match st {
        Sup::Full => 10,
        Sup::Partial => 4,
        _ => 0,
    }
}

/// DDK 成熟恒加分。
pub const DDK_MATURE: u32 = 20;

/// 评估簿。
pub struct DrvEval {
    /// 矩阵 [候选][代际]。
    mat: [[Sup; NGEN]; NCAND],
    magic: u32,
}

impl DrvEval {
    /// 建。
    pub fn new() -> DrvEval {
        DrvEval { mat: [[Sup::Blank; NGEN]; NCAND], magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记格。
    pub fn set(&mut self, cand: usize, gen: usize, st: Sup) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if cand >= NCAND || gen >= NGEN {
            return Err(XC_BLANK);
        }
        self.mat[cand][gen] = st;
        Ok(())
    }

    /// 查格。
    pub fn get(&self, cand: usize, gen: usize) -> Result<Sup, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if cand >= NCAND || gen >= NGEN {
            return Err(XC_BLANK);
        }
        Ok(self.mat[cand][gen])
    }

    /// 候选总分。
    pub fn score(&self, cand: usize) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if cand >= NCAND {
            return Err(XC_BLANK);
        }
        let mut t = 0;
        for g in 0..NGEN {
            t += sup_score(self.mat[cand][g]);
        }
        if cand == CAND_DDK {
            t += DDK_MATURE;
        }
        Ok(t)
    }

    /// 矩阵完整核：无 Blank。
    pub fn complete(&self) -> bool {
        for c in 0..NCAND {
            for g in 0..NGEN {
                if self.mat[c][g] == Sup::Blank {
                    return false;
                }
            }
        }
        true
    }

    /// 推荐门：完整 + 总分比。
    pub fn recommend(&self) -> Result<usize, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if !self.complete() {
            return Err(XC_BLANK);
        }
        let mesa = self.score(CAND_MESA)?;
        let ddk = self.score(CAND_DDK)?;
        if mesa >= ddk {
            Ok(CAND_MESA)
        } else {
            Ok(CAND_DDK)
        }
    }
}

pub fn run_veb307_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0307");
    // 族零：计分口径。
    s.add("支持 10", sup_score(Sup::Full) == 10, "满分");
    s.add("部分 4", sup_score(Sup::Partial) == 4, "鼓励分");
    s.add("不支持 0", sup_score(Sup::No) == 0, "零分");
    s.add("白格 0", sup_score(Sup::Blank) == 0, "白卷");
    // 族一：记格与查格。
    let mut e = DrvEval::new();
    s.add("越界拒", e.set(2, 0, Sup::Full) == Err(XC_BLANK), "无三候选");
    s.add("代越界拒", e.set(0, 3, Sup::Full) == Err(XC_BLANK), "无四代");
    s.add("Mesa 全支持", e.set(CAND_MESA, 0, Sup::Full).is_ok(), "代 0");
    let _ = e.set(CAND_MESA, 1, Sup::Full);
    let _ = e.set(CAND_MESA, 2, Sup::Partial);
    let _ = e.set(CAND_DDK, 0, Sup::Partial);
    let _ = e.set(CAND_DDK, 1, Sup::Full);
    let _ = e.set(CAND_DDK, 2, Sup::Full);
    s.add("矩阵完整", e.complete(), "无白格");
    // 族二：计分。
    // Mesa：10+10+4 = 24。
    s.add("Mesa 24", e.score(CAND_MESA) == Ok(24), "无加分");
    // DDK：4+10+10+20 = 44。
    s.add("DDK 44", e.score(CAND_DDK) == Ok(44), "加成熟 20");
    // 族三：推荐门。
    s.add("荐 DDK", e.recommend() == Ok(CAND_DDK), "44>24");
    // 反转：Mesa 提升到全满 → 30 vs 44 仍 DDK。
    // 让 Mesa 在所有代都支持 = 30，仍小于 44。
    // DDK 降到全不支持：0+20=20 < Mesa 30 → 荐 Mesa。
    let mut e2 = DrvEval::new();
    let _ = e2.set(CAND_MESA, 0, Sup::Full);
    let _ = e2.set(CAND_MESA, 1, Sup::Full);
    let _ = e2.set(CAND_MESA, 2, Sup::Full);
    let _ = e2.set(CAND_DDK, 0, Sup::No);
    let _ = e2.set(CAND_DDK, 1, Sup::No);
    let _ = e2.set(CAND_DDK, 2, Sup::No);
    s.add("Mesa 30", e2.score(CAND_MESA) == Ok(30), "全满");
    s.add("DDK 20", e2.score(CAND_DDK) == Ok(20), "成熟托底");
    s.add("荐 Mesa", e2.recommend() == Ok(CAND_MESA), "30>20");
    // 平局荐开源（≥ 判定）。
    let mut e3 = DrvEval::new();
    // Mesa: 4*3=12；DDK: 0*3+20=20。改 DDK 全 No、Mesa 全 Partial + ...
    // 要 12 ≥ 20 不行；构造等分：Mesa 10+10+0=20，DDK 0+0+20=20。
    let _ = e3.set(CAND_MESA, 0, Sup::Full);
    let _ = e3.set(CAND_MESA, 1, Sup::Full);
    let _ = e3.set(CAND_MESA, 2, Sup::No);
    let _ = e3.set(CAND_DDK, 0, Sup::No);
    let _ = e3.set(CAND_DDK, 1, Sup::No);
    let _ = e3.set(CAND_DDK, 2, Sup::No);
    s.add("平分 20/20", e3.score(CAND_MESA) == Ok(20) && e3.score(CAND_DDK) == Ok(20), "等分");
    s.add("平局荐开源", e3.recommend() == Ok(CAND_MESA), "≥ 含等");
    // 族四：白格拒荐。
    let mut w = DrvEval::new();
    let _ = w.set(CAND_MESA, 0, Sup::Full);
    // 其余白格。
    s.add("白格在", w.complete() == false, "不完整");
    s.add("白格拒荐", w.recommend() == Err(XC_BLANK), "白卷不荐");
    // 计分照常（白格 0 分）。
    s.add("白格计分 10", w.score(CAND_MESA) == Ok(10), "照算");
    s.add("DDK 白格 20", w.score(CAND_DDK) == Ok(20), "成熟仍在");
    s.add("魔数核", PO_SEG_BASE == 0x315C0, "段基址");
    s
}
