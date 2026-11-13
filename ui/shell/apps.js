// Janela principal: lista de mini-apps, consentimento de permissões, loja, chaves confiáveis e
// configurações. A navegação entre as seções é do menu nativo (Exibir); cada mini-app abre na
// sua própria janela nativa (ver view.js). Nada vindo de mini-app passa por aqui como HTML, e
// não há estilo em linha (a CSP não permite).
(() => {
  const PERMS = {
    log: "Registrar mensagens de diagnóstico",
    storage: "Guardar dados próprios neste computador (até 4 MiB)",
    clock: "Ler a data e a hora atuais",
    http: "Acessar a internet, somente os domínios listados abaixo",
  };

  const $ = (id) => document.getElementById(id);

  function el(tag, text, cls) {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  }

  function button(label, onClick, cls) {
    const b = el("button", label, cls);
    b.type = "button";
    b.addEventListener("click", onClick);
    return b;
  }

  function status(message, isError) {
    const s = $("status");
    s.textContent = message || "";
    s.className = isError ? "error" : "";
  }

  const errText = (r) => (r.error && r.error.message) || "erro desconhecido";

  // ---- Seções -----------------------------------------------------------------------------
  // "apps", "store" ou "settings": uma por vez, escolhida pelo menu nativo (Exibir).
  function showSection(name) {
    for (const p of document.querySelectorAll("[data-panel]")) {
      p.hidden = p.dataset.panel !== name;
    }
    if (name === "settings") refreshTrust();
  }

  // ---- Lista e ações ----------------------------------------------------------------------
  // Botão de dois cliques: o primeiro só "arma" e mostra o que vai acontecer.
  function armed(label, armedLabel, onConfirm, cls) {
    const b = button(
      label,
      async () => {
        if (!b.dataset.armed) {
          b.dataset.armed = "1";
          b.textContent = armedLabel;
          return;
        }
        delete b.dataset.armed;
        b.textContent = label;
        await onConfirm();
      },
      cls,
    );
    b.addEventListener("blur", () => {
      delete b.dataset.armed;
      b.textContent = label;
    });
    return b;
  }

  async function openApp(a) {
    const r = await window.host("apps.open", { id: a.id });
    if (!r.ok) status(errText(r), true);
  }

  async function exportData(a) {
    status("Escolha onde salvar o backup...");
    const r = await window.host("data.export", { id: a.id });
    if (!r.ok) return status(errText(r), true);
    status(r.value.saved ? `Backup de ${a.name} salvo.` : "Exportação cancelada.");
  }

  async function importData(a) {
    status("Escolha o arquivo de backup...");
    const r = await window.host("data.import", { id: a.id });
    if (!r.ok) return status(errText(r), true);
    status(
      r.value.restored === null
        ? "Restauração cancelada."
        : `Dados de ${a.name} restaurados (${r.value.restored} itens).`,
    );
  }

  async function uninstall(a, keepData) {
    const res = await window.host("apps.uninstall", { id: a.id, keep_data: keepData });
    if (!res.ok) status(errText(res), true);
    else status(keepData ? `${a.name} removido; os dados foram mantidos.` : `${a.name} removido.`);
    refreshApps();
  }

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
        const open = button("Abrir", () => openApp(a), "primary");
        open.disabled = !a.enabled;
        li.append(
          open,
          button(a.enabled ? "Suspender" : "Retomar", async () => {
            const res = await window.host("apps.set_enabled", { id: a.id, enabled: !a.enabled });
            if (!res.ok) status(errText(res), true);
            refreshApps();
          }),
        );
      }
      const more = el("div", undefined, "app-actions");
      if (!a.problem) {
        more.append(
          button("Exportar dados", () => exportData(a)),
          armed("Restaurar dados", "Confirmar: substitui os dados atuais", () => importData(a)),
        );
      }
      // Remover: primeiro clique pergunta o que fazer com os dados.
      const rm = button(
        "Remover",
        () => {
          rm.hidden = true;
          const box = el("span", undefined, "confirm");
          box.append(
            button("Remover e apagar os dados", () => uninstall(a, false), "danger"),
            button("Remover e manter os dados", () => uninstall(a, true)),
            button("Cancelar", () => {
              box.remove();
              rm.hidden = false;
            }),
          );
          more.append(box);
        },
        "danger",
      );
      more.append(rm);
      li.append(more);
      ul.append(li);
    }
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
        if (perm === "http") {
          // Os domínios aprovados e o que isso implica: o app pode enviar a eles o que lê.
          li.append(
            el("p", `Domínios: ${(p.hosts || []).join(", ") || "(nenhum)"}`, "meta"),
            el(
              "p",
              "O app pode enviar a esses domínios qualquer dado que ele lê, inclusive os que guarda aqui. Só marque se confia no autor.",
              "new-perms",
            ),
          );
        }
        ul.append(li);
        boxes.push([perm, cb]);
      }
      card.append(ul);
      const actions = el("div", undefined, "actions");
      const ok = button(
        p.is_update ? "Atualizar" : "Instalar",
        async () => {
          const approved = boxes.filter(([, cb]) => cb.checked).map(([perm]) => perm);
          const res = await window.host("apps.install", { token, approved });
          status(res.ok ? `${res.value.name} instalado.` : errText(res), !res.ok);
          refreshPending();
          refreshApps();
        },
        "primary",
      );
      const no = button("Cancelar", async () => {
        await window.host("apps.discard", { token });
        refreshPending();
      });
      actions.append(ok, no);
      card.append(actions);
      box.append(card);
    }
  }

  // ---- Loja -------------------------------------------------------------------------------
  const DAY = 86400;

  function showCatalogValidity(expiresAt) {
    const p = $("store-validity");
    const days = Math.floor((expiresAt - Date.now() / 1000) / DAY);
    const date = new Date(expiresAt * 1000).toLocaleDateString("pt-BR");
    const soon = days <= 14;
    p.textContent = soon
      ? `Atenção: o catálogo vence em ${date} (faltam ${Math.max(days, 0)} dias). Gere e publique um novo.`
      : `Catálogo válido até ${date} (faltam ${days} dias).`;
    p.className = soon ? "notice" : "hint";
    p.hidden = false;
  }

  async function loadCatalog() {
    status("Carregando catálogo...");
    const r = await window.host("store.catalog");
    const ul = $("store-list");
    ul.replaceChildren();
    $("store-validity").hidden = true;
    if (!r.ok) return status(errText(r), true);
    status("");
    showCatalogValidity(r.value.expires_at);
    if (r.value.entries.length === 0) ul.append(el("li", "Catálogo vazio."));
    for (const e of r.value.entries) {
      const li = el("li");
      li.append(el("span", `${e.name} ${e.version}`, "app-name"));
      const b = button("Baixar", async () => {
        b.disabled = true;
        status(`Baixando ${e.name}...`);
        const d = await window.host("store.download", { id: e.id });
        b.disabled = false;
        if (!d.ok) return status(errText(d), true);
        status(`${e.name} baixado e verificado: revise as permissões.`);
        showSection("apps");
        refreshPending();
      });
      li.append(b);
      ul.append(li);
    }
  }

  // ---- Autores confiáveis -----------------------------------------------------------------
  const short = (k) => `${k.slice(0, 8)}…${k.slice(-8)}`;

  async function refreshTrust() {
    const r = await window.host("trust.list");
    const ul = $("trust-list");
    ul.replaceChildren();
    if (!r.ok) return status(errText(r), true);
    $("trust-pending").hidden = !r.value.pending_restart;
    if (r.value.keys.length === 0) {
      ul.append(el("li", "Nenhuma chave: nada pode ser instalado.", "hint"));
    }
    for (const k of r.value.keys) {
      const li = el("li");
      const name = el("span", short(k.key), "app-name key");
      name.title = k.key;
      li.append(name);
      let state = "ativa";
      if (k.in_file && !k.active) state = "passa a valer ao reiniciar";
      if (!k.in_file && k.active) state = "removida: deixa de valer ao reiniciar";
      li.append(el("span", state, "badge"));
      if (k.in_file) {
        li.append(
          armed(
            "Remover",
            "Confirmar: os apps desta chave deixam de abrir",
            async () => {
              const res = await window.host("trust.remove", { key: k.key });
              if (!res.ok) status(errText(res), true);
              refreshTrust();
            },
            "danger",
          ),
        );
      }
      ul.append(li);
    }
  }

  const HEX64 = /^[0-9a-fA-F]{64}$/;

  function initTrustUi() {
    const input = $("trust-new");
    const add = $("trust-add");
    const disarm = () => {
      delete add.dataset.armed;
      add.textContent = "Adicionar";
    };
    input.addEventListener("input", disarm);
    add.addEventListener("click", async () => {
      const key = input.value.trim();
      if (!HEX64.test(key)) return status("Chave inválida: são 64 caracteres hexadecimais.", true);
      if (!add.dataset.armed) {
        add.dataset.armed = "1";
        add.textContent = "Confirmar: confiar nesta chave";
        return;
      }
      disarm();
      const r = await window.host("trust.add", { key });
      if (!r.ok) return status(errText(r), true);
      input.value = "";
      status("Chave adicionada. Reinicie o Superapp para ela valer.");
      refreshTrust();
    });
    $("trust-restart").addEventListener("click", async () => {
      await window.host("app.restart");
    });
  }

  // ---- Menu nativo ------------------------------------------------------------------------
  async function pickFile() {
    const r = await window.host("apps.pick");
    if (!r.ok) return status(errText(r), true);
    if (r.value.picked) {
      status("");
      showSection("apps");
      refreshPending();
    }
  }

  // O Rust só avisa qual item do menu foi escolhido (ids fixos, ver menu.rs); a ação roda aqui,
  // pelos mesmos comandos de sempre, que passam pela política e pela auditoria do Router.
  function handleMenu(action) {
    switch (action) {
      case "pick":
        pickFile();
        break;
      case "sec-apps":
        showSection("apps");
        break;
      case "sec-store":
        showSection("store");
        break;
      case "sec-settings":
        showSection("settings");
        break;
      case "update-check":
        showSection("settings");
        $("update-check").click();
        break;
      case "diag":
        showSection("settings");
        $("diag-run").click();
        break;
    }
  }

  // ---- Inicialização ----------------------------------------------------------------------
  window.initAppsUi = async function () {
    $("apps-pick").addEventListener("click", pickFile);
    // ☰: o conteúdo do menu é nativo; o botão só pede ao host que o abra no cursor.
    $("burger").addEventListener("click", async () => {
      const r = await window.host("menu.popup");
      if (!r.ok) status(errText(r), true);
    });
    initTrustUi();
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
    $("store-load").addEventListener("click", loadCatalog);
    $("diag-run").addEventListener("click", async () => {
      const r = await window.host("diag.report");
      if (!r.ok) return status(errText(r), true);
      const out = $("diag-out");
      out.value = JSON.stringify(r.value, null, 2);
      out.hidden = false;
    });
    try {
      // Arquivo arrastado: o Rust avisa (null = ok, texto = motivo da recusa).
      await window.__TAURI__.event.listen("apps://staged", (e) => {
        status(e.payload ? `Pacote recusado: ${e.payload}` : "", !!e.payload);
        if (!e.payload) showSection("apps");
        refreshPending();
      });
      await window.__TAURI__.event.listen("menu://action", (e) => handleMenu(e.payload));
    } catch (_) {
      /* sem eventos: a tela ainda funciona, só sem menu/arrastar */
    }
    await refreshApps();
    await refreshPending();
  };
})();
