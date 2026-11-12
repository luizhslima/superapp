// Telas do shell: abas, lista de mini-apps, consentimento de permissões, loja, chaves confiáveis
// e renderização da UI declarativa (ADR-009). Nada vindo de mini-app vira HTML: tudo entra por
// textContent/value, e não há estilo em linha (a CSP não permite).
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

  // ---- Abas -------------------------------------------------------------------------------
  // Abas fixas ("apps", "store", "settings") e uma aba por mini-app aberto ("app:<id>"), cada
  // uma com a sua própria tela e os seus próprios campos.
  const MAX_APP_TABS = 8;
  const appTabs = new Map(); // id do app -> { name, wrap, panel, view, fields, busy }
  let currentTab = "apps";

  function showTab(name) {
    currentTab = name;
    for (const t of document.querySelectorAll("#tabs [data-tab]")) {
      t.setAttribute("aria-selected", String(t.dataset.tab === name));
    }
    for (const p of document.querySelectorAll("[data-panel]")) {
      p.hidden = p.dataset.panel !== name;
    }
    // Loja e Configurações (abertas pelo menu) só aparecem como aba enquanto estão ativas.
    for (const w of document.querySelectorAll("#tabs [data-section]")) {
      w.hidden = w.dataset.section !== name;
    }
  }

  function selectTab(name) {
    showTab(name);
    if (name === "settings") refreshTrust();
  }

  function cycleTab(step) {
    const order = [...document.querySelectorAll("#tabs [data-tab]")]
      .filter((t) => !t.closest("[hidden]"))
      .map((t) => t.dataset.tab);
    const i = order.indexOf(currentTab);
    selectTab(order[(i + step + order.length) % order.length]);
  }

  function openAppTab(a) {
    const key = `app:${a.id}`;
    if (appTabs.has(a.id)) return selectTab(key);
    if (appTabs.size >= MAX_APP_TABS) {
      return status(`Limite de ${MAX_APP_TABS} abas de mini-apps: feche alguma.`, true);
    }
    const btn = button(a.name, () => selectTab(key));
    btn.setAttribute("role", "tab");
    btn.setAttribute("aria-selected", "false");
    btn.dataset.tab = key;
    const close = button("×", () => closeAppTab(a.id), "tab-close");
    close.setAttribute("aria-label", `Fechar aba ${a.name}`);
    const wrap = el("span", undefined, "tab-wrap");
    wrap.append(btn, close);
    $("tabs").append(wrap);

    const panel = el("main");
    panel.setAttribute("role", "tabpanel");
    panel.dataset.panel = key;
    panel.hidden = true;
    const view = el("div", undefined, "view");
    panel.append(el("h2", a.name), view);
    $("panel-settings").after(panel);

    appTabs.set(a.id, { name: a.name, wrap, panel, view, fields: new Map(), busy: false });
    selectTab(key);
    dispatch(a.id, { type: "init" });
  }

  function closeAppTab(id) {
    const t = appTabs.get(id);
    if (!t) return;
    appTabs.delete(id);
    t.wrap.remove();
    t.panel.remove();
    if (currentTab === `app:${id}`) selectTab("apps");
  }

  // ---- UI declarativa -------------------------------------------------------------------
  // Os campos de cada aba (id -> função que lê o valor) vão junto de cada clique (formulário).
  function collectValues(tab) {
    const v = {};
    for (const [id, get] of tab.fields) v[id] = get();
    return v;
  }

  function clickEvent(id, tab) {
    const event = { type: "click", id };
    if (tab.fields.size > 0) event.values = collectValues(tab);
    return event;
  }

  function labelled(text, control, cls) {
    const l = el("label", undefined, cls || "field");
    l.append(el("span", text), control);
    return l;
  }

  // Enter num campo aciona o botão da mesma linha ou, na falta, o primeiro botão da tela.
  function submitFrom(input, tab) {
    const row = input.closest(".row");
    const b = (row && row.querySelector("button")) || tab.view.querySelector("button");
    if (b) b.click();
  }

  function render(node, parent, appId, tab) {
    switch (node.type) {
      case "text":
        parent.append(el("p", node.text, "text"));
        break;
      case "heading":
        parent.append(el("h3", node.text));
        break;
      case "divider":
        parent.append(el("hr"));
        break;
      case "button":
        parent.append(button(node.label, () => dispatch(appId, clickEvent(node.id, tab))));
        break;
      case "list": {
        const ul = el("ul");
        for (const item of node.items) ul.append(el("li", item));
        parent.append(ul);
        break;
      }
      case "action_list": {
        const ul = el("ul", undefined, "actions");
        for (const item of node.items) {
          const li = el("li");
          if (item.done !== null && item.done !== undefined) {
            li.append(el("span", item.done ? "☑" : "☐", "mark"));
          }
          li.append(el("span", item.text, item.done ? "item-text done" : "item-text"));
          for (const a of item.actions || []) {
            li.append(button(a.label, () => dispatch(appId, clickEvent(a.id, tab))));
          }
          ul.append(li);
        }
        parent.append(ul);
        break;
      }
      case "stack":
      case "row": {
        const d = el("div", undefined, node.type);
        for (const c of node.children) render(c, d, appId, tab);
        parent.append(d);
        break;
      }
      case "input": {
        const i = document.createElement("input");
        i.type = "text";
        i.maxLength = 1000;
        i.value = node.value || "";
        i.placeholder = node.placeholder || "";
        i.autocomplete = "off";
        i.addEventListener("keydown", (e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            submitFrom(i, tab);
          }
        });
        tab.fields.set(node.id, () => i.value);
        parent.append(labelled(node.label, i));
        break;
      }
      case "checkbox": {
        const c = document.createElement("input");
        c.type = "checkbox";
        c.checked = !!node.checked;
        tab.fields.set(node.id, () => String(c.checked));
        const l = el("label", undefined, "check");
        l.append(c, el("span", node.label));
        parent.append(l);
        break;
      }
      case "select": {
        const s = document.createElement("select");
        for (const o of node.options) {
          const opt = document.createElement("option");
          opt.value = o;
          opt.textContent = o;
          s.append(opt);
        }
        s.value = node.selected || node.options[0];
        tab.fields.set(node.id, () => s.value);
        parent.append(labelled(node.label, s));
        break;
      }
      case "progress": {
        const wrap = el("div", undefined, "progress");
        const p = document.createElement("progress");
        p.max = 100;
        p.value = node.value;
        if (node.label) p.setAttribute("aria-label", node.label);
        wrap.append(p);
        if (node.label) wrap.append(el("span", node.label));
        parent.append(wrap);
        break;
      }
    }
  }

  async function dispatch(id, event) {
    const tab = appTabs.get(id);
    if (!tab || tab.busy) return;
    tab.busy = true;
    tab.view.setAttribute("aria-busy", "true");
    try {
      const r = await window.host("apps.dispatch", { id, event });
      if (!appTabs.has(id)) return; // a aba foi fechada enquanto o app rodava
      if (!r.ok) {
        status(`${tab.name}: ${errText(r)}`, true);
        return;
      }
      status("");
      tab.fields = new Map();
      tab.view.replaceChildren();
      render(r.value, tab.view, id, tab);
    } finally {
      tab.view.removeAttribute("aria-busy");
      tab.busy = false;
    }
  }

  // ---- Lista e ações ----------------------------------------------------------------------
  // Botão de dois cliques: o primeiro só "arma" e mostra o que vai acontecer.
  function armed(label, armedLabel, onConfirm, cls) {
    const b = button(label, async () => {
      if (!b.dataset.armed) {
        b.dataset.armed = "1";
        b.textContent = armedLabel;
        return;
      }
      delete b.dataset.armed;
      b.textContent = label;
      await onConfirm();
    }, cls);
    b.addEventListener("blur", () => {
      delete b.dataset.armed;
      b.textContent = label;
    });
    return b;
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
    closeAppTab(a.id);
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
        const open = button("Abrir", () => openAppTab(a), "primary");
        open.disabled = !a.enabled;
        li.append(
          open,
          button(a.enabled ? "Suspender" : "Retomar", async () => {
            const res = await window.host("apps.set_enabled", { id: a.id, enabled: !a.enabled });
            if (!res.ok) status(errText(res), true);
            if (a.enabled) closeAppTab(a.id);
            refreshApps();
          }),
        );
      }
      const more = el("div", undefined, "app-actions");
      if (!a.problem) {
        more.append(
          button("Exportar dados", () => exportData(a)),
          armed(
            "Restaurar dados",
            "Confirmar: substitui os dados atuais",
            () => importData(a),
          ),
        );
      }
      // Remover: primeiro clique pergunta o que fazer com os dados.
      const rm = button("Remover", () => {
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
      }, "danger");
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
        ul.append(li);
        boxes.push([perm, cb]);
      }
      card.append(ul);
      const actions = el("div", undefined, "actions");
      const ok = button(p.is_update ? "Atualizar" : "Instalar", async () => {
        const approved = boxes.filter(([, cb]) => cb.checked).map(([perm]) => perm);
        const res = await window.host("apps.install", { token, approved });
        status(res.ok ? `${res.value.name} instalado.` : errText(res), !res.ok);
        refreshPending();
        refreshApps();
      }, "primary");
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
        status(`${e.name} baixado e verificado: revise as permissões abaixo.`);
        selectTab("apps");
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

  async function pickFile() {
    const r = await window.host("apps.pick");
    if (!r.ok) return status(errText(r), true);
    if (r.value.picked) {
      status("");
      selectTab("apps");
      refreshPending();
    }
  }

  // Hambúrguer: submenu com as seções e ações. Cada item tem um `data-action` que é um dos ids
  // de handleMenu (os mesmos do menu nativo), então os dois menus fazem exatamente a mesma coisa.
  function initHamburger() {
    const btn = $("hamburger");
    const menu = $("menu");
    const items = () => [...menu.querySelectorAll("[role=menuitem]")];
    const close = (refocus) => {
      menu.hidden = true;
      btn.setAttribute("aria-expanded", "false");
      if (refocus) btn.focus();
    };
    const open = () => {
      menu.hidden = false;
      btn.setAttribute("aria-expanded", "true");
      items()[0].focus();
    };
    btn.addEventListener("click", () => (menu.hidden ? open() : close(true)));
    menu.addEventListener("click", (e) => {
      const it = e.target.closest("[data-action]");
      if (!it) return;
      close(true);
      handleMenu(it.dataset.action);
    });
    menu.addEventListener("keydown", (e) => {
      const list = items();
      const i = list.indexOf(document.activeElement);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        list[(i + 1) % list.length].focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        list[(i - 1 + list.length) % list.length].focus();
      } else if (e.key === "Home") {
        e.preventDefault();
        list[0].focus();
      } else if (e.key === "End") {
        e.preventDefault();
        list[list.length - 1].focus();
      } else if (e.key === "Escape") {
        e.preventDefault();
        close(true);
      } else if (e.key === "Tab") {
        close(false);
      }
    });
    // Clicar fora fecha.
    document.addEventListener("click", (e) => {
      if (!menu.hidden && !e.target.closest(".menu-wrap")) close(false);
    });
  }

  function handleMenu(action) {
    switch (action) {
      case "pick":
        pickFile();
        break;
      case "close-tab":
        if (currentTab.startsWith("app:")) closeAppTab(currentTab.slice(4));
        else if (currentTab !== "apps") selectTab("apps");
        break;
      case "tab-apps":
        selectTab("apps");
        break;
      case "tab-store":
        selectTab("store");
        break;
      case "tab-settings":
        selectTab("settings");
        break;
      case "next-tab":
        cycleTab(1);
        break;
      case "prev-tab":
        cycleTab(-1);
        break;
      case "update-check":
        selectTab("settings");
        $("update-check").click();
        break;
      case "diag":
        selectTab("settings");
        $("diag-run").click();
        break;
    }
  }

  // ---- Inicialização ----------------------------------------------------------------------
  window.initAppsUi = async function () {
    for (const t of document.querySelectorAll("#tabs [data-tab]")) {
      t.addEventListener("click", () => selectTab(t.dataset.tab));
    }
    for (const c of document.querySelectorAll("#tabs [data-close]")) {
      c.addEventListener("click", () => selectTab("apps"));
    }
    initHamburger();
    $("apps-pick").addEventListener("click", pickFile);
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
    // O Rust avisa quando um arquivo foi arrastado (null = ok, texto = motivo da recusa).
    try {
      await window.__TAURI__.event.listen("apps://staged", (e) => {
        status(e.payload ? `Pacote recusado: ${e.payload}` : "", !!e.payload);
        if (!e.payload) selectTab("apps");
        refreshPending();
      });
    } catch (_) {
      /* sem eventos: a tela ainda funciona ao recarregar */
    }
    // Menu nativo: o Rust só avisa qual item foi escolhido (ids fixos); a ação roda aqui,
    // pelos mesmos comandos de sempre.
    try {
      await window.__TAURI__.event.listen("menu://action", (e) => handleMenu(e.payload));
    } catch (_) {
      /* sem eventos: o menu não faz nada, a tela segue funcionando */
    }
    await refreshApps();
    await refreshPending();
  };
})();
