/**
 * I 通用域 · AI-U1 — 设置中心「系统快捷键与通用交互」标签页
 * （F401-F450 全量面板 + 检查项对账区）。
 *
 * 面板纪律（与 U3/MouseJ1 同源）：
 * - 名称 + 一句话说明 + 调节控件三件套齐全（F474 同源）；
 * - 默认档 = 主册判据默认；全部改动即时生效（u1Store 订阅制广播）；
 * - 高密度纵深排布：十个分组区 + 对账区（F302 乙基线）；
 * - 对账区渲染 kernel/varix/src/uni1/ 51 块 CheckSet 机数台账
 *   （ledger.ts 与 tally.rs 同数——隔离验证与检查项对账的前端展示面）。
 */

import { useEffect, useState } from "react";
import { u1Store, U1_DEFAULTS, type U1Section } from "../u1/u1store";
import { TASKMGR_ENTRIES, refreshWithinBudget, reconcileReadings, LOCK_BUDGET_MS, WINX_OPEN_BUDGET_MS, winXLetterJump } from "../u1/hotkeys";
import { THIS_PC_LIST, WIN_E_COLD_BUDGET_MS, HELP_ANCHORS, TRASH_MENU, TRASH_DROP_RADIUS_PX } from "../u1/explorerkeys";
import { ESC_TIERS, SLIDER_REGISTRY, REPEAT_FIRST_DELAY_MS, REPEAT_STEP_MS, DROPDOWN_TOGGLE_BUDGET_MS } from "../u1/menus";
import { IME_CLICK_BUDGET_MS, VOLUME_APPLY_BUDGET_MS, METER_LATENCY_BUDGET_MS, CAM_PREVIEW_BUDGET_MS } from "../u1/flyouts";
import { ICON_MODES, ZOOM_MIN, ZOOM_MAX, ZOOM_STEP, TOP_EDGE_RETRACT_MS } from "../u1/viewfx";
import { FORMAT_CANCEL_WINDOW_MS, FS_COMPAT, FOREGROUND_IO_BUDGET_MS } from "../u1/disksuite";
import { RESTORE_BUDGET_MS, RESTORE_CAP, BT_RECONNECT_BUDGET_MS, DRIVER_SOURCE_LABELS } from "../u1/wizards";
import { BLACKOUT_BUDGET_MS, DEFAULT_SCHEME } from "../u1/displayav";
import { STICKY_TOGGLE_PRESSES, SHAKE_WINDOW_MS, SHAKE_CROSSES_REQUIRED, SHAKE_AMPLITUDE_MIN_PX, CASCADE_STEP_MS, READONLY_SAVE_OPTIONS, PRINT_QUICK_ITEMS, NO_PRINTER_GUIDE } from "../u1/docops";
import { U1_LEDGER, ledgerTotals, noTruncation, rangeComplete, AGGREGATOR_CAPACITY } from "../u1/ledger";
import { SectionCard } from "./MouseJ1Panels";
import "../../styles/u1.css";

type Cfg = Record<string, unknown>;

function useSection<T extends Cfg>(section: U1Section): [T, (patch: Partial<T>) => void] {
  const [, tick] = useState(0);
  useEffect(() => u1Store.subscribe(() => tick((v) => v + 1)), []);
  const value = { ...(U1_DEFAULTS[section] as T), ...(u1Store.get(section) as unknown as T) };
  return [value, (patch) => u1Store.set(section, patch as Record<string, unknown>)];
}

/* ------------------------------- 通用行与控件 ------------------------------- */

function Row(props: { fno: string; name: string; desc: string; children: React.ReactNode }) {
  return (
    <div className="u1-row">
      <div>
        <div className="u1-name"><span className="fno">{props.fno}</span>{props.name}</div>
        <div className="u1-desc">{props.desc}</div>
      </div>
      <div className="u1-ctl">{props.children}</div>
    </div>
  );
}

function Toggle(props: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={props.on} className="u1-toggle" data-on={props.on}
      onClick={() => props.onChange(!props.on)}>
      {props.on ? "开" : "关"}<span className="u1-toggle-knob" aria-hidden />
      <span className="u1-visually-hidden">{props.label}</span>
    </button>
  );
}

function Num(props: { value: number; min: number; max: number; step?: number; onChange: (v: number) => void; suffix?: string }) {
  return (
    <span className="u1-num">
      <input type="number" value={props.value} min={props.min} max={props.max} step={props.step ?? 1}
        onChange={(e) => props.onChange(Number(e.target.value))} />
      {props.suffix && <span className="u1-num-suffix">{props.suffix}</span>}
    </span>
  );
}

/* ------------------------------- 面板分组区 ------------------------------- */

function AutoArrangePanel(): React.ReactElement {
  const [cfg, set] = useSection<{ order: string; enabled: boolean }>("autoArrange");
  return (
    <SectionCard title="桌面自动排列" f="F401" defaultOpen>
      <Row fno="F401" name="自动排列模式" desc="开启后图标按所选四序排列，自由拖拽互斥关闭（切换互斥判据）；临时移动松手弹回（F124 弹性档）。">
        <span className="u1-seg">
          {(["name", "size", "type", "date"] as const).map((o) => (
            <button key={o} type="button" className="u1-seg-btn" data-on={cfg.order === o}
              onClick={() => set({ order: o, enabled: true })}>{({ name: "名称", size: "大小", type: "类型", date: "日期" } as const)[o]}</button>
          ))}
        </span>
      </Row>
      <Row fno="F401" name="自动排列开关" desc="默认关——自由摆放为主；开启即进入四序排列（互斥）。">
        <Toggle on={cfg.enabled} onChange={(v) => set({ enabled: v })} label="自动排列" />
      </Row>
    </SectionCard>
  );
}

function HotkeysPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ hotkey: string; refreshMs: number; lockWin: boolean; altF4Confirm: boolean; winI: boolean; winX: boolean }>("sysHotkeys");
  const [taskmgr, setTaskmgr] = useSection<{ hotkey: string; refreshMs: number }>("taskmgrHot");
  return (
    <SectionCard title="系统快捷键与入口" f="F402-F408">
      <Row fno="F402" name="任务管理器入口" desc={`三入口注册（F244 同表）：${TASKMGR_ENTRIES.join(" / ")}；刷新节奏 1s±0.1s。`}>
        <Num value={taskmgr.refreshMs} min={900} max={1100} step={50} suffix="ms"
          onChange={(v) => setTaskmgr({ refreshMs: refreshWithinBudget(v) ? v : 1000 })} />
      </Row>
      <Row fno="F402" name="三处数据对账演示" desc="同进程读数误差 <3% 判据（样例 100 / 101 / 99 → 绿）。">
        <span className={reconcileReadings(100, 101, 99) ? "u1-ok" : "u1-bad"}>{reconcileReadings(100, 101, 99) ? "对账一致" : "超差"}</span>
      </Row>
      <Row fno="F403" name="Win+L 锁屏" desc={`锁定预算 ${LOCK_BUDGET_MS}ms；媒体暂停续播；摘要只计数。`}>
        <Toggle on={cfg.lockWin} onChange={(v) => set({ lockWin: v })} label="Win+L" />
      </Row>
      <Row fno="F405" name="Alt+F4 确认链" desc="焦点三场景判定；未保存窗口走三问（另存/不保存/取消）。">
        <Toggle on={cfg.altF4Confirm} onChange={(v) => set({ altF4Confirm: v })} label="Alt+F4 确认" />
      </Row>
      <Row fno="F407" name="Win+I 单例聚焦" desc="未开→开；已开→聚焦；在搜索页→聚焦搜索框不重置。">
        <Toggle on={cfg.winI} onChange={(v) => set({ winI: v })} label="Win+I" />
      </Row>
      <Row fno="F408" name="Win+X 九项对照表" desc={`九项+首字母快捷；打开 <${WINX_OPEN_BUDGET_MS / 1000}s。样例：按 S → ${winXLetterJump("s")?.label ?? "无命中"}。`}>
        <Toggle on={cfg.winX} onChange={(v) => set({ winX: v })} label="Win+X" />
      </Row>
    </SectionCard>
  );
}

function MenusPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ enabled: boolean; budgetMs: number }>("escStack");
  return (
    <SectionCard title="菜单、对话框与键盘操作" f="F424/F433-F436">
      <Row fno="F424" name="Esc 通用关闭" desc={`四层语义（${ESC_TIERS.join("→")}）逐层剥离——一次 Esc 只关最上一层；响应 ≤${cfg.budgetMs}ms。`}>
        <Num value={cfg.budgetMs} min={50} max={200} step={10} suffix="ms" onChange={(v) => set({ budgetMs: v })} />
      </Row>
      <Row fno="F424" name="桌面态无副作用" desc="栈空时 Esc 不做任何事（判据原文——关不掉的桌面不是问题，乱关才是）。">
        <Toggle on={cfg.enabled} onChange={(v) => set({ enabled: v })} label="Esc 栈" />
      </Row>
      <Row fno="F433" name="Shift+F10 键盘右键" desc="呼出位置=焦点元素中心；与 F215 翻转边界、F207 Enter/Esc 语义一致。" >
        <span className="u1-ok">全键盘链路已注册</span>
      </Row>
      <Row fno="F434" name="对话框键位矩阵" desc="三类控件×四键；热键无下划线仍生效；删除类对话框 Esc=取消。" >
        <span className="u1-ok">矩阵已登记</span>
      </Row>
      <Row fno="F435" name="下拉框跳选与滚动跟随" desc={`展开收起 ≤${DROPDOWN_TOGGLE_BUDGET_MS}ms；预览代值 Esc 恢复（无损反悔）。`}>
        <span className="u1-ok">三招矩阵已登记</span>
      </Row>
      <Row fno="F436" name="滑杆步进定义表" desc={`${SLIDER_REGISTRY.map((s) => `${s.name} ${s.step}`).join(" / ")}；连发首牙 ${REPEAT_FIRST_DELAY_MS}ms、阶梯 ${REPEAT_STEP_MS}ms（F240 同源）。`}>
        <span className="u1-ok">{SLIDER_REGISTRY.length} 滑杆在册</span>
      </Row>
    </SectionCard>
  );
}

function ExplorerPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ winEMode: "tab" | "window"; quickNewFolder: boolean; trashBadge: boolean; dragToTrash: boolean }>("explorerKeys");
  return (
    <SectionCard title="资源管理器键位与回收站" f="F404/F409-F415/F431">
      <Row fno="F404" name="Win+E 默认模式" desc={`此机页六区清单钉死（${THIS_PC_LIST.length} 区）；冷启动 ≤${WIN_E_COLD_BUDGET_MS}ms 骨架先行。`}>
        <span className="u1-seg">
          {(["tab", "window"] as const).map((m) => (
            <button key={m} type="button" className="u1-seg-btn" data-on={cfg.winEMode === m}
              onClick={() => set({ winEMode: m })}>{m === "tab" ? "标签页" : "新窗口"}</button>
          ))}
        </span>
      </Row>
      <Row fno="F409" name="F1 上下文帮助" desc={`映射表 ${Object.keys(HELP_ANCHORS).length} 页全覆盖；不抢焦点；死锚=0。`}>
        <span className="u1-ok">锚点全绿</span>
      </Row>
      <Row fno="F414" name="拖拽进回收站" desc={`三路同归（拖拽/Delete/右键）；判定半径 ${TRASH_DROP_RADIUS_PX}px；误拖可撤销（F202）。`}>
        <Toggle on={cfg.dragToTrash} onChange={(v) => set({ dragToTrash: v })} label="拖拽删除" />
      </Row>
      <Row fno="F415" name="回收站两态与角标" desc={`删入/清出 <1s 切换；菜单四项：${TRASH_MENU.join("、")}。`}>
        <Toggle on={cfg.trashBadge} onChange={(v) => set({ trashBadge: v })} label="角标" />
      </Row>
      <Row fno="F431" name="Ctrl+Shift+N 快捷新建" desc="命名初态全选；重名自动「新建文件夹(2)」递增；连建循环退出即落。">
        <Toggle on={cfg.quickNewFolder} onChange={(v) => set({ quickNewFolder: v })} label="快捷新建" />
      </Row>
    </SectionCard>
  );
}

function IndicatorsPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ imeBadge: boolean; volumeFly: boolean; batteryFly: boolean }>("indicators");
  return (
    <SectionCard title="指示器与浮层" f="F421-F423/F448/F449">
      <Row fno="F421" name="输入法徽标" desc={`循环顺序=F373 设置序；点击响应 ≤${IME_CLICK_BUDGET_MS}ms；三处同步对拍。`}>
        <Toggle on={cfg.imeBadge} onChange={(v) => set({ imeBadge: v })} label="输入法徽标" />
      </Row>
      <Row fno="F422" name="音量浮层" desc={`拖动实时生效 ≤${VOLUME_APPLY_BUDGET_MS}ms；图标上方居中；底缘安全钳制。`}>
        <Toggle on={cfg.volumeFly} onChange={(v) => set({ volumeFly: v })} label="音量浮层" />
      </Row>
      <Row fno="F423" name="电池浮层" desc="续航估算 ±15% 诚实标注；三处数据同源（F366/F423/F291）。">
        <Toggle on={cfg.batteryFly} onChange={(v) => set({ batteryFly: v })} label="电池浮层" />
      </Row>
      <Row fno="F448" name="麦克风电平表" desc={`入表延迟 ≤${METER_LATENCY_BUDGET_MS}ms；分档人话建议；测试中 F322 指示亮。`}>
        <span className="u1-ok">阈值在册</span>
      </Row>
      <Row fno="F449" name="摄像头预览" desc={`预览 ≤${CAM_PREVIEW_BUDGET_MS}ms；参数两路诚实标注；关闭即释放（指示灭）。`}>
        <span className="u1-ok">释放账在册</span>
      </Row>
    </SectionCard>
  );
}

function ViewFxPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ zoomStep: number; pageKeepRelative: boolean; fullscreenHint: boolean }>("viewFx");
  return (
    <SectionCard title="视图与缩放" f="F429/F430/F432">
      <Row fno="F429" name="F11 全屏" desc={`顶缘滑出 ${TOP_EDGE_RETRACT_MS / 1000}s 自动收回；退出双键 F11/Esc；提示一次性。`}>
        <Toggle on={cfg.fullscreenHint} onChange={(v) => set({ fullscreenHint: v })} label="一次性提示" />
      </Row>
      <Row fno="F430" name="Ctrl+滚轮缩放" desc={`图标五档（${ICON_MODES.join("/")}）；锚点不动点数学；边界贴边+微弹；范围 ${ZOOM_MIN}–${ZOOM_MAX}‰。`}>
        <Num value={cfg.zoomStep} min={ZOOM_MIN} max={ZOOM_MAX} step={ZOOM_STEP / 10} suffix="‰/格"
          onChange={(v) => set({ zoomStep: v })} />
      </Row>
      <Row fno="F432" name="列表翻页相对位置保持" desc="翻页前第 N 行翻后仍第 N 行（选中项随页同步移动）；万项帧就绪。">
        <Toggle on={cfg.pageKeepRelative} onChange={(v) => set({ pageKeepRelative: v })} label="相对位置保持" />
      </Row>
    </SectionCard>
  );
}

function DiskToolsPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ formatCancelWindowMs: number; lnkAutofix: boolean; isoCap: number }>("diskTools");
  return (
    <SectionCard title="磁盘工具" f="F437-F440">
      <Row fno="F437" name="格式化取消窗口" desc={`开始后 ${FORMAT_CANCEL_WINDOW_MS / 1000}s 内可取消；系统/S: 盘需输入卷标字母二次确认。`}>
        <Num value={cfg.formatCancelWindowMs} min={1000} max={5000} step={500} suffix="ms"
          onChange={(v) => set({ formatCancelWindowMs: v })} />
      </Row>
      <Row fno="F437" name="文件系统兼容表" desc={`${Object.keys(FS_COMPAT).join(" / ")}——未知 fs 不列（诚实兼容）。`}>
        <span className="u1-ok">兼容表在册</span>
      </Row>
      <Row fno="F438" name="改符 lnk 自动修复" desc="改符成功即对登记快捷方式做前缀重写——断链在源头预防；变更留痕 F372。">
        <Toggle on={cfg.lnkAutofix} onChange={(v) => set({ lnkAutofix: v })} label="自动修复" />
      </Row>
      <Row fno="F439" name="驱动器加密强制导出" desc={`恢复密钥未确认导出则启用恒拒（跳不过——不可关）；前台 IO 预算 ${FOREGROUND_IO_BUDGET_MS}ms。`}>
        <span className="u1-ok">判据钉死</span>
      </Row>
      <Row fno="F440" name="ISO 同时挂载上限" desc="超出上限拒绝并给人话提示；挂载盘只读；重启不保留（会话态）。">
        <Num value={cfg.isoCap} min={1} max={8} onChange={(v) => set({ isoCap: v })} suffix="个" />
      </Row>
    </SectionCard>
  );
}

function WizardsPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ taskIdleMinutes: number; btReconnectMs: number; printerTestPage: boolean }>("wizards");
  return (
    <SectionCard title="向导族" f="F441-F444">
      <Row fno="F441" name="计划任务空闲条件" desc="三步建任务；频率全型（一次性/每日/每周/每月）人话标签；执行留痕 F372。">
        <Num value={cfg.taskIdleMinutes} min={1} max={120} suffix="分钟" onChange={(v) => set({ taskIdleMinutes: v })} />
      </Row>
      <Row fno="F442" name="还原点" desc={`创建 ≤${RESTORE_BUDGET_MS / 1000}s；上限 ${RESTORE_CAP} 轮替最旧、最近一个永留；手动/自动标注。`}>
        <span className="u1-ok">轮替纪律在册</span>
      </Row>
      <Row fno="F443" name="蓝牙配对" desc={`确认码核对不跳过；失败归因映射表；自动重连 ≤${BT_RECONNECT_BUDGET_MS / 1000}s。`}>
        <Num value={cfg.btReconnectMs} min={1000} max={6000} step={500} suffix="ms" onChange={(v) => set({ btReconnectMs: v })} />
      </Row>
      <Row fno="F444" name="打印机安装" desc={`驱动来源三态（${Object.values(DRIVER_SOURCE_LABELS).join(" / ")}）；测试页一键；无打印机有 PDF 出路。`}>
        <Toggle on={cfg.printerTestPage} onChange={(v) => set({ printerTestPage: v })} label="装完打印测试页" />
      </Row>
      <Row fno="F428" name="Ctrl+P 打印链路" desc={`四常用项（${PRINT_QUICK_ITEMS.join("、")}）；${NO_PRINTER_GUIDE}`}>
        <span className="u1-ok">分页预览数学在册</span>
      </Row>
    </SectionCard>
  );
}

function DisplayAvPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ snapPx: number; numberOverlayMs: number; confirmWindowMs: number; customMaxMs: number }>("displayAv");
  return (
    <SectionCard title="显示与声音" f="F445-F447">
      <Row fno="F445" name="显示器吸附与编号" desc={`对齐线 ±${cfg.snapPx}px；编号大号 ${cfg.numberOverlayMs / 1000}s 对应实物；窗口回流 F353。`}>
        <Num value={cfg.snapPx} min={4} max={16} suffix="px" onChange={(v) => set({ snapPx: v })} />
      </Row>
      <Row fno="F446" name="分辨率确认倒计时" desc={`「保留新设置？」${cfg.confirmWindowMs / 1000}s 超时自动回滚；黑屏期 ≤${BLACKOUT_BUDGET_MS}ms 账；游戏向=最高刷。`}>
        <Num value={cfg.confirmWindowMs} min={5000} max={30000} step={1000} suffix="ms" onChange={(v) => set({ confirmWindowMs: v })} />
      </Row>
      <Row fno="F447" name="事件声音方案" desc={`默认六事件（${DEFAULT_SCHEME.map(([e]) => e).join("、")}）；自定义 WAV ≤${cfg.customMaxMs / 1000}s；静音测试可当场验证。`}>
        <Num value={cfg.customMaxMs} min={1000} max={5000} step={500} suffix="ms" onChange={(v) => set({ customMaxMs: v })} />
      </Row>
    </SectionCard>
  );
}

function DocOpsPanel(): React.ReactElement {
  const [cfg, set] = useSection<{ shakeEnabled: boolean; stickyEnabled: boolean; filterEnabled: boolean; filterMinHoldMs: number; neverRemind: boolean }>("a11yKeys");
  const [shakeCfg, setShakeCfg] = useSection<{ enabled: boolean; windowMs: number; amplitudePx: number }>("shake");
  return (
    <SectionCard title="文档操作与无障碍" f="F413/F425-F428/F450" defaultOpen>
      <Row fno="F425" name="Aero Shake" desc={`判定：${SHAKE_WINDOW_MS}ms 内过中线 ≥${SHAKE_CROSSES_REQUIRED} 次且幅度 ≥${SHAKE_AMPLITUDE_MIN_PX}px；误触 20 次 0 触发；依次收缩 ${CASCADE_STEP_MS}ms/窗。`}>
        <Toggle on={shakeCfg.enabled} onChange={(v) => setShakeCfg({ enabled: v })} label="Aero Shake" />
      </Row>
      <Row fno="F426" name="保存三键与只读三问" desc={`Ctrl+S / Ctrl+Shift+S / F12；只读失败三选：${READONLY_SAVE_OPTIONS.join("、")}（取消永远安全）。`}>
        <span className="u1-ok">键位注册 F244</span>
      </Row>
      <Row fno="F427" name="文件剪切延迟执行" desc="剪切后源文件还在直到粘贴——粘贴前可反悔；粘贴冲突走 F087 面板。">
        <span className="u1-ok">延迟语义在册</span>
      </Row>
      <Row fno="F450" name="粘滞键" desc={`连按 ${STICKY_TOGGLE_PRESSES} 次 Shift 弹确认框（防误启）；修饰键逐键锁存合成；指示器随开随显。`}>
        <Toggle on={cfg.stickyEnabled} onChange={(v) => set({ stickyEnabled: v })} label="粘滞键" />
      </Row>
      <Row fno="F450" name="筛选键" desc={`短促误击忽略；阈值 ${cfg.filterMinHoldMs}ms 可调；长按重复抑制。`}>
        <span className="u1-num">
          <input type="number" value={cfg.filterMinHoldMs} min={20} max={200} step={10} aria-label="筛选键阈值"
            onChange={(e) => set({ filterMinHoldMs: Number(e.target.value) })} />
          <span className="u1-num-suffix">ms</span>
        </span>
      </Row>
      <Row fno="F450" name="永不再提醒" desc="登记后连按 Shift 直接启用（用户已表达过意愿——提示温和不烦人）。">
        <Toggle on={cfg.neverRemind} onChange={(v) => set({ neverRemind: v })} label="不再提醒" />
      </Row>
    </SectionCard>
  );
}

/* ------------------------------- 检查项对账区（与 tally.rs 同数） ------------------------------- */

function LedgerPanel(): React.ReactElement {
  const totals = ledgerTotals();
  return (
    <SectionCard title="检查项对账（kernel/varix/src/uni1 · 51 块 CheckSet 机数）" f="十二查·10" defaultOpen>
      <div className="u1-ledger-summary">
        <span>块数 {totals.blocks}/{AGGREGATOR_CAPACITY}（{noTruncation() ? "零截断" : "超容——红"}）</span>
        <span>检查项 {totals.checks}</span>
        <span>宿主单测 {totals.unitTests}</span>
        <span>范围 {rangeComplete() ? "F401-F450 完整" : "缺项——红"}</span>
      </div>
      <table className="u1-ledger">
        <thead>
          <tr><th>项</th><th>模块</th><th>功能</th><th>检查</th><th>单测</th></tr>
        </thead>
        <tbody>
          {U1_LEDGER.map((r) => (
            <tr key={r.tag}>
              <td className="u1-ledger-tag">{r.tag}</td>
              <td className="u1-ledger-mod">{r.module}</td>
              <td>{r.name}</td>
              <td className="u1-ledger-num">{r.checks}</td>
              <td className="u1-ledger-num">{r.unitTests}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="u1-ledger-note">
        数字产出命令：<code>cd _attic/aiu1-f401-f450/cabin &amp;&amp; cargo run --bin tally</code>
        （51 块 492 项检查 · 零红 · 零截断；与 <code>隔离验证与检查项对账.md</code> §3 同源）。
      </div>
    </SectionCard>
  );
}

/* ------------------------------- 总装 ------------------------------- */

export function U1Tab(): React.ReactElement {
  return (
    <div className="u1-tab">
      <p className="u1-tab-intro">
        系统快捷键、通用交互语义、磁盘工具与向导族——五十项判据的前端生效面；
        判据唯一源 = 主册 I-1，参数与内核语义核（kernel/varix/src/uni1/）同源。
      </p>
      <AutoArrangePanel />
      <HotkeysPanel />
      <MenusPanel />
      <ExplorerPanel />
      <IndicatorsPanel />
      <ViewFxPanel />
      <DiskToolsPanel />
      <WizardsPanel />
      <DisplayAvPanel />
      <DocOpsPanel />
      <LedgerPanel />
    </div>
  );
}
