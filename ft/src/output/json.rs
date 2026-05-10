use anyhow::Result;
use ft_core::task::Task;

/// Pretty-print all tasks as a JSON array on stdout.
pub fn render(tasks: &[&Task]) -> Result<()> {
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, tasks)?;
    println!();
    Ok(())
}
