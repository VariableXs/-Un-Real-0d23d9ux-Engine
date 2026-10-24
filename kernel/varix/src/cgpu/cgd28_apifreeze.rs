//! cgd28_apifreeze — CGPU-F0508 抖动抑制 API 冻结 v1（签名冻结/版本化/命名一致/十年承诺）。
//!
//! 判据原文（施工书锚点 CGPU-F0508）：API：抖动抑制族冻结（检测订阅/策略
//! 配置/均衡器控制/验证接口——签名冻结版本化）；与 D01 帧计时 API 命名一
//! 致；十年承诺。判据：冻结、一致、承诺、判据。
//!
//! 模型：四族 API 以常量签名表冻结（名/参数数/版本号三属性）；版本 v1 单
//! 向增长（只加不破）；命名对账与 D01 前缀规则一致（cgpu_jitter_* / cgpu_
//! frame_*）；十年承诺以承诺期常量与承诺声明结构落账，任何冻结签名被改动
//! 即段对账拒绝。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9D00（cgpu 域内唯一）。
pub const XC_AF_NOT_FROZEN: u32 = 0x9D01;
pub const XC_AF_NAME_MISMATCH: u32 = 0x9D02;
pub const XC_AF_ARITY_MISMATCH: u32 = 0x9D03;
pub const XC_AF_VERSION_REGRESS: u32 = 0x9D04;
pub const XC_AF_PROMISE_MISSING: u32 = 0x9D05;

/// API 冻结版本。
pub const API_VERSION: u32 = 1;

/// 十年承诺期（月）。
pub const PROMISE_MONTHS: u32 = 120;

/// 四族编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFamily {
    /// 检测订阅。
    Subscribe = 0,
    /// 策略配置。
    Policy = 1,
    /// 均衡器控制。
    Balance = 2,
    /// 验证接口。
    Verif = 3,
}

/// 一条冻结签名。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApiSig {
    /// 冻结名。
    pub name: &'static str,
    /// 参数个数。
    pub arity: u8,
    /// 引入版本。
    pub since: u32,
    /// 所属族。
    pub family: ApiFamily,
}

/// 冻结正本 v1（四族各一条代表签名，扩展只追加）。
pub const FROZEN_V1: [ApiSig; 4] = [
    ApiSig {
        name: "cgpu_jitter_subscribe",
        arity: 2,
        since: 1,
        family: ApiFamily::Subscribe,
    },
    ApiSig {
        name: "cgpu_jitter_policy_set",
        arity: 3,
        since: 1,
        family: ApiFamily::Policy,
    },
    ApiSig {
        name: "cgpu_jitter_balance_ctl",
        arity: 2,
        since: 1,
        family: ApiFamily::Balance,
    },
    ApiSig {
        name: "cgpu_jitter_verif_run",
        arity: 1,
        since: 1,
        family: ApiFamily::Verif,
    },
];

/// D01 帧计时冻结正本（命名一致性对账基准）。
pub const D01_FROZEN: [ApiSig; 2] = [
    ApiSig {
        name: "cgpu_frame_tick_get",
        arity: 1,
        since: 1,
        family: ApiFamily::Subscribe,
    },
    ApiSig {
        name: "cgpu_frame_budget_set",
        arity: 2,
        since: 1,
        family: ApiFamily::Policy,
    },
];

/// 命名一致性规则（D01 惯例）：前缀 `cgpu_jitter_` 或 `cgpu_frame_`、
/// 全小写下划线、无连字符、以动词收尾词非空。
pub fn name_conforms(name: &str) -> bool {
    let ok_prefix = name.starts_with("cgpu_jitter_") || name.starts_with("cgpu_frame_");
    if !ok_prefix {
        return false;
    }
    let mut bytes = name.bytes();
    let mut prev_underscore = false;
    while let Some(b) = bytes.next() {
        let legal = b == b'_' || (b >= b'a' && b <= b'z') || (b >= b'0' && b <= b'9');
        if !legal {
            return false;
        }
        if b == b'_' && prev_underscore {
            return false;
        }
        prev_underscore = b == b'_';
    }
    // 尾部不得以下划线收尾。
    !name.ends_with('_')
}

/// 版本校验：since ∈ 1..=当前版本。
pub fn since_valid(since: u32) -> bool {
    since >= 1 && since <= API_VERSION
}

/// 冻结账：签名一旦入账不可改（同名额即视为改动企图）。
#[derive(Debug)]
pub struct FreezeBook {
    sigs: Vec<ApiSig>,
    /// 承诺声明（月数）。
    promise_months: Option<u32>,
}

impl FreezeBook {
    /// 空账。
    pub fn new() -> Self {
        FreezeBook {
            sigs: Vec::new(),
            promise_months: None,
        }
    }

    /// 导入冻结正本。
    pub fn load_v1(&mut self) -> Result<usize, u32> {
        let mut i = 0;
        while i < FROZEN_V1.len() {
            self.enroll(&FROZEN_V1[i])?;
            i += 1;
        }
        Ok(self.sigs.len())
    }

    /// 入账：重名拒（冻结不可改）、since 越界拒。
    pub fn enroll(&mut self, s: &ApiSig) -> Result<(), u32> {
        if !since_valid(s.since) {
            return Err(XC_AF_VERSION_REGRESS);
        }
        if !name_conforms(s.name) {
            return Err(XC_AF_NAME_MISMATCH);
        }
        let mut i = 0;
        while i < self.sigs.len() {
            if self.sigs[i].name == s.name {
                return Err(XC_AF_NOT_FROZEN);
            }
            i += 1;
        }
        self.sigs.push(*s);
        Ok(())
    }

    /// 签名条数。
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }

    /// 查签名。
    pub fn find(&self, name: &str) -> Option<&ApiSig> {
        let mut i = 0;
        while i < self.sigs.len() {
            if self.sigs[i].name == name {
                return Some(&self.sigs[i]);
            }
            i += 1;
        }
        None
    }

    /// 参数数对账：给定名与期望参数数。
    pub fn assert_arity(&self, name: &str, arity: u8) -> Result<(), u32> {
        let s = self.find(name).ok_or(XC_AF_NOT_FROZEN)?;
        if s.arity != arity {
            return Err(XC_AF_ARITY_MISMATCH);
        }
        Ok(())
    }

    /// 承诺声明：一次性落账，重复声明拒。
    pub fn declare_promise(&mut self, months: u32) -> Result<(), u32> {
        if months < PROMISE_MONTHS {
            return Err(XC_AF_PROMISE_MISSING);
        }
        if self.promise_months.is_some() {
            return Err(XC_AF_PROMISE_MISSING);
        }
        self.promise_months = Some(months);
        Ok(())
    }

    /// 承诺在账？
    pub fn promise_ok(&self) -> bool {
        matches!(self.promise_months, Some(m) if m >= PROMISE_MONTHS)
    }

    /// 命名与 D01 一致性对账：全部在账签名过命名规则。
    pub fn names_all_conform(&self) -> bool {
        let mut i = 0;
        while i < self.sigs.len() {
            if !name_conforms(self.sigs[i].name) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 四族覆盖对账：每族至少一条在账。
    pub fn families_covered(&self) -> bool {
        let mut cover = [false; 4];
        let mut i = 0;
        while i < self.sigs.len() {
            let f = self.sigs[i].family as usize;
            if f < 4 {
                cover[f] = true;
            }
            i += 1;
        }
        cover[0] && cover[1] && cover[2] && cover[3]
    }
}

impl Default for FreezeBook {
    fn default() -> Self {
        Self::new()
    }
}

/// API 冻结总门面。
#[derive(Debug, Default)]
pub struct ApiFreezeEngine {
    /// 冻结账。
    pub book: FreezeBook,
}

impl ApiFreezeEngine {
    /// 自检一：v1 正本入账——四族四签名齐备。
    pub fn selfcheck_freeze(&mut self) -> Result<(), u32> {
        let n = self.book.load_v1()?;
        if n != 4 {
            return Err(XC_AF_NOT_FROZEN);
        }
        if !self.book.families_covered() {
            return Err(XC_AF_NOT_FROZEN);
        }
        // 重名入账拒（冻结不可改）。
        if self.book.enroll(&FROZEN_V1[0]).is_ok() {
            return Err(XC_AF_NOT_FROZEN);
        }
        Ok(())
    }

    /// 自检二：命名一致——本族与 D01 全过规则；反例逐个被拒。
    pub fn selfcheck_names(&self) -> Result<(), u32> {
        if !self.book.names_all_conform() {
            return Err(XC_AF_NAME_MISMATCH);
        }
        // D01 基准全过。
        let mut i = 0;
        while i < D01_FROZEN.len() {
            if !name_conforms(D01_FROZEN[i].name) {
                return Err(XC_AF_NAME_MISMATCH);
            }
            i += 1;
        }
        let bad = [
            "gpu_jitter_x",
            "cgpu_jitter_x-y",
            "cgpu_jitter_X",
            "cgpu_jitter_",
            "cgpu_jitter__x",
            "cgpu_frame_tICK",
        ];
        let mut i = 0;
        while i < bad.len() {
            if name_conforms(bad[i]) {
                return Err(XC_AF_NAME_MISMATCH);
            }
            i += 1;
        }
        // 合名可入账。
        let mut b2 = FreezeBook::new();
        b2.enroll(&ApiSig {
            name: "cgpu_jitter_threshold_set",
            arity: 1,
            since: 1,
            family: ApiFamily::Policy,
        })?;
        Ok(())
    }

    /// 自检三：签名参数数对账——四签名逐一核对、错参拒、未知名拒。
    pub fn selfcheck_arity(&self) -> Result<(), u32> {
        self.book.assert_arity("cgpu_jitter_subscribe", 2)?;
        self.book.assert_arity("cgpu_jitter_policy_set", 3)?;
        self.book.assert_arity("cgpu_jitter_balance_ctl", 2)?;
        self.book.assert_arity("cgpu_jitter_verif_run", 1)?;
        // 错参拒。
        if self.book.assert_arity("cgpu_jitter_subscribe", 3).is_ok() {
            return Err(XC_AF_ARITY_MISMATCH);
        }
        // 未知名拒。
        if self.book.assert_arity("cgpu_jitter_missing", 1).is_ok() {
            return Err(XC_AF_NOT_FROZEN);
        }
        Ok(())
    }

    /// 自检四：版本——since 越界拒、v1 扩展可追加（只加不破）。
    pub fn selfcheck_version(&self) -> Result<(), u32> {
        if since_valid(0) || since_valid(API_VERSION + 1) {
            return Err(XC_AF_VERSION_REGRESS);
        }
        let mut b = FreezeBook::new();
        // v2 引入拒（版本回退/超前皆拒）。
        if b.enroll(&ApiSig {
            name: "cgpu_jitter_future_api",
            arity: 1,
            since: 2,
            family: ApiFamily::Balance,
        })
        .is_ok()
        {
            return Err(XC_AF_VERSION_REGRESS);
        }
        // v1 追加可。
        b.enroll(&ApiSig {
            name: "cgpu_jitter_spectrum_get",
            arity: 1,
            since: 1,
            family: ApiFamily::Verif,
        })?;
        if b.len() != 1 {
            return Err(XC_AF_NOT_FROZEN);
        }
        Ok(())
    }

    /// 自检五：十年承诺——低于承诺期拒、恰十年过、重复声明拒、判据收口。
    pub fn selfcheck_promise(&mut self) -> Result<(), u32> {
        let mut b = FreezeBook::new();
        if b.declare_promise(PROMISE_MONTHS - 1).is_ok() {
            return Err(XC_AF_PROMISE_MISSING);
        }
        b.declare_promise(PROMISE_MONTHS)?;
        if !b.promise_ok() {
            return Err(XC_AF_PROMISE_MISSING);
        }
        if b.declare_promise(PROMISE_MONTHS * 2).is_ok() {
            return Err(XC_AF_PROMISE_MISSING);
        }
        // 本账补承诺。
        if !self.book.promise_ok() {
            self.book.declare_promise(PROMISE_MONTHS)?;
        }
        if !self.book.promise_ok() {
            return Err(XC_AF_PROMISE_MISSING);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9D00..0x9E00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_AF_NOT_FROZEN,
            XC_AF_NAME_MISMATCH,
            XC_AF_ARITY_MISMATCH,
            XC_AF_VERSION_REGRESS,
            XC_AF_PROMISE_MISSING,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9D00 || codes[i] >= 0x9E00 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd28_selfchecks() {
        let mut e = ApiFreezeEngine::default();
        e.selfcheck_freeze().unwrap();
        e.selfcheck_names().unwrap();
        e.selfcheck_arity().unwrap();
        e.selfcheck_version().unwrap();
        e.selfcheck_promise().unwrap();
        assert_eq!(ApiFreezeEngine::segment_guard(), Ok(()));
    }
}
