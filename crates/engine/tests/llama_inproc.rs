//! Runs a real model through the in-process llama.cpp backend.
//!
//! Needs `--features llama-inproc` and a small GGUF in `INFERMARK_TEST_GGUF`; CI downloads
//! `stories15M-q8_0.gguf` (27 MB, hash-checked). Without the variable the test is skipped,
//! except in CI where a missing model is a failure.

#![cfg(feature = "llama-inproc")]

use infermark_engine::LlamaInProcess;
use infermark_sdk::{Backend, GenerateRequest, LoadOptions, ModelSpec};

fn model() -> Option<ModelSpec> {
    let Ok(path) = std::env::var("INFERMARK_TEST_GGUF") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "INFERMARK_TEST_GGUF must point at a GGUF model in CI"
        );
        eprintln!("skipped: set INFERMARK_TEST_GGUF to a small GGUF model");
        return None;
    };
    Some(ModelSpec {
        id: "test-model".into(),
        location: path,
        sha256: None,
    })
}

fn request(max_tokens: u32) -> GenerateRequest {
    GenerateRequest {
        system: None,
        prompt: "Once upon a time".into(),
        max_tokens,
        temperature: 0.0,
        stop: vec![],
        think: false,
        ignore_eos: true,
    }
}

#[test]
fn generates_tokens_with_timings() {
    let Some(spec) = model() else { return };
    let mut backend = LlamaInProcess::new().expect("llama.cpp starts");
    println!("engine: {}", backend.version());
    let options = LoadOptions {
        context_size: 512,
        threads: Some(2),
        gpu_layers: None,
        seed: 42,
    };
    backend.load(&spec, &options).expect("model loads");

    let mut text = String::new();
    let mut events = 0;
    let stats = backend
        .generate(&request(16), &mut |ev| {
            assert!(!ev.thinking);
            events += 1;
            text.push_str(&ev.text);
        })
        .expect("generates");
    println!("{text:?} {stats:?}");
    // `ignore_eos` always produces exactly `max_tokens`.
    assert_eq!(stats.output_tokens, 16);
    assert!(events > 0 && !text.trim().is_empty());
    assert!(stats.prompt_tokens >= 4, "{stats:?}");
    assert!(stats.load_ms > 0.0);
    assert!(stats.prompt_eval_ms > 0.0 && stats.generation_ms > 0.0);
    assert!(stats.time_to_first_token_ms > 0.0);
    assert!(stats.time_to_first_token_ms <= stats.prompt_eval_ms + stats.generation_ms + 1.0);
    assert!(stats.time_to_first_visible_token_ms.is_some());
    assert!(stats.generation_tokens_per_sec() > 0.0);

    // No prompt cache: the second run reads the whole prompt again and, greedy, says the same.
    let mut again = String::new();
    let second = backend
        .generate(&request(16), &mut |ev| again.push_str(&ev.text))
        .expect("generates again");
    assert_eq!(second.prompt_tokens, stats.prompt_tokens);
    assert_eq!(again, text);

    // Stop strings end the answer and are not shown.
    let stop = GenerateRequest {
        stop: vec![".".into()],
        ..request(64)
    };
    let mut stopped = String::new();
    let s = backend
        .generate(&stop, &mut |ev| stopped.push_str(&ev.text))
        .expect("generates with a stop string");
    println!("{stopped:?} {s:?}");
    assert!(!stopped.contains('.'), "{stopped:?}");
    assert!(s.output_tokens < 64, "{s:?}");

    backend.unload();
    let err = backend.generate(&request(4), &mut |_| {}).unwrap_err();
    assert_eq!(err, "no model loaded");
}
