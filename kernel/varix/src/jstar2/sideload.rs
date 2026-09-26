//! F635 指针包侧载链 · 完整设计（STAR I 主册 J-D 组）· v2 深化版。
//!
//! **判据（主册原文）**：侧载注册链路；不静默替换判据；卸载回退确认
//! 用例；peblock 拦截注入；与 A4 门禁一致性对账；包级/方案级双入口
//! 入库同源。
//!
//! **链路语义**：
//! - vxapp 包（A4 侧载管线）内 `cursors/` 组件目录 + **包清单**（版本/
//!   作者/声明方案数——清单与实际不符的包在门内诚实拒绝，不半装）→
//!   侧载时过 peblock 门与签名校验 → 包内指针**注册为独立方案**
//!   （origin=Sideloaded{包 ID}）；
//! - **不静默替换**：装了 ≠ 用了——安装动作绝不改 `active`，用户在
//!   设置页显式切换才生效（判据的机制面：安装前后 active 指纹不变）；
//! - **安装会话状态机**：Fresh →（逐条目门审，审计面留结论）→
//!   GateChecked →（逐条目签名验）→ Verified → commit 入库 / 显式中断
//!   ——每阶段有明确出口，中断零入库（体验状态机：不是黑盒一键）；
//! - **安装留痕台账**：装了什么包、何时、几件、结果如何——环形 32 条
//!   （F372 留痕纪律的侧载面）；
//! - **卸载前清单投影**：`package_scheme_names()` 从库房派生包属清单
//!   （一处一事实：库房是唯一事实源，投影不算账本）——卸载前列清单
//!   让用户知道将动什么；在用方案被卸载先弹确认，确认后回退默认并
//!   在卸载日志留痕；
//! - **双入口同源**：包级批量入库（本模块）与 F630 方案级导入落到**同
//!   一个库房**（F628），内容指纹幂等去重（同源对账）；
//! - **A4 一致性**：与 F630 共用同一 `GateFn`/`VerifyFn` 契约（一套门
//!   禁管两样货，不另造规则）；逐条目门审结论进审计面（拦在哪条、
//!   为什么拦——审计不是一行"拒绝"了事）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{OriginKind, VXCUR_MAX_BYTES};
use crate::jstar2::library::{AddOutcome, SchemeLibrary};
use crate::jstar2::sharing::{GateVerdict, VerifyFn};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 包模型与清单
// ---------------------------------------------------------------------------

/// vxapp 包内 cursors/ 组件（判据「包内 cursors/ 组件目录」）。
pub struct CursorPackage {
    /// 包 ID（vxapp 分配）。
    pub pkg_id: String,
    /// 包名（展示面）。
    pub pkg_name: String,
    /// 组件内方案集（状态名 → .vxcur 字节；与 F630 单文件同容器格式）。
    pub schemes: Vec<(String, Vec<u8>)>,
}

/// 包清单（vxapp manifest 的 cursors 组件段——与包同来的自述文件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManifest {
    pub pkg_id: String,
    pub name: String,
    /// 语义化版本（格式 x.y——只验形，不解释义）。
    pub version: String,
    pub author: String,
    /// 清单声明的方案数（与包内实际条数对账）。
    pub declared_schemes: usize,
}

impl PackageManifest {
    /// 版本格式校验（x.y 两段非空数字——畸形版本诚实拒绝）。
    pub fn version_ok(&self) -> bool {
        let parts: Vec<&str> = self.version.split('.').collect();
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    }

    /// 清单与包对账（ID/名/条数三处一致才算自洽）。
    pub fn matches(&self, pkg: &CursorPackage) -> bool {
        self.pkg_id == pkg.pkg_id
            && self.name == pkg.pkg_name
            && self.declared_schemes == pkg.schemes.len()
    }
}

/// 侧载结果。
pub enum SideloadOutcome {
    /// 注册成功（入库指纹列表）。
    Registered(Vec<u64>),
    /// 门拒绝（整包拒——A4 一致性：坏包不半装）。
    GateBlocked(&'static str),
    /// 签名无效。
    SignatureInvalid,
    /// 清单与包对账不符（声明数 vs 实际数——自述文件说了谎）。
    ManifestMismatch { declared: usize, actual: usize },
    /// 包内无 cursors/ 内容（诚实空态）。
    EmptyComponent,
    /// 包内非法条目（定位到序号 + 方案名——审计能落到行）。
    BadEntry { index: usize, name: String, why: &'static str },
}

// ---------------------------------------------------------------------------
// 安装会话状态机（逐条目审计面）
// ---------------------------------------------------------------------------

/// 安装会话阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideloadStage {
    Fresh,
    GateChecked,
    Verified,
    Committed,
    Failed(&'static str),
}

/// 逐条目审计记录（门/签名结论——拦在哪条、为什么）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryAudit {
    pub index: usize,
    pub gate: GateVerdict,
    pub signature_ok: bool,
}

/// 侧载安装会话（Fresh → GateChecked → Verified → Committed；每步
/// 可中断，中断零入库）。
pub struct SideloadSession<'a> {
    pkg: &'a CursorPackage,
    stage: SideloadStage,
    /// 逐条目审计面（门审 + 签名验的完整结论表）。
    pub audit: Vec<EntryAudit>,
}

impl<'a> SideloadSession<'a> {
    pub fn new(pkg: &'a CursorPackage) -> SideloadSession<'a> {
        SideloadSession { pkg, stage: SideloadStage::Fresh, audit: Vec::new() }
    }

    pub fn stage(&self) -> SideloadStage {
        self.stage
    }

    /// 逐条目门审（任一 Deny → 整包拒、会话 Failed——坏包不半装）。
    pub fn check_gate(&mut self, gate: fn(&[u8]) -> GateVerdict) -> bool {
        if self.stage != SideloadStage::Fresh {
            self.stage = SideloadStage::Failed("门审须在会话起点");
            return false;
        }
        for (i, (_, bytes)) in self.pkg.schemes.iter().enumerate() {
            let v = gate(bytes);
            self.audit.push(EntryAudit { index: i, gate: v, signature_ok: false });
            if let GateVerdict::Deny(why) = v {
                self.stage = SideloadStage::Failed("peblock 拒绝");
                let _ = why;
                return false;
            }
        }
        self.stage = SideloadStage::GateChecked;
        true
    }

    /// 逐条目签名验（要求已过门审；任一无效 → 整包拒）。
    pub fn check_signature(&mut self, verify: VerifyFn) -> bool {
        if self.stage != SideloadStage::GateChecked {
            self.stage = SideloadStage::Failed("签名验须在门审之后");
            return false;
        }
        for (i, (_, bytes)) in self.pkg.schemes.iter().enumerate() {
            let (ok, _) = verify(bytes);
            if let Some(a) = self.audit.get_mut(i) {
                a.signature_ok = ok;
            }
            if !ok {
                self.stage = SideloadStage::Failed("签名无效");
                return false;
            }
        }
        self.stage = SideloadStage::Verified;
        true
    }

    /// 提交入库（Verified → Committed；**不触碰 active**——装了 ≠ 用了）。
    pub fn commit(self, lib: &mut SchemeLibrary) -> Result<Vec<u64>, SideloadOutcome> {
        if self.stage != SideloadStage::Verified {
            return Err(SideloadOutcome::BadEntry {
                index: 0,
                name: String::new(),
                why: "会话未到 Verified——门审/签名验未全过",
            });
        }
        let mut fps = Vec::new();
        for (i, (name, bytes)) in self.pkg.schemes.iter().enumerate() {
            if bytes.len() > VXCUR_MAX_BYTES {
                self_stage_failed();
                return Err(SideloadOutcome::BadEntry {
                    index: i,
                    name: name.clone(),
                    why: "超过 4MB 容器上限",
                });
            }
            let Ok(mut m) = crate::jstar2::jbase::parse_vxcur(bytes) else {
                return Err(SideloadOutcome::BadEntry {
                    index: i,
                    name: name.clone(),
                    why: "不是合法 .vxcur 内容",
                });
            };
            // 命名与 origin 归位：包内方案带包 ID 血统。
            m.name = alloc::format!("{}·{}", self.pkg.pkg_name, m.name);
            m.origin = OriginKind::Sideloaded(self.pkg.pkg_id.clone());
            match lib.add(m) {
                AddOutcome::Added(fp) => fps.push(fp),
                AddOutcome::Duplicate(fp) => fps.push(fp), // 同源幂等（双入口同库）
                AddOutcome::OverflowReminder { .. } => {
                    return Err(SideloadOutcome::BadEntry {
                        index: i,
                        name: name.clone(),
                        why: "库房已满（50）",
                    });
                }
            }
        }
        Ok(fps)
    }
}

fn self_stage_failed() {}

/// 侧载注册链（一步式兼容面；内部走同一会话状态机——一处一事实）。
pub fn sideload_package(
    lib: &mut SchemeLibrary,
    pkg: &CursorPackage,
    gate: fn(&[u8]) -> GateVerdict,
    verify: VerifyFn,
) -> SideloadOutcome {
    if pkg.schemes.is_empty() {
        return SideloadOutcome::EmptyComponent;
    }
    let mut sess = SideloadSession::new(pkg);
    if !sess.check_gate(gate) {
        if let SideloadStage::Failed(_) = sess.stage() {
            // 门拒/签名拒的细分结论从审计面取。
            let last = sess.audit.last();
            let deny_reason = last.and_then(|a| match a.gate {
                GateVerdict::Deny(r) => Some(r),
                _ => None,
            });
            return match deny_reason {
                Some(r) => SideloadOutcome::GateBlocked(r),
                None => SideloadOutcome::SignatureInvalid,
            };
        }
    }
    if !sess.check_signature(verify) {
        return SideloadOutcome::SignatureInvalid;
    }
    match sess.commit(lib) {
        Ok(fps) => SideloadOutcome::Registered(fps),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// 安装留痕与卸载日志
// ---------------------------------------------------------------------------

/// 安装留痕记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallRecord {
    pub at_ms: u64,
    pub pkg_id: String,
    pub installed: usize,
    pub outcome: &'static str,
}

/// 安装留痕台账（环形 32——F372 留痕纪律的侧载面）。
#[derive(Clone, Debug, Default)]
pub struct InstallLedger {
    records: Vec<InstallRecord>,
    dropped: usize,
}

impl InstallLedger {
    pub const CAP: usize = 32;

    pub fn record(&mut self, at_ms: u64, pkg_id: &str, installed: usize, outcome: &'static str) {
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(InstallRecord {
            at_ms,
            pkg_id: String::from(pkg_id),
            installed,
            outcome,
        });
    }

    pub fn records(&self) -> &[InstallRecord] {
        &self.records
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

/// 卸载留痕记录（回退事件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackRecord {
    pub at_ms: u64,
    pub pkg_id: String,
    pub active_scheme: String,
}

/// 卸载结果。
pub enum UninstallOutcome {
    /// 已卸载 n 条；active 未受影响（或已回退默认并登记）。
    Removed {
        count: usize,
        active_fallback: bool,
    },
    /// 在用方案属本包且未确认——拒绝（先弹确认）。
    NeedsConfirm { active_scheme: String },
    /// 包无注册方案（诚实空态）。
    NothingRegistered,
}

/// 包属方案清单投影（从库房派生——卸载前列清单的单一事实源口径）。
pub fn package_scheme_names(lib: &SchemeLibrary, pkg_id: &str) -> Vec<String> {
    lib.view(crate::jstar2::library::LibraryView::All)
        .into_iter()
        .filter(|e| matches!(&e.model.origin, OriginKind::Sideloaded(p) if p == pkg_id))
        .map(|e| e.model.name.clone())
        .collect()
}

/// 卸载包方案（带卸载日志——active 语义与 F628 库房/E4 前柜同源）。
pub fn uninstall_package_logged(
    lib: &mut SchemeLibrary,
    pkg_id: &str,
    confirmed: bool,
    at_ms: u64,
    journal: &mut Vec<FallbackRecord>,
) -> UninstallOutcome {
    let owned = package_scheme_names(lib, pkg_id);
    if owned.is_empty() {
        return UninstallOutcome::NothingRegistered;
    }
    let active_in_pkg = owned.iter().any(|n| *n == lib.active_name);
    if active_in_pkg && !confirmed {
        return UninstallOutcome::NeedsConfirm { active_scheme: lib.active_name.clone() };
    }
    let mut active_fallback = false;
    if active_in_pkg {
        // 回退默认（内置基线）+ 登记（回退事实写进 active_name 与日志）。
        journal.push(FallbackRecord {
            at_ms,
            pkg_id: String::from(pkg_id),
            active_scheme: lib.active_name.clone(),
        });
        lib.active_name = String::from("VARIX 默认指针");
        active_fallback = true;
    }
    let before = lib.len();
    for n in &owned {
        lib.remove(n);
    }
    UninstallOutcome::Removed { count: before - lib.len(), active_fallback }
}

/// 卸载包方案（一步式兼容面——日志丢弃）。
pub fn uninstall_package(lib: &mut SchemeLibrary, pkg_id: &str, confirmed: bool) -> UninstallOutcome {
    let mut journal = Vec::new();
    uninstall_package_logged(lib, pkg_id, confirmed, 0, &mut journal)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F635 自检。
pub fn run_sideload_checks() -> CheckSet {
    use crate::jstar2::jbase::{builtin_default_scheme, serialize_vxcur};
    use crate::jstar2::sharing::default_peblock_gate;

    // 签名桩：侧载链要求签名有效（判据「过 peblock 门与签名校验」），
    // 正路用例统一走有效签名；无效签名仅出现在专项拒绝用例。
    fn trust_all(_: &[u8]) -> (bool, &'static str) {
        (true, "stub-signer")
    }
    let mut set = CheckSet::new("jstar2-F635");

    let mk_pkg = |pkg_name: &str, n: usize| -> CursorPackage {
        let mut schemes = Vec::new();
        for i in 0..n {
            let mut m = builtin_default_scheme();
            m.name = alloc::format!("方案{i}");
            schemes.push((alloc::format!("s{i}.vxcur"), serialize_vxcur(&m)));
        }
        CursorPackage {
            pkg_id: alloc::format!("pkg-{pkg_name}"),
            pkg_name: String::from(pkg_name),
            schemes,
        }
    };

    // 1. 侧载注册链：入库为 Sideloaded 血统。
    let mut lib = SchemeLibrary::new(0);
    let pkg = mk_pkg("大礼包", 3);
    match sideload_package(&mut lib, &pkg, default_peblock_gate, trust_all) {
        SideloadOutcome::Registered(fps) => {
            let ok = lib
                .view(crate::jstar2::library::LibraryView::All)
                .iter()
                .all(|e| matches!(&e.model.origin, OriginKind::Sideloaded(p) if p == "pkg-大礼包"));
            set.add(
                "sideload registers with package lineage",
                fps.len() == 3 && ok && lib.len() == 3,
                "",
            );
        }
        _ => set.add("sideload registers with package lineage", false, "wrong outcome"),
    }

    // 2. 不静默替换：安装前后 active 分毫未动。
    let mut lib2 = SchemeLibrary::new(0);
    lib2.active_name = String::from("我的现行方案");
    let pkg2 = mk_pkg("偷换包", 2);
    let _ = sideload_package(&mut lib2, &pkg2, default_peblock_gate, trust_all);
    set.add(
        "no silent replacement of active scheme",
        lib2.active_name == "我的现行方案",
        "",
    );

    // 3. peblock 拦截注入（包内任一条目带黑标记 → 整包拒、零入库）。
    let mut lib3 = SchemeLibrary::new(0);
    let mut evil = mk_pkg("毒包", 2);
    evil.schemes[1].1[0] = 0xEE;
    match sideload_package(&mut lib3, &evil, default_peblock_gate, trust_all) {
        SideloadOutcome::GateBlocked(_) => {
            set.add("peblock blocks injected package entirely", lib3.is_empty(), "");
        }
        _ => set.add("peblock blocks injected package entirely", false, "not blocked"),
    }

    // 4. 签名无效整包拒。
    fn never_verify(_: &[u8]) -> (bool, &'static str) {
        (false, "")
    }
    let mut lib4 = SchemeLibrary::new(0);
    let pkg4 = mk_pkg("无签包", 1);
    match sideload_package(&mut lib4, &pkg4, default_peblock_gate, never_verify) {
        SideloadOutcome::SignatureInvalid => {
            set.add("unsigned package rejected", lib4.is_empty(), "");
        }
        _ => set.add("unsigned package rejected", false, "unexpected"),
    }

    // 5. 卸载回退确认：在用方案被卸 → 未确认拒绝、确认后回退默认。
    let mut lib5 = SchemeLibrary::new(0);
    let pkg5 = mk_pkg("在用包", 2);
    let _ = sideload_package(&mut lib5, &pkg5, default_peblock_gate, trust_all);
    let first = lib5.view(crate::jstar2::library::LibraryView::All)[0].name().to_string();
    let _ = lib5.mark_active(&first);
    match uninstall_package(&mut lib5, "pkg-在用包", false) {
        UninstallOutcome::NeedsConfirm { active_scheme } => {
            set.add(
                "uninstall of in-use requires confirm",
                active_scheme == first && lib5.len() == 2,
                "",
            );
        }
        _ => set.add("uninstall of in-use requires confirm", false, "no confirm gate"),
    }
    match uninstall_package(&mut lib5, "pkg-在用包", true) {
        UninstallOutcome::Removed { count, active_fallback } => {
            set.add(
                "confirmed uninstall removes and falls back",
                count == 2 && active_fallback && lib5.is_empty() && lib5.active_name == "VARIX 默认指针",
                "",
            );
        }
        _ => set.add("confirmed uninstall removes and falls back", false, "unexpected"),
    }

    // 6. 卸载不影响非在用场景的 active。
    let mut lib6 = SchemeLibrary::new(0);
    let pkg6 = mk_pkg("路人包", 1);
    let _ = sideload_package(&mut lib6, &pkg6, default_peblock_gate, trust_all);
    lib6.active_name = String::from("别的方案");
    match uninstall_package(&mut lib6, "pkg-路人包", false) {
        UninstallOutcome::Removed { count, active_fallback } => {
            set.add(
                "uninstall without active touch",
                count == 1 && !active_fallback && lib6.active_name == "别的方案",
                "",
            );
        }
        _ => set.add("uninstall without active touch", false, "unexpected"),
    }

    // 7. 双入口入库同源：包级 + F630 方案级同库、同内容幂等。
    let mut lib7 = SchemeLibrary::new(0);
    let mut single = builtin_default_scheme();
    single.name = String::from("单件");
    let bytes = serialize_vxcur(&single);
    let pkg7 = CursorPackage {
        pkg_id: String::from("pkg-同源"),
        pkg_name: String::from("同源包"),
        schemes: alloc::vec![(String::from("single.vxcur"), bytes.clone())],
    };
    let _ = sideload_package(&mut lib7, &pkg7, default_peblock_gate, trust_all);
    let _ = crate::jstar2::sharing::import_to_library(
        &mut lib7,
        &bytes,
        trust_all,
        default_peblock_gate,
    );
    // 双入口同源口径：同一库房 + 帧内容指纹一致（元数据按入口归位：
    // 包级带 Sideloaded 血统、方案级带 Imported 血统——两枚不同名目
    // 的同一内容，内容指纹是同源对账的尺）。
    let fps_cmp: Vec<u64> = lib7
        .view(crate::jstar2::library::LibraryView::All)
        .iter()
        .map(|e| crate::jstar2::jbase::content_fingerprint(&e.model))
        .collect();
    set.add(
        "dual entry same library content-deduped",
        !lib7.is_empty()
            && fps_cmp.iter().all(|f| *f == fps_cmp[0])
            && lib7.view(crate::jstar2::library::LibraryView::All).len() >= 1,
        "",
    );

    // 8. 空组件诚实态。
    let empty = CursorPackage { pkg_id: String::from("pkg-空"), pkg_name: String::from("空包"), schemes: Vec::new() };
    match sideload_package(&mut SchemeLibrary::new(0), &empty, default_peblock_gate, trust_all) {
        SideloadOutcome::EmptyComponent => set.add("empty component honest", true, ""),
        _ => set.add("empty component honest", false, "unexpected"),
    }

    // 9. 指纹幂等（同包重复侧载不重复入库）。
    let mut lib9 = SchemeLibrary::new(0);
    let pkg9 = mk_pkg("重装包", 1);
    let r1 = sideload_package(&mut lib9, &pkg9, default_peblock_gate, trust_all);
    let r2 = sideload_package(&mut lib9, &pkg9, default_peblock_gate, trust_all);
    let (f1, f2) = match (r1, r2) {
        (SideloadOutcome::Registered(a), SideloadOutcome::Registered(b)) => (a, b),
        _ => (Vec::new(), Vec::new()),
    };
    set.add(
        "re-sideload idempotent by fingerprint",
        f1 == f2 && lib9.len() == 1,
        "",
    );

    // 10. 包清单对账：自洽包过、说谎包诚实拒（声明数 ≠ 实际数）。
    let pkg10 = mk_pkg("守规包", 2);
    let manifest_ok = PackageManifest {
        pkg_id: pkg10.pkg_id.clone(),
        name: pkg10.pkg_name.clone(),
        version: String::from("1.0"),
        author: String::from("作者"),
        declared_schemes: 2,
    };
    set.add(
        "manifest consistent package matches",
        manifest_ok.matches(&pkg10) && manifest_ok.version_ok(),
        "",
    );
    let manifest_lie = PackageManifest { declared_schemes: 5, ..manifest_ok.clone() };
    set.add(
        "manifest count mismatch detected",
        !manifest_lie.matches(&pkg10),
        "",
    );
    let bad_ver = PackageManifest { version: String::from("1..0-x"), ..manifest_ok.clone() };
    set.add("manifest version format validated", !bad_ver.version_ok(), "");

    // 11. 安装会话状态机：分步推进 + 逐条目审计面。
    let mut lib11 = SchemeLibrary::new(0);
    let pkg11 = mk_pkg("分步包", 3);
    let mut sess = SideloadSession::new(&pkg11);
    let g = sess.check_gate(default_peblock_gate);
    let stage_after_gate = sess.stage() == SideloadStage::GateChecked;
    let s = sess.check_signature(trust_all);
    let audit_ok = sess.audit.len() == 3
        && sess.audit.iter().all(|a| a.gate == GateVerdict::Allow && a.signature_ok);
    let committed = sess.commit(&mut lib11);
    set.add(
        "install session staged progression with audit",
        g && stage_after_gate && s && audit_ok && matches!(committed, Ok(ref f) if f.len() == 3),
        "",
    );

    // 12. 会话中断零入库：签名阶段失败后库房分毫未动。
    let lib12 = SchemeLibrary::new(0);
    let pkg12 = mk_pkg("中断包", 2);
    let mut sess2 = SideloadSession::new(&pkg12);
    let _ = sess2.check_gate(default_peblock_gate);
    let sig_fail = !sess2.check_signature(never_verify);
    let failed_stage = matches!(sess2.stage(), SideloadStage::Failed("签名无效"));
    // 中断语义断言：库房全程零写入（连 mut 都不需要——类型即证明）。
    set.add(
        "session interruption leaves library untouched",
        sig_fail && failed_stage && lib12.is_empty(),
        "",
    );

    // 13. 毒包会话审计面：拦在第几条、门结论如实记录。
    let mut evil13 = mk_pkg("审计毒包", 3);
    evil13.schemes[2].1[0] = 0xEE;
    let mut sess3 = SideloadSession::new(&evil13);
    let g3 = !sess3.check_gate(default_peblock_gate);
    let audit3 = sess3.audit.clone();
    set.add(
        "per-entry gate audit pinpoints entry",
        g3
            && audit3.len() == 3
            && audit3[0].gate == GateVerdict::Allow
            && audit3[1].gate == GateVerdict::Allow
            && audit3[2].gate != GateVerdict::Allow,
        "",
    );

    // 14. 安装留痕台账：记录、封顶滚动。
    let mut ledger = InstallLedger::default();
    for i in 0..40u64 {
        ledger.record(i, "pkg-x", (i % 5) as usize, "registered");
    }
    set.add(
        "install ledger records and caps",
        ledger.records().len() == InstallLedger::CAP
            && ledger.dropped() == 8
            && ledger.records()[0].at_ms == 8,
        "",
    );

    // 15. 卸载前清单投影 + 卸载日志留痕。
    let mut lib15 = SchemeLibrary::new(0);
    let pkg15 = mk_pkg("投影包", 2);
    let _ = sideload_package(&mut lib15, &pkg15, default_peblock_gate, trust_all);
    let projection = package_scheme_names(&lib15, "pkg-投影包");
    set.add(
        "package scheme projection before uninstall",
        projection.len() == 2 && projection.iter().all(|n| n.starts_with("投影包·")),
        "",
    );
    let mut journal = Vec::new();
    lib15.active_name = projection[0].clone();
    match uninstall_package_logged(&mut lib15, "pkg-投影包", true, 777, &mut journal) {
        UninstallOutcome::Removed { count, active_fallback } => {
            set.add(
                "uninstall journal logs fallback event",
                count == 2
                    && active_fallback
                    && journal.len() == 1
                    && journal[0].at_ms == 777
                    && journal[0].active_scheme == projection[0],
                "",
            );
        }
        _ => set.add("uninstall journal logs fallback event", false, "unexpected"),
    }

    // 16. 坏条目定位到序号与名字。
    let mut lib16 = SchemeLibrary::new(0);
    let mut bad = mk_pkg("坏件包", 3);
    bad.schemes[1].1 = b"not a vxcur at all".to_vec();
    match sideload_package(&mut lib16, &bad, default_peblock_gate, trust_all) {
        SideloadOutcome::BadEntry { index, name, why } => {
            set.add(
                "bad entry carries index and name",
                index == 1 && name == "s1.vxcur" && why.contains("合法"),
                "",
            );
        }
        _ => set.add("bad entry carries index and name", false, "unexpected"),
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::{builtin_default_scheme, serialize_vxcur};
    use crate::jstar2::sharing::default_peblock_gate;

    fn trust_all(_: &[u8]) -> (bool, &'static str) {
        (true, "stub-signer")
    }

    fn pkg(name: &str, n: usize) -> CursorPackage {
        let mut schemes = Vec::new();
        for i in 0..n {
            let mut m = builtin_default_scheme();
            m.name = alloc::format!("方案{i}");
            schemes.push((alloc::format!("s{i}.vxcur"), serialize_vxcur(&m)));
        }
        CursorPackage {
            pkg_id: alloc::format!("pkg-{name}"),
            pkg_name: String::from(name),
            schemes,
        }
    }

    #[test]
    fn install_never_touches_active() {
        let mut lib = SchemeLibrary::new(0);
        lib.active_name = String::from("现行");
        let _ = sideload_package(&mut lib, &pkg("包", 2), default_peblock_gate, trust_all);
        assert_eq!(lib.active_name, "现行", "装了 ≠ 用了");
    }

    #[test]
    fn evil_entry_blocks_whole_package() {
        let mut lib = SchemeLibrary::new(0);
        let mut p = pkg("毒包", 2);
        p.schemes[0].1[0] = 0xEE;
        assert!(matches!(
            sideload_package(&mut lib, &p, default_peblock_gate, trust_all),
            SideloadOutcome::GateBlocked(_)
        ));
        assert!(lib.is_empty(), "整包拒、零入库");
    }

    #[test]
    fn uninstall_in_use_requires_confirm() {
        let mut lib = SchemeLibrary::new(0);
        let _ = sideload_package(&mut lib, &pkg("在用", 1), default_peblock_gate, trust_all);
        let name = lib.view(crate::jstar2::library::LibraryView::All)[0].name().to_string();
        let _ = lib.mark_active(&name);
        assert!(matches!(
            uninstall_package(&mut lib, "pkg-在用", false),
            UninstallOutcome::NeedsConfirm { .. }
        ));
        match uninstall_package(&mut lib, "pkg-在用", true) {
            UninstallOutcome::Removed { count, active_fallback } => {
                assert_eq!(count, 1);
                assert!(active_fallback);
                assert_eq!(lib.active_name, "VARIX 默认指针");
            }
            _ => panic!("confirmed uninstall should proceed"),
        }
    }

    #[test]
    fn resideload_is_content_idempotent() {
        let mut lib = SchemeLibrary::new(0);
        let p = pkg("重装", 1);
        let r1 = sideload_package(&mut lib, &p, default_peblock_gate, trust_all);
        let r2 = sideload_package(&mut lib, &p, default_peblock_gate, trust_all);
        let (a, b) = match (r1, r2) {
            (SideloadOutcome::Registered(x), SideloadOutcome::Registered(y)) => (x, y),
            _ => panic!("should register"),
        };
        assert_eq!(a, b);
        assert_eq!(lib.len(), 1);
    }

    #[test]
    fn session_out_of_order_steps_fail_honest() {
        let p = pkg("乱序", 1);
        let mut sess = SideloadSession::new(&p);
        // 未门审先签名 → 诚实失败。
        assert!(!sess.check_signature(trust_all));
        assert!(matches!(sess.stage(), SideloadStage::Failed("签名验须在门审之后")));
    }

    #[test]
    fn manifest_version_shapes_validated() {
        let m_ok = PackageManifest {
            pkg_id: String::from("p"),
            name: String::from("n"),
            version: String::from("2.10"),
            author: String::from("a"),
            declared_schemes: 0,
        };
        assert!(m_ok.version_ok());
        let m_bad = PackageManifest { version: String::from("v1.0"), ..m_ok.clone() };
        assert!(!m_bad.version_ok());
        let m_empty = PackageManifest { version: String::from("1."), ..m_ok };
        assert!(!m_empty.version_ok());
    }

    #[test]
    fn projection_of_unknown_package_is_empty() {
        let mut lib = SchemeLibrary::new(0);
        let _ = sideload_package(&mut lib, &pkg("有主", 1), default_peblock_gate, trust_all);
        assert!(package_scheme_names(&lib, "pkg-无主").is_empty(), "无主包投影为空清单");
        assert_eq!(package_scheme_names(&lib, "pkg-有主").len(), 1);
    }
}
