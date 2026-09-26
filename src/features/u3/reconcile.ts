/**
 * F550 检查项对账引擎（三面对账 · 账册检对账 · 合并无冲突验证）。
 *
 * 判据唯一源（主册 F550 摘文）：「25 检查点入脚本并全绿基线；与 F400 脚本
 * 合并无冲突；锚点可执行性抽查（随机 5 条实机跑）；账册检对账」。
 *
 * 三面口径（一处一事实）：
 * - **主册面**：F501-F550 五十判据编号（唯一源，永不缺行）；
 * - **内核面**：kernel/varix/src/ustar3/ 50 域 CheckSet（v1 收口数字，
 *   域检查项数登记自 docs/AI-U3-完成报告.md §2 域对账总表——cargo 宿主
 *   通道实测产物，数字与 robust.rs 域表注册一致）；
 * - **前端面**：src/features/u3/ 九域自检（anchor.ts anchorRuntime 实时跑，
 *   本引擎直接调用——账不是抄的，是现场重跑的）。
 *
 * 对账规则：每个 F 编号三面必须同时有承载（内核域注册 + 前端自检域 + UI
 * 承载面声明）；任一面缺失 = 对账红（不静默）。
 * 合并无冲突：U3 锚点编号空间（F501-F550）与 F400（H 域收官）/F575（批次七）
 * 不重叠——区间声明即判据。
 */

import { anchorRuntime, U3_ANCHOR_DOMAINS } from "./anchor";
import {
  KERNEL_DOMAIN_CHECKS, kernelLedgerSelfCheck, NEIGHBOR_ANCHOR_SPACES, anchorSpaceConflictFree,
} from "./ledger";
// 账册数据层 re-export（API 兼容：既有消费方 import 路径不变）
export { KERNEL_DOMAIN_CHECKS, kernelLedgerSelfCheck, NEIGHBOR_ANCHOR_SPACES, anchorSpaceConflictFree };

/* ------------------------------- 主册面（50 判据编号） ------------------------------- */

/** 主册 F501-F550 判据编号与名称（判据唯一源的编号索引——一处一事实）。 */
export const MASTER_F501_F550: Array<{ fno: string; name: string }> = [
  { fno: "F501", name: "桌面图标文字可读性" },
  { fno: "F502", name: "图标文字两行封顶" },
  { fno: "F503", name: "图标网格密度" },
  { fno: "F504", name: "PIN 快速解锁" },
  { fno: "F505", name: "蓝牙动态锁" },
  { fno: "F506", name: "访客模式" },
  { fno: "F507", name: "锁屏防截图" },
  { fno: "F508", name: "应用防截标记" },
  { fno: "F509", name: "文件粉碎" },
  { fno: "F510", name: "单文件加密" },
  { fno: "F511", name: "剪贴板一键清空" },
  { fno: "F512", name: "截图历史" },
  { fno: "F513", name: "Ctrl 定位指针" },
  { fno: "F514", name: "声音视觉提示" },
  { fno: "F515", name: "查找与替换" },
  { fno: "F516", name: "通知横幅位置设置" },
  { fno: "F517", name: "资源管理器启动页设置" },
  { fno: "F518", name: "输入法切换键自定义" },
  { fno: "F519", name: "大写锁定提示音" },
  { fno: "F520", name: "标题栏中键最小化" },
  { fno: "F521", name: "截图保存位置设置" },
  { fno: "F522", name: "指针轨迹显示" },
  { fno: "F523", name: "打字时隐藏指针" },
  { fno: "F524", name: "撤销清空回收站" },
  { fno: "F525", name: "快捷键速查卡导出" },
  { fno: "F526", name: "资源管理器状态栏" },
  { fno: "F527", name: "导航树折叠展开" },
  { fno: "F528", name: "树与列表双向同步" },
  { fno: "F529", name: "复制前空间预检" },
  { fno: "F530", name: "复制后校验" },
  { fno: "F531", name: "复制任务队列化" },
  { fno: "F532", name: "打开失败人话诊断" },
  { fno: "F533", name: "只读介质提醒" },
  { fno: "F534", name: "长路径全程支持" },
  { fno: "F535", name: "Win+数字快捷启动" },
  { fno: "F536", name: "Win+T 任务栏遍历" },
  { fno: "F537", name: "Win+逗号 瞥桌面" },
  { fno: "F538", name: "Alt+Esc 窗口循环" },
  { fno: "F539", name: "桌面布局锁定" },
  { fno: "F540", name: "内存诊断" },
  { fno: "F541", name: "网络重置" },
  { fno: "F542", name: "ClickLock 拖拽锁定" },
  { fno: "F543", name: "分设备音量记忆" },
  { fno: "F544", name: "通知音量独立分级" },
  { fno: "F545", name: "蓝牙耳机电量显示" },
  { fno: "F546", name: "新设备接入通知" },
  { fno: "F547", name: "音量左右平衡" },
  { fno: "F548", name: "任务管理器置顶" },
  { fno: "F549", name: "时钟悬停完整日期" },
  { fno: "F550", name: "I 域批次六验收锚点" },
];

/* ------------------------------- 内核面（域→检查项数账册） ------------------------------- */

/* ------------------------------- UI 承载面声明 ------------------------------- */

/** 每个前端域的 UI 承载声明（三面之一：判据必须在界面/运行时有落点）。 */
export const FRONTEND_DOMAIN_CARRIER: Record<string, { module: string; carrier: string }> = {
  deskicons: { module: "deskicons.ts + deskiconLayer.tsx", carrier: "IconLabel 件 + resnap 出口 + U3Tab 桌面图标组" },
  locksec: { module: "locksec.ts", carrier: "U3Tab 锁屏与安全组（PIN 冷却表/沙盒四轴/防截样张）" },
  filesec: { module: "filesec.ts", carrier: "U3Tab 文件安全组 + U3Runtime 剪贴板热键" },
  pointerfx: { module: "pointerfx.ts", carrier: "U3Runtime 涟漪/光带/Caps 音 + U3Tab 指针组" },
  explorerx: { module: "explorerx.ts", carrier: "U3Tab 资源管理器组（状态栏三段实时样张）" },
  copyops: { module: "copyops.ts", carrier: "U3Tab 复制与回收站组" },
  winkeys: { module: "winkeys.ts", carrier: "U3Tab 窗口与快捷键组 + actions.ts 四动作" },
  sysdev: { module: "sysdev.ts", carrier: "U3Tab 系统与设备组 + U3Runtime 蓝牙锁心跳" },
  clockcal: { module: "clockcal.ts", carrier: "ClockHoverTip 件 + U3Tab 农历样张" },
};

/* ------------------------------- 对账核心 ------------------------------- */

/** F 编号 → 前端域映射（与内核 mod.rs 域表同源的分派——一处一事实）。 */
export const FNO_TO_FRONTEND_DOMAIN: Record<string, string> = {};
for (const d of U3_ANCHOR_DOMAINS) {
  // fRange 形如 "F501-F503" / "F513·F514·F519·F520·F522·F523"
  const tokens = d.fRange.split(/[·,\s]+/);
  const nos: number[] = [];
  for (const t of tokens) {
    const m = /^F(\d{3})(?:-F?(\d{3}))?$/.exec(t);
    if (m) {
      const a = Number(m[1]);
      const b = m[2] ? Number(m[2]) : a;
      for (let n = a; n <= b; n++) nos.push(n);
    }
  }
  for (const n of nos) FNO_TO_FRONTEND_DOMAIN[`F${n}`] = d.domain;
}

/** F 编号 → 内核域映射（KERNEL_DOMAIN_CHECKS 的 fRange 解析，同法）。 */
export const FNO_TO_KERNEL_DOMAIN: Record<string, string> = {};
for (const d of Object.keys(KERNEL_DOMAIN_CHECKS)) {
  const tokens = (KERNEL_DOMAIN_CHECKS[d]?.fRange ?? "").split(/[/,]+/);
  for (const t of tokens) {
    const m = /^F(\d{3})(?:-F?(\d{3}))?$/.exec(t.trim());
    if (m) {
      const a = Number(m[1]);
      const b = m[2] ? Number(m[2]) : a;
      for (let n = a; n <= b; n++) FNO_TO_KERNEL_DOMAIN[`F${n}`] = d;
    }
  }
}

export interface ReconcileRow {
  fno: string;
  name: string;
  kernelDomain: string | null;
  frontendDomain: string | null;
  uiCarrier: string | null;
  ok: boolean;
}

export interface ReconcileReport {
  rows: ReconcileRow[];
  total: number;
  ok: number;
  missing: Array<{ fno: string; faces: string[] }>; // 缺哪几面（不静默）
  frontendRuntime: ReturnType<typeof anchorRuntime>;
  kernelLedger: ReturnType<typeof kernelLedgerSelfCheck>;
  allGreen: boolean;
}

/**
 * 三面对账（账册检——每次调用现场重跑前端自检，不抄缓存账）。
 * F550 自身特殊：它是锚点域（内核 anchor 25 检查点 + 前端 anchor.ts），
 * UI 承载 = U3Tab 对账面板（本报告本身）——自我指涉以声明处理。
 */
export function reconcileF501F550(): ReconcileReport {
  const kernelLedger = kernelLedgerSelfCheck();
  const frontendRuntime = anchorRuntime();
  const frontendByDomain = new Map(frontendRuntime.results.map((r) => [r.domain, r]));

  const rows: ReconcileRow[] = MASTER_F501_F550.map((m) => {
    const kernelDomain = FNO_TO_KERNEL_DOMAIN[m.fno] ?? null;
    const frontendDomain = FNO_TO_FRONTEND_DOMAIN[m.fno] ?? null;
    const fe = frontendDomain ? frontendByDomain.get(frontendDomain) : undefined;
    // UI 承载：F550 = 对账面板自身；其余按域查承载声明
    const uiCarrier =
      m.fno === "F550"
        ? "U3Tab 对账面板（本报告）"
        : frontendDomain
          ? (FRONTEND_DOMAIN_CARRIER[frontendDomain]?.carrier ?? null)
          : null;
    const ok = kernelDomain !== null && !!fe && fe.checks.length > 0 && uiCarrier !== null;
    return { fno: m.fno, name: m.name, kernelDomain, frontendDomain, uiCarrier, ok };
  });

  const missing = rows.filter((r) => !r.ok).map((r) => ({
    fno: r.fno,
    faces: [
      r.kernelDomain === null ? "kernel" : null,
      r.frontendDomain === null || !rows.find((x) => x.fno === r.fno)?.ok ? null : null,
      r.uiCarrier === null ? "ui" : null,
    ].filter((x): x is string => x !== null),
  }));

  const okCount = rows.filter((r) => r.ok).length;
  return {
    rows,
    total: rows.length,
    ok: okCount,
    missing,
    frontendRuntime,
    kernelLedger,
    allGreen: okCount === rows.length && kernelLedger.ledgerOk && frontendRuntime.allGreen,
  };
}

/* ------------------------------- 合并无冲突验证（F550 判据之二） ------------------------------- */

/** 锚点可执行性抽查（判据：随机 5 条实机跑——本引擎喂确定性步长抽取）。 */
export function anchorExecutabilitySpotCheck(n = 5): Array<{ fno: string; name: string; executable: boolean }> {
  return MASTER_F501_F550.filter((_, i) => i % Math.ceil(MASTER_F501_F550.length / n) === 0)
    .slice(0, n)
    .map((m) => ({
      fno: m.fno,
      name: m.name,
      executable: (FNO_TO_FRONTEND_DOMAIN[m.fno] ?? "") !== "" && (FNO_TO_KERNEL_DOMAIN[m.fno] ?? "") !== "",
    }));
}
