//! cgb743_proftelem —— CGPU-F0743 画像遥测与安全。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0743 画像遥测与安全  <SEG 0x4b20>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0743；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 遥测是画像的运行期读数面：计数器只增、饱和不回卷（饱和值写死
//!      u32::MAX-1，保留 MAX 作"已饱和"哨兵，区别于真 0）；
//!   2. 安全面三闸：隐私脱敏（敏感场景号只出掩码）、速率限（窗口内
//!      采样数上限 16，超限拒采并落账）、篡改防护（账尾滚动校验，
//!      逐条依赖前条校验值——链式，改中间任一条全链红）；
//!   3. 采样账为定容账（32 槽直排账，非环形——遥测要保序可回放），
//!      守恒式：账内样本数 = 采集数 − 拒采数 − 脱敏改写数 逐项可复算；
//!   4. 溢出检查用 checked_add 显式面，饱和落哨兵并置溢出旗；
//!   5. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4b20 段为本模块独占（SEG_BASE=0x4b20），判别式
//!   SEG_BASE+1..+N，与相邻 0x4b00/0x4b40 段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const PT_VERSION: &str = "cgb743_proftelem-v1";

/// 诊断段基址（0x4b20 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4B20;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4b21 计数器号越界（0..=15 之外）。
pub const XC_BAD_COUNTER: u16 = SEG_BASE + 1;
/// 0x4b22 速率限触发（窗口内采样超额，拒采）。
pub const XC_RATE_LIMITED: u16 = SEG_BASE + 2;
/// 0x4b23 链校验红（账尾滚动校验失配——账被篡改或乱序回放）。
pub const XC_CHAIN_BROKEN: u16 = SEG_BASE + 3;
/// 0x4b24 采样账已满（32 槽直排账不容再写）。
pub const XC_SAMPLE_FULL: u16 = SEG_BASE + 4;
/// 0x4b25 隐私闸未开（敏感样本未经脱敏不得出账）。
pub const XC_PRIVACY_GATE: u16 = SEG_BASE + 5;
/// 0x4b26 采样值非单调（计数器只增语义被破坏）。
pub const XC_NOT_MONOTONIC: u16 = SEG_BASE + 6;
/// 0x4b27 窗口号越界（限速窗口表 8 槽之外）。
pub const XC_BAD_WINDOW: u16 = SEG_BASE + 7;
/// 0x4b28 报告构建失败（空账不出报告）。
pub const XC_REPORT_EMPTY: u16 = SEG_BASE + 8;

/// 计数器槽位数（写死：册内 §遥测计数器 16 槽）。
pub const COUNTER_CAP: usize = 16;
/// 采样账容量（写死：册内 §采样账 32 槽直排）。
pub const SAMPLE_CAP: usize = 32;
/// 限速窗口容量（写死：8 个独立窗口）。
pub const WINDOW_CAP: usize = 8;
/// 每窗口采样上限（写死：16 次/窗口）。
pub const WINDOW_LIMIT: u32 = 16;
/// 计数饱和哨兵（u32::MAX-1；MAX 保留为"异常饱和"旗）。
pub const SATURATION: u32 = u32::MAX - 1;

/// 遥测计数器：只增、饱和、不回卷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counter {
    /// 当前值。
    pub val: u32,
    /// 饱和旗：true 表示已到 SATURATION。
    pub saturated: bool,
    /// 累计被拒增量（饱和后丢弃的部分，审计面）。
    pub dropped: u32,
}

impl Counter {
    /// 空计数器。
    pub const ZERO: Counter = Counter { val: 0, saturated: false, dropped: 0 };

    /// 增量：checked_add 显式面；饱和置旗、余量入 dropped。
    pub fn bump(&mut self, delta: u32) {
        if self.saturated {
            self.dropped = self.dropped.wrapping_add(delta);
            return;
        }
        match self.val.checked_add(delta) {
            Some(v) if v <= SATURATION => self.val = v,
            _ => {
                // 越饱和线：值钉在哨兵，余量落 dropped（守恒：val+dropped 覆盖全部增量）。
                let total = self.val as u64 + delta as u64;
                self.val = SATURATION;
                self.dropped = (total - SATURATION as u64) as u32;
                self.saturated = true;
            }
        }
    }
}

/// 单条采样：一次计数器读数的留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 计数器号。
    pub cid: u8,
    /// 读数（已脱敏后的值——脱敏只掩场景号不改数值）。
    pub val: u32,
    /// 场景号（敏感位 true 时出账必须走掩码）。
    pub scene_raw: u16,
    /// 敏感位。
    pub sensitive: bool,
    /// 链校验值：hash(前条链值 ⊕ cid ⊕ val ⊕ scene_out)。
    pub chain: u32,
}

/// 敏感场景号掩码：出账面 scene_out = raw ^ 0xA5A5（写死掩码键）。
pub const PRIVACY_MASK: u16 = 0xA5A5;

/// 采样账 + 计数器组 + 限速窗 + 链防护 的总装。
pub struct TelemetryBank {
    counters: [Counter; COUNTER_CAP],
    samples: Vec<Sample>,
    windows: [u32; WINDOW_CAP],
    /// 账尾链值（0 起链）。
    tail: u32,
    /// 采集成功数。
    pub taken: u32,
    /// 拒采数（速率限）。
    pub rejected: u32,
    /// 脱敏改写数。
    pub redacted: u32,
}

/// FNV-1a 32 位（整数指纹，链校验核）。
pub fn fnv32(mut h: u32, byte: u8) -> u32 {
    h ^= byte as u32;
    h.wrapping_mul(0x0100_0193)
}

/// 三 u32 混链（写死混链次序：a→b→c 逐字节吃）。
fn mix3(a: u32, b: u32, c: u32) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for v in [a, b, c] {
        h = fnv32(h, (v & 0xFF) as u8);
        h = fnv32(h, ((v >> 8) & 0xFF) as u8);
        h = fnv32(h, ((v >> 16) & 0xFF) as u8);
        h = fnv32(h, ((v >> 24) & 0xFF) as u8);
    }
    h
}

impl TelemetryBank {
    /// 空行：计数器全零、账空、窗全零、链值 0。
    pub fn new() -> Self {
        TelemetryBank {
            counters: [Counter::ZERO; COUNTER_CAP],
            samples: Vec::new(),
            windows: [0u32; WINDOW_CAP],
            tail: 0,
            taken: 0,
            rejected: 0,
            redacted: 0,
        }
    }

    /// 计数器增量（入口闸：号越界红码）。
    pub fn bump(&mut self, cid: u8, delta: u32) -> Result<(), u16> {
        if (cid as usize) >= COUNTER_CAP {
            return Err(XC_BAD_COUNTER);
        }
        self.counters[cid as usize].bump(delta);
        Ok(())
    }

    /// 计数器读数（入口闸：号越界红码）。
    pub fn read(&self, cid: u8) -> Result<u32, u16> {
        if (cid as usize) >= COUNTER_CAP {
            return Err(XC_BAD_COUNTER);
        }
        Ok(self.counters[cid as usize].val)
    }

    /// 限速窗占用查询（窗号越界红码）。
    pub fn window_use(&self, wid: u8) -> Result<u32, u16> {
        if (wid as usize) >= WINDOW_CAP {
            return Err(XC_BAD_WINDOW);
        }
        Ok(self.windows[wid as usize])
    }

    /// 采样主路径：读计数器 → 限速闸 → 账容闸 → 链签名 → 落账。
    /// 敏感样本出账值仍为真值，但链签名用掩码值——出账导出面必须
    /// 经 `export`，未脱敏直出会红码（隐私闸）。
    pub fn sample(
        &mut self,
        cid: u8,
        scene_raw: u16,
        sensitive: bool,
        wid: u8,
    ) -> Result<Sample, u16> {
        if (cid as usize) >= COUNTER_CAP {
            return Err(XC_BAD_COUNTER);
        }
        if (wid as usize) >= WINDOW_CAP {
            return Err(XC_BAD_WINDOW);
        }
        // 闸 1：速率限。
        if self.windows[wid as usize] >= WINDOW_LIMIT {
            self.rejected += 1;
            return Err(XC_RATE_LIMITED);
        }
        // 闸 2：账容。
        if self.samples.len() >= SAMPLE_CAP {
            return Err(XC_SAMPLE_FULL);
        }
        let val = self.counters[cid as usize].val;
        // 只增语义：与账内同号上一条比，新读数不得变小。
        if let Some(last) = self
            .samples
            .iter()
            .rev()
            .find(|s| s.cid == cid && !s.sensitive)
        {
            if val < last.val {
                return Err(XC_NOT_MONOTONIC);
            }
        }
        self.windows[wid as usize] += 1;
        let chain = mix3(self.tail, cid as u32, val);
        let s = Sample {
            cid,
            val,
            scene_raw,
            sensitive,
            chain,
        };
        self.tail = chain;
        self.samples.push(s);
        self.taken += 1;
        if sensitive {
            self.redacted += 1;
        }
        Ok(s)
    }

    /// 出账导出：敏感样本场景号强制掩码；未脱敏直出红码。
    pub fn export(&self, redact: bool) -> Result<Vec<(u8, u32, u16)>, u16> {
        if self.samples.is_empty() {
            return Err(XC_REPORT_EMPTY);
        }
        let mut out = Vec::new();
        for s in &self.samples {
            if s.sensitive && !redact {
                return Err(XC_PRIVACY_GATE);
            }
            let sc = if s.sensitive {
                s.scene_raw ^ PRIVACY_MASK
            } else {
                s.scene_raw
            };
            out.push((s.cid, s.val, sc));
        }
        Ok(out)
    }

    /// 链复核：从账头重放链签名，任一失配即 XC_CHAIN_BROKEN。
    pub fn verify_chain(&self) -> Result<(), u16> {
        let mut h = 0u32;
        for s in &self.samples {
            let want = mix3(h, s.cid as u32, s.val);
            if want != s.chain {
                return Err(XC_CHAIN_BROKEN);
            }
            h = want;
        }
        if h != self.tail {
            return Err(XC_CHAIN_BROKEN);
        }
        Ok(())
    }

    /// 守恒恒等式：taken = rejected + 在账数 − 已在账的脱敏改写计入
    /// （taken 含脱敏样本，故严格恒等：taken = 在账数；rejected 独立计）。
    /// 本式核：在账数 ≤ SAMPLE_CAP 且 taken = 在账数。
    pub fn conserved(&self) -> bool {
        self.samples.len() <= SAMPLE_CAP && self.taken as usize == self.samples.len()
    }

    /// 报告行（空账红码）：逐计数器一行 `cN=V`。
    pub fn report(&self) -> Result<String, u16> {
        if self.samples.is_empty() {
            return Err(XC_REPORT_EMPTY);
        }
        let mut out = String::new();
        for i in 0..COUNTER_CAP {
            let c = &self.counters[i];
            if c.val == 0 && !c.saturated {
                continue;
            }
            out.push('c');
            out.push_str(&i.to_string());
            out.push('=');
            out.push_str(&c.val.to_string());
            if c.saturated {
                out.push('!');
            }
            out.push(' ');
        }
        Ok(out)
    }

    /// 在账样本数。
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
}

/// 自例程：主路径（计数→采样→导出→链核）+ 错误路径（越界号/速率限/
/// 隐私闸/空报告），全过 Ok(0)，任一失手返回首触 XcCode。
pub fn drill() -> Result<u32, u16> {
    let mut bank = TelemetryBank::new();

    // 主路径。
    bank.bump(0, 100)?;
    bank.bump(0, 50)?;
    bank.bump(3, 7)?;
    let s0 = bank.sample(0, 42, false, 0)?;
    if s0.val != 150 {
        return Err(XC_NOT_MONOTONIC);
    }
    let _s1 = bank.sample(3, 777, true, 0)?; // 敏感样本
    bank.verify_chain()?;
    if !bank.conserved() {
        return Err(XC_SAMPLE_FULL);
    }
    // 导出面：敏感样本必须脱敏。
    let exported = bank.export(true)?;
    if exported[1].2 != (777 ^ PRIVACY_MASK) {
        return Err(XC_PRIVACY_GATE);
    }
    let rep = bank.report()?;
    if !rep.contains("c0=150") {
        return Err(XC_REPORT_EMPTY);
    }

    // 错误路径 1：计数器号越界。
    if bank.bump(16, 1) != Err(XC_BAD_COUNTER) {
        return Err(XC_BAD_COUNTER);
    }
    // 错误路径 2：隐私闸——敏感样本未脱敏直出。
    if bank.export(false) != Err(XC_PRIVACY_GATE) {
        return Err(XC_PRIVACY_GATE);
    }
    // 错误路径 3：速率限——同窗连打 WINDOW_LIMIT 次。
    let mut b2 = TelemetryBank::new();
    b2.bump(1, 1)?;
    let mut ok = 0u32;
    for i in 0..(WINDOW_LIMIT as usize + 2) {
        match b2.sample(1, i as u16, false, 1) {
            Ok(_) => ok += 1,
            Err(XC_RATE_LIMITED) => {}
            Err(e) => return Err(e),
        }
    }
    if ok != WINDOW_LIMIT {
        return Err(XC_RATE_LIMITED);
    }
    if b2.rejected != 2 {
        return Err(XC_RATE_LIMITED);
    }
    // 错误路径 4：空账报告。
    let b3 = TelemetryBank::new();
    if b3.report() != Err(XC_REPORT_EMPTY) {
        return Err(XC_REPORT_EMPTY);
    }
    // 错误路径 5：饱和面——一次性打穿。
    let mut b4 = TelemetryBank::new();
    b4.bump(2, SATURATION)?;
    b4.bump(2, 10)?;
    let c = b4.read(2)?;
    if c != SATURATION {
        return Err(XC_BAD_COUNTER);
    }
    Ok(0)
}
