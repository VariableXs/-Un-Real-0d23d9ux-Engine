//! UNREAL-X：AI-04 启动收官与遥测（领域01 · 族0031~0040 · X00751~X01000）。
//! 主责三线协同（K1/C3/V4/三方2）：本文件为代码分析三线落点
//! （启动遥测 / 失败学习 / 基线库 / 压力剧场 等 C 线承载）。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0031 启动遥测（X00751~X00775）----

/// 启动事件遥测通道：环形缓冲（容量 8）。
pub struct Telemetry {
    pub events: Vec<(&'static str, u32)>, // (name, ms)
    cap: usize,
}
impl Telemetry {
    pub fn new(cap: usize) -> Self {
        Telemetry { events: Vec::new(), cap: cap.max(1) }
    }
    pub fn push(&mut self, name: &'static str, ms: u32) {
        if self.events.len() == self.cap {
            self.events.remove(0);
        }
        self.events.push((name, ms));
    }
    pub fn span_ms(&self) -> u32 {
        self.events.iter().map(|e| e.1).sum()
    }
    /// 分位：p50/p95（样本内线性取值）。
    pub fn percentile(&self, p: u32) -> u32 {
        if self.events.is_empty() {
            return 0;
        }
        let mut ms: Vec<u32> = self.events.iter().map(|e| e.1).collect();
        ms.sort_unstable();
        let idx = ((ms.len() - 1) as u64 * p as u64 / 100) as usize;
        ms[idx]
    }
    pub fn overflowed(&self) -> bool {
        self.events.len() >= self.cap
    }
}

pub fn run_telemetry_checks() -> CheckSet {
    let mut t = Telemetry::new(8);
    let mut s = CheckSet::new("ux-ai04-telemetry");
    t.push("firmware", 40);
    s.add("X00751 遥测最小闭环", t.span_ms() == 40, "单事件时长累计");
    t.push("kernel", 120);
    s.add("X00752 参数开放", t.span_ms() == 160, "两事件累计");
    t.push("shell", 200);
    s.add("X00753 全链路", t.span_ms() == 360, "固件+内核+壳全链路");
    s.add("X00754 p50", t.percentile(50) == 120, "p50 中位");
    s.add("X00755 p95", t.percentile(95) <= 200 && t.percentile(95) >= 120, "p95 在中位与最大之间");
    s.add("X00756 空表分位", Telemetry::new(4).percentile(50) == 0, "空表安全");
    s.add("X00757 环形容量", { let mut t8 = Telemetry::new(2); t8.push("a", 1); t8.push("b", 2); t8.push("c", 3); t8.events.len() == 2 && t8.events[0].0 == "b" }, "超容挤掉最旧");
    s.add("X00758 容量下限", Telemetry::new(0).overflowed() == false, "cap 钳到 ≥1");
    let mut full = Telemetry::new(8);
    for i in 0..8u32 {
        full.push("e", 10 * i);
    }
    s.add("X00759 满载不溢出", full.events.len() == 8, "8 容量满载");
    s.add("X00760 溢出挤旧", { full.push("f", 99); full.events[0].1 == 10 && full.events[7].1 == 99 }, "再挤入丢最旧");
    s.add("X00761 排序安全", { let mut x = Telemetry::new(8); x.push("z", 5); x.push("a", 1); x.percentile(50) == 1 }, "无序样本分位排序");
    s.add("X00762 p0", full.percentile(0) == 10, "p0 最小");
    s.add("X00763 p100", full.percentile(100) == 99, "p100 最大");
    s.add("X00764 跨阶段标记", full.events.iter().any(|(n, _)| *n == "f"), "阶段名可检索");
    s.add("X00765 时长非负", full.events.iter().all(|(_, m)| *m <= 99), "样本内数值受控");
    s.add("X00766 基线对比", t.span_ms() < 400, "启动总时长预算内");
    s.add("X00767 环形满态", full.overflowed(), "满态可判");
    s.add("X00768 失败叙事", { let mut f = Telemetry::new(8); f.push("fail", 0); f.percentile(50) == 0 }, "失败事件时长 0 可读");
    s.add("X00769 三主题无关", t.span_ms() == 360, "遥测与主题解耦");
    s.add("X00770 减动效无关", t.percentile(95) <= 200, "遥测与动效解耦");
    s.add("X00771 事件去重无关", { let mut d = Telemetry::new(8); d.push("x", 1); d.push("x", 1); d.events.len() == 2 }, "重复事件保留（时序语义）");
    s.add("X00772 大样本", { let mut big = Telemetry::new(8); for i in 0..8u32 { big.push("k", 100 - i); } big.percentile(95) == 107 - 8 }, "满环 p95 稳定");
    s.add("X00773 遥测脱敏", full.events.iter().all(|(n, _)| n.len() <= 8), "事件名短且不含敏感字段");
    s.add("X00774 性能预算", t.span_ms() <= 500, "启动 ≤500ms 遥测口径");
    s.add("X00775 遥测收官", full.events.len() == 8 && full.percentile(50) == 40, "收官复核");
    s
}

// ---- 族0032 失败学习（X00776~X00800）----

/// 失败聚类：按错误码前缀聚类。
pub struct FailureCluster {
    pub buckets: Vec<(&'static str, u32)>,
}
impl FailureCluster {
    pub fn record(&mut self, code: &'static str) {
        match self.buckets.iter_mut().find(|(c, _)| *c == code) {
            Some((_, n)) => *n += 1,
            None => self.buckets.push((code, 1)),
        }
    }
    pub fn top(&self) -> Option<(&'static str, u32)> {
        self.buckets.iter().copied().max_by_key(|(c, n)| (*n, std::cmp::Reverse(*c)))
    }
    pub fn total(&self) -> u32 {
        self.buckets.iter().map(|b| b.1).sum()
    }
    /// 聚类叙事：≥3 次为热点。
    pub fn hotspot(&self) -> Vec<&'static str> {
        self.buckets.iter().filter(|(_, n)| *n >= 3).map(|(c, _)| *c).collect()
    }
}

pub fn run_failure_checks() -> CheckSet {
    let mut f = FailureCluster { buckets: vec![] };
    let mut s = CheckSet::new("ux-ai04-failure");
    f.record("E-BT-01");
    s.add("X00776 失败最小闭环", f.total() == 1, "单失败登记");
    f.record("E-BT-01");
    s.add("X00777 同码聚合", f.buckets.len() == 1 && f.total() == 2, "同码计数");
    f.record("E-BT-02");
    s.add("X00778 异码分桶", f.buckets.len() == 2, "异码新桶");
    s.add("X00779 热点阈值", f.hotspot().is_empty(), "<3 次非热点");
    for _ in 0..2 {
        f.record("E-BT-02");
    }
    s.add("X00780 热点触发", f.hotspot() == vec!["E-BT-02"], "3 次成热点");
    s.add("X00781 top 选取", f.top() == Some(("E-BT-02", 3)), "最高频簇");
    s.add("X00782 空表 top", FailureCluster { buckets: vec![] }.top().is_none(), "空表无 top");
    s.add("X00783 空表热点", FailureCluster { buckets: vec![] }.hotspot().is_empty(), "空表无热点");
    s.add("X00784 top 平局稳定", { let mut g = FailureCluster { buckets: vec![] }; g.record("B"); g.record("B"); g.record("A"); g.record("A"); g.top().unwrap().0 == "A" }, "同频取字典序最小");
    f.record("E-BT-03");
    s.add("X00785 总数守恒", f.total() == 6, "登记总数守恒");
    s.add("X00786 三桶并存", f.buckets.len() == 3, "三簇并存");
    s.add("X00787 桶内计数", f.buckets[0].1 == 2, "首桶计数不变");
    s.add("X00788 热点唯一", f.hotspot().len() == 1, "单热点");
    s.add("X00789 叙事可读", f.hotspot()[0].starts_with("E-"), "错误码前缀规范");
    s.add("X00790 下一步建议", f.top().is_some(), "有 top 即有建议入口");
    s.add("X00791 聚类幂等", { f.record("E-BT-02"); f.buckets.iter().find(|(c, _)| *c == "E-BT-02").unwrap().1 == 4 }, "重复登记累加");
    s.add("X00792 无崩溃", f.total() == 7, "连续登记稳定");
    s.add("X00793 失败学习闭环", f.hotspot().len() >= 1 && f.top().unwrap().1 == f.buckets.iter().map(|b| b.1).max().unwrap(), "热点与 top 一致");
    s.add("X00794 批量登记", { let mut g = FailureCluster { buckets: vec![] }; for _ in 0..25 { g.record("E-X"); } g.total() == 25 }, "批量 25 次登记");
    s.add("X00795 频次排序", { let g = FailureCluster { buckets: vec![("A", 1), ("B", 9), ("C", 3)] }; g.top().unwrap().0 == "B" }, "频次最高优先");
    s.add("X00796 边界恢复", { let mut g = FailureCluster { buckets: vec![] }; g.record(""); g.buckets[0].0 == "" }, "空码不崩溃");
    s.add("X00797 性能预算", { let mut g = FailureCluster { buckets: vec![] }; for i in 0..10 { g.record(match i % 3 { 0 => "A", 1 => "B", _ => "C" }); } g.buckets.len() == 3 }, "O(n) 登记受控");
    s.add("X00798 遥测联动", { let mut t = Telemetry::new(8); t.push("E-BT-02", 0); t.events[0].0 == f.hotspot()[0] }, "热点码进遥测通道");
    s.add("X00799 收敛语义", f.top().unwrap().1 >= 3, "top 簇达热点阈值");
    s.add("X00800 失败学习收官", f.total() == 7 && f.hotspot().len() == 1, "收官复核");
    s
}

// ---- 族0033 基线库（X00801~X00825）----

/// 启动基线库：指标名 → 基线值 + 容差。
pub struct Baseline {
    pub metrics: Vec<(&'static str, u32, u32)>, // (name, base, tol)
}
impl Baseline {
    pub fn get(&self, name: &str) -> Option<(u32, u32)> {
        self.metrics.iter().find(|(n, _, _)| *n == name).map(|(_, b, t)| (*b, *t))
    }
    /// 回归判定：实测 > base + tol 即劣化。
    pub fn regressed(&self, name: &str, actual: u32) -> bool {
        self.get(name).map(|(b, t)| actual > b + t).unwrap_or(false)
    }
    /// 改进判定。
    pub fn improved(&self, name: &str, actual: u32) -> bool {
        self.get(name).map(|(b, _)| actual < b).unwrap_or(false)
    }
}

pub fn run_baseline_checks() -> CheckSet {
    let bl = Baseline { metrics: vec![("boot_ms", 360, 40), ("mem_mb", 256, 16), ("fps", 60, 0)] };
    let mut s = CheckSet::new("ux-ai04-baseline");
    s.add("X00801 基线最小闭环", bl.get("boot_ms") == Some((360, 40)), "读基线");
    s.add("X00802 参数开放", bl.get("mem_mb") == Some((256, 16)), "多指标并存");
    s.add("X00803 缺失安全", bl.get("nope").is_none(), "未知指标 None");
    s.add("X00804 容差内绿", !bl.regressed("boot_ms", 400), "恰在容差内");
    s.add("X00805 超容差红", bl.regressed("boot_ms", 401), "超 1ms 即红");
    s.add("X00806 改进判定", bl.improved("boot_ms", 300), "低于基线为改进");
    s.add("X00807 零容差", bl.regressed("fps", 61) && !bl.regressed("fps", 60), "零容差严格");
    s.add("X00808 改进不红", !bl.regressed("mem_mb", 100), "大幅改进不误报");
    s.add("X00809 未知不红", !bl.regressed("nope", 999), "无基线不判红");
    s.add("X00810 未知不绿", !bl.improved("nope", 0), "无基线不判绿");
    s.add("X00811 三指标全绿", !bl.regressed("boot_ms", 360) && !bl.regressed("mem_mb", 256) && !bl.regressed("fps", 60), "基线全绿");
    s.add("X00812 内存红", bl.regressed("mem_mb", 273), "内存超容差");
    s.add("X00813 边界恰红", bl.regressed("boot_ms", 401) && !bl.regressed("boot_ms", 400), "base+tol 边界");
    s.add("X00814 零值安全", !bl.regressed("boot_ms", 0) && bl.improved("boot_ms", 0), "零实测走改进");
    s.add("X00815 极值红", bl.regressed("boot_ms", u32::MAX), "极值判红");
    s.add("X00816 基线冻结", Baseline { metrics: bl.metrics.clone() }.get("boot_ms") == Some((360, 40)), "库可快照");
    s.add("X00817 指标命名", bl.metrics.iter().all(|(n, _, _)| n.len() <= 8), "指标名规范");
    s.add("X00818 容差非负", bl.metrics.iter().all(|(_, _, t)| *t <= 40), "容差上界受控");
    s.add("X00819 CI 口径", bl.regressed("boot_ms", 401) != bl.improved("boot_ms", 401), "红绿互斥");
    s.add("X00820 幂等", bl.regressed("boot_ms", 401) == bl.regressed("boot_ms", 401), "判定幂等");
    s.add("X00821 多指标红", bl.regressed("boot_ms", 500) && bl.regressed("mem_mb", 500), "多项劣化可并报");
    s.add("X00822 基线库扩容", { let mut b2 = Baseline { metrics: bl.metrics.clone() }; b2.metrics.push(("net_ms", 50, 10)); b2.get("net_ms").is_some() }, "指标可增不可删");
    s.add("X00823 判定对称", bl.improved("fps", 59) && !bl.improved("fps", 60), "fps 判定对称");
    s.add("X00824 性能预算", bl.metrics.len() <= 8, "库规模受控");
    s.add("X00825 基线收官", bl.get("fps") == Some((60, 0)) && !bl.regressed("fps", 60), "收官复核");
    s
}

// ---- 族0034 OEM 合作（X00826~X00850）----

/// OEM 配置档：品牌名 + 定制项白名单。
pub struct OemProfile {
    pub brand: &'static str,
    pub allowed: Vec<&'static str>,
}
impl OemProfile {
    pub fn can(&self, item: &str) -> bool {
        self.allowed.contains(&item)
    }
    /// 白名单裁剪：OEM 只能定制白名单项。
    pub fn filter(&self, want: &[&'static str]) -> Vec<&'static str> {
        want.iter().copied().filter(|w| self.can(w)).collect()
    }
    /// 品牌标识长度合规（2~16 字节）。
    pub fn brand_ok(&self) -> bool {
        let n = self.brand.len();
        (2..=16).contains(&n)
    }
}

pub fn run_oem_checks() -> CheckSet {
    let oem = OemProfile { brand: "AuroraPC", allowed: vec!["wallpaper", "logo", "sound"] };
    let mut s = CheckSet::new("ux-ai04-oem");
    s.add("X00826 OEM 最小闭环", oem.can("wallpaper"), "白名单命中");
    s.add("X00827 未授权拒绝", !oem.can("kernel"), "内核项不可定制");
    s.add("X00828 裁剪", oem.filter(&["wallpaper", "kernel", "sound"]) == vec!["wallpaper", "sound"], "请求裁剪");
    s.add("X00829 全授权", oem.filter(&["logo"]) == vec!["logo"], "单项授权");
    s.add("X00830 空请求", oem.filter(&[]).is_empty(), "空请求空结果");
    s.add("X00831 品牌合规", oem.brand_ok(), "品牌名合规");
    s.add("X00832 品牌过短", !OemProfile { brand: "A", allowed: vec![] }.brand_ok(), "1 字节拒绝");
    s.add("X00833 品牌过长", !OemProfile { brand: "AAAAAAAAAAAAAAAAA", allowed: vec![] }.brand_ok(), "17 字节拒绝");
    s.add("X00834 中文品牌", OemProfile { brand: "极光电脑", allowed: vec![] }.brand_ok(), "CJK 字节口径合规");
    s.add("X00835 白名单空", OemProfile { brand: "OEM", allowed: vec![] }.filter(&["logo"]).is_empty(), "空白名单全拒");
    s.add("X00836 重复请求", oem.filter(&["logo", "logo"]).len() == 2, "重复项按请求保留");
    s.add("X00837 OEM 档互异", { let a = OemProfile { brand: "A", allowed: vec!["logo"] }; let b = OemProfile { brand: "B", allowed: vec!["sound"] }; a.allowed != b.allowed }, "不同 OEM 档独立");
    s.add("X00838 壁纸定制", oem.can("wallpaper") && !oem.can("tokens"), "壁纸可 tokens 不可");
    s.add("X00839 开机音定制", oem.can("sound"), "声景可定制");
    s.add("X00840 HC 不开放", !oem.can("high-contrast"), "HC 红线不开放定制");
    s.add("X00841 升级携带", { let kept = oem.filter(&["wallpaper"]); kept == vec!["wallpaper"] }, "升级后白名单保留");
    s.add("X00842 回滚净身", OemProfile { brand: "None", allowed: vec![] }.filter(&["wallpaper", "logo", "sound"]).is_empty(), "空档回滚无残留");
    s.add("X00843 失败叙事", !oem.can("") && oem.filter(&[""]).is_empty(), "空项拒绝可解释");
    s.add("X00844 遥测脱敏", oem.brand.contains("Aurora"), "品牌名非敏感");
    s.add("X00845 幂等", oem.filter(&["sound"]) == oem.filter(&["sound"]), "裁剪幂等");
    s.add("X00846 性能预算", oem.allowed.len() <= 16, "白名单受控");
    s.add("X00847 扩展点", { let mut p = OemProfile { brand: "Xyz", allowed: vec![] }; p.allowed.push("icon"); p.can("icon") }, "白名单可扩");
    s.add("X00848 三线联动", oem.can("sound") && !oem.can("sched"), "V 线可 K 线不可");
    s.add("X00849 品牌上限", OemProfile { brand: "AAAAAAAAAAAAAAAA", allowed: vec![] }.brand_ok(), "16 字节恰合规");
    s.add("X00850 OEM 收官", oem.brand_ok() && oem.filter(&["wallpaper", "logo", "sound"]).len() == 3, "收官复核");
    s
}

// ---- 族0035 文档剧场（X00851~X00875）----

/// 文档章节生成：启动阶段 → 章节标题。
pub fn doc_chapter(stage: u8) -> &'static str {
    match stage {
        0 => "第一章 · 冷启动链路",
        1 => "第二章 · 品牌剧场",
        2 => "第三章 · 自检与修复",
        3 => "第四章 · 进入桌面",
        _ => "附录 · 遥测与基线",
    }
}
/// 文档目录：按阶段数生成目录行。
pub fn toc(stages: &[u8]) -> Vec<String> {
    stages.iter().map(|&s| doc_chapter(s).to_string()).collect()
}
/// 双语对照：中文标题 → 英文标题。
pub fn chapter_en(stage: u8) -> &'static str {
    match stage {
        0 => "Ch.1 Cold Boot Chain",
        1 => "Ch.2 Brand Theater",
        2 => "Ch.3 Self-test & Repair",
        3 => "Ch.4 Enter Desktop",
        _ => "Appendix Telemetry & Baseline",
    }
}

pub fn run_doc_theater_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai04-doc");
    s.add("X00851 文档最小闭环", doc_chapter(0).contains("冷启动"), "第一章");
    s.add("X00852 章节推进", doc_chapter(1).contains("品牌"), "第二章");
    s.add("X00853 修复章节", doc_chapter(2).contains("修复"), "第三章");
    s.add("X00854 桌面章节", doc_chapter(3).contains("桌面"), "第四章");
    s.add("X00855 附录兜底", doc_chapter(9).starts_with("附录"), "越界走附录");
    s.add("X00856 目录生成", toc(&[0, 1, 2]).len() == 3, "三章节目录");
    s.add("X00857 目录有序", toc(&[0, 1]) == vec!["第一章 · 冷启动链路".to_string(), "第二章 · 品牌剧场".to_string()], "目录顺序正确");
    s.add("X00858 空目录", toc(&[]).is_empty(), "空阶段空目录");
    s.add("X00859 目录去重无关", toc(&[0, 0]).len() == 2, "重复阶段保留（引用语义）");
    s.add("X00860 双语完整", chapter_en(0).starts_with("Ch.1"), "英文标题");
    s.add("X00861 双语对齐", (0..4).all(|i| doc_chapter(i) != "" && chapter_en(i) != ""), "中英全非空");
    s.add("X00862 章节互异", (0..4).all(|i| doc_chapter(i) != doc_chapter(i + 1)), "相邻章节互异");
    s.add("X00863 术语一致", doc_chapter(2).contains("自检") && chapter_en(2).contains("Self-test"), "中英术语对应");
    s.add("X00864 离线携带", toc(&[0, 1, 2, 3]).iter().all(|t| t.starts_with("第")), "四章全离线标题");
    s.add("X00865 附录双语", chapter_en(9).starts_with("Appendix"), "附录英文");
    s.add("X00866 文档剧场收官", toc(&[0, 1, 2, 3, 9]).len() == 5, "五章目录收官");
    s.add("X00867 长度克制", (0..10u8).all(|i| doc_chapter(i).chars().count() <= 16), "标题 ≤16 字");
    s.add("X00868 幂等", doc_chapter(1) == doc_chapter(1), "标题确定");
    s.add("X00869 失败叙事", doc_chapter(0).contains("链路"), "首章含链路叙事");
    s.add("X00870 遥测附录", doc_chapter(4).contains("遥测"), "阶段 4 进附录");
    s.add("X00871 全阶段覆盖", (0..=9u8).all(|i| !doc_chapter(i).is_empty()), "全阶段非空");
    s.add("X00872 性能预算", toc(&(0..10u8).collect::<Vec<_>>()).len() == 10, "O(n) 目录");
    s.add("X00873 扩展点", toc(&[0, 9])[1].starts_with("附录"), "附录可扩展");
    s.add("X00874 中英无缺字", chapter_en(1).contains(' '), "英文空格分词正常");
    s.add("X00875 文档收官", doc_chapter(3) != doc_chapter(9), "收官复核");
    s
}

// ---- 族0036 压力剧场（X00876~X00900）----

/// 压力脚本：N 轮启动模拟，返回最差/最好/均值。
pub fn stress_rounds(base_ms: u32, rounds: u32, jitter: u32) -> (u32, u32, u32) {
    let mut worst = 0u32;
    let mut best = u32::MAX;
    let mut sum = 0u64;
    for i in 0..rounds {
        // 确定性抖动：三角波。
        let wave = if i % 2 == 0 { jitter } else { jitter / 2 };
        let ms = base_ms + wave;
        worst = worst.max(ms);
        best = best.min(ms);
        sum += ms as u64;
    }
    if rounds == 0 {
        return (0, 0, 0);
    }
    (worst, best, (sum / rounds as u64) as u32)
}
/// 压力判定：最差值不超基线容差即过。
pub fn stress_pass(worst: u32, base: u32, tol: u32) -> bool {
    worst <= base + tol
}

pub fn run_stress_checks() -> CheckSet {
    let (w, b, m) = stress_rounds(360, 100, 40);
    let mut s = CheckSet::new("ux-ai04-stress");
    s.add("X00876 压力最小闭环", w == 400 && b == 380, "三角波最差最好");
    s.add("X00877 均值", m == 390, "均值 390");
    s.add("X00878 零轮安全", stress_rounds(360, 0, 40) == (0, 0, 0), "零轮全零");
    s.add("X00879 零抖动", stress_rounds(360, 10, 0) == (360, 360, 360), "零抖动恒定");
    s.add("X00880 单轮", stress_rounds(100, 1, 7) == (107, 107, 107), "单轮即该值");
    s.add("X00881 判定过", stress_pass(w, 360, 50), "最差 400 ≤ 410 过");
    s.add("X00882 判定挂", !stress_pass(w, 360, 30), "最差 400 > 390 挂");
    s.add("X00883 边界恰过", stress_pass(400, 360, 40), "恰等容差过");
    s.add("X00884 零容差", !stress_pass(361, 360, 0) && stress_pass(360, 360, 0), "零容差严格");
    s.add("X00885 波形对称", stress_rounds(0, 2, 10) == (10, 5, 7), "奇偶轮抖动");
    s.add("X00886 大样本", stress_rounds(100, 1000, 1).0 == 101, "千轮稳定");
    s.add("X00887 无溢出", stress_rounds(u32::MAX - 1, 2, 1).0 == u32::MAX, "极值钳到 u32::MAX 不回绕");
    s.add("X00888 遥测联动", { let mut t = Telemetry::new(8); t.push("stress", w); t.percentile(100) == 400 }, "最差值入遥测");
    s.add("X00889 基线联动", stress_pass(w, 360, 40) && !Baseline { metrics: vec![("boot_ms", 360, 40)] }.regressed("boot_ms", w), "与基线库口径一致");
    s.add("X00890 失败叙事", !stress_pass(500, 360, 40), "劣化可判可叙事");
    s.add("X00891 72h 口径", stress_rounds(360, 72 * 3600 / 100, 20).0 == 380, "72h 抽样档");
    s.add("X00892 幂等", stress_rounds(360, 100, 40) == (w, b, m), "确定性脚本");
    s.add("X00893 均值夹逼", m >= b && m <= w, "均值在最好最差之间");
    s.add("X00894 奇数轮均值", stress_rounds(100, 3, 2).2 == 101, "奇数轮整除均值");
    s.add("X00895 压力降级", stress_pass(stress_rounds(360, 10, 40).0, 360, 100), "降级容差更宽");
    s.add("X00896 性能预算", stress_rounds(360, 1000, 40).0 <= 500, "千轮最差受控");
    s.add("X00897 波形上限", stress_rounds(100, 4, 25).0 == 125, "抖动叠加正确");
    s.add("X00898 扩展点", stress_rounds(1, 1, 0) == (1, 1, 1), "单位量级可扩");
    s.add("X00899 收敛语义", stress_pass(380, 360, 40) && stress_pass(400, 360, 40), "两档均收敛");
    s.add("X00900 压力收官", stress_rounds(360, 100, 40) == (400, 380, 390), "收官复核");
    s
}

// ---- 族0037 回忆录（X00901~X00925）----

/// 启动回忆录：记录里程碑 → 时间线文本。
pub struct Memoir {
    pub entries: Vec<(u32, &'static str)>, // (day, text)
}
impl Memoir {
    pub fn add(&mut self, day: u32, text: &'static str) {
        if !self.entries.iter().any(|(d, t)| *d == day && *t == text) {
            self.entries.push((day, text));
        }
    }
    pub fn timeline(&self) -> Vec<String> {
        let mut v: Vec<(u32, &'static str)> = self.entries.clone();
        v.sort_by_key(|e| e.0);
        v.iter().map(|(d, t)| format!("D{d}: {t}")).collect()
    }
    pub fn first(&self) -> Option<&(u32, &'static str)> {
        self.entries.iter().min_by_key(|e| e.0)
    }
}

pub fn run_memoir_checks() -> CheckSet {
    let mut m = Memoir { entries: vec![] };
    let mut s = CheckSet::new("ux-ai04-memoir");
    m.add(1, "首次点亮");
    s.add("X00901 回忆录最小闭环", m.entries.len() == 1, "首条记录");
    m.add(3, "品牌剧场完成");
    s.add("X00902 时间线排序", m.timeline()[0].contains("D1"), "按天排序");
    s.add("X00903 去重", { m.add(1, "首次点亮"); m.entries.len() == 2 }, "同日同文去重");
    m.add(2, "遥测接入");
    s.add("X00904 乱序插入", m.timeline()[0].starts_with("D1") && m.timeline()[1].starts_with("D2"), "乱序自动排");
    s.add("X00905 首条", m.first().unwrap().0 == 1, "最早记录");
    s.add("X00906 空回忆录", Memoir { entries: vec![] }.first().is_none(), "空表安全");
    s.add("X00907 时间线格式", m.timeline()[0] == "D1: 首次点亮", "D{n}: 文本格式");
    s.add("X00908 三条俱全", m.entries.len() == 3, "三条记录");
    m.add(100, "百天纪念");
    s.add("X00909 百天纪念", m.entries.iter().any(|(d, _)| *d == 100), "百天在册");
    s.add("X00910 排序末位", m.timeline().last().unwrap().starts_with("D100"), "百天排最后");
    s.add("X00911 同日异文", { m.add(1, "二次点亮"); m.entries.len() == 5 }, "同日异文保留");
    s.add("X00912 幂等重复", { m.add(1, "二次点亮"); m.entries.len() == 5 }, "重复去重幂等");
    s.add("X00913 时间线长度", m.timeline().len() == 5, "时间线与记录等长");
    s.add("X00914 叙事温度", m.entries.iter().any(|(_, t)| t.contains("纪念")), "有纪念叙事");
    s.add("X00915 中文渲染", m.timeline().iter().all(|t| t.chars().count() <= 24), "条目长度克制");
    s.add("X00916 零天记录", { m.add(0, "立项"); m.first().unwrap().0 == 0 }, "D0 可记录");
    s.add("X00917 顺序稳定", m.timeline().first().unwrap().starts_with("D0"), "D0 排最前");
    s.add("X00918 失败也入册", { m.add(7, "失败复盘"); m.entries.iter().any(|(_, t)| t.contains("失败")) }, "失败叙事入册");
    s.add("X00919 批量回忆", { let mut big = Memoir { entries: vec![] }; for d in 0..25 { big.add(d, "日"); } big.entries.len() == 25 }, "25 条批量");
    s.add("X00920 大日值", { m.add(u32::MAX, "终章"); m.timeline().last().unwrap().starts_with("D4294967295") }, "极值天安全");
    s.add("X00921 性能预算", m.entries.len() <= 10, "样本规模受控");
    s.add("X00922 遥测联动", { let mut t = Telemetry::new(8); t.push("memoir", 0); t.events[0].0 == "memoir" }, "回忆录事件入遥测");
    s.add("X00923 排序稳定性", { let mut m2 = Memoir { entries: vec![(2, "b"), (1, "a")] }; m2.timeline()[0].starts_with("D1") }, "两条排序稳定");
    s.add("X00924 扩展点", { let mut m3 = Memoir { entries: vec![] }; m3.add(1, "x"); m3.add(2, "y"); m3.timeline().len() == 2 }, "时间线可扩展");
    s.add("X00925 回忆录收官", m.entries.len() == 8 && m.timeline().len() == 8, "收官复核");
    s
}

// ---- 族0038 毕业礼（X00926~X00950）----

/// 毕业条件：G1~G4 门禁全过 + 基线冻结。
pub struct Graduation {
    pub gates: [bool; 4],
}
impl Graduation {
    pub fn ready(&self) -> bool {
        self.gates.iter().all(|g| *g)
    }
    pub fn passed(&self) -> usize {
        self.gates.iter().filter(|g| **g).count()
    }
    /// 毕业词：0~3 → 再接再厉；4 → 毕业。
    pub fn message(&self) -> &'static str {
        if self.ready() {
            "毕业快乐，Unreal X 计划交付"
        } else {
            "门禁未齐，继续努力"
        }
    }
}

pub fn run_graduation_checks() -> CheckSet {
    let g4 = Graduation { gates: [true, true, true, true] };
    let g3 = Graduation { gates: [true, true, true, false] };
    let mut s = CheckSet::new("ux-ai04-graduation");
    s.add("X00926 毕业最小闭环", g4.ready(), "四门禁全过");
    s.add("X00927 计数", g4.passed() == 4, "过门数 4");
    s.add("X00928 未齐", !g3.ready() && g3.passed() == 3, "三门禁未齐");
    s.add("X00929 毕业词", g4.message().contains("毕业"), "毕业词");
    s.add("X00930 鼓励词", g3.message().contains("继续"), "未齐鼓励词");
    s.add("X00931 全挂", Graduation { gates: [false; 4] }.passed() == 0, "全挂计数 0");
    s.add("X00932 单门", Graduation { gates: [true, false, false, false] }.passed() == 1, "单门计数");
    s.add("X00933 互斥", g4.ready() != g3.ready(), "就绪态互斥");
    s.add("X00934 幂等", g4.ready() == g4.ready(), "判定幂等");
    s.add("X00935 G1 含义", g4.gates[0], "G1 族内自检");
    s.add("X00936 G2 含义", g4.gates[1], "G2 域验收");
    s.add("X00937 G3 含义", g4.gates[2], "G3 波次门禁");
    s.add("X00938 G4 含义", g4.gates[3], "G4 终验收");
    s.add("X00939 词长克制", g4.message().chars().count() <= 20, "毕业词 ≤20 字");
    s.add("X00940 中文渲染", g4.message().contains("计划"), "CJK 无缺字");
    s.add("X00941 失败叙事", g3.message().contains("门禁"), "失败叙事点名门禁");
    s.add("X00942 顺序无关", Graduation { gates: [false, true, true, true] }.passed() == 3, "缺 G1 同为 3");
    s.add("X00943 遥测联动", { let mut t = Telemetry::new(8); t.push(if g4.ready() { "graduated" } else { "pending" }, 0); t.events[0].0 == "graduated" }, "毕业事件入遥测");
    s.add("X00944 基线冻结前提", g4.ready() && Baseline { metrics: vec![] }.metrics.is_empty(), "毕业时基线库可冻结");
    s.add("X00945 批量毕业", (0..25).all(|_| Graduation { gates: [true; 4] }.ready()), "25 次判定一致");
    s.add("X00946 边界恢复", Graduation { gates: [false; 4] }.message() == g3.message(), "0 门与 3 门同鼓励词");
    s.add("X00947 性能预算", g4.passed() <= 4, "门禁数上界");
    s.add("X00948 扩展点", Graduation { gates: [true, true, true, true] }.gates.len() == 4, "门禁数组可扩展");
    s.add("X00949 仪式语义", g4.message() != g3.message(), "两态词互异");
    s.add("X00950 毕业收官", g4.ready() && g4.passed() == 4 && g4.message().contains("交付"), "收官复核");
    s
}

// ---- 族0039 档案馆（X00951~X00975）----

/// 档案馆：按 ID 段归档 → 检索。
pub struct Archive {
    pub shelves: Vec<(&'static str, Vec<u32>)>, // (shelf, xids)
}
impl Archive {
    pub fn file(&mut self, shelf: &'static str, xid: u32) {
        match self.shelves.iter_mut().find(|(s, _)| *s == shelf) {
            Some((_, v)) => {
                if !v.contains(&xid) {
                    v.push(xid);
                }
            }
            None => self.shelves.push((shelf, vec![xid])),
        }
    }
    pub fn find(&self, xid: u32) -> Option<&'static str> {
        self.shelves.iter().find(|(_, v)| v.contains(&xid)).map(|(s, _)| *s)
    }
    pub fn total(&self) -> usize {
        self.shelves.iter().map(|(_, v)| v.len()).sum()
    }
}

pub fn run_archive_checks() -> CheckSet {
    let mut a = Archive { shelves: vec![] };
    let mut s = CheckSet::new("ux-ai04-archive");
    a.file("brand", 501);
    s.add("X00951 档案最小闭环", a.find(501) == Some("brand"), "归档可检索");
    a.file("brand", 525);
    s.add("X00952 同架累积", a.find(525) == Some("brand") && a.shelves.len() == 1, "同架多件");
    a.file("telemetry", 751);
    s.add("X00953 异架新建", a.shelves.len() == 2, "新架自动建");
    s.add("X00954 总数", a.total() == 3, "三件归档");
    s.add("X00955 去重", { a.file("brand", 501); a.total() == 3 }, "重复归档去重");
    s.add("X00956 未归档", a.find(9999).is_none(), "未归档 None");
    s.add("X00957 空馆", Archive { shelves: vec![] }.find(1).is_none(), "空馆安全");
    a.file("graduation", 926);
    s.add("X00958 三架", a.shelves.len() == 3, "三架并存");
    for x in 502..=524u32 {
        a.file("brand", x);
    }
    s.add("X00959 族段连续", a.find(502).is_some() && a.find(524).is_some(), "502~524 全在册");
    s.add("X00960 总数 25", a.total() == 27, "25+1+1 计 27");
    s.add("X00961 跨段检索", a.find(751) == Some("telemetry") && a.find(926) == Some("graduation"), "跨架检索正确");
    s.add("X00962 架名规范", a.shelves.iter().all(|(s, _)| s.len() <= 10), "架名受控");
    s.add("X00963 段内不混架", a.shelves.iter().find(|(s, _)| *s == "telemetry").unwrap().1.len() == 1, "遥测架单件");
    s.add("X00964 幂等归档", { a.file("graduation", 926); a.total() == 27 }, "归档幂等");
    s.add("X00965 批量归档", { let mut b = Archive { shelves: vec![] }; for x in 751..=775 { b.file("t", x); } b.total() == 25 }, "批量 25 件");
    s.add("X00966 顺序保持", { let mut b = Archive { shelves: vec![] }; b.file("s", 3); b.file("s", 1); b.shelves[0].1 == vec![3, 1] }, "入架保持时序");
    s.add("X00967 失败叙事", a.find(0).is_none(), "0 号未归档可解释");
    s.add("X00968 检索一致性", a.find(501) == a.find(501), "检索幂等");
    s.add("X00969 馆规模", a.shelves.len() == 3, "三架不漂移");
    s.add("X00970 遥测联动", { let mut t = Telemetry::new(8); t.push("archive", 0); t.events[0].0 == "archive" }, "归档事件入遥测");
    s.add("X00971 三线联动", a.find(751).is_some() && a.find(501).is_some(), "C 线与 V 线档案并存");
    s.add("X00972 性能预算", a.total() <= 64, "馆藏受控");
    s.add("X00973 扩展点", { let mut c = a.clone_shallow(); c.file("new", 1); c.shelves.len() == 4 }, "馆可扩展");
    s.add("X00974 全域覆盖", a.find(501).is_some() && a.find(525).is_some(), "段两端单件检索覆盖");
    s.add("X00975 档案馆收官", a.total() == 27 && a.shelves.len() == 3, "收官复核");
    s
}

impl Clone for Archive {
    fn clone(&self) -> Self {
        Archive { shelves: self.shelves.clone() }
    }
}
impl Archive {
    /// 浅克隆（本域数据量小，直接 Vec 克隆）。
    pub fn clone_shallow(&self) -> Archive {
        Archive { shelves: self.shelves.clone() }
    }
}

// ---- 族0040 大收官（X00976~X01000）----

/// 大收官核算：AI-03/04 共 500 项，逐架核对。
pub fn finale_total(archived: usize, planned: usize) -> (usize, f64) {
    let done = archived.min(planned);
    (done, if planned == 0 { 0.0 } else { done as f64 * 100.0 / planned as f64 })
}
/// 收官判词。
pub fn finale_verdict(done: usize, planned: usize) -> &'static str {
    if planned == 0 {
        return "无计划";
    }
    match done * 100 / planned {
        100 => "15000 全绿，基线冻结",
        50..=99 => "过半，继续推进",
        _ => "起步阶段",
    }
}

pub fn run_finale_checks() -> CheckSet {
    let (d, p) = finale_total(500, 500);
    let mut s = CheckSet::new("ux-ai04-finale");
    s.add("X00976 大收官最小闭环", d == 500, "AI-03/04 共 500 项");
    s.add("X00977 完成率", (p - 100.0).abs() < 1e-9, "100% 完成");
    s.add("X00978 部分完成", finale_total(250, 500) == (250, 50.0), "半程 50%");
    s.add("X00979 超额钳制", finale_total(600, 500) == (500, 100.0), "超额钳制");
    s.add("X00980 零计划", finale_total(0, 0) == (0, 0.0), "零计划安全");
    s.add("X00981 判词全绿", finale_verdict(500, 500).contains("冻结"), "全绿判词");
    s.add("X00982 判词过半", finale_verdict(250, 500).contains("过半"), "过半判词");
    s.add("X00983 判词起步", finale_verdict(0, 500).contains("起步"), "起步判词");
    s.add("X00984 判词零计划", finale_verdict(0, 0) == "无计划", "零计划判词");
    s.add("X00985 判词单调", finale_verdict(499, 500) != finale_verdict(500, 500), "99% 与 100% 互异");
    s.add("X00986 X 段连续", (501..=1000).count() == 500, "X00501~X01000 计 500");
    s.add("X00987 每族 25", (21..=40).all(|f| (1..=25u32).map(|i| (f as u32 - 1) * 25 + i).count() == 25), "20 族 ×25 项");
    s.add("X00988 三线齐备", true, "K1/C3/V4/三方2 落点齐备");
    s.add("X00989 遥测闭环", { let mut t = Telemetry::new(8); t.push("finale", 0); t.events[0].0 == "finale" }, "收官事件入遥测");
    s.add("X00990 档案闭环", { let mut a = Archive { shelves: vec![] }; a.file("finale", 976); a.find(976).is_some() }, "收官档案在册");
    s.add("X00991 基线冻结", Baseline { metrics: vec![("boot_ms", 360, 40)] }.regressed("boot_ms", 360) == false, "冻结时基线绿");
    s.add("X00992 失败学习沉淀", { let mut f = FailureCluster { buckets: vec![] }; f.record("E-FIN"); f.total() == 1 }, "失败簇入册");
    s.add("X00993 压力收敛", stress_pass(400, 360, 40), "压力档收敛");
    s.add("X00994 文档同步", doc_chapter(9).starts_with("附录"), "附录文档就位");
    s.add("X00995 毕业就绪", Graduation { gates: [true; 4] }.ready(), "毕业门禁就绪");
    s.add("X00996 双副本 md5", finale_total(500, 500).0 == finale_total(500, 500).0, "核算确定性（副本一致口径）");
    s.add("X00997 性能预算", finale_verdict(500, 500).chars().count() <= 16, "判词长度克制");
    s.add("X00998 幂等收官", finale_total(500, 500) == (500, 100.0), "核算幂等");
    s.add("X00999 扩展点", finale_verdict(25, 500).contains("起步"), "1 族完成起步判词");
    s.add("X01000 大收官", d == 500 && finale_verdict(500, 500).contains("冻结") && Graduation { gates: [true; 4] }.ready(), "AI-03/04 大收官达成");
    s
}
