// Janela de um mini-app: desenha a UI declarativa (ADR-009) e leva os eventos ao host.
// Nada vindo do mini-app vira HTML: tudo entra por textContent/value, e não há estilo em
// linha (a CSP não permite). O app é o da janela (decidido no Rust pelo rótulo).
(() => {
  const $ = (id) => document.getElementById(id);
  const view = $("view");

  function el(tag, text, cls) {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  }

  function button(label, onClick) {
    const b = el("button", label);
    b.type = "button";
    b.addEventListener("click", onClick);
    return b;
  }

  function status(message, isError) {
    const s = $("status");
    s.textContent = message || "";
    s.className = isError ? "error" : "";
  }

  // Campos da tela (id -> função que lê o valor). Vão junto de cada clique (formulário).
  let fields = new Map();

  function clickEvent(id) {
    const event = { type: "click", id };
    if (fields.size > 0) {
      event.values = {};
      for (const [k, get] of fields) event.values[k] = get();
    }
    return event;
  }

  function labelled(text, control) {
    const l = el("label", undefined, "field");
    l.append(el("span", text), control);
    return l;
  }

  // Enter num campo aciona o botão da mesma linha ou, na falta, o primeiro botão da tela.
  function submitFrom(input) {
    const row = input.closest(".row");
    const b = (row && row.querySelector("button")) || view.querySelector("button");
    if (b) b.click();
  }

  function render(node, parent) {
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
        parent.append(button(node.label, () => dispatch(clickEvent(node.id))));
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
            li.append(button(a.label, () => dispatch(clickEvent(a.id))));
          }
          ul.append(li);
        }
        parent.append(ul);
        break;
      }
      case "stack":
      case "row": {
        const d = el("div", undefined, node.type);
        for (const c of node.children) render(c, d);
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
            submitFrom(i);
          }
        });
        fields.set(node.id, () => i.value);
        parent.append(labelled(node.label, i));
        break;
      }
      case "checkbox": {
        const c = document.createElement("input");
        c.type = "checkbox";
        c.checked = !!node.checked;
        fields.set(node.id, () => String(c.checked));
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
        fields.set(node.id, () => s.value);
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

  // Avisa outras páginas do mesmo app (só o smoke escuta) que a tela foi desenhada.
  let bc = null;
  try {
    bc = new BroadcastChannel("appview");
  } catch (_) {
    /* sem BroadcastChannel: só deixa de avisar */
  }

  let busy = false;
  async function dispatch(event) {
    if (busy) return;
    busy = true;
    view.setAttribute("aria-busy", "true");
    try {
      const r = await window.host("view.dispatch", { event });
      if (!r.ok) {
        status((r.error && r.error.message) || "erro desconhecido", true);
        if (bc) bc.postMessage({ ok: false, error: r.error });
        return;
      }
      status("");
      fields = new Map();
      view.replaceChildren();
      render(r.value, view);
      if (bc) bc.postMessage({ ok: true, text: view.textContent });
    } finally {
      view.removeAttribute("aria-busy");
      busy = false;
    }
  }

  dispatch({ type: "init" });
})();
