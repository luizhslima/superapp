// Cliente do IPC: único ponto que fala com o host. Sempre resolve com {ok, value|error}.
window.host = async (command, args) => {
  try {
    return { ok: true, value: await window.__TAURI__.core.invoke("ipc", { command, args }) };
  } catch (error) {
    return { ok: false, error };
  }
};
