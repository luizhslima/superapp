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
  await window.host("smoke.report", r);
}

(async () => {
  const info = await window.host("app.info");
  out.textContent = info.ok ? `host v${info.value.version}` : `erro: ${info.error.message}`;
  if (info.ok && info.value.smoke) await smoke();
})();
