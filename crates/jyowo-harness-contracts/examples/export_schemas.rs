use std::fs;
use std::path::Path;

use harness_contracts::export_all_schemas;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new("schemas/harness-contracts");
    fs::create_dir_all(out_dir)?;

    for (name, schema) in export_all_schemas() {
        let path = out_dir.join(format!("{name}.json"));
        let json = serde_json::to_string_pretty(&schema)?;
        fs::write(path, format!("{json}\n"))?;
    }

    Ok(())
}
