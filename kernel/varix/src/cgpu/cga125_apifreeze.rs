//! cga125_apifreeze — CGPU-F0125 SIMD 库 API 终审冻结。
//!
//! 判据原文（施工书锚点 CGPU-F0125）：A 域全部 SIMD 算子的 API 终审：
//! 算子签名全集（栅格化/混合/文字/图像/编解码/工具族约百算子）逐签名
//! 冻结（F0382 范式）、稳定性分级、语义测试锁定、ABI 承诺（F0118）联动。
//! 冻结后算子签名变更=主版本事件。判据：全算子冻结、语义测试、分级、
//! ABI 联动、下游验证。
//!
//! **设计定盘**：六算子族闭集（栅格化/混合/文字/图像/编解码/工具），
//! 每算子登记（族别 + 签名指纹 + 稳定性级）。稳定性三级闭集：Stable
//! （十年承诺）/ Provisional（一版承诺）/ Experimental（无承诺，冻结
//! 时 Experimental 算子必须全量降级为 Provisional——冻结面不允许无承诺
//! 项）。语义测试锁定 = 每算子配测试指纹（无测试指纹的算子不得冻结）。
//! ABI 联动 = 冻结集指纹与 F0118 ABI 账的兼容核验入口（基线指纹集 =
//! 本册冻结集的投影）。下游验证 = 下游消费者登记引用，冻结后引用指纹
//! 比对。
//! 
//! 诊断码段 0x01B0~0x01BF（0x01 段第 12 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA125_VERSION: &str = "cga125-apifreeze-v1";

/// 诊断子区基值（0x01 段第 12 子区）。
pub const SEG_BASE: u16 = 0x01B0;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 算子族闭集（判据"栅格化/混合/文字/图像/编解码/工具族"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpFamily {
    /// 栅格化。
    Raster,
    /// 混合。
    Blend,
    /// 文字。
    Text,
    /// 图像。
    Image,
    /// 编解码。
    Codec,
    /// 工具。
    Util,
}

impl OpFamily {
    /// 全集（稳定序）。
    pub const ALL: [OpFamily; 6] = [
        OpFamily::Raster,
        OpFamily::Blend,
        OpFamily::Text,
        OpFamily::Image,
        OpFamily::Codec,
        OpFamily::Util,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            OpFamily::Raster => 0,
            OpFamily::Blend => 1,
            OpFamily::Text => 2,
            OpFamily::Image => 3,
            OpFamily::Codec => 4,
            OpFamily::Util => 5,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 稳定性分级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stability {
    /// 十年承诺。
    Stable,
    /// 一版承诺。
    Provisional,
    /// 无承诺（冻结前必须消亡的级别）。
    Experimental,
}

/// 算子冻结项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenOp {
    /// 算子 id。
    pub op: u32,
    /// 族别。
    pub fam: OpFamily,
    /// 签名指纹。
    pub sig_fp: u64,
    /// 语义测试指纹（零=无测试=不得冻结）。
    pub test_fp: u64,
    /// 稳定性级。
    pub stab: Stability,
}

/// API 冻结册。
pub struct ApiFreezeBook {
    /// 冻结项。
    ops: Vec<FrozenOp>,
    /// 下游消费者（引用登记）。
    consumers: Vec<(&'static str, u64)>,
    /// 冻结签章。
    sealed: Option<u64>,
}

impl ApiFreezeBook {
    /// 空册。
    pub fn new() -> Self {
        ApiFreezeBook {
            ops: Vec::new(),
            consumers: Vec::new(),
            sealed: None,
        }
    }

    /// 登记算子（重复 id 即拒；签名或测试指纹为零即拒——
    /// 无语义测试的算子不许进冻结册）。
    pub fn enroll(
        &mut self,
        op: u32,
        fam: OpFamily,
        sig: &[u8],
        test_fp: u64,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 7);
        }
        if op == 0 || self.ops.iter().any(|o| o.op == op) {
            return Err(SEG_BASE + 8);
        }
        if sig.is_empty() {
            return Err(SEG_BASE + 9);
        }
        if test_fp == 0 {
            return Err(SEG_BASE + 10);
        }
        self.ops.push(FrozenOp {
            op,
            fam,
            sig_fp: fnv1a64(sig),
            test_fp,
            stab: Stability::Experimental,
        });
        Ok(())
    }

    /// 分级（判据"稳定性分级"：降级单向——只能升不能降；
    /// Experimental→Provisional→Stable）。
    pub fn promote(&mut self, op: u32, to: Stability) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 7);
        }
        let e = match self.ops.iter_mut().find(|o| o.op == op) {
            Some(e) => e,
            None => return Err(SEG_BASE + 11),
        };
        let cur = e.stab as u32;
        let tgt = to as u32;
        if tgt <= cur {
            return Err(SEG_BASE + 12);
        }
        e.stab = to;
        Ok(())
    }

    /// 冻结（判据"全算子冻结"：无 Experimental 残留；六族每族
    /// 至少一算子——冻结是全族事件不是散件事件）。
    pub fn freeze_all(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 13);
        }
        if self.ops.iter().any(|o| o.stab == Stability::Experimental) {
            return Err(SEG_BASE + 14);
        }
        for fam in OpFamily::ALL {
            if !self.ops.iter().any(|o| o.fam == fam) {
                return Err(fam.code());
            }
        }
        let mut fp = FNV_OFFSET;
        for o in &self.ops {
            fp = fp.wrapping_mul(FNV_PRIME) ^ o.sig_fp;
            fp = fp.wrapping_mul(FNV_PRIME) ^ o.test_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }

    /// 冻结指纹。
    pub fn freeze_fp(&self) -> Option<u64> {
        self.sealed
    }

    /// 下游引用登记（判据"下游验证"：冻结后引用，指纹须与冻结指纹
    /// 一致——不一致即下游引用了漂移版本）。
    pub fn cite(&mut self, who: &'static str, cited_fp: u64) -> Result<(), u16> {
        let fp = self.sealed.ok_or(SEG_BASE + 15)?;
        if who.is_empty() || self.consumers.iter().any(|(w, _)| *w == who) {
            return Err(SEG_BASE + 16);
        }
        if cited_fp != fp {
            return Err(SEG_BASE + 17);
        }
        self.consumers.push((who, cited_fp));
        Ok(())
    }

    /// 下游验证数。
    pub fn consumer_count(&self) -> usize {
        self.consumers.len()
    }

    /// 算子数。
    pub fn op_count(&self) -> usize {
        self.ops.len()
    }

    /// 族别分布投影（判据"全算子冻结"的账面）：六族各多少算子。
    pub fn family_census(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for o in &self.ops {
            out[o.fam.ordinal() as usize] += 1;
        }
        out
    }

    /// 稳定性分布投影：三级各自数量。
    pub fn stability_census(&self) -> (usize, usize, usize) {
        let s = self
            .ops
            .iter()
            .filter(|o| o.stab == Stability::Stable)
            .count();
        let p = self
            .ops
            .iter()
            .filter(|o| o.stab == Stability::Provisional)
            .count();
        let e = self
            .ops
            .iter()
            .filter(|o| o.stab == Stability::Experimental)
            .count();
        (s, p, e)
    }
}

/// 签名漂移立案（判据"冻结后算子签名变更=主版本事件"的检测面）：
/// 冻结后同 op 的指纹与冻结册不一致即漂移立案（返回 Some(旧指纹)）；
/// op 不在册返回 Err；一致返回 None。
pub fn drift_detect(
    ops: &[FrozenOp],
    op: u32,
    current_sig: &[u8],
) -> Result<Option<u64>, u16> {
    let e = ops.iter().find(|o| o.op == op).ok_or(SEG_BASE + 18)?;
    let cur = fnv1a64(current_sig);
    if cur == e.sig_fp {
        Ok(None)
    } else {
        Ok(Some(e.sig_fp))
    }
}

impl ApiFreezeBook {
    /// ABI 联动投影（判据"ABI 承诺（F0118）联动"）：在役冻结签名集
    /// 导出为 (op, 指纹) 基线——喂给 F0118 的 abi_compat_check 做跨账
    /// 对拍（本册是 A 域冻结真源，F0118 是版本化真源，两者以指纹集
    /// 相等互证）。
    pub fn abi_baseline(&self) -> Vec<(u32, u64)> {
        self.ops
            .iter()
            .filter(|o| o.stab != Stability::Experimental)
            .map(|o| (o.op, o.sig_fp))
            .collect()
    }

    /// 语义测试锁定核验（判据"语义测试锁定"）：冻结集内全部算子
    /// 的测试指纹非零。
    pub fn test_lock_ok(&self) -> Result<(), u16> {
        for o in &self.ops {
            if o.test_fp == 0 {
                return Err(SEG_BASE + 10);
            }
        }
        Ok(())
    }

    /// 冻结就绪报告（判据"全算子冻结"的预检面）：不落签，
    /// 只报告距离可冻结还差什么（Experimental 残留数 + 缺族清单）。
    pub fn readiness(&self) -> (usize, Vec<OpFamily>) {
        let exp = self
            .ops
            .iter()
            .filter(|o| o.stab == Stability::Experimental)
            .count();
        let missing: Vec<OpFamily> = OpFamily::ALL
            .iter()
            .copied()
            .filter(|f| !self.ops.iter().any(|o| o.fam == *f))
            .collect();
        (exp, missing)
    }
}


// ===== 深化块（cga125_apifreeze · API 冻结深化审计）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_apifreeze {
    //! API 冻结深化审计 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 冻结族签名复审：族表逐项 FNV-1a 64 位复算，任何一项为 0 视为未冻结。
    pub fn signature_fnv(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 族冻结完整性：任一签名命中零哈希即拒绝（空承诺 = 未冻结）。
    pub fn freeze_family_complete(sigs: &[u64]) -> Result<(), u16> {
        if sigs.is_empty() {
            return Err(1);
        }
        for (i, s) in sigs.iter().enumerate() {
            if *s == 0 {
                return Err(2 + i as u16);
            }
        }
        Ok(())
    }

    /// 版本前进单调性：新版本号必须严格大于旧版本号（回退 = 破坏冻结承诺）。
    pub fn version_forward_only(old: u32, new: u32) -> Result<(), u16> {
        if new <= old {
            return Err(64);
        }
        Ok(())
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0125-深化");
        let ok = freeze_family_complete(&[signature_fnv(b"api.v1"), signature_fnv(b"api.v2")]).is_ok();
        cs.add("签名FNV非零", ok, "");
        cs.add("版本只进不退", version_forward_only(1, 2).is_ok() && version_forward_only(2, 1).is_err(), "");
        cs
    }
}
