// Play: one obvious button and the state of the game in one sentence (not installed, ready, update available, running),
// the problems in plain words. Without a server the main block is the full offline game; with one, the main block is
// that server (online play), the other servers and the solo game come under it. A short note says what this launcher
// is. The advanced mode adds its tools below.
import { play } from "../sound/engine";
import { lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, toAppError, type LaunchReport, type ModeId, type Preflight, type QuickMode, type UiPrefs } from "../api";
import { useApp } from "../ctx";
import { diagnostic } from "../diagnostic";
import { mark } from "../perf";
import { errorText, useI18n } from "../i18n";
import { refusalText } from "../refusal";
import { IconAlert, IconFriends, IconInstall, IconJoin, IconPlay, IconRelay, IconStop, Mark } from "../icons";
import { Problem, openLogs } from "../ui";
import { OfflineExtras, OtherServers, SoloCard, useOnline } from "./play/Online";
import { populationText } from "../community/population";

// "Game update available" (the wizard's texts stay out of this page's chunk), and the advanced tools.
const GameUpdateNotice = lazy(() => import("../install/UpdateNotice").then((m) => ({ default: m.GameUpdateNotice })));
const AdvancedPlay = lazy(() => import("./play/AdvancedPlay"));

/** Decorative ink blot behind the main block (an original shape, drawn for this launcher). */
export function Blot({ className }: { className?: string }) {
  return (
    <svg className={`blot ${className ?? ""}`} viewBox="0 0 260 200" aria-hidden="true" focusable="false">
      <path fill="currentColor" d="M131 12c27-2 37 23 60 31 24 8 51 9 57 35 6 27-17 41-17 63 0 23 17 43 0 60-17 18-45 4-67 12-23 8-38 30-64 25-26-4-30-30-50-42-21-13-50-14-55-40-5-27 21-41 24-65 3-24-16-49 4-65 19-15 44 1 66-6 16-5 22-25 42-8z" />
      <circle cx="232" cy="22" r="8" fill="currentColor" />
      <circle cx="30" cy="172" r="5" fill="currentColor" />
      <circle cx="248" cy="150" r="3.5" fill="currentColor" />
    </svg>
  );
}

/** Paint drips along the top edge of a block. */
export function Drips() {
  return (
    <svg className="drips" viewBox="0 0 300 42" preserveAspectRatio="none" aria-hidden="true" focusable="false">
      <path fill="currentColor" d="M0 0H300V9C290 9 288 12 286 18S284 34 278 34 272 20 270 14 262 9 250 10 238 10 234 16 232 26 227 26 222 16 220 12 205 8 190 10 170 10 166 15 164 40 158 40 152 17 148 12 125 9 110 10 94 11 92 17 90 28 85 28 80 16 78 12 55 8 40 10 20 10 16 15 14 30 9 30 4 17 0 12Z" />
    </svg>
  );
}

type Pending = { mode: ModeId; kind: string; fields: string[]; detail: string };

export default function Play() {
  const { t, lang } = useI18n();
  const app = useApp();
  const { game, status, go, config, player, setConfig, fail, cliPlay, clearCliPlay, toast, advanced, installing, info, install } = app;
  const online = useOnline();
  const [pre, setPre] = useState<Preflight | null>(null);
  const [report, setReport] = useState<LaunchReport | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [crashClosed, setCrashClosed] = useState<number | null>(null);
  const [args, setArgs] = useState("");
  const quickRef = useRef<HTMLButtonElement>(null);

  const ready = status?.ready ?? false;
  const live = game.state === "running" || game.restarted;
  const settled = game.state === "exited" ? `${game.ended_at}` : game.state;

  useEffect(() => { mark("play:effects"); }, []);
  // The light preflight: the profile is checked against the game's catalogue only when that is already loaded (the
  // Locker or an earlier launch did it); a launch always checks it fully.
  const reload = useCallback(() => { void api.playPreflight(true).then(setPre).catch(() => {}); }, []);
  useEffect(reload, [reload, settled, game.mode, config?.apply_profile, config?.lan_enabled, config?.lan, config?.skip_first_launch, config?.unlock_all, config?.root]);
  useEffect(() => { if (pre) mark("play:preflight"); }, [pre]);
  useEffect(() => { if (status?.ready) mark("play:ready"); }, [status?.ready]);
  // What the game did with the profile at the last launch (its log), once the first screen has its data.
  useEffect(() => {
    if (!status) return;
    let alive = true;
    void api.launchReport(false).then((r) => alive && setReport(r)).catch(() => {});
    return () => { alive = false; };
  }, [status, settled]);

  const saveUi = (patch: Partial<UiPrefs>) => { if (config) void api.setUiPrefs({ ...config.ui, ...patch }).then(setConfig).catch(fail); };
  const isQuick = (m: ModeId): m is QuickMode => m === "original" || m === "range" || m === "hub";
  /** `remember`: this launch becomes the one a `--play` shortcut without a mode would repeat. */
  const launch = useCallback(async (mode: ModeId, ignore = false, untouched = false, remember = true) => {
    try {
      setProblem(null);
      setPending(null);
      await api.launchMode({ mode, ignore_profile: ignore, untouched_save: untouched, extra_args: args });
      if (remember && isQuick(mode) && config && config.ui.last_mode !== mode) saveUi({ last_mode: mode });
    } catch (e) {
      const err = toAppError(e);
      if (err.kind === "profile_incomplete" || err.kind === "profile_invalid") setPending({ mode, kind: err.kind, fields: err.kind === "profile_incomplete" ? err.details : [], detail: err.details[0] ?? "" });
      else setProblem(errorText(err, t));
    }
    reload();
  }, [args, config, reload, t]); // eslint-disable-line react-hooks/exhaustive-deps

  // `--play <mode>` of a desktop shortcut: started as soon as the install status is known, once.
  useEffect(() => {
    if (!cliPlay || !status || !config) return;
    clearCliPlay();
    if (status.ready && !live) void launch(cliPlay, false, cliPlay === "original" && config.ui.full_untouched, false);
  }, [cliPlay, status, config]); // eslint-disable-line react-hooks/exhaustive-deps
  // Enter starts the game: the main button has the focus unless the person already put it somewhere else. Done after
  // the first screen has painted, and only once the install status is known (the button cannot take it before).
  useEffect(() => {
    if (!ready && !live) return;
    const id = requestAnimationFrame(() => {
      if (document.activeElement === document.body || document.activeElement === null) quickRef.current?.focus();
      mark("play:focused");
    });
    return () => cancelAnimationFrame(id);
  }, [ready, live]);

  // Field names inside a sentence: lower case ("il manque coiffure, bas").
  const names = (list: string[]) => list.map((k) => t(`profile.cat.${k}` as "profile.cat.style").toLocaleLowerCase(lang)).join(", ");
  const modeOn = (id: string) => pre?.modes.find((m) => m.id === id)?.enabled !== false;
  const name = player.profile?.name?.trim();
  const diag = (what: string, extra: string[] = []) => () => diagnostic({ info, status, game, lang }, what, extra);

  // ---- the main block: one state, one sentence, one obvious button ---------------------------------------------
  // With a server it is that server: its name, what it says it offers, the player's state there; the game's own state
  // (installing, not installed, running) still takes the sentence and the button, since online play goes through it.
  let tone: "ok" | "live" | "todo" | "busy" = "ok";
  let pulse = false;
  let dim = false;
  let title: string;
  let line: ReactNode;
  let actions;
  const big = (label: string, icon: ReactNode, onClick: () => void) => (
    <button type="button" ref={quickRef} className="btn btn-big btn-play" onClick={() => { play("confirm"); onClick(); }}>{icon} {label}</button>
  );
  // An update of the installed game: that version stays playable until the update puts the new one in place (it waits
  // for the game to be closed for that), except for the few seconds it takes.
  const updating = installing !== null && !!install?.update && ready;
  if (installing !== null && !updating) {
    tone = "busy";
    title = online ? online.server.name : t("play.hero.installing.title");
    const pct = Math.floor(installing * 100);
    line = online ? (installing > 0 ? t("play.on.installing", { pct }) : t("play.on.installing0")) : installing > 0 ? t("play.hero.installing", { pct }) : t("play.hero.installing0");
    actions = <button type="button" className="btn btn-big" onClick={() => go("setup")}><IconInstall /> {t("play.install.follow")}</button>;
  } else if (updating && !live) {
    tone = "busy";
    title = online ? online.server.name : t("play.hero.updating.title");
    const swapping = !!install?.replacing;
    line = swapping ? t("play.hero.updating.swap") : t("play.hero.updating", { pct: Math.floor((installing ?? 0) * 100) });
    actions = (
      <>
        <button type="button" ref={quickRef} className="btn btn-big btn-play" disabled={swapping || !status || !modeOn("original")}
          onClick={() => { play("confirm"); void launch("original", false, !!config?.ui.full_untouched); }}>
          <IconPlay /> {t("quick.play")}
        </button>
        <button type="button" className="btn btn-ghost btn-big" onClick={() => go("setup")}><IconInstall /> {t("play.update.follow")}</button>
      </>
    );
  } else if (status && !ready && !live) {
    tone = "todo";
    title = online ? online.server.name : t("play.hero.missing.title");
    line = online ? t("play.on.missing") : t("play.hero.missing");
    actions = <button type="button" className="btn btn-big" onClick={() => go("setup")}><IconInstall /> {t("play.install")}</button>;
  } else if (live) {
    tone = "live";
    title = online ? online.server.name : t("play.hero.running.title");
    line = updating && install?.waiting === "game_running" ? t("play.hero.updating.wait")
      : game.restarted ? t("play.hero.restarted") : online && game.mode === "match" ? t("play.on.match") : t("play.hero.running");
    actions = (
      <>
        <button type="button" ref={quickRef} className="btn btn-big" onClick={() => void api.gameFocus().then((ok) => { if (!ok) toast({ kind: "info", text: t("quick.nowindow") }); }).catch(fail)}>
          <IconPlay /> {t("quick.back")}
        </button>
        <button type="button" className="btn btn-ghost btn-big" onClick={() => void api.gameStop().catch(fail)}><IconStop /> {t("quick.stop")}</button>
      </>
    );
  } else if (online) {
    title = online.server.name;
    const now = online.live;
    if (now?.kind === "found" || now?.kind === "match") {
      tone = "live";
      line = now.kind === "found" ? t("play.on.found") : t("play.on.live");
      actions = big(t("play.on.match.see"), <IconPlay />, () => go("community"));
    } else if (now?.kind === "room") {
      pulse = true;
      line = now.code ? t("play.on.room", { code: now.code }) : t("play.on.room.nocode");
      actions = big(t("play.on.room.back"), <IconFriends />, () => go("community", "rooms"));
    } else if (now?.kind === "search") {
      pulse = true;
      line = t("play.on.search");
      actions = big(t("play.on.search.see"), <IconPlay />, () => go("community", "queues"));
    } else if (!online.service) {
      line = t("play.on.relay");
      actions = big(t("play.on.play"), <IconPlay />, () => go("community"));
    } else if (online.session?.logged_in === false) {
      dim = true;
      line = t("play.on.out");
      actions = big(t("play.on.login"), <IconJoin />, () => go("community"));
    } else {
      line = online.session ? (online.name ? t("play.on.as", { name: online.name }) : t("play.on.as.anon")) : null;
      actions = big(t("play.on.play"), <IconPlay />, () => go("community"));
    }
  } else {
    title = t("play.title2");
    line = t("play.hero.ready");
    actions = (
      <button type="button" ref={quickRef} className="btn btn-big btn-play" disabled={!status || !modeOn("original")}
        onClick={() => { play("confirm"); void launch("original", false, !!config?.ui.full_untouched); }}>
        <IconPlay /> {t("quick.play")}
      </button>
    );
  }
  const heroPop = online ? online.pop[online.server.id] ?? null : null;
  const solo = (mode: "original" | "range" | "hub") => { play("confirm"); void launch(mode, false, mode === "original" && !!config?.ui.full_untouched); };

  // ---- what may need the player's attention -------------------------------------------------------------------
  const crashed = game.state === "exited" && game.abnormal && crashClosed !== game.ended_at && !live;
  const fb = report?.feedback;
  const rec = report?.record;
  let warn: string | null = null;
  if (!live && ready && !pending) {
    if (pre && pre.profile === "incomplete") warn = t("play.warn.incomplete", { fields: names(pre.missing) });
    else if (pre && pre.profile === "invalid") warn = t("play.pend.invalid", { detail: pre.detail ?? "" });
    // The game's reason names identifiers: the simple mode says it in general (the Locker shows it at the field).
    else if (rec?.profile === "passed" && fb?.state === "failed") warn = advanced ? t("play.warn.refused", { reason: fb.refusal ? refusalText(fb.refusal, t) : fb.reason ?? "" }) : t("play.warn.refused.simple");
    else if (rec?.profile === "passed" && fb?.state === "needs_appearance") warn = t("play.warn.needs", { fields: names(fb.missing.length ? fb.missing : rec.missing) });
  }

  return (
    <section className={`page page-play ${advanced ? "has-adv" : ""} ${online ? "has-server" : ""}`}>
      <div className={`hero-play hero-${tone} ${online ? "hero-online" : ""}`}>
        <Drips />
        <div className="hero-art" aria-hidden="true">
          <Blot />
          <span className="ring r1" /><span className="ring r2" />
          <Mark className="hero-mark" size="100%" />
          {online && <><span className="peer p1" /><span className="peer p2" /><span className="peer p3" /></>}
        </div>
        <div className="hero-copy">
          <div className="eyebrow hero-eyebrow">
            {online ? <>{online.service ? <IconFriends /> : <IconRelay />} {t("play.on.eyebrow")}</> : name ? t("play.hello", { name }) : t("play.hello.anon")}
          </div>
          <h1 className="display hero-title">{title}</h1>
          {online?.description && <p className="hero-desc">{online.description}</p>}
          {online && online.pills.length > 0 && (
            <ul className="hero-feats">{online.pills.map((p) => <li key={p}>{p}</li>)}</ul>
          )}
          {status && line ? (
            <p className={`hero-line ${pulse ? "pulse" : ""} ${dim ? "dim" : ""}`} aria-live="polite"><span className="hero-dot" />{line}</p>
          ) : (
            <span className="skeleton" style={{ width: "18rem", height: "1.2rem" }} />
          )}
        </div>
        <div className="hero-actions">
          {actions}
          {heroPop && <span className="hero-pop"><span className="pop-dot" />{populationText(heroPop, lang, t)}</span>}
        </div>
        {ready && installing === null && <Suspense fallback={null}><GameUpdateNotice /></Suspense>}
      </div>

      {pending && (
        <Problem
          tone="warn"
          title={pending.kind === "profile_incomplete" ? t("play.pend.title", { fields: names(pending.fields) }) : t("play.pend.invalid", { detail: pending.detail })}
          fix={t("play.pend.fix")}
          actions={
            <>
              <button className="btn btn-small" onClick={() => go("profile")}>{t("play.warn.fix")}</button>
              <button className="btn btn-small btn-ghost" disabled={live} onClick={() => void launch(pending.mode, true)}>{t("play.pend.anyway")}</button>
            </>
          }
          onClose={() => setPending(null)}
        />
      )}
      {problem && (
        // The back end's message names files and system errors: in the diagnostic and the advanced details only.
        <Problem title={t("play.fail.title")} fix={t("play.fail.fix")} diag={diag(problem)} logs={openLogs} onClose={() => setProblem(null)}
          details={<p className="tiny muted mono wrapany">{problem}</p>} />
      )}
      {crashed && (
        <Problem
          title={t("play.crash.title")}
          fix={t("play.crash.fix")}
          diag={diag(t("play.crash.title"))}
          logs={openLogs}
          details={<p className="tiny muted mono">{t("play.state.exited.bad", { code: game.exit_code ?? "?" })}{game.log_path ? ` · ${game.log_path}` : ""}</p>}
          onClose={() => setCrashClosed(game.ended_at)}
        />
      )}
      {warn && (
        <p className="play-warn" role="status"><IconAlert /> <span className="grow">{warn}</span> <button className="link" onClick={() => go("profile")}>{t("play.warn.fix")}</button></p>
      )}

      {online ? (
        <>
          <OtherServers online={online} />
          <SoloCard disabled={!ready || live || (installing !== null && !updating) || !!install?.replacing} modeOff={(id) => !modeOn(id)} onLaunch={solo} />
        </>
      ) : (
        <OfflineExtras />
      )}

      {advanced && (
        <Suspense fallback={null}>
          <AdvancedPlay pre={pre} reload={reload} launch={launch} args={args} setArgs={setArgs} />
        </Suspense>
      )}
    </section>
  );
}
