pub fn parse_on_off(s: &str) -> anyhow::Result<bool> {
    match s.to_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(anyhow::anyhow!("Expected 'on' or 'off', got: {}", s)),
    }
}
