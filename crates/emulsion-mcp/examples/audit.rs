//! Export the current MCP inventory without calling tools or touching artwork.
fn main() {
    let report = emulsion_mcp::audit::report();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if !report["issues"].as_array().unwrap().is_empty() {
        std::process::exit(1);
    }
}
