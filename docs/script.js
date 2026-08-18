import { createClient } from 'https://esm.sh/@supabase/supabase-js@2';

const SUPABASE_URL = 'https://bqtexxwyabicspfbpalf.supabase.co';
const SUPABASE_ANON_KEY =
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImJxdGV4eHd5YWJpY3NwZmJwYWxmIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODY4NTY3MTMsImV4cCI6MjEwMjQzMjcxM30.r6igXYU197wzWy-RB_unhnqoLRDJEZqW6nXoMY3tADc';

const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY);

async function refreshUserBox() {
  const {
    data: { session },
  } = await supabase.auth.getSession();
  const box = document.getElementById('user-box');

  if (session) {
    box.innerHTML = `${session.user.email} <button class="secondary" onclick="doLogout()">Sign out</button>`;
    document.getElementById('login-view').style.display = 'none';
    document.getElementById('app-view').style.display = 'block';
    await loadRunners();
    await loadBuilds();
  } else {
    box.innerHTML = '';
    document.getElementById('login-view').style.display = 'block';
    document.getElementById('app-view').style.display = 'none';
  }
}

window.doLogin = async function () {
  const email = document.getElementById('email').value;
  const password = document.getElementById('password').value;
  const { error } = await supabase.auth.signInWithPassword({ email, password });
  document.getElementById('login-error').textContent = error ? error.message : '';
  if (!error) await refreshUserBox();
};

window.doLogout = async function () {
  await supabase.auth.signOut();
  await refreshUserBox();
};

window.showTab = function (name) {
  document.getElementById('tab-runners').classList.toggle('active', name === 'runners');
  document.getElementById('tab-builds').classList.toggle('active', name === 'builds');
  document.getElementById('tab-secrets').classList.toggle('active', name === 'secrets');
  document.getElementById('view-runners').style.display = name === 'runners' ? 'block' : 'none';
  document.getElementById('view-builds').style.display = name === 'builds' ? 'block' : 'none';
  document.getElementById('view-secrets').style.display = name === 'secrets' ? 'block' : 'none';
  if (name === 'secrets') loadSecrets();
};

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
      <td><button class="secondary" onclick="showBuild('${b.id}')">Logs</button></td>
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
    detail.innerHTML = `<p>${error.message}</p>`;
    return;
  }
  detail.innerHTML = `<h3>Build ${buildId}</h3><pre>${data.map((r) => r.line).join('\n')}</pre>`;
};

async function loadSecrets() {
  const { data, error } = await supabase.rpc('list_online_secrets');
  const body = document.getElementById('secrets-body');
  body.innerHTML = '';
  if (error) {
    body.innerHTML = `<tr><td>${error.message}</td></tr>`;
    return;
  }
  for (const row of data) {
    body.innerHTML += `<tr><td>${row.name}</td></tr>`;
  }
}

window.addSecret = async function () {
  const name = document.getElementById('secret-name').value;
  const value = document.getElementById('secret-value').value;
  const { error } = await supabase.rpc('add_online_secret', { secret_name: name, secret_value: value });
  document.getElementById('secret-add-error').textContent = error ? error.message : '';
  if (!error) {
    document.getElementById('secret-name').value = '';
    document.getElementById('secret-value').value = '';
    loadSecrets();
  }
};

window.grantSecret = async function () {
  const name = document.getElementById('grant-secret-name').value;
  const runnerId = document.getElementById('grant-runner-id').value;
  const { error } = await supabase.rpc('grant_secret_to_runner', { secret_name: name, runner_id: runnerId });
  document.getElementById('secret-grant-error').textContent = error ? error.message : '';
};

refreshUserBox();