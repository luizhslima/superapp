const out = document.getElementById("out");

// E1.3: abre 2 micro-frontends, trava o A e mede se o shell e o B continuam respondendo.
async function mfeTest() {
  const bc = new BroadcastChannel("mfe");
  const ready = new Set();
  const pongs = [];
  bc.onmessage = (e) => {
    if (e.data.type === "ready") ready.add(e.data.from);
    if (e.data.type === "pong") pongs.push(e.data);
  };
  const t = {};
  t.open_a = await window.host("shell.open_mfe", { id: "a" });
  t.open_b = await window.host("shell.open_mfe", { id: "b" });
  t.open_bad = await window.host("shell.open_mfe", { id: "../x" });
  for (let i = 0; i < 100 && ready.size < 2; i++) await new Promise((r) => setTimeout(r, 100));
  t.ready = [...ready].sort();
  bc.postMessage({ type: "hang", target: "a" });
  await new Promise((r) => setTimeout(r, 500));
  // Com o A travado: shell responde ao host? B responde por BroadcastChannel? A responde (não deve)?
  const t0 = performance.now();
  t.shell_rpc = await window.host("app.info");
  t.shell_rpc_ms = Math.round(performance.now() - t0);
  bc.postMessage({ type: "ping", n: 1 });
  await new Promise((r) => setTimeout(r, 1500));
  t.pong_from = pongs.map((p) => p.from).sort();
  return t;
}

// E1.4: canal binário pelo Router: round-trip, limites, comando desconhecido e vazão.
async function bytesTest() {
  const MiB = 1024 * 1024;
  const t = {};
  const sum = (u) => u.reduce((a, b) => (a + b) | 0, 0);
  const small = new Uint8Array([1, 2, 3, 250]);
  const e1 = await window.hostBytes("blob.echo", small);
  t.echo_small = e1.ok && e1.value.length === 4 && e1.value[3] === 250;
  const mid = new Uint8Array(MiB).map((_, i) => i & 255);
  const e2 = await window.hostBytes("blob.echo", mid);
  t.echo_1mib = e2.ok && e2.value.length === MiB && sum(e2.value) === sum(mid);
  t.unknown = await window.hostBytes("blob.nope", small);
  t.over_in = await window.hostBytes("blob.echo", new Uint8Array(2 * MiB)); // > 1 MiB: barrado pelo hook
  const asked = (n) => new Uint8Array(new Uint32Array([n]).buffer);
  t.over_out = await window.hostBytes("blob.gen", asked(32 * MiB));
  t.bad_gen = await window.hostBytes("blob.gen", new Uint8Array([1, 2]));
  // Vazão (10 MiB): saída (host→UI) e ida e volta.
  const big = new Uint8Array(10 * MiB);
  let t0 = performance.now();
  const g = await window.hostBytes("blob.gen", asked(10 * MiB));
  t.gen_ms = Math.round(performance.now() - t0);
  t.gen_ok = g.ok && g.value.length === 10 * MiB;
  // Só envio (UI→host): escala com o tamanho? (linear = custo por byte; maior = custo fixo)
  t.up = {};
  for (const mib of [0.0625, 0.25, 1]) {
    const buf = new Uint8Array(Math.round(mib * MiB));
    const t1 = performance.now();
    const r = await window.hostBytes("blob.sink", buf);
    t.up[mib + "MiB"] = r.ok ? Math.round(performance.now() - t1) : "erro";
  }
  // Mesmo teste pelo esquema sapp:// (sem Isolation).
  const f = {};
  const fe = await window.hostBlob("blob.echo", mid);
  f.echo_1mib = fe.ok && fe.value.length === MiB && sum(fe.value) === sum(mid);
  f.unknown = await window.hostBlob("blob.nope", small);
  f.over_in = await window.hostBlob("blob.echo", new Uint8Array(17 * MiB));
  f.over_out = await window.hostBlob("blob.gen", asked(32 * MiB));
  f.bad_gen = await window.hostBlob("blob.gen", new Uint8Array([1, 2]));
  t0 = performance.now();
  const fg = await window.hostBlob("blob.gen", asked(10 * MiB));
  f.gen_ms = Math.round(performance.now() - t0);
  f.gen_ok = fg.ok && fg.value.length === 10 * MiB;
  t0 = performance.now();
  const fe10 = await window.hostBlob("blob.echo", big);
  f.echo_ms = Math.round(performance.now() - t0);
  f.echo10_ok = fe10.ok && fe10.value.length === 10 * MiB;
  t0 = performance.now();
  await window.hostBlob("blob.sink", big);
  f.up_ms = Math.round(performance.now() - t0);
  t.blob = f;
  return t;
}

async function smoke() {
  // Resultado da janela sem capability chega por BroadcastChannel.
  const bc = new BroadcastChannel("smoke");
  const untrusted = new Promise((resolve) => {
    bc.onmessage = (e) => resolve(e.data);
    setTimeout(() => resolve({ timeout: true }), 8000);
  });
  const r = {};
  r.echo_ok = await window.host("app.echo", { text: "oi" });
  r.echo_invalid = await window.host("app.echo", { text: 5 });
  r.unknown = await window.host("nao.existe");
  r.privileged = await window.host("debug.privileged");
  // Rejeições do hook: nome inválido, plugin não permitido e args grandes demais.
  r.hook_bad_name = await window.host("Bad Name!");
  r.hook_plugin = await window.__TAURI__.core
    .invoke("plugin:fs|read_text_file", { path: "x" })
    .then(() => ({ ok: true }), (error) => ({ ok: false, error }));
  r.hook_big_args = await window.host("app.echo", { text: "x".repeat(70000) });
  r.untrusted = await untrusted;
  r.mfe = await mfeTest();
  r.bytes = await bytesTest();
  r.apps = await appsTest();
  await window.host("smoke.report", r);
}

// E2.4: instalação com consentimento, execução com UI declarativa, suspensão e remoção,
// do pacote assinado preparado pelo xtask (chega como se tivesse sido arrastado).
async function appsTest() {
  const t = {};
  const pend = await window.host("apps.pending");
  t.pending = pend.ok ? pend.value.length : -1;
  if (t.pending !== 1) return t;
  const { token, preview } = pend.value[0];
  t.preview = preview;
  t.bad_token = await window.host("apps.install", { token: "nao-existe", approved: [] });
  t.bad_perm = await window.host("apps.install", { token, approved: ["root"] });
  t.exceed = await window.host("apps.install", { token, approved: ["storage", "clock"] });
  t.install = await window.host("apps.install", { token, approved: ["storage"] });
  const ev = (event) => window.host("apps.dispatch", { id: "clicker", event });
  // Janela nativa do mini-app (rótulo app-clicker, criada pelo host): ela mesma pede a UI pelo
  // `view.dispatch` e avisa por BroadcastChannel que desenhou. A janela principal não tem essa
  // capability, e ids inválidos ou de apps não instalados são recusados.
  const bcv = new BroadcastChannel("appview");
  const drawn = new Promise((res) => {
    bcv.onmessage = (e) => res(e.data);
    setTimeout(() => res(null), 15000);
  });
  t.open_bad = await window.host("apps.open", { id: "../x" });
  t.open_missing = await window.host("apps.open", { id: "nao-existe" });
  t.view_from_main = await window.host("view.dispatch", { event: { type: "init" } });
  t.open = await window.host("apps.open", { id: "clicker" });
  t.view = await drawn;
  t.init = await ev({ type: "init" });
  await ev({ type: "click", id: "inc" });
  t.click = await ev({ type: "click", id: "inc" });
  t.bad_event = await ev({ type: "click", id: "../x" });
  // Valores de campos vão junto do clique; chave inválida é recusada antes de chegar ao app.
  t.click_values = await ev({ type: "click", id: "nada", values: { campo: "texto\ncom linha" } });
  t.bad_values = await ev({ type: "click", id: "inc", values: { "A b": "x" } });
  t.bad_value_text = await ev({ type: "click", id: "inc", values: { campo: "a\u0000b" } });
  t.unknown_event_field = await window.host("apps.dispatch", {
    id: "clicker",
    event: { type: "click", id: "inc", values: {}, extra: 1 },
  });
  // Chaves confiáveis: lista funciona; entrada ruim ou inexistente é recusada.
  t.trust_list = await window.host("trust.list");
  t.trust_bad = await window.host("trust.add", { key: "zz" });
  t.trust_missing = await window.host("trust.remove", { key: "ab".repeat(32) });
  // Backup de app inexistente falha antes de abrir qualquer diálogo.
  t.export_missing = await window.host("data.export", { id: "nao-existe" });
  t.suspend = await window.host("apps.set_enabled", { id: "clicker", enabled: false });
  t.suspended_run = await ev({ type: "init" });
  await window.host("apps.set_enabled", { id: "clicker", enabled: true });
  t.resumed = await ev({ type: "init" });
  t.list = await window.host("apps.list");
  t.uninstall = await window.host("apps.uninstall", { id: "clicker", keep_data: false });
  t.window_after_uninstall = await new Promise((res) => setTimeout(() => res(true), 300));
  t.after = await window.host("apps.list");
  t.gone = await ev({ type: "init" });
  t.pending_after = (await window.host("apps.pending")).value.length;
  return t;
}

(async () => {
  const info = await window.host("app.info");
  out.textContent = info.ok ? `host v${info.value.version}` : `erro: ${info.error.message}`;
  if (info.ok && !info.value.smoke && !info.value.bench) await window.initAppsUi();
  if (info.ok && info.value.smoke) await smoke();
  if (info.ok && info.value.bench) await window.host("bench.ready");
})();
