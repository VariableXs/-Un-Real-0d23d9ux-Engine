//! unx48314_cfgpriv — UNX-F48314 配置隐私账（配置不含用户输入明文，零外联）（诊断段 0xC820）。
//!
//! 判据原文：
//!   J1 扫描 ×10 零明文零外联；判据锚定 §AI-61 §2 专题六。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 隐私双红线：配置数据（键值全量）不得含用户输入明文（用户打
//!      过的字一个都不许进配置）；配置不得携外联端点（外联位恒零）。
//!   2. 明文侦测：用户输入滚动入观察窗，观察窗内容生成观察指纹集
//!      （二元组指纹：相邻两字符折叠）；配置值逐字节滑窗算指纹与
//!      观察集比对——命中即明文泄漏（E_PLAINTEXT）。
//!   3. 外联侦测：配置项外联位（endpoint 字段）恒零，非零即零外联
//!      红线破（E_PHONE_OUT）——配置面不接云。
//!   4. 扫描 ×10（J1）：十轮全量扫描（配置账逐项过），每轮双红线
//!      零命中落账（轮号、扫描项数、命中数=0）；负例注入哨兵验证
//!      侦测器非摆设（塞明文必红）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC820;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PLAINTEXT: u16 = SEG_BASE + 1; // 配置含用户输入明文
const E_PHONE_OUT: u16 = SEG_BASE + 2; // 配置携外联端点
const E_SCAN_MISS: u16 = SEG_BASE + 3; // 扫描轮不足
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 隐私账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 扫描器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 扫描口径。
pub const SCAN_ROUNDS: u32 = 10;

/// 配置项一条。
#[derive(Clone, Debug)]
pub struct CfgItem {
    pub key: u16,
    pub val: Vec<u8>,       // 配置值字节（隐私面）
    pub endpoint: u16,      // 外联端点位（0 = 零外联）
}

/// 隐私扫描器。
pub struct PrivacyScan {
    watch: Vec<u16>, // 观察指纹集（用户输入二元组折叠）
    pub rounds: u32,
    pub clean_rounds: u32,
    pub scan_log: Vec<(u32, u32, u32)>, // (轮号, 扫描项数, 命中数)
    pub items_scanned: u64,
}

impl PrivacyScan {
    pub fn new() -> PrivacyScan {
        PrivacyScan {
            watch: Vec::new(),
            rounds: 0,
            clean_rounds: 0,
            scan_log: Vec::new(),
            items_scanned: 0,
        }
    }

    /// 用户输入滚动入观察窗：相邻两字符折叠成指纹入观察集。
    pub fn observe_input(&mut self, stream: &[u16]) {
        let mut i = 1usize;
        while i < stream.len() {
            let fp = pair_fp(stream[i - 1], stream[i]);
            // 观察集去重。
            let mut hit = false;
            let mut j = 0usize;
            while j < self.watch.len() {
                if self.watch[j] == fp {
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                self.watch.push(fp);
            }
            i += 1;
        }
    }

    pub fn watch_size(&self) -> usize {
        self.watch.len()
    }

    /// 单项扫描：值域滑窗二元组指纹比对观察集 + 外联位核验。
    /// 返回 Ok(()) 为双红线零命中。
    pub fn scan_item(&mut self, item: &CfgItem) -> Result<(), u16> {
        self.items_scanned += 1;
        // 红线二：外联位恒零。
        if item.endpoint != 0 {
            return Err(E_PHONE_OUT);
        }
        // 红线一：值域滑窗指纹比对。
        if item.val.len() >= 2 {
            let mut i = 1usize;
            while i < item.val.len() {
                let fp = pair_fp(item.val[i - 1] as u16, item.val[i] as u16);
                let mut j = 0usize;
                while j < self.watch.len() {
                    if self.watch[j] == fp {
                        return Err(E_PLAINTEXT);
                    }
                    j += 1;
                }
                i += 1;
            }
        }
        Ok(())
    }

    /// 一轮全量扫描（J1 主体）：配置账逐项过，双红线零命中即本轮绿、
    /// 落账留痕；任一命中即整轮红带诊断码。
    pub fn scan_round(&mut self, round: u32, items: &[CfgItem]) -> Result<(), u16> {
        self.rounds += 1;
        if round != self.rounds {
            return Err(E_STATE_BAD);
        }
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < items.len() {
            if self.scan_item(&items[i]).is_err() {
                hits += 1;
            }
            i += 1;
        }
        self.scan_log.push((round, items.len() as u32, hits));
        if hits == 0 {
            self.clean_rounds += 1;
            Ok(())
        } else {
            Err(E_PLAINTEXT)
        }
    }

    /// 隐私账审计：×10 轮全绿、扫描日志与轮数守恒、命中账闭合
    /// （日志命中和 = 全轮命中量）。
    pub fn audit(&self, want_rounds: u32) -> Result<(), u16> {
        if self.clean_rounds < want_rounds {
            return Err(E_SCAN_MISS);
        }
        if self.scan_log.len() as u32 != self.rounds {
            return Err(E_LEDGER_BAD);
        }
        let mut sum_hits = 0u32;
        let mut i = 0usize;
        while i < self.scan_log.len() {
            let (r, n, h) = self.scan_log[i];
            if r != (i + 1) as u32 {
                return Err(E_LEDGER_BAD);
            }
            sum_hits += h;
            let _ = n;
            i += 1;
        }
        // 清轮 = 总轮 - 红轮；日志命中和与清轮口径闭合：全清轮时命中和 0。
        if self.clean_rounds == self.rounds && sum_hits != 0 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：扫描达标 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.clean_rounds == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 二元组指纹：相邻两字符折叠（观察集与扫描面同源函数）。
pub fn pair_fp(a: u16, b: u16) -> u16 {
    a.wrapping_mul(31).wrapping_add(b).wrapping_mul(3) ^ 0x5A17
}

/// 固定配置构造器（隐私面常量账：值域全来自配置常量，与用户输入无关）。
pub fn const_configs() -> Vec<CfgItem> {
    let mut items: Vec<CfgItem> = Vec::new();
    let mut v1: Vec<u8> = Vec::new();
    v1.push(0x01);
    v1.push(0x05); // 候选数 5
    let mut v2: Vec<u8> = Vec::new();
    v2.push(0x00);
    v2.push(0x14); // 预算 20ms
    let mut v3: Vec<u8> = Vec::new();
    v3.push(0x00); // 方案开关
    items.push(CfgItem { key: 0x0012, val: v1, endpoint: 0 });
    items.push(CfgItem { key: 0x0013, val: v2, endpoint: 0 });
    items.push(CfgItem { key: 0x0021, val: v3, endpoint: 0 });
    items
}

/// 自例程判据：逐条落实 J1（扫描 ×10 零明文零外联），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：用户输入滚动入观察窗（真实键流），常量配置账
    //    ×10 轮全量扫描双红线零命中。
    let mut ps = PrivacyScan::new();
    let mut input: Vec<u16> = Vec::new();
    let mut i = 0u32;
    while i < 32 {
        input.push(0x41 + (i * 7 % 26) as u16); // 用户 ASCII 键流
        i += 1;
    }
    ps.observe_input(&input);
    if ps.watch_size() == 0 {
        return Err(E_STATE_BAD); // 观察窗未建立
    }
    let items = const_configs();
    let mut r = 1u32;
    while r <= SCAN_ROUNDS {
        ps.scan_round(r, &items)?;
        r += 1;
    }
    ps.audit(SCAN_ROUNDS)?;
    if ps.clean_rounds != SCAN_ROUNDS {
        return Err(E_SCAN_MISS);
    }
    hits += 1;

    // R2 明文侦测负例（哨兵）：把用户输入二元组直接塞进配置值 →
    //    扫描必报 E_PLAINTEXT（侦测器非摆设）。
    let mut poisoned = const_configs();
    let mut pv: Vec<u8> = Vec::new();
    pv.push(input[0] as u8);
    pv.push(input[1] as u8); // 用户二元组低字节注入
    poisoned.push(CfgItem { key: 0x9999, val: pv, endpoint: 0 });
    if ps.scan_round(11, &poisoned).is_ok() {
        return Err(E_PLAINTEXT);
    }
    if ps.clean_rounds != SCAN_ROUNDS {
        return Err(E_SCAN_MISS); // 红轮不计入清轮
    }
    hits += 1;

    // R3 外联负例（哨兵）：endpoint 非零 → E_PHONE_OUT，单项扫描即拦。
    let mut phoned = CfgItem { key: 0x8888, val: Vec::new(), endpoint: 0x0AC0 };
    if ps.scan_item(&phoned).is_ok() {
        return Err(E_PHONE_OUT);
    }
    phoned.endpoint = 0xFFFF;
    if ps.scan_item(&phoned).is_ok() {
        return Err(E_PHONE_OUT);
    }
    hits += 1;

    // R4 观察窗构建核：单字流（无相邻对）不产指纹（观察集空）；
    //    重复二元组去重（同指纹只入一次）。
    let mut ps2 = PrivacyScan::new();
    ps2.observe_input(&[0x4E00]);
    if ps2.watch_size() != 0 {
        return Err(E_LEDGER_BAD);
    }
    ps2.observe_input(&[0x4E00, 0x4E01, 0x4E00, 0x4E01]);
    if ps2.watch_size() != 2 {
        return Err(E_LEDGER_BAD); // (E00,E01) 与 (E01,E00) 两指纹，重复去重
    }
    hits += 1;

    // R5 扫描日志闭合：R1 十轮 + R2 一轮红 → 日志 11 条、轮号 1..11、
    //    清轮 10、红轮命中 1。
    if ps.scan_log.len() != 11 {
        return Err(E_LEDGER_BAD);
    }
    if ps.scan_log[10].0 != 11 || ps.scan_log[10].2 != 1 {
        return Err(E_LEDGER_BAD);
    }
    let mut sum_hits = 0u32;
    let mut i = 0usize;
    while i < ps.scan_log.len() {
        sum_hits += ps.scan_log[i].2;
        i += 1;
    }
    if sum_hits != 1 {
        return Err(E_LEDGER_BAD);
    }
    if ps.items_scanned != 34 {
        return Err(E_LEDGER_BAD); // 前十轮各 3 项 + 红轮 4 项
    }
    hits += 1;

    // R6 短值面：零长度/单字节值跳过滑窗比对（无二元组可泄）仍须过
    //    外联红线。
    let mut tiny = CfgItem { key: 0x0007, val: Vec::new(), endpoint: 0 };
    ps2.scan_item(&tiny)?;
    tiny.val.push(0x42);
    ps2.scan_item(&tiny)?;
    hits += 1;

    // R7 轮号违规负例：轮号跳号 → E_STATE_BAD（扫描账轮号闭合）。
    let mut ps3 = PrivacyScan::new();
    ps3.scan_round(1, &items)?;
    if ps3.scan_round(3, &items).is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零清轮不写回、空摘要拒、×10 达标+非空摘要放行。
    if ps2.writeback(true, 0).is_ok() {
        return Err(E_STATE_BAD);
    }
    if ps.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ps.writeback(true, 0x0A)?;
    hits += 1;

    Ok(hits)
}
