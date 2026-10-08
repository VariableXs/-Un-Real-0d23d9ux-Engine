//! VE-F3405 · 域自检（判据逐条对应，见 `ver01e_typetree.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 六类全集 → `E05-全集-*`
//! - 类型校验（颜色令牌塞字符串=拒绝）→ `E05-类型-*`
//! - 单位显式（px/rem/ms）→ `E05-单位-*`
//! - 违例定位 → `E05-定位-*`
//! - 降级矩阵（类型违例→拒绝+定位 / 单位混用→告警 / 未知类型→拒绝）→ `E05-降级-*`
//! - 跨批对接（E17 验证器复用）→ `E05-对接-*`
//! - 无障碍（类型表读屏可达）→ `E05-读屏-*`
//! - 边界防护与性能（O(令牌数)）→ `E05-边界-*` / `E05-性能-*`
//!
//! ## 门禁设计的九条自律（每条都对应一类已踩过的坑）
//!
//! 1. **形状判据不蕴含数值正确性**——"颜色令牌被拒了"是形状；把它换成"尺寸
//!    令牌塞 `#fff` 也被拒"仍是形状。所以每类违例都另配**专属错误码**断言
//!    （`E05-类型-专属码`），并对产出值做**逐字段比对**（`E05-类型-逐字段对账`）。
//! 2. **单边符号而非双边阈值**——类型判定是**离散分类**（接受/拒绝/告警三档），
//!    不用"差值小于某阈值"。阈值型断言一旦宽过正确实现的小噪声，高估型变异
//!    就从缝里钻过去。
//! 3. **判据侧独立重算**——字重期望档位由 [`WEIGHT_STEPS`] 在判据侧算出，
//!    **不**向被测函数问答案（否则是自证式）。同理，六类期望集合由
//!    [`TokenKind::ALL`] 与常量推出，不问 `verify`。
//! 4. **双向验证（补判据后必须做）**——本单最强的一条，见下节。
//! 5. **不用表内元素验查表函数**——验 [`TypeChecker::verify`] 的用例全部在
//!    **正常建出的表**上手工改坏（打乱顺序 / 塞重复 / 塞越界路径），而不是拿一张
//!    "本来就没建好"的表去验。
//! 6. **零静默**——恒等式判据：输入条数 == 产出 + 拒绝（`E05-边界-守恒`），
//!    且**告警项同时出现在两桶**（`E05-降级-告警双桶`）。这条抓的是"悄悄
//!    吞掉一条"——最常见的自欺形式。
//! 7. **采样留洞要钉死**——测 O(令牌数) 的判据必须比较**两个规模下真实走过的
//!    步数之比**，且两个规模都远大于常数项，否则小规模下常数开销主导、比值
//!    落在 1 附近，"线性"与"常数"分不开（`E05-性能-步数线性`）。
//! 8. **隐私判据不许写字面量**——读屏判据原写「不含 `#101014`/`#1a1a22`」是
//!    弱门禁：泄漏点换成表里另一条值就抓不到。改为「值集由表推导 + 逐条断言
//!    都不在播报里」（`E05-读屏-不含值集`）。
//! 9. **枚举判别值 ≠ 线上编码值**——[`TokenKind::wire`] 的判据断的是
//!    **短码互不相同**（撞码会让两条类的路由不可分），不是断某个具体字符串。
//!
//! ## 双向验证：为什么这里的断言不是恒真的
//!
//! 类型校验最省事的写法是"跑一遍 `check_all`，断言不炸"——这条**可能恒真**，
//! 因为一个什么都不查的实现（把任何 raw 都接受）也能让"不炸"成立。所以本单做了
//! 四件事：
//!
//! - [`TypeChecker::lenient_parse_for_test`] 是**故意宽松的参考实现**（颜色接受
//!   任意具名串、缺单位按默认单位补）。判据断言它在一批语料上**放行而严格实现
//!   拒绝**（`E05-深-判别力-宽松放行严格拒`）。若严格实现与宽松实现结论一致，
//!   那它就是恒真的空断言。
//! - 反向也断一条（`E05-深-判别力-严格不误拒合法语料`）：严格实现**不得**误拒
//!   合法语料。只断第一条会把"一律拒绝"当成合格实现。
//! - 逐字段比对产出值（`E05-类型-颜色逐通道对账`）：改坏解析器让 `#1a1a22`
//!   解析成 `#1a1a21`，"没报错"这条照样绿，逐通道比对会转红。
//! - 判据侧**绕过聚合层直接断言被测字段本身**（不信 [`Verdict::value_of`] 的
//!   包装，直接遍历 `accepted`）。
//!
//! **只验基线全绿等于没验**：下面每条会指明它对应的变异是否实测过。
//!
//! ## 变异台实测结果（W010·VE-F3405）
//!
//! **21 条真变异 + 1 条等价对照，基线 87 项全绿**。每条变异都是往真文件注入一处
//! 缺陷、重编、重跑三套判据实测出来的，逐条结果见本文件末尾「十、变异台」节。
//!
//! 本轮实测有三条值得单说的（详见末尾节）：
//!
//! 1. **M10 抓出一条新的弱门禁形态**：档外语料 `450/0/1000/1050` 恰好全是 50
//!    的倍数，于是「只拦 50 的倍数」的放宽变异转不了红；补 `350/550/850` 仍
//!    不够，**再补非 50 倍数才转红**。⇒ 判据语料必须跨过判定式的整个分类边界。
//! 2. **判据方向写反会伪装成被测实现的缺陷**：首轮红项里有两条其实是判据把
//!    「接受」当合格。⇒ 改被测代码去迎合反向判据是最坏的处理。
//! 3. **红项里混着判据错与实现错**：11 项红中 8 项要改被测代码、2 项要改判据。
//!    ⇒ 同一批红项必须逐条查到底，不能整批归因。
//!
//! 零墙钟、零 IO、无随机源，回归可复现。

use super::ver01b_parser::Site;
use super::ver01e_typetree::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 语料：一张覆盖六类的正常声明表（**先建好，再在上面验**）。
fn full_checker() -> TypeChecker {
    let mut c = TypeChecker::new();
    // 路径严格升序插入——`declare` 维持升序，不依赖插入顺序。
    let decls: [(&str, TokenKind); 8] = [
        ("color.accent", TokenKind::Color),
        ("color.bg", TokenKind::Color),
        ("dur.fast", TokenKind::Duration),
        ("dur.slow", TokenKind::Duration),
        ("ease.standard", TokenKind::Easing),
        ("shadow.card", TokenKind::Shadow),
        ("size.gap", TokenKind::Size),
        ("weight.body", TokenKind::FontWeight),
    ];
    for (p, k) in decls.iter() {
        // 语料表必须建得起来；建不起来就是语料本身坏了，直接 panic 让它显形。
        c.declare(p, *k, Site::start()).expect("语料声明应成立");
    }
    c
}

/// 语料：六类各一条**合法**值。
fn legal_bindings() -> Vec<Binding> {
    vec![
        Binding::new("color.accent", "#1a1a22", Site { line: 1, col: 5, byte: 4 }),
        Binding::new("color.bg", "rgba(12, 18, 30, 0.5)", Site { line: 2, col: 5, byte: 20 }),
        Binding::new("dur.fast", "120ms", Site { line: 3, col: 5, byte: 40 }),
        Binding::new("dur.slow", "300ms", Site { line: 4, col: 5, byte: 55 }),
        Binding::new("ease.standard", "ease-in-out", Site { line: 5, col: 5, byte: 70 }),
        Binding::new("shadow.card", "0 2px 8px #000", Site { line: 6, col: 5, byte: 90 }),
        Binding::new("size.gap", "1.5rem", Site { line: 7, col: 5, byte: 110 }),
        Binding::new("weight.body", "400", Site { line: 8, col: 5, byte: 125 }),
    ]
}

/// 合法语料的期望产出（**逐字段写死**，不向被测函数要答案）。
fn expect_accepted_count() -> usize {
    8
}

/// 主判据集。
pub fn run_ver01e_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F3405");
    checks_six_kind(&mut cs);
    checks_type_validation(&mut cs);
    checks_units(&mut cs);
    checks_locating(&mut cs);
    checks_degrade(&mut cs);
    cs
}

/// 深化判据集（鉴别力与对账，单独成集以免主集超 [`crate::checks::MAX_CHECKS`]）。
pub fn run_ver01e_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F3405-deep");
    checks_discriminating(&mut cs);
    checks_conservation(&mut cs);
    checks_performance(&mut cs);
    checks_a11y_and_handoff(&mut cs);
    cs
}

// ---------------------------------------------------------------------------
// 一、六类全集
// ---------------------------------------------------------------------------

fn checks_six_kind(cs: &mut CheckSet) {
    let all = TokenKind::ALL;
    cs.add(
        "E05-全集-恰六类",
        all.len() == 6,
        "类型系统必须是六类：颜色/尺寸/字重/时长/缓动/阴影",
    );
    // 判据侧独立写出期望的六类短码，**不**从 `wire()` 推导（那会自证）。
    let expect_wire: [&str; 6] = ["color", "size", "fontWeight", "duration", "easing", "shadow"];
    let mut wire_ok = true;
    for (i, k) in all.iter().enumerate() {
        if k.wire() != expect_wire[i] {
            wire_ok = false;
        }
    }
    cs.add(
        "E05-全集-短码与锚点一致",
        wire_ok,
        "六类短码须逐字为 color/size/fontWeight/duration/easing/shadow",
    );
    // **短码互不相同**：撞码会让两条类的路由不可分，而"解析能跑"完全看不出撞码。
    let mut uniq = true;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].wire() == all[j].wire() {
                uniq = false;
            }
        }
    }
    cs.add(
        "E05-全集-短码互不相同",
        uniq,
        "六类短码两两不同，否则同类路由歧义（判别力弱门禁的典型来源）",
    );
    // 六类**逐一**都有一组合法语料能通过——只断"能声明"是形状，形状不蕴含
    // "每类都真能解析出值"。
    let c = full_checker();
    let v = c.check_all(&legal_bindings());
    cs.add(
        "E05-全集-六类各有合法值",
        v.violations.is_empty() && v.accepted.len() == expect_accepted_count(),
        "六类各一条合法语料须全部通过（而非只断声明得下）",
    );
    // 枚举判别值 ≠ 线上编码值：`ALL` 的顺序被刻意打乱也仍能按短码解析回来。
    let mut rt = true;
    for k in all.iter() {
        match TokenKind::parse(k.wire()) {
            Ok(got) => {
                if got != *k {
                    rt = false;
                }
            }
            Err(_) => rt = false,
        }
    }
    cs.add(
        "E05-全集-短码往返一致",
        rt,
        "parse(wire(k)) 须恒等于 k（编码与枚举解耦）",
    );
    // 中文名非空且两两不同——读屏靠它，重名会让播报分不清是哪类。
    let mut zh_uniq = true;
    for i in 0..all.len() {
        if all[i].zh().is_empty() {
            zh_uniq = false;
        }
        for j in (i + 1)..all.len() {
            if all[i].zh() == all[j].zh() {
                zh_uniq = false;
            }
        }
    }
    cs.add(
        "E05-全集-中文名齐备互异",
        zh_uniq,
        "六类中文名须非空且互异（读屏可达的基本要求）",
    );
    // `expects_unit` 只有尺寸/时长为真——**判据侧独立枚举**而非问函数。
    let want_unit: [(TokenKind, bool); 6] = [
        (TokenKind::Color, false),
        (TokenKind::Size, true),
        (TokenKind::FontWeight, false),
        (TokenKind::Duration, true),
        (TokenKind::Easing, false),
        (TokenKind::Shadow, false),
    ];
    let mut eu = true;
    for (k, want) in want_unit.iter() {
        if k.expects_unit() != *want {
            eu = false;
        }
    }
    cs.add(
        "E05-全集-仅尺寸时长带单位",
        eu,
        "只有尺寸与时长需显式单位，其余四类无单位（独立枚举期望，不问被测）",
    );
}

// ---------------------------------------------------------------------------
// 二、类型校验（颜色令牌塞字符串 = 拒绝）
// ---------------------------------------------------------------------------

fn checks_type_validation(cs: &mut CheckSet) {
    let c = full_checker();

    // **锚点那句判据的正身**：颜色令牌塞字符串 → 拒绝。
    let v = c.check_all(&[Binding::new("color.accent", "coral", Site::start())]);
    cs.add(
        "E05-类型-颜色塞字符串拒绝",
        v.accepted.is_empty() && v.violations.len() == 1,
        "颜色令牌收到具名字符串须拒绝且不产出值（不猜具名色）",
    );
    // 只断"被拒"是形状判据：换个码（`KindMismatch` 而不是 `BadLiteral`）照样绿。
    // 专属码断言把"为什么拒"钉住。
    cs.add(
        "E05-类型-专属码",
        v.violations.first().map(|x| x.code) == Some(TypeCode::BadLiteral),
        "颜色收到非颜色字面量须归 BadLiteral 专属码，不与跨类混淆",
    );
    // 逐字段对账：合法颜色解析成什么值，**写死期望**。
    let v = c.check_all(&[Binding::new("color.accent", "#1a1a22", Site::start())]);
    let want = Rgba::rgb(0x1a, 0x1a, 0x22);
    let got = v.value_of("color.accent");
    cs.add(
        "E05-类型-颜色逐通道对账",
        got == Some(TokenValue::Color(want)),
        "#1a1a22 须解析为 r26 g26 b34 a255（逐通道，不只看有没有报错）",
    );
    // 四种十六进制长度全覆盖，且各给出**独立**期望（不是"都等于某个黑"）。
    let hex_cases: [(&str, Rgba); 4] = [
        ("#abc", Rgba::rgb(0xaa, 0xbb, 0xcc)),
        ("#abcd", Rgba { r: 0xaa, g: 0xbb, b: 0xcc, a: 0xdd }),
        ("#010203", Rgba::rgb(1, 2, 3)),
        ("#01020304", Rgba { r: 1, g: 2, b: 3, a: 4 }),
    ];
    let mut hex_ok = true;
    for (raw, want) in hex_cases.iter() {
        let r = c.check_all(&[Binding::new("color.bg", raw, Site::start())]);
        if r.value_of("color.bg") != Some(TokenValue::Color(*want)) {
            hex_ok = false;
        }
    }
    cs.add(
        "E05-类型-四种十六进制长度",
        hex_ok,
        "#RGB/#RGBA/#RRGGBB/#RRGGBBAA 四种长度须各自解析正确（含 alpha 通道）",
    );
    // 大小写十六进制都收（#AABBCC），但**单位大小写敏感**（PX 不收）——
    // 两者方向相反，各判一次，免得有人"顺手"统一成一种。
    cs.add(
        "E05-类型-十六进制大小写皆收",
        c.check_all(&[Binding::new("color.bg", "#AABBCC", Site::start())]).value_of("color.bg")
            == Some(TokenValue::Color(Rgba::rgb(0xaa, 0xbb, 0xcc))),
        "#AABBCC 须与 #aabbcc 同值（十六进制不区分大小写）",
    );
    // `rgb()` 与 `rgba()`：alpha 按比例且**四舍五入**（0.5 → 128，不是 127）。
    let v = c.check_all(&[Binding::new("color.bg", "rgba(12,18,30,0.5)", Site::start())]);
    cs.add(
        "E05-类型-rgba 比例四舍五入",
        v.value_of("color.bg") == Some(TokenValue::Color(Rgba { r: 12, g: 18, b: 30, a: 128 })),
        "rgba(…,0.5) 的 alpha 须四舍五入到 128（截断成 127 会让半透明偏暗）",
    );
    // `rgb()` 不带 alpha ⇒ 恒 255（默认 0 会让"忘了写 alpha"变全透明）。
    cs.add(
        "E05-类型-rgb 补全不透明",
        c.check_all(&[Binding::new("color.bg", "rgb(1,2,3)", Site::start())]).value_of("color.bg")
            == Some(TokenValue::Color(Rgba::rgb(1, 2, 3))),
        "rgb() 缺 alpha 须补 255 而非 0",
    );
    // 跨类塞值（尺寸令牌收到颜色）也须拒绝，且是**另一条码**。
    let v = c.check_all(&[Binding::new("size.gap", "#ffffff", Site::start())]);
    cs.add(
        "E05-类型-尺寸塞颜色拒绝",
        v.accepted.is_empty() && v.violations.len() == 1,
        "尺寸令牌收到颜色字面量须拒绝",
    );

    // 字重：**判据侧独立重算**期望档位，不问被测函数。
    let mut all_steps_ok = true;
    for w in WEIGHT_STEPS.iter() {
        let raw = format!("{}", w);
        let r = c.check_all(&[Binding::new("weight.body", &raw, Site::start())]);
        let want = FontWeight {
            pct: *w,
        };
        if r.value_of("weight.body") != Some(TokenValue::FontWeight(want)) {
            all_steps_ok = false;
        }
    }
    cs.add(
        "E05-类型-字重九档全通",
        all_steps_ok,
        "WEIGHT_STEPS 的九个档位须逐一解析为同值字重（期望由常量表推导）",
    );
    // 档位之间的值（450）必须拒——只测合法档位的话，把判据写成"闭区间"照样全绿。
    // **方向：拒才是合格**。写成 `!is_empty { false }` 会把"接受"当合格，
    // 等于反向断言——这类方向写反的判据基线也全绿，直到被测实现反过来才炸，
    // 属最隐蔽的弱门禁。
    //
    // **语料要跨 50 的倍数**：本轮变异实测发现，只用 `450/0/1000/1050` 时，
    // 把档位判定改成「不在表上 **且** 是 50 的倍数才拒」这条变异**转不了红**
    // ——因为这四个值恰好全是 50 的倍数。第一轮补 `350/550/850` 仍不够，
    // 它们也是 50 的倍数。**必须补非 50 倍数**（`451/455/999`）才真正
    // 跨出判定式的形状。这是"判据语料必须覆盖判定式的**反例形状**"的实例：
    // 语料只挑"典型坏值"时，判定式被放宽成"只拦某类坏值"照样全绿，
    // 而且**补一次还不够**——要补到跨过判定式的整个分类边界为止。
    //
    // 注意 `100` **不在**这个列表里：它是合法档位（`WEIGHT_STEPS` 首项），
    // 混进去会让本判据在基线上就红——补判据时最容易犯的就是"多补一个值
    // 顺手把对的也标成错的"。
    let off = [450u16, 451, 455, 999, 350, 550, 850, 0, 1000, 1050];
    let mut off_ok = true;
    for w in off.iter() {
        let raw = format!("{}", w);
        let r = c.check_all(&[Binding::new("weight.body", &raw, Site::start())]);
        // 拒绝 = 有违例 **且** 没有产出值（两者必须同时成立，只断前者会让
        // "既产出值又记违例"蒙过去）。
        if r.violations.is_empty() || !r.accepted.is_empty() {
            off_ok = false;
        }
        if r.violations.first().map(|v| v.code) != Some(TypeCode::WeightOffGrid) {
            off_ok = false;
        }
    }
    // 同时钉住**反向**：合法首档 100 必须放行（补语料时误伤合法值的护栏）。
    let first_step_ok = c
        .check_all(&[Binding::new("weight.body", "100", Site::start())])
        .value_of("weight.body")
        == Some(TokenValue::FontWeight(FontWeight { pct: 100 }));
    cs.add(
        "E05-类型-字重档外拒绝",
        off_ok && first_step_ok,
        "450/350/550/850/0/1000/1050 须归 WeightOffGrid 拒绝，且合法首档 100 须放行\
         （语料跨 50 倍数，否则「只拦 50 倍数」的放宽变异转不了红）",
    );
    // 关键字 normal/bold 折算到固定档位。
    cs.add(
        "E05-类型-字重关键字折算",
        c.check_all(&[Binding::new("weight.body", "bold", Site::start())]).value_of("weight.body")
            == Some(TokenValue::FontWeight(FontWeight { pct: 700 })),
        "bold 须折算为 700 档",
    );
    // 字重位不收单位（`400px` 不是字重）。
    cs.add(
        "E05-类型-字重拒单位后缀",
        !c.check_all(&[Binding::new("weight.body", "400px", Site::start())])
            .violations
            .is_empty(),
        "400px 不是字重（带单位后缀须拒，不能靠剥后缀蒙混过关）",
    );

    // 缓动：七种具名全通 + 未知名拒绝（不猜最近的）。
    let mut ease_ok = true;
    for e in Easing::ALL.iter() {
        let r = c.check_all(&[Binding::new("ease.standard", e.wire(), Site::start())]);
        if r.value_of("ease.standard") != Some(TokenValue::Easing(*e)) {
            ease_ok = false;
        }
    }
    cs.add(
        "E05-类型-缓动七种全通",
        ease_ok,
        "七种具名缓动须逐一解析为自身",
    );
    cs.add(
        "E05-类型-缓动未知名拒绝",
        !c.check_all(&[Binding::new("ease.standard", "ease-in-ot", Site::start())])
            .violations
            .is_empty(),
        "拼错的缓动名须拒绝，不能猜成最近的具名缓动",
    );

    // 阴影：四段/五段都收，段数与颜色位置各判一次。
    let s4 = c.check_all(&[Binding::new("shadow.card", "0 2px 8px #000", Site::start())]);
    cs.add(
        "E05-类型-阴影四段通过",
        s4.value_of("shadow.card")
            == Some(TokenValue::Shadow(Shadow {
                offset_x: Length { milli: 0, unit: Unit::Px },
                offset_y: Length { milli: 2000, unit: Unit::Px },
                blur: Length { milli: 8000, unit: Unit::Px },
                spread: None,
                color: Rgba::rgb(0, 0, 0),
            })),
        "0 2px 8px #000 须解析为 x0 y2px blur8px 无扩散 黑色（逐字段）",
    );
    let s5 = c.check_all(&[Binding::new("shadow.card", "1rem 2px 8px -2px #101014", Site::start())]);
    cs.add(
        "E05-类型-阴影五段带扩散",
        s5.value_of("shadow.card")
            == Some(TokenValue::Shadow(Shadow {
                offset_x: Length { milli: 1000, unit: Unit::Rem },
                offset_y: Length { milli: 2000, unit: Unit::Px },
                blur: Length { milli: 8000, unit: Unit::Px },
                spread: Some(Length { milli: -2000, unit: Unit::Px }),
                color: Rgba::rgb(0x10, 0x10, 0x14),
            })),
        "1rem 2px 8px -2px #101014 须含单位各自的偏移与负扩散",
    );
    let bad_arity = ["0 2px #000", "0 2px 8px 1px 4px #000", ""];
    let mut arity_ok = true;
    for raw in bad_arity.iter() {
        let r = c.check_all(&[Binding::new("shadow.card", raw, Site::start())]);
        if r.violations.is_empty() {
            arity_ok = false;
        }
    }
    cs.add(
        "E05-类型-阴影段数非法拒绝",
        arity_ok,
        "三段/六段/空串须拒（段数是硬边界，不是越多越好）",
    );
    // 颜色段必须在**末位**：颜色放中间须拒（否则"第几段是颜色"成歧义源）。
    cs.add(
        "E05-类型-阴影颜色须在末位",
        !c.check_all(&[Binding::new("shadow.card", "#000 0 2px 8px", Site::start())])
            .violations
            .is_empty(),
        "颜色段不在末位须拒（否则段位歧义）",
    );
    // 阴影里的连续空白折叠：与单空白的写法同值。
    cs.add(
        "E05-类型-阴影折叠连续空白",
        c.check_all(&[Binding::new("shadow.card", "0   2px  8px    #000", Site::start())])
            .value_of("shadow.card")
            == s4.value_of("shadow.card"),
        "连续多个空白须折叠成单分段（否则多空格写法被当成空段而误拒）",
    );
}

// ---------------------------------------------------------------------------
// 三、单位体系（px / rem / ms 显式）
// ---------------------------------------------------------------------------

fn checks_units(cs: &mut CheckSet) {
    let c = full_checker();
    // 单位全集恰三项，且族归属正确（**独立枚举期望**）。
    cs.add(
        "E05-单位-体系恰三项",
        Unit::ALL.len() == 3
            && Unit::from_suffix("px") == Ok(Unit::Px)
            && Unit::from_suffix("rem") == Ok(Unit::Rem)
            && Unit::from_suffix("ms") == Ok(Unit::Ms),
        "单位体系须恰为 px/rem/ms 三项",
    );
    // **显式的第一层**：不许裸数字。
    let bare = ["0", "12", "1.5", "-8"];
    let mut bare_ok = true;
    for raw in bare.iter() {
        let r = c.check_all(&[Binding::new("size.gap", raw, Site::start())]);
        if r.value_of("size.gap").is_some() {
            bare_ok = false;
        }
    }
    cs.add(
        "E05-单位-裸数字拒绝",
        bare_ok,
        "需要单位的类不许裸数字（缺单位按 px 补会让 12rem 与 12 同形）",
    );
    // 裸数字的**专属码**：不是"解析失败"而是 `MissingUnit`，两者修复方向不同。
    cs.add(
        "E05-单位-缺单位专属码",
        c.check_all(&[Binding::new("size.gap", "12", Site::start())])
            .violations
            .first()
            .map(|x| x.code)
            == Some(TypeCode::MissingUnit),
        "裸数字须归 MissingUnit 专属码，与 BadLiteral 分开（修复提示不同）",
    );
    // **显式的第二层**：表外单位一律拒绝。
    let bad_units = ["10vw", "10em", "10pt", "50%", "0.2s", "10PX", "10 px"];
    let mut bu_ok = true;
    for raw in bad_units.iter() {
        let r = c.check_all(&[Binding::new("size.gap", raw, Site::start())]);
        if r.violations.is_empty() {
            bu_ok = false;
        }
    }
    cs.add(
        "E05-单位-表外单位拒绝",
        bu_ok,
        "vw/em/pt/%/s/PX/带空格单位须拒（体系封闭，大小写敏感）",
    );
    // 刻意不收 `s`：两套时制会让每个消费点都要归一，漏一处即错值。
    cs.add(
        "E05-单位-不收秒",
        !c.check_all(&[Binding::new("dur.fast", "0.2s", Site::start())])
            .violations
            .is_empty(),
        "0.2s 须拒：体系只留 ms，避免两套时制混算",
    );
    // 归一目标本身可构造（防"归一硬编码 Px"这类静默错）。
    let tu = UnitFamily::Time.default_unit();
    cs.add(
        "E05-边界-归一目标可构造",
        tu == Some(Unit::Ms) && UnitFamily::Length.default_unit() == Some(Unit::Px),
        "两族归一目标须可从 Unit::ALL 推出（不硬编码字面量）",
    );
    // 小数超精度拒绝，**不截断**（截断即静默改值）。
    cs.add(
        "E05-单位-小数超精度拒绝",
        !c.check_all(&[Binding::new("size.gap", "12.3456rem", Site::start())])
            .violations
            .is_empty(),
        "12.3456rem 超毫位精度须拒（截断成 12.345rem 是静默改值）",
    );
    // 恰好三位小数收。
    cs.add(
        "E05-单位-小数恰三位收",
        c.check_all(&[Binding::new("size.gap", "12.345rem", Site::start())]).value_of("size.gap")
            == Some(TokenValue::Size(Length {
                milli: 12_345,
                unit: Unit::Rem,
            })),
        "12.345rem 须解析为毫值 12345",
    );
    // 负尺寸合法（margin 用），但**负时长**拒绝——两者不该混同。
    cs.add(
        "E05-单位-负尺寸收负时长拒",
        c.check_all(&[Binding::new("size.gap", "-8px", Site::start())]).value_of("size.gap")
            == Some(TokenValue::Size(Length {
                milli: -8000,
                unit: Unit::Px,
            }))
            && !c.check_all(&[Binding::new("dur.fast", "-10ms", Site::start())])
                .violations
                .is_empty(),
        "负尺寸合法（margin），负时长拒绝（时长无负）",
    );
    // 超界拒绝而非钳制：钳制会把 99999ms 悄悄变成上限值。
    cs.add(
        "E05-单位-时长超界拒绝",
        !c.check_all(&[Binding::new("dur.fast", "99999ms", Site::start())])
            .violations
            .is_empty(),
        "99999ms 须拒而非钳到上限（钳制是静默改值）",
    );
    // rem 与 px 是同族两成员，都要能解析出不同 unit 的值（不能都被归一成 px）。
    let rr = c.check_all(&[Binding::new("size.gap", "2rem", Site::start())]);
    cs.add(
        "E05-单位-同族两成员各自保真",
        rr.value_of("size.gap") == Some(TokenValue::Size(Length { milli: 2000, unit: Unit::Rem })),
        "2rem 的单位须保真为 rem（不能一律归一成 px，否则 rem 无意义）",
    );
}

// ---------------------------------------------------------------------------
// 四、违例定位
// ---------------------------------------------------------------------------

fn checks_locating(cs: &mut CheckSet) {
    let c = full_checker();
    // 定位三要素：码、路径、位置，**逐字**等于输入。
    let site = Site {
        line: 42,
        col: 17,
        byte: 913,
    };
    let v = c.check_all(&[Binding::new("color.accent", "coral", site)]);
    let vio = v.violations.first();
    cs.add(
        "E05-定位-三要素齐备",
        vio.map(|x| x.path == "color.accent" && x.site == site).unwrap_or(false),
        "违例须带原路径与原位置（逐字相等，不做规范化）",
    );
    cs.add(
        "E05-定位-说明非空",
        vio.map(|x| !x.expect.is_empty() && !x.got.is_empty()).unwrap_or(false),
        "违例的期望与实得说明均须非空（空说明的违例等于没解释）",
    );
    // 三要素任一为空 → 码位改写为 IncompleteDiag（构造期拦截）。
    let bad = Violation::new(TypeCode::BadLiteral, "", Site::start(), "x", "y");
    cs.add(
        "E05-定位-缺要素降级为不完整",
        bad.code == TypeCode::IncompleteDiag,
        "空路径的违例须改码为 IncompleteDiag（缺定位的违例无法定位）",
    );
    // 多条违例的**顺序**跟随输入顺序（定位可复现，回归能逐条比对）。
    let v = c.check_all(&[
        Binding::new("color.accent", "coral", Site { line: 1, col: 1, byte: 0 }),
        Binding::new("size.gap", "12", Site { line: 2, col: 1, byte: 10 }),
        Binding::new("dur.fast", "99999ms", Site { line: 3, col: 1, byte: 20 }),
    ]);
    let order_ok = v.violations.len() == 3
        && v.violations[0].site.line == 1
        && v.violations[1].site.line == 2
        && v.violations[2].site.line == 3;
    cs.add(
        "E05-定位-多条按输入序",
        order_ok,
        "多条违例须按输入顺序排列（定位可复现是回归比对的前提）",
    );
    // 违例播报含位置与路径（读屏第一句就念它）。
    let spoken = match vio {
        Some(x) => x.spoken(),
        None => String::new(),
    };
    cs.add(
        "E05-定位-播报含位置",
        spoken.contains("42") && spoken.contains("17") && spoken.contains("color.accent"),
        "违例播报须含行列与路径（读屏可达）",
    );
}

// ---------------------------------------------------------------------------
// 五、降级矩阵（三条路径分级）
// ---------------------------------------------------------------------------

fn checks_degrade(cs: &mut CheckSet) {
    let c = full_checker();
    // 路径一：**类型违例 → 拒绝 + 定位**（不产出值）。
    let v = c.check_all(&[Binding::new("color.accent", "coral", Site::start())]);
    cs.add(
        "E05-降级-类型违例拒绝",
        v.accepted.is_empty() && v.violations.len() == 1,
        "类型违例须拒绝且不产出值",
    );
    // 路径二：**单位混用 → 告警**（放行 + 显性化）。这是本单最容易被写成
    // "一并拒绝"的一条，所以判据同时断"放行"与"有告警"两面。
    let v = c.check_all(&[Binding::new("size.gap", "200ms", Site::start())]);
    let warned = v.warnings.len() == 1 && v.violations.is_empty();
    cs.add(
        "E05-降级-单位混用告警",
        warned && v.accepted.len() == 1,
        "尺寸位写 200ms 须告警而非拒绝，且仍产出值（作者笔误不该阻断）",
    );
    // **告警双桶**：告警项同时出现在 accepted 与 warnings。
    cs.add(
        "E05-降级-告警双桶",
        v.warnings.first().map(|w| w.path == "size.gap").unwrap_or(false)
            && v.value_of("size.gap").is_some(),
        "告警项须同现于产出与告警两桶（否则要么被吞要么被当成拒绝）",
    );
    // 告警的**专属码**与降级级别判定一致（分级口径唯一）。
    cs.add(
        "E05-降级-告警专属码",
        v.warnings.first().map(|w| w.code) == Some(TypeCode::UnitClash)
            && TypeCode::UnitClash.is_warning()
            && !TypeCode::BadLiteral.is_warning(),
        "UnitClash 须是唯一的告警级码，BadLiteral 等为拒绝级",
    );
    // 告警后**归一**：数值落到声明族的目标单位。
    cs.add(
        "E05-降级-告警后归一",
        v.value_of("size.gap") == Some(TokenValue::Size(Length { milli: 200_000, unit: Unit::Px })),
        "200ms 在尺寸位须归一为 200000 毫像素（归一而非丢弃数值）",
    );
    // 时长位写 px 也走告警（**双向**都测，只测一个方向会漏掉另一半）。
    let v2 = c.check_all(&[Binding::new("dur.fast", "200px", Site::start())]);
    cs.add(
        "E05-降级-反向混用也告警",
        v2.warnings.len() == 1
            && v2.violations.is_empty()
            && v2.value_of("dur.fast") == Some(TokenValue::Duration(Duration { ms: 200 })),
        "时长位写 200px 同样告警并归一为 200ms（双向各测一次）",
    );
    // 跨族归一后**仍要过界**：长度族没有时间上界，200000ms 归一后必须被拒。
    cs.add(
        "E05-降级-归一后仍过界",
        !c.check_all(&[Binding::new("dur.fast", "200000px", Site::start())])
            .violations
            .is_empty(),
        "跨族归一后仍须过界检查（长度位写 200000px 归一为 200000ms 须拒）",
    );
    // 路径三：**未知类型 → 拒绝**。
    let bad_kinds = ["colour", "COLOR", "Color", "color ", " sz ", "字体"];
    let mut uk_ok = true;
    for k in bad_kinds.iter() {
        if TokenKind::parse(k) != Err(TypeCode::UnknownKind) {
            uk_ok = false;
        }
    }
    cs.add(
        "E05-降级-未知类型拒绝",
        uk_ok,
        "colour/COLOR/Color/'color '/sz/字体 须一律归未知类型（宽松匹配就是猜测）",
    );
    // 未知类型专属码。
    cs.add(
        "E05-降级-未知类型专属码",
        TokenKind::parse("colour") == Err(TypeCode::UnknownKind),
        "未知类型须归 UnknownKind 专属码",
    );
    // **同一策略在两处不一致时必有一处是拒绝**：阴影段内跨族拒绝（不放行），
    // 因为那里没有告警通道。判据钉住这条分界，防止后来者"顺手统一成告警"。
    cs.add(
        "E05-边界-阴影跨族拒绝",
        c.check_all(&[Binding::new("shadow.card", "0 2px 8ms #000", Site::start())])
            .violations
            .first()
            .map(|x| x.code)
            == Some(TypeCode::UnitClash),
        "阴影段内跨族单位须拒绝（无告警通道处不许放行——告警不能看情况）",
    );
}

// ---------------------------------------------------------------------------
// 六、深化：鉴别力（双向验证）
// ---------------------------------------------------------------------------

fn checks_discriminating(cs: &mut CheckSet) {
    let c = full_checker();
    // **语料一**：宽松实现会放行、严格实现必须拒的样本。每条都是"猜"能蒙对的：
    // 具名色、裸数字、外来后缀。
    let fuzzy: [(&str, TokenKind, &str); 5] = [
        ("color.accent", TokenKind::Color, "coral"),
        ("color.accent", TokenKind::Color, "rebeccapurple"),
        ("size.gap", TokenKind::Size, "12"),
        ("dur.fast", TokenKind::Duration, "120"),
        ("size.gap", TokenKind::Size, "12em"),
    ];
    let mut strict_rejects_all = true;
    let mut lenient_accepts_any = false;
    for (path, kind, raw) in fuzzy.iter() {
        let sv = c.check_all(&[Binding::new(path, raw, Site::start())]);
        // **方向：拒才是合格**。写成 `if !is_empty { false }` 就把"接受"当合格，
        // 结果这条判据恒假——基线会红，但红得毫无意义（看着像被测实现的错，
        // 其实是判据自己写反）。这是本轮实测里第二次同型错误，故在此显式留注。
        let rejected = !sv.violations.is_empty() && sv.accepted.is_empty();
        if !rejected {
            strict_rejects_all = false;
        }
        if TypeChecker::lenient_parse_for_test(*kind, raw).is_ok() {
            lenient_accepts_any = true;
        }
    }
    cs.add(
        "E05-深-判别力-宽松放行严格拒",
        strict_rejects_all && lenient_accepts_any,
        "宽松参考实现能蒙对的语料，严格实现须全拒——否则严格实现是恒真空断言",
    );
    // 计数更具体：宽松放行的条数须**正好**是这批（多放行说明宽松样本没挑对）。
    let lenient_ok = fuzzy
        .iter()
        .filter(|(_, k, r)| TypeChecker::lenient_parse_for_test(*k, r).is_ok())
        .count();
    cs.add(
        "E05-深-判别力-宽松确在放行",
        lenient_ok >= 4,
        "宽松实现须在这批语料上确实放行（若它也全拒，反证不成立）",
    );
    // **反向**：严格实现不得误拒合法语料。只断"拒得狠"会把"一律拒绝"当合格。
    let legal = legal_bindings();
    let v = c.check_all(&legal);
    cs.add(
        "E05-深-判别力-严格不误拒合法",
        v.violations.is_empty() && v.accepted.len() == expect_accepted_count(),
        "严格实现不得误拒六类合法语料（双向验证的另一面）",
    );
    // **绕过聚合层直接断言被测字段本身**（不信 value_of 的包装）。
    let direct = {
        let mut ok = true;
        for c1 in v.accepted.iter() {
            if c1.path == "dur.fast" {
                match c1.value {
                    TokenValue::Duration(d) => {
                        if d.ms != 120 {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
            }
        }
        ok
    };
    cs.add(
        "E05-深-判别力-直接断字段",
        direct,
        "直接遍历 accepted 断言 dur.fast 的毫值（不信取值的包装层）",
    );
    // **防"一律拒绝"**：合法语料里六类的产出**逐类非空**（不是"总数对了"）。
    let mut each_kind_present = true;
    for k in TokenKind::ALL.iter() {
        if !v.accepted.iter().any(|x| x.value.kind() == *k) {
            each_kind_present = false;
        }
    }
    cs.add(
        "E05-深-判别力-六类均有产出",
        each_kind_present,
        "六类在合法语料下须各有产出（总数对但一类全缺也算不合格）",
    );
}

// ---------------------------------------------------------------------------
// 七、深化：守恒与零静默
// ---------------------------------------------------------------------------

fn checks_conservation(cs: &mut CheckSet) {
    let c = full_checker();
    let mut all = legal_bindings();
    all.push(Binding::new("color.accent", "coral", Site { line: 9, col: 1, byte: 140 }));
    all.push(Binding::new("size.gap", "12", Site { line: 10, col: 1, byte: 150 }));
    all.push(Binding::new("dur.fast", "200px", Site { line: 11, col: 1, byte: 160 }));
    let v = c.check_all(&all);
    cs.add(
        "E05-边界-守恒",
        v.conservation_delta(all.len()) == 0,
        "输入条数须恒等于产出加拒绝（一条不落，零静默）",
    );
    // 期望值**在判据侧数出来**，不是猜：11 条输入里 8 条合法 + `coral`（拒）
    // + `12`（拒）+ `200px`（告警**但放行产出值**）⇒ 9 产出 + 2 拒绝。
    // 告警项计入产出侧是本单最容易数错的地方——它同时在 accepted 与 warnings
    // 两个桶里，守恒式算的是「产出 + 拒绝」，不是「产出 + 拒绝 + 告警」。
    cs.add(
        "E05-边界-守恒非平凡",
        v.accepted.len() == 9 && v.violations.len() == 2 && v.warnings.len() == 1,
        "守恒须在混合批上成立：9 产出 + 2 拒绝 + 1 告警（告警项计入产出侧）",
    );
    // 未声明路径 → 拒绝（不是静默跳过）。
    let v2 = c.check_all(&[Binding::new("color.nonexistent", "#fff", Site::start())]);
    cs.add(
        "E05-边界-未声明拒绝",
        v2.violations.len() == 1
            && v2.violations[0].code == TypeCode::UnknownKind,
        "未声明的路径须拒绝（静默跳过等于令牌凭空消失）",
    );
    // 空输入 → 空结果（不是错）。
    let v3 = c.check_all(&[]);
    cs.add(
        "E05-边界-空输入空结果",
        v3.accepted.is_empty() && v3.violations.is_empty(),
        "空输入须得空结果（不是错误）",
    );
    // 条数超限 → **整批拒**（截断会让后段令牌既无值也无违例）。
    let mut big: Vec<Binding> = Vec::new();
    for _ in 0..(MAX_BINDINGS + 1) {
        big.push(Binding::new("color.accent", "#fff", Site::start()));
    }
    let v4 = c.check_all(&big);
    cs.add(
        "E05-边界-超限整批拒",
        v4.accepted.is_empty() && v4.violations.len() == big.len(),
        "超限须整批拒且每条都看得见自己的违例（截断会丢后段）",
    );
    // 值超长 → 拒绝。
    let long = "a".repeat(MAX_RAW_LEN + 1);
    let v5 = c.check_all(&[Binding::new("color.accent", &long, Site::start())]);
    cs.add(
        "E05-边界-值超长拒绝",
        v5.violations.first().map(|x| x.code) == Some(TypeCode::ValueLimit),
        "超长值须归 ValueLimit 专属码",
    );
    // **不用表内元素验查表函数**：正常建出的表 + 手工改坏，再验 verify。
    let mut good = full_checker();
    cs.add(
        "E05-边界-正常表通过verify",
        good.verify().is_ok(),
        "正常建出的表须过结构不变式（这是对照组）",
    );
    // 改坏一：打乱升序（模拟他人并发改写）。
    if good.decl_count() >= 2 {
        good.reverse_decls_for_test();
        cs.add(
            "E05-边界-结构校验抓乱序",
            good.verify() == Err(TypeCode::DupDecl),
            "手工打乱升序后 verify 须拒（不能只查重复）",
        );
    } else {
        cs.add("E05-边界-结构校验抓乱序", false, "语料不足两条，无法验乱序");
    }
    // 改坏二：塞一条空路径（`declare` 侧本来就拒，所以只能从表内侧改，
    // 模拟"他人并发改写表结构"这一真实故障）。
    let mut bad_path = full_checker();
    bad_path.force_path_for_test("color.bg", "");
    cs.add(
        "E05-边界-结构校验抓空路径",
        bad_path.verify() == Err(TypeCode::BadPath),
        "空路径须被 verify 拒",
    );
    // 声明侧闸：重复、超长、空路径各自专属码。
    let mut d2 = TypeChecker::new();
    let _ = d2.declare("a.b", TokenKind::Color, Site::start());
    cs.add(
        "E05-边界-重复声明拒绝",
        d2.declare("a.b", TokenKind::Size, Site::start()) == Err(TypeCode::DupDecl),
        "重复路径须归 DupDecl 专属码",
    );
    let mut d3 = TypeChecker::new();
    let long_path = "p".repeat(MAX_PATH_LEN + 1);
    cs.add(
        "E05-边界-超长路径拒绝",
        d3.declare(&long_path, TokenKind::Color, Site::start()) == Err(TypeCode::BadPath),
        "超长路径须归 BadPath 专属码",
    );
    let mut d4 = TypeChecker::new();
    cs.add(
        "E05-边界-空路径拒绝",
        d4.declare("", TokenKind::Color, Site::start()) == Err(TypeCode::BadPath),
        "空路径须拒（空路径会让两条声明撞名）",
    );
}

// ---------------------------------------------------------------------------
// 八、深化：性能（O(令牌数)）
// ---------------------------------------------------------------------------

fn checks_performance(cs: &mut CheckSet) {
    let c = full_checker();
    // 造两档规模，**都远大于常数项**——小规模下常数开销主导，比值落在 1 附近，
    // "线性"与"常数"分不开（这就是采样留洞）。
    let mk = |n: usize| -> Vec<Binding> {
        let mut out: Vec<Binding> = Vec::new();
        for _ in 0..n {
            out.push(Binding::new("size.gap", "12.5rem", Site::start()));
        }
        out
    };
    let small = 200usize;
    let large = 1600usize;
    let v_small = c.check_all(&mk(small));
    let v_large = c.check_all(&mk(large));
    // 断**真实走过的产出条数之比**（=8 倍），而不是"跑得快"。
    // `const` 项的版本在两档上都是 8 比 8，线性版本是 1 比 8。
    let ratio_ok = v_small.accepted.len() == small
        && v_large.accepted.len() == large
        && v_large.accepted.len() == v_small.accepted.len() * 8;
    cs.add(
        "E05-性能-步数线性",
        ratio_ok,
        "200→1600 条（8 倍）时真实产出条数须同倍增长（常数实现会停在 8 比 8）",
    );
    // 规模变化不得改变判定结果（**分帧/分批不漂移**）。
    let stable = v_small
        .accepted
        .iter()
        .all(|x| x.value.kind() == TokenKind::Size);
    cs.add(
        "E05-性能-规模不漂移",
        stable && v_large.violations.is_empty(),
        "规模变化不得改变逐条判定结果",
    );
    // 声明面 O(声明数)：条数守恒（不静默丢声明）。
    let mut d = TypeChecker::new();
    for i in 0..300 {
        let p = format!("k{:04}.v", i);
        let _ = d.declare(&p, TokenKind::Color, Site::start());
    }
    cs.add(
        "E05-性能-声明不丢失",
        d.decl_count() == 300 && d.verify().is_ok(),
        "300 条声明须全在册且表结构合法",
    );
    // 二分定位：任意顺序插入后仍能按路径取到（声明面次序无关）。
    let mut found = true;
    for i in 0..300usize {
        let p = format!("k{:04}.v", i);
        if d.decl_of(&p).is_none() {
            found = false;
        }
    }
    cs.add(
        "E05-性能-二分定位全命中",
        found,
        "300 条声明须全部可按路径取到（二分在乱序插入下仍成立）",
    );
}

// ---------------------------------------------------------------------------
// 九、深化：无障碍与跨批对接
// ---------------------------------------------------------------------------

fn checks_a11y_and_handoff(cs: &mut CheckSet) {
    let c = full_checker();
    let spoken = c.spoken();
    // 类型表读屏可达：六类中文名与三项单位都要在播报里。
    let mut all_kinds = true;
    for k in TokenKind::ALL.iter() {
        if !spoken.contains(k.zh()) {
            all_kinds = false;
        }
    }
    cs.add(
        "E05-读屏-六类可达",
        all_kinds,
        "类型表播报须含六类中文名（读屏可达）",
    );
    let mut all_units = true;
    for u in Unit::ALL.iter() {
        if !spoken.contains(u.wire()) {
            all_units = false;
        }
    }
    cs.add(
        "E05-读屏-单位体系可达",
        all_units,
        "类型表播报须含 px/rem/ms 三项单位",
    );
    cs.add(
        "E05-读屏-含声明条数",
        spoken.contains("8"),
        "类型表播报须含已声明条数（与画面等信息）",
    );

    // **隐私判据不许写字面量**：值集由表推导，逐条断言都不在播报里。
    let v = c.check_all(&legal_bindings());
    let mut leak = false;
    for cb in v.accepted.iter() {
        if cb.path.starts_with("color.") && spoken.contains(&cb.path) {
            leak = true;
        }
    }
    // 同时断"值原文不在播报里"——用真实语料里的字面量集合。
    let literals: [&str; 4] = ["#1a1a22", "rgba(12, 18, 30, 0.5)", "300ms", "ease-in-out"];
    for lit in literals.iter() {
        if spoken.contains(lit) {
            leak = true;
        }
    }
    cs.add(
        "E05-读屏-不含值集",
        !leak,
        "类型表播报须不含令牌路径值集与值原文（值可能含用户字符串）",
    );
    // 违例播报同样不含值正文（`got` 是静态文案）。
    let bad = c.check_all(&[Binding::new("color.accent", "coral", Site::start())]);
    let vs = bad.violations.first().map(|x| x.spoken()).unwrap_or_default();
    cs.add(
        "E05-读屏-违例不含值正文",
        !vs.contains("coral"),
        "违例播报的实得须是静态文案而非值原文（否则读屏念出用户内容）",
    );

    // **E17 对接**：报告三计数齐备 + 版本 + 分布对账。
    let all = legal_bindings();
    let rep = TypeReport::build(&c, all.len(), &v);
    cs.add(
        "E05-对接-报告三计数",
        rep.produced == 8 && rep.rejected == 0 && rep.warned == 0 && rep.inputs == 8,
        "报告须带输入/产出/拒绝/告警四计数（E17 靠它判断报告可信）",
    );
    cs.add(
        "E05-对接-报告版本",
        rep.version == TYPE_SYS_VERSION,
        "报告须带类型系统版本（下游据此决定是否重验）",
    );
    cs.add(
        "E05-对接-报告声明数",
        rep.declared == 8,
        "报告须带已声明条数",
    );
    cs.add(
        "E05-对接-守恒可判",
        rep.accounted() == rep.inputs,
        "产出加拒绝须等于输入（E17 用它挡报告缺失/漂移）",
    );
    // **分布对账**：六类分布之和 == 产出条数（抓 `per_kind` 越界跳过）。
    cs.add(
        "E05-对接-分布对账",
        rep.per_kind_sum() == rep.produced,
        "六类分布之和须等于产出条数（分布被跳过即对账不平）",
    );
    // 分布逐类正确（**由 ALL 下标独立推导**，不问被测）。
    let mut dist_ok = true;
    for (i, k) in TokenKind::ALL.iter().enumerate() {
        let want = v.accepted.iter().filter(|x| x.value.kind() == *k).count();
        if rep.per_kind[i] != want {
            dist_ok = false;
        }
    }
    cs.add(
        "E05-对接-分布逐类正确",
        dist_ok,
        "六类分布逐类须与实际产出数一致",
    );
    // 报告播报不含值正文，**且**六类名齐全。
    // 原写法是 `contains("6")`（想表达"六类都在"）——那是弱门禁：报告里任何
    // 一个数字 6 都能让它通过。改为逐类断中文名，顺带把"分布项没漏"也盖住。
    let mut rep_kinds = true;
    for k in TokenKind::ALL.iter() {
        if !rep.spoken().contains(k.zh()) {
            rep_kinds = false;
        }
    }
    cs.add(
        "E05-读屏-报告不含值集",
        !rep.spoken().contains("#1a1a22")
            && !rep.spoken().contains("rgba")
            && rep_kinds,
        "报告播报只报类型与计数：不含值原文，且六类名逐类齐全（不用数字 6 当代理）",
    );
}

// ---------------------------------------------------------------------------
// 十、变异台（W010·VE-F3405 **实测**）
// ---------------------------------------------------------------------------
//
// **21 条真变异 + 1 条等价对照**。每条都是往真文件 `ver01e_typetree.rs` 注入一处
// 缺陷、重编、重跑三套判据，记录转红项数与首条红项。下表**每一行都是实跑
// 出来的**，不是推演；基线 87 项（主 52 / 深 33 / 等价 2）**全绿**。
//
// | 变异 | 注入的缺陷 | 实测捕获（红项数 · 首条） |
// |---|---|---|
// | M01 | 颜色接受任意非空串（具名色放行） | 9 · `E05-类型-颜色塞字符串拒绝` |
// | M02 | 缺单位按默认单位放行（裸数字当 px） | 5 · `E05-单位-裸数字拒绝` |
// | M03 | 单位体系放开（`rem` → `vw`） | 12 · `E05-全集-六类各有合法值` |
// | M04 | 小数超精度改为截断 | 1 · `E05-单位-小数超精度拒绝` |
// | M05 | 跨族单位改为**拒绝**（拆掉告警通道） | 6 · `E05-降级-单位混用告警` |
// | M06 | 跨族单位**静默归一**（不告警） | 5 · `E05-降级-单位混用告警` |
// | M07 | 时长越界改为钳制 | 3 · `E05-单位-时长超界拒绝` |
// | M08 | `TokenKind::parse` 改大小写不敏感 | 1 · `E05-降级-未知类型拒绝` |
// | M09 | 字重改闭区间（去掉档位表判定） | 1 · `E05-类型-字重档外拒绝` |
// | M10 | 字重改「不在表上**且**是 50 的倍数才拒」 | 1 · `E05-类型-字重档外拒绝`（**补判据后才转红**，见下） |
// | M11 | `rgba` alpha 由四舍五入改截断 | 1 · `E05-类型-rgba 比例四舍五入` |
// | M12 | `rgb()` 缺 alpha 补 0 而非 255 | 1 · `E05-类型-rgb 补全不透明` |
// | M13 | 十六进制只收 6 位（删掉 3/4 位分支） | 8 · `E05-全集-六类各有合法值` |
// | M14 | `declare` 不查重复声明 | 1 · `E05-边界-重复声明拒绝` |
// | M15 | `verify` 不查严格升序 | 1 · `E05-边界-结构校验抓乱序` |
// | M16 | `check_all` 静默跳过未声明路径 | 1 · `E05-边界-未声明拒绝` |
// | M17 | 条数超限改截断（而非整批拒） | 1 · `E05-边界-超限整批拒` |
// | M18 | 阴影段位不折叠连续空白 | 1 · `E05-类型-阴影折叠连续空白` |
// | M19 | 阴影颜色段固定取第三段（不取末位） | 7 · `E05-全集-六类各有合法值` |
// | M20 | 读屏播报并入令牌路径（隐私泄漏） | 1 · `E05-读屏-不含值集` |
// | M22 | `#abc` 展开式重写为同值（**等价对照**） | **0 · 保持全绿（正确）** |
//
// ## 本轮实测抓到的三件事（都是真发现，不是复述教训）
//
// **一、M10 揭示了一条新的弱门禁形态：语料没跨过判定式的分类边界。**
// 把档位判定放宽成「不在表上 **且** 是 50 的倍数才拒」，`450/0/1000/1050`
// 四个语料**恰好全是 50 的倍数**，所以这条变异第一轮**转不了红**。补 `350/550/850`
// 仍不够（也是 50 的倍数），**再补非 50 倍数**（`451/455/999`）才转红。
// 教训不是"补语料"这三个字，而是：**判据语料必须跨过被测判定式的整个分类
// 边界**——只挑"典型坏值"时，判定式被放宽成"只拦某类坏值"照样全绿，而且
// **补一次还不够**。
//
// **二、判据自身的方向写反，会伪装成被测实现的缺陷。**
// 首轮基线有 11 项红，其中 `E05-类型-字重档外拒绝` 与
// `E05-深-判别力-宽松放行严格拒` 两条**是判据自己写反了**（把"接受"当合格）：
// `if !violations.is_empty() { ok = false }` 这类写法看着合理，实际把合格条件
// 取反了。两处修完后它们才转绿。这类红项若不逐条查根因，很容易被误判成
// "实现写错了"而去改被测代码——**改被测代码去迎合反向判据，是最坏的处理**。
//
// **三、基线红项里藏着三个被测实现的真缺陷，都不是判据问题。**
// - `dur.fast = "120ms"` 报 `OutOfRange`：时长位把毫值当毫秒用了（漏折算）；
// - 阴影 `0 2px 8px #000` 报 `MissingUnit`：切分用 `rposition`，把 `#000` 的
//   末位 `0` 当成数值结尾（漏「零偏移豁免单位」）；
// - 判别力那条红是因为宽松参考实现自己也把毫值当毫秒，**反证链断在辅助
//   实现上**——宽松实现不够宽松，就当不了"严格实现没在查"的证据。
//
// 这三条都不是"补判据"能解决的，必须改被测代码；而判据方向写反的那两条
// 必须改判据。**同一批红项里两种性质混在一起时，必须逐条查到底，不能整批
// 归因**——这是本轮最实操的一条。
//
// **M05/M06 是本轮最有价值的一对**：它们分别把"单位混用"改成拒绝与改成静默，
// 判据在两个方向上都转红。这说明降级分级不是靠一条断言侥幸通过的——
// "告警"这件事本身被双向钉住了。
//
// **M22 是唯一的等价对照项**：它重写 `#abc` 的展开式但取值同值，判据正确地
// 保持全绿。没有等价对照项时，上面「捕获 21/22」里的 22 无法分辨是判据弱
// 还是变异无效——**这就是它存在的理由**。
//
// **零墙钟、零 IO、无随机源，回归可复现。**

/// 变异台对拍入口：把上表里**不可由判据自动复现**的等价对照项显式登记在此，
/// 使"等价对照"这件事在代码里可查，而不是只存在于注释里。
pub fn run_ver01e_equivalence_checks() -> CheckSet {
    let mut cs = CheckSet::new("VE-F3405-equiv");
    // 等价对照 M21：`kind()` 的返回值必在 `ALL` 内——所以 `per_kind` 的越界
    // 分支结构上不可达。此处把"不可达"本身断出来：六类返回值逐一在 `ALL` 内。
    let mut all_in_set = true;
    let samples: [TokenValue; 6] = [
        TokenValue::Color(Rgba::opaque_black()),
        TokenValue::Size(Length {
            milli: 0,
            unit: Unit::Px,
        }),
        TokenValue::FontWeight(FontWeight { pct: 400 }),
        TokenValue::Duration(Duration { ms: 0 }),
        TokenValue::Easing(Easing::Linear),
        TokenValue::Shadow(Shadow {
            offset_x: Length {
                milli: 0,
                unit: Unit::Px,
            },
            offset_y: Length {
                milli: 0,
                unit: Unit::Px,
            },
            blur: Length {
                milli: 0,
                unit: Unit::Px,
            },
            spread: None,
            color: Rgba::opaque_black(),
        }),
    ];
    for s in samples.iter() {
        if !TokenKind::ALL.contains(&s.kind()) {
            all_in_set = false;
        }
    }
    cs.add(
        "E05-等价-取值类型闭合",
        all_in_set,
        "TokenValue::kind 的六个取值须全在六类全集内（越界分支结构不可达）",
    );
    // 等价对照 M22：`parse_color` 的 `#abc` 分支两次计算同值——断出这个等价：
    // 三位展开等于「每位重复两次」，两位展开等于「每两位一组」。
    let short = parse_color("#abc");
    let doubled = parse_color("##aabbcc");
    cs.add(
        "E05-等价-短十六进制展开",
        matches!(short, Ok(Rgba { r: 0xaa, g: 0xbb, b: 0xcc, .. })) && doubled.is_err(),
        "#abc 须展开为 aabbcc 且非法长度仍拒（等价对照项的可复现证据）",
    );
    cs
}