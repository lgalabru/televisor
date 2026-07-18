import { InstallActions } from "./install-actions";
import { CurvedTerminalWall } from "./curved-terminal-wall";

const catalog = [
  {
    number: "01",
    kicker: "Live",
    title: "The match is already on.",
    command: "televisor play fox sports 1",
    detail: "Fast fuzzy search across 37k+ channels, with local caching.",
    className: "catalog-card card-live",
  },
  {
    number: "02",
    kicker: "Guide",
    title: "Now. Next. Later.",
    command: "televisor guide bbc one",
    detail: "EPG and catch-up playback in the panel’s own timezone.",
    className: "catalog-card card-guide",
  },
  {
    number: "03",
    kicker: "Movies",
    title: "VOD without the wandering.",
    command: "televisor movies dune",
    detail: "Search plots, ratings, cast, duration, and recent additions.",
    className: "catalog-card card-movies",
  },
  {
    number: "04",
    kicker: "Series",
    title: "One line to S01E02.",
    command: "televisor play --series 48173 -s 1 -e 2",
    detail: "Find a show, list its episodes, and jump directly into playback.",
    className: "catalog-card card-series",
  },
];

const terminalScreens = [
  { id: "01", label: "CHANNELS", command: "channels sports", lines: ["42817  SKY SPORTS F1", "18209  F1 TV INTL"], state: "LIVE" },
  { id: "02", label: "GUIDE", command: "guide bbc one", lines: ["18:00  NEWS AT SIX", "18:30  LOCAL NEWS"], state: "SYNC" },
  { id: "03", label: "MOVIES", command: "movies dune", lines: ["93012  DUNE: PART TWO", "RATING  8.7 / 10"], state: "VOD" },
  { id: "04", label: "STATUS", command: "status", lines: ["ACCOUNT  HOME", "CONNECTIONS  1 / 2"], state: "OK" },
  { id: "05", label: "PLAY", command: "play 42817", lines: ["OPENING  VLC", "FORMAT  M3U8"], state: "LIVE" },
  { id: "06", label: "SERIES", command: "series severance", lines: ["48173  SEVERANCE", "2 SEASONS"], state: "VOD" },
  { id: "07", label: "EPISODES", command: "episodes 48173", lines: ["S01E01  GOOD NEWS", "S01E02  HALF LOOP"], state: "READY" },
  { id: "08", label: "FAVORITES", command: "favorites", lines: ["★  BBC ONE", "★  F1 TV"], state: "LOCAL" },
  { id: "09", label: "CATCH-UP", command: "play --at 20:00", lines: ["ARCHIVE FOUND", "DURATION  90 MIN"], state: "REPLAY" },
  { id: "10", label: "MCP", command: "televisor mcp", lines: ["12 TOOLS READY", "STDIO CONNECTED"], state: "LINK" },
  { id: "11", label: "HISTORY", command: "history", lines: ["BBC ONE  18:04", "DUNE  21:19"], state: "LOCAL" },
  { id: "12", label: "PLAYER", command: "open stream", lines: ["PLAYER  MPV", "BUFFER  100%"], state: "PLAY" },
];

function BrandMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 32 32" fill="none">
      <rect x="3" y="4" width="26" height="20" rx="3" stroke="currentColor" strokeWidth="2" />
      <path d="m10 11 4 3.5-4 3.5M17 18h5M12 28h8" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 20 20" fill="none">
      <path d="m4 10 4 4 8-9" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function ArrowUpRight() {
  return (
    <svg aria-hidden="true" viewBox="0 0 20 20" fill="none">
      <path d="M6 14 14 6M8 6h6v6" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export default function Home() {
  return (
    <main id="main-content">
      <a className="skip-link" href="#main-content">Skip to content</a>

      <header className="site-header shell">
        <a className="brand" href="#top" aria-label="televisor.sh home">
          <span className="brand-icon"><BrandMark /></span>
          <span>televisor<span className="brand-dot">.</span>sh</span>
        </a>
        <nav aria-label="Primary navigation">
          <a href="#watch">Watch</a>
          <a href="#agents">For agents</a>
          <a href="#install">Install</a>
        </nav>
        <a className="header-github" href="https://github.com/lgalabru/televisor" target="_blank" rel="noreferrer">
          GitHub <ArrowUpRight />
        </a>
      </header>

      <section className="hero" id="top">
        <div className="hero-copy shell">
          <p className="eyebrow"><span className="signal-dot" /> IPTV / CLI / MCP</p>
          <h1>Every stream. <span>One command.</span></h1>
          <p className="hero-lede">
            Live TV, movies, and series—ready for you or your agent.
          </p>
          <InstallActions />
          <ul className="hero-proof" aria-label="Product highlights">
            <li><CheckIcon /> Xtream + M3U</li>
            <li><CheckIcon /> VLC, mpv, IINA, ffplay</li>
            <li><CheckIcon /> MIT licensed</li>
          </ul>
        </div>

        <CurvedTerminalWall screens={terminalScreens} />
      </section>

      <section className="signal-strip" aria-label="Product capabilities">
        <div className="signal-track shell">
          <span>LIVE TV</span><i>+</i><span>MOVIES</span><i>+</i><span>SERIES</span><i>+</i><span>EPG</span><i>+</i><span>CATCH-UP</span><i>+</i><span>MCP</span>
        </div>
      </section>

      <section className="catalog-section shell" id="watch">
        <div className="section-heading">
          <div>
            <p className="eyebrow muted">THE CATALOG</p>
            <h2>Your provider.<br />Now queryable.</h2>
          </div>
          <p>Keep the content you already have. Lose the channel-number archaeology.</p>
        </div>
        <div className="catalog-grid">
          {catalog.map((item) => (
            <article className={item.className} key={item.kicker}>
              <div className="card-topline"><span>{item.kicker}</span><span>{item.number}</span></div>
              <div className="card-art" aria-hidden="true">
                <span>{item.kicker.slice(0, 1)}</span>
                <i />
              </div>
              <div className="card-content">
                <h3>{item.title}</h3>
                <p>{item.detail}</p>
                <code><span>$</span> {item.command}</code>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="agent-section" id="agents">
        <div className="shell agent-grid">
          <div className="agent-copy">
            <p className="eyebrow"><span className="signal-dot" /> AGENT READY</p>
            <h2>Give your agent<br />the remote.</h2>
            <p>
              Televisor is also an MCP server with 12 focused tools. Ask Claude or Codex what’s on, find the right episode, and launch playback locally.
            </p>
            <div className="agent-stats">
              <div><strong>12</strong><span>MCP tools</span></div>
              <div><strong>0</strong><span>stream URLs exposed</span></div>
              <div><strong>3</strong><span>agent clients configured</span></div>
            </div>
          </div>

          <div className="mcp-panel">
            <div className="mcp-panel-header"><span>MCP handshake</span><span className="mcp-connected"><i /> CONNECTED</span></div>
            <div className="mcp-message user-message">
              <span className="message-label">YOU</span>
              <p>What’s on BBC One now? Play it if it’s the news.</p>
            </div>
            <div className="mcp-tool-call">
              <span className="message-label">TOOL CALL</span>
              <p><b>get_epg</b>({`{ channel: "bbc one" }`})</p>
              <div className="tool-result"><span>NOW</span><strong>BBC News at Six</strong><em>18:00—18:30</em></div>
            </div>
            <div className="mcp-tool-call second-call">
              <span className="message-label">TOOL CALL</span>
              <p><b>play_channel</b>({`{ query: "bbc one" }`})</p>
              <div className="success-line"><CheckIcon /> Playback started in VLC</div>
            </div>
          </div>
        </div>
      </section>

      <section className="privacy-section shell">
        <div className="privacy-number">{"// 01"}</div>
        <div>
          <p className="eyebrow muted">BUILT FOR LOCAL CONTROL</p>
          <h2>Your streams don’t become model context.</h2>
        </div>
        <p>
          Playback launches in your local media player. Credentialed stream URLs are never returned to the model, logs are redacted, and account credentials stay CLI-only.
        </p>
      </section>

      <section className="install-section" id="install">
        <div className="shell install-grid">
          <div>
            <p className="eyebrow">READY WHEN YOU ARE</p>
            <h2>Less browsing.<br />More watching.</h2>
          </div>
          <div className="install-steps">
            <div><span>01</span><code>just install</code><p>Build and install the binary.</p></div>
            <div><span>02</span><code>televisor setup</code><p>Add your provider and agent configs.</p></div>
            <div><span>03</span><code>televisor play</code><p>Search once. Start watching.</p></div>
            <InstallActions />
          </div>
        </div>
      </section>

      <footer className="site-footer shell">
        <a className="brand" href="#top" aria-label="televisor.sh home">
          <span className="brand-icon"><BrandMark /></span>
          <span>televisor<span className="brand-dot">.</span>sh</span>
        </a>
        <p>IPTV from the terminal.</p>
        <div>
          <a href="https://github.com/lgalabru/televisor" target="_blank" rel="noreferrer">GitHub</a>
          <a href="https://github.com/lgalabru/televisor/blob/main/LICENSE" target="_blank" rel="noreferrer">MIT License</a>
        </div>
      </footer>
    </main>
  );
}
