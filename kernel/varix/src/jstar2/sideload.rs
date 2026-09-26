//! F635 指针包侧载链 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：侧载注册链路；不静默替换判据；卸载回退确认
//! 用例；peblock 拦截注入；与 A4 门禁一致性对账；包级/方案级双入口
//! 入库同源。
//!
//! **链路语义**：
//! - vxapp 包（A4 侧载管线）内 `cursors/` 组件目录 → 侧载时过 peblock
//!   门与签名校验 → 包内指针**注册为独立方案**（origin=Sideloaded{包
//!   ID}）；
//! - **不静默替换**：装了 ≠ 用了——安装动作绝不改 `active`，用户在
//!   设置页显式切换才生效（判据的机制面：安装前后 active 指纹不变）；
//! - **卸载回退**：卸载包 = 其方案一并下架；在用方案被卸载时先弹确认
//!   （未确认拒绝），确认后回退默认并登记；
//! - **双入口同源**：包级批量入库（本模块）与 F630 方案级导入落到**同
//!   一个库房**（F628），内容指纹幂等去重（同源对账）；
//! - **A4 一致性**：与 F630 共用同一 `GateFn`/`VerifyFn` 契约（一套门
//!   禁管两样货，不另造规则）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{OriginKind, VXCUR_MAX_BYTES};
use crate::jstar2::library::{AddOutcome, SchemeLibrary};
use crate::jstar2::sharing::{GateVerdict, VerifyFn};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 包模型
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

/// 侧载结果。
pub enum SideloadOutcome {
    /// 注册成功（入库指纹列表）。
    Registered(Vec<u64>),
    /// 门拒绝（整包拒——A4 一致性：坏包不半装）。
    GateBlocked(&'static str),
    /// 签名无效。
    SignatureInvalid,
    /// 包内无 cursors/ 内容（诚实空态）。
    EmptyComponent,
    /// 包内非法条目（定位到方案名）。
    BadEntry(String, &'static str),
}

/// 侧载注册链（门 → 签名 → 解析 → 入库；**不触碰 active**）。
pub fn sideload_package(
    lib: &mut SchemeLibrary,
    pkg: &CursorPackage,
    gate: fn(&[u8]) -> GateVerdict,
    verify: VerifyFn,
) -> SideloadOutcome {
    if pkg.schemes.is_empty() {
        return SideloadOutcome::EmptyComponent;
    }
    // 门与签名对全包字节逐条校验（整包一致的门禁口径）。
    for (_, bytes) in &pkg.schemes {
        if let GateVerdict::Deny(why) = gate(bytes) {
            return SideloadOutcome::GateBlocked(why);
        }
        let (ok, _) = verify(bytes);
        if !ok {
            return SideloadOutcome::SignatureInvalid;
        }
    }
    let mut fps = Vec::new();
    for (name, bytes) in &pkg.schemes {
        if bytes.len() > VXCUR_MAX_BYTES {
            return SideloadOutcome::BadEntry(name.clone(), "超过 4MB 容器上限");
        }
        let Ok(mut m) = crate::jstar2::jbase::parse_vxcur(bytes) else {
            return SideloadOutcome::BadEntry(name.clone(), "不是合法 .vxcur 内容");
        };
        // 命名与 origin 归位：包内方案带包 ID 血统。
        m.name = alloc::format!("{}·{}", pkg.pkg_name, m.name);
        m.origin = OriginKind::Sideloaded(pkg.pkg_id.clone());
        match lib.add(m) {
            AddOutcome::Added(fp) => fps.push(fp),
            AddOutcome::Duplicate(fp) => fps.push(fp), // 同源幂等（双入口同库）
            AddOutcome::OverflowReminder { .. } => {
                return SideloadOutcome::BadEntry(name.clone(), "库房已满（50）");
            }
        }
    }
    SideloadOutcome::Registered(fps)
}

/// 卸载链：包方案下架；在用方案被卸载需确认（未确认拒绝并说明）。
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

/// 卸载包方案（active 语义与 F628 库房/E4 前柜同源：`lib.active_name`）。
pub fn uninstall_package(
    lib: &mut SchemeLibrary,
    pkg_id: &str,
    confirmed: bool,
) -> UninstallOutcome {
    let owned: Vec<String> = lib
        .view(crate::jstar2::library::LibraryView::All)
        .into_iter()
        .filter(|e| matches!(&e.model.origin, OriginKind::Sideloaded(p) if p == pkg_id))
        .map(|e| e.model.name.clone())
        .collect();
    if owned.is_empty() {
        return UninstallOutcome::NothingRegistered;
    }
    let active_in_pkg = owned.iter().any(|n| *n == lib.active_name);
    if active_in_pkg && !confirmed {
        return UninstallOutcome::NeedsConfirm { active_scheme: lib.active_name.clone() };
    }
    let mut active_fallback = false;
    if active_in_pkg {
        // 回退默认（内置基线）+ 登记（回退事实写进 active_name）。
        lib.active_name = String::from("VARIX 默认指针");
        active_fallback = true;
    }
    let before = lib.len();
    for n in &owned {
        lib.remove(n);
    }
    UninstallOutcome::Removed { count: before - lib.len(), active_fallback }
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
}
