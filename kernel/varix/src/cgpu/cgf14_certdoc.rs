//! cgf14_certdoc — CGPU-F0812 认证文档（白皮书/规范/规则表——文档即契约，
//! 过期文档比没文档更危险）。
//!
//! 判据原文（施工书锚点 CGPU-F0812）：文档：认证白皮书（六步流程/四级判
//! 据/有效期）；能力探测规范；重认证触发规则表。判据：白皮书、规范、规则
//! 表、判据。
//!
//! 职责定位：认证白皮书结构化仓（六步流程闭集/四级判据量化表/有效期规则
//! ——每节带指纹，文档版本化）+ 能力探测规范（五步探测的规范文档面——与
//! vck04_hetprobe 五步对齐，规范与实现互为锚）+ 重认证触发规则表（表驱动
//! ——触发类型→受影响场景→增量计划，与 cgf08_certvalid 规则对齐）+ 文档
//! 一致性核验（白皮书引用的规则表号/规范节号必须真实存在——悬空引用即文
//! 档烂账）。红线：文档数字与实现常量对拍（四级阈值/90 天有效期——文档说
//! 的和代码做的不许两套账）；规则表条目闭集（表外触发类型拒绝入文档）。
//! 诊断码段 0x6401~0x6405。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const CERT_DOC_VERSION: &str = "F14-certdoc-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x64 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_DANGLING_REF: u32 = 0x6401; // 悬空引用（文档引了不存在的节/表）
pub const XC_DOC_CODE_DRIFT: u32 = 0x6402; // 文档-代码漂移（数字两套账）
pub const XC_RULE_OUT_OF_SET: u32 = 0x6403; // 规则表外触发类型
pub const XC_DOC_SECTION_DUP: u32 = 0x6404; // 节号重复
pub const XC_WP_STALE: u32 = 0x6405; // 白皮书过期（指纹未随实现更新）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 白皮书六步流程（闭集——与认证流水一致）。
pub const WP_FLOW_STEPS: [&str; 6] = [
    "探测", "考卷", "判级", "有效期", "复核", "公开",
];
/// 四级判据（闭集——与 cgf07_certgrade 对齐）。
pub const WP_GRADES: [&str; 4] = ["gold", "silver", "bronze", "uncertified"];
/// 有效期天数（与 cgf08_certvalid 对齐——文档-代码对拍锚）。
pub const WP_VALIDITY_DAYS: u32 = 90;
/// 探测五步（与 vck04_hetprobe 对齐）。
pub const PROBE_SPEC_STEPS: [&str; 5] = ["枚举", "查询", "隔离", "登记", "缓存公示"];
/// 重认证触发类型闭集（与 cgf08/cgf11 对齐）。
pub const REAUTH_TRIGGERS: [&str; 3] = ["driver_major", "system_update", "engine_change"];
/// 白皮书节容量。
pub const WP_SECTION_CAP: usize = 24;

/// 白皮书节。
#[derive(Clone, Debug, PartialEq)]
pub struct WpSection {
    /// 节号（1..）。
    pub no: usize,
    /// 节标题。
    pub title: String,
    /// 内容（结构化摘要——数字对拍面）。
    pub body: String,
    /// 节指纹。
    pub sig: u64,
}

impl WpSection {
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_mix(h, &(self.no as u64).to_le_bytes());
        h = fnv_mix(h, self.title.as_bytes());
        h = fnv_mix(h, self.body.as_bytes());
        h
    }
}

pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x1000_0000_01b3)
}

pub fn fnv_mix(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < data.len() {
        h = fnv_byte(h, data[i]);
        i += 1;
    }
    h
}

/// 白皮书仓。
pub struct WhitePaper {
    sections: Vec<WpSection>,
    /// 全文指纹。
    wp_sig: u64,
    pub version: u32,
    pub rejects: u64,
}

impl WhitePaper {
    pub fn new() -> Self {
        WhitePaper {
            sections: Vec::new(),
            wp_sig: 0,
            version: 1,
            rejects: 0,
        }
    }

    fn recompute(&mut self) {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < self.sections.len() {
            h = fnv_mix(h, &self.sections[i].sig.to_le_bytes());
            i += 1;
        }
        self.wp_sig = h;
    }

    /// 节登记（节号唯一、指纹计算）。
    pub fn add_section(&mut self, no: usize, title: &str, body: &str) -> Result<u64, u32> {
        if no == 0 || self.sections.len() >= WP_SECTION_CAP {
            self.rejects += 1;
            return Err(XC_DOC_SECTION_DUP);
        }
        let mut i = 0;
        while i < self.sections.len() {
            if self.sections[i].no == no {
                self.rejects += 1;
                return Err(XC_DOC_SECTION_DUP);
            }
            i += 1;
        }
        let s = WpSection {
            no,
            title: String::from(title),
            body: String::from(body),
            sig: 0,
        };
        let sig = s.fingerprint();
        self.sections.push(WpSection { sig, ..s });
        self.recompute();
        Ok(sig)
    }

    /// 六步流程完整性核验（判据"六步流程"——白皮书必须含全部六步节）。
    pub fn verify_flow_complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < WP_FLOW_STEPS.len() {
            let step = WP_FLOW_STEPS[i];
            let mut found = false;
            let mut j = 0;
            while j < self.sections.len() {
                if contains_str(&self.sections[j].title, step) {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(XC_DANGLING_REF);
            }
            i += 1;
        }
        Ok(())
    }

    /// 文档-代码对拍（判据"四级判据/有效期"——文档正文数字与实现常量一
    /// 致：四级闭集名与有效期天数必须出现在正文中，数字漂移即立案）。
    pub fn verify_against_code(&self) -> Result<(), u32> {
        // 四级闭集名全出现在某节正文。
        let mut g = 0;
        while g < WP_GRADES.len() {
            let mut found = false;
            let mut j = 0;
            while j < self.sections.len() {
                if contains_str(&self.sections[j].body, WP_GRADES[g]) {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                return Err(XC_DOC_CODE_DRIFT);
            }
            g += 1;
        }
        // 有效期天数对拍：正文必须含 "90天"（WP_VALIDITY_DAYS 的文档表示）。
        let mut has_validity = false;
        let mut k = 0;
        while k < self.sections.len() {
            if contains_str(&self.sections[k].body, "90天") {
                has_validity = true;
                break;
            }
            k += 1;
        }
        if !has_validity {
            return Err(XC_DOC_CODE_DRIFT);
        }
        Ok(())
    }

    /// 悬空引用核验（判据"判据"面）：正文引用 "规则表N" 必须有第 N 节。
    pub fn verify_no_dangling_refs(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sections.len() {
            let body = &self.sections[i].body;
            // 扫描 "规则表"+数字 的引用。
            let bytes = body.as_bytes();
            let mut p = 0;
            while p + 9 <= bytes.len() {
                if &bytes[p..p + 9] == b"\xe8\xa7\x84\xe5\x88\x99\xe8\xa1\xa8" {
                    // "规则表" UTF-8 9 字节——后面跟 ASCII 数字。
                    let mut q = p + 9;
                    let mut num: usize = 0;
                    let mut got = false;
                    while q < bytes.len() && bytes[q] >= b'0' && bytes[q] <= b'9' {
                        num = num * 10 + (bytes[q] - b'0') as usize;
                        q += 1;
                        got = true;
                    }
                    if got {
                        // 引用必须命中真实节号。
                        let mut hit = false;
                        let mut j = 0;
                        while j < self.sections.len() {
                            if self.sections[j].no == num {
                                hit = true;
                                break;
                            }
                            j += 1;
                        }
                        if !hit {
                            return Err(XC_DANGLING_REF);
                        }
                    }
                }
                p += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 白皮书指纹查询（文档版本锚——实现更新后指纹必须刷新，否则过期）。
    pub const fn signature(&self) -> u64 {
        self.wp_sig
    }

    /// 节数。
    pub const fn len(&self) -> usize {
        self.sections.len()
    }
}

/// ASCII 子串包含。
pub fn contains_str(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.len() == 0 || hb.len() < nb.len() {
        return nb.len() == 0;
    }
    let mut i = 0;
    while i + nb.len() <= hb.len() {
        let mut m = true;
        let mut j = 0;
        while j < nb.len() {
            if hb[i + j] != nb[j] {
                m = false;
                break;
            }
            j += 1;
        }
        if m {
            return true;
        }
        i += 1;
    }
    false
}

/// 重认证触发规则表（判据"重认证触发规则表"——表驱动）。
pub struct ReauthRuleTable {
    /// 规则条目：触发类型 → 受影响场景闭集。
    rules: Vec<(&'static str, Vec<&'static str>)>,
    pub rejects: u64,
}

impl ReauthRuleTable {
    pub fn new() -> Self {
        ReauthRuleTable {
            rules: Vec::new(),
            rejects: 0,
        }
    }

    /// 规则登记（触发类型闭集校验——表外类型拒）。
    pub fn add_rule(&mut self, trigger: &'static str, scenarios: Vec<&'static str>) -> Result<(), u32> {
        let mut known = false;
        let mut i = 0;
        while i < REAUTH_TRIGGERS.len() {
            if REAUTH_TRIGGERS[i] == trigger {
                known = true;
                break;
            }
            i += 1;
        }
        if !known || scenarios.len() == 0 {
            self.rejects += 1;
            return Err(XC_RULE_OUT_OF_SET);
        }
        // 重复触发类型拒绝（一类型一规则——规则表不是流水账）。
        let mut j = 0;
        while j < self.rules.len() {
            if self.rules[j].0 == trigger {
                self.rejects += 1;
                return Err(XC_RULE_OUT_OF_SET);
            }
            j += 1;
        }
        self.rules.push((trigger, scenarios));
        Ok(())
    }

    /// 触发查询（受影响场景闭集——增量重认证计划的输入）。
    pub fn scenarios_of(&self, trigger: &str) -> Option<&Vec<&'static str>> {
        let mut i = 0;
        while i < self.rules.len() {
            if self.rules[i].0 == trigger {
                return Some(&self.rules[i].1);
            }
            i += 1;
        }
        None
    }

    /// 全量核验：三类触发全部在册（规则表完整性——缺类即文档烂账）。
    pub fn verify_complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < REAUTH_TRIGGERS.len() {
            if self.scenarios_of(REAUTH_TRIGGERS[i]).is_none() {
                return Err(XC_RULE_OUT_OF_SET);
            }
            i += 1;
        }
        Ok(())
    }

    /// 规则数。
    pub const fn len(&self) -> usize {
        self.rules.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：白皮书 / 探测规范 / 规则表 / 文档-代码对拍）
// ---------------------------------------------------------------------------

impl WhitePaper {
    /// 组一：白皮书——六步节齐、节号重复拒、悬空引用查。
    pub fn selfcheck_wp(&mut self) -> Result<(), u32> {
        let mut wp = WhitePaper::new();
        wp.add_section(1, "探测流程", "五步探测见规则表2")?;
        wp.add_section(2, "考卷", "全链考卷闭集")?;
        wp.add_section(3, "判级", "四级判据 gold/silver/bronze/uncertified")?;
        wp.add_section(4, "有效期", "有效期90天，过期显性")?;
        wp.add_section(5, "复核", "复核工单同逻辑重判")?;
        wp.add_section(6, "公开", "认证矩阵全量公开")?;
        wp.verify_flow_complete()?;
        wp.verify_no_dangling_refs()?; // 规则表2 → 第 2 节存在
        // 重复节号拒。
        if wp.add_section(3, "重复", "x") != Err(XC_DOC_SECTION_DUP) {
            return Err(XC_DOC_SECTION_DUP);
        }
        // 悬空引用：引用规则表9（不存在）。
        wp.add_section(7, "坏引用", "见规则表9")?;
        if wp.verify_no_dangling_refs() != Err(XC_DANGLING_REF) {
            return Err(XC_DANGLING_REF);
        }
        Ok(())
    }

    /// 组二：探测规范——五步闭集对齐（与 vck04 五步一一对应）。
    pub fn selfcheck_probe_spec(&mut self) -> Result<(), u32> {
        // 五步常量必须与 vck04 五步语义一致（此处核验闭集长度与首尾步名）。
        if PROBE_SPEC_STEPS.len() != 5 {
            return Err(XC_DOC_CODE_DRIFT);
        }
        if PROBE_SPEC_STEPS[0] != "枚举" || PROBE_SPEC_STEPS[4] != "缓存公示" {
            return Err(XC_DOC_CODE_DRIFT);
        }
        // 白皮书含探测规范节。
        let mut wp = WhitePaper::new();
        wp.add_section(1, "能力探测规范", "枚举→查询→隔离→登记→缓存公示")?;
        if wp.len() != 1 {
            return Err(XC_DOC_SECTION_DUP);
        }
        Ok(())
    }

    /// 组三：规则表——三类齐、表外拒、重复拒、查询正确。
    pub fn selfcheck_rules(&mut self) -> Result<(), u32> {
        let mut t = ReauthRuleTable::new();
        t.add_rule("driver_major", vec!["all"])?;
        t.add_rule("system_update", vec!["video", "compute"])?;
        t.add_rule("engine_change", vec!["affected_scenarios"])?;
        t.verify_complete()?;
        if t.len() != 3 {
            return Err(XC_RULE_OUT_OF_SET);
        }
        // 表外类型拒。
        if t.add_rule("weather_change", vec!["x"]) != Err(XC_RULE_OUT_OF_SET) {
            return Err(XC_RULE_OUT_OF_SET);
        }
        // 重复类型拒。
        if t.add_rule("driver_major", vec!["y"]) != Err(XC_RULE_OUT_OF_SET) {
            return Err(XC_RULE_OUT_OF_SET);
        }
        // 空场景拒。
        if t.add_rule("engine_change", vec![]) != Err(XC_RULE_OUT_OF_SET) {
            return Err(XC_RULE_OUT_OF_SET);
        }
        // 查询。
        let s = t.scenarios_of("system_update");
        match s {
            Some(v) => {
                if v.len() != 2 || v[0] != "video" {
                    return Err(XC_RULE_OUT_OF_SET);
                }
            }
            None => return Err(XC_RULE_OUT_OF_SET),
        }
        Ok(())
    }

    /// 组四：文档-代码对拍——四级名与 90 天必须在正文、缺任一即漂移。
    pub fn selfcheck_code_drift(&mut self) -> Result<(), u32> {
        let mut wp = WhitePaper::new();
        wp.add_section(1, "判级", "四级判据：gold silver bronze uncertified")?;
        wp.add_section(2, "有效期", "有效期90天")?;
        wp.verify_against_code()?;
        // 缺 90 天 → 漂移。
        let mut wp2 = WhitePaper::new();
        wp2.add_section(1, "判级", "gold silver bronze uncertified")?;
        wp2.add_section(2, "有效期", "有效期 60 天（写错数字）")?;
        if wp2.verify_against_code() != Err(XC_DOC_CODE_DRIFT) {
            return Err(XC_DOC_CODE_DRIFT);
        }
        // 缺四级名 → 漂移。
        let mut wp3 = WhitePaper::new();
        wp3.add_section(1, "判级", "金 银 铜 未认证（没用代码闭集名）")?;
        wp3.add_section(2, "有效期", "90天")?;
        if wp3.verify_against_code() != Err(XC_DOC_CODE_DRIFT) {
            return Err(XC_DOC_CODE_DRIFT);
        }
        // 指纹确定性：同内容同指纹。
        let mut wp4 = WhitePaper::new();
        wp4.add_section(1, "判级", "gold silver bronze uncertified")?;
        wp4.add_section(2, "有效期", "有效期90天")?;
        if wp.signature() != wp4.signature() {
            return Err(XC_WP_STALE);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_wp()?;
        self.selfcheck_probe_spec()?;
        self.selfcheck_rules()?;
        self.selfcheck_code_drift()?;
        Ok(())
    }
}
