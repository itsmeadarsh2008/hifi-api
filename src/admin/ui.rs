use axum::response::Html;

pub async fn admin_index() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

const ADMIN_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta name="color-scheme" content="dark">
<title>HiFi API Admin</title>
<style>
/* Original HiFi design system — no frameworks, no external requests. */
* { margin:0; padding:0; box-sizing:border-box; }
:root {
  --bg0:#080b11; --bg1:#0e131c; --bg2:#141b29; --bg3:#1b2334;
  --line:#263049; --line-soft:#1a2233;
  --ink:#edf0f7; --ink-dim:#a3adbf; --ink-faint:#5d6a82;
  --accent:#7c6cf5; --accent-ink:#fff; --accent-soft:rgba(124,108,245,.14);
  --green:#34d399; --red:#f87171; --amber:#fbbf24; --blue:#60a5fa; --purple:#c084fc;
  --r-lg:16px; --r-md:12px; --r-sm:8px;
  --font-ui:-apple-system,BlinkMacSystemFont,'Segoe UI',Inter,Roboto,'Helvetica Neue',Arial,sans-serif;
  --font-mono:ui-monospace,'SF Mono','Cascadia Code','JetBrains Mono',Menlo,Consolas,monospace;
  color-scheme:dark;
}
body { font-family:var(--font-ui); background:radial-gradient(1200px 600px at 80% -10%,rgba(124,108,245,.07),transparent 60%),var(--bg0); color:var(--ink); padding:0; min-height:100vh; font-size:14px; line-height:1.55; }
.shell { display:grid; grid-template-columns:248px minmax(0,1fr); min-height:100vh; }
.side { position:sticky; top:0; height:100vh; overflow-y:auto; background:rgba(14,19,28,.7); backdrop-filter:blur(10px); border-right:1px solid var(--line-soft); padding:26px 18px; display:flex; flex-direction:column; gap:6px; }
.brand { display:flex; align-items:center; gap:11px; padding:2px 8px 18px; }
.brand-mark { width:30px; height:30px; border-radius:9px; background:conic-gradient(from 210deg,var(--accent),var(--blue),var(--green),var(--accent)); box-shadow:0 0 18px rgba(124,108,245,.45); flex-shrink:0; }
.brand-name { font-weight:700; font-size:15px; letter-spacing:-0.2px; }
.brand-sub { font-size:10.5px; color:var(--ink-faint); letter-spacing:0.4px; }
.nav-label { font-size:10px; text-transform:uppercase; letter-spacing:1.1px; color:var(--ink-faint); padding:14px 10px 6px; }
.nav a { display:flex; align-items:center; gap:10px; color:var(--ink-dim); text-decoration:none; font-size:13px; font-weight:500; padding:8px 10px; border-radius:var(--r-sm); border:1px solid transparent; transition:all 0.15s; }
.nav a:hover { color:var(--ink); background:rgba(124,108,245,.08); border-color:rgba(124,108,245,.18); }
.nav a::before { content:''; width:6px; height:6px; border-radius:50%; background:var(--ink-faint); flex-shrink:0; transition:all 0.15s; }
.nav a:hover::before { background:var(--accent); box-shadow:0 0 8px var(--accent); }
.side-foot { margin-top:auto; padding:14px 10px 0; font-size:11px; color:var(--ink-faint); border-top:1px solid var(--line-soft); }
.main { min-width:0; padding:26px 30px 20px; max-width:1060px; }
.topbar { position:sticky; top:0; z-index:50; display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap; margin:-26px -30px 26px; padding:16px 30px; background:rgba(8,11,17,.78); backdrop-filter:blur(12px); -webkit-backdrop-filter:blur(12px); border-bottom:1px solid var(--line-soft); }
.topbar h1 { font-size:19px; font-weight:700; letter-spacing:-0.3px; margin:0; }
.topbar .actions { display:flex; gap:8px; align-items:center; }
.main { min-width:0; padding:26px 30px 20px; max-width:1060px; }
section[id] { scroll-margin-top:18px; }
.sec-head { display:flex; align-items:baseline; justify-content:space-between; margin:2px 0 14px; }
.sec-head h2 { font-size:15px; font-weight:700; letter-spacing:-0.2px; margin:0; }
@media (max-width:900px) {
  .shell { grid-template-columns:1fr; }
  .side { position:static; height:auto; border-right:none; border-bottom:1px solid var(--line-soft); padding:16px; }
  .side .nav { display:flex; overflow-x:auto; gap:2px; padding-bottom:4px; }
  .side .nav a { white-space:nowrap; }
  .nav-label, .side-foot, .brand-sub { display:none; }
  .main { padding:18px 14px; }
}
.footer { text-align:center; color:#5f6f60; font-size:11px; padding:24px 0 12px; }

.stats { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-bottom:24px; }
.stat-card { background:linear-gradient(180deg,var(--bg3) 0%,var(--bg2) 100%); border:1px solid var(--line-soft); border-radius:var(--r-md); padding:16px 20px; transition:border-color 0.2s, transform 0.2s, box-shadow 0.2s; position:relative; overflow:hidden; }
.stat-card::before { content:''; position:absolute; top:0; left:0; right:0; height:2px; background:linear-gradient(90deg,transparent,var(--accent),transparent); opacity:0; transition:opacity 0.25s; }
.stat-card:hover { border-color:var(--line); transform:translateY(-2px); box-shadow:0 12px 32px rgba(0,0,0,0.35); }
.stat-card:hover::before { opacity:1; }
.stat-card .label { font-size:11px; color:var(--ink-dim); text-transform:uppercase; letter-spacing:0.6px; font-weight:600; }
.stat-card .value { font-size:27px; font-weight:750; margin-top:4px; color:var(--ink); letter-spacing:-0.5px; font-variant-numeric:tabular-nums; }

.accounts-grid { display:flex; flex-direction:column; gap:18px; margin-bottom:24px; }
.account-card { background:var(--bg2); border:1px solid var(--line-soft); border-radius:var(--r-lg); overflow:hidden; transition:border-color 0.2s, box-shadow 0.25s, transform 0.2s; }
.account-card:hover { border-color:var(--line); box-shadow:0 12px 36px rgba(0,0,0,0.4); }

.card-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:linear-gradient(180deg,var(--bg3) 0%,var(--bg2) 100%); border-bottom:1px solid var(--line-soft); flex-wrap:wrap; gap:10px; }
.card-header .left { display:flex; align-items:center; gap:10px; min-width:0; }
.acc-num { display:inline-flex; align-items:center; justify-content:center; min-width:24px; height:24px; padding:0 7px; border-radius:8px; background:var(--accent-soft); border:1px solid rgba(124,108,245,.3); color:var(--accent); font-size:11px; font-weight:700; flex-shrink:0; }
.card-header .label { font-weight:650; font-size:14px; color:var(--ink); white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }

.card-body { padding:18px 20px; }
.cred-row { display:flex; align-items:baseline; gap:8px; padding:8px 0; font-size:12px; }
.cred-row:last-child { padding-bottom:0; }
.cred-key { color:var(--ink-dim); min-width:120px; user-select:none; flex-shrink:0; }
.cred-key::after { content:'='; margin-left:4px; color:var(--line); }
.cred-value { color:var(--ink); word-break:break-all; min-width:0; font-family:var(--font-mono); font-size:12px; }
.cred-value.masked { color:var(--blue); }
.cred-value.token { color:var(--purple); font-size:11px; }

.card-footer { display:flex; align-items:center; justify-content:space-between; padding:12px 20px; border-top:1px solid var(--line-soft); background:rgba(0,0,0,0.22); flex-wrap:wrap; gap:10px; }
.card-stats { display:flex; gap:14px; flex-wrap:wrap; }
.card-stat { font-size:11px; color:var(--ink-dim); white-space:nowrap; }
.card-stat strong { color:var(--ink); font-variant-numeric:tabular-nums; }
.test-badge { cursor:pointer; text-decoration:underline; text-decoration-style:dotted; text-underline-offset:2px; }
.test-badge:hover { color:var(--ink); }

.card-actions { display:flex; gap:6px; flex-wrap:wrap; }
.status-dot { display:inline-block; width:10px; height:10px; border-radius:50%; flex-shrink:0; }
.status-ok { background:var(--green); box-shadow:0 0 6px rgba(52,211,153,0.5); }
.status-warn { background:var(--amber); box-shadow:0 0 6px rgba(251,191,36,0.5); }
.status-err { background:var(--red); box-shadow:0 0 6px rgba(248,113,113,0.5); }

.btn { background:var(--bg3); border:1px solid var(--line); color:var(--ink); padding:8px 16px; border-radius:10px; cursor:pointer; font-size:12px; font-weight:600; font-family:var(--font-ui); transition:background 0.15s, border-color 0.15s, transform 0.15s, box-shadow 0.15s; }
.btn:hover { background:#242e44; border-color:#35405c; transform:translateY(-1px); box-shadow:0 6px 16px rgba(0,0,0,0.35); }
.btn:active { transform:translateY(0); box-shadow:none; }
.btn:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
.btn:disabled { opacity:0.55; cursor:wait; transform:none; }
.btn-primary { background:linear-gradient(180deg,#8577f7 0%,#6f5ef2 100%); border-color:rgba(124,108,245,0.6); color:#fff; box-shadow:0 4px 14px rgba(124,108,245,0.3); }
.btn-primary:hover { background:linear-gradient(180deg,#9486f8 0%,#7c6cf5 100%); border-color:var(--accent); box-shadow:0 6px 20px rgba(124,108,245,0.4); }
.btn-danger { border-color:rgba(248,113,113,0.45); color:var(--red); }
.btn-danger:hover { background:var(--red); border-color:var(--red); color:#0b0e14; }
.btn-active { background:var(--accent-soft); border-color:rgba(124,108,245,0.55); color:var(--accent); }

.form-section { background:var(--bg2); border:1px solid var(--line-soft); border-radius:var(--r-lg); padding:28px; margin-bottom:24px; transition:border-color 0.2s, box-shadow 0.2s; }
.form-section:hover { border-color:var(--line); box-shadow:0 12px 36px rgba(0,0,0,0.3); }
.form-section h3 { font-size:16px; font-weight:700; letter-spacing:-0.2px; margin-bottom:6px; color:var(--ink); display:flex; align-items:center; gap:10px; }
.form-section h3::before { content:''; display:inline-block; width:4px; height:18px; border-radius:2px; background:linear-gradient(180deg,var(--accent),var(--blue)); }
.form-section > p { color:var(--ink-dim); }
.form-row { display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-bottom:16px; }
.form-row.full { grid-template-columns:1fr; }
.pw-wrap { position:relative; }
.pw-wrap input { padding-right:38px; }
.form-row { display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-bottom:16px; }
.form-row.full { grid-template-columns:1fr; }
.pw-wrap { position:relative; }
.pw-wrap input { padding-right:38px; }
.pw-toggle { position:absolute; right:6px; top:50%; transform:translateY(-50%); background:none; border:none; color:#8b949e; cursor:pointer; font-size:15px; padding:4px 6px; line-height:1; }
.pw-toggle:hover { color:#f0f6fc; }

.error { color:var(--red); font-size:13px; margin-bottom:10px; padding:10px 14px; background:rgba(248,113,113,0.08); border:1px solid rgba(248,113,113,0.25); border-radius:var(--r-sm); }
.success { color:var(--green); font-size:13px; margin-bottom:10px; padding:10px 14px; background:rgba(52,211,153,0.08); border:1px solid rgba(52,211,153,0.25); border-radius:var(--r-sm); }
.error:empty, .success:empty { display:none; padding:0; margin:0; border:none; }

.overlay { display:none; position:fixed; inset:0; background:rgba(4,6,10,0.7); z-index:100; backdrop-filter:blur(8px); -webkit-backdrop-filter:blur(8px); }
.overlay.open { display:flex; align-items:center; justify-content:center; }
.modal { background:var(--bg2); border:1px solid var(--line); border-radius:var(--r-lg); padding:26px; width:520px; max-width:92vw; max-height:90vh; overflow-y:auto; scrollbar-width:none; animation:modalIn 0.22s cubic-bezier(0.2,0.9,0.3,1.2); box-shadow:0 24px 80px rgba(0,0,0,0.5); }
.modal::-webkit-scrollbar { display:none; }
@keyframes modalIn { from { opacity:0; transform:scale(0.95) translateY(8px); } to { opacity:1; transform:scale(1) translateY(0); } }
.modal h3 { font-size:17px; margin-bottom:20px; color:#f0f6fc; }
.modal .form-group { margin-bottom:16px; }
.modal .modal-actions { display:flex; gap:10px; margin-top:18px; }
.modal .modal-actions .btn { padding:8px 20px; font-size:13px; }

.empty-state { text-align:center; padding:48px 20px; color:#8b949e; }
.empty-state p { font-size:15px; margin-bottom:6px; }
.empty-state .hint { font-size:13px; }

.test-pass { color:var(--green); }
.test-fail { color:var(--red); }
.test-pending { color:var(--amber); }

.status-label { font-size:11px; margin-left:6px; padding:2px 8px; border-radius:20px; font-weight:600; letter-spacing:0.2px; }
.status-label.status-ok { color:var(--green); background:rgba(52,211,153,0.12); }
.status-label.status-err { color:var(--red); background:rgba(248,113,113,0.12); }
.status-label.status-premium { color:var(--green); background:rgba(52,211,153,0.12); }
.status-label.status-preview { color:var(--amber); background:rgba(251,191,36,0.12); }
.status-label.status-muted { color:var(--ink-dim); background:rgba(163,173,191,0.12); }

.test-results-section { background:var(--bg2); border:1px solid var(--line-soft); border-radius:var(--r-md); margin:24px 0; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.test-results-section:hover { border-color:var(--line); box-shadow:0 12px 36px rgba(0,0,0,0.3); }
.test-results-section .test-results-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:rgba(0,0,0,0.2); border-bottom:1px solid var(--line-soft); }
.test-results-section .test-results-header h3 { font-size:14px; color:#f0f6fc; font-weight:600; }
.test-results-section .test-results-header .test-summary { font-size:11px; color:#8b949e; }
.test-results-section .test-results-body { padding:4px; }
.test-result-row { display:flex; align-items:center; gap:10px; padding:10px 16px; border-bottom:1px solid #21262d; cursor:pointer; transition:background 0.12s; font-size:12px; }
.test-result-row:hover { background:#1c2128; }
.test-result-row:last-child { border-bottom:none; }
.test-result-row .result-label { flex:1; color:#c9d1d9; font-weight:500; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.test-result-row .result-status { min-width:44px; font-weight:600; }
.test-result-row .result-http { min-width:34px; color:#8b949e; }
.test-result-row .result-ms { min-width:60px; color:#8b949e; text-align:right; }
.test-result-row .result-premium { min-width:80px; color:#8b949e; font-size:11px; }
.test-result-row .result-token { min-width:90px; color:#8b949e; font-size:11px; }

.json-key { color:#79c0ff; }
.json-string { color:#a5d6ff; }
.json-number { color:#79c0ff; }
.json-boolean { color:#ff7b72; }
.json-null { color:#ff7b72; }
.json-bracket { color:#c9d1d9; }

@media (max-width:768px) {
  body { padding:12px; }
  .stats { grid-template-columns:repeat(2,1fr); gap:10px; }
  .card-header { flex-direction:column; align-items:stretch; }
  .card-footer { flex-direction:column; align-items:stretch; gap:12px; }
  .card-stats { gap:10px; }
  .cred-row { flex-direction:column; gap:2px; padding:6px 0; }
  .cred-key { min-width:0; }
  .cred-key::after { content:':'; }
  .form-row { grid-template-columns:1fr; gap:14px; }
  .form-section { padding:20px; }
  .header h1 { font-size:18px; }
  .test-results-section { padding:16px; overflow-x:auto; }
  .test-result-row { min-width:500px; }
}

@media (max-width:480px) {
  .stats { grid-template-columns:1fr; }
  .modal { padding:20px; max-width:96vw; }
}

@keyframes highlightPulse { 0%,100% { border-color:#30363d; } 50% { border-color:#58a6ff; box-shadow:0 0 20px rgba(88,166,255,0.15); } }
.form-highlight { animation:highlightPulse 1.5s ease; }

.terminal { background:#060a08; border:1px solid #1e3a26; border-radius:var(--r-md); overflow:hidden; margin-bottom:24px; box-shadow:0 0 32px rgba(52,211,153,0.06); }
.term-bar { display:flex; align-items:center; gap:10px; padding:9px 14px; background:#0b120c; border-bottom:1px solid #1d3a24; }
.term-dots { display:flex; gap:6px; }
.term-dots i { width:10px; height:10px; border-radius:50%; background:#2a3b2d; }
.term-dots i:nth-child(1) { background:#f85149; }
.term-dots i:nth-child(2) { background:#d29922; }
.term-dots i:nth-child(3) { background:#3fb950; }
.term-title { font-size:11px; color:#7d8a7e; font-family:monospace; flex:1; }
.term-live { font-size:10px; color:#3fb950; font-family:monospace; letter-spacing:1px; animation:termBlink 2s infinite; }
@keyframes termBlink { 0%,100% { opacity:1; } 50% { opacity:0.35; } }
.term-meta { display:flex; gap:16px; flex-wrap:wrap; padding:9px 14px; border-bottom:1px solid #142114; font-family:monospace; font-size:11px; color:#5f6f60; }
.term-meta strong { color:#9fe8b4; font-weight:600; }
.term-meta .card-stat { font-size:11px; }
.term-body { font-family:'SF Mono','Fira Code',Menlo,Consolas,monospace; font-size:12px; line-height:1.75; padding:12px 14px; height:280px; overflow-y:auto; color:#c9e8d2; scrollbar-width:thin; scrollbar-color:#1d3a24 transparent; }
.term-body::-webkit-scrollbar { width:8px; }
.term-body::-webkit-scrollbar-thumb { background:#1d3a24; border-radius:4px; }
.term-line { white-space:nowrap; }
.term-time { color:#4a5a4c; }
.term-method { font-weight:700; }
.m-GET { color:#3fb950; }
.m-POST { color:#58a6ff; }
.m-PUT { color:#d29922; }
.m-PATCH { color:#d2a8ff; }
.m-DELETE { color:#f85149; }
.term-path { color:#e6f5ea; }
.term-id { color:#d2a8ff; }
.term-dim { color:#5f6f60; }
.term-cursor { display:inline-block; width:8px; height:14px; background:#3fb950; vertical-align:-2px; animation:termBlink 1.1s infinite; }
@media (max-width:768px) { .term-body { height:220px; font-size:11px; } }
</style>
</head>
<body>
<div class="shell">
<aside class="side">
<div class="brand"><span class="brand-mark"></span><span><span class="brand-name">HiFi Admin</span><br><span class="brand-sub" id="version">v2.10</span></span></div>
<div class="nav-label">Monitor</div>
<nav class="nav">
<a href="#sec-overview">Overview</a>
<a href="#sec-requests">Requests</a>
<a href="#sec-accounts">Accounts</a>
</nav>
<div class="nav-label">Manage</div>
<nav class="nav">
<a href="#sec-add">Add account</a>
<a href="#sec-import">Import / Export</a>
<a href="#sec-settings">Settings</a>
<a href="#sec-proxies">Proxies</a>
<a href="#sec-alerts">Alerts</a>
<a href="#sec-keys">API keys</a>
<a href="#sec-backup">Backup</a>
<a href="#sec-cache">Cache</a>
<a href="#sec-tests">Tests</a>
</nav>
<div class="side-foot"><button class="btn btn-primary" onclick="testAll()" id="testAllBtn" style="width:100%">Test All</button></div>
</aside>
<main class="main">
<div id="error" class="error"></div>
<div id="success" class="success"></div>
<section id="sec-overview" aria-label="Overview"><div id="stats" class="stats"></div></section>
<section id="sec-requests" aria-label="Requests"><div class="terminal">
<div class="term-bar"><span class="term-dots"><i></i><i></i><i></i></span><span class="term-title">hifi-api — live request log</span><span class="term-live" id="term-live">● LIVE</span></div>
<div class="term-meta">
<span>Total <strong id="rq-total">—</strong></span>
<span>Errors <strong id="rq-errors">—</strong></span>
<span title="4xx except 429: bad input, auth">Client <strong id="rq-user-errors">—</strong></span>
<span title="429/5xx: throttled or Tidal failing">Upstream <strong id="rq-upstream-errors">—</strong></span>
<span>p50 <strong id="rq-p50">—</strong></span>
<span>p95 <strong id="rq-p95">—</strong></span>
<span id="rq-endpoints"></span>
<span id="rq-statuses" style="color:#f0883e"></span>
<span id="rq-err-endpoints" style="color:#f85149"></span>
<span id="rq-slowest" style="color:#d29922"></span>
<span id="rq-tracks" style="color:#d2a8ff"></span>
<span style="margin-left:auto"><label style="font-size:11px;color:#8b949e">filter <select id="rq-filter" onchange="loadRequestLog()" style="background:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:4px;font-size:11px"><option value="all">all</option><option value="errors">errors</option><option value="slow">slow</option></select></label></span>
</div>
<div id="rq-recent" class="term-body"></div>
</div></section>
<section id="sec-accounts" aria-label="Accounts"><div class="sec-head"><h2>Accounts</h2></div><div id="accounts-container" class="accounts-grid"></div></section>
<div class="form-section" id="sec-add">
<h3>Add Account</h3>
<div class="form-row">
<div class="form-group"><label>Label</label><input type="text" id="new-label" placeholder="My Account"></div>
<div class="form-group"><label>User ID (optional)</label><input type="text" id="new-user-id" placeholder="208921067"></div>
</div>
<div class="form-row full">
<div class="form-group"><label>Client ID</label><input type="text" id="new-client-id" placeholder="client_id"></div>
</div>
<div class="form-row">
<div class="form-group"><label>Client Secret</label><div class="pw-wrap"><input type="password" id="new-client-secret" placeholder="client_secret"><button type="button" class="pw-toggle" onclick="togglePw('new-client-secret', this)" title="Show/hide">&#128065;</button></div></div>
<div class="form-group"><label>Refresh Token</label><div class="pw-wrap"><input type="password" id="new-refresh-token" placeholder="refresh_token"><button type="button" class="pw-toggle" onclick="togglePw('new-refresh-token', this)" title="Show/hide">&#128065;</button></div></div>
</div>
<button class="btn btn-primary" onclick="addAccount()">Add Account</button>
<button class="btn" onclick="startOAuth()" id="oauthBtn" style="margin-left:8px">Add via OAuth</button>
<label style="display:inline-flex;align-items:center;gap:6px;margin-left:12px;font-size:12px;color:#8b949e"><input type="checkbox" id="new-catalog" style="width:auto"> Catalog-only (metadata, never playback)</label>
</div>

<div class="form-section" id="sec-import">
<h3>Import / Export</h3>
<p style="font-size:12px;color:#8b949e;margin-bottom:12px">Backup or restore all Tidal credentials as JSON. Import skips duplicates by refresh_token.</p>
<div style="display:flex;gap:8px;flex-wrap:wrap">
<button class="btn" onclick="exportCredentials()">Export credentials.json</button>
<button class="btn" onclick="document.getElementById('importFile').click()">Import credentials.json</button>
<input type="file" id="importFile" accept=".json,application/json" style="display:none" onchange="importCredentials(event)">
</div>
<div id="importResult" style="font-size:12px;margin-top:10px;color:#8b949e"></div>
</div>

<div class="form-section" id="sec-settings">
<h3>Settings</h3>
<div class="form-row">
<div class="form-group"><label>Atmos default</label><select id="rl-atmos" style="width:100%;background:#0d1117;border:1px solid #30363d;color:#c9d1d9;padding:10px 12px;border-radius:6px;font-size:13px"><option value="off">Off (FLAC first)</option><option value="prefer">Prefer (Atmos first)</option></select></div>
<div class="form-group"><label style="display:flex;align-items:center;gap:8px;text-transform:none;font-size:13px;color:#c9d1d9"><input type="checkbox" id="rl-autoheal" style="width:auto"> Auto-heal system-disabled accounts</label></div>
</div>
<p style="font-size:11px;color:#8b949e;margin-bottom:16px">No request throttling is applied — all requests go straight to Tidal with account failover.</p>
<button class="btn btn-primary" onclick="saveSettings()">Save Settings</button>
</div>

<div class="form-section" id="sec-proxies">
<h3>Proxies</h3>
<div class="card-stats" style="margin-bottom:12px">
<span class="card-stat">Status <strong id="px-status">—</strong></span>
<span class="card-stat">Current <strong id="px-current">—</strong></span>
<span class="card-stat">Pool <strong id="px-pool">—</strong></span>
<span class="card-stat">Fails <strong id="px-fails">—</strong></span>
</div>
<p style="font-size:11px;color:#8b949e;margin-bottom:0">Optional. Set <span style="font-family:monospace">USE_PROXIES=true</span> + <span style="font-family:monospace">PROXIES_FILE</span> and restart to route Tidal traffic through rotating proxies. Without it, everything goes direct.</p>
</div>

<div class="form-section" id="sec-alerts">
<h3>Alerts</h3>
<div class="card-stats" style="margin-bottom:12px">
<span class="card-stat">Discord <strong id="al-discord">—</strong></span>
</div>
<p style="font-size:11px;color:#8b949e;margin-bottom:12px">Notifies on account 403 (suspension risk) and all-accounts-down. Set <span style="font-family:monospace">DISCORD_WEBHOOK_URL</span> and restart to enable.</p>
<button class="btn" onclick="testAlert()" id="alertTestBtn">Send Test Alert</button>
<button class="btn" onclick="sendReport('status')" id="reportStatusBtn" style="margin-left:8px">Send Status</button>
<button class="btn" onclick="sendReport('accounts')" id="reportAccountsBtn" style="margin-left:8px">Send Accounts</button>
</div>

<div class="form-section" id="sec-keys">
<h3>API Keys</h3>
<p style="font-size:11px;color:#8b949e;margin-bottom:12px">While no key exists the API stays open. Creating the first key locks all public routes behind <span style="font-family:monospace">X-API-Key</span> (or owner <span style="font-family:monospace">X-Admin-Key</span>). Quota 0 = unlimited.</p>
<div class="form-row">
<div class="form-group"><label>Label</label><input type="text" id="new-key-label" placeholder="My app"></div>
<div class="form-group"><label>Quota (requests, 0 = unlimited)</label><input type="number" id="new-key-quota" min="0" placeholder="0"></div>
</div>
<button class="btn btn-primary" onclick="addApiKey()">Create Key</button>
<div id="keyResult" style="font-size:12px;margin-top:10px;color:#3fb950;word-break:break-all"></div>
<div id="keys-container" style="margin-top:12px"></div>
</div>

<div class="form-section" id="sec-backup">
<h3>Backup / Restore</h3>
<p style="font-size:11px;color:#8b949e;margin-bottom:12px">Download a snapshot of the database (accounts, keys, settings), or restore from one. Restore reloads everything live — no restart needed.</p>
<div style="display:flex;gap:8px;flex-wrap:wrap">
<button class="btn" onclick="downloadBackup()">Download Backup</button>
<button class="btn" onclick="document.getElementById('restoreFile').click()">Restore from File</button>
<input type="file" id="restoreFile" accept=".db,.sqlite,.sqlite3,application/x-sqlite3" style="display:none" onchange="restoreBackup(event)">
</div>
<div id="restoreResult" style="font-size:12px;margin-top:10px;color:#8b949e"></div>
</div>

<div class="form-section" id="sec-cache">
<h3>Cache</h3>
<div class="card-stats" style="margin-bottom:12px">
<span class="card-stat">Cache hits <strong id="cc-hits">—</strong></span>
<span class="card-stat">Misses <strong id="cc-misses">—</strong></span>
<span class="card-stat">Hit rate <strong id="cc-hitrate">—</strong></span>
<span class="card-stat">Stale <strong id="cc-stale">—</strong></span>
<span class="card-stat">Negative <strong id="cc-negative">—</strong></span>
</div>
<button class="btn" onclick="clearCache()" id="clearCacheBtn">Clear Cache</button>
</div>

<section id="sec-tests" aria-label="Tests"><div class="test-results-section" id="testResultsSection" style="display:none">
<div class="test-results-header">
<h3>Test Results</h3>
<div class="test-summary" id="testSummary"></div>
</div>
<div class="test-results-body" id="testResultsList"></div>
</div>
</section>
<footer class="footer">HiFi API admin · served locally, no external requests</footer>
</main>
</div>
<div id="oauthOverlay" class="overlay" onclick="if(event.target===this)closeOAuth()">
<div class="modal">
<h3>Authorize via Tidal</h3>
<p style="margin-bottom:16px;color:#8b949e;font-size:14px">Open this URL in your browser, log into Tidal, and authorize the app.</p>
<div style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:16px;word-break:break-all;font-size:13px;font-family:monospace;color:#58a6ff;margin-bottom:16px" id="oauthUrl">—</div>
<button class="btn" onclick="copyOAuthUrl()" id="copyOAuthBtn" style="margin-right:8px">Copy URL</button>
<button class="btn" onclick="openOAuthUrl()" id="openOAuthBtn">Open</button>
<div class="form-group" style="margin-top:16px"><label>Label (optional — applied when authorization completes)</label><input type="text" id="oauth-modal-label" placeholder="My Tidal" oninput="updateOAuthLabel()"></div>
<p style="margin-top:16px;color:#8b949e;font-size:13px" id="oauthStatus">Waiting for authorization...</p>
<div class="modal-actions">
<button class="btn" onclick="closeOAuth()">Cancel</button>
</div>
</div>
</div>

<div id="editOverlay" class="overlay" onclick="if(event.target===this)closeEdit()">
<div class="modal">
<h3 id="editTitle">Edit Account</h3>
<div class="form-group"><label>Label</label><input type="text" id="ed-label"></div>
<div class="form-group"><label>User ID</label><input type="text" id="ed-user-id"></div>
<div class="form-group"><label>Client ID</label><input type="text" id="ed-client-id"></div>
<div class="form-group"><label>Client Secret</label><div class="pw-wrap"><input type="password" id="ed-client-secret"><button type="button" class="pw-toggle" onclick="togglePw('ed-client-secret', this)" title="Show/hide">&#128065;</button></div></div>
<div class="form-group"><label>Refresh Token</label><div class="pw-wrap"><input type="password" id="ed-refresh-token"><button type="button" class="pw-toggle" onclick="togglePw('ed-refresh-token', this)" title="Show/hide">&#128065;</button></div></div>
<div class="modal-actions">
<button class="btn btn-primary" onclick="saveEdit()">Save</button>
<button class="btn" onclick="closeEdit()">Cancel</button>
</div>
</div>
</div>

<div id="testOverlay" class="overlay" onclick="if(event.target===this)closeTestDetails()">
<div class="modal" style="width:680px">
<h3 id="testTitle">Test Results</h3>
<div id="testDetailsContent" style="font-size:12px;line-height:1.6;max-height:60vh;overflow-y:auto;scrollbar-width:none"></div>
<div class="modal-actions">
<button class="btn" onclick="closeTestDetails()">Close</button>
</div>
</div>
</div>

<script>
var API = window.location.origin;
var adminKey = localStorage.getItem('admin_key') || '';
var editId = null;

function headers() {
    return { 'Content-Type': 'application/json', ...(adminKey ? { 'X-Admin-Key': adminKey } : {}) };
}

function setKey() {
    var k = prompt('Enter admin key:', adminKey);
    if (k) { adminKey = k; localStorage.setItem('admin_key', k); fetchData(); }
}
if (!adminKey) setKey();

function relAgo(ts) {
    if (!ts || ts <= 0) return 'never';
    var s = Math.floor(Date.now() / 1000) - ts;
    if (s < 90) return 'just now';
    var m = Math.floor(s / 60);
    if (m < 60) return m + 'm ago';
    var h = Math.floor(m / 60);
    if (h < 48) return h + 'h ago';
    return Math.floor(h / 24) + 'd ago';
}

function timeStr(ts) {
    if (!ts || ts === 0) return 'No expiry / needs refresh';
    var d = new Date(ts * 1000);
    var diff = d - new Date();
    if (diff < 0) {
        var ago = Math.floor(-diff / 60000);
        if (ago < 60) return 'expired ' + ago + 'm ago';
        return 'expired ' + Math.floor(ago / 60) + 'h ' + (ago % 60) + 'm ago';
    }
    var mins = Math.floor(diff / 60000);
    if (mins < 60) return mins + 'm';
    return Math.floor(mins / 60) + 'h ' + (mins % 60) + 'm';
}

function playbackCard(pb) {
    pb = pb || {};
    var active = pb.active != null ? pb.active : '—';
    var pool = pb.pool_size != null ? pb.pool_size : '—';
    return '<div class="stat-card"><div class="label">Playback live</div><div class="value">' + active + '/' + pool + '</div></div>';
}

function catalogCard(cat) {
    cat = cat || {};
    var mode = cat.mode || 'pool';
    var label = mode === 'static_token' ? 'Static token' : (mode === 'account' ? esc(cat.label || 'Catalog') : 'Pool');
    var color = mode === 'pool' ? '#8b949e' : '#d2a8ff';
    return '<div class="stat-card"><div class="label">Catalog</div><div class="value" style="font-size:18px;color:' + color + '">' + label + '</div></div>';
}

function redisCard(redis) {    redis = redis || {};
    if (!redis.configured) {
        return '<div class="stat-card"><div class="label">Redis Sync</div><div class="value" style="font-size:18px;color:#8b949e">Single-host</div></div>';
    }
    var ep = redis.endpoint ? '<div style="font-size:11px;color:#8b949e;margin-top:4px;word-break:break-all">' + esc(redis.endpoint) + '</div>' : '';
    if (redis.status === 'ok') {
        return '<div class="stat-card" style="border-color:#3fb950"><div class="label">Redis Sync</div><div class="value" style="color:#3fb950;font-size:18px">SYNCED</div>' + ep + '</div>';
    }
    return '<div class="stat-card" style="border-color:#f85149"><div class="label">Redis Sync</div><div class="value" style="color:#f85149;font-size:18px">DOWN</div>' + ep + '</div>';
}

var _testResults = {};
var _testCacheTs = 0;

async function testAll() {
    var now = Math.floor(Date.now() / 1000);
    if (now - _testCacheTs < 30 && Object.keys(_testResults).length > 0) {
        renderTestResults(_testResults);
        return;
    }
    var btn = document.getElementById('testAllBtn');
    btn.textContent = 'Testing...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/accounts/test-all', { method: 'POST', headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Test failed: ' + res.status; return; }
        var data = await res.json();
        _testResults = {};
        for (var r of data.results) { _testResults[r.id] = r; }
        _testCacheTs = now;
        renderTestResults(_testResults);
        fetchData();
    } catch(e) {
        document.getElementById('error').textContent = 'Test error: ' + e.message;
    } finally {
        btn.textContent = 'Test All';
        btn.disabled = false;
    }
}

function renderTestResults(results) {
    var ids = Object.keys(results);
    var section = document.getElementById('testResultsSection');
    if (ids.length === 0) { section.style.display = 'none'; return; }
    section.style.display = 'block';
    var pass = 0, fail = 0;
    for (var id in results) { if (results[id].ok) pass++; else fail++; }
    document.getElementById('testSummary').textContent = pass + ' passed, ' + fail + ' failed — click a row for details';
    var html = '';
    for (var i = 0; i < ids.length; i++) {
        var id = ids[i];
        var r = results[id];
        var acc = window._accounts.find(function(x) { return x.id === id; });
        var label = acc ? esc(acc.label || id.slice(0, 8)) : id.slice(0, 8);
        var statusClass = r.ok ? 'test-pass' : 'test-fail';
        var statusText = r.ok ? 'PASS' : 'FAIL';
        var msText = r.ms ? r.ms + 'ms' : '-';
        var httpText = r.status_code || '-';
        var tokenStr = r.token_expires_at ? timeStr(r.token_expires_at) : '-';
        var premText = r.premium ? esc(r.premium) : '-';
        html += '<div class="test-result-row" onclick="showTestDetails(\'' + id + '\')">';
        html += '<span class="result-label">' + label + '</span>';
        html += '<span class="result-status ' + statusClass + '">' + statusText + '</span>';
        html += '<span class="result-http">' + httpText + '</span>';
        html += '<span class="result-ms">' + msText + '</span>';
        html += '<span class="result-premium">' + premText + '</span>';
        html += '<span class="result-token">' + tokenStr + '</span>';
        html += '</div>';
        var badge = document.getElementById('test-' + id);
        if (badge) {
            badge.className = r.ok ? 'card-stat test-pass' : 'card-stat test-fail';
            badge.innerHTML = 'Test <strong>' + (r.ok ? 'OK ' + r.ms + 'ms' : 'FAIL ' + (r.error || '')) + '</strong>';
        }
    }
    document.getElementById('testResultsList').innerHTML = html;
}

function formatJsonString(str) {
    var indent = 0, result = '', inStr = false;
    for (var i = 0; i < str.length; i++) {
        var ch = str[i];
        if (inStr) {
            result += ch;
            if (ch === '\\' && i + 1 < str.length) { result += str[++i]; }
            else if (ch === '"') { inStr = false; }
            continue;
        }
        if (ch === '"') { inStr = true; result += ch; continue; }
        if (ch === '{' || ch === '[') {
            indent++;
            result += ch + '\n' + '  '.repeat(indent);
            continue;
        }
        if (ch === '}' || ch === ']') {
            indent = Math.max(0, indent - 1);
            result += '\n' + '  '.repeat(indent) + ch;
            continue;
        }
        if (ch === ',') { result += ch + '\n' + '  '.repeat(indent); continue; }
        if (ch === ':') { result += ': '; continue; }
        result += ch;
    }
    return esc(result);
}

function highlightJsonString(str) {
    var html = esc(str);
    return html.replace(
        /("(?:\\.|[^"\\])*")\s*:|("(?:\\.|[^"\\])*")|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(\btrue\b|\bfalse\b)|(\bnull\b)|([{}[\]])/g,
        function(m, key, str, num, bool, nul, bracket) {
            if (key) return '<span class="json-key">' + key + '</span>:';
            if (str) return '<span class="json-string">' + str + '</span>';
            if (num) return '<span class="json-number">' + num + '</span>';
            if (bool) return '<span class="json-boolean">' + bool + '</span>';
            if (nul) return '<span class="json-null">' + nul + '</span>';
            if (bracket) return '<span class="json-bracket">' + bracket + '</span>';
            return m;
        }
    );
}

function renderResponsePreview(preview) {
    if (!preview) return '';
    if (typeof preview === 'object') return highlightJsonString(JSON.stringify(preview, null, 2));
    if (typeof preview !== 'string') return esc(String(preview));
    try { var parsed = JSON.parse(preview); return highlightJsonString(JSON.stringify(parsed, null, 2)); }
    catch(_) {
        var cleaned = preview.replace(/\.\.\.\s*$/, '').trim();
        try { var parsed = JSON.parse(cleaned); return highlightJsonString(JSON.stringify(parsed, null, 2)); }
        catch(_2) { return formatJsonString(preview).replace(/\n/g, '<br>').replace(/  /g, '&nbsp;&nbsp;'); }
    }
}

function showTestDetails(id) {
    var r = _testResults[id];
    if (!r) { document.getElementById('error').textContent = 'No test results for this account. Click Test All first.'; return; }
    var label = 'Unknown';
    var acc = window._accounts.find(function(x) { return x.id === id; });
    if (acc) label = esc(acc.label || acc.id.slice(0, 8));
    document.getElementById('testTitle').textContent = 'Test: ' + label;
    var html = '';
    html += '<div class="cred-row"><span class="cred-key">Status</span><span class="cred-value ' + (r.ok ? 'test-pass' : 'test-fail') + '"><strong>' + (r.ok ? 'PASS' : 'FAIL') + '</strong></span></div>';
    html += '<div class="cred-row"><span class="cred-key">HTTP Status</span><span class="cred-value">' + (r.status_code || '-') + '</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Response Time</span><span class="cred-value">' + r.ms + 'ms</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Token Expiry</span><span class="cred-value">' + timeStr(r.token_expires_at) + '</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Active</span><span class="cred-value">' + (r.is_active ? 'Yes' : 'No') + '</span></div>';
    if (r.premium) {
        html += '<div class="cred-row"><span class="cred-key">Premium</span><span class="cred-value">' + esc(r.premium) + (r.premium_reason ? ' — ' + esc(r.premium_reason) : '') + '</span></div>';
    }
    if (r.error) {
        html += '<div class="cred-row" style="margin-top:12px"><span class="cred-key">Error</span><span class="cred-value test-fail">' + esc(r.error) + '</span></div>';
    }
    var raw = r.response_body || r.response || r.response_preview;
    if (raw) {
        var pretty = renderResponsePreview(raw);
        html += '<div style="margin-top:16px;padding-top:12px;border-top:1px solid #30363d"><span class="cred-key" style="display:block;margin-bottom:8px">Full JSON Response</span>';
        html += '<pre style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap;word-break:break-word;color:#c9d1d9;font-size:11px;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace">' + pretty + '</pre></div>';
    }
    document.getElementById('testDetailsContent').innerHTML = html;
    document.getElementById('testOverlay').classList.add('open');
}

function closeTestDetails() {
    document.getElementById('testOverlay').classList.remove('open');
}

function trunc(s, n) {
    if (!s) return '';
    n = n || 40;
    return s.length > n ? s.slice(0, n) + '...' : s;
}

function openEdit(id) {
    editId = id;
    var a = window._accounts.find(function(x) { return x.id === id; });
    if (!a) return;
    document.getElementById('ed-label').value = a.label || '';
    document.getElementById('ed-user-id').value = a.user_id || '';
    document.getElementById('ed-client-id').value = a.client_id || '';
    document.getElementById('ed-client-secret').value = a.client_secret || '';
    document.getElementById('ed-refresh-token').value = a.refresh_token || '';
    document.getElementById('editTitle').textContent = 'Edit ' + (a.label || a.id.slice(0, 8));
    document.getElementById('editOverlay').classList.add('open');
}

function closeEdit() {
    editId = null;
    document.getElementById('editOverlay').classList.remove('open');
}

async function saveEdit() {
    var id = editId;
    if (!id) return;
    var body = {
        label: document.getElementById('ed-label').value,
        user_id: document.getElementById('ed-user-id').value || null,
        client_id: document.getElementById('ed-client-id').value,
        client_secret: document.getElementById('ed-client-secret').value,
        refresh_token: document.getElementById('ed-refresh-token').value,
    };
    try {
        var res = await fetch('/admin/accounts/' + id, {
            method: 'PATCH', headers: headers(), body: JSON.stringify(body)
        });
        if (res.ok) {
            closeEdit();
            fetchData();
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function fetchData() {
    try {
        var [statsRes, accountsRes] = await Promise.all([
            fetch('/admin/stats', { headers: headers() }),
            fetch('/admin/accounts', { headers: headers() })
        ]);
        fetch('/').then(function(r) { return r.json(); }).then(function(d) {
            if (d && d.version) document.getElementById('version').textContent = 'v' + d.version;
        }).catch(function() {});
        if (statsRes.status === 401 || accountsRes.status === 401) { setKey(); return; }

        if (!statsRes.ok) {
            var text = await statsRes.text();
            document.getElementById('error').textContent = 'Stats: ' + statsRes.status + ' ' + text.slice(0, 200);
            return;
        }
        if (!accountsRes.ok) {
            var text = await accountsRes.text();
            document.getElementById('error').textContent = 'Accounts: ' + accountsRes.status + ' ' + text.slice(0, 200);
            return;
        }

        var statsText = await statsRes.text();
        var accountsText = await accountsRes.text();
        var stats, accounts;
        try { stats = JSON.parse(statsText); } catch(e) { document.getElementById('error').textContent = 'Stats parse: ' + statsText.slice(0, 200); return; }
        try { accounts = JSON.parse(accountsText); } catch(e) { document.getElementById('error').textContent = 'Accounts parse: ' + accountsText.slice(0, 200); return; }
        window._accounts = accounts.accounts;

        document.getElementById('stats').innerHTML =
            '<div class="stat-card"><div class="label">Total Requests</div><div class="value">' + (stats.total_requests || 0) + '</div></div>' +
            '<div class="stat-card"><div class="label">Error Rate <span style="opacity:.55;font-size:11px">last 5k</span></div><div class="value">' + (stats.error_rate || '0.00%') + '</div></div>' +
            '<div class="stat-card"><div class="label">Active</div><div class="value">' + (stats.healthy_accounts || 0) + '/' + (stats.total_accounts || 0) + '</div></div>' +
            '<div class="stat-card"><div class="label">Premium</div><div class="value">' + (stats.premium_accounts != null ? stats.premium_accounts : '—') + '</div></div>' +
            playbackCard(stats.playback) +
            catalogCard(stats.catalog) +
            redisCard(stats.redis);

        var html = '';
        if (accounts.accounts.length === 0) {
            html = '<div class="empty-state"><p>No accounts configured</p><p class="hint">Add one above or set CLIENT_ID/REFRESH_TOKEN in .env</p></div>';
        } else {
            for (var i = 0; i < accounts.accounts.length; i++) {
                var a = accounts.accounts[i];
                var label = a.label || a.id.slice(0, 8);
                var statusClass = 'status-dot ' + (a.is_active ? 'status-ok' : 'status-err');
                var statusText = a.is_active ? 'Active' : 'Inactive';
                var activeCls = a.is_active ? ' btn-active' : '';
                var toggleText = a.is_active ? 'ON' : 'OFF';
                var tokenStr = timeStr(a.token_expires_at);
                var uid = a.user_id || '-';
                var catalogBadge = a.is_catalog ? '<span class="status-label" style="color:#d2a8ff;background:rgba(210,168,255,0.1)">CATALOG</span>' : '';
                var premiumBadge = (function(p) {
                    if (p === 'premium') return '<span class="status-label status-premium">PREMIUM</span>';
                    if (p === 'preview-only') return '<span class="status-label status-preview">PREVIEW-ONLY</span>';
                    if (p === 'error') return '<span class="status-label status-err">CHECK FAILED</span>';
                    return '<span class="status-label status-muted">UNCHECKED</span>';
                })(a.premium_status);
                var catalogBtn = a.is_catalog
                    ? '<button class="btn" onclick="setCatalog(\'' + a.id + '\',false)" title="Return to playback pool">Uncatalog</button>'
                    : '<button class="btn" onclick="setCatalog(\'' + a.id + '\',true)" title="Metadata only, never playback">Catalog</button>';
                html += '<div class="account-card">' +
                    '<div class="card-header">' +
                        '<div class="left"><span class="acc-num">' + (i + 1) + '</span><span class="' + statusClass + '"></span><span class="label">' + esc(label) + '</span><span class="status-label ' + (a.is_active ? 'status-ok' : 'status-err') + '">' + statusText + '</span>' + catalogBadge + premiumBadge + '</div>' +
                        '<div class="card-actions">' +
                            '<button class="btn" onclick="refreshAccount(\'' + a.id + '\')">Refresh Token</button>' +
                            '<button class="btn" onclick="checkPremium(\'' + a.id + '\')" title="Probe whether this account serves full quality">Check premium</button>' +
                            '<button class="btn" onclick="openEdit(\'' + a.id + '\')">Edit</button>' +
                            '<button class="btn" onclick="duplicateAccount(\'' + a.id + '\')">Duplicate</button>' +
                            catalogBtn +
                            '<button class="btn' + activeCls + '" onclick="toggleAccount(\'' + a.id + '\',' + (!a.is_active) + ')">' + toggleText + '</button>' +
                            '<button class="btn btn-danger" onclick="removeAccount(\'' + a.id + '\')">Delete</button>' +
                        '</div>' +
                    '</div>' +
                    '<div class="card-body">' +
                        '<div class="cred-row"><span class="cred-key">CLIENT_ID</span><span class="cred-value">' + esc(a.client_id) + '</span></div>' +
                        '<div class="cred-row"><span class="cred-key">CLIENT_SECRET</span><span class="cred-value masked">' + esc(a.client_secret.slice(0, 20)) + '***</span></div>' +
                        '<div class="cred-row"><span class="cred-key">USER_ID</span><span class="cred-value">' + esc(uid) + '</span></div>' +
                        '<div class="cred-row"><span class="cred-key">REFRESH_TOKEN</span><span class="cred-value token">' + esc(trunc(a.refresh_token, 50)) + '</span></div>' +
                    '</div>' +
                    '<div class="card-footer">' +
                        '<div class="card-stats">' +
                            '<span class="card-stat">Requests <strong>' + a.request_count + '</strong></span>' +
                            '<span class="card-stat">Errors <strong>' + a.error_count + '</strong>' + (a.request_count > 0 ? ' (' + (100 * a.error_count / a.request_count).toFixed(1) + '%)' : '') + '</span>' +
                            '<span class="card-stat">Used <strong>' + relAgo(a.last_used) + '</strong></span>' +
                            '<span class="card-stat">Premium <strong>' + esc(a.premium_status || 'unknown') + '</strong>' + (a.premium_checked_at > 0 ? ' (' + relAgo(a.premium_checked_at) + ')' : '') + '</span>' +
                            (a.auto_disabled ? '<span class="card-stat">Auto-heal <strong>retrying</strong></span>' : '') +
                            '<span class="card-stat">Token <strong>' + tokenStr + '</strong></span>' +
                            '<span class="card-stat test-badge" id="test-' + a.id + '" onclick="showTestDetails(\'' + a.id + '\')">Test <strong>-</strong></span>' +
                        '</div>' +
                    '</div>' +
                '</div>';
            }
        }
        document.getElementById('accounts-container').innerHTML = html;
        renderTestResults(_testResults);
    } catch(e) {
        document.getElementById('error').textContent = 'Failed: ' + e.message;
    }
}

function esc(s) {
    return (s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function togglePw(id, btn) {
    var input = document.getElementById(id);
    if (!input) return;
    var show = input.type === 'password';
    input.type = show ? 'text' : 'password';
    if (btn) btn.innerHTML = show ? '&#128064;' : '&#128065;';
}
async function refreshAccount(id) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/refresh', {
            method: 'POST', headers: headers()
        });
        var data = await res.json();
        if (res.ok && data.status !== 'error') {
            document.getElementById('success').textContent = 'Token refreshed, account reactivated!';
            fetchData();
        } else {
            document.getElementById('error').textContent = 'Refresh failed: ' + (data.message || data.detail || res.status);
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function checkPremium(id) {
    try {
        document.getElementById('success').textContent = 'Probing account…';
        var res = await fetch('/admin/accounts/' + id + '/check-premium', {
            method: 'POST', headers: headers()
        });
        var data = await res.json();
        if (res.ok) {
            var msg = 'Premium check: ' + data.premium;
            if (data.reason) msg += ' (' + data.reason + ')';
            document.getElementById('success').textContent = msg;
            fetchData();
        } else {
            document.getElementById('error').textContent = 'Check failed: ' + (data.detail || res.status);
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function toggleAccount(id, active) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/toggle', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ active: active })
        });
        if (res.ok) fetchData();
        else { var d = await res.json(); document.getElementById('error').textContent = d.detail || 'Error'; }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function removeAccount(id) {
    if (!confirm('Delete this account?')) return;
    try {
        var res = await fetch('/admin/accounts/' + id, {
            method: 'DELETE', headers: headers()
        });
        if (res.ok) fetchData();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function setCatalog(id, catalog) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/catalog', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ catalog: catalog })
        });
        if (res.ok) fetchData();
        else { var d = await res.json(); document.getElementById('error').textContent = d.detail || 'Error'; }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

function duplicateAccount(id) {
    var a = window._accounts.find(function(x) { return x.id === id; });
    if (!a) { document.getElementById('error').textContent = 'Account not found'; return; }
    document.getElementById('new-label').value = 'Copy of ' + (a.label || a.id.slice(0, 8));
    document.getElementById('new-user-id').value = a.user_id || '';
    document.getElementById('new-client-id').value = a.client_id || '';
    document.getElementById('new-client-secret').value = a.client_secret || '';
    document.getElementById('new-refresh-token').value = a.refresh_token || '';
    var form = document.querySelector('.form-section');
    form.scrollIntoView({ behavior: 'smooth', block: 'center' });
    form.classList.add('form-highlight');
    setTimeout(function() { form.classList.remove('form-highlight'); }, 1500);
}

async function addAccount() {
    var label = document.getElementById('new-label').value;
    var userId = document.getElementById('new-user-id').value || null;
    var client_id = document.getElementById('new-client-id').value;
    var client_secret = document.getElementById('new-client-secret').value;
    var refresh_token = document.getElementById('new-refresh-token').value;

    if (!client_id || !client_secret || !refresh_token) {
        document.getElementById('error').textContent = 'Client ID, secret, and refresh token are required';
        return;
    }

    try {
        var res = await fetch('/admin/accounts', {
            method: 'POST', headers: headers(),
            body: JSON.stringify({ label: label, user_id: userId, client_id: client_id, client_secret: client_secret, refresh_token: refresh_token, catalog: document.getElementById('new-catalog').checked })
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Account added!';
            fetchData();
            document.getElementById('new-label').value = '';
            document.getElementById('new-user-id').value = '';
            document.getElementById('new-client-id').value = '';
            document.getElementById('new-client-secret').value = '';
            document.getElementById('new-refresh-token').value = '';
            document.getElementById('new-catalog').checked = false;
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

var oauthSessionId = null;
var oauthPollInterval = null;

var oauthLabelTimer = null;

function startOAuth() {
    document.getElementById('oauth-modal-label').value = document.getElementById('new-label').value || '';
    document.getElementById('oauthUrl').textContent = 'Starting...';
    document.getElementById('oauthStatus').textContent = 'Contacting Tidal...';
    document.getElementById('oauthOverlay').classList.add('open');
    var lbl = (document.getElementById('new-label').value || '').trim();
    fetch('/admin/setup', { method: 'POST', headers: headers(), body: JSON.stringify({ label: lbl || null }) })
        .then(function(r) {
            if (!r.ok) throw new Error('HTTP ' + r.status);
            return r.json();
        })
        .then(function(data) {
            document.getElementById('oauthUrl').textContent = data.verification_uri;
            oauthSessionId = data.session_id;
            document.getElementById('oauthStatus').textContent = 'Open the URL above and authorize in your browser. Waiting...';
            if (oauthPollInterval) clearInterval(oauthPollInterval);
            oauthPollInterval = setInterval(pollOAuth, 3000);
        })
        .catch(function(e) {
            document.getElementById('oauthStatus').textContent = 'Error: ' + e.message;
        });
}

function updateOAuthLabel() {
    if (!oauthSessionId) return;
    if (oauthLabelTimer) clearTimeout(oauthLabelTimer);
    oauthLabelTimer = setTimeout(function() {
        var lbl = (document.getElementById('oauth-modal-label').value || '').trim();
        fetch('/admin/setup/' + oauthSessionId, {
            method: 'PATCH', headers: headers(), body: JSON.stringify({ label: lbl || null })
        }).catch(function() {});
    }, 500);
}

function pollOAuth() {
    if (!oauthSessionId) return;
    fetch('/admin/setup/' + oauthSessionId, { headers: headers() })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.status === 'complete') {
                document.getElementById('oauthStatus').textContent = 'Account ' + data.label + ' added!';
                if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
                setTimeout(function() { closeOAuth(); fetchData(); }, 1500);
            } else if (data.status === 'error') {
                document.getElementById('oauthStatus').textContent = 'Error: ' + data.error;
                if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
            } else {
                document.getElementById('oauthStatus').textContent = 'Waiting for you to authorize in the browser...';
            }
        })
        .catch(function(e) {
            document.getElementById('oauthStatus').textContent = 'Poll error: ' + e.message;
        });
}

function closeOAuth() {
    document.getElementById('oauthOverlay').classList.remove('open');
    oauthSessionId = null;
    if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
}

function copyOAuthUrl() {
    var url = document.getElementById('oauthUrl').textContent;
    if (!url || url === '—' || url === 'Starting...') return;
    navigator.clipboard.writeText(url).then(function() {
        var btn = document.getElementById('copyOAuthBtn');
        btn.textContent = 'Copied!';
        setTimeout(function() { btn.textContent = 'Copy URL'; }, 2000);
    });
}

function openOAuthUrl() {
    var url = document.getElementById('oauthUrl').textContent;
    if (!url || url === '—' || url === 'Starting...') return;
    window.open(url, '_blank');
}

fetchData();
setInterval(fetchData, 15000);
loadSettings();
loadProxyStatus();
setInterval(loadProxyStatus, 15000);
loadAlertStatus();

async function loadAlertStatus() {
    try {
        var res = await fetch('/admin/alerts', { headers: headers() });
        if (!res.ok) return;
        var a = (await res.json()).alerts || {};
        document.getElementById('al-discord').textContent = a.discord_configured ? 'Configured' : 'Not set';
    } catch(e) {}
}

async function sendReport(kind) {
    var btn = document.getElementById(kind === 'status' ? 'reportStatusBtn' : 'reportAccountsBtn');
    btn.textContent = 'Sending...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/report', { method: 'POST', headers: headers(), body: JSON.stringify({ kind: kind }) });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Report sent!';
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = kind === 'status' ? 'Send Status' : 'Send Accounts';
        btn.disabled = false;
    }
}

async function testAlert() {
    var btn = document.getElementById('alertTestBtn');
    btn.textContent = 'Sending...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/test', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Test alert sent!';
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = 'Send Test Alert';
        btn.disabled = false;
    }
}
loadRequestLog();
setInterval(loadRequestLog, 15000);
loadCacheStats();
setInterval(loadCacheStats, 15000);
loadApiKeys();
setInterval(loadApiKeys, 15000);

async function loadApiKeys() {
    try {
        var res = await fetch('/admin/keys', { headers: headers() });
        if (!res.ok) return;
        var keys = (await res.json()).api_keys || [];
        var html = '';
        for (var k of keys) {
            var quota = (k.quota && k.quota > 0) ? (k.used + '/' + k.quota) : (k.used + '/∞');
            html += '<div class="cred-row"><span class="cred-key">' + esc(k.label || k.key_prefix) + '</span>' +
                '<span class="cred-value">' + esc(k.key_prefix) + '… · used ' + quota + ' · ' + (k.is_active ? 'ON' : 'OFF') + '</span>' +
                '<span style="margin-left:auto;display:flex;gap:6px">' +
                '<button class="btn" onclick="toggleApiKey(\'' + k.id + '\',' + (!k.is_active) + ')">' + (k.is_active ? 'OFF' : 'ON') + '</button>' +
                '<button class="btn btn-danger" onclick="removeApiKey(\'' + k.id + '\')">Delete</button>' +
                '</span></div>';
        }
        document.getElementById('keys-container').innerHTML = html || '<span style="font-size:12px;color:#8b949e">No keys — API is open</span>';
    } catch(e) {}
}

async function addApiKey() {
    try {
        var res = await fetch('/admin/keys', {
            method: 'POST', headers: headers(),
            body: JSON.stringify({ label: document.getElementById('new-key-label').value, quota: parseInt(document.getElementById('new-key-quota').value) || 0 })
        });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('keyResult').textContent = 'New key (copy now, shown once): ' + data.api_key;
            document.getElementById('new-key-label').value = '';
            document.getElementById('new-key-quota').value = '';
            loadApiKeys();
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function toggleApiKey(id, active) {
    try {
        var res = await fetch('/admin/keys/' + id + '/toggle', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ active: active })
        });
        if (res.ok) loadApiKeys();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function removeApiKey(id) {
    if (!confirm('Delete this API key? Clients using it will get 401.')) return;
    try {
        var res = await fetch('/admin/keys/' + id, { method: 'DELETE', headers: headers() });
        if (res.ok) loadApiKeys();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function downloadBackup() {
    try {
        var res = await fetch('/admin/backup', { headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Backup failed: ' + res.status; return; }
        var blob = await res.blob();
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a'); a.href = url; a.download = 'hifi-backup.db'; a.click();
        URL.revokeObjectURL(url);
        document.getElementById('success').textContent = 'Backup downloaded';
    } catch(e) { document.getElementById('error').textContent = e.message; }
}

async function restoreBackup(e) {
    var file = e.target.files[0]; if (!file) return;
    if (!confirm('Restore database from ' + file.name + '? Current accounts, keys and settings will be replaced.')) { e.target.value = ''; return; }
    try {
        var buf = await file.arrayBuffer();
        var res = await fetch('/admin/backup/restore', { method: 'POST', headers: headers(), body: buf });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Restored!';
            document.getElementById('restoreResult').textContent = '';
            fetchData();
            loadApiKeys();
        } else {
            document.getElementById('error').textContent = data.detail || 'Restore failed';
        }
    } catch(err) { document.getElementById('error').textContent = 'Restore error: ' + err.message; }
    e.target.value = '';
}

async function clearCache() {    var btn = document.getElementById('clearCacheBtn');
    btn.textContent = 'Clearing...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/cache/clear', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Cache cleared!';
            loadRequestLog();
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = 'Clear Cache';
        btn.disabled = false;
    }
}

async function loadCacheStats() {
    try {
        var res = await fetch('/admin/cache', { headers: headers() });
        if (!res.ok) return;
        var c = (await res.json()).cache || {};
        document.getElementById('cc-hits').textContent = c.hits != null ? c.hits : '—';
        document.getElementById('cc-misses').textContent = c.misses != null ? c.misses : '—';
        var hr = (c.hits != null && c.misses != null && (c.hits + c.misses) > 0)
            ? (100 * c.hits / (c.hits + c.misses)).toFixed(2) + '%' : '—';
        document.getElementById('cc-hitrate').textContent = hr;
        document.getElementById('cc-stale').textContent = c.stale != null ? c.stale : '—';
        document.getElementById('cc-negative').textContent = c.negative != null ? c.negative : '—';
    } catch(e) {}
}

async function loadRequestLog() {
    try {
        var res = await fetch('/admin/requests?limit=20', { headers: headers() });
        if (!res.ok) return;
        var r = (await res.json()).requests || {};
        document.getElementById('rq-total').textContent = r.total != null ? r.total : '—';
        document.getElementById('rq-errors').textContent = r.errors != null ? r.errors : '—';
        document.getElementById('rq-user-errors').textContent = r.user_errors != null ? r.user_errors : '—';
        document.getElementById('rq-upstream-errors').textContent = r.upstream_errors != null ? r.upstream_errors : '—';
        document.getElementById('rq-p50').textContent = r.p50_ms != null ? r.p50_ms + 'ms' : '—';
        document.getElementById('rq-p95').textContent = r.p95_ms != null ? r.p95_ms + 'ms' : '—';
        var ep = '';
        for (var e of (r.by_endpoint || []).slice(0, 8)) {
            ep += '<span>' + esc(e.endpoint) + ' <strong>×' + e.hits + '</strong></span>';
        }
        document.getElementById('rq-endpoints').innerHTML = ep;
        var st = '';
        var codes = Object.keys(r.by_status || {}).sort(function(a, b) { return Number(a) - Number(b); });
        for (var ci = 0; ci < codes.length; ci++) {
            st += '<span>' + esc(codes[ci]) + ' <strong>×' + r.by_status[codes[ci]] + '</strong></span>';
        }
        document.getElementById('rq-statuses').innerHTML = st ? '<span style="color:#5f6f60">status:</span> ' + st : '';
        var ee = '';
        for (var x of (r.errors_by_endpoint || []).slice(0, 5)) {
            ee += '<span>' + esc(x.endpoint) + ' <strong>×' + x.errors + '</strong></span>';
        }
        document.getElementById('rq-err-endpoints').innerHTML = ee ? '<span style="color:#5f6f60">err:</span> ' + ee : '';
        var tt = '';
        for (var t of (r.top_tracks || []).slice(0, 5)) {
            tt += '<span>#' + esc(t.id) + ' <strong>×' + t.hits + '</strong></span>';
        }
        document.getElementById('rq-tracks').innerHTML = tt ? '<span style="color:#5f6f60">top:</span> ' + tt : '';
        var sw = '';
        for (var s of (r.slowest || []).slice(0, 3)) {
            sw += '<span>' + esc(s.endpoint) + (s.detail ? ' #' + esc(s.detail) : '') + ' <strong>' + s.latency_ms + 'ms</strong></span>';
        }
        document.getElementById('rq-slowest').innerHTML = sw ? '<span style="color:#5f6f60">slowest:</span> ' + sw : '';
        var rows = '';
        var filter = 'all';
        try { filter = document.getElementById('rq-filter').value || 'all'; } catch(e) {}
        for (var q of (r.recent || [])) {
            if (filter === 'errors' && !(q.status >= 400)) continue;
            if (filter === 'slow' && !q.slow && !(q.latency_ms >= 3000)) continue;
            var cls = q.status >= 500 ? 'test-fail' : (q.status >= 400 ? 'test-pending' : 'test-pass');
            var t = '';
            if (q.ts) {
                var d = new Date(q.ts * 1000);
                t = ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2) + ':' + ('0' + d.getSeconds()).slice(-2);
            }
            rows += '<div class="term-line"><span class="term-time">' + t + '</span> ' +
                '<span class="term-method m-' + q.method + '">' + q.method + '</span> ' +
                '<span class="term-path">' + esc(q.path) + '</span>' +
                (q.detail ? ' <span class="term-id">#' + esc(q.detail) + '</span>' : '') + ' ' +
                '<span class="' + cls + '">' + q.status + '</span> ' +
                (q.cache ? '<span class="term-cache">[' + esc(q.cache) + ']</span> ' : '') +
                '<span class="term-dim"' + (q.slow ? ' style="color:#d29922"' : '') + '>' + q.latency_ms + 'ms ' + esc(q.client_ip) + '</span></div>';
        }
        var box = document.getElementById('rq-recent');
        if (rows) {
            box.innerHTML = rows + '<div class="term-line"><span class="term-dim">$</span> <span class="term-cursor"></span></div>';
            box.scrollTop = box.scrollHeight;
        } else {
            box.innerHTML = '<div class="term-line"><span class="term-dim">$ waiting for traffic…</span> <span class="term-cursor"></span></div>';
        }
    } catch(e) {}
}

async function loadProxyStatus() {
    try {
        var res = await fetch('/admin/proxies', { headers: headers() });
        if (!res.ok) return;
        var p = (await res.json()).proxies || {};
        document.getElementById('px-status').textContent = !p.enabled ? 'Disabled (direct)' : (p.ready ? 'Active' : 'No working proxy');
        document.getElementById('px-current').textContent = p.current || (p.enabled ? '—' : 'direct');
        document.getElementById('px-pool').textContent = p.pool_size != null ? p.pool_size : '—';
        document.getElementById('px-fails').textContent = p.consecutive_fails != null ? p.consecutive_fails : '—';
    } catch(e) {}
}

async function loadSettings() {
    try {
        var res = await fetch('/admin/settings', { headers: headers() });
        if (!res.ok) return;
        var d = await res.json();
        var r = d.settings || d.rate_limits || {};
        document.getElementById('rl-autoheal').checked = r.auto_heal !== false;
        document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
    } catch(e) {}
}

async function exportCredentials() {
    try {
        var res = await fetch('/admin/accounts/export', { headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Export failed: ' + res.status; return; }
        var data = await res.json();
        var list = data.accounts || data;
        var blob = new Blob([JSON.stringify(list, null, 2)], { type: 'application/json' });
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a'); a.href = url; a.download = 'credentials.json'; a.click();
        URL.revokeObjectURL(url);
        document.getElementById('success').textContent = 'Exported ' + list.length + ' accounts';
    } catch(e) { document.getElementById('error').textContent = e.message; }
}

async function importCredentials(e) {
    var file = e.target.files[0]; if (!file) return;
    try {
        var text = await file.text();
        var json = JSON.parse(text);
        var payload = json.accounts ? json : (Array.isArray(json) ? { accounts: json } : json);
        var res = await fetch('/admin/accounts/import', { method: 'POST', headers: headers(), body: JSON.stringify(payload) });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = 'Imported ' + data.imported + ' accounts (skipped ' + data.skipped + ')';
            document.getElementById('importResult').textContent = data.errors && data.errors.length ? 'Errors: ' + JSON.stringify(data.errors).slice(0, 400) : '';
            fetchData();
        } else { document.getElementById('error').textContent = data.detail || 'Import failed'; }
    } catch(err) { document.getElementById('error').textContent = 'Import error: ' + err.message; }
    e.target.value = '';
}

async function saveSettings() {
    var body = {
        settings: {
            auto_heal: document.getElementById('rl-autoheal').checked,
            atmos_mode: document.getElementById('rl-atmos').value
        }
    };
    try {
        var res = await fetch('/admin/settings', {
            method: 'PUT', headers: headers(), body: JSON.stringify(body)
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Settings saved!';
            var d = await res.json();
            var r = d.settings || d.rate_limits || {};
            document.getElementById('rl-autoheal').checked = r.auto_heal !== false;
            document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error saving settings';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}
</script>
</body>
</html>"##;
