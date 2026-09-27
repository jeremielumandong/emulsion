//! Check Google endpoint reachability without reading registrations or tokens.
fn main() -> anyhow::Result<()> {
    let client = emulsion_cloud::http::Client::default();
    for (name, method, url, expected) in [
        (
            "Google sign-in discovery",
            "GET",
            "https://accounts.google.com/.well-known/openid-configuration",
            200,
        ),
        (
            "Google token endpoint (unauthenticated)",
            "POST",
            "https://oauth2.googleapis.com/token",
            400,
        ),
        (
            "Google account lookup (unauthenticated)",
            "GET",
            "https://openidconnect.googleapis.com/v1/userinfo",
            401,
        ),
    ] {
        let response = client
            .send(method, url, None, &[], &[])
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        println!("{name}: HTTP {}", response.status);
        anyhow::ensure!(
            response.status == expected,
            "Unexpected endpoint response; no credentials were sent"
        );
    }
    println!(
        "Network checks passed. Account sign-in and API enablement still require an in-app connection."
    );
    Ok(())
}
