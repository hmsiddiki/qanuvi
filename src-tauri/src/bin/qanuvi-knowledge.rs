//! `qanuvi-knowledge`: makes the key pair and signs Qanuvi's knowledge files (24c). The private key file stays with
//! the publisher, never in the repository or the published folder.
//!
//! qanuvi-knowledge keygen <private-key-file>     writes a new private key, prints the public key
//! qanuvi-knowledge sign <knowledge.json> <private-key-file>   checks the JSON and writes <knowledge.json>.sig

use qanuvi_lib::knowledge;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen", key] => keygen(Path::new(key)),
        ["sign", file, key] => sign(Path::new(file), Path::new(key)),
        _ => Err("Usage: qanuvi-knowledge keygen <private-key-file> | sign <knowledge.json> <private-key-file>".into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn keygen(key: &Path) -> Result<(), String> {
    if key.exists() {
        return Err(format!(
            "{} exists already; it is not replaced.",
            key.display()
        ));
    }
    let (secret, public) = knowledge::keygen()?;
    if let Some(dir) = key.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(key, secret).map_err(|e| e.to_string())?;
    println!(
        "Private key written to {}. Public key:\n{public}",
        key.display()
    );
    Ok(())
}

fn sign(file: &Path, key: &Path) -> Result<(), String> {
    let body = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| format!("{} is not JSON: {e}", file.display()))?;
    let secret = fs::read_to_string(key).map_err(|e| format!("{}: {e}", key.display()))?;
    let sig = knowledge::sign(&body, &secret)?;
    let out = format!("{}.sig", file.display());
    fs::write(&out, sig).map_err(|e| e.to_string())?;
    println!("Signed: {out}");
    Ok(())
}
