mod router;

use crate::config::apps::Apps;
use anyhow::{Context, Result};
use essentials::debug;
use gateway::tokio_rustls::{
    rustls::{
        crypto::aws_lc_rs::sign::any_supported_type,
        pki_types::{CertificateDer, PrivateKeyDer},
        server::ResolvesServerCertUsingSni,
        sign::CertifiedKey,
        ServerConfig,
    },
    TlsAcceptor,
};
use gateway::{self, http::HeaderMapExt, tcp, Request, Server};
use http::header;
use router::AnyRouterBuilder;
use rustls_pemfile::{certs, private_key};
use std::{
    fs::File,
    io::{self, BufReader},
    sync::Arc,
};
use std::{net::IpAddr, path::Path};
use tokio::fs;

use crate::env::Env;

async fn load_config(config_path: impl AsRef<Path>) -> Result<Apps> {
    let config_data = fs::read_to_string(config_path)
        .await
        .with_context(|| "Failed to read config file")?;
    Apps::new(config_data).with_context(|| "Failed to parse config file")
}

fn peer_key_from_host() -> impl Fn(&Request) -> Option<String> + Send + Sync + 'static {
    |req: &Request| {
        req.header(header::HOST)
            .and_then(|host| host.to_str().ok())
            .map(|host| host.split(':').next().unwrap().to_string())
    }
}

fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    certs(&mut BufReader::new(File::open(path)?)).collect()
}

fn load_keys(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    private_key(&mut BufReader::new(File::open(path)?))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No keys found in file"))
}

pub async fn build(env: Env) -> Result<Server> {
    let config = load_config(env.config_file).await?;
    debug!("{:?}", config);
    let (peers, configs) = config.apps.into_iter().collect::<(Vec<_>, Vec<_>)>();
    let mut builder = gateway::builder(
        tcp::Builder::build(
            configs
                .iter()
                .map(|app| {
                    (
                        app.name.clone(),
                        tcp::config::Connection::new(
                            format!("{}:{}", app.upstream.host, app.upstream.port),
                            app.hostname.clone(),
                        ),
                    )
                })
                .collect(),
        ),
        peer_key_from_host(),
    )
    .with_app_port(env.http_port.unwrap_or(80))
    .with_health_check_port(env.healthcheck_port.unwrap_or(9000))
    .with_host(env.host.unwrap_or(IpAddr::from([127, 0, 0, 1])));
    for peer in peers.into_iter() {
        builder = builder.register_peer(peer, AnyRouterBuilder);
    }
    let mut tls_resolver = ResolvesServerCertUsingSni::new();
    for config in configs.into_iter() {
        let folders = env.certs_dir.join(&config.name);
        let certs = load_certs(folders.join("cert.pem").as_path())?;
        let key = load_keys(folders.join("key.pem").as_path())?;
        let key = any_supported_type(&key)?;
        let private_key = CertifiedKey::new(certs, key);
        tls_resolver.add(&config.name, private_key)?;
    }
    let tls_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(tls_resolver));
    let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));
    builder
        .with_tls(env.https_port.unwrap_or(443), tls_acceptor)
        .build()
        .await
}
