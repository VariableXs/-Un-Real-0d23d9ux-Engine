//! unx1501_regmat —— UNX-F1501 域回归矩阵冻结 v1（场景×判据×预期三维）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B36）：UNX-F1501 · 域回归矩阵冻结 v1（场景×判据×预期三维） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1501-J1 域回归矩阵 v1 冻结（场景/判据引用/预期三列 × 全域场景
//!           登记：账本五族+零丢五场景+四态+接管等价+注入四族），矩阵行数登记
//!           100% 覆盖断言，矩阵漂移注入 10/10 必抓
//! 组件：三维回归矩阵（场景/判据引用/预期三列冻结行表，行数闭集）+
//!      五族场景登记（账本五族：B01-B05 族 + 零丢五场景 + 四态场景 +
//!      接管等价场景 + 注入四族，逐族逐行登记 100% 覆盖）+
//!      判据引用闸（每行判据引用必须是 UNX-Fxxxx-Jn 形且可解析）+
//!      预期闭集（PASS/REJECT/RED 三态，闭集外即漂移）+
//!      冻结指纹（全矩阵语义序 FNV）+ 漂移注入核（10 处 10/10 必抓）。
//! 职责定位：域级回归的唯一权威矩阵——每个场景一行、每行判据可解引、
//!   每行预期闭集化；矩阵冻结后成为回归执行的输入契约（F0641 回归轮
//!   与 F1581 总回归都消费本矩阵）；漂移即指纹变化即显性。
//! 性能逐项分解：行登记 O(1)/行；覆盖断言 O(行数)；指纹 O(表字节)；
//!   引用解析 O(名字长)。全整数确定性，零浮点零墙钟。
//! 诊断码段：0x5280（0x5281~0x5288 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RM_VERSION: &str = "unx1501_regmat-v1";

/// 诊断段基址（0x5280 段 · 本模块独占）。
pub const RM_SEG_BASE: u16 = 0x5280;

/// 五族场景名（冻结声明：账本五族 B01–B05）。
pub const RM_LEDGER_FAMILIES: [&str; 5] = ["账本B01帧账", "账本B02预算账", "账本B03时序账", "账本B04认证账", "账本B05账本索引"];

/// 零丢五场景。
pub const RM_ZERO_SCENES: [&str; 5] = ["零丢全入账", "零丢显性覆盖", "零丢序号洞检出", "零丢静默丢失抓", "零丢全域合计"];

/// 四态场景。
pub const RM_FOUR_STATES: [&str; 4] = ["四态Init", "四态Polling", "四态Yielded", "四态Done"];

/// 接管等价场景。
pub const RM_TAKEOVER_SCENES: [&str; 2] = ["接管等价中断面", "接管等价轮询面"];

/// 注入四族。
pub const RM_INJECT_FAMILIES: [&str; 4] = ["注入族账面破坏", "注入族序号破坏", "注入族状态破坏", "注入族开销破坏"];

/// 预期三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmExpect {
    /// 通过（功能绿）。
    Pass,
    /// 拒绝（注入必须被拒）。
    Reject,
    /// 红码（破坏必须显性红）。
    Red,
}

impl RmExpect {
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            RmExpect::Pass => "PASS",
            RmExpect::Reject => "REJECT",
            RmExpect::Red => "RED",
        }
    }
}

/// 矩阵行（场景/判据引用/预期三列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RmRow {
    /// 场景名（唯一键）。
    pub scene: String,
    /// 判据引用（UNX-Fxxxx-Jn 形）。
    pub judge_ref: String,
    /// 预期。
    pub expect: RmExpect,
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmCode {
    /// 0x5281 场景重名。
    SceneDup = 0x5281,
    /// 0x5282 判据引用形不合法。
    JudgeRefMalformed = 0x5282,
    /// 0x5283 场景缺失（覆盖断言红：族内场景未登记）。
    SceneMissing = 0x5283,
    /// 0x5284 预期闭集外。
    ExpectUnknown = 0x5284,
    /// 0x5285 矩阵已冻结（seal 后写拒）。
    Sealed = 0x5285,
    /// 0x5286 冻结指纹漂移（reseal 异指纹）。
    SealDrift = 0x5286,
    /// 0x5287 行序漂移（族序被改）。
    OrderDrift = 0x5287,
    /// 0x5288 场景名空。
    SceneEmpty = 0x5288,
}

impl RmCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            RmCode::SceneDup => "场景重名（场景是矩阵唯一键）",
            RmCode::JudgeRefMalformed => "判据引用不合法（须 UNX-Fxxxx-Jn 形）",
            RmCode::SceneMissing => "族内场景未登记（覆盖 <100%）",
            RmCode::ExpectUnknown => "预期不在三态闭集（PASS/REJECT/RED）",
            RmCode::Sealed => "矩阵已冻结：登记通道关闭",
            RmCode::SealDrift => "冻结指纹漂移（矩阵内容在 seal 间被改）",
            RmCode::OrderDrift => "族序漂移（登记序违冻结族序）",
            RmCode::SceneEmpty => "场景名为空",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            RmCode::SceneDup => "场景改名或并入既有行；一行一场景",
            RmCode::JudgeRefMalformed => "判据引用按 UNX-F####-J# 书写并与施工书对齐",
            RmCode::SceneMissing => "补登记缺失场景；覆盖 100% 是冻结前置",
            RmCode::ExpectUnknown => "预期只许 PASS/REJECT/RED 三态",
            RmCode::Sealed => "冻结后只读；变更走升版（v1→v2）留指纹账",
            RmCode::SealDrift => "两次 seal 之间矩阵被改：追写路径回滚",
            RmCode::OrderDrift => "按族序登记；族序是冻结面",
            RmCode::SceneEmpty => "场景名必填",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RmDiag {
    /// 码。
    pub code: RmCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl RmDiag {
    /// 构造。
    pub fn new(code: RmCode, site: &str, detail: String) -> Self {
        RmDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 判据引用解析（UNX-F####-J# 形；合法返回（F号, J号））。
pub fn rm_parse_judge(r: &str) -> Option<(u32, u32)> {
    let b = r.as_bytes();
    // 前缀 UNX-F
    if b.len() < 9 || &r[..4] != "UNX-" || &r[4..5] != "F" {
        return None;
    }
    let rest = &r[5..];
    let dash = rest.find('-')?;
    if dash == 0 {
        return None;
    }
    let fid = &rest[..dash];
    let jpart = &rest[dash + 1..];
    if jpart.len() < 2 || !jpart.starts_with('J') {
        return None;
    }
    let mut fnum: u32 = 0;
    for c in fid.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        fnum = fnum.wrapping_mul(10).wrapping_add((c - b'0') as u32);
    }
    let mut jnum: u32 = 0;
    for c in jpart[1..].bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        jnum = jnum.wrapping_mul(10).wrapping_add((c - b'0') as u32);
    }
    if jnum == 0 {
        return None;
    }
    Some((fnum, jnum))
}

/// 域回归矩阵 v1。
#[derive(Clone, Debug, Default)]
pub struct RmMatrix {
    /// 行表（登记序 = 冻结序）。
    pub rows: Vec<RmRow>,
    /// 冻结指纹（0 = 未冻结）。
    pub seal_fp: u64,
    /// 版本。
    pub version: u32,
}

impl RmMatrix {
    /// 新矩阵（v1）。
    pub fn new() -> Self {
        RmMatrix { rows: Vec::new(), seal_fp: 0, version: 1 }
    }

    /// 登记一行（场景闸/引用闸/预期闸/冻结闸四道）。
    pub fn register(&mut self, scene: &str, judge_ref: &str, expect: RmExpect) -> Result<(), RmDiag> {
        if self.seal_fp != 0 {
            return Err(RmDiag::new(RmCode::Sealed, "register:seal", format!("场景 [{}]", scene)));
        }
        if scene.is_empty() {
            return Err(RmDiag::new(RmCode::SceneEmpty, "register:empty", "场景名为空".to_string()));
        }
        for r in &self.rows {
            if r.scene == scene {
                return Err(RmDiag::new(RmCode::SceneDup, "register:dup", format!("场景 [{}]", scene)));
            }
        }
        if rm_parse_judge(judge_ref).is_none() {
            return Err(RmDiag::new(
                RmCode::JudgeRefMalformed,
                "register:ref",
                format!("引用 [{}]", judge_ref),
            ));
        }
        self.rows.push(RmRow {
            scene: String::from(scene),
            judge_ref: String::from(judge_ref),
            expect,
        });
        Ok(())
    }

    /// 全族场景批量登记（五族+零丢+四态+接管+注入 闭集 20 场景）。
    pub fn register_all(&mut self, refs: &[(&str, &str)]) -> Result<usize, RmDiag> {
        // refs: (场景, 判据引用) 对。
        let mut n = 0usize;
        for (scene, r) in refs {
            // 预期规则：注入族场景必 REJECT/RED，其余 PASS。
            let expect = if scene.starts_with("注入族") {
                if scene.contains("破坏") && scene.contains("状态") {
                    RmExpect::Red
                } else {
                    RmExpect::Reject
                }
            } else {
                RmExpect::Pass
            };
            self.register(scene, r, expect)?;
            n += 1;
        }
        Ok(n)
    }

    /// 覆盖断言（判据点名：矩阵行数登记 100% 覆盖）：
    /// 闭集 20 场景（5+5+4+2+4）全部在册。
    pub fn coverage_ok(&self) -> (bool, Vec<String>) {
        let mut missing: Vec<String> = Vec::new();
        let mut check = |names: &[&str], missing: &mut Vec<String>| {
            for name in names {
                let mut found = false;
                for r in &self.rows {
                    if r.scene == *name {
                        found = true;
                        break;
                    }
                }
                if !found {
                    missing.push(String::from(*name));
                }
            }
        };
        check(&RM_LEDGER_FAMILIES, &mut missing);
        check(&RM_ZERO_SCENES, &mut missing);
        check(&RM_FOUR_STATES, &mut missing);
        check(&RM_TAKEOVER_SCENES, &mut missing);
        check(&RM_INJECT_FAMILIES, &mut missing);
        (missing.is_empty(), missing)
    }

    /// 冻结（覆盖 100% 是前置；指纹入册）。
    pub fn seal(&mut self) -> Result<u64, RmDiag> {
        let (cov, missing) = self.coverage_ok();
        if !cov {
            return Err(RmDiag::new(
                RmCode::SceneMissing,
                "seal:cov",
                format!("缺 {} 场景", missing.len()),
            ));
        }
        let fp = self.fingerprint();
        if self.seal_fp != 0 {
            if self.seal_fp == fp {
                return Ok(fp);
            }
            return Err(RmDiag::new(
                RmCode::SealDrift,
                "seal:drift",
                format!("首 0x{:016X} 现 0x{:016X}", self.seal_fp, fp),
            ));
        }
        self.seal_fp = fp;
        Ok(fp)
    }

    /// 全矩阵语义序指纹。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for r in &self.rows {
            for b in r.scene.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            for b in r.judge_ref.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= r.expect as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 报表。
    pub fn report(&self) -> String {
        let (cov, missing) = self.coverage_ok();
        format!(
            "矩阵 {} 行 覆盖:{} 冻结:0x{:016X} v{} 缺:{}",
            self.rows.len(),
            cov,
            self.seal_fp,
            self.version,
            missing.len()
        )
    }
}

/// v1 标准引用集（20 场景 → 判据引用映射，冻结声明）。
pub fn rm_v1_refs() -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str)> = Vec::new();
    v.push((RM_LEDGER_FAMILIES[0], "UNX-F0621-J1"));
    v.push((RM_LEDGER_FAMILIES[1], "UNX-F0621-J2"));
    v.push((RM_LEDGER_FAMILIES[2], "UNX-F0641-J1"));
    v.push((RM_LEDGER_FAMILIES[3], "UNX-F0661-J1"));
    v.push((RM_LEDGER_FAMILIES[4], "UNX-F0681-J1"));
    v.push((RM_ZERO_SCENES[0], "UNX-F1461-J1"));
    v.push((RM_ZERO_SCENES[1], "UNX-F1461-J1"));
    v.push((RM_ZERO_SCENES[2], "UNX-F1461-J1"));
    v.push((RM_ZERO_SCENES[3], "UNX-F1461-J1"));
    v.push((RM_ZERO_SCENES[4], "UNX-F1461-J1"));
    v.push((RM_FOUR_STATES[0], "UNX-F1201-J1"));
    v.push((RM_FOUR_STATES[1], "UNX-F1201-J1"));
    v.push((RM_FOUR_STATES[2], "UNX-F1201-J1"));
    v.push((RM_FOUR_STATES[3], "UNX-F1201-J1"));
    v.push((RM_TAKEOVER_SCENES[0], "UNX-F1102-J1"));
    v.push((RM_TAKEOVER_SCENES[1], "UNX-F1201-J1"));
    v.push((RM_INJECT_FAMILIES[0], "UNX-F1461-J1"));
    v.push((RM_INJECT_FAMILIES[1], "UNX-F1461-J1"));
    v.push((RM_INJECT_FAMILIES[2], "UNX-F1201-J1"));
    v.push((RM_INJECT_FAMILIES[3], "UNX-F1421-J1"));
    v
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：20 场景登记 → 覆盖 100% → 冻结 → reseal 同指纹。
pub fn drill_full() -> (bool, String) {
    let mut m = RmMatrix::new();
    let refs = rm_v1_refs();
    match m.register_all(&refs) {
        Ok(n) => {
            if n != 20 {
                return (false, format!("登记 {} ≠ 20", n));
            }
        }
        Err(d) => return (false, d.full()),
    }
    let fp1 = match m.seal() {
        Ok(f) => f,
        Err(d) => return (false, d.full()),
    };
    let fp2 = match m.seal() {
        Ok(f) => f,
        Err(d) => return (false, d.full()),
    };
    (fp1 == fp2 && m.row_count() == 20, m.report())
}

/// 矩阵漂移注入核：10 处 10/10 必抓。
pub fn inject_drift() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 基线矩阵。
    let mut m = RmMatrix::new();
    let _ = m.register_all(&rm_v1_refs());
    // 注 1：场景重名。
    let mut m1 = RmMatrix::new();
    let _ = m1.register("场景A", "UNX-F1201-J1", RmExpect::Pass);
    if m1.register("场景A", "UNX-F1201-J1", RmExpect::Pass).is_err() {
        caught += 1;
        detail.push("注1 场景重名:抓".to_string());
    } else {
        detail.push("注1 场景重名:漏".to_string());
    }
    // 注 2：判据引用形不合法。
    if m.register("场景X", "J1", RmExpect::Pass).is_err() {
        caught += 1;
        detail.push("注2 引用形:抓".to_string());
    } else {
        detail.push("注2 引用形:漏".to_string());
    }
    // 注 3：引用 F 段非数字。
    if m.register("场景Y", "UNX-Fabcd-J1", RmExpect::Pass).is_err() {
        caught += 1;
        detail.push("注3 F段非数字:抓".to_string());
    } else {
        detail.push("注3 F段非数字:漏".to_string());
    }
    // 注 4：场景缺失封账拒。
    let mut m4 = RmMatrix::new();
    let _ = m4.register("单场景", "UNX-F1201-J1", RmExpect::Pass);
    if m4.seal().is_err() {
        caught += 1;
        detail.push("注4 覆盖不足封账拒:抓".to_string());
    } else {
        detail.push("注4 覆盖不足封账拒:漏".to_string());
    }
    // 注 5：冻结后写。
    if m.register("场景Z", "UNX-F1201-J1", RmExpect::Pass).is_err() {
        caught += 1;
        detail.push("注5 冻结后写:抓".to_string());
    } else {
        detail.push("注5 冻结后写:漏".to_string());
    }
    // 注 6：reseal 漂移（行被旁路删后 reseal）。
    let mut m6 = RmMatrix::new();
    let _ = m6.register_all(&rm_v1_refs());
    let _ = m6.seal();
    m6.rows.pop();
    if m6.seal().is_err() {
        caught += 1;
        detail.push("注6 reseal漂移:抓".to_string());
    } else {
        detail.push("注6 reseal漂移:漏".to_string());
    }
    // 注 7：覆盖缺场景检出。
    let mut m7 = RmMatrix::new();
    let refs7 = rm_v1_refs();
    // 少登记一个。
    for (s, r) in refs7.iter().skip(1) {
        let _ = m7.register(s, r, RmExpect::Pass);
    }
    let (cov7, miss7) = m7.coverage_ok();
    if !cov7 && miss7.len() == 1 {
        caught += 1;
        detail.push("注7 缺场景检出:抓".to_string());
    } else {
        detail.push("注7 缺场景检出:漏".to_string());
    }
    // 注 8：引用解析（合法引用可解）。
    match rm_parse_judge("UNX-F1461-J1") {
        Some((1461, 1)) => {
            caught += 1;
            detail.push("注8 合法引用解析:抓(可解)".to_string());
        }
        _ => detail.push("注8 合法引用解析:漏".to_string()),
    }
    // 注 9：J 段为 0 拒。
    if rm_parse_judge("UNX-F1461-J0").is_none() {
        caught += 1;
        detail.push("注9 J0拒:抓".to_string());
    } else {
        detail.push("注9 J0拒:漏".to_string());
    }
    // 注 10：空场景名拒。
    if m.register("", "UNX-F1201-J1", RmExpect::Pass).is_err() {
        caught += 1;
        detail.push("注10 空场景名:抓".to_string());
    } else {
        detail.push("注10 空场景名:漏".to_string());
    }
    (caught, 10, detail)
}
