import { useCallback, useEffect, useRef, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";

/**
 * B-27：应用生态 2.0（设置页「生态」标签）。
 * - 可移植性评估向导（绿/黄红评估卡，启发式理由逐条列出）+ 搬迁执行器；
 * - Steam 库扫描（libraryfolders.vdf）与 steam:// 协议直通；
 * - 文件关联表（环境内"打开方式"，无关联时宿主兜底打开）。
 */

interface PortabilityCard {
  verdict: "green" | "yellow" | "red" | string;
  reasons: string[];
  exeSizeBytes: number;
  dirWritable: boolean;
  uninstallEntry: string | null;
}
interface SteamGame {
  appId: string;
  name: string;
}
interface FileAssoc {
  ext: string;
  appId: string;
  appName: string;
}

const VERDICT_COLOR: Record<string, string> = {
  green: "#3ecf8e",
  yellow: "#ffd479",
  red: "#ff8a8a",
};

export function EcoTab() {
  const { t } = useI18n();
  const [card, setCard] = useState<PortabilityCard | null>(null);
  const [cardExe, setCardExe] = useState<string>("");
  const [games, setGames] = useState<SteamGame[] | null>(null);
  const [assocs, setAssocs] = useState<FileAssoc[]>([]);
  const [assocExt, setAssocExt] = useState("");
  const busyRef = useRef(false);

  const refreshAssocs = useCallback(async () => {
    try {
      setAssocs(await ipc.fileAssocList());
    } catch {
      /* 列表可空 */
    }
  }, []);

  useEffect(() => {
    void refreshAssocs();
  }, [refreshAssocs]);

  const withBusy = async (fn: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    try {
      await fn();
    } finally {
      busyRef.current = false;
    }
  };

  const assessExe = () =>
    void withBusy(async () => {
      const f = await openFileDialog({
        multiple: false,
        filters: [{ name: "EXE", extensions: ["exe"] }],
      });
      if (typeof f !== "string") return;
      setCardExe(f);
      try {
        setCard(await ipc.portabilityAssess(f));
      } catch (e) {
        pushToast("error", t("ecoAssessFail"), errMessage(e).message);
      }
    });

  const migrate = () =>
    void withBusy(async () => {
      if (!cardExe) return;
      const name = cardExe.split(/[\\/]/).pop()?.replace(/\.exe$/i, "") ?? "app";
      try {
        const r = await ipc.ecosystemMigrate(cardExe, name);
        pushToast(
          "success",
          t("ecoMigrated"),
          `${r.appId} · ${formatBytes(r.bytesCopied)}`,
        );
      } catch (e) {
        pushToast("error", t("ecoMigrateFail"), errMessage(e).message);
      }
    });

  const scanSteam = () =>
    void withBusy(async () => {
      try {
        setGames(await ipc.steamLibraryScan());
      } catch (e) {
        pushToast("error", t("ecoSteamFail"), errMessage(e).message);
      }
    });

  const launchSteam = (g: SteamGame) =>
    void withBusy(async () => {
      try {
        await ipc.steamLaunch(g.appId);
        pushToast("success", t("ecoSteamLaunched"), g.name);
      } catch (e) {
        pushToast("error", t("ecoSteamFail"), errMessage(e).message);
      }
    });

  const addAssoc = () =>
    void withBusy(async () => {
      const ext = assocExt.trim();
      if (!ext) return;
      try {
        await ipc.fileAssocSet(ext, "vscode", "VS Code");
        setAssocExt("");
        await refreshAssocs();
        pushToast("success", t("ecoAssocSet"), ext);
      } catch (e) {
        pushToast("error", t("ecoAssocFail"), errMessage(e).message);
      }
    });

  return (
    <div className="eco-tab">
      {/* 评估向导 */}
      <h4>{t("ecoAssessTitle")}</h4>
      <p className="dim small">{t("ecoAssessHint")}</p>
      <button type="button" onClick={assessExe}>
        {t("ecoPickExe")}
      </button>
      {card && (
        <div className="eco-card">
          <div className="eco-verdict" style={{ color: VERDICT_COLOR[card.verdict] }}>
            {card.verdict.toUpperCase()}
          </div>
          <ul>
            {card.reasons.map((r, i) => (
              <li key={i} className="dim small">
                {r}
              </li>
            ))}
          </ul>
          <button type="button" onClick={migrate}>
            {t("ecoMigrate")}
          </button>
        </div>
      )}

      {/* Steam */}
      <h4>{t("ecoSteamTitle")}</h4>
      <button type="button" onClick={scanSteam}>
        {t("ecoSteamScan")}
      </button>
      {games && (
        <div className="eco-games">
          {games.length === 0 && <p className="dim small">{t("ecoSteamEmpty")}</p>}
          {games.map((g) => (
            <div key={g.appId} className="eco-game">
              <span>{g.name}</span>
              <button type="button" onClick={() => launchSteam(g)}>
                {t("ecoSteamRun")}
              </button>
            </div>
          ))}
        </div>
      )}

      {/* 文件关联 */}
      <h4>{t("ecoAssocTitle")}</h4>
      <div className="eco-assoc-add">
        <input
          placeholder={t("ecoAssocExt")}
          value={assocExt}
          onChange={(e) => setAssocExt(e.target.value)}
        />
        <button type="button" disabled={!assocExt.trim()} onClick={addAssoc}>
          {t("ecoAssocAdd")}
        </button>
      </div>
      {assocs.map((a) => (
        <div key={a.ext} className="dim small">
          .{a.ext} → {a.appName}
        </div>
      ))}
      <p className="dim small">{t("ecoAssocNote")}</p>
    </div>
  );
}
