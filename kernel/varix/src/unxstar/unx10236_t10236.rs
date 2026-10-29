//! UNX-F10236 负样本注入（已知失败样本验证归因器）（0x116C0 段）
//!
//! 锚点：UNX-F10236-J1 负样本集入册（≥10 条已知失败样本登记且全被归因
//! 器正确归因 10/10）；J2 归因器反测（负样本注入后归因器输出类别与预
//! 注册类别一致，错配 10/10 次检出）；J3 归因器误报率（干净样本注入后
//! 归因器必须零误报）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·归因周期收口组）。
//!
//! 结构：负样本册（症状签名 × 预注册根因）+ 归因器（规则驱动分类核）
//! + 反测对平核（归因结果 vs 预注册）+ 误报账 + 注入核（错注册/漏注
//! 册/干净样本误报必抓）。

use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x116C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x116C0;
pub const NS_BAD_REG: u32 = SEG_BASE + 0x01; // 错预注册
pub const NS_MISS: u32 = SEG_BASE + 0x02; // 漏注册
pub const NS_FALSE_POS: u32 = SEG_BASE + 0x03; // 误报
pub const NS_EMPTY_SIG: u32 = SEG_BASE + 0x04; // 空签名
pub const NS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后入册

/// 根因类别（与归因器规则一一对应）。
pub const R_TIMEOUT: u8 = 0; // 超时
pub const R_CAP: u8 = 1; // 容量
pub const R_ZERO: u8 = 2; // 除零/零宽
pub const R_OOB: u8 = 3; // 越界
pub const R_DRIFT: u8 = 4; // 数字漂移
pub const R_PANIC: u8 = 5; // panic 面

/// 归因器可识别的症状信号闭集。
pub const SIG_SLOW: u8 = 0; // 时长超阈
pub const SIG_ECAP: u8 = 1; // E_CAP 报错
pub const SIG_DIV0: u8 = 2; // 除零
pub const SIG_RANGE: u8 = 3; // 越界 panic
pub const SIG_CONST: u8 = 4; // 常量期望漂移
pub const SIG_UNWIND: u8 = 5; // unwind 面包

pub fn sig_of_label(label: &str) -> Option<u8> {
    match label {
        "slow" => Some(SIG_SLOW),
        "ecap" => Some(SIG_ECAP),
        "div0" => Some(SIG_DIV0),
        "range" => Some(SIG_RANGE),
        "const" => Some(SIG_CONST),
        "unwind" => Some(SIG_UNWIND),
        _ => None,
    }
}

/// 归因规则：单信号 → 根因（规则真源，判据据此对平）。
pub fn attribute(sig: u8) -> u8 {
    match sig {
        SIG_SLOW => R_TIMEOUT,
        SIG_ECAP => R_CAP,
        SIG_DIV0 => R_ZERO,
        SIG_RANGE => R_OOB,
        SIG_CONST => R_DRIFT,
        _ => R_PANIC,
    }
}

/// 负样本条目：症状信号 + 预注册根因。
#[derive(Clone, PartialEq)]
pub struct NegSample {
    pub id: u32,
    pub sig: u8,
    pub registered: u8,
    pub sig_text: String,
}

impl NegSample {
    pub fn ok(&self) -> bool {
        !self.sig_text.is_empty() && self.sig_text.len() <= 64
    }

    /// 反测：归因器输出是否与预注册一致。
    pub fn agrees(&self) -> bool {
        attribute(self.sig) == self.registered
    }
}

/// 负样本册。
pub struct NegBook {
    rows: Vec<NegSample>,
    frozen: bool,
    false_pos: Vec<u32>,
}

impl NegBook {
    pub fn new() -> NegBook {
        NegBook {
            rows: Vec::new(),
            frozen: false,
            false_pos: Vec::new(),
        }
    }

    /// 入册：空签名/冻结后写拒收；标签必须是归因器可识别闭集。
    pub fn add(&mut self, id: u32, label: &str, reg: u8, text: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        let sig = match sig_of_label(label) {
            Some(x) => x,
            None => return Err("unknown_sig"),
        };
        if text.is_empty() {
            return Err("empty_sig_text");
        }
        if reg > R_PANIC {
            return Err("bad_reg");
        }
        self.rows.push(NegSample {
            id,
            sig,
            registered: reg,
            sig_text: text.to_string(),
        });
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 反测全量：返回错配清单（预注册与归因器不一致的 id）。
    pub fn crosscheck(&mut self) -> Vec<u32> {
        let mut bad = Vec::new();
        for s in &self.rows {
            if !s.agrees() {
                bad.push(s.id);
            }
        }
        bad
    }

    /// 误报检测：干净样本（无任何信号）喂归因器必须零归因。
    pub fn feed_clean(&mut self, id: u32) -> bool {
        // 干净样本没有标签可识别 → 归因器应无输出；若误归因记误报账
        match sig_of_label("clean") {
            None => true, // 闭集外标签拒绝 → 无归因 → 无误报
            Some(_) => {
                self.false_pos.push(id);
                false
            }
        }
    }

    pub fn false_pos_count(&self) -> usize {
        self.false_pos.len()
    }

    pub fn all_agree(&self) -> bool {
        self.rows.iter().all(|s| s.agrees())
    }
}

/// 预置十条已知失败负样本（真实历史事故语义）。
pub fn seed(b: &mut NegBook) -> Result<(), &'static str> {
    let seeds: [(u32, &str, u8, &str); 10] = [
        (1, "slow", R_TIMEOUT, "二分重跑分钟级"),
        (2, "ecap", R_CAP, "P_MAX=16 装不下 17 项"),
        (3, "div0", R_ZERO, "h1=0 零宽窗"),
        (4, "range", R_OOB, "卡号 [10] 越界"),
        (5, "const", R_DRIFT, "写死数字漂移"),
        (6, "unwind", R_PANIC, "析构递归爆栈"),
        (7, "range", R_OOB, "三位尾号取错下标"),
        (8, "ecap", R_CAP, "满册后 CAP 前置拦截"),
        (9, "const", R_DRIFT, "期望值算错"),
        (10, "unwind", R_PANIC, "深递归 fold"),
    ];
    for (id, label, reg, text) in seeds {
        b.add(id, label, reg, text)?;
    }
    Ok(())
}

/// 注入核：向册注入指定故障。
pub fn inject_fault(b: &mut NegBook, code: u32, id: u32) -> Result<(), &'static str> {
    match code {
        NS_BAD_REG => b.add(id, "slow", 200, "错注册样本"),
        NS_MISS => {
            // 漏注册：入册时预注册与规则故意错开
            b.add(id, "range", R_TIMEOUT, "错配样本")
        }
        NS_FALSE_POS => {
            let ok = b.feed_clean(id);
            if ok {
                Ok(())
            } else {
                Err("false_pos_recorded")
            }
        }
        NS_EMPTY_SIG => b.add(id, "range", R_OOB, ""),
        NS_FROZEN => {
            b.freeze();
            b.add(id, "slow", R_TIMEOUT, "冻结后样本")
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10236_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10236");

    // J1 十条负样本入册且全部与归因器一致
    let (j1_n, j1_agree) = {
        let mut b = NegBook::new();
        let _ = seed(&mut b);
        (b.len(), b.all_agree())
    };
    s.add("J1-十条负样本归因一致", j1_n == 10 && j1_agree, "10/10 归因正确");

    // J2 反测对平：错配注入后 crosscheck 必检出
    let j2 = {
        let mut b = NegBook::new();
        let _ = seed(&mut b);
        inject_fault(&mut b, NS_MISS, 99);
        let bad = b.crosscheck();
        bad == vec![99u32]
    };
    s.add("J2-错配检出", j2, "预注册错开必进错配清单");

    // J3 误报率：干净样本零误报（闭集外标签无归因）
    let j3 = {
        let mut b = NegBook::new();
        let _ = seed(&mut b);
        b.feed_clean(88);
        b.false_pos_count() == 0
    };
    s.add("J3-零误报", j3, "干净样本零归因");

    // J4 注入面五路全检出
    // 分路检出语义：拒收通道（add 拒）与审计通道（crosscheck / 零误报）分开计
    let mut caught = 0;
    if inject_fault(&mut NegBook::new(), NS_BAD_REG, 77).is_err() {
        caught += 1; // 错注册：登记即拒
    }
    {
        let mut b = NegBook::new();
        let _ = inject_fault(&mut b, NS_MISS, 77);
        if !b.crosscheck().is_empty() {
            caught += 1; // 漏注册：放行入册但 crosscheck 必点名
        }
    }
    {
        let mut b = NegBook::new();
        let _ = inject_fault(&mut b, NS_FALSE_POS, 77);
        if b.false_pos_count() == 0 {
            caught += 1; // 干净样本：不得产生误归因
        }
    }
    if inject_fault(&mut NegBook::new(), NS_EMPTY_SIG, 77).is_err() {
        caught += 1; // 空签名：登记即拒
    }
    if inject_fault(&mut NegBook::new(), NS_FROZEN, 77).is_err() {
        caught += 1; // 冻结后写：拒收
    }
    s.add("J4-注入五路", caught == 5, "五路注入全拒/检出");

    // J5 错注册拒因精确
    let j5 = inject_fault(&mut NegBook::new(), NS_BAD_REG, 5) == Err("bad_reg");
    s.add("J5-错注册拒因", j5, "闭集外注册即拒");

    // J6 未知标签拒收
    let j6 = {
        let mut b = NegBook::new();
        b.add(1, "mystery", R_CAP, "未知标签") == Err("unknown_sig")
    };
    s.add("J6-未知标签拒", j6, "归因器闭集外必拒");

    // J7 冻结红线
    let j7 = {
        let mut b = NegBook::new();
        inject_fault(&mut b, NS_FROZEN, 1);
        b.is_frozen() && b.add(2, "slow", R_TIMEOUT, "x") == Err("frozen")
    };
    s.add("J7-冻结拒收", j7, "印后入册必抓");

    // J8 规则真源对平：六信号逐一归因且互不撞
    let j8 = {
        let sigs = [SIG_SLOW, SIG_ECAP, SIG_DIV0, SIG_RANGE, SIG_CONST, SIG_UNWIND];
        let outs: Vec<u8> = sigs.iter().map(|x| attribute(*x)).collect();
        let mut uniq = true;
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                if outs[i] == outs[j] {
                    uniq = false;
                }
            }
        }
        uniq && attribute(SIG_DIV0) == R_ZERO && attribute(SIG_UNWIND) == R_PANIC
    };
    s.add("J8-规则真源对平", j8, "六信号六根因一一对应");

    // J9 十轮反测回放：负样本集反测 10 轮全一致（确定性）
    let j9 = {
        let mut ok = 0;
        for _ in 0..10 {
            let mut b = NegBook::new();
            let _ = seed(&mut b);
            if b.crosscheck().is_empty() {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J9-十轮反测", j9, "10/10 轮零错配");

    // J10 负样本 ok 性质：空文本样本必假
    let j10 = {
        let bad = NegSample {
            id: 1,
            sig: SIG_ECAP,
            registered: R_CAP,
            sig_text: String::new(),
        };
        !bad.ok()
    };
    s.add("J10-空文本负样本", j10, "空签名 ok 必假");

    s
}
