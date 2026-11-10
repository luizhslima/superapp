# ADR-006 · Modelo de capabilities e Isolation Pattern

- **Status**: Accepted (compatibilidade com Binary IPC validada em S3)
- **Contexto**: A UI web não é confiável (XSS, supply chain). Lições do WeChat/FinClip mostram que permissões devem ser impostas pelo host.
- **Decisão**: Zero privilégio por padrão; capabilities estáticas do Tauri com escopos semânticos (ex.: `$APPDATA/...`); Isolation Pattern ativo; CSP restritiva; comandos IPC fora de escopo são bloqueados e registrados.
- **Mini-apps**: modelo análogo via manifesto de permissões versionado, validado e assinado, mapeado para imports WIT concedidos.
- **Consequências**: (+) defesa em profundidade. (−) fricção de desenvolvimento; mitigar com tooling para gerar capabilities.
- **Detalhes**: [06-security-model.md](../06-security-model.md).
