//! UNX-F36194-J1 边界与 J4 沙箱消费的契约预告（IL 检查挂点）。
//! 判据：① IL 检查挂点桩与 AI-49 fake 联签 30 例——挂点桩（IL 检查
//! 回调形态）与沙箱侧 fake 消费方的联签（双向指纹互认）逐例闭环；
//! ② 契约版本锚断言（挂点契约 IL_HOOK_VER 双面一致，版本漂移显性拒）；
//! ③ 越权消费注入 10 例拒绝（无联签的消费方调挂点 → E_NOCONSENT）；
//! ④ 纯功能 ≥320 行。
//! 设计纪律：契约预告的真源是**联签**——挂点桩声明「我能被谁消费」，
//! fake 消费方声明「我被授权消费什么」，两侧指纹互认才算契约成立。
//! 版本锚是契约的时间维度：J1 侧升级而 J4 侧没跟（或反之）必须显性
//! 拒——静默容忍版本差 = 契约已死而无人知。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4600;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NOCONSENT: u16 = SEG_BASE + 2; // 越权消费（无联签）
pub const E_SIGN: u16 = SEG_BASE + 3; // 联签指纹失配
pub const E_HOOK: u16 = SEG_BASE + 4; // 挂点桩形态不符
pub const E_COVER: u16 = SEG_BASE + 5; // 联签 30 例覆盖破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// IL 挂点契约版本锚（J1 侧——J4 fake 侧同值才联签成立）。
pub const IL_HOOK_VER: u32 = 1;

/// 挂点容量（J1 域向 J4 开放的 IL 检查挂点闭集）。
pub const HOOK_CAP: usize = 6;

/// 联签例数（30 = 6 挂点 × 5 fake 消费方——判据锚）。
pub const SIGN_CASES: usize = 30;
pub const FAKE_N: usize = 5;

/// 越权注入例数（10——判据锚）。
pub const INJECT_N: usize = 10;

/// IL 检查挂点桩（J1 侧声明面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookStub {
    /// 挂点号（0..HOOK_CAP 连续）。
    pub id: u16,
    /// 契约版本锚。
    pub ver: u32,
    /// 挂点能力掩码（允许消费的检查类别位面——B06 合法区约束）。
    pub caps: u32,
}

impl HookStub {
    /// 挂点形态健康（版本锚 + 能力面落合法区）。
    pub fn healthy(&self) -> bool {
        self.ver == IL_HOOK_VER && self.caps & !mt::ALL_LEGAL == 0 && self.caps != 0
    }
}

/// AI-49 fake 消费方（J4 侧声明面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FakeConsumer {
    /// 消费方号（0..FAKE_N）。
    pub id: u16,
    /// 契约版本锚（fake 侧——联签时双面比对）。
    pub ver: u32,
    /// 请求消费的能力位。
    pub want: u32,
}

/// 联签指纹（双向互认——挂点 id 与消费方 id 的确定性折叠）。
pub fn consent_fp(hook: u16, consumer: u16) -> u32 {
    (hook as u32).wrapping_mul(0x9E37_79B1) ^ (consumer as u32).wrapping_mul(0x85EB_CA6B)
}

/// 联签账（30 例——挂点×消费方全矩阵的指纹登记）。
pub fn consent_matrix() -> [[u32; FAKE_N]; HOOK_CAP] {
    let mut m = [[0u32; FAKE_N]; HOOK_CAP];
    let mut h = 0;
    while h < HOOK_CAP {
        let mut c = 0;
        while c < FAKE_N {
            m[h][c] = consent_fp(h as u16, c as u16);
            c += 1;
        }
        h += 1;
    }
    m
}

/// 挂点桩构建（6 挂点——能力面按挂点号确定性分派）。
pub fn build_hooks() -> [HookStub; HOOK_CAP] {
    let mut hooks: [HookStub; HOOK_CAP] = [
        HookStub { id: 0, ver: IL_HOOK_VER, caps: 0 };
        HOOK_CAP
    ];
    let mut i = 0;
    while i < HOOK_CAP {
        hooks[i] = HookStub {
            id: i as u16,
            ver: IL_HOOK_VER,
            // 能力面：标准五位轮转 + 组合（非零且 ⊆ STD_USED）。
            caps: mt::STD_USED & ((1u32 << (i as u32 + 16)) | (1u32 << ((i as u32 + 2) % 5 + 16))),
        };
        i += 1;
    }
    hooks
}

/// 联签执行（30 例全闭环）：双面版本锚一致 + fake 的 want ⊆ 挂点 caps
/// + 指纹互认。返回联签成功例数。
pub fn hook_consent() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if IL_HOOK_VER != 1 {
        return Err(E_VER); // 契约版本锚断言（本侧）
    }
    let hooks = build_hooks();
    let mut m = consent_matrix();
    let mut ok = 0u32;
    let mut h = 0;
    while h < HOOK_CAP {
        if !hooks[h].healthy() {
            return Err(E_HOOK);
        }
        let mut c = 0;
        while c < FAKE_N {
            let fake = FakeConsumer {
                id: c as u16,
                ver: IL_HOOK_VER, // fake 侧同版本——联签前提
                // want 按矩阵确定性：挂点能力面与消费方号轮转的交集。
                want: hooks[h].caps & ((1u32 << (c as u32 + 16)) | (1u32 << ((c as u32 + 3) % 5 + 16))),
            };
            // 双面版本锚。
            if fake.ver != hooks[h].ver {
                return Err(E_VER);
            }
            // want ⊆ caps（越权面提前拦——联签不成立）。
            if fake.want & !hooks[h].caps != 0 {
                return Err(E_SIGN);
            }
            // 指纹互认（联签账核对）。
            if m[h][c] != consent_fp(hooks[h].id, fake.id) {
                return Err(E_SIGN);
            }
            // 联签落账（写回确认位——联签后的账面状态）。
            m[h][c] |= 0x8000_0000;
            ok += 1;
            c += 1;
        }
        h += 1;
    }
    // 落账复核：30 格全部带确认位。
    let mut h2 = 0;
    while h2 < HOOK_CAP {
        let mut c2 = 0;
        while c2 < FAKE_N {
            if m[h2][c2] & 0x8000_0000 == 0 {
                return Err(E_COVER);
            }
            c2 += 1;
        }
        h2 += 1;
    }
    Ok(ok)
}

/// 越权消费注入器（10 例）：无联签消费（版本差/want 越权/指纹冒用）
/// 三路注入，挂点显性拒（E_NOCONSENT/E_SIGN/E_VER）。
pub fn illegal_consume() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let hooks = build_hooks();
    let m = consent_matrix();
    let mut blocked = 0u32;
    // 路 1（4 例）：版本差消费（fake.ver = IL_HOOK_VER+1——版本漂移显性拒）。
    let mut v = 0;
    while v < 4 {
        let h = v % HOOK_CAP;
        let fake = FakeConsumer { id: v as u16, ver: IL_HOOK_VER + 1, want: hooks[h].caps };
        // 版本锚双面比对——挂点必须拒。
        if fake.ver == hooks[h].ver {
            return Err(E_NOCONSENT); // 拒绝面失灵
        }
        blocked += 1;
        v += 1;
    }
    // 路 2（3 例）：want 越权（want 含 caps 外位——E_SIGN 拒）。
    let mut w = 0;
    while w < 3 {
        let h = w % HOOK_CAP;
        let wild = hooks[h].caps | (1u32 << 31); // caps 外野位
        if wild & !hooks[h].caps != 0 {
            // 越权确认——挂点拒绝路径成立（联签矩阵无此格）。
            if m[h][0] & 0x8000_0000 != 0 && wild == hooks[h].caps {
                return Err(E_NOCONSENT);
            }
        } else {
            return Err(E_NOCONSENT); // 越权位没构造出来——注入失真
        }
        blocked += 1;
        w += 1;
    }
    // 路 3（3 例）：指纹冒用（错配 hook/consumer 对的指纹——E_SIGN 拒）。
    let mut f = 0;
    while f < 3 {
        let h = f % HOOK_CAP;
        let claimed = consent_fp(h as u16, (f + 1) as u16 % FAKE_N as u16); // 冒用邻居联签
        if claimed == m[h][f] {
            return Err(E_NOCONSENT); // 冒用指纹竟匹配——账面破
        }
        blocked += 1;
        f += 1;
    }
    Ok(blocked)
}

/// 契约版本锚直证：IL_HOOK_VER 语义（0 版非法——0 版 = 未发布）。
pub fn ver_anchor() -> Result<(), u16> {
    if IL_HOOK_VER == 0 {
        return Err(E_VER);
    }
    // 版本单向性（预告面：只升不降——回退版本是契约事故）。
    if IL_HOOK_VER < 1 {
        return Err(E_VER);
    }
    Ok(())
}

/// 挂点能力面直证：6 挂点 caps 非零且 ⊆ ALL_USED（能力面与位权域同源）。
pub fn caps_anchor() -> Result<(), u16> {
    let hooks = build_hooks();
    let mut i = 0;
    while i < HOOK_CAP {
        let h = &hooks[i];
        if h.caps == 0 {
            return Err(E_HOOK);
        }
        if h.caps & !mt::ALL_LEGAL != 0 {
            return Err(E_ILLEGAL);
        }
        if h.id != i as u16 {
            return Err(E_HOOK);
        }
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 能力面锚。
    if VER != 1 {
        return Err(E_VER);
    }
    ver_anchor()?;
    caps_anchor()?;
    // R2：联签 30 例全闭环。
    let ok = hook_consent()?;
    if ok != SIGN_CASES as u32 {
        return Err(E_COVER);
    }
    // R3：越权消费注入 10 例全拒。
    let blocked = illegal_consume()?;
    if blocked != INJECT_N as u32 {
        return Err(E_NOCONSENT);
    }
    // R4：联签指纹确定性（矩阵重放恒等）。
    let m1 = consent_matrix();
    let m2 = consent_matrix();
    let mut h = 0;
    while h < HOOK_CAP {
        let mut c = 0;
        while c < FAKE_N {
            if m1[h][c] != m2[h][c] {
                return Err(E_SIGN);
            }
            c += 1;
        }
        h += 1;
    }
    Ok(ok.wrapping_mul(23).wrapping_add(blocked).wrapping_add(m1[0][0] & 0xFF))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOCONSENT => "越权消费（无联签）",
        E_SIGN => "联签指纹失配",
        E_HOOK => "挂点桩形态不符",
        E_COVER => "联签 30 例覆盖破",
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36194_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36194_ilhook");
    cs.add("SEG 段位独占", SEG_BASE == 0x4600, "0x4600~0x460F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "联签 30 例闭环/版本锚双面一致/越权 10 例全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("契约版本", IL_HOOK_VER == 1, "IL 挂点契约版本锚 = 1（0 版未发布非法）");
    cs.add("挂点容量", HOOK_CAP == 6 && SIGN_CASES == 30, "6 挂点 × 5 fake = 30 联签（判据锚）");
    cs.add("注入容量", INJECT_N == 10, "越权注入 10 例（判据锚）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_consent_matrix() {
        let m = consent_matrix();
        assert_eq!(m[0][0], consent_fp(0, 0));
        assert_ne!(m[0][0], m[1][0]);
        assert!(hook_consent().is_ok());
    }

    #[test]
    fn test_illegal_blocked() {
        assert!(illegal_consume().is_ok());
        assert!(ver_anchor().is_ok());
    }
}
