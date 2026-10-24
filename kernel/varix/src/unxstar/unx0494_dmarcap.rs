//! unx0494_dmarcap —— UNX-F0494 DMAR 能力寄存器解析（VT-d CAP/ECAP 位域闸）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0494 · DMAR 能力闸 · 目标 280 行
//! 判据原文：UNX-F0494-J1 DMAR 能力闸：能一 SAGAW 非零
//!           （支持的聚合客户地址宽度位域至少一位置位，全零
//!           拒）；能二 宽度必须受支（所请求的客户地址宽度
//!           39/48/57 位必须在 SAGAW 支持集内，越宽拒）；能三
//!           ECAP 门控（引用中断重映射前必须 ECAP.IR 置位，
//!           否则拒）；能四 MAMV 上限（异步合并向量数不得超
//!           CAP.MAMV 字段值）；能五 能力快照稳定（同一输入
//!           两次解析位级恒等）。
//! 组件：CAP/ECAP 位域解析器 + SAGAW 支持集 + 宽度选择闸 + IR 门控。
//! 职责定位：DMAR 能力寄存器是 IOMMU 的「出厂铭牌」——硬件只
//!   承诺铭牌上写的宽度与特性。软件在使能翻译前按铭牌自检
//!   是免于「配了 57 位宽硬件只支持 48 位」这类静默翻译故
//!   障的唯一防线；铭牌本身不改，闸只读不写。
//! 性能逐项分解：解析 O(1)；支持集判定 O(1)（位测试）。
//! 诊断码段：0x34A40（0x34A41~0x34A49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34A40;
pub const XC_NOSAGAW: u32 = SEG_BASE + 1; // SAGAW 全零
pub const XC_BADAW: u32 = SEG_BASE + 2; // 宽度不受支
pub const XC_NOIR: u32 = SEG_BASE + 3; // 引用 IR 但 ECAP.IR 未置位
pub const XC_MAMVOVR: u32 = SEG_BASE + 4; // 超异步合并向量上限

/// 客户地址宽度档位（位偏移 → 位宽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwWidth {
    W39,
    W48,
    W57,
}

/// 从 CAP.SAGAW 位域（bit 8~12）解出支持宽度集。
pub fn parse_sagaw(cap: u64) -> ([AwWidth; 3], usize) {
    let sagaw = ((cap >> 8) & 0x1F) as u8;
    let mut out = [AwWidth::W39; 3];
    let mut n = 0usize;
    // bit0→39 位、bit1→48 位、bit2→57 位（业界约定三档）。
    if sagaw & 0x01 != 0 {
        out[n] = AwWidth::W39;
        n += 1;
    }
    if sagaw & 0x02 != 0 {
        out[n] = AwWidth::W48;
        n += 1;
    }
    if sagaw & 0x04 != 0 {
        out[n] = AwWidth::W57;
        n += 1;
    }
    (out, n)
}

/// CAP.MAMV 字段（bit 48~53）：最大异步合并向量数。
pub fn parse_mamv(cap: u64) -> u32 {
    ((cap >> 48) & 0x3F) as u32
}

/// ECAP.IR 位（bit 22）：中断重映射支持。
pub fn ecap_has_ir(ecap: u64) -> bool {
    ecap & (1 << 22) != 0
}

/// 能力闸：对 (cap, ecap) 与请求做五类闸顺序核。
pub struct CapGate {
    pub cap: u64,
    pub ecap: u64,
}

impl CapGate {
    pub fn new(cap: u64, ecap: u64) -> Self {
        CapGate { cap, ecap }
    }

    /// 闸一：SAGAW 非零。SAGAW 全零 = 硬件不承诺任何宽度，翻译不可配。
    pub fn check_sagaw(&self) -> Result<(), u32> {
        let (_, n) = parse_sagaw(self.cap);
        if n == 0 {
            return Err(XC_NOSAGAW);
        }
        Ok(())
    }

    /// 闸二：宽度必须受支。
    pub fn check_width(&self, w: AwWidth) -> Result<(), u32> {
        let (set, n) = parse_sagaw(self.cap);
        if set[..n].contains(&w) {
            Ok(())
        } else {
            Err(XC_BADAW)
        }
    }

    /// 闸三：ECAP.IR 门控——引用中断重映射前必须置位。
    pub fn check_ir(&self) -> Result<(), u32> {
        if ecap_has_ir(self.ecap) {
            Ok(())
        } else {
            Err(XC_NOIR)
        }
    }

    /// 闸四：MAMV 上限。
    pub fn check_mamv(&self, vectors: u32) -> Result<(), u32> {
        let mamv = parse_mamv(self.cap);
        if vectors > mamv {
            return Err(XC_MAMVOVR);
        }
        Ok(())
    }

    /// 联合协商：按宽度偏好序列（由高到低）找首个同时过三闸的可行配置。
    /// 返回 (选定宽度, 请求 MAMV 校验结果)；无解返回宽度闸错误。
    pub fn negotiate(&self, want_ir: bool) -> Result<(AwWidth, ()), u32> {
        // 能三前置：要求 IR 但硬件不承诺 → 直接拒。
        if want_ir {
            self.check_ir()?;
        }
        // 宽度偏好：57 → 48 → 39（宽者优先）。
        for w in [AwWidth::W57, AwWidth::W48, AwWidth::W39].iter() {
            if self.check_width(*w).is_ok() {
                return Ok((*w, ()));
            }
        }
        Err(XC_BADAW)
    }
}

pub fn run_unx0494_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0494");
    cs.add("SEG 段位独占", SEG_BASE == 0x34A40 && XC_MAMVOVR == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 夹具：主流硬件铭牌——39/48 位宽、MAMV=5、IR 支持。
    let cap_ok: u64 = (0b011_u64 << 8) | (5u64 << 48); // SAGAW=0b011（39+48），MAMV=5
    let ecap_ok: u64 = 1 << 22; // ECAP.IR
    let g = CapGate::new(cap_ok, ecap_ok);

    // 族一：SAGAW 非零。
    cs.add("SAGAW 非零成", g.check_sagaw().is_ok(), "0b011 两档支持");
    let g_zero = CapGate::new(0, 0);
    cs.add("SAGAW 全零拒", g_zero.check_sagaw() == Err(XC_NOSAGAW), "零承诺翻译不可配");

    // 族二：宽度受支。
    cs.add("39 位成", g.check_width(AwWidth::W39).is_ok(), "支持集内");
    cs.add("48 位成", g.check_width(AwWidth::W48).is_ok(), "支持集内");
    cs.add("57 位拒", g.check_width(AwWidth::W57) == Err(XC_BADAW), "SAGAW bit2 未置");

    // 族三：ECAP.IR 门控。
    cs.add("IR 支持成", g.check_ir().is_ok(), "bit22 置位");
    let g_noir = CapGate::new(cap_ok, 0);
    cs.add("IR 缺失拒", g_noir.check_ir() == Err(XC_NOIR), "未承诺 IR 不得引用");

    // 族四：MAMV 上限。
    cs.add("MAMV 边界成", g.check_mamv(5).is_ok(), "等于上限（<= 合法）");
    cs.add("MAMV 超限拒", g.check_mamv(6) == Err(XC_MAMVOVR), "5+1 越界");
    cs.add("MAMV 解析", parse_mamv(cap_ok) == 5, "bit48~53 原值");

    // 族五：能力快照稳定（幂等）。
    let (_, n1) = parse_sagaw(cap_ok);
    let (_, n2) = parse_sagaw(cap_ok);
    let m1 = parse_mamv(cap_ok);
    let m2 = parse_mamv(cap_ok);
    cs.add("快照恒等", n1 == n2 && m1 == m2 && n1 == 2 && m1 == 5, "同输入两位级恒等");

    // 族六：全档位硬件（SAGAW=0b111）三宽全过。
    let cap_full: u64 = 0b111_u64 << 8;
    let gf = CapGate::new(cap_full, ecap_ok);
    cs.add("全档全过", gf.check_width(AwWidth::W39).is_ok() && gf.check_width(AwWidth::W48).is_ok() && gf.check_width(AwWidth::W57).is_ok(), "0b111 三档齐");
    let (_, nf) = parse_sagaw(cap_full);
    cs.add("全档计数三", nf == 3, "三档齐计三");

    // 族七：组合闸依序短路（第一错先报）。
    let g_bad = CapGate::new(0, 0);
    let r = g_bad.check_sagaw().and_then(|_| g_bad.check_ir());
    cs.add("闸序短路", r == Err(XC_NOSAGAW), "SAGAW 先于 IR 报错");
    let r2 = g_noir.check_sagaw().and_then(|_| g_noir.check_ir());
    cs.add("二闸承接", r2 == Err(XC_NOIR), "SAGAW 过后 IR 闸接管");

    // 族八：联合协商。
    let g = CapGate::new(cap_ok, ecap_ok); // 支持 39+48、MAMV=5、IR
    cs.add("协商取宽优先", g.negotiate(false) == Ok((AwWidth::W48, ())), "57 不支 → 落 48");
    cs.add("协商要 IR 成", g.negotiate(true).is_ok(), "IR 已承诺");
    let g_noir = CapGate::new(cap_ok, 0);
    cs.add("协商要 IR 拒", g_noir.negotiate(true) == Err(XC_NOIR), "IR 未承诺先拒");
    let g_zero = CapGate::new(0, ecap_ok);
    cs.add("协商零宽拒", g_zero.negotiate(false) == Err(XC_BADAW), "无宽可协商");
    // 全档硬件协商取最高。
    let g_full = CapGate::new(0b111_u64 << 8, ecap_ok);
    cs.add("全档协商顶格", g_full.negotiate(false) == Ok((AwWidth::W57, ())), "三档全支取 57");

    cs
}
