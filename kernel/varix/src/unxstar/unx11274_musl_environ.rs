//! UNX-F11274 musl 环境变量消费（0x1f4a0 段）
//! 纯功能模型：environ 是「NAME=VALUE」串数组，getenv 线性查找；
//! 空环境与缺变量都返回空指针（不 panic），名字/重复名/值形态在登记
//! 时即判，封册后 environ 数组不再增长。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f4a0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4a0;
/// 名字非法（空、含 '='、含控制字符或超长）。
pub const EV_NAME: u32 = SEG_BASE + 0x01;
/// 同名变量重复登记。
pub const EV_DUP: u32 = SEG_BASE + 0x02;
/// 值含控制字符或超长。
pub const EV_VALUE: u32 = SEG_BASE + 0x03;
/// 查询或删除缺失。
pub const EV_MISS: u32 = SEG_BASE + 0x04;
/// 空环境渲染（environ 自省无行可出）。
pub const EV_EMPTY: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const EV_FROZEN: u32 = SEG_BASE + 0x06;
/// environ 数组已满。
pub const EV_CAP: u32 = SEG_BASE + 0x07;

/// environ 定容。
pub const ENV_CAP: usize = 32;
/// 名字长度上界。
pub const NAME_MAX: usize = 64;
/// 值长度上界（内核 execve 单串上限的保守值）。
pub const VALUE_MAX: usize = 200;
/// 静态缺省 PATH（空环境场景的兜底值）。
pub const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_74;

/// 一条环境变量：名字/值/是否导出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvEntry {
    pub name: String,
    pub value: String,
    pub exported: bool,
}

impl EnvEntry {
    /// 逐字渲染 `NAME=VALUE`。
    pub fn render(&self) -> String {
        format!("{}={}", self.name, self.value)
    }

    /// 计入 environ 串数组的字节量（含 '=' 与结尾 NUL 位）。
    pub fn total(&self) -> usize {
        self.name.len().wrapping_add(self.value.len()).wrapping_add(1)
    }
}

/// environ 账：登记 → 查找 → 删除 → 渲染。
#[derive(Clone, Debug)]
pub struct Environ {
    pub entries: Vec<EnvEntry>,
    pub accepts: u32,
    pub rejects: u32,
    pub misses: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Environ {
    pub fn new() -> Environ {
        Environ { entries: Vec::new(), accepts: 0, rejects: 0, misses: 0, frozen: false, root: None }
    }

    /// 名字合法：非空、无 '='、无控制字符、长度不超 NAME_MAX。
    fn name_ok(n: &str) -> bool {
        if n.is_empty() || n.len() > NAME_MAX || n.contains('=') {
            return false;
        }
        n.bytes().all(|b| b > 0x20 && b < 0x7f)
    }

    /// 值合法：长度不超 VALUE_MAX，只允许可打印字符与空格。
    fn value_ok(v: &str) -> bool {
        v.len() <= VALUE_MAX && v.bytes().all(|b| b == b' ' || (b >= 0x21 && b < 0x7f))
    }

    /// 登记一条环境变量。判定序：封册 → 名字 → 重复 → 值 → 定容。
    pub fn set(&mut self, name: &str, value: &str) -> Result<usize, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_FROZEN);
        }
        if !Self::name_ok(name) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_NAME);
        }
        if self.entries.iter().any(|e| e.name == name) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_DUP);
        }
        if !Self::value_ok(value) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_VALUE);
        }
        if self.entries.len() >= ENV_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_CAP);
        }
        self.entries.push(EnvEntry {
            name: String::from(name),
            value: String::from(value),
            exported: true,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.entries.len().wrapping_sub(1))
    }

    /// getenv 语义：命中返回值，未命中返回 None（等价空指针）。
    pub fn lookup(&self, name: &str) -> Option<&str> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.name == name {
                    return Some(e.value.as_str());
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 带账查询：未命中计入 misses 并返回 EV_MISS。
    pub fn get_counted(&mut self, name: &str) -> Result<String, u32> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.name == name {
                    return Ok(e.value.clone());
                }
            }
            k = k.wrapping_add(1);
        }
        self.misses = self.misses.wrapping_add(1);
        Err(EV_MISS)
    }

    /// unsetenv：删除命中项；未命中计 misses。
    pub fn unset(&mut self, name: &str) -> Result<EnvEntry, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EV_FROZEN);
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.name == name {
                    return Ok(self.entries.remove(k));
                }
            }
            k = k.wrapping_add(1);
        }
        self.misses = self.misses.wrapping_add(1);
        Err(EV_MISS)
    }

    /// environ 逐行渲染；空环境无行可出，返回 EV_EMPTY。
    pub fn render_all(&self) -> Result<String, u32> {
        if self.entries.is_empty() {
            return Err(EV_EMPTY);
        }
        let mut out = String::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                out.push_str(&e.render());
                out.push('\n');
            }
            k = k.wrapping_add(1);
        }
        Ok(out)
    }

    /// environ 串数组总字节量。
    pub fn total_bytes(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                n = n.wrapping_add(e.total());
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 批量导入 `K=V` 串；缺 '=' 的串按形态违规拒绝。
    pub fn import(&mut self, list: &[&str]) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < list.len() {
            if let Some(s) = list.get(k) {
                match s.find('=') {
                    Some(i) => {
                        if self.set(&s[..i], &s[i + 1..]).is_ok() {
                            n = n.wrapping_add(1);
                        }
                    }
                    None => {
                        self.rejects = self.rejects.wrapping_add(1);
                    }
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// FNV-1a：名字/值/导出位序敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                let mut j = 0usize;
                while j < e.name.len() {
                    if let Some(b) = e.name.as_bytes().get(j) {
                        h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
                h = (h ^ 0x3d).wrapping_mul(FNV_PRIME);
                let mut m = 0usize;
                while m < e.value.len() {
                    if let Some(b) = e.value.as_bytes().get(m) {
                        h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
                    }
                    m = m.wrapping_add(1);
                }
                h = (h ^ e.exported as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 典型 shell 环境（顺序即插入序）。
pub fn shell_environ() -> Environ {
    let mut e = Environ::new();
    let _ = e.set("PATH", DEFAULT_PATH);
    let _ = e.set("HOME", "/root");
    let _ = e.set("LANG", "C.UTF-8");
    let _ = e.set("TERM", "linux");
    e
}

/// 四探针：(a) 名字含 '='，(b) 重名，(c) 值含换行，(d) 封册后登记。
pub fn inject_four() -> [bool; 4] {
    let mut a = Environ::new();
    let bad_a = a.set("A=B", "x").err() == Some(EV_NAME);

    let mut b = shell_environ();
    let bad_b = b.set("PATH", "/bin").err() == Some(EV_DUP);

    let mut c = Environ::new();
    let bad_c = c.set("K", "v\nw").err() == Some(EV_VALUE);

    let mut d = shell_environ();
    let _ = d.freeze();
    let bad_d = d.set("EXTRA", "1").err() == Some(EV_FROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// Thirteen contract checks over environ traversal, getenv and gates.
pub fn run_unx11274_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11274");

    s.add("X1-SEGMENT-DOMAIN", SEG_BASE == 0x1f4a0 && EV_NAME == SEG_BASE + 0x01
        && EV_DUP == SEG_BASE + 0x02 && EV_VALUE == SEG_BASE + 0x03
        && EV_MISS == SEG_BASE + 0x04 && EV_EMPTY == SEG_BASE + 0x05
        && EV_FROZEN == SEG_BASE + 0x06 && EV_CAP == SEG_BASE + 0x07,
        "错误码落在 0x1f4a0 段连续偏移上");

    let e = shell_environ();
    s.add("X2-ENVIRON-WALK", e.entries.len() == 4 && e.accepts == 4
        && e.lookup("PATH") == Some(DEFAULT_PATH) && e.lookup("HOME") == Some("/root"),
        "四个变量按插入序可遍历，getenv 命中值一致");

    let mut m = shell_environ();
    let miss = m.get_counted("NOPE");
    let gone = m.unset("HOME");
    let again = m.unset("HOME");
    s.add("X3-MISS-SEMANTICS", miss.err() == Some(EV_MISS) && m.misses == 2
        && gone.is_ok() && m.lookup("HOME").is_none()
        && again.err() == Some(EV_MISS) && m.entries.len() == 3,
        "缺变量 getenv/unset 返回 EV_MISS 且不动 environ");

    let empty = Environ::new();
    s.add("X4-EMPTY-ENV", empty.render_all().err() == Some(EV_EMPTY)
        && empty.entries.is_empty() && empty.lookup("PATH").is_none()
        && !DEFAULT_PATH.is_empty(), "空环境无行可出且查找为空指针语义");

    let mut n = Environ::new();
    let n1 = n.set("", "x").err();
    let n2 = n.set("A B", "x").err();
    s.add("X5-NAME-GATE", n1 == Some(EV_NAME) && n2 == Some(EV_NAME) && n.rejects == 2,
        "空名与含空格名均被 EV_NAME 拒绝");

    let mut d = shell_environ();
    let ed = d.set("HOME", "/home/u").err();
    s.add("X6-DUP-GATE", ed == Some(EV_DUP) && d.entries.len() == 4 && d.rejects == 1,
        "重名登记被 EV_DUP 拒绝且原值不动");

    let mut v = Environ::new();
    let v1 = v.set("K", "v\tw").err();
    let long = "a".repeat(VALUE_MAX + 1);
    let v2 = v.set("L", &long).err();
    s.add("X7-VALUE-GATE", v1 == Some(EV_VALUE) && v2 == Some(EV_VALUE)
        && v.rejects == 2 && v.entries.is_empty(), "控制字符与超长值均被 EV_VALUE 拒绝");

    let mut im = Environ::new();
    let got = im.import(&["A=1", "B=2", "C=3", "BAD"]);
    let dump = im.render_all().unwrap_or_default();
    s.add("X8-IMPORT", got == 3 && im.entries.len() == 3 && im.rejects == 1
        && dump.contains("B=2\n"), "批量导入 3 条成功、缺 '=' 串计拒");

    let mut cap = Environ::new();
    let mut i = 0u32;
    while cap.entries.len() < ENV_CAP {
        let nm = format!("K{}", i);
        let _ = cap.set(&nm, "v");
        i = i.wrapping_add(1);
    }
    let ec = cap.set("OVER", "v").err();
    s.add("X9-CAP", cap.entries.len() == ENV_CAP && ec == Some(EV_CAP)
        && cap.accepts == ENV_CAP as u32 && cap.rejects == 1,
        "environ 满 32 条后追加被 EV_CAP 拒绝");

    let want = format!("PATH={}\nHOME=/root\nLANG=C.UTF-8\nTERM=linux\n", DEFAULT_PATH);
    let got = shell_environ().render_all().unwrap_or_default();
    s.add_dyn("X10-RENDER", got == want, &format!("渲染={}", got.replace('\n', "|")));

    let a = shell_environ();
    let mut b = shell_environ();
    let h0 = a.fold();
    let h1 = b.fold();
    if let Some(e) = b.entries.get_mut(0) {
        e.value.push_str("/x");
    }
    s.add("X11-FOLD", h0 != 0 && h0 == h1 && b.fold() != h0,
        "同构环境指纹相同，改值后指纹变化");

    let mut f = shell_environ();
    let r1 = f.freeze();
    let r2 = f.freeze();
    s.add("X12-FREEZE-IDEM", r1.is_some() && r1 == r2 && f.frozen,
        "封册返回根且二次调用同值");

    let mut z = shell_environ();
    let _ = z.freeze();
    let ez = z.set("NEW", "1").err();
    let dump = z.render_all().unwrap_or_default();
    s.add_dyn("X13-FROZEN-GATE", ez == Some(EV_FROZEN) && z.rejects == 1
        && dump.contains("HOME=/root"), &format!("封后写={:?} 拒绝={}", ez, z.rejects));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11274_flow() {
        let mut e = shell_environ();
        assert_eq!(e.entries.len(), 4);
        assert_eq!(e.accepts, 4);
        assert_eq!(e.rejects, 0);
        assert_eq!(e.lookup("PATH"), Some(DEFAULT_PATH));
        assert_eq!(e.lookup("NOPE"), None);
        assert_eq!(e.render_all().map(|d| d.lines().count()), Ok(4));
        assert!(e.total_bytes() > 40);
        assert!(e.get_counted("HOME").is_ok());
        let root = e.freeze();
        assert!(root.is_some());
        assert_eq!(root, e.freeze());
        assert!(e.frozen);
    }

    #[test]
    fn unx11274_gates() {
        let mut e = Environ::new();
        assert_eq!(e.set("", "v").err(), Some(EV_NAME));
        assert_eq!(e.set("A=B", "v").err(), Some(EV_NAME));
        assert!(e.set("A", "1").is_ok());
        assert_eq!(e.set("A", "2").err(), Some(EV_DUP));
        assert_eq!(e.set("B", "x\ny").err(), Some(EV_VALUE));
        assert_eq!(e.get_counted("K").err(), Some(EV_MISS));
        assert_eq!(e.unset("K").err(), Some(EV_MISS));
        assert_eq!(e.accepts, 1);
        assert_eq!(e.rejects, 4);
        assert_eq!(e.misses, 2);
        assert_eq!(e.unset("A").map(|x| x.value), Ok(String::from("1")));
        assert_eq!(e.entries.len(), 0);
        assert_eq!(Environ::new().render_all().err(), Some(EV_EMPTY));
        let _ = e.freeze();
        assert_eq!(e.set("C", "3").err(), Some(EV_FROZEN));
        assert_eq!(e.rejects, 5);

        let mut c = Environ::new();
        let mut i = 0u32;
        while c.entries.len() < ENV_CAP {
            let nm = format!("K{}", i);
            assert!(c.set(&nm, "v").is_ok());
            i = i.wrapping_add(1);
        }
        assert_eq!(c.set("OVER", "v").err(), Some(EV_CAP));
        assert_eq!(c.accepts, ENV_CAP as u32);
        assert_eq!(c.rejects, 1);
    }

    #[test]
    fn unx11274_checks_green() {
        let s = run_unx11274_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
