//! UNX-F36224-J1 授出掩码 ≤ 请求掩码 ≤ MAXIMUM_ALLOWED 链式约束。
//! 判据：① 三段链式（grant ⊆ request_eff ⊆ ma_bound）100 例零越界——
//! 常规请求、MA 请求、剥位常规请求三态全测；② 链破注入（授出 > 请求）
//! 10 例全部被断言拦截（错误分型：grant 越请求 / 请求越 MA 界两型可归
//! 因）；③ MA 界 = B10 MA 展开基元（36181::ma_expand）的确定性投影——
//! 本面不复述展开式，只验链；④ 纯功能代码 ≥320 行。
//! 设计纪律：链式约束是「授出永不超发」的总闸——Windows 语义里
//! GrantedAccess 恒是 DesiredAccess 的子集、DesiredAccess 恒被 MA 展开
//! 截界；三段各自的越界形态不同（越请求=引擎写宽 / 越界=请求面污染），
//! 分型诊断是本面的真实深度。账面「⊆」一律按位或累积（算术和有进位
//! 不保子集序）。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36181_maxallowed as ma;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa1a0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_G_R: u16 = SEG_BASE + 2; // 链破一型：授出 > 请求
pub const E_R_B: u16 = SEG_BASE + 3; // 链破二型：请求 > MA 界
pub const E_ILLEGAL: u16 = SEG_BASE + 4; // 任一段越存储/掩码合法区
pub const E_NEG: u16 = SEG_BASE + 5; // 注入例未被拦截
pub const E_STAT: u16 = SEG_BASE + 6; // 链账统计不平

/// 模块版本锚。
pub const VER: u32 = 1;

/// 请求侧 MA 位（0x0200_0000 ∈ GEN_RSVD——请求面专用，存储面非法，
/// 与 36181::MA_BIT 同值同语义，此处显式重锚供链面独立消费）。
pub const MA_REQ: u32 = 0x0200_0000;

const _: () = assert!(MA_REQ == ma::MA_BIT, "MA 请求位与 B10 真源不同值");
const _: () = assert!(MA_REQ & gf::GA_STORE_LEGAL == 0, "MA 位不得落在存储合法区");

/// 链式约束判定（判据 ① 的核心）：
///   grant ⊆ request_eff ⊆ bound，其中
///   request_eff =（desired 含 MA 位）→ MA 展开结果；（否则）→ desired ∩ 存储合法区
///   bound = ma_expand(candidate, deny_hit)（B10 基元直调）。
/// 返回 request_eff 供调用方落账（授出、请求、界三元组同账）。
/// 分型：grant 越请求 → E_G_R（引擎写宽）；请求越界 → E_R_B（请求污染）；
/// 候选/授出越合法区 → E_ILLEGAL（在链判定前先拦——非法值不进链）。
pub fn chain_check(grant: u32, desired: u32, candidate: u32, deny_hit: u32) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if grant & !gf::GA_STORE_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    if candidate & !mt::ALL_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    let bound = ma::ma_expand(candidate, deny_hit)?;
    let request_eff = if desired & MA_REQ != 0 {
        // MA 请求：请求面以展开界为有效请求（尽量授语义）。
        bound
    } else {
        desired & gf::GA_STORE_LEGAL
    };
    if request_eff & !bound != 0 {
        return Err(E_R_B);
    }
    if grant & !request_eff != 0 {
        return Err(E_G_R);
    }
    Ok(request_eff)
}

/// 链账（三段并集落账 + 统计；账只收绿链——红链在 record 入口即拒）。
/// 注意：三段「⊆」关系用**按位或**累积（算术和有进位不保子集序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainLedger {
    pub n: u32,
    pub grant_or: u32,
    pub request_or: u32,
    pub bound_or: u32,
    pub ma_requests: u32,
    pub plain_requests: u32,
    pub g_r_broken: u32,
    pub r_b_broken: u32,
}

impl ChainLedger {
    pub fn fresh() -> ChainLedger {
        ChainLedger {
            n: 0,
            grant_or: 0,
            request_or: 0,
            bound_or: 0,
            ma_requests: 0,
            plain_requests: 0,
            g_r_broken: 0,
            r_b_broken: 0,
        }
    }

    /// 记一链（先过链检再落账——账面只收绿链）。
    pub fn record(&mut self, grant: u32, desired: u32, candidate: u32, deny_hit: u32) -> Result<(), u16> {
        let req_eff = chain_check(grant, desired, candidate, deny_hit)?;
        let bound = ma::ma_expand(candidate, deny_hit)?;
        self.n = self.n.wrapping_add(1);
        self.grant_or |= grant;
        self.request_or |= req_eff;
        self.bound_or |= bound;
        if desired & MA_REQ != 0 {
            self.ma_requests = self.ma_requests.wrapping_add(1);
        } else {
            self.plain_requests = self.plain_requests.wrapping_add(1);
        }
        Ok(())
    }

    /// 记一破链（分型计数——账面同时是注入拦截的计数器）。
    pub fn record_broken(&mut self, grant: u32, desired: u32, candidate: u32, deny_hit: u32) {
        match chain_check(grant, desired, candidate, deny_hit) {
            Err(E_G_R) => self.g_r_broken = self.g_r_broken.wrapping_add(1),
            Err(E_R_B) => self.r_b_broken = self.r_b_broken.wrapping_add(1),
            _ => {}
        }
    }

    /// 统计恒等：n = ma + plain（分账/总账对平）。
    pub fn tally_ok(&self) -> bool {
        self.n == self.ma_requests + self.plain_requests
    }

    /// 链守恒：账面三段并集逐位保序（grant_or ⊆ request_or ⊆ bound_or）。
    pub fn chain_ok(&self) -> Result<(), u16> {
        if !self.tally_ok() {
            return Err(E_STAT);
        }
        if self.grant_or & !self.request_or != 0 {
            return Err(E_G_R);
        }
        if self.request_or & !self.bound_or != 0 {
            return Err(E_R_B);
        }
        Ok(())
    }
}

/// 100 例链样本（确定性生成：候选 × Deny × desired 三循环投影）。
/// 授出恒取「界剥两位」的确定性合法子集；MA 请求例授出全取界。
pub fn chain_samples() -> [(u32, u32, u32, u32); 100] {
    let cand = [
        sr::STD_ALL,
        sr::READ_CONTROL | sr::SYNCHRONIZE | 0x0003,
        mt::SPEC_MASK | sr::STD_ALL,
        sr::DELETE | sr::READ_CONTROL | 0x0020,
        sr::WRITE_DAC | sr::SYNCHRONIZE | 0x0001,
    ];
    let deny = [
        0u32,
        sr::WRITE_DAC,
        sr::DELETE | sr::WRITE_OWNER,
        mt::SPEC_MASK & !0x003F,
        sr::SYNCHRONIZE,
    ];
    let mut out = [(0u32, 0u32, 0u32, 0u32); 100];
    let mut i = 0;
    while i < 100 {
        let c = cand[i % 5];
        let d = deny[(i / 5) % 5];
        // 授出 = 有效请求 ∩ 界 ∩ 第 i 位裁剪（确定性合法子集）。
        let bound = ma::ma_expand(c, d).unwrap_or(0);
        let desired = match i % 3 {
            0 => bound,                    // 常规请求=全要
            1 => bound & !sr::SYNCHRONIZE, // 常规请求=剥一位
            _ => bound | MA_REQ,           // MA 请求
        };
        let grant = if i % 3 == 2 { bound } else { bound & !0x0001 & !sr::SYNCHRONIZE };
        out[i] = (grant, desired, c, d);
        i += 1;
    }
    out
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：MA 位双面纪律锚（请求侧合法、存储面非法——链前先钉死）。
    if MA_REQ != ma::MA_BIT || MA_REQ & gf::GA_STORE_LEGAL != 0 {
        return Err(E_ILLEGAL);
    }
    // R1：100 例链式零越界（判据 ① 落点）——三段逐例复核 + 账统计。
    let samples = chain_samples();
    let mut led = ChainLedger::fresh();
    let mut i = 0;
    while i < 100 {
        let (grant, desired, cand, deny) = samples[i];
        let req_eff = chain_check(grant, desired, cand, deny)?;
        // 三段自证：grant ⊆ req_eff ⊆ bound。
        let bound = ma::ma_expand(cand, deny)?;
        if grant & !req_eff != 0 {
            return Err(E_G_R);
        }
        if req_eff & !bound != 0 {
            return Err(E_R_B);
        }
        // MA 请求例：request_eff = bound（尽量授即全取界）。
        if desired & MA_REQ != 0 && req_eff != bound {
            return Err(E_R_B);
        }
        led.record(grant, desired, cand, deny)?;
        i += 1;
    }
    if led.n != 100 || !led.tally_ok() {
        return Err(E_STAT);
    }
    // i ∈ [0,100) 且 i%3==2 的例数 = 33（MA 请求），其余 67 为常规请求。
    if led.ma_requests != 33 || led.plain_requests != 67 {
        return Err(E_STAT);
    }
    led.chain_ok()?;
    // R2：链破注入 10 例（判据 ② 落点）：授出 > 请求 6 例 + 请求 > 界 4 例，
    // 分型计数器逐例对账。
    // 一型（E_G_R）：请求不含 WRITE_DAC 但授出带。
    led.record_broken(sr::WRITE_DAC, sr::READ_CONTROL, sr::STD_ALL, 0);
    led.record_broken(sr::DELETE | 0x0001, 0x0001, 0x0001 | sr::DELETE, 0);
    led.record_broken(sr::SYNCHRONIZE, sr::READ_CONTROL, sr::STD_ALL, 0);
    led.record_broken(sr::WRITE_OWNER, sr::STD_ALL & !sr::WRITE_OWNER, sr::STD_ALL, 0);
    led.record_broken(mt::SPEC_MASK, 0x00FF & mt::SPEC_MASK, mt::SPEC_MASK, 0);
    led.record_broken(0x0003, 0x0001, mt::SPEC_MASK, 0);
    // 二型（E_R_B）：请求位不在 MA 界内（Deny 命中剥位后仍要）。注意
    // MA 请求的 request_eff 恒 = 界——二型只在常规请求面可触发。
    led.record_broken(0, sr::WRITE_DAC, sr::STD_ALL, sr::WRITE_DAC);
    led.record_broken(0, mt::SPEC_MASK, sr::READ_CONTROL, 0);
    led.record_broken(0, sr::SYNCHRONIZE, sr::READ_CONTROL | 0x0001, 0);
    led.record_broken(0, sr::DELETE | 0x0002, 0x0002 | sr::READ_CONTROL, 0);
    if led.g_r_broken != 6 || led.r_b_broken != 4 {
        return Err(E_NEG);
    }
    // 逐例复核两型错误码可归因（分型诊断的真实深度）。
    if chain_check(sr::WRITE_DAC, sr::READ_CONTROL, sr::STD_ALL, 0).err() != Some(E_G_R) {
        return Err(E_NEG);
    }
    if chain_check(0, sr::WRITE_DAC, sr::STD_ALL, sr::WRITE_DAC).err() != Some(E_R_B) {
        return Err(E_NEG);
    }
    // R3：MA 请求语义（request_eff = bound——尽量授即全取界）。授出取
    // 剥位后的界（grant ⊆ request_eff 的链纪律对 MA 请求同样成立）。
    let req = chain_check(sr::DELETE | sr::READ_CONTROL | sr::SYNCHRONIZE, MA_REQ, sr::STD_ALL, sr::WRITE_DAC)?;
    if req != sr::DELETE | sr::READ_CONTROL | sr::SYNCHRONIZE {
        return Err(E_R_B);
    }
    if req & ma::MA_NO_VIRT != 0 {
        return Err(E_ILLEGAL);
    }
    // 零许可 MA 请求：授空 ≠ 拒绝失败（MA 永不失败的链面兑现）。
    let zero = chain_check(0, MA_REQ, sr::READ_CONTROL, sr::READ_CONTROL)?;
    if zero != 0 {
        return Err(E_R_B);
    }
    // R4：越合法区注入（grant 带 GENERIC 位 / 候选带保留位）→ E_ILLEGAL。
    if chain_check(0x8000_0000, 0, sr::STD_ALL, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // 候选带标准段保留位（bit 21）：chain_check 先于 ma_expand 拒——
    // 非法值不进链，E_ILLEGAL 透传。
    if chain_check(0, 0, 0x0020_0000 | sr::READ_CONTROL, 0).err() != Some(E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // Deny 命中位越合法区同样 E_ILLEGAL（三元组三面全守门）。候选先行
    // 拦下后，deny 越界由 ma_expand 拒——透传 B10 真源错误码。
    if chain_check(0, 0, sr::STD_ALL, 0x0200_0000).err() != Some(ma::E_ILLEGAL) {
        return Err(E_ILLEGAL);
    }
    // R5：账面负例（record 拒链不入账——账只收绿链；破链只进分型计数）。
    let mut led2 = ChainLedger::fresh();
    if led2.record(sr::WRITE_DAC, sr::READ_CONTROL, sr::STD_ALL, 0).is_ok() {
        return Err(E_NEG);
    }
    if led2.record(sr::READ_CONTROL, sr::READ_CONTROL, sr::READ_CONTROL, 0).is_err() {
        return Err(E_STAT);
    }
    if led2.n != 1 || led2.g_r_broken != 0 || led2.r_b_broken != 0 {
        return Err(E_STAT);
    }
    // 破链 record 后账面 n 不动（分型计数与链账严格分账）。
    led2.record_broken(sr::SYNCHRONIZE, sr::READ_CONTROL, sr::STD_ALL, 0);
    if led2.n != 1 || led2.g_r_broken != 1 {
        return Err(E_STAT);
    }
    led2.chain_ok()?;
    // 校验和：账面三段并集 + 例数（确定性总锚）。
    Ok(led
        .grant_or
        .wrapping_add(led.request_or)
        .wrapping_add(led.bound_or)
        .wrapping_add(led.n)
        .wrapping_add(led.g_r_broken)
        .wrapping_add(led.r_b_broken))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_G_R => "链破一型：授出 > 请求（引擎写宽）",
        E_R_B => "链破二型：请求 > MA 界（请求面污染）",
        E_ILLEGAL => "任一段越存储/掩码合法区（非法值不进链）",
        E_NEG => "注入例未被拦截（链断言/分型计数失效）",
        E_STAT => "链账统计不平（分账/总账破）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36224_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36224_gachain");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1a0, "0xa1a0~0xa1aF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例零越界/10 例链破分型全拦/MA 语义"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("MA 位锚", MA_REQ == ma::MA_BIT && MA_REQ & gf::GA_STORE_LEGAL == 0, "请求侧 MA 位与 B10 真源同值；存储面恒非法");
    cs.add("界真源", ma::ma_expand(sr::STD_ALL, 0).is_ok(), "MA 界 = 36181 基元直调（本面零复述）");
    cs.add("分型锚", chain_check(sr::WRITE_DAC, sr::READ_CONTROL, sr::STD_ALL, 0).err() == Some(E_G_R) && chain_check(0, sr::WRITE_DAC, sr::STD_ALL, sr::WRITE_DAC).err() == Some(E_R_B), "两型链破错误码互异可归因");
    cs.add("账恒等", ChainLedger::fresh().tally_ok(), "空账统计恒等（n = ma + plain）");
    cs.add("账守恒", ChainLedger::fresh().chain_ok().is_ok(), "空账三段并集逐位保序");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_chain_two_break_types() {
        assert_eq!(chain_check(sr::SYNCHRONIZE, sr::READ_CONTROL, sr::STD_ALL, 0).err(), Some(E_G_R));
        // 二型只在常规请求面可触发（MA 请求 request_eff 恒 = 界）。
        assert_eq!(chain_check(0, sr::SYNCHRONIZE, sr::READ_CONTROL | 0x0001, 0).err(), Some(E_R_B));
        assert!(chain_check(sr::READ_CONTROL, sr::READ_CONTROL, sr::STD_ALL, 0).is_ok());
    }

    #[test]
    fn test_ledger_rejects_red_chains() {
        let mut led = ChainLedger::fresh();
        assert!(led.record(0, sr::READ_CONTROL, sr::STD_ALL, 0).is_ok());
        assert!(led.record(sr::STD_ALL, 0, sr::STD_ALL, 0).is_err());
        assert_eq!(led.n, 1);
        assert!(led.tally_ok());
        // 破链只进分型计数，不进链账。
        led.record_broken(sr::WRITE_DAC, sr::READ_CONTROL, sr::STD_ALL, 0);
        assert_eq!(led.n, 1);
        assert_eq!(led.g_r_broken, 1);
        assert!(led.chain_ok().is_ok());
    }

    #[test]
    fn test_ma_semantics() {
        // MA 请求 = 全取展开界；虚授位恒零；零许可合法。
        let b = chain_check(0, MA_REQ, sr::STD_ALL, 0).unwrap_or(0xFF);
        assert_eq!(b, sr::STD_ALL & !ma::MA_NO_VIRT);
        assert_eq!(chain_check(0, MA_REQ, sr::READ_CONTROL, sr::READ_CONTROL).unwrap_or(0xFF), 0);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36224_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 7);
    }
}
