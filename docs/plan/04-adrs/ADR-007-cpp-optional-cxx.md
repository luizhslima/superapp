# ADR-007 · C++ opcional via `cxx` / `cxx-async`

- **Status**: Proposed — adiar até haver caso de uso concreto; validar em S4
- **Contexto**: C++ é necessário para llama.cpp, gráficos e aceleração de hardware. `cxx` oferece FFI segura e tradução de exceções→`Result`; `cxx-async` integra corrotinas C++20 com Tokio.
- **Decisão**: C++ **não** entra no MVP. Fase 4 só ocorre com um módulo real. Toda fronteira C++ passa por `cxx`; sem `unsafe` manual salvo justificativa.
- **Consequências**: (+) mantém simplicidade e CI barata no início. (−) integração tardia exige toolchains nos 3 SOs.
- **Alternativas**: rodar módulos C++ compilados para Wasm; processo separado.
- **Riscos**: R4.
