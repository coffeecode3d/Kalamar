// The update prompts that follow the player through the launcher: a bar at the top of every page (the launcher's own
// update first, then the game's), the dialog that asks once at the start, and the badge on the rail's Settings entry
// once the player put the update off. Which one shows, where and when is decided by `prompt.ts` (tested); this file
// draws it and calls the same commands as Settings > About (`LauncherUpdate.tsx`): there is no second update state.
// The release notes are the signed manifest's, set as text (never as HTML).
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { installApi, updateApi, type UpdateCheck, type UpdateView } from "../api";
import { useApp, type PageId } from "../ctx";
import { useLive } from "../community/store";
import { useI18n, type Key } from "../i18n";
import { IconAlert, IconCheck, IconRefresh } from "../icons";
import { putOffGameUpdate, startUpdate, usePutOffGameUpdates } from "../install/UpdateNotice";
import { approx, useW } from "../install/texts";
import { updateClass } from "../install/wait";
import { useMatchPhase } from "../match/MatchFlow";
import { Modal } from "../ui";
import { errorKey, mb } from "./LauncherUpdate";
import { dialogDone, markDialogDone, plan, restartBlock, type Bar, type Busy, type GameFacts, type LauncherState, type Plan } from "./prompt";
import { setUpdateView, useLauncherUpdate } from "./store";

// ---- what the launcher is busy with ---------------------------------------------------------------------------------

function useBusy(): Busy {
  const { game, game2, installing, cliPlay } = useApp();
  const phase = useMatchPhase();
  const live = useLive();
  return {
    game: game.state === "running" || game.restarted || game2.state === "running",
    install: installing !== null,
    // a private match connecting, a command-line launch about to start, a search, a found or a running match
    match: phase === "connecting" || cliPlay !== null || live.match != null || live.ticket != null,
  };
}

// ---- the installed game against the launcher's code ---------------------------------------------------------------

/** Asked at start, when an installation ends and when the window gets the focus back (at most every 10 minutes). */
function useGameCheck(): UpdateCheck | null {
  const { installing } = useApp();
  const idle = installing === null;
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const last = useRef(0);
  useEffect(() => {
    let alive = true;
    const ask = () => {
      last.current = Date.now();
      void installApi.updateCheck().then((c) => { if (alive) setCheck(c); }).catch(() => {});
    };
    if (!idle) {
      setCheck(null);
      return () => { alive = false; };
    }
    ask();
    const focus = () => { if (Date.now() - last.current > 600_000) ask(); };
    window.addEventListener("focus", focus);
    return () => { alive = false; window.removeEventListener("focus", focus); };
  }, [idle]);
  return check;
}

function gameFacts(check: UpdateCheck | null, off: ReadonlySet<string>): GameFacts | null {
  if (!check?.available) return null;
  const kind = check.game_from ? "needed" : check.update ? "available" : null;
  return kind ? { kind, putOff: off.has(check.bundled_commit) } : null;
}

// ---- the bar ---------------------------------------------------------------------------------------------------------

type Tone = "info" | "ok" | "warn" | "bad";

interface Row { tone: Tone; title: string; state?: ReactNode; progress?: number; actions?: ReactNode; error?: string }

function useLauncherRow({ v, state, busy, compact }: { v: UpdateView; state: LauncherState; busy: Busy; compact: boolean }): Row {
  const { t } = useI18n();
  const { fail } = useApp();
  const act = (p: Promise<UpdateView>) => void p.then(setUpdateView).catch(fail);
  const a = v.available;
  const err = v.error && v.phase !== "downloading" ? t(errorKey(v.error)) : undefined;
  const btn = (label: string, onClick: () => void, o: { ghost?: boolean; disabled?: boolean; title?: string } = {}) => (
    <button key={label} className={`btn btn-small ${o.ghost ? "btn-ghost" : ""}`} disabled={o.disabled} title={o.title} onClick={onClick}>{label}</button>
  );
  if (state === "notice" && v.notice) {
    const n = v.notice;
    return { tone: n.kind === "updated" ? "ok" : "warn", title: t(`lup.notice.${n.kind}` as Key, { from: n.from, to: n.to }), actions: btn(t("lup.ok"), () => act(updateApi.dismiss()), { ghost: true }) };
  }
  if (!a) return { tone: "info", title: "" };
  const download = () => act(updateApi.download());
  switch (state) {
    case "downloading": {
      const p = v.progress;
      const pct = p && p.total > 0 ? Math.floor((p.done / p.total) * 100) : 0;
      return { tone: "info", title: t("lup.downloading", { version: a.version, pct }), progress: pct, actions: btn(t("lup.cancel"), () => act(updateApi.cancel()), { ghost: true }) };
    }
    case "ready":
    case "blocked": {
      const why = restartBlock(v, busy);
      const installer = a.mode === "installer";
      const reason = why ? t(`lup.blocked.${why}` as Key) : null;
      return {
        tone: "info",
        title: t(installer ? "lup.ready.installer" : "lup.ready", { version: a.version }),
        state: reason ?? t(installer ? "lup.installer.hint" : "lup.restart.hint"),
        error: err,
        actions: btn(t(installer ? "lup.installer" : "lup.restart"), () => act(updateApi.apply()), { disabled: !!why, title: reason ?? undefined }),
      };
    }
    case "elsewhere":
      return {
        tone: "info",
        title: t("lup.ready.elsewhere", { version: a.version }),
        state: t(a.why === "temp" ? "lup.elsewhere.temp" : "lup.elsewhere.readonly"),
        error: err,
        actions: btn(t("lup.elsewhere.pick"), () => void open({ multiple: false, directory: true, title: t("lup.elsewhere.pick") }).then((d) => { if (typeof d === "string" && d) act(updateApi.saveTo(d)); }).catch(fail)),
      };
    case "working":
      return { tone: "info", title: t(a.mode === "installer" ? "lup.starting.installer" : "lup.restarting", { version: a.version }) };
    case "saved":
      return {
        tone: "ok",
        title: t("lup.saved", { version: a.version, path: v.saved_to ?? "" }),
        state: t("lup.saved.hint"),
        actions: <>{btn(t("lup.saved.open"), () => void updateApi.showSaved().catch(fail))}{btn(t("lup.ok"), () => act(updateApi.dismiss()), { ghost: true })}</>,
      };
    case "error":
      return { tone: "bad", title: t("lup.avail", { version: a.version }), error: err, actions: btn(t("lup.retry"), download) };
    default:
      return {
        tone: "info",
        title: t("lup.avail", { version: a.version }),
        state: compact ? undefined : t(a.channel === "beta" ? "lup.avail.beta" : "lup.avail.size", { mb: mb(a.size) }),
        actions: <>{btn(t("lup.go"), download)}{btn(t("lup.later"), () => act(updateApi.later()), { ghost: true })}</>,
      };
  }
}

function useGameRow({ check, kind }: { check: UpdateCheck; kind: "available" | "needed" }): Row {
  const { w } = useW();
  const { go } = useApp();
  const open = () => startUpdate(check.install_dir, go);
  const later = () => putOffGameUpdate(check.bundled_commit);
  const actions = (label: string) => (
    <>
      <button className="btn btn-small" onClick={open}>{label}</button>
      <button className="btn btn-small btn-ghost" onClick={later}>{w("upd.avail.later")}</button>
    </>
  );
  if (kind === "needed") {
    const v = { from: check.game_from ?? "", to: check.game_to };
    return { tone: "warn", title: w("ver.title", v), state: w("ver.text", v), actions: actions(w("ver.go", v)) };
  }
  const u = check.update;
  const cls = u ? updateClass(u, check.eta) : "short";
  return { tone: cls === "full" ? "bad" : cls === "long" ? "warn" : "info", title: w("upd.avail.title"), state: `${w(`upd.class.${cls}`)} · ${w("upd.about", { t: approx(check.eta) })}`, actions: actions(w("upd.avail.go")) };
}

/** Publishes the bar's height, so that the pages sized to the window (Play) leave room for it. */
function useBarHeight(ref: React.RefObject<HTMLDivElement | null>, on: boolean) {
  useLayoutEffect(() => {
    const el = ref.current;
    const root = document.documentElement;
    if (!on || !el) {
      root.style.removeProperty("--ubar-h");
      return;
    }
    const set = () => root.style.setProperty("--ubar-h", `${el.offsetHeight}px`);
    set();
    if (typeof ResizeObserver === "undefined") return () => root.style.removeProperty("--ubar-h");
    const ro = new ResizeObserver(set);
    ro.observe(el);
    return () => { ro.disconnect(); root.style.removeProperty("--ubar-h"); };
  }, [ref, on]);
}

const ICON: Record<Tone, ReactNode> = {
  info: <IconRefresh className="upd-ico" />, ok: <IconCheck className="upd-ico" />, warn: <IconAlert className="upd-ico" />, bad: <IconAlert className="upd-ico" />,
};

function BarView({ row, compact, source }: { row: Row; compact: boolean; source: Bar["source"] }) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  useBarHeight(ref, true);
  return (
    <div ref={ref} className={`ubar ubar-${row.tone} ${compact ? "ubar-compact" : ""}`} role="region" aria-label={t(source === "game" ? "ubar.label.game" : "ubar.label")} data-source={source}>
      {ICON[row.tone]}
      <div className="ubar-text">
        <p className="ubar-title" aria-live={row.progress === undefined ? "polite" : "off"}>{row.title}</p>
        {row.state && <p className={`ubar-state ${compact ? "sr-only" : ""}`}>{row.state}</p>}
        {row.error && <p className="ubar-error" role="alert"><IconAlert /> {row.error}</p>}
      </div>
      {row.progress !== undefined && (
        <div className="ubar-progress" role="progressbar" aria-label={row.title} aria-valuemin={0} aria-valuemax={100} aria-valuenow={row.progress}><span style={{ width: `${row.progress}%` }} /></div>
      )}
      {row.actions && <div className="ubar-actions">{row.actions}</div>}
    </div>
  );
}

/** The launcher's bar. */
function LauncherBar({ v, bar, busy }: { v: UpdateView; bar: Extract<Bar, { source: "launcher" }>; busy: Busy }) {
  const row = useLauncherRow({ v, state: bar.state, busy, compact: bar.compact });
  return <BarView row={row} compact={bar.compact} source="launcher" />;
}

function GameBar({ check, bar }: { check: UpdateCheck; bar: Extract<Bar, { source: "game" }> }) {
  const row = useGameRow({ check, kind: bar.state });
  return <BarView row={row} compact={bar.compact} source="game" />;
}

// ---- the dialog at the start -----------------------------------------------------------------------------------------

function StartDialog({ v, onDone }: { v: UpdateView; onDone: () => void }) {
  const { t } = useI18n();
  const { fail } = useApp();
  const a = v.available;
  if (!a) return null;
  const later = () => { onDone(); void updateApi.later().then(setUpdateView).catch(() => {}); };
  const now = () => { onDone(); void updateApi.download().then(setUpdateView).catch(fail); };
  return (
    <Modal title={t("lup.dialog.title", { version: a.version })} onClose={later} labelledBy="lup-dialog-title">
      <p className="small muted">{t(a.channel === "beta" ? "lup.avail.beta" : "lup.avail.size", { mb: mb(a.size) })}</p>
      <h3 className="lup-dialog-new">{t("lup.dialog.new")}</h3>
      {a.notes ? <pre className="lup-notes-text lup-dialog-notes">{a.notes}</pre> : <p className="small muted">{t("lup.notes.none")}</p>}
      <p className="small muted">{t("lup.dialog.hint")}</p>
      <div className="row">
        <button className="btn btn-ghost" onClick={later}>{t("lup.later")}</button>
        <button className="btn" data-autofocus onClick={now}>{t("lup.dialog.now")}</button>
      </div>
    </Modal>
  );
}

/** How long the launcher must have been idle before the dialog appears, and how often it looks again when another
 *  dialog is open. */
const IDLE_MS = 1500;

function useDialogTurn(want: boolean): boolean {
  const [turn, setTurn] = useState(false);
  useEffect(() => {
    if (!want) {
      setTurn(false);
      return;
    }
    const id = window.setInterval(() => {
      // never over another dialog (the migration notice, a confirmation)
      if (!document.querySelector(".scrim")) setTurn(true);
    }, IDLE_MS);
    return () => window.clearInterval(id);
  }, [want]);
  return want && turn;
}

// ---- the whole --------------------------------------------------------------------------------------------------------

/** Mounted once in the shell. `onBadge` tells the rail what is put off. */
export default function UpdatePrompts({ page, onBadge }: { page: PageId; onBadge: (b: Plan["badge"]) => void }) {
  const v = useLauncherUpdate();
  const busy = useBusy();
  const check = useGameCheck();
  const off = usePutOffGameUpdates();
  const [answered, setAnswered] = useState(dialogDone());
  const p = plan({ launcher: v, game: gameFacts(check, off), page, busy, dialogDone: answered });
  const turn = useDialogTurn(p.dialog);
  useEffect(() => { onBadge(p.badge); }, [p.badge, onBadge]);
  useEffect(() => () => onBadge(null), [onBadge]);
  return (
    <>
      {p.bar?.source === "launcher" && v && <LauncherBar v={v} bar={p.bar} busy={busy} />}
      {p.bar?.source === "game" && check && <GameBar check={check} bar={p.bar} />}
      {turn && v && <StartDialog v={v} onDone={() => { markDialogDone(); setAnswered(true); }} />}
    </>
  );
}
