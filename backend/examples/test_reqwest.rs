use std::error::Error;

#[tokio::main]
async fn main() {
    println!("=== Default client ===");
    test_with(reqwest::Client::new()).await;

    println!("\n=== IPv4 only ===");
    let client = reqwest::Client::builder()
        .local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
        .build()
        .unwrap();
    test_with(client).await;

    println!("\n=== No proxy ===");
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    test_with(client).await;
}

async fn test_with(client: reqwest::Client) {
    match client.get("http://192.168.1.17:8080/v1/models").send().await {
        Ok(resp) => println!("OK: {}", resp.status()),
        Err(e) => {
            println!("Error: {e}");
            print_error_chain(&e);
        }
    }
}

fn print_error_chain(e: &dyn Error) {
    let mut source = e.source();
    let mut depth = 1;
    while let Some(s) = source {
        println!("  cause {depth}: {s}");
        source = s.source();
        depth += 1;
    }
}
