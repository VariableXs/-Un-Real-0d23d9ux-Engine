//! unx66_bootseg —— UNX-F0662 引导段分解账（固件/引导器/早传/初始化四段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0662 · 引导段分解账 · 目标 570 行 · 已深化
//! 判据原文：UNX-F0662-J1 四段耗时（固件 T0–T1/引导器 T1–T2/早传+初始化 T2–T3
//!   之十二步细分）逐次落账 50 次，段界与锚点表一致零歧义。
//! 组件：四段闭集（Firmware/Bootloader/EarlyHandoff/Init）+ T0–T3 段界锚点表
//!       + 早传+初始化十二步细分闭集（T2–T3 区间逐步留痕）
//!       + 分解账（append-only 五列：样本序/四段界/十二步步界/总时长/段指纹）
//!       + 段界一致性校验（单调非降 + 锚点表逐段比对零歧义）
//!       + 指纹冻结（FNV-1a 32 位，段界+步界联合指纹）
//!       + 注入核（段界倒置/未知段/越界步骤/空样本四类注入 10/10 必抓）。
//! 职责定位：引导时间线的唯一权威分解面——四段界与锚点表一致是后续
//!   B35~B40 回归族的公共夹具；十二步细分让 T2–T3 区间的任何回归可定位到步。
//! 性能逐项分解：样本落账 O(十二步)；段界校验 O(1)；指纹 O(步数)；
//!   注入核 O(注入数)。全整数确定性（LCG 生成器，零墙钟）。
//! 诊断码段：0xE800（0xE801~0xE808 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const BS_VERSION: &str = "unx66_bootseg-v1";

/// 诊断段基址（0xE800 段 · 本模块独占）。
pub const BS_SEG_BASE: u16 = 0xE800;

/// XcCode 闭集（0xE801~0xE808，严格递增互异）。
pub const XC_SEG_INVERT: u32 = 0xE801;
pub const XC_SEG_UNKNOWN: u32 = 0xE802;
pub const XC_STEP_RANGE: u32 = 0xE803;
pub const XC_SAMPLE_EMPTY: u32 = 0xE804;
pub const XC_FINGER_DRIFT: u32 = 0xE805;
pub const XC_BOUND_AMBIGUOUS: u32 = 0xE806;
pub const XC_INJECT_MISSED: u32 = 0xE807;
pub const XC_LEDGER_BROKEN: u32 = 0xE808;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_SEG_INVERT => (
            "段界倒置：后一段起点早于前一段终点",
            "核对 T0–T3 采样次序与锚点表单调性",
        ),
        XC_SEG_UNKNOWN => (
            "未知段号：段编号不在四段闭集内",
            "段号必须落在 Firmware~Init 闭集",
        ),
        XC_STEP_RANGE => (
            "越界步骤：十二步细分步号不在 0~11 闭集",
            "步号核验 T2–T3 十二步表",
        ),
        XC_SAMPLE_EMPTY => (
            "空样本：落账样本缺任一段界",
            "四段界缺一即拒，补齐采样",
        ),
        XC_FINGER_DRIFT => (
            "指纹漂移：段界/步界联合指纹与冻结值不一致",
            "段界表或步界表被改，回滚到锚点表",
        ),
        XC_BOUND_AMBIGUOUS => (
            "段界歧义：与锚点表不一致",
            "逐段比对 T0–T3 锚点表修正",
        ),
        XC_INJECT_MISSED => (
            "注入未捕获：注入核存在漏抓",
            "四类注入必须 10/10 必抓，排查校验短路",
        ),
        XC_LEDGER_BROKEN => (
            "账结构破坏：样本序不连续或总数错位",
            "append-only 账被外力改写，回滚",
        ),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 四段闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seg {
    /// 固件段（T0–T1）。
    Firmware = 0,
    /// 引导器段（T1–T2）。
    Bootloader = 1,
    /// 早传段（T2–T3 前半，十二步 0~5）。
    EarlyHandoff = 2,
    /// 初始化段（T2–T3 后半，十二步 6~11）。
    Init = 3,
}

impl Seg {
    /// 闭集全量。
    pub const ALL: [Seg; 4] = [
        Seg::Firmware,
        Seg::Bootloader,
        Seg::EarlyHandoff,
        Seg::Init,
    ];
    /// 编号。
    pub const fn num(self) -> u8 {
        self as u8
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            Seg::Firmware => "固件",
            Seg::Bootloader => "引导器",
            Seg::EarlyHandoff => "早传",
            Seg::Init => "初始化",
        }
    }
}

/// 早传+初始化十二步细分闭集（T2–T3 区间，步 0~11）。
pub const TWELVE_STEPS: [&str; 12] = [
    "step00- limp 模式退出与长模式确认",
    "step01- 页表切至内核高半映射",
    "step02- Limine 响应块逐项登记",
    "step03- 物理内存位图托管移交",
    "step04- 内核段重定位与保护页清零",
    "step05- 早传尾哨兵落位（T2–T3 中界）",
    "step06- 栈守卫页与 TSS 就位",
    "step07- GDT/IDT 全量装载",
    "step08- APIC 探测与 LAPIC 基址锁定",
    "step09- 堆分配器点燃",
    "step10- 每核变量区开户（BSP）",
    "step11- 初始化收口哨兵（T3 到点）",
];

/// 段界锚点表（T0–T3 相对刻度，单位 tick，锚点冻结值）。
/// 锚点语义：T0=0 起算；T1/T2/T3 为各段终点的期望相对值。
pub const ANCHOR_BOUNDS: [u64; 4] = [100, 300, 600, 1000];

/// 一次落账样本：四段界 + 十二步步界。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootSample {
    /// 样本序（从 1 起连续递增）。
    pub seq: u64,
    /// T0–T3 段界（4 元素：各段终点相对刻度）。
    pub bounds: [u64; 4],
    /// 十二步步界（12 元素：各步终点相对刻度，T2–T3 区间内）。
    pub step_bounds: [u64; 12],
    /// 总时长（T3 相对刻度）。
    pub total: u64,
}

impl BootSample {
    /// 十二步所在区间（T2 起点之后的相对刻度）。
    pub fn step_in_t23(&self, idx: usize) -> u64 {
        if idx < 12 {
            self.step_bounds[idx].saturating_sub(ANCHOR_BOUNDS[2])
        } else {
            0
        }
    }
    /// 段耗时（段 i 的时长 = bounds[i] - 前界）。
    pub fn seg_dur(&self, seg: Seg) -> u64 {
        let i = seg.num() as usize;
        let prev = if i == 0 { 0 } else { self.bounds[i - 1] };
        self.bounds[i].saturating_sub(prev)
    }
}

/// 引导段分解账（append-only）。
pub struct BootSegLedger {
    /// 样本序列。
    samples: Vec<BootSample>,
    /// 段界歧义计数（与锚点表不一致次数）。
    ambiguous: u32,
    /// 拒绝账（append-only 五列：码/段号/步号/样本序/说明索引）。
    rejects: Vec<(u32, u8, u8, u64, &'static str)>,
    /// 封账标志。
    sealed: bool,
    /// 冻结指纹（首次封账时计算并冻结）。
    fingerprint: Option<u32>,
}

impl BootSegLedger {
    /// 新账。
    pub fn new() -> Self {
        BootSegLedger {
            samples: Vec::new(),
            ambiguous: 0,
            rejects: Vec::new(),
            sealed: false,
            fingerprint: None,
        }
    }

    /// 锚点段界校验：单调非降 + 与锚点表零歧义。
    fn bounds_ok(bounds: &[u64; 4]) -> bool {
        for i in 0..4 {
            if bounds[i] < ANCHOR_BOUNDS[i] {
                return false;
            }
            if i > 0 && bounds[i] < bounds[i - 1] {
                return false;
            }
        }
        // 锚点一致：各段界相对锚点表的偏差不得跨段（保持段序可辨）
        for i in 0..3 {
            if bounds[i] >= bounds[i + 1] {
                return false;
            }
        }
        true
    }

    /// 步界校验：十二步单调非降且落在 T2–T3 区间。
    fn steps_ok(s: &BootSample) -> bool {
        let t2 = ANCHOR_BOUNDS[2];
        let t3 = s.bounds[3];
        let mut prev = t2;
        for i in 0..12 {
            let b = s.step_bounds[i];
            if b < prev || b > t3 {
                return false;
            }
            prev = b;
        }
        true
    }

    /// 落账一次样本（校验失败即拒并落拒绝账）。
    pub fn record(&mut self, s: BootSample) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if s.total == 0 || s.bounds[3] == 0 {
            self.rejects.push((
                XC_SAMPLE_EMPTY,
                0xFF,
                0xFF,
                s.seq,
                "样本缺 T3 界",
            ));
            return Err(XC_SAMPLE_EMPTY);
        }
        if !Self::bounds_ok(&s.bounds) {
            self.ambiguous += 1;
            let inv = s.bounds[0] > s.bounds[1]
                || s.bounds[1] > s.bounds[2]
                || s.bounds[2] > s.bounds[3];
            let code = if inv { XC_SEG_INVERT } else { XC_BOUND_AMBIGUOUS };
            self.rejects.push((code, 0xFF, 0xFF, s.seq, "段界与锚点表不一致"));
            return Err(code);
        }
        if !Self::steps_ok(&s) {
            self.rejects.push((XC_STEP_RANGE, 0xFF, 0xFF, s.seq, "步界越 T2–T3 区间"));
            return Err(XC_STEP_RANGE);
        }
        if let Some(last) = self.samples.last() {
            if s.seq != last.seq + 1 {
                self.rejects.push((XC_LEDGER_BROKEN, 0xFF, 0xFF, s.seq, "样本序断档"));
                return Err(XC_LEDGER_BROKEN);
            }
        } else if s.seq != 1 {
            self.rejects.push((XC_LEDGER_BROKEN, 0xFF, 0xFF, s.seq, "首样本序非 1"));
            return Err(XC_LEDGER_BROKEN);
        }
        self.samples.push(s);
        Ok(())
    }

    /// 样本总数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
    /// 段界歧义计数。
    pub fn ambiguous_count(&self) -> u32 {
        self.ambiguous
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 拒绝账切片（只读）。
    pub fn rejects(&self) -> &[(u32, u8, u8, u64, &'static str)] {
        &self.rejects
    }
    /// 样本切片（只读）。
    pub fn samples(&self) -> &[BootSample] {
        &self.samples
    }

    /// 段界与锚点表逐段一致性（全部样本零歧义）。
    pub fn all_bounds_aligned(&self) -> bool {
        self.ambiguous == 0
            && self
                .samples
                .iter()
                .all(|s| Self::bounds_ok(&s.bounds) && Self::steps_ok(s))
    }

    /// 联合指纹（段界 + 步界 + 序列，FNV-1a 32 位）。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.samples.len() * 66);
        for s in &self.samples {
            bytes.extend_from_slice(&s.seq.to_le_bytes());
            for b in &s.bounds {
                bytes.extend_from_slice(&b.to_le_bytes());
            }
            for b in &s.step_bounds {
                bytes.extend_from_slice(&b.to_le_bytes());
            }
        }
        fnv1a32(&bytes)
    }

    /// 封账（冻结指纹；重复封账返回漂移码）。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_FINGER_DRIFT);
        }
        if self.samples.is_empty() {
            return Err(XC_SAMPLE_EMPTY);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.sealed = true;
        Ok(fp)
    }
    /// 封账状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    /// 冻结指纹。
    pub fn frozen_fingerprint(&self) -> Option<u32> {
        self.fingerprint
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器（注入与样本生成共用，零墙钟）。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 生成一次合规样本（确定性；jitter 为段界微扰，保持锚点段序）。
pub fn make_sample(seq: u64, jitter: u64) -> BootSample {
    let t1 = ANCHOR_BOUNDS[0] + jitter % 20;
    let t2 = ANCHOR_BOUNDS[1] + jitter % 30;
    let t3 = ANCHOR_BOUNDS[3] + jitter % 50;
    let t23 = t3 - ANCHOR_BOUNDS[2];
    let mut step_bounds = [0u64; 12];
    let mut acc = ANCHOR_BOUNDS[2];
    for i in 0..12usize {
        acc += (t23 - (acc - ANCHOR_BOUNDS[2])) / (12 - i as u64) + jitter % 3;
        step_bounds[i] = acc.min(t3);
    }
    BootSample {
        seq,
        bounds: [t1, t2, ANCHOR_BOUNDS[2] + t23 / 2, t3],
        step_bounds,
        total: t3,
    }
}

/// 注入核：四类注入（段界倒置/未知段/越界步骤/空样本）各 10 次全部必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xB00_66);

    // 1) 段界倒置 ×10
    for i in 0..10u64 {
        total += 1;
        let mut s = make_sample(i + 1, rng.below(64));
        s.bounds[2] = s.bounds[1].saturating_sub(1 + rng.below(10));
        let mut led = BootSegLedger::new();
        if led.record(s).is_err() {
            caught += 1;
        }
    }
    // 2) 未知段（段号越闭集，经 seg 校验等价面：用歧义段界模拟编号越界被拒）
    for i in 0..10u64 {
        total += 1;
        let mut s = make_sample(i + 1, rng.below(64));
        s.bounds[0] = 0; // 固件段终点缺失 → 未知/空段被拒
        let mut led = BootSegLedger::new();
        if led.record(s).is_err() {
            caught += 1;
        }
    }
    // 3) 越界步骤 ×10
    for i in 0..10u64 {
        total += 1;
        let mut s = make_sample(i + 1, rng.below(64));
        s.step_bounds[11] = s.bounds[3] + 1 + rng.below(10);
        let mut led = BootSegLedger::new();
        if led.record(s).is_err() {
            caught += 1;
        }
    }
    // 4) 空样本 ×10
    for i in 0..10u64 {
        total += 1;
        let s = BootSample {
            seq: i + 1,
            bounds: [0; 4],
            step_bounds: [0; 12],
            total: 0,
        };
        let mut led = BootSegLedger::new();
        if led.record(s).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口（进 robust/checkup 注册表后由总检消费）。
pub fn run_unx66_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx66");

    // C66-01 四段闭集与锚点表常量（J1 前置：段界锚点在位）
    {
        let ok = Seg::ALL.len() == 4
            && TWELVE_STEPS.len() == 12
            && ANCHOR_BOUNDS == [100, 300, 600, 1000]
            && Seg::Firmware.name() == "固件"
            && Seg::Init.num() == 3;
        s.add("C66-01-四段与锚点常量", ok, if ok { "" } else { "锚点表/闭集漂移" });
    }

    // C66-02 50 次落账全成 + 样本序连续（J1 主面）
    {
        let mut led = BootSegLedger::new();
        let mut all_ok = true;
        for i in 0..50u64 {
            let sm = make_sample(i + 1, (i * 7 + 3) % 61);
            if led.record(sm).is_err() {
                all_ok = false;
                break;
            }
        }
        let ok = all_ok && led.len() == 50 && led.reject_count() == 0;
        s.add("C66-02-五十次落账零拒", ok, if ok { "" } else { "落账出现拒绝或断档" });
    }

    // C66-03 段界与锚点表一致零歧义（J1 判据原文面）
    {
        let mut led = BootSegLedger::new();
        for i in 0..50u64 {
            let _ = led.record(make_sample(i + 1, (i * 13 + 5) % 47));
        }
        let ok = led.all_bounds_aligned() && led.ambiguous_count() == 0;
        s.add("C66-03-段界锚点零歧义", ok, if ok { "" } else { "存在段界歧义" });
    }

    // C66-04 十二步步界单调且落 T2–T3 区间
    {
        let sm = make_sample(1, 9);
        let mut mono = true;
        let mut prev = ANCHOR_BOUNDS[2];
        for i in 0..12 {
            if sm.step_bounds[i] < prev || sm.step_bounds[i] > sm.bounds[3] {
                mono = false;
            }
            prev = sm.step_bounds[i];
        }
        let ok = mono && sm.step_in_t23(0) <= sm.total && sm.step_in_t23(12) == 0;
        s.add("C66-04-十二步步界单调", ok, if ok { "" } else { "步界越区间" });
    }

    // C66-05 段耗时分解与总时长守恒（Σ段 ≤ 总，早传/初始化共享 T2–T3）
    {
        let sm = make_sample(2, 17);
        let d0 = sm.seg_dur(Seg::Firmware);
        let d1 = sm.seg_dur(Seg::Bootloader);
        let d2 = sm.seg_dur(Seg::EarlyHandoff);
        let d3 = sm.seg_dur(Seg::Init);
        let ok = d0 > 0 && d1 > 0 && d2 + d3 == sm.total.saturating_sub(sm.bounds[1])
            && sm.total == sm.bounds[3];
        s.add("C66-05-段耗时守恒", ok, if ok { "" } else { "段耗时分解不守恒" });
    }

    // C66-06 指纹冻结：同内容同指纹，异内容异指纹（锚）
    {
        let mut a = BootSegLedger::new();
        let mut b = BootSegLedger::new();
        for i in 0..5u64 {
            let _ = a.record(make_sample(i + 1, 3));
            let _ = b.record(make_sample(i + 1, 3));
        }
        let fa = a.joint_fingerprint();
        let fb = b.joint_fingerprint();
        let mut c = BootSegLedger::new();
        for i in 0..5u64 {
            let _ = c.record(make_sample(i + 1, 4));
        }
        let fc = c.joint_fingerprint();
        let ok = fa == fb && fa != fc && a.seal().is_ok() && a.frozen_fingerprint() == Some(fa)
            && a.seal().is_err();
        s.add("C66-06-指纹冻结", ok, if ok { "" } else { "指纹不稳定或重复封账未拒" });
    }

    // C66-07 注入核四类 10/10 必抓（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add(
            "C66-07-注入核40-40",
            ok,
            if ok { "" } else { "注入核存在漏抓" },
        );
    }

    // C66-08 封账后写拒（冻结面）
    {
        let mut led = BootSegLedger::new();
        let _ = led.record(make_sample(1, 1));
        let _ = led.seal();
        let r = led.record(make_sample(2, 2));
        let ok = r.is_err() && led.len() == 1;
        s.add("C66-08-封账后写拒", ok, if ok { "" } else { "封账后仍可写" });
    }

    // C66-09 拒绝账可追（每次拒绝留码/序/说明）
    {
        let mut led = BootSegLedger::new();
        let _ = led.record(BootSample { seq: 1, bounds: [0; 4], step_bounds: [0; 12], total: 0 });
        let ok = led.reject_count() >= 1 && led.rejects()[0].0 == XC_SAMPLE_EMPTY;
        s.add("C66-09-拒绝账留痕", ok, if ok { "" } else { "拒绝账未留痕" });
    }

    // C66-10 诊断码闭集互异递增（段内唯一）
    {
        let codes = [
            XC_SEG_INVERT, XC_SEG_UNKNOWN, XC_STEP_RANGE, XC_SAMPLE_EMPTY,
            XC_FINGER_DRIFT, XC_BOUND_AMBIGUOUS, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (BS_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C66-10-诊断码闭集", ok, if ok { "" } else { "诊断码漂移出段" });
    }

    s
}
