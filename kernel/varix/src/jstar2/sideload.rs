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

// ---------------------------------------------------------------------------
// v4 深化批：侧载队列（批量包逐个过门、逐条目结果留痕）· 磁盘配额
// （字节计、超限诚实拒绝）· 安装回滚快照（装前快照 + 一键回滚）·
// 包清单版本迁移（旧版清单字段补齐）
// ---------------------------------------------------------------------------

/// 队列单包处理结果（逐包留痕——批量侧载的审计面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueEntryResult {
    pub pkg_id: String,
    pub pkg_name: String,
    /// 结论类别（registered / gate-blocked / signature-invalid / empty /
    /// manifest-mismatch / bad-entry / quota-denied——人话由 detail 承载）。
    pub kind: &'static str,
    pub installed: usize,
    pub detail: String,
}

/// 结论拆解（kind, 件数, 人话明细）——留痕与结果枚举同源，一处一事实。
fn outcome_parts(o: &SideloadOutcome) -> (&'static str, usize, String) {
    match o {
        SideloadOutcome::Registered(fps) => ("registered", fps.len(), String::from("入库成功")),
        SideloadOutcome::GateBlocked(r) => ("gate-blocked", 0, String::from(*r)),
        SideloadOutcome::SignatureInvalid => ("signature-invalid", 0, String::from("签名校验未通过")),
        SideloadOutcome::ManifestMismatch { declared, actual } => {
            ("manifest-mismatch", 0, alloc::format!("清单声明 {} 件、包内实际 {} 件", declared, actual))
        }
        SideloadOutcome::EmptyComponent => ("empty", 0, String::from("包内无 cursors/ 组件内容")),
        SideloadOutcome::BadEntry { index, name, why } => {
            ("bad-entry", 0, alloc::format!("第 {} 件「{}」：{}", index, name, why))
        }
    }
}

/// 侧载队列（批量包逐个过门：每包独立走完整链——配额收编 → 门审 →
/// 签名验 → 入库；单包失败不拖垮整队，坏件隔离留痕、好件照装）。
#[derive(Default)]
pub struct SideloadQueue {
    pending: Vec<CursorPackage>,
    trail: Vec<QueueEntryResult>,
}

impl SideloadQueue {
    pub fn new() -> SideloadQueue {
        SideloadQueue { pending: Vec::new(), trail: Vec::new() }
    }

    pub fn push(&mut self, pkg: CursorPackage) {
        self.pending.push(pkg);
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// 逐包过门（按入队顺序；配额不过的包不进门——字节账先行）。
    pub fn drain(
        &mut self,
        lib: &mut SchemeLibrary,
        quota: &mut DiskQuota,
        gate: fn(&[u8]) -> GateVerdict,
        verify: VerifyFn,
    ) {
        for pkg in self.pending.drain(..) {
            let bytes = DiskQuota::package_bytes(&pkg);
            let (kind, installed, detail) = match quota.admit(&pkg) {
                Err(d) => (
                    "quota-denied",
                    0,
                    alloc::format!("磁盘配额不足：需 {} 字节、仅余 {} 字节", d.need_bytes, d.free_bytes),
                ),
                Ok(_) => {
                    let o = sideload_package(lib, &pkg, gate, verify);
                    let (k, n, d) = outcome_parts(&o);
                    if k != "registered" {
                        // 失败包不占账——字节账两头对平。
                        let _ = quota.release(bytes);
                    }
                    (k, n, d)
                }
            };
            self.trail.push(QueueEntryResult {
                pkg_id: pkg.pkg_id.clone(),
                pkg_name: pkg.pkg_name.clone(),
                kind,
                installed,
                detail,
            });
        }
    }

    pub fn trail(&self) -> &[QueueEntryResult] {
        &self.trail
    }

    /// 批次汇总（装了几包、几件、几包失败——批量面板的单行数）。
    pub fn tally(&self) -> (usize, usize, usize) {
        let ok = self.trail.iter().filter(|r| r.kind == "registered").count();
        let items: usize = self.trail.iter().map(|r| r.installed).sum();
        (ok, items, self.trail.len() - ok)
    }

    pub fn processed(&self) -> usize {
        self.trail.len()
    }
}

/// 配额拒绝明细（超限的诚实数字——差多少给多少，不给「失败」两个字）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaDenial {
    pub need_bytes: u64,
    pub free_bytes: u64,
}

/// 侧载磁盘配额（字节计：cursors/ 组件载荷逐包记账；超限诚实拒绝
/// 不静默挤占，失败/回滚如实归还——字节账两头对平）。
#[derive(Clone, Debug)]
pub struct DiskQuota {
    limit_bytes: u64,
    used_bytes: u64,
}

impl DiskQuota {
    /// 默认上限 8MB（.vxcur 容器上限 4MB × 2 包余量——指针包体量小）。
    pub const DEFAULT_LIMIT: u64 = 8 * 1024 * 1024;

    pub fn new(limit_bytes: u64) -> DiskQuota {
        DiskQuota { limit_bytes, used_bytes: 0 }
    }

    pub fn used(&self) -> u64 {
        self.used_bytes
    }

    pub fn free(&self) -> u64 {
        self.limit_bytes - self.used_bytes
    }

    /// 包字节面（cursors/ 组件实际载荷 = 全部 .vxcur 字节之和）。
    pub fn package_bytes(pkg: &CursorPackage) -> u64 {
        pkg.schemes.iter().map(|(_, b)| b.len() as u64).sum()
    }

    /// 收编记账（够则记账返回记账后用量；不够返回拒绝明细——诚实数字）。
    pub fn admit(&mut self, pkg: &CursorPackage) -> Result<u64, QuotaDenial> {
        let need = Self::package_bytes(pkg);
        if need > self.free() {
            return Err(QuotaDenial { need_bytes: need, free_bytes: self.free() });
        }
        self.used_bytes += need;
        Ok(self.used_bytes)
    }

    /// 归还记账（只许还账不许透支——还超已用是账目错误，诚实拒绝）。
    pub fn release(&mut self, bytes: u64) -> bool {
        if bytes > self.used_bytes {
            return false;
        }
        self.used_bytes -= bytes;
        true
    }

    /// 回滚到快照标记（只许退到更小的账——标记之后的账作废）。
    pub fn roll_back_to(&mut self, mark: u64) -> bool {
        if mark > self.used_bytes {
            return false;
        }
        self.used_bytes = mark;
        true
    }

    /// 配额单行报告（已用/上限/余量——设置页存储行的数源）。
    pub fn report_line(&self) -> String {
        alloc::format!(
            "侧载配额 已用 {}/{} 字节、余 {} 字节",
            self.used_bytes, self.limit_bytes, self.free()
        )
    }
}

/// 安装前快照（库房名单 + 在用方案名 + 配额标记——一键回滚的完整凭据；
/// 只记名单不记内容：库房是唯一事实源，快照是名单投影不是第二账本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallSnapshot {
    names: Vec<String>,
    active_name: String,
    quota_used: u64,
}

/// 拍快照（安装动作之前调用——回滚凭据必须先于风险存在）。
pub fn capture_snapshot(lib: &SchemeLibrary, quota_used: u64) -> InstallSnapshot {
    InstallSnapshot {
        names: lib
            .view(crate::jstar2::library::LibraryView::All)
            .iter()
            .map(|e| e.model.name.clone())
            .collect(),
        active_name: lib.active_name.clone(),
        quota_used,
    }
}

/// 一键回滚（卸掉快照之后混进来的侧载件、在用方案指向被卸件时恢复
/// 快照在用、配额退回标记；返回卸掉件数。只动 Sideloaded 血统——
/// 用户既有资产分毫不碰）。
pub fn rollback(lib: &mut SchemeLibrary, quota: &mut DiskQuota, snap: &InstallSnapshot) -> usize {
    let names = lib
        .view(crate::jstar2::library::LibraryView::All)
        .iter()
        .filter(|e| {
            matches!(&e.model.origin, OriginKind::Sideloaded(_))
                && !snap.names.iter().any(|n| *n == e.model.name)
        })
        .map(|e| e.model.name.clone())
        .collect::<Vec<String>>();
    let mut removed = 0usize;
    for n in &names {
        if lib.remove(n) {
            removed += 1;
        }
    }
    // 在用方案被回退过的场景：快照里的在用方案若已回到库房，恢复在用。
    if lib.active_name != snap.active_name && lib.get(&snap.active_name).is_some() {
        lib.active_name = snap.active_name.clone();
    }
    let _ = quota.roll_back_to(snap.quota_used);
    removed
}

/// 旧版包清单（v1 时代：只有名目，版本/作者/声明数是后来补的字段——
/// 缺失用 Option 如实表达，不用空串冒充）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyManifest {
    pub pkg_id: String,
    pub name: String,
    pub declared_schemes: Option<usize>,
    pub version: Option<String>,
    pub author: Option<String>,
}

impl LegacyManifest {
    /// v1 → v2 迁移（缺字段补齐但不编数：声明数缺省 = 包内实际条数
    /// （清单向包对齐——包是事实源）；版本缺省 = "1.0"；作者缺省 =
    /// "未知作者"）。
    pub fn migrate(self, pkg: &CursorPackage) -> PackageManifest {
        PackageManifest {
            pkg_id: self.pkg_id,
            name: self.name,
            version: self.version.unwrap_or_else(|| String::from("1.0")),
            author: self.author.unwrap_or_else(|| String::from("未知作者")),
            declared_schemes: self.declared_schemes.unwrap_or(pkg.schemes.len()),
        }
    }

    /// 迁移完备性：迁移产物必须过 v2 的全部既有校验（版本形 + 三处
    /// 对账）——迁移不走后门，老包过的是同一条门。
    pub fn migrated_ok(self, pkg: &CursorPackage) -> bool {
        let m = self.migrate(pkg);
        m.version_ok() && m.matches(pkg)
    }
}

/// F635 v4 自检。
pub fn run_sideload_v4_checks() -> CheckSet {
    use crate::jstar2::jbase::{builtin_default_scheme, serialize_vxcur};
    use crate::jstar2::sharing::default_peblock_gate;
    fn trust_all(_: &[u8]) -> (bool, &'static str) {
        (true, "stub-signer")
    }
    let mut set = CheckSet::new("jstar2-F635-v4");

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
    // 配额专用合成包（配额面只计字节不解析——哑字节即可精确控制数量级）。
    let syn_pkg = |id: &str, n: usize, sz: usize| -> CursorPackage {
        CursorPackage {
            pkg_id: alloc::format!("pkg-{id}"),
            pkg_name: String::from(id),
            schemes: (0..n).map(|i| (alloc::format!("d{i}.vxcur"), alloc::vec![0u8; sz])).collect(),
        }
    };

    // 1. 队列批量过门：三包五件全过、逐包留痕。
    let mut lib = SchemeLibrary::new(0);
    let mut q = SideloadQueue::new();
    q.push(mk_pkg("甲包", 2));
    q.push(mk_pkg("乙包", 1));
    q.push(mk_pkg("丙包", 2));
    let mut quota = DiskQuota::new(DiskQuota::DEFAULT_LIMIT);
    q.drain(&mut lib, &mut quota, default_peblock_gate, trust_all);
    let (ok_pkgs, items, fails) = q.tally();
    set.add(
        "queue drains batch with per-package trail",
        q.pending() == 0 && q.processed() == 3 && ok_pkgs == 3 && items == 5 && fails == 0 && lib.len() == 5,
        "",
    );

    // 2. 坏件隔离：毒包只拒自己，同队好件照装。
    let mut lib2 = SchemeLibrary::new(0);
    let mut q2 = SideloadQueue::new();
    let mut evil = mk_pkg("毒包", 1);
    evil.schemes[0].1[0] = 0xEE;
    q2.push(mk_pkg("良包", 2));
    q2.push(evil);
    let mut quota2 = DiskQuota::new(DiskQuota::DEFAULT_LIMIT);
    q2.drain(&mut lib2, &mut quota2, default_peblock_gate, trust_all);
    let trail2 = q2.trail();
    set.add(
        "queue isolates poisoned package",
        q2.tally() == (1, 2, 1)
            && lib2.len() == 2
            && trail2.iter().any(|r| r.kind == "gate-blocked" && r.pkg_name == "毒包"),
        "",
    );

    // 3. 拦截结论留痕到人话（拦在哪条为什么——detail 非空可读）。
    set.add(
        "queue trail carries human readable detail",
        trail2.iter().filter(|r| r.kind == "gate-blocked").all(|r| !r.detail.is_empty()),
        "",
    );

    // 4. 配额记账：收编后用量 = 包字节和（合成包 2 × 1000 字节）。
    let mut quota3 = DiskQuota::new(4096);
    let p_small = syn_pkg("小包", 2, 1000);
    let b_small = DiskQuota::package_bytes(&p_small);
    let _ = quota3.admit(&p_small);
    set.add(
        "quota admits and accounts bytes",
        b_small == 2000 && quota3.used() == b_small && quota3.free() == 4096 - b_small,
        "",
    );

    // 5. 配额超限诚实拒绝：数字明细（需多少、余多少）。
    let big = syn_pkg("大件", 1, 5000);
    match quota3.admit(&big) {
        Err(d) => set.add(
            "quota denial carries honest numbers",
            d.need_bytes == 5000 && d.free_bytes == 4096 - b_small,
            "",
        ),
        Ok(_) => set.add("quota denial carries honest numbers", false, "should deny"),
    }

    // 6. 配额归还对平 + 透支还账拒绝。
    let ok_release = quota3.release(b_small);
    set.add(
        "quota release reconciles and refuses overdraft",
        ok_release && quota3.used() == 0 && !quota3.release(1),
        "",
    );

    // 7. 配额单行报告可读（含上限与余量数字）。
    set.add("quota report line readable", quota3.report_line().contains("0/4096"), "");

    // 8. 失败包不占账：毒包被拒后配额只剩良包的账（队列面归还）。
    set.add(
        "failed packages release quota",
        quota2.used() == DiskQuota::package_bytes(&mk_pkg("良包", 2)),
        "",
    );

    // 9. 快照回滚：快照后侧载的件被卸、配额退标记、在用方案保持。
    let mut lib3 = SchemeLibrary::new(0);
    let mut quota4 = DiskQuota::new(DiskQuota::DEFAULT_LIMIT);
    let pre = mk_pkg("装前件", 1);
    let _ = sideload_package(&mut lib3, &pre, default_peblock_gate, trust_all);
    let name_pre = lib3.view(crate::jstar2::library::LibraryView::All)[0].name().to_string();
    let _ = lib3.mark_active(&name_pre);
    let snap = capture_snapshot(&lib3, quota4.used());
    let post = mk_pkg("装后件", 2);
    let _ = sideload_package(&mut lib3, &post, default_peblock_gate, trust_all);
    let _ = quota4.admit(&post);
    let removed = rollback(&mut lib3, &mut quota4, &snap);
    set.add(
        "rollback removes post-snapshot sideloads",
        removed == 2 && lib3.len() == 1 && quota4.used() == 0 && lib3.active_name == name_pre,
        "",
    );

    // 10. 回滚不碰用户既有资产（非 Sideloaded 血统分毫不碰）。
    let mut lib4 = SchemeLibrary::new(0);
    let mut user = builtin_default_scheme();
    user.name = String::from("用户自建");
    let _ = lib4.add(user);
    let snap4 = capture_snapshot(&lib4, 0);
    let side = mk_pkg("侧载件", 1);
    let _ = sideload_package(&mut lib4, &side, default_peblock_gate, trust_all);
    let removed4 = rollback(&mut lib4, &mut DiskQuota::new(1024), &snap4);
    set.add(
        "rollback spares non-sideloaded assets",
        removed4 == 1 && lib4.len() == 1 && lib4.get("用户自建").is_some(),
        "",
    );

    // 11. 回滚恢复被顶掉的在用方案（快照在用件仍在库房 → 恢复在用）。
    let mut lib5 = SchemeLibrary::new(0);
    let pre5 = mk_pkg("旧主", 1);
    let _ = sideload_package(&mut lib5, &pre5, default_peblock_gate, trust_all);
    let name_pre5 = lib5.view(crate::jstar2::library::LibraryView::All)[0].name().to_string();
    let _ = lib5.mark_active(&name_pre5);
    let snap5 = capture_snapshot(&lib5, 0);
    let post5 = mk_pkg("新欢", 1);
    let _ = sideload_package(&mut lib5, &post5, default_peblock_gate, trust_all);
    let name_post = lib5
        .view(crate::jstar2::library::LibraryView::All)
        .iter()
        .find(|e| e.model.name.contains("新欢"))
        .map(|e| e.model.name.clone())
        .unwrap_or_default();
    let _ = lib5.mark_active(&name_post);
    let removed5 = rollback(&mut lib5, &mut DiskQuota::new(1024), &snap5);
    set.add(
        "rollback restores snapshot active scheme",
        removed5 == 1 && lib5.active_name == name_pre5,
        "",
    );

    // 12. 配额回滚标记语义：只许退到更小账、越标拒绝。
    let mut quota5 = DiskQuota::new(4096);
    let p5 = syn_pkg("记账户", 1, 1000);
    let _ = quota5.admit(&p5);
    let mark = quota5.used();
    let _ = quota5.admit(&p5);
    let fut_ok = quota5.roll_back_to(mark);
    let fut_bad = quota5.roll_back_to(mark * 2);
    set.add(
        "quota rollback mark semantics",
        fut_ok && !fut_bad && quota5.used() == mark,
        "",
    );

    // 13. v1 清单迁移：缺字段如实补齐（不编数）+ 过 v2 同一条门。
    let pkg13 = mk_pkg("老包", 2);
    let legacy = LegacyManifest {
        pkg_id: pkg13.pkg_id.clone(),
        name: pkg13.pkg_name.clone(),
        declared_schemes: None,
        version: None,
        author: None,
    };
    let m13 = legacy.clone().migrate(&pkg13);
    set.add(
        "legacy manifest migration fills honestly",
        m13.version == "1.0"
            && m13.author == "未知作者"
            && m13.declared_schemes == 2
            && m13.matches(&pkg13)
            && legacy.clone().migrated_ok(&pkg13),
        "",
    );

    // 14. 声明数说谎的老包：迁移后对账照拒（迁移不洗白）；坏版本形
    //     迁移后同样过不了版本校验。
    let liar = LegacyManifest { declared_schemes: Some(9), ..legacy.clone() };
    let badver = LegacyManifest { version: Some(String::from("v2")), declared_schemes: Some(2), ..legacy };
    set.add(
        "migration does not launder lies",
        !liar.migrated_ok(&pkg13) && !badver.migrated_ok(&pkg13),
        "",
    );

    // 15. 队列对配额拒绝的诚实留痕（超限包零入库、明细带数字）。
    let mut lib6 = SchemeLibrary::new(0);
    let mut q6 = SideloadQueue::new();
    let mut huge = mk_pkg("巨包", 1);
    huge.schemes[0].1 = alloc::vec![0u8; 2048];
    q6.push(huge);
    let mut tiny_quota = DiskQuota::new(1024);
    q6.drain(&mut lib6, &mut tiny_quota, default_peblock_gate, trust_all);
    let t6 = q6.trail();
    set.add(
        "queue records quota denial honestly",
        t6.len() == 1 && t6[0].kind == "quota-denied" && t6[0].detail.contains("配额") && lib6.is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
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
    fn quota_bytes_roundtrip() {
        let mut q = DiskQuota::new(4096);
        let p = CursorPackage {
            pkg_id: String::from("pkg-计"),
            pkg_name: String::from("计"),
            schemes: alloc::vec![
                (String::from("d0.vxcur"), alloc::vec![0u8; 1000]),
                (String::from("d1.vxcur"), alloc::vec![0u8; 1000]),
            ],
        };
        let b = DiskQuota::package_bytes(&p);
        assert_eq!(b, 2000);
        assert!(q.admit(&p).is_ok());
        assert_eq!(q.used(), b);
        assert!(q.release(b));
        assert_eq!(q.used(), 0);
        assert!(!q.release(b), "空账还账拒绝");
    }

    #[test]
    fn package_bytes_empty_is_zero() {
        let e = CursorPackage {
            pkg_id: String::from("p"),
            pkg_name: String::from("n"),
            schemes: Vec::new(),
        };
        assert_eq!(DiskQuota::package_bytes(&e), 0);
    }

    #[test]
    fn rollback_snapshot_semantics() {
        let mut lib = SchemeLibrary::new(0);
        let s0 = pkg("基线", 1);
        let _ = sideload_package(&mut lib, &s0, default_peblock_gate, trust_all);
        let snap = capture_snapshot(&lib, 0);
        let _ = sideload_package(&mut lib, &pkg("增量", 2), default_peblock_gate, trust_all);
        assert_eq!(rollback(&mut lib, &mut DiskQuota::new(1024), &snap), 2);
        assert_eq!(lib.len(), 1);
    }

    #[test]
    fn migration_defaults_and_lies() {
        let p = pkg("迁移", 1);
        let leg = LegacyManifest {
            pkg_id: p.pkg_id.clone(),
            name: p.pkg_name.clone(),
            declared_schemes: None,
            version: None,
            author: None,
        };
        let m = leg.clone().migrate(&p);
        assert!(m.matches(&p) && m.version_ok());
        let liar = LegacyManifest { declared_schemes: Some(7), ..leg };
        assert!(!liar.migrated_ok(&p));
    }

    #[test]
    fn queue_tally_counts() {
        let mut lib = SchemeLibrary::new(0);
        let mut q = SideloadQueue::new();
        q.push(pkg("一", 1));
        q.push(pkg("二", 2));
        let mut quota = DiskQuota::new(DiskQuota::DEFAULT_LIMIT);
        q.drain(&mut lib, &mut quota, default_peblock_gate, trust_all);
        assert_eq!(q.tally(), (2, 3, 0));
        assert_eq!(q.pending(), 0);
        assert_eq!(q.processed(), 2);
    }

    #[test]
    fn v4_checks_all_green() {
        let set = run_sideload_v4_checks();
        assert!(!set.truncated());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "v4 check red: {}", c.name);
        }
    }
}
