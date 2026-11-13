// Provedores de LLM atrás de uma interface única. Só este serviço conhece a chave.

export interface Provider {
  readonly name: string;
  complete(prompt: string, maxTokens: number): Promise<string>;
}

/** Sem custo e sem rede: devolve o prompt. Serve para testar o encanamento. */
export class EchoProvider implements Provider {
  readonly name = "echo";
  async complete(prompt: string, maxTokens: number): Promise<string> {
    return `[echo] ${prompt.slice(0, maxTokens * 4)}`;
  }
}

/** API de mensagens da Anthropic. Exige ANTHROPIC_API_KEY. */
export class AnthropicProvider implements Provider {
  readonly name = "anthropic";
  private readonly key: string;
  private readonly model: string;

  constructor(env: NodeJS.ProcessEnv = process.env) {
    this.key = env.ANTHROPIC_API_KEY ?? "";
    this.model = env.ANTHROPIC_MODEL ?? "claude-haiku-5-5";
    if (!this.key) throw new Error("ANTHROPIC_API_KEY não definida");
  }

  async complete(prompt: string, maxTokens: number): Promise<string> {
    const r = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers: {
        "x-api-key": this.key,
        "anthropic-version": "2023-06-01",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        model: this.model,
        max_tokens: maxTokens,
        messages: [{ role: "user", content: prompt }],
      }),
      signal: AbortSignal.timeout(30_000),
    });
    if (!r.ok) throw new Error(`provedor respondeu ${r.status}`);
    const j = (await r.json()) as { content?: { text?: string }[] };
    return (j.content ?? []).map((p) => p.text ?? "").join("");
  }
}

export function build(env: NodeJS.ProcessEnv = process.env): Provider {
  const name = env.LLM_PROVIDER ?? "echo";
  if (name === "echo") return new EchoProvider();
  if (name === "anthropic") return new AnthropicProvider(env);
  throw new Error(`LLM_PROVIDER desconhecido: ${name}`);
}
