// The launcher's own update, in the simple mode's words: its block in Settings > About (the state, "Check for updates",
// and the same line the bar shows everywhere: Update, the download's progress, "Restart to finish"). The bar at the top
// of every page and the start dialog are in UpdateBar.tsx; both use the actions and the texts defined here. A build
// without an update repository shows none of it (`enabled` is false). Nothing is installed without a click. The notes
// come from the signed manifest and are set as text: no HTML, no Markdown, no link.
import type { ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { updateApi, type UpdateView } from "../api";
import { useApp } from "../ctx";
import { formatDateTime, useI18n, type Key } from "../i18n";
import { IconAlert, IconRefresh } from "../icons";
import { restartBlock } from "./prompt";
import { setUpdateView, useLauncherUpdate } from "./store";

export { setUpdateView, useLauncherUpdate };

export const mb = (bytes: number) => (bytes / 1048576).toFixed(bytes >= 10485760 ? 0 : 1);

export function errorKey(code: string): Key {
  const known = ["network", "integrity", "untrusted", "disk", "locked", "not_writable", "spawn", "other_instance", "installer", "rollback_failed", "rate_limited", "blocked_game", "blocked_install", "blocked_relay"];
  return (known.includes(code) ? `lup.err.${code}` : "lup.err.generic") as Key;
}

/** The release notes, as text. */
function Notes({ text }: { text: string }) {
  const { t } = useI18n();
  return (
    <details className="lup-notes">
      <summary>{t("lup.notes")}</summary>
      {text ? <pre className="lup-notes-text">{text}</pre> : <p className="small muted">{t("lup.notes.none")}</p>}
    </details>
  );
}

/** The update's line in Settings > About (`inAbout`: no "Later" there; it was the Play page's line before the bar). */
export function LauncherUpdateLine({ inAbout = false }: { inAbout?: boolean }) {
  const v = useLauncherUpdate();
  const { t } = useI18n();
  const { fail, game, game2, installing } = useApp();
  if (!v || !v.enabled) return null;
  const act = (p: Promise<UpdateView>) => void p.then(setUpdateView).catch(fail);
  const waitFor = restartBlock(v, { game: game.state === "running" || game.restarted || game2.state === "running", install: installing !== null, match: false });
  const ok = () => act(updateApi.dismiss());

  const a = v.available;
  if (!a) return null;
  if (!inAbout && v.later && v.phase === "idle") return null;

  const blocked = waitFor ? t(`lup.blocked.${waitFor}` as Key) : null;
  let title: string;
  let body: ReactNode = null;
  let actions: ReactNode = null;
  switch (v.phase) {
    case "downloading": {
      const p = v.progress;
      const pct = p && p.total > 0 ? Math.floor((p.done / p.total) * 100) : 0;
      title = t("lup.downloading", { version: a.version, pct });
      body = <div className="bar lup-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}><span style={{ width: `${pct}%` }} /></div>;
      actions = <button className="btn btn-small btn-ghost" onClick={() => act(updateApi.cancel())}>{t("lup.cancel")}</button>;
      break;
    }
    case "ready":
      if (a.mode === "elsewhere") {
        title = t("lup.ready.elsewhere", { version: a.version });
        body = <p className="small muted">{t(a.why === "temp" ? "lup.elsewhere.temp" : "lup.elsewhere.readonly")}</p>;
        actions = (
          <button className="btn btn-small" onClick={() => void open({ multiple: false, directory: true, title: t("lup.elsewhere.pick") }).then((d) => { if (typeof d === "string" && d) act(updateApi.saveTo(d)); }).catch(fail)}>
            {t("lup.elsewhere.pick")}
          </button>
        );
      } else {
        title = t(a.mode === "installer" ? "lup.ready.installer" : "lup.ready", { version: a.version });
        body = <p className="small muted">{blocked ?? t(a.mode === "installer" ? "lup.installer.hint" : "lup.restart.hint")}</p>;
        actions = (
          <button className="btn btn-small" disabled={!!waitFor} onClick={() => act(updateApi.apply())}>
            {t(a.mode === "installer" ? "lup.installer" : "lup.restart")}
          </button>
        );
      }
      break;
    case "applying":
    case "restarting":
      title = t(a.mode === "installer" ? "lup.starting.installer" : "lup.restarting", { version: a.version });
      break;
    case "saved":
      title = t("lup.saved", { version: a.version, path: v.saved_to ?? "" });
      body = <p className="small muted">{t("lup.saved.hint")}</p>;
      actions = (
        <>
          <button className="btn btn-small" onClick={() => void updateApi.showSaved().catch(fail)}>{t("lup.saved.open")}</button>
          <button className="btn btn-small btn-ghost" onClick={ok}>{t("lup.ok")}</button>
        </>
      );
      break;
    default:
      title = t("lup.avail", { version: a.version });
      body = <p className="small muted">{t(a.channel === "beta" ? "lup.avail.beta" : "lup.avail.size", { mb: mb(a.size) })}</p>;
      actions = (
        <>
          <button className="btn btn-small" onClick={() => act(updateApi.download())}>{v.error ? t("lup.retry") : t("lup.go")}</button>
          {!inAbout && <button className="btn btn-small btn-ghost" onClick={() => act(updateApi.later())}>{t("lup.later")}</button>}
        </>
      );
  }
  return (
    <div className={`upd-line lup-line ${inAbout ? "lup-in-about" : ""}`} role="status" aria-live="polite">
      <IconRefresh className="upd-ico" />
      <div className="grow lup-main">
        <p className="upd-title">{title}</p>
        {body}
        {v.error && v.phase !== "downloading" && <p className="small lup-error" role="alert"><IconAlert /> {t(errorKey(v.error))}</p>}
        {(v.phase === "idle" || v.phase === "ready") && <Notes text={a.notes} />}
      </div>
      {actions && <div className="row lup-actions">{actions}</div>}
    </div>
  );
}

/** Settings > About: the version, the state of the last check, "Check now"; the switch and the channel are drawn by the
 *  Settings page with its own controls (`children`). */
export function LauncherUpdateAbout({ children }: { children?: ReactNode }) {
  const v = useLauncherUpdate();
  const { t, lang } = useI18n();
  const { fail } = useApp();
  if (!v || !v.enabled) return null;
  let state: string;
  if (v.checking) state = t("lup.checking");
  else if (v.available) state = t("lup.avail", { version: v.available.version });
  else if (v.check_error === "rate_limited") state = t("lup.check.rate");
  else if (v.check_error === "untrusted") state = t("lup.check.untrusted");
  else if (v.check_error) state = t("lup.check.failed");
  else if (v.last_check) state = t("lup.uptodate");
  else state = t("lup.never");
  return (
    <div className="set-block lup-about">
      <div className="set-block-head">
        <h3>{t("lup.title")}</h3>
        <button className="btn btn-small" disabled={v.checking || v.phase === "downloading"} onClick={() => void updateApi.check().then(setUpdateView).catch(fail)}>
          {v.checking ? t("lup.checking") : t("lup.check")}
        </button>
      </div>
      <dl className="facts">
        <dt>{t("lup.current")}</dt><dd>{v.current}</dd>
        <dt>{t("lup.state")}</dt><dd>{state}</dd>
        {v.last_check && <><dt>{t("lup.last")}</dt><dd>{formatDateTime(new Date(v.last_check * 1000), lang)}</dd></>}
      </dl>
      {v.available && <LauncherUpdateLine inAbout />}
      {children}
    </div>
  );
}
