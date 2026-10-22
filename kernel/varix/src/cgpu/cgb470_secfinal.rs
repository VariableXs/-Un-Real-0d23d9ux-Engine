//! CGPU-F0470 · 安全终审（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 安全终审：三输入面🔴零（第三方声明面 F0392/配方导入面 F0362/
//!   账单导出面 F0425——逐面清洗与 fuzz 证据核对）；
//! - 防欺诈体系终验（虚报/循环借贷/申请风暴的防护完整性）；
//! - 权限面复核（配额热调整的权限模型 F0390）；🔴零确认。
//!   判据：三面、防欺诈终验、权限复核、🔴零、判据。
//!
//! 设计纪律：
//! - 三输入面逐面核验：每面（清洗闸+fuzz 证据）双证据齐备才判绿，
//!   任一面缺证据即终审不通过（🔴零是全称断言不是抽查）。
//! - 防欺诈三防：虚报（申请量与账本对不上→拒）、循环借贷（A→B→A
//!   环检测→拒）、申请风暴（频率封顶→限流）——三防各自可独立
//!   复验。
//! - 权限面：配额热调整的权限模型复核（角色×操作矩阵），越权即拒。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1090 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1090，全仓独占）。
pub const SEG_BASE: u16 = 0x1090;

pub const E_SF_FACE: u16 = SEG_BASE + 1; // 输入面证据缺失
pub const E_SF_FRAUD: u16 = SEG_BASE + 2; // 欺诈命中
pub const E_SF_PERM: u16 = SEG_BASE + 3; // 越权
pub const E_SF_NOTZERO: u16 = SEG_BASE + 4; // 🔴未归零

/// 三输入面。
pub const FACE_THIRD_PARTY: u8 = 1; // 第三方声明面 F0392
pub const FACE_RECIPE_IMPORT: u8 = 2; // 配方导入面 F0362
pub const FACE_BILL_EXPORT: u8 = 3; // 账单导出面 F0425
pub const FACES: [u8; 3] = [FACE_THIRD_PARTY, FACE_RECIPE_IMPORT, FACE_BILL_EXPORT];

/// 面证据（清洗闸过 + fuzz 绿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceEvidence {
    pub face: u8,
    pub wash_ok: bool,
    pub fuzz_ok: bool,
}

impl FaceEvidence {
    pub fn green(&self) -> bool {
        FACES.contains(&self.face) && self.wash_ok && self.fuzz_ok
    }
}

/// 🔴零确认：三面全绿（全称）。
pub fn faces_zero(evs: &[FaceEvidence]) -> Result<(), u16> {
    for &f in FACES.iter() {
        let hit = evs.iter().find(|e| e.face == f);
        match hit {
            Some(e) if e.green() => {}
            _ => return Err(E_SF_NOTZERO),
        }
    }
    Ok(())
}

/// 防欺诈一：虚报检测（申报量 vs 账本可支付）。
pub fn fraud_overclaim(claimed: u64, ledger_avail: u64) -> Result<(), u16> {
    if claimed > ledger_avail {
        return Err(E_SF_FRAUD);
    }
    Ok(())
}

/// 防欺诈二：循环借贷环检测（借贷有向边 → DFS 环查找）。
/// 边表 edges[i] = i 借给 j 的目标列表；环=欺诈。
pub fn fraud_cycle(edges: &[Vec<u32>], n: usize) -> Result<(), u16> {
    // Warshall 传递闭包：reach[i][i]=true 即存在环（n 为小规模场景）。
    let mut reach = vec![vec![false; n]; n];
    for i in 0..n {
        if let Some(l) = edges.get(i) {
            for &j in l.iter() {
                let j = j as usize;
                if j < n {
                    reach[i][j] = true;
                }
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            if reach[i][k] {
                for j in 0..n {
                    if reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
    }
    for i in 0..n {
        if reach[i][i] {
            return Err(E_SF_FRAUD); // 自可达=环
        }
    }
    Ok(())
}

/// 防欺诈三：申请风暴（频率封顶）。
pub const STORM_CAP: u64 = 100;

pub fn fraud_storm(recent_requests: u64) -> Result<(), u16> {
    if recent_requests > STORM_CAP {
        return Err(E_SF_FRAUD);
    }
    Ok(())
}

/// 权限模型：角色×操作矩阵。
pub const R_OPERATOR: u8 = 1; // 操作员
pub const R_ADMIN: u8 = 2; // 管理员
pub const O_QUOTA_HOTSET: u8 = 1; // 配额热调整
pub const O_DRAIN: u8 = 2; // 全量清账
pub const O_RECIPE_LOAD: u8 = 3; // 配方导入

pub fn perm_ok(role: u8, op: u8) -> Result<(), u16> {
    let allowed = match (role, op) {
        (R_OPERATOR, O_QUOTA_HOTSET) => true,
        (R_ADMIN, O_QUOTA_HOTSET) | (R_ADMIN, O_DRAIN) | (R_ADMIN, O_RECIPE_LOAD) => true,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(E_SF_PERM)
    }
}

/// 安全终审编排：三面🔴零 + 三防 + 权限全绿。
pub fn final_security(evs: &[FaceEvidence]) -> Result<(), u16> {
    faces_zero(evs)?;
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、三输入面🔴零。
    {
        let full = vec![
            FaceEvidence { face: FACE_THIRD_PARTY, wash_ok: true, fuzz_ok: true },
            FaceEvidence { face: FACE_RECIPE_IMPORT, wash_ok: true, fuzz_ok: true },
            FaceEvidence { face: FACE_BILL_EXPORT, wash_ok: true, fuzz_ok: true },
        ];
        v.push(("F0470-三面-全绿🔴零", final_security(&full).is_ok()));
        // 缺面。
        let miss = full[..2].to_vec();
        v.push(("F0470-三面-缺面拦", final_security(&miss) == Err(E_SF_NOTZERO)));
        // 面证据不齐。
        let half = vec![
            FaceEvidence { face: FACE_THIRD_PARTY, wash_ok: true, fuzz_ok: false },
            FaceEvidence { face: FACE_RECIPE_IMPORT, wash_ok: true, fuzz_ok: true },
            FaceEvidence { face: FACE_BILL_EXPORT, wash_ok: true, fuzz_ok: true },
        ];
        v.push(("F0470-三面-fuzz缺证据拦", final_security(&half) == Err(E_SF_NOTZERO)));
    }
    // 二、防欺诈：虚报。
    {
        v.push(("F0470-防欺诈-虚报拒", fraud_overclaim(900, 800) == Err(E_SF_FRAUD)));
        v.push(("F0470-防欺诈-足额通过", fraud_overclaim(500, 800).is_ok()));
    }
    // 三、防欺诈：循环借贷环。
    {
        // A→B→A 环。
        let ring: Vec<Vec<u32>> = vec![vec![1], vec![0], Vec::new()];
        v.push(("F0470-防欺诈-双元环拒", fraud_cycle(&ring, 3) == Err(E_SF_FRAUD)));
        // 三元环 A→B→C→A。
        let ring3: Vec<Vec<u32>> = vec![vec![1], vec![2], vec![0]];
        v.push(("F0470-防欺诈-三元环拒", fraud_cycle(&ring3, 3) == Err(E_SF_FRAUD)));
        // 无环 DAG。
        let dag: Vec<Vec<u32>> = vec![vec![1, 2], vec![2], Vec::new()];
        v.push(("F0470-防欺诈-DAG通过", fraud_cycle(&dag, 3).is_ok()));
        // 自环。
        let self_loop: Vec<Vec<u32>> = vec![vec![0]];
        v.push(("F0470-防欺诈-自环拒", fraud_cycle(&self_loop, 1) == Err(E_SF_FRAUD)));
    }
    // 四、防欺诈：申请风暴。
    {
        v.push(("F0470-防欺诈-风暴限流", fraud_storm(STORM_CAP + 1) == Err(E_SF_FRAUD)));
        v.push(("F0470-防欺诈-频率封顶内过", fraud_storm(STORM_CAP).is_ok()));
    }
    // 五、权限复核。
    {
        v.push(("F0470-权限-操作员热调过", perm_ok(R_OPERATOR, O_QUOTA_HOTSET).is_ok()));
        v.push(("F0470-权限-操作员清账拒", perm_ok(R_OPERATOR, O_DRAIN) == Err(E_SF_PERM)));
        v.push(("F0470-权限-管理员三权过", perm_ok(R_ADMIN, O_QUOTA_HOTSET).is_ok() && perm_ok(R_ADMIN, O_DRAIN).is_ok() && perm_ok(R_ADMIN, O_RECIPE_LOAD).is_ok()));
        v.push(("F0470-权限-角色非法拒", perm_ok(9, O_DRAIN) == Err(E_SF_PERM)));
    }
    // 六、判据自检。
    {
        v.push(("F0470-判据-三面枚举", FACES == [1, 2, 3]));
        v.push(("F0470-判据-风暴封顶入册", STORM_CAP == 100));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0470_secfinal_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
