//! Local registration import without displaying credential contents.
//! cargo run -p emulsion-cloud --example configure -- /path/to/cloud-dir /path/to/desktop.json
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().ok_or_else(|| {
        anyhow::anyhow!("Usage: configure CLOUD_DATA_DIRECTORY DESKTOP_REGISTRATION_JSON")
    })?;
    let source = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing registration file path"))?;
    anyhow::ensure!(args.next().is_none(), "Unexpected extra argument");
    let store = emulsion_cloud::Store::new(std::path::PathBuf::from(root));
    emulsion_cloud::auth::Config::import(&store, std::path::Path::new(&source))?;
    println!("Registration imported locally. No account sign-in or file transfer was performed.");
    Ok(())
}
