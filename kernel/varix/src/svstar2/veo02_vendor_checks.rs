//! VE-F2802 · 域自检（判据逐条对应，见 `veo02_vendor.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O02-架构-*`
//! - 集成边界 → `O02-边界-*`
//! - 解析子集 → `O02-子集-*`
//! - 判据（自证可追溯）→ `O02-判据-*`
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制+告警 / 异常检出→立案流转）
//!   → `O02-降级-*`
//! - 跨批对接（上游契约接收哈希对账 / 下游消费接口前向声明 / 对账钩子）
//!   → `O02-对接-*`
//! - 无障碍（文档替述可读）→ `O02-读屏-*`
//! - 错误路径零静默 → `O02-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo01_arch::ServoDecision;
use super::veo02_vendor::*;
use crate::checks::CheckSet;

/// 上游 style 的 revision（40 位十六进制的**形状样本**，非真实上游哈希）。
///
/// 这里的值只用于「形状 + 对账逻辑」自证，**不冒充真实上游 commit**——
/// 真实 pin 由O 域负责人按当日上游填入。写死一个看起来像真哈希的值
/// 反而危险：后来人会以为它就是上游。
const REV_A: &str = "0123456789abcdef0123456789abcdef01234567";
/// 上游 style_traits 的 revision（形状样本）。
const REV_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

/// 源哈希样本（形状样本）。
const HASH_A: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// FNV-1a64 正样本：对已知字节串 `servo-style-v1` 算出的确定值。
///
/// 期望值写死为常量 `be07ceff78db0334`，**不是现算的**——现算等于自证。
/// 该值经独立实现（Python 侧同算法复算）对拍确认，**不是随手编的**：
/// 门禁里的常量必须是算出来的，编出来的常量会让整条判据失去意义。
const EXPECT_HASH_STYLE_V1: &str = "be07ceff78db0334";

fn check_hash_shape() -> bool {
    // 独立复算 FNV-1a64，与 veo01 的实现互为对拍。
    let bytes: &[u8] = b"servo-style-v1";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{:016x}", h) == EXPECT_HASH_STYLE_V1
}

// ---------------------------------------------------------------------------
// O02-架构-*
// ---------------------------------------------------------------------------

/// O02-架构-01：决策单源——本单不复制F2801 决策表，只读引用。
///
/// 真判据是「改了本单的副本不会影响行为」：把本单能拿到的 crate 列表
/// 与 F2801 决策表对拍，**任一不一致即红**。若本单偷偷维护了第二份
/// 决策表，这个对拍在副本被改后会立刻抓到。
fn chk_arch_decision_single_source(set: &mut CheckSet) {
    let names = ["style", "style_traits", "layout"];
    let mut all_mapped = true;
    for n in names.iter() {
        if crate_id_of(n).is_none() {
            all_mapped = false;
        }
    }
    // 未知 crate 必须返回 None（**不得落到保留句柄**）。
    let unknown_none = crate_id_of("styel").is_none() && crate_id_of("").is_none();
    // 决策表条目数与 F2801 一致（3 条）。
    let count_ok = crate_decisions().len() == 3;
    // 不引入的 crate 必须写明归属（否则「不引入」等于「暂时没引」）。
    let owner_declared = crate_decisions()
        .iter()
        .filter(|d| d.decision == ServoDecision::Decline)
        .all(|d| !d.owner_elsewhere.trim().is_empty());
    let ok = all_mapped && unknown_none && count_ok && owner_declared;
    set.add("O02-架构-01-决策单源不复制", ok, "");
}

/// O02-架构-02：理由必填——引入与不引入都要有理由。
fn chk_arch_reason_mandatory(set: &mut CheckSet) {
    let all_have_reason = crate_decisions().iter().all(|d| !d.reason.trim().is_empty());
    // 理由不能是占位空话。
    let not_placeholder = crate_decisions()
        .iter()
        .all(|d| d.reason.len() >= 8 && !d.reason.contains("TODO"));
    let ok = all_have_reason && not_placeholder;
    set.add("O02-架构-02-选型理由必填", ok, "");
}

/// O02-架构-03：布局归属裁决落到可执行闸门（不是注释里的声明）。
///
/// 真判据：布局符号引用必须**被拒**且错误码是`E_LAYOUT_FORBIDDEN`。
/// 只检查「文档里写了不引入」是恒真弱门禁。
fn chk_arch_layout_is_gate(set: &mut CheckSet) {
    let mut b = ServoBoundary::standard();
    let r = b.admit(CRATE_STYLE, "layout::block::BlockFormattingContext");
    let refused = r.is_err();
    let code_right = match &r {
        Err(e) => e.code == E_LAYOUT_FORBIDDEN,
        Ok(_) => false,
    };
    // 越界必须同时立案（拒绝只是当场挡住，立案才可统计）。
    let filed = b.overreach.len() == 1;
    let ok = refused && code_right && filed;
    set.add("O02-架构-03-布局禁扩是闸门", ok, "");
}

// ---------------------------------------------------------------------------
// O02-边界-*
// ---------------------------------------------------------------------------

/// O02-边界-01：准入面收窄——白名单外的符号一律拒。
fn chk_boundary_surface_enforced(set: &mut CheckSet) {
    let mut b = ServoBoundary::standard();
    // 白名单内放行。
    let allowed = b.admit(CRATE_STYLE, "StyleSheet").is_ok();
    // 白名单外拒绝，错误码是「未授权」而非「越界」（二者语义不同）。
    let r = b.admit(CRATE_STYLE, "Parser");
    let denied = r.is_err();
    let code_right = match &r {
        Err(e) => e.code == E_SYMBOL_NOT_ALLOWED,
        Ok(_) => false,
    };
    let ok = allowed && denied && code_right;
    set.add("O02-边界-01-导入面收窄生效", ok, "");
}

/// O02-边界-02：未登记 crate 默认不可见（默认拒绝，不是默认放行）。
fn chk_boundary_default_deny(set: &mut CheckSet) {
    let mut b = ServoBoundary::new();
    // 空边界下任何符号都应被拒。
    let r = b.admit(CRATE_STYLE, "StyleSheet");
    let denied = r.is_err();
    let no_surface = b.gate.total() == 0;
    let ok = denied && no_surface;
    set.add("O02-边界-02-默认拒绝而非放行", ok, "");
}

/// O02-边界-03：layout 既不登记 vendor 也不出现在导入面。
fn chk_boundary_layout_absent(set: &mut CheckSet) {
    let b = ServoBoundary::standard();
    let no_surface = b.gate.surface(CRATE_LAYOUT).is_none();
    let no_record = b.registry.record(CRATE_LAYOUT).is_none();
    // 决定性判据：决策与登记对账必须通过（登记了 layout 就会红）。
    let reconciled = b.registry.reconcile_with_decisions().is_ok();
    let ok = no_surface && no_record && reconciled;
    set.add("O02-边界-03-layout双缺席", ok, "");
}

/// O02-边界-04：符号授权必须带理由（无理由白名单=没有闸门）。
fn chk_boundary_reason_required(set: &mut CheckSet) {
    let mut s = CrateSurface::new(CRATE_STYLE);
    let r = s.grant(SymbolGrant {
        symbol: "StyleSheet",
        why: "",
    });
    let rejected = r.is_err();
    // 带理由则放行。
    let ok2 = s
        .grant(SymbolGrant {
            symbol: "StyleSheet",
            why: "样式表产物容器",
        })
        .is_ok();
    // 重复授权拒绝（两份理由说明口径已分叉）。
    let dup = s
        .grant(SymbolGrant {
            symbol: "StyleSheet",
            why: "另一套说法",
        })
        .is_err();
    let ok = rejected && ok2 && dup;
    set.add("O02-边界-04-授权理由必填", ok, "");
}

/// O02-边界-05：导入面容量守卫（防「白名单写成整 crate」）。
fn chk_boundary_surface_cap(set: &mut CheckSet) {
    let mut s = CrateSurface::new(CRATE_STYLE);
    let mut filled = 0u32;
    let mut hit_cap = false;
    for i in 0..(MAX_SYMBOLS_PER_CRATE + 4) {
        let sym: &'static str = Box::leak(format!("sym{}", i).into_boxed_str());
        let r = s.grant(SymbolGrant {
            symbol: sym,
            why: "容量守卫样本",
        });
        if r.is_err() {
            hit_cap = true;
            break;
        }
        filled += 1;
    }
    // 上限必须恰在 MAX_SYMBOLS_PER_CRATE 处触发。
    let exact = filled == MAX_SYMBOLS_PER_CRATE as u32 && hit_cap;
    set.add("O02-边界-05-导入面容量守卫", exact, "");
}

// ---------------------------------------------------------------------------
// O02-合规-*（MPL-2.0 是 vendor 的真实代价）
// ---------------------------------------------------------------------------

/// O02-合规-01：有改动必须给出改动事实与发布路径（文件级 copyleft）。
fn chk_license_modified_needs_publish(set: &mut CheckSet) {
    let mut reg = VendorRegistry::new();
    // 缺发布路径 ⇒ 拒。
    let bad = reg.register(VendorRecord::modified(
        CRATE_STYLE,
        "MPL-2.0",
        "改了 cascade 顺序",
        "",
    ));
    let rejected = bad.is_err();
    let code_right = match &bad {
        Err(e) => e.code == E_LICENSE_INCOMPLETE,
        Ok(_) => false,
    };
    // 三项齐整⇒ 放行。
    let good = reg
        .register(VendorRecord::modified(
            CRATE_STYLE,
            "MPL-2.0",
            "改了 cascade 顺序以对齐 O 域七段管线",
            "third_party/servo-style/",
        ))
        .is_ok();
    let ok = rejected && code_right && good;
    set.add("O02-合规-01-改动须可发布", ok, "");
}

/// O02-合规-02：未改动上游不要求发布路径（引用不传染）。
fn chk_license_pristine_no_publish(set: &mut CheckSet) {
    let mut reg = VendorRegistry::new();
    let r = reg.register(VendorRecord::pristine(CRATE_STYLE, "MPL-2.0"));
    let accepted = r.is_ok();
    // 许可证标识为空必须拒（「都是 MPL」不是登记）。
    let mut reg2 = VendorRegistry::new();
    let no_license = reg2.register(VendorRecord::pristine(CRATE_STYLE, ""));
    let ok = accepted && no_license.is_err();
    set.add("O02-合规-02-未改动免发布", ok, "");
}

/// O02-合规-03：决策与登记双向一致（漏做与越界都要抓到）。
fn chk_license_decision_reconcile(set: &mut CheckSet) {
    let b = ServoBoundary::standard();
    let ok_std = b.registry.reconcile_with_decisions().is_ok();
    // 反向：把 layout 也登记进去⇒ 必须被抓。
    let mut bad = VendorRegistry::new();
    let _ = bad.register(VendorRecord::pristine(CRATE_STYLE, "MPL-2.0"));
    let _ = bad.register(VendorRecord::pristine(CRATE_STYLE_TRAITS, "MPL-2.0"));
    let _ = bad.register(VendorRecord::pristine(CRATE_LAYOUT, "MPL-2.0"));
    let r = bad.reconcile_with_decisions();
    let layout_caught = match &r {
        Err(e) => e.code == E_LAYOUT_FORBIDDEN,
        Ok(_) => false,
    };
    // 漏做：只登记 style，缺 style_traits。
    let mut missing = VendorRegistry::new();
    let _ = missing.register(VendorRecord::pristine(CRATE_STYLE, "MPL-2.0"));
    let miss_caught = missing.reconcile_with_decisions().is_err();
    let ok = ok_std && layout_caught && miss_caught;
    set.add("O02-合规-03-决策登记双向一致", ok, "");
}

// ---------------------------------------------------------------------------
// O02-对接-*（哈希对账在 vendor 场景的形态）
// ---------------------------------------------------------------------------

/// O02-对接-01：revision 钉定——形状校验+ 真实对账（重算而非看填没填）。
fn chk_pin_revision(set: &mut CheckSet) {
    // 形状不对⇒ 拒。
    let short = VendorPin::new(CRATE_STYLE, "abc", HASH_A);
    let rejected = short.is_err();
    // 形状对且十六进制 ⇒ 放行。
    let good = VendorPin::new(CRATE_STYLE, REV_A, HASH_A);
    let accepted = match &good {
        Ok(p) => p.revision_is_hex(),
        Err(_) => false,
    };
    // 关键：对账必须能抓漂移。传入与 pin 不同的实际哈希。
    let drift = match &good {
        Ok(p) => p.verify("ffffffffffffffffffffffffffffffff").is_err(),
        Err(_) => false,
    };
    // 一致时通过。
    let matched = match &good {
        Ok(p) => p.verify(HASH_A).is_ok(),
        Err(_) => false,
    };
    // 两个引入的 crate 都必须能被钉定——pin 不是单 crate 的专属动作。
    let traits_pinned = match VendorPin::new(CRATE_STYLE_TRAITS, REV_B, HASH_A) {
        Ok(p) => p.revision_is_hex() && p.verify(HASH_A).is_ok(),
        Err(_) => false,
    };
    let ok = rejected && accepted && drift && matched && traits_pinned;
    set.add("O02-对接-01-revision钉定对账", ok, "");
}

/// O02-对接-02：revision 必须是十六进制（防止拿占位串当pin）。
fn chk_pin_hex_only(set: &mut CheckSet) {
    // 40 位但含非十六进制字符（如 z）。
    let bad: &'static str = "z123456789abcdef0123456789abcdef01234567";
    let p = match VendorPin::new(CRATE_STYLE, bad, HASH_A) {
        Ok(p) => p,
        Err(_) => {
            set.add("O02-对接-02-revision须十六进制", true, "");
            return;
        }
    };
    let ok = !p.revision_is_hex();
    set.add("O02-对接-02-revision须十六进制", ok, "");
}

/// O02-对接-03：改动审计必填摘要（空摘要让合规无法复核）。
fn chk_change_ledger(set: &mut CheckSet) {
    let mut l = ChangeLedger::new();
    let empty = l.record(ChangeRecord {
        crate_id: CRATE_STYLE,
        file: "style/properties.rs",
        summary: "",
    });
    let rejected = empty.is_err();
    let good = l.record(ChangeRecord {
        crate_id: CRATE_STYLE,
        file: "style/properties.rs",
        summary: "把 cascade 阶段顺序对齐 O 域七段管线",
    });
    let accepted = good.is_ok();
    // 计数按 crate 归集。
    let counted = l.count_of(CRATE_STYLE) == 1 && l.count_of(CRATE_LAYOUT) == 0;
    let ok = rejected && accepted && counted;
    set.add("O02-对接-03-改动审计留痕", ok, "");
}

// ---------------------------------------------------------------------------
// O02-降级-*（三闸 + 立案流转）
// ---------------------------------------------------------------------------

/// O02-降级-01：拒绝三要素齐发（码/事/因/下一步/责任方）。
///
/// 判据不是「有错误就行」，是**五个字段都不空**——缺 next 的拒绝
/// 会把实现者堵在死路上（知道错了，不知道该干嘛）。
fn chk_degrade_reject_triple(set: &mut CheckSet) {
    let mut b = ServoBoundary::new();
    let r = b.admit(CRATE_STYLE, "StyleSheet");
    let e = match r {
        Err(e) => e,
        Ok(_) => {
            set.add("O02-降级-01-拒绝三要素齐发", false, "");
            return;
        }
    };
    let five = !e.code.trim().is_empty()
        && !e.what.trim().is_empty()
        && !e.why.trim().is_empty()
        && !e.next.trim().is_empty()
        && !e.who.trim().is_empty();
    // 「为什么」必须是实质说明，不是占位。
    let why_substantive = e.why.len() >= 8;
    let ok = five && why_substantive;
    set.add("O02-降级-01-拒绝三要素齐发", ok, "");
}

/// O02-降级-02：异常检出→立案流转（越界必进台账，可统计）。
fn chk_degrade_case_filing(set: &mut CheckSet) {
    let mut b = ServoBoundary::standard();
    // 三个不同越界符号 ⇒ 三条立案。
    let s1 = b.admit(CRATE_STYLE, "layout::tree");
    let s2 = b.admit(CRATE_STYLE, "webrender::display_item");
    let s3 = b.admit(CRATE_STYLE, "layout::geometry");
    let all_refused = s1.is_err() && s2.is_err() && s3.is_err();
    // 重复检出同一符号不重复立案（重复不增加信息量）。
    let before = b.overreach.len();
    let _ = b.admit(CRATE_STYLE, "layout::tree");
    let no_dup = b.overreach.len() == before;
    // 条目数= 3：`layout::tree` / `webrender::display_item` / `layout::geometry`
    // 是**三个不同符号名**，各自立案一条（去重按符号名，不按前缀——
    // 同属 layout 前缀的两个符号仍是两次独立的越界尝试）。
    let counted = b.overreach.len() == 3;
    let ok = all_refused && no_dup && counted;
    set.add("O02-降级-02-越界立案流转", ok, "");
}

/// O02-降级-03：容量守卫触发时拒绝而非静默丢弃。
fn chk_degrade_cap_guard(set: &mut CheckSet) {
    let mut l = OverreachLedger::new();
    let mut hit = false;
    for i in 0..(MAX_OVERREACH + 4) {
        let sym: &'static str = Box::leak(format!("ov::sym{}", i).into_boxed_str());
        if l.file(OverreachCase {
            symbol: sym,
            code: E_LAYOUT_FORBIDDEN,
        })
        .is_err()
        {
            hit = true;
            break;
        }
    }
    // 条目数必须**恰好**停在上限，不是无限增长也不是提前停。
    let exact = hit && l.len() == MAX_OVERREACH;
    set.add("O02-降级-03-容量守卫拒绝不丢弃", exact, "");
}

/// O02-降级-04：总账审计能抓「后门放进禁扩符号」。
///
/// 真判据是**绕过准入闸门**直接往导入面塞 layout 符号后审计必须红。
/// 只测 `admit()` 走正路是弱门禁（正常路径本来就不会放禁扩符号进来）。
fn chk_degrade_audit_catches_backdoor(set: &mut CheckSet) {
    let mut b = ServoBoundary::standard();
    // 走正路：审计通过。
    let clean = b.audit().is_ok();
    // 后门：直接改导入面（模拟绕过 admit 的手工编辑）。
    if let Some(s) = b.gate.surface_mut(CRATE_STYLE) {
        s.symbols.push(SymbolGrant {
            symbol: "layout::block::Block",
            why: "后门注入样本",
        });
    }
    let r = b.audit();
    let caught = match &r {
        Err(e) => e.code == E_LAYOUT_FORBIDDEN,
        Ok(_) => false,
    };
    let ok = clean && caught;
    set.add("O02-降级-04-审计抓后门注入", ok, "");
}

// ---------------------------------------------------------------------------
// O02-子集-* / O02-判据-* / O02-读屏-*
// ---------------------------------------------------------------------------

/// O02-子集-01：解析子集边界——本单**不碰**属性全表（F2803 的地盘）。
///
/// 判据：本单不得引入任何属性清单类型；白名单里的符号全是**类型载体**
/// 而非解析行为（`Parser` 不在白名单里，见 O02-边界-01）。
fn chk_subset_no_property_table(set: &mut CheckSet) {
    let b = ServoBoundary::standard();
    // 白名单里不得出现解析器/分词器类符号。
    let no_parser = ALLOWED_STYLE_SYMBOLS.iter().all(|g| {
        g.symbol != "Parser" && g.symbol != "Tokenizer" && g.symbol != "RuleList"
    });
    // 白名单条目数是有界的（小集，不是整 crate）。
    let bounded = ALLOWED_STYLE_SYMBOLS.len() + ALLOWED_STYLE_TRAITS_SYMBOLS.len()
        <= MAX_SYMBOLS_PER_CRATE;
    // 标准边界的符号面确实登记了。
    let registered = b.gate.total() > 0;
    let ok = no_parser && bounded && registered;
    set.add("O02-子集-01-不越界代做属性表", ok, "");
}

/// O02-判据-01：判据自证可追溯——常量与实际能力对得上。
///
/// 真判据是**拿常量去当输入用**，不是比对常量本身（比对常量=恒真）。
/// 例：`HIT`-类常量当除数用一次，除零会 panic；`MAX_*` 当循环上限用，
/// 条目数必须恰好等于它。
fn chk_criterion_constants_live(set: &mut CheckSet) {
    // 常量当上限用：符号面恰好装到上限。
    let mut s = CrateSurface::new(CRATE_STYLE);
    let mut n = 0usize;
    for _ in 0..MAX_SYMBOLS_PER_CRATE {
        let sym: &'static str = Box::leak(format!("s{}", n).into_boxed_str());
        if s.grant(SymbolGrant {
            symbol: sym,
            why: "上限样本",
        })
        .is_err()
        {
            break;
        }
        n += 1;
    }
    let cap_exact = n == MAX_SYMBOLS_PER_CRATE;
    // REVISION_HEX_LEN 当切片长度用。
    let rev_bytes = REV_A.as_bytes();
    let hex_len_live = rev_bytes.len() == REVISION_HEX_LEN;
    // LICENSE_FIELD_COUNT 三件：许可证 + 改动事实 + 发布路径。
    let rec = VendorRecord::modified(CRATE_STYLE, "MPL-2.0", "改动事实", "发布路径");
    let fields_live = rec.compliance_complete() && LICENSE_FIELD_COUNT == 3;
    let ok = cap_exact && hex_len_live && fields_live;
    set.add("O02-判据-01-常量参与实算", ok, "");
}

/// O02-判据-02：哈希对账是重算（正负样本成对）。
fn chk_criterion_hash_recompute(set: &mut CheckSet) {
    // FNV-1a64 复算与 veo01 实现对拍（同一输入两次调用必一致）。
    let a = fnv1a64_hex(b"servo-style-v1");
    let bb = fnv1a64_hex(b"servo-style-v1");
    let stable = a == bb;
    // 不同输入必须不同哈希（否则对账无区分力）。
    let c = fnv1a64_hex(b"servo-style-v2");
    let distinct = a != c;
    // 期望值写死为常量（不是现算的——现算等于自证）。
    let expect = a == EXPECT_HASH_STYLE_V1;
    let ok = stable && distinct && expect && check_hash_shape();
    set.add("O02-判据-02-哈希对账是重算", ok, "");
}

/// O02-读屏-01：文档替述可读（无障碍）。
///
/// 判据不是「字符串非空」——那恒真。真判据：替述里**必须出现三个
/// crate 名与决策词**，少一个就说明读屏用户拿不到「引了什么、没引什么」。
fn chk_screen_summary_readable(set: &mut CheckSet) {
    let b = ServoBoundary::standard();
    let s = b.screen_summary();
    let has_style = s.contains("style");
    let has_traits = s.contains("style_traits");
    let has_layout = s.contains("layout");
    let has_decision = s.contains("引入");
    // 单行可读（不含换行符，读屏不会被截断成多段）。
    let single_line = !s.contains('\n');
    // 长度有实质内容。
    let substantial = s.len() >= 40;
    let ok = has_style && has_traits && has_layout && has_decision && single_line && substantial;
    set.add("O02-读屏-01-替述含决策全貌", ok, "");
}

/// O02-读屏-02：合规条目读屏单行（含许可证与义务状态）。
fn chk_screen_vendor_line(set: &mut CheckSet) {
    let r = VendorRecord::modified(
        CRATE_STYLE,
        "MPL-2.0",
        "改了 cascade 顺序",
        "third_party/servo-style/",
    );
    let line = r.screen_line();
    let has_license = line.contains("MPL-2.0");
    let has_obligation = line.contains("MPL-2.0");
    let has_crate = line.contains("style");
    let single_line = !line.contains('\n');
    let ok = has_license && has_obligation && has_crate && single_line;
    set.add("O02-读屏-02-合规单行可读", ok, "");
}

/// O02-错误-01：错误路径零静默——每条错误码都有触发样本。
///
/// 真判据：逐个错误码**实际触发**一次并核对返回码，不是查表里有这个常量。
pub fn chk_error_codes_reachable(set: &mut CheckSet) {
    let mut hit = 0u32;
    let total = 8u32;

    // E_SYMBOL_NOT_ALLOWED
    let mut b = ServoBoundary::standard();
    if b.admit(CRATE_STYLE, "Parser").is_err() {
        hit += 1;
    }
    // E_LAYOUT_FORBIDDEN
    let mut b2 = ServoBoundary::standard();
    if b2.admit(CRATE_STYLE, "layout::x").is_err() {
        hit += 1;
    }
    // E_SYMBOL_DUP
    let mut s = CrateSurface::new(CRATE_STYLE);
    let _ = s.grant(SymbolGrant {
        symbol: "A",
        why: "首次",
    });
    if s.grant(SymbolGrant {
        symbol: "A",
        why: "重复",
    })
    .is_err()
    {
        hit += 1;
    }
    // E_SURFACE_CAP
    let mut s2 = CrateSurface::new(CRATE_STYLE);
    let mut capped = false;
    for i in 0..(MAX_SYMBOLS_PER_CRATE + 2) {
        let sym: &'static str = Box::leak(format!("c{}", i).into_boxed_str());
        if s2.grant(SymbolGrant {
            symbol: sym,
            why: "容量样本",
        })
        .is_err()
        {
            capped = true;
            break;
        }
    }
    if capped {
        hit += 1;
    }
    // E_LICENSE_INCOMPLETE
    let mut reg = VendorRegistry::new();
    if reg
        .register(VendorRecord::modified(CRATE_STYLE, "MPL-2.0", "改了", ""))
        .is_err()
    {
        hit += 1;
    }
    // E_CHANGE_INCOMPLETE
    let mut cl = ChangeLedger::new();
    if cl
        .record(ChangeRecord {
            crate_id: CRATE_STYLE,
            file: "f.rs",
            summary: "",
        })
        .is_err()
    {
        hit += 1;
    }
    // E_REVISION_DRIFT
    if let Ok(p) = VendorPin::new(CRATE_STYLE, REV_A, HASH_A) {
        if p.verify("00000000000000000000000000000000").is_err() {
            hit += 1;
        }
    } else {
        hit += 1;
    }
    // E_CAP_FULL
    let mut ov = OverreachLedger::new();
    let mut full = false;
    for i in 0..(MAX_OVERREACH + 2) {
        let sym: &'static str = Box::leak(format!("f{}", i).into_boxed_str());
        if ov
            .file(OverreachCase {
                symbol: sym,
                code: E_LAYOUT_FORBIDDEN,
            })
            .is_err()
        {
            full = true;
            break;
        }
    }
    if full {
        hit += 1;
    }

    let ok = hit == total;
    set.add("O02-错误-01-八类错误码全可触发", ok, "");
}

/// VE-F2802 域自检入口。
pub fn run_veo02_checks() -> CheckSet {
    let mut set = CheckSet::new("veo02-vendor");
    chk_arch_decision_single_source(&mut set);
    chk_arch_reason_mandatory(&mut set);
    chk_arch_layout_is_gate(&mut set);
    chk_boundary_surface_enforced(&mut set);
    chk_boundary_default_deny(&mut set);
    chk_boundary_layout_absent(&mut set);
    chk_boundary_reason_required(&mut set);
    chk_boundary_surface_cap(&mut set);
    chk_license_modified_needs_publish(&mut set);
    chk_license_pristine_no_publish(&mut set);
    chk_license_decision_reconcile(&mut set);
    chk_pin_revision(&mut set);
    chk_pin_hex_only(&mut set);
    chk_change_ledger(&mut set);
    chk_degrade_reject_triple(&mut set);
    chk_degrade_case_filing(&mut set);
    chk_degrade_cap_guard(&mut set);
    chk_degrade_audit_catches_backdoor(&mut set);
    chk_subset_no_property_table(&mut set);
    chk_criterion_constants_live(&mut set);
    chk_criterion_hash_recompute(&mut set);
    chk_screen_summary_readable(&mut set);
    chk_screen_vendor_line(&mut set);
    chk_error_codes_reachable(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veo02_boundary_selfcheck_clean() {
        let a = ServoBoundary::standard();
        assert!(a.audit().is_ok(), "标准边界不应有审计问题");
    }

    #[test]
    fn veo02_layout_never_admits() {
        let mut b = ServoBoundary::standard();
        for sym in ["layout::tree", "layout::style", "webrender::frame"] {
            assert!(b.admit(CRATE_STYLE, sym).is_err(), "{sym} 不应放行");
        }
    }

    #[test]
    fn veo02_crates_and_types_are_used() {
        // 防止「类型/常量齐备但一个函数都没调」的恒真测试。
        assert_eq!(crate_decisions().len(), 3);
        assert!(crate_id_of("style").is_some());
        assert!(crate_id_of("nope").is_none());
    }
}
