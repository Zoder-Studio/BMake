import {
  createClient
} from "https://esm.sh/@supabase/supabase-js@2";


/*
 * BMake Control Plane
 *
 * IMPORTANT:
 * The anon key is safe to expose in frontend code.
 * Security MUST be enforced by Supabase RLS.
 */

const SUPABASE_URL =
  "https://bqtexxwyabicspfbpalf.supabase.co";

const SUPABASE_ANON_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJxdGV4eHh3YWJpY3NwZmJwYWxmIiwic3ViIjoiYW5vbiIsImlhdCI6MTc4Njg1NjcxMywiZXhwIjoyMTAyNDMyNzEzfQ.r6igXYU197wzWy-RB_unhnqoLRDJEZqW6nXoMY3tADc";


const supabase = createClient(
  SUPABASE_URL,
  SUPABASE_ANON_KEY
);


let authMode = "signin";


/* =========================================
   AUTH
========================================= */

window.setAuthMode = function (mode) {

  authMode = mode;

  const signin =
    document.getElementById("signin-tab");

  const signup =
    document.getElementById("signup-tab");

  const title =
    document.getElementById("auth-title");

  const subtitle =
    document.getElementById("auth-subtitle");

  const submit =
    document.getElementById("auth-submit");

  const confirm =
    document.getElementById("confirm-password-wrap");

  const message =
    document.getElementById("auth-message");

  message.textContent = "";
  message.className = "";

  signin.classList.toggle(
    "auth-switch-active",
    mode === "signin"
  );

  signup.classList.toggle(
    "auth-switch-active",
    mode === "signup"
  );


  if (mode === "signin") {

    title.textContent = "Welcome back";

    subtitle.textContent =
      "Sign in to your BMake Control Plane.";

    submit.textContent = "Sign in";

    confirm.style.display = "none";

  } else {

    title.textContent =
      "Create your account";

    subtitle.textContent =
      "Create an account for BMake Control Plane.";

    submit.textContent =
      "Create account";

    confirm.style.display =
      "flex";

  }

};


window.handleAuth = async function (event) {

  event.preventDefault();

  const email =
    document.getElementById("email")
      .value
      .trim();

  const password =
    document.getElementById("password")
      .value;

  const confirmPassword =
    document.getElementById("confirm-password")
      .value;

  const submit =
    document.getElementById("auth-submit");

  const message =
    document.getElementById("auth-message");


  message.textContent = "";

  submit.disabled = true;


  try {

    if (authMode === "signup") {

      if (password !== confirmPassword) {

        throw new Error(
          "Passwords do not match."
        );

      }


      const {
        data,
        error
      } = await supabase.auth.signUp({
        email,
        password
      });


      if (error) {
        throw error;
      }


      if (!data.session) {

        message.className =
          "status-SUCCESS";

        message.textContent =
          "Account created. Check your email to confirm your account.";

        return;
      }


      await refreshUserBox();

      return;
    }


    const {
      error
    } = await supabase.auth.signInWithPassword({
      email,
      password
    });


    if (error) {
      throw error;
    }


    await refreshUserBox();


  } catch (error) {

    message.className =
      "status-ERROR";

    message.textContent =
      error.message ||
      "Authentication failed.";

  } finally {

    submit.disabled = false;

  }

};


window.doLogout = async function () {

  await supabase.auth.signOut();

  await refreshUserBox();

};


/* =========================================
   USER
========================================= */

async function refreshUserBox() {

  const {
    data: {
      session
    }
  } = await supabase.auth.getSession();


  const userBox =
    document.getElementById("user-box");


  if (!session) {

    userBox.innerHTML = "";

    document.getElementById(
      "login-view"
    ).style.display = "flex";

    document.getElementById(
      "app-view"
    ).style.display = "none";

    return;
  }


  userBox.innerHTML = `
    <span>${escapeHtml(
      session.user.email
    )}</span>

    <button
      class="secondary-button"
      onclick="doLogout()"
    >
      Sign out
    </button>
  `;


  document.getElementById(
    "login-view"
  ).style.display = "none";


  document.getElementById(
    "app-view"
  ).style.display = "block";


  document.getElementById(
    "settings-email"
  ).textContent =
    session.user.email;


  await Promise.all([
    loadRunners(),
    loadSecrets(),
    loadBuilds()
  ]);

}


/* =========================================
   TABS
========================================= */

window.showTab = function (name) {

  const tabs = [
    "runners",
    "secrets",
    "builds",
    "settings"
  ];


  for (const tab of tabs) {

    const button =
      document.getElementById(
        `tab-${tab}`
      );

    const view =
      document.getElementById(
        `view-${tab}`
      );


    button.classList.toggle(
      "active",
      tab === name
    );

    view.hidden =
      tab !== name;

  }

};


/* =========================================
   RUNNERS
========================================= */

async function loadRunners() {

  const body =
    document.getElementById(
      "runners-body"
    );


  const {
    data,
    error
  } = await supabase
    .from("runners")
    .select("*")
    .order(
      "created_at",
      {
        ascending: false
      }
    );


  if (error) {

    body.innerHTML = `
      <tr>
        <td colspan="6">
          ${escapeHtml(error.message)}
        </td>
      </tr>
    `;

    return;
  }


  body.innerHTML = "";


  for (const runner of data || []) {

    body.innerHTML += `
      <tr>

        <td>
          ${escapeHtml(runner.name)}
        </td>

        <td>
          ${escapeHtml(runner.runs_on)}
        </td>

        <td>
          ${escapeHtml(
            runner.version || "—"
          )}
        </td>

        <td>
          ${escapeHtml(
            runner.arch || "—"
          )}
        </td>

        <td class="status-${escapeHtml(
          runner.status
        )}">
          ${escapeHtml(
            runner.status
          )}
        </td>

        <td>
          <code>
            ${escapeHtml(runner.id)}
          </code>
        </td>

      </tr>
    `;

  }


  await loadRunnerOptions(data || []);

}


window.openRunnerModal = function () {

  document.getElementById(
    "runner-modal"
  ).hidden = false;

};


window.closeRunnerModal = function () {

  document.getElementById(
    "runner-modal"
  ).hidden = true;

};


window.createRunner = async function (event) {

  event.preventDefault();


  const name =
    document.getElementById(
      "runner-name"
    ).value.trim();

  const runsOn =
    document.getElementById(
      "runner-runs-on"
    ).value.trim();

  const version =
    document.getElementById(
      "runner-version"
    ).value.trim();

  const arch =
    document.getElementById(
      "runner-arch"
    ).value.trim();


  const {
    error
  } = await supabase
    .from("runners")
    .insert({
      name,
      runs_on: runsOn,
      version,
      arch,
      status: "OFFLINE"
    });


  if (error) {

    document.getElementById(
      "runner-error"
    ).textContent =
      error.message;

    return;
  }


  document.querySelector(
    "#runner-modal form"
  ).reset();


  closeRunnerModal();

  await loadRunners();

};


/* =========================================
   SECRET UI
========================================= */

async function loadSecrets() {

  const list =
    document.getElementById(
      "secrets-list"
    );


  /*
   * IMPORTANT:
   *
   * This query assumes that the backend stores
   * secret metadata separately from secret values.
   *
   * NEVER select encrypted/plaintext secret
   * values into this frontend.
   */

  const {
    data,
    error
  } = await supabase
    .from("secrets")
    .select(
      "id,name,runner_id,created_at"
    )
    .order(
      "created_at",
      {
        ascending: false
      }
    );


  if (error) {

    list.innerHTML = `
      <div class="secret-item">
        ${escapeHtml(error.message)}
      </div>
    `;

    return;
  }


  list.innerHTML = "";


  for (const secret of data || []) {

    list.innerHTML += `
      <div class="secret-item">

        <div>

          <div class="secret-name">
            ${escapeHtml(secret.name)}
          </div>

          <small>
            Created ${escapeHtml(
              secret.created_at || ""
            )}
          </small>

        </div>

        <span>
          ••••••••
        </span>

      </div>
    `;

  }


  if (!data?.length) {

    list.innerHTML = `
      <div class="secret-item">
        No secrets have been created yet.
      </div>
    `;

  }

}


window.openSecretModal = async function () {

  await loadRunners();

  document.getElementById(
    "secret-modal"
  ).hidden = false;

};


window.closeSecretModal = function () {

  document.getElementById(
    "secret-modal"
  ).hidden = true;

};


window.createSecret = async function (event) {

  event.preventDefault();


  const name =
    document.getElementById(
      "secret-name"
    ).value.trim();

  const value =
    document.getElementById(
      "secret-value"
    ).value;

  const runnerId =
    document.getElementById(
      "secret-runner"
    ).value;


  if (!name || !value) {

    document.getElementById(
      "secret-error"
    ).textContent =
      "Secret name and value are required.";

    return;
  }


  /*
   * IMPORTANT SECURITY NOTE:
   *
   * Do NOT store the actual secret value
   * directly in a normal Supabase table.
   *
   * Production implementation should send
   * this through an Edge Function/backend
   * that encrypts it using a server-side key.
   *
   * The browser must never receive the
   * encryption master key.
   */

  const {
    error
  } = await supabase.functions.invoke(
    "secret-create",
    {
      body: {
        name,
        value,
        runner_id:
          runnerId || null
      }
    }
  );


  if (error) {

    document.getElementById(
      "secret-error"
    ).textContent =
      error.message;

    return;
  }


  document.querySelector(
    "#secret-modal form"
  ).reset();


  closeSecretModal();

  await loadSecrets();

};


/* =========================================
   RUNNER OPTIONS
========================================= */

async function loadRunnerOptions(
  runners
) {

  const select =
    document.getElementById(
      "secret-runner"
    );


  select.innerHTML = `
    <option value="">
      All permitted systems
    </option>
  `;


  for (const runner of runners) {

    const option =
      document.createElement("option");

    option.value =
      runner.id;

    option.textContent =
      `${runner.name} (${runner.runs_on})`;

    select.appendChild(option);

  }

}


/* =========================================
   BUILDS
========================================= */

async function loadBuilds() {

  const body =
    document.getElementById(
      "builds-body"
    );


  const {
    data,
    error
  } = await supabase
    .from("builds")
    .select("*")
    .order(
      "created_at",
      {
        ascending: false
      }
    )
    .limit(50);


  if (error) {

    body.innerHTML = `
      <tr>
        <td colspan="5">
          ${escapeHtml(error.message)}
        </td>
      </tr>
    `;

    return;
  }


  body.innerHTML = "";


  for (const build of data || []) {

    body.innerHTML += `
      <tr>

        <td class="status-${escapeHtml(
          build.status
        )}">
          ${escapeHtml(
            build.status
          )}
        </td>

        <td>
          ${escapeHtml(
            build.runs_on || ""
          )}

          ${escapeHtml(
            build.runs_on_version || ""
          )}
        </td>

        <td>
          ${escapeHtml(
            build.start_time ||
            build.created_at ||
            ""
          )}
        </td>

        <td>
          ${escapeHtml(
            build.duration_secs ??
            ""
          )}
        </td>

        <td>
          <button
            class="secondary-button"
            onclick="showBuild('${escapeHtml(
              build.id
            )}')"
          >
            Logs
          </button>
        </td>

      </tr>
    `;

  }

}


window.showBuild = async function (
  buildId
) {

  const detail =
    document.getElementById(
      "build-detail"
    );


  const {
    data,
    error
  } = await supabase
    .from("build_logs")
    .select("line")
    .eq("build_id", buildId)
    .order(
      "id",
      {
        ascending: true
      }
    );


  if (error) {

    detail.innerHTML =
      `<p>${escapeHtml(
        error.message
      )}</p>`;

    return;
  }


  const log =
    (data || [])
      .map(row => row.line)
      .join("\n");


  detail.innerHTML = `
    <h3>Build ${escapeHtml(
      buildId
    )}</h3>

    <pre>${escapeHtml(
      log
    )}</pre>
  `;

};


/* =========================================
   SECURITY HELPERS
========================================= */

function escapeHtml(value) {

  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");

}


/* =========================================
   AUTH STATE
========================================= */

supabase.auth.onAuthStateChange(
  async () => {
    await refreshUserBox();
  }
);


refreshUserBox();