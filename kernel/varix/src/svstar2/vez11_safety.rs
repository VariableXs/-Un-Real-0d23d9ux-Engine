//! VE-F5211 · 特效安全（VE-Z 域 · 安全面 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5211`
//!
//! **判据（锚点原文）**：资源校验、参数清洗、光敏红线、隔离、判据。
//!
//! **职责定位（锚点原文）**：特效安全——三防：特效资源校验（特效引用
//! 的资源（贴图/模型）完整性校验——坏资产隔离）/参数注入防护（特效参
//! 数来自脚本时全清洗——F5066 桥消费）/闪烁安全（特效闪烁参数强制过
//! F4508 光敏检测——红线级）。
//!
//! **数据结构（锚点原文）**：资源校验器；参数清洗器；光敏检测接入。
//!
//! **错误路径与降级矩阵（锚点原文）**：坏资产→隔离+告警；注入→拒绝；
//! 闪烁超标→阻断+强制修正（红线级）。
//!
//! **性能逐项分解（锚点原文）**：校验 O(资源数)；拒绝 O(1)；阻断 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4508 红线上游；F5066 桥消费；F5215
//! fuzz。
//!
//! **无障碍与隐私（锚点原文）**：光敏红线即最高安全项（域本色核心）；
//! 审计脱敏。
//!
//! # 深化说明：安全三防不是三层建议——坏资产进不了渲染，注入进不了
//! ## 参数，超标闪烁进不了屏幕
//!
//! 「资源完整性校验」如果只在下载时验一次，运行期磁盘坏块/内存翻转
//! 就会让坏贴图悄悄上屏——完整性必须是**每次引用都重验**的哈希对拍
//! （[`AssetChecker::verify`] FNV-1a 64 位重算比对），坏资产进隔离区
//! （[`Quarantine`]）+告警计数，隔离区资产被引用→
//! [`E_SF_QUARANTINE`]——隔离不是删除（坏在哪要可追查）但**绝不渲染**
//! ；「参数来自脚本时全清洗」落成双闸：注入形闭集（分号/管道/反引号/
//! 换行/引号/美元七形——F5066 桥注入的形态学）字符级扫描
//! [`E_SF_INJECT`] 拒绝 O(1)，类型面强转 i64（非数值
//! [`E_SF_BAD_PARAM`]）——清洗是类型面的事不是字符串修补的事；「闪
//! 烁安全红线级」落成 [`FlashGate`]：频率 >3Hz（WCAG 光敏口径）或深
//! 度超限 → **原参数阻断+安全参数强制生效**（[`FlashVerdict`]——不是
//! 建议修正，是修正后的参数直接顶上，红线没有商量），阻断 O(1)——
//! 锚点「阻断 O(1)」；三防判定全走**脱敏审计账**（[`SafetyAudit`] 只
//! 记名与判定码不记参数值——审计自身不成为隐私面）。
//!
//! ## 深化一：隔离区是三态账不是黑名单——在册/验讫/隔离各是一态
//!
//! 资产三态（[`AssetChecker`]）：在册（登记哈希）→验讫（重算一致）/
//! 隔离（重算不符——坏在哪次校验被发现的记录还在）。隔离区可查
//! （[`AssetChecker::is_quarantined`]）可点名（批量校验坏名单），隔离
//! 后引用一律拒——坏资产的传染路径（引用链）在第一环断掉。
//!
//! ## 深化二：清洗是 fail-fast——批里混一个注入形全批拒
//!
//! [`ParamSanitizer::clean_all`] 逐参清洗遇注入即停整批拒（不跳过坏
//! 的继续好的——脚本参数是**一个整体**，半清洗的参数集比全拒危险得多
//! ：特效拿到的是不完整语义）；拒绝计数分账（注入/类型各一账）——
//! 攻击面画像可分。
//!
//! ## 深化三：红线幂等——修正后的参数再检必过
//!
//! [`FlashGate::check`] 对修正参数（3Hz/30）再检必 passed（修正值在
//! 安全区内——红线不是抖动源）；边界恰值含端点（3Hz/30 恰好放行——
//! 与 F4405/F5207 含端点口径一致），4Hz 即拒——红线边界清晰无模糊带。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_SF_* 前缀）
// ---------------------------------------------------------------------------

/// 坏资产：完整性校验失败（哈希不符）或未登记。
pub const E_SF_BAD_ASSET: &str = "E_SF_BAD_ASSET";
/// 参数注入：脚本参数含注入形字符。
pub const E_SF_INJECT: &str = "E_SF_INJECT";
/// 闪烁超标：光敏红线阻断+强制修正。
pub const E_SF_FLASH: &str = "E_SF_FLASH";
/// 隔离区违规：被隔离资产被引用。
pub const E_SF_QUARANTINE: &str = "E_SF_QUARANTINE";
/// 参数类型错：脚本参数非数值。
pub const E_SF_BAD_PARAM: &str = "E_SF_BAD_PARAM";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_SF_BAD_ASSET => "坏资产：完整性校验失败或未登记",
        E_SF_INJECT => "参数注入：含注入形字符",
        E_SF_FLASH => "闪烁超标：光敏红线阻断+强制修正",
        E_SF_QUARANTINE => "隔离区违规：被隔离资产被引用",
        E_SF_BAD_PARAM => "参数类型错：非数值",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_SF_BAD_ASSET => "换完好资产或修复后重新登记",
        E_SF_INJECT => "剥离注入形字符再提参",
        E_SF_FLASH => "使用强制修正后的安全参数",
        E_SF_QUARANTINE => "先解除隔离（修复资产）再引用",
        E_SF_BAD_PARAM => "参数值改为整数文本",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、资源校验器：登记哈希→引用重验→坏资产隔离
// ---------------------------------------------------------------------------

/// FNV-1a 64 位（资源完整性指纹）。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 资源校验器：登记哈希+引用重验+隔离区（校验 O(资源数)——逐件对拍）。
#[derive(Clone, Debug, Default)]
pub struct AssetChecker {
    registry: Vec<(String, u64)>,
    /// 隔离区（坏资产名册——隔离不删除，坏在哪可追查）。
    pub quarantine: Vec<String>,
    /// 坏资产告警计数。
    pub bad_alerts: u32,
    /// 隔离资产被引用拦截计数。
    pub quarantine_blocks: u32,
}

impl AssetChecker {
    /// 空校验器。
    pub fn new() -> Self {
        AssetChecker::default()
    }

    /// **登记资产**：记完整性指纹（重复登记拒）。
    pub fn register(&mut self, name: &str, content: &str) -> Result<(), &'static str> {
        if self.registry.iter().any(|(n, _)| n == name) {
            return Err(E_SF_BAD_ASSET);
        }
        self.registry.push((String::from(name), fnv64(content.as_bytes())));
        Ok(())
    }

    /// **引用重验**（每次引用都验——运行期坏块逃不过）：未登记/
    /// 哈希不符 → 隔离+告警 [`E_SF_BAD_ASSET`]；已隔离再引用 →
    /// [`E_SF_QUARANTINE`]（传染路径第一环断掉）；验讫 Ok。
    pub fn verify(&mut self, name: &str, content: &str) -> Result<(), &'static str> {
        if self.quarantine.iter().any(|n| n == name) {
            self.quarantine_blocks += 1;
            return Err(E_SF_QUARANTINE);
        }
        let expect = match self.registry.iter().find(|(n, _)| n == name) {
            Some((_, h)) => *h,
            None => {
                self.bad_alerts += 1;
                self.quarantine.push(String::from(name));
                return Err(E_SF_BAD_ASSET);
            }
        };
        if fnv64(content.as_bytes()) != expect {
            self.bad_alerts += 1;
            self.quarantine.push(String::from(name));
            return Err(E_SF_BAD_ASSET);
        }
        Ok(())
    }

    /// **批量校验**（O(资源数)——逐件重验）：返回坏资产名册（点名）。
    pub fn verify_all(&mut self, set: &[(&str, &str)]) -> Vec<String> {
        let mut bad = Vec::new();
        let mut i = 0usize;
        while i < set.len() {
            let (name, content) = set[i];
            if self.verify(name, content).is_err() {
                bad.push(String::from(name));
            }
            i += 1;
        }
        bad
    }

    /// 隔离判据。
    pub fn is_quarantined(&self, name: &str) -> bool {
        self.quarantine.iter().any(|n| n == name)
    }

    /// 在册资产数。
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、参数清洗器：注入形闭集 + 类型面强转（F5066 桥消费）
// ---------------------------------------------------------------------------

/// 注入形闭集：分号/管道/反引号/换行/回车/引号/美元（F5066 桥注入
/// 形态学——字符级扫描，出现即拒不修补）。
pub const INJECT_PATTERNS: [char; 8] = [';', '|', '`', '\n', '\r', '"', '\'', '$'];

/// 注入形扫描（拒绝 O(1)——七形定长线性）。
pub fn sanitize_str(raw: &str) -> Result<(), &'static str> {
    let mut i = 0usize;
    while i < INJECT_PATTERNS.len() {
        if raw.contains(INJECT_PATTERNS[i]) {
            return Err(E_SF_INJECT);
        }
        i += 1;
    }
    Ok(())
}

/// 参数清洗器：脚本参数全清洗（清洗 O(参数数)；拒绝 O(1)——锚点）。
#[derive(Clone, Debug, Default)]
pub struct ParamSanitizer {
    /// 注入拦截计数。
    pub inject_blocks: u32,
    /// 类型错拦截计数。
    pub type_blocks: u32,
}

impl ParamSanitizer {
    /// 空清洗器。
    pub fn new() -> Self {
        ParamSanitizer::default()
    }

    /// **单参清洗**：注入形扫描优先（安全优先于类型）→ 类型面强转
    /// i64（非数值 [`E_SF_BAD_PARAM`]——清洗是类型面的事不是字符串
    /// 修补的事）。
    pub fn clean(&mut self, raw: &str) -> Result<i64, &'static str> {
        if sanitize_str(raw).is_err() {
            self.inject_blocks += 1;
            return Err(E_SF_INJECT);
        }
        match raw.parse::<i64>() {
            Ok(v) => Ok(v),
            Err(_) => {
                self.type_blocks += 1;
                Err(E_SF_BAD_PARAM)
            }
        }
    }

    /// **批清洗**（fail-fast——脚本参数是一个整体，半清洗的参数集比
    /// 全拒危险得多：特效拿到的是不完整语义）。全部通过才返回值集。
    pub fn clean_all(&mut self, pairs: &[(&str, &str)]) -> Result<Vec<(String, i64)>, &'static str> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < pairs.len() {
            let (name, raw) = pairs[i];
            let v = self.clean(raw)?;
            out.push((String::from(name), v));
            i += 1;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 三、光敏红线：闪烁参数强制过检（红线级——阻断+强制修正）
// ---------------------------------------------------------------------------

/// 闪烁频率安全上限（Hz——WCAG 光敏口径 3Hz）。
pub const FLASH_HZ_MAX: u32 = 3;
/// 闪烁深度安全上限（含端点——与 F4405/F5207 口径一致）。
pub const FLASH_DEPTH_MAX: i64 = 30;

/// 闪烁参数（整数面——无浮点无 libm）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashParams {
    /// 闪烁频率（Hz）。
    pub hz: u32,
    /// 闪烁深度（0..=100）。
    pub depth: i64,
}

/// 红线判定结果：passed=原参数放行；未 passed 时 effective=强制修正后
/// 的安全参数（红线没有商量——修正参数直接顶上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashVerdict {
    /// 原参数是否放行。
    pub passed: bool,
    /// 实际生效参数（passed=true 时=原参数；false 时=修正值）。
    pub effective: FlashParams,
    /// 判定码（passed 时空串；阻断时 [`E_SF_FLASH`]）。
    pub code: &'static str,
}

/// 光敏检测接入（红线级——最高安全项）。
#[derive(Clone, Debug, Default)]
pub struct FlashGate {
    /// 红线阻断计数。
    pub flash_blocks: u32,
    /// 强制修正计数。
    pub forced_fixes: u32,
}

impl FlashGate {
    /// 空闸。
    pub fn new() -> Self {
        FlashGate::default()
    }

    /// **红线检**（阻断 O(1)——锚点）：频率 >3Hz 或深度超限 → 原参数
    /// 阻断+安全参数强制生效+双计数；修正参数再检必过（红线幂等——
    /// 修正值在安全区内，红线不是抖动源）。
    pub fn check(&mut self, p: FlashParams) -> FlashVerdict {
        if p.hz > FLASH_HZ_MAX || p.depth > FLASH_DEPTH_MAX {
            self.flash_blocks += 1;
            self.forced_fixes += 1;
            let fixed_hz = if p.hz > FLASH_HZ_MAX { FLASH_HZ_MAX } else { p.hz };
            let fixed_depth = if p.depth > FLASH_DEPTH_MAX { FLASH_DEPTH_MAX } else { p.depth };
            return FlashVerdict {
                passed: false,
                effective: FlashParams { hz: fixed_hz, depth: fixed_depth },
                code: E_SF_FLASH,
            };
        }
        FlashVerdict { passed: true, effective: p, code: "" }
    }
}

// ---------------------------------------------------------------------------
// 四、脱敏审计账：只记名与判定码不记参数值
// ---------------------------------------------------------------------------

/// 安全审计账（审计脱敏——条目不含参数值，审计自身不成为隐私面）。
#[derive(Clone, Debug, Default)]
pub struct SafetyAudit {
    entries: Vec<(String, &'static str)>,
}

impl SafetyAudit {
    /// 空账。
    pub fn new() -> Self {
        SafetyAudit::default()
    }

    /// 记一条（名+判定码——无参数值位）。
    pub fn record(&mut self, name: &str, code: &'static str) {
        self.entries.push((String::from(name), code));
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判据。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 查条目判定码。
    pub fn code_of(&self, idx: usize) -> Option<&'static str> {
        self.entries.get(idx).map(|(_, c)| *c)
    }
}

// ---------------------------------------------------------------------------
// 五、三防总编排：资源→参数→闪烁（红线最后——最高优先）
// ---------------------------------------------------------------------------

/// 三防总检报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafetyReport {
    /// 验讫资产数。
    pub verified: usize,
    /// 清洗通过参数数。
    pub cleaned: usize,
    /// 实际生效闪烁参数（红线修正后）。
    pub flash_effective: FlashParams,
}

/// 特效安全总编排（三防串联——上线前唯一安全门）。
#[derive(Clone, Debug, Default)]
pub struct SafetyHub {
    pub assets: AssetChecker,
    pub params: ParamSanitizer,
    pub flash: FlashGate,
    pub audit: SafetyAudit,
}

impl SafetyHub {
    /// 空编排。
    pub fn new() -> Self {
        SafetyHub::default()
    }

    /// **三防总检**：资源批量校验（坏即 Err）→参数批清洗（注入/类型
    /// 错即 Err）→闪烁红线检（超标强制修正后放行生效参数）。三步全过
    /// 出 [`SafetyReport`]；审计账每步落一条（脱敏——只记名与码）。
    pub fn clear_for_launch(
        &mut self,
        fx: &str,
        assets: &[(&str, &str)],
        script_params: &[(&str, &str)],
        flash: FlashParams,
    ) -> Result<SafetyReport, &'static str> {
        // 防 1：资源完整性（批量 O(资源数)）。
        let bad = self.assets.verify_all(assets);
        if !bad.is_empty() {
            self.audit.record(fx, E_SF_BAD_ASSET);
            return Err(E_SF_BAD_ASSET);
        }
        // 防 2：参数注入清洗（fail-fast）。
        let cleaned = self.params.clean_all(script_params)?;
        self.audit.record(fx, E_SF_CLEAN_OK_MARKER);
        // 防 3：光敏红线（阻断+强制修正）。
        let verdict = self.flash.check(flash);
        self.audit.record(fx, verdict.code);
        if !verdict.passed {
            // 红线级：修正参数生效（不是拒绝上线——是带着安全参数上线）。
            return Ok(SafetyReport {
                verified: assets.len(),
                cleaned: cleaned.len(),
                flash_effective: verdict.effective,
            });
        }
        Ok(SafetyReport {
            verified: assets.len(),
            cleaned: cleaned.len(),
            flash_effective: verdict.effective,
        })
    }
}

/// 审计用「清洗通过」标记码（非错误码——审计账的判定码面）。
pub const E_SF_CLEAN_OK_MARKER: &str = "E_SF_CLEAN_OK";

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5211 域自检（判据 13 条六组：资源校验/隔离/参数清洗/光敏红线/总编排/判据元）。
pub fn run_vez11_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5211");

    // --- 资源校验（判据一）---
    // 资-01：登记+重验一致通过 + 批量 O(资源数) 全绿。
    let mut ac = AssetChecker::new();
    let r1 = ac.register("tex_rain", "PNGDATA-RAIN-V1");
    let r2 = ac.register("mesh_hit", "OBJDATA-HIT-V1");
    let v1 = ac.verify("tex_rain", "PNGDATA-RAIN-V1");
    let v2 = ac.verify("mesh_hit", "OBJDATA-HIT-V1");
    let batch = ac.verify_all(&[("tex_rain", "PNGDATA-RAIN-V1"), ("mesh_hit", "OBJDATA-HIT-V1")]);
    let asset_ok = r1.is_ok() && r2.is_ok() && v1.is_ok() && v2.is_ok()
        && batch.is_empty()
        && ac.len() == 2
        && !ac.is_empty()
        && ac.bad_alerts == 0;
    if asset_ok {
        s.ok("EZ11-资-01-登记重验批量全绿");
    } else {
        s.fail("EZ11-资-01-登记重验批量全绿", "完整性对拍失效");
    }
    // 资-02：坏资产（篡改内容哈希不符）→隔离+告警+批量坏名单点名。
    let mut ac2 = AssetChecker::new();
    let _ = ac2.register("tex_a", "AAAA");
    let _ = ac2.register("tex_b", "BBBB");
    let bad = ac2.verify_all(&[("tex_a", "AAAA"), ("tex_b", "BBBB-TAMPERED")]);
    let corrupt_ok = bad.len() == 1
        && bad[0] == "tex_b"
        && ac2.is_quarantined("tex_b")
        && !ac2.is_quarantined("tex_a")
        && ac2.bad_alerts == 1;
    if corrupt_ok {
        s.ok("EZ11-资-02-坏资产隔离告警点名");
    } else {
        s.fail("EZ11-资-02-坏资产隔离告警点名", "坏资产闸漏");
    }

    // --- 隔离（判据二）---
    // 离-01：隔离区资产被引用→E_SF_QUARANTINE 拒+计数（传染第一环断）。
    let rq = ac2.verify("tex_b", "BBBB");
    let quar_ok = rq == Err(E_SF_QUARANTINE) && ac2.quarantine_blocks == 1;
    // 未登记资产引用→隔离+告警（未知资产按坏资产处理）。
    let ru = ac2.verify("tex_ghost", "???");
    let unknown_ok = ru == Err(E_SF_BAD_ASSET) && ac2.is_quarantined("tex_ghost");
    if quar_ok && unknown_ok {
        s.ok("EZ11-离-01-隔离区引用拒绝");
    } else {
        s.fail("EZ11-离-01-隔离区引用拒绝", "隔离闸漏");
    }

    // --- 参数清洗（判据三）---
    // 清-01：合法数值过+非数值拒（类型面）+计数分账。
    let mut ps = ParamSanitizer::new();
    let c1 = ps.clean("42");
    let c2 = ps.clean("-7");
    let c3 = ps.clean("abc");
    let clean_ok = c1 == Ok(42) && c2 == Ok(-7) && c3 == Err(E_SF_BAD_PARAM)
        && ps.type_blocks == 1
        && ps.inject_blocks == 0;
    if clean_ok {
        s.ok("EZ11-清-01-类型面强转分账");
    } else {
        s.fail("EZ11-清-01-类型面强转分账", "清洗闸失效");
    }
    // 清-02：注入形七闭集逐形拒绝（分号/管道/反引号/换行/回车/引号/美元）。
    let mut ps2 = ParamSanitizer::new();
    let inj = [
        ("semi", "42;rm"),
        ("pipe", "42|cat"),
        ("backtick", "4`2"),
        ("newline", "42\nx"),
        ("cr", "42\rx"),
        ("dquote", "4\"2"),
        ("squote", "4'2"),
        ("dollar", "42$HOME"),
    ];
    let mut inj_ok = inj.len() == 8;
    for (_, raw) in inj.iter() {
        if ps2.clean(raw) != Err(E_SF_INJECT) {
            inj_ok = false;
        }
    }
    inj_ok = inj_ok && ps2.inject_blocks == 8 && ps2.type_blocks == 0;
    if inj_ok {
        s.ok("EZ11-清-02-注入形七闭集拒绝");
    } else {
        s.fail("EZ11-清-02-注入形七闭集拒绝", "注入闸漏");
    }
    // 清-03：批清洗 fail-fast——批里混注入形全批拒（半清洗比全拒危险）。
    let mut ps3 = ParamSanitizer::new();
    let good_batch = ps3.clean_all(&[("intensity", "30"), ("radius", "12")]);
    let mixed = ps3.clean_all(&[("intensity", "30"), ("radius", "12;drop")]);
    let failfast_ok = match good_batch {
        Ok(v) => v.len() == 2 && v[0].1 == 30 && v[1].1 == 12,
        Err(_) => false,
    } && mixed == Err(E_SF_INJECT);
    if failfast_ok {
        s.ok("EZ11-清-03-批清洗fail-fast");
    } else {
        s.fail("EZ11-清-03-批清洗fail-fast", "fail-fast 失效");
    }

    // --- 光敏红线（判据四）---
    // 红-01：超标阻断+强制修正（8Hz/80 → 3/30）+修正幂等+双计数。
    let mut fg = FlashGate::new();
    let v1 = fg.check(FlashParams { hz: 8, depth: 80 });
    let red1 = !v1.passed && v1.code == E_SF_FLASH
        && v1.effective == FlashParams { hz: 3, depth: 30 }
        && fg.flash_blocks == 1
        && fg.forced_fixes == 1;
    let v2 = fg.check(v1.effective);
    let idem = v2.passed && v2.effective == v1.effective;
    if red1 && idem {
        s.ok("EZ11-红-01-红线阻断强制修正幂等");
    } else {
        s.fail("EZ11-红-01-红线阻断强制修正幂等", "红线失效");
    }
    // 红-02：边界恰值含端点——3Hz/30 恰好放行，4Hz/31 即拒。
    let mut fg2 = FlashGate::new();
    let edge_ok = fg2.check(FlashParams { hz: 3, depth: 30 }).passed;
    let edge_bad_hz = !fg2.check(FlashParams { hz: 4, depth: 30 }).passed;
    let edge_bad_depth = !fg2.check(FlashParams { hz: 3, depth: 31 }).passed;
    if edge_ok && edge_bad_hz && edge_bad_depth {
        s.ok("EZ11-红-02-边界恰值含端点");
    } else {
        s.fail("EZ11-红-02-边界恰值含端点", "边界漂移");
    }

    // --- 总编排（判据五）---
    // 编-01：三防串联 happy path——资源全绿+参数全过+红线放行。
    let mut hub = SafetyHub::new();
    let _ = hub.assets.register("tex_r", "R-DATA");
    let rep = hub.clear_for_launch(
        "fx_p_rain",
        &[("tex_r", "R-DATA")],
        &[("intensity", "40"), ("radius", "8")],
        FlashParams { hz: 2, depth: 20 },
    );
    let happy = match rep {
        Ok(r) => r.verified == 1 && r.cleaned == 2 && r.flash_effective.hz == 2 && r.flash_effective.depth == 20,
        Err(_) => false,
    };
    // 编-02：闪烁超标走总编排——不拒上线而是带修正参数上线（红线级语义）。
    let rep2 = hub.clear_for_launch(
        "fx_p_strobe",
        &[("tex_r", "R-DATA")],
        &[("intensity", "50")],
        FlashParams { hz: 10, depth: 90 },
    );
    let redline = match rep2 {
        Ok(r) => r.flash_effective == FlashParams { hz: 3, depth: 30 },
        Err(_) => false,
    };
    // 编-03：审计账脱敏——条目只有名与判定码（无参数值位）+步序对拍
    // （两特效各两条：清洗码+红线码）。
    let audit_ok = hub.audit.len() == 4
        && hub.audit.code_of(0) == Some(E_SF_CLEAN_OK_MARKER)
        && hub.audit.code_of(1) == Some("")
        && hub.audit.code_of(2) == Some(E_SF_CLEAN_OK_MARKER)
        && hub.audit.code_of(3) == Some(E_SF_FLASH);
    if happy && redline && audit_ok {
        s.ok("EZ11-编-01-三防串联与脱敏审计");
    } else {
        s.fail("EZ11-编-01-三防串联与脱敏审计", "编排失效");
    }
    // 编-04：坏资产总检拦——批量含坏即 Err 且审计落坏资产码。
    let mut hub2 = SafetyHub::new();
    let _ = hub2.assets.register("tex_x", "X-DATA");
    let rb = hub2.clear_for_launch(
        "fx_p_bad",
        &[("tex_x", "X-DATA-TAMPERED")],
        &[("a", "1")],
        FlashParams { hz: 1, depth: 10 },
    );
    let hubbad = rb == Err(E_SF_BAD_ASSET)
        && hub2.audit.len() == 1
        && hub2.audit.code_of(0) == Some(E_SF_BAD_ASSET)
        && hub2.assets.is_quarantined("tex_x");
    if hubbad {
        s.ok("EZ11-编-02-坏资产总检拦截");
    } else {
        s.fail("EZ11-编-02-坏资产总检拦截", "总检闸漏");
    }

    // --- F5066 桥消费接缝（判据六附）---
    // 桥-01：清洗输出 i64 值集直供 F5207 校验器钳制（消费接缝真实调用）。
    let mut ps4 = ParamSanitizer::new();
    let vals = ps4.clean_all(&[("intensity", "250"), ("radius", "12")]);
    let mut vd = crate::svstar2::vez07_params::Validator::new();
    let _ = vd.book.define("intensity", 0, 100, 40, "特效强度");
    let _ = vd.book.define("radius", 1, 64, 16, "作用半径");
    let bridge_ok = match vals {
        Ok(v) => match vd.validate_all(&v) {
            Ok(out) => out.len() == 2 && out[0].1 == 100 && out[1].1 == 12,
            Err(_) => false,
        },
        Err(_) => false,
    } && vd.clamped_count == 1;
    if bridge_ok {
        s.ok("EZ11-桥-01-清洗值直供F5207钳制");
    } else {
        s.fail("EZ11-桥-01-清洗值直供F5207钳制", "消费接缝失效");
    }

    // --- 判据元（判据七）---
    // 元-01：五码互异 + 三要素齐备。
    let codes = [E_SF_BAD_ASSET, E_SF_INJECT, E_SF_FLASH, E_SF_QUARANTINE, E_SF_BAD_PARAM];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ11-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ11-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic 零 unsafe（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez11_safety.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ11-判据-02-零panic零unsafe面");
    } else {
        s.fail("EZ11-判据-02-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }
    // 元-03：三防计数分账互不串（同一 hub 上三类违规各归各账；
    // fx2 用独立资产——tex_m 已被 fx1 隔离，避免隔离闸先拦）。
    let mut hub3 = SafetyHub::new();
    let _ = hub3.assets.register("tex_m", "M");
    let _ = hub3.assets.register("tex_m2", "M2");
    let _ = hub3.clear_for_launch("fx1", &[("tex_m", "M-X")], &[("a", "1")], FlashParams { hz: 1, depth: 1 });
    let _ = hub3.clear_for_launch("fx2", &[("tex_m2", "M2")], &[("a", "1;b")], FlashParams { hz: 1, depth: 1 });
    let mixed_acct = hub3.assets.bad_alerts == 1
        && hub3.assets.quarantine_blocks == 0
        && hub3.params.inject_blocks == 1
        && hub3.params.type_blocks == 0
        && hub3.flash.flash_blocks == 0;
    if mixed_acct {
        s.ok("EZ11-判据-03-三防计数分账不串");
    } else {
        s.fail("EZ11-判据-03-三防计数分账不串", "计数串账");
    }

    s
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
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
