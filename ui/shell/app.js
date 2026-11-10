const out = document.getElementById("out");

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
  await window.host("smoke.report", r);
}

(async () => {
  const info = await window.host("app.info");
  out.textContent = info.ok ? `host v${info.value.version}` : `erro: ${info.error.message}`;
  if (info.ok && info.value.smoke) await smoke();
})();
