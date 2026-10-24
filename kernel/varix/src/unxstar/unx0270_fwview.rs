//! unx0270_fwview —— UNX-F0270 capsule/efirt/secureboot 固件三视图。
//! 锚点（CoRun Unxreal 主册）：UNX-F0270 · 固件三视图 · 目标 260 行
//! 判据原文：UNX-F0270-J1 固件三视图逐面对账：capsule（升级胶囊
//!           清单）、efirt（运行期服务表）、secureboot（信任链
//!           数据库）三面各自可枚举、可校验、可交叉（capsule 必
//!           须被 secureboot 数据库信任才可入队）。
//! 组件：capsule 条目（GUID 简化指纹 + 版本 + 签名位）+ efirt 服务
//!       行（服务号 + 只读位）+ secureboot 库（信任指纹集）+ 交叉
//!       核（capsule 入队前置：指纹 ∈ 信任库）。
//! 职责定位：固件三视图是「升级安全的三重门」——capsule 没过
//!   secureboot 信任核就入队 = 把固件升级的钥匙交给陌生人；efirt
//!   只读位是运行期服务的最小权限边界。三面各自闭合 + 交叉一致
//!   才是完整视图。
//! 性能逐项分解：枚举 O(面深)；信任核 O(库深)；交叉 O(胶囊×库)。
//! 诊断码段：0x202C0（0x202C1~0x202C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FW_VERSION: &str = "unx0270_fwview-v1";

/// 诊断段基址（0x202C0 段 · 本模块独占）。
pub const FW_SEG_BASE: u32 = 0x202C0;
pub const XC_UNTRUSTED: u32 = 0x202C1; // capsule 不在信任库
pub const XC_DUP_GUID: u32 = 0x202C2; // 指纹重
pub const XC_VER_ZERO: u32 = 0x202C3; // 版本 0 拒
pub const XC_SVC_DUP: u32 = 0x202C4; // 服务号重
pub const XC_SVC_OOR: u32 = 0x202C5; // 服务号越域
pub const XC_TRUST_EMPTY: u32 = 0x202C6; // 空信任库拒
pub const XC_CAPSULE_EMPTY: u32 = 0x202C7; // 空胶囊清单拒
pub const XC_QUEUE_STATE: u32 = 0x202C8; // 入队态破
pub const XC_VIEW_DRIFT: u32 = 0x202C9; // 三视图交叉漂移

/// capsule 升级胶囊。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capsule {
    /// 指纹（真实 GUID 的 FNV 简化，8 字节）。
    pub fp: u64,
    pub version: u32,
    /// 供方签名位（存在性，非密码学）。
    pub signed: bool,
}

impl Capsule {
    pub fn new(fp: u64, version: u32, signed: bool) -> Result<Capsule, u32> {
        if fp == 0 {
            return Err(XC_DUP_GUID);
        }
        if version == 0 {
            return Err(XC_VER_ZERO);
        }
        Ok(Capsule { fp, version, signed })
    }
}

/// efirt 运行期服务行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiSvc {
    /// 服务号 1~64 闭集。
    pub svc: u32,
    /// 只读位。
    pub ro: bool,
}

impl EfiSvc {
    pub fn new(svc: u32, ro: bool) -> Result<EfiSvc, u32> {
        if svc == 0 || svc > 64 {
            return Err(XC_SVC_OOR);
        }
        Ok(EfiSvc { svc, ro })
    }
}

/// secureboot 信任库。
pub struct TrustDb {
    fps: Vec<u64>,
}

impl TrustDb {
    pub fn new(fps: &[u64]) -> Result<TrustDb, u32> {
        if fps.is_empty() {
            return Err(XC_TRUST_EMPTY);
        }
        for f in fps.iter() {
            if *f == 0 {
                return Err(XC_DUP_GUID);
            }
            if fps.iter().filter(|x| *x == f).count() > 1 {
                return Err(XC_DUP_GUID);
            }
        }
        Ok(TrustDb { fps: fps.to_vec() })
    }

    /// 信任核。
    pub fn trusts(&self, fp: u64) -> bool {
        self.fps.contains(&fp)
    }

    /// 库深。
    pub fn len(&self) -> u32 {
        self.fps.len() as u32
    }
}

/// 三视图交叉核：capsule 入队前置 = 签名位 + 信任库命中。
/// 返回（入队清单, 拒收清单（带因码））。
pub fn capsule_admission(caps: &[Capsule], db: &TrustDb) -> Result<(Vec<u64>, Vec<(u64, u32)>), u32> {
    if caps.is_empty() {
        return Err(XC_CAPSULE_EMPTY);
    }
    for c in caps.iter() {
        if caps.iter().filter(|x| x.fp == c.fp).count() > 1 {
            return Err(XC_DUP_GUID);
        }
    }
    let mut admit = Vec::new();
    let mut reject = Vec::new();
    for c in caps.iter() {
        if c.signed && db.trusts(c.fp) {
            admit.push(c.fp);
        } else if !c.signed {
            reject.push((c.fp, XC_UNTRUSTED));
        } else {
            reject.push((c.fp, XC_UNTRUSTED)); // 签了但不被信任
        }
    }
    Ok((admit, reject))
}

/// efirt 表构造：服务号唯一（重号当场拒）。
pub fn svc_table(rows: &[EfiSvc]) -> Result<Vec<EfiSvc>, u32> {
    for r in rows.iter() {
        if rows.iter().filter(|x| x.svc == r.svc).count() > 1 {
            return Err(XC_SVC_DUP);
        }
    }
    Ok(rows.to_vec())
}

/// 三视图交叉一致：每个已入队 capsule 指纹必须仍在信任库
/// （入队后信任库回收指纹 = 视图漂移）。
pub fn cross_check(admitted: &[u64], db: &TrustDb) -> Result<(), u32> {
    for fp in admitted.iter() {
        if !db.trusts(*fp) {
            return Err(XC_VIEW_DRIFT);
        }
    }
    Ok(())
}

pub fn run_unx0270_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0270_fwview");
    // 族一：三面构造。
    let db = match TrustDb::new(&[0xA1A1, 0xB2B2, 0xC3C3]) {
        Ok(d) => d,
        Err(_) => {
            cs.add("信任库构造", false, "被拒");
            return cs;
        }
    };
    cs.add("库深 3", db.len() == 3, "三指纹");
    cs.add("命中信任", db.trusts(0xB2B2), "库内");
    cs.add("未录不信任", !db.trusts(0xDEAD), "库外");
    // 族二：capsule 准入。
    let caps = vec![
        Capsule::new(0xA1A1, 7, true).unwrap_or(Capsule { fp: 0xA1A1, version: 7, signed: true }),
        Capsule { fp: 0xB2B2, version: 3, signed: false },
        Capsule { fp: 0xDEAD, version: 9, signed: true },
    ];
    let (admit, reject) = match capsule_admission(&caps, &db) {
        Ok(x) => x,
        Err(_) => {
            cs.add("准入", false, "被拒");
            return cs;
        }
    };
    cs.add("签名+信任入队", admit.as_slice() == &[0xA1A1u64], "双条件齐");
    cs.add("无签拒收", reject.contains(&(0xB2B2, XC_UNTRUSTED)), "签名位缺");
    cs.add("库外拒收", reject.contains(&(0xDEAD, XC_UNTRUSTED)), "信任核拦");
    cs.add("交叉一致", cross_check(&admit, &db).is_ok(), "入队仍在库");
    // 族三：交叉漂移（信任库缩容后旧入队失效）。
    let small_db = TrustDb::new(&[0xB2B2]).unwrap_or(TrustDb { fps: vec![0xB2B2] });
    cs.add("漂移检出", cross_check(&admit, &small_db) == Err(XC_VIEW_DRIFT), "A1 已不在库");
    // 族四：efirt 表。
    let svcs = vec![
        EfiSvc::new(1, true).unwrap_or(EfiSvc { svc: 1, ro: true }),
        EfiSvc { svc: 7, ro: false },
        EfiSvc { svc: 64, ro: true },
    ];
    let tbl = svc_table(&svcs);
    cs.add("服务表过", tbl.is_ok(), "三行唯一");
    cs.add("重号拒", svc_table(&[
        EfiSvc { svc: 1, ro: true },
        EfiSvc { svc: 1, ro: false },
    ]).is_err(), "SVC_DUP");
    cs.add("越域拒", EfiSvc::new(0, true).is_err(), "SVC_OOR");
    cs.add("上越域拒", EfiSvc::new(65, true).is_err(), "SVC_OOR");
    // 族五：构造拒。
    cs.add("空库拒", TrustDb::new(&[]).is_err(), "TRUST_EMPTY");
    cs.add("库重指纹拒", TrustDb::new(&[0x1, 0x1]).is_err(), "DUP");
    cs.add("零指纹拒", Capsule::new(0, 1, true).is_err(), "DUP_GUID");
    cs.add("零版本拒", Capsule::new(0xFF, 0, true).is_err(), "VER_ZERO");
    cs.add("空胶囊拒", capsule_admission(&[], &db).is_err(), "CAPSULE_EMPTY");
    cs.add("胶囊重指纹拒", capsule_admission(&[
        Capsule { fp: 0xA1A1, version: 1, signed: true },
        Capsule { fp: 0xA1A1, version: 2, signed: true },
    ], &db).is_err(), "DUP_GUID");
    cs
}
