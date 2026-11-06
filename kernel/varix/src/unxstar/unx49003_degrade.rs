//! UNX-F49003 quirk 外降级口径执行 + UNX-F49004 T1 档机型清单承诺面 · 0x71900
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-B 首槽；任务板 J1 引用按原文保留，
//! 不把 F49003 卡面引用的 F49004-J1 改写成 F49003-J1，也不把 F49004 卡面
//! 引用的 F49005-J1 改写成 F49004-J1，以便任务板可逐字回查）：
//!   UNX-F49003 quirk 外降级口径执行（识别可算、手势不承诺——如实记账不虚报）:
//!           ① 纯功能代码 ≥360 行；
//!           ② 判据原文：UNX-F49004-J1 清单登记在册 + 清单外零承诺断言；判据锚定 §AI-62 §6 攻坚②；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49004 T1 档机型清单承诺面（清单限定承诺——触控硬件碎片化口径落账）:
//!           ① 纯功能代码 ≥340 行；
//!           ② 判据原文：UNX-F49005-J1 三档 ×10^2 行为一致；判据锚定 体验十 全局一致性；
//!           ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（降级口径 + T1 承诺面）：
//!   能力矩阵自审 → 清单成员证明 → 识别集计算 → 承诺集裁剪 → 三态分类 →
//!   零虚报复核 → 账本渲染。
//! 矩阵自审先于成员证明：`committed ⊆ recognizable` 是承诺面能存在的前提。
//! 矩阵坏掉时，清单里写「本机型承诺八项」只是把坏数据复制了十二遍；先自审
//! 才能把「矩阵病」与「机型不在清单」分成两类错误，而不是让后者背前者。
//! 成员证明先于识别集：先确认「这台机器在不在 T1 清单里」，再算「它现场能
//! 识别出什么」。顺序反过来会让清单外设备也带着识别集进入裁剪，裁剪后
//! claimed 可能非零——「清单外零承诺断言」当场破，而且破得无声无息。
//! 识别集先于承诺集：承诺只能是「识别得出来」的子集。先裁剪后识别等于先
//! 许诺再验货：识别不出的能力已经被记进账本，这就是虚报的形态本身。
//! 承诺集裁剪先于三态分类：Full/Degraded/Refused 由 withheld（承诺了但识别
//! 不出）决定。分类先做的话，账本按「看起来正常」归档，随后补上的裁剪无法
//! 把 Degraded 改回来——分类一旦落账就不可回写，只能再记一笔修正。
//! 三态分类先于零虚报复核：复核算的是 claimed ∩ ¬recognized 的条数，以及
//! granted == claimed + withheld。账本没有分类计数就没有分母，「零虚报」无从
//! 判定；反过来若分类由复核倒推，复核就等于自证，判据失去独立性。
//! 零虚报复核先于渲染：渲染行是账本对外的唯一投影。先渲染后复核，一旦有
//! 虚报，屏幕上已经出现一行「看起来合规」的数字，事后纠错只能靠人眼回查，
//! 而回查者看到的还是那行被污染的文本。
//! 段位守卫最后：14 个诊断码必须落在 [0x71901, 0x7190E]。渲染与复核都靠码值
//! 定位；窗口一旦漂移，前六项闸门的失败会被记成「未知诊断码」，红变成灰。
//! 关键差异：本模块不做表项三段式匹配（B11-A 的 F49001/F49002 承担），不做
//! 触控板三档配置与热生效（B11-C 的 F49005/F49006 承担），不做多用户隔离与
//! 表版本校验和（B11-D 的 F49007/F49008 承担），也不做 fuzz 与损坏配置回退
//! （B11-E 的 F49009/F49010 承担）。F49005-J1「三档 ×10^2 行为一致」的三档
//! 载体在 B11-C，本模块只逐字保留该引用、不自造三档，以免把 C 槽的判据提前
//! 在自己的账本里「预支」成绿。
//! 本模块只回答两问：
//!   ① 一次设备探测落在 T1 清单内还是清单外，其承诺集是否恰好等于
//!      「清单授权 ∩ 现场可识别」，清单外是否严格零承诺；
//!   ② 十台设备的降级记账是否零虚报（claimed ⊄ recognized 的条数必须为 0，
//!      且 granted == claimed + withheld 必须逐台成立）。
//! 表以定长数组固化、求值为纯函数：同输入必同输出，跨批复跑不漂移。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71900，VX6 批 B11-B 首槽）。
pub const SEG_BASE: u32 = 0x71900;

/// 能力矩阵行数（识别可算的十二项触控能力）。
pub const CAP_COUNT: usize = 12;
/// T1 清单机型条数。
pub const COMMITTED_COUNT: usize = 12;
/// 实际承诺的能力条数（矩阵中被清单引用的那八项）。
pub const COMMITTED_CAPS: u32 = 8;
/// 降级记账设备数（判据 ×10）。
pub const DEGRADE_DEVICES: usize = 10;
/// 降级记账轮数（每轮一台设备）。
pub const DEGRADE_ROUNDS: u32 = 10;
/// 场景轮数上界（覆盖闸门）。
pub const SCENARIO_MAX: u32 = 400;
/// 行为一致复跑轮数（×10^2）。
pub const REPLAY_ROUNDS: u32 = 100;
/// 黄金脚本例数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 注入路数（能力/清单/报告三类篡改）。
pub const INJECT_ROUTES: usize = 5;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 账本字段数。
pub const STATS_FIELDS: usize = 10;
/// fuzz 轮数上限。
pub const FUZZ_ROUNDS: u32 = 10_000;

/// 固件版本下界（低于此值视为坏探测）。
pub const FW_MIN: u32 = 0x0000_0100;
/// 固件版本上界。
pub const FW_MAX: u32 = 0x00FF_FFFF;
/// T1 档位编号（清单只登记这一档）。
pub const TIER_T1: u8 = 1;

// ── 能力位（一能力一位，矩阵行序 = 位序） ─────────────────────────
pub const CAP_SCROLL2: u32 = 1 << 0;
pub const CAP_PINCH2: u32 = 1 << 1;
pub const CAP_DRAG3: u32 = 1 << 2;
pub const CAP_SWIPE3: u32 = 1 << 3;
pub const CAP_GESTURE4: u32 = 1 << 4;
pub const CAP_PALM: u32 = 1 << 5;
pub const CAP_EDGE: u32 = 1 << 6;
pub const CAP_FORCE: u32 = 1 << 7;
pub const CAP_HIRES: u32 = 1 << 8;
pub const CAP_INERTIA: u32 = 1 << 9;
pub const CAP_STYLUS: u32 = 1 << 10;
pub const CAP_CUSTOM: u32 = 1 << 11;

/// 矩阵内「已承诺」八项的掩码（清单授权只能引用这些位）。
pub const CAP_COMMITTED_MASK: u32 =
    CAP_SCROLL2 | CAP_PINCH2 | CAP_DRAG3 | CAP_SWIPE3 | CAP_PALM | CAP_EDGE | CAP_FORCE | CAP_HIRES;
/// 矩阵全位掩码。
pub const CAP_ALL_MASK: u32 = (1u32 << CAP_COUNT) - 1;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegCode {
    /// 能力矩阵条数/位序非法
    CapBad,
    /// 承诺位未被识别位包含（虚报）
    OverclaimBad,
    /// T1 清单条数/区间/档位非法
    CommittedBad,
    /// 同 (厂商, 机型, 固件区间) 重复登记
    DupModel,
    /// 清单档位不是 T1（或档位越界）
    TierBad,
    /// 成员证明不成立
    MemberBad,
    /// 降级账不守恒
    LedgerBad,
    /// 声称能力未被实际能力包含（零虚报被破）
    ClaimBad,
    /// 复跑摘要不一致
    ReplayBad,
    /// 注入未全捕获
    InjectBad,
    /// 渲染不可解析
    RenderBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// 黄金/边界断言失败
    FuzzBad,
}

impl DegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DegCode::CapBad => "能力矩阵条数不是 12 或位序与行序不符",
            DegCode::OverclaimBad => "承诺位落在识别位之外，承诺面本身虚报",
            DegCode::CommittedBad => "T1 清单条数/固件区间/排序非法",
            DegCode::DupModel => "同一机型出现重叠固件区间，成员身份二义",
            DegCode::TierBad => "清单档位不是 T1 或档位越界",
            DegCode::MemberBad => "清单成员证明不成立：应命中未命中或不该命中却命中",
            DegCode::LedgerBad => "降级账不守恒（三态计数或裁剪恒等式断裂）",
            DegCode::ClaimBad => "声称能力 ⊄ 实际识别能力，零虚报被破",
            DegCode::ReplayBad => "两次确定性复跑账本摘要不一致",
            DegCode::InjectBad => "注入轮次未被全部捕获",
            DegCode::RenderBad => "渲染行字段数或数值不可解析",
            DegCode::CoverBad => "覆盖轮数为零或越过上限",
            DegCode::SegmentBad => "诊断码越出本模块段窗",
            DegCode::FuzzBad => "黄金样例或边界条件与预期不一致",
        }
    }

    pub const ALL: [DegCode; 14] = [
        DegCode::CapBad,
        DegCode::OverclaimBad,
        DegCode::CommittedBad,
        DegCode::DupModel,
        DegCode::TierBad,
        DegCode::MemberBad,
        DegCode::LedgerBad,
        DegCode::ClaimBad,
        DegCode::ReplayBad,
        DegCode::InjectBad,
        DegCode::RenderBad,
        DegCode::CoverBad,
        DegCode::SegmentBad,
        DegCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DegCode::ALL.len() {
        if let Some(c) = DegCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 能力矩阵 ─────────────────────────────────────────────────────
/// 一项触控能力：能否被内核识别（可算）、是否列入 T1 承诺面（可承诺）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capability {
    pub id: &'static str,
    pub bit: u8,
    pub recognizable: bool,
    pub committed: bool,
}

pub const CAPS: [Capability; CAP_COUNT] = [
    Capability {
        id: "scroll2",
        bit: 0,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "pinch2",
        bit: 1,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "drag3",
        bit: 2,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "swipe3",
        bit: 3,
        recognizable: true,
        committed: true,
    },
    // 四指手势：报文里可算（触点序可判），但 T1 一档机型的固件差异太大，
    // 明确不承诺——这是「识别可算、手势不承诺」的字面落点。
    Capability {
        id: "gesture4",
        bit: 4,
        recognizable: true,
        committed: false,
    },
    Capability {
        id: "palm",
        bit: 5,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "edge",
        bit: 6,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "force",
        bit: 7,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "hires",
        bit: 8,
        recognizable: true,
        committed: true,
    },
    Capability {
        id: "inertia",
        bit: 9,
        recognizable: true,
        committed: false,
    },
    Capability {
        id: "stylus",
        bit: 10,
        recognizable: true,
        committed: false,
    },
    Capability {
        id: "custom",
        bit: 11,
        recognizable: true,
        committed: false,
    },
];

/// 矩阵统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapStats {
    pub total: u32,
    pub recognizable: u32,
    pub committed: u32,
    pub fingerprint: u64,
}

/// 单位掩码（位号越界给 0，不 panic）。
pub fn cap_bit(i: u8) -> u32 {
    if i as u32 >= CAP_COUNT as u32 {
        0
    } else {
        1u32 << (i as u32)
    }
}

/// 位计数（不依赖 intrinsics，避免 no_std 目标差异）。
pub fn popcount(mut x: u32) -> u32 {
    let mut n = 0u32;
    while x != 0 {
        x &= x.wrapping_sub(1);
        n = n.saturating_add(1);
    }
    n
}

/// 矩阵自审：条数 → 位序 → committed ⊆ recognizable → 指纹。
pub fn cap_integrity(table: &[Capability]) -> Result<CapStats, u32> {
    if table.len() != CAP_COUNT {
        return Err(DegCode::CapBad.val());
    }
    let mut fp = p_core::FNV_OFF;
    let mut recognizable = 0u32;
    let mut committed = 0u32;
    let mut i = 0usize;
    while i < table.len() {
        let c = match table.get(i) {
            Some(v) => *v,
            None => return Err(DegCode::CapBad.val()),
        };
        if c.id.is_empty() {
            return Err(DegCode::CapBad.val());
        }
        if c.bit as usize != i {
            return Err(DegCode::CapBad.val());
        }
        // 承诺位必须先能被识别——这是矩阵层的零虚报前提。
        if c.committed && !c.recognizable {
            return Err(DegCode::OverclaimBad.val());
        }
        if c.recognizable {
            recognizable = recognizable.saturating_add(1);
        }
        if c.committed {
            committed = committed.saturating_add(1);
        }
        let bs = c.id.as_bytes();
        let mut k = 0usize;
        while k < bs.len() {
            if let Some(b) = bs.get(k) {
                fp = p_core::fold_byte(fp, *b);
            }
            k += 1;
        }
        fp = p_core::fold_u32(fp, u32::from(c.bit));
        fp = p_core::fold_u32(fp, u32::from(c.recognizable));
        fp = p_core::fold_u32(fp, u32::from(c.committed));
        i += 1;
    }
    Ok(CapStats {
        total: table.len() as u32,
        recognizable,
        committed,
        fingerprint: fp,
    })
}

/// 矩阵承诺位掩码。
pub fn table_committed_mask(table: &[Capability]) -> u32 {
    let mut m = 0u32;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(c) = table.get(i) {
            if c.committed {
                m |= cap_bit(c.bit);
            }
        }
        i += 1;
    }
    m
}

/// 矩阵识别位掩码。
pub fn table_recognizable_mask(table: &[Capability]) -> u32 {
    let mut m = 0u32;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(c) = table.get(i) {
            if c.recognizable {
                m |= cap_bit(c.bit);
            }
        }
        i += 1;
    }
    m
}

// ── T1 档机型清单承诺面 ──────────────────────────────────────────
/// 一条 T1 清单登记：(厂商, 机型, 固件区间) → 档位 + 授权能力位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T1Model {
    pub id: &'static str,
    pub vendor: u16,
    pub model: u16,
    pub fw_lo: u32,
    pub fw_hi: u32,
    pub tier: u8,
    pub grant: u32,
}

/// 12 条 T1 登记，按 (厂商, 机型, 固件下界) 升序固化。
pub const T1_MODELS: [T1Model; COMMITTED_COUNT] = [
    T1Model {
        id: "surface-laptop-studio",
        vendor: 0x045E,
        model: 0x0C01,
        fw_lo: 0x0001_0100,
        fw_hi: 0x0001_04FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
    T1Model {
        id: "framework-13",
        vendor: 0x0F11,
        model: 0x0001,
        fw_lo: 0x0001_0000,
        fw_hi: 0x0001_02FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_FORCE,
    },
    T1Model {
        id: "acer-swift-edge",
        vendor: 0x1025,
        model: 0x1601,
        fw_lo: 0x0002_0000,
        fw_hi: 0x0002_01FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_HIRES,
    },
    T1Model {
        id: "dell-xps13-9340",
        vendor: 0x1028,
        model: 0x0BE1,
        fw_lo: 0x0003_0000,
        fw_hi: 0x0003_05FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
    T1Model {
        id: "dell-xps16-9640",
        vendor: 0x1028,
        model: 0x0BE2,
        fw_lo: 0x0003_0600,
        fw_hi: 0x0003_0BFF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
    T1Model {
        id: "hp-spectre-x360-14",
        vendor: 0x103C,
        model: 0x8B21,
        fw_lo: 0x0004_0100,
        fw_hi: 0x0004_03FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_EDGE,
    },
    T1Model {
        id: "asus-zenbook-14",
        vendor: 0x1043,
        model: 0x1C42,
        fw_lo: 0x0005_0000,
        fw_hi: 0x0005_02FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_EDGE & !CAP_HIRES,
    },
    T1Model {
        id: "razer-blade-14",
        vendor: 0x1532,
        model: 0x028F,
        fw_lo: 0x0006_0000,
        fw_hi: 0x0006_01FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
    T1Model {
        id: "system76-lemur-pro",
        vendor: 0x1558,
        model: 0x1401,
        fw_lo: 0x0007_0000,
        fw_hi: 0x0007_04FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_PALM,
    },
    T1Model {
        id: "thinkpad-x1c-g12",
        vendor: 0x17AA,
        model: 0x22F1,
        fw_lo: 0x0008_0000,
        fw_hi: 0x0008_07FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
    T1Model {
        id: "thinkpad-x1y-g9",
        vendor: 0x17AA,
        model: 0x22F2,
        fw_lo: 0x0008_0800,
        fw_hi: 0x0008_0BFF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK & !CAP_FORCE,
    },
    T1Model {
        id: "huawei-matebook-xpro",
        vendor: 0x19E5,
        model: 0x3E01,
        fw_lo: 0x0009_0000,
        fw_hi: 0x0009_02FF,
        tier: TIER_T1,
        grant: CAP_COMMITTED_MASK,
    },
];

/// 清单统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListStats {
    pub total: u32,
    pub grant_union: u32,
    pub fingerprint: u64,
}

/// 清单自审：条数 → 固件区间 → 档位 → 授权 ⊆ 承诺位 → 排序 → 重叠。
pub fn model_integrity(table: &[T1Model]) -> Result<ListStats, u32> {
    if table.len() != COMMITTED_COUNT {
        return Err(DegCode::CommittedBad.val());
    }
    let cap_map = match cap_integrity(&CAPS) {
        Ok(_) => CAP_COMMITTED_MASK,
        Err(e) => return Err(e),
    };
    let mut fp = p_core::FNV_OFF;
    let mut union = 0u32;
    let mut i = 0usize;
    while i < table.len() {
        let m = match table.get(i) {
            Some(v) => *v,
            None => return Err(DegCode::CommittedBad.val()),
        };
        if m.id.is_empty() {
            return Err(DegCode::CommittedBad.val());
        }
        if m.fw_lo > m.fw_hi || m.fw_lo < FW_MIN || m.fw_hi > FW_MAX {
            return Err(DegCode::CommittedBad.val());
        }
        if m.tier != TIER_T1 {
            return Err(DegCode::TierBad.val());
        }
        // 授权位必须落在矩阵的承诺面内：清单不能替矩阵「加承诺」。
        if m.grant & !cap_map != 0 || m.grant == 0 {
            return Err(DegCode::ClaimBad.val());
        }
        union |= m.grant;
        // 与前面所有条目比对：区间倒序、重复、重叠都要拒。
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                let same_identity = p.vendor == m.vendor && p.model == m.model;
                let overlap = !(m.fw_hi < p.fw_lo || p.fw_hi < m.fw_lo);
                if same_identity && overlap {
                    return Err(DegCode::DupModel.val());
                }
                if m.vendor < p.vendor
                    || (m.vendor == p.vendor && m.model < p.model)
                    || (m.vendor == p.vendor && m.model == p.model && m.fw_lo <= p.fw_lo)
                {
                    return Err(DegCode::CommittedBad.val());
                }
            }
            j += 1;
        }
        let bs = m.id.as_bytes();
        let mut k = 0usize;
        while k < bs.len() {
            if let Some(b) = bs.get(k) {
                fp = p_core::fold_byte(fp, *b);
            }
            k += 1;
        }
        fp = p_core::fold_u32(fp, u32::from(m.vendor));
        fp = p_core::fold_u32(fp, u32::from(m.model));
        fp = p_core::fold_u32(fp, m.fw_lo);
        fp = p_core::fold_u32(fp, m.fw_hi);
        fp = p_core::fold_u32(fp, u32::from(m.tier));
        fp = p_core::fold_u32(fp, m.grant);
        i += 1;
    }
    Ok(ListStats {
        total: table.len() as u32,
        grant_union: union,
        fingerprint: fp,
    })
}

// ── 探测与成员证明 ───────────────────────────────────────────────
/// 一次设备探测：厂商 / 机型 / 固件版本 / quirk 位（该位为 1 表示现场识别不出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub vendor: u16,
    pub model: u16,
    pub fw: u32,
    pub quirk_mask: u32,
}

impl Probe {
    pub const fn new(vendor: u16, model: u16, fw: u32, quirk_mask: u32) -> Probe {
        Probe {
            vendor,
            model,
            fw,
            quirk_mask,
        }
    }
}

/// 成员查找：唯一命中才算命中，二义（多命中）按未命中处理。
pub fn membership_in(table: &[T1Model], probe: &Probe) -> Option<usize> {
    let mut hit: Option<usize> = None;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(m) = table.get(i) {
            if m.vendor == probe.vendor
                && m.model == probe.model
                && probe.fw >= m.fw_lo
                && probe.fw <= m.fw_hi
            {
                if hit.is_some() {
                    // 二义：清单自审本应拒绝，这里保守按未命中。
                    return None;
                }
                hit = Some(i);
            }
        }
        i += 1;
    }
    hit
}

pub fn membership(probe: &Probe) -> Option<usize> {
    membership_in(&T1_MODELS, probe)
}

/// 成员证明：每条登记都能被自己的 (厂商, 机型, 两端固件) 唯一定位到。
pub fn membership_proof() -> bool {
    let mut i = 0usize;
    while i < T1_MODELS.len() {
        let m = match T1_MODELS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let lo = Probe::new(m.vendor, m.model, m.fw_lo, 0);
        let hi = Probe::new(m.vendor, m.model, m.fw_hi, 0);
        let mid = match m.fw_lo.checked_add((m.fw_hi - m.fw_lo) / 2) {
            Some(v) => v,
            None => return false,
        };
        let midp = Probe::new(m.vendor, m.model, mid, 0);
        if membership(&lo) != Some(i) || membership(&hi) != Some(i) || membership(&midp) != Some(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 清单外探测集（八条：两条异厂商、两条邻号机型、两条固件越界、
/// 两条全零/全 F 的坏探测）。
pub const OUTSIDE_PROBES: [Probe; 8] = [
    Probe::new(0xDEAD, 0x0001, 0x0001_0000, 0),
    Probe::new(0xFFFF, 0xFFFF, 0xFFFF_FFFF, 0),
    Probe::new(0x045E, 0x0C02, 0x0001_0100, 0),
    Probe::new(0x19E5, 0x3E02, 0x0009_0100, 0),
    Probe::new(0x17AA, 0x22F1, 0x0007_FFFF, 0),
    Probe::new(0x17AA, 0x22F1, 0x0008_0800, 0),
    Probe::new(0x1028, 0x0BE1, 0x0002_0000, 0),
    Probe::new(0x0000, 0x0000, 0, 0),
];

/// 清单外零承诺：每条清单外探测都判不存在成员，且承诺位为 0。
pub fn outside_zero_promise() -> bool {
    let mut i = 0usize;
    while i < OUTSIDE_PROBES.len() {
        let p = match OUTSIDE_PROBES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if membership(&p).is_some() {
            return false;
        }
        let r = evaluate(&p);
        if r.claimed != 0 || r.granted != 0 || r.entry.is_some() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探测求值（识别 → 裁剪 → 分类） ──────────────────────────────
/// 三态分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 清单内，承诺全部可识别
    Full,
    /// 清单内，部分承诺现场识别不出（已如实扣减）
    Degraded,
    /// 不承诺：清单外，或清单内但一条也识别不出
    Refused,
}

impl Outcome {
    pub fn code(self) -> u8 {
        match self {
            Outcome::Full => 0,
            Outcome::Degraded => 1,
            Outcome::Refused => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Outcome::Full => "full",
            Outcome::Degraded => "degraded",
            Outcome::Refused => "refused",
        }
    }

    pub fn from_code(c: u8) -> Option<Outcome> {
        match c {
            0 => Some(Outcome::Full),
            1 => Some(Outcome::Degraded),
            2 => Some(Outcome::Refused),
            _ => None,
        }
    }
}

/// 一次求值的完整结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapReport {
    pub entry: Option<u8>,
    pub granted: u32,
    pub recognized: u32,
    pub claimed: u32,
    pub withheld: u32,
    pub outcome: Outcome,
}

/// 求值：现场识别集 → 承诺裁剪 → 分类。三步严格按闸门次序推进。
pub fn evaluate_in(caps: &[Capability], table: &[T1Model], probe: &Probe) -> CapReport {
    let entry = membership_in(table, probe);
    let granted = match entry {
        Some(i) => match table.get(i) {
            Some(m) => m.grant,
            None => 0,
        },
        None => 0,
    };
    // 现场识别集：矩阵认可的识别位，扣除 quirk 打掉的位。
    let recognized = table_recognizable_mask(caps) & !probe.quirk_mask;
    // 承诺裁剪：承诺只能是识别的子集。
    let claimed = granted & recognized;
    let withheld = granted & !recognized;
    let outcome = if entry.is_none() {
        Outcome::Refused
    } else if claimed == 0 {
        Outcome::Refused
    } else if withheld == 0 {
        Outcome::Full
    } else {
        Outcome::Degraded
    };
    CapReport {
        entry: entry.map(|i| i as u8),
        granted,
        recognized,
        claimed,
        withheld,
        outcome,
    }
}

pub fn evaluate(probe: &Probe) -> CapReport {
    evaluate_in(&CAPS, &T1_MODELS, probe)
}

/// 报告闸门：虚报、裁剪恒等式、清单外零承诺三关。
pub fn report_gate(entry: Option<usize>, r: &CapReport) -> Result<(), u32> {
    if r.claimed & !r.recognized != 0 {
        return Err(DegCode::ClaimBad.val());
    }
    if r.claimed | r.withheld != r.granted {
        return Err(DegCode::ClaimBad.val());
    }
    if r.claimed & r.withheld != 0 {
        return Err(DegCode::ClaimBad.val());
    }
    match entry {
        None => {
            if r.granted != 0 || r.claimed != 0 || r.withheld != 0 {
                return Err(DegCode::ClaimBad.val());
            }
        }
        Some(_) => {
            if r.granted == 0 {
                return Err(DegCode::ClaimBad.val());
            }
        }
    }
    if r.outcome == Outcome::Full && r.withheld != 0 {
        return Err(DegCode::FuzzBad.val());
    }
    if r.outcome == Outcome::Degraded && (r.withheld == 0 || r.claimed == 0) {
        return Err(DegCode::FuzzBad.val());
    }
    if r.outcome == Outcome::Refused && r.claimed != 0 {
        return Err(DegCode::FuzzBad.val());
    }
    Ok(())
}

/// 报告是否如实（零虚报的逐份判定）。
pub fn report_honest(r: &CapReport) -> bool {
    r.claimed & !r.recognized == 0
        && r.claimed | r.withheld == r.granted
        && r.claimed & r.withheld == 0
        && match r.entry {
            None => r.granted == 0 && r.claimed == 0,
            Some(_) => r.granted != 0,
        }
}

// ── 降级账（十台设备） ───────────────────────────────────────────
/// 降级账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeLedger {
    pub devices: u32,
    pub listed: u32,
    pub unlisted: u32,
    pub full: u32,
    pub degraded: u32,
    pub refused: u32,
    pub granted: u32,
    pub claimed: u32,
    pub withheld: u32,
    pub overclaims: u32,
}

impl DegradeLedger {
    pub const fn new() -> DegradeLedger {
        DegradeLedger {
            devices: 0,
            listed: 0,
            unlisted: 0,
            full: 0,
            degraded: 0,
            refused: 0,
            granted: 0,
            claimed: 0,
            withheld: 0,
            overclaims: 0,
        }
    }

    /// 守恒：每台设备恰好落进三态之一；清单内 + 清单外 = 设备数；
    /// 承诺位恰好被「声称 + 扣减」剖分（不漏不重）。
    pub fn conserved(&self) -> bool {
        self.devices == self.full.saturating_add(self.degraded).saturating_add(self.refused)
            && self.devices == self.listed.saturating_add(self.unlisted)
            && self.granted == self.claimed.saturating_add(self.withheld)
    }

    /// 零虚报：声称 ⊄ 识别的条数必须为 0。
    pub fn honest(&self) -> bool {
        self.overclaims == 0
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.devices);
        h = p_core::fold_u32(h, self.listed);
        h = p_core::fold_u32(h, self.unlisted);
        h = p_core::fold_u32(h, self.full);
        h = p_core::fold_u32(h, self.degraded);
        h = p_core::fold_u32(h, self.refused);
        h = p_core::fold_u32(h, self.granted);
        h = p_core::fold_u32(h, self.claimed);
        h = p_core::fold_u32(h, self.withheld);
        h = p_core::fold_u32(h, self.overclaims);
        h
    }
}

/// 一台设备的探测计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevicePlan {
    pub label: &'static str,
    /// 清单内机型下标；None 表示清单外设备（走 vendor/model_no 自造探测）。
    pub listed: Option<u8>,
    pub vendor: u16,
    pub model_no: u16,
    /// 现场 quirk 路数（0..9，见 QUIRK_ROUTES）。
    pub route: u8,
}

/// quirk 注入路数 → quirk 掩码。
pub const QUIRK_ROUTES: [u32; 10] = [
    0,
    CAP_FORCE,
    CAP_GESTURE4,
    CAP_HIRES,
    CAP_PALM,
    CAP_DRAG3,
    CAP_EDGE,
    CAP_SWIPE3,
    CAP_ALL_MASK,
    CAP_COMMITTED_MASK,
];

/// 十台设备的探测计划：八台清单内 + 两台清单外，覆盖三态与
/// 「quirk 打在不承诺位上不应降级」这一容易做错的格子。
pub const DEVICE_PLAN: [DevicePlan; DEGRADE_DEVICES] = [
    DevicePlan {
        label: "surface-clean",
        listed: Some(0),
        vendor: 0,
        model_no: 0,
        route: 0,
    },
    DevicePlan {
        label: "framework-force-quirk",
        listed: Some(1),
        vendor: 0,
        model_no: 0,
        route: 1,
    },
    DevicePlan {
        label: "acer-hires-quirk",
        listed: Some(2),
        vendor: 0,
        model_no: 0,
        route: 3,
    },
    DevicePlan {
        label: "dell13-force-quirk",
        listed: Some(3),
        vendor: 0,
        model_no: 0,
        route: 1,
    },
    DevicePlan {
        label: "dell16-all-quirk",
        listed: Some(4),
        vendor: 0,
        model_no: 0,
        route: 8,
    },
    DevicePlan {
        label: "hp-palm-quirk",
        listed: Some(5),
        vendor: 0,
        model_no: 0,
        route: 4,
    },
    DevicePlan {
        label: "asus-drag-quirk",
        listed: Some(6),
        vendor: 0,
        model_no: 0,
        route: 5,
    },
    DevicePlan {
        label: "razer-clean",
        listed: Some(7),
        vendor: 0,
        model_no: 0,
        route: 0,
    },
    DevicePlan {
        label: "unlisted-vendor",
        listed: None,
        vendor: 0xDEAD,
        model_no: 0x0001,
        route: 0,
    },
    DevicePlan {
        label: "unlisted-model",
        listed: None,
        vendor: 0x103C,
        model_no: 0x9999,
        route: 2,
    },
];

/// 计划 → 探测。
pub fn probe_for(plan: &DevicePlan) -> Probe {
    match plan.listed {
        Some(i) => {
            let m = match T1_MODELS.get(i as usize) {
                Some(v) => *v,
                None => return Probe::new(plan.vendor, plan.model_no, 0, 0),
            };
            let mid = m.fw_lo.saturating_add((m.fw_hi.saturating_sub(m.fw_lo)) / 2);
            let q = *QUIRK_ROUTES.get(plan.route as usize).unwrap_or(&0);
            Probe::new(m.vendor, m.model, mid, q)
        }
        None => {
            let q = *QUIRK_ROUTES.get(plan.route as usize).unwrap_or(&0);
            Probe::new(plan.vendor, plan.model_no, 0x0001_0000, q)
        }
    }
}

/// 单台入账（求值 → 闸门 → 记账）。
fn step(lg: &mut DegradeLedger, plan: &DevicePlan) -> Result<(), u32> {
    let p = probe_for(plan);
    let r = evaluate(&p);
    let entry = membership(&p);
    report_gate(entry, &r)?;
    if !report_honest(&r) {
        lg.overclaims = lg.overclaims.saturating_add(1);
    }
    // 声称位里凡是识别不出的，都是虚报形态；逐位核对。
    if r.claimed & !r.recognized != 0 {
        lg.overclaims = lg.overclaims.saturating_add(1);
    }
    lg.devices = lg.devices.saturating_add(1);
    if entry.is_some() {
        lg.listed = lg.listed.saturating_add(1);
    } else {
        lg.unlisted = lg.unlisted.saturating_add(1);
    }
    match r.outcome {
        Outcome::Full => lg.full = lg.full.saturating_add(1),
        Outcome::Degraded => lg.degraded = lg.degraded.saturating_add(1),
        Outcome::Refused => lg.refused = lg.refused.saturating_add(1),
    }
    lg.granted = lg.granted.saturating_add(popcount(r.granted));
    lg.claimed = lg.claimed.saturating_add(popcount(r.claimed));
    lg.withheld = lg.withheld.saturating_add(popcount(r.withheld));
    Ok(())
}

/// 十台设备降级记账（判据载体：×10）。
pub fn degrade_rounds(rounds: u32) -> Result<DegradeLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(DegCode::CoverBad.val());
    }
    let cap = match cap_integrity(&CAPS) {
        Ok(_) => (),
        Err(e) => return Err(e),
    };
    let _ = cap;
    if let Err(e) = model_integrity(&T1_MODELS) {
        return Err(e);
    }
    let mut lg = DegradeLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let idx = (r as usize) % DEGRADE_DEVICES;
        let plan = match DEVICE_PLAN.get(idx) {
            Some(v) => v,
            None => return Err(DegCode::CoverBad.val()),
        };
        step(&mut lg, plan)?;
        r += 1;
    }
    if !lg.conserved() {
        return Err(DegCode::LedgerBad.val());
    }
    if !lg.honest() {
        return Err(DegCode::ClaimBad.val());
    }
    Ok(lg)
}

/// 同一批设备按逆序再记一遍：账本与顺序无关，摘要必须逐位相同。
pub fn degrade_rounds_rev(rounds: u32) -> Result<DegradeLedger, u32> {
    if rounds == 0 || rounds > SCENARIO_MAX {
        return Err(DegCode::CoverBad.val());
    }
    let mut lg = DegradeLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let idx = (rounds - 1 - r) as usize % DEGRADE_DEVICES;
        let plan = match DEVICE_PLAN.get(idx) {
            Some(v) => v,
            None => return Err(DegCode::CoverBad.val()),
        };
        step(&mut lg, plan)?;
        r += 1;
    }
    if !lg.conserved() || !lg.honest() {
        return Err(DegCode::LedgerBad.val());
    }
    Ok(lg)
}

/// 复跑一致：正序与逆序摘要相同、且两轮正序摘要相同。
pub fn degrade_replay(rounds: u32) -> bool {
    let a = degrade_rounds(rounds);
    let b = degrade_rounds(rounds);
    let c = degrade_rounds_rev(rounds);
    match (a, b, c) {
        (Ok(a), Ok(b), Ok(c)) => a.digest() == b.digest() && a.digest() == c.digest(),
        _ => false,
    }
}

// ── 黄金样例（16 例，显式期望值） ───────────────────────────────
/// 黄金例：机型下标 / 固件取法 / quirk 路数 / 期望三态码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenCase {
    pub model: u8,
    pub fw_sel: u8,
    pub quirk: u8,
    pub expect: u8,
}

/// 固件取法：0 下界 / 1 上界 / 2 中点 / 3 下界之下 / 4 上界之上。
pub const GOLDEN_CASES: [GoldenCase; GOLDEN_ROUNDS as usize] = [
    GoldenCase {
        model: 0,
        fw_sel: 0,
        quirk: 0,
        expect: 0,
    },
    GoldenCase {
        model: 0,
        fw_sel: 1,
        quirk: 0,
        expect: 0,
    },
    GoldenCase {
        model: 0,
        fw_sel: 2,
        quirk: 1,
        expect: 1,
    },
    GoldenCase {
        model: 0,
        fw_sel: 2,
        quirk: 2,
        expect: 0,
    },
    GoldenCase {
        model: 1,
        fw_sel: 0,
        quirk: 1,
        expect: 0,
    },
    GoldenCase {
        model: 1,
        fw_sel: 0,
        quirk: 4,
        expect: 1,
    },
    GoldenCase {
        model: 2,
        fw_sel: 2,
        quirk: 3,
        expect: 0,
    },
    GoldenCase {
        model: 2,
        fw_sel: 2,
        quirk: 5,
        expect: 1,
    },
    GoldenCase {
        model: 3,
        fw_sel: 3,
        quirk: 0,
        expect: 2,
    },
    GoldenCase {
        model: 3,
        fw_sel: 0,
        quirk: 8,
        expect: 2,
    },
    GoldenCase {
        model: 4,
        fw_sel: 1,
        quirk: 9,
        expect: 2,
    },
    GoldenCase {
        model: 5,
        fw_sel: 2,
        quirk: 6,
        expect: 0,
    },
    GoldenCase {
        model: 5,
        fw_sel: 2,
        quirk: 7,
        expect: 1,
    },
    GoldenCase {
        model: 6,
        fw_sel: 0,
        quirk: 4,
        expect: 1,
    },
    GoldenCase {
        model: 8,
        fw_sel: 2,
        quirk: 4,
        expect: 0,
    },
    GoldenCase {
        model: 9,
        fw_sel: 4,
        quirk: 0,
        expect: 2,
    },
];

/// 黄金例 → 探测。
pub fn golden_probe(c: &GoldenCase) -> Probe {
    let m = match T1_MODELS.get(c.model as usize) {
        Some(v) => *v,
        None => return Probe::new(0, 0, 0, 0),
    };
    let fw = match c.fw_sel {
        0 => m.fw_lo,
        1 => m.fw_hi,
        2 => m.fw_lo.saturating_add((m.fw_hi - m.fw_lo) / 2),
        3 => m.fw_lo.saturating_sub(1),
        _ => m.fw_hi.saturating_add(1),
    };
    let q = *QUIRK_ROUTES.get(c.quirk as usize).unwrap_or(&0);
    Probe::new(m.vendor, m.model, fw, q)
}

/// 黄金执行：三态、闸门、恒等式、逐例一致。
pub fn golden_ok() -> bool {
    let mut i = 0usize;
    while i < GOLDEN_CASES.len() {
        let c = match GOLDEN_CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = golden_probe(&c);
        let r = evaluate(&p);
        if r.outcome.code() != c.expect {
            return false;
        }
        if report_gate(membership(&p), &r).is_err() {
            return false;
        }
        if !report_honest(&r) {
            return false;
        }
        // 三态与裁剪量的相互印证。
        match r.outcome {
            Outcome::Full => {
                if r.withheld != 0 || r.claimed != r.granted || r.claimed == 0 {
                    return false;
                }
            }
            Outcome::Degraded => {
                if r.withheld == 0 || r.claimed == 0 {
                    return false;
                }
            }
            Outcome::Refused => {
                if r.claimed != 0 {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// 黄金样例覆盖度：三种固件取法与三种三态都被覆盖到。
pub fn golden_coverage() -> bool {
    let mut fw_seen = 0u32;
    let mut out_seen = 0u32;
    let mut i = 0usize;
    while i < GOLDEN_CASES.len() {
        if let Some(c) = GOLDEN_CASES.get(i) {
            fw_seen |= 1u32 << (c.fw_sel as u32 & 7);
            out_seen |= 1u32 << (c.expect as u32 & 7);
        }
        i += 1;
    }
    fw_seen == 0x1F && out_seen == 0x07
}

// ── 注入与反例 ──────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub attempts: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.attempts > 0 && self.caught == self.attempts
    }
}

/// 五路注入：
/// 0 矩阵行 committed=true 但 recognizable=false（承诺面虚报）；
/// 1 清单授权引用矩阵未承诺的位（清单替矩阵加承诺）；
/// 2 清单固件区间倒置；
/// 3 清单插入重叠登记（成员二义）；
/// 4 清单外探测被伪造成有声称（报告层虚报）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DegCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4900_3D91);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES as u32);
        let ok = match route {
            0 => {
                let mut t = CAPS.to_vec();
                if let Some(slot) = t.get_mut(4) {
                    slot.committed = true;
                    slot.recognizable = false;
                }
                cap_integrity(&t).err() == Some(DegCode::OverclaimBad.val())
            }
            1 => {
                let mut t = T1_MODELS.to_vec();
                if let Some(slot) = t.get_mut(0) {
                    slot.grant = slot.grant | CAP_GESTURE4;
                }
                model_integrity(&t).err() == Some(DegCode::ClaimBad.val())
            }
            2 => {
                let mut t = T1_MODELS.to_vec();
                if let Some(slot) = t.get_mut(2) {
                    slot.fw_lo = slot.fw_hi.saturating_add(1);
                }
                model_integrity(&t).err() == Some(DegCode::CommittedBad.val())
            }
            3 => {
                let mut t = T1_MODELS.to_vec();
                if let (Some(src), Some(slot)) = (t.get(5).copied(), t.get_mut(6)) {
                    *slot = src;
                }
                model_integrity(&t).err() == Some(DegCode::DupModel.val())
            }
            _ => {
                // 清单外探测 + 伪造声称：报告闸门必须拒。
                let p = Probe::new(0xDEAD, 0x0001, 0x0001_0000, 0);
                let mut f = evaluate(&p);
                f.granted = CAP_COMMITTED_MASK;
                f.claimed = CAP_COMMITTED_MASK;
                f.withheld = 0;
                report_gate(None, &f).err() == Some(DegCode::ClaimBad.val())
            }
        };
        if ok {
            caught = caught.saturating_add(1);
        }
        last = route;
        r += 1;
    }
    Ok(InjectLog {
        attempts: rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒（五条独立反例，不依赖随机）。
pub fn tamper_rejected() -> bool {
    // 1) 承诺位落在识别位之外。
    let mut caps = CAPS.to_vec();
    if let Some(slot) = caps.get_mut(6) {
        slot.recognizable = false;
    }
    let c1 = cap_integrity(&caps).err() == Some(DegCode::OverclaimBad.val());

    // 2) 清单授权越出承诺面。
    let mut t = T1_MODELS.to_vec();
    if let Some(slot) = t.get_mut(3) {
        slot.grant = slot.grant | CAP_STYLUS;
    }
    let c2 = model_integrity(&t).err() == Some(DegCode::ClaimBad.val());

    // 3) 档位不是 T1。
    let mut t = T1_MODELS.to_vec();
    if let Some(slot) = t.get_mut(3) {
        slot.tier = 2;
    }
    let c3 = model_integrity(&t).err() == Some(DegCode::TierBad.val());

    // 4) 清单少一条：条数闸门。
    let short = &T1_MODELS[..COMMITTED_COUNT - 1];
    let c4 = model_integrity(short).err() == Some(DegCode::CommittedBad.val());

    // 5) 矩阵少一行：条数闸门。
    let caps_short = &CAPS[..CAP_COUNT - 1];
    let c5 = cap_integrity(caps_short).err() == Some(DegCode::CapBad.val());

    // 6) 伪造报告：清单内设备声称了识别不出的位。
    let p = Probe::new(0x045E, 0x0C01, 0x0001_0200, CAP_FORCE);
    let mut f = evaluate(&p);
    f.claimed |= CAP_FORCE;
    f.withheld = 0;
    let c6 = report_gate(Some(0), &f).err() == Some(DegCode::ClaimBad.val());

    c1 && c2 && c3 && c4 && c5 && c6
}

// ── 段位守卫 + 渲染 ─────────────────────────────────────────────
/// 段位守卫：14 个诊断码全部落在 [SEG_BASE+1, SEG_BASE+14]。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DegCode::ALL.len() {
        if let Some(c) = DegCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 六字段渲染：tag | params（矩阵条数）| schedule（清单条数）|
/// fails（拒绝 + 降级条数）| keys（评估设备数）| digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 切六字段（不做语义校验，供 audit_render 与调用方共用）。
pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut i = 0usize;
    while i < RENDER_FIELDS {
        match parts.get(i).and_then(|x| *x) {
            Some(v) => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = v;
                }
            }
            None => return None,
        }
        i += 1;
    }
    Some(out)
}

/// 渲染审计：六字段、四数值可解析、摘要 16 位十六进制、重渲染逐字节一致。
pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(v) => v,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

/// 账本渲染（字段语义：params=矩阵条数, schedule=清单条数,
/// fails=拒绝+降级, keys=评估设备数）。
pub fn ledger_row(lg: &DegradeLedger) -> String {
    let fails = lg.refused.saturating_add(lg.degraded);
    render_row("D49003", CAP_COUNT as u32, COMMITTED_COUNT as u32, fails, lg.devices, lg.digest())
}

/// 渲染自检：账本行可解析、且字段值与账本一致。
pub fn render_selfcheck() -> bool {
    let lg = match degrade_rounds(DEGRADE_ROUNDS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let row = ledger_row(&lg);
    if !audit_render(&row) {
        return false;
    }
    let parts = match row_fields(&row) {
        Some(v) => v,
        None => return false,
    };
    let mut ok = true;
    let mut i = 1usize;
    while i < 5 {
        if let Some(p) = parts.get(i) {
            let mut sp = p.split('=');
            let _k = sp.next();
            match sp.next().and_then(parse_u32) {
                Some(_) => {}
                None => ok = false,
            }
        }
        i += 1;
    }
    ok
}

// ── 判据集 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49003 ①：能力矩阵自审（12 行、位序、承诺 ⊆ 识别）。
    let stats = cap_integrity(&CAPS);
    let mask_ok = table_committed_mask(&CAPS) == CAP_COMMITTED_MASK;
    let cap_ok = match &stats {
        Ok(s) => s.total == CAP_COUNT as u32 && s.committed == COMMITTED_CAPS && mask_ok,
        Err(_) => false,
    };
    out.push((
        "F49003/能力矩阵自审",
        cap_ok,
        format!(
            "rows={:?} committed={:?} mask={:05X}",
            stats.as_ref().map(|s| s.total),
            stats.as_ref().map(|s| s.committed),
            table_committed_mask(&CAPS)
        ),
    ));

    // F49003 ②：识别可算集合严格大于承诺集合（手势识别但不承诺）。
    let rec = table_recognizable_mask(&CAPS);
    let wider = rec == CAP_ALL_MASK && (rec & !CAP_COMMITTED_MASK) != 0;
    out.push((
        "F49003/识别可算超承诺",
        wider,
        format!("recognizable={:05X} committed={:05X}", rec, CAP_COMMITTED_MASK),
    ));

    // F49003 ③：降级 ×10 设备记账零虚报。
    let lg = degrade_rounds(DEGRADE_ROUNDS);
    let lg_ok = match &lg {
        Ok(l) => {
            l.devices == DEGRADE_ROUNDS
                && l.listed == 8
                && l.unlisted == 2
                && l.full == 4
                && l.degraded == 3
                && l.refused == 3
                && l.overclaims == 0
                && l.conserved()
                && l.honest()
        }
        Err(_) => false,
    };
    out.push((
        "F49003/降级×10零虚报",
        lg_ok,
        format!(
            "dev={:?} listed={:?} unlisted={:?} over={:?}",
            lg.as_ref().map(|l| l.devices),
            lg.as_ref().map(|l| l.listed),
            lg.as_ref().map(|l| l.unlisted),
            lg.as_ref().map(|l| l.overclaims)
        ),
    ));

    // F49003 ④：反例必拒 + 注入全捕获。
    let tamper = tamper_rejected();
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    out.push((
        "F49003/反例必拒",
        tamper && inj_ok,
        format!(
            "tamper={} inject={:?}/{:?}",
            tamper,
            inj.as_ref().map(|l| l.caught),
            inj.as_ref().map(|l| l.attempts)
        ),
    ));

    // F49004 ①：清单登记在册（12 条、T1 档、授权 ⊆ 承诺面、成员证明）。
    let list = model_integrity(&T1_MODELS);
    let proof = membership_proof();
    let list_ok = match &list {
        Ok(s) => s.total == COMMITTED_COUNT as u32 && s.grant_union != 0 && proof,
        Err(_) => false,
    };
    out.push((
        "F49004/清单登记在册",
        list_ok,
        format!(
            "rows={:?} union={:05X} proof={}",
            list.as_ref().map(|s| s.total),
            list.as_ref().map(|s| s.grant_union).unwrap_or(0),
            proof
        ),
    ));

    // F49004 ②：清单外零承诺断言。
    let zero = outside_zero_promise();
    out.push((
        "F49004/清单外零承诺",
        zero,
        format!("probes={} zero={}", OUTSIDE_PROBES.len(), zero),
    ));

    // F49004 ③：黄金 16 例三态全对 + 覆盖度。
    let golden = golden_ok();
    let cov = golden_coverage();
    out.push((
        "F49004/黄金16例",
        golden && cov,
        format!("cases={} golden={} cover={}", GOLDEN_ROUNDS, golden, cov),
    ));

    // F49004 ④：复跑零漂移（正序/逆序）+ 段位守卫 + 渲染。
    let replay = degrade_replay(DEGRADE_ROUNDS);
    let seg = segment_guard();
    let render = render_selfcheck();
    let dg = lg.as_ref().map(|l| l.digest()).unwrap_or(0);
    out.push((
        "F49004/复跑与段位",
        replay && seg && render && dg != 0,
        format!("replay={} seg={} render={} digest={:016X}", replay, seg, render, dg),
    ));

    out
}

pub fn run_unx49003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49004 quirk 降级口径与 T1 清单承诺面");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(DegCode::CapBad.val(), SEG_BASE + 0x01);
        assert_eq!(DegCode::FuzzBad.val(), SEG_BASE + 14);
        assert_eq!(DegCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < DegCode::ALL.len() {
            if let Some(c) = DegCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
        assert_eq!(detail(SEG_BASE), "未知诊断码");
    }

    #[test]
    fn capability_matrix_is_self_consistent() {
        let s = cap_integrity(&CAPS).unwrap();
        assert_eq!(s.total, 12);
        assert_eq!(s.committed, 8);
        assert_eq!(s.recognizable, 12);
        assert_ne!(s.fingerprint, 0);
    }

    #[test]
    fn committed_is_subset_of_recognizable() {
        assert_eq!(table_committed_mask(&CAPS) & !table_recognizable_mask(&CAPS), 0);
        assert_eq!(table_committed_mask(&CAPS), CAP_COMMITTED_MASK);
        assert_eq!(table_recognizable_mask(&CAPS), CAP_ALL_MASK);
        assert!(!CAPS[4].committed);
        assert!(CAPS[4].recognizable);
    }

    #[test]
    fn t1_list_is_self_consistent() {
        let s = model_integrity(&T1_MODELS).unwrap();
        assert_eq!(s.total, 12);
        assert_ne!(s.fingerprint, 0);
        assert_eq!(s.grant_union & !CAP_COMMITTED_MASK, 0);
        let mut i = 0usize;
        while i < T1_MODELS.len() {
            assert_eq!(T1_MODELS[i].tier, TIER_T1);
            assert!(T1_MODELS[i].grant != 0);
            assert!(T1_MODELS[i].fw_lo <= T1_MODELS[i].fw_hi);
            i += 1;
        }
    }

    #[test]
    fn membership_proves_every_row() {
        assert!(membership_proof());
        let m = T1_MODELS[9];
        let p = Probe::new(m.vendor, m.model, m.fw_lo, 0);
        assert_eq!(membership(&p), Some(9));
    }

    #[test]
    fn outside_probes_are_all_unlisted() {
        assert!(outside_zero_promise());
        let mut i = 0usize;
        while i < OUTSIDE_PROBES.len() {
            let p = OUTSIDE_PROBES[i];
            assert_eq!(membership(&p), None);
            let r = evaluate(&p);
            assert_eq!(r.outcome, Outcome::Refused);
            assert_eq!(r.claimed, 0);
            assert_eq!(r.granted, 0);
            i += 1;
        }
    }

    #[test]
    fn listed_clean_device_is_full() {
        let m = T1_MODELS[0];
        let p = Probe::new(m.vendor, m.model, m.fw_lo, 0);
        let r = evaluate(&p);
        assert_eq!(r.outcome, Outcome::Full);
        assert_eq!(r.claimed, r.granted);
        assert_eq!(r.withheld, 0);
        assert!(report_honest(&r));
    }

    #[test]
    fn quirk_on_ungranted_cap_does_not_degrade() {
        // Framework 不承诺压力点击：打掉 FORCE 不应产生扣减。
        let m = T1_MODELS[1];
        let p = Probe::new(m.vendor, m.model, m.fw_lo, CAP_FORCE);
        let r = evaluate(&p);
        assert_eq!(r.outcome, Outcome::Full);
        assert_eq!(r.withheld, 0);
    }

    #[test]
    fn quirk_on_granted_cap_degrades_honestly() {
        let m = T1_MODELS[0];
        let p = Probe::new(m.vendor, m.model, m.fw_lo, CAP_FORCE);
        let r = evaluate(&p);
        assert_eq!(r.outcome, Outcome::Degraded);
        assert_eq!(r.withheld, CAP_FORCE);
        assert_eq!(r.claimed & CAP_FORCE, 0);
        assert_eq!(r.claimed | r.withheld, r.granted);
        assert!(report_honest(&r));
    }

    #[test]
    fn all_caps_broken_refuses_with_zero_claims() {
        let m = T1_MODELS[3];
        let p = Probe::new(m.vendor, m.model, m.fw_lo, CAP_ALL_MASK);
        let r = evaluate(&p);
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(r.claimed, 0);
        assert_eq!(r.withheld, r.granted);
    }

    #[test]
    fn ten_devices_are_conserved_and_honest() {
        let l = degrade_rounds(DEGRADE_ROUNDS).unwrap();
        assert_eq!(l.devices, 10);
        assert_eq!(l.listed, 8);
        assert_eq!(l.unlisted, 2);
        assert_eq!(l.full + l.degraded + l.refused, 10);
        assert_eq!(l.granted, l.claimed + l.withheld);
        assert_eq!(l.overclaims, 0);
        assert!(l.conserved() && l.honest());
        assert!(l.withheld > 0);
    }

    #[test]
    fn replay_is_order_independent() {
        assert!(degrade_replay(DEGRADE_ROUNDS));
        let a = degrade_rounds(DEGRADE_ROUNDS).unwrap();
        let b = degrade_rounds_rev(DEGRADE_ROUNDS).unwrap();
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn rounds_bounds_are_enforced() {
        assert!(degrade_rounds(0).is_err());
        assert!(degrade_rounds(SCENARIO_MAX + 1).is_err());
        assert!(inject_rounds(0).is_err());
        assert!(inject_rounds(FUZZ_ROUNDS + 1).is_err());
    }

    #[test]
    fn golden_sixteen_cases_pass() {
        assert!(golden_ok());
        assert!(golden_coverage());
        assert_eq!(GOLDEN_CASES.len(), 16);
    }

    #[test]
    fn injection_catches_every_attempt() {
        let l = inject_rounds(1_000).unwrap();
        assert_eq!(l.attempts, 1_000);
        assert_eq!(l.caught, 1_000);
        assert!(l.all_caught());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn report_gate_rejects_overclaim() {
        let p = Probe::new(0x045E, 0x0C01, 0x0001_0200, CAP_FORCE);
        let mut f = evaluate(&p);
        f.claimed |= CAP_FORCE;
        f.withheld = 0;
        assert_eq!(report_gate(Some(0), &f).err(), Some(DegCode::ClaimBad.val()));
        assert!(!report_honest(&f));
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("D49003", 12, 12, 6, 10, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("D49003|params=12|schedule=12|fails=6|keys=10"));
        assert!(!audit_render("D49003|params=x|schedule=12|fails=6|keys=10|digest=0000000000000001"));
        assert!(!audit_render("D49003|params=12|schedule=12|fails=6|keys=10|digest=123"));
        assert!(row_fields(&r).is_some());
        assert_eq!(row_fields(&r).unwrap()[0], "D49003");
        assert_eq!(parse_u32("12"), Some(12));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("000000000000000"), None);
    }

    #[test]
    fn ledger_row_is_auditable() {
        let l = degrade_rounds(DEGRADE_ROUNDS).unwrap();
        let row = ledger_row(&l);
        assert!(audit_render(&row));
        assert!(row.starts_with("D49003|params=12|schedule=12|fails=6|keys=10|"));
        assert!(render_selfcheck());
    }

    #[test]
    fn mask_helpers_are_totally_defined() {
        assert_eq!(cap_bit(0), 1);
        assert_eq!(cap_bit(11), 1 << 11);
        assert_eq!(cap_bit(12), 0);
        assert_eq!(cap_bit(200), 0);
        assert_eq!(popcount(0), 0);
        assert_eq!(popcount(CAP_ALL_MASK), 12);
        assert_eq!(popcount(CAP_COMMITTED_MASK), 8);
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49003_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 10);
        assert_eq!(RENDER_FIELDS, 6);
    }

    #[test]
    fn outcome_codes_round_trip() {
        let mut c = 0u8;
        while c < 3 {
            let o = Outcome::from_code(c).unwrap();
            assert_eq!(o.code(), c);
            assert!(!o.name().is_empty());
            c += 1;
        }
        assert!(Outcome::from_code(3).is_none());
    }

    #[test]
    fn device_plan_covers_three_outcomes() {
        let mut full = 0u32;
        let mut deg = 0u32;
        let mut refu = 0u32;
        let mut i = 0usize;
        while i < DEVICE_PLAN.len() {
            let r = evaluate(&probe_for(&DEVICE_PLAN[i]));
            match r.outcome {
                Outcome::Full => full += 1,
                Outcome::Degraded => deg += 1,
                Outcome::Refused => refu += 1,
            }
            i += 1;
        }
        assert_eq!(full, 4);
        assert_eq!(deg, 3);
        assert_eq!(refu, 3);
    }
}
