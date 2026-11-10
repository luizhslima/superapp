// Cliente do IPC: único ponto que fala com o host. Sempre resolve com {ok, value|error}.
window.host = async (command, args) => {
  try {
    return { ok: true, value: await window.__TAURI__.core.invoke("ipc", { command, args }) };
  } catch (error) {
    return { ok: false, error };
  }
};

// Canal binário: corpo cru (Uint8Array) e nome do comando no cabeçalho. Resolve {ok, value: Uint8Array}.
window.hostBytes = async (command, body) => {
  try {
    const buf = await window.__TAURI__.core.invoke("ipc_bytes", body, { headers: { command } });
    return { ok: true, value: new Uint8Array(buf) };
  } catch (error) {
    return { ok: false, error };
  }
};

// Corpos grandes (fora do Isolation, ~50x mais rápido para envio). Resolve {ok, value: Uint8Array}
// ou {ok:false, error:{code,message}} com o mesmo formato de erro dos outros canais.
window.hostBlob = async (command, body) => {
  try {
    const url = window.__TAURI__.core.convertFileSrc(command, "sapp");
    const res = await fetch(url, { method: "POST", body });
    const buf = new Uint8Array(await res.arrayBuffer());
    if (res.ok) return { ok: true, value: buf };
    return { ok: false, error: JSON.parse(new TextDecoder().decode(buf)) };
  } catch (error) {
    return { ok: false, error: { code: "internal", message: String(error) } };
  }
};
