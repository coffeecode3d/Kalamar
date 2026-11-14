// The install wizard: Welcome → Your files → What will be installed → Install → Done. The back end
// (src-tauri/src/installer/) runs the install pipeline; this screen only gathers the files, shows the plan, asks for the
// agreement and follows the progress events. The simple mode keeps each screen to what a player needs; the technical
// details (tools and their identifiers, every step, the live log, the build) are folded or in the advanced mode.
import "../styles/wizard.css";
import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api, installApi, onInstallProgress, toAppError, wizardIntent, type DirCheck, type FileCheck, type InstallPlan, type InstallRequest,
  type InstallSnapshot, type StepView, type WizardInfo, type ExistingInstall,
} from "../api";
import { UpdateSummary } from "./UpdateNotice";
import { ReadyRecap, WaitTasks } from "./WaitTasks";
import { useApp } from "../ctx";
import { BRAND } from "../brand";
import { LANGS, useI18n } from "../i18n";
import { redact } from "../diagnostic";
import { IconAlert, IconCheck, IconFolder, IconInfo, IconPlay, IconRefresh, IconStop, Mark } from "../icons";
import { CopyButton, Modal } from "../ui";
import { fmtDuration, gb, useW } from "./texts";

type Screen = "welcome" | "files" | "plan" | "install" | "done";
type FileKind = "base" | "update" | "keys";
const SCREENS: Screen[] = ["welcome", "files", "plan", "install", "done"];

const EMPTY_REQ: InstallRequest = { install_dir: "", base_nsp: "", update_nsp: "", prod_keys: "", jobs: 0, allow_prereqs: false, force_rebuild: false, free_build_files: false, low_priority: false };

function Stepper({ at }: { at: Screen }) {
  const { w } = useW();
  const idx = SCREENS.indexOf(at);
  return (
    <ol className="wiz-stepper" aria-label={w("wiz.steps")}>
      {SCREENS.map((s, i) => (
        <li key={s} className={i < idx ? "done" : i === idx ? "now" : ""} aria-current={i === idx ? "step" : undefined}>
          <span className="wiz-dot">{i < idx ? <IconCheck /> : i + 1}</span>
          <span className="wiz-step-name">{w(`wiz.stepper.${s}`)}</span>
        </li>
      ))}
    </ol>
  );
}

function Bar({ value, indeterminate, big }: { value: number | null; indeterminate?: boolean; big?: boolean }) {
  const pct = value == null ? 0 : Math.round(Math.max(0, Math.min(1, value)) * 1000) / 10;
  return (
    <div className={`wiz-bar ${big ? "wiz-bar-big" : ""} ${indeterminate ? "indet" : ""}`} role="progressbar" aria-valuemin={0} aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : pct}>
      <span style={indeterminate ? undefined : { width: `${pct}%` }} />
    </div>
  );
}

function Check({ c, w }: { c: FileCheck | null | "busy"; w: (k: string, v?: Record<string, string | number>) => string }) {
  if (c === null) return null;
  if (c === "busy") return <div className="wiz-check busy">{w("wiz.file.checking")}</div>;
  const vars = Object.fromEntries(c.params);
  const Icon = c.status === "ok" ? IconCheck : c.status === "warn" ? IconInfo : IconAlert;
  return <div className={`wiz-check ${c.status}`}><Icon />{w(`chk.${c.code}`, vars)}</div>;
}

function FileRow({ label, hint, value, onChange, onPick, check, pickLabel, invalid }: {
  label: string; hint: string; value: string; onChange: (v: string) => void; onPick: () => void; check: ReactNode; pickLabel: string; invalid?: string;
}) {
  const id = useId();
  return (
    <div className="wiz-file">
      <label htmlFor={id} className="wiz-file-label">{label}</label>
      <div className="wiz-file-hint">{hint}</div>
      <div className="row">
        <input id={id} className="grow mono" value={value} spellCheck={false} onChange={(e) => onChange(e.target.value)} aria-invalid={!!invalid} />
        <button type="button" className="btn btn-ghost" onClick={onPick}><IconFolder /> {pickLabel}</button>
      </div>
      {invalid && <div className="wiz-check bad"><IconAlert />{invalid}</div>}
      {check}
    </div>
  );
}

/** The language menu of the first run (the rail and the settings are not there yet). */
function LangMenu() {
  const { lang } = useI18n();
  const { setConfig, fail } = useApp();
  return (
    <label className="wiz-lang">
      <span className="sr-only">Language</span>
      <select value={lang} onChange={(e) => void api.setLanguage(e.target.value).then(setConfig).catch(fail)} aria-label="Language">
        {LANGS.map((l) => <option key={l.code} value={l.code} lang={l.code}>{l.name}</option>)}
      </select>
    </label>
  );
}

export default function Wizard({ reinstall, onExit, firstRun }: { reinstall?: boolean; onExit?: () => void; firstRun?: boolean }) {
  const { w } = useW();
  const { go, refresh, reloadPlayer, setConfig, advanced } = useApp();
  const [screen, setScreen] = useState<Screen>("welcome");
  const [info, setInfo] = useState<WizardInfo | null>(null);
  const [req, setReq] = useState<InstallRequest>(EMPTY_REQ);
  const [checks, setChecks] = useState<Record<FileKind, FileCheck | null | "busy">>({ base: null, update: null, keys: null });
  const [dir, setDir] = useState<DirCheck | null>(null);
  const [fieldErr, setFieldErr] = useState<Record<string, string>>({});
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const [agree, setAgree] = useState(false);
  const [snap, setSnap] = useState<InstallSnapshot | null>(null);
  const [askCancel, setAskCancel] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const errMsg = (e: unknown) => { const a = toAppError(e); return a.kind === "runtime_missing" ? w("wiz.bundle.error") : a.message; };

  // Load the remembered request and any running install.
  useEffect(() => {
    void (async () => {
      try {
        const i = await installApi.info();
        setInfo(i);
        setReq({ ...i.request, jobs: i.request.jobs || 0 });
        // A running install, or (from the rail) how the last one ended: ready, or why it stopped.
        const result = wizardIntent.showResult && !!i.snapshot.outcome;
        wizardIntent.showResult = false;
        if (i.snapshot.active || result) {
          setSnap(i.snapshot);
          setScreen("install");
        }
      } catch (e) {
        setError(errMsg(e));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let off: (() => void) | undefined;
    void onInstallProgress((s) => {
      setSnap(s);
      if (s.outcome === "done") {
        void refresh();
        void reloadPlayer();
      }
    }).then((u) => (off = u));
    return () => off?.();
  }, [refresh, reloadPlayer]);

  // File checks, debounced per field.
  const runCheck = useCallback(async (kind: FileKind, path: string) => {
    if (!path.trim()) return setChecks((c) => ({ ...c, [kind]: null }));
    setChecks((c) => ({ ...c, [kind]: "busy" }));
    try {
      const r = await installApi.checkFile(kind, path);
      setChecks((c) => ({ ...c, [kind]: r }));
    } catch {
      setChecks((c) => ({ ...c, [kind]: { status: "bad", code: "unreadable", params: [], size: null, program_size: null } }));
    }
  }, []);
  useEffect(() => { if (screen === "files") { const t = setTimeout(() => void runCheck("base", req.base_nsp), 300); return () => clearTimeout(t); } }, [req.base_nsp, screen, runCheck]);
  useEffect(() => { if (screen === "files") { const t = setTimeout(() => void runCheck("update", req.update_nsp), 300); return () => clearTimeout(t); } }, [req.update_nsp, screen, runCheck]);
  useEffect(() => { if (screen === "files") { const t = setTimeout(() => void runCheck("keys", req.prod_keys), 300); return () => clearTimeout(t); } }, [req.prod_keys, screen, runCheck]);
  useEffect(() => {
    if (screen !== "files" || !req.install_dir.trim()) return;
    const n = ++seq.current;
    const t = setTimeout(() => void installApi.checkDir(req.install_dir).then((d) => n === seq.current && setDir(d)), 300);
    return () => clearTimeout(t);
  }, [req.install_dir, screen]);

  const pick = async (kind: FileKind) => {
    const filters = kind === "keys"
      ? [{ name: w("wiz.filter.keys"), extensions: ["keys"] }, { name: w("wiz.filter.all"), extensions: ["*"] }]
      : [{ name: w("wiz.filter.nsp"), extensions: ["nsp"] }, { name: w("wiz.filter.all"), extensions: ["*"] }];
    const r = await open({ multiple: false, directory: false, filters, title: w(`wiz.file.${kind}`) });
    if (typeof r === "string") setReq((q) => ({ ...q, [kind === "base" ? "base_nsp" : kind === "update" ? "update_nsp" : "prod_keys"]: r }));
  };
  const pickDir = async () => {
    const r = await open({ multiple: false, directory: true, title: w("wiz.file.dir") });
    if (typeof r === "string") setReq((q) => ({ ...q, install_dir: r }));
  };

  const filesNeeded = dir?.needs_files ?? true;
  const filesOk = (["base", "update", "keys"] as FileKind[]).every((k) => { const c = checks[k]; return filesNeeded ? c && c !== "busy" && c.status !== "bad" : c !== "busy" && (!c || c.status !== "bad"); })
    && !!dir && dir.absolute && dir.kind !== "other" && (dir.free == null || dir.free >= dir.need);

  const toPlan = async () => {
    setPlanBusy(true);
    setFieldErr({});
    try {
      const p = await installApi.plan(req);
      setPlan(p);
      setAgree(false);
      setScreen("plan");
    } catch (e) {
      const err = toAppError(e);
      if (err.kind === "validation") setFieldErr(Object.fromEntries(err.fields.map((f) => [f.key, w(`val.${f.message}`)])));
      else setError(err.kind === "runtime_missing" ? w("wiz.bundle.error") : err.message);
    } finally {
      setPlanBusy(false);
    }
  };

  const start = async (r: InstallRequest) => {
    setError(null);
    try {
      const s = await installApi.start(r);
      setSnap(s);
      setScreen("install");
    } catch (e) {
      setError(errMsg(e));
    }
  };

  const resume = async () => {
    if (!info) return;
    // A resume shows the plan again only when tools are still missing (their install needs the agreement).
    try {
      const p = await installApi.plan(info.request);
      setPlan(p);
      if (p.needs_prereqs) {
        setAgree(false);
        setScreen("plan");
      } else void start({ ...info.request, allow_prereqs: false });
    } catch (e) {
      setError(errMsg(e));
    }
  };

  /** "Update the installed game": the folder of an existing install; its extracted data makes the files optional. */
  const adopt = async (e: ExistingInstall) => {
    const r = { ...req, install_dir: e.dir };
    setReq(r);
    if (e.needs_files) return setScreen("files");
    try {
      setPlan(await installApi.plan(r));
      setAgree(false);
      setScreen("plan");
    } catch (x) {
      setError(errMsg(x));
    }
  };

  // Opened from an "Update" button: straight to the plan of that install (it says what will be redone).
  useEffect(() => {
    const dir = wizardIntent.updateDir;
    if (!info || !dir || info.pending || info.snapshot.active) return;
    wizardIntent.updateDir = null;
    const e = info.existing.find((x) => x.dir.toLowerCase() === dir.toLowerCase());
    if (e) void adopt(e);
  }, [info]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Keep playing the installed build as it is (the launcher points at it; the update can be done later). */
  const useAsIs = async (e: ExistingInstall) => {
    try {
      setConfig(await api.setRoot(e.game_root));
      await refresh();
      onExit?.();
      go("play");
    } catch (x) {
      setError(errMsg(x));
    }
  };

  const shown: Screen = snap?.outcome === "done" && screen === "install" ? "done" : screen;
  useEffect(() => { document.getElementById("main")?.scrollTo(0, 0); }, [shown]);
  const existing = (info?.existing ?? []).filter((e) => e.exe || e.exefs);

  return (
    <section className="page wiz" aria-labelledby="wiz-title">
      <div className="wiz-top">
        <div className="wiz-brand"><Mark size="2.2rem" /><span className="display">{BRAND.name}</span></div>
        <Stepper at={shown} />
        {firstRun && <LangMenu />}
      </div>
      {info?.bundle_error && <div className="notice notice-error" role="alert"><IconAlert width="1.2em" height="1.2em" /><span>{w("wiz.bundle.error")}</span></div>}
      {error && <div className="notice notice-error" role="alert"><IconAlert width="1.2em" height="1.2em" /><span>{error}</span></div>}

      {shown === "welcome" && (
        <div className="wiz-body">
          <h1 id="wiz-title" className="display wiz-title">{reinstall ? w("wiz.reinstall.title") : w("wiz.welcome.title")}</h1>
          <p className="wiz-lead">{reinstall ? w("wiz.reinstall.lead") : w("wiz.welcome.lead2")}</p>
          {info?.pending && (
            <div className="card card-warn wiz-resume">
              <h2 className="card-title">{w("wiz.resume.title")}</h2>
              <p>{advanced ? w("wiz.resume.text", { dir: info.request.install_dir }) : w("wiz.resume.text2")}</p>
              {info.interrupted_step && <p><strong>{w("wiz.resume.step", { step: w(`step.${info.interrupted_step}`) })}</strong></p>}
              {advanced && info.recovered.length > 0 && <p className="hint">{w("wiz.resume.recovered", { list: info.recovered.join(" ; ") })}</p>}
              {info.needs_reboot && <p className="warn-text">{w("wiz.resume.reboot")}</p>}
              <button className="btn btn-big" onClick={() => void resume()}><IconRefresh /> {w("wiz.resume.go")}</button>
            </div>
          )}
          {!info?.pending && existing.map((e) => (
            <div key={e.dir} className="card card-ok wiz-existing">
              <h2 className="card-title">{w(e.kind === "kit" ? "wiz.exist.kit" : "wiz.exist.wizard")}</h2>
              <p>{w(e.needs_files ? "wiz.exist.files" : "wiz.exist.nofiles2")} {w("wiz.exist.data")}</p>
              {advanced && (
                <ul className="wiz-list small muted">
                  <li className="mono">{e.dir}</li>
                  <li>{w("wiz.exist.source", { source: e.source ?? "?" })}</li>
                  <li>{w(e.lift && e.objects ? "wiz.exist.fast" : "wiz.exist.slow")}</li>
                </ul>
              )}
              <div className="row wrap">
                <button className="btn" onClick={() => void adopt(e)}><IconRefresh /> {w("wiz.exist.update")}</button>
                {e.exe && <button className="btn btn-ghost" onClick={() => void useAsIs(e)}>{w("wiz.exist.asis")}</button>}
              </div>
            </div>
          ))}
          <div className="wiz-grid">
            <div className="card">
              <h2 className="card-title">{w("wiz.need.title")}</h2>
              <ul className="wiz-list">
                <li>{w("wiz.need.base")}</li>
                <li>{w("wiz.need.update")}</li>
                <li>{w("wiz.need.keys")}</li>
                <li>{w("wiz.need.disk", { need: info ? gb(info.fresh.disk_need) : "15", final: info ? gb(info.fresh.footprint) : "13" })}</li>
                <li>{info ? w("wiz.need.time2", { min: Math.max(1, Math.round(info.fresh.eta / 60)), mb: Math.round(info.fresh.download_bytes / 1e6) }) : w("wiz.need.time")}</li>
                <li>{w("wiz.need.nothing")}</li>
              </ul>
            </div>
            <div className="card wiz-legal">
              <h2 className="card-title">{w("wiz.legal.title")}</h2>
              <ul className="wiz-list">
                {[1, 2, 3, 4].map((n) => <li key={n}>{w(`wiz.legal.${n}`)}</li>)}
              </ul>
            </div>
          </div>
          {advanced && info?.bundle && (
            <details className="advanced wiz-build">
              <summary>{w("wiz.build.title")}</summary>
              <p className="small">{w("wiz.build.text", { commit: info.bundle.commit.slice(0, 12), files: info.bundle.files })}</p>
            </details>
          )}
          <div className="wiz-actions">
            {onExit && <button className="btn btn-ghost" onClick={onExit}>{reinstall ? w("wiz.back") : w("wiz.have.build")}</button>}
            <button className="btn btn-big" onClick={() => setScreen("files")} disabled={!!info?.bundle_error} data-autofocus data-next>{w("wiz.start")}</button>
          </div>
        </div>
      )}

      {shown === "files" && (
        <div className="wiz-body">
          <h1 id="wiz-title" className="display wiz-title">{w("wiz.files.title")}</h1>
          <p className="wiz-lead">{w("wiz.files.lead")}</p>
          {dir && !dir.needs_files && <div className="notice notice-ok"><IconCheck width="1.2em" height="1.2em" /><span>{w("wiz.files.notneeded")}</span></div>}
          <div className="card wiz-files">
            <FileRow label={w("wiz.file.base")} hint={w("wiz.file.base.hint")} value={req.base_nsp} pickLabel={w("wiz.file.pick")}
              onChange={(v) => setReq({ ...req, base_nsp: v })} onPick={() => void pick("base")} check={<Check c={checks.base} w={w} />} invalid={fieldErr.base_nsp} />
            <FileRow label={w("wiz.file.update")} hint={w("wiz.file.update.hint")} value={req.update_nsp} pickLabel={w("wiz.file.pick")}
              onChange={(v) => setReq({ ...req, update_nsp: v })} onPick={() => void pick("update")} check={<Check c={checks.update} w={w} />} invalid={fieldErr.update_nsp} />
            <FileRow label={w("wiz.file.keys")} hint={w("wiz.file.keys.hint")} value={req.prod_keys} pickLabel={w("wiz.file.pick")}
              onChange={(v) => setReq({ ...req, prod_keys: v })} onPick={() => void pick("keys")} check={<Check c={checks.keys} w={w} />} invalid={fieldErr.prod_keys} />
            <FileRow label={w("wiz.file.dir")} hint={advanced ? w("wiz.file.dir.hint", { sub: "work\\splatoon3-11.2.0" }) : w("wiz.file.dir.hint2")} value={req.install_dir} pickLabel={w("wiz.file.pickdir")}
              onChange={(v) => setReq({ ...req, install_dir: v })} onPick={() => void pickDir()} invalid={fieldErr.install_dir}
              check={dir && (
                <>
                  {!dir.absolute ? <div className="wiz-check bad"><IconAlert />{w("dir.relative")}</div>
                    : <div className={`wiz-check ${dir.kind === "other" ? "bad" : "ok"}`}>{dir.kind === "other" ? <IconAlert /> : <IconCheck />}{w(`dir.${dir.kind}`)}</div>}
                  {dir.absolute && dir.free != null && (
                    <div className={`wiz-check ${dir.free >= dir.need ? "ok" : "bad"}`}>{dir.free >= dir.need ? <IconInfo /> : <IconAlert />}
                      {w(dir.free >= dir.need ? "dir.free" : "dir.free.low", { free: gb(dir.free), need: gb(dir.need) })}</div>
                  )}
                </>
              )} />
            {advanced && (
              <details className="advanced">
                <summary>{w("wiz.advanced")}</summary>
                <label className="field-label" htmlFor="wiz-jobs">{w("wiz.adv.jobs")}</label>
                <div className="row">
                  <input id="wiz-jobs" type="number" min={0} max={64} className="mono" style={{ maxWidth: "7rem" }} value={req.jobs || ""}
                    placeholder={String(info?.default_jobs ?? 8)} onChange={(e) => setReq({ ...req, jobs: Math.max(0, Math.min(64, Number(e.target.value) || 0)) })} />
                  <span className="hint">{w("wiz.adv.jobs.hint", { n: info?.default_jobs ?? 8 })}</span>
                </div>
                <label className="check"><input type="checkbox" checked={req.low_priority} onChange={(e) => setReq({ ...req, low_priority: e.target.checked })} /> {w("wiz.adv.low")}</label>
                <label className="check"><input type="checkbox" checked={req.free_build_files} onChange={(e) => setReq({ ...req, free_build_files: e.target.checked })} /> {w("wiz.adv.free", { size: 2 })}</label>
                {reinstall && <label className="check"><input type="checkbox" checked={req.force_rebuild} onChange={(e) => setReq({ ...req, force_rebuild: e.target.checked })} /> {w("wiz.adv.force")}</label>}
              </details>
            )}
          </div>
          <div className="wiz-actions">
            <button className="btn btn-ghost" onClick={() => setScreen("welcome")}>{w("wiz.back")}</button>
            <button className="btn btn-big" disabled={!filesOk || planBusy} onClick={() => void toPlan()} data-next>{w("wiz.next")}</button>
          </div>
        </div>
      )}

      {shown === "plan" && plan && <PlanView plan={plan} req={req} agree={agree} setAgree={setAgree} advanced={advanced}
        onBack={() => setScreen("files")} onStart={() => void start({ ...req, allow_prereqs: plan.needs_prereqs && agree })} />}

      {shown === "install" && snap && <InstallView snap={snap} onCancel={() => setAskCancel(true)}
        onRetry={() => void start({ ...(info?.request ?? req), ...req, allow_prereqs: req.allow_prereqs || !!plan?.needs_prereqs })}
        onFiles={() => setScreen("files")} />}

      {shown === "done" && snap && (
        <div className="wiz-body">
          {/* Ready: Play first, then what was set during the wait and what is left (each with its button). */}
          <div className="wiz-done">
            <div className="wiz-done-mark" aria-hidden="true"><IconCheck /></div>
            <div className="grow">
              <h1 id="wiz-title" className="display wiz-title">{w(snap.update ? "ready.update.title" : "wiz.done.title")}</h1>
              <p className="wiz-lead">{w(snap.update ? "ready.update.lead" : "wiz.done.lead2")}</p>
              <p className="small muted">
                {w("wiz.done.time", { t: fmtDuration(snap.elapsed) })}
                {snap.footprint != null && ` · ${w("wiz.done.size", { size: gb(snap.footprint, 1) })}`}
              </p>
              {advanced && <p className="mono small muted">{w("wiz.done.where", { path: snap.game_root ?? "" })}</p>}
            </div>
            <button className="btn btn-big btn-play wiz-done-play" data-autofocus onClick={() => { onExit?.(); go("play"); }}><IconPlay /> {w("ready.play")}</button>
          </div>
          <ReadyRecap />
          <p className="hint">{advanced ? w("wiz.done.note") : w("wiz.done.note2")}</p>
          {advanced && info?.existing.some((e) => e.dir.toLowerCase() === (snap.install_dir ?? "").toLowerCase() && e.exe) && snap.steps.some((s) => s.id === "compile" && s.status === "done") && <p className="hint">{w("wiz.done.rollback")}</p>}
        </div>
      )}

      {askCancel && (
        <Modal title={w("wiz.run.cancel.confirm.title")} onClose={() => setAskCancel(false)}>
          <p>{w("wiz.run.cancel.confirm.text")}</p>
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <button className="btn btn-ghost" data-autofocus onClick={() => setAskCancel(false)}>{w("wiz.run.cancel.no")}</button>
            <button className="btn btn-danger" onClick={() => { setAskCancel(false); void installApi.cancel(); }}>{w("wiz.run.cancel.yes")}</button>
          </div>
        </Modal>
      )}
    </section>
  );
}

/** What will be installed: three facts (time, disk, tools), the downloads and where they come from, the agreement.
 *  The advanced mode adds the tools' identifiers, the packages, every step, memory and the source commit. */
function PlanView({ plan, req, agree, setAgree, advanced, onBack, onStart }: {
  plan: InstallPlan; req: InstallRequest; agree: boolean; setAgree: (v: boolean) => void; advanced: boolean; onBack: () => void; onStart: () => void;
}) {
  const { w } = useW();
  const missingTools = plan.tools.filter((t) => !t.path);
  const downloads = plan.downloads.filter((d) => advanced || d.needed);
  // Agreeing is asked only when something would be installed on the system (never with the portable tools): the
  // Install button is then the agreement.
  const ok = agree || !plan.needs_prereqs;
  const portable = plan.toolchain === "portable";
  const system = plan.toolchain === "system";
  return (
    <div className="wiz-body">
      <h1 id="wiz-title" className="display wiz-title">{w("wiz.plan.title")}</h1>
      <p className="wiz-lead">{w(portable ? "wiz.plan.lead2" : system ? "wiz.plan.lead3" : "wiz.plan.lead")}</p>
      {plan.existing && (
        <div className="notice notice-ok"><IconCheck width="1.2em" height="1.2em" />
          <span>{w(plan.needs_files ? "wiz.plan.update.files" : "wiz.plan.update.nofiles")} {w("wiz.plan.update.safe")}</span>
        </div>
      )}
      {plan.update && (
        <UpdateSummary u={plan.update} eta={plan.eta} etaMax={plan.eta_max} redo={plan.steps.filter((s) => !s.done).map((s) => s.id)}
          from={plan.installed_source} to={plan.source_commit} />
      )}
      <dl className="plan-facts">
        <div><dt>{w("wiz.plan.fact.time")}</dt><dd>{w("wiz.plan.fact.time.v", { min: Math.max(1, Math.round(plan.eta / 60)) })}</dd></div>
        <div><dt>{w("wiz.plan.fact.disk")}</dt><dd>{w("wiz.plan.fact.disk.v", { need: gb(plan.disk_needed), final: gb(plan.footprint) })}<span className="muted"> · {w("wiz.plan.fact.free", { free: gb(plan.disk_free) })}</span></dd></div>
        <div><dt>{w("wiz.plan.fact.tools")}</dt><dd>{portable ? w(plan.download_bytes > 0 ? "wiz.plan.fact.tools.portable" : "wiz.plan.fact.tools.inplace", { mb: Math.round(plan.download_bytes / 1e6) }) : missingTools.length ? w("wiz.plan.fact.tools.v", { list: missingTools.map((t) => w(`tool.${t.id}`).split(" (")[0]).join(", ") }) : w("wiz.plan.fact.tools.ok")}</dd></div>
      </dl>
      {plan.needs_admin && <div className="notice notice-info"><IconInfo width="1.2em" height="1.2em" /><span>{w("wiz.plan.admin")}</span></div>}

      <div className="card">
        <h2 className="card-title">{w("wiz.plan.downloads")}</h2>
        <ul className="wiz-downloads">
          {downloads.map((d) => (
            <li key={d.id} className={d.needed ? "" : "muted"}>
              <span className="dl-name">{w(`dl.${d.id}`)}</span>
              <span className="dl-src mono">{d.source}</span>
              {d.bytes > 0 && <span className="tiny muted">{w("wiz.plan.mb", { mb: (d.bytes / 1e6).toFixed(d.bytes < 10e6 ? 1 : 0) })}</span>}
              {advanced && <span className="tiny muted">{d.needed ? (d.items.length ? d.items.join(" · ") : w("wiz.plan.needed")) : w("wiz.plan.notneeded")}</span>}
            </li>
          ))}
        </ul>
        <p className="hint">{w("wiz.plan.nothing")}</p>
      </div>

      {advanced && (
        <div className="wiz-grid">
          <div className="card">
            <h2 className="card-title">{w(portable ? "wiz.plan.tools2" : "wiz.plan.tools")}</h2>
            {portable && <p className="ok-text"><IconCheck width="1.1em" height="1.1em" /> {w("wiz.plan.tools.portable", { dir: plan.tools_dir })}</p>}
            {!portable && !plan.needs_prereqs && <p className="ok-text"><IconCheck width="1.1em" height="1.1em" /> {w("wiz.plan.tools.allok")}</p>}
            <ul className="wiz-tools">
              {plan.tools.map((t) => (
                <li key={t.id}>
                  <span className={`pill ${t.path ? "pill-ok" : "pill-bad"}`}>{t.path ? w("wiz.plan.tool.present") : w("wiz.plan.tool.install", { id: t.winget })}</span>
                  <span>{w(`tool.${t.id}`)}</span>
                </li>
              ))}
            </ul>
            {plan.missing_packages.length > 0 && <p className="hint">{w(system ? "wiz.plan.packages.system" : "wiz.plan.packages", { list: plan.missing_packages.join(", ") })}</p>}
            <ul className="wiz-list small">
              <li>{w("wiz.plan.disk.need2", { need: gb(plan.disk_needed, 1), free: gb(plan.disk_free), final: gb(plan.footprint, 1) })}</li>
              <li>{w("wiz.plan.ram", { ram: plan.ram == null ? "?" : Math.round(plan.ram / 2 ** 30), jobs: plan.jobs })}</li>
              {req.low_priority && <li>{w("wiz.plan.priority")}</li>}
              {plan.source_commit && <li className="muted">{w("wiz.plan.source", { commit: plan.source_commit.slice(0, 12) })}</li>}
            </ul>
          </div>
          <div className="card">
            <h2 className="card-title">{w("wiz.plan.steps")}</h2>
            <ol className="wiz-plan-steps">
              {plan.steps.map((s) => (
                <li key={s.id} className={s.done ? "muted" : ""}>{w(`step.${s.id}`)}{s.done
                  ? <span className="pill pill-ok">{w("wiz.plan.step.done")}</span>
                  : s.reference >= 20 && <span className="tiny muted"> · ~{fmtDuration(s.reference)}</span>}</li>
              ))}
            </ol>
          </div>
        </div>
      )}

      {plan.needs_prereqs && (
        <label className="check wiz-agree">
          <input type="checkbox" checked={agree} onChange={(e) => setAgree(e.target.checked)} />
          <span>{w("wiz.plan.confirm.tools")}</span>
        </label>
      )}
      <div className="wiz-actions">
        <button className="btn btn-ghost" onClick={onBack}>{w("wiz.back")}</button>
        <button className="btn btn-big" disabled={!ok} onClick={onStart} data-next>{w("wiz.plan.install")}</button>
      </div>
    </div>
  );
}

function stepIcon(s: StepView) {
  if (s.status === "done" || s.status === "skipped") return <IconCheck />;
  if (s.status === "failed") return <IconAlert />;
  if (s.status === "cancelled") return <IconStop />;
  return <span />;
}

function InstallView({ snap, onCancel, onRetry, onFiles }: { snap: InstallSnapshot; onCancel: () => void; onRetry: () => void; onFiles: () => void }) {
  const { w, has } = useW();
  const { info, advanced } = useApp();
  const cur = snap.steps.find((s) => s.id === snap.current) ?? null;
  const at = cur ? snap.steps.indexOf(cur) + 1 : snap.steps.filter((s) => s.status === "done" || s.status === "skipped").length;
  const ended = !snap.active;
  const stageText = snap.stage ? w(`stage.${snap.stage}`, { detail: snap.detail ?? "" }) : null;
  const err = snap.error;
  // Some advice points at the log lines, which only the advanced mode shows: the simple mode has its own wording.
  const errKey = err ? (!advanced && has(`e.${err.code}.simple`) ? `e.${err.code}.simple` : `e.${err.code}`) : "";
  const errParts = err ? w(errKey, { ...Object.fromEntries(err.params), step: w(`step.${err.step}`) }).split("|") : [];
  const details = () => redact(err
    ? [`${BRAND.name} ${info?.version ?? ""} install wizard`, `step: ${err.step}`, `code: ${err.code}`, `params: ${err.params.map(([k, v]) => `${k}=${v}`).join(", ")}`,
      `exit code: ${err.exit_code ?? "-"}`, `log: ${err.log ?? "-"}`, `source: ${snap.source_commit ?? "-"}`, "", ...err.tail].join("\n")
    : "");
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => { if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight; }, [snap.tail]);
  const steps = (
    <ol className="wiz-steps" aria-label={w("wiz.stepper.install")}>
      {snap.steps.map((s) => (
        <li key={s.id} className={`st-${s.status}`}>
          <span className="wiz-st-dot">{stepIcon(s)}</span>
          <span className="grow">{w(`step.${s.id}`)}</span>
          <span className="tiny muted">{s.status === "skipped" ? w("wiz.run.skipped") : s.secs != null && s.status !== "running" ? fmtDuration(s.secs) : ""}</span>
        </li>
      ))}
    </ol>
  );

  return (
    <div className="wiz-body">
      <h1 id="wiz-title" className="display wiz-title">{ended && snap.outcome === "failed" ? w("err.title") : ended && snap.outcome === "cancelled" ? w("wiz.run.stopped") : w(snap.update ? "wiz.run.title.update" : "wiz.run.title")}</h1>
      <div className="wiz-run">
      <div className="stack">
      {(!ended || snap.outcome !== "failed") && (
        <div className="card wiz-overall">
          <div className="wiz-overall-head">
            <span className="wiz-pct display">{Math.floor(snap.overall * 100)} %</span>
            <span className="grow wiz-now">
              {cur && !ended ? <><strong>{w("wiz.run.step", { n: at, total: snap.steps.length })}</strong> {w(`step.${cur.id}`)}</> : null}
            </span>
          </div>
          <Bar value={snap.overall} big />
          <div className="row between small muted">
            <span>{w("wiz.run.elapsed", { t: fmtDuration(snap.elapsed) })}</span>
            {snap.active && snap.eta != null && <span>{w("wiz.run.eta", { t: fmtDuration(snap.eta) })}</span>}
          </div>
          {cur && !ended && !snap.waiting && (
            <div className="wiz-current">
              <div className="row between small"><span className="wiz-stage">{stageText ?? "…"}</span><span className="muted">{snap.step_eta != null ? w("wiz.run.eta.step", { t: fmtDuration(snap.step_eta) }) : ""}</span></div>
              <Bar value={snap.step_fraction} indeterminate={snap.step_fraction == null} />
            </div>
          )}
          {!ended && snap.background.map((b) => (
            <div key={b.id} className="wiz-current wiz-beside">
              <div className="row between small"><span className="wiz-stage">{w("wiz.run.beside", { step: w(`step.${b.id}`) })}{b.stage ? ` · ${w(`stage.${b.stage}`, { detail: b.detail ?? "" })}` : ""}</span></div>
              <Bar value={b.fraction} indeterminate={b.fraction == null} />
            </div>
          ))}
          {snap.waiting && <div className="notice notice-info"><IconInfo width="1.2em" height="1.2em" /><span>{w(snap.update && snap.waiting === "game_running" ? "wiz.run.waiting.game_update" : `wiz.run.waiting.${snap.waiting}`)}</span></div>}
          {snap.active && snap.update && !snap.waiting && <p className="hint wiz-playable">{w(snap.replacing ? "wiz.run.replacing" : "wiz.run.playable")}</p>}
          {snap.warnings.map((x) => <div key={x} className="wiz-check warn"><IconInfo />{w(`wiz.run.warn.${x}`)}</div>)}
          {snap.active && (
            <div className="row wiz-overall-foot">
              <span className="hint grow">{w("wiz.run.leave")}</span>
              <button className="btn btn-ghost btn-small" disabled={snap.cancel_requested} onClick={onCancel}><IconStop /> {snap.cancel_requested ? w("wiz.run.cancelling") : w("wiz.run.cancel")}</button>
            </div>
          )}
        </div>
      )}

      {ended && snap.outcome === "cancelled" && (
        <div className="card card-warn">
          <p>{w("wiz.run.cancelled")}</p>
          <div className="row"><button className="btn" onClick={onRetry}><IconRefresh /> {w("wiz.run.resume")}</button></div>
        </div>
      )}
      {ended && snap.outcome === "reboot" && <div className="card card-warn"><p>{w("wiz.run.reboot")}</p></div>}
      {ended && snap.outcome === "failed" && err && (
        <div className="problem problem-bad wiz-error" role="alert">
          <IconAlert className="problem-ico" />
          <div className="problem-body">
            <p className="small muted">{w("err.step", { step: w(`step.${err.step}`) })}</p>
            <p className="problem-title">{errParts[0]}</p>
            {errParts[1] && <p className="problem-fix">{errParts[1]}</p>}
            <div className="row wrap problem-actions">
              <button className="btn btn-small" onClick={onRetry}><IconRefresh /> {w("err.retry")}</button>
              <CopyButton text={details} label={w("err.copy2")} small />
              {advanced && snap.install_dir && <button className="btn btn-ghost btn-small" onClick={() => void installApi.openLogs(snap.install_dir!)}><IconFolder /> {w("err.logs")}</button>}
              <button className="btn btn-ghost btn-small" onClick={onFiles}>{w("err.files")}</button>
            </div>
            {advanced && err.exit_code != null && <p className="tiny muted mono">{err.step} · {err.code} · exit {err.exit_code}</p>}
            {advanced && err.tail.length > 0 && (<><h3>{w("err.log")}</h3><pre className="log wiz-log">{err.tail.slice(-14).join("\n")}</pre></>)}
          </div>
        </div>
      )}

        {snap.active && <WaitTasks snap={snap} update={snap.update} />}
        {advanced && (
          <details className="advanced wiz-tail" open={!ended}>
            <summary>{w("wiz.run.log")}</summary>
            <pre className="log wiz-log" ref={logRef}>{snap.tail.slice(-40).join("\n") || "…"}</pre>
          </details>
        )}
      </div>
      {steps}
      </div>
    </div>
  );
}
