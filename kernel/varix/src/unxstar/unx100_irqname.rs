//! unx100_irqname — UNX-F0911 设备中断命名与标识（人话可读名）。
//!
//! 判据原文（施工书锚点 UNX-F0911，CoRun·UNX-A2-B06 批）：
//! **UNX-F0911-J1 命名覆盖率 100%（全部注册中断有名），唯一性（重名
//! 10/10 次红账），人话输出联动（F0898 解释器含名字段）**。
//!
//! 职责定位：B06 中断面走到 F0910 已收编 INTx 兼容路径——路由、账
//! 本、审计全齐，但"第 27 号中断是谁的"仍然只能答向量号——这是
//! 匿名面。Windows 深在哪：设备管理器里每个中断都能定位到
//! "设备名+功能+资源视图"，WDF 的 InterruptTranslated 资源描述、
//! ETW 的 Interrupt/MSI 事件都带设备标识——**排障的第一动作是
//! 按名定位而不是按号猜**；内核调试器 !idt/!irq 输出必带符号名，
//! 无名中断在 Windows 生态里等于"不可排障"。Varix：①**命名
//! 强制**——注册进命名面的中断必须有名（空名/非法名显性拒），三
//! 段式人话名（设备段.功能段.实例段）字符集/长度/段数/保留字全
//! 规则化；②**唯一性硬账**——正查表（键→名）+反查表（名→键）
//! 双账，重名在入账口显性红（不静默覆盖不静默去重），10/10 注入
//! 全检出；③**覆盖率 100% 判据**——对账函数吃注册面键全集，逐键
//! 查命名账，缺一即立案（无名中断没有生存空间）；④**人话输出联
//! 动**——explain/humanize 体例与 F0898 对齐（行式输出含名字段），
//! 同输入两次输出逐字一致（确定性判据，无时钟无随机）。
//! 上游：F0901 注册面（键全集来源）、F0910 INTx 路径（设备进命名
//! 面）、F0908/F0909（路径形态来源）；下游：F0898 路由面人话化
//! （解释器消费本单名字段）、ktest B06。
//! 本单交付**命名规则 + 双账唯一性 + 覆盖率对账 + explain 体例
//! + 万级压测 + 违例注入 + 快照**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显性
//! 注入）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照
//! 往返；错误全显性带诊断码与建议。诊断码段 0x44（0x43 为 F0910
//! 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const IRQ_NAME_VERSION: &str = "B06-irqname-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x44 段，本单独占——0x43 为 F0910 所取）
// ---------------------------------------------------------------------------

/// 0x4401 空名（注册未带名——命名强制第一闸）。
pub const XC_EMPTY_NAME: u32 = 0x4401;
/// 0x4402 重名（名→键反查表已有他主——唯一性红账）。
pub const XC_DUP_NAME: u32 = 0x4402;
/// 0x4403 非法字符/形态（字符集外、空段、点/连字符打头结尾）。
pub const XC_BAD_CHARS: u32 = 0x4403;
/// 0x4404 超长（总长或单段超界）。
pub const XC_NAME_TOO_LONG: u32 = 0x4404;
/// 0x4405 保留字（整名命中保留字闭集——语义歧义禁用）。
pub const XC_RESERVED_WORD: u32 = 0x4405;
/// 0x4406 覆盖率缺口（注册面对账有名/总数≠100%——无名中断立案）。
pub const XC_COVERAGE_GAP: u32 = 0x4406;
/// 0x4407 解释器输出漂移（同输入两次 explain 逐字不一致）。
pub const XC_EXPLAIN_DRIFT: u32 = 0x4407;
/// 0x4408 注销后残留（正/反查表注销不同步——双账失守）。
pub const XC_RESIDUE_AFTER_UNREG: u32 = 0x4408;
/// 0x4409 改名越权（目标名已被占用或源键不存在）。
pub const XC_RENAME_VIOLATION: u32 = 0x4409;
/// 0x440B 段数越界（三段式界 1..=4 段被破）。
pub const XC_SEG_COUNT: u32 = 0x440B;
/// 0x440C 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x440C;
/// 0x440D 重复注册（同键二度入账——键也是唯一资源）。
pub const XC_DUP_KEY: u32 = 0x440D;
/// 0x440E 键无名（explain/rename/unregister 消费不存在的键）。
pub const XC_KEY_ABSENT: u32 = 0x440E;

// ---------------------------------------------------------------------------
// §1 命名规则（三段式人话名）
// ---------------------------------------------------------------------------

/// 名总长上界。
pub const NAME_LEN_MAX: usize = 48;
/// 单段长上界。
pub const SEG_LEN_MAX: usize = 16;
/// 段数上界（三段式惯例，容 1..=4）。
pub const SEG_COUNT_MAX: usize = 4;

/// 保留字闭集（整名禁用——语义歧义：这些词是面内术语不是设备名）。
pub const RESERVED_WORDS: [&str; 7] = [
    "reserved",
    "unnamed",
    "none",
    "null",
    "unknown",
    "pending",
    "default",
];

/// 名合法性：字符集 [a-z0-9.-]、非空、总长 1..=48、段数 1..=4、
/// 每段非空且长 1..=16、不以 '.'/'-' 开头结尾、整名不命中保留字。
///
/// 纯函数：规则单点定义，校验与注入共用同一口径。
pub fn validate_name(name: &str) -> Result<(), (u32, String)> {
    if name.is_empty() {
        return Err((XC_EMPTY_NAME, "空名——注册必须带人话名".to_string()));
    }
    if name.len() > NAME_LEN_MAX {
        return Err((
            XC_NAME_TOO_LONG,
            format!("名长 {} 超界 {}（建议：设备段.功能段.实例段）", name.len(), NAME_LEN_MAX),
        ));
    }
    // 保留字整名比对（先于字符集——语义错误优先报语义码）。
    for r in RESERVED_WORDS.iter() {
        if name == *r {
            return Err((
                XC_RESERVED_WORD,
                format!("整名命中保留字 \"{}\"——面内术语禁作设备名", r),
            ));
        }
    }
    // 打头/结尾禁 '.' 与 '-'（形态端正）。
    let head = name.as_bytes()[0];
    let tail = name.as_bytes()[name.len() - 1];
    if head == b'.' || head == b'-' {
        return Err((XC_BAD_CHARS, "名以 '.' 或 '-' 开头——形态非法".to_string()));
    }
    if tail == b'.' || tail == b'-' {
        return Err((XC_BAD_CHARS, "名以 '.' 或 '-' 结尾——形态非法".to_string()));
    }
    // 字符集：小写字母/数字/点/连字符。
    for b in name.bytes() {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-';
        if !ok {
            return Err((
                XC_BAD_CHARS,
                format!("字符 '{}' 不在合法集 [a-z0-9.-]", b as char),
            ));
        }
    }
    // 段切分：非空段、段数界、单段长界。
    let segs: Vec<&str> = name.split('.').collect();
    if segs.len() > SEG_COUNT_MAX {
        return Err((
            XC_SEG_COUNT,
            format!("段数 {} 超界 {}（三段式：设备.功能.实例）", segs.len(), SEG_COUNT_MAX),
        ));
    }
    for s in segs.iter() {
        if s.is_empty() {
            return Err((XC_BAD_CHARS, "存在空段（连续点或点端）——形态非法".to_string()));
        }
        if s.len() > SEG_LEN_MAX {
            return Err((
                XC_NAME_TOO_LONG,
                format!("段 \"{}\" 长 {} 超单段界 {}", s, s.len(), SEG_LEN_MAX),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §2 路径形态（F0908/F0909 同源闭集——explain 输出字段）
// ---------------------------------------------------------------------------

/// 中断路径形态：与 F0909 优先序同源（MSI-X > MSI > INTx > None）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathForm {
    /// MSI-X 多向量。
    MsiX,
    /// MSI 单向量。
    Msi,
    /// INTx 传统引脚。
    Intx,
    /// 未定路径（命名先于路由定档——允许，explain 显性标注）。
    None,
}

impl PathForm {
    /// 全态枚举。
    pub const fn all() -> [PathForm; 4] {
        [PathForm::MsiX, PathForm::Msi, PathForm::Intx, PathForm::None]
    }

    /// 下标。
    pub const fn idx(self) -> usize {
        match self {
            PathForm::MsiX => 0,
            PathForm::Msi => 1,
            PathForm::Intx => 2,
            PathForm::None => 3,
        }
    }

    /// 人话名（explain 消费）。
    pub const fn name(self) -> &'static str {
        match self {
            PathForm::MsiX => "msix",
            PathForm::Msi => "msi",
            PathForm::Intx => "intx",
            PathForm::None => "未定档",
        }
    }
}

// ---------------------------------------------------------------------------
// §3 命名账（双账唯一性 + 覆盖率对账）
// ---------------------------------------------------------------------------

/// 一条有名中断。
#[derive(Clone, Debug)]
pub struct NamedIrq {
    /// 权威名（已过 validate_name 闸）。
    pub name: String,
    /// 路径形态。
    pub form: PathForm,
    /// 逻辑向量/实例号（注册面语义透传）。
    pub vector: u32,
    /// 注册时刻（tick 显性注入——零墙钟）。
    pub registered_at: u64,
    /// 改名前名（未改名则 None——改名审计链）。
    pub renamed_from: Option<String>,
}

/// 命名账：正查表（键→项）+ 反查表（名→键）双账唯一。
///
/// 唯一性纪律：入账口同时写双表，重名在反查表显性红——不静默
/// 覆盖、不静默去重；注销双表同清，残留即 0x4408 立案。
#[derive(Default)]
pub struct NameLedger {
    /// 正查表：中断键 → 命名项。
    pub by_key: BTreeMap<u32, NamedIrq>,
    /// 反查表：人话名 → 中断键（唯一性硬账）。
    pub by_name: BTreeMap<String, u32>,
    /// 注册总数。
    pub registered_total: u64,
    /// 注销总数。
    pub unregistered_total: u64,
    /// 改名总数。
    pub rename_total: u64,
    /// 重名红账计数（判据：重名 10/10 次红——每检出一笔计一笔）。
    pub duplicate_red_total: u64,
    /// 非法名拒绝计数（形态/长度/保留字/空名）。
    pub reject_total: u64,
}

impl NameLedger {
    /// 新账。
    pub fn new() -> NameLedger {
        NameLedger {
            by_key: BTreeMap::new(),
            by_name: BTreeMap::new(),
            registered_total: 0,
            unregistered_total: 0,
            rename_total: 0,
            duplicate_red_total: 0,
            reject_total: 0,
        }
    }

    /// 注册并命名：名闸→键唯一→名唯一→双账入。
    pub fn register(
        &mut self,
        key: u32,
        name: &str,
        form: PathForm,
        vector: u32,
        tick: u64,
    ) -> Result<(), (u32, String)> {
        // 名闸（空名/非法形态/超长/保留字）。
        if let Err(e) = validate_name(name) {
            self.reject_total += 1;
            return Err(e);
        }
        // 键唯一（同键二度入账显性拒——键与名同为唯一资源）。
        if self.by_key.contains_key(&key) {
            self.reject_total += 1;
            return Err((
                XC_DUP_KEY,
                format!("键 #{} 已注册为 \"{}\"——重复注册拒", key, self.by_key[&key].name),
            ));
        }
        // 名唯一（重名红账——判据唯一性的检出点）。
        if self.by_name.contains_key(name) {
            self.duplicate_red_total += 1;
            return Err((
                XC_DUP_NAME,
                format!(
                    "名 \"{}\" 已被键 #{} 持有——重名红账（可改实例段后重试）",
                    name, self.by_name[name]
                ),
            ));
        }
        self.by_name.insert(name.to_string(), key);
        self.by_key.insert(
            key,
            NamedIrq {
                name: name.to_string(),
                form,
                vector,
                registered_at: tick,
                renamed_from: None,
            },
        );
        self.registered_total += 1;
        Ok(())
    }

    /// 注销：双表同清；任一表残留即红（调用方对 0x4408 负责——本
    /// 函数自身保证同步，测试面用快照差验证）。
    pub fn unregister(&mut self, key: u32) -> Result<(), (u32, String)> {
        let item = match self.by_key.remove(&key) {
            Some(v) => v,
            None => {
                return Err((
                    XC_KEY_ABSENT,
                    format!("键 #{} 无名可注——消费不存在的键", key),
                ))
            }
        };
        self.by_name.remove(&item.name);
        self.unregistered_total += 1;
        Ok(())
    }

    /// 显性改名：旧名释放、新名占位、改名链记录（renamed_from）。
    pub fn rename(&mut self, key: u32, new_name: &str) -> Result<(), (u32, String)> {
        if let Err(e) = validate_name(new_name) {
            self.reject_total += 1;
            return Err(e);
        }
        if !self.by_key.contains_key(&key) {
            return Err((
                XC_RENAME_VIOLATION,
                format!("键 #{} 不存在——改名无源", key),
            ));
        }
        if self.by_name.contains_key(new_name) {
            self.duplicate_red_total += 1;
            return Err((
                XC_RENAME_VIOLATION,
                format!(
                    "目标名 \"{}\" 已被键 #{} 占用——改名越权",
                    new_name, self.by_name[new_name]
                ),
            ));
        }
        // BTreeMap 双向改写（取旧项→改→回插；零 unwrap：两步都带
        // 存在性前置检查，remove 后 insert 不可能失手）。
        let mut item = self.by_key.remove(&key).unwrap_or(NamedIrq {
            name: String::new(),
            form: PathForm::None,
            vector: 0,
            registered_at: 0,
            renamed_from: None,
        });
        self.by_name.remove(&item.name);
        item.renamed_from = Some(item.name.clone());
        item.name = new_name.to_string();
        self.by_name.insert(new_name.to_string(), key);
        self.by_key.insert(key, item);
        self.rename_total += 1;
        Ok(())
    }

    /// 覆盖率对账：吃注册面键全集（F0901 面语义），逐键查名。
    ///
    /// 返回（有名数, 总数）；缺名即 Err 立案——判据"命名覆盖率
    /// 100%"的强制点。
    pub fn coverage_audit(&self, registry_keys: &[u32]) -> Result<(usize, usize), (u32, String)> {
        let total = registry_keys.len();
        let mut named = 0usize;
        let mut missing: Vec<u32> = Vec::new();
        for k in registry_keys.iter() {
            if self.by_key.contains_key(k) {
                named += 1;
            } else {
                missing.push(*k);
            }
        }
        if named != total {
            return Err((
                XC_COVERAGE_GAP,
                format!(
                    "覆盖率 {}/{} 缺口——无键集 {}（无名中断没有生存空间）",
                    named,
                    total,
                    missing
                        .iter()
                        .map(|k| format!("#{}", k))
                        .collect::<Vec<String>>()
                        .join(",")
                ),
            ));
        }
        Ok((named, total))
    }

    /// 反查：名→键。
    pub fn lookup(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// 正查：键→名（借用串）。
    pub fn name_of(&self, key: u32) -> Option<&str> {
        self.by_key.get(&key).map(|v| v.name.as_str())
    }
}

// ---------------------------------------------------------------------------
// §4 人话输出（F0898 explain/humanize 体例对齐——名字段联动）
// ---------------------------------------------------------------------------

/// explain 行格式单点定义（F0898 解释器消费本格式——名字段在此）。
///
/// 确定性纪律：纯查账拼接，无时钟无随机无迭代顺序漂移（BTreeMap
/// 键序）——同账两次输出逐字一致（0x4407 判据的保障本体）。
pub fn explain_one(ledger: &NameLedger, key: u32) -> Result<String, (u32, String)> {
    let item = match ledger.by_key.get(&key) {
        Some(v) => v,
        None => {
            return Err((
                XC_KEY_ABSENT,
                format!("键 #{} 未入命名账——explain 无源", key),
            ))
        }
    };
    let mut line = format!(
        "中断 #{} 名=\"{}\" 形态={} 向量={} 注册于 tick={}",
        key, item.name, item.form.name(), item.vector, item.registered_at
    );
    if let Some(old) = item.renamed_from.as_ref() {
        line.push_str(&format!(" 曾名=\"{}\"", old));
    }
    Ok(line)
}

/// humanize 全量：按键序逐行 explain（路由面行式体例）。
pub fn humanize_all(ledger: &NameLedger) -> String {
    let mut out = String::new();
    for (k, _) in ledger.by_key.iter() {
        // 键序遍历中的键必在账内——explain_one 不会 Err；若 Err 则
        // 跳过该行（防御式，正常路径不可达）。
        if let Ok(line) = explain_one(ledger, *k) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// 确定性对拍：同账两次 explain 全量逐字一致（0x4407 强制点）。
pub fn explain_determinism(ledger: &NameLedger) -> Result<usize, (u32, String)> {
    let a = humanize_all(ledger);
    let b = humanize_all(ledger);
    if a != b {
        return Err((
            XC_EXPLAIN_DRIFT,
            format!(
                "两次输出不等（len {} vs {}）——解释器含时钟/随机/顺序漂移",
                a.len(),
                b.len()
            ),
        ));
    }
    Ok(a.lines().count())
}

// ---------------------------------------------------------------------------
// §5 LCG（确定性仿真——Numerical Recipes 参数）
// ---------------------------------------------------------------------------

/// 线性同余发生器（a=1664525 c=1013904223——NR 1992 参数）。
pub struct Lcg(u64);

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 下一步（取高 33 位——低阶位周期性差）。
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }

    /// 半开区间 [0, n)。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §6 FNV-1a 快照
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// FNV-1a 32 位（字节序逐字节）。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---------------------------------------------------------------------------
// §7 万级压测（确定性设备名生成 + 覆盖率 + explain 对拍）
// ---------------------------------------------------------------------------

/// 设备词根表（压测名生成——确定性：词根×实例号）。
pub const ROOTS: [&str; 8] = ["nvme", "xhci", "ahci", "gpu", "net", "audio", "pciex", "stor"];

/// 压测规模：注册 1 万键。
pub const STRESS_KEYS: u32 = 10_000;

/// 压测入口：万键注册→覆盖率 100%→explain 确定性→双账守恒。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    let mut rng = Lcg::new(0x1159_11E5);
    // 1）万键注册：名 = 词根.irq.序号（唯一性由序号保证——LCG 只
    //    决定形态与向量注入的确定性）。
    for i in 0..STRESS_KEYS {
        let root = ROOTS[rng.below(ROOTS.len() as u32) as usize];
        let name = format!("{}.irq.{}", root, i);
        let form = PathForm::all()[rng.below(4) as usize];
        let vector = rng.below(256);
        led.register(i, &name, form, vector, i as u64)?;
    }
    // 2）账面守恒：注册总数==STRESS_KEYS，注销 0。
    if led.registered_total != STRESS_KEYS as u64 || led.unregistered_total != 0 {
        return Err((
            XC_SNAP_DRIFT,
            format!(
                "压测账面失守：注册={} 注销={}（期望 {} / 0）",
                led.registered_total, led.unregistered_total, STRESS_KEYS
            ),
        ));
    }
    // 3）覆盖率 100%（判据 J1-第一项的正主）。
    let keys: Vec<u32> = (0..STRESS_KEYS).collect();
    let (named, total) = led.coverage_audit(&keys)?;
    // 4）explain 确定性对拍（判据 J1-第三项的强制面）。
    let lines = explain_determinism(&led)?;
    if lines != STRESS_KEYS as usize {
        return Err((
            XC_EXPLAIN_DRIFT,
            format!("explain 行数 {} ≠ 注册数 {}", lines, STRESS_KEYS),
        ));
    }
    // 5）双账互证：正查表每键的名在反查表回指同键。
    for (k, item) in led.by_key.iter() {
        match led.by_name.get(&item.name) {
            Some(k2) if k2 == k => {}
            _ => {
                return Err((
                    XC_RESIDUE_AFTER_UNREG,
                    format!("双账失守：键 #{} 名 \"{}\" 反查不回指", k, item.name),
                ))
            }
        }
    }
    // 6）抽样正查：任意键的名可被 lookup 回键。
    for _ in 0..100u32 {
        let k = rng.below(STRESS_KEYS);
        let n = match led.name_of(k) {
            Some(s) => s.to_string(),
            None => {
                return Err((XC_KEY_ABSENT, format!("抽样键 #{} 无名", k)))
            }
        };
        if led.lookup(&n) != Some(k) {
            return Err((
                XC_RESIDUE_AFTER_UNREG,
                format!("抽样反查失守：名 \"{}\" 回键≠#{}", n, k),
            ));
        }
    }
    Ok(format!(
        "万级压测全绿：注册 {} 覆盖 {}/{} explain {} 行逐字一致 双账互证过",
        STRESS_KEYS, named, total, lines
    ))
}

// ---------------------------------------------------------------------------
// §8 注入面（唯一性 10/10 + 非法名 10 类 + 覆盖缺口 + 注销残留
//        + 改名链 + 重复键）
// ---------------------------------------------------------------------------

/// 唯一性注入：重名 10/10 次红账（判据 J1-第二项正主）。
pub fn inject_dup_names_10() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    for i in 0..10u8 {
        // 先注册键 A 持名，再以键 B 撞同名——必红 0x4402。
        let name = format!("dup.irq.{}", i);
        led.register(100 + i as u32, &name, PathForm::Intx, i as u32, i as u64)?;
        let r = led.register(200 + i as u32, &name, PathForm::Msi, i as u32, i as u64);
        let red = matches!(r, Err((XC_DUP_NAME, _)));
        hits.push((i, red, match r {
            Err((code, m)) => format!("code={:#06x} {}", code, m),
            Ok(_) => "未拦——重名入账（唯一性失守）".to_string(),
        }));
    }
    // 红账计数同步：10 次检出==duplicate_red_total。
    if led.duplicate_red_total != 10 {
        return Err((
            XC_DUP_NAME,
            format!("红账计数 {} ≠ 检出 10——账实不符", led.duplicate_red_total),
        ));
    }
    // 撞名键 B 不得入正查表（拒=不入账）。
    if led.by_key.contains_key(&200) {
        return Err((XC_DUP_NAME, "被拒键 #200 仍入正查表——假拒".to_string()));
    }
    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_DUP_NAME, format!("重名检出缺口：{}", names.join("；"))));
    }
    Ok(format!("重名 10/10 全红账（计数 {}）", led.duplicate_red_total))
}

/// 非法名 10 类注入：闸口全拦（每类对准一个规则分支）。
pub fn inject_bad_names_10() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    let mut hits: Vec<(u8, bool, String)> = Vec::new();
    // 1 空名。
    hits.push((1, matches!(validate_name(""), Err((XC_EMPTY_NAME, _))),
        "空名未拦".to_string()));
    // 2 超总长（49 字节）。
    let long49 = "a".repeat(49);
    hits.push((2, matches!(validate_name(&long49), Err((XC_NAME_TOO_LONG, _))),
        "超总长未拦".to_string()));
    // 3 大写（字符集外）。
    hits.push((3, matches!(validate_name("NVME.irq.1"), Err((XC_BAD_CHARS, _))),
        "大写未拦".to_string()));
    // 4 非法字符（空格）。
    hits.push((4, matches!(validate_name("nvme irq 1"), Err((XC_BAD_CHARS, _))),
        "空格未拦".to_string()));
    // 5 空段（连续点）。
    hits.push((5, matches!(validate_name("nvme..irq"), Err((XC_BAD_CHARS, _))),
        "空段未拦".to_string()));
    // 6 超段（5 段）。
    hits.push((6, matches!(validate_name("a.b.c.d.e"), Err((XC_SEG_COUNT, _))),
        "超段未拦".to_string()));
    // 7 保留字。
    hits.push((7, matches!(validate_name("reserved"), Err((XC_RESERVED_WORD, _))),
        "保留字未拦".to_string()));
    // 8 点结尾。
    hits.push((8, matches!(validate_name("nvme."), Err((XC_BAD_CHARS, _))),
        "点结尾未拦".to_string()));
    // 9 点开头。
    hits.push((9, matches!(validate_name(".nvme"), Err((XC_BAD_CHARS, _))),
        "点开头未拦".to_string()));
    // 10 连字符开头。
    hits.push((10, matches!(validate_name("-nvme"), Err((XC_BAD_CHARS, _))),
        "连字符开头未拦".to_string()));
    // 计数同步走 register 路径（纯函数注入不计账面——账面由闸口计）。
    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_BAD_CHARS, format!("非法名闸口漏网：{}", names.join("；"))));
    }
    // register 路径计数验证：非法名入账被拒且计数抬。
    let r = led.register(1, "", PathForm::None, 0, 0);
    let counted = led.reject_total == 1;
    if !(matches!(r, Err((XC_EMPTY_NAME, _))) && counted) {
        return Err((
            XC_EMPTY_NAME,
            format!("register 空名拒/{}/计数 {}——闸口或账面失守",
                r.is_err(), led.reject_total),
        ));
    }
    Ok("非法名 10 类全拦 + register 计数对拍过".to_string())
}

/// 覆盖缺口注入：注册面键集含未命名键→对账必红；补名后转绿。
pub fn inject_coverage_gap() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    // 注册面 6 键，命名账只收 4 键——缺口 2 键。
    for i in 0..4u32 {
        led.register(i, &format!("dev.irq.{}", i), PathForm::MsiX, i, i as u64)?;
    }
    let keys: Vec<u32> = vec![0, 1, 2, 3, 4, 5];
    let r = led.coverage_audit(&keys);
    if !matches!(r, Err((XC_COVERAGE_GAP, _))) {
        return Err((
            XC_COVERAGE_GAP,
            format!("覆盖缺口未立案：{:?}", r.map(|(a, b)| (a, b))),
        ));
    }
    // 补名 2 键→转绿（覆盖率 100%）。
    led.register(4, "dev.irq.4", PathForm::MsiX, 4, 4)?;
    led.register(5, "dev.irq.5", PathForm::Intx, 5, 5)?;
    let (named, total) = led.coverage_audit(&keys)?;
    if named != total || named != 6 {
        return Err((
            XC_COVERAGE_GAP,
            format!("补名后 {}/{} ≠ 6/6——判据面失守", named, total),
        ));
    }
    Ok("覆盖缺口立案→补名转绿 6/6（覆盖率判据双向验证）".to_string())
}

/// 注销残留注入：双账同清 + 残留检出 + 消费不存在键显性红。
pub fn inject_unreg_residue() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    led.register(7, "res.irq.7", PathForm::Msi, 7, 0)?;
    led.unregister(7)?;
    // 残留审计：正反双表必须同时无痕。
    let residue = led.by_key.contains_key(&7) || led.by_name.contains_key("res.irq.7");
    if residue {
        return Err((XC_RESIDUE_AFTER_UNREG, "注销后双账有残留".to_string()));
    }
    // 消费不存在键：explain/unregister 显性红（不静默返回空）。
    let r1 = explain_one(&led, 7);
    let r2 = led.unregister(7);
    if !(matches!(r1, Err((XC_KEY_ABSENT, _))) && matches!(r2, Err((XC_KEY_ABSENT, _)))) {
        return Err((
            XC_KEY_ABSENT,
            format!("不存在键消费未显性红：explain_err={}/unreg_err={}",
                r1.is_err(), r2.is_err()),
        ));
    }
    // 账面守恒：注册 1 注销 1。
    if led.registered_total != 1 || led.unregistered_total != 1 {
        return Err((
            XC_SNAP_DRIFT,
            format!("账面失守：注册 {} 注销 {}", led.registered_total, led.unregistered_total),
        ));
    }
    Ok("注销双账同清 + 不存在键显性红 + 账面守恒".to_string())
}

/// 改名链注入：显性改名记录曾名 + 目标名占用拦截 + 改名后 explain 联动。
pub fn inject_rename_chain() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    led.register(9, "old.irq.9", PathForm::Intx, 9, 0)?;
    // 正常改名：曾名链记录。
    led.rename(9, "new.irq.9")?;
    let item = match led.by_key.get(&9) {
        Some(v) => v,
        None => return Err((XC_KEY_ABSENT, "改名后键丢失".to_string())),
    };
    let chain_ok = item.name == "new.irq.9"
        && item.renamed_from.as_deref() == Some("old.irq.9")
        && led.rename_total == 1;
    if !chain_ok {
        return Err((XC_RENAME_VIOLATION, "改名链未记录（名/曾名/计数）".to_string()));
    }
    // 旧名反查必须释放、新名反查命中。
    if led.lookup("old.irq.9").is_some() || led.lookup("new.irq.9") != Some(9) {
        return Err((XC_RESIDUE_AFTER_UNREG, "改名后旧名未释放或新名未命中".to_string()));
    }
    // 目标名占用拦截：先立键 10 持名，再让键 9 撞名——红且计数抬。
    led.register(10, "taken.irq.10", PathForm::Msi, 10, 1)?;
    let r = led.rename(9, "taken.irq.10");
    if !matches!(r, Err((XC_RENAME_VIOLATION, _))) || led.duplicate_red_total != 1 {
        return Err((
            XC_RENAME_VIOLATION,
            format!("改名撞名未拦/计数失守：red={}", led.duplicate_red_total),
        ));
    }
    // 改名后 explain 含曾名字段（人话联动）。
    let line = explain_one(&led, 9)?;
    if !line.contains("曾名=\"old.irq.9\"") {
        return Err((
            XC_EXPLAIN_DRIFT,
            format!("explain 未含曾名字段：{}", line),
        ));
    }
    Ok("改名链（曾名记录+旧名释放+撞名拦截+explain 联动）全过".to_string())
}

/// 重复键注入：同键二度入账显性红。
pub fn inject_dup_key() -> Result<String, (u32, String)> {
    let mut led = NameLedger::new();
    led.register(3, "first.irq.3", PathForm::MsiX, 3, 0)?;
    let r = led.register(3, "second.irq.3", PathForm::Msi, 3, 1);
    if !matches!(r, Err((XC_DUP_KEY, _))) {
        return Err((XC_DUP_KEY, "同键二度入账未拦".to_string()));
    }
    // 原名不得被二度注册覆盖。
    if led.name_of(3) != Some("first.irq.3") {
        return Err((XC_DUP_KEY, "重复注册覆盖了原名——静默覆盖失守".to_string()));
    }
    Ok("重复键显性红 + 原名保全".to_string())
}

// ---------------------------------------------------------------------------
// §9 快照
// ---------------------------------------------------------------------------

/// 命名账快照指纹。
pub struct NameSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl NameSnap {
    /// 采样：总账 + 双账键序内容。
    pub fn take(led: &NameLedger) -> NameSnap {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&led.registered_total.to_le_bytes());
        buf.extend_from_slice(&led.unregistered_total.to_le_bytes());
        buf.extend_from_slice(&led.rename_total.to_le_bytes());
        buf.extend_from_slice(&led.duplicate_red_total.to_le_bytes());
        buf.extend_from_slice(&led.reject_total.to_le_bytes());
        for (k, v) in led.by_key.iter() {
            buf.extend_from_slice(&k.to_le_bytes());
            buf.extend_from_slice(v.name.as_bytes());
            buf.push(v.form.idx() as u8);
            buf.extend_from_slice(&v.vector.to_le_bytes());
            buf.extend_from_slice(&v.registered_at.to_le_bytes());
        }
        NameSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!(
                "键={} 名={} 注册={} 注销={} 改名={} 重名红={} 拒={}",
                led.by_key.len(),
                led.by_name.len(),
                led.registered_total,
                led.unregistered_total,
                led.rename_total,
                led.duplicate_red_total,
                led.reject_total
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// §10 规则自检（validate 契约内证）
// ---------------------------------------------------------------------------

/// 命名规则自检：合法名样本全过 + 边界长恰过（48 过 49 拦）。
pub fn rule_selfcheck() -> Result<String, (u32, String)> {
    // 合法样本：单段/两段/三段/四段/含数字/含连字符/最长合法。
    let good = [
        "dev",
        "dev.irq",
        "nvme.irq.3",
        "a.b.c.d",
        "gpu2.irq.17",
        "pci-express.irq.0",
        &format!("{}.irq.{}", "x".repeat(16), "y".repeat(16)), // 16+5+16=37 ≤48
    ];
    for g in good.iter() {
        if let Err(e) = validate_name(g) {
            return Err((XC_BAD_CHARS, format!("合法名 \"{}\" 被误拦：{}", g, e.1)));
        }
    }
    // 边界：恰 48 过。
    let edge48 = "a".repeat(NAME_LEN_MAX);
    if validate_name(&edge48).is_err() {
        return Err((XC_NAME_TOO_LONG, "恰 48 被误拦——边界失守".to_string()));
    }
    // 保留字仅整名命中：含保留字的复合名合法（如 "reserved.irq.1"）。
    if validate_name("reserved.irq.1").is_err() {
        return Err((
            XC_RESERVED_WORD,
            "复合名含保留字被误拦——整名口径失守".to_string(),
        ));
    }
    Ok(format!("规则自检过：合法样本 {} 个 + 边界 48/49 + 整名保留字口径", good.len()))
}

// ---------------------------------------------------------------------------
// §11 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：规则自检 + 万级压测 + 六路注入 + 快照稳定，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(rule_selfcheck()?);
    parts.push(inject_dup_names_10()?);
    parts.push(inject_bad_names_10()?);
    parts.push(inject_coverage_gap()?);
    parts.push(inject_unreg_residue()?);
    parts.push(inject_rename_chain()?);
    parts.push(inject_dup_key()?);
    parts.push(stress_10k()?);
    // 快照稳定性（空账双采样）。
    let fresh = NameLedger::new();
    let s0 = NameSnap::take(&fresh);
    let s1 = NameSnap::take(&fresh);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空账快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x} {}）", s0.hash, s0.summary));
    Ok(parts.join("；"))
}
