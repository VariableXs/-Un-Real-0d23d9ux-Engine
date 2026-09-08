import { formatDateTime, useI18n } from "../../i18n";
import { Modal } from "../../components/Modal";
import { startLabels } from "./labels";

/**
 * 化境 V-14（车道 S）：开始菜单右键「属性」简易面板。
 * 纯只读展示：名称 / 类型 / 路径 / 加入时间 / 本机使用次数。
 * 诚实边界：全部信息读本机（第三方登记表 + usage.ts 本地计数），零网络；
 * 官方软件无独立路径/安装时间，如实显示「—」。
 */

export interface StartPropsInfo {
  gridId: string;
  name: string;
  /** labels.ts 的 kind* key。 */
  kindKey: string;
  path: string | null;
  addedAt: number | null;
  usage: number;
}

export function StartPropsPanel(props: { info: StartPropsInfo | null; onClose: () => void }): React.ReactElement | null {
  const { lang } = useI18n();
  const L = startLabels(lang);
  const info = props.info;
  if (!info) return null;
  return (
    <Modal open title={L.propsTitle} onClose={props.onClose} width={460}>
      <div className="start-props">
        <div className="start-props-row">
          <span className="dim">{L.propName}</span>
          <span>{info.name}</span>
        </div>
        <div className="start-props-row">
          <span className="dim">{L.propKind}</span>
          <span>{L[info.kindKey] ?? info.kindKey}</span>
        </div>
        <div className="start-props-row">
          <span className="dim">{L.propPath}</span>
          <span className="start-props-path" title={info.path ?? undefined}>
            {info.path ?? L.propNoPath}
          </span>
        </div>
        <div className="start-props-row">
          <span className="dim">{L.propAdded}</span>
          <span>{info.addedAt ? formatDateTime(info.addedAt, lang) : "—"}</span>
        </div>
        <div className="start-props-row">
          <span className="dim">{L.propUsage}</span>
          <span>{info.usage}</span>
        </div>
        <p className="dim small">{L.propsLocalNote}</p>
      </div>
    </Modal>
  );
}
