import { useEffect, useRef, useState } from "react";
import { ExternalLink, Folder, Image, Pencil, Trash2, X } from "lucide-react";
import { openContextMenu } from "../../components/ContextMenu";
import { askConfirm } from "../../components/Modal";
import { useI18n } from "../../i18n";
import { SHELF_COLORS, type DesktopLayout, type ShelfColor } from "./layout";

/**
 * 批次B（规格 4.7）：文件架飞出面板。
 * - 双击桌面文件架 → 在其旁弹出成员面板（支持嵌套：双击子文件架切换到子架）
 * - 成员：单击选中、双击打开；右键 打开 / 移出到桌面 / 移除登记（第三方）
 * - 底栏：自定义颜色（6 色）/ 链接文件夹（可选，用于"在文件管理器中打开"定位）/ 重命名 / 删除
 * - 数量徽标在桌面图标上（titlebar 显示成员数）
 */

export interface FlyItem {
  id: string;
  label: string;
  icon: React.ElementType; // Lucide 或风格化 SVG 字形
  img?: string | null;
  hue: number;
  kind: "sys" | "app" | "third" | "shelf";
}

export function ShelfFlyout(props: {
  shelfId: string;
  layout: DesktopLayout;
  resolve: (id: string) => FlyItem | null;
  onOpenItem: (id: string) => void;
  onMoveOut: (id: string) => void;
  onRemoveThird: (id: string) => void;
  onOpenExplorer: (path: string) => void;
  onRename: (id: string) => void;
  onChangeIcon: (id: string) => void;
  onDelete: (id: string) => void;
  onSetColor: (id: string, color: ShelfColor) => void;
  onLinkFolder: (id: string) => void;
  onClearLink: (id: string) => void;
  onClose: () => void;
  anchorPx: { left: number; top: number };
}): React.ReactElement {
  const { t } = useI18n();
  const shelf = props.layout.shelves[props.shelfId];
  const ref = useRef<HTMLDivElement>(null);
  const [clamped, setClamped] = useState<{ left: number; top: number }>(props.anchorPx);

  // 视口钳制：优先右展开，右溢出→左展开；上下钳制
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const M = 10;
    let left = props.anchorPx.left;
    if (left + r.width > window.innerWidth - M) left = Math.max(M, props.anchorPx.left - r.width - 8);
    const top = Math.min(Math.max(M, props.anchorPx.top), Math.max(M, window.innerHeight - M - r.height));
    setClamped({ left, top });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.anchorPx.left, props.anchorPx.top, props.shelfId, props.layout]);

  if (!shelf) return <></>;
  const members = shelf.members
    .map((id) => props.resolve(id))
    .filter((x): x is FlyItem => x !== null);

  const memberMenu = (e: React.MouseEvent, m: FlyItem): void => {
    e.preventDefault();
    e.stopPropagation();
    const items = [
      { label: t("desktopOpen"), onClick: () => props.onOpenItem(m.id) },
      { label: t("shelfMoveOut"), onClick: () => props.onMoveOut(m.id) },
      ...("third" === m.kind
        ? [
            {
              label: t("tpRemove"),
              danger: true,
              onClick: () => props.onRemoveThird(m.id),
            },
          ]
        : []),
    ];
    openContextMenu(e.clientX, e.clientY, items);
  };

  return (
    <div
      ref={ref}
      className={`shelf-flyout sc-${shelf.color}`}
      style={{ left: clamped.left, top: clamped.top }}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.stopPropagation()}
    >
      <header className="shf-head">
        <span className="shf-title" title={shelf.linkedPath ?? undefined}>
          {shelf.name}
        </span>
        <span className="shf-count">{members.length}</span>
        <button type="button" className="shf-x" onClick={props.onClose} aria-label={t("close")}>
          <X size={14} />
        </button>
      </header>

      <div className="shf-grid">
        {members.length === 0 && <div className="shf-empty">{t("shelfEmpty")}</div>}
        {members.map((m) => {
          const Icon = m.icon;
          return (
            <button
              key={m.id}
              type="button"
              className="shf-item"
              title={m.label}
              onClick={() => props.onOpenItem(m.id)}
              onDoubleClick={() => props.onOpenItem(m.id)}
              onContextMenu={(e) => memberMenu(e, m)}
            >
              <span className="shf-tile" style={{ ["--hue" as string]: String(m.hue) }}>
                {m.img ? <img src={m.img} alt="" draggable={false} /> : <Icon size={22} strokeWidth={1.6} />}
              </span>
              <span className="shf-label">{m.label}</span>
            </button>
          );
        })}
      </div>

      <footer className="shf-foot">
        <span className="shf-colors" role="group" aria-label={t("shelfColor")}>
          {SHELF_COLORS.map((c) => (
            <button
              key={c}
              type="button"
              className={`shf-dot${shelf.color === c ? " on" : ""}`}
              data-color={c}
              onClick={() => props.onSetColor(props.shelfId, c)}
              aria-label={c}
            />
          ))}
        </span>
        <span className="shf-actions">
          {shelf.linkedPath && (
            <button
              type="button"
              className="shf-btn"
              title={shelf.linkedPath}
              onClick={() => props.onOpenExplorer(shelf.linkedPath!)}
            >
              <ExternalLink size={13} /> {t("shelfOpenInExplorer")}
            </button>
          )}
          <button
            type="button"
            className="shf-btn"
            onClick={() => (shelf.linkedPath ? props.onClearLink(props.shelfId) : props.onLinkFolder(props.shelfId))}
          >
            <Folder size={13} /> {shelf.linkedPath ? t("shelfClearLink") : t("shelfLinkFolder")}
          </button>
          <button type="button" className="shf-btn" onClick={() => props.onChangeIcon(props.shelfId)}>
            <Image size={13} /> {t("changeIcon")}
          </button>
          <button type="button" className="shf-btn" onClick={() => props.onRename(props.shelfId)}>
            <Pencil size={13} /> {t("rename")}
          </button>
          <button
            type="button"
            className="shf-btn danger"
            onClick={() =>
              void askConfirm({
                title: t("shelfDelete"),
                body: t("shelfDeleteBody", { name: shelf.name }),
                danger: true,
                okLabel: t("shelfDelete"),
              }).then((ok) => {
                if (ok) props.onDelete(props.shelfId);
              })
            }
          >
            <Trash2 size={13} /> {t("shelfDelete")}
          </button>
        </span>
      </footer>
    </div>
  );
}
