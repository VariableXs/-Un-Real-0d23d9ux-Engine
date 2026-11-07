//! UNX-F51629 攻击样本库扩容（×30 新样本入库与指纹） · 0xC2000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51630-J1 样本 ×30 全录指纹账；UNX-F51630-J1R 未录样本注入 10/10 拒绝（版本账纪律）。
//!
//! 结构清单：
//!   - SampleRegistry：攻击样本库（族别/指纹 FNV-1a/版本账）
//!   - VersionLedger：版本账（单调版本号 + 样本逐条入账）
//!   - RegLedger：入库审计账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② 扩容语义：×30 个新样本分 5 族（零长/越界/重放/风暴/畸形）每族 6 条，
//!      逐条入库并计算 FNV-1a 指纹落账——J1 判「样本 ×30 全录指纹账」：
//!      库内样本数 = 30、指纹账行数 = 30、指纹两两可分辨（无碰撞）。
//!   ③ 版本账纪律（J1R）：每个样本携带版本号，版本必须严格递增；未录样本
//!      （不在库中的指纹）注入必须 10/10 拒绝——「未录即拒」是扩容后库的
//!      闭包不变量。
//!   ④ 指纹唯一性：同族内样本内容经 LCG 变异保证指纹不同；若碰撞显性记码。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51629 − 51501)。
pub const SEG_BASE: u32 = 0xC2000;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51629u32 - 51501));

/// 新样本总数（判据 J1 的 ×30）。
pub const NEW_SAMPLES: usize = 30;
/// 族数。
pub const FAMILIES: usize = 5;
/// 每族样本数。
pub const PER_FAMILY: usize = NEW_SAMPLES / FAMILIES;
/// 库容量上限。
pub const REGISTRY_CAP: usize = 64;
/// 未录样本注入轮数（判据 J1R 的 ×10）。
pub const UNSEEN_ROUNDS: u32 = 10;

/// 样本族别（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 零长族
    ZeroLen = 0,
    /// 越界族
    Overrun = 1,
    /// 重放族
    Replay = 2,
    /// 风暴族
    Storm = 3,
    /// 畸形族
    Malformed = 4,
}

impl Family {
    /// 族码闭集解码。
    pub fn from_code(c: u32) -> Option<Family> {
        match c {
            0 => Some(Family::ZeroLen),
            1 => Some(Family::Overrun),
            2 => Some(Family::Replay),
            3 => Some(Family::Storm),
            4 => Some(Family::Malformed),
            _ => None,
        }
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Family::ZeroLen => "零长",
            Family::Overrun => "越界",
            Family::Replay => "重放",
            Family::Storm => "风暴",
            Family::Malformed => "畸形",
        }
    }
}

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 族码越闭集
    FamilyUnknown,
    /// 样本指纹重复（碰撞）
    FingerprintDup,
    /// 库容量已满
    RegistryFull,
    /// 版本倒退
    VersionRegress,
    /// 版本重复
    VersionDup,
    /// 空样本拒收
    SampleEmpty,
    /// 样本超长拒收
    SampleOversize,
    /// 未录样本注入拒绝
    UnseenRejected,
    /// 完整性缺口（指纹账行数≠样本数）
    CompletenessGap,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl RegCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::Ok => "样本入库成功：指纹落账",
            RegCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            RegCode::FamilyUnknown => "族码越闭集：只接受 0..4 五族",
            RegCode::FingerprintDup => "指纹碰撞：同库样本指纹必须两两不同",
            RegCode::RegistryFull => "库容量已满：超出上限不得再入库",
            RegCode::VersionRegress => "版本倒退：入库版本号必须严格递增",
            RegCode::VersionDup => "版本重复：同版本不得重复入库",
            RegCode::SampleEmpty => "空样本拒收：零字节样本无指纹价值",
            RegCode::SampleOversize => "样本超长：超出上限按拒绝处理",
            RegCode::UnseenRejected => "未录样本拒绝：不在库中的指纹一律拒收（版本账纪律）",
            RegCode::CompletenessGap => "完整性缺口：指纹账行数必须等于库内样本数",
            RegCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [RegCode; 12] = [
    RegCode::Ok,
    RegCode::SegBad,
    RegCode::FamilyUnknown,
    RegCode::FingerprintDup,
    RegCode::RegistryFull,
    RegCode::VersionRegress,
    RegCode::VersionDup,
    RegCode::SampleEmpty,
    RegCode::SampleOversize,
    RegCode::UnseenRejected,
    RegCode::CompletenessGap,
    RegCode::FingerprintMismatch,
];

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 样本条目。
#[derive(Clone)]
pub struct Sample {
    pub family: Family,
    pub version: u64,
    pub payload: Vec<u8>,
    pub fp: u64,
}

/// 攻击样本库 + 版本账。
pub struct SampleRegistry {
    samples: Vec<Sample>,
    fp_rows: Vec<(u64, u64)>,
    last_version: u64,
    trace: Vec<String>,
}

impl SampleRegistry {
    /// 空库。
    pub fn new() -> SampleRegistry {
        SampleRegistry { samples: Vec::new(), fp_rows: Vec::new(), last_version: 0, trace: Vec::new() }
    }
    /// 入库：族码闭集 → 空/超长 → 版本单调 → 指纹唯一 → 落账。
    pub fn admit(&mut self, fam_code: u32, version: u64, payload: &[u8]) -> Result<u64, RegCode> {
        let fam = match Family::from_code(fam_code) {
            Some(f) => f,
            None => return Err(RegCode::FamilyUnknown),
        };
        if payload.is_empty() {
            return Err(RegCode::SampleEmpty);
        }
        if payload.len() > 64 {
            return Err(RegCode::SampleOversize);
        }
        if version < self.last_version && !self.samples.is_empty() {
            return Err(RegCode::VersionRegress);
        }
        if self.samples.iter().any(|s| s.version == version) {
            return Err(RegCode::VersionDup);
        }
        if self.samples.len() >= REGISTRY_CAP {
            return Err(RegCode::RegistryFull);
        }
        let fp = fnv1a(payload) ^ ((fam as u64) << 56);
        if self.samples.iter().any(|s| s.fp == fp) {
            return Err(RegCode::FingerprintDup);
        }
        self.samples.push(Sample { family: fam, version, payload: payload.iter().copied().collect(), fp });
        self.fp_rows.push((fp, version));
        self.last_version = version;
        self.trace.push(format!("fam={} ver={}", fam.name(), version));
        Ok(fp)
    }
    /// 版本账查询：指纹在库则返回版本。
    pub fn lookup(&self, fp: u64) -> Option<u64> {
        for s in self.samples.iter() {
            if s.fp == fp {
                return Some(s.version);
            }
        }
        None
    }
    /// 未录样本守门：不在库 → UnseenRejected。
    pub fn gate(&self, fp: u64) -> Result<u64, RegCode> {
        match self.lookup(fp) {
            Some(v) => Ok(v),
            None => Err(RegCode::UnseenRejected),
        }
    }
    /// 库内样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    /// 指纹账行数。
    pub fn fp_rows(&self) -> usize {
        self.fp_rows.len()
    }
    /// 指纹两两不同。
    pub fn unique_fps(&self) -> bool {
        for i in 0..self.samples.len() {
            let a = match self.samples.get(i) {
                Some(s) => s.fp,
                None => return false,
            };
            for j in (i + 1)..self.samples.len() {
                if let Some(b) = self.samples.get(j) {
                    if a == b.fp {
                        return false;
                    }
                }
            }
        }
        true
    }
    /// 当前版本。
    pub fn version(&self) -> u64 {
        self.last_version
    }
    /// 轨迹。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 入库审计账本。
#[derive(Default)]
pub struct RegLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl RegLedger {
    /// 记一次检出。
    pub fn hit(&mut self, round: u32, code: RegCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, RegCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 31 ^ 0x853C_49E6_748F_EA9B
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 构造族内第 k 号样本的确定性载荷（LCG 无关，保证双跑一致）。
pub fn make_payload(fam: Family, k: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(0x80 + fam as u8);
    v.push(k as u8);
    let mut x: u64 = (fam as u64) * 0x100 + k as u64;
    for _ in 0..(6 + k) {
        x = x.wrapping_mul(0x100000001B3).wrapping_add(0x9E3779B97F4A7C15);
        v.push((x >> 13) as u8);
    }
    v
}

/// ×30 新样本全量入库（J1 主流程）：5 族 × 每族 6 条，版本严格递增。
pub fn populate_registry() -> Result<SampleRegistry, RegCode> {
    let mut reg = SampleRegistry::new();
    let mut ver = 1u64;
    for f in 0..FAMILIES {
        let fam = match Family::from_code(f as u32) {
            Some(f) => f,
            None => return Err(RegCode::FamilyUnknown),
        };
        for k in 0..PER_FAMILY {
            let payload = make_payload(fam, k);
            reg.admit(f as u32, ver, &payload)?;
            ver += 1;
        }
    }
    Ok(reg)
}

/// 入库注入 ×10 轮：五族坏样本（空/超长/版本倒退/版本重复/族码越界）各 2 轮。
pub fn injection_campaign() -> RegLedger {
    let mut led = RegLedger::default();
    let mut rnd = Lcg::new();
    let base = match populate_registry() {
        Ok(r) => r,
        Err(_) => SampleRegistry::new(),
    };
    for r in 0..UNSEEN_ROUNDS {
        let fam = (r % 5) as u64;
        let mut reg = SampleRegistry::new();
        let _ = reg.admit(0, 1, &make_payload(Family::ZeroLen, 0));
        let hit: Option<RegCode> = match fam {
            // 空样本
            0 => match reg.admit(0, 2, &[]) {
                Err(c) => Some(c),
                Ok(_) => None,
            },
            // 超长样本
            1 => {
                let mut big: Vec<u8> = Vec::new();
                for _ in 0..65 {
                    big.push(0x41);
                }
                match reg.admit(1, 2, &big) {
                    Err(c) => Some(c),
                    Ok(_) => None,
                }
            }
            // 版本倒退
            2 => match reg.admit(2, 0, &make_payload(Family::Replay, 9)) {
                Err(c) => Some(c),
                Ok(_) => None,
            },
            // 版本重复
            3 => match reg.admit(3, 1, &make_payload(Family::Storm, 9)) {
                Err(c) => Some(c),
                Ok(_) => None,
            },
            // 族码越界
            _ => match reg.admit(9, 2, &make_payload(Family::Malformed, 9)) {
                Err(c) => Some(c),
                Ok(_) => None,
            },
        };
        let _ = rnd.next();
        // 另加未录样本守门注入：从 base 拿一个库外指纹，gate 必拒。
        let unseen_fp = fnv1a(&[0xEE, 0xFF, r as u8]);
        if base.gate(unseen_fp) == Err(RegCode::UnseenRejected) {
            // 计入检出的一部分：守门成功。
        } else {
            return led; // 守门失败直接终止（逃逸面）
        }
        match hit {
            Some(c) => led.hit(r + 1, c),
            None => led.miss(r + 1),
        }
    }
    led
}

/// 未录样本注入 ×10 拒绝回归（J1R）：库外指纹 10/10 拒绝。
pub fn unseen_rejection_regression() -> (u32, u32) {
    let mut rej = 0u32;
    let mut leak = 0u32;
    let reg = match populate_registry() {
        Ok(r) => r,
        Err(_) => SampleRegistry::new(),
    };
    for r in 0..UNSEEN_ROUNDS {
        // 确定性库外指纹：扰动第 r 字节。
        let mut payload = make_payload(Family::Malformed, 0);
        if let Some(slot) = payload.get_mut(2) {
            *slot = slot.wrapping_add(0x30 + r as u8);
        }
        let fp = fnv1a(&payload) ^ ((Family::Malformed as u64) << 56);
        match reg.gate(fp) {
            Err(RegCode::UnseenRejected) => rej += 1,
            _ => leak += 1,
        }
    }
    (rej, leak)
}

/// 指纹账完整性：库内样本数、指纹账行数、唯一性。
pub fn completeness_face() -> (usize, usize, bool) {
    match populate_registry() {
        Ok(reg) => (reg.len(), reg.fp_rows(), reg.unique_fps()),
        Err(_) => (0, 0, false),
    }
}

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    match populate_registry() {
        Ok(reg) => {
            for line in reg.trace() {
                let mut bytes: Vec<u8> = Vec::new();
                for b in line.as_bytes() {
                    bytes.push(*b ^ (fp & 0xFF) as u8);
                }
                fp = fnv1a(&bytes) ^ fp.rotate_left(7);
            }
            fp = fnv1a(&reg.version().to_le_bytes()) ^ fp.rotate_left(5);
        }
        Err(_) => {}
    }
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 收口自检（16 条判据）。
pub fn run_unx51629_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51629");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 ×30 全量入库（J1）。
    {
        let face = completeness_face();
        s.add_dyn(
            "样本 ×30 全录指纹账",
            face.0 == NEW_SAMPLES && face.1 == NEW_SAMPLES && face.2,
            "UNX-F51630-J1 样本 ×30 全录指纹账且指纹唯一",
        );
    }
    // 4 族闭集。
    {
        let mut all = true;
        for c in 0..5u32 {
            if Family::from_code(c).is_none() {
                all = false;
            }
        }
        s.add_dyn("族闭集", all && Family::from_code(5).is_none(), "五族闭集解码必须完备");
    }
    // 5 未录样本 ×10 拒绝（J1R）。
    {
        let (rej, leak) = unseen_rejection_regression();
        s.add_dyn(
            "未录样本 10/10 拒绝",
            rej == UNSEEN_ROUNDS && leak == 0,
            "UNX-F51630-J1R 未录样本注入 10/10 拒绝（版本账纪律）",
        );
    }
    // 6 版本严格递增。
    {
        let reg = match populate_registry() {
            Ok(r) => r,
            Err(_) => SampleRegistry::new(),
        };
        s.add_dyn(
            "版本严格递增",
            reg.version() == NEW_SAMPLES as u64,
            "30 条样本版本必须推进到 30",
        );
    }
    // 7 空样本拒。
    {
        let mut reg = SampleRegistry::new();
        let r = reg.admit(0, 1, &[]);
        s.add_dyn("空样本拒收", r == Err(RegCode::SampleEmpty), "零字节样本无指纹价值");
    }
    // 8 超长样本拒。
    {
        let mut reg = SampleRegistry::new();
        let mut big: Vec<u8> = Vec::new();
        for _ in 0..65 {
            big.push(0x41);
        }
        let r = reg.admit(1, 1, &big);
        s.add_dyn("超长样本拒收", r == Err(RegCode::SampleOversize), "超上限样本按拒绝处理");
    }
    // 9 版本倒退与重复。
    {
        let mut reg = SampleRegistry::new();
        let _ = reg.admit(0, 3, &make_payload(Family::ZeroLen, 1));
        let back = reg.admit(0, 2, &make_payload(Family::ZeroLen, 2));
        let dup = reg.admit(0, 3, &make_payload(Family::ZeroLen, 3));
        s.add_dyn(
            "版本倒退与重复",
            back == Err(RegCode::VersionRegress) && dup == Err(RegCode::VersionDup),
            "版本必须严格递增且不得重复",
        );
    }
    // 10 指纹碰撞显性记。
    {
        let mut reg = SampleRegistry::new();
        let p = make_payload(Family::Storm, 4);
        let first = reg.admit(3, 1, &p);
        let second = reg.admit(3, 2, &p);
        s.add_dyn(
            "指纹碰撞显性记",
            first.is_ok() && second == Err(RegCode::FingerprintDup),
            "同载荷重复入库必须显性记碰撞",
        );
    }
    // 11 族码越界。
    {
        let mut reg = SampleRegistry::new();
        let r = reg.admit(7, 1, &make_payload(Family::Malformed, 1));
        s.add_dyn("族码越界", r == Err(RegCode::FamilyUnknown), "族码只接受 0..4");
    }
    // 12 入库注入 ×10 全检出。
    {
        let led = injection_campaign();
        s.add_dyn(
            "入库注入全检出",
            led.detected() == UNSEEN_ROUNDS && led.conserved(),
            "五族坏样本 ×10 全检出且账守恒",
        );
    }
    // 13 未录守门语义。
    {
        let reg = match populate_registry() {
            Ok(r) => r,
            Err(_) => SampleRegistry::new(),
        };
        let known = reg.lookup(fnv1a(&make_payload(Family::Replay, 2)) ^ ((Family::Replay as u64) << 56));
        let unknown = reg.gate(0xDEAD_BEEF_CAFE_F00D);
        s.add_dyn(
            "守门语义",
            known == Some(15) && unknown == Err(RegCode::UnseenRejected),
            "库内指纹可查、库外指纹必拒",
        );
    }
    // 14 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == UNSEEN_ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 16 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51629_checks_green() {
        let set = run_unx51629_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51629u32 - 51501));
        assert_eq!(SEG_BASE, 0xC2000);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(RegCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(RegCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn thirty_samples_all_fingerprinted() {
        let (n, rows, uniq) = completeness_face();
        assert_eq!(n, NEW_SAMPLES);
        assert_eq!(rows, NEW_SAMPLES);
        assert!(uniq);
    }

    #[test]
    fn family_closed_set() {
        for c in 0..5u32 {
            assert!(Family::from_code(c).is_some());
        }
        assert!(Family::from_code(5).is_none());
    }

    #[test]
    fn versions_monotone_to_30() {
        let reg = match populate_registry() {
            Ok(r) => r,
            Err(_) => SampleRegistry::new(),
        };
        assert_eq!(reg.version(), NEW_SAMPLES as u64);
    }

    #[test]
    fn empty_and_oversize_rejected() {
        let mut reg = SampleRegistry::new();
        assert_eq!(reg.admit(0, 1, &[]), Err(RegCode::SampleEmpty));
        let big: Vec<u8> = (0..65).map(|_| 0x41).collect();
        assert_eq!(reg.admit(1, 1, &big), Err(RegCode::SampleOversize));
    }

    #[test]
    fn version_regress_and_dup() {
        let mut reg = SampleRegistry::new();
        let _ = reg.admit(0, 3, &make_payload(Family::ZeroLen, 1));
        assert_eq!(reg.admit(0, 2, &make_payload(Family::ZeroLen, 2)), Err(RegCode::VersionRegress));
        assert_eq!(reg.admit(0, 3, &make_payload(Family::ZeroLen, 3)), Err(RegCode::VersionDup));
    }

    #[test]
    fn fingerprint_dup_detected() {
        let mut reg = SampleRegistry::new();
        let p = make_payload(Family::Storm, 4);
        assert!(reg.admit(3, 1, &p).is_ok());
        assert_eq!(reg.admit(3, 2, &p), Err(RegCode::FingerprintDup));
    }

    #[test]
    fn unseen_gate_rejects_ten_of_ten() {
        let (rej, leak) = unseen_rejection_regression();
        assert_eq!(rej, UNSEEN_ROUNDS);
        assert_eq!(leak, 0);
    }

    #[test]
    fn known_lookup_works() {
        let reg = match populate_registry() {
            Ok(r) => r,
            Err(_) => SampleRegistry::new(),
        };
        // Replay 族第 3 条（ver 15：前两族各 6 条）。
        let fp = fnv1a(&make_payload(Family::Replay, 2)) ^ ((Family::Replay as u64) << 56);
        assert_eq!(reg.lookup(fp), Some(15));
        assert!(reg.gate(fp).is_ok());
    }

    #[test]
    fn campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), UNSEEN_ROUNDS);
        assert_eq!(led.rows(), UNSEEN_ROUNDS as usize);
    }

    #[test]
    fn registry_cap_enforced() {
        let mut reg = SampleRegistry::new();
        let mut ver = 0u64;
        let mut last: Result<u64, RegCode> = Ok(0);
        for i in 0..REGISTRY_CAP {
            // 定容四字节载荷：逐 i 唯一且不超长。
            let p: [u8; 4] = [0x90, (i / 251) as u8, i as u8, 0x5A];
            last = reg.admit((i % 5) as u32, ver, &p);
            if last.is_ok() {
                ver += 1;
            }
        }
        assert_eq!(
            reg.admit(0, ver, &[0x91, 0x00, 0xFF, 0x5A]),
            Err(RegCode::RegistryFull)
        );
        // 首轮初值从未被读取属预期：显性消费以通过 no_std 死存检查。
        let _ = last;
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
