use log::info;
use stun::addr::*;
use stun::agent::*;
use stun::client::*;
use stun::message::*;
use stun::xoraddr::*;
use stun::Error;

use clap::{arg, command, Arg, ArgAction};
use std::sync::Arc;
use tokio::net::{lookup_host, UdpSocket};

#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    let matches = command!() // requires `cargo` feature
        .arg(
            arg!([SERVER] "STUN server host and port. If PORT is omitted, it defaults to 3478")
                .value_name("HOST:PORT")
                .default_value("stun.cloudflare.com"),
        )
        .arg(
            Arg::new("ipv4_only")
                .long("ipv4-only")
                .short('4')
                .required(false)
                .help("Do not use IPv6 addresses when connecting to STUN server")
                .action(ArgAction::SetTrue),
        )
        .get_matches();

    let (handler_tx, mut handler_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut server = matches.get_one::<String>("SERVER").unwrap().clone();
    if server.as_str().starts_with('[') {
        // IPv6 literal
        if !server.contains("]:") {
            server.push_str(":3478");
        }
    } else if !server.contains(':') {
        server.push_str(":3478");
    }

    let ipv4_only = matches.get_flag("ipv4_only");

    let addr = lookup_host(server)
        .await
        .expect("failed to parse/resolve server address and port number")
        .find(|addr| !ipv4_only || addr.is_ipv4())
        .expect("unable to resolve remote host name");

    info!("Server address is: {}", addr);

    let conn = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .await?;

    conn.connect(addr).await?;

    let mut client = ClientBuilder::new().with_conn(Arc::new(conn)).build()?;

    let mut msg = Message::new();
    msg.build(&[Box::<TransactionId>::default(), Box::new(BINDING_REQUEST)])?;

    client.send(&msg, Some(Arc::new(handler_tx))).await?;

    if let Some(event) = handler_rx.recv().await {
        let msg = event.event_body?;
        let mut xor_addr = XorMappedAddress::default();
        if xor_addr.get_from(&msg).is_ok() {
            println!("{}", xor_addr.ip);
        } else {
            info!("XOR-MAPPED-ADDRESS attribute does not exist, trying MAPPED-ADDRESS instead");

            let mut addr = MappedAddress::default();
            addr.get_from(&msg)?;
            println!("{}", addr.ip);
        }
    }

    client.close().await?;

    Ok(())
}
