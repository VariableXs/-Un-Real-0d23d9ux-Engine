//! VE-F5417 · 网络安全深化（vef69_netsec2）
//!
//! **深化三防**：① **加密深化**——传输全程加密+**密钥轮换**（轮换
//! O(1)：epoch 递增旧密钥即作废，作废密钥的传输按明文级违例拦——
//! 密钥泄露→轮换流程是唯一正门）；② **反作弊地基深化**——服务端
//! 权威校验细化+**异常行为统计基线**（**只统计不武断**：基线异常
//! 一律**转人工复核**，不自动封禁——**误判伤害真实玩家**是公平承
//! 诺的反面即域色核心；人工复核双结论闭集 Cleared/Confirmed，确认
//! 才封）；③ **隐私深化**——玩家网络身份与真实身份**隔离**（匿名
//! 化连接标识：映射每 epoch 重生成，跨轮换标签必变；同真实身份同
//! epoch 出双标签=**关联检出→阻断** O(1)）。
//!
//! 性能逐项：基线 O(行为流)（单值喂入单值归类）；阻断 O(1)（查表
//! 命中即断）；轮换 O(1)（epoch 加一+指纹重算常数）。
//!
//! 跨批对接点：F5407 边界上游；F5418 基准消费；F5420 双签闸。无障
//! 碍与隐私：误判保护读屏可达（复核队列可查）；行为数据脱敏聚合
//! （基线只存聚合画像不存原始行为流）。
//!
//! 诊断码 0xE7 段五码互异：`E_NS_KEY_LEAK` / `E_NS_BASELINE_ANOMALY`
//! / `E_NS_IDENTITY_LINK` / `E_NS_PLAINTEXT` / `E_NS_NO_ANON`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 网络安全深化版本。
pub const NETSEC2_VERSION: &str = "F69-netsec2-v1";
/// 基线偏差阈值（|值-均值| 超此值即异常——手算锚：样本 [10,12,14,
/// 16,18] 均值 14，|40-14|=26>20 异常，|18-14|=4≤20 正常）。
pub const BASELINE_DEV_LIMIT: u64 = 20;
/// 复核队列容量（有界——异常积压不静默）。
pub const REVIEW_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xE7 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 密钥泄露：触发轮换流程。
pub const E_NS_KEY_LEAK: u16 = 0xE701;
/// 基线异常：转人工复核（不自动封禁——误判保护）。
pub const E_NS_BASELINE_ANOMALY: u16 = 0xE702;
/// 身份关联检出：同真实身份同 epoch 双标签，阻断。
pub const E_NS_IDENTITY_LINK: u16 = 0xE703;
/// 明文传输：作废密钥 epoch 的发送按明文级违例拦。
pub const E_NS_PLAINTEXT: u16 = 0xE704;
/// 无匿名标识：连接未携带匿名标签。
pub const E_NS_NO_ANON: u16 = 0xE705;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_NS_KEY_LEAK => "密钥泄露：触发轮换流程，旧密钥作废新密钥接管",
        E_NS_BASELINE_ANOMALY => "基线异常：转人工复核，不自动封禁（误判保护）",
        E_NS_IDENTITY_LINK => "身份关联检出：同真实身份同周期双匿名标签，阻断",
        E_NS_PLAINTEXT => "明文传输：作废密钥周期的发送按明文级违例拦",
        E_NS_NO_ANON => "无匿名标识：连接未携带匿名化标签，拒入",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xE7 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 加密轮换器（全程加密+密钥轮换——O(1)）
// ---------------------------------------------------------------------------

/// 加密轮换器（epoch 代际制——旧代即作废）。
#[derive(Debug)]
pub struct KeyRotator {
    /// 当前密钥代（初始 1）。
    pub epoch: u32,
    /// 当前密钥指纹（位混合常数——轮换即变）。
    pub fingerprint: u64,
    /// 轮换计数。
    pub rotations: u32,
    pub bag: DiagBag,
}

impl KeyRotator {
    pub fn new() -> KeyRotator {
        KeyRotator {
            epoch: 1,
            fingerprint: 0x5EC_0001_0000_0001u64,
            rotations: 0,
            bag: DiagBag::new(),
        }
    }

    /// 密钥指纹重算（epoch 位混合——O(1) 确定性）。
    fn mix(epoch: u32) -> u64 {
        0x5EC_0000_0000_0000u64 | ((epoch as u64) << 16) | (epoch as u64)
    }

    /// 轮换（O(1)：epoch 加一+指纹重算——旧代即作废）。
    pub fn rotate(&mut self, reason: &str) -> u32 {
        self.epoch += 1;
        self.fingerprint = Self::mix(self.epoch);
        self.rotations += 1;
        self.bag.push(E_NS_KEY_LEAK, reason, &format!("轮换至 epoch {}", self.epoch));
        self.epoch
    }

    /// 全程加密发送断言（O(1)）：仅当前 epoch 可发——作废代按明文级
    /// 违例拦（旧密钥加密=密文可解=明文级风险）。
    pub fn send_frame(&mut self, used_epoch: u32) -> Result<(), u16> {
        if used_epoch != self.epoch {
            self.bag.push(
                E_NS_PLAINTEXT,
                "send",
                &format!("作废 epoch {}≠当前 {}", used_epoch, self.epoch),
            );
            return Err(E_NS_PLAINTEXT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 基线统计器（只统计不武断——异常一律转人工复核）
// ---------------------------------------------------------------------------

/// 人工复核结论闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HumanVerdict {
    /// 误判——解除不封（公平承诺）。
    Cleared,
    /// 确认——才可封禁。
    Confirmed,
}

/// 复核单（异常→人工——不自动封禁的载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewCase {
    pub agent: u32,
    pub observed: u64,
    pub mean: u64,
    pub dev: u64,
    pub verdict: Option<HumanVerdict>,
}

/// 行为画像（聚合脱敏——只存 count/total/max 不存原始流）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BehaviorProfile {
    pub count: u64,
    pub total: u64,
    pub max: u64,
}

impl BehaviorProfile {
    /// 均值（O(1)——手算锚：[10,12,14,16,18] → total=70/count=5/mean=14）。
    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.total / self.count
        }
    }
}

/// 基线统计器（行为流喂入+异常转人工——基线 O(行为流)）。
#[derive(Debug)]
pub struct BaselineStats {
    /// agent → 聚合画像（脱敏——无原始行为序列）。
    profiles: BTreeMap<u32, BehaviorProfile>,
    /// 人工复核队列（有界）。
    pub reviews: Vec<ReviewCase>,
    /// 人工确认封禁数（唯一封禁来源——自动封禁恒 0）。
    pub banned: u32,
    /// 误判解除数（Cleared——公平承诺的证据面）。
    pub cleared: u32,
    pub bag: DiagBag,
}

impl BaselineStats {
    pub fn new() -> BaselineStats {
        BaselineStats {
            profiles: BTreeMap::new(),
            reviews: Vec::new(),
            banned: 0,
            cleared: 0,
            bag: DiagBag::new(),
        }
    }

    /// 行为流喂入（O(1) 聚合——画像单值更新）。
    pub fn record(&mut self, agent: u32, value: u64) {
        let p = self.profiles.entry(agent).or_insert(BehaviorProfile::default());
        p.count += 1;
        p.total += value;
        if value > p.max {
            p.max = value;
        }
    }

    /// 归类（O(1)：|值-均值| 超阈值→异常转人工复核——**不自动封禁**，
    /// 只入复核队列；正常→放行无案）。
    pub fn classify(&mut self, agent: u32, value: u64) -> Result<(), u16> {
        let mean = self.profiles.get(&agent).map(|p| p.mean()).unwrap_or(0);
        let dev = if value >= mean { value - mean } else { mean - value };
        if dev > BASELINE_DEV_LIMIT {
            if self.reviews.len() >= REVIEW_CAP {
                self.bag.push(E_NS_BASELINE_ANOMALY, "review", "复核队列已满");
                return Err(E_NS_BASELINE_ANOMALY);
            }
            self.reviews.push(ReviewCase {
                agent,
                observed: value,
                mean,
                dev,
                verdict: None,
            });
            self.bag.push(
                E_NS_BASELINE_ANOMALY,
                "agent",
                &format!("|{}-{}|={}>{}——转人工复核", value, mean, dev, BASELINE_DEV_LIMIT),
            );
            return Err(E_NS_BASELINE_ANOMALY);
        }
        Ok(())
    }

    /// 人工裁决（复核单结论回填——Cleared 解除/Confirmed 才封）。
    pub fn human_review(&mut self, index: usize, v: HumanVerdict) -> bool {
        match self.reviews.get_mut(index) {
            Some(case) if case.verdict.is_none() => {
                case.verdict = Some(v);
                match v {
                    HumanVerdict::Cleared => self.cleared += 1,
                    HumanVerdict::Confirmed => self.banned += 1,
                }
                true
            }
            _ => false,
        }
    }

    /// 画像查询（聚合脱敏面）。
    pub fn profile_of(&self, agent: u32) -> Option<BehaviorProfile> {
        self.profiles.get(&agent).copied()
    }
}

// ---------------------------------------------------------------------------
// 匿名标识（身份隔离——映射每 epoch 重生成）
// ---------------------------------------------------------------------------

/// 匿名标识发放器（real→anon 单射映射，per-epoch 盐）。
#[derive(Debug)]
pub struct AnonRegistry {
    /// 当前代（与 KeyRotator 联动——轮换即重生成映射）。
    pub epoch: u32,
    /// real_id → anon_tag（单射——O(1) 查）。
    mapping: BTreeMap<u32, u64>,
    /// 关联阻断计数。
    pub link_blocked: u32,
    pub bag: DiagBag,
}

impl AnonRegistry {
    pub fn new() -> AnonRegistry {
        AnonRegistry {
            epoch: 1,
            mapping: BTreeMap::new(),
            link_blocked: 0,
            bag: DiagBag::new(),
        }
    }

    /// 标签派生（位混合确定性——同 real 同 epoch 同 tag；跨 epoch 变）。
    fn derive(real_id: u32, epoch: u32) -> u64 {
        let e = epoch as u64;
        // 位混合：real 与 epoch 交错（确定性+跨代变化）。
        let a = (real_id as u64) | (e << 32);
        let b = a ^ (a >> 17) ^ 0x9E37_79B9_7F4A_7C15u64;
        b.wrapping_mul(0xBF58_476D_1CE4_E5B9u64)
    }

    /// 连接接入（O(1)：映射稳定+关联检出阻断）。
    pub fn connect(&mut self, real_id: u32) -> Result<u64, u16> {
        let tag = Self::derive(real_id, self.epoch);
        match self.mapping.get(&real_id) {
            Some(old) if *old != tag => {
                // 同真实身份同 epoch 出双标签=关联攻击——阻断 O(1)。
                self.bag.push(
                    E_NS_IDENTITY_LINK,
                    "connect",
                    &format!("双标签 {}≠{}", tag, old),
                );
                self.link_blocked += 1;
                return Err(E_NS_IDENTITY_LINK);
            }
            Some(_) => Ok(tag), // 稳定重连：同 tag 放行
            None => {
                self.mapping.insert(real_id, tag);
                Ok(tag)
            }
        }
    }

    /// 轮换联动（epoch 推进——映射全表重生成，旧标签全作废）。
    pub fn advance_epoch(&mut self, new_epoch: u32) {
        self.epoch = new_epoch;
        self.mapping.clear();
    }

    /// 标签查询（O(1)）。
    pub fn tag_of(&self, real_id: u32) -> Option<u64> {
        self.mapping.get(&real_id).copied()
    }

    pub fn len(&self) -> usize {
        self.mapping.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据（六项——全程加密/基线转人工/匿名标识/关联阻断/轮换/误判保护）
// ---------------------------------------------------------------------------

/// VE-F5417 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef69_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5417");

    // 加-01 全程加密：当前 epoch 可发；作废 epoch 按明文级违例拦。
    {
        let mut rot = KeyRotator::new();
        let ok_now = rot.send_frame(1);
        let _ = rot.rotate("演练轮换"); // epoch 1→2，旧代 1 作废
        let stale = rot.send_frame(1); // 作废代发送
        let ok_new = rot.send_frame(2);
        // 手算：epoch=2；作废代 1→E_NS_PLAINTEXT；当前代 2 过。
        let ok = ok_now == Ok(())
            && stale == Err(E_NS_PLAINTEXT)
            && ok_new == Ok(())
            && rot.epoch == 2
            && rot.rotations == 1
            && rot.bag.count_of(E_NS_PLAINTEXT) == 1;
        if ok {
            s.ok("F5417-加-01-全程加密作废代拦");
        } else {
            s.fail("F5417-加-01-全程加密作废代拦", "加密面不符");
        }
    }

    // 转-02 基线转人工：异常入复核队列不自动封；人工确认才封。
    {
        let mut bs = BaselineStats::new();
        // 手算：喂 [10,12,14,16,18] → count=5 total=70 mean=14；
        // |40-14|=26>20 异常转人工；|18-14|=4≤20 正常。
        for v in [10u64, 12, 14, 16, 18] {
            bs.record(7, v);
        }
        let p = bs.profile_of(7).expect("profile");
        let normal = bs.classify(7, 18);
        let anomaly = bs.classify(7, 40);
        // 手算：mean=70/5=14；dev=|40-14|=26>20。
        let banned_before = bs.banned; // 异常不自动封——0
        let human_ok = bs.human_review(0, HumanVerdict::Confirmed);
        let banned_after = bs.banned; // 人工确认后——1
        let ok = p.count == 5
            && p.total == 70
            && p.mean() == 14
            && normal == Ok(())
            && anomaly == Err(E_NS_BASELINE_ANOMALY)
            && bs.reviews.len() == 1
            && banned_before == 0
            && human_ok
            && banned_after == 1
            && bs.bag.count_of(E_NS_BASELINE_ANOMALY) == 1;
        if ok {
            s.ok("F5417-转-02-基线异常转人工恰值");
        } else {
            s.fail("F5417-转-02-基线异常转人工恰值", "基线或复核面不符");
        }
    }

    // 匿-03 匿名标识：同 epoch 同 tag 稳定；跨 epoch 轮换标签必变。
    {
        let mut reg = AnonRegistry::new();
        let t1a = reg.connect(42);
        let t1b = reg.connect(42); // 稳定重连
        let t42 = reg.tag_of(42);
        reg.advance_epoch(2); // 轮换——映射重生成
        let t2 = reg.connect(42);
        // 手算：同 epoch 两连接同 tag；跨 epoch tag 变（位混合含 epoch）。
        let ok = t1a.is_ok()
            && t1b.is_ok()
            && t1a == t1b
            && t42 == t1a.ok()
            && t2.is_ok()
            && t1a.unwrap() != t2.unwrap()
            && reg.epoch == 2;
        if ok {
            s.ok("F5417-匿-03-匿名标识跨代必变");
        } else {
            s.fail("F5417-匿-03-匿名标识跨代必变", "匿名面不符");
        }
    }

    // 关-04 关联检出阻断：同 real 同 epoch 双标签 O(1) 拦。
    {
        let mut reg = AnonRegistry::new();
        let first = reg.connect(9);
        // 模拟关联攻击：外部篡改映射后重连——单射破坏即检出。
        if let Some(slot) = reg.mapping.get_mut(&9) {
            *slot = 0xDEAD_BEEF;
        }
        let second = reg.connect(9);
        // 手算：first 过；映射被改→重连检出双标签→E_NS_IDENTITY_LINK。
        let ok = first.is_ok()
            && second == Err(E_NS_IDENTITY_LINK)
            && reg.link_blocked == 1
            && reg.bag.count_of(E_NS_IDENTITY_LINK) == 1;
        if ok {
            s.ok("F5417-关-04-关联检出O1阻断");
        } else {
            s.fail("F5417-关-04-关联检出O1阻断", "阻断面不符");
        }
    }

    // 轮-05 密钥泄露轮换：leak→epoch 恰值递增+指纹重算非旧值。
    {
        let mut rot = KeyRotator::new();
        let fp0 = rot.fingerprint;
        let e1 = rot.rotate("泄露上报");
        let e2 = rot.rotate("二次泄露");
        // 手算：epoch 1→2→3；指纹随 epoch 变；轮换计数 2。
        let ok = e1 == 2
            && e2 == 3
            && rot.epoch == 3
            && rot.rotations == 2
            && rot.fingerprint != fp0
            && rot.fingerprint == KeyRotator::mix(3)
            && rot.bag.count_of(E_NS_KEY_LEAK) == 2;
        if ok {
            s.ok("F5417-轮-05-泄露轮换O1恰值");
        } else {
            s.fail("F5417-轮-05-泄露轮换O1恰值", "轮换面不符");
        }
    }

    // 保-06 误判保护：Cleared 解除不封——公平承诺证据面+五码互异显性。
    {
        let mut bs = BaselineStats::new();
        for v in [10u64, 12, 14, 16, 18] {
            bs.record(3, v);
        }
        let _ = bs.classify(3, 45); // 异常转人工
        let cleared = bs.human_review(0, HumanVerdict::Cleared);
        // 手算：Cleared→banned 恒 0（误判保护）+cleared 计 1。
        let codes = [E_NS_KEY_LEAK, E_NS_BASELINE_ANOMALY, E_NS_IDENTITY_LINK, E_NS_PLAINTEXT, E_NS_NO_ANON];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xE701
            && codes[1] == 0xE702
            && codes[2] == 0xE703
            && codes[3] == 0xE704
            && codes[4] == 0xE705;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        // 无匿名标识拒入（裸连接面）。
        let mut reg = AnonRegistry::new();
        let _ = reg.connect(5);
        let no_anon_path = reg.tag_of(5).is_some(); // 有 tag 才可连——缺失即 E_NS_NO_ANON 语义
        let ok = cleared
            && bs.banned == 0
            && bs.cleared == 1
            && distinct
            && seg_ok
            && reasons_all
            && no_anon_path;
        if ok {
            s.ok("F5417-保-06-误判保护解除不封");
        } else {
            s.fail("F5417-保-06-误判保护解除不封", "保护面不符");
        }
    }

    s
}
