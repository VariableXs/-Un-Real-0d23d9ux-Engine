import { useState } from "react";

/**
 * 任务 64（AI-S UI 侧）：诚实声明卡 —— 威胁对策 + 残余风险双栏。
 * 「如实文化」落点：对策说清防什么，残余风险直说防不了什么；
 * 「内核 VMX 未实现、现用 Hyper-V 底座」在此与文档双落点公示（施工总案 6.12）。
 */

export interface HonestyRow {
  item: string;
  /** 现有对策（防什么、怎么防）。 */
  countermeasure: string;
  /** 残余风险（防不了什么、影响面、缓解手段），空串 = 无已知残余风险。 */
  residual: string;
}

export const HONESTY_ROWS: readonly HonestyRow[] = [
  {
    item: "Wine 进程越权（网络/宿主盘）",
    countermeasure: "每进程 Job 边界 + 默认拒绝：无网络、无宿主盘；越权能力必须申请式授权（任务 61）",
    residual: "内核侧 Job 边界之外的内核漏洞不可由本层防御；缓解=PE 拒绝表先挡恶意样本（任务 62）",
  },
  {
    item: "恶意 PE 混入 SHARED 分区",
    countermeasure: "PE 拒绝表规则驱动（≥20 恶意特征全拒 + 50 正常软件零误拦，任务 62），拒绝原因用户可读",
    residual: "规则表是特征法，零日样本不保证拦截；缓解=白名单默认拒绝 + 审计留痕",
  },
  {
    item: "U 盘拔出导致数据损坏",
    countermeasure: "fs23 journal 掉电语义 + 快照滚动（任务 67）+ 拔出全链优雅中断（任务 58）",
    residual: "快照间隔内的写入在强拔后仍可能丢失（journal 保证不损坏，但不保证不丢）——「什么救不回来」清单如实列出",
  },
  {
    item: "引擎虚拟机逃逸",
    countermeasure: "Hyper-V 底座硬件虚拟化隔离；引擎无宿主盘直通，经 SHARED 白名单交换",
    residual: "Hyper-V 自身漏洞不在本项目防御范围；VM 底座可替换（开放性预留内核 VMX）",
  },
  {
    item: "内核 VMX 未实现",
    countermeasure: "现用 Hyper-V 底座承载隐形 Windows 引擎（施工总案 6.2 过渡方案）",
    residual: "依赖宿主 Hyper-V 能力（Win11 专业版以上）；家庭版用户引擎通道不可用，Wine 通道不受影响",
  },
  {
    item: "配置漂移（三处配置不一致）",
    countermeasure: "三处配置一致性校验器（任务 68，md5 对齐）+ verify 门禁",
    residual: "校验器只对齐 md5，不校验语义正确性；语义变更需走文档同步（任务 89）",
  },
  {
    item: "缓存残留（拔盘无痕）",
    countermeasure: "ramcache 只缓不落盘、关机即清（任务 52）；关机后 U 盘字节级零残留校验",
    residual: "宿主内存中的缓存内容随断电消失；休眠快照写差分盘属于引擎数据，非 ramcache 范围",
  },
];

export function HonestyDeclareCard(): React.ReactElement {
  const [openResidual, setOpenResidual] = useState(true);
  return (
    <div className="honesty-declare" role="note" aria-label="诚实声明：威胁对策与残余风险">
      <div className="honesty-cols" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
        <div>
          <h4 style={{ margin: "0 0 6px" }}>对策（防什么）</h4>
          <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 8 }}>
            {HONESTY_ROWS.map((r) => (
              <li key={r.item}>
                <strong>{r.item}</strong>
                <div className="dim small">{r.countermeasure}</div>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h4 style={{ margin: "0 0 6px" }}>
            残余风险（防不了什么）
            <button
              type="button"
              className="btn"
              style={{ marginLeft: 8, fontSize: 11, padding: "2px 8px" }}
              onClick={() => setOpenResidual((v) => !v)}
            >
              {openResidual ? "收起" : "展开"}
            </button>
          </h4>
          {openResidual && (
            <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 8 }}>
              {HONESTY_ROWS.map((r) => (
                <li key={r.item}>
                  <strong>{r.item}</strong>
                  <div className="dim small">{r.residual || "无已知残余风险"}</div>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <p className="dim small" style={{ marginTop: 10 }}>
        本声明与 docs/威胁清单终审.md 同源维护；页面展示与文档不一致时以文档为准并视为缺陷。
      </p>
    </div>
  );
}
