use crate::rules::Diagnostic;
use anyhow::Result;

pub fn report(diagnostics: &[Diagnostic]) -> Result<()> {
    let json = serde_json::to_string_pretty(diagnostics)?;
    println!("{}", json);
    Ok(())
}
