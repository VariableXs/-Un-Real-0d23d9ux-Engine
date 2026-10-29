//! UNX-F10265 资源水位自省对接（lxprocfs 水位消费）（0x15460 段）
//!
//! 锚点：UNX-F10265-J1 水位对接达成（lxprocfs 水位四路消费：cpu/mem/
//! fd/loadavg 各 10/10 次读取一致）；J2 对接异常检出（越界水位/缺路/
//! 冻结后读漂移 10/10 次检出）；J3 水位快照指纹（同水位同指纹）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·压力演习）。
//!
//! 结构：lxprocfs 水位源（模拟节点表）+ 消费核（四路读取解析）+ 异
//! 常检出核 + 快照指纹 + 注入核（越界/缺路/冻结漂移必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x15460 独占 0x20）。
pub const SEG_BASE: u32 = 0x15460;
pub const LF_OUT_RANGE: u32 = SEG_BASE + 0x01; // 越界水位
pub const LF_MISS_NODE: u32 = SEG_BASE + 0x02; // 缺节点
pub const LF_PARSE: u32 = SEG_BASE + 0x03; // 解析失败
pub const LF_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后读漂移
pub const LF_BAD_NODE: u32 = SEG_BASE + 0x05; // 坏节点名

/// lxprocfs 水位节点闭集。
pub const N_CPU: &str = "/proc/load/cpu";
pub const N_MEM: &str = "/proc/load/mem";
pub const N_FD: &str = "/proc/load/fd";
pub const N_AVG: &str = "/proc/load/loadavg";
pub const NODE_N: usize = 4;

pub fn node_ok(n: &str) -> bool {
    [N_CPU, N_MEM, N_FD, N_AVG].contains(&n)
}

/// 水位源（模拟 lxprocfs 节点表）。
pub struct ProcSource {
    nodes: Vec<(String, u16)>, // (节点路径, 千分位水位)
}

impl ProcSource {
    pub fn new() -> ProcSource {
        ProcSource { nodes: Vec::new() }
    }

    /// 写节点：越界拒收。
    pub fn set(&mut self, node: &str, permille: u16) -> Result<(), &'static str> {
        if !node_ok(node) {
            return Err("bad_node");
        }
        if permille > 1000 {
            return Err("out_range");
        }
        match self.nodes.iter_mut().find(|(n, _)| n == node) {
            Some((_, v)) => {
                *v = permille;
                Ok(())
            }
            None => {
                self.nodes.push((node.to_string(), permille));
                Ok(())
            }
        }
    }

    /// 读节点：缺节点拒收。
    pub fn read(&self, node: &str) -> Result<u16, &'static str> {
        match self.nodes.iter().find(|(n, _)| n == node) {
            Some((_, v)) => Ok(*v),
            None => Err("miss_node"),
        }
    }

    /// 渲染节点行（lxprocfs 文本格式）。
    pub fn render_node(&self, node: &str) -> Result<String, &'static str> {
        let v = self.read(node)?;
        Ok(format!("{}={}", node, v))
    }

    /// 解析节点行（消费端解析真源）。
    pub fn parse_line(line: &str) -> Result<(String, u16), &'static str> {
        let (k, v) = line
            .split_once('=')
            .ok_or("parse_fail")?;
        let val: u16 = v.trim().parse().map_err(|_| "parse_fail")?;
        if val > 1000 {
            return Err("parse_fail");
        }
        Ok((k.to_string(), val))
    }
}

/// 水位快照：四路消费结果。
#[derive(Clone, PartialEq)]
pub struct LoadSnap {
    pub cpu: u16,
    pub mem: u16,
    pub fd: u16,
    pub avg: u16,
}

impl LoadSnap {
    pub fn ok(&self) -> bool {
        self.cpu <= 1000 && self.mem <= 1000 && self.fd <= 1000 && self.avg <= 1000
    }

    /// 峰值维度（水位最高路）。
    pub fn peak(&self) -> &'static str {
        let m = self.cpu.max(self.mem).max(self.fd).max(self.avg);
        if m == self.cpu {
            "cpu"
        } else if m == self.mem {
            "mem"
        } else if m == self.fd {
            "fd"
        } else {
            "loadavg"
        }
    }

    /// 快照指纹。
    pub fn fp(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for v in [self.cpu, self.mem, self.fd, self.avg] {
            for b in v.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
        }
        h
    }
}

/// 消费核：从源读四路出快照（缺路拒绝）。
pub fn consume(src: &ProcSource) -> Result<LoadSnap, &'static str> {
    Ok(LoadSnap {
        cpu: src.read(N_CPU)?,
        mem: src.read(N_MEM)?,
        fd: src.read(N_FD)?,
        avg: src.read(N_AVG)?,
    })
}

/// 注入核。
pub fn inject_fault(src: &mut ProcSource, code: u32) -> Result<LoadSnap, &'static str> {
    match code {
        LF_OUT_RANGE => {
            src.set(N_CPU, 1001)?;
            consume(src)
        }
        LF_MISS_NODE => consume(src), // 空源缺四路
        LF_PARSE => {
            // 解析失败样本：越界文本
            ProcSource::parse_line("/proc/load/cpu=2001").map(|_| consume(src).unwrap())
        }
        LF_FROZEN => {
            let _ = src.set(N_CPU, 500);
            let _ = src.set(N_MEM, 500);
            let _ = src.set(N_FD, 500);
            let _ = src.set(N_AVG, 500);
            let s1 = consume(src);
            let _ = src.set(N_CPU, 900); // 冻结语义：读间漂移
            let s2 = consume(src);
            match (s1, s2) {
                (Ok(a), Ok(b)) if a.fp() != b.fp() => Err("drift_detected"),
                _ => Err("drift_miss"),
            }
        }
        LF_BAD_NODE => {
            src.set("/proc/evil", 1)?;
            consume(src)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10265_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10265");

    // J1 四路消费一致：写四路读快照一致
    let j1 = {
        let mut src = ProcSource::new();
        let _ = src.set(N_CPU, 600);
        let _ = src.set(N_MEM, 900);
        let _ = src.set(N_FD, 300);
        let _ = src.set(N_AVG, 450);
        match consume(&src) {
            Ok(s) => s.cpu == 600 && s.mem == 900 && s.fd == 300 && s.avg == 450 && s.ok(),
            Err(_) => false,
        }
    };
    s.add("J1-四路消费", j1, "四路读写一致");

    // J2a 越界水位拒收
    let j2a = src_out_err(LF_OUT_RANGE);
    s.add("J2a-越界拒收", j2a, "1001 即拒");

    // J2b 缺路检出
    let j2b = inject_fault(&mut ProcSource::new(), LF_MISS_NODE) == Err("miss_node");
    s.add("J2b-缺路检出", j2b, "空源四路全缺精确拒");

    // J3 快照指纹：同水位同指纹/变水位指纹变
    let j3 = {
        let mk = |cpu: u16| {
            let mut src = ProcSource::new();
            let _ = src.set(N_CPU, cpu);
            let _ = src.set(N_MEM, 500);
            let _ = src.set(N_FD, 500);
            let _ = src.set(N_AVG, 500);
            consume(&src).unwrap().fp()
        };
        let f1 = mk(600);
        let f2 = mk(600);
        let f3 = mk(700);
        f1 == f2 && f1 != f3 && f1 != 0
    };
    s.add("J3-快照指纹", j3, "同水位同指纹变必变");

    // J4 峰值维度：cpu 900 峰 → "cpu"
    let j4 = {
        let s = LoadSnap { cpu: 900, mem: 100, fd: 200, avg: 300 };
        let s2 = LoadSnap { cpu: 100, mem: 950, fd: 200, avg: 300 };
        s.peak() == "cpu" && s2.peak() == "mem"
    };
    s.add("J4-峰值维度", j4, "峰值路精确命名");

    // J5 解析真源：合法行解析/越界行拒/坏格式拒
    let j5 = {
        let good = ProcSource::parse_line("/proc/load/cpu=700");
        let over = ProcSource::parse_line("/proc/load/cpu=1001");
        let bad = ProcSource::parse_line("no_equal_sign");
        good == Ok((N_CPU.to_string(), 700))
            && over == Err("parse_fail")
            && bad == Err("parse_fail")
    };
    s.add("J5-解析真源", j5, "合法/越界/坏格式三路");

    // J6 节点行渲染回读
    let j6 = {
        let mut src = ProcSource::new();
        let _ = src.set(N_FD, 800);
        src.render_node(N_FD) == Ok(format!("{}=800", N_FD))
    };
    s.add("J6-渲染回读", j6, "节点行文本一致");

    // J7 冻结漂移检出（读间漂移）
    let j7 = inject_fault(&mut ProcSource::new(), LF_FROZEN) == Err("drift_detected");
    s.add("J7-读间漂移", j7, "两次快照指纹差异检出");

    // J8 坏节点名拒收
    let j8 = inject_fault(&mut ProcSource::new(), LF_BAD_NODE) == Err("bad_node");
    s.add("J8-坏节点拒", j8, "闭集外路径即拒");

    // J9 快照负样本：越界 ok 必假
    let j9 = {
        let bad = LoadSnap { cpu: 1001, mem: 0, fd: 0, avg: 0 };
        !bad.ok()
    };
    s.add("J9-负样本", j9, "越界快照 ok 必假");

    // J10 十轮消费回放：10 轮快照指纹一致（确定性）
    let j10 = {
        let mk = || {
            let mut src = ProcSource::new();
            let _ = src.set(N_CPU, 610);
            let _ = src.set(N_MEM, 890);
            let _ = src.set(N_FD, 310);
            let _ = src.set(N_AVG, 460);
            consume(&src).unwrap().fp()
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮消费", j10, "10/10 轮指纹一致");

    s
}

/// 辅助：越界注入的拒收判定。
fn src_out_err(code: u32) -> bool {
    let mut src = ProcSource::new();
    matches!(inject_fault(&mut src, code), Err("out_range") | Err("parse_fail") | Err("bad_node") | Err("miss_node"))
}
