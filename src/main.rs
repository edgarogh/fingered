#[macro_use]
extern crate tracing;

use crate::config::Config;
use crate::listener::{AnyListener, AnySocketAddr};
use crate::request::Request;
use clap::builder::TypedValueParser;
use clap::Parser;
use futures::StreamExt;
use listenfd::ListenFd;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook_tokio::Signals;
use std::borrow::Borrow;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};
use tokio::net::TcpListener;
use tokio::select;
use tracing::instrument;
use tracing_subscriber::EnvFilter;

mod config;
mod listener;
mod request;

const FINGER_PORT: u16 = 79;

/// Max length of a request in bytes
///
/// The input stream will be truncated to this limit to prevent DoS.
const SANE_REQUEST_LENGTH: u64 = 1024;

/// Server-sent reply when a client includes a "@host..." forwarding request in their message
///
/// Copy-pasted straight from [IETF's RFC 1288][rfc]'s suggestion.
///
/// [rfc]: https://datatracker.ietf.org/doc/html/rfc1288#section-3.2.1
const REPLY_NO_FORWARDING: &[u8] = b"Finger forwarding service denied\r\n";

/// Server-sent reply when a client tries to list users and the server denies it
///
/// Copy-pasted straight from [IETF's RFC 1288][rfc]'s suggestion.
///
/// [rfc]: https://datatracker.ietf.org/doc/html/rfc1288#section-3.2.2
const REPLY_NO_LISTING: &[u8] = b"Finger online user list denied\r\n";

/// Server-sent reply when a client fingers a nonexistent username
const REPLY_USER_NOT_FOUND: &[u8] = b"User not found\r\n";

#[derive(Parser)]
#[clap(about, version)]
pub struct Args {
    /// IP address or Unix socket path to listen on
    ///
    /// May be omitted if the program is started with socket activation.
    /// Must be omitted if `--inetd` is given.
    // It would've been simpler to just implement `FromStr` BUT I want to be able to parse non-UTF-8
    // unix socket paths (only representable as OsStr/OsString).
    #[clap(value_parser = clap::builder::OsStringValueParser::new().try_map(|str| AnySocketAddr::try_from(str.as_ref())))]
    bind_to: Option<AnySocketAddr>,

    /// Run as an inetd-compatible child process, treating stdin and stdout as a socket
    #[clap(long, conflicts_with = "bind_to")]
    inetd: bool,

    /// Path to the `users.toml` file
    #[clap(long, default_value = "/etc/fingered/users.toml")]
    users_file: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.inetd {
        main_inetd(args).await
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();

        main_daemon(args).await
    }
}

async fn main_daemon(args: Args) {
    info!("starting daemon");

    let mut listen_fd = ListenFd::from_env();
    let (server, local_addr) = if let Some(bind_to) = args.bind_to {
        let server = match AnyListener::bind(&bind_to).await {
            Ok(server) => server,
            Err(err) => {
                error!("cannot bind to {}: {err}", bind_to);
                return;
            }
        };

        (server, bind_to)
    } else if let Ok(Some(tcp)) = listen_fd.take_tcp_listener(0) {
        let local_addr = tcp.local_addr().unwrap();
        let listener = TcpListener::from_std(tcp).unwrap();
        info!("tcp socket descriptor given on LISTEN_FDS, listening on it");
        (listener.into(), local_addr.into())
    } else {
        // Fake a missing argument
        Args::parse_from(["", "--help"]);
        unreachable!();
    };

    info!("listening on {}", local_addr);

    let users_file = Arc::<Path>::from(args.users_file);
    let users = tokio::fs::read_to_string(users_file.as_ref())
        .await
        .unwrap();

    let config = Arc::new(Config::new_parsed(&users).unwrap());
    validate_config(config.get().await.as_ref());

    let mut signals = Signals::new([SIGHUP, SIGINT, SIGQUIT, SIGTERM]).unwrap();

    loop {
        let client = select! { biased;
            Some(signal) = signals.next() => match signal {
                SIGINT | SIGQUIT | SIGTERM => break,
                SIGHUP => {
                    let config = Arc::clone(&config);
                    tokio::task::spawn(reload_config(Arc::clone(&users_file), config));
                    continue;
                },
                _ => unreachable!()
            },
            accepted = server.accept() => accepted.unwrap(),
        };

        let config = config.get().await;
        tokio::task::spawn(async move {
            let mut client = client;
            let peer_display = client.peer_display();
            let mut client = client.split();
            let (input, output) = client.as_parts();
            handle_client(&peer_display, &config, input, output).await
        });
    }

    info!("exited gracefully");
}

async fn main_inetd(_args: Args) {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();

    // We're not bothering with the async runtime
    let users = std::fs::read_to_string("./users.toml").unwrap();
    let users = toml::from_str::<config::Users>(&users).unwrap();
    handle_client(&"inetd", &users, &mut input, &mut output)
        .await
        .unwrap();
}

#[instrument(skip_all, fields(peer = %_peer))]
async fn handle_client(
    _peer: &(dyn std::fmt::Display + Sync),
    users: &(dyn Borrow<config::Users> + Sync),
    input: &mut (dyn AsyncRead + Send + Unpin),
    output: &mut (dyn AsyncWrite + Send + Unpin),
) -> io::Result<()> {
    debug!("incoming request");
    let users = users.borrow();
    let mut reader = BufReader::new(input.take(SANE_REQUEST_LENGTH));
    let mut writer = BufWriter::new(output);
    let mut buffer = Vec::with_capacity(32);
    reader.read_until(b'\n', &mut buffer).await?;
    let buffer = std::str::from_utf8(&buffer).unwrap();
    let req = Request::from_str(buffer).unwrap();

    if req.forwarding.is_some() {
        writer.write_all(REPLY_NO_FORWARDING).await?;
    } else if let Some(username) = req.user {
        if let Some(user) = users.find(username) {
            debug!("requested user {username:?}");

            let info = match req.verbose {
                false => user.info(),
                true => user.long_info(),
            };

            writer.write_all(info.as_bytes()).await?;
        } else {
            debug!("requested nonexistent user {username:?}");
            writer.write_all(REPLY_USER_NOT_FOUND).await?;
        }
    } else {
        debug!("requested user list");
        if users.enable_index {
            for (name, user) in &users.users {
                if !user.unlisted {
                    writer.write_all(name.as_bytes()).await?;
                    writer.write_all(b"\r\n").await?;
                }
            }
        } else {
            debug!("user list denied by config");
            writer.write_all(REPLY_NO_LISTING).await?;
        }
    }

    writer.flush().await.unwrap();

    Ok(())
}

fn validate_config(users: &config::Users) {
    for (name, user) in &users.users {
        if matches!(&user.info, Some(info) if !info.is_ascii()) {
            warn!("user {name:?}'s info contains non-ASCII characters; most clients won't render them correctly")
        }
        if matches!(&user.long_info, Some(info) if !info.is_ascii()) {
            warn!("user {name:?}'s long-info contains non-ASCII characters; most clients won't render them correctly")
        }
    }
}

#[instrument(skip_all)]
async fn reload_config(config_file_path: impl AsRef<Path>, config: impl Borrow<Config>) {
    info!("reloading config");

    let source = match tokio::fs::read_to_string(config_file_path.as_ref()).await {
        Ok(source) => source,
        Err(err) => {
            error!("cannot open config file: {err}");
            return;
        }
    };

    if let Err(err) = config.borrow().load(&source).await {
        error!("cannot parse config file: {err}");
    }
}
