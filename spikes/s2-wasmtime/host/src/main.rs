// Spike S2: Wasmtime + Component Model + fuel + limite de memória + sem permissões WASI.
use anyhow::Result;
use std::time::Instant;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    world: "miniapp",
    path: "../wit",
    imports: { default: async },
    exports: { default: async },
});

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl spike::host::host_api::Host for State {
    async fn log(&mut self, msg: String) {
        println!("    [guest log] {msg}");
    }
    async fn slow_add(&mut self, a: u32, b: u32) -> u32 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await; // I/O async do host
        a + b
    }
}

fn new_store(engine: &Engine, fuel: u64, mem_mb: usize) -> Store<State> {
    // Sem preopens, sem env, sem rede: nenhuma permissão WASI concedida.
    let wasi = WasiCtxBuilder::new().build();
    let limits = StoreLimitsBuilder::new()
        .memory_size(mem_mb * 1024 * 1024)
        .build();
    let mut store = Store::new(
        engine,
        State {
            wasi,
            table: ResourceTable::new(),
            limits,
        },
    );
    store.limiter(|s| &mut s.limits);
    store.set_fuel(fuel).unwrap();
    store
}

fn verdict<T: std::fmt::Debug>(name: &str, r: Result<T>) {
    match r {
        Ok(v) => println!("  {name}: OK -> {v:?}"),
        Err(e) => {
            let root = e.root_cause().to_string();
            let short = root.lines().next().unwrap_or("");
            println!("  {name}: CONTIDO (trap) -> {short}");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("uso: host <componente.wasm>");

    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    let engine = Engine::new(&cfg)?;

    // Compilação AOT (custo pago uma vez) e carga da forma pré-compilada.
    let t = Instant::now();
    let bytes = std::fs::read(&path)?;
    let precompiled = engine.precompile_component(&bytes)?;
    println!("precompile: {:?} ({} KB wasm -> {} KB nativo)", t.elapsed(), bytes.len() / 1024, precompiled.len() / 1024);

    let component = unsafe { Component::deserialize(&engine, &precompiled)? };
    let mut linker: Linker<State> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    Miniapp::add_to_linker::<State, wasmtime::component::HasSelf<State>>(&mut linker, |s| s)?;
    let pre = MiniappPre::new(linker.instantiate_pre(&component)?)?;

    // 1) Custo de instanciação (o que importa para "cold start de mini-app").
    let n = 1000;
    let mut total = std::time::Duration::ZERO;
    for _ in 0..n {
        let mut store = new_store(&engine, 1_000_000, 16);
        let t = Instant::now();
        let _app = pre.instantiate_async(&mut store).await?;
        total += t.elapsed();
    }
    println!("instanciacao: media {:?} em {n} execucoes", total / n);

    // 2) Chamada normal com import async do host.
    println!("\ncasos:");
    let mut store = new_store(&engine, 10_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    verdict("run(41) c/ import async", app.call_run(&mut store, 41).await.map_err(Into::into));

    // 3) Loop infinito -> fuel esgotado.
    let mut store = new_store(&engine, 5_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    let t = Instant::now();
    verdict("spin() fuel=5M", app.call_spin(&mut store).await.map_err(Into::into));
    println!("    tempo ate abortar: {:?}", t.elapsed());

    // 4) Estouro de memoria (limite 16 MB, guest pede 64 MB).
    let mut store = new_store(&engine, 100_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    verdict("hog(64MB) limite=16MB", app.call_hog(&mut store, 64).await.map_err(Into::into));

    // 5) Dentro do limite deve funcionar.
    let mut store = new_store(&engine, 100_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    verdict("hog(4MB) limite=16MB", app.call_hog(&mut store, 4).await.map_err(Into::into));

    // 6) FS e rede nao declarados.
    let mut store = new_store(&engine, 100_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    verdict("try_fs()", app.call_try_fs(&mut store).await.map_err(Into::into));
    let mut store = new_store(&engine, 100_000_000, 16);
    let app = pre.instantiate_async(&mut store).await?;
    verdict("try_net()", app.call_try_net(&mut store).await.map_err(Into::into));

    Ok(())
}
