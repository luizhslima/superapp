// Micro-frontend: id vem do nome do arquivo (mfe-a.html -> "a"). Fala com o shell só por
// BroadcastChannel e com o host só por window.host (política por janela no Router).
const id = location.pathname.match(/mfe-([a-z])\.html$/)[1];
const bc = new BroadcastChannel("mfe");
document.getElementById("out").textContent = `micro-frontend ${id}`;
bc.onmessage = (e) => {
  const m = e.data;
  if (m.type === "ping") bc.postMessage({ type: "pong", from: id, n: m.n });
  // Simula um micro-frontend travado: laço infinito na thread da página.
  if (m.type === "hang" && m.target === id) setTimeout(() => { for (;;) {} }, 50);
};
window.host("app.info").then(() => bc.postMessage({ type: "ready", from: id }));
