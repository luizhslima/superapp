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
  t.over_in = await window.hostBytes("blob.echo", new Uint8Array(17 * MiB));
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
  for (const mib of [0.0625, 0.25, 1, 2]) {
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
  await window.host("smoke.report", r);
}

(async () => {
  const info = await window.host("app.info");
  out.textContent = info.ok ? `host v${info.value.version}` : `erro: ${info.error.message}`;
  if (info.ok && info.value.smoke) await smoke();
})();
