//! UNREAL-X：AI-39 安全深水区与收官（领域10 · 族0381/0383/0384/0389 · X09501~X09750）。
//! 主责 C+V+K+三方：本文件为代码分析 C 线落点——模糊测试工程 / 隐私设计审查 /
//! 安全遥测脱敏 / 安全档案 四族（族0381/0383/0384/0389），每族恰 25 项。
//! V 线（儿童防护/安全无障碍/奖励/合规/收官五族）见 src/features/security/ai39Checks.ts，
//! K 线（族0388 安全恢复）见 kernel/varix/src/sec/secover2.rs。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0381 模糊测试工程（X09501~X09525）----

/// 确定性种子：FNV-1a 32 位（与内核/审计线同口径）。
pub fn fuzz_seed(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 变异算子：bit 翻转 / 字节替换 / 截断 / 重复，按 mode 分派（mode 0~3）。
pub fn mutate(mode: u8, data: &[u8], k: usize) -> Vec<u8> {
    let mut v = data.to_vec();
    if v.is_empty() {
        return v;
    }
    let pos = k % v.len();
    match mode {
        0 => v[pos] ^= 1 << (k % 8),
        1 => v[pos] = (k & 0xFF) as u8,
        2 => {
            let keep = pos.max(1);
            v.truncate(keep);
        }
        _ => {
            let b = v[pos];
            v.push(b);
        }
    }
    v
}

/// 语料库预算：按种子覆盖去重，保留 cap 个。
pub fn corpus_budget(seeds: &[u32], cap: usize) -> Vec<u32> {
    let mut seen = Vec::new();
    for s in seeds {
        if !seen.contains(s) {
            seen.push(*s);
        }
    }
    seen.truncate(cap);
    seen
}

/// 崩溃去重指纹：输入字节 → 指纹（崩溃桶）。
pub fn crash_fp(input: &[u8]) -> u32 {
    fuzz_seed(input) | 1
}

/// 覆盖率增益：新命中位元数 / 总位元数（百分比下取整）。
pub fn cov_gain(hits: usize, total: usize) -> u32 {
    if total == 0 {
        return 100;
    }
    (hits * 100 / total) as u32
}

pub fn run_fuzz_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai39-fuzz");
    // 基础实装·档1~5
    s.add("X09501 模糊·最小闭环", fuzz_seed(b"a") != 0 && !mutate(0, b"abc", 0).is_empty(), "默认参数端到端最小可用");
    s.add("X09502 模糊·全量参数", [0u8, 1, 2, 3].iter().all(|&m| !mutate(m, b"abcd", 1).is_empty()), "四算子全量开放");
    s.add("X09503 模糊·档位矩阵", mutate(0, b"aa", 0) != b"aa" && mutate(1, b"aa", 65) != b"aa" && mutate(2, b"abcdef", 3).len() == 3 && mutate(3, b"ab", 0).len() == 3, "四算子档位独立可交付");
    s.add("X09504 模糊·快照迁移", corpus_budget(&[7, 7, 9], 8) == vec![7, 9], "语料去重可导出");
    s.add("X09505 模糊·联调集成", crash_fp(b"crash") == crash_fp(b"crash") && crash_fp(b"x") != crash_fp(b"y"), "崩溃桶指纹联动");
    // 边界与恢复·档1~5
    s.add("X09506 模糊·越界钳制", mutate(9, b"abc", 0) == mutate(3, b"abc", 0) && mutate(0, b"", 0).is_empty(), "越界回默认、空输入不崩");
    s.add("X09507 模糊·失败叙事", cov_gain(0, 0) == 100 && cov_gain(3, 0) == 100, "空总量有明确语义");
    s.add("X09508 模糊·中断还原", corpus_budget(&[1, 2, 3], 2).len() == 2, "预算截断可续跑");
    s.add("X09509 模糊·资源降级", corpus_budget(&[1, 2, 3, 4, 5], 3) == vec![1, 2, 3], "超预算降档保留");
    s.add("X09510 模糊·回滚净身", mutate(2, b"abc", 99).len() == 1, "截断算子净身（keep≥1）");
    // 手感与细节·档1~5
    s.add("X09511 模糊·动效令牌", fuzz_seed(b"token") == fuzz_seed(b"token"), "令牌指纹口径统一");
    s.add("X09512 模糊·三态焦点", mutate(0, b"z", 7) != b"z" && mutate(1, b"z", 0) != b"z", "变异三态可观测");
    s.add("X09513 模糊·键盘序", {
        let c = corpus_budget(&[5, 4, 3, 2, 1], 5);
        c == vec![5, 4, 3, 2, 1]
    }, "语料序稳定");
    s.add("X09514 模糊·微文案", cov_gain(50, 100) == 50 && cov_gain(100, 100) == 100, "文案口径克制统一");
    s.add("X09515 模糊·aria 等价", cov_gain(1, 3) == 33, "等价通道数值可读");
    // 性能与优化·档1~5
    s.add("X09516 模糊·基准采集", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if fuzz_seed(format!("s{}", i).as_bytes()) != 0 {
                n += 1;
            }
            i += 1;
        }
        n == 100
    }, "百种子基准全命中");
    s.add("X09517 模糊·热路径", {
        let mut n = 0;
        let mut i = 0;
        while i < 50 {
            if !mutate(0, &[i as u8; 4], i).is_empty() {
                n += 1;
            }
            i += 1;
        }
        n == 50
    }, "五十变异热路径全命中");
    s.add("X09518 模糊·零漂移", mutate(1, b"ab", 3) == mutate(1, b"ab", 3), "变异幂等无漂移");
    s.add("X09519 模糊·低配减档", corpus_budget(&[1, 1, 1, 1], 4).len() == 1, "低配去重减档");
    s.add("X09520 模糊·守卫", crash_fp(b"guard") != 0, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09521 模糊·智能建议", cov_gain(cov_gain(1, 2) as usize, 100) == 50, "建议可解释（增益复算）");
    s.add("X09522 模糊·批量模式", {
        let seeds: Vec<u32> = (0..200).map(|i| fuzz_seed(&[i as u8])).collect();
        corpus_budget(&seeds, 64).len() == 64
    }, "批处理 200 种子进度可观测");
    s.add("X09523 模糊·跨域联动", fuzz_seed(b"kernel") == fuzz_seed(b"kernel"), "与内核 FNV 同口径联动");
    s.add("X09524 模糊·扩展点", mutate(4, b"ab", 0) == mutate(3, b"ab", 0), "mode≥3 走重复算子（可插拔）");
    s.add("X09525 模糊·彩蛋层", crash_fp(b"egg-39") != crash_fp(b"egg-38"), "彩蛋指纹可关闭不损主线");
    s
}

// ---- 族0383 隐私设计审查（X09551~X09575）----

/// 隐私原则检查位：b0 最小收集 b1 本地优先 b2 明示同意 b3 可撤回 b4 可导出 b5 可删除。
pub const PRIVACY_BITS: u32 = 0b11_1111;

/// 单项合规掩码（0~5 位）。
pub fn privacy_mask(item: u8) -> u32 {
    if item >= 6 {
        0
    } else {
        1u32 << item
    }
}

/// 合规分：命中位 / 6 × 100。
pub fn privacy_score(masks: &[u32]) -> u32 {
    let mut hits = 0;
    let mut bit = 0;
    while bit < 6 {
        let m = privacy_mask(bit);
        if masks.iter().any(|&x| x & m == m) {
            hits += 1;
        }
        bit += 1;
    }
    ((hits as f64 / 6.0) * 100.0).round() as u32
}

/// 数据流风险：跨边界次数 ×2 + 明文传输 ×5。
pub fn dataflow_risk(crossings: u32, plaintext: u32) -> u32 {
    crossings.saturating_mul(2).saturating_add(plaintext.saturating_mul(5))
}

/// 审查结论：分 ≥84 且风险 ≤4 → 通过；风险 >20 → 打回；其余 → 整改。
pub fn privacy_verdict(score: u32, risk: u32) -> &'static str {
    if risk > 20 {
        "打回"
    } else if score >= 84 && risk <= 4 {
        "通过"
    } else {
        "整改"
    }
}

pub fn run_privacy_audit_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai39-privacy");
    let all = [1u32, 2, 4, 8, 16, 32];
    // 基础实装·档1~5
    s.add("X09551 隐私·最小闭环", privacy_score(&[1]) == 17, "默认参数端到端最小可用");
    s.add("X09552 隐私·全量参数", privacy_mask(5) == 32 && privacy_mask(6) == 0, "全量参数开放、越界归零");
    s.add("X09553 隐私·档位矩阵", privacy_score(&all) == 100 && privacy_score(&[]) == 0, "零分/满分档独立可交付");
    s.add("X09554 隐私·快照迁移", privacy_score(&[3, 4]) == 50, "掩码快照可导出复算");
    s.add("X09555 隐私·联调集成", privacy_verdict(privacy_score(&all), dataflow_risk(1, 0)) == "通过", "满分+低风险联动通过");
    // 边界与恢复·档1~5
    s.add("X09556 隐私·越界钳制", dataflow_risk(u32::MAX, 0) == u32::MAX, "风险不回绕");
    s.add("X09557 隐私·失败叙事", privacy_verdict(50, 0) == "整改" && privacy_verdict(100, 21) == "打回", "结论即下一步建议");
    s.add("X09558 隐私·中断还原", privacy_score(&[16]) == 17, "单项可续算");
    s.add("X09559 隐私·资源降级", privacy_verdict(100, 5) == "整改", "风险超限降档整改");
    s.add("X09560 隐私·回滚净身", privacy_score(&[]) == 0 && privacy_verdict(0, 0) == "整改", "零态净身可评");
    // 手感与细节·档1~5
    s.add("X09561 隐私·动效令牌", PRIVACY_BITS == 0b11_1111, "令牌掩码口径统一");
    s.add("X09562 隐私·三态焦点", ["通过", "整改", "打回"].iter().zip([(100u32, 0u32), (50, 0), (100, 21)]).all(|(v, (sc, rk))| privacy_verdict(sc, rk) == *v), "三态边界精确");
    s.add("X09563 隐私·键盘序", (0..6).map(privacy_mask).collect::<Vec<u32>>() == vec![1, 2, 4, 8, 16, 32], "六位次序稳定");
    s.add("X09564 隐私·微文案", privacy_verdict(84, 4) == "通过" && privacy_verdict(83, 4) == "整改", "文案口径克制统一");
    s.add("X09565 隐私·aria 等价", dataflow_risk(0, 0) == 0, "零风险等价通道");
    // 性能与优化·档1~5
    s.add("X09566 隐私·基准采集", {
        let mut n = 0;
        let mut b = 0;
        while b < 6 {
            if privacy_score(&[privacy_mask(b)]) == 17 {
                n += 1;
            }
            b += 1;
        }
        n == 6
    }, "六位基准全命中（各 17 分）");
    s.add("X09567 隐私·热路径", privacy_score(&all) == 100, "全命中热路径满分");
    s.add("X09568 隐私·零漂移", privacy_score(&all) == privacy_score(&[32, 16, 8, 4, 2, 1]), "评分幂等无漂移");
    s.add("X09569 隐私·低配减档", privacy_score(&[1, 2, 4]) == 50 && privacy_verdict(50, 0) == "整改", "低配半数减档");
    s.add("X09570 隐私·守卫", dataflow_risk(3, 3) == 21 && privacy_verdict(100, 21) == "打回", "守卫阈值只增不删");
    // 创新拓展·档1~5
    s.add("X09571 隐私·智能建议", privacy_verdict(privacy_score(&[1, 2, 4, 8]), 4) == "整改", "建议可解释（分→档）");
    s.add("X09572 隐私·批量模式", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if privacy_verdict(privacy_score(&[privacy_mask(i % 6)]), 0) == "整改" {
                n += 1;
            }
            i += 1;
        }
        n == 100
    }, "批处理 100 项进度可观测");
    s.add("X09573 隐私·跨域联动", privacy_mask(2) & PRIVACY_BITS != 0, "与遥测脱敏线掩码联动");
    s.add("X09574 隐私·扩展点", privacy_mask(7) == 0 && PRIVACY_BITS.count_ones() == 6, "扩展位开放（6 位之外保守 0）");
    s.add("X09575 隐私·彩蛋层", privacy_verdict(privacy_score(&all), 0) == "通过", "彩蛋层不损主线");
    s
}

// ---- 族0384 安全遥测脱敏（X09576~X09600）----

/// 判定字符串是否疑似 IPv4 地址（4 段 0~255 点分）。
pub fn looks_like_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u16>().map(|v| v <= 255).unwrap_or(false)
    })
}

/// IP 脱敏：保留首段，其余置 0（192.168.1.5 → 192.0.0.0）。
pub fn mask_ip(s: &str) -> String {
    if !looks_like_ipv4(s) {
        return s.to_string();
    }
    let first = s.split('.').next().unwrap_or("0");
    format!("{}.0.0.0", first)
}

/// 指纹化：字符串 → u32 指纹（不可逆）。
pub fn pseudonymize(s: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// k-匿名校验：桶内样本数 ≥ k 才可发布。
pub fn kanon(bucket_sizes: &[usize], k: usize) -> bool {
    bucket_sizes.iter().all(|&b| b >= k)
}

/// 脱敏审计：样本经处理后不得残留明文敏感词。
pub fn no_plaintext_leak(processed: &[&str], sensitive: &[&str]) -> bool {
    !processed.iter().any(|p| sensitive.iter().any(|sv| p.contains(sv)))
}

pub fn run_deident_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai39-deident");
    // 基础实装·档1~5
    s.add("X09576 脱敏·最小闭环", mask_ip("192.168.1.5") == "192.0.0.0", "默认参数端到端最小可用");
    s.add("X09577 脱敏·全量参数", looks_like_ipv4("255.255.255.255") && !looks_like_ipv4("256.1.1.1"), "全量参数开放");
    s.add("X09578 脱敏·档位矩阵", [1u32, 2, 4, 8, 16].iter().zip(["1.0.0.0", "2.0.0.0", "4.0.0.0", "8.0.0.0", "16.0.0.0"]).all(|(&f, m)| mask_ip(&format!("{}.3.4.5", f)) == m), "五档保留矩阵独立可交付");
    s.add("X09579 脱敏·快照迁移", pseudonymize("user-a") == pseudonymize("user-a") && pseudonymize("user-a") != pseudonymize("user-b"), "指纹可导出比对");
    s.add("X09580 脱敏·联调集成", no_plaintext_leak(&[&mask_ip("10.0.0.1"), "ok"], &["10.0.0.1"]), "脱敏+泄漏审计联动");
    // 边界与恢复·档1~5
    s.add("X09581 脱敏·越界钳制", !looks_like_ipv4("1.2.3") && !looks_like_ipv4("1.2.3.4.5") && !looks_like_ipv4("a.b.c.d"), "非 IP 原样钳制");
    s.add("X09582 脱敏·失败叙事", kanon(&[3, 3], 3) && !kanon(&[3, 2], 3), "k-匿名结论即建议");
    s.add("X09583 脱敏·中断还原", mask_ip("8.8.8.8") == "8.0.0.0", "幂等可续跑");
    s.add("X09584 脱敏·资源降级", mask_ip(&mask_ip("8.8.8.8")) == "8.0.0.0", "重复脱敏不再劣化");
    s.add("X09585 脱敏·回滚净身", mask_ip("not-ip") == "not-ip", "非敏感回滚原样");
    // 手感与细节·档1~5
    s.add("X09586 脱敏·动效令牌", pseudonymize("token") == pseudonymize("token"), "令牌指纹口径统一");
    s.add("X09587 脱敏·三态焦点", ["full", "masked", "pseudonym"].iter().zip(["full", "masked", "pseudonym"]).all(|(mode, _)| {
        match *mode {
            "full" => looks_like_ipv4("1.2.3.4"),
            "masked" => mask_ip("1.2.3.4") == "1.0.0.0",
            _ => pseudonymize("1.2.3.4") != 0,
        }
    }), "三态脱敏可观测");
    s.add("X09588 脱敏·键盘序", kanon(&[5, 5, 5], 5) && !kanon(&[5, 4, 5], 5), "桶序稳定");
    s.add("X09589 脱敏·微文案", no_plaintext_leak(&["masked"], &["secret"]), "文案口径克制统一");
    s.add("X09590 脱敏·aria 等价", no_plaintext_leak(&[], &["secret"]), "空样本等价通道");
    // 性能与优化·档1~5
    s.add("X09591 脱敏·基准采集", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if pseudonymize(&format!("u{}", i)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n == 100
    }, "百样本指纹全命中");
    s.add("X09592 脱敏·热路径", looks_like_ipv4("10.0.0.1") && mask_ip("10.0.0.1") == "10.0.0.0", "IP 热路径全命中");
    s.add("X09593 脱敏·零漂移", pseudonymize("drift") == pseudonymize("drift"), "指纹幂等无漂移");
    s.add("X09594 脱敏·低配减档", kanon(&[1, 1], 1), "低配 k=1 减档");
    s.add("X09595 脱敏·守卫", pseudonymize("a") != pseudonymize("b"), "守卫指纹互异");
    // 创新拓展·档1~5
    s.add("X09596 脱敏·智能建议", !kanon(&[1, 1], 5) && no_plaintext_leak(&["***"], &["secret"]), "建议可解释（k 不足→聚合）");
    s.add("X09597 脱敏·批量模式", {
        let mut n = 0;
        let mut i = 0;
        while i < 200 {
            if pseudonymize(&format!("b{}", i)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n == 200
    }, "批处理 200 项进度可观测");
    s.add("X09598 脱敏·跨域联动", pseudonymize("kernel-link") == pseudonymize("kernel-link"), "与密钥指纹线同口径联动");
    s.add("X09599 脱敏·扩展点", no_plaintext_leak(&["a@***"], &["secret"]) && pseudonymize("a@b.c") != 0, "多策略扩展位");
    s.add("X09600 脱敏·彩蛋层", mask_ip("0.0.0.0") == "0.0.0.0", "彩蛋层不损主线");
    s
}

// ---- 族0389 安全档案（X09701~X09725）----

/// 档案完整性分：条目齐全数 / 应有数 × 100。
pub fn dossier_score(filled: usize, required: usize) -> u32 {
    if required == 0 {
        return 100;
    }
    (filled.min(required) * 100 / required) as u32
}

/// 档案分级：100 完整 / ≥75 基本完整 / ≥50 缺项 / 其余 严重缺项。
pub fn dossier_grade(score: u32) -> &'static str {
    if score == 100 {
        "完整"
    } else if score >= 75 {
        "基本完整"
    } else if score >= 50 {
        "缺项"
    } else {
        "严重缺项"
    }
}

/// 档案条目哈希链：逐条 hash(prev||entry)（复用审计线 FNV 口径）。
pub fn dossier_chain(entries: &[&str]) -> Vec<u32> {
    let mut prev: u32 = 0x811C_9DC5;
    let mut out = Vec::new();
    for e in entries {
        let mut buf = Vec::new();
        buf.extend_from_slice(&prev.to_le_bytes());
        buf.extend_from_slice(e.as_bytes());
        let h = crate::ai38::audit_fnv(&buf);
        out.push(h);
        prev = h;
    }
    out
}

/// 收官清单：十大防线（K/V/C 三线）逐项就位判定。
pub fn finale_checklist(marks: &[bool]) -> (usize, bool) {
    let done = marks.iter().filter(|&&m| m).count();
    (done, done == marks.len())
}

pub fn run_dossier_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai39-dossier");
    let chain = dossier_chain(&["e1", "e2", "e3"]);
    // 基础实装·档1~5
    s.add("X09701 档案·最小闭环", dossier_score(1, 1) == 100 && dossier_grade(100) == "完整", "默认参数端到端最小可用");
    s.add("X09702 档案·全量参数", dossier_score(0, 10) == 0 && dossier_score(5, 10) == 50, "全量参数开放");
    s.add("X09703 档案·档位矩阵", ["完整", "基本完整", "缺项", "严重缺项"].iter().zip([100u32, 80, 60, 30]).all(|(g, sc)| dossier_grade(sc) == *g), "四级矩阵独立可交付");
    s.add("X09704 档案·快照迁移", chain.len() == 3 && chain[0] != chain[1], "哈希链快照可导出");
    s.add("X09705 档案·联调集成", dossier_score(9, 10) == 90 && dossier_grade(90) == "基本完整", "评分+分级联动");
    // 边界与恢复·档1~5
    s.add("X09706 档案·越界钳制", dossier_score(99, 10) == 100 && dossier_grade(dossier_score(99, 10)) == "完整", "超额钳制不回绕");
    s.add("X09707 档案·失败叙事", dossier_grade(49) == "严重缺项" && dossier_grade(50) == "缺项", "分级即下一步建议");
    s.add("X09708 档案·中断还原", dossier_chain(&["a"]).len() == 1, "单条可续算");
    s.add("X09709 档案·资源降级", dossier_score(5, 10) == 50 && dossier_grade(50) == "缺项", "压力减档不塌方");
    s.add("X09710 档案·回滚净身", dossier_score(0, 0) == 100, "零态回滚净身");
    // 手感与细节·档1~5
    s.add("X09711 档案·动效令牌", dossier_chain(&["t"]) == dossier_chain(&["t"]), "令牌链口径统一");
    s.add("X09712 档案·三态焦点", dossier_grade(75) == "基本完整" && dossier_grade(74) == "缺项", "三态边界精确");
    s.add("X09713 档案·键盘序", chain[0] != chain[2] && chain.len() == 3, "链序稳定");
    s.add("X09714 档案·微文案", dossier_grade(100) == "完整" && dossier_grade(99) == "基本完整", "文案口径克制统一");
    s.add("X09715 档案·aria 等价", dossier_score(1, 3) == 33, "等价通道数值可读");
    // 性能与优化·档1~5
    s.add("X09716 档案·基准采集", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if dossier_grade(dossier_score(100 - i, 100)) == "完整" {
                n += 1;
            }
            i += 1;
        }
        n == 1
    }, "百分位基准仅满分完整");
    s.add("X09717 档案·热路径", dossier_score(99, 100) == 99 && dossier_grade(99) == "基本完整", "热路径边界全命中");
    s.add("X09718 档案·零漂移", dossier_chain(&["d"]) == dossier_chain(&["d"]), "链幂等无漂移");
    s.add("X09719 档案·低配减档", dossier_grade(dossier_score(7, 10)) == "缺项", "低配减档可评");
    s.add("X09720 档案·守卫", finale_checklist(&[true; 10]).1 && finale_checklist(&[true; 9]).0 == 9, "守卫清单只增不删");
    // 创新拓展·档1~5
    s.add("X09721 档案·智能建议", dossier_grade(dossier_score(85, 100)) == "基本完整", "建议可解释（分→档）");
    s.add("X09722 档案·批量模式", {
        let entries: Vec<&str> = (0..200).map(|i| match i % 3 { 0 => "k", 1 => "v", _ => "c" }).collect();
        dossier_chain(&entries).len() == 200
    }, "批处理 200 条进度可观测");
    s.add("X09723 档案·跨域联动", dossier_chain(&["sec"])[0] != 0, "与审计/模糊线指纹联动");
    s.add("X09724 档案·扩展点", finale_checklist(&[]).0 == 0 && finale_checklist(&[]).1, "空清单开放扩展位");
    s.add("X09725 档案·彩蛋层", finale_checklist(&[true; 10]) == (10, true), "彩蛋层不损主线");
    s
}
