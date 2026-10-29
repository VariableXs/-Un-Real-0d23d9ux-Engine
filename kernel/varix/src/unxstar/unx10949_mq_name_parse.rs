//! UNX-F10949 mq 名字解析错误矩阵（路径/权限）（0x1d620 段）
//!
//! 纯功能模型：POSIX 消息队列名字解析裁决板。
//! 两错误面：① 名字非法（不以 / 起头、越 NAME_MAX、首字符后含 /、
//! 含非法字符）→ EINVAL；② 名字空间无检索权 → EACCES。
//! 反面千轮样本只走合法名与有权域，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d620;
/// 名字非法
pub const NB_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字空间无检索权
pub const NB_EACCES: u32 = SEG_BASE + 0x02;
/// 名字过长（EINVAL 子面）
pub const NB_2LONG: u32 = SEG_BASE + 0x03;
/// 板内对账失败
pub const NB_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 名字最大长度（含前导 /）
pub const NAME_MAX: usize = 64;
/// 名字空间检索权位
pub const NS_SEARCH: u32 = 0x4;
/// 反面验收轮数
pub const NB_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 名字解析结果。
#[derive(Clone, Debug, PartialEq)]
pub struct NameRes {
    /// 规范化名（去前导 / 后的本名）
    pub bare: String,
    /// 是否合法
    pub ok: bool,
}

/// 名字空间条目。
#[derive(Clone, Debug)]
pub struct NsDir {
    /// 名字空间名
    pub label: String,
    /// 权限位
    pub mode: u32,
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct NbLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// 名字解析裁决板。
#[derive(Clone, Debug)]
pub struct NameBoard {
    /// 名字空间（多域权限）
    pub dirs: Vec<NsDir>,
    /// 已解析成功的名字账
    pub names: Vec<String>,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// 逐轮日志
    pub log: Vec<NbLog>,
}

impl NameBoard {
    /// 建板：给定名字空间。
    pub fn new(dirs: Vec<NsDir>) -> Self {
        NameBoard {
            dirs,
            names: Vec::new(),
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            eacces_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == NB_EINVAL || x == NB_2LONG => {
                self.einval_hits = self.einval_hits.wrapping_add(1)
            }
            x if x == NB_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(NbLog { round, verdict });
    }

    /// 静态名字校验：返回 Ok(bare) 或 Err(码)。
    pub fn check_name(name: &str) -> Result<String, u32> {
        if name.is_empty() || !name.starts_with('/') {
            return Err(NB_EINVAL);
        }
        if name.len() > NAME_MAX {
            return Err(NB_2LONG);
        }
        let bare = &name[1..];
        if bare.is_empty() || bare.contains('/') {
            return Err(NB_EINVAL);
        }
        let bytes = bare.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            let okc = (c >= b'a' && c <= b'z')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'0' && c <= b'9')
                || c == b'_'
                || c == b'-'
                || c == b'.';
            if !okc {
                return Err(NB_EINVAL);
            }
            i += 1;
        }
        Ok(bare.to_string())
    }

    /// 裁决一次名字解析。判定序：
    /// ① 静态校验 → EINVAL/2LONG；② 域检索权 → EACCES；③ 入账。
    pub fn resolve(&mut self, domain: usize, name: &str, round: usize) -> Result<String, u32> {
        let bare = NameBoard::check_name(name)?;
        if domain >= self.dirs.len() {
            self.book(round, NB_EINVAL);
            return Err(NB_EINVAL);
        }
        if self.dirs[domain].mode & NS_SEARCH == 0 {
            self.book(round, NB_EACCES);
            return Err(NB_EACCES);
        }
        self.names.push(bare.clone());
        self.book(round, 0);
        Ok(bare)
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.eacces_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.names.len() {
            let bytes = self.names[i].as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = (h ^ (bytes[j] as u64)).wrapping_mul(0x100000001b3);
                j += 1;
            }
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "NameBoard{{dirs={},names={},rounds={},ok={},einval={},eacces={}}}",
            self.dirs.len(), self.names.len(), self.rounds, self.ok_hits,
            self.einval_hits, self.eacces_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被板正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut b = NameBoard::new(vec![
        NsDir { label: "open".to_string(), mode: NS_SEARCH },
        NsDir { label: "shut".to_string(), mode: 0 },
    ]);
    // 注入 1：不以 / 起头 → EINVAL。
    caught[0] = matches!(b.resolve(0, "no-slash", 9001), Err(x) if x == NB_EINVAL);
    // 注入 2：首字符后再含 / → EINVAL。
    caught[1] = matches!(b.resolve(0, "/a/b", 9002), Err(x) if x == NB_EINVAL);
    // 注入 3：非法字符（空格）→ EINVAL。
    caught[2] = matches!(b.resolve(0, "/bad name", 9003), Err(x) if x == NB_EINVAL);
    // 注入 4：无检索权域 → EACCES（先保证名字本身合法）。
    caught[3] = matches!(b.resolve(1, "/legal", 9004), Err(x) if x == NB_EACCES);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法名为主，三错误面轮转。返回符合预期轮数。
pub fn sample_thousand(b: &mut NameBoard) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < NB_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 5 {
            // 合法面：轮转后缀名。
            let name = format!("/q{}", r % 50);
            matched = b.resolve(0, &name, r).is_ok();
        } else if low == 5 {
            // EINVAL 面：无前导 /。
            matched = matches!(b.resolve(0, "bare", r), Err(x) if x == NB_EINVAL);
        } else if low == 6 {
            // EINVAL 面：嵌套 /。
            matched = matches!(b.resolve(0, "/a/b", r), Err(x) if x == NB_EINVAL);
        } else if low == 7 {
            // EACCES 面。
            matched = matches!(b.resolve(1, "/okname", r), Err(x) if x == NB_EACCES);
        } else if low == 8 {
            // EINVAL 面：空名。
            matched = matches!(b.resolve(0, "", r), Err(x) if x == NB_EINVAL);
        } else {
            // 合法面：含点与连字符。
            let name = format!("/v{}.{}", r % 9, r % 7);
            matched = b.resolve(0, &name, r).is_ok();
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10949 判据组。
pub fn run_unx10949_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10949");
    let mk = || {
        NameBoard::new(vec![
            NsDir { label: "open".to_string(), mode: NS_SEARCH },
            NsDir { label: "shut".to_string(), mode: 0 },
        ])
    };
    // J1 正面之一：非法名 → EINVAL 十轮全中（四种坏形态轮转）。
    let mut b1 = mk();
    let bads = ["noslash", "/a/b", "/bad name", "", "/x!", "/a\nb"];
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let nm = bads[k % bads.len()];
        if matches!(b1.resolve(0, nm, k), Err(x) if x == NB_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "非法名十轮全部裁 EINVAL");
    // J1 正面之二：名 EACCES 十轮全中。
    let mut b2 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(b2.resolve(1, "/good", k), Err(x) if x == NB_EACCES) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", c == 10, "无检索权域十轮全部裁 EACCES");
    // J2 反面：千轮合法名零误报。
    let mut b3 = mk();
    let good = sample_thousand(&mut b3);
    s.add_dyn("J2-反面零误报", good == NB_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "无斜杠/嵌套/坏字符/无权域四路坏样本全部拦截",
    );
    // J4 判定序：EINVAL 先于 EACCES（坏名 + 无权域裁 EINVAL）。
    let mut b4 = mk();
    s.add(
        "J4-判定序",
        matches!(b4.resolve(1, "badname", 1), Err(x) if x == NB_EINVAL),
        "静态校验先行裁定",
    );
    // J5 越长名子面：恰好越界一位。
    let mut long_name = String::new();
    long_name.push('/');
    let mut i = 0;
    while i < NAME_MAX {
        long_name.push('a');
        i += 1;
    }
    s.add(
        "J5-越长子面",
        matches!(NameBoard::check_name(&long_name), Err(x) if x == NB_2LONG),
        "NAME_MAX+1 长度裁越长子面",
    );
    // J6 恰好 NAME_MAX 合法。
    let mut fit_name = String::new();
    fit_name.push('/');
    let mut i = 0;
    while i < NAME_MAX - 1 {
        fit_name.push('a');
        i += 1;
    }
    s.add(
        "J6-边界合法",
        NameBoard::check_name(&fit_name).is_ok(),
        "恰好 NAME_MAX 长度合法",
    );
    // J7 错误码分段。
    s.add(
        "J7-错误码分段",
        NB_EINVAL > SEG_BASE && NB_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J8 错误码互异。
    let codes = [NB_EINVAL, NB_EACCES, NB_2LONG, NB_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J8-错误码互异", distinct, "四个错误码两两不同");
    // J9 账目对平。
    s.add("J9-账目对平", b3.ledger_ok(), "千轮之后三类计数与轮次、日志逐项对平");
    // J10 fold 稳定。
    let f = b3.fold();
    let clone = b3.clone();
    s.add("J10-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J11 渲染非空。
    let txt = b3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("NameBoard"), "板级渲染含类型名与计数");
    // J12 规范化：bare 名去前导 /。
    let mut b5 = mk();
    let bare = b5.resolve(0, "/vxq1", 1);
    s.add(
        "J12-规范化",
        matches!(&bare, Ok(t) if t == "vxq1"),
        "解析结果为去前导斜杠的本名",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == NB_ROUNDS,
        &format!("good={} board={}", good, b3.render()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10949_mq_name_parse_flow() {
        // 流程：合法名 → 无斜杠 → 嵌套 → 坏字符 → 无权域 → 越长。
        let mut b = NameBoard::new(vec![
            NsDir { label: "open".to_string(), mode: NS_SEARCH },
            NsDir { label: "shut".to_string(), mode: 0 },
        ]);
        assert_eq!(b.resolve(0, "/ok", 1), Ok("ok".to_string()));
        assert!(matches!(b.resolve(0, "bad", 2), Err(x) if x == NB_EINVAL));
        assert!(matches!(b.resolve(0, "/a/b", 3), Err(x) if x == NB_EINVAL));
        assert!(matches!(b.resolve(0, "/a b", 4), Err(x) if x == NB_EINVAL));
        assert!(matches!(b.resolve(1, "/fine", 5), Err(x) if x == NB_EACCES));
        let mut long_name = String::new();
        long_name.push('/');
        let mut i = 0;
        while i < NAME_MAX + 2 {
            long_name.push('a');
            i += 1;
        }
        assert!(matches!(b.resolve(0, &long_name, 6), Err(x) if x == NB_2LONG));
        assert!(b.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10949_checks_green() {
        let s = run_unx10949_checks();
        assert!(s.all_passed(), "UNX-F10949 判据未全绿");
    }
}
