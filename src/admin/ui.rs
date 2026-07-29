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
.container { max-width:1000px; margin:0 auto; }
h1 { font-size:24px; margin-bottom:20px; color:#f0f6fc; }
.stats { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-bottom:24px; }
.stat-card { background:#161b22; border:1px solid #30363d; border-radius:6px; padding:16px; }
.stat-card .label { font-size:12px; color:#8b949e; }
.stat-card .value { font-size:24px; font-weight:600; margin-top:4px; }
table { width:100%; border-collapse:collapse; background:#161b22; border:1px solid #30363d; border-radius:6px; overflow:hidden; }
th { background:#21262d; padding:10px 12px; text-align:left; font-size:12px; color:#8b949e; text-transform:uppercase; }
td { padding:10px 12px; border-top:1px solid #30363d; font-size:14px; }
.status-dot { display:inline-block; width:8px; height:8px; border-radius:50%; margin-right:6px; }
.status-ok { background:#3fb950; }
.status-warn { background:#d29922; }
.status-err { background:#f85149; }
.toggle-btn { background:#21262d; border:1px solid #30363d; color:#c9d1d9; padding:4px 10px; border-radius:4px; cursor:pointer; font-size:12px; white-space:nowrap; }
.toggle-btn:hover { background:#30363d; }
.toggle-btn.active { background:#1f6feb; border-color:#1f6feb; }
.form-section { background:#161b22; border:1px solid #30363d; border-radius:6px; padding:16px; margin-top:16px; }
.form-section h3 { margin-bottom:12px; color:#f0f6fc; }
input[type="text"], input[type="password"] { width:100%; background:#0d1117; border:1px solid #30363d; color:#c9d1d9; padding:8px 12px; border-radius:4px; margin-bottom:8px; }
button[type="submit"] { background:#238636; color:#fff; border:none; padding:8px 16px; border-radius:4px; cursor:pointer; }
button[type="submit"]:hover { background:#2ea043; }
.error { color:#f85149; margin-top:8px; }
.success { color:#3fb950; margin-top:8px; }
.overlay { display:none; position:fixed; inset:0; background:rgba(0,0,0,0.6); z-index:100; }
.overlay.open { display:flex; align-items:center; justify-content:center; }
.modal { background:#161b22; border:1px solid #30363d; border-radius:8px; padding:24px; width:480px; max-width:90vw; max-height:90vh; overflow-y:auto; }
.modal h3 { margin-bottom:16px; color:#f0f6fc; }
.modal label { display:block; font-size:12px; color:#8b949e; margin-bottom:4px; margin-top:12px; }
.modal label:first-of-type { margin-top:0; }
.modal .modal-actions { display:flex; gap:8px; margin-top:16px; }
</style>
</head>
<body>
<div class="container">
<h1>HiFi API Admin</h1>
<div id="stats" class="stats"></div>
<div id="error" class="error"></div>
<div id="success" class="success"></div>
<h2>Accounts</h2>
<table><thead><tr>
<th>Label</th><th>User ID</th><th>Status</th><th>Requests</th><th>Errors</th><th>Rate Limited</th><th>Token Expires</th><th>Action</th>
</tr></thead><tbody id="accounts-tbody"></tbody></table>
<div class="form-section">
<h3>Add Account</h3>
<input type="text" id="new-label" placeholder="Label (optional)">
<input type="text" id="new-client-id" placeholder="Client ID">
<input type="password" id="new-client-secret" placeholder="Client Secret">
<input type="password" id="new-refresh-token" placeholder="Refresh Token">
<button onclick="addAccount()">Add Account</button>
</div>
</div>

<div id="editOverlay" class="overlay" onclick="if(event.target===this)closeEdit()">
<div class="modal">
<h3 id="editTitle">Edit Account</h3>
<label>Label</label>
<input type="text" id="ed-label">
<label>Client ID</label>
<input type="text" id="ed-client-id">
<label>Client Secret</label>
<input type="password" id="ed-client-secret">
<label>Refresh Token</label>
<input type="password" id="ed-refresh-token">
<div class="modal-actions">
<button class="toggle-btn" onclick="saveEdit()" style="background:#238636;color:#fff">Save</button>
<button class="toggle-btn" onclick="closeEdit()">Cancel</button>
</div>
</div>
</div>

<script>
const API = window.location.origin;
let adminKey = localStorage.getItem('admin_key') || '';
let editId = null;

function headers() {
    return { 'Content-Type': 'application/json', ...(adminKey ? { 'X-Admin-Key': adminKey } : {}) };
}

function setKey() {
    const k = prompt('Enter admin key:', adminKey);
    if (k) { adminKey = k; localStorage.setItem('admin_key', k); fetchData(); }
}
if (!adminKey) setKey();

function statusClass(a) {
    const now = Math.floor(Date.now() / 1000);
    if (!a.is_active) return 'status-err';
    if (a.rate_limited_until > now) return 'status-warn';
    return 'status-ok';
}

function timeStr(ts) {
    if (!ts || ts === 0) return '-';
    const d = new Date(ts * 1000);
    const diff = d - new Date();
    if (diff < 0) return 'expired';
    const mins = Math.floor(diff / 60000);
    if (mins < 60) return mins + 'm';
    return Math.floor(mins / 60) + 'h ' + (mins % 60) + 'm';
}

function openEdit(id) {
    editId = id;
    const a = window._accounts.find(x => x.id === id);
    if (!a) return;
    document.getElementById('ed-label').value = a.label || '';
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
    const id = editId;
    if (!id) return;
    const body = {
        label: document.getElementById('ed-label').value,
        client_id: document.getElementById('ed-client-id').value,
        client_secret: document.getElementById('ed-client-secret').value,
        refresh_token: document.getElementById('ed-refresh-token').value,
    };
    try {
        const res = await fetch('/admin/accounts/' + id, {
            method: 'PATCH', headers: headers(), body: JSON.stringify(body)
        });
        if (res.ok) {
            closeEdit();
            fetchData();
        } else {
            const d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function fetchData() {
    try {
        const [statsRes, accountsRes] = await Promise.all([
            fetch('/admin/stats', { headers: headers() }),
            fetch('/admin/accounts', { headers: headers() })
        ]);
        if (statsRes.status === 401 || accountsRes.status === 401) { setKey(); return; }

        if (!statsRes.ok) {
            var text = await statsRes.text();
            document.getElementById('error').textContent = 'Stats API: ' + (statsRes.status) + ' ' + text.slice(0, 200);
            return;
        }
        if (!accountsRes.ok) {
            var text = await accountsRes.text();
            document.getElementById('error').textContent = 'Accounts API: ' + (accountsRes.status) + ' ' + text.slice(0, 200);
            return;
        }

        var statsText = await statsRes.text();
        var accountsText = await accountsRes.text();
        var stats, accounts;
        try { stats = JSON.parse(statsText); } catch(e) { document.getElementById('error').textContent = 'Stats parse error: ' + statsText.slice(0, 200); return; }
        try { accounts = JSON.parse(accountsText); } catch(e) { document.getElementById('error').textContent = 'Accounts parse error: ' + accountsText.slice(0, 200); return; }
        window._accounts = accounts.accounts;

        document.getElementById('stats').innerHTML = [
            '<div class="stat-card"><div class="label">Total Requests</div><div class="value">' + stats.total_requests + '</div></div>',
            '<div class="stat-card"><div class="label">Error Rate</div><div class="value">' + (stats.error_rate || '0.00%') + '</div></div>',
            '<div class="stat-card"><div class="label">Active</div><div class="value">' + (stats.healthy_accounts || 0) + '/' + (stats.total_accounts || 0) + '</div></div>',
            '<div class="stat-card"><div class="label">Rate Limited</div><div class="value">' + (stats.rate_limited_accounts || 0) + '</div></div>'
        ].join('');

        document.getElementById('accounts-tbody').innerHTML = accounts.accounts.map(function(a) {
            var label = a.label || a.id.slice(0, 8);
            var userId = a.user_id || '-';
            var statusDot = 'status-dot ' + statusClass(a);
            var statusText = a.is_active ? 'Active' : 'Inactive';
            var activeClass = a.is_active ? ' active' : '';
            var toggleText = a.is_active ? 'ON' : 'OFF';

            return '<tr>' +
                '<td>' + esc(label) + '</td>' +
                '<td>' + esc(userId) + '</td>' +
                '<td><span class="' + statusDot + '"></span>' + statusText + '</td>' +
                '<td>' + a.request_count + '</td>' +
                '<td>' + a.error_count + '</td>' +
                '<td>' + timeStr(a.rate_limited_until) + '</td>' +
                '<td>' + timeStr(a.token_expires_at) + '</td>' +
                '<td nowrap>' +
                    '<button class="toggle-btn" onclick="openEdit(\'' + a.id + '\')">Edit</button> ' +
                    '<button class="toggle-btn' + activeClass + '" onclick="toggleAccount(\'' + a.id + '\',' + (!a.is_active) + ')">' + toggleText + '</button> ' +
                    '<button class="toggle-btn" onclick="removeAccount(\'' + a.id + '\')">Delete</button>' +
                '</td></tr>';
        }).join('');
    } catch(e) {
        document.getElementById('error').textContent = 'Failed to fetch data: ' + e.message;
    }
}

function esc(s) {
    return (s || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

async function toggleAccount(id, active) {
    try {
        const res = await fetch('/admin/accounts/' + id + '/toggle', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ active })
        });
        const data = await res.json();
        if (res.ok) fetchData();
        else document.getElementById('error').textContent = data.detail || 'Error';
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function removeAccount(id) {
    if (!confirm('Delete this account?')) return;
    try {
        const res = await fetch('/admin/accounts/' + id, {
            method: 'DELETE', headers: headers()
        });
        if (res.ok) fetchData();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function addAccount() {
    var label = document.getElementById('new-label').value;
    var client_id = document.getElementById('new-client-id').value;
    var client_secret = document.getElementById('new-client-secret').value;
    var refresh_token = document.getElementById('new-refresh-token').value;

    if (!client_id || !client_secret || !refresh_token) {
        document.getElementById('error').textContent = 'Client ID, secret, and refresh token are required';
        return;
    }

    try {
        const res = await fetch('/admin/accounts', {
            method: 'POST', headers: headers(),
            body: JSON.stringify({ label: label, client_id: client_id, client_secret: client_secret, refresh_token: refresh_token })
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Account added!';
            fetchData();
            document.getElementById('new-label').value = '';
            document.getElementById('new-client-id').value = '';
            document.getElementById('new-client-secret').value = '';
            document.getElementById('new-refresh-token').value = '';
        } else {
            const data = await res.json();
            document.getElementById('error').textContent = data.detail || 'Error';
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
