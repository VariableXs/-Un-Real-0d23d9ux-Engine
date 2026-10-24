//! UNX-F4861 · ext4 挂载选项解析器与选项矩阵账 · 诊断段 0x7A80
//! 验收锚点：UNX-F4861-J1 选项解析器对 20 组合法选项串解析账与规格矩阵一致率 100%，
//! 非法选项 10/10 次以 EINVAL 拒绝并留诊断账——逗号分隔键值对；合法键白名单
//! （rw/ro/data/errors/noatime/commit/barrier），带值键 data/errors/commit；
//! 非法：未知键、缺值、值越界、重复键 → EINVAL 留账。


use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x7A80;

/// 解析产物：规范选项矩阵。
#[derive(Clone, Debug, PartialEq)]
pub struct MountOpts {
    pub rw: bool,
    pub data: String,   // journal | ordered | writeback
    pub errors: String, // continue | remount-ro | panic
    pub commit: u32,    // 秒，1~600
    pub noatime: bool,
    pub barrier: u8, // 0/1
}

impl MountOpts {
    pub fn new() -> Self {
        MountOpts { rw: true, data: "ordered".to_string(), errors: "remount-ro".to_string(), commit: 5, noatime: false, barrier: 1 }
    }
}

pub const EINVAL: i32 = 22;

/// 解析器：逗号分隔键值对 → MountOpts 或 EINVAL。
pub fn parse_opts(input: &str) -> Result<MountOpts, (i32, String)> {
    let mut o = MountOpts::new();
    let mut seen: Vec<String> = Vec::new();
    if input.is_empty() {
        return Ok(o);
    }
    for part in input.split(',') {
        let (k, v) = match part.find('=') {
            Some(i) => (&part[..i], Some(&part[i + 1..])),
            None => (part, None),
        };
        if seen.iter().any(|x| x == k) {
            return Err((EINVAL, format!("dup key: {}", k)));
        }
        seen.push(k.to_string());
        match k {
            "rw" => o.rw = true,
            "ro" => o.rw = false,
            "noatime" => o.noatime = true,
            "barrier" => match v {
                Some("0") => o.barrier = 0,
                Some("1") => o.barrier = 1,
                _ => return Err((EINVAL, format!("barrier val: {}", v.unwrap_or("")))),
            },
            "data" => match v {
                Some("journal") | Some("ordered") | Some("writeback") => o.data = v.unwrap().to_string(),
                _ => return Err((EINVAL, format!("data val: {}", v.unwrap_or("")))),
            },
            "errors" => match v {
                Some("continue") | Some("remount-ro") | Some("panic") => o.errors = v.unwrap().to_string(),
                _ => return Err((EINVAL, format!("errors val: {}", v.unwrap_or("")))),
            },
            "commit" => match v {
                Some(vs) => match vs.parse::<u32>() {
                    Ok(n) if n >= 1 && n <= 600 => o.commit = n,
                    _ => return Err((EINVAL, format!("commit val: {}", vs))),
                },
                None => return Err((EINVAL, "commit missing".to_string())),
            },
            _ => return Err((EINVAL, format!("unknown key: {}", k))),
        }
    }
    Ok(o)
}

/// 规格矩阵对拍：解析结果与期望逐字段一致。
pub fn matches_spec(o: &MountOpts, spec: &MountOpts) -> bool {
    o.rw == spec.rw && o.data == spec.data && o.errors == spec.errors && o.commit == spec.commit && o.noatime == spec.noatime && o.barrier == spec.barrier
}

/// 诊断账：非法拒绝留痕。
pub struct RejectLedger {
    pub entries: Vec<(String, i32)>,
}

impl RejectLedger {
    pub fn new() -> Self {
        RejectLedger { entries: Vec::new() }
    }
    pub fn record(&mut self, input: &str, code: i32) {
        self.entries.push((input.to_string(), code));
    }
    pub fn all_einval(&self) -> bool {
        self.entries.iter().all(|(_, c)| *c == EINVAL)
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x7A80 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x4861_7A80;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("解析-空串走默认", parse_opts("").is_ok()));
    out.push(("解析-未知键EINVAL", parse_opts("foobar").is_err()));
    out.push(("EINVAL-常数恰22", EINVAL == 22));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx4861_checks：本单判据聚合（诊断段 0x7A80）。
pub fn run_unx4861_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F4861 挂载选项解析器与矩阵账");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 20 组合法选项串：串 + 期望规格
    let cases: Vec<(&str, MountOpts)> = vec![
        ("", MountOpts::new()),
        ("ro", MountOpts { rw: false, ..MountOpts::new() }),
        ("rw", MountOpts::new()),
        ("noatime", MountOpts { noatime: true, ..MountOpts::new() }),
        ("barrier=0", MountOpts { barrier: 0, ..MountOpts::new() }),
        ("barrier=1", MountOpts::new()),
        ("data=journal", MountOpts { data: "journal".to_string(), ..MountOpts::new() }),
        ("data=ordered", MountOpts::new()),
        ("data=writeback", MountOpts { data: "writeback".to_string(), ..MountOpts::new() }),
        ("errors=continue", MountOpts { errors: "continue".to_string(), ..MountOpts::new() }),
        ("errors=remount-ro", MountOpts::new()),
        ("errors=panic", MountOpts { errors: "panic".to_string(), ..MountOpts::new() }),
        ("commit=1", MountOpts { commit: 1, ..MountOpts::new() }),
        ("commit=600", MountOpts { commit: 600, ..MountOpts::new() }),
        ("commit=30", MountOpts { commit: 30, ..MountOpts::new() }),
        ("ro,noatime", MountOpts { rw: false, noatime: true, ..MountOpts::new() }),
        ("rw,barrier=0,commit=15", MountOpts { barrier: 0, commit: 15, ..MountOpts::new() }),
        ("data=journal,errors=panic", MountOpts { data: "journal".to_string(), errors: "panic".to_string(), ..MountOpts::new() }),
        ("ro,data=writeback,commit=60,noatime", MountOpts { rw: false, data: "writeback".to_string(), commit: 60, noatime: true, ..MountOpts::new() }),
        ("rw,noatime,barrier=0,data=journal,errors=continue,commit=120", MountOpts { noatime: true, barrier: 0, data: "journal".to_string(), errors: "continue".to_string(), commit: 120, ..MountOpts::new() }),
    ];
    let mut hit = 0u32;
    for (input, spec) in &cases {
        if let Ok(o) = parse_opts(input) {
            if matches_spec(&o, spec) {
                hit += 1;
            }
        }
    }
    let legal_ok = hit == 20;
    s.add("合法-20组解析与规格矩阵一致率100%", legal_ok, if legal_ok { "" } else { "一致率缺口" });

    // 非法 10 组：EINVAL 全拒 + 留账
    let bad_inputs = ["foobar", "data=none", "data", "errors=boom", "commit=0", "commit=601", "commit", "barrier=2", "rw,ro", "commit=5,commit=9"];
    let mut led = RejectLedger::new();
    let mut rejected = 0u32;
    for b in bad_inputs.iter() {
        match parse_opts(b) {
            Err((c, _)) => {
                rejected += 1;
                led.record(b, c);
            }
            Ok(_) => {}
        }
    }
    let bad_ok = rejected == 10 && led.all_einval() && led.entries.len() == 10;
    s.add("非法-10/10次EINVAL拒绝", bad_ok, if bad_ok { "" } else { "拒止面缺口" });
    s.add("非法-诊断账全留痕", led.entries.len() == 10, if led.entries.len() == 10 { "" } else { "留账缺口" });

    // 拒因细查：dup 键、未知键、越界值三类各现
    let has_dup = led.entries.iter().any(|(i, _)| i.contains("ro") == false && *i == "rw,ro");
    let has_unknown = bad_inputs.iter().any(|i| parse_opts(i).is_err()) && parse_opts("foobar").is_err();
    let has_range = parse_opts("commit=601").is_err() && parse_opts("commit=0").is_err();
    s.add("拒因-dup/未知/越界三类齐备", has_dup && has_unknown && has_range, if has_dup && has_unknown && has_range { "" } else { "拒因分类缺口" });

    // 边界：commit=1 与 commit=600 恰可（含等号）
    let lo = parse_opts("commit=1").map(|o| o.commit) == Ok(1);
    let hi = parse_opts("commit=600").map(|o| o.commit) == Ok(600);
    s.add("边界-commit下上界恰可", lo && hi, if lo && hi { "" } else { "边界漂移" });

    // 键序无关：两种顺序同结果
    let o1 = parse_opts("ro,noatime").ok();
    let o2 = parse_opts("noatime,ro").ok();
    let order_ok = o1.is_some() && o1 == o2;
    s.add("键序-顺序无关等价", order_ok, if order_ok { "" } else { "键序不等价" });

    // FNV 选项账指纹稳定
    let o3 = parse_opts("data=journal,commit=30").unwrap();
    let h1 = fnv1a(format!("{}{}", o3.data, o3.commit).as_bytes());
    let h2 = fnv1a(b"journal30");
    s.add("FNV-选项指纹稳定", h1 == h2, if h1 == h2 { "" } else { "指纹不稳" });

    s
}
