import Fastify, { type FastifyInstance } from "fastify";
import { type Provider } from "./providers.js";

export const MAX_PROMPT_CHARS = 20_000;

/** Monta o app com o provedor injetado (facilita testar sem rede). */
export function buildApp(provider: Provider): FastifyInstance {
  const app = Fastify({
    logger: true,
    bodyLimit: 64 * 1024,
    // recusa campos desconhecidos em vez de descartá-los em silêncio
    ajv: { customOptions: { removeAdditional: false } },
  });

  app.get("/health", async () => ({ status: "ok" }));

  app.post(
    "/v1/complete",
    {
      schema: {
        body: {
          type: "object",
          required: ["prompt"],
          additionalProperties: false,
          properties: {
            prompt: { type: "string", minLength: 1, maxLength: MAX_PROMPT_CHARS },
            max_tokens: { type: "integer", minimum: 1, maximum: 4096, default: 512 },
          },
        },
      },
    },
    async (req, reply) => {
      const { prompt, max_tokens } = req.body as { prompt: string; max_tokens: number };
      try {
        const text = await provider.complete(prompt, max_tokens);
        return { text, provider: provider.name };
      } catch (err) {
        // não vaza detalhes do provedor (nem chave) ao cliente
        req.log.error({ err: String(err) }, "falha no provedor");
        return reply.code(502).send({ error: "falha no provedor de LLM" });
      }
    },
  );

  return app;
}
