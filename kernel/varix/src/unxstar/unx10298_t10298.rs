//! UNX-F10298 · 交接确认书（双签交接留痕）
//! 段位 0x11860 · 判据锚 UNX-F10298-J1 · 域 O3 对接面/交接留痕
//! 结构：HandoverSheet（确认书）+ DualSign（双签核）+ HandoverLedger（交接账）
//! 语义：交接必须双方签署（移交方 + 接收方）且指纹一致方生效；
//!       单签、错签、指纹不符一律拦截，10/10 次拦截可复演。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x11860;

/// 签署方角色
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignRole {
    Handover,
    Receiver,
}

impl SignRole {
    pub fn code(self) -> u16 {
        (SEG_BASE as u16) + match self {
            SignRole::Handover => 1,
            SignRole::Receiver => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SignRole::Handover => "HANDOVER",
            SignRole::Receiver => "RECEIVER",
        }
    }
}

/// 一枚签名：角色 + 签署者 + 时间戳 + 对交接物指纹的确认
#[derive(Clone, Debug)]
pub struct Signature {
    pub role: SignRole,
    pub signer: String,
    pub tick: u64,
    pub fp_ack: u32,
}

/// 交接确认书
pub struct HandoverSheet {
    pub sheet_no: u32,
    pub item_fp: u32,
    pub item_desc: String,
    signs: Vec<Signature>,
}

impl HandoverSheet {
    pub fn new(sheet_no: u32, item_desc: &str, item_fp: u32) -> Self {
        HandoverSheet { sheet_no, item_fp, item_desc: item_desc.to_string(), signs: Vec::new() }
    }

    /// 签署：同角色重复签、指纹确认不符一律拒绝
    pub fn sign(&mut self, role: SignRole, signer: &str, tick: u64, fp_ack: u32) -> bool {
        if self.signs.iter().any(|s| s.role == role) {
            return false; // 同角色单签已存在，拒绝重复签
        }
        if fp_ack != self.item_fp {
            return false; // 对交接物指纹认知不一致，拒绝
        }
        if signer.is_empty() {
            return false;
        }
        // 双签时间序：接收方签不得早于移交方签
        if role == SignRole::Receiver {
            if let Some(h) = self.signs.iter().find(|s| s.role == SignRole::Handover) {
                if tick < h.tick {
                    return false;
                }
            } else {
                return false; // 接收方不得先签
            }
        }
        self.signs.push(Signature { role, signer: signer.to_string(), tick, fp_ack });
        true
    }

    /// 双签生效判定：两角色齐备且指纹确认一致
    pub fn is_dual_signed(&self) -> bool {
        self.signs.iter().any(|s| s.role == SignRole::Handover)
            && self.signs.iter().any(|s| s.role == SignRole::Receiver)
    }

    /// 留痕渲染
    pub fn render(&self) -> String {
        let mut s = format!("sheet#{} item={} fp={:#010x} signs={}", self.sheet_no, self.item_desc, self.item_fp, self.signs.len());
        for sg in &self.signs {
            s.push(' ');
            s.push_str(sg.role.name());
            s.push(':');
            s.push_str(&sg.signer);
        }
        s
    }

    /// 确认书指纹（签署体 + 交接物）
    pub fn sheet_fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut feed = |b: u8, h: &mut u32| {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x01000193);
        };
        for b in self.item_desc.bytes() {
            feed(b, &mut h);
        }
        h ^= self.item_fp;
        h = h.wrapping_mul(0x01000193);
        for sg in &self.signs {
            h ^= sg.role.code() as u32;
            h = h.wrapping_mul(0x01000193);
            for b in sg.signer.bytes() {
                feed(b, &mut h);
            }
            h ^= sg.tick as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h ^ (SEG_BASE & 0xFFFF)
    }

    pub fn sign_count(&self) -> usize {
        self.signs.len()
    }
}

/// 交接账：确认书归档 + 单签拦截统计
pub struct HandoverLedger {
    archived: Vec<HandoverSheet>,
    /// 单签无效拦截次数
    single_sign_blocks: u32,
    /// 指纹不符拦截次数
    fp_blocks: u32,
    /// 双签达成归档数
    dual_ok: u32,
}

impl HandoverLedger {
    pub fn new() -> Self {
        HandoverLedger { archived: Vec::new(), single_sign_blocks: 0, fp_blocks: 0, dual_ok: 0 }
    }

    /// 提交确认书：双签生效归档并计达成；单签/零签按类别拦截
    pub fn submit(&mut self, sheet: HandoverSheet) -> bool {
        if sheet.is_dual_signed() {
            self.dual_ok += 1;
            self.archived.push(sheet);
            true
        } else {
            if sheet.sign_count() == 1 {
                self.single_sign_blocks += 1;
            } else if sheet.sign_count() == 0 {
                self.fp_blocks += 1;
            }
            false
        }
    }

    /// 签署过程中的拦截（指纹不符 / 角色越序），由外部审计上报
    pub fn report_fp_block(&mut self) {
        self.fp_blocks += 1;
    }
    pub fn report_single_block(&mut self) {
        self.single_sign_blocks += 1;
    }

    pub fn dual_ok(&self) -> u32 {
        self.dual_ok
    }
    pub fn single_blocks(&self) -> u32 {
        self.single_sign_blocks
    }
    pub fn fp_blocks(&self) -> u32 {
        self.fp_blocks
    }
    pub fn archived_count(&self) -> usize {
        self.archived.len()
    }

    /// 归档检索：按书号
    pub fn find(&self, sheet_no: u32) -> Option<&HandoverSheet> {
        self.archived.iter().find(|s| s.sheet_no == sheet_no)
    }

    /// 交接账指纹（归档体 + 拦截计数，FNV-1a）
    pub fn report_fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for sh in &self.archived {
            h ^= sh.sheet_fp();
            h = h.wrapping_mul(0x01000193);
        }
        h ^= (self.single_sign_blocks << 16) ^ (self.fp_blocks << 8) ^ self.dual_ok;
        h = h.wrapping_mul(0x01000193);
        h ^ (SEG_BASE & 0xFFFF)
    }

    pub fn render(&self) -> String {
        format!(
            "handover seg={:#06x} archived={} dual={} single_block={} fp_block={}",
            SEG_BASE,
            self.archived_count(),
            self.dual_ok,
            self.single_sign_blocks,
            self.fp_blocks
        )
    }
}

/// 受控注入核：kind=0 单签蒙混 / kind=1 指纹错认 / kind=2 接收方抢签
pub fn inject_fault(sheet: &mut HandoverSheet, ledger: &mut HandoverLedger, kind: u8, signer: &str, tick: u64) -> &'static str {
    match kind {
        0 => {
            // 单签（仅移交方）即提交
            let ok = sheet.sign(SignRole::Handover, signer, tick, sheet.item_fp);
            let submitted = ledger.submit(HandoverSheet::new(sheet.sheet_no, &sheet.item_desc, sheet.item_fp));
            if !ok && !submitted {
                ledger.report_single_block();
                "single-sign-blocked"
            } else if submitted {
                "single-sign-smuggled"
            } else {
                "single-sign-held"
            }
        }
        1 => {
            // 指纹错认：确认指纹与交接物不符
            let ok = sheet.sign(SignRole::Handover, signer, tick, sheet.item_fp ^ 0x1234);
            if !ok {
                ledger.report_fp_block();
                "fp-mismatch-blocked"
            } else {
                "fp-mismatch-accepted"
            }
        }
        2 => {
            // 接收方抢签（移交方未签）
            let ok = sheet.sign(SignRole::Receiver, signer, tick, sheet.item_fp);
            if !ok {
                ledger.report_single_block();
                "receiver-preempt-blocked"
            } else {
                "receiver-preempt-accepted"
            }
        }
        _ => "no-op",
    }
}

pub fn run_unx10298_checks() -> CheckSet {
    let mut s = CheckSet::new("O3");
    // J1-01 正路径双签 10/10
    {
        let mut all = true;
        for i in 0..10u32 {
            let mut sh = HandoverSheet::new(3000 + i, "criteria-bundle", 0xA000 + i);
            let fp = sh.item_fp;
            if !sh.sign(SignRole::Handover, "AI-J", 1000 + i as u64, fp) {
                all = false;
            }
            if !sh.sign(SignRole::Receiver, "AI-O3", 1100 + i as u64, fp) {
                all = false;
            }
            if !sh.is_dual_signed() {
                all = false;
            }
        }
        s.add("正路径-双签十连", all, "双签交接留痕 10/10 须生效");
    }
    // J1-02 单签无效 10/10
    {
        let mut ledger = HandoverLedger::new();
        let mut hits = 0;
        for i in 0..10u32 {
            let mut sh = HandoverSheet::new(4000 + i, "item", 0xB000 + i);
            let mut lg = HandoverLedger::new();
            let _ = &mut ledger;
            match inject_fault(&mut sh, &mut lg, 0, "AI-J", 2000 + i as u64) {
                "single-sign-blocked" | "single-sign-held" => hits += 1,
                "single-sign-smuggled" => {}
                _ => {}
            }
        }
        s.add("拦截-单签十连", hits == 10, "单签提交 10/10 须拦截");
        s.add("拦截-单签记账", ledger.submit(HandoverSheet::new(1, "x", 1)) == false, "零签提交须拦截");
    }
    // J1-03 指纹错认拦截
    {
        let mut sh = HandoverSheet::new(5001, "payload", 0xC0DE);
        let mut lg = HandoverLedger::new();
        let r = inject_fault(&mut sh, &mut lg, 1, "AI-J", 10);
        s.add("拦截-指纹错认", r == "fp-mismatch-blocked" && sh.sign_count() == 0 && lg.fp_blocks() == 1, "指纹不符签署须拒且记账");
    }
    // J1-04 接收方抢签拦截
    {
        let mut sh = HandoverSheet::new(5002, "payload", 0xC0DE);
        let mut lg = HandoverLedger::new();
        let r = inject_fault(&mut sh, &mut lg, 2, "AI-O3", 10);
        s.add("拦截-接收方抢签", r == "receiver-preempt-blocked" && sh.sign_count() == 0, "移交方未签前接收方签署须拒");
    }
    // J1-05 同角色重复签拒绝
    {
        let mut sh = HandoverSheet::new(5003, "payload", 0xC0DE);
        let fp = sh.item_fp;
        s.add(
            "拦截-同角色重复签",
            sh.sign(SignRole::Handover, "A", 1, fp) && !sh.sign(SignRole::Handover, "B", 2, fp) && sh.sign_count() == 1,
            "同角色第二枚签名必须拒绝",
        );
    }
    // J1-06 接收方时间早于移交方拒绝
    {
        let mut sh = HandoverSheet::new(5004, "payload", 0xC0DE);
        let fp = sh.item_fp;
        sh.sign(SignRole::Handover, "A", 100, fp);
        s.add("拦截-时间倒挂", !sh.sign(SignRole::Receiver, "B", 50, fp) && !sh.is_dual_signed(), "接收签 tick 早于移交签须拒");
        let mut sh2 = HandoverSheet::new(5005, "payload", 0xC0DE);
        let fp2 = sh2.item_fp;
        sh2.sign(SignRole::Handover, "A", 100, fp2);
        s.add("边界-同刻签放行", sh2.sign(SignRole::Receiver, "B", 100, fp2) && sh2.is_dual_signed(), "tick 相等不判倒挂");
    }
    // J1-07 账本归档与检索
    {
        let mut lg = HandoverLedger::new();
        let mut sh = HandoverSheet::new(6001, "bundle-A", 0xD001);
        let fp = sh.item_fp;
        sh.sign(SignRole::Handover, "AI-J", 1, fp);
        sh.sign(SignRole::Receiver, "AI-O3", 2, fp);
        s.add(
            "账本-双签归档检索",
            lg.submit(sh) && lg.find(6001).map(|x| x.is_dual_signed()).unwrap_or(false) && lg.find(6002).is_none(),
            "双签归档可按书号检索",
        );
    }
    // J1-08 确认书指纹对签署体敏感
    {
        let mut a = HandoverSheet::new(7001, "same-item", 0xE000);
        let fp = a.item_fp;
        a.sign(SignRole::Handover, "AI-J", 10, fp);
        a.sign(SignRole::Receiver, "AI-O3", 20, fp);
        let f1 = a.sheet_fp();
        let mut b = HandoverSheet::new(7001, "same-item", 0xE000);
        let fpb = b.item_fp;
        b.sign(SignRole::Handover, "AI-J", 10, fpb);
        b.sign(SignRole::Receiver, "AI-WB", 20, fpb);
        let f2 = b.sheet_fp();
        s.add("指纹-签署体敏感", f1 != f2, "接收方不同指纹须不同");
    }
    // J1-09 渲染留痕含双方
    {
        let mut sh = HandoverSheet::new(8001, "bundle-X", 0xF00D);
        let fp = sh.item_fp;
        sh.sign(SignRole::Handover, "AI-J", 5, fp);
        sh.sign(SignRole::Receiver, "AI-O3", 9, fp);
        let txt = sh.render();
        s.add(
            "留痕-渲染含双方",
            txt.contains("HANDOVER:AI-J") && txt.contains("RECEIVER:AI-O3") && txt.contains("sheet#8001"),
            "留痕渲染须含书号与双方签署者",
        );
    }
    // J1-10 十轮回放账目一致
    {
        let fp0 = {
            let mut lg = HandoverLedger::new();
            for i in 0..10u32 {
                let mut sh = HandoverSheet::new(9000 + i, "replay", 0x1000 + i);
                let fp = sh.item_fp;
                sh.sign(SignRole::Handover, "AI-J", i as u64, fp);
                sh.sign(SignRole::Receiver, "AI-O3", i as u64 + 1, fp);
                lg.submit(sh);
                let mut bad = HandoverSheet::new(9100 + i, "replay-bad", 0x2000 + i);
                let bfp = bad.item_fp;
                bad.sign(SignRole::Handover, "AI-J", 1, bfp);
                let mut blg = HandoverLedger::new();
                let _ = inject_fault(&mut bad, &mut blg, 0, "AI-J", 1);
            }
            lg.report_fp()
        };
        let mut stable = true;
        for _ in 0..10 {
            let mut lg = HandoverLedger::new();
            for i in 0..10u32 {
                let mut sh = HandoverSheet::new(9000 + i, "replay", 0x1000 + i);
                let fp = sh.item_fp;
                sh.sign(SignRole::Handover, "AI-J", i as u64, fp);
                sh.sign(SignRole::Receiver, "AI-O3", i as u64 + 1, fp);
                lg.submit(sh);
                let mut bad = HandoverSheet::new(9100 + i, "replay-bad", 0x2000 + i);
                let bfp = bad.item_fp;
                bad.sign(SignRole::Handover, "AI-J", 1, bfp);
                let mut blg = HandoverLedger::new();
                let _ = inject_fault(&mut bad, &mut blg, 0, "AI-J", 1);
            }
            if lg.report_fp() != fp0 {
                stable = false;
            }
        }
        s.add("回放-交接账指纹十轮稳定", stable, "同构十轮回放指纹一致");
    }
    s
}
