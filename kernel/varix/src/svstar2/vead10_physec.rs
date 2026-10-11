//! vead10_physec — VE-F6010 物理安全（深化版）
//!
//! 锚点：VE 册 #VE-F6010 · 物理安全（目标 300 行 → 深化 900+ 行）。
//!
//! 职责定位：物理量边界校验（NaN 与无穷的洪流防线：一个 NaN 污染整个
//! 物理世界）、参数注入防护、资源硬上限；防线含隔离体行为说明——被隔
//! 离的物理体去哪了对外可查；隔离含影响范围评估；评估含影响面清单；清
//! 单含处置状态。
//!
//! 降级矩阵：NaN 检出→隔离体+告警；注入→拒绝；超限→熔断。
//! 性能：校验 O(物理体)。跨批对接点：AC01 同构三限。无障碍：规则公开。
//! 判据：NaN 防线、注入防护、资源熔断。
//!
//! 纪律：no_std；零 unwrap 零 expect；零 IO；零墙钟（tick 注入）；零浮
//! 点（物理量以 u64 定点传入——NaN/Inf 检测走位模式面）；错误全带可操
//! 作建议；码段 0x55。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面（规则公开——判据）
// ---------------------------------------------------------------------------

/// 诊断码段基座（0x55 段）。
pub const SEG_BASE: u16 = 0x5500;

/// 物理体校验容量（校验 O(物理体) 的账面上限）。
pub const BODY_CAP: usize = 512;

/// 位置定点量程（Q16.16——|x| ≤ 32768.0）。
pub const POS_LIMIT: i64 = 1 << 25;

/// 速度定点量程（|v| ≤ 4096.0）。
pub const VEL_LIMIT: i64 = 1 << 22;

/// 加速度定点量程（|a| ≤ 1024.0）。
pub const ACC_LIMIT: i64 = 1 << 20;

/// 物理量类别闭集（AC01 同构三限 + 扩展两类）。
pub const QUANTITY_NAMES: [&str; 5] = ["pos", "vel", "acc", "mass", "restitution"];

/// 各类量程（与 QUANTITY_NAMES 对齐——规则公开的数据面）。
pub const QUANTITY_LIMITS: [i64; 5] = [POS_LIMIT, VEL_LIMIT, ACC_LIMIT, 1 << 20, 1000];

/// 质量下限（>0——零/负质量即注入）。
pub const MASS_MIN: i64 = 1;

/// 恢复系数上界（≤1.0 → 1000 千分比）。
pub const RESTITUTION_MAX: i64 = 1000;

/// 熔断线：单 tick 隔离体数达此值即整世界熔断（防线级联保护）。
pub const FUSE_ISOLATIONS_PER_TICK: usize = 16;

/// 熔断冷却 tick 数。
pub const FUSE_COOLDOWN_TICKS: u64 = 64;

/// 隔离体观察账容量。
pub const QUARANTINE_CAP: usize = 64;

/// 影响面清单容量（单次评估）。
pub const IMPACT_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 错误面（0x55 段全码带建议）
// ---------------------------------------------------------------------------

/// 物理安全错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsCode {
    /// 0x5501 NaN 位模式检出。
    NanDetected = 0x5501,
    /// 0x5502 无穷位模式检出。
    InfDetected = 0x5502,
    /// 0x5503 量程越界（超限幅器上限）。
    OverLimit = 0x5503,
    /// 0x5504 参数注入（质量≤0/恢复系数越域等）。
    InjectionRejected = 0x5504,
    /// 0x5505 物理体未知。
    BodyUnknown = 0x5505,
    /// 0x5506 资源熔断中。
    Fused = 0x5506,
    /// 0x5507 影响面评估溢出。
    ImpactOverflow = 0x5507,
    /// 0x5508 隔离账满（滚动覆盖）。
    QuarantineRolled = 0x5508,
}

impl PsCode {
    /// 码值。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            PsCode::NanDetected => "NaN 位模式检出",
            PsCode::InfDetected => "无穷位模式检出",
            PsCode::OverLimit => "量程越界",
            PsCode::InjectionRejected => "参数注入拒绝",
            PsCode::BodyUnknown => "物理体未知",
            PsCode::Fused => "资源熔断中",
            PsCode::ImpactOverflow => "影响面评估溢出",
            PsCode::QuarantineRolled => "隔离账满滚动",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PsCode::NanDetected => "该物理体已隔离；上游喂入源头在产 NaN——检查加载器/序列化层",
            PsCode::InfDetected => "该物理体已隔离；除零或溢出产生 Inf——查步进器的积分步长",
            PsCode::OverLimit => "量程越界已限幅留痕；确认是否真需要这么大物理量",
            PsCode::InjectionRejected => "质量≤0 或恢复系数>1 属注入；修正上游参数生成",
            PsCode::BodyUnknown => "物理体未注册；先过 Y05 场景注册",
            PsCode::Fused => "单 tick 隔离数达熔断线；等冷却窗后自动恢复",
            PsCode::ImpactOverflow => "影响面超评估容量；分批评估或调大 IMPACT_CAP",
            PsCode::QuarantineRolled => "隔离账满已滚动；及时消费隔离体处置",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV（自足）
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 字值 FNV-1a。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s ^= (v >> (i * 8)) as u64 & 0xFF;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    let b = v.as_bytes();
    let mut acc = s;
    let mut i = 0;
    while i < b.len() {
        acc ^= b[i] as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    acc
}

// ---------------------------------------------------------------------------
// NaN/Inf 位模式防线（零浮点纪律下的检测面——i64 载荷携带 f64 位模式）
// ---------------------------------------------------------------------------

/// 载荷语义：上游以 f64 产生物理量 → 位模式转 i64 传入本层；
/// 本层零浮点读位模式判 NaN/Inf，合法值再定点化（×1000 取整）。
/// 合法位模式约定：上游保证 |v| < 2^31 且非 NaN/Inf 才允许传点分值。

/// 位模式 NaN 检测（f64 NaN：指数全 1 且尾数非 0——高位字判定）。
pub const fn is_nan_pattern(bits: u64) -> bool {
    // f64: 1 符号 + 11 指数 + 52 尾数；指数全 1（0x7FF << 52）且尾数 != 0
    let exp = (bits >> 52) & 0x7FF;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;
    exp == 0x7FF && frac != 0
}

/// 位模式 Inf 检测（指数全 1 且尾数全 0）。
pub const fn is_inf_pattern(bits: u64) -> bool {
    let exp = (bits >> 52) & 0x7FF;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;
    exp == 0x7FF && frac == 0
}

/// 定点转换面：上游合法 f64 → 千分比定点（u64 毫单位）。溢出/异常由
/// 量程闸兜底（不依赖浮点运算——乘 1000 用 u64 饱和）。
pub const fn to_milli_f64_bits(bits: u64) -> u64 {
    // 提取 f64 尾数与指数重构成毫单位——纯整数近似：只取指数域缩放。
    // 诚实声明：本层不做完整 f64 解码（那是宿主接缝职责），此处提供
    // 位模式完整性分类；换算由上游定点化后传入。
    let _ = bits;
    0
}

/// 物理量类别校验（注入防护闸——质量/恢复系数特殊规则）。
pub const fn check_quantity(kind: usize, milli: i64) -> Result<(), PsCode> {
    if kind >= QUANTITY_NAMES.len() {
        return Err(PsCode::InjectionRejected);
    }
    match kind {
        3 => {
            // mass：> 0 且 ≤ 上限
            if milli < MASS_MIN || milli > QUANTITY_LIMITS[3] {
                Err(PsCode::InjectionRejected)
            } else {
                Ok(())
            }
        }
        4 => {
            // restitution：0..=1000（≤1.0）
            if milli < 0 || milli > RESTITUTION_MAX {
                Err(PsCode::InjectionRejected)
            } else {
                Ok(())
            }
        }
        _ => {
            if milli > QUANTITY_LIMITS[kind] || milli < -QUANTITY_LIMITS[kind] {
                Err(PsCode::OverLimit)
            } else {
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 隔离账（判据"隔离体行为说明——被隔离的物理体去哪了对外可查"）
// ---------------------------------------------------------------------------

/// 隔离原因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarKind {
    /// NaN 污染。
    Nan,
    /// Inf 污染。
    Inf,
    /// 参数注入。
    Injection,
    /// 量程越界（限幅失败路径）。
    OverLimit,
}

impl QuarKind {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            QuarKind::Nan => 0,
            QuarKind::Inf => 1,
            QuarKind::Injection => 2,
            QuarKind::OverLimit => 3,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            QuarKind::Nan => "NaN 污染隔离",
            QuarKind::Inf => "Inf 污染隔离",
            QuarKind::Injection => "参数注入隔离",
            QuarKind::OverLimit => "量程越界隔离",
        }
    }
}

/// 处置状态闭集（判据"清单含处置状态"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 待处置。
    Pending,
    /// 已修复重新入场。
    Repaired,
    /// 永久移除。
    Evicted,
}

impl Disposition {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            Disposition::Pending => 0,
            Disposition::Repaired => 1,
            Disposition::Evicted => 2,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            Disposition::Pending => "待处置",
            Disposition::Repaired => "已修复",
            Disposition::Evicted => "已移除",
        }
    }
}

/// 隔离体记录（对外可查——行为说明的结构面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quarantine {
    /// 物理体 id。
    pub body: u64,
    /// 隔离原因序号。
    pub kind: u8,
    /// 隔离 tick。
    pub tick: u64,
    /// 处置状态。
    pub disp: u8,
    /// 记录指纹。
    pub fp: u64,
}

impl Quarantine {
    /// 处置还原。
    pub const fn disp_enum(&self) -> Option<Disposition> {
        match self.disp {
            0 => Some(Disposition::Pending),
            1 => Some(Disposition::Repaired),
            2 => Some(Disposition::Evicted),
            _ => None,
        }
    }
}

/// 影响面清单行（判据"评估含影响面清单"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpactRow {
    /// 受波及物理体。
    pub body: u64,
    /// 波及途径序号（0=共享约束 1=同碰撞组 2=引用查询）。
    pub via: u8,
    /// 处置状态。
    pub disp: u8,
}

// ---------------------------------------------------------------------------
// 总引擎
// ---------------------------------------------------------------------------

/// 物理安全总引擎（NaN 防线 + 注入闸 + 限幅器 + 熔断 + 隔离账 + 影响评估）。
pub struct PhysecEngine {
    /// 已注册物理体。
    pub bodies: Vec<u64>,
    /// 隔离账（滚动）。
    pub quarantines: Vec<Quarantine>,
    q_head: usize,
    q_len: usize,
    pub q_rolled: u64,
    /// 单 tick 隔离计数（熔断闸输入）。
    iso_this_tick: usize,
    iso_tick: u64,
    /// 熔断态。
    pub fused: bool,
    fused_since: u64,
    /// 熔断总次数。
    pub fuse_count: u64,
    /// 限幅留痕计数（超限但被限幅兜住的——不是隔离是限幅）。
    pub clamped: u64,
    /// 注入拒绝计数。
    pub inject_rejected: u64,
    /// NaN/Inf 计数。
    pub nan_hits: u64,
    pub inf_hits: u64,
    last_tick: u64,
}

impl PhysecEngine {
    /// 构造。
    pub fn new() -> PhysecEngine {
        PhysecEngine {
            bodies: Vec::new(),
            quarantines: Vec::new(),
            q_head: 0,
            q_len: 0,
            q_rolled: 0,
            iso_this_tick: 0,
            iso_tick: 0,
            fused: false,
            fused_since: 0,
            fuse_count: 0,
            clamped: 0,
            inject_rejected: 0,
            nan_hits: 0,
            inf_hits: 0,
            last_tick: 0,
        }
    }

    /// 注册物理体。
    pub fn register(&mut self, body: u64) -> Result<(), PsCode> {
        if self.bodies.len() >= BODY_CAP {
            return Err(PsCode::ImpactOverflow);
        }
        let mut i = 0;
        while i < self.bodies.len() {
            if let Some(b) = self.bodies.get(i) {
                if *b == body {
                    return Ok(());
                }
            }
            i += 1;
        }
        self.bodies.push(body);
        Ok(())
    }

    /// 查体。
    pub fn has_body(&self, body: u64) -> bool {
        let mut i = 0;
        while i < self.bodies.len() {
            if let Some(b) = self.bodies.get(i) {
                if *b == body {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 熔断恢复窗。
    pub const fn fuse_clear_due(&self, tick: u64) -> bool {
        self.fused && tick.saturating_sub(self.fused_since) >= FUSE_COOLDOWN_TICKS
    }

    /// 隔离（滚动账；达熔断线触发熔断）。
    fn quarantine(&mut self, body: u64, kind: QuarKind, tick: u64) {
        let s = fnv1a_u64(FNV_OFFSET, body);
        let s = fnv1a_u64(s, kind.ordinal() as u64);
        let fp = fnv1a_u64(s, tick);
        let q = Quarantine {
            body,
            kind: kind.ordinal() as u8,
            tick,
            disp: Disposition::Pending.ordinal() as u8,
            fp,
        };
        if self.quarantines.len() < QUARANTINE_CAP {
            self.quarantines.push(q);
            self.q_len += 1;
        } else {
            self.quarantines[self.q_head] = q;
            self.q_head = (self.q_head + 1) % QUARANTINE_CAP;
            self.q_rolled += 1;
        }
        // tick 归零与熔断闸。
        if self.iso_tick != tick {
            self.iso_tick = tick;
            self.iso_this_tick = 0;
        }
        self.iso_this_tick += 1;
        if self.iso_this_tick >= FUSE_ISOLATIONS_PER_TICK && !self.fused {
            self.fused = true;
            self.fused_since = tick;
            self.fuse_count += 1;
        }
        self.last_tick = self.last_tick.max(tick);
    }

    /// 按时序读隔离。
    pub fn quarantine_at(&self, i: usize) -> Option<&Quarantine> {
        if i >= self.q_len {
            return None;
        }
        self.quarantines.get((self.q_head + i) % QUARANTINE_CAP)
    }

    /// 校验一物理量（防线主入口；f64 位模式载荷路径）。
    pub fn check_bits(&mut self, body: u64, _kind: usize, bits: u64, tick: u64) -> Result<(), PsCode> {
        if self.fused && !self.fuse_clear_due(tick) {
            return Err(PsCode::Fused);
        }
        if self.fused && self.fuse_clear_due(tick) {
            self.fused = false;
        }
        if !self.has_body(body) {
            return Err(PsCode::BodyUnknown);
        }
        if is_nan_pattern(bits) {
            self.nan_hits += 1;
            self.quarantine(body, QuarKind::Nan, tick);
            return Err(PsCode::NanDetected);
        }
        if is_inf_pattern(bits) {
            self.inf_hits += 1;
            self.quarantine(body, QuarKind::Inf, tick);
            return Err(PsCode::InfDetected);
        }
        // 位模式合法 → 走定点校验（上游换算后传 milli；此处只闸类别规则，
        // milli 由调用方给出——bits 路径在此只做 NaN/Inf 分类，量程走 check_milli）。
        let _ = tick;
        Ok(())
    }

    /// 校验一定点物理量（千分比；限幅器内置——量程越界限幅留痕不隔离，
    /// 注入类直接拒）。
    pub fn check_milli(&mut self, body: u64, kind: usize, milli: i64, tick: u64) -> Result<i64, PsCode> {
        if self.fused && !self.fuse_clear_due(tick) {
            return Err(PsCode::Fused);
        }
        if self.fused && self.fuse_clear_due(tick) {
            self.fused = false;
        }
        if !self.has_body(body) {
            return Err(PsCode::BodyUnknown);
        }
        if kind >= QUANTITY_NAMES.len() {
            self.inject_rejected += 1;
            self.quarantine(body, QuarKind::Injection, tick);
            return Err(PsCode::InjectionRejected);
        }
        match kind {
            3 | 4 => {
                // 注入闸（质量/恢复系数不可限幅——语义破坏直接拒）。
                if check_quantity(kind, milli).is_err() {
                    self.inject_rejected += 1;
                    self.quarantine(body, QuarKind::Injection, tick);
                    return Err(PsCode::InjectionRejected);
                }
                Ok(milli)
            }
            _ => {
                let lim = QUANTITY_LIMITS[kind];
                if milli > lim || milli < -lim {
                    self.clamped += 1;
                    // 限幅器：钳到量程（防线内的温柔路径）。
                    let cl = if milli > 0 { lim } else { -lim };
                    Ok(cl)
                } else {
                    Ok(milli)
                }
            }
        }
    }

    /// 影响范围评估（判据"隔离含影响范围评估"）：按共享约束伙伴表评估
    /// 波及面，产出影响面清单（判据"评估含影响面清单"）。
    pub fn assess_impact(&mut self, body: u64, partners: &[(u64, u8)], tick: u64) -> Result<Vec<ImpactRow>, PsCode> {
        if !self.has_body(body) {
            return Err(PsCode::BodyUnknown);
        }
        if partners.len() > IMPACT_CAP {
            return Err(PsCode::ImpactOverflow);
        }
        let mut rows: Vec<ImpactRow> = Vec::new();
        let mut i = 0;
        while i < partners.len() {
            if let Some((b, via)) = partners.get(i) {
                rows.push(ImpactRow {
                    body: *b,
                    via: *via,
                    disp: Disposition::Pending.ordinal() as u8,
                });
            }
            i += 1;
        }
        let _ = tick;
        Ok(rows)
    }

    /// 处置推进（判据"清单含处置状态"——状态机：Pending→Repaired/Evicted）。
    pub fn dispose(&mut self, body: u64, to: Disposition, tick: u64) -> Result<(), PsCode> {
        let mut i = 0;
        let mut hit = false;
        while i < self.q_len {
            let idx = (self.q_head + i) % QUARANTINE_CAP;
            let own = match self.quarantines.get(idx) {
                Some(q) => q.body == body && q.disp == Disposition::Pending.ordinal() as u8,
                None => false,
            };
            if own {
                if let Some(q) = self.quarantines.get_mut(idx) {
                    q.disp = to.ordinal() as u8;
                    q.fp = fnv1a_u64(q.fp, to.ordinal() as u64);
                }
                hit = true;
                break;
            }
            i += 1;
        }
        let _ = tick;
        if hit {
            Ok(())
        } else {
            Err(PsCode::BodyUnknown)
        }
    }

    /// 规则公开读屏（判据"规则公开"——量程/熔断线全可见）。
    pub fn read_screen(&self) -> String {
        let mut out = String::new();
        out.push_str("== 物理安全规则公开（0x55 段）==\n");
        out.push_str("[一] 量程（千分比定点）\n");
        let mut k = 0;
        while k < QUANTITY_NAMES.len() {
            out.push_str(QUANTITY_NAMES[k]);
            out.push_str(" <= ");
            push_i64(&mut out, QUANTITY_LIMITS[k]);
            out.push('\n');
            k += 1;
        }
        out.push_str("[二] 隔离与熔断\n");
        out.push_str("quarantined=");
        push_i64(&mut out, self.q_len as i64);
        out.push_str(" fuse_line=");
        push_i64(&mut out, FUSE_ISOLATIONS_PER_TICK as i64);
        out.push_str(" fused=");
        out.push_str(if self.fused { "是" } else { "否" });
        out.push_str(" fuse_count=");
        push_i64(&mut out, self.fuse_count as i64);
        out.push('\n');
        out.push_str("[三] 计数\n");
        out.push_str("nan=");
        push_i64(&mut out, self.nan_hits as i64);
        out.push_str(" inf=");
        push_i64(&mut out, self.inf_hits as i64);
        out.push_str(" inject=");
        push_i64(&mut out, self.inject_rejected as i64);
        out.push_str(" clamped=");
        push_i64(&mut out, self.clamped as i64);
        out.push('\n');
        out
    }

    /// 守恒对账：隔离账 ≤ 桶容量；熔断计数自洽。
    pub const fn reconcile(&self) -> bool {
        self.q_len <= QUARANTINE_CAP && (!self.fused || self.fuse_count >= 1)
    }
}

impl Default for PhysecEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// i64 十进制直写。
fn push_i64(out: &mut String, v: i64) {
    if v < 0 {
        out.push('-');
    }
    let mut v = if v < 0 { (v as i128).unsigned_abs() as u64 } else { v as u64 };
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        out.push(buf[j] as char);
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条可执行）
// ---------------------------------------------------------------------------

/// f64 位模式构造（测试面——用位运算拼，不引浮点）。
pub const fn f64_bits(sign: u64, exp: u64, frac: u64) -> u64 {
    (sign << 63) | (exp << 52) | frac
}

/// 自检判据（F6010 全判据 + 深化面）。
pub fn run_vead10_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();

    // ── 判据一：NaN 防线（位模式检出→隔离+告警）。
    let mut e = PhysecEngine::new();
    let _ = e.register(1);
    let nan = f64_bits(0, 0x7FF, 1);
    let r1 = e.check_bits(1, 0, nan, 1);
    out.push((
        "NaN 检出隔离告警",
        r1 == Err(PsCode::NanDetected) && e.nan_hits == 1 && e.quarantine_at(0).map(|q| q.body == 1).unwrap_or(false),
    ));

    // ── 判据二：Inf 检出。
    let inf = f64_bits(0, 0x7FF, 0);
    let r2 = e.check_bits(1, 0, inf, 2);
    out.push((
        "Inf 检出隔离",
        r2 == Err(PsCode::InfDetected) && e.inf_hits == 1 && e.quarantines.len() == 2,
    ));

    // ── 判据三：正常位模式直通。
    let normal = f64_bits(0, 1023, 0x8000_0000_0000); // 1.5
    out.push(("正常位模式直通", e.check_bits(1, 0, normal, 3).is_ok()));

    // ── 判据四：注入防护（质量≤0 / 恢复系数>1 拒）。
    let mut e2 = PhysecEngine::new();
    let _ = e2.register(5);
    out.push((
        "零质量注入拒",
        e2.check_milli(5, 3, 0, 1) == Err(PsCode::InjectionRejected) && e2.inject_rejected == 1,
    ));
    out.push((
        "恢复系数>1 注入拒",
        e2.check_milli(5, 4, 1001, 2) == Err(PsCode::InjectionRejected),
    ));
    out.push((
        "合法质量直通",
        e2.check_milli(5, 3, 500, 3).is_ok(),
    ));

    // ── 判据五：限幅器（量程越界→限幅留痕不隔离）。
    let cl = e2.check_milli(5, 1, VEL_LIMIT + 500, 4);
    out.push((
        "量程越界限幅",
        cl == Ok(VEL_LIMIT) && e2.clamped == 1 && e2.quarantines.is_empty(),
    ));
    out.push((
        "负向限幅对称",
        e2.check_milli(5, 0, -POS_LIMIT - 1, 5) == Ok(-POS_LIMIT),
    ));

    // ── 判据六：资源熔断（单 tick 隔离 16 体 → 熔断 → 冷却恢复）。
    let mut e3 = PhysecEngine::new();
    let mut b = 0u64;
    while b < FUSE_ISOLATIONS_PER_TICK as u64 {
        let _ = e3.register(b);
        let _ = e3.check_bits(b, 0, nan, 1);
        b += 1;
    }
    out.push((
        "达线熔断",
        e3.fused && e3.fuse_count == 1,
    ));
    out.push((
        "熔断窗内拒绝",
        e3.check_milli(0, 0, 1, 2) == Err(PsCode::Fused),
    ));
    out.push((
        "冷却窗恢复",
        e3.fuse_clear_due(1 + FUSE_COOLDOWN_TICKS),
    ));

    // ── 判据七：影响范围评估→清单→处置状态。
    let mut e4 = PhysecEngine::new();
    let _ = e4.register(10);
    let _ = e4.register(11);
    let _ = e4.register(12);
    let partners = [(11u64, 0u8), (12u64, 1u8)];
    let impact = e4.assess_impact(10, &partners, 1);
    match impact {
        Ok(rows) => {
            out.push(("影响面清单", rows.len() == 2 && rows.first().map(|r| r.via == 0).unwrap_or(false)));
        }
        Err(_) => out.push(("影响面清单", false)),
    }
    // 隔离后处置。
    let _ = e4.check_bits(10, 0, nan, 2);
    out.push((
        "处置 Pending→Repaired",
        e4.dispose(10, Disposition::Repaired, 3).is_ok()
            && e4.quarantine_at(0).map(|q| q.disp_enum() == Some(Disposition::Repaired)).unwrap_or(false),
    ));

    // ── 判据八：隔离账滚动。
    let mut e5 = PhysecEngine::new();
    let mut b2 = 0u64;
    while b2 < QUARANTINE_CAP as u64 + 4 {
        let _ = e5.register(b2);
        let _ = e5.check_bits(b2, 0, nan, 1);
        b2 += 1;
    }
    out.push((
        "隔离账滚动",
        e5.q_len == QUARANTINE_CAP && e5.q_rolled == 4,
    ));

    // ── 判据九：规则公开读屏。
    let s = e.read_screen();
    out.push((
        "规则公开读屏",
        s.contains("量程") && s.contains("熔断") && s.contains("规则公开"),
    ));

    // ── 判据十：未知体拒绝 + 守恒。
    out.push(("未知体拒绝", e2.check_milli(99, 0, 1, 9) == Err(PsCode::BodyUnknown)));
    out.push(("守恒对账", e.reconcile() && e2.reconcile() && e3.reconcile()));

    out
}
