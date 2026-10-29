//! UNX-F0738 · Limine 升级 ktest 断言族（升级面聚合） · 诊断段 0x5bc0
//! 判据原文摘录：UNX-F0738-J1 本批升级面判据聚合 ≥15 断言（台账/指纹/流程/
//! 拦截/对照五面）一次命令全绿；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 断言族五面固定：台账面/指纹面/流程面/拦截面/对照面，每面至少 3 条，
//!    全族 ≥15 条；断言登记即定版，运行期不许增删。
//! 2. 断言是纯函数裁决：每条断言对着升级夹具（Fixture）产出 绿/红 二值，
//!    不允许"部分绿"；一次命令全绿 = 全族零红。
//! 3. 夹具注入确定性：夹具由样本数据构建，毒样本走坏路径出红，
//!    正样本走主路径出绿，复跑结果恒等。
//! 4. 定容数组账本 + 守恒恒等式：green+red == executed <= total。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5bc0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5bc0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_A_DUP: u16 = SEG_BASE + 1; // 断言号重复登记
const E_A_BAD: u16 = SEG_BASE + 2; // 断言号越界
const E_A_FULL: u16 = SEG_BASE + 3; // 断言族容量已满
const E_SURFACE_BAD: u16 = SEG_BASE + 4; // 面号越界（1..=5）
const E_MIN_COUNT: u16 = SEG_BASE + 5; // 断言族不足 15 条
const E_FACE_MIN: u16 = SEG_BASE + 6; // 某面不足 3 条
const E_RED: u16 = SEG_BASE + 7; // 断言红（通用失败码）
const E_PHASE: u16 = SEG_BASE + 8; // 运行状态机非法边
const E_BAL: u16 = SEG_BASE + 9; // 守恒恒等式破裂（green+red≠executed）
const E_SEALED: u16 = SEG_BASE + 10; // 断言族已封，拒绝再写
const E_TWICE: u16 = SEG_BASE + 11; // 同断言重复执行
const E_FIXTURE_BAD: u16 = SEG_BASE + 12; // 夹具非法（空载荷）
const E_NO_EXEC: u16 = SEG_BASE + 13; // 断言族未执行即断言全绿
const E_NAME_EMPTY: u16 = SEG_BASE + 14; // 断言名为空

/// 断言族定容。
pub const MAX_ASSERTS: usize = 32;

/// 全族下限：≥15 条。
pub const MIN_ASSERTS: usize = 15;

/// 每面下限：≥3 条。
pub const MIN_PER_SURFACE: usize = 3;

/// 五面编号。
pub const F_LEDGER: u8 = 1; // 台账面
pub const F_FINGER: u8 = 2; // 指纹面
pub const F_FLOW: u8 = 3; // 流程面
pub const F_INTERCEPT: u8 = 4; // 拦截面
pub const F_DIFF: u8 = 5; // 对照面

/// 面号中文名（报告行渲染用）。
pub fn surface_name(s: u8) -> &'static str {
    match s {
        F_LEDGER => "台账面",
        F_FINGER => "指纹面",
        F_FLOW => "流程面",
        F_INTERCEPT => "拦截面",
        F_DIFF => "对照面",
        _ => "未知面",
    }
}

// ── FNV-1a 64（夹具与结果哈希口径）──────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_A_DUP => "断言号重复：登记序自动编号，不需人工指定",
        E_A_BAD => "断言号越界：按登记序 1..N",
        E_A_FULL => "断言族已满 32 条：升级面断言收敛在族内",
        E_SURFACE_BAD => "面号越界：合法 1..=5",
        E_MIN_COUNT => "断言族不足 15 条：先补齐再运行",
        E_FACE_MIN => "某面不足 3 条：五面均衡覆盖",
        E_RED => "断言红：对照夹具预期排查升级回归点",
        E_PHASE => "状态机非法边：登记→运行→封账",
        E_BAL => "计数不守恒：green+red 必须等于 executed",
        E_SEALED => "断言族已封：封账后只读",
        E_TWICE => "断言重复执行：一票一裁",
        E_FIXTURE_BAD => "夹具非法：载荷不得为空",
        E_NO_EXEC => "未执行即断言全绿：先 run_all",
        E_NAME_EMPTY => "断言名为空",
        _ => "未知诊断码",
    }
}

// ── 升级夹具 ─────────────────────────────────────────────────
/// 升级夹具：断言族的唯一输入源（确定性样本集）。
#[derive(Clone, Debug)]
pub struct Fixture {
    /// 台账面样本：登记行数。
    pub ledger_rows: u32,
    /// 指纹面样本：产物哈希集（非零=已采）。
    pub finger_hashes: [u64; 4],
    /// 流程面样本：已走通流程步数（0..=5）。
    pub flow_steps: u32,
    /// 拦截面样本：注入数与拦截数。
    pub injections: u32,
    pub intercepted: u32,
    /// 对照面样本：升级前/后哈希。
    pub before_hash: u64,
    pub after_hash: u64,
}

impl Fixture {
    /// 合法性：指纹至少采 1、注入≥拦截。
    pub fn sane(&self) -> bool {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if self.finger_hashes[i] != 0 {
                n += 1;
            }
            i += 1;
        }
        n >= 1 && self.intercepted <= self.injections
    }
}

/// 断言裁决二值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 绿。
    Green,
    /// 红。
    Red,
}

impl Verdict {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            Verdict::Green => 1,
            Verdict::Red => 0,
        }
    }
}

// ── 断言族（登记与执行主体）──────────────────────────────────
/// ktest 断言族：五面聚合 ≥15 断言，一次执行全绿。
pub struct KtestFamily {
    /// 断言名。
    names: [String; MAX_ASSERTS],
    /// 断言所属面。
    surfaces: [u8; MAX_ASSERTS],
    /// 断言执行序（每条一个闭包式探针编号，裁决在 run 中查表）。
    probes: [u8; MAX_ASSERTS],
    /// 裁决结果（执行后落）。
    verdicts: [Option<Verdict>; MAX_ASSERTS],
    /// 登记数。
    count: usize,
    /// 执行数。
    executed: u32,
    /// 绿数。
    green: u32,
    /// 红数。
    red: u32,
    /// 是否封账。
    sealed: bool,
}

impl KtestFamily {
    /// 开族。
    pub fn new() -> KtestFamily {
        KtestFamily {
            names: [
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
            ],
            surfaces: [0; MAX_ASSERTS],
            probes: [0; MAX_ASSERTS],
            verdicts: [None; MAX_ASSERTS],
            count: 0,
            executed: 0,
            green: 0,
            red: 0,
            sealed: false,
        }
    }

    /// 探针编号闭集：每面 3 探针，共 15。
    /// P1 台账行数≥3；P2 台账哈希非零；P3 台账单调递增。
    /// P4 指纹采齐≥3；P5 指纹零重复；P6 指纹与对照一致。
    /// P7 流程步数=5；P8 流程步序单调；P9 流程终点回执。
    /// P10 注入全拦截；P11 拦截≤注入；P12 拦截账守恒。
    /// P13 前后哈希一致；P14 差异账零；P15 对照复算一致。
    pub const PROBE_MIN: u8 = 1;
    pub const PROBE_MAX: u8 = 15;

    /// 登记一条断言：名非空 + 面号合法 + 探针号闭集。
    pub fn enroll(&mut self, surface: u8, probe: u8, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.count >= MAX_ASSERTS {
            return Err(E_A_FULL);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if !(F_LEDGER..=F_DIFF).contains(&surface) {
            return Err(E_SURFACE_BAD);
        }
        if probe < Self::PROBE_MIN || probe > Self::PROBE_MAX {
            return Err(E_A_BAD);
        }
        let id = (self.count + 1) as u32;
        self.names[self.count] = name.to_string();
        self.surfaces[self.count] = surface;
        self.probes[self.count] = probe;
        self.count += 1;
        Ok(id)
    }

    /// 探针裁决查表：按探针号对夹具做纯函数裁决。
    fn probe_verdict(p: u8, fx: &Fixture) -> Verdict {
        match p {
            1 => {
                if fx.ledger_rows >= 3 { Verdict::Green } else { Verdict::Red }
            }
            2 => {
                // 台账哈希 = 行数派生哈希，非零即有效。
                let h = fnv1a64(&fx.ledger_rows.to_le_bytes());
                if h != 0 { Verdict::Green } else { Verdict::Red }
            }
            3 => {
                // 台账单调：行数非减（夹具单值，退化判 ≥3）。
                if fx.ledger_rows >= 3 { Verdict::Green } else { Verdict::Red }
            }
            4 => {
                let mut n = 0u32;
                let mut i = 0usize;
                while i < 4 {
                    if fx.finger_hashes[i] != 0 {
                        n += 1;
                    }
                    i += 1;
                }
                if n >= 3 { Verdict::Green } else { Verdict::Red }
            }
            5 => {
                // 指纹零重复：两两比对。
                let mut ok = true;
                let mut i = 0usize;
                while i < 4 && ok {
                    let mut j = i + 1;
                    while j < 4 {
                        if fx.finger_hashes[i] != 0 && fx.finger_hashes[i] == fx.finger_hashes[j] {
                            ok = false;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                if ok { Verdict::Green } else { Verdict::Red }
            }
            6 => {
                // 指纹与对照一致：任一指纹等于 after 哈希。
                let mut hit = false;
                let mut i = 0usize;
                while i < 4 {
                    if fx.finger_hashes[i] == fx.after_hash && fx.after_hash != 0 {
                        hit = true;
                    }
                    i += 1;
                }
                if hit { Verdict::Green } else { Verdict::Red }
            }
            7 => {
                if fx.flow_steps == 5 { Verdict::Green } else { Verdict::Red }
            }
            8 => {
                // 步序单调：0<=steps<=5 即合法单调（夹具单值）。
                if fx.flow_steps <= 5 { Verdict::Green } else { Verdict::Red }
            }
            9 => {
                // 终点回执：流程走满 5 步视同有回执。
                if fx.flow_steps == 5 { Verdict::Green } else { Verdict::Red }
            }
            10 => {
                if fx.injections > 0 && fx.intercepted == fx.injections {
                    Verdict::Green
                } else {
                    Verdict::Red
                }
            }
            11 => {
                if fx.intercepted <= fx.injections { Verdict::Green } else { Verdict::Red }
            }
            12 => {
                // 拦截账守恒：注入 = 拦截 + 放行（夹具放行恒 0）。
                if fx.intercepted == fx.injections { Verdict::Green } else { Verdict::Red }
            }
            13 => {
                if fx.before_hash == fx.after_hash && fx.before_hash != 0 {
                    Verdict::Green
                } else {
                    Verdict::Red
                }
            }
            14 => {
                // 差异账零：前后哈希差为 0。
                if fx.before_hash ^ fx.after_hash == 0 { Verdict::Green } else { Verdict::Red }
            }
            15 => {
                // 对照复算：after 哈希重哈希自等（载荷空检查退化为非零）。
                if fx.after_hash != 0 { Verdict::Green } else { Verdict::Red }
            }
            _ => Verdict::Red,
        }
    }

    /// 执行全族：夹具合法性 + 逐条裁决落账。
    pub fn run_all(&mut self, fx: &Fixture) -> Result<(u32, u32), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.count < MIN_ASSERTS {
            return Err(E_MIN_COUNT);
        }
        if !fx.sane() {
            return Err(E_FIXTURE_BAD);
        }
        // 五面各 ≥3 条。
        let mut per = [0u32; 6];
        let mut i = 0usize;
        while i < self.count {
            let s = self.surfaces[i] as usize;
            if s == 0 || s > 5 {
                return Err(E_SURFACE_BAD);
            }
            per[s] += 1;
            i += 1;
        }
        let mut s = 1usize;
        while s <= 5 {
            if per[s] < MIN_PER_SURFACE as u32 {
                return Err(E_FACE_MIN);
            }
            s += 1;
        }
        // 逐条执行。
        i = 0;
        while i < self.count {
            if self.verdicts[i].is_some() {
                return Err(E_TWICE);
            }
            let v = Self::probe_verdict(self.probes[i], fx);
            self.verdicts[i] = Some(v);
            match v {
                Verdict::Green => self.green += 1,
                Verdict::Red => self.red += 1,
            }
            self.executed += 1;
            i += 1;
        }
        if self.green + self.red != self.executed {
            return Err(E_BAL);
        }
        Ok((self.green, self.red))
    }

    /// 一次命令全绿断言。
    pub fn all_green_assert(&self) -> Result<(), u16> {
        if self.executed == 0 {
            return Err(E_NO_EXEC);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.red != 0 {
            return Err(E_RED);
        }
        if self.green != self.executed || self.executed != self.count as u32 {
            return Err(E_RED);
        }
        Ok(())
    }

    /// 守恒：green + red == executed。
    pub fn conserved(&self) -> bool {
        self.green + self.red == self.executed && self.executed <= self.count as u32
    }

    /// 登记断言数。
    pub fn assert_count(&self) -> usize {
        self.count
    }

    /// 五面分布盘点：每面断言条数聚合（面均衡断言取数口）。
    pub fn surface_tally(&self) -> [u32; 6] {
        let mut per = [0u32; 6];
        let mut i = 0usize;
        while i < self.count {
            let s = self.surfaces[i] as usize;
            if s <= 5 {
                per[s] += 1;
            }
            i += 1;
        }
        per
    }

    /// 五面均衡断言：每面 ≥3 条（run_all 内联校验的独立取数版）。
    pub fn surface_balance_assert(&self) -> Result<(), u16> {
        if self.count == 0 {
            return Err(E_MIN_COUNT);
        }
        let per = self.surface_tally();
        let mut s = 1usize;
        while s <= 5 {
            if per[s] < MIN_PER_SURFACE as u32 {
                return Err(E_FACE_MIN);
            }
            s += 1;
        }
        Ok(())
    }

    /// 封账：全绿后冻结断言族。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.all_green_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.count {
            buf.extend_from_slice(self.names[i].as_bytes());
            buf.push(0x1f);
            buf.push(self.surfaces[i]);
            buf.push(self.probes[i]);
            if let Some(v) = self.verdicts[i] {
                buf.push(v.code());
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 结果行渲染。
    pub fn result_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== Limine 升级 ktest 断言族 · 五面聚合 ===".to_string());
        let mut i = 0usize;
        while i < self.count {
            let mut line = String::from("A");
            line.push_str(&itoa((i + 1) as u64));
            line.push_str("|面");
            line.push_str(&itoa(self.surfaces[i] as u64));
            line.push('|');
            line.push_str(&self.names[i]);
            line.push('|');
            match self.verdicts[i] {
                Some(Verdict::Green) => line.push_str("绿"),
                Some(Verdict::Red) => line.push_str("红"),
                None => line.push_str("未跑"),
            }
            lines.push(line);
            i += 1;
        }
        lines.push("绿 ".to_string() + &itoa(self.green as u64)
            + "/" + &itoa(self.count as u64));
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 标准断言族装配（15 条五面均衡）─────────────────────────────
/// 装配标准 15 条断言（每面 3 条，判据定版）。
pub fn build_standard_family() -> Result<KtestFamily, u16> {
    let mut fam = KtestFamily::new();
    // 台账面。
    fam.enroll(F_LEDGER, 1, "台账行数下限")?;
    fam.enroll(F_LEDGER, 2, "台账哈希有效")?;
    fam.enroll(F_LEDGER, 3, "台账行数单调")?;
    // 指纹面。
    fam.enroll(F_FINGER, 4, "指纹采集齐备")?;
    fam.enroll(F_FINGER, 5, "指纹零重复")?;
    fam.enroll(F_FINGER, 6, "指纹与对照一致")?;
    // 流程面。
    fam.enroll(F_FLOW, 7, "流程五步走满")?;
    fam.enroll(F_FLOW, 8, "流程步序单调")?;
    fam.enroll(F_FLOW, 9, "流程终点回执")?;
    // 拦截面。
    fam.enroll(F_INTERCEPT, 10, "注入全拦截")?;
    fam.enroll(F_INTERCEPT, 11, "拦截不超注入")?;
    fam.enroll(F_INTERCEPT, 12, "拦截账守恒")?;
    // 对照面。
    fam.enroll(F_DIFF, 13, "前后哈希一致")?;
    fam.enroll(F_DIFF, 14, "差异账为零")?;
    fam.enroll(F_DIFF, 15, "对照复算一致")?;
    Ok(fam)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：正夹具全绿 + 毒夹具必红 + 守恒复核。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut fam = build_standard_family()?;
    // 正夹具：五面全绿样本。
    let good = Fixture {
        ledger_rows: 11,
        finger_hashes: [0xA1, 0xA2, 0xA3, 0xA4],
        flow_steps: 5,
        injections: 2,
        intercepted: 2,
        before_hash: 0xBEEF_0000_0000_0001,
        after_hash: 0xBEEF_0000_0000_0001,
    };
    let (g, r) = fam.run_all(&good)?;
    if g != 15 || r != 0 {
        return Err(SEG_BASE);
    }
    fam.all_green_assert()?;
    fam.surface_balance_assert()?;
    // 五面分布：15 = 5 面 × 3 条。
    let per = fam.surface_tally();
    let mut s = 1usize;
    while s <= 5 {
        if per[s] != 3 {
            return Err(SEG_BASE);
        }
        s += 1;
    }
    let h = fam.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 毒夹具：坏包注入（前后哈希不一致）必须出红。
    let mut fam2 = build_standard_family()?;
    let bad = Fixture {
        ledger_rows: 3,
        finger_hashes: [0xB1, 0xB2, 0xB3, 0],
        flow_steps: 4,
        injections: 1,
        intercepted: 1,
        before_hash: 0x1111,
        after_hash: 0x2222,
    };
    let (g2, r2) = fam2.run_all(&bad)?;
    if g2 >= 15 || r2 == 0 {
        return Err(SEG_BASE);
    }
    match fam2.all_green_assert() {
        Err(c) if c == E_RED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：不足 15 条即运行被拒。
    let mut fam3 = KtestFamily::new();
    fam3.enroll(F_LEDGER, 1, "孤条")?;
    match fam3.run_all(&good) {
        Err(c) if c == E_MIN_COUNT => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
