// Compila app.js para um componente Wasm (clicker-js.wasm) usando jco (versão fixa).
// Uso: node build.mjs   (precisa de Node 20+; baixa o jco via npx na primeira vez)
//
// O jco NÃO remove importações não usadas: com o mundo completo do host (`miniapp`), o
// componente exigiria as três permissões (log, storage, clock) mesmo usando só storage. Por
// isso declaramos um mundo enxuto (`app`) só com o que o app usa. O host aceita: a
// exportação `run` é a mesma e o linker do host tem as interfaces concedidas.
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const hostWit = join(here, "../../crates/wasm-host/wit/host.wit");
const work = join(here, ".build/wit");

rmSync(join(here, ".build"), { recursive: true, force: true });
mkdirSync(join(work, "deps/superapp-host"), { recursive: true });
cpSync(hostWit, join(work, "deps/superapp-host/host.wit"));
writeFileSync(
  join(work, "app.wit"),
  `package superapp:clicker-js@0.1.0;

world app {
  // Só as permissões que o app usa. Para outro app, importe só o necessário.
  import superapp:host/storage@0.1.0;
  export run: func(input: string) -> result<string, string>;
}
`,
);

const JCO = "@bytecodealliance/jco@1.41.0";
const npx = process.platform === "win32" ? "npx.cmd" : "npx";
const r = spawnSync(
  npx,
  [
    "--yes",
    JCO,
    "componentize",
    join(here, "app.js"),
    "--wit",
    work,
    "--world-name",
    "app",
    // O interpretador embute `fetch` (wasi:http). O host nunca liga rede, então o app não pode
    // importá-la: desabilita o recurso para o componente carregar.
    "--disable",
    "http",
    "fetch-event",
    "--out",
    join(here, "clicker-js.wasm"),
  ],
  { stdio: "inherit", shell: process.platform === "win32" },
);
process.exit(r.status ?? 1);
