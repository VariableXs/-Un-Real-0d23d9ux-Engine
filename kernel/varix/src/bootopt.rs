//! F022 引导超时默认项 — boot menu countdown policy: default entry Varix,
//! validated timeout range, cmdline overrides. This is the kernel-side
//! contract the installer (AI-18) serializes into the bootloader config and
//! the boot screen consumes for its countdown line.

/// Default seconds the menu counts down before booting the default entry.
/// 5s per 双域总案·阶段0 requirement (was 3).
pub const DEFAULT_TIMEOUT_SECS: u32 = 5;
/// Hard lower bound (0 = boot instantly without showing the menu).
pub const MIN_TIMEOUT_SECS: u32 = 0;
/// Hard upper bound — beyond this the menu is considered stuck.
pub const MAX_TIMEOUT_SECS: u32 = 60;
/// Default entry booted when the countdown expires.
pub const DEFAULT_ENTRY: &str = "varix";

/// A 卡（VARIX + VARIABLE）加载完是否交接给 Windows 上的 Variable（需求 2）。
///
/// 默认**开**：内核里跑不了 Tauri（需要 Windows + WebView2），"进入 Variable"
/// 只能交接出去；关掉则落内核自绘 ushell（保留路径，也用于排障）。
pub const DEFAULT_HANDOFF_TO_VARIABLE: bool = true;

/// Boot menu options (F022 result).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootOptions {
    /// Countdown seconds; 0 means "boot immediately, no menu".
    pub timeout_secs: u32,
    /// Entry selected when the countdown expires.
    pub default_entry: &'static str,
    /// Whether the user supplied values (vs. defaults).
    pub customized: bool,
    /// cmdline explicitly set `boot_timeout=`（bootcfg 合并的逐字段优先级用）。
    pub customized_timeout: bool,
    /// cmdline explicitly set `boot_default=`（同上）。
    pub customized_entry: bool,
    /// A 卡加载完交接给 Windows 上的 Variable（需求 2）；见
    /// [`DEFAULT_HANDOFF_TO_VARIABLE`]。
    pub handoff_to_variable: bool,
    /// cmdline 显式设了 `handoff=`（bootcfg 合并的逐字段优先级用）。
    pub customized_handoff: bool,
}

impl Default for BootOptions {
    fn default() -> BootOptions {
        BootOptions {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            default_entry: DEFAULT_ENTRY,
            customized: false,
            customized_timeout: false,
            customized_entry: false,
            handoff_to_variable: DEFAULT_HANDOFF_TO_VARIABLE,
            customized_handoff: false,
        }
    }
}

impl BootOptions {
    /// Clamp a requested timeout into the valid range.
    pub fn clamp_timeout(secs: u32) -> u32 {
        secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
    }

    /// Build from the kernel command line (`boot_timeout=`, `boot_default=`).
    pub fn from_cmdline(cmdline: &str) -> BootOptions {
        let mut opts = BootOptions::default();
        let mut customized = false;
        for token in cmdline.split_whitespace() {
            if let Some(v) = token.strip_prefix("boot_timeout=") {
                if let Some(secs) = parse_u32(v) {
                    opts.timeout_secs = Self::clamp_timeout(secs);
                    opts.customized_timeout = true;
                    customized = true;
                }
            } else if let Some(v) = token.strip_prefix("boot_default=") {
                if !v.is_empty() && v.len() <= 32 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    opts.default_entry = match v {
                        "varix" => "varix",
                        "windows" => "windows",
                        "uefi" => "uefi",
                        _ => "other",
                    };
                    opts.customized_entry = true;
                    customized = true;
                }
            } else if let Some(v) = token.strip_prefix("handoff=") {
                // 交接开关（需求 2）：只认明确的 0/1 与常见拼法，含糊值一律
                // 忽略并保留默认——引导期不猜用户意图。
                match v {
                    "0" | "false" | "off" => {
                        opts.handoff_to_variable = false;
                        opts.customized_handoff = true;
                        customized = true;
                    }
                    "1" | "true" | "on" => {
                        opts.handoff_to_variable = true;
                        opts.customized_handoff = true;
                        customized = true;
                    }
                    _ => {}
                }
            }
        }
        opts.customized = customized;
        opts
    }

    /// Is the menu shown at all?
    pub fn menu_visible(&self) -> bool {
        self.timeout_secs > 0
    }

    /// The countdown line for the boot screen, e.g. `BOOTING VARIX IN 3`.
    /// Writes into `out` and returns the byte count.
    pub fn countdown_line(&self, remaining: u32, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let push = |b: &[u8], out: &mut [u8], n: &mut usize| {
            for &c in b {
                if *n < out.len() {
                    out[*n] = c;
                    *n += 1;
                }
            }
        };
        push(b"BOOTING ", out, &mut n);
        // Uppercase microcopy per the boot-line HUD spec.
        let entry: &[u8] = match self.default_entry {
            "varix" => b"VARIX",
            "windows" => b"WINDOWS",
            "uefi" => b"UEFI",
            _ => b"OTHER",
        };
        push(entry, out, &mut n);
        if self.timeout_secs > 0 {
            push(b" IN ", out, &mut n);
            // decimal seconds
            let mut digits = [0u8; 10];
            let mut w = 0;
            let mut v = remaining;
            if v == 0 {
                digits[0] = b'0';
                w = 1;
            } else {
                while v > 0 && w < digits.len() {
                    digits[w] = b'0' + (v % 10) as u8;
                    v /= 10;
                    w += 1;
                }
            }
            while w > 0 {
                w -= 1;
                if n < out.len() {
                    out[n] = digits[w];
                    n += 1;
                }
            }
        }
        n
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

// ---------------------------------------------------------------------------
// Target adapter
// ---------------------------------------------------------------------------

use crate::once::OnceLock;

static OPTS: OnceLock<BootOptions> = OnceLock::new();

/// Resolve the boot options from the Limine cmdline (F022 target path).
pub fn init() -> BootOptions {
    let opts = BootOptions::from_cmdline(crate::limine::cmdline());
    let _ = OPTS.set(opts);
    opts
}

pub fn options() -> BootOptions {
    OPTS.get().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_point_at_varix() {
        let o = BootOptions::default();
        assert_eq!(o.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(o.default_entry, "varix");
        assert!(!o.customized);
        assert!(o.menu_visible());
    }

    #[test]
    fn timeout_clamping() {
        assert_eq!(BootOptions::clamp_timeout(0), 0);
        assert_eq!(BootOptions::clamp_timeout(5), 5);
        assert_eq!(BootOptions::clamp_timeout(60), 60);
        assert_eq!(BootOptions::clamp_timeout(61), 60);
        assert_eq!(BootOptions::clamp_timeout(999_999), 60);
    }

    #[test]
    fn cmdline_overrides() {
        let o = BootOptions::from_cmdline("boot_timeout=10 boot_default=varix");
        assert_eq!(o.timeout_secs, 10);
        assert_eq!(o.default_entry, "varix");
        assert!(o.customized);

        let o2 = BootOptions::from_cmdline("boot_timeout=99");
        assert_eq!(o2.timeout_secs, 60); // clamped
        assert!(o2.customized);

        let o3 = BootOptions::from_cmdline("boot_timeout=0");
        assert_eq!(o3.timeout_secs, 0);
        assert!(!o3.menu_visible());

        // invalid values fall back to defaults
        let o4 = BootOptions::from_cmdline("boot_timeout=abc boot_default=");
        assert_eq!(o4.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert!(!o4.customized);
    }

    #[test]
    fn default_entry_whitelist() {
        assert_eq!(
            BootOptions::from_cmdline("boot_default=windows").default_entry,
            "windows"
        );
        assert_eq!(
            BootOptions::from_cmdline("boot_default=uefi").default_entry,
            "uefi"
        );
        // unknown entries are still valid menu entries
        assert_eq!(
            BootOptions::from_cmdline("boot_default=custom-entry").default_entry,
            "other"
        );
        // hostile values rejected
        assert_eq!(
            BootOptions::from_cmdline("boot_default=../etc/passwd").default_entry,
            "varix"
        );
    }

    #[test]
    fn countdown_lines() {
        let o = BootOptions::default();
        let mut out = [0u8; 64];
        let n = o.countdown_line(3, &mut out);
        assert_eq!(&out[..n], b"BOOTING VARIX IN 3");
        let n2 = o.countdown_line(0, &mut out);
        assert_eq!(&out[..n2], b"BOOTING VARIX IN 0");

        // zero timeout: no countdown suffix at all
        let z = BootOptions { timeout_secs: 0, ..BootOptions::default() };
        let n3 = z.countdown_line(0, &mut out);
        assert_eq!(&out[..n3], b"BOOTING VARIX");

        // buffer smaller than the line truncates without panicking
        let mut tiny = [0u8; 4];
        let n4 = o.countdown_line(3, &mut tiny);
        assert_eq!(n4, 4);
        assert_eq!(&tiny, b"BOOT");
    }

    #[test]
    fn handoff_defaults_to_on() {
        // 需求 2：A 卡默认交接给 Windows 上的 Variable（内核里跑不了 Tauri）。
        let d = BootOptions::default();
        assert!(d.handoff_to_variable, "交接默认必须为开");
        assert!(!d.customized_handoff, "默认值不算 cmdline 显式指定");
    }

    #[test]
    fn handoff_cmdline_overrides() {
        for off in ["handoff=0", "handoff=false", "handoff=off"] {
            let o = BootOptions::from_cmdline(off);
            assert!(!o.handoff_to_variable, "{off} 应关掉交接");
            assert!(o.customized_handoff, "{off} 应标记为显式指定");
            assert!(o.customized, "{off} 属于用户定制");
        }
        for on in ["handoff=1", "handoff=true", "handoff=on"] {
            let o = BootOptions::from_cmdline(on);
            assert!(o.handoff_to_variable, "{on} 应打开交接");
            assert!(o.customized_handoff);
        }
    }

    #[test]
    fn handoff_ignores_ambiguous_values() {
        // 含糊值（拼错/空/怪值）一律保留默认并**不**标记为显式指定——
        // 否则配置文件里的 handoff 会被一个垃圾 cmdline 静默吃掉。
        for weird in ["handoff=", "handoff=yes", "handoff=2", "handoff=maybe"] {
            let o = BootOptions::from_cmdline(weird);
            assert!(o.handoff_to_variable, "{} 应保留默认", weird);
            assert!(!o.customized_handoff, "{} 不该算显式指定", weird);
        }
    }
}
