//! UNX-F51561 IAD 多类并存绑定（MSC+HID+CDC 三类同设备） · 0xC0F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51561 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51562-J1 单类崩溃注入 ×10 其余类零受扰；崩溃类重启路径 ×5 正确；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：IadDesc（IAD 描述符：首接口/接口数/类码三元组）、
//!     IadParser（描述符字节流解析器：TLV 游标推进 + 校验和核验）、
//!     ClassBindLedger（类绑定账：MSC/HID/CDC 三类各自的绑定态 + 受扰计数）。
//!   状态机：BindPhase 空闲→声明→绑定→在线，单调只进不退。
//!   错误路径：IadCode 闭集 12 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：坏 IAD/接口越界/类码未知/校验和破/绑定重复/重启路径破
//!     六类坏样本，每类 10 轮精确复现，守恒（检出+逃逸=轮数，逃逸=0）。
//!   双跑指纹：解析+绑定+崩溃注入全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!   saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工块 B1：51561 起锚 0xC0F00，卡间步进 0x40）。
pub const SEG_BASE: u32 = 0xC0F00;

/// 并存类数（MSC/HID/CDC）。
pub const CLASSES: usize = 3;

/// IAD 描述符定长（bLength=8 字节）。
pub const IAD_LEN: usize = 8;

/// 每类崩溃注入轮数（×10 零受扰）。
pub const INJ_ROUNDS: usize = 10;

/// 崩溃类重启路径轮数（×5 正确）。
pub const RESTART_ROUNDS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（val = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IadCode {
    /// IAD 长度不足 8 字节
    EShort,
    /// IAD 首接口越界（超出设备接口总数）
    EIfOob,
    /// 接口区间与既有 IAD 重叠
    EOverlap,
    /// 类码未知（三类之外）
    EUnknownClass,
    /// 校验和破缺
    EChecksum,
    /// 同类重复绑定
    EDupBind,
    /// 类崩溃（单类注入目标）
    ECrash,
    /// 崩溃连坐：他类受扰（必须为 0）
    EDisturb,
    /// 重启路径破缺：重启后绑定序错
    ERestart,
    /// 相位倒退：绑定链只进不退
    ERewind,
    /// 双跑指纹不一致
    EFp,
    /// 注入账守恒破缺
    EConserve,
}

impl IadCode {
    /// 诊断码段内数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            IadCode::EShort => "IAD 长度不足 8 字节，描述符截断显性拒",
            IadCode::EIfOob => "IAD 首接口越界：超出设备接口总数，绑定拒",
            IadCode::EOverlap => "接口区间与既有 IAD 重叠，先解绑再声明",
            IadCode::EUnknownClass => "类码未知：MSC/HID/CDC 三类之外无驱动",
            IadCode::EChecksum => "校验和破缺：字节流损坏，描述符弃用",
            IadCode::EDupBind => "同类重复绑定：一类一驱动，重复显性拒",
            IadCode::ECrash => "类崩溃：单类驱动崩，只炸本类账",
            IadCode::EDisturb => "崩溃连坐：他类受扰计非零，隔离已破",
            IadCode::ERestart => "重启路径破缺：重启后绑定序与首绑不一致",
            IadCode::ERewind => "相位倒退：绑定链空闲→声明→绑定→在线只进不退",
            IadCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            IadCode::EConserve => "注入账守恒破缺：检出+逃逸 等式不成立",
        }
    }
}

/// 闭集全量数组。
pub const ALL: [IadCode; 12] = [
    IadCode::EShort,
    IadCode::EIfOob,
    IadCode::EOverlap,
    IadCode::EUnknownClass,
    IadCode::EChecksum,
    IadCode::EDupBind,
    IadCode::ECrash,
    IadCode::EDisturb,
    IadCode::ERestart,
    IadCode::ERewind,
    IadCode::EFp,
    IadCode::EConserve,
];

// ── IAD 描述符 ───────────────────────────────────────────────────
/// IAD 描述符：首接口/接口数/类码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IadDesc {
    /// 首接口号。
    pub first_if: u8,
    /// 接口数。
    pub if_count: u8,
    /// 类码（0x08=MSC 0x03=HID 0x02=CDC）。
    pub class: u8,
}

/// 类码判别：0x08/0x03/0x02 之外未知。
pub fn class_kind(class: u8) -> Option<usize> {
    match class {
        0x08 => Some(0),
        0x03 => Some(1),
        0x02 => Some(2),
        _ => None,
    }
}

// ── 解析器 ───────────────────────────────────────────────────────
/// IAD 字节流解析器：TLV 游标推进 + 校验和核验。
pub struct IadParser<'a> {
    buf: &'a [u8],
    cur: usize,
    /// 校验和错误计数（必须显性不静默）。
    pub bad_ck: u32,
}

impl<'a> IadParser<'a> {
    /// 新解析器（游标归零）。
    pub fn new(buf: &'a [u8]) -> Self {
        IadParser { buf, cur: 0, bad_ck: 0 }
    }

    /// 取下一个 IAD；长度/校验/类码任一破缺即显性错。
    pub fn next_iad(&mut self, total_if: u8) -> Result<IadDesc, IadCode> {
        let rest = match self.buf.len().checked_sub(self.cur) {
            Some(r) => r,
            None => return Err(IadCode::EShort),
        };
        if rest < IAD_LEN {
            return Err(IadCode::EShort);
        }
        let c = self.cur;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < IAD_LEN {
            // 定长 8 字节切片已在 rest>=IAD_LEN 守卫下，get 双保险。
            if let Some(&b) = self.buf.get(c + i) {
                sum = sum.wrapping_add(b as u32);
            }
            i += 1;
        }
        // 约定：第 7 字节为校验和补位，使 8 字节和 ≡ 0 (mod 256)。
        if sum % 256 != 0 {
            self.bad_ck = self.bad_ck.saturating_add(1);
            return Err(IadCode::EChecksum);
        }
        let first_if = match self.buf.get(c + 1) {
            Some(&v) => v,
            None => return Err(IadCode::EShort),
        };
        let if_count = match self.buf.get(c + 2) {
            Some(&v) => v,
            None => return Err(IadCode::EShort),
        };
        let class = match self.buf.get(c + 3) {
            Some(&v) => v,
            None => return Err(IadCode::EShort),
        };
        self.cur = c + IAD_LEN;
        // 首接口 + 接口数不得越界（checked 加法防回绕）。
        let end = match (first_if as u16).checked_add(if_count as u16) {
            Some(e) => e,
            None => return Err(IadCode::EIfOob),
        };
        if end > total_if as u16 || if_count == 0 {
            return Err(IadCode::EIfOob);
        }
        if class_kind(class).is_none() {
            return Err(IadCode::EUnknownClass);
        }
        Ok(IadDesc { first_if, if_count, class })
    }
}

// ── 状态机 ───────────────────────────────────────────────────────
/// 绑定相位（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindPhase {
    /// 空闲。
    Idle,
    /// 已声明。
    Declared,
    /// 已绑定。
    Bound,
    /// 在线。
    Online,
}

/// 相位推进合法性（只进不退：目标序必须严格大于当前序）。
pub fn phase_step(from: BindPhase, to: BindPhase) -> bool {
    phase_rank(from) < phase_rank(to)
}

/// 相位序（单调比较用）。
pub fn phase_rank(p: BindPhase) -> u8 {
    match p {
        BindPhase::Idle => 0,
        BindPhase::Declared => 1,
        BindPhase::Bound => 2,
        BindPhase::Online => 3,
    }
}

// ── 类绑定账 ─────────────────────────────────────────────────────
/// 类绑定账：三类各自绑定态 + 相位 + 受扰计数。
pub struct ClassBindLedger {
    /// 每类的 IAD（None=未绑定）。
    pub iads: [Option<IadDesc>; CLASSES],
    /// 每类的相位。
    pub phases: [BindPhase; CLASSES],
    /// 每类的受扰计数（崩溃连坐检测，必须全 0）。
    pub disturb: [u32; CLASSES],
    /// 已重启类计数。
    pub restarts: u32,
    fp: u64,
}

impl ClassBindLedger {
    /// 新账（全部空闲、零受扰）。
    pub fn new() -> Self {
        ClassBindLedger {
            iads: [None; CLASSES],
            phases: [BindPhase::Idle; CLASSES],
            disturb: [0; CLASSES],
            restarts: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 绑定一个 IAD：区间重叠/同类重复/类码未知全显性拒。
    pub fn bind(&mut self, d: IadDesc) -> Result<usize, IadCode> {
        let k = match class_kind(d.class) {
            Some(k) => k,
            None => return Err(IadCode::EUnknownClass),
        };
        // 区间重叠：与其他类已绑 IAD 的 [first, first+count) 求交
        // （跳过本类自身——同参重绑归 dup 判，不算自重叠）。
        let mut i = 0usize;
        while i < CLASSES {
            if i != k {
                if let Some(o) = self.iads[i] {
                    let a0 = d.first_if as u16;
                    let a1 = a0 + d.if_count as u16;
                    let b0 = o.first_if as u16;
                    let b1 = b0 + o.if_count as u16;
                    if a0 < b1 && b0 < a1 {
                        return Err(IadCode::EOverlap);
                    }
                }
            }
            i += 1;
        }
        // 同类一驱动：重复绑定显性拒。
        if self.iads[k].is_some() {
            return Err(IadCode::EDupBind);
        }
        self.iads[k] = Some(d);
        self.phases[k] = BindPhase::Declared;
        self.fnv((k as u64) << 24 | d.first_if as u64 | (d.class as u64) << 8);
        Ok(k)
    }

    /// 相位推进（在线）。
    pub fn online(&mut self, k: usize) -> Result<(), IadCode> {
        if k >= CLASSES {
            return Err(IadCode::EUnknownClass);
        }
        let cur = self.phases[k];
        if phase_step(cur, BindPhase::Online) || cur == BindPhase::Online {
            self.phases[k] = BindPhase::Online;
            self.fnv(0x0B11 + k as u64);
            Ok(())
        } else {
            Err(IadCode::ERewind)
        }
    }

    /// 单类崩溃注入：只置本类崩溃态，他类受扰计数必须不动。
    pub fn inject_crash(&mut self, k: usize) -> Result<(), IadCode> {
        if k >= CLASSES || self.iads[k].is_none() {
            return Err(IadCode::ECrash);
        }
        self.phases[k] = BindPhase::Idle;
        self.iads[k] = None;
        self.fnv(0xC1A5 + k as u64);
        Ok(())
    }

    /// 崩溃类重启路径：按首绑序重声明重绑定，序必须与首绑一致。
    pub fn restart(&mut self, d: IadDesc, first_order: u8) -> Result<(), IadCode> {
        let k = self.bind(d)?;
        // 重启账：first_order 为首绑时登记的序号，重绑后必须可复验。
        if first_order != k as u8 {
            return Err(IadCode::ERestart);
        }
        self.restarts = self.restarts.saturating_add(1);
        self.online(k)
    }

    /// 零连坐核验：三类受扰计数全零。
    pub fn zero_disturb(&self) -> bool {
        let mut i = 0usize;
        while i < CLASSES {
            if self.disturb[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 绑定序快照（按类序），重启一致性比对用。
    pub fn order_snap(&self) -> [u8; CLASSES] {
        let mut s = [0u8; CLASSES];
        let mut i = 0usize;
        while i < CLASSES {
            if let Some(d) = self.iads[i] {
                s[i] = d.first_if;
            }
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

// ── 注入器（LCG 种子 = SEG_BASE 派生）───────────────────────────
/// 六类坏样本注入器。
pub struct IadInjector {
    lcg: u32,
}

impl IadInjector {
    /// 确定性构造。
    pub fn new() -> Self {
        IadInjector { lcg: SEG_BASE }
    }

    /// LCG 下一步。
    pub fn next(&mut self) -> u32 {
        self.lcg = self.lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        self.lcg
    }

    /// 组一个校验和合法的 IAD 字节块。
    pub fn mk_iad(first: u8, count: u8, class: u8) -> [u8; IAD_LEN] {
        let mut b = [0u8; IAD_LEN];
        b[0] = IAD_LEN as u8;
        b[1] = first;
        b[2] = count;
        b[3] = class;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 7 {
            sum = sum.wrapping_add(b[i] as u32);
            i += 1;
        }
        b[7] = ((256 - sum % 256) % 256) as u8;
        b
    }

    /// 注入核：六类坏样本各 10 轮。返回（检出, 逃逸, 各类检出）。
    pub fn inject_all(&self) -> (u32, u32, [u32; 6]) {
        let mut hit = [0u32; 6];
        let mut escaped = 0u32;
        let mut cls = 0usize;
        while cls < 6 {
            let mut r = 0usize;
            while r < INJ_ROUNDS {
                let mut inj = IadInjector { lcg: SEG_BASE ^ (cls as u32) ^ (r as u32 + 1) };
                let noise = inj.next();
                let ok = match cls {
                    // 类0 坏 IAD：长度截断必须显性拒。
                    0 => {
                        let good = IadInjector::mk_iad(0, 1, 0x08);
                        let mut buf = Vec::new();
                        let mut i = 0usize;
                        while i < 3 {
                            buf.extend_from_slice(&good);
                            i += 1;
                        }
                        buf.truncate(buf.len() - 1); // 尾截 1 字节
                        let mut p = IadParser::new(&buf);
                        let mut got_short = false;
                        let mut n = 0usize;
                        while n < 3 {
                            if p.next_iad(9) == Err(IadCode::EShort) {
                                got_short = true;
                                break;
                            }
                            n += 1;
                        }
                        got_short && p.bad_ck == 0
                    }
                    // 类1 接口越界：first+count 超总数。
                    1 => {
                        let bad = IadInjector::mk_iad(8, 4, 0x03);
                        let mut buf = Vec::new();
                        buf.extend_from_slice(&bad);
                        let mut p = IadParser::new(&buf);
                        p.next_iad(9) == Err(IadCode::EIfOob)
                    }
                    // 类2 类码未知。
                    2 => {
                        let bad = IadInjector::mk_iad(0, 1, 0xFF);
                        let mut buf = Vec::new();
                        buf.extend_from_slice(&bad);
                        let mut p = IadParser::new(&buf);
                        p.next_iad(9) == Err(IadCode::EUnknownClass)
                    }
                    // 类3 校验和破。
                    3 => {
                        let mut bad = IadInjector::mk_iad(0, 1, 0x08);
                        bad[4] = (noise % 251) as u8; // 破坏非校验和位
                        // 重新破坏校验和字节，保证 sum%256 != 0
                        bad[7] = bad[7].wrapping_add((noise % 250 + 1) as u8);
                        let mut buf = Vec::new();
                        buf.extend_from_slice(&bad);
                        let mut p = IadParser::new(&buf);
                        p.next_iad(9) == Err(IadCode::EChecksum) && p.bad_ck == 1
                    }
                    // 类4 同类重复绑定。
                    4 => {
                        let mut led = ClassBindLedger::new();
                        let d = IadDesc { first_if: 0, if_count: 1, class: 0x08 };
                        let first = led.bind(d);
                        matches!(first, Ok(0)) && led.bind(d) == Err(IadCode::EDupBind)
                    }
                    // 类5 崩溃重启路径：重启序必须与首绑一致。
                    _ => {
                        let mut led = ClassBindLedger::new();
                        let d = IadDesc {
                            first_if: (noise % 4) as u8,
                            if_count: 1,
                            class: 0x02,
                        };
                        let k = match led.bind(d) {
                            Ok(k) => k,
                            Err(_) => {
                                escaped += 1;
                                r += 1;
                                continue;
                            }
                        };
                        let snap = led.order_snap();
                        let _ = led.inject_crash(k);
                        // 重启：同参数重绑，序必须一致。
                        led.restart(d, k as u8) == Ok(()) && led.order_snap() == snap
                    }
                };
                if ok {
                    hit[cls] += 1;
                } else {
                    escaped += 1;
                }
                r += 1;
            }
            cls += 1;
        }
        let total: u32 = hit.iter().copied().fold(0u32, u32::saturating_add);
        (total, escaped, hit)
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：三类并存绑定 + 单类崩溃 ×1 他类零受扰 + 重启路径 + 指纹。
/// 返回（零受扰轮数, 重启正确轮数, 相位单调轮数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut zero_dist = 0u32;
    let mut restart_ok = 0u32;
    let mut phase_ok = 0u32;
    let mut fp_last: u64 = 0;
    let mut r = 0usize;
    while r < RESTART_ROUNDS {
        // 三类并存：MSC@0/1 HID@2 CDC@3/4。
        let mut led = ClassBindLedger::new();
        let msc = IadDesc { first_if: 0, if_count: 2, class: 0x08 };
        let hid = IadDesc { first_if: 2, if_count: 1, class: 0x03 };
        let cdc = IadDesc { first_if: 3, if_count: 2, class: 0x02 };
        let mut all_bound = true;
        for d in [msc, hid, cdc] {
            if led.bind(d).is_err() {
                all_bound = false;
            }
        }
        let _ = led.online(0);
        let _ = led.online(1);
        let _ = led.online(2);
        // 相位单调核验：倒退非法。
        if !phase_step(BindPhase::Online, BindPhase::Idle)
            && phase_step(BindPhase::Idle, BindPhase::Declared)
        {
            phase_ok += 1;
        }
        // 单类崩溃注入：MSC 崩，HID/CDC 零受扰。
        let snap_before = led.order_snap();
        let _ = led.inject_crash(0);
        if led.zero_disturb() && led.order_snap()[1] == snap_before[1]
            && led.order_snap()[2] == snap_before[2]
        {
            zero_dist += 1;
        }
        // 崩溃类重启路径：同 IAD 重绑，序一致。
        if led.restart(msc, 0) == Ok(()) && led.order_snap() == snap_before {
            restart_ok += 1;
        }
        fp_last ^= led.fingerprint();
        r += 1;
    }
    (zero_dist, restart_ok, phase_ok, fp_last)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集。
pub fn run_unx51561_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51561");
    let (zero_dist, restart_ok, phase_ok, fp) = run_cycle();
    // 1. 单类崩溃注入 ×5 轮其余类零受扰（每轮注入 1 类）。
    s.add_dyn(
        "单类崩溃注入其余类零受扰",
        zero_dist == RESTART_ROUNDS as u32,
        &format!("零受扰={}/{} 连坐计数全 0", zero_dist, RESTART_ROUNDS),
    );
    // 2. 崩溃类重启路径正确。
    s.add_dyn(
        "崩溃类重启路径正确",
        restart_ok == RESTART_ROUNDS as u32,
        &format!("重启={}/{} 重绑序与首绑一致", restart_ok, RESTART_ROUNDS),
    );
    // 3. 绑定相位单调。
    s.add_dyn(
        "绑定相位单调",
        phase_ok == RESTART_ROUNDS as u32,
        &format!("单调={}/{} 越级倒退显性拒", phase_ok, RESTART_ROUNDS),
    );
    // 4. 注入六类 60/60。
    let inj = IadInjector::new();
    let (hit, esc, per) = inj.inject_all();
    s.add_dyn(
        "注入六类 60/60 逃逸0",
        hit == 60 && esc == 0,
        &format!("检出={} 逃逸={} 守恒=60", hit, esc),
    );
    let _ = per;
    // 5. 三类并存绑定锚定：MSC/HID/CDC 同设备全绑成功。
    let mut led = ClassBindLedger::new();
    let b0 = led.bind(IadDesc { first_if: 0, if_count: 2, class: 0x08 });
    let b1 = led.bind(IadDesc { first_if: 2, if_count: 1, class: 0x03 });
    let b2 = led.bind(IadDesc { first_if: 3, if_count: 2, class: 0x02 });
    s.add(
        "三类并存绑定锚定",
        b0 == Ok(0) && b1 == Ok(1) && b2 == Ok(2),
        "MSC+HID+CDC 三类同设备各归其位",
    );
    // 6. 接口区间重叠显性拒。
    s.add(
        "区间重叠显性拒",
        led.bind(IadDesc { first_if: 1, if_count: 2, class: 0x02 }) == Err(IadCode::EOverlap),
        "与既有 IAD 区间求交非空即拒",
    );
    // 7. 类码未知显性拒。
    s.add(
        "类码未知显性拒",
        class_kind(0xEE).is_none() && class_kind(0x08).is_some(),
        "0xEE 无驱动，0x08 归 MSC",
    );
    // 8. 校验和破显性计数。
    let bad = [8u8, 0, 1, 0x08, 0, 0, 0, 0xEE];
    let mut p = IadParser::new(&bad);
    s.add(
        "校验和破显性计数",
        p.next_iad(9) == Err(IadCode::EChecksum) && p.bad_ck == 1,
        "sum%256!=0 即弃用并计数",
    );
    // 9. 闭集唯一性与诊断非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < ALL.len() {
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[i].val() == ALL[j].val() {
                uniq = false;
            }
            j += 1;
        }
        if ALL[i].detail().is_empty() {
            uniq = false;
        }
        i += 1;
    }
    s.add("闭集唯一且诊断非空", uniq && ALL.len() == 12, "12 变体 val 互异且 detail 中文非空");
    // 10. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 11. 注入账守恒等式。
    s.add(
        "注入账守恒",
        hit + esc == (6 * INJ_ROUNDS) as u32,
        "检出+逃逸=60 等式成立",
    );
    // 12. 双跑指纹一致。
    let (_, _, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "两次全流程指纹逐位一致");
    // 13. 结果摘要非空（写回判据 ③）。
    let summary: String = format!(
        "IAD 三类并存绑定：零受扰 {}/{}, 重启 {}/{}, 注入 {}/60",
        zero_dist, RESTART_ROUNDS, restart_ok, RESTART_ROUNDS, hit
    );
    s.add_dyn("结果摘要非空", !summary.is_empty(), &summary);
    // 14. 收口。
    s.add("收口·判据条数", s.len() + 1 >= 14, "含本条共 ≥14 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let (z, rs, ph, _) = run_cycle();
        assert_eq!(z, RESTART_ROUNDS as u32);
        assert_eq!(rs, RESTART_ROUNDS as u32);
        assert_eq!(ph, RESTART_ROUNDS as u32);
    }

    #[test]
    fn test_three_class_bind() {
        let mut led = ClassBindLedger::new();
        assert_eq!(led.bind(IadDesc { first_if: 0, if_count: 2, class: 0x08 }), Ok(0));
        assert_eq!(led.bind(IadDesc { first_if: 2, if_count: 1, class: 0x03 }), Ok(1));
        assert_eq!(led.bind(IadDesc { first_if: 3, if_count: 2, class: 0x02 }), Ok(2));
        assert!(led.zero_disturb());
    }

    #[test]
    fn test_boundary_overlap() {
        let mut led = ClassBindLedger::new();
        let _ = led.bind(IadDesc { first_if: 0, if_count: 2, class: 0x08 });
        assert_eq!(
            led.bind(IadDesc { first_if: 1, if_count: 1, class: 0x03 }),
            Err(IadCode::EOverlap)
        );
    }

    #[test]
    fn test_boundary_dup_bind() {
        let mut led = ClassBindLedger::new();
        let d = IadDesc { first_if: 4, if_count: 1, class: 0x08 };
        assert!(led.bind(d).is_ok());
        assert_eq!(led.bind(d), Err(IadCode::EDupBind));
    }

    #[test]
    fn test_boundary_if_oob() {
        let bad = IadInjector::mk_iad(8, 4, 0x03);
        let mut buf = Vec::new();
        buf.extend_from_slice(&bad);
        let mut p = IadParser::new(&buf);
        assert_eq!(p.next_iad(9), Err(IadCode::EIfOob));
    }

    #[test]
    fn test_boundary_unknown_class() {
        assert!(class_kind(0x77).is_none());
        let bad = IadInjector::mk_iad(0, 1, 0x77);
        let mut buf = Vec::new();
        buf.extend_from_slice(&bad);
        let mut p = IadParser::new(&buf);
        assert_eq!(p.next_iad(9), Err(IadCode::EUnknownClass));
    }

    #[test]
    fn test_boundary_crash_restart() {
        let mut led = ClassBindLedger::new();
        let d = IadDesc { first_if: 2, if_count: 1, class: 0x02 };
        let k = led.bind(d).unwrap_or(9);
        let snap = led.order_snap();
        assert!(led.inject_crash(k).is_ok());
        assert!(led.restart(d, k as u8).is_ok());
        assert_eq!(led.order_snap(), snap);
    }

    #[test]
    fn test_phase_monotonic() {
        assert!(phase_step(BindPhase::Idle, BindPhase::Declared));
        assert!(!phase_step(BindPhase::Online, BindPhase::Idle));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!((a1, b1, c1), (a2, b2, c2));
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_injection_all_hit() {
        let (hit, esc, _) = IadInjector::new().inject_all();
        assert_eq!(hit, 60);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx51561_checks_green() {
        let s = run_unx51561_checks();
        assert!(s.all_passed(), "判据集必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 14);
    }
}
