use std::net::TcpStream;
use std::time::Duration;

fn main() {
    let targets = [
        ("192.168.1.17:8080", "LAN server"),
        ("1.1.1.1:80", "Cloudflare external"),
        ("93.184.216.34:80", "example.com external"),
    ];

    for (addr, label) in &targets {
        print!("{label} ({addr}): ");
        match TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_secs(3),
        ) {
            Ok(_) => println!("OK"),
            Err(e) => println!("FAIL - {e}"),
        }
    }
}
