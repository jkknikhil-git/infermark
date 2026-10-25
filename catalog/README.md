# Model catalog

[`models.toml`](models.toml) lists the open-weight GGUF models the app can download. Each entry has:

- a download URL **pinned to a commit**, and the file's exact size and SHA-256. A download that doesn't match is deleted and never used.
- the parameter count, quantisation and license, shown before downloading
- the model's shape (layers, KV heads, head size, context length). The hardware gate uses it to
  work out how much memory the model needs before anything is downloaded.
- the matching Ollama tag, if there is one

Downloads are accepted only over HTTPS and only from allow-listed hosts (Hugging Face).

## Adding a model

1. Find a single-file GGUF on Hugging Face. Split files aren't supported yet.
2. Generate the entry:

   ```sh
   cargo run -p infermark-models --example catalog_entry -- <hf-repo> <file.gguf>
   ```

   The tool gets the size and SHA-256 from the Hugging Face API and the shape from the first
   few megabytes of the file. It never downloads the whole model.
3. Add `id`, `name`, `family`, `params_b`, `quant`, `license`, `tags` and `ollama`. Keep the
   list ordered by size, then open a pull request. `cargo test -p infermark-models` checks every entry.
