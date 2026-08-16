use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub supabase_url: String,
    pub anon_key: String,
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub email: String,
}

fn credentials_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".bmake").join("credentials.toml")
}

pub fn load_session() -> Result<Option<Session>> {
    let path = credentials_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(Some(toml::from_str(&content)?))
}

fn save_session(session: &Session) -> Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(session)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn clear_session() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Authenticates against Supabase Auth's password grant and stores the
/// resulting session at ~/.bmake/credentials.toml — never inside the
/// project directory, so it can never end up committed to a repo.
pub fn login(supabase_url: &str, anon_key: &str, email: &str, password: &str) -> Result<Session> {
    let url = format!("{}/auth/v1/token?grant_type=password", supabase_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .header("apikey", anon_key)
        .json(&json!({ "email": email, "password": password }))
        .send()
        .context("Failed to reach Supabase Auth")?;

    if !resp.status().is_success() {
        bail!("Login failed: {}", resp.text().unwrap_or_default());
    }

    let body: Value = resp.json()?;
    let access_token = body["access_token"].as_str().unwrap_or_default().to_string();
    let refresh_token = body["refresh_token"].as_str().unwrap_or_default().to_string();
    let user_id = body["user"]["id"].as_str().unwrap_or_default().to_string();

    if access_token.is_empty() || user_id.is_empty() {
        bail!("Login response was missing access_token or user id");
    }

    let session = Session {
        supabase_url: supabase_url.trim_end_matches('/').to_string(),
        anon_key: anon_key.to_string(),
        access_token,
        refresh_token,
        user_id,
        email: email.to_string(),
    };
    save_session(&session)?;
    Ok(session)
}

fn rest_url(session: &Session, path: &str) -> String {
    format!("{}/rest/v1/{}", session.supabase_url, path)
}

fn auth_headers(session: &Session) -> Vec<(&'static str, String)> {
    vec![
        ("apikey", session.anon_key.clone()),
        ("Authorization", format!("Bearer {}", session.access_token)),
    ]
}

pub fn register_runner(session: &Session, name: &str, runs_on: &str, version: &str, arch: &str) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let mut req = client
        .post(rest_url(session, "runners"))
        .header("Prefer", "return=representation")
        .json(&json!({
            "owner": session.user_id, "name": name, "runs_on": runs_on,
            "version": version, "arch": arch, "status": "OFFLINE",
        }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to register runner: {}", resp.text().unwrap_or_default());
    }
    let mut body: Value = resp.json()?;
    Ok(body.as_array_mut().and_then(|a| a.pop()).unwrap_or(Value::Null))
}

pub fn set_runner_status(session: &Session, runner_id: &str, status: &str) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let mut req = client
        .patch(format!("{}?id=eq.{}", rest_url(session, "runners"), runner_id))
        .json(&json!({ "status": status }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to update runner status: {}", resp.text().unwrap_or_default());
    }
    Ok(())
}

pub fn list_runners(session: &Session) -> Result<Vec<Value>> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(rest_url(session, "runners?select=*"));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to list runners: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.json()?)
}

pub fn find_matching_runner(session: &Session, runs_on: &str, version: Option<&str>, arch: Option<&str>) -> Result<Option<Value>> {
    let mut query = format!("runners?select=*&status=eq.ONLINE&runs_on=eq.{}", urlencode(runs_on));
    if let Some(v) = version {
        query.push_str(&format!("&version=eq.{}", urlencode(v)));
    }
    if let Some(a) = arch {
        query.push_str(&format!("&arch=eq.{}", urlencode(a)));
    }
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(rest_url(session, &query));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to query runners: {}", resp.text().unwrap_or_default());
    }
    let mut list: Vec<Value> = resp.json()?;
    Ok(if list.is_empty() { None } else { Some(list.remove(0)) })
}

pub fn create_build(session: &Session, runner_id: Option<&str>) -> Result<String> {
    let client = reqwest::blocking::Client::new();
    let mut req = client
        .post(rest_url(session, "builds"))
        .header("Prefer", "return=representation")
        .json(&json!({ "owner": session.user_id, "runner_id": runner_id, "status": "PENDING" }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to create build: {}", resp.text().unwrap_or_default());
    }
    let mut body: Value = resp.json()?;
    let id = body
        .as_array_mut()
        .and_then(|a| a.pop())
        .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if id.is_empty() {
        bail!("Build creation did not return an id");
    }
    Ok(id)
}

pub fn update_build(session: &Session, build_id: &str, patch: Value) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.patch(format!("{}?id=eq.{}", rest_url(session, "builds"), build_id)).json(&patch);
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to update build: {}", resp.text().unwrap_or_default());
    }
    Ok(())
}

pub fn create_job(session: &Session, runner_id: &str, bm_content: &str, build_id: Option<&str>) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let mut req = client
        .post(rest_url(session, "jobs"))
        .header("Prefer", "return=representation")
        .json(&json!({
            "owner": session.user_id, "runner_id": runner_id,
            "bm_content": bm_content, "build_id": build_id, "status": "PENDING",
        }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to create job: {}", resp.text().unwrap_or_default());
    }
    let mut body: Value = resp.json()?;
    Ok(body.as_array_mut().and_then(|a| a.pop()).unwrap_or(Value::Null))
}

pub fn get_job(session: &Session, job_id: &str) -> Result<Option<Value>> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(rest_url(session, &format!("jobs?id=eq.{}&select=*", job_id)));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to fetch job: {}", resp.text().unwrap_or_default());
    }
    let mut list: Vec<Value> = resp.json()?;
    Ok(if list.is_empty() { None } else { Some(list.remove(0)) })
}

pub fn list_pending_jobs(session: &Session, runner_id: &str) -> Result<Vec<Value>> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(rest_url(
        session,
        &format!("jobs?runner_id=eq.{}&status=eq.PENDING&select=*&order=created_at.asc", runner_id),
    ));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to list pending jobs: {}", resp.text().unwrap_or_default());
    }
    Ok(resp.json()?)
}

/// Atomically claims a PENDING job via the `claim_job` RPC, so two runners
/// racing on the same job can't both start executing it.
pub fn claim_job(session: &Session, job_id: &str) -> Result<Option<Value>> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.post(format!("{}/rest/v1/rpc/claim_job", session.supabase_url)).json(&json!({ "job_id": job_id }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to claim job: {}", resp.text().unwrap_or_default());
    }
    let mut list: Vec<Value> = resp.json()?;
    Ok(if list.is_empty() { None } else { Some(list.remove(0)) })
}

pub fn update_job_status(session: &Session, job_id: &str, status: &str) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.patch(format!("{}?id=eq.{}", rest_url(session, "jobs"), job_id)).json(&json!({ "status": status }));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to update job status: {}", resp.text().unwrap_or_default());
    }
    Ok(())
}

pub fn append_log(session: &Session, build_id: &str, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let rows: Vec<Value> = lines.iter().map(|l| json!({ "build_id": build_id, "line": l })).collect();
    let client = reqwest::blocking::Client::new();
    let mut req = client.post(rest_url(session, "build_logs")).json(&rows);
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to upload logs: {}", resp.text().unwrap_or_default());
    }
    Ok(())
}

pub fn fetch_logs_since(session: &Session, build_id: &str, offset: usize) -> Result<Vec<String>> {
    let client = reqwest::blocking::Client::new();
    let mut req = client.get(rest_url(
        session,
        &format!("build_logs?build_id=eq.{}&select=line&order=id.asc&offset={}", build_id, offset),
    ));
    for (k, v) in auth_headers(session) {
        req = req.header(k, v);
    }
    let resp = req.send()?;
    if !resp.status().is_success() {
        bail!("Failed to fetch logs: {}", resp.text().unwrap_or_default());
    }
    let rows: Vec<Value> = resp.json()?;
    Ok(rows.into_iter().filter_map(|r| r["line"].as_str().map(|s| s.to_string())).collect())
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c.to_string() } else { format!("%{:02X}", c as u32) })
        .collect()
}