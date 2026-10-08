//! 动态分析引擎（#088~#099）—— AI-02 域二：运行期轨迹的录制/统计/可视化纯逻辑。

// F088 函数级插桩：探针事件 → 进入/退出记录（参数/返回值内联）。
pub fn instrument_trace(events: &[(&str, bool, &str)]) -> Vec<String> {
    events
        .iter()
        .map(|&(f, enter, payload)| {
            if enter {
                format!("enter {f}({payload})")
            } else {
                format!("exit {f}={payload}")
            }
        })
        .collect()
}

/// 插桩轨迹的括号配平检查（enter/exit 必须成对）。
pub fn trace_balanced(events: &[(&str, bool, &str)]) -> bool {
    let mut depth = 0isize;
    for &(_, enter, _) in events {
        depth += if enter { 1 } else { -1 };
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

// F089 分支覆盖率实时统计：命中边计数，未命中变灰。
pub fn branch_coverage(total: usize, hits: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let hit: Vec<usize> = hits.to_vec();
    let gray: Vec<usize> = (0..total).filter(|b| !hits.contains(b)).collect();
    (hit, gray)
}

// F090 执行路径录制与重放：录制完整轨迹，前缀重放确定。
pub struct Recording {
    pub events: Vec<String>,
}

pub fn record(events: &[&str]) -> Recording {
    Recording { events: events.iter().map(|e| e.to_string()).collect() }
}

pub fn replay(rec: &Recording, upto: usize) -> Vec<String> {
    rec.events.iter().take(upto).cloned().collect()
}

// F091 内存访问模式追踪：地址区间 → 热力图分桶计数。
pub fn mem_heatmap(acc: &[(usize, usize)], buckets: usize, span: usize) -> Vec<usize> {
    let mut heat = vec![0usize; buckets];
    for &(addr, size) in acc {
        let b = std::cmp::min(addr * buckets / span.max(1), buckets - 1);
        heat[b] += size;
    }
    heat
}

// F092 性能热点采样：栈顶计数 → 热点 Top-N。
pub fn hot_samples(samples: &[&str], top: usize) -> Vec<(String, usize)> {
    let mut cnt: Vec<(String, usize)> = Vec::new();
    for s in samples {
        match cnt.iter_mut().find(|(k, _)| k == s) {
            Some((_, c)) => *c += 1,
            None => cnt.push((s.to_string(), 1)),
        }
    }
    cnt.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    cnt.truncate(top);
    cnt
}

// F093 输入变异与边界探测：系统性边界值集合。
pub fn boundary_mutations(v: i64) -> Vec<i64> {
    let mut m = vec![0, 1, -1, i64::MAX, i64::MIN, v - 1, v + 1, v];
    m.sort();
    m.dedup();
    m
}

// F094 异常堆栈聚合：按堆栈签名分组，频率降序。
pub fn aggregate_stacks(stacks: &[&[&str]]) -> Vec<(String, usize)> {
    let mut cnt: Vec<(String, usize)> = Vec::new();
    for s in stacks {
        let key = s.join(";");
        match cnt.iter_mut().find(|(k, _)| *k == key) {
            Some((_, c)) => *c += 1,
            None => cnt.push((key, 1)),
        }
    }
    cnt.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    cnt
}

// F095 线程调度可视化：上下文切换时间戳 → 甘特区间。
pub fn gantt(switches: &[(u64, usize)], total: u64) -> Vec<(usize, u64, u64)> {
    let mut out = Vec::new();
    for w in switches.windows(2) {
        out.push((w[0].1, w[0].0, w[1].0));
    }
    if let Some(last) = switches.last() {
        out.push((last.1, last.0, total));
    }
    out
}

// F096 GC 暂停追踪：GC 事件 → 时间轴暂停区间。
pub struct GcPause {
    pub at: u64,
    pub dur_ms: u64,
    pub reason: &'static str,
}

pub fn gc_intervals(pauses: &[GcPause]) -> Vec<(u64, u64)> {
    pauses.iter().map(|p| (p.at, p.at + p.dur_ms)).collect()
}

// F097 IO 调用追踪：延迟与吞吐聚合。
pub fn io_stats(ops: &[(u64, usize)]) -> (u64, u64) {
    // 返回（总延迟 µs, 平均吞吐 bytes/op）
    let total: u64 = ops.iter().map(|(l, _)| l).sum();
    let bytes: usize = ops.iter().map(|(_, b)| b).sum();
    let avg = if ops.is_empty() { 0 } else { (bytes / ops.len()) as u64 };
    (total, avg)
}

// F098 系统调用拦截：按名聚合次数与耗时，耗时降序。
pub fn syscall_report(calls: &[(&str, u64)]) -> Vec<(String, usize, u64)> {
    let mut agg: Vec<(String, usize, u64)> = Vec::new();
    for (name, cost) in calls {
        match agg.iter_mut().find(|(k, _, _)| k == name) {
            Some((_, c, t)) => {
                *c += 1;
                *t += cost;
            }
            None => agg.push((name.to_string(), 1, *cost)),
        }
    }
    agg.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    agg
}

// F099 网络请求链路：分段 span → 瀑布图（按开始时间排序）。
pub struct Span {
    pub name: &'static str,
    pub start: u64,
    pub dur: u64,
}

pub fn waterfall(spans: &[Span]) -> Vec<(String, u64, u64)> {
    let mut v: Vec<(String, u64, u64)> =
        spans.iter().map(|s| (s.name.to_string(), s.start, s.start + s.dur)).collect();
    v.sort_by_key(|(_, s, _)| *s);
    v
}

pub fn run_dyna_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("dyna");

    // F088
    let tr = instrument_trace(&[("f", true, "x=1"), ("f", false, "2")]);
    s.add("F088 函数级插桩", tr == vec!["enter f(x=1)", "exit f=2"] && trace_balanced(&[("f", true, "1"), ("f", false, "1")]), "进入/退出/参数记录");
    // F089
    let (hit, gray) = branch_coverage(4, &[0, 2]);
    s.add("F089 分支覆盖率实时统计", hit == vec![0, 2] && gray == vec![1, 3], "命中计数+未命中变灰");
    // F090
    let rec = record(&["a", "b", "c"]);
    let r1 = replay(&rec, 2);
    let r2 = replay(&rec, 2);
    s.add("F090 执行路径录制与重放", r1 == vec!["a".to_string(), "b".to_string()] && r1 == r2, "前缀重放确定性");
    // F091
    let heat = mem_heatmap(&[(0, 10), (9, 5)], 10, 10);
    s.add("F091 内存访问模式追踪", heat[0] == 10 && heat[9] == 5 && heat.iter().sum::<usize>() == 15, "地址分桶热度");
    // F092
    let hot = hot_samples(&["f", "g", "f", "f", "g"], 2);
    s.add("F092 性能热点采样", hot[0] == ("f".into(), 3) && hot[1] == ("g".into(), 2), "栈顶计数 Top-N");
    // F093
    let muts = boundary_mutations(5);
    s.add("F093 输入变异与边界探测", muts.contains(&0) && muts.contains(&4) && muts.contains(&6), "边界值系统枚举");
    // F094
    let agg = aggregate_stacks(&[&["f", "g"], &["f", "g"], &["h"]]);
    s.add("F094 异常堆栈聚合", agg[0] == ("f;g".into(), 2), "按签名聚合+频率排序");
    // F095
    let g = gantt(&[(0, 0), (5, 1), (9, 0)], 12);
    s.add("F095 线程调度可视化", g == vec![(0, 0, 5), (1, 5, 9), (0, 9, 12)], "切换时间戳→甘特区间");
    // F096
    let gc = gc_intervals(&[GcPause { at: 100, dur_ms: 5, reason: "alloc" }]);
    s.add("F096 GC暂停追踪", gc == vec![(100, 105)], "触发原因+暂停区间");
    // F097
    let (lat, thr) = io_stats(&[(100, 40), (300, 60)]);
    s.add("F097 IO调用追踪", lat == 400 && thr == 50, "总延迟+吞吐");
    // F098
    let sr = syscall_report(&[("read", 10), ("write", 30), ("read", 5)]);
    s.add("F098 系统调用拦截", sr[0] == ("write".into(), 1, 30) && sr[1] == ("read".into(), 2, 15), "频率+耗时聚合降序");
    // F099
    let wf = waterfall(&[Span { name: "dns", start: 0, dur: 2 }, Span { name: "tls", start: 2, dur: 3 }]);
    s.add("F099 网络请求链路", wf[0].0 == "dns" && wf[1] == ("tls".into(), 2, 5), "请求链路瀑布图");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f088_unbalanced_trace() {
        assert!(!trace_balanced(&[("f", true, "1")]));
        assert!(!trace_balanced(&[("f", false, "1")]));
    }

    #[test]
    fn f090_full_replay() {
        let rec = record(&["a", "b"]);
        assert_eq!(replay(&rec, 10), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn f092_stable_tiebreak() {
        let hot = hot_samples(&["b", "a"], 2);
        assert_eq!(hot, vec![("a".into(), 1), ("b".into(), 1)]);
    }

    #[test]
    fn f093_dedup_boundaries() {
        assert_eq!(boundary_mutations(1).len(), 6); // MIN,-1,0,1,2,MAX 去重后 6 个
    }

    #[test]
    fn f099_empty_spans() {
        assert!(waterfall(&[]).is_empty());
    }
}
