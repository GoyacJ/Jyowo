use std::io::{self, Write};

use harness_contracts::daemon_protocol_schema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = daemon_protocol_schema();
    let json = serde_json::to_string_pretty(&schema)?;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    writeln!(stdout, "{json}")?;
    Ok(())
}
