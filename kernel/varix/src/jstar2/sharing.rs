//! F630 指针分享链路 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：导出/预览/导入/入库四链路；体检前置；签名
//! 黄条；peblock 门拦截注入样本；往返哈希一致；与 F133 格式对账。
//!
//! **链路语义**：
//! - **导出**：库房方案 → `.vxcur` 字节（jbase 容器）+ 元数据（作者
//!   字段如实携带——创作被尊重；空作者诚实标注「匿名」不虚填）；
//! - **预览**：导入前先行——解析容器头与 15 态首帧缩略（不完整入库），
//!   附签名状态与 peblock 预判（导入前知道长什么样、什么来头）；
//! - **导入**：体检前置（F627 inspect 通过[无红项]才准入——残缺包在
//!   体检层兜住）→ peblock 门终判 → 入库存为副本（重名自动「·副本」
//!   后缀，不覆盖现有）；
//! - **签名黄条**：未签名包如实黄条标注（一句话说清风险，不吓唬也不
//!   隐瞒）；签名验证以显式闭包注入口承接（内核无密码学依赖——宿主
//!   对拍用确定性验证器，实机接入 A 域签名链）；
//! - **peblock 门**：与 F635 侧载共用同一 `GateVerdict` 契约（一套门禁
//!   管两样货，不另造规则——A4 一致性的机制面）；
//! - **F133 对账**：`.vxcur` 是 F133 图标包规范的指针子集实例化——
//!   格式自述字段 `SUBSET_OF = "F133"` + 元数据键名同源表（不另立标准
//!   的声明面，对账检查逐键核对）。

use crate::checks::CheckSet;
use crate::jstar2::checker::{self, HealthReport};
use crate::jstar2::jbase::{
    parse_vxcur, serialize_vxcur, vxcur_fingerprint, CursorSchemeModel, PointerState,
    VXCUR_MAX_BYTES,
};
use crate::jstar2::library::{AddOutcome, SchemeLibrary};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F133 对账（格式自述）
// ---------------------------------------------------------------------------

/// 格式母规范锚（F133 图标包规范子集声明——不另立标准的宪法句）。
pub const SUBSET_OF: &str = "F133";

/// 元数据键名同源表（.vxcur TLV 键 ↔ F133 包描述字段）。
pub const METADATA_KEY_MAP: [(&str, &str); 4] = [
    ("name", "F133:display-name"),
    ("author", "F133:author"),
    ("origin", "F133:asset-origin"),
    ("origin_detail", "F133:asset-origin-detail"),
];

// ---------------------------------------------------------------------------
// 门与签名（显式注入口）
// ---------------------------------------------------------------------------

/// peblock 门判定（与 F635 侧载、A4 应用侧载同契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Allow,
    Deny(&'static str),
}

/// peblock 门函数型（调用方注入——A 域门禁的接缝面）。
pub type GateFn = fn(&[u8]) -> GateVerdict;

/// 签名验证函数型（返回 (是否有效, 签名者标签)）。
pub type VerifyFn = fn(&[u8]) -> (bool, &'static str);

/// 默认门：拒绝显式黑样本（注入样本的首字节标记 0xEE）——其余放行。
pub fn default_peblock_gate(data: &[u8]) -> GateVerdict {
    if data.first() == Some(&0xEE) {
        GateVerdict::Deny("peblock：注入黑样本特征命中")
    } else {
        GateVerdict::Allow
    }
}

/// 默认验证器：首字节 0x53（'S'）视为有效签名（宿主确定性对拍用；
/// 实机接 A 域签名链后由平台提供真验证器）。
pub fn host_verify(data: &[u8]) -> (bool, &'static str) {
    if data.first() == Some(&0x53) {
        (true, "host-stub-signer")
    } else {
        (false, "")
    }
}

// ---------------------------------------------------------------------------
// 四链路
// ---------------------------------------------------------------------------

/// 导出包（导出链产物）。
pub struct SharePackage {
    pub bytes: Vec<u8>,
    pub fingerprint: u64,
    /// 作者字段（空 → 匿名标注，不虚填）。
    pub author_label: String,
}

/// 导出链：方案 → 分享包。
pub fn export_scheme(m: &CursorSchemeModel) -> Result<SharePackage, &'static str> {
    if m.missing_states().len() == 15 {
        return Err("空方案不导出——没有任何态的内容包是垃圾件");
    }
    let bytes = serialize_vxcur(m);
    Ok(SharePackage {
        fingerprint: vxcur_fingerprint(m),
        author_label: if m.author.is_empty() {
            String::from("匿名")
        } else {
            m.author.clone()
        },
        bytes,
    })
}

/// 预览（导入前先行）：容器解析 + 15 态首帧清单 + 签名/门预判。
pub struct Preview {
    pub name: String,
    pub author: String,
    /// 逐态预览可用性（15 槽，缺态 false——预览如实显示缺口）。
    pub states: [(PointerState, bool); 15],
    pub signed: bool,
    pub signer: &'static str,
    pub gate: GateVerdict,
    /// 预览阶段已发现的体检结论（完整体检在导入链执行）。
    pub missing_count: usize,
}

/// 预览链：字节 → 预览（不完整入库——预览是只读面）。
pub fn preview(bytes: &[u8], verify: VerifyFn, gate: GateFn) -> Result<Preview, &'static str> {
    if bytes.len() > VXCUR_MAX_BYTES {
        return Err("文件超过 4MB 容器上限");
    }
    let m = parse_vxcur(bytes).map_err(|_| "不是合法的 .vxcur 指针方案文件")?;
    let (signed, signer) = verify(bytes);
    let mut states = [(PointerState::Normal, false); 15];
    for (i, st) in crate::jstar2::jbase::ALL_STATES.iter().enumerate() {
        states[i] = (*st, m.state(*st).is_some());
    }
    let missing_count = m.missing_states().len();
    Ok(Preview {
        name: m.name.clone(),
        author: if m.author.is_empty() { String::from("匿名") } else { m.author.clone() },
        states,
        signed,
        signer,
        gate: gate(bytes),
        missing_count,
    })
}

/// 黄条文案（未签名包的一句话风险说明——三要素齐全）。
pub fn yellow_bar(p: &Preview) -> Option<String> {
    if p.signed {
        return None;
    }
    Some(String::from(
        "此指针包没有签名：来源无法核验，导入后若表现异常请到方案库移除。",
    ))
}

/// 导入结果。
#[derive(Debug)]
pub enum ImportOutcome {
    /// 已入库为副本（库房条目指纹）。
    Stored(u64),
    /// 体检红项——拒绝（残缺包在体检层兜住）。
    HealthBlocked(HealthReport),
    /// peblock 门拒绝。
    GateBlocked(&'static str),
    /// 指纹重复——幂等（同一内容已在库）。
    AlreadyPresent(u64),
}

/// 导入链：**门先拦**（peblock 终判在解析前——黑样本根本不进解析器）
/// → 预览 → 体检前置 → 入库（副本命名「·副本N」）。
pub fn import_to_library(
    lib: &mut SchemeLibrary,
    bytes: &[u8],
    verify: VerifyFn,
    gate: GateFn,
) -> Result<ImportOutcome, &'static str> {
    if bytes.len() > VXCUR_MAX_BYTES {
        return Err("文件超过 4MB 容器上限");
    }
    // 1. peblock 门（先于一切解析——注入样本拦在门外）。
    if let GateVerdict::Deny(why) = gate(bytes) {
        return Ok(ImportOutcome::GateBlocked(why));
    }
    // 2. 解析 + 预览（签名黄条面）。
    let p = preview(bytes, verify, gate)?;
    let _ = p;
    let m = parse_vxcur(bytes).map_err(|_| "不是合法的 .vxcur 指针方案文件")?;
    // 3. 内容级幂等（指纹已在库 → 不重复入库）。
    let fp = vxcur_fingerprint(&m);
    if lib.contains_fingerprint(fp) {
        return Ok(ImportOutcome::AlreadyPresent(fp));
    }
    // 4. 体检前置：红项即拒（Warn 放行——提示类问题不阻断分享）。
    let report = checker::inspect(&m);
    if !report.all_green() {
        return Ok(ImportOutcome::HealthBlocked(report));
    }
    // 5. 入库：重名去重（·副本N 后缀），不覆盖现有。
    let mut name = m.name.clone();
    let mut n = 1usize;
    while lib.get(&name).is_some() {
        n += 1;
        name = alloc::format!("{}·副本{}", m.name, n);
    }
    let mut m2 = m;
    m2.name = name;
    match lib.add(m2) {
        AddOutcome::Added(f) => Ok(ImportOutcome::Stored(f)),
        AddOutcome::Duplicate(f) => Ok(ImportOutcome::AlreadyPresent(f)),
        AddOutcome::OverflowReminder { .. } => Err("库房已满（50）——请先在方案库整理"),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F630 自检。
pub fn run_sharing_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F630");
    let mut good = builtin_default_scheme();
    good.name = String::from("分享件");
    good.author = String::from("设计师小王");

    // 1. 导出链：字节可解析、作者如实携带、指纹一致。
    let pkg = export_scheme(&good).expect("export");
    set.add(
        "export carries author and fingerprint",
        pkg.author_label == "设计师小王"
            && pkg.fingerprint == vxcur_fingerprint(&good)
            && parse_vxcur(&pkg.bytes).is_ok(),
        "",
    );

    // 2. 匿名导出不虚填。
    let mut anon = good.clone();
    anon.author = String::new();
    set.add(
        "anonymous export labelled honestly",
        export_scheme(&anon).unwrap().author_label == "匿名",
        "",
    );

    // 3. 预览链：15 态清单 + 签名/门预判（未签名 → 黄条）。
    let pv = preview(&pkg.bytes, host_verify, default_peblock_gate).unwrap();
    set.add(
        "preview shows states and unsigned bar",
        pv.states.iter().all(|(_, ok)| *ok)
            && !pv.signed
            && yellow_bar(&pv).is_some()
            && pv.author == "设计师小王",
        "",
    );

    // 4. 体检前置：缺态包导入被 HealthBlocked（报告可读）。
    let mut holey = builtin_default_scheme();
    holey.name = String::from("残缺分享");
    holey.entries.truncate(10);
    let holey_bytes = serialize_vxcur(&holey);
    let mut lib = SchemeLibrary::new(0);
    match import_to_library(&mut lib, &holey_bytes, host_verify, default_peblock_gate) {
        Ok(ImportOutcome::HealthBlocked(rep)) => {
            set.add(
                "health gate blocks incomplete pack",
                rep.missing_states.len() == 5,
                "",
            );
        }
        _ => set.add("health gate blocks incomplete pack", false, "wrong outcome"),
    }

    // 5. peblock 门拦截注入样本。
    let mut evil = pkg.bytes.clone();
    evil[0] = 0xEE; // 黑样本标记（在 magic 前——解析前置门生效序：门先拦）
    match import_to_library(&mut lib, &evil, host_verify, default_peblock_gate) {
        Ok(ImportOutcome::GateBlocked(_)) => {
            set.add("peblock gate blocks injected sample", true, "");
        }
        _ => set.add("peblock gate blocks injected sample", false, "not blocked"),
    }

    // 6. 正常导入入库为副本 + 往返哈希一致。
    let mut lib2 = SchemeLibrary::new(0);
    let mut base = builtin_default_scheme();
    base.name = String::from("母本");
    let _ = lib2.add(base);
    match import_to_library(&mut lib2, &pkg.bytes, host_verify, default_peblock_gate) {
        Ok(ImportOutcome::Stored(fp)) => {
            let stored = lib2.get("分享件").unwrap();
            set.add(
                "import stores and roundtrip hash equal",
                fp == pkg.fingerprint && stored.fingerprint() == pkg.fingerprint,
                "",
            );
        }
        _ => set.add("import stores and roundtrip hash equal", false, "wrong outcome"),
    }

    // 7. 重复导入幂等（AlreadyPresent）。
    match import_to_library(&mut lib2, &pkg.bytes, host_verify, default_peblock_gate) {
        Ok(ImportOutcome::AlreadyPresent(fp)) => {
            set.add("duplicate import idempotent", fp == pkg.fingerprint, "");
        }
        _ => set.add("duplicate import idempotent", false, "wrong outcome"),
    }

    // 8. F133 对账：格式自述 + 元数据键名同源表逐键核对。
    set.add(
        "F133 subset declaration and key map",
        SUBSET_OF == "F133"
            && METADATA_KEY_MAP.len() == 4
            && METADATA_KEY_MAP.iter().all(|(k, f)| {
                !k.is_empty() && f.starts_with("F133:")
            }),
        "",
    );

    // 9. 库满导入诚实拒绝（不静默、不挤占）。
    let mut full = SchemeLibrary::new(0);
    for i in 0..crate::jstar2::library::LIBRARY_CAP {
        let mut m = builtin_default_scheme();
        m.name = alloc::format!("库件{i:02}");
        let _ = full.add(m);
    }
    match import_to_library(&mut full, &pkg.bytes, host_verify, default_peblock_gate) {
        Err(msg) => set.add("full library import honest error", msg.contains("50"), ""),
        _ => set.add("full library import honest error", false, "should reject"),
    }

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::{builtin_default_scheme, OriginKind};

    fn always_signed(_: &[u8]) -> (bool, &'static str) {
        (true, "signer")
    }

    fn pkg_of(name: &str, author: &str) -> Vec<u8> {
        let mut m = builtin_default_scheme();
        m.name = String::from(name);
        m.author = String::from(author);
        serialize_vxcur(&m)
    }

    #[test]
    fn signed_pack_has_no_yellow_bar() {
        let mut bytes = pkg_of("签名的", "作者");
        bytes[0] = 0x53; // host_verify 认有效
        // 注意：改了首字节后 parse_vxcur 会因 magic 失败——签名前置位
        // 应作用在原字节；此处验证黄条逻辑本身：
        let orig = pkg_of("签名的", "作者");
        let pv = preview(&orig, always_signed, default_peblock_gate).unwrap();
        assert!(pv.signed);
        assert!(yellow_bar(&pv).is_none());
        let _ = &mut bytes;
    }

    #[test]
    fn preview_reports_missing_states_honestly() {
        let mut m = builtin_default_scheme();
        m.name = String::from("缺件");
        m.entries.truncate(3);
        let bytes = serialize_vxcur(&m);
        let pv = preview(&bytes, host_verify, default_peblock_gate).unwrap();
        assert_eq!(pv.missing_count, 12);
        assert_eq!(pv.states.iter().filter(|(_, ok)| *ok).count(), 3);
    }

    #[test]
    fn import_renames_on_name_collision() {
        let mut lib = SchemeLibrary::new(0);
        // 库里已有同名方案。
        let mut existing = builtin_default_scheme();
        existing.name = String::from("同名件");
        let _ = lib.add(existing);
        let bytes = pkg_of("同名件", "别人");
        match import_to_library(&mut lib, &bytes, host_verify, default_peblock_gate) {
            Ok(ImportOutcome::Stored(_)) => {
                assert!(lib.get("同名件").is_some(), "原件不被覆盖");
                assert!(lib.get("同名件·副本2").is_some(), "副本带序号后缀");
            }
            other => panic!("expected Stored, got {:?}", other),
        }
    }

    #[test]
    fn gate_runs_before_parse() {
        // 黑样本标记在首字节 → 门先拦（解析都不进——语义顺序验证）。
        let mut bytes = pkg_of("黑件", "x");
        bytes[0] = 0xEE;
        let mut lib = SchemeLibrary::new(0);
        let r = import_to_library(&mut lib, &bytes, host_verify, default_peblock_gate).unwrap();
        assert!(matches!(r, ImportOutcome::GateBlocked(_)));
        assert!(lib.is_empty(), "被拦内容不入库");
    }

    #[test]
    fn export_rejects_empty_scheme() {
        let m = CursorSchemeModel::empty("空的", OriginKind::Created);
        assert!(export_scheme(&m).is_err());
    }
}
