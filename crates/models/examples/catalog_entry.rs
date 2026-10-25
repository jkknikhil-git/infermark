//! Contributor tool: print a `catalog/models.toml` entry for a GGUF file on Hugging Face.
//!
//! ```text
//! cargo run -p infermark-models --example catalog_entry -- <repo> <file>
//! ```
//!
//! Size and SHA-256 come from the Hugging Face API; the model shape is read from the GGUF
//! header by streaming only the first few megabytes of the file.

use infermark_models::gguf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [repo, file] = args.as_slice() else {
        eprintln!("usage: catalog_entry <hf-repo> <file.gguf>");
        std::process::exit(2);
    };
    let get_json = |url: String| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_reader(
            ureq::get(&url).call()?.into_reader(),
        )?)
    };
    let meta = get_json(format!("https://huggingface.co/api/models/{repo}"))?;
    let revision = meta["sha"].as_str().ok_or("no commit sha")?;
    let tree = get_json(format!(
        "https://huggingface.co/api/models/{repo}/tree/{revision}"
    ))?;
    let entry = tree
        .as_array()
        .and_then(|a| a.iter().find(|f| f["path"] == file.as_str()))
        .ok_or("file not found in repo")?;
    let size = entry["lfs"]["size"].as_u64().ok_or("no lfs size")?;
    let sha = entry["lfs"]["oid"].as_str().ok_or("no lfs sha256")?;

    let url = format!("https://huggingface.co/{repo}/resolve/{revision}/{file}");
    let mut body = ureq::get(&url).call()?.into_reader();
    let header = gguf::read_header(&mut body)?;
    let s = header.shape()?;

    println!("url = \"{url}\"");
    println!("size_bytes = {size}");
    println!("sha256 = \"{sha}\"");
    println!(
        "# architecture: {:?}, name: {:?}",
        header.architecture(),
        header.name()
    );
    println!("[model.shape]");
    println!("layers = {}", s.layers);
    println!("kv_heads = {}", s.kv_heads);
    println!("head_dim = {}", s.head_dim);
    println!("context_length = {}", s.context_length);
    if let Some(f) = s.active_fraction {
        println!("active_fraction = {f:.4}");
    }
    Ok(())
}
