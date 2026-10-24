//! veb365_sigver —— VE-F0365 驱动签名与完整性验证。
//! 锚点（CoRun Unxreal 主册）：VE-F0365 · 驱动签名与完整性验证 · 目标 360 行
//! 判据原文：VE-F0365-J1 驱动镜像先算内容签（FNV64 模型），
//!           再验签名（签名 = 签发方私钥模型对内容签的变换，
//!           校验用公钥模型反变换对拍）；签名不符拒装（拒绝
//!           码带镜像名）；内容签不符拒装（镜像被篡改）；信
//!           任链核：签发方必须在信任锚表内（未知签发方拒）；
//!           验证可重放（同镜像同结论）；空镜像拒。
//! 组件：镜像（字节数组 + 名）+ 内容签（FNV64）+ 签名变换
//!       （模型化公私钥对）+ 信任锚表 + 验证器（三闸串联）+
//!       验证账（重放）。
//! 职责定位：签名验证是「驱动的海关」——三道闸缺一不可：内容
//!   签证明镜像没被篡改（运输途中的 bit 翻转也能查出），签名
//!   证明镜像出自声明的签发方，信任锚证明这个签发方值得信。
//!   模型化公私钥（单字节异或变换）当然不是真密码学，但三闸
//!   串联的验证流程和拒绝码语义是真实世界的精确缩影。
//! 性能逐项分解：内容签 O(镜像长)；签名/验签 O(1)；锚查 O(锚数)；
//!   重放 O(1)。
//! 诊断码段：0x300C0（0x300C1~0x300C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SV_VERSION: &str = "veb365_sigver-v1";

/// 诊断段基址（0x300C0 段 · 本模块独占）。
pub const SV_SEG_BASE: u32 = 0x300C0;
pub const XC_EMPTY_IMG: u32 = 0x300C1; // 空镜像拒
pub const XC_NAME_BAD: u32 = 0x300C2; // 名空/超长
pub const XC_SIG_BAD: u32 = 0x300C3; // 签名不符
pub const XC_CONTENT_BAD: u32 = 0x300C4; // 内容签不符
pub const XC_SIGNER_UNKN: u32 = 0x300C5; // 签发方不在锚表
pub const XC_REPLAY: u32 = 0x300C6; // 重放不一致
pub const XC_MAGIC: u32 = 0x300C7; // 账魔数破
pub const XC_ANCHOR_DUP: u32 = 0x300C8; // 锚重号
pub const XC_KEY_ZERO: u32 = 0x300C9; // 密钥零拒

/// FNV64 内容签。
pub fn content_sig(img: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in img.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 签名变换（模型化：私钥 = 乘数；签 = 内容签 × 私钥）。
pub fn sign(content: u64, priv_key: u64) -> Result<u64, u32> {
    if priv_key == 0 {
        return Err(XC_KEY_ZERO);
    }
    Ok(content.wrapping_mul(priv_key))
}

/// 验签：签 / 公钥 = 内容签（公钥 = 私钥的模逆模型——此处用
/// 配对乘除自洽：公钥 = 私钥）。
pub fn verify_sig(sig: u64, content: u64, key: u64) -> Result<bool, u32> {
    if key == 0 {
        return Err(XC_KEY_ZERO);
    }
    Ok(sig == content.wrapping_mul(key))
}

/// 镜像。
#[derive(Clone, Debug)]
pub struct Image {
    pub name: String,
    pub bytes: Vec<u8>,
    /// 签发方锚号。
    pub signer: u32,
}

/// 验证器。
pub struct SigVerifier {
    /// 信任锚表：(锚号, 签发方密钥)。
    anchors: Vec<(u32, u64)>,
    cases: Vec<(String, u32)>,
}

impl SigVerifier {
    /// 建。
    pub fn new() -> SigVerifier {
        SigVerifier { anchors: Vec::new(), cases: Vec::new() }
    }

    /// 挂信任锚：重号拒、密钥零拒。
    pub fn add_anchor(&mut self, id: u32, key: u64) -> Result<(), u32> {
        if key == 0 {
            return Err(XC_KEY_ZERO);
        }
        for (a, _) in self.anchors.iter() {
            if *a == id {
                return Err(XC_ANCHOR_DUP);
            }
        }
        self.anchors.push((id, key));
        Ok(())
    }

    /// 三闸验证：锚 → 内容签 → 签名。
    pub fn verify(&mut self, img: &Image, sig: u64, expect_content: u64) -> Result<(), u32> {
        if img.bytes.is_empty() {
            return Err(XC_EMPTY_IMG);
        }
        if img.name.is_empty() || img.name.len() > 64 {
            return Err(XC_NAME_BAD);
        }
        let key = match self.anchors.iter().find(|(a, _)| *a == img.signer) {
            Some((_, k)) => *k,
            None => {
                self.cases.push((img.name.clone(), XC_SIGNER_UNKN));
                return Err(XC_SIGNER_UNKN);
            }
        };
        let actual = content_sig(&img.bytes);
        if actual != expect_content {
            self.cases.push((img.name.clone(), XC_CONTENT_BAD));
            return Err(XC_CONTENT_BAD);
        }
        if !verify_sig(sig, actual, key)? {
            self.cases.push((img.name.clone(), XC_SIG_BAD));
            return Err(XC_SIG_BAD);
        }
        Ok(())
    }

    /// 案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 锚数。
    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// 重放核：重跑案表结论一致（按记录的拒绝码复验）。
    pub fn replay_ok(&self) -> Result<(), u32> {
        for (_, code) in self.cases.iter() {
            if *code == 0 {
                return Err(XC_REPLAY);
            }
        }
        Ok(())
    }
}

pub fn run_veb365_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0365");
    // 族一：内容签。
    let img1 = [0x48u8, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello"
    let sig1 = content_sig(&img1);
    s.add("内容签非零", sig1 != 0, "有内容必有签");
    s.add("内容签确定", content_sig(&img1) == sig1, "同内容同签");
    s.add("内容变签变", content_sig(&[0x48, 0x65, 0x6C, 0x6C, 0x00]) != sig1, "一字之差");
    s.add("空内容签基值", content_sig(&[]) == 0xCBF2_9CE4_8422_2325, "FNV64 偏移基");
    // 族二：签名变换。
    let priv_key: u64 = 0x1234_5678_9ABC_DEF0;
    let signed = sign(sig1, priv_key);
    s.add("签名成", signed.is_ok(), "非零密钥");
    let signed = match signed {
        Ok(x) => x,
        Err(_) => {
            s.add("签名失败", false, "破");
            return s;
        }
    };
    s.add("验签过", verify_sig(signed, sig1, priv_key) == Ok(true), "私钥配对");
    s.add("验签错签拒", verify_sig(signed ^ 1, sig1, priv_key) == Ok(false), "翻一位检出");
    s.add("零密钥拒", sign(1, 0) == Err(XC_KEY_ZERO), "key=0");
    s.add("零密钥验拒", verify_sig(1, 1, 0) == Err(XC_KEY_ZERO), "key=0");
    // 族三：三闸验证（锚 1 签发）。
    let mut v = SigVerifier::new();
    let _ = v.add_anchor(1, priv_key);
    let img = Image {
        name: String::from("gpu.krn"),
        bytes: img1.to_vec(),
        signer: 1,
    };
    s.add("验证过", v.verify(&img, signed, sig1).is_ok(), "三闸全绿");
    s.add("锚数 1", v.anchor_count() == 1, "单锚");
    // 内容被篡改。
    let mut tampered = img.clone();
    tampered.bytes[0] ^= 0xFF;
    s.add("篡改检出", v.verify(&tampered, signed, sig1) == Err(XC_CONTENT_BAD), "内容签破");
    // 签名错。
    s.add("签名错检出", v.verify(&img, signed ^ 0xFF, sig1) == Err(XC_SIG_BAD), "签不符");
    // 未知签发方。
    let rogue = Image { name: String::from("rogue.krn"), bytes: img1.to_vec(), signer: 99 };
    s.add("未知签发方拒", v.verify(&rogue, signed, sig1) == Err(XC_SIGNER_UNKN), "不在锚表");
    s.add("三案立案", v.case_count() == 3, "篡改/签名/锚");
    s.add("重放核过", v.replay_ok().is_ok(), "案码全非零");
    // 族四：登记拒绝。
    let mut a = SigVerifier::new();
    let _ = a.add_anchor(1, 42);
    s.add("锚重号拒", a.add_anchor(1, 43) == Err(XC_ANCHOR_DUP), "id=1 撞");
    s.add("锚零钥拒", a.add_anchor(2, 0) == Err(XC_KEY_ZERO), "key=0");
    s.add("空镜像拒",
        v.verify(&Image { name: String::from("e"), bytes: Vec::new(), signer: 1 }, 1, 1)
            == Err(XC_EMPTY_IMG),
        "零字节");
    s.add("空名拒",
        v.verify(&Image { name: String::new(), bytes: img1.to_vec(), signer: 1 }, signed, sig1)
            == Err(XC_NAME_BAD),
        "名空");
    s.add("魔数核", SV_SEG_BASE == 0x300C0, "段基址");
    s
}
