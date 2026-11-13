import assert from "node:assert/strict";
import { test } from "node:test";
import { buildApp } from "../src/app.js";
import { EchoProvider, type Provider } from "../src/providers.js";

test("health", async () => {
  const app = buildApp(new EchoProvider());
  const r = await app.inject({ method: "GET", url: "/health" });
  assert.deepEqual(r.json(), { status: "ok" });
});

test("complete com echo", async () => {
  const app = buildApp(new EchoProvider());
  const r = await app.inject({ method: "POST", url: "/v1/complete", payload: { prompt: "oi" } });
  assert.equal(r.statusCode, 200);
  assert.deepEqual(r.json(), { text: "[echo] oi", provider: "echo" });
});

test("rejeita vazio, enorme e campo extra", async () => {
  const app = buildApp(new EchoProvider());
  for (const payload of [{ prompt: "" }, { prompt: "x".repeat(20_001) }, { prompt: "a", x: 1 }]) {
    const r = await app.inject({ method: "POST", url: "/v1/complete", payload });
    assert.equal(r.statusCode, 400);
  }
});

test("erro do provedor vira 502 sem vazar detalhe", async () => {
  const quebrado: Provider = {
    name: "quebrado",
    complete: async () => {
      throw new Error("chave sk-secreta inválida");
    },
  };
  const app = buildApp(quebrado);
  const r = await app.inject({ method: "POST", url: "/v1/complete", payload: { prompt: "oi" } });
  assert.equal(r.statusCode, 502);
  assert.ok(!r.body.includes("sk-secreta"));
});
