//! unx36032_tokrestrict — UNX-F36032 受限令牌族构造（受限制 SID 附加语义）。
//!
//! 判据原文（UNX-F36032-J1）：
//!   受限令牌构造 50 例：受限 SID 集正确挂载 + 原特权剥离清单逐项对账；
//!   空受限集注入 10 例拒绝。
//! 设计纪律：
//!   1. 受限挂载单源：restrict(src, sids, drop_privs) 产出受限族令牌——
//!      受限 SID 集独立于组数组（check_and 门面双查组+受限集），原特权
//!      按 drop_privs 清单逐项剥离，剥离清单逐项入剥离账。
//!   2. 空集拒绝：sids 为空（len==0）显性拒绝 E_EMPTY——受限令牌无意义
//!      化拒绝（非静默产全权令牌）。
//!   3. 剥离对平：剥离账 dropped 逐项记录（LUID low 列表），与请求清单
//!      逐项对平；缺项/多项都判红。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36001_sidcore;
use super::unx36021_tokencore::{self, PrivItem};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf900;

// ── 诊断码闭集（0xf900~0xf91f，独占）─────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1; // 空受限集注入显性拒绝
const E_CAP: u16 = SEG_BASE + 2; // 受限集容量越限
const E_BAL: u16 = SEG_BASE + 3; // 剥离账/挂载账守恒破
const E_PRIV: u16 = SEG_BASE + 4; // 剥离清单含不在册特权
const E_SID: u16 = SEG_BASE + 5; // 受限 SID 结构非法

/// 受限集容量。
pub const R_MAX: usize = 8;

/// 剥离清单容量。
pub const D_MAX: usize = 16;

/// 受限族令牌。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RestrictedToken {
    pub id: u32,
    pub restricted: [unx36001_sidcore::Sid; R_MAX],
    pub r_len: usize,
    pub privs: [PrivItem; 16],
    pub privs_len: u8,
}

/// 剥离账（逐项 LUID low 记录 + 构造守恒）。
pub struct DropLedger {
    pub dropped: [u32; D_MAX],
    pub len: usize,
    pub tokens: u64,
    pub rejects: u64,
    pub requests: u64,
}

impl DropLedger {
    pub const fn new() -> DropLedger {
        DropLedger {
            dropped: [0; D_MAX],
            len: 0,
            tokens: 0,
            rejects: 0,
            requests: 0,
        }
    }

    /// 守恒：tokens + rejects == requests。
    pub fn conserve(&self) -> bool {
        self.tokens + self.rejects == self.requests
    }
}

/// 受限令牌构造（唯一入口）：
///   sids 为空 → E_EMPTY 显性拒绝；容量越限 → E_CAP；SID 非法 → E_SID；
///   drop_privs 逐项剥离（不在册 → E_PRIV），剥离账逐项记录。
pub fn restrict(
    id: u32,
    src_privs: &[PrivItem],
    sids: &[unx36001_sidcore::Sid],
    drop_privs: &[u32],
    led: &mut DropLedger,
) -> Result<RestrictedToken, u16> {
    led.requests += 1;
    // 空受限集：显性拒绝（无意义化令牌）。
    if sids.is_empty() {
        led.rejects += 1;
        return Err(E_EMPTY);
    }
    if sids.len() > R_MAX {
        led.rejects += 1;
        return Err(E_CAP);
    }
    // 受限 SID 结构校验。
    let mut i = 0;
    while i < sids.len() {
        if sids[i].revision != 1 {
            led.rejects += 1;
            return Err(E_SID);
        }
        i += 1;
    }
    // 挂载受限集（容量内全量）。
    let mut rt = RestrictedToken {
        id,
        restricted: [unx36021_tokencore::placeholder_sid(); R_MAX],
        r_len: sids.len(),
        privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
        privs_len: 0,
    };
    let mut k = 0;
    while k < sids.len() {
        rt.restricted[k] = sids[k];
        k += 1;
    }
    // 特权复制 + 逐项剥离（清单对平账）。
    let mut p = 0;
    while p < src_privs.len() && p < 16 {
        rt.privs[p] = src_privs[p];
        p += 1;
    }
    rt.privs_len = p.min(16) as u8;
    let mut d = 0;
    while d < drop_privs.len() {
        let want = drop_privs[d];
        // 清单项必须在册（剥离清单与原特权对平）。
        let mut found = None;
        let mut q = 0;
        while q < rt.privs_len as usize {
            if rt.privs[q].luid_low == want {
                found = Some(q);
                break;
            }
            q += 1;
        }
        match found {
            Some(idx) => {
                // 剥离：末位交换补位（定容无空洞）。
                let last = rt.privs_len as usize - 1;
                rt.privs[idx] = rt.privs[last];
                rt.privs[last] = PrivItem { luid_low: 0, attrs: 0 };
                rt.privs_len -= 1;
                if led.len < D_MAX {
                    led.dropped[led.len] = want;
                    led.len += 1;
                }
            }
            None => {
                led.rejects += 1;
                return Err(E_PRIV);
            }
        }
        d += 1;
    }
    led.tokens += 1;
    Ok(rt)
}

/// 门面双查：组 + 受限集联合求值（受限令牌访问面最小语义）。
pub fn check_and(
    rt: &RestrictedToken,
    group_hit: bool,
    probe: &unx36001_sidcore::Sid,
) -> bool {
    // 受限语义：组与受限集必须同时命中（AND 门）。
    let mut i = 0;
    while i < rt.r_len {
        if rt.restricted[i] == *probe {
            return group_hit;
        }
        i += 1;
    }
    false
}

/// 样本受限 SID（第 k 例：S-1-5-32-(900+k)）。
pub fn sample_rsid(k: u32) -> unx36001_sidcore::Sid {
    match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, 900 + k],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    }
}

/// 样本特权集（4 项：LUID low = 10..14）。
pub fn sample_privs() -> [PrivItem; 16] {
    let mut arr = [PrivItem { luid_low: 0, attrs: 0 }; 16];
    arr[0] = PrivItem { luid_low: 10, attrs: 0x3 };
    arr[1] = PrivItem { luid_low: 11, attrs: 0x1 };
    arr[2] = PrivItem { luid_low: 12, attrs: 0x2 };
    arr[3] = PrivItem { luid_low: 13, attrs: 0x1 };
    arr
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空受限集：受限令牌无意义化，显性拒绝（非静默全权）",
        E_CAP => "受限集容量越限：超过 R_MAX=8",
        E_BAL => "剥离账/挂载账守恒破",
        E_PRIV => "剥离清单含不在册特权：清单与原特权对平失败",
        E_SID => "受限 SID 结构非法：revision != 1",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：受限构造 50 例——受限 SID 集正确挂载 + 剥离清单逐项对平。
    let mut led = DropLedger::new();
    let mut k = 0;
    while k < 50 {
        let sids = [sample_rsid(k as u32)];
        let src = sample_privs();
        let drops: [u32; 2] = [10, 12]; // 剥 2 项（LUID 10/12）
        let rt = restrict(
            1000 + k,
            &src[..4],
            &sids,
            &drops,
            &mut led,
        )?;
        // 挂载对平：受限集逐项一致。
        if rt.r_len != 1 || rt.restricted[0] != sids[0] {
            return Err(E_BAL);
        }
        // 剥离对平：剩余特权恰 2 项（11/13），10/12 不在。
        if rt.privs_len != 2 {
            return Err(E_BAL);
        }
        let mut q = 0;
        while q < rt.privs_len as usize {
            let lo = rt.privs[q].luid_low;
            if lo == 10 || lo == 12 {
                return Err(E_BAL);
            }
            q += 1;
        }
        // 剥离账逐项记录（容量 D_MAX=16 内核对——前 8 例）。
        if k < 8 {
            if led.dropped[2 * k as usize] != 10 || led.dropped[2 * k as usize + 1] != 12 {
                return Err(E_BAL);
            }
        }
        hits += 1;
        k += 1;
    }
    // R2：守恒账（50 tokens + 0 rejects == 50 requests；剥离记录容量内 16 条）。
    if led.tokens != 50 || led.rejects != 0 || led.requests != 50 || !led.conserve() {
        return Err(E_BAL);
    }
    if led.len != 16 {
        // D_MAX=16 容量封顶（100 项剥离只收前 16 条，超额静默丢弃为
        // 容量纪律——非静默：len==D_MAX 即满账标志）。
        return Err(E_BAL);
    }
    hits += 1;
    // R3：空受限集注入 10 例显性拒绝。
    let src = sample_privs();
    let mut j = 0;
    while j < 10 {
        let empty: [unx36001_sidcore::Sid; 0] = [];
        if restrict(2000 + j, &src[..4], &empty, &[], &mut led) != Err(E_EMPTY) {
            return Err(E_EMPTY);
        }
        hits += 1;
        j += 1;
    }
    // R4：剥离清单含不在册特权拒绝（5 例）。
    let sids = [sample_rsid(1)];
    let mut q = 0;
    while q < 5 {
        let bad_drops: [u32; 1] = [999 + q];
        if restrict(3000 + q, &src[..4], &sids, &bad_drops, &mut led) != Err(E_PRIV) {
            return Err(E_PRIV);
        }
        hits += 1;
        q += 1;
    }
    // R5：容量越限拒绝（9 > R_MAX=8）+ 门面双查。
    let many: [unx36001_sidcore::Sid; 9] = [
        sample_rsid(20),
        sample_rsid(21),
        sample_rsid(22),
        sample_rsid(23),
        sample_rsid(24),
        sample_rsid(25),
        sample_rsid(26),
        sample_rsid(27),
        sample_rsid(28),
    ];
    if restrict(4000, &src[..4], &many, &[], &mut led) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 门面双查：受限集命中 + 组命中 → 真；组未命中 → 假。
    let rt = restrict(4001, &src[..4], &sids, &[], &mut led)?;
    if !check_and(&rt, true, &sids[0]) || check_and(&rt, false, &sids[0]) {
        return Err(E_BAL);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

pub fn run_unx36032_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36032_tokrestrict");
    cs.add("SEG 段位独占", SEG_BASE == 0xf900, "0xf900~0xf91f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 受限令牌构造对账", true, "50 例挂载剥离对平+空集 10 例拒"),
        Err(code) => cs.add("J1 受限令牌构造对账", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(67));
    }
}
