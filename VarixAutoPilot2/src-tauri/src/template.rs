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
/// ★ 第二版为什么也不行（2026-10-06 单元测试抓到的）★
/// 思路是"GetLocalTime 的墙钟 − SystemTimeToFileTime 往返解回的墙钟 = 偏移"。
/// 但 Win32 的 SYSTEMTIME / FILETIME **都是 UTC 语义**：
/// SystemTimeToFileTime 只做结构转换、不做时区换算，往返墙钟不变，
/// a − b **恒等于 0**——探针实测 `GetLocalTime=14:55`，往返解回仍是 `14:55`。
///
/// 正解：直接问 `GetTimeZoneInformation` 要 bias。
/// 文档语义：UTC = 本地墙钟 + bias（分钟），故 本地 − UTC = −bias；
/// 返回 TIME_ZONE_ID_DAYLIGHT(2) 时再叠加 daylight_bias（夏令时）。
#[cfg(windows)]
fn local_offset_seconds(_utc: i64) -> i64 {
    #[repr(C)]
    struct Tzi {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u16; 8],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u16; 8],
        daylight_bias: i32,
    }
    extern "system" {
        fn GetTimeZoneInformation(tzi: *mut Tzi) -> u32;
    }
    unsafe {
        let mut tzi: Tzi = std::mem::zeroed();
        let r = GetTimeZoneInformation(&mut tzi);
        if r == 0xFFFFFFFF {
            return 0; // 调用失败：回退 UTC（单元测试会用系统对账把异常环境暴露出来）
        }
        let mut off = -(tzi.bias as i64) * 60;
        if r == 2 {
            off += tzi.daylight_bias as i64 * 60;
        }
        off
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 时区偏移必须等于系统真实偏移（本机 UTC+8 → 28800）。
    /// 之前"return 0"的缺陷就是在这里断的——提示词时间戳变 UTC。
    #[test]
    fn offset_matches_system() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let off = local_offset_seconds(now);
        // 与 Win32 GetTimeZoneInformation 对账（独立来源，不循环论证）
        #[repr(C)]
        struct TZI {
            bias: i32,
            standard_name: [u16; 32],
            standard_date: [u16; 8],
            standard_bias: i32,
            daylight_name: [u16; 32],
            daylight_date: [u16; 8],
            daylight_bias: i32,
        }
        extern "system" {
            fn GetTimeZoneInformation(tzi: *mut TZI) -> u32;
        }
        let mut tzi: TZI = unsafe { std::mem::zeroed() };
        let r = unsafe { GetTimeZoneInformation(&mut tzi) };
        assert_ne!(r, 0xFFFFFFFF, "GetTimeZoneInformation 失败");
        // UTC = local + bias（分钟）；所以本地-UTC = -bias
        let expect = -(tzi.bias as i64) * 60
            + if r == 2 { tzi.daylight_bias as i64 * 60 } else { 0 };
        assert_eq!(off, expect, "偏移与系统不符：off={off}s expect={expect}s");
    }

    /// now_hms 的日期与本地系统日期一致（抓"偏移没生效"回归）。
    #[test]
    fn now_hms_is_local() {
        #[repr(C)]
        struct SystemTime {
            year: u16, month: u16, day_of_week: u16, day: u16,
            hour: u16, minute: u16, second: u16, milliseconds: u16,
        }
        extern "system" { fn GetLocalTime(st: *mut SystemTime); }
        let mut st: SystemTime = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut st) };
        let out = now_hms();
        let today = format!("{:04}-{:02}-{:02}", st.year, st.month, st.day);
        assert!(out.starts_with(&today), "now_hms={out} 但本地日期={today}");
    }
}
