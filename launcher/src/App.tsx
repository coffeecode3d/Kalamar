import { useAppSounds } from "./sound/events";
import { play } from "./sound/engine";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, installApi, onGame2Status, onGameStatus, onInstallCloseRequested, onInstallProgress, serversApi, toAppError, updateApi, wizardIntent, type AppInfo, type Boot, type ConfigView, type GameStatus, type InstallSnapshot, type InstallStatus, type Migration, type QuickMode, type ServersView } from "./api";
import { lookup } from "./locales";
import { InstallChip } from "./install/InstallChip";
import { BRAND } from "./brand";
import { mark } from "./perf";
import { Ctx, type AppCtx, type Notice, type PageId, type PlayerData } from "./ctx";
import { IconFriends, IconLocker, IconPlay, IconSettings, IconStop, Mark } from "./icons";
import { I18nContext, detectLang, errorText, isLang, makeT, type Key, type Lang } from "./i18n";
import { MatchFlowProvider } from "./match/MatchFlow";
import { NoticeBar, Skeleton } from "./ui";
import { PlayerCard } from "./PlayerCard";
import { MigrationNotice } from "./MigrationNotice";
import Play from "./pages/Play";
import { DiscordPresence } from "./discord/useDiscordPresence";

// Everything but the Play screen is loaded when it is first opened: the first screen carries React, the shell and Play.
const Profile = lazy(() => import("./pages/Profile"));
const Settings = lazy(() => import("./pages/Settings"));
const PrivateMatch = lazy(() => import("./match/PrivateMatch"));
const Wizard = lazy(() => import("./install/Wizard"));
const CloseDuringInstall = lazy(() => import("./install/CloseDuringInstall").then((m) => ({ default: m.CloseDuringInstall })));
// Community protocol client (src/community/): loaded when its page is first opened, never at start-up.
const CommunityPage = lazy(() => import("./community/CommunityPage"));
const CommunityOverlay = lazy(() => import("./community/Overlay"));
// The update bar (every page), the start dialog and the badge: the launcher's own update, then the game's.
const UpdatePrompts = lazy(() => import("./update/UpdateBar"));

/** What a page shows while its code loads (a few milliseconds from the local files). */
function PageLoading() {
  return <section className="page" aria-busy="true"><Skeleton w="14rem" h="2rem" /></section>;
}

/** The first screen's data, put in the page by the backend when it created the window (`__KALAMAR_BOOT__`), if it is
 *  recent: a page that reloads later asks again. The command line part is only honoured once per session. */
function takeEmbeddedBoot(): Boot | null {
  const w = window as unknown as { __KALAMAR_BOOT__?: { at: number; data: Boot } };
  const e = w.__KALAMAR_BOOT__;
  delete w.__KALAMAR_BOOT__;
  if (!e || Date.now() - e.at > 15000) return null;
  try {
    if (sessionStorage.getItem("kalamar.cli")) e.data.cli = { ...e.data.cli, play: null };
    else sessionStorage.setItem("kalamar.cli", "1");
  } catch {
    /* no session storage: the command line is then honoured at each load of the page, which is once */
  }
  return e.data;
}

/** The browser preview (vite --mode mock) opens the page named in its URL; the launcher always opens on Play. */
function firstPage(): { page: PageId; arg: string | null } {
  if (import.meta.env.VITE_KALAMAR_MOCK !== "1") return { page: "play", arg: null };
  const q = new URLSearchParams(location.search);
  const p = q.get("page");
  return { page: p === "profile" || p === "community" || p === "settings" || p === "setup" ? p : "play", arg: q.get("section") };
}

/** What the end of an installation says outside the window (a Windows notification): the install wizard's own clear
 *  sentence for a failure (its simple wording when it has one). */
export function endNotice(s: InstallSnapshot, lang: string): { title: string; body: string } | null {
  const w = (k: string, v?: Record<string, string | number>) => lookup("install", lang, k, v);
  if (s.outcome === "done") return s.update ? { title: w("end.update.title"), body: w("end.update.body") } : { title: w("end.done.title"), body: w("end.done.body") };
  if (s.outcome !== "failed" || !s.error) return null;
  const e = s.error;
  const simple = `e.${e.code}.simple`;
  const key = w(simple) !== simple ? simple : `e.${e.code}`;
  const sentence = w(key, { ...Object.fromEntries(e.params), step: w(`step.${e.step}`) }).split("|")[0].trim();
  return { title: w(s.update ? "end.failed.update.title" : "end.failed.title"), body: sentence };
}

export const IDLE: GameStatus = {
  state: "idle", pid: null, mode: null, started_at: null, ended_at: null, exit_code: null,
  stopped_by_user: false, abnormal: false, log_path: null, stdout_path: null, tail: null, restarted: false,
};

interface NavItem { id: PageId; label: Key; icon: ReactNode; needsServer?: boolean }

/** Four entries: Community only exists once a server is added (the launcher is an offline tool without one). */
const NAV: NavItem[] = [
  { id: "play", label: "nav.play", icon: <IconPlay /> },
  { id: "profile", label: "nav.locker", icon: <IconLocker /> },
  { id: "community", label: "nav.community", icon: <IconFriends />, needsServer: true },
  { id: "settings", label: "nav.settings", icon: <IconSettings /> },
];

export default function App() {
  mark("app:render"); // first call = start of the first render (the mark is recorded once)
  const start = useRef(firstPage()).current;
  const [page, setPage] = useState<PageId>(start.page);
  const [pageArg, setPageArg] = useState<string | null>(start.arg);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [status, setStatus] = useState<InstallStatus | null>(null);
  const [game, setGame] = useState<GameStatus>(IDLE);
  const [game2, setGame2] = useState<GameStatus>(IDLE);
  const [lastRun, setLastRun] = useState<GameStatus | null>(null);
  const [toasts, setToasts] = useState<(Notice & { id: number })[]>([]);
  const [player, setPlayer] = useState<PlayerData>({ profile: null, view: null });
  /** First run: no built game found, the install wizard takes the whole window (no rail). */
  const [firstRun, setFirstRun] = useState(false);
  /** The window was closed during an installation: ask what to do. */
  const [closeAsk, setCloseAsk] = useState(false);
  /** What the player put off on the update bar: a dot on the Settings entry (the section it leads to). */
  const [updBadge, setUpdBadge] = useState<"about" | "installed" | null>(null);
  const [servers, setServers] = useState<ServersView | null>(null);
  const [migration, setMigration] = useState<Migration | null>(null);
  const [cliPlay, setCliPlay] = useState<QuickMode | null>(null);
  /** The first screen's data has arrived (then the window is shown). */
  const [booted, setBooted] = useState(false);
  const [cliInvalid, setCliInvalid] = useState<string[]>([]);
  /** The community page was opened once: its overlay (match found, invitations) stays mounted from then on. */
  const [communityOn, setCommunityOn] = useState(false);
  const [installing, setInstalling] = useState<number | null>(null);
  /** The installation of this session (running or ended), and whether its end was seen on the install screen. */
  const [install, setInstall] = useState<InstallSnapshot | null>(null);
  const [endSeen, setEndSeen] = useState(true);
  const toastId = useRef(0);
  const ended = useRef<string | null>(null);

  useEffect(() => { mark("app:mounted"); }, []);
  useEffect(() => { if (status) mark("app:status-known"); }, [status]);
  useEffect(() => { if (config) mark("app:config-known"); }, [config]);
  useEffect(() => { if (player.profile) mark("app:player-loaded"); }, [player.profile]);

  const lang: Lang = isLang(config?.language) ? config.language : detectLang();
  const t = useMemo(() => makeT(lang), [lang]);
  useAppSounds(config, game);
  useEffect(() => { document.documentElement.lang = lang; }, [lang]);
  // First start (no language saved yet): keep the system's language, when supported, as the saved preference.
  useEffect(() => {
    if (config && !isLang(config.language)) void api.setLanguage(detectLang()).then(setConfig).catch(() => {});
  }, [config]);

  const toast = useCallback((n: Notice) => {
    if (n.kind === "error") play("error");
    const id = ++toastId.current;
    setToasts((l) => [...l.slice(-2), { ...n, id }]);
    if (n.kind !== "error") window.setTimeout(() => setToasts((l) => l.filter((x) => x.id !== id)), 4500);
  }, []);
  const fail = useCallback((e: unknown) => toast({ kind: "error", text: errorText(toAppError(e), t) }), [t, toast]);

  const refresh = useCallback(async () => {
    try {
      const [s, lr] = await Promise.all([api.installStatus(), api.lastRun()]);
      setStatus(s);
      setLastRun(lr);
    } catch (e) {
      fail(e);
    }
  }, [fail]);

  /** Reads the profile and the game catalogue (the rail's splashtag). Only the Locker asks: opening the launcher
   *  does not touch the game data. */
  const reloadPlayer = useCallback(async () => {
    try {
      const [profile, view] = await Promise.all([api.profileLoad(), api.profileCatalogue(false)]);
      setPlayer({ profile, view });
    } catch {
      /* the card then shows what it has */
    }
  }, []);

  const reloadServers = useCallback(async () => {
    try {
      setServers(await serversApi.view());
    } catch {
      setServers(null);
    }
  }, []);

  // One round trip for the first screen (see `boot` in commands.rs); nothing here reads game data.
  useEffect(() => {
    void (async () => {
      try {
        const b = takeEmbeddedBoot() ?? (await api.boot());
        setInfo(b.info);
        setConfig(b.config);
        setStatus(b.status);
        setGame(b.game);
        setGame2(b.game2);
        setLastRun(b.last_run);
        setServers(b.servers);
        setMigration(b.migration);
        setPlayer({ profile: b.profile, view: null });
        if (b.install_active) {
          setInstalling(0);
          void installApi.status().then((s) => { setInstall(s); setInstalling(s.active ? s.overall : null); }).catch(() => {});
        }
        const noGame = !b.status.ready && (b.status.root === null || !b.status.root_exists || b.status.missing.includes("exe"));
        if (b.install_active || noGame) {
          setFirstRun(noGame);
          setPage("setup");
        } else if (b.cli.play && b.game.state !== "running" && !b.game.restarted) {
          setCliPlay(b.cli.play);
        }
        if (b.cli.invalid.length) setCliInvalid(b.cli.invalid);
        // The page is up and has its data: after the launcher's own update, this is the new version's "started fine"
        // (the previous version, waiting hidden, then quits; without it, it puts itself back). A no-op otherwise.
        void updateApi.confirmStart().catch(() => {});
      } catch (e) {
        fail(e);
      } finally {
        setBooted(true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => { if (booted) { mark("app:booted"); requestAnimationFrame(() => mark("app:first-paint")); } }, [booted]);

  useEffect(() => {
    let off: (() => void) | undefined;
    void onGameStatus((s) => {
      setGame(s);
      if (s.state === "exited") void refresh();
    }).then((u) => (off = u));
    return () => off?.();
  }, [refresh]);

  useEffect(() => {
    let off: (() => void) | undefined;
    void onGame2Status(setGame2).then((u) => (off = u));
    return () => off?.();
  }, []);

  useEffect(() => {
    let off: (() => void) | undefined;
    void onInstallCloseRequested(() => setCloseAsk(true)).then((u) => (off = u));
    return () => off?.();
  }, []);

  // An installation keeps running when the wizard is left: the rail and the Play screen say how far it is. Its end is
  // said where the player is: the success (or error) sound, and, when the launcher is in the background, the taskbar
  // button flashes and a Windows notification says it (`notify_attention`: nothing more while the window is in front).
  const langRef = useRef(lang);
  langRef.current = lang;
  const pageRef = useRef(page);
  pageRef.current = page;
  useEffect(() => {
    let off: (() => void) | undefined;
    void onInstallProgress((s) => {
      setInstall(s);
      setInstalling(s.active ? s.overall : null);
      if (s.active) {
        ended.current = null;
        return;
      }
      if (!s.outcome || ended.current === s.outcome) return;
      ended.current = s.outcome;
      if (s.outcome === "done") play("success");
      if (s.outcome === "failed") play("error");
      if (s.outcome === "done" || s.outcome === "failed") {
        setEndSeen(pageRef.current === "setup");
        void refresh();
        const n = endNotice(s, langRef.current);
        if (n) void installApi.attention(n.title, n.body).catch(() => {});
      }
    }).then((u) => (off = u));
    return () => off?.();
  }, [refresh]);
  useEffect(() => { if (page === "setup") setEndSeen(true); }, [page]);

  const clearCliPlay = useCallback(() => setCliPlay(null), []);
  useEffect(() => {
    if (cliInvalid.length) toast({ kind: "info", text: t("quick.cli.bad", { arg: cliInvalid.join(" ") }) });
  }, [cliInvalid, t, toast]);

  const go = useCallback((p: PageId, arg?: string) => { setPage(p); setPageArg(arg ?? null); }, []);
  const advanced = config?.ui.advanced ?? false;
  const setAdvanced = useCallback((on: boolean) => {
    if (config) void api.setUiPrefs({ ...config.ui, advanced: on }).then(setConfig).catch(fail);
  }, [config, fail]);

  const ctx: AppCtx = useMemo(
    () => ({ info, config, status, game, game2, lastRun, player, reloadPlayer, cliPlay, clearCliPlay, servers, reloadServers, refresh, setConfig, go, pageArg, fail, toast, advanced, setAdvanced, installing, install }),
    [info, config, status, game, game2, lastRun, player, reloadPlayer, cliPlay, clearCliPlay, servers, reloadServers, refresh, go, pageArg, fail, toast, advanced, setAdvanced, installing, install],
  );
  /** The rail's installation: while it runs, and once it has ended until its result was seen. */
  const chip = install && (install.active || !endSeen) && page !== "setup"
    ? <InstallChip snap={install} onOpen={() => { wizardIntent.showResult = !install.active; go("setup"); }} />
    : null;

  // The server whose features are shown: with a community service, its page; a relay alone gives the private match.
  const provider = servers?.servers.find((s) => s.id === servers.features.provider_id) ?? null;
  const service = provider?.api ? provider : null;
  const hasServer = (servers?.features.servers ?? 0) > 0;
  useEffect(() => { if (page === "community" && service) setCommunityOn(true); }, [page, service]);
  // The last server was removed while its page was open.
  useEffect(() => { if (page === "community" && servers && !hasServer) setPage("play"); }, [page, servers, hasServer]);

  const running = game.state === "running" || game.restarted;
  const running2 = game2.state === "running";
  // The Discord presence (Settings › Discord): nothing leaves the page while it is off. Its own component, so the live
  // state of the community server it follows re-renders it alone.
  const presence = <DiscordPresence config={config} game={game} page={page} servers={servers} lang={lang} t={t} />;

  const toastList = (
    <div className="toasts" aria-live="polite">
      {toasts.map((x) => (
        <div key={x.id} className="toast" onClick={() => setToasts((l) => l.filter((y) => y.id !== x.id))}>
          <NoticeBar notice={x} />
        </div>
      ))}
    </div>
  );

  if (firstRun && page === "setup") {
    return (
      <I18nContext.Provider value={{ lang, t }}>
        <Ctx.Provider value={{ ...ctx, go: (p, arg) => { setFirstRun(false); go(p, arg); } }}>
          <div className="shell shell-setup">
            <Suspense fallback={null}><UpdatePrompts page="setup" onBadge={setUpdBadge} /></Suspense>
            <main className="main" id="main">
              <Suspense fallback={<PageLoading />}><Wizard firstRun onExit={() => { setFirstRun(false); go("play"); }} /></Suspense>
            </main>
            {presence}
            {toastList}
            {closeAsk && <Suspense fallback={null}><CloseDuringInstall onClose={() => setCloseAsk(false)} /></Suspense>}
          </div>
        </Ctx.Provider>
      </I18nContext.Provider>
    );
  }

  return (
    <I18nContext.Provider value={{ lang, t }}>
      <Ctx.Provider value={ctx}>
        <MatchFlowProvider>
          <div className={`shell ${advanced ? "is-advanced" : ""}`}>
            <aside className="rail">
              <div className="brand">
                <Mark className="brand-mark" size="2.4rem" />
                <div className="brand-name display">{BRAND.name}</div>
              </div>

              <nav aria-label={t("nav.label")} className="nav">
                {NAV.filter((n) => !n.needsServer || hasServer).map((n) => {
                  const on = page === n.id || (n.id === "settings" && page === "setup");
                  // Something put off on the update bar: the Settings entry says so and leads to its section.
                  const dot = n.id === "settings" && updBadge !== null;
                  return (
                    <button key={n.id} className={`nav-item ${on ? "on" : ""}`} data-nav={n.id} onClick={() => go(n.id, dot ? updBadge : undefined)} aria-current={on ? "page" : undefined}>
                      <span className="nav-ico">{n.icon}</span>
                      <span className="nav-label">{t(n.label)}</span>
                      {dot && <><span className="nav-dot" aria-hidden="true" /><span className="sr-only">{t("nav.badge.update")}</span></>}
                    </button>
                  );
                })}
              </nav>

              <div className="rail-foot">
                {chip}
                {(running || running2) && (
                  <div className="live" role="status">
                    <span className="live-dot" />
                    <span className="live-title grow">
                      {running ? t("rail.live") : t("rail.live.p2")}
                      {advanced && running && game.pid ? <span className="live-pid mono"> · PID {game.pid}</span> : null}
                    </span>
                    {running && game.state === "running" && (
                      <button className="icon-btn" title={t("play.stop")} aria-label={t("play.stop")} onClick={() => void api.gameStop().catch(fail)}><IconStop /></button>
                    )}
                  </div>
                )}
                <PlayerCard />
                {advanced && <button className="rail-adv" onClick={() => go("settings", "advanced")}>{t("rail.advanced")}</button>}
              </div>
            </aside>

            <div className="content">
            <Suspense fallback={null}><UpdatePrompts page={page} onBadge={setUpdBadge} /></Suspense>
            <main className="main" id="main">
              <div className="page-anim" key={page}>
                {page === "play" && <Play />}
                <Suspense fallback={<PageLoading />}>
                  {page === "profile" && <Profile notify={(n) => n && toast(n)} />}
                  {page === "community" && service && <CommunityPage server={service.id} />}
                  {page === "community" && !service && hasServer && <PrivateMatch />}
                  {page === "settings" && <Settings notify={(n) => n && toast(n)} />}
                  {page === "setup" && <Wizard reinstall={!!status?.ready} onExit={() => go("play")} />}
                </Suspense>
              </div>
            </main>
            </div>

            <MigrationNotice initial={migration} />
            {presence}
            {(communityOn || service?.background) && service && <Suspense fallback={null}><CommunityOverlay onPage={page === "community"} server={service.id} autoConnect={!!service.background} /></Suspense>}
            {toastList}
            {closeAsk && <Suspense fallback={null}><CloseDuringInstall onClose={() => setCloseAsk(false)} /></Suspense>}
          </div>
        </MatchFlowProvider>
      </Ctx.Provider>
    </I18nContext.Provider>
  );
}
