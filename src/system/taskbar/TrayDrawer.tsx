import { useMemo, useState } from "react";
import { ChevronUp, Search } from "lucide-react";
import { useI18n } from "../../i18n";
import { matchPinyin } from "../../lib/pinyin";

/**
 * M-11 托盘收纳抽屉（AI-03 任务栏与托盘组）：
 * 环境内托盘区图标超出阈值（默认 8）后收进二级抽屉：可搜索（拼音/首字母）、
 * 新图标出现时入口徽标 +1、点开后清零。
 * 边界如实声明：只读取并镜像展示环境内托盘图标，不修改系统托盘、不做系统托盘注入。
 * 后端暂无系统托盘枚举接口 → 首版只覆盖环境内图标（系统托盘镜像挂待验清单）。
 */

export interface TrayItem {
  id: string;
  label: string;
  node: React.ReactNode;
  onClick: () => void;
}

export const TRAY_OVERFLOW_THRESHOLD = 8;

/** 抽屉内搜索过滤（拼音/首字母/原文三路匹配）。 */
export function filterTrayItems<T extends { label: string }>(items: T[], q: string): T[] {
  const query = q.trim();
  if (!query) return items;
  return items.filter((it) => matchPinyin(it.label, query) || it.label.toLowerCase().includes(query.toLowerCase()));
}

export function TrayDrawer(props: {
  items: TrayItem[];
  newCount: number;
  onOpen: () => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const overflow = props.items.length - TRAY_OVERFLOW_THRESHOLD;
  const hidden = useMemo(() => props.items.slice(Math.max(0, TRAY_OVERFLOW_THRESHOLD)), [props.items]);
  const matched = filterTrayItems(hidden, q);

  if (overflow <= 0 && props.newCount <= 0) return null;

  return (
    <>

      {(overflow > 0 || props.newCount > 0) && (
        <button
          type="button"
          className={`tb-btn tray-btn${open ? " active" : ""}`}
          aria-label={t("tbTrayDrawer")}
          title={t("tbTrayDrawer")}
          onClick={() => {
            setOpen(!open);
            if (!open) props.onOpen();
          }}
        >
          <ChevronUp size={16} strokeWidth={1.7} />
          {props.newCount > 0 && <span className="tb-badge" aria-hidden>{props.newCount > 9 ? "9+" : props.newCount}</span>}
        </button>
      )}
      {open && (
        <div className="tray-drawer card-pop" role="dialog" aria-label={t("tbTrayDrawer")}>
          <div className="tray-drawer-search">
            <Search size={13} className="dim" />
            <input
              value={q}
              placeholder={t("tbTraySearch")}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => e.stopPropagation()}
            />
          </div>
          {matched.length === 0 && <p className="dim small tray-drawer-empty">{t("tbTrayNoMatch")}</p>}
          {matched.map((it) => (
            <button
              key={it.id}
              type="button"
              className="tray-drawer-item"
              title={it.label}
              onClick={() => {
                it.onClick();
                setOpen(false);
              }}
            >
              <span className="tray-drawer-icon">{it.node}</span>
              <span className="tray-drawer-name">{it.label}</span>
            </button>
          ))}
        </div>
      )}
    </>
  );
}
