use async_trait::async_trait;
use essentials::{debug, info};
use gateway::http::response::ResponseBody;
use gateway::http::HeaderMapExt;
use gateway::tokio_rustls::rustls::{
    crypto::aws_lc_rs::sign::any_supported_type,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::ResolvesServerCertUsingSni,
};
use gateway::{
    ReadRequest, ReadResponse, Request, Response, WriteHalf, WriteRequest, WriteResponse,
};
use http::StatusCode;
use rcgen::generate_simple_self_signed;
use rustls_pemfile::{certs, private_key};
use serde_json::json;
use std::fs::read_to_string;
use std::fs::File;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::{env, sync::Arc};
use testing_utils::fs::fixture::ChildPath;
use testing_utils::fs::prelude::{FileTouch, FileWriteStr, PathChild, PathCreateDir};
use testing_utils::fs::TempDir;
use testing_utils::{fs, server_cmd};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn single_server_config(app_port: u16, cert: &str, hostname: &str) -> serde_json::Value {
    json!({
        "certs": [
            cert
        ],
        "apps": {
            "hello.world.example": {
                "hostname": hostname,
                "upstream": {
                    "host": "127.0.0.1",
                    "port": app_port
                }
            }
        }
    })
}

pub struct Context {
    app: u16,
    pub domain: String,
    connector: TlsConnector,
    _origin_server: JoinHandle<()>,
    _app_server: Child,
}

#[derive(Debug)]
pub struct Body(pub String);

#[async_trait]
impl ResponseBody for Body {
    async fn read_all(self: Box<Self>, _len: usize) -> io::Result<String> {
        Ok(self.0)
    }

    async fn copy_to<'a>(
        &mut self,
        writer: &'a mut WriteHalf,
        _length: Option<usize>,
    ) -> io::Result<()> {
        writer.write_all(self.0.as_bytes()).await?;
        Ok(())
    }
}

pub async fn run_request(request: Request, ctx: &Context) -> Response {
    let stream = TcpStream::connect(&format!("127.0.0.1:{}", ctx.app))
        .await
        .unwrap();
    let domain = ServerName::try_from(ctx.domain.as_str())
        .unwrap()
        .to_owned();
    let mut stream = ctx.connector.connect(domain, stream).await.unwrap();
    stream.write_request(&request).await.unwrap();
    stream.flush().await.unwrap();
    // stream.shutdown().await.unwrap();
    let (mut response, remains) = stream.read_response().await.unwrap();
    debug!(?response, "read response");
    let mut body = String::from_utf8(remains.to_vec()).unwrap();
    let length = response
        .get_content_length()
        .unwrap()
        .saturating_sub(remains.len());
    if length > 0 {
        let mut buf = vec![0; length];
        stream.read_exact(&mut buf).await.unwrap();
        body.push_str(&String::from_utf8(buf).unwrap());
    }
    debug!(?response, "read response body");
    response.set_body(Body(body));
    response
}

pub async fn before_each() -> Context {
    if env::var("CI").is_err() {
        env::set_var("RUST_LOG", "debug");
        env::set_var("RUST_BACKTRACE", "0");
        env::set_var("APP_ENV", "d");
        essentials::install();
    }

    let domain = "hello.world.example";
    let origin_domain = "app.papi.prod.localhost";
    let temp = fs::TempDir::new().unwrap();
    let (certs_dir, connector) = setup_tls(domain, &temp);
    let origin_certs_dir = setup_tls(origin_domain, &temp)
        .0
        .to_path_buf()
        .join(origin_domain);
    let origin_cert = origin_certs_dir.join("cert.pem");
    let (mock_server, mock_port) = create_origin_server(origin_certs_dir, origin_domain).await;
    let input_file = create_config(
        mock_port,
        &temp,
        origin_cert.to_str().unwrap(),
        origin_domain,
    );
    let ports = testing_utils::get_random_ports(3);
    let app = server_cmd()
        .env("RUST_BACKTRACE", "full")
        .env("RUST_LOG", "debug")
        .env("HTTP_PORT", ports[0].to_string())
        .env("HTTPS_PORT", ports[1].to_string())
        .env("HEALTHCHECK_PORT", ports[2].to_string())
        .env("CONFIG_FILE", input_file.path())
        .env("CERTS_DIR", certs_dir.path())
        .spawn()
        .unwrap();

    wait_for_server(ports[2]).await;
    info!("Server started");
    Context {
        app: ports[1],
        domain: domain.to_string(),
        connector,
        _app_server: app,
        _origin_server: mock_server,
    }
}

pub async fn after_each(_ctx: ()) {}

fn load_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    certs(&mut std::io::BufReader::new(File::open(path)?)).collect()
}

fn load_keys(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    private_key(&mut std::io::BufReader::new(File::open(path)?))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No keys found in file"))
}

async fn create_origin_server(folder: PathBuf, hostname: &str) -> (JoinHandle<()>, u16) {
    let mut tls_resolver = ResolvesServerCertUsingSni::new();
    let certs = load_certs(folder.join("cert.pem").as_path()).unwrap();
    let key = load_keys(folder.join("key.pem").as_path()).unwrap();
    let key = any_supported_type(&key).unwrap();
    let private_key = CertifiedKey::new(certs, key);
    tls_resolver.add(hostname, private_key).unwrap();
    let tls_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(tls_resolver));
    let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = tls_acceptor.accept(stream).await.unwrap();
            let mut buffer = BufReader::new(&mut stream);
            let request = buffer.read_request().await.unwrap();
            if request.path == "/hello" {
                let mut response = Response::new(StatusCode::OK);
                response.insert_header("Content-Length", "13");
                stream.write_response(&response).await.unwrap();
                stream.flush().await.unwrap();
                stream.write_all(b"Hello, world!").await.unwrap();
                stream.flush().await.unwrap();
                stream.shutdown().await.unwrap();
            } else {
                let response = Response::new(StatusCode::NOT_FOUND);
                stream.write_response(&response).await.unwrap();
                stream.flush().await.unwrap();
                stream.shutdown().await.unwrap();
            }
        }
    });
    (handle, addr.port())
}

fn create_config(origin_port: u16, temp: &TempDir, cert: &str, hostname: &str) -> ChildPath {
    let config = single_server_config(origin_port, cert, hostname);
    let file = temp.child("config.json");
    file.touch().unwrap();
    file.write_str(&config.to_string()).unwrap();
    debug!("Provided config: {}", config.to_string());
    file
}

fn setup_tls(domain: &str, temp: &TempDir) -> (ChildPath, TlsConnector) {
    let subject_alt_names = vec![domain.to_string()];
    let rcgen::CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(subject_alt_names).unwrap();
    let dir = temp.child("certs");
    dir.create_dir_all().unwrap();
    let domain_dir = dir.child(domain);
    domain_dir.create_dir_all().unwrap();
    let cert_file = domain_dir.child("cert.pem");
    cert_file.write_str(&cert.pem()).unwrap();
    let key_file = domain_dir.child("key.pem");
    key_file.write_str(&key_pair.serialize_pem()).unwrap();
    debug!("Generated cert: {}", read_to_string(cert_file).unwrap());
    debug!("Generated key: {}", read_to_string(key_file).unwrap());
    let mut root_cert_store = RootCertStore::empty();
    root_cert_store.add(cert.der().clone()).unwrap();

    let config = ClientConfig::builder()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    (dir, connector)
}

async fn wait_for_server(health_check: u16) {
    use testing_utils::surf;
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    loop {
        if let Ok(response) = surf::get(format!("http://127.0.0.1:{}", health_check)).await {
            debug!("Health check response: {:?}", response);
            if response.status() == 200 {
                break;
            }
        }
        interval.tick().await;
    }
    debug!("Health check passed");
}
