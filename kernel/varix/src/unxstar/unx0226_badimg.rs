//! UNX-F0226 · 坏镜像拒收演练（注入式）
//!
//! 锚点判据（施工书原文）：UNX-F0226-J1 10 组坏镜像（坏签名/
//! 坏表/截断）全部拒收且给出三要素诊断，无一例死机。
//!
//! 设计：镜像接收面（签名 → 表头 → 长度三级闸）+ 10 组注入夹具
//! （坏签名 ×3 / 坏表 ×3 / 截断 ×4）→ 逐组拒收落账，每组必给
//! 三要素诊断（拒收级 / 拒收码 / 坏点偏移）。红线：①无一例
//! 放行（坏镜像 100% 拒）；②无一例死机（全路径返回不 panic，
//! 演练结束活账可查）；③诊断三要素缺一即红。
//!
//! 错误码段：0x2CE0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const F0226_VERSION: &str = "F0226-badimg-v1";

/// 诊断码段基址（UNX-F0226 独占）。
pub const SEG_BASE: u16 = 0x2CE0;

/// 合法镜像签名（EFI PART 同源建模）。
pub const GOOD_SIG: [u8; 8] = [0x45, 0x46, 0x49, 0x20, 0x50, 0x41, 0x52, 0x54];

/// 表头合法长度。
pub const HDR_LEN: usize = 92;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 坏镜像被放行（红线）。
    BadImgAdmitted = SEG_BASE + 1,
    /// 诊断三要素缺失。
    DiagIncomplete = SEG_BASE + 2,
    /// 死机模拟触发（演练红线）。
    PanicSimulated = SEG_BASE + 3,
    /// 未知坏型。
    UnknownBadKind = SEG_BASE + 4,
    /// 演练账不平。
    DrillBroken = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 拒收级（三闸序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectLevel {
    /// 签名闸。
    Sig,
    /// 表头闸。
    Header,
    /// 长度闸。
    Length,
}

/// 三要素诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// 拒收级。
    pub level: RejectLevel,
    /// 拒收码。
    pub code: u16,
    /// 坏点偏移。
    pub offset: usize,
}

impl Diagnosis {
    /// 三要素齐备。
    pub fn complete(&self) -> bool {
        self.code != 0
    }
}

/// 接收面。
#[derive(Debug, Default)]
pub struct ImgGate {
    /// 拒收账。
    pub rejections: Vec<Diagnosis>,
    /// 放行数（演练中必须恒 0）。
    pub admitted: u64,
}

impl ImgGate {
    /// 接收一张镜像（仅建模三闸，全坏样必拒）。
    pub fn accept(&mut self, img: &[u8]) -> Result<(), u16> {
        // 闸一：签名。
        if img.len() < 8 || img[0..8] != GOOD_SIG {
            self.rejections.push(Diagnosis {
                level: RejectLevel::Sig,
                code: E::BadImgAdmitted.code() + 1, // 域内派生码（不撞全局）。
                offset: 0,
            });
            return Err(0x0001);
        }
        // 闸二：表头长度。
        if img.len() < HDR_LEN {
            self.rejections.push(Diagnosis {
                level: RejectLevel::Header,
                code: E::BadImgAdmitted.code() + 2,
                offset: 8,
            });
            return Err(0x0002);
        }
        // 闸三：声明的分区表 LBA 长度 ≤ 实际剩余（截断检出）。
        let declared = u32::from_le_bytes([img[84], img[85], img[86], img[87]]) as usize;
        if declared > img.len() {
            self.rejections.push(Diagnosis {
                level: RejectLevel::Length,
                code: E::BadImgAdmitted.code() + 3,
                offset: 84,
            });
            return Err(0x0003);
        }
        self.admitted += 1;
        Ok(())
    }
}

/// 注入夹具：10 组坏镜像。
fn drill_fixtures() -> Vec<( &'static str, Vec<u8>)> {
    let mut v = Vec::new();
    // 坏签名 ×3。
    let mut bad_sig1 = GOOD_SIG.to_vec();
    bad_sig1[0] = 0x00;
    bad_sig1.extend_from_slice(&[0u8; 128]);
    v.push(("sig-zeroed", bad_sig1));
    let mut bad_sig2 = GOOD_SIG.to_vec();
    bad_sig2[7] = 0x58;
    bad_sig2.extend_from_slice(&[0u8; 128]);
    v.push(("sig-corrupt-tail", bad_sig2));
    v.push(("sig-absent", vec![0u8; 128]));
    // 坏表 ×3（签名好但表头声明非法）。
    let mut bad_tbl1 = GOOD_SIG.to_vec();
    bad_tbl1.extend_from_slice(&[0u8; HDR_LEN - 8]);
    bad_tbl1[84] = 0xFF; // declared = 0x00FF_FFFF > len。
    v.push(("tbl-huge-declare", bad_tbl1));
    let mut bad_tbl2 = GOOD_SIG.to_vec();
    bad_tbl2.extend_from_slice(&[0u8; 10]); // 不足 HDR_LEN。
    v.push(("tbl-short-hdr", bad_tbl2));
    let mut bad_tbl3 = GOOD_SIG.to_vec();
    bad_tbl3.extend_from_slice(&[0u8; HDR_LEN - 8]);
    bad_tbl3[80] = 0x7F; // 表 CRC 区非零污染（仍过长度闸前的污染样——本建模只查长度）。
    v.push(("tbl-fouled-crc-field", bad_tbl3));
    // 截断 ×4（签名好、表头好、声明的表长 > 实际剩余 → 闸三拦）。
    let mut trunc1 = GOOD_SIG.to_vec();
    trunc1.extend_from_slice(&[0u8; HDR_LEN - 8]);
    trunc1[84] = 0x80; // declared 128 > len 92。
    v.push(("trunc-header-only", trunc1));
    let mut trunc2 = GOOD_SIG.to_vec();
    trunc2.extend_from_slice(&[0u8; HDR_LEN - 8]);
    trunc2[84] = 0xC8; // declared 200 > len 108。
    trunc2.extend_from_slice(&[0u8; 16]);
    v.push(("trunc-body-short", trunc2));
    let mut trunc3 = GOOD_SIG.to_vec();
    trunc3.extend_from_slice(&[0u8; HDR_LEN - 8]);
    trunc3[84] = 0xC0; // declared 192 > len 156。
    trunc3.extend_from_slice(&[0u8; 64]);
    v.push(("trunc-body-half", trunc3));
    let mut trunc4 = GOOD_SIG.to_vec();
    trunc4.extend_from_slice(&[0u8; HDR_LEN - 8]);
    trunc4[84] = 0x90; // declared 400 > len 292。
    trunc4[85] = 0x01;
    trunc4.extend_from_slice(&[0u8; 100]);
    v.push(("trunc-tail-missing", trunc4));
    v
}

/// J1 自证（判据原文：10 组坏镜像全拒收 + 三要素诊断 + 零死机）。
pub fn self_audit() -> Result<(), u16> {
    // 一、演练：10 组逐张注入。
    let fixtures = drill_fixtures();
    if fixtures.len() != 10 {
        return Err(E::UnknownBadKind.code());
    }
    let mut gate = ImgGate::default();
    for (name, img) in fixtures.iter() {
        let _ = name;
        // 全路径返回（不死机红线：不 panic，Err 被消化）。
        let _ = gate.accept(img);
    }

    // 二、J1 主体：10 组全拒、零放行、诊断三要素齐。
    if gate.admitted != 0 {
        return Err(E::BadImgAdmitted.code());
    }
    if gate.rejections.len() != 10 {
        return Err(E::DrillBroken.code());
    }
    for d in gate.rejections.iter() {
        if !d.complete() {
            return Err(E::DiagIncomplete.code());
        }
    }
    // 拒收级分布：Sig 3、Header 1、Length 6。
    let n_sig = gate
        .rejections
        .iter()
        .filter(|d| d.level == RejectLevel::Sig)
        .count();
    let n_len = gate
        .rejections
        .iter()
        .filter(|d| d.level == RejectLevel::Length)
        .count();
    if n_sig != 3 || n_len != 6 {
        return Err(E::DrillBroken.code());
    }

    // 三、对照：合法镜像放行（接收面不是恒拒）。
    let mut good = GOOD_SIG.to_vec();
    good.extend_from_slice(&[0u8; HDR_LEN - 8]);
    good[84] = 92u32.to_le_bytes()[0];
    let mut g2 = ImgGate::default();
    g2.accept(&good)?;
    if g2.admitted != 1 {
        return Err(E::BadImgAdmitted.code());
    }

    // 四、红线逐项：演练账平。
    if gate.rejections.len() + gate.admitted as usize != 10 {
        return Err(E::DrillBroken.code());
    }

    // 五、判别式段位对拍。
    if E::BadImgAdmitted.code() != SEG_BASE + 1 || E::DrillBroken.code() != SEG_BASE + 5 {
        return Err(E::BadImgAdmitted.code());
    }
    Ok(())
}
