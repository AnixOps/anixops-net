//! Expose the same mesh view commands the desktop bridge invokes, for browser compatibility evidence.

use std::io;

use serde_json::{json, Map, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let status: Value = serde_json::from_reader(io::stdin().lock())?;
    let args = json!({ "status": status, "groupBy": "region", "filter": "all", "sort": "name" });
    let mut result = Map::new();
    for command in [
        "mesh_entry",
        "mesh_members",
        "mesh_subnets",
        "mesh_paths",
        "mesh_notices",
    ] {
        let view = client_view::invoke(command, &args).expect("mesh view command")?;
        result.insert(format!("view.{command}"), view);
    }
    result.insert("status".into(), status);
    serde_json::to_writer(io::stdout().lock(), &result)?;
    Ok(())
}
