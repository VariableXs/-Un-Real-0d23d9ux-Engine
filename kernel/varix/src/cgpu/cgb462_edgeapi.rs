//! CGPU-F0462 · 边界场景 API 冻结（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API：边界场景控制接口冻结（场景查询/强制触发演练/豁免设置
//!   ——签名冻结并入 C 域总册）；
//! - 与 O 域降级链 API 的衔接声明。判据：冻结、衔接、判据。
//!
//! 设计纪律：
//! - 三族签名冻结：查询（scene_state）、演练触发（drill_kick）、
//!   豁免（exempt_set）——签名表常量入册，v1.0 只增不改不删。
//! - 兼容闸：同名不同参（参数个数/类型序错）一律拒——签名指纹
//!   比对（名+参量数+类型码序）。
//! - 衔接声明：O 域降级链三对接点（场景状态上报/降级请求入口/
//!   恢复通知出口）显性入册，每点声明数据流向，缺失即断链可查。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1010 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1010，全仓独占）。
pub const SEG_BASE: u16 = 0x1010;

pub const E_API_NAME: u16 = SEG_BASE + 1; // API 名未冻结
pub const E_API_SIG: u16 = SEG_BASE + 2; // 签名不匹配（同名不同参）
pub const E_API_LOCKED: u16 = SEG_BASE + 3; // 冻结后不可变更
pub const E_API_LINK: u16 = SEG_BASE + 4; // 衔接点缺失

/// 类型码。
pub const T_U8: u8 = 1;
pub const T_U32: u8 = 2;
pub const T_U64: u8 = 3;
pub const T_BOOL: u8 = 4;

/// 冻结签名（名 + 参数类型码序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiSig {
    pub name: &'static str,
    pub args: Vec<u8>,
    /// 返回类型码。
    pub ret: u8,
}

impl ApiSig {
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in self.name.as_bytes() {
            h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
        }
        for &a in self.args.iter() {
            h = (h ^ a as u32).wrapping_mul(0x0100_0193);
        }
        (h ^ self.ret as u32).wrapping_mul(0x0100_0193)
    }
}

/// v1.0 冻结签名册（C 域总册并集源）。
pub fn frozen_v1() -> Vec<ApiSig> {
    vec![
        // 查询族：场景状态查询（场景码 → 状态+级别）。
        ApiSig { name: "edge.scene_state", args: vec![T_U8], ret: T_U8 },
        // 演练族：强制触发演练（场景码+轮次 → 结果码）。
        ApiSig { name: "edge.drill_kick", args: vec![T_U8, T_U64], ret: T_U8 },
        // 豁免族：豁免设置（场景码+豁免开关 → 是否受理）。
        ApiSig { name: "edge.exempt_set", args: vec![T_U8, T_BOOL], ret: T_BOOL },
    ]
}

/// API 注册表（冻结闸）。
#[derive(Debug)]
pub struct EdgeApiRegistry {
    sigs: Vec<ApiSig>,
    version: u32,
    locked: bool,
}

impl EdgeApiRegistry {
    pub fn v1() -> Self {
        Self { sigs: frozen_v1(), version: 1, locked: true }
    }

    /// 查询：名+参量序指纹匹配（同名不同参拒）。
    pub fn lookup(&self, name: &str, args: &[u8]) -> Result<&ApiSig, u16> {
        let hits: Vec<&ApiSig> = self.sigs.iter().filter(|s| s.name == name).collect();
        if hits.is_empty() {
            return Err(E_API_NAME);
        }
        hits.iter()
            .find(|s| s.args.as_slice() == args)
            .copied()
            .ok_or(E_API_SIG)
    }

    /// 增量注册（只增：同名任何变更拒）。
    pub fn add(&mut self, sig: ApiSig) -> Result<(), u16> {
        if self.locked && self.version >= 1 {
            if self.sigs.iter().any(|s| s.name == sig.name) {
                return Err(E_API_LOCKED);
            }
        }
        self.sigs.push(sig);
        Ok(())
    }

    /// 版本推进（只增）：签名数不减。
    pub fn bump_version(&mut self) -> u32 {
        self.version += 1;
        self.version
    }

    pub fn len(&self) -> usize {
        self.sigs.len()
    }
}

/// O 域降级链衔接点（三对接点显性入册）。
pub const LINK_STATE_REPORT: u8 = 1; // 场景状态上报 → O 域（本域→O）
pub const LINK_DEGRADE_REQ: u8 = 2; // 降级请求入口 ← O 域（O→本域）
pub const LINK_RECOVER_NOTIFY: u8 = 3; // 恢复通知出口 → O 域（本域→O）
pub const LINKS: [u8; 3] = [LINK_STATE_REPORT, LINK_DEGRADE_REQ, LINK_RECOVER_NOTIFY];

/// 衔接点声明（名+数据流向），三点齐备才算链接通。
pub fn link_declared(links: &[u8]) -> Result<(), u16> {
    for &need in LINKS.iter() {
        if !links.contains(&need) {
            return Err(E_API_LINK);
        }
    }
    Ok(())
}

/// 兼容闸示例判定：同名不同参在 v1.0 下的受理结果。
pub fn compat_check(name: &str, args: &[u8]) -> Result<u32, u16> {
    let reg = EdgeApiRegistry::v1();
    reg.lookup(name, args).map(|s| s.fingerprint())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、冻结：三族签名齐备。
    {
        let reg = EdgeApiRegistry::v1();
        v.push(("F0462-冻结-三族签名入册", reg.len() == 3));
        v.push(("F0462-冻结-查询族签名", compat_check("edge.scene_state", &[T_U8]).is_ok()));
        v.push(("F0462-冻结-演练族签名", compat_check("edge.drill_kick", &[T_U8, T_U64]).is_ok()));
        v.push(("F0462-冻结-豁免族签名", compat_check("edge.exempt_set", &[T_U8, T_BOOL]).is_ok()));
    }
    // 二、兼容闸：同名不同参拒。
    {
        v.push(("F0462-兼容-参量数不符拒", compat_check("edge.scene_state", &[T_U8, T_U8]) == Err(E_API_SIG)));
        v.push(("F0462-兼容-类型序不符拒", compat_check("edge.drill_kick", &[T_U64, T_U8]) == Err(E_API_SIG)));
        v.push(("F0462-兼容-未知名拒", compat_check("edge.unknown", &[T_U8]) == Err(E_API_NAME)));
        // 同名同参指纹稳定。
        let f1 = compat_check("edge.scene_state", &[T_U8]);
        let f2 = compat_check("edge.scene_state", &[T_U8]);
        v.push(("F0462-兼容-指纹稳定", f1.is_ok() && f1 == f2));
    }
    // 三、只增：同名变更拒，新名可增。
    {
        let mut reg = EdgeApiRegistry::v1();
        let dup = ApiSig { name: "edge.scene_state", args: vec![T_U8, T_U32], ret: T_U8 };
        v.push(("F0462-只增-同名变更拒", reg.add(dup) == Err(E_API_LOCKED)));
        let fresh = ApiSig { name: "edge.scene_batch", args: vec![T_U8], ret: T_U8 };
        v.push(("F0462-只增-新名可增", reg.add(fresh).is_ok() && reg.len() == 4));
        let ver = reg.bump_version();
        v.push(("F0462-只增-版本推进", ver == 2));
    }
    // 四、衔接声明：三点齐备。
    {
        v.push(("F0462-衔接-三点齐备通过", link_declared(&LINKS).is_ok()));
        v.push(("F0462-衔接-缺点判断链", link_declared(&[LINK_STATE_REPORT, LINK_DEGRADE_REQ]) == Err(E_API_LINK)));
        v.push(("F0462-衔接-流向覆盖双向", LINKS.contains(&LINK_DEGRADE_REQ) && LINKS.contains(&LINK_STATE_REPORT)));
    }
    // 五、判据自检。
    {
        let reg = EdgeApiRegistry::v1();
        let fp_a = reg.lookup("edge.exempt_set", &[T_U8, T_BOOL]).map(|s| s.fingerprint());
        let fp_b = reg.lookup("edge.drill_kick", &[T_U8, T_U64]).map(|s| s.fingerprint());
        v.push(("F0462-判据-不同签名指纹互异", fp_a != fp_b && fp_a.is_ok() && fp_b.is_ok()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0462_edgeapi_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
