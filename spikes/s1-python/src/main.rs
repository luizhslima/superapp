// Spike S1: subinterpretadores CPython 3.12 com GIL próprio, dirigidos pelo Rust.
#![allow(non_snake_case)]
use pyo3_ffi::*;
use std::ffi::{c_char, c_int, CStr, CString};
use std::time::Instant;

#[repr(C)]
struct InterpConfig {
    use_main_obmalloc: c_int,
    allow_fork: c_int,
    allow_exec: c_int,
    allow_threads: c_int,
    allow_daemon_threads: c_int,
    check_multi_interp_extensions: c_int,
    gil: c_int, // 1 = SHARED, 2 = OWN
}

#[repr(C)]
struct Status {
    kind: c_int,
    func: *const c_char,
    err_msg: *const c_char,
    exitcode: c_int,
}

extern "C" {
    fn Py_NewInterpreterFromConfig(tstate: *mut *mut PyThreadState, cfg: *const InterpConfig) -> Status;
}

const WORK: &str = "
def work(n):
    s = 0
    for i in range(n):
        s += (i * i) % 7
    return s
r = work(%N%)
";

fn run(code: &str) -> String {
    unsafe {
        let src = CString::new(format!(
            "{code}\n"
        ))
        .unwrap();
        let rc = PyRun_SimpleString(src.as_ptr());
        if rc != 0 {
            return "erro no script".into();
        }
        let m = PyImport_AddModule(c"__main__".as_ptr());
        let r = PyObject_GetAttrString(m, c"r".as_ptr());
        if r.is_null() {
            PyErr_Clear();
            return "sem r".into();
        }
        let s = PyObject_Str(r);
        let c = PyUnicode_AsUTF8(s);
        let out = CStr::from_ptr(c).to_string_lossy().into_owned();
        Py_DecRef(s);
        Py_DecRef(r);
        out
    }
}

/// Executa `code` em um subinterpretador novo (config dada). Retorna (saída, ok na criação).
fn in_subinterp(own_gil: bool, strict_ext: bool, code: &str) -> Result<String, String> {
    unsafe {
        let g = PyGILState_Ensure();
        let save = PyThreadState_Get();
        let cfg = InterpConfig {
            use_main_obmalloc: 0,
            allow_fork: 0,
            allow_exec: 0,
            allow_threads: 1,
            allow_daemon_threads: 0,
            check_multi_interp_extensions: strict_ext as c_int,
            gil: if own_gil { 2 } else { 1 },
        };
        let mut ts: *mut PyThreadState = std::ptr::null_mut();
        let st = Py_NewInterpreterFromConfig(&mut ts, &cfg);
        if st.kind != 0 {
            PyThreadState_Swap(save);
            PyGILState_Release(g);
            let msg = if st.err_msg.is_null() { "?".into() } else { CStr::from_ptr(st.err_msg).to_string_lossy().into_owned() };
            return Err(format!("criacao falhou: {msg}"));
        }
        let out = run(code);
        Py_EndInterpreter(ts);
        PyThreadState_Swap(save);
        PyGILState_Release(g);
        Ok(out)
    }
}

fn main() {
    let n: u64 = std::env::var("S1_N").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000_000);
    let workers = 4;
    let code = WORK.replace("%N%", &n.to_string());

    unsafe {
        Py_InitializeEx(0);
        let main_ts = PyEval_SaveThread(); // libera o GIL principal
        let _ = main_ts;
    }

    // 1) serial no interpretador principal.
    let t = Instant::now();
    for _ in 0..workers {
        unsafe {
            let g = PyGILState_Ensure();
            run(&code);
            PyGILState_Release(g);
        }
    }
    let serial = t.elapsed();

    // 2) 4 threads no MESMO interpretador (GIL compartilhado).
    let t = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| unsafe {
                let g = PyGILState_Ensure();
                run(&code);
                PyGILState_Release(g);
            });
        }
    });
    let shared = t.elapsed();

    // 3) 4 subinterpretadores, GIL próprio, em paralelo.
    let t = Instant::now();
    let mut results = vec![];
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..workers).map(|_| s.spawn(|| in_subinterp(true, true, &code))).collect();
        for h in hs {
            results.push(h.join().unwrap());
        }
    });
    let own = t.elapsed();

    println!("N = {n} iteracoes por worker, {workers} workers");
    println!("serial (1 interp, sequencial):       {serial:?}");
    println!("4 threads, 1 interp (GIL unico):     {shared:?}  speedup {:.2}x", serial.as_secs_f64() / shared.as_secs_f64());
    println!("4 subinterps GIL proprio (paralelo): {own:?}  speedup {:.2}x", serial.as_secs_f64() / own.as_secs_f64());
    println!("resultados subinterps: {:?}", results);

    // 4) compatibilidade de bibliotecas.
    println!("\nimports:");
    for lib in ["numpy", "pyarrow", "json"] {
        let code = format!("try:\n    import {lib}\n    r = 'ok'\nexcept BaseException as e:\n    r = type(e).__name__ + ': ' + str(e)[:110]");
        let main_res = unsafe {
            let g = PyGILState_Ensure();
            let o = run(&code);
            PyGILState_Release(g);
            o
        };
        let sub = in_subinterp(true, true, &code).unwrap_or_else(|e| e);
        println!("  {lib:8} principal: {main_res:50} | subinterp GIL proprio: {sub}");
    }
}
