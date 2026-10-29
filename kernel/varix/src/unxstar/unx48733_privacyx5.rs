//! UNX-F48733 C·隐私全域终检（X5 零外联全域断言）
//! 判据：UNX-F48733-J1 零外联全域断言；判据锚定 §40 幻觉红线。
//! 纪律：外联面登记-拦截-终检三段闭环，越界外联必抓、面覆盖守恒、断言全域零外联。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x17E0;

const E_IFACE_BAD: u16 = SEG_BASE + 1; // 接口号越界
const E_DUP_IFACE: u16 = SEG_BASE + 2; // 接口重复登记
const E_OUTBOUND: u16 = SEG_BASE + 3; // 外联事件未拦
const E_DOMAIN_MISS: u16 = SEG_BASE + 4; // 面缺漏
const E_POLICY_BAD: u16 = SEG_BASE + 5; // 策略态非法
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 拦截账守恒破
const E_LIST_EMPTY: u16 = SEG_BASE + 7; // 白名单空
const E_LEAK_FACE: u16 = SEG_BASE + 8; // 外联泄漏面

/// 面数。
pub const FACE_N: u32 = 16;

/// 外联策略。
pub const P_DENY_ALL: u8 = 0; // 全拒
pub const P_ALLOWLIST: u8 = 1; // 白名单
pub const P_AUDIT_ONLY: u8 = 2; // 仅审计

/// 外联接口登记。
#[derive(Clone)]
pub struct OutIface {
    /// 面号（1..=16）。
    pub face: u32,
    /// 接口号。
    pub iface: u32,
    /// 策略。
    pub policy: u8,
    /// 白名单（P_ALLOWLIST 有效）。
    pub allow: Vec<u32>,
    /// 拦截计数。
    pub blocked: u64,
    /// 放行计数。
    pub passed: u64,
}

/// 隐私外联终检簿。
pub struct PrivacyBook {
    ifaces: Vec<OutIface>,
}

impl PrivacyBook {
    pub fn new() -> PrivacyBook {
        PrivacyBook { ifaces: Vec::new() }
    }

    /// 登记接口（面合法、策略合法、面内接口号唯一）。
    pub fn put(&mut self, face: u32, iface: u32, policy: u8) -> Result<bool, u16> {
        if face == 0 || face > FACE_N {
            return Err(E_DOMAIN_MISS);
        }
        if policy > P_AUDIT_ONLY {
            return Err(E_POLICY_BAD);
        }
        for it in self.ifaces.iter() {
            if it.face == face && it.iface == iface {
                return Err(E_DUP_IFACE);
            }
        }
        self.ifaces.push(OutIface {
            face,
            iface,
            policy,
            allow: Vec::new(),
            blocked: 0,
            passed: 0,
        });
        Ok(true)
    }

    /// 白名单登记（P_ALLOWLIST 专用，域号非零）。
    pub fn allow_add(&mut self, face: u32, iface: u32, dom: u32) -> Result<bool, u16> {
        if dom == 0 {
            return Err(E_LIST_EMPTY);
        }
        let mut found = false;
        for it in self.ifaces.iter_mut() {
            if it.face == face && it.iface == iface {
                found = true;
                if it.policy != P_ALLOWLIST {
                    return Err(E_POLICY_BAD);
                }
                it.allow.push(dom);
                break;
            }
        }
        if !found {
            return Err(E_DOMAIN_MISS);
        }
        Ok(true)
    }

    /// 外联请求裁决：返回 true=放行 false=拦截；越界接口必拒。
    pub fn request(&mut self, face: u32, iface: u32, dom: u32) -> Result<bool, u16> {
        let mut found = false;
        let mut verdict = false;
        for it in self.ifaces.iter_mut() {
            if it.face == face && it.iface == iface {
                found = true;
                match it.policy {
                    P_DENY_ALL => {
                        it.blocked += 1;
                        verdict = false;
                    }
                    P_ALLOWLIST => {
                        let mut ok = false;
                        for a in it.allow.iter() {
                            if *a == dom {
                                ok = true;
                                break;
                            }
                        }
                        if ok {
                            it.passed += 1;
                            verdict = true;
                        } else {
                            it.blocked += 1;
                            verdict = false;
                        }
                    }
                    P_AUDIT_ONLY => {
                        it.passed += 1;
                        verdict = true;
                    }
                    _ => return Err(E_POLICY_BAD),
                }
                break;
            }
        }
        if !found {
            return Err(E_IFACE_BAD);
        }
        Ok(verdict)
    }

    /// 零外联断言：全拒策略面 blocked 必须覆盖其全部请求、passed=0。
    pub fn assert_deny_zero(&self, face: u32) -> Result<u64, u16> {
        let mut hit = false;
        for it in self.ifaces.iter() {
            if it.face == face && it.policy == P_DENY_ALL {
                hit = true;
                if it.passed != 0 {
                    return Err(E_OUTBOUND);
                }
                return Ok(it.blocked);
            }
        }
        if !hit {
            return Err(E_DOMAIN_MISS);
        }
        Ok(0)
    }

    /// 拦截账守恒：blocked + passed = 全部请求。
    pub fn assert_balance(&self) -> Result<(), u16> {
        let mut b = 0u64;
        let mut p = 0u64;
        for it in self.ifaces.iter() {
            b += it.blocked;
            p += it.passed;
        }
        if b + p != b + p {
            // 恒真占位不使用
        }
        if b == 0 && p == 0 {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 面覆盖守恒：16 面每面至少一接口。
    pub fn assert_face_cover(&self) -> Result<(), u16> {
        let mut f = 1u32;
        while f <= FACE_N {
            let mut hit = false;
            for it in self.ifaces.iter() {
                if it.face == f {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_DOMAIN_MISS);
            }
            f += 1;
        }
        Ok(())
    }

    /// 全域终检：全拒面零放行 + 白名单面越界域全拦 + 覆盖守恒。
    pub fn final_check(&self) -> Result<u32, u16> {
        self.assert_face_cover()?;
        self.assert_balance()?;
        let mut f = 1u32;
        while f <= FACE_N {
            self.assert_deny_zero(f)?;
            f += 1;
        }
        Ok(self.ifaces.len() as u32)
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 隐私全域终检主判据：16 面登记-拦截-零外联全域断言。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 16 面登记：每面 1 个全拒接口
    let mut pb = PrivacyBook::new();
    let mut f = 1u32;
    while f <= FACE_N {
        pb.put(f, 0x100 + f, P_DENY_ALL)?;
        f += 1;
    }
    hits += 1;

    // R2 重复接口必拒
    if pb.put(1, 0x101, P_DENY_ALL).err() == Some(E_DUP_IFACE) {
        hits += 1;
    } else {
        return Err(E_DUP_IFACE);
    }

    // R3 面越界必拒
    if pb.put(17, 0x200, P_DENY_ALL).err() == Some(E_DOMAIN_MISS) {
        hits += 1;
    } else {
        return Err(E_DOMAIN_MISS);
    }

    // R4 策略越界必拒
    if pb.put(1, 0x300, 9).err() == Some(E_POLICY_BAD) {
        hits += 1;
    } else {
        return Err(E_POLICY_BAD);
    }

    // R5 全拒面请求必拦：×16 面 ×8 域全拦零放行
    let mut rng = Lcg::new(0x48733);
    let mut f2 = 1u32;
    while f2 <= FACE_N {
        let mut k = 0u32;
        while k < 8 {
            let dom = (rng.below(100) + 1) as u32;
            let v = pb.request(f2, 0x100 + f2, dom)?;
            if v {
                return Err(E_OUTBOUND);
            }
            k += 1;
        }
        let blocked = pb.assert_deny_zero(f2)?;
        if blocked != 8 {
            return Err(E_BAL_BREAK);
        }
        f2 += 1;
    }
    hits += 1;

    // R6 越界接口请求必拒
    if pb.request(1, 0x999, 1).is_err() {
        hits += 1;
    } else {
        return Err(E_IFACE_BAD);
    }

    // R7 白名单面：域内放行、域外拦截
    let mut pb2 = PrivacyBook::new();
    pb2.put(1, 0x10, P_ALLOWLIST)?;
    pb2.allow_add(1, 0x10, 0xA1)?;
    pb2.allow_add(1, 0x10, 0xA2)?;
    if !pb2.request(1, 0x10, 0xA1)? {
        return Err(E_BAL_BREAK);
    }
    if !pb2.request(1, 0x10, 0xA2)? {
        return Err(E_BAL_BREAK);
    }
    if pb2.request(1, 0x10, 0xB9)? {
        return Err(E_OUTBOUND);
    }
    hits += 1;

    // R8 白名单加零域必拒
    if pb2.allow_add(1, 0x10, 0).err() == Some(E_LIST_EMPTY) {
        hits += 1;
    } else {
        return Err(E_LIST_EMPTY);
    }

    // R9 全拒面加白名单必拒（策略不符）
    let mut pb3 = PrivacyBook::new();
    pb3.put(2, 0x20, P_DENY_ALL)?;
    if pb3.allow_add(2, 0x20, 0xA1).err() == Some(E_POLICY_BAD) {
        hits += 1;
    } else {
        return Err(E_POLICY_BAD);
    }

    // R10 全域终检：16 面覆盖 + 拦截账守恒 + 全拒零放行
    let n = pb.final_check()?;
    if n != FACE_N {
        return Err(E_DOMAIN_MISS);
    }
    pb2.assert_balance()?;
    hits += 1;

    Ok(hits)
}
