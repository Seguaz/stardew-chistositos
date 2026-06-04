import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import gsap from "gsap";
import hero from "./assets/hero.png";
import logo from "./assets/logo.png";
import michi from "./assets/michi.png";
import {
  Home, LayoutGrid, Settings, Play, Folder, RefreshCw, Minus, X,
  ChevronDown, LogOut, ExternalLink, Server, Newspaper,
} from "lucide-react";

interface CheckResult {
  game_dir: string | null; local_version: string; remote_version: string;
  needs_update: boolean; size_mb: number; smapi: string; game_version: string;
  mods: string[]; error: string | null;
}
interface Progress { phase: string; message: string; percent: number; }
interface ServerStatus { online: boolean; players: number; }
interface Account { username: string; role: string; }
interface ChangeEntry { date: string; title: string; items: string[]; }
interface GamePrefs { lang: string; music: boolean; fullscreen: boolean; }

const LANGS: [string, string][] = [
  ["es", "Español"], ["en", "English"], ["de", "Deutsch"], ["fr", "Français"],
  ["it", "Italiano"], ["pt", "Português"], ["ru", "Русский"], ["ja", "日本語"],
  ["ko", "한국어"], ["zh", "中文"], ["hu", "Magyar"], ["tr", "Türkçe"],
];

type Flow = "checking" | "ready" | "update" | "working" | "launching" | "error";
type View = "home" | "mods" | "settings";

export default function App() {
  const [authChecked, setAuthChecked] = useState(false);
  const [account, setAccount] = useState<Account | null>(null);
  const [authMode, setAuthMode] = useState<"login" | "register">("login");
  const [authUser, setAuthUser] = useState("");
  const [authPass, setAuthPass] = useState("");
  const [authErr, setAuthErr] = useState("");
  const [authBusy, setAuthBusy] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const [flow, setFlow] = useState<Flow>("checking");
  const [view, setView] = useState<View>("home");
  const [check, setCheck] = useState<CheckResult | null>(null);
  const [server, setServer] = useState<ServerStatus | null>(null);
  const [news, setNews] = useState<ChangeEntry[]>([]);
  const [message, setMessage] = useState("Comprobando…");
  const [percent, setPercent] = useState(-1);
  const [error, setError] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [prefs, setPrefs] = useState<GamePrefs>({ lang: "es", music: true, fullscreen: false });

  const brandRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    (async () => {
      try { const a = await invoke<Account | null>("account_me"); if (a) setAccount(a); } catch {}
      setAuthChecked(true);
    })();
  }, []);

  const runCheck = useCallback(async (dir?: string | null) => {
    setFlow("checking"); setMessage("Comprobando actualizaciones…");
    try {
      const res = await invoke<CheckResult>("check_updates", { gameDir: dir ?? null });
      setCheck(res);
      if (res.error) { setError(res.error); setFlow("error"); return; }
      if (!res.game_dir) { setView("settings"); setFlow("error"); setError("No encuentro tu carpeta de Stardew Valley. Indícala en Ajustes."); return; }
      if (res.needs_update) { setFlow("update"); setMessage(`Actualización v${res.remote_version}`); }
      else { setFlow("ready"); setMessage(`Listo · v${res.remote_version}`); }
    } catch (e) { setError(String(e)); setFlow("error"); }
  }, []);

  useEffect(() => { if (account) runCheck(); }, [account, runCheck]);

  useEffect(() => {
    if (!account) return;
    invoke<GamePrefs>("get_prefs").then(setPrefs).catch(() => {});
  }, [account]);

  const savePrefs = (next: GamePrefs) => {
    setPrefs(next);
    invoke("set_prefs", { lang: next.lang, music: next.music, fullscreen: next.fullscreen }).catch(() => {});
  };

  // server status + changelog
  useEffect(() => {
    if (!account) return;
    let alive = true;
    const poll = async () => {
      try { const s = await invoke<ServerStatus>("get_server_status"); if (alive) setServer(s); }
      catch { if (alive) setServer({ online: false, players: 0 }); }
    };
    poll();
    invoke<ChangeEntry[]>("get_changelog").then((c) => { if (alive) setNews(c || []); }).catch(() => {});
    const id = setInterval(poll, 30000);
    return () => { alive = false; clearInterval(id); };
  }, [account]);

  useEffect(() => {
    const un = listen<Progress>("progress", (e) => { setMessage(e.payload.message); setPercent(e.payload.percent); });
    return () => { un.then((f) => f()); };
  }, []);

  // entrance
  useEffect(() => {
    if (!account || view !== "home" || !brandRef.current) return;
    gsap.from(brandRef.current, { opacity: 0, y: 18, duration: 0.6, ease: "power3.out" });
  }, [view, account]);

  useEffect(() => {
    if (!barRef.current || percent < 0) return;
    gsap.to(barRef.current, { width: `${percent}%`, duration: 0.35, ease: "power1.out" });
  }, [percent]);

  const doAuth = async () => {
    setAuthErr(""); setAuthBusy(true);
    try {
      const cmd = authMode === "login" ? "account_login" : "account_register";
      const a = await invoke<Account>(cmd, { username: authUser.trim(), password: authPass });
      setAccount(a); setAuthPass("");
    } catch (e) { setAuthErr(String(e)); } finally { setAuthBusy(false); }
  };
  const doLogout = async () => { try { await invoke("account_logout"); } catch {} setMenuOpen(false); setAccount(null); setView("home"); };
  const openPanel = () => { setMenuOpen(false); invoke("open_url", { url: "https://stardew.seguaz.online/admin" }); };

  const handlePlay = async () => {
    if (!check?.game_dir) { setView("settings"); return; }
    try {
      if (flow === "update") { setFlow("working"); setPercent(-1); setMessage("Preparando…"); await invoke("update_mods", { gameDir: check.game_dir }); }
      setFlow("launching"); setMessage("Iniciando Stardew Valley…"); setPercent(100);
      await invoke("launch_game", { gameDir: check.game_dir });
      setMessage("¡A jugar!");
      setTimeout(() => getCurrentWindow().close(), 1800);
    } catch (e) { setError(String(e)); setFlow("error"); }
  };
  const handleSetPath = async () => {
    const cleaned = await invoke<string | null>("validate_game_dir", { path: pathInput });
    if (cleaned) { setError(null); runCheck(cleaned); setView("home"); } else setError("Esa carpeta no tiene StardewModdingAPI.exe.");
  };
  const handleForceUpdate = async () => {
    if (!check?.game_dir) return;
    try { setView("home"); setFlow("working"); setPercent(-1); setMessage("Descargando pack…"); await invoke("update_mods", { gameDir: check.game_dir }); runCheck(check.game_dir); }
    catch (e) { setError(String(e)); setFlow("error"); }
  };

  const showBar = flow === "working" || flow === "launching";
  const indeterminate = flow === "working" && percent < 0;
  const busy = flow === "checking" || flow === "working" || flow === "launching";
  const playLabel = flow === "update" ? "ACTUALIZAR Y JUGAR" : flow === "working" ? "ACTUALIZANDO" : flow === "launching" ? "INICIANDO" : "JUGAR";
  const playSub = busy && flow !== "checking" ? message
    : flow === "update" ? `Descarga única · ${check?.size_mb ?? "?"} MB`
    : "";
  const isStaff = account && (account.role === "admin" || account.role === "mod");
  const Bg = () => (<><div className="hero-bg" style={{ backgroundImage: `url(${hero})` }} /><div className="hero-overlay" /></>);
  const WinBtns = () => (
    <div className="win-dots">
      <button className="wb" onClick={() => getCurrentWindow().minimize()}><Minus size={16} strokeWidth={3} /></button>
      <button className="wb wb-close" onClick={() => getCurrentWindow().close()}><X size={16} strokeWidth={3} /></button>
    </div>
  );

  if (!authChecked) {
    return (<div className="app"><Bg /><div className="auth"><span className="auth-loading">Cargando…</span></div></div>);
  }

  if (!account) {
    return (
      <div className="app">
        <Bg />
        <header className="topbar" data-tauri-drag-region><WinBtns /></header>
        <div className="auth">
          <div className="auth-card">
            <img className="auth-logo" src={logo} alt="Stardew" />
            <p className="auth-h">CHISTOSITOS · Inicia sesión para jugar</p>
            <div className="seg">
              <button className={authMode === "login" ? "on" : ""} onClick={() => { setAuthMode("login"); setAuthErr(""); }}>Entrar</button>
              <button className={authMode === "register" ? "on" : ""} onClick={() => { setAuthMode("register"); setAuthErr(""); }}>Crear cuenta</button>
            </div>
            <label>Usuario {authMode === "register" && "(será tu personaje)"}</label>
            <input value={authUser} onChange={(e) => setAuthUser(e.target.value)} onKeyDown={(e) => e.key === "Enter" && doAuth()} />
            <label>Contraseña</label>
            <input type="password" value={authPass} onChange={(e) => setAuthPass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && doAuth()} />
            <button className="btn primary block" style={{ marginTop: 18 }} onClick={doAuth} disabled={authBusy || !authUser || !authPass}>
              {authBusy ? "…" : authMode === "login" ? "Entrar" : "Crear y entrar"}
            </button>
            {authErr && <p className="err" style={{ textAlign: "center" }}>⚠ {authErr}</p>}
            <p className="auth-note">{authMode === "register" ? "Tu usuario será tu personaje: entrarás siempre a tu misma cabaña." : "Misma cuenta en el launcher y en el panel."}</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <Bg />

      <header className="topbar" data-tauri-drag-region>
        <div className="profile">
          <button className="profile-btn" onClick={() => setMenuOpen((o) => !o)}>
            <img className="avatar" src={michi} alt="" />
            <div style={{ textAlign: "left" }}>
              <div className="profile-name">{account.username}</div>
              <div className="profile-role">{account.role}</div>
            </div>
            <ChevronDown size={16} />
          </button>
          {menuOpen && (
            <>
              <div style={{ position: "fixed", inset: 0, zIndex: 25 }} onClick={() => setMenuOpen(false)} />
              <div className="profile-menu">
                <button className="menu-item" onClick={() => { setView("settings"); setMenuOpen(false); }}><Settings size={16} /> Ajustes</button>
                {isStaff && <button className="menu-item" onClick={openPanel}><ExternalLink size={16} /> Panel admin</button>}
                <button className="menu-item danger" onClick={doLogout}><LogOut size={16} /> Cerrar sesión</button>
              </div>
            </>
          )}
        </div>
        <WinBtns />
      </header>

      <div className="shell">
        <nav className="sidebar">
          <div className="nav-group">
            <button className={`nav ${view === "home" ? "active" : ""}`} onClick={() => setView("home")} title="Inicio"><Home size={22} /></button>
            <button className={`nav ${view === "mods" ? "active" : ""}`} onClick={() => setView("mods")} title="Mods"><LayoutGrid size={22} /></button>
            <button className={`nav ${view === "settings" ? "active" : ""}`} onClick={() => setView("settings")} title="Ajustes"><Settings size={22} /></button>
          </div>
        </nav>

        <div className="stage">
          {/* HOME */}
          <div className="brand" ref={brandRef}>
            <img className="brand-logo" src={logo} alt="Stardew Valley" />
            <div className="brand-tag">Chistositos · Co-op 24/7</div>
            <p className="brand-sub">Tu granja modeada de siempre. Pulsa jugar y entra directo a tu cabaña.</p>
          </div>

          <div className="cards">
            <div className="card">
              <div className="card-title"><Server /> Estado del servidor</div>
              <div className="status-big">
                <span className={`dot ${server?.online ? "on" : ""}`} />
                {server == null ? "Comprobando…" : server.online ? "En línea" : "Fuera de línea"}
              </div>
              <div className="status-meta">
                <div className="m"><b>{server?.players ?? "—"}</b>jugando ahora</div>
                <div className="m"><b>{check?.mods?.length ?? "—"}</b>mods</div>
                <div className="m"><b>{check?.game_version || "1.6.15"}</b>versión</div>
              </div>
            </div>

            <div className="card">
              <div className="card-title"><Newspaper /> Novedades</div>
              {news.length === 0 && <p className="muted" style={{ fontSize: 13 }}>Sin novedades.</p>}
              {news.map((n, i) => (
                <div className="news-item" key={i}>
                  <div className="news-head"><b>{n.title}</b><span>{n.date}</span></div>
                  <ul>{n.items.map((it, j) => <li key={j}>{it}</li>)}</ul>
                </div>
              ))}
            </div>
          </div>

          <div className="playbar">
            <button className="play-btn" onClick={handlePlay} disabled={busy}><Play fill="currentColor" strokeWidth={0} /></button>
            <div className="play-info">
              <span className="play-label">{playLabel}</span>
              {showBar
                ? <div className={`play-progress ${indeterminate ? "indet" : ""}`}><div className="play-progress-fill" ref={barRef} /></div>
                : playSub ? <span className="play-sub">{playSub}</span> : null}
              {!busy && (server?.players ?? 0) > 0 && (
                <div className="players">
                  {Array.from({ length: Math.min(server!.players, 6) }).map((_, i) => <img key={i} className="pav" src={michi} alt="" />)}
                  <span className="pcount">{server!.players} jugando</span>
                </div>
              )}
            </div>
          </div>

          {/* MODS */}
          {view === "mods" && (
            <div className="glass-panel">
              <div className="panel-h"><h2>Mods incluidos</h2><button className="btn" onClick={() => setView("home")}><X size={16} /> Cerrar</button></div>
              <p className="panel-sub">{check?.mods?.length ?? 0} mods · se sincronizan automáticamente al jugar</p>
              <div className="mods-grid">
                {(check?.mods ?? []).map((m) => <div className="mod-chip" key={m}><span className="mod-dot" />{m}</div>)}
              </div>
            </div>
          )}

          {/* SETTINGS */}
          {view === "settings" && (
            <div className="glass-panel">
              <div className="panel-h"><h2>Ajustes</h2><button className="btn" onClick={() => setView("home")}><X size={16} /> Cerrar</button></div>
              <div className="setting">
                <label>Cuenta</label>
                <div className="field"><span className="v">{account.username} · {account.role}</span></div>
              </div>
              <div className="setting">
                <label>Carpeta de Stardew Valley</label>
                <div className="field"><Folder size={18} /><span className="v">{check?.game_dir ?? "Sin detectar"}</span></div>
              </div>
              <div className="setting">
                <label>Cambiar carpeta (donde está StardewModdingAPI.exe)</label>
                <div className="row">
                  <input className="inp" value={pathInput} placeholder="C:\\...\\steamapps\\common\\Stardew Valley" onChange={(e) => setPathInput(e.target.value)} onKeyDown={(e) => e.key === "Enter" && handleSetPath()} />
                  <button className="btn" onClick={handleSetPath}>Usar</button>
                </div>
                {error && <p className="err">⚠ {error}</p>}
              </div>
              <div className="setting">
                <label>Opciones del juego</label>
                <div className="opt-row">
                  <span>Idioma</span>
                  <select className="inp" style={{ maxWidth: 200 }} value={prefs.lang} onChange={(e) => savePrefs({ ...prefs, lang: e.target.value })}>
                    {LANGS.map(([code, name]) => <option key={code} value={code}>{name}</option>)}
                  </select>
                </div>
                <div className="opt-row">
                  <span>Música</span>
                  <button className={`toggle ${prefs.music ? "on" : ""}`} onClick={() => savePrefs({ ...prefs, music: !prefs.music })}><span className="knob" /></button>
                </div>
                <div className="opt-row">
                  <span>Iniciar en pantalla completa</span>
                  <button className={`toggle ${prefs.fullscreen ? "on" : ""}`} onClick={() => savePrefs({ ...prefs, fullscreen: !prefs.fullscreen })}><span className="knob" /></button>
                </div>
                <p className="panel-sub" style={{ marginTop: 8 }}>Se aplican al iniciar el juego desde el launcher.</p>
              </div>
              <div className="setting">
                <label>Pack de mods</label>
                <button className="btn block" onClick={handleForceUpdate} disabled={!check?.game_dir || busy}><RefreshCw size={16} /> Re-descargar pack ({check?.size_mb ?? "?"} MB)</button>
                <p className="panel-sub" style={{ marginTop: 8 }}>Instalada: {check?.local_version || "ninguna"} · disponible: {check?.remote_version || "?"}</p>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
