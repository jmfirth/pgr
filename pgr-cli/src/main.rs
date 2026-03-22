#![warn(clippy::pedantic)]

#[allow(clippy::unnecessary_wraps)] // main will propagate errors once real logic is added
fn main() -> anyhow::Result<()> {
    println!("pgr");
    Ok(())
}
