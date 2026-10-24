//! ves471_s25sum — VE-F4237 · 契约与跨域汇总
//! U02 对 U01 模型/注册中心的双向回执（引擎执行状态汇入注册中心）+对 U03 验证平台的契约源输出确认；
//! 衔接点挂指针；回执断→补发+催办；源输出失配→对拍修正；越界→退回；
//! 衔接含无障碍判据状态同步核验（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1940;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    Ok = 0,
    ReceiptLost = SEG_BASE + 0x01, // 0x1941
    OutputMismatch = SEG_BASE + 0x02, // 0x1942
    OutOfScope = SEG_BASE + 0x03,  // 0x1943 越界退回
    A11ySyncGap = SEG_BASE + 0x04, // 0x1944（域本色）
}

/// 衔接点（挂指针）。
#[derive(Clone, Copy)]
pub struct Hook { pub target: &'static str, pub ptr: u32, pub up: bool, pub down: bool } // up=对 U01 回执 / down=对 U03 输出

/// 汇总册。
pub struct SumBook {
    pub hooks: Vec<Hook>,
    pub engine_state_rev: u64,  // 引擎执行状态版本
    pub reg_rev: u64,           // 注册中心版本
    pub out_sig: (u32, u32),    // 契约源输出（引擎签名, 平台确认签名）
    pub a11y_synced: bool,      // a11y 判据状态同步核验（域本色）
    pub urged: u32,
    pub returned: u32,
}

impl SumBook {
    pub fn new() -> Self {
        SumBook { hooks: Vec::new(), engine_state_rev: 0, reg_rev: 0, out_sig: (0, 0), a11y_synced: false, urged: 0, returned: 0 }
    }
    pub fn hook(&mut self, h: Hook) {
        self.hooks.retain(|x| x.target != h.target);
        self.hooks.push(h);
    }
    /// 双向回执核验：两向齐→通；断→补发催办。
    pub fn receipts(&mut self) -> Result<(), SmCode> {
        for h in &self.hooks {
            if !h.up || !h.down {
                self.urged += 1;
                return Err(SmCode::ReceiptLost);
            }
        }
        Ok(())
    }
    /// 引擎状态汇入注册中心：版本对齐核验。
    pub fn sync_state(&mut self, reg_rev: u64) -> Result<(), SmCode> {
        self.reg_rev = reg_rev;
        if self.engine_state_rev != reg_rev {
            self.engine_state_rev = reg_rev;
            return Err(SmCode::OutputMismatch);
        }
        Ok(())
    }
    /// 契约源输出确认：签名对拍；越界（未知目标）→退回。
    pub fn confirm_output(&mut self, target: &'static str, sig: u32) -> Result<(), SmCode> {
        if !self.hooks.iter().any(|h| h.target == target) {
            self.returned += 1;
            return Err(SmCode::OutOfScope);
        }
        self.out_sig = (self.out_sig.0, sig);
        if self.out_sig.0 != 0 && self.out_sig.0 != sig { return Err(SmCode::OutputMismatch); }
        Ok(())
    }
    pub fn set_engine_sig(&mut self, sig: u32) { self.out_sig.0 = sig; }
    /// 汇总终态：回执齐+状态同步+输出确认+a11y 同步（域本色）。
    pub fn final_ok(&self) -> Result<(), SmCode> {
        for h in &self.hooks {
            if !h.up || !h.down { return Err(SmCode::ReceiptLost); }
        }
        if self.engine_state_rev != self.reg_rev { return Err(SmCode::OutputMismatch); }
        if self.out_sig.0 == 0 || self.out_sig.0 != self.out_sig.1 { return Err(SmCode::OutputMismatch); }
        if !self.a11y_synced { return Err(SmCode::A11ySyncGap); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves471_checks() -> CheckSet {
    let mut set = CheckSet::new("ves471");
    let mut bk = SumBook::new();

    // C471-01 衔接点挂指针（U01 上行 + U03 下行）。
    bk.hook(Hook { target: "u01-registry", ptr: 601, up: true, down: false });
    bk.hook(Hook { target: "u03-platform", ptr: 602, up: false, down: true });
    chk(&mut set, "C471-01", bk.hooks.len() == 2, "双衔接挂载");

    // C471-02 回执断（缺下行）→催办。
    chk(&mut set, "C471-02", bk.receipts() == Err(SmCode::ReceiptLost) && bk.urged == 1, "回执断催办");

    // C471-03 补齐双向→通。
    bk.hooks[0].down = true;
    bk.hooks[1].up = true;
    chk(&mut set, "C471-03", bk.receipts().is_ok(), "双向回执齐");

    // C471-04 引擎状态汇入：版本失配→对版修正。
    bk.engine_state_rev = 5;
    chk(&mut set, "C471-04", bk.sync_state(6) == Err(SmCode::OutputMismatch) && bk.engine_state_rev == 6, "状态对版");

    // C471-05 再同步过。
    chk(&mut set, "C471-05", bk.sync_state(6).is_ok(), "状态同步");

    // C471-06 契约源输出确认（引擎签名设置+平台确认对拍）。
    bk.set_engine_sig(7001);
    chk(&mut set, "C471-06", bk.confirm_output("u03-platform", 7001).is_ok(), "输出确认");

    // C471-07 越界目标→退回。
    chk(&mut set, "C471-07", bk.confirm_output("u09-unknown", 7001) == Err(SmCode::OutOfScope)
        && bk.returned == 1, "越界退回");

    // C471-08 输出签名失配→失配码。
    chk(&mut set, "C471-08", bk.confirm_output("u03-platform", 7002) == Err(SmCode::OutputMismatch), "签名失配");

    // C471-09 a11y 状态同步缺→终态拒（域本色）。
    chk(&mut set, "C471-09", bk.final_ok() == Err(SmCode::A11ySyncGap), "a11y 同步缺拒");

    // C471-10 a11y 同步补齐→终态过。
    bk.a11y_synced = true;
    chk(&mut set, "C471-10", bk.final_ok().is_ok(), "汇总终态");

    // C471-11 回执断再催办幂等。
    bk.hooks[1].down = false;
    let _ = bk.receipts();
    bk.hooks[1].down = true;
    chk(&mut set, "C471-11", bk.urged == 2, "催办幂等");

    // C471-12 错误码段位单源。
    chk(&mut set, "C471-12", SmCode::A11ySyncGap as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves471_smoke() {
        let set = run_ves471_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves471 red: {:?}", red);
    }
}
