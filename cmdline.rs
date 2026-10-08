//! F017 引导参数解析 — kernel command line key-value framework.
//!
//! Grammar (whitespace-separated tokens):
//!   key=value    → value entry (`=` splits at the first occurrence)
//!   key          → flag entry with the empty value (present = true)
//!
//! Values are at most 128 bytes (longer ones truncate); the token count is
//! bounded to keep the whole framework allocation-free. Lookup is linear —
//! boot cmdlines are tiny.

/// Maximum tokens parsed from one command line.
pub const MAX_TOKENS: usize = 32;
/// Maximum bytes kept per value.
pub const MAX_VALUE: usize = 128;

#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// Parsed command line (borrows the source string — the Limine cmdline is a
/// static C string, so this is zero-copy).
#[derive(Clone, Copy, Debug)]
pub struct Cmdline<'a> {
    src: &'a str,
    entries: [Entry<'a>; MAX_TOKENS],
    count: usize,
}

impl<'a> Cmdline<'a> {
    /// Parse a command line string.
    pub fn parse(src: &'a str) -> Cmdline<'a> {
        let mut entries = [Entry { key: "", value: "" }; MAX_TOKENS];
        let mut count = 0usize;
        for token in src.split_whitespace() {
            if count >= MAX_TOKENS || token.is_empty() {
                break;
            }
            // Truncate pathologically long tokens defensively.
            let token = if token.len() > MAX_VALUE * 2 {
                &token[..MAX_VALUE * 2]
            } else {
                token
            };
            match token.split_once('=') {
                Some((k, v)) => {
                    let v = if v.len() > MAX_VALUE { &v[..MAX_VALUE] } else { v };
                    // Last occurrence wins (overwrite semantics, like Linux).
                    let mut replaced = false;
                    for e in entries[..count].iter_mut() {
                        if e.key == k {
                            e.value = v;
                            replaced = true;
                            break;
                        }
                    }
                    if !replaced {
                        entries[count] = Entry { key: k, value: v };
                        count += 1;
                    }
                }
                None => {
                    let mut replaced = false;
                    for e in entries[..count].iter_mut() {
                        if e.key == token {
                            replaced = true;
                            break;
                        }
                    }
                    if !replaced {
                        entries[count] = Entry { key: token, value: "" };
                        count += 1;
                    }
                }
            }
        }
        Cmdline { src, entries, count }
    }

    pub fn source(&self) -> &'a str {
        self.src
    }

    pub fn entries(&self) -> &[Entry<'_>] {
        &self.entries[..self.count]
    }

    /// Look up a key's value. Flags (bare tokens) return `Some("")`.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.entries[..self.count]
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.value)
    }

    /// Is the flag present? `flag` alone → true; `flag=off|0|false|no` → false.
    pub fn flag(&self, key: &str) -> bool {
        match self.get(key) {
            None => false,
            Some("") => true,
            Some(v) => !matches!(v, "off" | "0" | "false" | "no"),
        }
    }

    /// Parse a u64 value (decimal, or 0x-prefixed hex). Honors the `k`/`m`/`g`
    /// size suffixes (`mem=64m`).
    pub fn u64_of(&self, key: &str) -> Option<u64> {
        let v = self.get(key)?;
        parse_u64(v)
    }

    /// First positional (non `=`) token, for future `init=` style args.
    pub fn first_flag(&self) -> Option<&'a str> {
        self.entries[..self.count]
            .iter()
            .find(|e| e.value.is_empty())
            .map(|e| e.key)
    }
}

/// Parse one integer value with optional 0x hex prefix and k/m/g suffix.
pub fn parse_u64(v: &str) -> Option<u64> {
    let (num, mul) = match v.as_bytes().last() {
        Some(b'k') | Some(b'K') => (&v[..v.len() - 1], 1024u64),
        Some(b'm') | Some(b'M') => (&v[..v.len() - 1], 1024 * 1024),
        Some(b'g') | Some(b'G') => (&v[..v.len() - 1], 1024 * 1024 * 1024),
        _ => (v, 1),
    };
    if num.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(hex) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
        (hex, 16)
    } else {
        (num, 10)
    };
    if digits.is_empty() {
        return None; // bare "0x" / "0xk" style prefixes carry no digits
    }
    let mut acc: u64 = 0;
    for b in digits.bytes() {
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' if radix == 16 => (b - b'a' + 10) as u64,
            b'A'..=b'F' if radix == 16 => (b - b'A' + 10) as u64,
            _ => return None,
        };
        acc = acc.checked_mul(radix)?.checked_add(d)?;
    }
    acc.checked_mul(mul)
}

// ---------------------------------------------------------------------------
// Target adapter — parse the Limine cmdline once at boot
// ---------------------------------------------------------------------------

use crate::once::OnceLock;

static CMDLINE: OnceLock<Cmdline<'static>> = OnceLock::new();

/// Parse and install the global command line (returns the parse result).
pub fn init() -> Cmdline<'static> {
    let parsed = Cmdline::parse(crate::limine::cmdline());
    let _ = CMDLINE.set(parsed);
    parsed
}

pub fn get(key: &str) -> Option<&'static str> {
    CMDLINE.get().and_then(|c| c.get(key))
}

pub fn flag(key: &str) -> bool {
    CMDLINE.get().map(|c| c.flag(key)).unwrap_or(false)
}

pub fn u64_of(key: &str) -> Option<u64> {
    CMDLINE.get().and_then(|c| c.u64_of(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_values_and_flags() {
        let c = Cmdline::parse("root=varix log=debug kaslr quiet ro");
        assert_eq!(c.get("root"), Some("varix"));
        assert_eq!(c.get("log"), Some("debug"));
        assert!(c.flag("kaslr")); // bare flag
        assert!(c.flag("quiet"));
        assert!(c.flag("ro"));
        assert!(!c.flag("nosuch"));
        assert_eq!(c.entries().len(), 5);
        assert_eq!(c.first_flag(), Some("kaslr"));
    }

    #[test]
    fn last_occurrence_wins() {
        let c = Cmdline::parse("log=info log=trace");
        assert_eq!(c.get("log"), Some("trace"));
        assert_eq!(c.entries().len(), 1);
    }

    #[test]
    fn flag_value_semantics() {
        let c = Cmdline::parse("a b=off c=0 d=false e=no f=on g=1");
        assert!(c.flag("a"));
        assert!(!c.flag("b"));
        assert!(!c.flag("c"));
        assert!(!c.flag("d"));
        assert!(!c.flag("e"));
        assert!(c.flag("f"));
        assert!(c.flag("g"));
    }

    #[test]
    fn values_with_equals_signs() {
        let c = Cmdline::parse("cmd=a=b");
        // first '=' splits; the rest is part of the value
        assert_eq!(c.get("cmd"), Some("a=b"));
    }

    #[test]
    fn integer_parsing() {
        assert_eq!(parse_u64("42"), Some(42));
        assert_eq!(parse_u64("0x10"), Some(16));
        assert_eq!(parse_u64("0X1F"), Some(31));
        assert_eq!(parse_u64("2k"), Some(2048));
        assert_eq!(parse_u64("4m"), Some(4 * 1024 * 1024));
        assert_eq!(parse_u64("1g"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_u64("16K"), Some(16 * 1024));
        assert_eq!(parse_u64(""), None);
        assert_eq!(parse_u64("k"), None);
        assert_eq!(parse_u64("12x"), None);
        assert_eq!(parse_u64("0x"), None);
        assert_eq!(parse_u64("99999999999999999999"), None); // overflow
    }

    #[test]
    fn u64_of_through_cmdline() {
        let c = Cmdline::parse("timeout=10 slide=0x400000 mem=64m");
        assert_eq!(c.u64_of("timeout"), Some(10));
        assert_eq!(c.u64_of("slide"), Some(0x400000));
        assert_eq!(c.u64_of("mem"), Some(64 * 1024 * 1024));
        assert_eq!(c.u64_of("missing"), None);
    }

    #[test]
    fn empty_and_whitespace_only() {
        let c = Cmdline::parse("");
        assert_eq!(c.entries().len(), 0);
        assert_eq!(c.get("x"), None);
        let c2 = Cmdline::parse("   \t  ");
        assert_eq!(c2.entries().len(), 0);
        let c3 = Cmdline::parse("=value");
        // key would be empty — split_once gives ("", "value"); empty keys are
        // still recorded but effectively unfindable. Verify no panic.
        assert!(c3.entries().len() <= 1);
    }

    #[test]
    fn token_capacity_bound() {
        let mut src = String::new();
        for i in 0..64 {
            if i > 0 {
                src.push(' ');
            }
            src.push_str(&std::format!("k{}=v{}", i, i));
        }
        let c = Cmdline::parse(&src);
        assert_eq!(c.entries().len(), MAX_TOKENS);
        assert_eq!(c.get("k0"), Some("v0"));
        assert_eq!(c.get(&std::format!("k{}", MAX_TOKENS - 1)).unwrap(), std::format!("v{}", MAX_TOKENS - 1));
        assert_eq!(c.get(&std::format!("k{}", MAX_TOKENS)), None);
    }

    #[test]
    fn long_values_truncate() {
        let long = "x".repeat(500);
        let src = std::format!("key={}", long);
        let c = Cmdline::parse(&src);
        assert_eq!(c.get("key").map(|v| v.len()), Some(MAX_VALUE));
    }
}
