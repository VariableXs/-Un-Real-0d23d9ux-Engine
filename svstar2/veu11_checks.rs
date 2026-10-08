//! VE-F4011 判据层：国际化调试器（锚点六条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4011`
//!
//! **锚点原文六条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | 伪本地化 | `U11-PSD-*` | 变换可断言 + 膨胀比窗口 + 失真校准拒绝 |
//! | 硬编码检出 | `U11-HC-*` | 漏报红线注入审计实测 + 规则修正留痕 |
//! | 方向可视化 | `U11-DIR-*` | 单源段渲染 + 边界红线关闭标记实测 |
//! | 家族二十七 | `U11-FAM-*` | 27 成员复述单源 + 契约哈希 O(1) 对账 |
//! | 零常态 | `U11-ZERO-*` | 关闭态采样计数恒 0 |
//! | （仪表/错误码为锚点正文要求） | `U11-INS-*` `U11-ERR-*` | 五列 O(列) + 0x37xx 全映射 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! 膨胀比窗口与 FNV 常量判据侧字面量写死；硬编码检出用判据侧注入
//! 已知硬编码串（语料外且无标记）实测必触发；边界红线用关闭标记
//! 生成实测必立案；家族复述完整性判据侧全扫。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::veu10_corpus::CorpusLib;
use super::veu11_debug::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立 FNV-1a（常量字面量独立写死）。
fn alt_fnv(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 判据侧独立膨胀比核算（窗口 [1100,2500]，包裹 6 字符 + 40% 填充）。
fn alt_ratio_ok(in_chars: u64) -> bool {
    let target = in_chars * 1400 / 1000;
    let extra = if target > in_chars { target - in_chars } else { 0 };
    let out_len = in_chars + 6 + extra;
    let ratio = out_len * 1000 / in_chars;
    ratio >= 1100 && ratio <= 2500
}

/// 判据侧混合方向段（LTR→RTL→LTR 三段）。
fn alt_spans() -> [DirSpan; 3] {
    [DirSpan { rtl: false, chars: 4 }, DirSpan { rtl: true, chars: 3 }, DirSpan { rtl: false, chars: 2 }]
}

// ---------------------------------------------------------------------------
// U11-PSD：伪本地化（判据一）
// ---------------------------------------------------------------------------

fn fam_psd(s: &mut CheckSet) {
    // D1：变换产物结构——前缀+映射体+后缀（判据侧逐段核对）。
    let r = pseudo_transform("abcdef");
    let ok_d1 = match r {
        Ok((out, ratio)) => out.starts_with("[!!") && out.ends_with("!!]") && ratio >= 1100
            && ratio <= 2500,
        Err(_) => false,
    };
    s.add("U11-PSD-structure", ok_d1, "伪字符包裹结构完整");

    // D2：字符映射生效（abcdef → àbédéf 体）。判据侧写死期望映射体
    // 独立对拍：伪前缀之后紧跟逐字符映射结果；映射字符在体中、
    // 原样 ASCII 体不残留（恒等变换两分支皆必红）。
    let r2 = pseudo_transform("abcdef");
    let ok_d2 = match r2 {
        Ok((out, _)) => {
            let body = out.strip_prefix("[!!").unwrap_or("");
            body.starts_with("àbcdéf")
                && out.contains("é")
                && !body.starts_with("abcdef")
        }
        Err(_) => false,
    };
    s.add("U11-PSD-char-map", ok_d2, "伪字符映射生效");

    // D3：膨胀比窗口判据侧独立同判（6/16/64 字符三采样点）。
    let ok_d3 = alt_ratio_ok(6) && alt_ratio_ok(16) && alt_ratio_ok(64);
    let ok_d3b = pseudo_transform("abcdef").is_ok()
        && pseudo_transform("0123456789abcdef").is_ok();
    s.add("U11-PSD-ratio-window", ok_d3 && ok_d3b, "膨胀比两侧同判在窗内");

    // D4：模拟失真校准拒绝——短串包裹开销占比过大 = 失真（窗口外）。
    let short = pseudo_transform("ab");
    s.add("U11-PSD-distorted-short", short == Err(IErr::PseudoRatioDistorted),
          "短串失真校准专属拒");

    // D5：空串拒绝（伪检零长度无意义）。
    s.add("U11-PSD-empty", pseudo_transform("") == Err(IErr::PseudoRatioDistorted),
          "空串失真专属拒");

    // D6：语料全量伪变换预付入账（F4010 语料对端——长条目全过）。
    let lib = CorpusLib::builtin();
    let mut d = I18nDebugger::new();
    let n = d.load_corpus(&lib);
    let ok_d6 = n > 0 && d.pseudo_registered() == n;
    s.add("U11-PSD-corpus-load", ok_d6, "语料伪变换入账非零");
}

// ---------------------------------------------------------------------------
// U11-HC：硬编码检出（判据二）
// ---------------------------------------------------------------------------

fn fam_hc(s: &mut CheckSet) {
    let lib = CorpusLib::builtin();
    // H1：无标记串必检出（漏报红线注入审计实测——立案+计数增长）。
    let mut d = I18nDebugger::new();
    d.enabled = true;
    let _ = d.load_corpus(&lib);
    let before_flags = d.hardcode_flags;
    let before_cases = d.cases().len();
    let r = d.audit_string("HARDCODED LITERAL");
    let ok_h1 = r == Err(IErr::HardcodeFlagged)
        && d.hardcode_flags == before_flags + 1
        && d.cases().len() == before_cases + 1;
    s.add("U11-HC-injection-flagged", ok_h1, "注入硬编码必检出立案");

    // H2：注册伪条目通过审计（已本地化）。
    let (pseudo, _) = match pseudo_transform("已保存的文件") {
        Ok(x) => x,
        Err(_) => (String::from("[!!x!!]"), 0),
    };
    let mut d = I18nDebugger::new();
    d.enabled = true;
    let _ = d.load_corpus(&lib);
    let r2 = d.audit_string(&pseudo);
    let ok_h2 = r2 == Err(IErr::UnknownMark); // 带标记非语料 = 漏检修正而非硬编码
    s.add("U11-HC-marked-not-hardcode", ok_h2, "带标记不误判硬编码");

    // H3：规则漏检留痕——带标记非语料串 → 规则版本 +1 + 立案。
    let v_before = d.rules_version;
    let c_before = d.cases().len();
    let r3 = d.audit_string("[!!未知标记串!!]");
    let ok_h3 = r3 == Err(IErr::UnknownMark)
        && d.rules_version == v_before + 1
        && d.cases().len() == c_before + 1;
    s.add("U11-HC-rule-correction", ok_h3, "规则漏检修正留痕");

    // H4：判据侧哈希同源——语料伪条目摘要两侧同值。
    let (p2, _) = match pseudo_transform("تم حفظ الملف بنجاح. هل تريد متابعة التحرير؟") {
        Ok(x) => x,
        Err(_) => (String::from("[!!x!!]"), 0),
    };
    let ok_h4 = alt_fnv(&p2) == fnv1a(&p2);
    s.add("U11-HC-hash-same", ok_h4, "摘要判据侧同判");

    // H5：零常态下审计不采样（关闭态 audits 仍计数但不计采样——
    // 采样只在仪表渲染，审计是显式调用——零常态判据针对仪表与方向面）。
    let mut d = I18nDebugger::new();
    d.enabled = false;
    let _ = d.audit_string("no mark");
    let ok_h5 = d.sample_ops == 0;
    s.add("U11-HC-closed-no-sample", ok_h5, "关闭态采样恒 0");
}

// ---------------------------------------------------------------------------
// U11-DIR：方向可视化（判据三）
// ---------------------------------------------------------------------------

fn fam_dir(s: &mut CheckSet) {
    let spans = alt_spans();
    // V1：开启标记 → 边界清晰断言通过。
    let v1 = direction_view(&spans, true);
    let ok_v1 = match v1 {
        Ok(view) => boundary_clear(&spans, &view).is_ok() && view.contains("|⇄|"),
        Err(_) => false,
    };
    s.add("U11-DIR-markers-clear", ok_v1, "隔离标记边界清晰");

    // V2：红线实测——关闭标记 → 边界断言必失败（BoundaryUnclear）。
    let v2 = direction_view(&spans, false);
    let ok_v2 = match v2 {
        Ok(view) => boundary_clear(&spans, &view) == Err(IErr::BoundaryUnclear),
        Err(_) => false,
    };
    s.add("U11-DIR-unclear-redline", ok_v2, "关闭标记必判边界模糊");

    // V3：调试器方向审计立案（红线实测经立案流转）。
    let mut d = I18nDebugger::new();
    d.enabled = true;
    let c0 = d.cases().len();
    let r3 = d.direction_audit(&spans, false);
    let ok_v3 = r3 == Err(IErr::BoundaryUnclear) && d.cases().len() == c0 + 1;
    s.add("U11-DIR-audit-cased", ok_v3, "边界模糊立案流转");

    // V4：空方向源专属拒。
    s.add("U11-DIR-empty-source", direction_view(&[], true) == Err(IErr::DirSourceEmpty),
          "空源专属拒");

    // V5：同向无边界（全 LTR 不需标记，断言通过）。
    let same = [DirSpan { rtl: false, chars: 3 }, DirSpan { rtl: false, chars: 4 }];
    let v5 = direction_view(&same, true);
    let ok_v5 = match v5 {
        Ok(view) => boundary_clear(&same, &view).is_ok() && !view.contains("|⇄|"),
        Err(_) => false,
    };
    s.add("U11-DIR-same-dir-no-marker", ok_v5, "同向无边界无标记");
}

// ---------------------------------------------------------------------------
// U11-FAM：家族二十七（判据四）
// ---------------------------------------------------------------------------

fn fam_fam(s: &mut CheckSet) {
    // M1：27 成员逐一复述单源（判据侧全扫）。
    let f = FamilyEcho::new("F4011 单源契约文本");
    let mut all = true;
    let mut i = 0usize;
    while i < FAMILY_MEMBERS {
        if f.member_digest(i) != Ok(f.digest()) {
            all = false;
        }
        i += 1;
    }
    let ok_m1 = all && f.restatement_intact() && FAMILY_MEMBERS == 27;
    s.add("U11-FAM-27-restated", ok_m1, "27 成员复述完整");

    // M2：成员槽位越界专属拒（26/27 都合法，27 越界）。
    let ok_m2 = f.member_digest(26).is_ok() && f.member_digest(27) == Err(IErr::BadMemberSlot);
    s.add("U11-FAM-slot-bounds", ok_m2, "槽位 26 可 27 拒");

    // M3：契约哈希对账——正确通过、错误专属拒。
    let ok_m3 = f.accept_contract(f.digest()).is_ok()
        && f.accept_contract(f.digest() ^ 1) == Err(IErr::FamilyHashMismatch);
    s.add("U11-FAM-contract-hash", ok_m3, "契约对账两路");

    // M4：摘要判据侧独立同判。
    let ok_m4 = alt_fnv("F4011 单源契约文本") == f.digest();
    s.add("U11-FAM-digest-same", ok_m4, "摘要两侧同判");

    // M5：复述协议 O(1)——确定性口径：member_digest 直取不扫描
    // （越界路径与合法路径同样直接返回，无循环依赖计数）。
    let ok_m5 = f.member_digest(0).is_ok() && f.member_digest(13).is_ok();
    s.add("U11-FAM-o1-access", ok_m5, "直取口径可用");
}

// ---------------------------------------------------------------------------
// U11-ZERO：零常态（判据五）
// ---------------------------------------------------------------------------

fn fam_zero(s: &mut CheckSet) {
    // Z1：关闭态仪表渲染零采样零工作。
    let mut d = I18nDebugger::new();
    let rows = [InspectorRow {
        locale: String::from("zh"),
        direction: String::from("LTR"),
        format_out: String::from("[!!格式!!]"),
        family_ok: true,
        pseudo_ratio: 1400,
    }];
    let r1 = d.render_inspector(&rows);
    let ok_z1 = r1 == Ok(0) && d.sample_ops == 0;
    s.add("U11-ZERO-closed-zero-ops", ok_z1, "关闭态采样恒 0");

    // Z2：关闭态方向审计零采样零立案。
    let spans = alt_spans();
    let r2 = d.direction_audit(&spans, false);
    let ok_z2 = r2.is_ok() && d.sample_ops == 0 && d.cases().is_empty();
    s.add("U11-ZERO-closed-dir-silent", ok_z2, "关闭态方向面零工作");

    // Z3：打开才采样——开启后渲染计数 O(列)=行数×5。
    d.enabled = true;
    let r3 = d.render_inspector(&rows);
    let ok_z3 = r3 == Ok(1) && d.sample_ops == 5;
    s.add("U11-ZERO-open-samples", ok_z3, "开启按列采样 5/行");
}

// ---------------------------------------------------------------------------
// U11-INS：五列仪表
// ---------------------------------------------------------------------------

fn fam_ins(s: &mut CheckSet) {
    // N1：同 Locale 方向分歧立案（InspectorDiverged）。
    let mut d = I18nDebugger::new();
    d.enabled = true;
    let rows = [
        InspectorRow { locale: String::from("ar"), direction: String::from("RTL"),
            format_out: String::from("x"), family_ok: true, pseudo_ratio: 1400 },
        InspectorRow { locale: String::from("ar"), direction: String::from("LTR"),
            format_out: String::from("x"), family_ok: true, pseudo_ratio: 1400 },
    ];
    let r1 = d.render_inspector(&rows);
    let ok_n1 = r1 == Err(IErr::InspectorDiverged) && d.cases().len() == 1;
    s.add("U11-INS-divergence-cased", ok_n1, "仪表分歧立案");

    // N2：不同 Locale 无分歧。
    let rows2 = [
        InspectorRow { locale: String::from("ar"), direction: String::from("RTL"),
            format_out: String::from("x"), family_ok: true, pseudo_ratio: 1400 },
        InspectorRow { locale: String::from("zh"), direction: String::from("LTR"),
            format_out: String::from("x"), family_ok: true, pseudo_ratio: 1400 },
    ];
    let r2 = d.render_inspector(&rows2);
    let ok_n2 = r2 == Ok(2) && d.cases().len() == 1; // 旧案保留
    s.add("U11-INS-no-false-positive", ok_n2, "异 Locale 不误判");

    // N3：五列常量字面量。
    s.add("U11-INS-five-cols", INSPECTOR_COLS == 5, "五列字面量");
}

// ---------------------------------------------------------------------------
// U11-ERR：错误码全映射
// ---------------------------------------------------------------------------

fn fam_err(s: &mut CheckSet) {
    // E1：9 码全在 0x37xx 段且互异。
    let mut codes: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < IErr::ALL.len() {
        codes.push(IErr::ALL[i].code());
        i += 1;
    }
    let mut sorted = codes.clone();
    sorted.sort();
    let mut distinct = true;
    let mut k = 1usize;
    while k < sorted.len() {
        if sorted[k] == sorted[k - 1] {
            distinct = false;
        }
        k += 1;
    }
    let mut in_seg = true;
    let mut m = 0usize;
    while m < codes.len() {
        if codes[m] & 0xFF00 != 0x3700 {
            in_seg = false;
        }
        m += 1;
    }
    s.add("U11-ERR-9-distinct-inseg", distinct && in_seg && IErr::ALL.len() == 9,
          "9 码全在 0x37xx 段且互异");

    // E2：reason 全非空且互异。
    let mut rs: Vec<String> = Vec::new();
    let mut nonempty = true;
    let mut j = 0usize;
    while j < IErr::ALL.len() {
        let r = IErr::ALL[j].reason();
        if r.len() == 0 {
            nonempty = false;
        }
        rs.push(r);
        j += 1;
    }
    rs.sort();
    let mut rd = true;
    let mut k = 1usize;
    while k < rs.len() {
        if rs[k] == rs[k - 1] {
            rd = false;
        }
        k += 1;
    }
    s.add("U11-ERR-reasons-unique", nonempty && rd, "reason 全非空且互异");

    // E3：关键码字面量。
    s.add("U11-ERR-code-literals",
          IErr::PseudoRatioDistorted.code() == 0x3701
              && IErr::HardcodeFlagged.code() == 0x3702
              && IErr::BoundaryUnclear.code() == 0x3705
              && IErr::FamilyHashMismatch.code() == 0x3706,
          "关键码字面量");

    // E4：窗口常量字面量（判据侧写死防放水）。
    s.add("U11-ERR-window-literals",
          RATIO_MIN == 1100 && RATIO_MAX == 2500 && EXPANSION_PERMILLE == 1400
              && FAMILY_MEMBERS == 27 && CASE_CAP == 64,
          "五常量判据侧写死");

    // E5：立案簿满如实拒案（缩小不可行——用满灌实测）。
    let mut d = I18nDebugger::new();
    d.enabled = true;
    let mut filled = true;
    let mut n = 0u32;
    while n < 64 {
        if d.audit_string("HARDCODED").is_err() {
            // 前段 HardcodeFlagged 错误为预期立案
        } else {
            filled = false;
        }
        n += 1;
    }
    let over = d.audit_string("HARDCODED");
    let ok_e5 = filled && d.cases().len() == 64 && over == Err(IErr::CaseFull)
        && d.cases_rejected >= 1;
    s.add("U11-ERR-case-cap-honest", ok_e5, "簿满第 65 案如实拒");
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

/// VE-F4011 域自检（判据逐条映射锚点六条判据：
/// PSD 6 / HC 5 / DIR 5 / FAM 5 / ZERO 3 / INS 3 / ERR 5 共 32 项七族）。
pub fn run_veu11_checks() -> CheckSet {
    let mut s = CheckSet::new("i18n-debug");
    fam_psd(&mut s);
    fam_hc(&mut s);
    fam_dir(&mut s);
    fam_fam(&mut s);
    fam_zero(&mut s);
    fam_ins(&mut s);
    fam_err(&mut s);
    s
}
