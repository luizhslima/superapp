// Mini-app de exemplo em JavaScript (mesma interface do clicker em Rust): prova que o
// contrato é poliglota. Compilado para componente com `jco componentize` (ver build.mjs).
import { get, set } from "superapp:host/storage@0.1.0";

const dec = new TextDecoder();
const enc = new TextEncoder();

export function run(input) {
  const event = JSON.parse(input);
  const raw = get("clicks");
  let n = raw ? parseInt(dec.decode(raw), 10) || 0 : 0;
  if (event.type === "click" && event.id === "inc") {
    n += 1;
    set("clicks", enc.encode(String(n)));
  }
  return JSON.stringify({
    type: "stack",
    children: [
      { type: "text", text: `Cliques (JS): ${n}` },
      { type: "button", id: "inc", label: "Incrementar" },
    ],
  });
}
