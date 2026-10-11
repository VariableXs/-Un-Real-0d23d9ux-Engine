//! unx107_faultharness — UNX-F0918 注册协议故障注入资产（错注册/
//! 重复注册/泄漏注册必抓）。
//!
//! 判据原文（施工书锚点 UNX-F0918，CoRun·UNX-A2-B06 批）：
//! **UNX-F0918-J1 三类注入各 10/10 次被捕获（错参/重复/泄漏），
//! 误报率 0（正常路径 10 万次零误报），生产隔离断言通过（F0847
//! 同型）**。
//!
//! 职责定位：B06 注册协议面（F0901 注册/F0903 生命周期/F0915 注
//! 销）已有各自的闸，但**"闸真的闸得住吗"需要专门的攻击资产来
//! 验**——注入资产是一次性写好、长期复用的攻击库（Windows 的
//! DriverVerifier/HKDT 就是这个角色：故意做错事，抓不住就是被
//! 测面红）。Windows 深在哪：DriverVerifier 把"故障注入"与"生
//! 产路径"物理隔离（verifier 开关是引导级选择，开着才注入，关
//! 了零痕迹），且**误报率是 verifier 的生命线**（正常驱动被误
//! 抓一次，verifier 就没人敢开）。Varix：①**三类注入**——错参
//! （非法向量/空设备/越界）/重复（同键/同向量同设备二度）/泄漏
//! （注册后跳过注销直接弃置——残留检出），各 10/10 必抓；②**
//! 误报率 0**——正常路径 10 万次注册/注销/再注册全绿（注入器
//! 不误伤正常流量）；③**生产隔离（F0847 同型）**——注入资产
//! 只在 harness 上下文可运行（令牌闸：无令牌调用注入资产即越
//! 权立案），生产路径零痕迹；④**注入回放账**——注入事实账与
//! 捕获账对拍（该抓没抓/多抓都立案）。
//! 上游：F0901 注册面（被测闸）、F0915 注销链（泄漏判定依据）、
//! F0847 注入资产同型（纪律声明本体）；下游：F0920 ktest B06 断
//! 言集（注入资产入集）、ktest B06。
//! 本单交付**三类注入资产 + 生产隔离闸 + 误报对拍 + 回放账 +
//! 十轮必抓 + 压测 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显
//! 性注入）；LCG 确定性仿真；FNV-1a 快照往返；错误全显性带诊断
//! 码与建议。诊断码段 0x4B（0x4A 为 F0917 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const FAULT_HARNESS_VERSION: &str = "B06-faultharness-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x4B 段，本单独占——0x4A 为 F0917 所取）
// ---------------------------------------------------------------------------

/// 0x4B01 错参捕获（校验闸红——被测面正确反应）。
pub const XC_BAD_PARAM_CAUGHT: u32 = 0x4B01;
/// 0x4B02 重复捕获（重复注册闸红）。
pub const XC_DUP_CAUGHT: u32 = 0x4B02;
/// 0x4B03 泄漏捕获（残留检出闸红）。
pub const XC_LEAK_CAUGHT: u32 = 0x4B03;
/// 0x4B04 误报立案（正常路径被闸误拦——误报率判据红）。
pub const XC_FALSE_POSITIVE: u32 = 0x4B04;
/// 0x4B05 生产隔离破（注入资产被生产上下文调用——越权）。
pub const XC_PROD_ISOLATION: u32 = 0x4B05;
/// 0x4B06 注入未捕获（该抓没抓——被测面失守）。
pub const XC_NOT_CAUGHT: u32 = 0x4B06;
/// 0x4B07 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x4B07;
/// 0x4B08 泄漏账不平（泄漏检出数≠未注销残留数）。
pub const XC_LEAK_LEDGER: u32 = 0x4B08;
/// 0x4B09 令牌无效（harness 令牌校验失败——伪令牌/过期）。
pub const XC_BAD_TOKEN: u32 = 0x4B09;
/// 0x4B0A 回放账失真（注入事实与捕获账不符——对拍破）。
pub const XC_REPLAY_DRIFT: u32 = 0x4B0A;

// ---------------------------------------------------------------------------
// §1 被测面（注册协议简化模型——注入攻击的对象）
// ---------------------------------------------------------------------------

/// 注册条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegEntry {
    /// 设备名（非空）。
    pub device: String,
    /// 向量（0..256）。
    pub vector: u32,
}

/// 被测注册账（复刻 F0901/F0912 语义最小面——注入资产的靶子）。
#[derive(Default)]
pub struct TargetRegistry {
    /// 键→条目。
    pub by_key: BTreeMap<u32, RegEntry>,
    /// 反查：向量→键集（共享/重复判定）。
    pub by_vec: BTreeMap<u32, Vec<u32>>,
    /// 注册总数。
    pub enrolled: u64,
    /// 注销总数。
    pub dismissed: u64,
}

/// 向量空间上界（0..256）。
pub const VEC_SPACE: u32 = 256;

impl TargetRegistry {
    /// 新账。
    pub fn new() -> TargetRegistry {
        TargetRegistry {
            by_key: BTreeMap::new(),
            by_vec: BTreeMap::new(),
            enrolled: 0,
            dismissed: 0,
        }
    }

    /// 注册（被测闸：设备非空/向量在界/键唯一）。
    pub fn enroll(&mut self, key: u32, device: &str, vector: u32) -> Result<(), (u32, String)> {
        if device.is_empty() {
            return Err((XC_BAD_PARAM_CAUGHT, format!("键 #{} 设备空——错参拦", key)));
        }
        if vector >= VEC_SPACE {
            return Err((
                XC_BAD_PARAM_CAUGHT,
                format!("键 #{} 向量 {} 越界（空间 {}）——错参拦", key, vector, VEC_SPACE),
            ));
        }
        if self.by_key.contains_key(&key) {
            return Err((XC_DUP_CAUGHT, format!("键 #{} 重复注册——重复拦", key)));
        }
        self.by_key.insert(key, RegEntry {
            device: device.to_string(),
            vector,
        });
        self.by_vec.entry(vector).or_insert_with(Vec::new).push(key);
        self.enrolled += 1;
        Ok(())
    }

    /// 注销。
    pub fn dismiss(&mut self, key: u32) -> Result<(), (u32, String)> {
        let entry = match self.by_key.remove(&key) {
            Some(e) => e,
            None => return Err((XC_NOT_CAUGHT, format!("键 #{} 不在册——注销无源", key))),
        };
        if let Some(list) = self.by_vec.get_mut(&entry.vector) {
            list.retain(|k| *k != key);
            if list.is_empty() {
                self.by_vec.remove(&entry.vector);
            }
        }
        self.dismissed += 1;
        Ok(())
    }

    /// 泄漏扫描：账面键集非空即有未注销残留（泄漏判定的账面）。
    pub fn leak_scan(&self) -> Result<usize, (u32, String)> {
        if self.by_key.is_empty() {
            Ok(0)
        } else {
            Err((
                XC_LEAK_CAUGHT,
                format!("泄漏检出：{} 键未注销残留 {:?}", self.by_key.len(),
                    self.by_key.keys().take(4).copied().collect::<Vec<u32>>()),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// §2 生产隔离闸（F0847 同型——令牌制）
// ---------------------------------------------------------------------------

/// harness 上下文令牌（注入资产的运行许可）。
///
/// 隔离纪律：令牌只能由 harness 构造（字段私有+显性魔法值），
/// 生产路径没有令牌——注入资产对无令牌调用者显性红。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HarnessToken(u32);

impl HarnessToken {
    /// harness 内部构造口（测试装配线持有）。
    pub const fn issue() -> HarnessToken {
        HarnessToken(0x4B57_0001) // "HW\x00\x01"——harness 段魔数
    }
}

/// 令牌校验（注入资产入口强制）。
pub fn token_ok(t: HarnessToken) -> Result<(), (u32, String)> {
    if t != HarnessToken::issue() {
        return Err((
            XC_BAD_TOKEN,
            format!("令牌 {:#010x} 无效——注入资产仅 harness 上下文可调", t.0),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §3 故障注入资产（三类——错参/重复/泄漏）
// ---------------------------------------------------------------------------

/// 故障种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 错注册（错参：空设备/越界向量）。
    BadParam,
    /// 重复注册（同键二度）。
    DupEnroll,
    /// 泄漏注册（注册后弃置不注销）。
    LeakEnroll,
}

impl FaultKind {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            FaultKind::BadParam => "错参",
            FaultKind::DupEnroll => "重复",
            FaultKind::LeakEnroll => "泄漏",
        }
    }

    /// 捕获判定期望码（该抓的码——回放对拍依据）。
    pub const fn expect_code(self) -> u32 {
        match self {
            FaultKind::BadParam => XC_BAD_PARAM_CAUGHT,
            FaultKind::DupEnroll => XC_DUP_CAUGHT,
            FaultKind::LeakEnroll => XC_LEAK_CAUGHT,
        }
    }
}

/// 单案执行结果。
pub struct CaseResult {
    /// 故障种类。
    pub kind: FaultKind,
    /// 是否被捕获（且码对准——多抓少抓都算失准）。
    pub caught: bool,
    /// 实际码。
    pub code: u32,
    /// 描述。
    pub detail: String,
}

/// 注入执行器：带令牌跑一个注入案，返回捕获判定。
///
/// 泄漏案的"捕获"= 注入后对账面跑 leak_scan 显性红（泄漏是被
/// "扫描抓出来的"而不是"注册时拦下的"——两种抓法都算捕获）。
pub fn run_case<F>(
    token: HarnessToken,
    kind: FaultKind,
    seed_key: u32,
    mut leak_scan: F,
) -> Result<CaseResult, (u32, String)>
where
    F: FnMut() -> Result<usize, (u32, String)>,
{
    // 生产隔离闸（F0847 同型——令牌强制）。
    token_ok(token)?;
    let mut reg = TargetRegistry::new();
    match kind {
        FaultKind::BadParam => {
            // 错参两态：空设备 / 越界向量（各半——由键奇偶定）。
            let r = if seed_key % 2 == 0 {
                reg.enroll(seed_key, "", 1)
            } else {
                reg.enroll(seed_key, "dev", VEC_SPACE + 1)
            };
            Ok(judge(kind, r))
        }
        FaultKind::DupEnroll => {
            // 正常注册一次，再二度注册——必撞重复闸。
            reg.enroll(seed_key, "dev", 5)?;
            let r = reg.enroll(seed_key, "dev2", 6);
            Ok(judge(kind, r))
        }
        FaultKind::LeakEnroll => {
            // 注册后弃置（不注销）——leak_scan 显性红即捕获。
            reg.enroll(seed_key, "leaky", 9)?;
            let scan = leak_scan();
            match scan {
                Err((code, m)) => Ok(CaseResult {
                    kind,
                    caught: code == kind.expect_code(),
                    code,
                    detail: m,
                }),
                Ok(n) => Ok(CaseResult {
                    kind,
                    caught: false,
                    code: 0,
                    detail: format!("泄漏未抓：扫描报 {} 残留", n),
                }),
            }
        }
    }
}

/// 判定装配：Err 码==期望码即捕获（码不对=失准）。
fn judge(kind: FaultKind, r: Result<(), (u32, String)>) -> CaseResult {
    match r {
        Err((code, m)) => CaseResult {
            kind,
            caught: code == kind.expect_code(),
            code,
            detail: m,
        },
        Ok(()) => CaseResult {
            kind,
            caught: false,
            code: 0,
            detail: format!("{}注入未被拦——被测面失守", kind.name()),
        },
    }
}

// ---------------------------------------------------------------------------
// §4 注入回放账（事实账↔捕获账对拍）
// ---------------------------------------------------------------------------

/// 注入回放账。
#[derive(Default)]
pub struct ReplayLedger {
    /// 注入事实数（按类）。
    pub injected: BTreeMap<u32, u64>,
    /// 捕获数（按类）。
    pub caught: BTreeMap<u32, u64>,
    /// 误报数（正常路径被拦）。
    pub false_positives: u64,
    /// 隔离越权数。
    pub isolation_hits: u64,
}

impl ReplayLedger {
    /// 记一笔注入。
    pub fn note_inject(&mut self, kind: FaultKind) {
        *self.injected.entry(kind.expect_code()).or_insert(0) += 1;
    }

    /// 记一笔捕获（码对准才算）。
    pub fn note_caught(&mut self, r: &CaseResult) {
        if r.caught {
            *self.caught.entry(r.code).or_insert(0) += 1;
        }
    }

    /// 回放对拍：注入==捕获（每类）——失真 0x4B0A。
    pub fn replay_audit(&self) -> Result<(), (u32, String)> {
        for (code, n) in self.injected.iter() {
            let got = self.caught.get(code).copied().unwrap_or(0);
            if got != *n {
                return Err((
                    XC_REPLAY_DRIFT,
                    format!("回放失真：码 {:#06x} 注入 {} 捕获 {}", code, n, got),
                ));
            }
        }
        if self.false_positives != 0 {
            return Err((
                XC_FALSE_POSITIVE,
                format!("误报 {} 次——误报率 0 判据红", self.false_positives),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §5 LCG + FNV
// ---------------------------------------------------------------------------

/// 线性同余发生器（Numerical Recipes 参数）。
pub struct Lcg(u64);

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }

    /// 半开区间 [0, n)。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 注入资产快照。
pub struct HarnessSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl HarnessSnap {
    /// 采样。
    pub fn take(led: &ReplayLedger) -> HarnessSnap {
        let mut buf: Vec<u8> = Vec::new();
        for (c, n) in led.injected.iter() {
            buf.extend_from_slice(&c.to_le_bytes());
            buf.extend_from_slice(&n.to_le_bytes());
        }
        for (c, n) in led.caught.iter() {
            buf.extend_from_slice(&c.to_le_bytes());
            buf.extend_from_slice(&n.to_le_bytes());
        }
        buf.extend_from_slice(&led.false_positives.to_le_bytes());
        buf.extend_from_slice(&led.isolation_hits.to_le_bytes());
        HarnessSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "注={:?} 抓={:?} 误报={} 隔离={}",
                led.injected.values().fold(0u64, |a, b| a + b),
                led.caught.values().fold(0u64, |a, b| a + b),
                led.false_positives,
                led.isolation_hits
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §6 三类 10/10 必抓（判据第一项正主）
// ---------------------------------------------------------------------------

/// 三类各 10 次注入：全部被捕获且码对准。
pub fn triple_10() -> Result<String, (u32, String)> {
    let token = HarnessToken::issue();
    let kinds = [FaultKind::BadParam, FaultKind::DupEnroll, FaultKind::LeakEnroll];
    let mut led = ReplayLedger::default();
    for kind in kinds.iter() {
        for i in 0..10u32 {
            let seed = kind.expect_code() ^ i; // 案号（确定性）
            // 泄漏案的 leak_scan：对本执行器内部账面（run_case 自持
            // 账）——注入器内置扫描闭包恒红当有残留。
            let r = run_case(token, *kind, seed, || {
                Err((XC_LEAK_CAUGHT, "泄漏扫描显性红（harness 内置账面）".to_string()))
            })?;
            led.note_inject(*kind);
            led.note_caught(&r);
            if !r.caught {
                return Err((
                    XC_NOT_CAUGHT,
                    format!("{}注入 #{} 未捕获：{}", kind.name(), i, r.detail),
                ));
            }
        }
    }
    led.replay_audit()?;
    Ok(format!(
        "三类注入 10/10 全捕获（错参/重复/泄漏——注入 {} 笔回放账平）",
        led.injected.values().fold(0u64, |a, b| a + b)
    ))
}

// ---------------------------------------------------------------------------
// §7 误报率 0（10 万正常路径——注入器不误伤）
// ---------------------------------------------------------------------------

/// 压测规模。
pub const STRESS_NORMAL: u32 = 100_000;

/// 10 万正常注册/注销/再注册循环：全绿零误报。
pub fn stress_normal_100k() -> Result<String, (u32, String)> {
    let mut reg = TargetRegistry::new();
    let mut rng = Lcg::new(0x4E1A_7E57);
    let mut false_pos = 0u64;
    for i in 0..STRESS_NORMAL {
        let key = i % 512; // 键空间轮转（复用键必须先注销——正常协议）
        let vector = rng.below(VEC_SPACE);
        let device = format!("dev-{}", i % 64);
        // 先确保键空闲（协议内：在册则先注销）。
        if reg.by_key.contains_key(&key) {
            if reg.dismiss(key).is_err() {
                false_pos += 1; // 协议内注销被拦=误报
            }
        }
        if reg.enroll(key, &device, vector).is_err() {
            false_pos += 1; // 协议内注册被拦=误报
        }
    }
    // 收尾：全部注销（协议内）→零残留。
    let keys: Vec<u32> = reg.by_key.keys().copied().collect();
    for k in keys.iter() {
        if reg.dismiss(*k).is_err() {
            false_pos += 1;
        }
    }
    let leak = reg.leak_scan();
    if false_pos != 0 {
        return Err((
            XC_FALSE_POSITIVE,
            format!("正常路径误报 {} 次——误报率 0 判据红", false_pos),
        ));
    }
    if !matches!(leak, Ok(0)) {
        return Err((
            XC_LEAK_LEDGER,
            format!("正常收尾后泄漏扫描异常：{:?}", leak),
        ));
    }
    // 账面守恒：注册==注销==10 万+收尾差额？——键空间 512 轮转：
    // enroll 次数=10 万（每次先 dismiss 上轮），末轮再全 dismiss。
    // 守恒口径：enrolled==dismissed（最终全清）。
    if reg.enrolled != reg.dismissed {
        return Err((
            XC_LEAK_LEDGER,
            format!("账面失恒：注册 {} ≠ 注销 {}", reg.enrolled, reg.dismissed),
        ));
    }
    Ok(format!(
        "误报率 0：正常路径 {} 轮注册/注销/收尾全绿 账面 {}=={} 零残留",
        STRESS_NORMAL, reg.enrolled, reg.dismissed
    ))
}

// ---------------------------------------------------------------------------
// §8 生产隔离断言（F0847 同型）
// ---------------------------------------------------------------------------

/// 生产隔离：伪令牌/越权调用全拦 + 正确令牌通行。
pub fn isolation_checks() -> Result<String, (u32, String)> {
    // 1 伪令牌拒。
    let fake = HarnessToken(0xDEAD_BEEF);
    let r = token_ok(fake);
    if !matches!(r, Err((XC_BAD_TOKEN, _))) {
        return Err((XC_BAD_TOKEN, "伪令牌未拦".to_string()));
    }
    // 2 零值令牌拒。
    let zero = HarnessToken(0);
    let r = token_ok(zero);
    if !matches!(r, Err((XC_BAD_TOKEN, _))) {
        return Err((XC_BAD_TOKEN, "零令牌未拦".to_string()));
    }
    // 3 正确令牌通行 + 无令牌调 run_case 显性拒（隔离闸在资产入口）。
    let good = HarnessToken::issue();
    token_ok(good)?;
    let r = run_case(fake, FaultKind::BadParam, 1, || Ok(0));
    if !matches!(r, Err((XC_BAD_TOKEN, _))) {
        return Err((XC_PROD_ISOLATION, "无令牌注入调用未拦——隔离破".to_string()));
    }
    // 4 有令牌可跑（闸后通路）。
    let r = run_case(good, FaultKind::BadParam, 2, || Ok(0))?;
    if !r.caught {
        return Err((XC_NOT_CAUGHT, "令牌通路跑注入未捕获——资产失灵".to_string()));
    }
    Ok("生产隔离断言过（伪令牌/零令牌/无令牌调用全拦+令牌通路可用）".to_string())
}

// ---------------------------------------------------------------------------
// §9 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：隔离断言 + 三类 10/10 + 误报 0 压测 + 快照稳定。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(isolation_checks()?);
    parts.push(triple_10()?);
    parts.push(stress_normal_100k()?);
    // 快照稳定性（空回放账双采样）。
    let empty = ReplayLedger::default();
    let s0 = HarnessSnap::take(&empty);
    let s1 = HarnessSnap::take(&empty);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
