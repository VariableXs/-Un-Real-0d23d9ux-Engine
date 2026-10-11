//! # VE-F5215 · 特效 fuzz（Z 域 · 特效库总架构组 · 目标 300 行 · 实现深化版）
//!
//! **职责定位（锚点原文全要素）**：特效 fuzz——特效 fuzz（四类目标：
//! 参数 schema 畸形/组合配置越界/资源引用伪造/闪烁参数边界值；语料：
//! 真实特效配置变形+语法生成；发现即立案——光敏边界重点覆盖）。
//!
//! **数据结构（锚点原文）**：fuzz 语料池；语法生成器；立案钩子。
//!
//! **错误路径与降级矩阵（锚点原文 · F4943 矩阵接入——三行三裁决闭集）**：
//! - 崩溃（防线失守）→ 最小化修复（保崩删减到最小形——修最小的一处）；
//! - 伪造（引用不在真实库面）→ 校验拦截（同源口径——分叉引用不进管线）；
//! - 边界（光敏恰界与越界）→ 光敏检测验证（F5211 红线上游——恰界过/越界拦+修正幂等）。
//!
//! **性能逐项分解（锚点原文）**：每轮 O(语料数)——run_round 取干语料池；
//! 最小化 O(输入)——段删减每步重放探测器；限额 O(1)——记账闸先查后放。
//!
//! **跨批对接点（锚点原文）**：F5211 红线上游（光敏边界直打 FlashGate）；
//! F4943 矩阵接入（错误路径降级矩阵——本头注三行+元-03 对拍）。消费接缝：
//! F5207（import_preset 三闸+Validator 钳制——schema 畸形语料直打）、
//! F5208（StackResolver/ComboBudget——越界组合直打）、F5211（FlashGate——
//! 光敏边界直打）、F5214（fnv64 指纹同源——语料去重同口径）。
//!
//! **无障碍与隐私（锚点原文）**：无独立交互面；报告脱敏——立案簿条目
//! 只含编号/目标/指纹/裁决码，永不含原始输入内容（fuzz 输入是攻击面，
//! 报告自身不得成为隐私面或注入面）。
//!
//! **设计总纲**：fuzz 的「崩溃」在零 panic 面的真实现上映射为「防线失守」
//! ——应拒未拒即崩溃级发现（拦截漏=崩）。四类探测器直打真实现（F5207/
//! F5208/F5211），防线全活则语料全 Clean；最小化器/立案簿/限额闸走谓词
//! 驱动的真实逻辑路径（探测器即谓词——重放验证保崩语义）。全部整数面
//! （无 f32 无 libm——内核面纪律），确定性生成（同种子同语料——可复现
//! 立案，Y 域确定优先律延伸）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、Z 域字符串码：五码互异 + reason_of / direction_of 三要素
// ---------------------------------------------------------------------------

/// 防线失守立案：应拒未拒（拦截漏=崩）——崩溃→最小化修复。
pub const E_FZ_CRASH: &str = "E_FZ_CRASH";
/// 伪造引用立案钩子对偶：伪造引用被校验拦截（拦截成功记 Clean，案簿
/// 只收失守）——伪造→校验拦截。
pub const E_FZ_FORGED: &str = "E_FZ_FORGED";
/// 光敏边界立案：越界闪烁参数被放行（光敏红线失守——最高优先案）。
pub const E_FZ_BOUNDARY: &str = "E_FZ_BOUNDARY";
/// 最小化失败：删减过程丢失崩溃（保崩语义破坏——案不可归约即不可修）。
pub const E_FZ_MINFAIL: &str = "E_FZ_MINFAIL";
/// 限额超：单轮语料数或立案总数超记账闸（预算防护——拒绝继续灌入）。
pub const E_FZ_QUOTA: &str = "E_FZ_QUOTA";

/// 五码计数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→人读理由（三要素之一）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_FZ_CRASH => "防线失守：应拒语料未被拒（拦截漏=崩）",
        E_FZ_FORGED => "伪造引用：引用名不在真实库面",
        E_FZ_BOUNDARY => "光敏边界：越界闪烁参数被放行",
        E_FZ_MINFAIL => "最小化失败：删减丢失崩溃——案不可归约",
        E_FZ_QUOTA => "限额超：语料数或立案数超记账闸",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向（三要素之二）。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_FZ_CRASH => "最小化到最小失守形后修复拦截",
        E_FZ_FORGED => "同源口径拦截并记 Clean",
        E_FZ_BOUNDARY => "光敏红线阻断+强制修正并验证幂等",
        E_FZ_MINFAIL => "回滚到上一保崩形重新归约",
        E_FZ_QUOTA => "停止本轮灌入并复核闸值",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// 二、目标闭集 + 确定性随机（LCG）+ 指纹（消费 F5214 fnv64 同口径）
// ---------------------------------------------------------------------------

/// 四类 fuzz 目标闭集（锚点原文四类——顺序即探测优先序）。
pub const FUZZ_TARGETS: [&str; 4] = [
    "schema_malformed",
    "combo_overreach",
    "resource_forged",
    "flash_boundary",
];

/// 确定性线性同余（同种子同序列——立案可复现，确定优先律延伸）。
pub const fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// FNV-1a 64 位语料指纹（消费 F5214 同款实现——去重与脱敏报告同口径）。
pub fn fnv64(bytes: &[u8]) -> u64 {
    crate::svstar2::vez14_examples::fnv64(bytes)
}

/// FNV-1a 16 位（与 F5207 校验和同口径——合法种子语料需要配正确校验和）。
pub const fn fnv16(bytes: &[u8]) -> u16 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    ((h >> 48) as u16) ^ ((h >> 32) as u16)
}

/// 四位十六进制校验和尾（与 F5207 导出格式一致——`{:04x}`）。
pub fn sum_tail(body: &str) -> String {
    format!("{:04x}", fnv16(body.as_bytes()))
}

// ---------------------------------------------------------------------------
// 三、语料池：真实配置种子 + 变形算子 + 指纹去重 + 每轮取干
// ---------------------------------------------------------------------------

/// 变形算子闭集（真实特效配置变形——每算子一种破坏面）。
pub const MUTATORS: [&str; 10] = [
    "flip_head",     // 头段翻改：PRM1→PRM2
    "drop_tail",     // 删校验和段
    "bad_sum",       // 校验和改错
    "empty_name",    // 名段置空
    "oversize_val",  // 值改 i64 上界
    "neg_val",       // 值取负
    "dup_key",       // 键重复
    "truncate_half", // 截半
    "forged_ref",    // 引用名伪造
    "flash_over",    // 闪烁越界（光敏重点）
];

/// 单条语料：文本 + 来源标记 + 指纹（去重键）。
#[derive(Clone, Debug)]
pub struct CorpusItem {
    /// 语料文本（变形产物或语法生成产物）。
    pub text: String,
    /// 来源（seed=真实配置 / mut=变形 / gen=语法生成）。
    pub origin: &'static str,
    /// FNV-64 指纹（去重键——池内不收重）。
    pub fp: u64,
}

/// fuzz 语料池：指纹去重 + 每轮取干（每轮 O(语料数)——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct CorpusPool {
    items: Vec<CorpusItem>,
    /// 去重拒收计数。
    pub dup_blocked: u32,
    /// 种子计数。
    pub seeds: u32,
}

impl CorpusPool {
    /// 空池。
    pub fn new() -> Self {
        CorpusPool::default()
    }

    /// **入池**（指纹去重——池内不收重；去重 O(池) 与锚点每轮口径一致）。
    pub fn add(&mut self, text: &str, origin: &'static str) -> bool {
        let fp = fnv64(text.as_bytes());
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].fp == fp {
                self.dup_blocked += 1;
                return false;
            }
            i += 1;
        }
        if origin == "seed" {
            self.seeds += 1;
        }
        self.items.push(CorpusItem { text: String::from(text), origin, fp });
        true
    }

    /// **每轮取干**：取出全部语料并清池（每轮 O(语料数)——锚点口径）。
    pub fn drain(&mut self) -> Vec<CorpusItem> {
        let out = core::mem::take(&mut self.items);
        out
    }

    /// 池中语料数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// **注入真实配置种子**（真实特效配置——变形的原料必须来自真实面）：
    /// 三族种子各一条（preset/卡/闪烁），全部配合法校验和或合法参数。
    pub fn seed_real(&mut self) -> usize {
        let body1 = "ex_intensity=40;ex_speed=20";
        let s1 = format!("PRM1|{}|{}", body1, sum_tail(body1));
        let n0 = self.len();
        let _ = self.add(&s1, "seed");
        let _ = self.add("combo:particles:Additive:5:10", "seed");
        let _ = self.add("flash:hz=2;depth=20", "seed");
        self.len() - n0
    }
}

/// **变形一例**（真实配置变形——单算子单破坏面；变形对合法种子做减法
/// 或替换，产出应被防线拦截的畸形语料）。
pub fn mutate_once(seed: &str, op: &str) -> String {
    match op {
        // 头段翻改：PRM1 → PRM2（闸 1 应拒）。
        "flip_head" => seed.replacen("PRM1", "PRM2", 1),
        // 删校验和段：三段变两段（闸 1 段数应拒）。
        "drop_tail" => {
            let mut parts: Vec<&str> = seed.split('|').collect();
            if parts.len() >= 3 {
                parts.pop();
                parts.join("|")
            } else {
                String::from(seed)
            }
        }
        // 校验和改错：尾段改 0000（除非恰为 0000——那改 ffff）。
        "bad_sum" => {
            let body = seed.split('|').nth(1).unwrap_or("");
            let real = sum_tail(body);
            let fake: &str = if real == "0000" { "ffff" } else { "0000" };
            let mut parts: Vec<&str> = seed.split('|').collect();
            let n = parts.len();
            if n >= 3 {
                parts[n - 1] = fake;
                parts.join("|")
            } else {
                String::from(seed)
            }
        }
        // 名段置空：首个键名清空（闸 2 应拒空名）。
        "empty_name" => seed.replacen("ex_intensity=", "=40", 1),
        // 值改上界：i64::MAX 灌入（解析应过、schema 应钳——钳制漏即案）。
        "oversize_val" => seed.replacen("ex_intensity=40", "ex_intensity=9223372036854775807", 1),
        // 值取负：负值灌入（负深度越光敏界——红线应拦）。
        "neg_val" => seed.replacen("depth=20", "depth=-5", 1),
        // 键重复：同键双写（upsert 语义合法——防线应放行不崩）。
        "dup_key" => seed.replacen("ex_speed=20", "ex_speed=20;ex_speed=21", 1),
        // 截半：文本腰斩（半段语料——所有闸都应拒）。
        "truncate_half" => {
            let b = seed.as_bytes();
            let half = b.len() / 2;
            // 按字节切——UTF-8 边界防呆：向后退到字符边界。
            let mut cut = half;
            while cut > 0 && !seed.is_char_boundary(cut) {
                cut -= 1;
            }
            String::from_utf8_lossy(&b[..cut]).into_owned()
        }
        // 引用伪造：真实库面外名字（同源口径应拦）。
        "forged_ref" => String::from("ref:MagicFx::abracadabra"),
        // 闪烁越界：hz=4 超 3Hz 红线（光敏重点覆盖）。
        "flash_over" => seed.replacen("hz=2", "hz=4", 1),
        _ => String::from(seed),
    }
}

/// **全算子变形**：一条种子过全部变形算子（变形 O(算子数)——语料灌池）。
pub fn mutate_all(seed: &str, pool: &mut CorpusPool) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MUTATORS.len() {
        let text = mutate_once(seed, MUTATORS[i]);
        if pool.add(&text, "mut") {
            n += 1;
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 四、语法生成器：确定性畸形语料（同种子同序列——立案可复现）
// ---------------------------------------------------------------------------

/// 语法生成器（LCG 确定性——同种子产出全等序列）。
///
/// 生成三维正交畸形族：头形 × 体形 × 尾形（preset 格式），外加闪烁与
/// 引用两族——与变形算子互补（变形保结构、生成破结构）。
#[derive(Clone, Debug)]
pub struct GrammarGen {
    state: u64,
    /// 已生成条数。
    pub emitted: u32,
}

/// 头形闭集（闸 1 面）。
pub const HEAD_FORMS: [&str; 4] = ["PRM1", "PRM2", "", "PRM1X"];

/// 体形闭集（闸 2 面——键值对畸形）。
pub const BODY_FORMS: [&str; 6] = [
    "a=1;b=2",      // 合法对（对照组——生成器也产合法语料做对照）
    "a",            // 缺等号
    "a=1=2",        // 双等号
    "a=999",        // 大值（schema 面管界）
    "a=-1",         // 负值
    ";a=1;",        // 空段夹合法
];

/// 尾形闭集（闸 3 面）。
pub const TAIL_FORMS: [&str; 3] = ["good", "bad", "none"];

impl GrammarGen {
    /// 以种子建生成器。
    pub fn new(seed: u64) -> Self {
        GrammarGen { state: seed | 1, emitted: 0 }
    }

    /// 推进随机态并取回旧值（确定性——只依赖种子）。
    fn draw(&mut self) -> u64 {
        self.state = lcg_next(self.state);
        self.state
    }

    /// **生成一条 preset 语料**（头形×体形×尾形三维正交——LCG 选形；
    /// 尾形 good=正确校验和、bad=错校验和、none=缺尾段）。
    pub fn next_preset(&mut self) -> String {
        let d1 = self.draw();
        let d2 = self.draw();
        let d3 = self.draw();
        let head = HEAD_FORMS[(d1 as usize) % HEAD_FORMS.len()];
        let body = BODY_FORMS[(d2 as usize) % BODY_FORMS.len()];
        let tail = TAIL_FORMS[(d3 as usize) % TAIL_FORMS.len()];
        self.emitted += 1;
        match tail {
            "good" => format!("{}|{}|{}", head, body, sum_tail(body)),
            "bad" => {
                let real = sum_tail(body);
                let fake = if real == "0000" { String::from("ffff") } else { String::from("0000") };
                format!("{}|{}|{}", head, body, fake)
            }
            _ => format!("{}|{}", head, body),
        }
    }

    /// **生成一条闪烁语料**（hz/depth 在 [0,8]×[-5,40] 扫描——恰界与
    /// 越界双向覆盖，光敏重点）。
    pub fn next_flash(&mut self) -> String {
        let d1 = self.draw();
        let d2 = self.draw();
        let hz = (d1 % 9) as i64; // 0..=8——覆盖恰界 3 与越界 4+
        let depth = (d2 % 46) as i64 - 5; // -5..=40——覆盖恰界 30 与越界 31+
        self.emitted += 1;
        format!("flash:hz={};depth={}", hz, depth)
    }

    /// **生成一条引用语料**（真实面名与伪造名交替——拦截与放行双向）。
    pub fn next_ref(&mut self) -> String {
        let d1 = self.draw();
        let pick = (d1 as usize) % 3;
        self.emitted += 1;
        match pick {
            0 => String::from("ref:OverlayRules::new"),
            1 => String::from("ref:StackResolver::push"),
            _ => String::from("ref:FakeFx::nonexistent"),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、四目标探测器：直打真实现（F5207 / F5208 / F5211 / 同源口径）
// ---------------------------------------------------------------------------

/// 单条语料探测裁决：Clean=防线拦住（或行为正确）；Suspect=防线失守
/// （应拒未拒——崩溃级发现，立案）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// 防线拦住或行为正确。
    Clean,
    /// 防线失守——附裁决码（E_FZ_*）。
    Suspect(&'static str),
}

/// 探测统计账（四目标分账——每轮汇总）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeStats {
    /// schema 语料探测数。
    pub schema_n: u32,
    /// 组合语料探测数。
    pub combo_n: u32,
    /// 引用语料探测数。
    pub forged_n: u32,
    /// 闪烁语料探测数。
    pub flash_n: u32,
    /// Clean 数。
    pub clean_n: u32,
    /// Suspect 数（=立案数）。
    pub suspect_n: u32,
}

/// **差分镜像**：本地重推 F5207 三闸语义（头形/字段可解析/校验和）——
/// 预言机面。真实现行为与镜像不一致 = 行为漂移（回归级发现——立案）。
pub fn expect_import_ok(text: &str) -> bool {
    let mut parts = text.split('|');
    let head = parts.next().unwrap_or("");
    if head != "PRM1" {
        return false; // 闸 1：头形
    }
    let body = parts.next().unwrap_or("");
    let tail = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return false; // 闸 1：恰三段
    }
    if !body.is_empty() {
        for pair in body.split(';') {
            let mut kv = pair.split('=');
            let k = kv.next().unwrap_or("");
            let v = kv.next().unwrap_or("");
            if k.is_empty() || kv.next().is_some() {
                return false; // 闸 2：名非空+恰一键值
            }
            if v.parse::<i64>().is_err() {
                return false; // 闸 2：值可解析
            }
        }
    }
    tail == sum_tail(body) // 闸 3：校验和一致
}

/// **目标一：参数 schema 畸形**（差分 fuzz——直打 F5207）。
///
/// preset 语料喂 [`import_preset`]，裁决与差分镜像 [`expect_import_ok`]
/// 对拍：真实现 Ok 且镜像 Ok → Clean（放行合法形——行为一致）；真实现
/// Err 且镜像 Err → Clean（拦截畸形形——行为一致）；两者错位 → 行为
/// 漂移 Suspect（三闸回归面——fuzz 真发现）。语料不含 `|`（PRM 残形）
/// → 格式违约 Suspect（防御性立案）。
pub fn probe_schema(text: &str) -> ProbeVerdict {
    if !text.contains('|') {
        return ProbeVerdict::Suspect(E_FZ_CRASH); // PRM 残形——格式违约
    }
    let mirror_ok = expect_import_ok(text);
    match crate::svstar2::vez07_params::import_preset(text) {
        Ok(_) if mirror_ok => ProbeVerdict::Clean,
        Err(_) if !mirror_ok => ProbeVerdict::Clean,
        _ => ProbeVerdict::Suspect(E_FZ_CRASH), // 行为漂移——三闸回归
    }
}

/// **schema 边界差分**（目标一的数值面——直打 F5207 钳制）。
///
/// 三界值扫描：250 越上界必钳 100、-1 越下界必钳 0、101 越上界必钳
/// 100——任一失钳（clamped 漏置位或值出界）即案。幂等注册：重名定义
/// 失败走既有 schema 继续（钳制差分不依赖注册序）。
pub fn probe_schema_boundary(v: &mut crate::svstar2::vez07_params::Validator) -> ProbeVerdict {
    let _ = v.book.define("fz_schema_p", 0, 100, 40, "fuzz 边界参数");
    let cases: [(i64, i64); 3] = [(250, 100), (-1, 0), (101, 100)];
    let mut i = 0usize;
    while i < cases.len() {
        match v.clamp_with_hint("fz_schema_p", cases[i].0) {
            Ok(oc) => {
                if !(oc.clamped && oc.value == cases[i].1) {
                    return ProbeVerdict::Suspect(E_FZ_CRASH); // 失钳——schema 面漂移
                }
            }
            Err(_) => return ProbeVerdict::Suspect(E_FZ_CRASH),
        }
        i += 1;
    }
    ProbeVerdict::Clean
}

/// **目标二：组合配置越界**（直打 F5208）。
///
/// 语料形 `combo:class:blend:weight:cost`——解析后过两闸：无票准入必拒
/// （E_CB_OVERREACH——绕账 O(1) 拦截）；有票超支必拒且不记账（账实守恒）。
/// 任一越界卡被放行即案（预算失守=崩）。
pub fn probe_combo(text: &str) -> ProbeVerdict {
    let rest = match text.strip_prefix("combo:") {
        Some(r) => r,
        None => return ProbeVerdict::Suspect(E_FZ_CRASH), // 非 combo 语料路由错——生成器违约
    };
    // 四段式：class:blend:weight:cost（前缀已剥）。
    let segs: Vec<&str> = rest.split(':').collect();
    if segs.len() != 4 {
        return ProbeVerdict::Suspect(E_FZ_CRASH); // 四段式违约
    }
    let weight: i64 = match segs[2].parse() {
        Ok(w) => w,
        Err(_) => return ProbeVerdict::Suspect(E_FZ_CRASH), // 语料坏——生成器违约
    };
    let cost: u32 = match segs[3].parse() {
        Ok(c) => c,
        Err(_) => return ProbeVerdict::Suspect(E_FZ_CRASH),
    };
    // 语料面防呆只拦「解析后语义失真」形：负权重/权重溢 u32（as 转换
    // 会环绕失真）。cost 不设上界防呆——超大 cost 恰是组合越界 fuzz
    // 主体（预算闸应拒），防呆拦它等于拆自己的探测面。
    if weight < 0 || weight > u32::MAX as i64 {
        return ProbeVerdict::Suspect(E_FZ_CRASH); // 负/溢权重——语料违约
    }
    let card = crate::svstar2::vez08_combo::OverlayCard {
        name: String::from("fz_card"),
        class: String::from(segs[0]),
        blend: match segs[1] {
            "Additive" => crate::svstar2::vez08_combo::BlendMode::Additive,
            "Replace" => crate::svstar2::vez08_combo::BlendMode::Replace,
            _ => crate::svstar2::vez08_combo::BlendMode::Exclusive,
        },
        weight: weight as u32,
        cost_us: cost,
    };
    // 闸一：无票准入——必拒。
    let mut budget = crate::svstar2::vez08_combo::ComboBudget::new(100);
    if budget.admit(card.clone(), false) != Err(crate::svstar2::vez08_combo::E_CB_OVERREACH) {
        return ProbeVerdict::Suspect(E_FZ_CRASH); // 无票放行——绕账失守
    }
    // 闸二：有票超支——必拒且不记账（cap=100，单卡 cost>100 必超）。
    let mut budget2 = crate::svstar2::vez08_combo::ComboBudget::new(100);
    if budget2.admit(card, true) != Err(crate::svstar2::vez08_combo::E_CB_OVER_BUDGET) {
        if cost <= 100 {
            return ProbeVerdict::Clean; // 未超支且票真——合法放行
        }
        return ProbeVerdict::Suspect(E_FZ_CRASH); // 超支放行——预算失守
    }
    if budget2.spent() != 0 {
        return ProbeVerdict::Suspect(E_FZ_CRASH); // 超支还记账——账实失恒
    }
    ProbeVerdict::Clean
}

/// **目标三：资源引用伪造**（同源口径——F5214 REAL_API_SURFACE 消费）。
///
/// 拦截闸 [`check_ref`]：引用名不在真实库面 → 拦（伪造→校验拦截——
/// 锚点矩阵第二行）；在面 → 放行。探测裁决：拦截成功与合法放行皆为
/// Clean（防线活）；语料缺 `ref:` 前缀 → 格式违约 Suspect（防御性
/// 立案面——面外语料不得静默）。
pub fn check_ref(name: &str) -> Result<(), &'static str> {
    if crate::svstar2::vez14_examples::REAL_API_SURFACE.contains(&name) {
        Ok(())
    } else {
        Err(E_FZ_FORGED)
    }
}

pub fn probe_forged(text: &str) -> ProbeVerdict {
    match text.strip_prefix("ref:") {
        Some(name) => match check_ref(name) {
            Ok(()) => ProbeVerdict::Clean,       // 合法放行
            Err(_) => ProbeVerdict::Clean,       // 伪造拦截——防线活（拦截即处置）
        },
        None => ProbeVerdict::Suspect(E_FZ_FORGED), // 面外语料——格式违约
    }
}

/// **目标四：闪烁参数边界**（直打 F5211 红线——光敏重点覆盖）。
///
/// 恰边界 (3,30) 必放行；越界 (3,31)/(4,30)/(4,31) 必拦+修正+幂等
/// （修正参数再检必过——红线不是抖动源）。语料 hz ∈ [0,8]、depth ∈
/// [-5,40] 全扫描：任何越界参数被 passed 即案（光敏红线失守）。
pub fn probe_flash(text: &str, gate: &mut crate::svstar2::vez11_safety::FlashGate) -> ProbeVerdict {
    let rest = match text.strip_prefix("flash:") {
        Some(r) => r,
        None => return ProbeVerdict::Suspect(E_FZ_BOUNDARY),
    };
    let mut hz: Option<u32> = None;
    let mut depth: Option<i64> = None;
    for pair in rest.split(';') {
        let mut kv = pair.split('=');
        let k = kv.next().unwrap_or("");
        let val: i64 = match kv.next().unwrap_or("").parse() {
            Ok(n) => n,
            Err(_) => return ProbeVerdict::Suspect(E_FZ_BOUNDARY),
        };
        if k == "hz" {
            if val < 0 {
                return ProbeVerdict::Suspect(E_FZ_BOUNDARY); // 负 hz 语料违约
            }
            hz = Some(val as u32);
        } else if k == "depth" {
            depth = Some(val);
        }
    }
    let (hz, depth) = match (hz, depth) {
        (Some(h), Some(d)) => (h, d),
        _ => return ProbeVerdict::Suspect(E_FZ_BOUNDARY),
    };
    let vd = crate::svstar2::vez11_safety::FlashParams { hz, depth };
    let verdict = gate.check(vd);
    // 语义复核：越界必拦、恰界必过（与 F5211 常数对拍——红线独立复核）。
    let should_block = hz > crate::svstar2::vez11_safety::FLASH_HZ_MAX
        || depth > crate::svstar2::vez11_safety::FLASH_DEPTH_MAX;
    if should_block {
        if verdict.passed {
            return ProbeVerdict::Suspect(E_FZ_BOUNDARY); // 越界放行——红线失守
        }
        // 修正幂等：修正参数再检必过。
        let recheck = gate.check(verdict.effective);
        if !recheck.passed {
            return ProbeVerdict::Suspect(E_FZ_BOUNDARY); // 修正后仍拦——抖动源
        }
        ProbeVerdict::Clean
    } else if verdict.passed {
        ProbeVerdict::Clean
    } else {
        ProbeVerdict::Suspect(E_FZ_BOUNDARY) // 恰界被拦——红线误伤（过严也是失守）
    }
}

// ---------------------------------------------------------------------------
// 六、最小化器：段删减保崩 ddmin 简化（最小化 O(输入)——锚点口径）
// ---------------------------------------------------------------------------

/// 最小化器：对失守语料做段删减——每删一段重放谓词（探测器），仍失守
/// 则保留删除（保崩语义）；删到不可再缩或失守丢失为止。
///
/// 返回 `Ok(min)` = 最小保崩形（含原样——单段或无段可删即已最小）；
/// `Err(E_FZ_MINFAIL)` 两失败面：① 案输入本身不复现失守（非崩输入进
/// 最小化=违约）；② 全删空（谓词对空文本仍失守——空即最小但语料面
/// 不接受空形，归约违约）。
pub fn minimize(crash_input: &str, is_crash: &dyn Fn(&str) -> bool) -> Result<String, &'static str> {
    // 前置：案必须真实失守（非崩输入进入最小化=违约）。
    if !is_crash(crash_input) {
        return Err(E_FZ_MINFAIL);
    }
    let mut current: Vec<String> = crash_input
        .split(';')
        .map(|s| String::from(s))
        .collect();
    // 单段不可分——直接回原样（已是最小）。
    if current.len() <= 1 {
        return Ok(String::from(crash_input));
    }
    // 逐段删减（delta debugging 单遍——每步重放保崩）。
    let mut i = 0usize;
    while i < current.len() {
        let mut candidate = current.clone();
        let _ = candidate.remove(i);
        let cand_text = candidate.join(";");
        if !cand_text.is_empty() && is_crash(&cand_text) {
            current = candidate; // 保崩——保留删除（不前进，同位还有段可删）
        } else {
            i += 1; // 丢崩或空形——该段不可删（回滚）
        }
    }
    if current.is_empty() {
        return Err(E_FZ_MINFAIL); // 全删空——语料面不接受空形
    }
    Ok(current.join(";"))
}

// ---------------------------------------------------------------------------
// 七、立案钩子：CaseFile + FuzzLedger（脱敏报告 + 光敏优先）
// ---------------------------------------------------------------------------

/// 单宗立案：编号/目标/指纹/裁决码/优先级（光敏案优先级 1——最高）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseFile {
    /// 立案编号（FZ-0001 起递增）。
    pub no: u32,
    /// 所属目标（四类之一）。
    pub target: &'static str,
    /// 输入指纹（FNV-64——脱敏面：报告永不含原文）。
    pub fp: u64,
    /// 裁决码（E_FZ_CRASH / E_FZ_BOUNDARY / E_FZ_FORGED / E_FZ_MINFAIL）。
    pub code: &'static str,
    /// 优先级（光敏=1，其余=2）。
    pub priority: u32,
}

/// 立案簿：钩子记账 + 光敏优先 + 脱敏报告（发现即立案——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct FuzzLedger {
    cases: Vec<CaseFile>,
    /// 下一案号。
    next_no: u32,
    /// 光敏案计数（重点覆盖指标）。
    pub photosensitive_cases: u32,
}

impl FuzzLedger {
    /// 空簿。
    pub fn new() -> Self {
        FuzzLedger { cases: Vec::new(), next_no: 1, photosensitive_cases: 0 }
    }

    /// **立案钩子**（发现即立案）：光敏案（E_FZ_BOUNDARY）优先级 1；
    /// 其余 2。编号单调递增。立案 O(1) 摊还——锚点限额 O(1) 面。
    pub fn case_open(&mut self, target: &'static str, input: &str, code: &'static str) -> CaseFile {
        let c = CaseFile {
            no: self.next_no,
            target,
            fp: fnv64(input.as_bytes()),
            code,
            priority: if code == E_FZ_BOUNDARY { 1 } else { 2 },
        };
        if code == E_FZ_BOUNDARY {
            self.photosensitive_cases += 1;
        }
        self.next_no += 1;
        self.cases.push(c.clone());
        c
    }

    /// 案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// **脱敏报告**：只出编号/目标/指纹/裁决码——永不含原文
    /// （报告脱敏——锚点原文；输入是攻击面，报告不得成为注入面）。
    pub fn report(&self) -> Vec<(u32, &'static str, u64, &'static str)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = &self.cases[i];
            out.push((c.no, c.target, c.fp, c.code));
            i += 1;
        }
        out
    }

    /// **光敏优先取案**：优先级 1 的案先出（光敏边界重点覆盖——处置序）。
    pub fn take_priority_first(&self) -> Option<CaseFile> {
        let mut best: Option<&CaseFile> = None;
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = &self.cases[i];
            match best {
                None => best = Some(c),
                Some(b) => {
                    if c.priority < b.priority || (c.priority == b.priority && c.no < b.no) {
                        best = Some(c);
                    }
                }
            }
            i += 1;
        }
        best.cloned()
    }
}

// ---------------------------------------------------------------------------
// 八、限额闸：单轮语料限额 + 立案总限额（O(1)——锚点口径）
// ---------------------------------------------------------------------------

/// 单轮语料限额（语料灌入超此数即拒——预算防护）。
pub const MAX_PER_ROUND: usize = 64;
/// 立案总数限额（立案簿超此数即拒续——账面防护）。
pub const MAX_CASES: usize = 128;

/// 限额闸（先查后放 O(1)——锚点「限额 O(1)」）。
#[derive(Clone, Debug, Default)]
pub struct QuotaGate {
    /// 本轮已灌语料数。
    pub round_used: usize,
    /// 立案总数记账。
    pub cases_total: usize,
    /// 拒续计数。
    pub quota_blocks: u32,
}

impl QuotaGate {
    /// 空闸。
    pub fn new() -> Self {
        QuotaGate::default()
    }

    /// **语料灌入**：超单轮限额即拒（O(1) 检查——先查后放）。
    pub fn admit_corpus(&mut self) -> Result<(), &'static str> {
        if self.round_used >= MAX_PER_ROUND {
            self.quota_blocks += 1;
            return Err(E_FZ_QUOTA);
        }
        self.round_used += 1;
        Ok(())
    }

    /// **立案入账**：超总限额即拒（O(1)——锚点口径）。
    pub fn admit_case(&mut self) -> Result<(), &'static str> {
        if self.cases_total >= MAX_CASES {
            self.quota_blocks += 1;
            return Err(E_FZ_QUOTA);
        }
        self.cases_total += 1;
        Ok(())
    }

    /// **轮切换**：单轮记账清零（总数记账保留）。
    pub fn round_reset(&mut self) {
        self.round_used = 0;
    }
}

// ---------------------------------------------------------------------------
// 九、FuzzRunner 主编排：每轮 O(语料数)——四目标路由探测+立案
// ---------------------------------------------------------------------------

/// fuzz 跑批器：语料池 + 探测统计 + 立案簿 + 限额闸四件编排
/// （每轮 O(语料数)——锚点口径）。
#[derive(Clone, Debug, Default)]
pub struct FuzzRunner {
    /// 探测统计账。
    pub stats: ProbeStats,
    /// 立案簿。
    pub ledger: FuzzLedger,
    /// 限额闸。
    pub quota: QuotaGate,
}

impl FuzzRunner {
    /// 空跑批器。
    pub fn new() -> Self {
        FuzzRunner::default()
    }

    /// **跑一轮**：语料按前缀路由四目标探测器（每轮 O(语料数)——锚点）。
    ///
    /// 路由规则：含 `|` 或 `PRM` 前缀 → schema；`combo:` → 组合；`ref:`
    /// → 伪造；`flash:` → 闪烁；其余语料违约（生成器/变形器产出面外
    /// 文本——防御性立案，不静默丢弃）。路由后每条按其族内判定。
    pub fn run_round(
        &mut self,
        pool: &mut CorpusPool,
        gate: &mut crate::svstar2::vez11_safety::FlashGate,
    ) -> usize {
        let batch = pool.drain();
        let mut opened = 0usize;
        let mut i = 0usize;
        while i < batch.len() {
            let item = &batch[i];
            if self.quota.admit_corpus().is_err() {
                break; // 限额拒续——本轮截断（语料弃入下轮）
            }
            let (verdict, route) = if item.text.contains('|') || item.text.starts_with("PRM") {
                self.stats.schema_n += 1;
                (probe_schema(&item.text), "schema_malformed")
            } else if item.text.starts_with("combo:") {
                self.stats.combo_n += 1;
                (probe_combo(&item.text), "combo_overreach")
            } else if item.text.starts_with("ref:") {
                self.stats.forged_n += 1;
                (probe_forged(&item.text), "resource_forged")
            } else if item.text.starts_with("flash:") {
                self.stats.flash_n += 1;
                (probe_flash(&item.text, gate), "flash_boundary")
            } else {
                (ProbeVerdict::Suspect(E_FZ_CRASH), "schema_malformed") // 面外语料——路由违约立案
            };
            if let ProbeVerdict::Suspect(code) = verdict {
                self.stats.suspect_n += 1;
                if self.quota.admit_case().is_ok() {
                    let _ = self.ledger.case_open(route, &item.text, code);
                    opened += 1;
                }
            } else {
                self.stats.clean_n += 1;
            }
            i += 1;
        }
        self.quota.round_reset();
        opened
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5215 域自检（判据 10 条六组：目标闭集/确定性/schema/组合伪造/光敏/
/// 最小化/限额/立案簿/判据元）。
pub fn run_vez15_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5215");

    // --- fuzz-01：目标闭集+语料池（种子+变形+去重+取干）---
    let mut pool = CorpusPool::new();
    let seeded = pool.seed_real();
    let seed_text = {
        let body = "ex_intensity=40;ex_speed=20";
        format!("PRM1|{}|{}", body, sum_tail(body))
    };
    let mutated = mutate_all(&seed_text, &mut pool);
    let dup = pool.add(&seed_text, "mut"); // 重复灌入——应拒
    let drained = pool.drain();
    let fz1_ok = FUZZ_TARGETS.len() == 4
        && seeded == 3
        && mutated >= 8 // 十算子去重后至少 8 条入库（个别变形可能与种子同文——去重合理）
        && !dup
        && pool.dup_blocked >= 1
        && drained.len() >= 11
        && pool.is_empty();
    if fz1_ok {
        s.ok("EZ15-fuzz-01-目标闭集与语料池");
    } else {
        s.fail("EZ15-fuzz-01-目标闭集与语料池", "语料池失效");
    }

    // --- fuzz-02：语法生成器确定性（同种子同序列）+ 覆盖面 ---
    let mut g1 = GrammarGen::new(42);
    let mut g2 = GrammarGen::new(42);
    let mut gen_a: Vec<String> = Vec::new();
    let mut gen_b: Vec<String> = Vec::new();
    let mut k = 0usize;
    while k < 12 {
        gen_a.push(g1.next_preset());
        gen_b.push(g2.next_preset());
        k += 1;
    }
    let mut same_seq = true;
    let mut k2 = 0usize;
    while k2 < gen_a.len() {
        if gen_a[k2] != gen_b[k2] {
            same_seq = false;
        }
        k2 += 1;
    }
    let mut heads_seen = 0usize;
    for h in HEAD_FORMS.iter() {
        let mut gi = GrammarGen::new(7);
        let mut hit = false;
        let mut n = 0usize;
        while n < 40 && !hit {
            let t = gi.next_preset();
            let head = t.split('|').next().unwrap_or("§");
            if head == *h {
                hit = true;
            }
            n += 1;
        }
        if hit {
            heads_seen += 1;
        }
    }
    let fz2_ok = same_seq && g1.emitted == 12 && heads_seen == HEAD_FORMS.len();
    if fz2_ok {
        s.ok("EZ15-fuzz-02-语法生成器确定性");
    } else {
        s.fail("EZ15-fuzz-02-语法生成器确定性", "生成器失效");
    }

    // --- fuzz-03：schema 差分防线（直打 F5207 三闸行为对拍+边界钳制）---
    let mut sch_clean = 0u32;
    let mut sch_suspect = 0u32;
    let mut sch_n = 0u32;
    let mut gi3 = GrammarGen::new(9);
    let mut k3 = 0usize;
    while k3 < 24 {
        let t = gi3.next_preset();
        sch_n += 1;
        if probe_schema(&t) == ProbeVerdict::Clean {
            sch_clean += 1;
        } else {
            sch_suspect += 1;
        }
        k3 += 1;
    }
    // 变形语料同路（preset 域八算子——neg_val/flash_over 产非 preset 文本另测）。
    const PRESET_MUTATORS: [&str; 8] = [
        "flip_head", "drop_tail", "bad_sum", "empty_name", "oversize_val", "neg_val", "dup_key", "truncate_half",
    ];
    let mut k3b = 0usize;
    while k3b < PRESET_MUTATORS.len() {
        let t = mutate_once(&seed_text, PRESET_MUTATORS[k3b]);
        sch_n += 1;
        if probe_schema(&t) == ProbeVerdict::Clean {
            sch_clean += 1;
        } else {
            sch_suspect += 1;
        }
        k3b += 1;
    }
    // 边界差分：三界值钳制全对（250→100 / -1→0 / 101→100）。
    let mut v3 = crate::svstar2::vez07_params::Validator::new();
    let boundary_clean = probe_schema_boundary(&mut v3) == ProbeVerdict::Clean;
    let fz3_ok = sch_n == 32 && sch_suspect == 0 && sch_clean == 32 && boundary_clean
        && v3.clamped_count >= 3;
    if fz3_ok {
        s.ok("EZ15-fuzz-03-schema差分防线");
    } else {
        s.fail("EZ15-fuzz-03-schema差分防线", "schema 面漂移或失钳");
    }

    // --- fuzz-04：组合越界+伪造拦截（直打 F5208+同源口径）---
    let combo_good = probe_combo("combo:particles:Additive:5:10");
    let combo_over = probe_combo("combo:particles:Additive:5:101"); // cost 超 cap——超支拒+不记账
    let combo_badfmt = probe_combo("combo:particles"); // 段数违约——防御性立案面
    let forged_blocked = probe_forged("ref:MagicFx::abracadabra"); // 伪造→拦截（Clean=防线活）
    let forged_real = probe_forged("ref:OverlayRules::new"); // 真面→放行
    let forged_badfmt = probe_forged("MagicFx::abracadabra"); // 缺前缀——格式违约立案面
    // 非 preset 域变形算子独立验证：flash 种子过 flash_over 算子 → hz=4
    // 越界语料被红线拦+修正幂等（光敏重点覆盖的变形面）。
    let mut gate4 = crate::svstar2::vez11_safety::FlashGate::new();
    let flash_seed = "flash:hz=2;depth=20";
    let flash_over_text = mutate_once(flash_seed, "flash_over");
    let flash_over_clean = flash_over_text == "flash:hz=4;depth=20"
        && probe_flash(&flash_over_text, &mut gate4) == ProbeVerdict::Clean;
    // 拦截闸直打：伪造名必拒（校验拦截——矩阵第二行）、真面名必放行。
    let ref_gate = check_ref("MagicFx::abracadabra") == Err(E_FZ_FORGED)
        && check_ref("OverlayRules::new").is_ok()
        && check_ref("StackResolver::push").is_ok();
    let fz4_ok = combo_good == ProbeVerdict::Clean
        && combo_over == ProbeVerdict::Clean
        && combo_badfmt == ProbeVerdict::Suspect(E_FZ_CRASH)
        && forged_blocked == ProbeVerdict::Clean
        && forged_real == ProbeVerdict::Clean
        && forged_badfmt == ProbeVerdict::Suspect(E_FZ_FORGED)
        && ref_gate
        && flash_over_clean;
    if fz4_ok {
        s.ok("EZ15-fuzz-04-组合越界与伪造拦截");
    } else {
        s.fail("EZ15-fuzz-04-组合越界与伪造拦截", "组合/伪造防线失守");
    }

    // --- fuzz-05：光敏边界（直打 F5211 红线——重点覆盖）---
    let mut gate = crate::svstar2::vez11_safety::FlashGate::new();
    // 恰边界双向：(3,30) 必过；(0,0) 必过。
    let edge_pass1 = probe_flash("flash:hz=3;depth=30", &mut gate) == ProbeVerdict::Clean;
    let edge_pass2 = probe_flash("flash:hz=0;depth=0", &mut gate) == ProbeVerdict::Clean;
    // 越界双向：恰超一单位即拦（3,31)/(4,30)——修正幂等。
    let over1 = probe_flash("flash:hz=3;depth=31", &mut gate) == ProbeVerdict::Clean;
    let over2 = probe_flash("flash:hz=4;depth=30", &mut gate) == ProbeVerdict::Clean;
    // 负深度语料（变形算子产出）——hz 正常 depth=-5：不越上界应放行。
    let neg_ok = probe_flash("flash:hz=2;depth=-5", &mut gate) == ProbeVerdict::Clean;
    // 全扫描：语法生成 24 条闪烁语料全 Clean（防线无失守无误伤）。
    let mut gi5 = GrammarGen::new(11);
    let mut flash_clean = 0u32;
    let mut k5 = 0usize;
    while k5 < 24 {
        let t = gi5.next_flash();
        if probe_flash(&t, &mut gate) == ProbeVerdict::Clean {
            flash_clean += 1;
        }
        k5 += 1;
    }
    let fz5_ok = edge_pass1 && edge_pass2 && over1 && over2 && neg_ok && flash_clean == 24;
    if fz5_ok {
        s.ok("EZ15-fuzz-05-光敏边界重点覆盖");
    } else {
        s.fail("EZ15-fuzz-05-光敏边界重点覆盖", "光敏边界失守或误伤");
    }

    // --- fuzz-06：最小化器（保崩删减+MINFAIL 失败分支）---
    // 成功形：谓词=文本含 "crash"——多段崩例删到只剩含 crash 的段。
    let crash_input = "keep1;crash_here;keep2;keep3";
    let pred1 = |t: &str| t.contains("crash");
    let min_ok = minimize(crash_input, &pred1);
    let min_ok_v = match &min_ok {
        Ok(m) => m.as_str() == "crash_here", // 最小保崩形=只留致崩段
        Err(_) => false,
    };
    // 失败形一：谓词恒假（首查即不崩）——MINFAIL 立案。
    let pred2 = |_t: &str| false;
    let min_fail = minimize("a;b;c", &pred2) == Err(E_FZ_MINFAIL);
    // 地板形：谓词对一切非空真——删到单段即达地板（空形不可删——
    // 语料面最小单位），返回单段最小形。
    let pred3 = |t: &str| !t.is_empty();
    let min_floor = minimize("a;b;c", &pred3) == Ok(String::from("c"));
    // 不可再缩形：谓词=恰原文全段才崩——单删即丢 → 原样返回（已最小）。
    let pred4 = |t: &str| t == "a;b;c";
    let min_irreducible = minimize("a;b;c", &pred4) == Ok(String::from("a;b;c"));
    // 单段形：不可分——原样返回。
    let pred5 = |t: &str| t.contains("x");
    let min_single = minimize("solo_x", &pred5) == Ok(String::from("solo_x"));
    let fz6_ok = min_ok_v && min_fail && min_floor && min_irreducible && min_single;
    if fz6_ok {
        s.ok("EZ15-fuzz-06-最小化保崩与MINFAIL");
    } else {
        s.fail("EZ15-fuzz-06-最小化保崩与MINFAIL", "最小化器失效");
    }

    // --- fuzz-07：限额闸（O(1) 先查后放）---
    let mut q = QuotaGate::new();
    let mut filled = 0usize;
    let mut quota_hit = false;
    while filled < MAX_PER_ROUND + 4 {
        if q.admit_corpus().is_err() {
            quota_hit = true;
            break;
        }
        filled += 1;
    }
    let corpus_quota_exact = filled == MAX_PER_ROUND && quota_hit && q.quota_blocks == 1;
    let mut q2 = QuotaGate::new();
    let mut case_n = 0usize;
    let mut case_hit = false;
    while case_n < MAX_CASES + 2 {
        if q2.admit_case().is_err() {
            case_hit = true;
            break;
        }
        case_n += 1;
    }
    let case_quota_exact = case_n == MAX_CASES && case_hit;
    q.round_reset();
    let reset_ok = q.round_used == 0 && q.cases_total == 0; // 轮清零不动总数
    let fz7_ok = corpus_quota_exact && case_quota_exact && reset_ok;
    if fz7_ok {
        s.ok("EZ15-fuzz-07-限额闸先查后放");
    } else {
        s.fail("EZ15-fuzz-07-限额闸先查后放", "限额闸失效");
    }

    // --- fuzz-08：立案簿（发现即立案+编号单调+光敏优先+脱敏）---
    let mut led = FuzzLedger::new();
    let c1 = led.case_open("schema_malformed", "PRM2|a=1|deadbeef", E_FZ_CRASH);
    let c2 = led.case_open("flash_boundary", "flash:hz=9;depth=99", E_FZ_BOUNDARY);
    let c3 = led.case_open("resource_forged", "ref:FakeFx::x", E_FZ_FORGED);
    let first = led.take_priority_first();
    let rep = led.report();
    let fp_manual = fnv64(b"flash:hz=9;depth=99");
    let fz8_ok = c1.no == 1 && c2.no == 2 && c3.no == 3
        && c2.priority == 1 && c1.priority == 2 && c3.priority == 2
        && led.photosensitive_cases == 1
        && match &first {
            Some(f) => f.no == c2.no && f.code == E_FZ_BOUNDARY,
            None => false,
        }
        && rep.len() == 3
        && rep.iter().any(|r| r.0 == 2 && r.3 == E_FZ_BOUNDARY && r.2 == fp_manual)
        && led.len() == 3;
    if fz8_ok {
        s.ok("EZ15-fuzz-08-立案簿光敏优先脱敏");
    } else {
        s.fail("EZ15-fuzz-08-立案簿光敏优先脱敏", "立案簿失效");
    }

    // --- fuzz-09：主编排一轮（真实防线全活→零立案——防线在才零案）---
    let mut r = FuzzRunner::new();
    let mut pool9 = CorpusPool::new();
    let _ = pool9.seed_real();
    let mut m9 = 0usize;
    while m9 < MUTATORS.len() {
        let t = mutate_once(&seed_text, MUTATORS[m9]);
        let _ = pool9.add(&t, "mut");
        m9 += 1;
    }
    let mut gi9 = GrammarGen::new(13);
    let mut g9 = 0usize;
    while g9 < 8 {
        let t = gi9.next_preset();
        let _ = pool9.add(&t, "gen");
        g9 += 1;
    }
    let mut g9b = 0usize;
    while g9b < 8 {
        let t = gi9.next_flash();
        let _ = pool9.add(&t, "gen");
        g9b += 1;
    }
    let mut g9c = 0usize;
    while g9c < 6 {
        let t = gi9.next_ref();
        let _ = pool9.add(&t, "gen");
        g9c += 1;
    }
    let mut gate9 = crate::svstar2::vez11_safety::FlashGate::new();
    let before = pool9.len();
    let opened9 = r.run_round(&mut pool9, &mut gate9);
    let st = r.stats.clone();
    // 防线全活：fake 引用语料会被伪造闸立案吗？——不：伪造拦截成功=Clean
    // （防线拦住记 Clean），只有防线失守才立案。全真实现活 → opened==0。
    let fz9_ok = before >= 24 && opened9 == 0 && st.suspect_n == 0
        && st.clean_n == before as u32
        && st.schema_n >= 1 && st.combo_n >= 1 && st.forged_n >= 1 && st.flash_n >= 1
        && r.ledger.is_empty()
        && pool9.is_empty();
    if fz9_ok {
        s.ok("EZ15-fuzz-09-主编排四目标零失守");
    } else {
        s.fail("EZ15-fuzz-09-主编排四目标零失守", "编排或防线失守");
    }

    // --- 判据元（判据六组）---
    // 元-01：五码互异 + 三要素齐备。
    let codes = [E_FZ_CRASH, E_FZ_FORGED, E_FZ_BOUNDARY, E_FZ_MINFAIL, E_FZ_QUOTA];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i10 = 0usize;
    while i10 < codes.len() && codes_ok {
        if reason_of(codes[i10]) == "未知错误码" || direction_of(codes[i10]).is_empty() {
            codes_ok = false;
        }
        let mut j10 = i10 + 1;
        while j10 < codes.len() {
            if codes[i10] == codes[j10] {
                codes_ok = false;
            }
            j10 += 1;
        }
        i10 += 1;
    }
    if codes_ok {
        s.ok("EZ15-元-01-码互异三要素齐备");
    } else {
        s.fail("EZ15-元-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez15_fuzz.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ15-元-02-零panic零unsafe面");
    } else {
        s.fail("EZ15-元-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：F4943 错误路径降级矩阵接入对拍——三行三裁决闭集齐备
    // （崩溃→最小化修复 / 伪造→校验拦截 / 边界→光敏检测验证）。
    let matrix_rows = [
        (E_FZ_CRASH, "最小化修复", "minimal"),
        (E_FZ_FORGED, "校验拦截", "intercept"),
        (E_FZ_BOUNDARY, "光敏检测验证", "photosensitive"),
    ];
    let mut matrix_ok = matrix_rows.len() == 3;
    for row in matrix_rows.iter() {
        if reason_of(row.0) == "未知错误码" || direction_of(row.0).is_empty() {
            matrix_ok = false;
        }
        // 处置动词与矩阵行对拍：CRASH 行处置含"最小化"、FORGED 行含"拦截"、
        // BOUNDARY 行含"光敏"。
        let d = direction_of(row.0);
        let keyword = match row.2 {
            "minimal" => "最小化",
            "intercept" => "拦截",
            _ => "光敏",
        };
        if !d.contains(keyword) {
            matrix_ok = false;
        }
        let _ = row.1;
    }
    if matrix_ok {
        s.ok("EZ15-元-03-降级矩阵三行接入");
    } else {
        s.fail("EZ15-元-03-降级矩阵三行接入", "矩阵漂移");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤；
/// 字符串字面量整体剥空——判据模式串不自 hit，与 F5214 同款同口径）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
