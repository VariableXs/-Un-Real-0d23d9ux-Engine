//! UNX-F2404 · 页帧序号与物理地址互转原语 · 诊断段 0x6D20（独占 0x6D20~0x6D2F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2404）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2404-J1 互转原语 10^7 次宿主随机往返零误差，边界值（0/最高帧/
//! 未对齐入参）按契约拒绝。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 往返零误差：pfn→phys→pfn 与 phys→pfn→phys 两条往返链，以确定性
//!        LCG 伪随机（同种子同序列，可 CI 回放）驱动 10^7 次，任何一次
//!        往返偏差即红账。全程整数运算，零浮点。
//!   J1-b 边界契约：pfn=0 合法（物理 0 页是真实页帧）；pfn 超过最高帧号
//!        拒绝并留账；phys 未按 4KiB 对齐的入参按「未对齐拒绝」契约处理，
//!        绝不静默截断（截断=伪造对齐，是错误来源）。
//!   J1-c 溢出面：u64 物理地址域上 pfn<<12 在最高帧附近可能溢出，
//!        用 checked_shl/checked_mul 显式判溢出，溢出走 Err 不绕行。
//!
//! **建模口径（诚实声明）**：本模块是 PFN↔物理地址的纯算术契约层，
//! 物理地址空间上限以 64 位寻址 + 可配置「最高帧」账为准；不触碰真实
//! 页表（真机路径在 A4 域页表面）。所有随机序列由 LCG（乘 6364136223846793005、
//! 加 1442695040888963407，Knuth MMIX 常数）确定生成，种子写死。
//!
//! **段位复核记录**：0x6D20~0x6D2F 经全树 grep（varix/src 全目录
//! 0x6Dxx 字面量扫描）零命中后选定；0x6D00/0x6D10 段已被他单占用。
//!
//! **自包含纪律**：仅 `use core::…`/`use alloc::…`，禁 `use crate::…`；
//! 零 unwrap/expect/panic!/运行期 assert!；可失败全走 Result；
//! const fn 只用 while+索引。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6D20~0x6D2F 十六码闭集，一码一义）
// ===========================================================================

/// 段基址（保留码：段标识，不作错误码）。
pub const SEG_BASE: u16 = 0x6D20;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x6D2F;

/// 页帧大小偏移：4KiB = 2^12，x86-64 基础页粒度。
pub const PAGE_SHIFT: u32 = 12;
/// 页帧大小字节数：4096。
pub const PAGE_SIZE: u64 = 1u64 << PAGE_SHIFT;
/// 页内偏移掩码：低 12 位。
pub const PAGE_OFFSET_MASK: u64 = PAGE_SIZE - 1;

/// 错误码闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfnError {
    /// 0x6D21 物理地址未按页对齐。
    PhysUnaligned = 1,
    /// 0x6D22 页帧号超出最高帧账。
    PfnBeyondTop,
    /// 0x6D23 物理地址换算溢出（pfn<<12 超 u64）。
    PhysOverflow,
    /// 0x6D24 物理地址超出寻址上限账。
    PhysBeyondLimit,
    /// 0x6D25 往返验证偏差（对账失败）。
    RoundtripDrift,
    /// 0x6D26 对账样本数为零（空跑不叫绿）。
    EmptyAudit,
    /// 0x6D27 最高帧账未初始化。
    TopFrameUnset,
    /// 0x6D28 页粒度非法（非 2 的幂或 < 4KiB）。
    BadGranularity,
}

impl PfnError {
    /// 诊断码：SEG_BASE + 变体序号，闭集内一码一义。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断：说明哪一步错、该怎么改。
    pub fn detail(self) -> String {
        match self {
            PfnError::PhysUnaligned => format!(
                "[0x{:04X}] 物理地址未按 4KiB 对齐：入参低 12 位非零。\
                 改法：调用方先做 addr & !PAGE_OFFSET_MASK 截断到页界，\
                 或改用本模块 contract_to_page() 显式页对齐入口（留痕）。",
                self.code()
            ),
            PfnError::PfnBeyondTop => format!(
                "[0x{:04X}] 页帧号超出最高帧账：先查 top_frame() 当前账，\
                 入参必须 < top_frame。改法：扩内存区间登记后重设 top_frame。",
                self.code()
            ),
            PfnError::PhysOverflow => format!(
                "[0x{:04X}] pfn<<12 溢出 u64：页帧号过大。\
                 改法：核对 pfn 来源是否被污染（如把字节数当帧号传入）。",
                self.code()
            ),
            PfnError::PhysBeyondLimit => format!(
                "[0x{:04X}] 物理地址超出寻址上限账（phys_limit）。\
                 改法：确认平台物理位宽（MAX_PHYS_ADDR_BIT）后再换算。",
                self.code()
            ),
            PfnError::RoundtripDrift => format!(
                "[0x{:04X}] 往返验证出现偏差：pfn→phys→pfn 或反向链在某样本\
                 上不自洽。改法：取对账账本中首个偏差样本（样本号+双端值）\
                 复现，检查 PAGE_SHIFT 是否被运行期改动。",
                self.code()
            ),
            PfnError::EmptyAudit => format!(
                "[0x{:04X}] 对账样本数为零：空跑不得报绿。\
                 改法：样本数至少 AUDIT_MIN_SAMPLES。",
                self.code()
            ),
            PfnError::TopFrameUnset => format!(
                "[0x{:04X}] 最高帧账未初始化：先调用 set_top_frame()。\
                 改法：引导期内存探测完成后立刻登记，禁止默认 0。",
                self.code()
            ),
            PfnError::BadGranularity => format!(
                "[0x{:04X}] 页粒度非法：必须为 2 的幂且 ≥ 4096。\
                 改法：检查是否误传了页数或掩码。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、常量与平台账
// ===========================================================================

/// 对账最小样本数：低于此数的自检不算绿（防零跑假绿）。
pub const AUDIT_MIN_SAMPLES: u32 = 1_000;
/// 往返全量验证样本数：J1 口径 10^7。分段执行由调用方控制。
pub const AUDIT_FULL_SAMPLES: u64 = 10_000_000;
/// LCG 乘子（Knuth MMIX）。
const LCG_MUL: u64 = 6364136223846793005;
/// LCG 增量（Knuth MMIX）。
const LCG_ADD: u64 = 1442695040888963407;
/// 审计种子：写死保证可复现（同种子同序列）。
pub const AUDIT_SEED: u64 = 0x2404_6D20_9E37_79B9;
/// 物理寻址上限位宽：x86-64 规范 52 位（4KiB 页口径 52 位物理）。
pub const MAX_PHYS_ADDR_BIT: u32 = 52;

/// 平台寻址账：一次登记，只读。
#[derive(Clone, Copy, Debug)]
pub struct PlatformAddr {
    /// 最高合法页帧号（不含）：pfn 合法域 [0, top_frame)。
    top_frame: u64,
    /// 物理地址上限（不含）：由 top_frame 闭式推出。
    phys_limit: u64,
    /// 已登记标志：未登记前一切换算拒绝。
    seeded: bool,
}

impl PlatformAddr {
    /// 空账：未初始化态。
    pub const fn new() -> Self {
        PlatformAddr {
            top_frame: 0,
            phys_limit: 0,
            seeded: false,
        }
    }

    /// 登记最高帧：闭式推出物理上限。帧数为 0 拒绝。
    pub fn seed(&mut self, top_frame: u64) -> result::Result<(), PfnError> {
        if self.seeded {
            // 幂等：同值重复登记放行（引导重试路径），异值拒绝（防漂移）。
            if self.top_frame == top_frame {
                return Ok(());
            }
            return Err(PfnError::TopFrameUnset);
        }
        if top_frame == 0 {
            return Err(PfnError::PfnBeyondTop);
        }
        // 闭式上限：top_frame * PAGE_SIZE；溢出显性拒绝。
        let limit = match top_frame.checked_mul(PAGE_SIZE) {
            Some(v) => v,
            None => return Err(PfnError::PhysOverflow),
        };
        self.top_frame = top_frame;
        self.phys_limit = limit;
        self.seeded = true;
        Ok(())
    }

    /// 最高帧号（不含）。未登记返回 None（诚实不造假）。
    pub fn top_frame(&self) -> Option<u64> {
        if self.seeded {
            Some(self.top_frame)
        } else {
            None
        }
    }

    /// 物理上限（不含）。
    pub fn phys_limit(&self) -> Option<u64> {
        if self.seeded {
            Some(self.phys_limit)
        } else {
            None
        }
    }
}

// ===========================================================================
// 三、核心互转原语
// ===========================================================================

/// pfn → 物理地址：严格对账入口。
///
/// 契约：pfn 必须 < top_frame；溢出与越界显性拒绝，绝不静默。
pub fn pfn_to_phys(p: &PlatformAddr, pfn: u64) -> result::Result<u64, PfnError> {
    let top = match p.top_frame() {
        Some(t) => t,
        None => return Err(PfnError::TopFrameUnset),
    };
    if pfn >= top {
        return Err(PfnError::PfnBeyondTop);
    }
    match pfn.checked_mul(PAGE_SIZE) {
        Some(a) => Ok(a),
        None => Err(PfnError::PhysOverflow),
    }
}

/// 物理地址 → pfn：严格契约入口（不截断）。
///
/// 契约：地址必须页对齐；未对齐拒绝（调用方应显式走 floor 路径）。
pub fn phys_to_pfn(p: &PlatformAddr, addr: u64) -> result::Result<u64, PfnError> {
    if !p.seeded {
        return Err(PfnError::TopFrameUnset);
    }
    if addr & PAGE_OFFSET_MASK != 0 {
        return Err(PfnError::PhysUnaligned);
    }
    let limit = p.phys_limit;
    if addr >= limit {
        return Err(PfnError::PhysBeyondLimit);
    }
    Ok(addr >> PAGE_SHIFT)
}

/// 物理地址 → pfn 的下取整宽容入口：显式留痕的截断（非静默）。
///
/// 与 phys_to_pfn 的区别：允许页内偏移，返回所在页帧 + 页内偏移；
/// 这是「知情截断」，与「伪造对齐」的静默截断有账面区别。
pub fn phys_to_pfn_floor(p: &PlatformAddr, addr: u64) -> result::Result<(u64, u64), PfnError> {
    if !p.seeded {
        return Err(PfnError::TopFrameUnset);
    }
    if addr >= p.phys_limit {
        return Err(PfnError::PhysBeyondLimit);
    }
    let off = addr & PAGE_OFFSET_MASK;
    Ok((addr >> PAGE_SHIFT, off))
}

/// 页对齐入口：地址所在页的页基址（知情取整）。
pub fn page_base_of(p: &PlatformAddr, addr: u64) -> result::Result<u64, PfnError> {
    let (pfn, _) = phys_to_pfn_floor(p, addr)?;
    match pfn.checked_mul(PAGE_SIZE) {
        Some(a) => Ok(a),
        None => Err(PfnError::PhysOverflow),
    }
}

// ===========================================================================
// 四、确定性 LCG 与往返验证引擎
// ===========================================================================

/// 确定性 LCG 状态：同种子同序列。
pub struct Lcg(u64);

impl Lcg {
    /// 固定种子构造。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed ^ 0xA404_6D20_5BF0_A8B3)
    }

    /// 下一个 64 位伪随机数（全幅）。
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.0
    }

    /// [0, bound) 内伪随机：拒绝采样避免模偏。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        // 2^64 上取整分桶：简单拒绝采样（最坏重试率 < 50%）。
        let zone = u64::MAX - (u64::MAX % bound) - 1;
        let mut v = self.next_u64();
        while v > zone {
            v = self.next_u64();
        }
        v % bound
    }
}

/// 单次往返账目：偏差样本留痕。
#[derive(Clone, Copy, Debug)]
pub struct DriftSample {
    /// 样本序号（从 0 起）。
    pub seq: u64,
    /// 起点值（pfn 或 phys）。
    pub origin: u64,
    /// 往返终值。
    pub returned: u64,
    /// 方向：true = pfn→phys→pfn。
    pub fwd: bool,
}

/// 往返对账账本：可查前 K 个偏差样本。
pub struct RoundtripLedger {
    /// 累计验证次数。
    pub checked: u64,
    /// 偏差次数。
    pub drifts: u64,
    /// 前几个偏差样本（封顶 8 个，够复现即可）。
    pub samples: [Option<DriftSample>; 8],
    /// 样本槽游标。
    cursor: usize,
}

impl RoundtripLedger {
    /// 空账。
    pub const fn new() -> Self {
        RoundtripLedger {
            checked: 0,
            drifts: 0,
            samples: [None; 8],
            cursor: 0,
        }
    }

    /// 记一次偏差（前 8 个封顶留痕）。
    fn record_drift(&mut self, seq: u64, origin: u64, returned: u64, fwd: bool) {
        if self.cursor < 8 {
            self.samples[self.cursor] = Some(DriftSample {
                seq,
                origin,
                returned,
                fwd,
            });
            self.cursor += 1;
        }
        self.drifts += 1;
    }

    /// 全绿判定：验证数达标且零偏差。
    pub fn is_green(&self, min: u64) -> bool {
        self.checked >= min && self.drifts == 0
    }
}

/// J1-a 证据生成器：pfn→phys→pfn 往返 n 次（确定性序列）。
///
/// `n` 由调用方决定；J1 全量口径传 `AUDIT_FULL_SAMPLES`（10^7），
/// 快速抽核传 `AUDIT_MIN_SAMPLES`。每一步都走严格契约入口，
/// 随机样本天然合法（域内采样），因此任何 Err 都按偏差入账。
pub fn roundtrip_pfn_forward(
    p: &PlatformAddr,
    n: u64,
) -> result::Result<RoundtripLedger, PfnError> {
    let top = match p.top_frame() {
        Some(t) if t > 0 => t,
        _ => return Err(PfnError::TopFrameUnset),
    };
    let mut rng = Lcg::new(AUDIT_SEED);
    let mut led = RoundtripLedger::new();
    let mut i: u64 = 0;
    while i < n {
        let pfn = rng.next_below(top);
        match pfn_to_phys(p, pfn) {
            Ok(phys) => match phys_to_pfn(p, phys) {
                Ok(back) => {
                    if back != pfn {
                        led.record_drift(i, pfn, back, true);
                    }
                }
                Err(_) => led.record_drift(i, pfn, u64::MAX, true),
            },
            Err(_) => led.record_drift(i, pfn, u64::MAX, true),
        }
        led.checked += 1;
        i += 1;
    }
    Ok(led)
}

/// J1-a 证据生成器（反向链）：phys→pfn→phys 往返 n 次。
///
/// 采样口径：在页基址域采样（对齐地址），保证反向链样本合法。
pub fn roundtrip_phys_backward(
    p: &PlatformAddr,
    n: u64,
) -> result::Result<RoundtripLedger, PfnError> {
    let top = match p.top_frame() {
        Some(t) if t > 1 => t,
        _ => return Err(PfnError::TopFrameUnset),
    };
    let mut rng = Lcg::new(AUDIT_SEED ^ 0x5150_2404);
    let mut led = RoundtripLedger::new();
    let mut i: u64 = 0;
    while i < n {
        let pfn = rng.next_below(top - 1);
        let phys = pfn << PAGE_SHIFT;
        match phys_to_pfn(p, phys) {
            Ok(back) => {
                let phys2 = match pfn_to_phys(p, back) {
                    Ok(v) => v,
                    Err(_) => u64::MAX,
                };
                if back != pfn || phys2 != phys {
                    led.record_drift(i, phys, phys2, false);
                }
            }
            Err(_) => led.record_drift(i, phys, u64::MAX, false),
        }
        led.checked += 1;
        i += 1;
    }
    Ok(led)
}

// ===========================================================================
// 五、边界契约验证（J1-b）
// ===========================================================================

/// 边界注入账：每类注入的接受/拒绝明细。
#[derive(Clone, Copy, Debug)]
pub struct BoundaryCase {
    /// 注入种类：0=未对齐地址 1=超界 pfn 2=超界 phys 3=未初始化账。
    pub kind: u8,
    /// 注入值。
    pub value: u64,
    /// 契约判定：true=按契约拒绝。
    pub rejected: bool,
    /// 拒绝码（接受时为 0）。
    pub code: u16,
}

/// 边界契约批量验证：四类注入逐条核，全部按契约拒绝才绿。
///
/// J1-b 口径：边界值（0/最高帧/未对齐入参）按契约拒绝——注意
/// pfn=0 是合法页帧（不拒），真正拒的是：未对齐地址、越界 pfn、
/// 越界物理地址、未初始化账上的任何换算。
pub fn boundary_contract_audit(p: &PlatformAddr) -> result::Result<Vec<BoundaryCase>, PfnError> {
    let top = match p.top_frame() {
        Some(t) if t > 0 => t,
        Some(_) => return Err(PfnError::PfnBeyondTop),
        None => return Err(PfnError::TopFrameUnset),
    };
    let mut cases: Vec<BoundaryCase> = Vec::new();
    // 1) 未对齐物理地址（低 12 位逐位单置，共 12 例）。
    let mut b: u32 = 0;
    while b < PAGE_SHIFT {
        let addr = (top - 1) * PAGE_SIZE + (1u64 << b);
        let r = phys_to_pfn(p, addr);
        cases.push(BoundaryCase {
            kind: 0,
            value: addr,
            rejected: r.is_err(),
            code: match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            },
        });
        b += 1;
    }
    // 2) 越界 pfn：top、top+1、u64 饱和近似值。
    let mut probe: u64 = top;
    let mut k = 0;
    while k < 3 {
        let r = pfn_to_phys(p, probe);
        cases.push(BoundaryCase {
            kind: 1,
            value: probe,
            rejected: r.is_err(),
            code: match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            },
        });
        probe = match probe.checked_add(top) {
            Some(v) => v,
            None => break,
        };
        k += 1;
    }
    // 3) 越界物理地址：phys_limit 本体与 +页。
    let limit = p.phys_limit;
    let mut pa = limit;
    let mut k2 = 0;
    while k2 < 2 {
        let r = phys_to_pfn(p, pa);
        cases.push(BoundaryCase {
            kind: 2,
            value: pa,
            rejected: r.is_err(),
            code: match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            },
        });
        pa = match pa.checked_add(PAGE_SIZE) {
            Some(v) => v,
            None => break,
        };
        k2 += 1;
    }
    // 4) 合法边界放行核：pfn=0 与 pfn=top-1 必须双向可达。
    let lo = pfn_to_phys(p, 0);
    cases.push(BoundaryCase {
        kind: 3,
        value: 0,
        rejected: lo.is_err(),
        code: match lo {
            Err(e) => e.code(),
            Ok(_) => 0,
        },
    });
    let hi = pfn_to_phys(p, top - 1);
    cases.push(BoundaryCase {
        kind: 3,
        value: top - 1,
        rejected: hi.is_err(),
        code: match hi {
            Err(e) => e.code(),
            Ok(_) => 0,
        },
    });
    Ok(cases)
}

/// 边界全绿判定：所有「应拒」样本确实被拒，合法边界样本确实放行。
pub fn boundary_all_contract(cases: &[BoundaryCase]) -> bool {
    let mut i = 0;
    while i < cases.len() {
        let c = &cases[i];
        // kind 3 是放行核：必须 NOT rejected；其余必须 rejected。
        let expect_reject = c.kind != 3;
        if expect_reject != c.rejected {
            return false;
        }
        i += 1;
    }
    cases.len() > 0
}

// ===========================================================================
// 六、未初始化账防线（J1-b 附带）
// ===========================================================================

/// 空账上的换算必须全部拒绝（防「默认 0 帧」假绿）。
pub fn uninitialized_must_reject() -> result::Result<(), PfnError> {
    let blank = PlatformAddr::new();
    let r1 = pfn_to_phys(&blank, 0);
    let r2 = phys_to_pfn(&blank, 0);
    let r3 = phys_to_pfn_floor(&blank, 0);
    if r1.is_err() && r2.is_err() && r3.is_err() {
        Ok(())
    } else {
        Err(PfnError::TopFrameUnset)
    }
}

// ===========================================================================
// 七、综合自检（功能面，非判据文件）
// ===========================================================================

/// 自检摘要：J1 各子条一次跑完的结论。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 往返抽核验证数。
    pub rt_checked: u64,
    /// 往返偏差数。
    pub rt_drifts: u64,
    /// 边界注入样本数。
    pub boundary_cases: usize,
    /// 边界违例数。
    pub boundary_violations: usize,
    /// 未初始化防线是否通过。
    pub blank_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿：抽核达标 + 零偏差 + 零违例 + 防线过。
    pub fn all_green(&self) -> bool {
        self.rt_checked >= AUDIT_MIN_SAMPLES as u64
            && self.rt_drifts == 0
            && self.boundary_violations == 0
            && self.boundary_cases > 0
            && self.blank_guard_ok
    }
}

/// 一次跑完 J1 全部子条（抽核口径 10^4；全量 10^7 由调用方分片跑）。
pub fn self_audit() -> result::Result<SelfAudit, PfnError> {
    let mut p = PlatformAddr::new();
    // 抽核平台账：1 Mi 页（4 GiB 空间），样本密度足够。
    p.seed(1 << 20)?;
    let fwd = roundtrip_pfn_forward(&p, 10_000)?;
    let bwd = roundtrip_phys_backward(&p, 10_000)?;
    let drifts = fwd.drifts + bwd.drifts;
    let checked = fwd.checked + bwd.checked;
    if fwd.checked == 0 || bwd.checked == 0 {
        return Err(PfnError::EmptyAudit);
    }
    let cases = boundary_contract_audit(&p)?;
    let mut viol = 0usize;
    let mut i = 0;
    while i < cases.len() {
        let c = &cases[i];
        let expect_reject = c.kind != 3;
        if expect_reject != c.rejected {
            viol += 1;
        }
        i += 1;
    }
    let blank_ok = uninitialized_must_reject().is_ok();
    Ok(SelfAudit {
        rt_checked: checked,
        rt_drifts: drifts,
        boundary_cases: cases.len(),
        boundary_violations: viol,
        blank_guard_ok: blank_ok,
    })
}
