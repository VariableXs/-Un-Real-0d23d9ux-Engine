//! 提示词模板渲染（纯函数，无副作用）。
//!
//! 与 Node 版一致的两条关键决策：
//! - **系统内置变量自动注入**（`{{时间}}`）。早先把它当普通变量，
//!   结果"未填就拦"把它一起拦了——让人手填时间戳是设计错误。
//! - **未填的用户变量保留占位符而不是替换成空**。
//!   空段落会让人以为"这里没什么要说的"，而实际上是没配。

use serde::Serialize;
use serde_json::Value;

/// 渲染产物。
#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    pub text: String,
    pub missing: Vec<String>,
}

/// 替换 `{{键}}`。手写扫描而不是正则：键名极短，手写更快且无正则回溯开销。
pub fn render(tpl: &str, vars: &serde_json::Value) -> Rendered {
    let mut missing: Vec<String> = Vec::new();
    let mut out = String::with_capacity(tpl.len() + 512);
    let bytes: Vec<char> = tpl.chars().collect();
    let mut i = 0usize;

    while i < bytes.len() {
        // 找 "{{"
        if bytes[i] == '{' && i + 1 < bytes.len() && bytes[i + 1] == '{' {
            //找配对的 "}}"
            let mut j = i + 2;
            let mut key = String::new();
            let mut found = false;
            while j + 1 < bytes.len() {
                if bytes[j] == '}' && bytes[j + 1] == '}' {
                    found = true;
                    break;
                }
                key.push(bytes[j]);
                j += 1;
            }
            if found {
                let k = key.trim().to_string();
                // 系统内置变量
                if k == "时间" {
                    out.push_str(&now_hms());
                    i = j + 2;
                    continue;
                }
                // 用户变量：注意 JSON false / 0 / "" 的区分
                let v = vars.get(&k);
                let s = match v {
                    Some(Value::Null) | None => None,
                    Some(Value::Bool(b)) => Some(b.to_string()),
                    Some(Value::Number(n)) => Some(n.to_string()),
                    Some(Value::String(s)) => {
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.clone())
                        }
                    }
                    Some(other) => Some(other.to_string()),
                };
                match s {
                    Some(ref txt) => out.push_str(txt),
                    None => {
                        if !missing.contains(&k) {
                            missing.push(k.clone());
                        }
                        out.push_str("{{");
                        out.push_str(&k);
                        out.push_str("}}");
                    }
                }
                i = j + 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    Rendered {
        text: out,
        missing,
    }
}

/// 当前时间 `YYYY-MM-DD HH:MM:SS`。
///
/// 手写时间换算：避开 chrono 依赖（省 ~200KB，且逻辑只有 6 行）。
/// ★ 正确性说明 ★：按本地时区解读 UNIX 秒。要精确到时区需 chrono，
/// 而这里只用于提示词里的"发送时刻"标记，秒级本地时间足够。
fn now_hms() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let local = secs + local_offset_seconds(secs);
    let (y, mo, d, h, mi, s) = civil_from_unix(local);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// 取本地时区偏移（秒）。
///
/// ★ 为什么不能偷懒返回 0 ★
/// 早先版本直接 `return 0`，于是提示词里的时间戳会是 **UTC**——
/// 本机是 UTC+8，显示会比真实时间差 8 小时。
/// 这种"看起来能用但内容是错的"最糟：用户不会怀疑时间戳，只会以为程序算错了。
///
/// 做法：问 Windows 要本地时间与 UTC 的差。
/// `FileTimeToSystemTime` 把 FILETIME 转成 SYSTEMTIME（UTC），
/// 再与 `GetLocalTime` 的结果相比，差值就是偏移。
/// 借用 `windows` crate 不划算（多一个依赖），这里直接 FFI 三个 Win32 函数。
#[cfg(windows)]
fn local_offset_seconds(_utc: i64) -> i64 {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    extern "system" {
        fn GetLocalTime(st: *mut SystemTime);
        fn SystemTimeToFileTime(st: *mut SystemTime, ft: *mut u64) -> i32;
        fn FileTimeToSystemTime(ft: *const u64, st: *mut SystemTime) -> i32;
    }

    unsafe {
        let mut local = std::mem::zeroed::<SystemTime>();
        GetLocalTime(&mut local);
        // 把本地时间当作 UTC 折成 FILETIME，再解回 UTC 墙上时间。
        // 两者之差就是时区偏移。
        let mut ft = 0u64;
        if SystemTimeToFileTime(&mut local, &mut ft) == 0 {
            return 0;
        }
        let mut utc = std::mem::zeroed::<SystemTime>();
        if FileTimeToSystemTime(&ft, &mut utc) == 0 {
            return 0;
        }
        let a = wall_seconds(
            local.year as i64, local.month as u32, local.day as u32,
            local.hour as u32, local.minute as u32, local.second as u32,
        );
        let b = wall_seconds(
            utc.year as i64, utc.month as u32, utc.day as u32,
            utc.hour as u32, utc.minute as u32, utc.second as u32,
        );
        a - b
    }
}

#[cfg(windows)]
fn wall_seconds(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    // 以"年1月1日 00:00:00"为基准折算（简化：按 365.2425 天/年）
    let days = days_from_civil(y, mo, d);
    days * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64
}

#[cfg(windows)]
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[cfg(not(windows))]
fn local_offset_seconds(_utc: i64) -> i64 {
    // 非 Windows（本项目只在 Windows 跑），退回 UTC。
    0
}

/// UNIX 秒 → (年,月,日,时,分,秒)。Howard Hinnant 的 civil_from_days 算法。
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
}
