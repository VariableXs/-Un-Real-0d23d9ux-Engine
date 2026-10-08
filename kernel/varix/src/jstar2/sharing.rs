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

// ---------------------------------------------------------------------------
// v3 深化批：许可声明面 · 分块传输模型 · 血统链 · 导出配额 ·
// 包 ID 碰撞对账 · 导入前重命名通道
// ---------------------------------------------------------------------------



/// 许可证枚举（分享的法治面：不写许可的包不给出预览黄条——三不承诺
/// 的「不托管」不等于「无规则」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum License {
    Cc0,
    CcBy,
    AllRightsReserved,
}

impl License {
    pub fn key(self) -> &'static str {
        match self {
            License::Cc0 => "CC0",
            License::CcBy => "CC-BY",
            License::AllRightsReserved => "ARR",
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            License::Cc0 => "公有领域（CC0）",
            License::CcBy => "署名（CC-BY）",
            License::AllRightsReserved => "保留所有权利",
        }
    }

    /// 许可是否允许再分发（ARR 只许个人导入不许转传——黄条升级红条的依据）。
    pub fn allows_reshare(self) -> bool {
        !matches!(self, License::AllRightsReserved)
    }

    fn from_key(k: &str) -> Option<License> {
        match k {
            "CC0" => Some(License::Cc0),
            "CC-BY" => Some(License::CcBy),
            "ARR" => Some(License::AllRightsReserved),
            _ => None,
        }
    }
}

/// 清单（license = author + license 键，行式文本挂在包字节头后不再改——
/// 清单是导出者写的，导入者只能读到，不能改写）。
pub fn manifest_line(author: &str, license: License) -> String {
    alloc::format!("license={}|{}\n", license.key(), author)
}

/// 从包字节读清单（无清单 = None——旧包兼容：预览黄条注明「未声明许可」）。
/// 清单只可能是头部第一行（导出者先写清单再拼包体）——只对首行做
/// UTF-8 解析，包体二进制不参与（整体解析会被二进制毒死）。
pub fn manifest_of(bytes: &[u8]) -> Option<(License, String)> {
    let head_end = bytes.iter().position(|&b| b == b'\n').unwrap_or(bytes.len().min(4096));
    let head = core::str::from_utf8(&bytes[..head_end]).ok()?;
    let rest = head.strip_prefix("license=")?;
    let (k, author) = rest.split_once('|')?;
    License::from_key(k).map(|l| (l, String::from(author)))
}

/// 分块传输模型：把包切成 chunk 字节块（序号 | 总数 | 载荷），对端
/// 重组后哈希对账（传输层损坏/丢块在重组时如实检出）。
pub const CHUNK_SIZE: usize = 64;

/// 切块（每块头 4 字节：序号 u16LE | 总数 u16LE——64KB+ 包体也够编，
/// u8 总数在内置方案 16KB+ 容器上溢是已修缺陷）。
pub fn chunkify(data: &[u8]) -> Vec<Vec<u8>> {
    let total = data.len().div_ceil(CHUNK_SIZE).max(1);
    let mut out = Vec::new();
    for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
        let seq = i as u16;
        let tot = total as u16;
        let mut block = alloc::vec![
            (seq & 0xFF) as u8,
            (seq >> 8) as u8,
            (tot & 0xFF) as u8,
            (tot >> 8) as u8
        ];
        block.extend_from_slice(chunk);
        out.push(block);
    }
    out
}

/// 重组（乱序进块也收——按序号重排；块数不足/序号越界 → None）。
pub fn reassemble(blocks: &[Vec<u8>], expect_fp: u64) -> Option<Vec<u8>> {
    if blocks.is_empty() || blocks[0].len() < 4 {
        return None;
    }
    let total = u16::from_le_bytes([blocks[0][2], blocks[0][3]]) as usize;
    if total == 0 || blocks.len() != total {
        return None;
    }
    let mut slots: Vec<Option<Vec<u8>>> = alloc::vec![None; total];
    for b in blocks {
        if b.len() < 4 {
            return None;
        }
        let idx = u16::from_le_bytes([b[0], b[1]]) as usize;
        if idx >= total {
            return None;
        }
        slots[idx] = Some(b[4..].to_vec());
    }
    let mut data = Vec::new();
    for s in slots {
        data.extend_from_slice(&s?);
    }
    if vxcur_fingerprint_payload(&data) != expect_fp {
        return None; // 重组产物与预期指纹不符——传输损坏，诚实拒绝
    }
    Some(data)
}

/// 载荷指纹（fnv1a64 直出——与 vxcur 指纹解耦：传输层对的是字节面）。
fn vxcur_fingerprint_payload(d: &[u8]) -> u64 {
    crate::jstar2::jbase::fnv1a64(d)
}

/// 血统链（包的旅行史：每次导出/导入追加一站——「这包从哪来」的
/// 可追溯面；环形 8，只保最近 8 站）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineageStop {
    pub device_tag: String,
    pub at_ms: u64,
    pub action: &'static str, // "export" | "import"
}

#[derive(Clone, Debug, Default)]
pub struct LineageChain {
    stops: Vec<LineageStop>,
}

impl LineageChain {
    pub fn new() -> LineageChain {
        LineageChain { stops: Vec::new() }
    }

    pub fn record(&mut self, device_tag: &str, at_ms: u64, action: &'static str) {
        if self.stops.len() >= 8 {
            self.stops.remove(0);
        }
        self.stops.push(LineageStop { device_tag: String::from(device_tag), at_ms, action });
    }

    /// 血统健康：至少一站、动作交替合法（export 后才能 import——
    /// 凭空 import 的包是伪造包）。
    pub fn healthy(&self) -> bool {
        if self.stops.is_empty() {
            return false;
        }
        self.stops.first().map(|s| s.action == "export").unwrap_or(false)
            && self.stops.windows(2).all(|w| {
                (w[0].action == "export" && w[1].action == "import")
                    || (w[0].action == "import" && w[1].action == "export")
            })
    }

    pub fn len(&self) -> usize {
        self.stops.len()
    }
}

/// 导出配额（每 24h 窗口 10 件——防爬防刷的节流面；环形计数）。
pub struct ExportQuota {
    stamps: Vec<u64>,
    pub window_ms: u64,
    pub limit: u32,
}

impl ExportQuota {
    pub fn new() -> ExportQuota {
        ExportQuota { stamps: Vec::new(), window_ms: 24 * 3_600_000, limit: 10 }
    }

    /// 申请导出（返回 false = 配额尽——诚实拒绝并提示何时恢复）。
    pub fn try_export(&mut self, at_ms: u64) -> bool {
        self.stamps.retain(|t| at_ms.saturating_sub(*t) < self.window_ms);
        if self.stamps.len() as u32 >= self.limit {
            return false;
        }
        self.stamps.push(at_ms);
        true
    }

    /// 配额恢复倒计时（ms；未满 → 0）。
    pub fn recovery_in_ms(&mut self, at_ms: u64) -> u64 {
        self.stamps.retain(|t| at_ms.saturating_sub(*t) < self.window_ms);
        if (self.stamps.len() as u32) < self.limit {
            return 0;
        }
        self.window_ms - at_ms.saturating_sub(self.stamps[0])
    }
}
impl Default for ExportQuota {
    fn default() -> Self {
        Self::new()
    }
}

/// 包 ID 碰撞对账：同指纹包重复导入的幂等语义在库房侧（Duplicate）；
/// 本函数是分享链侧的对账口径——同 ID 重导 = 幂等，不同 ID 撞名 =
/// 走库房自动改名（返回该走哪条路的决策）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionDecision {
    /// 同指纹——幂等命中，无事发生。
    Idempotent,
    /// 同名不同物——走库房 rename 通道。
    RenameNeeded,
}

pub fn decide_collision(existing_fp: u64, incoming_fp: u64, existing_name: &str, incoming_name: &str) -> CollisionDecision {
    if existing_fp == incoming_fp {
        CollisionDecision::Idempotent
    } else if existing_name == incoming_name {
        CollisionDecision::RenameNeeded
    } else {
        CollisionDecision::Idempotent
    }
}

/// 导入前重命名通道（分享链内置的合法改名点——改的是包名不是包体，
/// 指纹以内容面计不变；返回改名后的方案）。
pub fn rename_incoming(m: &CursorSchemeModel, new_name: &str) -> Result<CursorSchemeModel, &'static str> {
    if new_name.trim().is_empty() {
        return Err("包名不能为空");
    }
    let mut out = m.clone();
    out.name = String::from(new_name);
    Ok(out)
}

/// v3 自检。
pub fn run_sharing_v3_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F630-v3");
    let base = builtin_default_scheme();

    // 1. 许可面：三种许可键与再分发语义。
    set.add(
        "license keys and reshare rules",
        License::Cc0.allows_reshare()
            && License::CcBy.allows_reshare()
            && !License::AllRightsReserved.allows_reshare()
            && License::from_key("CC-BY") == Some(License::CcBy)
            && License::from_key("WTF") .is_none(),
        "",
    );

    // 2. 清单写读往返；无清单包 → None（旧包兼容路径）。
    let pkg = export_scheme(&base).unwrap();
    let with_manifest_bytes = {
        let mut b = Vec::new();
        b.extend_from_slice(manifest_line("VARIX", License::CcBy).as_bytes());
        b.extend_from_slice(&pkg.bytes);
        b
    };
    let manifest = manifest_of(&with_manifest_bytes);
    set.add(
        "manifest roundtrip and absent honest",
        manifest.as_ref().map(|(l, a)| *l == License::CcBy && a == "VARIX").unwrap_or(false)
            && manifest_of(&pkg.bytes).is_none(),
        "",
    );

    // 3. 分块传输：切块→重组逐字节复原；丢块 → 诚实 None。
    let chunks = chunkify(&pkg.bytes);
    let ok = reassemble(&chunks, vxcur_fingerprint_payload(&pkg.bytes));
    let mut short = chunks.clone();
    short.pop();
    set.add(
        "chunked transfer reassembles and detects loss",
        ok.map(|d| d == pkg.bytes).unwrap_or(false)
            && reassemble(&short, vxcur_fingerprint_payload(&pkg.bytes)).is_none(),
        "",
    );

    // 4. 乱序重组照收（传输层乱序是常态不是错误）。
    let mut shuffled = chunks.clone();
    shuffled.reverse();
    let ok2 = reassemble(&shuffled, vxcur_fingerprint_payload(&pkg.bytes));
    set.add("out of order reassembly ok", ok2.map(|d| d == pkg.bytes).unwrap_or(false), "");

    // 5. 血统链：export→import 交替合法；凭空 import / 连续 export 判 unhealthy。
    let mut chain = LineageChain::new();
    set.add("empty lineage unhealthy", !chain.healthy(), "");
    chain.record("y7000", 100, "export");
    chain.record("pad", 200, "import");
    set.add("alternating lineage healthy", chain.healthy() && chain.len() == 2, "");
    chain.record("pad", 300, "import");
    set.add("double import breaks lineage", !chain.healthy(), "");

    // 6. 血统环形 8（最旧站被挤掉但健康性仍可判）。
    let mut long_chain = LineageChain::new();
    for i in 0..12u64 {
        let action = if i % 2 == 0 { "export" } else { "import" };
        long_chain.record("dev", i * 10, action);
    }
    set.add(
        "lineage ring capped at 8",
        long_chain.len() == 8 && long_chain.healthy(),
        "",
    );

    // 7. 导出配额：10 件内全过、第 11 件拒、倒计时非零、窗口滑出恢复。
    let mut q = ExportQuota::new();
    let mut all_ok = true;
    for i in 0..10u64 {
        all_ok &= q.try_export(i * 1_000);
    }
    let denied = !q.try_export(10_000);
    let wait = q.recovery_in_ms(20_000);
    let recovered = q.try_export(24 * 3_600_000 + 1_000);
    set.add(
        "export quota throttles honestly",
        all_ok && denied && wait > 0 && recovered,
        "",
    );

    // 8. 碰撞决策：同指纹幂等；同名不同物走改名（改帧延时造「不同物」）。
    let pkg2 = export_scheme(&base).unwrap();
    let mut other = crate::jstar2::jbase::builtin_default_scheme();
    if let Some(sf) = other.state_mut(crate::jstar2::jbase::PointerState::Normal) {
        sf.frames[0].delay_ms = sf.frames[0].delay_ms + 7;
    }
    let pkg3 = export_scheme(&other).unwrap();
    set.add(
        "collision decision routes correctly",
        decide_collision(pkg.fingerprint, pkg2.fingerprint, "A", "A") == CollisionDecision::Idempotent
            && decide_collision(pkg.fingerprint, pkg3.fingerprint, "同名", "同名") == CollisionDecision::RenameNeeded,
        "",
    );

    // 9. 导入前重命名：空名拒绝、合法名改成功且内容指纹不变（内容面对账）。
    let renamed = rename_incoming(&base, "我的新名字").unwrap();
    let content_same = crate::jstar2::jbase::content_fingerprint(&renamed) == crate::jstar2::jbase::content_fingerprint(&base);
    set.add(
        "incoming rename keeps content",
        renamed.name == "我的新名字" && content_same && rename_incoming(&base, "  ").is_err(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;
    use crate::jstar2::jbase::builtin_default_scheme;

    #[test]
    fn chunkify_single_small_package() {
        let d = alloc::vec![1u8, 2, 3];
        let c = chunkify(&d);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0][0], 0); // 序号 u16LE 低字节
        assert_eq!(c[0][1], 0); // 序号 u16LE 高字节
        assert_eq!(c[0][2], 1); // 总数 u16LE 低字节
        assert_eq!(c[0][3], 0); // 总数 u16LE 高字节
    }

    #[test]
    fn reassemble_tampered_payload_rejected() {
        let base = builtin_default_scheme();
        let pkg = export_scheme(&base).unwrap();
        let mut chunks = chunkify(&pkg.bytes);
        chunks[0][4] ^= 0x01;
        assert!(reassemble(&chunks, vxcur_fingerprint_payload(&pkg.bytes)).is_none());
    }

    #[test]
    fn quota_recovery_zero_when_below_limit() {
        let mut q = ExportQuota::new();
        assert!(q.try_export(0));
        assert_eq!(q.recovery_in_ms(1_000), 0);
    }

    #[test]
    fn lineage_first_stop_must_be_export() {
        let mut c = LineageChain::new();
        c.record("pad", 1, "import");
        assert!(!c.healthy(), "凭空出现的 import = 伪造包");
    }
}

// ---------------------------------------------------------------------------
// v3·二：包体量级分档 · 密钥签名 MAC · 预览像素卡
// ---------------------------------------------------------------------------

/// 包体量级分档（人话：预览页「这个包多大」的三档口径）。
pub enum SizeClass {
    /// ≤ 8KB——聊天窗口直发无压力。
    Tiny,
    /// ≤ 256KB——论坛附件正常档。
    Medium,
    /// 更大——建议走轻量件导出。
    Heavy,
}

impl SizeClass {
    pub fn zh(self) -> &'static str {
        match self {
            SizeClass::Tiny => "轻量（≤8KB）",
            SizeClass::Medium => "标准（≤256KB）",
            SizeClass::Heavy => "重量级——建议改用轻量件导出",
        }
    }

    pub fn classify(bytes_len: usize) -> SizeClass {
        if bytes_len <= 8 * 1024 {
            SizeClass::Tiny
        } else if bytes_len <= 256 * 1024 {
            SizeClass::Medium
        } else {
            SizeClass::Heavy
        }
    }
}

/// 密钥签名（键控 MAC 模型：fnv1a64(key || data)——导出者持 key 签、
/// 导入者持同 key 验；实机接 A 域签名链后由平台密钥面替换本模型）。
pub fn sign_keyed(data: &[u8], key: &[u8]) -> u64 {
    let mut feed = Vec::with_capacity(key.len() + data.len());
    feed.extend_from_slice(key);
    feed.extend_from_slice(data);
    crate::jstar2::jbase::fnv1a64(&feed)
}

pub fn verify_keyed(data: &[u8], key: &[u8], want: u64) -> bool {
    sign_keyed(data, key) == want
}

/// 预览像素卡：Normal 态首帧盒式降采样到 8×8（分享卡片封面——
/// 真图预览，不是名字占位；缺 Normal 态 → None 诚实）。
pub fn preview_card(bytes: &[u8]) -> Option<crate::jstar2::jbase::PixBuf> {
    let m = parse_vxcur(bytes).ok()?;
    let f = m.state(crate::jstar2::jbase::PointerState::Normal)?.frames.first()?.buf();
    Some(box_downsample(&f, 8))
}

/// 盒式降采样（整数倍收缩；目标小于源时逐盒取均值——透明像素按
/// alpha 加权，不让透明区把颜色冲淡）。
pub fn box_downsample(src: &crate::jstar2::jbase::PixBuf, target: u16) -> crate::jstar2::jbase::PixBuf {
    let t = target.max(1);
    let mut out = crate::jstar2::jbase::PixBuf::new(t, t);
    let (sw, sh) = (src.w as u64, src.h as u64);
    for ty in 0..t {
        for tx in 0..t {
            let x0 = tx as u64 * sw / t as u64;
            let x1 = ((tx as u64 + 1) * sw / t as u64).max(x0 + 1);
            let y0 = ty as u64 * sh / t as u64;
            let y1 = ((ty as u64 + 1) * sh / t as u64).max(y0 + 1);
            let (mut r, mut g, mut b, mut a, mut wsum, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1.min(sh) {
                for x in x0..x1.min(sw) {
                    if let Some(px) = src.get(x as u16, y as u16) {
                        let w = px[3] as u64 + 1; // +1：全透明盒不除零
                        r += px[0] as u64 * w;
                        g += px[1] as u64 * w;
                        b += px[2] as u64 * w;
                        a += px[3] as u64;
                        wsum += w;
                        n += 1;
                    }
                }
            }
            if n > 0 {
                // 颜色按 alpha 加权均值；alpha 按像素数直均（覆盖率语义
                // —— alpha 均值混用加权分母会把 255/256 整除成 0，全图
                // 变全透明的已修缺陷）。
                out.set(
                    tx,
                    ty,
                    [(r / wsum) as u8, (g / wsum) as u8, (b / wsum) as u8, (a / n) as u8],
                );
            }
        }
    }
    out
}

/// v3·二 自检。
pub fn run_sharing_v3b_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F630-v3b");
    let base = builtin_default_scheme();
    let pkg = export_scheme(&base).unwrap();

    // 1. 量级分档：内置方案属轻量/标准档；人为大包归 Heavy。
    let cls = SizeClass::classify(pkg.bytes.len());
    let heavy = SizeClass::classify(300 * 1024);
    set.add(
        "size classes classify honestly",
        !matches!(cls, SizeClass::Heavy) && matches!(heavy, SizeClass::Heavy),
        "",
    );

    // 2. 密钥签名：对签对验绿；换 key 验签红；改字节验签红。
    let sig = sign_keyed(&pkg.bytes, b"share-key-2026");
    set.add(
        "keyed sign verify roundtrip",
        verify_keyed(&pkg.bytes, b"share-key-2026", sig)
            && !verify_keyed(&pkg.bytes, b"wrong-key", sig),
        "",
    );
    let mut tampered = pkg.bytes.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0x01;
    set.add("keyed sign catches tamper", !verify_keyed(&tampered, b"share-key-2026", sig), "");

    // 3. 预览像素卡：8×8 真图（非全透明——内置方案 Normal 态有内容）。
    let card = preview_card(&pkg.bytes);
    set.add(
        "preview card is real thumbnail",
        card.as_ref().map(|c| c.w == 8 && c.h == 8 && c.solid_count() > 0).unwrap_or(false),
        "",
    );

    // 4. 盒式降采样：纯色图收缩后仍是该纯色（均值语义对拍）。
    let mut solid = crate::jstar2::jbase::PixBuf::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            solid.set(x, y, [10, 20, 30, 255]);
        }
    }
    let ds = box_downsample(&solid, 4);
    set.add(
        "box downsample preserves solid color",
        ds.w == 4 && (0..4).all(|y| (0..4).all(|x| ds.get(x, y) == Some([10, 20, 30, 255]))),
        "",
    );

    // 5. 缺 Normal 态的包：预览卡诚实 None。
    let mut broken = base.clone();
    broken.entries.retain(|e| e.state != crate::jstar2::jbase::PointerState::Normal);
    let pkg2 = export_scheme(&broken).unwrap();
    set.add("missing normal state honest none", preview_card(&pkg2.bytes).is_none(), "");

    set
}

#[cfg(test)]
mod tests_v3b {
    use super::*;

    #[test]
    fn sign_is_key_dependent() {
        let a = sign_keyed(b"data", b"k1");
        let b = sign_keyed(b"data", b"k2");
        assert_ne!(a, b);
    }

    #[test]
    fn tiny_size_boundary() {
        assert!(matches!(SizeClass::classify(8 * 1024), SizeClass::Tiny));
        assert!(matches!(SizeClass::classify(8 * 1024 + 1), SizeClass::Medium));
    }
}
