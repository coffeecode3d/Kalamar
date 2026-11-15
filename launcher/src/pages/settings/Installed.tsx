// Settings > Installed game: is it ready, an update, reinstall, roll back, uninstall, the disk. The advanced mode adds
// what the former installation page showed: the game folder, the files checked, the last log, the last run.
import { useEffect, useState } from "react";
import { api, installApi, type DirCheck, type WizardInfo } from "../../api";
import { fmtDate, fmtMB, useApp, type Notice } from "../../ctx";
import { useI18n, type Key } from "../../i18n";
import { Modal, NoticeBar, Pill, useErrorNotice } from "../../ui";
import { IconInstall, IconRefresh } from "../../icons";
import { gb, approx, useW } from "../../install/texts";
import { startUpdate, useUpdateCheck } from "../../install/UpdateNotice";

function GameFolder() {
  const { t } = useI18n();
  const { config, refresh, setConfig, advanced } = useApp();
  const errNotice = useErrorNotice();
  const [root, setRoot] = useState("");
  const [translator, setTranslator] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  useEffect(() => {
    if (config) {
      setRoot(config.root ?? config.effective_root ?? "");
      setTranslator(config.translator ?? "");
    }
  }, [config]);
  const apply = async (f: () => Promise<unknown>) => {
    try {
      await f();
      setNotice(null);
      await refresh();
    } catch (e) {
      setNotice(errNotice(e));
    }
  };
  return (
    <div className="set-block">
      <div className="set-block-head"><h3>{t("home.root")}</h3></div>
      <p className="hint">{t("set.inst.root.hint")}</p>
      <div className="row">
        <input className="grow mono" value={root} placeholder={t("home.root.placeholder")} onChange={(e) => setRoot(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void apply(async () => setConfig(await api.setRoot(root.trim() || null)))} aria-label={t("home.root")} />
        <button className="btn btn-small" onClick={() => void apply(async () => setConfig(await api.setRoot(root.trim() || null)))}>{t("common.apply")}</button>
      </div>
      {advanced && (
        <>
          <label className="field-label" htmlFor="tr-path">{t("home.translator")}</label>
          <div className="row">
            <input id="tr-path" className="grow mono" value={translator} placeholder={config?.effective_translator ?? ""} onChange={(e) => setTranslator(e.target.value)} />
            <button className="btn btn-small" onClick={() => void apply(async () => setConfig(await api.setTranslator(translator.trim() || null)))}>{t("common.apply")}</button>
          </div>
          <p className="hint">{t("home.translator.hint")}</p>
        </>
      )}
      <NoticeBar notice={notice} />
    </div>
  );
}

export function Installed() {
  const { t, lang } = useI18n();
  const { w } = useW();
  const { status, lastRun, refresh, setConfig, toast, go, advanced } = useApp();
  const errNotice = useErrorNotice();
  const [info, setInfo] = useState<WizardInfo | null>(null);
  const [disk, setDisk] = useState<DirCheck | null>(null);
  const [ask, setAsk] = useState(false);
  const [saves, setSaves] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const update = useUpdateCheck();
  useEffect(() => { void refresh(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { void installApi.info().then(setInfo).catch(() => setInfo(null)); }, [status?.root]);
  const ready = status?.ready ?? false;
  // The wizard's folder holds the configured game root: <dir>\work\splatoon3-<version>.
  const dir = info && status?.root && info.state_finished != null && status.root.toLowerCase().startsWith(info.request.install_dir.toLowerCase()) ? info.request.install_dir : null;
  useEffect(() => { if (dir) void installApi.checkDir(dir).then(setDisk).catch(() => setDisk(null)); }, [dir]);
  // The install that holds the configured game root, when it keeps a previous executable (after an update).
  const here = info?.existing.find((e) => status?.root && e.game_root.toLowerCase() === status.root.toLowerCase()) ?? null;
  const act = async (f: () => Promise<void>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setNotice(errNotice(e));
    } finally {
      setBusy(false);
    }
  };
  const uninstall = () => act(async () => {
    if (!dir) return;
    const r = await installApi.uninstall(dir, saves);
    setAsk(false);
    toast({ kind: "ok", text: w("un.done", { size: gb(r.freed, 1) }) });
    setConfig(await api.getConfig());
    await refresh();
  });
  const rollback = () => act(async () => {
    if (!here) return;
    const r = await installApi.rollback(here.dir);
    toast({ kind: "ok", text: w("home.wiz.rollback.done", { now: r.now.slice(0, 12) }) });
    setInfo(await installApi.info());
    await refresh();
  });
  const missing = (status?.missing ?? []).map((m) => t(`check.${m}` as Key)).join(", ");

  return (
    <div className="stack">
      <div className={`inst-state ${ready ? "ok" : "bad"}`}>
        <span className="hero-dot" />
        <div className="grow">
          <p className="inst-title">{ready ? t("set.inst.ready") : status?.root ? t("set.inst.incomplete") : t("set.inst.none")}</p>
          <p className="small muted">
            {ready && status?.build_date ? t("set.inst.built", { date: fmtDate(status.build_date, lang) }) : !ready && status?.root && missing ? (advanced ? t("home.missing", { list: missing }) : t("set.inst.missing")) : t("set.inst.none.hint")}
            {disk?.free != null && ` · ${t("set.inst.free", { n: gb(disk.free) })}`}
          </p>
        </div>
      </div>

      {update?.available && update.update && (
        <div className="upd-line">
          <IconRefresh className="upd-ico" />
          <p className="grow upd-title">{w("upd.avail.short", { t: approx(update.eta) })}</p>
          <button className="btn btn-small" onClick={() => startUpdate(update.install_dir, go)}>{w("upd.avail.go")}</button>
        </div>
      )}

      <div className="row wrap">
        <button className={`btn ${ready ? "btn-ghost" : ""}`} onClick={() => go("setup")}>{ready ? <IconRefresh /> : <IconInstall />} {ready ? t("inst.reinstall") : w("home.wiz.install")}</button>
        {here?.previous && <button className="btn btn-ghost" disabled={busy} onClick={() => void rollback()}>{w("home.wiz.rollback")}</button>}
        {dir && <button className="btn btn-ghost" onClick={() => { setSaves(false); setNotice(null); setAsk(true); }}>{t("inst.uninstall")}</button>}
      </div>
      <p className="hint">{w("home.wiz.text")}</p>
      {!ask && <NoticeBar notice={notice} />}
      {advanced && ready && !dir && <p className="hint">{w("home.wiz.notwizard")}</p>}

      {(advanced || !ready) && <GameFolder />}

      {advanced && (
        <>
          <div className="set-block">
            <div className="set-block-head"><h3>{t("home.title")}</h3></div>
            <ul className="checks">
              {status?.checks.map((c) => (
                <li key={c.id}>
                  <Pill ok={c.present ? true : c.required ? false : null}>{c.present ? t("check.present") : c.required ? t("check.absent") : t("check.optional")}</Pill>
                  <div>
                    <div className="check-name">{t(`check.${c.id}` as Key)}{c.size ? <span className="muted"> · {t("home.size", { n: fmtMB(c.size) })}</span> : null}</div>
                    <div className="check-path mono">{c.path}</div>
                  </div>
                </li>
              ))}
            </ul>
            {status && status.root && !status.root_exists && <p className="bad-text">{t("check.root")} : {t("check.absent")}</p>}
            <p className="small"><span className="muted">{t("home.userdata")} :</span> <span className="mono wrapany">{status?.userdata ?? "—"}</span></p>
          </div>
          <div className="set-block">
            <div className="set-block-head"><h3>{t("home.lastlog")}</h3></div>
            {status?.last_log ? (
              <>
                <p className="row wrap">
                  <Pill ok={status.last_log.status === "completed" ? true : status.last_log.status === "fatal" ? false : null}>{t(`log.status.${status.last_log.status}` as Key)}</Pill>
                  <span className="mono small">{status.last_log.name}</span>
                  <span className="muted small">{fmtDate(status.last_log.modified, lang)} · {t("home.size", { n: fmtMB(status.last_log.size) })}</span>
                </p>
                {status.last_log.problem_lines.length > 0 && <pre className="log">{status.last_log.problem_lines.join("\n")}</pre>}
              </>
            ) : (
              <p className="muted">{t("home.lastlog.none")}</p>
            )}
            <p className="small">
              <span className="muted">{t("home.lastrun")} :</span>{" "}
              {lastRun ? <>{lastRun.stopped_by_user ? t("home.lastrun.stopped") : t("home.lastrun.exit", { code: lastRun.exit_code ?? t("common.unknown") })} <span className="muted">· {fmtDate(lastRun.ended_at, lang)}</span></> : t("common.none")}
            </p>
          </div>
        </>
      )}

      {ask && dir && (
        <Modal title={w("un.title")} onClose={() => setAsk(false)}>
          <p>{advanced ? w("un.text", { dir }) : w("un.text.simple")}</p>
          <label className="check"><input type="checkbox" checked={saves} onChange={(e) => setSaves(e.target.checked)} /> {w("un.saves")}</label>
          {!saves && <p className="hint">{w("un.saves.kept")}</p>}
          <NoticeBar notice={notice} />
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button className="btn btn-ghost" data-autofocus onClick={() => setAsk(false)}>{w("un.cancel")}</button>
            <button className="btn btn-danger" disabled={busy} onClick={() => void uninstall()}>{w("un.go")}</button>
          </div>
        </Modal>
      )}
    </div>
  );
}
