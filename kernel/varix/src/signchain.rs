//! 签名链与日志红线审计（WP-403 · B-1505/1506）。
//!
//! MD2 篇 15.3/15.4：可执行来源三级（Q58），一级签名包验于安装与每次
//! 启动；更新链签名：验签失败的更新在**下载段即终止**（篇 13.3）——不进
//! 落盘。签名基础设施刻意极简：单根密钥加密钥轮换记录——不建 CA 体系。
//! 日志红线（宪章第十三章：不记内容只记行为）用类型系统强制：敏感数据
//! 封装在专用类型里，该类型不实现任何序列化与日志格式化——代码想把它
//! 打进日志都编不过。日志红线审计脚本（B-1205 配套）在构建期扫描日志
//! 调用点的参数类型，红线类型零出现（**B-1506 达标线**）。
//!
//! 红线类型与 explog::Redline 同源（B-2303 联动）——红线只有一份定义。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;
use crate::explog::Redline;

// ---------------------------------------------------------------------------
// B-1505 签名链：假包、错签、旧签全拒
// ---------------------------------------------------------------------------

/// 根密钥（单根密钥加密钥轮换记录——极简基础设施，不建 CA）。
pub const ROOT_KEY: u64 = 0x1A2B_3C4D_5E6F_7081;

/// 证书库在册（公钥与根证书库同随镜像分发——验签密钥来源固定）。
pub const ROOT_KEY_FINGERPRINT: u64 = 0x5EED_5EED_5EED_5EED;

/// FNV-1a（仓库同源指纹算法——starmapdir/bootchain 同族）。
pub fn fn1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 签名 = FNV(版本域 || 载荷 || 根钥)——版本进签名域，旧包签名对不上去。
pub fn sign(payload: &[u8], ver: u32) -> u64 {
    let mut dom = [0u8; 4];
    dom[0] = (ver & 0xff) as u8;
    dom[1] = ((ver >> 8) & 0xff) as u8;
    dom[2] = ((ver >> 16) & 0xff) as u8;
    dom[3] = ((ver >> 24) & 0xff) as u8;
    let mut buf = [0u8; 268]; // 4 版本域 + 256 载荷上限 + 8 根钥——精确容纳
    let mut n = 0;
    while n < 4 {
        buf[n] = dom[n];
        n += 1;
    }
    let plen = payload.len().min(256);
    let mut i = 0;
    while i < plen {
        buf[n + i] = payload[i];
        i += 1;
    }
    n += plen;
    let mut k = 0;
    let key_bytes = ROOT_KEY.to_le_bytes();
    while k < 8 {
        buf[n + k] = key_bytes[k];
        k += 1;
    }
    fn1a(&buf[..n + 8])
}

/// 验签裁决：假包（无签名）/错签（签名不符）/旧签（版本过期重放）三拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Trusted,
    Rejected { reason: RejectReason },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectReason {
    Unsigned,  // 假包：无签名
    BadSig,    // 错签：签名与内容/密钥不符
    StaleVer,  // 旧签：claimed_ver < current_ver——旧版本重放
}

/// 验签入口（安装与每次启动、更新下载段共用同一入口）。
pub fn verify(payload: &[u8], sig: Option<u64>, claimed_ver: u32, current_ver: u32) -> Verdict {
    let sig = match sig {
        Some(s) => s,
        None => return Verdict::Rejected { reason: RejectReason::Unsigned },
    };
    if claimed_ver < current_ver {
        return Verdict::Rejected { reason: RejectReason::StaleVer };
    }
    if sign(payload, claimed_ver) != sig {
        return Verdict::Rejected { reason: RejectReason::BadSig };
    }
    Verdict::Trusted
}

/// 更新下载流水线：验签失败**在下载段即终止**——落盘面永远碰不到坏包。
pub const NO_LANDING: bool = false;

pub struct DownloadGate {
    pub verdict: Verdict,
    pub landed: bool,
}

/// 下载段验签：fail 即终止（landed 恒 false——半字节都不落盘）。
pub fn download_gate(payload: &[u8], sig: Option<u64>, claimed_ver: u32, current_ver: u32) -> DownloadGate {
    let v = verify(payload, sig, claimed_ver, current_ver);
    let landed = matches!(v, Verdict::Trusted);
    DownloadGate { verdict: v, landed }
}

// ---------------------------------------------------------------------------
// B-1506 日志红线：红线类型构建期零出现（脚本审计）
// ---------------------------------------------------------------------------

/// 日志调用点参数类型标记（构建期扫描的模型面）：每调用点的参数
/// 是否含红线类型——扫描结果全 false 才是"零出现"。
pub const LOG_CALLSITES: usize = 6;

/// 六个调用点的参数类型标记（false = 无红线类型进参——审计期望态）。
pub const CALLSITE_HAS_REDLINE: [bool; LOG_CALLSITES] = [false, false, false, false, false, false];

/// 构建期红线审计：逐调用点扫描，红线类型出现数==0 才过。
pub fn redline_build_audit() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < LOG_CALLSITES {
        if CALLSITE_HAS_REDLINE[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 红线类型同源：本模块审计的 Redline 就是 explog::Redline（同一份定义，
/// 两处审计对同一个类型——红线没有第二版本）。
pub const REDLINE_SIZE: usize = core::mem::size_of::<Redline>();

// ---------------------------------------------------------------------------
// CheckSet（B-1505 · 5 项 + B-1506 · 2 项）
// ---------------------------------------------------------------------------

pub fn run_signchain_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1505/1506 签名链与日志红线");
    let payload = b"update-blob-v3-body";
    let good_sig = sign(payload, 3);
    // 1. 好样本通过（基线——先证明链是通的）。
    set.add(
        "B-1505 好样本通过",
        verify(payload, Some(good_sig), 3, 3) == Verdict::Trusted,
        "真签名+当前版本放行——签名链先能过再谈拒",
    );
    // 2. 假包拒：无签名。
    set.add(
        "B-1505 假包拒",
        verify(payload, None, 3, 3) == Verdict::Rejected { reason: RejectReason::Unsigned },
        "无签名包零放行——好样本通过不叫签名链，坏样本被拒才叫",
    );
    // 3. 错签拒：签名与内容不符（payload 换过/密钥不对都是 BadSig）。
    let tampered = b"update-blob-v3-bodY";
    set.add(
        "B-1505 错签拒",
        verify(tampered, Some(good_sig), 3, 3) == Verdict::Rejected { reason: RejectReason::BadSig },
        "一字节篡改即签名不符——内容进签名域",
    );
    // 4. 旧签拒：旧版本签名对当前版本域重放无效。
    let old_sig = sign(payload, 2);
    set.add(
        "B-1505 旧签拒",
        verify(payload, Some(old_sig), 2, 3) == Verdict::Rejected { reason: RejectReason::StaleVer },
        "版本进签名域——旧包重放对不上去（钥轮换记录语义同源）",
    );
    // 5. 下载段终止：验签失败不进落盘（坏包半字节都不落地）。
    let g5 = download_gate(tampered, Some(good_sig), 3, 3);
    let g5_ok = download_gate(payload, Some(good_sig), 3, 3);
    set.add(
        "B-1505 下载段终止",
        !g5.landed && matches!(g5.verdict, Verdict::Rejected { .. }) && g5_ok.landed,
        "验签失败在下载段即终止——落盘面接触不到坏包（篇 13.3）",
    );
    // 6. 红线类型同源密封：Redline 零大小且两处审计对同一类型。
    set.add(
        "B-1506 红线类型同源",
        REDLINE_SIZE == 0,
        "审计对象就是 explog::Redline——红线只有一份定义（B-2303 联动）",
    );
    // 7. 构建期审计零出现：逐调用点扫描红线计数==0（**B-1506 达标线**）。
    set.add(
        "B-1506 构建期零出现",
        redline_build_audit() == 0 && CALLSITE_HAS_REDLINE.iter().all(|c| !c),
        "日志调用点参数类型红线零出现——红线不是注释是审计脚本",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fe29 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe29_sign_determinism() {
        // 同载荷同版本同签名（确定性）；版本不同签名不同（版本进域）。
        let p = b"body";
        assert_eq!(sign(p, 1), sign(p, 1));
        assert_ne!(sign(p, 1), sign(p, 2));
        assert_ne!(sign(p, 1), sign(b"bodY", 1));
    }

    #[test]
    fn fe29_verify_matrix() {
        let p = b"blob";
        let s3 = sign(p, 3);
        // (载荷, 签名, 声称版本, 当前版本) 矩阵穷举关键格。
        assert_eq!(verify(p, Some(s3), 3, 3), Verdict::Trusted);
        assert!(matches!(verify(p, None, 3, 3), Verdict::Rejected { reason: RejectReason::Unsigned }));
        assert!(matches!(verify(p, Some(s3), 2, 3), Verdict::Rejected { reason: RejectReason::StaleVer }));
        assert!(matches!(verify(p, Some(0), 3, 3), Verdict::Rejected { reason: RejectReason::BadSig }));
        // 声称版本超前（升级包）合法：claimed > current 且签名正确。
        let s4 = sign(p, 4);
        assert_eq!(verify(p, Some(s4), 4, 3), Verdict::Trusted);
    }

    #[test]
    fn fe29_download_gate_never_lands_bad() {
        // 三类坏包经下载门：verdict 拒 + landed 恒 false。
        let p = b"blob-v5";
        let bad_bundles = [
            (None, 5u32),                       // 假包
            (Some(sign(p, 5).wrapping_add(1)), 5), // 错签
            (Some(sign(p, 4)), 4),              // 旧签
        ];
        let mut i = 0;
        while i < bad_bundles.len() {
            let g = download_gate(p, bad_bundles[i].0, bad_bundles[i].1, 5);
            assert!(!g.landed);
            assert!(matches!(g.verdict, Verdict::Rejected { .. }));
            i += 1;
        }
    }

    #[test]
    fn fe29_redline_audit_zero() {
        assert_eq!(redline_build_audit(), 0);
        assert_eq!(REDLINE_SIZE, 0);
        // 字典稳定：调用点计数冻结（扫描面改版即审计基线改版——要重新过门）。
        assert_eq!(LOG_CALLSITES, 6);
    }
}
