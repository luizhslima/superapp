// Tela de mini-apps: lista, consentimento de permissões e renderização da UI declarativa
// (ADR-009). Nada vindo de mini-app vira HTML: tudo entra por textContent.
(() => {
  const PERMS = {
    log: "Registrar mensagens de diagnóstico",
    storage: "Guardar dados próprios neste computador (até 4 MiB)",
    clock: "Ler a data e a hora atuais",
  };

  const $ = (id) => document.getElementById(id);

  function el(tag, text, cls) {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  }

  function status(message, isError) {
    const s = $("status");
    s.textContent = message || "";
    s.className = isError ? "error" : "";
  }

  const errText = (r) => (r.error && r.error.message) || "erro desconhecido";

  // ---- UI declarativa -------------------------------------------------------------------
  function render(node, parent, appId) {
    switch (node.type) {
      case "text":
        parent.append(el("p", node.text));
        break;
      case "button": {
        const b = el("button", node.label);
        b.type = "button";
        b.addEventListener("click", () => dispatch(appId, { type: "click", id: node.id }));
        parent.append(b);
        break;
      }
      case "list": {
        const ul = el("ul");
        for (const item of node.items) ul.append(el("li", item));
        parent.append(ul);
        break;
      }
      case "stack": {
        const d = el("div", undefined, "stack");
        for (const c of node.children) render(c, d, appId);
        parent.append(d);
        break;
      }
    }
  }

  let openApp = null;

  async function dispatch(id, event) {
    const r = await window.host("apps.dispatch", { id, event });
    if (!r.ok) {
      status(`${id}: ${errText(r)}`, true);
      return;
    }
    status("");
    openApp = id;
    const view = $("view");
    view.replaceChildren();
    render(r.value, view, id);
    $("view-section").hidden = false;
  }

  // ---- Lista e ações ----------------------------------------------------------------------
  async function refreshApps() {
    const r = await window.host("apps.list");
    const ul = $("apps");
    ul.replaceChildren();
    if (!r.ok) return status(errText(r), true);
    $("apps-empty").hidden = r.value.length > 0;
    for (const a of r.value) {
      const li = el("li");
      li.append(el("span", `${a.name} ${a.version}`, "app-name"));
      if (a.problem) {
        li.append(el("span", `com problema: ${a.problem}`, "badge problem"));
      } else {
        li.append(el("span", a.enabled ? "ativo" : "suspenso", "badge"));
        const open = el("button", "Abrir");
        open.type = "button";
        open.disabled = !a.enabled;
        open.addEventListener("click", () => {
          $("view-name").textContent = a.name;
          dispatch(a.id, { type: "init" });
        });
        const toggle = el("button", a.enabled ? "Suspender" : "Retomar");
        toggle.type = "button";
        toggle.addEventListener("click", async () => {
          const res = await window.host("apps.set_enabled", { id: a.id, enabled: !a.enabled });
          if (!res.ok) status(errText(res), true);
          if (a.enabled && openApp === a.id) closeView();
          refreshApps();
        });
        li.append(open, toggle);
      }
      const rm = el("button", "Remover", "danger");
      rm.type = "button";
      rm.addEventListener("click", async () => {
        // Confirmação em dois cliques (sem depender de diálogo nativo do WebView).
        if (!rm.dataset.armed) {
          rm.dataset.armed = "1";
          rm.textContent = "Confirmar remoção (apaga os dados)";
          return;
        }
        const res = await window.host("apps.uninstall", { id: a.id, keep_data: false });
        if (!res.ok) status(errText(res), true);
        if (openApp === a.id) closeView();
        refreshApps();
      });
      li.append(rm);
      ul.append(li);
    }
  }

  function closeView() {
    openApp = null;
    $("view-section").hidden = true;
    $("view").replaceChildren();
  }

  // ---- Consentimento ----------------------------------------------------------------------
  async function refreshPending() {
    const r = await window.host("apps.pending");
    const box = $("pending");
    box.replaceChildren();
    const list = r.ok ? r.value : [];
    $("pending-section").hidden = list.length === 0;
    for (const { token, preview: p } of list) {
      const card = el("div", undefined, "consent");
      card.append(
        el("h3", p.is_update ? `Atualizar ${p.name} para ${p.version}?` : `Instalar ${p.name} ${p.version}?`),
      );
      card.append(el("p", `Autor (chave pública): ${p.signer}`, "meta"));
      if (p.is_update && p.added.length) {
        card.append(el("p", "Esta versão pede permissões novas, marcadas abaixo.", "new-perms"));
      }
      card.append(el("p", p.requested.length ? "Este mini-app pede permissão para:" : "Este mini-app não pede nenhuma permissão."));
      const ul = el("ul");
      const boxes = [];
      for (const perm of p.requested) {
        const li = el("li");
        const cb = document.createElement("input");
        cb.type = "checkbox";
        cb.checked = true;
        cb.id = `perm-${token}-${perm}`;
        const lab = el("label", ` ${PERMS[perm] || perm}${p.added.includes(perm) && p.is_update ? " (nova)" : ""}`);
        lab.htmlFor = cb.id;
        li.append(cb, lab);
        ul.append(li);
        boxes.push([perm, cb]);
      }
      card.append(ul);
      const actions = el("div", undefined, "actions");
      const ok = el("button", p.is_update ? "Atualizar" : "Instalar", "primary");
      ok.type = "button";
      ok.addEventListener("click", async () => {
        const approved = boxes.filter(([, cb]) => cb.checked).map(([perm]) => perm);
        const res = await window.host("apps.install", { token, approved });
        status(res.ok ? `${res.value.name} instalado.` : errText(res), !res.ok);
        refreshPending();
        refreshApps();
      });
      const no = el("button", "Cancelar");
      no.type = "button";
      no.addEventListener("click", async () => {
        await window.host("apps.discard", { token });
        refreshPending();
      });
      actions.append(ok, no);
      card.append(actions);
      box.append(card);
    }
  }

  window.initAppsUi = async function () {
    $("view-close").addEventListener("click", closeView);
    // Atualização do host: a UI só pede "verificar" e "instalar"; endereço e chave são do Rust.
    $("update-check").addEventListener("click", async () => {
      status("Verificando atualizações...");
      const r = await window.host("app.update.check");
      if (!r.ok) return status(errText(r), true);
      const v = r.value;
      $("update-install").hidden = !v.available;
      status(
        v.available
          ? `Nova versão ${v.available} disponível (atual ${v.current}).`
          : `Você já está na versão mais recente (${v.current}).`
      );
    });
    $("update-install").addEventListener("click", async () => {
      $("update-install").disabled = true;
      status("Baixando e verificando a assinatura...");
      const r = await window.host("app.update.install");
      // Em caso de sucesso o app reinicia e esta linha nem chega a rodar.
      if (!r.ok) status(errText(r), true);
      $("update-install").disabled = false;
    });
    $("store-load").addEventListener("click", async () => {
      status("Carregando catálogo...");
      const r = await window.host("store.catalog");
      const ul = $("store-list");
      ul.replaceChildren();
      if (!r.ok) return status(errText(r), true);
      status("");
      if (r.value.entries.length === 0) ul.append(el("li", "Catálogo vazio."));
      for (const e of r.value.entries) {
        const li = el("li");
        li.append(el("span", `${e.name} ${e.version}`, "app-name"));
        const b = el("button", "Baixar");
        b.type = "button";
        b.addEventListener("click", async () => {
          b.disabled = true;
          status(`Baixando ${e.name}...`);
          const d = await window.host("store.download", { id: e.id });
          b.disabled = false;
          if (!d.ok) return status(errText(d), true);
          status(`${e.name} baixado e verificado: revise as permissões abaixo.`);
          refreshPending();
        });
        li.append(b);
        ul.append(li);
      }
    });
    $("diag-run").addEventListener("click", async () => {
      const r = await window.host("diag.report");
      if (!r.ok) return status(errText(r), true);
      const out = $("diag-out");
      out.value = JSON.stringify(r.value, null, 2);
      out.hidden = false;
    });
    // O Rust avisa quando um arquivo foi arrastado (null = ok, texto = motivo da recusa).
    try {
      await window.__TAURI__.event.listen("apps://staged", (e) => {
        status(e.payload ? `Pacote recusado: ${e.payload}` : "", !!e.payload);
        refreshPending();
      });
    } catch (_) {
      /* sem eventos: a tela ainda funciona ao recarregar */
    }
    await refreshApps();
    await refreshPending();
  };
})();
