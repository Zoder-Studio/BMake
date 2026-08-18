import { createClient } from 'https://esm.sh/@supabase/supabase-js@2';

const SUPABASE_URL = 'https://bqtexxwyabicspfbpalf.supabase.co';
const SUPABASE_ANON_KEY =
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImJxdGV4eHd5YWJpY3NwZmJwYWxmIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODY4NTY3MTMsImV4cCI6MjEwMjQzMjcxM30.r6igXYU197wzWy-RB_unhnqoLRDJEZqW6nXoMY3tADc';

const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY);

let authMode = 'signin';
let runnersCache = null;

// ---------- auth ----------

async function refreshUserBox() {
  const {
    data: { session },
  } = await supabase.auth.getSession();
  const box = document.getElementById('user-box');

  if (session) {
    box.innerHTML = `<span class="user-email">${session.user.email}</span><button class="secondary-button" onclick="doLogout()">Sign out</button>`;
    document.getElementById('login-view').hidden = true;
    document.getElementById('app-view').hidden = false;
    showTab('runners');
  } else {
    box.innerHTML = '';
    document.getElementById('login-view').hidden = false;
    document.getElementById('app-view').hidden = true;
  }
}

window.setAuthMode = function (mode) {
  authMode = mode;
  document.getElementById('signin-tab').classList.toggle('auth-switch-active', mode === 'signin');
  document.getElementById('signup-tab').classList.toggle('auth-switch-active', mode === 'signup');

  const title = document.getElementById('auth-title');
  const subtitle = document.getElementById('auth-subtitle');
  const submit = document.getElementById('auth-submit');
  const confirmWrap = document.getElementById('confirm-password-wrap');
  const confirmInput = document.getElementById('confirm-password');
  const message = document.getElementById('auth-message');

  if (mode === 'signin') {
    title.textContent = 'Welcome back';
    subtitle.textContent = 'Sign in to your BMake Control Plane.';
    submit.textContent = 'Sign in';
    confirmWrap.hidden = true;
    confirmInput.required = false;
  } else {
    title.textContent = 'Create your account';
    subtitle.textContent = 'Set up a BMake Control Plane account.';
    submit.textContent = 'Create account';
    confirmWrap.hidden = false;
    confirmInput.required = true;
  }
  message.textContent = '';
  message.className = '';
};

window.handleAuth = async function (event) {
  event.preventDefault();
  const email = document.getElementById('email').value;
  const password = document.getElementById('password').value;
  const message = document.getElementById('auth-message');
  message.textContent = '';
  message.className = '';

  if (authMode === 'signup') {
    const confirm = document.getElementById('confirm-password').value;
    if (password !== confirm) {
      message.textContent = 'Passwords do not match';
      message.className = 'error-text';
      return;
    }
    const { data, error } = await supabase.auth.signUp({ email, password });
    if (error) {
      message.textContent = error.message;
      message.className = 'error-text';
      return;
    }
    if (!data.session) {
      message.textContent = 'Account created — check your email to confirm before signing in.';
      message.className = 'info-text';
      return;
    }
    await refreshUserBox();
    return;
  }

  const { error } = await supabase.auth.signInWithPassword({ email, password });
  if (error) {
    message.textContent = error.message;
    message.className = 'error-text';
    return;
  }
  await refreshUserBox();
};

window.doLogout = async function () {
  await supabase.auth.signOut();
  runnersCache = null;
  await refreshUserBox();
};

// ---------- tabs ----------

window.showTab = function (name) {
  const tabs = ['runners', 'secrets', 'builds', 'settings'];
  for (const t of tabs) {
    document.getElementById(`tab-${t}`).classList.toggle('active', t === name);
    document.getElementById(`view-${t}`).hidden = t !== name;
  }
  if (name === 'runners') loadRunners();
  if (name === 'secrets') loadSecrets();
  if (name === 'builds') loadBuilds();
  if (name === 'settings') loadSettings();
};

async function loadSettings() {
  const {
    data: { user },
  } = await supabase.auth.getUser();
  document.getElementById('settings-email').textContent = user ? user.email : '—';
}

// ---------- runners ----------

async function fetchRunnersCache() {
  if (runnersCache) return runnersCache;
  const { data, error } = await supabase.from('runners').select('id,name');
  runnersCache = error ? [] : data;
  return runnersCache;
}

async function loadRunners() {
  const { data, error } = await supabase.from('runners').select('*').order('created_at', { ascending: false });
  const body = document.getElementById('runners-body');
  body.innerHTML = '';
  if (error) {
    body.innerHTML = `<tr><td colspan="6">${error.message}</td></tr>`;
    return;
  }
  for (const r of data) {
    body.innerHTML += `<tr>
      <td>${r.name}</td><td>${r.runs_on}</td><td>${r.version}</td><td>${r.arch}</td>
      <td class="status-${r.status}">${r.status}</td>
      <td><code>${r.id}</code></td>
    </tr>`;
  }
}

window.openRunnerModal = function () {
  document.getElementById('runner-name').value = '';
  document.getElementById('runner-runs-on').value = '';
  document.getElementById('runner-version').value = '';
  document.getElementById('runner-arch').value = '';
  document.getElementById('runner-error').textContent = '';
  document.getElementById('runner-modal').hidden = false;
};

window.closeRunnerModal = function () {
  document.getElementById('runner-modal').hidden = true;
};

window.createRunner = async function (event) {
  event.preventDefault();
  const name = document.getElementById('runner-name').value.trim();
  const runsOn = document.getElementById('runner-runs-on').value.trim();
  const version = document.getElementById('runner-version').value.trim();
  const arch = document.getElementById('runner-arch').value.trim();
  const errorBox = document.getElementById('runner-error');
  errorBox.textContent = '';

  const {
    data: { user },
  } = await supabase.auth.getUser();

  const { error } = await supabase.from('runners').insert({
    owner: user.id,
    name,
    runs_on: runsOn,
    version,
    arch,
    status: 'OFFLINE',
  });

  if (error) {
    errorBox.textContent = error.message;
    return;
  }

  runnersCache = null;
  closeRunnerModal();
  loadRunners();
};

// ---------- secrets ----------

async function loadSecrets() {
  const { data, error } = await supabase.rpc('list_online_secrets');
  const container = document.getElementById('secrets-list');
  container.innerHTML = '';

  if (error) {
    container.innerHTML = `<p class="error-text">${error.message}</p>`;
    return;
  }
  if (!data.length) {
    container.innerHTML = '<p class="muted">No secrets yet.</p>';
    return;
  }

  const runners = await fetchRunnersCache();

  for (const row of data) {
    const { data: grantData } = await supabase.rpc('list_secret_grants', { p_secret_name: row.name });
    const grants = grantData || [];
    const grantedIds = new Set(grants.map((g) => g.runner_id));

    const chips =
      grants
        .map(
          (g) =>
            `<span class="chip">${g.runner_name}<button type="button" class="chip-x" onclick="revokeAccess('${row.name}','${g.runner_id}')">×</button></span>`
        )
        .join('') || '<span class="muted">No Runs-on granted yet</span>';

    const options = runners
      .filter((r) => !grantedIds.has(r.id))
      .map((r) => `<option value="${r.id}">${r.name}</option>`)
      .join('');

    const card = document.createElement('div');
    card.className = 'secret-card';
    card.innerHTML = `
      <div class="secret-card-header"><strong>${row.name}</strong></div>
      <div class="chip-row">${chips}</div>
      <div class="inline-grant">
        <select id="grant-select-${row.name}">
          <option value="">Grant to...</option>
          ${options}
        </select>
        <button type="button" class="secondary-button" onclick="grantAccess('${row.name}')">Grant</button>
      </div>
    `;
    container.appendChild(card);
  }
}

window.grantAccess = async function (secretName) {
  const select = document.getElementById(`grant-select-${secretName}`);
  const runnerId = select.value;
  if (!runnerId) return;
  const { error } = await supabase.rpc('grant_secret_to_runner', { secret_name: secretName, runner_id: runnerId });
  if (error) {
    alert(error.message);
    return;
  }
  loadSecrets();
};

window.revokeAccess = async function (secretName, runnerId) {
  const { error } = await supabase.rpc('revoke_secret_from_runner', {
    p_secret_name: secretName,
    p_runner_id: runnerId,
  });
  if (error) {
    alert(error.message);
    return;
  }
  loadSecrets();
};

window.openSecretModal = async function () {
  document.getElementById('secret-name').value = '';
  document.getElementById('secret-value').value = '';
  document.getElementById('secret-error').textContent = '';

  const select = document.getElementById('secret-runner');
  select.innerHTML = `
    <option value="">Don't grant yet (add access later)</option>
    <option value="__ALL__">Grant to ALL my Runs-on systems</option>
  `;
  const runners = await fetchRunnersCache();
  for (const r of runners) {
    const opt = document.createElement('option');
    opt.value = r.id;
    opt.textContent = r.name;
    select.appendChild(opt);
  }

  document.getElementById('secret-modal').hidden = false;
};

window.closeSecretModal = function () {
  document.getElementById('secret-modal').hidden = true;
};

window.createSecret = async function (event) {
  event.preventDefault();
  const name = document.getElementById('secret-name').value.trim();
  const value = document.getElementById('secret-value').value;
  const choice = document.getElementById('secret-runner').value;
  const errorBox = document.getElementById('secret-error');
  errorBox.textContent = '';

  const { error } = await supabase.rpc('add_online_secret', { secret_name: name, secret_value: value });
  if (error) {
    errorBox.textContent = error.message;
    return;
  }

  if (choice === '__ALL__') {
    const runners = await fetchRunnersCache();
    for (const r of runners) {
      await supabase.rpc('grant_secret_to_runner', { secret_name: name, runner_id: r.id });
    }
  } else if (choice) {
    const { error: grantError } = await supabase.rpc('grant_secret_to_runner', {
      secret_name: name,
      runner_id: choice,
    });
    if (grantError) {
      errorBox.textContent = grantError.message;
      return;
    }
  }

  closeSecretModal();
  loadSecrets();
};

// ---------- builds ----------

async function loadBuilds() {
  const { data, error } = await supabase.from('builds').select('*').order('created_at', { ascending: false }).limit(50);
  const body = document.getElementById('builds-body');
  body.innerHTML = '';
  if (error) {
    body.innerHTML = `<tr><td colspan="5">${error.message}</td></tr>`;
    return;
  }
  for (const b of data) {
    body.innerHTML += `<tr>
      <td class="status-${b.status}">${b.status}</td>
      <td>${b.runs_on || ''} ${b.runs_on_version || ''}</td>
      <td>${b.start_time || b.created_at}</td>
      <td>${b.duration_secs ?? ''}</td>
      <td><button type="button" class="secondary-button" onclick="showBuild('${b.id}')">Logs</button></td>
    </tr>`;
  }
}

window.showBuild = async function (buildId) {
  const { data, error } = await supabase
    .from('build_logs')
    .select('line')
    .eq('build_id', buildId)
    .order('id', { ascending: true });
  const detail = document.getElementById('build-detail');
  if (error) {
    detail.innerHTML = `<p class="error-text">${error.message}</p>`;
    return;
  }
  detail.innerHTML = `<h3>Build ${buildId}</h3><pre>${data.map((r) => r.line).join('\n')}</pre>`;
};

refreshUserBox();