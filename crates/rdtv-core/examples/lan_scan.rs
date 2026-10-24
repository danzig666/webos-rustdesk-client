//! Lists RustDesk computers answering LAN discovery: `cargo run -p rdtv-core --example lan_scan`.
fn main() {
    match rdtv_core::lan::discover(std::time::Duration::from_millis(2500)) {
        Ok(peers) if peers.is_empty() => println!("no RustDesk computers answered"),
        Ok(peers) => {
            for p in peers {
                println!(
                    "{} {} user={} platform={} mac={} ip={}",
                    p.id, p.hostname, p.username, p.platform, p.mac, p.ip
                );
            }
        }
        Err(e) => println!("discovery failed: {e}"),
    }
}
