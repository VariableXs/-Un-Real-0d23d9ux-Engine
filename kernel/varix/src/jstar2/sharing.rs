//! F630 指针分享链路 · 完整设计（STAR I 主册 J-C 组）· v2 深化版。
//!
//! **判据（主册原文）**：导出/预览/导入/入库四链路；体检前置；签名
//! 黄条；peblock 门拦截注入样本；往返哈希一致；与 F133 格式对账。
//!
//! **链路语义**：
//! - **导出**：库房方案 → `.vxcur` 字节（jbase 容器）+ 元数据（作者
//!   字段如实携带——创作被尊重；空作者诚实标注「匿名」不虚填）；
//!   导出选项模型：匿名化开关 + 轻量件模式（动画态只留首帧——论坛
//!   附件尺寸友好的静态预览件）；导出行为留痕台账（谁在何时导出了
//!   什么、指纹与模式——分享不是黑箱动作）；
//! - **预览**：导入前先行——解析容器头与 15 态首帧缩略（不完整入库），
//!   附签名状态与 peblock 预判（导入前知道长什么样、什么来头）；
//! - **导入会话状态机**：Fresh →（门预检）→ Previewed →（体检前置）
//!   → HealthPassed → commit 入库 / abort 显式取消——四链路的交互
//!   状态机化：预览后能反悔、体检结论先看后装、未预览不能直接提交
//!   （不盲装是流程属性而不是用户自觉）；
//! - **签名黄条**：未签名包如实黄条标注（三要素：发生了什么/为什么/
//!   下一步——不吓唬也不隐瞒）；门拒绝给红条（同三要素）；签名验证
//!   以显式闭包注入口承接（内核无密码学依赖——宿主对拍用确定性验证
//!   器，实机接入 A 域签名链）；
//! - **peblock 门**：与 F635 侧载共用同一 `GateVerdict` 契约（一套门禁
//!   管两样货，不另造规则——A4 一致性的机制面）；
//! - **F133 对账**：`.vxcur` 是 F133 图标包规范的指针子集实例化——
//!   格式自述字段 `SUBSET_OF = "F133"` + 元数据键名同源表 + **容器
//!   字节级键对账**（从真实容器解析出元数据键集合，与映射表逐键核对
//!   ——对账函数吃的是字节不是文档）；
//! - **往返保真**：单次往返指纹一致 + 双重往返（导出→导入→再导出）
//!   逐字节一致——分享链的保真口径钉在字节级。

use crate::checks::CheckSet;
use crate::jstar2::checker::{self, HealthReport};
use crate::jstar2::jbase::{
    parse_vxcur, serialize_vxcur, vxcur_fingerprint, CursorFrame, CursorSchemeModel, PointerState,
    VXCUR_MAGIC, VXCUR_MAX_BYTES,
};
use crate::jstar2::library::{AddOutcome, SchemeLibrary};
use alloc::string::{String, ToString};
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

/// 从 .vxcur 容器字节解析元数据键集合（字节级对账的解析面——
/// 布局：magic(6) | u16 meta_len | u16 state_count | u8 pair_count |
///       逐对: u8 key_len | key | u16 val_len | val）。
pub fn f133_meta_keys(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    if bytes.len() < VXCUR_MAGIC.len() + 5 || &bytes[..VXCUR_MAGIC.len()] != &VXCUR_MAGIC[..] {
        return Err("不是合法的 .vxcur 容器——键对账无从谈起");
    }
    let mut off = VXCUR_MAGIC.len();
    let meta_len = u16::from_le_bytes([bytes[off], bytes[off + 1]]) as usize;
    off += 2;
    let _state_count = u16::from_le_bytes([bytes[off], bytes[off + 1]]) as usize;
    off += 2;
    if off + meta_len > bytes.len() {
        return Err("容器元数据区越界");
    }
    let meta_end = off + meta_len;
    let pair_count = u16::from_le_bytes([bytes[off], bytes[off + 1]]) as usize;
    off += 2;
    let mut keys = Vec::new();
    for _ in 0..pair_count {
        if off >= meta_end {
            return Err("元数据对数越界");
        }
        let klen = bytes[off] as usize;
        off += 1;
        if off + klen > meta_end {
            return Err("键名越界");
        }
        keys.push(core::str::from_utf8(&bytes[off..off + klen]).map_err(|_| "键名非 UTF-8")?.to_string());
        off += klen;
        if off + 2 > meta_end {
            return Err("值长度越界");
        }
        let vlen = u16::from_le_bytes([bytes[off], bytes[off + 1]]) as usize;
        off += 2 + vlen;
        if off > meta_end {
            return Err("值越界");
        }
    }
    Ok(keys)
}

/// F133 键对账（吃真实容器字节）：元数据键集合与映射表逐键核对——
/// 键集不一致即格式漂移（对账函数有牙，不是文档声明了事）。
pub fn f133_reconcile(bytes: &[u8]) -> Result<(), &'static str> {
    let keys = f133_meta_keys(bytes)?;
    let expected: Vec<&str> = METADATA_KEY_MAP.iter().map(|(k, _)| *k).collect();
    if keys.len() != expected.len() {
        return Err("F133 键对账失败：元数据键数量与映射表不符");
    }
    for k in &expected {
        if !keys.iter().any(|g| g == k) {
            return Err("F133 键对账失败：容器缺少映射表键");
        }
    }
    Ok(())
}

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
// 导出选项与留痕
// ---------------------------------------------------------------------------

/// 导出选项（分享动作的参数面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    /// 匿名化：作者字段清空（导出者不想署名是权利，不是漏填）。
    pub anonymize: bool,
    /// 轻量件：每个动画态只保留首帧（论坛附件友好的静态预览件）。
    pub lite_static_only: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { anonymize: false, lite_static_only: false }
    }
}

/// 导出包（导出链产物）。
pub struct SharePackage {
    pub bytes: Vec<u8>,
    pub fingerprint: u64,
    /// 作者字段（空 → 匿名标注，不虚填）。
    pub author_label: String,
    /// 本包是否轻量件（预览/详情页如实标注口径来源）。
    pub lite: bool,
}

/// 导出链：方案 → 分享包（选项化）。
pub fn export_scheme_opts(
    m: &CursorSchemeModel,
    opts: ExportOptions,
) -> Result<SharePackage, &'static str> {
    if m.missing_states().len() == 15 {
        return Err("空方案不导出——没有任何态的内容包是垃圾件");
    }
    let mut out = m.clone();
    if opts.anonymize {
        out.author = String::new();
    }
    if opts.lite_static_only {
        for e in out.entries.iter_mut() {
            if e.frames.len() > 1 {
                e.frames.truncate(1);
            }
        }
    }
    let bytes = serialize_vxcur(&out);
    Ok(SharePackage {
        fingerprint: vxcur_fingerprint(&out),
        author_label: if out.author.is_empty() {
            String::from("匿名")
        } else {
            out.author.clone()
        },
        bytes,
        lite: opts.lite_static_only,
    })
}

/// 导出链（默认选项——兼容既有调用面）。
pub fn export_scheme(m: &CursorSchemeModel) -> Result<SharePackage, &'static str> {
    export_scheme_opts(m, ExportOptions::default())
}

/// 导出留痕记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRecord {
    pub at_ms: u64,
    pub name: String,
    pub fingerprint: u64,
    pub lite: bool,
}

/// 导出留痕台账（环形，容量 32——分享动作可回溯：导出了什么、何时、
/// 什么模式；超容丢最旧并如实滚动）。
#[derive(Clone, Debug, Default)]
pub struct ExportLedger {
    records: Vec<ExportRecord>,
    dropped: usize,
}

impl ExportLedger {
    pub const CAP: usize = 32;

    pub fn record(&mut self, at_ms: u64, name: &str, fingerprint: u64, lite: bool) {
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(ExportRecord {
            at_ms,
            name: String::from(name),
            fingerprint,
            lite,
        });
    }

    pub fn records(&self) -> &[ExportRecord] {
        &self.records
    }

    /// 超容丢最旧计数（容量纪律对账面）。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 最近一次导出（空台账诚实 None）。
    pub fn last(&self) -> Option<&ExportRecord> {
        self.records.last()
    }
}

// ---------------------------------------------------------------------------
// 预览与提示条（三要素文案）
// ---------------------------------------------------------------------------

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

/// 黄条文案（未签名包的一句话风险说明——三要素齐全：发生了什么/
/// 为什么/下一步）。
pub fn yellow_bar(p: &Preview) -> Option<String> {
    if p.signed {
        return None;
    }
    Some(String::from(
        "此指针包没有签名：来源无法核验，导入后若表现异常请到方案库移除。",
    ))
}

/// 红条文案（门拒绝的三要素——拦了说人话，不裸抛错误码）。
pub fn red_bar(reason: &str) -> String {
    let mut s = String::from("导入被安全门拦截：");
    s.push_str(reason);
    s.push_str("。下一步：如认为误判请联系包作者重新打包，或改用已签名的分享件。");
    s
}

// ---------------------------------------------------------------------------
// 导入会话状态机
// ---------------------------------------------------------------------------

/// 导入会话阶段（状态机：每一步都有明确出口，取消是显式出路）。
#[derive(Clone, Debug, PartialEq, Eq)]
enum SessionStage {
    /// 新会话（门预检已过）。
    Fresh,
    /// 已预览（签名黄条已呈现）。
    Previewed,
    /// 体检已过（红项零——可提交）。
    HealthPassed,
    /// 已终局（入库/被拦/取消——终态不可再推进）。
    Finished,
    /// 用户显式取消。
    Aborted,
}

/// 导入会话（四链路的交互状态机化：不盲装是流程属性）。
pub struct ImportSession {
    bytes: Vec<u8>,
    stage: SessionStage,
    preview: Option<Preview>,
    health: Option<HealthReport>,
    /// 门预检结论（Fresh 即判——黑样本进不了预览）。
    gate_verdict: GateVerdict,
}

impl ImportSession {
    /// 开启会话：先过 4MB 上限，再过 peblock 门预检（黑样本在解析前拦下）。
    pub fn start(bytes: &[u8], gate: GateFn) -> Result<ImportSession, &'static str> {
        if bytes.len() > VXCUR_MAX_BYTES {
            return Err("文件超过 4MB 容器上限");
        }
        Ok(ImportSession {
            bytes: bytes.to_vec(),
            stage: SessionStage::Fresh,
            preview: None,
            health: None,
            gate_verdict: gate(bytes),
        })
    }

    /// 门预检结论（只读面——UI 在 start 后立即可呈现拦截原因）。
    pub fn gate_verdict(&self) -> GateVerdict {
        self.gate_verdict
    }

    /// 当前阶段（只读面）。
    pub fn stage(&self) -> &'static str {
        match self.stage {
            SessionStage::Fresh => "fresh",
            SessionStage::Previewed => "previewed",
            SessionStage::HealthPassed => "health-passed",
            SessionStage::Finished => "finished",
            SessionStage::Aborted => "aborted",
        }
    }

    /// 预览（Fresh → Previewed；门被拦的会话诚实拒绝推进）。
    pub fn preview(&mut self, verify: VerifyFn) -> Result<&Preview, &'static str> {
        if self.stage != SessionStage::Fresh {
            return Err("会话不在 Fresh 阶段——预览不可重复执行");
        }
        if let GateVerdict::Deny(_) = self.gate_verdict {
            self.stage = SessionStage::Finished;
            return Err("peblock 门已拒绝——会话终止");
        }
        let p = preview(&self.bytes, verify, |_| GateVerdict::Allow)?;
        self.preview = Some(p);
        self.stage = SessionStage::Previewed;
        Ok(self.preview.as_ref().unwrap())
    }

    /// 预览结论只读视图。
    pub fn preview_view(&self) -> Option<&Preview> {
        self.preview.as_ref()
    }

    /// 体检前置（Previewed → HealthPassed / 留在 Previewed 且返回报告
    /// ——红项先看后装：报告给用户，不静默拒绝）。
    pub fn run_health(&mut self) -> Result<&HealthReport, &'static str> {
        if self.stage != SessionStage::Previewed {
            return Err("体检前置要求已预览——先看内容再谈健康");
        }
        let m = parse_vxcur(&self.bytes).map_err(|_| "不是合法的 .vxcur 指针方案文件")?;
        let report = checker::inspect(&m);
        let green = report.all_green();
        self.health = Some(report);
        if green {
            self.stage = SessionStage::HealthPassed;
        }
        Ok(self.health.as_ref().unwrap())
    }

    /// 体检结论只读视图。
    pub fn health(&self) -> Option<&HealthReport> {
        self.health.as_ref()
    }

    /// 提交入库（HealthPassed → Finished；未体检/非绿拒绝——流程闸不可绕）。
    pub fn commit(self, lib: &mut SchemeLibrary) -> Result<ImportOutcome, &'static str> {
        if self.stage != SessionStage::HealthPassed {
            return Err("提交要求体检全绿——未预览/未体检/有红项都不能装");
        }
        let m = parse_vxcur(&self.bytes).map_err(|_| "不是合法的 .vxcur 指针方案文件")?;
        import_model(lib, m)
    }

    /// 显式取消（任何非终态可取消——取消不写库不留痕）。
    pub fn abort(&mut self) -> bool {
        if matches!(self.stage, SessionStage::Finished | SessionStage::Aborted) {
            return false;
        }
        self.stage = SessionStage::Aborted;
        true
    }
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

/// 入库公共尾（重名「·副本N」后缀，不覆盖现有）。
fn import_model(lib: &mut SchemeLibrary, m: CursorSchemeModel) -> Result<ImportOutcome, &'static str> {
    let fp = vxcur_fingerprint(&m);
    if lib.contains_fingerprint(fp) {
        return Ok(ImportOutcome::AlreadyPresent(fp));
    }
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

/// 导入链（一步式兼容面）：**门先拦**（peblock 终判在解析前——黑样本
/// 根本不进解析器）→ 预览 → 体检前置 → 入库（副本命名）。
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
    // 3. 内容级幂等 + 4. 体检前置 + 5. 入库。
    if lib.contains_fingerprint(vxcur_fingerprint(&m)) {
        return Ok(ImportOutcome::AlreadyPresent(vxcur_fingerprint(&m)));
    }
    let report = checker::inspect(&m);
    if !report.all_green() {
        return Ok(ImportOutcome::HealthBlocked(report));
    }
    import_model(lib, m)
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

    // 8. F133 对账：格式自述 + 元数据键名同源表逐键核对 + 容器字节级键对账。
    set.add(
        "F133 subset declaration and key map",
        SUBSET_OF == "F133"
            && METADATA_KEY_MAP.len() == 4
            && METADATA_KEY_MAP.iter().all(|(k, f)| {
                !k.is_empty() && f.starts_with("F133:")
            }),
        "",
    );
    set.add(
        "F133 container byte-level key reconcile",
        f133_reconcile(&pkg.bytes).is_ok() && f133_meta_keys(&pkg.bytes).unwrap().len() == 4,
        "",
    );
    // 非容器字节诚实报错（对账函数有牙）。
    set.add(
        "F133 reconcile rejects non-container",
        f133_reconcile(b"garbage data!!").is_err(),
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

    // 10. 导出选项：轻量件只留首帧 + 匿名化组合。
    let mut anim = builtin_default_scheme();
    anim.name = String::from("动画件");
    anim.author = String::from("作者甲");
    // 给 Normal 态加第 2 帧（动画件）。
    if let Some(e) = anim.state_mut(PointerState::Normal) {
        let f0 = e.frames[0].clone();
        e.frames.push(CursorFrame::from_buf(f0.hot_x, f0.hot_y, 33, f0.buf()));
    }
    let lite = export_scheme_opts(&anim, ExportOptions { anonymize: true, lite_static_only: true }).unwrap();
    let lite_parsed = parse_vxcur(&lite.bytes).unwrap();
    set.add(
        "lite export keeps first frame only and anonymizes",
        lite.lite
            && lite.author_label == "匿名"
            && lite_parsed.state(PointerState::Normal).unwrap().frames.len() == 1,
        "",
    );
    // 轻量件延时不丢（元数据保真——首帧 delay 原样）。
    let orig_delay = anim.state(PointerState::Normal).unwrap().frames[0].delay_ms;
    set.add(
        "lite export preserves first frame delay",
        lite_parsed.state(PointerState::Normal).unwrap().frames[0].delay_ms == orig_delay,
        "",
    );

    // 11. 导入会话状态机：完整快乐路径（start→preview→health→commit）。
    let mut sess = ImportSession::start(&pkg.bytes, default_peblock_gate).unwrap();
    let fresh_ok = sess.stage() == "fresh";
    let pv2 = sess.preview(host_verify).unwrap().clone_preview_ok();
    let health_ok = sess.run_health().unwrap().all_green();
    let committed = sess.commit(&mut SchemeLibrary::new(0));
    set.add(
        "import session state machine happy path",
        fresh_ok && pv2 && health_ok && matches!(committed, Ok(ImportOutcome::Stored(_))),
        "",
    );

    // 12. 会话取消出路：预览后 abort 不写库；终态后 abort 拒绝。
    let mut sess2 = ImportSession::start(&pkg.bytes, default_peblock_gate).unwrap();
    let _ = sess2.preview(host_verify);
    let aborted = sess2.abort();
    let mut untouched = SchemeLibrary::new(0);
    let after_abort = sess2.commit(&mut untouched).is_err() && untouched.is_empty();
    set.add(
        "import session abort leaves library untouched",
        aborted && after_abort,
        "",
    );

    // 13. 流程闸：未预览直接 commit 拒绝（不盲装是流程属性）。
    let sess3 = ImportSession::start(&pkg.bytes, default_peblock_gate).unwrap();
    set.add(
        "commit without preview honestly rejected",
        sess3.commit(&mut SchemeLibrary::new(0)).is_err(),
        "",
    );

    // 14. 门拦会话：黑样本 start 即判，preview 终止会话。
    let mut sess4 = ImportSession::start(&evil, default_peblock_gate).unwrap();
    let gate_blocked = matches!(sess4.gate_verdict(), GateVerdict::Deny(_));
    let preview_refused = sess4.preview(host_verify).is_err() && sess4.stage() == "finished";
    set.add(
        "session gate blocks before preview",
        gate_blocked && preview_refused,
        "",
    );

    // 15. 红条三要素：门拒绝文案说人话（发生了什么/为什么/下一步）。
    let reason = match default_peblock_gate(&evil) {
        GateVerdict::Deny(r) => r,
        _ => "",
    };
    let red = red_bar(reason);
    set.add(
        "red bar three elements on gate denial",
        red.contains("拦截") && red.contains("peblock") && red.contains("下一步"),
        "",
    );

    // 16. 导出留痕台账：记录、封顶滚动、最近一次可查。
    let mut ledger = ExportLedger::default();
    for i in 0..40u64 {
        ledger.record(i, "件", i * 7, i % 2 == 0);
    }
    set.add(
        "export ledger records and caps",
        ledger.records().len() == ExportLedger::CAP
            && ledger.dropped() == 8
            && ledger.last().unwrap().at_ms == 39,
        "",
    );

    // 17. 双重往返逐字节一致（导出→导入→再导出）。
    let pkg_a = export_scheme(&good).unwrap();
    let reparsed = parse_vxcur(&pkg_a.bytes).unwrap();
    let pkg_b = export_scheme(&reparsed).unwrap();
    set.add(
        "double roundtrip byte identical",
        pkg_a.bytes == pkg_b.bytes && pkg_a.fingerprint == pkg_b.fingerprint,
        "",
    );

    set
}

// 预览克隆便捷口（检查代码用——Preview 字段面只读）。
impl Preview {
    fn clone_preview_ok(&self) -> bool {
        !self.name.is_empty()
    }
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

    #[test]
    fn session_health_red_stays_previewed_and_blocks_commit() {
        let mut m = builtin_default_scheme();
        m.name = String::from("缺件会话");
        m.entries.truncate(3);
        let bytes = serialize_vxcur(&m);
        let mut sess = ImportSession::start(&bytes, default_peblock_gate).unwrap();
        let _ = sess.preview(host_verify).unwrap();
        let rep = sess.run_health().unwrap();
        assert!(!rep.all_green());
        assert_eq!(sess.stage(), "previewed", "红项会话停在预览态");
        let err = sess.commit(&mut SchemeLibrary::new(0)).unwrap_err();
        assert!(err.contains("体检"), "未过体检不能提交");
    }

    #[test]
    fn session_double_preview_rejected() {
        let bytes = pkg_of("件", "a");
        let mut sess = ImportSession::start(&bytes, default_peblock_gate).unwrap();
        assert!(sess.preview(host_verify).is_ok());
        assert!(sess.preview(host_verify).is_err(), "预览不可重复执行");
    }

    #[test]
    fn lite_export_multi_frame_states_all_truncated() {
        let mut m = builtin_default_scheme();
        m.name = String::from("全动画");
        for st in crate::jstar2::jbase::ALL_STATES {
            if let Some(e) = m.state_mut(st) {
                let f0 = e.frames[0].clone();
                e.frames.push(CursorFrame::from_buf(f0.hot_x, f0.hot_y, 50, f0.buf()));
            }
        }
        let lite = export_scheme_opts(&m, ExportOptions { anonymize: false, lite_static_only: true }).unwrap();
        let parsed = parse_vxcur(&lite.bytes).unwrap();
        assert!(parsed.entries.iter().all(|e| e.frames.len() == 1), "所有态都截到单帧");
    }

    #[test]
    fn ledger_last_reflects_most_recent() {
        let mut ledger = ExportLedger::default();
        assert!(ledger.last().is_none(), "空台账诚实 None");
        ledger.record(5, "甲件", 100, false);
        ledger.record(9, "乙件", 200, true);
        let last = ledger.last().unwrap();
        assert_eq!(last.name, "乙件");
        assert!(last.lite);
        assert_eq!(ledger.dropped(), 0);
    }

    #[test]
    fn f133_keys_roundtrip_through_container() {
        let bytes = pkg_of("键对账", "我");
        let keys = f133_meta_keys(&bytes).unwrap();
        assert_eq!(keys, alloc::vec![String::from("name"), String::from("author"), String::from("origin"), String::from("origin_detail")]);
        assert!(f133_reconcile(&bytes).is_ok());
    }
}
