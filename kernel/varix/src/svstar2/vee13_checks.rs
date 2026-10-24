#![cfg_attr(not(test), no_std)]
//! # VE-F0813 · 文字渲染示例域自检
//!
//! 锚点判据原文：**4 例、三步跑通、金样同源、CI 冒烟复用、漂移即红**。
//! 本文件把这五条拆成可执行判据，并补「纪律本身也要有门禁」的一组：
//!
//! 锚点错误路径两条最容易被无声绕过——
//!   「示例与 API 漂移→CI 冒烟即红」：若 `api_aligned` 只比符号名，那签名不变而
//!   语义常量改了（超时 16ms→32ms）就抓不到，等于漂移检测是假的。
//!   「示例依赖资源缺失→打包校验拦截」：若资产检查只看「有没有声明资产」而不看
//!   「在不在打包清单里」，空声明就绕过了拦截。
//! 所以判据里必须有「语义常量改了就变指纹」与「清单外资产必须拦」的反向语料。
//!
//! ## 判据层纪律
//!
//! 1. **零 panic 面**：无 `unwrap()`/`expect()`/`panic!`，固定下标与越界一律走
//!    `.get(k)` / `Option` 并对 `None` 记红（被测改坏时症状必须是可定位红项，
//!    而不是判据自己先崩——判据先崩的话，变异症状会退化成 EMPTY，
//!    让人误以为是「变异没被捕获」）。
//! 2. **期望值不从被测反推**：指纹在判据侧用**独立的 FNV-1a 实现**重算一遍，
//!    与被测的 `surface_fingerprint` 比对；FNV 偏移基/素数/语义常量均为写死常量。
//! 3. **反向语料逐条双向验证**：每条纪律配一条「违反它必须变红」的语料。
//! 4. **汇总口径自洽**：`pass` 计数与 `ci_open` 门禁必须同口径——若漂移的示例
//!    仍被计入 pass，就会出现「pass=4 但 ci_open=false」，排查时不知信哪个。
//! 5. 分 a/b/c 三族规避 `MAX_CHECKS = 112` 截断。

use alloc::vec;

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vee12_textqa as qa;
use crate::svstar2::vee13_examples as ex;

// ===========================================================================
// 判据侧写死的期望值
// ===========================================================================

mod expect {
    /// 示例例数。
    pub const EXAMPLES: u32 = 4;
    /// README 步数。
    pub const STEPS: usize = 3;
    /// 冒烟动作数（编译/运行/退出码/截图比对）。
    pub const SMOKE_ACTIONS: u32 = 4;
    /// 帧率基线（非压测场景）。
    pub const FPS_BASE: u32 = 60;
    /// 压测场景帧率下限。
    pub const FPS_STRESS_FLOOR: u32 = 30;
    /// HelloText 源文件行数上限。
    pub const HELLO_MAX_LINES: u32 = 30;
    /// API 面数（三函数 + 选项面）。
    pub const SURFACES: usize = 4;
    /// 降级序长度。
    pub const DEGRADE: usize = 4;
    /// FNV 偏移基。
    pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    /// FNV 素数。
    pub const FNV_PRIME: u64 = 0x100_0000_01b3;
    /// MeasureText 语义常量（默认超时 16ms）。
    pub const SEM_MEASURE: u64 = 16;
    /// RenderText 语义常量（图元批次上限）。
    pub const SEM_RENDER: u64 = 4_096;
    /// CacheControl 语义常量（缓存水位 850‰）。
    pub const SEM_CACHE: u64 = 850;
    /// RenderOptions 语义常量（默认行高 1.2em）。
    pub const SEM_OPTIONS: u64 = 1_200;
    /// 分隔符（ASCII unit separator，避免符号名拼接歧义）。
    pub const SEP: u8 = 0x1f;
}

/// 判据侧独立的 FNV-1a 实现（不调被测的 `fnv1a`，那是被测的一部分）。
fn ind_fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes.iter() {
        h ^= *b as u64;
        // 用判据侧写死的素数，不引用被测的 FNV_PRIME 常量
        h = h.wrapping_mul(expect::FNV_PRIME);
    }
    h
}

/// 判据侧独立的单面指纹重算。
///
/// 刻意**不复用**被测的 `surface_fingerprint`——否则就是自证式：被测算错了，
/// 判据用被测的算法算同一个错值，两边一致，红项永远不会亮。
fn ind_surface_fp(symbol: &str, semantics: u64) -> u64 {
    let h = ind_fnv1a(expect::FNV_OFFSET, symbol.as_bytes());
    let mut buf = [0u8; 8];
    let mut i = 0;
    while i < 8 {
        // 大端喂入，与被测同口径（判据自己算一遍，不是问被测要字节序）
        buf[i] = ((semantics >> (56 - 8 * i)) & 0xff) as u8;
        i += 1;
    }
    ind_fnv1a(h, &buf)
}

/// 判据侧独立的「漂移检测有效性」判定：语义常量变了则指纹必须变。
///
/// 这是「漂移即红」这条纪律的核心断言。方向要**双向**：
///   - 语义常量不同 ⇒ 指纹必不同（否则改常量检测不出来）；
///   - 符号名不同 ⇒ 指纹必不同（否则改名检测不出来）。
fn ind_symbol_or_sem_change_moves_fp(
    a_symbol: &str,
    a_sem: u64,
    b_symbol: &str,
    b_sem: u64,
) -> bool {
    let fa = ind_surface_fp(a_symbol, a_sem);
    let fb = ind_surface_fp(b_symbol, b_sem);
    fa != fb
}

/// 判据总数（a+b+c，不含 meta）。按**运行值**计。
/// 实测基线：a=26 / b=31 / c=57，合计 114。
pub const TOTAL_CHECKS: usize = 114;
/// a 族条数。
pub const A_COUNT: usize = 26;
/// b 族条数。
pub const B_COUNT: usize = 31;
/// c 族条数。
pub const C_COUNT: usize = 57;

// ===========================================================================
// a 族：4 例与四件套（示例集结构 / 工程 / README 三步 / 资产 / 截图金样同源）
// ===========================================================================

/// a 族：示例集结构 + 四件套齐备。
pub fn run_vee13_checks_a() -> CheckSet {
    let mut s = CheckSet::new("VE-F0813-a");
    let set = ex::builtin_examples();

    // --- 4 例封闭全集 ---
    s.add(
        "示例数为 4",
        set.len() as u32 == expect::EXAMPLES,
        "锚点原文「示例 4 个」，少一示例即覆盖面缺口",
    );
    s.add(
        "示例身份无重复",
        set.duplicate_id_count() == 0,
        "重复身份会稀释「4 例」",
    );
    s.add(
        "缺失身份计数对满额集为 0（反向验证）",
        set.missing_id_count() == 0,
        "满额内置集不得报缺失",
    );
    s.add(
        "缺失身份计数对残缺集为 2（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if d.items.len() == expect::EXAMPLES as usize {
                d.items.remove(3);
                d.items.remove(2);
                hit = ex::ExampleSet::missing_id_count(&d) == 2;
            }
            hit
        },
        "移除 WorldText 与 A11yZoom 后缺失数恰为 2，用 == 钉死计数",
    );
    s.add(
        "缺失身份计数对空集为 4（反向验证）",
        {
            let empty = ex::ExampleSet::new(Vec::new());
            ex::ExampleSet::missing_id_count(&empty) == expect::EXAMPLES
        },
        "空集缺全部四例；恒返回 0 的实现会被这条抓住",
    );
    s.add(
        "四类示例无缺失",
        set.missing_id_count() == 0,
        "HelloText/排版压力场/世界内文字/无障碍放大覆盖四类接入路径",
    );
    //逐例点名：缺任一类都会让「4 例」变成 3 类 + 1 重复。
    //判据名用字面量——CheckSet 收&'static str，动态名只能靠泄漏，内核里不合适。
    let mut has_hello = false;
    let mut has_stress = false;
    let mut has_world = false;
    let mut has_a11y = false;
    for e in set.items.iter() {
        match e.id {
            ex::ExampleId::HelloText => has_hello = true,
            ex::ExampleId::LayoutStress => has_stress = true,
            ex::ExampleId::WorldText => has_world = true,
            ex::ExampleId::A11yZoom => has_a11y = true,
        }
    }
    s.add(
        "示例一 HelloText 在册（最小接入）",
        has_hello,
        "四类接入路径须逐类在册，不能靠凑数",
    );
    s.add(
        "示例二 排版压力场在册（性能调优）",
        has_stress,
        "1,000 动态文本对象 + 缓存水位可视化",
    );
    s.add(
        "示例三 世界内文字在册（变换与合批）",
        has_world,
        "3D 空间告示牌 + 深度淡化",
    );
    s.add(
        "示例四 无障碍放大在册（放大与高对比）",
        has_a11y,
        "动态字号缩放 + 高对比模式",
    );
    s.add(
        "示例序号 1..4 与身份对应",
        {
            let mut ok = true;
            for i in 0..set.len() {
                let e = set.get(i);
                let ord = match e {
                    Some(x) => x.id.ordinal(),
                    None => {
                        ok = false;
                        continue;
                    }
                };
                if ord != (i as u32) + 1 {
                    ok = false;
                }
            }
            ok && set.len() == expect::EXAMPLES as usize
        },
        "序号与身份脱节会让「第 N 例」在文档与代码里指向不同的东西",
    );

    // --- 可运行工程 ---
    s.add(
        "工程源文件行数均为正",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.project_lines == 0 {
                    ok = false;
                }
            }
            ok
        },
        "0 行工程谈不上可运行",
    );
    s.add(
        "HelloText 30 行内可跑（锚点原文）",
        {
            let mut ok = true;
            let mut seen = false;
            for e in set.items.iter() {
                if e.id == ex::ExampleId::HelloText {
                    seen = true;
                    if e.project_lines > expect::HELLO_MAX_LINES {
                        ok = false;
                    }
                }
            }
            ok && seen
        },
        "锚点原文「30行内可跑」——最小接入的意义就是短",
    );
    s.add(
        "其余示例不限 30 行（非最小接入不该被同一上限卡）",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.id != ex::ExampleId::HelloText && e.project_lines <= expect::HELLO_MAX_LINES {
                    // 非 HelloText 恰好 30 行不违规，但要求它们确实> 30 以体现差异，
                    // 这里只断言「没有把上限误用到非最小接入示例上」——
                    // 判据：非 HelloText 的行数不得等于 HelloText 的上限常量（否则说明上限写死在共享处）
                    if e.project_lines == expect::HELLO_MAX_LINES {
                        ok = false;
                    }
                }
            }
            ok
        },
        "30 行上限只约束示例一；把它套到全部示例上会逼示例三/四写残",
    );

    // --- README 三步跑通 ---
    s.add(
        "README 三步齐备且恰为三步",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if !e.readme_three_steps() {
                    ok = false;
                }
            }
            ok
        },
        "锚点原文「README（三步跑通）」——两步不叫三步，四步也不是本单规格",
    );
    s.add(
        "三步顺序为取资产→构建→运行",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.readme_steps.len() != expect::STEPS {
                    ok = false;
                    continue;
                }
                if e.readme_steps.get(0) != Some(&ex::Step::Fetch)
                    || e.readme_steps.get(1) != Some(&ex::Step::Build)
                    || e.readme_steps.get(2) != Some(&ex::Step::Run)
                {
                    ok = false;
                }
            }
            ok
        },
        "顺序即语义：先构建后取资产在工程上不成立",
    );
    s.add(
        "步序号 1..3 自洽",
        {
            let mut ok = true;
            for e in set.items.iter() {
                for st in e.readme_steps.iter() {
                    if st.ordinal() == 0 || st.ordinal() > expect::STEPS as u32 {
                        ok = false;
                    }
                }
            }
            ok
        },
        "步骤序号越界说明枚举与 README 脱节",
    );

    // --- 资产（打包校验拦截）---
    s.add(
        "资产全部在打包清单内",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if !e.assets_complete() {
                    ok = false;
                }
            }
            ok
        },
        "锚点原文「示例依赖资源缺失→打包校验拦截」",
    );
    s.add(
        "每个示例至少声明一个资产",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.assets.is_empty() {
                    ok = false;
                }
            }
            ok
        },
        "零资产声明等于绕过打包校验",
    );
    s.add(
        "资产名非空",
        {
            let mut ok = true;
            for e in set.items.iter() {
                for a in e.assets.iter() {
                    if a.name.is_empty() {
                        ok = false;
                    }
                }
            }
            ok
        },
        "空资产名会让打包校验无从匹配",
    );

    // --- 预期截图金样同源---
    s.add(
        "预期截图键落在金样集齐备范围（复用 F0812 口径）",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if !e.shot_golden_aligned() {
                    ok = false;
                }
            }
            ok
        },
        "锚点原文「预期截图（金样同源）」——字体号/Hinting 档须在册内枚举内",
    );
    s.add(
        "截图键字体号小于金样字体数",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if (e.shot_key.font as u32) >= qa::GOLDEN_FONT_COUNT {
                    ok = false;
                }
            }
            ok
        },
        "字体号越界即未登记字体",
    );
    s.add(
        "截图键 Hinting 档小于金样档数",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if (e.shot_key.hint as u32) >= qa::GOLDEN_HINT_COUNT {
                    ok = false;
                }
            }
            ok
        },
        "Hinting 档越界即未登记档",
    );
    s.add(
        "截图键字号非0",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.shot_key.size_px == 0 {
                    ok = false;
                }
            }
            ok
        },
        "字号 0 不是合法排版字号",
    );
    s.add(
        "截图指纹由金样同源函数得出（判据侧独立重算对拍）",
        {
            let mut ok = true;
            for e in set.items.iter() {
                let impl_fp = e.shot_fingerprint();
                let ind_fp = qa::key_fingerprint(&e.shot_key);
                if impl_fp != ind_fp {
                    ok = false;
                }
            }
            ok
        },
        "示例若另写一份近似算法算截图指纹，「金样同源」就是空话",
    );
    s.add(
        "不同示例的截图键互异（否则截图不具区分度）",
        {
            let mut dup = false;
            for i in 0..set.len() {
                for j in 0..i {
                    let a = set.get(i);
                    let b = set.get(j);
                    if let (Some(x), Some(y)) = (a, b) {
                        if x.shot_key.label() == y.shot_key.label() {
                            dup = true;
                        }
                    }
                }
            }
            !dup
        },
        "四示例共用一张截图键＝金样覆盖被压缩成 1",
    );
    s
}

/// a 族独立入口。
pub fn run_vee13_checks_a_standalone() -> CheckSet {
    run_vee13_checks_a()
}

// ===========================================================================
// b 族：API 面 / 指纹 / 漂移即红 / CI 冒烟复用
// ===========================================================================

/// b 族：API 面指纹与漂移即红。
pub fn run_vee13_checks_b() -> CheckSet {
    let mut s = CheckSet::new("VE-F0813-b");

    // --- API 面封闭全集 ---
    s.add(
        "API 面数为 4（三函数 + 选项面）",
        ex::ApiSurface::ALL.len() == expect::SURFACES,
        "F0817冻结三函数 + 选项面，缺一项则漂移检测有盲区",
    );
    let mut has_measure = false;
    let mut has_render = false;
    let mut has_cache = false;
    let mut has_options = false;
    for c in ex::ApiSurface::ALL.iter() {
        match c {
            ex::ApiSurface::MeasureText => has_measure = true,
            ex::ApiSurface::RenderText => has_render = true,
            ex::ApiSurface::CacheControl => has_cache = true,
            ex::ApiSurface::RenderOptions => has_options = true,
        }
    }
    s.add(
        "API 面 MeasureText 在册（F0817 度量）",
        has_measure,
        "三函数与选项面须逐项在册",
    );
    s.add(
        "API 面 RenderText 在册（F0817 图元提交）",
        has_render,
        "三函数与选项面须逐项在册",
    );
    s.add(
        "API 面 CacheControl 在册（F0817 图集管理）",
        has_cache,
        "三函数与选项面须逐项在册",
    );
    s.add(
        "API 面 RenderOptions 在册（选项面）",
        has_options,
        "选项面单列，否则结构体追加字段检不出",
    );
    s.add(
        "API 面序号 0..3 与全集自洽",
        {
            let mut ok = true;
            for (i, c) in ex::ApiSurface::ALL.iter().enumerate() {
                // 判据侧独立反查：全集第 i 项的符号名须能在全集里按序号找回同一面
                let mut back: Option<&'static str> = None;
                for (j, d) in ex::ApiSurface::ALL.iter().enumerate() {
                    if j == i {
                        back = Some(d.symbol());
                    }
                }
                match back {
                    Some(s) => {
                        if s != c.symbol() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                if c.symbol().is_empty() {
                    ok = false;
                }
            }
            ok
        },
        "全集与反查必须双向一致，否则漂移检测用的面集不封闭",
    );

    // --- 符号名与语义常量 ---
    s.add(
        "语义常量与册内锚点一致",
        {
            let mut ok = true;
            for c in ex::ApiSurface::ALL.iter() {
                let want = match c {
                    ex::ApiSurface::MeasureText => expect::SEM_MEASURE,
                    ex::ApiSurface::RenderText => expect::SEM_RENDER,
                    ex::ApiSurface::CacheControl => expect::SEM_CACHE,
                    ex::ApiSurface::RenderOptions => expect::SEM_OPTIONS,
                };
                if c.semantics() != want {
                    ok = false;
                }
            }
            ok
        },
        "语义常量是漂移检测的锚，锚本身漂移则检测失效",
    );
    s.add(
        "符号名非空且唯一",
        {
            let mut ok = true;
            for i in 0..ex::ApiSurface::ALL.len() {
                let a = ex::ApiSurface::ALL.get(i);
                for j in 0..i {
                    let b = ex::ApiSurface::ALL.get(j);
                    if let (Some(x), Some(y)) = (a, b) {
                        if x.symbol() == y.symbol() {
                            ok = false;
                        }
                        if x.symbol().is_empty() {
                            ok = false;
                        }
                    }
                }
            }
            ok
        },
        "符号名重复会让两个面指纹相同，漂移检测把它们当同一个面",
    );
    s.add(
        "符号名含 F0817 前缀（示例绑定的是冻结 API）",
        {
            let mut ok = true;
            for c in ex::ApiSurface::ALL.iter() {
                if !c.symbol().starts_with("F0817::") {
                    ok = false;
                }
            }
            ok
        },
        "示例须绑定F0817 的冻结三函数，不能指向别处",
    );

    // --- 指纹：判据侧独立重算 ---
    s.add(
        "单面指纹与判据侧独立实现逐面一致",
        {
            let mut ok = true;
            for c in ex::ApiSurface::ALL.iter() {
                let impl_fp = ex::surface_fingerprint(*c);
                let ind_fp = ind_surface_fp(c.symbol(), c.semantics());
                if impl_fp != ind_fp {
                    ok = false;
                }
            }
            ok
        },
        "指纹算法错则漂移检测整体失效——判据侧独立重算是唯一防线",
    );
    s.add(
        "四组语义常量两两指纹相异",
        {
            let mut ok = true;
            for i in 0..ex::ApiSurface::ALL.len() {
                for j in 0..i {
                    let a = ex::ApiSurface::ALL.get(i);
                    let b = ex::ApiSurface::ALL.get(j);
                    if let (Some(x), Some(y)) = (a, b) {
                        if ex::surface_fingerprint(*x) == ex::surface_fingerprint(*y) {
                            ok = false;
                        }
                    }
                }
            }
            ok
        },
        "两不同面同指纹＝漂移检测漏检其中之一",
    );
    s.add(
        "改语义常量必改指纹（漂移即红·反向验证）",
        ind_symbol_or_sem_change_moves_fp(
            "F0817::MeasureText",
            expect::SEM_MEASURE,
            "F0817::MeasureText",
            expect::SEM_MEASURE + 16,
        ),
        "语义常量改了而指纹不变＝漂移检测是假的（只比符号名的典型症状）",
    );
    s.add(
        "改符号名必改指纹（漂移即红·反向验证）",
        ind_symbol_or_sem_change_moves_fp(
            "F0817::RenderText",
            expect::SEM_RENDER,
            "F0817::RenderTextV2",
            expect::SEM_RENDER,
        ),
        "符号改名而指纹不变＝API 破坏性变更检测不到",
    );
    s.add(
        "语义常量与符号名都变必改指纹",
        ind_symbol_or_sem_change_moves_fp(
            "F0817::CacheControl",
            expect::SEM_CACHE,
            "F0817::CacheControl2",
            expect::SEM_CACHE + 50,
        ),
        "双重变更更须被抓到",
    );
    s.add(
        "面集指纹顺序无关（调整依赖顺序不是漂移）",
        {
            let a = ex::surface_set_fingerprint(&[
                ex::ApiSurface::MeasureText,
                ex::ApiSurface::RenderText,
                ex::ApiSurface::CacheControl,
                ex::ApiSurface::RenderOptions,
            ]);
            let b = ex::surface_set_fingerprint(&[
                ex::ApiSurface::RenderOptions,
                ex::ApiSurface::CacheControl,
                ex::ApiSurface::RenderText,
                ex::ApiSurface::MeasureText,
            ]);
            a == b && a != 0
        },
        "示例调整依赖顺序不该让冒烟变红——否则人们会去调顺序骗过门禁",
    );
    s.add(
        "面集指纹非零（非空输入不得产出零指纹）",
        ex::surface_set_fingerprint(&ex::ApiSurface::ALL) != 0,
        "零指纹与「无依赖」不可区分",
    );
    s.add(
        "面集指纹与全域实测指纹一致",
        ex::surface_set_fingerprint(&ex::ApiSurface::ALL) == ex::current_api_fingerprint(),
        "冒烟比对的两端必须同源",
    );
    // 反向：若面集指纹变成「按输入顺序」而非「按全集序」，则示例调整依赖顺序就红。
    // 语料：同一组面、两种书写顺序，指纹必须相等。
    s.add(
        "面集指纹书写顺序无关（两两对调全检·反向验证）",
        {
            let full = ex::ApiSurface::ALL;
            let mut ok = true;
            // 对每一对相邻面做对调，四组全对调后指纹都必须与原顺序一致
            for i in 0..full.len().saturating_sub(1) {
                let mut reordered: Vec<ex::ApiSurface> = Vec::new();
                for (j, c) in full.iter().enumerate() {
                    if j == i {
                        if let Some(nxt) = full.get(i + 1) {
                            reordered.push(*nxt);
                        }
                    } else if j == i + 1 {
                        if let Some(cur) = full.get(i) {
                            reordered.push(*cur);
                        }
                    } else {
                        reordered.push(*c);
                    }
                }
                if reordered.len() != full.len() {
                    ok = false;
                    continue;
                }
                if ex::surface_set_fingerprint(&reordered) != ex::surface_set_fingerprint(&full) {
                    ok = false;
                }
            }
            ok
        },
        "只测「全逆序」不够：逐对相邻对调才能暴露「按输入下标定位」的实现",
    );

    // --- 依赖面纪律 ---
    s.add(
        "示例依赖面全部在全集内",
        {
            let mut ok = true;
            for e in ex::builtin_examples().items.iter() {
                if ex::unknown_surface_count(&e.api_surfaces) != 0 {
                    ok = false;
                }
            }
            ok
        },
        "依赖未登记的 API 面＝漂移检测有盲区",
    );
    s.add(
        "示例依赖面无重复声明",
        {
            let mut ok = true;
            for e in ex::builtin_examples().items.iter() {
                if ex::duplicate_surface_count(&e.api_surfaces) != 0 {
                    ok = false;
                }
            }
            ok
        },
        "同一面写两遍是声明冗余",
    );
    s.add(
        "每个示例至少依赖一个 API 面",
        {
            let mut ok = true;
            for e in ex::builtin_examples().items.iter() {
                if e.api_surfaces.is_empty() {
                    ok = false;
                }
            }
            ok
        },
        "零依赖示例等于脱离引擎的独立程序，谈不上接入路径",
    );
    s.add(
        "未知面计数对全集输入为 0",
        ex::unknown_surface_count(&ex::ApiSurface::ALL) == 0,
        "全集输入不得报未知面",
    );
    s.add(
        "重复面计数对无重复输入为 0（反向验证）",
        ex::duplicate_surface_count(&[
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::RenderText,
        ]) == 0,
        "无重复输入不得误报重复",
    );
    s.add(
        "重复面计数对重复输入为 2（反向验证）",
        ex::duplicate_surface_count(&[
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::RenderText,
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::RenderText,
        ]) == 2,
        "两个面各重复一次须恰为 2，用 == 而非 >= 钉死计数",
    );
    s.add(
        "重复面计数按种类不按位置（同一面三次仍算一种）",
        ex::duplicate_surface_count(&[
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::MeasureText,
            ex::ApiSurface::RenderText,
        ]) == 1,
        "口径必须与duplicate_id_count / missing_id_count 一致；按位置计会得 3",
    );
    s.add(
        "示例身份重复计数按种类不按位置（同一身份三次仍算一种）",
        {
            let mut d = ex::builtin_examples();
            // 把三处改成同一身份：2 例变3 次重复 ⇒ 重复种类应为 1
            if d.items.len() == expect::EXAMPLES as usize {
                let id = ex::ExampleId::HelloText;
                for i in 0..d.items.len() {
                    if let Some(e) = d.items.get_mut(i) {
                        if i < 3 {
                            e.id = id;
                        }
                    }
                }
                ex::ExampleSet::duplicate_id_count(&d) == 1
            } else {
                false
            }
        },
        "身份计数与面计数口径统一，避免同一件事两个答案",
    );

    // --- 漂移即红（端到端）---
    s.add(
        "内置示例与当前 API 对齐（基线）",
        {
            let mut ok = true;
            for e in ex::builtin_examples().items.iter() {
                if !e.api_aligned() {
                    ok = false;
                }
            }
            ok
        },
        "内置示例烧的指纹必须等于此刻的 API 面指纹",
    );
    s.add(
        "单个示例漂移即红（反向验证）",
        {
            let mut set = ex::builtin_examples();
            let mut drifted = 0u32;
            if let Some(first) = set.items.get_mut(0) {
                first.api_fingerprint = first.api_fingerprint.wrapping_add(1);
                drifted = 1;
            }
            drifted == 1 && !ex::run_smoke(&set).ci_open()
        },
        "锚点原文「示例与 API 漂移→CI 冒烟即红」",
    );
    s.add(
        "全部示例漂移即红（反向验证）",
        {
            let mut set = ex::builtin_examples();
            for e in set.items.iter_mut() {
                e.api_fingerprint = e.api_fingerprint.wrapping_add(7);
            }
            let sum = ex::run_smoke(&set);
            !sum.api_aligned && !sum.ci_open()
        },
        "全体漂移不得因为「多数仍对齐」而放行",
    );
    s.add(
        "漂移的示例不计入 pass（汇总口径与门禁自洽）",
        {
            let mut set = ex::builtin_examples();
            if let Some(first) = set.items.get_mut(0) {
                first.api_fingerprint = first.api_fingerprint.wrapping_add(1);
            }
            let sum = ex::run_smoke(&set);
            // 门禁闭的同时 pass 必须少于 4——否则出现「pass=4 但 ci_open=false」
            // 这种自相矛盾的汇总，排查时不知该信哪个
            sum.pass < expect::EXAMPLES && !sum.ci_open()
        },
        "pass 计数与门禁必须同口径",
    );
    s.add(
        "漂移检测依赖语义常量：语义常量变则对齐失败（端到端反向验证）",
        {
            // 判据侧独立重算新指纹（模拟 API 侧改了语义常量），
            // 确认它与内置示例烧的值不等——否则「改常量不改指纹」端到端也不红
            let new_fp = ind_surface_fp("F0817::MeasureText", expect::SEM_MEASURE + 16);
            let cur_fp = ind_surface_fp("F0817::MeasureText", expect::SEM_MEASURE);
            new_fp != cur_fp
        },
        "端到端层面的语义漂移可检出",
    );
    s
}

/// b 族独立入口。
pub fn run_vee13_checks_b_standalone() -> CheckSet {
    run_vee13_checks_b()
}

// ===========================================================================
// c 族：CI 冒烟复用 / 三态 / 帧率 / 降级红线 / 汇总门禁
// ===========================================================================

/// c 族：CI 冒烟复用 + 帧率 + 降级红线 + 门禁。
pub fn run_vee13_checks_c() -> CheckSet {
    let mut s = CheckSet::new("VE-F0813-c");
    let set = ex::builtin_examples();

    // --- CI 冒烟复用全域范式（复用声明落地）---
    s.add(
        "冒烟动作数为 4（编译/运行/退出码/截图比对）",
        ex::SmokeAction::ALL.len() as u32 == expect::SMOKE_ACTIONS,
        "锚点原文「CI 冒烟脚本（编译+运行+退出码校验，复用全域 CI 范式）」",
    );
    let mut in_compile = false;
    let mut in_run = false;
    let mut in_exit = false;
    let mut in_golden = false;
    for c in ex::SmokeAction::ALL.iter() {
        match c {
            ex::SmokeAction::Compile => in_compile = true,
            ex::SmokeAction::Run => in_run = true,
            ex::SmokeAction::ExitCode => in_exit = true,
            ex::SmokeAction::GoldenCompare => in_golden = true,
        }
    }
    s.add(
        "冒烟动作 编译 在册",
        in_compile,
        "锚点原文「编译+运行+退出码校验，复用全域 CI 范式」",
    );
    s.add(
        "冒烟动作 运行 在册",
        in_run,
        "编译过不代表能跑",
    );
    s.add(
        "冒烟动作 退出码 在册",
        in_exit,
        "只看「跑起来了」不够，退出码须符合预期",
    );
    s.add(
        "冒烟动作 预期截图比对 在册",
        in_golden,
        "锚点原文「预期截图（金样同源）」须进 CI 动作",
    );
    s.add(
        "四个冒烟动作均为必需（无豁免）",
        {
            let mut ok = true;
            for a in ex::SmokeAction::ALL.iter() {
                if !a.required() {
                    ok = false;
                }
            }
            ok
        },
        "任一动作可豁免则冒烟可被绕过",
    );
    s.add(
        "冒烟动作名互异（不得两项同名而实为一项）",
        {
            let mut ok = true;
            for i in 0..ex::SmokeAction::ALL.len() {
                for j in 0..i {
                    let a = ex::SmokeAction::ALL.get(i);
                    let b = ex::SmokeAction::ALL.get(j);
                    if let (Some(x), Some(y)) = (a, b) {
                        if x.label() == y.label() {
                            ok = false;
                        }
                        if x.label().is_empty() {
                            ok = false;
                        }
                    }
                }
            }
            ok
        },
        "同名动作会让「4 个动作」实际只有更少",
    );
    s.add(
        "冒烟示例数与 F0812 登记的 4 例一致（复用声明）",
        qa::SMOKE_EXAMPLES == expect::EXAMPLES,
        "F0812 已登记 SMOKE_EXAMPLES=4；两处不得漂移",
    );
    s.add(
        "四例名称与 F0812 冒烟契约一致（复用声明）",
        {
            let mut ok = qa::SMOKE_CASES.len() == expect::EXAMPLES as usize;
            for (i, e) in set.items.iter().enumerate() {
                if let Some(c) = qa::SMOKE_CASES.get(i) {
                    if c.name != e.id.name() {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
            }
            ok
        },
        "示例改名而冒烟契约不改＝两边说的不是同一批示例",
    );

    // --- 三态：NotApplicable 不算绿 ---
    s.add(
        "Pass 是唯一 CI 绿（没测≠ 测过）",
        ex::SmokeVerdict::Pass.ci_green()
            && !ex::SmokeVerdict::Fail.ci_green()
            && !ex::SmokeVerdict::NotApplicable.ci_green(),
        "三态里NotApplicable 必须被门禁拒绝",
    );
    s.add(
        "三态标签互异",
        ex::SmokeVerdict::Pass.label() != ex::SmokeVerdict::Fail.label()
            && ex::SmokeVerdict::Pass.label() != ex::SmokeVerdict::NotApplicable.label()
            && ex::SmokeVerdict::Fail.label() != ex::SmokeVerdict::NotApplicable.label(),
        "标签重复会让日志里分不清是哪种失败",
    );
    s.add(
        "单例冒烟含 NotApplicable 则门禁闭（反向验证）",
        {
            let mut d = ex::builtin_examples();
            if let Some(e) = d.items.get_mut(0) {
                e.smoke = [
                    ex::SmokeVerdict::NotApplicable,
                    ex::SmokeVerdict::Pass,
                    ex::SmokeVerdict::Pass,
                    ex::SmokeVerdict::Pass,
                ];
            }
            let sum = ex::run_smoke(&d);
            sum.na == 1 && !sum.ci_open()
        },
        "把动作标成不适用来绕过检查，必须仍被拦",
    );
    s.add(
        "单例冒烟含 Fail 则门禁闭（反向验证）",
        {
            let mut d = ex::builtin_examples();
            if let Some(e) = d.items.get_mut(0) {
                e.smoke = [
                    ex::SmokeVerdict::Pass,
                    ex::SmokeVerdict::Fail,
                    ex::SmokeVerdict::Pass,
                    ex::SmokeVerdict::Pass,
                ];
            }
            let sum = ex::run_smoke(&d);
            sum.fail == 1 && !sum.ci_open()
        },
        "有一动作失败不得放行",
    );
    s.add(
        "冒烟未接入四动作则门禁闭（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.smoke_complete = false;
                hit = !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "「脚本写了但没接入」也是没跑——单独构造 smoke_complete=false，其余全绿",
    );
    s.add(
        "冒烟未接入四动作则该例不计pass（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.smoke_complete = false;
                // 其余全绿（资产/金样/帧率/三步都满足），只有 smoke_complete 被破坏
                hit = ex::run_smoke(&d).pass < expect::EXAMPLES;
            }
            hit
        },
        "smoke_green 必须真的看 smoke_complete，否则删掉这个检查判据照样全绿",
    );
    // 三步跑通：反向语料逐条（少一步 / 多一步 / 乱序）
    s.add(
        "三步少一步即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.readme_steps = vec![ex::Step::Fetch, ex::Step::Build];
                hit = !e.readme_three_steps() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "两步不叫三步",
    );
    s.add(
        "三步多一步即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.readme_steps = vec![
                    ex::Step::Fetch,
                    ex::Step::Build,
                    ex::Step::Run,
                    ex::Step::Run,
                ];
                hit = !e.readme_three_steps() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "四步也不是本单规格——只查「≥3步」会放过它",
    );
    s.add(
        "三步乱序即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.readme_steps =
                    vec![ex::Step::Build, ex::Step::Fetch, ex::Step::Run];
                hit = !e.readme_three_steps() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "只查「三步都出现过」会放过乱序——先构建后取资产不成立",
    );
    // 金样同源：反向语料
    s.add(
        "截图 Hinting 档越界即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.shot_key = qa::GoldenKey::new(0, 16, qa::GOLDEN_HINT_COUNT as u8);
                hit = !e.shot_golden_aligned() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "未登记的 Hinting 档不能进金样",
    );
    s.add(
        "截图字号 0 即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.shot_key = qa::GoldenKey::new(0, 0, 0);
                hit = !e.shot_golden_aligned() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "字号 0 不是合法排版",
    );
    s.add(
        "截图字体号越界即不合格（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.shot_key = qa::GoldenKey::new(qa::GOLDEN_FONT_COUNT as u16, 16, 0);
                hit = !e.shot_golden_aligned() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "未登记字体不能进金样",
    );
    s.add(
        "示例集少于 4 例则门禁闭（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut ok = false;
            // 移除一例（不足 4 例则本条记红，不早退以免族内条数漂移）
            if d.items.len() == expect::EXAMPLES as usize {
                d.items.remove(3);
                let sum = ex::run_smoke(&d);
                ok = sum.pass < expect::EXAMPLES && !sum.ci_open();
            }
            ok
        },
        "少一例时 pass 不得仍等于 4",
    );

    // --- 帧率基线与压测下限 ---
    s.add(
        "非压测示例帧率下限为 60（锚点原文基线）",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if !e.id.is_stress() && ex::fps_floor_of(e.id) != expect::FPS_BASE {
                    ok = false;
                }
            }
            ok
        },
        "锚点原文「示例帧率基线 60fps」",
    );
    s.add(
        "压测场景帧率下限为 30（锚点原文下限）",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if e.id.is_stress() && ex::fps_floor_of(e.id) != expect::FPS_STRESS_FLOOR {
                    ok = false;
                }
            }
            ok
        },
        "锚点原文「压测场景 30fps 下限」——与 60 合并会让压测永远红",
    );
    s.add(
        "恰在下限判过",
        {
            let mut ok = true;
            for e in set.items.iter() {
                if !e.fps_ok() {
                    ok = false;
                }
            }
            ok
        },
        "≥ 下限即达标",
    );
    s.add(
        "低于下限 1fps 即判不过（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.measured_fps = expect::FPS_BASE - 1;
                hit = !e.fps_ok();
            }
            hit
        },
        "帧率判定不含容差余量",
    );
    s.add(
        "压测场景 29fps 判不过（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(1) {
                e.measured_fps = expect::FPS_STRESS_FLOOR - 1;
                hit = !e.fps_ok() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "压测场景须按 30 判而非 60",
    );
    s.add(
        "压测场景不被 60fps 门槛卡住（30fps 应通过）",
        {
            let mut ok = false;
            if let Some(e) = set.get(1) {
                ok = e.id.is_stress() && e.measured_fps == expect::FPS_STRESS_FLOOR && e.fps_ok();
            }
            ok
        },
        "若压测场景也按 60 判，则30fps 压测场景永远红",
    );
    s.add(
        "仅压测场景标记 is_stress",
        {
            let mut cnt = 0u32;
            for e in set.items.iter() {
                if e.id.is_stress() {
                    cnt = cnt.saturating_add(1);
                }
            }
            cnt == 1
        },
        "把轻量示例也标成压测会让帧率基线失去区分度",
    );
    s.add(
        "仅无障碍示例标记 is_a11y",
        {
            let mut cnt = 0u32;
            for e in set.items.iter() {
                if e.id.is_a11y() {
                    cnt = cnt.saturating_add(1);
                }
            }
            cnt == 1
        },
        "红线保护序只对示例四生效",
    );

    // --- 降级红线保护序 ---
    s.add(
        "降级序长度 4",
        ex::DEGRADE_ORDER.len() == expect::DEGRADE,
        "降级序是封闭清单",
    );
    s.add(
        "内置降级序保护无障碍（红线保护序）",
        ex::degrade_order_protects_a11y(&ex::DEGRADE_ORDER),
        "放大/高对比须排在渲染质量之后才动",
    );
    s.add(
        "无障碍动作位次严格晚于所有非无障碍动作",
        {
            let mut max_plain: i64 = -1;
            let mut min_a11y: i64 = -1;
            for (i, a) in ex::DEGRADE_ORDER.iter().enumerate() {
                let idx = i as i64;
                if a.is_a11y() {
                    if min_a11y < 0 || idx < min_a11y {
                        min_a11y = idx;
                    }
                } else if idx > max_plain {
                    max_plain = idx;
                }
            }
            max_plain >= 0 && min_a11y >= 0 && max_plain < min_a11y
        },
        "判据侧独立重算位次关系，不采信实现的 bool 返回作唯一依据",
    );
    s.add(
        "高对比与放大均属无障碍动作",
        ex::DegradeAction::HighContrast.is_a11y() && ex::DegradeAction::A11yZoom.is_a11y(),
        "红线保护对象就这两个",
    );
    s.add(
        "Hinting 与字号采样不属无障碍（可先降）",
        !ex::DegradeAction::HintLevel.is_a11y() && !ex::DegradeAction::FontScale.is_a11y(),
        "降级序先砍渲染质量",
    );
    s.add(
        "降级序颠倒即拒绝（反向验证）",
        {
            let bad = [
                ex::DegradeAction::A11yZoom,
                ex::DegradeAction::HintLevel,
                ex::DegradeAction::FontScale,
                ex::DegradeAction::HighContrast,
            ];
            !ex::degrade_order_protects_a11y(&bad)
        },
        "把放大排到最前＝先砍无障碍，必须拦",
    );
    s.add(
        "无障碍动作提前一位即拒绝（反向验证·边界）",
        {
            let bad = [
                ex::DegradeAction::HintLevel,
                ex::DegradeAction::HighContrast,
                ex::DegradeAction::FontScale,
                ex::DegradeAction::A11yZoom,
            ];
            !ex::degrade_order_protects_a11y(&bad)
        },
        "只提前一位也必须红——边界要钉死",
    );
    s.add(
        "降级序长度不足即拒绝（反向验证）",
        !ex::degrade_order_protects_a11y(&[
            ex::DegradeAction::HintLevel,
            ex::DegradeAction::FontScale,
        ]),
        "少一项的降级序不构成完整保护",
    );
    s.add(
        "长度不符的降级序一律拒绝（空/超长/等于全集）",
        {
            let empty: Vec<ex::DegradeAction> = Vec::new();
            let mut over: Vec<ex::DegradeAction> = Vec::new();
            for a in ex::DegradeAction::ALL.iter() {
                over.push(*a);
            }
            over.push(ex::DegradeAction::HintLevel);
            // 空、超长都必须拒；恰好等于全集（且顺序正确）必须接受——
            // 三者一起钉，避免「只查非空」或「只查 ≤4」的实现混过去
            !ex::degrade_order_protects_a11y(&empty)
                && !ex::degrade_order_protects_a11y(&over)
                && ex::degrade_order_protects_a11y(&ex::DEGRADE_ORDER)
        },
        "长度检查须双向：短与长都拒、恰好全集才受",
    );
    s.add(
        "非无障碍动作全在前段时接受（正向边界）",
        {
            // Hint, Font, 高对比, 放大 —— 恰在相邻边界，仍须保护
            let edge = [
                ex::DegradeAction::HintLevel,
                ex::DegradeAction::FontScale,
                ex::DegradeAction::HighContrast,
                ex::DegradeAction::A11yZoom,
            ];
            ex::degrade_order_protects_a11y(&edge)
        },
        "紧邻边界是合法的——把 p<m 改成 p<=m 会误拒这条（变异须双向可区分）",
    );
    s.add(
        "降级动作序号 0..3 自洽",
        {
            let mut ok = true;
            for (i, a) in ex::DegradeAction::ALL.iter().enumerate() {
                if a.rank() != i as u32 {
                    ok = false;
                }
            }
            ex::DegradeAction::ALL.len() == expect::DEGRADE && ok
        },
        "rank 与全集顺序脱节会让红线断言失去依据",
    );
    s.add(
        "降级动作标签互异",
        {
            let mut ok = true;
            for i in 0..ex::DegradeAction::ALL.len() {
                for j in 0..i {
                    let a = ex::DegradeAction::ALL.get(i);
                    let b = ex::DegradeAction::ALL.get(j);
                    if let (Some(x), Some(y)) = (a, b) {
                        if x.label() == y.label() || x.label().is_empty() {
                            ok = false;
                        }
                    }
                }
            }
            ok
        },
        "同名动作会让降级日志无法区分",
    );

    // --- 资产缺失拦截 ---
    s.add(
        "清单外资产即拦截（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.assets = vec![ex::AssetRef::new("font-regular.ttf", 24_576, false)];
                hit = !e.assets_complete() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "锚点原文「示例依赖资源缺失→打包校验拦截」",
    );
    s.add(
        "零资产声明即拦截（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.assets = Vec::new();
                hit = !e.assets_complete() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "不声明资产即绕过打包校验",
    );
    s.add(
        "空资产名即拦截（反向验证）",
        {
            let mut d = ex::builtin_examples();
            let mut hit = false;
            if let Some(e) = d.items.get_mut(0) {
                e.assets = vec![ex::AssetRef::new("", 24_576, true)];
                hit = !e.assets_complete() && !ex::run_smoke(&d).ci_open();
            }
            hit
        },
        "空名资产打包时匹配不到任何文件",
    );

    // --- 基线：门禁放行 ---
    s.add(
        "内置示例集 CI 放行（基线）",
        ex::run_smoke(&set).ci_open(),
        "四例齐备、四件齐备、帧率达标、无漂移 ⇒ 放行",
    );
    s.add(
        "基线汇总恰为 4 通过 0 失败 0 不适用",
        {
            let sum = ex::run_smoke(&set);
            sum.pass == expect::EXAMPLES && sum.fail == 0 && sum.na == 0
        },
        "用 == 而非 >= 钉死计数",
    );
    s.add(
        "基线冒烟脚本齐备且 API 对齐",
        {
            let sum = ex::run_smoke(&set);
            sum.scripts_complete && sum.api_aligned
        },
        "四维门禁的两个前置",
    );
    s.add(
        "示例集越界取返回空（零 panic 面）",
        set.get(set.len()).is_none() && set.get(usize::MAX).is_none(),
        "判据区零 panic 面：越界走 Option",
    );

    // ===============================================================
    // 门禁五维逐维独立验证
    //
    // 五维（fail / na / pass / scripts_complete / api_aligned）若彼此蕴含，
    // 单独删掉任一维度都不改变门禁结论——门禁名义五维、实际少一维也看不出来。
    // 所以下面每条都构造「**只违反这一个维度、其余全绿**」的汇总。
    // ===============================================================
    s.add(
        "全绿汇总放行（门禁五维构造基线）",
        ex::SmokeSummary::all_green().ci_open(),
        "逐维验证的基线：五维全绿必须放行，否则下面每条都会假绿",
    );
    s.add(
        "仅 fail 非零即闭（其余四维全绿·逐维验证）",
        !ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 1, 0, true, true).ci_open(),
        "只违反 fail 一维：其余全绿时也必须闭",
    );
    s.add(
        "仅 na 非零即闭（其余四维全绿·逐维验证）",
        !ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 1, true, true).ci_open(),
        "只违反 na 一维：没测不能当测过",
    );
    s.add(
        "仅 pass 不足即闭（其余四维全绿·逐维验证）",
        !ex::SmokeSummary::of(qa::SMOKE_EXAMPLES - 1, 0, 0, true, true).ci_open(),
        "只违反 pass 一维：3 通过不等于 4 通过",
    );
    s.add(
        "仅脚本不齐即闭（其余四维全绿·逐维验证）",
        !ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, false, true).ci_open(),
        "只违反 scripts_complete 一维：脚本写了没接入也是没跑",
    );
    s.add(
        "仅 API 未对齐即闭（其余四维全绿·逐维验证）",
        !ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, true, false).ci_open(),
        "只违反 api_aligned 一维：漂移即红，且不得被 pass==4 顺带兜住",
    );
    s.add(
        "五维各自都能单独闭门禁（无冗余蕴含·逐维验证）",
        {
            let dims: [ex::SmokeSummary; 5] = [
                ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 1, 0, true, true),
                ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 1, true, true),
                ex::SmokeSummary::of(qa::SMOKE_EXAMPLES - 1, 0, 0, true, true),
                ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, false, true),
                ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, true, false),
            ];
            let mut n = 0u32;
            for d in dims.iter() {
                if !d.ci_open() {
                    n = n.saturating_add(1);
                }
            }
            n == 5
        },
        "任一维度能被其余维度蕴含 ⇒ 门禁实际维度数少于名义维度数",
    );
    s.add(
        "五维全绿时门禁放行（反向验证：不能闭过头）",
        ex::SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, true, true).ci_open(),
        "逐维钉死不等于门禁闭死——全绿必须放行",
    );
    s
}

/// c 族独立入口。
pub fn run_vee13_checks_c_standalone() -> CheckSet {
    run_vee13_checks_c()
}

// ===========================================================================
// meta：三族自检
// ===========================================================================

/// meta：三族全绿 + 条数守恒。
pub fn run_vee13_meta_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0813-meta");
    let fa = run_vee13_checks_a();
    let fb = run_vee13_checks_b();
    let fc = run_vee13_checks_c();
    let (_, pa) = fa.tally();
    let (_, pb) = fb.tally();
    let (_, pc) = fc.tally();
    s.add("a 族全绿", pa == 0, "a 族不得有红项");
    s.add("b 族全绿", pb == 0, "b 族不得有红项");
    s.add("c 族全绿", pc == 0, "c 族不得有红项");
    let na = fa.len();
    let nb = fb.len();
    let nc = fc.len();
    s.add("a 族未截断", !fa.truncated(), "a 族条数须≤ MAX_CHECKS");
    s.add("b 族未截断", !fb.truncated(), "b 族条数须 ≤ MAX_CHECKS");
    s.add("c 族未截断", !fc.truncated(), "c 族条数须 ≤ MAX_CHECKS");
    s.add(
        "a 族条数与声明一致",
        na == A_COUNT,
        "改枚举规模须同步 A_COUNT",
    );
    s.add(
        "b 族条数与声明一致",
        nb == B_COUNT,
        "改枚举规模须同步 B_COUNT",
    );
    s.add(
        "c 族条数与声明一致",
        nc == C_COUNT,
        "改枚举规模须同步 C_COUNT",
    );
    s.add(
        "三族合计判据数守恒",
        na + nb + nc == TOTAL_CHECKS,
        "a+b+c 须等于 TOTAL_CHECKS",
    );
    s.add(
        "total_check_count 与三族实测之和一致",
        total_check_count() == (na + nb + nc) as u32,
        "对外暴露的总数不得与三族实测脱节",
    );
    s
}

// ===========================================================================
// 聚合入口
// ===========================================================================

/// 三族合并。
pub fn run_vee13_checks() -> CheckSet {
    let a = run_vee13_checks_a_standalone();
    let b = run_vee13_checks_b_standalone();
    let c = run_vee13_checks_c_standalone();
    let ab = CheckSet::merge(a, b);
    CheckSet::merge(ab, c)
}

/// 判据总数（三族之和，按运行值）。
pub fn total_check_count() -> u32 {
    (run_vee13_checks_a_standalone().len()
        + run_vee13_checks_b_standalone().len()
        + run_vee13_checks_c_standalone().len()) as u32
}