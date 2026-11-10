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
  r.untrusted = await untrusted;
  await window.host("smoke.report", r);
}

(async () => {
  const info = await window.host("app.info");
  out.textContent = info.ok ? `host v${info.value.version}` : `erro: ${info.error.message}`;
  if (info.ok && info.value.smoke) await smoke();
})();
