use axum::response::Html;

pub async fn admin_index() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

const ADMIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HiFi API Admin</title>
<style>
* { margin:0; padding:0; box-sizing:border-box; }
body { font-family:'SF Mono','Fira Code','Cascadia Code','JetBrains Mono',Menlo,Monaco,Consolas,monospace; background:#0d1117; color:#c9d1d9; padding:20px; }
.container { max-width:1000px; margin:0 auto; padding:0 8px; }

.header { display:flex; align-items:center; justify-content:space-between; margin-bottom:24px; flex-wrap:wrap; gap:12px; }
.header h1 { font-size:22px; color:#f0f6fc; letter-spacing:-0.3px; }
.header .badge { font-size:11px; background:#1f6feb; color:#fff; padding:3px 10px; border-radius:10px; font-weight:500; }

.stats { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-bottom:24px; }
.stat-card { background:#161b22; border:1px solid #30363d; border-radius:10px; padding:18px 20px; transition:border-color 0.2s; }
.stat-card:hover { border-color:#484f58; }
.stat-card .label { font-size:11px; color:#8b949e; text-transform:uppercase; letter-spacing:0.5px; }
.stat-card .value { font-size:26px; font-weight:700; margin-top:4px; color:#f0f6fc; letter-spacing:-0.5px; }

.accounts-grid { display:flex; flex-direction:column; gap:20px; margin-bottom:24px; }
.account-card { background:#161b22; border:1px solid #30363d; border-radius:10px; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.account-card:hover { border-color:#484f58; box-shadow:0 4px 24px rgba(0,0,0,0.3); }

.card-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:#1c2128; border-bottom:1px solid #30363d; flex-wrap:wrap; gap:10px; }
.card-header .left { display:flex; align-items:center; gap:10px; min-width:0; }
.card-header .label { font-weight:600; font-size:14px; color:#f0f6fc; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }

.card-body { padding:18px 20px; }
.cred-row { display:flex; align-items:baseline; gap:8px; padding:8px 0; font-size:12px; }
.cred-row:last-child { padding-bottom:0; }
.cred-key { color:#8b949e; min-width:120px; user-select:none; flex-shrink:0; }
.cred-key::after { content:'='; margin-left:4px; color:#30363d; }
.cred-value { color:#c9d1d9; word-break:break-all; min-width:0; }
.cred-value.masked { color:#58a6ff; }
.cred-value.token { color:#d2a8ff; font-size:11px; }

.card-footer { display:flex; align-items:center; justify-content:space-between; padding:12px 20px; border-top:1px solid #30363d; background:#12161c; flex-wrap:wrap; gap:10px; }
.card-stats { display:flex; gap:14px; flex-wrap:wrap; }
.card-stat { font-size:11px; color:#8b949e; white-space:nowrap; }
.card-stat strong { color:#c9d1d9; }
.test-badge { cursor:pointer; text-decoration:underline; text-decoration-style:dotted; text-underline-offset:2px; }
.test-badge:hover { color:#f0f6fc; }

.card-actions { display:flex; gap:6px; flex-wrap:wrap; }
.status-dot { display:inline-block; width:10px; height:10px; border-radius:50%; flex-shrink:0; }
.status-ok { background:#3fb950; box-shadow:0 0 6px rgba(63,185,80,0.3); }
.status-warn { background:#d29922; box-shadow:0 0 6px rgba(210,153,34,0.3); }
.status-err { background:#f85149; box-shadow:0 0 6px rgba(248,81,73,0.3); }

.btn { background:#21262d; border:1px solid #30363d; color:#c9d1d9; padding:7px 14px; border-radius:6px; cursor:pointer; font-size:12px; font-weight:500; transition:all 0.15s; }
.btn:hover { background:#30363d; border-color:#484f58; transform:translateY(-1px); }
.btn:active { transform:translateY(0); }
.btn-primary { background:#238636; border-color:rgba(35,134,54,0.5); color:#fff; }
.btn-primary:hover { background:#2ea043; border-color:#2ea043; }
.btn-danger { border-color:rgba(248,81,73,0.4); color:#f85149; }
.btn-danger:hover { background:#f85149; border-color:#f85149; color:#fff; }
.btn-active { background:#1f6feb; border-color:rgba(31,111,235,0.5); color:#fff; }

.form-section { background:#161b22; border:1px solid #30363d; border-radius:10px; padding:28px; transition:border-color 0.2s; }
.form-section:hover { border-color:#484f58; }
.form-section h3 { font-size:16px; margin-bottom:20px; color:#f0f6fc; }
.form-row { display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-bottom:16px; }
.form-row.full { grid-template-columns:1fr; }
.form-group label { display:block; font-size:11px; color:#8b949e; margin-bottom:4px; font-weight:500; text-transform:uppercase; letter-spacing:0.3px; }
.form-group input { width:100%; background:#0d1117; border:1px solid #30363d; color:#c9d1d9; padding:10px 12px; border-radius:6px; font-size:13px; transition:border-color 0.15s; }
.form-group input:focus { outline:none; border-color:#1f6feb; box-shadow:0 0 0 3px rgba(31,111,235,0.15); }

.error { color:#f85149; font-size:13px; margin-bottom:10px; padding:8px 12px; background:rgba(248,81,73,0.08); border:1px solid rgba(248,81,73,0.2); border-radius:6px; }
.success { color:#3fb950; font-size:13px; margin-bottom:10px; padding:8px 12px; background:rgba(63,185,80,0.08); border:1px solid rgba(63,185,80,0.2); border-radius:6px; }
.error:empty, .success:empty { display:none; padding:0; margin:0; border:none; }

.overlay { display:none; position:fixed; inset:0; background:rgba(0,0,0,0.65); z-index:100; backdrop-filter:blur(6px); -webkit-backdrop-filter:blur(6px); }
.overlay.open { display:flex; align-items:center; justify-content:center; }
.modal { background:#161b22; border:1px solid #30363d; border-radius:12px; padding:24px; width:520px; max-width:92vw; max-height:90vh; overflow-y:auto; scrollbar-width:none; animation:modalIn 0.2s ease; }
.modal::-webkit-scrollbar { display:none; }
@keyframes modalIn { from { opacity:0; transform:scale(0.95) translateY(8px); } to { opacity:1; transform:scale(1) translateY(0); } }
.modal h3 { font-size:17px; margin-bottom:20px; color:#f0f6fc; }
.modal .form-group { margin-bottom:16px; }
.modal .modal-actions { display:flex; gap:10px; margin-top:18px; }
.modal .modal-actions .btn { padding:8px 20px; font-size:13px; }

.empty-state { text-align:center; padding:48px 20px; color:#8b949e; }
.empty-state p { font-size:15px; margin-bottom:6px; }
.empty-state .hint { font-size:13px; }

.test-pass { color:#3fb950; }
.test-fail { color:#f85149; }
.test-pending { color:#d29922; }

.status-label { font-size:11px; margin-left:6px; padding:2px 8px; border-radius:4px; font-weight:500; }
.status-label.status-ok { color:#3fb950; background:rgba(63,185,80,0.1); }
.status-label.status-err { color:#f85149; background:rgba(248,81,73,0.1); }

.test-results-section { background:#161b22; border:1px solid #30363d; border-radius:10px; margin:24px 0; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.test-results-section:hover { border-color:#484f58; box-shadow:0 4px 24px rgba(0,0,0,0.3); }
.test-results-section .test-results-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:#1c2128; border-bottom:1px solid #30363d; }
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
</style>
</head>
<body>
<div class="container">
<div class="header">
<h1>HiFi API Admin</h1>
<div style="display:flex;gap:8px;align-items:center">
<button class="btn btn-primary" onclick="testAll()" id="testAllBtn">Test All</button>
<span class="badge" id="version">v2.10</span>
</div>
</div>
<div id="stats" class="stats"></div>
<div id="error" class="error"></div>
<div id="success" class="success"></div>
<div id="accounts-container" class="accounts-grid"></div>
<div class="form-section">
<h3>Add Account</h3>
<div class="form-row">
<div class="form-group"><label>Label</label><input type="text" id="new-label" placeholder="My Account"></div>
<div class="form-group"><label>User ID (optional)</label><input type="text" id="new-user-id" placeholder="208921067"></div>
</div>
<div class="form-row full">
<div class="form-group"><label>Client ID</label><input type="text" id="new-client-id" placeholder="client_id"></div>
</div>
<div class="form-row">
<div class="form-group"><label>Client Secret</label><input type="password" id="new-client-secret" placeholder="client_secret"></div>
<div class="form-group"><label>Refresh Token</label><input type="password" id="new-refresh-token" placeholder="refresh_token"></div>
</div>
<button class="btn btn-primary" onclick="addAccount()">Add Account</button>
<button class="btn" onclick="startOAuth()" id="oauthBtn" style="margin-left:8px">Add via OAuth</button>
</div>

<div class="test-results-section" id="testResultsSection" style="display:none">
<div class="test-results-header">
<h3>Test Results</h3>
<div class="test-summary" id="testSummary"></div>
</div>
<div class="test-results-body" id="testResultsList"></div>
</div>
</div>
<div id="oauthOverlay" class="overlay" onclick="if(event.target===this)closeOAuth()">
<div class="modal">
<h3>Authorize via Tidal</h3>
<p style="margin-bottom:16px;color:#8b949e;font-size:14px">Open this URL in your browser, log into Tidal, and authorize the app.</p>
<div style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:16px;word-break:break-all;font-size:13px;font-family:monospace;color:#58a6ff;margin-bottom:16px" id="oauthUrl">—</div>
<button class="btn" onclick="copyOAuthUrl()" id="copyOAuthBtn" style="margin-right:8px">Copy URL</button>
<button class="btn" onclick="openOAuthUrl()" id="openOAuthBtn">Open</button>
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
<div class="form-group"><label>Client Secret</label><input type="password" id="ed-client-secret"></div>
<div class="form-group"><label>Refresh Token</label><input type="password" id="ed-refresh-token"></div>
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
        html += '<div class="test-result-row" onclick="showTestDetails(\'' + id + '\')">';
        html += '<span class="result-label">' + label + '</span>';
        html += '<span class="result-status ' + statusClass + '">' + statusText + '</span>';
        html += '<span class="result-http">' + httpText + '</span>';
        html += '<span class="result-ms">' + msText + '</span>';
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
    if (r.error) {
        html += '<div class="cred-row" style="margin-top:12px"><span class="cred-key">Error</span><span class="cred-value test-fail">' + esc(r.error) + '</span></div>';
    }
    var raw = r.response_body || r.response || r.response_preview;
    if (raw) {
        var pretty = renderResponsePreview(raw);
        html += '<div style="margin-top:16px;padding-top:12px;border-top:1px solid #30363d"><span class="cred-key" style="display:block;margin-bottom:8px">Full JSON Response</span>';
        html += '<pre style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap;word-break:break-word;color:#c9d1d9;font-size:11px">' + pretty + '</pre></div>';
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
            '<div class="stat-card"><div class="label">Error Rate</div><div class="value">' + (stats.error_rate || '0.00%') + '</div></div>' +
            '<div class="stat-card"><div class="label">Active</div><div class="value">' + (stats.healthy_accounts || 0) + '/' + (stats.total_accounts || 0) + '</div></div>' +
            '<div class="stat-card"><div class="label">Rate Limited</div><div class="value">' + (stats.rate_limited_accounts || 0) + '</div></div>';

        var html = '';
        if (accounts.accounts.length === 0) {
            html = '<div class="empty-state"><p>No accounts configured</p><p class="hint">Add one above or set CLIENT_ID/REFRESH_TOKEN in .env</p></div>';
        } else {
            for (var i = 0; i < accounts.accounts.length; i++) {
                var a = accounts.accounts[i];
                var label = a.label || a.id.slice(0, 8);
                var statusClass = 'status-dot ' + (a.is_active ? 'status-ok' : (a.rate_limited_until > Math.floor(Date.now()/1000) ? 'status-warn' : 'status-err'));
                var statusText = a.is_active ? 'Active' : 'Inactive';
                var activeCls = a.is_active ? ' btn-active' : '';
                var toggleText = a.is_active ? 'ON' : 'OFF';
                var rateStr = a.rate_limited_until ? timeStr(a.rate_limited_until) : 'No';
                var tokenStr = timeStr(a.token_expires_at);
                var uid = a.user_id || '-';

                html += '<div class="account-card">' +
                    '<div class="card-header">' +
                        '<div class="left"><span class="' + statusClass + '"></span><span class="label">' + esc(label) + '</span><span class="status-label ' + (a.is_active ? 'status-ok' : 'status-err') + '">' + statusText + '</span></div>' +
                        '<div class="card-actions">' +
                            '<button class="btn" onclick="refreshAccount(\'' + a.id + '\')">Refresh Token</button>' +
                            '<button class="btn" onclick="openEdit(\'' + a.id + '\')">Edit</button>' +
                            '<button class="btn" onclick="duplicateAccount(\'' + a.id + '\')">Duplicate</button>' +
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
                            '<span class="card-stat">Errors <strong>' + a.error_count + '</strong></span>' +
                            '<span class="card-stat">Rate Limited <strong>' + rateStr + '</strong></span>' +
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

async function refreshAccount(id) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/refresh', {
            method: 'POST', headers: headers()
        });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = 'Token refreshed, account reactivated!';
            fetchData();
        } else {
            document.getElementById('error').textContent = 'Refresh failed: ' + (data.message || data.detail || res.status);
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
            body: JSON.stringify({ label: label, user_id: userId, client_id: client_id, client_secret: client_secret, refresh_token: refresh_token })
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Account added!';
            fetchData();
            document.getElementById('new-label').value = '';
            document.getElementById('new-user-id').value = '';
            document.getElementById('new-client-id').value = '';
            document.getElementById('new-client-secret').value = '';
            document.getElementById('new-refresh-token').value = '';
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

function startOAuth() {
    document.getElementById('oauthUrl').textContent = 'Starting...';
    document.getElementById('oauthStatus').textContent = 'Contacting Tidal...';
    document.getElementById('oauthOverlay').classList.add('open');
    fetch('/admin/setup', { method: 'POST', headers: headers() })
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
</script>
</body>
</html>"#;
