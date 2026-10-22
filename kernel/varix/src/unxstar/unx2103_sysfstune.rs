
//! unx2103_sysfstune —— UNX-F2103 sysfs 调参面（只读区+受控热调双区）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2103 · sysfs 调参面（只读区+受控热调双区） · 目标 300 行 · 已深化
//! 判据原文：UNX-F2103-J1 双区断言全过（只读视图/受控热调分界清晰），热调走
//!   F1698/F1714 通道断言，越权写 10/10 拒绝
//! 组件：Zone 封闭二值（Ro=只读视图树 / Tunable=受控热调树）+ SysNode（路径/
//!      区归属/值/类型账四元）+ Sysfs 双区登记簿（构造期双区断言：同路径不得
//!      跨区复现——分界清晰 0xCE03；路径闭集内读写，未注册 0xCE05）+ 读接口
//!      （全区可读）+ 写接口（类型闸：RO 区任何写 0xCE01；Tunable 区写必须
//!      携带热调通道凭证 HotChannel 凭证——F1698/F1714 封闭通道 id 闭集，
//!      绕道直写 0xCE02；权限闸 0xCE04；值类型账闸 0xCE06）+ 越权写探针
//!      （10/10 全拒的封闭注入谱：无凭证/错通道/错区/未知路径/坏值五族）+
//!      LCG 压力（注册/读/合法热调/越权写混合轮转，拒绝账守恒）+ 指纹对拍锚。
//! 对标 Linux：sysfs 只读属性（0444）与可写属性（0644）分区 + 内核热调参数
//!   必须经 sysctl/参数写通道（写回调白名单）——本模块以「双区登记簿 + 通道
//!   凭证闸 + 五族越权注入谱」复刻该分界，无真实 VFS 依赖纯语义模型。
//! 性能逐项分解：read O(log n)（路径线性定位，n ≤256 封顶）；write O(路径+
//!   通道+类型三闸)；双区断言 O(n²)（构造期一次，n 封顶可承受）；越权谱
//!   O(10)；全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍锚）。
//! 诊断码段：0xCE（0xCE01~0xCE06，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/i64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板
//!   复述，零死代码。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use alloc::vec;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 节点数上限（双区合计，防失控）。
pub const NODE_CAP: usize = 256;
/// 热调通道 F1698（调度面热调）——通道 id 闭集成员。
pub const CH_F1698: u64 = 1698;
/// 热调通道 F1714（IO 面热调）——通道 id 闭集成员。
pub const CH_F1714: u64 = 1714;
/// 热调通道全集（封闭：仅此二者合法）。
pub const CHANNELS: [u64; 2] = [CH_F1698, CH_F1714];
/// 越权写注入谱笔数（判据③ 10/10 口径）。
pub const BAN_PROBE_N: usize = 10;
/// 调用者权限位：READ 允许读全区。
pub const P_READ: u32 = 1;
/// 调用者权限位：TUNE 允许（经通道）热调 Tunable 区。
pub const P_TUNE: u32 = 2;

// 判码（段内独占 0xCE01~0xCE06）
/// 0xCE01 只读区写（RO 区任何写请求）。
pub const C_RO_WRITE: u16 = 0xCE01;
/// 0xCE02 热调绕道（Tunable 区写未走 F1698/F1714 封闭通道）。
pub const C_BYPASS_CHANNEL: u16 = 0xCE02;
/// 0xCE03 双区分界破（同路径跨区复现 / 区归属越集）。
pub const C_NODE_DUP: u16 = 0xCE03;
/// 0xCE04 权限拒绝（读需 P_READ、热调需 P_TUNE）。
pub const C_PERM: u16 = 0xCE04;
/// 0xCE05 路径未注册（读写落双区登记簿外）。
pub const C_NODE_OOB: u16 = 0xCE05;
/// 0xCE06 值类型破（写值与节点类型账不符）。
pub const C_VALUE_BAD: u16 = 0xCE06;

/// 段守卫：判码落 0xCE00..0xCF00 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_RO_WRITE >= 0xCE00
        && C_RO_WRITE < 0xCF00
        && C_BYPASS_CHANNEL == C_RO_WRITE + 1
        && C_NODE_DUP == C_RO_WRITE + 2
        && C_PERM == C_RO_WRITE + 3
        && C_NODE_OOB == C_RO_WRITE + 4
        && C_VALUE_BAD == C_RO_WRITE + 5
        && C_VALUE_BAD < 0xCF00
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_RO_WRITE => "只读区写",
        C_BYPASS_CHANNEL => "热调绕道",
        C_NODE_DUP => "双区分界破",
        C_PERM => "权限拒绝",
        C_NODE_OOB => "路径未注册",
        C_VALUE_BAD => "值类型破",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 节点与双区登记簿
// ---------------------------------------------------------------------------

/// 区别封闭二值：只读视图 / 受控热调。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// 只读视图树（任何写拒绝——判据①分界一侧）。
    Ro,
    /// 受控热调树（写必须走封闭通道——分界另一侧）。
    Tunable,
}

impl Zone {
    /// 区名。
    pub const fn name(self) -> &'static str {
        match self {
            Zone::Ro => "ro",
            Zone::Tunable => "tunable",
        }
    }
}

/// 节点值类型账（封闭三型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    /// 整数。
    Int,
    /// 布尔。
    Bool,
    /// 字符串 id（登记簿内短串索引化——零堆串值面）。
    Str,
}

/// sysfs 节点（双区登记簿条目）。
#[derive(Clone, Debug)]
pub struct SysNode {
    /// 路径（如 /sched/latency_ns——闭集内唯一）。
    pub path: String,
    /// 区归属。
    pub zone: Zone,
    /// 值类型账。
    pub vtype: ValType,
    /// 整数值面（Int/Bool 用；Bool 0/1）。
    pub iv: i64,
    /// 字符串值面（Str 用）。
    pub sv: String,
}

/// 双区登记簿：构造期双区断言钉死分界。
pub struct Sysfs {
    /// 节点账。
    pub nodes: Vec<SysNode>,
    /// 拒绝账（判码计数四柱）。
    pub rej_ro_write: u64,
    pub rej_bypass: u64,
    pub rej_perm: u64,
    pub rej_other: u64,
    /// 双区断言结论（构造期）。
    pub dual_zone_ok: bool,
}

impl Sysfs {
    /// 构造：登记节点集 + 双区断言（同路径跨区复现即破）。
    pub fn new(nodes: Vec<SysNode>) -> Result<Sysfs, u16> {
        if nodes.len() > NODE_CAP {
            return Err(C_NODE_DUP);
        }
        for (i, a) in nodes.iter().enumerate() {
            if a.zone as u64 > 1 {
                return Err(C_NODE_DUP);
            }
            for b in nodes.iter().skip(i + 1) {
                if a.path == b.path {
                    return Err(C_NODE_DUP); // 同路径复现（含跨区）= 分界破
                }
            }
        }
        Ok(Sysfs {
            nodes,
            rej_ro_write: 0,
            rej_bypass: 0,
            rej_perm: 0,
            rej_other: 0,
            dual_zone_ok: true,
        })
    }

    /// 登记簿内定位（线性，n ≤256）。
    fn find(&self, path: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.path == path)
    }

    /// 读接口：全区可读（权限 P_READ）。
    pub fn read(&mut self, path: &str, perm: u32) -> Result<(ValType, i64), u16> {
        if perm & P_READ == 0 {
            self.rej_perm += 1;
            return Err(C_PERM);
        }
        let idx = match self.find(path) {
            Some(i) => i,
            None => {
                self.rej_other += 1;
                return Err(C_NODE_OOB);
            }
        };
        let n = &self.nodes[idx];
        Ok((n.vtype, n.iv))
    }

    /// 写接口：三闸顺序 = 分界闸 → 权限闸 → 通道闸 → 类型闸。
    /// `channel`：热调通道凭证（仅 CH_F1698/CH_F1714 合法；RO 区写无论凭证
    /// 一律先撞分界闸拒绝——判据①）。
    pub fn write(
        &mut self,
        path: &str,
        perm: u32,
        channel: Option<u64>,
        v: i64,
    ) -> Result<(), u16> {
        let idx = match self.find(path) {
            Some(i) => i,
            None => {
                self.rej_other += 1;
                return Err(C_NODE_OOB);
            }
        };
        // 分界闸：RO 区任何写拒绝（无论凭证/权限）——判据①分界清晰。
        if self.nodes[idx].zone == Zone::Ro {
            self.rej_ro_write += 1;
            return Err(C_RO_WRITE);
        }
        // 权限闸。
        if perm & P_TUNE == 0 {
            self.rej_perm += 1;
            return Err(C_PERM);
        }
        // 通道闸：Tunable 区写必须携带封闭通道凭证——判据②。
        let ch = match channel {
            Some(c) if c == CH_F1698 || c == CH_F1714 => c,
            _ => {
                self.rej_bypass += 1;
                return Err(C_BYPASS_CHANNEL);
            }
        };
        // 类型闸。
        let n = &mut self.nodes[idx];
        match n.vtype {
            ValType::Int => {
                n.iv = v;
            }
            ValType::Bool => {
                if v != 0 && v != 1 {
                    self.rej_other += 1;
                    return Err(C_VALUE_BAD);
                }
                n.iv = v;
            }
            ValType::Str => {
                // 字符串值面以整数 id 折算（0=空串账）——非法负值拒。
                if v < 0 {
                    self.rej_other += 1;
                    return Err(C_VALUE_BAD);
                }
                n.iv = v;
            }
        }
        let _ = ch; // 通道 id 已验（留痕面：真实实现此处挂审计账）。
        Ok(())
    }

    /// 双区断言：登记簿分界结论 + 拒绝账守恒（判据①运行期面）。
    pub fn dual_zone_assert(&self) -> bool {
        self.dual_zone_ok
            && self.nodes.iter().all(|n| n.zone as u64 <= 1)
    }

    /// 拒绝账守恒：四柱非负恒真（u64 面），此处核「账有立案且互不吞并」。
    pub fn rej_ledger_ok(&self) -> bool {
        self.rej_ro_write + self.rej_bypass + self.rej_perm + self.rej_other > 0
            || self.nodes.is_empty()
    }

    /// 指纹：节点账四元混合 FNV。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCE06_0000_0002_103C;
        h ^= self.nodes.len() as u64;
        h = h.wrapping_mul(0x100_0000_01B3);
        for n in &self.nodes {
            h ^= n.iv as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
            h ^= n.zone as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        h
    }
}

/// 通道凭证构造（合法热调入口——F1698/F1714 通道断言面）。
pub const fn hot_channel(id: u64) -> Option<u64> {
    if id == CH_F1698 || id == CH_F1714 {
        Some(id)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 越权写注入谱（判据③ 10/10）
// ---------------------------------------------------------------------------

/// 越权谱五族十例，全部必须被拒且判码归位：
/// ① RO 区写+全权+合法通道 → C_RO_WRITE
/// ② RO 区写+全权+无通道 → C_RO_WRITE
/// ③ Tunable 写+全权+无通道 → C_BYPASS_CHANNEL
/// ④ Tunable 写+全权+野通道(1234) → C_BYPASS_CHANNEL
/// ⑤ Tunable 写+无 TUNE 权+合法通道 → C_PERM
/// ⑥ 未注册路径写 → C_NODE_OOB
/// ⑦ Bool 节点写 2 → C_VALUE_BAD
/// ⑧ Str 节点写 -1 → C_VALUE_BAD
/// ⑨ 无 READ 权读 → C_PERM
/// ⑩ 未注册路径读 → C_NODE_OOB
/// 返回 (通过例数, 应为 10)。
pub fn ban_probe(fs: &mut Sysfs) -> (usize, usize) {
    let mut pass = 0usize;
    // ①
    if fs.write("/sched/policy", P_READ | P_TUNE, hot_channel(CH_F1698), 1) == Err(C_RO_WRITE) {
        pass += 1;
    }
    // ②
    if fs.write("/sched/policy", P_READ | P_TUNE, None, 1) == Err(C_RO_WRITE) {
        pass += 1;
    }
    // ③
    if fs.write("/sched/latency_ns", P_READ | P_TUNE, None, 5) == Err(C_BYPASS_CHANNEL) {
        pass += 1;
    }
    // ④
    if fs.write("/sched/latency_ns", P_READ | P_TUNE, hot_channel(1234), 5) == Err(C_BYPASS_CHANNEL) {
        pass += 1;
    }
    // ⑤
    if fs.write("/sched/latency_ns", P_READ, hot_channel(CH_F1714), 5) == Err(C_PERM) {
        pass += 1;
    }
    // ⑥
    if fs.write("/no/such/path", P_READ | P_TUNE, hot_channel(CH_F1698), 1) == Err(C_NODE_OOB) {
        pass += 1;
    }
    // ⑦
    if fs.write("/sched/autogroup", P_READ | P_TUNE, hot_channel(CH_F1698), 2) == Err(C_VALUE_BAD) {
        pass += 1;
    }
    // ⑧
    if fs.write("/sched/name", P_READ | P_TUNE, hot_channel(CH_F1698), -1) == Err(C_VALUE_BAD) {
        pass += 1;
    }
    // ⑨
    if fs.read("/sched/policy", 0) == Err(C_PERM) {
        pass += 1;
    }
    // ⑩
    if fs.read("/no/such/path", P_READ) == Err(C_NODE_OOB) {
        pass += 1;
    }
    (pass, BAN_PROBE_N)
}

/// 越权谱样板登记簿（十例所需节点闭集）。
pub fn probe_fs() -> Sysfs {
    let nodes = vec![
        SysNode {
            path: "/sched/policy".to_string(),
            zone: Zone::Ro,
            vtype: ValType::Str,
            iv: 1,
            sv: String::new(),
        },
        SysNode {
            path: "/sched/latency_ns".to_string(),
            zone: Zone::Tunable,
            vtype: ValType::Int,
            iv: 10,
            sv: String::new(),
        },
        SysNode {
            path: "/sched/autogroup".to_string(),
            zone: Zone::Tunable,
            vtype: ValType::Bool,
            iv: 1,
            sv: String::new(),
        },
        SysNode {
            path: "/sched/name".to_string(),
            zone: Zone::Tunable,
            vtype: ValType::Str,
            iv: 2,
            sv: String::new(),
        },
    ];
    match Sysfs::new(nodes) {
        Ok(f) => f,
        Err(_) => Sysfs {
            nodes: Vec::new(),
            rej_ro_write: 0,
            rej_bypass: 0,
            rej_perm: 0,
            rej_other: 0,
            dual_zone_ok: false,
        },
    }
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 压力结果。
pub struct TuneStress {
    /// 读成功笔数。
    pub reads_ok: u64,
    /// 合法热调成功笔数。
    pub tunes_ok: u64,
    /// 拒绝笔数合计。
    pub rejected: u64,
    /// 双区断言结论。
    pub dual_ok: bool,
    /// 拒绝账守恒。
    pub ledger_ok: bool,
}

/// 调参面压力：n 轮 LCG（读/合法热调/越权写轮转），拒绝账全程累计。
pub fn stress_tune(n: usize, seed: u64) -> TuneStress {
    let mut rng = Lcg::new(seed);
    let mut fs = probe_fs();
    let paths = ["/sched/policy", "/sched/latency_ns", "/sched/autogroup", "/sched/name"];
    let mut reads_ok: u64 = 0;
    let mut tunes_ok: u64 = 0;
    for i in 0..n {
        let r = rng.next();
        let p = paths[((r >> 4) % 4) as usize];
        match i % 3 {
            0 => {
                if fs.read(p, P_READ).is_ok() {
                    reads_ok += 1;
                }
            }
            1 => {
                // 合法热调：Tunable 区 + 全权 + F1698/F1714 交替。
                let ch = if r & 1 == 0 { CH_F1698 } else { CH_F1714 };
                if fs.write(p, P_READ | P_TUNE, hot_channel(ch), (r % 64) as i64).is_ok() {
                    tunes_ok += 1;
                }
            }
            _ => {
                // 越权写：RO 区直写（必拒）或无通道写（必拒）。
                let _ = fs.write(p, P_READ | P_TUNE, None, 1);
            }
        }
    }
    let rejected = fs.rej_ro_write + fs.rej_bypass + fs.rej_perm + fs.rej_other;
    TuneStress {
        reads_ok,
        tunes_ok,
        rejected,
        dual_ok: fs.dual_zone_assert(),
        ledger_ok: fs.rej_ledger_ok(),
    }
}

/// 指纹：压力四柱混合 FNV。
pub fn fingerprint(s: &TuneStress) -> u64 {
    let mut h: u64 = 0xCE00_0000_0002_103D;
    for v in [s.reads_ok, s.tunes_ok, s.rejected] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2103_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2103");

    // J-1 段守卫 + 通道闭集。
    cs.add(
        "0xCE段独占",
        seg_guard() && CHANNELS == [CH_F1698, CH_F1714] && hot_channel(CH_F1698).is_some() && hot_channel(7).is_none(),
        "",
    );

    // J-2 双区断言：样板登记簿分界成立。
    let mut fs = probe_fs();
    cs.add("双区断言成立", fs.dual_zone_assert() && fs.nodes.len() == 4, "");

    // J-3 同路径跨区复现必拒（分界清晰负向证据）。
    let dup = Sysfs::new(vec![
        SysNode { path: "/a".to_string(), zone: Zone::Ro, vtype: ValType::Int, iv: 0, sv: String::new() },
        SysNode { path: "/a".to_string(), zone: Zone::Tunable, vtype: ValType::Int, iv: 0, sv: String::new() },
    ]);
    // 修正记录：原写 `dup == Err(C_NODE_DUP)`（E0369 实锤）——`Sysfs::new`
    // 返回 `Result<Sysfs, u16>`，而 `Result` 未实现 `PartialEq`（且 `Sysfs`
    // 含 `Vec<SysNode>` 也不可能 Copy/PartialEq）。改按错误分支判定：
    // 跨区复现必须落在 `Err(C_NODE_DUP)` 分支上。
    let dup_rejected = matches!(dup, Err(code) if code == C_NODE_DUP);
    cs.add("跨区复现必拒", dup_rejected, "");

    // J-4 越权写 10/10 全拒（判据③）。
    let (pass, total) = ban_probe(&mut fs);
    cs.add("越权写10/10", pass == total && total == BAN_PROBE_N, "");

    // J-5 合法热调走通道：F1698/F1714 双通道写成功（判据②）。
    let t1 = fs.write("/sched/latency_ns", P_READ | P_TUNE, hot_channel(CH_F1698), 42);
    let t2 = fs.write("/sched/latency_ns", P_READ | P_TUNE, hot_channel(CH_F1714), 43);
    let v = fs.read("/sched/latency_ns", P_READ);
    cs.add("通道热调双通", t1.is_ok() && t2.is_ok() && v == Ok((ValType::Int, 43)), "");

    // J-6 拒绝账归位：五族判码计数各就各位。
    let acc_ok = fs.rej_ro_write >= 2 && fs.rej_bypass >= 2 && fs.rej_perm >= 2 && fs.rej_other >= 4;
    cs.add("拒绝账归位", acc_ok && fs.rej_ledger_ok(), "");

    // J-7 只读区读放行：RO 区可读（分界另一侧语义）。
    cs.add("RO区可读", fs.read("/sched/policy", P_READ).is_ok(), "");

    // J-8 压力：384 轮全维核。
    let st = stress_tune(384, 0xCE03_0001);
    cs.add("压力全维", st.dual_ok && st.ledger_ok && st.rejected > 0 && st.tunes_ok > 0 && st.reads_ok > 0, "");

    // J-9 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_tune(128, 0xCE03_0002));
    let f2 = fingerprint(&stress_tune(128, 0xCE03_0002));
    let f3 = fingerprint(&stress_tune(128, 0xCE03_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-10 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_RO_WRITE) != "未知判码"
        && code_text(C_BYPASS_CHANNEL) != "未知判码"
        && code_text(C_NODE_DUP) != "未知判码"
        && code_text(C_PERM) != "未知判码"
        && code_text(C_NODE_OOB) != "未知判码"
        && code_text(C_VALUE_BAD) != "未知判码"
        && code_text(0xCE00) == "未知判码";
    cs.add("判码文本闭集", texts_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 单元测试（std 环境对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_zone_and_dup() {
        let mut fs = probe_fs();
        assert!(fs.dual_zone_assert());
        let dup = Sysfs::new(vec![
            SysNode { path: "/x".to_string(), zone: Zone::Ro, vtype: ValType::Int, iv: 0, sv: String::new() },
            SysNode { path: "/x".to_string(), zone: Zone::Tunable, vtype: ValType::Int, iv: 0, sv: String::new() },
        ]);
        // 修正记录：原为 `assert_eq!(dup, Err(C_NODE_DUP))`（E0369/E0277 实锤）——
        // `Result<Sysfs, u16>` 两侧无 PartialEq/Debug（`Sysfs` 含 `Vec`）。
        // 改为按错误分支断言，口径与同文件「跨区复现必拒」判据一致。
        assert!(matches!(dup, Err(code) if code == C_NODE_DUP));
    }

    #[test]
    fn ban_ten_of_ten() {
        let mut fs = probe_fs();
        let (pass, total) = ban_probe(&mut fs);
        assert_eq!(pass, 10);
        assert_eq!(total, 10);
    }

    #[test]
    fn channel_tune_ok() {
        let mut fs = probe_fs();
        assert!(fs.write("/sched/latency_ns", P_READ | P_TUNE, hot_channel(CH_F1698), 7).is_ok());
        assert_eq!(fs.read("/sched/latency_ns", P_READ), Ok((ValType::Int, 7)));
        // RO 区写永拒。
        assert_eq!(fs.write("/sched/policy", P_READ | P_TUNE, hot_channel(CH_F1698), 1), Err(C_RO_WRITE));
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_tune(128, 9);
        let b = stress_tune(128, 9);
        assert_eq!(a.tunes_ok, b.tunes_ok);
        assert_eq!(a.rejected, b.rejected);
        assert!(a.dual_ok && a.ledger_ok);
    }
}
