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
body { font-family:-apple-system,BlinkMacSystemFont,sans-serif; background:#0d1117; color:#c9d1d9; padding:20px; }
.container { max-width:1000px; margin:0 auto; padding:0 8px; }
.header { display:flex; align-items:center; justify-content:space-between; margin-bottom:28px; }
.header h1 { font-size:24px; color:#f0f6fc; }
.header .badge { font-size:12px; background:#1f6feb; color:#fff; padding:4px 10px; border-radius:12px; }
.stats { display:grid; grid-template-columns:repeat(auto-fit,minmax(160px,1fr)); gap:16px; margin-bottom:28px; }
.stat-card { background:#161b22; border:1px solid #30363d; border-radius:8px; padding:20px; }
.stat-card .label { font-size:11px; color:#8b949e; text-transform:uppercase; letter-spacing:0.5px; }
.stat-card .value { font-size:28px; font-weight:700; margin-top:4px; color:#f0f6fc; }
.accounts-grid { display:flex; flex-direction:column; gap:20px; margin-bottom:28px; }
.account-card { background:#161b22; border:1px solid #30363d; border-radius:8px; overflow:hidden; }
.card-header { display:flex; align-items:center; justify-content:space-between; padding:16px 24px; background:#1c2128; border-bottom:1px solid #30363d; }
.card-header .left { display:flex; align-items:center; gap:12px; }
.card-header .label { font-weight:600; font-size:15px; color:#f0f6fc; }
.card-body { padding:24px; }
.cred-row { display:flex; align-items:baseline; gap:8px; padding:10px 0; font-family:'SF Mono',Monaco,monospace; font-size:13px; }
.cred-key { color:#8b949e; min-width:140px; user-select:none; }
.cred-key::after { content:'='; margin-left:4px; color:#30363d; }
.cred-value { color:#c9d1d9; word-break:break-all; }
.cred-value.masked { color:#58a6ff; }
.cred-value.token { color:#d2a8ff; font-size:12px; }
.card-footer { display:flex; align-items:center; justify-content:space-between; padding:14px 24px; border-top:1px solid #30363d; background:#12161c; }
.card-stats { display:flex; gap:16px; }
.card-stat { font-size:12px; color:#8b949e; }
.card-stat strong { color:#c9d1d9; }
.card-actions { display:flex; gap:6px; }
.status-dot { display:inline-block; width:10px; height:10px; border-radius:50%; }
.status-ok { background:#3fb950; box-shadow:0 0 6px rgba(63,185,80,0.3); }
.status-warn { background:#d29922; box-shadow:0 0 6px rgba(210,153,34,0.3); }
.status-err { background:#f85149; box-shadow:0 0 6px rgba(248,81,73,0.3); }
.btn { background:#21262d; border:1px solid #30363d; color:#c9d1d9; padding:8px 16px; border-radius:6px; cursor:pointer; font-size:12px; font-weight:500; transition:all 0.15s; }
.btn:hover { background:#30363d; }
.btn-primary { background:#238636; border-color:#238636; color:#fff; }
.btn-primary:hover { background:#2ea043; }
.btn-danger { border-color:#f85149; color:#f85149; }
.btn-danger:hover { background:#f85149; color:#fff; }
.btn-active { background:#1f6feb; border-color:#1f6feb; color:#fff; }
.form-section { background:#161b22; border:1px solid #30363d; border-radius:8px; padding:32px; }
.form-section h3 { font-size:16px; margin-bottom:24px; color:#f0f6fc; }
.form-row { display:grid; grid-template-columns:1fr 1fr; gap:16px; margin-bottom:20px; }
.form-row.full { grid-template-columns:1fr; }
.form-group label { display:block; font-size:12px; color:#8b949e; margin-bottom:4px; }
.form-group input { width:100%; background:#0d1117; border:1px solid #30363d; color:#c9d1d9; padding:12px 14px; border-radius:6px; font-size:14px; }
.form-group input:focus { outline:none; border-color:#1f6feb; }
.error { color:#f85149; font-size:14px; margin-bottom:12px; }
.success { color:#3fb950; font-size:14px; margin-bottom:12px; }
.overlay { display:none; position:fixed; inset:0; background:rgba(0,0,0,0.7); z-index:100; backdrop-filter:blur(4px); }
.overlay.open { display:flex; align-items:center; justify-content:center; }
.modal { background:#161b22; border:1px solid #30363d; border-radius:12px; padding:28px; width:520px; max-width:90vw; max-height:90vh; overflow-y:auto; }
.modal h3 { font-size:18px; margin-bottom:24px; color:#f0f6fc; }
.modal .form-group { margin-bottom:18px; }
.modal .modal-actions { display:flex; gap:10px; margin-top:20px; }
.modal .modal-actions .btn { padding:8px 20px; font-size:14px; }
.empty-state { text-align:center; padding:40px 20px; color:#8b949e; }
.empty-state p { font-size:15px; margin-bottom:8px; }
.empty-state .hint { font-size:13px; }
</style>
</head>
<body>
<div class="container">
<div class="header">
<h1>HiFi API Admin</h1>
<span class="badge" id="version">v2.10</span>
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
    if (!ts || ts === 0) return '-';
    var d = new Date(ts * 1000);
    var diff = d - new Date();
    if (diff < 0) return 'expired';
    var mins = Math.floor(diff / 60000);
    if (mins < 60) return mins + 'm';
    return Math.floor(mins / 60) + 'h ' + (mins % 60) + 'm';
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
                var rateStr = timeStr(a.rate_limited_until);
                var tokenStr = timeStr(a.token_expires_at);
                var uid = a.user_id || '-';

                html += '<div class="account-card">' +
                    '<div class="card-header">' +
                        '<div class="left"><span class="' + statusClass + '"></span><span class="label">' + esc(label) + '</span></div>' +
                        '<div class="card-actions">' +
                            '<button class="btn" onclick="openEdit(\'' + a.id + '\')">Edit</button>' +
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
                        '</div>' +
                    '</div>' +
                '</div>';
            }
        }
        document.getElementById('accounts-container').innerHTML = html;
    } catch(e) {
        document.getElementById('error').textContent = 'Failed: ' + e.message;
    }
}

function esc(s) {
    return (s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
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

fetchData();
setInterval(fetchData, 15000);
</script>
</body>
</html>"#;
